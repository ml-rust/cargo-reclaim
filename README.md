# cargo-reclaim

cargo-reclaim is a trust-first Rust artifact cleanup tool for Cargo projects and Cargo home data. It scans for reclaimable build outputs and cache entries, builds dry-run plans, persists plans for later validation, and only removes data after a fresh revalidation pass.

Supported Rust: `cargo-reclaim` targets Rust 1.85+ for edition 2024 support.

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
```

`scan` and `plan` both build a read-only cleanup plan for one or more roots. `plan` can also persist the plan with `--save-plan`, which is what the later `apply` flow consumes.

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

`cargo-home report` summarizes Cargo home caches and preserved paths. `cargo-home plan` builds a dry-run cleanup plan for the Cargo home tree, and `cargo-home apply` validates or executes a saved Cargo home plan.

## Scheduler Commands

```sh
cargo-reclaim scheduler preview --platform systemd-user --config reclaim.toml
cargo-reclaim scheduler install --platform launchd --config reclaim.toml --dry-run
cargo-reclaim scheduler uninstall --platform task-scheduler --config reclaim.toml --dry-run
cargo-reclaim scheduler run --config reclaim.toml --run-id test --log-path runs.jsonl --plan-path plan.json
```

`scheduler preview` emits the platform-specific installation artifacts without writing them. `scheduler install` and `scheduler uninstall` can stay in dry-run mode or execute through the selected backend, while `scheduler run` is the background cycle entrypoint used by scheduled jobs.

## Cargo Config Commands

```sh
cargo-reclaim cargo-config recommend --project path/to/project
cargo-reclaim cargo-config preview --project path/to/project --json
cargo-reclaim cargo-config apply --preview path/to/preview.json --yes
```

`cargo-config recommend` reports read-only Cargo build output configuration guidance. `cargo-config preview` builds a write plan for Cargo config files, and `cargo-config apply` applies a saved preview after explicit confirmation.

## Common Options

- `--config <path>` loads defaults from a TOML config file.
- `examples/reclaim.toml` is a tracked starter config that stays within the currently supported keys.
- `--policy <kind>` selects `observe`, `conservative`, `balanced`, `aggressive`, or `custom`.
- `--ignore <path>` skips paths during scanning.
- `--follow-symlinks`, `--allow-name-only-targets`, and `--cross-filesystems` adjust scan coverage.
- `--keep-recent-writes <dur>` preserves delete candidates that were modified recently.
- `--json` emits a structured document instead of terminal text.
