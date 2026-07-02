use std::path::PathBuf;

use cargo_reclaim::{
    GeneratedArtifactKind, PolicyKind, RemoveFileOutcome, Schedule, SchedulerCommandOutput,
    SchedulerExecutionStatus, SchedulerMode, SchedulerOperationBackend, SchedulerPlatform,
    SchedulerRequest, execute_scheduler_operation, plan_scheduler_install,
    plan_scheduler_uninstall,
};

fn request(platform: SchedulerPlatform) -> SchedulerRequest {
    SchedulerRequest {
        platform,
        instance_name: "daily-workstation".to_string(),
        config_path: PathBuf::from("/tmp/reclaim config.toml"),
        cargo_reclaim_bin: PathBuf::from("/usr/local/bin/cargo-reclaim"),
        schedule: Schedule::default(),
        mode: SchedulerMode::Observe,
        policy: Some(PolicyKind::Observe),
        allow_unattended_cleanup: false,
        allow_unattended_high_policy: false,
        state_dir: Some(PathBuf::from("/tmp/cargo reclaim/state")),
        log_dir: Some(PathBuf::from("/tmp/cargo reclaim/logs")),
    }
}

#[test]
fn executor_writes_artifact_contents_and_runs_argv_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_install(request(SchedulerPlatform::SystemdUser))?;
    let runner = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::RunnerScript)
        .expect("runner artifact");
    let mut backend = FakeBackend::default();

    let report = execute_scheduler_operation(&plan, &mut backend);

    assert!(report.succeeded());
    assert_eq!(report.totals.failed, 0);
    assert!(
        backend
            .writes
            .iter()
            .any(|(path, contents)| path == &runner.intended_install_path
                && contents == &runner.contents)
    );
    assert!(backend.commands.iter().any(|argv| {
        argv == &[
            "systemctl".to_string(),
            "--user".to_string(),
            "daemon-reload".to_string(),
        ]
    }));
    Ok(())
}

#[test]
fn executor_skips_missing_remove_file_without_blocking() -> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_uninstall(request(SchedulerPlatform::Launchd))?;
    let mut backend = FakeBackend {
        remove_outcome: RemoveFileOutcome::NotFound,
        ..FakeBackend::default()
    };

    let report = execute_scheduler_operation(&plan, &mut backend);

    assert!(report.succeeded());
    assert_eq!(report.totals.skipped, 2);
    assert!(
        report
            .steps
            .iter()
            .any(|step| step.status == SchedulerExecutionStatus::Skipped)
    );
    Ok(())
}

#[test]
fn executor_reports_failure_and_blocks_later_steps() -> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_uninstall(request(SchedulerPlatform::SystemdUser))?;
    let mut backend = FakeBackend {
        command_exit_code: Some(5),
        ..FakeBackend::default()
    };

    let report = execute_scheduler_operation(&plan, &mut backend);

    assert!(!report.succeeded());
    assert_eq!(report.totals.failed, 1);
    assert!(report.totals.blocked > 0);
    assert_eq!(report.steps[0].status, SchedulerExecutionStatus::Failed);
    assert!(
        report
            .steps
            .iter()
            .skip(1)
            .all(|step| step.status == SchedulerExecutionStatus::Blocked)
    );
    Ok(())
}

#[test]
fn executor_reports_missing_write_artifact_as_failure() {
    let mut plan = plan_scheduler_install(request(SchedulerPlatform::SystemdUser)).expect("plan");
    plan.artifacts.clear();
    let mut backend = FakeBackend::default();

    let report = execute_scheduler_operation(&plan, &mut backend);

    assert_eq!(report.totals.failed, 1);
    assert!(report.totals.blocked > 0);
}

#[derive(Debug)]
struct FakeBackend {
    writes: Vec<(PathBuf, String)>,
    commands: Vec<Vec<String>>,
    remove_outcome: RemoveFileOutcome,
    command_exit_code: Option<i32>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self {
            writes: Vec::new(),
            commands: Vec::new(),
            remove_outcome: RemoveFileOutcome::Removed,
            command_exit_code: None,
        }
    }
}

impl SchedulerOperationBackend for FakeBackend {
    fn ensure_dir(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    fn write_file(&mut self, path: &std::path::Path, contents: &str) -> Result<(), String> {
        self.writes.push((path.to_path_buf(), contents.to_string()));
        Ok(())
    }

    fn set_executable(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    fn remove_file(&mut self, _path: &std::path::Path) -> Result<RemoveFileOutcome, String> {
        Ok(self.remove_outcome)
    }

    fn run_command(&mut self, argv: &[String]) -> Result<SchedulerCommandOutput, String> {
        self.commands.push(argv.to_vec());
        Ok(SchedulerCommandOutput {
            exit_code: Some(self.command_exit_code.unwrap_or(0)),
            stdout: "stdout".to_string(),
            stderr: "stderr".to_string(),
        })
    }
}
