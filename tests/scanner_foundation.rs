use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ScannerOptions, SkipReason, TargetDirOverride, TargetDirOverrideSource, TargetEvidence,
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
    assert_eq!(skipped.skip_reason, Some(SkipReason::SymlinkNotFollowed));

    let followed = classify_target_candidate(
        &linked_target,
        None,
        None,
        &ScannerOptions {
            follow_symlinks: true,
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
    assert_eq!(skipped.skip_reason, Some(SkipReason::SymlinkNotFollowed));
    Ok(())
}

#[test]
fn target_dir_override_rejects_empty_input() {
    assert!(TargetDirOverride::new("", "CARGO_TARGET_DIR").is_err());
    assert!(TargetDirOverride::new("target", " ").is_err());
    assert!(TargetDirOverrideSource::new(" ").is_err());
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
