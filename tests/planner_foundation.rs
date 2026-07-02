use std::error::Error;
use std::time::{Duration, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, PathKind, PathSnapshot, PlanAction, PlanInput, PlannerCandidate, PlannerOptions,
    PolicyKind, TargetEvidence, WholeTargetMode, build_plan, plan_candidate,
    plan_candidate_with_active_observation, plan_candidate_with_options,
};

fn candidate(
    path: &str,
    size_bytes: u64,
    artifact_class: ArtifactClass,
    evidence: TargetEvidence,
) -> Result<PlannerCandidate, Box<dyn Error>> {
    Ok(PlannerCandidate::new(
        PathSnapshot::new(path, size_bytes)?,
        artifact_class,
        evidence,
    ))
}

fn candidate_with_modified(
    path: &str,
    size_bytes: u64,
    artifact_class: ArtifactClass,
    evidence: TargetEvidence,
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
    ))
}

#[test]
fn strong_incremental_balanced_policy_yields_delete() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn recent_write_keep_window_skips_active_delete_candidate() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate_with_modified(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            90,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(!entry.requires_confirmation);
    assert!(entry.policy_reason.contains("keep window"));
    Ok(())
}

#[test]
fn recent_write_keep_window_does_not_change_missing_mtime_candidate() -> Result<(), Box<dyn Error>>
{
    let entry = plan_candidate_with_options(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn keep_size_preserves_small_delete_candidate() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            keep_size_bytes: Some(100),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::Preserve);
    assert!(entry.policy_reason.contains("keep-size"));
    Ok(())
}

#[test]
fn recent_write_keep_window_takes_precedence_over_keep_size() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate_with_modified(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            90,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            keep_size_bytes: Some(100),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(entry.policy_reason.contains("keep window"));
    Ok(())
}

#[test]
fn keep_size_does_not_mask_weak_evidence_confirmation() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::weak_name_only("target")?,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            keep_size_bytes: Some(100),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    Ok(())
}

#[test]
fn recent_write_keep_window_does_not_mask_non_removable_class() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate_with_modified(
            "target/debug/deps",
            100,
            ArtifactClass::Deps,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            90,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::Preserve);
    Ok(())
}

#[test]
fn recent_write_keep_window_does_not_mask_weak_evidence_confirmation() -> Result<(), Box<dyn Error>>
{
    let entry = plan_candidate_with_options(
        candidate_with_modified(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::weak_name_only("target")?,
            90,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn default_planner_options_do_not_skip_recent_candidates() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate(
        candidate_with_modified(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            100,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn whole_target_confirm_mode_requires_confirmation() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate(
            "target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::project_context("Cargo.toml")?,
        )?,
        PolicyKind::Aggressive,
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::Confirm,
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_delete_confirmed_requires_aggressive_policy() -> Result<(), Box<dyn Error>> {
    let balanced = plan_candidate_with_options(
        candidate(
            "target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::project_context("Cargo.toml")?,
        )?,
        PolicyKind::Balanced,
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;
    let aggressive = plan_candidate_with_options(
        candidate(
            "target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::project_context("Cargo.toml")?,
        )?,
        PolicyKind::Aggressive,
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(balanced.action, PlanAction::RequiresConfirmation);
    assert!(balanced.requires_confirmation);
    assert_eq!(aggressive.action, PlanAction::Delete);
    assert!(!aggressive.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_name_only_evidence_never_direct_deletes() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate(
            "target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::weak_name_only("target")?,
        )?,
        PolicyKind::Aggressive,
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_delete_confirmed_respects_recent_keep_window() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_options(
        candidate_with_modified(
            "target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::project_context("Cargo.toml")?,
            90,
        )?,
        PolicyKind::Aggressive,
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(20)),
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_delete_confirmed_respects_active_observation() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate_with_active_observation(
        candidate(
            "/workspace/project/target",
            100,
            ArtifactClass::WholeTarget,
            TargetEvidence::project_context("Cargo.toml")?,
        )?
        .with_target_context(
            cargo_reclaim::TargetContext::new("/workspace/project/target")
                .with_project_root("/workspace/project"),
        ),
        PolicyKind::Aggressive,
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        &cargo_reclaim::ActiveObservation::complete([cargo_reclaim::ObservedCargoProcess::new(
            cargo_reclaim::CargoTool::Cargo,
        )
        .with_cwd("/workspace/project/src")]),
        UNIX_EPOCH + Duration::from_secs(100),
    )?;

    assert_eq!(entry.action, PlanAction::SkipActive);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn observe_policy_preserves_strong_removable_candidates() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Observe,
    )?;

    assert_eq!(entry.action, PlanAction::Preserve);
    assert!(!entry.action.is_delete());
    Ok(())
}

#[test]
fn protected_outputs_preserve_under_delete_capable_policies() -> Result<(), Box<dyn Error>> {
    for policy in [
        PolicyKind::Balanced,
        PolicyKind::Aggressive,
        PolicyKind::Custom,
    ] {
        for artifact_class in [
            ArtifactClass::Docs,
            ArtifactClass::Package,
            ArtifactClass::Timings,
            ArtifactClass::FinalExecutable,
            ArtifactClass::FinalLibrary,
            ArtifactClass::FinalRlib,
            ArtifactClass::FinalWasm,
        ] {
            let entry = plan_candidate(
                candidate(
                    "target/protected",
                    10,
                    artifact_class,
                    TargetEvidence::strong_marker("CACHEDIR.TAG")?,
                )?,
                policy,
            )?;

            assert_eq!(entry.action, PlanAction::Preserve);
            assert!(!entry.requires_confirmation);
        }
    }

    Ok(())
}

#[test]
fn unknown_class_yields_unknown_action() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate(
        candidate(
            "target/unclassified",
            10,
            ArtifactClass::Unknown,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(entry.action, PlanAction::Unknown);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn unknown_class_stays_unknown_under_observe_policy() -> Result<(), Box<dyn Error>> {
    let entry = plan_candidate(
        candidate(
            "target/unclassified",
            10,
            ArtifactClass::Unknown,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Observe,
    )?;

    assert_eq!(entry.action, PlanAction::Unknown);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn weak_name_only_evidence_on_removable_class_requires_confirmation() -> Result<(), Box<dyn Error>>
{
    let entry = plan_candidate(
        candidate(
            "target/debug/incremental",
            100,
            ArtifactClass::Incremental,
            TargetEvidence::weak_name_only("incremental")?,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn conservative_policy_is_narrower_than_balanced_planner_output() -> Result<(), Box<dyn Error>> {
    let conservative = plan_candidate(
        candidate(
            "target/debug/build/example-123",
            100,
            ArtifactClass::BuildScripts,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Conservative,
    )?;
    let balanced = plan_candidate(
        candidate(
            "target/debug/build/example-123",
            100,
            ArtifactClass::BuildScripts,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(conservative.action, PlanAction::Preserve);
    assert_eq!(balanced.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn hash_grouped_intermediate_is_not_conservative_removable() -> Result<(), Box<dyn Error>> {
    let conservative = plan_candidate(
        candidate(
            "target/debug/sample-0123456789abcdef.json",
            100,
            ArtifactClass::FingerprintGroupIntermediate,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Conservative,
    )?;
    let balanced = plan_candidate(
        candidate(
            "target/debug/sample-0123456789abcdef.json",
            100,
            ArtifactClass::FingerprintGroupIntermediate,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        )?,
        PolicyKind::Balanced,
    )?;

    assert_eq!(conservative.action, PlanAction::Preserve);
    assert_eq!(balanced.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn build_plan_preserves_candidate_order_and_derives_totals() -> Result<(), Box<dyn Error>> {
    let input = PlanInput::from_root(".")?;
    let plan = build_plan(
        input,
        PolicyKind::Balanced,
        [
            candidate(
                "target/doc",
                10,
                ArtifactClass::Docs,
                TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            )?,
            candidate(
                "target/debug/incremental",
                25,
                ArtifactClass::Incremental,
                TargetEvidence::project_context("Cargo.toml")?,
            )?,
            candidate(
                "target/unclassified",
                5,
                ArtifactClass::Unknown,
                TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            )?,
        ],
    )?;

    assert_eq!(plan.entries.len(), 3);
    assert_eq!(
        plan.entries[0].snapshot.path,
        PathSnapshot::new("target/doc", 0)?.path
    );
    assert_eq!(
        plan.entries[1].snapshot.path,
        PathSnapshot::new("target/debug/incremental", 0)?.path
    );
    assert_eq!(
        plan.entries[2].snapshot.path,
        PathSnapshot::new("target/unclassified", 0)?.path
    );
    assert_eq!(plan.totals.entry_count, 3);
    assert_eq!(plan.totals.total_bytes, 40);
    assert_eq!(plan.totals.preserved_count, 2);
    assert_eq!(plan.totals.delete_candidate_count, 1);
    Ok(())
}

#[test]
fn planner_entries_have_non_empty_policy_reasons() -> Result<(), Box<dyn Error>> {
    for (artifact_class, evidence, policy) in [
        (
            ArtifactClass::WholeTarget,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Balanced,
        ),
        (
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Balanced,
        ),
        (
            ArtifactClass::Incremental,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Observe,
        ),
        (
            ArtifactClass::Docs,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Balanced,
        ),
        (
            ArtifactClass::Unknown,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Balanced,
        ),
        (
            ArtifactClass::Incremental,
            TargetEvidence::weak_name_only("incremental")?,
            PolicyKind::Balanced,
        ),
        (
            ArtifactClass::Deps,
            TargetEvidence::strong_marker("CACHEDIR.TAG")?,
            PolicyKind::Balanced,
        ),
    ] {
        let entry = plan_candidate(
            candidate("target/example", 1, artifact_class, evidence)?,
            policy,
        )?;

        assert!(!entry.policy_reason.trim().is_empty());
    }

    Ok(())
}
