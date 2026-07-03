use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    ApplyEntryStatus, ApplyReport, ArtifactClass, InventoryOptions, PathKind, Plan, PlanAction,
    PlanCommandKind, PlanEntry, PlanInput, PlanInvocation, PlannerOptions, PolicyKind,
    ReclaimError, SavePlanOptions, ScanItem, ScanSkipReason, ScannerOptions, TargetCandidate,
    TargetCandidateKind, TargetEvidence, WholeTargetMode, execute_persisted_plan_apply,
    load_config_from_path, persist_plan, scan_roots, snapshot_path,
    validate_persisted_plan_for_apply,
};
use serde_json::json;

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
            let report = build_targets_report(&command.discovery)?;
            match command.discovery.output_format {
                OutputFormat::Terminal => write_targets_terminal(output, &report)?,
                OutputFormat::Json => write_targets_json(output, &report)?,
            }
        }
        TargetsCommand::Clean(command) => {
            let report = build_targets_report(&command.discovery)?;
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

struct TargetsReport {
    roots: Vec<PathBuf>,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
    total_size_bytes: u64,
    targets: Vec<TargetListEntry>,
    skipped_paths: Vec<TargetListSkip>,
    problems: Vec<TargetListProblem>,
}

#[derive(Clone)]
struct TargetListEntry {
    path: PathBuf,
    size_bytes: u64,
    path_kind: PathKind,
    evidence: TargetEvidence,
}

struct TargetListSkip {
    path: PathBuf,
    reason: ScanSkipReason,
    message: Option<String>,
}

struct TargetListProblem {
    path: PathBuf,
    message: String,
}

fn build_targets_report(command: &TargetsDiscoveryCommand) -> Result<TargetsReport, CliError> {
    let items = scan_roots(command.roots.iter().cloned(), &command.scanner_options)?;
    let mut seen_targets = HashSet::new();
    let mut targets = Vec::new();
    let mut skipped_paths = Vec::new();
    let mut problems = Vec::new();

    for item in items {
        match item {
            ScanItem::TargetCandidate(candidate) => {
                if candidate.kind != TargetCandidateKind::CargoTargetDir {
                    continue;
                }
                if !is_cleanable_cargo_target(&candidate) {
                    continue;
                }
                if !seen_targets.insert(normalize_for_dedupe(&candidate.path)) {
                    continue;
                }
                match target_entry(candidate, &command.inventory_options) {
                    Ok(entry) => targets.push(entry),
                    Err(problem) => problems.push(problem),
                }
            }
            ScanItem::Skipped(skip) => skipped_paths.push(TargetListSkip {
                message: skip_message(&skip.reason),
                path: skip.path,
                reason: skip.reason,
            }),
            ScanItem::CargoProject(_) => {}
        }
    }

    targets.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    skipped_paths.sort_by(|left, right| left.path.cmp(&right.path));
    problems.sort_by(|left, right| left.path.cmp(&right.path));
    let total_size_bytes = targets.iter().fold(0_u64, |total, target| {
        total.saturating_add(target.size_bytes)
    });

    Ok(TargetsReport {
        roots: command.roots.clone(),
        config_path: command.config_path.clone(),
        config_version: command.config_version,
        total_size_bytes,
        targets,
        skipped_paths,
        problems,
    })
}

fn target_entry(
    candidate: TargetCandidate,
    inventory_options: &InventoryOptions,
) -> Result<TargetListEntry, TargetListProblem> {
    let snapshot =
        snapshot_path(&candidate.path, inventory_options).map_err(|error| TargetListProblem {
            path: candidate.path.clone(),
            message: inventory_problem_message(error),
        })?;

    let evidence = candidate.evidence.ok_or_else(|| TargetListProblem {
        path: candidate.path.clone(),
        message: "target candidate has no evidence".to_string(),
    })?;

    Ok(TargetListEntry {
        path: snapshot.path,
        size_bytes: snapshot.size_bytes,
        path_kind: snapshot.path_kind,
        evidence,
    })
}

fn is_cleanable_cargo_target(candidate: &TargetCandidate) -> bool {
    match candidate.evidence.as_ref() {
        Some(TargetEvidence::ConfiguredPath { .. })
        | Some(TargetEvidence::ProjectContext { .. }) => true,
        Some(TargetEvidence::StrongMarker { .. }) | Some(TargetEvidence::WeakNameOnly { .. }) => {
            candidate
                .path
                .file_name()
                .is_some_and(|name| name == "target")
        }
        None => false,
    }
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

fn target_json(target: &TargetListEntry) -> serde_json::Value {
    json!({
        "path": target.path,
        "size_bytes": target.size_bytes,
        "size": human_bytes(target.size_bytes),
        "path_kind": path_kind_label(target.path_kind),
        "evidence": evidence_json(&target.evidence),
    })
}

fn skip_json(skip: &TargetListSkip) -> serde_json::Value {
    json!({
        "path": skip.path,
        "reason": skip_reason_label(&skip.reason),
        "message": skip.message,
    })
}

fn problem_json(problem: &TargetListProblem) -> serde_json::Value {
    json!({
        "path": problem.path,
        "message": problem.message,
    })
}

fn evidence_json(evidence: &TargetEvidence) -> serde_json::Value {
    match evidence {
        TargetEvidence::StrongMarker { marker } => json!({
            "kind": "strong_marker",
            "marker": marker,
        }),
        TargetEvidence::ConfiguredPath { source } => json!({
            "kind": "configured_path",
            "source": source,
        }),
        TargetEvidence::ProjectContext { project_manifest } => json!({
            "kind": "project_context",
            "project_manifest": project_manifest,
        }),
        TargetEvidence::WeakNameOnly { matched_name } => json!({
            "kind": "weak_name_only",
            "matched_name": matched_name,
        }),
    }
}

fn evidence_label(evidence: &TargetEvidence) -> &'static str {
    match evidence {
        TargetEvidence::StrongMarker { .. } => "strong_marker",
        TargetEvidence::ConfiguredPath { .. } => "configured_path",
        TargetEvidence::ProjectContext { .. } => "project_context",
        TargetEvidence::WeakNameOnly { .. } => "weak_name_only",
    }
}

fn path_kind_label(path_kind: PathKind) -> &'static str {
    match path_kind {
        PathKind::File => "file",
        PathKind::Directory => "directory",
        PathKind::Symlink => "symlink",
        PathKind::Unknown => "unknown",
    }
}

fn skip_reason_label(reason: &ScanSkipReason) -> &'static str {
    match reason {
        ScanSkipReason::DefaultIgnoredDir => "default_ignored_dir",
        ScanSkipReason::ConfiguredIgnoredPath => "configured_ignored_path",
        ScanSkipReason::SymlinkNotFollowed => "symlink_not_followed",
        ScanSkipReason::CrossFilesystem => "cross_filesystem",
        ScanSkipReason::WeakNameOnlySuppressed => "weak_name_only_suppressed",
        ScanSkipReason::AlreadyVisited => "already_visited",
        ScanSkipReason::CargoConfigUnsupported { .. } => "cargo_config_unsupported",
        ScanSkipReason::CargoConfigProblem { .. } => "cargo_config_problem",
        ScanSkipReason::ReadError { .. } => "read_error",
    }
}

fn skip_message(reason: &ScanSkipReason) -> Option<String> {
    match reason {
        ScanSkipReason::CargoConfigUnsupported { message }
        | ScanSkipReason::CargoConfigProblem { message }
        | ScanSkipReason::ReadError { message } => Some(message.clone()),
        _ => None,
    }
}

fn inventory_problem_message(error: ReclaimError) -> String {
    match error {
        ReclaimError::MissingInventoryPath { path } => {
            format!("target vanished during inventory: {}", path.display())
        }
        error => error.to_string(),
    }
}

fn normalize_for_dedupe(path: &Path) -> PathBuf {
    path.components().collect()
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit_index = 0;
    let mut value = bytes as f64;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} B")
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit_index])
    } else {
        format!("{value:.2} {}", UNITS[unit_index])
    }
}

fn apply_status_label(status: ApplyEntryStatus) -> &'static str {
    match status {
        ApplyEntryStatus::WouldDelete => "would_delete",
        ApplyEntryStatus::Deleted => "deleted",
        ApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        ApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
        ApplyEntryStatus::DeleteFailed => "delete_failed",
    }
}

fn targets_usage() -> String {
    "usage: cargo-reclaim targets [list] [OPTIONS] [ROOT ...]\n       cargo-reclaim targets clean (--target <path>|--interactive) [--yes] [OPTIONS] [ROOT ...]".to_string()
}
