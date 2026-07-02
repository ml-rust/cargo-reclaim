use std::io::Write;

use cargo_reclaim::CargoConfigRecommendReport;

use super::super::CliError;
use super::labels::{display_path, display_text, unsupported_reason_label};

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
