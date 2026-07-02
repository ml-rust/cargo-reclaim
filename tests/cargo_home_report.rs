use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    CargoHomeClass, CargoHomeReportRequest, CargoHomeSource, build_cargo_home_report,
    classify_cargo_home_relative_path,
};

#[test]
fn cargo_home_classifier_recognizes_book_layout_paths() {
    assert_eq!(
        classify_cargo_home_relative_path("registry/index"),
        CargoHomeClass::RegistryIndex
    );
    assert_eq!(
        classify_cargo_home_relative_path("registry/cache"),
        CargoHomeClass::RegistryCache
    );
    assert_eq!(
        classify_cargo_home_relative_path("registry/src"),
        CargoHomeClass::RegistrySource
    );
    assert_eq!(
        classify_cargo_home_relative_path("git/db"),
        CargoHomeClass::GitDatabase
    );
    assert_eq!(
        classify_cargo_home_relative_path("git/checkouts"),
        CargoHomeClass::GitCheckouts
    );
}

#[test]
fn cargo_home_classifier_preserves_config_credentials_and_unknowns() {
    assert_eq!(
        classify_cargo_home_relative_path("config.toml"),
        CargoHomeClass::Config
    );
    assert_eq!(
        classify_cargo_home_relative_path("credentials.toml"),
        CargoHomeClass::Credentials
    );
    assert_eq!(
        classify_cargo_home_relative_path(".crates.toml"),
        CargoHomeClass::InstallMetadata
    );
    assert_eq!(
        classify_cargo_home_relative_path("bin"),
        CargoHomeClass::InstalledBinaries
    );
    assert_eq!(
        classify_cargo_home_relative_path("custom"),
        CargoHomeClass::UnknownUserAuthored
    );
}

#[test]
fn cargo_home_report_inventory_is_read_only_and_sorted() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_report")?;
    fs::create_dir_all(temp.path().join("registry/cache/example"))?;
    fs::create_dir_all(temp.path().join("registry/src/example"))?;
    fs::create_dir_all(temp.path().join("git/db/repo"))?;
    fs::create_dir_all(temp.path().join("bin"))?;
    fs::write(temp.path().join("registry/cache/example/pkg.crate"), b"abc")?;
    fs::write(temp.path().join("registry/src/example/lib.rs"), b"source")?;
    fs::write(temp.path().join("git/db/repo/config"), b"git")?;
    fs::write(temp.path().join("config.toml"), b"[net]\n")?;
    fs::write(temp.path().join("credentials.toml"), b"token")?;
    fs::write(temp.path().join("custom.txt"), b"user")?;

    let report = build_cargo_home_report(CargoHomeReportRequest {
        cargo_home: Some(temp.path().to_path_buf()),
    })?;

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.input.source, CargoHomeSource::Explicit);
    assert_eq!(report.totals.known_cache_entry_count, 3);
    assert_eq!(report.totals.cache_bytes, 12);
    assert_eq!(report.totals.preserved_bytes, report.totals.total_bytes);
    assert!(report.problems.is_empty());
    assert!(
        report.recommendations[0]
            .message
            .contains("cache.auto-clean-frequency")
    );

    let relative_paths = report
        .entries
        .iter()
        .map(|entry| entry.relative_path.display().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        relative_paths,
        vec![
            "bin",
            "config.toml",
            "credentials.toml",
            "custom.txt",
            "git/db",
            "registry/cache",
            "registry/src"
        ]
    );
    assert!(report.entries.iter().all(|entry| entry.preserved));
    Ok(())
}

#[test]
fn cargo_home_report_errors_when_root_is_missing() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("cargo_home_missing")?;
    let missing = temp.path().join("missing");

    let error = build_cargo_home_report(CargoHomeReportRequest {
        cargo_home: Some(missing.clone()),
    })
    .expect_err("missing root should fail");

    assert!(error.to_string().contains(&missing.display().to_string()));
    Ok(())
}

#[test]
#[cfg(unix)]
fn cargo_home_report_skips_symlinks() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = TestTemp::new("cargo_home_symlink")?;
    fs::write(temp.path().join("target-file"), b"abc")?;
    symlink(temp.path().join("target-file"), temp.path().join("bin"))?;

    let report = build_cargo_home_report(CargoHomeReportRequest {
        cargo_home: Some(temp.path().to_path_buf()),
    })?;

    let entry = report
        .entries
        .iter()
        .find(|entry| entry.relative_path == Path::new("bin"))
        .expect("bin entry");
    assert!(entry.skipped);
    assert_eq!(entry.size_bytes, 0);
    assert!(entry.reason.contains("symlink"));
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
