use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::scanner::{CargoConfigUnsupportedReason, resolve_project_output_dirs_with_env};
use cargo_reclaim::{
    ScannerOptions, TargetDirOverride, TargetDirOverrideSource, TargetEvidence,
    classify_target_candidate, detect_cargo_project,
};

#[test]
fn cargo_manifest_detection_accepts_manifest_file_and_project_directory()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("manifest_detection")?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"sample\"\n",
    )?;

    let from_dir = detect_cargo_project(temp.path());
    let from_manifest = detect_cargo_project(temp.path().join("Cargo.toml"));

    assert!(from_dir.is_some());
    assert_eq!(
        from_dir.as_ref().map(|project| project.root_path.as_path()),
        Some(temp.path())
    );
    assert_eq!(
        from_manifest.map(|project| project.manifest_path),
        Some(temp.path().join("Cargo.toml"))
    );
    assert!(detect_cargo_project(temp.path().join("src")).is_none());
    Ok(())
}

#[test]
fn cachedir_tag_marker_produces_strong_marker_evidence() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cachedir_marker")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(
        target.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;

    let candidate = classify_target_candidate(&target, None, None, &ScannerOptions::default())?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::strong_marker("CACHEDIR.TAG")?)
    );
    assert_eq!(candidate.skip_reason, None);
    Ok(())
}

#[test]
fn rustc_info_marker_produces_strong_marker_evidence() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("rustc_info_marker")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join(".rustc_info.json"), "{}\n")?;

    let candidate = classify_target_candidate(&target, None, None, &ScannerOptions::default())?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::strong_marker(".rustc_info.json")?)
    );
    assert_eq!(candidate.skip_reason, None);
    Ok(())
}

#[test]
fn configured_override_preserves_source_label() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("configured_override")?;
    let target = temp.path().join("custom-target");
    fs::create_dir(&target)?;
    let target_dir_override = TargetDirOverride::new(&target, "CARGO_TARGET_DIR")?;

    let candidate = classify_target_candidate(
        &target,
        None,
        Some(&target_dir_override),
        &ScannerOptions::default(),
    )?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::configured_path("CARGO_TARGET_DIR")?)
    );
    assert_eq!(candidate.skip_reason, None);
    Ok(())
}

#[test]
fn configured_override_takes_precedence_over_cache_marker() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("configured_override_marker")?;
    let target = temp.path().join("custom-target");
    fs::create_dir(&target)?;
    fs::write(
        target.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;
    let target_dir_override = TargetDirOverride::new(&target, "CARGO_TARGET_DIR")?;

    let candidate = classify_target_candidate(
        &target,
        None,
        Some(&target_dir_override),
        &ScannerOptions::default(),
    )?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::configured_path("CARGO_TARGET_DIR")?)
    );
    Ok(())
}

#[test]
fn configured_override_uses_lexical_path_matching() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("configured_override_normalized")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    let target_dir_override =
        TargetDirOverride::new(temp.path().join(".").join("target"), "CARGO_TARGET_DIR")?;

    let candidate = classify_target_candidate(
        &target,
        None,
        Some(&target_dir_override),
        &ScannerOptions::default(),
    )?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::configured_path("CARGO_TARGET_DIR")?)
    );
    Ok(())
}

#[test]
fn project_context_uses_adjacent_manifest_evidence() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("project_context")?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"sample\"\n",
    )?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;
    let project = detect_cargo_project(temp.path())
        .ok_or_else(|| std::io::Error::other("expected project"))?;

    let candidate =
        classify_target_candidate(&target, Some(&project), None, &ScannerOptions::default())?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::project_context(
            temp.path().join("Cargo.toml")
        )?)
    );
    assert_eq!(candidate.skip_reason, None);
    Ok(())
}

#[test]
fn target_name_fallback_uses_weak_evidence() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("name_fallback")?;
    let target = temp.path().join("target");
    fs::create_dir(&target)?;

    let candidate = classify_target_candidate(&target, None, None, &ScannerOptions::default())?;

    assert_eq!(
        candidate.evidence,
        Some(TargetEvidence::weak_name_only("target")?)
    );
    assert_eq!(candidate.skip_reason, None);
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_candidate_is_skipped_by_default_and_allowed_when_enabled() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("symlink_candidate")?;
    let real_target = temp.path().join("real-target");
    fs::create_dir(&real_target)?;
    fs::write(
        real_target.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;
    let linked_target = temp.path().join("target");
    symlink(&real_target, &linked_target)?;

    let skipped =
        classify_target_candidate(&linked_target, None, None, &ScannerOptions::default())?;
    assert_eq!(skipped.evidence, None);
    assert_eq!(
        skipped.skip_reason,
        Some(cargo_reclaim::SkipReason::SymlinkNotFollowed)
    );

    let followed = classify_target_candidate(
        &linked_target,
        None,
        None,
        &ScannerOptions {
            follow_symlinks: true,
            ..ScannerOptions::default()
        },
    )?;
    assert_eq!(
        followed.evidence,
        Some(TargetEvidence::strong_marker("CACHEDIR.TAG")?)
    );
    assert_eq!(followed.skip_reason, None);
    Ok(())
}

#[test]
#[cfg(unix)]
fn broken_symlink_candidate_is_still_skipped_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("broken_symlink_candidate")?;
    let linked_target = temp.path().join("target");
    symlink(temp.path().join("missing-target"), &linked_target)?;

    let skipped =
        classify_target_candidate(&linked_target, None, None, &ScannerOptions::default())?;

    assert_eq!(skipped.evidence, None);
    assert_eq!(
        skipped.skip_reason,
        Some(cargo_reclaim::SkipReason::SymlinkNotFollowed)
    );
    Ok(())
}

#[test]
fn target_dir_override_rejects_empty_input() {
    assert!(TargetDirOverride::new("", "CARGO_TARGET_DIR").is_err());
    assert!(TargetDirOverride::new("target", " ").is_err());
    assert!(TargetDirOverrideSource::new(" ").is_err());
}

#[test]
fn cargo_config_extensionless_file_wins_over_toml_file() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_precedence")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config"),
        "[build]\ntarget-dir = \"extensionless-target\"\n",
    )?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"toml-target\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert_eq!(dirs.dirs[0].path, temp.path().join("extensionless-target"));
    Ok(())
}

#[test]
fn cargo_config_include_merges_before_including_file_and_skips_optional_missing()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_include")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/base.toml"),
        "[build]\ntarget-dir = \"included-target\"\nbuild-dir = \"included-build\"\n",
    )?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "include = [\"base.toml\", { path = \"missing.toml\", optional = true }]\n[build]\ntarget-dir = \"local-target\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert_eq!(dirs.dirs[0].path, temp.path().join("local-target"));
    assert_eq!(dirs.dirs[1].path, temp.path().join("included-build"));
    Ok(())
}

#[test]
fn cargo_config_reports_missing_required_include() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_required_include")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "include = [\"missing.toml\"]\n[build]\ntarget-dir = \"local-target\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert_eq!(dirs.dirs[0].path, temp.path().join("local-target"));
    assert_eq!(dirs.problems.len(), 1);
    assert!(dirs.problems[0].path.ends_with(".cargo/missing.toml"));
    assert!(dirs.problems[0].message.contains("does not exist"));
    Ok(())
}

#[test]
fn cargo_config_toml_relative_paths_are_relative_to_config_parent_project()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_relative")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"nested/../custom-target\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert_eq!(dirs.dirs[0].path, temp.path().join("custom-target"));
    Ok(())
}

#[test]
fn cargo_config_environment_overrides_toml_and_direct_target_dir_env_wins()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_env")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"toml-target\"\n",
    )?;
    let env = env_for(temp.path()).into_iter().chain([
        ("CARGO_TARGET_DIR".into(), "legacy-target".into()),
        ("CARGO_BUILD_TARGET_DIR".into(), "direct-target".into()),
    ]);

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env)?;

    assert_eq!(dirs.dirs[0].path, temp.path().join("direct-target"));
    assert_eq!(dirs.dirs[0].source.label, "CARGO_BUILD_TARGET_DIR");
    Ok(())
}

#[test]
fn cargo_config_build_dir_defaults_and_dedupes_with_target_dir() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_dedupe")?;
    write_manifest(temp.path())?;

    let default_dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;
    assert!(default_dirs.dirs.is_empty());

    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"same\"\nbuild-dir = \"same\"\n",
    )?;

    let deduped = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;
    assert_eq!(deduped.dirs.len(), 1);
    assert_eq!(deduped.dirs[0].path, temp.path().join("same"));
    Ok(())
}

#[test]
fn cargo_config_build_dir_templates_resolve_workspace_root_and_cargo_cache_home()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_templates")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"target\"\nbuild-dir = \"{cargo-cache-home}/builds/{workspace-root}\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert_eq!(
        dirs.dirs[1].path,
        temp.path()
            .join("cargo-home/builds")
            .join(temp.path().to_string_lossy().trim_start_matches('/'))
    );
    Ok(())
}

#[test]
fn cargo_config_workspace_path_hash_template_is_reported_as_unsupported()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_config_unsupported")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nbuild-dir = \"{workspace-root}/{workspace-path-hash}\"\n",
    )?;

    let dirs = resolve_project_output_dirs_with_env(temp.path(), env_for(temp.path()))?;

    assert!(dirs.dirs.is_empty());
    assert_eq!(dirs.unsupported.len(), 1);
    assert_eq!(
        dirs.unsupported[0].reason,
        CargoConfigUnsupportedReason::WorkspacePathHashTemplate
    );
    Ok(())
}

fn env_for(temp: &std::path::Path) -> Vec<(String, String)> {
    vec![
        (
            "CARGO_HOME".to_string(),
            temp.join("cargo-home").display().to_string(),
        ),
        ("HOME".to_string(), temp.join("home").display().to_string()),
    ]
}

fn write_manifest(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
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

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
