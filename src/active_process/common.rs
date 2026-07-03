use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::planner::{CargoTool, ObservedCargoProcess, TargetContext};

use super::foundation::ActiveObservationScope;

pub(super) fn process_matches_scope(
    process: &ObservedCargoProcess,
    scope: &ActiveObservationScope,
) -> bool {
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

pub(super) fn detect_tool(program_name: &str, cmdline: &[String]) -> Option<CargoTool> {
    tool_from_name(program_name).or_else(|| {
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

pub(super) fn extract_referenced_paths(cmdline: &[String], cwd: Option<&Path>) -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use crate::planner::{CargoTool, ObservedCargoProcess, TargetContext};

    use super::*;

    #[test]
    fn detects_tool_from_comm_or_program_path() {
        assert_eq!(detect_tool("cargo", &[]), Some(CargoTool::Cargo));
        assert_eq!(
            detect_tool("", &["/toolchains/stable/bin/rustc".to_string()]),
            Some(CargoTool::Rustc)
        );
        assert_eq!(detect_tool("bash", &["bash".to_string()]), None);
    }

    #[test]
    fn extracts_paths_from_cargo_and_rustc_arguments() {
        let paths = extract_referenced_paths(
            &[
                "rustc".to_string(),
                "--target-dir".to_string(),
                "target".to_string(),
                "--extern=foo=target/debug/deps/libfoo.rlib".to_string(),
                "--emit".to_string(),
                "dep-info=target/debug/foo.d,link".to_string(),
                "-Ldependency=target/debug/deps".to_string(),
            ],
            Some(Path::new("/work/crate")),
        );

        assert!(paths.contains(&PathBuf::from("/work/crate/target")));
        assert!(paths.contains(&PathBuf::from("/work/crate/target/debug/deps/libfoo.rlib")));
        assert!(paths.contains(&PathBuf::from("/work/crate/target/debug/foo.d")));
        assert!(paths.contains(&PathBuf::from("/work/crate/target/debug/deps")));
    }

    #[test]
    fn process_scope_matches_project_cwd_or_target_reference() {
        let scope = ActiveObservationScope::from_target_contexts([TargetContext::new(
            "/work/crate/target",
        )
        .with_project_root("/work/crate")]);

        assert!(process_matches_scope(
            &ObservedCargoProcess::new(CargoTool::Cargo).with_cwd("/work/crate"),
            &scope
        ));
        assert!(process_matches_scope(
            &ObservedCargoProcess::new(CargoTool::Rustc)
                .with_referenced_path("/work/crate/target/debug/deps/libx.rlib"),
            &scope
        ));
    }
}
