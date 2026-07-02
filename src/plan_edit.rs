use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::SystemTime;

use crate::persistence::{
    PersistedPlan, PersistedPlanEntry, PersistedPlanTotals, PlanId, PlanPersistenceError,
    ensure_plan_usable,
};

const SELECT_REASON: &str = "explicitly selected for deletion";
const DESELECT_REASON: &str = "explicitly preserved by selection";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEditRequest {
    pub select: Vec<String>,
    pub deselect: Vec<String>,
}

impl PlanEditRequest {
    pub fn new(select: Vec<String>, deselect: Vec<String>) -> Result<Self, PlanEditError> {
        if select.is_empty() && deselect.is_empty() {
            return Err(PlanEditError::NoEdits);
        }

        let selected = select.iter().collect::<HashSet<_>>();
        if let Some(path) = deselect.iter().find(|path| selected.contains(path)) {
            return Err(PlanEditError::ConflictingEdit {
                path: (*path).clone(),
            });
        }

        Ok(Self { select, deselect })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEditReport {
    pub plan_id: PlanId,
    pub selected_count: usize,
    pub deselected_count: usize,
    pub totals: PersistedPlanTotals,
}

pub fn edit_persisted_plan(
    document: &mut PersistedPlan,
    request: &PlanEditRequest,
    now: SystemTime,
) -> Result<PlanEditReport, PlanEditError> {
    ensure_plan_usable(document, now)?;

    let path_index = build_path_index(&document.body.plan.entries);
    let selected_indices = resolve_paths(&path_index, &request.select)?;
    let deselected_indices = resolve_paths(&path_index, &request.deselect)?;
    drop(path_index);

    let selected_count = apply_paths(
        &mut document.body.plan.entries,
        &selected_indices,
        "delete",
        SELECT_REASON,
    );
    let deselected_count = apply_paths(
        &mut document.body.plan.entries,
        &deselected_indices,
        "preserve",
        DESELECT_REASON,
    );

    document.body.plan.totals = totals_from_entries(&document.body.plan.entries);
    document.body.interactive_selection_modified = true;
    document.id = PlanId::from_body(&document.body)?;

    Ok(PlanEditReport {
        plan_id: document.id.clone(),
        selected_count,
        deselected_count,
        totals: document.body.plan.totals.clone(),
    })
}

#[derive(Debug, Clone, Copy)]
struct PathEntry {
    index: usize,
    count: usize,
}

fn build_path_index(entries: &[PersistedPlanEntry]) -> HashMap<&str, PathEntry> {
    let mut index = HashMap::with_capacity(entries.len());
    for (entry_index, entry) in entries.iter().enumerate() {
        index
            .entry(entry.snapshot.path.as_str())
            .and_modify(|entry: &mut PathEntry| entry.count += 1)
            .or_insert(PathEntry {
                index: entry_index,
                count: 1,
            });
    }
    index
}

fn resolve_paths(
    index: &HashMap<&str, PathEntry>,
    paths: &[String],
) -> Result<Vec<usize>, PlanEditError> {
    let mut resolved = Vec::with_capacity(paths.len());
    for path in paths {
        match index.get(path.as_str()) {
            None => return Err(PlanEditError::EntryNotFound { path: path.clone() }),
            Some(entry) if entry.count > 1 => {
                return Err(PlanEditError::AmbiguousEntryPath { path: path.clone() });
            }
            Some(entry) => resolved.push(entry.index),
        }
    }
    Ok(resolved)
}

fn apply_paths(
    entries: &mut [PersistedPlanEntry],
    indices: &[usize],
    action: &str,
    reason: &str,
) -> usize {
    let mut edited_count = 0;
    for &entry_index in indices {
        let entry = &mut entries[entry_index];
        entry.action.clear();
        entry.action.push_str(action);
        entry.requires_confirmation = false;
        entry.policy_reason.clear();
        entry.policy_reason.push_str(reason);
        edited_count += 1;
    }
    edited_count
}

fn totals_from_entries(entries: &[PersistedPlanEntry]) -> PersistedPlanTotals {
    let mut totals = PersistedPlanTotals {
        entry_count: entries.len(),
        total_bytes: 0,
        preserved_count: 0,
        delete_candidate_count: 0,
    };

    for entry in entries {
        totals.total_bytes = totals.total_bytes.saturating_add(entry.snapshot.size_bytes);
        if entry.action == "delete" {
            totals.delete_candidate_count += 1;
        } else {
            totals.preserved_count += 1;
        }
    }

    totals
}

#[derive(Debug)]
pub enum PlanEditError {
    NoEdits,
    ConflictingEdit { path: String },
    EntryNotFound { path: String },
    AmbiguousEntryPath { path: String },
    Persistence(PlanPersistenceError),
}

impl fmt::Display for PlanEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEdits => formatter.write_str("edit-plan requires at least one edit"),
            Self::ConflictingEdit { path } => {
                write!(
                    formatter,
                    "entry `{path}` cannot be both selected and deselected"
                )
            }
            Self::EntryNotFound { path } => {
                write!(formatter, "no persisted plan entry matches `{path}`")
            }
            Self::AmbiguousEntryPath { path } => {
                write!(formatter, "persisted plan entry path `{path}` is ambiguous")
            }
            Self::Persistence(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for PlanEditError {}

impl From<PlanPersistenceError> for PlanEditError {
    fn from(error: PlanPersistenceError) -> Self {
        Self::Persistence(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{
        PERSISTED_PLAN_SCHEMA_VERSION, PersistedEvidence, PersistedInventoryOptions,
        PersistedPathSnapshot, PersistedPlan, PersistedPlanBody, PersistedPlanEntry,
        PersistedPlanInput, PersistedPlanSnapshot, PersistedPlannerOptions,
        PersistedScannerOptions, PersistedTimestamp, PlanCommandKind, PlanId, PlanInvocation,
    };
    use crate::{PLAN_SCHEMA_VERSION, PlanPersistenceResult};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn edits_matching_persisted_paths_and_recomputes_body_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original_id = document.id.clone();
        let delete_path = "target/debug/incremental".to_string();
        let preserve_path = "target/doc".to_string();

        let report = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new(vec![delete_path.clone()], vec![preserve_path.clone()])?,
            created_at,
        )?;

        assert_ne!(document.id, original_id);
        assert_eq!(document.id, PlanId::from_body(&document.body)?);
        assert!(document.body.interactive_selection_modified);
        assert_eq!(report.selected_count, 1);
        assert_eq!(report.deselected_count, 1);
        assert_eq!(report.totals.delete_candidate_count, 1);
        assert_eq!(report.totals.preserved_count, 1);
        let selected = entry(&document, &delete_path).expect("selected entry");
        assert_eq!(selected.action, "delete");
        assert!(!selected.requires_confirmation);
        assert_eq!(selected.policy_reason, SELECT_REASON);
        let deselected = entry(&document, &preserve_path).expect("deselected entry");
        assert_eq!(deselected.action, "preserve");
        assert!(!deselected.requires_confirmation);
        assert_eq!(deselected.policy_reason, DESELECT_REASON);
        Ok(())
    }

    #[test]
    fn rejects_unknown_paths_without_rewriting_id() -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new(vec!["missing".to_string()], Vec::new())?,
            created_at,
        )
        .expect_err("unknown path should fail");

        assert!(matches!(error, PlanEditError::EntryNotFound { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_later_unknown_paths_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new(
                vec![
                    "target/debug/incremental".to_string(),
                    "missing".to_string(),
                ],
                Vec::new(),
            )?,
            created_at,
        )
        .expect_err("unknown path should fail");

        assert!(matches!(error, PlanEditError::EntryNotFound { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_persisted_paths_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        document
            .body
            .plan
            .entries
            .push(persisted_entry("target/doc", "preserve", 4));
        document.body.plan.totals = PersistedPlanTotals {
            entry_count: 3,
            total_bytes: 11,
            preserved_count: 2,
            delete_candidate_count: 1,
        };
        document.id = PlanId::from_body(&document.body)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new(vec!["target/doc".to_string()], Vec::new())?,
            created_at,
        )
        .expect_err("ambiguous path should fail");

        assert!(matches!(error, PlanEditError::AmbiguousEntryPath { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    fn entry<'a>(document: &'a PersistedPlan, path: &str) -> Option<&'a PersistedPlanEntry> {
        document
            .body
            .plan
            .entries
            .iter()
            .find(|entry| entry.snapshot.path == path)
    }

    fn document(
        created_at: SystemTime,
        expires_at: SystemTime,
    ) -> PlanPersistenceResult<PersistedPlan> {
        let body = PersistedPlanBody {
            created_at: PersistedTimestamp::from_system_time(created_at)?,
            expires_at: PersistedTimestamp::from_system_time(expires_at)?,
            interactive_selection_modified: false,
            invocation: PlanInvocation {
                command: PlanCommandKind::Plan,
                policy: "balanced".to_string(),
                config_path: None,
                config_version: None,
                scanner_options: PersistedScannerOptions {
                    follow_symlinks: false,
                    allow_name_only_targets: false,
                    cross_filesystems: false,
                    ignored_paths: Vec::new(),
                },
                inventory_options: PersistedInventoryOptions {
                    follow_symlinks: false,
                },
                planner_options: PersistedPlannerOptions::default(),
            },
            plan: PersistedPlanSnapshot {
                schema_version: PLAN_SCHEMA_VERSION,
                input: PersistedPlanInput {
                    roots: vec![".".to_string()],
                },
                entries: vec![
                    persisted_entry("target/debug/incremental", "requires_confirmation", 3),
                    persisted_entry("target/doc", "delete", 4),
                ],
                totals: PersistedPlanTotals {
                    entry_count: 2,
                    total_bytes: 7,
                    preserved_count: 1,
                    delete_candidate_count: 1,
                },
            },
        };
        let id = PlanId::from_body(&body)?;
        Ok(PersistedPlan {
            schema_version: PERSISTED_PLAN_SCHEMA_VERSION,
            id,
            body,
        })
    }

    fn persisted_entry(path: &str, action: &str, size_bytes: u64) -> PersistedPlanEntry {
        PersistedPlanEntry {
            snapshot: PersistedPathSnapshot {
                path: path.to_string(),
                size_bytes,
                path_kind: "directory".to_string(),
                modified: None,
            },
            artifact_class: "incremental".to_string(),
            evidence: PersistedEvidence {
                kind: "weak_name_only".to_string(),
                marker: None,
                source: None,
                project_manifest: None,
                matched_name: Some("target".to_string()),
            },
            action: action.to_string(),
            policy_reason: "original".to_string(),
            requires_confirmation: action == "requires_confirmation",
        }
    }
}
