use std::io::Write;

use cargo_reclaim::CargoConfigRecommendReport;
use cargo_reclaim::config::{CargoConfigApplyReport, CargoConfigPreviewReport};

use super::super::CliError;
use super::labels::{
    display_path, display_text, preview_operation_status_label, unsupported_reason_label,
};

pub(super) fn write_terminal_recommend_report(
    output: &mut impl Write,
    report: &CargoConfigRecommendReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim cargo-config recommend")?;
    writeln!(
        output,
        "read-only/dry-run; no Cargo config files were modified"
    )?;
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(output, "project: {}", display_path(&report.project))?;
    writeln!(output, "target dirs: {}", report.target_dirs.len())?;
    writeln!(output, "build dirs: {}", report.build_dirs.len())?;
    writeln!(output, "recommendations: {}", report.recommendations.len())?;
    writeln!(output, "unsupported: {}", report.unsupported.len())?;
    writeln!(output, "problems: {}", report.problems.len())?;

    if !report.target_dirs.is_empty() {
        writeln!(output)?;
        for dir in &report.target_dirs {
            writeln!(
                output,
                "target-dir\t{}\t{}",
                display_path(&dir.path),
                display_text(&dir.source)
            )?;
        }
    }

    if !report.build_dirs.is_empty() {
        writeln!(output)?;
        for dir in &report.build_dirs {
            writeln!(
                output,
                "build-dir\t{}\t{}",
                display_path(&dir.path),
                display_text(&dir.source)
            )?;
        }
    }

    if !report.recommendations.is_empty() {
        writeln!(output)?;
        for recommendation in &report.recommendations {
            writeln!(
                output,
                "recommendation: {}\t{}\t{}",
                display_text(&recommendation.key),
                display_text(recommendation.recommended.as_deref().unwrap_or("")),
                display_text(&recommendation.reason)
            )?;
        }
    }

    if !report.unsupported.is_empty() {
        writeln!(output)?;
        for unsupported in &report.unsupported {
            writeln!(
                output,
                "unsupported: {}\t{}",
                unsupported_reason_label(&unsupported.reason),
                display_text(&unsupported.source)
            )?;
        }
    }

    if !report.problems.is_empty() {
        writeln!(output)?;
        for problem in &report.problems {
            writeln!(
                output,
                "problem: {}\t{}",
                display_path(&problem.path),
                display_text(&problem.message)
            )?;
        }
    }

    Ok(())
}

pub(super) fn write_terminal_preview_report(
    output: &mut impl Write,
    report: &CargoConfigPreviewReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim cargo-config preview")?;
    writeln!(
        output,
        "read-only/dry-run; no Cargo config files were modified"
    )?;
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(output, "project: {}", display_path(&report.project))?;
    writeln!(
        output,
        "target config file: {}",
        display_path(&report.target_config_file)
    )?;
    writeln!(
        output,
        "target config exists: {}",
        report.target_config_snapshot.exists
    )?;
    if let Some(hash) = &report.target_config_snapshot.hash {
        writeln!(output, "target config hash: {}", display_text(hash))?;
    }
    if let Some(size_bytes) = report.target_config_snapshot.size_bytes {
        writeln!(output, "target config size bytes: {size_bytes}")?;
    }
    writeln!(
        output,
        "modified cargo config files: {}",
        report.modified_cargo_config_files
    )?;
    writeln!(output, "operations: {}", report.operations.len())?;
    writeln!(output, "unsupported: {}", report.unsupported.len())?;
    writeln!(output, "problems: {}", report.problems.len())?;

    if !report.operations.is_empty() {
        writeln!(output)?;
        for operation in &report.operations {
            writeln!(
                output,
                "operation: {}\t{}\t{}\t{}\t{}",
                display_text(&operation.key),
                display_text(operation.current.as_deref().unwrap_or("")),
                display_text(operation.recommended.as_deref().unwrap_or("")),
                preview_operation_status_label(operation.status),
                display_text(&operation.reason)
            )?;
        }
    }

    if !report.unsupported.is_empty() {
        writeln!(output)?;
        for unsupported in &report.unsupported {
            writeln!(
                output,
                "unsupported: {}\t{}",
                unsupported_reason_label(&unsupported.reason),
                display_text(&unsupported.source)
            )?;
        }
    }

    if !report.problems.is_empty() {
        writeln!(output)?;
        for problem in &report.problems {
            writeln!(
                output,
                "problem: {}\t{}",
                display_path(&problem.path),
                display_text(&problem.message)
            )?;
        }
    }

    Ok(())
}

pub(super) fn write_terminal_apply_report(
    output: &mut impl Write,
    report: &CargoConfigApplyReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim cargo-config apply")?;
    if report.modified_cargo_config_files {
        writeln!(output, "Cargo config files were modified")?;
    } else {
        writeln!(output, "No Cargo config files were modified")?;
    }
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(
        output,
        "preview path: {}",
        display_path(&report.preview_path)
    )?;
    writeln!(
        output,
        "target config file: {}",
        display_path(&report.target_config_file)
    )?;
    writeln!(output, "applied: {}", report.applied)?;
    writeln!(
        output,
        "modified cargo config files: {}",
        report.modified_cargo_config_files
    )?;
    writeln!(output, "operations: {}", report.operations.len())?;

    if !report.operations.is_empty() {
        writeln!(output)?;
        for operation in &report.operations {
            writeln!(
                output,
                "operation: {}\t{}\t{}\t{}\t{}",
                display_text(&operation.key),
                display_text(operation.current.as_deref().unwrap_or("")),
                display_text(operation.recommended.as_deref().unwrap_or("")),
                preview_operation_status_label(operation.status),
                display_text(&operation.reason)
            )?;
        }
    }

    Ok(())
}
