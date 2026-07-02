use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    CargoHomeApplyEntryStatus, CargoHomePlanRequest, PlanId, PolicyKind, SaveCargoHomePlanOptions,
    build_cargo_home_plan, persist_cargo_home_plan, validate_cargo_home_plan_for_apply,
};

#[test]
fn cargo_home_apply_validation_reports_would_delete_without_deleting() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cargo_home_apply_would_delete")?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;

    let document = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    let report =
        validate_cargo_home_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_500))?;

    assert!(report.dry_run);
    assert!(report.validation_only);
    assert_eq!(report.totals.delete_candidate_count, 1);
    assert_eq!(report.totals.would_delete_count, 1);
    assert_eq!(report.totals.would_delete_bytes, 3);
    assert!(cache_file.is_file());
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == CargoHomeApplyEntryStatus::WouldDelete)
    );
    Ok(())
}

#[test]
fn cargo_home_apply_validation_skips_stale_size_kind_symlink_and_class_changes()
-> Result<(), Box<dyn Error>> {
    assert_stale_after_mutation("size", |temp, _document| {
        fs::write(
            temp.path().join("registry/cache/example/pkg.crate"),
            b"abcd",
        )?;
        Ok(())
    })?;
    assert_stale_after_mutation("kind", |temp, _document| {
        fs::remove_file(temp.path().join("registry/cache/example/pkg.crate"))?;
        fs::create_dir(temp.path().join("registry/cache/example/pkg.crate"))?;
        Ok(())
    })?;
    assert_stale_after_mutation("symlink", |temp, _document| {
        let path = temp.path().join("registry/cache/example/pkg.crate");
        fs::remove_file(&path)?;
        std::os::unix::fs::symlink(temp.path().join("target"), &path)?;
        Ok(())
    })?;
    assert_stale_after_mutation("same_size_content", |temp, _document| {
        fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"xyz")?;
        Ok(())
    })?;
    assert_stale_after_mutation("class", |temp, document| {
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, b"abc")?;
        let entry = document
            .body
            .plan
            .entries
            .iter_mut()
            .find(|entry| entry.action == "delete_candidate")
            .expect("delete candidate entry");
        entry.path = config_path.display().to_string();
        entry.relative_path = "config.toml".to_string();
        document.id = PlanId::from_body(&document.body)?;
        Ok(())
    })?;
    Ok(())
}

#[test]
fn cargo_home_apply_validation_never_deletes_preserved_or_unknown_entries()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_apply_preserved")?;
    fs::write(temp.path().join("config.toml"), b"[net]\n")?;
    fs::write(temp.path().join("custom"), b"abc")?;

    let document = persisted_plan(temp.path(), PolicyKind::Aggressive)?;
    let report =
        validate_cargo_home_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_500))?;

    assert_eq!(report.totals.delete_candidate_count, 0);
    assert_eq!(report.totals.would_delete_count, 0);
    assert_eq!(report.totals.skipped_count, 2);
    assert!(
        report
            .entries
            .iter()
            .all(|entry| entry.status == CargoHomeApplyEntryStatus::NotPlannedForDeletion)
    );
    assert!(temp.path().join("config.toml").is_file());
    assert!(temp.path().join("custom").is_file());
    Ok(())
}

#[test]
fn cargo_home_apply_validation_skips_when_root_is_replaced_by_symlink() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cargo_home_apply_root_symlink")?;
    let root = temp.path().join("cargo-home");
    let cache_file = root.join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let document = persisted_plan(&root, PolicyKind::Conservative)?;
    let replacement = temp.path().join("replacement-cargo-home");
    let replacement_cache_file = replacement.join("registry/cache/example/pkg.crate");
    fs::create_dir_all(replacement_cache_file.parent().expect("cache parent"))?;
    fs::write(&replacement_cache_file, b"abc")?;

    fs::remove_dir_all(&root)?;
    std::os::unix::fs::symlink(&replacement, &root)?;
    let report =
        validate_cargo_home_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_500))?;

    assert_eq!(report.totals.would_delete_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(replacement_cache_file.is_file());
    Ok(())
}

#[test]
fn cargo_home_apply_validation_rejects_relative_path_escape() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_apply_escape")?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let mut document = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    let entry = document
        .body
        .plan
        .entries
        .iter_mut()
        .find(|entry| entry.action == "delete_candidate")
        .expect("delete candidate entry");
    entry.path = temp
        .path()
        .join("registry/cache/../../registry/cache")
        .display()
        .to_string();
    entry.relative_path = "registry/cache/../../registry/cache".to_string();
    document.id = PlanId::from_body(&document.body)?;

    let report =
        validate_cargo_home_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_500))?;

    assert_eq!(report.totals.would_delete_count, 0);
    assert_eq!(report.totals.stale_skip_count, 1);
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == CargoHomeApplyEntryStatus::SkipStalePlan)
    );
    assert!(cache_file.is_file());
    Ok(())
}

fn assert_stale_after_mutation(
    name: &str,
    mutate: impl FnOnce(
        &TestTemp,
        &mut cargo_reclaim::PersistedCargoHomePlan,
    ) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new(&format!("cargo_home_apply_stale_{name}"))?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let mut document = persisted_plan(temp.path(), PolicyKind::Conservative)?;

    mutate(&temp, &mut document)?;
    let report =
        validate_cargo_home_plan_for_apply(&document, UNIX_EPOCH + Duration::from_secs(1_500))?;

    assert_eq!(report.totals.would_delete_count, 0, "{name}");
    assert_eq!(report.totals.stale_skip_count, 1, "{name}");
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == CargoHomeApplyEntryStatus::SkipStalePlan),
        "{name}"
    );
    Ok(())
}

fn persisted_plan(
    cargo_home: &Path,
    policy: PolicyKind,
) -> Result<cargo_reclaim::PersistedCargoHomePlan, Box<dyn Error>> {
    let plan = build_cargo_home_plan(CargoHomePlanRequest {
        cargo_home: Some(cargo_home.to_path_buf()),
        policy,
    })?;
    Ok(persist_cargo_home_plan(
        &plan,
        SaveCargoHomePlanOptions {
            created_at: UNIX_EPOCH + Duration::from_secs(1_000),
            expires_at: UNIX_EPOCH + Duration::from_secs(2_000),
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

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
