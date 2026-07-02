use std::fs::{self, Metadata};
use std::path::Path;

use crate::persistence::{PersistedPathSnapshot, PersistedPlanEntry, PersistedTimestamp};

use super::report::{ApplyEntryResult, ApplyEntryStatus};

pub(super) fn revalidate_entry(entry: &PersistedPlanEntry) -> ApplyEntryResult {
    if entry.action != "delete" {
        return ApplyEntryResult::new(
            entry.snapshot.path.clone(),
            entry.action.clone(),
            ApplyEntryStatus::NotPlannedForDeletion,
            entry.snapshot.size_bytes,
            "entry is not planned for deletion",
        );
    }

    if entry.requires_confirmation {
        return ApplyEntryResult::new(
            entry.snapshot.path.clone(),
            entry.action.clone(),
            ApplyEntryStatus::NotPlannedForDeletion,
            entry.snapshot.size_bytes,
            "delete entry requires confirmation",
        );
    }

    match revalidate_snapshot(&entry.snapshot) {
        Ok(()) => ApplyEntryResult::new(
            entry.snapshot.path.clone(),
            entry.action.clone(),
            ApplyEntryStatus::WouldDelete,
            entry.snapshot.size_bytes,
            "delete candidate revalidated; no files were deleted",
        ),
        Err(reason) => ApplyEntryResult::new(
            entry.snapshot.path.clone(),
            entry.action.clone(),
            ApplyEntryStatus::SkipStalePlan,
            entry.snapshot.size_bytes,
            reason,
        ),
    }
}

fn revalidate_snapshot(snapshot: &PersistedPathSnapshot) -> Result<(), String> {
    let path = Path::new(&snapshot.path);
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "skip_stale_plan: failed to read path metadata for {}: {error}",
            snapshot.path
        )
    })?;

    if metadata.file_type().is_symlink() {
        return Err("skip_stale_plan: path is now a symlink".to_string());
    }

    let current_kind = path_kind(&metadata);
    if current_kind != snapshot.path_kind {
        return Err(format!(
            "skip_stale_plan: path kind changed from {} to {current_kind}",
            snapshot.path_kind
        ));
    }

    let current_size = measure_size(path, &metadata)?;
    if current_size != snapshot.size_bytes {
        return Err(format!(
            "skip_stale_plan: size changed from {} to {current_size}",
            snapshot.size_bytes
        ));
    }

    if let Some(expected_modified) = snapshot.modified {
        let current_modified = metadata.modified().ok().and_then(system_time_to_timestamp);
        if current_modified != Some(expected_modified) {
            return Err("skip_stale_plan: modification time changed".to_string());
        }
    }

    Ok(())
}

fn measure_size(path: &Path, metadata: &Metadata) -> Result<u64, String> {
    if metadata.is_file() {
        return Ok(metadata.len());
    }

    if metadata.is_dir() {
        let mut total = 0_u64;
        for entry in fs::read_dir(path).map_err(|error| {
            format!(
                "skip_stale_plan: failed to read directory {}: {error}",
                path.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "skip_stale_plan: failed to read directory entry in {}: {error}",
                    path.display()
                )
            })?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                format!(
                    "skip_stale_plan: failed to read child metadata under {}: {error}",
                    path.display()
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err("skip_stale_plan: directory now contains a symlink".to_string());
            }
            total = total.saturating_add(measure_size(&entry.path(), &metadata)?);
        }
        return Ok(total);
    }

    Ok(metadata.len())
}

fn path_kind(metadata: &Metadata) -> &'static str {
    if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else {
        "unknown"
    }
}

fn system_time_to_timestamp(time: std::time::SystemTime) -> Option<PersistedTimestamp> {
    PersistedTimestamp::from_system_time(time).ok()
}
