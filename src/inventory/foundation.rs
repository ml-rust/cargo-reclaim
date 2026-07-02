use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::classifier::classify_target_relative_path;
use crate::error::{ReclaimError, ReclaimResult};
use crate::model::TargetEvidence;
use crate::planner::{PlannerCandidate, TargetContext};

use super::snapshot::snapshot_target_relative_path_from_normalized_child;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InventoryOptions {
    pub follow_symlinks: bool,
    pub skipped_paths: Vec<PathBuf>,
}

pub fn planner_candidate_from_target_relative_path(
    target_root: impl AsRef<Path>,
    child_path: impl AsRef<Path>,
    evidence: TargetEvidence,
    options: &InventoryOptions,
) -> ReclaimResult<PlannerCandidate> {
    let child_path = normalize_target_relative_child(child_path.as_ref())?;
    let target_root = target_root.as_ref();
    let snapshot =
        snapshot_target_relative_path_from_normalized_child(target_root, &child_path, options)?;
    let artifact_class = classify_target_relative_path(&child_path);

    Ok(
        PlannerCandidate::new(snapshot, artifact_class, evidence.clone())
            .with_target_context(target_context_from_evidence(target_root, &evidence)),
    )
}

pub fn planner_candidate_from_target_relative_path_with_context(
    target_root: impl AsRef<Path>,
    child_path: impl AsRef<Path>,
    evidence: TargetEvidence,
    target_context: TargetContext,
    options: &InventoryOptions,
) -> ReclaimResult<PlannerCandidate> {
    let child_path = normalize_target_relative_child(child_path.as_ref())?;
    let snapshot = snapshot_target_relative_path_from_normalized_child(
        target_root.as_ref(),
        &child_path,
        options,
    )?;
    let artifact_class = classify_target_relative_path(&child_path);

    Ok(PlannerCandidate::new(snapshot, artifact_class, evidence)
        .with_target_context(target_context))
}

pub(super) fn normalize_target_relative_child(child_path: &Path) -> ReclaimResult<PathBuf> {
    if child_path.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    if child_path.is_absolute() {
        return Err(ReclaimError::AbsoluteInventoryChildPath {
            path: child_path.to_path_buf(),
        });
    }

    let mut normalized = PathBuf::new();
    for component in child_path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ReclaimError::InventoryPathEscape {
                    path: child_path.to_path_buf(),
                });
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    Ok(normalized)
}

pub(super) fn target_context_from_evidence(
    target_root: &Path,
    evidence: &TargetEvidence,
) -> TargetContext {
    let mut target_context = TargetContext::new(target_root);

    if let TargetEvidence::ProjectContext { project_manifest } = evidence
        && let Some(project_root) = project_manifest.parent()
    {
        target_context = target_context.with_project_root(project_root);
    }

    target_context
}

pub(super) fn is_configured_skipped(path: &Path, options: &InventoryOptions) -> bool {
    let Some(path) = real_path(path) else {
        return false;
    };
    options.skipped_paths.iter().any(|skipped| {
        real_path(skipped).is_some_and(|skipped| path == skipped || path.starts_with(skipped))
    })
}

pub(crate) fn real_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok().or_else(|| {
        let parent = path.parent()?;
        let file_name = path.file_name()?;
        Some(fs::canonicalize(parent).ok()?.join(file_name))
    })
}
