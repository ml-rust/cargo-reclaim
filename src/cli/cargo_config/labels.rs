use std::path::Path;

use cargo_reclaim::CargoConfigUnsupportedReason;
use cargo_reclaim::config::CargoConfigPreviewOperationStatus;

pub(super) fn unsupported_reason_label(reason: &CargoConfigUnsupportedReason) -> &'static str {
    match reason {
        CargoConfigUnsupportedReason::WorkspacePathHashTemplate => "workspace_path_hash_template",
    }
}

pub(super) fn preview_operation_status_label(
    status: CargoConfigPreviewOperationStatus,
) -> &'static str {
    match status {
        CargoConfigPreviewOperationStatus::Insert => "insert",
        CargoConfigPreviewOperationStatus::Unsupported => "unsupported",
        CargoConfigPreviewOperationStatus::Refused => "refused",
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
