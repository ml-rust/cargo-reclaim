use std::fmt;
use std::path::{Component, Path, PathBuf};

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
    pub instance_name: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerOperation {
    Install,
    Uninstall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerOperationPlan {
    pub command: &'static str,
    pub operation: SchedulerOperation,
    pub dry_run: bool,
    pub platform: SchedulerPlatform,
    pub artifacts: Vec<GeneratedArtifact>,
    pub steps: Vec<SchedulerPlanStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerPlanStep {
    EnsureDir {
        path: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        artifact_kind: GeneratedArtifactKind,
    },
    SetExecutable {
        path: PathBuf,
    },
    RemoveFile {
        path: PathBuf,
    },
    RunCommand {
        argv: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerExecutionReport {
    pub command: &'static str,
    pub operation: SchedulerOperation,
    pub dry_run: bool,
    pub platform: SchedulerPlatform,
    pub artifacts: Vec<GeneratedArtifact>,
    pub steps: Vec<SchedulerExecutionStep>,
    pub totals: SchedulerExecutionTotals,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerExecutionStep {
    pub step: SchedulerPlanStep,
    pub status: SchedulerExecutionStatus,
    pub message: Option<String>,
    pub command_output: Option<SchedulerCommandOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerExecutionStatus {
    Applied,
    Skipped,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SchedulerExecutionTotals {
    pub applied: usize,
    pub skipped: usize,
    pub failed: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerCommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl SchedulerExecutionReport {
    pub fn succeeded(&self) -> bool {
        self.totals.failed == 0 && self.totals.blocked == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    InvalidSchedule(String),
    InvalidInstanceName(String),
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
            Self::InvalidInstanceName(value) => write!(
                formatter,
                "invalid scheduler name `{value}`; use only ASCII letters, digits, '-', '_', or '.', and do not use path separators"
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

pub fn scheduler_instance_name_from_config(
    explicit_name: Option<&str>,
    config_path: &Path,
) -> Result<String, SchedulerError> {
    if let Some(name) = explicit_name {
        return validate_scheduler_instance_name(name).map(ToOwned::to_owned);
    }

    Ok(derive_scheduler_instance_name(config_path))
}

pub(crate) fn validate_scheduler_instance_name(name: &str) -> Result<&str, SchedulerError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || !name.chars().all(is_safe_instance_character)
    {
        return Err(SchedulerError::InvalidInstanceName(name.to_string()));
    }
    Ok(name)
}

fn derive_scheduler_instance_name(config_path: &Path) -> String {
    let stem = config_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(sanitize_instance_stem)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "config".to_string());
    format!("{stem}-{:016x}", stable_path_hash(config_path))
}

fn sanitize_instance_stem(stem: &str) -> String {
    stem.chars()
        .map(|character| {
            if is_safe_instance_character(character) {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn is_safe_instance_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

fn stable_path_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for component in path.components() {
        hash_path_component(&mut hash, component);
        hash_byte(&mut hash, 0xff);
    }
    hash
}

fn hash_path_component(hash: &mut u64, component: Component<'_>) {
    for byte in component.as_os_str().to_string_lossy().as_bytes() {
        hash_byte(hash, *byte);
    }
}

fn hash_byte(hash: &mut u64, byte: u8) {
    *hash ^= u64::from(byte);
    *hash = hash.wrapping_mul(0x100000001b3);
}

pub fn policy_label(policy: PolicyKind) -> &'static str {
    match policy {
        PolicyKind::Observe => "observe",
        PolicyKind::Conservative => "conservative",
        PolicyKind::Balanced => "balanced",
        PolicyKind::Aggressive => "aggressive",
        PolicyKind::Custom => "custom",
    }
}

pub fn platform_label(platform: SchedulerPlatform) -> &'static str {
    match platform {
        SchedulerPlatform::SystemdUser => "systemd-user",
        SchedulerPlatform::Launchd => "launchd",
        SchedulerPlatform::TaskScheduler => "task-scheduler",
    }
}

pub fn mode_label(mode: SchedulerMode) -> &'static str {
    match mode {
        SchedulerMode::Observe => "observe",
        SchedulerMode::Cleanup => "cleanup",
    }
}

pub fn operation_label(operation: SchedulerOperation) -> &'static str {
    match operation {
        SchedulerOperation::Install => "install",
        SchedulerOperation::Uninstall => "uninstall",
    }
}

pub fn execution_status_label(status: SchedulerExecutionStatus) -> &'static str {
    match status {
        SchedulerExecutionStatus::Applied => "applied",
        SchedulerExecutionStatus::Skipped => "skipped",
        SchedulerExecutionStatus::Failed => "failed",
        SchedulerExecutionStatus::Blocked => "blocked",
    }
}

pub fn artifact_kind_label(kind: GeneratedArtifactKind) -> &'static str {
    match kind {
        GeneratedArtifactKind::SystemdService => "systemd-service",
        GeneratedArtifactKind::SystemdTimer => "systemd-timer",
        GeneratedArtifactKind::LaunchdPlist => "launchd-plist",
        GeneratedArtifactKind::TaskSchedulerXml => "task-scheduler-xml",
        GeneratedArtifactKind::RunnerScript => "runner-script",
    }
}
