# Changelog

All notable changes to cargo-reclaim will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). cargo-reclaim uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.4.0] - 2026-07-22

### Added

- New `sweep` policy: cargo-sweep-style reclamation of cold final binaries (`final_executable`, `final_rlib`, `final_library`, `final_wasm`) once they are older than a sweep age threshold, on top of the balanced removable set. It never deletes whole targets, docs, packages, or unknown files, and — like every policy — reclaims nothing from a target with an active build. Configure the age gate with `[planner].sweep_older_than` (default 24h).
- Per-trigger policy override: a `[background.periodic]` or `[background.trigger]` block may set its own `policy` (e.g. `policy = "sweep"`), so a disk-pressure trigger can reclaim more aggressively than the routine cadence while still satisfying the unattended high-policy gate.

### Notes

- Active builds remain fully protected: while any `cargo`/`rustc` process is touching a target, cargo-reclaim reclaims nothing from it (it cannot distinguish a superseded hash variant from a live feature-variant the linker needs without cargo's fingerprint DB). Age-based reclaim happens between builds, where cargo re-plans and rebuilds anything removed.

---

## [0.3.0] - 2026-07-21

### Added

- Split the `[background]` watcher into independent, composable trigger blocks: `[background.periodic]` (fires on a timer) and `[background.trigger]` (fires on a poll). Configure either or both, so a routine cadence and a responsive disk-pressure gate can run at once — the latter can trim the instant free space crosses a threshold instead of waiting for the next periodic pass.
- Introduced the *limiter* concept, orthogonal to the trigger: each block may carry `only_when_disk_free_below`, `min_free_disk`, or `max_target_size`. With no limiter a fired run always cleans; with a limiter it cleans only when a threshold is breached. Disk limiters use a cheap free-space check; `max_target_size` scans target sizes.
- Surfaced non-fatal config deprecation notices through `ReclaimConfig::deprecations`, printed as warnings by the scheduler commands.

### Changed

- `mode` is no longer a `[background]` key; how a run is triggered is now expressed by the presence of the `periodic`/`trigger` blocks. Policy and budget config still govern what a run removes and how much.

### Deprecated

- The flat `[background]` keys `mode`, `check_every`, `only_when_disk_free_below`, and `min_free_disk` are still accepted and normalized into the new blocks (with a warning), and will be removed in 0.4. `mode = "periodic"` maps to a `[background.periodic]` block; `mode = "threshold"` maps to a `[background.trigger]` block that inherits `[policy].max_target_size` as a limiter.

### Fixed

- Kept the background inventory from aborting a run when a concurrent `cargo build` deletes an artifact (for example a `deps/*.rcgu.o` object file) between directory enumeration and the snapshot's stat; the vanished path is now skipped like the stale-deps and stale-incremental passes already did, instead of failing the run.

---

## [0.2.2] - 2026-07-07

### Fixed

- Rejected a `plan --json` dry-run report (or an unrecognized file) passed to `apply --plan`, `edit-plan --plan`, or `cargo-home apply --plan` with an actionable error that points at `--save-plan`, instead of a raw serialization error about a missing `id` field ([#1](https://github.com/ml-rust/cargo-reclaim/issues/1)).
- Discovered a shared `CARGO_TARGET_DIR` or `build.target-dir` that lives on a different filesystem than the project root without requiring `--cross-filesystems`; the flag now governs incidental traversal only, not explicitly configured output locations ([#2](https://github.com/ml-rust/cargo-reclaim/issues/2)).
- Recognized a cargo target directory by its `.rustc_info.json` marker regardless of directory name, so a shared target directory named e.g. `cargo-target` is listed and cleanable; the generic `CACHEDIR.TAG` marker still requires the conventional `target` name ([#2](https://github.com/ml-rust/cargo-reclaim/issues/2)).

### Changed

- Explained empty `list` results, distinguishing "no Rust project found under the scanned roots" from "Rust projects found, but no cleanable target directories," in both terminal and JSON output.

---

## [0.2.1] - 2026-07-05

### Fixed

- Summarized foreground cleanup/apply terminal output by default and wrote complete per-run JSON reports under the cargo-reclaim state directory.

---

## [0.2.0] - 2026-07-05

### Changed

- Made `cargo-reclaim <roots...>` the primary cleanup assistant entrypoint, with smart trim as the default cleanup mode for active projects.
- Made `cargo-reclaim list <roots...>` the read-only target inventory surface and removed the old public `targets` command surface.
- Moved explicit whole-target deletion to the cleanup assistant path with `--target <path> --delete-target --yes`, keeping whole-target cleanup separate from default smart trim.
- Updated CLI help, JSON inventory output, README examples, and release preparation metadata around the 0.2.0 command model.

### Added

- Real terminal assistant coverage for selector, mode, confirmation, cancellation, page-skipping flags, and non-TTY guard behavior.
- Deterministic CLI integration test isolation from live cargo/rustc process scans.

### Fixed

- Improved target inventory sizing throughput and stale plan-test stability before the 0.2.0 release.

---

## [0.1.1] - 2026-07-04

### Fixed

- Support Cargo subcommand invocation through `cargo reclaim ...` by accepting Cargo's leading `reclaim` shim argument before normal command parsing.

---

## [0.1.0] - 2026-07-03

> First release of cargo-reclaim: safe Cargo artifact cleanup for real Rust workstations.

### Added

#### Planning and apply

- Read-only scan and plan workflows for Cargo target directories.
- Persisted cleanup plans with expiration and apply-time revalidation.
- Policy modes: `observe`, `conservative`, `balanced`, `aggressive`, and `custom`.
- Ignore, skip, recent-write, keep-size, and rustc/toolchain preservation controls.

#### Target cleanup

- Cargo target discovery from project roots, Cargo configuration, and target-root evidence.
- Interactive target directory discovery and selected whole-target cleanup through `cargo-reclaim targets` and `cargo-reclaim targets clean`.
- Partial cleanup classes for incremental artifacts, fingerprints, build-script caches, dep-info files, temporary files, object metadata, stale fingerprint-group intermediates, stale hashed `deps` variants, old deps outputs guarded by recent-write windows, and stale incremental sessions or unit variants.
- Protected classes for whole targets, docs, packages, timings, final binaries, final libraries, final `.rlib`, final `.wasm`, and unknown artifacts.

#### Cargo home

- Cargo home cache reporting.
- Cargo home cleanup plans and saved-plan apply.

#### Scheduler

- Config-driven scheduler execution.
- Resident scheduler service support with platform preview/install/uninstall flows for Linux `systemd-user`, macOS `launchd`, and Windows Task Scheduler.
- Active `cargo`/`rustc` process detection on Linux through procfs and on macOS/Windows through a native process-table provider.
- Threshold background cleanup with per-target size ceilings, target size goals, absolute free-space triggers, and durable service state.
- Durable scheduler run logs and state directories.

#### Cargo config

- Read-only Cargo config recommendations.
- Cargo config preview and explicit apply flow.

#### Documentation and automation

- Saved plan editing with list, selector, and interactive modes.
- JSON output for automation-friendly scan, plan, scheduler, target, and Cargo home workflows.
- Deep directory measurement for CLI, scheduler, and background service planning, so cleanup budgets and reported sizes reflect real directory contents.
- Scheduler service status that keeps `running` when PID liveness cannot be inspected from the current environment instead of falsely reporting a stale service.

### Safety

- Destructive flows require explicit confirmation with `--yes`.
- Saved apply flows revalidate path kind, size, modification time, and symlink state before deletion.
- Whole-target deletion remains separate from partial artifact trimming and requires explicit selected target cleanup or whole-target policy configuration.

---

[0.2.2]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.2.2
[0.2.1]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.2.1
[0.2.0]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.2.0
[0.1.1]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.1.1
[0.1.0]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.1.0
