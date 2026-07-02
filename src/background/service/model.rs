use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ReclaimError;
use crate::persistence::{PersistedTimestamp, PlanPersistenceError};
use crate::scheduler::{SchedulerError, SchedulerMode};

use super::super::BackgroundRunnerError;

pub const BACKGROUND_SERVICE_STATE_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_BACKGROUND_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

pub type BackgroundServiceResult<T> = Result<T, BackgroundServiceError>;

#[derive(Debug)]
pub enum BackgroundServiceError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Timestamp(PlanPersistenceError),
    Scheduler(SchedulerError),
    Runner(BackgroundRunnerError),
    Config(String),
    AlreadyRunning {
        lock_path: PathBuf,
    },
    StaleLock {
        lock_path: PathBuf,
    },
}

impl fmt::Display for BackgroundServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Serialize { path, source } => {
                write!(
                    formatter,
                    "failed to serialize {}: {source}",
                    path.display()
                )
            }
            Self::Json { path, source } => {
                write!(formatter, "failed to parse {}: {source}", path.display())
            }
            Self::Timestamp(source) => write!(formatter, "invalid service timestamp: {source}"),
            Self::Scheduler(source) => source.fmt(formatter),
            Self::Runner(source) => source.fmt(formatter),
            Self::Config(message) => formatter.write_str(message),
            Self::AlreadyRunning { lock_path } => write!(
                formatter,
                "cargo-reclaim scheduler service is already running; lock exists at {}",
                lock_path.display()
            ),
            Self::StaleLock { lock_path } => write!(
                formatter,
                "cargo-reclaim scheduler service lock exists at {}; stale-lock recovery is not automatic yet",
                lock_path.display()
            ),
        }
    }
}

impl Error for BackgroundServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Serialize { source, .. } | Self::Json { source, .. } => Some(source),
            Self::Timestamp(source) => Some(source),
            Self::Scheduler(source) => Some(source),
            Self::Runner(source) => Some(source),
            Self::Config(_) | Self::AlreadyRunning { .. } | Self::StaleLock { .. } => None,
        }
    }
}

impl From<SchedulerError> for BackgroundServiceError {
    fn from(error: SchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

impl From<BackgroundRunnerError> for BackgroundServiceError {
    fn from(error: BackgroundRunnerError) -> Self {
        Self::Runner(error)
    }
}

impl From<ReclaimError> for BackgroundServiceError {
    fn from(error: ReclaimError) -> Self {
        Self::Config(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundServiceOptions {
    pub config_path: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub mode: Option<SchedulerMode>,
    pub max_cycles: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundServicePaths {
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub plans_dir: PathBuf,
    pub lock_path: PathBuf,
    pub state_path: PathBuf,
    pub runs_log_path: PathBuf,
}

impl BackgroundServicePaths {
    pub fn new(state_dir: impl Into<PathBuf>, log_dir: impl Into<PathBuf>) -> Self {
        let state_dir = state_dir.into();
        let log_dir = log_dir.into();
        Self {
            plans_dir: state_dir.join("plans"),
            lock_path: state_dir.join("service.lock"),
            state_path: state_dir.join("service-state.json"),
            runs_log_path: log_dir.join("runs.jsonl"),
            state_dir,
            log_dir,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundServiceState {
    pub schema_version: u16,
    pub status: BackgroundServiceStatus,
    pub pid: Option<u32>,
    pub started_at: Option<PersistedTimestamp>,
    pub last_run_id: Option<String>,
    pub last_run_at: Option<PersistedTimestamp>,
    pub next_run_at: Option<PersistedTimestamp>,
    pub consecutive_failures: u32,
    pub last_problem: Option<String>,
}

impl BackgroundServiceState {
    pub fn missing() -> Self {
        Self {
            schema_version: BACKGROUND_SERVICE_STATE_SCHEMA_VERSION,
            status: BackgroundServiceStatus::Unknown,
            pid: None,
            started_at: None,
            last_run_id: None,
            last_run_at: None,
            next_run_at: None,
            consecutive_failures: 0,
            last_problem: Some("service state file does not exist".to_owned()),
        }
    }

    pub(crate) fn running(started_at: PersistedTimestamp) -> Self {
        Self {
            schema_version: BACKGROUND_SERVICE_STATE_SCHEMA_VERSION,
            status: BackgroundServiceStatus::Running,
            pid: Some(std::process::id()),
            started_at: Some(started_at),
            last_run_id: None,
            last_run_at: None,
            next_run_at: None,
            consecutive_failures: 0,
            last_problem: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundServiceStatus {
    Running,
    Stopped,
    Unknown,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundServiceRunSummary {
    pub state: BackgroundServiceState,
    pub cycles_completed: usize,
}
