use std::time::SystemTime;

use crate::ReclaimResult;
use crate::model::{ArtifactClass, PlanAction, PlanEntry};
use crate::policy::PolicyKind;

use super::{PlannerCandidate, PlannerOptions};

pub(super) fn plan_candidate_for_policy(
    policy: PolicyKind,
    candidate: PlannerCandidate,
    options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<PlanEntry> {
    let PlannerCandidate {
        snapshot,
        artifact_class,
        evidence,
    } = candidate;

    if artifact_class == ArtifactClass::Unknown {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::Unknown,
            "artifact class is unknown and needs classification before deletion",
            false,
        );
    }

    if policy == PolicyKind::Observe {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "observe policy preserves candidates without deleting",
        );
    }

    if PolicyKind::is_default_protected_output(artifact_class) {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "policy protects durable user-facing outputs by default",
        );
    }

    if !is_removable_for_policy(policy, artifact_class) {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "artifact class is not removable for the selected policy",
        );
    }

    if evidence.is_weak_name_only() {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::RequiresConfirmation,
            "name-only evidence is below the default delete confidence threshold",
            true,
        );
    }

    if is_recently_modified(&snapshot.modified, options, now) {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::SkipActive,
            "recent target writes are inside the active-project keep window",
            false,
        );
    }

    if policy.allows_delete(&PlanAction::Delete, artifact_class, &evidence) {
        return PlanEntry::delete(
            snapshot,
            artifact_class,
            evidence,
            "policy permits deletion for this classified removable artifact",
            false,
        );
    }

    PlanEntry::preserved(
        snapshot,
        artifact_class,
        evidence,
        "artifact class is not removable for the selected policy",
    )
}

fn is_recently_modified(
    modified: &Option<SystemTime>,
    options: &PlannerOptions,
    now: SystemTime,
) -> bool {
    let Some(keep_window) = options.recent_write_keep_window else {
        return false;
    };
    let Some(modified) = *modified else {
        return false;
    };

    now.duration_since(modified)
        .map(|age| age <= keep_window)
        .unwrap_or(true)
}

fn is_removable_for_policy(policy: PolicyKind, artifact_class: ArtifactClass) -> bool {
    match policy {
        PolicyKind::Observe => false,
        PolicyKind::Conservative => PolicyKind::is_conservative_removable_class(artifact_class),
        PolicyKind::Balanced | PolicyKind::Aggressive | PolicyKind::Custom => {
            PolicyKind::is_default_removable_class(artifact_class)
        }
    }
}
