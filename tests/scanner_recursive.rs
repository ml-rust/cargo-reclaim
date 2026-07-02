use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ScanItem, ScanSkipReason, ScannerOptions, TargetCandidate, TargetEvidence, scan_roots,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn scan_finds_project_and_adjacent_target() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_project_target")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join("target"))?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;
    let expected_evidence = TargetEvidence::project_context(temp.path().join("Cargo.toml"))?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::CargoProject(project)
            if project.manifest_path == temp.path().join("Cargo.toml")
    )));
    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == temp.path().join("target")
            && candidate.evidence.as_ref() == Some(&expected_evidence)
    }));
    Ok(())
}

#[test]
fn scan_skips_default_ignored_dirs_without_recursing() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_default_ignores")?;
    for dir_name in [".git", ".cargo", "node_modules"] {
        let ignored = temp.path().join(dir_name);
        fs::create_dir(&ignored)?;
        write_manifest(&ignored)?;
    }

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    for dir_name in [".git", ".cargo", "node_modules"] {
        let ignored = temp.path().join(dir_name);
        assert!(items.iter().any(|item| matches!(
            item,
            ScanItem::Skipped(skip)
                if skip.path == ignored && skip.reason == ScanSkipReason::DefaultIgnoredDir
        )));
        assert!(!items.iter().any(|item| matches!(
            item,
            ScanItem::CargoProject(project) if project.root_path == ignored
        )));
    }
    Ok(())
}

#[test]
fn scan_suppresses_weak_name_only_targets_by_default() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_weak_default")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == target && skip.reason == ScanSkipReason::WeakNameOnlySuppressed
    )));
    assert!(!target_candidates(&items).any(|candidate| candidate.path == target));
    Ok(())
}

#[test]
fn scan_emits_weak_name_only_targets_when_enabled() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_weak_allowed")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;

    let items = scan_roots(
        [temp.path()],
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
    )?;
    let expected_evidence = TargetEvidence::weak_name_only("target")?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target && candidate.evidence.as_ref() == Some(&expected_evidence)
    }));
    Ok(())
}

#[test]
#[cfg(unix)]
fn scan_skips_symlinked_dirs_by_default() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("recursive_symlink_default")?;
    let real = temp.path().join("real");
    fs::create_dir(&real)?;
    write_manifest(&real)?;
    let linked = temp.path().join("linked");
    symlink(&real, &linked)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == linked && skip.reason == ScanSkipReason::SymlinkNotFollowed
    )));
    assert!(!items.iter().any(|item| matches!(
        item,
        ScanItem::CargoProject(project) if project.root_path == linked
    )));
    Ok(())
}

#[test]
#[cfg(unix)]
fn scan_follows_symlinked_dirs_when_enabled() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("recursive_symlink_follow")?;
    let real = temp.path().join("real");
    fs::create_dir(&real)?;
    write_manifest(&real)?;
    let linked = temp.path().join("linked");
    symlink(&real, &linked)?;

    let items = scan_roots(
        [temp.path()],
        &ScannerOptions {
            follow_symlinks: true,
            ..ScannerOptions::default()
        },
    )?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::CargoProject(project) if project.root_path == linked
    )));
    Ok(())
}

#[test]
#[cfg(unix)]
fn scan_following_symlinks_does_not_revisit_directory_cycles() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("recursive_symlink_cycle")?;
    let real = temp.path().join("real");
    fs::create_dir(&real)?;
    write_manifest(&real)?;
    symlink(&real, real.join("cycle"))?;

    let items = scan_roots(
        [temp.path()],
        &ScannerOptions {
            follow_symlinks: true,
            ..ScannerOptions::default()
        },
    )?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == real.join("cycle") && skip.reason == ScanSkipReason::AlreadyVisited
    )));
    Ok(())
}

#[test]
fn scan_honors_configured_ignored_path() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_configured_ignore")?;
    let ignored = temp.path().join("ignored");
    fs::create_dir(&ignored)?;
    write_manifest(&ignored)?;

    let items = scan_roots(
        [temp.path()],
        &ScannerOptions {
            ignored_paths: vec![ignored.clone()],
            ..ScannerOptions::default()
        },
    )?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == ignored && skip.reason == ScanSkipReason::ConfiguredIgnoredPath
    )));
    assert!(!items.iter().any(|item| matches!(
        item,
        ScanItem::CargoProject(project) if project.root_path == ignored
    )));
    Ok(())
}

#[test]
fn scan_nested_project_target_uses_nearest_project_context() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_nested_context")?;
    write_manifest(temp.path())?;
    let nested = temp.path().join("crates").join("member");
    fs::create_dir_all(&nested)?;
    write_manifest(&nested)?;
    let target = nested.join("target");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;
    let expected_evidence = TargetEvidence::project_context(nested.join("Cargo.toml"))?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target && candidate.evidence.as_ref() == Some(&expected_evidence)
    }));
    Ok(())
}

fn write_manifest(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(path.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    Ok(())
}

fn target_candidates<'a>(items: &'a [ScanItem]) -> impl Iterator<Item = &'a TargetCandidate> + 'a {
    items.iter().filter_map(|item| match item {
        ScanItem::TargetCandidate(candidate) => Some(candidate),
        _ => None,
    })
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

fn test_lock() -> Result<std::sync::MutexGuard<'static, ()>, Box<dyn Error>> {
    ENV_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("env lock poisoned").into())
}
