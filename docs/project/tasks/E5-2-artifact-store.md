# Task: E5-2 Content-addressed artifact store

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Claim: [#59](https://github.com/kunwarshivam/mandate/issues/59) (coordinator `cursor`).

## Story

- **Story:** E5-2 ([backlog](../06-backlog-v1.md#e5-journal))
- **Acceptance criteria (verbatim):** "As an engineer, I want large artifacts stored by content
  hash so that the journal stays small and verifiable." The backlog gives no *Accepted when*
  clause; the interpretation below sets one.
- **PRD / HLD / spec anchors:** PRD 6.7 (FR-7.1 to FR-7.7); HLD §7 "Large artifacts … live in
  object storage, content-addressed by hash; events store the hash"; journal spec §3
  (`artifact_refs`), §6.2 (write-once storage), §6.3 (artifacts), §11 check 6 (`artifact_missing`,
  `artifact_mismatch`).
- **Decisions that apply:** DEC-71 (journal spec), DEC-72 (ADR-0001: ES-02, ES-07, ES-09, ES-11,
  ES-15), DEC-77 (two-PR mechanics), DEC-80 (no plain comments), DEC-83 (mutation gate in tests
  PRs), DEC-107 (this story's crate and semantics).

## Interpretation: accepted when

1. **Address.** An artifact is stored under `sha256:{hex}` of its exact stored bytes (spec §6.3),
   the same form `artifact_refs` holds. Callers storing JSON (configuration objects) pass
   canonical bytes (`mandate_canon::to_canonical`); the store never re-encodes. Personal-data
   artifacts are encrypted by the caller before the put and so are addressed as ciphertext (§6.3).
2. **Put** returns the address and is idempotent: the same bytes give the same address and one
   object. It is write-once: an object is never replaced, and an existing object that no longer
   re-hashes makes the put fail `Corrupt` and stays as evidence (§11: nothing is repaired in place).
3. **Get** re-hashes: a missing object is `Missing`, bytes that do not hash to the address are
   `Corrupt`, and a store that cannot be read is `Unavailable` (safe to retry).
4. **Verification** (§11 check 6) reads through the same trait; `Unavailable` counts as
   `artifact_missing`, so an unreachable store never passes.
5. **Filesystem backend:** atomic publish: the bytes go to a new temporary file that is flushed
   and then hard-linked under the address, so a reader sees a whole object or none, and a write
   that never finished (a temporary file under any name) is never an object and is never reused.
   Durable: the file and the shard directory are flushed. Concurrent puts of the same bytes all
   succeed with one object, objects are read-only, and the store survives reopening. What the
   tests can and cannot pin here is listed under "Planted bugs".

## Scope

- **Reference cases that must move from pending to passing:** none; the journal vectors have no
  artifact case. The existing `journal::tamper::*` and `journal::chain::*` cases must stay passing.
- **Invariants touched** (each a named test; oracles are published SHA-256 vectors, the stored map
  read directly, and the files on disk read directly):

  | Clause | Test |
  |---|---|
  | §6.3 address is SHA-256 of the stored bytes | `artifacts::the_address_is_sha256_of_the_stored_bytes` (FIPS 180-2 vectors), `fs::objects_live_at_sha256_shard_hex_and_are_read_only` |
  | `artifact_refs` form only | `artifacts::references_parse_only_in_the_artifact_refs_form` |
  | Round trip | `artifacts::put_then_get_returns_the_bytes`, `fs::put_then_get_returns_the_bytes` (properties) |
  | Same bytes, same address, one object | `artifacts::the_same_bytes_get_the_same_address_once`, `fs::the_same_bytes_get_the_same_address_once` (properties) |
  | Corruption detected on read | `artifacts::one_flipped_bit_is_detected_on_read`, `fs::one_flipped_bit_on_disk_is_detected_on_read` (properties; flips and truncations) |
  | Never overwrites; §11 nothing repaired in place | `artifacts::put_never_overwrites_a_corrupt_object`, `fs::put_never_overwrites_a_corrupt_object` |
  | Missing versus unavailable | `artifacts::an_absent_reference_is_missing`, `fs::an_absent_reference_is_missing`, `fs::an_unreadable_object_is_unavailable_not_missing`, `fs::a_name_in_place_without_an_object_is_unavailable`, `fs::a_root_that_cannot_be_created_is_unavailable` |
  | §11 check 6 through the store; fails closed | `artifacts::verification_reads_artifacts_through_the_store`, `fs::verification_reads_artifacts_from_disk` |
  | Atomic publish; a crashed write is no object | `fs::readers_see_a_whole_object_or_none_while_it_is_written`, `fs::a_crashed_write_leaves_no_object` |
  | Concurrent, survives reopening | `fs::concurrent_puts_of_the_same_bytes_all_succeed_with_one_object`, `fs::objects_survive_reopening_the_store` |
  | Stable reason codes (ES-09) | `artifacts::error_codes_are_stable` |

- **Crates in scope:** `mandate-journal` (layer 2, pure, safety-critical): `ArtifactRef`,
  `ArtifactError`, `ArtifactSource`, `ArtifactStore`, `get_artifact`, `check_artifact`, the map
  store, and the verifier reading through the trait. New `mandate-artifacts-fs` (layer 6, impure,
  safety-critical, DEC-107): `FsArtifactStore`.
- **Crates out of scope:** `mandate-canon`, `mandate-time`, `mandate-num`, `xtask/src`,
  `mandate-refcases` (unchanged: it passes a map, which still implements the trait),
  `mandate-journal-pg` (E5-3), `mandate-cli`, `mandate-marketdata`.
- **New dependencies allowed:** none. `mandate-artifacts-fs` uses only `std` and workspace crates;
  `proptest` (dev) is already registered.
- **Safety-critical:** yes (the journal's integrity). Tests PR: API stubs, every new test pending
  `E5-2`. Implementation PR: fills the stubs; test files only lose the pending markers.
- **Size budget:** 400 non-generated lines per PR (ES-13).

## Stubs in the tests PR

`ArtifactRef::from_digest`, `ArtifactRef::parse`, `ArtifactRef::digest`, and the map's
`read_artifact` are real in the tests PR, because the E5-1 verifier and its tests keep working
through the new trait (they replace the old `ArtifactSource::artifact`). Every other new body is a
stub, so every new test fails until the implementation PR: `cargo nextest run -p mandate-journal
-p mandate-artifacts-fs --run-ignored ignored-only --no-fail-fast` on the tests PR runs 23 tests
and all 23 fail.

## Planted bugs

ES-11 and the story playbook require each oracle to be shown failing on a seeded bug. Each bug
below was planted by hand in the implementation, both crates' tests were run (`cargo nextest run
-p mandate-journal -p mandate-artifacts-fs --no-fail-fast`), and the bug was reverted. This is on
top of `cargo mutants` with zero missed.

| Bug | Planted in | Change |
|---|---|---|
| J1 | `ArtifactRef::of` | hashes the bytes plus a trailing zero byte |
| J2 | `ArtifactRef::of` | every address is the digest of the empty string |
| J3 | `Display` | drops the `sha256:` prefix |
| J4 | `ArtifactRef::parse` | accepts uppercase hex |
| J5 | `ArtifactError::code` | `Missing` and `Corrupt` codes swapped |
| J6 | `get_artifact` | skips the re-hash |
| J7 | `get_artifact` | reports every source error as `Missing` |
| J8 | `check_artifact` | accepts any non-empty bytes |
| J9 | map `put_artifact` | overwrites an existing object |
| J10 | map `read_artifact` | an absent key is `Unavailable` |
| J11 | map `put_artifact` | bytes already stored are an error |
| V1 | `verify_events` | an artifact the source cannot read (`Unavailable`) passes |
| V2 | `verify_events` | a source's `Corrupt` is `artifact_missing` |
| V3 | `verify_events` | re-hashes only empty bytes from the source |
| F1 | `FsArtifactStore::open` | ignores errors creating the root |
| F2 | `FsArtifactStore::open` | empties the object directory |
| F3 | `object_path` | no `<first two hex>` shard directory |
| F4 | `write_temp` | objects left writable |
| F5 | `put_artifact` | the temporary file is not removed after the link |
| F6 | `write_temp` | one fixed temporary name `tmp/<hex>`, opened with create and truncate |
| F7 | `write_temp` | a temporary name that is taken fails the put (one attempt) |
| F8 | `publish` | renames the temporary file into place |
| F9 | `publish` | copies the temporary file into place (not atomic) |
| F10 | `publish` | an existing name counts as success |
| F11 | `publish` | checks the existing object with the raw read |
| F12 | `publish` | every failed link is `Unavailable` |
| F13 | `read_artifact` | any read error other than not-found is `Missing` |
| F14 | `read_artifact` | not-found is `Unavailable` |
| F15 | `read_artifact` | falls back to the temporary file `tmp/<hex>` |
| F16 | `write_temp` | the file is not flushed (`sync_all` removed) |
| F17 | `publish` | the shard directory is not flushed after the link |

One row per oracle and harness case:

| Test | Oracle | Fails on |
|---|---|---|
| `artifacts::the_address_is_sha256_of_the_stored_bytes` | FIPS 180-2 vectors | J1, J2, J3 |
| `artifacts::references_parse_only_in_the_artifact_refs_form` | spelled-out bad forms | J4 (also J1, J2, J3) |
| `artifacts::error_codes_are_stable` | spec §11 codes written out | J5 |
| `artifacts::an_absent_reference_is_missing` | empty map | J10 |
| `artifacts::put_never_overwrites_a_corrupt_object` | map read directly | J9, J11 (also J2, J8) |
| `artifacts::verification_reads_artifacts_through_the_store` | fixed-error sources | V1, V2, J7 (also J1, J2, J3) |
| `artifacts::put_then_get_returns_the_bytes` (property) | `Digest::of`, map read directly | J1, J2, J3 |
| `artifacts::the_same_bytes_get_the_same_address_once` (property) | byte equality, map size | J11, J2 |
| `artifacts::one_flipped_bit_is_detected_on_read` (property) | flipped copy | J6, J8, J2 |
| `fs::objects_live_at_sha256_shard_hex_and_are_read_only` | FIPS vector, files read directly | F3, F4, F5 (also J1, J2, J3) |
| `fs::an_absent_reference_is_missing` | empty directory | F14 |
| `fs::an_unreadable_object_is_unavailable_not_missing` | a directory at the object path | F13, J7, F10, F15 (also J2) |
| `fs::a_name_in_place_without_an_object_is_unavailable` | a dangling symlink at the object path | F8, F10 (also F9, F14) |
| `fs::a_root_that_cannot_be_created_is_unavailable` | a file at the root path | F1 |
| `fs::objects_survive_reopening_the_store` | a second `open` | F2 |
| `fs::put_never_overwrites_a_corrupt_object` | file read directly | F8, F9, F10, F11, F12, J6, J8 (also J2) |
| `fs::concurrent_puts_of_the_same_bytes_all_succeed_with_one_object` | 8 threads, files listed | F12, F6, F5, F2 |
| `fs::readers_see_a_whole_object_or_none_while_it_is_written` | a reader thread polling 32 objects of 2 MiB | F9 (20 of 20 runs; also F14, J2) |
| `fs::a_crashed_write_leaves_no_object` | torn leftovers under four temporary names | F6, F7, F15, F5, F14 |
| `fs::verification_reads_artifacts_from_disk` | file tampered directly | V3 (also J1, J2, J3) |
| `fs::put_then_get_returns_the_bytes` (property) | `ArtifactRef::of`, file read directly | F5 |
| `fs::the_same_bytes_get_the_same_address_once` (property) | byte equality, files listed | F12, F5, J2 |
| `fs::one_flipped_bit_on_disk_is_detected_on_read` (property) | flipped or truncated copy | J6, J8, J2 |

V3 also fails the E5-1 test `verify::artifacts_are_checked`.

What the tests cannot pin, and why:

- **Durability (F16, F17).** No test fails when the file or the shard directory is not flushed:
  both only matter after a power loss or kernel crash, which a test process cannot cause or
  observe. `cargo mutants` does not delete calls either, so review alone keeps the flushes.
- **A crash at an arbitrary point inside a put.** A test cannot kill the process between two
  system calls. `fs::a_crashed_write_leaves_no_object` plants what such a crash can leave (torn
  bytes under the temporary names a writer uses, including this process's next one) and checks
  none of it is read or reused. The atomicity of the link itself is the filesystem's.
- **Atomicity is sampled, not proven.** `fs::readers_see_a_whole_object_or_none_while_it_is_written`
  catches a non-atomic publish only if the reader lands in the window when a partial file is
  visible. It caught F9 in 20 of 20 runs; a missed run would pass a bad implementation, never
  fail a good one.
- **Filesystems without hard links** (some network and FAT filesystems) are not supported: the
  link fails and every put is `Unavailable`, which the tests show for other link failures but not
  on such a filesystem.

## Not done here (with the story that owns each)

- Object storage with object lock and retention (§6.2): a later backend behind the same traits,
  with the cold store and export story.
- Encryption of personal-data artifacts and the free-text scan (§6.3, §6.4; open question 3): the
  vault stories. The store is agnostic: it addresses whatever bytes it is given.
- Garbage collection of leftover temporary files after a crash: they are never read; removal
  belongs with the operator tooling.
- `ConfigSnapshotRegistered` and replay resolving `config_refs` from the configuration store
  (§8): the first story that registers configuration.
- A `mandate-cli` command over the store: `mandate-cli` is another builder's crate today.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-journal -p mandate-artifacts-fs
cargo nextest run -p mandate-journal -p mandate-artifacts-fs --run-ignored ignored-only --no-fail-fast   # tests PR: all fail
cargo mutants --test-tool nextest -p mandate-journal -p mandate-artifacts-fs \
  --file crates/mandate-journal/src/artifact.rs --file crates/mandate-journal/src/verify.rs \
  --file crates/mandate-artifacts-fs/src/lib.rs
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision (the new crate did: DEC-107).

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: artifacts are referenced by events other
      stories emit).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
