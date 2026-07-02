use std::path::PathBuf;

use crate::policy::PolicyKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherMode {
    Periodic,
    Threshold,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatcherThresholds {
    pub max_target_size_bytes: Option<u64>,
    pub disk_free_below_basis_points: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherDecisionInput {
    pub enabled: bool,
    pub mode: WatcherMode,
    pub thresholds: WatcherThresholds,
    pub observed_targets: Vec<WatcherObservedTarget>,
    pub disk_free_basis_points: Option<u16>,
    pub selected_policy: PolicyKind,
    pub unattended_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherObservedTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherDecision {
    pub state: WatcherDecisionState,
    pub reasons: Vec<WatcherTriggerReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherDecisionState {
    Inactive,
    NonThresholdMode,
    NotTriggered,
    TriggeredPlanOnly,
    TriggeredPlanAndApply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherTriggerReason {
    TargetSizeExceeded {
        path: PathBuf,
        size_bytes: u64,
        max_target_size_bytes: u64,
    },
    DiskFreeBelow {
        free_basis_points: u16,
        threshold_basis_points: u16,
    },
}

pub fn decide_watcher_thresholds(input: WatcherDecisionInput) -> WatcherDecision {
    if !input.enabled {
        return WatcherDecision {
            state: WatcherDecisionState::Inactive,
            reasons: Vec::new(),
        };
    }

    if input.mode != WatcherMode::Threshold {
        return WatcherDecision {
            state: WatcherDecisionState::NonThresholdMode,
            reasons: Vec::new(),
        };
    }

    let reasons = threshold_reasons(&input);
    if reasons.is_empty() {
        return WatcherDecision {
            state: WatcherDecisionState::NotTriggered,
            reasons,
        };
    }

    let state = if input.selected_policy != PolicyKind::Observe && input.unattended_allowed {
        WatcherDecisionState::TriggeredPlanAndApply
    } else {
        WatcherDecisionState::TriggeredPlanOnly
    };

    WatcherDecision { state, reasons }
}

fn threshold_reasons(input: &WatcherDecisionInput) -> Vec<WatcherTriggerReason> {
    let mut reasons = Vec::new();

    if let Some(max_target_size_bytes) = input.thresholds.max_target_size_bytes {
        reasons.extend(
            input
                .observed_targets
                .iter()
                .filter(|target| target.size_bytes > max_target_size_bytes)
                .map(|target| WatcherTriggerReason::TargetSizeExceeded {
                    path: target.path.clone(),
                    size_bytes: target.size_bytes,
                    max_target_size_bytes,
                }),
        );
    }

    if let (Some(free_basis_points), Some(threshold_basis_points)) = (
        input.disk_free_basis_points,
        input.thresholds.disk_free_below_basis_points,
    ) && free_basis_points < threshold_basis_points
    {
        reasons.push(WatcherTriggerReason::DiskFreeBelow {
            free_basis_points,
            threshold_basis_points,
        });
    }

    reasons
}
