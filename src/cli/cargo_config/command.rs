use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::config::{
    CargoConfigApplyRequest, CargoConfigPreviewRequest, apply_cargo_config_preview,
    build_cargo_config_preview_report,
};
use cargo_reclaim::{CargoConfigRecommendRequest, build_cargo_config_recommend_report};

use super::super::{CliError, OutputFormat, next_path};
use super::json::{
    write_json_apply_report, write_json_preview_report, write_json_recommend_report,
};
use super::terminal::{
    write_terminal_apply_report, write_terminal_preview_report, write_terminal_recommend_report,
};

#[derive(Debug)]
pub(in crate::cli) struct CargoConfigCommand {
    subcommand: CargoConfigSubcommand,
    project: PathBuf,
    preview_path: Option<PathBuf>,
    output_format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoConfigSubcommand {
    Recommend,
    Preview,
    Apply,
}

pub(in crate::cli) fn parse_cargo_config_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CargoConfigCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "cargo-config requires `recommend`, `preview`, or `apply`".to_string(),
        ));
    };
    let subcommand_text = subcommand.to_string_lossy();
    let subcommand = parse_subcommand(&subcommand_text)?;

    let mut project = PathBuf::from(".");
    let mut preview_path = None;
    let mut yes = false;
    let mut output_format = OutputFormat::Terminal;
    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "cargo-config arguments must be valid UTF-8 options".to_string(),
            ));
        };
        match arg_text {
            "-h" | "--help" => {
                return Err(CliError::Help(usage_for_subcommand(subcommand)));
            }
            "--project" => {
                reject_project_for_apply(subcommand)?;
                project = next_path(&mut args, "--project")?;
            }
            value if value.starts_with("--project=") => {
                reject_project_for_apply(subcommand)?;
                let path = &value["--project=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--project requires a value".to_string()));
                }
                project = PathBuf::from(path);
            }
            "--json" => output_format = OutputFormat::Json,
            "--preview" if subcommand == CargoConfigSubcommand::Apply => {
                preview_path = Some(next_path(&mut args, "--preview")?);
            }
            value
                if subcommand == CargoConfigSubcommand::Apply
                    && value.starts_with("--preview=") =>
            {
                let path = &value["--preview=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--preview requires a value".to_string()));
                }
                preview_path = Some(PathBuf::from(path));
            }
            "--yes" if subcommand == CargoConfigSubcommand::Apply => yes = true,
            "--apply" | "--yes" | "--preview" => {
                return Err(CliError::Usage(format!(
                    "cargo-config {} is read-only/dry-run only; no Cargo config files can be modified",
                    subcommand.name()
                )));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{value}`")));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected cargo-config argument `{value}`"
                )));
            }
        }
    }

    if subcommand == CargoConfigSubcommand::Apply {
        if preview_path.is_none() {
            return Err(CliError::Usage(
                "cargo-config apply requires --preview <path>".to_string(),
            ));
        }
        if !yes {
            return Err(CliError::Usage(
                "cargo-config apply requires --yes".to_string(),
            ));
        }
    }

    Ok(CargoConfigCommand {
        subcommand,
        project,
        preview_path,
        output_format,
    })
}

pub(in crate::cli) fn run_cargo_config_command(
    command: &CargoConfigCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command.subcommand {
        CargoConfigSubcommand::Recommend => run_recommend_command(command, stdout)?,
        CargoConfigSubcommand::Preview => run_preview_command(command, stdout)?,
        CargoConfigSubcommand::Apply => run_apply_command(command, stdout)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_recommend_command(
    command: &CargoConfigCommand,
    stdout: &mut impl Write,
) -> Result<(), CliError> {
    let report = build_cargo_config_recommend_report(CargoConfigRecommendRequest {
        project: command.project.clone(),
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_recommend_report(stdout, &report)?,
        OutputFormat::Json => write_json_recommend_report(stdout, &report)?,
    }
    Ok(())
}

fn run_preview_command(
    command: &CargoConfigCommand,
    stdout: &mut impl Write,
) -> Result<(), CliError> {
    let report = build_cargo_config_preview_report(CargoConfigPreviewRequest {
        project: command.project.clone(),
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_preview_report(stdout, &report)?,
        OutputFormat::Json => write_json_preview_report(stdout, &report)?,
    }
    Ok(())
}

fn run_apply_command(
    command: &CargoConfigCommand,
    stdout: &mut impl Write,
) -> Result<(), CliError> {
    let Some(preview_path) = command.preview_path.as_ref() else {
        return Err(CliError::Usage(
            "cargo-config apply requires --preview <path>".to_string(),
        ));
    };
    let report = apply_cargo_config_preview(CargoConfigApplyRequest {
        preview_path: preview_path.clone(),
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_apply_report(stdout, &report)?,
        OutputFormat::Json => write_json_apply_report(stdout, &report)?,
    }
    Ok(())
}

fn parse_subcommand(value: &str) -> Result<CargoConfigSubcommand, CliError> {
    match value {
        "recommend" => Ok(CargoConfigSubcommand::Recommend),
        "preview" => Ok(CargoConfigSubcommand::Preview),
        "apply" => Ok(CargoConfigSubcommand::Apply),
        value => Err(CliError::Usage(format!(
            "unknown cargo-config command `{value}`; expected `recommend`, `preview`, or `apply`"
        ))),
    }
}

impl CargoConfigSubcommand {
    fn name(self) -> &'static str {
        match self {
            Self::Recommend => "recommend",
            Self::Preview => "preview",
            Self::Apply => "apply",
        }
    }
}

fn usage_for_subcommand(subcommand: CargoConfigSubcommand) -> String {
    match subcommand {
        CargoConfigSubcommand::Recommend | CargoConfigSubcommand::Preview => format!(
            "usage: cargo-reclaim cargo-config {} [--project <path>] [--json]",
            subcommand.name()
        ),
        CargoConfigSubcommand::Apply => {
            "usage: cargo-reclaim cargo-config apply --preview <path> --yes [--json]".to_string()
        }
    }
}

fn reject_project_for_apply(subcommand: CargoConfigSubcommand) -> Result<(), CliError> {
    if subcommand == CargoConfigSubcommand::Apply {
        return Err(CliError::Usage(
            "cargo-config apply does not accept --project; use --preview <path>".to_string(),
        ));
    }
    Ok(())
}
