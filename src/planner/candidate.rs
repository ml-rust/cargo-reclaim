use std::time::SystemTime;

use crate::model::{ArtifactClass, PathSnapshot, TargetEvidence};

use super::TargetContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerCandidate {
    pub snapshot: PathSnapshot,
    pub artifact_class: ArtifactClass,
    pub evidence: TargetEvidence,
    pub target_context: Option<TargetContext>,
    /// Newest artifact mtime across the whole target this candidate belongs to.
    /// A build writes into its target continuously, so this is a race-free signal
    /// that the target has an active build — one the point-in-time process scan
    /// can miss. Set once per target after all candidates are collected; `None`
    /// for direct callers that plan a candidate without a target-wide walk.
    pub target_newest_modified: Option<SystemTime>,
}

impl PlannerCandidate {
    pub fn new(
        snapshot: PathSnapshot,
        artifact_class: ArtifactClass,
        evidence: TargetEvidence,
    ) -> Self {
        Self {
            snapshot,
            artifact_class,
            evidence,
            target_context: None,
            target_newest_modified: None,
        }
    }

    pub fn with_target_context(mut self, target_context: TargetContext) -> Self {
        self.target_context = Some(target_context);
        self
    }
}
