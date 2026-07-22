use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

use super::common::detect_tool;

/// Outcome of a disruptive build-kill sweep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KillReport {
    /// Build processes sent SIGTERM.
    pub signalled: usize,
    /// Processes still alive after the grace period, then SIGKILLed.
    pub force_killed: usize,
}

/// Stop the `cargo`/`rustc` build processes whose working directory is under one
/// of `roots`: send SIGTERM, wait `grace`, then SIGKILL any survivors.
///
/// Only build processes inside the managed `roots` are ever touched — never a
/// build elsewhere, and never cargo-reclaim itself (its process name is not
/// `cargo`/`rustc`). This is deliberately destructive: it is the "stop the runaway
/// build and reset its target" action a disruptive trigger performs before cleaning.
pub fn kill_active_builds_under_roots(roots: &[PathBuf], grace: Duration) -> KillReport {
    let self_pid = std::process::id();
    let mut system = System::new();
    let refresh = ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::Always)
        .with_cwd(UpdateKind::Always);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);

    let targets: Vec<Pid> = system
        .processes()
        .iter()
        .filter(|(pid, process)| pid.as_u32() != self_pid && is_build_under_roots(process, roots))
        .map(|(pid, _)| *pid)
        .collect();

    let mut report = KillReport::default();
    for pid in &targets {
        if let Some(process) = system.process(*pid)
            && process.kill_with(Signal::Term).unwrap_or(false)
        {
            report.signalled += 1;
        }
    }

    if report.signalled == 0 {
        return report;
    }

    std::thread::sleep(grace);

    // Re-check the targeted PIDs; any still alive ignored SIGTERM and gets SIGKILL.
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&targets),
        true,
        ProcessRefreshKind::nothing(),
    );
    for pid in &targets {
        if let Some(process) = system.process(*pid)
            && process.kill()
        {
            report.force_killed += 1;
        }
    }

    report
}

fn is_build_under_roots(process: &sysinfo::Process, roots: &[PathBuf]) -> bool {
    let name = process.name().to_string_lossy();
    let cmdline: Vec<String> = process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    build_process_matches(&name, &cmdline, process.cwd(), roots)
}

/// Whether a process is a `cargo`/`rustc` build whose working directory lies under
/// one of `roots`. The roots guard is what keeps the kill scoped to the managed
/// tree; a process with no cwd, or one outside the roots, is never a target.
fn build_process_matches(
    name: &str,
    cmdline: &[String],
    cwd: Option<&Path>,
    roots: &[PathBuf],
) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    if !roots
        .iter()
        .any(|root| cwd == root.as_path() || cwd.starts_with(root))
    {
        return false;
    }
    detect_tool(name, cmdline).is_some()
}

#[cfg(test)]
mod tests {
    use super::build_process_matches;
    use std::path::{Path, PathBuf};

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/home/u/Projects")]
    }

    #[test]
    fn matches_cargo_build_under_root() {
        assert!(build_process_matches(
            "cargo",
            &["cargo".to_string(), "build".to_string()],
            Some(Path::new("/home/u/Projects/app")),
            &roots(),
        ));
    }

    #[test]
    fn matches_rustc_under_root() {
        assert!(build_process_matches(
            "rustc",
            &["rustc".to_string()],
            Some(Path::new("/home/u/Projects/app/crate")),
            &roots(),
        ));
    }

    #[test]
    fn skips_build_outside_roots() {
        assert!(!build_process_matches(
            "cargo",
            &["cargo".to_string()],
            Some(Path::new("/tmp/other")),
            &roots(),
        ));
    }

    #[test]
    fn skips_non_build_processes_and_self() {
        assert!(!build_process_matches(
            "bash",
            &["bash".to_string()],
            Some(Path::new("/home/u/Projects/app")),
            &roots(),
        ));
        // cargo-reclaim's own process name is not `cargo`/`rustc`, so it is never a target.
        assert!(!build_process_matches(
            "cargo-reclaim",
            &["cargo-reclaim".to_string()],
            Some(Path::new("/home/u/Projects")),
            &roots(),
        ));
    }

    #[test]
    fn skips_process_without_cwd() {
        assert!(!build_process_matches(
            "cargo",
            &["cargo".to_string()],
            None,
            &roots(),
        ));
    }
}
