use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn edit_plan_selects_exact_persisted_entry_path() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_select")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--allow-name-only-targets", "--save-plan"])
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let before: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let entry_path = before["plan"]["entries"][0]["snapshot"]["path"]
        .as_str()
        .expect("persisted path")
        .to_string();
    let original_id = before["id"].as_str().expect("plan id").to_string();
    assert_eq!(
        before["plan"]["entries"][0]["action"],
        "requires_confirmation"
    );

    let edit_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .arg("--select")
        .arg(&entry_path)
        .arg("--json")
        .output()?;

    assert!(edit_output.status.success());
    assert!(String::from_utf8(edit_output.stderr)?.is_empty());
    let stdout: Value = serde_json::from_slice(&edit_output.stdout)?;
    assert_eq!(stdout["command"], "edit-plan");
    assert_eq!(stdout["selected_count"], 1);
    assert_eq!(stdout["deselected_count"], 0);
    assert_eq!(stdout["totals"]["delete_candidate_count"], 1);

    let after: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_ne!(after["id"].as_str().expect("plan id"), original_id);
    assert_eq!(after["id"], stdout["plan_id"]);
    assert_eq!(after["interactive_selection_modified"], true);
    assert_eq!(after["plan"]["totals"]["delete_candidate_count"], 1);
    assert_eq!(after["plan"]["totals"]["preserved_count"], 0);
    let entry = &after["plan"]["entries"][0];
    assert_eq!(entry["snapshot"]["path"], entry_path);
    assert_eq!(entry["action"], "delete");
    assert_eq!(entry["requires_confirmation"], false);
    assert_eq!(entry["policy_reason"], "explicitly selected for deletion");
    Ok(())
}

#[test]
fn edit_plan_deselects_without_deleting_on_apply_validation() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_deselect")?;
    write_manifest(temp.path())?;
    let artifact = temp.path().join("target/debug/incremental/cache.bin");
    fs::create_dir_all(artifact.parent().expect("artifact parent"))?;
    fs::write(&artifact, b"abc")?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--save-plan"])
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let before: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let entry_path = before["plan"]["entries"][0]["snapshot"]["path"]
        .as_str()
        .expect("persisted path")
        .to_string();
    assert_eq!(before["plan"]["entries"][0]["action"], "delete");

    let edit_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .arg("--deselect")
        .arg(&entry_path)
        .output()?;
    assert!(edit_output.status.success());
    let stdout = String::from_utf8(edit_output.stdout)?;
    assert!(stdout.contains("cargo-reclaim edit-plan"));
    assert!(stdout.contains("deselected: 1"));

    let after: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(after["plan"]["entries"][0]["action"], "preserve");
    assert_eq!(after["plan"]["entries"][0]["requires_confirmation"], false);
    assert_eq!(
        after["plan"]["entries"][0]["policy_reason"],
        "explicitly preserved by selection"
    );
    assert_eq!(after["plan"]["totals"]["delete_candidate_count"], 0);
    assert_eq!(after["plan"]["totals"]["preserved_count"], 1);

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["apply", "--plan"])
        .arg(&plan_path)
        .output()?;
    assert!(apply_output.status.success());
    assert!(artifact.is_file());
    let apply_stdout = String::from_utf8(apply_output.stdout)?;
    assert!(apply_stdout.contains("would delete: 0"));
    Ok(())
}

#[test]
fn edit_plan_rejects_missing_plan_last_yes_and_no_edits() -> Result<(), Box<dyn Error>> {
    for args in [
        vec!["edit-plan"],
        vec!["edit-plan", "--plan", "last", "--select", "target"],
        vec!["edit-plan", "last", "--select", "target"],
        vec![
            "edit-plan",
            "--plan",
            "plan.json",
            "--yes",
            "--select",
            "target",
        ],
        vec!["edit-plan", "--plan", "plan.json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(args)
            .output()?;
        assert_eq!(output.status.code(), Some(2));
    }

    Ok(())
}

#[test]
fn edit_plan_rejects_unmatched_entry_path_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_unmatched")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    let plan_path = temp.path().join("saved-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--save-plan"])
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());
    let before = fs::read(&plan_path)?;

    let edit_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select", "target"])
        .output()?;

    assert_eq!(edit_output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(edit_output.stderr)?;
    assert!(stderr.contains("no persisted plan entry matches"));
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
