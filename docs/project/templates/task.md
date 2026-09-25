# Task: <story ID> <title>

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** E?-? (link to the [backlog](../06-backlog-v1.md))
- **Acceptance criteria (verbatim):**
- **PRD / HLD / spec anchors:**
- **Decisions that apply (DEC-NN):**

## Scope

- **Reference cases that must move from pending to passing:** (case IDs)
- **Invariants touched:** (MI-n, I-n)
- **Crates in scope:**
- **Crates out of scope:**
- **New dependencies allowed:** none
- **Safety-critical:** yes / no. If yes, this task is the **tests PR** or the **implementation PR**
  of the two-PR flow; name which.
- **Size budget:** (non-generated lines)

## Commands

```bash
cargo xtask check
cargo nextest run -p <crate> <filter>
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
- [ ] New state changes emit journal events.
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
