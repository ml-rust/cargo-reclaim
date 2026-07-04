use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    ApplyReport, ArtifactClass, InventoryOptions, Plan, PlanAction, PlanCommandKind, PlanEntry,
    PlanInput, PlanInvocation, PlannerOptions, PolicyKind, SavePlanOptions, ScannerOptions,
    WholeTargetMode, execute_persisted_plan_apply, load_config_from_path, persist_plan,
    snapshot_path, validate_persisted_plan_for_apply,
};
use serde_json::json;

use super::apply::apply_status_label;
use super::target_report::{
    TargetListEntry, TargetsDiscovery, TargetsReport, build_targets_report, evidence_label,
    human_bytes, normalize_for_dedupe, problem_json, skip_json, target_json,
};
use super::{
    CliError, OutputFormat, inline_config_path, inline_ignore_path, inline_skip_path, next_path,
};

const CLEAN_PLAN_EXPIRY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub(in crate::cli) enum TargetsCommand {
    List(TargetsListCommand),
    Clean(TargetsCleanCommand),
}

#[derive(Debug)]
pub(in crate::cli) struct TargetsListCommand {
    discovery: TargetsDiscoveryCommand,
}

#[derive(Debug)]
pub(in crate::cli) struct TargetsCleanCommand {
    discovery: TargetsDiscoveryCommand,
    selected_targets: Vec<PathBuf>,
    interactive: bool,
    execute: bool,
}

#[derive(Debug)]
struct TargetsDiscoveryCommand {
    roots: Vec<PathBuf>,
    output_format: OutputFormat,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
}

impl TargetsDiscoveryCommand {
    fn report_discovery(&self) -> TargetsDiscovery {
        TargetsDiscovery::new(
            self.roots.clone(),
            self.scanner_options.clone(),
            self.inventory_options.clone(),
            self.config_path.clone(),
            self.config_version,
        )
    }
}

#[derive(Default)]
struct CleanParseOptions {
    selected_targets: Vec<PathBuf>,
    interactive: bool,
    execute: bool,
}

pub(in crate::cli) fn parse_targets_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<TargetsCommand, CliError> {
    let mut args = args.into_iter().collect::<Vec<_>>();
    let first = args.first().and_then(|arg| arg.to_str());
    let clean = first == Some("clean");
    let list = first == Some("list");
    if clean || list {
        args.remove(0);
    }

    if clean {
        parse_targets_clean_command(args).map(TargetsCommand::Clean)
    } else {
        parse_targets_list_command(args).map(TargetsCommand::List)
    }
}

fn parse_targets_list_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<TargetsListCommand, CliError> {
    Ok(TargetsListCommand {
        discovery: parse_targets_discovery(args, false)?.0,
    })
}

fn parse_targets_clean_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<TargetsCleanCommand, CliError> {
    let (discovery, clean_options) = parse_targets_discovery(args, true)?;
    if clean_options.selected_targets.is_empty() && !clean_options.interactive {
        return Err(CliError::Usage(
            "targets clean requires --target <path> or --interactive".to_string(),
        ));
    }

    Ok(TargetsCleanCommand {
        discovery,
        selected_targets: clean_options.selected_targets,
        interactive: clean_options.interactive,
        execute: clean_options.execute,
    })
}

fn parse_targets_discovery(
    args: impl IntoIterator<Item = OsString>,
    allow_clean_options: bool,
) -> Result<(TargetsDiscoveryCommand, CleanParseOptions), CliError> {
    let mut roots = Vec::new();
    let mut output_format = OutputFormat::Terminal;
    let mut config_path = None;
    let mut scanner_options = ScannerOptions::default();
    let mut cli_follow_symlinks = false;
    let mut cli_allow_name_only_targets = false;
    let mut cli_cross_filesystems = false;
    let mut clean_options = CleanParseOptions::default();
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
            "-h" | "--help" => return Err(CliError::Help(targets_usage())),
            "--" => {
                roots.extend(args.map(PathBuf::from));
                break;
            }
            "--config" => {
                config_path = Some(next_path(&mut args, "--config")?);
            }
            "--ignore" => {
                scanner_options
                    .ignored_paths
                    .push(next_path(&mut args, "--ignore")?);
            }
            "--skip" => {
                scanner_options
                    .skipped_paths
                    .push(next_path(&mut args, "--skip")?);
            }
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
            "--json" => output_format = OutputFormat::Json,
            "--target" if allow_clean_options => {
                clean_options
                    .selected_targets
                    .push(next_path(&mut args, "--target")?);
            }
            value if allow_clean_options && value.starts_with("--target=") => {
                let target = &value["--target=".len()..];
                if target.is_empty() {
                    return Err(CliError::Usage("--target requires a value".to_string()));
                }
                clean_options.selected_targets.push(PathBuf::from(target));
            }
            "--interactive" if allow_clean_options => clean_options.interactive = true,
            "--yes" if allow_clean_options => clean_options.execute = true,
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown targets option `{value}`")));
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }

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
        } else {
            roots.push(PathBuf::from("."));
        }
    }

    if let Some(config) = config {
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
    }

    let inventory_options = InventoryOptions {
        follow_symlinks: scanner_options.follow_symlinks,
        skipped_paths: scanner_options.skipped_paths.clone(),
        deep_target_scan: false,
        deep_directory_measurement: false,
    };

    Ok((
        TargetsDiscoveryCommand {
            roots,
            output_format,
            scanner_options,
            inventory_options,
            config_path,
            config_version,
        },
        clean_options,
    ))
}

pub(in crate::cli) fn run_targets_command(
    command: &TargetsCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command {
        TargetsCommand::List(command) => {
            let report = build_targets_report(&command.discovery.report_discovery())?;
            match command.discovery.output_format {
                OutputFormat::Terminal => write_targets_terminal(output, &report)?,
                OutputFormat::Json => write_targets_json(output, &report)?,
            }
        }
        TargetsCommand::Clean(command) => {
            let report = build_targets_report(&command.discovery.report_discovery())?;
            let selected = select_targets(command, &report)?;
            let apply_report = run_selected_target_cleanup(command, &report, selected)?;
            match command.discovery.output_format {
                OutputFormat::Terminal => {
                    write_targets_clean_terminal(output, command.execute, &apply_report)?
                }
                OutputFormat::Json => {
                    write_targets_clean_json(output, command.execute, &apply_report)?
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn select_targets(
    command: &TargetsCleanCommand,
    report: &TargetsReport,
) -> Result<Vec<TargetListEntry>, CliError> {
    let mut selected_paths = command.selected_targets.clone();
    if command.interactive {
        selected_paths.extend(prompt_for_target_selection(report)?);
    }

    let mut selected = Vec::new();
    for selected_path in selected_paths {
        let selected_key = normalize_for_dedupe(&selected_path);
        let Some(target) = report
            .targets
            .iter()
            .find(|target| normalize_for_dedupe(&target.path) == selected_key)
        else {
            return Err(CliError::Usage(format!(
                "selected target `{}` was not discovered; run `cargo-reclaim targets` with the same roots first",
                selected_path.display()
            )));
        };
        if !selected
            .iter()
            .any(|entry: &TargetListEntry| normalize_for_dedupe(&entry.path) == selected_key)
        {
            selected.push(target.clone());
        }
    }

    if selected.is_empty() {
        return Err(CliError::Usage("no targets selected".to_string()));
    }
    Ok(selected)
}

fn prompt_for_target_selection(report: &TargetsReport) -> Result<Vec<PathBuf>, CliError> {
    let mut stderr = io::stderr();
    writeln!(stderr, "cargo-reclaim targets clean interactive")?;
    writeln!(
        stderr,
        "Select whole target dirs by number; deletion still requires --yes."
    )?;
    writeln!(stderr)?;
    for (index, target) in report.targets.iter().enumerate() {
        writeln!(
            stderr,
            "{}\t{}\t{}\t{}",
            index + 1,
            human_bytes(target.size_bytes),
            evidence_label(&target.evidence),
            target.path.display()
        )?;
    }
    writeln!(stderr)?;
    writeln!(stderr, "Selection:")?;
    stderr.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("cancel") {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for token in trimmed
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
    {
        let index = token
            .parse::<usize>()
            .map_err(|_| CliError::Usage(format!("invalid target selection `{token}`")))?;
        if index == 0 || index > report.targets.len() {
            return Err(CliError::Usage(format!(
                "target selection `{token}` is out of range"
            )));
        }
        paths.push(report.targets[index - 1].path.clone());
    }
    Ok(paths)
}

fn run_selected_target_cleanup(
    command: &TargetsCleanCommand,
    report: &TargetsReport,
    selected: Vec<TargetListEntry>,
) -> Result<ApplyReport, CliError> {
    let now = SystemTime::now();
    let plan = selected_targets_plan(
        report.roots.clone(),
        selected,
        &command.discovery.inventory_options,
    )?;
    let planner_options = PlannerOptions {
        whole_target_mode: WholeTargetMode::DeleteConfirmed,
        ..PlannerOptions::default()
    };
    let mut invocation = PlanInvocation::new(
        PlanCommandKind::Plan,
        PolicyKind::Aggressive,
        &command.discovery.scanner_options,
        &command.discovery.inventory_options,
        &planner_options,
    );
    if let (Some(config_path), Some(config_version)) = (
        &command.discovery.config_path,
        command.discovery.config_version,
    ) {
        invocation = invocation.with_config(config_path, config_version);
    }
    let document = persist_plan(
        &plan,
        SavePlanOptions {
            created_at: now,
            expires_at: now.checked_add(CLEAN_PLAN_EXPIRY).ok_or_else(|| {
                CliError::Usage("targets clean plan expiry overflowed".to_string())
            })?,
            interactive_selection_modified: command.interactive,
            invocation,
        },
    )?;

    if command.execute {
        Ok(execute_persisted_plan_apply(&document, now)?)
    } else {
        Ok(validate_persisted_plan_for_apply(&document, now)?)
    }
}

fn selected_targets_plan(
    roots: Vec<PathBuf>,
    selected: Vec<TargetListEntry>,
    inventory_options: &InventoryOptions,
) -> Result<Plan, CliError> {
    let mut entries = Vec::new();
    for target in selected {
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
    Ok(Plan::new(PlanInput::new(roots)?, entries))
}

fn write_targets_terminal(output: &mut impl Write, report: &TargetsReport) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim targets")?;
    writeln!(output, "read-only; no files were deleted or modified")?;
    writeln!(
        output,
        "roots: {}",
        report
            .roots
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    if let Some(config_path) = &report.config_path {
        writeln!(
            output,
            "config: {}{}",
            config_path.display(),
            report
                .config_version
                .map(|version| format!(" (version {version})"))
                .unwrap_or_default()
        )?;
    }
    writeln!(
        output,
        "targets: {} ({})",
        report.targets.len(),
        human_bytes(report.total_size_bytes)
    )?;
    if !report.problems.is_empty() {
        writeln!(output, "problems: {}", report.problems.len())?;
    }
    if !report.skipped_paths.is_empty() {
        writeln!(output, "skipped scan paths: {}", report.skipped_paths.len())?;
    }

    if !report.targets.is_empty() {
        writeln!(output)?;
        writeln!(output, "index\tsize\tbytes\tevidence\tpath")?;
        for (index, target) in report.targets.iter().enumerate() {
            writeln!(
                output,
                "{}\t{}\t{}\t{}\t{}",
                index + 1,
                human_bytes(target.size_bytes),
                target.size_bytes,
                evidence_label(&target.evidence),
                target.path.display()
            )?;
        }
    }

    if !report.problems.is_empty() {
        writeln!(output)?;
        writeln!(output, "problems")?;
        for problem in &report.problems {
            writeln!(output, "{}\t{}", problem.message, problem.path.display())?;
        }
    }

    Ok(())
}

fn write_targets_clean_terminal(
    output: &mut impl Write,
    execute: bool,
    report: &ApplyReport,
) -> Result<(), CliError> {
    if execute {
        writeln!(output, "cargo-reclaim targets clean execution")?;
        writeln!(
            output,
            "execution mode; selected target dirs were revalidated before deletion"
        )?;
    } else {
        writeln!(output, "cargo-reclaim targets clean validation")?;
        writeln!(
            output,
            "validation only; pass --yes to delete selected target dirs"
        )?;
    }
    writeln!(output, "targets: {}", report.totals.entry_count)?;
    writeln!(output, "would delete: {}", report.totals.would_delete_count)?;
    writeln!(
        output,
        "would delete bytes: {}",
        report.totals.would_delete_bytes
    )?;
    writeln!(output, "deleted: {}", report.totals.applied_count)?;
    writeln!(output, "deleted bytes: {}", report.totals.applied_bytes)?;
    writeln!(output, "failures: {}", report.totals.failed_count)?;
    for entry in &report.entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}",
            apply_status_label(entry.status),
            entry.size_bytes,
            entry.deleted_bytes.unwrap_or(0),
            entry.path
        )?;
    }
    Ok(())
}

fn write_targets_json(output: &mut impl Write, report: &TargetsReport) -> Result<(), CliError> {
    serde_json::to_writer_pretty(
        &mut *output,
        &json!({
            "command": "targets",
            "schema_version": 1,
            "roots": report.roots,
            "config_path": report.config_path,
            "config_version": report.config_version,
            "totals": {
                "target_count": report.targets.len(),
                "total_size_bytes": report.total_size_bytes,
                "skipped_path_count": report.skipped_paths.len(),
                "problem_count": report.problems.len(),
            },
            "targets": report.targets.iter().map(target_json).collect::<Vec<_>>(),
            "skipped_paths": report.skipped_paths.iter().map(skip_json).collect::<Vec<_>>(),
            "problems": report.problems.iter().map(problem_json).collect::<Vec<_>>(),
        }),
    )?;
    writeln!(output)?;
    Ok(())
}

fn write_targets_clean_json(
    output: &mut impl Write,
    execute: bool,
    report: &ApplyReport,
) -> Result<(), CliError> {
    serde_json::to_writer_pretty(
        &mut *output,
        &json!({
            "command": "targets clean",
            "dry_run": !execute,
            "plan_id": report.plan_id.as_str(),
            "totals": {
                "target_count": report.totals.entry_count,
                "would_delete_count": report.totals.would_delete_count,
                "applied_count": report.totals.applied_count,
                "failed_count": report.totals.failed_count,
                "would_delete_bytes": report.totals.would_delete_bytes,
                "applied_bytes": report.totals.applied_bytes,
            },
            "entries": report.entries.iter().map(|entry| {
                json!({
                    "path": entry.path,
                    "status": apply_status_label(entry.status),
                    "size_bytes": entry.size_bytes,
                    "deleted_bytes": entry.deleted_bytes,
                    "reason": entry.reason,
                })
            }).collect::<Vec<_>>(),
        }),
    )?;
    writeln!(output)?;
    Ok(())
}

fn targets_usage() -> String {
    "usage: cargo-reclaim targets [list] [OPTIONS] [ROOT ...]\n       cargo-reclaim targets clean (--target <path>|--interactive) [--yes] [OPTIONS] [ROOT ...]".to_string()
}
