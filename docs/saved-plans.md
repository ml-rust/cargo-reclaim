# Saved plans

A saved plan is the output of `plan --save-plan <path>`. The `apply` command reads it.
The `edit-plan` command reads and rewrites it.

Plans are machine-revalidated documents. Do not edit them by hand. Use `edit-plan` instead.

## Create and apply a plan

```sh
cargo-reclaim plan ~/Projects/my-crate --policy balanced --save-plan /tmp/reclaim-plan.json
cargo-reclaim apply --plan /tmp/reclaim-plan.json
cargo-reclaim apply --plan /tmp/reclaim-plan.json --yes
```

- `plan` writes the plan file and deletes nothing.
- `apply` without `--yes` revalidates the plan and reports the result. It deletes nothing.
- `apply --yes` deletes the entries that pass revalidation.
- `apply` requires `--plan <path>`. It does not accept an implicit last plan.

## Expiry

A saved plan expires after 24 hours by default.
Pass `--expires-in <duration>` with `--save-plan` to set another expiry, for example `--expires-in 4h`.
`apply` rejects an expired plan with `persisted plan has expired`.

## Revalidation

`apply` checks the current filesystem against the saved snapshot before it deletes anything.
It skips an entry when:

- The entry changed since the plan was saved.
- The path vanished.
- The path became a symlink.
- The snapshot no longer matches.

A skipped entry has the status `skip_stale_plan` in the report. cargo-reclaim does not force it.

## Review and edit a plan

```sh
cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --list
cargo-reclaim edit-plan --plan /tmp/reclaim-plan.json --interactive
```

| Flag                           | Effect                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| `--list`                       | Prints the entries. Writes nothing.                        |
| `--interactive`                | Opens a selection prompt. Saves the plan when you accept the prompt. |
| `--select <name>`              | Selects an entry by name.                                   |
| `--select-index <n>`           | Selects an entry by list index.                             |
| `--select-class <class>`       | Selects every entry in an artifact class.                   |
| `--deselect`, `--deselect-index`, `--deselect-class` | Deselects entries with the same selectors. |
| `--json`                       | Prints entries fully expanded.                              |

Each edit flag (`--select`, `--select-index`, `--select-class`, `--deselect`, `--deselect-index`, `--deselect-class`) rewrites the plan file in place. `--list` does not.

## File format

The writer stores repeated values once in a `dictionaries` table at the snapshot level.
Each entry holds indexes into that table.

- Decoding rejects an out-of-range index. A plan that cannot be rebuilt exactly is never applied.
- To read a plan with `jq`, resolve the indexes through `.plan.dictionaries`.
- `edit-plan --list --json` prints every entry fully expanded. Use it when a script needs plain entries.

## Audit plans from the background service

The background service writes audit plans. An audit plan records one `active_skips` entry per protected target
instead of one entry per file. Each `active_skips` entry holds:

| Field         | Meaning                                                        |
| ------------- | -------------------------------------------------------------- |
| `target`      | The protected target directory.                                |
| `entry_count` | Number of files the planner collapsed into this entry.         |
| `total_bytes` | Total size of those files.                                     |
| `reasons`     | The distinct active-build signals that fired.                  |

`totals` still counts every entry the planner produced.
Plans you save with `--save-plan` keep every entry, so `edit-plan` can select any entry the planner protected.

Background audit plans live in `<state_dir>/plans/`. See [Scheduler](scheduler.md#state-retention) for retention.
