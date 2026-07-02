use crate::model::{ArtifactClass, PathSnapshot, TargetEvidence};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerCandidate {
    pub snapshot: PathSnapshot,
    pub artifact_class: ArtifactClass,
    pub evidence: TargetEvidence,
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
        }
    }
}
