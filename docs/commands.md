# Command reference

This page lists every command and flag. For a short overview, see the [README](../README.md#commands).
For the scheduler, see [Scheduler](scheduler.md). For the config file, see [Config file reference](config.md).

## Root cleanup

```sh
cargo-reclaim [OPTIONS] [ROOT ...]
cargo-reclaim cleanup [--all|--target <path>] [--delete-target] [--yes] [OPTIONS] [ROOT ...]
```

The root form and the `cleanup` form behave the same way.

| Flag                             | Effect                                                                                   |
| -------------------------------- | ---------------------------------------------------------------------------------------- |
| `--all`                          | Selects every eligible smart-trim candidate. Conflicts with `--target`.                  |
| `--target <path>`                | Selects one target directory. Repeat the flag for more targets.                          |
| `--delete-target`                | Deletes each selected target directory whole. Skips the mode choice.                     |
| `--yes`                          | Deletes after validation. Without it, the run validates only.                            |
| `--dry-run`, `--validate`        | Validates only. Conflicts with `--yes`.                                                  |
| `--json`                         | Prints a JSON document instead of text.                                                  |
| `--policy <kind>`                | Sets the policy. See [Policies](policies.md).                                            |
| `--config <path>`                | Loads defaults from a config file.                                                       |
| `--ignore <path>`                | Reports a path as ignored while scanning.                                                |
| `--skip <path>`                  | Prunes a path and its descendants from scanning.                                         |
| `--allow-name-only-targets`      | Includes weak `target/` matches as confirmation-only.                                    |
| `--follow-symlinks`              | Follows symlinks during scan and inventory.                                              |
| `--cross-filesystems`            | Allows a recursive scan across filesystem boundaries.                                    |
| `--keep-recent-writes <dur>`     | Keeps delete candidates modified within `s`, `m`, `h`, or `d`.                           |
| `--keep-days <days>`             | Day-count alias for `--keep-recent-writes`.                                              |
| `--keep-size <size>`             | Keeps delete candidates at or below this size.                                           |
| `--keep-rustc-hash <u64>`        | Keeps fingerprint grouped intermediates for this rustc hash. Repeatable.                 |
| `--keep-installed-toolchains`    | Keeps fingerprint groups for installed rustup toolchains.                                |
| `--keep-toolchain <name>`        | Keeps fingerprint groups for a named rustup toolchain. Repeatable.                       |

Selection rules:

- With no selector and a terminal, the root form opens the cleanup assistant.
- With no selector and no terminal, the command fails with a usage error. Pass `--all` or `--target`.
- `--yes` alone does not select targets. Use `--all --yes` for bulk cleanup.
- `--target` skips the selection pages. `--all` selects every target. `--delete-target` skips the mode page.

When a run completes, it prints a summary and a `full report: <path>` line. The report file holds every entry.

### Interaction modes

| Mode                         | Example                                                                                 | Result                                                          |
| ---------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| Assistant (default)          | `cargo-reclaim ~/Projects`                                                              | Opens the cleanup assistant.                                    |
| Read-only inventory          | `cargo-reclaim list ~/Projects`                                                         | Lists target directories and sizes.                             |
| Bulk smart trim              | `cargo-reclaim ~/Projects --all --yes`                                                  | Trims all eligible candidates after validation.                 |
| Whole-target delete          | `cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes`   | Deletes one target directory after validation.                  |
| Validate only               | `cargo-reclaim ~/Projects --all --dry-run`                                              | Reports the planned changes. Deletes nothing.                   |

## scan and plan

```sh
cargo-reclaim scan ~/Projects
cargo-reclaim plan ~/Projects/my-crate --policy balanced --whole-target off --keep-recent-writes 4h --save-plan /tmp/reclaim-plan.json
```

`scan` and `plan` build a dry-run plan. They accept `--policy`, `--config`, `--ignore`, `--skip`, `--json`, and the scan and keep flags from the root form. They also accept:

| Flag                        | Effect                                                                                  |
| --------------------------- | --------------------------------------------------------------------------------------- |
| `--whole-target <mode>`     | `off`, `confirm`, or `delete`. `delete` requires the `aggressive` policy.               |
| `--save-plan <path>`        | Writes the plan to a file for `apply`.                                                  |
| `--expires-in <duration>`   | Sets the saved plan expiry. Default `24h`.                                              |

`scan` and `plan` reject `--all`, `--target`, and `--yes`. Run `apply --plan <path> --yes` to delete the entries in a saved plan.

## list

```sh
cargo-reclaim list ~/Projects
cargo-reclaim list ~/Projects --json
```

`list` prints target directories and sizes, largest first. It deletes nothing.
It accepts `--config`, `--ignore`, `--skip`, `--allow-name-only-targets`, `--follow-symlinks`, `--cross-filesystems`, and `--json`.

## apply

```sh
cargo-reclaim apply --plan /tmp/reclaim-plan.json
cargo-reclaim apply --plan /tmp/reclaim-plan.json --yes
```

| Flag              | Effect                                                                    |
| ----------------- | ------------------------------------------------------------------------- |
| `--plan <path>`   | Required. The saved plan to revalidate.                                   |
| `--yes`           | Deletes the entries that pass revalidation. Without it, the run validates only. |
| `--json`          | Prints a JSON report.                                                     |

See [Saved plans](saved-plans.md) for revalidation and expiry.

## edit-plan

```sh
cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --list
cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --interactive
```

Flags: `--plan <path>`, `--list`, `--interactive`, `--select`, `--select-index`, `--select-class`, `--deselect`, `--deselect-index`, `--deselect-class`, and `--json`.
See [Saved plans](saved-plans.md#review-and-edit-a-plan) for each flag.

## cargo-home

The `cargo-home` commands report and clean Cargo home cache data. They use the same saved-plan and revalidation model as target cleanup.

```sh
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-plan.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-plan.json --yes
```

| Subcommand                                      | Effect                                                              |
| ----------------------------------------------- | ------------------------------------------------------------------- |
| `report [--cargo-home <path>] [--json]`         | Reports cache usage and preserved paths. Read-only.                 |
| `plan [--cargo-home <path>] [--policy <kind>] [--save-plan <path>] [--expires-in <duration>] [--json]` | Builds a dry-run plan.   |
| `apply --plan <path> [--yes] [--json]`          | Revalidates a saved plan. Deletes with `--yes`.                     |

`cargo-home` accepts the policies `observe`, `conservative`, `balanced`, `aggressive`, `sweep`, and `custom`.

## cargo-config

`cargo-config` reads and writes Cargo build output config for a project.

| Subcommand                                      | Effect                                                                       |
| ----------------------------------------------- | ---------------------------------------------------------------------------- |
| `recommend [--project <path>] [--json]`         | Reports recommendations. Read-only.                                          |
| `preview [--project <path>] [--json]`           | Prints the planned change. Modifies no files.                                |
| `apply --preview <path> --yes [--json]`         | Writes the previewed change to the project-local Cargo config file.          |

The applied change sets `build.build-dir = "target/build"` in the project-local Cargo config file.

## scheduler

See [Scheduler](scheduler.md) for every subcommand, the platform artifacts, and the service behavior.

## Real usage recipes

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

# Clean Cargo home caches through a persisted, revalidated plan.
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-reclaim.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-reclaim.json --yes
```

## Exit status

`scheduler install` and `scheduler uninstall` exit with a non-zero status when an operation errors.
