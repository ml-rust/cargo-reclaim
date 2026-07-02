use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, InventoryOptions, PlanAction, PlannerOptions, PolicyKind, ScannerOptions,
    TargetCandidate, TargetCandidateKind, TargetEvidence, build_plan_from_roots,
    build_plan_from_roots_with_options, build_plan_from_scan_items,
    planner_candidates_from_target_root,
};

#[test]
fn scanned_project_target_builds_policy_plan_from_artifact_boundaries() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_project_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::create_dir_all(temp.path().join("target/mystery"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;
    fs::write(temp.path().join("target/mystery/blob"), b"unknown")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.artifact_class, ArtifactClass::Incremental);
    assert_eq!(incremental.action, PlanAction::Delete);
    assert_eq!(incremental.snapshot.size_bytes, 3);

    let docs = entry_for(&plan, temp.path().join("target/doc"))?;
    assert_eq!(docs.artifact_class, ArtifactClass::Docs);
    assert_eq!(docs.action, PlanAction::Preserve);

    let unknown = entry_for(&plan, temp.path().join("target/mystery/blob"))?;
    assert_eq!(unknown.artifact_class, ArtifactClass::Unknown);
    assert_eq!(unknown.action, PlanAction::Unknown);
    Ok(())
}

#[test]
fn configured_custom_target_builds_policy_plan_from_scanned_roots() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_configured_target_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"custom-target\"\n",
    )?;
    fs::create_dir_all(temp.path().join("custom-target/debug/incremental"))?;
    fs::write(
        temp.path()
            .join("custom-target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("custom-target/debug/incremental"))?;
    assert_eq!(incremental.artifact_class, ArtifactClass::Incremental);
    assert_eq!(incremental.action, PlanAction::Delete);
    assert!(matches!(
        incremental.evidence,
        TargetEvidence::ConfiguredPath { ref source } if source.contains("build.target-dir")
    ));
    Ok(())
}

#[test]
fn observe_policy_preserves_delete_capable_entries_from_scanned_roots() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_observe_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Observe,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::Preserve);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    Ok(())
}

#[test]
fn recent_write_keep_window_skips_scanned_delete_candidates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_recent_write")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            recent_write_keep_window: Some(std::time::Duration::from_secs(24 * 60 * 60)),
        },
        SystemTime::now(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::SkipActive);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    assert_eq!(plan.totals.preserved_count, 1);
    Ok(())
}

#[test]
fn protected_outputs_are_preserved_from_scanned_target_contents() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_protected_outputs")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/package"))?;
    fs::create_dir_all(temp.path().join("target/timings"))?;
    fs::create_dir_all(temp.path().join("target/debug"))?;
    fs::write(temp.path().join("target/package/sample.crate"), b"crate")?;
    fs::write(
        temp.path().join("target/timings/cargo-timing.html"),
        b"time",
    )?;
    fs::write(temp.path().join("target/debug/sample"), b"bin")?;
    fs::write(temp.path().join("target/debug/libsample.rlib"), b"rlib")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    for (path, artifact_class) in [
        (temp.path().join("target/package"), ArtifactClass::Package),
        (temp.path().join("target/timings"), ArtifactClass::Timings),
        (
            temp.path().join("target/debug/sample"),
            ArtifactClass::FinalExecutable,
        ),
        (
            temp.path().join("target/debug/libsample.rlib"),
            ArtifactClass::FinalRlib,
        ),
    ] {
        let entry = entry_for(&plan, path)?;
        assert_eq!(entry.artifact_class, artifact_class);
        assert_eq!(entry.action, PlanAction::Preserve);
    }

    Ok(())
}

#[test]
fn weak_name_only_targets_are_suppressed_by_default_and_confirmation_gated_when_allowed()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_weak_targets")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let suppressed = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    assert!(suppressed.entries.is_empty());

    let allowed = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&allowed, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::RequiresConfirmation);
    assert!(incremental.requires_confirmation);
    assert_eq!(
        incremental.evidence,
        TargetEvidence::weak_name_only("target")?
    );
    Ok(())
}

#[test]
fn lower_level_scan_item_planning_still_requires_explicit_weak_target_option()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_weak_scan_items")?;
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let input = cargo_reclaim::PlanInput::from_root(temp.path())?;
    let weak_candidate = cargo_reclaim::ScanItem::TargetCandidate(TargetCandidate {
        path: target.clone(),
        kind: TargetCandidateKind::CargoTargetDir,
        evidence: Some(TargetEvidence::weak_name_only("target")?),
        skip_reason: None,
    });

    let suppressed = build_plan_from_scan_items(
        input.clone(),
        PolicyKind::Balanced,
        [weak_candidate.clone()],
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    assert!(suppressed.entries.is_empty());

    let allowed = build_plan_from_scan_items(
        input,
        PolicyKind::Balanced,
        [weak_candidate],
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;
    assert_eq!(allowed.entries.len(), 1);
    assert_eq!(allowed.entries[0].action, PlanAction::RequiresConfirmation);
    Ok(())
}

#[test]
fn duplicate_scan_roots_do_not_duplicate_plan_entries() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_dedupe")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path(), temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].snapshot.path,
        temp.path().join("target/debug/incremental")
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlinked_target_root_is_rejected_by_inventory_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("integration_symlinked_target_root")?;
    let real_target = temp.path().join("real_target");
    fs::create_dir_all(real_target.join("debug/incremental"))?;
    fs::write(real_target.join("debug/incremental/cache.bin"), b"abc")?;
    let linked_target = temp.path().join("target");
    symlink(&real_target, &linked_target)?;

    let result = planner_candidates_from_target_root(
        &linked_target,
        TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        &InventoryOptions::default(),
    );

    assert!(matches!(
        result,
        Err(cargo_reclaim::ReclaimError::InventorySymlinkNotFollowed { path })
            if path == linked_target
    ));
    Ok(())
}

#[test]
#[cfg(unix)]
fn target_content_symlinks_are_not_planned_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("integration_target_symlink")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("outside"))?;
    fs::write(temp.path().join("outside/file.d"), b"outside")?;
    fs::create_dir(temp.path().join("target"))?;
    symlink(
        temp.path().join("outside"),
        temp.path().join("target/linked"),
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert!(plan.entries.is_empty());
    Ok(())
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

fn write_manifest(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(path.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
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
