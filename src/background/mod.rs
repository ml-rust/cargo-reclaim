mod run_log;
mod runner;
mod service;

pub use run_log::{
    BACKGROUND_RUN_LOG_SCHEMA_VERSION, BackgroundApplyEntrySummary, BackgroundApplySummary,
    BackgroundApplyTotals, BackgroundPlanSummary, BackgroundPlanTotals, BackgroundRunEventKind,
    BackgroundRunLogError, BackgroundRunLogRecord, BackgroundRunLogResult,
    BackgroundSkippedProject, BackgroundTriggerReasonSummary, BackgroundTriggerSummary,
    append_background_run_log_record, read_background_run_log,
};
pub use runner::{
    BackgroundRunReport, BackgroundRunRequest, BackgroundRunTrigger, BackgroundRunnerError,
    BackgroundRunnerResult, run_background_cleanup_cycle,
};
pub use service::{
    BACKGROUND_SERVICE_STATE_SCHEMA_VERSION, BackgroundServiceClock, BackgroundServiceCycleRunner,
    BackgroundServiceError, BackgroundServiceOptions, BackgroundServicePaths,
    BackgroundServiceResult, BackgroundServiceRunSummary, BackgroundServiceSleeper,
    BackgroundServiceState, BackgroundServiceStatus, DEFAULT_BACKGROUND_CHECK_EVERY,
    PlatformBackgroundServiceCycleRunner, SystemBackgroundServiceClock,
    ThreadBackgroundServiceSleeper, read_background_service_state, run_background_service,
    run_background_service_with_runtime, write_background_service_state,
};
