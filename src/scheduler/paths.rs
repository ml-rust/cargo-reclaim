use std::path::PathBuf;

use super::model::{SchedulerPlatform, SchedulerRequest};

pub(crate) struct SchedulerPaths {
    pub(crate) instance_name: String,
    pub(crate) systemd_service_name: String,
    pub(crate) systemd_timer_name: String,
    pub(crate) launchd_label: String,
    pub(crate) task_name: String,
    pub(crate) state_dir: PathBuf,
    pub(crate) log_dir: PathBuf,
    pub(crate) runner_path: PathBuf,
    pub(crate) log_path: PathBuf,
}

impl SchedulerPaths {
    pub(crate) fn new(request: &SchedulerRequest) -> Self {
        let state_dir = request.state_dir.clone().unwrap_or_else(|| {
            default_instance_state_dir(request.platform, &request.instance_name)
        });
        let log_dir = request
            .log_dir
            .clone()
            .unwrap_or_else(|| default_instance_log_dir(request.platform, &request.instance_name));
        let runner_name = match request.platform {
            SchedulerPlatform::SystemdUser | SchedulerPlatform::Launchd => {
                format!("scheduler-runner-{}.sh", request.instance_name)
            }
            SchedulerPlatform::TaskScheduler => {
                format!("scheduler-runner-{}.ps1", request.instance_name)
            }
        };
        let runner_path = match request.platform {
            SchedulerPlatform::SystemdUser
            | SchedulerPlatform::Launchd
            | SchedulerPlatform::TaskScheduler => state_dir.join(runner_name),
        };
        Self {
            instance_name: request.instance_name.clone(),
            systemd_service_name: format!("cargo-reclaim-{}.service", request.instance_name),
            systemd_timer_name: format!("cargo-reclaim-{}.timer", request.instance_name),
            launchd_label: format!("com.cargo-reclaim.{}", request.instance_name),
            task_name: format!(r"\cargo-reclaim\{}", request.instance_name),
            log_path: log_dir.join("scheduler.log"),
            state_dir,
            log_dir,
            runner_path,
        }
    }

    pub(crate) fn task_scheduler_xml_path(&self) -> PathBuf {
        self.state_dir
            .join(format!("cargo-reclaim-{}.xml", self.instance_name))
    }

    pub(crate) fn systemd_service_path(&self) -> PathBuf {
        home_dir()
            .map(|home| {
                home.join(".config/systemd/user")
                    .join(&self.systemd_service_name)
            })
            .unwrap_or_else(|| {
                PathBuf::from(".config/systemd/user").join(&self.systemd_service_name)
            })
    }

    pub(crate) fn systemd_timer_path(&self) -> PathBuf {
        home_dir()
            .map(|home| {
                home.join(".config/systemd/user")
                    .join(&self.systemd_timer_name)
            })
            .unwrap_or_else(|| PathBuf::from(".config/systemd/user").join(&self.systemd_timer_name))
    }

    pub(crate) fn launchd_plist_path(&self) -> PathBuf {
        home_dir()
            .map(|home| {
                home.join("Library/LaunchAgents")
                    .join(format!("{}.plist", self.launchd_label))
            })
            .unwrap_or_else(|| {
                PathBuf::from("Library/LaunchAgents").join(format!("{}.plist", self.launchd_label))
            })
    }
}

pub fn default_state_dir(platform: SchedulerPlatform) -> PathBuf {
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

pub fn default_log_dir(platform: SchedulerPlatform) -> PathBuf {
    match platform {
        SchedulerPlatform::SystemdUser => default_state_dir(platform).join("logs"),
        SchedulerPlatform::Launchd => home_dir()
            .map(|home| home.join("Library/Logs/cargo-reclaim"))
            .unwrap_or_else(|| PathBuf::from("cargo-reclaim/logs")),
        SchedulerPlatform::TaskScheduler => default_state_dir(platform).join("logs"),
    }
}

pub fn default_instance_state_dir(platform: SchedulerPlatform, instance_name: &str) -> PathBuf {
    default_state_dir(platform).join(instance_name)
}

pub fn default_instance_log_dir(platform: SchedulerPlatform, instance_name: &str) -> PathBuf {
    default_log_dir(platform).join(instance_name)
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(crate) fn local_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}
