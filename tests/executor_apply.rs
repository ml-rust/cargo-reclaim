use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, InventoryOptions, PathKind, PathSnapshot, Plan, PlanAction, PlanCommandKind,
    PlanEntry, PlanInput, PlanInvocation, PlanPersistenceError, PolicyKind, SavePlanOptions,
    ScannerOptions, TargetEvidence, persist_plan, validate_persisted_plan_for_apply,
};

#[test]
fn apply_validation_revalidates_delete_candidates_without_deleting() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_validate")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;

    let report =
        validate_persisted_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.totals.would_delete_count, 1);
    assert_eq!(report.totals.would_delete_bytes, 3);
    assert!(report.dry_run);
    assert!(file.is_file());
    Ok(())
}

#[test]
fn apply_validation_reports_stale_plan_when_path_changes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_stale")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;
    fs::write(&file, b"changed")?;

    let report =
        validate_persisted_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.totals.would_delete_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(report.entries[0].reason.contains("skip_stale_plan"));
    Ok(())
}

#[test]
fn apply_validation_rejects_expired_plans() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_expired")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;

    assert!(matches!(
        validate_persisted_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(5_000)),
        Err(PlanPersistenceError::PlanExpired)
    ));
    Ok(())
}

fn persisted_plan_for_path(
    path: impl Into<PathBuf>,
    size_bytes: u64,
) -> Result<cargo_reclaim::PersistedPlan, Box<dyn Error>> {
    let path = path.into();
    let modified = fs::metadata(&path)?.modified().ok();
    let snapshot = PathSnapshot::with_details(path, size_bytes, PathKind::File, modified)?;
    let entry = PlanEntry::new(
        snapshot,
        ArtifactClass::Incremental,
        TargetEvidence::project_context("Cargo.toml")?,
        PlanAction::Delete,
        "derived intermediate output",
        false,
    )?;
    let plan = Plan::new(PlanInput::from_root(".")?, vec![entry]);
    Ok(persist_plan(
        &plan,
        SavePlanOptions {
            created_at: UNIX_EPOCH + Duration::from_secs(1_000),
            expires_at: UNIX_EPOCH + Duration::from_secs(2_000),
            interactive_selection_modified: false,
            invocation: PlanInvocation::new(
                PlanCommandKind::Plan,
                PolicyKind::Balanced,
                &ScannerOptions::default(),
                &InventoryOptions::default(),
            ),
        },
    )?)
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
