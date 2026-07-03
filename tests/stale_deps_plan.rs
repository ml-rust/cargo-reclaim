use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, InventoryOptions, PlanAction, PlanInvocation, PlannerOptions, PolicyKind,
    SavePlanOptions, ScannerOptions, build_plan_from_roots, build_plan_from_roots_with_options,
    persist_plan,
};

#[test]
fn stale_deps_deletes_older_anchored_hash_variant_and_keeps_newest() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_delete_old")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint(temp.path(), "debug", "sample", old_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    write_deps_file(
        temp.path(),
        "debug",
        &format!("sample-{old_hash}.d"),
        b"old dep",
    )?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;
    write_deps_file(
        temp.path(),
        "debug",
        &format!("sample-{new_hash}.d"),
        b"new dep",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let old_binary = entry_for(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    assert_eq!(old_binary.artifact_class, ArtifactClass::StaleDeps);
    assert_eq!(old_binary.action, PlanAction::Delete);

    let old_dep_info = entry_for(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}.d")),
    )?;
    assert_eq!(old_dep_info.artifact_class, ArtifactClass::StaleDeps);
    assert_eq!(old_dep_info.action, PlanAction::Delete);

    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{new_hash}")),
    )?;
    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{new_hash}.d")),
    )?;
    Ok(())
}

#[test]
fn stale_deps_requires_valid_fingerprint_anchor() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_anchor_required")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    Ok(())
}

#[test]
fn stale_deps_prunes_orphaned_duplicate_hashes_when_fingerprint_dir_is_missing()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_orphaned_duplicates")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    write_deps_file(
        temp.path(),
        "debug",
        &format!("sample-{old_hash}.d"),
        b"old dep",
    )?;
    sleep_for_mtime_tick();
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;
    write_deps_file(
        temp.path(),
        "debug",
        &format!("sample-{new_hash}.d"),
        b"new dep",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let old_binary = entry_for(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    assert_eq!(old_binary.artifact_class, ArtifactClass::StaleDeps);
    assert_eq!(old_binary.action, PlanAction::Delete);
    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{new_hash}")),
    )?;
    Ok(())
}

#[test]
fn stale_deps_ignores_malformed_fingerprint_anchor() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_malformed_anchor")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint_contents(temp.path(), "debug", "sample", old_hash, b"not-json")?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    Ok(())
}

#[test]
fn stale_deps_respects_keep_rustc_hashes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_keep_rustc")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint(temp.path(), "debug", "sample", old_hash, 7)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 8)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            keep_rustc_hashes: vec![7],
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    assert_preserved_deps_output(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    Ok(())
}

#[test]
fn stale_deps_weak_evidence_requires_confirmation() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_deps_weak_evidence")?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint(temp.path(), "debug", "sample", old_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;

    let entry = entry_for(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    assert_eq!(entry.artifact_class, ArtifactClass::StaleDeps);
    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn stale_deps_recent_write_window_does_not_hide_proven_stale_variant() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("stale_deps_recent_write")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint(temp.path(), "debug", "sample", old_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(24 * 60 * 60)),
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let entry = entry_for(
        &plan,
        temp.path()
            .join(format!("target/debug/deps/sample-{old_hash}")),
    )?;
    assert_eq!(entry.artifact_class, ArtifactClass::StaleDeps);
    assert_eq!(entry.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn persisted_stale_deps_delete_entry_uses_fast_snapshot_revalidation() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("stale_deps_persistence")?;
    write_manifest(temp.path())?;
    let old_hash = "1111111111111111";
    let new_hash = "2222222222222222";
    write_fingerprint(temp.path(), "debug", "sample", old_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{old_hash}"), b"old")?;
    sleep_for_mtime_tick();
    write_fingerprint(temp.path(), "debug", "sample", new_hash, 1)?;
    write_deps_file(temp.path(), "debug", &format!("sample-{new_hash}"), b"new")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    let created_at = SystemTime::now();
    let document = persist_plan(
        &plan,
        SavePlanOptions {
            created_at,
            expires_at: created_at + Duration::from_secs(300),
            interactive_selection_modified: false,
            invocation: PlanInvocation::new(
                cargo_reclaim::PlanCommandKind::Plan,
                PolicyKind::Balanced,
                &ScannerOptions::default(),
                &InventoryOptions::default(),
                &PlannerOptions::default(),
            ),
        },
    )?;
    let value = serde_json::to_value(&document)?;
    let entries = value["plan"]["entries"]
        .as_array()
        .expect("entries should be an array");
    let stale_entry = entries
        .iter()
        .find(|entry| {
            entry["artifact_class"] == "stale_deps"
                && entry["snapshot"]["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(&format!("sample-{old_hash}")))
        })
        .expect("persisted stale deps entry");

    assert_eq!(stale_entry["snapshot"]["path_kind"], "file");
    assert!(stale_entry["snapshot"].get("content_fingerprint").is_none());
    Ok(())
}

#[test]
fn stale_incremental_deletes_older_session_and_keeps_newest() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_incremental_delete_old")?;
    write_manifest(temp.path())?;
    write_incremental_session(temp.path(), "debug", "sample-1abc", "s-old")?;
    sleep_for_mtime_tick();
    write_incremental_session(temp.path(), "debug", "sample-1abc", "s-new")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions {
            deep_directory_measurement: true,
            ..InventoryOptions::default()
        },
    )?;

    let old_session = entry_for(
        &plan,
        temp.path()
            .join("target/debug/incremental/sample-1abc/s-old"),
    )?;
    assert_eq!(old_session.artifact_class, ArtifactClass::StaleIncremental);
    assert_eq!(old_session.action, PlanAction::Delete);
    assert_no_entry(
        &plan,
        temp.path()
            .join("target/debug/incremental/sample-1abc/s-new"),
    );
    assert_no_entry(&plan, temp.path().join("target/debug/incremental"));
    Ok(())
}

#[test]
fn stale_incremental_deletes_older_unit_variant_and_keeps_newest() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_incremental_delete_old_unit")?;
    write_manifest(temp.path())?;
    write_incremental_session(temp.path(), "debug", "sample-1abc", "s-only")?;
    sleep_for_mtime_tick();
    write_incremental_session(temp.path(), "debug", "sample-2def", "s-only")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions {
            deep_directory_measurement: true,
            ..InventoryOptions::default()
        },
    )?;

    let old_unit = entry_for(
        &plan,
        temp.path().join("target/debug/incremental/sample-1abc"),
    )?;
    assert_eq!(old_unit.artifact_class, ArtifactClass::StaleIncremental);
    assert_eq!(old_unit.action, PlanAction::Delete);
    assert_no_entry(
        &plan,
        temp.path().join("target/debug/incremental/sample-2def"),
    );
    assert_no_entry(&plan, temp.path().join("target/debug/incremental"));
    Ok(())
}

#[test]
fn stale_incremental_recent_project_write_does_not_hide_old_session() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("stale_incremental_recent_project")?;
    write_manifest(temp.path())?;
    write_incremental_session(temp.path(), "debug", "sample-1abc", "s-old")?;
    sleep_for_mtime_tick();
    write_incremental_session(temp.path(), "debug", "sample-1abc", "s-new")?;
    fs::write(temp.path().join("target/debug/fresh-file"), b"fresh")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions {
            deep_directory_measurement: true,
            ..InventoryOptions::default()
        },
        &PlannerOptions {
            recent_write_keep_window: Some(Duration::from_secs(24 * 60 * 60)),
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let old_session = entry_for(
        &plan,
        temp.path()
            .join("target/debug/incremental/sample-1abc/s-old"),
    )?;
    assert_eq!(old_session.artifact_class, ArtifactClass::StaleIncremental);
    assert_eq!(old_session.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn stale_incremental_ignores_units_with_single_or_unmarked_session() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("stale_incremental_ignore_single")?;
    write_manifest(temp.path())?;
    write_incremental_session(temp.path(), "debug", "single-1abc", "s-only")?;
    let unmarked = temp
        .path()
        .join("target/debug/incremental/unmarked-1abc/s-old");
    fs::create_dir_all(&unmarked)?;
    sleep_for_mtime_tick();
    fs::create_dir_all(
        temp.path()
            .join("target/debug/incremental/unmarked-1abc/s-new"),
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_no_entry(
        &plan,
        temp.path()
            .join("target/debug/incremental/single-1abc/s-only"),
    );
    assert_no_entry(&plan, unmarked);
    Ok(())
}

fn write_manifest(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(path.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    Ok(())
}

fn write_fingerprint(
    root: &Path,
    profile: &str,
    family: &str,
    hash: &str,
    rustc: u64,
) -> Result<(), Box<dyn Error>> {
    write_fingerprint_contents(
        root,
        profile,
        family,
        hash,
        format!(r#"{{"rustc":{rustc}}}"#).as_bytes(),
    )
}

fn write_fingerprint_contents(
    root: &Path,
    profile: &str,
    family: &str,
    hash: &str,
    contents: &[u8],
) -> Result<(), Box<dyn Error>> {
    let dir = root
        .join("target")
        .join(profile)
        .join(".fingerprint")
        .join(format!("{family}-{hash}"));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("fingerprint.json"), contents)?;
    Ok(())
}

fn write_deps_file(
    root: &Path,
    profile: &str,
    file_name: &str,
    contents: &[u8],
) -> Result<(), Box<dyn Error>> {
    let deps = root.join("target").join(profile).join("deps");
    fs::create_dir_all(&deps)?;
    fs::write(deps.join(file_name), contents)?;
    Ok(())
}

fn write_incremental_session(
    root: &Path,
    profile: &str,
    unit: &str,
    session: &str,
) -> Result<(), Box<dyn Error>> {
    let dir = root
        .join("target")
        .join(profile)
        .join("incremental")
        .join(unit)
        .join(session);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("dep-graph.bin"), b"dep graph")?;
    fs::write(dir.join("query-cache.bin"), b"query cache")?;
    fs::write(dir.join("work-products.bin"), b"work products")?;
    Ok(())
}

fn sleep_for_mtime_tick() {
    thread::sleep(Duration::from_millis(20));
}

fn entry_for(
    plan: &cargo_reclaim::Plan,
    path: PathBuf,
) -> Result<&cargo_reclaim::PlanEntry, Box<dyn Error>> {
    plan.entries
        .iter()
        .find(|entry| entry.snapshot.path == path)
        .ok_or_else(|| format!("missing plan entry for {}", path.display()).into())
}

fn assert_no_entry(plan: &cargo_reclaim::Plan, path: PathBuf) {
    assert!(
        plan.entries.iter().all(|entry| entry.snapshot.path != path),
        "unexpected plan entry for {}",
        path.display()
    );
}

fn assert_preserved_deps_output(
    plan: &cargo_reclaim::Plan,
    path: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let entry = entry_for(plan, path)?;
    assert_eq!(entry.artifact_class, ArtifactClass::DepsOutput);
    assert_eq!(entry.action, PlanAction::Preserve);
    Ok(())
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
