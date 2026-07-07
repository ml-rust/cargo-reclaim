use std::fmt;
use std::path::PathBuf;

pub type PlanPersistenceResult<T> = Result<T, PlanPersistenceError>;

#[derive(Debug)]
pub enum PlanPersistenceError {
    InvalidTimeRange,
    InvalidPlan {
        message: String,
    },
    TimestampBeforeUnixEpoch,
    Io {
        path: PathBuf,
        message: String,
    },
    Encode {
        message: String,
    },
    Decode {
        path: PathBuf,
        message: String,
    },
    DryRunReport {
        path: PathBuf,
        source_command: String,
        expected_command: String,
    },
    UnrecognizedDocument {
        path: PathBuf,
    },
    PersistenceSchemaMismatch {
        found: u16,
        expected: u16,
    },
    PlanSchemaMismatch {
        found: u16,
        expected: u16,
    },
    PlanExpired,
    PlanIdMismatch {
        expected: String,
        found: String,
    },
}

impl fmt::Display for PlanPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimeRange => {
                formatter.write_str("plan expiry must be after creation time")
            }
            Self::InvalidPlan { message } => formatter.write_str(message),
            Self::TimestampBeforeUnixEpoch => {
                formatter.write_str("plan timestamp must be at or after the Unix epoch")
            }
            Self::Io { path, message } => {
                write!(formatter, "failed to access {}: {message}", path.display())
            }
            Self::Encode { message } => {
                write!(formatter, "failed to encode persisted plan: {message}")
            }
            Self::Decode { path, message } => write!(
                formatter,
                "failed to read persisted plan {}: {message}",
                path.display()
            ),
            Self::DryRunReport {
                path,
                source_command,
                expected_command,
            } => write!(
                formatter,
                "{} is a dry-run report from `{source_command} --json`, not an executable plan; regenerate it with `{expected_command} --save-plan <path>` and apply that file",
                path.display()
            ),
            Self::UnrecognizedDocument { path } => write!(
                formatter,
                "{} is not a recognizable cargo-reclaim plan document",
                path.display()
            ),
            Self::PersistenceSchemaMismatch { found, expected } => write!(
                formatter,
                "persisted plan schema mismatch: found {found}, expected {expected}"
            ),
            Self::PlanSchemaMismatch { found, expected } => write!(
                formatter,
                "embedded plan schema mismatch: found {found}, expected {expected}"
            ),
            Self::PlanExpired => formatter.write_str("persisted plan has expired"),
            Self::PlanIdMismatch { expected, found } => {
                write!(
                    formatter,
                    "persisted plan id mismatch: expected {expected}, found {found}"
                )
            }
        }
    }
}

impl std::error::Error for PlanPersistenceError {}

impl From<serde_json::Error> for PlanPersistenceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Encode {
            message: error.to_string(),
        }
    }
}
