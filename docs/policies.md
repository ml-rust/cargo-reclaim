# Policies and active build protection

A policy sets which artifact classes a plan can delete.
Set it with `--policy <kind>` or the `mode` key in `[policy]`.

## Policy kinds

| Policy         | Default | Automatic deletes in a plan                                                                 | Typical use                                              |
| -------------- | ------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `observe`      | No      | Nothing. Cleanup-capable entries are preserved for reporting.                               | Inventory, dashboards, CI reporting, first run on a new machine. |
| `conservative` | No      | `incremental` and `tmp` artifacts only.                                                     | Active projects where rebuild impact must stay minimal.  |
| `balanced`     | Yes     | The default removable classes. See below.                                                   | Normal workstation cleanup and scheduled partial trimming. |
| `aggressive`   | No      | The same classes as `balanced`. Whole-target deletion stays separate.                       | One-off deep cleanup when rebuild cost is acceptable.    |
| `sweep`        | No      | The `balanced` classes plus cold final binaries. Final binaries need `[planner].sweep_older_than` age (default `24h`). | Disk-pressure recovery. Frees stale binaries and keeps the hot build path. |
| `custom`       | No      | The same classes as `balanced`.                                                             | Reserved for future config-driven tuning.                |

### Default removable classes

The `balanced`, `aggressive`, and `custom` policies delete these classes:

- `incremental`
- `build_scripts`
- `fingerprint`
- `tmp`
- `fingerprint_group_intermediate`
- `stale_deps`
- `stale_incremental`
- `deps_output`
- `dep_info`
- `object_metadata`

`deps_output` entries delete only when `recent_write_keep_window` is set.
`sweep` adds `final_executable`, `final_library`, `final_rlib`, and `final_wasm` after the age gate.

## Protected outputs

These outputs stay protected under every policy except whole-target deletion:

- Whole target directories
- `docs`
- `package`
- `timings`
- `final_executable`
- `final_library`
- `final_rlib`
- `final_wasm`
- `unknown`

`sweep` releases the four `final_*` classes once they pass the age gate.
A target with only name-based evidence requires confirmation. Pass `--allow-name-only-targets` to include such targets as confirmation-only. The planner does not delete it automatically.

## Active build protection

cargo-reclaim protects a whole target when a build is active on it. A target counts as active when either signal fires:

- A running `cargo` or `rustc` process has its working directory under the project root.
- A running `cargo` or `rustc` process references a path that overlaps the project root.
- A file in the target was written within the `--keep-recent-writes` window. The window must be set for this signal.

The planner reclaims nothing from an active target. It cannot tell which artifacts a running build needs.
A superseded hashed `deps` variant can still be a live feature variant under `--all-features`.
Cargo also does not rebuild an output its fingerprint database considers fresh.

Reclaim happens between builds. Cargo re-plans and rebuilds anything it still needs on the next build.
A target under continuous heavy building cannot shrink through normal cleanup.
Use a disruptive trigger from [Scheduler](scheduler.md#disruptive-triggers), or run `cargo clean` in a quiet window.

### Process detection

On Linux, cargo-reclaim reads `/proc` to find running processes.
On macOS and Windows, it uses the `sysinfo` crate.
Set `CARGO_RECLAIM_DISABLE_ACTIVE_PROCESS_DETECTION=1` to turn off process detection. The write-window signal still applies.

A disruptive trigger that sets `interrupt_active_build = true` skips active build protection on purpose.
