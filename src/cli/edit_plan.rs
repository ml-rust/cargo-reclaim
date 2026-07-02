use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use cargo_reclaim::{
    PersistedPlan, PersistedPlanEntry, PlanEditReport, PlanEditRequest, edit_persisted_plan,
    ensure_plan_usable, load_plan_from_path, save_plan_to_path,
};
use serde::Serialize;

use super::{CliError, OutputFormat};

#[derive(Debug)]
pub(super) struct EditPlanCommand {
    pub(super) plan_path: PathBuf,
    pub(super) operation: EditPlanOperation,
    pub(super) output_format: OutputFormat,
}

#[derive(Debug)]
pub(super) enum EditPlanOperation {
    Edit(PlanEditRequest),
    List,
}

pub(super) fn parse_edit_plan_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<EditPlanCommand, CliError> {
    let args = args.into_iter().collect::<Vec<_>>();
    let mut index = 0;
    let mut plan_path = None;
    let mut select = Vec::new();
    let mut deselect = Vec::new();
    let mut select_indices = Vec::new();
    let mut deselect_indices = Vec::new();
    let mut list = false;
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
            "--list" => {
                list = true;
                index += 1;
            }
            "--select" => {
                index = collect_edit_values(&args, index + 1, "--select", &mut select)?;
            }
            value if value.starts_with("--select=") => {
                select.push(required_inline_value(value, "--select")?.to_string());
                index += 1;
            }
            "--select-index" => {
                index =
                    collect_index_values(&args, index + 1, "--select-index", &mut select_indices)?;
            }
            value if value.starts_with("--select-index=") => {
                select_indices.push(parse_index_value(
                    required_inline_value(value, "--select-index")?,
                    "--select-index",
                )?);
                index += 1;
            }
            "--deselect" => {
                index = collect_edit_values(&args, index + 1, "--deselect", &mut deselect)?;
            }
            value if value.starts_with("--deselect=") => {
                deselect.push(required_inline_value(value, "--deselect")?.to_string());
                index += 1;
            }
            "--deselect-index" => {
                index = collect_index_values(
                    &args,
                    index + 1,
                    "--deselect-index",
                    &mut deselect_indices,
                )?;
            }
            value if value.starts_with("--deselect-index=") => {
                deselect_indices.push(parse_index_value(
                    required_inline_value(value, "--deselect-index")?,
                    "--deselect-index",
                )?);
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
                    "unexpected edit-plan argument `{value}`; pass `--plan <path>` and explicit edits or `--list`"
                )));
            }
        }
    }

    let Some(plan_path) = plan_path else {
        return Err(CliError::Usage(
            "edit-plan requires an explicit `--plan <path>`".to_string(),
        ));
    };

    let operation = if list {
        if !select.is_empty()
            || !deselect.is_empty()
            || !select_indices.is_empty()
            || !deselect_indices.is_empty()
        {
            return Err(CliError::Usage(
                "`--list` cannot be combined with edit flags".to_string(),
            ));
        }
        EditPlanOperation::List
    } else {
        EditPlanOperation::Edit(PlanEditRequest::new_with_indices(
            select,
            deselect,
            select_indices,
            deselect_indices,
        )?)
    };

    Ok(EditPlanCommand {
        plan_path,
        operation,
        output_format,
    })
}

pub(super) fn run_edit_plan(
    command: &EditPlanCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let mut document = load_plan_from_path(&command.plan_path)?;
    match &command.operation {
        EditPlanOperation::Edit(request) => {
            let report = edit_persisted_plan(&mut document, request, SystemTime::now())?;
            save_plan_to_path(&command.plan_path, &document)?;
            write_edit_plan_report(output, command, &report)?;
        }
        EditPlanOperation::List => {
            ensure_plan_usable(&document, SystemTime::now())?;
            write_edit_plan_list(output, command, &document)?;
        }
    }
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

fn collect_index_values(
    args: &[OsString],
    mut index: usize,
    option: &'static str,
    values: &mut Vec<usize>,
) -> Result<usize, CliError> {
    let start_len = values.len();
    while index < args.len() {
        let value = arg_text(&args[index])?;
        if value.starts_with('-') {
            break;
        }
        values.push(parse_index_value(&value, option)?);
        index += 1;
    }

    if values.len() == start_len {
        return Err(CliError::Usage(format!("{option} requires a value")));
    }
    Ok(index)
}

fn parse_index_value(value: &str, option: &'static str) -> Result<usize, CliError> {
    let index = value.parse::<usize>().map_err(|_| {
        CliError::Usage(format!("{option} requires a positive 1-based entry index"))
    })?;
    if index == 0 {
        return Err(CliError::Usage(format!(
            "{option} requires a positive 1-based entry index"
        )));
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

fn write_edit_plan_list(
    output: &mut impl Write,
    command: &EditPlanCommand,
    document: &PersistedPlan,
) -> Result<(), CliError> {
    if command.output_format == OutputFormat::Json {
        let document = JsonEditPlanList::from_plan(command, document);
        serde_json::to_writer(&mut *output, &document)?;
        writeln!(output)?;
        return Ok(());
    }

    writeln!(output, "cargo-reclaim edit-plan list")?;
    writeln!(output, "read-only; no plan file was modified")?;
    writeln!(output, "plan id: {}", document.id.as_str())?;
    writeln!(output, "entries: {}", document.body.plan.totals.entry_count)?;
    writeln!(
        output,
        "delete candidates: {}",
        document.body.plan.totals.delete_candidate_count
    )?;
    writeln!(
        output,
        "preserved/unknown: {}",
        document.body.plan.totals.preserved_count
    )?;
    writeln!(
        output,
        "estimated bytes: {}",
        document.body.plan.totals.total_bytes
    )?;

    if document.body.plan.entries.is_empty() {
        return Ok(());
    }

    writeln!(output)?;
    for (index, entry) in document.body.plan.entries.iter().enumerate() {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}",
            index + 1,
            display_text(&entry.action),
            display_text(&entry.artifact_class),
            entry.snapshot.size_bytes,
            confirmation_marker(entry.requires_confirmation),
            display_text(&entry.snapshot.path)
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonEditPlanList {
    command: &'static str,
    plan_path: String,
    plan_id: String,
    totals: JsonEditPlanTotals,
    entries: Vec<JsonEditPlanEntry>,
}

impl JsonEditPlanList {
    fn from_plan(command: &EditPlanCommand, document: &PersistedPlan) -> Self {
        Self {
            command: "edit-plan list",
            plan_path: command.plan_path.display().to_string(),
            plan_id: document.id.as_str().to_string(),
            totals: JsonEditPlanTotals {
                entry_count: document.body.plan.totals.entry_count,
                total_bytes: document.body.plan.totals.total_bytes,
                preserved_count: document.body.plan.totals.preserved_count,
                delete_candidate_count: document.body.plan.totals.delete_candidate_count,
            },
            entries: document
                .body
                .plan
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| JsonEditPlanEntry::from_entry(index + 1, entry))
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonEditPlanTotals {
    entry_count: usize,
    total_bytes: u64,
    preserved_count: usize,
    delete_candidate_count: usize,
}

#[derive(Serialize)]
struct JsonEditPlanEntry {
    index: usize,
    path: String,
    action: String,
    artifact_class: String,
    size_bytes: u64,
    path_kind: String,
    requires_confirmation: bool,
    policy_reason: String,
}

impl JsonEditPlanEntry {
    fn from_entry(index: usize, entry: &PersistedPlanEntry) -> Self {
        Self {
            index,
            path: entry.snapshot.path.clone(),
            action: entry.action.clone(),
            artifact_class: entry.artifact_class.clone(),
            size_bytes: entry.snapshot.size_bytes,
            path_kind: entry.snapshot.path_kind.clone(),
            requires_confirmation: entry.requires_confirmation,
            policy_reason: entry.policy_reason.clone(),
        }
    }
}

fn confirmation_marker(requires_confirmation: bool) -> &'static str {
    if requires_confirmation {
        "confirm"
    } else {
        "-"
    }
}

fn display_text(value: &str) -> String {
    value.escape_default().to_string()
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
        let request = match command.operation {
            EditPlanOperation::Edit(request) => request,
            EditPlanOperation::List => {
                return Err(CliError::Usage("expected edit operation".into()));
            }
        };
        assert_eq!(request.select, ["target/a", "target/b"]);
        assert_eq!(request.deselect, ["target/c"]);
        assert!(request.select_indices.is_empty());
        assert!(request.deselect_indices.is_empty());
        assert_eq!(command.output_format, OutputFormat::Json);
        Ok(())
    }

    #[test]
    fn parses_explicit_plan_and_multiple_index_values() -> Result<(), CliError> {
        let command = parse_edit_plan_command(
            [
                "edit-plan",
                "--plan",
                "plan.json",
                "--select-index",
                "1",
                "2",
                "--deselect-index=3",
            ]
            .into_iter()
            .skip(1)
            .map(OsString::from),
        )?;

        assert_eq!(command.plan_path, PathBuf::from("plan.json"));
        let request = match command.operation {
            EditPlanOperation::Edit(request) => request,
            EditPlanOperation::List => {
                return Err(CliError::Usage("expected edit operation".into()));
            }
        };
        assert!(request.select.is_empty());
        assert!(request.deselect.is_empty());
        assert_eq!(request.select_indices, [1, 2]);
        assert_eq!(request.deselect_indices, [3]);
        Ok(())
    }

    #[test]
    fn parses_list_without_edits() -> Result<(), CliError> {
        let command = parse_edit_plan_command(
            ["--plan", "plan.json", "--list", "--json"].map(OsString::from),
        )?;

        assert_eq!(command.plan_path, PathBuf::from("plan.json"));
        assert!(matches!(command.operation, EditPlanOperation::List));
        assert_eq!(command.output_format, OutputFormat::Json);
        Ok(())
    }

    #[test]
    fn rejects_missing_edits() {
        let error = parse_edit_plan_command(["--plan", "plan.json"].map(OsString::from))
            .expect_err("missing edits should fail");
        assert!(matches!(error, CliError::PlanEdit(_)));
    }

    #[test]
    fn rejects_list_with_edits() {
        let error = parse_edit_plan_command(
            ["--plan", "plan.json", "--list", "--select-index", "1"].map(OsString::from),
        )
        .expect_err("list with edits should fail");
        assert!(matches!(error, CliError::Usage(_)));
    }
}
