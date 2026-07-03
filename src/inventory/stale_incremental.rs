use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::{ReclaimError, ReclaimResult};
use crate::model::{ArtifactClass, TargetEvidence};
use crate::planner::{PlannerCandidate, TargetContext};

use super::foundation::{InventoryOptions, is_configured_skipped};
use super::snapshot_target_relative_path;

pub(crate) fn append_stale_incremental_candidates(
    target_root: &Path,
    evidence: &TargetEvidence,
    target_context: &TargetContext,
    options: &InventoryOptions,
    candidates: &mut Vec<PlannerCandidate>,
) -> ReclaimResult<()> {
    let stale_paths = collect_stale_incremental_paths(target_root, options)?;
    if stale_paths.is_empty() {
        return Ok(());
    }

    let stale_full_paths = stale_paths
        .iter()
        .map(|path| target_root.join(path))
        .collect::<Vec<_>>();
    candidates.retain(|candidate| {
        candidate.artifact_class != ArtifactClass::Incremental
            || stale_full_paths
                .iter()
                .all(|stale_path| !stale_path.starts_with(&candidate.snapshot.path))
    });

    let mut existing_paths = candidates
        .iter()
        .map(|candidate| candidate.snapshot.path.clone())
        .collect::<HashSet<_>>();

    for child_path in stale_paths {
        let full_path = target_root.join(&child_path);
        if existing_paths.contains(&full_path) {
            continue;
        }

        let snapshot = match snapshot_target_relative_path(target_root, &child_path, options) {
            Ok(snapshot) => snapshot,
            Err(ReclaimError::MissingInventoryPath { .. }) => continue,
            Err(error) => return Err(error),
        };
        let candidate =
            PlannerCandidate::new(snapshot, ArtifactClass::StaleIncremental, evidence.clone())
                .with_target_context(target_context.clone());
        existing_paths.insert(candidate.snapshot.path.clone());
        candidates.push(candidate);
    }

    Ok(())
}

fn collect_stale_incremental_paths(
    target_root: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Vec<PathBuf>> {
    let mut incremental_dirs = Vec::new();
    let mut visited_dirs = HashSet::new();
    collect_incremental_dirs(
        target_root,
        PathBuf::new(),
        options,
        &mut visited_dirs,
        &mut incremental_dirs,
    )?;

    let mut stale_paths = Vec::new();
    for incremental_dir in incremental_dirs {
        stale_paths.extend(stale_session_paths_from_incremental_dir(
            target_root,
            &incremental_dir,
            options,
        )?);
    }
    stale_paths.sort();
    Ok(stale_paths)
}

fn collect_incremental_dirs(
    target_root: &Path,
    child_path: PathBuf,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    incremental_dirs: &mut Vec<PathBuf>,
) -> ReclaimResult<()> {
    let full_path = target_root.join(&child_path);
    if is_configured_skipped(&full_path, options) {
        return Ok(());
    }

    let symlink_metadata = match fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(inventory_read_error(&full_path, error)),
    };
    if symlink_metadata.file_type().is_symlink() && !options.follow_symlinks {
        return Ok(());
    }

    let metadata = if symlink_metadata.file_type().is_symlink() {
        fs::metadata(&full_path).map_err(|error| inventory_read_error(&full_path, error))?
    } else {
        symlink_metadata
    };

    if !metadata.is_dir() {
        return Ok(());
    }

    if is_incremental_dir_path(&child_path) {
        incremental_dirs.push(child_path);
        return Ok(());
    }

    let canonical_path =
        fs::canonicalize(&full_path).map_err(|error| inventory_read_error(&full_path, error))?;
    if !visited_dirs.insert(canonical_path) {
        return Ok(());
    }

    for child in sorted_children(&full_path)? {
        let Some(file_name) = child.file_name() else {
            continue;
        };
        collect_incremental_dirs(
            target_root,
            child_path.join(file_name),
            options,
            visited_dirs,
            incremental_dirs,
        )?;
    }

    Ok(())
}

fn stale_session_paths_from_incremental_dir(
    target_root: &Path,
    incremental_dir: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Vec<PathBuf>> {
    let full_incremental_dir = target_root.join(incremental_dir);
    let mut units_by_family = HashMap::<String, Vec<UnitVariant>>::new();
    let mut stale_paths = Vec::new();

    for unit_dir in sorted_children(&full_incremental_dir)? {
        let Some(unit_name) = unit_dir.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let unit_path = incremental_dir.join(unit_name);
        let full_unit_path = target_root.join(&unit_path);
        if is_configured_skipped(&full_unit_path, options)
            || !is_directory(&full_unit_path, options)?
        {
            continue;
        }

        let Some(newest_modified) = valid_unit_newest_modified(target_root, &unit_path, options)?
        else {
            continue;
        };
        let Some(family) = unit_family(unit_name) else {
            continue;
        };
        units_by_family
            .entry(family.to_string())
            .or_default()
            .push(UnitVariant {
                path: unit_path,
                newest_modified,
            });
    }

    let stale_unit_paths = stale_unit_variant_paths(units_by_family);
    let stale_unit_set = stale_unit_paths.iter().cloned().collect::<HashSet<_>>();
    stale_paths.extend(stale_unit_paths);

    for unit_dir in sorted_children(&full_incremental_dir)? {
        let Some(unit_name) = unit_dir.file_name() else {
            continue;
        };
        let unit_path = incremental_dir.join(unit_name);
        let full_unit_path = target_root.join(&unit_path);
        if is_configured_skipped(&full_unit_path, options)
            || !is_directory(&full_unit_path, options)?
        {
            continue;
        }
        if stale_unit_set.contains(&unit_path) {
            continue;
        }
        stale_paths.extend(stale_sessions_from_unit_dir(
            target_root,
            &unit_path,
            options,
        )?);
    }

    Ok(stale_paths)
}

#[derive(Debug)]
struct UnitVariant {
    path: PathBuf,
    newest_modified: SystemTime,
}

fn stale_unit_variant_paths(units_by_family: HashMap<String, Vec<UnitVariant>>) -> Vec<PathBuf> {
    units_by_family
        .into_values()
        .filter(|variants| variants.len() > 1)
        .flat_map(|variants| {
            let newest_modified = variants
                .iter()
                .map(|variant| variant.newest_modified)
                .max()
                .expect("non-empty variants");
            variants.into_iter().filter_map(move |variant| {
                (variant.newest_modified < newest_modified).then_some(variant.path)
            })
        })
        .collect()
}

fn valid_unit_newest_modified(
    target_root: &Path,
    unit_path: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Option<SystemTime>> {
    let full_unit_path = target_root.join(unit_path);
    let mut newest_modified = None::<SystemTime>;

    for child in sorted_children(&full_unit_path)? {
        let Some(file_name) = child.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !file_name.starts_with("s-") {
            continue;
        }
        if !is_directory(&child, options)? {
            continue;
        }

        let Some(modified) = valid_session_modified(&child, options)? else {
            continue;
        };
        newest_modified = Some(newest_modified.map_or(modified, |current| current.max(modified)));
    }

    Ok(newest_modified)
}

fn stale_sessions_from_unit_dir(
    target_root: &Path,
    unit_path: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Vec<PathBuf>> {
    let full_unit_path = target_root.join(unit_path);
    let mut sessions = HashMap::<PathBuf, Option<SystemTime>>::new();

    for child in sorted_children(&full_unit_path)? {
        let Some(file_name) = child.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if !file_name.starts_with("s-") {
            continue;
        }

        let session_path = unit_path.join(file_name);
        let full_session_path = target_root.join(&session_path);
        if is_configured_skipped(&full_session_path, options) {
            continue;
        }
        if !is_directory(&full_session_path, options)? {
            continue;
        }

        let Some(modified) = valid_session_modified(&full_session_path, options)? else {
            continue;
        };
        sessions.insert(session_path, Some(modified));
    }

    if sessions.len() < 2 {
        return Ok(Vec::new());
    }

    let Some(newest_modified) = sessions.values().flatten().copied().max() else {
        return Ok(Vec::new());
    };

    Ok(sessions
        .into_iter()
        .filter_map(|(path, modified)| {
            modified
                .is_some_and(|modified| modified < newest_modified)
                .then_some(path)
        })
        .collect())
}

fn valid_session_modified(
    path: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Option<SystemTime>> {
    let Some(mut newest_modified) = directory_modified(path, options)? else {
        return Ok(None);
    };

    let mut has_marker = false;
    for marker in ["dep-graph.bin", "query-cache.bin", "work-products.bin"] {
        let marker_path = path.join(marker);
        let metadata = match fs::symlink_metadata(&marker_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(inventory_read_error(&marker_path, error)),
        };
        if !metadata.is_file() {
            continue;
        }
        has_marker = true;
        if let Ok(modified) = metadata.modified() {
            newest_modified = newest_modified.max(modified);
        }
    }

    Ok(has_marker.then_some(newest_modified))
}

fn directory_modified(
    path: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<Option<SystemTime>> {
    let symlink_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(inventory_read_error(path, error)),
    };
    if symlink_metadata.file_type().is_symlink() && !options.follow_symlinks {
        return Ok(None);
    }

    let metadata = if symlink_metadata.file_type().is_symlink() {
        fs::metadata(path).map_err(|error| inventory_read_error(path, error))?
    } else {
        symlink_metadata
    };

    Ok(metadata
        .is_dir()
        .then(|| metadata.modified().ok())
        .flatten())
}

fn is_directory(path: &Path, options: &InventoryOptions) -> ReclaimResult<bool> {
    Ok(directory_modified(path, options)?.is_some())
}

fn is_incremental_dir_path(path: &Path) -> bool {
    let components = normal_components(path);
    match components.as_slice() {
        [profile, incremental] => is_profile_root(profile) && *incremental == "incremental",
        [triple, profile, incremental] => {
            is_target_triple(triple) && is_profile_root(profile) && *incremental == "incremental"
        }
        _ => false,
    }
}

fn unit_family(unit_name: &str) -> Option<&str> {
    let (family, hash) = unit_name.rsplit_once('-')?;
    (!family.is_empty() && hash.len() >= 4 && hash.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then_some(family)
}

fn normal_components(path: &Path) -> Vec<&OsStr> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(component) => Some(component),
            _ => None,
        })
        .collect()
}

fn is_profile_root(component: &OsStr) -> bool {
    component
        .to_str()
        .is_some_and(|component| !component.is_empty() && !component.contains('.'))
}

fn is_target_triple(component: &OsStr) -> bool {
    component
        .to_str()
        .is_some_and(|component| component.matches('-').count() >= 2)
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
