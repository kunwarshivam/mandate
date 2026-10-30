# earlier-303-599

DEC-303 to DEC-599 — the first era: the journal spec's closed schemas and their registrations,
the executor's order-builder and protection rules, and the harness readings that pinned them.
Most of what these decisions settled now lives in the specs and the code; the summaries below
name only what a reader may still need to know as rules. The decisions are verbatim below.

- Each decision was its own file, reserved by the file itself (DEC-344); DEC-176 lets an agent
  take a reading that only tightens the rule, never one that loosens it toward the code.
- Tripwires are an envelope field over the agent's fills (DEC-350); a fired tripwire is a latched
  limit only the owner's acknowledgment lifts (DEC-351).
- A ladder's remainder counts only what is left and only while something will send it
  (DEC-410); a trim of the whole position is never withheld for the minimum order size
  (DEC-423); the gate's quantity grid is the instrument's own `qty_increment` (DEC-427).
- Protection is sized net of fills the broker reported and the executor has not applied
  (DEC-421), never over shares the owner released and still holds (DEC-362, see DEC-2362).
- A risk-reducing version passes in a lone workspace, but a confirmed mandate beyond a later
  policy denies only actions that add risk (DEC-444, DEC-534).
- ShellCheck and actionlint are part of lint (DEC-329, DEC-330); a saved proptest failure seed
  fails `cargo xtask check` whether tracked or not (DEC-381).
- The app's icons are Pixelarticons at 24px (DEC-478); the UI state system is one verdict chip
  and restrictions in one line (DEC-512).


<a id="dec-304"></a>

## DEC-304: Journal vectors pin §9.2's closed schemas below the payload's top level

| | |
|---|---|
| **Status** | Accepted (agent, DEC-176 (1): a tightening) |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The agent (stream L), under DEC-176, on the coordinator's ruling on [#429](https://github.com/kunwarshivam/mandate/pull/429) round 1 (B1); the follow-up on the rulings on [#445](https://github.com/kunwarshivam/mandate/pull/445) rounds 1 to 3 |
| **Story or claim** | E7-10, DEC-168; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [#430](https://github.com/kunwarshivam/mandate/pull/430); the follow-up vectors PR (`agent/l-e7-10-vectors-2`) |

### Decision

§9.2 already says every schema is closed, but the vectors had pinned it only at the payload's top level. Five invalid drafts close the gap: `step_up` carrying a token, `step_up.assertion_id` not text, `step_up.authenticated_at` off the §4.7 form, `step_up.method` `null`, and a `provenance` entry carrying a quoted span. Each is caught by its own new generator mutant: `open.step_up`, `loose.step_up.assertion_id`, `loose.step_up.authenticated_at`, `loose.step_up.method`, and `open.provenance_entry`. No spec text and no rule changes. It ships apart from E7-10's tests PR because ES-22 keeps vectors and crate code in separate changes; that PR pins the same three behaviours with hand-written Rust tests (DEC-303 item 7)

**Follow-up (#445 rounds 1 to 3).** E7-10's first implementation PR pinned four more behaviours with in-crate Rust tests, and the reviewer asked for each as a vector too, so the generator's mutant suite sees them. Thirteen invalid drafts are added, each caught by its own new generator mutant and by no other draft:

| Draft | Expect | Mutant |
|---|---|---|
| `refused_at_a_date` | `non_canonical` at `payload.effective_at` | `text.payload.effective_at` |
| `version_created_without_version` | `schema` at `payload.mandate_version` | `nullable.payload.mandate_version` |
| `fee_params_unsorted` (rules 20 and 21) | `non_canonical` at `payload.params`, then `schema` there | `order.rule_21_first` |
| `fee_named_and_versioned` | `schema` at `payload.model_id` | `order.rule_21_version_first` |
| `fee_versioned_and_admitting` | `schema` at `payload.model_version` | `order.rule_21_admits_first` |
| `fee_named_with_params` | `schema` at `payload.model_id` | `order.rule_21_params_first` |
| `disclosure_step_up_assertion_null` | `schema` at `payload.step_up.assertion_id` | `nullable.step_up.assertion_id` |
| `disclosure_step_up_time_null` | `schema` at `payload.step_up.authenticated_at` | `nullable.step_up.authenticated_at` |
| `provenance_second_path_null` | `schema` at `payload.provenance[1].path` | `nullable.provenance_entry.path` |
| `provenance_second_entry_quoted_span` | `schema` at `payload.provenance[1].quoted_span` | `open_after_first.provenance_entry` |
| `connection_second_scope_null` | `schema` at `payload.scopes[1]` | `first_only.scopes` |
| `confirmed_second_path_null` | `schema` at `payload.confirmed_paths[1]` | `first_only.confirmed_paths` |
| `model_second_param_null` | `schema` at `payload.params[1]` | `first_only.params` |

- **Drafts that break two rules.** An expectation may carry `also`: the later rules a draft breaks, in report order. The journal reports only the first, which is all the Rust consumers read; the generator checks the whole list.
- **Null list elements.** The ordering rules 17 to 20 read a `null` element as empty text, as the journal's reader does. This matters only under a seeded bug, since the type check refuses the element first.
- **Scope.** No spec text and no rule changes. It ships apart from crate code (ES-22). `mandate-journal`'s in-crate vector tests count their drafts from below, so they need no change.

### Rationale

§9.2 says every schema is closed, and the vectors had shown it only for top-level members. `step_up` is the one nested record that carries authentication evidence, so it is where a token would sit (`AGENTS.md` rules 6 and 7).

### Alternatives considered

Rust tests alone, which leave the generator's mutant suite blind to the nested depths; adding the vectors to #429, which ES-22 forbids beside crate code.

### History

Written as item 11 of DEC-261's decision-log row by #430. Moved here by E7-10's implementation PR, as the coordinator ruled on #429. The row keeps a pointer.

<a id="dec-305"></a>

## DEC-305: The fee-step snapshot's §9.2 payload and its pause, pinned pending against stubs

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: reversible tests-PR shape for a safety-critical writer (DEC-77); item 3 only tightens (it removes a hold on a risk-reducing pause) |
| **Story or claim** | The stream K writer follow-up (DEC-261 item 7), PR #441, branch `agent/k-fee-step-snapshot-tests-flash` |

### Decision

1. The executor's fee step owes an `AccountSnapshotRecorded` whose payload is every member
   journal spec v0.7 §9.2 closes, each once and in §9.2's type: `status` and `crypto_status`
   `text`; `trading_blocked`, `account_blocked`, and `trade_suspended_by_user` `boolean`;
   `multiplier` `integer`; `equity`, `cash`, `buying_power`, `non_marginable_buying_power`, and
   `accrued_fees` `decimal`; `model_cash`, `cash_band`, and `cash_in_band` present as `null`
   (§4.2; rule 24 makes all three `null` together on a fee posting, which compares no cash); and
   the batch's `risk_clock` beside them (DEC-302, DEC-306). The vectors' `snapshot_fees` draft
   (`fixtures/refcases/journal.json`, `control_stream.drafts`), which the reference validator
   accepts, is that payload member for member: the pin builds the broker account from the
   vector's own values and requires the writer to give back the vector's value, in the vector's
   type, for every member. A wrong type, a value carrying anything the broker did not report (an
   account number, for one), a member written twice, or a member missing fails it.
2. The writer lands as `reconcile::fee_step_snapshot_fields`, a stub answering
   `Unimplemented { story: "E7-10" }` until the implementation PR fills it and wires it into
   `fees` (DEC-77); `fees` keeps journaling today's reduced form until then, and
   `AccountSnapshotRecorded` stays unregistered (DEC-261 item 7, DEC-303 item 4).
3. The fee step's pause and alert never wait for the snapshot's own validation. `append` refuses
   a batch whole, so a snapshot draft that validation refuses is left out of the batch, and the
   pause and the alert run all the same (`AGENTS.md` rules 3 and 13, DEC-261 item 7). The tail
   lands as `reconcile::fee_step_pause_and_alert(batch, recorded)`, stubbed the same way. Today
   `fees` journals the snapshot behind a `?`, which is the ordering the pending test holds
   against.
4. **The alert's subject.** The owner is alerted once, under the generic key
   `reconciliation_fees`, and the alert carries nothing but that key and one opaque event id
   (`AGENTS.md` rule 6, DEC-11). When the snapshot recorded, the id is the snapshot's. When it did
   not, the id is the pause's own `AgentModeApplied`, which the same batch journals before the
   alert, so the alert always names an event already journaled (rule 5) and never an id no event
   carries.
5. One pending test pins items 3 and 4 together,
   `the_fee_steps_pause_and_alert_run_whether_or_not_the_snapshot_recorded`, so a stub filled
   with today's behaviour (pause only when the snapshot recorded) fails it. The implementation PR
   deletes the `#[ignore]` lines of this PR's pins and nothing else.

### Rationale

The fee step is the step that pauses every agent and alerts the owner (§11): a snapshot the
schema refuses must never hold that, or a refused writer would silence the one control a fee
difference has. Pinning the payload through the vectors keeps the writer and the reference
validator on one list, and pinning values rather than names is what catches the type slip that
DEC-174 item 5 and DEC-302 were both found from. Naming the pause as the alert's subject keeps
the alert inside rules 5 and 6 without inventing a record.

### Alternatives considered

- Pin the member names only. Rejected (#441 round 1, B1): a writer typing `multiplier` as text,
  or writing every member `null`, passed, and `append` would refuse both.
- Name the pass's `ReconciliationRun` as the subject when the snapshot is refused. Rejected: it is
  journaled after the fee step, so the alert would name an event not yet journaled when the step
  decides it.
- Pin the reduced form and register the schema with a `null`-tolerant reader. Rejected: §9.1's
  absent-member rule is the spec's own words, and a schema that reads absence as `null` is a
  weaker schema, not a reader choice.
- Stub the whole `fees` step. Rejected: live tests drive it, so a stub there would break the
  crate's live suite, and DEC-77's stubs sit beside live behavior, never over it.

<a id="dec-306"></a>

## DEC-306: The executor's risk_clock stamp is a whole-second timestamp, pending against a stub

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: reversible tests-PR shape for a safety-critical writer (DEC-77) |
| **Story or claim** | The stream K writer follow-up (DEC-261 item 7), branch `agent/k-fee-step-snapshot-tests-flash` |

### Decision

1. The account stream's `risk_clock` is written as journal spec §9.2's `risk_clock` type
   (DEC-302): a whole-second §4.7 timestamp, never integer seconds. The vectors pair the two
   forms — `snapshot_fees` carries `2026-09-21T21:00:00.000000000Z`, and their
   `snapshot_risk_clock_as_seconds` draft refuses `1790024400`, the same instant as seconds — so
   one pin reads both halves from the fixture.
2. The stamp lands as `payload::risk_clock_stamp`, a stub answering
   `Unimplemented { story: "E7-10" }` until the implementation PR replaces `batch::journal`'s
   `payload::clock` stamp with it and deletes the pending pin. Until then every account-stream
   draft keeps the integer form the vectors refuse, which is why `AccountSnapshotRecorded` stays
   unregistered (DEC-261 item 7, DEC-303).

### Rationale

`payload::clock` writes the integer form on every account-stream event today, so the fee step's
own snapshot could never commit once the schema registers: the pin holds the form the validator
checks, derived from the validator's own vectors rather than transcribed.

### Alternatives considered

- Pin the form on one event type only (the snapshot). Rejected: the stamp is one function behind
  every account-stream draft, and a pin on one caller would let the others drift.
- Change `clock` in place in this tests PR. Rejected: every live executor test journals through
  it; DEC-77's stubs sit beside live behavior, never over it.

<a id="dec-307"></a>

## DEC-307: IntentReceived is §9.1's nine members, and OrderSubmitted's one nullable member is written as null

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: reversible tests-PR shape for a safety-critical writer (DEC-77). Every member list here is the spec's or the registered schema's, so nothing loosens the spec toward the code |
| **Story or claim** | Stream K's account-stream drafts row (DEC-174 item 5), PR #441, branch `agent/k-fee-step-snapshot-tests-flash` |

### Decision

1. **`IntentReceived` is exactly the nine members journal spec §9.1 closes**, the ones
   `mandate-journal` registers as `INTENT_RECEIVED_V1` and the vectors' chain event at seq 2
   carries: `IntentProposed`'s `instrument_id`, `side`, `type`, `tif`, `qty`, `limit_price`, and
   `purpose`, plus `intent_id` and `agent_id` ("the executor's `IntentReceived` copies the members
   and adds those two"). Never `agent`, `kind`, `instrument`, or `limit`, and nothing else: the
   protective prices `received` writes beside the intent today (`stop`, `take_profit`) are not
   members, and the registered schema refuses that payload at `payload.stop`, so a protected
   opening would never be gated. The writer lands as `intent::intent_received_fields`, a stub
   answering `Unimplemented { story: "E7-10" }`. Its pins drive an unprotected intent and two
   protected ones and require the same nine members each time.
2. **`tif` is the one the agent proposed.** §9.1 has `IntentReceived` copy `IntentProposed`,
   whose action members repeat its `DecisionMade` exactly (rule 10), so the record names the
   proposed order, not the one the submission will carry: a crypto add is submitted limit `ioc`
   where the proposal said `day` (`protection::crypto_add`). The writer takes the proposed value
   as an argument; `IntentHandoff` carries no `tif` today, so the implementation PR brings it to
   `received`. A pin drives `day`, `gtc`, and `ioc` and requires each back.
3. **`OrderSubmitted` writes `limit_price` present as `null` when the order has no limit**, never
   omitted (§4.2, DEC-174 item 5), and that is the only member this decision pins. §9 does not
   close `OrderSubmitted`'s schema, so the reading the registered one enforces governs (the
   coordinator's ruling on #441, round 1): `ORDER_SUBMITTED_V1` and the vectors' seq 4 draft hold
   eight members, `client_order_id`, `attempt`, `instrument_id`, `side`, `type`, `tif`, `qty`, and
   `limit_price`, of which `limit_price` is the one nullable member. A member the schema does not
   list is refused at that member, `null` or not, so the writer never writes `intent_id`,
   `stop_price`, `order_class`, `take_profit`, or `stop` as empty members. The writer lands as
   `intent::order_submitted_optional_fields(request)`, stubbed the same way; its pins drive a
   market order, a limit order, a stop-limit, and a limit order with OCO legs, and require
   `limit_price` alone each time.
4. **Each pinned payload is proven against the real validator.** Every pin writes the writer's
   members into the vectors' own draft for that event and requires `mandate-journal`'s
   `Draft::parse` to accept it, so no pin can be satisfied by a payload `append` refuses.
5. **Not settled here.** The executor's fold reads members of both events that no registered
   schema carries, and `Batch::journal` stamps `risk_clock` on both, which neither schema lists.
   Which of those move to another record and which need a spec change is DEC-360's question
   (Proposed, the founder). Until it is ruled, the implementation PR conforms the two writers
   only as far as these pins reach, and the drafts stay refused at `append` as they are today,
   which adds no risk for an opening (rule 3); DEC-360 records what it holds for an exit.
6. The pending tests of this PR pin the decision; the implementation PR deletes their
   `#[ignore]` lines.

### Rationale

The first draft of this decision kept the protective prices on `IntentReceived` and required five
members as `null` that no clause and no registered schema declares, so its pins could be met only
by payloads `append` refuses (#441 round 1, B2 and B3). Taking every member list from §9.1 and the
registered schemas, and handing each pinned payload to `Draft::parse`, makes that impossible.

### Alternatives considered

- Keep `stop` and `take_profit` on `IntentReceived`. Rejected: §9.1 closes the event at nine
  members, so it would loosen the spec toward the code, which DEC-176 reserves for the founder;
  DEC-360 puts that question to the founder instead.
- Require `OrderSubmitted`'s conditional members as `null`. Rejected: they are not members of the
  registered schema, so the pinned payload is refused at `payload.intent_id`.
- Take `tif` from the submission. Rejected: §9.1 makes `IntentReceived` a copy of the proposal.

<a id="dec-308"></a>

## DEC-308: The refusal's effective_at is the §4.7 timestamp of the judged second, pending against payload::stamp

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: the reversible tests-PR shape for a safety-critical writer (DEC-77), applying the merged §9.2 row to stream I / M7; every reading tightens toward it |
| **Story or claim** | The stream I / M7 writer follow-up (DEC-261 item 7; the backlog's "Journal spec v0.7 §9.2's follow-ups" row), branch `agent/m7-refused-timestamp-tests` |

### Decision

1. The runtime's refusal (`escalation::refused`, DEC-291 item 1's one copy) records
   `effective_at` as journal spec v0.7 §9.2 types it: a §4.7 timestamp — the canonical
   `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ` form — carrying the judged second, the later of the command's
   `submitted_at` and the folded clock. §9.2 supersedes DEC-291 item 1's "same second" for this
   member (DEC-261 item 7): the instant does not change, only its form.
2. The member lands behind `payload::stamp(at, field)`, beside `payload::seconds`, which this
   tests PR ships as a stub answering `RuntimeError::Unimplemented { story: "E8-3" }`. `refused`
   keeps `payload::seconds` until the implementation PR fills the stub and switches its one call
   site. Until then the writer emits the integer form, which the §9.2 schema that E7-10's
   implementation 1/2 registered (#445) refuses at `append` as `non_canonical` at
   `payload.effective_at` — DEC-261 item 7's standing state for this writer, and the defect this
   follow-up's implementation lifts. The marker and the stub cite E8-3, the story whose refusal
   writer this changes, because `cargo xtask ci pending` requires the two to agree; the backlog's
   follow-up row has no story id of its own yet, and the backlog notes that it needs one.
3. The stamp's contract is the vectors': the value for a judged second is exactly that instant's
   §4.7 string — the canonical form `UtcNanos` prints and `UtcNanos::parse` reads back unchanged —
   and never integer risk-clock seconds.

### Rationale

§9.1 ruled every payload instant a timestamp, and §9.2's row closes the refusal's `effective_at`
as one; the runtime's writer is the last one on the integer form, and #445's registration now
refuses its drafts at `append`. The seam beside `seconds` keeps DEC-77's stubs beside live
behavior, never over it (DEC-306's precedent on the executor's stamp), so the crate's live refusal
tests keep running the whole window.

### Alternatives considered

- Change `seconds` in place in this tests PR. Rejected: the runtime writes risk-clock seconds on
  many members, and every live test that reads one would break; DEC-77's stubs sit beside live
  behavior, never over it.
- Leave the writer on the integer form and let the implementation PR change `refused` and its
  tests together. Rejected: the tests PR would pin nothing, and the pinned judged-second property
  would land unproven.
- Type the member as §9.2's `risk_clock` instead of `timestamp`. Rejected: `risk_clock` is the
  account stream's fold clock, which `append` checks never decreases; `effective_at` is the
  command's judged instant, which is not a risk input.

<a id="dec-309"></a>

## DEC-309: The M7 writer follow-up's pending pins, and the two test changes the supersession forces

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: the reversible tests-PR shape for a safety-critical writer (DEC-77); the coordinator named the in-module test change, and the `tests/` one follows from DEC-77 item 2 |
| **Story or claim** | The stream I / M7 writer follow-up (DEC-261 item 7), branch `agent/m7-refused-timestamp-tests` |

### Decision

1. **Five pending tests**, `#[ignore = "pending E8-3"]`, plain `fn`s in `escalation::tests`
   (in-module, where DEC-77's amendment puts a writer's pins): the refusal's `effective_at` is the
   §4.7 timestamp of the judged second for
   `a_refused_resume_stamps_effective_at_the_judged_second`,
   `a_refused_stop_stamps_effective_at_the_judged_second`,
   `a_refusal_stamps_effective_at_for_each_of_its_four_reasons` (`step_up_missing`,
   `step_up_stale`, `step_up_reused` on one paper view, `step_up_method` on a backtest one), and
   `the_refusals_effective_at_is_the_vectors_refused_stop_timestamp`; plus the coordinator-named
   change to `a_stop_processed_late_is_refused_at_the_folded_second`, whose assertion that
   `effective_at` is the integer of the folded second now holds the §4.7 timestamp of that
   instant, through the stub, and which is pending until the writer stamps (DEC-261 item 7,
   DEC-308).
2. **Each pin holds three things**: the stamped value is the §4.7 string of the judged second,
   computed by the test from the instant itself, twice — by civil calendar arithmetic over the
   epoch day, and as `UtcNanos`' canonical form, the two held to agree — an oracle of its own
   rather than the writer's; the journaled refusal carries that value, so a filled stub nobody
   calls fails; and — for the vector pin — the value is the committed `refused_stop` vector's
   `effective_at`, read from `fixtures/refcases/journal.json` together with the integer seconds
   its `refused_at_risk_clock_seconds` draft refuses (whose change is checked to be
   `payload.effective_at`), the two halves held to name one instant and the string held to the
   test's own civil derivation. The
   judged second is pinned where it differs from `submitted_at`: the resume and Stop pins process
   a command late, so a stamp of the submitted second fails them.
3. **`tests/approvals.rs`'s `refused_only` reads `effective_at` as the judged second in either
   form** — the integer seconds DEC-291's writer shipped, or the §4.7 timestamp of the same
   instant — so the implementation PR changes no `tests/` file but the `#[ignore]` deletions
   (DEC-77 item 2). A timestamp is compared as an instant, not a truncated second: one off the
   whole second (`<judged second>.999999999Z`) is not the judged second and fails. The form
   itself is the pending pins', not `refused_only`'s.
4. **Do-nothing discipline.** Each pin must fail a plausible no-op stub — the integer form, the
   seconds as text, `null` — with zero pending tests passing, and every plant the review seeds
   must fail a pin the PR body names.

### Rationale

Two assertions pin the integer form today: the in-module one the coordinator named, and
`refused_only`, which three live `tests/` tests share. Changing both here keeps the
implementation PR to ignore-line deletions while the judged-second property stays live through
`refused_only` the whole window, and the form is pinned pending where it can fail at the stub
(DEC-137).

### Alternatives considered

- Leave `refused_only` on the integer assertion. Rejected: the implementation PR would have to
  edit a `tests/` file, which DEC-77 item 2 forbids.
- Make the three `refused_only` tests pending with it. Rejected: three live tests of a
  safety-critical copy path would stop running for the window, and DEC-77's stubs sit beside live
  behavior, never over it.
- Pin the form only, not the wiring. Rejected: a stub filled with the right string while `refused`
  kept `payload::seconds` would pass, and the writer's drafts would stay refused at `append`.

<a id="dec-310"></a>

## DEC-310: The window's risk story, the implementation PR's one-change scope, and the fixture read

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent under DEC-79 and DEC-176: reversible sequencing and test-only dependency choices for a safety-critical writer (DEC-77) |
| **Story or claim** | The stream I / M7 writer follow-up (DEC-261 item 7), branch `agent/m7-refused-timestamp-tests` |

### Decision

1. **The window adds no risk.** Until the implementation lands, the writer emits the integer form,
   which the §9.2 schema registered in E7-10's implementation 1/2 (#445) refuses at `append` as
   `non_canonical` at `payload.effective_at` — DEC-261 item 7's standing state for this writer
   (`AGENTS.md` rule 3): a refusal copies a judgment that changed no mode and no position, the
   fold reads the event as `copy_of` and never reads the member, and replay is identical under
   both forms. The implementation PR's form fix is what lets the drafts commit again.
2. **The implementation PR's scope is one change**: fill `payload::stamp` with the canonical form
   (`UtcNanos` over the whole second), switch `refused`'s `effective_at` member to it, and delete
   this PR's five `#[ignore = "pending E8-3"]` lines — and in test files nothing else (DEC-77
   item 2, DEC-309 item 3).
3. **The vectors stay the oracle, read not transcribed, and the PR adds no dependency.** The pin
   reads `fixtures/refcases/journal.json`'s `refused_stop` draft through `mandate_canon::parse`,
   the strict JSON reader `mandate-runtime` already depends on, and `tests/approvals.rs` reads
   `UtcNanos` through the crate's existing `mandate-time` dependency, so neither `Cargo.toml`,
   `Cargo.lock`, nor `docs/dependencies.md` changes.

### Rationale

The supersession is a form change on a member the fold never reads, behind a validation that
already refuses the drafts, so the conservative order — pins first, writer second — changes no
outcome while it holds. Reading the committed vectors keeps the writer and the reference validator
on one list, the discipline that found DEC-174 item 5 and DEC-302.

### Alternatives considered

- Switch the writer in this tests PR. Rejected: that is the implementation (DEC-77), and the live
  tests that read the integer form would break with it.
- Transcribe the vector's string into the pin. Rejected: a vector change would pass silently;
  #441's pins read the fixture for the same reason.
- Read the fixture with `serde_json` as a new dev-dependency. Rejected in round 1's ruling: the
  crate's own `mandate_canon::parse` reads the vectors, so the PR needs no dependency at all.
- Land the writer change with the schema registration instead of behind its own tests PR.
  Rejected: #445 carried the registration alone (its item 1), and the backlog row sequences this
  writer's fix behind its own pending pins.

<a id="dec-314"></a>

## DEC-314: The trading-domain day-trade fold refuses a short equity position, loudly, at the count

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | the `opencode` stream G agent under DEC-79 and DEC-176 (the reading only tightens: a fold that would have folded a short as long now refuses) |
| **Story or claim** | stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the backlog row "Refuse a short position in the trading-domain day-trade fold structurally" (#412 review, nit 3); the harness story is E6-10 |

### Decision

1. `trading_domain::gate::Gate::fold` refuses a negative `SignedQty` equity position, and only a
   negative one, with the
   error `` `<instrument>` is held short, which the day-trade fold refuses (its magnitude would
   fold as long and understate the day-trade count) `` — the text
   `a_short_position_is_refused_where_a_day_trade_count_is_expected` pins — instead of folding the
   position's magnitude as shares held long. A flat position, `0`, is not a short: it folds as no
   shares held, as on `main`. The live `only_a_negative_quantity_is_a_short_to_the_fold` pins the
   boundary at `held_overnight`, the function the fold reads each position through, since no case
   can reach it with a flat position: the accounting keeps none (`Account::opening` drops a zero
   entry, and a fill that closes a position removes it), which the live
   `a_flat_initial_position_is_no_position_to_the_fold` pins at the case level (#438 review,
   round 1, blocker 1). The boundary is pinned at `0` and `10`, which are held as their shares,
   and at `-0.0000001` (a ten-millionth of a share), `-0.5`, `-1` and `-10`, which are refused,
   so neither a refusal widened to zero nor one narrowed to a whole share passes (#438 review,
   round 2, blocker 2). Until E6-10 lands, the refused magnitudes are pinned against the stub's
   report, and the implementation PR swaps that expectation for the text above.
2. The refusal is the fold's own: it fires wherever the fold runs, before any count is compared.
   Today only a `day_trade_count` expectation (`Gate::check_day_trade_count`) reaches it with a
   short: a `legacy_pdt` account's `Gate::decide` refuses a short snapshot with its own text
   (`` `<instrument>` is held short, which no v1 gate snapshot holds ``, pinned live by
   `an_unmarked_or_short_position_is_refused_rather_than_valued`) before it folds, so no case
   stages the fold's refusal from `decide` (#438 review, round 1, minor 1).
3. The tests PR carries it as a stub pending E6-10 (the story the harness's `crypto_status` pins
   already name), reporting `GateError::Unimplemented`; the implementation PR puts the refusal in
   place and deletes the `#[ignore]` line.

### Rationale

No v1 case holds a short (`AGENTS.md` rule 12) and `Gate::decide` refuses a short snapshot, but a
`day_trade_count` expectation reaches the fold without that refusal, where folding
`position.qty().abs()` holds the short long and understates the count — the exact defect the
backlog row names.

### Alternatives considered

Folding the magnitude with a fixture-only guard (the defect itself); refusing in
`check_day_trade_count` only (leaves `decide`'s legacy-pdt fold reading the magnitude); refusing
with an unnamed error (not loud, cannot be pinned).

<a id="dec-315"></a>

## DEC-315: The trading-domain harness reads `crypto_status` as check 1's `crypto_active`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | the `opencode` stream G agent under DEC-176 (the reading resolves the gap by the one that adds no risk: an unstated status stays crypto-active exactly as the harness defaults it today, and a stated non-`ACTIVE` status can only refuse) |
| **Story or claim** | stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the backlog row "Read `crypto_status` in the trading-domain harness" (DEC-285 item 5); the harness story is E6-10 |

### Decision

1. `crypto_status` is read from `initial.account` and from `broker_account_update` data as §7.3's
   first row reads it: `ACTIVE` leaves the account crypto-active, any other status denies the
   account's crypto openings at check 1 with `crypto_account_inactive`, and equities are never
   judged by it.
2. A detected inactivity is stored, in the gate's `crypto_active` field that its snapshot hands
   check 1, and a later `ACTIVE` never lifts it in the harness — lifting needs the owner's
   acknowledgment and refresh (E7-5), as with the account's other status fields. The tests PR
   stages the field, the read as a method on the gate, and the snapshot's read of the field, so
   the implementation PR changes only the stub's body (#438 review, round 1, major 2).
3. The three pins of the old "not interpreted until E6-10" answer are dropped; the reading is
   carried as a stub pending E6-10 reporting `GateError::Unimplemented`, and
   `the_initial_crypto_status_is_check_1s_crypto_active`,
   `an_account_update_s_crypto_status_is_check_1s_crypto_active` (item 1) and
   `a_later_active_crypto_status_never_lifts_a_detected_inactivity` (item 2) pin it. Until the
   implementation PR lands, the in-module `an_initial_crypto_status_reports_its_stub_until_e6_10`
   and `an_account_update_s_crypto_status_reports_its_stub_until_e6_10` pin the stub's report live
   in place of the dropped pins; that PR replaces them (DEC-77's 2026-09-27 amendment).

### Rationale

No fixture case states the field, so the harness hands the gate a crypto-active account; the row
asks the harness to read it when a case needs it. The never-lifts reading is the status row's own
(`a_later_active_status_never_lifts_a_detected_restriction`) and is fail-closed (rule 3).

### Alternatives considered

Leaving the field refused (the row's defect); letting an update's `ACTIVE` lift the inactivity
(loosens a restriction no owner action has cleared); making a non-`ACTIVE` status block the whole
account (§7.3 scopes the field to crypto).

<a id="dec-317"></a>

## DEC-317: The MC-E harness, step 2: the `lifecycle` op's runtime driver and its published map

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The agent (`claude-code`, stream M7) under DEC-79 and DEC-176: a harness that drives the real runtime, reads every member, and fails what it cannot interpret adds no risk |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); DEC-292 item 3, DEC-280; mandate spec §6.1, §6.4, §11 |

### Decision

1. **The real runtime, one deployment.** Each `op: lifecycle` case runs `mandate-runtime`'s
   `handle` and `fold` for one deployment. It starts from a clean `ReconciliationRun` folded at
   `start`, then `Input::Started`, which must journal nothing. Every draft is folded back as the
   agent stream's next event. Every owner answer is an `ApprovalResponseSubmitted` that is folded on
   the control stream and then handed in, as the tailer does. Every admission, re-validation,
   timeout and cancellation in a draft is therefore `mandate-runtime`'s and `mandate-approval`'s.
   The driver decides none of them.
2. **The steps.**
   - **`ask`** is a tick at the folded clock. Its plan proposes the bound order once, classified
     `ask` by its `decided_by`, and the dry run allows it. The request's reference mark is folded as
     the account stream's `MarkUpdated` at its `seq`, with earlier seqs repeating its price. A `seq`
     that is not after the stream's head fails.
   - **`tick`** is a tick.
   - **`fold`** folds a `MarkUpdated` of the asked instrument at its unchanged price, carrying the
     event's `clock` as `risk_clock`, and steps nothing (DEC-280 item 2).
   - **`cancel`** hands the input that cancels for its reason. That is a `MandateVersionApplied` or
     an exits-only `AgentModeApplied` on the account stream, or the owner's pause, Stop or agent
     kill switch on the control stream, with fresh `cli_confirm` evidence.
   - **`batch`** folds an exits-only `AgentModeApplied` without handing it, then hands each
     response. The response's own step therefore applies the tightening and cancels before it
     judges (DEC-131 item 25(j)). Only `mode_tightened` is batched, because every other
     cancellation is applied by its own input's step.

   A re-tailed `source` hands the event already folded again and folds nothing. A re-tail with
   other members fails.
3. **`now` reaches the runtime through its ports.**
   - The version, the instrument restriction and the working universe come from the view.
   - The classification and the dry run are the plan's and the gate's answers.
   - The mark is always folded as the account stream's latest `MarkUpdated`, even when it equals the
     last folded one. A mark is never withdrawn. Folding always only adds an input the runtime sees,
     so this reading is taken under DEC-176. It replaces "when it differs from the last folded one",
     which #475's slice 3 removed as unobservable.
   - The mode is folded as an `AgentModeApplied` when it differs from the effective mode, which
     must then read as `now` states.

   In a `batch`, `now` is the state the step starts from.
4. **The map.**
   - A draft's `clock` is the folded risk clock after its step.
   - Instants are whole seconds, `limit_price` is `limit`, and a step-up's `assertion` is
     `assertion_id`.
   - Every other member compares as the runtime's canonical value, which is how the case writes
     it. Decimals compare as their canonical text, so a case that wrote `155.00` would fail
     loudly rather than pass.
   - The case's approval, source and content-hash names are bound one to one to the runtime's ids
     and hash when the runtime first writes them. An unbound name passes as written unless it is a
     bound runtime name.
   - The content hash is treated as a name because the reference model hashes its canonical
     request as a stand-in for the content object (`ref.py`, `escalation_step`). The runtime's own
     `content_hash` is checked to be the SHA-256 of its inline `content`.
   - An `IntentProposed` names its approval through the `ApprovalRevalidated` it is caused by, and
     its mandate version through that approval's `ApprovalRequested`. Journal spec §9.1's closed
     `IntentProposed` carries neither.
5. **Records that no script states are checked and set aside, and nothing else is.** These are an
   ask's `DecisionMade`, and a cancellation's `AgentModeChanged` (checked by its reason),
   `OwnerExitRequested` and `KillSwitchActivated`. Every other member the runtime writes is either
   stated by the case or checked here:
   - `on_timeout` is `skip`.
   - `ApprovalRequested.content` is checked only as the preimage of its `content_hash`. Its keys and
     its action's values are `mandate-approval`'s to pin, by
     `content::the_content_object_has_exactly_the_listed_fields` and
     `the_action_is_the_bound_order_and_its_value`.
   - `ApprovalRequested`'s `causation_id` is the ask's `DecisionMade`, and `ApprovalDelivered`'s is
     its request.
   - `ApprovalDelivered.message_id` is null.
   - `ApprovalResponded.role` is `approver`.
   - `ApprovalRevalidated`'s compared values are the bound order's and `now`'s, with `band_bp` 100
     for `us_equity` and 200 for `crypto`.

   Every handoff follows the record that authorises it, and every `IntentProposed` is handed
   exactly once (`AGENTS.md` rule 5). The step's other effects, the deadline timer and the opaque
   notification, are `mandate-runtime`'s to pin, by
   `an_ask_for_an_exit_is_proposed_and_an_ask_for_an_opening_asks` and
   `a_timely_grant_acts_with_the_bound_order`.
6. **Every member is read (DEC-85).** The driver reads the case, the context, each step by its
   kind, the bound order (each member also compared with the request the runtime wrote), each
   response and its step-up, each `now`, each expectation, and each expected draft by its type. An
   unknown member or value fails, naming it. So does anything the runtime cannot be driven to: a
   push channel or a second inbox (E8-4), a purpose other than `open` or `increase`, or a side other
   than `buy`. Quiet hours are parsed but cannot change a draft, since they suppress push alone and
   the runtime's one channel is `cli_inbox`. MC-E29 pins exactly that.
7. **What passes and what waits.**
   - **Once all three slices land, fifteen cases pass:** MC-E02 to MC-E05, MC-E07 to MC-E16, MC-E31.
   - **Ten fail only on `ApprovalResponded`'s `quorum`:** MC-E01, MC-E06, MC-E17, MC-E19 to
     MC-E24, MC-E29. `quorum` is the approver count and independence that check 7 applied. Journal
     spec §9 requires it and the runtime does not write it yet (E8-3, a backlog row). With `quorum`
     struck, all ten pass.
   - **MC-E18** is [DEC-318](earlier-303-599.md#dec-318)'s.

   No reference case changes.
8. **Delivery.** The driver lands in the three PRs [DEC-366](earlier-303-599.md#dec-366) sets out. Until a step's
   PR lands, a case fails at that step, naming the slice that owes it.

### Rationale

DEC-292 item 3 asked for an arm that drives the runtime rather than the reference model. Driving
`handle` and `fold` with the case's `context` and `now` as ports makes every outcome the runtime's.
Publishing one map keeps the comparison strict where the two vocabularies differ: ids, the content
hash's stand-in, instants as seconds, and member names.

### Alternatives considered

- **A reference interpreter of the lifecycle.** Rejected: it would check the reference against
  itself.
- **Comparing the case's content hash literally.** Rejected: the reference's stand-in is not the
  content object, so every response would be refused as `content_mismatch`.
- **Writing `quorum` into the runtime here.** Rejected: it is the runtime's change, tests first,
  under E8-3.

<a id="dec-318"></a>

## DEC-318: MC-E18 against §6.4 "Cancellation": a grant while the mode is exits-only

| | |
|---|---|
| **Status** | Accepted: option (a), by the founder on 2026-10-02 (relayed by the coordinator on [#213](https://github.com/kunwarshivam/mandate/issues/213)) |
| **Date** | 2026-10-02 |
| **Decided by** | The founder, 2026-10-02 (option (a)); the reading of what still reaches check 9's `mode` is [DEC-430](earlier-303-599.md#dec-430) |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-317](earlier-303-599.md#dec-317) item 7 |

### Decision

1. **The question.** MC-E18 admits a grant while `now.mode` is `exits_only` and expects
   re-validation to skip it with reason `mode` (check 9). Mandate spec §6.4 "Cancellation" says an
   effective mode of exits-only or stricter cancels every pending approval in the same step, before
   that step judges a response. The runtime does exactly that. Driven by the DEC-317 harness, the
   response step writes `AgentModeChanged`, then `ApprovalCanceled` (`mode_tightened`), then
   `ApprovalResponded` (`refused`, `not_pending`). Both outcomes skip and neither acts, so no risk
   differs. But the drafts differ, and check 9's `mode` is reachable by no path the spec names.
2. **The options for the founder.**
   - **(a)** Restate MC-E18 as the cancellation the spec gives. This is a `mandate.yaml` change.
   - **(b)** Keep the case and name in §6.4 the path by which a pending approval meets a non-normal
     mode at re-validation.
3. **Until the founder decides:** MC-E18 stays pending, and no case, spec text or runtime behaviour
   changes. This is the conservative option, because the runtime's behaviour cancels earlier and
   never acts.
4. **The founder's decision (2026-10-02): option (a).** MC-E18 is restated as the cancellation §6.4
   gives and the runtime writes: in the response's step, `AgentModeChanged`, then `ApprovalCanceled`
   (`mode_tightened`), then `ApprovalResponded` (`refused`, `not_pending`). No spec text and no
   runtime behaviour changes. The reference model drafts the cancellation and the refusal, and the
   harness strips `AgentModeChanged`, as it does for every cancellation's cause. The case is
   regenerated from `reference/mandate` and retitled, so its title no longer says re-validation
   skips for `mode`. [DEC-430](earlier-303-599.md#dec-430) records what still reaches check 9's `mode` arm.

### Rationale

Moving the case toward the runtime, or the runtime toward the case, changes either a reference case
or a safety-critical path. DEC-176 lets an agent take only a reading that tightens. Here both
outcomes skip and the question is which record the spec owes, so the choice goes to the founder.

### Alternatives considered

- **Taking reading (a) as an agent tightening.** Rejected: it edits reference-case YAML, which is
  founder-gated.
- **Making the runtime judge before it cancels.** Rejected: that contradicts §6.4 and DEC-131 item
  25(j).

<a id="dec-326"></a>

## DEC-326: `market_orders_barred` drops its `at.opening_auction` arm

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (`opencode`, stream G; the coordinator assigned DEC-326 to DEC-328 to this lane at dispatch, 2026-10-01) under DEC-79, a reversible engineering decision |
| **Story or claim** | stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the #228 round-1 backlog minor 2, from E6-6's bug list |

### Decision

`mandate-risk`'s `market_orders_barred` no longer reads `at.opening_auction`. The arm decided
nothing the remaining arms do not decide:

1. `session::derive` sets `opening_auction` only when `session == PreMarket` (§4.3's 09:28 to
   09:30 ET, the last two minutes of pre-market), and for a US equity the session arm
   (`at.session != Session::Regular`) already bars the market order there.
2. Crypto's `SessionAt` always carries `opening_auction: false`, so the arm never fired for
   crypto.

So no decision depends on the clause and no test can catch its removal — the backlog row's own
criterion for dropping it rather than pinning it. `session::rules`' `AuctionWindow` denial of a
market **opening** in either auction window is a different decision and is untouched.

### Rationale

A dead arm gets copied (the trust ladder's first rung is no help here: the clause is a plain
`bool` disjunct), and every reader of §5.1 has to re-derive that it is dead. The removal puts the
reading where the code is.

### Alternatives considered

- **Pin it with a test.** Impossible as the backlog row states: no input reaches the arm without
  another arm already barring the order, so no observable decision changes when it goes.
- **Keep it and document it as redundant.** Leaves the dead code the row asks to settle.

<a id="dec-327"></a>

## DEC-327: an exit whose pacing cannot compute is routed by E6-6; until then the path reports its stub

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (`opencode`, stream G; the coordinator assigned DEC-326 to DEC-328 to this lane at dispatch, 2026-10-01) under DEC-79, a reversible engineering decision; the implementation reading is the one that adds no risk (DEC-176) |
| **Story or claim** | stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the #311 round-2 backlog minor 1 |

### Decision

1. **The defect.** `conduct::pacing` propagates the collar's arithmetic errors, so `evaluate`
   returns `Err(GateError::Num(_))` for a **discretionary** exit (order builder, goal completion,
   removed instrument), an error where `AGENTS.md` rule 13 owes a routed exit. Two figures reach
   it, both on the passive end of a sell's collar, `bid × (1 + band)`, which the collar computes
   whatever the limit:
   - **Overflow.** `Price::collar_bound` cannot hold the product at the 9 places a price keeps.
     This is not one threshold: the product overflows only where it needs nine places and has no
     trailing zeros to drop, so at a band of 0.2 a bid of `66023468761886947994.619958614`
     overflows and `66023468761886947995` does not.
   - **Not positive.** For a US equity, a passive end below one Reg NMS tick (a bid below about
     `0.0000834` at a band of 0.2) truncates to zero on the grid when a limit above it is lowered
     to it, and `Price::on_tick` returns `not_positive`. Crypto is not put on a grid and is
     unaffected.

   The aggressive end `bid × (1 − x)` rounds up and fails only for a configured `x` of one. No
   participation cap can fail: `Qty::portion` takes a fraction of at most one and the remaining
   daily cap's only error is the `Negative` that `slice` already reads as zero, so an owner exit
   always gets a decision, and risk exits, protective orders and kill switches are not paced at
   all. On the advisory path `mandate-shell`'s `map::verdict_of` turns the error into a denial;
   on the binding path the caller fails closed. The other properties' generators are deliberately
   small and never reach the collar's edges, so nothing exercised it.
2. **The tests PR.** The path reports the stub instead: `Unimplemented("pacing", "E6-6")`, behind
   the pending property `an_exit_over_extreme_figures_is_still_routed`, which draws both triggers
   by name (`66023468761886947994.619958614` and `0.0000833`) beside the decimal's edges, over
   every paced exit origin, and asserts a decision: `Ok`, never an error, never a denial, and the
   whole proposed quantity, so a fix that routed the exit sliced to zero fails it. Only the
   collar's error is mapped to the stub; `slice` cannot fail, so it is not wrapped (DEC-326's
   criterion for dead code).
3. **The implementation reading E6-6 takes.** A bound the arithmetic cannot compute paces nothing:
   the control is skipped as if its input were missing (DEC-163 item 4's missing-volume reading,
   DEC-163 item 5's no-usable-quote reading for the collar), and the exit goes as proposed. Never
   a denial, never a fabricated figure.

### Rationale

The executor's only honest answer to a gate error on an exit would be to route the exit anyway,
which spreads rule 13 across two crates and loses the gate's own contract that an exit gets a
decision. Failing closed by refusing an exit adds risk. Skipping the control that cannot compute
is the reading that never denies an exit and never invents one.

### Alternatives considered

- **State what the executor does with the error** (the backlog row's other arm): rejected above.
- **Saturate in `mandate-num`**: a silent wrong figure on a trading path is worse than no figure;
  the crate's own rule is exact or an error.
- **Fix it in this PR**: a tests PR for a safety-critical path lands the tests first (DEC-77), and
  the fix deserves its own mutation-gated change under E6-6, whose bug list the backlog row joins.

<a id="dec-328"></a>

## DEC-328: the opposite-fill property draws its boundary explicitly; the exempt exit is asserted whole

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (`opencode`, stream G; the coordinator assigned DEC-326 to DEC-328 to this lane at dispatch, 2026-10-01) under DEC-79, a reversible engineering decision |
| **Story or claim** | stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the #311 round-2 backlog minors 2 and 3 |

### Decision

1. `properties::only_an_opposite_side_fill_starts_the_interval` draws the fill's age in
   nanoseconds, with 60 s, 60 s and a nanosecond, 60 s less a nanosecond, and 61 s drawn
   explicitly beside a uniform range, and draws `now` off the whole second, so the interval's
   inclusive boundary (DEC-163 item 3) is pinned every run at the precision the gate compares at:
   60 s after an opposite-side fill still blocks, 60 s and a nanosecond does not, and a gate that
   dropped either instant's nanoseconds fails it.
2. `gate::tests::an_allowed_exit_is_never_below_its_minimum_or_zero` asserts `sent == proposed`
   for `Origin::RiskEngine` and `Origin::ProtectiveLeg` — the two purposes no conduct control may
   pace — over every generated shape, not only in the slice tests' fixed rows.

### Rationale

The old draw `0..120` lands on 60 or 61 by chance, and `ci test` fixes no proptest seed, so a
boundary regression could pass for many runs. The whole-exit assertion moves the exempt purposes'
guarantee from a fixed table into the property that generates the shapes.

### Alternatives considered

- **Rely on `the_opposite_fill_interval_includes_its_last_instant`** (the fixed edge test): it
  stays, but a property that cannot catch the regression it names on a given run is not what the
  spec's "every claim is a test" rule asks for.
- **Assert `d.pacing.is_none()` for the exempt purposes**: stronger than the quantity assertion,
  and a trap — a market-kind draw in the close window is sent as a marketable limit whatever the
  purpose (DEC-129 item 28), so the assertion would break on the next generator that earns its
  keep. The quantity is what the backlog row names.

<a id="dec-329"></a>

## DEC-329: ShellCheck over `.github/scripts/` joins `cargo xtask ci lint`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (an `opencode` lane; the coordinator assigned DEC-329 to DEC-331 to this lane at dispatch) under DEC-79: a reversible engineering decision that adds checks and loosens none. Round-1 fixes by Claude Opus 5.5 |
| **Story or claim** | E1-4; PR [#434](https://github.com/kunwarshivam/mandate/pull/434) |

### Decision

1. The lint job runs ShellCheck, at its default severity, over every `*.sh` under
   `.github/scripts/`, sorted by path, and fails on any finding, an `info` such as `SC2086`
   included. The findings travel in the failure message.
2. A directory holding no scripts still runs `shellcheck --version`, so an uninstalled tool fails
   the job it belongs to; a missing tool or a missing directory is a loud error, never a skip.
3. This amends ADR-0001 ES-12's lint job, together with DEC-330.
4. `a_shellcheck_error_in_a_fixture_script_fails_lint` holds the oracle: a clean script passes and
   a planted `SC2086` fails naming the code. Its wiring into the job is held by DEC-331 item 3.

### Rationale

The scripts under `.github/scripts/` are the merge path (DEC-175) and CI's short path, run as they
are on the default branch; a quoting mistake there should fail a check rather than wait for a
reviewer (the #316 reviews).

### Alternatives considered

- **ShellCheck at `--severity=warning`.** It would pass the unquoted expansion the backlog's
  acceptance criterion names (`SC2086` is `info`).
- **Leave shell review to reviewers.** That is the gap the #316 reviews found.

<a id="dec-330"></a>

## DEC-330: actionlint over `.github/workflows/` joins `cargo xtask ci lint`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (an `opencode` lane; the coordinator assigned DEC-329 to DEC-331 to this lane at dispatch) under DEC-79: a reversible engineering decision that adds checks and loosens none. Round-1 fixes by Claude Opus 5.5 |
| **Story or claim** | E1-4; PR [#434](https://github.com/kunwarshivam/mandate/pull/434) |

### Decision

1. The lint job runs actionlint over every `*.yml` and `*.yaml` under `.github/workflows/`, sorted
   by path, and fails on any finding, which travels in the failure message with the tool's stderr.
2. A directory holding no workflows still runs `actionlint --version`, so an uninstalled tool
   fails the job it belongs to; a missing tool or a missing directory is a loud error, never a
   skip.
3. This amends ADR-0001 ES-12's lint job, together with DEC-329.
4. `an_actionlint_error_in_a_fixture_workflow_fails_lint` holds the oracle: a minimal workflow
   passes and an unknown key fails naming it. Its wiring into the job is held by DEC-331 item 3.

### Rationale

A workflow mistake (an unknown key, a bad expression) otherwise surfaces only when the workflow
runs, or in review. `docs-only.sh` classifies any workflow change as not docs-only, so a workflow
edit always reaches `fast` and this check.

### Alternatives considered

- **Lint workflows only in the nightly run.** A broken required workflow would land first.

<a id="dec-331"></a>

## DEC-331: E1-4's linters are pinned, and a fixture repository holds their wiring into the lint job

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (an `opencode` lane; the coordinator assigned DEC-329 to DEC-331 to this lane at dispatch) under DEC-79: a reversible engineering decision that adds checks and loosens none. Round-1 fixes by Claude Opus 5.5 |
| **Story or claim** | E1-4; PR [#434](https://github.com/kunwarshivam/mandate/pull/434), round 1 (M2 and the minors) |

### Decision

1. **Versions.** ShellCheck 0.11.0 and actionlint 1.7.12, the same in `.cursor/install.sh`,
   `.github/workflows/ci.yml` and `.github/workflows/nightly.yml`.
2. **Install paths and integrity.** `.cursor/install.sh` downloads both from their GitHub release
   assets and checks each archive's SHA-256 in-repo, as it does for typos and gitleaks. In CI,
   actionlint is downloaded and checked against `ACTIONLINT_SHA256` in the workflow, as gitleaks
   is. ShellCheck in CI rides `taiki-e/install-action` (pinned by commit SHA), so its integrity
   there rests on that action's own manifest, not on a checksum in this repository: the same
   treatment `typos` and `cargo-nextest` already have.
3. **The wiring is tested.** The lint job is the function `lint(root, workspace_checks)`, which runs
   ShellCheck over `root`'s `.github/scripts/`, actionlint over its `.github/workflows/`, then the
   workspace checks (fmt, clippy, layers, markers, the feature map, typos, ruff); `ci lint` calls it
   with the current repository and the real workspace checks. Because the job takes the repository
   it runs in (the DEC-139 pattern), the test
   `the_lint_job_fails_on_a_finding_of_either_tool_in_its_repository` drives it over a fixture
   repository: clean, it passes and reaches the workspace checks; a planted `SC2086` fails it
   naming the code; an unknown workflow key fails it naming the key. Discarding either tool's
   result in the job, or skipping the workspace checks, fails that test.
4. **What the fixture tests prove.** `a_shellcheck_error_in_a_fixture_script_fails_lint` and
   `an_actionlint_error_in_a_fixture_workflow_fails_lint` call the two checks directly: they hold
   that each check judges the files it is given (the exit status, the findings in the message, the
   file extensions), not that the job calls them. Item 3's test holds the job.

### Rationale

`xtask` is not `safety_critical`, so the mutation gate does not cover it, and clippy's dead-code
lint only catches a deleted call, not a discarded result (the #434 round-1 review showed `let _ =`
on either call left `ci lint` green with a live `SC2086` in the tree). The fmt and clippy half of
the job needs a Cargo workspace, which a fixture repository would have to build, so the job takes
it as an argument and the test records that it was reached.

### Alternatives considered

- **Leave the fixture tests as the proof.** They call the checks directly and cannot see the job.
- **A fixture Cargo and uv workspace driving every step.** Slow, and it tests fmt and clippy, which
  are not this change.
- **Checksum ShellCheck in CI by hand.** A second install path for one tool; the action's pinned
  manifest already covers typos and cargo-nextest the same way.

<a id="dec-335"></a>

## DEC-335: E17-8's follow-up: the empty scoreable set's refusal is named, where the nameless code answered

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (an `opencode` lane, stream J; the coordinator assigned DEC-335 to DEC-337 to this lane at dispatch) under DEC-79: a name for a condition the code already refuses, no existing code changed. Round-1 fixes by Claude Opus 5.5 |
| **Story or claim** | E17-8's follow-up (#410 review minor 3, DEC-282 item 9), mandate spec §8.6, [backlog](../06-backlog-v1.md) E17-8; claim [#132](https://github.com/kunwarshivam/mandate/issues/132); PR [#435](https://github.com/kunwarshivam/mandate/pull/435) |

### Decision

1. An evaluation whose scoreable set is empty (no theses at all, or every thesis unscoreable) and
   that the count refusal does not answer refuses with its own `ResearchError` arm,
   `EmptyScoreableSet`, and its own ES-09 code, `empty_scoreable_set`. The code is added; nothing is
   renamed or removed (ES-09's add-only registry, the DEC-255 pattern). The run refuses fail-loud
   under a name a caller can read, and no figure is ever invented for an empty set.
2. **What a caller sees.** The named code reaches a caller only at a registered
   `minimum_scoreable` of zero, the only minimum an empty set is not below. Below a positive
   minimum an empty scoreable set still refuses `window_not_closed`, which does not tell "no
   theses at all" from "theses, none scoreable" (DEC-336 sets this reach; the backlog offers the
   broader reach to the founder).
3. The arm and its code land live in the tests PR, pinned by the module's live code-pin test the
   mutation gate runs (the E17-6 pattern). The evaluation path itself is a stub returning
   `Unimplemented` until the implementation PR deletes only the pending tests' `#[ignore]` lines
   (DEC-77 stage 1, DEC-337).

### Rationale

DEC-282 item 9 left the refusal nameless and owed the name to a backlog row. The failure mode
(reaching `Ratio::mean` of an empty set) is unchanged in kind: still fail-loud, still no report;
only now it names itself.

### Alternatives considered

- **Keeping `Num(DivisionByZero)`.** The status quo: loud but nameless, exactly what minor 3 named.
- **Refusing earlier as `WindowNotClosed` at every minimum.** It would contradict the count
  refusal's own meaning: the window's count condition is true, yet the empty set is its own, more
  specific, condition.
- **A broader arm that fires before the count check at every minimum.** Two of #410's round-1 live
  pins freeze the count refusal for an empty scoreable set below a positive minimum, and the frozen
  surface stays.

<a id="dec-336"></a>

## DEC-336: E17-8's follow-up: the named empty-set refusal's reach and precedence

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (an `opencode` lane, stream J; the coordinator assigned DEC-335 to DEC-337 to this lane at dispatch) under DEC-176: the narrower reading, which changes only the condition DEC-282 item 9 named and loosens nothing. Round-1 fixes by Claude Opus 5.5 |
| **Story or claim** | E17-8's follow-up, DEC-335; claim [#132](https://github.com/kunwarshivam/mandate/issues/132); PR [#435](https://github.com/kunwarshivam/mandate/pull/435) |

### Decision

1. The arm answers exactly where the nameless `Num(DivisionByZero)` did: a scoreable set the count
   refusal does not answer, which is an empty set at a registered `minimum_scoreable` of zero, the
   only minimum an empty set is not below.
2. Below a positive minimum the count refusal keeps the empty set, as #410's round-1 live pins
   freeze it (`a_thesis_whose_closes_all_lie_outside_its_window_is_unscoreable`,
   `a_degenerate_window_is_unscoreable_never_a_figure`).
3. A non-empty set below the minimum keeps `WindowNotClosed`, and a non-empty set at its minimum
   reports unchanged.
4. Input integrity keeps its precedence: a thesis whose horizon closes outside the registered
   window is refused before the empty set is ever reached (DEC-282 item 3).

### Rationale

The follow-up's own brief names the minimum-of-zero case; the broader reach would need two live
pins changed, which the frozen surface forbids. Every empty scoreable set still refuses at every
minimum, so no figure and no division by zero escapes under this reading; it only leaves the
positive-minimum case under its existing code.

### Alternatives considered

- **The arm before the count check at every minimum.** It breaks the two live pins, and the
  implementation PR may only delete `#[ignore]` lines. It changes pinned ES-09 refusal codes set by
  DEC-282 item 8, so it is the founder's call; the backlog's E17-8 entry offers it.
- **The arm keyed on an empty theses input rather than an empty scoreable set.** An all-unscoreable
  input at a minimum of zero would still reach the nameless division.

<a id="dec-344"></a>

## DEC-344: Each decision is its own file; the decision log takes no new rows

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The coordinating agent (`claude-code`) under DEC-79, a reversible process decision; the founder approved it the same day |
| **Story or claim** | Coordinator log [#165](https://github.com/kunwarshivam/mandate/issues/165) |

### Decision

1. A decision first written after this one is a file, `docs/project/decisions/DEC-<n>.md`, in the
   format the directory's [README](README.md) gives. The decision log's Decisions table takes no
   new rows; DEC-01 to DEC-302 stay there and stay binding.
2. A number is reserved by adding its file with the status `Reserved` in the first commit that
   uses it. The log's Reserved identifiers table takes no new `DEC-` rows. It stays the registry
   for the other kinds of identifier (`OD-`, ADR numbers, epic and story IDs), which are minted
   rarely.
3. A PR edits only its own decision files.
4. DEC-303 to DEC-343, handed out before this decision, are written as files too. An open PR that
   carries one as a log row moves it into a file the next time it merges `main`.
5. No file is generated from these and committed: an index would be one more file every PR edits.

### Rationale

In the two days before this decision, 72 of the 115 commits on `main` edited the decision log, and
nearly all of them appended to the end of the same two tables. Each merge therefore left every
other open PR conflicting, and each conflict cost a merge of `main`, a hand resolution, a CI run
and a fresh approval of the new head. With about twenty lanes in flight that queue, not the
building, set the pace.

Two PRs that add different files do not conflict. Two PRs that take the same number add the same
path, and git refuses the second, so the collision the coordination playbook calls silent becomes
loud without a registry row having to reach `main` first.

The log is also 790 kB in 408 lines, with single rows of several thousand characters, which no
reviewer reads as a table. A file per decision can use headings and lists.

### Alternatives considered

- **One decisions file per PR.** It removes the conflicts as well, but a decision is cited by its
  number, not by the PR that carried it, and a second PR that amends or completes a decision would
  have to edit another PR's file.
- **A union merge driver (`merge=union` in `.gitattributes`).** GitHub does not apply merge
  drivers when it computes a PR's mergeability or merges it, so the PR would still show as
  conflicting.
- **GitHub's merge queue.** It is offered only to repositories an organization owns, and this one
  is under a personal account. It would also not remove a textual conflict; it only replaces the
  up-to-date requirement, which the `main` ruleset no longer sets.
- **Reserving every range in the log in a coordinator-only PR before dispatch.** That removes the
  reservation conflicts and keeps the conflict on the decision rows themselves.
- **Moving DEC-01 to DEC-302 into files now.** Several hundred links point at the log, and every
  open PR would conflict with the move. It can be done later, in a quiet queue, as its own change.

### Not decided here

The work tracker and the backlog are edited by most PRs as well (48 and 71 of the same 115
commits) and conflict in the same way. They need their own change. A check that refuses a new
decision row in the log, and a malformed decision file, belongs in `cargo xtask` and is owed as a
follow-up.

<a id="dec-345"></a>

## DEC-345: `mandate-executor`'s test event ids are alphanumeric, so a protective order can be named in the integration suites

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176: a tests correction that tightens nothing and loosens nothing |
| **Story or claim** | E7-4 slice 2, claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

1. `crates/mandate-executor/tests/common/mod.rs`'s `derived_id`, the integration suites' own
   derivation of an event id (DEC-131 item 6), derives `e{epoch}h{head}o{ordinal}` in place of
   `e{epoch}-h{head}-o{ordinal}`. This is a DEC-77 tests correction, landing before E7-4 slice 2's
   implementation PR.
2. Nothing else changes. `ClientOrderId::for_protection` still refuses an event id that is not
   alphanumeric rather than escaping it (trading-domain spec §2.3, DEC-160 (3)(a), and the
   in-module test `a_protection_event_that_is_not_alphanumeric_is_refused`).

### Rationale

§2.3 names a protective order `{entry}-p{protection}`, where `{protection}` is the alphanumeric id
of the `ProtectionChanged` that records the placement. A production event id is a ULID, so the
grammar holds there. The integration suites' ids carried hyphens, so every placement the executor
makes under `tests/` was refused as `malformed_client_order_id`. These placements are a bracket's legs,
the OCO for a partly filled entry, and a re-placement after an exit. No test under `tests/` reached
one before slice 2. Slice 2's pending tests (`hand::a_partly_filled_bracket_becomes_an_oco_for_the_filled_quantity`,
`a_terminal_partly_filled_entry_is_oco_d_at_once`, `fault::crash_between_entry_fill_and_oco`, and
the RC-14 family behind them) need one, so with hyphenated ids they could not pass on any
correct implementation.

The in-module suites already derive `e{epoch}h{head}o{ordinal}` (`reconcile::tests::Ids`). The new
format stays injective, since the letters separate the three numbers. No live test reads the old
format: `cargo nextest run -p mandate-executor` passes unchanged. `cargo xtask ci pending` still
finds every pending test failing at its stub.

### Alternatives considered

- **Escape or strip the hyphens in `for_protection`.** That loosens an idempotency derivation in a
  safety-critical crate to suit a test double, and it reverses a pinned refusal. Rejected.
- **Derive `{protection}` from something other than the event id.** That contradicts §2.3 and
  DEC-160, which only the spec's owner may change. Rejected.

<a id="dec-346"></a>

## DEC-346: E7-4 slice 2's readings: brackets, the partial-fill interval and timeout, and the OCO for the filled quantity

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176: each item tightens a rule or resolves a gap by the reading that adds no risk |
| **Story or claim** | E7-4 slice 2 (DEC-160 (1), (4), (5)), claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

1. **The tranche model** (trading-domain spec §5.4). An opening or add on an equity that carries its
   protective prices goes as one GTC bracket at exactly those prices, with no extended hours. An add
   where protection already rests is a new bracket beside it and cancels nothing. This replaces the
   `bracketed_add` stub (DEC-160 (5)). An entry without prices still goes plain, and an exit never
   carries a bracket. `OrderSubmitted` journals `order_class: bracket` with its `take_profit` and
   `stop`, and the fold reads them back, so a resubmission after a confirmed absence sends the same
   bracket (§5.7).
2. **No bracket to go as.** An equity entry with a stop and no take-profit answers E7-4's stub, as
   `replace` does for an equity stop-only placement, rather than going unprotected. An opening may
   always be refused (`AGENTS.md` rule 2). A crypto entry is never a bracket (§5.4: simple orders
   only). Its stop-limit is slice 5's, and until then it goes as before.
3. **"The filled quantity."** For a bracket entry this is the larger of the fills the fold has
   ingested and the broker's own cumulative `filled_qty` on the order's latest report. That figure
   is now folded from `OrderStateChanged` into the order's detail. A report runs ahead of its fills,
   so waiting for the fills would leave filled shares unprotected for longer.
4. **The records.** At the entry's first fill, `ProtectionChanged unprotected_start` names the entry
   as `bracket`, and the interval runs from there. Once the entry is completely filled, its legs are
   active at the broker: `ProtectionChanged placed` records them as one protective order for the
   entry's whole quantity, named `{entry}-p{record}` (§2.3), with no `OrderSubmitted`, because the
   broker created them (DEC-160 (3)). If an interval was open, `unprotected_end` follows. A
   `placed` or `unprotected_end` that names the entry as `bracket` marks it done, so each entry is
   protected once, across restarts.
5. **The timeout.** An entry still working (`Accepted` or `PartiallyFilled`) has its remainder
   cancelled, once, at the shorter of `bracket_partial_fill_timeout_s` and `max_unprotected_s` after
   its interval started. §5.4's bound also cancels the unfilled order at the bound, not only alerts.
   The cancel comes at once when the entry's venue is no longer the regular session before the
   closing auction window, which reads §5.4's "always before the closing auction window" as that
   window or anything later. Cancelling an opening's remainder only reduces risk.
6. **The OCO for the filled quantity.** An entry that turns terminal partly filled, after that cancel
   or by any other path (§5.4's second case), gets one GTC OCO at the bracket's prices. The OCO is
   sized to the filled quantity, capped at what the position leaves after every live protective
   order and every exit still selling (rule 12: Σ protective sells never exceed the position). For
   the cap, the position includes the fills the broker reported and the fold has not yet ingested
   (item 3). Nothing left to cover places nothing, and the interval ends either way. DEC-348 item 2
   (the coordinator's RC-21 ruling (a)) succeeds this item's interval end: once it lands, an OCO
   placed here ends the interval at the broker's acknowledgment, not at the submission. The OCO is
   placed whatever the agent's mode. Rule 13 lets a pause hold protection but never requires it, and
   placing it only reduces risk.
7. **A property that no implementation can pass yet.** On a script that ends while the protected
   lead's partly filled entry is still inside its interval (no completing fill, no timeout reached
   or confirmed), `properties::every_unprotected_interval_has_a_journaled_start_and_end` fails,
   since nothing could have closed that interval. Its `BEHAVIOUR_ONLY_TESTS` row stays. A DEC-77
   tests correction (backlog) should judge an interval still open at the end against what could
   have closed it. The other two `properties` rows are deleted, since their minimal failure is now
   the kill switch's stub (slice 7).

### Rationale

These are the points §5.4 and DEC-160 leave open for slice 2. Each takes the reading that leaves
less unprotected or sells less: a report counts before its fills, the cap keeps protection within
the position, and an entry that cannot be bracketed is refused rather than sent bare.

### Alternatives considered

- **Size the OCO from ingested fills only.** That leaves reported fills unprotected until a
  reconciliation ingests them. Rejected.
- **Place the OCO for the whole filled quantity uncapped.** That can over-cover after an exit sold
  part of the position while the entry was working, against rule 12. Rejected.
- **Send an equity stop-only entry as a plain order.** That adds unprotected risk the intent asked
  to protect. Rejected.

### Left open (backlog)

- `mandate-alpaca` must map `{entry}-p{record}` to the broker's own leg ids before any shell path
  hands the executor a protected intent. Today none does, so slice 2's brackets are reachable only
  from the executor's tests. This is the existing row for the slice that reconciles protective legs.
- A re-placement after an exit covers every tranche at the prices of the latest `placed`, because
  the fold keeps one price pair per instrument. Tranches at different prices make that choice real
  for the first time, so it goes to the coordinator.
- Bracket legs and the partial-fill OCO record no `created_on`. §5.4's re-placement before expiry is
  slice 5's trading-day part, and it must date the placement from its record.
- An intent received and not yet submitted carries its stop and take-profit only on
  `IntentReceived`, which #441's DEC-307 item 1 closes at nine members; where those two go is
  DEC-360's (Proposed, the founder). A submitted entry recovers its bracket from `OrderSubmitted`'s
  `order_class` (item 1), so only the not-yet-submitted case depends on DEC-360's outcome.

<a id="dec-347"></a>

## DEC-347: The executor's reference-case harness starts as a production shell does, from an attributed position

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79: a reversible tests correction that loosens no rule |
| **Story or claim** | E7-4 slice 5's trading-day part (backlog: "the harness's startup reconciliation"), claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

A DEC-77 tests correction to `crates/mandate-executor/tests/refcases.rs` and `tests/common/mod.rs`,
landing before slice 5's trading-day part:

1. **The seed starts ready.** After the seeded restart, the harness runs the startup reconciliation
   and the account report, as `Shell::ready` does for the hand suites, through a new
   `Shell::ready_with`. The account it reports carries `initial.account`'s settled cash as its
   cash. The fixture names no other figure, so equity and buying power stay the shell's default
   account's, since settled cash is not buying power (#447 round 1, m2). Before this, the executor rightly held every
   opening `startup_reconciliation_pending`: it holds openings until a reconciliation has run and
   an account has been journaled since the start (§11, the coordinator's rulings on #174).
2. **A seeded position is attributed.** `initial.positions` is seeded as the agent's own
   `OrderSubmitted`, `FillApplied` and `filled` state, where it was a bare `FillApplied`. Without
   that, no agent's lots make up the position, the seeded protection has no single holder
   (DEC-160 (3)(b)), and every opening is held `protection_unattributed`. #242 made the same
   correction to the hand suites.
3. **A reconciliation check reports the step's cash.** Both snapshots `assert_reconciliation`
   builds for an unposted-fee check report the step's expected settled cash, where they reported
   the default 20000. That figure was never compared before an account was observed. The second
   snapshot's cash is what keeps the tolerance sub-check turning on the quantity alone (#447
   round 1, B1).
4. **A proposal is named when it is made.** A `propose_order` step's `name` is bound to its order at
   proposal time when the step expects no `submit` (RC-22's `buy_passive`), so a later step's
   `cancel: buy_passive` can find it. A step that expects a submission still binds its name to
   the order its `submit` matches, so a passive exit's protective OCO, which carries the exit's own
   id, is not bound twice.
5. **Each item is pinned by a live test that fails when it is reverted** (#447 round 1, m3):
   - `the_seed_runs_the_startup_reconciliation_and_reports_the_account` (item 1);
   - `the_seeded_protection_belongs_to_the_positions_holder` (item 2);
   - `a_reconciliation_check_reports_the_steps_own_cash` (item 3): it runs RC-07 step 0's
     reconciliation live through `assert_reconciliation`, whose tolerance check now also requires
     the mismatch to be the quantity's alone, so deleting either snapshot's `account_after(step)`
     fails it (#447 round 2, M1);
   - `a_proposal_is_named_when_made_only_where_no_submission_is_expected` (item 4).

### Rationale

The harness drove each case from a state no production shell could be in. The executor's correct
holds then stopped every case with an opening at its first step, whatever slice landed. The four
items change only how the harness meets a case, never what it asserts. On `main` every refcase
still fails at its stub (`cargo xtask ci pending` exit 0), and every live test passes.

### Alternatives considered

- **Exempt the harness's openings from the startup hold in the executor.** That weakens a safety
  hold to suit a test. Rejected.
- **Seed the protection's agent in the `placed` record.** The leg-agent rule reads the lots, not the
  record, by design (DEC-160 (3)). Rejected.

<a id="dec-348"></a>

## DEC-348: When a sequence's unprotected interval starts and ends (RC-14, RC-21), and RC-22's unasserted step

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The coordinator's rulings on [#174](https://github.com/kunwarshivam/mandate/pull/174) (comments [5944523791](https://github.com/kunwarshivam/mandate/pull/174#issuecomment-5944523791) and [5944795818](https://github.com/kunwarshivam/mandate/pull/174#issuecomment-5944795818)), under DEC-176: each item takes the reading that only tightens, so an agent may accept it. The founder may reverse any of them |
| **Story or claim** | E7-4 slice 5's trading-day part and the RC-14 acceptance clause, claim [#137](https://github.com/kunwarshivam/mandate/issues/137) |

### Decision

1. **The interval starts when the cancel is asked** (RC-14 base, step 0). The executor keeps
   journaling `ProtectionChanged unprotected_start` when it asks the protective cancels (DEC-160
   (8), rule 5), so the interval, and `max_unprotected_s` with it, run from that earlier instant.
   The founder-owned YAML, which lists the start after `await_cancel_confirmed`, is not edited.
   The executor's reference-case harness matches `journal: unprotected_window_start` anywhere
   before the cancel's confirmation, where the step lists one, and before the gate's re-run, and
   never after either. That is tighter than the ruling's "before `rerun_gate`", which DEC-176
   permits. Every other action keeps the fixture's order. Each bound is pinned on its own:
   - `refcases::an_interval_start_journaled_after_the_confirmation_fails`: a start after the
     confirmation, the YAML's literal order, fails the case;
   - `an_interval_start_after_the_gate_re_runs_fails` and
     `an_interval_start_before_the_gate_re_runs_matches`: in a step with no confirmation, the
     re-run bounds the start.
2. **The interval ends when the broker acknowledges the new protection** (RC-21, step 2). It does
   not end when the executor sends it. A write-ahead executor journals a step's drafts before
   that step's requests, so an end journaled with the submit precedes it. The end therefore
   waits for the step in which the broker accepts the new OCO, which matches the YAML's order
   literally and keeps the window open over the OCO's own round trip. This applies alike to
   `replace`, `passive_exit`'s placement and slice 2's OCO for a partly filled entry. It lands in
   its own implementation PR, with the rule-13 properties shown green. The harness side lands here:
   - an `unprotected_end` that names what it is `awaiting` is not read as the interval's end. No
     record on `main` carried `awaiting` when this landed. The implementation PR (#463) emits the
     key, but no live case reaches the guard, since each case that lists the end also has the
     step's acknowledgment end the interval. A harness test for it is a backlog row;
   - where a step lists `journal: unprotected_window_end` after a `submit_protective`, the harness
     acknowledges the step's unanswered protective submissions within the step, since that order
     says the broker accepted them before the end. `ends_after_protection` is pinned by
     `refcases::only_a_step_ending_after_its_protection_is_acknowledged_within_it` over RC-21's
     steps.
   - **Only the acknowledgment ends a waiting interval** (#463 round 1, B1; the implementation's
     choice). An interval waiting on new protection ends when the broker has acknowledged every
     order it waits on: `Accepted`, `PartiallyFilled`, `Filled`, `PendingCancel` or
     `PendingReplace`. A first draft also ended it once the fold's position read flat. That read
     the ingested position, while slice 2's OCO is sized from the broker's reported fills (DEC-346
     item 3), so it ended the interval in the step that sent the OCO, unacknowledged. The shortcut
     is dropped rather than repaired: every way a position goes flat while it waits already has
     its own end, so no path is left where a held position's interval ends before the broker
     acknowledges. The lifecycle of a waiting interval:
     - **On acknowledgment** of every awaited order, it ends at that step.
     - **On a flat position:** through the awaited order's own fill (`Filled` counts as
       acknowledged), or through an exit's sequence. That sequence's `unprotected_start` ends the
       waiting interval, and its own re-placement, with nothing left to cover, sends nothing and
       ends its interval at once. A position that goes flat any other way, such as activity a
       reconciliation adopts, leaves the interval open until the bound alerts.
     - **On a new start** in the instrument, it ends at that instant, and the new interval
       carries the instrument on.
     - **On a refusal** (`Rejected`, cancelled unacknowledged, `Unknown`), it stays open, and
       §5.4's bound alerts the owner once.
     - **On a restart,** the fold rebuilds what it waits on, and it stays open until the first
       step after the restart finds every awaited order acknowledged. `Input::Started` runs no
       `settle`, so that is the next input, never the restart itself.
     None of these ends it while shares the protection was sized for are uncovered at the
     broker.
3. **RC-22's `conduct_breach` step is asserted by nobody here** (the second ruling). The harness
   skips `fees_charged` and `conduct_breach` steps by name, since they belong to other streams and
   drive nothing here, so it asserts none of their `actions`. Steps 0 to 5 of RC-22 stay fully
   asserted, and this loosens no executor assertion.
   - **A partly unasserted case is not passing.** RC-22's step 6 expects
     `alert_owner: conduct_limit_breached` and `agent_mode: exits_only`. E6-11 owes them: its
     acceptance clause routes a conduct breach to `exits_only` with an owner alert, and names
     RC-22's `conduct_breach` step. So RC-22 stays pending in `crates/mandate-refcases/status.toml`,
     named for E6-11 in the status PR, never listed passing on this harness's account.
   - **The guard.** `refcases::a_case_with_an_unasserted_step_is_never_listed_passing` reads
     `status.toml` and fails if any driven case with such a step is listed passing.
     `the_guard_catches_an_unasserted_case_listed_passing` plants exactly that.

### Rationale

Both readings make the window the executor counts as unprotected longer, never shorter. A bound
measured from the earlier start, or held to the later end, fires no later than the literal one
would.

The RC-14 and RC-21 ruling ([5944523791](https://github.com/kunwarshivam/mandate/pull/174#issuecomment-5944523791))
named DEC-347 as the file for its readings. They are recorded here in DEC-348 instead, since
DEC-347 already held the harness's start readings when the ruling came; the coordinator ruled on
DEC-348 by name in #447's first round.

### Alternatives considered

- **RC-14: journal the start at the confirmation.** That matches the YAML literally, but it loosens
  the bound by the confirmation's latency. Rejected by the ruling.
- **RC-21: let the harness accept a journal action before the request it accompanies.** That counts
  as protected an OCO the broker has not yet accepted. Rejected by the ruling.

<a id="dec-349"></a>

## DEC-349: E7-4 slice 5's crypto readings: protection follows the ingested fill, net of the fee the buy withholds

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176: each item tightens a rule or resolves a gap by the reading that adds no risk |
| **Story or claim** | E7-4 slice 5 (crypto, DEC-36; trading-domain spec §5.4, §6.3), claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

1. **A crypto entry's stop-limit follows its ingested fill.** A crypto entry that carries its stop
   goes as a simple order. Its one GTC stop-limit (DEC-36) is placed once the entry is terminal
   and every fill the broker reported for it has been ingested. It is not placed on the order
   report alone. An equity bracket entry's filled quantity is the larger of its ingested fills and
   the broker's reported cumulative `filled_qty`, so its protection may follow the report (DEC-346
   item 3, E7-4 slice 2). A crypto entry's may not, because a crypto buy's position differs from
   its reported quantity by the fee it withholds in the asset.
2. **Net of the fee the buy withholds.** A crypto buy's fee is withheld in the asset
   (trading-domain spec §6.3: `fee_qty = round(gross × bps ÷ 10000, 9, half_up)`, `received =
   gross − fee_qty`; RC-07), and the executor learns it only when it posts
   (`FeesCharged crypto_asset`). A stop-limit placed or re-placed for a crypto buy therefore covers
   the position less `bought × the larger of the maker and taker rates`, since a broker fill does
   not say which applied. The stop never outlasts the position once the fee posts (rule 12). RC-20's
   re-placement for `0.59975` after a `0.1` add to `0.5` is that figure.
3. **The crypto add sequence** (§5.4, DEC-36: cancel the stop-limit, confirm, submit the add as a
   limit IOC, re-place for the new net quantity).
   - Once the gate allows a crypto add where a stop-limit rests, the interval's start records the
     entry, agent, stop and the add, and the resting protection is cancelled by id.
   - Once the cancels are confirmed, the gate re-runs and the add goes as a limit IOC.
   - Once the add is terminal and its fills are ingested, the stop-limit is re-placed (item 2).
   - An add the re-run denies, or one abandoned for age, still has the stop-limit re-placed.
   - A crypto add while another sequence runs in the instrument answers E7-4's stub. An add may
     always be refused (`AGENTS.md` rule 2), and it never goes beside protection it did not make
     way for.
4. **A crypto entry with no offset.** A crypto entry carrying a stop under no
   `crypto_stop_limit_offset` has no stop-limit to take, so it answers the stub at submission
   rather than going unprotected.
5. **The tests correction** (DEC-77, before the implementation PR). In `hand.rs`:
   - `a_crypto_add_is_a_limit_ioc_inside_the_sequence` seeds its position through the agent's own
     order, so its stop-limit has a single holder (DEC-160 (3)(b), #242's pattern). The bare fill
     left the add held `protection_unattributed`.
   - `a_crypto_position_carries_one_stop_limit_for_the_whole_position` and
     `a_crypto_stop_limit_is_re_placed_for_the_new_net_quantity` feed the entry's fill after its
     report (item 1).
   - The first now expects `0.49875`, the position less the taker fee `0.00125` (item 2), where it
     expected the gross `0.5`.
   - The second buys `0.123456789`, whose fee rounds at the ninth place. It expects the stop for
     `0.123148147`, recomputed in whole nano-units from §6.3's half-up rule, so a fee rounded down
     (`0.000308641`) fails it. On `0.5` the fee is exact and every rounding agrees (#448 round 1,
     B1).

### Rationale

Sizing from the report or the gross quantity would place a stop-limit for shares the account will
not hold once the fee posts, and the broker refuses a sell beyond the balance. Waiting for the fill
and netting the larger fee keeps Σ protective sells within the position. The window until the
stop-limit is placed is an unprotected interval like any other, bounded by `max_unprotected_s` and
alerted.

**The residual, stated plainly.** A broker fill does not say whether it was maker or taker, so the
stop is sized at the larger, taker, rate. When the fill was in fact a maker fill, the account
holds more than the stop covers, by `gross × (taker − maker)`: `0.0005` BTC on a `0.5` buy at the
test configuration's 15 and 25 bps. That remainder stays uncovered for as long as the stop rests,
and `max_unprotected_s` does not bound it, because the interval ends when the stop is placed.
Under-covering by that dust is the reading that adds no risk next to a stop the broker refuses for
over-covering (DEC-176). The real fix, carrying `liquidity` on `BrokerFill` or re-sizing at the
posted `FeesCharged crypto_asset`, is a backlog row.

### Alternatives considered

- **The maker rate, or the gross quantity.** Either can over-cover. Rejected.
- **Journal an asset-fee accrual at the fill.** That is the fee step's writer, another lane's work
  (DEC-305 to DEC-307), and this reading does not need it. Not taken here.

<a id="dec-366"></a>

## DEC-366: An ES-13 exception for the MC-E `lifecycle` driver, landed in three PRs sized to the `full` cap

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The coordinator's ruling on [#440](https://github.com/kunwarshivam/mandate/pull/440), under DEC-79: an engineering-size limit, not a safety rule. The founder may reverse it |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-317](earlier-303-599.md#dec-317), DEC-253 items 2 to 4 |

### Decision

1. **The exception.** `crates/mandate-refcases/src/mandate/escalation/lifecycle.rs` may exceed
   ES-13's 400-line split threshold (DEC-253 item 3, DEC-165 item 17's count). This follows the
   precedent of DEC-178's `risk_gate.rs` and DEC-250's `order_builder.rs`. The exception covers
   this file only.
2. **Three PRs, not one.** The driver lands in three PRs, each carrying its own live tests
   (DEC-253 item 2). Each is opened from `main` after the one before it merges, or is stacked on the
   one before it and retargeted to `main` when that one merges. The coordinator's ruling on #440
   stacked slice 2 that way. The three are:
   - **(a)** the shell, the ports, the context and the `ask` step;
   - **(b)** `response`, `now`, the step-up and the copies;
   - **(c)** `tick`, `fold`, `cancel`, `batch`, re-validation and the intent.

   Each PR's body says which steps it interprets and which still fail, naming the slice that owes
   them.
3. **The size holds** (DEC-398 overtakes this item's original time condition).
   - Each PR passes the mutation gate with 0 missed mutants. As first written, this item also asked
     that each PR fit `full`'s 30-minute timeout and that no PR raise the cap. Both runs of slice 1
     at that limit were cancelled with the mutation run unfinished, so DEC-398 gives the `full` job
     60 minutes. A diff that cannot finish in 60 minutes is split (ES-13).
   - No single PR adds more than about 700 non-test lines. A slice that would add more splits
     further; a slice is not split for time alone.
4. **MC-E18** stays pending, and [DEC-318](earlier-303-599.md#dec-318) stays Proposed for the founder, in all three.

### Rationale

The driver is one scene. A seam through the shell would separate it from its own checks, as
DEC-250 item 15 found for family B. But one PR's mutation run (220 mutants, 86 minutes at the
gate's `--jobs 2`) is too long for any reasonable `full` limit, so the file lands in three slices,
each within ES-13's size and finishing within DEC-398's 60 minutes.

### Alternatives considered

- **Strict slices of 400 lines (about five PRs).** Rejected: each would cut through the shared
  shell.
- **One PR with a raised cap.** Rejected: 86 minutes is beyond even DEC-398's 60. DEC-398 raised
  `full`'s limit from 30 to 60 minutes for every PR, which this decision first ruled out, after
  slice 1's runs at 30 minutes were cancelled unfinished.

<a id="dec-367"></a>

## DEC-367: E7-4 slice 5's trading-day part: the copied trading day and §5.4's re-placement before expiry

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176: each item tightens a rule or resolves a gap by the reading that adds no risk |
| **Story or claim** | E7-4 slice 5's trading-day part (DEC-260 (13)), claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

1. **The trading day is a copied fact** (journal spec §2). `TradingDayStarted` joins the copied
   events. The executor copies it onto the account stream with its `causation_id`, and the fold
   keeps the greatest `date` copied as the current trading day, so a re-copied or out-of-order
   older fact never moves it back. Protection placed from then on records that date as
   `created_on`. A protective order with no `created_on` (one placed before any trading day) is
   due: the first trading day re-places it once, and every later placement carries a date. Its
   creation date is unknown, so it is read as the oldest it could be, which only re-places earlier
   (#468 round 1, M3; DEC-176).
2. **The buffer day** (trading-domain spec §5.4). A GTC order expires `gtc_expiry_days` calendar days
   after its `created_on`. At a copied `TradingDayStarted`, the executor re-places it once at most
   `protective_replace_buffer_trading_days` trading days remain from that day up to the expiry. That
   day is §5.4's "first `TradingDayStarted` on or after the trading day that is N trading days before
   the expiry date". The trading days come from the US-equities calendar. A day the calendar cannot
   place (outside its range) counts as a non-trading day. Counting fewer trading days makes the
   count reach the buffer sooner, so a day mistaken that way can only bring the re-placement
   earlier (rule 3). Counting it as a trading day, the first draft, re-placed a day late around
   2029-01-01 (#468 round 1, M2). It matches `session.rs`'s refusal to read a date past the range
   as an ordinary trading day (the coordinator's ruling 5926945398).
   That low count alone would make an order placed near the calendar's edge due the day it is
   placed, and its replacement due again every trading day after. So two counts of the trading
   days in `[today, expiry)` are kept (#468 round 2, M1, the coordinator's rule):
   - **The low count** reads a day past the calendar as non-trading.
   - **The high count** reads it as trading unless it is a weekend.
   - **Which count decides.** An order is due when its low count is within the buffer, unless its
     low count was already within the buffer on the day it was created. Such an order is due only
     when its high count is. An undated order is judged by the low count alone (item 1).
   - **The effect.** The first order at the edge is re-placed early. Its replacement is not
     re-placed again until its high count, the only count available, reaches the buffer.
   - **One re-placement at the edge, and no loop** (#468 round 3, the coordinator's ruling). No
     order is due on the day it is created. An order whose low count was within the buffer on its
     creation day is not due on its next trading day. The one case outside that is an order whose
     low count was just outside the buffer on its creation day, such as one placed 2028-12-21:
     it is due on its next trading day, and its replacement, placed within the buffer, is judged
     by its high count and is not due again. So the rule allows exactly one extra re-placement at
     the calendar's edge, and no loop. A property test over the trading days from 2028-09-01 to
     2029-03-01 checks both clauses with its own count, and the 2028-12-21 case is pinned by name.

   The real fix is to extend the committed calendar well past the GTC window, which is a backlog
   row; this rule is the net under it.
3. **One instrument's re-placement is the whole of its protection.** In every instrument where
   protection rests at recorded prices, no exit sequence runs and no re-placement is under way, and
   where any resting protective order is due, every resting protective order is cancelled by id. An
   exit order working there does not stop it: item 4(a) re-places beside it, net of what it may
   sell. Protection with no recorded prices has nothing to be
   re-placed at, so it is left resting (#468 round 1, m2). The interval's
   `unprotected_start` records `replacing`, the entry, the agent and the prices. Once nothing rests,
   protection is re-placed for the held quantity at those prices, and the interval ends at the
   broker's acknowledgment (DEC-348 item 2). Cancelling only the due orders would leave two
   generations of protection to reconcile, for no gain in coverage.
4. **Re-placement beside a working exit, and no share left uncovered** (rule 12, §5.4's `fits`;
   #468 round 1, B1; the founder's decision on #468 of 2026-10-02, which replaces round 2's
   deferral).
   - **Every re-placement's size.** After an exit sequence, at re-protection for an unpriced exit
     (DEC-160 (12)), before expiry, and at a top-up, protection covers the held quantity less two
     things: what the protection still resting may sell, and what the instrument's own exit
     orders still working may sell (their remainders). An exit intent with no order yet counts
     nothing here, since its own sequence re-protects around it. A lone ladder between rungs
     counts nothing either (#468 round 4, M1, agent-accepted under DEC-176: it keeps more cover
     and adds no sell). Rule 12 bounds live sells, and a lone ladder's next rung is never sent
     beside resting protection: where protection rests, `lone_steps` starts a sequence for it,
     whose first step cancels that protection. So the protection and the remainder are never
     live together. Sizing protection net of the remainder instead would leave those shares
     with neither protection nor a sell that can go before the open, against DEC-260 (18). The
     oracle checks it: a lone ladder's rung is never sent while protection is live in its
     instrument.
   - **(a) Due protection beside a working exit is re-placed**, never left to expire because an
     exit is working. At a copied `TradingDayStarted`, protection that is due is cancelled and
     re-placed for what the working exits cannot sell, by the size rule above. Its interval is
     journaled before any cancel. Round 1's guard against re-placing beside a working exit
     existed because a re-placement for the whole held quantity would over-cover; the size rule
     removed that reason, so the guard is gone.
   - **(a′) Due protection with nothing to re-place it at is never silent.** Protection with no
     recorded prices (item 3) is not cancelled. On every trading day it is due, it is journaled
     (`ProtectionChanged expiry_unreplaceable`, naming the orders) and the owner is alerted
     (`protection_expiring`).
   - **(b) No share is left uncovered** once nothing is under way. After every step, in an
     instrument where no exit sequence or re-placement runs, any of these can leave the position
     under-covered:
     - an exit that ended unfilled in this step (cancelled, expired or rejected);
     - a protective order cancelled or expired in this step.

     When that happens (protection plus working exits below the held quantity), the shortfall is
     protected again at once (`re_cover`): an interval is started, then `re_place`. The prices are
     the resting protection's, or else the ended protective order's own (`OrderDetail.request`).
     The step's own drafts name the orders that ended, so a replay re-covers the same.
   - **Refused protection is not retried here.** Protection the broker refuses, or cancels while
     the interval still awaits its acknowledgment, keeps that interval open and bounded (DEC-348
     item 2). So a refusal never loops, and a broker cancelling faster than `max_unprotected_s`
     cannot restart the bound (#468 round 3, m1). The top-up covers a cancel after
     acknowledgment.
   - **Never silent** (#468 round 3, M1). Where the top-up finds the position under-covered with
     no prices anywhere to place from, it journals `expiry_unreplaceable` and alerts the owner
     (`protection_expiring`), once a step. So does a sequence or re-placement that ends with
     shares left to cover and no prices. That covers two cases: an exit that ends with nothing
     protecting the position and nothing recorded, and unpriced protection that expires at the
     broker, which was alerted only on the days before it was lost. The alert does not end the
     interval (#468 round 4, m2). Its `unprotected_end` ends the sequence or re-placement and is
     marked `uncovered`, and the interval stays open: nothing covers those shares, so §5.4's bound
     still governs them and alerts them at `max_unprotected_s` (rule 3). The next interval
     started in the instrument ends it, as it ends one waiting on an acknowledgment.
   - **A ladder between rungs is under way.** An order that ends as a step of one of its
     instrument's lone ladders (one for each exit, DEC-424) leaves the next rung or the park to
     follow (DEC-260 (14), (18)), so the top-up
     leaves it, as it leaves a sequence. Any other order that ends beside that ladder is topped
     up or alerted as usual.
   - **Agent mode.** A due day still cancels and re-places for an agent that is paused or
     stopped, and the top-up runs for one too. Rule 13 permits a hold on a paused or stopped
     agent; it does not require one. Holding here would let the protection lapse instead, which
     is the opposite of the safe default (rule 3).
   - **The oracle.** `Desk::within_position` has an under-cover arm: while no sequence or
     re-placement is under way by the oracle's own record, live protective orders and live exits
     cover the whole position.
     - **On round 2's head** it failed for (b), on a broker-side cancel of an exit beside an OCO
       for the rest (`an_exit_the_broker_cancels_leaves_no_share_uncovered`).
     - **Over the random scripts** it then found a third case: a passive exit's placement
       cancelled for a later exit that the bound then abandoned
       (`a_placement_cancelled_for_an_exit_since_abandoned_is_re_placed`). The protective-cancel
       trigger in (b) closes it.
5. **An exit takes the instrument over** (DEC-347). An exit that starts while a re-placement waits
   for its cancels runs its own sequence, and its own re-placement ends the interval. A replay
   rebuilds the re-placement from its journaled start, so a restart resumes it.
6. **RC-22's test goes live, and the case stays pending.** This change makes RC-22's steps 0 to 5
   reachable, and its Rust test passes on them. `cargo xtask ci pending` refuses a pending test that
   passes, so its `#[ignore]` goes. Its step 6 is still asserted by nobody here (DEC-348 item 3).
   So the case is not listed passing: `status.toml` keeps it pending for E6-11, and
   `refcases::a_case_with_an_unasserted_step_is_never_listed_passing` guards that.

### Rationale

§5.4 states when and how re-placement happens, but not how the trading days are counted, where
they come from, or what a re-placement covers. Each item takes the reading that re-places no later
and leaves no stale protection resting.

### Alternatives considered

- **Count calendar days for the buffer.** That is looser than §5.4's "trading days" around long
  weekends and holidays. Rejected.
- **Re-place only the expiring order.** That leaves two generations of protection to reconcile, with
  no gain in coverage. Rejected.
- **Top up at the next trading day, not in the same step** (item 4 (b)). Rejected: the shares stay
  uncovered until then, which is exactly the gap the founder's decision closes.
- **Top up the whole position, not the shortfall.** Rejected: protection already resting and exits
  still working cover their part. Re-placing all of it would over-cover beside them (rule 12), or
  cancel and re-place protection that is fine.
- **Top up anyway with no prices** (from a quote, or the last fill). Rejected: no stop was ever
  chosen for those shares, and inventing one is the platform deciding a trade. Journaling and
  alerting puts it before the owner.
- **Treat an unacknowledged cancel like any other** (round 3, m1). Rejected: a broker that cancels
  each placement before acknowledging it would restart the bound for ever, with no alert. A
  refusal was already excluded for the same reason.
- **Hold the re-placement while the agent is paused or stopped.** Rejected, as item 4 says: rule 13
  permits that hold and does not require it, and holding would let protection lapse.

<a id="dec-369"></a>

## DEC-369: E7-3's corporate-action tests correction: the expected difference, the applied action, and the reconciliation's cash

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79, DEC-77 and DEC-176: each item tightens a test toward the spec or corrects a fixture so the test can check what its title claims |
| **Story or claim** | E7-3 and E7-4 slice 5 (corporate actions), claim [#137](https://github.com/kunwarshivam/mandate/issues/137), umbrella [#174](https://github.com/kunwarshivam/mandate/pull/174) |

### Decision

1. **The pending split explains the expected difference only, in its own window and direction**
   (trading-domain spec §8.5, §11). §8.5's window runs from the application to the broker's posting:
   "Between application and the broker's posting of the action ... reconciliation treats the
   expected difference as `pending_corporate_action`". In that window the model is ahead of the
   broker: it has applied the split and the broker has not posted it. RC-04 step 4 is that state,
   with the model at 40 and the broker at 10.
   `hand::a_pending_corporate_action_difference_is_not_a_mismatch` held no position and folded only
   the preparation. It then asserted that a broker at 40 was no mismatch. That is outside the window
   and in the opposite direction: the model would understate the holding fourfold, and nothing
   would correct it.
   The test now holds 10 shares and folds the applied 4-for-1 split, then reconciles a broker still
   at 10: exactly RC-04 step 4. It does not require the window to open at the preparation or to stay
   open after the broker has posted. Either reading loosens §8.5, and neither is an agent's to take
   (DEC-176, #457 round 1). The other state, prepared but not applied with the broker already
   posted, is real after a crash. It is a backlog row under E7-3, with two safe outcomes, and it
   needs a recorded decision before a test.
2. **An applied action names which action it is** (journal spec §9's catalogue row for
   `CorporateActionPrepared` and `CorporateActionApplied` names `action` among its key payload
   fields). `coverage::a_split_multiplies_the_position_and_ends_the_pending_action` folded an
   applied split with no `action`. A split is not inferred from a `ratio`, since that is a guess, so
   the test now names `action: split`. This item says nothing about the other key fields:
   `ex_date`, and `ratio` on a preparation, stay optional to the fold.
3. **A reconciliation step reports the step's own cash** (DEC-347 item 3's rule, extended to every
   reconciliation step). The harness's `snapshot` for a `reconciliation` or `broker_position_update`
   step took the shared default account, whose cash is not the case's. A case such as RC-04 then
   fails on a cash difference that the case never states, and that would hide whether its position
   tolerance holds. Each snapshot now reports `account_after(step)`, the case's settled cash. The
   live `refcases::a_reconciliation_check_reports_the_steps_own_cash` pins it on RC-04's
   reconciliation step.

### Rationale

Items 1 and 2 make each test assert what its title claims under the spec's narrower wording.
Item 3 makes the harness meet a case with the account the case describes. None loosens an
assertion: every one of the four tests still fails at its stub on `main`. What the four do not
reach is a reverse split or a split with a fractional result, where the re-scaled protective
quantity shrinks. That is a backlog prerequisite for the implementation PR.

### Alternatives considered

- **Keep the hand test and except every difference while a split is pending.** That is the looser
  reading, and it hides a real mismatch for the life of the split. Rejected; were it wanted, it
  would be the founder's to decide, not a tests correction's.
- **Open the window at the preparation, with the broker ahead of the model.** That is the first
  draft of item 1's test. It understates the holding until something corrects it, and nothing is
  scheduled to. Rejected (#457 round 1, B1).
- **Let the fold infer `split` from a `ratio`.** That guesses which action an event records.
  Rejected (DEC-85).

<a id="dec-377"></a>

## DEC-377: E4-3, the fill model's session-of-asset-class refusal: the table, the error, and the call site

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (Claude Opus 5.5, taking over an `opencode` lane's tests PR; the coordinator assigned DEC-377 to DEC-379 at dispatch) under DEC-79: a reversible engineering decision that tightens the fill model's input checks and loosens nothing |
| **Story or claim** | E4-3 (minted here), the gap DEC-114 item 2 discloses; PR [#437](https://github.com/kunwarshivam/mandate/pull/437) round 1 (M2, M3, M4, minors 1 and 2) |

### Decision

1. **The story id E4-3 is minted** for the follow-up the tracker's known-issues row names (tests
   and implementation). E4-1 and E4-2 are complete, so a pending marker naming either could never
   expire with its story (DEC-137). The pending tests are marked `pending E4-3`, and the backlog
   carries the E4-3 row.
2. **The table.** Spec §4.3 gives crypto the continuous session alone, and a US equity the four
   New York sessions (overnight, pre-market, regular, after-hours) alone. A bar whose session its
   instrument's asset class never trades is refused in both directions: a continuous bar for an
   equity, and a regular, extended, or overnight bar for crypto.
3. **The error.** `SimError::SessionOffAssetClass(index)`, code `session_off_asset_class`, names the
   first such bar by its index in the sequence, as `check_bars` names the first bar it refuses.
   Its message states the rule for both directions. An empty bar sequence carries no session and
   is accepted.
4. **The entry point and the call site.** `check_sessions_of_asset_class(instrument, bars)` is
   public beside `simulate`. The implementation calls it from `simulate` **after** the order-policy
   loop (`check_order` over every order), not beside `check_bars`. The live
   `the_model_rejects_bars_and_orders_it_cannot_simulate` passes a crypto order on a
   regular-session bar and pins the order policy's own cause (`oco_on_a_fractional_instrument`,
   `day_order_on_a_continuous_instrument`); DEC-77 item 2 lets the implementation PR change tests
   only by deleting `#[ignore]` lines, so the order policy's cause must stay the first failing
   cause.
5. **The staging** (DEC-77 stage 1, DEC-137): the tests PR adds the error, the code, a
   `SimError::Unimplemented` variant (code `unimplemented`), and the stub returning it; `simulate`
   is untouched, so nothing that works on `main` changes.

### Rationale

The fill model is pessimistic wherever the spec leaves a choice (DEC-106), and a bar the spec says
cannot exist is not a choice: an equity stop on a continuous bar would fill on a session no equity
bar can carry. Refusing it is fail-loud on bad input, so it only removes fills, never adds one.
Naming the first bar keeps the error as useful as the other bar errors and is pinned by tests, so
the next change cannot drift it silently.

### Alternatives considered

- **Keep an asset-class session test inside the walk.** DEC-114 item 2 weighed and rejected it. A
  pre-check that refuses the whole input is not that alternative.
- **Call the check beside `check_bars`.** It pre-empts two pinned order-policy causes in a live
  test the implementation PR may not edit.
- **Refuse only a continuous bar for an equity** (the known-issues row's first wording). It leaves
  a crypto instrument filling on equity-session bars, which §4.3 equally rules out.
- **Keep `pending E4-1`.** The story is complete, so the marker could never expire with it.

<a id="dec-380"></a>

## DEC-380: E17-8's empty-basket follow-up: the named refusal and its tests PR's staging

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The stream-J builder, under DEC-176 (the reading that only tightens: a refusal that already existed gets a name, and no figure or division by zero can escape) and DEC-79 (the DEC-77 staging) |
| **Story or claim** | E17-8's follow-up (#435 review, minor 3, in the backlog); claim [#132](https://github.com/kunwarshivam/mandate/issues/132) |

### Decision

1. **The arm.** `score::basket_return` over a basket with no members refuses `ResearchError::EmptyBasket`, with the add-only ES-09 code `empty_basket`. Today that path reaches `Ratio::mean` of nothing and refuses a nameless `Num(DivisionByZero)`. A basket with members is unchanged.
2. **Its reach through `evaluate`.** The baseline is computed per scoreable thesis, before the count refusal. So a scoreable thesis against an empty basket refuses `empty_basket` at every registered minimum. With no scoreable thesis the basket is never read, and the scoreable set's own refusals keep their precedence: `empty_scoreable_set` at a minimum of zero (DEC-335), `window_not_closed` below a positive one. A thesis outside the registered window is still refused first (DEC-282 item 3).
3. **Staging (DEC-77 stage 1).**
   - The arm and its code land live, pinned by `the_score_codes_are_pinned`.
   - The empty-basket path is a stub returning `Unimplemented("basket_return", "E17-8")`, in its own function so the mutation gate exempts its body (DEC-337's reason).
   - The pending suite has two hand cases and one property:
     - `an_empty_basket_is_an_error`;
     - `an_evaluation_against_an_empty_basket_refuses_with_its_own_code`;
     - `no_basket_reaches_division_by_zero`, which asserts that each of its five branches is reached.
   - Each pending test fails at the stub with the stub's own report.
4. **The one live pin that changes.** `an_empty_basket_is_an_error` (#405) asserted only `matches!(refusal, ResearchError::Num(_))`. The named arm cannot satisfy that, and the implementation PR may only delete `#[ignore]` lines. So this tests PR rewrites the test to assert the arm and its code, which is stricter than `Num(_)`, and marks it pending under E17-8. On `main`, between the two PRs, an empty basket refuses `Unimplemented`: still a refusal, never a figure. Nothing outside `mandate-research` calls `basket_return` or matches on either code.

### Rationale

The backlog's follow-up asks for exactly this naming. Every input that refused before still refuses, so the change tightens the contract and loosens nothing.

Unlike DEC-336's broad reading, no decision deliberately chose the code being replaced. DEC-282 item 8 set the window refusals, not this one. The empty basket's `Num(_)` was the mean's own error, pinned by the tests PR as "the Num error". The pin is the one test whose assertion must change. The only form a tests PR can stage is a stricter pending test, and DEC-77 then has the implementation PR delete only `#[ignore]` lines.

### Alternatives considered

- **Keep `an_empty_basket_is_an_error` live and loosen it to "any refusal".** That weakens a live test (AGENTS.md "Do not"), even if only for the interim.
- **Leave the live pin as it is and add a separate pending test.** The implementation would then fail the live pin, and DEC-77 forbids the implementation PR from editing it.
- **Name the refusal inside `mandate-num`** (an empty-mean `NumError`, which would keep `Num(_)` true). That changes a shared crate's error for every caller of `Ratio::mean`, and the code would still be generic to the mean rather than to the basket.
- **Check for the empty basket in `evaluate`, before the per-thesis loop.** An empty basket would then refuse even when no thesis reads it, which overrides `empty_scoreable_set` and `window_not_closed` where they answer today. That changes pinned refusals rather than naming one.

<a id="dec-381"></a>

## DEC-381: A saved proptest failure seed fails `cargo xtask check`, tracked or not

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The stream-J builder, under DEC-79 (a reversible engineering and process decision), on the coordinator's ruling on #449's round-1 minor 1 |
| **Story or claim** | E17-8 (#449 review, m1); claim [#132](https://github.com/kunwarshivam/mandate/issues/132) |

### Decision

1. `cargo xtask ci lint` runs a `proptest-seeds` check. It fails on any proptest failure-seed file in the tree, in either of the two shapes proptest writes: `proptest-regressions/<file>.txt` at a crate root, which is its default, and `<file>.proptest-regressions` beside the source. The check fails whether the file is tracked or untracked, ignored or not.
2. `.gitignore` also ignores `proptest-regressions/`. Until now its only seed pattern, `*.proptest-regressions`, never matched the default shape.
3. The pending gate's own test (`a_property_that_shrinks_away_from_its_stub_is_reported_on_every_run`) now asserts through the same helper, so it covers both shapes and tracked files too.
4. No crate's `failure_persistence` changes. Live properties still save a failing case in `ci test`, where it helps the person debugging. The saved file must then be deleted before the next `cargo xtask check` passes.

### Rationale

Proptest replays a saved seed before it draws fresh cases. A seed left by an earlier run, on other code or under a planted bug, therefore changes which cases every later run tries (DEC-164), and a committed seed does that in every checkout.

#449 committed one. A plant's failing run wrote `crates/mandate-research/proptest-regressions/score.txt`, and nothing caught it:
- `.gitignore`'s pattern does not match the default shape;
- the xtask test checked only untracked files, and only in the other shape.

The rule was already written down (`.gitignore`: "never committed"), so this only moves it from the guided rung to the checked rung (AGENTS.md, "The trust ladder").

### Alternatives considered

- **`failure_persistence: None` in every property.** Eleven property files already set it, but twelve crates do not. It would also lose the saved case that helps a person debugging a live failure. The ruling excluded it.
- **Fail only tracked files.** This would leave a local seed free to perturb the next local run and the pending gate's verdicts. The check runs on a fresh checkout in CI, so failing untracked files costs nothing there.

<a id="dec-383"></a>

## DEC-383: a collar that cannot be computed is skipped alone; the exit keeps its limit and the other controls, and a proposal of zero keeps the error

| | |
|---|---|
| **Status** | Accepted; item 3 superseded by [DEC-401](earlier-303-599.md#dec-401) |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (stream G's builder; DEC-383 to DEC-385 assigned to it by the coordinator at dispatch) under DEC-79 and DEC-176: each item reads DEC-327 item 3 the way that adds no risk |
| **Story or claim** | E6-6, stream G, claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the backlog row "E6-6: route a discretionary exit whose collar cannot be computed"; follows [#436](https://github.com/kunwarshivam/mandate/pull/436) and DEC-327 |

### Decision

DEC-327 item 3 says a bound the arithmetic cannot compute "paces nothing" and the exit "goes as
proposed", reading the control as if its input were missing (DEC-163 item 5). Read precisely:

1. **Only the collar is skipped.** The participation caps still slice the exit and the close
   window still sends it as a marketable limit, exactly as they would beside a quote the collar
   computes and leaves alone. "Paces nothing" is the collar's own pacing, not the exit's.
2. **The exit keeps its own limit price.** No bound, tick or quote stands in for the one that
   could not be computed, and `Pacing::applied` does not name `Collar`. The quote is still usable
   (not stale, not crossed), so it is not §4.4's presumed halt: a market exit in the regular
   session outside the close window stays a market order, as it would beside any usable quote.
3. **A proposal of zero keeps the collar's error.** It reduces nothing, so `AGENTS.md` rule 13
   owes it no route, and allowing it would send an order of nothing. It gets the arithmetic's own
   error (`overflow` or `not_positive`), as it did before #436's stub. That a zero proposal over a
   computable collar is allowed is older than this fix and unchanged by it; the backlog row
   "E6-6: decide a proposal of zero quantity by name" carries it.
4. **Only the collar's two diagnosed errors are read this way, and only for a discretionary
   exit.** The failing call returns `NumError`, so no other gate error can be swallowed, and of
   its variants only `overflow` and `not_positive` are skipped (`conduct::routed`); any other,
   a `mandate-num` stub's `unimplemented` included, is still the gate's error (the #452 ruling's
   m3, a tightening). Check 5 for an opening or an increase is unchanged: an opening whose collar
   cannot be computed keeps its error and is never allowed.
5. **Every collar failure is covered**, not only the two DEC-327 item 1 names: the passive end's
   overflow, the sub-tick `not_positive`, and a configured `x` of one, whose aggressive end is
   zero.

Pinned by `properties::an_exit_over_extreme_figures_is_still_routed` (#436, now live) and, in
module (DEC-77 item 2, the #452 ruling), `gate::tests::an_exit_whose_collar_cannot_be_computed_is_paced_by_the_other_controls_alone`
(items 1, 2 and 5; its slice is recomputed in `i128` from the draw, a fractional cap included),
`gate::tests::an_opening_whose_collar_cannot_be_computed_is_never_allowed` (item 4),
`gate::tests::a_zero_quantity_exit_over_an_uncomputable_collar_is_not_routed` (item 3) and
`conduct::tests::only_the_two_diagnosed_errors_are_skipped` (item 4).

### Rationale

Skipping every control on a collar failure would let an exceptional quote lift the participation
caps (§9.6), a conduct control the exit is still owed; it adds market-impact risk and nothing
needs it, since the caps cannot fail (DEC-327 item 1). Reading the quote as missing for §4.4 would
turn a market exit into a marketable limit priced at the proposal's own limit, a figure the
proposal never meant as a marketable price. Each reading here routes the whole exit, never denies
it, and invents no figure (`AGENTS.md` rules 3 and 13). Item 3 keeps the existing answer for an
input that reduces nothing rather than invent a route for it.

### Alternatives considered

- **Skip all pacing on a collar failure** (the cheapest fix that passes #436's property): drops
  the participation slice and the close window. Rejected above; the new property fails it.
- **Price the exit at the bid**: a figure no rule produced; DEC-327 item 3 rules out a fabricated
  figure.
- **Keep the bound that could be computed**: for a sell only the passive end fails (except at
  `x = 1`), so the aggressive end `bid × (1 − x)` is computable. But it is computable exactly where
  the quote is extreme: at a bid near `6.6 × 10^19` the floor is near `6.5 × 10^19`, a sell limit
  no book fills, so raising the exit to it would deny in effect the exit rule 13 protects.
- **Treat the quote as missing everywhere**: changes `market_orders_barred` for a quote that is
  usable, so the gate would call a sane market a presumed halt.
- **Route a zero proposal too**: allows an order of nothing on a path that errored before; it
  changes a verdict this fix does not need to change.

<a id="dec-386"></a>

## DEC-386: E4-3's implementation: the session table as an exhaustive match, and the stub's `Unimplemented` variant retired

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (the E4-3 implementation lane; the coordinator assigned DEC-386 to DEC-388 at dispatch) under DEC-79: reversible engineering decisions inside a change DEC-377 already accepted, loosening nothing |
| **Story or claim** | E4-3, the implementation PR (DEC-77 stage 2) after the tests PR [#437](https://github.com/kunwarshivam/mandate/pull/437) |

### Decision

1. **The table is an exhaustive match.** `asset_class_trades` in `crates/mandate-sim/src/fill.rs`
   names every `Session` for every `AssetClass`, with no wildcard arm, so a new session or asset
   class fails to compile until someone decides which sessions it trades, rather than inheriting
   an accept or a refuse. The rows are DEC-377 item 2's: crypto the continuous session alone, a US
   equity the overnight, pre-market, regular, and after-hours sessions.
2. **`SimError::Unimplemented` is removed** with the stub that returned it (DEC-377 item 5). No
   other stub in `mandate-sim` returns it and no test names it, so a variant whose message says
   the check is not implemented would be dead surface the next agent copies.
3. **The hand test's doc comment is left as written.** The doc comment on
   `a_bar_on_a_session_its_asset_class_never_trades_is_refused_by_index` still says the check
   reports itself on the stub. DEC-77 item 2 lets this PR change `tests/` only by deleting the
   `#[ignore]` lines and adding the assertions its round-2 review asked for, so the sentence goes
   to the work tracker's known issues for the next `mandate-sim` tests change.

### Rationale

The check only refuses input spec §4.3 says cannot exist, so a missing arm must be a compile error
rather than a silent accept (DEC-106's pessimism). Item 2 follows the crate rule that a stub's
error leaves with its stub. Item 3 keeps DEC-77's line between tests and implementation.

### Alternatives considered

- **`(session == Continuous) == (asset_class == Crypto)`.** One line, but a new session would be
  accepted for equities without a decision. Rejected.
- **Keep `Unimplemented` for later stubs.** A later tests PR adds its own when it needs one, as
  DEC-137 lets it. Rejected.

<a id="dec-389"></a>

## DEC-389: The account-stream writers' implementation: what is wired now, and what waits

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The agent under DEC-79 and DEC-176: reversible sequencing of a safety-critical implementation (DEC-77). Nothing here loosens a spec or a schema; every part that would is left to DEC-360 (Proposed, the founder) |
| **Story or claim** | E7-10, stream K's account-stream writers, the implementation PR after #441, branch `agent/k-account-stream-writers-impl` |

### Decision

1. **Implemented, and wired.** `payload::risk_clock_stamp` writes §9.2's whole-second timestamp,
   and `Batch::journal` stamps it on every account-stream event in place of integer seconds
   (DEC-306 item 2). `reconcile::fee_step_pause_and_alert` journals the pause, then alerts once
   under `reconciliation_fees`, naming the snapshot when it recorded and the pause's own
   `AgentModeApplied` when it did not (DEC-305 items 3 and 4). `fees` calls it, and no longer
   holds the pause behind its own snapshot's `?`: a snapshot the fee step cannot record (its
   payload or its fold refused) is left out of the batch, the batch's working copy is restored to
   before it, and the pause and the alert run all the same.
2. **Implemented, not yet wired: the fee step's snapshot payload.** `fee_step_snapshot_fields`
   writes §9.2's fourteen members with the three cash members `null` (DEC-305 item 1), and its pin
   and #441 round 2's m1 pin (a second account, every value differing from the vector's) are live.
   `fees` keeps journaling the reduced form, because the live pin
   `the_fee_steps_snapshot_is_never_refused_for_its_members` asserts that the fee step's own
   snapshot carries no `model_cash` (`[false, true]`), and this PR may change no live test (DEC-77).
   The writer goes into `fees` in the change that registers `AccountSnapshotRecorded`, which must
   correct that pin and `mandate-journal`'s `account_snapshot_recorded_waits_for_the_fee_step_writer`
   together (DEC-261 item 7, DEC-303 item 4). Until then the schema stays unregistered, so nothing
   is refused that is accepted today.
3. **Implemented, not yet wired: `IntentReceived` and `OrderSubmitted`.** `intent_received_fields`
   writes §9.1's nine members and `order_submitted_optional_fields` writes `limit_price`, `null`
   when the order has none (DEC-307), and their four pins are live. `received` and `journal_rung`
   keep their own pairs, which is DEC-360's conservative option: wiring them would either drop the
   members the fold replays (the protective prices, the submission's agent, purpose, and legs), so
   a protected opening would lose its protection after a restart (rule 13), or keep those members
   beside the new ones, a writer gaining members DEC-360 holds for the founder. `received` also
   cannot write the proposed `tif` until `IntentHandoff` carries it, and adding that field changes
   the tests' own handoffs, which is a tests change (DEC-77). Both writers are wired in with
   DEC-360's ruling, through a tests PR that brings the proposed `tif`.

### Rationale

The pins test the writers, so they all go live here and the mutation gate reaches every writer
body. Wiring goes as far as it can without changing a live test or a member list the founder holds:
the stamp and the pause tail, which DEC-360 item 3 says do not wait. The pause is the step's risk
reduction, so taking it off its snapshot's `?` is the change rules 3 and 13 ask for first.

### Alternatives considered

- Wire `fee_step_snapshot_fields` into `fees` now and correct the live pair pin. Rejected: an
  implementation PR changes no live test (DEC-77), and the pin's partner in `mandate-journal` moves
  with the registration, so the two belong in that change.
- Wire the intent writers and keep the old members beside them. Rejected: the writers would gain
  members, which DEC-360 holds for the founder, and the fold would have to read two names for each
  member.
- Leave the pause behind the snapshot's `?` until the snapshot is wired. Rejected: the pause is
  risk reduction and DEC-305 item 3 already decided it never waits for the snapshot.

<a id="dec-390"></a>

## DEC-390: The executor's fold reads `risk_clock` in both forms the account stream has written

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The agent under DEC-79 and DEC-176: it tightens what the fold accepts as a timestamp and keeps replay of journals already written, adding no risk |
| **Story or claim** | E7-10, stream K's account-stream writers, the implementation PR after #441, branch `agent/k-account-stream-writers-impl` |

### Decision

1. The fold reads an account-stream event's `risk_clock` as §9.2's whole-second §4.7 timestamp
   (what `Batch::journal` writes from DEC-306 on), or as the integer seconds the stream wrote
   before it, which the golden journal (`crates/mandate-executor/tests/golden-journal.json`,
   `FOLD_VERSION` 1) carries. Both name the same second, so a replay folds the same state from
   either.
2. A timestamp that is not canonical §4.7 text or is off the whole second is no clock: the event is
   refused with `risk_clock_missing`, never rounded. The clock's never-backwards check is unchanged.
3. The integer form is read, never written: `payload::clock` is test-only, kept for the in-crate
   tests' hand-built payloads.
4. **Owed before E7-2, E7-3, and E7-4 go live.** The pending property oracles in
   `crates/mandate-executor/tests/properties.rs` read `risk_clock` through `number()`, which is
   `Value::as_int`, at `:398` (`ProtectionAccountant::of`), `:2637`, and `:2363`. Against the
   timestamp stamp they read nothing, so the planted-bug-14 oracle
   (`no_interval_exceeds_the_limit_without_an_alert`) and the planted-bug-19 oracle
   (`no_submission_carries_an_intent_older_than_its_maximum_age`) would pass while checking nothing.
   Those readers need a timestamp-aware read before those stories delete their `#[ignore]` lines;
   this change may not edit them (DEC-77). The backlog's stream K follow-ups carry the row.

### Rationale

The golden journal is a test fixture this PR may not change (DEC-77), and journals written before
the stamp changed must still replay. Reading both forms of the same second changes no folded
state; refusing an off-second timestamp keeps the fold from inventing a clock.

### Alternatives considered

- Read only the timestamp. Rejected: the golden journal and every in-crate test payload carry
  integer seconds, so the fold would refuse its own history.
- Bump `FOLD_VERSION` and rewrite the golden journal. Rejected: the folded state does not change,
  and the golden journal is a test fixture.

<a id="dec-392"></a>

## DEC-392: The rule-13 oracle reads the calendar's range at each instant, not at the script's start

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the agent (Claude Opus 5.5, stream K) under DEC-79: a reversible engineering decision about a test oracle, which takes the reading DEC-260 (13) already states and loosens no rule |
| **Story or claim** | E7-4 (stream K), claim [#137](https://github.com/kunwarshivam/mandate/issues/137); the failure reported on [#445](https://github.com/kunwarshivam/mandate/pull/445) on 2026-10-02 |

### Decision

1. **The executor is right; the oracle was wrong.** A US-equity risk exit placed at 2018-01-01
   00:00 ET (the New Year holiday, in the overnight session) is held `session_closed`. DEC-260 (13)
   says exactly this: while no v1 session is open (overnight per DEC-30, a weekend, a holiday), an
   equity exit of any purpose is held `session_closed`, rule 13's broker hold, until the
   calendar's next pre-market open. Trading-domain spec §4.3 allows no orders in the overnight
   session, and a holiday has no session at all. No venue can take the order, so the broker
   holds it, which is one of the holds `AGENTS.md` rule 13 allows. The executor's source does not
   change.
2. **The oracle's defect.** `protection::sequence_tests::rule_13_script` decided once, from the
   script's start, whether the calendar covers the script. Before the calendar's first date
   (2018-01-01) every instant reads as the regular session (DEC-260 (13)). A script from the eve
   of 2018 (`EVE_OF_2018`, Sunday 2017-12-31 12:00 ET) can tick past that date. The oracle then
   still read every instant as the regular session, and it refused the executor's correct hold.
   The property failed only when proptest drew that start and enough long ticks, so CI's `fast`
   job went red at random on every PR.
3. **The fix.** The oracle asks at each instant whether the calendar covers it
   (`Desk::covered(now)`, from `CALENDAR_FROM`, 2018-01-01 00:00 ET). Its own list of trading days
   gains the calendar's first week, 2018-01-02 to 05. Those are the trading days a script from
   the eve of 2018 can reach: 63 moves of at most two hours each end by Friday 2018-01-05 18:00 ET.
   The holds the oracle accepts are unchanged: `session_closed` and `session_unknown`, and only
   while no v1 session is open by the oracle's own reading. The generator is unchanged, and so are
   the property's cases and starts.
4. **The regression.** CI's minimal input is the always-run test
   `rule_13_holds_for_a_risk_exit_on_the_new_year_holiday_overnight`. It fails on the old oracle
   with the reported error and passes on the fixed one.

### Rationale

The spec and the accepted decision agree, so this is reading (b) of the dispatch: no venue takes
the order then, and the exit waits for the broker. The fix states exactly that hold: from the
calendar's first date, while it names no v1 session open. It accepts nothing wider. A sweep over
every weekday holiday and every half day in the calendar (2018 to 2028), at each session boundary,
for every exit purpose, equity and crypto, found the executor holding and releasing exactly as
DEC-260 (13) says. That covers 1,115 instants. No other case fails.
Twenty thousand random scripts from instants around the calendar's first date pass the fixed
oracle. The fixed oracle still catches three executor plants: a holiday read as the regular
session, an overnight session read as extended hours, and a regular session read as closed.

### Alternatives considered

- **Release the exit on a holiday or overnight (change the executor).** Rejected. It contradicts
  DEC-30 and DEC-260 (13), and it would send an order no v1 session can take.
- **Drop the eve-of-2018 start, or keep scripts inside one side of the date.** Rejected. That
  narrows the generator, which weakens the property.
- **Commit a proptest seed file.** Rejected. A named test that always runs pins the case, and the
  dispatch forbids regression files.

<a id="dec-395"></a>

## DEC-395: E6-10's harness implementation: the short refusal and the `crypto_status` read

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the stream G builder (claim [#123](https://github.com/kunwarshivam/mandate/issues/123); the coordinator assigned DEC-395 to DEC-397 at dispatch) under DEC-79 and DEC-176: reversible engineering readings inside DEC-314 and DEC-315, each of which only refuses more |
| **Story or claim** | E6-10, the implementation PR (DEC-77 stage 2) after the tests PR [#438](https://github.com/kunwarshivam/mandate/pull/438) |

### Decision

1. **The stubs' bodies.** `held_overnight` in `crates/mandate-refcases/src/trading_domain/gate.rs`
   refuses a negative `SignedQty` with DEC-314 item 1's text and holds any other quantity as its
   shares. `Gate::crypto_active` stores `false` in the gate's `crypto_active` field for any
   `crypto_status` other than `ACTIVE` and leaves the field as it is for `ACTIVE`, so a later
   `ACTIVE` never lifts a detected inactivity (DEC-315 items 1 and 2). Only the two bodies change;
   the field, the method and the snapshot's read are #438's.
2. **What counts as `ACTIVE`.** Only the string `ACTIVE`, spelled exactly, is active; another
   case (`active`), a padded or an empty string is inactive and denies crypto openings at check 1.
   A `crypto_status` that is not a string is refused with ``account `crypto_status` is not a
   string``, as the account's `status` is, rather than read either way. Both readings match how
   the harness already reads `status` and only refuse more (rule 3).
3. **The stub pins are swapped as #438 named.** `only_a_negative_quantity_is_a_short_to_the_fold`
   expects DEC-314's text for the refused magnitudes, and adds `-0.000000001`, the smallest short
   a case can state (`QTY_SCALE` 9), so a refusal narrowed to `|qty| >= 0.00000001` fails it (#438
   review, round 3, m6). The in-module `an_initial_crypto_status_reports_its_stub_until_e6_10` and
   `an_account_update_s_crypto_status_reports_its_stub_until_e6_10`, and their
   `CRYPTO_STATUS_STUB`, are replaced (DEC-315 item 3).
4. **Their replacements pin the read live, in-module (DEC-77's 2026-09-27 amendment).**
   `a_crypto_status_never_refuses_an_equity_decision_or_a_crypto_risk_exit` runs RC-09 as written
   under `ACTIVE` and `INACTIVE`, initial and in an update: its AAPL opening is still denied by the
   day-trade budget and its BTCUSD risk exit is still allowed (`AGENTS.md` rule 13).
   `a_crypto_status_that_is_not_a_string_is_refused` and
   `only_an_exact_active_crypto_status_leaves_crypto_openings_open` pin item 2.
5. **The pending tests' doc comments under `tests/` stay as written.** Three doc comments in
   `crates/mandate-refcases/tests/trading_domain_gate_harness.rs`, and the in-module
   `a_short_position_is_refused_where_a_day_trade_count_is_expected`'s, still say the read or the
   refusal is pending E6-10. DEC-77 item 2 lets this PR change those tests only by deleting their
   `#[ignore]` lines, so the sentences go to the work tracker's known issues for the next
   trading-domain harness tests change, as DEC-386 item 3 did.
6. **DEC-316 is not used.** The lane that wrote #438 reserved DEC-316 for this record. A PR edits
   only its own decision files (the decisions README), so this record is DEC-395 and DEC-316 stays
   Reserved for its holder, or for the coordinator, to release.

### Rationale

DEC-314 and DEC-315 already decided the behaviour, and #438 staged everything but the two bodies
so that this PR would change nothing else. Items 2 and 4 close claims the bodies make that no
pending test pins: the exact spelling, the non-string value, and that the status never touches an
equity decision or a crypto exit.

### Alternatives considered

Reading `crypto_status` case-insensitively (a broker that sends `active` would then be trusted on a
spelling §7.3 does not give); reading a non-string as inactive (silently accepts a malformed
case); deleting the two stub pins without replacements (the read would lose its live pins on the
exit and equity paths); editing the `tests/` doc comments here (DEC-77 item 2).

<a id="dec-400"></a>

## DEC-400: family B's trim arm: how the harness compares a `trim_to_target` case

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the stream G builder (claim [#123](https://github.com/kunwarshivam/mandate/issues/123)) under DEC-79: reversible readings of the shared harness, inside DEC-250's composition; none changes a case, a spec or the gate |
| **Story or claim** | E6-4, the family-B trim arm (DEC-250 item 11, DEC-399 item 6) |

### Decision

1. **The composition.** On a `trim_to_target` base, `trim_first` in
   `crates/mandate-refcases/src/mandate/order_builder.rs` first asks `mandate_risk::trim_proposals`.
   It passes DEC-250 item 4's scene, and the `scale_sizes` rung at the case's factor is active for
   `scale_active_s`. If the gate proposes a trim of the case's instrument, that trim is the case's
   action. If it proposes none, the builder proposes and is judged as on any other base (§6.2
   step 1, DEC-250 item 1). The session is `session_at` at `now` (DEC-250 item 3).
2. **The guards come from the case.** `trim_proposals` names no guard, so the harness derives
   `reference/mandate/ref.py`'s guards from the case's own figures, in `ref.py`'s order. The band
   and the excess are taken from the case's stated `cap` and `current_mv`, after both have been
   compared with the builder's (#478 review, m2). The guards are:
   - `rung_not_confirmed`: `scale_active_s` is below `breach_confirm_s`;
   - `holding`;
   - `regular_session_only`: an equity outside the regular session;
   - `below_minimum_order`: the trim's quantity is under the instrument's `min_order_size`. A case
     may state `min_order_size`, and without it the minimum is the case's `qty_increment`.
     (Amended by DEC-399 item 8 in #498. This line read `min_order_usd`, the dollar minimum, which
     `ref.py` still reads until the reference PR moves it to the same quantity minimum.)

   A trim is due when the factor is below one and the market value is over `factor × cap` by at
   least `rebalance_band × cap`. When no trim is due, `trim_withheld` must be absent. When one is
   due, the gate's answer and the guards must agree: a trim only when no guard holds, and none
   only when one does. In the second case, the stated `trim_withheld` must list exactly those
   guards. A disagreement fails the case and names both answers. That includes an empty answer
   where a trim is due and no guard holds, so a case stating `trim_withheld: []` cannot pass on a
   gate that never trims (#478 review, m1).

   **Which guards this arm judges.** All four of `ref.py`'s guards are judged, each one alone:
   - `rung_not_confirmed` by MC-B30;
   - `holding` by MC-B30 doctored confirmed and Holding;
   - `regular_session_only` by MC-B31 doctored not Holding;
   - `below_minimum_order` by MC-B30 confirmed at a `min_order_size` of 4 shares, and MC-B17 at 3
     shares (trims) and 4 (fails naming `trim_withheld`).

   MC-B31 states two guards, and either would mask the other (#478 review, M1). MC-B17 under a
   1000-dollar `min_order_usd` still trims, since the dollar minimum is §8.3 step 5's and not the
   trim's (DEC-399 item 8; `a_trim_the_gate_and_the_guards_disagree_on_fails_naming_both`).
3. **What a trim states.** These members, and no others:
   - `cap` and `current_mv`, as the builder computes them;
   - `action: sell`, `origin: risk_engine` and `reason: trim_to_target`;
   - `purpose: risk_exit`, which is also the gate's;
   - `qty`, the gate's trim;
   - `limit_price` at the bid. `TrimProposal` carries no price, so this member pins the case
     file's own consistency, not the gate (#478 review, n1);
   - `order_usd`, `qty × bid`;
   - `autonomy`, from `mandate_builder::classify` on a `risk_exit` action, which §6.2 step 3 makes
     AUTO `builtin_risk_reducing`. Step 3 reads only the purpose. The action's other facts are the
   case's, and `first_trade_in_instrument` is `false`, because a trim sells a held position. That
   is also what `!has_prior_fill` gives on every trim case, since a held position makes the case
   file's `has_prior_fill` default true (#478 review, n5).

   The trim reaches no dry run and no builder figure, so the case states neither. A builder action
   on a trim base states no `origin`.
4. **The harness's oracle.** All 32 family-B cases are in `PASSING`, and the `OWED` table is
   deleted, because an empty lookup is code no case can drive (DEC-250's precedent). The in-module
   sweeps now run over the trim cases too, and their fixture counts move with them:
   - 13 holds;
   - 976 doctorings;
   - 1257 refused inputs;
   - 32 × 4 + 59 + 2 plants.

   The trim-guard plant ("a guard stated on a mandate that does not trim") skips trim bases, where
   the guards are inputs. The sibling sweep gains `origin` (`risk_engine` and `builder`), a trim
   reason's hold-reason sibling, and `trim_withheld`.

### Rationale

The case file states the trim, its guards and the builder's answer after it. Without an arm that
compares all three, four cases owed nothing to the gate. Deriving the guards from the case keeps
the gate's API as #136 fixed it. It also judges the gate's empty answer against an expectation
computed elsewhere, rather than explaining that answer after the fact.

### Not done

- **`ref.py`'s `factor < 1` guard** is read in two places: `ref.py` and this arm's
  `order_builder.rs::trim_guards`. A rung factor of 1 cannot be written at all: the schema's
  `open_fraction` excludes it, and `scaling_rung` requires the case's factor to be a rung's, so it
  stays (#123, the coordinator's ruling). The dollar minimum that row also named is replaced by the
  quantity minimum (DEC-399 item 8).
- **MC-B17 and MC-B30 to MC-B32 are still `pending` in `status.toml`** until the status-only PR.

### Alternatives considered

- **Widen `trim_proposals` to return the guards.** That is an API change, which needs a DEC-77 tests
  PR. The executor needs only the trims, so it would add surface for the harness's sake.
- **Compare only the trim and leave `trim_withheld` unread.** That breaks DEC-85's rule that every
  member is read, so a wrong guard list would pass.

<a id="dec-401"></a>

## DEC-401: the gate refuses a proposal of zero quantity by name

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | the stream G builder (claim [#123](https://github.com/kunwarshivam/mandate/issues/123)) under DEC-79 and DEC-176. The reading only refuses more: it turns an allow of nothing into a refusal and weakens no safety rule |
| **Story or claim** | E6-6, the backlog row "E6-6: decide a proposal of zero quantity by name", which DEC-383 item 3 deferred here. The tests PR is `agent/g-e6-6-zero-qty-tests`, and the implementation follows (DEC-77) |

### Decision

1. **`mandate_risk::evaluate` refuses a proposal whose quantity is zero with
   `GateError::ZeroQuantity`** (code `zero_quantity`). The refusal comes before any check:
   - for every origin and both sides;
   - flat or held;
   - in every agent mode and on both passes;
   - ahead of every other refusal, an unread working universe's included.

   A refusal is not a verdict (ADR-0001 ES-09). The decision depends on the quantity alone, so
   the smallest quantity above zero, 0.000000001, is decided exactly as before.
2. **Why a refusal and not a denial.** An order of zero is no order. It reduces nothing, so
   refusing it denies no reduction (`AGENTS.md` rule 13). It adds nothing, so refusing it adds no
   risk (rule 3). An allow would hand the broker an order it refuses, and the E6-6 exit-routing
   fix's differential matrix found 6,840 such allows on `main`. A denial would need a reason code
   registered in the trading-domain spec for a case that is no trading decision. The refusal
   needs none, and it is named in the crate's own error codes.
3. **It replaces DEC-383 item 3.** A zero proposal over an uncomputable collar used to keep the
   collar's arithmetic error (`overflow` or `not_positive`). It is now refused as `zero_quantity`
   before the collar is reached, like every other zero proposal. DEC-383's status says so. Two
   tests pin it:
   - the pending grid in `tests/hand.rs`, whose rows include DEC-383's three uncomputable quotes;
   - the in-module property `gate::tests::a_zero_quantity_exit_over_an_uncomputable_collar_is_not_routed`,
     which asserts the stub's own refusal over those quotes. A collar error therefore satisfies
     neither (#486 review, M1).
4. **The DEC-77 sequence.** The tests PR adds three things:
   - the variant;
   - a stub, `gate::refuse_nothing`, which already refuses a zero proposal as
     `Unimplemented("the zero-quantity refusal", "E6-6")`;
   - two pending tests in `tests/hand.rs`.

   The stub's refusal is already stricter than `main`'s allow. The precedent is DEC-108 item 7's
   `SimError::ZeroQuantity`: the simulator refuses an order of nothing in the same way. DEC-129
   item 29 is not the precedent, because it fails closed for adding risk only (#486 review, m3).
   The in-module property is generated by `proptest!` and cannot be pending, so in the tests PR it
   asserts the stub's `Unimplemented("the zero-quantity refusal", "E6-6")` exactly. **The
   implementation PR must:**
   - return `GateError::ZeroQuantity` from the stub;
   - delete the two `#[ignore]` lines;
   - sharpen that in-module property to `GateError::ZeroQuantity`.

   The last is a requirement, not an option.

5. **The constraint on the gate port.** This change is safe because no caller presents a zero
   quantity to the gate today. The §9.1 gate port into `mandate-executor`
   (`crates/mandate-executor/src/gate.rs`, not wired yet) inherits that as a constraint. Its
   adapter must either filter a zero quantity before the gate or treat `ZeroQuantity` as nothing
   to do. It must never let the refusal abort a multi-order step: a flatten, an exit-price ladder
   rung or an exit sequence. The backlog row "E7: the gate port's adapter handles
   `ZeroQuantity`" carries it to that story (#486 review, m4).

### Rationale

A gate that allows an order of nothing tells the executor to send something the broker refuses,
and the journal records an allow that was never an order. Refusing one by name keeps the decision
log honest, and it costs nothing, since no zero order changes any position.

### Alternatives considered

- **Deny with a registered reason code.** That needs a trading-domain spec change for a non-order.
  A denial is also a trading verdict the journal and surveillance count, and a non-order should
  not be one.
- **Keep allowing a zero and let the executor drop it.** Then the gate's decision would not be the
  decision. Every consumer would have to know that an allow can mean nothing.

<a id="dec-402"></a>

## DEC-402: The `AccountSnapshotRecorded` registration, tests first: a stub, and the three ordering pins made pending

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79) |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The agent (stream L), under DEC-79 and DEC-176: reversible sequencing of a safety-critical change (DEC-77). Nothing here loosens a spec or a schema |
| **Story or claim** | E7-10 (the part DEC-261 item 7 held back), DEC-389 item 2; stream L, claim [#171](https://github.com/kunwarshivam/mandate/issues/171); tests PR [#467](https://github.com/kunwarshivam/mandate/pull/467), implementation on `agent/l-snapshot-impl` (items 6 and 7) |

### Decision

1. **Why now.** DEC-261 item 7 held the registration until stream K's fee-step writer conformed. #456 (DEC-389, DEC-390) implemented it: `fee_step_snapshot_fields` writes §9.2's fourteen members with the three cash members `null`, and `risk_clock` is stamped as a whole-second timestamp on every account-stream event. DEC-389 item 2 left two things to the change that registers the schema: wiring that writer into `fees`, and correcting the live pair pin. This decision is that change's tests stage.
2. **The stub.** `control::governs` routes `AccountSnapshotRecorded` on the account stream to §9.2, and `control::payload` answers it with `InvalidReason::Unimplemented` (`unimplemented`) at `payload`. The variant comes back for this tests PR and goes with the implementation, as with E7-10's first stub. The snapshot was refused as `unknown_schema` before, so it is still refused: nothing that was accepted is now refused, and nothing that was refused is now accepted.
3. **DEC-303 item 4's three live pins become one live and three pending tests.**
    - `mandate-journal`: `account_snapshot_recorded_waits_for_the_fee_step_writer` becomes `account_snapshot_recorded_is_routed_to_section_9_2`, a live test that the snapshot is never `unknown_schema` on the account stream. It joins a pending test, `account_snapshot_recorded_refuses_an_unlisted_member`: `schema` at `payload.unregistered`, as on §9.2's eleven other pairs. `stream_types_and_required_config_refs_match_the_spec` counts the pair as closed by §9.2.
    - `mandate-refcases`: `snapshot_drafts_stay_unregistered` becomes the pending `an_account_snapshot_is_closed_and_checked_by_rule_24`. It turns on, in Rust, the vectors' own `expect` for all 15 snapshot drafts: 2 valid and 13 invalid, including rule 24's. It also requires the two snapshot base drafts (`snapshot_reconciled`, `snapshot_fees`) to append.
    - `mandate-executor`: `the_fee_steps_snapshot_is_never_refused_for_its_members` becomes pending, and stricter. Each of the executor's own snapshots (the fee step's, then a cash comparison's) must be accepted whole, and the fee step's must write `model_cash` as `null`. It parses before it compares members, so it fails at the journal's stub first. Once the schema registers, it fails at `payload.model_cash` until `fees` journals `fee_step_snapshot_fields`. So the registration and the writer cannot land apart, which is DEC-261 item 7's ordering made checkable.
4. **Proof the pending tests can pass and do not pass on nothing.**
    - A throwaway implementation (not committed) makes all four tests pass and leaves the three crates' 1012 live tests green. It registers the fifteen members and rule 24, with `mandate-num`'s `Usd` for the band, and journals `fee_step_snapshot_fields` in `fees`.
    - Five plants on it are each caught: the fee step not wired; rule 24's band strict; rule 24's null check off; the schema open to an unlisted member; and `risk_clock` nullable.
    - Two do-nothing stubs pass none of the three pending tests: one that accepts every snapshot, and one that refuses every snapshot as `schema`.
5. **The implementation PR** registers the schema with rule 24 in `mandate-journal`, wires `fee_step_snapshot_fields` into `fees` (dropping its `allow(dead_code)`), removes `InvalidReason::Unimplemented`, and deletes the three `#[ignore]` lines. It also turns on the snapshot drafts in `control::tests`, which `SNAPSHOT` holds back today. Its readings, such as the decimal type rule 24 compares in, are recorded then.

6. **Rule 24 compares exact `Usd`** (the implementation). `mandate-journal` gains `mandate-num`, a layer-0 workspace crate, so `cash_band` ≥ 0 and |`cash` − `model_cash`| ≤ `cash_band` are decided on exact decimals, never on text or floats.
    - **Out-of-range amounts.** §9.2 bounds no decimal's size, and §4.6 admits more than `Usd` holds. Both cases are refused, never compared approximately. This is the reading that refuses (DEC-176), and it can only refuse a snapshot no writer produces: the executor's amounts are `Usd` already, and step 4 decides `cash_in_band` with the same `Usd` comparison.
      - **An amount `Usd` cannot hold exactly** (a mantissa past 96 bits at its scale) is refused as `schema` at that member.
      - **A difference that overflows although both amounts fit** is refused as `schema` at `payload.model_cash` (#473 round 1, n1). The path names the subtrahend, the member the executor computes; `cash` would serve as well, and the in-module test pins the choice.
    - **Alternative considered: `mandate_num::UsdExact`** (#473 round 1, n2). It is public, parses at any scale its 256-bit magnitude holds, and has `checked_sub` and `is_below`, so it could compare every §4.6 decimal exactly with no refusal at all. `Usd` was kept because both sides of the executor's comparison are `Usd`, so the journal decides with the same type the writer used. No §4.6-valid draft a writer produces falls between the two. Changing to `UsdExact` later would only accept more, so it would need its own decision.
    - **The reference.** `reference/journal/control.py` compares in Python's `Decimal`. The two can differ only on an amount past `Usd`'s range, which no vector holds.
7. **What the implementation wires.**
    - **The journal.** `control::schema` registers the fifteen members, and `control::payload` applies rule 24 after rules 17 to 23, on a well-typed payload only. `InvalidReason::Unimplemented` goes.
    - **The executor.** `fees` journals `fee_step_snapshot_fields`, so the fee step's own snapshot carries the three cash members as `null`. Its `allow(dead_code)` goes.
    - **Pending tests.** The three are live, with only their `#[ignore]` lines deleted.
    - **In-crate tests.** `control::tests` now reads every snapshot draft. The valid and invalid counts' lower bounds are 8 and 75, and both snapshot base drafts must parse. `a_required_member_is_never_null` gains the snapshot's twelve non-nullable members, for 56 paths. So the mutation gate judges rule 24 inside the crate.

8. **#467's round-1 minors, taken here as ruled.**
    - **m3: the snapshot reaches its schema through `schema`'s `schema_version` gate,** like every other §9.2 type. `control::tests::a_snapshot_at_another_schema_version_is_an_unknown_schema` pins that a snapshot at `schema_version` 2 is refused as `unknown_schema`, and it fails alone when the gate is bypassed for the snapshot.
    - **m2:** the catalogue test's doc says `fees` journals the writer from the registering change.
    - **m5:** the backlog row now says what is done, and counts 15 snapshot drafts.
    - **m4:** with the three pins live, the feature map's `Run` command executes them.

### Rationale

DEC-77's tests-first stage for the last schema §9.2 closes. DEC-261 item 7's ordering (the writer before the registration) moves from three live pins that held the schema back to pending tests that land the two together.

### Alternatives considered

- **Register the schema in one PR with the writer wiring, with no tests stage.** Rejected: DEC-77 puts tests first on a safety-critical path.
- **Keep `unknown_schema` as the stub's answer.** Rejected: a pending test must fail at its own stub's report (DEC-137), and `unknown_schema` is not one.
- **Wire the writer in this PR.** Rejected: that is implementation (DEC-77), and the live pin would have to change in the same PR.

### History

Supersedes DEC-303 item 4's three live pins, as DEC-303 item 4 and DEC-389 item 2 anticipated.

<a id="dec-408"></a>

## DEC-408: #485's invariant: live exits and the rungs a ladder will still send never exceed the position

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176, on the coordinator's rulings on #468 (2(i); 15:06Z on where these tests land) and on #494 and #496 rounds 1 (16:55Z) and 2 (20:02Z): it tightens rule 12's Σ check and weakens nothing |
| **Story or claim** | E7-4, issue [#485](https://github.com/kunwarshivam/mandate/issues/485), found by #468's rule-13 property; the design is DEC-409 and DEC-410, in #494 |

### Decision

1. **The invariant.** After every input, the live non-protective sells at the broker, plus the remainders exit ladders between rungs will still send, sell at most the position (§5.3 rules 3 and 4, §5.6 step 5, rule 12).
   - **What "between rungs" means.** A ladder is between rungs from the confirmation of a step's cancel that leaves part of its rung unsold, until it sends that part as its next rung, or ends.
   - **What is counted.** A remainder counts only while something will send it, and only what the position and the open sells leave it, in its exits' id order (DEC-410 item 1).
2. **The oracle counts it on its own record.** The rule-13 oracle in `protection.rs`'s `sequence_tests` follows each remainder from the drafts and the broker's answers, never from the fold.
   - **It starts** when the oracle confirms the cancel of a rung that was asked as a step (`ladder_step`), with part of it unsold:
     - a lone ladder (DEC-260 (14)) always starts one;
     - a sequence starts one only while it still climbs (not paused, not stopped, not past its interval's bound). Otherwise its ladder ends there (DEC-409).
   - **It counts** while it will be sent:
     - a lone ladder's, unless it is parked while the agent is paused or stopped;
     - a sequence's, while the agent is neither and the interval has not reached its bound.
   - **It ends** at any of these:
     - the exit's next rung;
     - the exit's abandonment;
     - the interval's bound while the sequence runs;
     - the instrument's `unprotected_end`, unless the exit was parked for the next open (DEC-260 (18)), in which case it becomes a lone ladder's;
     - a `ProtectionChanged rung_short` that sends nothing.

     A ladder that ended may send no later rung: the oracle fails on one.
   - **What it counts is the oracle's own figure, never stored.** Whenever the oracle reads a remainder, it takes the stepped rung's unfilled quantity, capped at the position less the live exits at the broker and less the remainders ahead of it in id order.
   - **Every later rung sends exactly that figure** (round 2's M2). A rung of a ladder between rungs must send what its remainder counts at that moment. A rung sent short must be journaled `rung_short`, with the quantity not sent equal to the unfilled quantity less that figure. So a remainder that shrank too far, or was never given back, fails here: the under-sell side that the Σ check alone cannot see.
   - **The check.** The live exits plus the counted remainders are a second Σ check, beside the existing check of every live order. A protective order is left out of the second, since the ladder's next rung cancels it first.
   - **This is the over-cover side only.** The under-cover arm (every held share covered by a protective order or a live exit, once nothing is under way) arrives with #468.
3. **No exit is sized, held or refused for a remainder, except a discretionary exit's sizing.**
   - A risk exit, an owner exit or a flatten must go at its full quantity.
   - A discretionary exit may be sized to what the oracle's count leaves. It may be denied `sell_exceeds_available` when nothing is left, but only when no session or price hold applies (DEC-410 item 3, Proposed).
   - Under the Σ checks, a hold `unknown_order_in_flight` must coincide with an order the script's broker left unknown. That broker never leaves one, so any such hold fails (round 2's minor 2).
   - No new hold reason is accepted, and the oracle's bound exempts no new one.
4. **The pending tests** are in `sequence_tests`, marked `pending E7-4` (the kill switch's is `pending E7-3`):
   - `a_risk_exit_beside_a_sequence_between_rungs_never_over_sells`: #485's minimal script, a rung stepped while paused.
   - `a_ladder_stopped_between_rungs_never_resumes_beside_a_new_exit`: the same, stopped.
   - `a_risk_exit_held_overnight_beside_a_parked_ladder_goes_whole_and_the_ladder_sends_what_is_left`: the parked route. It over-sells 12 against 10 today with no pause at all. The fix sends the exit of 8 whole, and the ladder's rung sends 2, journaled short by 2.
   - `a_risk_exit_beside_a_parked_ladder_selling_everything_goes_whole`: the exit of 4 goes whole, and the ladder's rung sends the other 6.
   - `a_parked_sequence_past_its_bound_never_sends_its_remainder`: the review of #494's loop (M2). A sequence parked beside a working owner exit reaches its bound in the night, so its remainder ends; a risk exit at the open goes whole, and no rung of the old ladder follows.
   - `a_ladder_that_resumes_with_nothing_left_sends_nothing` and `a_ladder_that_resumes_after_its_position_shrank_sends_only_what_is_left`: a parked lone ladder of a paused agent counts nothing. When the agent resumes after the stop has sold the position, its remainder counts only what is left.
   - `a_restart_between_rungs_still_counts_the_parked_remainder` and `a_refused_rung_leaves_its_remainder_to_the_next_exit`. Both fail today on the parked route's over-sell in their prefix; their tails are what the fix must get right.
   - `a_kill_switch_between_rungs_never_over_sells` (`pending E7-3`).
   - `rule_13_holds_over_random_scripts_from_between_rungs`: the oracle over random scripts that start from a rung stepped while paused or stopped, or from a parked ladder beside a held exit, with stops, refused rungs and restarts in their tails.
   - `a_discretionary_exit_beside_a_parked_remainder_is_sized_to_what_is_left` (round 2's minor 3): a discretionary exit of 8 held overnight, beside a remainder of 4, is sized to 6 when its paused agent resumes at the regular open.
   - `a_discretionary_exit_with_nothing_left_is_held_overnight_then_refused_at_the_open` (minor 3): beside a remainder of 10, it is held `session_closed` in the night, not denied. It is denied `sell_exceeds_available` at the regular open.
   - `a_rung_sent_short_beside_a_sell_the_broker_then_refuses_is_journaled_short` (round 2's M2): at the open the exit of 8 goes whole and the ladder's rung goes short by 2 in the same step, journaled. The broker then refuses the 8 (`Move::RejectFirst`), and the record stands.

   The thirteen E7-4 tests fail on the oracle's count or their own assertion, not at a stub. Each is a named `BEHAVIOUR_ONLY_TESTS` row, which the fix deletes. The kill switch's test stops at E7-3's stub.

   `gate::remainder_tests::a_stopped_agents_parked_ladder_never_shrinks_another_agents_exit` and `a_stopped_agents_parked_ladder_leaves_a_discretionary_exit_whole` are live: two agents in one instrument (the review of #494's script). Today's gate counts no remainder, so another agent's risk exit or discretionary exit beside a stopped agent's parked ladder goes whole; it must keep doing so. The second is minor 3's "remainder gone" side. With one agent, the script cannot reach it: the bound that ends a parked sequence also abandons the exits held in its instrument.
5. **Nothing is weakened, and `main` stays green.**
   - The existing oracle's checks stand. No seed is pinned. The existing generator is unchanged; the new moves (a stop, a refused rung, a restart, the kill switch, a stop's fill) are drawn only by the new property and the named scripts.
   - **Every new check runs only in `rule_13_summed`**, in the named scripts and the new property, all pending (round 2's B1). That covers the Σ check, the ended-ladder check, the zero and size checks, the later-rung check, the `rung_short` check and the unknown-order check. Their bookkeeping runs in every script, but only these checks can fail one.
   - **Measured:** the live property over seeds 1 to 40 on this head. The result is in the PR description; only #489's known seed goes red.
   - **Why the live random property waits.** It draws a fresh seed in `ci test`, inside the required `fast` check, in the mutation gate's baseline, and nightly; only `ci pending` pins a seed. With the Σ checks, the live property could fail at random on the very bug these tests pin.
   - **The fix moves it over:** #485's fix deletes the pending tests' `#[ignore]` lines and moves the live random property onto `rule_13_summed`.
6. **The scripts' pauses are journaled and stepped.**
   - A pause or a stop is committed as `AgentModeApplied` and folded, rather than set on the live state, so a restart and the end-of-script refold read the mode each decision was taken under.
   - The copy itself is not stepped, since the executor's reaction to a mode change is still E7-4's stub. The script steps a tick at the same instant instead, as the shell would step the copy, so the executor acts on the new mode before the oracle checks it.

### Rationale

The rule-13 property found the over-sell while #468 was in review: a sequence that lingers between rungs counted nothing in the gate's `available`. The coordinator ruled that the tests hold the Σ with the remainders counted. Round 1's review showed that counting a remainder nothing will send costs real risk reduction, so the oracle counts only what will be sent, and checks that the plan, not the sell, gives way. Round 2's review showed that nothing caught an under-sell, so the oracle now checks every later rung against its own figure.

### Alternatives considered

- **Count only the live orders, as before.** Rejected: it catches an over-sell only once both rungs are live, and says nothing about the state that caused it.
- **Count every stepped rung's remainder until its next rung.** Rejected: the first draft of the oracle did this. It failed on every seed, because a sequence past its bound or paused at its step never sends its next rung.
- **Round 2's oracle (#496 at 193bda7): check each journaled trim against the oracle's shortfall or overhang.** Withdrawn with the stored trim (DEC-410). It also ran three new failing checks in the live property (round 2's B1).
- **Round 1's oracle (#496 at effaa80).** Rejected by the round-1 ruling:
  - it counted a parked lone ladder's remainder whatever its agent's mode;
  - it accepted a sized-down risk exit;
  - it exempted a new hold from every bound.

<a id="dec-414"></a>

## DEC-414: The registration of `ThesisProposed` and `ThesisRevised`: the stubs, and where the mandate re-derivation lives

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176) |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The agent (stream J, builder J3), under DEC-79 and DEC-176. Every stub refuses what was refused before, and every pending test only refuses more |
| **Story or claim** | E17-2, its journal half; stream J, claim [#132](https://github.com/kunwarshivam/mandate/issues/132); tests PR on `agent/j3-thesis-registration-tests` (DEC-77 stage 2) |

### Decision

1. **Scope.** This decision covers the `mandate-journal` registration of journal spec v0.9 §9.4's `ThesisProposed` and `ThesisRevised` (DEC-413). It also covers the requirement that blocks the registration's acceptance (DEC-413 item 5; #490 round 1, M2): re-deriving mandate spec §8.5 checks 4, 5, 6, 10 and 16's cap from the mandate the record names. This tests PR adds stubs and pending tests (DEC-77 stage 2). The implementation PR replaces the stubs and deletes only the `#[ignore]` lines.
2. **The stubs in `mandate-journal`.**
    - The §9 catalogue gains both types: agent stream, `config_refs` `mandate_version` and `model_version` (§9's "man, mod").
    - `control::governs` routes both on the agent stream to §9.4. That puts them beside §9.2's and §9.3's types, as `control.py` holds their rules in the reference.
    - `control::payload` refuses both as `InvalidReason::Unimplemented` at `payload`. The variant comes back for this stub, as it did for #482, and the implementation PR removes it again.
    - Before this PR both types were `unknown_event_type`. They are refused before and after, so nothing that was accepted is refused now.
    - The variant's code, `unimplemented`, is deliberately pinned by no test: `tests/append.rs`'s hand-written list of reason codes does not name it, because the implementation PR removes the variant, and a line naming it would only be deleted again (#510 round 1, minor 4).
3. **Where the re-derivation lives: `mandate_spec::context::check_thesis_record`.**
    - It takes the record's payload, the digest its `config_refs.mandate_version` names, and the same document lookup `JournaledFact::from_record` takes.
    - It parses and re-hashes the stored mandate, and refuses a record whose verdict passes over a check that mandate fails.
    - **Why `mandate-spec`.** The re-derivation reads the mandate's typed fields (`universe.pinned`, `autonomy.admission`, `universe.asset_classes`, `behavior.research`, the signal models' `admits_instruments`). Only `mandate-spec` holds those types. `mandate-journal` is a layer below it and has no mandate types; `append` holds no document store; and §9.4 deliberately gives the check no schema rule. This is where §9.3's classification re-derivation lives too (DEC-404 item 5).
    - **Why a function of its own, not `JournaledFact::from_record`.** §9.4 maps neither record to a fact, and `from_record` takes the payload alone, while the mandate is named only in `config_refs`. Widening `from_record` would touch every caller for two records it maps to nothing.
    - **Who calls it.** Any reader of a thesis record calls it before acting on the record: the lineage fold, and the writer's own read-back. That is recorded on the backlog's E17-2 row. No caller exists yet, because no fold reads these records (§9.4).
4. **What the function answers.** Each answer is pinned by a pending test in `thesis_tests`, in `context.rs`.
    - `InvalidInput` at `mandate_version` when the document is absent, or is stored under another document's digest.
    - `InvalidInput` at `admitted` when the record admits past a check the mandate fails: check 4 (no research agent), check 5 (a pinned universe, MI-20, as in MC-N08), check 6 (`admission: deny`, as in MC-N15), check 10 (an asset class outside the mandate's), or check 16 (a revision past the cap, as in MC-N24).
    - `InvalidInput` at `reason` when the record is refused at a check later than the first one the mandate fails. A test with a mandate that fails two of the checks pins "first", and a refusal at check 17 under a mandate with no research agent pins check 4 inside that search (#510 round 1, M2).
    - `InvalidInput` at `reason` when the record is refused at check 5, 6 or 10 and the mandate passes that check. These three read the mandate alone. Check 6 stays among them only while the overlay raises `auto` to `ask` and never to `deny`, as DEC-413 item 4 records.
    - `Ok(())` otherwise. That includes a refusal at check 4 or 16 that the mandate does not show, because the overlay or the folded `retired` flag can fail those two.
5. **The `man` ref.** On these two records, `config_refs.mandate_version` names the mandate in force when the thesis was judged. `a_thesis_is_judged_under_the_mandate_it_names` pins that: one admission is accepted under the research mandate and refused under a pinned one, with both stored, so the function reads the named document and no other.
6. **Pending tests in `mandate-journal`, from §9.4's vectors.**
    - `every_thesis_draft_is_judged_as_its_vectors_say`: every base and valid draft parses, and every invalid draft is refused with its first reason and path. That includes the seven report-order drafts.
    - `a_required_thesis_member_is_never_null`: the 20 members §9.4 lists without `?`, written from the spec's table, and the first and second elements of `evidence_sources`, so an element typed `text?` is refused too (#510 round 1, M1; #445 found the same gap three times for §9.2). The backlog records a helper that derives such lists from the schema, so the next never-null test cannot forget a list's elements.
    - `a_thesis_record_at_another_schema_version_is_an_unknown_schema`.
    - `thesis_records_refuse_an_unlisted_member`, in `tests/catalogue.rs`.
    - One live test, `thesis_records_are_routed_to_section_9_4`, shows neither type is refused for want of a catalogue entry or a schema.
7. **Prerequisite of the implementation PR: journal spec v0.11 ([#513](https://github.com/kunwarshivam/mandate/pull/513), DEC-413 item 7).** It types §9.4's `instrument_id` as `asset_id` and adds four `research` drafts that refuse a non-asset-ID value, as §9.3's: a ticker, a number, a trailing newline, and capitals. Two of them, the ticker and the capitals, are told apart by the type change alone; `id` refuses the other two as well, and they pin §9.3's newline and type seeded bugs for this section (#513 round 1, minor 3). The implementation PR lands after it and registers the member as `Ty::AssetId`, so the journal never appends an instrument a later mapping cannot read (#503 round 1, m3; #510 round 1, minor 2). Nothing in this tests PR assumes either type: every `instrument_id` in its fixtures is already a canonical asset ID.

### Rationale

DEC-413 item 5 and the #490 ruling make the re-derivation a condition of the registration's acceptance. Pinning it as pending tests in the tests PR means the implementation cannot land without it. Putting it in `mandate-spec`, beside §9.3's classification re-derivation, keeps the journal crate free of mandate types and the schema free of a rule `append` cannot evaluate.

### Alternatives considered

- **The re-derivation in `mandate-journal`, reading the mandate as untyped JSON.** Rejected: it would duplicate the mandate's parse and grammar, and it would need a document store that `append` does not have.
- **A new parameter on `JournaledFact::from_record`.** Rejected: item 3.
- **A §9.4 schema rule.** Rejected by the #490 ruling: `append` does not resolve the `man` ref.

<a id="dec-415"></a>

## DEC-415: Mandate spec §8.5's refusal list has one home, `mandate_domain::ThesisRefusal`

| | |
|---|---|
| **Status** | Accepted (agent, DEC-79 and DEC-176) |
| **Date** | 2026-10-03 (UTC) |
| **Decided by** | The agent (stream J, builder J3), under DEC-79. This is a reversible engineering choice. No outcome changes: every record, thesis and mandate gets the verdict it got before |
| **Story or claim** | E17-2 follow-up, #519 review round 1, minors 2 and 4; stream J, claim [#132](https://github.com/kunwarshivam/mandate/issues/132); tests PR on `agent/j3-refusal-home-tests`, implementation PR on `agent/j3-refusal-home-impl` |

### Decision

1. **One home.** Mandate spec §8.5's seventeen refusal reasons become one type, `mandate_domain::ThesisRefusal`, declared in check order. It carries each reason's journaled code (`code`), its check number (`check_number`), the codes as a closed set (`CODES`), §8.2's ignored-output predicate (`is_ignored_output`), and `parse` from a code. Three copies of the list exist today with nothing pinning them equal, and all three read this type instead:
    - `mandate-research`'s `RefusalReason`. It becomes `pub use mandate_domain::ThesisRefusal as RefusalReason`, so the crate's API and its tests do not change.
    - `mandate-journal`'s `control::THESIS_REFUSALS`. §9.4's `reason` is typed `Ty::OneOf(&ThesisRefusal::CODES)`.
    - `mandate-spec`'s `context::THESIS_REFUSALS`. `check_thesis_record` takes a refusal's check number from `ThesisRefusal::parse(reason)?.check_number()`.
2. **Why `mandate-domain`.**
    - It sits at layer 1, below all three readers: journal at 2, spec at 3, research at 5.
    - ES-02's planned order already runs domain → journal.
    - The crate's rule is that it names things and decides nothing. The list fits that rule: which check fails is still decided by `mandate-research`, rules 35 to 37 by `mandate-journal`, and the mandate's checks by `mandate-spec`.
    - The new edges are journal → domain and research → domain. Both are workspace-internal, need no `layers.toml` change, and add no external dependency.
3. **One form for a check (minor 4).** A check is named by its number from 1, as §8.5 numbers it.
    - `mandate-journal` loses its 0-based index and `CORROBORATION_CHECK` (15). Rule 37 compares check numbers: with `corroboration` null, `reason` is "a check numbered 15 or lower" (journal spec §9.4), so the bound is `check_number() <= ThesisRefusal::NoCorroboration.check_number()`. The 0-based `index < 15` it replaces meant the same thing, which is exactly the confusion minor 4 names.
    - Rule 36's "first failing of checks 1 to 3" becomes a walk over `ThesisRefusal::all()`.
4. **The staging (DEC-77, DEC-83).**
    - **Tests PR.** The type ships live with live tests in `crates/mandate-domain/tests/domain.rs`, following that file's own rule: a total map over the crate's variants is small enough to be right in the PR that adds it, and a pending marker would hide it from the diff mutation gate. `mandate-research`'s `RefusalReason` shipped the same way (`tests/rules.rs`'s header).
    - **Why no pending test.** The tests pin the list against §8.5's table typed independently from the spec, the 1-based numbers, the parse round trip and its exact-spelling refusal, the ignored outputs, and `Ord` as check order.
    - **Implementation PR.** It moves the three readers onto the type and changes no test file. Moving a list that already agrees changes no outcome, so no test could fail before it and pass after; the existing suites of all three crates are the regression pins.
5. **`DomainError::UnknownThesisRefusal`**, code `unknown_thesis_refusal`, is add-only (ES-09).
    - **In `mandate-spec`.** An unknown `reason` was `InvalidInput` at `reason` before and stays so.
    - **In `mandate-journal`.** It never reaches the new error, because `Ty::OneOf` refuses an unknown code as `schema` first, as before.
6. **`is_ignored_output` moves with the list.** `mandate-research` calls it as a method on the reason, and a method on a re-exported type must live where the type does. The ignored outputs are named in §8.2, the same table that numbers the checks.

### Rationale

The #519 review found the list written three times in three safety-critical crates, with only review keeping them equal. A reason renamed in one crate would leave the other two disagreeing about the same journaled record: the journal might accept a record the research crate never writes, or a re-derivation might number a check one off. The trust ladder puts this on rung 1: a type the three crates read cannot drift, where a test or an xtask check can only notice the drift.

### Alternatives considered

- **An xtask check that the three lists agree** (rung 2). The backlog row names it as the fallback. It still leaves three lists to edit, and the check has to parse Rust source.
- **A test in each crate comparing its list to `mandate-research`'s.** Neither `mandate-journal` nor `mandate-spec` may depend on a layer-5 crate.
- **`mandate-canon` (layer 0).** It holds canonical bytes and digests, not domain names.
- **A tests PR with stubbed `code` and `check_number`.** Both are infallible, so a stub would have to return a wrong value rather than an error. The mutation gate would also skip the crate while the stubs' tests were pending, which is the reason `domain.rs` and `rules.rs` keep their vocabulary live.

<a id="dec-418"></a>

## DEC-418: #489's test: a risk exit allowed during a passive sequence's cancel is never stranded

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176, on the coordinator's rulings on #468 (15:06Z: #489 is fixed tests first, in its own PR) and on #501 (17:26Z: on `main`, apart from #485's chain) |
| **Story or claim** | E7-4, issue [#489](https://github.com/kunwarshivam/mandate/issues/489), found by #468's rule-13 property |

### Decision

1. **The pending test** is `sequence_tests::a_risk_exit_allowed_during_a_passive_sequences_cancel_is_never_stranded`, `pending E7-4`. It is #489's minimal script, from the state #468 found it in (`beside_a_lone_exit`): a lone risk exit of 4 working beside an OCO for the other 6.
   - A risk exit of 4 above the bid goes passive, and its sequence cancels the OCO.
   - A risk exit of 3 is allowed meanwhile and waits on that cancel.
   - On the confirmation the passive exit is placed as an OCO for 4. Beside the lone 4 and the waiting 3, that no longer fits within 10.
   - Nothing cancels it, so the exit of 3 waits past `max_intent_age_s`. The rule-13 oracle's bound catches this: a hold rule 13 does not allow.
2. **The test fails on the oracle's bound, not at a stub,** so it is a named `BEHAVIOUR_ONLY_TESTS` row, which the fix deletes.
3. **The starting state is ported from #468** (`LONE` and `beside_a_lone_exit`), with #468's own `lone` switch on the script runner (`rule_13_script_from`), so #468's merge of `main` meets the same names. The test runs the existing rule-13 oracle. Its bound is what catches #489, so it needs nothing from #485's Σ checks, and this PR stands on `main` apart from #485's chain (the coordinator's ruling on #501, 17:26Z).
4. **No random property from that state here.** From `beside_a_lone_exit` on `main`, the rule-13 property first finds the over-cover #468 fixed in its round 1 (M5: re-protection beside a working exit for the whole position, 14 against 10). It could not pass before #468. #468's own property, which starts half its scripts from that state, covers #489's fix once #468 merges `main`.
5. **Round 1** (the coordinator's ruling on #506 and #508, 18:50Z):
   - **The release is pinned.** The script runs on to the placement's confirmed cancel, so it asserts the exit of 3 is submitted, not only that a cancel was asked.
   - **`a_refused_cancel_of_an_unacknowledged_placement_is_asked_again`** (pending E7-4, a named behaviour-only row). The broker refuses the cancel of the placement, unacknowledged. It must not be read as the order's rejection; the oracle fails on that. The placement is queried, and the cancel is asked again once it is seen live. Today it fails on the stranding itself, since nothing cancels the placement yet.
   - **`a_held_exit_never_has_a_placement_cancelled_for_it`** (live). A held exit cannot move, so no placement is cancelled for it. Today's code never cancels the placement, so this holds now, and DEC-419's fix must keep it.
   - **`beside_a_lone_exit`** carries #468's `created_on` again, so the two copies are identical.

### Rationale

The coordinator ruled that #489 blocks #468, and is fixed after #485, tests first, in its own PR. The named script reproduces it in one move sequence, and fails for #489's reason alone.

### Alternatives considered

- **A pending random property from the lone state.** Rejected: on `main` its minimal failure is #468's already-fixed over-cover, not #489, so it would pin the wrong bug and stay red after #489's fix.

<a id="dec-419"></a>

## DEC-419: A waiting exit cancels a handed-on placement that no longer fits beside it

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 |
| **Decided by** | The stream K builder (`claude-code`) under DEC-79 and DEC-176: it applies the existing rule (#286 round 2, M1′) at the moment it was missing, and adds no risk |
| **Story or claim** | E7-4, issue [#489](https://github.com/kunwarshivam/mandate/issues/489) |

### Decision

1. **The rule.** A passive exit placed while other exits were still selling hands its sequence on (`passive_exit`). The protection it placed holds a later exit back only once its stop and their sells no longer fit within the position (`fits`). When that happens, `begin_exit` cancels the placement for the later exit. That check ran only when the later exit was allowed. An exit allowed *before* the placement existed waited behind it forever.
2. **The fix.** At every step an exit waits, the placement's own step included, `overtaken` checks a handed-on placement against the exits still selling and those allowed and waiting that the gate does not hold. If it does not fit, it is cancelled as `begin_exit` cancels it, and the waiting exit goes once the cancel is confirmed, as any exit waiting on a protective cancel does.
   - **The ask is recorded** (#508 round 1, M1). A cancel asked for an order the broker has not yet acknowledged moves no state, but the fold now records it (`cancel_unconfirmed`) all the same. So a refusal of that cancel is the cancel's refusal: the order is marked overdue and queried (§5.7), never journaled rejected while the broker may still hold it.
   - **Asked once, and again only when it is needed.** `overtaken` asks only where no cancel is outstanding: never for an order `PendingCancel`, nor for one `Unknown` while the query a refused cancel sent is out (it is never cancelled blind), nor for a `Submitting` one whose cancel is already asked (#508 round 2, M1′). After a refusal, the query's answer moves the placement to `Accepted` if it is still live, and the next check asks again, once, so the waiting exit is not left to the bound. That is why a per-step check is now safe. The first version was one-shot, because the fold recorded no ask for an unacknowledged order.
   - **A held exit cannot move** (#508 round 1, m1). Its quantity is left out of `overtaken`'s fit (`fits_beside_movable`), so no placement is cancelled for an exit the gate holds. Once released, it is gated again and handled like any other.
   - **A later arrival** is still `begin_exit`'s to handle, as before.
3. **Lifecycle.**
   - **What it blocks:** nothing new. It only lifts a wait the rule already meant to lift.
   - **How it ends:** at the cancel's confirmation. A refused cancel is queried and asked again while the placement is live. A broker that never answers holds the exit, which is rule 13's broker hold, as for every protective cancel.
   - **Paused or stopped agent:** a held exit is left out of the fit, so the placement stays for it.
   - **Restart:** the ask, the cancel and the waiting exit are folded. A restart between the placement and its cancel replays the whole step, since a batch commits whole.
   - **No unbounded gap** (#508 round 1, m4). Every cancel `overtaken` makes falls inside the handed-on sequence's interval, which is open, journaled and bounded by `max_unprotected_s` (§5.4). If the cancel never completes, the bound ends the exits beside the placement and re-protects what is left. The gap is never longer than the bound, and never silent.
4. **What this closes, and what it does not** (#508 round 1, m4). #489 is closed as a stranding: a risk exit never waits for ever behind a placement the executor made. The over-commitment behind it stays with #485's chain: the gate's `available` does not count an exit already allowed with no order yet, so two exits can be allowed for more than the position.

### Rationale

Rule 13 lets an exit be held only by a pause or stop, an `Unknown` order, or the broker. A wait behind an order the executor itself placed, that nothing will ever cancel, is none of those. The existing rule already says the placement is cancelled when a later exit no longer fits beside it. This makes it hold for an exit that was waiting when the placement arrived.

### Alternatives considered

- **Size the passive placement net of the exits waiting too.** Rejected: the placement is the passive exit's own order, at its own quantity. It cannot shrink without changing the exit.
- **Abandon the waiting exit at `max_intent_age_s`.** Rejected: that refuses risk reduction (rule 13).
- **Decide the fit before placing, and do not place** (#508 round 1, M1). The fit is decidable before the placement: the same `fits_beside_movable` against the same exits. Not taken, because the placement is the passive exit's own order. Not sending it needs a new answer for an exit the gate allowed (re-gate it, send it marketable, or hold it), which is a larger change than this defect needs. Place-then-cancel reuses `begin_exit`'s existing rule, inside a bounded interval.
- **Keep the check one-shot and leave a refused cancel to the bound.** Rejected: the waiting exit would then wait for the bound, which abandons it, refusing risk reduction (rule 13).

<a id="dec-420"></a>

## DEC-420: How stream H reads delegations into `mandate-spec`, and DEC-353's rule in Rust

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The stream H builder (agent), under DEC-79 and DEC-176: each reading follows the spec or takes its stricter side |
| **Story or claim** | E6-13 (DEC-353's code half) and the `mandate-spec` half of E8-8; claim [#439](https://github.com/kunwarshivam/mandate/issues/439); [task brief](../tasks/E6-13-delegations-in-mandate-spec.md) |

### Decision

1. **Scope.** DEC-353's rule reads the new version's delegations, and `mandate-spec` had none. The
   parse cannot accept `autonomy.delegations` until V-041 to V-043 check them, or a mandate with a
   60-day delegation or a cap past the allocation would validate. So this story puts delegations into
   `mandate-spec` as a document:
   - the type and the parse;
   - V-022 and V-023 for a delegation, and V-041 to V-043;
   - §9.2's `autonomy.delegations` row, and DEC-353's two conditions in the autonomy row.

   The runtime lift (§6.2 step 4a, MI-26 to MI-28), the approval card's shapes and the MC-U family
   stay with E8-8. This is the scope proposed on the claim on 2026-10-02 (option 1).
2. **The type.** `Autonomy` gains `delegations: Vec<Delegation>`, empty when the member is absent.
   `Delegation` holds the schema's nine members, typed:
   - `id` as `DelegationId`, with the rule-id grammar;
   - `lifts` as `Lifts::Default` or `Lifts::Rule(RuleId)`;
   - `when` as a `Condition`;
   - the two dollar caps as positive decimals, and `max_orders` as 1 to 1,000;
   - `starts_at` and `expires_at` as `Option<UtcNanos>` (item 3);
   - `source_approval_id` as an optional `ApprovalId`, a lower-case uuid.
3. **The parse accepts exactly what the schema accepts, as ES-22 asks.**
   - Each refusal names the delegation's own pointer (`/autonomy/delegations/<n>/<member>`).
   - An instant off the schema's pattern (`YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ`, nine fractional digits)
     is `off_pattern`. The pattern is not a calendar, so a text it matches that names no instant
     (`2026-02-30T…`, or an hour of 24) parses, as `None`. V-041 refuses it: its window has no end
     to bound. This is DEC-151's reading of a date, which V-015 refuses.
   - DEC-271 item 6 refuses a calendar-less `review_by` at the parse instead, because there `None`
     already means "no review date", so a malformed date would switch the review off. Here `None`
     means no such instant, and no validated mandate can hold one, since V-041 refuses it at either
     end. The V-041 tests pin both ends, so this reading rests on them.
   - More than 20 delegations is `out_of_bounds` at the array.
4. **V-042 needs the previous document.** `PreviousVersion` gains `mandate: Option<Mandate>`.
   - V-042 classifies with the delegations removed from both versions and the new version's review
     date in both (DEC-273), so a version increasing only because of what its own delegations lift
     carries them (DEC-353 item 4).
   - With the document withheld (`None`), a carried delegation cannot be told from a new one, so
     V-042 refuses every delegation the new version holds. That is the side that adds no risk
     (`AGENTS.md` rule 3).
   - The journal fold's context (`context.rs`) holds only the version's identity today, so it
     passes `None`. A version with delegations validated through it is refused until it can load
     the previous document by digest.
5. **V-022 covers every delegation.**
   - A provenance entry that covers a delegation's path, or that a delegation's path covers
     (`/autonomy/delegations`, `/autonomy/delegations/0`, or `/autonomy/delegations/0/max_order_usd`),
     must be `user_entered` and confirmed, as for every `auto`. That is DEC-161's reading of `covers`
     in both directions.
   - V-017, V-018 and V-023 read a delegation's `when` as they read a rule's.
6. **The classifier.**
   - The `/autonomy/delegations` path is its own row: matched by `id`, reducing only if every
     change removes or narrows a delegation.
   - The autonomy row compares the two versions with the delegations removed, and reads the new
     version's `lifts` for DEC-353's two conditions, carve-out included.
7. **The order path ignores delegations until E8-8.** `mandate-builder` reads no delegation, so an
   `ask` a delegation would lift stays `ask`. That is stricter than the spec, never looser (MI-26),
   and E8-8 adds the lift.
8. **DEC-77.**
   - **The tests PR:** adds the types and the `Violation` variants. The stub parse refuses a
     document holding `delegations` as `unimplemented`, so it still fails closed. The stub
     classifier answers `unimplemented` for any autonomy block that holds one. The tests are in
     `crates/mandate-spec/tests/delegations.rs`, pending on those stubs.
   - **The implementation PR:** replaces the stubs, and the refcase harness reads a
     `previous_version` document.
   - **The status PR:** marks MC-J01, MC-J03, MC-J05, MC-J06 and MC-J09 passing.

<a id="dec-421"></a>

## DEC-421: Protection is sized net of fills the broker reported and the executor has not applied

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-02 (UTC) |
| **Decided by** | The stream K builder (agent), under DEC-79 and DEC-176: protection is sized toward what the broker holds, never above what `main` places, so no safety rule is weakened; the two states where it still errs are recorded in items 1 and 5 and tracked in the backlog |
| **Story or claim** | E7-4; [#515](https://github.com/kunwarshivam/mandate/issues/515); #468's round-3 review; the coordinator's rulings on #468 (18:42Z), on #517 and #518 round 1 (21:45Z), and round 2 (00:28Z) |

### Decision

1. **The rule: protection is sized to what the broker holds, by the best reading the executor has.** §5.4 asks two things of every re-placement, and this decision keeps both:
   - **the cap:** Σ protective sells never exceed the position (rule 12);
   - **the cover:** after a terminal state, the remaining quantity is protected, no less.

   Both are measured against what the broker holds: the folded position less every fill the broker has reported on an ended sell that the fold has not applied (`OrderDetail.reported_filled` beyond the order's `filled_qty`), net of item 5's unattributed sells. The reading is exact when every sale in the instrument is the platform's own; item 5 says the one state where it reads high, and the bullet below the one where it reads low. It is never higher than the fold's position, so it never places more than `main` does.
   - **One rule for every terminal state.** A sell ended `Filled`, `Canceled`, `Expired`, `Rejected`, `Replaced` or `Abandoned` whose report carries a `filled_qty` beyond what is applied comes off. Every such report asks for a reconciliation (`orders.rs`), so the broker's own number is the best reading until the fill is applied. Item 5's netting closes an under-cover only when the sold shares arrive as a sell fill that names no order. Nothing closes one from a terminal report that **overstates** its fills (a `Rejected` carrying a `filled_qty` no fill matches): `reported_filled` only rises, and neither `ReconciliationRun` nor `BrokerPositionObserved` lowers it. That under-cover has no bound until the backlog's reconciliation-bounded correction lands. It is still the ruled choice, because waiting for the reconciliation leaves every case unprotected for an unbounded time (Alternatives).
   - **The gate reads the fold's position.** `gate.rs`'s `available` (§5.3 rules 3 and 4) does not take the correction, so while a correction is open the gate can admit a sell for the folded quantity when the broker holds less. This decision changes protection sizing only. Moving the gate onto the reading would change what DEC-410 sizes and crowds out on the gate path, and the reading itself still errs both ways (item 5 and the bullet above). That is a change for the reconciliation-bounded story in the backlog, not for this fix.
   - **Every reader of the held position in `protection.rs`** (#518):
     - `long`, the corrected reading;
     - `replace` (the sequence's re-placement): takes it through `long`;
     - `passive_exit` (the rest beside a passive exit): takes it through `long`. Before #518 it read the fold's position directly, which over-covered by the unapplied part (#518's review, blocker 2);
     - `fits` and `fits_beside_movable`: take it through `long`. So a handed-on placement now holds its exits back (on §5.4's own cancel) in states where the fold's position said it fitted, which errs toward re-protecting what the broker holds;
     - `bracket`'s room: takes it through `long`, and adds the entry's own unapplied buy fill as before. The two cannot meet, because `long` subtracts only sells;
     - the triggered-stop watchdog's exit size (`covered.min(long)`): takes it, so a breached stop never sells more than the broker holds, and sells all of it;
     - `is_protected`, the coverage probe: reads the fold's position on purpose. It answers whether the folded position is covered, so it can only under-report coverage while a correction is open, which is the safe direction for a probe.
2. **Tests first (DEC-77).** Four pending tests in `sequence_tests` pin it on `main`. Each fails on the size it asserts, not at a stub, so each is a `BEHAVIOUR_ONLY_TESTS` row until the fix deletes it:
   - `an_exit_ended_with_a_fill_not_yet_applied_is_re_protected_for_what_the_broker_holds`: #515's first script. Exactly 7 re-placed, and the interval ends `placed`, `unprotected_end`; `main` places 10.
   - `only_the_part_of_a_reported_fill_not_yet_applied_comes_off`: 2 applied, then 3 reported. Exactly 7; `main` places 8.
   - `a_fill_applied_after_its_report_comes_off_once`: the update after the report re-places nothing, and the cover stays 7; `main` covers 10.
   - `a_passive_exit_after_an_unapplied_fill_protects_only_what_the_broker_holds`: a passive exit after the report. The passive exit and the rest of the position are protected for 7 in all; `main` places 10.
3. **The oracle.** The rule-13 property's Σ check reads the fold's position today. It gets an arm
   that reads the broker's reported fills instead, so that it does not trust the fold here. The
   arm extends the Σ check #496 brings to `main`, so it is added once #496 has merged, in this
   tests PR if it is still open, otherwise in the fix PR before the fix.
4. **#468's route.** The top-up (`re_cover`) reaches the same sizing. Its script, #515's second,
   is pinned in #468 when it merges `main` with this fix.
5. **Lifecycle.** The correction lasts while the broker's report is ahead of the fills the fold has applied to that order, and no longer:
   - **an attributed fill** applies it: `filled_qty` catches up, and the unapplied part reads zero;
   - **an unattributed sell fill** in the instrument (a `FillApplied` with no `client_order_id`, after `ExternalActivityIngested`) has already taken the shares off the folded position. The fold keeps it in the instrument's pool of unattributed sells. Each ended sell nets its unapplied part against the pool, whichever arrives first (`OrderDetail.netted`). The netting runs after every account event, in one place (`fold::net_after`), so every way an order ends (`OrderStateChanged`, `OrderAbandoned`) and every unattributed fill passes through it. Without it the correction would outlive the fill and leave held shares bare (#518's review round 1, blocker 1; round 2, blocker 1, the `OrderAbandoned` path).
     - **Where it reads high:** the executor cannot tell its own fill that lost its `client_order_id` from a sale the owner made at the broker themselves; `BrokerFill` carries nothing that separates them. If the owner sells after the exit was submitted while the exit's report is unapplied, the owner's sale nets the report: the correction is cancelled, and the held position reads above what the broker holds by the netted part. Protection then over-covers (#518 round 2, major 2: Σ 7 against 4 held). This is inherited, not new: `main` never corrects, so it places the same or more. It is a §5.4 cap breach in that state, not accepted as how protection behaves, and the backlog's reconciliation-bounded correction closes it.
     - **The bound:** only an unattributed sell applied after the order was submitted can be netted, since one applied before cannot be that order's fill.
     - **A later attributed fill:** if one applies the same order's shares, it caps the netted part, and the excess is not returned to the pool. That sale was a different one, and returning it could net another order's report and over-cover.
     - **No new event:** all of this is folded from existing events;
   - **a restart** folds `reported_filled`, the fills and the netting from the journal, so it sizes the same way.

### Rationale

Rule 12 and §5.4: the sum of protective sells never exceeds the position. Waiting for the
reconciliation before re-placing would leave the remaining shares unprotected for an unbounded
time. Sizing on the broker's own report covers exactly what it holds, at once.

### Alternatives considered

- **Hold the re-placement until the reconciliation applies the fill.** Rejected: it opens a gap
  with no bound of its own.
- **Apply the reported fill in the fold at once.** Rejected: an order report carries no fill price
  or fees (`BrokerOrder`), so the fold would book a fill it cannot value. The fill update or the
  reconciliation's activities carry them.

<a id="dec-424"></a>

## DEC-424: A lone ladder is its exit's own: one instrument may hold one for each exit laddered in it

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-03 (UTC) |
| **Decided by** | The stream K builder (agent), on the coordinator's ruling on #468 (03:26Z), under DEC-79 and DEC-176: the change adds no sell and only tightens, since a remainder that was dropped is now counted and sent, so no safety rule is weakened |
| **Story or claim** | E7-4; issue [#533](https://github.com/kunwarshivam/mandate/issues/533), found by #468's random rule-13 property; tests PR, then the fix PR |

### Decision

1. **The rule: a lone ladder belongs to its exit.**
   - `ExecutorState.ladders` holds one lone ladder per exit, keyed by its instrument and its exit's intent. It used to hold one per instrument.
   - A second exit laddered in the same instrument, while nothing protects it, gets its own entry. It never replaces the first.
   - Each reader acts on the entry of the exit it names, and on no other:
     - `rung_submitted` adds the exit's own entry;
     - `rung_stepped` marks the stepped rung's own ladder;
     - the `GateDecided` park marks the parked exit's own ladder;
     - the `rung_short` fold, with nothing sent, ends the named exit's ladder only;
     - absorption at `unprotected_start` takes in only the sequence's own exit's ladder;
     - the end of a parked sequence carries its own ladder on as a lone one;
     - `lone_steps` steps and sends each ladder's rung;
     - `ladder_steps` asks each ladder's step.
2. **What `remainders` counts.** Every lone ladder in the instrument counts under DEC-410 item 1's rules: unless it is parked while its agent is paused or stopped, and unless its exit was abandoned. Together with a sequence's ladder, they share the room in their exits' intent id order. DEC-410 item 1 already set that order for two ladders, so no new rule is needed for more. A dropped remainder used to count nothing. Now it counts, so the gate's room for a discretionary exit can only shrink, never grow (DEC-410 item 3).
3. **Why it is a defect, not a choice.**
   - Before this decision, `rung_submitted` replaced the first exit's entry. Its stepped rung's remainder was then never sent, with no hold, denial or record. A risk exit stopped selling (`AGENTS.md` rule 13), in the same class as #489.
   - DEC-410 item 1, the spec's §5.3 rule 4 and §5.6 step 5, and #499's review all assumed at most one lone ladder per instrument. Nothing kept it so.
4. **Tests first (DEC-77).** Five pending tests in `sequence_tests` pin it on `main`. Each fails on its own assertion, not at a stub, so each is a `BEHAVIOUR_ONLY_TESTS` row until the fix deletes it:
   - `a_second_laddered_exit_never_drops_the_first_exits_remainder`: #533's minimal script. The first exit's next rung sends its 1, and the second exit's ladder still steps and sends its own; `main` sends nothing for the first.
   - `two_laddered_exits_parked_overnight_both_count_in_their_exits_id_order`: two exits parked between rungs overnight count 6 and then 4; `main` counts only the second.
   - `a_short_rung_that_sends_nothing_ends_only_its_own_exits_lone_ladder`: the `rung_short` fold. The other exit's ladder still counts.
   - `an_abandoned_exits_lone_ladder_counts_nothing_beside_another`: the abandoned guard. The other exit's ladder still counts.
   - `a_sequence_for_one_laddered_exit_takes_only_that_exits_lone_ladder`: absorption. The sequence takes its own exit's ladder, stepping and parked, and the other lone ladder stays.
5. **Lifecycle.** Each lone ladder enters and ends exactly as DEC-410 item 1 and DEC-409 say for one, independently of the others in its instrument:
   - it enters when its exit's rung is submitted laddered, or when its parked sequence's protection returns;
   - it ends when its exit's next rung is sent, when a short rung sends nothing, when a sequence for the same exit takes it in, or when its exit is abandoned (from then on it counts nothing);
   - a restart folds every entry from the journal, so it counts the same.

### Rationale

Rule 13: a risk exit is held only by agent mode `paused` or `stopped`, an `Unknown` order, or the broker, and is never silently dropped. Keying the ladders per exit removes the one place where a second exit could erase the first exit's progress.

### Alternatives considered

- **Hold the second exit until the first lone ladder ends.** Rejected: rule 13 allows no such hold.
- **Send the second exit without a ladder.** Rejected: an unladdered risk exit can rest unfilled at a stale limit, which delays risk reduction.
- **Merge both exits into one ladder.** Rejected: each exit has its own intent, quantity and record, and an exit abandoned or denied must not take the other with it.

<a id="dec-426"></a>

## DEC-426: DEC-399 item 8's in-place corrections are recorded, and rule 4 stands

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-03 (UTC) |
| **Decided by** | the stream G builder (claim [#123](https://github.com/kunwarshivam/mandate/issues/123)) under DEC-79, as a reversible process decision. It adds no permission and changes no rule |
| **Story or claim** | E6-4; the minors of [#530](https://github.com/kunwarshivam/mandate/pull/530)'s review, round 1 (m4) |

### Decision

1. **The record.** [DEC-399](DEC-399.md) item 8 was edited in place twice after it was accepted,
   each time on the coordinator's ruling:
   - [#504](https://github.com/kunwarshivam/mandate/pull/504) named the trim's minimum as the
     instrument's `min_order_size` and added its known-defect clause (option (a) on #123);
   - [#530](https://github.com/kunwarshivam/mandate/pull/530) corrected that clause's example,
     whose trim is the whole position on the venue's 0.0001 grid but 0.000116667 on the gate's
     1e-9 grid, so DEC-423 does not release it (the coordinator's ruling on
     [#520](https://github.com/kunwarshivam/mandate/pull/520)).

   Both edits departed from the [README](README.md)'s rule 4, which says to supersede an accepted
   decision rather than edit it. This decision records them as departures, so a reader of DEC-399
   knows its item 8 is not the text accepted on 2026-10-02.
2. **Rule 4 is unchanged.** It gains no clause for a correction a coordinator rules. From here
   on, a correction to an accepted decision, a figure or an example included, is a new decision
   that names what it corrects, and the old decision's file changes only in its status line.
3. **No other effect.** DEC-399's rule, DEC-423's exemption and every reference case stand as
   they are on `main`.

### Rationale

#530's review found the departure and offered two ways to record it: name the DEC-399 correction
in DEC-423 item 5, or give rule 4 a clause for corrections a coordinator rules. The first would
itself edit an accepted decision, a third departure to record the second. The second widens what
an accepted decision's file may take, which weakens the property rule 4 exists for: that an
accepted decision reads as it was accepted. A decision file of its own records both edits and
keeps rule 4 as strict as it was.

### Alternatives considered

- **Name the correction in DEC-423 item 5.** Refused: it edits an accepted decision to excuse
  editing an accepted decision.
- **Give rule 4 a clause for a coordinator-ruled correction.** Refused: it loosens a process rule
  to fit what was done, where recording what was done costs nothing.
- **Revert DEC-399 item 8 to its accepted text and move the corrections into new decisions.**
  Refused: it rewrites a decision the code and the reference already cite, for no change in
  meaning.

<a id="dec-428"></a>

## DEC-428: How `mandate-spec` reads V-047, and its recheck when a version is applied

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-03 (UTC) |
| **Decided by** | The stream H4 builder (agent), under DEC-79 and DEC-176: each reading follows the spec or takes its stricter side, and none loosens DEC-411 |
| **Story or claim** | E10-1, DEC-411's code; claim [#124](https://github.com/kunwarshivam/mandate/issues/124); [task brief](../tasks/E10-1-v047-in-mandate-spec.md) |

### Decision

1. **The policy is an input, not a lookup.** `ValidationContext` gains
   `independent_approval_required: bool`, the effective value after §4.3's hierarchy, and
   `ContextArgs` gains the same member, which `from_journal` carries as given. The policy owner
   computes it; `mandate-spec` reads no policy store. An absent policy key is `false`, as §4.3's
   "once `true` at a level, every child is `true`" reads it.
2. **The count stays `workspace_users: u32`.** A count can be absent in one place only, the
   identity service's `Membership`, and `ContextArgs::membership: None` already folds to zero
   users. Zero and one are the same for V-047 and for V-020, so an absent count refuses exactly as
   §4.1 says ("counts as one user"); it is never read as two. The refcase harness's absent member
   is the fixture default, one user. No `Option<u32>` is added, because nothing that builds a
   `ValidationContext` has an absent count to give it, and a new type would change every caller
   to say what the fold already says.
3. **The recheck at application is its own function.** `validate::recheck_at_application` returns
   the violated codes among V-002 and V-047, the two rules §4.1 says are checked again when a
   version is applied, and no others; an empty set lets the version apply. The caller folds the
   context at the moment of application, not the one the version was confirmed under. V-002 is in
   it because §4.1 names it, which only refuses more: the backlog row on the reference's V-002 gap
   (#528 round 2) stays the reference's.
4. **The apply-time case is a Rust test, not a case in the file.** A case file entry for a second
   user deactivated between confirmation and application would hold the same context as MC-V69 (the
   policy, one user): validation and the recheck read the same two facts, and the file has no
   application kind. So the case is `a_second_user_deactivated_before_application_refuses_the_version`
   in `crates/mandate-spec/tests/independence.rs`, folded through `from_journal` at confirmation and
   at application, with V-002's half beside it. The reference's V-047 stays validation-only, as
   DEC-411 item 5 records, until the V-002 reference row models application for both.
5. **The tests PR's stub refuses.** Until the implementation lands, `validate` returns
   `SpecError::Unimplemented` for a context with the policy on, never a report without V-047, and
   `recheck_at_application` returns it always. No caller on `main` sets the policy, so nothing that
   validates today changes. These are the stub's own guard, so the implementation PR deletes them with
   it, beside the `#[ignore]` lines under `tests/` (DEC-77): the two `Unimplemented` paragraphs in
   `src/validate.rs` and the stub-guard test in `src/validate/tests.rs` (#536 round 1, minor 3).
6. **The exemption is DEC-444's.** This item first read "no exemption", refusing every version in a
   lone workspace until the founder answered DEC-411 item 6. The founder answered on 2026-10-03
   (DEC-444): a version §9.2 classifies as risk-reducing against its previous version passes V-047;
   a neutral one, a risk-increasing one, a first version, and one whose previous document
   `PreviousVersion::mandate` does not hold are refused. The previous version must be the document
   whose hash is the agent's current `mandate_version` (DEC-444 item 3), and `PreviousVersion`
   carries no digest yet, so until it does the code refuses every version under V-047 (the refusing
   side, as DEC-444 item 3 notes for E10-1); the exception's test lands with the digest. When it can
   match, it classifies with `change::classify`, the classifier E10-3 uses, and no other.

### Rationale

The spec says what V-047 reads and when; these readings place it in the crate's existing shapes
with the fewest new types, and each keeps the fail-closed default rule 3 asks for.

### Alternatives considered

- **`workspace_users: Option<u32>`.** Rejected (item 2): every constructor states a count, and the
  one absent source already folds to a refusing value.
- **Recheck V-047 only.** Rejected: a function named for what §4.1 rechecks at application must not
  leave out the rule §4.1 names first.
- **Return the whole validation report at application.** Rejected: a rule that reads only the
  document cannot change between confirmation and application, and reporting one there would
  suggest it can.

<a id="dec-430"></a>

## DEC-430: Check 9's `mode` arm after DEC-318 option (a): reached by no named path, kept as defence in depth

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-03 (UTC) |
| **Decided by** | The stream M7 builder (agent), under DEC-79 and DEC-176. The reading changes no rule, keeps every check, and adds no risk |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-318](earlier-303-599.md#dec-318) option (a), the founder's decision of 2026-10-02 |

### Decision

1. **The reference model follows §6.4 "Cancellation" in a response's own step.** If `now.mode` is
   exits-only or stricter, `escalation_step` drafts `ApprovalCanceled` (`mode_tightened`) for every
   pending approval before it judges the response. The response then meets an empty pending set and
   is refused as `not_pending`. This matches the runtime (`mandate-runtime` `step::applied`, which
   cancels with `mode_tightened` whenever the effective mode is `ExitsOnly` or stricter) and
   DEC-131 item 25(j). The fuzz oracle (`fuzz_escalation`) clears its own pending set for such a
   step, and MI-21's "no approval outlives the tightening" check now also covers it.
2. **What reaches check 9's `mode` arm now: no path the spec names.** A grant reaches re-validation
   only after it is admitted. Admission needs a pending approval. In any step whose effective mode
   is stricter than `normal`, every pending approval is cancelled before admission. The arm is
   therefore unreachable in the reference model, through the runtime's `handle`, and from every
   MC-E case. The evidence is that with the arm removed (`if False:`), `generate.py` regenerates
   `mandate.yaml` byte for byte and `fuzz.py` seeds 1, 2 and 3 report 0 failures.
3. **The arm stays, as defence in depth.** `ref.py`'s `approval_revalidate` and
   `mandate_approval::revalidate` keep it. The spec lists it, removing a check would weaken a gate
   (DEC-79), and it is still the library's answer when a caller hands it a non-normal mode
   (`mandate-approval/tests/revalidation.rs`, `a_mode_other_than_normal_skips`). The docstring of
   `approval_revalidate` says it is reached by no named step.
4. **Seeded bugs.** `mutants.py` drops "re-validation ignores the mode", which item 2 shows is
   now an equivalent mutant that no oracle can catch. It adds "a response step judges before an
   exits-only mode cancels", which removes item 1's cancellation and is caught by the fuzz (MI-24:
   a grant would count while the oracle holds nothing pending).
5. **Backlog.** Three rows are carried on the backlog:
   - a spec sentence in §6.4 saying check 9's `mode` arm is defence in depth that no named step
     reaches. This is a spec PR and changes no rule;
   - the code PR that makes the harness strip the response step's `AgentModeChanged`
     (`restriction_changed`) and moves `the_lifecycle_cases_split_as_the_runtime_stands` to expect
     MC-E18 to pass, tests first;
   - the doc comment of `a_mode_other_than_normal_skips`, which still names MC-E18.

### Rationale

The founder chose option (a) and asked what now reaches check 9's `mode` arm. Item 2 answers:
nothing does, and item 3 keeps the arm rather than leaving it to read as a live path or deleting a
check. The reading only records and tests behaviour that the spec and the runtime already have.

### Alternatives considered

- **Delete check 9's `mode` arm.** Rejected. It weakens a listed check, which DEC-79 reserves for
  the founder, and it gains nothing.
- **Keep the old mutant.** Rejected. An equivalent mutant always survives, so `mutants.py` would
  fail for good.
- **Restate MC-E18 as a `batch` input.** Rejected. That is MC-E31. Option (a) names the response's
  own step, in which the mode reaches the runtime through the fold, not a cancelling input.

<a id="dec-435"></a>

## DEC-435: `webamp` in the web app's landing page

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-03 |
| **Decided by** | The founder (2026-10-03), recording the approval given when #363 merged |
| **Story or claim** | The web app (DEC-200); the dependency registry row for `webamp` |

### Decision

1. `webamp` (MIT) stays a dependency of `web/`, used only by the landing page's desktop and loaded
   client-side. It is on no trading, journal, or credential path.
2. The registry row in `docs/dependencies.md` cites this decision.

### Rationale

Every direct dependency needs an approval on record. `webamp` merged in #363 without one. The founder
kept it on 2026-10-03.

<a id="dec-450"></a>

## DEC-450: The first paper trade is on BTC/USD

| | |
|---|---|
| **Status** | Superseded by [DEC-509](earlier-303-599.md#dec-509) |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The founder: on 2026-10-03 (relayed by the coordinator on claim [#171](https://github.com/kunwarshivam/mandate/issues/171)), and again in session on 2026-10-07; recorded by an agent (Claude Code, the coordinator) |
| **Story or claim** | E7-7 and E7-19; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [DEC-475](earlier-303-599.md#dec-475), [DEC-502](earlier-303-599.md#dec-502) |

### Decision

1. The first paper order placed through the production path ([DEC-502](earlier-303-599.md#dec-502)) is on
   **BTC/USD**, on the founder's Alpaca paper account.
2. BTC/USD reaches that path as an input, in the confirmed mandate and the effective-dated
   instrument snapshot, never as a constant in shipping code ([DEC-475](earlier-303-599.md#dec-475) item 3).
3. Nothing about crypto changes for it: crypto sessions are continuous by the trading domain spec,
   and no session, gate, or account rule is altered or relaxed.
4. E7-7's one-shot AAPL assembly is not run. It stays only until E7-19 slice 5 deletes it
   (DEC-475 item 6).

### Rationale

Crypto trades around the clock, so the paper path can be exercised, restarted, and re-run at any
hour rather than inside the US regular session. The AAPL assembly is a shortcut the founder has
ruled out as progress (DEC-475): it compiles the instrument and model into the shell, and its
moving-average signal on recent AAPL prices produced no order.

### Alternatives considered

- **A US equity first.** It exercises the US account rules from the first order, but only during
  regular-session hours. Rejected for the first order; equities follow on the same path as inputs.
- **Run the AAPL assembly now.** Rejected: it proves the shortcut, not the production path.

### Revisit when

Never for the first order. Later instruments are mandate inputs and need no decision here.

<a id="dec-451"></a>

## DEC-451: Flatten planning validates sealed rows and includes only scoped unattributed exits (amends DEC-449)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-04 (UTC) |
| **Decided by** | The coordinating agent, under DEC-79: a reversible engineering decision that implements the already-approved journal and kill-switch rules without weakening either |
| **Story or claim** | E7-7; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); amends DEC-449 items 1 and 3 |

### Decision

1. **A flatten folds stored journal rows, not draft-shaped test doubles.** Before a stored body is
   passed through strict `Draft::parse`, the adapter parses its canonical object, requires its
   journal-assigned `seq`, `prev_hash`, and `recorded_at` to equal the row columns, removes exactly
   those three assigned members, and validates the remaining draft. Unknown writer-supplied members
   remain unknown and are refused. The normalized draft payload then goes through
   `mandate_executor::fold`.
2. **The sub-ledger still comes only from the deciding agent's attributed orders.** Ownerless
   orders never contribute quantity. The adapter derives the instruments the agent actually closes
   from that sub-ledger after applying an owner exit's optional instrument scope.
3. **Cancel candidates are wider than sub-ledger contributors, with a closed scope.** They contain
   every working order attributed to the deciding agent, plus a working order with no attributed
   agent — represented either by no owner or the executor's current `*` sentinel — only when its
   purpose is `Protective` or `RiskExit` and its instrument is one the resulting plan closes.
   Those two purposes are the unattributed protective legs and watchdog exits §5.5 permits,
   cancelled by `client_order_id`; every other ownerless purpose, every unrelated instrument, and
   every order attributed to another agent is excluded. The optional instrument scope is applied
   again when working orders are mapped, so this is never cancel-all. The shell pins both
   ownerless representations directly; the executor's current `*` writer cannot pass the journal's
   `id` schema and must be corrected by stream K before a conforming replay can contain it.
4. **DEC-449's other boundaries remain unchanged:** the read capability,
   stream, and optional clock arrive at `plan`; planning folds at request time; the runtime port is
   unchanged; sessions come from the committed calendar; broker quantities are not an input;
   working means nonterminal with quantity unfilled; runtime requests name intents while cancels
   name client orders; and the probe plans one synthetic kill switch from the mandate document.

### Rationale

`MemoryJournal` and Postgres store sealed bodies, so validating a stored body as if it were a draft
rejects the journal's own assigned fields and makes every real flatten unavailable. Conversely,
filtering to attributed orders before cancellation silently drops the ownerless protective and
watchdog orders trading-domain §5.4 and §5.5 require the closing agent to cancel. Separating
sub-ledger contributors from scoped cancel candidates preserves ownership while admitting exactly
the required risk-reducing cancellations.

### Alternatives considered

- **Accept the sealed fields in `Draft::parse`:** rejected because drafts do not own those members;
  allowing them would weaken writer validation and blur the journal boundary.
- **Skip draft validation for stored rows:** rejected because replay must not admit a payload the
  journal would have refused before sealing.
- **Pass every ownerless order to an agent flatten:** rejected because it would cancel protection
  and watchdog exits in instruments the agent does not close.
- **Treat ownerless quantity as the deciding agent's position:** rejected because §5.5 requires the
  flatten to sell only that agent's attributed sub-ledger.

<a id="dec-453"></a>

## DEC-453: Close the reconciliation-run journal schema

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-79 and DEC-176 |
| **Story or claim** | E7-7; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [PR #598](https://github.com/kunwarshivam/mandate/pull/598) |

### Decision

1. `ReconciliationRun` schema version 1 is the executor's existing complete record:
   `result` (`clean`, `adopted`, or `mismatch`), nullable `checkpoint`, non-negative
   `snapshot_head`, non-negative `differences`, and whole-second `risk_clock`.
2. An absent activity checkpoint is journaled as `null`, not an empty string. A non-empty
   checkpoint remains opaque.
3. The schema stays closed. Unknown fields, unknown results, non-canonical clocks, and malformed
   field types are refused by the journal.

### Rationale

The journal catalogue already assigns `ReconciliationRun` to the executor-owned account stream,
the trading-domain reconciliation lifecycle requires the record, and the executor already emits
these five facts. The missing registration made every real startup reconciliation fail closed as
`unknown_schema`. Registering the exact emitted shape resolves that omission without relaxing
validation. Representing no checkpoint as `null` follows journal §4.2 and preserves the executor's
existing `None` state.

### Alternatives considered

- Leave E7-7 blocked on a separate schema story: rejected because the event and writer are already
  specified and implemented, and the missing closed schema is the integration defect E7-7 exposed.
- Accept an open object or bypass validation: rejected because either weakens the journal contract.
- Accept the empty string checkpoint: rejected because journal §4.2 reserves empty values for
  `null`.

<a id="dec-457"></a>

## DEC-457: Bind executor decisions without widening the closed gate schema

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The founder, for the one-PR delivery instruction retained from DEC-463; the implementing agent under DEC-79 and DEC-176 for the conservative schema-compatible boundary |
| **Story or claim** | E7-7; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [PR #598](https://github.com/kunwarshivam/mandate/pull/598) |

### Decision

This decision supersedes DEC-463 and preserves all of its safety and delivery rulings with one
schema-compatible correction:

1. `mandate-executor::handle` requires a pure input source for trusted values outside its
   account-stream fold. The source returns facts, never a verdict.
2. The executor derives `ProposedOrder` from its own intent and calls `mandate_risk::evaluate`
   directly. An advisory result cannot override a binding denial.
3. Existing account-stream checks run first and may only narrow the result. A complete binding run
   journals the full gate evidence. A local-only denial uses the closed `GateDecided.data_profile`
   value `account_stream_only`, empty quote and mark evidence, and the checks it actually reached;
   it does not add the unregistered `evaluation` member.
4. Missing, mismatched, or unreadable binding inputs fail closed for an opening or increase. They
   do not turn snapshot availability into a new denial of a risk reduction.
5. The boundary introduces no network or broker activity and does not change paper-only scope,
   journal-before-acting, CI, mutation, or independent-review requirements.
6. E7-7 remains one consolidated pull request, superseding DEC-77's multi-PR sequence and
   DEC-166's delivery slicing for this story while retaining their safety gates.

### Rationale

Journal §9.5 closes `GateDecided` and rejects unknown members. `data_profile` already records which
evidence profile produced a decision, so using `account_stream_only` there preserves DEC-463's
distinction without widening a closed record or losing the denial's reached checks. The change is
strictly representational and cannot turn a denial into an allow.

### Alternatives considered

- Add `evaluation` to schema version 2: rejected because it widens an accepted closed schema and
  duplicates `data_profile`.
- Drop local-only denial records: rejected because a binding refusal must remain journaled.
- Treat missing binding evidence as an allow: rejected by rule 3 and DEC-463's binding boundary.

<a id="dec-462"></a>

## DEC-462: Permit the atomic E7-7 migration only on PR 598

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The founder under DEC-79 |
| **Story or claim** | E7-7; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [PR #598](https://github.com/kunwarshivam/mandate/pull/598) |

### Decision

1. ES-22 permits protected spec and reference files to land with their matching runtime code only
   for E7-7 in PR #598.
2. The exception requires PR #598 and the exact PR-body marker
   `ES-22-atomic-exception: E7-7 PR #598 (DEC-462)`. No other pull request, branch, marker, story,
   or mixed change is exempt.
3. All tests, reference checks, mutation gates, CI, and independent review remain required.
4. The exception does not authorize an Alpaca paper send. DEC-450 still requires separate,
   explicit founder confirmation, paper-only credentials, and a founder-selected instrument.

### Rationale

The attempted protected-only split in PR #599 fails the lifecycle harness because the corrected
closed reference shapes and the runtime writers must change atomically. Landing either half first
would knowingly leave accepted reference cases and runtime behavior inconsistent. A PR-number and
exact-marker check contains the exception to the migration the founder approved while preserving
ES-22 for every other change.

### Alternatives considered

- Split the protected half into PR #599: rejected because its lifecycle tests fail without the
  matching runtime half.
- Weaken or skip the lifecycle tests: rejected because every approved reference case must remain
  enforced.
- Allow a general atomic-migration escape hatch: rejected because it would broaden ES-22 beyond
  E7-7.

<a id="dec-464"></a>

## DEC-464: Keep required CI under ten minutes by sharding mutation tests

| | |
|---|---|
| **Status** | Superseded by DEC-473 |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-79 |
| **Story or claim** | CI scalability correction requested by the founder; PR #601 |

### Decision

1. Every required pull-request check has a hard ten-minute timeout. The branch-protection names
   remain `ci / fast` and `ci / full`.
2. `fast` remains unchanged. The old serial `full` work is split into:
   - `full-checks`, which runs fixture drift, reference, supply-chain, and PostgreSQL checks;
   - twelve deterministic, zero-based slice `cargo-mutants` shards, each with its own unmutated
     baseline and existing live-test preflight; and
   - `full`, a quick aggregate that succeeds only when `full-checks` and every mutation shard
     succeed.
3. `cargo xtask check` remains the complete local oracle. It runs the mutation gate once,
   unsharded. `cargo xtask ci full` names only the non-mutation full checks; CI invokes
   `cargo xtask ci mutants` once per shard through `MANDATE_MUTANT_SHARD=INDEX/TOTAL`.
4. Sharding changes scheduling only. It does not change the selected source diff, packages,
   mutant count, test command, baseline requirement, live-test preflight, stub exemptions, or
   zero-survivor rule.

### Rationale

PR #598 measured `fast` at 6m20s and the old `full` at 55m48s. Non-mutation full work consumed
about 3m18s and the mutation baseline 3m36s; executing 330 mutants consumed about 47m23s. The
serial mutation loop therefore determined almost the entire critical path. Twelve shards reduce
that execution component to roughly four minutes per runner while retaining an independent
baseline and the same gate on every shard.

The ten-minute timeout makes the latency requirement enforceable rather than aspirational. A
future growth in the diff or test suite fails visibly instead of silently restoring hour-long
feedback.

### Alternatives considered

- Skip each shard's baseline after one shared baseline: rejected because a shard must fail closed
  on the exact source and test command it judges.
- Reduce mutation scope, mutant count, or live-test checks: rejected because that weakens the
  safety gate.
- Keep a 60-minute serial job: rejected because it blocks development feedback and already took
  55m48s on PR #598.
- Buy larger runners or add a CI vendor: deferred; deterministic native sharding needs no new
  dependency, service, credential, or spending decision.

<a id="dec-465"></a>

## DEC-465: Deploy the web app from the production branch only

| | |
|---|---|
| **Status** | Accepted; items 1 and 3 superseded by [DEC-823](earlier-600-899.md#dec-823) once its slice S11 moves the web app to Cloudflare Workers. Item 2 stays in force |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-79 |
| **Story or claim** | Vercel preview rate-limit correction requested by the founder; PR #602 |

### Decision

1. Vercel's Git integration disables automatic deployments for every branch except `main`.
   Pull-request pushes no longer create preview deployments; commits to the production branch
   continue to deploy.
2. GitHub's `web` workflow remains the pull-request gate for changes under `web/`: lint, type
   checking, unit tests, design checks, and the production build still run there.
3. The policy lives in repository-root `vercel.json`, where the connected Vercel project reads its
   Git configuration, so it is versioned and reviewable rather than held only in dashboard state.

### Rationale

Vercel creates a preview deployment for every pull request by default, including backend-only
changes. Those deployments consume the project's rate and concurrency limits without adding
evidence beyond the repository's required web checks. Production-only deployment retains the
hosted application while removing that duplicate per-PR work.

Vercel's `git.deploymentEnabled` branch map is evaluated before it creates an automatic
deployment. The minimatch `**` wildcard disables every branch, including slash-named
`cursor/...` branches; the explicit `main: true` wins when both rules match. This avoids both the
single-star pattern, which does not cross a slash, and the weaker ignored-build mechanism, which
first creates and then cancels a deployment and can still consume concurrency.

### Alternatives considered

- Deploy previews only when `web/` changes: rejected because the remaining preview volume can
  still exhaust the rate limit, and GitHub already validates web changes.
- Change only the Vercel dashboard setting: rejected because it is invisible to repository review
  and can drift.
- Disconnect Vercel from GitHub: rejected because production deployments should remain automatic.

<a id="dec-466"></a>

## DEC-466: Use the reviewed E7-7 artifacts for the one manual paper migration

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-79 and the founder's one-order paper approval |
| **Story or claim** | E7-7; claim #171; PR #603 |

### Decision

1. The E7-7 binary takes an explicit config directory and accepts only the reviewed AAPL,
   `quant.ma_crossover`, paper-environment artifact shapes used by the E7-7 fixture.
2. Each artifact is read once. The run refuses one that is missing or malformed, that lacks a member
   it uses or carries one it does not, or that is differently dated or scoped. The bytes it checked
   and the canonical mandate are hashed into the binding gate's configuration references. All of
   this happens before any credential is read.
3. The binary constructs only `AlpacaPaperHttp` from the paper credential variables, then reads
   the broker with GETs only, assembles the trusted contexts from that snapshot and the stored
   datasets (DEC-470), and only then calls the production stage assembly. It has no disconnected
   fallback and no configurable host.
4. This is the one E7-7 migration run. It is not a general deployment configuration and does not
   authorize another instrument, a live host, live credentials, or more than one submission.

### Rationale

PR #598 completed and verified the deterministic path but deliberately left the shipping binary
without its trusted contexts or transport. Reusing the exact independently reviewed fixture
artifacts closes that assembly gap without adding a second set of trading rules or a general
configuration surface. The existing dry-run flag, durable journal requirement, write-ahead
ordering, one-submission guard, paper-only transport, and restart reconciliation remain unchanged.

### Alternatives considered

- A general deployment-config service: rejected as M9 scope and unnecessary for one paper
  migration.
- Keeping the disconnected binary and sending with a separate script: rejected because it would
  bypass the tested journal, executor, and reconciliation path.
- A configurable Alpaca host: rejected because the paper boundary must remain unrepresentable.

<a id="dec-470"></a>

## DEC-470: Read the E7-7 paper run's facts from the broker and stored data, and refuse the rest

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-176 (each reading tightens a rule or closes a gap by the reading that adds no risk) and DEC-79 |
| **Story or claim** | E7-7; claim #171; PR #603 |

### Decision

The one E7-7 paper run (DEC-466) assembles its trusted contexts from one `PaperFacts` snapshot and
nothing else. The snapshot is filled only by GETs to Alpaca paper (the account, positions, open
orders, the asset record, the latest IEX quote, and, by DEC-471, the trailing window's IEX minute
bars) and by the stored daily dataset. Every fact that is
missing, stale, or ambiguous refuses the run before anything is sent (`AGENTS.md` rule 3).

1. **Clean account only.** A paper account holding any position or any open order is refused
   (`a paper account with no position and no open order`). This first-order migration maps no
   existing broker state into the gate's account, agent, or reservation snapshots, so the gate
   snapshot's empty positions and working orders are true by construction. A restart after the one
   submission therefore refuses at the preflight once the broker shows the order, and at the
   executor's one-cycle guard (`cycle_already_open`) before it does.
2. **Quote and asset age.** The rule-set artifact names `iex_quote_max_age_s`, an integer from 1 to
   60 (the fixture says 10). The latest IEX quote and the asset record must each be read no earlier
   than that many seconds before the run clock and not after it, and the quote must be uncrossed
   (`bid ≤ ask`). The preflight clock is read after the GETs with nanoseconds. The binding gate
   reads the injected process clock again for every request, replaces the template's earlier time,
   and refuses if the quote has become stale or the regular session has reached its close window.
3. **Builder limit and protection from the live quote.** The builder's bid and ask are the live
   quote's, so the entry limit is the live ask. The bracket's protection is derived from the
   mandate's validated distances and that ask, with exact decimal operations only: the stop is
   `ask × (1 − stop_distance)` and the take-profit `ask × (1 + take_profit_distance)`, each rounded
   up onto the Reg NMS grid (trading-domain spec §2.1) because both are sell prices. At an ask of
   255.20 with distances 0.05 and 0.10 that is 242.44 and 280.72. The derivation is
   `mandate_executor::equity_bracket_prices` (DEC-471 item 3). Classification must carry the
   sized proposal's exact notional; a one-share action cannot classify a proposal sized to any
   other quantity.
4. **Liquidity facts.** The daily dataset must pass the same trust check as the signal's bars.
   From its last 20 listed sessions: the prior close is the last close; the 20-session median dollar
   volume is the lower of the two middle values of `volume × close`, which is never above the true
   median; ADV is the mean volume truncated to whole shares. The trailing five-minute volume sums
   the AAPL 1-minute IEX bars that start no earlier than five minutes before the run clock. A bar
   ending after the run clock is refused, as is a window with no bar. IEX volume understates
   consolidated volume, so each figure errs toward the tighter limit. The pure
   `mandate-liquidity` crate computes all four from typed inputs; `mandate-shell` validates and
   parses stored daily bars and maps broker minute bars. The minute bars are read from the broker
   by DEC-471. A stored minute dataset (`--minute-dataset`) is no longer read, because
   `mandate download` cannot store today.
5. **Buying power and fees.** The gate's model buying power is the lesser of the broker's cash and
   non-marginable buying power (`BrokerAccount::one_x_buying_power`). That figure is §7.2's
   enforced 1× cap, which check 7 applies
   (DEC-129 item 7), so an account whose `multiplier` is not 1 is not refused for it. An account
   with nonzero accrued fees is refused.
6. **No status feed.** The run reads no halt or status feed, so the gate snapshot says
   `halted: false` with `status_feed_current: false`, which keeps market orders barred (§9.6). The
   fresh, uncrossed quote in item 2 is the only market-state evidence the run acts on.
7. **Session.** The run clock must be inside the New York regular session and before its last
   `close_window_minutes` (from the gate configuration); otherwise the run refuses.
8. **Instrument artifact.** The ETP class and `etp_classified_at` come from the instrument artifact.
   A classification dated after the run clock refuses, because the gate itself does not refuse a
   future-dated one. The broker's asset record must carry the artifact's asset id and symbol, the
   `us_equity` class, and the Nasdaq exchange.
9. **Risk equities.** Agent equity, the high-water mark, the day-start equity, and the capital base
   equal the mandate's `allocation_usd`: a first deployment has no history to read. The shipping
   CLI therefore has no `--new-cycle`; any replayed intent refuses with `cycle_already_open`, so
   these first-deployment values cannot reset realized loss, drawdown, cooldowns, or daily counts.
10. **Fee reservation.** Each binding gate request carries `round(estimated fees, 2, ceiling)` for
    its own order (trading-domain spec §7.2, DEC-129 item 16), computed by
    `mandate_accounting::fee_reservation` (DEC-471 item 3). An equity buy is priced by folding it,
    as a fill at its limit, into an empty ledger under the reviewed paper fee configuration, so the
    fee rule is accounting's own rather than a second copy; under that schedule a one-share buy
    reserves 0.01. A sell reserves zero, because the gate reads the reservation only in check 7,
    which only an opening reaches. An order the schedule cannot price gets no gate input, which the
    executor treats as unavailable.

Recorded with these readings, not decided by them: Alpaca's day-trading regime is
`intraday_margin` (trading-domain spec §7.2 and §9.2); `maintenance_excess` and `prior_close_equity`
are zero because, under that regime, the gate denies nothing per order on them (DEC-129 item 7);
and `median_dollar_volume_30d` and the last trade are absent.

### Rationale

The independent review of PR #603 found shipping facts typed into the assembly rather than read.
Every reading here replaces such a value with a broker GET or a stored-dataset rule, or refuses where
no source exists. Requiring a clean account removes the need to map arbitrary broker state, which
is the part that could understate exposure. Each item only narrows when the run may proceed, so
DEC-176 lets an agent accept it.

### Alternatives considered

- Mapping existing positions and orders into the gate snapshots: rejected for this run; no reviewed
  mapping from broker state to the gate's account, agent, and reservation snapshots exists yet, and a
  wrong one would understate exposure.
- A zero fee reservation: rejected; the paper schedule charges a buy the CAT fee, so zero would
  understate the cost check 7 compares.
- Taking the trailing volume from the quote endpoint or leaving it unset: rejected; the quote
  carries no volume, and an unset value would fail the participation cap.
- Floating-point protection prices or the stored close as the entry: rejected; floats are barred in
  order paths, and a stale close would misplace the limit and the bracket.
- A consolidated (SIP) feed: rejected; the paper plan reads IEX only, and IEX errs conservative.

<a id="dec-471"></a>

## DEC-471: Read the E7-7 trailing volume from Alpaca's IEX minute bars, and compute the paper run's figures in the crates that own them

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-05 (UTC) |
| **Decided by** | The implementing agent under DEC-79 and DEC-176; the extra operation is GET-only, bounded, fail-closed, and adds no trading authority |
| **Story or claim** | E7-7; claim #171; PR #603 |

### Decision

1. **One more GET on the data host: AAPL's latest complete IEX minute bars.** This amends DEC-168
   item 2, under which the latest quote was the data host's only request.
   `DataClient::recent_minute_bars(instrument, window)` sends a `BarsRequest`. That type has
   private fields and is built only by `BarsRequest::recent_minutes(instrument, now, window)` at the
   clock's now:
   - **Equities only.** A pair, or a symbol that `symbol_segment` refuses, is refused.
   - **Window.** The window must be a whole number of minutes, from one to fifteen
     (`MAX_BARS_WINDOW`).
   - **Bounds.** The first bar starts on the first minute boundary at or after `now − window`. The
     last bar starts one minute before the last boundary at or before `now`, so every bar asked for
     has closed by `now`. An empty range is refused.
   - **Current day.** The range must lie on `now`'s New York date, so the read never reaches into a
     previous session.
   - **Path.** `/v2/stocks/{symbol}/bars?timeframe=1Min&start={first}&end={last}&limit={n}&adjustment=raw&feed=iex&sort=asc`.
     Both bounds are inclusive whole-second RFC 3339 times, and `n` is exactly the number of
     minutes, so one page holds every bar.
   - **Host.** The request goes only through `DataTransport::send_bars`. `AlpacaPaperHttp` dials it
     at the compiled `DATA_HOST` with `GET`, and only if the URL parses back to exactly the path
     that was built. ES-23's two hosts are unchanged.
2. **Every answer but one complete page of bars inside the window is refused** (`AGENTS.md` rule 3).
   Each refusal is a `ReadError` with a stable code:
   - `paginated`: a non-null `next_page_token`.
   - `other_instrument`: another `symbol`.
   - `absent`: a `null` or empty `bars`.
   - `missing_field`: no `bars` or no `next_page_token`.
   - `out_of_window`: a bar whose start is off the minute grid, not after the bar before it, or
     outside the requested range.
   - The statuses and the wire and transport codes are as in DEC-168 item 4.

   Each volume is read from its JSON number token's own text into a `mandate-num` `Qty`. An
   exponent, a string, more than nine places, and a negative number are refused, and an `f64` is
   never used. Together with item 1, no bar is older than the window, and none was still open when
   the request was built.
3. **Each figure is computed in the crate that owns its rule, and the shell only maps and
   orchestrates** (DEC-138 item 3):
   - **`mandate-liquidity`.** The pure `daily_liquidity` computes the prior close, the lower median
     of `volume × close`, and the mean volume truncated to whole shares over the last 20 typed
     daily inputs (DEC-470 item 4). `trailing_volume` sums typed minute inputs that start no earlier
     than five minutes before the run clock. It refuses a bar that ends after the clock, starts that
     do not strictly increase, and a window with no bar. `mandate-shell` only validates the stored
     dataset, parses its bar fields into `Price` and `Qty`, maps broker bars, and orchestrates.
   - **`mandate-executor`.** `equity_bracket_prices` derives the bracket's stop and take-profit
     (DEC-130 item 18, DEC-470 item 3). `BrokerAccount::one_x_buying_power` gives the 1× cap
     (DEC-470 item 5).
   - **`mandate-builder`.** `buy_action` states an opening's classification facts by the rule
     `propose` itself uses.
   - **`mandate-accounting`.** `fee_reservation` prices a `ProspectiveOrder` (DEC-470 item 10). An
     equity buy is folded into an empty ledger. A crypto buy and every sell reserve zero without any
     computation, so a reservation cannot stand between an exit and the gate (`AGENTS.md` rule 13).

   The shell still compares and moves instants for the quote age, the session, and the close
   window. It does no arithmetic on money or quantity.
4. **`--minute-dataset` is removed.** The preflight reads the five-minute window's bars at its own
   clock. The run clock is read after the preflight, and the trailing volume is judged at it. A
   minute that closes between the two clocks is not counted, which only lowers the volume and so
   tightens the participation cap. If more than five minutes pass, the window is empty and the run
   refuses.

### Rationale

DEC-470 item 4 took today's minute bars from a stored dataset. However, `mandate download` refuses
an `--end` on or after today, so a real run could only ever refuse. The bars endpoint is on the
host the latest quote already uses, with the same credentials and the same IEX feed (trading-domain
spec §4.2). It is read-only and is bounded by type to the current session's closed minutes.

The earlier assembly also computed the median, the averages, the trailing sum, the bracket prices,
the classification facts, and the fee reservation in `mandate-shell`. DEC-138 item 3 forbids that.
Each rule now lives in the crate that owns it and has its own tests there.

### Alternatives considered

- **Letting `mandate download` fetch today:** rejected. A partial day stored as a dataset could later
  be read as complete, and that would loosen every reader of the store.
- **A streaming minute-bar subscription:** rejected for this run. It is a new long-lived surface, and
  one bounded GET answers the question.
- **Following `next_page_token`:** rejected. The limit already covers every minute asked for, so a
  second page means the answer is not the one requested.
- **A default or zero trailing volume when IEX printed no bar:** rejected. An unknown volume is not
  zero; the run refuses instead.

### Mutation boundary

`mandate-liquidity` is a pure layer-1 safety-critical crate because its figures feed the binding
gate's liquidity floor and participation caps. It depends only on `mandate-num` and
`mandate-time`; its changed source therefore enters the required mutation gate with the same
zero-missed-mutant rule as the other order-path crates. `mandate-marketdata` remains an impure
layer-6 adapter and is not marked safety-critical solely to pull this arithmetic into mutation
testing.

<a id="dec-473"></a>

## DEC-473: Expand the mutation matrix when a safety-critical diff outgrows twelve shards

| | |
|---|---|
| **Status** | Superseded by DEC-498 |
| **Date** | 2026-10-06 (UTC) |
| **Decided by** | Cursor agent under DEC-79 |
| **Story or claim** | E6-13 implementation; PR #618 |

### Decision

1. Supersede DEC-464's twelve-shard mutation matrix with twenty-four deterministic, zero-based
   slice shards.
2. Keep the ten-minute timeout, complete source diff, selected packages, live-test preflight,
   unmutated baseline, test command, and zero-survivor rule unchanged.
3. Keep `cargo xtask check` unsharded as the complete local oracle.

### Rationale

E6-13 adds 169 mutants across `mandate-executor`, `mandate-refcases`, and `mandate-spec`. In the
twelve-shard CI run, each shard selected fourteen or fifteen mutants. Shards 0 through 6 reached
the ten-minute hard limit, while shards 7 through 11 completed in two to seven minutes. The
unmutated baselines consumed about one to two minutes; the contiguous source ordering then put
the slower executor and harness mutants together.

Twenty-four slices select at most eight mutants each; with this 169-mutant diff, the final
populated slice has one mutant and two trailing slices are empty. This bounds mutation work
without reducing coverage or relaxing the timeout, and it preserves deterministic partitioning
with the existing `cargo-mutants` mechanism.

### Alternatives considered

- Increase the ten-minute timeout: rejected because it weakens the latency gate DEC-464 made
  enforceable.
- Reduce the selected diff, tests, or mutant count: rejected because it weakens a safety-critical
  gate.
- Change to round-robin partitioning: rejected because mutant runtimes are not stable enough to
  prove that interleaving alone keeps every shard below the limit.
- Add larger runners or an external CI service: rejected because it introduces spending or a new
  service when the native matrix can retain the same proof.

<a id="dec-475"></a>

## DEC-475: Make model output the production-cycle input and paper an environment adapter

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-06 (UTC) |
| **Decided by** | The founder, in session |
| **Story or claim** | E7-19; E7-7 production follow-up; claim #171 |

### Decision

1. `mandate_runtime::Input::ModelOutput` is the production cycle's strategy boundary. A model
   gateway supplies a typed output carrying its registered model identity, version, content hash,
   instrument, observation time and expiry. The runtime verifies it against the confirmed mandate;
   deterministic builder, autonomy, gate and executor code alone may turn it into an order.
2. The production shell takes one validated deployment input: opaque workspace, agent, connection
   and account references; the confirmed mandate version; effective-dated policy, gate, executor,
   calendar, fee, instrument and model-registry snapshots; the authoritative account and market
   snapshots; and the model output. Each configuration is content addressed and journaled.
3. Instrument, model, deployment identity, gate values and executor values are inputs. Production
   assembly may not select AAPL, `quant.ma_crossover`, fixed deployment IDs, or a compile-time
   `GateConfig` or `ExecutorConfig`. Types and constructors enforce that the complete validated
   input exists before a cycle can start.
4. Paper and live use the same production-cycle API. Only connector, credential and environment
   policy adapters differ. This repository continues to compile no live-order adapter and never
   places a real order (`AGENTS.md` rule 8).
5. Tests call that same API through recorded paper transports. There is no synthetic strategy,
   proof mode, test-only execution branch, or special signal that shipping code can select.
6. E7-7's one-shot AAPL assembly remains only while E7-19 migrates each input to the production
   boundary. It is deleted when the paper adapter calls the production API. This amends DEC-466
   items 1 and 4; its paper-host, reviewed-artifact, preflight and one-submission safeguards remain.
7. The migration is kept runnable after each slice. Existing paper preflight remains available
   until its replacement passes the same fail-closed, journal-before-send and restart-reconciliation
   checks. No slice weakens or skips those checks.

### Rationale

E7-7 proved the crate wiring but coupled the external-order acceptance test to one instrument and
one market-dependent strategy. Its recorded fixture was long while the current AAPL dataset was
flat, so the reviewed manual path could not produce the order it existed to prove. Adding another
ticker, synthetic model or proof flag would preserve the defect: product choices would still live
in the shell.

The runtime already has the correct boundary. It consumes `ModelOutput` as an opinion and emits
ordered effects; it does not call a model or broker. Making that boundary the production API lets
the model gateway, deployment manager and broker adapters evolve independently while all
environments use the same mandate, builder, gate, journal, executor and reconciliation path.

### Alternatives considered

- Add a deterministic proof model or `--integration-proof`: rejected because it creates a
  test-only product path and proves neither a real model nor normal input assembly.
- Change AAPL or the moving-average windows until a long signal appears: rejected because it
  outcome-chases and leaves deployment choices compiled into the shell.
- Generalize only the tracer CLI flags: rejected because strings at the CLI are not validated,
  versioned deployment inputs and do not close the configuration or journal-reference gaps.
- Wait for the control plane before changing the shell: rejected because the typed production API
  is the contract those services need and lets them be built in parallel.

<a id="dec-483"></a>

## DEC-483: Changing a mandate from the Edit form and from a message

| | |
|---|---|
| **Status** | Accepted (founder direction 2026-10-06 for the two ways in; agent, reversible engineering, DEC-79, for the rest) |
| **Date** | 2026-10-06 (UTC) |
| **Decided by** | The founder: "you should be able to edit mandates through agent chats", then "well edit form could work but also through chat" (one typo corrected). A Cursor agent wrote the items (branch `cursor/mandate-edit-a2cd`) |
| **Story or claim** | A6 Mandate › Edit, W4 web redesign (claim #297); the A6 and D14 entries of the [product-experience brief](../../product/09-product-experience.md) |

### Decision

1. **Two ways in, one path.** The owner changes a running agent's mandate from Mandate › Edit or
   by saying so in the agent's thread or the copilot. Both build the same proposed version, diff it
   against the version in effect, classify every changed path by mandate spec §9.2, validate it
   (§4.1 ranges and V-rules, the retail profile of §4.3, and §5.1 for capital), and show the same
   review (`ChangeReview`, `web/src/components/mandate/change-review.tsx`) with one confirmation.
   The core is `web/src/lib/mandate-change.ts`; the deployment repeats every check when it applies
   a version.
2. **What can be changed here:** capital, the lifetime and daily loss limits, the largest order,
   the largest position in dollars and as a share of equity, total holdings, orders a day, the
   protective stop, the approval window, the two-approver threshold, and quiet hours when they are
   set. Instruments, signal models, the owner's rules, the goal, the drawdown ladder, the
   environment and the connection are not; a message asking for one says so and lists what can be.
3. **The review is A6's.** In a message it opens inline, under the owner's words, rather than as a
   link to another screen, and it records the same thing: screen A6, the environment, and every
   line the owner saw, in order (`MandateConfirmed`, mandate §10). A version from a message carries
   `user_stated` provenance with the owner's words quoted; one from the form carries
   `user_entered`.
4. **Reading a message is deterministic.** Fixed phrases name a limit, and the new value follows
   "to", in figures, in that limit's unit (`readChange`, `web/src/lib/change-request.ts`). Several
   clauses may be joined by "and", commas or semicolons. A difference ("by $200"), a value in
   words, a shorthand like "2k", a limit named twice, or a clause it cannot place is asked about,
   never guessed. A question is answered as a question. Asking what a limit should be gets "I don't
   suggest limits: they're yours to set" (the No-Nudge Rule). No model reads or writes any of it.
5. **Owner only.** Confirming a version uses the `agents.deploy` capability, the owner's alone, as
   creating an agent does (rule 11). Other roles see the review without a way to confirm it.
6. **How a version applies** (§2.2, §5.1):
   - Risk-reducing and neutral versions apply on confirmation, with no passkey.
   - A risk-increasing version takes the passkey, is recorded as waiting, and applies at the
     agent's next check with no order in an unknown state. The version before it stays in effect
     until then, and Versions shows it as waiting.
   - When a version applies, requests waiting for the owner are canceled (`version_applied`), and
     resting opening orders larger than the new limits are canceled. Limits already reached stay
     in force: no version clears a latched limit.
   - A capital change scales the agent's equity, high-water mark and loss marks by the same factor,
     rounded up, so no loss fraction falls.
7. **One version waits at a time.** A version confirmed while another waits replaces it: the waiting
   one is rejected with "Version N, confirmed after it, replaces it." A reducing change is never
   held behind a waiting increase, and the review says in advance that the waiting version will not
   apply. A review shown before another version applied is refused rather than applied on top of
   it, and records no version.
8. **Amends DEC-479 item 5.** Acting from a message now covers two things sent from a card the
   owner confirms: Pause, and a new version of the mandate's limits. Every other action still opens
   Stop or setup, and no order is placed from a message.

### Rationale

The founder asked for both: a form for the owner who knows which field to change, and a message for
the owner already talking to the agent. Rule 11 needs every change to the envelope to be a confirmed
version, so the two must not differ in what they check or record; one core and one review make
that structural, not a matter of keeping two screens in step. Reading the message by fixed phrases
keeps "LLMs produce opinions, never orders" true for limits too: the value the owner confirms is the
one they wrote, read back to them in the review before anything is sent.

### Alternatives considered

- **A link from the message to the Edit form, prefilled:** rejected. The owner would confirm on a
  second screen what they had just said on the first, and a prefilled form invites confirming
  values not looked at.
- **A model reading the message**, as setup does under DEC-476: deferred. Changing a running
  agent's limits is narrower than describing a new one, the phrases cover it, and a fixed reader
  needs no model decision from the founder (DEC-476 item 7).
- **Holding a reducing change behind a waiting increase:** rejected. Reducing risk never waits for
  approval or for a riskier change (rule 2).
- **Letting two versions wait:** rejected. The order they would apply in is not visible to the owner.

### Consequences

- Mandate › Edit is built; the brief's A6 primary action exists for the fields in item 2.
- Not covered: independent approval where a workspace policy requires it, and the policy
  nonconforming state (the fixtures have no workspace policies yet); editing instruments, signal
  models, rules, the goal and the ladder.

<a id="dec-488"></a>

## DEC-488: `ApprovalResponded.quorum`: its shape, when the runtime writes it, and what it reads

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The stream M7 builder (agent), under DEC-79 and DEC-176: the reading records what check 7 already applied, changes no outcome, and adds no risk |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-317](earlier-303-599.md#dec-317) item 7; journal spec §9; mandate spec §6.4; DEC-173 item 13; DEC-257 items 1 to 3; DEC-278 item 3 |

### Decision

1. **The shape.** The member is `quorum: {required: <int>, independent: <bool>}`, the two keys the
   request's content object already carries under `approvers` (journal spec §9's
   `ApprovalRequested` row), so the record is read the way the request is. `required` is the
   approver count check 7 applied, as a canonical integer, and `independent` whether the mandate's
   author was excluded from the grants that count. No other member: the MC-E cases and the
   reference model (`ref.py`, `approval_admit`'s `judged`) write exactly these two, and the
   lifecycle harness faults a member too many (`a_recorded_quorum_is_compared`).
2. **When it is written: exactly on a grant check 7 judged.** That is a grant whose result is
   `admitted` or `counted`, and a grant refused at check 7 itself, `duplicate_approver` or
   `not_independent`. A skip (checks 1 to 5 only, PX-7) and a grant refused at checks 1 to 6
   carry no `quorum` member at all, not a null one: the record states the requirement that was
   applied, never one that would have been, and journal spec §4 keeps null and absent distinct.
   The reference model and the generated cases agree (`quorum` appears on every admitted grant of
   MC-E01, MC-E06, MC-E17, MC-E19 to MC-E24 and MC-E29, and on no skip or earlier refusal), and
   the harness faults a member the runtime writes that a case does not state.
3. **What it reads.** The value is `mandate_approval::quorum` of the bound requirement the fold
   holds for the request (`approvers_required`, `independent_required`, read back from the
   journaled `ApprovalRequested`) and the policy overlay admission read in the same step, the same
   call `admit`'s check 7 makes (DEC-257 item 1). The overlay is `PolicyOverlay::NONE` until the
   runtime folds `PolicyChanged` (DEC-278 item 3), so today the member is the bound requirement;
   when the overlay folds, the member tightens with check 7 and nothing here changes. The fold
   reads nothing from the member: it is a record, not state, and a restart rebuilds it from the
   request alone.
4. **No schema changes.** `mandate-journal` registers no closed payload schema for
   `ApprovalResponded`; §9's catalogue row names the member and closing the approval events'
   schemas is the backlog row DEC-257 item 17 and DEC-278 item 13 carry. When that schema closes it
   takes item 1's shape.

### Rationale

Journal spec §9 requires the member "for a grant that reaches check 7", DEC-317 item 7 found the
runtime does not write it, and ten reference cases fail on it alone. The reading only names what
"reaches check 7" means in the runtime's own vocabulary (the four results check 7 can give), and
takes the value from the one function that already computes it.

### Alternatives considered

- **`quorum: null` on every other response.** Rejected: the reference model omits the key, the
  harness would fault the null as a member the case does not state, and a null requirement reads as
  a requirement of nothing.
- **Carrying the quorum inside `mandate_approval::Admission`.** Rejected: an API change to a
  layer-1 crate, with its own DEC-77 sequence, for a value the runtime computes with the same call;
  the predicate "check 7 judged it" is then stated once in the runtime, beside its refusal table.
- **Writing it for every grant that passed check 6, refused at 7 or not.** That is this decision:
  a grant refused `duplicate_approver` or `not_independent` was judged by check 7, so the record
  says what it applied.

<a id="dec-489"></a>

## DEC-489: E8-3's `quorum` tests ship pending with no stub, as behaviour-only rows of the pending gate

| | |
|---|---|
| **Status** | Accepted (item 4 amended by DEC-499 item 3) |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The stream M7 builder (agent), under DEC-79: a process reading of DEC-77 and DEC-137 for one change, which the coordinator may overturn before the implementation lands |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-488](earlier-303-599.md#dec-488); DEC-77, DEC-110, DEC-137, DEC-140's addendum (the precedent) |

### Decision

1. **The tests PR carries no runtime stub.** `ApprovalResponded.quorum` is a member appended to a
   record the live grant path already writes, and that path is exercised by live tests in
   `mandate-runtime` (`a_timely_grant_acts_with_the_bound_order` and the other grant cases) and by
   the MC-E lifecycle harness. Every stub shape fails one of them: a stub that errs
   (`RuntimeError::Unimplemented`) refuses every admitted grant, so the live grant tests break and
   cannot be weakened (`AGENTS.md`, "Do not"); a stub that writes a loud value writes stub text
   into a journaled record and makes the runtime write a member the cases do not state, so the
   harness's three tests that strike `quorum` from the ten pending cases (item 4) fail; and a stub
   that writes nothing is the code as it stands.
2. **The six pending tests are named in `xtask`'s `BEHAVIOUR_ONLY_TESTS`** (DEC-137's exception,
   the precedent being E7-4 slice 5's rows under the coordinator's ruling (d) on #174 and E2-14's
   tracer row): `an_admitted_grant_records_the_quorum_check_7_applied_and_no_other_response_does`,
   `the_quorum_is_rebuilt_from_the_journal_on_restart`,
   `a_grant_counted_short_of_two_approvers_records_the_requirement_it_was_counted_against` and
   `an_author_s_grant_under_independence_is_refused_with_the_quorum_that_excluded_it` in
   `crates/mandate-runtime/tests/approvals.rs`,
   `the_quorum_is_recorded_exactly_when_check_7_judged_a_grant` in
   `crates/mandate-runtime/tests/approval_properties.rs`, and
   `the_quorum_cases_pass_whole_and_their_quorum_is_compared` in
   `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs`, which runs the ten quorum cases
   whole and shows the harness compares the member (each leaf edited fails naming `quorum`, the
   member dropped fails as one the case does not state). Each runs and fails on the member's
   absence (`left: None` in the runtime's tests, "the runtime records no quorum" in the harness),
   DEC-110's rule holds for each, and none can pass on the code as it stands, which is the
   do-nothing implementation: 0 of 6 pass. They land in two tests PRs under ES-13's 400-line
   limit, each adding its own rows: the first carries the one-approver cases, the property and the
   harness test; the second the two stricter-binding cases with the `two_approvers` and
   `rebound_shell` fixtures, which rewrite the journaled request's `approvers_required` and
   `independent_required` and replay it, since the runtime binds one approver without independence
   itself (DEC-278 item 3).
3. **The implementation PR deletes the six rows with the six `#[ignore]` lines.** The gate reports
   a row whose test no longer pends as stale, so the exception expires with the story (DEC-137).
4. **The implementation PR also moves the lifecycle harness's snapshot of "as the runtime stands".**
   Three live tests in `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs` state the
   runtime's current answer and must flip with it: `the_lifecycle_cases_split_as_the_runtime_stands`
   (the ten quorum cases fail naming "the runtime records no quorum"),
   `the_quorum_cases_fail_on_the_quorum_alone` (with `quorum` struck, each passes) and
   `every_expected_member_of_a_lifecycle_case_is_compared` (reads the quorum cases with `quorum`
   struck). Once the runtime writes the member, a struck `quorum` is a member the case does not
   state, so each of the three would fail or check nothing. The implementation PR therefore edits
   that file beyond deleting `#[ignore]` lines: the ten ids join `PASSING`, `QUORUM`,
   `without_quorum` and `the_quorum_cases_fail_on_the_quorum_alone` go, and
   `every_expected_member_of_a_lifecycle_case_is_compared` reads the fixture as written. This
   decision is the ruling DEC-77's amendment asks for: it names the `tests/` file and says why, and
   the precedent is #550, which moved MC-E18 into `PASSING` beside the harness code that made it
   pass. No other test file changes beyond its `#[ignore]` lines.

### Rationale

DEC-77's shape, a stub whose report every pending test stops at, assumes an entry point the live
code does not yet reach. Here the entry point is reached and must keep working. The gate already
has the exception for exactly this case, named row by row so it cannot spread, and the tests still
prove what they must: each fails now, and each is shown to fail on the planted bugs the tests PR
reports.

### Alternatives considered

- **A loud stub value in the journaled member.** Rejected: stub text in a durable record between
  the two PRs, and the harness's three snapshot tests to edit twice.
- **Marking the live grant tests pending again.** Rejected: weakening tests to make a change pass.
- **Writing the implementation in the tests PR.** Rejected: DEC-77 keeps tests and implementation
  in separate PRs for a safety-critical crate.

<a id="dec-493"></a>

## DEC-493: The per-PR `reference` job checks the journal generator and the mutation anchors

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | An agent (Claude Code), under DEC-79: an engineering decision about CI |
| **Story or claim** | E6-13 follow-ups (#443 round 3; #476 round 1, m4); claim [#646](https://github.com/kunwarshivam/mandate/issues/646); PR [#647](https://github.com/kunwarshivam/mandate/pull/647) |

### Decision

1. `cargo xtask ci reference`, the per-PR job in `full`, runs `reference/journal/generate.py --check`
   after the mandate reference's checks. The journal generator runs in the same pinned environment
   as the mandate reference, `reference/mandate/requirements.txt`, which pins the PyYAML and
   jsonschema it needs; `reference/` has one environment, and a second requirements file is not
   added. The pytest `python/mandate_tools/tests/test_journal_agent_vectors.py`, which runs the same
   `--check` from the `python/` workspace in `fast`, stays: `fast` and `full` run in parallel, so
   the nine seconds cost no wall clock, and the two checks read the generator through two
   environments.
2. The job also runs an anchor-only check of `reference/mandate/mutants.py`:
   `python -m mandate_tools.mutation_anchors`, a module of the `python/` workspace, imports
   `mutants.py` without running it, takes every module-level dict whose values are `(old, new)`
   string pairs as a mutant table, and fails naming each anchor whose `old` text `ref.py` no
   longer holds. It refuses a `mutants.py` in which it finds no table. The nightly's full sweep is
   unchanged.
3. The check lives in `python/mandate_tools/`, not as a flag of `mutants.py`, because `reference/`
   is a protected path (ES-22): a change there may not ship with the `xtask` code that calls it,
   and it needs a DEC of its own. Reading the tables by shape rather than by name means a table
   added to `mutants.py` later is covered without a change here.
4. Running ruff over `reference/` (#443 round 1, m4) stays owed. The lint job runs ruff over the
   `python/` workspace only, and `reference/mandate/mutants.py` would fail `ruff format --check`
   as written, so that follow-up is a `reference/` change under its own DEC.

### Rationale

A stale anchor passed `full` on #443's round-2 head because only the weekly nightly runs
`mutants.py`, whose first assertion is the anchor check; the sweep behind it takes most of an hour,
so it cannot move into a per-PR job under the ten-minute budget (DEC-464, superseded by DEC-473, which keeps it). The anchor check alone runs
in well under a second. The journal generator's `--check` takes nine seconds and carries its
self-test, validators, independent recomputation and seeded bugs, so a reference PR that edits
`journal.yaml` by hand, or breaks a validator, fails `full` where the reference checks are read.

### Alternatives considered

- A `--anchors` flag in `mutants.py`: the right place in the long run, but a `reference/` change
  cannot ship with the `xtask` change (ES-22), and the Python workspace lints and tests the module
  where `reference/` is frozen and unlinted.
- The anchor check as a Python snippet embedded in `xtask/src/main.rs`: no lint, no test, and
  Python in a Rust string literal is not a path the next agent should copy.
- Running the full `mutants.py` per PR: rejected by the ten-minute budget (DEC-464, kept by DEC-473).
- Dropping the pytest that already runs `generate.py --check` in `fast`: rejected; tests never get
  weaker to make a change pass, and `fast` fails faster than `full` on a hand-edited vector.

<a id="dec-496"></a>

## DEC-496: The `Draft::schema_version -> 1` mutation exclusion is withdrawn

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | An agent (Claude Code), under DEC-79: a reversible engineering decision that only tightens the mutation gate (a pure tightening, by DEC-176's test), from the independent review of [#644](https://github.com/kunwarshivam/mandate/pull/644) (finding 2) |
| **Story or claim** | E7-19 (claim #171), the merge coordinator's follow-up to #644 |

### Decision

1. `.cargo/mutants.toml` no longer excludes `replace Draft::schema_version -> u64 with 1`. Its
   recorded reason, "every registered payload schema is version 1 … until the first version 2
   schema exists", stopped being true when `OrderSubmitted` version 2 was registered
   (`crates/mandate-journal/src/control.rs`), and #644 makes that method the sole switch for every
   version-2 configuration check (`Draft::config_artifact_path`, `validate_config_artifacts`).
   Planted by hand on `main` (196d36b), the mutant fails one live test,
   `control::tests::each_rule_45_batch_commits_or_is_refused_at_its_draft_and_member`, so it is
   not equivalent today; on #644's head it fails four.
2. The exclusion list is empty. A future exclusion needs the founder's approval and a reason no
   test can tell the mutant apart from the real code (ADR-0001 ES-11).
3. ADR-0001 ES-11 records that no exclusion is approved. The per-PR gate's test selection, which
   #644 changes, is recorded by that PR's own decision, not here.

### Rationale

The gate runs on the diff, so the exclusion cost nothing while `schema_version` was untouched and
would have silently exempted the next edit to it from the one mutant that disables all of #644's
checks. Withdrawing an exclusion only adds mutants the gate must catch; it weakens nothing.

### Alternatives considered

- Keep the exclusion with a corrected reason: rejected, the mutant is provably caught, so no
  reason holds.
- Delete it inside #644: rejected only for ownership; `.cargo/` is founder-owned (CODEOWNERS), so
  the change ships alone where the founder can revert it without touching the journal code.

<a id="dec-497"></a>

## DEC-497: a mutation run executes the reference suites whenever it mutates a crate they are built on

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The merge coordinator (agent), under DEC-79: a reversible engineering decision about the gate's own reach and cost, which the founder may overturn |
| **Story or claim** | E7-19 follow-up to [#644](https://github.com/kunwarshivam/mandate/pull/644), round 2 finding 1; [#652](https://github.com/kunwarshivam/mandate/pull/652) review rounds 1 to 4; ES-11's mutation gate, DEC-137, DEC-139, DEC-253, DEC-464's ten-minute budget |

### Decision

1. **A mutation run that mutates any crate `mandate-refcases` is built on also runs
   `mandate-refcases`.** `cargo xtask ci mutants` adds `--test-package=mandate-refcases` to the
   mutated packages, so every mutant is judged by the whole reference harness as well as by its own
   package's tests.
2. **The reach comes from cargo's dependency graph, not from a list.** `workspace_closure` in
   `xtask/src/main.rs` computes each package's transitive workspace dependencies from
   `cargo metadata`; the run asks whether any mutated package is in `mandate-refcases`'s closure.
   Nothing has to be written down, so nothing can be written down wrongly.
3. **The whole package runs; no nextest filter narrows it.** A suite that exists is a suite that
   judges, including one the change has just added.
4. **A mutation of a package the suites are not built on costs nothing.** The run is exactly what it
   was before this decision.
5. **The closure is the suites' exact reach, not an estimate of it.** A test can only exercise code
   its package links, and `mandate-refcases` links every crate it drives in-process: it declares no
   dev-dependencies and spawns no binary. So there is no crate a suite can fail on that
   `cargo metadata` cannot see. `no_reference_suite_reaches_a_crate_by_spawning_it` keeps the
   second half of that true, refusing a `Command::new` or a `CARGO_BIN_EXE` in the reference
   package, which would reach a crate the graph does not name.
6. **The rule is `external_oracles` in `xtask/src/main.rs`,** a pure function with a unit test,
   `the_reference_package_runs_whenever_a_crate_it_is_built_on_is_mutated`, and a test over the real
   dependency graph, `the_reference_suites_judge_every_crate_they_are_built_on`, which names the
   fourteen crates the suites exercise, `mandate-canon` and `mandate-spec` included.
7. **No per-suite declaration table.** `crates/mandate-refcases/oracles.toml` and the
   `cargo xtask oracles` check that held it to the suites are both gone. Their history is below,
   because the reason they are gone is the whole content of this decision.

### Rationale

A mutant is judged only by the tests cargo-mutants runs, and cargo-mutants runs the tests of the
mutated package. Several crates' oracles are not there: they live in `mandate-refcases`, a package
the mutated crate does not depend on, so nothing discovers them. E7-19 showed the hole directly —
`validate_config_artifacts` replaced by `Ok(())` was caught only by two `production_config_refs`
cases, and the gate as it stood never ran them.

The first fix keyed the choice on the diff: run a suite when the change edits it. #652's review
refused it, rightly. DEC-77 lands the tests and the implementation of a story in two PRs, so an
implementation PR legitimately changes only `crates/mandate-journal/src` and edits no suite; under
the diff rule its mutants were judged without the one oracle that could catch them, which is the
DEC-79 zero-missed-mutants assurance quietly not holding on a safety-critical path.

The next three shapes all tried to run *some* of the suites, selected by a per-suite table naming
the crates each one drives. Each shape was refused by the next review round, and always for the
same reason, found in a different place:

- Round 2: `agent_stream` and `asset_id` call `Draft::parse`, which validates through
  `mandate_time::UtcNanos::parse`; neither entry named `mandate-time`, and `agent_stream` generates
  malformed timestamps on purpose, so a `mandate-time` mutant escaped the suites written to catch
  it. Fixed by having an entry name only what a suite calls and letting cargo's graph supply the
  transitive reach.
- Round 3: `mandate_harness` interprets every reference-case kind, so it drives six crates its
  entry did not name. Fixed by widening that entry.
- Round 4: `harness` and `trading_domain_gate_harness` parse decimals through
  `mandate_canon::DecStr`, and `mandate_gate_harness` validates through `mandate-spec`; three more
  entries under-named.

Three rounds, three findings, each one a crate a suite genuinely exercises and its declaration
genuinely omitted. The entries were not careless; the rule was. Asking anyone, human or agent, to
keep a per-suite list of exercised crates true is an audit that is wrong the day it is written and
wronger every month after, and — this is what makes it unacceptable here — its failures are
silent. An omitted crate does not fail CI. It reports a mutant caught that no oracle ran.

So the selection is gone. The suites run whole, whenever the run mutates anything they are built
on, and the question "does this entry name every crate the suite exercises?" cannot be asked
because there is no entry.

This is sound rather than merely conservative. The three failed rounds each compared a written
claim against what the code does, and lost; the dependency closure is not a claim about the
suites, it is a property of them. A test exercises exactly the code its package links, and
`mandate-refcases` links everything it drives: fourteen workspace crates, all normal dependencies,
no dev-dependencies, and no subprocess anywhere in the package. Under those two facts the closure
and the reach are the same set, which is why decision item 5 pins the second of them with a test
rather than leaving it to hold by habit.

What that costs is less than the three refused rounds assumed. Measured at `d8b99df` on four cores
with two test threads, the whole package is 62 seconds, and three suites are 44 of them
(`mandate_harness` 19.7s, `mandate_lifecycle_harness` 13.5s, `refcases` 10.7s); the other nine
together are about 18. But `mandate_harness` and `refcases` read the whole approved corpus, so any
honest declaration for them names most of the core — which is exactly what round 3 found. A table
that is correct therefore selects the expensive suites for nearly every change, and the saving it
buys over running everything is roughly the 14 seconds of `mandate_lifecycle_harness`, a fifth of
the package. A fifth is not worth a rule whose errors are silent and which three consecutive
reviews caught in a different place.

The worst case is barely moved by this simplification: a correct table selected every suite for a
layer-0 mutation anyway. What it costs on the runner, where the harness takes 63 to 83 seconds a
mutant rather than the 32 it takes here, is DEC-498's, measured there rather than scaled from
here.

### Alternatives considered

- **Key the suites on the change's diff** (the first shape). Cheap and wrong: an implementation PR
  that edits no suite runs no external oracle, which DEC-77 makes an ordinary case.
- **A per-suite declaration table, with or without a transitive closure under it** (the second,
  third and fourth shapes, refused by #652's review rounds 2, 3 and 4 respectively). Covered above:
  it saves about 14 seconds a mutant and buys a silent failure mode that three reviews each
  demonstrated.
- **Derive the mapping from the suites' source, by the crate names each file mentions.** Automatic,
  but unsound in the direction that matters: a suite that reaches a crate only through the
  harness's own code mentions it nowhere.
- **Make the mutated crates depend on `mandate-refcases` so cargo-mutants finds the oracles.**
  Refused: it inverts the layering (`xtask/layers.toml` puts the harness above the core) and would
  put a test harness in the dependency graph of shipped crates.
- **Move the oracles into each crate's own tests.** The reference cases are one approved corpus read
  by one harness (DEC-253); splitting it per crate would duplicate the corpus and lose the single
  place that proves every case is covered.
- **Make the expensive suites cheaper.** Worth doing on its own merits, and it would shrink the
  worst case for everyone rather than for the changes a table happens to classify as cheap. It
  belongs in the backlog, not in a PR about configuration references.

<a id="dec-498"></a>

## DEC-498: one mutant per shard, a hundred and ninety-two shards, a cap on every phase cargo-mutants will cap, and a named remainder under them

| | |
|---|---|
| **Status** | Accepted; item 6's remainder restated, and the mutants job's database and build profile added, by [DEC-519](earlier-303-599.md#dec-519); item 1's matrix sized to the diff, `min(192, n)` shards with the same slice each, by [DEC-538](earlier-303-599.md#dec-538) |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The merge coordinator (agent), under DEC-79: a reversible scheduling decision of the same kind DEC-464 and DEC-473 made, which the founder may overturn |
| **Story or claim** | [#652](https://github.com/kunwarshivam/mandate/pull/652); supersedes DEC-473's shard count; DEC-464's ten-minute budget, DEC-497; measured by [#657](https://github.com/kunwarshivam/mandate/pull/657) |

### Decision

1. Supersede DEC-473's twenty-four mutation shards with **a hundred and ninety-two** deterministic,
   zero-based slice shards, sized so that the widest safety-critical diff measured so far
   (E6-13's 169 mutants) puts **at most one mutant in a shard**.
2. **Run one mutant at a time** (`MUTANT_JOBS` in `xtask/src/main.rs`, `cargo mutants --jobs 1`).
   Two workers contend, and with item 1 a CI shard has nothing to parallelise anyway; the setting
   still governs the unsharded local run.
3. **Set cargo-mutants' per-mutant test timeout explicitly, to 180 seconds**
   (`MUTANT_TEST_TIMEOUT`), instead of taking the value cargo-mutants derives. `--timeout` also
   caps the unmutated baseline's test phase, so this one number bounds both.
4. **Cap a mutant's build too, at 60 seconds** (`MUTANT_BUILD_TIMEOUT`, `--build-timeout`), which
   cargo-mutants leaves off by default. A mutated operator in a top-level `const` can size an
   array, so a mutant can lengthen a build and not only a test run. Unlike `--timeout`, this one
   spares the baseline, so a cold cache is not cut off.
5. **Budget a shard on what each phase is permitted to cost, not on how fast phases were observed
   to run.** One mutant costs at most 240 seconds and the baseline's tests at most 180, so 420 of
   the ten minutes are bounded by configuration whatever the diff does. Items 1 to 4 are chosen
   together for that sum; none may be changed without redoing it.
6. **Name the remainder rather than claiming to bound it.** Job overhead, the live-test preflight,
   mutant enumeration, and the baseline's *build* are what no cap of ours reaches. A populated
   shard pays all of it whatever slice it draws, so sharding cannot reduce it. Item 5 leaves it
   180 seconds against about 40 measured on a one-package diff, and that reserve is the extent
   of the claim.
7. Keep everything DEC-473 kept: the ten-minute job limit, the complete source diff, the selected
   packages, the live-test preflight, the unmutated baseline, the test command, and the
   zero-survivor rule. `cargo xtask check` stays unsharded as the complete local oracle.
8. **When a shard runs out of time on its mutant, expand the matrix; never the job's ten-minute
   limit, the diff, or the tests.** When the remainder is what overran, item 6 applies instead,
   and the matrix is not the answer: every populated shard pays the same remainder.
9. **Every term of a shard budget is either capped by configuration, or shown by command to be
   beyond a mutant's reach, or named as diff-dependent with its reserve and its failure mode
   stated.** Eight drafts of this decision priced a term at a measurement, asserted a phase was
   out of a mutant's reach, or generalised one diff's numbers to every diff; a reviewer refuted
   each with a single command. So: a term nobody has measured at its worst is recorded as
   unmeasured, a remedy nobody has checked is not named at all, and a claim with "never" or
   "always" in it is a command to run.

### Rationale

DEC-497 runs the whole reference harness for any mutation of a crate it is built on, so a mutant
of a core crate costs its own package's tests plus the whole of `mandate-refcases`. A shard has
four cost terms: a mutant's build, a mutant's tests, the unmutated baseline's tests, and
everything else. The first three are capped by configuration. The fourth is named, not bounded.

#### What the runner measured

[#657](https://github.com/kunwarshivam/mandate/pull/657) is a throwaway branch built to produce
these numbers: three untested `pub fn`s in `mandate-canon`, nine mutants, none caught, all in a
single shard. An uncaught mutant runs the whole harness to completion, so it measures the
harness's natural pace. The same nine ran four times, twice at one worker and twice at two, with
the timeout lifted so nothing was cut off. Ranges cover all nine mutants in each run.

| Workers | Run | Test phase | Build | Wall clock per mutant | Nine mutants |
|---|---|---|---|---|---|
| 1 | a | 77–83s | 16–27s | 99s | 14m49s |
| 1 | b | 63–66s | 16–25s | 82s | 12m17s |
| 2 | a | 61–139s | 21–55s | 83s | 12m29s |
| 2 | b | 90–181s | 24–71s | 108s | 16m15s |

**Why one worker (item 2).** Two runs each support no claim about throughput, and the gap between
the settings (82 and 99 seconds a mutant at one, 83 and 108 at two) is smaller than the gap
between repeated runs of either. What the samples do show, well outside that spread, is that two
workers contend: a test phase runs to 181 seconds at two workers and to 83 at one. One worker
keeps a single mutant short, and makes a shard's duration the sum of its mutants rather than a
sequence of waves.

**Why 180 seconds (item 3).** The timeout has to sit above every legitimate harness run, or it
turns a judged mutant into a `TIMEOUT` and fails a green PR. The slowest legitimate test phase at
one worker is 83 seconds, and repeated runs of the same configuration differ by about a quarter,
so 180 is 2.2 times the slowest observation. It is not a claim that only hangs reach the cap: a
mutation that makes the code slow without hanging can reach it too, and item 5 is what makes that
affordable.

Item 3 also closes a live failure. Left derived, cargo-mutants sets the per-mutant timeout to five
times the baseline's test phase, floored at twenty seconds — and it measures that baseline over
the slice's own mutated packages, so a shard whose baseline is a one-second crate gives every
mutant twenty seconds and `TIMEOUT`s each one that reaches the harness. That is loud rather than
silent (`cargo mutants` exits 3, which `mutants_outcome` refuses), but it would redden every
honest change whose mutants the harness catches slowly.

#### Which cap reaches which phase

The two caps divide asymmetrically, and the division is read off the tool, not guessed.

**`--timeout` covers every test phase, the baseline's included.** Rerun a shard with `--timeout 5`
and the baseline is killed mid-suite and the whole run refused:

```
$ cargo mutants --in-diff … --timeout 5 --build-timeout 60 --shard 12/96 --sharding slice
SIGTERM [0.118s] (46/691) mandate-refcases mandate::research::tests::every_input_object_rejects_…
*** result: Timeout
ERROR cargo test failed in an unmutated tree, so no mutants were tested
```

**`--build-timeout` covers every build phase *except* the baseline's.** With `--build-timeout 3`,
a twelve-second baseline build completed and was reported `ok` while every mutant build was cut
off at three seconds, so a cold cache compiling the workspace from scratch is never cut off.

A build cap is needed at all because a mutant can lengthen a build. Operators are mutated wherever
they appear, top-level `const` items included:

```
$ cargo mutants --list      # on `pub const N: usize = 1024 % 7;  pub static T: [u8; N] = ...`
src/lib.rs:1:27: replace % with /
src/lib.rs:1:27: replace % with +
```

`1024 % 7` is 2 and `1024 + 7` is 1031, so the array `N` sizes grows five hundredfold — codegen
and link work, which the `long_running_const_eval` lint does not catch. Sixty seconds stands
against the 16 to 27 seconds mutant builds took at one worker.

**The baseline's packages come from the shard's own mutants, not from `--test-package`.** Two runs
of the same gate, differing only in the slice they draw:

```
shard 0, mutant in mandate-journal
  baseline:  cargo nextest run --package=mandate-journal@0.0.0
             Starting 106 tests across 8 binaries     Summary [ 1.364s]   9s build
shard 12, mutant in mandate-refcases
  baseline:  cargo nextest run --package=mandate-refcases@0.0.0
             Starting 691 tests across 13 binaries    Summary [31.961s]  19s build
```

Both passed `--test-package=mandate-journal --test-package=mandate-refcases`. So a shard whose
mutant lives in `mandate-refcases` pays the whole reference harness before it — and E6-13's
169-mutant diff, which this matrix is sized on, mutates `mandate-refcases` among others. DEC-473
measured exactly that: "the unmutated baselines consumed about one to two minutes". Item 3's cap
holds that test phase down; the build under it is not capped.

#### The remainder

Call the uncapped part the **remainder**: job overhead, the live-test preflight (`cargo nextest
list` per mutated package, DEC-139), cargo-mutants' enumeration of the complete mutant list before
`--shard` selects from it, and the baseline's build.

- **It grows with the diff, and this decision does not bound it.** Unmutated means *this PR's*
  source, so the baseline builds exactly the packages the diff touches. What a maximally wide diff
  costs is an open question; nobody has measured one.
- **Sharding cannot reduce it.** Every *populated* shard pays it in full. An empty slice is the
  exception: cargo-mutants returns before the baseline, which is why a shard with no mutant costs
  10 seconds and not 40.
- **Measured on `ubuntu-24.04`, on this decision's own matrix and its one-package diff**: 24
  seconds of job overhead, and a `cargo xtask ci mutants` step of 54 seconds, of which 39 were its
  mutant and 1 the baseline's tests. **About 40 altogether.** The 14 seconds left in that step is
  a residual, not a partition: it holds the preflight, enumeration, a 9-second baseline build, and
  cargo-mutants' own startup and tree copy, which nobody has separated. The widest measurement
  anyone has is DEC-473's, and it does not separate the baseline's build from its tests either.

#### The budget

```
k × (60 + 180) + 180 ≤ 600 − remainder      so k = 1 leaves the remainder 180 seconds
```

The second term is the baseline's capped test phase; `k` is the mutants in the slice. At `k = 2`
the capped phases alone come to 660 and overrun the limit before the remainder is counted at all,
which is why **one mutant per shard** is item 1 rather than a nicety.

`--sharding slice` gives shard `i` the half-open range `[i × c, (i+1) × c)` for `c = ceil(N / S)`:
early slices are full, one may be partial, and trailing slices are empty, which is not an even
spread. A slice holds at most `c`, so one mutant a shard needs `S ≥ N`. The widest
safety-critical diff measured so far is E6-13's 169, so **a hundred and ninety-two** — clear of
169 and inside GitHub's matrix ceiling of 256.

In normal operation a shard is far cheaper: a caught mutant stops at nextest's first failure, so
#652's own shards, one caught mutant each, finished in 72 to 88 seconds end to end, and shards
with no mutant finished in 44. On a public repository GitHub's standard runners cost nothing, so
eight times DEC-473's job count costs concurrency and not money. The first run of this matrix took
28 to 82 seconds a shard and 7,147 seconds of runner time, while the whole `ci` workflow finished
in **7 minutes 18 seconds**: about sixteen shards at a time.

It does multiply one thing that is not free. Every shard downloads cargo-mutants from GitHub's
release assets, and a transient failure reddens a required check that DEC-464 forbids retrying
automatically — the run on this decision's own branch lost five shards to `curl: (22) The
requested URL returned error: 500` within the same second. So every checksum-verified download in
`ci.yml` now passes `--retry 5 --retry-delay 2 --retry-all-errors`. The checksum check still
decides what is installed; the retry only decides how many times a transient 500 wastes a job.

#### When the budget is wrong

Whatever runs a shard out of time fails loudly: the shard hits GitHub's hard timeout, the job
fails, the `full` aggregate fails, and the PR cannot merge. No mutant is ever reported caught
because its shard ran out of time. Being wrong here costs a red check, never a gap in the gate.

The guarantee is narrower than its converse, and worth stating exactly: up to 192 mutants, **the
three capped phases cannot exceed 420 of a shard's 600 seconds, however badly its mutant
behaves**. Whether the shard survives still depends on the remainder fitting the other 180, and
item 6 is the admission that nothing here guarantees that. Too many mutants in a slice is answered
by item 8; a remainder past its reserve has no answer here, because more shards each paying the
same remainder changes nothing. That case needs its own decision, taken against a measurement of
the diff that caused it.

#### A disagreement this surfaced, not resolved here

[ADR-0001](../../adr/0001-engineering-setup.md) ES-12 says "every required check and every job it
aggregates has a hard timeout of ten minutes or less", and DEC-464 made that the enforceable gate.
But `ci.yml`'s `fast` job, one of the two required checks, carries `timeout-minutes: 15`.

Neither reading is taken here. Lowering `fast` to ten minutes tightens the rule but is not
risk-free — the job runs near seven minutes today, so the cap could redden honest changes — and
raising the ADR to fifteen weakens a stated gate, which DEC-79 reserves to the founder. DEC-176
lets an agent settle only a reading that adds no risk, and neither of these does. Recorded for
the founder; this decision's own jobs (`mutants`, `full-checks`) are at ten.

### Alternatives considered

- **Raise the ten-minute job limit.** Refused for DEC-473's reason: it weakens the latency gate
  DEC-464 made enforceable, and a limit tuned to today's suite is a number that rots.
- **Size the matrix on the measured pace rather than on the timeout.** `k ≤ 4` and 48 shards
  follow from pricing a mutant at the 107 seconds the harness took when nothing went wrong. But
  the timeout permits far more, so that matrix held its mutants only while every one behaved. A
  capacity conditional on the thing being measured is not a capacity.
- **Leave `--build-timeout` at its default of none**, on the argument that every mutation genre
  edits an expression inside a function and so changes run-time behaviour rather than the
  compiler's work. The `const` counterexample above refutes it.
- **Keep two workers and bound the critical path by waves.** `ceil(k / 2)` waves, each as long as
  its slowest mutant, is the correct arithmetic for two workers. But two workers double a mutant's
  latency for no throughput these measurements can see, so the right fix is to remove the waves.
- **Keep ninety-six shards and lower the test timeout to fit two mutants.** Two mutants and a
  capped baseline need `2 × (60 + T) + T ≤ 600 − remainder`; at a remainder of 50 that is
  `T ≤ 143`, 1.7 times the slowest legitimate test phase against a run-to-run spread of about a
  quarter. That buys the margin from the one term the gate must never be stingy with: `T` is what
  stands between a slow honest mutant and a `TIMEOUT` that reports nothing. The jobs are free.
- **Call the remainder beyond a mutation's reach, since the baseline is unmutated.** Unmutated
  means this PR's source, so the baseline's cost is the diff's cost, and the preflight and
  enumeration ahead of it scale with the diff too.
- **Leave the per-mutant timeout derived, and run the reference package in the baseline too.** It
  would make the derived number honest, but it charges every shard a full harness pass before its
  first mutant, including shards whose mutants are nowhere near the harness, and it puts the
  per-mutant term back on a measurement.
- **Narrow the external oracles so a core mutation does not run every suite.** Tried in three
  shapes and withdrawn in DEC-497: it saves little and buys a failure mode that reports a mutant
  caught when no oracle ran.
- **Make the reference harness cheaper.** Filed as E1-6, and worth doing: it is what puts a core
  crate's mutant test phase at 83 seconds, so it is the lever on item 3's cap and, on a refcases
  shard, on the baseline's capped test phase. It is a lever on capped phases, not on the
  remainder. Either way it does not belong in a PR about configuration references.
- **Larger runners or an external CI service.** Refused for DEC-473's reason: it introduces
  spending or a new service, which DEC-79 reserves to the founder, when the native matrix holds.

<a id="dec-499"></a>

## DEC-499: E8-3's implementation PR also stops two lifecycle-harness helpers striking `quorum`, and keeps `QUORUM`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The stream M7 builder (agent), under DEC-79 and DEC-176: the two tests compare one member more than they do today, and nothing they assert is dropped |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); extends [DEC-489](earlier-303-599.md#dec-489) item 4; [DEC-488](earlier-303-599.md#dec-488); DEC-77 |

### Decision

1. **DEC-489 item 4 missed two helpers.** Besides the three "as the runtime stands" tests it names,
   `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs` strikes `quorum` from an expected
   draft in two helpers that live tests use: `retailed` (MC-E06, for
   `a_retailed_source_is_the_same_event`) and `granted_with` (MC-E01, for
   `now_reaches_the_runtime_through_its_ports`). Each says the quorum "is all it misses today".
   Once the runtime writes the member (DEC-488), the struck draft lacks a member the runtime writes,
   and both tests fail with "the runtime wrote `quorum`, which the case does not state". Run
   against the implementation, the harness fails exactly these five tests: DEC-489 item 4's three
   and these two.
2. **The implementation PR edits those two helpers too:** `retailed` and `granted_with` keep the
   fixture's `quorum` as written, and their doc comments drop "with its quorum struck". If nothing
   is left for `retailed` to do, it goes, and its test reads the fixture as written. Nothing these
   tests assert changes: every other edit they make to MC-E06 and MC-E01 stays, and each then also
   compares `quorum`.
3. **`QUORUM` stays.** DEC-489 item 4 says "`QUORUM`, `without_quorum` and
   `the_quorum_cases_fail_on_the_quorum_alone` go", but
   `the_quorum_cases_pass_whole_and_their_quorum_is_compared` iterates `QUORUM`, and as a pending
   test its body may change only by losing its `#[ignore]` line. So the constant stays, with its
   doc comment no longer saying the runtime does not record the member. What goes is
   `without_quorum` and `the_quorum_cases_fail_on_the_quorum_alone`.
4. **No other test file changes** beyond its `#[ignore]` lines, DEC-489 items 3 and 4, and items 2
   and 3 here.

### Rationale

DEC-77 lets an implementation PR edit a test file beyond its `#[ignore]` lines only under a ruling
that names the file and says why. DEC-489 item 4 is that ruling for the same file, but it lists
three tests and these two helpers are a fourth and fifth instance of the same snapshot of "as the
runtime stands". The change only makes two tests compare more.

### Alternatives considered

- **Leave the helpers and let the two tests fail.** Rejected: a red suite.
- **Mark the two tests pending in tests PR 2.** Rejected: they are live tests of the re-tail and of
  re-validation through the ports, and marking them pending weakens them for a whole PR cycle.
- **Strike the member in the harness's comparison instead.** Rejected: it would stop the harness
  comparing `quorum`, the opposite of DEC-488.

<a id="dec-500"></a>

## DEC-500: CodeQL's `rust/cleartext-logging` query is excluded

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The founder, on the coordinator's recommendation of 2026-10-07 ("lets do this"); recorded by an agent (Claude Code) |
| **Story or claim** | The CodeQL workflow added in [#658](https://github.com/kunwarshivam/mandate/pull/658); found on [#662](https://github.com/kunwarshivam/mandate/pull/662) |

### Decision

1. The "CodeQL Advanced" workflow (`.github/workflows/codeql.yml`) reads
   `.github/codeql/codeql-config.yml`, which excludes one query, `rust/cleartext-logging`
   (CWE-312, security severity 7.5). Every other query in the default Rust suite, and every query
   for the workflow's other languages, still runs.
2. The exclusion is for this query only. A new query that raises alerts is classified on its own
   evidence, never excluded by analogy to this one.
3. The exclusion is paired with a narrow replacement, backlog story E7-20: a repository query under
   `.github/codeql/queries/`, read by the same config, that flags any value returned by
   `expose_secret()` reaching a logging or print sink, proven by a seeded bug it catches. It keys on
   the credential type, not on names, so it raises nothing for accounts or journal hashes.

### Rationale

The query finds data whose *name* looks sensitive flowing into a log or print. Its name
heuristic (CodeQL's shared `SensitiveDataHeuristics`) classifies anything matching `acc(ou)?nt` as
account information and anything matching `trusted` as a secret. In this repository those words
name the domain: the account stream, the account ledger, `append_account`, `account_wide`, and the
journal's `trusted_start`. On `main` at `0b0ffef` (CodeQL 2.27.2 run locally with the
`rust-code-scanning` suite, build mode none) the query raised 110 alerts, all of them false
positives:

- **Sources.** The flows start at names containing `account` or `trusted`
  (`account(...)`, `account_event(...)`, `account_fields(...)`, `account_restriction(...)`,
  `account_stream_checks(...)`, `trusted_start(...)`, `append_account(...)`, `account_wide(...)` and
  their kin), and pass through values derived from them (`checked_sub`, `fees_total`,
  `buying_power`). The one source named `secret` is the loop variable of
  `crates/mandate-shell/tests/tracer.rs`'s check that PII sentinels never reach the journal.
- **Sinks.** Every sink but one is an assertion or print message in test code (`tests/`, or a
  `#[cfg(test)]` module in `src/`, such as `crates/mandate-shell/src/stages/fail_closed.rs`, which
  `stages.rs` declares under `#[cfg(test)]`). The one other is the tracer binary's
  `println!("submitted {id}")`, which prints an opaque client order id to the operator's terminal.
  AGENTS.md rule 6 allows opaque ids.
- **Nothing logged is a credential, a position, order details or mandate content.** The 18 alerts
  #662 drew were the journal's `prev_hash`, a SHA-256 over public journal events that the
  verification report prints on purpose (DEC-490 item 7).

An always-red check that is always wrong teaches reviewers to ignore the CodeQL check, which hides
the queries that do find something. The workflow gates no merge today: `merge-approved.sh` requires
only `ci.yml`. But every PR touching an account or journal path would carry the red check.

**What this gives up:** CodeQL would no longer flag a real credential written to a log, and
credentials already live outside a vault: the Alpaca connector (`crates/mandate-alpaca/src/http.rs`)
and the market-data client (`crates/mandate-marketdata/src/http.rs`) hold their API keys as
`secrecy::SecretString` and read them with `expose_secret()` only to build a request header marked
sensitive. Until item 3's query lands, that risk is held by:

- AGENTS.md rule 7: credentials never leave the vault and are never logged.
- Rule 6: notification payloads carry only opaque ids.
- `SecretString`'s own `Debug`, which prints `[REDACTED]`, so only an explicit `expose_secret()`
  can reach a log.
- `cargo xtask ci supply-chain` runs gitleaks over the history.
- The independent review checks the safety-critical paths that handle credentials (connectors, the
  vault, authentication).

### Alternatives considered

- **Keep the query and dismiss each alert in the code-scanning UI.** Rejected. Dismissals do not
  carry over to new code, and every new account-path change raises fresh alerts.
- **Model the domain's names as non-sensitive (a models-as-data barrier or a custom heuristic).**
  Rejected. The heuristic is shared and keyed on names, so the model would have to list every
  account-named function and keep up with new ones, and it would still guess at what is secret by
  name.
- **Replace the name heuristic now with a query keyed on the credential type.** Taken as item 3,
  not as this change. A query whose sources are calls to `expose_secret()` and whose sinks are the
  excluded query's own logging sinks (`CleartextLogging::Sink`) is about 40 lines; run locally on
  `main` it took 11 s and found nothing. It ships with a seeded bug that it must catch, so it is its
  own change rather than part of this one.
- **Exclude test paths only (`paths-ignore`).** Rejected. It removes most alerts but not the
  production `println!` or the `src/` test modules, and it also hides every other query's findings
  in tests.
- **Remove the CodeQL workflow.** Rejected. The other queries cost nothing and may find real
  defects.

### Revisit when

E7-20 lands (item 3), or a credential type that is not a `SecretString` appears, which item 3's
query would not see. If CodeQL's shared heuristic stops keying on `account` and `trusted`, the
exclusion can be dropped instead.

<a id="dec-501"></a>

## DEC-501: E8-3's ten quorum cases move to passing, and the lifecycle harness's renamed test is recorded

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | An agent (Claude Code, the merge coordinator), under DEC-79 and DEC-176: it records cases that now pass and names a test edit the independent review of #665 judged a consequence of a ruled one |
| **Story or claim** | E8-3; claim [#213](https://github.com/kunwarshivam/mandate/issues/213); [DEC-488](earlier-303-599.md#dec-488), [DEC-489](earlier-303-599.md#dec-489), [DEC-499](earlier-303-599.md#dec-499); DEC-77, DEC-277, DEC-317 item 7 |

### Decision

1. **The status rows.** In `crates/mandate-refcases/status.toml`, `[mandate]`, these cases move
   from `pending` to `passing`: MC-E01, MC-E06, MC-E17, MC-E19, MC-E20, MC-E21, MC-E22, MC-E23,
   MC-E24 and MC-E29. They waited only on `ApprovalResponded.quorum`, which the runtime writes
   since E8-3's implementation ([#665](https://github.com/kunwarshivam/mandate/pull/665),
   DEC-488). The comment above the family now says so. Under DEC-277 a passing case never goes back
   to pending except with a change to its expected output.
2. **The harness test's rename is ruled.** #665 renamed the lifecycle harness's
   `the_lifecycle_cases_split_as_the_runtime_stands` to `every_lifecycle_case_passes_whole`, and
   rewrote the module doc from "sixteen pass whole, and the rest fail" to "every case passes
   whole". DEC-489 item 4 said that test "must flip"; it did not name a rename. The independent
   review of #665 judged both edits a consequence of the ruled flip. The old name states a split
   the ruled body no longer makes, and AGENTS.md puts the reason in a name, so keeping it would
   leave a false name under a ruled body. Nothing the old test asserted was weakened.
   `the_quorum_cases_pass_whole_and_their_quorum_is_compared` replaces the old failing branch with
   a stronger check: each case is stated once, and each leaf edit fails naming `quorum`. This item
   is the ruling DEC-77 asks for, after the fact.
3. **Owed by the fold story.** Until `PolicyChanged` folds (DEC-257 item 12), `PolicyOverlay::NONE`
   is the only overlay reachable, so no test can tell a `quorum` record built from the overlay
   apart from one built from the request's binding alone. The code passes check 7's own overlay
   (`&context.policy`) to `mandate_approval::quorum`, so the record cannot disagree with the
   decision. The tests PR of the fold story adds a case pinning the record to an overlay-tightened
   quorum (#665 review, minor 2).

### Rationale

The status file is the record of which reference cases the code reproduces. The ten cases pass
with `--include-ignored` on #665's head, and the harness test that runs them whole is live. Item 2
closes the process gap the review named, without reopening a merged PR for a name.

### Alternatives considered

- **Rename the test back in a tests correction.** Rejected. It would restore a name that states
  something false, which is the same defect a review would then raise.
- **Leave the rename unrecorded.** Rejected. DEC-77 lets an implementation PR edit a test only
  under a ruling that names the edit, and the next agent copies what the record allows.

<a id="dec-502"></a>

## DEC-502: The first real paper trade: what it is, and that it comes first

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The founder, in session on 2026-10-07, choosing the coordinator's recommended option on each of four questions; recorded by an agent (Claude Code, the coordinator) |
| **Story or claim** | E7-19, E19-1; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [DEC-450](earlier-303-599.md#dec-450), [DEC-475](earlier-303-599.md#dec-475) |

### Decision

1. **The bar.** The first real paper trade is one order on the founder's Alpaca paper account
   that reaches the broker only through the production cycle API ([DEC-475](earlier-303-599.md#dec-475)), where:
   - the deployment input is built from a **confirmed mandate version**, not a test fixture;
   - the model output comes from a **pinned signal model** (mandate spec §8.1, V-007), run
     outside the production cycle and the shell, and handed to the cycle as its typed
     `ModelOutput`;
   - the Alpaca paper adapter calls that API, and E7-7's AAPL assembly is deleted (E7-19 slice 5);
   - the journal verifies afterwards, and a restart sends no second order.
2. **The model.** The first model is a deterministic quant model, the moving-average crossover
   (`quant.ma_crossover`), pinned by id, version, and content hash of its code and parameter
   schema (mandate spec §8.1). Quant models make no model call and do not go through the model
   gateway ([inference spec §1.3](../../specs/inference.md#13-non-goals)), so the first trade needs
   neither the gateway (E15-6) nor an inference registry entry (E15-7); a small model host runs
   the quant code and supplies its typed output. The founder chose this reading on 2026-10-07,
   after the first draft of this decision routed the quant model through the gateway, which the
   inference spec excludes. The LLM research model follows through the gateway and registry, and
   its output enters the same production cycle API.
3. **The instrument** is BTC/USD ([DEC-450](earlier-303-599.md#dec-450)).
4. **The order of work.** This path goes to the top of the tracker's "Next, in order". E7-4
   slice 7, the agent-scoped kill switch, finishes first because it is nearly done. Tripwires
   (E6-13), M7's remainder, and M4's remainder follow.
5. **Out of the bar.** A supervised agent process with scheduled cycles (E19-1's full process
   lifecycle) is not required for the first trade. The deployment input it would build must
   still come from a confirmed mandate version. The M7 soak needs the full process.
6. **What does not change.** Every safety gate stays: tests first for safety-critical crates
   (DEC-77), an independent review on a different model, zero missed mutants, journal before
   acting, and the founder's explicit confirmation immediately before the one submission.

### Rationale

DEC-475 already ruled that product choices may not live in the shell. A first trade that still
selects its instrument or model in code proves nothing the product will use. The quant model is
days less work than the LLM path, and it needs no gateway or inference registry, which the
inference spec reserves for model calls; the LLM model adds them without changing the cycle API.
Requiring only the confirmed mandate, not the whole agent process, keeps the first trade honest
while about one to two weeks away rather than several.

### Alternatives considered

- **The LLM research model first.** Rejected for the first trade: it needs spend caps, the
  research agent, and its retrieval before any order, which is weeks more. It follows.
- **The full agent process (E19-1) as the bar.** Rejected for the first trade, kept for the soak.
- **The production path with a model output supplied by hand.** Rejected: a hand-fed output is
  close to the synthetic proof model DEC-475 forbids.
- **Route quant models through the gateway and registry too**, amending inference spec §1.3 and
  §4.1 for one path for every model type. Rejected by the founder: a spec change plus the gateway
  before the first trade, for a model that makes no call.
- **Keep the existing order of work.** Rejected: the paper trade proves the integration that
  every later milestone rests on.

### Revisit when

The first trade is placed, or a step of item 1 turns out to need a founder-reserved decision
(DEC-79); the brief for this path names any it finds.

<a id="dec-509"></a>

## DEC-509: The first paper trade is on SPY; BTC/USD follows as a mandate input

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-07 (UTC) |
| **Decided by** | The founder, in session on 2026-10-07, on the coordinator's evidence below; recorded by an agent (Claude Code, the builder of the first paper trade brief) |
| **Story or claim** | DEC-502; E7-19, E7-21, E7-22; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) |

### Decision

1. **The first paper order placed through the production path ([DEC-502](earlier-303-599.md#dec-502)) is on
   SPY**, a liquid US ETF, on the founder's Alpaca paper account. This amends DEC-502 item 3 and
   supersedes [DEC-450](earlier-303-599.md#dec-450).
2. SPY reaches the path as an input, in the confirmed mandate's pinned instrument and the
   registered instrument snapshot, never as a constant in shipping code
   ([DEC-475](earlier-303-599.md#dec-475) item 3). The founder confirms it in the mandate.
3. **BTC/USD follows later as a mandate input,** once the crypto slices land (E7-21, E7-22,
   E7-4's crypto protection). Nothing in the path is specific to SPY, so a BTC/USD deployment
   needs those slices and a confirmed version, not a release choosing it.
4. Nothing about any rule changes for SPY. The US equity rules apply in full: regular-session
   openings outside the closing window, the eligibility floor with SPY classified as a plain ETP
   from a dated source, the collar, participation caps, buying power at 1×, and the day-trading
   regime.

### Rationale

The coordinator downloaded BTC/USD daily bars from Alpaca's `crypto-us` feed for 2026-09-01 to
2026-10-06. Their 30-day median daily dollar volume is about 127,900 USD, against the gate's
1,000,000 USD `crypto_liquidity_floor_usd`, which is also the platform minimum (trading-domain
spec §3.2 item 7). The gate would deny the opening `below_liquidity_floor`, and lowering the floor
would weaken a safety rule. M1's exit run had already shown a thin venue: 3,709 of 7,150 BTC/USD
minute bars carried zero volume. SPY passes every equity floor by a wide margin and needs no
crypto slice, so the first trade can be placed sooner without relaxing anything. The cost is
that it runs only inside the regular session.

### Alternatives considered

- **Keep BTC/USD and lower the crypto floor.** Rejected: it weakens a safety rule (DEC-79), and
  only to make one order possible.
- **Keep BTC/USD and wait for volume.** Rejected: nothing suggests the venue's volume will rise
  above the floor.
- **Another crypto pair.** Rejected: the same venue, likely thinner still.
- **AAPL**, E7-7's instrument. Not chosen: SPY's volume is larger, and a new instrument shows
  that the path takes the instrument as an input.

<a id="dec-510"></a>

## DEC-510: J0's artifact-aware Postgres append: its API, its outcomes, and how its tests reach the stub

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the J0 builder), under DEC-79: reversible engineering choices |
| **Story or claim** | J0, E7-19 slice 4's remainder; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) X-10 |

### Decision

1. **API.** `mandate-journal-pg` gains `PgJournal::append_with_config_artifacts(stream,
   expected_head, writer_epoch, recorded_at, drafts, artifacts)`, named and shaped after
   `MemoryJournal::append_with_config_artifacts`. `artifacts` is `&(dyn ArtifactSource + Sync)`, so
   the append's future stays `Send`; `mandate-artifacts-fs`'s `FsArtifactStore` and the in-memory map
   are both `Sync`. It returns `Result<AppendOutcome, AppendError>`, where `AppendError` is
   `Integrity` (stored events failed re-verification, as `append`'s `IntegrityError`) or, until the
   implementation PR, the stub's `Unimplemented`. The plain `append` is unchanged: it still refuses
   every draft `Draft::config_artifact_path` names as `missing_artifact`, so a caller without an
   artifact store keeps failing closed.
2. **Outcomes.** For the same stream state, drafts, and artifacts, the Postgres append gives exactly
   `MemoryJournal`'s outcome, and holds the same heads and rows afterwards. That includes the order of
   journal spec §5.1: idempotency first (an identical retry is `AlreadyCommitted` and a changed resend
   `IdempotencyConflict`, whatever the store holds or whether it can be read), then the configuration
   objects, then fencing and the head check. A missing object is `Invalid` `missing_artifact` at the
   member that references it; an object whose bytes do not hash to their reference, or a store that
   cannot be read, is `Unavailable` (safe to retry), as `MemoryJournal` maps them; the reference
   cases' invalid references keep their reasons and paths. A refused batch writes nothing and uses no
   `seq`.
3. **An unreachable database is `Unavailable` before any object can refuse a draft.** Only the
   database can decide idempotency, so a draft that may already be committed is never refused for its
   artifacts. Every J0 test starts with this property, which needs no database: `cargo xtask ci
   pending` runs in `fast`, where there is no database (DEC-109 item 7), and a Postgres test that
   skips there would pass, which the pending gate refuses. Starting with the property makes every
   pending test stop at the stub with or without a database, and after the implementation the
   Postgres tests still skip without `MANDATE_PG_URL` and run in `full`.
4. **One predicate for both stores.** The implementation PR exports `mandate-journal`'s existing
   check of a batch's configuration objects (the loop `MemoryJournal::append_inner` runs over
   `validate_config_artifacts`) as a public function that both journals call, with no change to any
   outcome, rather than copying it into `mandate-journal-pg`. The first paper trade brief lists
   `mandate-journal` as "used, not changed"; this is the one change it makes there, and it makes the
   two stores' outcomes equal by construction. The objects are read inside the append's transaction,
   after the idempotency lookup, while the head row is locked.
5. **Oracles.** The tests compare every append with `MemoryJournal` and also state each outcome
   independently: the reference cases' expected reasons and paths, §5.1's order written into the
   test, and, in the property, an outcome derived from how each step was built, from the test's own
   record of what committed and a table of which members each version-2 record must have stored
   (journal spec §9's append-time check, DEC-484). Because of item 4, a bug in the shared predicate
   would make both journals agree; the independent oracles catch it (the planted-bug table in J0's
   tests PR).

### Rationale

1. Same name and argument order as the memory journal, so a caller switches stores by type; the
   `Sync` bound costs nothing for the two stores that exist and keeps the journal usable from
   spawned tasks. A separate error type keeps `append`'s signature unchanged.
2. The goal of J0 (X-10) is that the CLI's registrations and the shell's version-2 records reach
   Postgres with the outcomes they already have in memory; any difference would make the store
   choice change what is journaled.
3. DEC-109 keeps `fast` database-free on purpose; the pending gate must still see each pending test
   fail at its stub.
4. Two copies of a predicate drift; the differential tests would catch a drift only on the inputs
   they draw. Exporting a function moves no behavior.

### Alternatives considered

- **Pass the artifact source to `PgJournal::new`.** Rejected: the CLI and the paper adapter would
  need two journal values for one pool, and a plain append could no longer be told from an
  artifact-aware one at the call.
- **`&dyn ArtifactSource` without `Sync`.** Rejected: the append's future would not be `Send`.
- **Copy the check into `mandate-journal-pg`.** Rejected for item 4's reason.
- **Read the objects before the transaction.** Rejected: an unreachable database would then answer
  `Invalid` for a draft that may already be committed, contrary to §5.1's order (item 3).
- **Mark the Postgres tests pending only in `full`.** Rejected: the pending gate runs in `fast`
  (DEC-110) and has no per-job exception.

<a id="dec-512"></a>

## DEC-512: One state system: modes named by what the agent may do, one verdict chip, restrictions in one line

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | the agent, under DEC-79 (a reversible product decision), from the founder's direction to work `web/design/plan.md` |
| **Story or claim** | plan D in `web/design/plan.md`, from findings R-3, R-8 and R-10 of `web/design/review-2026-10-07.md`; PR #669 |

### Decision

1. **A mode is named by what the agent may do.** `normal` reads "Trading", `exits_only` reads
   "Selling only", `paused` "Paused", `stopped` "Stopped". "Normal" told an owner nothing until
   they met "Exits only"; "Trading" needs no second word to be understood. The spec's terms
   (`exits_only`, exits-only) are unchanged in the code, the journal and the documents; only the
   owner's word changes. Every consequence that named the mode follows ("Selling only until a new
   risk day", "Drawdown: selling only").
2. **The mode chip is a dot and a word.** The checkbox, logout, pause and cancel glyphs go: a ticked
   box read as a control the owner could toggle. A 6px dot in the chip's own colour (a ring for
   selling only, half stopped) marks it as a state; the chip's field (the well, an ink ring, solid
   ink) and the word carry the mode as before.
3. **One verdict chip, one width, first in its row.** Every list of gate decisions draws the
   verdict with `VerdictChip`: a quiet chip on the well for an allow that went out, an ink ring for
   everything else, in a column of one width (`--verdict-w`) before the action, so "Not allowed"
   and "Allowed" line up and never wrap.
4. **"Asked you" is a verdict in a list.** An allow the mandate turned into a request
   (`approval_id` set) reads "Asked you" beside its action, so "Allowed" only ever means the gate
   passed it and it went out. "Allowed" next to "Asked you for approval" read as "it bought it". In
   a sentence about the gate itself (the desk's risk step, "Risk gate: Allowed.") the gate's own word
   stays, since there the sentence says who asked.
5. **Restrictions on an agent's row are one line.** "Drawdown: selling only since 14:01", each
   with what it blocks and how it ends behind a hover, not a full-sentence pill per restriction that
   tripled the row. The agent page's restrictions panel keeps the full text.

### Rationale

The review found three things an owner would misread: a mode word with no meaning on its own, a
glyph that looked like a control, and a verdict that contradicted the sentence under it. All three
are in the state system, so they are fixed together, as one set of rules, rather than three patches.

### Alternatives considered

- **Keep "Normal" and add a tooltip.** A tooltip is not read on a phone, and the word still means
  nothing in a list.
- **Colour the verdicts.** Rejected by the Meaning Rule: a verdict is not a gain or a loss, and
  crimson is the kill switch alone.
- **Keep the restriction pills on the row.** They are the right content on the agent page and the
  wrong size on a list.

<a id="dec-513"></a>

## DEC-513: The cutting pass: no door that does not open, one menu, Go to before Stop, less chrome

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | the agent, under DEC-79 (reversible product and engineering decisions), from the founder's direction to work `web/design/plan.md` |
| **Story or claim** | plan C in `web/design/plan.md`, from findings R-4, R-8, R-17, R-18 and R-21 of `web/design/review-2026-10-07.md`; PR #669 |

### Decision

1. **No door that does not open.** A screen whose `built` is false keeps its route and its
   "coming" page with its purpose (`routes.test.tsx` still holds that a direct address is never a
   dead end), but no dock, menu, sheet or palette lists it (`listedScreens()`). The Audit and
   Workspace index pages list the built screens as doors and name what is coming in one line of
   text. Connections, unbuilt, leaves the dock.
2. **One menu.** The dock is Home, Messages, Approvals, Alerts, Agents, Positions, a hairline,
   More, a hairline, Stop. Audit's own menu folds into More under its group label, opening with the
   audit overview; the workspace group keeps its overview alone while every screen under it is
   still to come, since the overview names them. The phone's More sheet lists the same screens.
3. **Go to before Stop.** The palette lists Go to, then the agents, then Safety with "Stop…" last.
   First in the list with nothing typed, "Stop…" read as the default act. Stop is on every screen
   already, and typing its name still finds it. This amends the sentence in DEC-208's record that
   put it first; the Control Rule is untouched.
4. **The More sheet leads with what the dock does not hold.** Approvals and Alerts first, then
   Search, then the rest; the account, the workspace switcher and the theme at the foot. The sheet
   carries no "Fixture data" tag of its own; the page's foot has one.
5. **One "Fixture data" tag per screen.** The chart's footer and the Positions screen drop theirs;
   the shell's, at the foot of every non-record page, stays.
6. **The paper badge sits in the header and on record screens.** List screens (Agents, Positions,
   Approvals, Alerts, the section indexes and the registry screens) no longer repeat it in their
   title; the agent page and a request, being records, keep it, as DEC-204's safety resolution 7
   requires.
7. **Provenance tags are short.** "Proposed", "Default", "Template", "You said", "You entered",
   in two shapes as before: a hairline for what the owner gave and a dashed border for what the
   platform authored. The set-up summary uses the same words.
8. **Messages' rail waits for room.** The thread is the widest column; the rail shows from 80rem,
   not 64rem, so at 64 to 80rem the thread is not the narrowest thing on the page.
9. **Needs you holds the requests and the agents' conditions.** A degraded feed asks nothing of
   the owner and the status strip and Alerts already say it, so it leaves Needs you; an agent's
   condition is one line per agent and condition, with every instrument it covers in one pair of
   brackets, not a line per instrument. The Alerts screen and the Stop control's reasons still
   count every line (`alertLines`).

### Rationale

The review's one finding above the rest was that the product's weaknesses were duplicates and
unbuilt doors, not missing features: six screens that said "next slice", two navigation systems
on desktop, three "Fixture data" tags on one page, a verdict chip in two styles, a badge repeated
in every title. Each item here removes something standing between the owner and the three things
the product has to show (the owl, the rules, the record); none adds a feature.

### Alternatives considered

- **Keep the unbuilt pages as a roadmap in the navigation.** A door that does not open is a
  promise broken on first click; the index pages keep the roadmap in one honest line.
- **Keep Audit as its own menu.** Two menus beside each other for seven screens was the dock's
  second navigation system; the group label inside More carries the distinction.
- **Keep "Stop…" first in the palette.** The reason it was first (one keystroke from anywhere)
  holds just as well last: the dock's Stop is on every screen, and the palette finds it by name.

<a id="dec-514"></a>

## DEC-514: The owls hatch once, and draw only on whole pixels

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | the agent, under DEC-79 (a reversible product decision), from the founder's direction to work `web/design/plan.md` |
| **Story or claim** | plan E in `web/design/plan.md`; PR #669 |

### Decision

1. **A new agent's owl hatches.** When the runtime records the agent (the "recorded" phase after
   the passkey), its owl rises out of an egg drawn on the sprite's own 16-pixel grid: the shell parts
   along its crack, the top half lifting away and the bottom half sinking, over 600ms, once. It
   answers the owner's act of creating the agent (DESIGN.md, Motion) and never plays again. Under
   reduced motion the owl is simply there, with no egg (`HatchingOwl`, `owl.test.tsx`).
2. **Owls draw only on whole pixels.** An owl renders at 16, 32, 48, 64 or 80px, where each of its
   16 sprite pixels is a whole number of screen pixels, as Pixelarticons already do on their 24px
   grid. The 24, 28 and 56px owls (the copilot's header, the phone thread bar, the set-up's progress
   and the landing assistant) move to 32 and 64px. The rule is in DESIGN.md under Owls.

### Rationale

The review found the owls and the icons reading as two products because their pixels were two
sizes, and that the product's most memorable moment, a new agent, ended in a sentence. Both are
inside the rules: the hatch answers an action and nothing about it follows money, and the grid is
the same discipline the icons already keep.

### Alternatives considered

- **A looping idle animation for a new owl.** Rejected: nothing in the frame moves on its own
  (DEC-215), and a loop is decoration.
- **Scaling the icons to the owls' pixel.** The icons are drawn at 1px per pixel on a 24px grid
  and would blur at any other size; the owls scale cleanly.

<a id="dec-515"></a>

## DEC-515: Home and the agent page, first pass: the request card, the sparkline's limit, the Stop sheet's weight, the equity row

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | the agent, under DEC-79 (reversible product decisions), from the founder's direction to work `web/design/plan.md` |
| **Story or claim** | plan F in `web/design/plan.md`, from findings R-2, R-5, R-13 and R-22 of `web/design/review-2026-10-07.md`; PR #669 |

### Decision

1. **A request is a pale volt card at every width.** Needs you's requests take the phone's tinted
   card on desktop too, so the one thing asking for the owner is the one tinted thing on the screen;
   an agent's condition stays a hairline row. The deadline stays a fixed time, never a countdown.
2. **A sparkline scales to its own range, with the limit as a region.** An agent row's sparkline
   no longer stretches its range to include the daily loss limit, which drew a quiet day as a flat
   line over a distant dashed rule; the line takes its own range, and the limit is the pale volt
   region below a dashed line, rising into view as the agent nears it and sitting as a sliver at
   the foot while it is far below.
3. **The Stop sheet's account choices share one weight.** Pause all, Stop all and Close everything
   are each an outlined row: Pause in an ink ring, the two kill switches in the crimson ring. Until
   now Pause was solid ink and Stop all solid crimson, so the most drastic choice was also the
   loudest, against "pausing is the least drastic and comes first". Close everything's list of what
   it does moves to its record screen, where it is confirmed; the sheet keeps one sentence that
   still names the orders Owlhead did not place and the owner's own holdings.
4. **"Equity now" is a marked row.** On the mandate ladder the current equity was a solid ink
   block that read as a button; it is now a row on the card with a volt ring and the marker dot the
   rail uses.

### Rationale

Each is one finding from the review, inside the rules (the Meaning Rule keeps ink for the account
and crimson for the kill switch; the Control Rule is untouched), and each is reversible. The rest
of plan F (the mandate as the agent page, the diary, the record you can test, the sand clock) is
larger and each will take its own decision.

### Alternatives considered

- **A countdown in the request card.** Rejected by the No-Nudge Rule.
- **Keep Stop all filled.** The fill said "most important", which on a sheet whose first line is
  "pausing is the least drastic and comes first" was the wrong emphasis; the crimson ring still
  names it the kill switch.

<a id="dec-518"></a>

## DEC-518: `quant.ma_crossover` 1.0.0's content object, and when its hash is pinned as a literal

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The E15-13 builder (Claude Code), under DEC-79 and DEC-176, on the coordinator's ruling of 2026-10-08 on the M1 plan |
| **Story or claim** | E15-13 M1; claim [#674](https://github.com/kunwarshivam/mandate/issues/674); [first paper trade brief](../tasks/first-paper-trade.md) |

### Decision

1. **When the literal is pinned.** DEC-504 item 2 asks for a test that pins each registered
   model's hash as a literal. The listed files include the host's own `src/ma_crossover.rs`, whose
   final bytes exist only once M2 implements it, and DEC-77 lets M2 change no test file. So:
   - M1 tests the **mechanism**: the host's content object must equal one written by hand in
     the test, with each listed file's SHA-256 read from disk at test time. Any edit to a listed
     file changes both, so the test still proves the hash covers the bytes the host was built
     from.
   - A follow-up PR after M2 merges adds the **literal**: a test asserting the hash of
     `quant.ma_crossover` 1.0.0 equals the 64 hex characters M2's bytes give. From then on, an
     edit to a listed file fails the build until the model gets a new version.
   - That PR lands before D1 registers the hash in any journal. Nothing registers the model
     before then, so no pin can name the unpinned hash.
2. **The files the hash lists** (`code`, sorted by path):
   - `crates/mandate-backtest/src/strategy/ma_crossover.rs`, the crossover and its `Signal`
     (M0);
   - `crates/mandate-modelhost/src/ma_crossover.rs`, the model's own host code: how its
     parameters are read and how its signal becomes an output.

   The host's generic checks in `src/lib.rs` (pin, registry, closes, calendar) are not the
   model's code, so an edit there does not change the model's hash.
3. **The content object of `quant.ma_crossover` 1.0.0**, the canonical JSON M1's test writes out:
   - `kind` `quant_model_content`, `content_version` 1, `model_id`, `model_version`, and `code`
     as in item 2;
   - `params_schema`:
     - `fast_periods`, an integer from 1 to 1000;
     - `slow_periods`, an integer from 2 to 1000;
     - the constraint `fast_periods < slow_periods`;
     - no defaults (DEC-52).
   - `inputs`: daily closes of the pinned instrument, the US-equity regular session as the
     trading day, at least `slow_periods` of them.
   - `output`: `long` is direction `long`, conviction 1, confidence 1; `flat` and `undecided`
     are none (DEC-157 item 4).
   - `methodology`: "Long when the mean of the last fast_periods daily closes is strictly above
     the mean of the last slow_periods daily closes, compared by cross-multiplying the window
     sums; otherwise no output." It describes the method only, with no performance claim
     (mandate spec §8.1).
   - `authorship`: `platform`.
4. **The version is 1.0.0.** E7-7's fixture pinned 1.0.0 to the digest of `{"id","version"}`,
   but no control stream ever registered it (DEC-504 item 4), so no journal has given 1.0.0
   another hash.
5. **`horizon_s` is `max_output_age_s`**, as the E7-7 mapping set it. The model states no holding
   horizon of its own, and the output stops being fresh at the same instant.
6. **1.0.0 serves US-equity sessions only** (added in M1b, from #683's review, under DEC-176).
   The content object's `inputs.trading_day` is `us_equity_regular_session`. That tightens
   DEC-504 item 1's "the trading day's boundary per asset class" to the one asset class this
   version reads: the host computes its closes' completeness, end session and `as_of` from the
   New York exchange calendar handed in to `evaluate` (never a calendar of its own, DEC-503), and
   from no other asset class's trading day (wording corrected in M1c, from #684's review). A
   crypto deployment, such as the first paper trade
   brief's BTC/USD follow-up, therefore needs a new model version with its own `trading_day`
   and content hash. It cannot pin 1.0.0. This narrows what 1.0.0 may be pinned for and adds
   no risk.

### Rationale

The literal cannot be written before the files it hashes are final without either an M2 that
edits a test file (against DEC-77) or an M1 that implements the host's code (not a tests PR).
Pinning it right after M2, and before anything registers the hash, keeps DEC-504's guarantee
("a model whose code changed cannot keep its version") for every hash any journal can hold.

### Alternatives considered

- **Pin the literal in M1.** Rejected: M1's host file is a stub, so the literal would be wrong as
  soon as M2 lands.
- **Let M2 add the literal test.** Rejected: DEC-77 lets an implementation PR change test files
  only by deleting `#[ignore]` lines.
- **Leave the host's own file out of `code`.** Rejected: DEC-504 item 1 lists the files that
  implement the signal *and its mapping to an output*, and the mapping is in that file.

<a id="dec-519"></a>

## DEC-519: every mutation shard has the Postgres database, and every mutant builds without debuginfo

| | |
|---|---|
| **Status** | Accepted; item 2's build profile extended with `opt-level = 1` outside the packages a mutant rebuilds by [DEC-931](earlier-900-1999.md#dec-931) (agent, DEC-79: a reversible change to the gate's configuration that judges more mutants and lowers, raises, or skips nothing; CI and xtask are founder-owned in `CODEOWNERS`, so the founder may overturn it) |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The DEC-519 builder (agent), under DEC-79 |
| **Story or claim** | A revision of [DEC-498](earlier-303-599.md#dec-498) (its item 6 restated, its items 1 to 5 kept); DEC-464's ten-minute limit; DEC-109's Postgres tests; found on [#678](https://github.com/kunwarshivam/mandate/pull/678) and [#677](https://github.com/kunwarshivam/mandate/pull/677) |

### Decision

1. **Every `mutants` shard runs with the database `full-checks` has.** Both jobs start the same
   digest-pinned PostgreSQL 18 from one script, `.github/scripts/start-postgres.sh`, before their
   `cargo xtask` step, and both set `MANDATE_PG_URL` and `MANDATE_PG_REQUIRED=1`. A mutant in
   code only the Postgres tests reach (`mandate-journal-pg` today, `mandate-cli`'s control journal
   next) is then judged by those tests instead of being reported missed while they skip, and a
   shard that lost its database fails rather than skipping them.
2. **Every build the mutation job makes is without debuginfo.** xtask sets
   `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` (`MUTANT_BUILD_PROFILE`) on the
   job's building children: the pre-flight's `cargo nextest list` and `cargo mutants`, whose
   baseline and mutant builds inherit them. So CI and a local `cargo xtask check` build mutants the
   same way. The matrix also sets both in its own environment, which puts them in rust-cache's
   key, so the dependencies it restores for the pre-flight were built with the same profile.
3. **DEC-498's items 1 to 5 stand unchanged:** 192 shards, one mutant a shard, one worker, a
   180-second test cap that also bounds the baseline's tests, and a 60-second build cap. The three
   capped phases still cost at most 420 of a shard's 600 seconds. No cap is lowered or raised, no
   mutant is skipped, and no crate is excluded.
4. **DEC-498's item 6 is restated with one more named term.** The remainder, everything no cap
   reaches, now includes starting the database. That term is beyond a mutant's reach: it runs
   before the gate, from a pinned image, and costs the same for every diff and every shard. It
   measured 8 to 23 seconds over 387 starts on `ubuntu-24.04` (below). The remainder measured 67
   to 84 seconds on this decision's populated shards. The widest seen so far is #677's, about 112
   seconds with debuginfo on; adding the slowest database start makes about 135, inside DEC-498's
   180-second reserve. The matrix does not grow: one mutant a shard is already the floor, and
   sharding cannot reduce a term every populated shard pays (DEC-498 item 8).
5. **The script waits for readiness over TCP and retries the pull.** The image's first start runs
   a temporary server on the Unix socket only, so a socket check can pass just before that server
   stops. A failed pull is retried four times, 2 to 16 seconds apart, because a transient registry
   error would otherwise redden a required check DEC-464 forbids retrying, as DEC-498 found for
   release downloads. The digest still decides what runs. Both changes apply to `full-checks`
   too, since it now uses the same script.
6. **Two xtask tests hold items 1 and 2.**
   `every_mutation_shard_has_the_database_the_postgres_tests_need` reads `ci.yml` and requires
   both jobs to set both variables and to start the script before their gate step.
   `the_mutants_job_builds_without_debuginfo` requires the two settings on the job's building
   children (with `CARGO_TARGET_DIR` still removed) and in the matrix's environment. Each was shown
   to fail on a seeded bug: `MANDATE_PG_URL` removed from the matrix, and `.envs(...)` removed from
   `mutants_job_cargo`.
7. **What this does not settle: the mutant build of a low-layer crate.** The baseline builds only
   the mutated packages (DEC-498, "The baseline's packages come from the shard's own mutants"), so
   a `mandate-time` mutant's first build compiles the rest of its tested packages' graph
   (`mandate-shell`, `mandate-refcases` and everything under them) from nothing, inside the
   60-second cap. Debuginfo off brought #677's shape under the cap on CI (below). How far under is
   not measured, because cargo-mutants prints no build time for a caught mutant. A diff whose
   tested graph is larger could still pass the cap. If it does, the failure is loud: a `TIMEOUT`
   makes `cargo mutants` exit 3, and `mutants_outcome` refuses it. That case needs its own
   decision, taken against a measurement of the diff that caused it. No remedy is named here
   (DEC-498 item 9).

### Rationale

#### The database (item 1)

The Postgres tests skip without `MANDATE_PG_URL` (DEC-109). CI's mutants shards never set it, so
every mutant in Postgres-only code ran against tests that passed by skipping. The gate then
reported it missed and failed an honest PR (#678's `lost_a_race` guard). The same three
`mandate-journal-pg` mutants, run as the gate runs them (`--test-tool nextest --jobs 1 --timeout
180`, `--test-package=mandate-journal-pg`, debuginfo off), against a throwaway local
PostgreSQL 18:

| Database | `lost_a_race` guard → `true` | `lost_a_race` guard → `false` | `row.hash == head.hash` guard → `false` | Baseline |
|---|---|---|---|---|
| none | MISSED | MISSED | MISSED | 71s build + 10.7s test |
| `MANDATE_PG_URL` set | caught | caught | caught | 67s build + 20.0s test |

On CI, with this change, on a throwaway branch (`claude/hejxo4-dec-519-measure-pg`, run
[37721443293](https://github.com/kunwarshivam/mandate/actions/runs/37721443293)) whose only
source edit is an equivalent reordering inside `lost_a_race`, both of its mutants
(`lost_a_race -> bool` with `true`, and with `false`) were caught. Their baselines ran the
Postgres tests for real: 22 and 27 seconds of build, 14 and 16 of test. The jobs took 102 and 113
seconds.

`MANDATE_PG_REQUIRED` is what makes this fail closed. Without it, a shard whose database failed
to start would skip the tests again and report the mutants missed. That is loud, but it is the
wrong reason. With it, the tests themselves fail.

#### Starting the database costs 8 to 23 seconds (item 4)

| Where | Runs | `Start PostgreSQL 18` step |
|---|---|---|
| `full-checks`, before this change | 37710536919, 37714195405, 37716655304 | 10s, 17s, 8s |
| `mutants`, 192 shards, `measure-time` branch | 37721440822 | 8 to 20s, median 10.5s |
| `mutants`, 192 shards, `measure-pg` branch | 37721443293 | 8 to 23s, median 11s |

So 384 shards pulled the image from Docker Hub within a few seconds of each other, and none
failed. That is the extent of the evidence on registry limits. A pull that fails five times fails
the shard, loudly.

#### Debuginfo (item 2)

#677 (run [37710536919](https://github.com/kunwarshivam/mandate/actions/runs/37710536919)),
with debuginfo on: a `mandate-time` mutant whose tested packages were `mandate-time`,
`mandate-shell` and `mandate-refcases` built for 11 seconds as a baseline, then hit `TIMEOUT` at
60 seconds as a mutant (shard 3; shards 1 and 4 likewise). The baseline is quick because it
builds `mandate-time` alone. The mutant is not, because it builds everything else.

The same shape with debuginfo off, on a throwaway branch (`claude/hejxo4-dec-519-measure-time`,
run [37721440822](https://github.com/kunwarshivam/mandate/actions/runs/37721440822)) that makes
an equivalent edit in `last_completed_regular_session` and touches `mandate-shell`'s crate doc,
so the tested packages are the same three. All four mutants (shards 0, 2, 3 and 4) were caught,
none `TIMEOUT`. Each baseline built in 7 to 8 seconds and tested in 1. Each mutant's build and
test together took 100 to 121 seconds, and each job 167 to 189.

Measured locally, on four CPUs shared with four other agents (load average 10 to 16 throughout),
so wall-clock times there are noisy and CPU time is the steadier figure:

| Build of the three packages' tests (`cargo nextest run --no-run`) | Debuginfo on | Debuginfo off |
|---|---|---|
| From nothing, CPU seconds (user + sys) | 230, 226 | 164, 174 |
| From nothing, wall seconds | 129, 122 | 111, 207 |
| After a one-file change in `mandate-time`, CPU seconds | 80 | 44 |
| After a one-file change in `mandate-time`, wall seconds | 53 | 29 |

| Through `cargo mutants`, one mutant | Debuginfo on | Debuginfo off |
|---|---|---|
| `mandate-time` (`session.rs:337:74`, `<=` → `>`), tests `mandate-time`, `mandate-shell`, `mandate-refcases` | baseline 15.5s build; mutant 119.6s build + 115.2s test | baseline 18.9s and 26.5s build; mutant 124.6s and 136.9s build, 105.5s and 126.4s test |
| `mandate-journal` (`config_artifact_refusal` → `None`), tests `mandate-journal`, `mandate-journal-pg`, `mandate-refcases`, with the database | baseline 45.3s build + 7.2s test; mutant 150.4s build + 16.7s test | baseline 25.8s build + 4.3s test; mutant 69.7s build + 18.5s test |

So debuginfo off saves about a quarter of the CPU of building the graph from nothing, and nearly
half after a one-file change. On a machine this loaded, wall time does not separate the two
`mandate-time` cases at all. A `--leak-dirs` run confirmed the setting reaches cargo-mutants'
builds: the `mandate-shell` test binary it built has no `.debug_info` section. The "12 to 25
seconds without debuginfo" seen earlier is the one-file-change case, where the graph is already
built. A cargo-mutants mutant build is not that case (item 7).

Debuginfo changes no test's verdict, only what a backtrace can name, and the gate reads verdicts.

#### The budget

DEC-498's equation is unchanged, with `k` the mutants in a slice:

```
k × (60 + 180) + 180 ≤ 600 − remainder      k = 1, so the remainder has 180 seconds
```

The remainder's terms, measured on `ubuntu-24.04`:

| Term | Capped? | Measured | Depends on |
|---|---|---|---|
| Job setup, checkout, tools, toolchain, cache | no | 19 to 27s, the database step aside | nothing a mutant touches |
| **Start PostgreSQL** (new) | no; out of a mutant's reach | 8 to 23s | nothing a mutant touches; registry health |
| xtask build and metadata | no | 4 to 9s | the diff, slightly |
| Pre-flight `cargo nextest list` | no | 8 to 10s for `mandate-time` (this run); 69s for `mandate-shell` and `mandate-time` with debuginfo on (#677 shard 3); 5 to 25s for `mandate-journal-pg` | the diff; item 2 lowers it |
| Enumeration, startup, tree copy | no | about 1 to 2s | the diff |
| Baseline build | no (`--build-timeout` spares it) | 7 to 8s (`mandate-time`); 22 to 27s (`mandate-journal-pg`); 11s with debuginfo on (#677) | the diff; item 2 lowers it |

The remainder of a populated shard on this decision's runs was 67 seconds (`measure-time` shard
0: a 189-second job, less 121 for its mutant and 1 for its baseline's tests) and 84 seconds
(`measure-pg` shard 1: a 113-second job, less 13 for its mutant and 16 for its baseline's tests).
The widest remainder anyone has measured is #677 shard 3: 19 seconds of setup, 9 of xtask, 69 of
pre-flight, 1 of enumeration, 11 of baseline build and about 3 more, so about 112, with debuginfo
on and no database. Adding the slowest database start seen gives about **135 against the 180
reserved**. So no shard is added and no cap moves.

The capped test phases themselves now include Postgres tests that used to skip: 14 to 16 seconds
for `mandate-journal-pg`'s baseline on CI. A test set holding both those tests and the reference
harness (DEC-498's slowest legitimate harness run, 83 seconds) would come to roughly 100. The
180-second cap would then be 1.8 times the slowest legitimate run rather than 2.2. That
combination has not been run on CI. It is recorded here as unmeasured, not as a bound.

The slowest shard on either run took 189 seconds of its 600, and every other required job stayed
under ten minutes: `fast` 321 and 416 seconds, `full-checks` 299 and 261.

### Alternatives considered

- **Start the database only in shards with a mutant.** It would save the 8 to 23 seconds in
  empty shards, which have no budget problem. But only the gate knows whether a slice is
  populated, so the gate would have to start a container. That is a second path to the same
  database for no gain where the budget is tight.
- **A GitHub `services:` container, as `nightly.yml` does.** It costs the same pull and start,
  and it would be a second way of starting the same database beside `full-checks`' step. One
  script for both jobs keeps one digest and one readiness check.
- **The runner image's preinstalled PostgreSQL.** It is not the version `full-checks` tests
  against (ES-08), and it is not pinned.
- **Leave debuginfo to the caller's environment.** Only CI would get it, and a local
  `cargo xtask check` would build mutants differently from the gate it is meant to reproduce.
- **Set debuginfo only in xtask, not in the matrix's environment.** rust-cache keys on `CARGO*`
  variables, which its own log lists. Without them in the job's environment, the cache would keep
  restoring dependencies built with debuginfo, and the pre-flight would rebuild them in every
  shard.
- **Raise the build cap.** Refused by the task and by DEC-498 item 5: the 420 capped seconds are
  chosen together.

<a id="dec-520"></a>

## DEC-520: P0's Postgres `ControlJournal`, the `--journal` and `--store` options, and their tests

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the P0 builder), under DEC-79: reversible engineering choices |
| **Story or claim** | P0, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) P0 and X-12 |

DEC-511 and DEC-516, the numbers first named for this, were taken by #669 and #679; 520 was free on
`main` and every branch when it was reserved.

### Decision

1. **The adapter.** `mandate-cli` gains `postgres::PgControlJournal`, `mandate-journal-pg`'s
   `PgJournal` and `mandate-artifacts-fs`'s `FsArtifactStore` behind the existing
   `ControlJournal` trait. Its `append` is J0's `append_with_config_artifacts` reading the store
   (DEC-510), so a registration whose object is not stored is refused `missing_artifact` and writes
   nothing; the trait is unchanged. The adapter owns a current-thread Tokio runtime and blocks on
   each call, as `mandate-shell`'s `StoreJournal` does, so a command uses it outside any other
   runtime; `wait` sleeps. A journal call that returns no answer is a `ControlError::Journal`
   whose message is `the journal failed: <code>`. For `rows`, `head` and `take_ownership`,
   `<code>` is `mandate-journal-pg`'s `PgError` code, `unavailable` or `integrity`; for `append`,
   which answers an unreachable database as the outcome `Unavailable`, it is the §11 check the
   stored bytes failed (`non_canonical`, `rehash_mismatch`, and so on). Opening refuses with
   `the journal DSN does not parse` or `the artifact store: <code>` (the store's `ArtifactError`
   code). No message carries the DSN: `JournalArgs::journal`
   is a `SecretString` exposed only to `PgJournal::from_dsn` (item 7). Amended in the
   implementation PR (#680's review, minor 2) to say exactly what the code does.
2. **The options.** `postgres::JournalArgs` is a `clap::Args` with two required options,
   `--journal <DSN>` and `--store <DIR>`, which D1 and D2 flatten into their commands (V0 takes
   `--journal` alone, DEC-522 item 2). That is
   the skeleton P0 owes; it adds no top-level command, because each of D1, D2 and V0 adds its own.
   The DSN carries no password: sqlx reads one from `PGPASSWORD` or `~/.pgpass`, which keeps it out
   of the shell history and the process list. `JournalArgs`' `Debug` prints no DSN, and no error
   repeats it (`AGENTS.md` rule 7). The CLI does not refuse a DSN that does carry a password: the
   tests' and CI's DSNs do, and the rule is about what the CLI prints.
3. **The tests reach the stub without a database.** As DEC-510 item 3 does for J0, every Postgres
   test starts with a property that needs none: a DSN that does not parse is refused when the
   journal is opened, as a journal error that does not repeat the DSN's password. The pending gate
   runs in `fast`, where there is no database (DEC-109 item 7).
4. **Oracles.** The Postgres tests compare the adapter with three oracles that share no code with
   it: the journal vectors' control stream (stored bytes and hashes); `MemoryJournal`'s
   artifact-aware append over the same objects; and the CLI tests' own journal (`tests/common`),
   the one every M7 command test runs against, on ownership, fencing, a moved head, an identical
   retry and a changed resend. The comparison with that journal reads outcome kinds, `seq`,
   `event_id`, epochs and heads, not sealed bytes, which it does not compute.
5. **Test setup.** The tests reuse `mandate-journal-pg`'s per-test schema (`tests/support`) and
   reach it through the DSN `open` takes, with `options=-c search_path=<schema> -c
   role=mandate_journal_app`, so they exercise the DSN path the founder's run uses. That module
   needs `sqlx` and `proptest` as `mandate-cli` dev-dependencies (both already in the workspace and
   `docs/dependencies.md`) and Tokio's multi-thread runtime in tests.
6. **CI.** `cargo xtask ci postgres` runs `mandate-cli` beside `mandate-journal-pg`, so these tests
   run against PostgreSQL 18 in `full`; elsewhere they skip without `MANDATE_PG_URL`, as the
   journal's do.
7. **The DSN is a secret by type** (amended after #680's review, minor 1). `JournalArgs::journal`
   is a `secrecy::SecretString`, as `mandate-marketdata` and `mandate-alpaca` hold their keys, so
   `Debug` cannot print it and the hand-written redacting `Debug` goes; the implementation exposes
   it only to `PgJournal::from_dsn`. This tightens item 2's rule 7 from tested to unrepresentable.
   It is an interface change, so it lands as a tests correction before the implementation PR,
   changing two lines of `tests/postgres.rs` (the `args` helper converts with `into`, and the
   options test compares `expose_secret()`); `mandate-cli` gains `secrecy` in `allowed_external`.

### Rationale

1. The trait already names every call a command makes; keeping it unchanged leaves the M7 commands
   and their tests untouched. A runtime per adapter is the pattern the shell already uses, and the
   CLI's binary runs one command per process.
2. A required option is refused by `clap` before anything runs. Passwords on command lines leak
   through history and process listings; sqlx's environment and pgpass support removes the need.
3. and 6. DEC-109 keeps `fast` database-free; `full` is where Postgres tests run.
4. Each oracle catches a different class of fault: the vectors catch any change to the stored
   bytes, `MemoryJournal` catches an append that does not read the store, and the CLI tests'
   journal ties the adapter to the retry arms the commands were tested against.

### Alternatives considered

- **Global `--journal` and `--store` on `Cli`.** Rejected: every command would carry them, and
  `download` and `inspect` need neither.
- **Refuse a DSN with a password.** Rejected for now (item 2); a later change can tighten it once
  CI passes its password through `PGPASSWORD`.
- **Make the trait async.** Rejected: every command and every M7 test would change for one adapter.
- **Copy the schema setup into `mandate-cli`.** Rejected: one setup, shared by path, cannot drift.

<a id="dec-522"></a>

## DEC-522: V0's `mandate journal export`, and a synchronous `main` for commands on their own runtime

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the V0 builder), under DEC-79: reversible engineering choices |
| **Story or claim** | V0, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) X-11 |

DEC-521 was the highest number on `main` and every branch when this was reserved (DEC-516 item 6).

### Decision

1. **`main` is synchronous.** `mandate`'s `main` stops being `#[tokio::main(flavor =
   "current_thread")]`. `download`, the one command that runs async code, builds its own
   current-thread runtime and blocks on it. Commands on the workspace journal (`journal export`
   now, D1 and D2 next) block on the journal's own runtime, as `PgControlJournal` does, which panics
   inside another runtime. That constraint was found in #687's review, and V0 is the first command
   wired through `main`.
2. **`mandate journal export <stream> <out> --journal <DSN>`.** It writes every event of one
   stream, from `seq` 1, as the journal spec §6.2 segment `journal verify` reads. Each event is
   re-checked on read (§11 checks 1 to 5, `PgJournal::rows`), and the file is written with
   `mandate_journal::export_segment`, the function the tests' exports already use. It takes
   `--journal` alone, not DEC-520's `JournalArgs`: an export reads no artifact, and `journal verify
   --store` is where the artifacts are checked (X-11). The DSN is a `SecretString`, exposed only to
   open the journal.
3. **Refusals, each before anything is written, each led by its code:**
   - a stream id that is not one of §2's (`export_stream_id_invalid`), before the journal is
     opened;
   - an `<out>` that exists (`export_file_exists`), because a segment never replaces evidence;
   - a stream with no event (`export_stream_empty`), whose segment would verify nothing;
   - and the journal's own `the journal failed: <code>` (DEC-520 item 1).

   No message names the DSN.

   **How the file is written** (amended in the implementation PR, on #691's review):
   - the segment goes whole into `<out>.tmp`, created with `create_new`, then `sync_all`;
   - it is published under `<out>` by `std::fs::hard_link`, and the temporary file is removed
     afterwards, whether the publish succeeded or failed;
   - the directory is then flushed.

   The reasons:
   - A segment cut short at a line boundary verifies as a shorter valid stream, so `<out>` must
     hold the whole segment or not exist.
   - A rename on Unix (`rename(2)`, which `std::fs::rename` calls) replaces an existing target
     without error. `link(2)` never does: it fails with `EEXIST`. So an `<out>` that appears
     between the first check and the publish is not overwritten: the export fails with the
     system's "file exists" error, which no test can drive without a hook between the two steps.
     `mandate-artifacts-fs` publishes its objects the same way. `renameat2(RENAME_NOREPLACE)`
     would need a new dependency for the same guarantee.
   - A `<out>.tmp` that already exists is refused, and left in place, so a concurrent export's
     temporary file is never written over or removed.

   V0 writes no manifest and no `SegmentExported` event: those belong to the cold store's segment
   shipping (§6.2), not to an export for verification.
4. **Tests.** They extend `crates/mandate-cli/tests/postgres.rs`:
   - the export equals the control-stream vectors' segment, built line by line from their
     canonical bodies and hashes, and `journal verify` passes it with the store;
   - the refusals of item 3;
   - the binary exports a stream and then verifies it, which pins item 1, since an asynchronous
     `main` panics there.

   Each test starts with the bad-stream refusal, which needs no database, so the pending gate sees
   the stub in `fast` (DEC-510 item 3).

### Rationale

1. A synchronous `main` puts each command's runtime where it is used and runs nothing inside
   another. The alternative, moving journal commands onto a plain thread, keeps an unused runtime
   on `main` for every command but `download`.
2. One function writes every segment, so `journal verify` reads exactly what the journal's own
   tests export. Requiring `--store` for a command that never reads it would only invite a wrong
   path.
3. An export is evidence for an auditor. Overwriting one, or writing an empty one that verifies, are
   the two ways it could mislead.

### Alternatives considered

- **Run journal commands on a spawned thread inside the async `main`.** Rejected for item 1's
  reason.
- **Flatten `JournalArgs` and check artifacts while exporting.** Rejected: it duplicates `journal
  verify` and refuses to export a journal whose store is missing an object, which is the journal an
  auditor most needs to see.
- **Export a `seq` range.** Not now. The first paper trade's streams are small, and a range needs a
  trusted start the export would have to print; a later story can add it.

<a id="dec-523"></a>

## DEC-523: The registered instrument snapshot's exact members, types and value sets

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The E19-11 builder (Claude Code), under DEC-79 and DEC-176, on the coordinator's ruling of 2026-10-08: it pins a shape the first paper trade brief names only by its members, and every choice is the narrower one |
| **Story or claim** | E19-11 (D3) and E10-16 (D1); [first paper trade brief](../tasks/first-paper-trade.md) ("The instrument snapshot's shape"); DEC-505 item 1 |

### Decision

1. **The object.** An `instrument_snapshot` configuration object, the one D1 (`mandate config
   register`) stores and registers with `ConfigSnapshotRegistered` (kind `instrument_snapshot`)
   and the one D3's reader in `mandate-shell` takes, is a canonical JSON object ([journal spec
   §4](../../specs/journal.md)) with **exactly these eight members**. An unknown member, a
   missing member, or a member of another type is refused (as DEC-466 item 2 refused it for the
   E7-7 file):

   | Member | Type | Allowed values |
   |---|---|---|
   | `asset_class` | string | `us_equity` |
   | `etp` | string | `plain` |
   | `etp_classified_at` | string | An RFC 3339 instant with an upper-case `T`, as `UtcNanos::parse_rfc3339` reads it. D1 writes the UTC form, `YYYY-MM-DDTHH:MM:SSZ` |
   | `etp_source` | string | `nasdaq_trader_symbol_directory` |
   | `exchange` | string | `arca` or `nasdaq` |
   | `increment` | string | `whole` |
   | `instrument_id` | string | The broker's asset id, non-empty |
   | `symbol` | string | The instrument's symbol, non-empty |

2. **`etp_source` is a closed set.** Its one value names where the ETP classification was taken
   from: the Nasdaq Trader symbol directory's ETF column (trading spec §3.2 item 6). The
   classification's date is `etp_classified_at`, not part of `etp_source`. A free-text source
   would let any string pass as provenance, so any other value is refused. Adding a source needs a
   new decision.
3. **The value sets are the narrow ones.** `asset_class`, `etp` and `increment` keep the single
   values the E7-7 reader requires.

   `exchange` is `arca` or `nasdaq`: SPY lists on `arca` (the brief's
   [SPY table](../tasks/first-paper-trade.md#spy-specifics), row "Exchange"), and E7-7's AAPL on
   `nasdaq`. This narrows what the E7-7 reader accepts: its `exchanges`
   (`mandate-shell/src/paper/artifacts.rs`) also maps `nyse`, `amex` and `bats`. A registered snapshot naming any of those three is
   refused until a deployment needs it, under its own decision.
4. **Who conforms.** D3's reader of the registered object, and D1's writer, both conform to
   items 1 to 3:
   - D1 refuses to register an object outside them.
   - D3 refuses to build a deployment input from one.
   - Neither reads the other's code to learn the shape; this decision is the contract.
5. **What the members are checked against.** DEC-505 item 1 narrows the effective
   `instrument_snapshot` registration to the latest by `seq` whose object's `instrument_id` is the
   pinned asset id. D3 then requires `symbol` to be the pinned symbol, as the E7-7 reader does.
   The trading gate denies an opening when `etp_classified_at` is absent or older than the rule
   set's `etp_classification_max_age_s` (trading spec §3.2 item 6). This decision changes none of
   those rules.

### Rationale

The brief named the snapshot's members (E7-7's seven plus `etp_source`) but not their types or
value sets, and `etp_source` appeared only by name. Two builders write the two sides, D1 and D3.
Pinning the shape here lets each build to a written contract, and each narrower choice only
refuses more: a free-text source, or an exchange no deployment uses, would have been accepted.

### Alternatives considered

- **`etp_source` as free text.** Rejected: provenance that accepts any string proves nothing.
- **`exchange` as the five values the E7-7 reader maps.** Rejected: no deployment on this path
  needs `nyse`, `amex` or `bats`. Each joins under its own decision when one does.
- **Leave the shape to D1's code.** Rejected: D3 would then read D1's code as its specification,
  and the two could drift.

<a id="dec-525"></a>

## DEC-525: `mandate-cli` moves to layer 9, and may never reach the connector, the shell or a paper binary

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The coordinator chose the route (a), and an agent recorded it (Claude Code, the E10-16 builder), under DEC-79. It tightens: the CLI gains one allowed crate and loses the connector and the shell for good |
| **Story or claim** | D1, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) D1 |

DEC-524 was the highest number on `main` and every branch when this was reserved (DEC-516 item 6).

### Decision

1. **`mandate-cli` moves from layer 7 to layer 9** in `xtask/layers.toml`. D1's `model register`
   must take the content object and its hash from `mandate_modelhost::content()`, so that no
   caller ever supplies a hash. `mandate-modelhost` is layer 8 (DEC-503 item 2), and a crate
   depends only on strictly lower layers, so the CLI must sit above it. Nothing depends on
   `mandate-cli`, so nothing else moves. The host and the shell stay side by side at 8, and the
   host still reaches nothing above it. The `mandate-modelhost` dependency itself arrives with D1,
   the first code that calls it.
2. **The guard: `forbidden_internal`.** Each crate's entry in `xtask/layers.toml` may name workspace
   crates it may never reach, whatever the layers would allow. `mandate-cli`'s list is
   `mandate-alpaca`, `mandate-paper` and `mandate-shell`: the connector, the paper adapter planned
   at layer 9, and the shell. So the founder's CLI stays structurally unable to reach a broker or
   place an order (`AGENTS.md` rules 8 and 12). `cargo xtask layers` (`layer_problems`) enforces
   three rules:
   - **Reach.** A crate may not reach a forbidden crate directly or through any chain of path
     dependencies of any kind (normal, dev and build); `forbidden_reached` reads
     `workspace_closure`, which follows all three.
   - **No path dependency outside the workspace,** of any kind and from any crate. `cargo metadata
     --no-deps` does not walk such a crate, so a chain through it would be invisible (#697's
     review, M1: an excluded `tools/evader` reaching the connector through a CLI dev-dependency).
     A path dependency counts as a member only if its path is the directory of the member it
     names, not by name alone. A crate outside the workspace can take a member's package name
     (`package = "mandate-liquidity"`), so matching names would let it through (#697's review,
     round 2). `git` dependencies, which have no path, are outside `layers`; `deny.toml`'s sources
     policy refuses them in `ci full` (`unknown-git = "deny"`).
   - **Every forbidden name is a workspace member or listed in `planned`,** a new top-level list in
     `layers.toml`, so a misspelt name cannot guard nothing. `mandate-paper` is in `planned`: it is
     covered the day it is created.
3. **Tests** (in `xtask`):
   - `a_crate_that_reaches_a_forbidden_crate_is_a_problem_and_one_that_does_not_is_none` covers a
     forbidden crate reached through a chain, none reached, and an empty list;
   - `the_cli_reaches_no_crate_that_can_place_an_order` pins the CLI's list as written and checks
     the workspace as built against it. It then runs the whole layers check with a stricter policy
     (the CLI forbidden `mandate-journal`, which it does reach) and sees that crate named;
   - `the_closure_follows_normal_dev_and_build_path_dependencies` uses a synthetic `Metadata`;
   - `a_path_dependency_outside_the_workspace_is_a_problem` covers normal, dev and build kinds,
     with a clean workspace that has none;
   - `a_crate_outside_the_workspace_with_a_members_name_is_a_problem` covers a shadow crate of
     each kind, which is refused and not followed as the member;
   - `a_forbidden_name_must_be_a_member_or_planned` covers a misspelt name and a planned one.

### Rationale

The routes considered for `model register`:

- **(a) Move the CLI up.** The hash is computed by the code that runs the model, and the CLI
  cannot be handed a different one.
- **(b) A content file that the host writes and the CLI registers.** The CLI could re-hash the
  file's bytes, but could not tell them from what the host computes. A forged or stale file would
  register, and fail only later at the run's FT-4 check.
- **(c) Move content computation below layer 8.** This changes the pinned content hash, because
  the host's own source is in it (DEC-504). M2b's literal and the content object's `sources` would
  need re-pinning.

Moving the CLI up lets it newly reach `mandate-alpaca` (7) and `mandate-shell` (8). The layers alone
would allow that, so the guard turns "the CLI never places an order" from a convention into a
checked rule (trust ladder rung 2), at the cost of one list in the policy file.

### Alternatives considered

- **Routes (b) and (c).** Rejected for the reasons above.
- **A layer between 8 and 9 for the CLI.** Rejected: the layers are integers, and the guard is
  needed at any layer above 7.
- **Forbid only direct dependencies.** Rejected: the CLI could still reach the connector through
  any crate that does.

<a id="dec-526"></a>

## DEC-526: D1's `config register` and `model register`: exact objects, the host's hash, paper only

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the E10-16 builder), under DEC-79 and DEC-176: each reading below tightens what the brief asks or resolves a gap by the reading that adds no risk |
| **Story or claim** | D1, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) D1 |

DEC-525 was the highest number on `main` and every branch when this was reserved (DEC-516 item 6).

### Decision

1. **Two library functions, then the commands.** D1 adds `mandate_cli::config::register(journal,
   store, owner, kind, object, now)` and `register_model(journal, store, owner, model_id,
   model_version, now)`, tested against `tests/common`'s journal double and an in-memory artifact
   store. The `clap` commands over P0's `--journal` and `--store`, the source of the `Owner` they
   run as, and a binary test against Postgres are a follow-on slice, D1b, so that each tests PR
   stays under 400 lines. `mandate-modelhost` becomes a normal dependency of `mandate-cli` here,
   the first code that calls it (DEC-525 item 1).
2. **Paper only.** Both functions refuse `paper_only` for an owner whose environment is not
   `paper`, before anything else is read, stored or committed. The brief says every command
   refuses `live`; refusing `backtest` too is the tighter reading, since no paper run reads a
   backtest control stream.
3. **The instrument snapshot is exactly DEC-523's object.** It has eight members, each a string,
   and no others: `asset_class` `us_equity`, `etp` `plain`, `etp_classified_at` an instant
   `UtcNanos::parse_rfc3339` reads, `etp_source` `nasdaq_trader_symbol_directory`, `exchange`
   `arca` or `nasdaq`, `increment` `whole`, and `instrument_id` and `symbol` non-empty. Anything
   else is refused `instrument_snapshot_invalid`. That covers a member that is missing, extra or of
   another type, and every other value. Bytes that are not JSON, or JSON that is not an object, are
   refused `config_object_invalid` first, as for every kind (item 4). D3b's reader holds the same
   shape.

   D1 does not normalise `etp_classified_at`: it accepts any instant `parse_rfc3339` reads and stores
   the text as given. A future-dated instant is not refused here. The run refuses it, since the
   paper path's judge requires a classification dated before the run (`mandate-shell`'s
   `paper/judge.rs`). Refusing it at registration too would be a tightening, which D1 does not add.
4. **The other kinds.** The object must be a JSON object, or it is refused
   `config_object_invalid`. A `policy_set` or `model_registry` object's own `kind` must be the
   kind registered, or it is refused `config_kind_mismatch`. That is the check the journal's
   append makes for version 2 (journal spec §11 check 6), so the CLI refuses before storing
   rather than after. The CLI does not read `fee_config`, `trading_calendar` and `rule_set`
   objects further: the runtime reads and refuses them (D3b's fee date), and the CLI duplicating
   those rules would let the two drift. For example, a `fee_config` that names `live` is stored and
   registered here. The run refuses it downstream: its fee configuration goes through
   `paper_only_fee_config` (`mandate-executor`), which refuses any environment but `paper`.
5. **What is stored and committed.** The object is stored in canonical form (journal spec §4).
   `content_hash` is the reference of those bytes, and the envelope's `artifact_refs` lists it.
   The schema version is 2 for `policy_set` and `model_registry` (DEC-484 item 4) and 1 for the
   rest. The payload is exactly §9.2's members, with no `step_up` and no owner's second: a
   registration is not a stepped-up command. Each function refuses first, then decides the event
   id (DEC-290), then stores the object, then commits one `ConfigSnapshotRegistered`. So no
   refusal writes anything, and no committed event names an object that is not stored. A store
   that fails is reported as a journal error, and nothing is appended.
6. **A re-run finds the registration.** The id is derived from the payload and the control head,
   with `Repeat::FindsEarlier`. Registering the same object again, as the stream's last event,
   returns the earlier event and commits nothing (DEC-290 item 5). After any other event, it
   commits a new registration, which the latest-by-`seq` rule (DEC-505 item 1) reads as current.
7. **`model register` takes no hash.** The content object and its hash come only from
   `mandate_modelhost::content(model_id, model_version)`. The function has no hash or content
   parameter. A model the host has no content for (`Refusal::UnknownModel`) is refused
   `model_unknown`; any other refusal of the host keeps its own code, so it never reads as an
   unknown model (#702's review, m7). The payload is kind
   `model_version` at schema version 1, with the model id and version as given, the content hash,
   `params` as the names under the content object's `params_schema.params` sorted, and
   `admits_instruments: false` (DEC-504 item 3).

   "No caller supplies a hash" holds for `model register`. It does not hold for a `model_registry`
   object, which `config register` stores as the owner wrote it. A caller-authored registry that
   names a hash other than the host's is caught at run time: the host refuses the pin with
   `PinHashMismatch` or `RegistryMismatch` (`mandate-modelhost`). DEC-504 item 4 (the hash is the
   hash of the code that runs) is held by M2b's literal pin of `quant.ma_crossover` 1.0.0's hash,
   not by D1.
8. **Implementation shape.** `control.rs` gains a commit for a payload that is exactly the
   owner's choice. It is the same retry loop as `commit`, with the envelope's schema version and
   `artifact_refs` given by the caller. The implementation PR changes the tests file only by
   deleting its `#[ignore]` lines (DEC-77).

### Rationale

The brief asks for exact DEC-523 objects, a hash only from the host, and paper only. Each reading
above is the strict side of a choice the brief left open:

- **Backtest.** It is refused as well as live.
- **Kind mismatch.** It is refused before storing.
- **Re-runs.** They are found rather than committed twice.

The one loosening considered, letting the CLI validate the version-1 kinds, was rejected. It would
add a second reader of those objects, which the runtime already refuses on.

### Alternatives considered

- **Validating the version-1 kinds in the CLI** (fee schedule, calendar, rule set). Rejected: it
  would add a second reader of those objects, which the runtime already refuses on, and the two
  could drift (item 4).
- **Taking a content hash or file for `model register`.** Rejected: DEC-504 item 3 and the brief
  require the hash to come from the code that runs, so the function takes only the id and version
  (item 7, DEC-525).
- **A stepped-up registration (`cli_confirm`).** Rejected: §9.2's payload has no `step_up` member,
  and a registration commits configuration, not a risk-taking act; the confirmation that binds a
  mandate to it is D2's `version confirm`.
- **Committing first and storing after.** Rejected: an event could then name an object that is not
  stored, which the append-time check refuses for version 2 and a reader cannot recover from for
  version 1 (item 5).
- **Refusing `live` only, as the brief says.** Rejected for the tighter paper-only reading
  (item 2).
- **Normalising or bounding `etp_classified_at`** (for example, refusing a future instant at
  registration). Not taken in D1, which adds no feature under review; the run already refuses a
  future-dated classification (item 3).

### Tests

`crates/mandate-cli/tests/config_register.rs`, live since the implementation (#704):

- `an_instrument_snapshot_of_dec_523_is_stored_registered_and_found_on_a_rerun` covers `arca` and
  `nasdaq`, input that is not canonical but is stored canonical, the exact payload, and a re-run;
- `a_snapshot_outside_dec_523_is_refused_before_anything_is_stored` has 26 cases: one extra
  member, each value outside the set, empty ids, a lower-case `t`, a non-instant, and each member
  both removed and given a boolean;
- `the_other_kinds_register_at_their_schema_version_and_a_bad_object_is_refused`, including an
  instrument snapshot that is not an object (`config_object_invalid`);
- `a_model_is_registered_with_the_hosts_content_and_hash_only`, which also refuses an unknown id
  and an unknown version;
- `nothing_is_registered_outside_paper`, for `live` and `backtest`;
- `a_store_that_fails_commits_nothing`, for both commands: each returns the error, and the journal
  sees no append. It pins both the store-before-commit order and that a store failure is not
  swallowed (#702's review, M1).

Every committed draft is read back through `mandate_journal::Draft::parse`, the journal's own
schema check. That check covers §9.2's rules 20 and 21 and the envelope's `artifact_refs`.

<a id="dec-527"></a>

## DEC-527: Who runs `config register` and `model register`: two flags, always paper (D1b)

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The coordinator accepted the builder's proposal (items 1 to 6) and chose item 7's command from the options the builder raised, and an agent recorded both (Claude Code, the E10-16 builder), under DEC-79 and DEC-176. It is an engineering reading for Phase 1 that only tightens: no input can make the CLI's owner anything but `paper` |
| **Story or claim** | D1b, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) D1b |

DEC-526 was the highest number on `main` and every branch when this number was taken (DEC-516
item 6). This file was written whole as `Accepted` in #705's first commit, not first committed as
`Reserved` as the decisions README's step 2 asks. The number was still free on `main` and every
branch when #705 opened.

### Context

`control.rs`'s `Owner` and the M7 brief both say the owner comes from "the CLI's workspace
configuration", and neither says what that is. Today the only `Owner` is built in tests, and
`main.rs` wires no command that needs one. D1b wires the first two: `config register` and
`model register` (DEC-526 item 1).

### Decision

1. **Two flags, both required, with no defaults.** Each command takes `--workspace <id>` and
   `--user <id>` (`OwnerArgs`), beside P0's `--journal` and `--store`. For Phase 1, "the CLI's
   workspace configuration" is the command line.
2. **The environment is never an input.** The binary builds `Environment::Paper` in code. There is
   no `--environment` flag and no file or variable it is read from, so no input can name `live`.
   D1's `paper_only` refusal (DEC-526 item 2) stays as the second guard.
3. **Bad ids are refused before anything is touched,** before the file is read, the journal is
   opened or the store is created:
   - The workspace must make `ctl:{workspace}` a stream id of journal spec §2: one or more ASCII
     letters, digits, `_` or `-`. Otherwise it is refused `owner_workspace_invalid`.
   - The user id is an opaque token: 1 to 64 lower-case ASCII letters, digits, `_` or `-`. Otherwise
     it is refused `owner_user_invalid`. So an email, or a name with a space or a capital, never
     reaches `actor.id` (journal spec §6.4). The CLI cannot tell a lower-case name from an opaque id;
     the founder is told to use an opaque one.
4. **A file the command cannot read** is refused `config_file_unreadable`, before the journal is
   opened. Each refusal message carries its code and never any part of the DSN (`AGENTS.md` rule 7).
5. **What the commands print.** On success each prints one line to standard output, whose words
   include the event id and `seq` it committed, or the ones a re-run found (DEC-290). It prints
   nothing to standard error, no part of the DSN and no object content. A refusal prints nothing to
   standard output.

   A re-run that finds its earlier event returns before it writes the store. So a re-run against a
   different `--store` leaves the object missing from that store. That fails closed: D3b's reader
   refuses a registration whose object is missing (`ObjectMissing`) at run time.
6. **Tests.** The flags are parsed and required, with no `--environment`. Bad ids are refused with
   nothing touched, and a DEC-523 refusal needs no database. These run without a database, and the
   Postgres binary test starts with them (DEC-510 item 3). The binary test registers the SPY
   snapshot and the model in Postgres, finds the model on a re-run, and reads both events and
   objects back.
7. **`mandate workspace open` opens the control stream; nothing else does.** The journal refuses
   any first event that is not `StreamOpened` (journal spec §2), and nothing in the paper path opens
   `ctl:{workspace}`. So on a fresh workspace, the first `config register` would be refused
   `not_stream_opened`. The coordinator chose a separate command, built as its own slice, D1c (a
   tests PR, then its implementation), after D1b:
   - It takes `--workspace`, `--journal` and `--store`. It takes no `--user`, since its event's
     actor is the `control_services` system opener, not the owner.
   - It commits exactly one event: `StreamOpened` on `ctl:{workspace}`, with the payload
     `{"stream_type": "control", "workspace_id": …}` and the opener the journal vectors use, the
     `system` actor `control_services`.
   - It is paper only: the environment is built in code, as item 2.
   - It refuses if the stream already holds any event, so it never appends a second opener.

   This keeps the CLI a one-shot writer (DEC-155 item 5) whose every command commits exactly one
   control-stream event (the brief, "The confirmed mandate"; `control.rs`). Until D1c
   lands, the D1b binary test opens `ctl:ws1` itself with the same draft, as a stand-in.
8. **D1c's readings** (the tests PR for item 7):
   - **Refusal codes.** A bad workspace is refused `owner_workspace_invalid` by item 3's rule,
     before the journal is opened or the store created. A stream holding any event is refused
     `workspace_already_open`, before anything is appended. Neither refusal prints anything.
   - **The event.** The envelope's actor is `{"kind": "system", "id": "control_services"}`, with
     the CLI's own build and version. The environment is `paper`, and the payload is exactly
     `stream_type` and `workspace_id`. The event id is derived like every control-stream id
     (DEC-290), from the payload at head 0. So two runs that race on an empty stream draft the
     same event, and the journal stores it once.
   - **Output.** It prints one line, whose words include the event id and `seq` 1, and no part of
     the DSN.
   - **Tests.** `crates/mandate-cli/tests/workspace_open.rs`:
     - the flags are a live test, with `--user` and `--environment` as unknown arguments;
     - the refusals without a database are pending;
     - the Postgres binary test is pending. It opens `ctl:ws1`, checks the event field by field,
       is refused on a second run with nothing appended, and opens a second workspace.

### Rationale

The flags are what `mandate-tracer` already takes (`--workspace`), they need no new file format,
and a test can drive them. Building the environment in code means a live owner is unrepresentable
at the edge, not just refused, which is the higher rung of the trust ladder.

### Alternatives

- **A workspace configuration file (`--config`).** This matches the M7 wording more closely, but it
  is a new format to specify, and a file's environment field would itself need refusing. It comes
  with M8 identity (E9-1, E9-4), when the user id comes from an identity provider rather than the
  command line.
- **Environment variables.** Rejected as hidden state: the command line would not show who a
  command runs as.
- **`config register` opening an empty stream in the same append** (a batch of `StreamOpened` and
  the registration). Rejected: the command would then commit two events, against the one-event rule above,
  and every other command would need the same rule (item 7).

<a id="dec-528"></a>

## DEC-528: No order ticket; display-only instrument search and owner watchlists; adoption of existing holdings deferred

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The founder, in session on 2026-10-08; recorded by an agent (Claude Code, the coordinator's builder) |
| **Story or claim** | No claim issue covers this docs-only change; it records the founder's 2026-10-08 session. New rows E10-19, E11-10, E11-11 and E19-12, and the adoption row under "Later" in the [backlog](../06-backlog-v1.md) |

When this number was reserved, DEC-526 was held on #702 (since merged) and 527 had been left to
the D1b builder; 528 was free on `main` and every branch (DEC-516 item 6).

### Context

The founder described the flow they want:
- **Arriving.** An owner arrives with several connected accounts and sees that no agents are
  deployed.
- **Finding an instrument.** They find it by name, ticker or list.
- **Acting.** They either buy, or start an agent that works toward a goal or only monitors. They may
  also want an agent over what they already hold.
- **What agents do.** Agents react to market events (news, earnings dates and calls, filings) and
  run until a condition. Before acting, an agent goes thesis → research → risk → decision.

Mapped against the documents, three points needed a decision. The rest are new stories that conflict
with no accepted decision. The founder agreed with the five outcomes below; sub-items marked
*proposed* are the recording agent's reversible proposals, not founder decisions.

### Decision

1. **Manual buying: no order ticket.** All buying goes through an agent:
   - an owner request to an agent the owner already has (DEC-141, chat and connected clients), or
   - setting up a new agent.

   The [product experience brief](../../product/09-product-experience.md) §5's Rule 12 row stands
   unchanged ("No order ticket and no direct broker action anywhere"). The flow on Home (signed-in,
   D1) reads "search → open an instrument → ask an agent, or set up an agent". No screen offers
   "buy". *Proposed:* the spec changes this needs are those under "Follow-up spec changes" below.
2. **Instrument search is display-only, and watchlists are the owner's.**
   - **Search.** An owner may search instruments by name or ticker.
   - **Watchlists.** An owner may keep watchlists curated on the platform's side. *Proposed:* they
     are stored in the workspace; whether watchlist edits are journaled stays open, as E11-11 says.
   - **What the platform never adds.** No ranking, scoring or flagging of instruments, no
     "interesting", "trending" or "popular" list, and no list or suggestion the platform authors.
   - **Result order** (*proposed*). Search results are ordered by how they match the query text,
     then alphabetically, never by any market measure.
   - **The data plane.** [Data-plane spec §1.4](../../specs/data-plane.md#14-non-goals) still holds:
     the data plane never ranks, scores or flags an instrument as interesting. Screens remain
     owner-configured filters that feed the research agent.
   - **The broker's lists.** The Robinhood connector still calls no watchlist or scan tool
     ([connections spec §6.2](../../specs/connections.md#62-how-mcp-maps-to-the-connector-interface)
     rule 2).
   - **What a list is not.** A watchlist is not an envelope field. A watchlist steers no agent's
     universe and is never read by an agent.
3. **Agents over existing holdings (adoption) are deferred.** DEC-46 stands: an agent's sub-ledger
   holds only what that agent bought. Adoption waits until cost basis and tax lots are modelled.
   *Proposed* prerequisites:
   - importing lots and their basis from the broker, with the source of each recorded;
   - wash-sale handling across the adopted lots and the agent's own trades;
   - how an adopted position interacts with
     [trading-domain spec §7.1](../../specs/trading-domain.md#71-account-ledger-dec-26)'s rule
     that activity the platform did not originate is external activity, which puts the account's
     agents in `exits_only`;
   - how it interacts with [connections spec](../../specs/connections.md) CN-7 (such activity is
     never adopted as ours).

   The backlog's "Later" section carries the row.
4. **New stories, in no conflict with an accepted decision.** Each comes after the first paper trade
   and changes nothing in the tracker's current "Next, in order":
   - **Monitor-only agents (E10-19).** A monitor agent watches and alerts, and never opens a
     position: its autonomy denies every opening. Its notifications carry only opaque IDs and
     generic text (`AGENTS.md` rule 6). *Proposed:* placed in Phase 3, unless pulled forward once
     its mandate spec change lands.
   - **Home (signed-in, D1) and a cross-account holdings view (E11-10).**
     - The view is read-only across the owner's connections, and per connection it shows only that
       connection's own account (CN-8; for Robinhood, the agentic account only).
     - A holding the platform did not buy is never shown as an agent's or adopted (CN-7).
     - Home shows the connected accounts, their holdings, and "no agents deployed".
   - **Instrument search and owner watchlists (E11-11)**, as item 2 says.
   - **Event-triggered research (E19-12).** News, filings, price moves, and earnings dates and call
     transcripts may bring a research run forward. Event sources, debounce and caps are envelope
     fields under `behavior.cadence` ([mandate spec](../../specs/mandate.md)); no event triggers a
     run unless a confirmed mandate version enables it (rule 11), and a change that adds event
     sources classifies as increasing. It needs:
     - debouncing, and rate and cost caps;
     - DEC-101's source allowlist and corroboration;
     - the prompt-injection defences;
     - a source evaluation for an earnings calendar and transcripts.

     Research still produces opinions only (rule 4). *Proposed:* placed after the research agent's
     DEC-99 evaluation passes.
5. **Unchanged.**
   - Robinhood stays Phase 2 (agentic MCP, the agentic account only).
   - What an agent may trade stays v1's scope: long-only US stocks, ETFs and crypto spot. Options,
     short sales, margin and perpetuals stay in their later phases.
   - Live trading stays gated on counsel (DEC-98).

### Follow-up spec changes

*Proposed.* These are protected paths, so this decision edits none of them. Each needs its own
spec-change PR before its story's tests:

- **Agent harness spec §5.3** (E19-12). Today a research run becomes `Due` only when the risk clock
  reaches `next_proposal_at`, and invariant HI-8 says no run starts before it. An event trigger
  needs a second edge, with its debounce and caps, and HI-8 must be amended so that a trigger the
  confirmed mandate enables is the only other way a run starts, never bypassing the run's
  preconditions.
- **Mandate spec** (E10-19, E19-12). How a monitor agent is expressed: either an envelope with no
  allocation, or autonomy that denies `open` and `increase`. Also which validation rules and
  reference cases change, and what it alerts on. For event triggers, the event sources, debounce
  and caps under `behavior.cadence`.
- **Workspace API spec** (E11-10, E11-11). The holdings, search and watchlist routes, with their
  read models served from the journal (API-14).
- **Data-plane spec** (E11-11, E19-12). The instrument directory search reads, and the earnings
  calendar and transcript sources once evaluated. §1.4 needs no change for display-only search,
  because it ranks nothing.

### Rationale

1. **No order ticket.** An order ticket would be a path around the mandate, the builder and the
   autonomy rules. Every buy through an agent keeps rules 1, 4 and 12 whole, and DEC-141 already
   gives the owner a way to ask.
2. **Search and watchlists.** Finding an instrument by its name, and keeping one's own list, is
   display and the owner's own choice. Ranking or suggesting would be the platform's voice, which
   [data-plane spec §1.4](../../specs/data-plane.md#14-non-goals) and the product experience
   brief's P3 already exclude. A list no agent reads cannot become a universe hint.
3. **Adoption.** Without lots and basis, an adopted position's P&L, its limits and its wash-sale
   exposure cannot be computed correctly, and today the specs read such holdings as external
   activity.

### Alternatives considered

- **An order ticket for manual buys.** Rejected: it bypasses the agent path and the rules above.
- **Platform-suggested lists** (trending, popular, interesting). Rejected: platform-authored
  suggestions, which §1.4 excludes.
- **Watchlists stored at Robinhood through its watchlist tools.** Rejected: connections spec §6.2
  rule 2 allowlists no such tool, and the lists would leave the workspace.
- **A watchlist an owner may pin into a mandate.** Rejected in review: a list that can feed an
  agent's universe is an envelope input by another name. An instrument enters an agent only through
  a mode the owner already confirms in a mandate version.
- **Adoption now, with basis taken at the current mark.** Rejected: it misstates P&L and wash-sale
  exposure, and it contradicts CN-7.

### Not decided here

No legal or compliance wording. Whether counsel should review the following is listed for the
founder in this decision's PR, and the [compliance document](../../product/08-compliance-and-regulatory.md)
is not edited:
- display-only search and owner watchlists;
- a monitor agent's alerts;
- event-triggered research (for example, compliance questions 26 and 31).

<a id="dec-530"></a>

## DEC-530: D2's `version create`, `version confirm` and `agent deploy`: records, codes, checks, and a three-way split

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | An agent (Claude Code, the E10-16 D2 builder), under DEC-79 and DEC-176: each reading below tightens what the brief and the specs ask, or resolves a gap by the reading that adds no risk |
| **Story or claim** | D2, E10-16; claim [#171](https://github.com/kunwarshivam/mandate/issues/171); [first paper trade brief](../tasks/first-paper-trade.md) D2 |

DEC-528 was the highest number on `main` and every branch when the coordinator reserved this one for D2 (DEC-516 item 6).

### Context

The brief's D2 adds the three commands that make a confirmed, deployed version on the workspace
control stream, the input D3 folds (DEC-505). Journal spec §9.2 closes the three payloads and
leaves the rest of each mandate spec §10 row to a stored record named by `record_ref`. Nothing says
what the CLI's records hold, what a `cli_confirm` code is bound to here, which V-rules the CLI can
check before the run, or what a re-run finds.

### Decision

1. **Three slices.** Formatted, the tests for `version create` and `version confirm` alone are
   about 350 lines, so D2 splits before review, as the brief requires of a slice over 400:
   - **D2a:** `mandate_cli::version::create(journal, store, owner, document, now)` and
     `confirm(journal, store, ids, owner, version, code, now)`, library functions tested against
     `tests/common`'s journal double and an in-memory store.
   - **D2b:** `agent deploy` as a library function, item 9.
   - **D2c:** the `clap` commands `mandate version create <file>`, `mandate version confirm`, and
     `mandate agent deploy`, over DEC-527's `--workspace` and `--user` and P0's `--journal` and
     `--store`. It adds the read-only function that shows a gesture's code and warnings before the
     owner types it, and a binary test against Postgres. Its refusals carry their code and never
     any part of the DSN, as DEC-527 item 4 says.
2. **Paper only, twice.** Each function refuses `paper_only` for an owner not in `paper` before
   reading anything (DEC-526 item 2, so `backtest` too). `create` refuses `mandate_not_paper` for a
   document whose `environment` is not `paper`: DEC-505 item 2 refuses `live`, and the schema
   admits nothing else.
3. **`version create`.** The bytes must be JSON that `Mandate::parse` reads (the schema), or they
   are refused `mandate_invalid`. The version is `sha256:` and the SHA-256 of the mandate's
   canonical bytes (mandate spec §9.1). The command stores the canonical document and then the
   version record, and commits one `MandateVersionCreated`:
   - **Payload:** `mandate_version`; `provenance`, one `{path, source: "user_entered"}` for every
     top-level member of the document but the system fields `mandate_schema_version` and
     `source_text_ref`, ascending, since the founder wrote the file (brief, "The confirmed
     mandate"); and `record_ref`.
   - **Envelope:** schema version 1, `config_refs` empty, and `artifact_refs` the version and the
     record's reference, ascending, as the journal vectors' control stream has them.
   - **Record:** exactly `{"kind": "mandate_version_record", "mandate_version", "user"}`. Mandate
     spec §10's other contents describe a compiler, a template, a policy set and a previous version,
     none of which this path has. The validation results and warnings are recorded at confirmation
     (item 6), where they are checked.
4. **The codes.** A code is the first eight hex digits of the SHA-256 of a canonical object, the
   shape every `cli_confirm` code takes (DEC-155 items 3 and 4):
   - `version confirm`: `{"gesture": "mandate_confirm", "mandate_version": V}`;
   - `agent deploy`: `{"agent_id": A, "gesture": "agent_deploy", "mandate_version": V}`.

   So a code confirms exactly the version shown, and a confirmation code never deploys. The
   gesture's step-up evidence, `{assertion_id, authenticated_at, method: "cli_confirm"}`, uses the
   risk-clock second as `OwnerCommandIssued`'s does. It goes in the record, because §9.2's payloads
   are closed and carry none. A fresh assertion id is minted only after every check has passed, so a
   refusal or a re-run spends none.
5. **What `version confirm` checks, in order,** each a refusal that writes nothing:
   1. `paper_only`.
   2. `version_unknown`: no `MandateVersionCreated` on the stream names the version.
   3. `document_missing`, or `document_corrupt` for stored bytes that do not re-hash or are not a
      mandate.
   4. `code_mismatch`.
   5. `control_stream_invalid`: a control-stream record `JournaledFact::from_record` cannot map, or
      a registered instrument snapshot whose object cannot be read. The CLI refuses rather than
      skipping, as the fold does.
   6. `version_invalid`: any V-rule but V-002, in the context `ValidationContext::from_journal`
      folds from the stream's facts plus the confirmation being made. The context:
      - **Agent:** none, so every deployed agent's allocation and claims count as another's.
      - **Connection:** the document's.
      - **Validation date:** the UTC date of the command, never earlier than New York's.
      - **Membership:** one user and one approver (DEC-505 item 3).
      - **`independent_approval_required`:** `false`. The CLI reads no policy set, and the run's
        validation is the authority (D3); nothing trades on a confirmation alone.
      - **Connection's environment:** taken as matching only when the stream holds no
        `ConnectionEstablished` or `ConnectionRevoked` for the connection (DEC-505 item 3's
        reading). Otherwise the fold's refusing value is kept.

      V-002 is left out because it needs the broker equity the run's preflight reads (DEC-505
      item 3), which no record carries.
   7. `instrument_unregistered`: a pinned instrument that the latest `instrument_snapshot`
      registration naming its asset id does not name with the pinned symbol, or that no
      registration names (DEC-505 item 1, DEC-523 item 5).
6. **What `version confirm` writes.** The record is exactly `{"kind":
   "mandate_confirmation_record", "mandate_version", "step_up", "user", "warnings"}`, where
   `warnings` are the mandate spec §4.2 codes the confirmation acknowledges, ascending. The command
   then commits one `MandateConfirmed` whose `confirmed_paths` are the envelope paths of item 3,
   with the envelope of item 3. D2c prints the summary from the stored document and this record,
   so the record does not store the printed text.
7. **Re-runs commit nothing.** The CLI reads the facts on the stream rather than DEC-290 item 5's
   last-event rule, since each of these records states a fact once rather than a command a runtime
   copies:
   - `create` of a version any `MandateVersionCreated` names returns that event.
   - `confirm` returns the stream's latest `MandateConfirmed` when it names the version. Confirming
     a version after another version's confirmation commits anew, and makes it the latest confirmed
     version, which item 9 requires.
8. **Order.** Every refusal comes first. Then the event id is derived (DEC-290), the document
   (`create`) and the record are stored, and one event is committed through DEC-526 item 8's
   commit. A store that fails is a journal error, and nothing is appended.
9. **`agent deploy` (D2b)** takes the agent, the version and its code, and checks, in order:
   1. `paper_only`.
   2. `agent_invalid`, for an id that does not make `agent:{workspace}:{agent}` a stream id.
   3. `version_unknown`, then `document_missing` and `document_corrupt`.
   4. `version_unconfirmed`: no `MandateConfirmed` names the version.
   5. `version_superseded`: its latest confirmation is not the stream's latest `MandateConfirmed`.
   6. `code_mismatch`.
   7. `agent_active`: the agent's latest `AgentDeployed`, with no `AgentStopped` after it, names
      another version. Changing a running agent's version is mandate spec §2.2's application, the
      runtime's `MandateVersionApplied`, never a second deployment.
   8. `control_stream_invalid`, `version_invalid` and `instrument_unregistered` as item 5, with the
      agent named, so V-006 sees the other agents' claims and V-031 the agent's version in force.

   Its record is exactly `{"agent_id", "kind": "deployment_record", "mandate_version", "step_up",
   "user"}`. Its `AgentDeployed` has payload `{agent_id, mandate_version, record_ref}`,
   `config_refs` `{"mandate_version": V}` (§9.2 rule 22), and `artifact_refs` as in item 3. A re-run
   while the agent's active deployment is the version returns that event.
10. **The refusal codes are closed:** `paper_only`, `mandate_invalid`, `mandate_not_paper`,
    `version_unknown`, `document_missing`, `document_corrupt`, `code_mismatch`,
    `control_stream_invalid`, `version_invalid` and `instrument_unregistered` (D2a), and
    `agent_invalid`, `version_unconfirmed`, `version_superseded` and `agent_active` (D2b). Adding
    one needs a new decision.
11. **Not written by D2.** `ConnectionEstablished` (DEC-505 item 3), account snapshots, and policy
    sets.

### Rationale

Each choice is the narrower one:

- **Refusing more than the brief names:** `backtest` owners, a non-paper document, a version that
  fails a V-rule before the run sees it, and a symbol the registered snapshot does not carry.
- **Codes:** they bind one gesture to one version.
- **Records:** they hold only what happened.

The one value the CLI cannot know, the broker's equity for V-002, is left to the run that reads it,
as DEC-505 already rules. The policy's `independent_approval_required` is read as `false` only for
a check whose authority is the run's, so no order depends on it.

### Alternatives considered

- **D2 as one slice.** Rejected for size (item 1).
- **Step-up evidence in the payload, as `OwnerCommandIssued` carries it.** Rejected: §9.2's schemas
  are closed, and a draft with a `step_up` member is refused.
- **A confirmation code bound to the control stream's head**, as a resume's is. Rejected: a
  confirmation is about a document, not the stream's state, so a re-run after an unrelated
  registration would need a new code for the same version.
- **Checking V-002 against equity typed on the command line.** Rejected: the CLI would assert a
  broker fact it never read.

### Tests

D2a: `crates/mandate-cli/tests/version.rs`, pending E10-16 until the implementation PR. The
implementation changes it only by deleting its `#[ignore]` lines (DEC-77).

- `a_version_is_its_stored_document_created_with_every_path_user_entered`: non-canonical input
  stored canonical under its version, the exact payload, record and envelope, and a re-run after
  another event finding the first.
- `a_confirmation_is_one_event_bound_to_the_version_shown_that_the_spec_fold_reads`: the exact
  payload and record, with W-002 and the first assertion, and a re-run spending nothing. Another
  connection's revocation does not refuse it, since only facts about its own connection count.
  After another version is created and confirmed, confirming the first again commits one new event,
  which becomes the latest confirmation (item 7). The
  stream then folds with `JournaledFact::from_record` and `ValidationContext::from_journal`, the
  functions D3 builds the run's input from (`mandate-cli` may not depend on `mandate-shell`), and
  only V-001 and V-002 remain, the two facts the run supplies, so V-002 alone did not refuse the
  confirmation.
- `every_refusal_has_its_own_code_and_writes_nothing`: twenty-four cases covering D2a's ten codes,
  each leaving the journal, the store and the assertion count unchanged. Among them:
  - a code for another version;
  - a stored document replaced by another, valid mandate (`document_corrupt`, so the read must
    re-hash);
  - a snapshot with another symbol, and one for another asset id;
  - a sweep of `version_invalid`, one case per rule, each failing that rule and V-002 only: V-001
    (revoked connection), V-006 (another deployed agent claims SPY), V-007 (unregistered model
    version), V-008, V-012, V-013, V-014, V-016, V-030, V-032 (a retired agent's loss carry) and
    V-034.
- `a_store_that_fails_commits_nothing`: a store that fails at each write in turn, the document's
  and then the record's for `create` and the record's for `confirm`, commits nothing. So the order,
  every object stored before the commit, is pinned as well as the failure.

D2b: `crates/mandate-cli/tests/deploy.rs`, over a control stream the tests seed in §9.2's shapes:

- `a_deployment_is_one_agent_deployed_naming_the_version_and_its_record`: the exact payload,
  `config_refs.mandate_version` (rule 22), `artifact_refs` and record, and a re-run after another
  event finding the first and spending no assertion.
- `the_spec_fold_reads_the_deployment_as_the_agents_version`: `AgentVersionActive` for the agent
  and version, and only V-001 and V-002 remaining.
- `a_stopped_agent_deploys_again_and_a_reconfirmed_version_is_the_latest`: a stop ends the
  deployment; a superseded version is refused until it is confirmed again.
- `every_refusal_has_its_own_code_and_writes_nothing`: item 9's twelve codes, each writing nothing,
  including another agent's stop leaving the deployment active, and two cases pinning the order
  where two checks fail: `version_superseded` before `code_mismatch`, and `code_mismatch` before
  `agent_active`.
- `a_store_that_fails_commits_nothing`.

Payloads, records and codes are written out from the journal vectors' shapes and this decision,
and every committed draft is read back through `mandate_journal::Draft::parse`.

<a id="dec-532"></a>

## DEC-532: A working bracket's held legs in protection sizing (E1's tests), and the cancel-wait oracle's overdue reading

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The E7-4 stream K builder (Claude Code), under DEC-79 (reversible engineering) and DEC-176 (item 1 enforces rule 12's Σ protective sells ≤ position and adds no risk; item 3 reads an oracle as DEC-160 (13), (18) already rule). The coordinator assigned the number and accepted item 3's oracle fix in #668 (73ecacf) |
| **Story or claim** | E7-4 defect E1, claim [#137](https://github.com/kunwarshivam/mandate/issues/137); stacked on [#668](https://github.com/kunwarshivam/mandate/pull/668) |

### Context

DEC-521 item 2 found that held bracket legs are counted by no cap, and #689's two reviews traced
that root cause, E1's, into four protection-sizing paths in `protection.rs`:
- `re_place`, reached from `settle` → `replace` after an exit sequence;
- `re_place`, reached from `new_day`'s re-placement before expiry, through `replacements`;
- `passive_exit`'s rest OCO;
- `re_cover` → `re_place`.

Each sizes on `long`, the position as the broker holds it. That includes a working bracket entry's
filled shares, while `covered` counts no held legs. This DEC-77 tests change pins the four paths;
the fix is the next PR.

### Decision

1. **A working bracket's filled shares are its held legs' to cover.** A bracket entry that is not
   terminal, has fills, and whose legs are not yet recorded `placed` holds legs for its whole
   quantity Q1 (trading spec §5.4: "Bracket legs are held until the entry is completely filled").
   Its filled quantity f1 is therefore spoken for:
   - if it completes, its legs sell Q1 against the Q1 its fills add;
   - if it ends partly filled, its OCO sells f1 (DEC-346 item 6).

   Protection sized for the rest of the position covers the position less every such entry's f1,
   less what live protection and working exits already cover. Sizing on the whole position covers
   B + f1, and once the bracket completes the sells are B + f1 + Q1 against a position of B + Q1:
   an oversell of f1 that nothing later corrects (rule 12; `AGENTS.md` rule 12).

   Meanwhile the f1 shares sit in the bracket's own unprotected interval. That interval is
   journaled, and bounded by `bracket_partial_fill_timeout`, after which the remainder is cancelled
   and the OCO placed, and by `max_unprotected_s` (§5.4). Leaving them to it adds no risk the spec
   has not accepted. Covering them twice is what makes a short possible.
2. **The four pending hand cases.** Each starts from the protected 10 (an OCO for 10) and a bracket
   of 10 holding 4. Each asserts the quantity re-placed, against what the code sizes today:

   | Case | Path | Expected | Code today |
   |---|---|---|---|
   | `an_exits_re_placement_leaves_a_held_brackets_shares_to_its_legs` | another agent's bracket; a risk exit of 2 fills, then `replace` | 8 | 12 |
   | `a_re_placement_before_expiry_leaves_a_held_brackets_shares_to_its_legs` | the agent's own bracket still working; the buffer day's re-placement is confirmed | 10 | 14 |
   | `a_passive_exits_rest_leaves_a_held_brackets_shares_to_its_legs` | another agent's bracket; this agent's passive exit of 3; the rest OCO | 7 | 11 |
   | `a_re_cover_leaves_a_held_brackets_shares_to_its_legs` | a completed bracket of 2 rests its legs; the broker cancels those legs | 2 | 6 |

   Each expected number is the position outside the held bracket, less what still rests and what
   exits sell. Each code-today number is 4 more, the held f1.

   A fifth, live case pins the boundary: `a_terminal_brackets_placed_oco_is_not_subtracted_again`.
   Another agent's bracket that ended partly filled (4 of 10) already has its OCO for the 4, so a
   risk exit's re-placement covers all 12 left. It passes today, and fails (8 against 12) with a
   fix that subtracts every bracket's fills rather than only held ones.

   None of the four cases reaches a stub, so each is pending under a `BEHAVIOUR_ONLY_TESTS` row
   citing backlog E1. The prepared fix, which subtracts held brackets' f1 from `long` at the three
   sizing sites, passes all five, so the expectations are reachable. It is not part of this change.
3. **The cancel-wait oracle's overdue reading.** `no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding`
   failed CI's unmutated baseline on #668 at 4a3be70 (mutants shard 26/192).

   What the executor does, the same on `main`:
   1. An exit waits on an unacknowledged opening.
   2. Rule 5's bound marks that opening's wait overdue (`cancel_overdue`, state `unknown`) before
      any cancel can be asked, since an unacknowledged order is queried, never cancelled blind.
   3. Once the broker acknowledges the opening, the executor asks its cancel and submits the exit in
      the same step.

   DEC-160 (7), (13), (18) rule exactly this: an exit never waits on an opening whose wait went
   overdue, "answered or not, and never twice on the same submission of it". The oracle, as
   DEC-506 item 7 wrote it, honoured only an overdue journaled *after* the cancel.

   The reading #668 took (73ecacf): a buy submission whose wait was journaled overdue holds no sell
   through a cancel asked later for it, and a new `OrderSubmitted` for that id starts the wait
   afresh. Protective cancels are unchanged. The reading follows the accepted decision and does not
   loosen the spec.

   Evidence. Before the reading, seeds 17, 32 and 48 of 1 to 60 failed on this overdue shape in
   CPHC. They no longer fail on it. Since #668's round 2 the property leads every script with
   E5b's shape (`STEP_BESIDE_LEAD`) and is pending under its `BEHAVIOUR_ONLY_TESTS` row, so on
   `main` it fails at every seed, those three included, on E5b: an exit submitted while another
   exit's unacknowledged rung has a step cancel outstanding (backlog row "E7-4 (stream K), E5b
   from #668's round-2 review"). It goes live with E5b's fix.

   Scope. The opening in the overdue shape is a plain buy, so the exit beside its pending cancel
   oversells nothing. A bracket entry's overdue cancel is E5's (its legs may still activate,
   which no cap counts), and the reading exempts it from the wait too, as DEC-160's carve-out
   reads; E5 tracks that hazard.
4. **What this leaves.** E1's original case, an exit submitted beside a just-activated bracket's
   legs, stays pinned by `protective_sell_quantity_never_exceeds_the_position_in_any_script`
   under its row. E1's fix PR takes the four cases live, deletes their rows, and fixes that case
   too. E5, an overdue cancel of a bracket entry, is separate and awaits the coordinator.

<a id="dec-538"></a>

## DEC-538: size the mutation matrix to the diff; the gate's mutants and each shard's load stay the same

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-08 (UTC) |
| **Decided by** | The DEC-538 builder (agent), under DEC-79: a reversible engineering and process decision about CI scheduling that removes no mutant, test, cap, or check; CI and xtask are founder-owned in `CODEOWNERS`, so the founder may overturn it |
| **Story or claim** | CI queue correction requested by the coordinator; on [DEC-498](earlier-303-599.md#dec-498) items 1 and 7, [DEC-464](earlier-303-599.md#dec-464)'s ten-minute limit, [DEC-519](earlier-303-599.md#dec-519), [DEC-112](../04-decision-log.md#decisions), [DEC-137](../04-decision-log.md#decisions) |

### Decision

1. **A plan job sizes the mutation matrix.** `cargo xtask ci mutants --plan` computes, without
   building, the mutants the gate would test on the PR's diff, and prints `mutants=<n>`,
   `shards=[0, …, N-1]` and `total=<N>` for `GITHUB_OUTPUT`, where `N = min(192, n)`. CI's new
   `mutants-plan` job runs it, and the `mutants` matrix is `fromJSON` of its `shards`, each shard
   running with `MANDATE_MUTANT_SHARD=k/N`.
2. **The plan and the gate share one code path.** Both go through `gate_diff` (the base, the
   safety-critical crates, the changed `.rs` files under their `src/`, the diff file) and
   `gate_mutants` (`cargo mutants --list --json --in-diff`, which reads the same
   `.cargo/mutants.toml` as the run). DEC-137's stub bodies are not filtered out of either: their
   mutants are tested, and only a miss in one is exempted afterwards, so they count toward the
   plan and toward a shard's load exactly as they did.
3. **A shard checks its plan.** CI passes the plan's count to every shard as
   `MANDATE_MUTANTS_PLANNED`. A shard whose own listing has a different count, or whose shard total
   is not `min(192, planned)`, fails before it builds anything (`check_schedule`). Without the
   variable, as in `cargo xtask check`, nothing changes.
4. **The gate's mutant set is unchanged, and so is each shard's load.** cargo-mutants' `slice`
   sharding gives shard `k` of `N` the `k`-th run of `ceil(n / N)` listed mutants. For `n ≤ 192`
   that run is one mutant at width `n` and at width 192; for `n > 192` the width is 192 as before.
   So shard `k` of the plan tests exactly what shard `k` of 192 tested, every shard the plan drops
   was one that tested nothing, and the shards together test the same set. DEC-498's caps, its
   one-worker setting, and its budget of 420 capped seconds a shard are untouched.
5. **A plan of zero launches no shard.** The `mutants` job is skipped when the plan reports `0`,
   which happens exactly when every shard would have reported nothing to test: no base, no
   safety-critical source changed, or changed source that generates no mutant (a tests-only or
   `#[cfg(test)]`-only change). A documentation-only change (DEC-112) is classified in the plan job
   and reports `0` without a toolchain.
6. **`full` accepts that skip only beside a plan of zero.** The required `full` job now needs
   `full-checks`, `mutants-plan` and `mutants`. It passes only when `full-checks` and the plan
   succeeded, and the matrix succeeded for a plan of one mutant or more, or was skipped for a plan
   of zero. A failed or cancelled plan, a count that is not a number, a skipped matrix beside a
   plan that counted mutants, and a matrix that ran beside a plan of zero all fail it. The required
   check names stay `fast` and `full`, so branch protection needs no change.
7. **DEC-519 holds for every shard that runs:** the same digest-pinned PostgreSQL before the gate,
   `MANDATE_PG_URL` and `MANDATE_PG_REQUIRED=1`, and no debuginfo. The plan job needs no database
   and builds nothing, so it has neither.

This is consistent with DEC-464 and DEC-498 item 8, "add shards, never remove mutants": no
mutant leaves the gate and no shard gets more of them. Only runners that had nothing to test are
no longer started.

### Rationale

Every non-documentation PR started all 192 shards, and each one checked out the repository, started
Postgres, installed nextest and cargo-mutants, and restored the cache before learning whether its
slice held anything. A PR with three mutants used three of them; a tests-only PR used none. The
idle runners filled the queue every other PR waits in.

Runners each kind of PR starts for the mutation gate, before and after (the plan is one extra
runner; `fast`, `full-checks` and `full` are unchanged):

| PR | Mutants | Before | After |
|---|---|---|---|
| Documentation only | 0 | 192 (each on the docs-only path) | 1 (the plan, no toolchain) |
| Tests only, or no safety-critical source | 0 | 192 | 1 |
| A ten-mutant diff | 10 | 192 | 11 |
| A five-hundred-mutant diff | 500 | 192 | 193 |

Two recent PRs, replanned against their bases: E7-19 (#729) lists 3 mutants and E7-16 (#715) lists
4, so each now starts 4 and 5 runners where it started 192.

#### What holds it

xtask tests, each shown to fail on a seeded bug:

- `the_planned_shards_test_the_gates_mutants_and_no_more_per_shard` writes its own diff in a fixture
  repository and asks `cargo mutants --list --shard k/N --sharding slice` itself, for every `k` at
  `N = 192` and at the plan's `N`: the plan counts the tool's listing, shard `k` of the plan equals
  shard `k` of 192, the dropped shards are empty, the union is the whole listing, and a shard
  whose count or total disagrees with the plan is refused before the run. Seeded: a plan that
  counts nothing, and the shard's schedule check removed.
- `no_planned_shard_carries_more_than_its_slice_of_192` compares the slices for every `n` from 0 to
  1000. Seeded: two mutants a shard.
- `the_plan_counts_zero_exactly_when_the_gate_has_nothing_to_test` covers no base, a tests-only
  change, and a `#[cfg(test)]`-only source change, and a change that does generate a mutant.
- `full_accepts_a_skipped_matrix_only_beside_a_plan_of_zero` runs `full`'s script from `ci.yml`
  under `bash -e` over all 512 combinations of the three results and eight counts, against a truth
  table written in the test. Seeded: the zero branch accepting any result.
- `ci_sizes_the_mutation_matrix_from_the_plan`, `a_shards_schedule_must_match_its_plan`, and
  `the_plan_sizes_the_matrix_to_the_diff` pin the workflow wiring, the shard check, and the
  outputs.

### Alternatives considered

- **Skip only the docs-only and zero-mutant cases, keeping 192 otherwise.** It removes most idle
  runners, but a three-mutant diff still starts 189 that test nothing.
- **Fewer, wider shards (`N` below `min(192, n)`).** It would put more than one mutant in a shard
  for some diffs, against DEC-498's budget. Rejected; widening is never this decision's lever.
- **Let each shard exit early when its slice is empty.** It still starts the runner, so the queue
  is the same.
