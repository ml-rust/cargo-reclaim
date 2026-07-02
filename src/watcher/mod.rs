mod decision;

pub use decision::{
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherMode,
    WatcherObservedTarget, WatcherThresholds, WatcherTriggerReason, decide_watcher_thresholds,
};
