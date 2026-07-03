use std::path::PathBuf;

use cargo_reclaim::{
    PolicyKind, WatcherDecisionInput, WatcherDecisionState, WatcherMode, WatcherObservedTarget,
    WatcherThresholds, WatcherTriggerReason, decide_watcher_thresholds,
};

fn observed_target(path: &str, size_bytes: u64) -> WatcherObservedTarget {
    WatcherObservedTarget {
        path: PathBuf::from(path),
        size_bytes,
    }
}

fn threshold_input() -> WatcherDecisionInput {
    WatcherDecisionInput {
        enabled: true,
        mode: WatcherMode::Threshold,
        thresholds: WatcherThresholds {
            max_target_size_bytes: Some(100),
            disk_free_below_basis_points: Some(1_500),
            min_free_disk_bytes: Some(1_000),
        },
        observed_targets: vec![observed_target("/workspace/a/target", 90)],
        disk_free_basis_points: Some(2_000),
        disk_free_bytes: Some(2_000),
        selected_policy: PolicyKind::Balanced,
        unattended_allowed: false,
    }
}

#[test]
fn inactive_background_does_not_trigger() {
    let mut input = threshold_input();
    input.enabled = false;
    input.observed_targets = vec![observed_target("/workspace/a/target", 200)];
    input.disk_free_basis_points = Some(1_000);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::Inactive);
    assert!(decision.reasons.is_empty());
}

#[test]
fn periodic_background_is_not_threshold_decision() {
    let mut input = threshold_input();
    input.mode = WatcherMode::Periodic;
    input.observed_targets = vec![observed_target("/workspace/a/target", 200)];
    input.disk_free_basis_points = Some(1_000);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::NonThresholdMode);
    assert!(decision.reasons.is_empty());
}

#[test]
fn no_crossed_thresholds_does_not_trigger() {
    let decision = decide_watcher_thresholds(threshold_input());

    assert_eq!(decision.state, WatcherDecisionState::NotTriggered);
    assert!(decision.reasons.is_empty());
}

#[test]
fn target_size_above_threshold_triggers_plan_only() {
    let mut input = threshold_input();
    input.observed_targets = vec![observed_target("/workspace/a/target", 101)];

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanOnly);
    assert_eq!(
        decision.reasons,
        [WatcherTriggerReason::TargetSizeExceeded {
            path: PathBuf::from("/workspace/a/target"),
            size_bytes: 101,
            max_target_size_bytes: 100
        }]
    );
}

#[test]
fn disk_free_below_threshold_triggers_plan_only() {
    let mut input = threshold_input();
    input.disk_free_basis_points = Some(1_499);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanOnly);
    assert_eq!(
        decision.reasons,
        [WatcherTriggerReason::DiskFreeBelow {
            free_basis_points: 1_499,
            threshold_basis_points: 1_500
        }]
    );
}

#[test]
fn disk_free_bytes_below_threshold_triggers_plan_only() {
    let mut input = threshold_input();
    input.disk_free_bytes = Some(999);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanOnly);
    assert_eq!(
        decision.reasons,
        [WatcherTriggerReason::DiskFreeBytesBelow {
            free_bytes: 999,
            min_free_disk_bytes: 1_000
        }]
    );
}

#[test]
fn equal_thresholds_do_not_trigger() {
    let mut input = threshold_input();
    input.observed_targets = vec![observed_target("/workspace/a/target", 100)];
    input.disk_free_basis_points = Some(1_500);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::NotTriggered);
    assert!(decision.reasons.is_empty());
}

#[test]
fn missing_disk_metric_does_not_trigger() {
    let mut input = threshold_input();
    input.thresholds.max_target_size_bytes = None;
    input.disk_free_basis_points = None;
    input.disk_free_bytes = None;

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::NotTriggered);
    assert!(decision.reasons.is_empty());
}

#[test]
fn multiple_crossed_thresholds_preserve_reasons() {
    let mut input = threshold_input();
    input.observed_targets = vec![
        observed_target("/workspace/a/target", 101),
        observed_target("/workspace/b/target", 80),
        observed_target("/workspace/c/target", 120),
    ];
    input.disk_free_basis_points = Some(1_000);

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanOnly);
    assert_eq!(
        decision.reasons,
        [
            WatcherTriggerReason::TargetSizeExceeded {
                path: PathBuf::from("/workspace/a/target"),
                size_bytes: 101,
                max_target_size_bytes: 100
            },
            WatcherTriggerReason::TargetSizeExceeded {
                path: PathBuf::from("/workspace/c/target"),
                size_bytes: 120,
                max_target_size_bytes: 100
            },
            WatcherTriggerReason::DiskFreeBelow {
                free_basis_points: 1_000,
                threshold_basis_points: 1_500
            }
        ]
    );
}

#[test]
fn observe_policy_keeps_crossed_threshold_plan_only() {
    let mut input = threshold_input();
    input.observed_targets = vec![observed_target("/workspace/a/target", 101)];
    input.selected_policy = PolicyKind::Observe;
    input.unattended_allowed = true;

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanOnly);
}

#[test]
fn unattended_allowed_with_non_observe_policy_triggers_plan_and_apply() {
    let mut input = threshold_input();
    input.observed_targets = vec![observed_target("/workspace/a/target", 101)];
    input.unattended_allowed = true;

    let decision = decide_watcher_thresholds(input);

    assert_eq!(decision.state, WatcherDecisionState::TriggeredPlanAndApply);
}
