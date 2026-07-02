use std::path::{Component, Path};

use super::model::CargoHomeClass;

pub fn classify_cargo_home_relative_path(path: impl AsRef<Path>) -> CargoHomeClass {
    let parts = normalized_parts(path.as_ref());
    match parts.as_slice() {
        ["registry", "index"] => CargoHomeClass::RegistryIndex,
        ["registry", "cache"] => CargoHomeClass::RegistryCache,
        ["registry", "src"] => CargoHomeClass::RegistrySource,
        ["git", "db"] => CargoHomeClass::GitDatabase,
        ["git", "checkouts"] => CargoHomeClass::GitCheckouts,
        ["config.toml"] | ["config"] => CargoHomeClass::Config,
        ["credentials.toml"] | ["credentials"] => CargoHomeClass::Credentials,
        [".crates.toml"] | [".crates2.json"] => CargoHomeClass::InstallMetadata,
        ["bin"] => CargoHomeClass::InstalledBinaries,
        _ => CargoHomeClass::UnknownUserAuthored,
    }
}

fn normalized_parts(path: &Path) -> Vec<&str> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_cargo_home_layout_paths() {
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
    fn preserves_sensitive_and_user_authored_paths() {
        assert_eq!(
            classify_cargo_home_relative_path("config.toml"),
            CargoHomeClass::Config
        );
        assert_eq!(
            classify_cargo_home_relative_path("credentials"),
            CargoHomeClass::Credentials
        );
        assert_eq!(
            classify_cargo_home_relative_path(".crates2.json"),
            CargoHomeClass::InstallMetadata
        );
        assert_eq!(
            classify_cargo_home_relative_path("bin"),
            CargoHomeClass::InstalledBinaries
        );
        assert_eq!(
            classify_cargo_home_relative_path("registry"),
            CargoHomeClass::UnknownUserAuthored
        );
        assert_eq!(
            classify_cargo_home_relative_path("custom"),
            CargoHomeClass::UnknownUserAuthored
        );
    }
}
