use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hash::format_sha256;

use super::error::PlanPersistenceResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanId(pub String);

impl PlanId {
    pub fn from_body<T>(body: &T) -> PlanPersistenceResult<Self>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec(body)?;
        let digest = Sha256::digest(bytes);
        Ok(Self(format_sha256(&digest)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
