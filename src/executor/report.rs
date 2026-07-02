use crate::persistence::PlanId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub plan_id: PlanId,
    pub dry_run: bool,
    pub entries: Vec<ApplyEntryResult>,
    pub totals: ApplyTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyTotals {
    pub entry_count: usize,
    pub delete_candidate_count: usize,
    pub would_delete_count: usize,
    pub skipped_count: usize,
    pub stale_skip_count: usize,
    pub applied_count: usize,
    pub failed_count: usize,
    pub would_delete_bytes: u64,
    pub applied_bytes: u64,
}

impl ApplyReport {
    pub(super) fn new(plan_id: PlanId, entries: Vec<ApplyEntryResult>) -> Self {
        Self {
            plan_id,
            dry_run: true,
            totals: ApplyTotals::from_entries(&entries),
            entries,
        }
    }

    pub(super) fn executed(plan_id: PlanId, entries: Vec<ApplyEntryResult>) -> Self {
        Self {
            plan_id,
            dry_run: false,
            totals: ApplyTotals::from_entries(&entries),
            entries,
        }
    }
}

impl ApplyTotals {
    fn from_entries(entries: &[ApplyEntryResult]) -> Self {
        let mut totals = Self {
            entry_count: entries.len(),
            ..Self::default()
        };

        for entry in entries {
            if entry.planned_action == "delete" {
                totals.delete_candidate_count += 1;
            }

            match entry.status {
                ApplyEntryStatus::WouldDelete => {
                    totals.would_delete_count += 1;
                    totals.would_delete_bytes =
                        totals.would_delete_bytes.saturating_add(entry.size_bytes);
                }
                ApplyEntryStatus::Deleted => {
                    totals.applied_count += 1;
                    totals.applied_bytes = totals.applied_bytes.saturating_add(entry.size_bytes);
                }
                ApplyEntryStatus::NotPlannedForDeletion => totals.skipped_count += 1,
                ApplyEntryStatus::SkipStalePlan => {
                    totals.skipped_count += 1;
                    totals.stale_skip_count += 1;
                }
                ApplyEntryStatus::DeleteFailed => totals.failed_count += 1,
            }
        }

        totals
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyEntryResult {
    pub path: String,
    pub planned_action: String,
    pub status: ApplyEntryStatus,
    pub reason: String,
    pub size_bytes: u64,
}

impl ApplyEntryResult {
    pub(super) fn new(
        path: String,
        planned_action: String,
        status: ApplyEntryStatus,
        size_bytes: u64,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            path,
            planned_action,
            status,
            reason: reason.into(),
            size_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyEntryStatus {
    WouldDelete,
    Deleted,
    NotPlannedForDeletion,
    SkipStalePlan,
    DeleteFailed,
}
