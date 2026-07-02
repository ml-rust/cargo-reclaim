use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::model::{
    GeneratedArtifact, GeneratedArtifactKind, SchedulerCommandOutput, SchedulerExecutionReport,
    SchedulerExecutionStatus, SchedulerExecutionStep, SchedulerExecutionTotals,
    SchedulerOperationPlan, SchedulerPlanStep,
};

pub trait SchedulerOperationBackend {
    fn ensure_dir(&mut self, path: &Path) -> Result<(), String>;
    fn write_file(&mut self, path: &Path, contents: &str) -> Result<(), String>;
    fn set_executable(&mut self, path: &Path) -> Result<(), String>;
    fn remove_file(&mut self, path: &Path) -> Result<RemoveFileOutcome, String>;
    fn run_command(&mut self, argv: &[String]) -> Result<SchedulerCommandOutput, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveFileOutcome {
    Removed,
    NotFound,
}

#[derive(Debug, Default)]
pub struct RealSchedulerOperationBackend;

pub fn execute_scheduler_operation(
    plan: &SchedulerOperationPlan,
    backend: &mut impl SchedulerOperationBackend,
) -> SchedulerExecutionReport {
    let mut steps = Vec::with_capacity(plan.steps.len());
    let mut totals = SchedulerExecutionTotals::default();
    let mut blocked = false;

    for step in &plan.steps {
        let execution_step = if blocked {
            SchedulerExecutionStep {
                step: step.clone(),
                status: SchedulerExecutionStatus::Blocked,
                message: Some("blocked by an earlier failed scheduler operation step".to_string()),
                command_output: None,
            }
        } else {
            execute_step(step, &plan.artifacts, backend)
        };

        if execution_step.status == SchedulerExecutionStatus::Failed {
            blocked = true;
        }
        increment_totals(&mut totals, execution_step.status);
        steps.push(execution_step);
    }

    SchedulerExecutionReport {
        command: plan.command,
        operation: plan.operation,
        dry_run: false,
        platform: plan.platform,
        artifacts: plan.artifacts.clone(),
        steps,
        totals,
    }
}

impl SchedulerOperationBackend for RealSchedulerOperationBackend {
    fn ensure_dir(&mut self, path: &Path) -> Result<(), String> {
        fs::create_dir_all(path).map_err(error_message)
    }

    fn write_file(&mut self, path: &Path, contents: &str) -> Result<(), String> {
        write_file_atomically(path, contents).map_err(error_message)
    }

    fn set_executable(&mut self, path: &Path) -> Result<(), String> {
        set_executable(path).map_err(error_message)
    }

    fn remove_file(&mut self, path: &Path) -> Result<RemoveFileOutcome, String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(RemoveFileOutcome::Removed),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(RemoveFileOutcome::NotFound)
            }
            Err(error) => Err(error_message(error)),
        }
    }

    fn run_command(&mut self, argv: &[String]) -> Result<SchedulerCommandOutput, String> {
        let Some((program, args)) = argv.split_first() else {
            return Err("command argv must not be empty".to_string());
        };
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(error_message)?;
        Ok(SchedulerCommandOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn execute_step(
    step: &SchedulerPlanStep,
    artifacts: &[GeneratedArtifact],
    backend: &mut impl SchedulerOperationBackend,
) -> SchedulerExecutionStep {
    match step {
        SchedulerPlanStep::EnsureDir { path } => match backend.ensure_dir(path) {
            Ok(()) => applied(step),
            Err(message) => failed(step, message, None),
        },
        SchedulerPlanStep::WriteFile {
            path,
            artifact_kind,
        } => execute_write_file(step, path, *artifact_kind, artifacts, backend),
        SchedulerPlanStep::SetExecutable { path } => match backend.set_executable(path) {
            Ok(()) => applied(step),
            Err(message) => failed(step, message, None),
        },
        SchedulerPlanStep::RemoveFile { path } => match backend.remove_file(path) {
            Ok(RemoveFileOutcome::Removed) => applied(step),
            Ok(RemoveFileOutcome::NotFound) => SchedulerExecutionStep {
                step: step.clone(),
                status: SchedulerExecutionStatus::Skipped,
                message: Some("file was not present".to_string()),
                command_output: None,
            },
            Err(message) => failed(step, message, None),
        },
        SchedulerPlanStep::RunCommand { argv } => execute_run_command(step, argv, backend),
    }
}

fn execute_write_file(
    step: &SchedulerPlanStep,
    path: &Path,
    artifact_kind: GeneratedArtifactKind,
    artifacts: &[GeneratedArtifact],
    backend: &mut impl SchedulerOperationBackend,
) -> SchedulerExecutionStep {
    let Some(artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == artifact_kind && artifact.intended_install_path == path)
    else {
        return failed(
            step,
            format!(
                "missing generated artifact for {} at {}",
                super::model::artifact_kind_label(artifact_kind),
                path.display()
            ),
            None,
        );
    };

    match backend.write_file(path, &artifact.contents) {
        Ok(()) => applied(step),
        Err(message) => failed(step, message, None),
    }
}

fn execute_run_command(
    step: &SchedulerPlanStep,
    argv: &[String],
    backend: &mut impl SchedulerOperationBackend,
) -> SchedulerExecutionStep {
    match backend.run_command(argv) {
        Ok(output) if output.exit_code == Some(0) => SchedulerExecutionStep {
            step: step.clone(),
            status: SchedulerExecutionStatus::Applied,
            message: None,
            command_output: Some(output),
        },
        Ok(output) => {
            let message = match output.exit_code {
                Some(code) => format!("command exited with status {code}"),
                None => "command terminated without an exit status".to_string(),
            };
            failed(step, message, Some(output))
        }
        Err(message) => failed(step, message, None),
    }
}

fn applied(step: &SchedulerPlanStep) -> SchedulerExecutionStep {
    SchedulerExecutionStep {
        step: step.clone(),
        status: SchedulerExecutionStatus::Applied,
        message: None,
        command_output: None,
    }
}

fn failed(
    step: &SchedulerPlanStep,
    message: String,
    command_output: Option<SchedulerCommandOutput>,
) -> SchedulerExecutionStep {
    SchedulerExecutionStep {
        step: step.clone(),
        status: SchedulerExecutionStatus::Failed,
        message: Some(message),
        command_output,
    }
}

fn increment_totals(totals: &mut SchedulerExecutionTotals, status: SchedulerExecutionStatus) {
    match status {
        SchedulerExecutionStatus::Applied => totals.applied += 1,
        SchedulerExecutionStatus::Skipped => totals.skipped += 1,
        SchedulerExecutionStatus::Failed => totals.failed += 1,
        SchedulerExecutionStatus::Blocked => totals.blocked += 1,
    }
}

fn write_file_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let temp_path = temp_path_for(path)?;
    fs::write(&temp_path, contents)?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => replace_file_after_rename_failure(path, &temp_path, rename_error),
    }
}

fn replace_file_after_rename_failure(
    path: &Path,
    temp_path: &Path,
    rename_error: io::Error,
) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => match fs::rename(temp_path, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(temp_path);
                Err(error)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let _ = fs::remove_file(temp_path);
            Err(rename_error)
        }
        Err(error) => {
            let _ = fs::remove_file(temp_path);
            Err(error)
        }
    }
}

fn temp_path_for(path: &Path) -> io::Result<PathBuf> {
    let Some(file_name) = path.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "write-file path must include a file name",
        ));
    };
    let mut temp_name = file_name.to_os_string();
    temp_name.push(format!(".tmp.{}", std::process::id()));
    Ok(path.with_file_name(temp_name))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable(path: &Path) -> io::Result<()> {
    let _ = fs::metadata(path)?;
    Ok(())
}

fn error_message(error: io::Error) -> String {
    error.to_string()
}
