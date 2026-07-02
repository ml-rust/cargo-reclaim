use std::io::Write;

use cargo_reclaim::{CargoHomePlan, CargoHomeReport};

use super::super::CliError;
use super::labels::{
    action_label, class_label, display_path, display_text, path_kind_label, policy_label,
    source_label,
};

pub(super) fn write_terminal_report(
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

pub(super) fn write_terminal_plan(
    output: &mut impl Write,
    plan: &CargoHomePlan,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim cargo-home plan")?;
    writeln!(
        output,
        "dry-run planning only; no files were deleted or modified"
    )?;
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(output, "root: {}", display_path(&plan.input.root))?;
    writeln!(output, "source: {}", source_label(plan.input.source))?;
    writeln!(output, "policy: {}", policy_label(plan.policy))?;
    writeln!(output, "entries: {}", plan.totals.entry_count)?;
    writeln!(
        output,
        "delete candidates: {}",
        plan.totals.delete_candidate_count
    )?;
    writeln!(
        output,
        "delete candidate bytes: {}",
        plan.totals.delete_candidate_bytes
    )?;
    writeln!(output, "preserved entries: {}", plan.totals.preserved_count)?;
    writeln!(output, "preserved bytes: {}", plan.totals.preserved_bytes)?;
    writeln!(
        output,
        "skipped/problem entries: {}",
        plan.totals.skipped_count
    )?;
    writeln!(output, "problems: {}", plan.totals.problem_count)?;

    if !plan.recommendations.is_empty() {
        writeln!(output)?;
        for recommendation in &plan.recommendations {
            writeln!(
                output,
                "recommendation: {}",
                display_text(&recommendation.message)
            )?;
        }
    }

    if plan.entries.is_empty() {
        writeln!(output)?;
        writeln!(output, "no Cargo home plan entries found")?;
        return Ok(());
    }

    writeln!(output)?;
    for entry in &plan.entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}",
            action_label(entry.action),
            class_label(entry.class),
            path_kind_label(entry.path_kind),
            entry.size_bytes,
            display_path(&entry.relative_path),
            display_text(&entry.reason)
        )?;
    }
    Ok(())
}
