use std::path::Path;

use cargo_reclaim::{
    CargoHomeClass, CargoHomePathKind, CargoHomePlanAction, CargoHomeSource, PolicyKind,
};

pub(super) fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

pub(super) fn action_label(action: CargoHomePlanAction) -> &'static str {
    match action {
        CargoHomePlanAction::DeleteCandidate => "delete_candidate",
        CargoHomePlanAction::Preserve => "preserve",
        CargoHomePlanAction::SkipProblem => "skip_problem",
    }
}

pub(super) fn source_label(source: CargoHomeSource) -> &'static str {
    match source {
        CargoHomeSource::Explicit => "explicit",
        CargoHomeSource::CargoHomeEnv => "cargo_home_env",
        CargoHomeSource::HomeDefault => "home_default",
    }
}

pub(super) fn class_label(class: CargoHomeClass) -> &'static str {
    match class {
        CargoHomeClass::RegistryIndex => "registry_index",
        CargoHomeClass::RegistryCache => "registry_cache",
        CargoHomeClass::RegistrySource => "registry_source",
        CargoHomeClass::GitDatabase => "git_database",
        CargoHomeClass::GitCheckouts => "git_checkouts",
        CargoHomeClass::Config => "config",
        CargoHomeClass::Credentials => "credentials",
        CargoHomeClass::InstalledBinaries => "installed_binaries",
        CargoHomeClass::InstallMetadata => "install_metadata",
        CargoHomeClass::UnknownUserAuthored => "unknown_user_authored",
    }
}

pub(super) fn path_kind_label(kind: CargoHomePathKind) -> &'static str {
    match kind {
        CargoHomePathKind::File => "file",
        CargoHomePathKind::Directory => "directory",
        CargoHomePathKind::Symlink => "symlink",
        CargoHomePathKind::Other => "other",
    }
}

pub(super) fn display_path(path: &Path) -> String {
    display_text(&path.display().to_string())
}

pub(super) fn path_string(path: impl AsRef<Path>) -> String {
    path.as_ref().display().to_string()
}

pub(super) fn display_text(value: &str) -> String {
    value.escape_default().to_string()
}
