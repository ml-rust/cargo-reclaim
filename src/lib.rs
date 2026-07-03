pub mod active_process;
pub mod background;
pub mod cargo_home;
pub mod classifier;
pub mod config;
pub mod disk;
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
mod toolchain_hash;
pub mod watcher;

pub use active_process::{
    ActiveObservationProvider, ActiveObservationScope, ProcfsActiveObservationProvider,
    SysinfoActiveObservationProvider, platform_active_observation_provider,
};
pub use background::{
    BACKGROUND_RUN_LOG_SCHEMA_VERSION, BACKGROUND_SERVICE_STATE_SCHEMA_VERSION,
    BackgroundApplyEntrySummary, BackgroundApplySummary, BackgroundApplyTotals,
    BackgroundPlanSummary, BackgroundPlanTotals, BackgroundRunEventKind, BackgroundRunLogError,
    BackgroundRunLogRecord, BackgroundRunLogResult, BackgroundRunReport, BackgroundRunRequest,
    BackgroundRunTrigger, BackgroundRunnerError, BackgroundRunnerResult, BackgroundServiceClock,
    BackgroundServiceCycleRunner, BackgroundServiceError, BackgroundServiceOptions,
    BackgroundServicePaths, BackgroundServiceResult, BackgroundServiceRunSummary,
    BackgroundServiceSleeper, BackgroundServiceState, BackgroundServiceStatus,
    BackgroundSkippedProject, BackgroundTriggerReasonSummary, BackgroundTriggerSummary,
    DEFAULT_BACKGROUND_CHECK_EVERY, PlatformBackgroundServiceCycleRunner,
    SystemBackgroundServiceClock, ThreadBackgroundServiceSleeper, append_background_run_log_record,
    read_background_run_log, read_background_service_state, refresh_background_service_state,
    run_background_cleanup_cycle, run_background_service, run_background_service_with_runtime,
    write_background_service_state,
};
pub use cargo_home::{
    CARGO_HOME_PERSISTED_PLAN_SCHEMA_VERSION, CARGO_HOME_PLAN_SCHEMA_VERSION,
    CARGO_HOME_REPORT_SCHEMA_VERSION, CargoHomeApplyEntryResult, CargoHomeApplyEntryStatus,
    CargoHomeApplyReport, CargoHomeApplyTotals, CargoHomeClass, CargoHomeEntry, CargoHomeError,
    CargoHomeInput, CargoHomePathKind, CargoHomePlan, CargoHomePlanAction, CargoHomePlanEntry,
    CargoHomePlanRequest, CargoHomePlanTotals, CargoHomeProblem, CargoHomeRecommendation,
    CargoHomeReport, CargoHomeReportRequest, CargoHomeResolveRequest, CargoHomeSource,
    CargoHomeTotals, PersistedCargoHomeInput, PersistedCargoHomePlan, PersistedCargoHomePlanBody,
    PersistedCargoHomePlanEntry, PersistedCargoHomePlanSnapshot, PersistedCargoHomePlanTotals,
    SaveCargoHomePlanOptions, build_cargo_home_plan, build_cargo_home_plan_from_report,
    build_cargo_home_report, classify_cargo_home_relative_path, ensure_cargo_home_plan_usable,
    execute_cargo_home_plan_apply, inventory_cargo_home, load_cargo_home_plan_from_path,
    persist_cargo_home_plan, resolve_cargo_home, save_cargo_home_plan_to_path,
    validate_cargo_home_plan_for_apply,
};
pub use classifier::{Classifier, classify_target_relative_path};
pub use config::{
    BackgroundConfig, BackgroundMode, CARGO_CONFIG_RECOMMEND_SCHEMA_VERSION, CargoConfigOutputDir,
    CargoConfigRecommendReport, CargoConfigRecommendRequest, CargoConfigRecommendation,
    ConfigError, PolicyThresholdConfig, ReclaimConfig, WholeTargetConfig,
    build_cargo_config_recommend_report, load_config_from_path, parse_config,
};
pub use disk::{DiskFreeSpace, disk_free_basis_points, disk_free_space};
pub use error::{ReclaimError, ReclaimResult};
pub use executor::{
    ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals, execute_persisted_plan_apply,
    validate_persisted_plan_for_apply,
};
pub use integration::{
    BuildPlanFromScanItemsRequest, BuildPlanFromScanItemsWithProviderRequest,
    active_observation_scope_from_scan_items, build_plan_from_roots,
    build_plan_from_roots_with_active_observation,
    build_plan_from_roots_with_active_observation_provider, build_plan_from_roots_with_options,
    build_plan_from_scan_items, build_plan_from_scan_items_with_active_observation,
    build_plan_from_scan_items_with_active_observation_provider,
    build_plan_from_scan_items_with_options,
};
pub use inventory::{
    InventoryOptions, planner_candidate_from_target_relative_path,
    planner_candidate_from_target_relative_path_with_context, planner_candidates_from_target_root,
    planner_candidates_from_target_root_with_context, snapshot_path, snapshot_target_relative_path,
};
pub use model::{
    ArtifactClass, PLAN_SCHEMA_VERSION, PathKind, PathSnapshot, Plan, PlanAction, PlanEntry,
    PlanInput, PlanSkip, PlanSkipReason, PlanTotals, TargetEvidence,
};
pub use persistence::{
    PERSISTED_PLAN_SCHEMA_VERSION, PersistedEvidence, PersistedInventoryOptions,
    PersistedPathSnapshot, PersistedPlan, PersistedPlanBody, PersistedPlanEntry,
    PersistedPlanInput, PersistedPlanSkip, PersistedPlanSnapshot, PersistedPlanTotals,
    PersistedPlannerOptions, PersistedScannerOptions, PersistedTimestamp, PersistedWholeTargetMode,
    PlanCommandKind, PlanId, PlanInvocation, PlanPersistenceError, PlanPersistenceResult,
    SavePlanOptions, ensure_plan_usable, load_plan_from_path, persist_plan, save_plan_to_path,
};
pub use plan_edit::{PlanEditError, PlanEditReport, PlanEditRequest, edit_persisted_plan};
pub use planner::{
    ActiveObservation, CargoTool, ObservedCargoProcess, PlannerCandidate, PlannerOptions,
    ProcessView, TargetContext, WholeTargetMode, build_plan, build_plan_with_active_observation,
    build_plan_with_options, plan_candidate, plan_candidate_with_active_observation,
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
pub use scheduler::RealSchedulerOperationBackend;
pub use scheduler::{
    DEFAULT_SCHEDULER_INSTANCE_NAME, GeneratedArtifact, GeneratedArtifactKind, RemoveFileOutcome,
    Schedule, SchedulerCommandOutput, SchedulerError, SchedulerExecutionReport,
    SchedulerExecutionStatus, SchedulerExecutionStep, SchedulerExecutionTotals, SchedulerMode,
    SchedulerOperation, SchedulerOperationBackend, SchedulerOperationPlan, SchedulerPlanStep,
    SchedulerPlatform, SchedulerReport, SchedulerRequest, artifact_kind_label,
    default_instance_log_dir, default_instance_state_dir, default_log_dir, default_state_dir,
    execute_scheduler_operation, execution_status_label, generate_scheduler_artifacts, mode_label,
    operation_label, plan_scheduler_install, plan_scheduler_uninstall, platform_label,
    policy_label, scheduler_instance_name_from_config,
};
pub use toolchain_hash::{
    ToolchainHashError, ToolchainHashResolver, ToolchainHashResult,
    resolve_command_toolchain_hash_options, resolve_toolchain_hash_options,
};
pub use watcher::{
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherMode,
    WatcherObservedTarget, WatcherThresholds, WatcherTriggerReason, decide_watcher_thresholds,
};
