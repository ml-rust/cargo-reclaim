use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimConfig {
    pub version: u16,
    pub roots: Vec<PathBuf>,
    pub ignored_paths: Vec<PathBuf>,
    pub policy: Option<String>,
    pub scanner: ScannerConfig,
    pub scheduler: SchedulerConfig,
    pub recent_write_keep_window: Option<Duration>,
}

impl ReclaimConfig {
    fn from_document(
        document: ConfigDocument,
        relative_base: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        if document.version != 1 {
            return Err(ConfigError::UnsupportedVersion(document.version));
        }

        let policy_keep_recent_projects = document
            .policy
            .as_ref()
            .and_then(|policy| policy.keep_recent_projects.as_deref())
            .map(parse_config_duration)
            .transpose()?;
        let planner_recent_write_keep_window = document
            .planner
            .as_ref()
            .and_then(|planner| planner.recent_write_keep_window.as_deref())
            .map(parse_config_duration)
            .transpose()?;

        Ok(Self {
            version: document.version,
            roots: document
                .roots
                .into_iter()
                .map(|path| resolve_config_path(path, relative_base))
                .collect(),
            ignored_paths: document
                .ignore
                .into_iter()
                .map(|path| resolve_config_path(path, relative_base))
                .collect(),
            policy: document.policy.and_then(|policy| policy.mode),
            scanner: document.scanner.unwrap_or_default(),
            scheduler: document
                .scheduler
                .unwrap_or_default()
                .resolve_paths(relative_base),
            recent_write_keep_window: planner_recent_write_keep_window
                .or(policy_keep_recent_projects),
        })
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
    fn resolve_paths(mut self, relative_base: Option<&Path>) -> Self {
        self.state_dir = self
            .state_dir
            .map(|path| resolve_config_path(path, relative_base));
        self.log_dir = self
            .log_dir
            .map(|path| resolve_config_path(path, relative_base));
        self
    }
}

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

fn parse_config_duration(value: &str) -> Result<Duration, ConfigError> {
    let trimmed = value.trim();
    let Some((amount, unit)) = trimmed.split_once(' ') else {
        return parse_compact_duration(trimmed);
    };
    let amount = parse_positive_amount(amount, value)?;
    let seconds = match unit {
        "second" | "seconds" => amount,
        "minute" | "minutes" => amount.saturating_mul(60),
        "hour" | "hours" => amount.saturating_mul(60 * 60),
        "day" | "days" => amount.saturating_mul(24 * 60 * 60),
        _ => return Err(ConfigError::InvalidDuration(value.to_string())),
    };
    Ok(Duration::from_secs(seconds))
}

fn parse_compact_duration(value: &str) -> Result<Duration, ConfigError> {
    let Some((number, suffix)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return Err(ConfigError::InvalidDuration(value.to_string()));
    };
    let amount = parse_positive_amount(number, value)?;
    let seconds = match suffix {
        "s" => amount,
        "m" => amount.saturating_mul(60),
        "h" => amount.saturating_mul(60 * 60),
        "d" => amount.saturating_mul(24 * 60 * 60),
        _ => return Err(ConfigError::InvalidDuration(value.to_string())),
    };
    Ok(Duration::from_secs(seconds))
}

fn parse_positive_amount(value: &str, original: &str) -> Result<u64, ConfigError> {
    let amount = value
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidDuration(original.to_string()))?;
    if amount == 0 {
        return Err(ConfigError::InvalidDuration(original.to_string()));
    }
    Ok(amount)
}

fn expand_home(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    let Some(rest) = text.strip_prefix("~/") else {
        return path;
    };
    let Some(home) = home_dir() else {
        return path;
    };
    home.join(rest)
}

fn resolve_config_path(path: PathBuf, relative_base: Option<&Path>) -> PathBuf {
    let expanded = expand_home(path);
    if expanded.is_absolute() {
        return expanded;
    }
    relative_base
        .map(|base| base.join(&expanded))
        .unwrap_or(expanded)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[derive(Debug, Deserialize)]
struct ConfigDocument {
    version: u16,
    #[serde(default)]
    roots: Vec<PathBuf>,
    #[serde(default)]
    ignore: Vec<PathBuf>,
    #[serde(default)]
    policy: Option<PolicyConfig>,
    #[serde(default)]
    scanner: Option<ScannerConfig>,
    #[serde(default)]
    planner: Option<PlannerConfig>,
    #[serde(default)]
    scheduler: Option<SchedulerConfig>,
}

#[derive(Debug, Deserialize)]
struct PolicyConfig {
    mode: Option<String>,
    keep_recent_projects: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlannerConfig {
    recent_write_keep_window: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse(toml::de::Error),
    UnsupportedVersion(u16),
    InvalidDuration(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "failed to read config {}: {source}",
                    path.display()
                )
            }
            Self::Parse(error) => write!(formatter, "failed to parse config: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported config version {version}")
            }
            Self::InvalidDuration(value) => write!(formatter, "invalid config duration `{value}`"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<toml::de::Error> for ConfigError {
    fn from(error: toml::de::Error) -> Self {
        Self::Parse(error)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{ConfigError, parse_config, parse_config_with_base};

    #[test]
    fn parses_supported_config_shape() -> Result<(), Box<dyn std::error::Error>> {
        let config = parse_config(
            r#"
version = 1
roots = ["projects"]
ignore = ["projects/pinned"]

[policy]
mode = "conservative"
keep_recent_projects = "3 days"
remove_classes = ["incremental"]
preserve_final_artifacts = true

[scanner]
follow_symlinks = true
allow_name_only_targets = true
cross_filesystems = true

[planner]
recent_write_keep_window = "4h"

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
mode = "periodic"

[future]
field = true
"#,
        )?;

        assert_eq!(config.version, 1);
        assert_eq!(config.roots, [PathBuf::from("projects")]);
        assert_eq!(config.ignored_paths, [PathBuf::from("projects/pinned")]);
        assert_eq!(config.policy.as_deref(), Some("conservative"));
        assert_eq!(
            config
                .recent_write_keep_window
                .expect("keep window")
                .as_secs(),
            4 * 60 * 60
        );
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
    fn rejects_unsupported_version_and_invalid_durations() {
        assert!(matches!(
            parse_config("version = 2"),
            Err(ConfigError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            parse_config("version = 1\n[policy]\nkeep_recent_projects = \"0d\""),
            Err(ConfigError::InvalidDuration(_))
        ));
    }
}
