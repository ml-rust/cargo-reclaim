use std::io::Write;

use cargo_reclaim::{
    CargoHomeApplyEntryStatus, CargoHomeApplyReport, CargoHomeEntry, CargoHomePlan,
    CargoHomePlanEntry, CargoHomeReport,
};
use serde::Serialize;

use super::super::CliError;
use super::labels::{
    action_label, class_label, path_kind_label, path_string, policy_label, source_label,
};

pub(super) fn write_json_report(
    output: &mut impl Write,
    report: &CargoHomeReport,
) -> Result<(), CliError> {
    let document = JsonCargoHomeReport::from_report(report);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn write_json_plan(
    output: &mut impl Write,
    plan: &CargoHomePlan,
) -> Result<(), CliError> {
    let document = JsonCargoHomePlan::from_plan(plan);
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

pub(super) fn write_json_apply_report(
    output: &mut impl Write,
    report: &CargoHomeApplyReport,
) -> Result<(), CliError> {
    let entries = report
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.path,
                "planned_action": entry.planned_action,
                "status": apply_status_label(entry.status),
                "size_bytes": entry.size_bytes,
                "reason": entry.reason,
            })
        })
        .collect::<Vec<_>>();
    let document = serde_json::json!({
        "schema_version": 1,
        "command": "cargo-home apply",
        "dry_run": report.dry_run,
        "validation_only": report.validation_only,
        "plan_id": report.plan_id.as_str(),
        "totals": {
            "entry_count": report.totals.entry_count,
            "delete_candidate_count": report.totals.delete_candidate_count,
            "would_delete_count": report.totals.would_delete_count,
            "applied_count": report.totals.applied_count,
            "failed_count": report.totals.failed_count,
            "skipped_count": report.totals.skipped_count,
            "stale_skip_count": report.totals.stale_skip_count,
            "would_delete_bytes": report.totals.would_delete_bytes,
            "applied_bytes": report.totals.applied_bytes,
        },
        "entries": entries,
    });
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonCargoHomeReport {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    input: JsonCargoHomeInput,
    totals: JsonCargoHomeTotals,
    entries: Vec<JsonCargoHomeEntry>,
    recommendations: Vec<String>,
    problems: Vec<JsonCargoHomeProblem>,
}

impl JsonCargoHomeReport {
    fn from_report(report: &CargoHomeReport) -> Self {
        Self {
            schema_version: report.schema_version,
            command: "cargo-home report",
            dry_run: true,
            input: JsonCargoHomeInput {
                root: path_string(&report.input.root),
                source: source_label(report.input.source),
            },
            totals: JsonCargoHomeTotals {
                entry_count: report.totals.entry_count,
                total_bytes: report.totals.total_bytes,
                cache_bytes: report.totals.cache_bytes,
                preserved_bytes: report.totals.preserved_bytes,
                skipped_count: report.totals.skipped_count,
                problem_count: report.totals.problem_count,
                known_cache_entry_count: report.totals.known_cache_entry_count,
            },
            entries: report
                .entries
                .iter()
                .map(JsonCargoHomeEntry::from_entry)
                .collect(),
            recommendations: report
                .recommendations
                .iter()
                .map(|recommendation| recommendation.message.clone())
                .collect(),
            problems: report
                .problems
                .iter()
                .map(|problem| JsonCargoHomeProblem {
                    path: path_string(&problem.path),
                    message: problem.message.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoHomeInput {
    root: String,
    source: &'static str,
}

#[derive(Serialize)]
struct JsonCargoHomeTotals {
    entry_count: usize,
    total_bytes: u64,
    cache_bytes: u64,
    preserved_bytes: u64,
    skipped_count: usize,
    problem_count: usize,
    known_cache_entry_count: usize,
}

#[derive(Serialize)]
struct JsonCargoHomeEntry {
    path: String,
    relative_path: String,
    class: &'static str,
    path_kind: &'static str,
    size_bytes: u64,
    preserved: bool,
    skipped: bool,
    reason: String,
}

impl JsonCargoHomeEntry {
    fn from_entry(entry: &CargoHomeEntry) -> Self {
        Self {
            path: path_string(&entry.path),
            relative_path: path_string(&entry.relative_path),
            class: class_label(entry.class),
            path_kind: path_kind_label(entry.path_kind),
            size_bytes: entry.size_bytes,
            preserved: entry.preserved,
            skipped: entry.skipped,
            reason: entry.reason.clone(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoHomeProblem {
    path: String,
    message: String,
}

#[derive(Serialize)]
struct JsonCargoHomePlan {
    schema_version: u16,
    command: &'static str,
    dry_run: bool,
    policy: &'static str,
    input: JsonCargoHomeInput,
    totals: JsonCargoHomePlanTotals,
    entries: Vec<JsonCargoHomePlanEntry>,
    recommendations: Vec<String>,
    problems: Vec<JsonCargoHomeProblem>,
}

impl JsonCargoHomePlan {
    fn from_plan(plan: &CargoHomePlan) -> Self {
        Self {
            schema_version: plan.schema_version,
            command: "cargo-home plan",
            dry_run: true,
            policy: policy_label(plan.policy),
            input: JsonCargoHomeInput {
                root: path_string(&plan.input.root),
                source: source_label(plan.input.source),
            },
            totals: JsonCargoHomePlanTotals {
                entry_count: plan.totals.entry_count,
                total_bytes: plan.totals.total_bytes,
                delete_candidate_count: plan.totals.delete_candidate_count,
                delete_candidate_bytes: plan.totals.delete_candidate_bytes,
                preserved_count: plan.totals.preserved_count,
                preserved_bytes: plan.totals.preserved_bytes,
                skipped_count: plan.totals.skipped_count,
                problem_count: plan.totals.problem_count,
            },
            entries: plan
                .entries
                .iter()
                .map(JsonCargoHomePlanEntry::from_entry)
                .collect(),
            recommendations: plan
                .recommendations
                .iter()
                .map(|recommendation| recommendation.message.clone())
                .collect(),
            problems: plan
                .problems
                .iter()
                .map(|problem| JsonCargoHomeProblem {
                    path: path_string(&problem.path),
                    message: problem.message.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct JsonCargoHomePlanTotals {
    entry_count: usize,
    total_bytes: u64,
    delete_candidate_count: usize,
    delete_candidate_bytes: u64,
    preserved_count: usize,
    preserved_bytes: u64,
    skipped_count: usize,
    problem_count: usize,
}

#[derive(Serialize)]
struct JsonCargoHomePlanEntry {
    path: String,
    relative_path: String,
    class: &'static str,
    path_kind: &'static str,
    size_bytes: u64,
    action: &'static str,
    reason: String,
}

impl JsonCargoHomePlanEntry {
    fn from_entry(entry: &CargoHomePlanEntry) -> Self {
        Self {
            path: path_string(&entry.path),
            relative_path: path_string(&entry.relative_path),
            class: class_label(entry.class),
            path_kind: path_kind_label(entry.path_kind),
            size_bytes: entry.size_bytes,
            action: action_label(entry.action),
            reason: entry.reason.clone(),
        }
    }
}

fn apply_status_label(status: CargoHomeApplyEntryStatus) -> &'static str {
    match status {
        CargoHomeApplyEntryStatus::WouldDelete => "would_delete",
        CargoHomeApplyEntryStatus::Deleted => "deleted",
        CargoHomeApplyEntryStatus::NotPlannedForDeletion => "not_planned_for_deletion",
        CargoHomeApplyEntryStatus::SkipStalePlan => "skip_stale_plan",
        CargoHomeApplyEntryStatus::DeleteFailed => "delete_failed",
    }
}
