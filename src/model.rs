use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::{ReclaimError, ReclaimResult};

pub const PLAN_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub schema_version: u16,
    pub input: PlanInput,
    pub entries: Vec<PlanEntry>,
    pub skipped_paths: Vec<PlanSkip>,
    pub totals: PlanTotals,
}

impl Plan {
    pub fn new(input: PlanInput, entries: Vec<PlanEntry>) -> Self {
        Self::with_skipped_paths(input, entries, Vec::new())
    }

    pub fn with_skipped_paths(
        input: PlanInput,
        entries: Vec<PlanEntry>,
        skipped_paths: Vec<PlanSkip>,
    ) -> Self {
        let totals = PlanTotals::from_entries_and_skips(&entries, &skipped_paths);

        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            input,
            entries,
            skipped_paths,
            totals,
        }
    }

    pub fn is_schema_current(&self) -> bool {
        self.schema_version == PLAN_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    pub roots: Vec<PathBuf>,
}

impl PlanInput {
    pub fn new(roots: impl IntoIterator<Item = impl Into<PathBuf>>) -> ReclaimResult<Self> {
        let mut validated_roots = Vec::new();

        for root in roots.into_iter().map(Into::into) {
            require_non_empty_path(&root)?;
            validated_roots.push(root);
        }

        if validated_roots.is_empty() {
            return Err(ReclaimError::EmptyPath);
        }

        Ok(Self {
            roots: validated_roots,
        })
    }

    pub fn from_root(root: impl Into<PathBuf>) -> ReclaimResult<Self> {
        Self::new([root.into()])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSkip {
    pub path: PathBuf,
    pub reason: PlanSkipReason,
    pub message: Option<String>,
}

impl PlanSkip {
    pub fn new(
        path: impl Into<PathBuf>,
        reason: PlanSkipReason,
        message: Option<String>,
    ) -> ReclaimResult<Self> {
        let path = path.into();
        require_non_empty_path(&path)?;

        Ok(Self {
            path,
            reason,
            message,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSkipReason {
    DefaultIgnoredDir,
    ConfiguredIgnoredPath,
    SymlinkNotFollowed,
    CrossFilesystem,
    WeakNameOnlySuppressed,
    AlreadyVisited,
    CargoConfigUnsupported,
    CargoConfigProblem,
    ReadError,
}

impl PlanSkipReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::DefaultIgnoredDir => "default_ignored_dir",
            Self::ConfiguredIgnoredPath => "configured_ignored_path",
            Self::SymlinkNotFollowed => "symlink_not_followed",
            Self::CrossFilesystem => "cross_filesystem",
            Self::WeakNameOnlySuppressed => "weak_name_only_suppressed",
            Self::AlreadyVisited => "already_visited",
            Self::CargoConfigUnsupported => "cargo_config_unsupported",
            Self::CargoConfigProblem => "cargo_config_problem",
            Self::ReadError => "read_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub snapshot: PathSnapshot,
    pub artifact_class: ArtifactClass,
    pub evidence: TargetEvidence,
    pub action: PlanAction,
    pub policy_reason: String,
    pub requires_confirmation: bool,
}

impl PlanEntry {
    pub fn new(
        snapshot: PathSnapshot,
        artifact_class: ArtifactClass,
        evidence: TargetEvidence,
        action: PlanAction,
        policy_reason: impl Into<String>,
        requires_confirmation: bool,
    ) -> ReclaimResult<Self> {
        Ok(Self {
            snapshot,
            artifact_class,
            evidence,
            action,
            policy_reason: non_empty_policy_reason(policy_reason)?,
            requires_confirmation,
        })
    }

    pub fn preserved(
        snapshot: PathSnapshot,
        artifact_class: ArtifactClass,
        evidence: TargetEvidence,
        reason: impl Into<String>,
    ) -> ReclaimResult<Self> {
        Self::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::Preserve,
            reason,
            false,
        )
    }

    pub fn delete(
        snapshot: PathSnapshot,
        artifact_class: ArtifactClass,
        evidence: TargetEvidence,
        reason: impl Into<String>,
        requires_confirmation: bool,
    ) -> ReclaimResult<Self> {
        Self::new(
            snapshot,
            artifact_class,
            evidence,
            PlanAction::Delete,
            reason,
            requires_confirmation,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub preserved_count: usize,
    pub delete_candidate_count: usize,
    pub skipped_path_count: usize,
}

impl PlanTotals {
    pub fn from_entries(entries: &[PlanEntry]) -> Self {
        Self::from_entries_and_skips(entries, &[])
    }

    pub fn from_entries_and_skips(entries: &[PlanEntry], skipped_paths: &[PlanSkip]) -> Self {
        let mut totals = Self {
            entry_count: entries.len(),
            skipped_path_count: skipped_paths.len(),
            ..Self::default()
        };

        for entry in entries {
            totals.total_bytes = totals.total_bytes.saturating_add(entry.snapshot.size_bytes);

            match entry.action {
                PlanAction::Delete => totals.delete_candidate_count += 1,
                PlanAction::Preserve
                | PlanAction::SkipActive
                | PlanAction::SkipLocked
                | PlanAction::Unknown
                | PlanAction::RequiresConfirmation => totals.preserved_count += 1,
            }
        }

        totals
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSnapshot {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub path_kind: PathKind,
    pub modified: Option<SystemTime>,
}

impl PathSnapshot {
    pub fn new(path: impl Into<PathBuf>, size_bytes: u64) -> ReclaimResult<Self> {
        Self::with_details(path, size_bytes, PathKind::Unknown, None)
    }

    pub fn with_details(
        path: impl Into<PathBuf>,
        size_bytes: u64,
        path_kind: PathKind,
        modified: Option<SystemTime>,
    ) -> ReclaimResult<Self> {
        let path = path.into();
        require_non_empty_path(&path)?;

        Ok(Self {
            path,
            size_bytes,
            path_kind,
            modified,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathKind {
    File,
    Directory,
    Symlink,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    Delete,
    Preserve,
    SkipActive,
    SkipLocked,
    Unknown,
    RequiresConfirmation,
}

impl PlanAction {
    pub fn is_delete(&self) -> bool {
        matches!(self, Self::Delete)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactClass {
    WholeTarget,
    Incremental,
    Deps,
    BuildScripts,
    Fingerprint,
    Docs,
    Package,
    Timings,
    Tmp,
    FingerprintGroupIntermediate,
    DepInfo,
    ObjectMetadata,
    FinalExecutable,
    FinalLibrary,
    FinalRlib,
    FinalWasm,
    Unknown,
}

impl ArtifactClass {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::WholeTarget => "whole_target",
            Self::Incremental => "incremental",
            Self::Deps => "deps",
            Self::BuildScripts => "build_scripts",
            Self::Fingerprint => "fingerprint",
            Self::Docs => "docs",
            Self::Package => "package",
            Self::Timings => "timings",
            Self::Tmp => "tmp",
            Self::FingerprintGroupIntermediate => "fingerprint_group_intermediate",
            Self::DepInfo => "dep_info",
            Self::ObjectMetadata => "object_metadata",
            Self::FinalExecutable => "final_executable",
            Self::FinalLibrary => "final_library",
            Self::FinalRlib => "final_rlib",
            Self::FinalWasm => "final_wasm",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetEvidence {
    StrongMarker { marker: String },
    ConfiguredPath { source: String },
    ProjectContext { project_manifest: PathBuf },
    WeakNameOnly { matched_name: String },
}

impl TargetEvidence {
    pub fn strong_marker(marker: impl Into<String>) -> ReclaimResult<Self> {
        Ok(Self::StrongMarker {
            marker: non_empty_string(marker)?,
        })
    }

    pub fn configured_path(source: impl Into<String>) -> ReclaimResult<Self> {
        Ok(Self::ConfiguredPath {
            source: non_empty_string(source)?,
        })
    }

    pub fn project_context(project_manifest: impl Into<PathBuf>) -> ReclaimResult<Self> {
        let project_manifest = project_manifest.into();
        require_non_empty_path(&project_manifest)?;

        Ok(Self::ProjectContext { project_manifest })
    }

    pub fn weak_name_only(matched_name: impl Into<String>) -> ReclaimResult<Self> {
        Ok(Self::WeakNameOnly {
            matched_name: non_empty_string(matched_name)?,
        })
    }

    pub fn is_weak_name_only(&self) -> bool {
        matches!(self, Self::WeakNameOnly { .. })
    }

    pub fn meets_default_delete_confidence(&self) -> bool {
        !self.is_weak_name_only()
    }
}

fn require_non_empty_path(path: &Path) -> ReclaimResult<()> {
    if path.as_os_str().is_empty() {
        return Err(ReclaimError::EmptyPath);
    }

    Ok(())
}

fn non_empty_string(value: impl Into<String>) -> ReclaimResult<String> {
    let value = value.into();

    if value.trim().is_empty() {
        return Err(ReclaimError::EmptyEvidence);
    }

    Ok(value)
}

fn non_empty_policy_reason(value: impl Into<String>) -> ReclaimResult<String> {
    let value = value.into();

    if value.trim().is_empty() {
        return Err(ReclaimError::EmptyPolicyReason);
    }

    Ok(value)
}
