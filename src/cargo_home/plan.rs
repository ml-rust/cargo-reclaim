use std::path::{Path, PathBuf};

use crate::PolicyKind;

use super::model::{
    CARGO_HOME_PLAN_SCHEMA_VERSION, CargoHomeClass, CargoHomeEntry, CargoHomeError,
    CargoHomePathKind, CargoHomePlan, CargoHomePlanAction, CargoHomePlanEntry, CargoHomePlanTotals,
    CargoHomeProblem, CargoHomeReport,
};
use super::report::{CargoHomeReportRequest, build_cargo_home_report};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoHomePlanRequest {
    pub cargo_home: Option<PathBuf>,
    pub policy: PolicyKind,
}

pub fn build_cargo_home_plan(
    request: CargoHomePlanRequest,
) -> Result<CargoHomePlan, CargoHomeError> {
    let report = build_cargo_home_report(CargoHomeReportRequest {
        cargo_home: request.cargo_home,
    })?;
    Ok(build_cargo_home_plan_from_report(report, request.policy))
}

pub fn build_cargo_home_plan_from_report(
    report: CargoHomeReport,
    policy: PolicyKind,
) -> CargoHomePlan {
    let entries = report
        .entries
        .iter()
        .map(|entry| plan_entry(entry, policy, &report.problems))
        .collect::<Vec<_>>();
    let totals = totals_for_entries(&entries, report.problems.len());
    CargoHomePlan {
        schema_version: CARGO_HOME_PLAN_SCHEMA_VERSION,
        input: report.input,
        policy,
        entries,
        totals,
        recommendations: report.recommendations,
        problems: report.problems,
    }
}

fn plan_entry(
    entry: &CargoHomeEntry,
    policy: PolicyKind,
    problems: &[CargoHomeProblem],
) -> CargoHomePlanEntry {
    let (action, reason) = if entry.skipped
        || entry.path_kind == CargoHomePathKind::Symlink
        || has_problem(entry, problems)
    {
        (
            CargoHomePlanAction::SkipProblem,
            skip_problem_reason(entry.path_kind),
        )
    } else if is_delete_candidate(entry.class, policy) {
        (
            CargoHomePlanAction::DeleteCandidate,
            delete_candidate_reason(entry.class, policy),
        )
    } else {
        (
            CargoHomePlanAction::Preserve,
            preserve_reason(entry.class, policy),
        )
    };

    CargoHomePlanEntry {
        path: entry.path.clone(),
        relative_path: entry.relative_path.clone(),
        class: entry.class,
        path_kind: entry.path_kind,
        size_bytes: entry.size_bytes,
        action,
        reason: reason.to_string(),
    }
}

fn has_problem(entry: &CargoHomeEntry, problems: &[CargoHomeProblem]) -> bool {
    problems
        .iter()
        .any(|problem| path_is_at_or_inside(&problem.path, &entry.path))
}

fn path_is_at_or_inside(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn is_delete_candidate(class: CargoHomeClass, policy: PolicyKind) -> bool {
    match policy {
        PolicyKind::Observe | PolicyKind::Custom => false,
        PolicyKind::Conservative => matches!(class, CargoHomeClass::RegistryCache),
        PolicyKind::Balanced => matches!(
            class,
            CargoHomeClass::RegistryCache | CargoHomeClass::RegistrySource
        ),
        PolicyKind::Aggressive => is_known_cache_class(class),
    }
}

fn is_known_cache_class(class: CargoHomeClass) -> bool {
    matches!(
        class,
        CargoHomeClass::RegistryIndex
            | CargoHomeClass::RegistryCache
            | CargoHomeClass::RegistrySource
            | CargoHomeClass::GitDatabase
            | CargoHomeClass::GitCheckouts
    )
}

fn skip_problem_reason(path_kind: CargoHomePathKind) -> &'static str {
    if path_kind == CargoHomePathKind::Symlink {
        "skipped; Cargo home cleanup plan preserves symlinks"
    } else {
        "skipped; Cargo home cleanup plan preserves entries with incomplete inspection"
    }
}

fn delete_candidate_reason(class: CargoHomeClass, policy: PolicyKind) -> &'static str {
    match (policy, class) {
        (PolicyKind::Conservative, CargoHomeClass::RegistryCache) => {
            "delete candidate; conservative Cargo home policy includes registry package cache"
        }
        (PolicyKind::Balanced, CargoHomeClass::RegistryCache) => {
            "delete candidate; balanced Cargo home policy includes registry package cache"
        }
        (PolicyKind::Balanced, CargoHomeClass::RegistrySource) => {
            "delete candidate; balanced Cargo home policy includes unpacked registry sources"
        }
        (PolicyKind::Aggressive, CargoHomeClass::RegistryIndex) => {
            "delete candidate; aggressive Cargo home policy includes registry indexes"
        }
        (PolicyKind::Aggressive, CargoHomeClass::RegistryCache) => {
            "delete candidate; aggressive Cargo home policy includes registry package cache"
        }
        (PolicyKind::Aggressive, CargoHomeClass::RegistrySource) => {
            "delete candidate; aggressive Cargo home policy includes unpacked registry sources"
        }
        (PolicyKind::Aggressive, CargoHomeClass::GitDatabase) => {
            "delete candidate; aggressive Cargo home policy includes cached git databases"
        }
        (PolicyKind::Aggressive, CargoHomeClass::GitCheckouts) => {
            "delete candidate; aggressive Cargo home policy includes cached git checkouts"
        }
        _ => "delete candidate; Cargo home policy selected this cache entry",
    }
}

fn preserve_reason(class: CargoHomeClass, policy: PolicyKind) -> &'static str {
    match class {
        CargoHomeClass::Config => "preserved; Cargo home configuration is never selected",
        CargoHomeClass::Credentials => "preserved; Cargo home credentials are never selected",
        CargoHomeClass::InstalledBinaries => {
            "preserved; Cargo-installed binaries are never selected"
        }
        CargoHomeClass::InstallMetadata => "preserved; Cargo install metadata is never selected",
        CargoHomeClass::UnknownUserAuthored => {
            "preserved; unrecognized Cargo home paths may be user-authored"
        }
        _ if policy == PolicyKind::Observe => {
            "preserved; observe Cargo home policy does not select cleanup candidates"
        }
        _ if policy == PolicyKind::Custom => {
            "preserved; Cargo home custom selectors are not configured"
        }
        CargoHomeClass::RegistryIndex => {
            "preserved; selected Cargo home policy keeps registry indexes"
        }
        CargoHomeClass::RegistryCache => {
            "preserved; selected Cargo home policy keeps registry package cache"
        }
        CargoHomeClass::RegistrySource => {
            "preserved; selected Cargo home policy keeps unpacked registry sources"
        }
        CargoHomeClass::GitDatabase => {
            "preserved; selected Cargo home policy keeps cached git databases"
        }
        CargoHomeClass::GitCheckouts => {
            "preserved; selected Cargo home policy keeps cached git checkouts"
        }
    }
}

fn totals_for_entries(entries: &[CargoHomePlanEntry], problem_count: usize) -> CargoHomePlanTotals {
    let mut total_bytes = 0u64;
    let mut delete_candidate_count = 0usize;
    let mut delete_candidate_bytes = 0u64;
    let mut preserved_count = 0usize;
    let mut preserved_bytes = 0u64;
    let mut skipped_count = 0usize;

    for entry in entries {
        total_bytes = total_bytes.saturating_add(entry.size_bytes);
        match entry.action {
            CargoHomePlanAction::DeleteCandidate => {
                delete_candidate_count += 1;
                delete_candidate_bytes = delete_candidate_bytes.saturating_add(entry.size_bytes);
            }
            CargoHomePlanAction::Preserve => {
                preserved_count += 1;
                preserved_bytes = preserved_bytes.saturating_add(entry.size_bytes);
            }
            CargoHomePlanAction::SkipProblem => {
                skipped_count += 1;
            }
        }
    }

    CargoHomePlanTotals {
        entry_count: entries.len(),
        total_bytes,
        delete_candidate_count,
        delete_candidate_bytes,
        preserved_count,
        preserved_bytes,
        skipped_count,
        problem_count,
    }
}
