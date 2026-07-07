use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ReclaimResult;
use crate::model::TargetEvidence;

use super::cargo::detect_cargo_project;
use super::cargo_config::{
    CargoConfigProblem, CargoConfigUnsupported, CargoConfigUnsupportedReason,
    resolve_project_output_dirs,
};
use super::filesystem::{filesystem_device, is_cross_filesystem};
use super::foundation::TargetDirOverride;
use super::foundation::{CargoProject, ScannerOptions};
use super::targets::{
    TargetCandidate, TargetCandidateKind, classify_target_candidate_with_overrides,
};

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
    CargoConfigUnsupported { message: String },
    CargoConfigProblem { message: String },
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
        let mut emitted_targets = HashSet::new();
        scan_path(
            &root,
            options,
            None,
            root_device,
            &mut visited_dirs,
            &mut emitted_targets,
            &mut items,
        )?;
    }

    Ok(items)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectScanContext {
    project: CargoProject,
    configured_output_dirs: Vec<TargetDirOverride>,
}

fn scan_path(
    path: &Path,
    options: &ScannerOptions,
    project_context: Option<&ProjectScanContext>,
    root_device: Option<u64>,
    visited_dirs: &mut HashSet<PathBuf>,
    emitted_targets: &mut HashSet<PathBuf>,
    items: &mut Vec<ScanItem>,
) -> ReclaimResult<()> {
    if is_configured_skipped(path, options) {
        return Ok(());
    }

    if is_configured_ignored(path, options) {
        push_skipped(items, path, ScanSkipReason::ConfiguredIgnoredPath);
        return Ok(());
    }

    let symlink_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            push_read_error(items, path, error);
            return Ok(());
        }
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
    let local_project_context = if let Some(project) = project {
        let output_dirs = resolve_project_output_dirs(&project.root_path)?;
        for problem in &output_dirs.problems {
            push_cargo_config_problem(items, &project.root_path, problem);
        }
        for unsupported in &output_dirs.unsupported {
            push_cargo_config_unsupported(items, &project.root_path, unsupported);
        }
        let configured_output_dirs = output_dirs.dirs;
        for output_dir in &configured_output_dirs {
            emit_configured_output_dir(
                output_dir,
                Some(&project),
                options,
                emitted_targets,
                items,
            )?;
        }
        items.push(ScanItem::CargoProject(project.clone()));
        Some(ProjectScanContext {
            project,
            configured_output_dirs,
        })
    } else {
        None
    };
    let project_context = local_project_context.as_ref().or(project_context);

    let configured_output_dirs = project_context
        .map(|context| context.configured_output_dirs.as_slice())
        .unwrap_or(&[]);
    emit_target_candidate(
        path,
        project_context.map(|context| &context.project),
        configured_output_dirs,
        options,
        emitted_targets,
        items,
    )?;

    for child in sorted_children(path, items) {
        scan_path(
            &child,
            options,
            project_context,
            root_device,
            visited_dirs,
            emitted_targets,
            items,
        )?;
    }

    Ok(())
}

fn emit_configured_output_dir(
    output_dir: &TargetDirOverride,
    project_context: Option<&CargoProject>,
    options: &ScannerOptions,
    emitted_targets: &mut HashSet<PathBuf>,
    items: &mut Vec<ScanItem>,
) -> ReclaimResult<()> {
    let path = output_dir.path.as_path();

    if is_configured_skipped(path, options) {
        return Ok(());
    }

    if is_configured_ignored(path, options) {
        push_skipped(items, path, ScanSkipReason::ConfiguredIgnoredPath);
        return Ok(());
    }

    if is_under_default_ignored_dir(path) {
        push_skipped(items, path, ScanSkipReason::DefaultIgnoredDir);
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

    // A configured output dir is an explicit, named location: unlike incidental
    // traversal (see `scan_path`), it is not gated by the filesystem boundary, so
    // a shared CARGO_TARGET_DIR on a separate disk is discovered without
    // `--cross-filesystems`.
    emit_target_candidate(
        path,
        project_context,
        std::slice::from_ref(output_dir),
        options,
        emitted_targets,
        items,
    )
}

fn emit_target_candidate(
    path: &Path,
    project_context: Option<&CargoProject>,
    configured_output_dirs: &[TargetDirOverride],
    options: &ScannerOptions,
    emitted_targets: &mut HashSet<PathBuf>,
    items: &mut Vec<ScanItem>,
) -> ReclaimResult<()> {
    let candidate = classify_target_candidate_with_overrides(
        path,
        project_context,
        configured_output_dirs,
        options,
    )?;
    if candidate.kind == TargetCandidateKind::CargoTargetDir {
        let normalized_path = lexically_normalize(&candidate.path);
        if !emitted_targets.insert(normalized_path) {
            return Ok(());
        }

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

fn push_cargo_config_unsupported(
    items: &mut Vec<ScanItem>,
    path: &Path,
    unsupported: &CargoConfigUnsupported,
) {
    let reason = match unsupported.reason {
        CargoConfigUnsupportedReason::WorkspacePathHashTemplate => {
            "unsupported build.build-dir template {workspace-path-hash}"
        }
    };
    push_skipped(
        items,
        path,
        ScanSkipReason::CargoConfigUnsupported {
            message: format!("{} in {}", reason, unsupported.source),
        },
    );
}

fn push_cargo_config_problem(items: &mut Vec<ScanItem>, path: &Path, problem: &CargoConfigProblem) {
    push_skipped(
        items,
        path,
        ScanSkipReason::CargoConfigProblem {
            message: format!("{}: {}", problem.path.display(), problem.message),
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

fn is_under_default_ignored_dir(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| DEFAULT_IGNORED_DIRS.contains(&name))
    })
}

fn is_configured_ignored(path: &Path, options: &ScannerOptions) -> bool {
    let normalized_path = lexically_normalize(path);
    options
        .ignored_paths
        .iter()
        .any(|ignored| lexically_normalize(ignored) == normalized_path)
}

fn is_configured_skipped(path: &Path, options: &ScannerOptions) -> bool {
    let normalized_path = lexically_normalize(path);
    options.skipped_paths.iter().any(|skipped| {
        let skipped = lexically_normalize(skipped);
        normalized_path == skipped || normalized_path.starts_with(skipped)
    })
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
