use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn targets_list_reports_discovered_targets_sorted_by_size() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_list")?;
    let small = write_project(temp.path(), "small", b"abc")?;
    let large = write_project(temp.path(), "large", b"abcdef")?;

    let document = run_json_command(["targets", "--json"], temp.path())?;
    assert_eq!(document["command"], "targets");
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
fn list_reports_discovered_targets_sorted_by_size() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_list_list")?;
    let small = write_project(temp.path(), "small", b"abc")?;
    let large = write_project(temp.path(), "large", b"abcdef")?;

    let document = run_json_command(["list", "--json"], temp.path())?;
    assert_eq!(document["command"], "targets");
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

    assert_eq!(document["command"], "targets");
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
fn targets_list_excludes_non_cargo_cache_dirs() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_exclude_cache")?;
    let project = write_project(temp.path(), "project", b"abc")?;
    let cache = temp.path().join(".pytest_cache");
    fs::create_dir(&cache)?;
    fs::write(
        cache.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;

    let document = run_json_command(["targets", "--json"], temp.path())?;
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
    assert_eq!(document["command"], "targets");
    assert_eq!(document["totals"]["target_count"], 1);
    Ok(())
}

#[test]
fn targets_clean_selected_target_validates_without_yes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_clean_validate")?;
    let project = write_project(temp.path(), "project", b"abcdef")?;
    let target = project.join("target");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["targets", "clean", "--target"])
        .arg(&target)
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim targets clean validation"));
    assert!(stdout.contains("would_delete"));
    assert!(target.is_dir());
    Ok(())
}

#[test]
fn targets_clean_selected_target_deletes_with_yes() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_clean_execute")?;
    let project = write_project(temp.path(), "project", b"abcdef")?;
    let target = project.join("target");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["targets", "clean", "--target"])
        .arg(&target)
        .args(["--yes"])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim targets clean execution"));
    assert!(stdout.contains("deleted"));
    assert!(!target.exists());
    assert!(project.join("Cargo.toml").is_file());
    Ok(())
}

#[test]
fn targets_clean_interactive_selects_by_number() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_targets_clean_interactive")?;
    let project = write_project(temp.path(), "project", b"abcdef")?;
    let target = project.join("target");

    let mut child = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["targets", "clean", "--interactive", "--yes"])
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.as_mut().expect("stdin").write_all(b"1\n")?;
    let output = child.wait_with_output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("Selection:"));
    assert!(String::from_utf8(output.stdout)?.contains("cargo-reclaim targets clean execution"));
    assert!(!target.exists());
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
