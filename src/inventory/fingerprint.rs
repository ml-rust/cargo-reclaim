use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::classifier::classify_target_relative_path;
use crate::error::{ReclaimError, ReclaimResult};
use crate::model::{ArtifactClass, TargetEvidence};
use crate::planner::{PlannerCandidate, TargetContext};
use crate::policy::PolicyKind;

use super::foundation::{InventoryOptions, is_configured_skipped};
use super::snapshot_target_relative_path;

pub(crate) fn append_fingerprint_group_candidates(
    target_root: &Path,
    evidence: &TargetEvidence,
    target_context: &TargetContext,
    options: &InventoryOptions,
    keep_rustc_hashes: &[u64],
    candidates: &mut Vec<PlannerCandidate>,
) -> ReclaimResult<()> {
    let hashes = collect_fingerprint_hashes(target_root, options, keep_rustc_hashes)?;
    if hashes.is_empty() {
        return Ok(());
    }

    let existing_delete_roots = candidates
        .iter()
        .filter(|candidate| PolicyKind::is_default_removable_class(candidate.artifact_class))
        .map(|candidate| candidate.snapshot.path.clone())
        .collect::<Vec<_>>();
    let mut existing_paths = candidates
        .iter()
        .map(|candidate| candidate.snapshot.path.clone())
        .collect::<HashSet<_>>();
    let mut matched_paths = Vec::new();
    let mut visited_dirs = HashSet::new();
    collect_hash_matched_paths(
        target_root,
        PathBuf::new(),
        &hashes,
        options,
        &existing_delete_roots,
        &mut visited_dirs,
        &mut matched_paths,
    )?;

    for child_path in matched_paths {
        let full_path = target_root.join(&child_path);
        if existing_paths.contains(&full_path) {
            candidates.retain(|candidate| candidate.snapshot.path != full_path);
        }

        let snapshot = snapshot_target_relative_path(target_root, &child_path, options)?;
        let candidate = PlannerCandidate::new(
            snapshot,
            ArtifactClass::FingerprintGroupIntermediate,
            evidence.clone(),
        )
        .with_target_context(target_context.clone());
        existing_paths.insert(candidate.snapshot.path.clone());
        candidates.push(candidate);
    }

    Ok(())
}

fn collect_fingerprint_hashes(
    target_root: &Path,
    options: &InventoryOptions,
    keep_rustc_hashes: &[u64],
) -> ReclaimResult<HashSet<String>> {
    let mut hashes = HashMap::new();
    let keep_rustc_hashes = keep_rustc_hashes.iter().copied().collect::<HashSet<_>>();
    let mut visited_dirs = HashSet::new();
    collect_fingerprint_hashes_from_child(
        target_root,
        PathBuf::new(),
        options,
        &keep_rustc_hashes,
        &mut visited_dirs,
        &mut hashes,
    )?;
    Ok(hashes
        .into_iter()
        .filter_map(|(hash, status)| status.should_emit().then_some(hash))
        .collect())
}

fn collect_fingerprint_hashes_from_child(
    target_root: &Path,
    child_path: PathBuf,
    options: &InventoryOptions,
    keep_rustc_hashes: &HashSet<u64>,
    visited_dirs: &mut HashSet<PathBuf>,
    hashes: &mut HashMap<String, FingerprintHashStatus>,
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

    if metadata.is_dir() {
        if is_in_fingerprint_dir(&child_path)
            && let Some(hash) = cargo_hash_suffix(child_path.file_name())
        {
            let status = fingerprint_dir_rustc_status(&full_path, keep_rustc_hashes)?;
            hashes.entry(hash.to_string()).or_default().merge(status);
        }

        let canonical_path = fs::canonicalize(&full_path)
            .map_err(|error| inventory_read_error(&full_path, error))?;
        if !visited_dirs.insert(canonical_path) {
            return Ok(());
        }

        for child in sorted_children(&full_path)? {
            let Some(file_name) = child.file_name() else {
                continue;
            };
            collect_fingerprint_hashes_from_child(
                target_root,
                child_path.join(file_name),
                options,
                keep_rustc_hashes,
                visited_dirs,
                hashes,
            )?;
        }
    }

    Ok(())
}

fn collect_hash_matched_paths(
    target_root: &Path,
    child_path: PathBuf,
    hashes: &HashSet<String>,
    options: &InventoryOptions,
    existing_delete_roots: &[PathBuf],
    visited_dirs: &mut HashSet<PathBuf>,
    matched_paths: &mut Vec<PathBuf>,
) -> ReclaimResult<()> {
    let full_path = target_root.join(&child_path);
    if is_configured_skipped(&full_path, options)
        || is_under_existing_delete_root(&full_path, existing_delete_roots)
    {
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

    if is_hash_matched_intermediate(&child_path, metadata.is_file(), hashes) {
        matched_paths.push(child_path.clone());
    }

    if metadata.is_dir() {
        let canonical_path = fs::canonicalize(&full_path)
            .map_err(|error| inventory_read_error(&full_path, error))?;
        if !visited_dirs.insert(canonical_path) {
            return Ok(());
        }

        for child in sorted_children(&full_path)? {
            let Some(file_name) = child.file_name() else {
                continue;
            };
            collect_hash_matched_paths(
                target_root,
                child_path.join(file_name),
                hashes,
                options,
                existing_delete_roots,
                visited_dirs,
                matched_paths,
            )?;
        }
    }

    Ok(())
}

fn is_hash_matched_intermediate(
    child_path: &Path,
    is_file: bool,
    hashes: &HashSet<String>,
) -> bool {
    if !is_group_search_location(child_path) {
        return false;
    }

    let Some(hash) = cargo_hash_suffix(child_path.file_name()) else {
        return false;
    };
    if !hashes.contains(hash) {
        return false;
    }

    if is_file && is_protected_file_name(child_path.file_name()) {
        return false;
    }

    classify_target_relative_path(child_path) == ArtifactClass::Unknown
}

fn is_group_search_location(child_path: &Path) -> bool {
    let components = child_path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(component) => Some(component),
            _ => None,
        })
        .collect::<Vec<_>>();

    if components.len() < 2 {
        return false;
    }

    if components
        .iter()
        .any(|component| is_group_artifact_dir(component))
    {
        return true;
    }

    is_profile_root_path(&components)
}

fn is_group_artifact_dir(component: &OsStr) -> bool {
    component == ".fingerprint"
        || component == "fingerprint"
        || component == "build"
        || component == "deps"
        || component == "native"
}

fn is_in_fingerprint_dir(child_path: &Path) -> bool {
    child_path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|parent| parent == ".fingerprint" || parent == "fingerprint")
}

fn is_profile_root_path(components: &[&OsStr]) -> bool {
    match components {
        [profile, _] => is_profile_root(profile),
        [triple, profile, _] => is_target_triple(triple) && is_profile_root(profile),
        _ => false,
    }
}

fn is_profile_root(component: &OsStr) -> bool {
    !is_group_artifact_dir(component)
        && component != "doc"
        && component != "docs"
        && component != "package"
        && component != "timings"
        && component != "cargo-timings"
        && component != "tmp"
        && component != "target"
        && component
            .to_str()
            .is_some_and(|component| !component.is_empty() && !component.contains('.'))
}

fn is_target_triple(component: &OsStr) -> bool {
    component
        .to_str()
        .is_some_and(|component| component.matches('-').count() >= 2)
}

fn cargo_hash_suffix(file_name: Option<&OsStr>) -> Option<&str> {
    let file_name = file_name?.to_str()?;
    let name_before_extension = Path::new(file_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or(file_name);
    let (_, hash) = name_before_extension.rsplit_once('-')?;

    (hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hash)
}

#[derive(Debug, Clone, Copy, Default)]
struct FingerprintHashStatus {
    has_kept: bool,
    has_unkept: bool,
}

impl FingerprintHashStatus {
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

    fn merge(&mut self, other: Self) {
        self.has_kept |= other.has_kept;
        self.has_unkept |= other.has_unkept;
    }

    fn should_emit(self) -> bool {
        // If a cargo hash appears with both kept and unkept rustc values, stay conservative
        // and skip emitting it so we do not delete a shared hash group accidentally.
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

fn is_protected_file_name(file_name: Option<&OsStr>) -> bool {
    let Some(file_name) = file_name.and_then(OsStr::to_str) else {
        return true;
    };
    let Some(extension) = Path::new(file_name).extension().and_then(OsStr::to_str) else {
        return true;
    };

    matches!(
        extension,
        "a" | "dll" | "dylib" | "exe" | "lib" | "rlib" | "rmeta" | "so" | "wasm"
    )
}

fn is_under_existing_delete_root(path: &Path, existing_delete_roots: &[PathBuf]) -> bool {
    existing_delete_roots
        .iter()
        .any(|root| path == root || path.starts_with(root))
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
