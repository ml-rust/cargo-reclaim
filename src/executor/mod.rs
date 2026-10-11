mod activity;
mod report;
mod revalidate;

pub use report::{ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals};

use std::time::SystemTime;

use crate::persistence::{PersistedPlan, PlanPersistenceResult, ensure_plan_usable};

use self::activity::ApplyActivity;
use self::revalidate::delete_revalidated_entry;

pub fn validate_persisted_plan_for_apply(
    document: &PersistedPlan,
    now: SystemTime,
) -> PlanPersistenceResult<ApplyReport> {
    ensure_plan_usable(document, now)?;
    Ok(ApplyReport::new(
        document.id.clone(),
        collect_revalidated_entries(document, now),
    ))
}

pub fn execute_persisted_plan_apply(
    document: &PersistedPlan,
    now: SystemTime,
) -> PlanPersistenceResult<ApplyReport> {
    execute_immediate_plan_apply(document, now, false)
}

/// Only an in-memory request may bypass activity protection. Saved plans do not
/// carry this authorization, and all file snapshot checks still apply.
pub(crate) fn execute_immediate_plan_apply(
    document: &PersistedPlan,
    now: SystemTime,
    interrupt_active_build: bool,
) -> PlanPersistenceResult<ApplyReport> {
    ensure_plan_usable(document, now)?;
    Ok(ApplyReport::executed(
        document.id.clone(),
        collect_deleted_entries(document, now, interrupt_active_build),
    ))
}

fn collect_revalidated_entries(document: &PersistedPlan, now: SystemTime) -> Vec<ApplyEntryResult> {
    let mut activity = ApplyActivity::new(document, now);
    document
        .body
        .plan
        .entries
        .iter()
        .map(|entry| activity.revalidate(entry))
        .collect()
}

fn collect_deleted_entries(
    document: &PersistedPlan,
    now: SystemTime,
    interrupt_active_build: bool,
) -> Vec<ApplyEntryResult> {
    let mut activity = (!interrupt_active_build).then(|| ApplyActivity::new(document, now));
    document
        .body
        .plan
        .entries
        .iter()
        .map(|entry| match &mut activity {
            Some(activity) => activity.revalidate(entry),
            None => revalidate::revalidate_entry(entry),
        })
        .map(delete_revalidated_entry)
        .collect()
}
