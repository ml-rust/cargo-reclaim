use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use cargo_reclaim::{
    CargoHomePlan, CargoHomePlanRequest, CargoHomeReportRequest, PolicyKind,
    SaveCargoHomePlanOptions, build_cargo_home_plan, build_cargo_home_report,
    execute_cargo_home_plan_apply, load_cargo_home_plan_from_path, persist_cargo_home_plan,
    save_cargo_home_plan_to_path, validate_cargo_home_plan_for_apply,
};

use super::super::persistence::{SavePlanRequest, parse_duration};
use super::super::{CliError, OutputFormat, next_path, next_value};
use super::json::{write_json_apply_report, write_json_plan, write_json_report};
use super::terminal::{write_terminal_apply_report, write_terminal_plan, write_terminal_report};

#[derive(Debug)]
pub(in crate::cli) struct CargoHomeCommand {
    kind: CargoHomeCommandKind,
    cargo_home: Option<PathBuf>,
    policy: PolicyKind,
    output_format: OutputFormat,
    save_plan: Option<SavePlanRequest>,
    plan_path: Option<PathBuf>,
    execute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoHomeCommandKind {
    Report,
    Plan,
    Apply,
}

pub(in crate::cli) fn parse_cargo_home_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CargoHomeCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "cargo-home requires `report`, `plan`, or `apply`".to_string(),
        ));
    };
    let kind = match subcommand.to_string_lossy().as_ref() {
        "report" => CargoHomeCommandKind::Report,
        "plan" => CargoHomeCommandKind::Plan,
        "apply" => CargoHomeCommandKind::Apply,
        value => {
            return Err(CliError::Usage(format!(
                "unknown cargo-home command `{value}`; expected `report`, `plan`, or `apply`"
            )));
        }
    };

    let mut cargo_home = None;
    let mut policy = PolicyKind::default();
    let mut output_format = OutputFormat::Terminal;
    let mut save_plan = None;
    let mut expires_in = None;
    let mut plan_path = None;
    let mut execute = false;
    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "cargo-home arguments must be valid UTF-8 options".to_string(),
            ));
        };
        match arg_text {
            "-h" | "--help" => {
                return Err(CliError::Help(usage_for_kind(kind).to_string()));
            }
            "--cargo-home" => {
                if kind == CargoHomeCommandKind::Apply {
                    return Err(CliError::Usage(
                        "`--cargo-home` is not supported by `cargo-home apply`; pass `--plan <path>`"
                            .to_string(),
                    ));
                }
                cargo_home = Some(next_path(&mut args, "--cargo-home")?);
            }
            value if value.starts_with("--cargo-home=") => {
                if kind == CargoHomeCommandKind::Apply {
                    return Err(CliError::Usage(
                        "`--cargo-home` is not supported by `cargo-home apply`; pass `--plan <path>`"
                            .to_string(),
                    ));
                }
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
            "--save-plan" if kind == CargoHomeCommandKind::Plan => {
                save_plan = Some(SavePlanRequest::new(next_path(&mut args, "--save-plan")?));
            }
            value if kind == CargoHomeCommandKind::Plan && value.starts_with("--save-plan=") => {
                let path = &value["--save-plan=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--save-plan requires a value".to_string()));
                }
                save_plan = Some(SavePlanRequest::new(PathBuf::from(path)));
            }
            "--expires-in" if kind == CargoHomeCommandKind::Plan => {
                expires_in = Some(parse_duration(&next_value(&mut args, "--expires-in")?)?);
            }
            value if kind == CargoHomeCommandKind::Plan && value.starts_with("--expires-in=") => {
                expires_in = Some(parse_duration(&value["--expires-in=".len()..])?);
            }
            "--plan" if kind == CargoHomeCommandKind::Apply => {
                plan_path = Some(next_plan_path(&mut args)?);
            }
            value if kind == CargoHomeCommandKind::Apply && value.starts_with("--plan=") => {
                let path = &value["--plan=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--plan requires a value".to_string()));
                }
                plan_path = Some(validate_plan_path(PathBuf::from(path))?);
            }
            "--json" => output_format = OutputFormat::Json,
            "--yes" if kind == CargoHomeCommandKind::Apply => execute = true,
            "--last" | "last" if kind == CargoHomeCommandKind::Apply => {
                return Err(CliError::Usage(
                    "implicit `last` plans are not supported; pass an explicit `--plan <path>`"
                        .to_string(),
                ));
            }
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

    if let Some(expires_in) = expires_in {
        let Some(save_plan) = save_plan.as_mut() else {
            return Err(CliError::Usage(
                "`--expires-in` requires `--save-plan`".to_string(),
            ));
        };
        save_plan.set_expires_in(expires_in);
    }

    if kind == CargoHomeCommandKind::Apply && plan_path.is_none() {
        return Err(CliError::Usage(
            "cargo-home apply requires an explicit `--plan <path>`".to_string(),
        ));
    }

    Ok(CargoHomeCommand {
        kind,
        cargo_home,
        policy,
        output_format,
        save_plan,
        plan_path,
        execute,
    })
}

pub(in crate::cli) fn run_cargo_home_command(
    command: &CargoHomeCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command.kind {
        CargoHomeCommandKind::Report => run_cargo_home_report(command, stdout),
        CargoHomeCommandKind::Plan => run_cargo_home_plan(command, stdout),
        CargoHomeCommandKind::Apply => run_cargo_home_apply(command, stdout),
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
    if let Some(save_plan) = &command.save_plan {
        save_cargo_home_plan(&plan, save_plan)?;
    }
    match command.output_format {
        OutputFormat::Terminal => write_terminal_plan(stdout, &plan)?,
        OutputFormat::Json => write_json_plan(stdout, &plan)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_cargo_home_apply(
    command: &CargoHomeCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let plan_path = command.plan_path.as_ref().ok_or_else(|| {
        CliError::Usage("cargo-home apply requires an explicit `--plan <path>`".to_string())
    })?;
    let document = load_cargo_home_plan_from_path(plan_path)?;
    let report = if command.execute {
        execute_cargo_home_plan_apply(&document, SystemTime::now())?
    } else {
        validate_cargo_home_plan_for_apply(&document, SystemTime::now())?
    };
    match command.output_format {
        OutputFormat::Terminal => write_terminal_apply_report(stdout, &report)?,
        OutputFormat::Json => write_json_apply_report(stdout, &report)?,
    }
    let exit_code = if report.totals.failed_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };
    Ok(exit_code)
}

fn save_cargo_home_plan(plan: &CargoHomePlan, request: &SavePlanRequest) -> Result<(), CliError> {
    let created_at = SystemTime::now();
    let expires_at = created_at
        .checked_add(request.expires_in)
        .ok_or_else(|| CliError::Usage("plan expiry duration is too large".to_string()))?;
    let document = persist_cargo_home_plan(
        plan,
        SaveCargoHomePlanOptions {
            created_at,
            expires_at,
        },
    )?;
    save_cargo_home_plan_to_path(&request.path, &document)?;
    Ok(())
}

fn next_plan_path(args: &mut impl Iterator<Item = OsString>) -> Result<PathBuf, CliError> {
    let path = next_path(args, "--plan")?;
    validate_plan_path(path)
}

fn validate_plan_path(path: PathBuf) -> Result<PathBuf, CliError> {
    if path.as_os_str() == "last" {
        return Err(CliError::Usage(
            "implicit `last` plans are not supported; pass an explicit plan path".to_string(),
        ));
    }
    Ok(path)
}

fn usage_for_kind(kind: CargoHomeCommandKind) -> &'static str {
    match kind {
        CargoHomeCommandKind::Report => {
            "usage: cargo-reclaim cargo-home report [--cargo-home <path>] [--json]"
        }
        CargoHomeCommandKind::Plan => {
            "usage: cargo-reclaim cargo-home plan [--cargo-home <path>] [--policy <kind>] [--save-plan <path>] [--expires-in <duration>] [--json]"
        }
        CargoHomeCommandKind::Apply => {
            "usage: cargo-reclaim cargo-home apply --plan <path> [--yes] [--json]"
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
