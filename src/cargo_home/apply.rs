use std::fs::{self, Metadata};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::persistence::{PersistedTimestamp, PlanId, PlanPersistenceResult, fingerprint_path};

use super::classify::classify_cargo_home_relative_path;
use super::model::{CargoHomeClass, CargoHomePathKind};
use super::persistence::{
    PersistedCargoHomePlan, PersistedCargoHomePlanEntry, class_label,
    ensure_cargo_home_plan_usable, path_kind_label,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeApplyReport {
    pub plan_id: PlanId,
    pub dry_run: bool,
    pub validation_only: bool,
    pub entries: Vec<CargoHomeApplyEntryResult>,
    pub totals: CargoHomeApplyTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoHomeApplyTotals {
    pub entry_count: usize,
    pub delete_candidate_count: usize,
    pub would_delete_count: usize,
    pub would_delete_bytes: u64,
    pub applied_count: usize,
    pub applied_bytes: u64,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub stale_skip_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeApplyEntryResult {
    pub path: String,
    pub planned_action: String,
    pub status: CargoHomeApplyEntryStatus,
    pub size_bytes: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoHomeApplyEntryStatus {
    WouldDelete,
    Deleted,
    NotPlannedForDeletion,
    SkipStalePlan,
    DeleteFailed,
}

pub fn validate_cargo_home_plan_for_apply(
    document: &PersistedCargoHomePlan,
    now: SystemTime,
) -> PlanPersistenceResult<CargoHomeApplyReport> {
    ensure_cargo_home_plan_usable(document, now)?;
    let entries = collect_revalidated_entries(document);

    Ok(CargoHomeApplyReport {
        plan_id: document.id.clone(),
        dry_run: true,
        validation_only: true,
        totals: CargoHomeApplyTotals::from_entries(&entries),
        entries,
    })
}

pub fn execute_cargo_home_plan_apply(
    document: &PersistedCargoHomePlan,
    now: SystemTime,
) -> PlanPersistenceResult<CargoHomeApplyReport> {
    ensure_cargo_home_plan_usable(document, now)?;
    let entries = collect_deleted_entries(document);

    Ok(CargoHomeApplyReport {
        plan_id: document.id.clone(),
        dry_run: false,
        validation_only: false,
        totals: CargoHomeApplyTotals::from_entries(&entries),
        entries,
    })
}

impl CargoHomeApplyTotals {
    fn from_entries(entries: &[CargoHomeApplyEntryResult]) -> Self {
        let mut totals = Self {
            entry_count: entries.len(),
            ..Self::default()
        };

        for entry in entries {
            if entry.planned_action == "delete_candidate" {
                totals.delete_candidate_count += 1;
            }

            match entry.status {
                CargoHomeApplyEntryStatus::WouldDelete => {
                    totals.would_delete_count += 1;
                    totals.would_delete_bytes =
                        totals.would_delete_bytes.saturating_add(entry.size_bytes);
                }
                CargoHomeApplyEntryStatus::Deleted => {
                    totals.applied_count += 1;
                    totals.applied_bytes = totals.applied_bytes.saturating_add(entry.size_bytes);
                }
                CargoHomeApplyEntryStatus::NotPlannedForDeletion => totals.skipped_count += 1,
                CargoHomeApplyEntryStatus::SkipStalePlan => {
                    totals.skipped_count += 1;
                    totals.stale_skip_count += 1;
                }
                CargoHomeApplyEntryStatus::DeleteFailed => totals.failed_count += 1,
            }
        }

        totals
    }
}

fn collect_revalidated_entries(
    document: &PersistedCargoHomePlan,
) -> Vec<CargoHomeApplyEntryResult> {
    document
        .body
        .plan
        .entries
        .iter()
        .map(|entry| revalidate_entry(document, entry))
        .collect()
}

fn collect_deleted_entries(document: &PersistedCargoHomePlan) -> Vec<CargoHomeApplyEntryResult> {
    document
        .body
        .plan
        .entries
        .iter()
        .map(|entry| revalidate_entry(document, entry))
        .map(delete_revalidated_entry)
        .collect()
}

fn revalidate_entry(
    document: &PersistedCargoHomePlan,
    entry: &PersistedCargoHomePlanEntry,
) -> CargoHomeApplyEntryResult {
    if entry.action != "delete_candidate" {
        return result(
            entry,
            CargoHomeApplyEntryStatus::NotPlannedForDeletion,
            "entry is not planned for deletion",
        );
    }

    match revalidate_delete_candidate(document, entry) {
        Ok(()) => result(
            entry,
            CargoHomeApplyEntryStatus::WouldDelete,
            "delete candidate revalidated; no files were deleted",
        ),
        Err(reason) => result(entry, CargoHomeApplyEntryStatus::SkipStalePlan, reason),
    }
}

fn delete_revalidated_entry(entry: CargoHomeApplyEntryResult) -> CargoHomeApplyEntryResult {
    if entry.status != CargoHomeApplyEntryStatus::WouldDelete {
        return entry;
    }

    match remove_path(Path::new(&entry.path)) {
        Ok(()) => CargoHomeApplyEntryResult {
            status: CargoHomeApplyEntryStatus::Deleted,
            reason: "deleted revalidated path".to_string(),
            ..entry
        },
        Err(reason) => CargoHomeApplyEntryResult {
            status: CargoHomeApplyEntryStatus::DeleteFailed,
            reason,
            ..entry
        },
    }
}

fn revalidate_delete_candidate(
    document: &PersistedCargoHomePlan,
    entry: &PersistedCargoHomePlanEntry,
) -> Result<(), String> {
    let root = Path::new(&document.body.plan.input.root);
    let path = Path::new(&entry.path);
    let relative_path = Path::new(&entry.relative_path);
    let joined = root.join(relative_path);

    if !is_plain_relative_path(relative_path) {
        return Err(
            "skip_stale_plan: persisted relative path must stay inside Cargo home".to_string(),
        );
    }

    if path != joined {
        return Err("skip_stale_plan: path no longer matches root and relative path".to_string());
    }

    let root_metadata = fs::symlink_metadata(root).map_err(|error| {
        format!(
            "skip_stale_plan: failed to read persisted Cargo home root {}: {error}",
            root.display()
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(
            "skip_stale_plan: persisted Cargo home root is not a real directory".to_string(),
        );
    }

    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "skip_stale_plan: failed to read path metadata for {}: {error}",
            entry.path
        )
    })?;

    if metadata.file_type().is_symlink() {
        return Err("skip_stale_plan: path is now a symlink".to_string());
    }

    let canonical_root = root.canonicalize().map_err(|error| {
        format!(
            "skip_stale_plan: failed to resolve persisted Cargo home root {}: {error}",
            root.display()
        )
    })?;
    let canonical_path = path.canonicalize().map_err(|error| {
        format!(
            "skip_stale_plan: failed to resolve persisted path {}: {error}",
            entry.path
        )
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err("skip_stale_plan: path is outside persisted Cargo home root".to_string());
    }

    let current_kind = current_path_kind(&metadata);
    if current_kind != entry.path_kind {
        return Err(format!(
            "skip_stale_plan: path kind changed from {} to {current_kind}",
            entry.path_kind
        ));
    }

    let current_size = measure_size(path, &metadata)?;
    if current_size != entry.size_bytes {
        return Err(format!(
            "skip_stale_plan: size changed from {} to {current_size}",
            entry.size_bytes
        ));
    }

    let current_fingerprint = fingerprint_path(path, &metadata)
        .map_err(|error| format!("skip_stale_plan: failed to fingerprint path: {error}"))?;
    let Some(expected_fingerprint) = &entry.content_fingerprint else {
        return Err("skip_stale_plan: persisted content fingerprint is missing".to_string());
    };
    if current_fingerprint != *expected_fingerprint {
        return Err("skip_stale_plan: content fingerprint changed".to_string());
    }

    if let Some(expected_modified) = entry.modified {
        let current_modified = metadata.modified().ok().and_then(system_time_to_timestamp);
        if current_modified != Some(expected_modified) {
            return Err("skip_stale_plan: modification time changed".to_string());
        }
    }

    let Some(persisted_class) = class_from_label(&entry.class) else {
        return Err("skip_stale_plan: persisted class is not recognized".to_string());
    };
    let current_class = classify_cargo_home_relative_path(relative_path);
    if current_class != persisted_class {
        return Err(format!(
            "skip_stale_plan: class changed from {} to {}",
            entry.class,
            class_label(current_class)
        ));
    }

    if !known_cache_class_selected_by_policy(persisted_class, &document.body.policy) {
        return Err(
            "skip_stale_plan: persisted class is not selected by persisted policy".to_string(),
        );
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

fn is_plain_relative_path(path: &Path) -> bool {
    let mut has_component = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => has_component = true,
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => return false,
        }
    }
    has_component
}

fn result(
    entry: &PersistedCargoHomePlanEntry,
    status: CargoHomeApplyEntryStatus,
    reason: impl Into<String>,
) -> CargoHomeApplyEntryResult {
    CargoHomeApplyEntryResult {
        path: entry.path.clone(),
        planned_action: entry.action.clone(),
        status,
        size_bytes: entry.size_bytes,
        reason: reason.into(),
    }
}

fn class_from_label(value: &str) -> Option<CargoHomeClass> {
    match value {
        "registry_index" => Some(CargoHomeClass::RegistryIndex),
        "registry_cache" => Some(CargoHomeClass::RegistryCache),
        "registry_source" => Some(CargoHomeClass::RegistrySource),
        "git_database" => Some(CargoHomeClass::GitDatabase),
        "git_checkouts" => Some(CargoHomeClass::GitCheckouts),
        "config" => Some(CargoHomeClass::Config),
        "credentials" => Some(CargoHomeClass::Credentials),
        "installed_binaries" => Some(CargoHomeClass::InstalledBinaries),
        "install_metadata" => Some(CargoHomeClass::InstallMetadata),
        "unknown_user_authored" => Some(CargoHomeClass::UnknownUserAuthored),
        _ => None,
    }
}

fn known_cache_class_selected_by_policy(class: CargoHomeClass, policy: &str) -> bool {
    match policy {
        "conservative" => matches!(class, CargoHomeClass::RegistryCache),
        "balanced" => matches!(
            class,
            CargoHomeClass::RegistryCache | CargoHomeClass::RegistrySource
        ),
        "aggressive" => matches!(
            class,
            CargoHomeClass::RegistryIndex
                | CargoHomeClass::RegistryCache
                | CargoHomeClass::RegistrySource
                | CargoHomeClass::GitDatabase
                | CargoHomeClass::GitCheckouts
        ),
        _ => false,
    }
}

fn current_path_kind(metadata: &Metadata) -> &'static str {
    if metadata.is_file() {
        path_kind_label(CargoHomePathKind::File)
    } else if metadata.is_dir() {
        path_kind_label(CargoHomePathKind::Directory)
    } else {
        path_kind_label(CargoHomePathKind::Other)
    }
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
            let child_path: PathBuf = entry.path();
            let metadata = fs::symlink_metadata(&child_path).map_err(|error| {
                format!(
                    "skip_stale_plan: failed to read child metadata under {}: {error}",
                    path.display()
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err("skip_stale_plan: directory now contains a symlink".to_string());
            }
            total = total.saturating_add(measure_size(&child_path, &metadata)?);
        }
        return Ok(total);
    }

    Ok(metadata.len())
}

fn system_time_to_timestamp(time: std::time::SystemTime) -> Option<PersistedTimestamp> {
    PersistedTimestamp::from_system_time(time).ok()
}
