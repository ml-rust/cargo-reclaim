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

pub(crate) fn append_stale_deps_candidates(
    target_root: &Path,
    evidence: &TargetEvidence,
    target_context: &TargetContext,
    options: &InventoryOptions,
    keep_rustc_hashes: &[u64],
    candidates: &mut Vec<PlannerCandidate>,
) -> ReclaimResult<()> {
    let stale_paths = collect_stale_deps_paths(target_root, options, keep_rustc_hashes)?;
    if stale_paths.is_empty() {
        return Ok(());
    }

    let stale_full_paths = stale_paths
        .iter()
        .map(|path| target_root.join(path))
        .collect::<HashSet<_>>();
    candidates.retain(|candidate| !stale_full_paths.contains(&candidate.snapshot.path));

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
        let candidate = PlannerCandidate::new(snapshot, ArtifactClass::StaleDeps, evidence.clone())
            .with_target_context(target_context.clone());
        existing_paths.insert(candidate.snapshot.path.clone());
        candidates.push(candidate);
    }

    Ok(())
}

fn collect_stale_deps_paths(
    target_root: &Path,
    options: &InventoryOptions,
    keep_rustc_hashes: &[u64],
) -> ReclaimResult<Vec<PathBuf>> {
    let mut deps_dirs = Vec::new();
    let mut visited_dirs = HashSet::new();
    collect_deps_dirs(
        target_root,
        PathBuf::new(),
        options,
        &mut visited_dirs,
        &mut deps_dirs,
    )?;

    let mut stale_paths = Vec::new();
    let keep_rustc_hashes = keep_rustc_hashes.iter().copied().collect::<HashSet<_>>();
    for deps_dir in deps_dirs {
        stale_paths.extend(stale_paths_from_deps_dir(
            target_root,
            &deps_dir,
            options,
            &keep_rustc_hashes,
        )?);
    }
    stale_paths.sort();
    Ok(stale_paths)
}

fn collect_deps_dirs(
    target_root: &Path,
    child_path: PathBuf,
    options: &InventoryOptions,
    visited_dirs: &mut HashSet<PathBuf>,
    deps_dirs: &mut Vec<PathBuf>,
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

    if is_deps_dir_path(&child_path) {
        deps_dirs.push(child_path);
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
        collect_deps_dirs(
            target_root,
            child_path.join(file_name),
            options,
            visited_dirs,
            deps_dirs,
        )?;
    }

    Ok(())
}

fn stale_paths_from_deps_dir(
    target_root: &Path,
    deps_dir: &Path,
    options: &InventoryOptions,
    keep_rustc_hashes: &HashSet<u64>,
) -> ReclaimResult<Vec<PathBuf>> {
    let fingerprint_anchors =
        collect_profile_fingerprint_hashes(target_root, deps_dir, keep_rustc_hashes)?;
    if fingerprint_anchors.is_present_and_empty() {
        return Ok(Vec::new());
    }

    let mut groups: HashMap<String, HashMap<String, DepsHashVariant>> = HashMap::new();
    let full_deps_dir = target_root.join(deps_dir);

    for child in sorted_children(&full_deps_dir)? {
        let Some(file_name) = child.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let child_path = deps_dir.join(file_name);
        let full_path = target_root.join(&child_path);
        if is_configured_skipped(&full_path, options) {
            continue;
        }

        let symlink_metadata = match fs::symlink_metadata(&full_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(inventory_read_error(&full_path, error)),
        };
        if symlink_metadata.file_type().is_symlink() && !options.follow_symlinks {
            continue;
        }

        let metadata = if symlink_metadata.file_type().is_symlink() {
            fs::metadata(&full_path).map_err(|error| inventory_read_error(&full_path, error))?
        } else {
            symlink_metadata
        };
        if !metadata.is_file() {
            continue;
        }

        let Some((family, hash)) = cargo_family_hash(file_name) else {
            continue;
        };
        let hash_status = match fingerprint_anchors.status_for_hash(hash) {
            Some(status) => status,
            None => continue,
        };
        groups
            .entry(family.to_string())
            .or_default()
            .entry(hash.to_string())
            .or_default()
            .push(child_path, metadata.modified().ok(), hash_status);
    }

    let mut stale_paths = Vec::new();
    for variants in groups.into_values() {
        stale_paths.extend(stale_variant_paths(variants));
    }
    Ok(stale_paths)
}

fn stale_variant_paths(variants: HashMap<String, DepsHashVariant>) -> Vec<PathBuf> {
    if variants.len() < 2 {
        return Vec::new();
    }

    let Some(newest_modified) = variants
        .values()
        .filter_map(|variant| variant.newest_modified)
        .max()
    else {
        return Vec::new();
    };

    variants
        .into_values()
        .filter(|variant| {
            variant.hash_status.should_emit()
                && variant
                    .newest_modified
                    .is_some_and(|modified| modified < newest_modified)
        })
        .flat_map(|variant| variant.paths)
        .collect()
}

#[derive(Debug, Default)]
struct DepsHashVariant {
    paths: Vec<PathBuf>,
    newest_modified: Option<SystemTime>,
    hash_status: FingerprintHashStatus,
}

impl DepsHashVariant {
    fn push(
        &mut self,
        path: PathBuf,
        modified: Option<SystemTime>,
        hash_status: FingerprintHashStatus,
    ) {
        self.paths.push(path);
        self.hash_status.merge(hash_status);
        self.newest_modified = match (self.newest_modified, modified) {
            (Some(current), Some(next)) => Some(current.max(next)),
            (None, Some(next)) => Some(next),
            (current, None) => current,
        };
    }
}

fn collect_profile_fingerprint_hashes(
    target_root: &Path,
    deps_dir: &Path,
    keep_rustc_hashes: &HashSet<u64>,
) -> ReclaimResult<FingerprintAnchors> {
    let Some(profile_dir) = deps_dir.parent() else {
        return Ok(FingerprintAnchors::Present(HashMap::new()));
    };
    let fingerprint_dir = profile_dir.join(".fingerprint");
    let full_fingerprint_dir = target_root.join(&fingerprint_dir);
    let entries = match fs::read_dir(&full_fingerprint_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(FingerprintAnchors::Missing);
        }
        Err(error) => return Err(inventory_read_error(&full_fingerprint_dir, error)),
    };

    let mut hashes = HashMap::new();
    for entry in entries {
        let entry = entry.map_err(|error| inventory_read_error(&full_fingerprint_dir, error))?;
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(inventory_read_error(&path, error)),
        };
        if !metadata.is_dir() {
            continue;
        }
        let Some(hash) = cargo_hash_from_fingerprint_dir_name(path.file_name()) else {
            continue;
        };
        let status = fingerprint_dir_rustc_status(&path, keep_rustc_hashes)?;
        if status.has_evidence() {
            hashes
                .entry(hash.to_string())
                .or_insert(status)
                .merge(status);
        }
    }
    Ok(FingerprintAnchors::Present(hashes))
}

#[derive(Debug)]
enum FingerprintAnchors {
    Missing,
    Present(HashMap<String, FingerprintHashStatus>),
}

impl FingerprintAnchors {
    fn is_present_and_empty(&self) -> bool {
        matches!(self, Self::Present(hashes) if hashes.is_empty())
    }

    fn status_for_hash(&self, hash: &str) -> Option<FingerprintHashStatus> {
        match self {
            Self::Missing => Some(FingerprintHashStatus::orphaned()),
            Self::Present(hashes) => hashes.get(hash).copied(),
        }
    }
}

fn cargo_hash_from_fingerprint_dir_name(file_name: Option<&OsStr>) -> Option<&str> {
    let file_name = file_name?.to_str()?;
    let (_, hash) = file_name.rsplit_once('-')?;
    (hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hash)
}

#[derive(Debug, Clone, Copy, Default)]
struct FingerprintHashStatus {
    has_kept: bool,
    has_unkept: bool,
}

impl FingerprintHashStatus {
    fn orphaned() -> Self {
        Self {
            has_kept: false,
            has_unkept: true,
        }
    }

    fn from_rustc(rustc: u64, keep_rustc_hashes: &HashSet<u64>) -> Self {
        if keep_rustc_hashes.contains(&rustc) {
            Self {
                has_kept: true,
                has_unkept: false,
            }
        } else {
            Self {
                has_kept: false,
                has_unkept: true,
            }
        }
    }

    fn has_evidence(self) -> bool {
        self.has_kept || self.has_unkept
    }

    fn merge(&mut self, other: Self) {
        self.has_kept |= other.has_kept;
        self.has_unkept |= other.has_unkept;
    }

    fn should_emit(self) -> bool {
        self.has_unkept && !self.has_kept
    }
}

fn fingerprint_dir_rustc_status(
    path: &Path,
    keep_rustc_hashes: &HashSet<u64>,
) -> ReclaimResult<FingerprintHashStatus> {
    let entries = fs::read_dir(path).map_err(|error| inventory_read_error(path, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| inventory_read_error(path, error))?;
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("json") {
            continue;
        }
        let contents =
            fs::read_to_string(&path).map_err(|error| inventory_read_error(&path, error))?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
            return Ok(FingerprintHashStatus::default());
        };
        return Ok(value
            .get("rustc")
            .and_then(serde_json::Value::as_u64)
            .map(|rustc| FingerprintHashStatus::from_rustc(rustc, keep_rustc_hashes))
            .unwrap_or_default());
    }

    Ok(FingerprintHashStatus::default())
}

fn is_deps_dir_path(path: &Path) -> bool {
    let components = normal_components(path);
    match components.as_slice() {
        [profile, deps] => is_profile_root(profile) && *deps == "deps",
        [triple, profile, deps] => {
            is_target_triple(triple) && is_profile_root(profile) && *deps == "deps"
        }
        _ => false,
    }
}

fn cargo_family_hash(file_name: &str) -> Option<(&str, &str)> {
    let name_before_extension = Path::new(file_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or(file_name);
    let (family, hash) = name_before_extension.rsplit_once('-')?;
    if family.is_empty() {
        return None;
    }
    (hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some((family, hash))
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
