use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::error::ReclaimResult;
use crate::inventory::{InventoryOptions, planner_candidates_from_target_root};
use crate::model::{Plan, PlanInput};
use crate::planner::{PlannerOptions, build_plan_with_options};
use crate::policy::PolicyKind;
use crate::scanner::{ScanItem, ScannerOptions, TargetCandidateKind, scan_roots};

pub fn build_plan_from_roots(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
) -> ReclaimResult<Plan> {
    build_plan_from_roots_with_options(
        roots,
        policy,
        scanner_options,
        inventory_options,
        &PlannerOptions::default(),
        SystemTime::now(),
    )
}

pub fn build_plan_from_roots_with_options(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    planner_options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    let roots = roots.into_iter().map(Into::into).collect::<Vec<_>>();
    let input = PlanInput::new(roots.clone())?;
    let items = scan_roots(roots, scanner_options)?;

    build_plan_from_scan_items_with_options(
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options,
        now,
    )
}

pub fn build_plan_from_scan_items(
    input: PlanInput,
    policy: PolicyKind,
    items: impl IntoIterator<Item = ScanItem>,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
) -> ReclaimResult<Plan> {
    build_plan_from_scan_items_with_options(
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        &PlannerOptions::default(),
        SystemTime::now(),
    )
}

pub fn build_plan_from_scan_items_with_options(
    input: PlanInput,
    policy: PolicyKind,
    items: impl IntoIterator<Item = ScanItem>,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    planner_options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    let mut seen_target_roots = HashSet::new();
    let mut candidates = Vec::new();

    for item in items {
        let ScanItem::TargetCandidate(target_candidate) = item else {
            continue;
        };

        if target_candidate.kind != TargetCandidateKind::CargoTargetDir {
            continue;
        }

        let Some(evidence) = target_candidate.evidence else {
            continue;
        };
        if evidence.is_weak_name_only() && !scanner_options.allow_name_only_targets {
            continue;
        }

        if !seen_target_roots.insert(target_candidate.path.clone()) {
            continue;
        }

        candidates.extend(planner_candidates_from_target_root(
            target_candidate.path,
            evidence,
            inventory_options,
        )?);
    }

    build_plan_with_options(input, policy, candidates, planner_options, now)
}
