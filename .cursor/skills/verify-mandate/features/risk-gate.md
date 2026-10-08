# Risk gate

Planned by [the E6-3 task brief](../../../../docs/project/tasks/E6-3-risk-gate.md) and DEC-129. The
implementation PRs fill the crate in story by story: E6-3 has landed `evaluate` and `agent_flatten`,
E6-9 check 3's halt and no market orders under a presumed halt (a market exit is re-priced),
E6-7 check 2's eligibility floor, E6-6 `session_at`, check 3's sessions, the rest of check 4,
check 7's buying power and check 8's `legacy_pdt` budget with its account-wide ledger fold, and E6-8 check 5's mark and collar,
check 6's conduct controls, the pacing of an allowed exit, `evaluate_cancel` and `surveillance`
(DEC-163), and E6-10 check 2's USD pairs for crypto, read from
`InstrumentSnapshot::quote_currency` (DEC-254, DEC-255), so every check is whole for both asset
classes. Were a check still owed, the gate would fail closed for adding risk (DEC-129 item 29),
while a reducing purpose passes it.

- **Spec:** `docs/specs/trading-domain.md` §9 (§9.1 the evaluation order and reason codes,
  §9.2 the day-trading regime, §9.3 leverage and short sales, §9.4 sessions, §9.5
  buying power, §9.6 market-conduct controls), §3.1 to §3.3 (instrument fields, the
  eligibility floor, concentration), §4.3 and §4.4 (sessions, auction windows, halts),
  §5.1 to §5.6 (the v1 order policy, the constraints before submission, the kill switch,
  exit pricing), §7.2 to §7.4 (buying power, account restrictions, agent modes), §8.2
  (risk marks); `docs/specs/mandate.md` §1.1 (MI-1 to MI-20), §2.3 (the working universe),
  §5.3, §5.5, §5.9; backlog E6-3, E6-4, E6-6 to E6-10.
- **Code:** `mandate-risk`: `crates/mandate-risk/src/lib.rs` (the gate's inputs, the eight §9.1
  checks as `Check`, the four verdicts, `ReasonCode` with the registered spelling of each, `Origin`
  and the `Purpose` it maps to, `GateError`, and the signatures of `evaluate`, `evaluate_cancel`,
  `assign_purpose`, `session_at`, `size_factor`, `trim_proposals`, `agent_flatten`, `fold_day_trades` and
  `surveillance`), `crates/mandate-risk/src/gate.rs` (`evaluate`: the eight checks in order,
  purpose assignment, check 1 whole, the working universe, §5.3 rules 3 and 9, §5.1's limit-only
  openings, the re-pricing of a market exit, and the fail-closed
  refusal of an opening while a check is owed), `crates/mandate-risk/src/limits.rs` (the §5.3
  mandate limits: concentration, order size, the re-entry cooldown, orders per day, and gross
  exposure with the account's own 1×),
  `crates/mandate-risk/src/flatten.rs` (`agent_flatten`, the agent-scoped kill switch's plan),
  `crates/mandate-risk/src/floor.rs` (check 2's eligibility floor, trading spec §3.2),
  `crates/mandate-risk/src/session.rs` (`session_at` from the committed calendar and check 3's
  session and auction-window rules), `crates/mandate-risk/src/account_rules.rs` (§5.3 rules 2 and
  4 to 8, buying power with the fee reservation, and the `legacy_pdt` day-trade budget),
  `crates/mandate-risk/src/daytrades.rs` (`fold_day_trades`, §9.2's `legacy_pdt` ledger folded
  account-wide from every agent's fills, DEC-259, with its in-module hand tests and a running-total
  oracle property),
  `crates/mandate-risk/src/conduct.rs` (check 5's fresh quote and collar, check 6's conduct
  controls, the collar, participation and close-window pacing of an allowed exit, a
  discretionary exit whose collar cannot be computed routed whole by the other controls alone
  (E6-6, DEC-327, DEC-383), and `evaluate_cancel`'s minimum resting time, trading spec §8.2 and
  §9.6),
  `crates/mandate-risk/src/trim.rs` (`trim_proposals`, mandate §5.5's `trim_to_target` under
  DEC-65's guards, DEC-399, with in-module boundary tests),
  `crates/mandate-risk/src/surveillance.rs` (§9.6's daily surveillance report: figures and flagged
  thresholds, concentration a figure only, no judgement), `crates/mandate-risk/src/spec_types.rs` (the stream-F shapes this crate needs
  before `mandate-spec` and `mandate-domain` exist, in the names DEC-128 item 21 fixes; the first
  implementation PR after stream F's tests PR deletes it). It reads `mandate-accounting`'s
  `AccountType`, `AssetClass` and `Side` and changes neither them nor `mandate-time`.
- **Tests:** `crates/mandate-risk/tests/hand.rs` (every MC-G and MC-F figure recomputed from the
  spec, the mode rule, the `Unknown`-order rule, the account states, the eligibility floor, and a
  check that every reason code the gate can emit is registered in the founder-owned case file),
  `crates/mandate-risk/tests/properties.rs` (one property per invariant and per "never" or "always"
  in §9, including MI-1 scoped to its own words, the mode rule, MI-8, a shadow-ledger sequence
  property, and `an_exit_over_extreme_figures_is_still_routed`), `crates/mandate-risk/tests/usd_pairs.rs` (§3.2 item 7's USD pairs for crypto, E6-10:
  a non-USD or unstated pair denied at check 2, a USD pair passing, check 2 whole for crypto, an
  exit in any pair and a US equity never judged by the rule, and a property whose oracle is
  `opening ∧ crypto ∧ quote ≠ USD`; live since E6-10's implementation, #394),
  `crates/mandate-risk/tests/common/mod.rs` (the fixtures and the independent `i128`
  oracle, which never calls the crate's arithmetic), and the in-module tests in `gate.rs`,
  `conduct.rs` and `surveillance.rs` for the boundaries the files above cannot pin (among them
  E6-6's exit routing, DEC-383: an exit over an uncomputable collar routed at its own limit and
  sliced as an `i128` oracle computes, an opening over one keeping the collar's error, a
  proposal of zero refused before the collar is reached (DEC-401), and only `overflow` and
  `not_positive` skipped). Planted bugs per test: the task
  brief.
- **Reference cases:** `mandate::MC-G01` to `MC-G16` and `MC-F01` to `MC-F04` in
  `fixtures/refcases/mandate.json`, through `crates/mandate-refcases/src/mandate/risk_gate.rs`
  (DEC-178; MC-G13 stays pending on E6-8's checks 5 and 6), with
  `crates/mandate-refcases/tests/mandate_gate_harness.rs` proving the two arms read and compare
  every member; `crates/mandate-risk/tests/refcases.rs` is the crate-local copy it replaces, kept
  until a follow-up deletes it; `trading_domain::RC-09`, `RC-09B`, `RC-15`, `RC-16`, `RC-22`
  and `RC-25` with their variants, and the `propose_order` steps of `RC-03`, `RC-08` and `RC-18`,
  in `fixtures/refcases/trading-domain.json`. The trading-domain harness decides them through
  `evaluate` (E6-9, DEC-199): RC-03's `gate_rejects_zero_crossing_order`, RC-08 and RC-18's
  `generic_cash_account` run; the rest fail naming the stories they still wait for (RC-15 on E7-2,
  E7-3, E7-4 and E7-5).
- **Run:** `cargo nextest run -p mandate-risk`, and
  `cargo test -p mandate-refcases --test refcases -- --include-ignored mandate::MC-G` (and
  `mandate::MC-F`).
