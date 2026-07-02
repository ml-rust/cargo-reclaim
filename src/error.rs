use std::error::Error;
use std::fmt;
use std::path::PathBuf;

pub type ReclaimResult<T> = Result<T, ReclaimError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimError {
    EmptyPath,
    EmptyEvidence,
    EmptyPolicyReason,
    AbsoluteInventoryChildPath { path: PathBuf },
    InventoryPathEscape { path: PathBuf },
    InventorySymlinkNotFollowed { path: PathBuf },
    MissingInventoryPath { path: PathBuf },
    InventoryRead { path: PathBuf, message: String },
    DiskRead { path: PathBuf, message: String },
}

impl fmt::Display for ReclaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("path must not be empty"),
            Self::EmptyEvidence => formatter.write_str("target evidence must not be empty"),
            Self::EmptyPolicyReason => formatter.write_str("policy reason must not be empty"),
            Self::AbsoluteInventoryChildPath { path } => {
                write!(
                    formatter,
                    "inventory child path must be relative: {}",
                    path.display()
                )
            }
            Self::InventoryPathEscape { path } => {
                write!(
                    formatter,
                    "inventory child path must not escape target root: {}",
                    path.display()
                )
            }
            Self::InventorySymlinkNotFollowed { path } => {
                write!(
                    formatter,
                    "inventory path is a symlink and symlink following is disabled: {}",
                    path.display()
                )
            }
            Self::MissingInventoryPath { path } => {
                write!(
                    formatter,
                    "inventory path does not exist: {}",
                    path.display()
                )
            }
            Self::InventoryRead { path, message } => {
                write!(
                    formatter,
                    "failed to read inventory path {}: {message}",
                    path.display()
                )
            }
            Self::DiskRead { path, message } => {
                write!(
                    formatter,
                    "failed to read disk metrics for {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ReclaimError {}
