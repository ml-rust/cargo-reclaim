use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn list_reports_discovered_targets_sorted_by_size() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_list")?;
    let small = write_project(temp.path(), "small", b"abc")?;
    let large = write_project(temp.path(), "large", b"abcdef")?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["command"], "list");
    assert_eq!(document["totals"]["target_count"], 2);
    assert_eq!(
        document["targets"][0]["path"],
        large.join("target").display().to_string()
    );
    assert_eq!(document["targets"][0]["size_bytes"], 6);
    assert_eq!(
        document["targets"][1]["path"],
        small.join("target").display().to_string()
    );
    assert_eq!(document["targets"][1]["size_bytes"], 3);
    Ok(())
}

#[test]
fn list_reports_same_size_targets_sorted_by_path_with_exact_total() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_same_size")?;
    let alpha = write_project(temp.path(), "alpha", b"abc")?;
    let beta = write_project(temp.path(), "beta", b"abc")?;

    let document = run_json_command(["list", "--json"], temp.path())?;

    assert_eq!(document["command"], "list");
    assert_eq!(document["totals"]["target_count"], 2);
    assert_eq!(document["totals"]["total_size_bytes"], 6);
    assert_eq!(
        document["targets"][0]["path"],
        alpha.join("target").display().to_string()
    );
    assert_eq!(document["targets"][0]["size_bytes"], 3);
    assert_eq!(
        document["targets"][1]["path"],
        beta.join("target").display().to_string()
    );
    assert_eq!(document["targets"][1]["size_bytes"], 3);
    Ok(())
}

#[test]
fn list_excludes_non_cargo_cache_dirs() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_exclude_cache")?;
    let project = write_project(temp.path(), "project", b"abc")?;
    let cache = temp.path().join(".pytest_cache");
    fs::create_dir(&cache)?;
    fs::write(
        cache.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["totals"]["target_count"], 1);
    assert_eq!(
        document["targets"][0]["path"],
        project.join("target").display().to_string()
    );
    Ok(())
}

#[test]
fn list_clean_does_not_delete_targets() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_clean_guard")?;
    let project = write_project(temp.path(), "project", b"abcdef")?;
    let target = project.join("target");
    fs::create_dir(temp.path().join("clean"))?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .current_dir(temp.path())
        .args(["list", "clean", "--json"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(target.exists());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["command"], "list");
    assert_eq!(document["totals"]["target_count"], 1);
    Ok(())
}

#[test]
fn targets_command_is_not_public_surface() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_removed")?;
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("targets")
        .current_dir(temp.path())
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)?.contains("unknown command `targets`"));
    Ok(())
}

#[test]
fn list_reports_no_rust_project_found_for_root_without_projects() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_no_project")?;
    fs::create_dir(temp.path().join("some-data"))?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["totals"]["target_count"], 0);
    assert_eq!(document["totals"]["project_count"], 0);
    assert_eq!(
        document["note"],
        "no Rust project found under the scanned roots"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("list")
        .arg(temp.path())
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("targets: 0"));
    assert!(stdout.contains("note: no Rust project found under the scanned roots"));
    Ok(())
}

#[test]
fn list_reports_projects_present_but_no_cleanable_targets() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_project_no_target")?;
    let project = temp.path().join("proj");
    fs::create_dir_all(&project)?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"proj\"\n")?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["totals"]["target_count"], 0);
    assert_eq!(document["totals"]["project_count"], 1);
    assert_eq!(
        document["note"],
        "Rust projects found, but no cleanable target directories"
    );
    Ok(())
}

#[test]
fn list_reports_no_note_when_targets_are_found() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_note_absent")?;
    write_project(temp.path(), "project", b"abc")?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["totals"]["target_count"], 1);
    assert!(document["note"].is_null());
    Ok(())
}

fn write_project(
    root: &Path,
    name: &str,
    target_contents: &[u8],
) -> Result<PathBuf, Box<dyn Error>> {
    let project = root.join(name);
    fs::create_dir_all(project.join("target"))?;
    fs::write(
        project.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\n"),
    )?;
    fs::write(project.join("target/cache.bin"), target_contents)?;
    Ok(project)
}

fn run_json_command(args: [&str; 2], root: &Path) -> Result<Value, Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(args)
        .arg(root)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    Ok(serde_json::from_slice(&output.stdout)?)
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
