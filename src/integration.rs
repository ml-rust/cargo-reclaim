use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::active_process::{ActiveObservationProvider, ActiveObservationScope};
use crate::error::{ReclaimError, ReclaimResult};
use crate::inventory::snapshot_path;
use crate::inventory::{InventoryOptions, planner_candidates_from_target_root_with_context};
use crate::inventory::{
    append_fingerprint_group_candidates, append_stale_deps_candidates,
    append_stale_incremental_candidates,
};
use crate::model::{ArtifactClass, Plan, PlanInput, PlanSkip, PlanSkipReason};
use crate::planner::{
    ActiveObservation, PlannerCandidate, PlannerOptions, TargetContext, WholeTargetMode,
    build_plan_with_active_observation,
};
use crate::policy::PolicyKind;
use crate::scanner::{
    ScanItem, ScanSkip, ScanSkipReason, ScannerOptions, TargetCandidateKind, scan_roots,
};

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

pub struct BuildPlanFromScanItemsWithProviderRequest<'a, I, P> {
    pub input: PlanInput,
    pub policy: PolicyKind,
    pub items: I,
    pub scanner_options: &'a ScannerOptions,
    pub inventory_options: &'a InventoryOptions,
    pub planner_options: &'a PlannerOptions,
    pub active_observation_provider: &'a P,
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

pub fn build_plan_from_roots_with_active_observation_provider(
    roots: impl IntoIterator<Item = impl Into<PathBuf>>,
    policy: PolicyKind,
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
    planner_options: &PlannerOptions,
    active_observation_provider: &impl ActiveObservationProvider,
    now: SystemTime,
) -> ReclaimResult<Plan> {
    let roots = roots.into_iter().map(Into::into).collect::<Vec<_>>();
    let input = PlanInput::new(roots.clone())?;
    let items = scan_roots(roots, scanner_options)?;
    let active_observation =
        active_observation_provider.observe(&active_observation_scope_from_scan_items(&items));

    build_plan_from_scan_items_with_active_observation_impl(BuildPlanFromScanItemsActiveRequest {
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

pub fn build_plan_from_scan_items_with_active_observation_provider<I, P>(
    request: BuildPlanFromScanItemsWithProviderRequest<'_, I, P>,
) -> ReclaimResult<Plan>
where
    I: IntoIterator<Item = ScanItem>,
    P: ActiveObservationProvider,
{
    let BuildPlanFromScanItemsWithProviderRequest {
        input,
        policy,
        items,
        scanner_options,
        inventory_options,
        planner_options,
        active_observation_provider,
        now,
    } = request;
    let items = items.into_iter().collect::<Vec<_>>();
    let scope = active_observation_scope_from_scan_items(&items);
    let active_observation = active_observation_provider.observe(&scope);

    build_plan_from_scan_items_with_active_observation_impl(BuildPlanFromScanItemsActiveRequest {
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

pub fn active_observation_scope_from_scan_items(items: &[ScanItem]) -> ActiveObservationScope {
    ActiveObservationScope::from_target_contexts(items.iter().filter_map(|item| {
        let ScanItem::TargetCandidate(target_candidate) = item else {
            return None;
        };
        target_candidate.target_context.clone()
    }))
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
    build_plan_from_scan_items_with_active_observation_impl(BuildPlanFromScanItemsActiveRequest {
        input,
        policy,
        items: items.into_iter().collect::<Vec<_>>(),
        scanner_options,
        inventory_options,
        planner_options,
        active_observation,
        now,
    })
}

struct BuildPlanFromScanItemsActiveRequest<'a> {
    input: PlanInput,
    policy: PolicyKind,
    items: Vec<ScanItem>,
    scanner_options: &'a ScannerOptions,
    inventory_options: &'a InventoryOptions,
    planner_options: &'a PlannerOptions,
    active_observation: &'a ActiveObservation,
    now: SystemTime,
}

fn build_plan_from_scan_items_with_active_observation_impl(
    request: BuildPlanFromScanItemsActiveRequest<'_>,
) -> ReclaimResult<Plan> {
    let BuildPlanFromScanItemsActiveRequest {
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
    let mut skipped_paths = Vec::new();
    let scanner_skipped_paths = &scanner_options.skipped_paths;

    for item in items {
        let target_candidate = match item {
            ScanItem::TargetCandidate(target_candidate) => target_candidate,
            ScanItem::Skipped(skip) => {
                skipped_paths.push(plan_skip_from_scan_skip(skip)?);
                continue;
            }
            ScanItem::CargoProject(_) => continue,
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
        let mut inventory_options = inventory_options.clone();
        inventory_options
            .skipped_paths
            .extend(scanner_skipped_paths.iter().cloned());

        if planner_options.whole_target_mode == WholeTargetMode::Off
            || has_skipped_descendant(&target_candidate.path, &inventory_options)
        {
            let mut target_candidates = match planner_candidates_from_target_root_with_context(
                &target_candidate.path,
                evidence.clone(),
                target_context.clone(),
                &inventory_options,
            ) {
                Ok(candidates) => candidates,
                Err(error) => {
                    if push_vanished_inventory_skip(&error, &mut skipped_paths)? {
                        continue;
                    }
                    return Err(error);
                }
            };
            match append_fingerprint_group_candidates(
                &target_candidate.path,
                &evidence,
                &target_context,
                &inventory_options,
                &planner_options.keep_rustc_hashes,
                &mut target_candidates,
            ) {
                Ok(()) => {}
                Err(error) => {
                    if push_vanished_inventory_skip(&error, &mut skipped_paths)? {
                        continue;
                    }
                    return Err(error);
                }
            }
            match append_stale_deps_candidates(
                &target_candidate.path,
                &evidence,
                &target_context,
                &inventory_options,
                &planner_options.keep_rustc_hashes,
                &mut target_candidates,
            ) {
                Ok(()) => {}
                Err(error) => {
                    if push_vanished_inventory_skip(&error, &mut skipped_paths)? {
                        continue;
                    }
                    return Err(error);
                }
            }
            match append_stale_incremental_candidates(
                &target_candidate.path,
                &evidence,
                &target_context,
                &inventory_options,
                &mut target_candidates,
            ) {
                Ok(()) => {}
                Err(error) => {
                    if push_vanished_inventory_skip(&error, &mut skipped_paths)? {
                        continue;
                    }
                    return Err(error);
                }
            }
            // A build writes into its target continuously, so the newest artifact
            // mtime across the whole target is a race-free signal of an active
            // build — unlike the point-in-time process scan, which can be sampled
            // in a gap between rustc invocations or miss a build driver it does
            // not recognize. Stamp it on every candidate so the planner can
            // protect StaleDeps/StaleIncremental (old mtimes by definition, and
            // otherwise reliant on the racy process scan alone) while the target
            // is active.
            let target_newest_modified = target_candidates
                .iter()
                .filter_map(|candidate| candidate.snapshot.modified)
                .max();
            for candidate in &mut target_candidates {
                candidate.target_newest_modified = target_newest_modified;
            }
            candidates.extend(target_candidates);
        } else {
            let snapshot = match snapshot_path(&target_candidate.path, &inventory_options) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    if push_vanished_inventory_skip(&error, &mut skipped_paths)? {
                        continue;
                    }
                    return Err(error);
                }
            };
            candidates.push(
                PlannerCandidate::new(snapshot, ArtifactClass::WholeTarget, evidence)
                    .with_target_context(target_context),
            );
        }
    }

    let plan = build_plan_with_active_observation(
        input,
        policy,
        candidates,
        planner_options,
        active_observation,
        now,
    )?;
    Ok(Plan::with_skipped_paths(
        plan.input,
        plan.entries,
        skipped_paths,
    ))
}

fn push_vanished_inventory_skip(
    error: &ReclaimError,
    skipped_paths: &mut Vec<PlanSkip>,
) -> ReclaimResult<bool> {
    let ReclaimError::MissingInventoryPath { path } = error else {
        return Ok(false);
    };

    skipped_paths.push(PlanSkip::new(
        path.clone(),
        PlanSkipReason::VanishedDuringInventory,
        Some("path vanished while building the cleanup plan; this usually means an active build changed target contents".to_string()),
    )?);
    Ok(true)
}

fn plan_skip_from_scan_skip(skip: ScanSkip) -> ReclaimResult<PlanSkip> {
    let (reason, message) = match skip.reason {
        ScanSkipReason::DefaultIgnoredDir => (PlanSkipReason::DefaultIgnoredDir, None),
        ScanSkipReason::ConfiguredIgnoredPath => (PlanSkipReason::ConfiguredIgnoredPath, None),
        ScanSkipReason::SymlinkNotFollowed => (PlanSkipReason::SymlinkNotFollowed, None),
        ScanSkipReason::CrossFilesystem => (PlanSkipReason::CrossFilesystem, None),
        ScanSkipReason::WeakNameOnlySuppressed => (PlanSkipReason::WeakNameOnlySuppressed, None),
        ScanSkipReason::AlreadyVisited => (PlanSkipReason::AlreadyVisited, None),
        ScanSkipReason::CargoConfigUnsupported { message } => {
            (PlanSkipReason::CargoConfigUnsupported, Some(message))
        }
        ScanSkipReason::CargoConfigProblem { message } => {
            (PlanSkipReason::CargoConfigProblem, Some(message))
        }
        ScanSkipReason::ReadError { message } => (PlanSkipReason::ReadError, Some(message)),
    };

    PlanSkip::new(skip.path, reason, message)
}

fn has_skipped_descendant(target_root: &Path, inventory_options: &InventoryOptions) -> bool {
    let target_root = lexically_normalize(target_root);
    inventory_options.skipped_paths.iter().any(|skipped| {
        let skipped = lexically_normalize(skipped);
        skipped != target_root && skipped.starts_with(&target_root)
    })
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}
