use std::time::SystemTime;

use crate::ReclaimResult;
use crate::model::{ArtifactClass, PathSnapshot, PlanAction, PlanEntry, TargetEvidence};
use crate::policy::PolicyKind;

use super::{
    ActiveObservation, PlannerCandidate, PlannerOptions, ProcessView, TargetContext,
    WholeTargetMode,
};

pub(super) fn plan_candidate_for_policy(
    policy: PolicyKind,
    candidate: PlannerCandidate,
    options: &PlannerOptions,
    active_observation: &ActiveObservation,
    now: SystemTime,
) -> ReclaimResult<PlanEntry> {
    let PlannerCandidate {
        snapshot,
        artifact_class,
        evidence,
        target_context,
    } = candidate;

    if artifact_class == ArtifactClass::WholeTarget {
        return plan_whole_target_candidate(
            policy,
            snapshot,
            evidence,
            target_context,
            options,
            active_observation,
            now,
        );
    }

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

    if artifact_class == ArtifactClass::DepsOutput && options.recent_write_keep_window.is_none() {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "deps outputs require a recent-write keep window before automatic deletion",
        );
    }

    if !matches!(
        artifact_class,
        ArtifactClass::StaleDeps | ArtifactClass::StaleIncremental
    ) && is_recently_modified(&snapshot.modified, options, now)
    {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::SkipActive,
            "recent target writes are inside the active-project keep window",
            false,
        );
    }

    if is_under_keep_size(&snapshot, options) {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "artifact size is inside the configured keep-size threshold",
        );
    }

    if let Some(reason) = active_skip_reason(target_context.as_ref(), active_observation) {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::SkipActive,
            reason,
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

fn plan_whole_target_candidate(
    policy: PolicyKind,
    snapshot: PathSnapshot,
    evidence: TargetEvidence,
    target_context: Option<TargetContext>,
    options: &PlannerOptions,
    active_observation: &ActiveObservation,
    now: SystemTime,
) -> ReclaimResult<PlanEntry> {
    match options.whole_target_mode {
        WholeTargetMode::Off => PlanEntry::preserved(
            snapshot,
            ArtifactClass::WholeTarget,
            evidence,
            "whole-target planning is disabled",
        ),
        WholeTargetMode::Confirm => PlanEntry::new(
            snapshot,
            ArtifactClass::WholeTarget,
            evidence,
            PlanAction::RequiresConfirmation,
            "whole-target deletion requires explicit confirmation",
            true,
        ),
        WholeTargetMode::DeleteConfirmed => {
            if evidence.is_weak_name_only() {
                return PlanEntry::new(
                    snapshot,
                    ArtifactClass::WholeTarget,
                    evidence,
                    PlanAction::RequiresConfirmation,
                    "name-only evidence is below the whole-target delete confidence threshold",
                    true,
                );
            }

            if is_recently_modified(&snapshot.modified, options, now) {
                return PlanEntry::new(
                    snapshot,
                    ArtifactClass::WholeTarget,
                    evidence,
                    PlanAction::SkipActive,
                    "recent target writes are inside the active-project keep window",
                    false,
                );
            }

            if is_under_keep_size(&snapshot, options) {
                return PlanEntry::preserved(
                    snapshot,
                    ArtifactClass::WholeTarget,
                    evidence,
                    "artifact size is inside the configured keep-size threshold",
                );
            }

            if let Some(reason) = active_skip_reason(target_context.as_ref(), active_observation) {
                return PlanEntry::new(
                    snapshot,
                    ArtifactClass::WholeTarget,
                    evidence,
                    PlanAction::SkipActive,
                    reason,
                    false,
                );
            }

            if policy == PolicyKind::Aggressive {
                return PlanEntry::delete(
                    snapshot,
                    ArtifactClass::WholeTarget,
                    evidence,
                    "aggressive policy permits confirmed whole-target deletion",
                    false,
                );
            }

            PlanEntry::new(
                snapshot,
                ArtifactClass::WholeTarget,
                evidence,
                PlanAction::RequiresConfirmation,
                "whole-target deletion requires aggressive policy after confirmation",
                true,
            )
        }
    }
}

fn active_skip_reason(
    target_context: Option<&TargetContext>,
    active_observation: &ActiveObservation,
) -> Option<String> {
    let target_context = target_context?;

    match &active_observation.process_view {
        ProcessView::NotAttempted => None,
        ProcessView::PermissionLimited { reason } => Some(format!(
            "process inspection was permission-limited for active project detection: {}",
            non_empty_observation_reason(reason)
        )),
        ProcessView::Failed { reason } => Some(format!(
            "process inspection failed for active project detection: {}",
            non_empty_observation_reason(reason)
        )),
        ProcessView::Complete { processes } => processes
            .iter()
            .find_map(|process| target_context.active_match(process))
            .map(|active_match| match active_match {
                super::active::ActiveMatch::CwdUnderProject { cwd, project_root } => format!(
                    "observed Cargo or rustc process cwd {} is under project root {}",
                    cwd.display(),
                    project_root.display()
                ),
                super::active::ActiveMatch::ReferencedPathOverlapsRoot {
                    referenced_path,
                    root,
                } => format!(
                    "observed Cargo or rustc process path {} overlaps build root {}",
                    referenced_path.display(),
                    root.display()
                ),
            }),
    }
}

fn non_empty_observation_reason(reason: &str) -> &str {
    let reason = reason.trim();
    if reason.is_empty() {
        "reason unavailable"
    } else {
        reason
    }
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

fn is_under_keep_size(snapshot: &PathSnapshot, options: &PlannerOptions) -> bool {
    options
        .keep_size_bytes
        .is_some_and(|keep_size_bytes| snapshot.size_bytes <= keep_size_bytes)
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
