# Mandate document, validation, risk state, and change classification

Planned by [the stream-F task brief](../../../../docs/project/tasks/M5-F-mandate-spec.md) and DEC-128.
The crates exist; the rules above `SchemaDec` are stubs until their implementation PRs (DEC-77).

- **Spec:** `docs/specs/mandate.md` §1.1 (MI-1 to MI-20), §2 (lifecycle, provenance, the working
  universe), §3 (structure and goals), §4 (validation, warnings, the policy hierarchy), §5 (the risk
  state, the risk day, breach confirmation, the lifetime floor, restrictions and the effective mode),
  §6.3 (the condition language), §7 (platform defaults and proposals), §9 (the version hash and
  change classification), §11 (the reference cases); `docs/specs/trading-domain.md` §8.1 and §8.2
  (cost-basis reduction, risk marks); `docs/specs/journal.md` §4 (the canonical form the version
  hashes) and §9; ADR-0001 ES-02, ES-04, ES-09, ES-21, ES-22.
- **Code:** `mandate-spec`: `crates/mandate-spec/src/dec.rs` (`SchemaDec`, a decimal checked against
  its field's whole schema `$def` — the pattern and, for `decimal`, the `-0` exclusion — so the text is
  already the journal form and a version hash reproduces the confirmed document),
  `crates/mandate-spec/src/document.rs` (the hashed envelope document and the strict parse),
  `crates/mandate-spec/src/validate.rs` (the V-rules, the warnings, the closed platform-default list,
  `ValidatedMandate`), `crates/mandate-spec/src/policy.rs` (the hierarchy and its runtime overlay),
  `crates/mandate-spec/src/risk.rs` (the risk-state types, breach confirmation, risk days),
  `crates/mandate-spec/src/risk/limits.rs` (the §5.2 and §5.6 comparisons),
  `crates/mandate-spec/src/risk/fold.rs` (the fold over marks, fills, clock ticks, universe changes,
  staleness, a `profit_stop` goal, and the stepwise lift after a reset, slices R2 to R4, DEC-167
  items 5 to 8), `crates/mandate-spec/src/risk/fold/daily.rs` (the daily loss over risk days: the
  rollover, a breach carried over it, the renewal, and the lift),
  `crates/mandate-spec/src/risk/fold/owner.rs` (acknowledgments, allocation changes, floor
  loosening, goal completion, and retirement, slice R4),
  `crates/mandate-spec/src/goal.rs`, `crates/mandate-spec/src/change.rs` (the version and §9.2
  classification), `crates/mandate-spec/src/condition.rs` (the §6.3 language, owned here and nowhere
  else), `crates/mandate-spec/src/context.rs` (`ValidationContext::from_journal`, the fold over
  journaled facts with refusing defaults, DEC-169); `mandate-domain`: `crates/mandate-domain/src/lib.rs` (the vocabulary `mandate-risk`,
  `mandate-builder`, and the research agent share). The exact arithmetic stays in `mandate-num`
  (ES-04).
- **Tests:** `crates/mandate-spec/tests/dec.rs` (live: every grammar's own values, the four things
  `DecStr` normalises pinned as rejections, an integer oracle for the ordering, and the normal-form
  identity the version hash rests on), `crates/mandate-spec/tests/document.rs` (the code and pointer
  each rejection carries), `crates/mandate-spec/tests/validate.rs` (the closed §7 list, the provenance
  rules, the confirmation screen's four figures), `crates/mandate-spec/tests/policy.rs` (the nearest
  broken ancestor, each key kind, the absence asymmetry), `crates/mandate-spec/tests/policy_document.rs`
  (a `policy.schema.json` document and a `policy_set` object read strictly, DEC-484 item 4),
  `crates/mandate-spec/tests/risk_day.rs` (the year tiled without gap or overlap),
  `crates/mandate-spec/tests/goal.rs` (each §3.1 "done when" row, and a `profit_stop` left to the risk
  state), `crates/mandate-spec/tests/risk.rs` (the §5 fold: the ladder and its hysteresis boundary,
  breach confirmation either side of the window, the two-quote hard trigger, the rollover, the daily
  lift and its renewal, acknowledgment and the stepwise lift, the floor and its loosening, the loss
  carry, allocation scaling, session marks and staleness, and eleven properties whose oracles are an
  `i128` accumulator, an interval scan for breach time, and a second reader of the journal),
  `crates/mandate-spec/tests/context.rs` (the context from journaled facts: each field's source and
  refusing default, the base refused without any one required fact, and a property against an oracle
  that reads the journal backwards),
  `crates/mandate-spec/tests/common/mod.rs` (a mandate as a canonical value, built by hand);
  `crates/mandate-spec/tests/change.rs` (§9.2 row by row in both directions, the DEC-121 pinning
  switch as a whole and each way it fails to be one, the autonomy shapes, the literal version-vector
  digest, and four properties: step-up exactly when some path increases risk, the join over changed
  paths against hand-written per-edit classes, an allocation-only change classified by its direction,
  and MI-11 against a first-match evaluator of the test's own; DEC-172),
  `crates/mandate-spec/src/change/tests.rs` (the `not_in` shapes no other test reaches, DEC-172
  item 13); `crates/mandate-spec/src/risk/limits.rs`'s tests (the comparisons against an integer
  oracle, the exact set of limits they read, and the profit stop's level);
  `crates/mandate-spec/src/risk/fold/tests.rs` (the fold's edges: the lift delay's boundary,
  restart, and a clock that over-reports, severe rungs that a receding drawdown does not lift, the
  whole-second monotone risk clock, staleness, the crypto and equity clocks, the floor's carry,
  fills, the daily action, a breach carried over the rollover, the renewal, the daily hard wait
  across midnight, the profit stop, and a lift property whose oracle is a run rule);
  `crates/mandate-spec/src/risk/fold/tests/owner.rs` (slice R4's edges, and two properties: an
  allocation change never triggers or lifts a limit, against the same walk with a tick in its place
  and §5.1 recomputed on `i128` counts; and the ladder is monotone, against the latched rungs
  rebuilt from the journal); `crates/mandate-num/src/sizing.rs`'s tests (`UsdExact::quotient`);
  `crates/mandate-domain/tests/domain.rs` (live). Planted bugs per test: the task brief and the E10-3
  tests and implementation PRs.
- **Reference cases:** `fixtures/refcases/mandate.json` families S, V, P, C, R, T, and L (202 cases),
  through `crates/mandate-refcases/src/mandate.rs`; families G and F are stream G's, through
  `crates/mandate-refcases/src/mandate/risk_gate.rs` (Risk gate, above); families A and B stay
  with stream H and fail as "not interpreted until" their owning story, and family N is stream J's
  (below). A rejection that carries no reason
  fails its case, so the thirty cases expecting `schema_valid: false` cannot pass on a parse that
  refuses everything.
- **Run:** `cargo nextest run -p mandate-spec`, `cargo nextest run -p mandate-domain`, and
  `cargo test -p mandate-refcases -- --include-ignored mandate::`.
