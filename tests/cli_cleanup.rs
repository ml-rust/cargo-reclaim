use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod common;

use serde_json::Value;

#[test]
fn top_level_all_validates_smart_trim_without_deleting_artifact() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_all_validate")?;
    let project = write_project(temp.path(), "project")?;
    let artifact = project.join("target/debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .arg(&project)
        .arg("--all")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup validation"));
    assert!(stdout.contains("validation only"));
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn binary_accepts_existing_path_as_cleanup_root() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_path_root")?;
    let project = write_project(temp.path(), "project")?;
    let artifact = project.join("target/debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg(&project)
        .arg("--all")
        .output()?;

    assert!(output.status.success());
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn typo_command_is_not_treated_as_cleanup_root() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_typo_command")?;
    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleen", "--all"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("unknown command `cleen`"));
    Ok(())
}

#[test]
fn top_level_all_executes_smart_trim_and_preserves_target_outputs() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_all_execute")?;
    let project = write_project(temp.path(), "project")?;
    let artifact = project.join("target/debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .arg(&project)
        .args(["--all", "--yes"])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup execution"));
    assert!(!artifact.exists());
    assert!(project.join("target").is_dir());
    assert!(project.join("target/doc/index.html").is_file());
    Ok(())
}

#[test]
fn explicit_target_validates_smart_trim_without_deleting_whole_target() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_cleanup_target_validate")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");
    let artifact = target.join("debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .args(["--target"])
        .arg(&target)
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup validation"));
    assert!(target.is_dir());
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn explicit_target_smart_trim_does_not_require_containing_root() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_target_trim_no_root")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");
    let artifact = target.join("debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .args(["--target"])
        .arg(&target)
        .output()?;

    assert!(output.status.success());
    assert!(target.is_dir());
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn explicit_target_smart_trim_executes_without_deleting_whole_target() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_cleanup_target_trim_execute")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");
    let artifact = target.join("debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .args(["--target"])
        .arg(&target)
        .arg("--yes")
        .output()?;

    assert!(output.status.success());
    assert!(target.is_dir());
    assert!(!artifact.exists());
    assert!(target.join("doc/index.html").is_file());
    Ok(())
}

#[test]
fn explicit_configured_target_uses_containing_root_context() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_configured_target")?;
    let project = write_project(temp.path(), "project")?;
    let configured_target = project.join("custom-target");
    let artifact = configured_target.join("debug/incremental/unit/cache.bin");
    fs::create_dir_all(configured_target.join("debug/incremental/unit"))?;
    fs::create_dir_all(project.join(".cargo"))?;
    fs::write(
        project.join(".cargo/config.toml"),
        r#"[build]
target-dir = "custom-target"
"#,
    )?;
    fs::write(&artifact, b"configured incremental")?;

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .args(["--target"])
        .arg(&configured_target)
        .arg("--yes")
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    assert!(!artifact.exists());
    assert!(configured_target.is_dir());
    assert!(
        project
            .join("target/debug/incremental/unit/cache.bin")
            .is_file()
    );
    Ok(())
}

#[test]
fn explicit_target_delete_executes_whole_target_cleanup() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_target_delete")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup", "--target"])
        .arg(&target)
        .args(["--delete-target", "--yes"])
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup execution"));
    assert!(!target.exists());
    assert!(project.join("Cargo.toml").is_file());
    Ok(())
}

#[test]
fn explicit_target_delete_does_not_require_containing_root() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_target_delete_no_root")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup", "--target"])
        .arg(&target)
        .args(["--delete-target", "--yes"])
        .output()?;

    assert!(output.status.success());
    assert!(!target.exists());
    assert!(project.join("Cargo.toml").is_file());
    Ok(())
}

#[test]
fn yes_without_selector_returns_usage_error_without_deleting() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_yes_selector")?;
    let project = write_project(temp.path(), "project")?;
    let artifact = project.join("target/debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .arg("reclaim")
        .arg(&project)
        .arg("--yes")
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("pass --all, --target <path>"));
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn all_conflicts_with_target_selector() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_selector_conflict")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup", "--all", "--target"])
        .arg(&target)
        .arg(&project)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("--all conflicts with --target"));
    Ok(())
}

#[test]
fn validation_aliases_do_not_delete() -> Result<(), Box<dyn Error>> {
    for alias in ["--dry-run", "--validate"] {
        let temp = TestTemp::new("cli_cleanup_validation_alias")?;
        let project = write_project(temp.path(), "project")?;
        let artifact = project.join("target/debug/incremental/unit/cache.bin");

        let output = common::cargo_reclaim_command(temp.path())
            .args(["cleanup", "--all"])
            .arg(alias)
            .arg(&project)
            .output()?;

        assert!(output.status.success());
        assert!(artifact.is_file());
    }
    Ok(())
}

#[test]
fn json_output_uses_apply_report_shape() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_json")?;
    let project = write_project(temp.path(), "project")?;

    let validation = run_json(&["cleanup", "--all", "--json"], &project)?;
    assert_eq!(validation["command"], "cleanup");
    assert_eq!(validation["dry_run"], true);
    assert!(
        validation["totals"]["would_delete_count"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    let execution = run_json(&["cleanup", "--all", "--json", "--yes"], &project)?;
    assert_eq!(execution["command"], "cleanup");
    assert_eq!(execution["dry_run"], false);
    assert!(execution["totals"]["applied_count"].as_u64().unwrap_or(0) > 0);
    Ok(())
}

fn write_project(root: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let project = root.join(name);
    fs::create_dir_all(project.join("target/debug/incremental/unit"))?;
    fs::create_dir_all(project.join("target/doc"))?;
    fs::write(
        project.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\n"),
    )?;
    fs::write(
        project.join("target/debug/incremental/unit/cache.bin"),
        b"incremental",
    )?;
    fs::write(project.join("target/doc/index.html"), b"docs")?;
    Ok(project)
}

fn run_json(args: &[&str], root: &Path) -> Result<Value, Box<dyn Error>> {
    let output = common::cargo_reclaim_command(root)
        .args(args)
        .arg(root)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    Ok(serde_json::from_slice(&output.stdout)?)
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
