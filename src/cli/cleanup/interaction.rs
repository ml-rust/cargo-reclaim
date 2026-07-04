use std::collections::HashSet;
use std::io::{self, IsTerminal};

use cargo_reclaim::ScannerOptions;

use super::super::cleanup_assistant::{
    CleanupAssistantAction, CleanupAssistantMode, CleanupAssistantPage,
    CleanupAssistantStartOptions,
};
use super::super::cleanup_terminal::run_cleanup_terminal_assistant;
use super::super::target_report::{TargetsDiscovery, build_targets_report, normalize_for_dedupe};
use super::super::{CliError, OutputFormat};
use super::CleanupCommand;

#[derive(Debug, Clone)]
pub(super) struct ResolvedCleanupCommand {
    pub(super) command: CleanupCommand,
    pub(super) cancelled: bool,
}

pub(super) fn resolve_cleanup_assistant(
    command: &CleanupCommand,
) -> Result<ResolvedCleanupCommand, CliError> {
    let decision = cleanup_interaction_decision(command, cleanup_assistant_tty_available());
    let CleanupInteractionDecision::Assistant(request) = decision else {
        if matches!(decision, CleanupInteractionDecision::UsageError) {
            return Err(no_cleanup_selector_error());
        }
        return Ok(ResolvedCleanupCommand {
            command: command.clone(),
            cancelled: false,
        });
    };

    let report = build_cleanup_assistant_targets_report(command, request.target_selection)?;
    if report.targets.is_empty() {
        return Err(CliError::Usage(
            "cleanup found no target directories to select".to_string(),
        ));
    }
    let start_options = cleanup_assistant_start_options(command, &report, request)?;
    let selection = run_cleanup_terminal_assistant(&report, start_options)?;
    if selection.action == CleanupAssistantAction::Cancel {
        return Ok(ResolvedCleanupCommand {
            command: command.clone(),
            cancelled: true,
        });
    }

    let mut resolved = command.clone();
    resolved.prompt_selector = false;
    if command.all && !selection.target_selection_modified {
        resolved.selected_targets.clear();
    } else {
        resolved.selected_targets = selection.targets;
        resolved.all = false;
    }
    resolved.delete_target = selection.mode == CleanupAssistantMode::DeleteTarget;
    resolved.execute = selection.action == CleanupAssistantAction::Execute;
    resolved.validate_only = selection.action == CleanupAssistantAction::ValidateOnly;
    resolved.interactive_selection_modified = selection.target_selection_modified;
    Ok(ResolvedCleanupCommand {
        command: resolved,
        cancelled: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupInteractionDecision {
    NonInteractive,
    UsageError,
    Assistant(CleanupAssistantRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CleanupAssistantRequest {
    target_selection: CleanupAssistantTargetSelection,
    first_page: CleanupAssistantPage,
    minimum_page: CleanupAssistantPage,
    forced_mode: Option<CleanupAssistantMode>,
    forced_action: Option<CleanupAssistantAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupAssistantTargetSelection {
    User,
    Explicit,
    All,
}

fn cleanup_interaction_decision(
    command: &CleanupCommand,
    tty_available: bool,
) -> CleanupInteractionDecision {
    let has_selector = command.all || !command.selected_targets.is_empty();
    if command.output_format != OutputFormat::Terminal || !tty_available {
        return if has_selector {
            CleanupInteractionDecision::NonInteractive
        } else {
            CleanupInteractionDecision::UsageError
        };
    }

    let forced_action = cleanup_forced_action(command);
    if has_selector && forced_action.is_some() {
        return CleanupInteractionDecision::NonInteractive;
    }

    let forced_mode = command
        .delete_target
        .then_some(CleanupAssistantMode::DeleteTarget);
    let target_selection = if command.all {
        CleanupAssistantTargetSelection::All
    } else if command.selected_targets.is_empty() {
        CleanupAssistantTargetSelection::User
    } else {
        CleanupAssistantTargetSelection::Explicit
    };
    let (first_page, minimum_page) = if has_selector && command.delete_target {
        (CleanupAssistantPage::Action, CleanupAssistantPage::Action)
    } else if has_selector {
        (CleanupAssistantPage::Mode, CleanupAssistantPage::Mode)
    } else {
        (CleanupAssistantPage::Targets, CleanupAssistantPage::Targets)
    };

    CleanupInteractionDecision::Assistant(CleanupAssistantRequest {
        target_selection,
        first_page,
        minimum_page,
        forced_mode,
        forced_action,
    })
}

fn cleanup_forced_action(command: &CleanupCommand) -> Option<CleanupAssistantAction> {
    if command.execute {
        Some(CleanupAssistantAction::Execute)
    } else if command.validate_only {
        Some(CleanupAssistantAction::ValidateOnly)
    } else {
        None
    }
}

fn build_cleanup_assistant_targets_report(
    command: &CleanupCommand,
    target_selection: CleanupAssistantTargetSelection,
) -> Result<super::super::target_report::TargetsReport, CliError> {
    let (roots, scanner_options) = match target_selection {
        CleanupAssistantTargetSelection::Explicit => {
            let mut roots = command.roots.clone();
            roots.extend(command.selected_targets.iter().cloned());
            (roots, explicit_target_scanner_options(command))
        }
        CleanupAssistantTargetSelection::User | CleanupAssistantTargetSelection::All => {
            (command.roots.clone(), command.scanner_options.clone())
        }
    };
    build_targets_report(&TargetsDiscovery::new(
        roots,
        scanner_options,
        command.inventory_options.clone(),
        command.config_path.clone(),
        command.config_version,
    ))
}

fn cleanup_assistant_start_options(
    command: &CleanupCommand,
    report: &super::super::target_report::TargetsReport,
    request: CleanupAssistantRequest,
) -> Result<CleanupAssistantStartOptions, CliError> {
    let selected = match request.target_selection {
        CleanupAssistantTargetSelection::User => vec![false; report.targets.len()],
        CleanupAssistantTargetSelection::All => vec![true; report.targets.len()],
        CleanupAssistantTargetSelection::Explicit => explicit_target_selection(command, report)?,
    };
    Ok(CleanupAssistantStartOptions {
        selected,
        first_page: request.first_page,
        minimum_page: request.minimum_page,
        forced_mode: request.forced_mode,
        forced_action: request.forced_action,
    })
}

fn explicit_target_selection(
    command: &CleanupCommand,
    report: &super::super::target_report::TargetsReport,
) -> Result<Vec<bool>, CliError> {
    let selected_targets = command
        .selected_targets
        .iter()
        .map(|path| normalize_for_dedupe(path))
        .collect::<HashSet<_>>();
    let selected = report
        .targets
        .iter()
        .map(|target| selected_targets.contains(&normalize_for_dedupe(&target.path)))
        .collect::<Vec<_>>();
    let matched_count = selected.iter().filter(|selected| **selected).count();
    if matched_count == selected_targets.len() {
        Ok(selected)
    } else {
        Err(CliError::Usage(
            "selected target was not discovered; pass a root that contains it or the target path itself".to_string(),
        ))
    }
}

fn cleanup_assistant_tty_available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal()
}

fn no_cleanup_selector_error() -> CliError {
    CliError::Usage(
        "cleanup requires a selector: pass --all or --target <path>, or run no-selector cleanup from an interactive terminal".to_string(),
    )
}

fn explicit_target_scanner_options(command: &CleanupCommand) -> ScannerOptions {
    let mut scanner_options = command.scanner_options.clone();
    scanner_options.allow_name_only_targets = true;
    scanner_options
}

#[cfg(test)]
mod tests {
    use super::*;
    use cargo_reclaim::{InventoryOptions, PlannerOptions, PolicyKind, WholeTargetMode};
    use std::path::PathBuf;

    fn command_fixture() -> CleanupCommand {
        let scanner_options = ScannerOptions::default();
        CleanupCommand {
            roots: vec![PathBuf::from(".")],
            selected_targets: Vec::new(),
            all: false,
            delete_target: false,
            execute: false,
            validate_only: false,
            prompt_selector: true,
            interactive_selection_modified: false,
            output_format: OutputFormat::Terminal,
            policy: PolicyKind::Balanced,
            inventory_options: InventoryOptions {
                follow_symlinks: scanner_options.follow_symlinks,
                skipped_paths: scanner_options.skipped_paths.clone(),
                deep_target_scan: false,
                deep_directory_measurement: true,
            },
            scanner_options,
            planner_options: PlannerOptions {
                whole_target_mode: WholeTargetMode::Off,
                ..PlannerOptions::default()
            },
            config_path: None,
            config_version: None,
        }
    }

    #[test]
    fn explicit_target_tty_starts_assistant_at_mode_page() {
        let mut command = command_fixture();
        command.prompt_selector = false;
        command.selected_targets.push(PathBuf::from("target"));

        let decision = cleanup_interaction_decision(&command, true);

        assert_eq!(
            decision,
            CleanupInteractionDecision::Assistant(CleanupAssistantRequest {
                target_selection: CleanupAssistantTargetSelection::Explicit,
                first_page: CleanupAssistantPage::Mode,
                minimum_page: CleanupAssistantPage::Mode,
                forced_mode: None,
                forced_action: None,
            })
        );
    }

    #[test]
    fn explicit_target_delete_tty_starts_assistant_at_action_page() {
        let mut command = command_fixture();
        command.prompt_selector = false;
        command.selected_targets.push(PathBuf::from("target"));
        command.delete_target = true;

        let decision = cleanup_interaction_decision(&command, true);

        assert_eq!(
            decision,
            CleanupInteractionDecision::Assistant(CleanupAssistantRequest {
                target_selection: CleanupAssistantTargetSelection::Explicit,
                first_page: CleanupAssistantPage::Action,
                minimum_page: CleanupAssistantPage::Action,
                forced_mode: Some(CleanupAssistantMode::DeleteTarget),
                forced_action: None,
            })
        );
    }

    #[test]
    fn explicit_target_with_action_flags_is_noninteractive() {
        let updates: [fn(&mut CleanupCommand); 2] = [
            |command: &mut CleanupCommand| command.execute = true,
            |command: &mut CleanupCommand| command.validate_only = true,
        ];
        for update in updates {
            let mut command = command_fixture();
            command.prompt_selector = false;
            command.selected_targets.push(PathBuf::from("target"));
            update(&mut command);

            assert_eq!(
                cleanup_interaction_decision(&command, true),
                CleanupInteractionDecision::NonInteractive
            );
        }
    }

    #[test]
    fn all_tty_mirrors_explicit_target_start_pages() {
        let mut command = command_fixture();
        command.prompt_selector = false;
        command.all = true;

        assert_eq!(
            cleanup_interaction_decision(&command, true),
            CleanupInteractionDecision::Assistant(CleanupAssistantRequest {
                target_selection: CleanupAssistantTargetSelection::All,
                first_page: CleanupAssistantPage::Mode,
                minimum_page: CleanupAssistantPage::Mode,
                forced_mode: None,
                forced_action: None,
            })
        );

        command.delete_target = true;
        assert_eq!(
            cleanup_interaction_decision(&command, true),
            CleanupInteractionDecision::Assistant(CleanupAssistantRequest {
                target_selection: CleanupAssistantTargetSelection::All,
                first_page: CleanupAssistantPage::Action,
                minimum_page: CleanupAssistantPage::Action,
                forced_mode: Some(CleanupAssistantMode::DeleteTarget),
                forced_action: None,
            })
        );
    }

    #[test]
    fn all_with_action_flags_is_noninteractive() {
        let updates: [fn(&mut CleanupCommand); 2] = [
            |command: &mut CleanupCommand| command.execute = true,
            |command: &mut CleanupCommand| command.validate_only = true,
        ];
        for update in updates {
            let mut command = command_fixture();
            command.prompt_selector = false;
            command.all = true;
            update(&mut command);

            assert_eq!(
                cleanup_interaction_decision(&command, true),
                CleanupInteractionDecision::NonInteractive
            );
        }
    }

    #[test]
    fn no_selector_non_tty_or_json_is_usage_error() {
        let command = command_fixture();
        assert_eq!(
            cleanup_interaction_decision(&command, false),
            CleanupInteractionDecision::UsageError
        );

        let mut json_command = command_fixture();
        json_command.output_format = OutputFormat::Json;
        assert_eq!(
            cleanup_interaction_decision(&json_command, true),
            CleanupInteractionDecision::UsageError
        );
    }
}
