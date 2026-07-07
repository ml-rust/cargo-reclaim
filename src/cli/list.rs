use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{InventoryOptions, ScannerOptions, load_config_from_path};
use serde_json::json;

use super::target_report::{
    TargetsDiscovery, TargetsReport, build_targets_report, evidence_label, human_bytes,
    problem_json, skip_json, target_json,
};
use super::{
    CliError, OutputFormat, inline_config_path, inline_ignore_path, inline_skip_path, next_path,
};

#[derive(Debug)]
pub(in crate::cli) struct ListCommand {
    discovery: ListDiscoveryCommand,
}

#[derive(Debug)]
struct ListDiscoveryCommand {
    roots: Vec<PathBuf>,
    output_format: OutputFormat,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
}

impl ListDiscoveryCommand {
    fn report_discovery(&self) -> TargetsDiscovery {
        TargetsDiscovery::new(
            self.roots.clone(),
            self.scanner_options.clone(),
            self.inventory_options.clone(),
            self.config_path.clone(),
            self.config_version,
        )
    }
}

pub(in crate::cli) fn parse_list_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ListCommand, CliError> {
    Ok(ListCommand {
        discovery: parse_list_discovery(args)?,
    })
}

fn parse_list_discovery(
    args: impl IntoIterator<Item = OsString>,
) -> Result<ListDiscoveryCommand, CliError> {
    let mut roots = Vec::new();
    let mut output_format = OutputFormat::Terminal;
    let mut config_path = None;
    let mut scanner_options = ScannerOptions::default();
    let mut cli_follow_symlinks = false;
    let mut cli_allow_name_only_targets = false;
    let mut cli_cross_filesystems = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(ignore_path) = inline_ignore_path(&arg)? {
            scanner_options.ignored_paths.push(ignore_path);
            continue;
        }
        if let Some(skip_path) = inline_skip_path(&arg)? {
            scanner_options.skipped_paths.push(skip_path);
            continue;
        }
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            roots.push(PathBuf::from(arg));
            continue;
        };

        match arg_text {
            "-h" | "--help" => return Err(CliError::Help(list_usage())),
            "--" => {
                roots.extend(args.map(PathBuf::from));
                break;
            }
            "--config" => {
                config_path = Some(next_path(&mut args, "--config")?);
            }
            "--ignore" => {
                scanner_options
                    .ignored_paths
                    .push(next_path(&mut args, "--ignore")?);
            }
            "--skip" => {
                scanner_options
                    .skipped_paths
                    .push(next_path(&mut args, "--skip")?);
            }
            "--allow-name-only-targets" => {
                scanner_options.allow_name_only_targets = true;
                cli_allow_name_only_targets = true;
            }
            "--follow-symlinks" => {
                scanner_options.follow_symlinks = true;
                cli_follow_symlinks = true;
            }
            "--cross-filesystems" => {
                scanner_options.cross_filesystems = true;
                cli_cross_filesystems = true;
            }
            "--json" => output_format = OutputFormat::Json,
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown list option `{value}`")));
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }

    let config = config_path
        .as_ref()
        .map(load_config_from_path)
        .transpose()?;
    let config_version = config.as_ref().map(|config| config.version);

    if roots.is_empty() {
        if let Some(config_roots) = config
            .as_ref()
            .filter(|config| !config.roots.is_empty())
            .map(|config| config.roots.clone())
        {
            roots = config_roots;
        } else {
            roots.push(PathBuf::from("."));
        }
    }

    if let Some(config) = config {
        let mut ignored_paths = config.ignored_paths;
        ignored_paths.extend(scanner_options.ignored_paths);
        scanner_options.ignored_paths = ignored_paths;
        let mut skipped_paths = config.skipped_paths;
        skipped_paths.extend(scanner_options.skipped_paths);
        scanner_options.skipped_paths = skipped_paths;

        if !cli_follow_symlinks && let Some(follow_symlinks) = config.scanner.follow_symlinks {
            scanner_options.follow_symlinks = follow_symlinks;
        }
        if !cli_allow_name_only_targets
            && let Some(allow_name_only_targets) = config.scanner.allow_name_only_targets
        {
            scanner_options.allow_name_only_targets = allow_name_only_targets;
        }
        if !cli_cross_filesystems && let Some(cross_filesystems) = config.scanner.cross_filesystems
        {
            scanner_options.cross_filesystems = cross_filesystems;
        }
    }

    let inventory_options = InventoryOptions {
        follow_symlinks: scanner_options.follow_symlinks,
        skipped_paths: scanner_options.skipped_paths.clone(),
        deep_target_scan: false,
        deep_directory_measurement: false,
    };

    Ok(ListDiscoveryCommand {
        roots,
        output_format,
        scanner_options,
        inventory_options,
        config_path,
        config_version,
    })
}

pub(in crate::cli) fn run_list_command(
    command: &ListCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let report = build_targets_report(&command.discovery.report_discovery())?;
    match command.discovery.output_format {
        OutputFormat::Terminal => write_list_terminal(output, &report)?,
        OutputFormat::Json => write_list_json(output, &report)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn write_list_terminal(output: &mut impl Write, report: &TargetsReport) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim list")?;
    writeln!(output, "read-only; no files were deleted or modified")?;
    writeln!(
        output,
        "roots: {}",
        report
            .roots
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    if let Some(config_path) = &report.config_path {
        writeln!(
            output,
            "config: {}{}",
            config_path.display(),
            report
                .config_version
                .map(|version| format!(" (version {version})"))
                .unwrap_or_default()
        )?;
    }
    writeln!(
        output,
        "targets: {} ({})",
        report.targets.len(),
        human_bytes(report.total_size_bytes)
    )?;
    if let Some(diagnosis) = report.empty_diagnosis() {
        writeln!(output, "note: {diagnosis}")?;
    }
    if !report.problems.is_empty() {
        writeln!(output, "problems: {}", report.problems.len())?;
    }
    if !report.skipped_paths.is_empty() {
        writeln!(output, "skipped scan paths: {}", report.skipped_paths.len())?;
    }

    if !report.targets.is_empty() {
        writeln!(output)?;
        writeln!(output, "index\tsize\tbytes\tevidence\tpath")?;
        for (index, target) in report.targets.iter().enumerate() {
            writeln!(
                output,
                "{}\t{}\t{}\t{}\t{}",
                index + 1,
                human_bytes(target.size_bytes),
                target.size_bytes,
                evidence_label(&target.evidence),
                target.path.display()
            )?;
        }
    }

    if !report.problems.is_empty() {
        writeln!(output)?;
        writeln!(output, "problems")?;
        for problem in &report.problems {
            writeln!(output, "{}\t{}", problem.message, problem.path.display())?;
        }
    }

    Ok(())
}

fn write_list_json(output: &mut impl Write, report: &TargetsReport) -> Result<(), CliError> {
    serde_json::to_writer_pretty(
        &mut *output,
        &json!({
            "command": "list",
            "schema_version": 1,
            "roots": report.roots,
            "config_path": report.config_path,
            "config_version": report.config_version,
            "totals": {
                "target_count": report.targets.len(),
                "total_size_bytes": report.total_size_bytes,
                "project_count": report.project_count,
                "skipped_path_count": report.skipped_paths.len(),
                "problem_count": report.problems.len(),
            },
            "note": report.empty_diagnosis(),
            "targets": report.targets.iter().map(target_json).collect::<Vec<_>>(),
            "skipped_paths": report.skipped_paths.iter().map(skip_json).collect::<Vec<_>>(),
            "problems": report.problems.iter().map(problem_json).collect::<Vec<_>>(),
        }),
    )?;
    writeln!(output)?;
    Ok(())
}

fn list_usage() -> String {
    "usage: cargo-reclaim list [OPTIONS] [ROOT ...]".to_string()
}
