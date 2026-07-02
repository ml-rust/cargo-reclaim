use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ApplyEntryResult, ApplyEntryStatus, ApplyReport, ApplyTotals, ArtifactClass,
    BackgroundApplySummary, BackgroundPlanSummary, BackgroundRunEventKind, BackgroundRunLogError,
    BackgroundRunLogRecord, BackgroundTriggerReasonSummary, BackgroundTriggerSummary, PathKind,
    PathSnapshot, PersistedTimestamp, Plan, PlanAction, PlanEntry, PlanId, PlanInput, PolicyKind,
    TargetEvidence, WatcherDecision, WatcherDecisionState, WatcherTriggerReason,
    append_background_run_log_record, read_background_run_log,
};

#[test]
fn append_writes_jsonl_records_and_read_preserves_order() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_order")?;
    let path = temp.path.join("runs.jsonl");
    let first = record("run-1", 10, BackgroundRunEventKind::Started);
    let second = record("run-2", 20, BackgroundRunEventKind::ApplyCompleted);

    append_background_run_log_record(&path, &first)?;
    append_background_run_log_record(&path, &second)?;

    let contents = fs::read_to_string(&path)?;
    assert_eq!(contents.lines().count(), 2);
    let first_line = contents
        .lines()
        .next()
        .ok_or_else(|| std::io::Error::other("missing first log line"))?;
    let first_line: serde_json::Value = serde_json::from_str(first_line)?;
    assert_eq!(first_line["event"], "started");
    assert!(first_line.get("kind").is_none());
    let records = read_background_run_log(&path)?;
    assert_eq!(records, vec![first, second]);
    Ok(())
}

#[test]
fn read_rejects_malformed_json_with_line_number() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_malformed")?;
    let path = temp.path.join("runs.jsonl");
    let valid_record = record("run-1", 10, BackgroundRunEventKind::Started);
    let valid_line = serde_json::to_string(&valid_record)?;
    fs::write(&path, format!("{valid_line}\n{{not-json}}\n"))?;

    assert!(matches!(
        read_background_run_log(&path),
        Err(BackgroundRunLogError::Json { line: 2, .. })
    ));
    Ok(())
}

#[test]
fn read_rejects_unsupported_schema_version() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_schema")?;
    let path = temp.path.join("runs.jsonl");
    let mut record = record("run-1", 10, BackgroundRunEventKind::Started);
    record.schema_version = 999;
    fs::write(&path, format!("{}\n", serde_json::to_string(&record)?))?;

    assert!(matches!(
        read_background_run_log(&path),
        Err(BackgroundRunLogError::UnsupportedSchemaVersion {
            line: 1,
            found: 999,
            ..
        })
    ));
    Ok(())
}

#[test]
fn threshold_trigger_summary_preserves_details() {
    let decision = WatcherDecision {
        state: WatcherDecisionState::TriggeredPlanAndApply,
        reasons: vec![
            WatcherTriggerReason::TargetSizeExceeded {
                path: PathBuf::from("/work/project/target"),
                size_bytes: 42,
                max_target_size_bytes: 40,
            },
            WatcherTriggerReason::DiskFreeBelow {
                free_basis_points: 1_200,
                threshold_basis_points: 1_500,
            },
        ],
    };

    let summary = BackgroundTriggerSummary::from_watcher_decision(&decision);

    assert_eq!(summary.state, "triggered_plan_and_apply");
    assert_eq!(
        summary.reasons,
        vec![
            BackgroundTriggerReasonSummary::TargetSizeExceeded {
                path: "/work/project/target".to_owned(),
                size_bytes: 42,
                max_target_size_bytes: 40,
            },
            BackgroundTriggerReasonSummary::DiskFreeBelow {
                free_basis_points: 1_200,
                threshold_basis_points: 1_500,
            },
        ]
    );
}

#[test]
fn apply_summary_exposes_notable_entries_and_totals() {
    let report = ApplyReport {
        plan_id: PlanId("sha256:abc".to_owned()),
        dry_run: false,
        entries: vec![
            apply_entry("deleted/path", ApplyEntryStatus::Deleted, 10),
            apply_entry("failed/path", ApplyEntryStatus::DeleteFailed, 20),
            apply_entry("stale/path", ApplyEntryStatus::SkipStalePlan, 30),
            apply_entry(
                "preserved/path",
                ApplyEntryStatus::NotPlannedForDeletion,
                40,
            ),
        ],
        totals: ApplyTotals {
            entry_count: 4,
            delete_candidate_count: 3,
            skipped_count: 2,
            stale_skip_count: 1,
            applied_count: 1,
            failed_count: 1,
            applied_bytes: 10,
            ..ApplyTotals::default()
        },
    };

    let summary = BackgroundApplySummary::from_apply_report(&report);

    assert_eq!(summary.plan_id, "sha256:abc");
    assert_eq!(summary.totals.entry_count, 4);
    assert_eq!(summary.totals.applied_count, 1);
    assert_eq!(summary.totals.failed_count, 1);
    assert_eq!(summary.totals.stale_skip_count, 1);
    assert_eq!(summary.notable_entries.len(), 3);
    assert_eq!(summary.notable_entries[0].status, "deleted");
    assert_eq!(summary.notable_entries[1].status, "delete_failed");
    assert_eq!(summary.notable_entries[2].status, "skip_stale_plan");
}

#[test]
fn plan_summary_records_policy_and_totals_without_entries() -> Result<(), Box<dyn Error>> {
    let plan = sample_plan()?;

    let summary = BackgroundPlanSummary::from_plan(PolicyKind::Balanced, &plan);

    assert_eq!(summary.plan_id, None);
    assert_eq!(summary.policy, "balanced");
    assert_eq!(summary.totals.entry_count, 2);
    assert_eq!(summary.totals.total_bytes, 30);
    assert_eq!(summary.totals.delete_candidate_count, 1);
    assert_eq!(serde_json::to_value(&summary)?.get("entries"), None);
    Ok(())
}

#[test]
fn append_creates_parent_directory_and_preserves_unrelated_files() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_parent")?;
    let unrelated = temp.path.join("keep.txt");
    fs::write(&unrelated, "keep")?;
    let path = temp.path.join("nested/logs/runs.jsonl");

    append_background_run_log_record(&path, &record("run-1", 10, BackgroundRunEventKind::Started))?;

    assert!(path.is_file());
    assert_eq!(fs::read_to_string(unrelated)?, "keep");
    Ok(())
}

fn record(run_id: &str, unix_seconds: u64, kind: BackgroundRunEventKind) -> BackgroundRunLogRecord {
    BackgroundRunLogRecord::new(
        run_id,
        PersistedTimestamp {
            unix_seconds,
            nanoseconds: 0,
        },
        kind,
    )
}

fn apply_entry(path: &str, status: ApplyEntryStatus, size_bytes: u64) -> ApplyEntryResult {
    ApplyEntryResult {
        path: path.to_owned(),
        planned_action: "delete".to_owned(),
        status,
        reason: status_label(status).to_owned(),
        size_bytes,
    }
}

fn status_label(status: ApplyEntryStatus) -> &'static str {
    match status {
        ApplyEntryStatus::WouldDelete => "would_delete",
        ApplyEntryStatus::Deleted => "deleted",
        ApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        ApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
        ApplyEntryStatus::DeleteFailed => "delete_failed",
    }
}

fn sample_plan() -> Result<Plan, Box<dyn Error>> {
    let delete_entry = PlanEntry::new(
        PathSnapshot::with_details(
            "target/debug/incremental",
            10,
            PathKind::Directory,
            Some(UNIX_EPOCH),
        )?,
        ArtifactClass::Incremental,
        TargetEvidence::project_context("Cargo.toml")?,
        PlanAction::Delete,
        "derived intermediate output",
        false,
    )?;
    let preserved_entry = PlanEntry::preserved(
        PathSnapshot::new("target/doc", 20)?,
        ArtifactClass::Docs,
        TargetEvidence::strong_marker("rustdoc")?,
        "policy protects durable output",
    )?;

    Ok(Plan::new(
        PlanInput::from_root(".")?,
        vec![delete_entry, preserved_entry],
    ))
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
