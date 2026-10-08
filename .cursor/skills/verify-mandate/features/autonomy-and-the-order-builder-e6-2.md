# Autonomy and the order builder (E6-2)

Planned by [the E6-2 task brief](../../../../docs/project/tasks/E6-2-autonomy-and-order-builder.md) and
DEC-130. The implementation lands in two slices. Slice 1 (autonomy, DEC-152) implements §6.2 in
`autonomy.rs` and §6.3's `Condition::matches` in `mandate-spec`, and its 40 tests are live: family A
(`MC-A01` to `MC-A16`), 16 hand tests and 8 properties. Slice 2 (the builder) implements §8.1 to
§8.3 in `builder.rs` and the sizing arithmetic in `mandate-num`, and the rest go live: family B (the
28 builder cases), 46 hand tests, 18 properties, and the two `mandate-num` tests. No E6-2 test in
the crate is pending.

- **Spec:** `docs/specs/mandate.md` §6.1 to §6.4 (purposes, the evaluation order, the condition
  language, the approver count and the skip-on-timeout), §8.1 to §8.3 (the signal-model contract,
  output freshness, and `conviction_linear`), §3.1 (`accumulate` and `profit_stop`), §5.2 and §5.3
  (exact comparisons and the limits the proposal is clipped to); `docs/specs/trading-domain.md` §5.1
  and §5.3 (the v1 order policy), §8.2 (the risk mark), §9.1 and §9.6 (the gate's verdicts and the
  pacing of a discretionary exit).
- **Code:** `mandate-builder`: `crates/mandate-builder/src/lib.rs` (the crate's contract and
  `BuilderError`'s refusals with their stable codes),
  `crates/mandate-builder/src/autonomy.rs` (§6.2's order, the built-in AUTO purposes, the first
  match, the default, the admission ceiling, the approver count, and the `Facts` a proposed action
  presents to a §6.3 rule, with in-module tests of the rule walk, the re-check and `decide`),
  `crates/mandate-spec/src/condition.rs` (`Condition::matches`, with in-module tests of every
  operator and combinator), `crates/mandate-builder/src/builder.rs` (§8.1 and §8.2's pinned triple
  and freshness, and §8.3's combine, decide, size, accumulate clips and minimum order, and
  `buy_action`, the classification facts of a buy that `propose` and a caller stating them up front
  share). The exact
  arithmetic is `mandate-num`'s (ES-04): `crates/mandate-num/src/sizing.rs` (`SizeFraction`, `Unit`,
  `Conviction`, `Signed`, the two `weighted_ratio` quotients, and `UsdExact`). The gate's dry-run
  verdict reaches the crate as a value, so `mandate-risk` is not a dependency (DEC-130 item 2), and
  the §6.3 condition tree is `mandate-spec`'s (DEC-128 item 18).
- **Tests:** `crates/mandate-builder/tests/hand.rs` (54 tests: every §6 and §8.3 figure recomputed
  by hand from the rule, the two freshness bounds at their exact instants, the tie-break among
  duplicate outputs, the four clips, the accumulate clips with fees, and the two orderings DEC-130
  item 21 fixes), `crates/mandate-builder/tests/properties.rs` (25 properties against three
  independent oracles: the combine step as `i128` integer arithmetic with its own half-even
  rounding, the sizing chain as rationals compared by cross-multiplication, and a naive rule walk
  that re-reads the list from the start), `crates/mandate-builder/tests/refcases.rs` (the 16 `MC-A`
  and 28 `MC-B` cases loaded from the fixture, one test per case id),
  `crates/mandate-builder/tests/common/mod.rs` (the two reference bases as typed inputs),
  `crates/mandate-builder/tests/buy_action.rs` (the after-values of an opening and an increase by
  hand, the crossed and overnight refusals, and `propose`'s action equal to `buy_action`'s), and the
  two `mandate-num` additions in `crates/mandate-num/tests/num.rs`. Planted bugs per test: the task
  brief and the tests PR's body.
- **Reference cases:** the 16 `mandate::MC-A` cases and the 28 `mandate::MC-B` builder cases other
  than `MC-B17` and `MC-B30` to `MC-B32`, in `fixtures/refcases/mandate.json`. Family A also runs in
  the shared harness, through `crates/mandate-refcases/src/mandate/autonomy.rs` on the parsed
  mandate's own `autonomy` block (DEC-162); its `status.toml` rows move in a status-only PR, since
  the spec guard keeps that file apart from code (ES-22). Family B (all 32 `MC-B` cases) runs in the
  shared harness through `crates/mandate-refcases/src/mandate/order_builder.rs` (DEC-250): `propose`,
  then `mandate_risk::evaluate` on the proposed order as §6.2 step 2's dry run, then `decide` on
  that verdict, with the session and close window from `mandate_risk::session_at`. All 32 pass:
  `MC-B22` after hours and `MC-B23` in the close window since #347 moved their clocks, the three
  crypto buys, `MC-B26` to `MC-B28`, since E6-10's check 2 (#422), and the four `trim_to_target`
  cases since the trim arm compares `mandate_risk::trim_proposals`' trim and `ref.py`'s guards
  (E6-4, DEC-400), and `status.toml` lists all four as passing. Its in-module tests doctor the fixture to prove
  every member is read, a cash fee rate the gate would not reserve is refused, and a `session` or
  `in_close_window` label that contradicts `now` fails the case.
- **Run:** `cargo nextest run -p mandate-builder -p mandate-num`; families A and B in the shared
  harness with `cargo nextest run -p mandate-refcases --run-ignored all mandate::MC-A
  mandate::autonomy mandate::MC-B mandate::order_builder` (the flag runs cases `status.toml` does
  not yet list as passing).
