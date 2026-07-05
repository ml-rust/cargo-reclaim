<div align="center">

# cargo-reclaim

<h3>Safe Cargo cleanup for real Rust workstations.</h3>

<p>
cargo-reclaim finds large Cargo build directories, trims stale deps and incremental caches, and keeps active projects under control with a default cleanup assistant, saved plans, and a resident scheduler.
</p>

<p>
  <a href="#quickstart"><strong>Quickstart</strong></a>
  <a href="#main-commands"><strong>Commands</strong></a>
  <a href="#real-usage-recipes"><strong>Recipes</strong></a>
  <a href="CONTRIBUTING.md"><strong>Contributing</strong></a>
</p>

<p>
  <a href="https://discord.gg/jBhFk9kHPg">
    <img src="https://img.shields.io/discord/1453357769720594543?label=Discord&logo=discord&logoColor=white&color=5865F2" alt="Join the Discord">
  </a>
</p>

<p>
  <a href="https://crates.io/crates/cargo-reclaim">
    <img src="https://img.shields.io/crates/v/cargo-reclaim" alt="crates.io">
  </a>
  <a href="https://docs.rs/cargo-reclaim">
    <img src="https://img.shields.io/docsrs/cargo-reclaim" alt="docs.rs">
  </a>
  <a href="LICENSE-MIT">
    <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0">
  </a>
  <a href="https://github.com/ml-rust/cargo-reclaim/stargazers">
    <img src="https://img.shields.io/github/stars/ml-rust/cargo-reclaim?style=social" alt="GitHub stars">
  </a>
</p>

</div>

cargo-reclaim is a safer, background-friendly companion to `cargo clean`. It opens a TTY cleanup assistant by default, trims stale artifacts instead of wiping whole targets, inventories Cargo target directories before deletion, validates saved plans before apply, and can run as a resident scheduler for active Rust workstations.

## Quickstart

Install from crates.io:

```sh
cargo install cargo-reclaim
```

From a checkout:

```sh
cargo install --path .
```

Primary cleanup assistant:

```sh
cargo-reclaim ~/Projects
cargo-reclaim list ~/Projects
cargo-reclaim ~/Projects --all --yes
cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes
cargo-reclaim scheduler preview --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler install --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml --json
```

Foreground `cleanup` and `apply` terminal runs print a concise summary plus `full report: <path>` for the complete JSON entry list. `scheduler service status` prints the resolved state file, log directory, run log path, lifetime totals, and recent run summaries. The resident scheduler writes JSONL run records to `runs.jsonl` under the resolved `log_dir`.

Supported Rust: `cargo-reclaim` targets Rust 1.88+.

## Why cargo-reclaim

- **Find the real disk hogs.** Discover Cargo target directories from project roots, Cargo config target dirs, and target-root evidence, then list them largest-first with measured sizes before cleanup starts.
- **Trim instead of wipe.** Reclaim stale hashed `deps` variants, old deps outputs, stale incremental sessions, fingerprints, build-script caches, temporary files, and other partial artifacts without always deleting the whole `target` directory.
- **Protect active work.** Use the TTY cleanup assistant, recent-write windows, preserved classes, policy modes, saved plans, and fresh apply-time revalidation before deletion.
- **Clean interactively or automatically.** Open the assistant for guided cleanup, run one-shot plans, or install a resident scheduler service that keeps projects below size ceilings.
- **Automate safely.** Emit JSON for scripts, dashboards, TUI frontends, and other tools; saved plans make review and execution separate steps.
- **Cover Cargo home too.** Report and clean Cargo home cache data through the same persisted-plan safety model.

## Why not `cargo clean`?

Use `cargo clean` when you are inside one project and want to delete that project's whole build output now. That is simple and correct for a full reset, but it also throws away useful artifacts that make the next build fast. For active projects, cargo-reclaim is designed to preserve the hot build path while trimming stale bulk around it.

`cargo clean` is a manual per-project reset. `cargo-reclaim` is an operating layer around Cargo cleanup: it finds targets, explains what can be reclaimed, preserves active builds and recent outputs, validates saved plans, and can keep cleanup running in the background.

## Interaction Modes

| Mode                         | Example                                                                                                                 | Skips                                                      | Result                                                                         |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------ |
| TTY assistant default        | `cargo-reclaim ~/Projects`                                                                                              | Nothing; this is the primary guided flow                   | Opens the cleanup assistant for smart trim or whole-target deletion decisions. |
| Read-only inventory          | `cargo-reclaim list ~/Projects`                                                                                         | Cleanup entirely                                           | Lists Cargo target directories and sizes.                                      |
| Bulk smart trim              | `cargo-reclaim ~/Projects --all --yes`                                                                                  | The assistant and per-target selection                     | Selects all eligible smart-trim candidates and executes them after validation. |
| Explicit whole-target delete | `cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes`                                   | Assistant mode choice and selection pages                  | Deletes the selected target directory directly after validation.               |
| Validate only                | `cargo-reclaim ~/Projects --all --dry-run` or `cargo-reclaim ~/Projects --target ~/Projects/my-crate/target --validate` | Deletion and assistant pages skipped by the selector flags | Revalidates and reports what would change without touching files.              |

`--target` skips selection, `--all` selects all, `--delete-target` skips mode choice, `--yes` executes, and `--dry-run` or `--validate` validate only. `--yes` alone does not imply `--all`; bulk cleanup needs explicit `--all --yes`.

## Key Capabilities

### Target Discovery

- Lists Cargo target directories under one or more roots.
- Understands configured Cargo target dirs and shared target dirs.
- Reports measured size largest-first.
- Supports explicit whole-target cleanup through the assistant path when that is the chosen operation.

### Partial Artifact Cleanup

- Trims stale rustc incremental cache sessions and older compile-unit variants.
- Trims stale hashed files under `target/*/deps` when older hash variants can be distinguished from the newest family member.
- Trims direct old deps outputs when a recent-write keep window is configured.
- Preserves final binaries, final libraries, docs, packages, timings, unknown artifacts, and whole target directories unless explicit whole-target cleanup is requested.

### Safety And Revalidation

- `scan`, `plan`, `list`, `cargo-home report`, and config recommendation commands are read-only.
- `apply` consumes a saved plan and revalidates filesystem state before deletion.
- Entries that changed, disappeared, became symlinks, or no longer match the saved snapshot are skipped instead of blindly removed.
- Destructive commands require `--yes`.

### Scheduler

- Installs platform service artifacts for Linux `systemd --user`, macOS `launchd`, and Windows Task Scheduler.
- Runs a resident background loop from a TOML config.
- Supports per-target high-water limits and global free-space thresholds.
- Writes durable service state and JSONL run logs for diagnostics.

### Cargo Home

- Reports Cargo home cache usage.
- Builds saved cleanup plans for Cargo home data.
- Applies only saved Cargo home plans, with the same validation boundary as target cleanup.

## Main Commands

```sh
cargo-reclaim ~/Projects
cargo-reclaim list ~/Projects
cargo-reclaim list ~/Projects --json
cargo-reclaim ~/Projects --all --yes
cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes

cargo-reclaim plan ~/Projects/my-crate --policy balanced --whole-target off --keep-recent-writes 4h --save-plan /tmp/reclaim-plan.json
cargo-reclaim apply --plan /tmp/reclaim-plan.json
cargo-reclaim apply --plan /tmp/reclaim-plan.json --yes

cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --list
cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --interactive

cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-plan.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-plan.json --yes

cargo-reclaim scheduler preview --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler install --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml --json
cargo-reclaim scheduler service run --config ~/.config/cargo-reclaim/reclaim.toml --max-cycles 1 --json
```

`scan` and `plan` both build a read-only cleanup plan for one or more roots. `plan` can also persist the plan with `--save-plan`, which is what `apply` consumes later. `list` is the read-only target inventory surface, while the root form opens the interactive cleanup assistant in a TTY.

## Real Usage Recipes

```sh
# Open the cleanup assistant for a project tree.
cargo-reclaim ~/Projects

# Produce machine-readable target inventory for another tool.
cargo-reclaim list ~/Projects --json

# Delete all eligible smart-trim candidates after validation.
cargo-reclaim ~/Projects --all --yes

# Delete one known target directory without hand-editing a saved plan.
cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes

# Trim stale deps and incremental artifacts from an active project.
cargo-reclaim plan ~/Projects/my-crate --policy balanced --whole-target off --keep-recent-writes 4h --save-plan /tmp/my-crate-reclaim.json
cargo-reclaim apply --plan /tmp/my-crate-reclaim.json --yes

# Run one scheduler service cycle for diagnostics.
cargo-reclaim scheduler service run --config ~/.config/cargo-reclaim/reclaim.toml --max-cycles 1 --json

# Check whether the resident scheduler service is alive and what it last did.
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml --json

# Inspect the raw scheduler run log printed by service status.
tail -n 20 ~/.local/state/cargo-reclaim/logs/runs.jsonl

# Inspect a foreground cleanup/apply report from the `full report:` path printed after the run.
jq '.totals, .entries[] | select(.status != "not_planned_for_deletion")' ~/.local/state/cargo-reclaim/reports/*.json

# Preview platform service artifacts before installing them.
cargo-reclaim scheduler preview --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler preview --platform launchd --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler preview --platform task-scheduler --config ~/.config/cargo-reclaim/reclaim.toml

# Check resident scheduler status after installation.
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml --json

# Clean Cargo home caches through a persisted, revalidated plan.
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-reclaim.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-reclaim.json --yes
```

## Validation And Apply Flow

1. Build a dry-run plan with `scan`, `plan`, `--dry-run`, or `--validate`.
2. Persist the plan with `--save-plan <path>` when you want a later apply step.
3. Review or edit the saved plan with `edit-plan` if needed.
4. Run `apply --plan <path>` to validate the saved plan against the current filesystem state.
5. Add `--yes` only when you want the validated delete actions to run.

This revalidation step is the core safety boundary: cargo-reclaim refuses stale assumptions instead of deleting artifacts from an old snapshot.

## Policy Modes

| Policy         | Default? | Deletes automatically in a plan?                                                                                                                                                                                                                                                                                            | Typical use                                                                |
| -------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `observe`      | No       | Nothing. Cleanup-capable entries are preserved for reporting.                                                                                                                                                                                                                                                               | Inventory, dashboards, CI/reporting, or first run on an unknown machine.   |
| `conservative` | No       | Narrow low-risk classes such as incremental and temporary artifacts.                                                                                                                                                                                                                                                        | Active projects where rebuild impact must stay minimal.                    |
| `balanced`     | Yes      | Default removable classes: incremental, build-script caches, fingerprints, temporary artifacts, stale fingerprint-group intermediates, stale hashed deps variants, old hashed deps outputs when a recent-write keep window is configured, stale incremental sessions or unit variants, dep-info files, and object metadata. | Normal workstation cleanup and scheduled partial trimming.                 |
| `aggressive`   | No       | Same default removable classes as `balanced`; whole-target deletion is still separate and requires explicit whole-target cleanup.                                                                                                                                                                                           | One-off deep cleanup when rebuild cost is acceptable.                      |
| `custom`       | No       | Currently follows the default removable class set used by `balanced`.                                                                                                                                                                                                                                                       | Config-driven future policy tuning while preserving current safety checks. |

Protected by default in every non-whole-target policy: whole target directories, docs, packages, timings, final executables, final libraries, final `.rlib` files, final `.wasm` files, and unknown artifacts. Weak name-only target evidence requires confirmation instead of automatic deletion.

## Scheduler Configuration

Example active-project scheduler shape:

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

`max_target_size` is the per-target high-water trigger. `target_size_goal` is the lower trim goal for budgeted project cleanup. `min_free_disk` is an absolute global free-space trigger, and `target_free_disk` is the global free-space goal used to budget a cleanup run.

## Platform Notes

- Linux uses `procfs` for active-process detection when `/proc` is readable; macOS and Windows use a native process-table provider through `sysinfo`.
- Scheduler preview, install, and uninstall support `systemd-user` on Linux, `launchd` on macOS, and `task-scheduler` on Windows.
- The scheduler service persists state and run logs. `scheduler service status` prints the resolved `service-state.json` and `runs.jsonl` paths, may return `unknown` until the service has written state, keeps `running` when PID liveness cannot be inspected, and reports `stale` when a saved running PID is definitely dead.
- Cargo config resolution treats `build-dir = "{workspace-root}/{workspace-path-hash}"` as unsupported, so that template is reported instead of being used as a write target.

## Common Options

- `--config <path>` loads defaults from a TOML config file.
- `--policy <kind>` selects `observe`, `conservative`, `balanced`, `aggressive`, or `custom`.
- `--whole-target <mode>` selects `off`, `confirm`, or `delete`.
- `--ignore <path>` reports a path as ignored during scanning.
- `--skip <path>` prunes a path and its descendants from scanning without reporting entries below it.
- `--follow-symlinks`, `--allow-name-only-targets`, and `--cross-filesystems` adjust scan coverage.
- `--keep-recent-writes <dur>` preserves delete candidates that were modified recently.
- `--keep-days <days>` is a day-count alias for recent-write preservation.
- `--keep-size <size>` preserves delete candidates at or below the given size.
- `--keep-rustc-hash <u64>`, `--keep-installed-toolchains`, and repeatable `--keep-toolchain <name>` preserve fingerprint grouped intermediates tied to specific rustc hashes.
- `--json` emits a structured document instead of terminal text.
