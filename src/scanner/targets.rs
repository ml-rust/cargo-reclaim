use std::path::{Component, Path, PathBuf};

use crate::error::ReclaimResult;
use crate::model::TargetEvidence;
use crate::planner::TargetContext;

use super::foundation::{CargoProject, ScannerOptions, TargetDirOverride};

const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
const RUSTC_INFO: &str = ".rustc_info.json";
const TARGET_DIR_NAME: &str = "target";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetCandidate {
    pub path: PathBuf,
    pub kind: TargetCandidateKind,
    pub evidence: Option<TargetEvidence>,
    pub target_context: Option<TargetContext>,
    pub skip_reason: Option<SkipReason>,
}

impl TargetCandidate {
    fn candidate(
        path: PathBuf,
        kind: TargetCandidateKind,
        evidence: TargetEvidence,
        target_context: Option<TargetContext>,
    ) -> Self {
        Self {
            path,
            kind,
            evidence: Some(evidence),
            target_context,
            skip_reason: None,
        }
    }

    fn skipped(path: PathBuf, skip_reason: SkipReason) -> Self {
        Self {
            path,
            kind: TargetCandidateKind::Unknown,
            evidence: None,
            target_context: None,
            skip_reason: Some(skip_reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCandidateKind {
    CargoTargetDir,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    SymlinkNotFollowed,
    NotRecognized,
}

pub fn classify_target_candidate(
    path: impl AsRef<Path>,
    project: Option<&CargoProject>,
    target_dir_override: Option<&TargetDirOverride>,
    options: &ScannerOptions,
) -> ReclaimResult<TargetCandidate> {
    if let Some(target_dir_override) = target_dir_override {
        classify_target_candidate_with_overrides(
            path,
            project,
            std::slice::from_ref(target_dir_override),
            options,
        )
    } else {
        classify_target_candidate_with_overrides(path, project, &[], options)
    }
}

pub(crate) fn classify_target_candidate_with_overrides(
    path: impl AsRef<Path>,
    project: Option<&CargoProject>,
    target_dir_overrides: &[TargetDirOverride],
    options: &ScannerOptions,
) -> ReclaimResult<TargetCandidate> {
    let path = path.as_ref();

    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
        && !options.follow_symlinks
    {
        return Ok(TargetCandidate::skipped(
            path.to_path_buf(),
            SkipReason::SymlinkNotFollowed,
        ));
    }

    let normalized_path = lexically_normalize(path);

    if path.join(CACHEDIR_TAG).is_file() {
        return Ok(TargetCandidate::candidate(
            path.to_path_buf(),
            TargetCandidateKind::CargoTargetDir,
            TargetEvidence::strong_marker(CACHEDIR_TAG)?,
            target_context(path, project),
        ));
    }

    if path.join(RUSTC_INFO).is_file() {
        return Ok(TargetCandidate::candidate(
            path.to_path_buf(),
            TargetCandidateKind::CargoTargetDir,
            TargetEvidence::strong_marker(RUSTC_INFO)?,
            target_context(path, project),
        ));
    }

    if let Some(target_dir_override) = target_dir_overrides
        .iter()
        .find(|override_dir| lexically_normalize(&override_dir.path) == normalized_path)
    {
        return Ok(TargetCandidate::candidate(
            path.to_path_buf(),
            TargetCandidateKind::CargoTargetDir,
            TargetEvidence::configured_path(target_dir_override.source.label.clone())?,
            target_context(path, project),
        ));
    }

    if let Some(project) = project
        && lexically_normalize(project.root_path.join(TARGET_DIR_NAME)) == normalized_path
    {
        return Ok(TargetCandidate::candidate(
            path.to_path_buf(),
            TargetCandidateKind::CargoTargetDir,
            TargetEvidence::project_context(project.manifest_path.clone())?,
            Some(TargetContext::new(path).with_project_root(&project.root_path)),
        ));
    }

    if path.file_name().is_some_and(|name| name == TARGET_DIR_NAME) {
        return Ok(TargetCandidate::candidate(
            path.to_path_buf(),
            TargetCandidateKind::CargoTargetDir,
            TargetEvidence::weak_name_only(TARGET_DIR_NAME)?,
            target_context(path, project),
        ));
    }

    Ok(TargetCandidate::skipped(
        path.to_path_buf(),
        SkipReason::NotRecognized,
    ))
}

fn target_context(path: &Path, project: Option<&CargoProject>) -> Option<TargetContext> {
    let mut target_context = TargetContext::new(path);

    if let Some(project) = project {
        target_context = target_context.with_project_root(&project.root_path);
    }

    Some(target_context)
}

fn lexically_normalize(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}
