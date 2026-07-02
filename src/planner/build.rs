use std::time::SystemTime;

use crate::ReclaimResult;
use crate::model::{Plan, PlanEntry, PlanInput};
use crate::policy::PolicyKind;

use super::foundation::plan_candidate_for_policy;
use super::{PlannerCandidate, PlannerOptions};

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
    plan_candidate_for_policy(policy, candidate, options, now)
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
    let entries = candidates
        .into_iter()
        .map(|candidate| plan_candidate_with_options(candidate, policy, options, now))
        .collect::<ReclaimResult<Vec<_>>>()?;

    Ok(Plan::new(input, entries))
}
