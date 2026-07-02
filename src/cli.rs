use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use cargo_reclaim::{
    InventoryOptions, PolicyKind, ReclaimError, ScannerOptions, build_plan_from_roots,
};

mod output;
mod persistence;

use output::{write_help, write_plan};
use persistence::{SavePlanRequest, parse_duration, save_plan};

pub fn run() -> ExitCode {
    match run_with_args(env::args_os().skip(1), &mut io::stdout(), &mut io::stderr()) {
        Ok(code) => code,
        Err(error) => {
            let code = error.exit_code();
            let _ = writeln!(io::stderr(), "cargo-reclaim: {error}");
            code
        }
    }
}

fn run_with_args(
    args: impl IntoIterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Result<ExitCode, CliError> {
    match parse_args(args)? {
        Command::Help => {
            write_help(stdout)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Plan(command) => {
            let plan = build_plan_from_roots(
                command.roots,
                command.policy,
                &command.scanner_options,
                &command.inventory_options,
            )?;
            if let Some(request) = command.save_plan.as_ref() {
                save_plan(
                    &plan,
                    command.mode,
                    command.policy,
                    &command.scanner_options,
                    &command.inventory_options,
                    request,
                )?;
            }
            write_plan(
                stdout,
                &plan,
                command.policy,
                command.mode,
                command.output_format,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::UnsupportedApply => {
            writeln!(
                stderr,
                "cargo-reclaim: apply is not available yet; run `cargo-reclaim plan` for a dry-run plan"
            )?;
            Ok(ExitCode::from(2))
        }
    }
}

#[derive(Debug)]
enum Command {
    Help,
    Plan(PlanCommand),
    UnsupportedApply,
}

#[derive(Debug)]
struct PlanCommand {
    mode: PlanMode,
    roots: Vec<PathBuf>,
    policy: PolicyKind,
    output_format: OutputFormat,
    save_plan: Option<SavePlanRequest>,
    scanner_options: ScannerOptions,
    inventory_options: InventoryOptions,
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

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };

    match command.to_string_lossy().as_ref() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "scan" => parse_plan_command(PlanMode::Scan, args),
        "plan" => parse_plan_command(PlanMode::Plan, args),
        "apply" => Ok(Command::UnsupportedApply),
        command => Err(CliError::Usage(format!(
            "unknown command `{command}`; expected `scan`, `plan`, or `help`"
        ))),
    }
}

fn parse_plan_command(
    mode: PlanMode,
    args: impl IntoIterator<Item = OsString>,
) -> Result<Command, CliError> {
    let mut roots = Vec::new();
    let mut policy = PolicyKind::Balanced;
    let mut output_format = OutputFormat::Terminal;
    let mut save_plan = None;
    let mut expires_in = None;
    let mut scanner_options = ScannerOptions::default();
    let mut inventory_options = InventoryOptions::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if let Some(ignore_path) = inline_ignore_path(&arg)? {
            scanner_options.ignored_paths.push(ignore_path);
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
                policy = parse_policy(&value)?;
            }
            value if value.starts_with("--policy=") => {
                policy = parse_policy(&value["--policy=".len()..])?;
            }
            "--ignore" => {
                scanner_options
                    .ignored_paths
                    .push(next_path(&mut args, "--ignore")?);
            }
            "--allow-name-only-targets" => scanner_options.allow_name_only_targets = true,
            "--follow-symlinks" => {
                scanner_options.follow_symlinks = true;
                inventory_options.follow_symlinks = true;
            }
            "--cross-filesystems" => scanner_options.cross_filesystems = true,
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

    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    if let Some(expires_in) = expires_in {
        let Some(save_plan) = save_plan.as_mut() else {
            return Err(CliError::Usage(
                "`--expires-in` requires `--save-plan`".to_string(),
            ));
        };
        save_plan.set_expires_in(expires_in);
    }

    Ok(Command::Plan(PlanCommand {
        mode,
        roots,
        policy,
        output_format,
        save_plan,
        scanner_options,
        inventory_options,
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
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        const PREFIX: &[u8] = b"--ignore=";
        let bytes = arg.as_os_str().as_bytes();
        if let Some(path) = bytes.strip_prefix(PREFIX) {
            if path.is_empty() {
                return Err(CliError::Usage("--ignore requires a value".to_string()));
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
            .and_then(|value| value.strip_prefix("--ignore="))
        {
            if value.is_empty() {
                return Err(CliError::Usage("--ignore requires a value".to_string()));
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

#[derive(Debug)]
enum CliError {
    Usage(String),
    Reclaim(ReclaimError),
    Io(io::Error),
    Json(serde_json::Error),
    Persistence(cargo_reclaim::PlanPersistenceError),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message),
            Self::Reclaim(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Persistence(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CliError {}

impl CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Usage(_) => ExitCode::from(2),
            Self::Reclaim(_) | Self::Io(_) | Self::Json(_) | Self::Persistence(_) => {
                ExitCode::FAILURE
            }
        }
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

#[cfg(test)]
mod tests {
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
    fn parse_plan_options() -> Result<(), CliError> {
        let Command::Plan(command) = parse_args(
            [
                "scan",
                "--policy=observe",
                "--ignore",
                "target",
                "--json",
                "--allow-name-only-targets",
                "--follow-symlinks",
                "--cross-filesystems",
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
        assert!(command.scanner_options.allow_name_only_targets);
        assert!(command.scanner_options.follow_symlinks);
        assert!(command.inventory_options.follow_symlinks);
        assert!(command.scanner_options.cross_filesystems);
        Ok(())
    }

    #[test]
    fn parse_rejects_apply_flags_on_dry_run_commands() {
        let error = parse_args(["plan", "--apply"].map(OsString::from)).unwrap_err();

        assert!(error.to_string().contains("dry-run plan"));
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
}
