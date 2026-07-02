use std::path::Path;

use crate::{ReclaimError, ReclaimResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskFreeSpace {
    pub available_bytes: u64,
    pub total_bytes: u64,
}

impl DiskFreeSpace {
    pub fn free_basis_points(self) -> Option<u16> {
        if self.total_bytes == 0 {
            return None;
        }
        let basis_points =
            (u128::from(self.available_bytes) * 10_000) / u128::from(self.total_bytes);
        u16::try_from(basis_points.min(10_000)).ok()
    }
}

pub fn disk_free_space(path: impl AsRef<Path>) -> ReclaimResult<DiskFreeSpace> {
    let path = path.as_ref();
    let available_bytes = fs2::available_space(path).map_err(|error| ReclaimError::DiskRead {
        path: path.to_path_buf(),
        message: format!("failed to measure available disk space: {error}"),
    })?;
    let total_bytes = fs2::total_space(path).map_err(|error| ReclaimError::DiskRead {
        path: path.to_path_buf(),
        message: format!("failed to measure total disk space: {error}"),
    })?;
    Ok(DiskFreeSpace {
        available_bytes,
        total_bytes,
    })
}

pub fn disk_free_basis_points(path: impl AsRef<Path>) -> ReclaimResult<Option<u16>> {
    Ok(disk_free_space(path)?.free_basis_points())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_basis_points_with_integer_floor() {
        assert_eq!(
            DiskFreeSpace {
                available_bytes: 1,
                total_bytes: 4
            }
            .free_basis_points(),
            Some(2_500)
        );
    }

    #[test]
    fn ignores_zero_total_space() {
        assert_eq!(
            DiskFreeSpace {
                available_bytes: 1,
                total_bytes: 0
            }
            .free_basis_points(),
            None
        );
    }
}
