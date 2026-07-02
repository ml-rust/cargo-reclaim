use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use cargo_reclaim::{BackgroundMode, WholeTargetConfig, parse_config};

#[test]
fn reclaim_example_config_parses_through_the_real_config_parser() -> Result<(), Box<dyn Error>> {
    let config = parse_config(include_str!("../examples/reclaim.toml"))?;

    assert_eq!(config.version, 1);
    assert_eq!(config.roots, [PathBuf::from("projects")]);
    assert_eq!(config.ignored_paths, [PathBuf::from("projects/pinned")]);
    assert_eq!(config.skipped_paths, [PathBuf::from("projects/vendor")]);
    assert_eq!(config.policy.as_deref(), Some("conservative"));
    assert_eq!(config.whole_target, Some(WholeTargetConfig::Off));
    assert_eq!(config.allow_unattended_whole_target_delete, Some(false));
    assert_eq!(
        config.policy_thresholds.max_target_size_bytes,
        Some(5 * 1024 * 1024 * 1024)
    );
    assert_eq!(config.policy_thresholds.unattended_allowed, Some(false));
    assert_eq!(config.scanner.follow_symlinks, Some(true));
    assert_eq!(config.scanner.allow_name_only_targets, Some(true));
    assert_eq!(config.scanner.cross_filesystems, Some(true));
    assert_eq!(
        config.recent_write_keep_window,
        Some(Duration::from_secs(4 * 60 * 60))
    );
    assert_eq!(config.keep_size_bytes, Some(64 * 1024 * 1024));
    assert!(config.keep_rustc_hashes.is_empty());
    assert_eq!(config.scheduler.at.as_deref(), Some("04:15"));
    assert_eq!(config.scheduler.mode.as_deref(), Some("observe"));
    assert_eq!(config.scheduler.policy.as_deref(), Some("conservative"));
    assert_eq!(config.scheduler.allow_unattended_cleanup, Some(false));
    assert_eq!(config.scheduler.allow_unattended_high_policy, Some(false));
    assert_eq!(config.scheduler.state_dir, Some("state".into()));
    assert_eq!(config.scheduler.log_dir, Some("logs".into()));
    assert_eq!(config.background.enabled, Some(false));
    assert_eq!(config.background.mode, Some(BackgroundMode::Threshold));
    assert_eq!(
        config.background.check_every,
        Some(Duration::from_secs(15 * 60))
    );
    assert_eq!(
        config.background.only_when_disk_free_below_basis_points,
        Some(1250)
    );

    Ok(())
}
