# Changelog

All notable changes to cargo-reclaim will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). cargo-reclaim uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

> First release candidate for cargo-reclaim: dry-run-first Cargo artifact cleanup for large Rust workstations.

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

[Unreleased]: https://github.com/ml-rust/cargo-reclaim
