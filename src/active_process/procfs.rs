use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::planner::{ActiveObservation, CargoTool, ObservedCargoProcess, TargetContext};

use super::foundation::{ActiveObservationProvider, ActiveObservationScope};

#[derive(Debug, Clone)]
pub struct ProcfsActiveObservationProvider {
    proc_root: PathBuf,
}

impl ProcfsActiveObservationProvider {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
        }
    }
}

impl Default for ProcfsActiveObservationProvider {
    fn default() -> Self {
        Self::new("/proc")
    }
}

impl ActiveObservationProvider for ProcfsActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> ActiveObservation {
        match observe_procfs(&self.proc_root, scope) {
            Ok(processes) => ActiveObservation::complete(processes),
            Err(ProcfsObservationError::PermissionLimited(reason)) => {
                ActiveObservation::permission_limited(reason)
            }
            Err(ProcfsObservationError::Failed(reason)) => ActiveObservation::failed(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcfsObservationError {
    PermissionLimited(String),
    Failed(String),
}

fn observe_procfs(
    proc_root: &Path,
    scope: &ActiveObservationScope,
) -> Result<Vec<ObservedCargoProcess>, ProcfsObservationError> {
    let entries = fs::read_dir(proc_root).map_err(map_root_error)?;
    let mut processes = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_not_found(&error) => continue,
            Err(error) if is_permission_error(&error) => {
                return Err(ProcfsObservationError::PermissionLimited(format!(
                    "cannot list process entry: {error}"
                )));
            }
            Err(error) => {
                return Err(ProcfsObservationError::Failed(format!(
                    "cannot list process entry: {error}"
                )));
            }
        };

        let file_name = entry.file_name();
        if !is_pid_name(&file_name) {
            continue;
        }

        let process_dir = entry.path();
        match inspect_process(&process_dir) {
            Ok(Some(process)) if process_matches_scope(&process, scope) => processes.push(process),
            Ok(Some(_)) => {}
            Ok(None) => {}
            Err(ProcessReadError::Vanished) => {}
            Err(ProcessReadError::PermissionLimited(reason)) => {
                return Err(ProcfsObservationError::PermissionLimited(reason));
            }
            Err(ProcessReadError::Failed(reason)) => {
                return Err(ProcfsObservationError::Failed(reason));
            }
        }
    }

    Ok(processes)
}

fn process_matches_scope(process: &ObservedCargoProcess, scope: &ActiveObservationScope) -> bool {
    scope.target_contexts().is_empty()
        || scope
            .target_contexts()
            .iter()
            .any(|context| process_matches_context(process, context))
}

fn process_matches_context(process: &ObservedCargoProcess, context: &TargetContext) -> bool {
    if let (Some(project_root), Some(cwd)) = (&context.project_root, &process.cwd)
        && path_is_under(cwd, project_root)
    {
        return true;
    }

    process.referenced_paths.iter().any(|referenced_path| {
        paths_overlap(referenced_path, &context.target_root)
            || context
                .build_root
                .as_ref()
                .is_some_and(|build_root| paths_overlap(referenced_path, build_root))
    })
}

fn map_root_error(error: io::Error) -> ProcfsObservationError {
    if is_permission_error(&error) {
        ProcfsObservationError::PermissionLimited(format!("cannot read process table: {error}"))
    } else {
        ProcfsObservationError::Failed(format!("cannot read process table: {error}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessReadError {
    Vanished,
    PermissionLimited(String),
    Failed(String),
}

fn inspect_process(process_dir: &Path) -> Result<Option<ObservedCargoProcess>, ProcessReadError> {
    let comm = match read_comm(&process_dir.join("comm")) {
        Ok(comm) => comm,
        Err(ProcessReadError::Vanished) => return Ok(None),
        Err(error) => return Err(error),
    };
    let cmdline = match read_cmdline(&process_dir.join("cmdline")) {
        Ok(cmdline) => cmdline,
        Err(ProcessReadError::Vanished) => return Ok(None),
        Err(error) => return Err(error),
    };
    let tool = detect_tool(&comm, &cmdline);
    let Some(tool) = tool else {
        return Ok(None);
    };

    let cwd = match fs::read_link(process_dir.join("cwd")) {
        Ok(path) => Some(path),
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) if is_permission_error(&error) => {
            return Err(ProcessReadError::PermissionLimited(format!(
                "{} cwd is not inspectable: {error}",
                tool_label(tool)
            )));
        }
        Err(error) => {
            return Err(ProcessReadError::Failed(format!(
                "{} cwd cannot be read: {error}",
                tool_label(tool)
            )));
        }
    };

    let mut process = ObservedCargoProcess::new(tool);
    if let Some(cwd) = cwd.as_ref() {
        process.cwd = Some(cwd.clone());
    }
    process.referenced_paths = extract_referenced_paths(&cmdline, cwd.as_deref());
    Ok(Some(process))
}

fn read_comm(path: &Path) -> Result<String, ProcessReadError> {
    let contents = read_to_string(path)?;
    Ok(contents.trim_end_matches(['\n', '\r']).to_string())
}

fn read_cmdline(path: &Path) -> Result<Vec<String>, ProcessReadError> {
    let bytes = read(path)?;
    Ok(bytes
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect())
}

fn read_to_string(path: &Path) -> Result<String, ProcessReadError> {
    fs::read_to_string(path).map_err(|error| map_process_read_error(path, error))
}

fn read(path: &Path) -> Result<Vec<u8>, ProcessReadError> {
    fs::read(path).map_err(|error| map_process_read_error(path, error))
}

fn map_process_read_error(path: &Path, error: io::Error) -> ProcessReadError {
    if is_not_found(&error) {
        ProcessReadError::Vanished
    } else if is_permission_error(&error) {
        ProcessReadError::PermissionLimited(format!(
            "{} is not inspectable: {error}",
            path.display()
        ))
    } else {
        ProcessReadError::Failed(format!("{} cannot be read: {error}", path.display()))
    }
}

fn detect_tool(comm: &str, cmdline: &[String]) -> Option<CargoTool> {
    tool_from_name(comm).or_else(|| {
        cmdline
            .first()
            .and_then(|program| Path::new(program).file_name())
            .and_then(OsStr::to_str)
            .and_then(tool_from_name)
    })
}

fn tool_from_name(name: &str) -> Option<CargoTool> {
    match name {
        "cargo" => Some(CargoTool::Cargo),
        "rustc" => Some(CargoTool::Rustc),
        _ => None,
    }
}

fn extract_referenced_paths(cmdline: &[String], cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut index = 1;

    while index < cmdline.len() {
        let arg = &cmdline[index];
        match arg.as_str() {
            "--target-dir" | "--manifest-path" | "--out-dir" => {
                if let Some(value) = cmdline.get(index + 1) {
                    push_resolved_path(&mut paths, value, cwd);
                    index += 2;
                    continue;
                }
            }
            "-L" => {
                if let Some(value) = cmdline.get(index + 1) {
                    push_library_search_path(&mut paths, value, cwd);
                    index += 2;
                    continue;
                }
            }
            "--extern" => {
                if let Some(value) = cmdline.get(index + 1) {
                    push_extern_path(&mut paths, value, cwd);
                    index += 2;
                    continue;
                }
            }
            "--emit" => {
                if let Some(value) = cmdline.get(index + 1) {
                    push_emit_paths(&mut paths, value, cwd);
                    index += 2;
                    continue;
                }
            }
            _ => {}
        }

        if let Some(value) = arg.strip_prefix("--target-dir=") {
            push_resolved_path(&mut paths, value, cwd);
        } else if let Some(value) = arg.strip_prefix("--manifest-path=") {
            push_resolved_path(&mut paths, value, cwd);
        } else if let Some(value) = arg.strip_prefix("--out-dir=") {
            push_resolved_path(&mut paths, value, cwd);
        } else if let Some(value) = arg.strip_prefix("--extern=") {
            push_extern_path(&mut paths, value, cwd);
        } else if let Some(value) = arg.strip_prefix("--emit=") {
            push_emit_paths(&mut paths, value, cwd);
        } else if let Some(value) = arg.strip_prefix("-L")
            && !value.is_empty()
        {
            push_library_search_path(&mut paths, value, cwd);
        }

        index += 1;
    }

    paths
}

fn push_library_search_path(paths: &mut Vec<PathBuf>, value: &str, cwd: Option<&Path>) {
    let path = value.split_once('=').map(|(_, path)| path).unwrap_or(value);
    push_resolved_path(paths, path, cwd);
}

fn push_extern_path(paths: &mut Vec<PathBuf>, value: &str, cwd: Option<&Path>) {
    if let Some((_, path)) = value.split_once('=') {
        push_resolved_path(paths, path, cwd);
    }
}

fn push_emit_paths(paths: &mut Vec<PathBuf>, value: &str, cwd: Option<&Path>) {
    for part in value.split(',') {
        if let Some(path) = part.strip_prefix("dep-info=") {
            push_resolved_path(paths, path, cwd);
        }
    }
}

fn push_resolved_path(paths: &mut Vec<PathBuf>, value: &str, cwd: Option<&Path>) {
    if value.is_empty() {
        return;
    }

    let path = PathBuf::from(value);
    let resolved = if path.is_absolute() {
        path
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        path
    };
    paths.push(lexically_normalize(resolved));
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn lexically_normalize(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

fn is_pid_name(name: &OsStr) -> bool {
    name.to_string_lossy()
        .bytes()
        .all(|byte| byte.is_ascii_digit())
}

fn is_not_found(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

fn is_permission_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::UnexpectedEof
    )
}

fn tool_label(tool: CargoTool) -> &'static str {
    match tool {
        CargoTool::Cargo => "cargo",
        CargoTool::Rustc => "rustc",
    }
}
