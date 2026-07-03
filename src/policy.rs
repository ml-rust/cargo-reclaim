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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolicyKind {
    Observe,
    Conservative,
    #[default]
    Balanced,
    Aggressive,
    Custom,
}

impl PolicyKind {
    pub fn default_protected_outputs() -> &'static [ArtifactClass] {
        DEFAULT_PROTECTED_OUTPUTS
    }

    pub fn is_default_protected_output(artifact_class: ArtifactClass) -> bool {
        DEFAULT_PROTECTED_OUTPUTS.contains(&artifact_class)
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
        }
    }
}
