use super::foundation::{ActiveObservationProvider, ActiveObservationScope};

pub fn platform_active_observation_provider() -> impl ActiveObservationProvider {
    PlatformActiveObservationProvider
}

struct PlatformActiveObservationProvider;

impl ActiveObservationProvider for PlatformActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> crate::planner::ActiveObservation {
        #[cfg(target_os = "linux")]
        {
            super::procfs::ProcfsActiveObservationProvider::default().observe(scope)
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = scope;
            crate::planner::ActiveObservation::not_attempted()
        }
    }
}
