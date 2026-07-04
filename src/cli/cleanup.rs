use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    ActiveObservationProvider, ApplyReport, ArtifactClass,
    BuildPlanFromScanItemsWithProviderRequest, InventoryOptions, Plan, PlanAction, PlanCommandKind,
    PlanEntry, PlanInput, PlanInvocation, PlannerOptions, PolicyKind, SavePlanOptions, ScanItem,
    ScannerOptions, TargetCandidateKind, TargetEvidence, WholeTargetMode,
    build_plan_from_roots_with_active_observation_provider,
    build_plan_from_scan_items_with_active_observation_provider, execute_persisted_plan_apply,
    load_config_from_path, persist_plan, resolve_command_toolchain_hash_options, scan_roots,
    snapshot_path, validate_persisted_plan_for_apply,
};

use super::apply::write_apply_report_with_command;
use super::cleanup_assistant::{
    CleanupAssistantAction, CleanupAssistantMode, CleanupAssistantPage,
    CleanupAssistantStartOptions,
};
use super::cleanup_terminal::run_cleanup_terminal_assistant;
use super::persistence::{parse_days, parse_duration, parse_size};
use super::target_report::{
    TargetsDiscovery, build_targets_report, is_cleanable_cargo_target, normalize_for_dedupe,
};
use super::{
    CliError, OutputFormat, inline_config_path, inline_ignore_path, inline_skip_path, next_path,
    next_value, parse_policy, parse_toolchain_name, parse_u64,
};

const CLEANUP_PLAN_EXPIRY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub(super) struct CleanupCommand {
    roots: Vec<PathBuf>,
    selected_targets: Vec<PathBuf>,
    all: bool,
    delete_target: bool,
    execute: bool,
    validate_only: bool,
    prompt_selector: bool,
    interactive_selection_modified: bool,
    output_format: OutputFormat,
    policy: PolicyKind,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    planner_options: PlannerOptions,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
}

pub(super) fn parse_cleanup_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CleanupCommand, CliError> {
    let mut roots = Vec::new();
    let mut selected_targets = Vec::new();
    let mut all = false;
    let mut delete_target = false;
    let mut execute = false;
    let mut validation_alias = false;
    let mut output_format = OutputFormat::Terminal;
    let mut policy = None;
    let mut config_path = None;
    let mut scanner_options = ScannerOptions::default();
    let mut planner_options = PlannerOptions {
        whole_target_mode: WholeTargetMode::Off,
        ..PlannerOptions::default()
    };
    let mut cli_follow_symlinks = false;
    let mut cli_allow_name_only_targets = false;
    let mut cli_cross_filesystems = false;
    let mut cli_recent_write_keep_window = false;
    let mut cli_keep_size = false;
    let mut cli_keep_rustc_hashes = false;
    let mut cli_keep_installed_toolchains = false;
    let mut cli_keep_toolchains = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(ignore_path) = inline_ignore_path(&arg)? {
            scanner_options.ignored_paths.push(ignore_path);
            continue;
        }
        if let Some(skip_path) = inline_skip_path(&arg)? {
            scanner_options.skipped_paths.push(skip_path);
            continue;
        }
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            roots.push(PathBuf::from(arg));
            continue;
        };

        match arg_text {
            "-h" | "--help" => return Err(CliError::Help(cleanup_usage())),
            "--" => {
                roots.extend(args.map(PathBuf::from));
                break;
            }
            "--all" => all = true,
            "--target" => selected_targets.push(next_path(&mut args, "--target")?),
            value if value.starts_with("--target=") => {
                let target = &value["--target=".len()..];
                if target.is_empty() {
                    return Err(CliError::Usage("--target requires a value".to_string()));
                }
                selected_targets.push(PathBuf::from(target));
            }
            "--delete-target" => delete_target = true,
            "--yes" => execute = true,
            "--dry-run" | "--validate" => validation_alias = true,
            "--json" => output_format = OutputFormat::Json,
            "--policy" => {
                policy = Some(parse_policy(&next_value(&mut args, "--policy")?)?);
            }
            value if value.starts_with("--policy=") => {
                policy = Some(parse_policy(&value["--policy=".len()..])?);
            }
            "--config" => config_path = Some(next_path(&mut args, "--config")?),
            "--ignore" => scanner_options
                .ignored_paths
                .push(next_path(&mut args, "--ignore")?),
            "--skip" => scanner_options
                .skipped_paths
                .push(next_path(&mut args, "--skip")?),
            "--allow-name-only-targets" => {
                scanner_options.allow_name_only_targets = true;
                cli_allow_name_only_targets = true;
            }
            "--follow-symlinks" => {
                scanner_options.follow_symlinks = true;
                cli_follow_symlinks = true;
            }
            "--cross-filesystems" => {
                scanner_options.cross_filesystems = true;
                cli_cross_filesystems = true;
            }
            "--keep-recent-writes" => {
                planner_options.recent_write_keep_window = Some(parse_duration(&next_value(
                    &mut args,
                    "--keep-recent-writes",
                )?)?);
                cli_recent_write_keep_window = true;
            }
            value if value.starts_with("--keep-recent-writes=") => {
                planner_options.recent_write_keep_window =
                    Some(parse_duration(&value["--keep-recent-writes=".len()..])?);
                cli_recent_write_keep_window = true;
            }
            "--keep-days" => {
                planner_options.recent_write_keep_window =
                    Some(parse_days(&next_value(&mut args, "--keep-days")?)?);
                cli_recent_write_keep_window = true;
            }
            value if value.starts_with("--keep-days=") => {
                planner_options.recent_write_keep_window =
                    Some(parse_days(&value["--keep-days=".len()..])?);
                cli_recent_write_keep_window = true;
            }
            "--keep-size" => {
                planner_options.keep_size_bytes =
                    Some(parse_size(&next_value(&mut args, "--keep-size")?)?);
                cli_keep_size = true;
            }
            value if value.starts_with("--keep-size=") => {
                planner_options.keep_size_bytes = Some(parse_size(&value["--keep-size=".len()..])?);
                cli_keep_size = true;
            }
            "--keep-rustc-hash" => {
                planner_options
                    .keep_rustc_hashes
                    .push(parse_u64(&next_value(&mut args, "--keep-rustc-hash")?)?);
                cli_keep_rustc_hashes = true;
            }
            value if value.starts_with("--keep-rustc-hash=") => {
                planner_options
                    .keep_rustc_hashes
                    .push(parse_u64(&value["--keep-rustc-hash=".len()..])?);
                cli_keep_rustc_hashes = true;
            }
            "--keep-installed-toolchains" => {
                planner_options.keep_installed_toolchains = true;
                cli_keep_installed_toolchains = true;
            }
            "--keep-toolchain" => {
                planner_options.keep_toolchains.push(parse_toolchain_name(
                    next_value(&mut args, "--keep-toolchain")?,
                    "--keep-toolchain",
                )?);
                cli_keep_toolchains = true;
            }
            value if value.starts_with("--keep-toolchain=") => {
                planner_options.keep_toolchains.push(parse_toolchain_name(
                    value["--keep-toolchain=".len()..].to_string(),
                    "--keep-toolchain",
                )?);
                cli_keep_toolchains = true;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown cleanup option `{value}`")));
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }

    if execute && validation_alias {
        return Err(CliError::Usage(
            "--dry-run/--validate conflicts with --yes".to_string(),
        ));
    }
    if all && !selected_targets.is_empty() {
        return Err(CliError::Usage(
            "--all conflicts with --target; choose one cleanup selector".to_string(),
        ));
    }
    let prompt_selector = !all && selected_targets.is_empty();

    let config = config_path
        .as_ref()
        .map(load_config_from_path)
        .transpose()?;
    let config_version = config.as_ref().map(|config| config.version);

    if roots.is_empty() {
        if let Some(config_roots) = config
            .as_ref()
            .filter(|config| !config.roots.is_empty())
            .map(|config| config.roots.clone())
        {
            roots = config_roots;
        } else if all || prompt_selector {
            roots.push(PathBuf::from("."));
        }
    }

    let policy = match policy {
        Some(policy) => policy,
        None => config
            .as_ref()
            .and_then(|config| config.policy.as_deref())
            .map(parse_policy)
            .transpose()?
            .unwrap_or(PolicyKind::Balanced),
    };

    if let Some(config) = config {
        let config_keep_rustc_hashes = config.keep_rustc_hashes;
        let config_keep_installed_toolchains = config.keep_installed_toolchains;
        let config_keep_toolchains = config.keep_toolchains;
        let mut ignored_paths = config.ignored_paths;
        ignored_paths.extend(scanner_options.ignored_paths);
        scanner_options.ignored_paths = ignored_paths;
        let mut skipped_paths = config.skipped_paths;
        skipped_paths.extend(scanner_options.skipped_paths);
        scanner_options.skipped_paths = skipped_paths;

        if !cli_follow_symlinks && let Some(follow_symlinks) = config.scanner.follow_symlinks {
            scanner_options.follow_symlinks = follow_symlinks;
        }
        if !cli_allow_name_only_targets
            && let Some(allow_name_only_targets) = config.scanner.allow_name_only_targets
        {
            scanner_options.allow_name_only_targets = allow_name_only_targets;
        }
        if !cli_cross_filesystems && let Some(cross_filesystems) = config.scanner.cross_filesystems
        {
            scanner_options.cross_filesystems = cross_filesystems;
        }
        if !cli_recent_write_keep_window {
            planner_options.recent_write_keep_window = config.recent_write_keep_window;
        }
        if !cli_keep_size {
            planner_options.keep_size_bytes = config.keep_size_bytes;
        }
        planner_options.target_size_goal_bytes = config.policy_thresholds.target_size_goal_bytes;
        planner_options.target_free_disk_bytes = config.background.target_free_disk_bytes;
        if !cli_keep_rustc_hashes {
            planner_options.keep_rustc_hashes = config_keep_rustc_hashes;
        }
        if !cli_keep_installed_toolchains {
            planner_options.keep_installed_toolchains = config_keep_installed_toolchains;
        }
        if !cli_keep_toolchains {
            planner_options.keep_toolchains = config_keep_toolchains;
        }
    }
    planner_options.whole_target_mode = WholeTargetMode::Off;

    let inventory_options = InventoryOptions {
        follow_symlinks: scanner_options.follow_symlinks,
        skipped_paths: scanner_options.skipped_paths.clone(),
        deep_target_scan: false,
        deep_directory_measurement: true,
    };

    Ok(CleanupCommand {
        roots,
        selected_targets,
        all,
        delete_target,
        execute,
        validate_only: validation_alias,
        prompt_selector,
        interactive_selection_modified: false,
        output_format,
        policy,
        scanner_options,
        inventory_options,
        planner_options,
        config_path,
        config_version,
    })
}

pub(super) fn run_cleanup_command(
    command: &CleanupCommand,
    output: &mut impl Write,
    active_observation_provider: &impl ActiveObservationProvider,
) -> Result<ExitCode, CliError> {
    let command = resolve_cleanup_assistant(command)?;
    if command.cancelled {
        writeln!(output, "cargo-reclaim cleanup cancelled")?;
        return Ok(ExitCode::SUCCESS);
    }

    let report = if command.command.delete_target {
        run_whole_target_cleanup(&command.command)?
    } else {
        run_smart_trim_cleanup(&command.command, active_observation_provider)?
    };
    let exit_code = if report.totals.failed_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    write_apply_report_with_command(output, &report, command.command.output_format, "cleanup")?;
    Ok(exit_code)
}

struct ResolvedCleanupCommand {
    command: CleanupCommand,
    cancelled: bool,
}

fn resolve_cleanup_assistant(command: &CleanupCommand) -> Result<ResolvedCleanupCommand, CliError> {
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
) -> Result<super::target_report::TargetsReport, CliError> {
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
    report: &super::target_report::TargetsReport,
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
    report: &super::target_report::TargetsReport,
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

fn run_smart_trim_cleanup(
    command: &CleanupCommand,
    active_observation_provider: &impl ActiveObservationProvider,
) -> Result<ApplyReport, CliError> {
    let mut planner_options = command.planner_options.clone();
    planner_options.whole_target_mode = WholeTargetMode::Off;
    resolve_command_toolchain_hash_options(&mut planner_options)?;
    let plan_roots = smart_trim_plan_roots(command);
    let scanner_options = if command.selected_targets.is_empty() {
        command.scanner_options.clone()
    } else {
        explicit_target_scanner_options(command)
    };
    let now = SystemTime::now();
    let mut plan = if command.selected_targets.is_empty() {
        build_plan_from_roots_with_active_observation_provider(
            plan_roots.clone(),
            command.policy,
            &scanner_options,
            &command.inventory_options,
            &planner_options,
            active_observation_provider,
            now,
        )?
    } else {
        let items = explicit_target_scan_items(plan_roots.clone(), &scanner_options, command)?;
        build_plan_from_scan_items_with_active_observation_provider(
            BuildPlanFromScanItemsWithProviderRequest {
                input: PlanInput::new(plan_roots)?,
                policy: command.policy,
                items,
                scanner_options: &scanner_options,
                inventory_options: &command.inventory_options,
                planner_options: &planner_options,
                active_observation_provider,
                now,
            },
        )?
    };
    if !command.selected_targets.is_empty() {
        plan = filter_plan_to_selected_targets(plan, &command.selected_targets);
    }
    apply_persisted_plan(
        command,
        &plan,
        command.policy,
        &scanner_options,
        &planner_options,
        now,
    )
}

fn smart_trim_plan_roots(command: &CleanupCommand) -> Vec<PathBuf> {
    if command.selected_targets.is_empty() {
        return command.roots.clone();
    }

    let mut roots = command.roots.clone();
    roots.extend(command.selected_targets.iter().cloned());
    roots
}

fn run_whole_target_cleanup(command: &CleanupCommand) -> Result<ApplyReport, CliError> {
    let now = SystemTime::now();
    let scanner_options = if command.selected_targets.is_empty() {
        command.scanner_options.clone()
    } else {
        explicit_target_scanner_options(command)
    };
    let selected = discover_selected_whole_targets(command, &scanner_options)?;
    let plan = selected_targets_plan(command.roots.clone(), selected, &command.inventory_options)?;
    let planner_options = PlannerOptions {
        whole_target_mode: WholeTargetMode::DeleteConfirmed,
        ..PlannerOptions::default()
    };
    apply_persisted_plan(
        command,
        &plan,
        PolicyKind::Aggressive,
        &scanner_options,
        &planner_options,
        now,
    )
}

fn explicit_target_scanner_options(command: &CleanupCommand) -> ScannerOptions {
    let mut scanner_options = command.scanner_options.clone();
    scanner_options.allow_name_only_targets = true;
    scanner_options
}

fn explicit_target_scan_items(
    roots: Vec<PathBuf>,
    scanner_options: &ScannerOptions,
    command: &CleanupCommand,
) -> Result<Vec<ScanItem>, CliError> {
    let selected = command
        .selected_targets
        .iter()
        .map(|path| normalize_for_dedupe(path))
        .collect::<HashSet<_>>();
    let mut items = scan_roots(roots, scanner_options)?;
    for item in &mut items {
        let ScanItem::TargetCandidate(candidate) = item else {
            continue;
        };
        if candidate.kind == TargetCandidateKind::CargoTargetDir
            && selected.contains(&normalize_for_dedupe(&candidate.path))
            && candidate
                .evidence
                .as_ref()
                .is_some_and(TargetEvidence::is_weak_name_only)
        {
            candidate.evidence = Some(TargetEvidence::configured_path("explicit --target")?);
        }
    }
    Ok(items)
}

fn filter_plan_to_selected_targets(plan: Plan, selected_targets: &[PathBuf]) -> Plan {
    let selected_targets = selected_targets
        .iter()
        .map(|path| normalize_for_dedupe(path))
        .collect::<Vec<_>>();
    let entries = plan
        .entries
        .into_iter()
        .filter(|entry| is_under_selected_target(&entry.snapshot.path, &selected_targets))
        .collect::<Vec<_>>();
    let skipped_paths = plan
        .skipped_paths
        .into_iter()
        .filter(|skip| is_under_selected_target(&skip.path, &selected_targets))
        .collect::<Vec<_>>();

    Plan::with_skipped_paths(plan.input, entries, skipped_paths)
}

fn is_under_selected_target(path: &Path, selected_targets: &[PathBuf]) -> bool {
    let path = normalize_for_dedupe(path);
    selected_targets
        .iter()
        .any(|target| path == *target || path.starts_with(target))
}

fn apply_persisted_plan(
    command: &CleanupCommand,
    plan: &Plan,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    planner_options: &PlannerOptions,
    now: SystemTime,
) -> Result<ApplyReport, CliError> {
    let mut invocation = PlanInvocation::new(
        PlanCommandKind::Plan,
        policy,
        scanner_options,
        &command.inventory_options,
        planner_options,
    );
    if let (Some(config_path), Some(config_version)) =
        (&command.config_path, command.config_version)
    {
        invocation = invocation.with_config(config_path, config_version);
    }
    let document = persist_plan(
        plan,
        SavePlanOptions {
            created_at: now,
            expires_at: now
                .checked_add(CLEANUP_PLAN_EXPIRY)
                .ok_or_else(|| CliError::Usage("cleanup plan expiry overflowed".to_string()))?,
            interactive_selection_modified: command.interactive_selection_modified,
            invocation,
        },
    )?;

    if command.execute {
        Ok(execute_persisted_plan_apply(&document, now)?)
    } else {
        Ok(validate_persisted_plan_for_apply(&document, now)?)
    }
}

#[derive(Clone)]
struct WholeTargetSelection {
    path: PathBuf,
    evidence: TargetEvidence,
}

fn discover_selected_whole_targets(
    command: &CleanupCommand,
    scanner_options: &ScannerOptions,
) -> Result<Vec<WholeTargetSelection>, CliError> {
    let discovery_roots = whole_target_discovery_roots(command);
    let items = scan_roots(discovery_roots, scanner_options)?;
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let ScanItem::TargetCandidate(candidate) = item else {
            continue;
        };
        if candidate.kind != TargetCandidateKind::CargoTargetDir
            || !is_cleanable_cargo_target(&candidate)
        {
            continue;
        }
        if seen.insert(normalize_for_dedupe(&candidate.path)) {
            let evidence = candidate.evidence.ok_or_else(|| {
                CliError::Usage(format!(
                    "target `{}` was discovered without target evidence",
                    candidate.path.display()
                ))
            })?;
            targets.push(WholeTargetSelection {
                path: candidate.path,
                evidence,
            });
        }
    }

    if command.all {
        if targets.is_empty() {
            return Err(CliError::Usage(
                "cleanup found no target directories to delete".to_string(),
            ));
        }
        return Ok(targets);
    }

    let mut selected = Vec::new();
    for selected_path in &command.selected_targets {
        let selected_key = normalize_for_dedupe(selected_path);
        let Some(target) = targets
            .iter()
            .find(|target| normalize_for_dedupe(&target.path) == selected_key)
        else {
            return Err(CliError::Usage(format!(
                "selected target `{}` was not discovered; pass a root that contains it or the target path itself",
                selected_path.display()
            )));
        };
        if !selected
            .iter()
            .any(|entry: &WholeTargetSelection| normalize_for_dedupe(&entry.path) == selected_key)
        {
            selected.push(target.clone());
        }
    }
    Ok(selected)
}

fn whole_target_discovery_roots(command: &CleanupCommand) -> Vec<PathBuf> {
    let mut roots = command.roots.clone();
    roots.extend(command.selected_targets.iter().cloned());
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    roots
}

fn selected_targets_plan(
    roots: Vec<PathBuf>,
    selected: Vec<WholeTargetSelection>,
    inventory_options: &InventoryOptions,
) -> Result<Plan, CliError> {
    let mut entries = Vec::new();
    let mut input_roots = roots;
    for target in selected {
        if input_roots.is_empty() {
            input_roots.push(target.path.clone());
        }
        let entry = PlanEntry::new(
            snapshot_path(&target.path, inventory_options)?,
            ArtifactClass::WholeTarget,
            target.evidence,
            PlanAction::Delete,
            "selected whole-target cleanup",
            false,
        )?;
        entries.push(entry);
    }
    Ok(Plan::new(PlanInput::new(input_roots)?, entries))
}

fn cleanup_usage() -> String {
    "usage: cargo-reclaim cleanup [--all|--target <path>] [--delete-target] [--yes] [OPTIONS] [ROOT ...]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
