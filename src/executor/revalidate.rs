use std::fs::{self, Metadata};
use std::path::{Component, Path, PathBuf};

use crate::persistence::{
    PersistedPathSnapshot, PersistedPlanEntry, PersistedTimestamp, fingerprint_path,
};

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

    match revalidate_entry_snapshot(entry) {
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

pub(super) fn delete_revalidated_entry(entry: ApplyEntryResult) -> ApplyEntryResult {
    if entry.status != ApplyEntryStatus::WouldDelete {
        return entry;
    }

    match remove_path(Path::new(&entry.path)) {
        Ok(()) => ApplyEntryResult::new(
            entry.path,
            entry.planned_action,
            ApplyEntryStatus::Deleted,
            entry.size_bytes,
            "deleted revalidated path",
        ),
        Err(reason) => ApplyEntryResult::new(
            entry.path,
            entry.planned_action,
            ApplyEntryStatus::DeleteFailed,
            entry.size_bytes,
            reason,
        ),
    }
}

fn revalidate_entry_snapshot(entry: &PersistedPlanEntry) -> Result<(), String> {
    revalidate_snapshot(&entry.snapshot)?;

    if entry.artifact_class == "whole_target" {
        revalidate_whole_target(entry)?;
    } else {
        revalidate_content_fingerprint(&entry.snapshot)?;
    }

    Ok(())
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

    if snapshot.path_kind != "directory" || snapshot.content_fingerprint.is_some() {
        let current_size = measure_size(path, &metadata, snapshot.content_fingerprint.is_some())?;
        if current_size != snapshot.size_bytes {
            return Err(format!(
                "skip_stale_plan: size changed from {} to {current_size}",
                snapshot.size_bytes
            ));
        }
    }

    if let Some(expected_modified) = snapshot.modified {
        let current_modified = metadata.modified().ok().and_then(system_time_to_timestamp);
        if current_modified != Some(expected_modified) {
            return Err("skip_stale_plan: modification time changed".to_string());
        }
    }

    Ok(())
}

fn revalidate_content_fingerprint(snapshot: &PersistedPathSnapshot) -> Result<(), String> {
    let Some(expected_fingerprint) = snapshot.content_fingerprint.as_ref() else {
        return if snapshot.path_kind == "directory" {
            Ok(())
        } else {
            Err("skip_stale_plan: persisted content fingerprint is missing".to_string())
        };
    };

    let path = Path::new(&snapshot.path);
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "skip_stale_plan: failed to read path metadata for {}: {error}",
            snapshot.path
        )
    })?;
    let current_fingerprint = fingerprint_path(path, &metadata)
        .map_err(|error| format!("skip_stale_plan: failed to fingerprint path: {error}"))?;
    if current_fingerprint != *expected_fingerprint {
        return Err("skip_stale_plan: content fingerprint changed".to_string());
    }

    Ok(())
}

fn revalidate_whole_target(entry: &PersistedPlanEntry) -> Result<(), String> {
    let path = Path::new(&entry.snapshot.path);
    if entry.snapshot.path_kind != "directory" {
        return Err("skip_stale_plan: whole-target entry is not a directory snapshot".to_string());
    }

    match entry.evidence.kind.as_str() {
        "strong_marker" => {
            let marker = entry.evidence.marker.as_deref().ok_or_else(|| {
                "skip_stale_plan: whole-target marker evidence is missing".to_string()
            })?;
            let marker_path = path.join(marker);
            let metadata = fs::symlink_metadata(&marker_path).map_err(|error| {
                format!(
                    "skip_stale_plan: whole-target marker {} is missing or unreadable: {error}",
                    marker_path.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(
                    "skip_stale_plan: whole-target marker is no longer a regular file".to_string(),
                );
            }
        }
        "project_context" => {
            let manifest = entry.evidence.project_manifest.as_deref().ok_or_else(|| {
                "skip_stale_plan: whole-target project context evidence is missing".to_string()
            })?;
            let manifest_path = Path::new(manifest);
            let metadata = fs::symlink_metadata(manifest_path).map_err(|error| {
                format!(
                    "skip_stale_plan: project manifest {} is missing or unreadable: {error}",
                    manifest_path.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(
                    "skip_stale_plan: project manifest is no longer a regular file".to_string(),
                );
            }
            let Some(project_root) = manifest_path.parent() else {
                return Err("skip_stale_plan: project manifest has no parent directory".to_string());
            };
            let expected_target = lexically_normalize(project_root.join("target"));
            let observed_target = lexically_normalize(path);
            if observed_target != expected_target {
                return Err(format!(
                    "skip_stale_plan: whole-target path {} no longer matches project target {}",
                    observed_target.display(),
                    expected_target.display()
                ));
            }
        }
        "configured_path" => {
            if entry.evidence.source.as_deref().is_none_or(str::is_empty) {
                return Err(
                    "skip_stale_plan: whole-target configured-path evidence is missing".to_string(),
                );
            }
        }
        "weak_name_only" => {
            let matched_name = entry.evidence.matched_name.as_deref().ok_or_else(|| {
                "skip_stale_plan: whole-target weak-name evidence is missing".to_string()
            })?;
            if path.file_name().and_then(|name| name.to_str()) != Some(matched_name) {
                return Err(format!(
                    "skip_stale_plan: whole-target basename no longer matches `{matched_name}`"
                ));
            }
        }
        value => {
            return Err(format!(
                "skip_stale_plan: whole-target evidence kind `{value}` is unsupported"
            ));
        }
    }

    Ok(())
}

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("delete_failed: failed to read path metadata: {error}"))?;

    if metadata.file_type().is_symlink() {
        return Err("delete_failed: path is now a symlink".to_string());
    }

    if metadata.is_file() {
        return fs::remove_file(path)
            .map_err(|error| format!("delete_failed: failed to remove file: {error}"));
    }

    if metadata.is_dir() {
        return fs::remove_dir_all(path)
            .map_err(|error| format!("delete_failed: failed to remove directory: {error}"));
    }

    Err("delete_failed: path kind is not removable".to_string())
}

fn measure_size(
    path: &Path,
    metadata: &Metadata,
    recurse_directories: bool,
) -> Result<u64, String> {
    if metadata.is_file() {
        return Ok(metadata.len());
    }

    if metadata.is_dir() {
        if !recurse_directories {
            return Ok(metadata.len());
        }

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
            total = total.saturating_add(measure_size(&entry.path(), &metadata, true)?);
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

fn lexically_normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
