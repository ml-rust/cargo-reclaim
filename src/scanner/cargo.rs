use std::path::{Path, PathBuf};

use super::foundation::CargoProject;

const CARGO_MANIFEST: &str = "Cargo.toml";

pub fn detect_cargo_project(path: impl AsRef<Path>) -> Option<CargoProject> {
    let path = path.as_ref();

    if path.is_dir() {
        let manifest_path = path.join(CARGO_MANIFEST);
        return manifest_path
            .is_file()
            .then(|| CargoProject::new(manifest_path, path.to_path_buf()));
    }

    if path.is_file() && path.file_name().is_some_and(|name| name == CARGO_MANIFEST) {
        let root_path = parent_or_current(path);
        return Some(CargoProject::new(path.to_path_buf(), root_path));
    }

    None
}

fn parent_or_current(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}
