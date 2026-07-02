use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::SystemTime;

use crate::persistence::{
    PersistedPlan, PersistedPlanEntry, PersistedPlanTotals, PlanId, PlanPersistenceError,
    ensure_plan_usable,
};

const SELECT_REASON: &str = "explicitly selected for deletion";
const DESELECT_REASON: &str = "explicitly preserved by selection";
const CANONICAL_ARTIFACT_CLASSES: &[&str] = &[
    "whole_target",
    "incremental",
    "deps",
    "build_scripts",
    "fingerprint",
    "docs",
    "package",
    "timings",
    "tmp",
    "dep_info",
    "object_metadata",
    "final_executable",
    "final_library",
    "final_rlib",
    "final_wasm",
    "unknown",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEditRequest {
    pub select: Vec<String>,
    pub deselect: Vec<String>,
    pub select_indices: Vec<usize>,
    pub deselect_indices: Vec<usize>,
    pub select_classes: Vec<String>,
    pub deselect_classes: Vec<String>,
}

impl PlanEditRequest {
    pub fn new(select: Vec<String>, deselect: Vec<String>) -> Result<Self, PlanEditError> {
        Self::new_with_indices(select, deselect, Vec::new(), Vec::new())
    }

    pub fn new_with_indices(
        select: Vec<String>,
        deselect: Vec<String>,
        select_indices: Vec<usize>,
        deselect_indices: Vec<usize>,
    ) -> Result<Self, PlanEditError> {
        Self::new_with_class_selectors(
            select,
            deselect,
            select_indices,
            deselect_indices,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn new_with_class_selectors(
        select: Vec<String>,
        deselect: Vec<String>,
        select_indices: Vec<usize>,
        deselect_indices: Vec<usize>,
        select_classes: Vec<String>,
        deselect_classes: Vec<String>,
    ) -> Result<Self, PlanEditError> {
        if select.is_empty()
            && deselect.is_empty()
            && select_indices.is_empty()
            && deselect_indices.is_empty()
            && select_classes.is_empty()
            && deselect_classes.is_empty()
        {
            return Err(PlanEditError::NoEdits);
        }

        validate_class_labels(&select_classes, ClassEditAction::Select)?;
        validate_class_labels(&deselect_classes, ClassEditAction::Deselect)?;

        let selected = select.iter().collect::<HashSet<_>>();
        if let Some(path) = deselect.iter().find(|path| selected.contains(path)) {
            return Err(PlanEditError::ConflictingEdit {
                path: (*path).clone(),
            });
        }

        if select_indices
            .iter()
            .chain(deselect_indices.iter())
            .any(|index| *index == 0)
        {
            return Err(PlanEditError::EntryNotFound {
                path: "entry index 0".to_string(),
            });
        }

        let selected_indices = select_indices.iter().collect::<HashSet<_>>();
        if let Some(index) = deselect_indices
            .iter()
            .find(|index| selected_indices.contains(index))
        {
            return Err(PlanEditError::ConflictingEdit {
                path: format!("entry index {}", index),
            });
        }

        let selected_classes = select_classes.iter().collect::<HashSet<_>>();
        if let Some(label) = deselect_classes
            .iter()
            .find(|label| selected_classes.contains(label))
        {
            return Err(PlanEditError::ConflictingEdit {
                path: format!("artifact class {label}"),
            });
        }

        Ok(Self {
            select,
            deselect,
            select_indices,
            deselect_indices,
            select_classes,
            deselect_classes,
        })
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
    let mut selected_indices = resolve_paths(&path_index, &request.select)?;
    let mut deselected_indices = resolve_paths(&path_index, &request.deselect)?;
    drop(path_index);
    selected_indices.extend(resolve_entry_indices(
        document.body.plan.entries.len(),
        &request.select_indices,
    )?);
    deselected_indices.extend(resolve_entry_indices(
        document.body.plan.entries.len(),
        &request.deselect_indices,
    )?);
    selected_indices.extend(resolve_artifact_classes(
        &document.body.plan.entries,
        &request.select_classes,
    )?);
    deselected_indices.extend(resolve_artifact_classes(
        &document.body.plan.entries,
        &request.deselect_classes,
    )?);
    dedupe_positions(&mut selected_indices);
    dedupe_positions(&mut deselected_indices);
    reject_conflicting_positions(
        &document.body.plan.entries,
        &selected_indices,
        &deselected_indices,
    )?;

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

fn resolve_entry_indices(
    entry_count: usize,
    indices: &[usize],
) -> Result<Vec<usize>, PlanEditError> {
    let mut resolved = Vec::with_capacity(indices.len());
    for &index in indices {
        if index == 0 {
            return Err(PlanEditError::EntryNotFound {
                path: format!("entry index {index}"),
            });
        }
        if index > entry_count {
            return Err(PlanEditError::EntryNotFound {
                path: format!("entry index {index}"),
            });
        }
        resolved.push(index - 1);
    }
    Ok(resolved)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassEditAction {
    Select,
    Deselect,
}

fn validate_class_labels(labels: &[String], action: ClassEditAction) -> Result<(), PlanEditError> {
    for label in labels {
        if !CANONICAL_ARTIFACT_CLASSES.contains(&label.as_str()) {
            return Err(PlanEditError::UnknownArtifactClass {
                label: label.clone(),
            });
        }
        if action == ClassEditAction::Select && matches!(label.as_str(), "unknown" | "whole_target")
        {
            return Err(PlanEditError::ProtectedArtifactClass {
                label: label.clone(),
            });
        }
    }
    Ok(())
}

fn resolve_artifact_classes(
    entries: &[PersistedPlanEntry],
    labels: &[String],
) -> Result<Vec<usize>, PlanEditError> {
    let mut resolved = Vec::new();
    for label in labels {
        let start_len = resolved.len();
        for (index, entry) in entries.iter().enumerate() {
            if entry.artifact_class == *label {
                resolved.push(index);
            }
        }
        if resolved.len() == start_len {
            return Err(PlanEditError::ArtifactClassNotFound {
                label: label.clone(),
            });
        }
    }
    Ok(resolved)
}

fn reject_conflicting_positions(
    entries: &[PersistedPlanEntry],
    selected_indices: &[usize],
    deselected_indices: &[usize],
) -> Result<(), PlanEditError> {
    let selected = selected_indices.iter().copied().collect::<HashSet<_>>();
    for &index in deselected_indices {
        if selected.contains(&index) {
            let entry = entries
                .get(index)
                .ok_or_else(|| PlanEditError::EntryNotFound {
                    path: format!("entry index {}", index.saturating_add(1)),
                })?;
            return Err(PlanEditError::ConflictingEdit {
                path: entry.snapshot.path.clone(),
            });
        }
    }
    Ok(())
}

fn dedupe_positions(indices: &mut Vec<usize>) {
    let mut seen = HashSet::with_capacity(indices.len());
    indices.retain(|index| seen.insert(*index));
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
    UnknownArtifactClass { label: String },
    ProtectedArtifactClass { label: String },
    ArtifactClassNotFound { label: String },
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
            Self::UnknownArtifactClass { label } => {
                write!(formatter, "unknown artifact class selector `{label}`")
            }
            Self::ProtectedArtifactClass { label } => {
                write!(
                    formatter,
                    "artifact class `{label}` cannot be selected by class; select explicit entries by path or index"
                )
            }
            Self::ArtifactClassNotFound { label } => {
                write!(
                    formatter,
                    "no persisted plan entry matches artifact class `{label}`"
                )
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
    fn edits_matching_persisted_entry_indices_and_recomputes_body_id()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original_id = document.id.clone();

        let report = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_indices(Vec::new(), Vec::new(), vec![1], vec![2])?,
            created_at,
        )?;

        assert_ne!(document.id, original_id);
        assert_eq!(document.id, PlanId::from_body(&document.body)?);
        assert!(document.body.interactive_selection_modified);
        assert_eq!(report.selected_count, 1);
        assert_eq!(report.deselected_count, 1);
        assert_eq!(document.body.plan.entries[0].action, "delete");
        assert_eq!(document.body.plan.entries[0].policy_reason, SELECT_REASON);
        assert_eq!(document.body.plan.entries[1].action, "preserve");
        assert_eq!(document.body.plan.entries[1].policy_reason, DESELECT_REASON);
        Ok(())
    }

    #[test]
    fn dedupes_same_action_entry_indices_before_reporting() -> Result<(), Box<dyn std::error::Error>>
    {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;

        let report = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_indices(
                vec!["target/debug/incremental".to_string()],
                Vec::new(),
                vec![1],
                Vec::new(),
            )?,
            created_at,
        )?;

        assert_eq!(report.selected_count, 1);
        assert_eq!(document.body.plan.entries[0].action, "delete");
        Ok(())
    }

    #[test]
    fn edits_matching_persisted_artifact_classes() -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        document.body.plan.entries[1].artifact_class = "docs".to_string();
        document.id = PlanId::from_body(&document.body)?;

        let report = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_class_selectors(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec!["incremental".to_string()],
                vec!["docs".to_string()],
            )?,
            created_at,
        )?;

        assert_eq!(report.selected_count, 1);
        assert_eq!(report.deselected_count, 1);
        assert_eq!(document.body.plan.entries[0].action, "delete");
        assert_eq!(document.body.plan.entries[0].policy_reason, SELECT_REASON);
        assert_eq!(document.body.plan.entries[1].action, "preserve");
        assert_eq!(document.body.plan.entries[1].policy_reason, DESELECT_REASON);
        Ok(())
    }

    #[test]
    fn dedupes_same_action_artifact_classes_before_reporting()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;

        let report = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_class_selectors(
                Vec::new(),
                Vec::new(),
                vec![1],
                Vec::new(),
                vec!["incremental".to_string(), "incremental".to_string()],
                Vec::new(),
            )?,
            created_at,
        )?;

        assert_eq!(report.selected_count, 2);
        assert_eq!(document.body.plan.entries[0].action, "delete");
        assert_eq!(document.body.plan.entries[1].action, "delete");
        Ok(())
    }

    #[test]
    fn rejects_unknown_artifact_class_labels_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = PlanEditRequest::new_with_class_selectors(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["typo".to_string()],
            Vec::new(),
        )
        .expect_err("unknown artifact class should fail");

        assert!(matches!(error, PlanEditError::UnknownArtifactClass { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_broad_unknown_artifact_class_selection_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = PlanEditRequest::new_with_class_selectors(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["unknown".to_string()],
            Vec::new(),
        )
        .expect_err("unknown class selection should fail");

        assert!(matches!(
            error,
            PlanEditError::ProtectedArtifactClass { .. }
        ));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_unmatched_artifact_classes_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_class_selectors(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec!["docs".to_string()],
                Vec::new(),
            )?,
            created_at,
        )
        .expect_err("unmatched artifact class should fail");

        assert!(matches!(error, PlanEditError::ArtifactClassNotFound { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_unmatched_persisted_entry_indices_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_indices(Vec::new(), Vec::new(), vec![3], Vec::new())?,
            created_at,
        )
        .expect_err("unmatched index should fail");

        assert!(matches!(error, PlanEditError::EntryNotFound { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_zero_persisted_entry_indices_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = PlanEditRequest::new_with_indices(Vec::new(), Vec::new(), vec![0], Vec::new())
            .expect_err("zero index should fail");

        assert!(matches!(error, PlanEditError::EntryNotFound { .. }));
        assert_eq!(document, original);
        Ok(())
    }

    #[test]
    fn rejects_cross_action_entry_conflicts_without_partial_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let created_at = UNIX_EPOCH + Duration::from_secs(100);
        let expires_at = created_at + Duration::from_secs(60);
        let mut document = document(created_at, expires_at)?;
        let original = document.clone();

        let error = edit_persisted_plan(
            &mut document,
            &PlanEditRequest::new_with_indices(
                vec!["target/debug/incremental".to_string()],
                Vec::new(),
                Vec::new(),
                vec![1],
            )?,
            created_at,
        )
        .expect_err("same entry conflict should fail");

        assert!(matches!(error, PlanEditError::ConflictingEdit { .. }));
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
