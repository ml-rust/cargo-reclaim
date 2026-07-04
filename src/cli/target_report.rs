use std::collections::HashSet;
use std::path::{Path, PathBuf};

use cargo_reclaim::{
    InventoryOptions, PathKind, ReclaimError, ScanItem, ScanSkipReason, ScannerOptions,
    TargetCandidate, TargetCandidateKind, TargetEvidence, scan_roots, snapshot_path_parallel,
};
use rayon::prelude::*;
use serde_json::json;

use super::CliError;

#[derive(Debug, Clone)]
pub(super) struct TargetsDiscovery {
    pub(super) roots: Vec<PathBuf>,
    pub(super) scanner_options: ScannerOptions,
    pub(super) inventory_options: InventoryOptions,
    pub(super) config_path: Option<PathBuf>,
    pub(super) config_version: Option<u16>,
}

impl TargetsDiscovery {
    pub(super) fn new(
        roots: Vec<PathBuf>,
        scanner_options: ScannerOptions,
        inventory_options: InventoryOptions,
        config_path: Option<PathBuf>,
        config_version: Option<u16>,
    ) -> Self {
        Self {
            roots,
            scanner_options,
            inventory_options,
            config_path,
            config_version,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TargetsReport {
    pub(super) roots: Vec<PathBuf>,
    pub(super) config_path: Option<PathBuf>,
    pub(super) config_version: Option<u16>,
    pub(super) total_size_bytes: u64,
    pub(super) targets: Vec<TargetListEntry>,
    pub(super) skipped_paths: Vec<TargetListSkip>,
    pub(super) problems: Vec<TargetListProblem>,
}

#[derive(Debug, Clone)]
pub(super) struct TargetListEntry {
    pub(super) path: PathBuf,
    pub(super) size_bytes: u64,
    pub(super) path_kind: PathKind,
    pub(super) evidence: TargetEvidence,
}

#[derive(Debug, Clone)]
pub(super) struct TargetListSkip {
    pub(super) path: PathBuf,
    pub(super) reason: ScanSkipReason,
    pub(super) message: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TargetListProblem {
    pub(super) path: PathBuf,
    pub(super) message: String,
}

pub(super) fn build_targets_report(command: &TargetsDiscovery) -> Result<TargetsReport, CliError> {
    let items = scan_roots(command.roots.iter().cloned(), &command.scanner_options)?;
    let mut seen_targets = HashSet::new();
    let mut candidates = Vec::new();
    let mut skipped_paths = Vec::new();

    for item in items {
        match item {
            ScanItem::TargetCandidate(candidate) => {
                if candidate.kind != TargetCandidateKind::CargoTargetDir {
                    continue;
                }
                if !is_cleanable_cargo_target(&candidate) {
                    continue;
                }
                if !seen_targets.insert(normalize_for_dedupe(&candidate.path)) {
                    continue;
                }
                candidates.push(candidate);
            }
            ScanItem::Skipped(skip) => skipped_paths.push(TargetListSkip {
                message: skip_message(&skip.reason),
                path: skip.path,
                reason: skip.reason,
            }),
            ScanItem::CargoProject(_) => {}
        }
    }

    let measured_targets: Vec<_> = candidates
        .into_par_iter()
        .map(|candidate| target_entry(candidate, &command.inventory_options))
        .collect();
    let mut targets = Vec::new();
    let mut problems = Vec::new();
    for measured in measured_targets {
        match measured {
            Ok(entry) => targets.push(entry),
            Err(problem) => problems.push(problem),
        }
    }

    targets.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    skipped_paths.sort_by(|left, right| left.path.cmp(&right.path));
    problems.sort_by(|left, right| left.path.cmp(&right.path));
    let total_size_bytes = targets.iter().fold(0_u64, |total, target| {
        total.saturating_add(target.size_bytes)
    });

    Ok(TargetsReport {
        roots: command.roots.clone(),
        config_path: command.config_path.clone(),
        config_version: command.config_version,
        total_size_bytes,
        targets,
        skipped_paths,
        problems,
    })
}

fn target_entry(
    candidate: TargetCandidate,
    inventory_options: &InventoryOptions,
) -> Result<TargetListEntry, TargetListProblem> {
    let snapshot = snapshot_path_parallel(&candidate.path, inventory_options).map_err(|error| {
        TargetListProblem {
            path: candidate.path.clone(),
            message: inventory_problem_message(error),
        }
    })?;

    let evidence = candidate.evidence.ok_or_else(|| TargetListProblem {
        path: candidate.path.clone(),
        message: "target candidate has no evidence".to_string(),
    })?;

    Ok(TargetListEntry {
        path: snapshot.path,
        size_bytes: snapshot.size_bytes,
        path_kind: snapshot.path_kind,
        evidence,
    })
}

pub(super) fn is_cleanable_cargo_target(candidate: &TargetCandidate) -> bool {
    match candidate.evidence.as_ref() {
        Some(TargetEvidence::ConfiguredPath { .. })
        | Some(TargetEvidence::ProjectContext { .. }) => true,
        Some(TargetEvidence::StrongMarker { .. }) | Some(TargetEvidence::WeakNameOnly { .. }) => {
            candidate
                .path
                .file_name()
                .is_some_and(|name| name == "target")
        }
        None => false,
    }
}

pub(super) fn target_json(target: &TargetListEntry) -> serde_json::Value {
    json!({
        "path": target.path,
        "size_bytes": target.size_bytes,
        "size": human_bytes(target.size_bytes),
        "path_kind": path_kind_label(target.path_kind),
        "evidence": evidence_json(&target.evidence),
    })
}

pub(super) fn skip_json(skip: &TargetListSkip) -> serde_json::Value {
    json!({
        "path": skip.path,
        "reason": skip_reason_label(&skip.reason),
        "message": skip.message,
    })
}

pub(super) fn problem_json(problem: &TargetListProblem) -> serde_json::Value {
    json!({
        "path": problem.path,
        "message": problem.message,
    })
}

pub(super) fn evidence_json(evidence: &TargetEvidence) -> serde_json::Value {
    match evidence {
        TargetEvidence::StrongMarker { marker } => json!({
            "kind": "strong_marker",
            "marker": marker,
        }),
        TargetEvidence::ConfiguredPath { source } => json!({
            "kind": "configured_path",
            "source": source,
        }),
        TargetEvidence::ProjectContext { project_manifest } => json!({
            "kind": "project_context",
            "project_manifest": project_manifest,
        }),
        TargetEvidence::WeakNameOnly { matched_name } => json!({
            "kind": "weak_name_only",
            "matched_name": matched_name,
        }),
    }
}

pub(super) fn evidence_label(evidence: &TargetEvidence) -> &'static str {
    match evidence {
        TargetEvidence::StrongMarker { .. } => "strong_marker",
        TargetEvidence::ConfiguredPath { .. } => "configured_path",
        TargetEvidence::ProjectContext { .. } => "project_context",
        TargetEvidence::WeakNameOnly { .. } => "weak_name_only",
    }
}

pub(super) fn path_kind_label(path_kind: PathKind) -> &'static str {
    match path_kind {
        PathKind::File => "file",
        PathKind::Directory => "directory",
        PathKind::Symlink => "symlink",
        PathKind::Unknown => "unknown",
    }
}

fn skip_reason_label(reason: &ScanSkipReason) -> &'static str {
    match reason {
        ScanSkipReason::DefaultIgnoredDir => "default_ignored_dir",
        ScanSkipReason::ConfiguredIgnoredPath => "configured_ignored_path",
        ScanSkipReason::SymlinkNotFollowed => "symlink_not_followed",
        ScanSkipReason::CrossFilesystem => "cross_filesystem",
        ScanSkipReason::WeakNameOnlySuppressed => "weak_name_only_suppressed",
        ScanSkipReason::AlreadyVisited => "already_visited",
        ScanSkipReason::CargoConfigUnsupported { .. } => "cargo_config_unsupported",
        ScanSkipReason::CargoConfigProblem { .. } => "cargo_config_problem",
        ScanSkipReason::ReadError { .. } => "read_error",
    }
}

fn skip_message(reason: &ScanSkipReason) -> Option<String> {
    match reason {
        ScanSkipReason::CargoConfigUnsupported { message }
        | ScanSkipReason::CargoConfigProblem { message }
        | ScanSkipReason::ReadError { message } => Some(message.clone()),
        _ => None,
    }
}

fn inventory_problem_message(error: ReclaimError) -> String {
    match error {
        ReclaimError::MissingInventoryPath { path } => {
            format!("target vanished during inventory: {}", path.display())
        }
        error => error.to_string(),
    }
}

pub(super) fn normalize_for_dedupe(path: &Path) -> PathBuf {
    path.components().collect()
}

pub(super) fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit_index = 0;
    let mut value = bytes as f64;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} B")
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit_index])
    } else {
        format!("{value:.2} {}", UNITS[unit_index])
    }
}
