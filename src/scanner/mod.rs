mod cargo;
mod foundation;
mod recursive;
mod targets;

pub use cargo::detect_cargo_project;
pub use foundation::{CargoProject, ScannerOptions, TargetDirOverride, TargetDirOverrideSource};
pub use recursive::{ScanItem, ScanSkip, ScanSkipReason, scan_roots};
pub use targets::{SkipReason, TargetCandidate, TargetCandidateKind, classify_target_candidate};
