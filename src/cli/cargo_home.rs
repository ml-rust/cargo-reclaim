use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cargo_reclaim::{
    CargoHomeClass, CargoHomeEntry, CargoHomePathKind, CargoHomeReport, CargoHomeReportRequest,
    CargoHomeSource, build_cargo_home_report,
};
use serde::Serialize;

use super::{CliError, OutputFormat, next_path};

#[derive(Debug)]
pub(super) struct CargoHomeCommand {
    cargo_home: Option<PathBuf>,
    output_format: OutputFormat,
}

pub(super) fn parse_cargo_home_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<CargoHomeCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage("cargo-home requires `report`".to_string()));
    };
    if subcommand.to_string_lossy() != "report" {
        return Err(CliError::Usage(format!(
            "unknown cargo-home command `{}`; expected `report`",
            subcommand.to_string_lossy()
        )));
    }

    let mut cargo_home = None;
    let mut output_format = OutputFormat::Terminal;
    while let Some(arg) = args.next() {
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "cargo-home report arguments must be valid UTF-8 options".to_string(),
            ));
        };
        match arg_text {
            "-h" | "--help" => {
                return Err(CliError::Usage(
                    "usage: cargo-reclaim cargo-home report [--cargo-home <path>] [--json]"
                        .to_string(),
                ));
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
            "--json" => output_format = OutputFormat::Json,
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{value}`")));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected cargo-home report argument `{value}`"
                )));
            }
        }
    }

    Ok(CargoHomeCommand {
        cargo_home,
        output_format,
    })
}

pub(super) fn run_cargo_home_report(
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

fn write_terminal_report(
    output: &mut impl Write,
    report: &CargoHomeReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim cargo-home report")?;
    writeln!(
        output,
        "dry-run/read-only; no files were deleted or modified"
    )?;
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(output, "root: {}", display_path(&report.input.root))?;
    writeln!(output, "source: {}", source_label(report.input.source))?;
    writeln!(output, "entries: {}", report.totals.entry_count)?;
    writeln!(
        output,
        "known cache entries: {}",
        report.totals.known_cache_entry_count
    )?;
    writeln!(output, "known cache bytes: {}", report.totals.cache_bytes)?;
    writeln!(output, "total bytes: {}", report.totals.total_bytes)?;
    writeln!(output, "preserved bytes: {}", report.totals.preserved_bytes)?;
    writeln!(
        output,
        "skipped/problem entries: {}",
        report.totals.skipped_count
    )?;
    writeln!(output, "problems: {}", report.totals.problem_count)?;

    if !report.recommendations.is_empty() {
        writeln!(output)?;
        for recommendation in &report.recommendations {
            writeln!(
                output,
                "recommendation: {}",
                display_text(&recommendation.message)
            )?;
        }
    }

    if report.entries.is_empty() {
        writeln!(output)?;
        writeln!(output, "no Cargo home reportable entries found")?;
        return Ok(());
    }

    writeln!(output)?;
    for entry in &report.entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            class_label(entry.class),
            path_kind_label(entry.path_kind),
            entry.size_bytes,
            display_path(&entry.relative_path),
            display_text(&entry.reason)
        )?;
    }
    Ok(())
}

fn write_json_report(output: &mut impl Write, report: &CargoHomeReport) -> Result<(), CliError> {
    let document = JsonCargoHomeReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonCargoHomeReport {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    input: JsonCargoHomeInput,
    totals: JsonCargoHomeTotals,
    entries: Vec<JsonCargoHomeEntry>,
    recommendations: Vec<String>,
    problems: Vec<JsonCargoHomeProblem>,
}

impl JsonCargoHomeReport {
    fn from_report(report: &CargoHomeReport) -> Self {
        Self {
            schema_version: report.schema_version,
            command: "cargo-home report",
            dry_run: true,
            input: JsonCargoHomeInput {
                root: path_string(&report.input.root),
                source: source_label(report.input.source),
            },
            totals: JsonCargoHomeTotals {
                entry_count: report.totals.entry_count,
                total_bytes: report.totals.total_bytes,
                cache_bytes: report.totals.cache_bytes,
                preserved_bytes: report.totals.preserved_bytes,
                skipped_count: report.totals.skipped_count,
                problem_count: report.totals.problem_count,
                known_cache_entry_count: report.totals.known_cache_entry_count,
            },
            entries: report
                .entries
                .iter()
                .map(JsonCargoHomeEntry::from_entry)
                .collect(),
            recommendations: report
                .recommendations
                .iter()
                .map(|recommendation| recommendation.message.clone())
                .collect(),
            problems: report
                .problems
                .iter()
                .map(|problem| JsonCargoHomeProblem {
                    path: path_string(&problem.path),
                    message: problem.message.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoHomeInput {
    root: String,
    source: &'static str,
}

#[derive(Serialize)]
struct JsonCargoHomeTotals {
    entry_count: usize,
    total_bytes: u64,
    cache_bytes: u64,
    preserved_bytes: u64,
    skipped_count: usize,
    problem_count: usize,
    known_cache_entry_count: usize,
}

#[derive(Serialize)]
struct JsonCargoHomeEntry {
    path: String,
    relative_path: String,
    class: &'static str,
    path_kind: &'static str,
    size_bytes: u64,
    preserved: bool,
    skipped: bool,
    reason: String,
}

impl JsonCargoHomeEntry {
    fn from_entry(entry: &CargoHomeEntry) -> Self {
        Self {
            path: path_string(&entry.path),
            relative_path: path_string(&entry.relative_path),
            class: class_label(entry.class),
            path_kind: path_kind_label(entry.path_kind),
            size_bytes: entry.size_bytes,
            preserved: entry.preserved,
            skipped: entry.skipped,
            reason: entry.reason.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoHomeProblem {
    path: String,
    message: String,
}

fn source_label(source: CargoHomeSource) -> &'static str {
    match source {
        CargoHomeSource::Explicit => "explicit",
        CargoHomeSource::CargoHomeEnv => "cargo_home_env",
        CargoHomeSource::HomeDefault => "home_default",
    }
}

fn class_label(class: CargoHomeClass) -> &'static str {
    match class {
        CargoHomeClass::RegistryIndex => "registry_index",
        CargoHomeClass::RegistryCache => "registry_cache",
        CargoHomeClass::RegistrySource => "registry_source",
        CargoHomeClass::GitDatabase => "git_database",
        CargoHomeClass::GitCheckouts => "git_checkouts",
        CargoHomeClass::Config => "config",
        CargoHomeClass::Credentials => "credentials",
        CargoHomeClass::InstalledBinaries => "installed_binaries",
        CargoHomeClass::InstallMetadata => "install_metadata",
        CargoHomeClass::UnknownUserAuthored => "unknown_user_authored",
    }
}

fn path_kind_label(kind: CargoHomePathKind) -> &'static str {
    match kind {
        CargoHomePathKind::File => "file",
        CargoHomePathKind::Directory => "directory",
        CargoHomePathKind::Symlink => "symlink",
        CargoHomePathKind::Other => "other",
    }
}

fn display_path(path: &Path) -> String {
    display_text(&path.display().to_string())
}

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

fn display_text(value: &str) -> String {
    value.escape_default().to_string()
}
