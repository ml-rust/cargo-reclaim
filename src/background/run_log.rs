use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::executor::{ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals};
use crate::model::{Plan, PlanSkip, PlanTotals};
use crate::persistence::{PersistedTimestamp, PlanId};
use crate::policy::PolicyKind;
use crate::watcher::{WatcherDecision, WatcherDecisionState, WatcherTriggerReason};

pub const BACKGROUND_RUN_LOG_SCHEMA_VERSION: u16 = 1;

pub type BackgroundRunLogResult<T> = Result<T, BackgroundRunLogError>;

#[derive(Debug)]
pub enum BackgroundRunLogError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    Json {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
    UnsupportedSchemaVersion {
        path: PathBuf,
        line: usize,
        found: u16,
        expected: u16,
    },
}

impl fmt::Display for BackgroundRunLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "background run log IO error at {}: {source}",
                    path.display()
                )
            }
            Self::Serialize { path, source } => {
                write!(
                    formatter,
                    "failed to serialize background run log record for {}: {source}",
                    path.display()
                )
            }
            Self::Json { path, line, source } => {
                write!(
                    formatter,
                    "failed to parse background run log record at {} line {line}: {source}",
                    path.display()
                )
            }
            Self::UnsupportedSchemaVersion {
                path,
                line,
                found,
                expected,
            } => {
                write!(
                    formatter,
                    "unsupported background run log schema version {found} at {} line {line}; expected {expected}",
                    path.display()
                )
            }
        }
    }
}

impl Error for BackgroundRunLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Serialize { source, .. } | Self::Json { source, .. } => Some(source),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRunLogRecord {
    pub schema_version: u16,
    pub run_id: String,
    pub recorded_at: PersistedTimestamp,
    #[serde(rename = "event")]
    pub kind: BackgroundRunEventKind,
    pub trigger: Option<BackgroundTriggerSummary>,
    pub selected_policy: Option<String>,
    pub plan: Option<BackgroundPlanSummary>,
    #[serde(default)]
    pub skipped_projects: Vec<BackgroundSkippedProject>,
    pub apply: Option<BackgroundApplySummary>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub problems: Vec<String>,
}

impl BackgroundRunLogRecord {
    pub fn new(
        run_id: impl Into<String>,
        recorded_at: PersistedTimestamp,
        kind: BackgroundRunEventKind,
    ) -> Self {
        Self {
            schema_version: BACKGROUND_RUN_LOG_SCHEMA_VERSION,
            run_id: run_id.into(),
            recorded_at,
            kind,
            trigger: None,
            selected_policy: None,
            plan: None,
            skipped_projects: Vec::new(),
            apply: None,
            recommendations: Vec::new(),
            problems: Vec::new(),
        }
    }

    pub fn with_trigger(mut self, trigger: BackgroundTriggerSummary) -> Self {
        self.trigger = Some(trigger);
        self
    }

    pub fn with_selected_policy(mut self, policy: PolicyKind) -> Self {
        self.selected_policy = Some(policy_label(policy).to_owned());
        self
    }

    pub fn with_plan(mut self, plan: BackgroundPlanSummary) -> Self {
        self.plan = Some(plan);
        self
    }

    pub fn with_plan_skipped_paths(mut self, plan: &Plan) -> Self {
        self.skipped_projects.extend(
            plan.skipped_paths
                .iter()
                .map(BackgroundSkippedProject::from_skip),
        );
        self
    }

    pub fn with_apply(mut self, apply: BackgroundApplySummary) -> Self {
        self.apply = Some(apply);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundRunEventKind {
    Started,
    Triggered,
    PlanBuilt,
    ApplyCompleted,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTriggerSummary {
    pub state: String,
    pub reasons: Vec<BackgroundTriggerReasonSummary>,
}

impl BackgroundTriggerSummary {
    pub fn from_watcher_decision(decision: &WatcherDecision) -> Self {
        Self {
            state: watcher_decision_state_label(decision.state).to_owned(),
            reasons: decision
                .reasons
                .iter()
                .map(BackgroundTriggerReasonSummary::from_watcher_trigger_reason)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackgroundTriggerReasonSummary {
    TargetSizeExceeded {
        path: String,
        size_bytes: u64,
        max_target_size_bytes: u64,
    },
    DiskFreeBelow {
        free_basis_points: u16,
        threshold_basis_points: u16,
    },
    DiskFreeBytesBelow {
        free_bytes: u64,
        min_free_disk_bytes: u64,
    },
}

impl BackgroundTriggerReasonSummary {
    fn from_watcher_trigger_reason(reason: &WatcherTriggerReason) -> Self {
        match reason {
            WatcherTriggerReason::TargetSizeExceeded {
                path,
                size_bytes,
                max_target_size_bytes,
            } => Self::TargetSizeExceeded {
                path: path.display().to_string(),
                size_bytes: *size_bytes,
                max_target_size_bytes: *max_target_size_bytes,
            },
            WatcherTriggerReason::DiskFreeBelow {
                free_basis_points,
                threshold_basis_points,
            } => Self::DiskFreeBelow {
                free_basis_points: *free_basis_points,
                threshold_basis_points: *threshold_basis_points,
            },
            WatcherTriggerReason::DiskFreeBytesBelow {
                free_bytes,
                min_free_disk_bytes,
            } => Self::DiskFreeBytesBelow {
                free_bytes: *free_bytes,
                min_free_disk_bytes: *min_free_disk_bytes,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundPlanSummary {
    pub plan_id: Option<String>,
    pub policy: String,
    pub totals: BackgroundPlanTotals,
}

impl BackgroundPlanSummary {
    pub fn from_plan(policy: PolicyKind, plan: &Plan) -> Self {
        Self::from_totals(policy, None, plan.totals)
    }

    pub fn from_plan_id_and_totals(
        policy: PolicyKind,
        plan_id: &PlanId,
        totals: PlanTotals,
    ) -> Self {
        Self::from_totals(policy, Some(plan_id.as_str().to_owned()), totals)
    }

    fn from_totals(policy: PolicyKind, plan_id: Option<String>, totals: PlanTotals) -> Self {
        Self {
            plan_id,
            policy: policy_label(policy).to_owned(),
            totals: BackgroundPlanTotals::from_plan_totals(totals),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundPlanTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub preserved_count: usize,
    pub delete_candidate_count: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped_path_count: usize,
}

impl BackgroundPlanTotals {
    fn from_plan_totals(totals: PlanTotals) -> Self {
        Self {
            entry_count: totals.entry_count,
            total_bytes: totals.total_bytes,
            preserved_count: totals.preserved_count,
            delete_candidate_count: totals.delete_candidate_count,
            skipped_path_count: totals.skipped_path_count,
        }
    }
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundSkippedProject {
    pub path: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl BackgroundSkippedProject {
    fn from_skip(skip: &PlanSkip) -> Self {
        Self {
            path: skip.path.display().to_string(),
            reason: skip.reason.label().to_owned(),
            message: skip.message.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundApplySummary {
    pub plan_id: String,
    pub dry_run: bool,
    pub totals: BackgroundApplyTotals,
    pub notable_entries: Vec<BackgroundApplyEntrySummary>,
}

impl BackgroundApplySummary {
    pub fn from_apply_report(report: &ApplyReport) -> Self {
        Self {
            plan_id: report.plan_id.as_str().to_owned(),
            dry_run: report.dry_run,
            totals: BackgroundApplyTotals::from_apply_totals(&report.totals),
            notable_entries: report
                .entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.status,
                        ApplyEntryStatus::Deleted
                            | ApplyEntryStatus::DeleteFailed
                            | ApplyEntryStatus::SkipStalePlan
                    )
                })
                .map(BackgroundApplyEntrySummary::from_apply_entry_result)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundApplyTotals {
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

impl BackgroundApplyTotals {
    fn from_apply_totals(totals: &ApplyTotals) -> Self {
        Self {
            entry_count: totals.entry_count,
            delete_candidate_count: totals.delete_candidate_count,
            would_delete_count: totals.would_delete_count,
            skipped_count: totals.skipped_count,
            stale_skip_count: totals.stale_skip_count,
            applied_count: totals.applied_count,
            failed_count: totals.failed_count,
            would_delete_bytes: totals.would_delete_bytes,
            applied_bytes: totals.applied_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundApplyEntrySummary {
    pub path: String,
    pub planned_action: String,
    pub status: String,
    pub reason: String,
    pub size_bytes: u64,
    pub deleted_bytes: Option<u64>,
}

impl BackgroundApplyEntrySummary {
    fn from_apply_entry_result(entry: &ApplyEntryResult) -> Self {
        Self {
            path: entry.path.clone(),
            planned_action: entry.planned_action.clone(),
            status: apply_entry_status_label(entry.status).to_owned(),
            reason: entry.reason.clone(),
            size_bytes: entry.size_bytes,
            deleted_bytes: entry.deleted_bytes,
        }
    }
}

pub fn append_background_run_log_record(
    path: impl AsRef<Path>,
    record: &BackgroundRunLogRecord,
) -> BackgroundRunLogResult<()> {
    let path = path.as_ref();

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| BackgroundRunLogError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| BackgroundRunLogError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    serde_json::to_writer(&mut file, record).map_err(|source| {
        BackgroundRunLogError::Serialize {
            path: path.to_path_buf(),
            source,
        }
    })?;
    file.write_all(b"\n")
        .and_then(|_| file.flush())
        .map_err(|source| BackgroundRunLogError::Io {
            path: path.to_path_buf(),
            source,
        })
}

pub fn read_background_run_log(
    path: impl AsRef<Path>,
) -> BackgroundRunLogResult<Vec<BackgroundRunLogRecord>> {
    let path = path.as_ref();
    let file =
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|source| BackgroundRunLogError::Io {
                path: path.to_path_buf(),
                source,
            })?;

    let mut records = Vec::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|source| BackgroundRunLogError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let record: BackgroundRunLogRecord =
            serde_json::from_str(&line).map_err(|source| BackgroundRunLogError::Json {
                path: path.to_path_buf(),
                line: line_number,
                source,
            })?;

        if record.schema_version != BACKGROUND_RUN_LOG_SCHEMA_VERSION {
            return Err(BackgroundRunLogError::UnsupportedSchemaVersion {
                path: path.to_path_buf(),
                line: line_number,
                found: record.schema_version,
                expected: BACKGROUND_RUN_LOG_SCHEMA_VERSION,
            });
        }

        records.push(record);
    }

    Ok(records)
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

fn watcher_decision_state_label(state: WatcherDecisionState) -> &'static str {
    match state {
        WatcherDecisionState::Inactive => "inactive",
        WatcherDecisionState::NonThresholdMode => "non_threshold_mode",
        WatcherDecisionState::NotTriggered => "not_triggered",
        WatcherDecisionState::TriggeredPlanOnly => "triggered_plan_only",
        WatcherDecisionState::TriggeredPlanAndApply => "triggered_plan_and_apply",
    }
}

fn apply_entry_status_label(status: ApplyEntryStatus) -> &'static str {
    match status {
        ApplyEntryStatus::WouldDelete => "would_delete",
        ApplyEntryStatus::Deleted => "deleted",
        ApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        ApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
        ApplyEntryStatus::DeleteFailed => "delete_failed",
    }
}
