use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ApplyEntryStatus, ArtifactClass, InventoryOptions, PathKind, PathSnapshot, Plan, PlanAction,
    PlanCommandKind, PlanEditRequest, PlanEntry, PlanId, PlanInput, PlanInvocation,
    PlanPersistenceError, PlannerOptions, PolicyKind, SavePlanOptions, ScannerOptions,
    TargetEvidence, edit_persisted_plan, execute_persisted_plan_apply, persist_plan, snapshot_path,
    validate_persisted_plan_for_apply,
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
fn apply_validation_reports_stale_plan_when_same_size_content_changes() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("apply_same_size_stale")?;
    let file = temp.path.join("target/debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().expect("file parent"))?;
    fs::write(&file, b"abc")?;
    let mut document = persisted_plan_for_path(&file, 3)?;
    document.body.plan.entries[0].snapshot.modified = None;
    document.id = PlanId::from_body(&document.body)?;
    fs::write(&file, b"xyz")?;

    let report =
        validate_persisted_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.totals.would_delete_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(
        report.entries[0]
            .reason
            .contains("content fingerprint changed")
    );
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
fn apply_execution_reports_measured_deleted_bytes_for_shallow_directory()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_dir_measured_bytes")?;
    let directory = temp.path.join("target/debug/incremental");
    let expected_deleted_bytes = 7;
    fs::create_dir_all(directory.join("session"))?;
    fs::write(directory.join("session/cache.bin"), b"abc")?;
    fs::write(directory.join("session/other.bin"), b"defg")?;
    let document = persisted_plan_for_directory(&directory, 0, false)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::Deleted);
    assert_eq!(report.entries[0].size_bytes, 0);
    assert_eq!(
        report.entries[0].deleted_bytes,
        Some(expected_deleted_bytes)
    );
    assert_eq!(report.totals.applied_bytes, expected_deleted_bytes);
    assert!(!directory.exists());
    Ok(())
}

#[test]
fn apply_execution_deletes_revalidated_whole_target() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_whole_target")?;
    write_manifest(&temp.path)?;
    let target = temp.path.join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let document = persisted_whole_target_plan_for_project(&target, temp.path.join("Cargo.toml"))?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::Deleted);
    assert_eq!(report.totals.applied_count, 1);
    assert!(!target.exists());
    Ok(())
}

#[test]
fn apply_execution_skips_whole_target_when_project_manifest_is_missing()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_whole_target_missing_manifest")?;
    write_manifest(&temp.path)?;
    let manifest = temp.path.join("Cargo.toml");
    let target = temp.path.join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let document = persisted_whole_target_plan_for_project(&target, &manifest)?;
    fs::remove_file(manifest)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::SkipStalePlan);
    assert!(report.entries[0].reason.contains("project manifest"));
    assert!(target.is_dir());
    Ok(())
}

#[test]
fn apply_execution_skips_whole_target_when_marker_is_missing() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_whole_target_missing_marker")?;
    let target = temp.path.join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(
        target.join("CACHEDIR.TAG"),
        b"Signature: 8a477f597d28d172789f06886806bc55",
    )?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let document =
        persisted_whole_target_plan(&target, TargetEvidence::strong_marker("CACHEDIR.TAG")?)?;
    let marker_size = fs::metadata(target.join("CACHEDIR.TAG"))?.len();
    fs::remove_file(target.join("CACHEDIR.TAG"))?;
    fs::write(
        target.join("same-size-placeholder"),
        vec![b'x'; marker_size as usize],
    )?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::SkipStalePlan);
    assert!(report.entries[0].reason.contains("skip_stale_plan"));
    assert!(target.is_dir());
    Ok(())
}

#[test]
#[cfg(unix)]
fn apply_execution_skips_whole_target_replaced_by_symlink() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("apply_whole_target_symlink")?;
    write_manifest(&temp.path)?;
    let target = temp.path.join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let document = persisted_whole_target_plan_for_project(&target, temp.path.join("Cargo.toml"))?;
    fs::remove_dir_all(&target)?;
    fs::create_dir(temp.path.join("replacement"))?;
    symlink(temp.path.join("replacement"), &target)?;

    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::SkipStalePlan);
    assert!(report.entries[0].reason.contains("symlink"));
    assert!(target.exists());
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
fn apply_execution_deletes_confirmation_entry_after_selection() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_execute_selected_confirmation")?;
    let directory = temp.path.join("target/debug/incremental");
    fs::create_dir_all(directory.join("session"))?;
    fs::write(directory.join("session/cache.bin"), b"abc")?;
    let mut document = persisted_plan_for_directory(&directory, 3, true)?;

    edit_persisted_plan(
        &mut document,
        &PlanEditRequest::new_with_indices(Vec::new(), Vec::new(), vec![1], Vec::new())?,
        UNIX_EPOCH + Duration::from_secs(1_100),
    )?;
    let report = execute_persisted_plan_apply(&document, UNIX_EPOCH + Duration::from_secs(1_100))?;

    assert_eq!(report.entries[0].status, ApplyEntryStatus::Deleted);
    assert_eq!(report.totals.applied_count, 1);
    assert!(!directory.exists());
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

#[test]
fn apply_preserves_an_unchanged_candidate_when_its_target_has_new_writes()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("apply_target_new_writes")?;
    let target = temp.path.join("target");
    let file = target.join("debug/deps/libsample-0123456789abcdef.rlib");
    fs::create_dir_all(file.parent().expect("parent"))?;
    fs::write(target.join(".rustc_info.json"), "{}")?;
    fs::write(&file, b"abc")?;
    let old = SystemTime::now() - Duration::from_secs(14 * 86400);
    // Windows requires write-attribute access when updating file timestamps.
    fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(old)?;
    let mut document = persisted_plan_for_path(&file, 3)?;
    document.body.plan.input.roots = vec![temp.path.to_string_lossy().into_owned()];
    document
        .body
        .invocation
        .planner_options
        .recent_write_keep_window_seconds = Some(7 * 86400);
    document.body.created_at = cargo_reclaim::PersistedTimestamp::from_system_time(old)?;
    document.body.expires_at = cargo_reclaim::PersistedTimestamp::from_system_time(
        SystemTime::now() + Duration::from_secs(3600),
    )?;
    document.id = PlanId::from_body(&document.body)?;
    fs::write(target.join("debug/deps/new-build.o"), b"new build output")?;

    let validation = validate_persisted_plan_for_apply(&document, SystemTime::now())?;
    assert_eq!(validation.totals.would_delete_count, 0);
    let report = execute_persisted_plan_apply(&document, SystemTime::now())?;
    assert_eq!(report.totals.applied_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert_eq!(fs::read(&file)?, b"abc");
    Ok(())
}

#[cfg(unix)]
#[test]
fn apply_honors_relative_exclusions_during_recent_write_revalidation() -> Result<(), Box<dyn Error>>
{
    // Keep the fixture beneath cwd so the plan can use relative paths without
    // changing process-global cwd in a parallel test run.
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = TestTemp {
        path: PathBuf::from(format!("reclaim-relative-{unique}")),
    };
    let target = temp.path.join("target");
    let skipped = target.join("debug/cache");
    let file = target.join("debug/deps/libsample-0123456789abcdef.rlib");
    fs::create_dir_all(file.parent().unwrap())?;
    fs::create_dir_all(&skipped)?;
    fs::write(target.join(".rustc_info.json"), "{}")?;
    fs::write(&file, b"abc")?;
    fs::write(skipped.join("fresh.bin"), b"new excluded data")?;
    let now = SystemTime::now();
    let old = now - Duration::from_secs(14 * 86400);
    for path in [
        &file,
        &target.join(".rustc_info.json"),
        &target.join("debug/deps"),
        &target.join("debug"),
        &target,
    ] {
        fs::File::open(path)?.set_modified(old)?;
    }
    let mut document = persisted_plan_for_path(&file, 3)?;
    document.body.plan.input.roots = vec![temp.path.to_string_lossy().into_owned()];
    document.body.invocation.scanner_options.skipped_paths =
        vec![skipped.to_string_lossy().into_owned()];
    document
        .body
        .invocation
        .planner_options
        .recent_write_keep_window_seconds = Some(7 * 86400);
    document.body.created_at = cargo_reclaim::PersistedTimestamp::from_system_time(old)?;
    document.body.expires_at =
        cargo_reclaim::PersistedTimestamp::from_system_time(now + Duration::from_secs(3600))?;
    document.id = PlanId::from_body(&document.body)?;

    let report = execute_persisted_plan_apply(&document, now)?;
    assert_eq!(report.totals.applied_count, 1, "{report:?}");
    assert!(!file.exists());
    assert!(skipped.join("fresh.bin").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn apply_rejects_a_symlink_substituted_for_an_ancestor() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("apply_parent_symlink")?;
    let target = temp.path.join("target");
    let file = target.join("debug/incremental/cache.bin");
    fs::create_dir_all(file.parent().expect("file parent"))?;
    fs::write(&file, b"abc")?;
    let document = persisted_plan_for_path(&file, 3)?;
    let outside = temp.path.join("outside-target");
    fs::rename(target.join("debug"), &outside)?;
    symlink(&outside, target.join("debug"))?;

    let now = UNIX_EPOCH + Duration::from_secs(1_100);
    let validation = validate_persisted_plan_for_apply(&document, now)?;
    assert_eq!(validation.totals.stale_skip_count, 1);
    let report = execute_persisted_plan_apply(&document, now)?;
    assert_eq!(report.totals.applied_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(report.entries[0].reason.contains("symlink"));
    assert_eq!(fs::read(outside.join("incremental/cache.bin"))?, b"abc");
    Ok(())
}

#[cfg(unix)]
#[test]
fn apply_requires_real_paths_when_a_parent_is_already_symlinked() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("apply_real_path")?;
    let real = temp.path.join("real");
    fs::create_dir(&real)?;
    let alias = temp.path.join("alias");
    symlink(&real, &alias)?;
    let file = alias.join("cache.bin");
    fs::write(&file, b"abc")?;
    let now = UNIX_EPOCH + Duration::from_secs(1_100);

    let aliased_plan = persisted_plan_for_path(&file, 3)?;
    let report = execute_persisted_plan_apply(&aliased_plan, now)?;
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(file.exists());

    let real_plan = persisted_plan_for_path(file.canonicalize()?, 3)?;
    let report = execute_persisted_plan_apply(&real_plan, now)?;
    assert_eq!(report.totals.applied_count, 1);
    assert!(!file.exists());
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

fn persisted_whole_target_plan_for_project(
    target: impl Into<PathBuf>,
    manifest: impl Into<PathBuf>,
) -> Result<cargo_reclaim::PersistedPlan, Box<dyn Error>> {
    persisted_whole_target_plan(target, TargetEvidence::project_context(manifest.into())?)
}

fn persisted_whole_target_plan(
    target: impl Into<PathBuf>,
    evidence: TargetEvidence,
) -> Result<cargo_reclaim::PersistedPlan, Box<dyn Error>> {
    let target = target.into();
    let snapshot = snapshot_path(&target, &InventoryOptions::default())?;
    let entry = PlanEntry::new(
        snapshot,
        ArtifactClass::WholeTarget,
        evidence,
        PlanAction::Delete,
        "aggressive policy permits confirmed whole-target deletion",
        false,
    )?;
    let plan = Plan::new(PlanInput::from_root(".")?, vec![entry]);
    Ok(persist_plan(
        &plan,
        SavePlanOptions {
            created_at: UNIX_EPOCH + Duration::from_secs(1_000),
            expires_at: UNIX_EPOCH + Duration::from_secs(2_000),
            interactive_selection_modified: true,
            invocation: PlanInvocation::new(
                PlanCommandKind::Plan,
                PolicyKind::Aggressive,
                &ScannerOptions::default(),
                &InventoryOptions::default(),
                &PlannerOptions {
                    whole_target_mode: cargo_reclaim::WholeTargetMode::DeleteConfirmed,
                    ..PlannerOptions::default()
                },
            ),
        },
    )?)
}

fn write_manifest(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    fs::write(path.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    Ok(())
}

struct TestTemp {
    path: PathBuf,
}

impl TestTemp {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().canonicalize()?.join(format!(
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
