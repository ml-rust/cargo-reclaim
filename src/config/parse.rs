use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::error::ConfigError;
use super::model::ReclaimConfig;

pub fn load_config_from_path(path: impl AsRef<Path>) -> Result<ReclaimConfig, ConfigError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let relative_base = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    parse_config_with_base(&contents, relative_base)
}

pub fn parse_config(contents: &str) -> Result<ReclaimConfig, ConfigError> {
    parse_config_with_base(contents, None)
}

fn parse_config_with_base(
    contents: &str,
    relative_base: Option<&Path>,
) -> Result<ReclaimConfig, ConfigError> {
    let document = toml::from_str::<ConfigDocument>(contents)?;
    ReclaimConfig::from_document(document, relative_base)
}

#[derive(Debug, Deserialize)]
pub(super) struct ConfigDocument {
    pub version: u16,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub ignore: Vec<PathBuf>,
    #[serde(default)]
    pub skip: Vec<PathBuf>,
    #[serde(default)]
    pub policy: Option<PolicyConfig>,
    #[serde(default)]
    pub scanner: Option<super::model::ScannerConfig>,
    #[serde(default)]
    pub planner: Option<PlannerConfig>,
    #[serde(default)]
    pub scheduler: Option<super::model::SchedulerConfig>,
    #[serde(default)]
    pub background: Option<BackgroundDocument>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PolicyConfig {
    pub mode: Option<String>,
    pub whole_target: Option<String>,
    pub allow_unattended_whole_target_delete: Option<bool>,
    pub keep_recent_projects: Option<String>,
    pub max_target_size: Option<String>,
    pub unattended_allowed: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PlannerConfig {
    pub recent_write_keep_window: Option<String>,
    pub keep_days: Option<u64>,
    pub keep_size: Option<String>,
    #[serde(default)]
    pub keep_rustc_hashes: Vec<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BackgroundDocument {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    pub check_every: Option<String>,
    pub only_when_disk_free_below: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{parse_config, parse_config_with_base};
    use crate::config::BackgroundMode;

    #[test]
    fn parses_supported_config_shape() -> Result<(), Box<dyn std::error::Error>> {
        let config = parse_config(
            r#"
version = 1
roots = ["projects"]
ignore = ["projects/pinned"]
skip = ["projects/vendor"]

[policy]
mode = "conservative"
whole_target = "confirm"
allow_unattended_whole_target_delete = false
keep_recent_projects = "3 days"
max_target_size = "5 GiB"
unattended_allowed = true
remove_classes = ["incremental"]
preserve_final_artifacts = true

[scanner]
follow_symlinks = true
allow_name_only_targets = true
cross_filesystems = true

[planner]
recent_write_keep_window = "4h"
keep_size = "64 MiB"
keep_rustc_hashes = [1, 2]

[scheduler]
at = "04:15"
mode = "cleanup"
policy = "conservative"
allow_unattended_cleanup = true
allow_unattended_high_policy = false
state_dir = "state"
log_dir = "logs"

[background]
enabled = true
mode = "threshold"
check_every = "15m"
only_when_disk_free_below = "12.5%"

[future]
field = true
"#,
        )?;

        assert_eq!(config.version, 1);
        assert_eq!(config.roots, [PathBuf::from("projects")]);
        assert_eq!(config.ignored_paths, [PathBuf::from("projects/pinned")]);
        assert_eq!(config.skipped_paths, [PathBuf::from("projects/vendor")]);
        assert_eq!(config.policy.as_deref(), Some("conservative"));
        assert_eq!(
            config.whole_target,
            Some(crate::config::WholeTargetConfig::Confirm)
        );
        assert_eq!(config.allow_unattended_whole_target_delete, Some(false));
        assert_eq!(
            config
                .recent_write_keep_window
                .expect("keep window")
                .as_secs(),
            4 * 60 * 60
        );
        assert_eq!(config.keep_size_bytes, Some(64 * 1024 * 1024));
        assert_eq!(config.keep_rustc_hashes, [1, 2]);
        assert_eq!(config.scanner.follow_symlinks, Some(true));
        assert_eq!(config.scanner.allow_name_only_targets, Some(true));
        assert_eq!(config.scanner.cross_filesystems, Some(true));
        assert_eq!(config.scheduler.at.as_deref(), Some("04:15"));
        assert_eq!(config.scheduler.mode.as_deref(), Some("cleanup"));
        assert_eq!(config.scheduler.policy.as_deref(), Some("conservative"));
        assert_eq!(config.scheduler.allow_unattended_cleanup, Some(true));
        assert_eq!(config.scheduler.allow_unattended_high_policy, Some(false));
        assert_eq!(config.scheduler.state_dir, Some(PathBuf::from("state")));
        assert_eq!(config.scheduler.log_dir, Some(PathBuf::from("logs")));
        assert_eq!(
            config.policy_thresholds.max_target_size_bytes,
            Some(5 * 1024 * 1024 * 1024)
        );
        assert_eq!(config.policy_thresholds.unattended_allowed, Some(true));
        assert_eq!(config.background.enabled, Some(true));
        assert_eq!(config.background.mode, Some(BackgroundMode::Threshold));
        assert_eq!(
            config
                .background
                .check_every
                .map(|duration| duration.as_secs()),
            Some(15 * 60)
        );
        assert_eq!(
            config.background.only_when_disk_free_below_basis_points,
            Some(1250)
        );
        Ok(())
    }

    #[test]
    fn resolves_relative_paths_against_config_directory() -> Result<(), Box<dyn std::error::Error>>
    {
        let config = parse_config_with_base(
            r#"
version = 1
roots = ["workspace", "/absolute"]
ignore = ["workspace/target"]
skip = ["workspace/vendor"]

[scheduler]
state_dir = "state"
log_dir = "logs"
"#,
            Some(Path::new("/tmp/reclaim-configs")),
        )?;

        assert_eq!(
            config.roots,
            [
                PathBuf::from("/tmp/reclaim-configs/workspace"),
                PathBuf::from("/absolute")
            ]
        );
        assert_eq!(
            config.ignored_paths,
            [PathBuf::from("/tmp/reclaim-configs/workspace/target")]
        );
        assert_eq!(
            config.skipped_paths,
            [PathBuf::from("/tmp/reclaim-configs/workspace/vendor")]
        );
        assert_eq!(
            config.scheduler.state_dir,
            Some(PathBuf::from("/tmp/reclaim-configs/state"))
        );
        assert_eq!(
            config.scheduler.log_dir,
            Some(PathBuf::from("/tmp/reclaim-configs/logs"))
        );
        Ok(())
    }

    #[test]
    fn parses_planner_keep_days_without_recent_write_window()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = parse_config(
            r#"
version = 1

[planner]
keep_days = 3
"#,
        )?;

        assert_eq!(
            config
                .recent_write_keep_window
                .expect("keep days window")
                .as_secs(),
            3 * 24 * 60 * 60
        );
        Ok(())
    }

    #[test]
    fn rejects_unsupported_version_and_invalid_durations() {
        assert!(matches!(
            parse_config("version = 2"),
            Err(super::ConfigError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            parse_config("version = 1\n[policy]\nkeep_recent_projects = \"0d\""),
            Err(super::ConfigError::InvalidDuration(_))
        ));
    }

    #[test]
    fn rejects_invalid_policy_threshold_sizes() {
        for value in ["0 GiB", "five GiB", "5 GB", "5"] {
            let contents = format!("version = 1\n[policy]\nmax_target_size = \"{value}\"");
            assert!(matches!(
                parse_config(&contents),
                Err(super::ConfigError::InvalidSize(_))
            ));
        }
    }

    #[test]
    fn rejects_invalid_background_disk_percentages() {
        for value in ["0%", "100.01%", "101%", "ten%", "12.345%", "12"] {
            let contents =
                format!("version = 1\n[background]\nonly_when_disk_free_below = \"{value}\"");
            assert!(matches!(
                parse_config(&contents),
                Err(super::ConfigError::InvalidPercentage(_))
            ));
        }
    }

    #[test]
    fn rejects_invalid_whole_target_mode() {
        assert!(matches!(
            parse_config("version = 1\n[policy]\nwhole_target = \"remove\""),
            Err(super::ConfigError::InvalidWholeTargetMode(value)) if value == "remove"
        ));
    }
}
