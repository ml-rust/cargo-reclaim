use std::collections::HashSet;
use std::env;
use std::path::{Component, Path, PathBuf};

use crate::error::ReclaimResult;

use super::foundation::{CargoOutputKind, TargetDirOverride};

const CONFIG_DIR: &str = ".cargo";
const EXTENSIONLESS_CONFIG: &str = "config";
const TOML_CONFIG: &str = "config.toml";
const WORKSPACE_ROOT_TEMPLATE: &str = "{workspace-root}";
const CARGO_CACHE_HOME_TEMPLATE: &str = "{cargo-cache-home}";
const WORKSPACE_PATH_HASH_TEMPLATE: &str = "{workspace-path-hash}";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoOutputDirs {
    pub dirs: Vec<TargetDirOverride>,
    pub unsupported: Vec<CargoConfigUnsupported>,
    pub problems: Vec<CargoConfigProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigUnsupported {
    pub source: String,
    pub reason: CargoConfigUnsupportedReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoConfigUnsupportedReason {
    WorkspacePathHashTemplate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoConfigProblem {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CargoBuildConfig {
    target_dir: Option<ConfigPathValue>,
    build_dir: Option<ConfigPathValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigPathValue {
    path: PathBuf,
    source: String,
    relative_base: PathBuf,
    supports_templates: bool,
}

pub fn resolve_project_output_dirs(project_root: &Path) -> ReclaimResult<CargoOutputDirs> {
    resolve_project_output_dirs_with_env(project_root, env::vars_os())
}

pub fn resolve_project_output_dirs_with_env<I, K, V>(
    project_root: &Path,
    env_vars: I,
) -> ReclaimResult<CargoOutputDirs>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<std::ffi::OsString>,
    V: Into<std::ffi::OsString>,
{
    let env_vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = env_vars
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
    let cargo_home = cargo_home_from_env(project_root, &env_vars);
    let mut config = CargoBuildConfig::default();
    let mut problems = Vec::new();

    if let Some(cargo_home) = cargo_home.as_ref()
        && let Some(path) = selected_config_file(cargo_home)
    {
        merge_config_path(path, &mut config, &mut HashSet::new(), &mut problems)?;
    }

    for ancestor in config_ancestors(project_root) {
        if let Some(path) = selected_config_file(&ancestor.join(CONFIG_DIR)) {
            merge_config_path(path, &mut config, &mut HashSet::new(), &mut problems)?;
        }
    }

    apply_env_overrides(project_root, &env_vars, &mut config);
    let mut output = build_output_dirs(project_root, cargo_home.as_deref(), config)?;
    output.problems = problems;
    Ok(output)
}

fn selected_config_file(cargo_dir: &Path) -> Option<PathBuf> {
    let extensionless = cargo_dir.join(EXTENSIONLESS_CONFIG);
    if extensionless.is_file() {
        return Some(extensionless);
    }

    let toml = cargo_dir.join(TOML_CONFIG);
    toml.is_file().then_some(toml)
}

fn merge_config_path(
    path: PathBuf,
    config: &mut CargoBuildConfig,
    visited: &mut HashSet<PathBuf>,
    problems: &mut Vec<CargoConfigProblem>,
) -> ReclaimResult<()> {
    if !path.is_file() || !visited.insert(lexically_normalize(&path)) {
        return Ok(());
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            problems.push(CargoConfigProblem {
                path,
                message: error.to_string(),
            });
            return Ok(());
        }
    };
    let value = match toml::from_str::<toml::Value>(&contents) {
        Ok(value) => value,
        Err(error) => {
            problems.push(CargoConfigProblem {
                path,
                message: error.to_string(),
            });
            return Ok(());
        }
    };

    for include in include_paths(&value, &path) {
        if include.optional && !include.path.is_file() {
            continue;
        }
        if !include.optional && !include.path.is_file() {
            problems.push(CargoConfigProblem {
                path: include.path,
                message: "included Cargo config does not exist".to_string(),
            });
            continue;
        }
        merge_config_path(include.path, config, visited, problems)?;
    }

    merge_build_config(&value, &path, config);
    Ok(())
}

fn include_paths(value: &toml::Value, config_path: &Path) -> Vec<ConfigInclude> {
    let Some(include) = value.get("include") else {
        return Vec::new();
    };

    let mut includes = Vec::new();
    match include {
        toml::Value::String(path) => {
            includes.push(ConfigInclude::new(
                resolve_include_path(config_path, path),
                false,
            ));
        }
        toml::Value::Array(values) => {
            for value in values {
                match value {
                    toml::Value::String(path) => includes.push(ConfigInclude::new(
                        resolve_include_path(config_path, path),
                        false,
                    )),
                    toml::Value::Table(table) => {
                        if let Some(toml::Value::String(path)) = table.get("path") {
                            let optional = table
                                .get("optional")
                                .and_then(toml::Value::as_bool)
                                .unwrap_or(false);
                            includes.push(ConfigInclude::new(
                                resolve_include_path(config_path, path),
                                optional,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        toml::Value::Table(table) => {
            if let Some(toml::Value::String(path)) = table.get("path") {
                let optional = table
                    .get("optional")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false);
                includes.push(ConfigInclude::new(
                    resolve_include_path(config_path, path),
                    optional,
                ));
            }
        }
        _ => {}
    }

    includes
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigInclude {
    path: PathBuf,
    optional: bool,
}

impl ConfigInclude {
    fn new(path: PathBuf, optional: bool) -> Self {
        Self { path, optional }
    }
}

fn resolve_include_path(config_path: &Path, include: &str) -> PathBuf {
    let path = PathBuf::from(include);
    if path.is_absolute() {
        path
    } else {
        lexically_normalize(
            config_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(path),
        )
    }
}

fn merge_build_config(value: &toml::Value, config_path: &Path, config: &mut CargoBuildConfig) {
    let Some(build) = value.get("build").and_then(toml::Value::as_table) else {
        return;
    };
    let relative_base = config_relative_base(config_path);

    if let Some(toml::Value::String(path)) = build.get("target-dir") {
        config.target_dir = Some(ConfigPathValue {
            path: PathBuf::from(path),
            source: format!("Cargo config build.target-dir ({})", config_path.display()),
            relative_base: relative_base.clone(),
            supports_templates: false,
        });
    }

    if let Some(toml::Value::String(path)) = build.get("build-dir") {
        config.build_dir = Some(ConfigPathValue {
            path: PathBuf::from(path),
            source: format!("Cargo config build.build-dir ({})", config_path.display()),
            relative_base,
            supports_templates: true,
        });
    }
}

fn config_relative_base(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn apply_env_overrides(
    project_root: &Path,
    env_vars: &[(std::ffi::OsString, std::ffi::OsString)],
    config: &mut CargoBuildConfig,
) {
    let direct_target_dir = env_value(env_vars, "CARGO_BUILD_TARGET_DIR");
    let legacy_target_dir = env_value(env_vars, "CARGO_TARGET_DIR");
    if let Some((name, value)) = direct_target_dir
        .map(|value| ("CARGO_BUILD_TARGET_DIR", value))
        .or_else(|| legacy_target_dir.map(|value| ("CARGO_TARGET_DIR", value)))
    {
        config.target_dir = Some(ConfigPathValue {
            path: PathBuf::from(value),
            source: name.to_string(),
            relative_base: project_root.to_path_buf(),
            supports_templates: false,
        });
    }

    if let Some(value) = env_value(env_vars, "CARGO_BUILD_BUILD_DIR") {
        config.build_dir = Some(ConfigPathValue {
            path: PathBuf::from(value),
            source: "CARGO_BUILD_BUILD_DIR".to_string(),
            relative_base: project_root.to_path_buf(),
            supports_templates: true,
        });
    }
}

fn env_value<'a>(
    env_vars: &'a [(std::ffi::OsString, std::ffi::OsString)],
    name: &str,
) -> Option<&'a std::ffi::OsStr> {
    env_vars
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_os_str())
}

fn build_output_dirs(
    project_root: &Path,
    cargo_home: Option<&Path>,
    config: CargoBuildConfig,
) -> ReclaimResult<CargoOutputDirs> {
    let target_is_configured = config.target_dir.is_some();
    let target = config.target_dir.unwrap_or_else(|| ConfigPathValue {
        path: PathBuf::from("target"),
        source: format!("Project default target-dir ({})", project_root.display()),
        relative_base: project_root.to_path_buf(),
        supports_templates: false,
    });

    let mut output = CargoOutputDirs::default();
    let target_path = resolve_config_path(&target, project_root, cargo_home, &mut output);
    if target_is_configured && let Some(path) = target_path.as_ref() {
        output.dirs.push(TargetDirOverride::with_kind(
            path,
            target.source.clone(),
            CargoOutputKind::TargetDir,
        )?);
    }

    if let Some(build_dir) = config.build_dir {
        let build_path = resolve_config_path(&build_dir, project_root, cargo_home, &mut output);
        if let Some(path) = build_path
            && target_path.as_ref().is_none_or(|target_path| {
                lexically_normalize(target_path) != lexically_normalize(&path)
            })
        {
            output.dirs.push(TargetDirOverride::with_kind(
                path,
                build_dir.source,
                CargoOutputKind::BuildDir,
            )?);
        }
    }

    Ok(output)
}

fn resolve_config_path(
    value: &ConfigPathValue,
    project_root: &Path,
    cargo_home: Option<&Path>,
    output: &mut CargoOutputDirs,
) -> Option<PathBuf> {
    let path_text = value.path.to_string_lossy();
    if path_text.contains(WORKSPACE_PATH_HASH_TEMPLATE) {
        output.unsupported.push(CargoConfigUnsupported {
            source: value.source.clone(),
            reason: CargoConfigUnsupportedReason::WorkspacePathHashTemplate,
        });
        return None;
    }

    let mut resolved = path_text.to_string();
    if value.supports_templates {
        resolved = resolved.replace(WORKSPACE_ROOT_TEMPLATE, &project_root.to_string_lossy());
        if let Some(cargo_home) = cargo_home {
            resolved = resolved.replace(CARGO_CACHE_HOME_TEMPLATE, &cargo_home.to_string_lossy());
        }
    }

    let path = PathBuf::from(resolved);
    Some(if path.is_absolute() {
        lexically_normalize(path)
    } else {
        lexically_normalize(value.relative_base.join(path))
    })
}

fn config_ancestors(project_root: &Path) -> Vec<PathBuf> {
    let mut ancestors: Vec<PathBuf> = project_root.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse();
    ancestors
}

fn cargo_home_from_env(
    project_root: &Path,
    env_vars: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Option<PathBuf> {
    env_value(env_vars, "CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env_value(env_vars, "HOME").map(|home| PathBuf::from(home).join(CONFIG_DIR)))
        .map(|path| {
            if path.is_absolute() {
                lexically_normalize(path)
            } else {
                lexically_normalize(project_root.join(path))
            }
        })
}

fn lexically_normalize(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}
