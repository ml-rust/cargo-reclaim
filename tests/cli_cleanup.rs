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
fn terminal_cleanup_output_summarizes_entries_and_writes_full_report() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_cleanup_terminal_report")?;
    let project = write_project(temp.path(), "project")?;
    let state_root = temp.path().join("state-root");

    let output = common::cargo_reclaim_command(temp.path())
        .env("HOME", &state_root)
        .env("USERPROFILE", &state_root)
        .env("LOCALAPPDATA", state_root.join("AppData/Local"))
        .arg("reclaim")
        .arg(&project)
        .args(["--all", "--yes"])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup execution"));
    assert!(stdout.contains("full report: "));
    assert!(stdout.contains("notable entries:"));
    assert!(stdout.contains("deleted\tdelete\t"));
    assert!(stdout.contains("entries not shown:"));
    assert!(!stdout.contains("not_planned_for_deletion"));
    assert!(!stdout.contains("target/doc/index.html"));

    let report_path = full_report_path(&stdout)?;
    assert!(report_path.is_file());
    let report: Value = serde_json::from_str(&fs::read_to_string(report_path)?)?;
    assert_eq!(report["command"], "cleanup");
    assert_eq!(report["dry_run"], false);
    let entries = report["entries"]
        .as_array()
        .ok_or("full report entries must be an array")?;
    assert!(
        entries
            .iter()
            .any(|entry| entry["status"] == "not_planned_for_deletion")
    );
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
    assert!(stderr.contains("pass --all or --target <path>"));
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn non_tty_no_selector_returns_usage_error_without_reading_stdin() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_no_selector_non_tty")?;
    let project = write_project(temp.path(), "project")?;
    let artifact = project.join("target/debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup"])
        .arg(&project)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("interactive terminal"));
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn non_tty_selector_bypasses_terminal_assistant() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_selector_non_tty")?;
    let project = write_project(temp.path(), "project")?;
    let target = project.join("target");
    let artifact = target.join("debug/incremental/unit/cache.bin");

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup", "--target"])
        .arg(&target)
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cleanup validation"));
    assert!(stdout.contains("validation only"));
    assert!(artifact.is_file());
    Ok(())
}

#[test]
fn json_no_selector_emits_structured_usage_error() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cleanup_no_selector_json")?;
    let project = write_project(temp.path(), "project")?;

    let output = common::cargo_reclaim_command(temp.path())
        .args(["cleanup", "--json"])
        .arg(&project)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stdout)?.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr)?;
    assert_eq!(error["error"]["kind"], "usage");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("interactive terminal")
    );
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

fn full_report_path(stdout: &str) -> Result<PathBuf, Box<dyn Error>> {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("full report: ").map(PathBuf::from))
        .ok_or_else(|| "missing full report path".into())
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
