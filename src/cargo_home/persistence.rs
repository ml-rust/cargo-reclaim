use std::fs::{self, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use serde::{Deserialize, Serialize};

use crate::persistence::{PersistedTimestamp, PlanId, PlanPersistenceError, PlanPersistenceResult};

use super::model::{
    CARGO_HOME_PLAN_SCHEMA_VERSION, CargoHomeClass, CargoHomePathKind, CargoHomePlan,
    CargoHomePlanAction, CargoHomePlanEntry, CargoHomeSource,
};

pub const CARGO_HOME_PERSISTED_PLAN_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomePlan {
    pub schema_version: u16,
    pub id: PlanId,
    #[serde(flatten)]
    pub body: PersistedCargoHomePlanBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomePlanBody {
    pub created_at: PersistedTimestamp,
    pub expires_at: PersistedTimestamp,
    pub command: String,
    pub policy: String,
    pub plan: PersistedCargoHomePlanSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveCargoHomePlanOptions {
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomePlanSnapshot {
    pub schema_version: u16,
    pub input: PersistedCargoHomeInput,
    pub entries: Vec<PersistedCargoHomePlanEntry>,
    pub totals: PersistedCargoHomePlanTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomeInput {
    pub root: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomePlanEntry {
    pub path: String,
    pub relative_path: String,
    pub class: String,
    pub action: String,
    pub path_kind: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<PersistedTimestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_fingerprint: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCargoHomePlanTotals {
    pub entry_count: usize,
    pub total_bytes: u64,
    pub delete_candidate_count: usize,
    pub delete_candidate_bytes: u64,
    pub preserved_count: usize,
    pub preserved_bytes: u64,
    pub skipped_count: usize,
    pub problem_count: usize,
}

pub fn persist_cargo_home_plan(
    plan: &CargoHomePlan,
    options: SaveCargoHomePlanOptions,
) -> PlanPersistenceResult<PersistedCargoHomePlan> {
    if options.expires_at <= options.created_at {
        return Err(PlanPersistenceError::InvalidTimeRange);
    }

    let body = PersistedCargoHomePlanBody {
        created_at: PersistedTimestamp::from_system_time(options.created_at)?,
        expires_at: PersistedTimestamp::from_system_time(options.expires_at)?,
        command: "cargo-home plan".to_string(),
        policy: policy_label(plan.policy).to_string(),
        plan: PersistedCargoHomePlanSnapshot::from_plan(plan)?,
    };
    let id = PlanId::from_body(&body)?;

    Ok(PersistedCargoHomePlan {
        schema_version: CARGO_HOME_PERSISTED_PLAN_SCHEMA_VERSION,
        id,
        body,
    })
}

pub fn ensure_cargo_home_plan_usable(
    document: &PersistedCargoHomePlan,
    now: SystemTime,
) -> PlanPersistenceResult<()> {
    if document.schema_version != CARGO_HOME_PERSISTED_PLAN_SCHEMA_VERSION {
        return Err(PlanPersistenceError::PersistenceSchemaMismatch {
            found: document.schema_version,
            expected: CARGO_HOME_PERSISTED_PLAN_SCHEMA_VERSION,
        });
    }

    if document.body.plan.schema_version != CARGO_HOME_PLAN_SCHEMA_VERSION {
        return Err(PlanPersistenceError::PlanSchemaMismatch {
            found: document.body.plan.schema_version,
            expected: CARGO_HOME_PLAN_SCHEMA_VERSION,
        });
    }

    if document.body.command != "cargo-home plan" {
        return Err(PlanPersistenceError::InvalidPlan {
            message: format!(
                "persisted Cargo home plan command mismatch: expected cargo-home plan, found {}",
                document.body.command
            ),
        });
    }

    if !is_known_policy_label(&document.body.policy) {
        return Err(PlanPersistenceError::InvalidPlan {
            message: format!(
                "persisted Cargo home plan policy is not recognized: {}",
                document.body.policy
            ),
        });
    }

    let root = Path::new(&document.body.plan.input.root);
    if !root.is_absolute() {
        return Err(PlanPersistenceError::InvalidPlan {
            message: "persisted Cargo home root must be absolute".to_string(),
        });
    }

    let expected_id = PlanId::from_body(&document.body)?;
    if expected_id != document.id {
        return Err(PlanPersistenceError::PlanIdMismatch {
            expected: expected_id.0,
            found: document.id.0.clone(),
        });
    }

    if now >= document.body.expires_at.to_system_time() {
        return Err(PlanPersistenceError::PlanExpired);
    }

    Ok(())
}

pub fn save_cargo_home_plan_to_path(
    path: impl AsRef<Path>,
    document: &PersistedCargoHomePlan,
) -> PlanPersistenceResult<()> {
    let path = path.as_ref();
    let temp_path = temp_sibling_path(path);
    let bytes = serde_json::to_vec_pretty(document)?;

    fs::write(&temp_path, bytes).map_err(|error| io_error(&temp_path, error))?;
    fs::rename(&temp_path, path).map_err(|error| io_error(path, error))?;
    Ok(())
}

pub fn load_cargo_home_plan_from_path(
    path: impl AsRef<Path>,
) -> PlanPersistenceResult<PersistedCargoHomePlan> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
    Ok(serde_json::from_slice(&bytes)?)
}

impl PersistedCargoHomePlanSnapshot {
    fn from_plan(plan: &CargoHomePlan) -> PlanPersistenceResult<Self> {
        let root = canonical_cargo_home_root(&plan.input.root)?;
        Ok(Self {
            schema_version: plan.schema_version,
            input: PersistedCargoHomeInput {
                root: path_string(&root),
                source: source_label(plan.input.source).to_string(),
            },
            entries: plan
                .entries
                .iter()
                .map(|entry| PersistedCargoHomePlanEntry::from_entry(entry, &root))
                .collect::<PlanPersistenceResult<Vec<_>>>()?,
            totals: PersistedCargoHomePlanTotals {
                entry_count: plan.totals.entry_count,
                total_bytes: plan.totals.total_bytes,
                delete_candidate_count: plan.totals.delete_candidate_count,
                delete_candidate_bytes: plan.totals.delete_candidate_bytes,
                preserved_count: plan.totals.preserved_count,
                preserved_bytes: plan.totals.preserved_bytes,
                skipped_count: plan.totals.skipped_count,
                problem_count: plan.totals.problem_count,
            },
        })
    }
}

impl PersistedCargoHomePlanEntry {
    fn from_entry(entry: &CargoHomePlanEntry, root: &Path) -> PlanPersistenceResult<Self> {
        let path = root.join(&entry.relative_path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|modified| PersistedTimestamp::from_system_time(modified).ok());

        Ok(Self {
            path: path_string(&path),
            relative_path: path_string(&entry.relative_path),
            class: class_label(entry.class).to_string(),
            action: action_label(entry.action).to_string(),
            path_kind: path_kind_label(entry.path_kind).to_string(),
            size_bytes: entry.size_bytes,
            modified,
            content_fingerprint: if entry.action == CargoHomePlanAction::DeleteCandidate {
                Some(fingerprint_path(&path, &metadata)?)
            } else {
                None
            },
            reason: entry.reason.clone(),
        })
    }
}

fn canonical_cargo_home_root(root: &Path) -> PlanPersistenceResult<PathBuf> {
    let metadata = fs::symlink_metadata(root).map_err(|error| io_error(root, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PlanPersistenceError::InvalidPlan {
            message: format!(
                "Cargo home root must be a real directory for persisted plans: {}",
                root.display()
            ),
        });
    }
    root.canonicalize().map_err(|error| io_error(root, error))
}

pub(crate) fn fingerprint_path(path: &Path, metadata: &Metadata) -> PlanPersistenceResult<String> {
    let mut hasher = Sha256::new();
    fingerprint_path_into(path, Path::new(""), metadata, &mut hasher)?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn fingerprint_path_into(
    path: &Path,
    relative_path: &Path,
    metadata: &Metadata,
    hasher: &mut Sha256,
) -> PlanPersistenceResult<()> {
    let kind = if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else {
        "other"
    };
    hasher.update(relative_path.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    hasher.update([0]);
    if let Ok(modified) = metadata.modified()
        && let Ok(timestamp) = PersistedTimestamp::from_system_time(modified)
    {
        hasher.update(timestamp.unix_seconds.to_le_bytes());
        hasher.update(timestamp.nanoseconds.to_le_bytes());
    }
    hasher.update([0]);

    if metadata.is_file() {
        let mut file = fs::File::open(path).map_err(|error| io_error(path, error))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| io_error(path, error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hasher.update([0]);
    }

    if metadata.is_dir() {
        let mut children = Vec::new();
        for child in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let child = child.map_err(|error| io_error(path, error))?;
            children.push(child.path());
        }
        children.sort();
        for child_path in children {
            let child_name =
                child_path
                    .file_name()
                    .ok_or_else(|| PlanPersistenceError::InvalidPlan {
                        message: format!(
                            "failed to read Cargo home child path under {}",
                            path.display()
                        ),
                    })?;
            let child_relative_path = if relative_path.as_os_str().is_empty() {
                PathBuf::from(child_name)
            } else {
                relative_path.join(child_name)
            };
            let child_metadata =
                fs::symlink_metadata(&child_path).map_err(|error| io_error(&child_path, error))?;
            fingerprint_path_into(&child_path, &child_relative_path, &child_metadata, hasher)?;
        }
    }

    Ok(())
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|file_name| file_name.to_string_lossy().to_string())
        .unwrap_or_else(|| "cargo-home-plan".to_string());
    path.with_file_name(format!(".{file_name}.tmp"))
}

fn io_error(path: &Path, error: std::io::Error) -> PlanPersistenceError {
    PlanPersistenceError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

pub(crate) fn policy_label(policy: crate::PolicyKind) -> &'static str {
    match policy {
        crate::PolicyKind::Observe => "observe",
        crate::PolicyKind::Conservative => "conservative",
        crate::PolicyKind::Balanced => "balanced",
        crate::PolicyKind::Aggressive => "aggressive",
        crate::PolicyKind::Custom => "custom",
    }
}

fn is_known_policy_label(policy: &str) -> bool {
    matches!(
        policy,
        "observe" | "conservative" | "balanced" | "aggressive" | "custom"
    )
}

pub(crate) fn source_label(source: CargoHomeSource) -> &'static str {
    match source {
        CargoHomeSource::Explicit => "explicit",
        CargoHomeSource::CargoHomeEnv => "cargo_home_env",
        CargoHomeSource::HomeDefault => "home_default",
    }
}

pub(crate) fn class_label(class: CargoHomeClass) -> &'static str {
    match class {
        CargoHomeClass::RegistryIndex => "registry_index",
        CargoHomeClass::RegistryCache => "registry_cache",
        CargoHomeClass::RegistrySource => "registry_source",
        CargoHomeClass::GitDatabase => "git_database",
        CargoHomeClass::GitCheckouts => "git_checkouts",
        CargoHomeClass::Config => "config",
        CargoHomeClass::Credentials => "credentials",
        CargoHomeClass::InstalledBinaries => "installed_binaries",
        CargoHomeClass::InstallMetadata => "install_metadata",
        CargoHomeClass::UnknownUserAuthored => "unknown_user_authored",
    }
}

pub(crate) fn action_label(action: CargoHomePlanAction) -> &'static str {
    match action {
        CargoHomePlanAction::DeleteCandidate => "delete_candidate",
        CargoHomePlanAction::Preserve => "preserve",
        CargoHomePlanAction::SkipProblem => "skip_problem",
    }
}

pub(crate) fn path_kind_label(kind: CargoHomePathKind) -> &'static str {
    match kind {
        CargoHomePathKind::File => "file",
        CargoHomePathKind::Directory => "directory",
        CargoHomePathKind::Symlink => "symlink",
        CargoHomePathKind::Other => "other",
    }
}
