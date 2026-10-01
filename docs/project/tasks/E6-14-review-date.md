# Task: E6-14 the review date

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Stream H, claim [#125](https://github.com/kunwarshivam/mandate/issues/125); readings in
DEC-271 to DEC-273 (the spec change, [#380](https://github.com/kunwarshivam/mandate/pull/380)) and
DEC-271 item 6 (the Rust side, below).

## Story

- **Story:** E6-14 ([backlog](../06-backlog-v1.md)).
- **Acceptance criteria (verbatim):** past the date, autonomous paths decide `ask`, positions and
  exits are untouched, and re-confirming (a version moving the date later, with step-up) restores
  them.
- **PRD / HLD / spec anchors:** [mandate spec §6.2](../../specs/mandate.md#62-evaluation) step 5b,
  §6.6, §9.2's `autonomy.review_by` row, MI-32; [ADR-0003](../../adr/0003-earned-autonomy.md) part
  10.
- **Decisions that apply (DEC-NN):** DEC-130, DEC-162, DEC-250 (the builder and its harness);
  DEC-185 and DEC-262 (the client ceiling, whose shape step 5b follows); DEC-188 (the review date);
  DEC-271 to DEC-273 (its readings).

## Scope

- **Reference cases that must move from pending to passing:** MC-D02 to MC-D04, MC-D13 to MC-D22,
  and MC-D25 to MC-D27 (the review decisions, the §9.2 row, and the bounds `mandate-spec` already
  checks). The other MC-D cases wait on other stories: MC-D01 and MC-D05 to MC-D12 on V-046, the
  review date's V-020 default and V-042 carry in `mandate-spec`'s validation (stream F, backlog
  E6-14); MC-D23 and MC-D24 on delegations in the Rust order path (E8-8).
- **Invariants touched:** MI-32; MI-1, MI-11, MI-17 and MI-30 must still hold.
- **Crates in scope:** `mandate-builder` (`autonomy.rs`, `builder.rs`); `mandate-spec`
  (`Autonomy.review_by`, its parse, and §9.2's row: the shared-crate touch the claim names); the
  `mandate-refcases` harness (`ActionContext` and `RiskContext` gain `risk_day`, and a `review` arm).
- **Crates out of scope:** `mandate-spec`'s validation (V-046, the V-020 default, the V-042 carry;
  stream F); `mandate-runtime` (deriving `risk_day` from the journaled risk clock when it calls the
  builder, and journaling `review_ceiling` in `DecisionMade`); the journal's rule-7 vectors (their
  own code change, the harness count moving with them).
- **New dependencies allowed:** none.
- **Safety-critical:** yes (autonomy policy, mandate classification). DEC-77 sequence: the tests
  PR (this brief, the stubs, pending tests), then the implementation PR (`tests/` only loses
  `#[ignore = "pending E6-14"]` lines), then the MC-D status PR.
- **Size budget:** under 150 non-test src lines across both PRs.

## Interface

- `mandate_spec::document::Autonomy` gains `review_by: Option<Date>`. In the tests PR the parse
  reads the member through a `review_date` stub returning `ParseError::Unimplemented`, so a
  document with a review date fails closed; the
  implementation reads it as the date it names, refuses a pattern-valid date off the calendar
  rather than reading it as "none", and classifies it by §9.2's own row, leaving it out of the
  autonomy row.
- `ActionContext` and `RiskContext` gain `risk_day: Date`, required with no default, as
  `requested_by` is (DEC-262 item 1). `propose` copies the risk context's day to the buy it
  proposes.
- `DecidedBy` gains `ReviewCeiling`, labelled `review_ceiling`.
- `classify` applies step 5b after step 5a when the policy has a review date: once `risk_day` is
  after it, the stricter of the decision so far and ASK, named `review_ceiling` only when that
  changed the decision. In the tests PR that step is a `review_ceiling` stub returning
  `BuilderError::Unimplemented`, reached only by a policy with a review date, which no parsed mandate
  can have yet.

**DEC-271 item 6, the Rust side** (agent, DEC-176: each reading only tightens or adds no risk):
(a) the risk day is a fact of the classified action, set by the caller from the journaled risk
clock, never computed by the builder from a wall clock; (b) a review date off the calendar is a
parse error, not "no review date", so it can never switch the review off; (c) the family-A and
family-N harness arms state no time, so they refuse a mandate with a review date rather than decide
it on a day nobody stated; (d) until the implementation, the parse's `review_date` stub refuses a document that sets one, and
`change.rs` compares the review date in
the autonomy row, so a version changing it is never read as reducing.

## Invariants and their tests

| Invariant | Tests |
|---|---|
| MI-32: past the review date no `open` or `increase` is AUTO, under any rule, default, admission setting or requester | `hand::an_auto_rule_stands_on_the_review_date_and_asks_the_day_after`, `hand::past_the_review_date_an_auto_default_asks_and_a_deny_still_denies`, `hand::past_the_review_date_an_admitted_first_order_asks_though_admission_is_auto`, `properties::past_the_review_date_no_opening_is_auto_and_before_it_nothing_changes`, `properties::every_rule_default_admission_requester_and_day_obeys_the_review_ceiling` |
| The review date is the last day `auto` stands; before it nothing changes | the first hand test, both properties |
| A `deny` still denies; an ASK keeps its source; the ceiling names itself only when it changed the decision | `hand::past_the_review_date_a_deny_denies_and_an_ask_keeps_its_rule`, `hand::past_the_review_date_a_client_ask_keeps_the_client_ceiling`, the properties |
| A ceiling ASK carries §6.4's approval and approver count | `hand::a_review_ceiling_ask_above_the_threshold_needs_two_approvers` |
| Exits are never narrowed (rules 2 and 13, MI-1) | `hand::past_the_review_date_every_reducing_purpose_is_auto_by_the_builtin` (live), the generated property, the sweep |
| Through `decide`: a deny skips, a defer defers, an allow asks | `hand::decide_asks_a_buy_past_the_review_date_and_skips_one_the_gate_denies`, `properties::no_buy_past_the_review_date_reaches_auto_through_decide` |
| `propose` carries the evaluation's risk day | `hand::a_proposed_buy_carries_the_risk_day_it_was_evaluated_on` (live) |
| §3: the review date parses as the date it names, and a malformed one is refused | `mandate-spec` `review_by::a_review_date_parses_as_the_date_it_names` |
| §9.2's row (DEC-273) and the autonomy row ignoring it | `mandate-spec` `review_by::the_review_date_classifies_by_its_own_row` |

The property's oracle is `properties.rs`'s naive walk extended by step 5b, which compares days by its
own arithmetic (`oracle_day_number`) and never by `Date`'s ordering; the sweep's is a third
formulation that asks which steps can deny and which can ask. Neither calls the crate's `stricter`.
Delegations (§6.2 step 4a) cannot be generated until E8-8 gives `Autonomy` one (DEC-262 item 5);
E8-8's tests PR extends both properties with them.

**Per-test do-nothing bar.** Each pending test fails on the stub as committed, on the identity
(`Ok((decision, by))`), on a constant ASK labelled `review_ceiling`, and, for the two
`mandate-spec` tests, on a parse that reads the member and drops it. The row test fails on that
stub only because §9.2's row reads the parsed `Option<Date>`, not the document's text: a row that
read the text would classify the change from the document and pass while the parse dropped the date
(#389 round 1, minor 1, shown on the implementation PR).

## Planted bugs

| Plant | Caught by |
|---|---|
| P1 the ceiling is skipped | all ten builder pending tests |
| P2 the ceiling applies on the review date itself | the first, second and third hand tests, the `decide` hand test, all three properties |
| P3 the ceiling turns a deny into an ask | the deny hand tests, all three properties |
| P4 the ceiling relabels an ask it did not raise | the deny-and-ask and client hand tests, all three properties |
| P5 the review ceiling runs before the client ceiling | the client hand test, all three properties |
| P6 the review date holds exits | `past_the_review_date_every_reducing_purpose_is_auto_by_the_builtin` (live), the generated property, the sweep |
| P7 the ceiling never reads the action's risk day | all ten builder pending tests |
| P8 `propose` drops the evaluation's risk day | `a_proposed_buy_carries_the_risk_day_it_was_evaluated_on` (live) |
| P9 an off-calendar review date reads as none | `review_by::a_review_date_parses_as_the_date_it_names` |
| P10 a later review date is reducing | `review_by::the_review_date_classifies_by_its_own_row` |
| P11 the autonomy row reads the review date as a change | `review_by::the_review_date_classifies_by_its_own_row` |
| P12 no row: the review date falls to the autonomy row | `review_by::the_review_date_classifies_by_its_own_row` |

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-builder -p mandate-spec --run-ignored all
cargo xtask ci pending
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

None was met. The readings taken are DEC-271 to DEC-273 and DEC-271 item 6.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none here: `classify` and the parse are pure;
      journaling `review_ceiling` in `DecisionMade` is the runtime's).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
