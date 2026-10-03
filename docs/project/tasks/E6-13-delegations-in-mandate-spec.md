# Task: E6-13 Delegations as a document in `mandate-spec`, and DEC-353's §9.2 rule

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Stream H, claim
[#439](https://github.com/kunwarshivam/mandate/issues/439). The scope is the one proposed on the
claim on 2026-10-02 (option 1) after the coordinator ordered "the Rust classifier for DEC-353's
shapes, tests PR first" in the round-2 rulings on
[#471](https://github.com/kunwarshivam/mandate/pull/471) and
[#505](https://github.com/kunwarshivam/mandate/pull/505).

## Story

- **Story:** E6-13's code half for DEC-353 ([backlog](../06-backlog-v1.md), the E6-13 row's
  "Also owed (#444, DEC-353)"). It is also the `mandate-spec` half of E8-8, the delegations story:
  the document, never the runtime lift.
- **Acceptance criteria (verbatim, the backlog row):** "`mandate-spec`'s §9.2 classifier takes
  DEC-353's rule, so MC-J01, MC-J03 and MC-J05 pass, with MC-J06 and MC-J09, which stay reducing.
  Size it with what comes first: `mandate-spec` has no delegations at all. `Autonomy` has no
  `delegations` member and `parse::autonomy`'s member list is closed, so a mandate carrying one is
  refused at parse (`unknown_member`); the type, the parser and §9.2's `autonomy.delegations` row,
  which the Rust classifier also lacks, come before the rule has anything to read."
- **PRD / HLD / spec anchors:** mandate spec §3 (`autonomy.delegations`), §4.1 (V-022, V-023,
  V-041, V-042, V-043), §6.5, §9.2 (the autonomy row and the `autonomy.delegations` row); the
  schema's `$defs/delegation`.
- **Decisions that apply:** DEC-77, DEC-79, DEC-176, DEC-181 (delegations), DEC-188 and DEC-273
  (V-042's review date), DEC-353 (the rule and the carve-out), DEC-404 item 7 (this classifier
  change lands before any path builds a context from a real journal), DEC-420 (this story's
  readings).

## Scope

- **Reference cases that must move from pending to passing:** MC-J01, MC-J03, MC-J05, MC-J06 and
  MC-J09. MC-J02, MC-J04, MC-J07, MC-J08 and MC-J10 already pass and must keep passing. Cases that
  also need tripwires (MC-W) or V-046 (MC-D11, MC-D12) or the runtime lift (MC-D23, MC-D24) stay
  pending.
- **Invariants touched:** MI-11 and MI-29 (the classifier), MI-26 only as far as V-041 makes a
  delegation name an `ask` (the lift itself is E8-8's runtime).
- **Crates in scope:** `mandate-spec` (stream F's crate; the coordinator assigned this work to
  stream H), and the harness rows in `mandate-refcases/status.toml` for the MC-J cases only.
- **Crates out of scope:** the order path (`mandate-builder`, `mandate-risk`), `mandate-journal`,
  the approval card, the MC-U family, tripwires (V-044 and the `autonomy.tripwires` row), V-046.
- **What the story adds to `mandate-spec`:**
  1. `Delegation` and `Lifts` (`default`, or `rule:<id>`) on `Autonomy`, and the parse of
     `$defs/delegation`. The canonical form already comes from the parsed document.
  2. V-041, V-042 (on its delegations-removed basis, with the new version's review date in both,
     DEC-273 and DEC-353 item 4) and V-043, as `Violation` variants in `validate`.
  3. V-022 and V-023 extended to a delegation (`user_entered` and confirmed; its `when` typed).
  4. §9.2's `autonomy.delegations` row (removed or narrowed is reducing; anything else is
     increasing) and DEC-353's two conditions in the autonomy row, reading the new version's
     `lifts`.
- **New dependencies allowed:** none.
- **Safety-critical:** yes (mandate validation and change classification). This brief covers both
  PRs of DEC-77: the **tests PR** (types and stubs, pending tests) and the **implementation PR**,
  then the status PR.
- **Size budget:** about 800 non-generated lines of tests and 600 of implementation.

## The tests PR

- The types, with the parse refusing a document that holds `delegations` as `unimplemented`, as
  E6-14's tests PR did for `review_by`, so it still fails closed.
- The V-codes as variants, unreported until the implementation.
- `change`'s delegation row and DEC-353's conditions as stubs that return
  `SpecError::Unimplemented` whenever either version holds a delegation.
- Pending tests, one per clause:
  - the parse of each member, and a refusal for each schema bound;
  - V-041, V-042 and V-043 at and past each edge;
  - V-022 and V-023 for a delegation;
  - the delegations row for each change kind;
  - each DEC-353 shape and the carve-out, as a property test against an oracle that decides orders
    its own way, shown failing on a seeded bug.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-spec
cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/MC-J/)'
MANDATE_BASE_REF=origin/main cargo xtask ci spec-guard
cargo xtask ci pending
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: a delegation is a document field).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
