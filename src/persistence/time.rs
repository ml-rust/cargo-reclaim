use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::error::{PlanPersistenceError, PlanPersistenceResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedTimestamp {
    pub unix_seconds: u64,
    pub nanoseconds: u32,
}

impl PersistedTimestamp {
    pub fn from_system_time(time: SystemTime) -> PlanPersistenceResult<Self> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PlanPersistenceError::TimestampBeforeUnixEpoch)?;

        Ok(Self {
            unix_seconds: duration.as_secs(),
            nanoseconds: duration.subsec_nanos(),
        })
    }

    pub fn to_system_time(self) -> SystemTime {
        UNIX_EPOCH + Duration::new(self.unix_seconds, self.nanoseconds)
    }
}
