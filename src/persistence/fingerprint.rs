use std::fs::{self, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{PersistedTimestamp, PlanPersistenceError, PlanPersistenceResult};

pub(crate) fn fingerprint_path(path: &Path, metadata: &Metadata) -> PlanPersistenceResult<String> {
    let mut hasher = Sha256::new();
    fingerprint_path_into(path, Path::new(""), metadata, &mut hasher)?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn fingerprint_path_into(
    path: &Path,
    relative_path: &Path,
    metadata: &Metadata,
    hasher: &mut Sha256,
) -> PlanPersistenceResult<()> {
    let kind = if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_file() {
        "file"
    } else if metadata.is_dir() {
        "directory"
    } else {
        "other"
    };
    hasher.update(relative_path.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    hasher.update([0]);
    if let Ok(modified) = metadata.modified()
        && let Ok(timestamp) = PersistedTimestamp::from_system_time(modified)
    {
        hasher.update(timestamp.unix_seconds.to_le_bytes());
        hasher.update(timestamp.nanoseconds.to_le_bytes());
    }
    hasher.update([0]);

    if metadata.is_file() {
        let mut file = fs::File::open(path).map_err(|error| io_error(path, error))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| io_error(path, error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hasher.update([0]);
    }

    if metadata.is_dir() {
        let mut children = Vec::new();
        for child in fs::read_dir(path).map_err(|error| io_error(path, error))? {
            let child = child.map_err(|error| io_error(path, error))?;
            children.push(child.path());
        }
        children.sort();
        for child_path in children {
            let child_name =
                child_path
                    .file_name()
                    .ok_or_else(|| PlanPersistenceError::InvalidPlan {
                        message: format!("failed to read child path under {}", path.display()),
                    })?;
            let child_relative_path = if relative_path.as_os_str().is_empty() {
                PathBuf::from(child_name)
            } else {
                relative_path.join(child_name)
            };
            let child_metadata =
                fs::symlink_metadata(&child_path).map_err(|error| io_error(&child_path, error))?;
            fingerprint_path_into(&child_path, &child_relative_path, &child_metadata, hasher)?;
        }
    }

    Ok(())
}

fn io_error(path: &Path, error: std::io::Error) -> PlanPersistenceError {
    PlanPersistenceError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
