use std::time::SystemTime;

use crate::ReclaimResult;
use crate::model::{Plan, PlanAction, PlanEntry, PlanInput};
use crate::policy::PolicyKind;

use super::foundation::plan_candidate_for_policy;
use super::{ActiveObservation, PlannerCandidate, PlannerOptions};

pub fn plan_candidate(candidate: PlannerCandidate, policy: PolicyKind) -> ReclaimResult<PlanEntry> {
    plan_candidate_with_options(
        candidate,
        policy,
        &PlannerOptions::default(),
        SystemTime::now(),
    )
}

pub fn plan_candidate_with_options(
    candidate: PlannerCandidate,
    policy: PolicyKind,
    options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<PlanEntry> {
    plan_candidate_with_active_observation(
        candidate,
        policy,
        options,
        &ActiveObservation::not_attempted(),
        now,
    )
}

pub fn plan_candidate_with_active_observation(
    candidate: PlannerCandidate,
    policy: PolicyKind,
    options: &PlannerOptions,
    active_observation: &ActiveObservation,
    now: SystemTime,
) -> ReclaimResult<PlanEntry> {
    plan_candidate_for_policy(policy, candidate, options, active_observation, now)
}

pub fn build_plan(
    input: PlanInput,
    policy: PolicyKind,
    candidates: impl IntoIterator<Item = PlannerCandidate>,
) -> ReclaimResult<Plan> {
    build_plan_with_options(
        input,
        policy,
        candidates,
        &PlannerOptions::default(),
        SystemTime::now(),
    )
}

pub fn build_plan_with_options(
    input: PlanInput,
    policy: PolicyKind,
    candidates: impl IntoIterator<Item = PlannerCandidate>,
    options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    build_plan_with_active_observation(
        input,
        policy,
        candidates,
        options,
        &ActiveObservation::not_attempted(),
        now,
    )
}

pub fn build_plan_with_active_observation(
    input: PlanInput,
    policy: PolicyKind,
    candidates: impl IntoIterator<Item = PlannerCandidate>,
    options: &PlannerOptions,
    active_observation: &ActiveObservation,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    let entries = candidates
        .into_iter()
        .map(|candidate| {
            plan_candidate_with_active_observation(
                candidate,
                policy,
                options,
                active_observation,
                now,
            )
        })
        .collect::<ReclaimResult<Vec<_>>>()?;
    let entries = apply_minimum_reclaim_budget(entries, options)?;

    Ok(Plan::new(input, entries))
}

fn apply_minimum_reclaim_budget(
    mut entries: Vec<PlanEntry>,
    options: &PlannerOptions,
) -> ReclaimResult<Vec<PlanEntry>> {
    let Some(minimum_reclaim_bytes) = options.minimum_reclaim_bytes else {
        return Ok(entries);
    };
    if minimum_reclaim_bytes == 0 {
        return Ok(entries);
    }

    let mut selected_delete_indices = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.action == PlanAction::Delete)
        .map(|(index, entry)| {
            (
                index,
                budget_priority(entry),
                std::cmp::Reverse(entry.snapshot.size_bytes),
                entry.snapshot.path.clone(),
            )
        })
        .collect::<Vec<_>>();
    selected_delete_indices.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
    });

    let mut selected_bytes = 0u64;
    let mut keep_deleting = vec![false; entries.len()];
    for (index, _, _, _) in selected_delete_indices {
        if selected_bytes >= minimum_reclaim_bytes {
            break;
        }
        keep_deleting[index] = true;
        selected_bytes = selected_bytes.saturating_add(entries[index].snapshot.size_bytes);
    }

    for (index, entry) in entries.iter_mut().enumerate() {
        if entry.action == PlanAction::Delete && !keep_deleting[index] {
            *entry = PlanEntry::preserved(
                entry.snapshot.clone(),
                entry.artifact_class,
                entry.evidence.clone(),
                "budgeted cleanup goal is already satisfied by higher-priority delete candidates",
            )?;
        }
    }

    Ok(entries)
}

fn budget_priority(entry: &PlanEntry) -> u8 {
    match entry.artifact_class {
        crate::model::ArtifactClass::Tmp => 0,
        crate::model::ArtifactClass::Incremental => 1,
        crate::model::ArtifactClass::FingerprintGroupIntermediate => 2,
        crate::model::ArtifactClass::DepInfo | crate::model::ArtifactClass::ObjectMetadata => 3,
        crate::model::ArtifactClass::WholeTarget => 4,
        _ => 5,
    }
}
