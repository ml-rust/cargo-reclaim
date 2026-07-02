use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{ReclaimError, ReclaimResult};
use crate::scanner::{
    CargoConfigProblem, CargoConfigUnsupported, TargetDirOverride, resolve_project_output_dirs,
};

pub const CARGO_CONFIG_RECOMMEND_SCHEMA_VERSION: u16 = 1;
pub const CARGO_CONFIG_PREVIEW_SCHEMA_VERSION: u16 = 1;

const CARGO_CONFIG_BUILD_DIR_KEY: &str = "build.build-dir";
const RECOMMENDED_BUILD_DIR: &str = "target/build";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigRecommendRequest {
    pub project: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigPreviewRequest {
    pub project: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigRecommendReport {
    pub schema_version: u16,
    pub project: PathBuf,
    pub target_dirs: Vec<CargoConfigOutputDir>,
    pub build_dirs: Vec<CargoConfigOutputDir>,
    pub recommendations: Vec<CargoConfigRecommendation>,
    pub unsupported: Vec<CargoConfigUnsupported>,
    pub problems: Vec<CargoConfigProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigPreviewReport {
    pub schema_version: u16,
    pub project: PathBuf,
    pub target_config_file: PathBuf,
    pub target_config_snapshot: CargoConfigFileSnapshot,
    pub dry_run: bool,
    pub modified_cargo_config_files: bool,
    pub operations: Vec<CargoConfigPreviewOperation>,
    pub unsupported: Vec<CargoConfigUnsupported>,
    pub problems: Vec<CargoConfigProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigOutputDir {
    pub path: PathBuf,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigRecommendation {
    pub key: String,
    pub current: Option<String>,
    pub recommended: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigFileSnapshot {
    pub exists: bool,
    pub hash: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigPreviewOperation {
    pub key: String,
    pub current: Option<String>,
    pub recommended: Option<String>,
    pub status: CargoConfigPreviewOperationStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoConfigPreviewOperationStatus {
    Insert,
    Unsupported,
    Refused,
}

pub fn build_cargo_config_recommend_report(
    request: CargoConfigRecommendRequest,
) -> ReclaimResult<CargoConfigRecommendReport> {
    let project = request.project;
    let resolved = resolve_project_output_dirs(&project)?;
    let mut target_dirs = Vec::with_capacity(resolved.dirs.len());
    let mut build_dirs = Vec::with_capacity(resolved.dirs.len());

    for dir in resolved.dirs {
        let output_dir = output_dir_from_override(dir);
        if output_dir.1 {
            build_dirs.push(output_dir.0);
        } else {
            target_dirs.push(output_dir.0);
        }
    }

    let recommendations = if build_dirs.is_empty() {
        vec![CargoConfigRecommendation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: None,
            recommended: Some(RECOMMENDED_BUILD_DIR.to_string()),
            reason: "Keeps Cargo intermediate build output separate from target-dir artifacts so cleanup policy can reason about each output class independently.".to_string(),
        }]
    } else {
        Vec::new()
    };

    Ok(CargoConfigRecommendReport {
        schema_version: CARGO_CONFIG_RECOMMEND_SCHEMA_VERSION,
        project: normalize_project_path(&project),
        target_dirs,
        build_dirs,
        recommendations,
        unsupported: resolved.unsupported,
        problems: resolved.problems,
    })
}

pub fn build_cargo_config_preview_report(
    request: CargoConfigPreviewRequest,
) -> ReclaimResult<CargoConfigPreviewReport> {
    let project = normalize_project_path(&request.project);
    let target_config_file = project.join(".cargo").join("config.toml");
    let target_config_snapshot = snapshot_config_file(&target_config_file)?;
    let resolved = resolve_project_output_dirs(&request.project)?;
    let build_dir = resolved.dirs.into_iter().find_map(|dir| {
        let (output_dir, is_build_dir) = output_dir_from_override(dir);
        is_build_dir.then_some(output_dir)
    });

    let operation = preview_operation(
        build_dir.as_ref(),
        &target_config_snapshot,
        &resolved.unsupported,
        &resolved.problems,
    );

    Ok(CargoConfigPreviewReport {
        schema_version: CARGO_CONFIG_PREVIEW_SCHEMA_VERSION,
        project,
        target_config_file,
        target_config_snapshot,
        dry_run: true,
        modified_cargo_config_files: false,
        operations: vec![operation],
        unsupported: resolved.unsupported,
        problems: resolved.problems,
    })
}

fn output_dir_from_override(dir: TargetDirOverride) -> (CargoConfigOutputDir, bool) {
    let is_build_dir = dir.is_build_dir();
    (
        CargoConfigOutputDir {
            path: dir.path,
            source: dir.source.label,
        },
        is_build_dir,
    )
}

fn preview_operation(
    build_dir: Option<&CargoConfigOutputDir>,
    target_config_snapshot: &CargoConfigFileSnapshot,
    unsupported: &[CargoConfigUnsupported],
    problems: &[CargoConfigProblem],
) -> CargoConfigPreviewOperation {
    if let Some(build_dir) = build_dir {
        return CargoConfigPreviewOperation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: Some(build_dir.path.display().to_string()),
            recommended: None,
            status: CargoConfigPreviewOperationStatus::Refused,
            reason: format!(
                "Refused to plan a Cargo config write because an active build.build-dir is already configured by {}.",
                build_dir.source
            ),
        };
    }

    if !unsupported.is_empty() {
        return CargoConfigPreviewOperation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: None,
            recommended: None,
            status: CargoConfigPreviewOperationStatus::Unsupported,
            reason: "Cargo config resolution reported unsupported build.build-dir settings, so selecting a write target would be unsafe.".to_string(),
        };
    }

    if !problems.is_empty() {
        return CargoConfigPreviewOperation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: None,
            recommended: None,
            status: CargoConfigPreviewOperationStatus::Unsupported,
            reason: "Cargo config resolution reported problems, so selecting a write target would be unsafe.".to_string(),
        };
    }

    let reason = if target_config_snapshot.exists {
        "Would insert build.build-dir = \"target/build\" into the project-local Cargo config file; dry-run only, no files were modified."
    } else {
        "Would create the project-local Cargo config file and insert build.build-dir = \"target/build\"; dry-run only, no files were modified."
    };

    CargoConfigPreviewOperation {
        key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
        current: None,
        recommended: Some(RECOMMENDED_BUILD_DIR.to_string()),
        status: CargoConfigPreviewOperationStatus::Insert,
        reason: reason.to_string(),
    }
}

fn snapshot_config_file(path: &Path) -> ReclaimResult<CargoConfigFileSnapshot> {
    match std::fs::read(path) {
        Ok(contents) => {
            let digest = Sha256::digest(&contents);
            Ok(CargoConfigFileSnapshot {
                exists: true,
                hash: Some(format!("sha256:{digest:x}")),
                size_bytes: Some(contents.len() as u64),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CargoConfigFileSnapshot {
            exists: false,
            hash: None,
            size_bytes: None,
        }),
        Err(error) => Err(ReclaimError::InventoryRead {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

fn normalize_project_path(project: &Path) -> PathBuf {
    std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf())
}
