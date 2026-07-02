use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::error::ReclaimResult;
use crate::inventory::{InventoryOptions, planner_candidates_from_target_root_with_context};
use crate::model::{Plan, PlanInput};
use crate::planner::{
    ActiveObservation, PlannerOptions, TargetContext, build_plan_with_active_observation,
};
use crate::policy::PolicyKind;
use crate::scanner::{ScanItem, ScannerOptions, TargetCandidateKind, scan_roots};

pub struct BuildPlanFromScanItemsRequest<'a, I> {
    pub input: PlanInput,
    pub policy: PolicyKind,
    pub items: I,
    pub scanner_options: &'a ScannerOptions,
    pub inventory_options: &'a InventoryOptions,
    pub planner_options: &'a PlannerOptions,
    pub active_observation: &'a ActiveObservation,
    pub now: SystemTime,
}

pub fn build_plan_from_roots(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
) -> ReclaimResult<Plan> {
    build_plan_from_roots_with_active_observation(
        roots,
        policy,
        scanner_options,
        inventory_options,
        &PlannerOptions::default(),
        &ActiveObservation::not_attempted(),
        SystemTime::now(),
    )
}

pub fn build_plan_from_roots_with_active_observation(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    planner_options: &PlannerOptions,
    active_observation: &ActiveObservation,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    let roots = roots.into_iter().map(Into::into).collect::<Vec<_>>();
    let input = PlanInput::new(roots.clone())?;
    let items = scan_roots(roots, scanner_options)?;

    build_plan_from_scan_items_with_active_observation(BuildPlanFromScanItemsRequest {
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options,
        active_observation,
        now,
    })
}

pub fn build_plan_from_roots_with_options(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    planner_options: &PlannerOptions,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    build_plan_from_roots_with_active_observation(
        roots,
        policy,
        scanner_options,
        inventory_options,
        planner_options,
        &ActiveObservation::not_attempted(),
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
    let planner_options = PlannerOptions::default();
    let active_observation = ActiveObservation::not_attempted();

    build_plan_from_scan_items_with_active_observation(BuildPlanFromScanItemsRequest {
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options: &planner_options,
        active_observation: &active_observation,
        now: SystemTime::now(),
    })
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
    let active_observation = ActiveObservation::not_attempted();

    build_plan_from_scan_items_with_active_observation(BuildPlanFromScanItemsRequest {
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options,
        active_observation: &active_observation,
        now,
    })
}

pub fn build_plan_from_scan_items_with_active_observation<I>(
    request: BuildPlanFromScanItemsRequest<'_, I>,
) -> ReclaimResult<Plan>
where
    I: IntoIterator<Item = ScanItem>,
{
    let BuildPlanFromScanItemsRequest {
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options,
        active_observation,
        now,
    } = request;
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

        let target_context = target_candidate
            .target_context
            .unwrap_or_else(|| TargetContext::new(&target_candidate.path));

        candidates.extend(planner_candidates_from_target_root_with_context(
            &target_candidate.path,
            evidence,
            target_context,
            inventory_options,
        )?);
    }

    build_plan_with_active_observation(
        input,
        policy,
        candidates,
        planner_options,
        active_observation,
        now,
    )
}
