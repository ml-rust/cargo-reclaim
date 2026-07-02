use std::path::PathBuf;

use super::model::{SchedulerPlatform, SchedulerRequest};

pub(crate) struct SchedulerPaths {
    pub(crate) state_dir: PathBuf,
    pub(crate) log_dir: PathBuf,
    pub(crate) plans_dir: PathBuf,
    pub(crate) runner_path: PathBuf,
    pub(crate) log_path: PathBuf,
}

impl SchedulerPaths {
    pub(crate) fn new(request: &SchedulerRequest) -> Self {
        let state_dir = request
            .state_dir
            .clone()
            .unwrap_or_else(|| default_state_dir(request.platform));
        let log_dir = request
            .log_dir
            .clone()
            .unwrap_or_else(|| default_log_dir(request.platform));
        let runner_path = match request.platform {
            SchedulerPlatform::SystemdUser => state_dir.join("scheduler-runner.sh"),
            SchedulerPlatform::Launchd => state_dir.join("scheduler-runner.sh"),
            SchedulerPlatform::TaskScheduler => state_dir.join("scheduler-runner.ps1"),
        };
        Self {
            plans_dir: state_dir.join("plans"),
            log_path: log_dir.join("scheduler.log"),
            state_dir,
            log_dir,
            runner_path,
        }
    }

    pub(crate) fn task_scheduler_xml_path(&self) -> PathBuf {
        self.state_dir.join("cargo-reclaim.xml")
    }

    pub(crate) fn systemd_service_path(&self) -> PathBuf {
        home_dir()
            .map(|home| home.join(".config/systemd/user/cargo-reclaim.service"))
            .unwrap_or_else(|| PathBuf::from(".config/systemd/user/cargo-reclaim.service"))
    }

    pub(crate) fn systemd_timer_path(&self) -> PathBuf {
        home_dir()
            .map(|home| home.join(".config/systemd/user/cargo-reclaim.timer"))
            .unwrap_or_else(|| PathBuf::from(".config/systemd/user/cargo-reclaim.timer"))
    }

    pub(crate) fn launchd_plist_path(&self) -> PathBuf {
        home_dir()
            .map(|home| home.join("Library/LaunchAgents/com.cargo-reclaim.plist"))
            .unwrap_or_else(|| PathBuf::from("Library/LaunchAgents/com.cargo-reclaim.plist"))
    }
}

pub(crate) fn default_state_dir(platform: SchedulerPlatform) -> PathBuf {
    match platform {
        SchedulerPlatform::SystemdUser => home_dir()
            .map(|home| home.join(".local/state/cargo-reclaim"))
            .unwrap_or_else(|| PathBuf::from(".cargo-reclaim/state")),
        SchedulerPlatform::Launchd => home_dir()
            .map(|home| home.join("Library/Application Support/cargo-reclaim"))
            .unwrap_or_else(|| PathBuf::from("cargo-reclaim/Application Support")),
        SchedulerPlatform::TaskScheduler => local_app_data_dir()
            .map(|local_app_data| local_app_data.join("cargo-reclaim"))
            .unwrap_or_else(|| PathBuf::from(r"cargo-reclaim")),
    }
}

pub(crate) fn default_log_dir(platform: SchedulerPlatform) -> PathBuf {
    match platform {
        SchedulerPlatform::SystemdUser => default_state_dir(platform).join("logs"),
        SchedulerPlatform::Launchd => home_dir()
            .map(|home| home.join("Library/Logs/cargo-reclaim"))
            .unwrap_or_else(|| PathBuf::from("cargo-reclaim/logs")),
        SchedulerPlatform::TaskScheduler => default_state_dir(platform).join("logs"),
    }
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(crate) fn local_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}
