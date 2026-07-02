use crate::model::{ArtifactClass, PathSnapshot, TargetEvidence};

use super::TargetContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerCandidate {
    pub snapshot: PathSnapshot,
    pub artifact_class: ArtifactClass,
    pub evidence: TargetEvidence,
    pub target_context: Option<TargetContext>,
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
        }
    }

    pub fn with_target_context(mut self, target_context: TargetContext) -> Self {
        self.target_context = Some(target_context);
        self
    }
}
