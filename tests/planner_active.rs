use std::error::Error;
use std::time::UNIX_EPOCH;

use cargo_reclaim::{
    ActiveObservation, ArtifactClass, CargoTool, ObservedCargoProcess, PathSnapshot, PlanAction,
    PlannerCandidate, PlannerOptions, PolicyKind, TargetContext, TargetEvidence,
    plan_candidate_with_active_observation,
};

fn candidate_with_context(
    path: &str,
    size_bytes: u64,
    artifact_class: ArtifactClass,
    evidence: TargetEvidence,
    target_context: TargetContext,
) -> Result<PlannerCandidate, Box<dyn Error>> {
    Ok(PlannerCandidate::new(
        PathSnapshot::new(path, size_bytes)?,
        artifact_class,
        evidence,
    )
    .with_target_context(target_context))
}

#[test]
fn not_attempted_process_view_preserves_delete_behavior() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            TargetContext::new("/work/sample/target").with_project_root("/work/sample"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::not_attempted(),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn permission_limited_process_view_skips_contextual_delete_candidate() -> Result<(), Box<dyn Error>>
{
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            TargetContext::new("/work/sample/target").with_project_root("/work/sample"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::permission_limited("process table denied"),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("permission-limited"));
    assert!(entry.policy_reason.contains("process table denied"));
    Ok(())
}

#[test]
fn permission_limited_process_view_does_not_mask_weak_evidence() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::weak_name_only("target")?,
            TargetContext::new("/work/sample/target").with_project_root("/work/sample"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::permission_limited("process table denied"),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    Ok(())
}

#[test]
fn failed_process_view_skips_contextual_delete_candidate() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            TargetContext::new("/work/sample/target").with_project_root("/work/sample"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::failed("process scan unavailable"),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("failed"));
    assert!(entry.policy_reason.contains("process scan unavailable"));
    Ok(())
}

#[test]
fn complete_process_view_skips_when_cargo_cwd_is_under_project_root() -> Result<(), Box<dyn Error>>
{
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            TargetContext::new("/work/sample/target").with_project_root("/work/sample"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::complete([
            ObservedCargoProcess::new(CargoTool::Cargo).with_cwd("/work/sample/crate")
        ]),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("cwd"));
    Ok(())
}

#[test]
fn complete_process_view_skips_when_referenced_path_overlaps_build_root()
-> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/sample/cache/builds/hash/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::configured_path("Cargo config build.target-dir")?,
            TargetContext::new("/work/sample/cache/target")
                .with_project_root("/work/sample")
                .with_build_root("/work/sample/cache/builds/hash"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::complete([ObservedCargoProcess::new(CargoTool::Rustc)
            .with_referenced_path("/work/sample/cache/builds/hash/debug/deps/unit.o")]),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("overlaps"));
    Ok(())
}

#[test]
fn complete_process_view_uses_component_prefixes_not_substrings() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context(
            "/work/app/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            TargetContext::new("/work/app/target").with_project_root("/work/app"),
        )?,
        PolicyKind::Balanced,
        &PlannerOptions::default(),
        &ActiveObservation::complete([ObservedCargoProcess::new(CargoTool::Cargo)
            .with_cwd("/work/application")
            .with_referenced_path("/work/app-target/debug/deps/unit.o")]),
        UNIX_EPOCH,
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}
