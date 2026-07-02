pub mod classifier;
pub mod error;
pub mod model;
pub mod planner;
pub mod policy;
pub mod scanner;

pub use classifier::{Classifier, classify_target_relative_path};
pub use error::{ReclaimError, ReclaimResult};
pub use model::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathSnapshot, Plan, PlanAction, PlanEntry, PlanInput,
    PlanTotals, TargetEvidence,
};
pub use planner::{PlannerCandidate, build_plan, plan_candidate};
pub use policy::PolicyKind;
pub use scanner::{
    CargoProject, ScannerOptions, SkipReason, TargetCandidate, TargetCandidateKind,
    TargetDirOverride, TargetDirOverrideSource, classify_target_candidate, detect_cargo_project,
};
