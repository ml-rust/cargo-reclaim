use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{ReclaimError, ReclaimResult};
use crate::scanner::{
    CargoConfigProblem, CargoConfigUnsupported, TargetDirOverride, resolve_project_output_dirs,
};

pub const CARGO_CONFIG_RECOMMEND_SCHEMA_VERSION: u16 = 1;
pub const CARGO_CONFIG_PREVIEW_SCHEMA_VERSION: u16 = 1;
pub const CARGO_CONFIG_APPLY_SCHEMA_VERSION: u16 = 1;

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
pub struct CargoConfigApplyRequest {
    pub preview_path: PathBuf,
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
pub struct CargoConfigApplyReport {
    pub schema_version: u16,
    pub preview_path: PathBuf,
    pub target_config_file: PathBuf,
    pub applied: bool,
    pub modified_cargo_config_files: bool,
    pub operations: Vec<CargoConfigPreviewOperation>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

pub fn apply_cargo_config_preview(
    request: CargoConfigApplyRequest,
) -> ReclaimResult<CargoConfigApplyReport> {
    let preview_path = request.preview_path;
    let preview = read_preview_document(&preview_path)?;
    let target_config_file = PathBuf::from(&preview.target_config_file);

    if let Some(reason) = validate_preview_document(&preview) {
        return Ok(refused_apply_report(
            preview_path,
            target_config_file,
            reason,
        ));
    }

    let snapshot = snapshot_config_file(&target_config_file)?;
    if snapshot != preview.target_config_snapshot {
        return Ok(refused_apply_report(
            preview_path,
            target_config_file,
            "Refused to apply because the target Cargo config file no longer matches the preview snapshot.".to_string(),
        ));
    }

    let mut document = match read_config_document(&target_config_file, snapshot.exists)? {
        ReadConfigDocument::Ready(document) => document,
        ReadConfigDocument::Refused(reason) => {
            return Ok(refused_apply_report(
                preview_path,
                target_config_file,
                reason,
            ));
        }
    };

    if cargo_build_dir_exists(&document) {
        return Ok(refused_apply_report(
            preview_path,
            target_config_file,
            "Refused to apply because build.build-dir is already configured in the target Cargo config file.".to_string(),
        ));
    }
    if !cargo_build_table_is_supported(&document) {
        return Ok(refused_apply_report(
            preview_path,
            target_config_file,
            "Refused to apply because build is not a table in the target Cargo config file."
                .to_string(),
        ));
    }

    insert_cargo_build_dir(&mut document);
    write_config_document(&target_config_file, &document)?;

    let written = read_config_document(&target_config_file, true)?;
    let ReadConfigDocument::Ready(written) = written else {
        return Err(ReclaimError::InventoryRead {
            path: target_config_file,
            message: "written Cargo config file could not be verified".to_string(),
        });
    };
    if !cargo_build_dir_exists(&written) {
        return Err(ReclaimError::InventoryRead {
            path: target_config_file,
            message: "build.build-dir was not present after writing the Cargo config file"
                .to_string(),
        });
    }

    Ok(CargoConfigApplyReport {
        schema_version: CARGO_CONFIG_APPLY_SCHEMA_VERSION,
        preview_path,
        target_config_file,
        applied: true,
        modified_cargo_config_files: true,
        operations: vec![CargoConfigPreviewOperation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: None,
            recommended: Some(RECOMMENDED_BUILD_DIR.to_string()),
            status: CargoConfigPreviewOperationStatus::Insert,
            reason:
                "Applied build.build-dir = \"target/build\" to the project-local Cargo config file."
                    .to_string(),
        }],
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

fn read_preview_document(path: &Path) -> ReclaimResult<JsonCargoConfigPreviewReport> {
    let contents = std::fs::read(path).map_err(|error| ReclaimError::InventoryRead {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    serde_json::from_slice(&contents).map_err(|error| ReclaimError::InventoryRead {
        path: path.to_path_buf(),
        message: format!("failed to parse Cargo config preview JSON: {error}"),
    })
}

fn validate_preview_document(preview: &JsonCargoConfigPreviewReport) -> Option<String> {
    if preview.schema_version != CARGO_CONFIG_PREVIEW_SCHEMA_VERSION {
        return Some(
            "Refused to apply because the Cargo config preview schema version is unsupported."
                .to_string(),
        );
    }
    if preview.command != "cargo-config preview" {
        return Some(
            "Refused to apply because the preview command is not cargo-config preview.".to_string(),
        );
    }
    if !preview.dry_run {
        return Some(
            "Refused to apply because the preview document is not marked as a dry run.".to_string(),
        );
    }
    if preview.modified_cargo_config_files {
        return Some(
            "Refused to apply because the preview document reports modified Cargo config files."
                .to_string(),
        );
    }
    if !is_project_local_cargo_config_path(Path::new(&preview.target_config_file)) {
        return Some(
            "Refused to apply because the preview target is not .cargo/config.toml.".to_string(),
        );
    }
    if !preview.unsupported.is_empty() {
        return Some("Refused to apply because the preview document contains unsupported Cargo config settings.".to_string());
    }
    if !preview.problems.is_empty() {
        return Some(
            "Refused to apply because the preview document contains Cargo config problems."
                .to_string(),
        );
    }
    let Some(operation) = preview.operations.first() else {
        return Some("Refused to apply because the preview document has no operation.".to_string());
    };
    if preview.operations.len() != 1 {
        return Some(
            "Refused to apply because the preview document must contain exactly one operation."
                .to_string(),
        );
    }
    if operation.key != CARGO_CONFIG_BUILD_DIR_KEY
        || operation.current.is_some()
        || operation.recommended.as_deref() != Some(RECOMMENDED_BUILD_DIR)
        || operation.status != "insert"
    {
        return Some("Refused to apply because the preview operation is not the supported build.build-dir insert.".to_string());
    }
    if preview.target_config_snapshot.exists {
        if preview.target_config_snapshot.hash.is_none()
            || preview.target_config_snapshot.size_bytes.is_none()
        {
            return Some(
                "Refused to apply because the preview target snapshot is incomplete.".to_string(),
            );
        }
    } else if preview.target_config_snapshot.hash.is_some()
        || preview.target_config_snapshot.size_bytes.is_some()
    {
        return Some(
            "Refused to apply because the absent preview target snapshot has file metadata."
                .to_string(),
        );
    }
    None
}

enum ReadConfigDocument {
    Ready(DocumentMut),
    Refused(String),
}

fn read_config_document(path: &Path, exists: bool) -> ReclaimResult<ReadConfigDocument> {
    if !exists {
        return Ok(ReadConfigDocument::Ready(DocumentMut::new()));
    }

    let contents = std::fs::read_to_string(path).map_err(|error| ReclaimError::InventoryRead {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    match contents.parse::<DocumentMut>() {
        Ok(document) => Ok(ReadConfigDocument::Ready(document)),
        Err(error) => Ok(ReadConfigDocument::Refused(format!(
            "Refused to apply because the target Cargo config file could not be parsed as TOML: {error}."
        ))),
    }
}

fn cargo_build_dir_exists(document: &DocumentMut) -> bool {
    document
        .get("build")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("build-dir"))
        .is_some()
}

fn cargo_build_table_is_supported(document: &DocumentMut) -> bool {
    document
        .get("build")
        .is_none_or(|item| item.as_table_like().is_some())
}

fn is_project_local_cargo_config_path(path: &Path) -> bool {
    path.is_absolute()
        && path.file_name().and_then(|value| value.to_str()) == Some("config.toml")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            == Some(".cargo")
}

fn insert_cargo_build_dir(document: &mut DocumentMut) {
    if !document.as_table().contains_key("build") {
        document["build"] = Item::Table(Table::new());
    }
    if let Some(build) = document.get_mut("build").and_then(Item::as_table_like_mut) {
        build.insert("build-dir", value(RECOMMENDED_BUILD_DIR));
    }
}

fn write_config_document(path: &Path, document: &DocumentMut) -> ReclaimResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ReclaimError::InventoryRead {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }

    let temp_path = temp_sibling_path(path);
    std::fs::write(&temp_path, document.to_string()).map_err(|error| {
        ReclaimError::InventoryRead {
            path: temp_path.clone(),
            message: error.to_string(),
        }
    })?;
    std::fs::rename(&temp_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        ReclaimError::InventoryRead {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config.toml");
    path.with_file_name(format!(
        ".{file_name}.cargo-reclaim-{}.tmp",
        std::process::id()
    ))
}

fn refused_apply_report(
    preview_path: PathBuf,
    target_config_file: PathBuf,
    reason: String,
) -> CargoConfigApplyReport {
    CargoConfigApplyReport {
        schema_version: CARGO_CONFIG_APPLY_SCHEMA_VERSION,
        preview_path,
        target_config_file,
        applied: false,
        modified_cargo_config_files: false,
        operations: vec![CargoConfigPreviewOperation {
            key: CARGO_CONFIG_BUILD_DIR_KEY.to_string(),
            current: None,
            recommended: None,
            status: CargoConfigPreviewOperationStatus::Refused,
            reason,
        }],
    }
}

#[derive(Debug, Deserialize)]
struct JsonCargoConfigPreviewReport {
    schema_version: u16,
    command: String,
    dry_run: bool,
    modified_cargo_config_files: bool,
    target_config_file: String,
    target_config_snapshot: CargoConfigFileSnapshot,
    operations: Vec<JsonCargoConfigPreviewOperation>,
    unsupported: Vec<serde_json::Value>,
    problems: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonCargoConfigPreviewOperation {
    key: String,
    current: Option<String>,
    recommended: Option<String>,
    status: String,
}
