# Research-agent contract, admission, and lineages

Planned by [the stream-J task brief](../../../../docs/project/tasks/M5-J-research-thin-slice.md) and
DEC-132. The crate is **stubs and pending tests** until the E17-3, E17-7, and E17-9 implementation
PRs land: every entry point returns `ResearchError::Unimplemented`, and `cargo xtask ci pending`
proves each pending test fails on them (DEC-110).

- **Spec:** `docs/specs/mandate.md` §1.1 (MI-15 to MI-20), §2.3 (the working universe at runtime),
  §4.3 (the internal research profile and the keys the overlay supplies), §8.1 (the signal model
  contract and the research agent as its one exception), §8.2 (the output shape and freshness), §8.4
  (the research agent: the thesis fields, the cost cap, correlated flow, the stagger, the thin
  slice), §8.5 (the seventeen ordered admission checks), §8.6 (thesis lifetime and revision
  lineages), §11; `docs/specs/trading-domain.md` §3.2 and §7.1 (the eligibility floor and
  instrument-group claims, whose verdicts arrive as facts), §9.6 (the conduct controls the stagger
  sits inside); `docs/specs/journal.md` §9 (`ThesisProposed`, `ThesisRevised`, `UniverseChanged`);
  `docs/adr/0002-autonomous-ideation-and-retail.md`; ADR-0001 ES-02, ES-09, ES-21.
- **Code:** `mandate-research`: `crates/mandate-research/src/lib.rs` (the thesis and its platform facts as typed data,
  `checks` and `admit` for the §8.5 decision, `fold_theses` for the §8.6 lineages, `expire_theses`
  for the three removals, `stagger_offset`, `stagger_release_at` and `next_proposal_at` for §8.4's
  timing, and the `ResearchEvent` entries the crate produces),
  `crates/mandate-research/src/score.rs` (E17-8's `evaluate`, the pure forward-paper scorer) and
  `crates/mandate-research/src/spec_types.rs` (the narrow stream-F views the first implementation PR
  after F's tests PR deletes). It **never calls a model:** a model output arrives as a typed value,
  and the platform boundary that produces it is E17-2's shell story. The spike that found the shape
  is `python/research_spike/` (E17-0, a spike, not product code).
- **Tests:** `crates/mandate-research/tests/admission.rs` (one case per §8.5 check, plus MC-N01,
  MC-N02, MC-N08, MC-N14, MC-N16, MC-N25 and MC-N26 by hand),
  `crates/mandate-research/tests/lineage.rs` (MC-N17 to MC-N19, MC-N24, MC-N27, MC-N28),
  `crates/mandate-research/tests/expiry.rs` (MC-N20 to MC-N22 and the horizon boundary),
  `crates/mandate-research/tests/stagger.rs` (MC-N23's three offsets and the anchor rules, with the
  least-significant-first reduction oracle),
  `crates/mandate-research/tests/properties.rs` (the four oracles: the universe replayed from the
  emitted events, the failing-check set computed unordered, an independent lineage counter, and the
  oracle self-checks that fail on a seeded bug),
  `crates/mandate-research/tests/score.rs` (E17-8's forward-paper scoring: hand-calculated cases,
  the boundary rules, and the empty scoreable set's named refusal `empty_scoreable_set` at a
  registered minimum of zero, DEC-335 and DEC-336, with its branch-counting property),
  `crates/mandate-research/tests/refcases.rs` (25 of the 28 family-N cases loaded from
  `fixtures/refcases/mandate.json` rather than typed out; the three that state `first_order_autonomy`
  wait for stream H's `classify`), and `crates/mandate-research/tests/rules.rs` (the rule logic this
  crate carries live, pinned unignored so `cargo mutants` reaches it). Planted bugs per test: the task
  brief.
- **Reference cases:** `fixtures/refcases/mandate.json` family N (28 cases: admission, lineage,
  thesis expiry, stagger), through `crates/mandate-refcases/src/mandate/research.rs` in the
  `mandate` suite (DEC-154). All four kinds are interpreted, and MC-N01, MC-N14 and MC-N26 decide
  their first order through `mandate-builder`'s `classify` (DEC-179), so all 28 run; the status rows
  of those three move in a status-only PR. The module's in-module tests doctor every expected member
  of every interpreted case and require it to fail, and read every first-order fact back from its
  own §6.3 field.
- **Run:** `cargo nextest run -p mandate-research` and
  `cargo test -p mandate-refcases -- --include-ignored mandate::MC-N`.
