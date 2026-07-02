use crate::ReclaimResult;
use crate::model::{Plan, PlanEntry, PlanInput};
use crate::policy::PolicyKind;

use super::PlannerCandidate;
use super::foundation::plan_candidate_for_policy;

pub fn plan_candidate(candidate: PlannerCandidate, policy: PolicyKind) -> ReclaimResult<PlanEntry> {
    plan_candidate_for_policy(policy, candidate)
}

pub fn build_plan(
    input: PlanInput,
    policy: PolicyKind,
    candidates: impl IntoIterator<Item = PlannerCandidate>,
) -> ReclaimResult<Plan> {
    let entries = candidates
        .into_iter()
        .map(|candidate| plan_candidate(candidate, policy))
        .collect::<ReclaimResult<Vec<_>>>()?;

    Ok(Plan::new(input, entries))
}
