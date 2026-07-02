use std::fmt;
use std::path::PathBuf;

pub const CARGO_HOME_REPORT_SCHEMA_VERSION: u16 = 1;
pub const CARGO_HOME_PLAN_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoHomeSource {
    Explicit,
    CargoHomeEnv,
    HomeDefault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeInput {
    pub root: PathBuf,
    pub source: CargoHomeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoHomeClass {
    RegistryIndex,
    RegistryCache,
    RegistrySource,
    GitDatabase,
    GitCheckouts,
    Config,
    Credentials,
    InstalledBinaries,
    InstallMetadata,
    UnknownUserAuthored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoHomePathKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeEntry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub class: CargoHomeClass,
    pub path_kind: CargoHomePathKind,
    pub size_bytes: u64,
    pub preserved: bool,
    pub skipped: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeProblem {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub cache_bytes: u64,
    pub preserved_bytes: u64,
    pub skipped_count: usize,
    pub problem_count: usize,
    pub known_cache_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeRecommendation {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomeReport {
    pub schema_version: u16,
    pub input: CargoHomeInput,
    pub entries: Vec<CargoHomeEntry>,
    pub totals: CargoHomeTotals,
    pub recommendations: Vec<CargoHomeRecommendation>,
    pub problems: Vec<CargoHomeProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoHomePlanAction {
    DeleteCandidate,
    Preserve,
    SkipProblem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomePlanEntry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub class: CargoHomeClass,
    pub path_kind: CargoHomePathKind,
    pub size_bytes: u64,
    pub action: CargoHomePlanAction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomePlanTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub delete_candidate_count: usize,
    pub delete_candidate_bytes: u64,
    pub preserved_count: usize,
    pub preserved_bytes: u64,
    pub skipped_count: usize,
    pub problem_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoHomePlan {
    pub schema_version: u16,
    pub input: CargoHomeInput,
    pub policy: crate::PolicyKind,
    pub entries: Vec<CargoHomePlanEntry>,
    pub totals: CargoHomePlanTotals,
    pub recommendations: Vec<CargoHomeRecommendation>,
    pub problems: Vec<CargoHomeProblem>,
}

#[derive(Debug)]
pub enum CargoHomeError {
    NoCargoHome,
    RootMissing { path: PathBuf },
    RootNotDirectory { path: PathBuf },
    RootUnreadable { path: PathBuf, message: String },
}

impl fmt::Display for CargoHomeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCargoHome => formatter.write_str(
                "could not resolve Cargo home; pass --cargo-home or set CARGO_HOME, HOME, or USERPROFILE",
            ),
            Self::RootMissing { path } => {
                write!(formatter, "Cargo home does not exist: {}", path.display())
            }
            Self::RootNotDirectory { path } => {
                write!(formatter, "Cargo home is not a directory: {}", path.display())
            }
            Self::RootUnreadable { path, message } => {
                write!(formatter, "failed to read Cargo home {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for CargoHomeError {}
