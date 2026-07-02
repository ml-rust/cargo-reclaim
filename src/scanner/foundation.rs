use std::path::PathBuf;

use crate::error::{ReclaimError, ReclaimResult};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScannerOptions {
    pub follow_symlinks: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDirOverride {
    pub path: PathBuf,
    pub source: TargetDirOverrideSource,
}

impl TargetDirOverride {
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> ReclaimResult<Self> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(ReclaimError::EmptyPath);
        }

        let source = TargetDirOverrideSource::new(source)?;

        Ok(Self { path, source })
    }
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
