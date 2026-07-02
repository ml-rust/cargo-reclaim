use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ActiveObservation, ActiveObservationProvider, ActiveObservationScope, BackgroundRunEventKind,
    BackgroundRunRequest, BackgroundRunTrigger, BackgroundRunnerError, InventoryOptions,
    PlannerOptions, PolicyKind, ScannerOptions, WatcherDecision, WatcherDecisionState,
    WatcherTriggerReason, read_background_run_log, run_background_cleanup_cycle,
};

#[test]
fn skipped_threshold_appends_started_and_skipped_without_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_runner_skipped")?;
    let request = request(
        &temp,
        "run-skipped",
        WatcherDecision {
            state: WatcherDecisionState::NotTriggered,
            reasons: Vec::new(),
        },
    )?;

    let report = run_background_cleanup_cycle(request, &NoopActiveProvider)?;

    assert_eq!(report.plan_id, None);
    assert!(!temp.path.join("plan.json").exists());
    assert_events(
        &temp.path.join("runs.jsonl"),
        &[
            BackgroundRunEventKind::Started,
            BackgroundRunEventKind::Skipped,
        ],
    )?;
    let records = read_background_run_log(temp.path.join("runs.jsonl"))?;
    assert_eq!(
        records[1]
            .trigger
            .as_ref()
            .map(|trigger| trigger.state.as_str()),
        Some("not_triggered")
    );
    Ok(())
}

#[test]
fn triggered_plan_only_builds_and_logs_plan_without_apply() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_runner_plan_only")?;
    let target_entry = temp.path.join("project/target/debug/incremental/cache.bin");
    write_project_target_file(&target_entry)?;
    let request = request(
        &temp,
        "run-plan-only",
        triggered(WatcherDecisionState::TriggeredPlanOnly),
    )?;

    let report = run_background_cleanup_cycle(request, &NoopActiveProvider)?;

    assert!(report.plan_id.is_some());
    assert!(report.apply_report.is_none());
    assert!(target_entry.exists());
    assert!(temp.path.join("plan.json").is_file());
    assert_events(
        &temp.path.join("runs.jsonl"),
        &[
            BackgroundRunEventKind::Started,
            BackgroundRunEventKind::Triggered,
            BackgroundRunEventKind::PlanBuilt,
        ],
    )?;
    let records = read_background_run_log(temp.path.join("runs.jsonl"))?;
    assert_eq!(
        records[2]
            .plan
            .as_ref()
            .and_then(|plan| plan.plan_id.as_deref()),
        report.plan_id.as_ref().map(|plan_id| plan_id.as_str())
    );
    assert!(records[2].apply.is_none());
    Ok(())
}

#[test]
fn triggered_plan_and_apply_logs_apply_completion() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_runner_apply")?;
    let target_entry = temp.path.join("project/target/debug/incremental/cache.bin");
    write_project_target_file(&target_entry)?;
    let request = request(
        &temp,
        "run-apply",
        triggered(WatcherDecisionState::TriggeredPlanAndApply),
    )?;

    let report = run_background_cleanup_cycle(request, &NoopActiveProvider)?;

    assert!(report.plan_id.is_some());
    assert!(report.apply_report.is_some());
    assert!(!temp.path.join("project/target/debug/incremental").exists());
    assert_events(
        &temp.path.join("runs.jsonl"),
        &[
            BackgroundRunEventKind::Started,
            BackgroundRunEventKind::Triggered,
            BackgroundRunEventKind::PlanBuilt,
            BackgroundRunEventKind::ApplyCompleted,
        ],
    )?;
    let records = read_background_run_log(temp.path.join("runs.jsonl"))?;
    let apply = records[3]
        .apply
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing apply summary"))?;
    assert_eq!(apply.totals.applied_count, 1);
    assert_eq!(apply.totals.failed_count, 0);
    Ok(())
}

#[test]
fn failure_after_start_appends_failed_record() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_runner_failure")?;
    let target_entry = temp.path.join("project/target/debug/incremental/cache.bin");
    write_project_target_file(&target_entry)?;
    let mut request = request(
        &temp,
        "run-failure",
        triggered(WatcherDecisionState::TriggeredPlanOnly),
    )?;
    request.expires_at = request.created_at;

    let error = run_background_cleanup_cycle(request, &NoopActiveProvider)
        .expect_err("invalid expiry should fail persistence");

    assert!(matches!(error, BackgroundRunnerError::PersistPlan(_)));
    assert_events(
        &temp.path.join("runs.jsonl"),
        &[
            BackgroundRunEventKind::Started,
            BackgroundRunEventKind::Triggered,
            BackgroundRunEventKind::Failed,
        ],
    )?;
    let records = read_background_run_log(temp.path.join("runs.jsonl"))?;
    assert!(
        records[2]
            .problems
            .iter()
            .any(|problem| problem.contains("plan expiry"))
    );
    Ok(())
}

fn request(
    temp: &TestTemp,
    run_id: &str,
    decision: WatcherDecision,
) -> Result<BackgroundRunRequest, Box<dyn Error>> {
    Ok(BackgroundRunRequest {
        run_id: run_id.to_owned(),
        log_path: temp.path.join("runs.jsonl"),
        plan_path: temp.path.join("plan.json"),
        roots: vec![temp.path.join("project")],
        policy: PolicyKind::Balanced,
        scanner_options: ScannerOptions::default(),
        inventory_options: InventoryOptions::default(),
        planner_options: PlannerOptions::default(),
        trigger: BackgroundRunTrigger::Decision(decision),
        config_path: Some(temp.path.join("reclaim.toml")),
        config_version: Some(1),
        created_at: UNIX_EPOCH + Duration::from_secs(1_000),
        now: UNIX_EPOCH + Duration::from_secs(1_100),
        expires_at: UNIX_EPOCH + Duration::from_secs(2_000),
    })
}

fn triggered(state: WatcherDecisionState) -> WatcherDecision {
    WatcherDecision {
        state,
        reasons: vec![WatcherTriggerReason::TargetSizeExceeded {
            path: PathBuf::from("project/target"),
            size_bytes: 4,
            max_target_size_bytes: 3,
        }],
    }
}

fn write_project_target_file(path: &Path) -> Result<(), Box<dyn Error>> {
    let project_root = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| std::io::Error::other("missing project root"))?;
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| std::io::Error::other("missing parent"))?,
    )?;
    fs::write(
        project_root.join("Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(path, b"abc")?;
    Ok(())
}

fn assert_events(path: &Path, expected: &[BackgroundRunEventKind]) -> Result<(), Box<dyn Error>> {
    let records = read_background_run_log(path)?;
    let events = records.iter().map(|record| record.kind).collect::<Vec<_>>();
    assert_eq!(events, expected);
    Ok(())
}

struct NoopActiveProvider;

impl ActiveObservationProvider for NoopActiveProvider {
    fn observe(&self, _scope: &ActiveObservationScope) -> ActiveObservation {
        ActiveObservation::not_attempted()
    }
}

struct TestTemp {
    path: PathBuf,
}

impl TestTemp {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cargo_reclaim_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
