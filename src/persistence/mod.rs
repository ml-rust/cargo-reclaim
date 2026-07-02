mod document;
mod error;
mod fingerprint;
mod fs;
mod id;
mod time;

pub use document::{
    PersistedEvidence, PersistedInventoryOptions, PersistedPathSnapshot, PersistedPlan,
    PersistedPlanBody, PersistedPlanEntry, PersistedPlanInput, PersistedPlanSkip,
    PersistedPlanSnapshot, PersistedPlanTotals, PersistedPlannerOptions, PersistedScannerOptions,
    PersistedWholeTargetMode, PlanCommandKind, PlanInvocation, SavePlanOptions, ensure_plan_usable,
    persist_plan,
};
pub use error::{PlanPersistenceError, PlanPersistenceResult};
pub(crate) use fingerprint::fingerprint_path;
pub use fs::{load_plan_from_path, save_plan_to_path};
pub use id::PlanId;
pub use time::PersistedTimestamp;

pub const PERSISTED_PLAN_SCHEMA_VERSION: u16 = 1;
