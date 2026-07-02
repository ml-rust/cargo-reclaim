use std::path::{Path, PathBuf};

use crate::error::ReclaimResult;
use crate::scanner::{
    CargoConfigProblem, CargoConfigUnsupported, TargetDirOverride, resolve_project_output_dirs,
};

pub const CARGO_CONFIG_RECOMMEND_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigRecommendRequest {
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
            key: "build.build-dir".to_string(),
            current: None,
            recommended: Some("target/build".to_string()),
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

fn normalize_project_path(project: &Path) -> PathBuf {
    std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf())
}
