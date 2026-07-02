use std::path::{Path, PathBuf};

use super::generate::generate_scheduler_artifacts;
use super::model::{
    GeneratedArtifact, GeneratedArtifactKind, SchedulerError, SchedulerOperation,
    SchedulerOperationPlan, SchedulerPlanStep, SchedulerPlatform, SchedulerRequest,
};
use super::paths::SchedulerPaths;

pub fn plan_scheduler_install(
    request: SchedulerRequest,
) -> Result<SchedulerOperationPlan, SchedulerError> {
    let paths = SchedulerPaths::new(&request);
    let report = generate_scheduler_artifacts(request)?;
    let artifacts = operation_artifacts(report.platform, report.artifacts, &paths);
    let mut steps = ensure_artifact_dirs(&artifacts);

    steps.extend(
        artifacts
            .iter()
            .map(|artifact| SchedulerPlanStep::WriteFile {
                path: artifact.intended_install_path.clone(),
                artifact_kind: artifact.kind,
            }),
    );

    steps.extend(
        artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == GeneratedArtifactKind::RunnerScript
                    && matches!(
                        report.platform,
                        SchedulerPlatform::SystemdUser | SchedulerPlatform::Launchd
                    )
            })
            .map(|artifact| SchedulerPlanStep::SetExecutable {
                path: artifact.intended_install_path.clone(),
            }),
    );

    steps.extend(install_commands(report.platform, &artifacts));

    Ok(SchedulerOperationPlan {
        command: "scheduler-install",
        operation: SchedulerOperation::Install,
        dry_run: true,
        platform: report.platform,
        artifacts,
        steps,
    })
}

pub fn plan_scheduler_uninstall(
    request: SchedulerRequest,
) -> Result<SchedulerOperationPlan, SchedulerError> {
    let paths = SchedulerPaths::new(&request);
    let platform = request.platform;
    let artifacts = uninstall_artifacts(platform, &paths);
    let mut steps = uninstall_commands(platform);

    steps.extend(
        artifacts
            .iter()
            .map(|artifact| SchedulerPlanStep::RemoveFile {
                path: artifact.intended_install_path.clone(),
            }),
    );
    if platform == SchedulerPlatform::SystemdUser {
        steps.push(run_command(["systemctl", "--user", "daemon-reload"]));
    }

    Ok(SchedulerOperationPlan {
        command: "scheduler-uninstall",
        operation: SchedulerOperation::Uninstall,
        dry_run: true,
        platform,
        artifacts,
        steps,
    })
}

fn uninstall_artifacts(
    platform: SchedulerPlatform,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    match platform {
        SchedulerPlatform::SystemdUser => vec![
            GeneratedArtifact {
                kind: GeneratedArtifactKind::RunnerScript,
                intended_install_path: paths.runner_path.clone(),
                contents: String::new(),
            },
            GeneratedArtifact {
                kind: GeneratedArtifactKind::SystemdService,
                intended_install_path: paths.systemd_service_path(),
                contents: String::new(),
            },
            GeneratedArtifact {
                kind: GeneratedArtifactKind::SystemdTimer,
                intended_install_path: paths.systemd_timer_path(),
                contents: String::new(),
            },
        ],
        SchedulerPlatform::Launchd => vec![
            GeneratedArtifact {
                kind: GeneratedArtifactKind::RunnerScript,
                intended_install_path: paths.runner_path.clone(),
                contents: String::new(),
            },
            GeneratedArtifact {
                kind: GeneratedArtifactKind::LaunchdPlist,
                intended_install_path: paths.launchd_plist_path(),
                contents: String::new(),
            },
        ],
        SchedulerPlatform::TaskScheduler => vec![
            GeneratedArtifact {
                kind: GeneratedArtifactKind::RunnerScript,
                intended_install_path: paths.runner_path.clone(),
                contents: String::new(),
            },
            GeneratedArtifact {
                kind: GeneratedArtifactKind::TaskSchedulerXml,
                intended_install_path: paths.task_scheduler_xml_path(),
                contents: String::new(),
            },
        ],
    }
}

fn operation_artifacts(
    platform: SchedulerPlatform,
    artifacts: Vec<GeneratedArtifact>,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    artifacts
        .into_iter()
        .map(|mut artifact| {
            if platform == SchedulerPlatform::TaskScheduler
                && artifact.kind == GeneratedArtifactKind::TaskSchedulerXml
            {
                artifact.intended_install_path = paths.task_scheduler_xml_path();
            }
            artifact
        })
        .collect()
}

fn ensure_artifact_dirs(artifacts: &[GeneratedArtifact]) -> Vec<SchedulerPlanStep> {
    let mut dirs = Vec::new();
    for artifact in artifacts {
        let Some(parent) = artifact.intended_install_path.parent() else {
            continue;
        };
        if parent.as_os_str().is_empty() || contains_path(&dirs, parent) {
            continue;
        }
        dirs.push(parent.to_path_buf());
    }
    dirs.into_iter()
        .map(|path| SchedulerPlanStep::EnsureDir { path })
        .collect()
}

fn contains_path(paths: &[PathBuf], path: &Path) -> bool {
    paths.iter().any(|known| known == path)
}

fn install_commands(
    platform: SchedulerPlatform,
    artifacts: &[GeneratedArtifact],
) -> Vec<SchedulerPlanStep> {
    match platform {
        SchedulerPlatform::SystemdUser => vec![
            run_command(["systemctl", "--user", "daemon-reload"]),
            run_command([
                "systemctl",
                "--user",
                "enable",
                "--now",
                "cargo-reclaim.service",
                "cargo-reclaim.timer",
            ]),
        ],
        SchedulerPlatform::Launchd => plist_path(artifacts)
            .map(|path| {
                vec![
                    run_command(["launchctl", "unload", path.as_str()]),
                    run_command(["launchctl", "load", "-w", path.as_str()]),
                ]
            })
            .unwrap_or_default(),
        SchedulerPlatform::TaskScheduler => task_xml_path(artifacts)
            .map(|path| {
                vec![run_command([
                    "schtasks",
                    "/Create",
                    "/TN",
                    r"\cargo-reclaim",
                    "/XML",
                    path.as_str(),
                    "/F",
                ])]
            })
            .unwrap_or_default(),
    }
}

fn uninstall_commands(platform: SchedulerPlatform) -> Vec<SchedulerPlanStep> {
    match platform {
        SchedulerPlatform::SystemdUser => vec![run_command([
            "systemctl",
            "--user",
            "disable",
            "--now",
            "cargo-reclaim.service",
            "cargo-reclaim.timer",
        ])],
        SchedulerPlatform::Launchd => {
            vec![run_command(["launchctl", "remove", "com.cargo-reclaim"])]
        }
        SchedulerPlatform::TaskScheduler => vec![run_command([
            "schtasks",
            "/Delete",
            "/TN",
            r"\cargo-reclaim",
            "/F",
        ])],
    }
}

fn plist_path(artifacts: &[GeneratedArtifact]) -> Option<String> {
    artifact_path(artifacts, GeneratedArtifactKind::LaunchdPlist)
}

fn task_xml_path(artifacts: &[GeneratedArtifact]) -> Option<String> {
    artifact_path(artifacts, GeneratedArtifactKind::TaskSchedulerXml)
}

fn artifact_path(artifacts: &[GeneratedArtifact], kind: GeneratedArtifactKind) -> Option<String> {
    artifacts
        .iter()
        .find(|artifact| artifact.kind == kind)
        .map(|artifact| artifact.intended_install_path.display().to_string())
}

fn run_command<const N: usize>(argv: [&str; N]) -> SchedulerPlanStep {
    SchedulerPlanStep::RunCommand {
        argv: argv.into_iter().map(ToOwned::to_owned).collect(),
    }
}
