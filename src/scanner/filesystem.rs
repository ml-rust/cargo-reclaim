use std::fs;
use std::path::Path;

use super::foundation::ScannerOptions;

/// The device of `path`, or `None` when cross-filesystem scanning is enabled (in
/// which case no boundary is enforced) or the device cannot be determined.
#[cfg(unix)]
pub(crate) fn filesystem_device(path: &Path, options: &ScannerOptions) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    if options.cross_filesystems {
        return None;
    }

    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(not(unix))]
pub(crate) fn filesystem_device(_path: &Path, _options: &ScannerOptions) -> Option<u64> {
    None
}

/// Whether a *traversal* entry sits on a different filesystem than the scan
/// root. This guard stops incidental recursion from wandering across mount
/// points; it is deliberately not applied to explicitly-configured output dirs,
/// which are named locations the user wants discovered wherever they live.
#[cfg(unix)]
pub(crate) fn is_cross_filesystem(
    metadata: &fs::Metadata,
    options: &ScannerOptions,
    root_device: Option<u64>,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    crosses_boundary(options.cross_filesystems, Some(metadata.dev()), root_device)
}

#[cfg(not(unix))]
pub(crate) fn is_cross_filesystem(
    _metadata: &fs::Metadata,
    _options: &ScannerOptions,
    _root_device: Option<u64>,
) -> bool {
    false
}

/// Pure device-comparison core: an entry crosses the boundary when cross-filesystem
/// scanning is off and its device is known and differs from the root's.
fn crosses_boundary(
    cross_filesystems: bool,
    entry_device: Option<u64>,
    root_device: Option<u64>,
) -> bool {
    if cross_filesystems {
        return false;
    }
    matches!(
        (entry_device, root_device),
        (Some(entry), Some(root)) if entry != root
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_on_a_foreign_device_crosses_the_boundary() {
        assert!(crosses_boundary(false, Some(2), Some(1)));
    }

    #[test]
    fn entry_on_the_root_device_does_not_cross() {
        assert!(!crosses_boundary(false, Some(1), Some(1)));
    }

    #[test]
    fn cross_filesystem_scanning_disables_the_boundary() {
        assert!(!crosses_boundary(true, Some(2), Some(1)));
    }

    #[test]
    fn unknown_root_device_never_crosses() {
        assert!(!crosses_boundary(false, Some(2), None));
    }
}
