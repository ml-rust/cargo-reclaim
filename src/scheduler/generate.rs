use std::path::{Path, PathBuf};

use crate::PolicyKind;

use super::model::{
    GeneratedArtifact, GeneratedArtifactKind, SchedulerError, SchedulerMode, SchedulerPlatform,
    SchedulerReport, SchedulerRequest,
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
    _policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: shell_runner(request, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::SystemdService,
            intended_install_path: paths.systemd_service_path(),
            contents: format!(
                "[Unit]\nDescription=cargo-reclaim resident background service\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=30\n\n[Install]\nWantedBy=default.target\n",
                systemd_quote(&paths.runner_path)
            ),
        },
    ]
}

fn launchd_artifacts(
    request: &SchedulerRequest,
    _policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: shell_runner(request, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::LaunchdPlist,
            intended_install_path: paths.launchd_plist_path(),
            contents: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>com.cargo-reclaim</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n  </array>\n  <key>KeepAlive</key>\n  <true/>\n  <key>RunAtLoad</key>\n  <true/>\n  <key>StandardOutPath</key>\n  <string>{}</string>\n  <key>StandardErrorPath</key>\n  <string>{}</string>\n</dict>\n</plist>\n",
                xml_escape(&paths.runner_path),
                xml_escape(&paths.log_path),
                xml_escape(&paths.log_path)
            ),
        },
    ]
}

fn task_scheduler_artifacts(
    request: &SchedulerRequest,
    _policy: PolicyKind,
    paths: &SchedulerPaths,
) -> Vec<GeneratedArtifact> {
    vec![
        GeneratedArtifact {
            kind: GeneratedArtifactKind::RunnerScript,
            intended_install_path: paths.runner_path.clone(),
            contents: powershell_runner(request, paths),
        },
        GeneratedArtifact {
            kind: GeneratedArtifactKind::TaskSchedulerXml,
            intended_install_path: PathBuf::from(r"Task Scheduler Library\cargo-reclaim.xml"),
            contents: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  <Triggers>\n    <LogonTrigger><Enabled>true</Enabled></LogonTrigger>\n  </Triggers>\n  <Settings>\n    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n    <RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>3</Count>\n    </RestartOnFailure>\n  </Settings>\n  <Actions Context=\"Author\">\n    <Exec>\n      <Command>powershell.exe</Command>\n      <Arguments>{}</Arguments>\n    </Exec>\n  </Actions>\n</Task>\n",
                xml_escape_str(&format!(
                    "-NoProfile -ExecutionPolicy Bypass -File {}",
                    powershell_quote(&paths.runner_path)
                ))
            ),
        },
    ]
}

fn shell_runner(request: &SchedulerRequest, paths: &SchedulerPaths) -> String {
    format!(
        "#!/bin/sh\nset -eu\nSTATE_DIR={}\nLOG_DIR={}\nLOG_PATH={}\nmkdir -p \"$STATE_DIR\" \"$LOG_DIR\"\nexec {} scheduler service run --config {} --json >> \"$LOG_PATH\" 2>&1\n",
        shell_quote(&paths.state_dir),
        shell_quote(&paths.log_dir),
        shell_quote(&paths.log_path),
        shell_quote(&request.cargo_reclaim_bin),
        shell_quote(&request.config_path)
    )
}

fn powershell_runner(request: &SchedulerRequest, paths: &SchedulerPaths) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n$StateDir = {}\n$LogDir = {}\n$LogPath = {}\nNew-Item -ItemType Directory -Force -Path $StateDir, $LogDir | Out-Null\n& {} scheduler service run --config {} --json *>> $LogPath\n",
        powershell_quote(&paths.state_dir),
        powershell_quote(&paths.log_dir),
        powershell_quote(&paths.log_path),
        powershell_quote(&request.cargo_reclaim_bin),
        powershell_quote(&request.config_path)
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
