# cargo-reclaim

cargo-reclaim keeps Rust workstations from filling up with Cargo build artifacts by finding target directories, trimming stale deps and incremental caches, and running safe scheduled cleanup without wiping active builds. It is a smarter, background-friendly companion to `cargo clean` for large Rust projects and long-running development machines.

## Why Not `cargo clean`?

Use `cargo clean` when you are inside one project and want to delete that project’s build output now. `cargo-reclaim` is for Rust development machines where disk usage builds up across many projects, shared target directories, incremental artifacts, Cargo home caches, and long-running active work.

`cargo-reclaim` adds the missing operating layer around cleanup:

- It finds Cargo target directories across project trees and reports their measured size largest-first.
- It lets you clean selected target directories interactively instead of manually finding and typing paths.
- It can trim partial artifacts such as incremental, build-script, fingerprint, temporary, stale hashed `deps` variants, old hashed `deps` outputs, and stale incremental session or unit variants without always deleting a whole `target` directory.
- It protects delayed or automated cleanup with dry-run plans, persisted plans, and fresh revalidation before deletion.
- It understands Cargo config target dirs, shared target dirs, ignore/skip rules, policy modes, recent-write preservation, and active process checks.
- It can run as a resident scheduler service, so projects stay below a size ceiling without a manual cleanup habit.
- It emits JSON for scripts, dashboards, TUI frontends, and other automation.
- It covers Cargo home cache cleanup through the same review/apply safety model.

In short: `cargo clean` is a manual per-project reset; `cargo-reclaim` is discovery, selection, partial trimming, background cleanup, and safety checks for a whole Rust workstation.

## Install

```sh
cargo install cargo-reclaim
cargo install --path .
```

The default policy is `balanced`, but the default workflow is dry-run-first: `scan` and `plan` only report, `apply` validates a saved plan before execution, and `--yes` is required before deletion. `--json` is available for stable machine-readable output. The CLI does not expose a GUI and it does not pretend to modify Cargo state unless a command explicitly says it will.

## Safety Model

- `scan` and `plan` only report what would be reclaimed.
- `targets` lists Cargo target directories without deleting anything.
- `targets clean` validates selected target directories and only deletes when `--yes` is present.
- `apply` requires an explicit plan path and supports validation-only mode unless `--yes` is set.
- Persisted plans are revalidated at apply time, so stale or changed entries can be skipped instead of blindly removed.
- `cargo-home report` and `cargo-config recommend` are read-only.
- `cargo-home plan`, `scheduler preview`, and `scheduler install` or `scheduler uninstall` with `--dry-run` stay in preview mode.
- `cargo-config apply` requires a preview file and `--yes` before it can write.

## Main Commands

```sh
cargo-reclaim plan .
cargo-reclaim scan --policy observe --json .
cargo-reclaim plan . --save-plan reclaim-plan.json --expires-in 7d
cargo-reclaim apply --plan reclaim-plan.json
cargo-reclaim apply --plan reclaim-plan.json --yes
cargo-reclaim edit-plan --plan reclaim-plan.json --list
cargo-reclaim edit-plan --plan reclaim-plan.json --select target/doc
cargo-reclaim edit-plan --plan reclaim-plan.json --interactive
cargo-reclaim targets .
cargo-reclaim targets clean --interactive .
cargo-reclaim targets clean --interactive --yes .
```

`scan` and `plan` both build a read-only cleanup plan for one or more roots. `plan` can also persist the plan with `--save-plan`, which is what the later `apply` flow consumes.

`edit-plan --interactive` reads and rewrites an explicit saved plan. It accepts entry numbers, project groups such as `p1`, class groups such as `c:incremental`, and `none` or `cancel`; project groups select only entries that are already delete candidates, and `whole_target` entries must be selected by entry number.

## Real Usage Recipes

```sh
# Find the largest Cargo target directories under a project tree.
cargo-reclaim targets ~/Projects

# Produce machine-readable target inventory for another tool.
cargo-reclaim targets ~/Projects --json

# Review and validate selected target deletion without deleting anything.
cargo-reclaim targets clean --interactive ~/Projects

# Delete selected target directories after the same validation pass.
cargo-reclaim targets clean --interactive --yes ~/Projects

# Delete one known target directory without typing it into a saved plan.
cargo-reclaim targets clean --target ~/Projects/old-crate/target --yes

# Trim stale incremental and deps artifacts from an active project without deleting the whole target.
cargo-reclaim plan ~/Projects/my-crate --policy balanced --whole-target off --keep-recent-writes 4h --save-plan /tmp/my-crate-reclaim.json
cargo-reclaim apply --plan /tmp/my-crate-reclaim.json
cargo-reclaim apply --plan /tmp/my-crate-reclaim.json --yes

# Run one service cycle from a config file for diagnostics.
cargo-reclaim scheduler service run --config ~/.config/cargo-reclaim/reclaim.toml --max-cycles 1 --json

# Check whether the resident scheduler service is alive and what it last did.
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml --json

# Preview platform service artifacts before installing them.
cargo-reclaim scheduler preview --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler preview --platform launchd --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler preview --platform task-scheduler --config ~/.config/cargo-reclaim/reclaim.toml

# Install a resident background scheduler on the current platform.
cargo-reclaim scheduler install --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml

# Clean Cargo home caches through a persisted, revalidated plan.
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-reclaim.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-reclaim.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-reclaim.json --yes
```

## Validation And Apply Flow

1. Build a dry-run plan with `scan` or `plan`.
2. Persist the plan with `--save-plan <path>` when you want a later apply step.
3. Review or edit the saved plan with `edit-plan` if needed.
4. Run `apply --plan <path>` to validate the saved plan against the current filesystem state.
5. Add `--yes` only when you want the validated delete actions to run.

This revalidation step is the core safety boundary: the tool is designed to refuse stale assumptions rather than delete artifacts from an old snapshot.

Balanced partial cleanup includes stale hashed files under `target/*/deps` when they can be distinguished from the newest hash variant for the same artifact family. If `.fingerprint` metadata still exists, stale deps matching uses it as an anchor and respects kept rustc/toolchain hashes; if the fingerprint directory has already been removed, duplicate hashed deps files can still be treated as orphaned stale variants. The `deps` directory itself remains preserved.

Balanced partial cleanup can also trim direct hashed files under `target/*/deps`, including test binaries, `.rlib`, `.rmeta`, dep-info, and object-style outputs, but only when a recent-write keep window is configured. Without `--keep-recent-writes` or an equivalent scheduler config value, these entries are reported as preserved `deps_output` instead of being deleted automatically.

Balanced partial cleanup also includes stale rustc incremental cache entries under `target/*/incremental`. It keeps the newest session per compile unit, keeps the newest unit variant per compile-unit family, requires rustc incremental marker files before treating a session as stale, and preserves the `incremental` parent directory itself when stale child entries are planned.

## Cargo Home Commands

```sh
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --save-plan cargo-home-plan.json
cargo-reclaim cargo-home apply --plan cargo-home-plan.json
cargo-reclaim cargo-home apply --plan cargo-home-plan.json --yes
```

`cargo-home report` summarizes Cargo home caches and preserved paths. `cargo-home plan` builds a dry-run cleanup plan for the Cargo home tree, and `cargo-home apply` validates or executes only a saved Cargo home plan; `apply` does not accept a live `--cargo-home` path.

## Target Directory Commands

```sh
cargo-reclaim targets .
cargo-reclaim targets list ~/Projects --json
cargo-reclaim target ~/Projects
cargo-reclaim targets clean --interactive ~/Projects
cargo-reclaim targets clean --interactive --yes ~/Projects
cargo-reclaim targets clean --target ~/Projects/my-crate/target --yes
cargo-reclaim targets clean --target ~/Projects/a/target --target ~/Projects/b/target --yes
```

`targets` discovers Cargo target directories from project context, configured Cargo target directories, and target-root evidence, then reports their measured size largest-first. `target` is an alias for `targets`, and `targets list` is the explicit form of the default list command.

`targets clean` is for whole target directory cleanup when that is the intended operation. Without `--yes`, it validates and reports what would be deleted. With `--interactive`, it prints numbered target choices and accepts numbers such as `1`, `1,3`, or `1 3`; with `--target`, it cleans explicit paths. Selected cleanup still goes through persisted-plan validation before deletion.

## Scheduler Commands

```sh
cargo-reclaim scheduler preview --platform systemd-user --config reclaim.toml
cargo-reclaim scheduler install --platform launchd --config reclaim.toml --dry-run
cargo-reclaim scheduler uninstall --platform task-scheduler --config reclaim.toml --dry-run
cargo-reclaim scheduler service run --config reclaim.toml
cargo-reclaim scheduler service status --config reclaim.toml
```

`scheduler preview` emits the platform-specific installation artifacts without writing them, including a systemd user service plus timer on Linux. `scheduler install` and `scheduler uninstall` can stay in dry-run mode or execute through the selected backend. Installed artifacts supervise `scheduler service run`, which keeps a resident background loop alive, records durable service state, and writes JSONL run logs. By default installs use the generic `cargo-reclaim` scheduler service; set `[scheduler] name = "workstation"` only when you intentionally want a separate named scheduler instance. `scheduler service run` and `scheduler service status` are config-driven; `status` reads persisted service state and can report `unknown` before the service has written state. `scheduler run` remains available as a single-cycle background execution entrypoint for diagnostics and compatibility.

Threshold background mode supports both project and global disk pressure controls. `[policy] max_target_size` is the per-target high-water trigger and `target_size_goal` is the lower trim goal for budgeted selection. `[background] only_when_disk_free_below` keeps the existing percentage trigger; `min_free_disk` adds an absolute free-space trigger and `target_free_disk` sets the global free-space goal used to budget a cleanup run.

Recommended active-project scheduler shape:

```toml
version = 1
roots = ["/home/you/Projects/my-crate"]

[policy]
mode = "balanced"
whole_target = "off"
allow_unattended_whole_target_delete = false
max_target_size = "100 GiB"
target_size_goal = "80 GiB"

[planner]
recent_write_keep_window = "4h"

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
mode = "threshold"
check_every = "30m"
min_free_disk = "150 GiB"
target_free_disk = "200 GiB"
```

## Platform Notes

- Linux uses `procfs` for active-process detection, so it can observe running `cargo` and `rustc` processes when `/proc` is readable; on non-Linux platforms active-process detection is not attempted and the planner proceeds without live process observation.
- `scheduler preview`, `install`, and `uninstall` support backend-specific artifacts for `systemd-user` on Linux, `launchd` on macOS, and `task-scheduler` on Windows.
- The scheduler service is a resident loop started by installed service artifacts; it persists service state and run logs, while `scheduler service status` reports the last recorded state, may return `unknown` until the service has written state, leaves `running` unchanged when PID liveness cannot be inspected from the current environment, and reports `stale` when a saved running PID is definitely dead.
- Cargo config resolution treats `build-dir = "{workspace-root}/{workspace-path-hash}"` as unsupported, so that template is reported instead of being used as a write target.

## Cargo Config Commands

```sh
cargo-reclaim cargo-config recommend --project path/to/project
cargo-reclaim cargo-config preview --project path/to/project --json
cargo-reclaim cargo-config apply --preview path/to/preview.json --yes
```

`cargo-config recommend` reports read-only Cargo build output configuration guidance. `cargo-config preview` builds a dry-run write plan for Cargo config files and does not modify files. `cargo-config apply` applies a saved preview only with `--preview <path> --yes`; it does not accept `--project`.

## Policy Modes

| Policy         | Default? | Deletes automatically in a plan?                                                                                                                                                                                                                                                                                                  | Typical use                                                                       |
| -------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `observe`      | No       | Nothing. All cleanup-capable entries are preserved for reporting.                                                                                                                                                                                                                                                                 | Inventory, dashboards, CI/reporting, or first run on an unknown machine.          |
| `conservative` | No       | Narrow low-risk classes: `incremental` and temporary artifacts.                                                                                                                                                                                                                                                                   | Active projects where you want minimal rebuild impact.                            |
| `balanced`     | Yes      | Default removable classes: `incremental`, build-script caches, fingerprints, temporary artifacts, stale fingerprint-group intermediates, stale hashed `deps` variants, old hashed `deps` outputs when a recent-write keep window is configured, stale incremental sessions or unit variants, dep-info files, and object metadata. | Normal workstation cleanup and scheduled partial trimming.                        |
| `aggressive`   | No       | Same default removable classes as `balanced`; whole-target deletion is still separate and requires `--whole-target delete` or confirmed selected target cleanup.                                                                                                                                                                  | One-off deep cleanup when rebuild cost is acceptable.                             |
| `custom`       | No       | Currently follows the default removable class set used by `balanced`.                                                                                                                                                                                                                                                             | Config-driven future policy tuning while preserving the same safety checks today. |

Protected by default in every non-whole-target policy: whole target directories, docs, packages, timings, final executables, final libraries, final `.rlib` files, final `.wasm` files, and unknown artifacts. Weak name-only target evidence requires confirmation instead of automatic deletion.

## Common Options

- `--config <path>` loads defaults from a TOML config file.
- `examples/reclaim.toml` is a tracked starter config that stays within the currently supported keys.
- `--policy <kind>` selects `observe`, `conservative`, `balanced`, `aggressive`, or `custom`.
- `--whole-target <mode>` selects `off`, `confirm`, or `delete`; direct delete requires aggressive policy, and config-driven unattended whole-target deletion also requires `allow_unattended_whole_target_delete = true`.
- `--ignore <path>` reports a path as ignored during scanning.
- `--skip <path>` prunes a path and its descendants from scanning without reporting entries below it.
- `--follow-symlinks`, `--allow-name-only-targets`, and `--cross-filesystems` adjust scan coverage.
- `--keep-recent-writes <dur>` preserves delete candidates that were modified recently.
- `--keep-days <days>` is a day-count alias for recent-write preservation.
- `--keep-size <size>` preserves delete candidates at or below the given size.
- `--keep-rustc-hash <u64>` preserves fingerprint grouped intermediates whose Cargo fingerprint JSON records that `rustc` hash.
- `--keep-installed-toolchains` and repeatable `--keep-toolchain <name>` resolve rustup toolchains into `rustc` hashes before applying the same fingerprint group preservation path.
- `--json` emits a structured document instead of terminal text.
