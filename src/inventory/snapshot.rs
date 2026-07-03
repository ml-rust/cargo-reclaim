use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{ReclaimError, ReclaimResult};
use crate::model::{PathKind, PathSnapshot};

use super::foundation::{InventoryOptions, is_configured_skipped, normalize_target_relative_child};

pub fn snapshot_path(
    path: impl AsRef<Path>,
    options: &InventoryOptions,
) -> ReclaimResult<PathSnapshot> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    let mut visited_dirs = HashSet::new();
    let measured = measure_path(path, options, &mut visited_dirs, true)?;

    PathSnapshot::with_details(
        path.to_path_buf(),
        measured.size_bytes,
        measured.path_kind,
        measured.modified,
    )
}

pub fn snapshot_target_relative_path(
    target_root: impl AsRef<Path>,
    child_path: impl AsRef<Path>,
    options: &InventoryOptions,
) -> ReclaimResult<PathSnapshot> {
    let target_root = target_root.as_ref();
    if target_root.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    let child_path = normalize_target_relative_child(child_path.as_ref())?;
    snapshot_target_relative_path_from_normalized_child(target_root, &child_path, options)
}

pub(super) fn snapshot_target_relative_path_from_normalized_child(
    target_root: impl AsRef<Path>,
    child_path: &Path,
    options: &InventoryOptions,
) -> ReclaimResult<PathSnapshot> {
    let target_root = target_root.as_ref();
    if target_root.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    let full_path = target_root.join(child_path);
    let mut visited_dirs = HashSet::new();
    let measured = measure_path(
        &full_path,
        options,
        &mut visited_dirs,
        options.deep_directory_measurement,
    )?;

    PathSnapshot::with_details(
        full_path,
        measured.size_bytes,
        measured.path_kind,
        measured.modified,
    )
}

struct MeasuredPath {
    size_bytes: u64,
    path_kind: PathKind,
    modified: Option<std::time::SystemTime>,
}

fn measure_path(
    path: &Path,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    deep_directory_measurement: bool,
) -> ReclaimResult<MeasuredPath> {
    measure_path_with_symlink_policy(
        path,
        options,
        visited_dirs,
        deep_directory_measurement,
        true,
    )
}

fn measure_path_with_symlink_policy(
    path: &Path,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    deep_directory_measurement: bool,
    reject_unfollowed_symlink: bool,
) -> ReclaimResult<MeasuredPath> {
    let metadata = symlink_metadata(path)?;

    if metadata.file_type().is_symlink() {
        if !options.follow_symlinks {
            if reject_unfollowed_symlink {
                return Err(ReclaimError::InventorySymlinkNotFollowed {
                    path: path.to_path_buf(),
                });
            }
            return Ok(MeasuredPath {
                size_bytes: metadata.len(),
                path_kind: PathKind::Symlink,
                modified: metadata.modified().ok(),
            });
        }

        return measure_followed_path(path, options, visited_dirs, deep_directory_measurement);
    }

    measure_from_metadata(
        path,
        metadata,
        options,
        visited_dirs,
        deep_directory_measurement,
    )
}

fn measure_followed_path(
    path: &Path,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    deep_directory_measurement: bool,
) -> ReclaimResult<MeasuredPath> {
    let metadata = fs::metadata(path).map_err(|error| inventory_read_error(path, error))?;
    measure_from_metadata(
        path,
        metadata,
        options,
        visited_dirs,
        deep_directory_measurement,
    )
}

fn measure_from_metadata(
    path: &Path,
    metadata: Metadata,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    deep_directory_measurement: bool,
) -> ReclaimResult<MeasuredPath> {
    let modified = metadata.modified().ok();

    if metadata.is_file() {
        return Ok(MeasuredPath {
            size_bytes: metadata.len(),
            path_kind: PathKind::File,
            modified,
        });
    }

    if metadata.is_dir() {
        let size_bytes = if deep_directory_measurement {
            measure_directory(path, options, visited_dirs)?
        } else {
            metadata.len()
        };
        return Ok(MeasuredPath {
            size_bytes,
            path_kind: PathKind::Directory,
            modified,
        });
    }

    Ok(MeasuredPath {
        size_bytes: metadata.len(),
        path_kind: PathKind::Unknown,
        modified,
    })
}

fn measure_directory(
    path: &Path,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
) -> ReclaimResult<u64> {
    let canonical_path =
        fs::canonicalize(path).map_err(|error| inventory_read_error(path, error))?;
    if !visited_dirs.insert(canonical_path) {
        return Ok(0);
    }

    let mut size_bytes = 0_u64;
    for entry in fs::read_dir(path).map_err(|error| inventory_read_error(path, error))? {
        let entry = entry.map_err(|error| inventory_read_error(path, error))?;
        if is_configured_skipped(&entry.path(), options) {
            continue;
        }
        let measured =
            measure_path_with_symlink_policy(&entry.path(), options, visited_dirs, true, false)?;
        size_bytes = size_bytes.saturating_add(measured.size_bytes);
    }

    Ok(size_bytes)
}

fn symlink_metadata(path: &Path) -> ReclaimResult<Metadata> {
    fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            ReclaimError::MissingInventoryPath {
                path: path.to_path_buf(),
            }
        } else {
            inventory_read_error(path, error)
        }
    })
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
