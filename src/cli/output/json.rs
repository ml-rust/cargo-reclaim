use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    PathSnapshot, Plan, PlanEntry, PlanSkip, PlanTotals, PolicyKind, TargetEvidence,
};
use serde::Serialize;

use super::labels::{
    action_label, artifact_label, evidence_kind_label, path_kind_label, policy_label,
    skip_reason_label,
};
use crate::cli::{CliError, PlanMode};

pub(super) fn write_plan(
    output: &mut impl Write,
    plan: &Plan,
    policy: PolicyKind,
    mode: PlanMode,
) -> Result<(), CliError> {
    let document = JsonPlan::from_plan(plan, policy, mode);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonPlan {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    policy: &'static str,
    input: JsonInput,
    totals: JsonTotals,
    skipped_paths: Vec<JsonPlanSkip>,
    entries: Vec<JsonEntry>,
}

impl JsonPlan {
    fn from_plan(plan: &Plan, policy: PolicyKind, mode: PlanMode) -> Self {
        Self {
            schema_version: plan.schema_version,
            command: command_label(mode),
            dry_run: true,
            policy: policy_label(policy),
            input: JsonInput::from_roots(&plan.input.roots),
            totals: JsonTotals::from_totals(plan.totals),
            skipped_paths: plan
                .skipped_paths
                .iter()
                .map(JsonPlanSkip::from_skip)
                .collect(),
            entries: plan.entries.iter().map(JsonEntry::from_entry).collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonInput {
    roots: Vec<String>,
}

impl JsonInput {
    fn from_roots(roots: &[PathBuf]) -> Self {
        Self {
            roots: roots.iter().map(path_string).collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonTotals {
    entry_count: usize,
    total_bytes: u64,
    preserved_count: usize,
    delete_candidate_count: usize,
    skipped_path_count: usize,
}

impl JsonTotals {
    fn from_totals(totals: PlanTotals) -> Self {
        Self {
            entry_count: totals.entry_count,
            total_bytes: totals.total_bytes,
            preserved_count: totals.preserved_count,
            delete_candidate_count: totals.delete_candidate_count,
            skipped_path_count: totals.skipped_path_count,
        }
    }
}

#[derive(Serialize)]
struct JsonPlanSkip {
    path: String,
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl JsonPlanSkip {
    fn from_skip(skip: &PlanSkip) -> Self {
        Self {
            path: path_string(&skip.path),
            reason: skip_reason_label(skip.reason),
            message: skip.message.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonEntry {
    snapshot: JsonSnapshot,
    artifact_class: &'static str,
    evidence: JsonEvidence,
    action: &'static str,
    policy_reason: String,
    requires_confirmation: bool,
}

impl JsonEntry {
    fn from_entry(entry: &PlanEntry) -> Self {
        Self {
            snapshot: JsonSnapshot::from_snapshot(&entry.snapshot),
            artifact_class: artifact_label(entry.artifact_class),
            evidence: JsonEvidence::from_evidence(&entry.evidence),
            action: action_label(&entry.action),
            policy_reason: entry.policy_reason.clone(),
            requires_confirmation: entry.requires_confirmation,
        }
    }
}

#[derive(Serialize)]
struct JsonSnapshot {
    path: String,
    size_bytes: u64,
    path_kind: &'static str,
    modified: Option<JsonModified>,
}

impl JsonSnapshot {
    fn from_snapshot(snapshot: &PathSnapshot) -> Self {
        Self {
            path: path_string(&snapshot.path),
            size_bytes: snapshot.size_bytes,
            path_kind: path_kind_label(snapshot.path_kind),
            modified: snapshot.modified.and_then(JsonModified::from_system_time),
        }
    }
}

#[derive(Serialize)]
struct JsonModified {
    unix_seconds: u64,
    nanoseconds: u32,
}

impl JsonModified {
    fn from_system_time(time: SystemTime) -> Option<Self> {
        let duration = time.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            unix_seconds: duration.as_secs(),
            nanoseconds: duration.subsec_nanos(),
        })
    }
}

#[derive(Serialize)]
struct JsonEvidence {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_manifest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_name: Option<String>,
}

impl JsonEvidence {
    fn from_evidence(evidence: &TargetEvidence) -> Self {
        match evidence {
            TargetEvidence::StrongMarker { marker } => Self {
                kind: evidence_kind_label(evidence),
                marker: Some(marker.clone()),
                source: None,
                project_manifest: None,
                matched_name: None,
            },
            TargetEvidence::ConfiguredPath { source } => Self {
                kind: evidence_kind_label(evidence),
                marker: None,
                source: Some(source.clone()),
                project_manifest: None,
                matched_name: None,
            },
            TargetEvidence::ProjectContext { project_manifest } => Self {
                kind: evidence_kind_label(evidence),
                marker: None,
                source: None,
                project_manifest: Some(path_string(project_manifest)),
                matched_name: None,
            },
            TargetEvidence::WeakNameOnly { matched_name } => Self {
                kind: evidence_kind_label(evidence),
                marker: None,
                source: None,
                project_manifest: None,
                matched_name: Some(matched_name.clone()),
            },
        }
    }
}

fn command_label(mode: PlanMode) -> &'static str {
    match mode {
        PlanMode::Scan => "scan",
        PlanMode::Plan => "plan",
    }
}

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}
