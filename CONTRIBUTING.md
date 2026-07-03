# Contributing

Thanks for contributing to [cargo-reclaim](https://crates.io/crates/cargo-reclaim). This guide covers the safety model, architecture conventions, and quality gates the project expects.

## Prerequisites

- Rust 1.85+ (edition 2024).
- A clean understanding of whether your change affects read-only reporting, plan generation, persisted plans, or deletion.
- A clean working tree before opening a pull request, except for intentional files in the change.

## What to contribute

The most valuable contributions are usually safer artifact classification, faster target discovery, better scheduler behavior, clearer dry-run/apply UX, stronger persisted-plan validation, platform service support, and tests that cover real Cargo project layouts.

Before changing cleanup policy, deletion behavior, scheduler automation, or persisted plan compatibility, open an issue or design note first. Small documentation fixes, test additions, and narrow bug fixes can go straight to a pull request.

## Safety Model

cargo-reclaim is a cleanup tool, so safety is part of the public API. Preserve these rules unless a design explicitly replaces them with something stronger:

- Read-only commands must stay read-only.
- Destructive commands must require an explicit `--yes` or equivalent confirmed execution path.
- Saved plans must be revalidated against current filesystem state before deletion.
- Changed entries, symlink substitutions, vanished paths, and mismatched snapshots must be skipped or reported instead of forced.
- Whole-target deletion must remain separate from partial artifact trimming.
- Active build protection and recent-write preservation must not be bypassed for broad classes without a specific proof that the candidate is stale.
- JSON output should remain stable enough for automation.

## Architecture

cargo-reclaim separates discovery, classification, planning, persistence, execution, and scheduler orchestration. Keep new behavior in the layer that owns the decision:

- **Inventory and scanner code** finds Cargo project evidence, target directories, and filesystem entries.
- **Classifier code** assigns artifact classes and protection semantics.
- **Planner code** decides delete, preserve, skip, or require confirmation based on policy and context.
- **Persistence code** serializes saved plans and validates them before apply.
- **Executor code** performs deletion only after validation.
- **Scheduler and background service code** decide when to run planning/apply flows, not what filesystem entries mean.
- **CLI code** translates user intent into the above layers and formats output.

Prefer focused files named by responsibility. Keep `mod.rs` files to declarations, re-exports, and wiring. Do not hide cleanup decisions in generic utility buckets.

## Testing

- Put unit tests next to the code they exercise inside `#[cfg(test)] mod tests` when testing private helpers or small isolated decisions.
- Put integration tests in top-level `tests/*.rs` when testing public CLI behavior, cross-module planning, persisted plans, executor/apply behavior, Cargo home flows, or scheduler behavior.
- Put shared integration-test helpers under `tests/common/mod.rs` or another subdirectory module, not `tests/common.rs`, so Cargo does not compile the helper as a separate empty integration-test crate.
- Tests for deletion behavior should use temporary directories and assert both planned actions and surviving protected files.
- Scheduler tests should avoid depending on a real system service manager unless explicitly marked as an acceptance/manual test.

## Local Quality Checks

Run these before submitting Rust changes:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features
cargo test -q -- --test-threads=1
git diff --check
```

For documentation-only changes, run at least:

```sh
git diff --check
```

## Pull Request Guidelines

- Keep PRs focused and scoped.
- Include tests for behavioral changes.
- Update README, CHANGELOG, examples, or config snippets when public behavior changes.
- Preserve dry-run-first behavior and apply-time revalidation.
- Do not weaken tests, lints, or safety checks to make a change pass.
- Avoid `unwrap`, `expect`, and `panic` in library/runtime code; return typed errors with context.
- Avoid repeated filesystem scans, unnecessary hashing, or unbounded directory walks in hot paths.

## Commit Messages

Use Conventional Commits with a clear, imperative summary, for example:

```text
feat(targets): add interactive selected cleanup
fix(scheduler): preserve running status when pid liveness is unknown
docs(readme): clarify active-project scheduler setup
```
