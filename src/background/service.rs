mod model;
mod request;

use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::active_process::platform_active_observation_provider;
use crate::config::ReclaimConfig;
use crate::persistence::PersistedTimestamp;

use super::{BackgroundRunReport, BackgroundRunRequest, run_background_cleanup_cycle};
use request::{BackgroundCycleRequestContext, scheduler_mode_from_config};

pub use model::{
    BACKGROUND_SERVICE_STATE_SCHEMA_VERSION, BackgroundServiceError, BackgroundServiceOptions,
    BackgroundServicePaths, BackgroundServiceResult, BackgroundServiceRunSummary,
    BackgroundServiceState, BackgroundServiceStatus, DEFAULT_BACKGROUND_CHECK_EVERY,
};

pub trait BackgroundServiceClock {
    fn now(&mut self) -> SystemTime;
}

pub trait BackgroundServiceSleeper {
    fn sleep(&mut self, duration: Duration);
}

pub trait BackgroundServiceCycleRunner {
    fn run_cycle(
        &mut self,
        request: BackgroundRunRequest,
    ) -> BackgroundServiceResult<BackgroundRunReport>;
}

#[derive(Debug, Default)]
pub struct SystemBackgroundServiceClock;

impl BackgroundServiceClock for SystemBackgroundServiceClock {
    fn now(&mut self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Debug, Default)]
pub struct ThreadBackgroundServiceSleeper;

impl BackgroundServiceSleeper for ThreadBackgroundServiceSleeper {
    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

#[derive(Debug, Default)]
pub struct PlatformBackgroundServiceCycleRunner;

impl BackgroundServiceCycleRunner for PlatformBackgroundServiceCycleRunner {
    fn run_cycle(
        &mut self,
        request: BackgroundRunRequest,
    ) -> BackgroundServiceResult<BackgroundRunReport> {
        let provider = platform_active_observation_provider();
        run_background_cleanup_cycle(request, &provider).map_err(BackgroundServiceError::from)
    }
}

pub fn run_background_service(
    options: BackgroundServiceOptions,
    config: &ReclaimConfig,
) -> BackgroundServiceResult<BackgroundServiceRunSummary> {
    let mut clock = SystemBackgroundServiceClock;
    let mut sleeper = ThreadBackgroundServiceSleeper;
    let mut runner = PlatformBackgroundServiceCycleRunner;
    run_background_service_with_runtime(options, config, &mut clock, &mut sleeper, &mut runner)
}

pub fn run_background_service_with_runtime(
    options: BackgroundServiceOptions,
    config: &ReclaimConfig,
    clock: &mut impl BackgroundServiceClock,
    sleeper: &mut impl BackgroundServiceSleeper,
    runner: &mut impl BackgroundServiceCycleRunner,
) -> BackgroundServiceResult<BackgroundServiceRunSummary> {
    let paths = BackgroundServicePaths::new(&options.state_dir, &options.log_dir);
    let _lock = ServiceLock::acquire(&paths.lock_path)?;
    ensure_service_dirs(&paths)?;

    let started_at = persisted_timestamp(clock.now())?;
    let mut state = BackgroundServiceState::running(started_at);
    write_background_service_state(&paths.state_path, &state)?;

    let interval = config
        .background
        .check_every
        .unwrap_or(DEFAULT_BACKGROUND_CHECK_EVERY);
    let request_context = BackgroundCycleRequestContext::from_config(
        config,
        &options.config_path,
        match options.mode {
            Some(mode) => mode,
            None => scheduler_mode_from_config(config)?,
        },
    )?;
    let mut cycles_completed = 0;

    loop {
        let now = clock.now();
        let stamp = build_service_timestamp(now);
        let run_id = format!("scheduler-{stamp}");
        let plan_path = paths.plans_dir.join(format!("cargo-reclaim-{stamp}.json"));
        let request =
            request_context.request(run_id.clone(), paths.runs_log_path.clone(), plan_path, now)?;

        match runner.run_cycle(request) {
            Ok(_) => {
                state.last_problem = None;
                state.consecutive_failures = 0;
            }
            Err(error) => {
                state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                state.last_problem = Some(error.to_string());
            }
        }
        cycles_completed += 1;

        state.last_run_id = Some(run_id);
        state.last_run_at = Some(persisted_timestamp(now)?);
        let next_run_at = now + interval;
        state.next_run_at = Some(persisted_timestamp(next_run_at)?);
        write_background_service_state(&paths.state_path, &state)?;

        if options
            .max_cycles
            .is_some_and(|max_cycles| cycles_completed >= max_cycles)
        {
            state.status = BackgroundServiceStatus::Stopped;
            state.pid = None;
            state.next_run_at = None;
            write_background_service_state(&paths.state_path, &state)?;
            return Ok(BackgroundServiceRunSummary {
                state,
                cycles_completed,
            });
        }

        sleeper.sleep(interval);
    }
}

pub fn read_background_service_state(
    path: impl AsRef<Path>,
) -> BackgroundServiceResult<Option<BackgroundServiceState>> {
    let path = path.as_ref();
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|source| BackgroundServiceError::Json {
                path: path.to_path_buf(),
                source,
            }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(BackgroundServiceError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub fn write_background_service_state(
    path: impl AsRef<Path>,
    state: &BackgroundServiceState,
) -> BackgroundServiceResult<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| BackgroundServiceError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let contents =
        serde_json::to_vec_pretty(state).map_err(|source| BackgroundServiceError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
    fs::write(path, contents).map_err(|source| BackgroundServiceError::Io {
        path: path.to_path_buf(),
        source,
    })
}

struct ServiceLock {
    path: std::path::PathBuf,
}

impl ServiceLock {
    fn acquire(path: &Path) -> BackgroundServiceResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| BackgroundServiceError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => {
                write_lock_file(file, path)?;
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                if path.is_file() {
                    Err(BackgroundServiceError::AlreadyRunning {
                        lock_path: path.to_path_buf(),
                    })
                } else {
                    Err(BackgroundServiceError::StaleLock {
                        lock_path: path.to_path_buf(),
                    })
                }
            }
            Err(source) => Err(BackgroundServiceError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

impl Drop for ServiceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_lock_file(file: File, path: &Path) -> BackgroundServiceResult<()> {
    let state = serde_json::json!({
        "schema_version": BACKGROUND_SERVICE_STATE_SCHEMA_VERSION,
        "pid": std::process::id(),
    });
    serde_json::to_writer(file, &state).map_err(|source| BackgroundServiceError::Serialize {
        path: path.to_path_buf(),
        source,
    })
}

fn ensure_service_dirs(paths: &BackgroundServicePaths) -> BackgroundServiceResult<()> {
    for path in [&paths.state_dir, &paths.log_dir, &paths.plans_dir] {
        fs::create_dir_all(path).map_err(|source| BackgroundServiceError::Io {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

fn persisted_timestamp(time: SystemTime) -> BackgroundServiceResult<PersistedTimestamp> {
    PersistedTimestamp::from_system_time(time).map_err(BackgroundServiceError::Timestamp)
}

fn build_service_timestamp(now: SystemTime) -> String {
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_seconds = duration.as_secs();
    let days = (total_seconds / 86_400) as i64;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let shifted = days_since_unix_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}
