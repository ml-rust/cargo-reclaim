use std::io::Write;

use cargo_reclaim::{
    GeneratedArtifact, SchedulerExecutionReport, SchedulerExecutionStep, SchedulerOperationPlan,
    SchedulerPlanStep, SchedulerReport, artifact_kind_label, execution_status_label, mode_label,
    operation_label, platform_label, policy_label,
};
use serde::Serialize;

use crate::cli::CliError;

pub(super) fn write_preview_terminal(
    output: &mut impl Write,
    report: &SchedulerReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim scheduler preview")?;
    writeln!(
        output,
        "dry-run only; no scheduler files were installed, tasks were registered, timers were enabled, or plans were run"
    )?;
    writeln!(output, "platform: {}", platform_label(report.platform))?;
    writeln!(output, "mode: {}", mode_label(report.mode))?;
    writeln!(
        output,
        "effective policy: {}",
        policy_label(report.effective_policy)
    )?;
    writeln!(output, "at: {}", report.schedule.as_hh_mm())?;
    writeln!(output, "artifacts: {}", report.artifacts.len())?;
    for artifact in &report.artifacts {
        writeln!(
            output,
            "{}\t{}",
            artifact_kind_label(artifact.kind),
            artifact.intended_install_path.display()
        )?;
    }
    Ok(())
}

pub(super) fn write_preview_json(
    output: &mut impl Write,
    report: &SchedulerReport,
) -> Result<(), CliError> {
    let document = JsonSchedulerReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn write_operation_terminal(
    output: &mut impl Write,
    plan: &SchedulerOperationPlan,
) -> Result<(), CliError> {
    writeln!(
        output,
        "cargo-reclaim scheduler {}",
        operation_label(plan.operation)
    )?;
    writeln!(
        output,
        "dry-run only; no files or scheduler registrations were changed"
    )?;
    writeln!(output, "platform: {}", platform_label(plan.platform))?;
    writeln!(output, "artifacts: {}", plan.artifacts.len())?;
    writeln!(output, "steps: {}", plan.steps.len())?;
    for step in &plan.steps {
        write_step(output, step)?;
    }
    Ok(())
}

pub(super) fn write_operation_json(
    output: &mut impl Write,
    plan: &SchedulerOperationPlan,
) -> Result<(), CliError> {
    let document = JsonSchedulerOperationPlan::from_plan(plan);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn write_execution_terminal(
    output: &mut impl Write,
    report: &SchedulerExecutionReport,
) -> Result<(), CliError> {
    writeln!(
        output,
        "cargo-reclaim scheduler {}",
        operation_label(report.operation)
    )?;
    writeln!(output, "platform: {}", platform_label(report.platform))?;
    writeln!(output, "artifacts: {}", report.artifacts.len())?;
    writeln!(
        output,
        "steps: {} applied, {} skipped, {} failed, {} blocked",
        report.totals.applied, report.totals.skipped, report.totals.failed, report.totals.blocked
    )?;
    for step in &report.steps {
        write_execution_step(output, step)?;
    }
    Ok(())
}

pub(super) fn write_execution_json(
    output: &mut impl Write,
    report: &SchedulerExecutionReport,
) -> Result<(), CliError> {
    let document = JsonSchedulerExecutionReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

fn write_step(output: &mut impl Write, step: &SchedulerPlanStep) -> Result<(), CliError> {
    match step {
        SchedulerPlanStep::EnsureDir { path } => {
            writeln!(output, "ensure-dir\t{}", path.display())?
        }
        SchedulerPlanStep::WriteFile {
            path,
            artifact_kind,
        } => writeln!(
            output,
            "write-file\t{}\t{}",
            artifact_kind_label(*artifact_kind),
            path.display()
        )?,
        SchedulerPlanStep::SetExecutable { path } => {
            writeln!(output, "set-executable\t{}", path.display())?
        }
        SchedulerPlanStep::RemoveFile { path } => {
            writeln!(output, "remove-file\t{}", path.display())?
        }
        SchedulerPlanStep::RunCommand { argv } => {
            writeln!(output, "run-command\t{}", argv.join("\t"))?
        }
    }
    Ok(())
}

fn write_execution_step(
    output: &mut impl Write,
    step: &SchedulerExecutionStep,
) -> Result<(), CliError> {
    write!(output, "{}\t", execution_status_label(step.status))?;
    write_step(output, &step.step)?;
    if let Some(message) = &step.message {
        writeln!(output, "message\t{message}")?;
    }
    if let Some(command_output) = &step.command_output {
        match command_output.exit_code {
            Some(code) => writeln!(output, "exit-code\t{code}")?,
            None => writeln!(output, "exit-code\tunknown")?,
        }
        if !command_output.stdout.is_empty() {
            writeln!(output, "stdout\t{}", command_output.stdout)?;
        }
        if !command_output.stderr.is_empty() {
            writeln!(output, "stderr\t{}", command_output.stderr)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonSchedulerReport<'a> {
    command: &'static str,
    dry_run: bool,
    platform: &'static str,
    mode: &'static str,
    effective_policy: &'static str,
    at: String,
    artifacts: Vec<JsonArtifact<'a>>,
}

impl<'a> JsonSchedulerReport<'a> {
    fn from_report(report: &'a SchedulerReport) -> Self {
        Self {
            command: report.command,
            dry_run: report.dry_run,
            platform: platform_label(report.platform),
            mode: mode_label(report.mode),
            effective_policy: policy_label(report.effective_policy),
            at: report.schedule.as_hh_mm(),
            artifacts: report
                .artifacts
                .iter()
                .map(JsonArtifact::from_artifact)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonSchedulerOperationPlan<'a> {
    command: &'static str,
    operation: &'static str,
    dry_run: bool,
    platform: &'static str,
    artifacts: Vec<JsonArtifact<'a>>,
    steps: Vec<JsonStep<'a>>,
}

impl<'a> JsonSchedulerOperationPlan<'a> {
    fn from_plan(plan: &'a SchedulerOperationPlan) -> Self {
        Self {
            command: plan.command,
            operation: operation_label(plan.operation),
            dry_run: plan.dry_run,
            platform: platform_label(plan.platform),
            artifacts: plan
                .artifacts
                .iter()
                .map(JsonArtifact::from_artifact)
                .collect(),
            steps: plan.steps.iter().map(JsonStep::from_step).collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonSchedulerExecutionReport<'a> {
    command: &'static str,
    operation: &'static str,
    dry_run: bool,
    platform: &'static str,
    artifacts: Vec<JsonArtifact<'a>>,
    totals: JsonExecutionTotals,
    steps: Vec<JsonExecutionStep<'a>>,
}

impl<'a> JsonSchedulerExecutionReport<'a> {
    fn from_report(report: &'a SchedulerExecutionReport) -> Self {
        Self {
            command: report.command,
            operation: operation_label(report.operation),
            dry_run: report.dry_run,
            platform: platform_label(report.platform),
            artifacts: report
                .artifacts
                .iter()
                .map(JsonArtifact::from_artifact)
                .collect(),
            totals: JsonExecutionTotals {
                applied: report.totals.applied,
                skipped: report.totals.skipped,
                failed: report.totals.failed,
                blocked: report.totals.blocked,
            },
            steps: report
                .steps
                .iter()
                .map(JsonExecutionStep::from_step)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonExecutionTotals {
    applied: usize,
    skipped: usize,
    failed: usize,
    blocked: usize,
}

#[derive(Serialize)]
struct JsonArtifact<'a> {
    kind: &'static str,
    intended_install_path: String,
    contents: &'a str,
}

impl<'a> JsonArtifact<'a> {
    fn from_artifact(artifact: &'a GeneratedArtifact) -> Self {
        Self {
            kind: artifact_kind_label(artifact.kind),
            intended_install_path: artifact.intended_install_path.display().to_string(),
            contents: &artifact.contents,
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum JsonStep<'a> {
    EnsureDir {
        path: String,
    },
    WriteFile {
        path: String,
        artifact_kind: &'static str,
    },
    SetExecutable {
        path: String,
    },
    RemoveFile {
        path: String,
    },
    RunCommand {
        argv: &'a [String],
    },
}

#[derive(Serialize)]
struct JsonExecutionStep<'a> {
    status: &'static str,
    step: JsonStep<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_output: Option<JsonCommandOutput<'a>>,
}

impl<'a> JsonExecutionStep<'a> {
    fn from_step(step: &'a SchedulerExecutionStep) -> Self {
        Self {
            status: execution_status_label(step.status),
            step: JsonStep::from_step(&step.step),
            message: step.message.as_deref(),
            command_output: step
                .command_output
                .as_ref()
                .map(JsonCommandOutput::from_output),
        }
    }
}

#[derive(Serialize)]
struct JsonCommandOutput<'a> {
    exit_code: Option<i32>,
    stdout: &'a str,
    stderr: &'a str,
}

impl<'a> JsonCommandOutput<'a> {
    fn from_output(output: &'a cargo_reclaim::SchedulerCommandOutput) -> Self {
        Self {
            exit_code: output.exit_code,
            stdout: &output.stdout,
            stderr: &output.stderr,
        }
    }
}

impl<'a> JsonStep<'a> {
    fn from_step(step: &'a SchedulerPlanStep) -> Self {
        match step {
            SchedulerPlanStep::EnsureDir { path } => Self::EnsureDir {
                path: path.display().to_string(),
            },
            SchedulerPlanStep::WriteFile {
                path,
                artifact_kind,
            } => Self::WriteFile {
                path: path.display().to_string(),
                artifact_kind: artifact_kind_label(*artifact_kind),
            },
            SchedulerPlanStep::SetExecutable { path } => Self::SetExecutable {
                path: path.display().to_string(),
            },
            SchedulerPlanStep::RemoveFile { path } => Self::RemoveFile {
                path: path.display().to_string(),
            },
            SchedulerPlanStep::RunCommand { argv } => Self::RunCommand { argv },
        }
    }
}
