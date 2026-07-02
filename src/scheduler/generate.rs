use std::path::{Path, PathBuf};

use crate::PolicyKind;

use super::model::{
    GeneratedArtifact, GeneratedArtifactKind, SchedulerError, SchedulerMode, SchedulerPlatform,
    SchedulerReport, SchedulerRequest, policy_label,
};
use super::paths::SchedulerPaths;

pub fn generate_scheduler_artifacts(
    request: SchedulerRequest,
) -> Result<SchedulerReport, SchedulerError> {
    let effective_policy = effective_policy(&request)?;
    let paths = SchedulerPaths::new(&request);
    let artifacts = match request.platform {
        SchedulerPlatform::SystemdUser => systemd_artifacts(&request, effective_policy, &paths),
        SchedulerPlatform::Launchd => launchd_artifacts(&request, effective_policy, &paths),
        SchedulerPlatform::TaskScheduler => {
            task_scheduler_artifacts(&request, effective_policy, &paths)
        }
    };

    Ok(SchedulerReport {
        command: "scheduler-preview",
        dry_run: true,
        platform: request.platform,
        mode: request.mode,
        schedule: request.schedule,
        effective_policy,
        artifacts,
    })
}

fn effective_policy(request: &SchedulerRequest) -> Result<PolicyKind, SchedulerError> {
    match request.mode {
        SchedulerMode::Observe => Ok(request.policy.unwrap_or(PolicyKind::Observe)),
        SchedulerMode::Cleanup => {
            if !request.allow_unattended_cleanup {
                return Err(SchedulerError::CleanupNotAllowed);
            }
            let policy = request.policy.unwrap_or(PolicyKind::Conservative);
            if matches!(
                policy,
                PolicyKind::Balanced | PolicyKind::Aggressive | PolicyKind::Custom
            ) && !request.allow_unattended_high_policy
            {
                return Err(SchedulerError::HighPolicyNotAllowed(policy));
            }
            Ok(policy)
        }
    }
}

fn systemd_artifacts(
    request: &SchedulerRequest,
    policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: shell_runner(request, policy, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::SystemdService,
            intended_install_path: paths.systemd_service_path(),
            contents: format!(
                "[Unit]\nDescription=cargo-reclaim scheduled run\n\n[Service]\nType=oneshot\nExecStart={}\n",
                systemd_quote(&paths.runner_path)
            ),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::SystemdTimer,
            intended_install_path: paths.systemd_timer_path(),
            contents: format!(
                "[Unit]\nDescription=cargo-reclaim scheduled timer\n\n[Timer]\nOnCalendar=*-*-* {:02}:{:02}:00\nPersistent=true\nUnit=cargo-reclaim.service\n\n[Install]\nWantedBy=timers.target\n",
                request.schedule.hour, request.schedule.minute
            ),
        },
    ]
}

fn launchd_artifacts(
    request: &SchedulerRequest,
    policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: shell_runner(request, policy, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::LaunchdPlist,
            intended_install_path: paths.launchd_plist_path(),
            contents: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>com.cargo-reclaim</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n  </array>\n  <key>StartCalendarInterval</key>\n  <dict>\n    <key>Hour</key>\n    <integer>{}</integer>\n    <key>Minute</key>\n    <integer>{}</integer>\n  </dict>\n  <key>StandardOutPath</key>\n  <string>{}</string>\n  <key>StandardErrorPath</key>\n  <string>{}</string>\n</dict>\n</plist>\n",
                xml_escape(&paths.runner_path),
                request.schedule.hour,
                request.schedule.minute,
                xml_escape(&paths.log_path),
                xml_escape(&paths.log_path)
            ),
        },
    ]
}

fn task_scheduler_artifacts(
    request: &SchedulerRequest,
    policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: powershell_runner(request, policy, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::TaskSchedulerXml,
            intended_install_path: PathBuf::from(r"Task Scheduler Library\cargo-reclaim.xml"),
            contents: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  <Triggers>\n    <CalendarTrigger>\n      <StartBoundary>2024-01-01T{:02}:{:02}:00</StartBoundary>\n      <ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>\n    </CalendarTrigger>\n  </Triggers>\n  <Actions Context=\"Author\">\n    <Exec>\n      <Command>powershell.exe</Command>\n      <Arguments>{}</Arguments>\n    </Exec>\n  </Actions>\n</Task>\n",
                request.schedule.hour,
                request.schedule.minute,
                xml_escape_str(&format!(
                    "-NoProfile -ExecutionPolicy Bypass -File {}",
                    powershell_quote(&paths.runner_path)
                ))
            ),
        },
    ]
}

fn shell_runner(request: &SchedulerRequest, policy: PolicyKind, paths: &SchedulerPaths) -> String {
    let plan_command = format!(
        "{} plan --config {} --policy {} --save-plan \"$PLAN_PATH\" --expires-in 1h --json >> \"$LOG_PATH\" 2>&1",
        shell_quote(&request.cargo_reclaim_bin),
        shell_quote(&request.config_path),
        shell_quote_str(policy_label(policy))
    );
    let apply_command = if request.mode == SchedulerMode::Cleanup {
        format!(
            "\n{} apply --plan \"$PLAN_PATH\" --yes --json >> \"$LOG_PATH\" 2>&1",
            shell_quote(&request.cargo_reclaim_bin)
        )
    } else {
        String::new()
    };

    format!(
        "#!/bin/sh\nset -eu\nSTATE_DIR={}\nLOG_DIR={}\nPLANS_DIR={}\nLOG_PATH={}\nmkdir -p \"$PLANS_DIR\" \"$LOG_DIR\"\nSTAMP=\"$(date -u +%Y%m%dT%H%M%SZ)\"\nPLAN_PATH=\"$PLANS_DIR/cargo-reclaim-$STAMP.json\"\n{}{}\n",
        shell_quote(&paths.state_dir),
        shell_quote(&paths.log_dir),
        shell_quote(&paths.plans_dir),
        shell_quote(&paths.log_path),
        plan_command,
        apply_command
    )
}

fn powershell_runner(
    request: &SchedulerRequest,
    policy: PolicyKind,
    paths: &SchedulerPaths,
) -> String {
    let apply_command = if request.mode == SchedulerMode::Cleanup {
        format!(
            "\n& {} apply --plan $PlanPath --yes --json *>> $LogPath",
            powershell_quote(&request.cargo_reclaim_bin)
        )
    } else {
        String::new()
    };

    format!(
        "$ErrorActionPreference = 'Stop'\n$StateDir = {}\n$LogDir = {}\n$PlansDir = {}\n$LogPath = {}\nNew-Item -ItemType Directory -Force -Path $PlansDir, $LogDir | Out-Null\n$Stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')\n$PlanPath = Join-Path $PlansDir \"cargo-reclaim-$Stamp.json\"\n& {} plan --config {} --policy {} --save-plan $PlanPath --expires-in 1h --json *>> $LogPath{}\n",
        powershell_quote(&paths.state_dir),
        powershell_quote(&paths.log_dir),
        powershell_quote(&paths.plans_dir),
        powershell_quote(&paths.log_path),
        powershell_quote(&request.cargo_reclaim_bin),
        powershell_quote(&request.config_path),
        powershell_quote_str(policy_label(policy)),
        apply_command
    )
}

fn shell_quote(path: &Path) -> String {
    shell_quote_str(&path.display().to_string())
}

fn shell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn systemd_quote(path: &Path) -> String {
    let text = path.display().to_string();
    if text
        .chars()
        .all(|character| !character.is_whitespace() && character != '"')
    {
        text
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn powershell_quote(path: &Path) -> String {
    powershell_quote_str(&path.display().to_string())
}

fn powershell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn xml_escape(path: &Path) -> String {
    xml_escape_str(&path.display().to_string())
}

fn xml_escape_str(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
