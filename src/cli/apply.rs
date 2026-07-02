use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use cargo_reclaim::{
    ApplyEntryStatus, ApplyReport, load_plan_from_path, validate_persisted_plan_for_apply,
};

use super::{CliError, OutputFormat, next_path};

#[derive(Debug)]
pub(super) struct ApplyCommand {
    pub(super) plan_path: PathBuf,
    pub(super) output_format: OutputFormat,
}

pub(super) fn parse_apply_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ApplyCommand, CliError> {
    let mut plan_path = None;
    let mut output_format = OutputFormat::Terminal;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "apply options must be valid UTF-8".to_string(),
            ));
        };

        match arg_text {
            "--json" => output_format = OutputFormat::Json,
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
    })
}

pub(super) fn run_apply(command: &ApplyCommand, output: &mut impl Write) -> Result<(), CliError> {
    let document = load_plan_from_path(&command.plan_path)?;
    let report = validate_persisted_plan_for_apply(&document, SystemTime::now())?;
    write_apply_report(output, &report, command.output_format)
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

fn write_apply_report(
    output: &mut impl Write,
    report: &ApplyReport,
    format: OutputFormat,
) -> Result<(), CliError> {
    if format == OutputFormat::Json {
        write_apply_json_report(output, report)?;
        return Ok(());
    }

    writeln!(output, "cargo-reclaim apply validation")?;
    writeln!(output, "validation only; no files were deleted or modified")?;
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
    writeln!(output, "skipped: {}", report.totals.skipped_count)?;
    writeln!(output, "stale skips: {}", report.totals.stale_skip_count)?;

    for entry in &report.entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}",
            apply_status_label(entry.status),
            entry.planned_action,
            entry.path,
            entry.reason
        )?;
    }

    Ok(())
}

fn write_apply_json_report(output: &mut impl Write, report: &ApplyReport) -> Result<(), CliError> {
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
            })
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({
        "command": "apply",
        "dry_run": true,
        "plan_id": report.plan_id.as_str(),
        "totals": {
            "entry_count": report.totals.entry_count,
            "delete_candidate_count": report.totals.delete_candidate_count,
            "would_delete_count": report.totals.would_delete_count,
            "skipped_count": report.totals.skipped_count,
            "stale_skip_count": report.totals.stale_skip_count,
            "would_delete_bytes": report.totals.would_delete_bytes,
        },
        "entries": entries,
    });
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

fn apply_status_label(status: ApplyEntryStatus) -> &'static str {
    match status {
        ApplyEntryStatus::WouldDelete => "would_delete",
        ApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        ApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
    }
}
