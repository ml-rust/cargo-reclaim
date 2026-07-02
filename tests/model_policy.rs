use std::error::Error;
use std::process::Command;

use cargo_reclaim::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathSnapshot, Plan, PlanAction, PlanEntry, PlanInput,
    PolicyKind, TargetEvidence,
};

#[test]
fn plan_uses_explicit_schema_version() -> Result<(), Box<dyn Error>> {
    let input = PlanInput::from_root(".")?;
    let plan = Plan::new(input, Vec::new());

    assert_eq!(plan.schema_version, PLAN_SCHEMA_VERSION);
    assert!(plan.is_schema_current());
    Ok(())
}

#[test]
fn unknown_artifacts_are_preserved_by_default() -> Result<(), Box<dyn Error>> {
    let action = PlanAction::Delete;
    let evidence = TargetEvidence::strong_marker("CACHEDIR.TAG")?;

    assert!(!PolicyKind::Balanced.allows_delete(&action, ArtifactClass::Unknown, &evidence));
    Ok(())
}

#[test]
fn user_facing_outputs_are_protected_by_default() {
    let protected = PolicyKind::default_protected_outputs();

    for artifact_class in [
        ArtifactClass::Docs,
        ArtifactClass::Package,
        ArtifactClass::Timings,
        ArtifactClass::FinalExecutable,
        ArtifactClass::FinalLibrary,
        ArtifactClass::FinalRlib,
        ArtifactClass::FinalWasm,
    ] {
        assert!(protected.contains(&artifact_class));
        assert!(!PolicyKind::is_default_removable_class(artifact_class));
    }
}

#[test]
fn observe_policy_never_allows_delete() -> Result<(), Box<dyn Error>> {
    let action = PlanAction::Delete;
    let evidence = TargetEvidence::strong_marker("CACHEDIR.TAG")?;

    assert!(!PolicyKind::Observe.allows_delete(&action, ArtifactClass::Incremental, &evidence));
    Ok(())
}

#[test]
fn conservative_policy_is_narrower_than_balanced() -> Result<(), Box<dyn Error>> {
    let action = PlanAction::Delete;
    let evidence = TargetEvidence::strong_marker("CACHEDIR.TAG")?;

    assert!(PolicyKind::Conservative.allows_delete(&action, ArtifactClass::Incremental, &evidence));
    assert!(!PolicyKind::Conservative.allows_delete(
        &action,
        ArtifactClass::BuildScripts,
        &evidence
    ));
    assert!(PolicyKind::Balanced.allows_delete(&action, ArtifactClass::BuildScripts, &evidence));
    Ok(())
}

#[test]
fn name_only_evidence_is_below_default_delete_confidence() -> Result<(), Box<dyn Error>> {
    let action = PlanAction::Delete;
    let evidence = TargetEvidence::weak_name_only("target")?;

    assert!(!evidence.meets_default_delete_confidence());
    assert!(!PolicyKind::Balanced.allows_delete(&action, ArtifactClass::Incremental, &evidence));
    Ok(())
}

#[test]
fn target_evidence_uses_product_confidence_classes() -> Result<(), Box<dyn Error>> {
    assert!(TargetEvidence::strong_marker("CACHEDIR.TAG")?.meets_default_delete_confidence());
    assert!(TargetEvidence::configured_path("CARGO_TARGET_DIR")?.meets_default_delete_confidence());
    assert!(TargetEvidence::project_context("Cargo.toml")?.meets_default_delete_confidence());
    assert!(!TargetEvidence::weak_name_only("target")?.meets_default_delete_confidence());
    Ok(())
}

#[test]
fn plan_totals_are_derived_from_entries() -> Result<(), Box<dyn Error>> {
    let input = PlanInput::new([".", "reference"])?;
    let preserved = PlanEntry::preserved(
        PathSnapshot::new("target/doc", 10)?,
        ArtifactClass::Docs,
        TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        "protected output",
    )?;
    let candidate = PlanEntry::delete(
        PathSnapshot::new("target/debug/incremental", 25)?,
        ArtifactClass::Incremental,
        TargetEvidence::project_context("Cargo.toml")?,
        "derived intermediate output",
        false,
    )?;

    let plan = Plan::new(input, vec![preserved, candidate]);

    assert_eq!(plan.totals.entry_count, 2);
    assert_eq!(plan.totals.total_bytes, 35);
    assert_eq!(plan.totals.preserved_count, 1);
    assert_eq!(plan.totals.delete_candidate_count, 1);
    Ok(())
}

#[test]
fn plan_input_accepts_multiple_roots() -> Result<(), Box<dyn Error>> {
    let input = PlanInput::new([".", "reference"])?;

    assert_eq!(input.roots.len(), 2);
    Ok(())
}

#[test]
fn cli_reports_planning_only_status() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim")).output()?;

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("planning only"));
    assert!(stdout.contains("no files are deleted or modified"));
    Ok(())
}
