# Task: E6-13 Tripwires

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). Stream H, claim
[#439](https://github.com/kunwarshivam/mandate/issues/439), reassigned to this Cursor session on
2026-10-05. This brief covers the DEC-77 tests PR and the implementation PR; the current change is
only the tests PR.

## Story

- **Story:** E6-13 ([backlog](../06-backlog-v1.md))
- **Acceptance criteria (verbatim):** "a fired tripwire acts at its next evaluation, journals the
  event, alerts with opaque text, and lifts only by the owner's acknowledgment with step-up; adding
  or tightening one applies at once."
- **PRD / HLD / spec anchors:** PRD FR-3.1 (the user-confirmed risk and autonomy envelope), FR-6.1
  (owner autonomy triggers), and FR-6.4 (opaque generic notifications); HLD §3 "The agent spec"
  (the binding confirmed envelope), §6.B "Decision cycle" (autonomy, risk gate, executor, and
  journal order), and §11 "Where the code stands" (risk-engine and execution crate boundaries);
  mandate spec MI-3, MI-7, MI-28, MI-31, V-020, V-042, V-044, §5.2, §5.8 to §5.10, §6.1, §6.5,
  §6.7, §9.2, and §11.
- **Decisions that apply:** DEC-77, DEC-79, DEC-176, DEC-187, DEC-350, DEC-351, DEC-352, DEC-411,
  DEC-438.

## Scope

- **Reference cases that must move from pending to passing:** implementation PR: MC-W01 to MC-W57;
  tests PR: none. Every MC-W status remains pending.
- **Invariants touched:** MI-1, MI-3, MI-7, MI-8, MI-28, MI-31.
- **Crates in scope:** `mandate-spec`, `mandate-executor`, `mandate-refcases`.
- **Crates out of scope:** `mandate-builder` production routing, `mandate-risk` production gate,
  notification delivery, journal schemas, the reference model and generated fixtures.
- **New external dependencies allowed:** none. The executor may use the existing
  workspace-internal `mandate-spec` crate for the canonical tripwire document vocabulary and
  `mandate-approval` for raw step-up evidence and `owner_command(Acknowledge, ...)`.
- **Safety-critical:** yes. This task describes both PRs in the DEC-77 flow. The current change is
  the **tests PR**: public types, fail-closed stubs, and plain pending tests only. The later
  **implementation PR** removes only the `#[ignore = "pending E6-13"]` attributes from these tests
  while adding production logic. A final status-only PR moves MC-W01 to MC-W57 to passing.
- **Size budget:** about 1,400 non-generated lines of tests and 900 lines of implementation.

## Public data shapes

1. `mandate_spec::document::Tripwire` is the document value
   `{ id, metric, threshold, action }`. `TripwireId` has the schema's rule-id grammar;
   `TripwireMetric` is the closed three-value metric list; `TripwireAction` is ordered
   `EndDelegations < ExitsOnly`; `Autonomy::tripwires` is absent-as-empty and at most 20.
2. `mandate_spec::change::classify_tripwires` is the §9.2 row. During the tests PR it returns
   `SpecError::Unimplemented` for every unequal pair and only `Neutral` for an identical pair.
   Parsing any document that states `autonomy.tripwires` returns `ParseError::Unimplemented`, so no
   unchecked tripwire reaches validation or runtime.
3. `mandate_executor::tripwire` owns the pure account-stream fold boundary. `TripwireInput` names
   version application, normal and late fills, risk-day rollover, marks, clock ticks, restart, and
   owner acknowledgment. A fill carries the accounting fold's already-derived net realized P&L and
   instrument; the fold derives first-ever status from its private lifetime fill history and does
   not trust a caller flag or recompute cost basis. `TripwireState` keeps its tripwires, metrics,
   latches, lifetime instruments, risk day, and spent assertion ids private, with no reset or
   construction surface. `TripwireValue` keeps count and USD metrics in distinct units while the
   document threshold remains `SchemaDec`. Acknowledgments carry raw `StepUp`, committed and
   processed risk clocks, environment, and independence evidence; the implementation must call
   `owner_command(Acknowledge, ...)` and spend every supplied assertion id even when refusing it.
   `TripwireSnapshot` and `TripwireEvent` name the externally visible projection and journal
   effects the caller must append before acting. `OwnerAlertSent` exposes only fixed generic text
`tripwire_fired`, not caller-provided text; it exposes only the trigger's per-transition journal
index so the append boundary can bind the opaque event id. `fold` is an E6-13 fail-closed stub.
4. `mandate-refcases` gets a `kind: tripwire` arm whose adapter is an E6-13 fail-closed stub. It
   cannot report any MC-W case passing until it interprets every case, step, probe, state, and
   journal expectation.

## Tests PR

- `mandate-spec` pending tests specify strict parsing, every V-044 boundary, V-020/V-042 interaction,
  every §9.2 change shape matched by id, and a property oracle that classifies random valid lists
  independently.
- `mandate-executor` pending tests specify each metric, exact threshold edges, risk-day and arming
  windows, version behavior, id-order firing, opaque alerts, delegation suspension, mode and exit
  safety, latched allocation blocking, exact missing/stale/reused/method/not-independent
  acknowledgment refusals, assertion spending on refusal, replay, late fills, and restart. Property
  oracles derive first-ever instruments, first-fire position, latch lifetime, and tightening
  monotonicity from generated histories folded through public inputs without calling production
  predicates or constructing private state.
- Live tests pin the closed public vocabularies and prove each stub rejects rather than returning a
  permissive default.
- MC-W01 to MC-W57 remain pending in `mandate-refcases/status.toml`.

## Implementation PR

- Replace the mandate parser, V-044 validator, provenance/V-042 integration, and tripwire
  classifier stubs with the rules in the cited spec.
- Fold §6.7 from account-stream inputs after the lifetime floor, deriving fill net realized P&L
  from the accounting result and preserving journal-before-action order.
- Complete the `kind: tripwire` adapter and extend the existing risk-state adapter for MC-W52 and
  MC-W57 without changing fixture expectations.
- Remove only the pending attributes from the tests PR. Move no statuses until a separate status
  PR.

## Commands

```bash
cargo fmt --all -- --check
cargo nextest run -p mandate-spec
cargo nextest run -p mandate-executor
cargo nextest run -p mandate-spec --run-ignored ignored-only -E 'test(/tripwire/)'
cargo nextest run -p mandate-executor --run-ignored ignored-only -E 'test(/tripwire/)'
cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/MC-W/)'
cargo xtask ci pending
MANDATE_BASE_REF=origin/cursor/e77-paper-send-4832 cargo xtask ci spec-guard
cargo xtask check
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new external dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The tests PR compiles; every pending test fails on a named fail-closed stub; no MC-W status
      moves.
- [ ] The implementation PR makes MC-W01 to MC-W57 pass, and no existing passing case fails.
- [ ] Each touched invariant has a property test whose oracle is independent and is shown to fail
      on a seeded bug.
- [ ] Every firing and lift journals before changing externally visible behavior; alerts contain
      only an opaque id and generic text.
- [ ] Docs are updated where behavior, interfaces, or decisions change.
- [ ] `cargo xtask check` is green for each PR.
- [ ] The PR description cites E6-13, the PRD requirement, and the HLD sections.
