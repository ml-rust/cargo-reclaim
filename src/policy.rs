use crate::model::{ArtifactClass, PlanAction, TargetEvidence};

const DEFAULT_PROTECTED_OUTPUTS: &[ArtifactClass] = &[
    ArtifactClass::WholeTarget,
    ArtifactClass::Docs,
    ArtifactClass::Package,
    ArtifactClass::Timings,
    ArtifactClass::FinalExecutable,
    ArtifactClass::FinalLibrary,
    ArtifactClass::FinalRlib,
    ArtifactClass::FinalWasm,
    ArtifactClass::Unknown,
];

/// Final build artifacts that the `Sweep` policy may reclaim once they are older
/// than the sweep age threshold (the planner applies that age gate). Everything
/// else in `DEFAULT_PROTECTED_OUTPUTS` stays protected under `Sweep` too.
const SWEEP_FINAL_ARTIFACTS: &[ArtifactClass] = &[
    ArtifactClass::FinalExecutable,
    ArtifactClass::FinalLibrary,
    ArtifactClass::FinalRlib,
    ArtifactClass::FinalWasm,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolicyKind {
    Observe,
    Conservative,
    #[default]
    Balanced,
    Aggressive,
    /// cargo-sweep-style reclamation: the balanced removable set plus cold final
    /// binaries (age-gated by the planner). Never deletes whole targets, docs,
    /// packages, or unknown files.
    Sweep,
    Custom,
}

impl PolicyKind {
    pub fn default_protected_outputs() -> &'static [ArtifactClass] {
        DEFAULT_PROTECTED_OUTPUTS
    }

    pub fn is_default_protected_output(artifact_class: ArtifactClass) -> bool {
        DEFAULT_PROTECTED_OUTPUTS.contains(&artifact_class)
    }

    /// A final binary the `Sweep` policy may reclaim when it is cold enough.
    pub fn is_sweep_final_artifact(artifact_class: ArtifactClass) -> bool {
        SWEEP_FINAL_ARTIFACTS.contains(&artifact_class)
    }

    /// Outputs this policy protects unconditionally (before any age gate). Same
    /// as the default set, except `Sweep` releases cold final binaries so the
    /// planner can age-gate them.
    pub fn is_protected_output(self, artifact_class: ArtifactClass) -> bool {
        if self == Self::Sweep && Self::is_sweep_final_artifact(artifact_class) {
            return false;
        }
        Self::is_default_protected_output(artifact_class)
    }

    pub fn is_default_removable_class(artifact_class: ArtifactClass) -> bool {
        matches!(
            artifact_class,
            ArtifactClass::Incremental
                | ArtifactClass::BuildScripts
                | ArtifactClass::Fingerprint
                | ArtifactClass::Tmp
                | ArtifactClass::FingerprintGroupIntermediate
                | ArtifactClass::StaleDeps
                | ArtifactClass::StaleIncremental
                | ArtifactClass::DepsOutput
                | ArtifactClass::DepInfo
                | ArtifactClass::ObjectMetadata
        )
    }

    pub fn is_conservative_removable_class(artifact_class: ArtifactClass) -> bool {
        matches!(
            artifact_class,
            ArtifactClass::Incremental | ArtifactClass::Tmp
        )
    }

    pub fn allows_delete(
        self,
        action: &PlanAction,
        artifact_class: ArtifactClass,
        evidence: &TargetEvidence,
    ) -> bool {
        match self {
            Self::Observe => false,
            Self::Conservative => {
                action.is_delete()
                    && Self::is_conservative_removable_class(artifact_class)
                    && evidence.meets_default_delete_confidence()
            }
            Self::Balanced | Self::Aggressive | Self::Custom => {
                action.is_delete()
                    && Self::is_default_removable_class(artifact_class)
                    && evidence.meets_default_delete_confidence()
            }
            Self::Sweep => {
                action.is_delete()
                    && (Self::is_default_removable_class(artifact_class)
                        || Self::is_sweep_final_artifact(artifact_class))
                    && evidence.meets_default_delete_confidence()
            }
        }
    }
}
