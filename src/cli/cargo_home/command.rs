use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    CargoHomePlanRequest, CargoHomeReportRequest, PolicyKind, build_cargo_home_plan,
    build_cargo_home_report,
};

use super::super::{CliError, OutputFormat, next_path};
use super::json::{write_json_plan, write_json_report};
use super::terminal::{write_terminal_plan, write_terminal_report};

#[derive(Debug)]
pub(in crate::cli) struct CargoHomeCommand {
    kind: CargoHomeCommandKind,
    cargo_home: Option<PathBuf>,
    policy: PolicyKind,
    output_format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoHomeCommandKind {
    Report,
    Plan,
}

pub(in crate::cli) fn parse_cargo_home_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CargoHomeCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "cargo-home requires `report` or `plan`".to_string(),
        ));
    };
    let kind = match subcommand.to_string_lossy().as_ref() {
        "report" => CargoHomeCommandKind::Report,
        "plan" => CargoHomeCommandKind::Plan,
        value => {
            return Err(CliError::Usage(format!(
                "unknown cargo-home command `{value}`; expected `report` or `plan`"
            )));
        }
    };

    let mut cargo_home = None;
    let mut policy = PolicyKind::default();
    let mut output_format = OutputFormat::Terminal;
    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "cargo-home arguments must be valid UTF-8 options".to_string(),
            ));
        };
        match arg_text {
            "-h" | "--help" => {
                return Err(CliError::Usage(usage_for_kind(kind).to_string()));
            }
            "--cargo-home" => {
                cargo_home = Some(next_path(&mut args, "--cargo-home")?);
            }
            value if value.starts_with("--cargo-home=") => {
                let path = &value["--cargo-home=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--cargo-home requires a value".to_string()));
                }
                cargo_home = Some(PathBuf::from(path));
            }
            "--policy" if kind == CargoHomeCommandKind::Plan => {
                let value = super::super::next_value(&mut args, "--policy")?;
                policy = parse_policy(&value)?;
            }
            value if kind == CargoHomeCommandKind::Plan && value.starts_with("--policy=") => {
                policy = parse_policy(&value["--policy=".len()..])?;
            }
            "--json" => output_format = OutputFormat::Json,
            "--apply" | "--yes" if kind == CargoHomeCommandKind::Plan => {
                return Err(CliError::Usage(
                    "cargo-home plan is dry-run only; apply is not available".to_string(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{value}`")));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected cargo-home argument `{value}`"
                )));
            }
        }
    }

    Ok(CargoHomeCommand {
        kind,
        cargo_home,
        policy,
        output_format,
    })
}

pub(in crate::cli) fn run_cargo_home_command(
    command: &CargoHomeCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command.kind {
        CargoHomeCommandKind::Report => run_cargo_home_report(command, stdout),
        CargoHomeCommandKind::Plan => run_cargo_home_plan(command, stdout),
    }
}

fn run_cargo_home_report(
    command: &CargoHomeCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let report = build_cargo_home_report(CargoHomeReportRequest {
        cargo_home: command.cargo_home.clone(),
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_report(stdout, &report)?,
        OutputFormat::Json => write_json_report(stdout, &report)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_cargo_home_plan(
    command: &CargoHomeCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let plan = build_cargo_home_plan(CargoHomePlanRequest {
        cargo_home: command.cargo_home.clone(),
        policy: command.policy,
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_plan(stdout, &plan)?,
        OutputFormat::Json => write_json_plan(stdout, &plan)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn usage_for_kind(kind: CargoHomeCommandKind) -> &'static str {
    match kind {
        CargoHomeCommandKind::Report => {
            "usage: cargo-reclaim cargo-home report [--cargo-home <path>] [--json]"
        }
        CargoHomeCommandKind::Plan => {
            "usage: cargo-reclaim cargo-home plan [--cargo-home <path>] [--policy <kind>] [--json]"
        }
    }
}

fn parse_policy(value: &str) -> Result<PolicyKind, CliError> {
    match value {
        "observe" => Ok(PolicyKind::Observe),
        "conservative" => Ok(PolicyKind::Conservative),
        "balanced" => Ok(PolicyKind::Balanced),
        "aggressive" => Ok(PolicyKind::Aggressive),
        "custom" => Ok(PolicyKind::Custom),
        _ => Err(CliError::Usage(format!(
            "unknown policy `{value}`; expected observe, conservative, balanced, aggressive, or custom"
        ))),
    }
}
