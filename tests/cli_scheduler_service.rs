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
            "roots = [{}]\n[[background.trigger]]\nevery = \"1s\"\n[scheduler]\nstate_dir = \"state\"\nlog_dir = \"logs\"\n",
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
    assert_eq!(
        document["paths"]["state_path"],
        state_path(temp.path()).display().to_string()
    );
    assert_eq!(
        document["paths"]["run_log_path"],
        run_log_path(temp.path()).display().to_string()
    );
    assert!(state_path(temp.path()).is_file());
    assert!(run_log_path(temp.path()).is_file());
    assert!(fs::read_dir(plans_dir(temp.path()))?.count() >= 1);
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
    fs::create_dir_all(temp.path().join("logs"))?;
    let pid = std::process::id();
    fs::write(
        state_path(temp.path()),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "status": "running",
            "pid": pid,
            "started_at": {"unix_seconds": 10, "nanoseconds": 0},
            "last_run_id": "scheduler-status-test",
            "last_run_at": {"unix_seconds": 20, "nanoseconds": 0},
            "next_run_at": {"unix_seconds": 30, "nanoseconds": 0},
            "consecutive_failures": 0,
            "last_problem": null,
        }))?,
    )?;
    fs::write(
        run_log_path(temp.path()),
        r#"{"schema_version":1,"run_id":"run-1","recorded_at":{"unix_seconds":20,"nanoseconds":0},"event":"started","trigger":null,"selected_policy":"conservative","plan":null,"skipped_projects":[],"apply":null,"recommendations":[],"problems":[]}
not-json
{"schema_version":1,"run_id":"run-1","recorded_at":{"unix_seconds":21,"nanoseconds":0},"event":"apply_completed","trigger":null,"selected_policy":"conservative","plan":null,"skipped_projects":[],"apply":{"plan_id":"sha256:test","dry_run":false,"totals":{"entry_count":2,"delete_candidate_count":1,"would_delete_count":0,"skipped_count":1,"stale_skip_count":0,"applied_count":1,"failed_count":0,"would_delete_bytes":0,"applied_bytes":1234},"notable_entries":[]},"recommendations":[],"problems":[]}
{"schema_version":1,"run_id":"run-2","recorded_at":{"unix_seconds":22,"nanoseconds":0},"event":"started","trigger":null,"selected_policy":"balanced","plan":null,"skipped_projects":[],"apply":null,"recommendations":[],"problems":[]}
{"schema_version":1,"run_id":"run-2","recorded_at":{"unix_seconds":23,"nanoseconds":0},"event":"failed","trigger":null,"selected_policy":"balanced","plan":null,"skipped_projects":[],"apply":null,"recommendations":[],"problems":["simulated failure"]}
"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "status", "--config"])
        .arg(&config_path)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["status"], "running");
    assert_eq!(document["pid"], pid);
    assert_eq!(document["last_run_id"], "scheduler-status-test");
    assert_eq!(
        document["paths"]["state_path"],
        state_path(temp.path()).display().to_string()
    );
    assert_eq!(
        document["paths"]["log_dir"],
        temp.path().join("logs").display().to_string()
    );
    assert_eq!(
        document["paths"]["run_log_path"],
        run_log_path(temp.path()).display().to_string()
    );
    assert_eq!(document["run_log"]["record_count"], 4);
    assert_eq!(document["run_log"]["corrupt_record_count"], 1);
    assert_eq!(document["run_log"]["started_count"], 2);
    assert_eq!(document["run_log"]["apply_completed_count"], 1);
    assert_eq!(document["run_log"]["failed_count"], 1);
    assert_eq!(document["run_log"]["applied_bytes"], 1234);
    assert_eq!(document["run_log"]["last_event"], "failed");
    assert_eq!(document["run_log"]["recent_runs"][0]["run_id"], "run-1");
    assert_eq!(
        document["run_log"]["recent_runs"][0]["last_event"],
        "apply_completed"
    );
    assert_eq!(document["run_log"]["recent_runs"][0]["applied_bytes"], 1234);
    assert_eq!(document["run_log"]["recent_runs"][0]["failed"], false);
    assert_eq!(document["run_log"]["recent_runs"][1]["run_id"], "run-2");
    assert_eq!(
        document["run_log"]["recent_runs"][1]["last_event"],
        "failed"
    );
    assert_eq!(document["run_log"]["recent_runs"][1]["applied_bytes"], 0);
    assert_eq!(document["run_log"]["recent_runs"][1]["failed"], true);

    let terminal = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scheduler", "service", "status", "--config"])
        .arg(&config_path)
        .output()?;
    assert!(terminal.status.success());
    let stdout = String::from_utf8(terminal.stdout)?;
    assert!(stdout.contains("cargo-reclaim scheduler service: running"));
    assert!(stdout.contains(&format!(
        "state file: {}",
        state_path(temp.path()).display()
    )));
    assert!(stdout.contains(&format!("log dir: {}", temp.path().join("logs").display())));
    assert!(stdout.contains(&format!("run log: {}", run_log_path(temp.path()).display())));
    assert!(stdout.contains(&format!("pid: {pid}")));
    assert!(stdout.contains("last run: scheduler-status-test"));
    assert!(stdout.contains("run log records: 4"));
    assert!(stdout.contains("corrupt run log records: 1"));
    assert!(stdout.contains("apply completed cycles: 1"));
    assert!(stdout.contains("failed cycles: 1"));
    assert!(stdout.contains("applied bytes: 1234 (1.21 KiB)"));
    assert!(stdout.contains("last event: failed"));
    assert!(stdout.contains("recent runs:"));
    assert!(
        stdout.contains(
            "run-1: last_event=apply_completed applied_bytes=1234 (1.21 KiB) failed=false"
        )
    );
    assert!(stdout.contains("run-2: last_event=failed applied_bytes=0 (0 B) failed=true"));
    Ok(())
}

#[test]
fn service_status_json_reports_dead_running_pid_as_stale() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scheduler_service_stale_status")?;
    let config_path = write_config(
        temp.path(),
        "[scheduler]\nstate_dir = \"state\"\nlog_dir = \"logs\"\n",
    )?;
    fs::create_dir_all(temp.path().join("state"))?;
    fs::write(
        state_path(temp.path()),
        r#"{
  "schema_version": 1,
  "status": "running",
  "pid": 0,
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
    assert_eq!(document["status"], "stale");
    assert_eq!(document["pid"], Value::Null);
    assert_eq!(document["next_run_at"], Value::Null);
    assert_eq!(document["last_problem"], "service pid is not running");
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

fn state_path(root: &Path) -> PathBuf {
    root.join("state").join("service-state.json")
}

fn plans_dir(root: &Path) -> PathBuf {
    root.join("state").join("plans")
}

fn run_log_path(root: &Path) -> PathBuf {
    root.join("logs").join("runs.jsonl")
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
