# Discovery and cleanup

This page covers how cargo-reclaim finds targets and which artifacts it trims.
For the policy classes that decide deletion, see [Policies](policies.md).

## Target discovery

cargo-reclaim finds Cargo target directories under each root. It reports their measured sizes, largest first.

- Scans each `ROOT` argument, or each `roots` entry in the config file when no `ROOT` is given.
- Reads `build.target-dir` and `build.build-dir` from Cargo config files.
- Reads the `CARGO_BUILD_TARGET_DIR` and `CARGO_TARGET_DIR` environment variables.
- Finds shared target directories outside the project tree, including a target on another disk.
- Excludes `target/` directories that match only by name. Pass `--allow-name-only-targets` to include them as confirmation-only.
- Reports `build.build-dir = "{workspace-root}/{workspace-path-hash}"` as unsupported. The template is never used as a write target.

Pass `--ignore <path>` to report a path as ignored. Pass `--skip <path>` to prune a path and its descendants from the scan.
Pass `--follow-symlinks` or `--cross-filesystems` to widen the scan.

## Partial artifact cleanup

A partial cleanup removes stale artifacts and keeps the active build path. The `balanced` policy trims these artifacts:

- Stale rustc incremental cache sessions and older compile-unit variants.
- Stale hashed files under `target/*/deps` when older hash variants can be told apart from the newest family member.
- Direct old deps outputs. These trim only when `--keep-recent-writes` or `recent_write_keep_window` is set.
- Fingerprints, build-script caches, temporary files, dep-info files, and object metadata.

The planner preserves these outputs unless whole-target cleanup is requested:

- Final binaries, final libraries, and final `.rlib` and `.wasm` files.
- Docs and packages.
- Timings.
- Unknown artifacts.
- Whole target directories.

Whole-target cleanup is a separate step. Pass `--whole-target delete` to plan it. The `delete` mode requires the `aggressive` policy.
The root form deletes whole targets only with `--delete-target` on selected targets.

## Cargo home

The `cargo-home` commands cover Cargo home cache data.

- `cargo-home report` reports cache usage and preserved paths. It is read-only.
- `cargo-home plan` builds a saved plan for Cargo home data.
- `cargo-home apply --plan <path> --yes` validates the saved plan and deletes its entries.

See [Command reference](commands.md#cargo-home) for every flag.
