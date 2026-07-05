use std::ffi::OsStr;

use super::foundation::{ActiveObservationProvider, ActiveObservationScope};

pub const DISABLE_ACTIVE_PROCESS_DETECTION_ENV: &str =
    "CARGO_RECLAIM_DISABLE_ACTIVE_PROCESS_DETECTION";

pub fn platform_active_observation_provider() -> impl ActiveObservationProvider {
    PlatformActiveObservationProvider
}

struct PlatformActiveObservationProvider;

impl ActiveObservationProvider for PlatformActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> crate::planner::ActiveObservation {
        if active_process_detection_disabled() {
            return crate::planner::ActiveObservation::not_attempted();
        }

        #[cfg(target_os = "linux")]
        {
            super::procfs::ProcfsActiveObservationProvider::default().observe(scope)
        }

        #[cfg(not(target_os = "linux"))]
        {
            super::sysinfo_provider::SysinfoActiveObservationProvider.observe(scope)
        }
    }
}

fn active_process_detection_disabled() -> bool {
    std::env::var_os(DISABLE_ACTIVE_PROCESS_DETECTION_ENV)
        .as_deref()
        .is_some_and(disable_env_value_is_truthy)
}

fn disable_env_value_is_truthy(value: &OsStr) -> bool {
    let value = value.to_string_lossy();
    let value = value.trim();
    !value.is_empty()
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::disable_env_value_is_truthy;

    #[test]
    fn disable_active_process_detection_env_parses_truthy_values() {
        assert!(disable_env_value_is_truthy(OsStr::new("1")));
        assert!(disable_env_value_is_truthy(OsStr::new("true")));
        assert!(disable_env_value_is_truthy(OsStr::new("yes")));
        assert!(disable_env_value_is_truthy(OsStr::new("ON")));
    }

    #[test]
    fn disable_active_process_detection_env_ignores_falsey_values() {
        assert!(!disable_env_value_is_truthy(OsStr::new("")));
        assert!(!disable_env_value_is_truthy(OsStr::new("0")));
        assert!(!disable_env_value_is_truthy(OsStr::new("false")));
        assert!(!disable_env_value_is_truthy(OsStr::new("no")));
        assert!(!disable_env_value_is_truthy(OsStr::new("off")));
    }
}
