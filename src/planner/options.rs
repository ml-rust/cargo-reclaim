use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerOptions {
    pub recent_write_keep_window: Option<Duration>,
}
