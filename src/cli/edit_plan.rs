use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use cargo_reclaim::{
    PlanEditReport, PlanEditRequest, edit_persisted_plan, load_plan_from_path, save_plan_to_path,
};

use super::{CliError, OutputFormat};

#[derive(Debug)]
pub(super) struct EditPlanCommand {
    pub(super) plan_path: PathBuf,
    pub(super) request: PlanEditRequest,
    pub(super) output_format: OutputFormat,
}

pub(super) fn parse_edit_plan_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<EditPlanCommand, CliError> {
    let args = args.into_iter().collect::<Vec<_>>();
    let mut index = 0;
    let mut plan_path = None;
    let mut select = Vec::new();
    let mut deselect = Vec::new();
    let mut output_format = OutputFormat::Terminal;

    while index < args.len() {
        let arg_text = arg_text(&args[index])?;

        match arg_text.as_str() {
            "--json" => {
                output_format = OutputFormat::Json;
                index += 1;
            }
            "--plan" => {
                let path = args
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| CliError::Usage("--plan requires a value".to_string()))?;
                plan_path = Some(validate_plan_path(PathBuf::from(path))?);
                index += 2;
            }
            value if value.starts_with("--plan=") => {
                plan_path = Some(validate_plan_path(PathBuf::from(
                    &value["--plan=".len()..],
                ))?);
                index += 1;
            }
            "--select" => {
                index = collect_edit_values(&args, index + 1, "--select", &mut select)?;
            }
            value if value.starts_with("--select=") => {
                select.push(required_inline_value(value, "--select")?.to_string());
                index += 1;
            }
            "--deselect" => {
                index = collect_edit_values(&args, index + 1, "--deselect", &mut deselect)?;
            }
            value if value.starts_with("--deselect=") => {
                deselect.push(required_inline_value(value, "--deselect")?.to_string());
                index += 1;
            }
            "--last" | "last" => {
                return Err(CliError::Usage(
                    "implicit `last` plans are not supported; pass `--plan <path>`".to_string(),
                ));
            }
            "--yes" => {
                return Err(CliError::Usage(
                    "`--yes` is not supported by edit-plan".to_string(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown edit-plan option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected edit-plan argument `{value}`; pass `--plan <path>` and explicit edits"
                )));
            }
        }
    }

    let Some(plan_path) = plan_path else {
        return Err(CliError::Usage(
            "edit-plan requires an explicit `--plan <path>`".to_string(),
        ));
    };

    let request = PlanEditRequest::new(select, deselect)?;

    Ok(EditPlanCommand {
        plan_path,
        request,
        output_format,
    })
}

pub(super) fn run_edit_plan(
    command: &EditPlanCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let mut document = load_plan_from_path(&command.plan_path)?;
    let report = edit_persisted_plan(&mut document, &command.request, SystemTime::now())?;
    save_plan_to_path(&command.plan_path, &document)?;
    write_edit_plan_report(output, command, &report)?;
    Ok(ExitCode::SUCCESS)
}

fn collect_edit_values(
    args: &[OsString],
    mut index: usize,
    option: &'static str,
    values: &mut Vec<String>,
) -> Result<usize, CliError> {
    let start_len = values.len();
    while index < args.len() {
        let value = arg_text(&args[index])?;
        if value.starts_with('-') {
            break;
        }
        values.push(value);
        index += 1;
    }

    if values.len() == start_len {
        return Err(CliError::Usage(format!("{option} requires a value")));
    }
    Ok(index)
}

fn validate_plan_path(path: PathBuf) -> Result<PathBuf, CliError> {
    if path.as_os_str() == "last" {
        return Err(CliError::Usage(
            "implicit `last` plans are not supported; pass an explicit plan path".to_string(),
        ));
    }
    Ok(path)
}

fn required_inline_value<'a>(value: &'a str, option: &'static str) -> Result<&'a str, CliError> {
    let prefix_len = option.len() + 1;
    let value = &value[prefix_len..];
    if value.is_empty() {
        return Err(CliError::Usage(format!("{option} requires a value")));
    }
    Ok(value)
}

fn arg_text(arg: &OsString) -> Result<String, CliError> {
    arg.clone()
        .into_string()
        .map_err(|_| CliError::Usage("edit-plan arguments must be valid UTF-8".to_string()))
}

fn write_edit_plan_report(
    output: &mut impl Write,
    command: &EditPlanCommand,
    report: &PlanEditReport,
) -> Result<(), CliError> {
    if command.output_format == OutputFormat::Json {
        let document = serde_json::json!({
            "command": "edit-plan",
            "plan_path": command.plan_path.display().to_string(),
            "plan_id": report.plan_id.as_str(),
            "selected_count": report.selected_count,
            "deselected_count": report.deselected_count,
            "totals": {
                "entry_count": report.totals.entry_count,
                "total_bytes": report.totals.total_bytes,
                "preserved_count": report.totals.preserved_count,
                "delete_candidate_count": report.totals.delete_candidate_count,
            },
        });
        serde_json::to_writer(&mut *output, &document)?;
        writeln!(output)?;
        return Ok(());
    }

    writeln!(output, "cargo-reclaim edit-plan")?;
    writeln!(output, "plan id: {}", report.plan_id.as_str())?;
    writeln!(output, "selected: {}", report.selected_count)?;
    writeln!(output, "deselected: {}", report.deselected_count)?;
    writeln!(output, "entries: {}", report.totals.entry_count)?;
    writeln!(
        output,
        "delete candidates: {}",
        report.totals.delete_candidate_count
    )?;
    writeln!(
        output,
        "preserved/unknown: {}",
        report.totals.preserved_count
    )?;
    writeln!(output, "estimated bytes: {}", report.totals.total_bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_plan_and_multiple_edit_values() -> Result<(), CliError> {
        let command = parse_edit_plan_command(
            [
                "edit-plan",
                "--plan",
                "plan.json",
                "--select",
                "target/a",
                "target/b",
                "--deselect=target/c",
                "--json",
            ]
            .into_iter()
            .skip(1)
            .map(OsString::from),
        )?;

        assert_eq!(command.plan_path, PathBuf::from("plan.json"));
        assert_eq!(command.request.select, ["target/a", "target/b"]);
        assert_eq!(command.request.deselect, ["target/c"]);
        assert_eq!(command.output_format, OutputFormat::Json);
        Ok(())
    }

    #[test]
    fn rejects_missing_edits() {
        let error = parse_edit_plan_command(["--plan", "plan.json"].map(OsString::from))
            .expect_err("missing edits should fail");
        assert!(matches!(error, CliError::PlanEdit(_)));
    }
}
