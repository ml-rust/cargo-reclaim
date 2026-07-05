use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ApplyEntryResult, ApplyEntryStatus, ApplyReport, SchedulerPlatform, default_state_dir,
    execute_persisted_plan_apply, load_plan_from_path, validate_persisted_plan_for_apply,
};

use super::{CliError, OutputFormat, next_path};

const TERMINAL_NOTABLE_ENTRY_LIMIT: usize = 20;

#[derive(Debug)]
pub(super) struct ApplyCommand {
    pub(super) plan_path: PathBuf,
    pub(super) output_format: OutputFormat,
    pub(super) execute: bool,
}

pub(super) fn parse_apply_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ApplyCommand, CliError> {
    let mut plan_path = None;
    let mut output_format = OutputFormat::Terminal;
    let mut execute = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "apply options must be valid UTF-8".to_string(),
            ));
        };

        match arg_text {
            "--json" => output_format = OutputFormat::Json,
            "--yes" => execute = true,
            "--plan" => plan_path = Some(next_plan_path(&mut args)?),
            value if value.starts_with("--plan=") => {
                plan_path = Some(validate_plan_path(PathBuf::from(
                    &value["--plan=".len()..],
                ))?);
            }
            "--last" | "last" => {
                return Err(CliError::Usage(
                    "implicit `last` plans are not supported; pass `--plan <path>`".to_string(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown apply option `{value}`")));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected apply argument `{value}`; pass `--plan <path>`"
                )));
            }
        }
    }

    let Some(plan_path) = plan_path else {
        return Err(CliError::Usage(
            "apply requires an explicit `--plan <path>`".to_string(),
        ));
    };

    Ok(ApplyCommand {
        plan_path,
        output_format,
        execute,
    })
}

pub(super) fn run_apply(
    command: &ApplyCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let document = load_plan_from_path(&command.plan_path)?;
    let report = if command.execute {
        execute_persisted_plan_apply(&document, SystemTime::now())?
    } else {
        validate_persisted_plan_for_apply(&document, SystemTime::now())?
    };
    let exit_code = if report.totals.failed_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    write_apply_report(output, &report, command.output_format)?;
    Ok(exit_code)
}

fn next_plan_path(args: &mut impl Iterator<Item = OsString>) -> Result<PathBuf, CliError> {
    let path = next_path(args, "--plan")?;
    validate_plan_path(path)
}

fn validate_plan_path(path: PathBuf) -> Result<PathBuf, CliError> {
    if path.as_os_str() == "last" {
        return Err(CliError::Usage(
            "implicit `last` plans are not supported; pass an explicit plan path".to_string(),
        ));
    }
    Ok(path)
}

pub(super) fn write_apply_report(
    output: &mut impl Write,
    report: &ApplyReport,
    format: OutputFormat,
) -> Result<(), CliError> {
    write_apply_report_with_command(output, report, format, "apply")
}

pub(super) fn write_apply_report_with_command(
    output: &mut impl Write,
    report: &ApplyReport,
    format: OutputFormat,
    command_label: &str,
) -> Result<(), CliError> {
    if format == OutputFormat::Json {
        write_apply_json_report(output, report, command_label)?;
        return Ok(());
    }

    let report_path = save_terminal_apply_report(report, command_label).ok();

    if report.dry_run {
        writeln!(output, "cargo-reclaim {command_label} validation")?;
        writeln!(output, "validation only; no files were deleted or modified")?;
    } else {
        writeln!(output, "cargo-reclaim {command_label} execution")?;
        writeln!(
            output,
            "execution mode; only freshly revalidated delete entries were removed"
        )?;
    }
    writeln!(output, "plan id: {}", report.plan_id.as_str())?;
    writeln!(output, "entries: {}", report.totals.entry_count)?;
    writeln!(
        output,
        "delete candidates: {}",
        report.totals.delete_candidate_count
    )?;
    writeln!(output, "would delete: {}", report.totals.would_delete_count)?;
    writeln!(
        output,
        "would delete bytes: {}",
        report.totals.would_delete_bytes
    )?;
    writeln!(output, "deleted: {}", report.totals.applied_count)?;
    writeln!(output, "deleted bytes: {}", report.totals.applied_bytes)?;
    writeln!(output, "delete failures: {}", report.totals.failed_count)?;
    writeln!(output, "skipped: {}", report.totals.skipped_count)?;
    writeln!(output, "stale skips: {}", report.totals.stale_skip_count)?;

    if let Some(path) = report_path {
        writeln!(output, "full report: {}", path.display())?;
    } else {
        writeln!(
            output,
            "full report: unavailable; rerun with --json to capture every entry"
        )?;
    }
    write_terminal_notable_entries(output, report)?;

    Ok(())
}

fn save_terminal_apply_report(
    report: &ApplyReport,
    command_label: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let report_dir = default_state_dir(current_scheduler_platform()).join("reports");
    fs::create_dir_all(&report_dir)?;
    let path = report_dir.join(report_file_name(report, command_label));
    let mut file = fs::File::create(&path)?;
    serde_json::to_writer_pretty(&mut file, &apply_json_document(report, command_label))?;
    writeln!(file)?;
    Ok(path)
}

fn report_file_name(report: &ApplyReport, command_label: &str) -> String {
    let timestamp_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let mode = if report.dry_run {
        "validation"
    } else {
        "execution"
    };
    let plan_id = sanitize_file_component(report.plan_id.as_str());
    format!("{timestamp_millis}-{command_label}-{mode}-{plan_id}.json")
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn current_scheduler_platform() -> SchedulerPlatform {
    if cfg!(target_os = "macos") {
        SchedulerPlatform::Launchd
    } else if cfg!(target_os = "windows") {
        SchedulerPlatform::TaskScheduler
    } else {
        SchedulerPlatform::SystemdUser
    }
}

fn write_terminal_notable_entries(
    output: &mut impl Write,
    report: &ApplyReport,
) -> Result<(), CliError> {
    let notable_entries = report
        .entries
        .iter()
        .filter(|entry| is_terminal_notable(entry))
        .collect::<Vec<_>>();
    let hidden_count = report.entries.len().saturating_sub(notable_entries.len());
    let shown_count = notable_entries.len().min(TERMINAL_NOTABLE_ENTRY_LIMIT);
    let omitted_notable_count = notable_entries.len().saturating_sub(shown_count);

    if shown_count > 0 {
        writeln!(output, "notable entries:")?;
        for entry in notable_entries
            .into_iter()
            .take(TERMINAL_NOTABLE_ENTRY_LIMIT)
        {
            writeln!(
                output,
                "{}\t{}\t{}\t{}",
                apply_status_label(entry.status),
                entry.planned_action,
                entry.path,
                entry.reason
            )?;
        }
    }

    if hidden_count > 0 || omitted_notable_count > 0 {
        writeln!(
            output,
            "entries not shown: {} preserved/unchanged, {} additional notable; use --json or open the full report for every entry",
            hidden_count, omitted_notable_count
        )?;
    }

    Ok(())
}

fn is_terminal_notable(entry: &ApplyEntryResult) -> bool {
    matches!(
        entry.status,
        ApplyEntryStatus::WouldDelete
            | ApplyEntryStatus::Deleted
            | ApplyEntryStatus::SkipStalePlan
            | ApplyEntryStatus::DeleteFailed
    )
}

fn write_apply_json_report(
    output: &mut impl Write,
    report: &ApplyReport,
    command_label: &str,
) -> Result<(), CliError> {
    let document = apply_json_document(report, command_label);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

fn apply_json_document(report: &ApplyReport, command_label: &str) -> serde_json::Value {
    let entries = report
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.path,
                "planned_action": entry.planned_action,
                "status": apply_status_label(entry.status),
                "reason": entry.reason,
                "size_bytes": entry.size_bytes,
                "deleted_bytes": entry.deleted_bytes,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "command": command_label,
        "dry_run": report.dry_run,
        "plan_id": report.plan_id.as_str(),
        "totals": {
            "entry_count": report.totals.entry_count,
            "delete_candidate_count": report.totals.delete_candidate_count,
            "would_delete_count": report.totals.would_delete_count,
            "applied_count": report.totals.applied_count,
            "failed_count": report.totals.failed_count,
            "skipped_count": report.totals.skipped_count,
            "stale_skip_count": report.totals.stale_skip_count,
            "would_delete_bytes": report.totals.would_delete_bytes,
            "applied_bytes": report.totals.applied_bytes,
        },
        "entries": entries,
    })
}

pub(super) fn apply_status_label(status: ApplyEntryStatus) -> &'static str {
    match status {
        ApplyEntryStatus::WouldDelete => "would_delete",
        ApplyEntryStatus::Deleted => "deleted",
        ApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        ApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
        ApplyEntryStatus::DeleteFailed => "delete_failed",
    }
}
