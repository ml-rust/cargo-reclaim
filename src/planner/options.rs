use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerOptions {
    pub recent_write_keep_window: Option<Duration>,
    pub keep_size_bytes: Option<u64>,
    pub keep_rustc_hashes: Vec<u64>,
    pub keep_installed_toolchains: bool,
    pub keep_toolchains: Vec<String>,
    pub whole_target_mode: WholeTargetMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WholeTargetMode {
    #[default]
    Off,
    Confirm,
    DeleteConfirmed,
}
