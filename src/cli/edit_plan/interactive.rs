use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use cargo_reclaim::{PersistedPlan, PlanEditRequest};

use super::{CliError, EditPlanCommand, OutputFormat};

pub(super) fn prompt_for_interactive_edit(
    document: &PersistedPlan,
) -> Result<Option<PlanEditRequest>, CliError> {
    let menu = InteractiveMenu::from_plan(document);
    let mut stderr = io::stderr();
    write_interactive_menu(&mut stderr, document, &menu)?;
    stderr.flush()?;

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    if is_cancel(trimmed) {
        return Ok(None);
    }

    let request = parse_interactive_selection(trimmed, document, &menu)?;
    Ok(Some(request))
}

pub(super) fn write_interactive_cancel_report(
    output: &mut impl Write,
    command: &EditPlanCommand,
) -> Result<(), CliError> {
    if command.output_format == OutputFormat::Json {
        let document = serde_json::json!({
            "command": "edit-plan",
            "plan_path": command.plan_path.display().to_string(),
            "status": "cancelled",
            "modified": false,
            "message": "interactive selection was not saved",
        });
        serde_json::to_writer(&mut *output, &document)?;
        writeln!(output)?;
        return Ok(());
    }

    writeln!(output, "cargo-reclaim edit-plan")?;
    writeln!(output, "interactive selection was not saved")?;
    writeln!(output, "plan unchanged")?;
    Ok(())
}

pub(super) fn is_confirmed(value: &str) -> bool {
    value.eq_ignore_ascii_case("y") || value.eq_ignore_ascii_case("yes")
}

struct InteractiveMenu {
    project_groups: Vec<ProjectGroup>,
    class_groups: Vec<String>,
}

struct ProjectGroup {
    label: String,
    manifest: String,
    indices: Vec<usize>,
}

impl InteractiveMenu {
    fn from_plan(document: &PersistedPlan) -> Self {
        let mut projects: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut classes = BTreeSet::new();
        for (index, entry) in document.body.plan.entries.iter().enumerate() {
            if entry.artifact_class != "whole_target"
                && let Some(manifest) = &entry.evidence.project_manifest
            {
                projects
                    .entry(manifest.clone())
                    .or_default()
                    .push(index + 1);
            }
            if entry.artifact_class != "whole_target" && entry.artifact_class != "unknown" {
                classes.insert(entry.artifact_class.clone());
            }
        }

        Self {
            project_groups: projects
                .into_iter()
                .enumerate()
                .map(|(index, (manifest, indices))| ProjectGroup {
                    label: format!("p{}", index + 1),
                    manifest,
                    indices,
                })
                .collect(),
            class_groups: classes.into_iter().collect(),
        }
    }
}

fn write_interactive_menu(
    output: &mut impl Write,
    document: &PersistedPlan,
    menu: &InteractiveMenu,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim edit-plan interactive")?;
    writeln!(
        output,
        "Select entries to delete; apply --plan remains the destructive step."
    )?;
    writeln!(
        output,
        "Tokens: entry number, project group pN, class c:<class>, or none/cancel."
    )?;
    writeln!(output)?;
    writeln!(output, "Entries:")?;
    for (index, entry) in document.body.plan.entries.iter().enumerate() {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            index + 1,
            display_text(&entry.artifact_class),
            display_text(&entry.action),
            entry.snapshot.size_bytes,
            display_text(&entry.snapshot.path)
        )?;
    }

    if !menu.project_groups.is_empty() {
        writeln!(output)?;
        writeln!(output, "Project groups:")?;
        for group in &menu.project_groups {
            writeln!(
                output,
                "{}\t{}\tentries {}",
                group.label,
                display_text(&group.manifest),
                join_indices(&group.indices)
            )?;
        }
    }

    if !menu.class_groups.is_empty() {
        writeln!(output)?;
        writeln!(output, "Class groups:")?;
        for class in &menu.class_groups {
            writeln!(output, "c:{}\t{}", display_text(class), display_text(class))?;
        }
    }

    if document
        .body
        .plan
        .entries
        .iter()
        .any(|entry| entry.artifact_class == "whole_target")
    {
        writeln!(
            output,
            "whole_target entries must be selected by entry number."
        )?;
    }

    writeln!(output)?;
    writeln!(output, "Selection:")?;
    Ok(())
}

fn parse_interactive_selection(
    input: &str,
    document: &PersistedPlan,
    menu: &InteractiveMenu,
) -> Result<PlanEditRequest, CliError> {
    let tokens = input
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(CliError::Usage(
            "interactive selection requires at least one token or `none`".to_string(),
        ));
    }
    if tokens.iter().any(|token| is_cancel(token)) {
        return Err(CliError::Usage(
            "`none`/`cancel` cannot be combined with other selections".to_string(),
        ));
    }

    let mut select_indices = Vec::new();
    let mut select_classes = Vec::new();
    for token in tokens {
        if let Some(class) = token.strip_prefix("c:") {
            if class.is_empty() {
                return Err(CliError::Usage(
                    "class selection requires c:<class>".to_string(),
                ));
            }
            if class == "whole_target" {
                return Err(CliError::Usage(
                    "whole_target must be selected by entry number".to_string(),
                ));
            }
            select_classes.push(class.to_string());
        } else if let Some(group_index) = token.strip_prefix('p') {
            let group_index = parse_group_index(group_index)?;
            let group = menu
                .project_groups
                .get(group_index - 1)
                .ok_or_else(|| CliError::Usage(format!("unknown project group `{token}`")))?;
            select_indices.extend(group.indices.iter().copied());
        } else {
            let index = parse_interactive_index(token)?;
            if index > document.body.plan.entries.len() {
                return Err(CliError::Usage(format!(
                    "entry index {index} is out of range for this plan"
                )));
            }
            select_indices.push(index);
        }
    }

    PlanEditRequest::new_with_class_selectors(
        Vec::new(),
        Vec::new(),
        select_indices,
        Vec::new(),
        select_classes,
        Vec::new(),
    )
    .map_err(CliError::from)
}

fn parse_interactive_index(value: &str) -> Result<usize, CliError> {
    let index = value.parse::<usize>().map_err(|_| {
        CliError::Usage("interactive selection requires a positive 1-based entry index".to_string())
    })?;
    if index == 0 {
        return Err(CliError::Usage(
            "interactive selection requires a positive 1-based entry index".to_string(),
        ));
    }
    Ok(index)
}

fn parse_group_index(value: &str) -> Result<usize, CliError> {
    let index = value
        .parse::<usize>()
        .map_err(|_| CliError::Usage("project group selection requires p<index>".to_string()))?;
    if index == 0 {
        return Err(CliError::Usage(
            "project group selection requires p<index>".to_string(),
        ));
    }
    Ok(index)
}

fn is_cancel(value: &str) -> bool {
    value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("cancel")
}

fn join_indices(indices: &[usize]) -> String {
    indices
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn display_text(value: &str) -> String {
    value.escape_default().to_string()
}
