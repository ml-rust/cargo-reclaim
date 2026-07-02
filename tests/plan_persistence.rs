use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ArtifactClass, InventoryOptions, PERSISTED_PLAN_SCHEMA_VERSION, PathKind, PathSnapshot, Plan,
    PlanAction, PlanCommandKind, PlanEntry, PlanInput, PlanInvocation, PlanPersistenceError,
    PlannerOptions, PolicyKind, SavePlanOptions, ScannerOptions, TargetEvidence,
    ensure_plan_usable, load_plan_from_path, persist_plan, save_plan_to_path,
};
use serde_json::json;

#[test]
fn persists_and_loads_plan_with_stable_id_and_timestamps() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("persistence_roundtrip")?;
    let created_at = UNIX_EPOCH + Duration::from_secs(1_000);
    let expires_at = created_at + Duration::from_secs(3_600);
    let modified = UNIX_EPOCH + Duration::new(900, 123);
    let plan = sample_plan(modified)?;
    let document = persist_plan(
        &plan,
        SavePlanOptions {
            created_at,
            expires_at,
            interactive_selection_modified: false,
            invocation: PlanInvocation::new(
                PlanCommandKind::Plan,
                PolicyKind::Balanced,
                &ScannerOptions {
                    ignored_paths: vec![PathBuf::from("ignored")],
                    ..ScannerOptions::default()
                },
                &InventoryOptions::default(),
                &PlannerOptions {
                    recent_write_keep_window: Some(Duration::from_secs(900)),
                },
            ),
        },
    )?;

    let path = temp.path.join("plan.json");
    save_plan_to_path(&path, &document)?;
    let loaded = load_plan_from_path(&path)?;

    assert_eq!(loaded.schema_version, PERSISTED_PLAN_SCHEMA_VERSION);
    assert_eq!(loaded.id, document.id);
    assert_eq!(loaded.id, cargo_reclaim::PlanId::from_body(&loaded.body)?);
    assert_eq!(loaded.body.created_at.unix_seconds, 1_000);
    assert_eq!(loaded.body.expires_at.unix_seconds, 4_600);
    assert!(!loaded.body.interactive_selection_modified);
    assert_eq!(loaded.body.invocation.command, PlanCommandKind::Plan);
    assert_eq!(loaded.body.invocation.policy, "balanced");
    assert_eq!(
        loaded.body.invocation.scanner_options.ignored_paths,
        ["ignored"]
    );
    assert_eq!(
        loaded
            .body
            .invocation
            .planner_options
            .recent_write_keep_window_seconds,
        Some(900)
    );
    assert_eq!(
        loaded.body.plan.entries[0]
            .snapshot
            .modified
            .unwrap()
            .nanoseconds,
        123
    );
    ensure_plan_usable(&loaded, created_at + Duration::from_secs(60))?;
    Ok(())
}

#[test]
fn rejects_expired_incompatible_and_mutated_documents() -> Result<(), Box<dyn Error>> {
    let created_at = UNIX_EPOCH + Duration::from_secs(1_000);
    let expires_at = created_at + Duration::from_secs(60);
    let mut document = persist_plan(
        &sample_plan(created_at)?,
        SavePlanOptions {
            created_at,
            expires_at,
            interactive_selection_modified: false,
            invocation: PlanInvocation::new(
                PlanCommandKind::Plan,
                PolicyKind::Balanced,
                &ScannerOptions::default(),
                &InventoryOptions::default(),
                &PlannerOptions::default(),
            ),
        },
    )?;

    assert!(matches!(
        ensure_plan_usable(&document, expires_at),
        Err(PlanPersistenceError::PlanExpired)
    ));

    document.schema_version = 999;
    assert!(matches!(
        ensure_plan_usable(&document, created_at),
        Err(PlanPersistenceError::PersistenceSchemaMismatch { .. })
    ));
    document.schema_version = PERSISTED_PLAN_SCHEMA_VERSION;

    document.body.plan.schema_version = 999;
    assert!(matches!(
        ensure_plan_usable(&document, created_at),
        Err(PlanPersistenceError::PlanSchemaMismatch { .. })
    ));
    document.body.plan.schema_version = cargo_reclaim::PLAN_SCHEMA_VERSION;

    document.body.plan.totals.total_bytes += 1;
    assert!(matches!(
        ensure_plan_usable(&document, created_at),
        Err(PlanPersistenceError::PlanIdMismatch { .. })
    ));
    Ok(())
}

#[test]
fn plan_invocation_defaults_missing_config_provenance() -> Result<(), Box<dyn Error>> {
    let invocation: PlanInvocation = serde_json::from_value(json!({
        "command": "plan",
        "policy": "balanced",
        "scanner_options": {
            "follow_symlinks": false,
            "allow_name_only_targets": false,
            "cross_filesystems": false,
            "ignored_paths": []
        },
        "inventory_options": {
            "follow_symlinks": false
        }
    }))?;

    assert_eq!(invocation.config_path, None);
    assert_eq!(invocation.config_version, None);
    assert_eq!(
        invocation.planner_options.recent_write_keep_window_seconds,
        None
    );
    Ok(())
}

fn sample_plan(modified: SystemTime) -> Result<Plan, Box<dyn Error>> {
    let snapshot = PathSnapshot::with_details(
        "target/debug/incremental",
        3,
        PathKind::Directory,
        Some(modified),
    )?;
    let entry = PlanEntry::new(
        snapshot,
        ArtifactClass::Incremental,
        TargetEvidence::project_context("Cargo.toml")?,
        PlanAction::Delete,
        "derived intermediate output",
        false,
    )?;
    Ok(Plan::new(PlanInput::from_root(".")?, vec![entry]))
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
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
