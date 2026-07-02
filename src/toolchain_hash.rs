use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::process::Command;

use crate::planner::PlannerOptions;
use rustc_stable_hash::StableSipHasher128;

pub type ToolchainHashResult<T> = Result<T, ToolchainHashError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolchainHashError {
    RustupToolchainListFailed {
        status: Option<i32>,
        stderr: String,
    },
    RustupRunRustcFailed {
        toolchain: String,
        status: Option<i32>,
        stderr: String,
    },
    EmptyRustcVersion {
        toolchain: String,
    },
    InvalidInstalledToolchainLine {
        line: String,
    },
    Io {
        command: String,
        message: String,
    },
    Utf8 {
        command: String,
        message: String,
    },
}

impl fmt::Display for ToolchainHashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RustupToolchainListFailed { status, stderr } => write!(
                formatter,
                "rustup toolchain list failed with status {}: {}",
                status_label(*status),
                stderr.trim()
            ),
            Self::RustupRunRustcFailed {
                toolchain,
                status,
                stderr,
            } => write!(
                formatter,
                "rustup run {toolchain} rustc -vV failed with status {}: {}",
                status_label(*status),
                stderr.trim()
            ),
            Self::EmptyRustcVersion { toolchain } => write!(
                formatter,
                "rustup run {toolchain} rustc -vV succeeded but did not emit version details"
            ),
            Self::InvalidInstalledToolchainLine { line } => write!(
                formatter,
                "rustup toolchain list reported an invalid installed toolchain line `{line}`"
            ),
            Self::Io { command, message } => {
                write!(formatter, "failed to run {command}: {message}")
            }
            Self::Utf8 { command, message } => {
                write!(formatter, "{command} emitted invalid UTF-8: {message}")
            }
        }
    }
}

impl Error for ToolchainHashError {}

pub trait ToolchainHashResolver {
    fn installed_toolchains(&self) -> ToolchainHashResult<Vec<String>>;
    fn toolchain_rustc_hash(&self, toolchain: &str) -> ToolchainHashResult<u64>;
}

#[derive(Debug, Clone, Copy, Default)]
struct CommandToolchainHashResolver;

impl ToolchainHashResolver for CommandToolchainHashResolver {
    fn installed_toolchains(&self) -> ToolchainHashResult<Vec<String>> {
        let output = Command::new("rustup")
            .arg("toolchain")
            .arg("list")
            .output()
            .map_err(|source| ToolchainHashError::Io {
                command: "rustup toolchain list".to_string(),
                message: source.to_string(),
            })?;
        if !output.status.success() {
            return Err(ToolchainHashError::RustupToolchainListFailed {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let stdout =
            String::from_utf8(output.stdout).map_err(|source| ToolchainHashError::Utf8 {
                command: "rustup toolchain list".to_string(),
                message: source.to_string(),
            })?;
        parse_installed_toolchains(&stdout)
    }

    fn toolchain_rustc_hash(&self, toolchain: &str) -> ToolchainHashResult<u64> {
        let output = Command::new("rustup")
            .arg("run")
            .arg(toolchain)
            .arg("rustc")
            .arg("-vV")
            .output()
            .map_err(|source| ToolchainHashError::Io {
                command: format!("rustup run {toolchain} rustc -vV"),
                message: source.to_string(),
            })?;
        if !output.status.success() {
            return Err(ToolchainHashError::RustupRunRustcFailed {
                toolchain: toolchain.to_string(),
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let stdout =
            String::from_utf8(output.stdout).map_err(|source| ToolchainHashError::Utf8 {
                command: format!("rustup run {toolchain} rustc -vV"),
                message: source.to_string(),
            })?;
        rustc_version_hash(toolchain, &stdout)
    }
}

pub fn resolve_toolchain_hash_options(
    options: &mut PlannerOptions,
    resolver: &impl ToolchainHashResolver,
) -> ToolchainHashResult<()> {
    if !options.keep_installed_toolchains && options.keep_toolchains.is_empty() {
        return Ok(());
    }

    let installed_toolchains = if options.keep_installed_toolchains {
        Some(resolver.installed_toolchains()?)
    } else {
        None
    };
    let mut seen_toolchains: HashSet<&str> = HashSet::new();
    let mut seen_hashes = options
        .keep_rustc_hashes
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    for toolchain in options.keep_toolchains.iter().map(String::as_str).chain(
        installed_toolchains
            .iter()
            .flat_map(|toolchains| toolchains.iter().map(String::as_str)),
    ) {
        if !seen_toolchains.insert(toolchain) {
            continue;
        }
        let rustc_hash = resolver.toolchain_rustc_hash(toolchain)?;
        if seen_hashes.insert(rustc_hash) {
            options.keep_rustc_hashes.push(rustc_hash);
        }
    }

    Ok(())
}

pub fn resolve_command_toolchain_hash_options(
    options: &mut PlannerOptions,
) -> ToolchainHashResult<()> {
    resolve_toolchain_hash_options(options, &CommandToolchainHashResolver)
}

fn parse_installed_toolchains(stdout: &str) -> ToolchainHashResult<Vec<String>> {
    let mut toolchains = Vec::new();
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        if name.starts_with('(') {
            return Err(ToolchainHashError::InvalidInstalledToolchainLine {
                line: line.to_string(),
            });
        }
        toolchains.push(name.to_string());
    }
    Ok(toolchains)
}

fn rustc_version_hash(toolchain: &str, stdout: &str) -> ToolchainHashResult<u64> {
    if stdout.trim().is_empty() {
        return Err(ToolchainHashError::EmptyRustcVersion {
            toolchain: toolchain.to_string(),
        });
    }
    let mut hasher = StableSipHasher128::new();
    stdout.hash(&mut hasher);
    Ok(Hasher::finish(&hasher))
}

fn status_label(status: Option<i32>) -> String {
    status
        .map(|status| status.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct FakeResolver {
        installed: Vec<String>,
        hashes: HashMap<String, u64>,
    }

    impl ToolchainHashResolver for FakeResolver {
        fn installed_toolchains(&self) -> ToolchainHashResult<Vec<String>> {
            Ok(self.installed.clone())
        }

        fn toolchain_rustc_hash(&self, toolchain: &str) -> ToolchainHashResult<u64> {
            self.hashes.get(toolchain).copied().ok_or_else(|| {
                ToolchainHashError::EmptyRustcVersion {
                    toolchain: toolchain.to_string(),
                }
            })
        }
    }

    #[test]
    fn parses_installed_toolchain_names_without_status_annotations() -> Result<(), Box<dyn Error>> {
        let toolchains = parse_installed_toolchains(
            "stable-x86_64-unknown-linux-gnu (default)\nnightly-x86_64-unknown-linux-gnu\n",
        )?;

        assert_eq!(
            toolchains,
            [
                "stable-x86_64-unknown-linux-gnu",
                "nightly-x86_64-unknown-linux-gnu"
            ]
        );
        Ok(())
    }

    #[test]
    fn hashes_rustc_verbose_version_output() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            rustc_version_hash("stable", "rustc 1.90.0\nhost: x86_64-unknown-linux-gnu\n")?,
            rustc_version_hash("stable", "rustc 1.90.0\nhost: x86_64-unknown-linux-gnu\n")?
        );
        assert_ne!(
            rustc_version_hash("stable", "rustc 1.90.0\nhost: x86_64-unknown-linux-gnu\n")?,
            rustc_version_hash("nightly", "rustc 1.91.0\nhost: x86_64-unknown-linux-gnu\n")?
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_rustc_verbose_version_output() {
        assert!(matches!(
            rustc_version_hash("stable", ""),
            Err(ToolchainHashError::EmptyRustcVersion { toolchain }) if toolchain == "stable"
        ));
    }

    #[test]
    fn resolves_named_and_installed_toolchains_into_existing_hashes() -> Result<(), Box<dyn Error>>
    {
        let mut resolver = FakeResolver {
            installed: vec!["stable".to_string(), "beta".to_string()],
            hashes: HashMap::new(),
        };
        resolver.hashes.insert("stable".to_string(), 7);
        resolver.hashes.insert("beta".to_string(), 8);
        resolver.hashes.insert("nightly".to_string(), 7);
        let mut options = PlannerOptions {
            keep_rustc_hashes: vec![3],
            keep_installed_toolchains: true,
            keep_toolchains: vec!["nightly".to_string()],
            ..PlannerOptions::default()
        };

        resolve_toolchain_hash_options(&mut options, &resolver)?;

        assert_eq!(options.keep_rustc_hashes, [3, 7, 8]);
        Ok(())
    }
}
