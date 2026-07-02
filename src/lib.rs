pub mod classifier;
pub mod error;
pub mod integration;
pub mod inventory;
pub mod model;
pub mod planner;
pub mod policy;
pub mod scanner;

pub use classifier::{Classifier, classify_target_relative_path};
pub use error::{ReclaimError, ReclaimResult};
pub use integration::{build_plan_from_roots, build_plan_from_scan_items};
pub use inventory::{
    InventoryOptions, planner_candidate_from_target_relative_path,
    planner_candidates_from_target_root, snapshot_target_relative_path,
};
pub use model::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathKind, PathSnapshot, Plan, PlanAction, PlanEntry,
    PlanInput, PlanTotals, TargetEvidence,
};
pub use planner::{PlannerCandidate, build_plan, plan_candidate};
pub use policy::PolicyKind;
pub use scanner::{
    CargoProject, ScanItem, ScanSkip, ScanSkipReason, ScannerOptions, SkipReason, TargetCandidate,
    TargetCandidateKind, TargetDirOverride, TargetDirOverrideSource, classify_target_candidate,
    detect_cargo_project, scan_roots,
};
