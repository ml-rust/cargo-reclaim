<div align="center">

# cargo-reclaim

<h3>Trims stale Cargo build artifacts with saved, revalidated plans.</h3>

<p>
cargo-reclaim finds Cargo target directories and trims their stale artifacts.
On the command line, a run validates only until you pass <code>--yes</code>.
</p>

<p>
  <a href="https://docs.rs/cargo-reclaim"><strong>Docs</strong></a>
  ·
  <a href="https://crates.io/crates/cargo-reclaim"><strong>Crate</strong></a>
  ·
  <a href="#commands"><strong>Commands</strong></a>
  ·
  <a href="#scheduler"><strong>Scheduler</strong></a>
  ·
  <a href="#quick-start"><strong>Example</strong></a>
  ·
  <a href="CONTRIBUTING.md"><strong>Contributing</strong></a>
</p>

<p>
  <a href="https://discord.gg/jBhFk9kHPg">
    <img src="https://img.shields.io/discord/1453357769720594543?label=Discord&logo=discord&logoColor=white&color=5865F2" alt="Join the Discord">
  </a>
</p>

<p>
  <a href="https://github.com/ml-rust/cargo-reclaim/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/ml-rust/cargo-reclaim/ci.yml?branch=main&label=ci" alt="CI status">
  </a>
  <a href="https://crates.io/crates/cargo-reclaim">
    <img src="https://img.shields.io/crates/v/cargo-reclaim" alt="crates.io">
  </a>
  <a href="https://docs.rs/cargo-reclaim">
    <img src="https://img.shields.io/docsrs/cargo-reclaim" alt="docs.rs">
  </a>
  <a href="#license">
    <img src="https://img.shields.io/crates/l/cargo-reclaim" alt="License">
  </a>
  <a href="https://github.com/ml-rust/cargo-reclaim/stargazers">
    <img src="https://img.shields.io/github/stars/ml-rust/cargo-reclaim?style=social" alt="GitHub stars">
  </a>
</p>

</div>

cargo-reclaim trims stale artifacts in Cargo `target` directories and Cargo home caches. It revalidates a saved plan before it deletes anything.
It can also run as a resident scheduler with size and disk-space triggers.

## Quick Start

cargo-reclaim requires Rust 1.88 or later.

Install from crates.io:

```sh
cargo install cargo-reclaim
```

Install from a checkout:

```sh
cargo install --path .
```

List the Cargo target directories under a project tree. This command reads only:

```sh
cargo-reclaim list ~/Projects
```

Open the cleanup assistant. This command needs a terminal:

```sh
cargo-reclaim ~/Projects
```

Trim every eligible target. This command deletes after validation:

```sh
cargo-reclaim ~/Projects --all --yes
```

Delete one target directory whole:

```sh
cargo-reclaim ~/Projects --target ~/Projects/old-crate/target --delete-target --yes
```

Foreground cleanup and apply runs print a summary and a `full report:` line. The file on that line holds every entry.

## Commands

| Command                      | Purpose                                                                                  | Deletes |
| ---------------------------- | ---------------------------------------------------------------------------------------- | ------- |
| `cargo-reclaim ROOT ...`     | Opens the cleanup assistant. Pass `--all` or `--target` to skip it.                       | With `--yes` |
| `cargo-reclaim list ROOT ...`| Lists target directories and sizes, largest first.                                       | No      |
| `scan` and `plan`            | Builds a dry-run plan. `plan --save-plan <path>` saves it.                               | No      |
| `apply --plan <path>`        | Revalidates a saved plan. Deletes with `--yes`.                                         | With `--yes` |
| `edit-plan --plan <path>`    | Lists or selects entries in a saved plan.                                                | No      |
| `cargo-home`                 | Reports, plans, and applies cleanup of Cargo home caches.                                | With `--yes` on `apply` |
| `cargo-config`               | Recommends, previews, and applies Cargo build output config.                             | With `--yes` on `apply` |
| `scheduler`                  | Previews, installs, and runs the platform service. See [Scheduler](#scheduler).          | Through the service |

The root form validates by default. Selection flags decide what a run covers:

| Flag                          | Effect                                                                      |
| ----------------------------- | --------------------------------------------------------------------------- |
| `--all`                       | Selects every eligible smart-trim candidate.                                |
| `--target <path>`             | Selects one target directory. Repeat for more targets.                      |
| `--delete-target`             | Deletes the selected target directories whole. Skips the mode page.         |
| `--yes`                       | Deletes after validation. Without it, the run validates only.               |
| `--dry-run` or `--validate`   | Validates only. Conflicts with `--yes`.                                     |

`--yes` alone does not select targets. Use `--all --yes` for bulk cleanup.
Without a terminal, the root form needs `--all` or `--target`.

Every flag, subcommand, and config key is in the [Command reference](docs/commands.md).

## Safety Model

- Read-only commands are `scan`, `plan`, `list`, `cargo-home report`, and `cargo-config recommend`.
- Destructive commands require `--yes`. Without it, a run validates only.
- `apply` revalidates the saved plan against the filesystem. It skips entries that changed, vanished, became symlinks, or no longer match the saved snapshot.
- A target with an active `cargo` or `rustc` process, or a recent write, is protected as a whole.
- Reclaim runs between builds. Cargo rebuilds anything still needed on the next build.

See [Policies and active build protection](docs/policies.md#active-build-protection) for the full rules.

## Policies

Pass a policy with `--policy <kind>`. The default is `balanced`.

| Policy         | Default | Automatic deletes                                                                       |
| -------------- | ------- | --------------------------------------------------------------------------------------- |
| `observe`      | No      | Nothing. Reports only.                                                                  |
| `conservative` | No      | `incremental` and `tmp` artifacts.                                                      |
| `balanced`     | Yes     | The default removable classes, such as incremental, fingerprint, and deps artifacts.    |
| `aggressive`   | No      | The same classes as `balanced`. Whole-target deletion needs this policy.                |
| `sweep`        | No      | The `balanced` classes plus cold final binaries older than `[planner].sweep_older_than`. |
| `custom`       | No      | The same classes as `balanced`.                                                         |

Protected outputs stay in place under every policy. Examples are docs, packages, timings, and unknown files.
`sweep` is the only policy that deletes final binaries, and only after their age gate.
The full class lists are in [Policies](docs/policies.md).

## Scheduler

`scheduler install` writes the service artifacts for your platform.
The service runs `scheduler service run`, a resident loop. The loop reads `[[background.trigger]]` blocks from a TOML config.
Supported platforms are `systemd-user`, `launchd`, and `task-scheduler`.

Example config:

```toml
version = 1
roots = ["/home/you/Projects/my-crate"]

[policy]
mode = "balanced"
max_target_size = "100 GiB"
target_size_goal = "80 GiB"

[planner]
recent_write_keep_window = "4h"

[scheduler]
mode = "cleanup"
policy = "balanced"
allow_unattended_cleanup = true
allow_unattended_high_policy = true

[background]
enabled = true

[[background.trigger]]
every = "30m"

[[background.trigger]]
every = "5m"
only_when_disk_free_below = "10%"
policy = "sweep"
```

This config trims with the `balanced` policy every 30 minutes.
Every 5 minutes, it runs the `sweep` policy when free disk space falls below 10%.

Preview the service artifacts, then install and check the service:

```sh
cargo-reclaim scheduler preview --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler install --platform systemd-user --config ~/.config/cargo-reclaim/reclaim.toml
cargo-reclaim scheduler service status --config ~/.config/cargo-reclaim/reclaim.toml
```

`scheduler install` runs when you omit `--dry-run`. Pass `--dry-run` to print the plan only.

> **Warning:** `kill_active_builds = true` stops `cargo` and `rustc` processes. `whole_target = "delete"` deletes whole target directories.
> Read [Disruptive triggers](docs/scheduler.md#disruptive-triggers) before you set either key.

The scheduler keeps its own state bounded with retention budgets. See [State retention](docs/scheduler.md#state-retention).
The default state and log paths differ by platform. See [Default paths](docs/scheduler.md#default-paths).

## Saved Plans

A saved plan is the output of `plan --save-plan <path>`. `apply --plan <path> --yes` deletes its entries.

- A saved plan expires after 24 hours. Pass `--expires-in <duration>` to change the expiry.
- `edit-plan` selects or deselects entries. Edit plans with `edit-plan`, not by hand.
- `apply` rejects an expired plan.

See [Saved plans](docs/saved-plans.md) for the file format and the revalidation rules.

## Cargo Home

The `cargo-home` commands cover Cargo home cache data. They follow the same saved-plan model as target cleanup.

```sh
cargo-reclaim cargo-home report --cargo-home ~/.cargo
cargo-reclaim cargo-home plan --cargo-home ~/.cargo --policy conservative --save-plan /tmp/cargo-home-plan.json
cargo-reclaim cargo-home apply --plan /tmp/cargo-home-plan.json --yes
```

`cargo-home apply` validates the saved plan before it deletes anything.

## Documentation

| Document                                                | Covers                                                          |
| ------------------------------------------------------- | --------------------------------------------------------------- |
| [Command reference](docs/commands.md)                   | Every subcommand, flag, and recipe.                             |
| [Config file reference](docs/config.md)                 | Every config key, with defaults and units.                      |
| [Scheduler](docs/scheduler.md)                          | Platform artifacts, service status, triggers, and retention.    |
| [Policies](docs/policies.md)                            | Policy classes, protected outputs, and active build protection. |
| [Saved plans](docs/saved-plans.md)                      | Plan creation, expiry, revalidation, and the file format.       |
| [Discovery and cleanup](docs/discovery.md)              | Target discovery, Cargo config inputs, and partial artifact trimming. |

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for conventions and checks.

Questions and design discussion happen on [Discord](https://discord.gg/jBhFk9kHPg).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
