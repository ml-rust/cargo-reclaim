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
fn scan_detects_project_configured_custom_target_without_name_only_option()
-> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_configured_target")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"custom-output\"\n",
    )?;
    let target = temp.path().join("custom-output");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ConfiguredPath { source }) if source.contains("build.target-dir")
            )
    }));
    Ok(())
}

#[test]
fn scan_ancestor_config_affects_nested_project() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_ancestor_config")?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"ancestor-target\"\n",
    )?;
    let nested = temp.path().join("workspace").join("member");
    fs::create_dir_all(&nested)?;
    write_manifest(&nested)?;
    let target = temp.path().join("ancestor-target");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ConfiguredPath { source }) if source.contains("build.target-dir")
            )
    }));
    Ok(())
}

#[test]
fn scan_env_target_dir_detects_outside_default_named_path() -> Result<(), Box<dyn Error>> {
    let _env_guard = EnvGuard::set("CARGO_BUILD_TARGET_DIR", "outside-output")?;
    let temp = TestTemp::new("recursive_env_target")?;
    write_manifest(temp.path())?;
    let target = temp.path().join("outside-output");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ConfiguredPath { source }) if source == "CARGO_BUILD_TARGET_DIR"
            )
    }));
    Ok(())
}

#[test]
fn scan_reads_cargo_config_but_still_skips_cargo_directory() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_cargo_skipped_read")?;
    write_manifest(temp.path())?;
    let cargo_dir = temp.path().join(".cargo");
    fs::create_dir(&cargo_dir)?;
    fs::write(
        cargo_dir.join("config.toml"),
        "[build]\ntarget-dir = \"configured-target\"\n",
    )?;
    fs::create_dir(cargo_dir.join("target"))?;
    let target = temp.path().join("configured-target");
    fs::create_dir(&target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == cargo_dir && skip.reason == ScanSkipReason::DefaultIgnoredDir
    )));
    assert!(target_candidates(&items).any(|candidate| candidate.path == target));
    assert!(!items.iter().any(|item| matches!(
        item,
        ScanItem::TargetCandidate(candidate) if candidate.path == temp.path().join(".cargo/target")
    )));
    Ok(())
}

#[test]
fn scan_detects_distinct_configured_build_dir() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_build_dir")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"target-out\"\nbuild-dir = \"build-out\"\n",
    )?;
    let target = temp.path().join("target-out");
    let build = temp.path().join("build-out");
    fs::create_dir(&target)?;
    fs::create_dir(&build)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == target
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ConfiguredPath { source }) if source.contains("build.target-dir")
            )
    }));
    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == build
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ConfiguredPath { source }) if source.contains("build.build-dir")
            )
    }));
    Ok(())
}

#[test]
fn scan_reports_unsupported_cargo_build_dir_template() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_unsupported_build_dir")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nbuild-dir = \"{workspace-root}/{workspace-path-hash}\"\n",
    )?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == temp.path()
                && matches!(
                    &skip.reason,
                    ScanSkipReason::CargoConfigUnsupported { message }
                        if message.contains("{workspace-path-hash}")
                )
    )));
    Ok(())
}

#[test]
fn scan_reports_malformed_cargo_config_without_stopping_project_scan() -> Result<(), Box<dyn Error>>
{
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_malformed_cargo_config")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(temp.path().join(".cargo/config.toml"), "[build\n")?;
    fs::create_dir(temp.path().join("target"))?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == temp.path()
                && matches!(
                    &skip.reason,
                    ScanSkipReason::CargoConfigProblem { message }
                        if message.contains("config.toml")
                )
    )));
    assert!(target_candidates(&items).any(|candidate| {
        candidate.path == temp.path().join("target")
            && matches!(
                candidate.evidence.as_ref(),
                Some(TargetEvidence::ProjectContext { .. })
            )
    }));
    Ok(())
}

#[test]
fn scan_skips_configured_output_under_default_ignored_directory() -> Result<(), Box<dyn Error>> {
    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_configured_output_ignored")?;
    write_manifest(temp.path())?;
    let cargo_dir = temp.path().join(".cargo");
    fs::create_dir(&cargo_dir)?;
    fs::write(
        cargo_dir.join("config.toml"),
        "[build]\ntarget-dir = \".cargo/target\"\n",
    )?;
    fs::create_dir(cargo_dir.join("target"))?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == cargo_dir.join("target")
                && skip.reason == ScanSkipReason::DefaultIgnoredDir
    )));
    assert!(!target_candidates(&items).any(|candidate| candidate.path == cargo_dir.join("target")));
    Ok(())
}

#[test]
#[cfg(unix)]
fn scan_skips_configured_symlink_output_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let _test_guard = test_lock()?;
    let temp = TestTemp::new("recursive_configured_symlink_output")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"linked-target\"\n",
    )?;
    let real_target = temp.path().join("real-target");
    fs::create_dir(&real_target)?;
    let linked_target = temp.path().join("linked-target");
    symlink(&real_target, &linked_target)?;

    let items = scan_roots([temp.path()], &ScannerOptions::default())?;

    assert!(items.iter().any(|item| matches!(
        item,
        ScanItem::Skipped(skip)
            if skip.path == linked_target && skip.reason == ScanSkipReason::SymlinkNotFollowed
    )));
    assert!(!target_candidates(&items).any(|candidate| candidate.path == linked_target));
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

struct EnvGuard {
    key: &'static str,
    old_value: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Result<Self, Box<dyn Error>> {
        let lock = ENV_LOCK
            .lock()
            .map_err(|_| std::io::Error::other("env lock poisoned"))?;
        let old_value = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Ok(Self {
            key,
            old_value,
            _lock: lock,
        })
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = self.old_value.as_ref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
