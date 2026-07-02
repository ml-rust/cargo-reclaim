use std::path::PathBuf;

use cargo_reclaim::{
    GeneratedArtifactKind, PolicyKind, Schedule, SchedulerError, SchedulerMode, SchedulerPlatform,
    SchedulerRequest, generate_scheduler_artifacts,
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
