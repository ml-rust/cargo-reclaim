use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn plan_command_prints_dry_run_summary_and_entries() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_plan_summary")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim plan dry-run"));
    assert!(stdout.contains("dry-run only; no files were deleted or modified"));
    assert!(stdout.contains("policy: balanced"));
    assert!(stdout.contains("entries: 2"));
    assert!(stdout.contains("delete\tincremental\t3\t"));
    assert!(stdout.contains("preserve\tdocs\t4\t"));
    assert!(stdout.contains("target/debug/incremental"));
    assert!(stdout.contains("target/doc"));
    assert!(String::from_utf8(output.stderr)?.is_empty());
    Ok(())
}

#[test]
fn scan_command_supports_observe_policy() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scan_observe")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scan", "--policy", "observe"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim scan dry-run"));
    assert!(stdout.contains("policy: observe"));
    assert!(stdout.contains("delete candidates: 0"));
    assert!(stdout.contains("preserve\tincremental\t3\t"));
    Ok(())
}

#[test]
fn plan_keep_recent_writes_reports_skip_active() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_keep_recent")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--keep-recent-writes", "1d"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("delete candidates: 0"));
    assert!(stdout.contains("skip_active\tincremental\t3\t"));
    assert!(stdout.contains("keep window"));
    Ok(())
}

#[test]
fn apply_flag_is_rejected_without_deleting_anything() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_reject_apply")?;
    write_manifest(temp.path())?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--apply")
        .arg(temp.path())
        .output()?;

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("dry-run plan"));
    Ok(())
}

#[test]
fn usage_errors_return_usage_exit_code() -> Result<(), Box<dyn Error>> {
    for args in [vec!["apply"], vec!["plan", "--unknown"], vec!["wat"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(args)
            .output()?;

        assert_eq!(output.status.code(), Some(2));
    }

    Ok(())
}

#[test]
fn plan_json_outputs_single_dry_run_document() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_plan_json")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(!stdout.contains("dry-run only"));
    let document: Value = serde_json::from_str(&stdout)?;

    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "plan");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["policy"], "balanced");
    assert_eq!(document["totals"]["entry_count"], 2);
    assert_eq!(document["totals"]["delete_candidate_count"], 1);
    assert_eq!(document["totals"]["preserved_count"], 1);

    let entries = document["entries"].as_array().expect("entries array");
    let incremental = entries
        .iter()
        .find(|entry| entry["artifact_class"] == "incremental")
        .expect("incremental entry");
    assert_eq!(incremental["action"], "delete");
    assert_eq!(incremental["snapshot"]["size_bytes"], 3);
    assert_eq!(incremental["snapshot"]["path_kind"], "directory");
    assert_eq!(incremental["evidence"]["kind"], "project_context");
    assert_eq!(
        incremental["evidence"]["project_manifest"],
        temp.path().join("Cargo.toml").display().to_string()
    );

    let docs = entries
        .iter()
        .find(|entry| entry["artifact_class"] == "docs")
        .expect("docs entry");
    assert_eq!(docs["action"], "preserve");
    Ok(())
}

#[test]
fn plan_json_whole_target_confirm_emits_single_confirmation_entry() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_whole_target_confirm")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--whole-target=confirm"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let entries = document["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["artifact_class"], "whole_target");
    assert_eq!(entries[0]["action"], "requires_confirmation");
    assert_eq!(entries[0]["requires_confirmation"], true);
    assert_eq!(
        entries[0]["snapshot"]["path"],
        temp.path().join("target").display().to_string()
    );
    Ok(())
}

#[test]
fn plan_json_whole_target_delete_emits_single_delete_entry() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_whole_target_delete")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "plan",
            "--json",
            "--policy",
            "aggressive",
            "--whole-target",
            "delete",
        ])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let entries = document["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["artifact_class"], "whole_target");
    assert_eq!(entries[0]["action"], "delete");
    assert_eq!(entries[0]["requires_confirmation"], false);
    Ok(())
}

#[test]
fn whole_target_delete_requires_aggressive_policy() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_whole_target_delete_balanced")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--whole-target", "delete"])
        .arg(temp.path())
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("aggressive"));
    Ok(())
}

#[test]
fn config_whole_target_delete_requires_unattended_allow_flag() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_config_whole_target_gate")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let config_path = temp.path().join("reclaim.toml");
    fs::write(
        &config_path,
        r#"
version = 1
roots = ["."]

[policy]
mode = "aggressive"
whole_target = "delete"
"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--config"])
        .arg(&config_path)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("allow_unattended_whole_target_delete"));
    Ok(())
}

#[test]
fn config_whole_target_delete_with_allow_flag_uses_aggressive_policy() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_config_whole_target_delete")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let config_path = temp.path().join("reclaim.toml");
    fs::write(
        &config_path,
        r#"
version = 1
roots = ["."]

[policy]
mode = "aggressive"
whole_target = "delete"
allow_unattended_whole_target_delete = true
"#,
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--config"])
        .arg(&config_path)
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let entries = document["entries"].as_array().expect("entries array");
    assert_eq!(document["policy"], "aggressive");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["artifact_class"], "whole_target");
    assert_eq!(entries[0]["action"], "delete");
    Ok(())
}

#[test]
fn plan_json_reports_recent_write_skip_active() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_plan_json_recent")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--keep-recent-writes=1d"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["totals"]["delete_candidate_count"], 0);
    assert_eq!(document["totals"]["preserved_count"], 1);
    assert_eq!(document["entries"][0]["action"], "skip_active");
    Ok(())
}

#[test]
fn scan_json_preserves_command_and_observe_policy() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_scan_json")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["scan", "--json", "--policy", "observe"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "scan");
    assert_eq!(document["policy"], "observe");
    assert_eq!(document["entries"][0]["action"], "preserve");
    assert_eq!(document["totals"]["delete_candidate_count"], 0);
    Ok(())
}

#[test]
fn json_reports_weak_name_only_confirmation_shape() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_json_weak")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--allow-name-only-targets"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let entry = &document["entries"][0];
    assert_eq!(entry["action"], "requires_confirmation");
    assert_eq!(entry["requires_confirmation"], true);
    assert_eq!(entry["evidence"]["kind"], "weak_name_only");
    assert_eq!(entry["evidence"]["matched_name"], "target");
    Ok(())
}

#[test]
fn plan_save_plan_writes_persisted_document() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_save_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg("--expires-in")
        .arg("30m")
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(
        temp.path()
            .join("target/debug/incremental/cache.bin")
            .is_file()
    );
    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(persisted["schema_version"], 1);
    assert!(
        persisted["id"]
            .as_str()
            .expect("plan id string")
            .starts_with("sha256:")
    );
    assert_eq!(persisted["invocation"]["command"], "plan");
    assert_eq!(persisted["invocation"]["policy"], "balanced");
    assert_eq!(persisted["interactive_selection_modified"], false);
    assert_eq!(
        persisted["expires_at"]["unix_seconds"]
            .as_u64()
            .expect("expires_at unix_seconds")
            - persisted["created_at"]["unix_seconds"]
                .as_u64()
                .expect("created_at unix_seconds"),
        30 * 60
    );
    assert_eq!(
        persisted["plan"]["entries"][0]["artifact_class"],
        "incremental"
    );
    Ok(())
}

#[test]
fn save_plan_records_recent_write_keep_window() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_save_recent_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .args(["--keep-recent-writes", "30m"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(
        persisted["invocation"]["planner_options"]["recent_write_keep_window_seconds"],
        30 * 60
    );
    assert_eq!(persisted["plan"]["entries"][0]["action"], "skip_active");
    Ok(())
}

#[test]
fn save_plan_records_config_provenance() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_save_config_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let config_path = temp.path().join("reclaim.toml");
    fs::write(
        &config_path,
        r#"
version = 1
roots = ["."]

[policy]
mode = "observe"
"#,
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--config")
        .arg(&config_path)
        .arg("--save-plan")
        .arg(&plan_path)
        .output()?;

    assert!(output.status.success());
    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(persisted["invocation"]["policy"], "observe");
    assert_eq!(
        persisted["invocation"]["config_path"],
        config_path.display().to_string()
    );
    assert_eq!(persisted["invocation"]["config_version"], 1);
    Ok(())
}

#[test]
fn json_output_can_be_combined_with_save_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_json_save_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--json", "--save-plan"])
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout)?;
    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(stdout["command"], "plan");
    assert_eq!(persisted["invocation"]["command"], "plan");
    assert_eq!(stdout["entries"][0]["artifact_class"], "incremental");
    assert_eq!(
        persisted["plan"]["entries"][0]["artifact_class"],
        "incremental"
    );
    Ok(())
}

#[test]
fn save_plan_flags_are_restricted_to_explicit_plan_saves() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_save_plan_reject")?;
    let plan_path = temp.path().join("saved-plan.json");

    for args in [
        vec!["scan", "--save-plan"],
        vec!["plan", "--expires-in", "30m"],
        vec![
            "plan",
            "--save-plan",
            plan_path.to_str().expect("plan path utf-8"),
            "--expires-in",
            "0s",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(args)
            .arg(temp.path())
            .output()?;
        assert_eq!(output.status.code(), Some(2));
    }

    let missing_path = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--save-plan"])
        .output()?;
    assert_eq!(missing_path.status.code(), Some(2));

    Ok(())
}

#[test]
fn apply_validates_explicit_plan_without_deleting_files() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_validate")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .output()?;

    assert!(apply_output.status.success());
    assert!(artifact.is_file());
    let stdout = String::from_utf8(apply_output.stdout)?;
    assert!(stdout.contains("cargo-reclaim apply validation"));
    assert!(stdout.contains("validation only; no files were deleted or modified"));
    assert!(stdout.contains("would delete: 1"));
    assert!(stdout.contains("would delete bytes: 3"));
    Ok(())
}

#[test]
fn apply_json_reports_validation_without_deleting_files() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_json")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .arg("--json")
        .output()?;

    assert!(apply_output.status.success());
    assert!(artifact.is_file());
    let document: Value = serde_json::from_slice(&apply_output.stdout)?;
    assert_eq!(document["command"], "apply");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["totals"]["would_delete_count"], 1);
    assert_eq!(document["totals"]["would_delete_bytes"], 3);
    assert_eq!(document["entries"][0]["status"], "would_delete");
    Ok(())
}

#[test]
fn apply_yes_executes_revalidated_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_yes")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .arg("--yes")
        .output()?;

    assert!(apply_output.status.success());
    assert!(!artifact.exists());
    let stdout = String::from_utf8(apply_output.stdout)?;
    assert!(stdout.contains("cargo-reclaim apply execution"));
    assert!(stdout.contains("deleted: 1"));
    assert!(stdout.contains("delete failures: 0"));
    assert!(stdout.contains("deleted\tdelete\t"));
    Ok(())
}

#[test]
fn apply_yes_json_reports_execution() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_yes_json")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(apply_output.status.success());
    assert!(!artifact.exists());
    let document: Value = serde_json::from_slice(&apply_output.stdout)?;
    assert_eq!(document["command"], "apply");
    assert_eq!(document["dry_run"], false);
    assert_eq!(document["totals"]["applied_count"], 1);
    assert_eq!(document["totals"]["applied_bytes"], 3);
    assert_eq!(document["totals"]["failed_count"], 0);
    assert_eq!(document["entries"][0]["status"], "deleted");
    Ok(())
}

#[test]
fn apply_reports_stale_skip_after_target_changes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_stale")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());
    fs::write(&artifact, b"changed")?;

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .output()?;

    assert!(apply_output.status.success());
    let stdout = String::from_utf8(apply_output.stdout)?;
    assert!(stdout.contains("stale skips: 1"));
    assert!(stdout.contains("skip_stale_plan"));
    Ok(())
}

#[test]
fn apply_yes_reports_stale_skip_without_deleting_changed_path() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_apply_yes_stale")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());
    fs::write(&artifact, b"changed")?;

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .arg("--yes")
        .output()?;

    assert!(apply_output.status.success());
    assert!(artifact.is_file());
    let stdout = String::from_utf8(apply_output.stdout)?;
    assert!(stdout.contains("stale skips: 1"));
    assert!(stdout.contains("skip_stale_plan"));
    assert!(stdout.contains("deleted: 0"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn apply_yes_exits_nonzero_when_delete_fails() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp = TestTemp::new("cli_apply_yes_failed")?;
    write_manifest(temp.path())?;
    let artifact_dir = temp.path().join("target/debug/incremental");
    fs::create_dir_all(artifact_dir.join("session"))?;
    fs::write(artifact_dir.join("session/cache.bin"), b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--save-plan")
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    fs::set_permissions(&artifact_dir, fs::Permissions::from_mode(0o555))?;
    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .arg("--yes")
        .output()?;
    fs::set_permissions(&artifact_dir, fs::Permissions::from_mode(0o755))?;

    assert_eq!(apply_output.status.code(), Some(1));
    assert!(artifact_dir.is_dir());
    let stdout = String::from_utf8(apply_output.stdout)?;
    assert!(stdout.contains("delete failures: 1"));
    assert!(stdout.contains("delete_failed"));
    Ok(())
}

#[test]
fn apply_requires_explicit_plan_path_and_rejects_last_alias() -> Result<(), Box<dyn Error>> {
    for args in [
        vec!["apply"],
        vec!["apply", "--json"],
        vec!["apply", "last"],
        vec!["apply", "--plan", "last"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(args)
            .output()?;
        assert_eq!(output.status.code(), Some(2));
    }

    Ok(())
}

#[test]
fn ignore_option_suppresses_target_entries_end_to_end() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_ignore")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("plan")
        .arg("--ignore")
        .arg(temp.path().join("target"))
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("entries: 0"));
    assert!(!stdout.contains("target/debug/incremental"));
    Ok(())
}

#[test]
fn allow_name_only_targets_surfaces_confirmation_entries_end_to_end() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_allow_name_only")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--allow-name-only-targets"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("requires_confirmation\tincremental\t3\t"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn follow_symlinks_option_includes_symlinked_project_end_to_end() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("cli_follow_symlink")?;
    let real = temp.path().join("real");
    fs::create_dir(&real)?;
    write_manifest(&real)?;
    fs::create_dir_all(real.join("target/debug/incremental"))?;
    fs::write(real.join("target/debug/incremental/cache.bin"), b"abc")?;
    let linked = temp.path().join("linked");
    symlink(&real, &linked)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--follow-symlinks"])
        .arg(&linked)
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("target/debug/incremental"));
    Ok(())
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
