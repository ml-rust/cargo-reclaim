use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    BackgroundRunEventKind, BackgroundRunLogRecord, BackgroundServiceOptions,
    BackgroundServicePaths, BackgroundServiceState, BackgroundServiceStatus,
    DEFAULT_SCHEDULER_INSTANCE_NAME, ReclaimConfig, SchedulerPlatform, default_instance_log_dir,
    default_instance_state_dir, default_log_dir, default_state_dir, load_config_from_path,
    read_background_run_log, read_background_service_state, refresh_background_service_state,
    run_background_service, scheduler_instance_name_from_config,
};

use super::super::{CliError, OutputFormat, inline_config_path, next_path, next_value};
use super::scheduler_subcommand_usage;

#[derive(Debug)]
pub(in crate::cli) enum SchedulerServiceCommand {
    Run(SchedulerServiceRunCommand),
    Status(SchedulerServiceStatusCommand),
}

#[derive(Debug)]
pub(in crate::cli) struct SchedulerServiceRunCommand {
    config_path: PathBuf,
    max_cycles: Option<usize>,
    output_format: OutputFormat,
}

#[derive(Debug)]
pub(in crate::cli) struct SchedulerServiceStatusCommand {
    config_path: PathBuf,
    output_format: OutputFormat,
}

pub(super) fn parse_scheduler_service(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerServiceCommand, CliError> {
    let mut args = args.into_iter();
    let Some(subcommand) = args.next() else {
        return Err(CliError::Usage(
            "scheduler service requires `run` or `status`".to_string(),
        ));
    };
    match subcommand.to_string_lossy().as_ref() {
        "run" => parse_service_run(args).map(SchedulerServiceCommand::Run),
        "status" => parse_service_status(args).map(SchedulerServiceCommand::Status),
        "-h" | "--help" | "help" => Err(CliError::Help(scheduler_subcommand_usage("service"))),
        value => Err(CliError::Usage(format!(
            "unknown scheduler service command `{value}`; expected `run` or `status`"
        ))),
    }
}

pub(super) fn run_scheduler_service(
    command: &SchedulerServiceCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match command {
        SchedulerServiceCommand::Run(command) => run_service(command, output),
        SchedulerServiceCommand::Status(command) => run_status(command, output),
    }
}

fn parse_service_run(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerServiceRunCommand, CliError> {
    let mut config_path = None;
    let mut max_cycles = None;
    let mut output_format = OutputFormat::Terminal;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "scheduler service run options must be valid UTF-8".to_string(),
            ));
        };
        match arg_text {
            "--config" => config_path = Some(next_path(&mut args, "--config")?),
            "--max-cycles" => {
                max_cycles = Some(parse_max_cycles(&next_value(&mut args, "--max-cycles")?)?)
            }
            value if value.starts_with("--max-cycles=") => {
                max_cycles = Some(parse_max_cycles(&value["--max-cycles=".len()..])?)
            }
            "--json" => output_format = OutputFormat::Json,
            "-h" | "--help" => {
                return Err(CliError::Help(scheduler_subcommand_usage("service run")));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scheduler service run option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected scheduler service run argument `{value}`"
                )));
            }
        }
    }

    Ok(SchedulerServiceRunCommand {
        config_path: config_path.ok_or_else(|| {
            CliError::Usage("scheduler service run requires --config".to_string())
        })?,
        max_cycles,
        output_format,
    })
}

fn parse_service_status(
    args: impl IntoIterator<Item = OsString>,
) -> Result<SchedulerServiceStatusCommand, CliError> {
    let mut config_path = None;
    let mut output_format = OutputFormat::Terminal;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }
        let Some(arg_text) = arg.as_os_str().to_str() else {
            return Err(CliError::Usage(
                "scheduler service status options must be valid UTF-8".to_string(),
            ));
        };
        match arg_text {
            "--config" => config_path = Some(next_path(&mut args, "--config")?),
            "--json" => output_format = OutputFormat::Json,
            "-h" | "--help" => {
                return Err(CliError::Help(scheduler_subcommand_usage("service status")));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scheduler service status option `{value}`"
                )));
            }
            value => {
                return Err(CliError::Usage(format!(
                    "unexpected scheduler service status argument `{value}`"
                )));
            }
        }
    }

    Ok(SchedulerServiceStatusCommand {
        config_path: config_path.ok_or_else(|| {
            CliError::Usage("scheduler service status requires --config".to_string())
        })?,
        output_format,
    })
}

fn run_service(
    command: &SchedulerServiceRunCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let config = load_config_from_path(&command.config_path)?;
    let config_path = canonical_config_path(command.config_path.clone());
    let options = service_options(&config_path, &config, command.max_cycles)?;
    let summary = run_background_service(options, &config)?;
    match command.output_format {
        OutputFormat::Terminal => {
            writeln!(output, "cargo-reclaim scheduler service")?;
            writeln!(output, "status: {}", status_label(summary.state.status))?;
            writeln!(output, "cycles: {}", summary.cycles_completed)?;
            if let Some(run_id) = &summary.state.last_run_id {
                writeln!(output, "last run: {run_id}")?;
            }
        }
        OutputFormat::Json => write_status_json(
            output,
            "scheduler-service-run",
            &summary.state,
            Some(summary.cycles_completed),
            None,
        )?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_status(
    command: &SchedulerServiceStatusCommand,
    output: &mut impl Write,
) -> Result<ExitCode, CliError> {
    let config = load_config_from_path(&command.config_path)?;
    let config_path = canonical_config_path(command.config_path.clone());
    let paths = service_paths(&config_path, &config)?;
    let state = read_background_service_state(&paths.state_path)?
        .map(refresh_background_service_state)
        .unwrap_or_else(BackgroundServiceState::missing);
    let run_summary = read_run_summary(&paths.log_dir.join("runs.jsonl"))?;
    match command.output_format {
        OutputFormat::Terminal => write_status_terminal(output, &state, run_summary.as_ref())?,
        OutputFormat::Json => write_status_json(
            output,
            "scheduler-service-status",
            &state,
            None,
            run_summary.as_ref(),
        )?,
    }
    Ok(ExitCode::SUCCESS)
}

fn service_options(
    config_path: &std::path::Path,
    config: &ReclaimConfig,
    max_cycles: Option<usize>,
) -> Result<BackgroundServiceOptions, CliError> {
    let paths = service_paths(config_path, config)?;
    Ok(BackgroundServiceOptions {
        config_path: config_path.to_path_buf(),
        state_dir: paths.state_dir,
        log_dir: paths.log_dir,
        mode: None,
        max_cycles,
    })
}

fn service_paths(
    config_path: &std::path::Path,
    config: &ReclaimConfig,
) -> Result<BackgroundServicePaths, CliError> {
    let instance_name =
        scheduler_instance_name_from_config(config.scheduler.name.as_deref(), config_path)?;
    Ok(BackgroundServicePaths::new(
        config
            .scheduler
            .state_dir
            .clone()
            .unwrap_or_else(|| default_service_state_dir(&instance_name)),
        config
            .scheduler
            .log_dir
            .clone()
            .unwrap_or_else(|| default_service_log_dir(&instance_name)),
    ))
}

fn default_service_state_dir(instance_name: &str) -> PathBuf {
    if instance_name == DEFAULT_SCHEDULER_INSTANCE_NAME {
        default_state_dir(SchedulerPlatform::SystemdUser)
    } else {
        default_instance_state_dir(SchedulerPlatform::SystemdUser, instance_name)
    }
}

fn default_service_log_dir(instance_name: &str) -> PathBuf {
    if instance_name == DEFAULT_SCHEDULER_INSTANCE_NAME {
        default_log_dir(SchedulerPlatform::SystemdUser)
    } else {
        default_instance_log_dir(SchedulerPlatform::SystemdUser, instance_name)
    }
}

fn write_status_terminal(
    output: &mut impl Write,
    state: &BackgroundServiceState,
    run_summary: Option<&RunSummary>,
) -> Result<(), CliError> {
    writeln!(
        output,
        "cargo-reclaim scheduler service: {}",
        status_label(state.status)
    )?;
    if let Some(pid) = state.pid {
        writeln!(output, "pid: {pid}")?;
    }
    if let Some(run_id) = &state.last_run_id {
        writeln!(output, "last run: {run_id}")?;
    }
    if let Some(problem) = &state.last_problem {
        writeln!(output, "problem: {problem}")?;
    }
    if let Some(summary) = run_summary {
        writeln!(output, "run log records: {}", summary.record_count)?;
        writeln!(output, "started cycles: {}", summary.started_count)?;
        writeln!(
            output,
            "apply completed cycles: {}",
            summary.apply_completed_count
        )?;
        writeln!(output, "failed cycles: {}", summary.failed_count)?;
        writeln!(output, "applied bytes: {}", summary.applied_bytes)?;
        if let Some(run_id) = &summary.last_event_run_id {
            writeln!(output, "last event run: {run_id}")?;
        }
        if let Some(kind) = summary.last_event_kind {
            writeln!(output, "last event: {}", run_event_kind_label(kind))?;
        }
    }
    Ok(())
}

fn write_status_json(
    output: &mut impl Write,
    command: &'static str,
    state: &BackgroundServiceState,
    cycles_completed: Option<usize>,
    run_summary: Option<&RunSummary>,
) -> Result<(), CliError> {
    let document = serde_json::json!({
        "command": command,
        "schema_version": state.schema_version,
        "status": status_label(state.status),
        "pid": state.pid,
        "started_at": state.started_at,
        "last_run_id": state.last_run_id,
        "last_run_at": state.last_run_at,
        "next_run_at": state.next_run_at,
        "consecutive_failures": state.consecutive_failures,
        "last_problem": state.last_problem,
        "cycles_completed": cycles_completed,
        "run_log": run_summary.map(JsonRunSummary::from),
    });
    serde_json::to_writer(&mut *output, &document)?;
    writeln!(output)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunSummary {
    record_count: usize,
    started_count: usize,
    apply_completed_count: usize,
    failed_count: usize,
    applied_bytes: u64,
    last_event_run_id: Option<String>,
    last_event_kind: Option<BackgroundRunEventKind>,
}

#[derive(serde::Serialize)]
struct JsonRunSummary {
    record_count: usize,
    started_count: usize,
    apply_completed_count: usize,
    failed_count: usize,
    applied_bytes: u64,
    last_event_run_id: Option<String>,
    last_event: Option<&'static str>,
}

impl From<&RunSummary> for JsonRunSummary {
    fn from(summary: &RunSummary) -> Self {
        Self {
            record_count: summary.record_count,
            started_count: summary.started_count,
            apply_completed_count: summary.apply_completed_count,
            failed_count: summary.failed_count,
            applied_bytes: summary.applied_bytes,
            last_event_run_id: summary.last_event_run_id.clone(),
            last_event: summary.last_event_kind.map(run_event_kind_label),
        }
    }
}

fn read_run_summary(path: &std::path::Path) -> Result<Option<RunSummary>, CliError> {
    if !path.exists() {
        return Ok(None);
    }

    Ok(Some(summarize_run_log(&read_background_run_log(path)?)))
}

fn summarize_run_log(records: &[BackgroundRunLogRecord]) -> RunSummary {
    let mut summary = RunSummary {
        record_count: records.len(),
        started_count: 0,
        apply_completed_count: 0,
        failed_count: 0,
        applied_bytes: 0,
        last_event_run_id: None,
        last_event_kind: None,
    };

    for record in records {
        match record.kind {
            BackgroundRunEventKind::Started => summary.started_count += 1,
            BackgroundRunEventKind::ApplyCompleted => {
                summary.apply_completed_count += 1;
                if let Some(apply) = &record.apply {
                    summary.applied_bytes = summary
                        .applied_bytes
                        .saturating_add(apply.totals.applied_bytes);
                }
            }
            BackgroundRunEventKind::Failed => summary.failed_count += 1,
            BackgroundRunEventKind::Triggered
            | BackgroundRunEventKind::PlanBuilt
            | BackgroundRunEventKind::Skipped => {}
        }
        summary.last_event_run_id = Some(record.run_id.clone());
        summary.last_event_kind = Some(record.kind);
    }

    summary
}

fn run_event_kind_label(kind: BackgroundRunEventKind) -> &'static str {
    match kind {
        BackgroundRunEventKind::Started => "started",
        BackgroundRunEventKind::Triggered => "triggered",
        BackgroundRunEventKind::PlanBuilt => "plan_built",
        BackgroundRunEventKind::ApplyCompleted => "apply_completed",
        BackgroundRunEventKind::Skipped => "skipped",
        BackgroundRunEventKind::Failed => "failed",
    }
}

fn status_label(status: BackgroundServiceStatus) -> &'static str {
    match status {
        BackgroundServiceStatus::Running => "running",
        BackgroundServiceStatus::Stopped => "stopped",
        BackgroundServiceStatus::Unknown => "unknown",
        BackgroundServiceStatus::Stale => "stale",
        BackgroundServiceStatus::Error => "error",
    }
}

fn parse_max_cycles(value: &str) -> Result<usize, CliError> {
    let max_cycles = value.parse::<usize>().map_err(|_| {
        CliError::Usage(format!(
            "invalid --max-cycles `{value}`; expected a positive integer"
        ))
    })?;
    if max_cycles == 0 {
        return Err(CliError::Usage(
            "--max-cycles must be greater than zero".to_string(),
        ));
    }
    Ok(max_cycles)
}

fn canonical_config_path(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cargo_reclaim::parse_config;

    use super::service_paths;

    #[test]
    fn service_paths_use_generic_defaults_without_name() -> Result<(), Box<dyn std::error::Error>> {
        let config = parse_config("version = 1\n")?;
        let paths = service_paths(Path::new("/tmp/projects/nodedb.toml"), &config)?;

        assert!(
            paths
                .state_dir
                .display()
                .to_string()
                .ends_with("cargo-reclaim")
        );
        assert!(
            paths
                .log_dir
                .display()
                .to_string()
                .ends_with("cargo-reclaim/logs")
        );
        Ok(())
    }
}
