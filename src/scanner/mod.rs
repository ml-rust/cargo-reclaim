mod cargo;
mod cargo_config;
mod filesystem;
mod foundation;
mod recursive;
mod targets;

pub use cargo::detect_cargo_project;
pub use cargo_config::{
    CargoConfigProblem, CargoConfigUnsupported, CargoConfigUnsupportedReason, CargoOutputDirs,
    resolve_project_output_dirs, resolve_project_output_dirs_with_env,
};
pub use foundation::{CargoProject, ScannerOptions, TargetDirOverride, TargetDirOverrideSource};
pub use recursive::{ScanItem, ScanSkip, ScanSkipReason, scan_roots};
pub use targets::{SkipReason, TargetCandidate, TargetCandidateKind, classify_target_candidate};
