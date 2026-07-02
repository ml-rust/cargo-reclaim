use std::error::Error;

use cargo_reclaim::{
    ArtifactClass, PathSnapshot, PlanAction, PlanInput, PlannerCandidate, PolicyKind,
    TargetEvidence, build_plan, plan_candidate,
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
