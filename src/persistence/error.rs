use std::fmt;
use std::path::PathBuf;

pub type PlanPersistenceResult<T> = Result<T, PlanPersistenceError>;

#[derive(Debug)]
pub enum PlanPersistenceError {
    InvalidTimeRange,
    TimestampBeforeUnixEpoch,
    Io { path: PathBuf, message: String },
    Json { message: String },
    PersistenceSchemaMismatch { found: u16, expected: u16 },
    PlanSchemaMismatch { found: u16, expected: u16 },
    PlanExpired,
    PlanIdMismatch { expected: String, found: String },
}

impl fmt::Display for PlanPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimeRange => {
                formatter.write_str("plan expiry must be after creation time")
            }
            Self::TimestampBeforeUnixEpoch => {
                formatter.write_str("plan timestamp must be at or after the Unix epoch")
            }
            Self::Io { path, message } => {
                write!(formatter, "failed to access {}: {message}", path.display())
            }
            Self::Json { message } => {
                write!(formatter, "failed to encode persisted plan: {message}")
            }
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
        Self::Json {
            message: error.to_string(),
        }
    }
}
