use std::error::Error;
use std::fmt;

pub type ReclaimResult<T> = Result<T, ReclaimError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimError {
    EmptyPath,
    EmptyEvidence,
    EmptyPolicyReason,
}

impl fmt::Display for ReclaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("path must not be empty"),
            Self::EmptyEvidence => formatter.write_str("target evidence must not be empty"),
            Self::EmptyPolicyReason => formatter.write_str("policy reason must not be empty"),
        }
    }
}

impl Error for ReclaimError {}
