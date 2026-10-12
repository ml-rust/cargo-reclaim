# Contributing

Thanks for contributing to [cargo-reclaim](https://crates.io/crates/cargo-reclaim). This guide covers the safety model, architecture conventions, and quality gates.

## Before you start

- Rust 1.88 or newer (edition 2024).
- Know whether your change affects read-only reporting, plan generation, persisted plans, or deletion.
- A clean working tree before you open a pull request, except for files the change intends to include.

Useful contributions:

- Safer artifact classification.
- Faster target discovery.
- Better scheduler behavior.
- Clearer dry-run and apply output.
- Stronger persisted-plan checks.
- Platform service support.
- Tests that cover real Cargo project layouts.

Open an issue or design note before you change cleanup policy, deletion behavior, scheduler automation, or persisted-plan compatibility. Small documentation fixes, test additions, and narrow bug fixes can go straight to a pull request.

## Safety model

cargo-reclaim is a cleanup tool, so safety is part of the public API. Preserve these rules unless a design replaces them with stronger ones:

- Read-only commands stay read-only.
- Destructive commands require an explicit `--yes` or an equivalent confirmed execution path.
- Check saved plans against the current filesystem state before deletion.
- Skip or report changed entries, symlink substitutions, vanished paths, and mismatched snapshots. Never force them.
- Keep whole-target deletion separate from partial artifact trimming.
- Do not bypass active-build protection or recent-write preservation for a broad class. Bypass them only with specific proof that a candidate is stale.
- Keep JSON output stable enough for automation.

## Architecture

cargo-reclaim separates discovery, classification, planning, persistence, execution, and scheduler orchestration. Put new behavior in the layer that owns the decision:

| Layer | Owns |
| --- | --- |
| Inventory and scanner code | Cargo project evidence, target directories, filesystem entries. |
| Classifier code | Artifact classes and protection semantics. |
| Planner code | The decision to delete, preserve, skip, or require confirmation, based on policy and context. |
| Persistence code | Saved-plan serialization and checks before apply. |
| Executor code | Deletion, only after checks pass. |
| Scheduler and background service code | When planning and apply flows run. Not what filesystem entries mean. |
| CLI code | Translation of user intent into the layers above, and output formatting. |

Prefer focused files named by responsibility. Keep `mod.rs` files to declarations, re-exports, and wiring. Do not place cleanup decisions in generic utility modules.

## Testing

- Put unit tests in `#[cfg(test)] mod tests` next to the code. Use them for private helpers and small isolated decisions.
- Put integration tests in top-level `tests/*.rs`. Use them for public CLI behavior, cross-module planning, persisted plans, executor and apply behavior, Cargo home flows, and scheduler behavior.
- Put shared integration-test helpers in `tests/common/mod.rs` or another subdirectory module. Do not use `tests/common.rs`. Cargo compiles that file as an empty integration-test crate.
- Use temporary directories for deletion tests. Assert both the planned actions and the surviving protected files.
- Scheduler tests do not depend on a real system service manager. Mark a test that does as an acceptance or manual test.

## Quality checks

Run these before you submit Rust changes:

```sh
cargo fmt --all
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --no-fail-fast -- --test-threads=1
git diff --check
```

When you change dependencies, also run the audit CI gates:

```sh
cargo deny --exclude-dev check   # advisories, licenses, sources
cargo check --locked --workspace --all-targets --all-features   # on the toolchain matching Cargo.toml's rust-version
```

For documentation-only changes, run at least `git diff --check`.

## Pull requests

- Keep each pull request focused and scoped.
- Include tests for behavioral changes.
- Update the README, CHANGELOG, examples, or config snippets when public behavior changes.
- Preserve dry-run-first behavior and recheck saved plans at apply time.
- Do not weaken tests, lints, or safety checks to make a change pass.
- Avoid `unwrap`, `expect`, and `panic` in library and runtime code. Return typed errors with context.
- Avoid repeated filesystem scans, unnecessary hashing, and unbounded directory walks in hot paths.

## Commit messages

Use Conventional Commits with a clear, imperative summary:

```text
feat(targets): add interactive selected cleanup
fix(scheduler): preserve running status when pid liveness is unknown
docs(readme): clarify active-project scheduler setup
```
