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
5. **Filesystem backend:** atomic publish (a reader sees a whole object or none), durable (file and
   directory flushed), concurrent puts of the same bytes all succeed with one object, objects
   read-only, and the store survives reopening.

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
  | Atomic, concurrent, durable | `fs::concurrent_puts_of_the_same_bytes_all_succeed_with_one_object`, `fs::objects_survive_reopening_the_store` |
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
stub, so every new test fails until the implementation PR.

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
cargo nextest run -p mandate-journal -p mandate-artifacts-fs --run-ignored all   # tests PR
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
