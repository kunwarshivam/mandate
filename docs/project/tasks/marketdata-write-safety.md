# Task: E2-1 follow-up: safe concurrent market-data dataset writes

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Claim
[#86](https://github.com/kunwarshivam/mandate/issues/86), coordinator `cursor`.

## Story

- **Story:** the E2-1 follow-up in the work tracker's known issues: "Market-data writes use a fixed
  `.partial` temporary name; concurrent writers to one partition need a lock or unique names",
  owned by "Before any parallel download".
- **Acceptance criteria:** writers of one dataset running at once, as threads or processes, never
  tear or replace a partition and end with identical, valid content; a crashed write's leftover
  neither blocks a write nor gets published; compare-before-write idempotency is kept; no manifest
  entry is lost.
- **PRD / HLD / spec anchors:** PRD FR-4.5 (backtests reproducible from a data snapshot); HLD §9
  "Market data service".
- **Decisions that apply:** DEC-89 (per-day partitions, the manifest, compare-before-write: identical
  bytes are left untouched, new partitions are written atomically, different bytes fail the run
  without touching the file), DEC-88, DEC-80. None taken here.

## The defect

`Store::put_day` read the manifest, checked the partition, and wrote each file through
`<file>.partial` and a rename. With two writers of one dataset:

- both open the same `.partial`; one truncates the other's half-written file, which the first then
  renames into place (a torn file), and the second's rename fails;
- both read the manifest, each adds its own day, and the last rename wins (a lost entry);
- two writers of one day with different data both see no partition, and the last rename replaces
  the first (DEC-89 says a stored partition is never replaced);
- a `.partial` left by a crash is truncated and reused.

The new tests reproduced all of these on the old code: torn manifests ("data after the value at
byte 560"), missing-file errors from the shared name, and a reused leftover.

## Design

1. **Lock.** `put_day` takes an exclusive advisory lock (`std::fs::File::lock`, `flock` on Linux
   and macOS) on the dataset directory, from before it reads the manifest until after it writes
   it. The manifest is shared by every day of a dataset, so this, and not an atomic replace with a
   retry, is the choice: the read, the checks, and both writes form one step, and a writer that
   loses a race sees the winner's partition and manifest and answers `Unchanged` or `Conflict`
   exactly as a later run would. The directory is locked, not a lock file in it, because the
   directory is never replaced and a lock file would be one more entry in every dataset (the
   E2-1 test that a run leaves two partitions and a manifest would see it). Datasets lock
   independently, so parallel downloads of different symbols do not wait on each other.
2. **Unique temporary names.** Each file is written to `<file>.<pid>.<n>.partial`, where `n` counts
   within the process, created with `create_new`; a taken name (a running writer's or a crashed
   one's) is skipped for the next `n`. A leftover is never opened, so it is never reused or
   published, and it never blocks a write. The names do not end in `.parquet`, so `inspect` does
   not report them as unlisted partitions.
3. **No-replace publish.** A new partition is published by a hard link from its flushed temporary
   file, which fails rather than replace an existing file, then the temporary file is removed and
   the directory flushed. This is E5-2's publish (`mandate-artifacts-fs`: unique temporary name,
   flush, hard link, directory flush). Under the lock the link cannot meet an existing file from
   another store writer; it guards against a writer that ignores the lock. The manifest must be
   replaced, so it is renamed into place under the lock, as before.

Filesystems without hard links (FAT, exFAT) cannot hold a dataset; E5-2 makes the same trade.

## Tests

In `crates/mandate-marketdata/tests/concurrent_writes.rs`. Every oracle computes the expected
bytes and manifest listing from `dataset::encode` and `Digest::of`, not from the store.

| Clause | Test |
|---|---|
| Eight writers of one partition: exactly one writes it, the rest find it unchanged; the partition and manifest are whole and identical to the encoding; no temporary file is left; a re-run changes no file | `concurrent_writers_of_one_partition_store_it_once_and_whole` (16 rounds) |
| Eight writers of one day with different data: the first stored version stays, every other writer gets `Conflict`, and the manifest agrees with the partition | `concurrent_writers_with_different_data_keep_the_first_and_report_conflicts` (16 rounds) |
| Eight writers of 48 different days, half of them empty: every put is `Written` and the manifest lists all 48 | `concurrent_writers_of_different_days_lose_no_manifest_entry` (4 rounds) |
| Four processes writing the same 12 days from different starting days: all succeed, the manifest lists every day, each partition decodes to its records, and each day is written by exactly one process | `concurrent_processes_writing_one_dataset_agree` (re-runs the test binary; `writer_process` is the child's body and does nothing when run directly) |
| Leftovers at the old `.partial` name and at the first 1024 names this process would use, longer than the real content: the write succeeds, publishes the real bytes, leaves every leftover byte-for-byte as it was, and `inspect` finds no problem | `a_crashed_writes_leftovers_are_neither_reused_nor_published` |
| A temporary file that cannot be created (a read-only dataset directory) is an I/O error, not an endless retry | `a_temporary_file_that_cannot_be_created_is_an_error_not_a_retry` (needs a non-root user, as CI's runner is) |
| Compare-before-write idempotency | the E2-1 tests in `tests/download.rs`, unchanged and passing, and the re-run check in the first test above |
| A partition on disk that the manifest does not list is neither overwritten nor adopted, with records or without | `a_partition_the_manifest_does_not_list_is_never_overwritten_or_adopted` (`tests/dataset.rs`) |
| A listed partition that was deleted is restored only by the same records; other records, or none, are a conflict that writes nothing | `a_listed_partition_that_was_deleted_is_restored_only_with_the_same_records` (`tests/dataset.rs`) |

The two `tests/dataset.rs` tests cover E2-1 checks in `put_day` that the wider mutation run below
showed no test pinned.

The leftover test names the temporary-file scheme, as E5-2's crash test does, because a leftover
only matters at a name a writer would try.

## Planted bugs

Each bug was planted in `crates/mandate-marketdata/src/dataset.rs`, the crate's tests run with
`cargo nextest run -p mandate-marketdata --no-fail-fast`, and the change reverted.

| Planted bug | Caught by |
|---|---|
| P1: no lock | one partition, different data, different days, processes |
| P2: the E2-1 write path (fixed `.partial`, rename, no lock) | all five concurrent-write tests |
| P3: temporary file opened without `create_new` and without truncation (reuses a leftover) | leftovers |
| P4: a taken temporary name is an error instead of being skipped | leftovers |
| P5: partition renamed into place, no lock | one partition, different data, different days, processes |
| P6: temporary file not removed after the link | all five concurrent-write tests, and E2-1's `second_run_changes_no_file` |
| P7: lock taken after the manifest is read | one partition, different days, processes |
| P8: lock guard dropped at once (`let _ = lock(..)`) | one partition, different data, different days, processes |
| P9: partition renamed into place, lock kept | not caught: under the lock no store writer can create the partition between the check and the publish, so rename and hard link behave the same; the link only guards against writers that bypass the lock |
| P10: directory not flushed after the link or rename | not caught: the flush makes the new name survive power loss, which a test cannot observe without crashing the machine |

## Mutation testing

`mandate-marketdata` is not safety-critical, so CI's diff gate skips it; these runs were made by
hand with cargo-mutants 27.1.0 and nextest, each job building in its own copy.

- On the diff (`cargo mutants --in-diff <diff of src/> -p mandate-marketdata`): 9 mutants, 6
  caught, 2 unviable (`lock` and `put_day` returning `Default`; the lock is covered by planted
  bugs P1, P7, and P8), 1 timeout (the `AlreadyExists` guard forced to `true` retries forever on a
  read-only directory, which is how the test catches it), 0 missed.
- On every mutant of `put_day` and the write helpers (`--file src/dataset.rs --re
  '...put_day|lock|temporary|write_new|replace|sync...'`): 21 mutants, 15 caught, 2 timeouts, 2
  unviable, 2 missed, both justified:
  - `replace > with < in Store::put_day` (line 165, E2-1 code): equivalent. For a day without
    records, a listed entry with rows above 0 is refused a few lines later anyway, because it
    differs from the empty entry, and nothing is written in between.
  - `replace sync -> Result<(), DatasetError> with Ok(())`: the directory flush only matters after
    power loss (planted bug P10). `--in-diff` does not generate it, because its body lines are
    unchanged from E2-1's `write_atomically`.

## Scope

- **Reference cases:** none (no market-data reference cases exist).
- **Crates in scope:** `mandate-marketdata`, the store's write path only (`Store::put_day` and its
  helpers in `src/dataset.rs`). `inspect`, `download`, `mandate-cli`, and corporate actions are
  untouched; the E2-4 wiring PR owns them.
- **New dependencies allowed:** none (`File::lock` is in the standard library since Rust 1.89).
- **Safety-critical:** no (market data), so this ships as one PR with its tests.

## Not done here

- Readers take no lock: a partition appears whole or not at all, and the manifest is replaced by
  rename, so a reader sees one manifest or the other.
- Parallel downloads themselves (several datasets at once in `mandate download`): this change only
  makes them safe to add.
- Cleaning up old leftovers: they are harmless and named `*.partial`; a user may delete them.

## Commands

```bash
cargo nextest run -p mandate-marketdata --test concurrent_writes
cargo nextest run -p mandate-marketdata -p mandate-cli
MANDATE_BASE_REF=$(git merge-base origin/main HEAD) cargo xtask check
```

## Stop conditions

None fired: no new dependency, no deviation from an accepted decision (DEC-89's
compare-before-write and never-replace rules are now also true under concurrency), no test
weakened or changed.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first and failed on the old code; each oracle failed on a planted bug (table
      above).
- [x] New state changes emit journal events (none: research data only).
- [x] Docs updated: this brief, the feature map, the work tracker.
- [x] `cargo xtask check` is green (summary in the PR).
- [x] The PR description is complete.
