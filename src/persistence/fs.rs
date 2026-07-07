use std::fs;
use std::path::{Path, PathBuf};

use super::document::PersistedPlan;
use super::document_kind::deserialize_plan_document;
use super::error::{PlanPersistenceError, PlanPersistenceResult};

pub fn save_plan_to_path(
    path: impl AsRef<Path>,
    document: &PersistedPlan,
) -> PlanPersistenceResult<()> {
    let path = path.as_ref();
    let temp_path = temp_sibling_path(path);
    let bytes = serde_json::to_vec_pretty(document)?;

    fs::write(&temp_path, bytes).map_err(|error| io_error(&temp_path, error))?;
    fs::rename(&temp_path, path).map_err(|error| io_error(path, error))?;
    Ok(())
}

pub fn load_plan_from_path(path: impl AsRef<Path>) -> PlanPersistenceResult<PersistedPlan> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|error| io_error(path, error))?;
    deserialize_plan_document(path, &bytes, "plan")
}

fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|file_name| file_name.to_string_lossy().to_string())
        .unwrap_or_else(|| "plan".to_string());
    path.with_file_name(format!(".{file_name}.tmp"))
}

fn io_error(path: &Path, error: std::io::Error) -> PlanPersistenceError {
    PlanPersistenceError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
