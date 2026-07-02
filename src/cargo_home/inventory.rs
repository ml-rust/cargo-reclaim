use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::classify::classify_cargo_home_relative_path;
use super::measure::measure_existing_path;
use super::model::{
    CargoHomeClass, CargoHomeEntry, CargoHomeError, CargoHomeInput, CargoHomeProblem,
};

const KNOWN_NESTED_PATHS: [&str; 5] = [
    "registry/index",
    "registry/cache",
    "registry/src",
    "git/db",
    "git/checkouts",
];

pub fn inventory_cargo_home(
    input: CargoHomeInput,
) -> Result<(Vec<CargoHomeEntry>, Vec<CargoHomeProblem>), CargoHomeError> {
    validate_root(&input.root)?;
    let mut relative_paths = known_existing_paths(&input.root);
    relative_paths.extend(unknown_top_level_paths(&input.root)?);
    relative_paths.sort();
    relative_paths.dedup();

    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for relative_path in relative_paths {
        let path = input.root.join(&relative_path);
        match entry_for_path(&path, &relative_path) {
            Ok((entry, entry_problems)) => {
                problems.extend(entry_problems);
                entries.push(entry);
            }
            Err(error) => {
                problems.push(CargoHomeProblem {
                    path: path.clone(),
                    message: error.to_string(),
                });
                entries.push(CargoHomeEntry {
                    path,
                    relative_path: relative_path.clone(),
                    class: classify_cargo_home_relative_path(&relative_path),
                    path_kind: super::model::CargoHomePathKind::Other,
                    size_bytes: 0,
                    preserved: true,
                    skipped: true,
                    reason: "preserved; path could not be inspected".to_string(),
                });
            }
        }
    }

    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok((entries, problems))
}

fn validate_root(root: &Path) -> Result<(), CargoHomeError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => CargoHomeError::RootMissing {
            path: root.to_path_buf(),
        },
        _ => CargoHomeError::RootUnreadable {
            path: root.to_path_buf(),
            message: error.to_string(),
        },
    })?;
    if !metadata.is_dir() {
        return Err(CargoHomeError::RootNotDirectory {
            path: root.to_path_buf(),
        });
    }
    fs::read_dir(root).map_err(|error| CargoHomeError::RootUnreadable {
        path: root.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(())
}

fn known_existing_paths(root: &Path) -> Vec<PathBuf> {
    KNOWN_NESTED_PATHS
        .iter()
        .map(PathBuf::from)
        .filter(|relative_path| root.join(relative_path).exists())
        .collect()
}

fn unknown_top_level_paths(root: &Path) -> Result<Vec<PathBuf>, CargoHomeError> {
    let children = fs::read_dir(root).map_err(|error| CargoHomeError::RootUnreadable {
        path: root.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut paths = Vec::new();
    for child in children {
        let child = child.map_err(|error| CargoHomeError::RootUnreadable {
            path: root.to_path_buf(),
            message: error.to_string(),
        })?;
        let relative_path = PathBuf::from(child.file_name());
        if should_report_top_level(&relative_path) {
            paths.push(relative_path);
        }
    }
    Ok(paths)
}

fn should_report_top_level(relative_path: &Path) -> bool {
    !matches!(relative_path.to_str(), Some("registry") | Some("git"))
        || !matches!(
            classify_cargo_home_relative_path(relative_path),
            CargoHomeClass::UnknownUserAuthored
        )
}

fn entry_for_path(
    path: &Path,
    relative_path: &Path,
) -> io::Result<(CargoHomeEntry, Vec<CargoHomeProblem>)> {
    let measured = measure_existing_path(path)?;
    let class = classify_cargo_home_relative_path(relative_path);
    let reason = if measured.skipped {
        measured.reason
    } else {
        reason_for_class(class).to_string()
    };
    Ok((
        CargoHomeEntry {
            path: path.to_path_buf(),
            relative_path: relative_path.to_path_buf(),
            class,
            path_kind: measured.kind,
            size_bytes: measured.size_bytes,
            preserved: true,
            skipped: measured.skipped,
            reason,
        },
        measured.problems,
    ))
}

fn reason_for_class(class: CargoHomeClass) -> &'static str {
    match class {
        CargoHomeClass::RegistryIndex
        | CargoHomeClass::RegistryCache
        | CargoHomeClass::RegistrySource
        | CargoHomeClass::GitDatabase
        | CargoHomeClass::GitCheckouts => "preserved; known Cargo global cache class",
        CargoHomeClass::Config => "preserved; Cargo configuration",
        CargoHomeClass::Credentials => "preserved; Cargo credentials",
        CargoHomeClass::InstalledBinaries => "preserved; installed Cargo binaries",
        CargoHomeClass::InstallMetadata => "preserved; Cargo install metadata",
        CargoHomeClass::UnknownUserAuthored => "preserved; unrecognized or user-authored path",
    }
}
