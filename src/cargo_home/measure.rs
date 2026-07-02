use std::fs;
use std::io;
use std::path::Path;

use super::model::{CargoHomePathKind, CargoHomeProblem};

pub(super) struct MeasuredPath {
    pub kind: CargoHomePathKind,
    pub size_bytes: u64,
    pub skipped: bool,
    pub reason: String,
    pub problems: Vec<CargoHomeProblem>,
}

pub(super) fn measure_existing_path(path: &Path) -> io::Result<MeasuredPath> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(MeasuredPath {
            kind: CargoHomePathKind::Symlink,
            size_bytes: 0,
            skipped: true,
            reason: "preserved; symlink was not followed".to_string(),
            problems: Vec::new(),
        });
    }

    if file_type.is_file() {
        return Ok(MeasuredPath {
            kind: CargoHomePathKind::File,
            size_bytes: metadata.len(),
            skipped: false,
            reason: "preserved; Cargo home report is read-only".to_string(),
            problems: Vec::new(),
        });
    }

    if file_type.is_dir() {
        let mut problems = Vec::new();
        let size_bytes = measure_directory(path, &mut problems);
        let skipped = !problems.is_empty();
        let reason = if skipped {
            "preserved; one or more child paths could not be read".to_string()
        } else {
            "preserved; Cargo home report is read-only".to_string()
        };
        return Ok(MeasuredPath {
            kind: CargoHomePathKind::Directory,
            size_bytes,
            skipped,
            reason,
            problems,
        });
    }

    Ok(MeasuredPath {
        kind: CargoHomePathKind::Other,
        size_bytes: metadata.len(),
        skipped: false,
        reason: "preserved; unrecognized filesystem entry".to_string(),
        problems: Vec::new(),
    })
}

fn measure_directory(path: &Path, problems: &mut Vec<CargoHomeProblem>) -> u64 {
    let children = match fs::read_dir(path) {
        Ok(children) => children,
        Err(error) => {
            problems.push(CargoHomeProblem {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return 0;
        }
    };

    let mut total = 0u64;
    for child in children {
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                problems.push(CargoHomeProblem {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        total = total.saturating_add(measure_child(&child.path(), problems));
    }
    total
}

fn measure_child(path: &Path, problems: &mut Vec<CargoHomeProblem>) -> u64 {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            problems.push(CargoHomeProblem {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return 0;
        }
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return 0;
    }
    if file_type.is_dir() {
        return measure_directory(path, problems);
    }
    metadata.len()
}
