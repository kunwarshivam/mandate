# Task: `xtask` per-PR checks for `reference/journal` and the mutation anchor (E6-13 follow-ups)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Claim
[#646](https://github.com/kunwarshivam/mandate/issues/646), coordinator `claude-code`.

## Story

- **Story:** two follow-ups recorded under E6-13 in the [backlog](../06-backlog-v1.md): "run
  `generate.py --check` per PR" (#443 round 3, m14; #476 round 1, m4) and "make a missing anchor
  fail per-PR CI, for example with an anchor-only check that runs in seconds" (#443 round 3).
- **Acceptance criteria:** `cargo xtask ci reference` runs `reference/journal/generate.py --check`
  and an anchor-only check of `reference/mandate/mutants.py` that fails when an anchor is missing
  or stale, without the mutant sweep; the nightly's full `mutants.py` run stays; each check is
  shown passing on `main`'s state and failing on a planted drift; the job stays well inside the
  ten-minute budget (DEC-464, kept by DEC-473).
- **PRD / HLD / spec anchors:** ADR-0001 ES-10 (the frozen reference implementations and their
  pinned environment), ES-12 (every CI job is one `cargo xtask ci <job>`), ES-22 (protected
  paths); the quality and release process ([07](../07-quality-and-release.md)).
- **Decisions that apply:** DEC-79, DEC-464 and DEC-473. Taken here: [DEC-493](../decisions/DEC-493.md).

## Scope

- **Reference cases that must move from pending to passing:** none.
- **Invariants touched:** none.
- **Crates in scope:** `xtask`; the `python/mandate_tools` workspace member.
- **Crates out of scope:** everything under `reference/` (protected; no change), every
  `crates/` member.
- **New dependencies allowed:** none.
- **Safety-critical:** no.
- **Size budget:** under 200 non-generated lines.

## Commands

```bash
cargo xtask check
time cargo xtask ci reference
cd python && uv run --locked pytest -q mandate_tools/tests/test_mutation_anchors.py
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

Met: the anchor check cannot be a flag of `mutants.py`, since `reference/` may not ship with
`xtask` code (ES-22); DEC-493 item 3 puts it in `python/mandate_tools/`.

## Definition of done

- [x] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first; the anchor check's tests pass on the committed reference, name a planted
      stale anchor, and refuse a table-less `mutants.py`.
- [x] New state changes emit journal events (none).
- [x] Docs updated where behavior, interfaces, or decisions changed (feature map, backlog,
      DEC-493, the pytest's docstring).
- [x] `cargo xtask check` is green (paste the summary in the PR).
- [x] The PR description is complete (see the PR template).
