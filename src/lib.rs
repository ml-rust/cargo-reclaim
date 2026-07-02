pub mod classifier;
pub mod error;
pub mod executor;
pub mod integration;
pub mod inventory;
pub mod model;
pub mod persistence;
pub mod planner;
pub mod policy;
pub mod scanner;

pub use classifier::{Classifier, classify_target_relative_path};
pub use error::{ReclaimError, ReclaimResult};
pub use executor::{
    ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals, execute_persisted_plan_apply,
    validate_persisted_plan_for_apply,
};
pub use integration::{build_plan_from_roots, build_plan_from_scan_items};
pub use inventory::{
    InventoryOptions, planner_candidate_from_target_relative_path,
    planner_candidates_from_target_root, snapshot_target_relative_path,
};
pub use model::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathKind, PathSnapshot, Plan, PlanAction, PlanEntry,
    PlanInput, PlanTotals, TargetEvidence,
};
pub use persistence::{
    PERSISTED_PLAN_SCHEMA_VERSION, PersistedEvidence, PersistedInventoryOptions,
    PersistedPathSnapshot, PersistedPlan, PersistedPlanBody, PersistedPlanEntry,
    PersistedPlanInput, PersistedPlanSnapshot, PersistedPlanTotals, PersistedScannerOptions,
    PersistedTimestamp, PlanCommandKind, PlanId, PlanInvocation, PlanPersistenceError,
    PlanPersistenceResult, SavePlanOptions, ensure_plan_usable, load_plan_from_path, persist_plan,
    save_plan_to_path,
};
pub use planner::{PlannerCandidate, build_plan, plan_candidate};
pub use policy::PolicyKind;
pub use scanner::{
    CargoProject, ScanItem, ScanSkip, ScanSkipReason, ScannerOptions, SkipReason, TargetCandidate,
    TargetCandidateKind, TargetDirOverride, TargetDirOverrideSource, classify_target_candidate,
    detect_cargo_project, scan_roots,
};
