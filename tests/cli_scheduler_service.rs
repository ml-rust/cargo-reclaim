use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn service_run_json_creates_state_log_and_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_service_run")?;
    let project = temp.path().join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    let config_path = write_config(
        temp.path(),
        &format!(
            "roots = [{}]\n[background]\ncheck_every = \"1s\"\n[scheduler]\nstate_dir = \"state\"\nlog_dir = \"logs\"\n",
            toml_string(&project)
        ),
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "run", "--config"])
        .arg(&config_path)
        .args(["--max-cycles", "1", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scheduler-service-run");
    assert_eq!(document["status"], "stopped");
    assert_eq!(document["cycles_completed"], 1);
    assert!(temp.path().join("state/service-state.json").is_file());
    assert!(temp.path().join("logs/runs.jsonl").is_file());
    assert!(fs::read_dir(temp.path().join("state/plans"))?.count() >= 1);
    Ok(())
}

#[test]
fn service_status_json_reports_persisted_state() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_service_status")?;
    let config_path = write_config(
        temp.path(),
        "[scheduler]\nstate_dir = \"state\"\nlog_dir = \"logs\"\n",
    )?;

    let missing = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "status", "--config"])
        .arg(&config_path)
        .arg("--json")
        .output()?;
    assert!(missing.status.success());
    let missing_document: Value = serde_json::from_slice(&missing.stdout)?;
    assert_eq!(missing_document["command"], "scheduler-service-status");
    assert_eq!(missing_document["status"], "unknown");

    fs::create_dir_all(temp.path().join("state"))?;
    fs::write(
        temp.path().join("state/service-state.json"),
        r#"{
  "schema_version": 1,
  "status": "running",
  "pid": 4242,
  "started_at": {"unix_seconds": 10, "nanoseconds": 0},
  "last_run_id": "scheduler-status-test",
  "last_run_at": {"unix_seconds": 20, "nanoseconds": 0},
  "next_run_at": {"unix_seconds": 30, "nanoseconds": 0},
  "consecutive_failures": 0,
  "last_problem": null
}"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "status", "--config"])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["status"], "running");
    assert_eq!(document["pid"], 4242);
    assert_eq!(document["last_run_id"], "scheduler-status-test");

    let terminal = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "status", "--config"])
        .arg(&config_path)
        .output()?;
    assert!(terminal.status.success());
    let stdout = String::from_utf8(terminal.stdout)?;
    assert!(stdout.contains("cargo-reclaim scheduler service: running"));
    assert!(stdout.contains("pid: 4242"));
    assert!(stdout.contains("last run: scheduler-status-test"));
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
