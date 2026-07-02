use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    GeneratedArtifact, GeneratedArtifactKind, PolicyKind, Schedule, SchedulerMode,
    SchedulerPlatform, SchedulerReport, SchedulerRequest, generate_scheduler_artifacts,
    load_config_from_path,
};
use serde::Serialize;

use super::{CliError, OutputFormat, inline_config_path, next_path, next_value, parse_policy};

#[derive(Debug)]
pub(super) struct SchedulerPreviewCommand {
    request: SchedulerRequest,
    output_format: OutputFormat,
}

pub(super) fn parse_scheduler_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerPreviewCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "scheduler requires `preview`; install and uninstall are not supported".to_string(),
        ));
    };
    match subcommand.to_string_lossy().as_ref() {
        "preview" => parse_scheduler_preview(args),
        "-h" | "--help" | "help" => Err(CliError::Usage(
            "usage: cargo-reclaim scheduler preview --platform <systemd-user|launchd|task-scheduler> --config <path>".to_string(),
        )),
        value => Err(CliError::Usage(format!(
            "unknown scheduler command `{value}`; expected `preview`"
        ))),
    }
}

fn parse_scheduler_preview(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerPreviewCommand, CliError> {
    let mut platform = None;
    let mut config_path = None;
    let mut at = None;
    let mut mode = None;
    let mut policy = None;
    let mut allow_unattended_cleanup = false;
    let mut allow_unattended_high_policy = false;
    let mut cargo_reclaim_bin = None;
    let mut output_format = OutputFormat::Terminal;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "scheduler preview options must be valid UTF-8".to_string(),
            ));
        };

        match arg_text {
            "--platform" => platform = Some(parse_platform(&next_value(&mut args, "--platform")?)?),
            value if value.starts_with("--platform=") => {
                platform = Some(parse_platform(&value["--platform=".len()..])?);
            }
            "--config" => config_path = Some(next_path(&mut args, "--config")?),
            "--at" => at = Some(next_value(&mut args, "--at")?),
            value if value.starts_with("--at=") => at = Some(value["--at=".len()..].to_string()),
            "--mode" => mode = Some(parse_mode(&next_value(&mut args, "--mode")?)?),
            value if value.starts_with("--mode=") => {
                mode = Some(parse_mode(&value["--mode=".len()..])?);
            }
            "--policy" => policy = Some(parse_policy(&next_value(&mut args, "--policy")?)?),
            value if value.starts_with("--policy=") => {
                policy = Some(parse_policy(&value["--policy=".len()..])?);
            }
            "--allow-unattended-cleanup" => allow_unattended_cleanup = true,
            "--allow-unattended-high-policy" => allow_unattended_high_policy = true,
            "--cargo-reclaim-bin" => {
                cargo_reclaim_bin = Some(next_path(&mut args, "--cargo-reclaim-bin")?);
            }
            value if value.starts_with("--cargo-reclaim-bin=") => {
                cargo_reclaim_bin = Some(PathBuf::from(&value["--cargo-reclaim-bin=".len()..]));
            }
            "--json" => output_format = OutputFormat::Json,
            "-h" | "--help" => {
                return Err(CliError::Usage(
                    "usage: cargo-reclaim scheduler preview --platform <systemd-user|launchd|task-scheduler> --config <path>".to_string(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scheduler preview option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected scheduler preview argument `{value}`"
                )));
            }
        }
    }

    let platform = platform
        .ok_or_else(|| CliError::Usage("scheduler preview requires --platform".to_string()))?;
    let config_path = config_path
        .ok_or_else(|| CliError::Usage("scheduler preview requires --config".to_string()))?;
    let config = load_config_from_path(&config_path)?;
    let scheduler = &config.scheduler;
    let schedule = Schedule::parse(at.as_deref().or(scheduler.at.as_deref()).unwrap_or("03:00"))?;
    let mode = match mode {
        Some(mode) => mode,
        None => scheduler
            .mode
            .as_deref()
            .map(parse_mode)
            .transpose()?
            .unwrap_or(SchedulerMode::Observe),
    };
    let policy = match policy {
        Some(policy) => Some(policy),
        None => scheduler.policy.as_deref().map(parse_policy).transpose()?,
    };
    let request = SchedulerRequest {
        platform,
        config_path,
        cargo_reclaim_bin: cargo_reclaim_bin.unwrap_or_else(default_cargo_reclaim_bin),
        schedule,
        mode,
        policy,
        allow_unattended_cleanup: allow_unattended_cleanup
            || scheduler.allow_unattended_cleanup.unwrap_or(false),
        allow_unattended_high_policy: allow_unattended_high_policy
            || scheduler.allow_unattended_high_policy.unwrap_or(false),
        state_dir: scheduler.state_dir.clone(),
        log_dir: scheduler.log_dir.clone(),
    };

    Ok(SchedulerPreviewCommand {
        request,
        output_format,
    })
}

pub(super) fn run_scheduler_preview(
    command: &SchedulerPreviewCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let report = generate_scheduler_artifacts(command.request.clone())?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal(output, &report)?,
        OutputFormat::Json => write_json(output, &report)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_platform(value: &str) -> Result<SchedulerPlatform, CliError> {
    match value {
        "systemd-user" => Ok(SchedulerPlatform::SystemdUser),
        "launchd" => Ok(SchedulerPlatform::Launchd),
        "task-scheduler" => Ok(SchedulerPlatform::TaskScheduler),
        _ => Err(CliError::Usage(format!(
            "unknown scheduler platform `{value}`; expected systemd-user, launchd, or task-scheduler"
        ))),
    }
}

fn parse_mode(value: &str) -> Result<SchedulerMode, CliError> {
    match value {
        "observe" => Ok(SchedulerMode::Observe),
        "cleanup" => Ok(SchedulerMode::Cleanup),
        _ => Err(CliError::Usage(format!(
            "unknown scheduler mode `{value}`; expected observe or cleanup"
        ))),
    }
}

fn default_cargo_reclaim_bin() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cargo-reclaim"))
}

fn write_terminal(output: &mut impl Write, report: &SchedulerReport) -> Result<(), CliError> {
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

fn write_json(output: &mut impl Write, report: &SchedulerReport) -> Result<(), CliError> {
    let document = JsonSchedulerReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
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

fn platform_label(platform: SchedulerPlatform) -> &'static str {
    match platform {
        SchedulerPlatform::SystemdUser => "systemd-user",
        SchedulerPlatform::Launchd => "launchd",
        SchedulerPlatform::TaskScheduler => "task-scheduler",
    }
}

fn mode_label(mode: SchedulerMode) -> &'static str {
    match mode {
        SchedulerMode::Observe => "observe",
        SchedulerMode::Cleanup => "cleanup",
    }
}

fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

fn artifact_kind_label(kind: GeneratedArtifactKind) -> &'static str {
    match kind {
        GeneratedArtifactKind::SystemdService => "systemd-service",
        GeneratedArtifactKind::SystemdTimer => "systemd-timer",
        GeneratedArtifactKind::LaunchdPlist => "launchd-plist",
        GeneratedArtifactKind::TaskSchedulerXml => "task-scheduler-xml",
        GeneratedArtifactKind::RunnerScript => "runner-script",
    }
}
