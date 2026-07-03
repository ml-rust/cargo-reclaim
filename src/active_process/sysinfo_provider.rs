use std::path::Path;

use crate::planner::{ActiveObservation, ObservedCargoProcess};

use super::common::{detect_tool, extract_referenced_paths, process_matches_scope};
use super::foundation::{ActiveObservationProvider, ActiveObservationScope};

#[derive(Debug, Clone, Default)]
pub struct SysinfoActiveObservationProvider;

impl ActiveObservationProvider for SysinfoActiveObservationProvider {
    fn observe(&self, scope: &ActiveObservationScope) -> ActiveObservation {
        let processes = observe_sysinfo_processes(scope);
        ActiveObservation::complete(processes)
    }
}

fn observe_sysinfo_processes(scope: &ActiveObservationScope) -> Vec<ObservedCargoProcess> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );

    system
        .processes()
        .values()
        .filter_map(|process| {
            let name = process.name().to_string_lossy();
            let cmdline = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            observed_process_from_parts(&name, &cmdline, process.cwd())
        })
        .filter(|process| process_matches_scope(process, scope))
        .collect()
}

fn observed_process_from_parts(
    name: &str,
    cmdline: &[String],
    cwd: Option<&Path>,
) -> Option<ObservedCargoProcess> {
    let tool = detect_tool(name, cmdline)?;
    let mut process = ObservedCargoProcess::new(tool);
    if let Some(cwd) = cwd {
        process.cwd = Some(cwd.to_path_buf());
    }
    process.referenced_paths = extract_referenced_paths(cmdline, cwd);
    Some(process)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::planner::{CargoTool, TargetContext};

    use super::*;

    #[test]
    fn sysinfo_parts_match_cargo_by_cwd_scope() {
        let scope = ActiveObservationScope::from_target_contexts([TargetContext::new(
            "/work/crate/target",
        )
        .with_project_root("/work/crate")]);

        let process = observed_process_from_parts(
            "cargo",
            &["cargo".to_string(), "test".to_string()],
            Some(Path::new("/work/crate")),
        )
        .expect("cargo should be detected");

        assert_eq!(process.tool, CargoTool::Cargo);
        assert!(process_matches_scope(&process, &scope));
    }

    #[test]
    fn sysinfo_parts_match_rustc_by_referenced_path_scope() {
        let scope = ActiveObservationScope::from_target_contexts([TargetContext::new(
            "/work/crate/target",
        )
        .with_build_root("/work/crate/target/debug")]);

        let process = observed_process_from_parts(
            "rustc",
            &[
                "rustc".to_string(),
                "--out-dir".to_string(),
                "target/debug/deps".to_string(),
            ],
            Some(Path::new("/work/crate")),
        )
        .expect("rustc should be detected");

        assert_eq!(process.tool, CargoTool::Rustc);
        assert_eq!(
            process.referenced_paths,
            vec![PathBuf::from("/work/crate/target/debug/deps")]
        );
        assert!(process_matches_scope(&process, &scope));
    }

    #[test]
    fn sysinfo_parts_ignore_unrelated_processes() {
        assert!(observed_process_from_parts("bash", &["bash".to_string()], None).is_none());
    }
}
