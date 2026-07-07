use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::error::{PlanPersistenceError, PlanPersistenceResult};

/// Coarse classification of a plan-shaped JSON document, used to turn a failed
/// load into an actionable diagnostic instead of a raw serde error.
///
/// The tool emits two documents that both look like "plan JSON": the executable
/// plan written by `--save-plan` (carries a top-level `id`) and the lossy report
/// written by `--json` (carries `dry_run`/`command` and no `id`). This is only
/// consulted when a concrete deserialize fails, so classification never runs on
/// the happy path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDocumentKind {
    /// A `--save-plan` document: has the top-level `id` that `apply` revalidates against.
    ExecutablePlan,
    /// A `--json` report: lacks the metadata required to execute a plan. Carries the
    /// document's own `command` (e.g. `plan`, `cargo-home plan`) when present, so the
    /// diagnostic names the command that actually produced the file.
    DryRunReport { source_command: Option<String> },
    /// Neither format — not a recognizable cargo-reclaim plan document.
    Unrecognized,
}

/// Fields that discriminate the two plan-shaped documents. Unknown fields are
/// ignored, so this probe succeeds on any JSON object regardless of the rest of
/// its shape.
#[derive(Deserialize)]
struct DocumentProbe {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    command: Option<String>,
}

/// Deserialize a persisted plan document, falling back to a format-aware
/// diagnostic when the concrete parse fails. The common success path is a single
/// parse; the classifier only runs to explain a failure. `expected_command` is
/// the command whose `--save-plan` output this loader consumes (e.g. `plan`).
pub(crate) fn deserialize_plan_document<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
    expected_command: &str,
) -> PlanPersistenceResult<T> {
    match serde_json::from_slice::<T>(bytes) {
        Ok(document) => Ok(document),
        Err(decode_error) => Err(diagnose_load_failure(
            path,
            bytes,
            expected_command,
            decode_error,
        )),
    }
}

/// Explain why a concrete deserialize failed: a dry-run report supplied where an
/// executable plan is required, an unrecognized document, or a genuinely corrupt
/// plan (looked executable but would not parse).
fn diagnose_load_failure(
    path: &Path,
    bytes: &[u8],
    expected_command: &str,
    decode_error: serde_json::Error,
) -> PlanPersistenceError {
    match classify_plan_document(bytes) {
        PlanDocumentKind::DryRunReport { source_command } => PlanPersistenceError::DryRunReport {
            path: path.to_path_buf(),
            source_command: source_command.unwrap_or_else(|| expected_command.to_string()),
            expected_command: expected_command.to_string(),
        },
        PlanDocumentKind::Unrecognized => PlanPersistenceError::UnrecognizedDocument {
            path: path.to_path_buf(),
        },
        PlanDocumentKind::ExecutablePlan => PlanPersistenceError::Decode {
            path: path.to_path_buf(),
            message: decode_error.to_string(),
        },
    }
}

/// Classify a plan-shaped document by the fields that are present. Both the
/// top-level plan and cargo-home plan documents share the same discriminators,
/// so a single classifier serves every loader.
pub(crate) fn classify_plan_document(bytes: &[u8]) -> PlanDocumentKind {
    let Ok(probe) = serde_json::from_slice::<DocumentProbe>(bytes) else {
        return PlanDocumentKind::Unrecognized;
    };
    if probe.id.is_some() {
        PlanDocumentKind::ExecutablePlan
    } else if probe.dry_run.is_some() || probe.command.is_some() {
        PlanDocumentKind::DryRunReport {
            source_command: probe.command,
        }
    } else {
        PlanDocumentKind::Unrecognized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_plan_is_detected_by_top_level_id() {
        let bytes = br#"{"schema_version":1,"id":"abc123","created_at":{}}"#;
        assert_eq!(
            classify_plan_document(bytes),
            PlanDocumentKind::ExecutablePlan
        );
    }

    #[test]
    fn dry_run_report_is_detected_without_id_and_carries_its_command() {
        let bytes = br#"{"schema_version":1,"command":"plan","dry_run":true,"entries":[]}"#;
        assert_eq!(
            classify_plan_document(bytes),
            PlanDocumentKind::DryRunReport {
                source_command: Some("plan".to_string()),
            }
        );
    }

    #[test]
    fn cargo_home_report_is_detected_by_command_without_id() {
        let bytes = br#"{"schema_version":1,"command":"cargo-home plan","dry_run":true}"#;
        assert_eq!(
            classify_plan_document(bytes),
            PlanDocumentKind::DryRunReport {
                source_command: Some("cargo-home plan".to_string()),
            }
        );
    }

    #[test]
    fn foreign_object_is_unrecognized() {
        assert_eq!(
            classify_plan_document(br#"{"hello":"world"}"#),
            PlanDocumentKind::Unrecognized
        );
    }

    #[test]
    fn non_object_json_is_unrecognized() {
        assert_eq!(
            classify_plan_document(b"[1, 2, 3]"),
            PlanDocumentKind::Unrecognized
        );
    }

    #[test]
    fn invalid_json_is_unrecognized() {
        assert_eq!(
            classify_plan_document(b"not json at all"),
            PlanDocumentKind::Unrecognized
        );
    }
}
