# Scheduler

The scheduler runs cargo-reclaim without a terminal. It has two parts:

- **Platform artifacts.** `scheduler install` writes a user service definition for your platform. The service starts `scheduler service run`.
- **Resident service.** `scheduler service run` loops on the triggers in the config file. It writes state and run logs.

For every config key, see [Config file reference](config.md).

## Subcommands

| Command                                  | Effect                                                                                  |
| ---------------------------------------- | --------------------------------------------------------------------------------------- |
| `scheduler preview`                      | Prints the artifacts that `install` writes. Writes nothing.                             |
| `scheduler install`                      | Writes and registers the platform artifacts. Pass `--dry-run` to print the plan only.     |
| `scheduler uninstall`                    | Removes the platform artifacts. Pass `--dry-run` to print the plan only.                  |
| `scheduler run`                          | Runs one cycle with an explicit `--run-id`, `--log-path`, and `--plan-path`.            |
| `scheduler service run`                  | Runs the resident loop. Pass `--max-cycles <n>` to stop after `n` cycles.                |
| `scheduler service status`               | Prints the service state and recent run summaries.                                      |

`preview`, `install`, and `uninstall` require `--platform <systemd-user|launchd|task-scheduler>` and `--config <path>`.
Pass `--json` to any command for a JSON document.

`install` runs when you omit `--dry-run`. It does not ask for `--yes`.

## Platforms

| Platform         | Artifact                                                                     | Start behavior                                  |
| ---------------- | ---------------------------------------------------------------------------- | ----------------------------------------------- |
| `systemd-user`   | User unit with `ExecStart`, `Restart=on-failure`, and `RestartSec=30`. Also a timer that starts the unit daily at `[scheduler].at`. | Starts with `default.target` for the user. The timer runs at `[scheduler].at`, default `03:00`. |
| `launchd`        | LaunchAgent plist in `~/Library/LaunchAgents`, with `KeepAlive` and `RunAtLoad`. | Starts at load and restarts when it exits.   |
| `task-scheduler` | Scheduled task that runs PowerShell, with a logon trigger.                   | Starts at logon. Retries a failed run three times, one minute apart. |

The generated runner runs `scheduler service run --config <path> --json`.
It appends output to the run log path.

### Default paths

| Platform         | Default state directory                              | Default log directory                        |
| ---------------- | ---------------------------------------------------- | -------------------------------------------- |
| `systemd-user`   | `~/.local/state/cargo-reclaim`                       | `<state_dir>/logs`                           |
| `launchd`        | `~/Library/Application Support/cargo-reclaim`        | `~/Library/Logs/cargo-reclaim`               |
| `task-scheduler` | `%LOCALAPPDATA%\cargo-reclaim`                       | `<state_dir>\logs`                           |

Set `state_dir` and `log_dir` in `[scheduler]` to override both paths.

## Service state and run logs

The service writes these files:

| File                                  | Location                 | Contents                                      |
| ------------------------------------- | ------------------------ | --------------------------------------------- |
| `service-state.json`                  | `<state_dir>`            | Current status, PID, last run, and `last_problem`. |
| `plans/`                              | `<state_dir>/plans`      | Plan snapshots, one per cycle.                |
| `reports/`                            | `<state_dir>/reports`    | Terminal `apply` reports.                     |
| `runs.jsonl`                          | `<log_dir>`              | One JSON record per run event.                |

Only one service runs per state directory. The lock file is `<state_dir>/service.lock`.
A second instance fails with
`cargo-reclaim scheduler service is already running; lock exists at <path>`.

### Status

`scheduler service status --config <path>` prints the state file path, the log directory, and the run log path.
It also prints run totals from `runs.jsonl`:

- Run log record count and corrupt record count.
- Started, apply completed, and failed cycle counts.
- Total applied bytes.
- The last event and the recent runs.

The `status` value has these meanings:

| Status    | Meaning                                                                                              |
| --------- | ---------------------------------------------------------------------------------------------------- |
| `running` | The service wrote this state. The status stays `running` when liveness cannot be confirmed.          |
| `stopped` | The run ended at its `--max-cycles` limit.                                                           |
| `unknown` | No state file exists yet. The service has not written state.                                         |
| `stale`   | The state says `running` but holds no PID.                                                           |

The service never writes `error`. The `problem` line in `status` gives the cause of a `stale` or `unknown` state.

On Linux, the service checks `/proc/<pid>`. A PID with a `/proc` entry is alive.
A PID without an entry gives `unknown` liveness, so the status stays `running`.
On macOS and Windows, liveness is always `unknown`.

## Trigger model

A `[[background.trigger]]` block combines four parts:

- **Trigger.** The `every` cadence. Use as many triggers as needed.
- **Limiter.** The `only_when_disk_free_below`, `min_free_disk`, or `max_target_size` key.
  A trigger with no limiter always cleans. A trigger with a limiter cleans only when the limiter is breached.
  Disk limiters use a free-space check. `max_target_size` scans target sizes.
- **Policy and budget.** The trigger `policy`, plus `target_size_goal` and `target_free_disk`.
- **Disruptiveness.** `interrupt_active_build`, `kill_active_builds`, and `whole_target`.

By default, a trigger reclaims nothing from a target with a running build.

### Unattended cleanup rules

| Rule                                                                                  | Config key                                    |
| ------------------------------------------------------------------------------------- | --------------------------------------------- |
| A `cleanup` run applies only with `--allow-apply` or this key set to `true`.          | `[scheduler].allow_unattended_cleanup`        |
| A `cleanup` run with `balanced`, `aggressive`, or `custom` needs this key set to `true`. | `[scheduler].allow_unattended_high_policy` |
| `whole_target = "delete"` needs the `aggressive` policy and this key set to `true`.   | `[policy].allow_unattended_whole_target_delete` |

## Disruptive triggers

Use these keys only when losing a build is acceptable.

| Setting                      | Effect                                                                                                   |
| ---------------------------- | -------------------------------------------------------------------------------------------------------- |
| `interrupt_active_build`     | Deletes in-use artifacts and whole targets during a build. The build fails when its files vanish.        |
| `kill_active_builds`         | Sends SIGTERM to the `cargo` and `rustc` processes that build targets under `roots`. Waits 5 seconds, then sends SIGKILL to survivors. Cleans after the kill. |
| `whole_target = "delete"`    | Runs `cargo clean` equivalent on each target. Needs `policy = "aggressive"` and the unattended delete key. |

This example runs a safe trim every 30 minutes. It also stops builds and wipes targets at 5% free disk:

```toml
[policy]
allow_unattended_whole_target_delete = true   # required by the emergency trigger

[[background.trigger]]
every = "30m"

[[background.trigger]]
every = "5m"
only_when_disk_free_below = "5%"
policy = "aggressive"
whole_target = "delete"
kill_active_builds = true
```

A trigger without a `policy` key uses `[scheduler].policy`.
The emergency trigger deliberately breaks builds. A broken build costs less than a full disk.

## State retention

Every cycle writes a plan snapshot, and every terminal `apply` writes a report.
Snapshots for large workspaces reach hundreds of megabytes. Retention keeps the state directories bounded.

- Pruning runs at service startup and after every cycle.
- Pruning removes the oldest files first. It never removes the newest plan.
- Logs rotate by rename, so a writer that holds the old file descriptor cannot lose a record.
- A pruning error does not stop the cycle. The error appears in `scheduler service status`.

The budgets and defaults are in [Config file reference](config.md#schedulerretention).
