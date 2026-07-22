use std::error::Error;
use std::time::{Duration, UNIX_EPOCH};

use cargo_reclaim::{
    ActiveObservation, ArtifactClass, CargoTool, ObservedCargoProcess, PathKind, PathSnapshot,
    PlanAction, PlannerCandidate, PlannerOptions, PolicyKind, TargetContext, TargetEvidence,
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

fn candidate_with_context_modified(
    path: &str,
    size_bytes: u64,
    artifact_class: ArtifactClass,
    evidence: TargetEvidence,
    target_context: TargetContext,
    modified_secs: u64,
) -> Result<PlannerCandidate, Box<dyn Error>> {
    Ok(PlannerCandidate::new(
        PathSnapshot::with_details(
            path,
            size_bytes,
            PathKind::Directory,
            Some(UNIX_EPOCH + Duration::from_secs(modified_secs)),
        )?,
        artifact_class,
        evidence,
    )
    .with_target_context(target_context))
}

fn active_cargo() -> ActiveObservation {
    ActiveObservation::complete([
        ObservedCargoProcess::new(CargoTool::Cargo).with_cwd("/work/sample/crate")
    ])
}

fn keep_window_options() -> PlannerOptions {
    PlannerOptions {
        recent_write_keep_window: Some(Duration::from_secs(60 * 60)),
        ..PlannerOptions::default()
    }
}

fn sample_context() -> TargetContext {
    TargetContext::new("/work/sample/target").with_project_root("/work/sample")
}

#[test]
fn stale_deps_is_protected_during_active_build() -> Result<(), Box<dyn Error>> {
    // Even a "superseded" hash variant can be a live feature-variant the running build
    // links against, so the whole target is protected while a build is active.
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/deps/sample-0123456789abcdef",
            100,
            ArtifactClass::StaleDeps,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Balanced,
        &keep_window_options(),
        &active_cargo(),
        UNIX_EPOCH + Duration::from_secs(2 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("cwd"));
    Ok(())
}

#[test]
fn deps_output_is_protected_during_active_build() -> Result<(), Box<dyn Error>> {
    // Regression: a current dependency output must never be deleted mid-build, even
    // when it is old — cargo may still be linking against it.
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/deps/libsample-0123456789abcdef.rmeta",
            100,
            ArtifactClass::DepsOutput,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Balanced,
        &keep_window_options(),
        &active_cargo(),
        UNIX_EPOCH + Duration::from_secs(10 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("cwd"));
    Ok(())
}

#[test]
fn incremental_is_protected_during_active_build() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Balanced,
        &keep_window_options(),
        &active_cargo(),
        UNIX_EPOCH + Duration::from_secs(10 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    Ok(())
}

#[test]
fn sweep_does_not_delete_final_binary_during_active_build() -> Result<(), Box<dyn Error>> {
    let options = PlannerOptions {
        recent_write_keep_window: Some(Duration::from_secs(60 * 60)),
        sweep_older_than: Some(Duration::from_secs(24 * 60 * 60)),
        ..PlannerOptions::default()
    };
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/app",
            100,
            ArtifactClass::FinalExecutable,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Sweep,
        &options,
        &active_cargo(),
        // 100h old — well past the sweep threshold, but a build is active.
        UNIX_EPOCH + Duration::from_secs(100 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    Ok(())
}

#[test]
fn interrupt_active_build_reclaims_during_active_build() -> Result<(), Box<dyn Error>> {
    // A disruptive trigger opts out of active-build protection: it deletes even
    // while a build runs (the build then fails when its files vanish — by design).
    let options = PlannerOptions {
        recent_write_keep_window: Some(Duration::from_secs(60 * 60)),
        interrupt_active_build: true,
        ..PlannerOptions::default()
    };
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/deps/libsample-0123456789abcdef.rmeta",
            100,
            ArtifactClass::DepsOutput,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Balanced,
        &options,
        &active_cargo(),
        UNIX_EPOCH + Duration::from_secs(10 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn cold_deps_output_is_reclaimable_between_builds() -> Result<(), Box<dyn Error>> {
    // With no build active, an old dependency output is reclaimable; cargo re-plans
    // and rebuilds it on the next build if it is still needed.
    let entry = plan_candidate_with_active_observation(
        candidate_with_context_modified(
            "/work/sample/target/debug/deps/libsample-0123456789abcdef.rmeta",
            100,
            ArtifactClass::DepsOutput,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            sample_context(),
            0,
        )?,
        PolicyKind::Balanced,
        &keep_window_options(),
        &ActiveObservation::not_attempted(),
        UNIX_EPOCH + Duration::from_secs(10 * 60 * 60),
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
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
