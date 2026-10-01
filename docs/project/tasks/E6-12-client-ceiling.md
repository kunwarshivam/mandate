# Task: E6-12 the client ceiling

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Stream H, claim [#125](https://github.com/kunwarshivam/mandate/issues/125); readings in
DEC-262.

## Story

- **Story:** E6-12 ([backlog](../06-backlog-v1.md)), the client-ceiling half. The per-client ask
  budget waits on its spec change (MI-33, DEC-251) and is not in this task.
- **Acceptance criteria (verbatim):** `requested_by` is set from the authenticated channel and
  journaled in `DecisionMade`; a client-requested opening is `ask` under every `auto` rule, `auto`
  default, live delegation, and `auto` admission, and a `deny` still denies; owner and agent requests
  decide as before; past 10 client-requested asks per client per risk day (the owner may lower it),
  further asks from that client are suppressed as `client_budget` and journaled, never notified, on
  top of §6.4's per-agent budget of 10; risk-limit alerts are never capped.
- **PRD / HLD / spec anchors:** [mandate spec §6.2](../../specs/mandate.md#62-evaluation) step 5a,
  MI-30, §6.4's `decided_by`; [ADR-0003](../../adr/0003-earned-autonomy.md) parts 2, 3 and 10.
- **Decisions that apply (DEC-NN):** DEC-130, DEC-162, DEC-250 (the builder and its harness);
  DEC-180, DEC-181 (delegations), DEC-185 (the client ceiling), DEC-197 (guardrails); DEC-252
  (`DecisionMade`'s `requested_by` and `decided_by`); DEC-262 (this task's readings).

## Scope

- **Reference cases that must move from pending to passing:** none. No mandate case states
  `requested_by`; the client ceiling is pinned by `mandate-builder`'s own tests (DEC-262 item 7).
- **Invariants touched:** MI-30; MI-1 and MI-17 must still hold (rules 2 and 13: exits are never
  narrowed; the admission ceiling still only tightens).
- **Crates in scope:** `mandate-builder` (`autonomy.rs`, one line of `builder.rs`); the family-A
  and family-N harness modules of `mandate-refcases`, which build an `ActionContext` and now state
  `requested_by: agent`.
- **Crates out of scope:** `mandate-runtime` and the MCP server (stamping `requested_by` from the
  channel and journaling it in `DecisionMade`), `mandate-approval` (the per-client ask budget),
  `mandate-spec` (delegations, E8-8).
- **New dependencies allowed:** none.
- **Safety-critical:** yes (autonomy policy). DEC-77 sequence: the tests PR (this brief, the
  `client_ceiling` stub returning `BuilderError::Unimplemented`, pending tests), then the
  implementation PR (`tests/` only loses `#[ignore = "pending E6-12"]` lines), then a status PR only
  if a reference case moves.
- **Size budget:** under 100 non-test src lines across both PRs.

## Interface

- `ActionContext` gains `requested_by: RequestedBy` (`Agent`, `Owner`, `Client`), required, with no
  default. It is not a §6.3 field and no rule reads it.
- `DecidedBy` gains `ClientCeiling`, labelled `client_ceiling`.
- `classify` applies step 5a last: for a client, the stricter of the decision so far and ASK, named
  `client_ceiling` only when that changed the decision. `decide` reaches it through `classify`.
- `propose` stamps `RequestedBy::Agent` on every buy it proposes.

## Invariants and their tests

| Invariant | Tests |
|---|---|
| A client's `open` or `increase` is never AUTO, under any rule, default, or admission setting | `hand::a_client_opening_under_an_auto_rule_is_asked_by_the_client_ceiling`, `hand::a_client_opening_under_an_auto_default_is_asked`, `hand::a_client_admission_under_an_auto_admission_setting_is_asked`, `properties::a_client_opening_is_never_auto_and_every_other_request_decides_as_before`, `properties::every_rule_default_admission_and_requester_obeys_the_client_ceiling` |
| A `deny` still denies, from a rule or the admission ceiling | `hand::a_deny_still_denies_a_client_request`, both properties |
| The ceiling names itself only when it changed the decision | `hand::an_ask_reached_before_the_client_ceiling_keeps_its_source`, the sweep |
| Owner and agent requests decide as before | `hand::owner_and_agent_requests_decide_as_before`, both properties |
| A ceiling ASK carries §6.4's approval and approver count | `hand::a_client_ask_above_the_threshold_needs_two_approvers` |
| Exits are never narrowed (rules 2 and 13) | `hand::a_client_exit_is_still_auto_by_the_builtin`, both properties |
| Through `decide`: a deny skips, a defer defers, an allow asks | `hand::decide_asks_a_client_buy_the_gate_allows_and_skips_one_it_denies`, `properties::no_client_buy_reaches_auto_through_decide` |
| `propose` is the agent's own request | `hand::a_proposed_buy_is_the_order_builders_own_request` (live) |

The property's oracle is `properties.rs`'s naive rule walk, extended by step 5a, and the sweep's is a
third formulation that asks which steps can deny and which can ask rather than ranking decisions.
Neither calls the crate's `stricter`.

## Planted bugs

| Plant | Caught by |
|---|---|
| P1 the ceiling is skipped | all twelve pending tests |
| P2 the ceiling turns a deny into an ask | `a_deny_still_denies_a_client_request`, `an_ask_reached_before_...`, all three properties |
| P3 the ceiling reaches owner requests | `owner_and_agent_requests_decide_as_before`, all three properties |
| P4 the client ceiling runs before the admission ceiling | `an_ask_reached_before_...`, all three properties |
| P5 the ceiling reads the rules' result and drops the admission ceiling's | `a_deny_still_denies_a_client_request`, `an_ask_reached_before_...`, all three properties |
| P6 the ceiling narrows a client's exit | `a_client_exit_is_still_auto_by_the_builtin`, the property and the sweep |
| P7 a ceiling ask carries no approval | `a_client_ask_above_the_threshold_...`, `a_client_opening_under_an_auto_rule_...`, `decide_asks_...`, the property and the sweep |
| P8 the ceiling keeps the rule's label | seven hand tests and all three properties |
| P9 the ceiling relabels an ask it did not raise | `a_deny_still_denies_a_client_request`, `an_ask_reached_before_...`, all three properties |
| P10 `decide` drops the requester | `decide_asks_...`, `no_client_buy_reaches_auto_through_decide` |
| P11 `propose` stamps another requester | `a_proposed_buy_is_the_order_builders_own_request` |

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-builder --run-ignored all
cargo xtask ci pending
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

None was met. The readings taken are DEC-262's; each only tightens or adds no risk (DEC-176).

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails.
- [ ] Tests came first; each touched invariant has a property test whose oracle is independent and
      was shown to fail on a seeded bug.
- [ ] New state changes emit journal events (none here: `classify` is pure; journaling
      `requested_by` is the runtime's, DEC-262 item 6).
- [ ] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
