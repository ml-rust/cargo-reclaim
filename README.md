# cargo-reclaim

cargo-reclaim is a trust-first Rust artifact cleanup tool for Cargo projects and Cargo home data. It scans for reclaimable build outputs and cache entries, builds dry-run plans, persists plans for later validation, and only removes data after a fresh revalidation pass.

Supported Rust: `cargo-reclaim` targets Rust 1.85+ for edition 2024 support.

## Install

```sh
cargo install cargo-reclaim
cargo install --path .
```

The default mode is conservative: `scan` and `plan` are dry-run only, `apply` validates a saved plan before execution, and `--json` is available for stable machine-readable output. The CLI does not expose a GUI and it does not pretend to modify Cargo state unless a command explicitly says it will.

## Safety Model

- `scan` and `plan` only report what would be reclaimed.
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
```

`scan` and `plan` both build a read-only cleanup plan for one or more roots. `plan` can also persist the plan with `--save-plan`, which is what the later `apply` flow consumes.

`edit-plan --interactive` reads and rewrites an explicit saved plan. It accepts entry numbers, project groups such as `p1`, class groups such as `c:incremental`, and `none` or `cancel`; project groups select only entries that are already delete candidates, and `whole_target` entries must be selected by entry number.

## Validation And Apply Flow

1. Build a dry-run plan with `scan` or `plan`.
2. Persist the plan with `--save-plan <path>` when you want a later apply step.
3. Review or edit the saved plan with `edit-plan` if needed.
4. Run `apply --plan <path>` to validate the saved plan against the current filesystem state.
5. Add `--yes` only when you want the validated delete actions to run.

This revalidation step is the core safety boundary: the tool is designed to refuse stale assumptions rather than delete artifacts from an old snapshot.

## Cargo Home Commands

```sh
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --save-plan cargo-home-plan.json
cargo-reclaim cargo-home apply --plan cargo-home-plan.json
cargo-reclaim cargo-home apply --plan cargo-home-plan.json --yes
```

`cargo-home report` summarizes Cargo home caches and preserved paths. `cargo-home plan` builds a dry-run cleanup plan for the Cargo home tree, and `cargo-home apply` validates or executes only a saved Cargo home plan; `apply` does not accept a live `--cargo-home` path.

## Scheduler Commands

```sh
cargo-reclaim scheduler preview --platform systemd-user --config reclaim.toml
cargo-reclaim scheduler install --platform launchd --config reclaim.toml --dry-run
cargo-reclaim scheduler uninstall --platform task-scheduler --config reclaim.toml --dry-run
cargo-reclaim scheduler service run --config reclaim.toml
cargo-reclaim scheduler service status --config reclaim.toml
```

`scheduler preview` emits the platform-specific installation artifacts without writing them, including a systemd user service plus timer on Linux. `scheduler install` and `scheduler uninstall` can stay in dry-run mode or execute through the selected backend. Installed artifacts supervise `scheduler service run`, which keeps a resident background loop alive, records durable service state, and writes JSONL run logs. By default installs use the generic `cargo-reclaim` scheduler service; set `[scheduler] name = "workstation"` only when you intentionally want a separate named scheduler instance. `scheduler service run` and `scheduler service status` are config-driven; `status` reads persisted service state and can report `unknown` before the service has written state. `scheduler run` remains available as a single-cycle background execution entrypoint for diagnostics and compatibility.

## Platform Notes

- Linux uses `procfs` for active-process detection, so it can observe running `cargo` and `rustc` processes when `/proc` is readable; on non-Linux platforms active-process detection is not attempted and the planner proceeds without live process observation.
- `scheduler preview`, `install`, and `uninstall` support backend-specific artifacts for `systemd-user` on Linux, `launchd` on macOS, and `task-scheduler` on Windows.
- The scheduler service is a resident loop started by installed service artifacts; it persists service state and run logs, while `scheduler service status` reports the last recorded state, may return `unknown` until the service has written state, and reports `stale` when a saved running PID is definitely dead.
- Cargo config resolution treats `build-dir = "{workspace-root}/{workspace-path-hash}"` as unsupported, so that template is reported instead of being used as a write target.

## Cargo Config Commands

```sh
cargo-reclaim cargo-config recommend --project path/to/project
cargo-reclaim cargo-config preview --project path/to/project --json
cargo-reclaim cargo-config apply --preview path/to/preview.json --yes
```

`cargo-config recommend` reports read-only Cargo build output configuration guidance. `cargo-config preview` builds a dry-run write plan for Cargo config files and does not modify files. `cargo-config apply` applies a saved preview only with `--preview <path> --yes`; it does not accept `--project`.

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
