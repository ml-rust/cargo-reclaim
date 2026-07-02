use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    Schedule, SchedulerMode, SchedulerPlatform, SchedulerRequest, generate_scheduler_artifacts,
    load_config_from_path, plan_scheduler_install, plan_scheduler_uninstall,
};

use super::{CliError, OutputFormat, inline_config_path, next_path, next_value, parse_policy};
use output::{
    write_operation_json, write_operation_terminal, write_preview_json, write_preview_terminal,
};

mod output;

#[derive(Debug)]
pub(super) enum SchedulerCommand {
    Preview(SchedulerRequestCommand),
    Install(SchedulerRequestCommand),
    Uninstall(SchedulerRequestCommand),
}

pub(super) type SchedulerPreviewCommand = SchedulerCommand;

#[derive(Debug)]
pub(super) struct SchedulerRequestCommand {
    request: SchedulerRequest,
    output_format: OutputFormat,
}

pub(super) fn parse_scheduler_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "scheduler requires `preview`, `install`, or `uninstall`".to_string(),
        ));
    };
    match subcommand.to_string_lossy().as_ref() {
        "preview" => parse_scheduler_request("preview", args)
            .map(SchedulerRequestParse::into_request_command)
            .map(SchedulerCommand::Preview),
        "install" => parse_scheduler_operation("install", args).map(SchedulerCommand::Install),
        "uninstall" => {
            parse_scheduler_operation("uninstall", args).map(SchedulerCommand::Uninstall)
        }
        "-h" | "--help" | "help" => Err(CliError::Usage(scheduler_help().to_string())),
        value => Err(CliError::Usage(format!(
            "unknown scheduler command `{value}`; expected `preview`, `install`, or `uninstall`"
        ))),
    }
}

fn parse_scheduler_operation(
    subcommand: &'static str,
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerRequestCommand, CliError> {
    let command = parse_scheduler_request(subcommand, args)?;
    if !command.request_dry_run {
        return Err(CliError::Usage(format!(
            "scheduler {subcommand} requires --dry-run; execution is not available yet"
        )));
    }
    Ok(command.into_request_command())
}

fn parse_scheduler_request(
    subcommand: &'static str,
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerRequestParse, CliError> {
    let mut platform = None;
    let mut config_path = None;
    let mut at = None;
    let mut mode = None;
    let mut policy = None;
    let mut allow_unattended_cleanup = false;
    let mut allow_unattended_high_policy = false;
    let mut cargo_reclaim_bin = None;
    let mut output_format = OutputFormat::Terminal;
    let mut request_dry_run = subcommand == "preview";
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(format!(
                "scheduler {subcommand} options must be valid UTF-8"
            )));
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
            "--dry-run" => request_dry_run = true,
            "-h" | "--help" => {
                return Err(CliError::Usage(scheduler_subcommand_usage(subcommand)));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scheduler {subcommand} option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected scheduler {subcommand} argument `{value}`"
                )));
            }
        }
    }

    let platform = platform
        .ok_or_else(|| CliError::Usage(format!("scheduler {subcommand} requires --platform")))?;
    let config_path = config_path
        .ok_or_else(|| CliError::Usage(format!("scheduler {subcommand} requires --config")))?;
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

    Ok(SchedulerRequestParse {
        request,
        output_format,
        request_dry_run,
    })
}

pub(super) fn run_scheduler_preview(
    command: &SchedulerCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command {
        SchedulerCommand::Preview(command) => {
            let report = generate_scheduler_artifacts(command.request.clone())?;
            match command.output_format {
                OutputFormat::Terminal => write_preview_terminal(output, &report)?,
                OutputFormat::Json => write_preview_json(output, &report)?,
            }
        }
        SchedulerCommand::Install(command) => {
            let plan = plan_scheduler_install(command.request.clone())?;
            match command.output_format {
                OutputFormat::Terminal => write_operation_terminal(output, &plan)?,
                OutputFormat::Json => write_operation_json(output, &plan)?,
            }
        }
        SchedulerCommand::Uninstall(command) => {
            let plan = plan_scheduler_uninstall(command.request.clone())?;
            match command.output_format {
                OutputFormat::Terminal => write_operation_terminal(output, &plan)?,
                OutputFormat::Json => write_operation_json(output, &plan)?,
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug)]
struct SchedulerRequestParse {
    request: SchedulerRequest,
    output_format: OutputFormat,
    request_dry_run: bool,
}

impl SchedulerRequestParse {
    fn into_request_command(self) -> SchedulerRequestCommand {
        SchedulerRequestCommand {
            request: self.request,
            output_format: self.output_format,
        }
    }
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

fn scheduler_help() -> &'static str {
    "usage: cargo-reclaim scheduler <preview|install|uninstall> --platform <systemd-user|launchd|task-scheduler> --config <path>"
}

fn scheduler_subcommand_usage(subcommand: &str) -> String {
    if subcommand == "preview" {
        "usage: cargo-reclaim scheduler preview --platform <systemd-user|launchd|task-scheduler> --config <path>".to_string()
    } else {
        format!(
            "usage: cargo-reclaim scheduler {subcommand} --dry-run --platform <systemd-user|launchd|task-scheduler> --config <path>"
        )
    }
}
