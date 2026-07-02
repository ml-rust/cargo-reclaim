use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, InventoryOptions, PathKind, ReclaimError, TargetEvidence,
    planner_candidate_from_target_relative_path, snapshot_path, snapshot_target_relative_path,
};

#[test]
fn file_snapshot_returns_exact_size() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("inventory_file")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("debug.d"), b"abcdef")?;

    let snapshot = snapshot_target_relative_path(&target, "debug.d", &InventoryOptions::default())?;

    assert_eq!(snapshot.path, target.join("debug.d"));
    assert_eq!(snapshot.size_bytes, 6);
    assert_eq!(snapshot.path_kind, PathKind::File);
    assert!(snapshot.modified.is_some());
    Ok(())
}

#[test]
fn directory_snapshot_aggregates_child_file_sizes_only() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("inventory_directory")?;
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/deps/nested"))?;
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/deps/lib.rlib"), b"abcd")?;
    fs::write(target.join("debug/deps/nested/unit.o"), b"abcdef")?;
    fs::write(target.join("debug/incremental/cache.bin"), b"not counted")?;

    let snapshot =
        snapshot_target_relative_path(&target, "debug/deps", &InventoryOptions::default())?;

    assert_eq!(snapshot.path, target.join("debug/deps"));
    assert_eq!(snapshot.size_bytes, 10);
    assert_eq!(snapshot.path_kind, PathKind::Directory);
    Ok(())
}

#[test]
fn root_snapshot_measures_target_directory_without_relative_child() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("inventory_root_snapshot")?;
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/deps"))?;
    fs::write(target.join("debug/deps/lib.rlib"), b"abcd")?;
    fs::write(target.join("debug/app"), b"abcdef")?;

    let snapshot = snapshot_path(&target, &InventoryOptions::default())?;

    assert_eq!(snapshot.path, target);
    assert_eq!(snapshot.size_bytes, 10);
    assert_eq!(snapshot.path_kind, PathKind::Directory);
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_snapshot_is_rejected_by_default_and_followed_when_enabled() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("inventory_symlink")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("real.d"), b"abc")?;
    symlink(target.join("real.d"), target.join("linked.d"))?;

    let rejected = snapshot_target_relative_path(&target, "linked.d", &InventoryOptions::default());
    assert!(matches!(
        rejected,
        Err(ReclaimError::InventorySymlinkNotFollowed { .. })
    ));

    let followed = snapshot_target_relative_path(
        &target,
        "linked.d",
        &InventoryOptions {
            follow_symlinks: true,
        },
    )?;

    assert_eq!(followed.size_bytes, 3);
    assert_eq!(followed.path_kind, PathKind::File);
    Ok(())
}

#[test]
fn absolute_child_path_and_parent_escape_are_rejected() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("inventory_rejects_paths")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;

    let absolute = snapshot_target_relative_path(
        &target,
        temp.path().join("target/debug/deps"),
        &InventoryOptions::default(),
    );
    assert!(matches!(
        absolute,
        Err(ReclaimError::AbsoluteInventoryChildPath { .. })
    ));

    let escape = snapshot_target_relative_path(
        &target,
        "../target/debug/deps",
        &InventoryOptions::default(),
    );
    assert!(matches!(
        escape,
        Err(ReclaimError::InventoryPathEscape { .. })
    ));
    Ok(())
}

#[test]
fn inventory_candidate_classifies_child_path_and_preserves_evidence() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("inventory_candidate")?;
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let evidence = TargetEvidence::project_context(temp.path().join("Cargo.toml"))?;

    let candidate = planner_candidate_from_target_relative_path(
        &target,
        "debug/incremental",
        evidence.clone(),
        &InventoryOptions::default(),
    )?;

    assert_eq!(candidate.artifact_class, ArtifactClass::Incremental);
    assert_eq!(candidate.evidence, evidence);
    assert_eq!(candidate.snapshot.path, target.join("debug/incremental"));
    assert_eq!(candidate.snapshot.size_bytes, 3);
    Ok(())
}

#[test]
fn missing_path_surfaces_stable_error() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("inventory_missing")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;

    let result = snapshot_target_relative_path(&target, "debug/deps", &InventoryOptions::default());

    assert_eq!(
        result,
        Err(ReclaimError::MissingInventoryPath {
            path: target.join("debug/deps")
        })
    );
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

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
