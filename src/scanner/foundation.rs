use std::path::PathBuf;

use crate::error::{ReclaimError, ReclaimResult};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScannerOptions {
    pub follow_symlinks: bool,
    pub allow_name_only_targets: bool,
    pub cross_filesystems: bool,
    pub ignored_paths: Vec<PathBuf>,
    pub skipped_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDirOverride {
    pub path: PathBuf,
    pub source: TargetDirOverrideSource,
    kind: CargoOutputKind,
}

impl TargetDirOverride {
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> ReclaimResult<Self> {
        Self::with_kind(path, source, CargoOutputKind::TargetDir)
    }

    pub(crate) fn with_kind(
        path: impl Into<PathBuf>,
        source: impl Into<String>,
        kind: CargoOutputKind,
    ) -> ReclaimResult<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(ReclaimError::EmptyPath);
        }

        Ok(Self {
            path,
            source: TargetDirOverrideSource::new(source)?,
            kind,
        })
    }

    pub(crate) fn is_build_dir(&self) -> bool {
        matches!(self.kind, CargoOutputKind::BuildDir)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoOutputKind {
    TargetDir,
    BuildDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDirOverrideSource {
    pub label: String,
}

impl TargetDirOverrideSource {
    pub fn new(label: impl Into<String>) -> ReclaimResult<Self> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(ReclaimError::EmptyEvidence);
        }

        Ok(Self { label })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoProject {
    pub manifest_path: PathBuf,
    pub root_path: PathBuf,
}

impl CargoProject {
    pub(crate) fn new(manifest_path: PathBuf, root_path: PathBuf) -> Self {
        Self {
            manifest_path,
            root_path,
        }
    }
}
