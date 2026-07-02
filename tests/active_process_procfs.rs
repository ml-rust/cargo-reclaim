#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    ActiveObservationProvider, ActiveObservationScope, CargoTool, ProcessView,
    ProcfsActiveObservationProvider, TargetContext,
};

#[test]
fn procfs_observes_exact_tools_and_structured_paths() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("procfs_exact_tools")?;
    let project = temp.path.join("workspace/project");
    let target = project.join("target");
    fs::create_dir_all(&target)?;
    process(
        temp.path(),
        "100",
        "cargo",
        &project,
        &["/usr/bin/cargo", "build", "--target-dir", "target"],
    )?;
    process(
        temp.path(),
        "101",
        "rustc",
        &project,
        &[
            "/usr/bin/rustc",
            "--out-dir",
            "target/debug/deps",
            "--emit=dep-info=target/debug/deps/unit.d,link",
            "-Ldependency=target/debug/deps",
            "-L",
            "dependency=target/debug/build",
            "--extern",
            "sample=target/debug/deps/libsample.rlib",
        ],
    )?;
    process(
        temp.path(),
        "102",
        "cargo-watch",
        &project,
        &["cargo-watch", "--target-dir", "target"],
    )?;

    let observation = ProcfsActiveObservationProvider::new(temp.path())
        .observe(&ActiveObservationScope::default());
    let ProcessView::Complete { processes } = observation.process_view else {
        panic!("expected complete observation");
    };

    assert_eq!(processes.len(), 2);
    let cargo = processes
        .iter()
        .find(|process| process.tool == CargoTool::Cargo)
        .ok_or("missing cargo process")?;
    let rustc = processes
        .iter()
        .find(|process| process.tool == CargoTool::Rustc)
        .ok_or("missing rustc process")?;
    assert_eq!(cargo.cwd.as_deref(), Some(project.as_path()));
    assert_eq!(cargo.referenced_paths, std::slice::from_ref(&target));
    assert!(
        rustc
            .referenced_paths
            .contains(&target.join("debug/deps/libsample.rlib"))
    );
    assert!(
        rustc
            .referenced_paths
            .contains(&target.join("debug/deps/unit.d"))
    );
    assert!(rustc.referenced_paths.contains(&target.join("debug/build")));
    Ok(())
}

#[test]
fn procfs_uses_program_basename_when_comm_is_not_exact() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("procfs_basename")?;
    let project = temp.path.join("workspace/project");
    fs::create_dir_all(&project)?;
    process(
        temp.path(),
        "100",
        "build-script",
        &project,
        &["/toolchains/stable/bin/rustc", "--out-dir=target/debug"],
    )?;

    let observation = ProcfsActiveObservationProvider::new(temp.path())
        .observe(&ActiveObservationScope::default());
    let ProcessView::Complete { processes } = observation.process_view else {
        panic!("expected complete observation");
    };

    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0].tool, CargoTool::Rustc);
    Ok(())
}

#[test]
fn procfs_filters_processes_to_scope() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("procfs_scope")?;
    let project = temp.path.join("workspace/project");
    let unrelated = temp.path.join("workspace/other");
    process(
        temp.path(),
        "100",
        "cargo",
        &unrelated,
        &["cargo", "build", "--target-dir", "target"],
    )?;
    let scope =
        ActiveObservationScope::from_target_contexts([
            TargetContext::new(project.join("target")).with_project_root(project)
        ]);

    let observation = ProcfsActiveObservationProvider::new(temp.path()).observe(&scope);
    let ProcessView::Complete { processes } = observation.process_view else {
        panic!("expected complete observation");
    };

    assert!(processes.is_empty());
    Ok(())
}

#[test]
fn procfs_ignores_vanished_pid() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("procfs_vanished")?;
    fs::create_dir(temp.path().join("100"))?;

    let observation = ProcfsActiveObservationProvider::new(temp.path())
        .observe(&ActiveObservationScope::default());
    let ProcessView::Complete { processes } = observation.process_view else {
        panic!("expected complete observation");
    };

    assert!(processes.is_empty());
    Ok(())
}

#[test]
fn procfs_reports_permission_limited_for_uninspectable_tool_process() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::PermissionsExt;

    let temp = TestTemp::new("procfs_permission_limited")?;
    let project = temp.path.join("workspace/project");
    let pid = process(temp.path(), "100", "cargo", &project, &["cargo", "build"])?;
    let cmdline = pid.join("cmdline");
    let mut permissions = fs::metadata(&cmdline)?.permissions();
    permissions.set_mode(0o000);
    fs::set_permissions(&cmdline, permissions)?;

    let observation = ProcfsActiveObservationProvider::new(temp.path())
        .observe(&ActiveObservationScope::default());

    let ProcessView::PermissionLimited { reason } = observation.process_view else {
        panic!("expected permission-limited observation");
    };
    assert!(reason.contains("cmdline"));
    Ok(())
}

fn process(
    proc_root: &Path,
    pid: &str,
    comm: &str,
    cwd: &Path,
    cmdline: &[&str],
) -> Result<PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    fs::create_dir_all(cwd)?;
    let pid_dir = proc_root.join(pid);
    fs::create_dir(&pid_dir)?;
    fs::write(pid_dir.join("comm"), format!("{comm}\n"))?;
    let mut bytes = Vec::new();
    for arg in cmdline {
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
    }
    fs::write(pid_dir.join("cmdline"), bytes)?;
    symlink(cwd, pid_dir.join("cwd"))?;
    Ok(pid_dir)
}

struct TestTemp {
    path: PathBuf,
}

impl TestTemp {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cargo_reclaim_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
