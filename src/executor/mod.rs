mod report;
mod revalidate;

pub use report::{ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals};

use std::time::SystemTime;

use crate::persistence::{PersistedPlan, PlanPersistenceResult, ensure_plan_usable};

use self::revalidate::{delete_revalidated_entry, revalidate_entry};

pub fn validate_persisted_plan_for_apply(
    document: &PersistedPlan,
    now: SystemTime,
) -> PlanPersistenceResult<ApplyReport> {
    ensure_plan_usable(document, now)?;
    Ok(ApplyReport::new(
        document.id.clone(),
        collect_revalidated_entries(document),
    ))
}

pub fn execute_persisted_plan_apply(
    document: &PersistedPlan,
    now: SystemTime,
) -> PlanPersistenceResult<ApplyReport> {
    ensure_plan_usable(document, now)?;
    Ok(ApplyReport::executed(
        document.id.clone(),
        collect_deleted_entries(document),
    ))
}

fn collect_revalidated_entries(document: &PersistedPlan) -> Vec<ApplyEntryResult> {
    document
        .body
        .plan
        .entries
        .iter()
        .map(revalidate_entry)
        .collect()
}

fn collect_deleted_entries(document: &PersistedPlan) -> Vec<ApplyEntryResult> {
    document
        .body
        .plan
        .entries
        .iter()
        .map(revalidate_entry)
        .map(delete_revalidated_entry)
        .collect()
}
