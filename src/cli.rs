use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    InventoryOptions, PlannerOptions, PolicyKind, ReclaimError, ScannerOptions, WholeTargetConfig,
    WholeTargetMode, load_config_from_path, platform_active_observation_provider,
};

mod apply;
mod cargo_config;
mod cargo_home;
mod edit_plan;
mod error_output;
mod output;
mod persistence;
mod plan;
mod scheduler;

use apply::{ApplyCommand, parse_apply_command, run_apply};
use cargo_config::{CargoConfigCommand, parse_cargo_config_command, run_cargo_config_command};
use cargo_home::{CargoHomeCommand, parse_cargo_home_command, run_cargo_home_command};
use edit_plan::{EditPlanCommand, parse_edit_plan_command, run_edit_plan};
use error_output::write_error_json;
use output::write_help;
use persistence::{SavePlanRequest, parse_duration};
use plan::run_plan_command;
use scheduler::{SchedulerPreviewCommand, parse_scheduler_command, run_scheduler_preview};

pub fn run() -> ExitCode {
    let mut stdout = io::stdout();
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let json_errors = args_request_json(&args);
    match run_with_args(args, &mut stdout) {
        Ok(code) => code,
        Err(error) => match error {
            CliError::Help(message) => {
                let _ = writeln!(stdout, "{message}");
                ExitCode::SUCCESS
            }
            error => {
                let code = error.exit_code();
                let mut stderr = io::stderr();
                if json_errors {
                    let _ = write_error_json(&mut stderr, &error);
                } else {
                    let _ = writeln!(stderr, "cargo-reclaim: {error}");
                }
                code
            }
        },
    }
}

fn run_with_args(
    args: impl IntoIterator<Item = OsString>,
    stdout: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match parse_args(args)? {
        Command::Help => {
            write_help(stdout)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            writeln!(stdout, "cargo-reclaim {}", env!("CARGO_PKG_VERSION"))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Plan(command) => {
            let provider = platform_active_observation_provider();
            run_plan_command(command, stdout, &provider)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Apply(command) => run_apply(&command, stdout),
        Command::EditPlan(command) => run_edit_plan(&command, stdout),
        Command::SchedulerPreview(command) => run_scheduler_preview(&command, stdout),
        Command::CargoConfig(command) => run_cargo_config_command(&command, stdout),
        Command::CargoHome(command) => run_cargo_home_command(&command, stdout),
    }
}

#[derive(Debug)]
enum Command {
    Help,
    Version,
    Plan(PlanCommand),
    Apply(ApplyCommand),
    EditPlan(EditPlanCommand),
    SchedulerPreview(SchedulerPreviewCommand),
    CargoConfig(CargoConfigCommand),
    CargoHome(CargoHomeCommand),
}

#[derive(Debug)]
struct PlanCommand {
    mode: PlanMode,
    roots: Vec<PathBuf>,
    policy: PolicyKind,
    output_format: OutputFormat,
    save_plan: Option<SavePlanRequest>,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    planner_options: PlannerOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanMode {
    Scan,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Terminal,
    Json,
}

fn args_request_json(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--json")
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };

    match command.to_string_lossy().as_ref() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "scan" => parse_plan_command(PlanMode::Scan, args),
        "plan" => parse_plan_command(PlanMode::Plan, args),
        "apply" => parse_apply_command(args).map(Command::Apply),
        "edit-plan" => parse_edit_plan_command(args).map(Command::EditPlan),
        "scheduler" => parse_scheduler_command(args).map(Command::SchedulerPreview),
        "cargo-config" => parse_cargo_config_command(args).map(Command::CargoConfig),
        "cargo-home" => parse_cargo_home_command(args).map(Command::CargoHome),
        command => Err(CliError::Usage(format!(
            "unknown command `{command}`; expected `scan`, `plan`, `apply`, `edit-plan`, `scheduler`, `cargo-config`, `cargo-home`, or `help`"
        ))),
    }
}

fn parse_plan_command(
    mode: PlanMode,
    args: impl IntoIterator<Item = OsString>,
) -> Result<Command, CliError> {
    let mut roots = Vec::new();
    let mut policy = None;
    let mut output_format = OutputFormat::Terminal;
    let mut save_plan = None;
    let mut expires_in = None;
    let mut config_path = None;
    let mut scanner_options = ScannerOptions::default();
    let mut planner_options = PlannerOptions::default();
    let mut cli_follow_symlinks = false;
    let mut cli_allow_name_only_targets = false;
    let mut cli_cross_filesystems = false;
    let mut cli_recent_write_keep_window = false;
    let mut whole_target_source = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(ignore_path) = inline_ignore_path(&arg)? {
            scanner_options.ignored_paths.push(ignore_path);
            continue;
        }
        if let Some(skip_path) = inline_skip_path(&arg)? {
            scanner_options.skipped_paths.push(skip_path);
            continue;
        }
        if let Some(path) = inline_config_path(&arg)? {
            config_path = Some(path);
            continue;
        }

        let Some(arg_text) = arg.as_os_str().to_str() else {
            roots.push(PathBuf::from(arg));
            continue;
        };

        match arg_text {
            "-h" | "--help" => return Ok(Command::Help),
            "--" => {
                roots.extend(args.map(PathBuf::from));
                break;
            }
            "--policy" => {
                let value = next_value(&mut args, "--policy")?;
                policy = Some(parse_policy(&value)?);
            }
            value if value.starts_with("--policy=") => {
                policy = Some(parse_policy(&value["--policy=".len()..])?);
            }
            "--whole-target" => {
                planner_options.whole_target_mode =
                    parse_whole_target_mode(&next_value(&mut args, "--whole-target")?)?;
                whole_target_source = Some(WholeTargetSource::Cli);
            }
            value if value.starts_with("--whole-target=") => {
                planner_options.whole_target_mode =
                    parse_whole_target_mode(&value["--whole-target=".len()..])?;
                whole_target_source = Some(WholeTargetSource::Cli);
            }
            "--config" => {
                config_path = Some(next_path(&mut args, "--config")?);
            }
            "--ignore" => {
                scanner_options
                    .ignored_paths
                    .push(next_path(&mut args, "--ignore")?);
            }
            "--skip" => {
                scanner_options
                    .skipped_paths
                    .push(next_path(&mut args, "--skip")?);
            }
            "--allow-name-only-targets" => {
                scanner_options.allow_name_only_targets = true;
                cli_allow_name_only_targets = true;
            }
            "--follow-symlinks" => {
                scanner_options.follow_symlinks = true;
                cli_follow_symlinks = true;
            }
            "--cross-filesystems" => {
                scanner_options.cross_filesystems = true;
                cli_cross_filesystems = true;
            }
            "--keep-recent-writes" => {
                planner_options.recent_write_keep_window = Some(parse_duration(&next_value(
                    &mut args,
                    "--keep-recent-writes",
                )?)?);
                cli_recent_write_keep_window = true;
            }
            value if value.starts_with("--keep-recent-writes=") => {
                planner_options.recent_write_keep_window =
                    Some(parse_duration(&value["--keep-recent-writes=".len()..])?);
                cli_recent_write_keep_window = true;
            }
            "--json" => output_format = OutputFormat::Json,
            "--save-plan" => {
                if mode != PlanMode::Plan {
                    return Err(CliError::Usage(
                        "`--save-plan` is only supported by `plan`".to_string(),
                    ));
                }
                save_plan = Some(SavePlanRequest::new(next_path(&mut args, "--save-plan")?));
            }
            value if value.starts_with("--save-plan=") => {
                if mode != PlanMode::Plan {
                    return Err(CliError::Usage(
                        "`--save-plan` is only supported by `plan`".to_string(),
                    ));
                }
                save_plan = Some(SavePlanRequest::new(PathBuf::from(
                    &value["--save-plan=".len()..],
                )));
            }
            "--expires-in" => {
                expires_in = Some(parse_duration(&next_value(&mut args, "--expires-in")?)?);
            }
            value if value.starts_with("--expires-in=") => {
                expires_in = Some(parse_duration(&value["--expires-in=".len()..])?);
            }
            "--apply" | "--yes" => {
                return Err(CliError::Usage(
                    "apply is not available yet; this command only builds a dry-run plan"
                        .to_string(),
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{value}`")));
            }
            _ => roots.push(PathBuf::from(arg)),
        }
    }

    let config = config_path
        .as_ref()
        .map(load_config_from_path)
        .transpose()?;
    let config_version = config.as_ref().map(|config| config.version);

    if roots.is_empty() {
        if let Some(config_roots) = config
            .as_ref()
            .filter(|config| !config.roots.is_empty())
            .map(|config| config.roots.clone())
        {
            roots = config_roots;
        } else {
            roots.push(PathBuf::from("."));
        }
    }

    let policy = match policy {
        Some(policy) => policy,
        None => config
            .as_ref()
            .and_then(|config| config.policy.as_deref())
            .map(parse_policy)
            .transpose()?
            .unwrap_or_default(),
    };
    if let Some(config) = config {
        let mut ignored_paths = config.ignored_paths;
        ignored_paths.extend(scanner_options.ignored_paths);
        scanner_options.ignored_paths = ignored_paths;
        let mut skipped_paths = config.skipped_paths;
        skipped_paths.extend(scanner_options.skipped_paths);
        scanner_options.skipped_paths = skipped_paths;
        if !cli_follow_symlinks && let Some(follow_symlinks) = config.scanner.follow_symlinks {
            scanner_options.follow_symlinks = follow_symlinks;
        }
        if !cli_allow_name_only_targets
            && let Some(allow_name_only_targets) = config.scanner.allow_name_only_targets
        {
            scanner_options.allow_name_only_targets = allow_name_only_targets;
        }
        if !cli_cross_filesystems && let Some(cross_filesystems) = config.scanner.cross_filesystems
        {
            scanner_options.cross_filesystems = cross_filesystems;
        }
        if !cli_recent_write_keep_window {
            planner_options.recent_write_keep_window = config.recent_write_keep_window;
        }
        if whole_target_source.is_none()
            && let Some(whole_target) = config.whole_target
        {
            planner_options.whole_target_mode = whole_target_mode_from_config(whole_target);
            whole_target_source = Some(WholeTargetSource::Config {
                allow_unattended_delete: config
                    .allow_unattended_whole_target_delete
                    .unwrap_or(false),
            });
        }
    }
    let inventory_options = InventoryOptions {
        follow_symlinks: scanner_options.follow_symlinks,
        skipped_paths: scanner_options.skipped_paths.clone(),
    };

    finish_plan_command(FinishPlanCommand {
        mode,
        roots,
        policy,
        output_format,
        save_plan,
        expires_in,
        config_path,
        config_version,
        scanner_options,
        inventory_options,
        planner_options,
        whole_target_source,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WholeTargetSource {
    Cli,
    Config { allow_unattended_delete: bool },
}

struct FinishPlanCommand {
    mode: PlanMode,
    roots: Vec<PathBuf>,
    policy: PolicyKind,
    output_format: OutputFormat,
    save_plan: Option<SavePlanRequest>,
    expires_in: Option<std::time::Duration>,
    config_path: Option<PathBuf>,
    config_version: Option<u16>,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
    planner_options: PlannerOptions,
    whole_target_source: Option<WholeTargetSource>,
}

fn finish_plan_command(mut command: FinishPlanCommand) -> Result<Command, CliError> {
    if command.planner_options.whole_target_mode == WholeTargetMode::DeleteConfirmed {
        if command.policy != PolicyKind::Aggressive {
            return Err(CliError::Usage(
                "`--whole-target delete` requires `--policy aggressive`".to_string(),
            ));
        }
        if matches!(
            command.whole_target_source,
            Some(WholeTargetSource::Config {
                allow_unattended_delete: false
            })
        ) {
            return Err(CliError::Usage(
                "config whole_target = \"delete\" requires allow_unattended_whole_target_delete = true".to_string(),
            ));
        }
    }

    if let Some(expires_in) = command.expires_in {
        let Some(save_plan) = command.save_plan.as_mut() else {
            return Err(CliError::Usage(
                "`--expires-in` requires `--save-plan`".to_string(),
            ));
        };
        save_plan.set_expires_in(expires_in);
    }

    Ok(Command::Plan(PlanCommand {
        mode: command.mode,
        roots: command.roots,
        policy: command.policy,
        output_format: command.output_format,
        save_plan: command.save_plan,
        config_path: command.config_path,
        config_version: command.config_version,
        scanner_options: command.scanner_options,
        inventory_options: command.inventory_options,
        planner_options: command.planner_options,
    }))
}

fn next_value(
    args: &mut impl Iterator<Item = OsString>,
    option: &'static str,
) -> Result<String, CliError> {
    let value = args
        .next()
        .ok_or_else(|| CliError::Usage(format!("{option} requires a value")))?;
    value
        .into_string()
        .map_err(|_| CliError::Usage(format!("{option} value must be valid UTF-8")))
}

fn next_path(
    args: &mut impl Iterator<Item = OsString>,
    option: &'static str,
) -> Result<PathBuf, CliError> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage(format!("{option} requires a value")))
}

fn inline_ignore_path(arg: &OsString) -> Result<Option<PathBuf>, CliError> {
    inline_path(arg, "--ignore=", "--ignore")
}

fn inline_skip_path(arg: &OsString) -> Result<Option<PathBuf>, CliError> {
    inline_path(arg, "--skip=", "--skip")
}

fn inline_config_path(arg: &OsString) -> Result<Option<PathBuf>, CliError> {
    inline_path(arg, "--config=", "--config")
}

fn inline_path(
    arg: &OsString,
    prefix: &'static str,
    option: &'static str,
) -> Result<Option<PathBuf>, CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let bytes = arg.as_os_str().as_bytes();
        if let Some(path) = bytes.strip_prefix(prefix.as_bytes()) {
            if path.is_empty() {
                return Err(CliError::Usage(format!("{option} requires a value")));
            }
            return Ok(Some(PathBuf::from(std::ffi::OsString::from_vec(
                path.to_vec(),
            ))));
        }
    }

    #[cfg(not(unix))]
    {
        if let Some(value) = arg
            .as_os_str()
            .to_str()
            .and_then(|value| value.strip_prefix(&format!("{option}=")))
        {
            if value.is_empty() {
                return Err(CliError::Usage(format!("{option} requires a value")));
            }
            return Ok(Some(PathBuf::from(value)));
        }
    }

    Ok(None)
}

fn parse_policy(value: &str) -> Result<PolicyKind, CliError> {
    match value {
        "observe" => Ok(PolicyKind::Observe),
        "conservative" => Ok(PolicyKind::Conservative),
        "balanced" => Ok(PolicyKind::Balanced),
        "aggressive" => Ok(PolicyKind::Aggressive),
        "custom" => Ok(PolicyKind::Custom),
        _ => Err(CliError::Usage(format!(
            "unknown policy `{value}`; expected observe, conservative, balanced, aggressive, or custom"
        ))),
    }
}

fn parse_whole_target_mode(value: &str) -> Result<WholeTargetMode, CliError> {
    match value {
        "off" => Ok(WholeTargetMode::Off),
        "confirm" => Ok(WholeTargetMode::Confirm),
        "delete" => Ok(WholeTargetMode::DeleteConfirmed),
        _ => Err(CliError::Usage(format!(
            "unknown whole-target mode `{value}`; expected off, confirm, or delete"
        ))),
    }
}

fn whole_target_mode_from_config(value: WholeTargetConfig) -> WholeTargetMode {
    match value {
        WholeTargetConfig::Off => WholeTargetMode::Off,
        WholeTargetConfig::Confirm => WholeTargetMode::Confirm,
        WholeTargetConfig::Delete => WholeTargetMode::DeleteConfirmed,
    }
}

#[derive(Debug)]
enum CliError {
    Help(String),
    Usage(String),
    Reclaim(ReclaimError),
    Config(cargo_reclaim::ConfigError),
    Io(io::Error),
    Json(serde_json::Error),
    Persistence(cargo_reclaim::PlanPersistenceError),
    PlanEdit(cargo_reclaim::PlanEditError),
    Scheduler(cargo_reclaim::SchedulerError),
    CargoHome(cargo_reclaim::CargoHomeError),
    BackgroundRunner(cargo_reclaim::BackgroundRunnerError),
    BackgroundService(cargo_reclaim::BackgroundServiceError),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message),
            Self::Help(message) => formatter.write_str(message),
            Self::Reclaim(error) => error.fmt(formatter),
            Self::Config(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Persistence(error) => error.fmt(formatter),
            Self::PlanEdit(error) => error.fmt(formatter),
            Self::Scheduler(error) => error.fmt(formatter),
            Self::CargoHome(error) => error.fmt(formatter),
            Self::BackgroundRunner(error) => error.fmt(formatter),
            Self::BackgroundService(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CliError {}

impl CliError {
    fn kind_label(&self) -> &'static str {
        match self {
            Self::Help(_) => "help",
            Self::Usage(_) => "usage",
            Self::Reclaim(_) => "reclaim",
            Self::Config(_) => "config",
            Self::Io(_) => "io",
            Self::Json(_) => "json",
            Self::Persistence(_) => "persistence",
            Self::PlanEdit(_) => "plan_edit",
            Self::Scheduler(_) => "scheduler",
            Self::CargoHome(_) => "cargo_home",
            Self::BackgroundRunner(_) => "background_runner",
            Self::BackgroundService(_) => "background_service",
        }
    }

    fn exit_code_value(&self) -> u8 {
        match self {
            Self::Help(_) => 0,
            Self::Usage(_) => 2,
            Self::Reclaim(_)
            | Self::Config(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::Persistence(_) => 1,
            Self::Scheduler(_) => 2,
            Self::CargoHome(_) | Self::BackgroundRunner(_) => 1,
            Self::BackgroundService(error) => match error {
                cargo_reclaim::BackgroundServiceError::Config(_)
                | cargo_reclaim::BackgroundServiceError::Scheduler(_) => 2,
                _ => 1,
            },
            Self::PlanEdit(error) => match error {
                cargo_reclaim::PlanEditError::NoEdits
                | cargo_reclaim::PlanEditError::ConflictingEdit { .. }
                | cargo_reclaim::PlanEditError::EntryNotFound { .. }
                | cargo_reclaim::PlanEditError::UnknownArtifactClass { .. }
                | cargo_reclaim::PlanEditError::ProtectedArtifactClass { .. }
                | cargo_reclaim::PlanEditError::ArtifactClassNotFound { .. }
                | cargo_reclaim::PlanEditError::AmbiguousEntryPath { .. } => 2,
                cargo_reclaim::PlanEditError::Persistence(_) => 1,
            },
        }
    }

    fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.exit_code_value())
    }
}

impl From<ReclaimError> for CliError {
    fn from(error: ReclaimError) -> Self {
        Self::Reclaim(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<cargo_reclaim::ConfigError> for CliError {
    fn from(error: cargo_reclaim::ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<cargo_reclaim::PlanPersistenceError> for CliError {
    fn from(error: cargo_reclaim::PlanPersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<cargo_reclaim::PlanEditError> for CliError {
    fn from(error: cargo_reclaim::PlanEditError) -> Self {
        Self::PlanEdit(error)
    }
}

impl From<cargo_reclaim::CargoHomeError> for CliError {
    fn from(error: cargo_reclaim::CargoHomeError) -> Self {
        Self::CargoHome(error)
    }
}

impl From<cargo_reclaim::SchedulerError> for CliError {
    fn from(error: cargo_reclaim::SchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

impl From<cargo_reclaim::BackgroundRunnerError> for CliError {
    fn from(error: cargo_reclaim::BackgroundRunnerError) -> Self {
        Self::BackgroundRunner(error)
    }
}

impl From<cargo_reclaim::BackgroundServiceError> for CliError {
    fn from(error: cargo_reclaim::BackgroundServiceError) -> Self {
        Self::BackgroundService(error)
    }
}

#[cfg(test)]
fn write_manifest(path: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::write(path.join("Cargo.toml"), "[package]\nname = \"sample\"\n")
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn parse_plan_defaults_to_current_directory_and_balanced_policy() -> Result<(), CliError> {
        let Command::Plan(command) = parse_args(["plan"].map(OsString::from))? else {
            panic!("expected plan command");
        };

        assert_eq!(command.roots, [PathBuf::from(".")]);
        assert_eq!(command.policy, PolicyKind::Balanced);
        assert_eq!(command.output_format, OutputFormat::Terminal);
        assert!(command.save_plan.is_none());
        assert!(!command.scanner_options.allow_name_only_targets);
        Ok(())
    }

    #[test]
    fn parse_top_level_version_flags() -> Result<(), CliError> {
        for flag in ["--version", "-V"] {
            let command = parse_args([flag].map(OsString::from))?;
            assert!(matches!(command, Command::Version));
        }

        Ok(())
    }

    #[test]
    fn run_top_level_version_writes_version_and_exits_success() -> Result<(), CliError> {
        let mut output = Vec::new();
        let status = run_with_args([OsString::from("--version")], &mut output)?;

        assert_eq!(status, ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(output).expect("version output utf-8"),
            format!("cargo-reclaim {}\n", env!("CARGO_PKG_VERSION"))
        );
        Ok(())
    }

    #[test]
    fn parse_plan_options() -> Result<(), CliError> {
        let Command::Plan(command) = parse_args(
            [
                "scan",
                "--policy=observe",
                "--ignore",
                "target",
                "--skip",
                "vendor",
                "--json",
                "--allow-name-only-targets",
                "--follow-symlinks",
                "--cross-filesystems",
                "--whole-target",
                "confirm",
                "workspace",
            ]
            .map(OsString::from),
        )?
        else {
            panic!("expected plan command");
        };

        assert_eq!(command.policy, PolicyKind::Observe);
        assert_eq!(command.output_format, OutputFormat::Json);
        assert_eq!(command.roots, [PathBuf::from("workspace")]);
        assert_eq!(
            command.scanner_options.ignored_paths,
            [PathBuf::from("target")]
        );
        assert_eq!(
            command.scanner_options.skipped_paths,
            [PathBuf::from("vendor")]
        );
        assert!(command.scanner_options.allow_name_only_targets);
        assert!(command.scanner_options.follow_symlinks);
        assert!(command.inventory_options.follow_symlinks);
        assert!(command.scanner_options.cross_filesystems);
        assert_eq!(
            command.planner_options.whole_target_mode,
            WholeTargetMode::Confirm
        );
        Ok(())
    }

    #[test]
    fn parse_plan_merges_config_with_cli_precedence() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TestTemp::new("cli_parse_config")?;
        let config_path = temp.path.join("reclaim.toml");
        std::fs::write(
            &config_path,
            r#"
version = 1
roots = ["configured-root"]
ignore = ["configured-root/target"]
skip = ["configured-root/vendor"]

[policy]
mode = "observe"
whole_target = "delete"
allow_unattended_whole_target_delete = false

[scanner]
follow_symlinks = true
allow_name_only_targets = true
cross_filesystems = true

[planner]
recent_write_keep_window = "2h"
"#,
        )?;

        let Command::Plan(command) = parse_args([
            OsString::from("plan"),
            OsString::from("--config"),
            config_path.clone().into_os_string(),
            OsString::from("--policy=aggressive"),
            OsString::from("--whole-target=off"),
            OsString::from("--ignore"),
            OsString::from("cli-ignore"),
            OsString::from("--skip=cli-skip"),
            OsString::from("--keep-recent-writes=30m"),
            OsString::from("cli-root"),
        ])?
        else {
            panic!("expected plan command");
        };

        assert_eq!(command.roots, [PathBuf::from("cli-root")]);
        assert_eq!(command.policy, PolicyKind::Aggressive);
        assert_eq!(command.config_path.as_deref(), Some(config_path.as_path()));
        assert_eq!(command.config_version, Some(1));
        assert_eq!(
            command.scanner_options.ignored_paths,
            [
                temp.path.join("configured-root/target"),
                PathBuf::from("cli-ignore")
            ]
        );
        assert_eq!(
            command.scanner_options.skipped_paths,
            [
                temp.path.join("configured-root/vendor"),
                PathBuf::from("cli-skip")
            ]
        );
        assert!(command.scanner_options.follow_symlinks);
        assert!(command.inventory_options.follow_symlinks);
        assert!(command.scanner_options.allow_name_only_targets);
        assert!(command.scanner_options.cross_filesystems);
        assert_eq!(
            command
                .planner_options
                .recent_write_keep_window
                .expect("cli keep window")
                .as_secs(),
            30 * 60
        );
        assert_eq!(
            command.planner_options.whole_target_mode,
            WholeTargetMode::Off
        );
        Ok(())
    }

    #[test]
    fn parse_plan_uses_config_roots_when_cli_roots_are_absent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TestTemp::new("cli_parse_config_roots")?;
        let config_path = temp.path.join("reclaim.toml");
        std::fs::write(
            &config_path,
            r#"
version = 1
roots = ["configured-root"]

[policy]
mode = "conservative"
"#,
        )?;

        let Command::Plan(command) = parse_args([
            OsString::from("scan"),
            OsString::from("--config"),
            config_path.into_os_string(),
        ])?
        else {
            panic!("expected plan command");
        };

        assert_eq!(command.roots, [temp.path.join("configured-root")]);
        assert_eq!(command.policy, PolicyKind::Conservative);
        Ok(())
    }

    #[test]
    fn parse_rejects_apply_flags_on_dry_run_commands() {
        let error = parse_args(["plan", "--apply"].map(OsString::from)).unwrap_err();

        assert!(error.to_string().contains("dry-run plan"));
    }

    #[test]
    fn parse_rejects_missing_config_value() {
        let error = parse_args(["plan", "--config"].map(OsString::from)).unwrap_err();
        assert!(error.to_string().contains("--config requires a value"));

        let error = parse_args(["scan", "--config="].map(OsString::from)).unwrap_err();
        assert!(error.to_string().contains("--config requires a value"));
    }

    #[test]
    fn parse_dash_dash_treats_remaining_args_as_roots() -> Result<(), CliError> {
        let Command::Plan(command) = parse_args(["plan", "--", "-root"].map(OsString::from))?
        else {
            panic!("expected plan command");
        };

        assert_eq!(command.roots, [PathBuf::from("-root")]);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn inline_ignore_preserves_non_utf8_path_bytes() -> Result<(), CliError> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let mut arg = b"--ignore=".to_vec();
        arg.extend_from_slice(b"target-\xFF");
        let Command::Plan(command) = parse_args([
            OsString::from("plan"),
            OsString::from_vec(arg),
            OsString::from("."),
        ])?
        else {
            panic!("expected plan command");
        };

        assert_eq!(
            command.scanner_options.ignored_paths[0]
                .as_os_str()
                .as_bytes(),
            b"target-\xFF"
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn inline_skip_preserves_non_utf8_path_bytes() -> Result<(), CliError> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let mut arg = b"--skip=".to_vec();
        arg.extend_from_slice(b"vendor-\xFF");
        let Command::Plan(command) = parse_args([
            OsString::from("plan"),
            OsString::from_vec(arg),
            OsString::from("."),
        ])?
        else {
            panic!("expected plan command");
        };

        assert_eq!(
            command.scanner_options.skipped_paths[0]
                .as_os_str()
                .as_bytes(),
            b"vendor-\xFF"
        );
        Ok(())
    }

    #[test]
    fn run_scan_and_plan_use_injected_active_provider() -> Result<(), Box<dyn std::error::Error>> {
        for mode in ["scan", "plan"] {
            let temp = TestTemp::new(&format!("cli_active_provider_{mode}"))?;
            write_manifest(&temp.path)?;
            std::fs::create_dir_all(temp.path.join("target/debug/incremental"))?;
            std::fs::write(temp.path.join("target/debug/incremental/cache.bin"), b"abc")?;

            let Command::Plan(command) = parse_args([
                OsString::from(mode),
                OsString::from("--json"),
                temp.path.clone().into_os_string(),
            ])?
            else {
                panic!("expected plan command");
            };
            let provider =
                FakeActiveObservationProvider::new(cargo_reclaim::ActiveObservation::complete([
                    cargo_reclaim::ObservedCargoProcess::new(cargo_reclaim::CargoTool::Cargo)
                        .with_cwd(temp.path.join("member")),
                ]));
            let mut output = Vec::new();

            run_plan_command(command, &mut output, &provider)?;

            let output = String::from_utf8(output)?;
            assert!(output.contains("skip_active"));
        }

        Ok(())
    }

    struct FakeActiveObservationProvider {
        observation: cargo_reclaim::ActiveObservation,
    }

    impl FakeActiveObservationProvider {
        fn new(observation: cargo_reclaim::ActiveObservation) -> Self {
            Self { observation }
        }
    }

    impl cargo_reclaim::ActiveObservationProvider for FakeActiveObservationProvider {
        fn observe(
            &self,
            scope: &cargo_reclaim::ActiveObservationScope,
        ) -> cargo_reclaim::ActiveObservation {
            assert!(!scope.target_contexts().is_empty());
            self.observation.clone()
        }
    }

    struct TestTemp {
        path: PathBuf,
    }

    impl TestTemp {
        fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
            let unique = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cargo_reclaim_{name}_{}_{}",
                std::process::id(),
                unique
            ));
            std::fs::create_dir(&path)?;
            Ok(Self { path })
        }
    }

    impl Drop for TestTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
