use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn cargo_config_recommend_json_reports_configured_output_dirs() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_json_dirs")?;
    let project = temp.path().join("project");
    write_project_config(
        &project,
        "[build]\ntarget-dir = \"target-out\"\nbuild-dir = \"build-out\"\n",
    )?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "recommend", "--json", "--project"])
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-config recommend");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(
        document["target_dirs"][0]["path"],
        path_string(project.join("target-out"))
    );
    assert_eq!(
        document["build_dirs"][0]["path"],
        path_string(project.join("build-out"))
    );
    assert!(
        document["target_dirs"][0]["source"]
            .as_str()
            .is_some_and(|source| source.contains("build.target-dir"))
    );
    assert!(
        document["build_dirs"][0]["source"]
            .as_str()
            .is_some_and(|source| source.contains("build.build-dir"))
    );
    assert_eq!(
        document["recommendations"].as_array().map(Vec::len),
        Some(0)
    );
    Ok(())
}

#[test]
fn cargo_config_recommend_json_recommends_build_dir_when_missing() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_missing_build_dir")?;
    let project = temp.path().join("project");
    write_project_config(&project, "[build]\ntarget-dir = \"target-out\"\n")?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "recommend", "--project"])
        .arg(&project)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(document["build_dirs"].as_array().map(Vec::len), Some(0));
    let recommendations = document["recommendations"]
        .as_array()
        .ok_or("recommendations array")?;
    assert!(recommendations.iter().any(|recommendation| {
        recommendation["key"] == "build.build-dir"
            && recommendation["recommended"] == "target/build"
            && recommendation["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("separate"))
    }));
    Ok(())
}

#[test]
fn cargo_config_recommend_rejects_apply_flags() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_reject")?;
    let project = temp.path().join("project");
    write_project_config(&project, "[build]\ntarget-dir = \"target-out\"\n")?;
    let config_path = project.join(".cargo/config.toml");
    let original_config = fs::read_to_string(&config_path)?;

    for flag in ["--apply", "--yes"] {
        let output = command_with_isolated_cargo_home(temp.path())
            .args(["cargo-config", "recommend", flag, "--project"])
            .arg(&project)
            .output()?;

        assert_eq!(output.status.code(), Some(2), "{flag}");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(
            stderr.contains("read-only/dry-run only"),
            "{flag}: {stderr}"
        );
        assert!(stderr.contains("no Cargo config files can be modified"));
        assert_eq!(fs::read_to_string(&config_path)?, original_config);
    }
    Ok(())
}

#[test]
fn cargo_config_preview_json_creates_no_config_file_when_absent() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_preview_absent")?;
    let project = temp.path().join("project");
    write_project_manifest(&project)?;
    let config_path = project.join(".cargo/config.toml");

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "preview", "--json", "--project"])
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    assert!(!config_path.exists());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-config preview");
    assert_eq!(document["dry_run"], true);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(document["target_config_file"], path_string(&config_path));
    assert_eq!(document["target_config_snapshot"]["exists"], false);
    assert!(document["target_config_snapshot"]["hash"].is_null());
    assert!(document["target_config_snapshot"]["size_bytes"].is_null());
    assert_eq!(document["operations"][0]["key"], "build.build-dir");
    assert_eq!(document["operations"][0]["current"], Value::Null);
    assert_eq!(document["operations"][0]["recommended"], "target/build");
    assert_eq!(document["operations"][0]["status"], "insert");
    assert!(
        document["operations"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("create"))
    );
    Ok(())
}

#[test]
fn cargo_config_preview_json_preserves_existing_config() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_preview_existing")?;
    let project = temp.path().join("project");
    let config = "[build]\ntarget-dir = \"target-out\"\n";
    write_project_config(&project, config)?;
    let config_path = project.join(".cargo/config.toml");
    let original_config = fs::read_to_string(&config_path)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "preview", "--project"])
        .arg(&project)
        .arg("--json")
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    assert_eq!(fs::read_to_string(&config_path)?, original_config);
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["target_config_file"], path_string(&config_path));
    assert_eq!(document["target_config_snapshot"]["exists"], true);
    assert!(
        document["target_config_snapshot"]["hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:"))
    );
    assert_eq!(
        document["target_config_snapshot"]["size_bytes"],
        config.len() as u64
    );
    assert_eq!(document["operations"][0]["status"], "insert");
    assert_eq!(document["modified_cargo_config_files"], false);
    Ok(())
}

#[test]
fn cargo_config_preview_refuses_when_build_dir_already_configured() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_preview_refused")?;
    let project = temp.path().join("project");
    write_project_config(
        &project,
        "[build]\ntarget-dir = \"target-out\"\nbuild-dir = \"build-out\"\n",
    )?;
    let config_path = project.join(".cargo/config.toml");
    let original_config = fs::read_to_string(&config_path)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "preview", "--json", "--project"])
        .arg(&project)
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    assert_eq!(fs::read_to_string(&config_path)?, original_config);
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["operations"][0]["key"], "build.build-dir");
    assert!(document["operations"][0]["recommended"].is_null());
    assert_eq!(document["operations"][0]["status"], "refused");
    assert!(
        document["operations"][0]["current"]
            .as_str()
            .is_some_and(|current| current.ends_with("build-out"))
    );
    assert!(
        document["operations"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("already configured"))
    );
    assert_eq!(document["modified_cargo_config_files"], false);
    Ok(())
}

#[test]
fn cargo_config_preview_rejects_apply_flags() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_preview_apply_reject")?;
    let project = temp.path().join("project");
    write_project_config(&project, "[build]\ntarget-dir = \"target-out\"\n")?;
    let config_path = project.join(".cargo/config.toml");
    let original_config = fs::read_to_string(&config_path)?;

    for flag in ["--apply", "--yes"] {
        let output = command_with_isolated_cargo_home(temp.path())
            .args(["cargo-config", "preview", flag, "--project"])
            .arg(&project)
            .output()?;

        assert_eq!(output.status.code(), Some(2), "{flag}");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(
            stderr.contains("read-only/dry-run only"),
            "{flag}: {stderr}"
        );
        assert!(stderr.contains("no Cargo config files can be modified"));
        assert_eq!(fs::read_to_string(&config_path)?, original_config);
    }
    Ok(())
}

#[test]
fn cargo_config_apply_preview_creates_absent_config() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_absent")?;
    let project = temp.path().join("project");
    write_project_manifest(&project)?;
    let config_path = project.join(".cargo/config.toml");
    let preview_path = generate_preview_json(temp.path(), &project)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["command"], "cargo-config apply");
    assert_eq!(document["preview_path"], path_string(&preview_path));
    assert_eq!(document["target_config_file"], path_string(&config_path));
    assert_eq!(document["applied"], true);
    assert_eq!(document["modified_cargo_config_files"], true);
    assert_eq!(document["operations"][0]["status"], "insert");

    let config = fs::read_to_string(&config_path)?;
    assert!(config.contains("[build]"));
    assert!(config.contains("build-dir = \"target/build\""));
    Ok(())
}

#[test]
fn cargo_config_apply_preview_preserves_existing_comments_and_keys() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_existing")?;
    let project = temp.path().join("project");
    write_project_config(
        &project,
        "# cargo settings\n[build]\n# output root\ntarget-dir = \"target-out\"\n",
    )?;
    let preview_path = generate_preview_json(temp.path(), &project)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["applied"], true);
    assert_eq!(document["modified_cargo_config_files"], true);

    let config = fs::read_to_string(project.join(".cargo/config.toml"))?;
    assert!(config.contains("# cargo settings"));
    assert!(config.contains("# output root"));
    assert!(config.contains("target-dir = \"target-out\""));
    assert!(config.contains("build-dir = \"target/build\""));
    Ok(())
}

#[test]
fn cargo_config_apply_preview_requires_yes_without_mutation() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_requires_yes")?;
    let project = temp.path().join("project");
    write_project_manifest(&project)?;
    let config_path = project.join(".cargo/config.toml");
    let preview_path = generate_preview_json(temp.path(), &project)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .output()?;

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("requires --yes"));
    assert!(!config_path.exists());
    Ok(())
}

#[test]
fn cargo_config_apply_preview_refuses_stale_snapshot_without_mutation() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cli_cargo_config_apply_stale")?;
    let project = temp.path().join("project");
    write_project_config(&project, "[build]\ntarget-dir = \"target-out\"\n")?;
    let config_path = project.join(".cargo/config.toml");
    let preview_path = generate_preview_json(temp.path(), &project)?;
    let stale_config = "[build]\ntarget-dir = \"changed-target\"\n";
    fs::write(&config_path, stale_config)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["applied"], false);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(document["operations"][0]["status"], "refused");
    assert!(
        document["operations"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("snapshot"))
    );
    assert_eq!(fs::read_to_string(&config_path)?, stale_config);
    Ok(())
}

#[test]
fn cargo_config_apply_preview_refuses_refused_operation_without_mutation()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_refused_preview")?;
    let project = temp.path().join("project");
    write_project_config(
        &project,
        "[build]\ntarget-dir = \"target-out\"\nbuild-dir = \"build-out\"\n",
    )?;
    let config_path = project.join(".cargo/config.toml");
    let original_config = fs::read_to_string(&config_path)?;
    let preview_path = generate_preview_json(temp.path(), &project)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["applied"], false);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(document["operations"][0]["status"], "refused");
    assert!(
        document["operations"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("supported"))
    );
    assert_eq!(fs::read_to_string(&config_path)?, original_config);
    Ok(())
}

#[test]
fn cargo_config_apply_preview_refuses_non_cargo_config_target_without_mutation()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cli_cargo_config_apply_target_path")?;
    let project = temp.path().join("project");
    write_project_manifest(&project)?;
    let preview_path = generate_preview_json(temp.path(), &project)?;
    let mut preview: Value = serde_json::from_slice(&fs::read(&preview_path)?)?;
    let target_path = temp.path().join("not-cargo/config.toml");
    preview["target_config_file"] = Value::String(path_string(&target_path));
    fs::write(&preview_path, serde_json::to_vec(&preview)?)?;

    let output = command_with_isolated_cargo_home(temp.path())
        .args(["cargo-config", "apply", "--preview"])
        .arg(&preview_path)
        .args(["--yes", "--json"])
        .output()?;

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["applied"], false);
    assert_eq!(document["modified_cargo_config_files"], false);
    assert_eq!(document["operations"][0]["status"], "refused");
    assert!(
        document["operations"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains(".cargo/config.toml"))
    );
    assert!(!target_path.exists());
    Ok(())
}

fn write_project_config(project: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(project.join(".cargo"))?;
    write_project_manifest(project)?;
    fs::write(project.join(".cargo/config.toml"), contents)?;
    Ok(())
}

fn generate_preview_json(root: &Path, project: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let output = command_with_isolated_cargo_home(root)
        .args(["cargo-config", "preview", "--json", "--project"])
        .arg(project)
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.is_empty());
    let preview_path = root.join("preview.json");
    fs::write(&preview_path, output.stdout)?;
    Ok(preview_path)
}

fn write_project_manifest(project: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(project)?;
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    Ok(())
}

fn command_with_isolated_cargo_home(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-reclaim"));
    command.env("CARGO_HOME", root.join("cargo-home"));
    command.env_remove("CARGO_BUILD_TARGET_DIR");
    command.env_remove("CARGO_TARGET_DIR");
    command.env_remove("CARGO_BUILD_BUILD_DIR");
    command
}

fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
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
