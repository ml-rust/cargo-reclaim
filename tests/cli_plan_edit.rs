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
fn edit_plan_selects_and_deselects_persisted_entry_indices() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_indices")?;
    let plan_path = write_two_entry_plan(&temp)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-index", "1", "--deselect-index=2", "--json"])
        .output()?;

    assert!(output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(stdout["selected_count"], 1);
    assert_eq!(stdout["deselected_count"], 1);

    let after: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(after["interactive_selection_modified"], true);
    assert_eq!(after["plan"]["entries"][0]["action"], "delete");
    assert_eq!(
        after["plan"]["entries"][0]["policy_reason"],
        "explicitly selected for deletion"
    );
    assert_eq!(after["plan"]["entries"][1]["action"], "preserve");
    assert_eq!(
        after["plan"]["entries"][1]["policy_reason"],
        "explicitly preserved by selection"
    );
    assert_eq!(after["plan"]["totals"]["delete_candidate_count"], 1);
    assert_eq!(after["plan"]["totals"]["preserved_count"], 1);
    Ok(())
}

#[test]
fn edit_plan_selects_and_deselects_persisted_artifact_classes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_classes")?;
    let plan_path = write_two_entry_plan(&temp)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args([
            "--select-class",
            "incremental",
            "--deselect-class=docs",
            "--json",
        ])
        .output()?;

    assert!(output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(stdout["selected_count"], 1);
    assert_eq!(stdout["deselected_count"], 1);

    let after: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let incremental = after["plan"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["artifact_class"] == "incremental")
        .expect("incremental entry");
    assert_eq!(incremental["action"], "delete");
    assert_eq!(
        incremental["policy_reason"],
        "explicitly selected for deletion"
    );
    let docs = after["plan"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["artifact_class"] == "docs")
        .expect("docs entry");
    assert_eq!(docs["action"], "preserve");
    assert_eq!(docs["policy_reason"], "explicitly preserved by selection");
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
fn edit_plan_rejects_invalid_artifact_classes_without_rewriting_plan() -> Result<(), Box<dyn Error>>
{
    for (name, args, message) in [
        (
            "unknown_label",
            vec!["--select-class", "increment"],
            "unknown artifact class selector",
        ),
        (
            "missing_value",
            vec!["--deselect-class"],
            "--deselect-class requires a value",
        ),
        (
            "unknown_class_selection",
            vec!["--select-class", "unknown"],
            "cannot be selected by class",
        ),
    ] {
        let temp = TestTemp::new(&format!("cli_edit_plan_invalid_class_{name}"))?;
        let plan_path = write_two_entry_plan(&temp)?;
        let before = fs::read(&plan_path)?;

        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(["edit-plan", "--plan"])
            .arg(&plan_path)
            .args(args)
            .output()?;

        assert_eq!(output.status.code(), Some(2));
        assert_eq!(fs::read(&plan_path)?, before);
        assert!(String::from_utf8(output.stderr)?.contains(message));
    }

    Ok(())
}

#[test]
fn edit_plan_rejects_unmatched_artifact_classes_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_unmatched_class")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-class", "final_wasm"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("no persisted plan entry matches artifact class `final_wasm`"));
    Ok(())
}

#[test]
fn edit_plan_rejects_invalid_entry_indices_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    for (name, args) in [
        ("out_of_range", vec!["--select-index", "3"]),
        ("zero", vec!["--select-index", "0"]),
        ("non_numeric", vec!["--select-index", "abc"]),
        ("missing", vec!["--select-index"]),
        ("deselect_zero", vec!["--deselect-index", "0"]),
    ] {
        let temp = TestTemp::new(&format!("cli_edit_plan_invalid_index_{name}"))?;
        let plan_path = write_two_entry_plan(&temp)?;
        let before = fs::read(&plan_path)?;

        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(["edit-plan", "--plan"])
            .arg(&plan_path)
            .args(args)
            .output()?;

        assert_eq!(output.status.code(), Some(2));
        assert_eq!(fs::read(&plan_path)?, before);
    }

    Ok(())
}

#[test]
fn edit_plan_rejects_cross_action_entry_conflicts_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_index_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;
    let before_json: Value = serde_json::from_slice(&before)?;
    let entry_path = before_json["plan"]["entries"][0]["snapshot"]["path"]
        .as_str()
        .expect("persisted path")
        .to_string();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select"])
        .arg(entry_path)
        .args(["--deselect-index", "1"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot be both selected and deselected"));
    Ok(())
}

#[test]
fn edit_plan_rejects_cross_action_class_and_index_conflicts_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_class_index_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-class", "incremental", "--deselect-index", "1"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot be both selected and deselected"));
    Ok(())
}

#[test]
fn edit_plan_rejects_cross_action_class_conflicts_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_class_class_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-class", "docs", "--deselect-class", "docs"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot be both selected and deselected"));
    Ok(())
}

#[test]
fn edit_plan_rejects_cross_action_class_and_path_conflicts_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_class_path_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;
    let before_json: Value = serde_json::from_slice(&before)?;
    let entry_path = before_json["plan"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["artifact_class"] == "docs")
        .expect("docs entry")["snapshot"]["path"]
        .as_str()
        .expect("persisted path")
        .to_string();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-class", "docs", "--deselect"])
        .arg(entry_path)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot be both selected and deselected"));
    Ok(())
}

#[test]
fn edit_plan_rejects_cross_action_index_conflicts_without_rewriting_plan()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_index_index_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--select-index", "1", "--deselect-index", "1"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("cannot be both selected and deselected"));
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

#[test]
fn edit_plan_list_terminal_numbers_entries_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_list_terminal")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before_bytes = fs::read(&plan_path)?;
    let before_json: Value = serde_json::from_slice(&before_bytes)?;
    let first = &before_json["plan"]["entries"][0];
    let first_row = format!(
        "1\t{}\t{}\t{}\t",
        first["action"].as_str().expect("action"),
        first["artifact_class"].as_str().expect("artifact class"),
        first["snapshot"]["size_bytes"]
            .as_u64()
            .expect("size bytes")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .arg("--list")
        .output()?;

    assert!(output.status.success());
    assert_eq!(fs::read(&plan_path)?, before_bytes);
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim edit-plan list"));
    assert!(stdout.contains("read-only; no plan file was modified"));
    assert!(stdout.contains(&format!(
        "plan id: {}",
        before_json["id"].as_str().expect("plan id")
    )));
    assert!(stdout.contains("entries: 2"));
    assert!(stdout.contains(&first_row));
    Ok(())
}

#[test]
fn edit_plan_list_json_preserves_order_and_fields() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_list_json")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before_bytes = fs::read(&plan_path)?;
    let before_json: Value = serde_json::from_slice(&before_bytes)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--list", "--json"])
        .output()?;

    assert!(output.status.success());
    assert_eq!(fs::read(&plan_path)?, before_bytes);
    let stdout: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(stdout["command"], "edit-plan list");
    assert_eq!(stdout["plan_path"], plan_path.display().to_string());
    assert_eq!(stdout["plan_id"], before_json["id"]);
    assert_eq!(stdout["totals"], before_json["plan"]["totals"]);

    let listed_entries = stdout["entries"].as_array().expect("listed entries");
    let persisted_entries = before_json["plan"]["entries"]
        .as_array()
        .expect("persisted entries");
    assert_eq!(listed_entries.len(), persisted_entries.len());
    for (index, (listed, persisted)) in listed_entries.iter().zip(persisted_entries).enumerate() {
        assert_eq!(listed["index"], index + 1);
        assert_eq!(listed["path"], persisted["snapshot"]["path"]);
        assert_eq!(listed["action"], persisted["action"]);
        assert_eq!(listed["artifact_class"], persisted["artifact_class"]);
        assert_eq!(listed["size_bytes"], persisted["snapshot"]["size_bytes"]);
        assert_eq!(listed["path_kind"], persisted["snapshot"]["path_kind"]);
        assert_eq!(
            listed["requires_confirmation"],
            persisted["requires_confirmation"]
        );
        assert_eq!(listed["policy_reason"], persisted["policy_reason"]);
    }
    Ok(())
}

#[test]
fn edit_plan_list_rejects_edit_flags_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_list_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;
    let before_json: Value = serde_json::from_slice(&before)?;
    let entry_path = before_json["plan"]["entries"][0]["snapshot"]["path"]
        .as_str()
        .expect("persisted path")
        .to_string();

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--list", "--select"])
        .arg(entry_path)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("`--list` cannot be combined with edit flags"));
    Ok(())
}

#[test]
fn edit_plan_list_rejects_index_edit_flags_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_list_index_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--list", "--select-index", "1"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("`--list` cannot be combined with edit flags"));
    Ok(())
}

#[test]
fn edit_plan_list_rejects_class_edit_flags_without_rewriting_plan() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_edit_plan_list_class_conflict")?;
    let plan_path = write_two_entry_plan(&temp)?;
    let before = fs::read(&plan_path)?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["edit-plan", "--plan"])
        .arg(&plan_path)
        .args(["--list", "--select-class", "incremental"])
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&plan_path)?, before);
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("`--list` cannot be combined with edit flags"));
    Ok(())
}

fn write_two_entry_plan(temp: &TestTemp) -> Result<PathBuf, Box<dyn Error>> {
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;
    let plan_path = temp.path().join("saved-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["plan", "--save-plan"])
        .arg(&plan_path)
        .arg(temp.path())
        .output()?;
    assert!(output.status.success());
    Ok(plan_path)
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
