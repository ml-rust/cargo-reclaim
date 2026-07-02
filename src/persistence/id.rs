use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::document::PersistedPlanBody;
use super::error::PlanPersistenceResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanId(pub String);

impl PlanId {
    pub fn from_body(body: &PersistedPlanBody) -> PlanPersistenceResult<Self> {
        let bytes = serde_json::to_vec(body)?;
        let digest = Sha256::digest(bytes);
        Ok(Self(format!("sha256:{digest:x}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
