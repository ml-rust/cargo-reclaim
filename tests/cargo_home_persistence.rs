use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    CargoHomePlanRequest, PlanId, PlanPersistenceError, PolicyKind, SaveCargoHomePlanOptions,
    build_cargo_home_plan, ensure_cargo_home_plan_usable, load_cargo_home_plan_from_path,
    persist_cargo_home_plan, save_cargo_home_plan_to_path,
};

#[test]
fn cargo_home_persisted_plan_round_trips_with_stable_id() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_persistence_round_trip")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;

    let created_at = UNIX_EPOCH + Duration::from_secs(1_000);
    let expires_at = UNIX_EPOCH + Duration::from_secs(2_000);
    let plan = build_cargo_home_plan(CargoHomePlanRequest {
        cargo_home: Some(temp.path().to_path_buf()),
        policy: PolicyKind::Conservative,
    })?;
    let document = persist_cargo_home_plan(
        &plan,
        SaveCargoHomePlanOptions {
            created_at,
            expires_at,
        },
    )?;
    let path = temp.path().join("cargo-home-plan.json");
    save_cargo_home_plan_to_path(&path, &document)?;

    let loaded = load_cargo_home_plan_from_path(&path)?;
    assert_eq!(loaded, document);
    assert_eq!(loaded.id, PlanId::from_body(&loaded.body)?);
    ensure_cargo_home_plan_usable(&loaded, UNIX_EPOCH + Duration::from_secs(1_500))?;
    Ok(())
}

#[test]
fn cargo_home_persisted_plan_rejects_id_mismatch() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_persistence_id_mismatch")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;

    let mut document = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    document.body.policy = "aggressive".to_string();

    assert!(matches!(
        ensure_cargo_home_plan_usable(&document, UNIX_EPOCH + Duration::from_secs(1_500)),
        Err(PlanPersistenceError::PlanIdMismatch { .. })
    ));
    Ok(())
}

#[test]
fn cargo_home_persisted_plan_rejects_expiry() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_persistence_expiry")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;

    let document = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    assert!(matches!(
        ensure_cargo_home_plan_usable(&document, UNIX_EPOCH + Duration::from_secs(2_000)),
        Err(PlanPersistenceError::PlanExpired)
    ));
    Ok(())
}

#[test]
fn cargo_home_persisted_plan_rejects_wrong_command_and_unknown_policy() -> Result<(), Box<dyn Error>>
{
    let temp = TestTemp::new("cargo_home_persistence_semantics")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;

    let mut wrong_command = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    wrong_command.body.command = "plan".to_string();
    wrong_command.id = PlanId::from_body(&wrong_command.body)?;
    assert!(matches!(
        ensure_cargo_home_plan_usable(&wrong_command, UNIX_EPOCH + Duration::from_secs(1_500)),
        Err(PlanPersistenceError::InvalidPlan { .. })
    ));

    let mut unknown_policy = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    unknown_policy.body.policy = "surprise".to_string();
    unknown_policy.id = PlanId::from_body(&unknown_policy.body)?;
    assert!(matches!(
        ensure_cargo_home_plan_usable(&unknown_policy, UNIX_EPOCH + Duration::from_secs(1_500)),
        Err(PlanPersistenceError::InvalidPlan { .. })
    ));
    Ok(())
}

#[test]
fn cargo_home_persisted_plan_has_cargo_home_specific_json_shape() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_persistence_shape")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    fs::write(temp.path().join("config.toml"), b"[net]\n")?;

    let document = persisted_plan(temp.path(), PolicyKind::Conservative)?;
    let value = serde_json::to_value(&document)?;

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["command"], "cargo-home plan");
    assert_eq!(value["policy"], "conservative");
    assert_eq!(value["plan"]["schema_version"], 1);
    assert_eq!(
        value["plan"]["input"]["root"],
        temp.path().canonicalize()?.display().to_string()
    );
    assert_eq!(value["plan"]["input"]["source"], "explicit");
    assert!(value.get("interactive_selection_modified").is_none());
    assert!(value.get("invocation").is_none());
    assert!(value["plan"].get("roots").is_none());

    let entries = value["plan"]["entries"].as_array().expect("entries array");
    let cache = entries
        .iter()
        .find(|entry| entry["class"] == "registry_cache")
        .expect("registry cache entry");
    assert_eq!(cache["action"], "delete_candidate");
    assert_eq!(cache["relative_path"], "registry/cache");
    assert!(
        cache["content_fingerprint"]
            .as_str()
            .is_some_and(|fingerprint| fingerprint.starts_with("sha256:"))
    );
    let config = entries
        .iter()
        .find(|entry| entry["class"] == "config")
        .expect("config entry");
    assert!(config.get("content_fingerprint").is_none());
    assert!(cache.get("artifact_class").is_none());
    assert!(cache.get("evidence").is_none());
    Ok(())
}

fn persisted_plan(
    cargo_home: &Path,
    policy: PolicyKind,
) -> Result<cargo_reclaim::PersistedCargoHomePlan, Box<dyn Error>> {
    let plan = build_cargo_home_plan(CargoHomePlanRequest {
        cargo_home: Some(cargo_home.to_path_buf()),
        policy,
    })?;
    Ok(persist_cargo_home_plan(
        &plan,
        SaveCargoHomePlanOptions {
            created_at: UNIX_EPOCH + Duration::from_secs(1_000),
            expires_at: UNIX_EPOCH + Duration::from_secs(2_000),
        },
    )?)
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
