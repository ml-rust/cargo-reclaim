use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::{BackgroundLimiter, ReclaimConfig, WholeTargetConfig};
use crate::disk::disk_free_space;
use crate::inventory::InventoryOptions;
use crate::planner::{PlannerOptions, WholeTargetMode};
use crate::policy::PolicyKind;
use crate::scanner::{ScanItem, ScannerOptions, TargetCandidateKind, scan_roots};
use crate::scheduler::{SchedulerError, SchedulerMode};
use crate::watcher::{
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherMode,
    WatcherObservedTarget, WatcherThresholds, decide_watcher_thresholds,
};

use super::super::{BackgroundRunRequest, BackgroundRunTrigger};
use super::model::{BackgroundServiceError, BackgroundServiceResult};

#[derive(Debug, Clone)]
pub(crate) struct BackgroundCycleRequestContext {
    config_path: PathBuf,
    config_version: u16,
    roots: Vec<PathBuf>,
    policy: PolicyKind,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    planner_options: PlannerOptions,
    mode: SchedulerMode,
    allow_apply: bool,
    background_enabled: bool,
}

impl BackgroundCycleRequestContext {
    pub(crate) fn from_config(
        config: &ReclaimConfig,
        config_path: &Path,
        mode: SchedulerMode,
    ) -> BackgroundServiceResult<Self> {
        let policy = effective_run_policy(mode, config)?;
        let allow_apply = config.scheduler.allow_unattended_cleanup.unwrap_or(false);
        validate_run_apply_policy(mode, allow_apply, policy, config)?;
        validate_whole_target_policy(policy, config)?;

        Ok(Self {
            config_path: config_path.to_path_buf(),
            config_version: config.version,
            roots: run_roots(config),
            policy,
            scanner_options: scanner_options_from_config(config),
            inventory_options: inventory_options_from_config(config),
            planner_options: planner_options_from_config(config),
            mode,
            allow_apply,
            background_enabled: config.background.enabled.unwrap_or(true),
        })
    }

    /// Build a cleanup request for a single background trigger, gated by its
    /// `limiter`. An empty limiter always cleans (a `periodic` block); a limiter
    /// with disk thresholds cleans only when a threshold is breached.
    pub(crate) fn request(
        &self,
        limiter: &BackgroundLimiter,
        run_id: String,
        log_path: PathBuf,
        plan_path: PathBuf,
        now: SystemTime,
    ) -> BackgroundServiceResult<BackgroundRunRequest> {
        Ok(BackgroundRunRequest {
            run_id,
            log_path,
            plan_path,
            roots: self.roots.clone(),
            policy: self.policy,
            scanner_options: self.scanner_options.clone(),
            inventory_options: self.inventory_options.clone(),
            planner_options: self.planner_options.clone(),
            trigger: BackgroundRunTrigger::Decision(self.run_decision(limiter)?),
            config_path: Some(self.config_path.clone()),
            config_version: Some(self.config_version),
            created_at: now,
            now,
            expires_at: now + Duration::from_secs(60 * 60),
        })
    }

    fn run_decision(
        &self,
        limiter: &BackgroundLimiter,
    ) -> BackgroundServiceResult<WatcherDecision> {
        if !self.background_enabled {
            return Ok(WatcherDecision {
                state: WatcherDecisionState::Inactive,
                reasons: Vec::new(),
            });
        }

        // A non-empty limiter gates the run: only clean when a threshold is
        // breached. Disk thresholds use a cheap free-space check; a
        // `max_target_size` limiter additionally scans target sizes.
        if !limiter.is_empty() {
            let observed_targets = if limiter.needs_target_scan() {
                observed_targets_from_roots(
                    &self.roots,
                    &self.scanner_options,
                    &self.inventory_options,
                )?
            } else {
                Vec::new()
            };
            let disk_free_space = if limiter.disk_free_below_basis_points.is_some()
                || limiter.min_free_disk_bytes.is_some()
            {
                self.observed_disk_free_space()?
            } else {
                None
            };
            return Ok(decide_watcher_thresholds(WatcherDecisionInput {
                enabled: self.background_enabled,
                mode: WatcherMode::Threshold,
                thresholds: WatcherThresholds {
                    max_target_size_bytes: limiter.max_target_size_bytes,
                    disk_free_below_basis_points: limiter.disk_free_below_basis_points,
                    min_free_disk_bytes: limiter.min_free_disk_bytes,
                },
                observed_targets,
                disk_free_basis_points: disk_free_space.and_then(|space| space.free_basis_points()),
                disk_free_bytes: disk_free_space.map(|space| space.available_bytes),
                selected_policy: self.policy,
                unattended_allowed: self.mode == SchedulerMode::Cleanup && self.allow_apply,
            }));
        }

        // No limiter: a periodic block always cleans when it fires.
        Ok(WatcherDecision {
            state: if self.mode == SchedulerMode::Cleanup
                && self.allow_apply
                && self.policy != PolicyKind::Observe
            {
                WatcherDecisionState::TriggeredPlanAndApply
            } else {
                WatcherDecisionState::TriggeredPlanOnly
            },
            reasons: Vec::new(),
        })
    }

    fn observed_disk_free_space(
        &self,
    ) -> BackgroundServiceResult<Option<crate::disk::DiskFreeSpace>> {
        let root = self
            .roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or_else(|| Path::new("."));
        disk_free_space(root)
            .map(Some)
            .map_err(BackgroundServiceError::from)
    }
}

pub(crate) fn scheduler_mode_from_config(
    config: &ReclaimConfig,
) -> BackgroundServiceResult<SchedulerMode> {
    match config.scheduler.mode.as_deref() {
        Some("observe") | None => Ok(SchedulerMode::Observe),
        Some("cleanup") => Ok(SchedulerMode::Cleanup),
        Some(value) => Err(BackgroundServiceError::Config(format!(
            "unknown scheduler mode `{value}`; expected observe or cleanup"
        ))),
    }
}

fn effective_run_policy(
    mode: SchedulerMode,
    config: &ReclaimConfig,
) -> BackgroundServiceResult<PolicyKind> {
    let policy = config
        .scheduler
        .policy
        .as_deref()
        .or(config.policy.as_deref())
        .map(parse_policy)
        .transpose()?;
    Ok(policy.unwrap_or(match mode {
        SchedulerMode::Observe => PolicyKind::Observe,
        SchedulerMode::Cleanup => PolicyKind::Conservative,
    }))
}

fn validate_run_apply_policy(
    mode: SchedulerMode,
    allow_apply: bool,
    policy: PolicyKind,
    config: &ReclaimConfig,
) -> BackgroundServiceResult<()> {
    if mode == SchedulerMode::Cleanup && !allow_apply {
        return Err(BackgroundServiceError::Scheduler(
            SchedulerError::CleanupNotAllowed,
        ));
    }
    if mode == SchedulerMode::Cleanup
        && allow_apply
        && matches!(
            policy,
            PolicyKind::Balanced | PolicyKind::Aggressive | PolicyKind::Custom
        )
        && !config
            .scheduler
            .allow_unattended_high_policy
            .unwrap_or(false)
    {
        return Err(BackgroundServiceError::Scheduler(
            SchedulerError::HighPolicyNotAllowed(policy),
        ));
    }
    Ok(())
}

fn validate_whole_target_policy(
    policy: PolicyKind,
    config: &ReclaimConfig,
) -> BackgroundServiceResult<()> {
    if config.whole_target != Some(WholeTargetConfig::Delete) {
        return Ok(());
    }

    if policy != PolicyKind::Aggressive {
        return Err(BackgroundServiceError::Config(
            "config whole_target = \"delete\" requires aggressive policy".to_owned(),
        ));
    }
    if !config.allow_unattended_whole_target_delete.unwrap_or(false) {
        return Err(BackgroundServiceError::Config(
            "config whole_target = \"delete\" requires allow_unattended_whole_target_delete = true"
                .to_owned(),
        ));
    }

    Ok(())
}

fn parse_policy(value: &str) -> BackgroundServiceResult<PolicyKind> {
    match value {
        "observe" => Ok(PolicyKind::Observe),
        "conservative" => Ok(PolicyKind::Conservative),
        "balanced" => Ok(PolicyKind::Balanced),
        "aggressive" => Ok(PolicyKind::Aggressive),
        "custom" => Ok(PolicyKind::Custom),
        _ => Err(BackgroundServiceError::Config(format!(
            "unknown policy `{value}`; expected observe, conservative, balanced, aggressive, or custom"
        ))),
    }
}

fn run_roots(config: &ReclaimConfig) -> Vec<PathBuf> {
    if config.roots.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        config.roots.clone()
    }
}

fn scanner_options_from_config(config: &ReclaimConfig) -> ScannerOptions {
    ScannerOptions {
        ignored_paths: config.ignored_paths.clone(),
        skipped_paths: config.skipped_paths.clone(),
        follow_symlinks: config.scanner.follow_symlinks.unwrap_or(false),
        allow_name_only_targets: config.scanner.allow_name_only_targets.unwrap_or(false),
        cross_filesystems: config.scanner.cross_filesystems.unwrap_or(false),
    }
}

fn inventory_options_from_config(config: &ReclaimConfig) -> InventoryOptions {
    InventoryOptions {
        follow_symlinks: config.scanner.follow_symlinks.unwrap_or(false),
        skipped_paths: config.skipped_paths.clone(),
        deep_target_scan: false,
        deep_directory_measurement: true,
    }
}

fn planner_options_from_config(config: &ReclaimConfig) -> PlannerOptions {
    PlannerOptions {
        recent_write_keep_window: config.recent_write_keep_window,
        keep_size_bytes: config.keep_size_bytes,
        target_size_goal_bytes: config.policy_thresholds.target_size_goal_bytes,
        target_free_disk_bytes: config.background.target_free_disk_bytes,
        minimum_reclaim_bytes: None,
        keep_rustc_hashes: config.keep_rustc_hashes.clone(),
        keep_installed_toolchains: config.keep_installed_toolchains,
        keep_toolchains: config.keep_toolchains.clone(),
        whole_target_mode: config
            .whole_target
            .map(whole_target_mode_from_config)
            .unwrap_or_default(),
    }
}

fn whole_target_mode_from_config(value: WholeTargetConfig) -> WholeTargetMode {
    match value {
        WholeTargetConfig::Off => WholeTargetMode::Off,
        WholeTargetConfig::Confirm => WholeTargetMode::Confirm,
        WholeTargetConfig::Delete => WholeTargetMode::DeleteConfirmed,
    }
}

fn observed_targets_from_roots(
    roots: &[PathBuf],
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
) -> BackgroundServiceResult<Vec<WatcherObservedTarget>> {
    let items = scan_roots(roots.iter().cloned(), scanner_options)
        .map_err(|error| BackgroundServiceError::Config(error.to_string()))?;
    let mut observed_targets = Vec::new();

    for item in items {
        let ScanItem::TargetCandidate(candidate) = item else {
            continue;
        };
        if candidate.kind != TargetCandidateKind::CargoTargetDir {
            continue;
        }

        let snapshot = crate::inventory::snapshot_path(&candidate.path, inventory_options)
            .map_err(|error| BackgroundServiceError::Config(error.to_string()))?;
        observed_targets.push(WatcherObservedTarget {
            path: candidate.path,
            size_bytes: snapshot.size_bytes,
        });
    }

    Ok(observed_targets)
}
