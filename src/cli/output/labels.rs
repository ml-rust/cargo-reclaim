use cargo_reclaim::{
    ArtifactClass, PathKind, PlanAction, PlanSkipReason, PolicyKind, TargetEvidence,
};

pub(super) fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

pub(super) fn action_label(action: &PlanAction) -> &'static str {
    match action {
        PlanAction::Delete => "delete",
        PlanAction::Preserve => "preserve",
        PlanAction::SkipActive => "skip_active",
        PlanAction::SkipLocked => "skip_locked",
        PlanAction::Unknown => "unknown",
        PlanAction::RequiresConfirmation => "requires_confirmation",
    }
}

pub(super) fn artifact_label(artifact_class: ArtifactClass) -> &'static str {
    match artifact_class {
        ArtifactClass::WholeTarget => "whole_target",
        ArtifactClass::Incremental => "incremental",
        ArtifactClass::Deps => "deps",
        ArtifactClass::BuildScripts => "build_scripts",
        ArtifactClass::Fingerprint => "fingerprint",
        ArtifactClass::Docs => "docs",
        ArtifactClass::Package => "package",
        ArtifactClass::Timings => "timings",
        ArtifactClass::Tmp => "tmp",
        ArtifactClass::FingerprintGroupIntermediate => "fingerprint_group_intermediate",
        ArtifactClass::StaleDeps => "stale_deps",
        ArtifactClass::StaleIncremental => "stale_incremental",
        ArtifactClass::DepsOutput => "deps_output",
        ArtifactClass::DepInfo => "dep_info",
        ArtifactClass::ObjectMetadata => "object_metadata",
        ArtifactClass::FinalExecutable => "final_executable",
        ArtifactClass::FinalLibrary => "final_library",
        ArtifactClass::FinalRlib => "final_rlib",
        ArtifactClass::FinalWasm => "final_wasm",
        ArtifactClass::Unknown => "unknown",
    }
}

pub(super) fn path_kind_label(path_kind: PathKind) -> &'static str {
    match path_kind {
        PathKind::File => "file",
        PathKind::Directory => "directory",
        PathKind::Symlink => "symlink",
        PathKind::Unknown => "unknown",
    }
}

pub(super) fn evidence_kind_label(evidence: &TargetEvidence) -> &'static str {
    match evidence {
        TargetEvidence::StrongMarker { .. } => "strong_marker",
        TargetEvidence::ConfiguredPath { .. } => "configured_path",
        TargetEvidence::ProjectContext { .. } => "project_context",
        TargetEvidence::WeakNameOnly { .. } => "weak_name_only",
    }
}

pub(super) fn skip_reason_label(reason: PlanSkipReason) -> &'static str {
    reason.label()
}

#[cfg(test)]
mod tests {
    use cargo_reclaim::{
        ArtifactClass, PathKind, PlanAction, PlanSkipReason, PolicyKind, TargetEvidence,
    };

    use super::{
        action_label, artifact_label, evidence_kind_label, path_kind_label, policy_label,
        skip_reason_label,
    };

    #[test]
    fn labels_cover_current_schema_enums() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(policy_label(PolicyKind::Observe), "observe");
        assert_eq!(policy_label(PolicyKind::Conservative), "conservative");
        assert_eq!(policy_label(PolicyKind::Balanced), "balanced");
        assert_eq!(policy_label(PolicyKind::Aggressive), "aggressive");
        assert_eq!(policy_label(PolicyKind::Custom), "custom");

        assert_eq!(action_label(&PlanAction::Delete), "delete");
        assert_eq!(action_label(&PlanAction::Preserve), "preserve");
        assert_eq!(action_label(&PlanAction::SkipActive), "skip_active");
        assert_eq!(action_label(&PlanAction::SkipLocked), "skip_locked");
        assert_eq!(action_label(&PlanAction::Unknown), "unknown");
        assert_eq!(
            action_label(&PlanAction::RequiresConfirmation),
            "requires_confirmation"
        );

        assert_eq!(artifact_label(ArtifactClass::WholeTarget), "whole_target");
        assert_eq!(artifact_label(ArtifactClass::Incremental), "incremental");
        assert_eq!(artifact_label(ArtifactClass::Deps), "deps");
        assert_eq!(artifact_label(ArtifactClass::BuildScripts), "build_scripts");
        assert_eq!(artifact_label(ArtifactClass::Fingerprint), "fingerprint");
        assert_eq!(artifact_label(ArtifactClass::Docs), "docs");
        assert_eq!(artifact_label(ArtifactClass::Package), "package");
        assert_eq!(artifact_label(ArtifactClass::Timings), "timings");
        assert_eq!(artifact_label(ArtifactClass::Tmp), "tmp");
        assert_eq!(
            artifact_label(ArtifactClass::FingerprintGroupIntermediate),
            "fingerprint_group_intermediate"
        );
        assert_eq!(artifact_label(ArtifactClass::StaleDeps), "stale_deps");
        assert_eq!(
            artifact_label(ArtifactClass::StaleIncremental),
            "stale_incremental"
        );
        assert_eq!(artifact_label(ArtifactClass::DepsOutput), "deps_output");
        assert_eq!(artifact_label(ArtifactClass::DepInfo), "dep_info");
        assert_eq!(
            artifact_label(ArtifactClass::ObjectMetadata),
            "object_metadata"
        );
        assert_eq!(
            artifact_label(ArtifactClass::FinalExecutable),
            "final_executable"
        );
        assert_eq!(artifact_label(ArtifactClass::FinalLibrary), "final_library");
        assert_eq!(artifact_label(ArtifactClass::FinalRlib), "final_rlib");
        assert_eq!(artifact_label(ArtifactClass::FinalWasm), "final_wasm");
        assert_eq!(artifact_label(ArtifactClass::Unknown), "unknown");

        assert_eq!(path_kind_label(PathKind::File), "file");
        assert_eq!(path_kind_label(PathKind::Directory), "directory");
        assert_eq!(path_kind_label(PathKind::Symlink), "symlink");
        assert_eq!(path_kind_label(PathKind::Unknown), "unknown");

        assert_eq!(
            evidence_kind_label(&TargetEvidence::strong_marker("CACHEDIR.TAG")?),
            "strong_marker"
        );
        assert_eq!(
            evidence_kind_label(&TargetEvidence::configured_path("config")?),
            "configured_path"
        );
        assert_eq!(
            evidence_kind_label(&TargetEvidence::project_context("Cargo.toml")?),
            "project_context"
        );
        assert_eq!(
            evidence_kind_label(&TargetEvidence::weak_name_only("target")?),
            "weak_name_only"
        );

        assert_eq!(
            skip_reason_label(PlanSkipReason::DefaultIgnoredDir),
            "default_ignored_dir"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::ConfiguredIgnoredPath),
            "configured_ignored_path"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::SymlinkNotFollowed),
            "symlink_not_followed"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::CrossFilesystem),
            "cross_filesystem"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::WeakNameOnlySuppressed),
            "weak_name_only_suppressed"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::AlreadyVisited),
            "already_visited"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::CargoConfigUnsupported),
            "cargo_config_unsupported"
        );
        assert_eq!(
            skip_reason_label(PlanSkipReason::CargoConfigProblem),
            "cargo_config_problem"
        );
        assert_eq!(skip_reason_label(PlanSkipReason::ReadError), "read_error");
        Ok(())
    }
}
