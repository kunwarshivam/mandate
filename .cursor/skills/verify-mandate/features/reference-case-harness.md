# Reference-case harness

- **Spec:** ADR-0001 ES-11; DEC-77 (pending and passing cases).
- **Code:** `mandate-refcases`: `crates/mandate-refcases/src/journal.rs` (one interpretation per
  prose case), `crates/mandate-refcases/src/trading_domain.rs` (fills, marks, fee charges,
  settlement, corporate actions, dividends, cash-in-lieu postings, the account type, and buying
  power; every other step type and expectation key fails as "not interpreted until" its owning
  story), `crates/mandate-refcases/src/trading_domain/gate.rs` (E6-9's gate driver, DEC-199:
  `propose_order` steps and the `decision` expectation through `mandate_risk::evaluate`, and the
  account's §7.3 status from `initial.account` and `broker_account_update`, `crypto_status` as
  check 1's `crypto_active` with only an exact `ACTIVE` active and a detected inactivity never
  lifted, and a short refused by the day-trade fold (E6-10, DEC-314, DEC-315, DEC-395); its
  in-module tests run RC-15's steps 3 and 4 in a `closing_only` account),
  `crates/mandate-refcases/tests/refcases.rs`, `crates/mandate-refcases/tests/harness.rs`
  (the harness reads the account type and checks `buying_power`: RC-08 and RC-18's cash variant
  without their gate step), `crates/mandate-refcases/tests/trading_domain_gate_harness.rs` (the gate
  driver reads every key it claims, an edited decision fails, and what it cannot read is refused;
  RC-15's `status_not_active` without its later stories' expectations passes; RC-09's crypto
  opening denied `crypto_account_inactive` by an initial or updated `crypto_status`, and never
  lifted by a later `ACTIVE`),
  `crates/mandate-refcases/src/mandate/research.rs` (family N of the
  mandate suite, through `mandate-research`), `crates/mandate-refcases/src/mandate/autonomy.rs`
  (family A, through `mandate-builder`'s `classify`), `crates/mandate-refcases/src/mandate/order_builder.rs`
  (family B, through `mandate-builder`'s `propose` and `decide` and `mandate-risk`'s `evaluate`),
  `crates/mandate-refcases/src/mandate/risk_gate.rs` (families G and F, through `mandate-risk`,
  with `crates/mandate-refcases/tests/mandate_gate_harness.rs`; DEC-178),
  `crates/mandate-refcases/status.toml` (founder-owned).
- **Suites:** `fixtures/refcases/journal.json` (46 cases, all passing),
  `fixtures/refcases/trading-domain.json` (accounting cases from E3-1 and E3-2; the rest
  pending their stories), `fixtures/refcases/mandate.json` (families S, V, P, C, R, T, and L
  harnessed by stream F, N by stream J, A and B by stream H, and G and F by stream G; a case whose
  own story is pending fails naming it).
- **Run:** `cargo nextest run -p mandate-refcases`; pending cases with
  `cargo test -p mandate-refcases -- --include-ignored`; the mutation gate on a harness change,
  `MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants` (DEC-253: the
  crate is `safety_critical = true`, so the gate covers it although it is a `tool` crate).
