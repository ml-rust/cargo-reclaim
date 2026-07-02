use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{CargoConfigRecommendRequest, build_cargo_config_recommend_report};

use super::super::{CliError, OutputFormat, next_path};
use super::json::write_json_recommend_report;
use super::terminal::write_terminal_recommend_report;

#[derive(Debug)]
pub(in crate::cli) struct CargoConfigCommand {
    project: PathBuf,
    output_format: OutputFormat,
}

pub(in crate::cli) fn parse_cargo_config_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CargoConfigCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "cargo-config requires `recommend`".to_string(),
        ));
    };
    let subcommand_text = subcommand.to_string_lossy();
    if subcommand_text != "recommend" {
        return Err(CliError::Usage(format!(
            "unknown cargo-config command `{}`; expected `recommend`",
            subcommand_text
        )));
    }

    let mut project = PathBuf::from(".");
    let mut output_format = OutputFormat::Terminal;
    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "cargo-config arguments must be valid UTF-8 options".to_string(),
            ));
        };
        match arg_text {
            "-h" | "--help" => {
                return Err(CliError::Usage(
                    "usage: cargo-reclaim cargo-config recommend [--project <path>] [--json]"
                        .to_string(),
                ));
            }
            "--project" => {
                project = next_path(&mut args, "--project")?;
            }
            value if value.starts_with("--project=") => {
                let path = &value["--project=".len()..];
                if path.is_empty() {
                    return Err(CliError::Usage("--project requires a value".to_string()));
                }
                project = PathBuf::from(path);
            }
            "--json" => output_format = OutputFormat::Json,
            "--apply" | "--yes" => {
                return Err(CliError::Usage(
                    "cargo-config recommend is read-only/dry-run only; no Cargo config files can be modified".to_string(),
                ));
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

    Ok(CargoConfigCommand {
        project,
        output_format,
    })
}

pub(in crate::cli) fn run_cargo_config_command(
    command: &CargoConfigCommand,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let report = build_cargo_config_recommend_report(CargoConfigRecommendRequest {
        project: command.project.clone(),
    })?;
    match command.output_format {
        OutputFormat::Terminal => write_terminal_recommend_report(stdout, &report)?,
        OutputFormat::Json => write_json_recommend_report(stdout, &report)?,
    }
    Ok(ExitCode::SUCCESS)
}
