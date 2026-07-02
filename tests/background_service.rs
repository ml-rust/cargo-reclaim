use std::error::Error;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cargo_reclaim::{
    BackgroundRunEventKind, BackgroundServiceClock, BackgroundServiceError,
    BackgroundServiceOptions, BackgroundServicePaths, BackgroundServiceSleeper,
    BackgroundServiceState, BackgroundServiceStatus, PersistedTimestamp,
    PlatformBackgroundServiceCycleRunner, load_config_from_path, read_background_run_log,
    read_background_service_state, run_background_service_with_runtime,
    write_background_service_state,
};

#[test]
fn service_lock_rejects_second_instance_while_held() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_service_lock")?;
    let config_path = write_config(temp.path(), "")?;
    let config = load_config_from_path(&config_path)?;
    let paths = BackgroundServicePaths::new(temp.path().join("state"), temp.path().join("logs"));
    fs::create_dir_all(&paths.state_dir)?;
    let _lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.lock_path)?;
    let mut clock = FakeClock::new([1_000, 1_001]);
    let mut sleeper = FakeSleeper::default();
    let mut runner = PlatformBackgroundServiceCycleRunner;

    let error = run_background_service_with_runtime(
        BackgroundServiceOptions {
            config_path,
            state_dir: paths.state_dir,
            log_dir: paths.log_dir,
            mode: None,
            max_cycles: Some(1),
        },
        &config,
        &mut clock,
        &mut sleeper,
        &mut runner,
    )
    .expect_err("second instance should not acquire lock");

    assert!(matches!(
        error,
        BackgroundServiceError::AlreadyRunning { .. }
    ));
    Ok(())
}

#[test]
fn bounded_service_run_writes_state_and_two_started_records() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_service_bounded")?;
    let project = write_project(temp.path())?;
    let config_path = write_config(
        temp.path(),
        &format!(
            "roots = [{}]\n[background]\ncheck_every = \"1s\"\n",
            toml_string(&project)
        ),
    )?;
    let config = load_config_from_path(&config_path)?;
    let state_dir = temp.path().join("state");
    let log_dir = temp.path().join("logs");
    let mut clock = FakeClock::new([1_000, 1_100, 1_200]);
    let mut sleeper = FakeSleeper::default();
    let mut runner = PlatformBackgroundServiceCycleRunner;

    let summary = run_background_service_with_runtime(
        BackgroundServiceOptions {
            config_path,
            state_dir: state_dir.clone(),
            log_dir: log_dir.clone(),
            mode: None,
            max_cycles: Some(2),
        },
        &config,
        &mut clock,
        &mut sleeper,
        &mut runner,
    )?;

    assert_eq!(summary.cycles_completed, 2);
    assert_eq!(summary.state.status, BackgroundServiceStatus::Stopped);
    assert_eq!(sleeper.sleeps, vec![Duration::from_secs(1)]);
    let state = read_background_service_state(state_dir.join("service-state.json"))?
        .ok_or_else(|| std::io::Error::other("missing service state"))?;
    assert_eq!(state.status, BackgroundServiceStatus::Stopped);
    assert_eq!(state.consecutive_failures, 0);
    assert_eq!(
        state.last_run_id.as_deref(),
        Some("scheduler-19700101T002000Z")
    );
    assert!(state.next_run_at.is_none());
    let records = read_background_run_log(log_dir.join("runs.jsonl"))?;
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == BackgroundRunEventKind::Started)
            .count(),
        2
    );
    assert!(
        state_dir
            .join("plans/cargo-reclaim-19700101T002000Z.json")
            .is_file()
    );
    Ok(())
}

#[test]
fn state_read_reports_missing_and_existing_state() -> Result<(), Box<dyn Error>> {
    let temp = TestTemp::new("background_service_state")?;
    let state_path = temp.path().join("state/service-state.json");

    assert!(read_background_service_state(&state_path)?.is_none());
    let missing = BackgroundServiceState::missing();
    assert_eq!(missing.status, BackgroundServiceStatus::Unknown);

    let state = BackgroundServiceState {
        schema_version: 1,
        status: BackgroundServiceStatus::Running,
        pid: Some(123),
        started_at: Some(PersistedTimestamp {
            unix_seconds: 10,
            nanoseconds: 0,
        }),
        last_run_id: Some("scheduler-test".to_owned()),
        last_run_at: None,
        next_run_at: None,
        consecutive_failures: 0,
        last_problem: None,
    };
    write_background_service_state(&state_path, &state)?;

    assert_eq!(read_background_service_state(&state_path)?, Some(state));
    Ok(())
}

#[derive(Default)]
struct FakeSleeper {
    sleeps: Vec<Duration>,
}

impl BackgroundServiceSleeper for FakeSleeper {
    fn sleep(&mut self, duration: Duration) {
        self.sleeps.push(duration);
    }
}

struct FakeClock {
    times: Vec<SystemTime>,
    index: usize,
}

impl FakeClock {
    fn new<const N: usize>(seconds: [u64; N]) -> Self {
        Self {
            times: seconds
                .into_iter()
                .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds))
                .collect(),
            index: 0,
        }
    }
}

impl BackgroundServiceClock for FakeClock {
    fn now(&mut self) -> SystemTime {
        let fallback = self.times.last().copied().unwrap_or(UNIX_EPOCH);
        let time = self.times.get(self.index).copied().unwrap_or(fallback);
        self.index += 1;
        time
    }
}

fn write_config(path: &Path, body: &str) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = path.join("reclaim.toml");
    fs::write(&config_path, format!("version = 1\n{body}"))?;
    Ok(config_path)
}

fn write_project(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let project = path.join("project");
    fs::create_dir_all(project.join("target/debug/incremental"))?;
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"sample\"\n")?;
    fs::write(project.join("target/debug/incremental/cache.bin"), b"cache")?;
    Ok(project)
}

fn toml_string(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('\\', "\\\\"))
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
