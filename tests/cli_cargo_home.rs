use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn cargo_home_report_terminal_output_is_read_only() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_terminal")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    fs::write(temp.path().join("config.toml"), b"[net]\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "report", "--cargo-home"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cargo-home report"));
    assert!(stdout.contains("dry-run/read-only; no files were deleted or modified"));
    assert!(stdout.contains("source: explicit"));
    assert!(stdout.contains("registry_cache\tdirectory\t3\tregistry/cache"));
    assert!(stdout.contains("config\tfile\t6\tconfig.toml"));
    assert!(stdout.contains("cache.auto-clean-frequency"));
    assert!(
        temp.path()
            .join("registry/cache/example/pkg.crate")
            .is_file()
    );
    Ok(())
}

#[test]
fn cargo_home_report_json_uses_cargo_home_schema() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_json")?;
    fs::create_dir_all(temp.path().join("git/checkouts/example"))?;
    fs::write(temp.path().join("git/checkouts/example/file"), b"abcd")?;
    fs::write(temp.path().join(".crates2.json"), b"{}")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "report", "--json", "--cargo-home"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-home report");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["input"]["source"], "explicit");
    assert_eq!(document["totals"]["entry_count"], 2);
    assert_eq!(document["totals"]["known_cache_entry_count"], 1);
    assert!(document.get("policy").is_none());
    assert!(document.get("delete_candidate_count").is_none());

    let entries = document["entries"].as_array().expect("entries array");
    let checkout = entries
        .iter()
        .find(|entry| entry["class"] == "git_checkouts")
        .expect("git checkouts entry");
    assert_eq!(checkout["relative_path"], "git/checkouts");
    assert_eq!(checkout["size_bytes"], 4);
    assert_eq!(checkout["preserved"], true);
    Ok(())
}

#[test]
fn cargo_home_report_rejects_plan_options() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_reject")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "report", "--save-plan"])
        .arg(temp.path().join("plan.json"))
        .arg("--cargo-home")
        .arg(temp.path())
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("unknown option"));
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
