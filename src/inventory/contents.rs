use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::classifier::classify_target_relative_path;
use crate::error::{ReclaimError, ReclaimResult};
use crate::model::{ArtifactClass, TargetEvidence};
use crate::planner::{PlannerCandidate, TargetContext};

use super::foundation::{
    InventoryOptions, is_configured_skipped,
    planner_candidate_from_target_relative_path_with_context, target_context_from_evidence,
};

pub fn planner_candidates_from_target_root(
    target_root: impl AsRef<Path>,
    evidence: TargetEvidence,
    options: &InventoryOptions,
) -> ReclaimResult<Vec<PlannerCandidate>> {
    let target_root = target_root.as_ref();
    let target_context = target_context_from_evidence(target_root, &evidence);
    planner_candidates_from_target_root_with_context(target_root, evidence, target_context, options)
}

pub fn planner_candidates_from_target_root_with_context(
    target_root: impl AsRef<Path>,
    evidence: TargetEvidence,
    target_context: TargetContext,
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
            &target_context,
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
    target_context: &TargetContext,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    candidates: &mut Vec<PlannerCandidate>,
) -> ReclaimResult<()> {
    let full_path = target_root.join(&child_path);
    if is_configured_skipped(&full_path, options) {
        return Ok(());
    }

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
    if !options.deep_target_scan && metadata.is_dir() && should_emit_shallow_candidate(&child_path)
    {
        candidates.push(planner_candidate_from_target_relative_path_with_context(
            target_root,
            child_path,
            evidence.clone(),
            target_context.clone(),
            options,
        )?);
        return Ok(());
    }

    if metadata.is_file()
        || (artifact_class != ArtifactClass::Unknown && artifact_class != ArtifactClass::Deps)
    {
        candidates.push(planner_candidate_from_target_relative_path_with_context(
            target_root,
            child_path,
            evidence.clone(),
            target_context.clone(),
            options,
        )?);
        return Ok(());
    }

    if metadata.is_dir() {
        if !options.deep_target_scan && !should_descend_shallow(&child_path) {
            return Ok(());
        }

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
                target_context,
                options,
                visited_dirs,
                candidates,
            )?;
        }

        if candidates.len() == candidate_count_before {
            candidates.push(planner_candidate_from_target_relative_path_with_context(
                target_root,
                child_path,
                evidence.clone(),
                target_context.clone(),
                options,
            )?);
        }
    }

    Ok(())
}

fn should_emit_shallow_candidate(child_path: &Path) -> bool {
    classify_target_relative_path(child_path) != ArtifactClass::Unknown
}

fn should_descend_shallow(child_path: &Path) -> bool {
    let components = child_path.components().count();
    if components == 0 || components >= 3 {
        return false;
    }

    child_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_routing_directory_name)
}

fn is_routing_directory_name(name: &str) -> bool {
    is_profile_root_name(name) || is_target_triple_name(name)
}

fn is_profile_root_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('.') && name != "target"
}

fn is_target_triple_name(name: &str) -> bool {
    name.matches('-').count() >= 2
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
