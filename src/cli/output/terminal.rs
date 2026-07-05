use std::io::Write;
use std::path::{Path, PathBuf};

use cargo_reclaim::{Plan, PlanEntry, PolicyKind};

use super::labels::{action_label, artifact_label, policy_label, skip_reason_label};
use crate::cli::{CliError, PlanMode};

pub(super) fn write_help(output: &mut impl Write) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim")?;
    writeln!(
        output,
        "Usage: cargo-reclaim [OPTIONS] [ROOT ...]\n       cargo-reclaim list [OPTIONS] [ROOT ...]\n       cargo-reclaim <scan|plan|apply|edit-plan|scheduler|cargo-config|cargo-home> [OPTIONS]"
    )?;
    writeln!(output)?;
    writeln!(output, "Commands:")?;
    writeln!(
        output,
        "  [ROOT ...]  Open the TTY cleanup assistant; use --all for bulk smart trim and --target with --delete-target for explicit whole-target deletion"
    )?;
    writeln!(output, "  scan    Build a read-only dry-run plan for roots")?;
    writeln!(output, "  plan    Build a read-only dry-run plan for roots")?;
    writeln!(output, "  apply   Validate or execute a persisted plan")?;
    writeln!(
        output,
        "  edit-plan  List, edit, or interactively select entries in a persisted plan"
    )?;
    writeln!(output, "  list    Read-only target inventory with sizes")?;
    writeln!(
        output,
        "  scheduler preview  Preview scheduler artifacts only"
    )?;
    writeln!(
        output,
        "  scheduler install --dry-run  Plan scheduler artifact installation"
    )?;
    writeln!(
        output,
        "  scheduler uninstall --dry-run  Plan scheduler artifact removal"
    )?;
    writeln!(
        output,
        "  cargo-home report  Report Cargo home caches and preserved paths"
    )?;
    writeln!(
        output,
        "  cargo-home plan  Build a dry-run Cargo home cleanup plan"
    )?;
    writeln!(
        output,
        "  cargo-home apply --plan <path> [--yes]  Validate or execute a persisted Cargo home cleanup plan"
    )?;
    writeln!(
        output,
        "  cargo-config recommend  Report read-only Cargo build output configuration recommendations"
    )?;
    writeln!(
        output,
        "  cargo-config preview  Preview read-only Cargo config write plan"
    )?;
    writeln!(
        output,
        "  cargo-config apply --preview <path> --yes  Apply a Cargo config preview"
    )?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --config <path>              Load scan/plan defaults from a TOML config file"
    )?;
    writeln!(
        output,
        "  --policy <kind>              observe, conservative, balanced, aggressive, custom"
    )?;
    writeln!(
        output,
        "  --whole-target <mode>        off, confirm, delete; delete requires aggressive policy"
    )?;
    writeln!(
        output,
        "  --ignore <path>              Report a path as ignored while scanning"
    )?;
    writeln!(
        output,
        "  --skip <path>                Do not scan a path or its descendants"
    )?;
    writeln!(
        output,
        "  --allow-name-only-targets    Include weak target/ matches as confirmation-only"
    )?;
    writeln!(
        output,
        "  --follow-symlinks            Follow symlinks during scan and inventory"
    )?;
    writeln!(
        output,
        "  --keep-days <days>           Preserve recently modified delete candidates for N days"
    )?;
    writeln!(
        output,
        "  --keep-size <size>           Preserve delete candidates at or below size"
    )?;
    writeln!(
        output,
        "  --keep-rustc-hash <u64>      Preserve fingerprint grouped intermediates for a rustc hash"
    )?;
    writeln!(
        output,
        "  --keep-installed-toolchains  Preserve fingerprint groups for installed rustup toolchains"
    )?;
    writeln!(
        output,
        "  --keep-toolchain <name>      Preserve fingerprint groups for a named rustup toolchain"
    )?;
    writeln!(
        output,
        "  --cross-filesystems          Allow recursive scan across filesystem boundaries"
    )?;
    writeln!(
        output,
        "  --keep-recent-writes <dur>   Preserve delete candidates modified within s, m, h, or d"
    )?;
    writeln!(
        output,
        "  --json                       Emit one dry-run JSON plan document"
    )?;
    Ok(())
}

pub(super) fn write_plan(
    output: &mut impl Write,
    plan: &Plan,
    policy: PolicyKind,
    mode: PlanMode,
) -> Result<(), CliError> {
    let mode_label = match mode {
        PlanMode::Scan => "scan",
        PlanMode::Plan => "plan",
    };

    writeln!(output, "cargo-reclaim {mode_label} dry-run")?;
    writeln!(output, "dry-run only; no files were deleted or modified")?;
    writeln!(
        output,
        "human-readable text; use --json for a stable structured document"
    )?;
    writeln!(output, "policy: {}", policy_label(policy))?;
    writeln!(output, "roots: {}", join_paths(&plan.input.roots))?;
    writeln!(output, "entries: {}", plan.totals.entry_count)?;
    writeln!(
        output,
        "delete candidates: {}",
        plan.totals.delete_candidate_count
    )?;
    writeln!(output, "preserved/unknown: {}", plan.totals.preserved_count)?;
    writeln!(output, "estimated bytes: {}", plan.totals.total_bytes)?;
    writeln!(
        output,
        "skipped scan paths: {}",
        plan.totals.skipped_path_count
    )?;

    if !plan.skipped_paths.is_empty() {
        writeln!(output)?;
        writeln!(output, "skipped scan path details:")?;
        for skip in &plan.skipped_paths {
            match &skip.message {
                Some(message) => writeln!(
                    output,
                    "  {}\t{}\t{}",
                    skip_reason_label(skip.reason),
                    display_path(&skip.path),
                    display_text(message)
                )?,
                None => writeln!(
                    output,
                    "  {}\t{}",
                    skip_reason_label(skip.reason),
                    display_path(&skip.path)
                )?,
            }
        }
    }

    if plan.entries.is_empty() {
        writeln!(
            output,
            "no reclaimable or reportable target artifacts found"
        )?;
        return Ok(());
    }

    writeln!(output)?;
    for entry in &plan.entries {
        write_entry(output, entry)?;
    }

    Ok(())
}

fn write_entry(output: &mut impl Write, entry: &PlanEntry) -> Result<(), CliError> {
    writeln!(
        output,
        "{}\t{}\t{}\t{}\t{}",
        action_label(&entry.action),
        artifact_label(entry.artifact_class),
        entry.snapshot.size_bytes,
        display_path(&entry.snapshot.path),
        display_text(&entry.policy_reason)
    )?;
    Ok(())
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| display_path(path))
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_path(path: &Path) -> String {
    display_text(&path.display().to_string())
}

fn display_text(value: &str) -> String {
    value.escape_default().to_string()
}
