# Backlog: v1

| | |
|---|---|
| **Owner** | Product and project management |
| **Status** | Draft v0.1 |

Epics map to [milestones](02-milestones-and-wbs.md) and [PRD](../product/04-prd-v1.md)
requirements. Priority uses MoSCoW: **Must**, **Should**, **Could**, **Won't (v1)**.
Stories follow "As a … I want … so that …" with acceptance criteria.

## Epic overview

| Epic | Milestone | PRD | Priority |
|---|---|---|---|
| E1 Foundations | M0 | — | Must |
| E2 Market data | M1 | 6.4 | Must |
| E3 Accounting | M2 | 6.4 | Must |
| E4 Simulated execution and backtest | M3 | 6.4 | Must |
| E5 Journal | M4 | 6.7 | Must |
| E6 Agent runtime and risk | M5 | 6.5 | Must |
| E7 Alpaca connector and recovery | M6, M8 | 6.2, 6.5 | Must |
| E8 Escalation and approvals | M7, M10 | 6.6 | Must |
| E9 Identity, tenancy, and policy | M8 | 6.1 | Must |
| E10 Mandate authoring | M8, M9 | 6.3 | Must |
| E11 Web app: dashboard and controls | M9 | 6.8 | Must |
| E12 Audit explorer | M9 | 6.7 | Must |
| E13 Hybrid deployment | M11 | 6.9 | Must |
| E14 Billing | M12 | 6.10 | Must |
| E15 Signal models: LLM and fast models, scorecards | M5 (LLM research, scorecards); Phase 3 (fast models) | 6.3, 6.5 | Must (E15-1, E15-3); Should |
| E16 Kraken Derivatives US connector | Phase 3 | 6.2 (FR-2.5) | Should |
| E17 Research agent and dynamic universe | M5 | 6.3 (FR-3.9), 6.5 | Must |

## Stories

### E1 Foundations

- **E1-1 (Must)** As an engineer, I want a Rust workspace and Python package with CI so that
  every change is built, linted, and tested.
  *Accepted when:* CI runs build, tests, and lint on every push; main is protected.
- **E1-2 (Must)** As an engineer, I want an ADR template and coding conventions so that
  decisions and code stay consistent.
- **E1-3 (Should)** As an engineer, I want the pending-test gate to read a failure's *cause* rather
  than its text, so that no wording in a test can stand in for the stub it is meant to reach
  (DEC-137, gap 1's remainder; the coordinator's round-2 ruling item 4).
  *Accepted when:* `cargo xtask ci pending` accepts only the `Err` value printed after
  `called \`Result::unwrap()\` on an \`Err\` value:`, the `todo!`/`unimplemented!` panic line, or the
  `Err(..)` `Debug` inside a proptest failure, and a marker written into a test's own assertion
  message no longer satisfies it; `BEHAVIOUR_ONLY_TESTS`, which has had no `mandate-risk` row
  since E6-8 (DEC-163), is retired as E7-4 lands, or replaced by a rule that reads the cause; and the planted cases of #172's reviews all fail the
  gate. Since DEC-164 a failing property is already read by proptest's report of its minimal
  failure alone, and `mandate-executor`'s three E7-4 properties that passed only on a stub report
  from a case shrinking moved past have their rows; E1-3 narrows that report, and every other
  test's output, to the cause.
- **E1-4 (Should)** As an engineer, I want `shellcheck` over `.github/scripts/` and `actionlint`
  over `.github/workflows/` in `cargo xtask ci lint`, installed at pinned versions by `install.sh`
  and CI, so that a shell or workflow mistake in the merge path (DEC-175) fails a check rather
  than waiting for a reviewer (the #316 reviews).
  *Accepted when:* both run in `ci lint`, pinned in `.github/workflows/ci.yml` and `install.sh`,
  and a planted `SC2086` or an unknown workflow key fails the job.
- **E1-5 (Should)** As an engineer, I want the merge script's remaining gaps from #316's
  round-3 review closed, so that the only path from approval to `main` (DEC-175) is tested as
  GitHub actually answers it. The items:
  - The stub `gh` serves only the first page unless `--paginate` is passed, and a case lists
    `web.yml` past file 100.
  - A fixture lists workflow runs newest first, as GitHub does, so that `.[-1]` in place of
    `max_by(.id)` fails.
  - A merge GitHub refuses is a skip, not a failed job. That covers a sweep and a per-PR run racing
    after one of them has merged, a ruleset block, and a head that moved between the read and the
    merge call.
  - The approval line is not read inside an HTML comment, an indented code block (four or more
    leading spaces; a bullet indented up to three spaces is still read, as DEC-175 allows), or a
    four-backtick fence.
  - The web path rule comes from `web.yml`'s `paths` filter, not a second copy, and a file renamed
    out of `web/` counts by its `previous_filename` too.
  - Optionally, the latest `labeled coordinator-approved` event must be newer than the head
    commit, so that a description line alone approves nothing.

  *Accepted when:* each item has a refusal or merge case in `xtask`'s merge-script tests, and each
  fails when its fix is reverted.

### E2 Market data

- **E2-1 (Must)** As a researcher, I want to download historical bars and trades for US stocks,
  ETFs, and crypto (starting with a stock/ETF basket and BTC/USD) for a date range so that I can
  backtest.
  *Accepted when:* `download` fetches Alpaca historical data into Parquet; re-running is idempotent.
- **E2-4 (Must)** As a researcher, I want corporate actions (splits, dividends) and market
  sessions recorded with the data so that stock history and gaps are interpreted correctly.
  *Accepted when:* `inspect` distinguishes session closures from true gaps; split-adjusted and
  raw prices are both available.
- **E2-2 (Must)** As a researcher, I want to inspect a dataset for coverage, gaps, duplicates,
  and summary statistics so that I trust it before using it.
  *Accepted when:* `inspect` reports gaps with exact timestamps; tests cover gap and duplicate detection.
- **E2-3 (Should)** As a researcher, I want order-book top-of-book data so that slippage models
  can use spreads.

### E3 Accounting

- **E3-1 (Must)** As a trader, I want positions, cash, fees, and realized and unrealized P&L
  computed correctly so that every later number is right.
  *Accepted when:* property-based tests and hand-calculated cases pass, including partial fills
  and position flips.
- **E3-2 (Must)** As a trader, I want splits and dividends applied to positions and cash so that
  stock P&L is correct.
- **E3-3 (Must)** As a trader, I want settlement tracked for cash accounts so that the system
  knows which cash is available to trade.

### E4 Simulated execution and backtest

- **E4-1 (Must)** As a researcher, I want market and limit orders filled with configurable
  slippage and fees so that backtests are realistic.
- **E4-2 (Must)** As a researcher, I want a baseline strategy and a metrics report (return,
  volatility, Sharpe, maximum drawdown, turnover, fees, buy-and-hold comparison) so that the
  loop is proven end to end.
  *Accepted when:* identical inputs produce identical outputs.

### E5 Journal

- **E5-1 (Must)** As an auditor, I want every event appended to a hash-chained journal with
  causal links so that history cannot be silently altered.
  *Accepted when:* the [journal test vectors](../specs/reference-cases/journal.yaml) (chain, string
  escaping, decimals, export line, Merkle anchor) reproduce byte for byte; the verification tool
  reports each tamper case's expected first failure; and the append protocol returns each append
  case's expected outcome ([journal spec](../specs/journal.md)).
- **E5-3 (Must)** As an operator, I want the Postgres journal hardened (body stored as exact
  canonical bytes with a hash check, append-only roles and triggers including TRUNCATE, stream
  heads with writer fencing) so that records cannot be altered by application code.
- **E5-2 (Must)** As an engineer, I want large artifacts stored by content hash so that the
  journal stays small and verifiable.
- **E5-5 (Must)** As an owner, I want my personal data (the broker's account number and account
  id, names, emails) held in the workspace's personal-data vault under a per-person key, with each
  journal event carrying only random vault references in `pii_refs`, so that the records can be
  kept and my data erased separately ([journal spec](../specs/journal.md) §3, §6.4). It replaces
  DEC-142's interim: `mandate-alpaca`'s record discards a redacted value and labels it by position.
  *Accepted when:* a recorded exchange's redacted values are in the vault, its `pii_refs` are the
  vault's random references passed into the record as an input (ES-21), sorted, and none is a hash
  of the value.
- **E5-4 (Must)** As an auditor, I want `mandate-cli journal verify` over an exported stream and
  its artifact store, and a CLI command to put and fetch artifacts, so that I can check a journal
  without writing code (M4's verification tool; deferred from the E5-1 and E5-2 briefs).
  *Accepted when:* for each [tamper case](../specs/reference-cases/journal.yaml) its input can
  express, the command reports the expected first failure; a deleted or altered artifact reports
  `artifact_missing` or `artifact_mismatch` ([journal spec](../specs/journal.md) §11).
- **E5-6a (Should)** As an auditor, I want the cold store's checks held to their invariants over
  random segment sequences, not only the hand-calculated cases, so that the walk cannot drift
  between the cases (E5-6's follow-up, owed by AGENTS.md's "property-based tests for invariants"
  and deferred by #404's review; a DEC-77 tests PR on claim
  [#374](https://github.com/kunwarshivam/mandate/issues/374)).
  *Accepted when:* proptest drives `mandate-journal-cold` over random segment sequences —
  contiguous runs split at random boundaries, tampered files, lying manifests, gaps and
  overlapping copies, mid-segment entries — and an independent oracle (its own accumulator,
  never the implementation's walk) checks the invariants: a verified range's state is the
  trusted start advanced by exactly the events inside it; every tamper is refused by the check
  that owns it, at the `seq` the range expected; nothing before the trusted start is checked;
  and the token entry point never answers `Ok` (DEC-263 to DEC-265).

### E6 Agent runtime and risk

- **E6-1 (Must)** As an operator, I want an agent to run a mandate continuously so that it
  trades without supervision.
- **E6-2 (Must)** As an operator, I want every proposed action classified AUTO, ASK, or DENY
  per the mandate so that autonomy matches my rules. *Accepted when:* mandate reference cases
  MC-A01 to MC-A11 and MC-B01 to MC-B29 pass.
- **E6-3 (Must)** As an owner, I want an independent risk gate enforcing all limits so that no
  agent logic can exceed them.
  *Accepted when:* simulation fuzzing across random market paths and mandates never produces
  an order outside limits; the mandate invariants MI-1 to MI-11 hold under property-based tests;
  mandate reference cases MC-G01 to MC-G13 and MC-F01 to MC-F04 pass.
- **E6-4 (Must)** As an owner, I want a daily-loss limit and a drawdown ladder so that losses
  trigger automatic de-risking. *Accepted when:* MC-R01 to MC-R15, MC-T01 to MC-T05, and MC-L01
  to MC-L09 pass.
- **E6-5 (Must)** As an owner, I want kill switches per agent, connection, and workspace so
  that I can stop everything immediately.
- **E6-6 (Must)** As an owner, I want US account rules (day-trading regime, settlement, short
  sales, market hours) enforced by the risk gate so that agents never get my account restricted.
  *Accepted when:* simulation tests for each rule pass; blocked orders are journaled with the rule.
- **E6-7 (Must)** As an owner, I want an instrument eligibility floor (exchange-listed, no OTC or
  IPO-day, price and liquidity floors, leveraged ETPs only with opt-in) so that agents stay in
  liquid, suitable instruments. *Accepted when:* RC-16 passes.
- **E6-8 (Must)** As an owner, I want market-conduct controls (one working order per side,
  minimum resting time, price collars, participation caps, order-to-fill limits, close-window
  rules, workspace self-trade prevention) and a daily surveillance report, so that agents cannot
  produce manipulation-like patterns. *Accepted when:* the gate's checks 5 and 6, the pacing of an
  allowed exit, the cancel rule and the surveillance report pass their `tests/` suites and in-module
  boundary tests with zero missed mutants (#311, DEC-163). RC-22 and RC-25 cannot run yet: the
  trading-domain harness has no gate driver, RC-25's steps carry no `quote` or volumes, and RC-22
  also needs E7-2, E7-4 and E6-11; each passes when those land (the "RC-22 and RC-25, blocked in the
  trading-domain harness" follow-up).
- **E6-9 (Must)** As an owner, I want account restrictions and trading halts checked before every
  order, so that agents stop adding risk when the broker restricts the account. *Accepted when:*
  RC-15 passes.
- **E6-10 (Must)** As an owner, I want the gate to admit crypto **USD pairs only** (trading-domain
  §3.2 item 7), so that an agent cannot open a stablecoin-quoted pair the floor was never written
  for. `AssetId` is a UUID, so the quote currency is its own input,
  `InstrumentSnapshot::quote_currency`, where only a stated USD admits (DEC-254). Until the
  implementation reads it the gate keeps a crypto opening owed at check 2 and refuses it
  fail-closed (DEC-129 item 34). *Accepted when:* a crypto opening in a non-USD pair is denied, a
  USD pair passes the floor, and check 2 is whole for crypto (`crates/mandate-risk/tests/usd_pairs.rs`).
- **E6-11 (Must)** As an owner, I want the daily surveillance report delivered to me and a conduct
  breach to move the agent to `exits_only`, so that §9.6's "breach → agent `exits_only`" and its
  "threshold breaches are routed to the owner, whose acknowledgment is journaled" hold. E6-8 computes
  the report (`mandate_risk::surveillance`) and denies the breaching opening; nothing consumes
  either yet. The runtime owns the mode transition (mandate §5.9), and the notification carries only
  opaque IDs (`AGENTS.md` rule 6). *Accepted when:* an order-to-fill breach switches the agent to
  `exits_only` with an owner alert, risk-reducing orders continue, the day's report is journaled and
  routed per workspace, the owner's acknowledgment is journaled, and RC-22's `conduct_breach` step
  passes.
- **E6-12 (Must, before E10-8)** As an owner, I want an order my connected agent asked for always to
  reach me, and asks capped per day, so that a prompt-injected or looping agent can neither trade
  unasked nor wear me down ([ADR-0003](../adr/0003-earned-autonomy.md) part 10, [DEC-185](04-decision-log.md#decisions), [DEC-195](04-decision-log.md#decisions)). The client ceiling is
  mandate spec §6.2 step 5a and MI-30 ([#328](https://github.com/kunwarshivam/mandate/pull/328)); the
  per-client ask budget waits on its spec change (MI-33, [DEC-251](04-decision-log.md#decisions)). Safety-critical (autonomy policy), DEC-77 sequence.
  *Accepted when:* `requested_by` is set from the authenticated channel and journaled in
  `DecisionMade`; a client-requested opening is `ask` under every `auto` rule, `auto` default, live
  delegation, and `auto` admission, and a `deny` still denies; owner and agent requests decide as
  before; past 10 client-requested asks per client per risk day (the owner may lower it), further
  asks from that client are suppressed as `client_budget` and journaled, never notified, on top of
  §6.4's per-agent budget of 10; risk-limit alerts are never capped.
- **E6-13 (Should)** As an owner, I want tripwires I set in advance to end my delegations or hold new
  openings when their condition is met, so that trust does not outlive the conditions I gave it
  under ([DEC-187](04-decision-log.md#decisions)). Waits on the mandate spec change for `autonomy.tripwires` (MI-31, V-044).
  Actions are `end_delegations` or `exits_only`, never `paused` (rule 13). *Accepted when:* a fired
  tripwire acts at its next evaluation, journals the event, alerts with opaque text, and lifts only
  by the owner's acknowledgment with step-up; adding or tightening one applies at once.
- **E6-14 (Should)** As an owner, I want every `auto` and every delegation to fall back to `ask` once
  my mandate passes its review date unconfirmed, so that an abandoned account stops acting alone
  ([DEC-188](04-decision-log.md#decisions)). Waits on the spec change for `autonomy.review_by` (MI-32). *Accepted when:*
  past the date, autonomous paths decide `ask`, positions and exits are untouched, and re-confirming
  (a version moving the date later, with step-up) restores them. The spec change is MI-32, V-046, and §6.2 step 5b
  with the MC-D cases ([DEC-271](04-decision-log.md#decisions) to [DEC-273](04-decision-log.md#decisions): re-confirming is
  risk-increasing and carries the delegations over). Owed after it: `mandate-spec` parsing the field, V-046, and its
  §9.2 row (stream F), then the spec change that makes the date required on every version holding an `auto` or a
  delegation, with the base mandates (DEC-272 item 3). Also owed, from #380's rulings: the vectors for journal spec §9.1
  rule 7's clause that `decided_by: review_ceiling` requires `autonomy: ask` (the clause is in the spec). One code PR
  carries the invalid draft `decision_review_ceiling_on_auto` and the valid draft `decision_review_ceiling_asked`, the
  validator rule `7.review_ceiling_label` and its mutant in `reference/journal/generate.py`, the regenerated
  `journal.yaml` and `fixtures/refcases/journal.json`, and the agent-stream harness's count
  (`crates/mandate-refcases/src/journal/agent_stream.rs`, 106 to 108), as DEC-177 items 6 and 14 did (ES-22).

### E7 Alpaca connector and recovery

- **E7-1 (Must)** As an operator, I want to connect my Alpaca paper and live accounts through
  OAuth, granting trading access only, so that agents can trade without Mandate ever being able
  to move my funds.
  *Accepted when:* the OAuth request contains only trading and account-read scopes; tokens are
  stored only in the workspace vault.
- **E7-2 (Must)** As an owner, I want order intents journaled with idempotency keys so that
  crashes never duplicate orders.
- **E7-3 (Must)** As an owner, I want the agent to reconcile with the exchange after a restart
  so that its state matches reality.
  *Accepted when:* fault injection at every submission step yields zero duplicates and full
  reconciliation; mismatches pause the agent and alert.
- **E7-4 (Must)** As an owner, I want protective exits resting at the broker as OCO or bracket
  orders, with defined exit and kill-switch sequences, so that positions keep protection if the
  platform is down.
  *Accepted when:* RC-14 passes; unprotected windows are journaled and alerted beyond the limit.
- **E7-6 (Must, M8)** As a retail user, I want to connect a Robinhood agentic trading account over
  MCP so that my agent trades US equities and crypto spot with the funds I deposited there and
  nothing else ([DEC-98](04-decision-log.md#decisions),
  [OD-12](04-decision-log.md#open-decisions)).
  *Accepted when:* the connector can place, cancel, and reconcile equity orders in the dedicated
  account only; every MCP exchange is journaled; beta terms are recorded; the connector requests and
  stores only the agentic account's data; account numbers are held by reference (journal spec §6.4)
  and never logged.
- **E7-5 (Must)** As an owner, I want one account ledger per broker account and one agent per
  instrument per account, so that agents never overspend or cross each other.
  *Accepted when:* RC-17 passes; external activity switches agents to exits-only (RC-15).
- **E7-7 (Must, M6)** As the founder, I want one order placed end to end on my Alpaca paper account
  through the real crates, so that integration defects appear before the Phase 1 soak
  ([task brief](tasks/E7-7-tracer-bullet.md), [DEC-138](04-decision-log.md#decisions)).
  *Accepted when:* the whole path — validated mandate, stored market data, the E4-2 moving-average
  baseline as the signal, the order builder's sizing, the risk gate, the runtime's decision cycle with
  journal-before-acting, the executor's idempotent intent, the Alpaca paper connector, the journal
  record, and a reconciliation after a restart — runs in CI against recorded Alpaca paper fixtures and
  touches no network; with any one stage replaced by a stub returning its crate's `Unimplemented`
  error, zero orders reach the connector and nothing is journaled as submitted; and a manual paper run
  places exactly one order, journals its intent before sending it, and refuses any host that is not
  Alpaca's paper host.
- **E7-8 (Must, M6)** As the founder, I want the tracer to read an instrument's asset record and its
  latest quote from Alpaca, so that sizing, the collar and the executor's instrument snapshot have a
  production source ([DEC-168](04-decision-log.md#decisions), the coordinator's ruling on #171).
  *Accepted when:* `TradingClient::asset` reads trading-domain §3.1's fields from `GET /v2/assets/{symbol}`
  and `DataClient::latest_quote` reads the IEX or crypto latest quote from the data host, both as exact
  values; a missing, unreadable, one-sided, other-instrument or stale answer is a typed refusal and
  never a value (rule 3); the data host is reachable only through its own request type; and the tests
  in `crates/mandate-alpaca/tests/reads.rs` pass against the recorded fixtures with no network.
- **E7-9 (Must, M6)** As the founder, I want the agent-stream payload schemas registered in
  `mandate-journal`, so that the tracer's journal records parse against the journal spec's vectors
  (DEC-168, the coordinator's ruling on #171). *Accepted when:* each agent-stream event the runtime
  and the executor write has a registered schema, tested first against the journal spec's vectors.
  *Unblocked in half* ([DEC-177](04-decision-log.md#decisions)): journal spec v0.4 closed none of the
  eleven agent-stream schemas the runtime writes (DEC-174). Journal spec v0.6 §9.1 closes the agent
  stream's `StreamOpened`, `ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`,
  `IntentProposed`, `AgentModeChanged`, `KillSwitchActivated`, and `OwnerExitRequested`, with
  vectors in `journal.yaml`'s `agent_stream` section, so their tests can be written first now. The
  approval events, which journal spec v0.5 added (M7, claim
  [#213](https://github.com/kunwarshivam/mandate/issues/213)), are not closed yet (DEC-177 item 23).
- **E7-10 (Must, M6)** As the founder, I want the control-stream payload schemas registered and mapped
  to stream F's `JournaledFact`, so that `ValidationContext::from_journal` has a production source
  (DEC-168, DEC-169, the coordinator's ruling on #124). *Accepted when:* `AccountSnapshotRecorded`,
  `AgentDeployed`, `AgentStopped`, `ConnectionEstablished`, `DisclosureAccepted`,
  `MandateVersionCreated`, `MandateConfirmed`, `ConfigSnapshotRegistered` and
  `PlatformOperatorAction` have registered schemas, and each maps to its `JournaledFact`, tests first.

### E8 Escalation and approvals

- **E8-1 (Must, M7)** As an approver, I want requests with the proposed action, alternatives,
  evidence, risk impact, deadline, and default so that I can decide quickly
  ([task brief](tasks/M7-escalation-v0.md), [DEC-155](04-decision-log.md#decisions)). "Alternatives"
  means the owner's choices (approve or skip, with the default stated), never platform-authored
  alternative trades ([mandate spec §6.4](../specs/mandate.md#64-approvals), FR-6.2).
  *Follow-up (DEC-165 item 3, #236):* the content's Trigger row still lacks "the rule as the owner
  wrote it". §6.4 now fixes it (DEC-173 item 2): `trigger.rule` is the owner's confirmed rule
  `{id, when, then}` exactly as the mandate holds it, or null for `default` and
  `admission_ceiling`. It joins the content object in a tests correction, with a test that
  scans owner-written text apart from the platform's own in
  `the_content_never_carries_advice_wording`, so an owner's rule named `target_weight` is shown as
  written and never read as platform advice.
  *Follow-up (#250 review, minor 1):* `RequestContent.risk_impact` is a `Vec<RiskFigure>`, so a
  caller could list fewer than §6.3's six figures, or one twice. Make it one figure per
  `RiskField` by type (`[RiskFigure; 6]` in `RiskField` order, or a map keyed by field), with a
  tests correction for `every_bound_field_moves_the_content_hash`, which truncates the list.
  *Done (#250 review, major; DEC-165 item 13):* `ApprovalRef::of_requested_event` accepts only a
  ULID-shaped event id, so free text cannot reach a notification through it. The runtime or CLI
  PR that first calls it must prove the id is that `ApprovalRequested` event's own.
  *Follow-up (#254 review, minor 1):* `mandate-approval`'s `is_ulid` is a copy of
  `mandate_journal::schema::is_ulid`. Layering stops the approval crate from depending on the
  journal: it is at layer 1 and the journal at layer 2. So nothing keeps the two copies in step.
  Pin them at rung 2, with an `xtask` check that compares the two function bodies, or at rung 1,
  by moving the shape into a layer-0 helper that both crates call.
- **E8-2 (Must, M7)** As an owner, I want timeouts to apply the safe default so that silence never
  adds risk ([task brief](tasks/M7-escalation-v0.md), [DEC-156](04-decision-log.md#decisions)).
  *Follow-up (#250 review, minor 4):* EI-13's first bound, one pending risk-adding approval per
  agent, is not in `mandate_approval::ask_permit`; it stays in the runtime's
  `awaiting_risk_approval`, and the runtime's tests PR must assert it.
- **E8-3 (Must, M7)** As an owner, I want approved actions re-validated for drift so that stale
  approvals are not executed blindly ([task brief](tasks/M7-escalation-v0.md),
  [DEC-156](04-decision-log.md#decisions)). The same brief covers M7's CLI owner control.
  *Done (#240 review, round 2, minor 2; the M7 tests correction, DEC-173 item 14):* the step-up
  generators in `tests/grant_properties.rs` reached an age of exactly 300 s only by chance
  (`-350..20` and `-400..60`), so a planted exclusive window survived about half of all seeds.
  Both now draw -300 one time in five (`at_the_window_edge_or`); the same planted window fails both
  properties it reaches on every one of 20 seeds, where the old generators missed it on 10.
  *Done (E8-3 implementation, the do-nothing sweep; the M7 tests correction, DEC-173 item 14):*
  `drift_exactly_at_the_band_is_inside_and_one_unit_over_either_way_is_not` and
  `no_mark_is_outside_the_band` checked the drift result of `revalidate` in one direction only, so
  a constant `Skip(Drift)` passed both. Each now opens with the paired positive, "the unchanged
  fixture acts", and that constant fails both.
  *Follow-up (#275 review, minor 2; the coordinator's ruling, rule 13):* when
  `owner_command(OwnerExit, …)` returns `CommandAuthority::Refused`, only the owner-exit
  privilege is withdrawn: selling equities outside the regular session at the confirmed bid.
  The exit itself is never withdrawn. The runtime must still route that owner exit, either as a
  regular-session exit or as the displayed-bid-confirmed exit once the owner confirms a fresh bid.
  The refusal never holds it, drops it, or turns it into a no-op. **Test obligation, in the
  runtime's tests PR (3 of 4):** a test commits an owner exit whose step-up is stale at commit
  and asserts two things. First, the refusal is journaled. Second, the exit is still routed as
  a regular-session exit and reaches the executor, so no step-up outcome can remove an owner's
  risk reduction. The mandate spec §6.1 wording is in ("Owner controls and step-up", DEC-173
  item 5).
  *Follow-up (M7 spec PR, DEC-173 item 1):* the MC-E cases (MC-E01 to MC-E31) are not yet in
  `mandate.yaml`. Two changes, in order. *Done (the tests correction,
  [#343](https://github.com/kunwarshivam/mandate/pull/343)):* `mandate_harness.rs` counts only the seven families it
  owns, by case-ID prefix (MC-S, MC-V, MC-P, MC-C, MC-R, MC-T, MC-L), and
  `a_kind_no_arm_interprets_fails_naming_it` accepts a kind no arm interprets as long as its cases
  fail, so a new family changes no harness test while a case added to or dropped from an owned
  family still fails. Still open: an MC-E spec PR that generates the cases from
  `reference/mandate/ref.py`'s escalation model (already fuzzed and mutation-checked), with
  `cargo xtask refcases --write`, and no `status.toml` row. Give the family a kind of its own: the
  family A, B, G, F, and P count tests select their cases by kind, so a new family reusing one of
  those kinds would change their counts. The same PR corrects mandate spec §11's and §1's sentences
  that MC-U "lands in its own tests-first change, because the shared harness pins the case count":
  since #343 it no longer does (#343 review, minor 4).
  *Done (the M7 spec PR, DEC-280):* MC-E01 to MC-E32 are generated from the reference model as
  `kind: escalation` cases, checked by `check_cases.py`, exported to `fixtures/refcases`, with no
  `status.toml` row; §11 lists the family and no longer says the harness pins the count.
  *Follow-up (DEC-280):* a harness arm for `kind: escalation` in `mandate-refcases`, tests first,
  driving `mandate-approval` (and the runtime for `op: lifecycle`), then the status PR that flips
  the rows it passes.
  *Part done (DEC-292):* the arm interprets `ask_permit` and `deliver_now` through
  `mandate-approval`, and MC-E25 to MC-E28, MC-E30 and MC-E32 pass; their `status.toml` rows are the
  status PR's. Still open: the `lifecycle` op's runtime driver for the other 26 cases, which fail
  naming the op until it lands.
  *Follow-up (the #416 review, minor 3):* every `ask_permit` case asks for one instrument, so
  family E cannot see the budget counted per instrument rather than per agent (`mandate-approval`'s
  own suite does). A future MC-E case should spread its ten asks across instruments. It changes
  `mandate.yaml`, so it goes to the founder under DEC-176 unless it only tightens a rule.
  *Follow-up (DEC-280 item 7):* `mandate-journal`'s catalogue gains `OwnerCommandRefused` (agent and
  account streams), and the runtime journals it for a refused resume or Stop, tests first, with a
  test that a refused Stop leaves exactly that event (the #395 review, major 1; DEC-278 item 12).
  *Tests done (the M7 `OwnerCommandRefused` tests PR, DEC-291); the runtime's implementation
  follows:* the catalogue admits it on the account and agent streams; three tests in
  `crates/mandate-runtime/tests/approvals.rs` are pending E8-3: a refused Stop leaves exactly its
  `OwnerCommandRefused` for each of missing, stale and reused evidence, a stale resume does the
  same, and a refused command re-tailed after a restart writes nothing. The executor's half (a
  refused acknowledgment) rides with E7-4 slice 5's copy of `OwnerAcknowledged` (DEC-291 item 4).
  *Done (the runtime's half, DEC-291):* the five tests pass. A resume or Stop whose step-up does
  not count writes exactly its `OwnerCommandRefused` (`command`, `effective_at`, `reason`, with the
  `OwnerCommandIssued` as `causation_id`), and the fold reads it as a copy, so a re-tailed command
  writes nothing.
  *Follow-up (the #413 review, minor 3):* give `mandate-approval` one `From<StepUpRefusal> for
  Refusal` mapping and one reason-code table, retiring the three tables kept by hand today
  (`mandate-runtime`'s `refusal_code`, its `OwnerCommandRefused` reasons, and `step_up_status`'s
  input), so a new refusal cannot be coded differently in two places.
  *Follow-up (the #397 review, minors 1 to 5; one M7 tests PR before the `clap` wiring makes the
  commands reachable):* pin the closed key set of every control payload the CLI commits
  (`OwnerCommandIssued`, `ApprovalResponseSubmitted`, `OwnerAcknowledged`) in `tests/agent.rs` and
  `tests/approvals.rs`, so a `typed_code` member fails; give `Command::Stop` the `--release` choice
  and the warning-shown record (DEC-136) and `status` the agent's restrictions, tests first; make
  `commit`'s idempotency hold across invocations by deriving the event id from the command's
  content and the head (or record why it cannot), add backoff between attempts, and word the
  exhausted report to match; and make `message(Outcome::Refused)` say whether the deadline has
  passed.
  *Tests done (the M7 CLI follow-up tests PR, DEC-290); the implementation follows:* the closed
  member sets, the derived event id and its independent oracle, and the three retry arms are live
  tests; 13 tests pending E8-3 in `tests/agent.rs` and `tests/approvals.rs` cover Stop's release
  and its warning, `status`'s restrictions, the re-run that finds its committed event (with another
  writer's event interleaved too), the kill switch always committed, the repeated acknowledgment,
  the runtime-recorded command committed anew, the backoff, and the refusal's deadline wording. `Command::Stop` gains `release`, `status` takes the account,
  `message` takes the owner's clock, `Ids` no longer mints the event id, and `ControlJournal` gains
  `wait`; the `clap` wiring still waits for the implementation.
  *Follow-up (the #409 review, minor 4; in the implementation PR):* `mandate-cli` carries the
  approval flow, step-up and the owner commands (`AGENTS.md`'s safety-critical list), so set
  `safety_critical = true` for it in `xtask/layers.toml`, add its CODEOWNERS line and the lint
  header `cargo xtask layers` checks, and let the mutation gate judge its diff.
  *Done (the M7 CLI follow-up implementation, DEC-290):* the 13 tests pass. A Stop with release
  records `release: true` and the warning's content reference, and its code binds the choice;
  `status` folds the account stream's restrictions; a re-run of the control stream's last,
  unrecorded command reports it, whatever head it was decided at, while a kill switch skips that
  search and is always committed; retries back off from 100 ms, doubling to 2 s, with a jitter
  drawn from the event id; and a refusal's message says whether the deadline has passed.
  `mandate-cli` is safety-critical (`xtask/layers.toml`, CODEOWNERS, the lint header). Still open:
  the `clap` wiring and a `mandate-journal-pg` `ControlJournal` (DEC-279 item 10).
  *Follow-up (#343 review, minor 1; a tests correction):* a new family that reuses an owned family's
  kind (for example an `MC-E01` of kind `semantic`) now moves no count in `mandate_harness.rs` and
  runs through that family's arm, where on `main` before #343 it failed two counts. `unread_keys`
  still refuses any member it ignores, so it cannot pass half-read, but the loud failure is gone.
  Assert that the owned-by-kind id set equals the owned-by-prefix set in
  `the_fixture_holds_the_families_this_stream_expects`; the reviewer's four-line version passes on
  today's fixture and fails on that scenario.
  *Follow-up (#343 review, nits):* make `INTERPRETED` a `pub const` in `src/mandate.rs` that
  `run_listed`'s dispatch and the test both read; the `{prefix}{n:02}` ids with a lexicographic sort
  break past 99 cases in a family; the uninterpreted-kind branch asserts only `is_err()`, not that
  the message names the kind.
  *Follow-up (M7 spec PR, DEC-173 item 11):* the `mandate-journal` catalogue (`src/catalogue.rs`,
  `tests/catalogue.rs`) needs `ApprovalRevalidated` (agent, `man`), `ApprovalResponseSubmitted`
  (ctl), and `OwnerCommandIssued` (ctl) from journal spec v0.5 before the runtime's tests PR can
  journal them.
  *Done (#321 review, major; DEC-173 items 13 to 15; the M7 tests correction, then E8-3's
  `quorum`, DEC-257):* `mandate-approval`'s admission read only the bound
  `approvers_required` and `independent_required`. It must judge check 7 against the stricter of
  those and the workspace policy overlay current at the effective time: independence if either
  requires it, the larger approver count, and an author's earlier `counted` grant not counting once
  independence is required. The tests correction gave `AdmissionContext.policy` the overlay and
  stubbed `mandate_approval::quorum`, which `admit` failed closed on under any overlay but
  `PolicyOverlay::NONE`, with eight tests in `tests/quorum.rs` pending E8-3 against
  `reference/mandate/ref.py`'s `approval_quorum`. The implementation PR implemented `quorum`,
  calls it for every overlay, leaves the author out of the grants that count while independence
  is required, and deleted the eight `#[ignore]` lines.
  *Tests done (M7 tests PR 3 of 4, DEC-257 items 5 to 12); the runtime's implementation follows:*
  `crates/mandate-runtime/tests/approvals.rs` and `approval_properties.rs` hold 20 tests pending
  E8-3 for the grant path, owner commands on the control stream, and the #281 owner-exit
  obligation. The implementation PR needs, first, the `mandate-journal` catalogue entries above
  (a real journal refuses `ApprovalRevalidated` until then), and it fills `PendingApproval` with the
  bound request, folds `MarkUpdated`, the control stream's assertions and its copies, and hands a
  one-instrument owner exit as an agent-scoped flatten (DEC-257 item 7).
  *Done (E8-3's runtime implementation, DEC-278):* the 20 tests pass; `mandate-runtime` asks with
  the content object, its hash, the `cli_inbox` delivery and one opaque notification, admits each
  `ApprovalResponseSubmitted` through `mandate_approval::admit` and re-validates a grant in the same
  batch, and copies each `OwnerCommandIssued`, never refusing a kill switch or an owner exit. The
  `mandate-journal` prerequisite below is still open, so the shell cannot yet append these events
  through a real journal.
  *Follow-up (DEC-278 item 13; DEC-156 item 5):* the runtime does not yet bound asking: at most 10
  requests per agent per America/New_York risk day, no re-ask of an instrument the owner skipped
  until the next risk day or applied version, none for one `timeout_s` after a timeout, and each
  suppressed ask recorded as `DecisionMade.ask_suppressed`. `mandate_approval::ask_permit` and
  `AskLedger` exist; fold the ledger and call them before `escalation::ask`, tests first (MC-E25 to
  MC-E28). The same field must also name a request that cannot bind (DEC-278 item 2: no
  `DecidedBy` label, a combined score that is not a decimal, or a content object that cannot be
  represented), which today leaves `ask` on `DecisionMade` with no reason anywhere in the journal
  (the #395 review, minor 3).
  *Follow-up (DEC-278 item 12; the #395 review, major 1):* a refused owner resume, Stop, or
  acknowledgment journals nothing. The M7 spec PR adds an agent-stream `OwnerCommandRefused`
  (the command's control-stream id as `causation_id`, its kind, and the reason `step_up_missing`,
  `step_up_stale`, `step_up_reused`, or `step_up_method`); the next M7 implementation PR journals
  it, with a test that a refused Stop leaves exactly that event.
  *Follow-up (the #395 review, nit):* `IntentProposed` carries no `mandate_version`, so EI-4's
  version equality is enforced by check 8 but readable only from `ApprovalRevalidated`; add it.
  *Follow-up (DEC-278 item 11; a tests correction):* bump `FOLD_VERSION` and regenerate
  `tests/golden-journal.json` with an approval's request, delivery, response, and re-validation and
  a folded mark, since the fold now derives approvals, marks, and the control stream's assertions
  and copies from them.
  *Follow-up (DEC-278 item 3):* the request's `risk_impact` is empty. When the gate's dry run
  exposes the §6.3 figures (`position_usd_after`, `gross_usd_after`, `bought_today_usd`, `drawdown`,
  `daily_pnl_fraction`) and the mandate's caps, put them in the content object.
  *Follow-up (DEC-257 item 12):* `PolicyChanged`'s payload is only "level, diff" in journal spec §9,
  so the runtime neither folds the overlay check 7 reads nor has a two-approver runtime case
  (PB-22). Close the payload, then fold it and add the cases in a tests correction.
  *Follow-up (DEC-257 item 12):* no agent-stream event records a refused resume or Stop; specify
  one (or say a refusal leaves only the control-stream command) so the runtime can journal it.
  *Follow-up (DEC-257 item 12; stream I):* `Input::Command` still carries the owner's pause,
  resume, Stop, exit and kill switch with no step-up judged; retire those for the control stream's
  `OwnerCommandIssued` once the shell tails it, leaving only the risk-limit and operator switches.
  *Tests done (M7 tests PR 4 of 4, DEC-257 items 13 to 17); the CLI's implementation follows:*
  `crates/mandate-cli/tests/approvals.rs` and `agent.rs` hold 15 tests pending E8-3 for
  `mandate approvals list`, `show`, `approve` and `skip`, and `mandate agent status`, `pause`,
  `resume`, `stop`, `kill`, `exit` and `acknowledge`, over the `ControlJournal` stubs in
  `crates/mandate-cli/src/control.rs`. The implementation PR also wires the commands into `clap`
  and `main.rs`, and gives `ControlJournal` a `mandate-journal-pg` adapter.
  *Done (E8-3's CLI implementation, DEC-279):* the 20 tests pass; `mandate-cli` lists and shows
  the inbox from the agent streams, commits `approve`, `skip` and every `agent` command as exactly
  one control-stream event under one minted id, binds each code to what it confirms and the
  control stream's head, and never refuses a kill switch or an owner exit.
  *Follow-up (DEC-279 item 10):* wire `approvals` and `agent` into `clap` and `main.rs` with a
  `mandate-journal-pg` `ControlJournal`, once the prerequisite below registers these events'
  schemas; until then a real journal refuses every command they would send. Add Stop's
  `--release` choice and the warning shown to `Command::Stop` and the payload (journal spec §9).
  *Prerequisite (DEC-257 item 17):* `mandate-journal` registers no payload schema for any agent- or
  control-stream event (`ApprovalRequested`, `ApprovalResponseSubmitted`, `OwnerCommandIssued`,
  `OwnerAcknowledged`, `OwnerCommandRefused`, and the rest), so a real journal refuses each one as
  `UnknownSchema` and neither the runtime's nor the CLI's implementation can append through one.
  Register the schemas as journal spec §9 closes them, with the catalogue entries above, before
  either implementation.
  *Follow-up (#321 review, minor 1):* broaden §6.1's single-use assertion ledger to any
  control-stream event carrying step-up evidence (`DisclosureAccepted`, `PolicyChanged`), which
  would make MI-24 true as written.
  *Follow-up (#321 review, minor 2):* §6.1's and §6.4's "one assertion per approval" should read "per grant",
  because a two-approver approval takes two assertions.
  *Follow-up (#321 review, minor 3):* §5.9 says the executor cancels pending approvals, but §6.4 and
  journal spec §2 put that in the runtime's step; align them.
  *Follow-up (#321 review, minor 4):* journal spec §9's `ApprovalRevalidated` row lacks the
  working-universe membership that check 9 compares.
  *Follow-up (#321 review, minor 5):* check 3's `role:` approver entries are resolved at an unstated
  moment; state it.
  *Follow-up (#321 review, minor 6):* the reference model's assertion ledger never fills `used` from
  `OwnerCommandIssued` or `OwnerAcknowledged`.
  *Follow-up (#321 review, minor 7):* `notifications.channels` cannot express `cli_inbox`.
  *Follow-up (#321 review, minor 8):* `recent_timeout` does not say whose `timeout_s` it uses.
  *Follow-up (#321 review, minor 9):* `reference/mandate/mutants.py` runs only in
  `cargo xtask ci nightly`, not in `cargo xtask check`.
  *Done (#321 round 2, minor 1; the M7 tests correction's reference half, DEC-173 item 13):*
  nothing tested that `ApprovalResponded` records the approver count and independence check 7
  applied, so dropping the member survived the whole fuzz. `fuzz_escalation` now asserts that every
  grant reaching check 7, and no other response, records its own `own_quorum`, and `mutants.py`
  gains two planted bugs, the member dropped and the bound quorum recorded in place of the applied
  one, which `origin/main`'s fuzz let through and this one catches.
  *Follow-up (#321 round 2, minor 2):* say which stream the policy overlay is folded from.
  `PolicyChanged` is on the workspace control stream, and §2's copy list for the agent runtime does
  not include it. State either that the runtime copies it into the agent stream, or that replay
  reads the recorded quorum rather than re-deriving the overlay. Every interleaving only
  over-tightens today, because check 7 takes the maximum with the bound values.
  *Follow-up (#321 round 2, minor 3):* the reference model reads absolute
  `independent_approval_required` and `two_approver_above_usd` from `PolicyChanged`, where journal §9
  gives `level`, `diff` and `affected agents`. State how a partial diff resolves, at which level,
  and whether the agent must be listed in `affected agents`.
  *Follow-up (#321 round 2, nits):*
  - reword the admission sentence as "the grants that count … now number check 7's approver count";
  - say once that "at the effective time" and "folded before the step" coincide because the clock
    fold advances on every event;
  - the approval surface shows the current requirement, not only the bound `approvers` (a PX item);
  - add the per-order approval to §4.3's list of what `independent_approval_required` scopes.
- **E8-4 (Must)** As an approver, I want notifications through web push, email, and a chat
  channel, with escalation chains and quiet hours.
- **E8-5 (Must)** As a fund, I want notifications to carry only opaque IDs, with details loaded
  from our workspace deployment, so that trading intent stays private.
  *Accepted when:* captured relay and provider payloads contain no instrument, size, price, or thesis.
- **E8-6 (Should)** As a fund, I want two approvers above a threshold.
- **E8-7 (Should)** As an approver, I want SMS and phone escalation.
- **E8-8 (Should)** As an owner, I want to answer an ask with "let it do this for a while", within
  caps I set in dollars, orders, and days, so that the agent stops asking me about what I have
  already said yes to ([ADR-0003](../adr/0003-earned-autonomy.md) parts 2 and 3, [DEC-181](04-decision-log.md#decisions)). The spec is
  mandate §6.4 and §6.5 ([#328](https://github.com/kunwarshivam/mandate/pull/328)); the MC-U
  reference cases come first, in their own tests-first change. Safety-critical (autonomy policy and
  the approval flow). *Accepted when:* the MC-U cases pass; a delegation lifts only the `ask` it
  names, within its caps and window, and never while suspended (MI-26 to MI-28); choosing a shape
  approves this action exactly as "Approve just this" would and creates a version confirmed with
  the same step-up, which applies at the next safe point; the sum of a version's delegation caps
  stays within the allocation ([DEC-196](04-decision-log.md#decisions), V-045); no scope is offered for an admission, a
  two-approver ask, a live environment, or a client session.

### E9 Identity, tenancy, and policy

- **E9-1 (Must)** As a user, I want to sign in with passkey or OIDC SSO.
- **E9-2 (Must)** As an admin, I want organizations, workspaces, and roles.
- **E9-3 (Must)** As an admin, I want org-level limits that workspaces and agents can only
  tighten.
- **E9-4 (Must)** As a security-conscious user, I want step-up authentication for sensitive
  actions.
- **E9-5 (Should)** As a fund, I want separation of duties between agent creators and approvers.
- **E9-6 (Must)** As a retail user, I want the retail profile (`auto` allowed, LLM ideas allowed,
  protection required, no leveraged ETPs, counsel-set loss ceiling and approval timeout minimum)
  applied to my workspace by default ([DEC-98](04-decision-log.md#decisions)).

### E10 Mandate authoring

- **E10-1 (Must)** As an operator, I want to describe an agent in plain language and get a
  compiled mandate with inferred fields highlighted. *Accepted when:* compiled mandates validate
  against the [mandate spec](../specs/mandate.md) (schema, V-rules, policy hierarchy; reference
  cases MC-S, MC-V, and MC-P pass); proposed envelope values are marked as proposed and no envelope
  field activates unconfirmed ([DEC-97](04-decision-log.md#decisions)).
- **E10-2 (Must)** As an operator, I want to edit the mandate as a form or YAML, kept in sync.
- **E10-3 (Must)** As an operator, I want mandates versioned with viewable diffs, and changes that
  increase risk to require step-up. *Accepted when:* the version vector and MC-C01 to MC-C48 pass.
- **E10-4 (Must)** As an operator, I want going live to require a backtest, a paper run, and
  step-up approval.
- **E10-5 (Should)** As a new user, I want templates for common mandates.
- **E10-6 (Should, M8, pulled forward)** As an owner who already runs my own agent (for example
  Claude), I want to connect it through a Mandate MCP server that exposes the same API Mandate uses
  to take my input, so that my agent can work inside my mandate without a separate path around it
  ([DEC-141](04-decision-log.md#decisions), [DEC-148](04-decision-log.md#decisions)). An optional
  channel and an adoption on-ramp, never the only or the main path: the complete product leads, and
  the platform's research agent brings the ideas. **Placement (DEC-148):** the first M8 story after
  the owner-input API (M8's mandate registry, approval service, and owner controls), E9-1, E9-2,
  E9-4, and E10-3, ahead of M8's other Should stories (E9-5, E10-5); it does not wait for M9 or M10,
  and its earlier dependencies (E5, E6-2, E6-3, E7-2, E7-3, E7-5, and the tracer bullet E7-7) land
  before the Phase 1 gate. No change to milestone order. *Accepted when:* every client request
  passes through the same order builder, autonomy rules, risk gate, account ledger, and journal as
  the owner's own input; the client cannot change an envelope field (it may only propose a mandate
  version that the human confirms with step-up); ASK approvals go only to the human owner, and a
  client cannot approve its own proposal; owner-only privileges (owner exits outside the regular
  session at a confirmed bid, Stop and release, the kill switch) stay with the human; the client
  authenticates with its own scoped, revocable token and no broker credential crosses MCP; every
  client call is journaled with the client's identity; and the owner can revoke the client at any
  time.
  *Amended by [DEC-183](04-decision-log.md#decisions) (founder, 2026-09-30):* packaged as the Owlhead plugin for OpenAI
  Dots, Meta Muse, and Grok Bot (E10-8), and every client-requested opening meets the client ceiling
  (E6-12).
- **E10-7 (Must)** As a new owner, I want to answer three questions (how much money, what goal, how
  much I can stand to lose) and get a complete mandate drafted for me to confirm on one card, so
  that I do not have to write a mandate to start ([ADR-0003](../adr/0003-earned-autonomy.md) part 1, [DEC-182](04-decision-log.md#decisions)). Builds on E10-1's
  compiler; within mandate spec §7. *Accepted when:* the three answers become `user_stated` fields
  (`capital.allocation_usd`, `goal`, `capital.max_loss_from_allocation`) with their quoted spans;
  every other drafted field is `platform_proposed` and inactive until confirmed (MI-12, V-020); no
  `auto`, delegation, pinned instrument, environment, or connection is ever proposed (V-022,
  V-038); what the goal types cannot express is flagged not enforced; the card shows the unasked
  dollars ([DEC-189](04-decision-log.md#decisions)); and it binds the version hash on confirmation.
- **E10-8 (Should, after E10-6 and E6-12)** As an owner who lives in OpenAI Dots, Meta Muse, or Grok
  Bot, I want Owlhead as a plugin there, so that my everyday agent can work with my money through
  Owlhead's gate ([DEC-183](04-decision-log.md#decisions)). Listed publicly as Owlhead, linking
  owlhead.ai (DEC-171). *Accepted when:* E10-6's acceptance holds for each host; the listing and its
  consent screen name the scopes in words; confirmations and approvals happen only in Owlhead's own
  app or CLI; each host's plugin terms are recorded in the competitive landscape.
- **E10-9 (Should, with E10-8)** As a connected agent, I want to ask "would this be allowed?" before
  asking the owner, so that I do not flood them with asks the gate would deny ([DEC-190](04-decision-log.md#decisions)).
  *Accepted when:* the dry-run tool returns the decision and the gate's reason code with the client
  ceiling applied, places nothing, creates no approval, counts against the client's rate limit, and
  is journaled. **Also ([DEC-191](04-decision-log.md#decisions)):** a hold-new-openings tool that sets
  `exits_only` and nothing else; lifting it is the owner's alone, with step-up.

### E11 Web app: dashboard and controls

- **E11-1 (Must)** As an operator, I want a dashboard of agents, state, positions, P&L, open
  approvals, and recent decisions.
- **E11-2 (Must)** As an operator, I want pause, resume, stop, and kill switches in the UI.
- **E11-3 (Must)** As an operator, I want alerts for risk rungs, reconciliation mismatches,
  stale data, and paused agents.
- **E11-4 (Should)** As an owner, I want a plan view that says what my agent is watching, what it
  would do next under which rule, whether that would run on its own or ask me, and what would stop
  it ([DEC-184](04-decision-log.md#decisions)). A read of the mandate and the runtime's state; changes nothing.
- **E11-5 (Should)** As an owner, I want one daily brief of what ran, what was skipped, what my
  delegations let through, which guardrails fired, and what changed, so that only urgent things
  interrupt me ([DEC-184](04-decision-log.md#decisions); PX-16). The notification says only that the brief is ready
  (rule 6).
- **E11-6 (Should)** As an owner, I want to talk to my agent in a chat thread, where a message
  becomes an owner request (builder, gate, and autonomy rules) or a proposed mandate version
  (confirmed with step-up), never an order by itself ([DEC-184](04-decision-log.md#decisions), [DEC-192](04-decision-log.md#decisions)).
  *Accepted when:* a model reply never renders as an action control; action cards are built by
  deterministic code; "why" answers come from the journal (E12-5).
- **E11-7 (Should)** As an owner, I want to see the unasked dollars (what can trade without asking
  right now) on the card, the dial, and the brief, and to switch on away mode in one action, so
  that I can size my agent's autonomy and turn it down when I cannot answer ([DEC-189](04-decision-log.md#decisions),
  [DEC-194](04-decision-log.md#decisions)). *Accepted when:* the figure matches the reference model; away mode writes a
  risk-reducing version that applies at once, and at its end date asks me to restore rather than
  restoring itself.
- **E11-8 (Should, after E10-3)** As an owner, I want to see what would have changed over the last
  30 days before I confirm a version that adds autonomy, so that I widen it on evidence
  ([DEC-186](04-decision-log.md#decisions)). Counts only, replayed from the journal; no profit, loss, or outcome; wording
  waits for compliance question 39.

### E12 Audit explorer

- **E12-1 (Must)** As an auditor, I want a causal trace from any fill back to its causes.
- **E12-2 (Must)** As an auditor, I want per-agent timelines with filters and JSON/CSV export.
- **E12-3 (Should)** As an auditor, I want to run chain verification from the UI.
- **E12-4 (Could, not yet planned)** As an owner, I want a monthly record of every mandate breach
  and near-breach on my account, derived from the journal and its anchors, so that I can see the
  mandate held ([strategy options §8](../product/10-strategy-options.md#defensible-differentiators),
  DEC-145). Publishing it beyond the owner needs counsel's answer and the founder (DEC-79).
  *Extended by [DEC-193](04-decision-log.md#decisions) (founder, 2026-09-30):* the record is checked against the hash
  chain and lists the versions in force, the actions under each by purpose and autonomy source,
  every delegation used, and every guardrail that fired, with no performance figure; its text waits
  for compliance question 40.
- **E12-5 (Should)** As an owner, I want "why did it do that?" answered from the journal, not from a
  model's memory ([DEC-192](04-decision-log.md#decisions)). *Accepted when:* each answer links the `DecisionMade`, thesis,
  rule or delegation, and gate result it cites; a model may phrase it but adds no fact; a missing
  fact reads "not recorded"; wording waits for compliance question 41.

### E13 Hybrid deployment

- **E13-1 (Must)** As an IT admin, I want to install the workspace deployment with Helm or
  Docker Compose using an enrollment token, with outbound-only connectivity.
- **E13-2 (Must)** As an IT admin, I want signed releases and upgrades that preserve agent state.
- **E13-3 (Must)** As an IT admin, I want SSO against our identity provider and a fallback
  approval channel through our own mail server.

### E14 Billing

- **E14-1 (Must)** As an org owner, I want to subscribe to a plan and be billed per
  organization.
- **E14-2 (Must)** As an org owner, I want a hybrid license key tied to my organization.
- **E14-3 (Should)** As an org owner, I want usage metering visible in the app.

### E15 Signal models: LLM and fast models, scorecards

- **E15-1 (Must)** As an operator, I want an LLM research signal model that writes theses
  asynchronously without blocking trading.
- **E15-2 (Should)** As an operator, I want a fast decision model with a hard deadline.
- **E15-3 (Must, Phase 1)** As an operator, I want each signal model's confidence measured against
  outcomes and shown in scorecards ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* every thesis is scored after its horizon against the pre-registered baselines
  (buy-and-hold of the eligible basket, and a broad index ETF), net of modeled costs; scores come
  from forward paper trading only, never from historical backtests of LLM theses; the scorecard
  shows each signal model's results beside the baselines.
- **E15-4 (Could)** As an operator, I want shadow mode for a new mandate version.
- **E15-5 (Should, after the Phase 1 exit)** As an owner, I want to run up to three variants of my
  mandate at once, one live and the others in shadow, so that I can compare strategy changes before
  risking money ([strategy option 16](../product/10-strategy-options.md#option-16-mandate-experiments-multi-variant-shadow-mode-founder-2026-09-27)).
  Subsumes E15-4 (one shadow candidate is the one-variant case) and is pulled forward from Phase 3;
  no change to the v1 milestones. *Accepted when:*
  each variant is its own owner-confirmed mandate version that differs only in strategy fields (signal
  models, weights, thresholds, cadence); shadow variants see the same market data and pass the same
  gate, evaluated against their own simulated account state, never consuming or holding the live
  account's buying power, day-trade budget, or reservations (so no shadow variant can hold a live
  exit, rule 13); they keep a simulated book labelled as simulated, any comparison between variants is
  labelled as hypothetical performance, and they send nothing to the broker; each variant's
  hypothesis and success criterion are journaled before it runs; promotion is only an owner action
  that creates a new confirmed mandate version, with no automatic winner-picking; and no mandate holds
  more than three variants.

### E17 Research agent and dynamic universe

Design: [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) ([DEC-97](04-decision-log.md#decisions)).
The mandate spec, schemas, reference implementation, and cases are rewritten first in spec-change PRs.
The first delivery is the [DEC-103](04-decision-log.md#decisions) thin slice: the research agent
runs only in the team's internal paper workspaces, with the research basket (DEC-90) as its fixed
test data universe, every admission `ask`, paper only, and scorecards on. The basket is never a
user's instrument choice. The full E17-3, for users' agents under their own envelopes, follows only
after the DEC-99 evaluation (E17-8) passes on the thin slice.

- **E17-0 (Must, now)** research spike: an LLM loop over news and prices, paper-traded on the
  research basket with fixed sizing, to de-risk E17-2 before it is product code
  ([task brief](tasks/RS-1-research-spike.md); `python/research_spike/`).
  *Accepted when:* a report with hit rate, expectancy versus SPY, and cost per thesis after two to
  three weeks of paper trading.
- **E17-1 (Must)** As an owner, I want the mandate split into envelope fields I confirm and a working
  universe the platform produces at runtime, so that I set the risk and the agent brings the ideas.
  *Accepted when:* the rewritten mandate spec's cases pass; the compiler may propose envelope values,
  each marked as proposed; no envelope field activates unconfirmed.
- **E17-2 (Must)** As an owner, I want a research agent that turns market data, news, filings, and
  the agent's memory into theses (instrument, direction, horizon, evidence, invalidation), journaled
  as `ThesisProposed`, so that the agent has ideas without me.
- **E17-3 (Must)** As an owner, I want instruments admitted into the working universe only through
  the eligibility floor, the policy's asset classes, `max_instruments`, instrument-group claims, and
  my autonomy rules (`new_instrument`, `thesis_confidence`; default `ask`), journaled as
  `UniverseChanged`, and removed to exits-only when a thesis is invalidated.
  *Accepted when:* simulation fuzzing over random theses never admits an ineligible instrument or
  exceeds the envelope; prompt-injection fixtures never reach an order.
- **E17-4 (Must)** As a fund, I want a bring-your-own-strategy mode that pins the universe and
  disables the research agent, so that today's behavior stays available.
- **E17-5 (Must)** As an owner, I want the input-drift detector (`unusual_input`, V-018) so that
  unusual inputs escalate before the research agent acts on them
  ([DEC-101](04-decision-log.md#decisions)).
- **E17-6 (Must)** As an operator, I want to see and stop research-agent flow across the accounts
  of a deployment, so that one thesis cannot concentrate orders from many accounts in one instrument
  unnoticed ([DEC-100](04-decision-log.md#decisions)).
  *Accepted when:* no workspace's gate reads another workspace's state (a two-workspace test shows
  one's positions never change the other's decisions); the aggregate-flow monitor sums research-agent
  exposure per instrument over its deployment's workspaces, in dollars and as a share of average
  daily dollar volume, alerts the operator above the thresholds, writes to no workspace, and sends
  nothing to the global control plane; the operator per-thesis halt, journaled in each workspace as
  a `PlatformOperatorAction`, stops matching research-agent admissions and openings in every
  workspace of the deployment while exits and protection continue, and never permits anything a
  workspace's own limits deny; openings on a new thesis wait for the workspace's deterministic
  stagger offset within the conduct controls; bring-your-own-strategy agents keep per-account
  controls only.
- **E17-7 (Must)** As an owner, I want the research agent to read only vetted sources and to admit an
  instrument only on corroborated evidence, so that one planted source cannot admit an instrument
  ([DEC-101](04-decision-log.md#decisions)).
  *Accepted when:* the source allowlist is versioned configuration; the agent reads no source outside
  it; an admission without corroboration by an independent source, or by market data consistent with
  the thesis, is rejected; the corroboration is recorded in `ThesisProposed`; prompt-injection
  fixtures for every input source never reach an order.
- **E17-8 (Must)** As the founder, I want a forward paper evaluation harness, so that thesis quality
  is judged on outcomes the model cannot have seen ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* the evaluation window, metric, and pass threshold come from a recorded decision
  made before the evaluation starts; it runs on the team's internal paper workspaces (DEC-103), so
  no user's results are aggregated; every thesis is scored after its horizon against buy-and-hold of
  the eligible basket and a broad index ETF, net of the cost model; the report states pass or fail
  against the threshold and is reproducible from the journal.
  *Follow-up (#410 review, minor 3; DEC-282 item 9):* an evaluation whose scoreable set is empty
  against a registered `minimum_scoreable` of zero refuses with `Num(DivisionByZero)` — fail-loud,
  but a code that tells a caller nothing. A tests PR names the refusal (a `ResearchError` arm of
  its own; the registry's codes are add-only) so the empty report carries its reason, with the
  frozen surface otherwise unchanged.
- **E17-9 (Should)** As an owner, I want the research agent to revise a thesis that failed on
  forward paper, with its autopsy recorded, so that the platform improves its ideas without hiding
  its failures ([DEC-111](04-decision-log.md#decisions)). *Accepted when:* a revision is journaled
  as `ThesisRevised` linked to its predecessor and names the failure it addresses; it starts with an
  empty scorecard and is scored only by the E17-8 evaluator; it passes the eligibility floor,
  corroboration, and the autonomy rules like a new thesis and cannot loosen any envelope field; past
  `max_revisions_per_lineage` the lineage is retired and the owner is told. Depends on E17-8 and
  on one completed DEC-99 evaluation on the DEC-103 thin slice.

### E16 Kraken Derivatives US connector (Phase 3)

- **E16-1 (Should)** As an operator, I want to connect a Kraken Derivatives US account (demo and
  live) with a trading-only key so that agents can trade CFTC-regulated crypto perpetuals.
  *Accepted when:* keys with withdrawal or transfer permissions are rejected before storage.
- **E16-2 (Should)** As a trader, I want perpetuals accounting (funding every eight hours,
  margin, liquidation thresholds) so that perpetual P&L and risk are correct.
- **E16-3 (Should)** As an operator, I want a funding/carry signal model for perpetuals.

## Won't (v1)

Live retail trading before counsel signs off; users outside the US; options; Interactive Brokers and Coinbase connectors; native mobile apps; WebAssembly plug-ins; SAML and SCIM;
fully on-prem control plane; shared data plane; strategy marketplace.

## Later (wanted after v1)

- **Custom signals for power users** ([DEC-20](04-decision-log.md#decisions)). Today a mandate chooses the
  platform's registered signal models, their declared parameters, fixed weights, thresholds, universe,
  autonomy, protection and cadence, but it cannot define a new signal formula, combine signals other than
  by fixed linear weights, or change sizing. Two steps, in order:
  1. A **declarative signal-expression layer**: arithmetic and comparisons over the platform's indicators
     and data, stored in the mandate, validated, bounded, and replayed deterministically. It keeps the
     mandate checkable.
  2. **WebAssembly plug-ins**, only if step 1 falls short: sandboxed, resource-limited, versioned by content
     hash, with every output journaled. A plug-in emits only an opinion (a signal or a thesis), which enters
     through the eligibility floor, the autonomy rules and the risk gate like the platform's own ideas. It
     never sizes or places an order (rule 4).

## Enterprise harness (proposed, DEC-149)

Epic **E18**, from the [harness engineering research](../product/11-harness-engineering.md#7-recommendations)
of 2026-09-27. [DEC-149](04-decision-log.md#decisions) makes the harness (gate, autonomy rules,
journal, executor, connectors, conformance suite, and MCP channel) an enterprise product that the
retail platform runs through. Every story here is **(Proposed, not scheduled)**: none is in the epic
overview, none is assigned a milestone, and v1's milestones do not change. **SC** marks a story that
touches a safety-critical path (gate, autonomy, journal, executor, connectors, credentials, auth, or
tenant isolation): the `AGENTS.md` safety-critical rules apply to it (tests written or verified
against approved reference cases first, property tests for its invariants, an independent review by
an agent on a different model, zero missed mutants, and green CI).

- **E18-1 (Proposed, not scheduled; SC)** As a broker or fintech, I want a versioned tenant policy
  overlay on my customers' mandates, with inline tests, so that my house rules apply to every agent.
  *Accepted when:* the overlay can only tighten a mandate, never loosen it (as E9-3's org limits
  do), and property tests prove that for any mandate and overlay the effective limits are at least
  as strict as both.
- **E18-2 (Proposed, not scheduled; SC)** As an enterprise SRE, I want OpenTelemetry GenAI spans
  derived from the journal so that agent runs show in my tracing stack. *Accepted when:* spans are
  derived from journaled events only, content attributes are off by default (no order details,
  positions, or mandate content leave the deployment unless the customer turns them on, rule 6 and
  "Do not"), and the semantic-conventions version is pinned.
- **E18-3 (Proposed, not scheduled; SC)** As a compliance officer, I want SIEM export and a signed
  per-period evidence pack so that I can file the period's record. *Accepted when:* the pack holds
  the chain segment, anchor proofs, mandate versions, gate verdicts, and reconciliation results,
  verifies offline with `journal verify`, and exports in at least one documented SIEM format.
- **E18-4 (Proposed, not scheduled; SC)** As an agent builder, I want the MCP channel to be a policy
  enforcement point so that my agent can act only through Mandate's rules. *Accepted when:* it
  accepts only tokens issued to Mandate and never passes a token through to a broker or another
  service; its tools are intent-shaped (`propose_intent`, `explain_verdict`), never a raw
  `place_order`; scopes start read-only and elevate incrementally; and tool schemas are pinned, failing
  closed on a change. Builds on E10-6.
- **E18-5 (Proposed, not scheduled; SC)** As an owner, I want each mandate version and each ASK
  approval signed with my step-up credential and journaled, in the manner of AP2's signed mandates,
  so that my intent is provable. *Accepted when:* an unsigned or wrongly signed version or approval
  is refused and journaled, and the signature verifies from the journal alone. Any legal wording
  about what a signature means is reserved for the founder (DEC-79).
- **E18-6 (Proposed, not scheduled)** As a connector or agent vendor, I want the conformance suite
  packaged as a certification kit so that I can show my integration is safe to connect.
  *Accepted when:* a third party runs it against its connector or agent and gets a report of pass^k
  over seeded fuzz runs, reproducible from the seed.
- **E18-7 (Proposed, not scheduled)** As a buyer, I want an adversarial bench so that I can see what
  a compromised model can do. *Accepted when:* with injected news, filings, and tool outputs, it
  measures the limit-breach rate given a fully compromised model (every model output adversarial),
  and the pass condition is zero breaches.
- **E18-8 (Proposed, not scheduled; SC)** As a platform operator, I want tenant isolation as a
  checked invariant so that no tenant can read or affect another's state. *Accepted when:* each
  tenant has its own chains and anchors, type or layer rules make cross-tenant access
  unrepresentable where possible, and a cross-tenant fuzz finds no leak.
- **E18-9 (Proposed, not scheduled; SC)** As an enterprise admin, I want OIDC and SAML SSO, SCIM,
  and AUDITOR and SUPERVISOR roles with ASK routing to supervisors so that the harness fits my
  identity and supervision model. *Accepted when:* each role's permissions are tested, and an ASK
  can route to a supervisor without letting anyone approve their own proposal. SAML and SCIM stay
  after v1 ([DEC-18](04-decision-log.md#decisions)).
- **E18-10 (Proposed, not scheduled; SC)** As a regulated firm, I want a self-hosted or VPC
  deployment with a pluggable external anchor (my WORM store or a transparency log) so that my
  records stay under my control. *Accepted when:* anchors written to the customer's store verify
  with `journal verify`. Relates to E13 and strategy option 15; a fully on-prem control plane stays
  Won't (v1).
- **E18-11 (Proposed, not scheduled; SC)** As a quant team, I want a sandbox for my strategy code so
  that it can propose but never trade. *Accepted when:* sandboxed code has no broker egress and no
  vault access and can emit only intents, which enter through the builder, autonomy rules, and gate
  (rule 4). Relates to DEC-20's later WebAssembly plug-ins.
- **E18-12 (Proposed, not scheduled; SC)** As a risk officer, I want sequence and flow policies over
  journaled intents so that order splitting and churn are caught. *Accepted when:* a split order
  that would breach a limit as one order is denied, and the policy reads journal state only.
- **E18-13 (Proposed, not scheduled; SC)** As an auditor, I want `mandate replay <range>` so that I
  can reproduce every gate verdict in a range. *Accepted when:* replay from the journal gives
  byte-identical verdicts, and any difference is reported, never silently accepted.
- **E18-14 (Proposed, not scheduled; SC)** As an agent builder, I want a denial to carry its reason
  codes and the tightest compliant alternative so that my agent can recover without guessing.
  *Accepted when:* the alternative is computed deterministically, passes the gate against the same
  state if proposed, and never adds risk beyond the denied request.
- **E18-15 (Proposed, not scheduled)** As a compliance buyer, I want a published compliance mapping
  so that I can trace each control to evidence. *Accepted when:* each control maps to a test ID and a
  journal event type, and CI fails if a mapped test or event type disappears. Its legal and
  compliance wording is reserved for the founder and counsel (DEC-79).

## Spec follow-ups (minor review findings, deferred by the freeze rule)

From the final review of mandate spec v0.3:

- Tie the loss carry to the broker account rather than `connection_id`; show the carry and its
  expiry at deployment.
- On the `disarm_ladder` confirmation screen, state that the lifetime floor becomes the only
  automated limit, as a percentage of the held position.
- Set a platform or retail minimum for `scale_lift_after_s` (at 0, stepwise lifts happen at once).
- Pace `trim_to_target` sells like discretionary exits (participation caps).
- Define whether the 1.25× floor hard level scales C × f or the remaining loss budget when L > 0.
- Harness default for `first_trade_in_instrument` (position quantity) versus the spec (no prior fill).
- Show the hard-trigger multiple and the 90-day carry window on the confirmation screen.
- Add fuzz coverage for multiple agents, trims, and owner exits.

From [ADR-0003](../adr/0003-earned-autonomy.md)'s guardrails (DEC-185 to DEC-197), spec changes that tighten, each with its invariant
fuzzed and seeded bugs caught before code:

- `autonomy.tripwires` (DEC-187): conditions over recorded outcomes, actions `end_delegations` or
  `exits_only`, risk-reducing to add; MI-31 and V-044.
- `autonomy.review_by` (DEC-188): a §7 platform default of 90 days, at most 180; past it every `auto`
  and delegation reads as `ask`; MI-32.
- The delegation shape chosen on `ApprovalResponded` joins journal spec §9.1 when the approval
  events close (M7); `DecisionMade`'s delegation and client members are closed (DEC-252).
- The per-client ask budget (DEC-195, DEC-251): at most 10 client-requested asks per client per
  risk day, which the owner may lower, suppressed as `client_budget` after §6.4's per-agent `budget`; MI-33.
- The delegation total (DEC-196): the sum of `max_total_usd` over a version's delegations is at most
  `capital.allocation_usd`; V-045.
- The unasked-dollars figure (DEC-189): its formula in mandate spec §4.2 beside the confirmation
  screen's worst-case figures, with a reference-model function the web figure is tested against.

From the independent reviews of stream J's implementation (`mandate-research`, #158 and #159):

- `reference/mandate/ref.py`'s `lineage_fold` starts from an empty lineage map and ignores the
  `lineages` input, while `fold_theses` continues from the `LineageState` it is given. No case
  exercises the difference; align `ref.py` or record the continuation in DEC-132.
- `ref.py`'s `T()` reads instants to whole seconds while the crate compares nanoseconds, so the two
  differ one nanosecond past a horizon. The crate is right; every fixture uses whole seconds.
- `ref.py`'s `_group_claimed` defaults an ungrouped instrument's group to its asset id, so a group id
  spelling that asset id claims it; DEC-132 item 12 and the merged test admit it. Align `ref.py`.
- `ResearchError::Unimplemented` is returned by no entry point, but stays until
  `crates/mandate-research/tests/rules.rs` stops constructing it (a tests correction).

From E10-3's implementation (DEC-172 items 1 and 12):

- Align `reference/mandate/ref.py`'s `classify` with the crate where the crate reads more strictly or more exactly: pinned instruments compared by whole entry (a symbol or asset-class change is an added instrument), an absent member and a `null` one reported as a changed path, and a same-set reordering of `asset_classes` neutral. No MC-C case exercises any of the three, and the reference's reading of the first is the one that could skip step-up.
- From the review of #318 and #319, three minors for the next `mandate-spec` tests correction:
  - replace the four near-identical `ValidationContext` fixtures in `tests/change.rs`, `tests/validate.rs`, `tests/goal.rs` and `tests/risk.rs` with one `validation_context()` in `tests/common/mod.rs`;
  - give `tests/change.rs` a `runner_with(cases)` instead of the MI-11 property's inline copy of `runner()`'s four fields;
  - `ChangeClass`'s derived `Ord` is now what `join` relies on (DEC-172 item 8). Only `the_class_order_is_the_severity_order` and four MC-C cases catch a reorder, so either give the variants explicit discriminants or point the enum's doc at that test.

From the independent review of E10-1's slice-S implementation ([#225](https://github.com/kunwarshivam/mandate/pull/225)
round 1), as the coordinator ruled there:

- `tests/document.rs`'s `two_documents_that_differ_only_in_order_hash_the_same` parses one value twice,
  and a canonical `Object` is a `BTreeMap`, so it pins determinism, not the order-independence its name
  claims. Fix it in the next tests correction that touches the file, from key-shuffled JSON text read
  through `mandate_canon::parse`.
- Done in E10-3's tests PR (`tests/change.rs::the_version_vector_is_pinned_by_its_literal_digest`):
  a live `mandate-spec` test now pins `btc_accumulator`'s literal `sha256:9fb03f7e…`, so a
  non-canonical writer no longer survives the canonical tests.

- `tests/vocabulary.rs::every_error_variant_has_its_own_stable_code` lacks `(ParseError::Diverged, "diverged")`.
  The code is pinned by the module test `a_mandate_changed_after_parsing_has_no_version`, but not in the
  table that asserts one code per variant. Add the row in the next tests correction that touches the file
  (#225, the coordinator's note after merge).

From E10-1's slice-V implementation (DEC-161):

- **`reference/mandate/ref.py`: `violates` reads a level's `null` as a limit** (#263 round 2). For
  `two_approver_above_usd` a level stating `null` should state nothing (DEC-128 item 30(b)); `ref.py`'s
  `violates` compares against it. Align the reference with the crate.
- **`PolicyOverlay::effective`'s `(Some(ceiling), Absent)` arm returns the ceiling without checking its
  type against the key** (#263 round 2, minor). A wrong-typed ceiling is refused elsewhere
  (`invalid_input`, item 30(d)); this arm should refuse it too.
- **Stream H: a gate holding a `Purpose` narrows only through `AddingPurpose::try_from`** (#263 round 2,
  nit 2). `PolicyOverlay::narrow` takes an `AddingPurpose`, so an exit's built-in AUTO cannot reach it
  (DEC-128 item 30(i)); the builder and the gate must convert with `try_from` and leave an exit's
  decision untouched on `Err`, never map an exit onto `Open` or `Increase`.
- **`ValidationContext::from_journal` (stream F, DEC-169):** implemented; its 17 tests are live.
  Stream L's E7-10 (DEC-168) maps the records to `JournaledFact`: `AccountSnapshotRecorded`,
  `ConnectionEstablished`, `ConnectionRevoked`, `DisclosureAccepted`, `AgentDeployed` and
  `MandateVersionApplied` (both `AgentVersionActive`), `UniverseChanged`, `AgentStopped`,
  `ConfigSnapshotRegistered`, `PlatformOperatorAction` (`model_withdrawn`), `MandateVersionCreated`, and
  `MandateConfirmed`. `AgentFlat` needs a source there too (the account ledger's flat-in-every-instrument
  signal). A record left unmapped is a fact the fold never sees, so the mapper's completeness is what
  covers the facts that only add (DEC-169 item 2).
- **MC-V status PR (stream F, after the E17-1 slice):** V-003, V-034 to V-037, V-039, W-006, and
  `worst_case_stop_distance` landed in their own slice (DEC-161 items 1 and 10), so all 67 MC-V cases pass
  locally; a status-only PR moves them to `passing` (DEC-77 item 3).
- **Confirmation-screen PR: the stop distance and the figures disagree on precision** (#252 review,
  minor 3). `worst_case_stop_distance` sums at a `Ratio`'s 24 places, while the four figures stop at
  `Fraction`'s 9 (`out_of_range` past it, DEC-161 item 3). A document with a 10- to 24-place stop gets a
  distance but no figures. The screen that shows both must pick one boundary.
- **Blocks any production caller of `validate`: an unmentioned envelope path reads as confirmed**
  (#252 review, minor 4; the coordinator's ruling there). `ProvenanceMap::at` defaults an unmentioned
  path to `user_entered` and confirmed, and V-020 and V-022 read only the entries present, so a
  document with no entries passes both, even with `admission: auto`. The fix lands in the
  `ValidationContext::from_journal` implementation PR (DEC-169): an unmentioned envelope path is
  unconfirmed, fires V-020, and an `auto` under it fires V-022. No production caller may use
  `validate` or `ValidatedMandate::new` until then.
- **Stream H:** `ConditionField::is_unit_bounded` and `mandate-builder`'s `well_typed` omit
  `thesis_confidence`, which §6.3 types "decimal in [0, 1]"; `validate` bounds it (DEC-161 item 5), so the
  order path's re-check is looser than the load check. Fix both with the builder's
  `oracle_is_unit_bounded` in one change.
- **`reference/mandate/ref.py`:** `PLATFORM_DEFAULTABLE` gives `leveraged_etp_disclosure_version` the value
  `None`, which the reference reads as "any value"; §7 allows only `null` (DEC-161 item 8). Give the
  reference a sentinel for "any value" so `None` can mean `null`.
- **W-005 misses a wrapped catch-all** (#238 review, round 1, minor 1). `Condition::is_catch_all` reads only
  a top-level `purpose in [increase, open]`, so `{"all": [{"field": "purpose", "op": "in", "value": ["open",
  "increase"]}]}` and `{"all": []}`, which also match every action a later rule could, warn of nothing.
  Warning-only, never blocking; widen it to any condition that holds for both purposes.
- **`validate::tests::oracle_default_allowed` returns `true` for `/environment` whatever its value**
  (#238 review, round 1, minor 1), so the V-020 property never exercises §7's `paper`-only bound there
  (`v020_reads_the_source_the_confirmation_and_the_listed_value` does). Make the oracle check `paper` in a
  later tests correction.
- The worst-case figures multiply by `Fraction`, which holds nine places, so a schema-valid fraction with
  ten or more is `out_of_range` (DEC-161 item 3). Move to an exact `Usd × Ratio` when `mandate-num` has
  one (stream H's `UsdExact` is the candidate).

From the independent review of E4-2's implementation ([#163](https://github.com/kunwarshivam/mandate/pull/163)
round 2, verdict approve), whose first two minors are closed by the third tests correction
(DEC-127 item 26) and whose third waits on another story:

- `NumError::Unimplemented` stays: it is returned by the fourteen `mandate-num::sizing` stubs E6-2 owes,
  so it is live code, not a leftover, and its row in `num::error_codes_are_stable` holds the wire
  spelling those stubs return. Drop the variant and the row together in a tests correction once E6-2's
  sizing is implemented and nothing constructs it.

From the independent review of stream H's tests PR (`mandate-builder`, [#175](https://github.com/kunwarshivam/mandate/pull/175)
round 2, verdict approve), each deferred by the freeze rule and none of them a gap in what the tests
assert:

- **Floor the accumulate goal clip per bound, not as a whole.** `properties::accumulate_coverage_reached`
  requires a goal clip; it does not require each of §8.3 step 4's three bounds to have been the binding
  one. The review measured them over 256 cases: the remaining quantity bound 151 times, the spend 7 and
  the average price 2. A floor at 2 in 256 would be flaky across seeds, so the fix is not a counter on
  its own — the `Accumulate` shape has to make each bound bind reliably first, which today it cannot,
  because it always sets `max_avg_price` to the ask and so leaves `a − max_avg × β` at zero and the
  average-price **clip** off (only its guard runs). Each bound is pinned exactly by
  `hand::accumulate_clipped_to_the_remaining_target_quantity`, `hand::accumulate_clipped_by_max_spend`
  and `hand::accumulate_clipped_by_max_avg_price`, so this buys explicit fuzz evidence, not new coverage.
- **Carry fees in the rational sizing oracle.** It takes `a = ask` and `β = 1`, so the fee arithmetic of
  §8.3 step 4 is pinned by `hand::accumulate_with_fees_counts_the_spend_and_the_quantity_received` and
  `MC-B28` rather than by the oracle. Widening the oracle means widening the generator to fee rates,
  which changes the rational magnitudes the `i128` oracle carries; worth doing deliberately.
- **The coverage counters are process-global statics.** Under `cargo test`'s shared process another
  property's cases could feed the gate. `cargo nextest` gives each test its own process and is what
  both `cargo xtask check` and CI run, so the gate is sound as used; a per-run counter would make it
  sound under either runner.

From the independent reviews of stream K's tests (`mandate-executor`, `mandate-alpaca`, #152):

- Key the reference-case partition check (`refcases::the_fixture_partition_is_driven_plus_dropped`)
  to the scopes the suite actually drives, not to the `executor` and `reconciliation` labels. Five
  of the sixteen driven entries sit outside that filter (`RC-06`'s
  `protective_orders_kept_through_dividend` is `accounting`; `RC-15` and its three variants are
  `gate`), so a new `gate`-scoped case still leaves the suite silently; renames and removals of
  every named entry are already caught (round-3 review finding 3).
- Normalize a crypto symbol `mandate-alpaca` reads back from the broker. Position paths now write
  `BTC/USD` as `BTCUSD` (`http::position_path`, the positions read and the account-wide close), but
  `wire` keeps whatever symbol a response names, so a position reported as `BTCUSD` and an order
  placed as `BTC/USD` would be two instrument ids, where trading-domain spec §2.3 makes them one.
  Only a recorded crypto position shows which form the paper host sends; normalize on
  `asset_class: crypto` before E7-3's reconciliation compares crypto positions.
- Read a working external notional order's exposure. `wire` ingests an external order placed by
  notional with `qty` equal to its `filled_qty`, so a working one understates what it can still
  buy, and its `notional` is read nowhere and is not in `record::RECORDED_FIELDS`. The exposure is
  bounded meanwhile: external activity puts every agent on the account in `exits_only` and blocks
  claiming the instrument until the owner acknowledges it (trading-domain spec §7.1), so no agent
  adds risk beside it (#195 review, round 1, finding 6).
- Make "only the fold writes folded state" a type guarantee in `mandate-executor`. `ExecutorState`'s
  fields are `pub(crate)`, so the rule that only `fold.rs` writes folded state and `step.rs` writes
  only the process-local fields (the epoch, `started`, the latest tick, the unresolved append) is a
  convention the review holds. A `FoldedState` newtype with private fields, written only through the
  fold and read through accessors, moves it to rung 1 (#194 review, round 1, finding 5).
- **E7-7, once stream E registers agent-stream payload schemas:** move `mandate-shell`'s
  committed-draft ledger from `mandate_canon::parse` to `mandate_journal::Draft::parse`, the
  oracle the brief names, and run `verify_events` over the in-module keystone's streams. Today no
  agent-stream event parses there (DEC-157 item 7; #227 review, round 1, minor 3), and the
  executor's account-stream drafts do not match the registered schemas either (DEC-174 item 5).
- **E7-7, blocking the slice that lets the crossover drive an order:** bound the stored bars'
  staleness. Check the span's last day against the run's `setup.now` (the last completed session
  before it) and refuse coverage that ends earlier. Today `Bars::closes` reads no clock, so a
  months-old dataset is trusted and feeds the signal. That is harmless only while every downstream
  stage refuses (DEC-166; #241 review, round 1, minor 5).
- **E7-7, unowned, blocking `tests/tracer.rs::outlier_close` (PB-15):** a market-data trust rule
  that refuses a close too far from its neighbours. The shell may not judge one, because that is
  price arithmetic (DEC-138 item 3, DEC-166 item 5). The test stays pending until an owner lands the
  rule in `mandate-marketdata`, or until E6-8's mark-and-collar, in the gate since DEC-163,
  refuses the limit end to end (the coordinator's ruling on #171).
- **E7-7, when streams F and H land:** a drift check for
  `crates/mandate-shell/tests/fixtures/tracer/generate.py`, like `reference/mandate/generate.py`'s,
  so the fixture's one share at 255.20, AUTO by `rule:routine`, stays recomputed from the rules
  (#227 review, round 1, minor 4).
- **Before stream G's gate is wired in:** register `startup_reconciliation_pending` in the
  trading-domain `reason_codes` registry, or record why not. It is a third partial-gate reason code
  outside the registry, beside `instrument_not_in_universe` and `broker`, so ES-09's stable reason
  codes do not yet cover what the partial gate journals ([DEC-129](04-decision-log.md#decisions)
  items 23 and 27, ADR-0001 ES-09; #206 review).
- **Before E6-10's implementation merges:** register `crypto_pair_not_usd` in the trading-domain
  `reason_codes` registry and name it in §3.2 item 7, or record why `not_in_working_universe`
  stands. Mandate spec §5.3 defines that code as "the instrument is in the working universe", which
  a BTC/USDT pair the research agent admitted is, so the journaled denial would say something false
  about it. Registering a code adds no risk and closes a gap, so DEC-176 lets an agent do it in its
  own spec PR; the tests' `PAIR_CODE` constant flips with it ([DEC-254](04-decision-log.md#decisions)
  item 3; DEC-129 items 25 and 27; #342 review, minors 1 and 2). *Done (#352,
  [DEC-255](04-decision-log.md#decisions)):* the code is registered and §3.2 item 7 names it; the
  gate's emission and the `PAIR_CODE` flip stay with E6-10's implementation.
- **The broker symbol's quote currency is read exactly** (E6-10; #342 review, minor 3). The gate's
  USD-pair rule rests on the §3.1 loader mapping a pair to `QuoteCurrency`, and E7-8's
  `TradingClient::asset` criterion does not name it. The loader matches `USD` exactly and
  case-sensitively, with tests that `usd`, `USDT`, `USDC`, a padded code, and an absent symbol all
  land on `Other` or `None` (DEC-254 item 1).
- **E6-10's tests nits** (#342 review): `usd_pairs.rs`'s property sets `quote_currency` twice for a
  crypto draw; DEC-254 item 3's alternatives omit DEC-129 item 27's "assert the verdict, leave the
  code unasserted" option; and `cargo xtask ci pending` accepts any `Unimplemented` report rather
  than the story its `#[ignore]` label names, which is how `hand::crypto_never_counts` sat labelled
  E6-6 while failing at E6-10's stub. Compare the stub's story with the label if it recurs.
- **`crypto_pair_not_usd`'s wording follow-ups** (#352 review, minors 2 to 4 and nits), in one
  docs change after #352 merges:
  - DEC-255's opening parenthetical says DEC-254 item 3 is "not yet on `main`"; #342 merged as
    `e7c870b` before DEC-255 was written. Drop the clause (minor 2);
  - `docs/project/08-work-tracker.md` still names trading-domain spec v0.12; every earlier bump
    updated it in the same PR. Say v0.13 and cite DEC-255 (minor 3);
  - trading-domain §3.2 item 7 names the code only for a pair "quoted in anything else", but
    DEC-254 item 1 and `usd_pairs.rs`'s `NOT_USD = [Some(Other), None]` deny an unstated quote
    currency the same way. Say "quoted in anything other than USD, or whose quote currency is not
    stated" (minor 4);
  - the v0.13 change-history entry's "No existing code changes (ES-09)" means no registered reason
    code changes; say so (nit);
  - the E6-3 brief's check-2 row carries an inline parenthetical in an otherwise bare list of
    codes; the Story column already names E6-10 (nit).
- **DEC-253's mutation-scope wording** (#345 review, minors and nits), one docs change:
  - ADR-0001 ES-13 names `risk_gate.rs` and `order_builder.rs` as the recorded exceptions, but
    ES-13's limit is per change, so read alone it licenses a later 900-line change to either file.
    Say the exceptions are the two merged changes that added them, and that neither exempts a
    later change (minor 1);
  - the `verify-mandate` skill's rule 5 states that pending-only harness lines survive the gate but
    not the remedy. Add: drive the line from a live doctored-case test in
    `crates/mandate-refcases/tests/`, as `mandate_gate_harness.rs` does (DEC-253 item 2) (minor 2);
  - DEC-253 item 3 splits a large harness arm into stacked 400-line PRs without saying each slice
    must carry the doctored-case tests item 2 requires, or the gate fails it (minor 3);
  - the family-B survivors row calls the `listing` site `qty_increment < 1`; the code reads
    `stated.increment < one` (nit);
  - "the 14 mutants that survive in merged harness code" is what two sampled diffs found, not a
    census; say so where it is repeated (nit);
  - the xtask fixture names a crate `core`, shadowing `std`'s; `base` or `product` reads better
    (nit);
  - `mutated_crates` no longer parses `layer`, so a typo there surfaces in `lint`, not `mutants`;
    intended, recorded so it is not mistaken for an oversight (nit).
  Also worth knowing: a change that touches only `crates/mandate-refcases/tests/` never starts the
  gate, so a tests correction that kills survivors proves it by re-running the gate over the
  original diff locally.
- **The journal generator's mutant coverage** (#340 round-2 review, minors and nits), one change to
  `reference/journal/generate.py` and its docs:
  - the "every oracle check has a registered vector mutant" guard filters on
    `check.startswith("owner_copies.")`, so 20 of the 44 `ORACLE_CHECKS` entries (for example
    `kill_switch.stopped`, `goal_exit.origin`, `owner_exits.sell`, `owner_exits.user`) can be deleted
    with `--check` still green. Require a mutant for every entry, with an explicit allow-list for
    the chain checks that re-verify the generator with its own `canon` and `sha256_hex`
    (`chain.seq`, `chain.canonical`, `chain.hash`, `chain.stream`, `chain.opened`) (minor 1);
  - `received.intent` is its own family, which silences the masking rule rather than isolating a
    mutant, and it has no mutant of its own, so deleting its oracle leaves `--check` green. Add
    one only it catches; the review showed "intent recorded after the account stream copied it"
    (`recorded_at` of the `intent` body moved to `2026-09-21T14:00:00.900000000Z`) does (minor 2);
  - `VALIDATOR_MUTANTS` lists `rule.16.mode.owner_pause`, `owner_resume`, and `owner_stop` by hand
    while the skip key derives from the reason, so a fourth `OWNER_MODE_REASONS` entry would get no
    mutant. Generate the entries from `OWNER_MODE_REASONS` (minor 3);
  - DEC-177 item 25(b) and #340's description say three mutants were re-seeded; two were, and the
    third was renamed into `received.intent`'s family, its own mutant still owed (nit);
  - `docs/specs/journal.md`'s rule-16 prose has a 107-character line; rewrap it (nit).
- **Family B's clock fix follow-ups** (#351 and #347 reviews, minors and nits; the fold-back,
  [#359](https://github.com/kunwarshivam/mandate/pull/359), removed `CLOCKED` and closed three of them):
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359), which deletes the function):* `labels_agree_with_calendar` in
    `crates/mandate-refcases/src/mandate/order_builder.rs` reached its answer through the harness's own `mandate_risk::session_at`, `test_default_gate_config()`
    and `market_session`, so a bug in any of them (the close window set to 0, `AfterHours` mapped to
    `Regular`, or `session::derive` reporting `Regular` after hours) flips oracle and harness
    together; only the literal-clock test caught them. Compute the expected session from a literal
    New York clock, or give each `CLOCKED` row its agreeing instant (#351 review, minor);
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359)):* `labels_agree_with_calendar` re-walked `family_b` for a case its caller
    holds; take `&case` (#351 review, nit);
  - **open:** `reference/mandate/check_cases.py`'s `calendar_at` assumes a full trading day for
    every builder case. On 2026-11-26 (closed) and 2026-11-27 (early close 13:00, after hours to 17:00) it
    disagrees with `crates/mandate-time/data/us-equities.calendar` both ways. Refuse, naming the
    case, when `now`'s date is not a full day in that calendar, or read the calendar (#347 review,
    minor);
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359), now 25):* `.cursor/skills/verify-mandate/feature-map.md`'s family-B bullet
    said 23 pass and that MC-B22 and MC-B23 fail on a contradicting label (#347 review, minor);
  - **open:** `reference/mandate/generate.py`'s `at_now` moves only the outputs' `as_of` and
    `expires_at`, not `gate_state.last_exit_fill_at`; assert that map is empty, so a later caller
    cannot move `now` past a re-entry cooldown unnoticed (#347 review, nit);
  - **open (#359 review, nits):** DEC-250 item 12's earlier amendment still names
    `the_two_session_cases_pass_once_now_agrees_with_their_labels` and its MC-B23 doctoring, both
    gone; the renamed `the_two_session_cases_pass_as_stated_and_compare_every_sibling` repeats what
    `every_expected_member_is_compared_and_required` and `every_builder_case_passes_or_fails_at_its_owner`
    already run, so fold it into them or say in its doc that it only names the two comparisons; and
    `CLOCKED`'s needles asserted the conflict message's whole wording ("states `after_hours`", "the
    calendar says `regular`"), where MC-B01's doctored proof in `the_builder_and_the_gate_see_one_scene`
    asserts only the member name, so pin the full message there. Replace `order_builder.rs`'s own
    `fails_naming` with an exact-message helper, as #360 did for family A.
- **E7-4 slice 1's tests correction:** close the do-nothing gap in `mandate-executor`'s generator
  properties. 29 of the 33 pass when every reachable stub returns `Ok(())`, so a no-op executor
  would satisfy them; each property must also assert a positive effect a no-op cannot produce
  (#231 review, follow-up a).
- **Before E7-3's buying-power path reads them:** the properties' model broker reports `equity` and
  `buying_power` that follow its `cash_moved`, as `cash` already does; today they stay at 20000
  whatever its fills (#231 review, follow-up b).
- Assert the ready precondition in `mandate-executor`'s properties script: after its start, an
  account has been observed and the startup `ReconciliationRun` recorded, so a script that stops
  starting ready fails at its start rather than at a later assertion; and update `play`'s doc to say
  it starts ready (#231 review, follow-up c).
- **Blocks E7-4 slice 5 (the trading day):** `mandate-executor` must copy the cross-stream facts
  journal spec §2 gives it (`AgentModeApplied` from the agent stream's `AgentModeChanged`,
  `TradingDayStarted`, `ClockAdvanced` crossing midnight America/New_York, `OwnerAcknowledged` from the
  control stream), each with its `causation_id`. An acknowledgment whose step-up does not count is
  copied as `OwnerCommandRefused` instead (command `acknowledge`, the reason, `effective_at`), lifts
  nothing, and is tested first to leave exactly that event (DEC-291 item 4, journal spec §2). Before E7-4 slice 1, `step`'s `Input::Journal(_) => Ok(())` copied
  nothing, silently, so `properties::every_copied_draft_cites_its_origin` sees no copied draft under any
  script and passes vacuously. The slice that adds the producer also adds a generator step (a clock
  advance crossing midnight New York, an owner acknowledgment) and asserts `seen > 0` on scripts
  containing it, shown failing under the do-nothing plant (#244 round 1, finding 3). Until then
  `Input::Journal` answers a loud `Unimplemented { story: "E7-4" }` naming slice 5, landing first in
  E7-4 slice 1 rather than dropping the fact (the coordinator's ruling on #244, 5861479849).
- **E7-4, the slice that reconciles protective legs (stream K):** make `mandate-alpaca`'s `wire.rs`
  keep each leg's `client_order_id` instead of reading `legs[].id` only, with its own `ready()` tests
  correction first, since `BrokerOrder.legs` changes type (#229's pattern). E7-4 slice 1 aligns
  `ClientOrderId::for_protection` to the §2.3 grammar (`{entry}-p{protection}`, legs `-tp` and `-sl`)
  and reads no leg id from a `BrokerOrder`, which an in-module test pins. Until the wire change lands,
  a broker-reported leg is attributed by the single holder or fails closed for openings; exits are
  untouched ([DEC-160](04-decision-log.md#decisions) 3a, #243 round 1, the coordinator's ruling (b)
  on #174, 5861764910).
- **E7-4 slices 2 and 3 (stream K):** `properties::protective_sell_quantity_never_exceeds_the_position_in_any_script`
  wants the `ProtectionChanged placed` at or after the entry's completion with no lag. That is right
  on the normal path (§5.4's legs activate at completion), but a re-placement after a
  cancelled-then-filled entry may lag by up to `max_unprotected_s`. If a slice turns it red there,
  allow that bound rather than loosening the assertion elsewhere (#244 round 3, minor 3).
- **E7-4 slice 2's tests correction (stream K):** a `properties` oracle expects `OwnerAlertSent`
  among the executor's drafts, but it is a control-stream event the executor never writes; the
  executor's alert is its own record (`ProtectionChanged interval_limit`) plus `Effect::Notify`.
  Correct the oracle before slice 2 un-ignores it (found building slice 3a, #267).
- **E7-4 slice 2 (stream K), #242's plants that go live with it:** plant 7 (held quantity 10 → 5)
  and plant 8 (held limit 150 → 100), and plant 9 (`fault::protected` on a plain `restart`), which
  the coordinator moved from 3a to slice 2 (#267, comment 5862923162): under rule 13 no exit waits
  for the startup reconciliation, so the plant gets weight only with the first opening through
  `fault::protected` (slice 2's add). Slice 2's PR shows each of the three red.
- **E7-4 slice 6's tests PR (stream K), from [#373](https://github.com/kunwarshivam/mandate/pull/373)
  round 1 (major 1):** §5.5 exempts kill-switch and mandate-limit flatten exits from the agent's
  mode, but slice 4a's ladder stops stepping while the agent is `paused` or `stopped`
  ([DEC-260](04-decision-log.md#decisions) (3)), and the gate's `mode_failure` holds every
  risk-reducing order at `paused` or stricter. Add a pending test: while the agent is paused, a
  flatten's ladder steps, its step cancel does not end the sequence, and `mode_failure` lets that
  flatten through. Slice 6 narrows `climbs` and the gate together to make it pass.
- **E7-4 slice 6 (stream K; moved from slice 5 by the coordinator's ruling D3 on #174), from
  [#373](https://github.com/kunwarshivam/mandate/pull/373) round 1 (minor 1):** wire the owner exit's floor (`OwnerExitRequested`'s confirmed floor) into
  `exit_limit` and `next_rung`, which pass none today, so §5.6's "never below an owner exit's
  floor" holds on the live paths and not only in `ladder_tests`. A rung the floor clamps sets
  `at_floor` and rests (§5.5's "any remainder rests at the floor"), rather than being cancelled and
  resubmitted at the same price every `exit_step_s`.
- **E7-4 slice 5 (stream K), from slice 4b ([DEC-260](04-decision-log.md#decisions) (12)):**
  a crypto stop-limit is watchdogged as soon as a sane mark is below its limit price, once slice 5
  places stop-limits. The session condition landed with slice 5's session part (DEC-260 (15)).
- **E7-4 slice 6's tests PR (stream K), from slice 4b (DEC-160 (11), (24)):** add pending tests
  that every kill switch whose scope covers an instrument cancels a working `*` watchdog exit
  there by its own `client_order_id` (`md-w-<record>`), an agent-scoped one included when it
  closes that instrument (§5.5's table), and that an agent-scoped kill switch never treats it as
  that agent's own: it is never counted in the agent's sub-ledger sell and never reached by
  cancel-all.
- **E7-4 slice 5's session part, left open (stream K, DEC-260 (13), (14)):** the closing auction
  window is §4.3's 10-minute default as a constant in `mandate-executor`'s `session`; read it from
  the gate's effective-dated configuration (`close_window_minutes`) once the executor holds it. A
  presumed halt is read from the latest quote only (older than `exit_step_s`, or not sane); add
  the trading-status and LULD feed §4.4 names once the shell subscribes to it.
- **The US-equities calendar's end (stream K, DEC-260 (13); the coordinator's ruling on #174,
  5926945398):** alert the owner and the operator well ahead of `crates/mandate-time/data/
  us-equities.calendar`'s last valid date (2028-12-31), for example 90 days before it, so the file
  is extended before every equity exit starts being held `session_unknown`.
- **`mandate-executor`'s clock-0 tests onto the calendar (stream K, DEC-260 (13)):** the hand,
  coverage, fault and properties suites run at risk-clock seconds near 0 (1970), before the
  calendar's first date, which `session` reads as the regular session. Move them onto calendar
  dates in their own DEC-77 tests PR; after it, an instant before the calendar's range is held
  `session_unknown` like one after it. Among them `hand::a_risk_exit_submits_inside_the_close_window` runs at
  clock 25, so its venue is the regular session: it proves the conduct exemption, not the close
  window its name claims (#400 round 1, minor 4).
- **E7-4 slice 5's trading-day part (stream K), from the session part ([DEC-260](04-decision-log.md#decisions)
  (13)):** fold `TradingDayStarted` together with what the reference-case harness then reaches:
  §5.4's GTC re-placement at the buffer day (`hand::protection_is_re_placed_at_the_buffer_day`,
  `protection_is_not_re_placed_early`, `a_protective_order_submits_with_no_buying_power`), the
  harness's startup reconciliation, which today holds its openings
  `startup_reconciliation_pending` (RC-14 `add_via_bracket`, RC-15
  `restriction_from_a_closing_only_reject`, RC-21, RC-22), RC-14's journal order
  (`unprotected_window_start` expected after the gate), and the cases the fold alone lets pass
  (RC-07 `unposted_crypto_fees_reconcile`, RC-14 `passive_exit_becomes_oco_take_profit`, RC-15
  `status_not_active`, `external_order_detected`, `unexplained_403s`). RC-24 ×2 go live with it:
  the session part already prices them (both pass with the fold, shown in its PR). Restore
  `TradingDayStarted` to `properties::every_catalogue_event_is_interpreted_or_named`'s
  `INTERPRETED` in the same change (#400 round 1, major 3).
- **E7-4 slice 6 (stream K), from [#400](https://github.com/kunwarshivam/mandate/pull/400) round 2
  (the coordinator's ruling 5930410998):** a ladder that parks at the close
  ([DEC-260](04-decision-log.md#decisions) (18)) alerts the owner once, when it parks, for
  `session_closed` as for `session_unknown`. The alert carries generic text and an opaque id only
  (`AGENTS.md` rule 6), and the park also counts in what `exit_held_long` reports. Today a
  `session_closed` park journals its hold and raises nothing, so an exit can wait 8 hours
  overnight unannounced. Add a test asserting exactly one notification across a parked night.
- **E7-4 slice 6 (stream K), from #400 round 2 (nit):** the rule-13 oracle
  (`protection::sequence_tests::rule_13_holds_over_random_scripts`) reached the composed plant (the
  `extended_hours` widening with the old step path) 7 of 9 times. Raise the 19:59:50 start's weight,
  or add a fixed script, so it catches that plant on every run.
- **E7-4 slice 6 (stream K), moved from slice 5 by the coordinator's ruling D3 on
  [#174](https://github.com/kunwarshivam/mandate/pull/174) (5926142854):** the four kill-switch
  session tests (`hand::an_automated_flatten_defers_equity_sells_to_the_session`,
  `an_automated_flatten_sells_crypto_at_once`,
  `an_owner_exit_outside_the_session_prices_from_the_confirmed_bid`,
  `an_unconfirmed_owner_exit_waits_for_the_session`) and the owner-confirmed extended-hours path:
  outside the regular session an owner exit sells equities through the ladder only on the
  confirmed bid, bid size and floor, and waits for the session otherwise (§5.5).
- **E7-4 (stream K), from [#385](https://github.com/kunwarshivam/mandate/pull/385)'s review (minor
  4):** the rule-13 oracle's waiting-exit `limit` branch
  (`protection::sequence_tests::rule_13_script`) is dormant: no script holds an exit unexcused long
  enough to reach it. The follow-up either makes the branch bite, with a script that holds an exit
  unexcused and a plant it catches, or deletes it along with `quiet_since`.
- **E7-4 (stream K), found by slice 4a's rule-13 oracle (`protection::sequence_tests::rule_13_holds_over_random_scripts`):**
  (1) a passive exit waits on its OCO's cancel confirmation with no bound and no alert: a broker
  that never confirms holds the exit for good, with the protection still resting (rule 13's broker
  hold, so no risk is added, but rule 3 wants the wait bounded). Bound it as rule 5's wait is
  (`unknown_absent_window_s`, then query the order and alert), with a test. (2) `release_held`
  journals only an allow, so when a held exit's cause changes (paused, then nothing to price; or
  an allow that would now over-sell beside other exits) the journal's last verdict names a cause
  that has gone; journal a changed hold reason once. (3) A later rung is sized from the fold's
  filled quantity when the step's cancel is confirmed; a fill the broker reports after that
  confirmation (`LateFillApplied`) is not netted, so the rung could sell more than the position
  by that fill (rule 12). The same lag sizes §5.4's re-placement. Size from the broker's own
  report of the cancelled order, or hold the rung for it, before slice 2 lets protected
  positions exist outside the tests (DEC-160 (4)).
- **E7-4 slices 5 and 6's tests correction (stream K), from #286 round 1 (minor 2):**
  `properties::no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding` counts a cancel as
  outstanding until the order is terminal, abandoned or its protection cancelled, so it would fail on
  rule 5's ruled carve-out: an exit that goes once its opening's cancel is overdue, answered or not,
  with the opening still resting ([DEC-160](04-decision-log.md#decisions) (7), (13), (18)).
  Carve that case out through a DEC-77 tests correction before slice 5 or 6 lets the property run.
- **Trading-domain spec §5.7's missing `PendingCancel` edges (stream K), from #286 round 2
  (minor 3):** a spec PR that adds `PendingCancel → Expired` and `PendingCancel → Rejected` to
  §5.7's transition table, with reference cases, and then the executor change that follows it.
  Until then such a report is journaled and ignored ([DEC-160](04-decision-log.md#decisions)
  (13)), so an exit waiting on the order goes at rule 5's bound (item (18)), but the residue
  stays: a day order that expired, or was rejected, while its cancel was outstanding is left in
  `PendingCancel` and keeps its buying-power reservation until reconciliation or a later report
  moves it.
- **`mandate-executor`, from #286 round 2 (nit 1):** marking an accepted order's cancel overdue
  (`protection::overdue`) journals a self-transition, which `orders::transition` records as an
  attempted edge with `ignored: true` so the fold applies only `cancel_overdue`. The record reads
  as a refused transition. A dedicated flag-only record needs the fold's `Unknown` branch
  (which resets `unknown_since` and the absence count) kept out of it, so it is not a one-line
  change; do it with the §5.7 edges above.
- **`mandate-executor` fees (stream K), from #259 round 1:** (1) a typed `Environment` in place of
  the stream's environment text, so `paper_only_fee_config` refuses a live stream by its type
  (rung 1) rather than by a string comparison; (2) `mandate_accounting::Config` carries the
  schedule's `effective_from` and refuses to price a trade date before it, where today
  `fee_config` validates the date only against the calendar's range and then drops it;
  (3) the ruling's flat per-order cost for the paper-only overestimate (#174, 5861904579) cannot be
  represented, because `EquityFees` has no per-order field, so the overestimate is carried by the
  per-share and rate figures, each at least ten times the transcribed schedule.
- **E7-4:** gate `mandate-executor`'s `resubmit` for an order with no `intent_id`. It sends again without running the gate; no slice through 6 writes such an order, but protective orders will, so it must be gated before they ship (#202 review, the coordinator's ruling, comment 5857629810).
- Fold `crypto_status` in `mandate-executor`. `AccountStateObserved` journals it, and §7.3 requires it `ACTIVE` for crypto orders, but the fold keeps no field for it until the gate's crypto check reads one; the journal holds it, so the fold can add it without a new event (#198 review, round 1, finding 8a).
- Give a §7.3 account restriction in `mandate-executor` a lift path. §7.3 says a detected
  restriction stands "until the owner acknowledges and the account is refreshed", but
  `state::restriction_for` only yields `reconciliation:{subject}`, so no `OwnerAcknowledged` can
  name `account_trading_blocked`, and a later `ACTIVE` read leaves `account_state` `Blocked` and the
  restriction in place. Pre-existing on main; the cash slice is the first to raise it from every
  reconciliation run (#205 review, round 2, minor 3).
- Pin or drop the two unreachable overflow sites in `ExecutorState::buying_power`: the reservations sum and the final `min(model, broker) − reserved`. `Usd` is signed, so each fails only at the decimal range, which no reservation reaches, and replacing either `None` with zero passes every test; the reachable site, the model's cash, is pinned (#198 review, round 2, finding 3).
- **Blocks running the executor across a session boundary:** fold `TradingDayStarted` and
  `RiskDayStarted` in `mandate-executor`. Since #194's round 1 both answer the later slice's
  `Unimplemented` stub, so the first day rollover stops the executor, failing closed. The slice that
  owns the day fold (protection re-placement at the GTC buffer day, §5.4) must interpret both, move
  them back into `properties::INTERPRETED` with live tests that fail when either arm is stubbed, and
  land before the executor runs across a session boundary (#194 review, round 2).
- Report a safety-critical function whose only mutants are unviable. `ci mutants` counted the one
  mutant of `mandate-executor`'s `every_agent` (a body of `Ok(Default::default())`, which does not
  compile because `EventId` has no `Default`) as unviable, so "0 missed" said nothing about the
  function that enforces §7.1's `exits_only`. Such a function needs a named live test, and the gate
  or the review checklist should list every function whose mutants are all unviable, so a reviewer
  names that test (#196 review, round 1, finding 2).
- Question for the spec owner: does §10's simulated regulatory fee apply to an **unattributed**
  paper equity fill (external activity, §7.1)? §10 books paper's regulatory fees in the shadow
  ledger without distinguishing, and an external fill has no client order for DEC-87's per-order
  TAF cap. Since #196 the executor books one only for a fill attributed to one of its orders,
  stated in `orders::simulated_fee`'s doc; booking it too would lower paper buying power, the
  conservative side (#196 review, round 1, finding 7).
- Pin the calendar roll of the simulated `FeesCharged` event's `day` in `mandate-executor`. The
  hour cutoff is pinned (a fill at 20:00 New York belongs to the next trade date), but every fixture
  fill trades on a Tuesday, so `first_on_or_after(date, is_trading_day)` is the identity and a
  `day` taken straight from the broker's `trade_date`, bypassing the account's calendar, passes the
  suite. A fixture with a Saturday or holiday `trade_date` pins it (#196 review, round 2, finding 1).
- Reconcile journal spec §9's "daily snapshot" with `AccountSnapshotRecorded`'s cadence in
  `mandate-executor`: since the cash slice every reconciliation run that has a base records one
  (at startup, at each `Unknown`, at session boundaries, at fee postings). Either the spec line
  names every run, or the executor records the daily one only and carries the comparison's base
  another way (#205 review, round 1, finding 7a).
- Give `AccountSnapshotRecorded` one shape in `mandate-executor`: a fee posting with no cash base
  records it through `fees()`'s fallback without `model_cash`, `cash_band` or `cash_in_band`,
  while a run with a base records all three (#205 review, round 1, finding 7b).
- Answer an underflow in `mandate-executor`'s `asset_fees` with a typed error: a posted crypto asset
  fee larger than the one held is folded as `checked_sub(charged).unwrap_or(Qty::ZERO)`, which errs
  strict but silently, where the crate's convention is a typed error (#205 review, round 1,
  finding 7c).
- Wire the gate's buying-power port in `mandate-executor`. `ExecutorState::buying_power` and
  `crypto_buying_power` are computed and pinned but only tests read them; the gate still checks no
  buying power. The port must read the equity row for an equity order and the crypto row for a
  crypto order, and treat `None` (no account reported, an incomplete report, or an overflow) as no
  buying power, failing closed (`AGENTS.md` rule 3; #205 review, round 1).
- Make the reproduction line `ci pending` prints runnable as printed: quote the `-E` filterset
  (it contains parentheses, so a shell refuses it) and add `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`,
  which the gate sets and nextest requires for libtest JSON. Both fail loudly today rather than
  giving a different verdict (#199 review, round 2, minor).

From the independent reviews of stream I's implementation (`mandate-runtime`, #151), each deferred by
a coordinator ruling rather than left undone (DEC-131 item 25):

- Match a kill switch's lift to the acknowledgment **of that switch**. A confirmed `RiskLimit` switch
  is lifted today by a looser account-stream copy caused by any `OwnerAcknowledged` the fold has
  seen, because `KillSwitchActivated` does not name the limit that pulled it. When it carries its
  limit, the lift must match the acknowledgment's subject to the switch (round-4 review finding 1).
- Give a kill switch pulled while the agent is already `paused` a way to lift. It writes no
  `AgentModeChanged`, so it has no `mode_event`, nothing on the account stream can confirm it, and
  only a new deployment clears it. It fails closed, but two limits in one cycle then keep the agent
  paused until a redeploy (round-4 review finding 2).
- Shrink `outstanding` on a terminal account-stream outcome (`GateDecided` denial, `OrderAbandoned`,
  a terminal order state), which needs the executor's `client_order_id`-to-intent mapping from M6.
  Until then a restart after any order waits one clean `ReconciliationRun` before the agent can trade
  or propose a discretionary exit (DEC-131 item 25(d), round-2 review minor 3).
- Record an approval response that arrives in the same step as the tightening which cancels its
  approval as `refused` rather than `recorded`; nothing is handed either way, so it is record
  accuracy (DEC-131 item 25(j), round-1 review finding 9).
- Act on a granted ASK. `PendingApproval` binds the instrument, version, deadline, and whether the
  action adds risk, and carries no order body, so the runtime records a response and re-proposes at
  the next evaluation rather than placing the bound order; the bound content is M7's (DEC-131 item
  25(a)).

From the independent review of stream G's mandate limits (`mandate-risk`, #160):

- Give `Computed` the account's own figures (`account_gross`, `account_equity`), so an account-1×
  `gross_exposure_limit` denial journals the figures it compared; today it carries none and is told
  apart from the agent-limit denial only by `computed.gross` being unset. It is an addition to
  #136's public API, so it needs its own ruling.
- Settle what DEC-129 item 2's "`computed` blocks included" means: `ref.py`'s keys only, or every
  figure the gate computed. The flat `Computed` also reports `order_usd`, `instrument_total` and
  `cap` on branches whose reference block omits them, and `compare_computed` checks only the keys a
  case states, so nothing asserts either reading.
- Give §3.3's "organization ceiling" on the per-instrument cap a `GateConfig` field and an owning
  story; check 2 has no value to bound the mandate's cap with today.
- Pin that the **agent's** gross exposure counts only its own working openings
  (`limits.rs`'s `working_cost(input, None, false)`): the substitution `false` → `true` survives
  every live test (#160 round-2 finding 4). It is conservative, since the whole-account sum is a
  superset and can only deny an opening earlier, so it is not a rule-1 breach; one assertion in
  `the_account_bound_counts_other_agents_working_orders` would close it.
- Decide what `agent_flatten` does when `max_exit_offset = 1` (#176 round-1 nit 3): the floor
  `bid × 0` fails `positive()`, so the owner's kill switch returns `GateError` where `ref.py` plans a
  floor of `0`, the one way the function can refuse an owner kill switch (AGENTS.md rule 13, "the
  kill switch is always available"). Unreachable today, since §5.6's fixed tier table sets the
  offset at 0.03 or 0.05 and no code path sets it to 1; a fix either bounds the offset below 1
  where it is configured or plans the flatten without a floor.
- Correct the E6-3 brief's test names (#176 round-1 nit 4): the clause table (line 583) and mutant
  row 33 name `properties::an_agent_flatten_never_touches_another_agent`, which lives in `hand.rs`.
- Delete `crates/mandate-risk/tests/refcases.rs` once the families G and F status rows (DEC-178)
  have merged: `crates/mandate-refcases/src/mandate/risk_gate.rs` now runs MC-G01 to MC-G16 and
  MC-F01 to MC-F04 against the same `evaluate` and `agent_flatten`, and the file's own doc says it
  moves there. Its doc on `FULL_GATE_ONLY` still calls MC-G13 pending on E6-8, which #311 made
  live; deleting the file retires that too, and moving any figure it pins that
  `crates/mandate-risk/tests/hand.rs` does not goes in the same tests correction.
- Reconcile MC-G02 with its header under DEC-176: the header says "every other check passes", yet
  its working opening order in the proposal's own instrument trips trading-domain §5.3 rule 6
  (`working_order_limit`), which is why [DEC-150](04-decision-log.md#decisions) item 1 lists it on
  `FULL_GATE_ONLY`. Stating the same $1,300 as a position (`positions_mv` 1300, no working order in
  that instrument) keeps `instrument_total` and `gross` at 1500, so the case would pin `gross` on an
  allow path, the window DEC-150 records, and the entry could expire. Restated, MC-G02 becomes an
  allowed opening (an increase), so like MC-G13 it runs through checks 5 and 6 and check 7, and the
  full gate allows it: against the restated state the harness reports the entry expired
  (DEC-178 item 11). The same pull request deletes the entry, and `FULL_GATE_ONLY` with it if
  nothing else is listed. The change touches the YAML,
  the reference implementation's checks, and the regenerated fixtures, so it cannot share a pull
  request with code (ES-22).
- Done in the families G and F harness tightening ([#348](https://github.com/kunwarshivam/mandate/pull/348);
  `an_allowed_exit_the_gate_paces_fails_since_no_case_states_a_pacing`,
  `an_allow_fails_unless_it_reports_every_check_passed_in_order`; DEC-178 items 11, 12 and 14):
  the gate arm takes `Decision` apart whole and requires no `pacing` on every `gate` case, so the
  always-marketable `pacing` below now fails MC-G08 to MC-G10, MC-G13 and MC-G15; item 12 says why
  the five values are typed again; the allow pin has its own edit test and item 11 says it holds
  only for an allowed opening (checks 2 and 5 to 8 are not run for an exit); and group ids are built
  from the case's own names, so the unreachable rank error is gone. The row as it was:
  Tighten the families G and F harness (#317 re-review, minor 2 and nits 1 to 4), in one tests
  correction of `crates/mandate-refcases/src/mandate/risk_gate.rs` and DEC-178:
  - compare `pacing` as `None` on every allowed `gate` case and destructure `Decision`, so a new
    member does not compile until it is compared; today a `pacing` that always sets
    `marketable_limit_required` leaves every F, G and L case green, and only `mandate-risk`'s own
    tests catch it (DEC-178 item 14);
  - reword DEC-178 item 12: the five non-fixture fields are compared with the same values typed
    again, because `mandate-risk`'s `test_default_config` is test-only and another crate cannot
    call it;
  - add an edit test for item 11's pin on an allowed order's whole `checks` list, which today no
    harness test of its own guards;
  - note in item 11 that the pin is only meaningful for an allowed opening (MC-G13), since the gate
    reports checks 5 to 8 as `Passed` for an exit without running them;
  - drop the unreachable typed error for an unknown group rank in `Scene::read`, or state why it
    stays.
- Tighten the family-B harness (#331 review, minors 4 and 5 and the nits), in one tests correction
  of `crates/mandate-refcases/src/mandate/order_builder.rs` and DEC-250. *Done (E6-2,
  [#346](https://github.com/kunwarshivam/mandate/pull/346); DEC-250 items 16 to 18), all but the shared gate helpers,
  which stay open:*
  - *done:* `an_unreadable_input_is_refused_naming_it` now also sweeps the `gate_state.positions_mv`
    map and its values, `gate_state.last_exit_fill_at` (whole, and a planted entry, since no case
    states one), `gate_state.working_universe` and its entries, and each working order's
    `instrument` and `max_cost`, in the gate state and restated (minor 4);
  - *done:* a cash fee rate above zero is refused for crypto as for equities, since the gate's
    `fee_reservation` is `Usd::ZERO`; a stated 0 is read and passes (minor 5, DEC-250 item 17);
  - *done:* `crates/mandate-builder/tests/refcases.rs`'s module doc says this crate's harness hands
    `decide` the case's verdict because it does not depend on `mandate-risk`, and points at the
    shared harness's propose, gate and `decide` composition (nit);
  - **open:** `test_default_gate_config` and `gate_mandate` are a third copy of the gate helpers,
    beside `risk_gate.rs` and `trading_domain/gate.rs`. Share them, and when they are shared, add
    DEC-178 item 12's check against `configs.test_default.gate` to the family-B arm (nit);
  - *done:* `INPUT_KEYS` keeps `fee_rate_cash`, which `a_cash_fee_rate_above_zero_is_refused` shows
    read, and drops `drawdown`, `daily_pnl_fraction` and `bought_today_usd`, which no base's
    autonomy rule reads, until a case states one (nit, DEC-250 item 16).
- **Family B's three contradicting clocks, as a reference-case PR under DEC-176** (DEC-250 item 12,
  #331 review). MC-B22 (`session: after_hours`), MC-B23 (`in_close_window: true`) and MC-B31
  (`session: after_hours`) all put `now` at 2026-09-22T14:00Z, the regular session, so the harness
  fails them naming the conflict. Move `now`, and each output's `as_of` and `expires_at` with it, to
  21:00Z for MC-B22, 19:55Z for MC-B23, and an after-hours instant for MC-B31. No expectation
  changes, and each case then tests the condition its title claims;
  `the_two_session_cases_pass_once_now_agrees_with_their_labels` already shows MC-B22 and MC-B23
  pass so moved on unmodified code, and their status rows follow. The YAML,
  `cargo xtask refcases --write`, and the reference checks land together, apart from code (ES-22).
  Whether `session` and `in_close_window` stay case-file inputs at all is a separate question,
  Proposed to the founder in DEC-250.
  *Done (DEC-250's 2026-09-30 amendment):* `generate.py` moves the three clocks and keeps each
  output's offsets from `now`, no expectation changes, and `check_cases.py` now fails any builder
  case whose `session` or `in_close_window` contradicts its `now`. MC-B22 and MC-B23 pass, and their
  status rows are a separate status-only PR. MC-B31 still fails at `mandate_risk::trim_proposals`
  (pending E6-4). The founder question above stays Proposed.
- **Tests correction for family B's moved clocks** (DEC-250's 2026-09-30 amendment; crate code, so
  not in the reference-case PR, ES-22). `crates/mandate-refcases/src/mandate/order_builder.rs`.
  *Done (`cursor/family-b-clock-owed-tests-v2-138b`, which lands before #347):* moving MC-B22 and
  MC-B23 from `OWED` to `PASSING` would be red on the fixture before #347, so a `CLOCKED` table
  judges them by the fixture's own consistency instead. When a case's `session` and
  `in_close_window` agree with `mandate_risk::session_at` at its `now`, it must pass; otherwise it
  must fail naming the conflict, as before. The test compares the labels itself, so a harness that
  stops comparing them is still caught. MC-B31 stays in `OWED`: a `trim_to_target` case stops at
  `trim_proposals` before its labels are compared. `the_two_session_cases_pass_once_now_agrees_with_their_labels`
  is kept, because on the fixture before #347 its clock moves are what give its sibling-swap proof
  (the only one for a sell's `order_type` and for a deferred outcome) a case that passes.
- Fold the clocked cases back once #347 and #346 have both merged (the tests correction above,
  follow-up). With the clocks moved, `labels_agree_with_calendar` is true for both cases, so:
  - move MC-B22 and MC-B23 into `PASSING`, where `every_expected_member_is_compared_and_required`
    also sweeps them, and delete `CLOCKED` and `labels_agree_with_calendar`;
  - drop `the_two_session_cases_pass_once_now_agrees_with_their_labels`'s clock doctoring, which
    re-sets MC-B22's clock to the values it already holds and changes only MC-B23's `expires_at`,
    and run its sibling-swap proof on the two cases as the fixture states them;
  - reword that test's doc, which says the two cases fail only because their labels contradict their
    clock. DEC-250 item 3's label-against-clock comparison stays proved by
    `the_builder_and_the_gate_see_one_scene`'s doctored MC-B01.

  *Done (E6-2, [#359](https://github.com/kunwarshivam/mandate/pull/359); DEC-250's 2026-09-30 amendment):* MC-B22 and
  MC-B23 are in `PASSING`, `CLOCKED` and `labels_agree_with_calendar` are gone, and the renamed
  `the_two_session_cases_pass_as_stated_and_compare_every_sibling` runs its sibling swap on the
  fixture as stated. The sweeps' counts, recomputed from the fixture, grow with the two cases (768
  doctorings, 150 plants, 981 refused inputs). With the harness's label comparison skipped,
  `the_builder_and_the_gate_see_one_scene` fails on MC-B01's doctored `session`.
- Derive the family-B sibling counts (#331 round-2 review, nit). *Done (E6-2,
  [#346](https://github.com/kunwarshivam/mandate/pull/346); DEC-250 item 18):* the hand-written `siblings == 87` and
  `siblings == 11` are replaced by an assertion that every enum-valued expectation a swept case
  states (a word, a null, or a non-empty list of words in some family-B case) has a `sibling` arm,
  `on_timeout` and `action` excepted, so a case that gains or loses an enum expectation needs no
  count edit.
- Give the family-B sibling classifier its own oracle, in a tests correction of
  `crates/mandate-refcases/src/mandate/order_builder.rs` (#346 review, minor and nits):
  - `every_sibling_fails_its_comparison` trusts `is_enum_value` with nothing checking it: blinding
    the classifier to `false` and dropping a `sibling` arm together leaves the suite green, which
    the old `siblings == 87` would have caught. Pin the enum-valued member count the fixture yields
    (12 today) beside the derived assertion, as the file already pins 899 and 698 (minor);
  - `NO_SIBLING` exempts the bare member names `on_timeout` and `action`, so it would also exempt
    a future `/expect/gate_dry_run/action`. Key it on the `(pointer, member)` pair, as
    `enum_valued` is (nit);
  - reading `gate_state.working_universe` reports a missing member as "is not a list"; say "fixture
    has no" for a missing one, as `list_at` did (nit);
  - DEC-250 item 15's ES-13 exception records 1,338 non-test `src` lines; the file has 1,352 since
    #346. Refresh the figure or state that the exception is not tied to an exact count (nit);
  - DEC-250 item 18 names `origin` and `trim_withheld` as the enum-valued members the trim cases
    will need `sibling` arms for; MC-B17's `reason: trim_to_target` is a third, not in
    `HOLD_REASONS`, so the guard fires when E6-4 turns MC-B17 green (nit; it fails safe).
- **Settle what `safety_critical = true` means for a `tool`-layer crate** (#331 round-2 review, for
  the founder's after-the-fact look). `xtask/layers.toml` marks `mandate-refcases`
  `safety_critical = true`, and CODEOWNERS lists it, but two checks read it as not safety-critical:
  - `cargo xtask ci mutants` skips it, because `mutated_crates` requires a `Product` layer;
  - DEC-250 item 15 applied ES-13's 800-line limit for crates outside the safety-critical list,
    not the 400 the flag implies.

  Neither changed #331's outcome, but a harness change could land with no mutation gate while its
  entry claims otherwise. The conservative reading is that the flag governs: `ci mutants` also
  mutates safety-critical `tool` crates on the diff, and ES-13's safety-critical limit applies,
  with DEC-178's `risk_gate.rs` and DEC-250's `order_builder.rs` as recorded exceptions or split.
  Record the reading in a decision-log row in the same change as the xtask edit.
  *Done (DEC-253):* the flag governs. `mutated_crates` reads `safety_critical` alone, pinned by
  `the_gate_mutates_every_safety_critical_crate_tool_layer_included`; ADR-0001's ES-11 and ES-13
  say so, and ES-13 records `risk_gate.rs` and `order_builder.rs` as exceptions, not split.
- `parse_status` returning an empty map survives every test (DEC-253's proof run: `cargo xtask ci
  mutants` on a diff touching it missed `replace parse_status -> Result<BTreeMap<String,
  CaseStatus>, String> with Ok(BTreeMap::new())`). `tests/refcases.rs` treats a case the map does
  not name as pending and ignores it, so a harness that read no status would run no case and pass,
  and ES-11's "a passing case never regresses" would hold vacuously. Add a live test in
  `crates/mandate-refcases/src/lib.rs` that parses a two-suite sample and asserts the whole map,
  and one that reading the committed `status.toml` yields at least one passing case. Until then
  the mutation gate fails any change whose diff touches `parse_status`.
- Family B's harness arm has 13 mutants no live test catches (DEC-253's timing run of the gate over
  DEC-250's diff of `crates/mandate-refcases/src/mandate/order_builder.rs`: 79 mutants, 40 caught,
  26 unviable, 13 missed). `scaling_rung` (five: every return value, its `==`, its `&&`) is reached
  only by the trim cases, which fail pending E6-4, so no live test runs it; `judge`'s
  `nothing_proposed` guard can be `true`; `Inputs::read`'s `has_prior_fill` default can lose its `!`
  (DEC-250 item 13: no base rule reads the flag); `listing`'s
  `qty_increment < 1` (fractionable) can be `==`, `>` or `<=`; and `builder_error` can return any
  string or flip its `==`. One tests correction of the arm adds doctored-case tests in
  `crates/mandate-refcases/tests/`, as `mandate_gate_harness.rs` does for families G and F, so each
  survivor is caught; until then the gate fails any change whose diff touches those lines.
- **RC-22 and RC-25, blocked in the trading-domain harness** (E6-8's implementation PR, DEC-163;
  the gate driver since E6-9, DEC-199). `crates/mandate-refcases/src/trading_domain/gate.rs` now
  decides `propose_order` steps with `mandate_risk::evaluate`, states the market data a case omits
  (a quote at the limit price, volumes that pass check 6) and the origin each purpose maps to, each
  with its own tests. RC-25 still waits for the instrument fields `prior_close` and
  `median_dollar_volume_20d` (E6-7's rows in the harness) and for `owner_confirmed_bid`, which the
  header writes as `true` and the gate takes as a price (DEC-199 Q3). RC-22 needs more:
  `broker_order_update` (E7-2), the exit sequence's `actions` (E7-4), and the `conduct_breach`
  step's switch to `exits_only` with an owner alert, a mode transition the runtime owns (§5.9,
  §9.6's "breach → agent `exits_only`", E6-11); the pure gate only denies the breaching opening
  `conduct_limit_breached`. RC-16 also needs the case file's `not_in_universe` reconciled with the
  gate's `not_in_working_universe` (DEC-199 Q1). All three state more than one proposal, so each
  also waits for E7-4 and E7-5 (DEC-199 item 3).
- Done in the trading-domain gate driver tightening ([#349](https://github.com/kunwarshivam/mandate/pull/349);
  `the_listing_and_market_are_dec_199_item_6s`,
  `the_filled_median_is_the_liquid_collar_tier_to_the_cent`,
  `the_filled_trailing_volume_is_the_order_size_cap_to_the_share`,
  `the_filled_minimum_order_is_one_share_to_the_last_place`,
  `a_second_proposal_in_one_case_waits_for_e7_4_and_e7_5`,
  `an_agent_mode_where_nothing_moves_it_names_the_account_ledger`; DEC-199 items 3, 6 and 12):
  every listing and market value is pinned, and the median, the trailing volume and the minimum
  order are shown deciding at their edge, the ADV as at least 1,000,000 (its own value cannot bind
  before the order cap); a case with a second proposal fails naming E7-4 and E7-5; and an
  `agent_mode` after an event `MODE_OWNERS` does not list fails naming E7-5 (`MODE_HOLDER`). The
  row as it was:
  Tighten the trading-domain gate driver (#333 review, minors 1 and 2 and nit 2), in one tests
  correction of `crates/mandate-refcases/src/trading_domain/gate.rs` and DEC-199:
  - pin, or better, show taking effect, the values DEC-199 item 6 fills: `median_dollar_volume_20d`
    (the collar tier), `min_order_size`, and the two participation volumes. Today changing any of
    them leaves every test green, although none of the three changes is looser than the spec;
  - refuse a second `propose_order` step in the same case until E7-4 and E7-5 land. DEC-199 item 3
    decides each proposal alone, against an account with no working, unknown or related resting
    orders, and nothing fails if a case adds a second proposal;
  - add `agent_mode` to `check()`'s allowed keys, so that an `agent_mode` expectation on an event
    outside `MODE_OWNERS` fails naming its owning story rather than as `expect: unknown key`.
- The case-file side of DEC-199 Q1 to Q3, in one reference-case change for the founder:
  - RC-16 says `not_in_working_universe`, the registered code mandate spec §5.3 defines;
  - the header says the eligibility floor is checked against the limit price when no
    `prior_close` is given (or a case below $5 states its own);
  - RC-25 carries `owner_confirmed_bid` as the displayed bid's price rather than `true`.
- **trading-domain §9.6: state that the opposite-fill interval includes its last instant**
  (DEC-163 item 3; DEC-176 clarification). §9.6's "within 60 seconds after" an opposite-side fill
  is read inclusively, so an opening exactly 60 s after the fill is denied; the spec text should
  say so, as a clarification that tightens nothing the code does not already enforce.
- **`Ratio` to `Fraction` in `mandate-num`.** The surveillance report turns a concentration
  `Ratio` into a `Fraction` by printing and re-parsing it (`surveillance.rs`'s `share_of_equity`),
  because `mandate-num` has no exact conversion. The text round trip is exact, but a typed
  `Fraction::try_from(Ratio)` with its own tests would retire it (DEC-163 item 8).
- **A concentration threshold for the surveillance report** (founder: compliance-visible). §9.6
  lists concentration among the report's checks and §3.3 supplies no number, so the report states
  each concentration figure and flags none; `SurveillanceBreach::Concentration` is not raised
  (DEC-163 item 8). A founder-set threshold would let it be.
- **`CancelCause` in place of `CancelInput`'s two booleans** (a follow-up story, with its own tests
  correction). `CancelInput` tells an exempt cancel by `precedes_risk_reducing_order` and a
  marketable order by `marketable`, both set by the executor. A required
  `CancelCause { RiskReducing, Replace { marketable }, Discretionary }` would make the cause
  explicit and let the journal say which (DEC-163 item 7).
- **Does a resting protective order count for self-trade prevention?** `related_account_resting`
  lists protective orders too (DEC-163 item 11), the conservative reading, so a bracket opening
  beside a resting protective sell in the same instrument is denied `conduct_limit_breached` though
  §5.3 rule 8 lets a bracket add a tranche. A stop is not in the book until triggered; deciding
  whether it counts would reopen that path.
- **An exit's pacing can return a gate error** (#311 round 2, minor 1). `conduct::pacing` propagates
  `NumError` from `Qty::portion` in `slice` and from `Price::collar_bound` in `price_by_collar`, so
  an owner or discretionary exit with extreme prices or volumes gets `Err` rather than a decision.
  Nothing exercises it, since the property's generators are small. Add a property over extreme
  prices and volumes, or state what the executor does with a gate error on an exit (it must still
  route the exit, rule 13).
- **The opposite-fill property pins its boundary only by chance** (#311 round 2, minor 2).
  `properties::only_an_opposite_side_fill_starts_the_interval` draws `elapsed in 0..120`, and
  `ci test` fixes no proptest seed. `gate::tests::the_opposite_fill_interval_includes_its_last_instant`
  pins the 60 s edge deterministically; add 60 to the property's draw explicitly so it is not the
  boundary's cover by luck.
- **Assert the whole exit for exempt purposes** (#311 round 2, minor 3).
  `an_allowed_exit_is_never_below_its_minimum_or_zero` samples `RiskEngine` and `ProtectiveLeg` but
  asserts only the quantity bound for them, not `sent == proposed`, which
  `a_slice_binds_only_below_the_proposal_and_names_its_cap` covers separately. Assert it in the
  property too.
- **E6-6 slice 2:** the account-wide fold is `mandate_risk::fold_day_trades` (DEC-259). The
  trading-domain harness reads RC-09's and RC-09B's `regime`, `prior_day_trades`, `last_equity`,
  `multiplier` and `day_trade_count` and drives the ledger through `fold_day_trades` from the
  case's fills (DEC-284), after the tests correction #408. A later `propose_order` step is decided
  once every earlier allowed one was filled in full on its side (DEC-259 item 7); the three tests
  that reading owed are in `trading_domain::gate::tests`:
  `a_later_proposal_waits_for_a_full_fill_on_its_side` (identical buys with no fill, a partial
  fill, and more), and `the_conduct_figures_are_the_fill_steps` with
  `the_opposite_fill_interval_runs_from_the_fill_step`, which catch the plant that takes the
  conduct figures from the proposal. RC-09's third step is a crypto proposal, which waits for
  E6-10's harness PR. The executor (E7-3) calls the fold account-wide and journals
  `DayTradeFold::today`. **A fold error refuses openings only** (#370 review, major 1): E7-3 must
  still route exits and protective orders when `fold_day_trades` fails (`AGENTS.md` rule 13), and
  E7-3's tests PR carries the pending test
  `a_failed_day_trade_fold_refuses_openings_and_still_routes_exits` for it.
- **Drive the trading-domain gate cases through the real order path once E7-4 and E7-5 do**
  (the coordinator's ruling on #370, item 2). DEC-259 item 7 is a reading of the reference harness
  only: it decides a later proposal once the earlier allowed ones were filled in full by `fill`
  steps, and carries their conduct figures from those fills. When E7-4's `actions` and E7-5's
  account ledger give a submission its working order, reservation and partial fills, switch
  `mandate-refcases`' trading-domain gate driver to that path, delete the reading and its
  `Order` bookkeeping in `trading_domain/gate.rs`, and let partial fills and overlapping orders be
  decided as the order path decides them. Also read a `broker_account_update` that changes the
  regime or its figures (DEC-284 item 4).
- **Check the trading-domain pin of pending reasons for completeness** (#408 review, not a
  finding). `every_other_rc_15_variant_and_gate_case_names_the_story_it_waits_for` is a fixed list,
  so nothing makes a newly pending gate case join it. A rung-2 check that derives the list from the
  pending gate cases, less those a ruling excepts (RC-09, RC-09B), would close the gap.
- **Lift §2.2's trade date onto `ExchangeCalendar`** (#370 review, minor 1). `mandate-risk`'s
  `daytrades::trading_day` re-states `TradingCalendar::equity_trade_date`'s 20:00 ET cutoff over
  the committed exchange calendar, and `the_trade_date_agrees_with_the_accounting_calendar` pins
  the two together across 2026's holidays, both daylight-saving changes and the cutoff. Give
  `mandate-time`'s `ExchangeCalendar` an `equity_trade_date` of its own and call it from both, so
  the rule lives once.

From the independent review of E6-2's autonomy slice ([#216](https://github.com/kunwarshivam/mandate/pull/216)
round 1, verdict approve; minor 2, deferred by the coordinator's ruling):

- **V-023 at load must bound a decimal's precision (stream F).** The schema's `decimal` admits 28
  fractional digits and `Ratio` holds 24, so a rule comparing `order_usd` against a 26-digit value
  passes `mandate-builder`'s `check_rules`/`well_typed` re-check and is refused mid-walk by
  `Condition::matches` with `too_precise`. It is still refused, so rule 3 holds, but DEC-152 (1)
  promises the whole rule set is re-checked before any rule is read. Stream F's V-023-at-load in
  `mandate-spec::validate` refuses such a value up front (landed with E10-1's slice V, DEC-161 item 5),
  and the order path's `well_typed` gains the same bound so both report it by name.
- **E6-6:** drop or pin the `at.opening_auction` clause in `mandate-risk`'s `market_orders_barred`.
  The opening auction is always pre-market, which the clause for a US equity outside the regular
  session already bars, and crypto never has an auction, so the clause changes no decision and no
  test can catch its removal (#228 review, round 1, minor 2; E6-6's bug list when it lands).
- **E6-6:** pin the rest of a re-priced exit and of the close window. The tests assert
  `marketable_limit_required` and the quantity of a market exit re-priced in an auction window but
  not its `limit_price` or `applied`, and `hand::the_close_window_follows_the_early_close_calendar`
  asserts only times inside the window, so it passes against a `close_window` that is always true
  (#228 review, round 1, minor 3; E6-6's bug list when it lands).

From E6-2's builder slice (stream H; found while implementing §8.3, not by a review):

- **§8.3 step 2's "whole position (minus working exits)" has no input (stream H, with stream I).**
  `AccountSnapshot` carries no working exit quantity, so `propose` sells the whole position, as
  `reference/mandate/ref.py` does, and a second discretionary exit while one is working is proposed at
  the full quantity. Until the field lands, the backstop is the risk gate's trading spec §5.3 rule 4
  (`sell_exceeds_available`, merged in #221), which binds every reduction and denies the over-sell,
  so the builder cannot turn a position short on its own. The field arrives by a DEC-77 tests
  correction ahead of the slice that consumes it (every `AccountSnapshot` literal in
  `crates/mandate-builder/tests/` names it); that slice then subtracts working exits in
  `discretionary_exit` and holds when nothing is left to sell (#234 review, round 1, M1).
- **The runtime filters model outputs by instrument before `combine` (stream I).** `combine` refuses
  the whole call on an output for another instrument (`output_instrument_mismatch`, DEC-130), the exit
  branch included, so handing it a tick's unfiltered output buffer would stop a discretionary exit
  the way a crossed quote did before #234 moved that refusal to the buy path. E6-1's evaluation loop
  passes only the outputs whose `instrument_id` is the instrument being sized, with a test that a
  foreign output in the buffer never blocks an exit (#234 review, round 1, m2).
- **`BuilderError::Unimplemented` and `NumError::Unimplemented` are now constructed by nothing.**
  Both stay because `tests/vocabulary.rs` and `num::error_codes_are_stable` pin their codes, and an
  implementation PR may not edit a test. Drop each variant with its row in the next tests correction
  that touches those files (E6-2; the `NumError` half is the E4-2 row above).

From E6-4's V-040 spec change (stream H; the coordinator's ruling on #251, round 1):

- **Add V-040's boundary pair to `mandate.yaml`, with the harness counts, in one approved change
  (founder).** The rule's boundaries are two ladders: factors of 12 places in total (valid) and 13
  (V-040). The 2¹³ × 5¹³ ladder, where the whole product fits and a subset does not, belongs there
  too. For now all three are pinned in `reference/mandate/fuzz.py::fuzz_ladder_precision`, which runs
  on every seed, and as in-module rows in `crates/mandate-spec/src/validate/tests.rs`. They are not
  reference cases because a case changes counts that live `mandate_harness.rs` tests assert (67
  semantic, 202 owned, and the member sweeps), and the spec guard keeps the fixture and
  those tests in separate PRs. The founder-owned YAML (ES-22) and the counts must change together.
- **Three minors from #251's round 2, deferred by the freeze rule.** (1) `docs/specs/mandate.md`'s
  front matter still says a change needs founder approval with no qualification; DEC-167 item 3
  records that a stricter V-rule is agent-accepted under DEC-79, and the front matter should say so
  in one clause. (2) `fuzz_ladder_precision` draws 1 to 4 scale rungs plus two fixed ones: it never
  reaches the five-rung maximum that item 3's "five 2-place rungs still fit" relies on, and at four
  it builds a six-rung ladder `maxItems: 5` forbids. Draw 1 to 3 scale rungs, and pin the
  five-rung, 2-place ladder. (3) The function imports `combinations` inside its body; move the
  import to the top of `reference/mandate/fuzz.py`.

From E6-4's slice L (stream H; the coordinator's ruling on #279, round 1, minor 3):

- **The caller of `goal::status` pins the sane-and-fresh ask (stream G).** `GoalInputs::ask` values a
  remainder against the minimum order, and `goal::status` cannot tell a bad tick from a real quote.
  One print far below the market would make any remainder look worth less than the minimum and
  finish the goal, and with `on_complete: release` that cancels protection and retires the agent
  (§3.1). The order path's goal evaluation in `mandate-risk` passes only an ask from a quote that
  passed §5.6's sane-and-fresh filter. It needs a test that a single bad tick never finishes a
  `release` goal.

From E6-4's slice R1 (stream H2; #289's review round 2 approved it, and the freeze rule defers
these to R2, where the fold starts reading `Limits::conditions`):

- **Ignore the directory proptest writes (#289 round 2, nit 2).** `.gitignore`'s
  `*.proptest-regressions` does not match the `proptest-regressions/` directory proptest writes
  beside a crate's `src/` (a failing in-module property leaves
  `crates/mandate-spec/proptest-regressions/risk/fold/tests.txt`); ignore the directory. R2 stays
  inside `crates/mandate-spec`, so it did not change `.gitignore`.

From E6-4's slice R2 (stream H2; DEC-167 item 6):

- **`ref.py` counts a fill as a sane quote (reference fix).** `RiskState.step` sets
  `quote = kind in ("mark", "fill") …`, so a fill re-reads the last mark as a second quote and can arm,
  latch, or clear a hard breach. §5.6 says "sane quote", and a fill quotes nothing: E moves by the
  fill's price against the mark, not to a new mark. One flash print followed by any fill would latch
  a limit on one print, which DEC-63 rules out. The crate follows the spec (DEC-167 item 6), and no
  reference case changes either way. Drop `"fill"` from the tuple and regenerate with
  `reference/mandate/generate.py`, which must leave `fixtures/refcases/mandate.json` unchanged.
- **The `ref.py` reading the #124 handover left for R4.** Settling time before an allocation change
  only when `at > self.t` is equivalent to settling always; settle always. Readings 1 and 2 were
  R3's and are DEC-167 item 7 (a) and (c); handover items 4 (one cash sum) and 5 (the post-loop lift
  reset) are R2's and are in `risk/fold.rs`. *Done in R4 (DEC-167 item 8 (d)):* the fold settles
  always.
- **R3's status PR: fifteen MC-R cases pass.** MC-R01 to MC-R08, MC-R13, MC-R15, MC-R18 to MC-R20,
  MC-R22, and MC-R24 pass `cargo test -p mandate-refcases --test refcases -- --include-ignored
  mandate::MC-R` on R3 (DEC-167 item 7 (l)); proposing them for `status.toml` is founder-owned.
  *Done ([#356](https://github.com/kunwarshivam/mandate/pull/356), under DEC-77 item 3 as #335
  and #337 were):* exactly these fifteen are marked; the other nine stop at R4's inputs.
- **DEC-167's wording after R3's tests correction** (#357 review, nits). *Done in R4:* both edits
  are made. Item 6(c) keeps the
  superseded sentence "so it stands when the step's own sale leaves the book flat, as in
  `ref.py`", whose attribution is wrong (`ref.py` discards `stale_mark` on a flat book); strike the
  clause now that 7(e) supersedes it. Item 7(a)'s "only the latch can come earlier" should say
  earlier than what: a wait restarted at the rollover.
- **The families G and F harness after its tightening** (#348 review, nits), one tests change to
  `crates/mandate-refcases/src/mandate/risk_gate.rs`: (1) compare `pacing` after the verdict and
  reason, so a case whose verdict is wrong fails naming the verdict rather than a pacing the wrong
  verdict brought; (2) name `compare_computed`'s `c` parameter for what it holds (the gate's
  figures); (3) extend the group-id test to a case with two groups, so the rank of a name among
  several is pinned, not only the one group's id; (4) shorten the done row's verbatim "The row as it
  was" copy to a pointer at #317's re-review. From the round-2 review: (5)
  `a_denial_fails_unless_exactly_one_check_failed_with_its_reason` ends with a bare `Ok(())`; bind
  its failing shapes to a name and finish with `expect_eq("failing shapes", shapes.len(), 3)`, as its
  allow sibling counts its edits; (6) DEC-178 item 11 records only the allow pin; add a sentence for
  the denial pin and its test.
- **The trading-domain gate driver after its tightening** (#349 review, nits), one tests change to
  `crates/mandate-refcases/src/trading_domain.rs` and its gate tests, plus DEC-199's wording:
  (1) `pending()` counts the second `propose_order` before a backtest case is dispatched, so say in
  `SUBMISSION_STORIES`'s doc that the refusal applies to every case kind; (2) a case whose earlier
  proposals are all denied is refused too, though a denial leaves no working order, so either admit
  it or record why the refusal stays uniform; (3) DEC-199 item 6 says the gate harness shows "four"
  members deciding at their edge where it lists three (the median, the trailing volume, and the
  minimum order; the ADV is shown only as at least 1,000,000); (4) rename the `num` closure in
  `the_listing_and_market_are_dec_199_item_6s`, which shadows the crate's `num`; (5) shorten the
  done row's verbatim "The row as it was" copy to a pointer at its review.
- **E8-3's check 7 tests after the overlay correction** (#354 and #355 review, nits; nits 2 to 5
  done in E8-3's `quorum` PR, DEC-257 item 4): `crates/mandate-approval/tests/quorum.rs` draws a
  `two_approver_above_usd` of `"0"`, which `schemas/policy.schema.json` excludes
  (`positive_decimal`); draw only ceilings a workspace can hold, as DEC-173 item 15 already does
  for grant sets. And (#368 review, minor 3) `tests/quorum.rs`'s module doc still says its tests
  are pending and fail on `quorum`'s `ApprovalError::Unimplemented`, which E8-3's `quorum` made
  false. A tests correction, since DEC-77 keeps `tests/` edits out of an implementation PR.
- **Family A's harness after its tightening** (#360 review, nits), one tests change to
  `crates/mandate-refcases/src/mandate/autonomy.rs` plus DEC-162's wording: (1) DEC-162 item 4 says
  the property test runs "over generated well-typed policies and facts", but `action_context()` pins
  `drawdown` and `daily_pnl_fraction` to zero and `well_typed_leaf()` never builds a condition over
  `instrument`, `session`, `drawdown`, `daily_pnl_fraction` or `position_pnl_fraction`; name the
  space the strategies cover (the claim holds, since §6.2 step 3 decides on `purpose` first);
  (2) the module doc names only `no_rule_set_ever_denies_or_asks_a_reducing_purpose`; add
  `hand::every_reducing_purpose_is_auto_by_the_builtin` for the other three reducing purposes, as the
  row does; (3) the placeholder refusal is the module's only message prefixed with the case id,
  which the suite already reports; drop the prefix and the `id` parameter it needed; (4) give
  `unread_instrument` a doc comment; (5) assert the second premise behind `PLANTED = "input"`, that
  no `action` or `expect` carries an `input` member.

From E6-4's slice R3 (stream H2; DEC-167 item 7):

- **`ref.py` drops a daily hard wait at the rollover (reference fix).** `RiskState.step` pops
  `hard_first["max_daily_loss"]` into the rollover record and never reads it again, so `hard_breach`
  can stay applied with nothing to clear it. The crate keeps the wait under the new day (DEC-167
  item 7 (a)). Keep it in `hard_first` and regenerate with `reference/mandate/generate.py`, which must
  leave `fixtures/refcases/mandate.json` unchanged (checked from the crate's side: putting `ref.py`'s
  reading into the fold leaves the same fifteen MC-R cases passing).

From E6-4's slice R4 (stream H2; DEC-167 item 8):

- **`ref.py` holds a re-triggered scale rung back during the stepped lift (reference fix).** After
  an acknowledgment `RiskState._ack` queues the scale rungs highest `at` first, and a rung that
  triggers again is removed from `reset_queue` and may lift only once the queue is empty, so the 3%
  rung can lift while the 4% rung stays active. §5.8 says highest `at` first, and the fold lifts the
  highest active rung next (DEC-167 item 8 (b)). Re-insert a re-triggered rung in its place, or lift
  only the highest active rung while stepping, and regenerate with `reference/mandate/generate.py`,
  which must leave `fixtures/refcases/mandate.json` unchanged. In `agent/h2-dec-270-reference` (DEC-275).
- **`ref.py` journals `hold_protected` for a completed `profit_stop` goal (reference fix).** A
  `goal_complete` input on a `profit_stop` goal (its end date) writes
  `{"type": "GoalCompleted", "on_complete": "hold_protected"}`; the fold writes its one §3.1
  outcome, `then: discretionary_exit_all_then_retire` (DEC-167 item 8 (f)). No reference case has
  one. In `agent/h2-dec-270-reference` (DEC-275).
- **`release` journals the loss carry: the reference side (DEC-270; stream H2, first).** One
  reference and reference-case PR, kept apart from crates (ES-22): `reference/mandate/ref.py`'s
  `goal_complete` arm journals `AgentStopped` with `loss_carry_usd` = max(0, N − E) after
  `PositionReleased` when `on_complete` is `release`; MC-R17's expected journal gains it; any §3.1 or
  §5.7 wording the spec needs under DEC-270; `generate.py`, `check_cases.py`, and the fuzz rerun, and
  `cargo xtask refcases --write`. Only MC-R17 moves. Merged in [#386](https://github.com/kunwarshivam/mandate/pull/386)
  (DEC-274), with MC-R25, MC-R26, and MC-V68 for the carry and the redeploy, after the harness PR
  [#381](https://github.com/kunwarshivam/mandate/pull/381) (DEC-276, DEC-277).
- **`release` journals the loss carry: the `mandate-spec` side (DEC-270; stream H2, after the
  reference PR).** `Fold::complete_goal`'s `Release` arm journals `AgentStopped` (reason
  `goal_complete`) with the loss carry, as `retire` does, and a test shows that releasing and
  redeploying on the same connection opens the new agent at the carried L (through
  `ValidationContext::from_journal` and V-032). Merged in [#387](https://github.com/kunwarshivam/mandate/pull/387).
- **MC-R17 back to passing (DEC-277; E6-4, stream H2).** The DEC-270 reference PR changes MC-R17's
  expected journal and marks it `pending`, the one flip DEC-277 allows. The `mandate-spec` side above
  must bring it back: the status PR that follows it marks MC-R17 `passing` again, with MC-R25,
  MC-R26, and MC-V68. Done in `agent/h2-dec-270-status` (DEC-77 item 3), after [#387](https://github.com/kunwarshivam/mandate/pull/387).
- **DEC-277's exception for the `journal` and `trading_domain` suites (the #381 review, round 2,
  minor 1).** `fixture_entry_changed` in `xtask/src/main.rs` finds a case by an object whose `id` is
  its `status.toml` key, which matches the `mandate` suite but no `journal` key and not
  `trading_domain`'s compound `RC-NN::sub_case` or version rows, so a flip there always reads
  "unchanged" and is refused. Extend the lookup to those key shapes, with fixture-repository tests in
  each suite.
- **Narrow DEC-277's exception to the case's expectation (the #381 review, round 2, minor 2).** The
  guard allows a `passing` → `pending` flip when any part of the case's fixture entry changes; allow
  it only when the entry's expectation members change (`expect`, or a risk-state step's `expect`), a
  tightening, so a title or note edit cannot take a case back to pending.
- **§5.10's `GoalCompleted` reason for a completed `profit_stop` goal (the #386 review, minor 1).** The
  journal table names `GoalCompleted`'s reasons as `profit_stop_reached`, `target_qty`, `max_spend`,
  and `end_date`, but a `profit_stop` goal completed by its end date journals no reason in either
  `ref.py` or the fold (`then: discretionary_exit_all_then_retire` only, DEC-275). Decide whether it
  carries `end_date`, as an accumulate goal's `goal::status` would, and move the spec, `ref.py`, and
  the fold together.
- **MC-R25's note pins E before the release (the #386 review, minor 2).** MC-R25 expects
  `agent_equity` 9850 after the release: the released position stays valued on the agent's books at
  the last mark, which is what makes the carry "E before the release" (DEC-274 item 2). Add a
  sentence to its note in `generate.py` saying so, so a later change that zeroes the released
  quantity is seen to break the reading rather than taken as a fix. Say too why the quantity must
  stay: a mark after the retirement still moves `agent_equity` and journals the ladder, the daily
  loss, and the floor on a holding the owner has taken, which nothing acts on while the agent is
  `stopped` (the #387 review, minor 2). With it, rewrap `complete_goal`'s doc comment in
  `crates/mandate-spec/src/risk/fold/owner.rs` to the file's 100-column width (minor 1).
- **R4's status PR: the last nine MC-R cases pass.** MC-R09 to MC-R12, MC-R14, MC-R16, MC-R17,
  MC-R21, and MC-R23 pass `cargo test -p mandate-refcases --test refcases -- --include-ignored
  mandate::MC-R` on R4 (DEC-167 item 8 (i)); marking them in `status.toml` follows under DEC-77
  item 3, as #356 did for fifteen.

Minor findings from the independent review of slice R2 ([#324](https://github.com/kunwarshivam/mandate/pull/324);
held back by the freeze rule; R3 took minors 1, 2, and 4, DEC-167 item 7 (i) and (j), and minor 6,
`SpecError::Unimplemented`'s doc):

- **The `strictest` oracle reads the daily-loss mode by hand** (#324, minor 3; a tests correction).
  `tests/risk.rs`'s `strictest` maps `DailyLoss` to `exits_only`, while the fold reads
  `daily_loss_action`. Its property still walks `ladder_only`, whose daily action is the base's
  `exits_only`, and R3's renewal walks are in-module tests that do not use it; take the action from
  the mandate before a walk over a `flatten_and_pause` daily loss does.
- **A `Qty::checked_sub` failure is always reported as a short sale** (#324, minor 5). True while a
  negative result is its only failure; name the error by its kind if `Qty` gains another.
- **Wrap `M5-F-mandate-spec.md`'s long line** (#324, minor 7), the R2 row that runs past the file's
  wrap width.

Nits from the independent review of slice R3 ([#344](https://github.com/kunwarshivam/mandate/pull/344);
held back by the freeze rule; its two minors and its DEC-167 nits went into R3's tests correction):

- **R3's remaining nits** (#344 review). (a) A step that leaves the book flat clears `stale_mark`
  with reason `sane_mark` though no mark arrived. No change: DEC-167 item 7 (e) records it, since
  `sane_mark` is the only clearing reason §5.10 names. (b) `Limits::daily_loss`, like
  `Limits::conditions`, re-parses `HARD_TRIGGER_MULTIPLE` on every call; parse it once into
  `Limits` (an implementation change). (c) DEC-167 item 7 (k) counts 441 added non-blank non-test
  lines where a recount gives 464. Immaterial: R3 is over ES-13's 400 either way, and item 5 fixes
  its contents.

From journal spec v0.6 §9.1, the agent-stream payload schemas ([DEC-177](04-decision-log.md#decisions);
DEC-174 items 4 and 5). Until each lands, the drafts it names stay refused at `append`, which adds no
risk (rule 3):

- **Stream I: the runtime writes §9.1's payloads.** In `mandate-runtime`'s `step.rs`, `payload.rs`
  and `state.rs`: `instrument_id` and `limit_price`, with `type` and `tif`, on `DecisionMade` and
  `IntentProposed`; `null` rather than empty strings (`reason_code` on an allow, the unconfirmed
  `OwnerExitRequested`); timestamps rather than risk-clock seconds (`ObservationRecorded.as_of`,
  `ModelOutputRecorded.as_of` and `expires_at`); `data_ref` and `content_hash` as stored artifacts
  rather than inline data; `DecisionMade`'s convictions, outputs used, model weights, and clips
  applied; `step_up` as `{assertion_id, authenticated_at, method}`; and the agent stream's
  `StreamOpened` at seq 1, which nothing writes today. From round 1 of the review (DEC-177 items 9,
  11, 12, and 13): `user` and `step_up_status` on every `OwnerExitRequested`, including the owner kill
  switch's, with `step_up` only when valid; `exit_origin` on every `DecisionMade`, with the
  convictions and score `null`, and the evaluation's lists empty, on a decision no §8.3 evaluation
  produced (a goal completion, a removed instrument, a risk exit); `ask_suppressed` when a
  classified `ask` is not asked (DEC-156 item 5); and `lifecycle` as `normal`, `paused`, or
  `stopped`, never `exits_only`. From the v0.5 reconciliation (DEC-177 item 24): `causation_id` on
  each copy of an owner command, the control stream's `OwnerCommandIssued` (§9.1 rule 16).
- **Stream L: the shell's envelope carries the required `config_refs` and the `artifact_refs`**
  (DEC-174 item 5). `mandate-shell` writes `config_refs: {}` and `artifact_refs: []` on every draft,
  so every event that requires `mandate_version` or `model_version` is `missing_config_ref`, and every
  event with a `ref` member is `artifact_refs`.
- **Stream K: the executor's account-stream drafts match the registered schemas and vectors**
  (DEC-174 item 5): `risk_clock` as a timestamp string (`batch.rs` writes an integer), and only on
  the risk inputs §2 lists; `IntentReceived` as `agent_id`, `instrument_id`, `limit_price`, `type`,
  and `tif` rather than `agent`, `kind`, `instrument`, and `limit`; `OrderSubmitted` writing `null`
  for its empty members rather than omitting them (§4.2).
- **E7-9's tests PR: the harness reads `agent_stream`, then the vectors become version 4.**
  `mandate-refcases`' journal module reproduces the section's chain and artifacts, refuses each
  invalid draft with its reason and path (a change may `delete` a member), accepts each valid draft
  and valid batch, refuses each invalid batch at its `draft_index`, and fails each
  `range_verification` case with its code at its seq; `mandate-journal` gains a boolean type, the
  schema choice by stream type for `StreamOpened` and `KillSwitchActivated`, and the §9.1 rules.
  `journal::version` pins 3 and is passing, so the bump is a code PR that accepts 4, then a one-line
  spec change (ES-22).
- **`mandate-journal` and the verifier check the agent stream's cross-event facts** (DEC-177 item
  14). `append` refuses a batch whose `IntentProposed` differs in an action member from the
  `DecisionMade` it names in the same batch (§9.1 rule 10), and §11's `intent_action_mismatch` and
  `mode_event_mismatch` run in `mandate journal verify` and the scheduled verification, against the
  vectors' `invalid_batches` and `range_verification`.
- **A generated `range_verification` vector with a forward reference (#384 review, major 1; DEC-168
  item 13).** `verify_agent_stream` indexes the whole range first, so an `IntentProposed` naming a
  `DecisionMade` later in the range is compared, and a `mode_event` naming a later event or its own
  switch fails, at any `from_seq`. Only `mandate-journal`'s in-module tests pin this today; the next
  journal reference PR adds generated range cases for both, at a `from_seq` above 1.
- **Pin §9.1's ordering of `artifact_refs` and `pii_refs` ahead of rules 14 to 16 (#384 review, minor
  2).** No vector or test breaks both an envelope check and a subject or copy rule, so swapping their
  order passes. A tests-correction PR from stream L adds a draft that breaks both and expects the
  envelope check's reason and path.
- **A reference past a bounded range's end is exempted like one before its start (#384 review round
  2, minor 3).** `verify_agent_stream` leaves unchecked a `causation_id` or `mode_event` that names no
  event of the range, whichever side of the range the event lies on. §11 exempts only a reference
  before the trusted start; a range with a `to_seq` needs the rule stated and a check or a test.
- **Duplicate `event_id`s in `verify_agent_stream`'s index are unpinned (#384 review round 2, minor
  3).** The index keeps the last row with an ID. `append` refuses a repeated `event_id`, so a stored
  range never holds one, but no test says what a tampered range with a duplicate reports.
- **An `invalid_drafts` vector for rule 12's `bid_size` (#384 review round 3, minor 1).** The journal
  vectors refuse a confirmed exit without `bid` or `floor`, but none without `bid_size`; only
  `mandate-journal`'s in-module `rule_12_needs_each_member_of_a_confirmed_bid` holds it. The next
  journal-spec PR adds the draft (ES-22 keeps generated vectors out of a code PR).
- **The model registry stores each pinned model's content object as an artifact.**
  `ModelOutputRecorded.content_hash` is a `sha256:` reference, so it is in `artifact_refs` (§3), and
  §11 check 6 fails `artifact_missing` unless the object is in the artifact store.
- **Mandate spec §8.2: say what happens to an output outside its ranges** (conviction in [−1, 1],
  confidence in [0, 1]): ignored with an `ignored` reason, or refused. §9.1 records the values as
  given and does not rule.
- **The account stream's `KillSwitchActivated` and `AgentModeApplied` schemas** are not closed by
  §9.1; they close with the executor's account-stream schemas.
- **Close the approval events' schemas in §9.1, with vectors** (DEC-177 item 23). `ApprovalRequested`,
  `ApprovalDelivered`, `ApprovalResponded`, `ApprovalRevalidated`, `ApprovalTimedOut`, and
  `ApprovalCanceled` (journal spec v0.5, mandate spec §6.4) are listed in §9 but not closed. The
  change that closes them also adds the chain events an approved order needs (`DecisionMade` with
  `autonomy: ask`, `ApprovalRequested`, `ApprovalResponded` naming an `ApprovalResponseSubmitted`,
  `ApprovalRevalidated` with result `act`, and its `IntentProposed` in the same batch); a
  `range_verification` case in which the approved intent differs from the bound content object in
  one action member, failing §11's `intent_action_mismatch` second clause; a mutant that skips that
  clause; and rule 7's check that no `ApprovalRequested` names a decision with `ask_suppressed`.
- **Verify owner copies against the control stream** (DEC-177 item 24). §9.1 rule 16 checks only
  that an owner copy's `causation_id` is non-null, because `append` and §11's per-range checks read
  one stream. A cross-stream check in `mandate journal verify` resolves it on `ctl:{workspace_id}`
  and fails unless it names an `OwnerCommandIssued` of the copy's command and subject, submitted
  before the copy, copied at most once into each event type on the agent stream, as the generator's
  `check_owner_copies` does for the vectors; §11 names the check and its code, with a vector.

Minor and nit findings from round 1 of the independent review of the journal spec v0.5 change
([DEC-177](04-decision-log.md#decisions); held back by the freeze rule, one row each):

- **A copied `AgentModeChanged` names its `AgentModeApplied`.** When the agent runtime copies a
  mode change the executor originated, the copy's `causation_id` names the originating
  `AgentModeApplied` (§2); add the rule to §9.1 and a vector for it.
  *Narrowed (DEC-177 item 24):* §9.1 rule 16 covers the owner's pause, resume, and Stop, which name
  their `OwnerCommandIssued`. What remains is the copy of an account-stream mode change (reasons
  `restriction_changed` and `awaiting_reconciliation`).
- **Bound the model-supplied free text.** `ModelOutputRecorded.model_id`, `model_version`,
  `direction`, and `invalidation` are any non-empty text: bound their length or check them against
  the model registry and the directions v1 allows, and scan them for personal data as §6.4
  requires.
- **`KillSwitchActivated` records the initiator's step-up**, or names its `OwnerExitRequested`
  (for example as `causation_id`), so the switch's own record shows what authorized it.
  *Done for the owner's switch (DEC-177 item 24):* §9.1 rule 16 makes an owner's
  `KillSwitchActivated` name its `OwnerCommandIssued`, which carries the step-up evidence (§9), as its
  `OwnerExitRequested` does. What remains is a `platform_operator` switch, which names no owner
  command.
- **`journal.yaml`'s header notes the DEC-176 exception.** Its line "Changing these vectors requires
  founder approval" predates DEC-176, under which agents accept changes that only tighten or
  reconcile.
- **Tidy the journal generator.** Add `from __future__ import annotations` to
  `reference/journal/generate.py`, whose forward reference in `T` fails on Python before 3.14, and
  run ruff over `reference/`.
- **Correct `mandate-refcases`' journal module doc.** `crates/mandate-refcases/src/journal.rs` says
  the vectors are version 2; they are version 3.

Minor findings from round 2 of the same review (#320 round 2;
[DEC-177](04-decision-log.md#decisions) item 19; held back by the freeze rule, one row each):

- **Test both directions of §9.1's biconditionals (#320 round 2).** Rules 1, 3, 4, 5, and 13 each
  have an invalid draft for one direction only (for example a sell labelled `open`, but no buy
  labelled an exit), so a validator checking only that direction passes. Add a draft for the other
  direction of each, and split each rule's mutant into one per direction, each caught only by its
  own draft.
- **Vectors for the untested envelope members (#320 round 2).** No draft tests that `artifact_refs`
  is exact (sorted, de-duplicated, and no ref the payload does not hold), that `pii_refs` is
  ordered, or that `actor.build` is required for a `system` or `agent` actor. Add an invalid draft
  and a mutant for each.
- **Assert §9.1's report order (#320 round 2).** "The first violation, in this order" is never
  tested: every invalid draft breaks one rule. Add drafts that each break two rules of different
  ranks (for example an extra member and a rule-4 breach, or rules 4 and 5 together) and expect the
  earlier one, with a mutant that reverses the order.
- **Trace mandate spec §5.10's `OwnerExitRequested` row to §9.1 (#320 round 2).** Its field list
  ("instrument or scope, bid shown and confirmed, user (opaque), step-up evidence") lacks
  `step_up_status`, which §9.1 requires on every owner exit (DEC-177 item 11). Add it there, and
  check the §10 row the same way.
- **Correct §9.1's citation for a removed instrument (#320 round 2).** `exit_origin`'s row cites
  mandate spec "(§2.2, §2.3)" for a removed instrument, where mandate spec §6.1 cites §2.3 and §5.9
  cites §2.3 and §8.6. Cite the sections the mandate spec gives.
  *Done (DEC-177 item 22):* the row cites mandate spec §6.1's purpose table, §2.3, and §8.6.
- **Reconcile #320 and #321 when the second merges (#320 round 2; DEC-177 item 20).** Both call
  themselves journal spec v0.5. Whichever merges second:
  (a) resolves the textual conflicts in the Status line and the v0.5 change-history bullet and
  renumbers itself to v0.6, in the §9.1 heading, the `journal.yaml` header, and `generate.py`'s
  `spec` string, then runs `reference/journal/generate.py --write` and `cargo xtask refcases --write`;
  (b) adds `ApprovalRevalidated` to §9.1 as a third allowed cause of `IntentProposed` (rule 10), and
  records a decision on whether `intent_action_mismatch` compares an approved intent against the
  approval's bound fields;
  (c) states a §9.1 causation rule for #321's §2 copy rule ("`causation_id` pointing to the owner
  command"), which the chain's `OwnerExitRequested` events (seqs 6, 9, 12) and `KillSwitchActivated`
  (seq 13) break with `causation_id: null`, and regenerates the vectors to meet it;
  (d) keeps one definition of `ask_suppressed`, which both add, and drops #320's hedge "once M7's
  change lands" in `DecisionMade`'s table;
  (e) re-checks each §9.1 citation of mandate spec §6.1 against the merged text, since §9.1 cites
  §6.1 for a rule only #321 states.
  *Done (DEC-177 items 21 to 24), with (b)'s vectors deferred:* #320's change is v0.6 (a); rule 10
  and §11 allow and bind the `ApprovalRevalidated` cause, whose range case waits for the row
  "Close the approval events' schemas in §9.1, with vectors" (b); rule 16 and the chain's causation
  ids (c); one `ask_suppressed` definition, citing mandate spec §6.4, and no hedge (d); and §9.1's
  §6.1 citations name what §6.1 states (e).

Minor and nit findings from round 1 of the independent review of the v0.5 reconciliation
([#340](https://github.com/kunwarshivam/mandate/pull/340); [DEC-177](04-decision-log.md#decisions)
item 25; held back by the freeze rule, one row each):

- **Name the members an approved intent is compared on (#340 round 1).** Rule 10 compares the
  seven intent fields, but the `IntentProposed` sentence and §11's second `intent_action_mismatch`
  clause mean the five `ApprovalRequested` binds (`instrument_id`, `side`, `qty`, `limit_price`,
  `purpose`; not `type` or `tif`). List the compared members in both places, so an approved intent
  is never refused over a `tif` its approval did not bind.
- **Enforce that `ApprovalRevalidated` precedes its intent in the same batch (#340 round 1).**
  §9.1 allows an `IntentProposed` caused by an `ApprovalRevalidated` with result `act` "that
  precedes it in the same batch", and no rule checks either the order or the batch.
- **State `OwnerCommandIssued`'s scope and subject (#340 round 1).** `check_owner_copies` models the
  command as carrying `scope` (including `instrument`) and `subject`, which §9's control-stream row
  does not state; add them to the row, or change the oracle to the members the row names.
- **Test rule 16's guard and report position (#340 round 1).** No draft shows that rule 16 is
  checked only on a well-typed payload, or that it reports after the subject rules; add a draft
  that breaks rule 15 and rule 16 together, and one with an ill-typed payload and no cause, each
  with a mutant.
- **A range case with `from_seq` above 1 (#340 round 1).** Every `range_verification` case has
  `from_seq: 1`, so `mode_event_mismatch.unresolved`'s full-chain guard is untested: add a case
  verifying from a later seq whose `mode_event` names an earlier event, expecting no failure, with
  a mutant that drops the guard.
- **`outside_session_exit_defer_code()` checks with `assert` (#340 round 1),** which `python -O`
  removes; raise instead.
- **Note: the `exit_origin` citation fix pulled a round-2 minor forward (#340 round 1, nit).** Item
  22 corrected the removed-instrument citation that #320 round 2 had backlogged; no action.
- **`propose` hard-codes `RequestedBy::Agent` (#369 round 1, owed by E10-6).** DEC-262 item 2
  stamps every buy `propose` sizes as the agent's own, because no owner or client request path
  calls the builder yet. The request path E10-6 adds must carry the authenticated channel's
  requester to the order it proposes (§6.2 step 5a) rather than reuse `propose`'s stamp, with a
  test that a client's request reaches `decide` as `client`.
- **The client-ceiling sweep has no delegation dimension (#369 round 1, owed by E8-8's tests PR).**
  `Autonomy` holds no `delegations` until E8-8 (DEC-262 item 5), so
  `every_rule_default_admission_and_requester_obeys_the_client_ceiling` and
  `a_client_opening_is_never_auto_and_every_other_request_decides_as_before` cover rules, defaults,
  admission and requester only. E8-8's tests PR adds live, spent, expired and suspended delegations
  to both and asserts that none lifts a client's order (MI-26, MI-30).
