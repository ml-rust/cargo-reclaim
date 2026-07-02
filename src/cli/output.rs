use std::io::Write;
use std::path::{Path, PathBuf};

use cargo_reclaim::{ArtifactClass, Plan, PlanAction, PlanEntry, PolicyKind};

use super::{CliError, PlanMode};

pub(super) fn write_help(output: &mut impl Write) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim")?;
    writeln!(
        output,
        "Usage: cargo-reclaim <scan|plan> [ROOT ...] [OPTIONS]"
    )?;
    writeln!(output)?;
    writeln!(output, "Commands:")?;
    writeln!(output, "  scan    Build a read-only dry-run plan for roots")?;
    writeln!(output, "  plan    Build a read-only dry-run plan for roots")?;
    writeln!(output)?;
    writeln!(output, "Options:")?;
    writeln!(
        output,
        "  --policy <kind>              observe, conservative, balanced, aggressive, custom"
    )?;
    writeln!(
        output,
        "  --ignore <path>              Skip a path while scanning"
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
        "  --cross-filesystems          Allow recursive scan across filesystem boundaries"
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
        "human-readable text; use JSON only after a JSON output mode is added"
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

fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

fn action_label(action: &PlanAction) -> &'static str {
    match action {
        PlanAction::Delete => "delete",
        PlanAction::Preserve => "preserve",
        PlanAction::SkipActive => "skip_active",
        PlanAction::SkipLocked => "skip_locked",
        PlanAction::Unknown => "unknown",
        PlanAction::RequiresConfirmation => "requires_confirmation",
    }
}

fn artifact_label(artifact_class: ArtifactClass) -> &'static str {
    match artifact_class {
        ArtifactClass::Incremental => "incremental",
        ArtifactClass::Deps => "deps",
        ArtifactClass::BuildScripts => "build_scripts",
        ArtifactClass::Fingerprint => "fingerprint",
        ArtifactClass::Docs => "docs",
        ArtifactClass::Package => "package",
        ArtifactClass::Timings => "timings",
        ArtifactClass::Tmp => "tmp",
        ArtifactClass::DepInfo => "dep_info",
        ArtifactClass::ObjectMetadata => "object_metadata",
        ArtifactClass::FinalExecutable => "final_executable",
        ArtifactClass::FinalLibrary => "final_library",
        ArtifactClass::FinalRlib => "final_rlib",
        ArtifactClass::FinalWasm => "final_wasm",
        ArtifactClass::Unknown => "unknown",
    }
}

fn display_path(path: &Path) -> String {
    display_text(&path.display().to_string())
}

fn display_text(value: &str) -> String {
    value.escape_default().to_string()
}
