use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn help_lists_cargo_home_plan_and_apply_commands() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-home report"));
    assert!(stdout.contains("cargo-home plan"));
    assert!(stdout.contains("cargo-home apply --plan <path> [--yes]"));
    Ok(())
}

#[test]
fn cargo_home_plan_help_exits_success_and_writes_stdout() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "plan", "--help"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("usage: cargo-reclaim cargo-home plan"));
    Ok(())
}

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

#[test]
fn cargo_home_plan_terminal_output_is_dry_run() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_plan_terminal")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    fs::write(temp.path().join("config.toml"), b"[net]\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "cargo-home",
            "plan",
            "--policy",
            "conservative",
            "--cargo-home",
        ])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("cargo-reclaim cargo-home plan"));
    assert!(stdout.contains("dry-run planning only; no files were deleted or modified"));
    assert!(stdout.contains("policy: conservative"));
    assert!(stdout.contains("delete_candidate\tregistry_cache\tdirectory\t3\tregistry/cache"));
    assert!(stdout.contains("preserve\tconfig\tfile\t6\tconfig.toml"));
    assert!(stdout.contains("cache.auto-clean-frequency"));
    assert!(
        temp.path()
            .join("registry/cache/example/pkg.crate")
            .is_file()
    );
    Ok(())
}

#[test]
fn cargo_home_plan_json_uses_plan_schema_without_target_fields() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_plan_json")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::create_dir_all(temp.path().join("git/checkouts/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    fs::write(temp.path().join("git/checkouts/example/file"), b"abcd")?;
    fs::write(temp.path().join(".crates2.json"), b"{}")?;

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "cargo-home",
            "plan",
            "--json",
            "--policy=aggressive",
            "--cargo-home",
        ])
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-home plan");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["policy"], "aggressive");
    assert_eq!(document["totals"]["entry_count"], 3);
    assert_eq!(document["totals"]["delete_candidate_count"], 2);
    assert_eq!(document["totals"]["delete_candidate_bytes"], 7);
    assert_eq!(document["totals"]["preserved_count"], 1);
    assert!(document["totals"].get("cache_bytes").is_none());
    assert!(document.get("artifact_class").is_none());
    assert!(document.get("target_evidence").is_none());

    let entries = document["entries"].as_array().expect("entries array");
    assert!(
        entries
            .iter()
            .all(|entry| entry.get("artifact_class").is_none())
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.get("target_evidence").is_none())
    );
    let cache = entries
        .iter()
        .find(|entry| entry["class"] == "registry_cache")
        .expect("registry cache entry");
    assert_eq!(cache["action"], "delete_candidate");
    assert!(
        cache["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
    Ok(())
}

#[test]
fn cargo_home_plan_save_plan_writes_persisted_document() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_plan_save")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    let plan_path = temp.path().join("cargo-home-plan.json");

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "plan", "--save-plan"])
        .arg(&plan_path)
        .args(["--expires-in", "1h"])
        .arg("--cargo-home")
        .arg(temp.path())
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let persisted: Value = serde_json::from_slice(&fs::read(plan_path)?)?;
    assert_eq!(persisted["schema_version"], 1);
    assert_eq!(persisted["command"], "cargo-home plan");
    assert_eq!(persisted["plan"]["input"]["source"], "explicit");
    assert_eq!(persisted["plan"]["totals"]["delete_candidate_count"], 1);
    assert!(persisted.get("invocation").is_none());
    Ok(())
}

#[test]
fn cargo_home_apply_plan_validates_without_deleting() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_apply")?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let plan_path = temp.path().join("cargo-home-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "cargo-home",
            "plan",
            "--policy",
            "conservative",
            "--save-plan",
        ])
        .arg(&plan_path)
        .arg("--cargo-home")
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "apply", "--json", "--plan"])
        .arg(&plan_path)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-home apply");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["validation_only"], true);
    assert_eq!(document["totals"]["would_delete_count"], 1);
    assert_eq!(document["totals"]["applied_count"], 0);
    assert_eq!(document["totals"]["applied_bytes"], 0);
    assert_eq!(document["totals"]["failed_count"], 0);
    assert_eq!(document["entries"][0]["status"], "would_delete");
    assert!(cache_file.is_file());
    Ok(())
}

#[test]
fn cargo_home_saved_relative_root_applies_from_different_cwd() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_relative_root")?;
    let cargo_home = temp.path().join("relative-cargo-home");
    let cache_file = cargo_home.join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let other_cwd = temp.path().join("other");
    fs::create_dir(&other_cwd)?;
    let plan_path = temp.path().join("cargo-home-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .current_dir(temp.path())
        .args([
            "cargo-home",
            "plan",
            "--policy",
            "conservative",
            "--save-plan",
        ])
        .arg(&plan_path)
        .args(["--cargo-home", "relative-cargo-home"])
        .output()?;
    assert!(plan_output.status.success());

    let persisted: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    assert_eq!(
        persisted["plan"]["input"]["root"],
        cargo_home.canonicalize()?.display().to_string()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .current_dir(other_cwd)
        .args(["cargo-home", "apply", "--json", "--plan"])
        .arg(&plan_path)
        .output()?;

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["totals"]["would_delete_count"], 1);
    assert!(cache_file.is_file());
    Ok(())
}

#[test]
fn cargo_home_apply_yes_json_deletes_revalidated_entries() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_apply_yes_json")?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;
    let plan_path = temp.path().join("cargo-home-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "cargo-home",
            "plan",
            "--policy",
            "conservative",
            "--save-plan",
        ])
        .arg(&plan_path)
        .arg("--cargo-home")
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "apply", "--json", "--yes", "--plan"])
        .arg(&plan_path)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-home apply");
    assert_eq!(document["dry_run"], false);
    assert_eq!(document["validation_only"], false);
    assert_eq!(document["totals"]["delete_candidate_count"], 1);
    assert_eq!(document["totals"]["would_delete_count"], 0);
    assert_eq!(document["totals"]["applied_count"], 1);
    assert_eq!(document["totals"]["applied_bytes"], 3);
    assert_eq!(document["totals"]["failed_count"], 0);
    assert_eq!(document["entries"][0]["status"], "deleted");
    assert!(!temp.path().join("registry/cache").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn cargo_home_apply_yes_exits_nonzero_when_delete_fails() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let temp = TestTemp::new("cli_cargo_home_apply_yes_failed")?;
    let git = temp.path().join("git");
    let git_db_file = git.join("db");
    fs::create_dir_all(&git)?;
    fs::write(&git_db_file, b"abc")?;
    let plan_path = temp.path().join("cargo-home-plan.json");

    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args([
            "cargo-home",
            "plan",
            "--policy",
            "aggressive",
            "--save-plan",
        ])
        .arg(&plan_path)
        .arg("--cargo-home")
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());

    let original_permissions = fs::metadata(&git)?.permissions();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o500))?;
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "apply", "--json", "--yes", "--plan"])
        .arg(&plan_path)
        .output()?;
    if git.exists() {
        fs::set_permissions(&git, original_permissions)?;
    }

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["totals"]["applied_count"], 0);
    assert_eq!(document["totals"]["failed_count"], 1);
    assert_eq!(document["entries"][0]["status"], "delete_failed");
    assert!(git_db_file.is_file());
    Ok(())
}

#[test]
fn cargo_home_apply_requires_explicit_plan_path_and_rejects_last_alias()
-> Result<(), Box<dyn Error>> {
    for args in [
        vec!["cargo-home", "apply"],
        vec!["cargo-home", "apply", "last"],
        vec!["cargo-home", "apply", "--plan", "last"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(args)
            .output()?;

        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8(output.stderr)?.contains("explicit"),
            "stderr should mention explicit plan path"
        );
    }
    Ok(())
}

#[test]
fn cargo_home_plan_rejects_apply_flags() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_home_plan_apply_reject")?;

    for flag in ["--apply", "--yes"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
            .args(["cargo-home", "plan", flag, "--cargo-home"])
            .arg(temp.path())
            .output()?;

        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert!(String::from_utf8(output.stderr)?.contains("dry-run only"));
    }
    Ok(())
}

#[test]
fn cargo_home_apply_rejects_dry_run_report_with_actionable_guidance() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_cargo_home_apply_report_rejected")?;
    let cache_file = temp.path().join("registry/cache/example/pkg.crate");
    fs::create_dir_all(cache_file.parent().expect("cache parent"))?;
    fs::write(&cache_file, b"abc")?;

    // `cargo-home plan --json` emits a dry-run report, not an executable plan.
    let plan_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "plan", "--json", "--cargo-home"])
        .arg(temp.path())
        .output()?;
    assert!(plan_output.status.success());
    let report_path = temp.path().join("cargo-home-report.json");
    fs::write(&report_path, &plan_output.stdout)?;

    let apply_output = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"))
        .args(["cargo-home", "apply", "--plan"])
        .arg(&report_path)
        .arg("--yes")
        .output()?;

    assert!(!apply_output.status.success());
    assert!(
        cache_file.is_file(),
        "report must not be executed as a plan"
    );
    let stderr = String::from_utf8(apply_output.stderr)?;
    assert!(
        stderr.contains("dry-run report"),
        "expected report-format diagnosis, got: {stderr}"
    );
    assert!(
        stderr.contains("--save-plan"),
        "expected guidance toward --save-plan, got: {stderr}"
    );
    assert!(
        !stderr.contains("encode"),
        "load failure must not be reported as an encode error: {stderr}"
    );
    assert!(
        !stderr.contains("missing field"),
        "raw serde error must not leak to the user: {stderr}"
    );
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
