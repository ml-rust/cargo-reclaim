use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerOptions {
    pub recent_write_keep_window: Option<Duration>,
    /// Age below which the `Sweep` policy will not reclaim a final binary
    /// (default 24h when unset). Ignored by other policies.
    pub sweep_older_than: Option<Duration>,
    pub keep_size_bytes: Option<u64>,
    pub target_size_goal_bytes: Option<u64>,
    pub target_free_disk_bytes: Option<u64>,
    pub minimum_reclaim_bytes: Option<u64>,
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
