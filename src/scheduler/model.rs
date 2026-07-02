use std::fmt;
use std::path::PathBuf;

use crate::PolicyKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerPlatform {
    SystemdUser,
    Launchd,
    TaskScheduler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerMode {
    Observe,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub hour: u8,
    pub minute: u8,
}

impl Schedule {
    pub fn parse(value: &str) -> Result<Self, SchedulerError> {
        let Some((hour, minute)) = value.split_once(':') else {
            return Err(SchedulerError::InvalidSchedule(value.to_string()));
        };
        if hour.len() != 2 || minute.len() != 2 {
            return Err(SchedulerError::InvalidSchedule(value.to_string()));
        }
        let hour = hour
            .parse::<u8>()
            .map_err(|_| SchedulerError::InvalidSchedule(value.to_string()))?;
        let minute = minute
            .parse::<u8>()
            .map_err(|_| SchedulerError::InvalidSchedule(value.to_string()))?;
        if hour > 23 || minute > 59 {
            return Err(SchedulerError::InvalidSchedule(value.to_string()));
        }
        Ok(Self { hour, minute })
    }

    pub fn as_hh_mm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

impl Default for Schedule {
    fn default() -> Self {
        Self { hour: 3, minute: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerRequest {
    pub platform: SchedulerPlatform,
    pub config_path: PathBuf,
    pub cargo_reclaim_bin: PathBuf,
    pub schedule: Schedule,
    pub mode: SchedulerMode,
    pub policy: Option<PolicyKind>,
    pub allow_unattended_cleanup: bool,
    pub allow_unattended_high_policy: bool,
    pub state_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedArtifactKind {
    SystemdService,
    SystemdTimer,
    LaunchdPlist,
    TaskSchedulerXml,
    RunnerScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    pub kind: GeneratedArtifactKind,
    pub intended_install_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerReport {
    pub command: &'static str,
    pub dry_run: bool,
    pub platform: SchedulerPlatform,
    pub mode: SchedulerMode,
    pub schedule: Schedule,
    pub effective_policy: PolicyKind,
    pub artifacts: Vec<GeneratedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    InvalidSchedule(String),
    CleanupNotAllowed,
    HighPolicyNotAllowed(PolicyKind),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchedule(value) => write!(
                formatter,
                "invalid scheduler time `{value}`; expected HH:MM in 24-hour time"
            ),
            Self::CleanupNotAllowed => formatter.write_str(
                "scheduler cleanup preview requires --allow-unattended-cleanup or [scheduler].allow_unattended_cleanup = true",
            ),
            Self::HighPolicyNotAllowed(policy) => write!(
                formatter,
                "scheduler cleanup with {} policy requires an explicit policy and --allow-unattended-high-policy or [scheduler].allow_unattended_high_policy = true",
                policy_label(*policy)
            ),
        }
    }
}

impl std::error::Error for SchedulerError {}

pub(crate) fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}
