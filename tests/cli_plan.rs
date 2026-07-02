use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    for args in [
        vec!["apply"],
        vec!["plan", "--json"],
        vec!["plan", "--unknown"],
        vec!["wat"],
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
