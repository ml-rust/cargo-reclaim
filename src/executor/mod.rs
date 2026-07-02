mod report;
mod revalidate;

pub use report::{ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals};

use std::time::SystemTime;

use crate::persistence::{PersistedPlan, PlanPersistenceResult, ensure_plan_usable};

use self::revalidate::revalidate_entry;

pub fn validate_persisted_plan_for_apply(
    document: &PersistedPlan,
    now: SystemTime,
) -> PlanPersistenceResult<ApplyReport> {
    ensure_plan_usable(document, now)?;

    let entries = document
        .body
        .plan
        .entries
        .iter()
        .map(revalidate_entry)
        .collect();

    Ok(ApplyReport::new(document.id.clone(), entries))
}
