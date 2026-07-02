use std::path::PathBuf;

use cargo_reclaim::{
    GeneratedArtifactKind, PolicyKind, Schedule, SchedulerError, SchedulerMode, SchedulerOperation,
    SchedulerPlanStep, SchedulerPlatform, SchedulerRequest, generate_scheduler_artifacts,
    plan_scheduler_install, plan_scheduler_uninstall,
};

fn request(platform: SchedulerPlatform) -> SchedulerRequest {
    SchedulerRequest {
        platform,
        config_path: PathBuf::from("/tmp/reclaim config.toml"),
        cargo_reclaim_bin: PathBuf::from("/usr/local/bin/cargo-reclaim"),
        schedule: Schedule::default(),
        mode: SchedulerMode::Observe,
        policy: None,
        allow_unattended_cleanup: false,
        allow_unattended_high_policy: false,
        state_dir: Some(PathBuf::from("/tmp/cargo reclaim/state")),
        log_dir: Some(PathBuf::from("/tmp/cargo reclaim/logs")),
    }
}

#[test]
fn default_observe_uses_observe_policy() -> Result<(), Box<dyn std::error::Error>> {
    let report = generate_scheduler_artifacts(request(SchedulerPlatform::SystemdUser))?;
    assert_eq!(report.mode, SchedulerMode::Observe);
    assert_eq!(report.effective_policy, PolicyKind::Observe);
    assert!(report.dry_run);
    Ok(())
}

#[test]
fn cleanup_defaults_to_conservative_when_allowed() -> Result<(), Box<dyn std::error::Error>> {
    let mut request = request(SchedulerPlatform::SystemdUser);
    request.mode = SchedulerMode::Cleanup;
    request.allow_unattended_cleanup = true;
    let report = generate_scheduler_artifacts(request)?;
    assert_eq!(report.effective_policy, PolicyKind::Conservative);
    Ok(())
}

#[test]
fn rejects_cleanup_without_cleanup_allowance() {
    let mut request = request(SchedulerPlatform::SystemdUser);
    request.mode = SchedulerMode::Cleanup;
    let error = generate_scheduler_artifacts(request).unwrap_err();
    assert_eq!(error, SchedulerError::CleanupNotAllowed);
}

#[test]
fn rejects_high_cleanup_without_explicit_high_allowance() {
    let mut request = request(SchedulerPlatform::SystemdUser);
    request.mode = SchedulerMode::Cleanup;
    request.policy = Some(PolicyKind::Balanced);
    request.allow_unattended_cleanup = true;
    let error = generate_scheduler_artifacts(request).unwrap_err();
    assert_eq!(
        error,
        SchedulerError::HighPolicyNotAllowed(PolicyKind::Balanced)
    );
}

#[test]
fn allows_explicit_high_cleanup_with_allowance() -> Result<(), Box<dyn std::error::Error>> {
    let mut request = request(SchedulerPlatform::SystemdUser);
    request.mode = SchedulerMode::Cleanup;
    request.policy = Some(PolicyKind::Aggressive);
    request.allow_unattended_cleanup = true;
    request.allow_unattended_high_policy = true;
    let report = generate_scheduler_artifacts(request)?;
    assert_eq!(report.effective_policy, PolicyKind::Aggressive);
    Ok(())
}

#[test]
fn rejects_invalid_hh_mm() {
    for value in ["3:00", "24:00", "03:60", "0300"] {
        assert!(Schedule::parse(value).is_err(), "{value}");
    }
}

#[test]
fn generated_runner_uses_explicit_config_policy_and_timestamped_plan()
-> Result<(), Box<dyn std::error::Error>> {
    let report = generate_scheduler_artifacts(request(SchedulerPlatform::SystemdUser))?;
    let runner = report
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::RunnerScript)
        .expect("runner artifact");
    assert!(runner.contents.contains(" plan --config "));
    assert!(runner.contents.contains(" --policy 'observe' "));
    assert!(runner.contents.contains(" --expires-in 1h "));
    assert!(runner.contents.contains(" --json "));
    assert!(runner.contents.contains("cargo-reclaim-$STAMP.json"));
    assert!(!runner.contents.contains(" last"));
    Ok(())
}

#[test]
fn default_paths_do_not_depend_on_shell_placeholder_expansion()
-> Result<(), Box<dyn std::error::Error>> {
    for platform in [
        SchedulerPlatform::SystemdUser,
        SchedulerPlatform::Launchd,
        SchedulerPlatform::TaskScheduler,
    ] {
        let mut request = request(platform);
        request.state_dir = None;
        request.log_dir = None;
        let report = generate_scheduler_artifacts(request)?;
        for artifact in report.artifacts {
            let path = artifact.intended_install_path.display().to_string();
            assert!(!path.contains('~'), "{path}");
            assert!(!path.contains("%LOCALAPPDATA%"), "{path}");
            assert!(!artifact.contents.contains('~'));
            assert!(!artifact.contents.contains("%LOCALAPPDATA%"));
        }
    }
    Ok(())
}

#[test]
fn platform_artifact_kinds_and_paths() -> Result<(), Box<dyn std::error::Error>> {
    let systemd = generate_scheduler_artifacts(request(SchedulerPlatform::SystemdUser))?;
    assert!(
        systemd
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == GeneratedArtifactKind::SystemdService)
    );
    assert!(
        systemd
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == GeneratedArtifactKind::SystemdTimer)
    );

    let launchd = generate_scheduler_artifacts(request(SchedulerPlatform::Launchd))?;
    assert!(
        launchd
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == GeneratedArtifactKind::LaunchdPlist)
    );
    assert!(launchd.artifacts.iter().any(|artifact| {
        artifact
            .intended_install_path
            .display()
            .to_string()
            .contains("LaunchAgents")
    }));

    let task = generate_scheduler_artifacts(request(SchedulerPlatform::TaskScheduler))?;
    assert!(
        task.artifacts
            .iter()
            .any(|artifact| artifact.kind == GeneratedArtifactKind::TaskSchedulerXml)
    );
    assert!(task.artifacts.iter().any(|artifact| {
        artifact
            .intended_install_path
            .display()
            .to_string()
            .contains("Task Scheduler")
    }));
    Ok(())
}

#[test]
fn escapes_paths_in_scripts_and_xml() -> Result<(), Box<dyn std::error::Error>> {
    let mut launchd_request = request(SchedulerPlatform::Launchd);
    launchd_request.config_path = PathBuf::from("/tmp/quote' and &/config.toml");
    launchd_request.cargo_reclaim_bin = PathBuf::from("/tmp/bin/cargo reclaim");
    launchd_request.state_dir = Some(PathBuf::from("/tmp/state & logs/state"));
    launchd_request.log_dir = Some(PathBuf::from("/tmp/state & logs/logs"));
    let report = generate_scheduler_artifacts(launchd_request)?;
    let runner = report
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::RunnerScript)
        .expect("runner");
    assert!(
        runner
            .contents
            .contains("'/tmp/quote'\\'' and &/config.toml'")
    );
    let plist = report
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::LaunchdPlist)
        .expect("plist");
    assert!(plist.contents.contains("&amp;"));

    let task = generate_scheduler_artifacts({
        let mut task_request = request(SchedulerPlatform::TaskScheduler);
        task_request.config_path = PathBuf::from("/tmp/quote' and &/config.toml");
        task_request.cargo_reclaim_bin = PathBuf::from("/tmp/bin/cargo reclaim");
        task_request.state_dir = Some(PathBuf::from("/tmp/state & logs/state"));
        task_request.log_dir = Some(PathBuf::from("/tmp/state & logs/logs"));
        task_request
    })?;
    let xml = task
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::TaskSchedulerXml)
        .expect("xml");
    assert!(
        xml.contents
            .contains("-NoProfile -ExecutionPolicy Bypass -File &apos;/tmp/state &amp; logs/state/scheduler-runner.ps1&apos;")
    );
    Ok(())
}

#[test]
fn systemd_install_plan_writes_artifacts_and_registers_timer()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_install(request(SchedulerPlatform::SystemdUser))?;

    assert_eq!(plan.operation, SchedulerOperation::Install);
    assert!(plan.dry_run);
    assert!(has_write_step(
        &plan.steps,
        GeneratedArtifactKind::RunnerScript
    ));
    assert!(has_write_step(
        &plan.steps,
        GeneratedArtifactKind::SystemdService
    ));
    assert!(has_write_step(
        &plan.steps,
        GeneratedArtifactKind::SystemdTimer
    ));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        SchedulerPlanStep::SetExecutable { path }
            if path.display().to_string().ends_with("scheduler-runner.sh")
    )));
    assert!(has_command(
        &plan.steps,
        &["systemctl", "--user", "daemon-reload"]
    ));
    assert!(has_command(
        &plan.steps,
        &[
            "systemctl",
            "--user",
            "enable",
            "--now",
            "cargo-reclaim.timer"
        ]
    ));
    Ok(())
}

#[test]
fn systemd_uninstall_plan_disables_timer_and_removes_known_files()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_uninstall(request(SchedulerPlatform::SystemdUser))?;

    assert_eq!(plan.operation, SchedulerOperation::Uninstall);
    assert!(has_command(
        &plan.steps,
        &[
            "systemctl",
            "--user",
            "disable",
            "--now",
            "cargo-reclaim.timer"
        ]
    ));
    assert!(has_command(
        &plan.steps,
        &["systemctl", "--user", "daemon-reload"]
    ));
    assert!(has_remove_step(&plan.steps, "scheduler-runner.sh"));
    assert!(has_remove_step(&plan.steps, "cargo-reclaim.service"));
    assert!(has_remove_step(&plan.steps, "cargo-reclaim.timer"));
    assert!(matches!(
        plan.steps.last(),
        Some(SchedulerPlanStep::RunCommand { argv })
            if argv
                .iter()
                .map(String::as_str)
                .eq(["systemctl", "--user", "daemon-reload"])
    ));
    Ok(())
}

#[test]
fn uninstall_plan_does_not_require_cleanup_policy_allowance()
-> Result<(), Box<dyn std::error::Error>> {
    let mut request = request(SchedulerPlatform::SystemdUser);
    request.mode = SchedulerMode::Cleanup;
    request.policy = Some(PolicyKind::Aggressive);

    let plan = plan_scheduler_uninstall(request)?;

    assert_eq!(plan.operation, SchedulerOperation::Uninstall);
    assert!(has_remove_step(&plan.steps, "cargo-reclaim.timer"));
    Ok(())
}

#[test]
fn launchd_install_plan_uses_launchctl_with_plist_path() -> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_install(request(SchedulerPlatform::Launchd))?;
    let plist = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::LaunchdPlist)
        .expect("plist")
        .intended_install_path
        .display()
        .to_string();

    assert!(has_command(&plan.steps, &["launchctl", "unload", &plist]));
    assert!(has_command(
        &plan.steps,
        &["launchctl", "load", "-w", &plist]
    ));
    Ok(())
}

#[test]
fn task_scheduler_install_plan_uses_state_xml_path() -> Result<(), Box<dyn std::error::Error>> {
    let plan = plan_scheduler_install(request(SchedulerPlatform::TaskScheduler))?;
    let xml = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == GeneratedArtifactKind::TaskSchedulerXml)
        .expect("xml")
        .intended_install_path
        .display()
        .to_string();

    assert_eq!(xml, "/tmp/cargo reclaim/state/cargo-reclaim.xml");
    assert!(has_command(
        &plan.steps,
        &[
            "schtasks",
            "/Create",
            "/TN",
            r"\cargo-reclaim",
            "/XML",
            &xml,
            "/F"
        ]
    ));
    assert!(has_command(
        &plan_scheduler_uninstall(request(SchedulerPlatform::TaskScheduler))?.steps,
        &["schtasks", "/Delete", "/TN", r"\cargo-reclaim", "/F"]
    ));
    Ok(())
}

fn has_write_step(steps: &[SchedulerPlanStep], artifact_kind: GeneratedArtifactKind) -> bool {
    steps.iter().any(|step| {
        matches!(
            step,
            SchedulerPlanStep::WriteFile {
                artifact_kind: kind,
                ..
            } if *kind == artifact_kind
        )
    })
}

fn has_remove_step(steps: &[SchedulerPlanStep], suffix: &str) -> bool {
    steps.iter().any(|step| {
        matches!(
            step,
            SchedulerPlanStep::RemoveFile { path } if path.display().to_string().ends_with(suffix)
        )
    })
}

fn has_command(steps: &[SchedulerPlanStep], expected: &[&str]) -> bool {
    steps.iter().any(|step| {
        matches!(
            step,
            SchedulerPlanStep::RunCommand { argv }
                if argv.iter().map(String::as_str).eq(expected.iter().copied())
        )
    })
}
