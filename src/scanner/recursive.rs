use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ReclaimResult;
use crate::model::TargetEvidence;

use super::cargo::detect_cargo_project;
use super::foundation::{CargoProject, ScannerOptions};
use super::targets::{TargetCandidate, TargetCandidateKind, classify_target_candidate};

const DEFAULT_IGNORED_DIRS: &[&str] = &[".git", ".cargo", "node_modules"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanItem {
    CargoProject(CargoProject),
    TargetCandidate(TargetCandidate),
    Skipped(ScanSkip),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSkip {
    pub path: PathBuf,
    pub reason: ScanSkipReason,
}

impl ScanSkip {
    fn new(path: PathBuf, reason: ScanSkipReason) -> Self {
        Self { path, reason }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanSkipReason {
    DefaultIgnoredDir,
    ConfiguredIgnoredPath,
    SymlinkNotFollowed,
    CrossFilesystem,
    WeakNameOnlySuppressed,
    AlreadyVisited,
    ReadError { message: String },
}

pub fn scan_roots(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    options: &ScannerOptions,
) -> ReclaimResult<Vec<ScanItem>> {
    let mut items = Vec::new();

    for root in roots {
        let root = root.into();
        let root_device = filesystem_device(&root, options);
        let mut visited_dirs = HashSet::new();
        scan_path(
            &root,
            options,
            None,
            root_device,
            &mut visited_dirs,
            &mut items,
        )?;
    }

    Ok(items)
}

fn scan_path(
    path: &Path,
    options: &ScannerOptions,
    project_context: Option<&CargoProject>,
    root_device: Option<u64>,
    visited_dirs: &mut HashSet<PathBuf>,
    items: &mut Vec<ScanItem>,
) -> ReclaimResult<()> {
    if is_configured_ignored(path, options) {
        push_skipped(items, path, ScanSkipReason::ConfiguredIgnoredPath);
        return Ok(());
    }

    let symlink_metadata = match read_symlink_metadata(path, items) {
        Some(metadata) => metadata,
        None => return Ok(()),
    };

    if symlink_metadata.file_type().is_symlink() && !options.follow_symlinks {
        push_skipped(items, path, ScanSkipReason::SymlinkNotFollowed);
        return Ok(());
    }

    let metadata = if symlink_metadata.file_type().is_symlink() {
        match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                push_read_error(items, path, error);
                return Ok(());
            }
        }
    } else {
        symlink_metadata
    };

    if !metadata.is_dir() {
        return Ok(());
    }

    if is_default_ignored_dir(path) {
        push_skipped(items, path, ScanSkipReason::DefaultIgnoredDir);
        return Ok(());
    }

    if let Ok(canonical_path) = fs::canonicalize(path)
        && !visited_dirs.insert(canonical_path)
    {
        push_skipped(items, path, ScanSkipReason::AlreadyVisited);
        return Ok(());
    }

    if is_cross_filesystem(&metadata, options, root_device) {
        push_skipped(items, path, ScanSkipReason::CrossFilesystem);
        return Ok(());
    }

    let project = detect_cargo_project(path);
    if let Some(project) = project.as_ref() {
        items.push(ScanItem::CargoProject(project.clone()));
    }
    let project_context = project.as_ref().or(project_context);

    let candidate = classify_target_candidate(path, project_context, None, options)?;
    if candidate.kind == TargetCandidateKind::CargoTargetDir {
        if candidate
            .evidence
            .as_ref()
            .is_some_and(TargetEvidence::is_weak_name_only)
            && !options.allow_name_only_targets
        {
            items.push(ScanItem::Skipped(ScanSkip::new(
                candidate.path,
                ScanSkipReason::WeakNameOnlySuppressed,
            )));
        } else {
            items.push(ScanItem::TargetCandidate(candidate));
        }
    }

    for child in sorted_children(path, items) {
        scan_path(
            &child,
            options,
            project_context,
            root_device,
            visited_dirs,
            items,
        )?;
    }

    Ok(())
}

fn sorted_children(path: &Path, items: &mut Vec<ScanItem>) -> Vec<PathBuf> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            push_read_error(items, path, error);
            return Vec::new();
        }
    };

    let mut children = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => children.push(entry.path()),
            Err(error) => push_read_error(items, path, error),
        }
    }
    children.sort();
    children
}

fn push_skipped(items: &mut Vec<ScanItem>, path: &Path, reason: ScanSkipReason) {
    items.push(ScanItem::Skipped(ScanSkip::new(path.to_path_buf(), reason)));
}

fn push_read_error(items: &mut Vec<ScanItem>, path: &Path, error: std::io::Error) {
    push_skipped(
        items,
        path,
        ScanSkipReason::ReadError {
            message: error.to_string(),
        },
    );
}

fn read_symlink_metadata(path: &Path, items: &mut Vec<ScanItem>) -> Option<fs::Metadata> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            push_read_error(items, path, error);
            None
        }
    }
}

fn is_default_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| DEFAULT_IGNORED_DIRS.contains(&name))
}

fn is_configured_ignored(path: &Path, options: &ScannerOptions) -> bool {
    let normalized_path = lexically_normalize(path);
    options
        .ignored_paths
        .iter()
        .any(|ignored| lexically_normalize(ignored) == normalized_path)
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

#[cfg(unix)]
fn filesystem_device(path: &Path, options: &ScannerOptions) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    if options.cross_filesystems {
        return None;
    }

    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(not(unix))]
fn filesystem_device(_path: &Path, _options: &ScannerOptions) -> Option<u64> {
    None
}

#[cfg(unix)]
fn is_cross_filesystem(
    metadata: &fs::Metadata,
    options: &ScannerOptions,
    root_device: Option<u64>,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    !options.cross_filesystems && root_device.is_some_and(|device| metadata.dev() != device)
}

#[cfg(not(unix))]
fn is_cross_filesystem(
    _metadata: &fs::Metadata,
    _options: &ScannerOptions,
    _root_device: Option<u64>,
) -> bool {
    false
}
