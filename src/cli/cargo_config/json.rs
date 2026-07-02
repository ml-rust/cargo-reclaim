use std::io::Write;

use cargo_reclaim::config::{
    CargoConfigFileSnapshot, CargoConfigPreviewOperation, CargoConfigPreviewReport,
};
use cargo_reclaim::{
    CargoConfigOutputDir, CargoConfigProblem, CargoConfigRecommendReport,
    CargoConfigRecommendation, CargoConfigUnsupported,
};
use serde::Serialize;

use super::super::CliError;
use super::labels::{path_string, preview_operation_status_label, unsupported_reason_label};

pub(super) fn write_json_recommend_report(
    output: &mut impl Write,
    report: &CargoConfigRecommendReport,
) -> Result<(), CliError> {
    let document = JsonCargoConfigRecommendReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn write_json_preview_report(
    output: &mut impl Write,
    report: &CargoConfigPreviewReport,
) -> Result<(), CliError> {
    let document = JsonCargoConfigPreviewReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonCargoConfigRecommendReport {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    modified_cargo_config_files: bool,
    project: String,
    target_dirs: Vec<JsonCargoConfigOutputDir>,
    build_dirs: Vec<JsonCargoConfigOutputDir>,
    recommendations: Vec<JsonCargoConfigRecommendation>,
    unsupported: Vec<JsonCargoConfigUnsupported>,
    problems: Vec<JsonCargoConfigProblem>,
}

impl JsonCargoConfigRecommendReport {
    fn from_report(report: &CargoConfigRecommendReport) -> Self {
        Self {
            schema_version: report.schema_version,
            command: "cargo-config recommend",
            dry_run: true,
            modified_cargo_config_files: false,
            project: path_string(&report.project),
            target_dirs: report
                .target_dirs
                .iter()
                .map(JsonCargoConfigOutputDir::from_dir)
                .collect(),
            build_dirs: report
                .build_dirs
                .iter()
                .map(JsonCargoConfigOutputDir::from_dir)
                .collect(),
            recommendations: report
                .recommendations
                .iter()
                .map(JsonCargoConfigRecommendation::from_recommendation)
                .collect(),
            unsupported: report
                .unsupported
                .iter()
                .map(JsonCargoConfigUnsupported::from_unsupported)
                .collect(),
            problems: report
                .problems
                .iter()
                .map(JsonCargoConfigProblem::from_problem)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigPreviewReport {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    modified_cargo_config_files: bool,
    project: String,
    target_config_file: String,
    target_config_snapshot: JsonCargoConfigFileSnapshot,
    operations: Vec<JsonCargoConfigPreviewOperation>,
    unsupported: Vec<JsonCargoConfigUnsupported>,
    problems: Vec<JsonCargoConfigProblem>,
}

impl JsonCargoConfigPreviewReport {
    fn from_report(report: &CargoConfigPreviewReport) -> Self {
        Self {
            schema_version: report.schema_version,
            command: "cargo-config preview",
            dry_run: report.dry_run,
            modified_cargo_config_files: report.modified_cargo_config_files,
            project: path_string(&report.project),
            target_config_file: path_string(&report.target_config_file),
            target_config_snapshot: JsonCargoConfigFileSnapshot::from_snapshot(
                &report.target_config_snapshot,
            ),
            operations: report
                .operations
                .iter()
                .map(JsonCargoConfigPreviewOperation::from_operation)
                .collect(),
            unsupported: report
                .unsupported
                .iter()
                .map(JsonCargoConfigUnsupported::from_unsupported)
                .collect(),
            problems: report
                .problems
                .iter()
                .map(JsonCargoConfigProblem::from_problem)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigFileSnapshot {
    exists: bool,
    hash: Option<String>,
    size_bytes: Option<u64>,
}

impl JsonCargoConfigFileSnapshot {
    fn from_snapshot(snapshot: &CargoConfigFileSnapshot) -> Self {
        Self {
            exists: snapshot.exists,
            hash: snapshot.hash.clone(),
            size_bytes: snapshot.size_bytes,
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigPreviewOperation {
    key: String,
    current: Option<String>,
    recommended: Option<String>,
    status: &'static str,
    reason: String,
}

impl JsonCargoConfigPreviewOperation {
    fn from_operation(operation: &CargoConfigPreviewOperation) -> Self {
        Self {
            key: operation.key.clone(),
            current: operation.current.clone(),
            recommended: operation.recommended.clone(),
            status: preview_operation_status_label(operation.status),
            reason: operation.reason.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigRecommendation {
    key: String,
    current: Option<String>,
    recommended: Option<String>,
    reason: String,
}

impl JsonCargoConfigRecommendation {
    fn from_recommendation(recommendation: &CargoConfigRecommendation) -> Self {
        Self {
            key: recommendation.key.clone(),
            current: recommendation.current.clone(),
            recommended: recommendation.recommended.clone(),
            reason: recommendation.reason.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigOutputDir {
    path: String,
    source: String,
}

impl JsonCargoConfigOutputDir {
    fn from_dir(dir: &CargoConfigOutputDir) -> Self {
        Self {
            path: path_string(&dir.path),
            source: dir.source.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigUnsupported {
    source: String,
    reason: &'static str,
}

impl JsonCargoConfigUnsupported {
    fn from_unsupported(unsupported: &CargoConfigUnsupported) -> Self {
        Self {
            source: unsupported.source.clone(),
            reason: unsupported_reason_label(&unsupported.reason),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoConfigProblem {
    path: String,
    message: String,
}

impl JsonCargoConfigProblem {
    fn from_problem(problem: &CargoConfigProblem) -> Self {
        Self {
            path: path_string(&problem.path),
            message: problem.message.clone(),
        }
    }
}
