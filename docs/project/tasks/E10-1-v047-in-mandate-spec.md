# Task: E10-1 V-047 in `mandate-spec` (DEC-411's code)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E10-1 ([backlog](../06-backlog-v1.md)), the part owed by [DEC-411](../decisions/DEC-411.md)
- **Acceptance criteria (verbatim):** "compiled mandates validate against the mandate spec (schema,
  V-rules, policy hierarchy; reference cases MC-S, MC-V, and MC-P pass)". Owed on the row:
  "`mandate-spec`'s `ValidationContext` gains the effective `independent_approval_required`, V-047
  refuses it in a workspace of fewer than two users, and the harness reads the cases' new context
  member, so MC-V69 to MC-V71 pass. V-047 also refuses again when a version is applied, as V-002
  does, and a case covers a second user deactivated between confirmation and application."
- **PRD / HLD / spec anchors:** PRD v1 E10-1; HLD, the mandate service's validation; mandate spec
  §4 (what validation reads), §4.1 V-002 and V-047, §4.3 (`independent_approval_required`).
- **Decisions that apply (DEC-NN):** DEC-411, DEC-428 (this task's readings), DEC-77, DEC-128,
  DEC-169, DEC-176.

## Scope

- **Reference cases that must move from pending to passing:** MC-V69, MC-V70, MC-V71 (in the
  status PR, after the implementation PR).
- **Invariants touched:** none of MI-n; §4.1's V-047 and V-002 at application.
- **Crates in scope:** `mandate-spec` (validate, context); `mandate-refcases` (the semantic
  context's new member, in the implementation PR).
- **Crates out of scope:** every other; `docs/specs/`, `reference/`, `fixtures/` (ES-22).
- **New dependencies allowed:** none
- **Safety-critical:** yes. Three PRs: the **tests PR** (the context members, `Violation::V047`, the
  stub `recheck_at_application`, `validate` refusing the policy as `unimplemented`, pending tests in
  `tests/independence.rs`), the **implementation PR** (V-047 at validation and at application, the
  harness member; only the `#[ignore]` lines leave `tests/`), and the **status PR** (MC-V69 to
  MC-V71 passing).
- **Size budget:** about 500 non-generated lines for the tests PR, 100 for the implementation PR.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-spec --run-ignored all -E 'binary(independence)'
cargo xtask ci pending
MANDATE_BASE_REF=origin/main cargo xtask ci mutants
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision, including any exemption from V-047 for a
  reducing or neutral version (DEC-411 item 6 is the founder's).

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none: validation records nothing).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
