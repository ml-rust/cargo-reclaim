use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use cargo_reclaim::{
    BackgroundLimiter, BackgroundRunReport, BackgroundRunRequest, BackgroundRunTrigger,
    DiskFreeSpace, InventoryOptions, PlannerOptions, PolicyKind, ScannerOptions, SchedulerMode,
    WatcherDecision, WatcherDecisionInput, WatcherDecisionState, WatcherMode,
    WatcherObservedTarget, WatcherThresholds, disk_free_space, load_config_from_path,
    platform_active_observation_provider, run_background_cleanup_cycle, scan_roots, snapshot_path,
};
use cargo_reclaim::{ScanItem, TargetCandidateKind, WholeTargetConfig, WholeTargetMode};

use super::super::{
    CliError, OutputFormat, inline_config_path, next_path, next_value, parse_policy,
};
use super::{parse_mode, scheduler_subcommand_usage};

#[derive(Debug)]
pub(in crate::cli) struct SchedulerRunCommand {
    config_path: PathBuf,
    run_id: String,
    log_path: PathBuf,
    plan_path: PathBuf,
    output_format: OutputFormat,
    mode: Option<SchedulerMode>,
    allow_apply: bool,
}

pub(super) fn parse_scheduler_run(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerRunCommand, CliError> {
    let mut config_path = None;
    let mut run_id = None;
    let mut log_path = None;
    let mut plan_path = None;
    let mut output_format = OutputFormat::Terminal;
    let mut mode = None;
    let mut allow_apply = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "scheduler run options must be valid UTF-8".to_string(),
            ));
        };

        match arg_text {
            "--config" => config_path = Some(next_path(&mut args, "--config")?),
            "--run-id" => run_id = Some(next_value(&mut args, "--run-id")?),
            value if value.starts_with("--run-id=") => {
                run_id = Some(value["--run-id=".len()..].to_string());
            }
            "--log-path" => log_path = Some(next_path(&mut args, "--log-path")?),
            value if value.starts_with("--log-path=") => {
                log_path = Some(PathBuf::from(&value["--log-path=".len()..]));
            }
            "--plan-path" => plan_path = Some(next_path(&mut args, "--plan-path")?),
            value if value.starts_with("--plan-path=") => {
                plan_path = Some(PathBuf::from(&value["--plan-path=".len()..]));
            }
            "--mode" => mode = Some(parse_mode(&next_value(&mut args, "--mode")?)?),
            value if value.starts_with("--mode=") => {
                mode = Some(parse_mode(&value["--mode=".len()..])?);
            }
            "--allow-apply" => allow_apply = true,
            "--json" => output_format = OutputFormat::Json,
            "-h" | "--help" => return Err(CliError::Help(scheduler_subcommand_usage("run"))),
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scheduler run option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected scheduler run argument `{value}`"
                )));
            }
        }
    }

    Ok(SchedulerRunCommand {
        config_path: config_path
            .ok_or_else(|| CliError::Usage("scheduler run requires --config".to_string()))?,
        run_id: run_id
            .ok_or_else(|| CliError::Usage("scheduler run requires --run-id".to_string()))?,
        log_path: log_path
            .ok_or_else(|| CliError::Usage("scheduler run requires --log-path".to_string()))?,
        plan_path: plan_path
            .ok_or_else(|| CliError::Usage("scheduler run requires --plan-path".to_string()))?,
        output_format,
        mode,
        allow_apply,
    })
}

pub(super) fn run_scheduler_cycle(
    command: &SchedulerRunCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let config = load_config_from_path(&command.config_path)?;
    for note in &config.deprecations {
        eprintln!("warning: {note}");
    }
    let mode = match command.mode {
        Some(mode) => mode,
        None => config
            .scheduler
            .mode
            .as_deref()
            .map(parse_mode)
            .transpose()?
            .unwrap_or(SchedulerMode::Observe),
    };
    let policy = effective_run_policy(mode, &config)?;
    let allow_apply =
        command.allow_apply || config.scheduler.allow_unattended_cleanup.unwrap_or(false);
    validate_run_apply_policy(mode, allow_apply, policy, &config)?;
    validate_whole_target_policy(policy, &config)?;
    let roots = run_roots(&config);
    let scanner_options = scanner_options_from_config(&config);
    let inventory_options = inventory_options_from_config(&config);
    let planner_options = planner_options_from_config(&config);
    let limiter = effective_background_limiter(&config);
    let observed_targets = if limiter.needs_target_scan() {
        observed_targets_from_roots(&roots, &scanner_options, &inventory_options)?
    } else {
        Vec::new()
    };
    let disk_free_space = if limiter.disk_free_below_basis_points.is_some()
        || limiter.min_free_disk_bytes.is_some()
    {
        observed_disk_free_space(&roots)?
    } else {
        None
    };

    let now = SystemTime::now();
    let request = BackgroundRunRequest {
        run_id: command.run_id.clone(),
        log_path: command.log_path.clone(),
        plan_path: command.plan_path.clone(),
        roots,
        policy,
        scanner_options,
        inventory_options,
        planner_options,
        trigger: BackgroundRunTrigger::Decision(run_decision(
            mode,
            allow_apply,
            &config,
            policy,
            &limiter,
            observed_targets,
            disk_free_space,
        )),
        config_path: Some(command.config_path.clone()),
        config_version: Some(config.version),
        created_at: now,
        now,
        expires_at: now + Duration::from_secs(60 * 60),
    };
    let provider = platform_active_observation_provider();
    let report = run_background_cleanup_cycle(request, &provider)?;
    let exit_code = scheduler_run_exit_code(&report);
    match command.output_format {
        OutputFormat::Terminal => write_scheduler_run_terminal(output, &report)?,
        OutputFormat::Json => write_scheduler_run_json(output, &report)?,
    }
    Ok(exit_code)
}

fn effective_run_policy(
    mode: SchedulerMode,
    config: &cargo_reclaim::ReclaimConfig,
) -> Result<PolicyKind, CliError> {
    let policy = config
        .scheduler
        .policy
        .as_deref()
        .or(config.policy.as_deref())
        .map(parse_policy)
        .transpose()?;
    Ok(policy.unwrap_or(match mode {
        SchedulerMode::Observe => PolicyKind::Observe,
        SchedulerMode::Cleanup => PolicyKind::Conservative,
    }))
}

fn validate_run_apply_policy(
    mode: SchedulerMode,
    allow_apply: bool,
    policy: PolicyKind,
    config: &cargo_reclaim::ReclaimConfig,
) -> Result<(), CliError> {
    if mode == SchedulerMode::Cleanup && !allow_apply {
        return Err(CliError::Scheduler(
            cargo_reclaim::SchedulerError::CleanupNotAllowed,
        ));
    }
    if mode == SchedulerMode::Cleanup
        && allow_apply
        && matches!(
            policy,
            PolicyKind::Balanced | PolicyKind::Aggressive | PolicyKind::Custom
        )
        && !config
            .scheduler
            .allow_unattended_high_policy
            .unwrap_or(false)
    {
        return Err(CliError::Scheduler(
            cargo_reclaim::SchedulerError::HighPolicyNotAllowed(policy),
        ));
    }
    Ok(())
}

fn validate_whole_target_policy(
    policy: PolicyKind,
    config: &cargo_reclaim::ReclaimConfig,
) -> Result<(), CliError> {
    if config.whole_target != Some(WholeTargetConfig::Delete) {
        return Ok(());
    }

    if policy != PolicyKind::Aggressive {
        return Err(CliError::Usage(
            "config whole_target = \"delete\" requires aggressive policy".to_string(),
        ));
    }
    if !config.allow_unattended_whole_target_delete.unwrap_or(false) {
        return Err(CliError::Usage(
            "config whole_target = \"delete\" requires allow_unattended_whole_target_delete = true"
                .to_string(),
        ));
    }

    Ok(())
}

fn run_roots(config: &cargo_reclaim::ReclaimConfig) -> Vec<PathBuf> {
    if config.roots.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        config.roots.clone()
    }
}

fn scanner_options_from_config(config: &cargo_reclaim::ReclaimConfig) -> ScannerOptions {
    ScannerOptions {
        ignored_paths: config.ignored_paths.clone(),
        skipped_paths: config.skipped_paths.clone(),
        follow_symlinks: config.scanner.follow_symlinks.unwrap_or(false),
        allow_name_only_targets: config.scanner.allow_name_only_targets.unwrap_or(false),
        cross_filesystems: config.scanner.cross_filesystems.unwrap_or(false),
    }
}

fn inventory_options_from_config(config: &cargo_reclaim::ReclaimConfig) -> InventoryOptions {
    InventoryOptions {
        follow_symlinks: config.scanner.follow_symlinks.unwrap_or(false),
        skipped_paths: config.skipped_paths.clone(),
        deep_target_scan: false,
        deep_directory_measurement: true,
    }
}

fn planner_options_from_config(config: &cargo_reclaim::ReclaimConfig) -> PlannerOptions {
    PlannerOptions {
        recent_write_keep_window: config.recent_write_keep_window,
        sweep_older_than: config.sweep_older_than,
        interrupt_active_build: false,
        keep_size_bytes: config.keep_size_bytes,
        target_size_goal_bytes: config.policy_thresholds.target_size_goal_bytes,
        target_free_disk_bytes: config.background.target_free_disk_bytes,
        minimum_reclaim_bytes: None,
        keep_rustc_hashes: config.keep_rustc_hashes.clone(),
        keep_installed_toolchains: config.keep_installed_toolchains,
        keep_toolchains: config.keep_toolchains.clone(),
        whole_target_mode: config
            .whole_target
            .map(whole_target_mode_from_config)
            .unwrap_or_default(),
    }
}

fn whole_target_mode_from_config(value: WholeTargetConfig) -> WholeTargetMode {
    match value {
        WholeTargetConfig::Off => WholeTargetMode::Off,
        WholeTargetConfig::Confirm => WholeTargetMode::Confirm,
        WholeTargetConfig::Delete => WholeTargetMode::DeleteConfirmed,
    }
}

/// The limiter that gates a one-shot `scheduler run`. If any configured trigger
/// fires unconditionally (no limiter), the one-shot run does too; otherwise it is
/// gated by the first trigger's limiter. With no triggers it runs unconditionally.
fn effective_background_limiter(config: &cargo_reclaim::ReclaimConfig) -> BackgroundLimiter {
    if config
        .background
        .triggers
        .iter()
        .any(|trigger| trigger.limiter.is_empty())
    {
        return BackgroundLimiter::default();
    }
    if let Some(trigger) = config.background.triggers.first() {
        return trigger.limiter.clone();
    }
    BackgroundLimiter::default()
}

fn run_decision(
    mode: SchedulerMode,
    allow_apply: bool,
    config: &cargo_reclaim::ReclaimConfig,
    policy: PolicyKind,
    limiter: &BackgroundLimiter,
    observed_targets: Vec<WatcherObservedTarget>,
    disk_free_space: Option<DiskFreeSpace>,
) -> WatcherDecision {
    let enabled = config.background.enabled.unwrap_or(true);
    if !enabled {
        return WatcherDecision {
            state: WatcherDecisionState::Inactive,
            reasons: Vec::new(),
        };
    }

    if !limiter.is_empty() {
        return cargo_reclaim::decide_watcher_thresholds(WatcherDecisionInput {
            enabled,
            mode: WatcherMode::Threshold,
            thresholds: WatcherThresholds {
                max_target_size_bytes: limiter.max_target_size_bytes,
                disk_free_below_basis_points: limiter.disk_free_below_basis_points,
                min_free_disk_bytes: limiter.min_free_disk_bytes,
            },
            observed_targets,
            disk_free_basis_points: disk_free_space.and_then(|space| space.free_basis_points()),
            disk_free_bytes: disk_free_space.map(|space| space.available_bytes),
            selected_policy: policy,
            unattended_allowed: mode == SchedulerMode::Cleanup && allow_apply,
        });
    }

    WatcherDecision {
        state: if mode == SchedulerMode::Cleanup && allow_apply && policy != PolicyKind::Observe {
            WatcherDecisionState::TriggeredPlanAndApply
        } else {
            WatcherDecisionState::TriggeredPlanOnly
        },
        reasons: Vec::new(),
    }
}

fn observed_targets_from_roots(
    roots: &[PathBuf],
    scanner_options: &ScannerOptions,
    inventory_options: &InventoryOptions,
) -> Result<Vec<WatcherObservedTarget>, CliError> {
    let items = scan_roots(roots.iter().cloned(), scanner_options)?;
    let mut observed_targets = Vec::new();

    for item in items {
        let ScanItem::TargetCandidate(candidate) = item else {
            continue;
        };
        if candidate.kind != TargetCandidateKind::CargoTargetDir {
            continue;
        }

        let snapshot = snapshot_path(&candidate.path, inventory_options)?;
        observed_targets.push(WatcherObservedTarget {
            path: candidate.path,
            size_bytes: snapshot.size_bytes,
        });
    }

    Ok(observed_targets)
}

fn observed_disk_free_space(roots: &[PathBuf]) -> Result<Option<DiskFreeSpace>, CliError> {
    let root = roots
        .first()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| std::path::Path::new("."));
    disk_free_space(root).map(Some).map_err(CliError::from)
}

fn scheduler_run_exit_code(report: &BackgroundRunReport) -> ExitCode {
    match report.apply_report.as_ref() {
        Some(apply) if apply.totals.failed_count > 0 => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}

fn write_scheduler_run_terminal(
    output: &mut impl Write,
    report: &BackgroundRunReport,
) -> Result<(), CliError> {
    writeln!(output, "cargo-reclaim scheduler run")?;
    writeln!(output, "run id: {}", report.run_id)?;
    if let Some(plan_id) = &report.plan_id {
        writeln!(output, "plan id: {}", plan_id.as_str())?;
    } else {
        writeln!(output, "plan id: none")?;
    }
    if let Some(apply) = &report.apply_report {
        writeln!(output, "delete failures: {}", apply.totals.failed_count)?;
    }
    Ok(())
}

fn write_scheduler_run_json(
    output: &mut impl Write,
    report: &BackgroundRunReport,
) -> Result<(), CliError> {
    let document = serde_json::json!({
        "command": "scheduler-run",
        "run_id": report.run_id,
        "trigger": {
            "state": watcher_state_label(report.decision.state),
            "reason_count": report.decision.reasons.len(),
        },
        "plan_id": report.plan_id.as_ref().map(|id| id.as_str()),
        "apply": report.apply_report.as_ref().map(|apply| serde_json::json!({
            "plan_id": apply.plan_id.as_str(),
            "totals": {
                "entry_count": apply.totals.entry_count,
                "delete_candidate_count": apply.totals.delete_candidate_count,
                "applied_count": apply.totals.applied_count,
                "failed_count": apply.totals.failed_count,
                "skipped_count": apply.totals.skipped_count,
                "stale_skip_count": apply.totals.stale_skip_count,
                "applied_bytes": apply.totals.applied_bytes,
            },
        })),
    });
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

fn watcher_state_label(state: WatcherDecisionState) -> &'static str {
    match state {
        WatcherDecisionState::Inactive => "inactive",
        WatcherDecisionState::NonThresholdMode => "non_threshold_mode",
        WatcherDecisionState::NotTriggered => "not_triggered",
        WatcherDecisionState::TriggeredPlanOnly => "triggered_plan_only",
        WatcherDecisionState::TriggeredPlanAndApply => "triggered_plan_and_apply",
    }
}
