pub mod background;
pub mod cargo_home;
pub mod classifier;
pub mod config;
pub mod error;
pub mod executor;
pub mod integration;
pub mod inventory;
pub mod model;
pub mod persistence;
pub mod plan_edit;
pub mod planner;
pub mod policy;
pub mod scanner;
mod scheduler;
pub mod watcher;

pub use background::{
    BACKGROUND_RUN_LOG_SCHEMA_VERSION, BackgroundApplyEntrySummary, BackgroundApplySummary,
    BackgroundApplyTotals, BackgroundPlanSummary, BackgroundPlanTotals, BackgroundRunEventKind,
    BackgroundRunLogError, BackgroundRunLogRecord, BackgroundRunLogResult,
    BackgroundSkippedProject, BackgroundTriggerReasonSummary, BackgroundTriggerSummary,
    append_background_run_log_record, read_background_run_log,
};
pub use cargo_home::{
    CARGO_HOME_PLAN_SCHEMA_VERSION, CARGO_HOME_REPORT_SCHEMA_VERSION, CargoHomeClass,
    CargoHomeEntry, CargoHomeError, CargoHomeInput, CargoHomePathKind, CargoHomePlan,
    CargoHomePlanAction, CargoHomePlanEntry, CargoHomePlanRequest, CargoHomePlanTotals,
    CargoHomeProblem, CargoHomeRecommendation, CargoHomeReport, CargoHomeReportRequest,
    CargoHomeResolveRequest, CargoHomeSource, CargoHomeTotals, build_cargo_home_plan,
    build_cargo_home_plan_from_report, build_cargo_home_report, classify_cargo_home_relative_path,
    inventory_cargo_home, resolve_cargo_home,
};
pub use classifier::{Classifier, classify_target_relative_path};
pub use config::{
    BackgroundConfig, BackgroundMode, ConfigError, PolicyThresholdConfig, ReclaimConfig,
    load_config_from_path, parse_config,
};
pub use error::{ReclaimError, ReclaimResult};
pub use executor::{
    ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals, execute_persisted_plan_apply,
    validate_persisted_plan_for_apply,
};
pub use integration::{
    build_plan_from_roots, build_plan_from_roots_with_options, build_plan_from_scan_items,
    build_plan_from_scan_items_with_options,
};
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
    PersistedPlanInput, PersistedPlanSnapshot, PersistedPlanTotals, PersistedPlannerOptions,
    PersistedScannerOptions, PersistedTimestamp, PlanCommandKind, PlanId, PlanInvocation,
    PlanPersistenceError, PlanPersistenceResult, SavePlanOptions, ensure_plan_usable,
    load_plan_from_path, persist_plan, save_plan_to_path,
};
pub use plan_edit::{PlanEditError, PlanEditReport, PlanEditRequest, edit_persisted_plan};
pub use planner::{
    PlannerCandidate, PlannerOptions, build_plan, build_plan_with_options, plan_candidate,
    plan_candidate_with_options,
};
pub use policy::PolicyKind;
pub use scanner::{
    CargoConfigProblem, CargoConfigUnsupported, CargoConfigUnsupportedReason, CargoOutputDirs,
    CargoProject, ScanItem, ScanSkip, ScanSkipReason, ScannerOptions, SkipReason, TargetCandidate,
    TargetCandidateKind, TargetDirOverride, TargetDirOverrideSource, classify_target_candidate,
    detect_cargo_project, resolve_project_output_dirs, resolve_project_output_dirs_with_env,
    scan_roots,
};
pub use scheduler::{
    GeneratedArtifact, GeneratedArtifactKind, Schedule, SchedulerError, SchedulerMode,
    SchedulerOperation, SchedulerOperationPlan, SchedulerPlanStep, SchedulerPlatform,
    SchedulerReport, SchedulerRequest, artifact_kind_label, generate_scheduler_artifacts,
    mode_label, operation_label, plan_scheduler_install, plan_scheduler_uninstall, platform_label,
    policy_label,
};
pub use watcher::{
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherMode,
    WatcherObservedTarget, WatcherThresholds, WatcherTriggerReason, decide_watcher_thresholds,
};
