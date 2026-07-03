# Changelog

All notable changes to cargo-reclaim will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). cargo-reclaim uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

- Interactive target directory discovery and selected whole-target cleanup through `cargo-reclaim targets` and `cargo-reclaim targets clean`.
- Resident scheduler service support with platform preview/install/uninstall flows for Linux `systemd-user`, macOS `launchd`, and Windows Task Scheduler.
- Threshold background cleanup with per-target size ceilings, target size goals, absolute free-space triggers, and durable service state.
- Partial stale artifact trimming for stale hashed `deps` variants, old deps outputs guarded by recent-write windows, and stale incremental sessions or unit variants.
- Cargo home report, plan, and saved-plan apply workflows.
- Saved plan editing with list, selector, and interactive modes.
- JSON output for automation-friendly scan, plan, scheduler, target, and Cargo home workflows.

### Changed

- CLI, scheduler, and background service planning use deep directory measurement so cleanup budgets and reported sizes reflect real directory contents.
- Scheduler service status keeps `running` when PID liveness cannot be inspected from the current environment instead of falsely reporting a stale service.
- README now follows the public ml-rust project format with a short positioning intro, capability sections, practical recipes, and scheduler guidance.

### Safety

- Destructive flows require explicit confirmation with `--yes`.
- Saved apply flows revalidate path kind, size, modification time, and symlink state before deletion.
- Whole-target deletion remains separate from partial artifact trimming and requires explicit selected target cleanup or whole-target policy configuration.

---

## [0.1.0] - 2026-07-03

> First release of cargo-reclaim: dry-run-first Cargo artifact cleanup for large Rust workstations.

### Added

#### Planning and apply

- Read-only scan and plan workflows for Cargo target directories.
- Persisted cleanup plans with expiration and apply-time revalidation.
- Policy modes: `observe`, `conservative`, `balanced`, `aggressive`, and `custom`.
- Ignore, skip, recent-write, keep-size, and rustc/toolchain preservation controls.

#### Target cleanup

- Cargo target discovery from project roots, Cargo configuration, and target-root evidence.
- Partial cleanup classes for incremental artifacts, fingerprints, build-script caches, dep-info files, temporary files, object metadata, and stale fingerprint-group intermediates.
- Protected classes for whole targets, docs, packages, timings, final binaries, final libraries, final `.rlib`, final `.wasm`, and unknown artifacts.

#### Cargo home

- Cargo home cache reporting.
- Cargo home cleanup plans and saved-plan apply.

#### Scheduler

- Config-driven scheduler execution.
- Scheduler artifact preview for supported desktop service managers.
- Durable scheduler run logs and state directories.

#### Cargo config

- Read-only Cargo config recommendations.
- Cargo config preview and explicit apply flow.

---

[Unreleased]: https://github.com/ml-rust/cargo-reclaim/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ml-rust/cargo-reclaim/releases/tag/v0.1.0
