use std::time::{Duration, SystemTime};

use crate::ReclaimResult;
use crate::model::{ArtifactClass, PathSnapshot, PlanAction, PlanEntry, TargetEvidence};
use crate::policy::PolicyKind;

use super::{
    ActiveObservation, PlannerCandidate, PlannerOptions, ProcessView, TargetContext,
    WholeTargetMode,
};

/// Default age below which the `Sweep` policy will not reclaim a final binary.
const DEFAULT_SWEEP_OLDER_THAN: Duration = Duration::from_secs(24 * 60 * 60);

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
        target_newest_modified,
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

    if policy.is_protected_output(artifact_class) {
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

    // While a build is touching this target, protect the WHOLE target — delete nothing.
    // cargo-reclaim cannot reliably tell which artifacts the running build still needs:
    // even a "superseded" hash variant can be a live feature-variant the linker wants
    // (multiple concurrent hashes exist under `--all-features`), and cargo will not
    // rebuild an output its fingerprint DB considers fresh. Only cargo's own fingerprint
    // DB is authoritative, so any mid-build deletion risks breaking the build. Reclaim
    // happens between builds instead — where cargo re-plans and rebuilds what it needs.
    // A disruptive trigger (`interrupt_active_build`) opts out of this on purpose,
    // accepting a broken build to reclaim space now.
    if !options.interrupt_active_build
        && let Some(active_reason) = active_skip_reason(target_context.as_ref(), active_observation)
    {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::SkipActive,
            active_reason,
            false,
        );
    }

    // Race-free active-build protection: a build writes into its target
    // continuously, so the target's newest artifact mtime stays fresh for the
    // whole build. Unlike the point-in-time process scan above — which can be
    // sampled in a gap between rustc invocations, or miss a build driver like
    // cargo-nextest it does not recognize — this cannot miss an active build.
    // While the target was written within the keep window, protect it entirely:
    // we cannot distinguish a superseded hash variant from a live feature-variant
    // the running build still needs without cargo's fingerprint DB, and cargo
    // will not rebuild an output its fingerprint DB considers fresh. This is the
    // only guard for StaleDeps/StaleIncremental, whose own mtimes are old by
    // definition, so the per-file check below never protects them. Reclaim
    // happens between builds, where cargo re-plans and rebuilds anything removed.
    if !options.interrupt_active_build
        && is_modified_within(
            &target_newest_modified,
            options.recent_write_keep_window,
            now,
        )
    {
        return PlanEntry::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::SkipActive,
            "target was written within the active-project keep window",
            false,
        );
    }

    // No active build: protect only the recent-write hot set. Between builds cargo
    // re-plans and rebuilds any output we remove, so age-based reclaim is recoverable.
    if !matches!(
        artifact_class,
        ArtifactClass::StaleDeps | ArtifactClass::StaleIncremental
    ) && is_modified_within(&snapshot.modified, options.recent_write_keep_window, now)
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

    // The Sweep policy reclaims final binaries, but only once they are clearly
    // cold (older than the sweep age threshold), so a recently-built binary is
    // never swept out from under active work.
    if policy == PolicyKind::Sweep
        && PolicyKind::is_sweep_final_artifact(artifact_class)
        && !is_older_than_sweep_threshold(&snapshot.modified, options, now)
    {
        return PlanEntry::preserved(
            snapshot,
            artifact_class,
            evidence,
            "final artifact is newer than the sweep age threshold",
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

            if !options.interrupt_active_build
                && is_recently_modified(&snapshot.modified, options, now)
            {
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

            if !options.interrupt_active_build
                && let Some(reason) =
                    active_skip_reason(target_context.as_ref(), active_observation)
            {
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
    is_modified_within(modified, options.recent_write_keep_window, now)
}

/// Whether `modified` falls within `window` of `now`. No window ⇒ never within
/// (nothing protected); an unreadable/future mtime is treated as within (protect).
fn is_modified_within(
    modified: &Option<SystemTime>,
    window: Option<Duration>,
    now: SystemTime,
) -> bool {
    let Some(window) = window else {
        return false;
    };
    let Some(modified) = *modified else {
        return false;
    };
    now.duration_since(modified)
        .map(|age| age <= window)
        .unwrap_or(true)
}

/// Whether an artifact is old enough for the `Sweep` policy to reclaim it. An
/// unreadable/future mtime is treated as not-old-enough (preserve).
fn is_older_than_sweep_threshold(
    modified: &Option<SystemTime>,
    options: &PlannerOptions,
    now: SystemTime,
) -> bool {
    let threshold = options.sweep_older_than.unwrap_or(DEFAULT_SWEEP_OLDER_THAN);
    let Some(modified) = *modified else {
        return false;
    };
    now.duration_since(modified)
        .map(|age| age > threshold)
        .unwrap_or(false)
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
        PolicyKind::Sweep => {
            PolicyKind::is_default_removable_class(artifact_class)
                || PolicyKind::is_sweep_final_artifact(artifact_class)
        }
    }
}
