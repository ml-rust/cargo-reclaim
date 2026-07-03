use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ActiveObservation, ActiveObservationProvider, ActiveObservationScope, ArtifactClass, CargoTool,
    InventoryOptions, ObservedCargoProcess, PathKind, PlanAction, PlanInput, PlanSkipReason,
    PlannerOptions, PolicyKind, ScanItem, ScannerOptions, TargetCandidate, TargetCandidateKind,
    TargetEvidence, ToolchainHashError, ToolchainHashResolver, ToolchainHashResult,
    WholeTargetMode, build_plan_from_roots, build_plan_from_roots_with_active_observation,
    build_plan_from_roots_with_active_observation_provider, build_plan_from_roots_with_options,
    build_plan_from_scan_items, planner_candidates_from_target_root,
    resolve_toolchain_hash_options,
};

#[test]
fn scanned_project_target_builds_policy_plan_from_artifact_boundaries() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_project_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/debug/.rustdoc_fingerprint"))?;
    fs::create_dir_all(temp.path().join("target/sqlx-tmp"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::create_dir_all(temp.path().join("target/mystery"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(
        temp.path()
            .join("target/debug/.rustdoc_fingerprint/cache.bin"),
        b"rustdoc",
    )?;
    fs::write(temp.path().join("target/sqlx-tmp/query.json"), b"sqlx")?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;
    fs::write(temp.path().join("target/mystery/blob"), b"unknown")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.artifact_class, ArtifactClass::Incremental);
    assert_eq!(incremental.action, PlanAction::Delete);
    assert_eq!(incremental.snapshot.path_kind, PathKind::Directory);

    let rustdoc = entry_for(&plan, temp.path().join("target/debug/.rustdoc_fingerprint"))?;
    assert_eq!(rustdoc.artifact_class, ArtifactClass::Fingerprint);
    assert_eq!(rustdoc.action, PlanAction::Delete);
    assert_eq!(rustdoc.snapshot.path_kind, PathKind::Directory);

    let sqlx = entry_for(&plan, temp.path().join("target/sqlx-tmp"))?;
    assert_eq!(sqlx.artifact_class, ArtifactClass::Tmp);
    assert_eq!(sqlx.action, PlanAction::Delete);
    assert_eq!(sqlx.snapshot.path_kind, PathKind::Directory);

    let docs = entry_for(&plan, temp.path().join("target/doc"))?;
    assert_eq!(docs.artifact_class, ArtifactClass::Docs);
    assert_eq!(docs.action, PlanAction::Preserve);

    let unknown = entry_for(&plan, temp.path().join("target/mystery/blob"))?;
    assert_eq!(unknown.artifact_class, ArtifactClass::Unknown);
    assert_eq!(unknown.action, PlanAction::Unknown);
    Ok(())
}

#[test]
fn scanned_project_target_keeps_deps_outputs_preserved_without_keep_window()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_project_deps_plan")?;
    write_manifest(temp.path())?;
    let deps = temp.path().join("target/debug/deps");
    fs::create_dir_all(&deps)?;
    fs::write(deps.join("sample-123.d"), b"dep")?;
    fs::write(deps.join("sample-123.o"), b"obj")?;
    fs::write(deps.join("libsample-123.rlib"), b"rlib")?;
    fs::write(deps.join("sample-123"), b"bin")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    for child in [
        deps.join("sample-123.d"),
        deps.join("sample-123.o"),
        deps.join("libsample-123.rlib"),
        deps.join("sample-123"),
    ] {
        let entry = entry_for(&plan, child)?;
        assert_eq!(entry.artifact_class, ArtifactClass::DepsOutput);
        assert_eq!(entry.action, PlanAction::Preserve);
    }

    Ok(())
}

#[test]
fn scanned_project_target_adds_hash_grouped_intermediates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_hash_group_plan")?;
    write_manifest(temp.path())?;
    let hash = "0123456789abcdef";
    let target = temp.path().join("target");
    fs::create_dir_all(target.join(format!("debug/.fingerprint/sample-{hash}")))?;
    fs::create_dir_all(target.join("debug/deps"))?;
    fs::write(
        target.join(format!("debug/.fingerprint/sample-{hash}/fingerprint.json")),
        br#"{"rustc":1}"#,
    )?;
    fs::write(target.join(format!("debug/sample-{hash}.json")), b"profile")?;
    fs::write(target.join(format!("debug/deps/sample-{hash}.d")), b"dep")?;
    fs::write(
        target.join(format!("debug/deps/sample-{hash}.json")),
        b"tracked",
    )?;
    fs::write(
        target.join(format!("debug/deps/libsample-{hash}.rlib")),
        b"rlib",
    )?;
    fs::write(
        target.join(format!("debug/deps/sample-{hash}.rmeta")),
        b"rmeta",
    )?;
    fs::write(target.join(format!("debug/sample-{hash}")), b"binary")?;
    fs::write(target.join("debug/other-1111111111111111.json"), b"other")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let profile = entry_for(&plan, target.join(format!("debug/sample-{hash}.json")))?;
    assert_eq!(
        profile.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_eq!(profile.action, PlanAction::Delete);

    for deps_child in [
        target.join(format!("debug/deps/sample-{hash}.json")),
        target.join(format!("debug/deps/sample-{hash}.d")),
        target.join(format!("debug/deps/libsample-{hash}.rlib")),
        target.join(format!("debug/deps/sample-{hash}.rmeta")),
    ] {
        let entry = entry_for(&plan, deps_child)?;
        assert_eq!(entry.artifact_class, ArtifactClass::DepsOutput);
        assert_eq!(entry.action, PlanAction::Preserve);
    }

    let binary = entry_for(&plan, target.join(format!("debug/sample-{hash}")))?;
    assert_ne!(
        binary.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_ne!(binary.action, PlanAction::Delete);

    let other = entry_for(&plan, target.join("debug/other-1111111111111111.json"))?;
    assert_eq!(other.artifact_class, ArtifactClass::Unknown);
    assert_eq!(other.action, PlanAction::Unknown);
    Ok(())
}

#[test]
fn keep_rustc_hash_preserves_matching_hash_grouped_intermediates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_keep_rustc_hash")?;
    write_manifest(temp.path())?;
    let kept_hash = "0123456789abcdef";
    let unkept_hash = "1111111111111111";
    let target = temp.path().join("target");
    fs::create_dir_all(target.join(format!("debug/.fingerprint/kept-{kept_hash}")))?;
    fs::create_dir_all(target.join(format!("debug/.fingerprint/unkept-{unkept_hash}")))?;
    fs::write(
        target.join(format!(
            "debug/.fingerprint/kept-{kept_hash}/fingerprint.json"
        )),
        br#"{"rustc":7}"#,
    )?;
    fs::write(
        target.join(format!(
            "debug/.fingerprint/unkept-{unkept_hash}/fingerprint.json"
        )),
        br#"{"rustc":8}"#,
    )?;
    fs::write(target.join(format!("debug/kept-{kept_hash}.json")), b"kept")?;
    fs::write(
        target.join(format!("debug/unkept-{unkept_hash}.json")),
        b"unkept",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            keep_rustc_hashes: vec![7],
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let kept = entry_for(&plan, target.join(format!("debug/kept-{kept_hash}.json")))?;
    assert_eq!(kept.artifact_class, ArtifactClass::Unknown);
    assert_eq!(kept.action, PlanAction::Unknown);

    let unkept = entry_for(
        &plan,
        target.join(format!("debug/unkept-{unkept_hash}.json")),
    )?;
    assert_eq!(
        unkept.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_eq!(unkept.action, PlanAction::Delete);
    Ok(())
}

#[test]
fn resolved_toolchain_hash_preserves_matching_hash_grouped_intermediates()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_keep_toolchain_hash")?;
    write_manifest(temp.path())?;
    let kept_hash = "0123456789abcdef";
    let unkept_hash = "1111111111111111";
    let target = temp.path().join("target");
    fs::create_dir_all(target.join(format!("debug/.fingerprint/kept-{kept_hash}")))?;
    fs::create_dir_all(target.join(format!("debug/.fingerprint/unkept-{unkept_hash}")))?;
    fs::write(
        target.join(format!(
            "debug/.fingerprint/kept-{kept_hash}/fingerprint.json"
        )),
        br#"{"rustc":7}"#,
    )?;
    fs::write(
        target.join(format!(
            "debug/.fingerprint/unkept-{unkept_hash}/fingerprint.json"
        )),
        br#"{"rustc":8}"#,
    )?;
    fs::write(target.join(format!("debug/kept-{kept_hash}.json")), b"kept")?;
    fs::write(
        target.join(format!("debug/unkept-{unkept_hash}.json")),
        b"unkept",
    )?;

    let mut planner_options = PlannerOptions {
        keep_installed_toolchains: true,
        keep_toolchains: vec!["stable".to_string()],
        ..PlannerOptions::default()
    };
    resolve_toolchain_hash_options(
        &mut planner_options,
        &FakeToolchainHashResolver {
            installed: vec!["nightly".to_string()],
        },
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &planner_options,
        SystemTime::now(),
    )?;

    let kept = entry_for(&plan, target.join(format!("debug/kept-{kept_hash}.json")))?;
    assert_eq!(kept.artifact_class, ArtifactClass::Unknown);
    assert_eq!(kept.action, PlanAction::Unknown);

    let unkept = entry_for(
        &plan,
        target.join(format!("debug/unkept-{unkept_hash}.json")),
    )?;
    assert_eq!(
        unkept.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_eq!(unkept.action, PlanAction::Delete);
    Ok(())
}

struct FakeToolchainHashResolver {
    installed: Vec<String>,
}

impl ToolchainHashResolver for FakeToolchainHashResolver {
    fn installed_toolchains(&self) -> ToolchainHashResult<Vec<String>> {
        Ok(self.installed.clone())
    }

    fn toolchain_rustc_hash(&self, toolchain: &str) -> ToolchainHashResult<u64> {
        match toolchain {
            "stable" | "nightly" => Ok(7),
            _ => Err(ToolchainHashError::EmptyRustcVersion {
                toolchain: toolchain.to_string(),
            }),
        }
    }
}

#[test]
fn keep_rustc_hash_preserves_conflicting_duplicate_hash_group() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_keep_rustc_hash_conflict")?;
    write_manifest(temp.path())?;
    let hash = "0123456789abcdef";
    let target = temp.path().join("target");
    fs::create_dir_all(target.join(format!("debug/.fingerprint/kept-{hash}")))?;
    fs::create_dir_all(target.join(format!("debug/.fingerprint/unkept-{hash}")))?;
    fs::write(
        target.join(format!("debug/.fingerprint/kept-{hash}/fingerprint.json")),
        br#"{"rustc":7}"#,
    )?;
    fs::write(
        target.join(format!("debug/.fingerprint/unkept-{hash}/fingerprint.json")),
        br#"{"rustc":8}"#,
    )?;
    fs::write(
        target.join(format!("debug/conflicting-{hash}.json")),
        b"conflict",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            keep_rustc_hashes: vec![7],
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let entry = entry_for(&plan, target.join(format!("debug/conflicting-{hash}.json")))?;
    assert_eq!(entry.artifact_class, ArtifactClass::Unknown);
    assert_eq!(entry.action, PlanAction::Unknown);
    Ok(())
}

#[test]
fn hash_grouped_intermediates_respect_policy_and_weak_evidence() -> Result<(), Box<dyn Error>> {
    let hash = "0123456789abcdef";
    let strong = TestTemp::new("integration_hash_policy_strong")?;
    write_manifest(strong.path())?;
    let strong_target = strong.path().join("target");
    fs::create_dir_all(strong_target.join(format!("debug/.fingerprint/sample-{hash}")))?;
    fs::write(
        strong_target.join(format!("debug/.fingerprint/sample-{hash}/fingerprint.json")),
        br#"{"rustc":1}"#,
    )?;
    fs::write(
        strong_target.join(format!("debug/sample-{hash}.json")),
        b"profile",
    )?;

    let conservative = build_plan_from_roots(
        [strong.path()],
        PolicyKind::Conservative,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    let conservative_entry = entry_for(
        &conservative,
        strong_target.join(format!("debug/sample-{hash}.json")),
    )?;
    assert_eq!(
        conservative_entry.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_eq!(conservative_entry.action, PlanAction::Preserve);

    let weak = TestTemp::new("integration_hash_policy_weak")?;
    let weak_target = weak.path().join("target");
    fs::create_dir_all(weak_target.join(format!("debug/.fingerprint/sample-{hash}")))?;
    fs::write(
        weak_target.join(format!("debug/.fingerprint/sample-{hash}/fingerprint.json")),
        br#"{"rustc":1}"#,
    )?;
    fs::write(
        weak_target.join(format!("debug/sample-{hash}.json")),
        b"profile",
    )?;

    let weak_plan = build_plan_from_roots(
        [weak.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;
    let weak_entry = entry_for(
        &weak_plan,
        weak_target.join(format!("debug/sample-{hash}.json")),
    )?;
    assert_eq!(
        weak_entry.artifact_class,
        ArtifactClass::FingerprintGroupIntermediate
    );
    assert_eq!(weak_entry.action, PlanAction::RequiresConfirmation);
    assert!(weak_entry.requires_confirmation);
    Ok(())
}

#[test]
fn hash_grouped_intermediates_require_valid_fingerprint_anchor_and_respect_skips()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_hash_group_skips")?;
    write_manifest(temp.path())?;
    let hash = "0123456789abcdef";
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/.fingerprint/sample-nothex"))?;
    fs::write(target.join(format!("debug/sample-{hash}.json")), b"profile")?;

    let without_anchor = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    let ungrouped = entry_for(
        &without_anchor,
        target.join(format!("debug/sample-{hash}.json")),
    )?;
    assert_eq!(ungrouped.artifact_class, ArtifactClass::Unknown);
    assert_eq!(ungrouped.action, PlanAction::Unknown);

    fs::create_dir_all(target.join(format!("debug/.fingerprint/sample-{hash}")))?;
    fs::write(
        target.join(format!("debug/.fingerprint/sample-{hash}/fingerprint.json")),
        b"not-json",
    )?;
    let invalid_metadata = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    let invalid_metadata_entry = entry_for(
        &invalid_metadata,
        target.join(format!("debug/sample-{hash}.json")),
    )?;
    assert_eq!(
        invalid_metadata_entry.artifact_class,
        ArtifactClass::Unknown
    );
    assert_eq!(invalid_metadata_entry.action, PlanAction::Unknown);

    fs::write(
        target.join(format!("debug/.fingerprint/sample-{hash}/fingerprint.json")),
        br#"{"rustc":1}"#,
    )?;
    let skipped = target.join(format!("debug/sample-{hash}.json"));
    let with_skip = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            skipped_paths: vec![skipped.clone()],
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;
    assert!(
        !with_skip
            .entries
            .iter()
            .any(|entry| entry.snapshot.path == skipped)
    );
    Ok(())
}

#[test]
fn skipped_path_inside_target_root_is_pruned_from_plan_entries() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_skip_inside_target")?;
    write_manifest(temp.path())?;
    let skipped = temp.path().join("target/debug/incremental");
    fs::create_dir_all(&skipped)?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(skipped.join("cache.bin"), b"abc")?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            skipped_paths: vec![skipped.clone()],
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;

    assert!(
        !plan
            .entries
            .iter()
            .any(|entry| entry.snapshot.path.starts_with(&skipped))
    );
    let docs = entry_for(&plan, temp.path().join("target/doc"))?;
    assert_eq!(docs.artifact_class, ArtifactClass::Docs);
    Ok(())
}

#[test]
fn scanner_skips_are_carried_as_plan_diagnostics() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_scan_skip_diagnostics")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join(".git"))?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_eq!(plan.totals.skipped_path_count, 1);
    assert_eq!(plan.skipped_paths[0].path, temp.path().join(".git"));
    assert_eq!(
        plan.skipped_paths[0].reason,
        cargo_reclaim::PlanSkipReason::DefaultIgnoredDir
    );
    assert_eq!(plan.skipped_paths[0].message, None);
    assert!(plan.entries.iter().any(|entry| {
        entry.snapshot.path == temp.path().join("target/debug/incremental")
            && entry.action == PlanAction::Delete
    }));
    Ok(())
}

#[test]
fn vanished_inventory_paths_become_plan_skipped_diagnostics() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_vanished_inventory")?;
    let target = temp.path().join("target");

    let plan = build_plan_from_scan_items(
        PlanInput::from_root(temp.path())?,
        PolicyKind::Balanced,
        [ScanItem::TargetCandidate(TargetCandidate {
            path: target.clone(),
            kind: TargetCandidateKind::CargoTargetDir,
            evidence: Some(TargetEvidence::strong_marker("CACHEDIR.TAG")?),
            target_context: None,
            skip_reason: None,
        })],
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert!(plan.entries.is_empty());
    assert_eq!(plan.totals.skipped_path_count, 1);
    assert_eq!(plan.skipped_paths[0].path, target);
    assert_eq!(
        plan.skipped_paths[0].reason,
        PlanSkipReason::VanishedDuringInventory
    );
    assert!(
        plan.skipped_paths[0]
            .message
            .as_deref()
            .is_some_and(|message| message.contains("active build"))
    );
    Ok(())
}

#[test]
fn whole_target_mode_falls_back_to_content_entries_when_skip_is_inside_target()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_skip_whole_target")?;
    write_manifest(temp.path())?;
    let skipped = temp.path().join("target/debug/incremental");
    fs::create_dir_all(&skipped)?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(skipped.join("cache.bin"), b"abc")?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Aggressive,
        &ScannerOptions::default(),
        &InventoryOptions {
            skipped_paths: vec![skipped.clone()],
            ..InventoryOptions::default()
        },
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    assert!(plan.entries.iter().all(|entry| {
        entry.artifact_class != ArtifactClass::WholeTarget
            && !entry.snapshot.path.starts_with(&skipped)
    }));
    let docs = entry_for(&plan, temp.path().join("target/doc"))?;
    assert_eq!(docs.artifact_class, ArtifactClass::Docs);
    Ok(())
}

#[test]
fn configured_custom_target_builds_policy_plan_from_scanned_roots() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_configured_target_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"custom-target\"\n",
    )?;
    fs::create_dir_all(temp.path().join("custom-target/debug/incremental"))?;
    fs::write(
        temp.path()
            .join("custom-target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("custom-target/debug/incremental"))?;
    assert_eq!(incremental.artifact_class, ArtifactClass::Incremental);
    assert_eq!(incremental.action, PlanAction::Delete);
    assert!(matches!(
        incremental.evidence,
        TargetEvidence::ConfiguredPath { ref source } if source.contains("build.target-dir")
    ));
    Ok(())
}

#[test]
fn observe_policy_preserves_delete_capable_entries_from_scanned_roots() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_observe_plan")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Observe,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::Preserve);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    Ok(())
}

#[test]
fn recent_write_keep_window_skips_scanned_delete_candidates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_recent_write")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            recent_write_keep_window: Some(std::time::Duration::from_secs(24 * 60 * 60)),
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::SkipActive);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    assert_eq!(plan.totals.preserved_count, 1);
    Ok(())
}

#[test]
fn keep_size_preserves_small_scanned_delete_candidates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_keep_size")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            keep_size_bytes: Some(1024 * 1024),
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::Preserve);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    assert_eq!(plan.totals.preserved_count, 1);
    Ok(())
}

#[test]
fn whole_target_mode_emits_target_root_candidate_and_skips_children() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_whole_target_confirm")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("target/doc"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(temp.path().join("target/doc/index.html"), b"docs")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::Confirm,
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    assert_eq!(plan.entries.len(), 1);
    let entry = entry_for(&plan, temp.path().join("target"))?;
    assert_eq!(entry.artifact_class, ArtifactClass::WholeTarget);
    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert_eq!(entry.snapshot.size_bytes, 7);
    assert!(entry.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_confirmed_delete_requires_aggressive_non_weak_target() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_whole_target_delete")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Aggressive,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let entry = entry_for(&plan, temp.path().join("target"))?;
    assert_eq!(entry.artifact_class, ArtifactClass::WholeTarget);
    assert_eq!(entry.action, PlanAction::Delete);
    assert!(!entry.requires_confirmation);
    Ok(())
}

#[test]
fn whole_target_name_only_target_stays_confirmation_only_when_allowed() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_whole_target_weak")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Aggressive,
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
        &PlannerOptions {
            whole_target_mode: WholeTargetMode::DeleteConfirmed,
            ..PlannerOptions::default()
        },
        SystemTime::now(),
    )?;

    let entry = entry_for(&plan, temp.path().join("target"))?;
    assert_eq!(entry.artifact_class, ArtifactClass::WholeTarget);
    assert_eq!(entry.action, PlanAction::RequiresConfirmation);
    assert!(entry.requires_confirmation);
    assert_eq!(entry.evidence, TargetEvidence::weak_name_only("target")?);
    Ok(())
}

#[test]
fn active_project_observation_skips_scanned_delete_candidates() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_active_observation")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots_with_active_observation(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions::default(),
        &ActiveObservation::complete([
            ObservedCargoProcess::new(CargoTool::Cargo).with_cwd(temp.path().join("member"))
        ]),
        SystemTime::now(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::SkipActive);
    assert_eq!(plan.totals.delete_candidate_count, 0);
    assert_eq!(plan.totals.preserved_count, 1);
    Ok(())
}

#[test]
fn active_provider_cargo_cwd_under_scanned_project_yields_skip_active() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_active_provider")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let provider = FakeActiveObservationProvider::new(ActiveObservation::complete([
        ObservedCargoProcess::new(CargoTool::Cargo).with_cwd(temp.path().join("member")),
    ]));
    let plan = build_plan_from_roots_with_active_observation_provider(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions::default(),
        &provider,
        SystemTime::now(),
    )?;

    let incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::SkipActive);
    Ok(())
}

#[test]
fn configured_build_dir_reference_protects_target_and_build_candidates()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_active_build_dir")?;
    write_manifest(temp.path())?;
    fs::create_dir(temp.path().join(".cargo"))?;
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nbuild-dir = \"custom-build\"\n",
    )?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::create_dir_all(temp.path().join("custom-build/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;
    fs::write(
        temp.path().join("custom-build/debug/incremental/cache.bin"),
        b"def",
    )?;

    let provider = FakeActiveObservationProvider::new(ActiveObservation::complete([
        ObservedCargoProcess::new(CargoTool::Rustc)
            .with_referenced_path(temp.path().join("custom-build/debug/deps/unit.o")),
    ]));
    let plan = build_plan_from_roots_with_active_observation_provider(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions::default(),
        &provider,
        SystemTime::now(),
    )?;

    let target_incremental = entry_for(&plan, temp.path().join("target/debug/incremental"))?;
    let build_incremental = entry_for(&plan, temp.path().join("custom-build/debug/incremental"))?;
    assert_eq!(target_incremental.action, PlanAction::SkipActive);
    assert_eq!(build_incremental.action, PlanAction::SkipActive);
    Ok(())
}

#[test]
fn protected_outputs_are_preserved_from_scanned_target_contents() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_protected_outputs")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/package"))?;
    fs::create_dir_all(temp.path().join("target/timings"))?;
    fs::create_dir_all(temp.path().join("target/debug"))?;
    fs::write(temp.path().join("target/package/sample.crate"), b"crate")?;
    fs::write(
        temp.path().join("target/timings/cargo-timing.html"),
        b"time",
    )?;
    fs::write(temp.path().join("target/debug/sample"), b"bin")?;
    fs::write(temp.path().join("target/debug/libsample.rlib"), b"rlib")?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    for (path, artifact_class) in [
        (temp.path().join("target/package"), ArtifactClass::Package),
        (temp.path().join("target/timings"), ArtifactClass::Timings),
        (
            temp.path().join("target/debug/sample"),
            ArtifactClass::FinalExecutable,
        ),
        (
            temp.path().join("target/debug/libsample.rlib"),
            ArtifactClass::FinalRlib,
        ),
    ] {
        let entry = entry_for(&plan, path)?;
        assert_eq!(entry.artifact_class, artifact_class);
        assert_eq!(entry.action, PlanAction::Preserve);
    }

    Ok(())
}

#[test]
fn deps_outputs_are_reclaimable_only_with_recent_write_keep_window() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_deps_output_window")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/deps"))?;
    let deps_binary = temp
        .path()
        .join("target/debug/deps/sample-0123456789abcdef");
    let deps_rlib = temp
        .path()
        .join("target/debug/deps/libsample-0123456789abcdef.rlib");
    fs::write(&deps_binary, b"bin")?;
    fs::write(&deps_rlib, b"rlib")?;

    let without_window = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    for path in [&deps_binary, &deps_rlib] {
        let entry = entry_for(&without_window, path.to_path_buf())?;
        assert_eq!(entry.artifact_class, ArtifactClass::DepsOutput);
        assert_eq!(entry.action, PlanAction::Preserve);
    }

    let with_window = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            recent_write_keep_window: Some(std::time::Duration::from_secs(1)),
            ..PlannerOptions::default()
        },
        SystemTime::now() + std::time::Duration::from_secs(10),
    )?;
    for path in [&deps_binary, &deps_rlib] {
        let entry = entry_for(&with_window, path.to_path_buf())?;
        assert_eq!(entry.artifact_class, ArtifactClass::DepsOutput);
        assert_eq!(entry.action, PlanAction::Delete);
    }

    Ok(())
}

#[test]
fn persisted_deps_output_delete_entry_uses_fast_snapshot_revalidation() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("integration_deps_output_persistence")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/deps"))?;
    let deps_binary = temp
        .path()
        .join("target/debug/deps/sample-0123456789abcdef");
    fs::write(&deps_binary, b"bin")?;

    let plan = build_plan_from_roots_with_options(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
        &PlannerOptions {
            recent_write_keep_window: Some(std::time::Duration::from_secs(1)),
            ..PlannerOptions::default()
        },
        SystemTime::now() + std::time::Duration::from_secs(10),
    )?;
    let created_at = SystemTime::now();
    let document = cargo_reclaim::persist_plan(
        &plan,
        cargo_reclaim::SavePlanOptions {
            created_at,
            expires_at: created_at + std::time::Duration::from_secs(300),
            interactive_selection_modified: false,
            invocation: cargo_reclaim::PlanInvocation::new(
                cargo_reclaim::PlanCommandKind::Plan,
                PolicyKind::Balanced,
                &ScannerOptions::default(),
                &InventoryOptions::default(),
                &PlannerOptions {
                    recent_write_keep_window: Some(std::time::Duration::from_secs(1)),
                    ..PlannerOptions::default()
                },
            ),
        },
    )?;
    let value = serde_json::to_value(&document)?;
    let deps_entry = value["plan"]["entries"]
        .as_array()
        .expect("entries should be an array")
        .iter()
        .find(|entry| entry["artifact_class"] == "deps_output")
        .expect("persisted deps output entry");

    assert_eq!(deps_entry["snapshot"]["path_kind"], "file");
    assert!(deps_entry["snapshot"].get("content_fingerprint").is_none());
    Ok(())
}

#[test]
fn weak_name_only_targets_are_suppressed_by_default_and_confirmation_gated_when_allowed()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_weak_targets")?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let suppressed = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    assert!(suppressed.entries.is_empty());

    let allowed = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;

    let incremental = entry_for(&allowed, temp.path().join("target/debug/incremental"))?;
    assert_eq!(incremental.action, PlanAction::RequiresConfirmation);
    assert!(incremental.requires_confirmation);
    assert_eq!(
        incremental.evidence,
        TargetEvidence::weak_name_only("target")?
    );
    Ok(())
}

#[test]
fn lower_level_scan_item_planning_still_requires_explicit_weak_target_option()
-> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_weak_scan_items")?;
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("debug/incremental"))?;
    fs::write(target.join("debug/incremental/cache.bin"), b"abc")?;
    let input = cargo_reclaim::PlanInput::from_root(temp.path())?;
    let weak_candidate = cargo_reclaim::ScanItem::TargetCandidate(TargetCandidate {
        path: target.clone(),
        kind: TargetCandidateKind::CargoTargetDir,
        evidence: Some(TargetEvidence::weak_name_only("target")?),
        target_context: None,
        skip_reason: None,
    });

    let suppressed = build_plan_from_scan_items(
        input.clone(),
        PolicyKind::Balanced,
        [weak_candidate.clone()],
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;
    assert!(suppressed.entries.is_empty());

    let allowed = build_plan_from_scan_items(
        input,
        PolicyKind::Balanced,
        [weak_candidate],
        &ScannerOptions {
            allow_name_only_targets: true,
            ..ScannerOptions::default()
        },
        &InventoryOptions::default(),
    )?;
    assert_eq!(allowed.entries.len(), 1);
    assert_eq!(allowed.entries[0].action, PlanAction::RequiresConfirmation);
    Ok(())
}

#[test]
fn duplicate_scan_roots_do_not_duplicate_plan_entries() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("integration_dedupe")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("target/debug/incremental"))?;
    fs::write(
        temp.path().join("target/debug/incremental/cache.bin"),
        b"abc",
    )?;

    let plan = build_plan_from_roots(
        [temp.path(), temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].snapshot.path,
        temp.path().join("target/debug/incremental")
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlinked_target_root_is_rejected_by_inventory_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("integration_symlinked_target_root")?;
    let real_target = temp.path().join("real_target");
    fs::create_dir_all(real_target.join("debug/incremental"))?;
    fs::write(real_target.join("debug/incremental/cache.bin"), b"abc")?;
    let linked_target = temp.path().join("target");
    symlink(&real_target, &linked_target)?;

    let result = planner_candidates_from_target_root(
        &linked_target,
        TargetEvidence::strong_marker("CACHEDIR.TAG")?,
        &InventoryOptions::default(),
    );

    assert!(matches!(
        result,
        Err(cargo_reclaim::ReclaimError::InventorySymlinkNotFollowed { path })
            if path == linked_target
    ));
    Ok(())
}

#[test]
#[cfg(unix)]
fn target_content_symlinks_are_not_planned_by_default() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("integration_target_symlink")?;
    write_manifest(temp.path())?;
    fs::create_dir_all(temp.path().join("outside"))?;
    fs::write(temp.path().join("outside/file.d"), b"outside")?;
    fs::create_dir(temp.path().join("target"))?;
    symlink(
        temp.path().join("outside"),
        temp.path().join("target/linked"),
    )?;

    let plan = build_plan_from_roots(
        [temp.path()],
        PolicyKind::Balanced,
        &ScannerOptions::default(),
        &InventoryOptions::default(),
    )?;

    assert!(plan.entries.is_empty());
    Ok(())
}

fn entry_for(
    plan: &cargo_reclaim::Plan,
    path: PathBuf,
) -> Result<&cargo_reclaim::PlanEntry, Box<dyn Error>> {
    plan.entries
        .iter()
        .find(|entry| entry.snapshot.path == path)
        .ok_or_else(|| format!("missing plan entry for {}", path.display()).into())
}

struct FakeActiveObservationProvider {
    observation: ActiveObservation,
}

impl FakeActiveObservationProvider {
    fn new(observation: ActiveObservation) -> Self {
        Self { observation }
    }
}

impl ActiveObservationProvider for FakeActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> ActiveObservation {
        assert!(!scope.target_contexts().is_empty());
        self.observation.clone()
    }
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
