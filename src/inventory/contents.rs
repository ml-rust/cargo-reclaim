use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::classifier::classify_target_relative_path;
use crate::error::{ReclaimError, ReclaimResult};
use crate::model::{ArtifactClass, TargetEvidence};
use crate::planner::PlannerCandidate;

use super::foundation::{InventoryOptions, planner_candidate_from_target_relative_path};

pub fn planner_candidates_from_target_root(
    target_root: impl AsRef<Path>,
    evidence: TargetEvidence,
    options: &InventoryOptions,
) -> ReclaimResult<Vec<PlannerCandidate>> {
    let target_root = target_root.as_ref();
    if target_root.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }
    let metadata = fs::symlink_metadata(target_root)
        .map_err(|error| inventory_read_error(target_root, error))?;
    if metadata.file_type().is_symlink() && !options.follow_symlinks {
        return Err(ReclaimError::InventorySymlinkNotFollowed {
            path: target_root.to_path_buf(),
        });
    }

    let mut candidates = Vec::new();
    let mut visited_dirs = HashSet::new();
    for child in sorted_children(target_root)? {
        let Some(child_name) = child.file_name() else {
            continue;
        };
        collect_child_candidates(
            target_root,
            PathBuf::from(child_name),
            &evidence,
            options,
            &mut visited_dirs,
            &mut candidates,
        )?;
    }

    Ok(candidates)
}

fn collect_child_candidates(
    target_root: &Path,
    child_path: PathBuf,
    evidence: &TargetEvidence,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    candidates: &mut Vec<PlannerCandidate>,
) -> ReclaimResult<()> {
    let full_path = target_root.join(&child_path);
    let symlink_metadata = fs::symlink_metadata(&full_path)
        .map_err(|error| inventory_read_error(&full_path, error))?;

    if symlink_metadata.file_type().is_symlink() && !options.follow_symlinks {
        return Ok(());
    }

    let metadata = if symlink_metadata.file_type().is_symlink() {
        fs::metadata(&full_path).map_err(|error| inventory_read_error(&full_path, error))?
    } else {
        symlink_metadata
    };

    let artifact_class = classify_target_relative_path(&child_path);
    if metadata.is_file() || artifact_class != ArtifactClass::Unknown {
        candidates.push(planner_candidate_from_target_relative_path(
            target_root,
            child_path,
            evidence.clone(),
            options,
        )?);
        return Ok(());
    }

    if metadata.is_dir() {
        let canonical_path = fs::canonicalize(&full_path)
            .map_err(|error| inventory_read_error(&full_path, error))?;
        if !visited_dirs.insert(canonical_path) {
            return Ok(());
        }

        let candidate_count_before = candidates.len();
        for child in sorted_children(&full_path)? {
            let Some(file_name) = child.file_name() else {
                continue;
            };
            collect_child_candidates(
                target_root,
                child_path.join(file_name),
                evidence,
                options,
                visited_dirs,
                candidates,
            )?;
        }

        if candidates.len() == candidate_count_before {
            candidates.push(planner_candidate_from_target_relative_path(
                target_root,
                child_path,
                evidence.clone(),
                options,
            )?);
        }
    }

    Ok(())
}

fn sorted_children(path: &Path) -> ReclaimResult<Vec<PathBuf>> {
    let entries = fs::read_dir(path).map_err(|error| inventory_read_error(path, error))?;
    let mut children = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| inventory_read_error(path, error))?;
        children.push(entry.path());
    }

    children.sort();
    Ok(children)
}

fn inventory_read_error(path: &Path, error: io::Error) -> ReclaimError {
    if error.kind() == io::ErrorKind::NotFound {
        ReclaimError::MissingInventoryPath {
            path: path.to_path_buf(),
        }
    } else {
        ReclaimError::InventoryRead {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    }
}
