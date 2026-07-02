use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::inventory::InventoryOptions;
use crate::model::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathKind, PathSnapshot, Plan, PlanAction, PlanEntry,
    PlanInput, PlanTotals, TargetEvidence,
};
use crate::planner::PlannerOptions;
use crate::policy::PolicyKind;
use crate::scanner::ScannerOptions;

use super::PERSISTED_PLAN_SCHEMA_VERSION;
use super::error::{PlanPersistenceError, PlanPersistenceResult};
use super::id::PlanId;
use super::time::PersistedTimestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlan {
    pub schema_version: u16,
    pub id: PlanId,
    #[serde(flatten)]
    pub body: PersistedPlanBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlanBody {
    pub created_at: PersistedTimestamp,
    pub expires_at: PersistedTimestamp,
    pub interactive_selection_modified: bool,
    pub invocation: PlanInvocation,
    pub plan: PersistedPlanSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePlanOptions {
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
    pub interactive_selection_modified: bool,
    pub invocation: PlanInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanInvocation {
    pub command: PlanCommandKind,
    pub policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_version: Option<u16>,
    pub scanner_options: PersistedScannerOptions,
    pub inventory_options: PersistedInventoryOptions,
    #[serde(default)]
    pub planner_options: PersistedPlannerOptions,
}

impl PlanInvocation {
    pub fn new(
        command: PlanCommandKind,
        policy: PolicyKind,
        scanner_options: &ScannerOptions,
        inventory_options: &InventoryOptions,
        planner_options: &PlannerOptions,
    ) -> Self {
        Self {
            command,
            policy: policy_label(policy).to_string(),
            config_path: None,
            config_version: None,
            scanner_options: PersistedScannerOptions::from_options(scanner_options),
            inventory_options: PersistedInventoryOptions::from_options(inventory_options),
            planner_options: PersistedPlannerOptions::from_options(planner_options),
        }
    }

    pub fn with_config(mut self, path: &Path, version: u16) -> Self {
        self.config_path = Some(path_string(path));
        self.config_version = Some(version);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanCommandKind {
    Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedScannerOptions {
    pub follow_symlinks: bool,
    pub allow_name_only_targets: bool,
    pub cross_filesystems: bool,
    pub ignored_paths: Vec<String>,
}

impl PersistedScannerOptions {
    fn from_options(options: &ScannerOptions) -> Self {
        Self {
            follow_symlinks: options.follow_symlinks,
            allow_name_only_targets: options.allow_name_only_targets,
            cross_filesystems: options.cross_filesystems,
            ignored_paths: options.ignored_paths.iter().map(path_string).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedInventoryOptions {
    pub follow_symlinks: bool,
}

impl PersistedInventoryOptions {
    fn from_options(options: &InventoryOptions) -> Self {
        Self {
            follow_symlinks: options.follow_symlinks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PersistedPlannerOptions {
    pub recent_write_keep_window_seconds: Option<u64>,
}

impl PersistedPlannerOptions {
    fn from_options(options: &PlannerOptions) -> Self {
        Self {
            recent_write_keep_window_seconds: options
                .recent_write_keep_window
                .map(|duration| duration.as_secs()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlanSnapshot {
    pub schema_version: u16,
    pub input: PersistedPlanInput,
    pub entries: Vec<PersistedPlanEntry>,
    pub totals: PersistedPlanTotals,
}

impl PersistedPlanSnapshot {
    fn from_plan(plan: &Plan) -> Self {
        Self {
            schema_version: plan.schema_version,
            input: PersistedPlanInput::from_input(&plan.input),
            entries: plan
                .entries
                .iter()
                .map(PersistedPlanEntry::from_entry)
                .collect(),
            totals: PersistedPlanTotals::from_totals(plan.totals),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlanInput {
    pub roots: Vec<String>,
}

impl PersistedPlanInput {
    fn from_input(input: &PlanInput) -> Self {
        Self {
            roots: input.roots.iter().map(path_string).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlanEntry {
    pub snapshot: PersistedPathSnapshot,
    pub artifact_class: String,
    pub evidence: PersistedEvidence,
    pub action: String,
    pub policy_reason: String,
    pub requires_confirmation: bool,
}

impl PersistedPlanEntry {
    fn from_entry(entry: &PlanEntry) -> Self {
        Self {
            snapshot: PersistedPathSnapshot::from_snapshot(&entry.snapshot),
            artifact_class: artifact_label(entry.artifact_class).to_string(),
            evidence: PersistedEvidence::from_evidence(&entry.evidence),
            action: action_label(&entry.action).to_string(),
            policy_reason: entry.policy_reason.clone(),
            requires_confirmation: entry.requires_confirmation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPathSnapshot {
    pub path: String,
    pub size_bytes: u64,
    pub path_kind: String,
    pub modified: Option<PersistedTimestamp>,
}

impl PersistedPathSnapshot {
    fn from_snapshot(snapshot: &PathSnapshot) -> Self {
        Self {
            path: path_string(&snapshot.path),
            size_bytes: snapshot.size_bytes,
            path_kind: path_kind_label(snapshot.path_kind).to_string(),
            modified: snapshot
                .modified
                .and_then(|modified| PersistedTimestamp::from_system_time(modified).ok()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPlanTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub preserved_count: usize,
    pub delete_candidate_count: usize,
}

impl PersistedPlanTotals {
    fn from_totals(totals: PlanTotals) -> Self {
        Self {
            entry_count: totals.entry_count,
            total_bytes: totals.total_bytes,
            preserved_count: totals.preserved_count,
            delete_candidate_count: totals.delete_candidate_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedEvidence {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_manifest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_name: Option<String>,
}

impl PersistedEvidence {
    fn from_evidence(evidence: &TargetEvidence) -> Self {
        match evidence {
            TargetEvidence::StrongMarker { marker } => Self {
                kind: "strong_marker".to_string(),
                marker: Some(marker.clone()),
                source: None,
                project_manifest: None,
                matched_name: None,
            },
            TargetEvidence::ConfiguredPath { source } => Self {
                kind: "configured_path".to_string(),
                marker: None,
                source: Some(source.clone()),
                project_manifest: None,
                matched_name: None,
            },
            TargetEvidence::ProjectContext { project_manifest } => Self {
                kind: "project_context".to_string(),
                marker: None,
                source: None,
                project_manifest: Some(path_string(project_manifest)),
                matched_name: None,
            },
            TargetEvidence::WeakNameOnly { matched_name } => Self {
                kind: "weak_name_only".to_string(),
                marker: None,
                source: None,
                project_manifest: None,
                matched_name: Some(matched_name.clone()),
            },
        }
    }
}

pub fn persist_plan(plan: &Plan, options: SavePlanOptions) -> PlanPersistenceResult<PersistedPlan> {
    if options.expires_at <= options.created_at {
        return Err(PlanPersistenceError::InvalidTimeRange);
    }

    let body = PersistedPlanBody {
        created_at: PersistedTimestamp::from_system_time(options.created_at)?,
        expires_at: PersistedTimestamp::from_system_time(options.expires_at)?,
        interactive_selection_modified: options.interactive_selection_modified,
        invocation: options.invocation,
        plan: PersistedPlanSnapshot::from_plan(plan),
    };
    let id = PlanId::from_body(&body)?;

    Ok(PersistedPlan {
        schema_version: PERSISTED_PLAN_SCHEMA_VERSION,
        id,
        body,
    })
}

pub fn ensure_plan_usable(document: &PersistedPlan, now: SystemTime) -> PlanPersistenceResult<()> {
    if document.schema_version != PERSISTED_PLAN_SCHEMA_VERSION {
        return Err(PlanPersistenceError::PersistenceSchemaMismatch {
            found: document.schema_version,
            expected: PERSISTED_PLAN_SCHEMA_VERSION,
        });
    }

    if document.body.plan.schema_version != PLAN_SCHEMA_VERSION {
        return Err(PlanPersistenceError::PlanSchemaMismatch {
            found: document.body.plan.schema_version,
            expected: PLAN_SCHEMA_VERSION,
        });
    }

    let expected_id = PlanId::from_body(&document.body)?;
    if expected_id != document.id {
        return Err(PlanPersistenceError::PlanIdMismatch {
            expected: expected_id.0,
            found: document.id.0.clone(),
        });
    }

    if now >= document.body.expires_at.to_system_time() {
        return Err(PlanPersistenceError::PlanExpired);
    }

    Ok(())
}

fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

fn action_label(action: &PlanAction) -> &'static str {
    match action {
        PlanAction::Delete => "delete",
        PlanAction::Preserve => "preserve",
        PlanAction::SkipActive => "skip_active",
        PlanAction::SkipLocked => "skip_locked",
        PlanAction::Unknown => "unknown",
        PlanAction::RequiresConfirmation => "requires_confirmation",
    }
}

fn artifact_label(artifact_class: ArtifactClass) -> &'static str {
    match artifact_class {
        ArtifactClass::Incremental => "incremental",
        ArtifactClass::Deps => "deps",
        ArtifactClass::BuildScripts => "build_scripts",
        ArtifactClass::Fingerprint => "fingerprint",
        ArtifactClass::Docs => "docs",
        ArtifactClass::Package => "package",
        ArtifactClass::Timings => "timings",
        ArtifactClass::Tmp => "tmp",
        ArtifactClass::DepInfo => "dep_info",
        ArtifactClass::ObjectMetadata => "object_metadata",
        ArtifactClass::FinalExecutable => "final_executable",
        ArtifactClass::FinalLibrary => "final_library",
        ArtifactClass::FinalRlib => "final_rlib",
        ArtifactClass::FinalWasm => "final_wasm",
        ArtifactClass::Unknown => "unknown",
    }
}

fn path_kind_label(path_kind: PathKind) -> &'static str {
    match path_kind {
        PathKind::File => "file",
        PathKind::Directory => "directory",
        PathKind::Symlink => "symlink",
        PathKind::Unknown => "unknown",
    }
}

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}
