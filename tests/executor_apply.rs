use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ApplyEntryStatus, ArtifactClass, InventoryOptions, PathKind, PathSnapshot, Plan, PlanAction,
    PlanCommandKind, PlanEntry, PlanInput, PlanInvocation, PlanPersistenceError, PlannerOptions,
    PolicyKind, SavePlanOptions, ScannerOptions, TargetEvidence, execute_persisted_plan_apply,
    persist_plan, validate_persisted_plan_for_apply,
};

#[test]
fn apply_validation_revalidates_delete_candidates_without_deleting() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_validate")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().expect("file parent"))?;
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
    fs::create_dir_all(file.parent().expect("file parent"))?;
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
    fs::create_dir_all(file.parent().expect("file parent"))?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;

    assert!(matches!(
        validate_persisted_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(5_000)),
        Err(PlanPersistenceError::PlanExpired)
    ));
    Ok(())
}

#[test]
fn apply_execution_deletes_revalidated_file() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_file")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().expect("file parent"))?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert!(!report.dry_run);
    assert_eq!(report.entries[0].status, ApplyEntryStatus::Deleted);
    assert_eq!(report.totals.applied_count, 1);
    assert_eq!(report.totals.applied_bytes, 3);
    assert_eq!(report.totals.failed_count, 0);
    assert!(!file.exists());
    Ok(())
}

#[test]
fn apply_execution_deletes_revalidated_directory() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_dir")?;
    let directory = temp.path.join("target/debug/incremental");
    fs::create_dir_all(directory.join("session"))?;
    fs::write(directory.join("session/cache.bin"), b"abc")?;
    let document = persisted_plan_for_directory(&directory, 3, false)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::Deleted);
    assert_eq!(report.totals.applied_count, 1);
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn apply_execution_skips_stale_paths_without_deleting() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_stale")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().expect("file parent"))?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;
    fs::write(&file, b"changed")?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::SkipStalePlan);
    assert_eq!(report.totals.applied_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(file.is_file());
    Ok(())
}

#[test]
fn apply_execution_skips_entries_requiring_confirmation() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_confirmation")?;
    let directory = temp.path.join("target");
    fs::create_dir_all(directory.join("debug/incremental"))?;
    fs::write(directory.join("debug/incremental/cache.bin"), b"abc")?;
    let document = persisted_plan_for_directory(&directory, 3, true)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(
        report.entries[0].status,
        ApplyEntryStatus::NotPlannedForDeletion
    );
    assert_eq!(report.totals.applied_count, 0);
    assert!(directory.is_dir());
    Ok(())
}

#[test]
#[cfg(unix)]
fn apply_execution_reports_delete_failures() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp = TestTemp::new("apply_execute_failed")?;
    let directory = temp.path.join("target/debug/incremental");
    fs::create_dir_all(directory.join("session"))?;
    fs::write(directory.join("session/cache.bin"), b"abc")?;
    let document = persisted_plan_for_directory(&directory, 3, false)?;

    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555))?;
    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::DeleteFailed);
    assert_eq!(report.totals.failed_count, 1);
    assert!(directory.is_dir());
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
                &PlannerOptions::default(),
            ),
        },
    )?)
}

fn persisted_plan_for_directory(
    path: impl Into<PathBuf>,
    size_bytes: u64,
    requires_confirmation: bool,
) -> Result<cargo_reclaim::PersistedPlan, Box<dyn Error>> {
    let path = path.into();
    let modified = fs::metadata(&path)?.modified().ok();
    let snapshot = PathSnapshot::with_details(path, size_bytes, PathKind::Directory, modified)?;
    let entry = PlanEntry::new(
        snapshot,
        ArtifactClass::Incremental,
        TargetEvidence::project_context("Cargo.toml")?,
        if requires_confirmation {
            PlanAction::RequiresConfirmation
        } else {
            PlanAction::Delete
        },
        "derived intermediate output",
        requires_confirmation,
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
                &PlannerOptions::default(),
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
