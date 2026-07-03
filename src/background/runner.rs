use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::active_process::ActiveObservationProvider;
use crate::error::ReclaimError;
use crate::executor::{ApplyReport, execute_persisted_plan_apply};
use crate::integration::build_plan_from_roots_with_active_observation_provider;
use crate::inventory::InventoryOptions;
use crate::persistence::{
    PersistedTimestamp, PlanCommandKind, PlanId, PlanInvocation, PlanPersistenceError,
    SavePlanOptions, persist_plan, save_plan_to_path,
};
use crate::planner::PlannerOptions;
use crate::policy::PolicyKind;
use crate::scanner::ScannerOptions;
use crate::toolchain_hash::{ToolchainHashError, resolve_command_toolchain_hash_options};
use crate::watcher::{
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherTriggerReason,
    decide_watcher_thresholds,
};

use super::{
    BackgroundApplySummary, BackgroundPlanSummary, BackgroundRunEventKind, BackgroundRunLogError,
    BackgroundRunLogRecord, BackgroundTriggerSummary, append_background_run_log_record,
};

pub type BackgroundRunnerResult<T> = Result<T, BackgroundRunnerError>;

#[derive(Debug)]
pub enum BackgroundRunnerError {
    BuildPlan(ReclaimError),
    ToolchainHash(ToolchainHashError),
    PersistPlan(PlanPersistenceError),
    SavePlan(PlanPersistenceError),
    Apply(PlanPersistenceError),
    RunLog(BackgroundRunLogError),
    CreatePlanDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for BackgroundRunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildPlan(source) => {
                write!(formatter, "failed to build background plan: {source}")
            }
            Self::ToolchainHash(source) => {
                write!(
                    formatter,
                    "failed to resolve background toolchain hashes: {source}"
                )
            }
            Self::PersistPlan(source) => {
                write!(formatter, "failed to persist background plan: {source}")
            }
            Self::SavePlan(source) => write!(formatter, "failed to save background plan: {source}"),
            Self::Apply(source) => write!(formatter, "failed to apply background plan: {source}"),
            Self::RunLog(source) => {
                write!(formatter, "failed to write background run log: {source}")
            }
            Self::CreatePlanDirectory { path, source } => {
                write!(
                    formatter,
                    "failed to create plan directory {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for BackgroundRunnerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BuildPlan(source) => Some(source),
            Self::ToolchainHash(source) => Some(source),
            Self::PersistPlan(source) | Self::SavePlan(source) | Self::Apply(source) => {
                Some(source)
            }
            Self::RunLog(source) => Some(source),
            Self::CreatePlanDirectory { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundRunRequest {
    pub run_id: String,
    pub log_path: PathBuf,
    pub plan_path: PathBuf,
    pub roots: Vec<PathBuf>,
    pub policy: PolicyKind,
    pub scanner_options: ScannerOptions,
    pub inventory_options: InventoryOptions,
    pub planner_options: PlannerOptions,
    pub trigger: BackgroundRunTrigger,
    pub config_path: Option<PathBuf>,
    pub config_version: Option<u16>,
    pub created_at: SystemTime,
    pub now: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundRunTrigger {
    Decision(WatcherDecision),
    DecisionInput(WatcherDecisionInput),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundRunReport {
    pub run_id: String,
    pub decision: WatcherDecision,
    pub plan_id: Option<PlanId>,
    pub apply_report: Option<ApplyReport>,
}

pub fn run_background_cleanup_cycle(
    request: BackgroundRunRequest,
    active_observation_provider: &impl ActiveObservationProvider,
) -> BackgroundRunnerResult<BackgroundRunReport> {
    let started = BackgroundRunLogRecord::new(
        request.run_id.clone(),
        persisted_timestamp(request.now)?,
        BackgroundRunEventKind::Started,
    )
    .with_selected_policy(request.policy);
    append_record(&request.log_path, &started)?;

    match run_after_started(request, active_observation_provider) {
        Ok(report) => Ok(report),
        Err(error) => {
            let BackgroundRunFailureDetails {
                request,
                problem,
                source,
            } = *error;
            append_failed_record(&request, &problem)?;
            Err(source)
        }
    }
}

fn run_after_started(
    request: BackgroundRunRequest,
    active_observation_provider: &impl ActiveObservationProvider,
) -> Result<BackgroundRunReport, BackgroundRunFailure> {
    let decision = match &request.trigger {
        BackgroundRunTrigger::Decision(decision) => decision.clone(),
        BackgroundRunTrigger::DecisionInput(input) => decide_watcher_thresholds(input.clone()),
    };
    let trigger_summary = BackgroundTriggerSummary::from_watcher_decision(&decision);

    if should_skip(decision.state) {
        let skipped = BackgroundRunLogRecord::new(
            request.run_id.clone(),
            persisted_timestamp_or_failure(&request)?,
            BackgroundRunEventKind::Skipped,
        )
        .with_selected_policy(request.policy)
        .with_trigger(trigger_summary);
        append_record_or_failure(&request, &skipped)?;

        return Ok(BackgroundRunReport {
            run_id: request.run_id,
            decision,
            plan_id: None,
            apply_report: None,
        });
    }

    let triggered = BackgroundRunLogRecord::new(
        request.run_id.clone(),
        persisted_timestamp_or_failure(&request)?,
        BackgroundRunEventKind::Triggered,
    )
    .with_selected_policy(request.policy)
    .with_trigger(trigger_summary.clone());
    append_record_or_failure(&request, &triggered)?;

    let mut planner_options = request.planner_options.clone();
    apply_target_size_goal_budget(&mut planner_options, &decision);
    resolve_command_toolchain_hash_options(&mut planner_options)
        .map_err(|source| failure(&request, BackgroundRunnerError::ToolchainHash(source)))?;

    let plan = build_plan_from_roots_with_active_observation_provider(
        request.roots.clone(),
        request.policy,
        &request.scanner_options,
        &request.inventory_options,
        &planner_options,
        active_observation_provider,
        request.now,
    )
    .map_err(|source| failure(&request, BackgroundRunnerError::BuildPlan(source)))?;

    let invocation = plan_invocation(&request, &planner_options);
    let document = persist_plan(
        &plan,
        SavePlanOptions {
            created_at: request.created_at,
            expires_at: request.expires_at,
            interactive_selection_modified: false,
            invocation,
        },
    )
    .map_err(|source| failure(&request, BackgroundRunnerError::PersistPlan(source)))?;

    ensure_plan_parent(&request.plan_path).map_err(|source| failure(&request, source))?;
    save_plan_to_path(&request.plan_path, &document)
        .map_err(|source| failure(&request, BackgroundRunnerError::SavePlan(source)))?;

    let plan_summary =
        BackgroundPlanSummary::from_plan_id_and_totals(request.policy, &document.id, plan.totals);
    let plan_built = BackgroundRunLogRecord::new(
        request.run_id.clone(),
        persisted_timestamp_or_failure(&request)?,
        BackgroundRunEventKind::PlanBuilt,
    )
    .with_selected_policy(request.policy)
    .with_trigger(trigger_summary.clone())
    .with_plan(plan_summary)
    .with_plan_skipped_paths(&plan);
    append_record_or_failure(&request, &plan_built)?;

    let mut apply_report = None;
    if decision.state == WatcherDecisionState::TriggeredPlanAndApply {
        let report = execute_persisted_plan_apply(&document, request.now)
            .map_err(|source| failure(&request, BackgroundRunnerError::Apply(source)))?;
        let apply_completed = BackgroundRunLogRecord::new(
            request.run_id.clone(),
            persisted_timestamp_or_failure(&request)?,
            BackgroundRunEventKind::ApplyCompleted,
        )
        .with_selected_policy(request.policy)
        .with_trigger(trigger_summary)
        .with_plan(BackgroundPlanSummary::from_plan_id_and_totals(
            request.policy,
            &document.id,
            plan.totals,
        ))
        .with_plan_skipped_paths(&plan)
        .with_apply(BackgroundApplySummary::from_apply_report(&report));
        append_record_or_failure(&request, &apply_completed)?;
        apply_report = Some(report);
    }

    Ok(BackgroundRunReport {
        run_id: request.run_id,
        decision,
        plan_id: Some(document.id),
        apply_report,
    })
}

fn should_skip(state: WatcherDecisionState) -> bool {
    matches!(
        state,
        WatcherDecisionState::Inactive
            | WatcherDecisionState::NonThresholdMode
            | WatcherDecisionState::NotTriggered
    )
}

fn apply_target_size_goal_budget(options: &mut PlannerOptions, decision: &WatcherDecision) {
    let Some(target_size_goal_bytes) = options.target_size_goal_bytes else {
        apply_target_free_disk_budget(options, decision);
        return;
    };

    let required_reclaim_bytes = decision
        .reasons
        .iter()
        .filter_map(|reason| match reason {
            WatcherTriggerReason::TargetSizeExceeded { size_bytes, .. } => {
                size_bytes.checked_sub(target_size_goal_bytes)
            }
            WatcherTriggerReason::DiskFreeBelow { .. }
            | WatcherTriggerReason::DiskFreeBytesBelow { .. } => None,
        })
        .max();

    if let Some(required_reclaim_bytes) = required_reclaim_bytes {
        options.minimum_reclaim_bytes = Some(
            options
                .minimum_reclaim_bytes
                .unwrap_or(0)
                .max(required_reclaim_bytes),
        );
    }

    apply_target_free_disk_budget(options, decision);
}

fn apply_target_free_disk_budget(options: &mut PlannerOptions, decision: &WatcherDecision) {
    let Some(target_free_disk_bytes) = options.target_free_disk_bytes else {
        return;
    };

    let required_reclaim_bytes = decision
        .reasons
        .iter()
        .filter_map(|reason| match reason {
            WatcherTriggerReason::DiskFreeBytesBelow { free_bytes, .. } => {
                target_free_disk_bytes.checked_sub(*free_bytes)
            }
            WatcherTriggerReason::TargetSizeExceeded { .. }
            | WatcherTriggerReason::DiskFreeBelow { .. } => None,
        })
        .max();

    if let Some(required_reclaim_bytes) = required_reclaim_bytes {
        options.minimum_reclaim_bytes = Some(
            options
                .minimum_reclaim_bytes
                .unwrap_or(0)
                .max(required_reclaim_bytes),
        );
    }
}

fn plan_invocation(
    request: &BackgroundRunRequest,
    planner_options: &PlannerOptions,
) -> PlanInvocation {
    let invocation = PlanInvocation::new(
        PlanCommandKind::Plan,
        request.policy,
        &request.scanner_options,
        &request.inventory_options,
        planner_options,
    );

    match (&request.config_path, request.config_version) {
        (Some(path), Some(version)) => invocation.with_config(path, version),
        _ => invocation,
    }
}

fn ensure_plan_parent(path: &Path) -> BackgroundRunnerResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| {
            BackgroundRunnerError::CreatePlanDirectory {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }

    Ok(())
}

fn append_failed_record(
    request: &BackgroundRunRequest,
    problem: &str,
) -> BackgroundRunnerResult<()> {
    let mut record = BackgroundRunLogRecord::new(
        request.run_id.clone(),
        persisted_timestamp(request.now)?,
        BackgroundRunEventKind::Failed,
    )
    .with_selected_policy(request.policy);
    record.problems.push(problem.to_owned());
    append_record(&request.log_path, &record)
}

fn append_record(path: &Path, record: &BackgroundRunLogRecord) -> BackgroundRunnerResult<()> {
    append_background_run_log_record(path, record).map_err(BackgroundRunnerError::RunLog)
}

type BackgroundRunFailure = Box<BackgroundRunFailureDetails>;

struct BackgroundRunFailureDetails {
    request: BackgroundRunRequest,
    problem: String,
    source: BackgroundRunnerError,
}

fn append_record_or_failure(
    request: &BackgroundRunRequest,
    record: &BackgroundRunLogRecord,
) -> Result<(), BackgroundRunFailure> {
    append_record(&request.log_path, record).map_err(|source| failure(request, source))
}

fn persisted_timestamp(now: SystemTime) -> BackgroundRunnerResult<PersistedTimestamp> {
    PersistedTimestamp::from_system_time(now).map_err(BackgroundRunnerError::PersistPlan)
}

fn persisted_timestamp_or_failure(
    request: &BackgroundRunRequest,
) -> Result<PersistedTimestamp, BackgroundRunFailure> {
    persisted_timestamp(request.now).map_err(|source| failure(request, source))
}

fn failure(request: &BackgroundRunRequest, source: BackgroundRunnerError) -> BackgroundRunFailure {
    Box::new(BackgroundRunFailureDetails {
        request: request.clone(),
        problem: source.to_string(),
        source,
    })
}
