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
