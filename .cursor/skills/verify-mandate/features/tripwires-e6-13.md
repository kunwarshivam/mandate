# Tripwires (E6-13)

Planned by [the E6-13 task brief](../../../../docs/project/tasks/E6-13-tripwires.md) and DEC-350 to
DEC-352. The DEC-77 tests PR ([#613](https://github.com/kunwarshivam/mandate/pull/613)) fixed the
public boundary and pending tests. The implementation removes those pending markers; MC-W01 to
MC-W57 remain pending in `status.toml` until the separate status-only PR.

- **Spec:** `docs/specs/mandate.md` MI-3, MI-7, MI-28, MI-31, V-044, §5.2, §5.8 to §5.10,
  §6.1, §6.5, §6.7, §9.2, and §11.
- **Code:** `crates/mandate-spec/src/document.rs` and `document/parse.rs` (the document
  vocabulary and strict parser), `crates/mandate-spec/src/validate.rs` (V-044),
  `crates/mandate-spec/src/change.rs` (§9.2 classification),
  `crates/mandate-executor/src/tripwire.rs` (the pure account-stream fold, metric accumulation,
  firing, latching, ordered journal effects, and step-up acknowledgment), and
  `crates/mandate-spec/src/risk/fold.rs` (the executor projection after the lifetime floor and
  before effective-mode application). The executor receives net realized P&L from
  `mandate-accounting`; it does not recompute cost basis.
- **Tests:** `crates/mandate-spec/tests/tripwires.rs` (parsing, V-044, V-020/V-042 interaction,
  classification, and its independent property oracle),
  `crates/mandate-executor/tests/tripwires.rs` (all three metrics, exact boundaries, version and
  risk-day behavior, ordered opaque effects, latching, acknowledgment refusals, replay, late
  fills, and restart, with independent sequence oracles), and the in-module integration tests in
  `crates/mandate-spec/src/risk/fold.rs` (the projection's restriction, lift, and event order).
- **Reference cases:** `mandate::MC-W01` to `MC-W57` in `fixtures/refcases/mandate.json`, through
  `crates/mandate-refcases/src/mandate/tripwire.rs`; MC-W52 and MC-W57 also run through the
  risk-state adapter in `crates/mandate-refcases/src/mandate.rs`. The harness derives realized P&L
  through `mandate-accounting` and compares every stated step, probe, state, and journal member.
- **Run:** `cargo nextest run -p mandate-spec -p mandate-executor`; all pending family-W cases with
  `cargo nextest run -p mandate-refcases --run-ignored all -E 'test(/MC-W/)'`.
