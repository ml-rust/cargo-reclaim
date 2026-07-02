use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn preview_systemd_terminal_reports_dry_run_and_artifacts() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_systemd")?;
    let config_path = write_config(temp.path(), "")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "scheduler",
            "preview",
            "--platform",
            "systemd-user",
            "--config",
        ])
        .arg(&config_path)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim scheduler preview"));
    assert!(stdout.contains("dry-run only"));
    assert!(stdout.contains("no scheduler files were installed"));
    assert!(stdout.contains("systemd-service"));
    assert!(stdout.contains("systemd-timer"));
    assert!(stdout.contains("runner-script"));
    Ok(())
}

#[test]
fn preview_json_reports_stable_shape() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_json")?;
    let config_path = write_config(
        temp.path(),
        r#"
[scheduler]
at = "04:30"
mode = "cleanup"
allow_unattended_cleanup = true
"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "preview", "--platform", "launchd", "--config"])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scheduler-preview");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["platform"], "launchd");
    assert_eq!(document["mode"], "cleanup");
    assert_eq!(document["effective_policy"], "conservative");
    assert_eq!(document["at"], "04:30");
    assert!(document["artifacts"].as_array().expect("artifacts").len() >= 2);
    Ok(())
}

#[test]
fn preview_does_not_create_intended_install_files() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_no_write")?;
    let config_path = write_config(
        temp.path(),
        r#"
[scheduler]
state_dir = "state"
log_dir = "logs"
"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "scheduler",
            "preview",
            "--platform",
            "systemd-user",
            "--config",
        ])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    for artifact in document["artifacts"].as_array().expect("artifacts") {
        let Some(path) = artifact["intended_install_path"].as_str() else {
            continue;
        };
        if path.starts_with(temp.path().to_string_lossy().as_ref()) {
            assert!(!Path::new(path).exists(), "{path} should not be created");
        }
    }
    assert!(!temp.path().join("state").exists());
    assert!(!temp.path().join("logs").exists());
    Ok(())
}

#[test]
fn unsafe_cleanup_and_high_policy_exit_usage_code() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_unsafe")?;
    let config_path = write_config(temp.path(), "")?;

    let cleanup = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "scheduler",
            "preview",
            "--platform",
            "systemd-user",
            "--config",
        ])
        .arg(&config_path)
        .args(["--mode", "cleanup"])
        .output()?;
    assert_eq!(cleanup.status.code(), Some(2));

    let high = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "scheduler",
            "preview",
            "--platform",
            "systemd-user",
            "--config",
        ])
        .arg(&config_path)
        .args([
            "--mode",
            "cleanup",
            "--policy",
            "balanced",
            "--allow-unattended-cleanup",
        ])
        .output()?;
    assert_eq!(high.status.code(), Some(2));
    Ok(())
}

#[test]
fn help_lists_scheduler_preview_but_not_install_uninstall_commands() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("scheduler preview"));
    assert!(!stdout.contains("scheduler install"));
    assert!(!stdout.contains("scheduler uninstall"));
    Ok(())
}

fn write_config(path: &Path, body: &str) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = path.join("reclaim.toml");
    fs::write(&config_path, format!("version = 1\n{body}"))?;
    Ok(config_path)
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
