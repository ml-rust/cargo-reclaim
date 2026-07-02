use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::error::ConfigError;
use super::parse::{BackgroundDocument, ConfigDocument, PlannerConfig, PolicyConfig};
use super::values::{
    parse_config_duration, parse_config_percentage_basis_points, parse_config_size,
    resolve_config_path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimConfig {
    pub version: u16,
    pub roots: Vec<PathBuf>,
    pub ignored_paths: Vec<PathBuf>,
    pub skipped_paths: Vec<PathBuf>,
    pub policy: Option<String>,
    pub whole_target: Option<WholeTargetConfig>,
    pub allow_unattended_whole_target_delete: Option<bool>,
    pub policy_thresholds: PolicyThresholdConfig,
    pub background: BackgroundConfig,
    pub scanner: ScannerConfig,
    pub scheduler: SchedulerConfig,
    pub recent_write_keep_window: Option<Duration>,
    pub keep_size_bytes: Option<u64>,
    pub keep_rustc_hashes: Vec<u64>,
}

impl ReclaimConfig {
    pub(super) fn from_document(
        document: ConfigDocument,
        relative_base: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let ConfigDocument {
            version,
            roots,
            ignore,
            skip,
            policy,
            scanner,
            planner,
            scheduler,
            background,
        } = document;
        if version != 1 {
            return Err(ConfigError::UnsupportedVersion(version));
        }

        let policy_keep_recent_projects = policy
            .as_ref()
            .and_then(|policy| policy.keep_recent_projects.as_deref())
            .map(parse_config_duration)
            .transpose()?;
        let PlannerConfig {
            recent_write_keep_window,
            keep_days,
            keep_size,
            keep_rustc_hashes,
        } = planner.unwrap_or_default();
        let planner_recent_write_keep_window = recent_write_keep_window
            .as_deref()
            .map(parse_config_duration)
            .transpose()?;
        let planner_keep_days_window = keep_days
            .map(|days| {
                if days == 0 {
                    return Err(ConfigError::InvalidDuration(days.to_string()));
                }
                Ok(Duration::from_secs(days.saturating_mul(24 * 60 * 60)))
            })
            .transpose()?;
        let planner_keep_size_bytes = keep_size.as_deref().map(parse_config_size).transpose()?;
        let policy_thresholds = policy
            .as_ref()
            .map(PolicyThresholdConfig::from_document)
            .transpose()?
            .unwrap_or_default();
        let background = background
            .map(BackgroundConfig::from_document)
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            version,
            roots: roots
                .into_iter()
                .map(|path| resolve_config_path(path, relative_base))
                .collect(),
            ignored_paths: ignore
                .into_iter()
                .map(|path| resolve_config_path(path, relative_base))
                .collect(),
            skipped_paths: skip
                .into_iter()
                .map(|path| resolve_config_path(path, relative_base))
                .collect(),
            policy: policy.as_ref().and_then(|policy| policy.mode.clone()),
            whole_target: policy
                .as_ref()
                .and_then(|policy| policy.whole_target.as_deref())
                .map(WholeTargetConfig::parse)
                .transpose()?,
            allow_unattended_whole_target_delete: policy
                .as_ref()
                .and_then(|policy| policy.allow_unattended_whole_target_delete),
            policy_thresholds,
            background,
            scanner: scanner.unwrap_or_default(),
            scheduler: scheduler.unwrap_or_default().resolve_paths(relative_base),
            recent_write_keep_window: planner_recent_write_keep_window
                .or(planner_keep_days_window)
                .or(policy_keep_recent_projects),
            keep_size_bytes: planner_keep_size_bytes,
            keep_rustc_hashes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeTargetConfig {
    Off,
    Confirm,
    Delete,
}

impl WholeTargetConfig {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "off" => Ok(Self::Off),
            "confirm" => Ok(Self::Confirm),
            "delete" => Ok(Self::Delete),
            _ => Err(ConfigError::InvalidWholeTargetMode(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolicyThresholdConfig {
    pub max_target_size_bytes: Option<u64>,
    pub unattended_allowed: Option<bool>,
}

impl PolicyThresholdConfig {
    fn from_document(document: &PolicyConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            max_target_size_bytes: document
                .max_target_size
                .as_deref()
                .map(parse_config_size)
                .transpose()?,
            unattended_allowed: document.unattended_allowed,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackgroundConfig {
    pub enabled: Option<bool>,
    pub mode: Option<BackgroundMode>,
    pub check_every: Option<Duration>,
    pub only_when_disk_free_below_basis_points: Option<u16>,
}

impl BackgroundConfig {
    fn from_document(document: BackgroundDocument) -> Result<Self, ConfigError> {
        Ok(Self {
            enabled: document.enabled,
            mode: document.mode.map(BackgroundMode::parse).transpose()?,
            check_every: document
                .check_every
                .as_deref()
                .map(parse_config_duration)
                .transpose()?,
            only_when_disk_free_below_basis_points: document
                .only_when_disk_free_below
                .as_deref()
                .map(parse_config_percentage_basis_points)
                .transpose()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundMode {
    Periodic,
    Threshold,
}

impl BackgroundMode {
    fn parse(value: String) -> Result<Self, ConfigError> {
        match value.as_str() {
            "periodic" => Ok(Self::Periodic),
            "threshold" => Ok(Self::Threshold),
            _ => Err(ConfigError::InvalidBackgroundMode(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ScannerConfig {
    pub follow_symlinks: Option<bool>,
    pub allow_name_only_targets: Option<bool>,
    pub cross_filesystems: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SchedulerConfig {
    pub at: Option<String>,
    pub mode: Option<String>,
    pub policy: Option<String>,
    pub allow_unattended_cleanup: Option<bool>,
    pub allow_unattended_high_policy: Option<bool>,
    pub state_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
}

impl SchedulerConfig {
    pub(super) fn resolve_paths(mut self, relative_base: Option<&Path>) -> Self {
        self.state_dir = self
            .state_dir
            .map(|path| resolve_config_path(path, relative_base));
        self.log_dir = self
            .log_dir
            .map(|path| resolve_config_path(path, relative_base));
        self
    }
}
