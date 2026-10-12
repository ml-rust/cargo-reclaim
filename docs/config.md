# Config file reference

This page lists every key the config loader accepts. Pass a config file with `--config <path>`.
The loader rejects unknown keys. A misspelled key fails at load time.

For the scheduler service and its trigger behavior, see [Scheduler](scheduler.md).
For a complete example file, see [examples/reclaim.toml](../examples/reclaim.toml).

## Rules

- The top level must set `version = 1`. Any other version is an error.
- Relative paths resolve against the directory that holds the config file.
- Sizes accept `B`, `KiB`, `MiB`, `GiB`, and `TiB`, with or without a space before the unit (`5 GiB`, `5GiB`). Decimal units such as `GB` are rejected.
- Durations accept `s`, `m`, `h`, or `d` suffixes (`30m`), or words (`30 minutes`, `2 days`).
- Percentages accept a value above `0` and at most `100`, with at most two decimal places (`12.5%`).

## Top level

| Key      | Type            | Meaning                                                  |
| -------- | --------------- | -------------------------------------------------------- |
| `version` | integer `1`    | Required. Config schema version.                         |
| `roots`  | array of paths  | Directories to scan when the command gives no `ROOT`.    |
| `ignore` | array of paths  | Paths reported as ignored while scanning.                |
| `skip`   | array of paths  | Paths pruned from scanning, with their descendants.      |

## `[policy]`

| Key                                      | Type    | Meaning                                                                                       |
| ---------------------------------------- | ------- | --------------------------------------------------------------------------------------------- |
| `mode`                                   | string  | Policy kind: `observe`, `conservative`, `balanced`, `aggressive`, `sweep`, or `custom`.       |
| `whole_target`                           | string  | `off`, `confirm`, or `delete`. `delete` requires the `aggressive` policy.                     |
| `allow_unattended_whole_target_delete`   | boolean | Must be `true` before a scheduled run can use `whole_target = "delete"`.                      |
| `keep_recent_projects`                   | string  | Duration. Used as the recent-write window when `[planner]` sets none. Lowest priority.        |
| `max_target_size`                        | size    | Target size threshold passed to background trigger setup.                                     |
| `target_size_goal`                       | size    | Size the planner reclaims each target down to.                                                |
| `unattended_allowed`                     | boolean | Parsed and stored. No current behavior reads it.                                              |

## `[scanner]`

| Key                       | Type    | Meaning                                                      |
| ------------------------- | ------- | ------------------------------------------------------------ |
| `follow_symlinks`         | boolean | Follows symlinks during scan and inventory.                  |
| `allow_name_only_targets` | boolean | Includes weak `target/` matches as confirmation-only.        |
| `cross_filesystems`       | boolean | Allows a recursive scan across filesystem boundaries.        |

## `[planner]`

| Key                         | Type             | Meaning                                                                                     |
| --------------------------- | ---------------- | ------------------------------------------------------------------------------------------- |
| `recent_write_keep_window`  | duration         | Keeps delete candidates modified within this window. Deps outputs are deletable only when this is set. |
| `sweep_older_than`          | duration         | Age gate for final binaries under the `sweep` policy. Default `24h`.                        |
| `keep_days`                 | integer          | Day-count alias for `recent_write_keep_window`.                                             |
| `keep_size`                 | size             | Keeps delete candidates at or below this size.                                              |
| `keep_rustc_hashes`         | array of integers | Keeps fingerprint grouped intermediates for these rustc hashes.                             |
| `keep_installed_toolchains` | boolean          | Keeps fingerprint groups for installed rustup toolchains.                                   |
| `keep_toolchains`           | array of strings | Keeps fingerprint groups for these rustup toolchain names.                                  |

## `[scheduler]`

| Key                           | Type    | Meaning                                                                                                  |
| ----------------------------- | ------- | -------------------------------------------------------------------------------------------------------- |
| `name`                        | string  | Scheduler instance name.                                                                                 |
| `at`                          | string  | Daily time in `HH:MM` form for the systemd-user timer, for example `04:15`. Default `03:00`. `--at` overrides it on `preview`, `install`, and `uninstall`. |
| `mode`                        | string  | `observe` or `cleanup`. A `cleanup` run applies the plan.                                               |
| `policy`                      | string  | Policy for scheduled runs. Default `conservative` in `cleanup` mode.                                     |
| `allow_unattended_cleanup`    | boolean | Lets scheduled runs apply without `--allow-apply`.                                                       |
| `allow_unattended_high_policy`| boolean | Required for `cleanup` runs with the `balanced`, `aggressive`, or `custom` policy.                       |
| `state_dir`                   | path    | Directory for service state and plan snapshots.                                                          |
| `log_dir`                     | path    | Directory for `runs.jsonl` and `scheduler.log`.                                                          |

### `[scheduler.retention]`

Retention budgets apply to the service state. Each key has the default shown.

| Key                 | Default   | Meaning                                                                          |
| ------------------- | --------- | -------------------------------------------------------------------------------- |
| `enabled`           | `true`    | Set `false` to keep every state file. Directories then grow without bound.       |
| `keep_plans`        | `32`      | Plan snapshots kept. `0` disables the count budget.                              |
| `max_plan_age`      | `14d`     | Age past which a plan snapshot or apply report is pruned.                        |
| `max_plans_size`    | `1 GiB`   | Total size of the plans directory.                                               |
| `keep_reports`      | `32`      | Terminal `apply` reports kept. `0` disables the count budget.                    |
| `max_log_size`      | `64 MiB`  | Size at which `runs.jsonl` and `scheduler.log` rotate.                           |
| `keep_rotated_logs` | `2`       | Rotated log generations kept (`runs.jsonl.1`, `runs.jsonl.2`). `0` discards on rotation. |

## `[background]`

| Key                 | Type     | Meaning                                                                                  |
| ------------------- | -------- | ---------------------------------------------------------------------------------------- |
| `enabled`           | boolean  | Enables the resident watcher.                                                            |
| `target_free_disk`  | size     | Budget goal: how much a limited run reclaims.                                            |
| `trigger`           | array    | Independent `[[background.trigger]]` blocks. See below.                                  |

### Deprecated flat keys

The flat keys below still load. The loader normalizes them into one trigger and prints a warning.
Migrate to `[[background.trigger]]` blocks.

| Key                          | Meaning                                                    |
| ---------------------------- | ---------------------------------------------------------- |
| `mode`                       | `periodic` or `threshold`.                                 |
| `check_every`                | Cadence, for example `15m`.                                |
| `only_when_disk_free_below`  | Free-space percentage limiter, for example `12.5%`.        |
| `min_free_disk`              | Free-space size limiter, for example `20 GiB`.             |

## `[[background.trigger]]`

Each trigger fires on its own cadence. Use as many triggers as needed.

| Key                        | Type    | Meaning                                                                                              |
| -------------------------- | ------- | ---------------------------------------------------------------------------------------------------- |
| `every`                    | duration | Cadence. A trigger with no limiter fires on this cadence and always cleans.                         |
| `policy`                   | string  | Policy for this trigger. Default is `[scheduler].policy`.                                            |
| `whole_target`             | string  | `off`, `confirm`, or `delete`. `delete` requires `aggressive` and `allow_unattended_whole_target_delete = true`. |
| `interrupt_active_build`   | boolean | Deletes in-use artifacts and whole targets during a build. The build then fails when its files vanish. |
| `kill_active_builds`       | boolean | Stops `cargo` and `rustc` processes under `roots` before cleaning. See [Scheduler](scheduler.md#disruptive-triggers). |
| `max_target_size`          | size    | Limiter. Cleans only when a target exceeds this size. Scans target sizes.                           |
| `only_when_disk_free_below`| percent | Limiter. Cleans only when free disk space falls below this percentage. Uses a free-space check.     |
| `min_free_disk`            | size    | Limiter. Cleans only when free disk space falls below this size.                                    |

Unknown trigger keys fail at load time. The loader does not ignore them.

## Example

```toml
version = 1
roots = ["/home/you/Projects/my-crate"]

[policy]
mode = "balanced"
whole_target = "off"
max_target_size = "100 GiB"
target_size_goal = "80 GiB"

[planner]
recent_write_keep_window = "4h"
sweep_older_than = "24h"

[scheduler]
at = "04:15"
mode = "cleanup"
policy = "balanced"
allow_unattended_cleanup = true
allow_unattended_high_policy = true
state_dir = "/home/you/.local/state/cargo-reclaim/my-crate"
log_dir = "/home/you/.local/state/cargo-reclaim/my-crate/logs"

[background]
enabled = true
target_free_disk = "200 GiB"

[[background.trigger]]
every = "30m"

[[background.trigger]]
every = "5m"
only_when_disk_free_below = "10%"
policy = "sweep"
```

The example runs a balanced trim every 30 minutes. It switches to `sweep` when free disk falls below 10%.
The emergency trigger with `kill_active_builds` and `whole_target = "delete"` is in [Scheduler](scheduler.md#disruptive-triggers).
