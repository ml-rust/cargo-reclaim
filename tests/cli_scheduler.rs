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
fn install_dry_run_json_reports_plan_and_does_not_create_files() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_install")?;
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
            "install",
            "--dry-run",
            "--platform",
            "systemd-user",
            "--config",
        ])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scheduler-install");
    assert_eq!(document["operation"], "install");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["platform"], "systemd-user");
    assert!(document["artifacts"].as_array().expect("artifacts").len() >= 2);
    assert!(
        document["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .any(|step| {
                step["kind"] == "run-command"
                    && step["argv"] == serde_json::json!(["systemctl", "--user", "daemon-reload"])
            })
    );
    assert!(!temp.path().join("state").exists());
    assert!(!temp.path().join("logs").exists());
    Ok(())
}

#[test]
fn uninstall_dry_run_json_reports_removal_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_uninstall")?;
    let config_path = write_config(temp.path(), "")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "scheduler",
            "uninstall",
            "--dry-run",
            "--platform",
            "launchd",
            "--config",
        ])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scheduler-uninstall");
    assert_eq!(document["operation"], "uninstall");
    assert!(
        document["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .any(|step| {
                step["kind"] == "run-command"
                    && step["argv"]
                        == serde_json::json!(["launchctl", "remove", "com.cargo-reclaim"])
            })
    );
    assert!(!temp.path().join("state").exists());
    assert!(!temp.path().join("logs").exists());
    Ok(())
}

#[test]
fn run_json_builds_plan_and_appends_run_log() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_run")?;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    let config_path = write_config(
        temp.path(),
        &format!("roots = [{}]\n", toml_string(&project)),
    )?;
    let log_path = temp.path().join("scheduler.jsonl");
    let plan_path = temp.path().join("plans/run.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "run", "--config"])
        .arg(&config_path)
        .args(["--run-id", "run-test", "--log-path"])
        .arg(&log_path)
        .arg("--plan-path")
        .arg(&plan_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scheduler-run");
    assert_eq!(document["run_id"], "run-test");
    assert!(document["plan_id"].as_str().is_some());
    assert!(plan_path.is_file());
    let log = fs::read_to_string(log_path)?;
    assert!(log.lines().count() >= 3);
    assert!(log.contains("\"run_id\":\"run-test\""));
    Ok(())
}

#[test]
fn run_threshold_mode_uses_measured_target_size() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_run_threshold")?;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    let config_path = write_config(
        temp.path(),
        &format!(
            "roots = [{}]\n[policy]\nmax_target_size = \"3 B\"\n[background]\nmode = \"threshold\"\n",
            toml_string(&project)
        ),
    )?;
    let log_path = temp.path().join("scheduler.jsonl");
    let plan_path = temp.path().join("plans/run.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "run", "--config"])
        .arg(&config_path)
        .args(["--run-id", "run-threshold", "--log-path"])
        .arg(&log_path)
        .arg("--plan-path")
        .arg(&plan_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["trigger"]["state"], "triggered_plan_only");
    assert_eq!(document["trigger"]["reason_count"], 1);
    assert!(document["plan_id"].as_str().is_some());
    let log = fs::read_to_string(log_path)?;
    assert!(log.contains("\"state\":\"triggered_plan_only\""));
    assert!(log.contains("\"kind\":\"target_size_exceeded\""));
    Ok(())
}

#[test]
fn run_config_whole_target_confirm_persists_root_entry() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_run_whole_target")?;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    let config_path = write_config(
        temp.path(),
        &format!(
            "roots = [{}]\n[policy]\nwhole_target = \"confirm\"\n",
            toml_string(&project)
        ),
    )?;
    let log_path = temp.path().join("scheduler.jsonl");
    let plan_path = temp.path().join("plans/run.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "run", "--config"])
        .arg(&config_path)
        .args(["--run-id", "run-whole-target", "--log-path"])
        .arg(&log_path)
        .arg("--plan-path")
        .arg(&plan_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let entries = persisted["plan"]["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["artifact_class"], "whole_target");
    assert_eq!(entries[0]["action"], "requires_confirmation");
    assert_eq!(
        persisted["invocation"]["planner_options"]["whole_target_mode"],
        "confirm"
    );
    Ok(())
}

#[test]
fn run_config_whole_target_delete_requires_allow_flag() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_run_whole_target_gate")?;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    let config_path = write_config(
        temp.path(),
        &format!(
            "roots = [{}]\n[policy]\nmode = \"aggressive\"\nwhole_target = \"delete\"\n",
            toml_string(&project)
        ),
    )?;
    let log_path = temp.path().join("scheduler.jsonl");
    let plan_path = temp.path().join("plans/run.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "run", "--config"])
        .arg(&config_path)
        .args(["--run-id", "run-whole-target-gate", "--log-path"])
        .arg(&log_path)
        .arg("--plan-path")
        .arg(&plan_path)
        .arg("--json")
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("allow_unattended_whole_target_delete"));
    assert!(!plan_path.exists());
    Ok(())
}

#[test]
fn run_missing_required_flags_exit_usage_code() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_run_usage")?;
    let config_path = write_config(temp.path(), "")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "run", "--config"])
        .arg(&config_path)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("scheduler run requires --run-id"));
    Ok(())
}

#[test]
fn help_lists_scheduler_dry_run_operation_commands() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("scheduler preview"));
    assert!(stdout.contains("scheduler install --dry-run"));
    assert!(stdout.contains("scheduler uninstall --dry-run"));
    Ok(())
}

#[test]
fn scheduler_preview_help_exits_success_and_writes_stdout() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "preview", "--help"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("usage: cargo-reclaim scheduler preview"));
    Ok(())
}

fn write_config(path: &Path, body: &str) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = path.join("reclaim.toml");
    fs::write(&config_path, format!("version = 1\n{body}"))?;
    Ok(config_path)
}

fn toml_string(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('\\', "\\\\"))
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
