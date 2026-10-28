# Journal Spec (v1)

| | |
|---|---|
| **Status** | v0.112 (v0.2 founder sign-off 2026-09-25, [DEC-71](../project/04-decision-log.md#decisions); v0.3 amendment [DEC-81](../project/04-decision-log.md#decisions); v0.4 adds the research-agent events of [DEC-97](../project/04-decision-log.md#decisions) and [DEC-111](../project/04-decision-log.md#decisions); v0.5 approval escalation v0, [DEC-173](../project/04-decision-log.md#decisions), amended by [DEC-181](../project/04-decision-log.md#decisions), whose `DecisionMade` members [DEC-252](../project/04-decision-log.md#decisions) closes in §9.1; v0.6 closes the agent stream's payload schemas, [DEC-177](../project/04-decision-log.md#decisions); v0.7 closes the control-stream schemas `ValidationContext` reads, `AccountSnapshotRecorded`, and `OwnerCommandRefused`, [DEC-261](../project/04-decision-log.md#decisions); v0.8 closes the account-stream risk-state records `MandateVersionApplied` and `UniverseChanged`, [DEC-403](../project/decisions/DEC-403.md); v0.9 closes the research agent's thesis records `ThesisProposed` and `ThesisRevised`, [DEC-413](../project/decisions/DEC-413.md); v0.10 types `UniverseChanged`'s instrument as an asset ID and states what §9.3's mapping refuses, [DEC-404](../project/decisions/DEC-404.md); v0.11 types the thesis records' `instrument_id` as an asset ID, [DEC-413](../project/decisions/DEC-413.md) item 7; v0.12 adds the notice stream, [DEC-438](../project/decisions/DEC-438.md) items 5 and 28; v0.13 closes the account-stream executor records of §9.5, [DEC-446](../project/decisions/DEC-446.md) and [DEC-447](../project/decisions/DEC-447.md); v0.14 closes `OrderStateChanged`, [DEC-459](../project/decisions/DEC-459.md); v0.15 closes the approval and reconciliation records in §9.6, [DEC-460](../project/decisions/DEC-460.md); v0.16 binds effective policy and model-registry snapshots to production decisions, [DEC-484](../project/decisions/DEC-484.md); v0.17 closes the owner's approval answer and the runtime's two records of it, [DEC-533](../project/decisions/DEC-533.md); v0.18 adds the policy overlay's `decided_by` label, [DEC-536](../project/decisions/DEC-536.md); v0.19 registers a broker's capability profile as configuration, [DEC-531](../project/decisions/DEC-531.md) item 4 and [DEC-630](../project/decisions/DEC-630.md); v0.20 adds the connection records and `ConnectionEstablished`'s `account_ref`, [DEC-800](../project/decisions/DEC-800.md); v0.21 closes the records the workspace API commits for drafts, the compiler, confirmation, and owner requests, [DEC-670](../project/decisions/DEC-670.md); v0.22 adds the `client` actor and closes `ConnectionRevoked`'s reason and the client records, [DEC-671](../project/decisions/DEC-671.md); v0.23 closes the hold on new openings, [DEC-672](../project/decisions/DEC-672.md); v0.24 suspends a connection on a later `tools_missing`, [DEC-674](../project/decisions/DEC-674.md), and reads a later check with no pin as `contract_drift`, [DEC-676](../project/decisions/DEC-676.md) item 2; v0.25 catalogues the identity records and closes the membership records, [DEC-648](../project/decisions/DEC-648.md); v0.26 closes the records-access, export, and verification records, [DEC-780](../project/decisions/DEC-780.md); v0.27 closes the anchor and segment records, [DEC-783](../project/decisions/DEC-783.md); v0.28 closes the alert and notice records and the notice stream's `StreamOpened`, [DEC-720](../project/decisions/DEC-720.md); v0.29 completes §9.13's check codes and verifies an operator read's break-glass cause, [DEC-774](../project/decisions/DEC-774.md); v0.30 brings the connection vectors to §9.8's and §11's readings, [DEC-696](../project/decisions/DEC-696.md); v0.31 states the membership writer's order rule in §9.12, [DEC-659](../project/decisions/DEC-659.md); v0.32 journals the pending connection as `ConnectionRequested`, with stream rule 131, [DEC-694](../project/decisions/DEC-694.md) item 4 and [DEC-699](../project/decisions/DEC-699.md); v0.33 admits the `ProtectionChanged` records the executor's fold needs in rules 41 and 44, [DEC-859](../project/decisions/DEC-859.md); v0.34 journals the broker's order id on `OrderStateChanged` version 2, [DEC-869](../project/decisions/DEC-869.md); v0.35 verifies a connection range from its connection anchor and fails closed without one, [DEC-885](../project/decisions/DEC-885.md); v0.36 records a verification's trusted start, count, and unproven token check on `VerificationRun` version 2, with an `incomplete` result, [DEC-788](../project/decisions/DEC-788.md) and [DEC-789](../project/decisions/DEC-789.md); v0.37 refuses a `ConnectionRevoked` on an account stream under rule 68, [DEC-888](../project/decisions/DEC-888.md), and folds §11's anchors only from a verified prefix bound to the range's start, [DEC-892](../project/decisions/DEC-892.md); v0.38 scopes a connection anchor to one stream, [DEC-889](../project/decisions/DEC-889.md), and checks a range's hot rows against its segments' cold files as `segment_rows_mismatch`, [DEC-894](../project/decisions/DEC-894.md); v0.39 verifies an anchor start's timestamp token, [DEC-896](../project/decisions/DEC-896.md); v0.40 records the provider's verdict on a failed `NoticeAttempted`, [DEC-706](../project/decisions/DEC-706.md); v0.41 closes the runtime's `ApprovalTimedOut`, [DEC-944](../project/decisions/DEC-944.md); v0.42 binds the order builder's plan on `ApprovalRequested` version 2, with rule 136, [DEC-942](../project/decisions/DEC-942.md); v0.43 closes the runtime's `ApprovalCanceled`, [DEC-950](../project/decisions/DEC-950.md); v0.44 registers the control-plane records, [DEC-981](../project/decisions/DEC-981.md); v0.45 closes `BacktestRunRecorded` and adds `EvaluationRunRecorded`, [DEC-1001](../project/decisions/DEC-1001.md) and [DEC-1002](../project/decisions/DEC-1002.md); v0.46 closes the agent stream's `ModelInvocationRecorded` and adds the model gateway's meter stream, [DEC-998](../project/decisions/DEC-998.md); v0.47 closes the scheduler stream's records, [DEC-1010](../project/decisions/DEC-1010.md); v0.48 registers the deployment manager's records, [DEC-1170](../project/decisions/DEC-1170.md); v0.49 adds the agent stream's invalidation verdict, `ThesisInvalidated`, [DEC-2202](../project/decisions/DEC-2202.md); v0.50 closes the agentic research run's records, [DEC-2203](../project/decisions/DEC-2203.md); v0.51 closes the control stream's `NotificationAddressChanged` and adds `NoticeAttempted` version 3, which names the web-push address an attempt went to, [DEC-2505](../project/decisions/DEC-2505.md); v0.52 types `EvaluationRunRecorded`'s `theses_head` as record hashes, [DEC-1142](../project/decisions/DEC-1142.md); v0.53 adds the deployment meter stream, [DEC-2207](../project/decisions/DEC-2207.md); v0.54 names the deployment's connection on `AgentDeployed` version 2, with rules 200 and 201, [DEC-1171](../project/decisions/DEC-1171.md); v0.55 adds the desk's `OwnerQuestionSubmitted` and the `answer` tool's check codes, [DEC-2240](../project/decisions/DEC-2240.md); v0.56 deploys an agent id once and reads a stop as final whether or not its deployment was read, [DEC-1173](../project/decisions/DEC-1173.md); v0.57 closes the owner's acknowledgment and adds its request, [DEC-2509](../project/decisions/DEC-2509.md); v0.58 closes the account stream's risk-limit records, [DEC-2301](../project/decisions/DEC-2301.md); v0.59 names `copied_from` on the account stream's `KillSwitchActivated`, [DEC-2003](../project/decisions/DEC-2003.md) item 4; v0.60 names `cancels` on the account stream's `KillSwitchActivated` as the orders whose cancel never lapses, [DEC-2302](../project/decisions/DEC-2302.md) item 2; v0.61 says when §9.28's executor judges an acknowledgment's step-up and reads the control stream for reuse, and adds §9.28's vectors, [DEC-2509](../project/decisions/DEC-2509.md); v0.62 adds the control stream's `StepUpConfirmed`, [DEC-2104](../project/decisions/DEC-2104.md); v0.63 adds `MarkUpdated` version 2, with the risk mark's bid, sanity verdict, and session, [DEC-2305](../project/decisions/DEC-2305.md); v0.64 states that rule 200 holds at the writers by construction, [DEC-1171](../project/decisions/DEC-1171.md); v0.65 receives an exit proposed at the market as `type` `market` on `IntentReceived`, with stream rule 245, [DEC-2037](../project/decisions/DEC-2037.md); v0.66 lets a workspace service claim, on the owner's Stop, the stream of an agent whose runtime never ran, and retire it, with rule 246, [DEC-2039](../project/decisions/DEC-2039.md); v0.67 journals an owner exit's confirmed bid beside its intent as `OwnerConfirmationReceived`, with rules 247 and 248, [DEC-2040](../project/decisions/DEC-2040.md); v0.68 journals an Alpaca OAuth connect's token breadth and the disclosure the owner confirmed on `ConnectionRequested` version 2, with rule 255, [DEC-2621](../project/decisions/DEC-2621.md); v0.69 closes the passkey credential records, with rules 249 to 251, [DEC-2108](../project/decisions/DEC-2108.md); v0.70 lets the deployment manager start an OAuth connect's token exchange, and stop it when its request closes, with rule 256, [DEC-1175](../project/decisions/DEC-1175.md); v0.71 adds the passkey assertion record `StepUpAsserted`, with rules 252 to 254, and `CredentialEnrolled` version 2's `backup_eligible`, [DEC-2122](../project/decisions/DEC-2122.md); v0.72 adds `ProtectionChanged` version 2, which discloses the unprotected fraction of a fractional equity position, under rule 44, [DEC-2312](../project/decisions/DEC-2312.md); v0.74 lets the deployment manager start an OAuth connect's executor on its token exchange's success, and stop it when its connection closes, with rule 257, [DEC-2335](../project/decisions/DEC-2335.md); v0.75 journals each close-position request of an account kill switch as `AccountCloseRequested` before it is sent, with rules 258 to 260, [DEC-2337](../project/decisions/DEC-2337.md); v0.76 says when an `AgentStopped` may retire an agent that ran, and lets a kill switch start an executor no agent wants, [DEC-2344](../project/decisions/DEC-2344.md); v0.77 adds the connection role records the role provisioner writes, with rules 261 to 263, and starts a token exchange only once its roles are made, [DEC-2350](../project/decisions/DEC-2350.md); v0.78 adds the notice kind `kill_switch_unapplied`, [DEC-2351](../project/decisions/DEC-2351.md); v0.79 adds the agent stream's `RetirementHeld` and the kind `retirement_held`, with rules 265 and 266, [DEC-2352](../project/decisions/DEC-2352.md); v0.80 lets the executor's fold read `OrderStateChanged` version 2's `broker_order_id` to attribute a broker fill that names only the broker's order id, [DEC-2353](../project/decisions/DEC-2353.md); v0.81 owes the removal of roles made after their request closed, in rule 263, [DEC-2350](../project/decisions/DEC-2350.md) item 7; v0.82 lets `StepUpConfirmed` confirm a paper OAuth connect, `action_kind` `connection`, under rule 242, [DEC-2125](../project/decisions/DEC-2125.md); v0.84 adds `MandateConfirmed` and `AgentDeployed` version 3, which carry the passkey step-up evidence rule 253 reads, with rules 267 and 268, [DEC-2131](../project/decisions/DEC-2131.md); v0.85 adds `DecisionMade` version 3, which names the checks the cell's runtime's advisory dry run deferred to the binding gate and the stored bars' figures it read, with rules 269 and 270, [DEC-2388](../project/decisions/DEC-2388.md); v0.86 says the agent runtime's fold reads the thesis records and `ThesisInvalidated` to sell a position whose thesis ended, [DEC-2391](../project/decisions/DEC-2391.md); v0.87 says what an event-triggered monitoring run's `trigger.cause` names, with rule 271, [DEC-2392](../project/decisions/DEC-2392.md); v0.88 closes the account stream's `AccountRestrictionChanged` with its cause, with rules 279 and 280, [DEC-2137](../project/decisions/DEC-2137.md); v0.89 adds `AccountSnapshotRecorded` version 2, which journals the connector's declared account type and day-trading regime and the broker's prior-close equity and maintenance margin, [DEC-2398](../project/decisions/DEC-2398.md); v0.90 closes the account stream's `KillSwitchActivated` as the executor writes it, with rules 281, 282 and 293 to 296, [DEC-2139](../project/decisions/DEC-2139.md); v0.91 adds the customer real-money gate's records `RealMoneyGateChanged` and `RealMoneyRefused`, with rules 273 and 274, [DEC-2134](../project/decisions/DEC-2134.md); v0.92 adds `ApprovalRequested` version 3, which names the checks the dry run deferred inside the hashed content, with rules 291 and 292, [DEC-2515](../project/decisions/DEC-2515.md); v0.93 closes the account stream's `ExternalActivityIngested`, `RejectObserved`, and `OrderAbandoned`, with rules 297 and 298, [DEC-2803](../project/decisions/DEC-2803.md); v0.94 closes the executor's `ClockAdvanced` copy on the account stream, with rules 301 and 302, [DEC-2808](../project/decisions/DEC-2808.md); v0.95 adds `FillApplied` version 2, which records an unattributed fill with a `null` client order id, with rule 299, [DEC-2812](../project/decisions/DEC-2812.md); v0.96 journals every account-restriction cause in force, with stream rule 309, [DEC-2816](../project/decisions/DEC-2816.md); v0.97 adds the news stream and `NewsItemStored`, with rules 283 to 286, [DEC-2278](../project/decisions/DEC-2278.md); v0.98 lets an unattributed `FillApplied` name no instrument snapshot, with rule 300, [DEC-2817](../project/decisions/DEC-2817.md); v0.99 binds an abandonment's `client_order_id` to its intent, with rule 303, [DEC-2820](../project/decisions/DEC-2820.md); v0.101 adds the kind `connection_unavailable`, an account restricted because the platform cannot reach or use its connection, beside the broker's `account_restriction`, so the kind set holds 36, [DEC-2520](../project/decisions/DEC-2520.md); v0.102 lifts an account restriction cause by cause, with `AccountRestrictionLifted`, version 2 of the acknowledgment records, and stream rules 310 to 312, [DEC-2824](../project/decisions/DEC-2824.md); v0.103 has the news reader keep no raw response, ids an item by its vendor id and version, and polls the tickers of the registered instrument snapshots, and names the news stream in §9.13's range grammar and the anchors' stream types, [DEC-2280](../project/decisions/DEC-2280.md); v0.104 lets an owner acknowledge a reconciliation hold, [DEC-2830](../project/decisions/DEC-2830.md); v0.107 says the account's executor journals the `connection_unavailable` restriction and its alert before the connection's state change, from the broker's answers counted per class, [DEC-2826](../project/decisions/DEC-2826.md) and [DEC-2832](../project/decisions/DEC-2832.md); v0.108 types the intent of the executor's own exits, `w-…` and `k-…`, on `IntentReceived`, `GateDecided`, `OrderRequestRecorded` and `ProtectionChanged`, [DEC-2845](../project/decisions/DEC-2845.md); v0.109 adds `GateDecided` version 3 with the executor's own `sized_qty`, `parked` and `held_long`, with rules 313 to 315, [DEC-2848](../project/decisions/DEC-2848.md); v0.110 admits the purpose `evaluation` on `MeterReserved`, a platform call that evaluates a registry entry and has no agent, under rule 156, [DEC-2216](../project/decisions/DEC-2216.md); v0.111 adds the kinds `protection_unattributed` and `attribution_deficit`, an owner told once per episode of held openings, so the kind set holds 38, [DEC-2855](../project/decisions/DEC-2855.md); v0.112 names the hot store's refusal of a news reader's write outside the news stream, [DEC-2278](../project/decisions/DEC-2278.md) item 2); changes need a decision-log entry (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) (version 3, with the generated `agent_stream` and additive `production_config_refs` and `policy_overlay` sections of §9.1, additive `broker_profile` section of §9, `control_stream` section of §9.2, `risk_state` section of §9.3, `research` section of §9.4, `account_stream` section of §9.5, its `protection_shapes` section of rules 41 and 44, and its `protection_fraction` section of `ProtectionChanged` version 2, `approval_answers` section of §9.7, its `approval_timeouts` section of `ApprovalTimedOut` and rule 135, and its `approval_cancellations` section of `ApprovalCanceled`, `approval_requests` section of §9.6's `ApprovalRequested` versions 1 and 2 and rule 136, its `approval_deferrals` section of version 3 and rules 291 and 292 with its `ranges` of §11's `approval_deferral_mismatch`, `connections` section of §9.8, its `connection_requests` section of rule 131, its `connection_ranges` section of §11's connection anchor, and its `connection_revocations` section of rule 68, `workspace_api` section of §9.9, `client_actor` section of §3 and §9.10, `hold` section of §9.11 with its `held_mismatch` range cases, `membership` and `membership_fold` sections of §9.12, `records_access` section of §9.13 with its `break_glass_cause_mismatch` range checks, `cold_records` section of §9.14, `verification_runs` section of §9.13's `VerificationRun` version 2, `segment_rows` section of §11's `segment_rows_mismatch`, `control_plane` section of §9.17, `backtest_records` section of §9.18, `model_gateway` section of §9.19, `deployment_manager` section of §9.21, `thesis_invalidation` section of §9.22, `agent_deployed` section of §9.24 and its `stops` streams of rule 246, `risk_limits` section of §9.27, `acknowledgments` section of §9.28, `step_up_confirmations` section of §9.29 with its `connection_drafts`, `risk_marks` section of §9.30, `market_intents` section of rule 245, `owner_confirmations` section of rules 247 and 248, `credentials` section of §9.31, `connection_requested` section of §9.8's `ConnectionRequested` version 2 and rule 255, `step_up_assertions` section of §9.32, `stepped_up_records` section of §9.9's `MandateConfirmed` and §9.24's `AgentDeployed` version 3 and rules 267 and 268, `account_closes` section of §9.5's `AccountCloseRequested` and rules 258 to 260, `account_restrictions` section of §9.5's `AccountRestrictionChanged` and rules 279 and 280, and `account_switches` section of §9.5's account-stream `KillSwitchActivated` and rules 281, 282, and 293 to 296, and `account_activity` section of §9.5's `ExternalActivityIngested`, `RejectObserved`, and `OrderAbandoned` and rules 297, 298 and 303, `account_clock` section of §9.5's account-stream `ClockAdvanced` and rules 301 and 302, `fill_applied` section of §9.5's `FillApplied` version 2 and rules 299 and 300, `exit_intents` section of §9.1's `intent` type on the records the executor's own exits write, and `gate_v3` section of §9.5's `GateDecided` version 3 and rules 313 to 315; [reference/journal/generate.py](../../reference/journal/generate.py)) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.

## Change history

- **v0.112 ([DEC-2278](../project/decisions/DEC-2278.md) item 2, 2026-10-28; lane SC):** §6.1's
  append-only enforcement adds the news reader's: its role, `mandate_news_reader`, may write only
  `news:` streams, and the hot store refuses any id, event, head, or head update it writes on
  another stream with `insufficient_privilege`, whichever role the session has set. It writes no
  record, no rule changes, and every vector is unchanged.
- **v0.111 ([DEC-2855](../project/decisions/DEC-2855.md), 2026-10-27; the founder's ruling, lane
  RK):** §9.15's closed set of kinds gains `protection_unattributed` and `attribution_deficit`
  (safety, after `protection`), 36 kinds to 38: the executor's `OwnerAlertSent` for the
  `GateDecided` that begins an episode of openings held for that reason (notifications spec §3.2).
  No record gains a member, and every vector is unchanged.
- **v0.110 ([DEC-2216](../project/decisions/DEC-2216.md), 2026-10-27; lane GW):** §9.19's
  `MeterReserved` admits the purpose `evaluation`: a call the platform makes to evaluate a registry
  entry (inference spec §4.3's gate and the live start's identity probe). It has no agent, so rule
  156 makes `agent_id` `null` exactly for `compiler` and `evaluation`. Every cap that is not an
  agent's binds it as it binds any call.
- **v0.109 ([DEC-2848](../project/decisions/DEC-2848.md), 2026-10-27; lane X):** §9.5 adds
  `GateDecided` version 3. The executor already wrote, and its fold already read, three members of
  its own on version 2, whose schema is closed, so a real journal refused each such record:
  - `sized_qty`, on a discretionary exit sized beside an exit ladder's rungs (DEC-410 item 3),
    which lost the exit's whole batch;
  - `parked`, on a ladder step held while no session is open (DEC-260 (18));
  - `held_long`, on an exit held past its age (the coordinator's ruling D2).

  Version 3 admits the three as nullable members, with rules 313 to 315 binding each to the
  decision it belongs to. The legacy `evaluation` the executor also wrote is not admitted, since
  `data_profile` carries it (DEC-457). Versions 1 and 2 are unchanged, and no stored vector's
  outcome or hash changes: the new `gate_v3` section holds the cases, and the vectors stay version 3.
- **v0.108 ([DEC-2845](../project/decisions/DEC-2845.md), 2026-10-27; lane X):** the account
  stream records the executor's own exits. Trading spec §2.3 names a kill switch's flatten
  `k-{switch}-{n}` and the triggered-stop watchdog's exit `w-{event}` (DEC-485 item 2, DEC-160).
  §9.5 typed `intent_id` as a `ulid` on every record such an exit writes, so the journal refused
  each batch that received, gated, or protected one. Since v0.93 only `OrderAbandoned` read them,
  as an `id`.
  - §9.1 adds the type `intent`: a `ulid`, or exactly `w-{ulid}` or `k-{ulid}-{n}`, with `n` a
    decimal from 0 to 999 with no leading zero. It admits no other form.
  - `intent_id` takes the type on `IntentReceived` and `GateDecided` at versions 1 and 2, on
    `OrderRequestRecorded`, and on `ProtectionChanged` at versions 1 and 2.
  - §2's sentence that `intent_id` is the agent stream's `IntentProposed` now excepts the
    executor's own exits, which no agent proposed.
  - No rule changes, and no stored vector's outcome or hash: the new `exit_intents` section holds
    the cases, and the vectors stay version 3.
- **v0.107 ([DEC-2826](../project/decisions/DEC-2826.md), [DEC-2832](../project/decisions/DEC-2832.md),
  2026-10-26; E7-2z, lane EX):** §9.5's sentence that left the `connection_unavailable`
  restriction and its alert to E7-2z now says what the account's executor journals and when.
  - **What it journals:** the restriction and its alert in one batch, before the next opening once
    a count is at its threshold, with every agent `exits_only` when the account state becomes
    stricter. The connection's `ConnectionStateChanged` follows in a later batch, once per reason
    since the state was entered or the condition last cleared. This supersedes v0.96's "stay
    unjournaled". The executor counts the broker's answers per class
    (connections spec §8.2): a lost or unreadable answer is a network failure, and a refused
    credential an authorization failure.
  - **What does not change:** no record, member, or rule. The existing rules 59, 60, 68 and 309
    already admit these records in this order, so the vectors stay version 3 and nothing is
    regenerated. v0.106 is held by another branch's change.
- **v0.106 ([DEC-2523](../project/decisions/DEC-2523.md), 2026-10-26; lane GL):** §9.28 check 3
  admits `passkey` step-up on an acknowledgment and its request in every environment, as identity
  spec §6.1 and §7.3 allow it. `cli_confirm` stays as it was: `paper`, and `live` only on the
  founder's own connection (DEC-529 item 3, DEC-923). It is a reading that adds no risk (DEC-176),
  since passkey is stronger evidence than `cli_confirm`. The executor's judgment gains two cases,
  passkey in paper and in live off the founder's connection, each lifting, and a seeded mutant that
  drops passkey; the vectors stay version 3.
- **v0.105 ([DEC-2522](../project/decisions/DEC-2522.md), 2026-10-26; E21-30, lane GL):** §11
  gains the agent-stream check `approval_deferral_mismatch`, which is rule 292 across batches.
  - An `ApprovalRequested` must name a cause, that cause must be an earlier `DecisionMade` on the
    stream, and the request's deferrals must equal the decision's. Each side's absent list reads as
    empty.
  - A cause outside the range's earlier events fails on a full chain and is unchecked on a later
    range.
  - §9.13's `VerificationRun` lists the code at both versions, and rule 111 classes it as reported
    at an event. This follows v0.29's reading for codes added to §11, which only tightens (DEC-176).
  - No record or rule changes. The `approval_deferrals` section gains `ranges`, so the vectors stay
    version 3.
- **v0.104 ([DEC-2830](../project/decisions/DEC-2830.md), 2026-10-26; E7-4b, lane L1):** §9.28's
  version-2 `subject` gains `reconciliation`, whose `event` names the `AgentModeApplied` that
  applied a reconciliation hold (trading spec §7.1, §11). The executor judges the copy by §9.28's
  four checks, the hold's own record standing in for the trigger, and lifts that hold alone with an
  `AgentModeApplied` to `normal` as the record right after the copy. The `restriction_lifts`
  section gains the subject, and the new `reconciliation_acknowledgments` section the executor's
  judgment; the vectors stay version 3.
- **v0.103 ([DEC-2280](../project/decisions/DEC-2280.md), 2026-10-26; E2-10, lane NE):** §9.37's
  writer keeps no raw response until the data plane has a durable store and DEC-433 item 20
  allows it. Its `item_id` is `<vendor id>:<vendor version>`. It polls the tickers of the
  workspace's registered instrument snapshots for the active theses' and held instruments. The
  record and its rules 283 to 286 are unchanged. §9.13's `stream_id` type, which §9.14's anchor
  leaves reuse, and §9.15's `stream_id` row now name the news stream, as v0.46 did for the meter
  stream, so an anchor, an export, a verification range, and a notice's cause can name it; v0.97
  left them out. The vectors gain the cases for a `news:` stream id there, and test vectors stay
  at version 3.
- **v0.102 ([DEC-2824](../project/decisions/DEC-2824.md), 2026-10-26; E7-2y slice A, lane L1):**
  An account restriction lifts cause by cause, as trading spec §7.3's "Lifts when" says. §9.28
  also fixes where a refused acknowledgment's refusal goes: the record right after its copy, in the
  copy's batch.
  - §9.28's `OwnerAcknowledged` and `OwnerAcknowledgmentRequested` gain version 2, whose `subject`
    (`limit` or `restriction`) lets an acknowledgment name an `AccountRestrictionChanged`. Version 1
    reads as `limit`. The executor judges a restriction's acknowledgment copy by §9.28's four
    checks.
  - §9.5 adds `AccountRestrictionLifted` at schema version 1. It names its cause, the accepted
    acknowledgment copy, the evidence that completes the lift, and the account state left.
  - New stream rules on the account stream:
    - 310: the cause is in force, and the state left is the strictest of the causes still in
      force;
    - 311: the acknowledgment copy is unrefused and names a restriction of the cause that stands
      to the lift, so it lifts once;
    - 312: for a broker cause, the latest account report before the lift, clean and after the
      acknowledgment; for the connection's, a `condition_cleared` before the acknowledgment with no
      degradation since, and the connection's `acknowledged` change between the acknowledgment and
      the lift.
  - Rule 309 now counts a cause in force only until a lift names it.
  - The agents follow the account state: the state left's entry is applied first, then every other
    account entry is removed, so no entry outlives its causes.
  - A restriction's acknowledgment is judged under the strictest independence of every agent of
    the account.
  - The new `restriction_lifts` section holds the cases (the `acknowledgments` section renames one),
    and the vectors stay version 3.
- **v0.99 ([DEC-2820](../project/decisions/DEC-2820.md), 2026-10-26; X10a, lane MC):** §9.5's
  `OrderAbandoned` gains rule 303: its `client_order_id` is `md-` followed by its `intent_id`, the
  form trading spec §2.3 states (v0.21, DEC-2807), which DEC-2803 item 5 left unbound until §2.3
  stated it. The executor writes it so already. The `account_activity` section's abandonment cases
  follow, and test vectors stay at version 3.
- **v0.98 ([DEC-2817](../project/decisions/DEC-2817.md), 2026-10-26; X10e, lane MC):** §9.5's
  `FillApplied` version 2 requires `instrument_snapshot` in its `config_refs` only of a fill that
  names an order of ours (rule 300). An unattributed fill in an instrument the platform holds no
  snapshot of names the fee configuration and both calendars alone. Before, the journal refused it,
  DEC-2805's hold stood in, and the activity cursor never passed it. Version 1, `LateFillApplied`,
  and a fill of an order of ours are unchanged. The `fill_applied` section gains the cases, and
  test vectors stay at version 3.
- **v0.97 ([DEC-2278](../project/decisions/DEC-2278.md), 2026-10-26; E2-10, lane NE):** §2, §3,
  and the new §9.37 add the news stream, `news:{workspace_id}`, which the workspace's news reader
  writes, its `StreamOpened`, and its one record, `NewsItemStored`, at schema version 1. New rules:
  - 283: sorted, unique tickers, one to 32, the item id's grammar, and known not before published;
  - 284: one record per `item_id`;
  - 285: known times never go back;
  - 286: the stream id and the `system` actor.

  The record names the item's typed record, which holds no licensed text, and no raw response. The
  runtime offers a recorded item with its `item_ref` as `data_ref`, so rule 271 binds one stored
  item to one run. Vectors follow with the record's registration, so the vectors stay version 3.
- **v0.96 ([DEC-2816](../project/decisions/DEC-2816.md), 2026-10-26; E7-2v slice A, lane L1):**
  §9.5 journals every account-restriction cause in force, not only one that makes the account state
  stricter. New stream rule 309: an `AccountRestrictionChanged` names a cause not already in force at
  that restriction or a stricter one, so each cause is on the stream once per restriction for its
  lift to name. The account state is the strictest restriction in force, never the last record's,
  and a record that leaves it unchanged moves no agent. The lift (E7-2y) and the
  connection's restriction and alert (E7-2z) stay unjournaled, and no restriction is lifted until
  then. Rules 310 to 312 are held for the lift. The `account_restrictions` section gains its
  streams; no other section changes, so the vectors stay version 3.
- **v0.95 ([DEC-2812](../project/decisions/DEC-2812.md), 2026-10-26; X10c, lane MC):** §9.5 adds
  `FillApplied` version 2. It has version 1's members with `client_order_id` `text?`, which is
  `null` for a fill that names no order of ours (trading spec §6.1, §7.1). Version 1 refused every
  such fill, so the executor's whole batch for an external fill was refused (DEC-2805). Rule 299
  binds a non-null id to trading spec §2.3's form. Version 1 and `LateFillApplied` version 1 are
  unchanged. Rule 300 stays unused. Test vectors stay at version 3, with a new `fill_applied` section.
- **v0.94 ([DEC-2808](../project/decisions/DEC-2808.md), 2026-10-26; E6-15 slice C, lane RK):**
  §9.5 closes the executor's `ClockAdvanced` copy on the account stream at schema version 1: the
  scheduler's tick as `clock`, and the same tick as `risk_clock`, which every account-stream row the
  executor's fold reads carries (§2). New rules 301 (`risk_clock` equals `clock`) and 302 (the copy
  names the scheduler's tick as its `causation_id`). `append` refused the copy as `unknown_schema`
  before, so the risk day could not roll: the account risk fold derives `RiskDayStarted` from the
  copy that crosses New York midnight ([DEC-2804](../project/decisions/DEC-2804.md) item 5).
  `TradingDayStarted`'s copy stays open (§9.20, E7-4 slice 5). The scheduler stream's
  `ClockAdvanced` (§9.20) is unchanged. Rules 299 and 300 stay unused ([DEC-2803](../project/decisions/DEC-2803.md)
  item 5). The new `account_clock` section holds the vectors; no other section changes, so the
  vectors stay version 3.
- **v0.93 ([DEC-2803](../project/decisions/DEC-2803.md), 2026-10-26; X10, lane MC):** §9.5 closes
  the account stream's `ExternalActivityIngested` (with its `source`, `open_order` or `fill`, and
  rules 297 and 298 on which members each source has), `RejectObserved`, and `OrderAbandoned` (with
  its closed `reason`) at schema version 1. `append` refused all three as `unknown_schema` before,
  so the executor's first external order, broker reject or abandoned intent refused its batch
  whole and stopped its process. Each member the broker supplies is nullable, written `null` when
  the broker sends it empty. The executor writes `source` and the `null` members in the code change
  that follows the journal's. The new `account_activity` section holds the vectors; no other
  section changes, so the vectors stay version 3.
- **v0.92 ([DEC-2515](../project/decisions/DEC-2515.md), 2026-10-26; E21-30, lane D1):**
  `ApprovalRequested` (§9.6) gains version 3: version 2's content, then `deferred_to_binding`, the
  checks the cell's runtime's dry run deferred to the executor's binding gate, in `DecisionMade`
  version 3's names, inside the hashed content, so the approver is told which checks have not run
  (DEC-2378 item 11 condition 4, mandate spec §6.4). New rule 291: the list is ascending, never
  empty, and names the liquidity floor and participation together. New rule 292, on the batch, at
  every version: a request names exactly the deferrals of the `DecisionMade` that asks, when that
  decision is in its batch. The vectors are the additive `approval_deferrals` section; in
  the `approval_requests` section only the case that refused version 3 as unknown now refuses
  version 4, and its counts are unchanged.
- **v0.91 ([DEC-2134](../project/decisions/DEC-2134.md), 2026-10-26; lane GL):** the control
  stream gains the customer real-money gate's two records at schema version 1 (§9.36):
  `RealMoneyGateChanged`, a platform operator's setting of whether the workspace may reach an
  environment other than `paper`, and `RealMoneyRefused`, the workspace API's record of a request
  it refused because that setting is off. New consistency rules 273 and 274. No reader of any other
  record changes, no record feeds a `ValidationContext`, and no other section's vectors change, so
  the vectors stay version 3. The new `real_money` section holds the vectors.
- **v0.90 ([DEC-2139](../project/decisions/DEC-2139.md), 2026-10-26; lane L1):** §9.5 closes the
  account stream's `KillSwitchActivated` at schema version 1, with the members the executor writes:
  `scope` (`agent` or `account`), `subject`, `initiator`, `purpose`, `cancels`, `closes`,
  `deferred`, the owner's confirmation (`floor`, `bid`, `bid_size`), `copied_from` (v0.59), and
  `risk_clock`, with rules 281, 282, and 293 to 296. `append` refused the record as
  `unknown_schema` before ("not yet closed" since v0.59), so in a deployed cell the executor's
  batch for every kill switch it applied was refused and the executor crash-looped. Every rule
  holds of what the executor writes today. The new `account_switches` section holds the vectors; no
  other section changes, so the vectors stay version 3.
- **v0.89 ([DEC-2398](../project/decisions/DEC-2398.md), 2026-10-26; E21-30, lane XF):** the
  account stream's `AccountSnapshotRecorded` gains version 2 (§9.35): version 1's members, with
  four after `accrued_fees`, what trading spec §9.2's day-trade budget reads: the connector's
  declared `account_type` and `day_trading_regime` (§7.2, DEC-840), each `null` when it declares
  none, and the broker's `last_equity` and `maintenance_margin` (the prior-close equity `legacy_pdt`
  compares with its threshold, and the maintenance excess `intraday_margin` checks). Version 1
  stays registered and is read as carrying none of them, so a reader that needs them holds an
  opening on it. No new rule: rule 24 applies to both versions. This supersedes DEC-524 item 3 for this record only;
  `AccountStateObserved` is unchanged.
- **v0.88 ([DEC-2137](../project/decisions/DEC-2137.md), 2026-10-26; E7-2u, lane L1):** §9.5 closes
  the account stream's `AccountRestrictionChanged` at schema version 1: `restriction`
  (`closing_only` or `blocked`), its closed `cause` (`broker_reject`, `broker_notice`, or
  `connection_unavailable`; trading spec §7.3, [DEC-441](../project/decisions/DEC-441.md) item 23),
  §7.3's `reason_code`, and `risk_clock`, with rules 279 and 280 binding the three to §7.3's table.
  `append` refused the record as `unknown_schema` before, so the executor's batch for a restricted
  account was refused whole and its process stopped. The executor writes `cause` in the code change
  that follows the journal's; until then the batch is still refused, at `payload.cause`. The new
  `account_restrictions` section holds the vectors; no other section changes, so the vectors stay
  version 3.
- **v0.87 ([DEC-2392](../project/decisions/DEC-2392.md), 2026-10-26; E19-12, lane NE):** §9.23's
  `RunStarted` with `trigger.kind` `event` (a `monitor` run, rule 172) names as `trigger.cause` the
  `ObservationRecorded` of the item that triggered it, which the runtime journals in the run's
  starting batch, before `RunStarted`. New stream rule 271. No record gains a member.
- **v0.86 ([DEC-2391](../project/decisions/DEC-2391.md), 2026-10-26; lane NE):** no record or rule
  changes. §9.4 and §9.22 say what the agent runtime's fold reads from the thesis records and
  `ThesisInvalidated`: each instrument's current thesis and when it ended, which mandate spec v0.10
  §8.3 step 2 turns into a discretionary exit with `exit_origin` `removed_instrument` while the
  agent holds the instrument. Neither record maps to a `JournaledFact`, so no risk state folds from
  either.
- **v0.85 ([DEC-2388](../project/decisions/DEC-2388.md) item 10, 2026-10-26; E21-30, lane D1):**
  `DecisionMade` (§9.1) gains version 3, the cell's runtime's advisory dry run (DEC-2378 item 11).
  Its `config_refs` are version 1's, `mandate_version`; its payload is version 1's members, then
  `deferred_to_binding`, the checks or parts of checks the dry run deferred to the executor's
  binding gate and never reported passed, and `liquidity`, the stored bars' four figures and the
  risk clock it read them at (DEC-2274 item 3). New consistency rules 269 and 270: an allowed
  opening names at least one deferral and nothing else does, in byte order; the liquidity floor and
  participation are deferred together, exactly when no figures were read; and only an opening or
  an increase carries figures. A dry run that defers nothing writes version 1 or 2 as before, and
  every reader of version 1 reads version 3 as version 1 and ignores the two members. Version 2 is
  unchanged: its stricter `config_refs` and version 3's members are not combined yet.
- **v0.84 ([DEC-2131](../project/decisions/DEC-2131.md), 2026-10-26; E9-4 S5, claim #759):**
  `MandateConfirmed` (§9.9) and `AgentDeployed` (§9.24) gain version 3: version 2's members, then
  `step_up`, §9.2's `{assertion_id, authenticated_at, method}`, never `null`. §9.32's rule 253 reads
  the authorized event's `payload.step_up`, which versions 1 and 2 of both lack (their evidence is
  in the record `record_ref` names), so `append` refused every batch that committed a confirmation
  or a deployment with its `StepUpAsserted`. Version 3 is written exactly when the workspace API
  commits the event under passkey step-up, with the evidence it judged; under any other step-up,
  or none, it writes version 2 as before, and the CLI is unchanged. Rules 18, 77, 78 and §9.2's
  mapping apply to `MandateConfirmed` version 3 as to version 2, and rules 22, 200 and 201 and the
  mapping to `AgentDeployed` version 3 as to version 2; every reader of version 2 reads version 3
  as version 2 and ignores `step_up`. New consistency rules 267 and 268: a version 3's `step_up` is
  a passkey's, authenticated 0 to 300 seconds before the envelope's `event_time` (identity spec
  §7.1, as rule 102 judges it), and a version-3 `AgentDeployed` is written by a `user`. Rule 253 is
  unchanged. Versions 1 and 2 stay registered and unchanged (§8). The new `stepped_up_records`
  section holds the vectors; the `agent_deployed` and `workspace_api` sections' unregistered-version
  cases move from version 3 to version 4, and no other section changes, so the vectors stay version 3. Readers move
  before any writer (DEC-2131 item 4): no writer writes version 3 until every reader reads it.
- **v0.82 ([DEC-2125](../project/decisions/DEC-2125.md), the founder's, 2026-10-25; E10-13, claim
  #754):** §9.29's `StepUpConfirmed` confirms a paper OAuth connect as well as a paper grant: rule
  242 admits `action_kind` `connection` beside `approve`, its `action_digest` the digest of the
  connect's action object, which the CLI computes from the workspace and the current disclosure
  document and version and stores, so `artifact_refs` names it as it names every `ref`. The record
  is otherwise unchanged and stays at schema version 1. The section's `connection_drafts` hold the
  connect's valid drafts; its invalid drafts add the kinds a connect is not (`connect`,
  `Connection`, a padded `connection`).
- **v0.81 ([DEC-2350](../project/decisions/DEC-2350.md) item 7; claim #1901):** rule 263 ties a
  `ConnectionRolesRemoved` to the request its connection's latest `ConnectionRolesMade` answers:
  its cause is a closer of that connection journaled after that request, rather than after the
  `ConnectionRolesMade` itself. The provisioner writes to the vault before it journals, so a request
  that closes while its roles are being made, or a `ConnectionRolesMade` that loses an append race
  to the closer, leaves the roles' record after the close; v0.77 then owed no removal and left the
  token exchange's role in the vault. It is owed now, citing that close. A removal still follows
  the latest `ConnectionRolesMade`, at most once, and never cites an earlier request's closer.
  The coordinator assigned this change v0.81, after the open branches' v0.78 to v0.80.
  Agent-accepted under DEC-176: it removes a role that would otherwise stay, adds no access, and
  refuses no record written today. The `connection_roles` section gains two streams, one stream and
  one vector mutant.
- **v0.80 ([DEC-2353](../project/decisions/DEC-2353.md); E7-4, claim #137):** §9.16's
  `broker_order_id` on `OrderStateChanged` version 2 was written and never read by the executor's
  fold. The fold now reads it for one thing: a broker fill that names no `client_order_id` (Alpaca's
  `FILL` activity names only the broker's order id) is attributed to the order whose version-2
  records carry that id, and judged as a fill naming that order. Two ids on one order, no id, or one
  id on two orders attribute nothing, and the fill stays external activity as before. No record,
  member, or rule changes, so no vector does. It only tightens (DEC-176): without it, every fill
  Alpaca reports leaves the agent's sub-ledger empty, so its kill switch sells none of its shares
  (`AGENTS.md` rule 13). The version number follows the open branches' v0.72 to v0.79.
- **v0.79 ([DEC-2352](../project/decisions/DEC-2352.md), an agent reading under
  [DEC-176](../project/04-decision-log.md#decisions)):** §9.34 adds the agent stream's
  `RetirementHeld` at schema version 1, the runtime's record that a retirement has waited another
  24 hours for what the agent still holds (DEC-2344 item 2), with rule 265 on the record and stream
  rule 266 on its one sequence per agent: each record with its alert, periods counting records a
  day apart, and none after the retirement ends at its own `AgentStopped` or an owner's resume or
  stop, a version applied moving the sequence on or ending it for a new cause;
  §9.15's closed set of kinds gains `retirement_held` (safety), 34 to 35. The `retirements` vectors
  are new: 24 invalid and 5 valid drafts and 28 streams, judged by the reference fold and an
  independent oracle, with 16 validator, 10 stream, and 5 vector mutants caught. **Versions and
  numbers:** v0.77 and rules 261 to 263 are #2278's (§9.33), and rule 264 #2294's, so this takes
  v0.79, §9.34, and rules 265 and 266, and renumbers at merge if that order changes.
- **v0.78 ([DEC-2351](../project/decisions/DEC-2351.md), an agent reading under
  [DEC-176](../project/04-decision-log.md#decisions)):** §9.15's closed set of kinds gains
  `kill_switch_unapplied` (safety, after `kill_switch`), 33 kinds to 34: the deployment manager's
  `OwnerAlertSent` for its own unapplied crash loop (notifications spec §3.2, DEC-2344 item 1).
  Stream rule 264 holds the kind to that subject, which the control stream alone shows: no record
  gains a member. The new `kill_switch_unapplied` vectors are 20 control streams, a deployment
  and retirement during the run, an agent wanting it at the loop, a connect, a revoked connection,
  a stop, a new run after a loop, and a runtime's and an exchange's loop among them, judged by the
  fold and an independent oracle, with 6 stream and 2 vector mutants caught. **Versions and
  numbers:** v0.77 is held by #2278 (DEC-2350) and lane V2's #2292 and #2293 stacked on it, and
  rules 261 to 263 by #2278, so this takes v0.78 and rule 264, and renumbers at merge if
  that order changes.
- **v0.77 ([DEC-2350](../project/decisions/DEC-2350.md); DEC-2630 item 7, claim #1901):** §9.33
  adds the role provisioner's three control-stream records, `ConnectionRolesMade`,
  `ConnectionRolesRefused`, and `ConnectionRolesRemoved`, at `schema_version` 1. A connect's token
  exchange and executor log in with Kubernetes-auth roles made per connection (DEC-2060), which
  only the provisioner may make. Rule 261 holds every record to a system actor and a cause; stream
  rules 262 and 263, the provisioner's and failing closed in the verifier's fold, tie each record to
  its request and let it remove only the token exchange's role, never the executor's, so a kill
  switch's start can always log in. §9.21's token
  exchange is started only once its request's `ConnectionRolesMade` is read, so a connect whose
  roles were never made, or were refused, starts nothing and ends by its deadline. The new
  `connection_roles` section holds the vectors; no existing member, rule, or vector changes.
  - **Versions.** Versions follow the coordinator's assignment: v0.72 is lane LV's
    `ProtectionChanged` version 2 (DEC-2312), v0.73 lane L2's active-address definition (DEC-2123),
    v0.74 lane D1's `exchanged` start (DEC-2335), v0.75 lane LV's `AccountCloseRequested`
    (DEC-2337), and v0.76 lane D1's retirement rule (DEC-2344). This change takes v0.77, §9.33, and
    rules 261 to 263, and renumbers at merge if that order changes.
- **v0.76 ([DEC-2344](../project/decisions/DEC-2344.md), an agent reading under
  [DEC-176](../project/04-decision-log.md#decisions)):** §9.2 states its writer's precondition for
  an `AgentStopped` that retires an agent that ran: the account stream's fold shows no position
  attributed to the agent and no working order of its own, working being every non-terminal state
  of trading spec §5.7 (mandate spec §3.1). It is the writer's rule, as rule 254 is: no member,
  rule, or vector changes, since a control-stream check does not read the account stream. It only
  delays a record that ends an agent.
  - **§9.21: an executor no agent wants.** A covering kill switch gives such an executor one
    `kill_switch` start; only a start committed after a switch answers it, since the manager reads
    only the control stream, and a generation that ends with success answers only
    the switches committed before its start. A batch start's `causation_id` is the first switch it
    answers. The manager journals no stop for a generation a switch keeps: it ends by its own exit,
    so `ProcessStopIssued`'s causes are unchanged. A crash loop first raises
    `kill_switch_unapplied`, and "Which fact is the pause" starts a crash-looped slot once for the
    covering switches not yet answered. No member, rule, or vector changes: each only adds a start
    in answer to a kill switch or delays a stop.
  - **Versions.** Versions follow the coordinator's assignment: v0.72 is lane LV's
    `ProtectionChanged` version 2 (DEC-2312), v0.73 lane L2's active-address definition
    (DEC-2123), v0.74 lane D1's `exchanged` start (DEC-2335), and v0.75 lane LV's
    `AccountCloseRequested` (DEC-2337). This change takes v0.76 and renumbers at merge if that
    order changes.
- **v0.75 ([DEC-2337](../project/decisions/DEC-2337.md); E7-4, claim #137):** §9.5 closes the
  account stream's `AccountCloseRequested` at `schema_version` 1: one request of one instrument's
  close-position under an account kill switch, journaled in its own batch before it is sent, naming
  the switch's `KillSwitchActivated`, the quantity held, and its `attempt`. Rule 258 requires a
  positive quantity and an attempt of at least 1; rule 259 makes the switch the record's cause; and
  stream rule 260 refuses a second request with the same switch, instrument, and attempt. A
  re-request goes only after a broker read taken since the last one shows the instrument still held
  and clear; that read is the executor's. Agent-accepted under DEC-176: it journals a request before
  an action the executor already takes, and refuses none written today. The new `account_closes`
  section holds the vectors.
  - **Versions.** Versions follow the coordinator's assignment: v0.72 is lane LV's
    `ProtectionChanged` version 2 (DEC-2312), v0.73 lane L2's active-address definition
    (DEC-2123), and v0.74 lane D1's `exchanged` start (DEC-2335). This change takes v0.75 and rules
    258 to 260 whatever the landing order.
- **v0.74 ([DEC-2335](../project/decisions/DEC-2335.md); DEC-2627 item 5, claim #1901):** §9.21
  admits the start of an OAuth connect's executor on its token exchange's success, and its stop when
  the connection closes: `ProcessStartIssued`'s `cause` adds `exchanged` and `ProcessStopIssued`'s
  adds `connection_closed`. Rule 167 now reads `deployed`, `requested` or `exchanged` exactly at
  generation 1, and rule 257 keeps `exchanged` and `connection_closed` to `executor`. Agent-accepted
  under DEC-176: it admits records that were impossible and refuses none written today. The
  `deployment_manager` section's new vectors cover each. v0.72 and v0.73 are other lanes' changes in
  flight; versions follow landing order.
- **v0.72 ([DEC-2312](../project/decisions/DEC-2312.md), [DEC-2315](../project/decisions/DEC-2315.md), E7-4, claim #137):** §9.5 adds
  `ProtectionChanged` version 2. It tightens nothing and loosens nothing a version-1 record proves
  (DEC-176). Numbered after v0.71, which is lane L1's.
  - **The member.** Version 1's members, then `unprotected_fraction` (`decimal?`), then
    `risk_clock`. Trading spec §5.4 protects only the whole-share part of a fractional equity
    position and discloses the fraction left unprotected; this is where that disclosure is
    recorded. Version 1 is never edited and stays registered.
  - **Rule 44.** At version 2 the fraction may be non-null only on `unprotected_start`, `placed`
    and `unprotected_end`: the records that start a fractional entry's interval, place its whole
    shares' protection, and end the interval with the position's final size, which a position built
    from several fills discloses only there (DEC-2315). Where non-null it is greater than 0 and less
    than 1, read as the normalized decimal. No new rule number. The executor's protection covers
    the whole shares only, never the fraction.
  - **Replay unchanged.** The fold does not read the fraction. Every version-1 record replays as
    before.
  - **Vectors.** A new generated `protection_fraction` section holds the version-2 bases, 7 valid
    drafts and 17 invalid ones. The valid ones are a fraction on a first-fill start, a placement
    and an end, the smallest and largest fractions, an exponent form normalized to 0.4, and no
    fraction. The invalid ones are a fraction on each action that discloses none (`cancelled`, a
    passive start, `intended`, `watchdog`, `exit_unpriced`, `ladder_floor`,
    `expiry_unreplaceable`, `rung_short`, `interval_limit`); 0, a negative, 1, 1 in exponent form,
    or more than 1; a JSON number or text that is no decimal; and a fraction on version 1. It has
    11 validator mutants (one widens the actions by each forbidden one), 1 tightening and 4 vector
    mutants, as `generate.py` prints. It is a section of its own, as `protection_shapes`
    is, so `mandate-journal`'s tests keep their answers until the code change that registers
    version 2 reads it (ES-22). The vectors stay version 3.
- **v0.71 ([DEC-2122](../project/decisions/DEC-2122.md), DEC-2108 item 3; E9-4 S3, claim #759):** §9.32 adds the
  control stream's `StepUpAsserted` at `schema_version` 1. It is the assertion behind one passkey
  step-up, with its credential, its signature counter, and the raw assertion as an artifact,
  appended immediately after the event it authorized in the same batch. Rule 252 requires its
  cause and a user actor. Batch rule 253 binds it to that event's actor and passkey step-up. Stream
  rule 254, a rising counter, is the writer's, and the verifier's fold fails closed on it. §9.2's
  step-up evidence is unchanged. The new `step_up_assertions` section holds the vectors.
  - **`CredentialEnrolled` version 2.** §9.31's enrolment adds `backup_eligible`, the registration's
    backup-eligible flag, which the step-up verifier holds every later assertion to. Version 1
    stays registered and unchanged, the API writes version 2 only, and the verifier never counts a
    version 1 credential, which carries no flag. It adds a member and loosens nothing (DEC-176).
    The `credential_enrolled_v2` section holds its vectors, every rule-249 clause read again at
    version 2 among them. The `credentials` section's version-refusal case moves from version 2 to
    version 3, and a version 1 enrolment carrying `backup_eligible` is refused there as `schema`.
    The fold reads version 2 before anything writes it (DEC-2122 item 5).
- **v0.70 ([DEC-1175](../project/decisions/DEC-1175.md); DEC-2627, claim #1901):** §9.21 admits the
  deployment manager's start of an OAuth connect's token exchange, and its stop when the request
  closes: `ProcessStartIssued`'s and `ProcessStopIssued`'s `process` add `token_exchange`, named by
  its `connection_id`; the start's `cause` adds `requested` and the stop's `request_closed`. Rule
  167 now reads `deployed` or `requested` exactly at generation 1, so an exchange is started once,
  and rule 256 ties `requested` and `request_closed` to `token_exchange` both ways.
  `ProcessCrashLooped` keeps `runtime` and `executor` only: an exchange is never restarted or
  looped. Agent-accepted under DEC-176: it admits records that were impossible and refuses none
  written today. The `deployment_manager` section's new vectors cover each.
  - **Versions.** Versions follow landing order (the coordinator's assignment): lane L1's passkey
    records took v0.69, landing first; this change takes v0.70, and L1's `StepUpAsserted` moves to
    v0.71. Rule 256 is this change's whatever the order.
- **v0.69 ([DEC-2108](../project/decisions/DEC-2108.md); E9-4 S3, claim #759):** §9.31 closes the
  control stream's `CredentialEnrolled` and `CredentialRemoved` at `schema_version` 1: a member's
  passkeys, folded by the API's passkey verifier. The public key is an artifact, and the raw
  credential id is journaled so step-up can name the member's own credentials. A first enrolment
  carries no step-up and a 24-hour cool-off, and one stepped up carries none (rule 249). Removal
  needs passkey step-up (rule 250). Stream rule 251, one enrolment per credential, is the writer's,
  and the verifier's fold fails closed on it. The new `credentials` section holds the vectors.
  DEC-2108's `StepUpAsserted` follows in its own change, with rules 252 to 254.
  - **Versions.** E7-27's change took v0.68 and rule 255, landing first; this change takes v0.69
    and keeps rules 249 to 251, as the coordinator assigned.
- **v0.68 ([DEC-2621](../project/decisions/DEC-2621.md); E7-27, E10-13, claim #754):** §9.8 adds
  `ConnectionRequested` version 2: version 1's members, then `credential` (`oauth` or `api_key`)
  and `breadth`, which for an `oauth` request records that the token may also reach the other
  environment (`may_reach`, while U-A4 is open) and the disclosure document and version the owner
  confirmed with the connect's step-up, and is `null` for `api_key`. Rule 255 requires `breadth`
  exactly for `oauth`, an `oauth` request to name `alpaca` and `paper`, and its disclosure to be
  `alpaca_oauth_token_breadth`. Version 1 stays registered and reads as `api_key`: no OAuth start
  was ever written at version 1. E7-27's order, no OAuth establishment before the record of its
  breadth, holds by rule 131 and the establishment's one writer, which appends it only to close an
  open version-2 `oauth` request; no stream rule is added, since §11 cannot tell an OAuth
  establishment from the CLI's API-key record. The new `connection_requested` section holds the vectors, and the `connections`
  section's version-2 refusal moves to version 3. Agent-accepted under DEC-176: it adds a record
  version and refusals and refuses nothing written today. The disclosure's wording is the
  founder's ([DEC-2622](../project/decisions/DEC-2622.md), Proposed).
  - **Versions.** Versions go to changes in the order they land (the coordinator's assignment): this
    change lands first and takes v0.68 and rule 255; lane L1's passkey events take the next versions.
- **v0.67 ([DEC-2040](../project/decisions/DEC-2040.md); E7-30, claim #1498):** §9.5 adds the
  account stream's `OwnerConfirmationReceived` at `schema_version` 1: the bid, bid size, and floor
  the owner confirmed for an owner's exit, journaled immediately after its `IntentReceived` in the
  same batch, so the executor's fold reads the floor back after a restart. It is bound to the one
  intent that follows from the `OwnerExitRequested`, its instrument and quantity, and expires 300 s
  after the step-up. Rule 247 requires its quantity, prices, and expiry to be positive. Batch rule
  248 requires the record immediately before it to be the `IntentReceived` of the same intent,
  instrument, and quantity, with purpose `owner_exit` and a limit that is null or not below the
  floor. `IntentReceived` is unchanged, so it stays the exact copy of the proposal. The
  new `owner_confirmations` section holds the vectors. Agent-accepted under DEC-176: the record
  only lets a sell go where `AGENTS.md` rule 13 already permits it, never below the owner's floor.
  - **Versions.** v0.66 and rule 246 are E21-33 slice 5's (DEC-2039), on main, so this change
    takes v0.67 and rules 247 and 248, as the coordinator assigned.
- **v0.66 ([DEC-2039](../project/decisions/DEC-2039.md); E21-33 slice 5, claim #1491):** §9.21
  says who writes `AgentStopped` for an agent whose runtime never ran: a version-1 deployment, a
  version-2 one rule 201 refuses, or one not yet started. The workspace service that takes the
  owner's Stop first claims the agent's stream: it appends that stream's `StreamOpened` at
  expected head 0 as its own actor, so the append's head check proves nothing opened it. It then
  writes the `AgentStopped`, with reason `owner_stop` and `loss_added` 0. A stream a runtime opened
  sends the Stop down the `OwnerCommandIssued` path, as before; a Stop is never refused. §2 and
  §9.1 say a runtime refuses a claimed stream: it appends and sends nothing, and exits. Stream rule
  246 is the writer's own. The `agent_deployed` vectors gain `stops`, each an agent stream and a
  control stream, and the rule's seeded bugs. No record's schema changes.
  - **Versions.** v0.65 and rule 245 are lane X's (#1783), so this change takes v0.66 and
    rule 246.
- **v0.65 ([DEC-2037](../project/decisions/DEC-2037.md); E7-29, claim #1498):** §9.5 receives an
  exit proposed at the market as `IntentReceived` `type` `market`, with `limit_price` `null`, as
  `IntentProposed` rules 2 and 3 already allow it to be proposed. Rule 245 binds the two members
  at either schema version: `type` is `limit` or `market`, `market` only for an exit, and
  `limit_price` null exactly for `market`. No schema changes: `limit_price` was already nullable.
  The new `market_intents` section holds the vectors: a market exit is valid, and a null limit with
  `type` `limit`, a limit with `type` `market`, and a market opening or increase are each invalid
  at the member rule 245 names. Agent-accepted under DEC-176: the rule only refuses records the
  executor never writes, and it lets an exit reach the core that the executor held before.
  - **Versions.** v0.64 (DEC-1171, #1784) is on main, so this change takes v0.65 and rule 245,
    which no other section uses. It renumbers at merge if that order changes.
- **v0.64 ([DEC-1171](../project/decisions/DEC-1171.md); E21-33, claim #1491):** §9.24 rule 200's
  writer bullet said each writer refuses a draft whose `connection_id` differs from its document's.
  The writers take `connection_id` from the document, so it cannot differ. The bullet now says rule
  200 holds there by construction, and that the draft is appended at the head the writer read, so a
  revocation written in between is decided again (the CLI, at most three decisions, then a journal
  error) or answered `journal_unavailable` (the workspace API, 503). `workspace-api.md`'s deploy row
  says the same.
  Wording only: no record, rule outcome or vector changes.
  - **Versions.** v0.63 (DEC-2305, #1778) is on main, so this change takes v0.64.
- **v0.63 ([DEC-2305](../project/decisions/DEC-2305.md); E6-15, claim #1599):** §9.30 closes the
  account stream's `MarkUpdated` at schema version 2: version 1's members, then `bid`, `sane`, and
  `session`, the three readings mandate spec §5.2's risk fold takes from a mark (trading spec §8.2's
  bid for a long, its sanity verdict, and the session it arrived in). Rule 244 refuses a sane mark
  without a positive bid. Version 1 stays registered; the risk fold reads it as no verdict, never
  as sane, so staleness keeps counting from the last sane version-2 mark. Agent-accepted under
  DEC-176 on the coordinator's ruling: it adds a version that tightens what a risk input proves,
  and loosens nothing.
  - **§9.27's `breach_time_s`.** A scale rung carries 0, not at least `breach_confirm_s`: mandate spec
    §5.5 applies `scale_sizes` immediately, with no confirmation, as DEC-2301 item 1 and the risk
    fold already read it. The row now says so. No rule or vector changes.
  - **Versions.** v0.62 (DEC-2104, #1722) is on main, so this change takes v0.63, §9.30, and rule
    244, which no other section uses. It renumbers at merge if that order changes.
- **v0.62 ([DEC-2104](../project/decisions/DEC-2104.md); E9-4, claim #759):** §9.29 adds the
  control stream's `StepUpConfirmed` at schema version 1: the CLI's record that the owner confirmed,
  on the host, one step-up challenge the workspace API issued for a paper grant made in the web app
  (DEC-2103 item 7, DEC-2605 row 5). Rules 242 and 243 shape it and confine it to one confirmation
  per challenge. Only the workspace API reads it, as one input to a `cli_confirm` step-up; the
  runtime and the executor never do, and no other record changes.
  - **Versions.** v0.61 (DEC-2509, #1670) is on main, so this change takes v0.62.
- **v0.61 ([DEC-2509](../project/decisions/DEC-2509.md); E8-3's CLI follow-up, claim #1594):**
  §9.28's executor judgment is stated in full, and its vectors land as the `acknowledgments`
  section. An acknowledgment's step-up is judged when the executor processes the copy, as mandate
  spec §6.1 says, and its method by the environment. Step-up reuse is the one input judged
  beyond the account stream, from the control stream up to the copy's original. A lift
  supersedes its own trigger. Supersession is per agent. Agent-accepted under DEC-176: each
  reading only refuses an acknowledgment, which only ever lifts a limit.
  - **Versions.** v0.59 (DEC-2003, #1663) and v0.60 (DEC-2302, #1619) are on main, so this change
    takes v0.61. It adds no section or rule, and renumbers at merge if that order changes.

- **v0.60 ([DEC-2302](../project/decisions/DEC-2302.md) item 2; E21-30):** §9's account-stream
  row for `KillSwitchActivated` names its `cancels` member as the orders the switch cancelled. The
  executor's fold reads it, and a refusal of one of those cancels never returns the order to
  working (trading spec §5.7 v0.19). Documentation of a member the executor already writes; no
  rule changes.
- **v0.59 ([DEC-2003](../project/decisions/DEC-2003.md) item 4; E21-30):** §9.1 names
  `copied_from` among the members of the account stream's `KillSwitchActivated`, which is not yet
  closed. The member holds the `event_id` of the agent-stream switch an executor applied, or `null`
  for one it originated. A later closed schema keeps it. Documentation of a member the executor
  already writes; no rule changes.
- **v0.58 ([DEC-2301](../project/decisions/DEC-2301.md); E6-15):** §9.27 closes the account
  stream's seven risk-limit records, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`,
  `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted`, at
  schema version 1 before anything writes them, with rules 230 to 238. Each member is the risk-state
  fold's event or its snapshot after the input, so a replay reproduces the record. Agent-accepted
  under DEC-176: the records are new, nothing writes them yet, and every rule only refuses drafts;
  none applies to an exit, a protective order, or a kill switch.
  - **Versions.** v0.55 (DEC-2240) is on main; v0.56 and v0.57 are queued for D1 (#1589) and M7
    (claim #1491's order), so this change takes v0.58, §9.27, and rules 230 to 238, which no other
    section or open branch uses. It renumbers at merge if that order changes.

- **v0.57 ([DEC-2509](../project/decisions/DEC-2509.md); E8-3's CLI follow-up, claim #1594):**
  §9.28 closes the control stream's `OwnerAcknowledged` at schema version 1 and adds
  `OwnerAcknowledgmentRequested`, with their account-stream copies, rule 239 and stream rules 240
  and 241. An acknowledgment carries the requesting user and the independence requirement as it
  stood (as §9's catalogue row and mandate spec §6.7 already required, and DEC-279 item 7's members
  lacked), names the `RiskLimitTriggered` it acknowledges as `event`, and, where an independent
  user is required, names the request it answers. A request names a trigger, carries step-up, and
  is void once that trigger is superseded. The writer sets both users from the principal it
  authenticated. The executor judges each acknowledgment copy from its account stream, matching a
  request copy by `causation_id`, and `OwnerCommandRefused` gains the account-stream reasons
  `trigger_unknown`, `trigger_superseded`, and `request_invalid` (rule 28). Agent-accepted under DEC-176: both events were refused `unknown_schema`
  until now, nothing could append them, and every rule only refuses an acknowledgment, which only
  ever lifts a limit; none holds an exit, a protective order, or a kill switch.
  - **Versions.** v0.58 (DEC-2301) is on main and reserves v0.57 for M7; v0.56 is D1's (#1589).
    This change takes v0.57, §9.28, and rules 239 to 241, which no other section or open branch
    uses (#1491). It renumbers at merge if that order changes.

- **v0.56 ([DEC-1173](../project/decisions/DEC-1173.md); E21-33):** §9.24 states that an agent id
  is deployed once: a writer of `AgentDeployed` refuses an agent that an `AgentStopped` on the
  control stream names, whatever version it would write. §9.21 reads an `AgentStopped` as final
  whether or not the manager read the deployment it stops (a version-1 deployment, or a version-2
  one rule 201 refuses). The `agent_deployed` vectors add a version-1 deployment, its stop and a
  version-2 deployment of the same agent, which starts nothing; a stream whose only establishment
  is an account stream's copy, which rule 201 refuses; and a range anchored after a revocation that
  follows the deployment, which passes. The oracle gains four seeded bugs, each caught. Only
  deployments, which add risk, are refused, and the vectors stay version 3 (DEC-176).
  - **Versions.** v0.55 (DEC-2240), v0.57 (DEC-2509) and v0.58 (DEC-2301) are on main; this
    change takes v0.56, which they left for it, adds no rule number, and changes only §9.21's and §9.24's text and the
    `agent_deployed` section.

- **v0.55 ([DEC-2240](../project/decisions/DEC-2240.md); E12-5, E19-15):** §9.25 closes the
  desk's `OwnerQuestionSubmitted` on the control stream at schema version 1 before anything writes
  it, with rules 190 and 191, and adds the `answer` tool's six check codes to §9.23's
  `ToolResultRecorded.code` (agent harness spec §15.5), as §9.23 says this change does. The run
  that answers is E19-13's, on its own records. Agent-accepted under DEC-176: the record is new, is
  not a risk input, and a refused question is never stored.
  - **Versions.** v0.49 (DEC-2202) to v0.54 (DEC-1171) are on main; this change takes v0.55 and
    §9.25 (§9.24 is DEC-1171's), and rules 190 and 191, which no other section uses.

- **v0.54 ([DEC-1171](../project/decisions/DEC-1171.md); E21-33):** §9.24 registers `AgentDeployed`
  at schema version 2: version 1's members, then `connection_id`, the connection of the stored
  mandate document `mandate_version` names. The deployment manager (§9.21) must learn a
  deployment's account from the journal alone, since the cell has no artifact store it shares with
  the workspace services; it reads the account from that connection's own version-2
  `ConnectionEstablished` on the same control stream, as it already does for a connection kill
  switch. Version 1 stays registered and unchanged. It adds a member and two checks and loosens
  nothing (DEC-176).
  - **Rule 200, where the document is read.** The member equals the document's `connection_id`.
    The writer judges it before appending, from the document it deploys, and §9.2's mapping to
    `AgentVersionActive` refuses a mismatch as malformed, so every `ValidationContext::from_journal`
    reader refuses the control stream at that record. `append`, §11, and the manager do not judge
    it; a stored mismatch fails closed, since the agent's runtime refuses it.
  - **Rule 201, a stream rule.** The connection was established on this control stream with an
    `account_ref` (version 2), and not since revoked, before the deployment. Held as rules 66 to 68
    are: the writer checks it before appending, and §11's `connection_lifecycle_mismatch` checks it
    from the connection anchor, which already holds what it reads. A writer writes version 2
    exactly when rule 201 holds, and version 1 otherwise, as it does today: a live deployment meets
    it already (the CLI's `admitted()`, DEC-529 item 3), and the CLI's paper `agent deploy` on a
    connection the stream never recorded stays version 1, which the manager does not start. No
    deployment that is accepted today is refused.
  - **Version 1 names no account.** The manager starts nothing for a version-1 deployment: it fails
    closed rather than read the document. Every other reader treats the two versions alike.
  - **Vectors.** A new generated `agent_deployed` section; no other section changes, so the
    vectors stay version 3 and `mandate-journal`'s tests are unchanged until it registers version 2.
  - **Versions.** v0.49 (DEC-2202) to v0.53 (DEC-2207) are on main; this change takes v0.54,
    §9.24, and rules 200 and 201, lane D1's block; numbers between §9.23's rules and 200 are left
    to other lanes. Whichever change lands first keeps its version and section numbers; the other
    renumbers at merge.

- **v0.53 ([DEC-2207](../project/decisions/DEC-2207.md); E19-13, delivery slice 2 of ADR-0005):**
  §2 and §9.19 add the deployment meter stream, `dmeter:{deployment_id}`, so the deployment's
  model-spend cap (a deployment setting, the founder's clarification of DEC-997 item 3) folds across
  every workspace. It holds a copy of every workspace's `MeterReserved` and `MeterSettled`, with the
  same members, so no record schema changes; rule 154 names its `StreamOpened`, and rules 155 to 158
  bind it unchanged, a copy's settlement naming the copy of its reservation. A reservation is
  appended to it before the workspace's own stream, so a failure between them leaves an orphan
  that only over-counts; inference spec §3.3 and §3.6 say so in the same change. Agent-accepted
  under DEC-176: it only refuses more.
  - **Versions.** v0.49 (DEC-2202) to v0.52 (DEC-1142) are on main; this change takes
    v0.53 and adds no rule number. Lane EX's explainer change follows whichever lands first.

- **v0.52 ([DEC-1142](../project/decisions/DEC-1142.md); E17-8, DEC-1002 item 7):** §9.18 types
  `EvaluationRunRecorded`'s `theses_head` as `[digest]`, not `[ref]`. Each entry is the `hash`
  (§3) of the last thesis record the evaluation read on one agent stream: a journal record's hash,
  which names no stored artifact. As a `ref` it had to be listed in `artifact_refs` and stored
  under its digest for §11 check 6, which no evaluator could meet without storing record hashes
  as artifacts; as a `digest` it stays out of `artifact_refs`, and a verifier checks it against
  the journal itself. It removes a requirement no record could meet and adds no risk (DEC-176).
  - **Vectors.** The test vectors' `backtest_records` section, generated with the writer reading
    of [DEC-1141](../project/decisions/DEC-1141.md), holds a base draft of each §9.18 record, the
    stored artifacts they name, an invalid draft for every member type and rule (among them a
    `sha256:`-prefixed head and a head listed in `artifact_refs`), and valid drafts for the cases
    a rule might be misread to refuse.
- **v0.51 ([DEC-2505](../project/decisions/DEC-2505.md); E8-14, E8-10, DEC-2502 item 5):** §9.15
  closes the control stream's `NotificationAddressChanged` at schema version 1 and adds
  `NoticeAttempted` version 3, with rules 210 to 216. Both were owed: workspace API §4.11 and §5.7
  write the first ([DEC-795](../project/decisions/DEC-795.md) item 5), and
  [DEC-2502](../project/decisions/DEC-2502.md) item 5 makes one send and one record per web-push
  address, which a record that names no address cannot hold. Until now `append` refused
  `NotificationAddressChanged` as `unknown_event_type`, and the dispatcher makes no web-push attempt
  (notifications spec §4.6). Agent-accepted under DEC-176: it admits the records accepted decisions
  already require, each with the shape they name, and every reading it adds only refuses more.
  - **`NotificationAddressChanged`.** Exactly workspace API §5.7's and identity spec §12.1's
    members: `member` (the user principal's ULID), `channel` (`web_push`), `action` (`added`,
    `removed`), `address_ref`, and `step_up`. An `added`'s `address_ref` is its own `event_id`, and
    `pii_refs` is exactly `[address_ref]` (rules 210 and 211). No member can hold an endpoint, a
    key, a push service's host, or a hash of any of them (NT-2, API-11); the VAPID `key_id` of
    [DEC-2503](../project/decisions/DEC-2503.md) item 5 stays in the vault entry with them.
  - **Who writes it, and with what.** An `added`, and a `removed` with step-up, are the member's own,
    by a `user` actor who is the `member`, with step-up valid as rule 102 reads it (rules 212 and
    213). A `removed` with `null` step-up rides a deactivation: rule 214, a batch rule, holds it to a
    `MemberDeactivated` or `MemberRemoved` of the same member by the same actor in its batch, with no
    alert, and holds every request's record to its `OwnerAlertSent` (`notification_address_changed`)
    in one batch, a replacement's `removed` to the `added` it rides with, and every such alert to a
    request's record.
  - **`NoticeAttempted` version 3.** Version 2's members in their order, then `address_ref`,
    non-null exactly when `channel` is `web_push` and then listed as the event's only `pii_refs`
    entry (rule 215), and the stop `address_retired`, only `abandoned` and only on web push
    (rule 216). A terminal attempt, a mark, and `address_missing`'s one report are per address.
  - **Versions.** From v0.51 `append` refuses `NoticeAttempted` at versions 1 and 2 as
    `unknown_schema` at `payload`, as v0.40 refused version 1: a version-2 record on web push names
    no address, and one shape per fact keeps a writer from choosing the shape that says less. Both
    still replay, with `address_ref` read as `null`. No deployment holds a web-push `NoticeAttempted`
    at any version.
  - **Vectors.** None: `reference/journal` models neither record. `crates/mandate-journal/tests/notices.rs`
    pins §9.15 (DEC-720 item 7), and the tests, the registration, and the dispatcher's and the
    workspace API's writers follow in their own code changes (ES-22). The vectors stay version 3.
  - **Numbering.** Rule numbers are reserved in blocks per lane (AH 171 to 183, I2 184 to 189, EX
    190 to 199, D1 200 to 209, L3 210 to 219); this change takes rules 210 to 216 from L3's block and
    adds no section. Versions go to the change that lands first, so v0.51 here is renumbered at merge
    if another lands before it; v0.48 and rules 165 to 168 are
    [DEC-1170](../project/decisions/DEC-1170.md)'s.
- **v0.50 ([DEC-2203](../project/decisions/DEC-2203.md); E19-13):** §9.23 closes the agentic
  research run's five records at schema version 1, with rules 171 to 183, before anything writes
  them: `RunStarted`, `ToolCallRecorded`, `ToolResultRecorded`, `RunSummaryRecorded`, and `RunEnded`
  ([agent harness spec](agent-harness.md) v0.2 §6.9, [ADR-0005](../adr/0005-agentic-harness.md)).
  Each turn stays one `ModelInvocationRecorded`, unchanged, its place in the run given by its order;
  rule 183 only places it inside its run. It adds records and loosens nothing: every other record,
  rule, and reading is unchanged (DEC-176).
  - **Instruments are asset IDs.** `RunSummaryRecorded`'s `instruments_read` and `watch[].instrument_id`
    are §9.3's `asset_id`, as §9.4 types the thesis records' (v0.11), not any `id`.
  - **What `append` checks.** A run starts once and ends once, and nothing of it follows its end; an
    approval is explained once; a tool call names an `ok` research turn of its run and a tool in the
    run's menu; indexes run from 0; each call has one result, coded as its class allows; a reply over
    the run's tool calls runs none of them.
  - **No vectors**, as for §9.15, §9.16, and §9.20.
  - **Versions.** v0.48 (§9.21, rules 165 to 168) is DEC-1170's and v0.49 (§9.22, rules 184 to 187)
    is lane I2's E19-5 (DEC-2202). This change takes v0.50, §9.23, and rules 171 to 183, numbers it
    held before E19-5 landed; rules 169 and 170 stay unused. Lane EX's explainer change (rules from
    190) and lane I1's gateway change follow it, renumbered at merge to follow whatever lands first.
- **v0.49 ([DEC-2202](../project/decisions/DEC-2202.md); E19-5):** §9.22 adds the agent stream's
  invalidation verdict, `ThesisInvalidated`, at schema version 1, with rules 184 to 187. Mandate
  spec §8.6 removes an instrument at once when its thesis is invalidated, and the executor's
  `UniverseChanged` (`thesis_invalidated`) needs a journaled cause to name; no record carried one,
  so no research run could start (agent harness spec §6.2). It admits a record that was impossible
  and loosens nothing: every other record, rule, and reading is unchanged (DEC-176).
  - **Two causes.** `model`: the research call's judged invalidation verdict (§9.19 rule 151),
    which the record names by its call record and its index; and `source_revoked`: a source the
    allowlist dropped (DEC-433 item 19, Proposed for the founder), which the record names by the
    source. No writer emits `source_revoked` until the founder decides item 19.
  - **One verdict, one record, one thesis once.** Rule 186 ties a model's invalidation to a judged
    invalidation verdict of a call earlier in its batch, and every such verdict to exactly one
    record, so no judged invalidation is dropped in silence (HI-22). Rule 187, a stream rule, names
    a thesis the stream admitted and that still holds its instrument, invalidated once.
  - **The executor's copy.** §9.3's `UniverseChanged` (`thesis_invalidated`) names the record as its
    `causation_id`. The copy is on another stream, so the verifier checks it, not `append`.
  - **Vectors.** A new generated `thesis_invalidation` section holds base drafts, an invalid draft for
    every member type and rule, valid drafts for the cases a rule might be misread to refuse,
    batches for rule 186, and agent streams for rule 187, each a list of `append` batches. The
    vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first. `mandate-journal`'s registration and
    the runtime's invalidation path follow in their own code change; until the record is
    registered, no research run starts.
- **v0.48 ([DEC-1170](../project/decisions/DEC-1170.md); E21-33):** §9.21 registers the deployment
  manager's three records on the workspace control stream, closed at schema version 1:
  `ProcessStartIssued`, `ProcessStopIssued`, and `ProcessCrashLooped`, with rules 165 to 168. Until
  this version `append` refused each as `unknown_event_type`; it admits records that were
  impossible and changes no other record, rule, or reading (DEC-176).
  - **Which fact is a restart.** A `ProcessStartIssued` whose `cause` is `restart`; the crash-loop
    bound (infrastructure design §3.4) counts their `recorded_at`, never the orchestrator's count
    (DEC-994 item 4, DEC-1170 item 2).
  - **Which fact is the pause.** `ProcessCrashLooped`: after it the manager starts nothing for the
    process but a kill switch's one start, until the agent is stopped (DEC-1170 items 3 and 5).
  - **No content.** Every member is an ID, a closed enumeration, or an integer.
- **v0.47 ([DEC-1010](../project/decisions/DEC-1010.md); E21-29):** §9.20 closes the scheduler
  stream's five records at schema version 1 before the scheduler process writes them: its
  `StreamOpened`, `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`, and
  `ClockToleranceExceeded`, with rules 159 to 164. §5.4 named the measurements but not their
  members; until this version a scheduler-stream record's payload was not checked. It binds the
  scheduler stream only: the executor's account-stream copies keep §9's row (E7-4 slice 5).
  Agent-accepted under DEC-176: it closes a gap that nothing wrote into, and every reading is the
  one that adds no risk.
  - **The measurement.** Offsets are signed `decimal` seconds (§4.4 keeps integers non-negative).
    The measured `offset` is the median by magnitude of the answering servers', the larger of two;
    a measurement no server answers has none and is out of tolerance (`AGENTS.md` rule 3).
  - **What reads it.** Design §3.4's start-up check, read the stricter way: while the last
    measurement is out of tolerance, or before the first, an executor takes no new opening (its
    story, E21-30); exits, protection, the kill switch, and the ticks are never held. Design §3.4
    says so in the same change.
  - **The flagged interval.** Records are flagged from the last in-tolerance measurement before a
    `ClockToleranceExceeded` to the next in-tolerance one: the earlier start flags more, never
    fewer. Flags are computed by exports and read by no risk input.
  - **The ticks.** `ClockAdvanced` carries its tick as `clock`, a whole-second `risk_clock`: one
    tick per elapsed second, each the later of the previous plus one second and the host clock's
    second, so a host clock that steps back never freezes or reverses the risk clock.
    `TradingDayStarted` follows the first tick of each new trading date in its batch. `append` does
    not compare ticks or dates across records at this version.
  - **Versions.** v0.46, §9.19, and rules up to 158 are held by an open change (the gateway lane);
    this change takes v0.47, §9.20, and rules 159 to 164, and is renumbered at merge if that
    changes.

- **v0.46 ([DEC-998](../project/decisions/DEC-998.md); E15-8, DEC-432 items 11, 19, 20, 22):**
  §9.19 closes the agent stream's `ModelInvocationRecorded` at schema version 1 and adds the model
  gateway's meter stream, `meter:{workspace_id}` (§2), with `StreamOpened`, `MeterReserved`, and
  `MeterSettled` and rules 146 to 158. The agent-stream record was catalogued with no registered
  payload, so `append` refused it and no gateway call could be journaled before its output was used
  (inference spec INF-10). It admits records that were impossible and loosens nothing: every other
  record, rule, and reading is unchanged (DEC-176). v0.44 (#1450) and v0.45 (#1472) are other
  changes' numbers, as are rules 137 to 145.
  - **Members.** §9.9's control-stream members (DEC-432 item 11) with `purpose` `research`,
    `llm_signal`, or `fast_signal` and no draft, then the agent harness spec's two (DEC-432 item 22):
    `observations`, the run's retrieved context, and `verdicts`, one per candidate in the response,
    with its kind. It adds `reservation`, the meter record the call reserved under, and `guard_rule`.
    For a prompt the guard refused, `prompt_ref` and `request_digest` are `null`: neither the prompt
    nor the canonical request that holds it is stored (INF-8, INF-14). Rule 153 refuses a second
    record of one call.
  - **The meter stream.** One writer per workspace, fenced by epoch (DEC-432 item 20). A call that may
    reach a provider reserves its maximum cost before its first attempt and settles once on completion.
    A refusal (`meter_unavailable` among them) and a cache hit have no meter record. A reservation never
    settled, because its call's process died, stays counted at its maximum (inference spec §7.3; this
    picks one fold for backlog E15-12 minor 8). Spend folds from the stream alone. The range grammar of
    §9.13 and the anchors' stream types name it.
  - **Vectors.** A new generated `model_gateway` section holds base drafts for the four records, an
    invalid draft for every member type and rule, valid drafts for the cases a rule might be misread to
    refuse, agent and meter streams for rules 153 and 158, spend folds recomputed by an independent
    accumulator, and a seeded fuzz of reservations, settlements, crashes, and month boundaries under
    caps. The vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first. `mandate-journal`'s registration of the
    records and the meter stream follows in its own code change; until it lands, no agent journals a
    gateway call and no live call is made (DEC-998 item 8).
- **v0.45 ([DEC-1001](../project/decisions/DEC-1001.md), [DEC-1002](../project/decisions/DEC-1002.md);
  E4-5, E17-8):** §9.18 closes the workspace control stream's `BacktestRunRecorded` at schema
  version 1 and adds `EvaluationRunRecorded`, the forward-paper evaluation's record. Both name a
  report artifact (§6.3) and the digests that reproduce it; neither carries a figure inline. The
  backtest record was catalogued with no registered payload, so no writer could append one; this
  admits a record that was impossible and loosens nothing (DEC-176).
  - **The marker.** `BacktestRunRecorded`'s `marker` is always `backtest`, so no reader can take a
    backtest for a paper or live result.
  - **No vectors yet.** Registration in `mandate-journal`, with vectors, is E4-5's and E17-8's; until
    then `append` refuses both, as it refuses every catalogued type with no registered payload.
- **v0.44 ([DEC-981](../project/decisions/DEC-981.md); E20-3):** §9.17 registers the ten records of
  [control-plane design §3.8](../design/control-plane.md#38-the-closed-message-set), closed at schema
  version 1, before any message flows: `ControlPlaneEnrolled`, `LicenseApplied`,
  `LicenseStateChanged`, `ReleaseOffered`, `ReleaseInstalled`, `ReleaseWithdrawnNoticed`,
  `CatalogEntryRegistered`, `DataBundleImported`, `UsageReportSealed`, and
  `ControlPlaneMessageRefused`, with rules 137 to 142. Until this version `append` refused each as
  `unknown_event_type`; it admits records that were impossible and changes no other record, rule,
  or reading (DEC-176).
  - **Writer and stream.** Workspace control services write every one on the workspace control
    stream as a `system` actor (rule 137); `cp-agent` never appends. A deployment-wide instruction
    is journaled on each covered workspace's stream before it takes effect there (DEC-981 items 1
    and 2).
  - **No content.** No member is `text`: IDs, closed enumerations, integers, timestamps, and
    digests only (design CP-1). Signed documents are named by `digest`, not stored.
  - **Anti-rollback stays with the writer.** Each instruction's `sequence` is recorded; workspace
    services refuse a replay against the stream's highest and journal the refusal. `append` and
    §11 compare no sequences at this version (DEC-981 item 4).
- **v0.43 ([DEC-950](../project/decisions/DEC-950.md); E7-26, DEC-944 item 5):** §9.7 closes the
  agent stream's `ApprovalCanceled` at schema version 1. It was catalogued with no registered
  payload, so `append` refused every draft of it: a tightening that canceled a pending ask (a
  version applied, a mode tightened, an owner pause or Stop, a kill switch) could not be recorded.
  It admits a record that was impossible and loosens nothing: every other record, rule, and
  reading is unchanged (DEC-176). v0.42 is another change's number.
  - **Members.** Exactly the two its §9 row and mandate spec §6.4 name: `approval` (`ulid`, the
    ask's `event_id`) and `reason`, one of the five cancel reasons the runtime writes. It carries no
    content, no instrument, and no owner identity (`AGENTS.md` rule 6).
  - **No legacy `rebound`.** The §9 row reads a legacy `rebound` as a cancel for either of the first
    two reasons. No journal can hold one, because every `ApprovalCanceled` was refused until this
    version, so the closed schema refuses it (`non_canonical`); the row's reading stays for any
    record written outside this journal (DEC-950 item 2).
  - **No cause rule.** The cancel's cause is the tightening, whose record differs by reason and
    by stream, and the runtime names none today; no rule reads `causation_id` at this version
    (DEC-950 item 3).
  - **Vectors.** A new generated `approval_cancellations` section holds the base draft, an invalid
    draft for every member type, the configuration, the stream, and the version, and a valid draft
    for each other reason, a cause, a correlation, and another approval. The vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first. `mandate-journal`'s registration of
    the schema follows in its own code change.
- **v0.42 ([DEC-942](../project/decisions/DEC-942.md); E7-26, DEC-940 item 6 gap 2):** §9.6's
  `ApprovalRequested` gains version 2, which binds the execution plan the order builder produced
  for the asked order. Before it, nothing journaled that plan, so a grant released an order with
  no time in force or protection, and the executor's sink refused every one. It records more and
  refuses more. Version 1 is unchanged and still refuses the new member, and no version 1 record,
  content hash, or reference case changes (DEC-176).
  - **Version 2.** Version 1's members, with one more `content` member, `execution`, required and
    last: `{asset_class: us_equity | crypto, tif: day | gtc | ioc, protection_required: boolean,
    protection: {stop: decimal, take_profit: decimal?}?}`, closed recursively like the rest of
    §9.6. `content_hash` is still the hash of the canonical `content`, so it covers the plan, and
    a grant that repeats the hash approves exactly that plan. A writer uses version 2 exactly when
    the builder produced a plan.
  - **Rule 136**, numbered on after rule 135: in version 2, `content.execution.asset_class` equals
    the top-level `asset_class`.
  - **Vectors.** A new generated `approval_requests` section holds a base draft at each version,
    an invalid draft for every plan member, rule 136, the plan under a stale hash, the version
    split, §9.6's mirror, deadline, label, default, choices and hash rules, the configuration, the
    stream, and the version, and four valid drafts. Before this, §9.6's `ApprovalRequested` had no
    generated vectors. The vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first. `mandate-journal`'s registration of
    version 2 and rule 136, the runtime's writer and fold, and the shell's version choice follow in
    their own code change (#1381).
- **v0.41 ([DEC-944](../project/decisions/DEC-944.md); E7-26, DEC-940 item 7):** §9.7 closes the
  agent stream's `ApprovalTimedOut` at schema version 1, with rule 135. It was catalogued with no
  registered payload, so `append` refused every draft of it and an ask whose deadline passed could
  not be recorded; the approval stayed pending on the journal. It admits a record that was
  impossible and loosens nothing: every other record, rule, and reading is unchanged (DEC-176).
  - **Members.** Exactly the two its §9 row and mandate spec §6.4 name: `approval` (`ulid`, the
    ask's `event_id`) and `on_timeout` (`skip`, the safe default that applied). It carries no
    content, no instrument, and no owner identity (`AGENTS.md` rule 6). The deadline is the ask's
    `ApprovalRequested.deadline`, which the record names through `approval`; it is not repeated
    (DEC-944 item 2).
  - **Rule 135**, numbered on after rule 134: `causation_id` is non-null and equals
    `payload.approval`, so a timeout is caused by the ask it ends, as rule 50 ties a copy to its
    answer.
  - **Vectors.** A new generated `approval_timeouts` section holds the base draft, an invalid draft
    for every member type, rule 135's three clauses, the configuration, the stream, and the
    version, and two valid drafts; `approval_answers` is unchanged. The vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first. `mandate-journal`'s registration of
    the schema and rule 135, and the runtime's writer naming its ask as the cause, follow in their
    own code change.
- **v0.40 ([DEC-706](../project/decisions/DEC-706.md); E8-10, E8-9):** §9.15's `NoticeAttempted`
  gains version 2, which records the provider's verdict on a `failed` attempt. It records more and
  refuses more; it admits nothing version 1 refused, and it changes no send, mark, or
  `channel_lost` (DEC-176).
  - **Version 2.** Version 1's members in their order, and `verdict` (`retryable` \| `permanent`,
    nullable) last. `reason` alone cannot say whether a channel is tried again: `provider_error` is
    `permanent` for a relay refusal ([DEC-728](../project/decisions/DEC-728.md) item 1) and a push
    `400`, `401`, or `403` ([DEC-729](../project/decisions/DEC-729.md) item 4) and `retryable` for a
    `5xx` or an unknown status (DEC-729 items 7 and 8), and `too_large` is `permanent` for a push
    `413` (DEC-729 item 5) and `retryable` as [DEC-725](../project/decisions/DEC-725.md) item 6.3
    reads it. DEC-725 item 6.3 says the verdict decides, so a reader of the notice stream alone (the
    dispatcher's restart, notifications spec §5.1 step 4, and NT-8's journal-only oracle) needs it.
  - **Rule 134**, numbered on after rule 133: `verdict` is non-null exactly when `status` is
    `failed`, and agrees with every reason whose verdict notifications spec §5.2 to §5.4 fix
    (`permanent` for the five marking reasons, `recipient_not_permitted`, and `address_missing`;
    `retryable` for `timeout` and `rate_limited`). An absent or extra member is `schema` at it, and
    a value outside the two is `non_canonical` (§9.1's types).
  - **Version 1.** Not edited (§8); it still replays, and a stored version-1 `failed` attempt reads
    as `permanent`. From v0.40 `append` refuses version 1 as `unknown_schema` at `payload`, so a
    writer cannot leave the verdict out by writing the older shape. No deployment holds a version-1
    record: only the dispatcher's step tests have appended one, and no dispatcher process runs
    (E8-10 slice D10).
  - **Lifecycle.** §9.15 names a terminal attempt: `delivered`, `suppressed_quiet_hours`,
    `abandoned`, or `failed` with the verdict `permanent`, as notifications spec §7's diagram has
    it.
  - **Vectors.** None: `reference/journal` does not model §9.15, which
    `crates/mandate-journal/tests/notices.rs` pins (DEC-720 item 7), and the vectors stay version
    3. The tests and then the `notices.rs` registration of version 2, with the dispatcher's writer
    moved to it, follow in their own code changes (DEC-77, ES-22).
- **v0.39 ([DEC-896](../project/decisions/DEC-896.md), [DEC-789](../project/decisions/DEC-789.md); E12-3):** §9.14 and
  §11 state that the run verifies the timestamp token of an anchor a range starts from. Text only,
  following the decision; each reading only refuses more (DEC-176): a start whose token nothing
  checked before now ends `fail` or `incomplete` on it, never `pass`.
  - **The start anchor's token** (DEC-896 item 1). A range whose trusted start is an
    `AnchorComputed` runs §11's token check on that anchor, as on an in-range anchor. §9.14's "What
    a verifier reads" says so, and §11's per-range list names the trusted-start anchor among the
    anchors `tsa_token_invalid` reads. Workspace API §4.8.1 step 2.2 already lists it and is
    unchanged.
  - **An absent or unreadable token** (DEC-896 items 2 and 3). §11's "Incomplete" first bullet
    says that a start anchor's token whose artifact is absent from a store that answers, or does
    not re-hash, fails `tsa_token_invalid`, and that an artifact store that cannot be read leaves
    the check incomplete, cause `token_unverifiable`, which §9.13's `incomplete` row now names too.
    Rules 132 and 133 are unchanged. The vectors are unchanged; the token vectors follow in
    DEC-896's sequence.
- **v0.38 ([DEC-889](../project/decisions/DEC-889.md), [DEC-894](../project/decisions/DEC-894.md); E7-17, E12-3):** §11
  scopes the connection anchor to one stream, and adds the range-level `segment_rows_mismatch`.
  Each item only refuses more (DEC-176): an anchored run fails closed where it used to judge, and a
  range that passed before can now fail, never the reverse.
  - **One stream per connection anchor** (DEC-889 item 2). An anchored run judges only the anchor's
    stream and fails closed, cause `unanchored`, at the first record of another stream that the
    range rules judge, the set the unanchored run judges. A control-stream `ConnectionRevoked` of
    another stream is never judged (DEC-885 I6), and an account-stream one is (DEC-888). An empty
    prefix's anchor takes its stream from the range's first judged record. §11's "Anchored" reading
    now says "every record of the anchor's stream", and its agreement with the full chain holds for a range of that stream alone. Lane L5's callers run one stream at a time
    (DEC-890), so nothing they run changes.
  - **`segment_rows_mismatch`** (DEC-894). A hot-store rewrite consistent from the trusted start
    passes every per-event check, and `segment_manifest_mismatch` reads only the cold file. The new
    check exports the hot rows of the trusted-start segment and of each segment wholly inside the
    range in §6.2's form and compares their SHA-256 with the manifest's `file_sha256`; rows past
    `to_seq` are compared, not counted, and a segment the snapshot holds only in part fails.
    Reported for the range with `seq` null: rule 111's null list gains it, and so does §9.13's
    `failure.check`, **at version 2 only** (a reading the coordinator confirmed). Version 1's closed list is not edited (§8), so a version
    1 record naming the code is refused (`non_canonical`). Listing a new check on version 2 is
    safe because no writer appends `VerificationRun` version 2 yet (E12-3); that is why it needs
    no version 3. Workspace API §4.8.1 runs it as step 2.3a, after `segment_manifest_mismatch`, and
    its threat table's "Forged trusted start" row names it.
  - **Vectors.** A new section, `segment_rows`, holds 7 ranges over the account stream's own chain,
    cut into three segments whose cold files are its honest export: a clean control, a rewrite of
    the start segment and of a segment wholly inside the range (each consistent from the trusted
    start, so failing only this check), a rewrite of the start segment's rows past `to_seq`, a
    start segment the snapshot holds only in part, and two controls (the whole start segment
    compared, and a segment ending past `to_seq` not judged); and a valid and 2 invalid
    `VerificationRun` drafts (the code with `seq` null; refused with a `seq`, and at version 1). 6 reference mutants are
    caught, `rows.skip` (no comparison) among them. It is a section of its own, so the Rust tests
    that read `cold_records` and `verification_runs`, whose code lacks the check, keep their
    answers; the code change that reads it follows (ES-22). The vectors stay version 3.
- **v0.37 ([DEC-888](../project/decisions/DEC-888.md), [DEC-892](../project/decisions/DEC-892.md)):** §9.8's rule 68 and §11 judge a
  `ConnectionRevoked` on an account stream, and §9.16 names a replacement successor's writer. Each
  item only refuses more or states what the writer already does (DEC-176). §11 folds the hold and
  connection anchors only from a verified prefix bound to the range's start ([DEC-892](../project/decisions/DEC-892.md)).
  - **An account-stream `ConnectionRevoked`** (the coordinator's ruling, fail-closed). The record
    belongs to the control stream, and `append` already refuses it elsewhere (`wrong_stream`). A
    stored one is now refused by rule 68 at that record, on a full chain and in an anchored range,
    and an unanchored range fails closed at it, since it is a connection record on an account
    stream. A control-stream revocation is still never judged, so no revocation a writer can append
    is held, §5.6's compromised one included. This amends v0.35's "a `ConnectionRevoked` never
    fails closed" for account streams only.
  - **§9.16's writers.** Besides its four kinds, a replacement successor's own record carries the
    replacement record's `replaced_by_broker_order_id` as its `broker_order_id`, or `null`
    ([DEC-870](../project/decisions/DEC-870.md)).
  - **Wording.** v0.35's entry says DEC-885's change "admits nothing the full chain refuses"
    (#1151 minor 1), and the Status table's test vectors row lists `connection_ranges` (#1151
    minor 3).
  - **A verified prefix for every anchor** ([DEC-892](../project/decisions/DEC-892.md); workspace API
    §4.8.1's Inputs). Before the hold anchor or the connection anchor is folded, the stored prefix,
    `seq` 1 to `from_seq − 1`, passes §11's checks 1 to 6 from genesis with no gap, and is bound to
    the range's trusted start: empty when `from_seq` is 1, else its last record has `seq`
    `from_seq − 1` and its hash equals the trusted start's `prev_hash`. A prefix that fails either
    check anchors nothing, so the range fails closed at its first judged record. For the
    connection anchor this amends DEC-885 item 2's definition; it only tightens, since an anchor
    it no longer yields makes the range fail closed.
  - **Vectors.** A new section, `connection_revocations` (6 cases: an account-stream revocation
    refused under rule 68 on a bound and an unbound full chain and in an anchored range, failing
    closed in an unanchored range, and two controls with the revocation on the control stream; 1
    mutant, `stream.68.revoked`). `connections` gains three sequences that enter `suspended` twice
    on one account stream: a rotation whose check fell in the first suspension, or the first
    suspension's own rotation, never clears the second, and a check and rotation after the second
    entry do (#1174); the stream mutant `stream.68.latest`, which keeps the first entry, is caught
    by them. The vectors stay version 3. The fold's tests read `connections`, which they already
    answer; the code change that reads `connection_revocations` follows (ES-22).
- **v0.36 ([DEC-788](../project/decisions/DEC-788.md), [DEC-789](../project/decisions/DEC-789.md)
  the founder, [DEC-786](../project/decisions/DEC-786.md), [DEC-787](../project/decisions/DEC-787.md)):**
  §9.13's `VerificationRun` gains version 2, so `GET /verifications/{id}` (workspace API §4.8.1)
  can rebuild a run from its record, and §11 gains an **incomplete** outcome for a check that
  cannot yet be proven. Each item only refuses more or records more; none admits a range that
  failed (DEC-176), except DEC-789's new outcome, which is the founder's.
  - **Version 2.** Each range adds `start` (`{kind, manifest_hash, anchor_event_id}`, the
    trusted start's record), `checked` (the events walked), and `incomplete` (the check that could
    not finish and its cause, `{check: tsa_token_invalid, cause}`, or `null`); `result` adds
    `incomplete`. Version 1 is not
    edited (§8): a closed schema gains members only at a new version, as `OrderStateChanged`'s
    did at v0.34, even though no writer appended version 1. It stays registered and replays;
    every writer appends version 2.
  - **Rules 132 and 133**, numbered on after rule 131 and renumbering none. Rule 132: the start
    names exactly its own record; genesis enters at seq 1 and an anchor at seq 2 or later;
    `checked` never exceeds the range and is the whole range unless the range failed; a failed
    range records no incomplete check; and while the token cannot be proven, an anchor start's
    range never passes. Rule 133: `fail` when a range failed, else `incomplete` when one is
    incomplete, else `pass`. Rule 112 is version 1's; rules 107, 110, and 111 judge both.
  - **The verification's id** is its `VerificationRun`'s `event_id`, as an export's is its
    `ExportCreated`'s (DEC-788 item 2).
  - **§11 "Incomplete"** (DEC-789, option (iii), the founder 2026-10-09). `verify_tsa` never answers
    `Ok` (DEC-265 item 1), so a token with the anchor's imprint, and an anchor the range checks whose
    token is `null`, leave the token check incomplete: never `pass`, never a false
    `tsa_token_invalid`, and no SEV-1. The CLI's `tsa_verification_incomplete` (DEC-490 item 6) is
    the same outcome under its own word. "On failure" now says it means a `fail` result.
  - **Restore drills** ([DEC-789](../project/decisions/DEC-789.md) item 9, the founder 2026-10-09):
    a drill passes on a token-only `incomplete` (every event walked, nothing failed, and every
    incomplete range's `cause` `token_unverifiable`); `anchor_unstamped`, any other incomplete
    check, or any failure is still an integrity incident (the coordinator's narrower reading under
    DEC-176, the founder informed). So each range's `incomplete` is `{check, cause}`, its `cause`
    `token_unverifiable` or `anchor_unstamped`, the latter recorded when a range meets both
    (DEC-789 item 7). Only `result: fail` raises the SEV-1 alert; `incomplete` is an
    informational row of infrastructure design §8.2 that pages no one.
  - **§9.14:** a manifest start is one the cold store vouches for; one only the hot store's record
    vouches for is not a trusted start (DEC-787 item 5).
  - **Vectors.** A new generated `verification_runs` section holds version 2's base drafts (passed,
    failed, incomplete), an invalid draft for every new member (`incomplete.cause` included) and
    every clause of rules 132 and 133, and valid drafts for the readings a rule might be misread to refuse; its oracles recompute
    each result by precedence and each count and start from the range. `records_access`, version
    1's, is unchanged. The vectors stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first: `mandate-journal` registers version 1
    only, and no Rust test reads `verification_runs` yet, so `main` stays green. The tests PR
    and the `records.rs` implementation follow and register version 2.
- **v0.35 ([DEC-885](../project/decisions/DEC-885.md)):** §11's `connection_lifecycle_mismatch`
  verifies a range from its **connection anchor**, the state the full-chain run's fold of stream
  rules 66, 67, 68, and 131 holds before the range's trusted start, and fails closed without one,
  as `held_mismatch` does ([DEC-782](../project/decisions/DEC-782.md) item 4). A range's fold no
  longer starts empty, which reported a false SEV-1 on a valid rotation, `reauthorize` or
  `reconnect` refusal, re-establishment, daily check, or state change that followed an
  establishment or binding before the range. It admits nothing the full chain refuses (DEC-176).
  - **The anchor.** Derived only by folding the stored chain from `seq` 1, never from a read
    model; §11 lists what it holds on each stream type. A chain that breaks a rule before the range
    yields none.
  - **Anchored**, a range judges every record as the full chain does, rule 131's closing
    existence included, which [DEC-699](../project/decisions/DEC-699.md) had kept to the
    full-chain run only because an empty fold could not see a request before the range; §9.8's
    rule 131 ranges clause says so.
  - **Unanchored**, a range fails closed at its first record rule 66, 67, 68, or 131 judges,
    reported as `connection_lifecycle_mismatch` with cause `unanchored`; rule 111's codes are
    unchanged, and a `ConnectionRevoked` never fails closed. `connection_cause_mismatch` stays
    full-chain only.
  - **Vectors.** A new section, `connection_ranges` (22 cases, 5 range mutants); the range vector
    `range_after_an_unseen_request` gains the chain before it and keeps its answer. The code
    change that reads them follows (ES-22).
- **v0.34 ([DEC-869](../project/decisions/DEC-869.md)):** §9.16 journals the broker's own id for
  an order, so a connector that cancels by it (the Robinhood connector, connections spec §6.2)
  rebuilds its `client_order_id` → order id map from the account stream at start and never holds
  it only in memory ([DEC-860](../project/decisions/DEC-860.md) item 7). It only adds (DEC-176).
  - **`OrderStateChanged` version 2**: version 1's members in their order, and `broker_order_id`
    (`text?`) last. Version 1 is not edited (§8), stays registered, and keeps replaying; a stream
    written before v0.34 holds no order id, and an order with none is cancelled by no guess: the
    connector refuses the cancel `NotSent` (`no_order_id`) and the owner is alerted.
  - **Who writes it.** Non-null exactly on a record written from a broker order record that
    carries the order's broker id (a place answer, an order query, a cancel answer that is the
    order, a list-and-match or reconciliation adoption); null on every other record.
  - **How it is read.** Only by the connector's rebuild; the executor's fold and every risk input
    are unchanged. Two different non-null ids for one `client_order_id` leave it with no
    recoverable id.
  - **Vectors.** None: `reference/journal` does not model `OrderStateChanged`, and the vectors stay
    version 3. `mandate-journal`'s registration of version 2 and the executor's writer follow in
    their own code change, which reads this one (ES-22).
- **v0.33 ([DEC-859](../project/decisions/DEC-859.md), option (a), the founder 2026-10-09):** §9.5
  rules 41 and 44 admit the `ProtectionChanged` records the executor's fold reads and the closed
  schema refused (DEC-446). This loosens the closed schema toward the code; the founder decided
  it (DEC-79, DEC-176). Every other clause of rules 41 to 44 stays.
  - **Rule 41.** `orders` may be empty on `passive_start` and `unprotected_start`, and still names
    at least one order on `placed` and `cancelled`. `awaiting` may be empty on `interval_limit`
    and `unprotected_end`, which the text allowed but the code and the reference read as
    non-empty; it stays empty on every other action.
  - **Rule 44.** On the two starts, where `replacing` is true, `entry` and `agent_id` may be
    non-null, together, with `intent_id` null: a re-placement before expiry. With `intent_id`
    non-null the three still go together, and with `replacing` false or null `entry` and
    `agent_id` still need an intent.
  - **Replay unchanged.** The fold already reads these shapes: an interval ends on an
    `unprotected_end` awaiting nothing and not uncovered, and a re-placement is read from
    `replacing`, `entry`, and `agent_id`. No start reads `orders`. Every record valid under v0.32
    stays valid.
  - **Vectors.** A new generated `protection_shapes` section holds 11 valid drafts, one or more
    for each row of DEC-859 item 1's table, and 14 near misses that stay invalid: `orders` empty
    on `placed` or `cancelled`, `orders` on an end, `awaiting` on a start or a `watchdog`, `entry`
    without `intent_id` when `replacing` is false or null, a re-placement naming only one of
    `entry` and `agent_id`, and the members an end still refuses. It has 5 validator, 3
    tightening (each puts a removed clause back), and 4 vector mutants, as `generate.py` prints.
    It is a section of its own because `mandate-journal`'s `protection_changed_rules` still reads
    v0.32 and its tests count the `account_stream` cases: the code change follows this one
    (ES-22) and reads it. `account_stream` is unchanged. The vectors stay version 3.
- **v0.32 ([DEC-694](../project/decisions/DEC-694.md) item 4, [DEC-699](../project/decisions/DEC-699.md)):**
  §9.8 journals the pending connection that connections spec §5.2 step 1 and §9.1 create at a
  connect's start. Each change only tightens (DEC-176).
  - **`ConnectionRequested`**, a new control-stream record at schema version 1: the
    `connection_id` and `account_ref` the connection manager assigns at the start, the broker, the
    environment, the user, and the step-up. It carries no credential, code, verifier, account
    number, fingerprint, or vault path. It is the event that commits with the step-up challenge
    marked used (identity spec §7.2 step 5), which the start did not have.
  - **Stream rule 131**, forward-only, as §11's `held_mismatch` is: a control stream is bound
    from its first request on, so every stream written under v0.20 to v0.31, which holds none,
    verifies as before, and once a stream holds a request no later connect may skip one. A request
    names a connection with no open request and no establishment on its control stream, and an
    `account_ref` never named there before. A connect's version-2 `ConnectionEstablished`, and a
    `ConnectionRefused` with occasion `connect`, each close that connection's open request, once,
    and repeat its broker, environment, user, and step-up, and the establishment its
    `account_ref`. A request stays open while its connect runs; §11 reports no open request. That
    a closing record has a request is checked in the full-chain run only, since a range's fold
    starts empty and would not see a request before its trusted start.
  - **Revoke while `connecting`.** A `ConnectionRevoked` is never refused for it, and closes the
    open request. Nothing is established or refused from that request afterwards: rule 131
    reports either, and a version-1 connect after the stream's first request breaks it too.
    Rule 67 reads such an id as never established, so the teardown's
    `ConnectionRefused` is judged by rule 131 alone, and a `reconnect` of it breaks rule 67.
    Connections spec §9.1 gains the revoke exit from `connecting`.
  - **No earlier outcome changes.** Every rule-131 clause, and rule 67's reading of a revocation
    while `connecting`, needs a request on the stream, which no stream written before v0.32 holds.
    The `connections` section is unchanged but its `spec` line.
  - **Sentences reconciled.** "`connecting` leaves no record but a `ConnectionRefused`" and
    "nothing else is kept of a refused connect" now name the request too, as connections spec §9.1
    does.
  - **Vectors.** A new generated `connection_requests` section holds the record's drafts (23
    invalid, 1 valid) and 28 rule-131 sequences, one of them a range (`scope: range`), with 14
    validator, 17 stream, and 6 vector mutants, as `generate.py` prints. It is a section of its own
    because the E7-17 fold (`crates/mandate-journal/src/connection_fold.rs`) does not check rule
    131 yet: its code change follows this one (ES-22) and reads it. The vectors stay version 3.
- **v0.31 ([DEC-659](../project/decisions/DEC-659.md)):** §9.12 gains **Order**, which states a
  reading DEC-659 already took under DEC-176 (it only tightens). The identity crate implements
  `check_order`, and E9-7's writer slice implements `write_membership` (DEC-646).
  - **The writer.** The one writer of the seven `Member*` types on `ctl:{workspace_id}`
    ([DEC-646](../project/decisions/DEC-646.md)) checks order inside the serialized append, against
    `last`, the stream's last membership record: an `event_time` before `last`'s, or a `seq` not
    above it, is refused, and an equal instant passes. The first record passes; a refused record is
    never clamped and commits nothing, and the caller retries; a moved head re-runs the check.
  - **The fold.** [DEC-657](../project/decisions/DEC-657.md) item 4, which the text did not state:
    a membership record out of order makes the fold unreadable from its `event_time`, so
    `workspace_users` reads 1, as for a record that does not fit its state. It stays the backstop.
  - **Vectors.** None changes: every `membership_fold` history is already in `seq` and
    `event_time` order. The vectors stay as they are.

- **v0.30 ([DEC-696](../project/decisions/DEC-696.md)):** the reference model of §9.8's stream rules
  and §11's connection checks takes three readings the text already states and the E7-17 fold
  (`crates/mandate-journal/src/connection_fold.rs`) already pins; one sentence of §11 says the
  third aloud (DEC-176).
  - **Rule 66, version 1.** A version-1 first establishment is never re-established, not even by
    another version 1 whose absent `account_ref` matches it. The reference admitted that one.
  - **Each stream on its own.** Rules 66 to 68 hold on their own stream, so the reference folds each
    control and account stream apart: a revocation on another workspace's control stream revokes
    nothing here, and one account stream's `reauthorize` check never admits another's rotation.
    The reference folded every account stream as one, so it also refused two connections' streams
    interleaved, which the text admits.
  - **§11, `reconnect` after either version.** A version-2 establishment of an id already
    established on the same control stream, at version 1 or 2, rests on a `reconnect` check; and a
    rotation rests on an establishment on its own control stream. The reference counted only
    version-2 establishments and keyed them across control streams; §11's sentence now says both.
  - **Vectors.** The `connections` section gains four `sequences` (two rule-66 refusals, the
    interleaved streams, and the other stream's rotation, refused) and four `chains` (a `reconnect`
    after version 1, valid; a `connect` there, refused; and two rotations on another control
    stream, refused), with two stream and two chain mutants. Every earlier case keeps its answer.
    The vectors stay version 3.
- **v0.29 ([DEC-774](../project/decisions/DEC-774.md)):** two readings that only tighten
  (DEC-176), one of the text alone, and a carve-out that keeps the freeze off risk reduction.
  - **§9.13's codes.** `VerificationRun`'s `failure.check` listed every §11 code except
    `held_mismatch` (v0.23), `connection_lifecycle_mismatch`, and `connection_cause_mismatch`
    (v0.20), so a run could not record those failures: the reference validator refused them as
    `non_canonical`. The list gains all three, and rule 111 classes each as reported at an event,
    since §11 reports each at the record that breaks it.
  - **An operator read's cause.** Rule 108 requires a `platform_operator`'s `RecordsAccessed` to
    name a `causation_id`, but `append` cannot see what it names. §11 gains the control stream's
    check `break_glass_cause_mismatch`, reported at the read: the cause must be an earlier
    `PlatformOperatorAction` on the same control stream. That record stays open (DEC-261 item 9,
    Proposed), and `append` refuses it as `unknown_schema`, so until item 9 is decided every
    operator read that `append` admits fails this check. §9.13's list and rule 111 admit the code.
  - **§11's headings.** §11's per-range checks are now labelled by where each is reported: for the
    range as a whole (`anchor_root_mismatch`, `tsa_token_invalid`, `segment_manifest_mismatch`,
    `segment_gap`) or at an event (`anchor_head_mismatch` and every check §11 lists by stream), as
    rule 111 and the reference validator already read them. Rule 111's behaviour does not change.
  - **The freeze never holds risk reduction.** §11's control-stream freeze, which an operator read
    can now reach, says what the account- and agent-stream case already says of the kill switch:
    risk exits, protective orders, owner exits, and the kill switch at any scope still work
    (`AGENTS.md` rule 13), citing workspace API API-7, which already keeps its risk-reducing
    operations recorded while the control stream is frozen. Nothing else the freeze covers changes.
  - **Vectors.** The `records_access` section gains a valid run failing on each of the four codes
    with its `seq`, an invalid run naming none for each, and `range_checks`, control-stream chains
    the reference verifier and an oracle of its own judge, the first of them failing the operator
    read `read_by_an_operator` (still valid at `append`). The vectors stay version 3.
- **v0.28 ([DEC-720](../project/decisions/DEC-720.md), [DEC-795](../project/decisions/DEC-795.md) items 5 to 7):**
  §9.15 closes the records of v0.12's notice stream at schema version 1, with rules 119 to 130:
  the subject stream owner's `OwnerAlertSent` and the dispatcher's notice-stream `StreamOpened`,
  `NoticeIssued`, and `NoticeAttempted` ([notifications spec §5.5](notifications.md#55-records)).
  Three types join §9.1's: `notice_id` (32 lowercase hex digits, never an event id), `stream_id`
  (§2's form), and `opaque` (a provider's message id, with no space). The kinds are notifications
  spec §3.2's 33, each with its class, and the attempt reasons are §5.2's closed enum with the
  dispatcher's `not_pending` and `retry_window_ended`; `notification_address_changed` and
  `address_missing` are DEC-795's (items 6 and 7). None of the four records holds free text. Three
  readings only refuse more (DEC-720 item 8, DEC-176): `OwnerAlertSent` is on the agent, account,
  or control stream, not "any stream type", since no kind has a scheduler subject; a `NoticeIssued`'s
  cause is an `OwnerAlertSent`, an `ApprovalRequested`, or for `channel_lost` a `NoticeAttempted`,
  never an owner command, which its alert's `owner_command` names; and notifications spec §3.4 says
  so for a user's kill switch. §9's control-stream table gains the `NotificationAddressChanged` row
  the workspace API's push-address routes write (DEC-795 item 5); its payload schema closes with
  E8-14. The vectors are unchanged: `crates/mandate-journal/tests/notices.rs` pins §9.15 (DEC-720
  item 7), and its pending tests list DEC-720's 32 kinds and 12 reasons, which DEC-795's two
  additions extend without a test refusing either.
  - **Order of the changes (ES-22).** The tests PR merged before this spec text; E8-9's
    implementation PR registers the four schemas and deletes only the tests' `#[ignore]` lines.
- **v0.27 ([DEC-783](../project/decisions/DEC-783.md)):** §9.14 closes the control stream's
  `AnchorComputed` and `SegmentExported` at schema version 1, with rules 113 to 118, from their §9
  rows, §6.2 and §10, using the shapes the code already reads: the anchor file of DEC-115 item 6
  and DEC-263's six manifest fields with the manifest hash. A verifier can therefore take a trusted
  start from either record in the journal itself. §11 gains the control stream's per-range check
  `anchor_self_mismatch`: an anchor's leaf for its own control stream names the event just before
  it, which an append rule cannot check because the journal assigns the `seq`. §9.13's
  `VerificationRun` admits the new code. The vectors gain a generated `cold_records` section and
  stay version 3.
- **v0.26 ([DEC-780](../project/decisions/DEC-780.md)):** §9.13 closes the control stream's
  `RecordsAccessed`, `ExportCreated`, and `VerificationRun` at schema version 1, with rules 107 to
  112, from the members their §9 rows list (accessor and scope; export manifest; scope and result),
  and `RecordsAccessed` adds the opaque resources read and an optional stored result, so the audit
  routes and the workspace API's other journaled reads write one record. v0.22's `client` actor
  records its own reads there, in §3's one shape (rules 81 to 83), with its own `id` as `accessor`
  (rule 108), and rule 83's note that the payload was not closed now points here. Each names what
  it covers as stream ranges of the writer's own workspace, bounded by event hashes, and nothing
  else: no instrument, order, position, or mandate content. Two types join §9.1's: `stream_id`
  (§2's form) and `digest` (64 lowercase hex: an event's hash, or the canonical export's verifier
  digest, which is not a `ref` and so is not listed in `artifact_refs`). Rule 107 refuses a range on
  another workspace's stream. The examination bundle (§12), which may carry resolved identities, is
  not an `ExportCreated` form at this version, and an export is named by its verifier digest
  (DEC-265 item 3), so no second manifest shape is defined. §7 now says a read or export is
  journaled before it is served, as API-16 already does for exports. The vectors gain a generated
  `records_access` section and stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first; `mandate-journal` registers the three
    schemas in E12-3's tests PR and implementation PR, so no Rust test reads the new section yet.
- **v0.25 ([DEC-437](../project/decisions/DEC-437.md) item 9, [DEC-648](../project/decisions/DEC-648.md)):**
  §9's control-stream table catalogues the identity spec's §12.1 records. §9.12 closes the seven
  membership records at schema version 1, with rules 96 to 106, and states the fold that identity
  spec §5.3's `workspace_users` reads. It adds `MemberInvitationRevoked`, which §12.1 lacked for its
  §5.1 `invited` to `revoked` transition, the accepted `invitation` on `MemberActivated`, and the
  cool-off end on `MemberReactivated`; each journals a state change identity spec §5 already defines.
  `MemberInvited` also carries `invited_at`, each grant the effective `independent_approval_required`,
  `MemberReactivated` the roles it restores, and each membership record, until the envelope gains
  identity spec §12.2's session field, a payload `session_ref`. A record's own instant is its
  envelope's `event_time`.
  The credential, session, service-account, host-CLI, and break-glass records stay open until
  their own change; the client records are §9.10's (v0.22, [DEC-671](../project/decisions/DEC-671.md)). The vectors gain a generated, additive `membership` section, so they stay
  version 3. A generated `membership_fold` section holds membership histories with their expected
  states and counts, reproduced by a reference fold.
- **v0.24 ([DEC-674](../project/decisions/DEC-674.md), [DEC-676](../project/decisions/DEC-676.md)):** §9.8 states that a later check (occasion
  `executor_start` or `daily`) whose `contract` result failed with `tools_missing` moves the
  connection to `suspended`, journaled as `ConnectionStateChanged` with reason `check_failed`, as
  [connections spec §8.1](connections.md#81-at-connect-at-every-executor-start-and-daily) check 1
  says for an allowlisted tool that is absent; `contract_drift` with no other failure degrades. §9.8's
  reasons table also reads a later check that finds no pin as `contract_drift`
  ([DEC-676](../project/decisions/DEC-676.md) item 2). No rule changes: rule 59 already admits `check_failed` into `suspended`. The vectors gain the valid drafts
  `checked_daily_with_tools_missing` and `suspended_on_a_failed_check` and the sequence
  `mcp_later_tools_missing_suspends`, and stay version 3.
- **v0.23 ([DEC-672](../project/decisions/DEC-672.md)):** §9.11 closes the hold on new openings
  ([DEC-191](../project/04-decision-log.md#decisions), [workspace API spec §4.2](workspace-api.md#42-deployments-and-the-agent-lifecycle-deployment-manager)).
  `OwnerCommandIssued` is closed for its two new commands, `hold_openings` and `lift_hold`, and only
  for them. The agent runtime's copies gain the hold at schema version 2: `AgentModeChanged` adds
  `held` and the reasons `owner_hold` and `owner_lift_hold`, and `OwnerCommandRefused` adds the
  command `lift_hold`. Rules 90 to 95: a client may hold and only a user lifts, a hold carries no
  step-up, and the mode a copy leaves is never looser than the owner's pause or hold, so a resume
  never clears a hold and a lift never clears a pause or a latched limit. Rule 83 admits a client's
  `OwnerCommandIssued`, which rule 90 confines to `hold_openings`. Version 1 of both agent-stream
  records stays registered and unchanged. The vectors gain a generated `hold` section and stay
  version 3.
- **v0.22 ([DEC-671](../project/decisions/DEC-671.md)):** §3's closed `actor.kind` set gains
  `client`, an owner-connected agent ([workspace API spec §3.3](workspace-api.md#33-authentication-and-sessions)
  item 4, [identity spec §12.2](identity.md)), whose actor alone carries `on_behalf_of`, the user it
  acts for. Every other actor keeps exactly its four members, so no recorded event changes. Rules 81
  to 83 shape the client actor and confine it to the control stream's `MandateDraftSaved`,
  `OwnerRequestSubmitted`, and `RecordsAccessed`. §9.9's rules 70 and 79 already had client branches,
  which now admit. §9.10 closes `ConnectionRevoked` version 2, with the reason `owner` or
  `compromised` and its step-up (workspace API §5.6), and `ClientConnected` and `ClientRevoked`
  (identity spec §12.1), with rules 84 to 89; `ClientRevoked` records why with a closed `reason`.
  Version 1 of `ConnectionRevoked` stays registered and unchanged. The vectors gain a generated
  `client_actor` section and stay version 3.
- **v0.21 ([DEC-670](../project/decisions/DEC-670.md)):** §9.9 closes four control-stream records
  the [workspace API](workspace-api.md) commits (its §4.1, §4.6, §5.1; DEC-436 item 14), with rules
  69 to 80. `MandateDraftSaved` records one explicit draft save, its draft stored as an artifact.
  `ModelInvocationRecorded` may now be written on the control stream, for the compiler alone, closed
  there with [DEC-432](../project/decisions/DEC-432.md) item 11's members; this answers the
  [inference spec's §13 question 1](inference.md#13-open-questions). On the agent stream it stays
  open, for E15-8. `MandateConfirmed` gains `agent_id` and `base_version` at schema version 2, so a
  confirmation names the agent it is for and the version it replaces. `OwnerRequestSubmitted`
  records an owner's request. These are the first rules that read `actor.kind`: only a user
  confirms, the compiler's record is the services', and `requested_by` follows the actor, never
  the body. `MandateConfirmed` version 1 stays registered and unchanged. No other record changes.
  The vectors gain a generated `workspace_api` section and stay version 3.
- **v0.20 ([DEC-800](../project/decisions/DEC-800.md)):** §9.8 journals a connection's history
  (connections spec CN-10, backlog E7-17). `ConnectionEstablished` gains `schema_version` 2, its
  version-1 members then `account_ref`, the connecting `user`, `step_up`, and the owner's
  `margin_attestation` for a live connection; `account_ref` binds the account stream to its
  connection (DEC-261 item 10). New records: `ConnectionRefused` (a refused check or a teardown)
  and `ConnectionCredentialRotated` on the control stream, and `ConnectionChecked`,
  `ConnectionStateChanged`, `ConnectionCredentialRefreshed`, and the executor's copies of the
  establishment and each rotation on the account stream, with consistency rules 54 to 65, of which
  61 and 63 are copy rules. The connecting executor journals its permission checks on the account
  stream, and `ConnectionEstablished` names them as its cause. Stream rules 66 to 68 allow a second
  `ConnectionEstablished` for a `connection_id` only after its `ConnectionRevoked`, with the same
  broker, environment, and `account_ref`; keep a rotation's scopes the same or narrower; keep one
  `account_ref` and one account stream to one connection; keep the account stream `connecting`
  until it holds its establishment's copy; and let the owner's acknowledgment return a connection
  to `active` only after its cause cleared, a suspension only on a credential the control services
  accepted after the suspension. The connection's owner checks them before appending, §11's new
  `connection_lifecycle_mismatch` checks every range, and the full-chain run's
  `connection_cause_mismatch` follows each cause across the two streams. No rule refuses a
  `ConnectionRevoked`. The vectors gain a generated, additive `connections` section and stay
  version 3.
- **v0.19 ([DEC-531](../project/decisions/DEC-531.md) item 4, [DEC-630](../project/decisions/DEC-630.md);
  [DEC-529](../project/decisions/DEC-529.md)):** `ConfigSnapshotRegistered` version 3 admits the
  kind `broker_profile`: a connector's capability profile (trading spec §5.2), registered under
  its content hash so replay and audit see which broker rules applied. §9 states its stored
  object as DEC-630 item 6 fixes it, and rule 21b checks it at append as rule 21a checks the
  version-2 kinds. Versions 1 and 2 keep their closed vocabularies and refuse the new kind. No
  record names a `broker_profile` in `config_refs` yet. The vectors gain a generated, additive
  `broker_profile` section, so the `production_config_refs` cases and their counts are unchanged,
  and they stay version 3.
- **v0.18 ([DEC-536](../project/decisions/DEC-536.md)):** `DecisionMade`'s `decided_by` may be
  `policy_overlay`, for a decision the effective policy changed at
  [mandate spec §6.2 step 5c](mandate.md#62-evaluation): an `auto` it narrowed to `ask` because the
  effective `auto_allowed` is false (§4.3), or an opening or increase it denied because the
  confirmed version is nonconforming ([DEC-534](../project/decisions/DEC-534.md) item 2). It is
  the same label the approval content's `trigger.decided_by` carries (mandate spec §6.4, where its
  `trigger.rule` is `null`). Rule 7 refuses it on an `auto`, and rule 5 already refuses it on a
  decision that is not an opening or an increase. The vectors gain a generated, additive
  `policy_overlay` section, so the `agent_stream` cases and their counts are unchanged, and they
  stay version 3. Version 1 and version 2 of `DecisionMade` keep their payload: only the label's
  closed set grows.
- **v0.17 ([DEC-533](../project/decisions/DEC-533.md)):** §9.7 closes the control stream's
  `ApprovalResponseSubmitted` and the agent stream's `ApprovalResponded` and
  `ApprovalRevalidated` at schema version 1, with rules 46 to 53, from the members their §9 rows
  and mandate spec §6.4 list. Their times are integer risk-clock seconds, as §9.6's
  `ApprovalRequested.deadline` and every writer of them are. That differs from the step-up
  evidence of §9.2's and §9.3's closed records, whose `authenticated_at` is a §4.7 timestamp;
  unifying the two is a later version (backlog). The vectors gain a generated `approval_answers`
  section and stay version 3.
- **v0.16 ([DEC-484](../project/decisions/DEC-484.md)):** `config_refs` gains `policy_set` and
  `model_registry`. Version 2 of `DecisionMade` requires both beside `mandate_version`; version 2
  of `ModelOutputRecorded` requires `model_registry` beside `mandate_version`; both keep version
  1's payload unchanged. `ConfigSnapshotRegistered` version 2 admits the two new kinds. These
  hashes bind the independently mutable policy and registry snapshots that admitted the
  production decision and model output. Existing version-1 records are unchanged.
- **v0.15 ([DEC-460](../project/decisions/DEC-460.md)):** §9.6 closes
  `ApprovalRequested`, including its nested canonical content, `ApprovalDelivered`,
  `BrokerPositionObserved`, `AgentModeApplied`, and `CompensatingEvent` at schema version 1.
- **v0.14 ([DEC-459](../project/decisions/DEC-459.md)):** `OrderStateChanged` version 1 is a
  closed record whose nullable evidence and boolean lifecycle markers are always present. This
  closes new appends without changing the executor fold's tolerance of historical sparse records.
- **v0.13 ([DEC-446](../project/decisions/DEC-446.md), [DEC-447](../project/decisions/DEC-447.md)):**
  §9.5 closes the account stream's executor records ([DEC-360](../project/decisions/DEC-360.md),
  option (c)). `IntentReceived`, `GateDecided`, and `OrderSubmitted` gain `risk_clock` at
  `schema_version` 2, each its version 1's members with `risk_clock` last; version 1 is never
  edited (§8) and stays registered. `OrderSubmitted`'s executor-only members move to a companion
  record, `OrderRequestRecorded` (version 1), journaled immediately before its order in one
  `append` batch and named by its `causation_id` (rule 45). `ProtectionChanged` is closed with
  the members the fold reads (rules 39 to 44), its action vocabulary gaining `intended` for the
  receipt-time protective prices, which leave `IntentReceived`, never a member there at either
  version. `StreamOpened` and `OwnerCommandRefused` carry no `risk_clock` (DEC-447). The vectors
  gain a generated `account_stream` section and stay version 3.
  - **Why:** §9.2 already says the executor writes `risk_clock` on every account-stream event,
    and the fold requires it (DEC-302 item 1); the three records were the only ones that sentence
    and the version-3 form left without it. A schema once closed is never edited in place, so the
    member joins at a new version, which no conforming version-1 record is refused by. The rest
    keeps `IntentReceived` the exact copy of the proposal §9 and rule 10 make it, and
    `OrderSubmitted` the version-3 request, instead of widening either toward the code (DEC-360
    item 2, the founder's ruling). The fold's reads of legacy records stay as tolerant as they
    were, so a recorded stream stays replayable (§8); `append` is what tightens.
  - **Order of the changes (ES-22).** Spec and vectors first, as DEC-174 item 4 and the
    coordinator's comment on #482 sequence §9.x work; the `mandate-journal` registration and the
    fold's reads follow in the tests PR and the implementation PR.
- **v0.12 ([DEC-438](../project/decisions/DEC-438.md) items 5 and 28):** the notice stream. A new
  stream type, `ntf:{workspace_id}`, whose single writer is the workspace's notification dispatcher
  ([notifications spec §5.1](notifications.md#51-the-dispatcher)), holds `NoticeIssued` and
  `NoticeAttempted`. `OwnerAlertSent` becomes the subject stream owner's record that an alert was
  raised, written in that stream and the subject's batch, with the alert's kind; its delivery
  members move to `NoticeAttempted`. Nothing writes `OwnerAlertSent` today, so no record changes
  meaning. No payload schema is closed here; E8-9's tests PR closes them.
- **v0.11 ([DEC-413](../project/decisions/DEC-413.md) item 7):** §9.4 types `instrument_id` as §9.3's `asset_id`, not any `id`. It only refuses more (DEC-176), and adds no member.
  - **Why:** v0.9 typed it `id` on DEC-403's reading, which v0.10 tightened for `UniverseChanged.instrument`. A thesis admitted on an instrument that is not an asset ID would reach the executor's `UniverseChanged`, which v0.10 refuses, so the admission would be journaled with no universe change to follow it. No conforming writer emits anything else: the platform resolves the instrument before it writes the entry.
  - **The vectors gain four `research` drafts**, as §9.3's: a ticker (`BTCUSD`, a valid `id`), an asset ID with a trailing newline, and one in capitals, each refused as `non_canonical` at `payload.instrument_id`, and a number, refused as `schema`. The reference validator reuses §9.3's `asset_id` type, and its four seeded bugs (`types.asset_id`, `types.asset_id_case`, `types.asset_id_ident`, `types.asset_id_trailing_newline`) are each caught in the `research` section too.
  - **Order of the changes (ES-22).** Unlike v0.10's §9.3 drafts, these can come first: `mandate-journal` does not register §9.4 yet, and every Rust test that reads the `research` section is pending until that registration (DEC-77).
- **v0.10 ([DEC-404](../project/decisions/DEC-404.md) item 9):** §9.3 tightens two things; neither adds a member, and both only refuse more (DEC-176).
  - **`UniverseChanged`'s `instrument` is a new `asset_id` type**: mandate spec §3's lowercase uuid form, not any `id`. An `id` that is not an asset ID appended before this and then left the stream's `ValidationContext` unbuildable, since the mapping parses it as an asset ID. A context that will not build is not a hold an exit may have (`AGENTS.md` rule 13). No conforming writer emits anything else.
  - **§9.3's mapping table states the classification check.** A `MandateVersionApplied` whose `classification` is not mandate spec §9.2's verdict of its two stored documents is refused, whether it was applied or rejected. That is what the registration implements (#497).
  - **The vectors gain three report-order drafts:** rules 29 then 30, 30 then 33, and 31 then 32, each with its seeded bug.
  - **Order of the changes (ES-22).** The `asset_id` drafts and the reference validator's type follow the `mandate-journal` code change that types the member. `mandate-journal` parses every §9.3 vector, so vectors added first would turn `main` red, as #443 and #474 ordered rule 28. Until that code change, both `mandate-journal`'s schema and the reference validator still typed `instrument` as `id` (#509, then the reference PR that added the `asset_id` drafts).
- **v0.9 ([DEC-413](../project/decisions/DEC-413.md)):** §9.4 closes the payload schemas of the
  agent stream's `ThesisProposed` and `ThesisRevised`, which v0.6 left to their own story. Both
  share one schema. Every member comes from mandate spec §8.2's output fields, §8.4's thesis table,
  or §9's row, and is traced to `mandate-research`'s `ThesisEntry` or the writer that adds it. The
  new rules are 34 to 38: the entry type follows the revision, the reason is null exactly on
  admission, checks 1 to 3 and check 15 are recomputed from the record, and the model reference
  binds the content hash. The cited sources are recorded in the order given. No fold reads the records yet, so §9.4
  maps them to no `JournaledFact`. The test vectors gain a generated `research` section and stay
  version 3.
- **v0.8 ([DEC-403](../project/decisions/DEC-403.md)):** §9.3 closes the payload schemas of the two
  account-stream records `ValidationContext::from_journal` reads that v0.7 left to their own story
  (DEC-303 item 6): `MandateVersionApplied` and `UniverseChanged`. Every member comes from mandate
  spec §5.10's row, from a writer that exists (`mandate-spec`'s risk fold, `mandate-research`'s
  admission and removal), or from §2's `risk_clock`. The new rules are 29 to 33, and §9.3 maps each
  record to its `JournaledFact`. The test vectors gain a generated `risk_state` section and stay
  version 3.
- **v0.7, amended ([DEC-351](../project/decisions/DEC-351.md) item 5):** `OwnerCommandRefused` may
  carry `not_independent`, the executor's refusal of an acknowledgment that lifts a fired tripwire
  under `independent_approval_required` from the user who requested the lift ([mandate spec
  §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)); rule 28 keeps the reason to the account
  stream, and `OwnerAcknowledged` names the requesting user and carries the independence requirement
  as it stood when the lift was requested. The reference validator's reason list
  gains it. `mandate-journal`'s schema and rule 28 came first, in their own code PR
  ([#474](https://github.com/kunwarshivam/mandate/pull/474)), because it parses every §9.2 vector and
  code cannot ship with a reference change (ES-22); then rule 28's validator check, its vectors (an
  acknowledgment refused as `not_independent`, accepted; a Stop refused as `not_independent`,
  refused), and its seeded bug.
- **v0.7, amended ([DEC-302](../project/04-decision-log.md#decisions)):** §9.2's
  `AccountSnapshotRecorded` gains `risk_clock`, which the executor writes on every account-stream
  event and folds each event at; v0.7 had missed it, so a conforming snapshot could not have been
  replayed. The new `risk_clock` type is a whole-second timestamp. The vectors gain five invalid
  drafts: the member absent, `null`, as integer seconds (the executor's form today), not a
  timestamp, and off the second.
- **v0.7 ([DEC-261](../project/04-decision-log.md#decisions)):** §9.2 closes the payload schemas of
  the control-stream and account-stream records among those `ValidationContext::from_journal` reads
  ([DEC-169](../project/04-decision-log.md#decisions)).
  These are the control stream's `StreamOpened`, `ConnectionEstablished`, `ConnectionRevoked`,
  `DisclosureAccepted`, `ConfigSnapshotRegistered`, `MandateVersionCreated`, `MandateConfirmed`,
  `AgentDeployed`, and `AgentStopped`, and the account stream's `AccountSnapshotRecorded`. §9.2 also
  closes `OwnerCommandRefused` on both streams that write it, with DEC-291's members. Every member
  comes from a writer that exists or from a clause that names it.
  - `MandateVersionCreated`, `MandateConfirmed`, and `AgentDeployed` carry what `JournaledFact` reads
    as members, and the rest of their mandate spec §10 row as a stored record named by its hash.
  - No payload carries a credential or personal data.
  - The new types are `pointer` and `date`, and the new rules are 17 to 27.
  - §9.2 maps each record to its `JournaledFact`.
  - `PlatformOperatorAction` stays open, with a Proposed note, because no clause names the members of
    three of its five actions.
  - The test vectors gain a generated `control_stream` section and stay version 3.

- **v0.6, amended ([DEC-280](../project/04-decision-log.md#decisions)):** the agent stream gains
  `OwnerCommandRefused`, the record of a resume or Stop the runtime refused for its step-up, and the
  account stream the same for an acknowledgment the executor refused ([mandate spec §6.1](mandate.md#61-purposes),
  which already said such a refusal is journaled). §9 lists it on both streams, §2's copy paragraphs
  name it, and copy rule 16 counts it. Its payload schema is **not closed yet** (§9.1), and neither
  are the approval events v0.5 added; the catalogue entry, the closed schema, and the runtime's
  write of it land together in their own change. The test vectors are unchanged.
- **v0.6, amended ([DEC-188](../project/04-decision-log.md#decisions), [DEC-271](../project/04-decision-log.md#decisions)):**
  `DecisionMade`'s `decided_by` may be `review_ceiling`, the review date of
  [mandate spec §6.2 step 5b](mandate.md#62-evaluation), the same label the approval content's
  `trigger.decided_by` carries ([mandate spec §6.4](mandate.md#64-approvals), where its
  `trigger.rule` is `null`). Rule 7 refuses it unless `autonomy` is `ask`, as it refuses
  `client_ceiling`. The clause's vectors (an invalid and a valid draft), the generator's validator
  rule and mutant, and the agent-stream harness's count follow in one code change (ES-22; backlog
  E6-14), as DEC-177 items 6 and 14 did.
- **v0.6 ([DEC-177](../project/04-decision-log.md#decisions)):** §9.1 closes the payload schemas of
  the agent stream's `StreamOpened`, `ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`,
  `IntentProposed`, `AgentModeChanged`, `KillSwitchActivated`, and `OwnerExitRequested`, and rules on
  every contradiction [DEC-174](../project/04-decision-log.md#decisions) item 3 found in them: `null`,
  never empty strings; timestamps, never risk-clock seconds; artifact references, never inline data;
  `IntentProposed` as the `IntentReceived` vector's intent fields. `OwnerExitRequested` records the
  owner and `step_up_status` on every owner exit; `DecisionMade` carries `exit_origin` and
  `ask_suppressed`, and the order builder's numbers only on decisions it produced. §11 gains the
  per-range checks `intent_action_mismatch` and `mode_event_mismatch`. The test vectors gain a
  generated `agent_stream` section (a hash-chained stream from `StreamOpened` with its artifacts, an
  invalid draft for every rule, valid drafts, batches, and range-verification cases) and stay
  version 3 until the harness reads it. The approval events that v0.5 added are not closed here.
  Reconciled with v0.5 (DEC-177 items 21 to 24): `IntentProposed` may be caused by an
  `ApprovalRevalidated` and then repeats its approval's bound action, and rule 16 makes each copy of
  an owner command name its `OwnerCommandIssued`.
- **v0.5, amended ([DEC-181](../project/04-decision-log.md#decisions)):** `DecisionMade` names the
  source of its autonomy decision and, when a delegation lifted it, the `delegation_id` that
  delegation usage is counted from ([mandate spec §6.5](mandate.md#65-delegations-dec-181-adr-0003));
  `ApprovalRequested`'s content object lists the delegation shapes offered in `choices`, and
  `ApprovalResponded` records the shape chosen.
  `DecisionMade` also records `requested_by`, and the client's id for a client-requested order, and the
  content object's `trigger` carries both
  ([DEC-185](../project/04-decision-log.md#decisions)).
  No new event type. §9.1's closed `DecisionMade` schema lists them as `decided_by`,
  `delegation_id`, `requested_by`, and `client_id`, with rule 5 and rule 7 clauses that tie them
  to `autonomy` and refuse a client-requested opening recorded as `auto`
  ([DEC-252](../project/04-decision-log.md#decisions)); the generated `agent_stream` vectors cover
  every clause and the test vectors stay version 3.
- **v0.5 ([DEC-173](../project/04-decision-log.md#decisions), with [DEC-155](../project/04-decision-log.md#decisions),
  [DEC-156](../project/04-decision-log.md#decisions), and [DEC-158](../project/04-decision-log.md#decisions)):**
  approval escalation v0 ([mandate spec §6.1, §6.4](mandate.md#64-approvals)). The workspace
  control stream gains `ApprovalResponseSubmitted` and `OwnerCommandIssued`, and `OwnerAcknowledged`
  carries step-up evidence; owner input is journaled there first and copied by the agent runtime
  with causation (§2). The agent stream's approval rows are split: `ApprovalRequested` carries the
  content object and its hash, `ApprovalResponded` the admission result with the quorum it
  applied, and the new `ApprovalRevalidated` the re-validation result; `DecisionMade` gains
  `ask_suppressed`. Test vectors are unchanged (version 3).
- **v0.4 ([DEC-97](../project/04-decision-log.md#decisions), [DEC-111](../project/04-decision-log.md#decisions)):**
  `ThesisProposed` and `ThesisRevised` join the agent stream and `UniverseChanged` the account
  stream, where it is a risk input carrying `risk_clock` ([mandate spec §2.3, §8.4 to
  §8.6](mandate.md#23-the-working-universe-at-runtime-dec-97)); `PlatformOperatorAction` gains the
  research-agent halt of [DEC-100](../project/04-decision-log.md#decisions); `ModelOutputRecorded`
  carries the thesis direction and lineage. Test vectors are unchanged (version 3): the new events
  have no vector until the Rust harness exists.
- **v0.3 ([DEC-81](../project/04-decision-log.md#decisions)):** `risk_clock` is required on every
  account-stream risk input, named by reference to the mandate spec §5.2 list; `OwnerAcknowledged`
  is copied into the account stream as a risk input; test vectors version 3 (the `FillApplied`
  vector carries `risk_clock`).
- **v0.2:** engineer review ("needs rework") and compliance review ("approve with changes").
  Stored bytes are exactly the hashed bytes; complete canonical rules (null vs absent, key charset,
  escaping, integer and decimal grammar, timestamps); writer/journal field split, idempotency order,
  gap prevention and writer fencing; Postgres append-only hardening; self-contained per-stream
  folds; required configuration references per event type; full gate-decision records; Merkle
  anchoring definition; ordered verification checks with codes; retention, access, export,
  redundancy, clock, and incident rules; expanded catalogue; regenerated test vectors.
- **v0.1:** initial draft.

## 1. Principles

1. **Append-only.** Events are never updated or deleted. Corrections are new events
   (`CompensatingEvent`) that reference what they correct.
2. **Write before acting.** An event describing an intent, decision, or submission is durably
   committed before any side effect it authorizes.
3. **One writer per stream,** fenced by an epoch. Ordering within a stream is total and gapless.
4. **Self-contained folds.** A stream's state is a pure fold over **its own events** plus
   content-addressed configuration. Facts from other streams are copied in by the stream's owner.
5. **Tamper-evident, then write-once.** Each event commits to its predecessor by hash; stream heads
   are anchored externally; closed segments move to write-once storage.
6. **Identity is retained; personal data is referenced.** Events carry opaque references; personal
   data lives in a vault, is retained for the records retention period, and is erased only after it.

## 2. Streams

| Stream type | `stream_id` | Owner (single writer) | Contents |
|---|---|---|---|
| Account | `acct:{workspace_id}:{account_ref}` | The account's executor (trading spec §7.1); the risk gate is a pure library it calls | Intents received, gate decisions, orders, fills, fees, cash, settlement, corporate actions, reconciliation, account state, restrictions, protection changes, copies of gating facts from other streams |
| Agent | `agent:{workspace_id}:{agent_id}` | The agent's runtime. A workspace service that takes the owner's Stop may instead open it with a claim and write nothing after it (§9.21, rule 246) | Observations, model invocations, signal-model outputs, decisions, intents proposed, approvals, agent mode changes |
| Workspace control | `ctl:{workspace_id}` | Workspace control services (in Phase 1, the founder's CLI) | Mandates, deployments, connections, disclosures, policy and configuration registration, owner acknowledgments, approval responses, owner commands, and alerts, surveillance reports, anchors, verification, records lifecycle, access and export, the control-plane link's records (§9.17), and the desk's owner questions (§9.25) |
| Scheduler | `clock:{workspace_id}` | The workspace scheduler | `ClockAdvanced`, `TradingDayStarted`, clock measurements |
| Meter | `meter:{workspace_id}` | The workspace's model-gateway meter writer, one replica at a time, fenced by epoch ([inference spec §3.6](inference.md#36-journaling)) | Model-call cost reservations and settlements (§9.19). Never a risk input; spend for caps folds from it |
| Deployment meter | `dmeter:{deployment_id}` | The deployment's model-gateway meter writer, one replica at a time, fenced by epoch ([DEC-2207](../project/decisions/DEC-2207.md)) | A copy of every workspace's meter records (§9.19), for the deployment's cap. The platform operator's: no workspace member reads it. Never a risk input |
| Notice | `ntf:{workspace_id}` | The workspace's notification dispatcher ([notifications spec §5.1](notifications.md#51-the-dispatcher)) | Notices issued and every delivery attempt. Never a risk input; no other stream's owner copies from it |
| News | `news:{workspace_id}` | The workspace's news reader, one replica at a time, fenced by epoch ([DEC-2278](../project/decisions/DEC-2278.md)) | `NewsItemStored`, one per news item the reader stored (§9.37). Never a risk input, and no stream copies a record from it. The agent runtime reads it to offer items as event triggers; an item reaches a run only through the agent stream's own `ObservationRecorded`, which names the item's stored record (rule 271) |

- **Identifier grammar:** every `{…}` segment matches `[A-Za-z0-9_-]+`. `account_ref` is an opaque
  internal ULID; the broker's account number lives in the personal-data vault.
- **No global order** across streams. Relationships use `causation_id` and `correlation_id`;
  cross-stream displays sort by `recorded_at` for readability only.
- **Cross-stream facts are copied by the owner** into the consuming stream with a `causation_id`:
  the executor writes `AgentModeApplied` (from the agent stream's `AgentModeChanged`),
  `UniverseChanged` (from the agent stream's `ThesisProposed` or `ThesisRevised`, or from a
  `MandateVersionApplied` that changed a pinned universe; mandate spec §2.3),
  `TradingDayStarted`, `ClockAdvanced` (the risk clock, mandate spec §5.2: copied only when a tick
  emits an event or crosses midnight), and time-driven events
  (from the scheduler) into the account stream, and derives `RiskDayStarted` there from the copied
  `ClockAdvanced` crossing midnight America/New_York. That copy carries its tick as both `clock`
  and `risk_clock`, and names the scheduler's tick as its `causation_id` (§9.5, rules 301 and 302).
  Every other account-stream risk input
  ([mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism): `MarkUpdated`,
  `FillApplied`, `LateFillApplied`, `FeesCharged`, `CorporateActionApplied`, `CashInLieuPosted`,
  `CompensatingEvent`, `MandateVersionApplied`, `RiskDayStarted`, and the copied
  `OwnerAcknowledged`) carries a required `risk_clock` payload field (whole-second timestamp, the
  latest tick the executor had seen); `append` rejects a `risk_clock` lower than the stream's last
  one. Owner acknowledgments are recorded in the control stream and copied by the executor into the
  account stream as `OwnerAcknowledged`, with `causation_id` pointing to the original; one whose
  step-up does not count is recorded there as `OwnerCommandRefused` instead, with the same
  `causation_id`, so the account stream records every acknowledgment the executor read
  ([mandate spec §6.1](mandate.md#61-purposes)). A mode change that **originates** on the account stream (account restrictions,
  mandate risk limits) is journaled there first as `AgentModeApplied`, and the agent runtime
  copies it into the agent stream as `AgentModeChanged`; the `causation_id` always points to the
  originating event. **Owner input is journaled on the control stream first**: an answer to an
  approval as `ApprovalResponseSubmitted`, a pause, resume, Stop, owner exit, or kill switch as
  `OwnerCommandIssued` ([mandate spec §6.1](mandate.md#61-purposes)), and a request for an order as
  `OwnerRequestSubmitted` (§9.9). The runtime hands a request to its order builder, which sizes,
  clips, and classifies it as it does any proposal, and its `DecisionMade` names the request as
  `causation_id`, with the request's `requested_by` and `client_id`. The link crosses streams, so
  `append` does not check it, as for the other copies. The agent runtime copies each
  event addressed to its agent at most once, with `causation_id` pointing to it (`ApprovalResponded`,
  `AgentModeChanged`, `OwnerExitRequested`, `KillSwitchActivated`, or, for a resume, Stop, or lift
of a hold its step-up does not count, `OwnerCommandRefused`; a hold and a lift are copied at
schema version 2, §9.11), and copies an accepted `OwnerQuestionSubmitted` (§9.25) as the `RunStarted` (§9.23) of
  an `owner` run whose `trigger.cause` names it; the control stream's `event_id`
  is the idempotency key. A user's kill switch is therefore a **command**
  to the stream owners, which journal `KillSwitchActivated` in their own streams.
- A stream begins with `StreamOpened` (seq 1), which records the stream type, subject, and
  environment.
- `intent_id` equals the `event_id` of the agent stream's `IntentProposed`, except on the executor's
  own exits (`w-…`, `k-…`; trading spec §2.3), which no agent proposed (§9.1's `intent` type).

## 3. Event envelope

The **body** is the envelope without `hash`. Fields marked *journal* are assigned by the journal
service at append; all others come from the writer's draft.

| Field | Source | Type and rules |
|---|---|---|
| `envelope_version` | writer | Integer; `1` for this spec |
| `environment` | writer | `paper`, `live`, or `backtest` |
| `event_id` | writer | ULID; globally unique; idempotency key. The ULID time component carries no meaning |
| `stream_id` | writer | Per §2 |
| `seq` | *journal* | Integer ≥ 1; gapless; the **only** ordering key in a stream |
| `prev_hash` | *journal* | 64 lowercase hex; hash of the previous event; 64 zeros for seq 1 |
| `event_type`, `schema_version` | writer | Catalogue name (§9) and payload schema version |
| `event_time` | writer | Timestamp at the source (broker, exchange, local); informational |
| `recorded_at` | *journal* | Timestamp when appended; informational |
| `clock_source` | writer | `broker`, `exchange`, `local`, `scheduler` |
| `causation_id`, `correlation_id` | writer | Event ID or `null` |
| `actor` | writer | `{kind, id, version, build}`; `kind` ∈ `system`, `agent`, `user`, `broker`, `platform_operator`, `client`; `build` is the binary digest (`sha256:…`) or `null` for external actors; users are opaque IDs. A `client` actor, and only it, has a fifth member, `on_behalf_of` (below) |
| `config_refs` | writer | Map of content hashes; required keys per event type (§9) |
| `payload` | writer | Per the event type's schema |
| `artifact_refs` | writer | Sorted, de-duplicated set of every `sha256:` reference in the payload |
| `pii_refs` | writer | Sorted set of opaque vault references (random IDs, never hashes of personal data) |
| `hash` | *journal* | SHA-256 of the canonical body; **not part of the body** |

**The client actor** ([DEC-671](../project/decisions/DEC-671.md)). An owner-connected agent
(DEC-141) is recorded as `{kind: "client", id, version, build: null, on_behalf_of}`: `id` is the
client's opaque ID, and `on_behalf_of` the opaque ID (an `id`) of the user it acts for, from its
token, never from a request body. This is the one actor shape every record a client may write uses.
A client is never a `user`, so a rule that requires a user actor, such as
[mandate spec §6.4](mandate.md#64-approvals) check 3, refuses a client from the record alone. A rule
that compares people reads the **human** of an actor: `on_behalf_of` for a client, `id` for anyone
else. So check 7's "not the mandate's author", [mandate spec §5.8](mandate.md)'s requester, and
[identity spec ID-6](identity.md) count a client as its user, and a person's own client is never
their second party. Checked at append, after the envelope's types (reason `schema`):

81. `on_behalf_of` is present exactly when `kind` is `client`, and is then an `id`
    (`actor.on_behalf_of`; a well-typed string of the wrong form is `non_canonical`). Every other
    actor is exactly as before. The API sets it from the client's token, which names a user, so it
    is never the client's own `id`. The host CLI's `system` actor with `on_behalf_of` (identity spec
    §6.4) is not admitted by this version; the change that adds `HostCliRegistered` widens this rule.
82. A `client` actor's `build` is `null` (`actor.build`): a client is external.
83. A `client` actor is on the control stream, on `MandateDraftSaved`, `OwnerRequestSubmitted`,
    `RecordsAccessed`, or `OwnerCommandIssued` only (`actor.kind`), reported before the payload is
    read. `RecordsAccessed`'s payload is closed in §9.13, where a client's read uses this actor
    shape and names the client's own `id` as `accessor` (rule 108). These four records are what its
    `propose`, `request`, `read` and `dry_run`, and `hold` scopes allow (workspace API §3.8); rule 90 confines its
    `OwnerCommandIssued` to `hold_openings`. Whatever else identity spec ID-11 forbids a client (confirming, approving,
    acknowledging, pausing and every other owner command, connections, membership) is refused at
    append, and check 3 refuses an approval again at the runtime.

## 4. Canonical serialization

The canonical form is JSON per **RFC 8785** (JCS), with these constraints that make it
implementation-independent:

1. **Keys** match `^[a-z][a-z0-9_]{0,63}$` (so code-point and UTF-16 ordering agree). Maps keyed by
   data (for example, signal-model weights) are encoded as arrays of `{key, value}` sorted by key bytes.
2. **Presence:** envelope fields and schema-declared payload fields are always present, `null` when
   empty. Map-valued objects (such as `config_refs`) omit absent keys and never contain `null`.
   Empty collections are `[]` or `{}`, never `null`. Schemas use `required` and
   `additionalProperties: false`.
3. **Strings:** escape only `"`, `\`, and U+0000–U+001F, using `\b \f \n \r \t` where defined and
   lowercase `\u00xx` otherwise; `/`, U+007F, U+2028, U+2029, and all non-ASCII characters are raw
   UTF-8; no Unicode normalization. **Lone surrogates are rejected** at ingest.
4. **Integers** only for listed fields (`envelope_version`, `seq`, `schema_version`, `attempt`,
   counts and whole-second durations declared as integers in a schema), in `0 … 2^53 − 1`, no leading zeros. **Money, quantities, and prices are never integers.**
5. **Booleans** are `true` / `false`. **Floating-point numbers are rejected.**
6. **Decimals are strings** matching `^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$`, excluding `-0`, with at
   most 28 fractional digits and absolute value below 7.9 × 10²⁸. Inputs are normalized (exponent
   removed, trailing fractional zeros and trailing point removed, `-0` → `0`); values outside the
   bounds or not parseable (for example `NaN`, `Infinity`, `+1`, `1_000`, whitespace) are
   **rejected, never rounded**.
7. **Timestamps** match `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$` (UTC; seconds 00–59, leap
   seconds smeared; years 1970–9999). Dates are `YYYY-MM-DD`.
8. The canonicalizer serializes a **sorted value tree**; it never relies on struct declaration order
   or insertion-order maps.

**Hash:** `hash = SHA-256(canonical body bytes)`, written as lowercase hex.

## 5. Writing

### 5.1 Append protocol

`append(stream_id, expected_head, writer_epoch, drafts[])`, where `expected_head` is the current
head `seq` (0 for an empty stream). The journal service, not the writer, assigns `seq`,
`prev_hash`, `recorded_at`, and `hash`. In one transaction:

1. **Idempotency first.** For each draft, look up `event_id`. If every draft already exists with an
   identical canonical draft, return `AlreadyCommitted` with the stored events. If any exists with
   different content, or the batch partially overlaps, fail with `IdempotencyConflict`.
2. **Fencing and head check:** `UPDATE stream_heads SET … WHERE stream_id = $s AND seq =
   $expected_head AND writer_epoch = $epoch`. No row updated → `HeadMismatch` (returns the actual
   head) or `Fenced` (a newer epoch owns the stream).
3. Assign `seq = head + 1, …`, chain `prev_hash`, compute `hash`, insert the events, and update the
   head. Batches are all-or-nothing.

Outcomes: `Committed`, `AlreadyCommitted`, `HeadMismatch`, `IdempotencyConflict`, `Fenced`,
`Invalid` (schema, missing required `config_refs`, non-canonical values), `Unavailable` (safe to
retry with the same drafts), `Ambiguous` (the writer must re-query by `event_id` before acting).

A new writer takes ownership by incrementing `writer_epoch`; the previous writer is fenced out.

### 5.2 Write before acting, and crash recovery

- Components act only after `Committed` or `AlreadyCommitted`. The executor journals
  `OrderSubmitted` **before** the broker request.
- **Recovery:** the owner folds its stream; for every `OrderSubmitted` without a broker
  acknowledgment, it queries the broker by `client_order_id` and resubmits with the same ID only if
  the broker confirms the order is absent (trading spec §5.7). Domain duplicates (fill IDs,
  attempts) are rejected by folded state; `event_id` covers transport retries only.

### 5.3 Durability and latency

- **Durable** means committed with `synchronous_commit = on`. In managed live deployments, at least
  one synchronous standby in a second failure domain must acknowledge before the append returns.
- Group commit is allowed; `prev_hash` is chained across unacknowledged events only within one
  transaction, and any error discards the in-memory head and re-reads it.
- **Target:** p99 append under 5 ms including the synchronous standby; benchmarked in M4.

### 5.4 Clock

`recorded_at` comes from the host clock, disciplined by NTP. The scheduler measures offset at
startup, before the regular session, and at least every 10 minutes, recording sources and stratum in
`ClockOffsetRecorded`. If the offset exceeds the tolerance (default 50 ms), it journals
`ClockToleranceExceeded`, and events recorded while out of tolerance are flagged in exports. Broker
`event_time` remains authoritative for executions. §9.20 closes these records, defines the measured offset, what reads it, and the
flagged interval, and holds that a measurement no server answers is out of tolerance.

## 6. Storage

### 6.1 Hot store (Postgres)

| Table | Columns |
|---|---|
| `events` | `stream_id`, `seq` (primary key), `event_id`, `event_type`, `schema_version`, `environment`, `recorded_at`, `prev_hash bytea`, `hash bytea`, **`body bytea`** (exactly the canonical body bytes); `CHECK (octet_length(hash) = 32 AND hash = sha256(body))`; `workspace_id` with row-level security in multi-tenant deployments (`null` for the deployment meter stream, §9.19) |
| `event_ids` | `event_id` (primary key), `stream_id`, `seq` — written in the same transaction (global uniqueness under partitioning) |
| `stream_heads` | `stream_id` (primary key), `seq`, `hash`, `writer_epoch` |

- The body is never stored as `json` or `jsonb`, and readers never re-serialize to verify.
- Partitioning, when needed, is by hash of `stream_id`.
- **Append-only enforcement:** a migration-only role owns the tables. Application roles have INSERT
  and SELECT only; UPDATE, DELETE, TRUNCATE, and TRIGGER are revoked; `BEFORE UPDATE`, `BEFORE
  DELETE`, and `BEFORE TRUNCATE` triggers reject changes. No application or operator role has DDL
  rights. DDL, trigger changes, and superuser sessions are logged to write-once storage and alert.
- **The news reader writes only news streams** (DEC-2278 item 2): its role, `mandate_news_reader`,
  is an application role, and `BEFORE INSERT` triggers on `event_ids` and `events`, and
  `BEFORE INSERT OR UPDATE` on `stream_heads`, refuse every row it writes whose `stream_id` does
  not begin `news:`, with `insufficient_privilege` (SQLSTATE `42501`) and a message that begins
  `mandate_news_reader writes only news streams` and names the refused `stream_id`. The refusal holds for the session's user as well
  as the current one, so a `SET ROLE` does not lift it. The reader's append fails, and it stops
  (§9.37). Every other role is unaffected.
- **No hot-store eviction in Phase 0.** Later, a separate evictor role may delete only rows with
  `seq ≤` the last verified cold segment, journaling `SegmentEvicted`.

### 6.2 Cold store, segments, and retention

- **Segments** are contiguous `seq` ranges of one stream, shipped continuously to object storage
  (**within 1 minute** of closing before live capital; daily is acceptable in Phase 0).
- **Segment format:** JSON Lines with LF separators and a trailing LF; each line is the canonical
  form of `{"body": <body>, "hash": "<hex>"}`. Because `"body"` sorts before `"hash"`, the body bytes
  are an exact slice of the line. A **manifest** (canonical JSON: stream, first and last `seq`,
  first `prev_hash`, last `hash`, file SHA-256) is hashed and referenced by a `SegmentExported` event.
- **Object lock in compliance mode** (not governance mode) for segments, manifests, artifacts,
  configuration objects, timestamp tokens, and verifier releases. Retain-until = creation + 6 years,
  extended by a job (journaled as `RetentionExtended`) while the supported position, lot, or account
  remains open, and by legal holds (`LegalHoldChanged`). Hybrid and on-prem installers check lock
  mode and retention at startup and refuse live trading if they are wrong.
- A second-region replica of the cold store exists before live capital; a restore-and-verify drill
  runs quarterly and is journaled.

### 6.3 Artifacts

Large items (model prompts and responses, data snapshots, reports) are stored under
`sha256:{hex}` of their stored bytes. Artifacts that may contain personal data are encrypted under a
vault-held key and hashed as ciphertext. Raw broker requests and responses (credentials redacted)
below the size threshold are inline in `payload.raw`; larger ones are artifacts.

### 6.4 Personal data and identity

- Personal data (names, emails, phone numbers, broker account numbers) lives in the vault under
  per-person keys; events carry `pii_refs`.
- **Identity records** (who acted, their role, and identity-provider subject at the time) are
  retained for the records retention period under the legal-obligation exemption; erasure happens
  only after retention ends and no legal hold applies, journaled as `PersonalDataErased`.
- Free text destined for artifacts (plain-language mandates, chat replies, prompts) is scanned and
  personal data is redacted or encrypted before storage.

### 6.5 Encryption and keys

Storage is encrypted at rest with per-workspace keys (bring-your-own-key for businesses). Key
versions are retained for the retention period; rotation never destroys old versions; rotations and
revocations are journaled (`KeyRotated`, `KeyRevoked`). Hashes are over plaintext canonical bodies;
anchors reveal only hashes.

## 7. Access

| Role | Access |
|---|---|
| Owner, workspace admin | Read their workspace's records; request exports |
| Auditor (read-only) | Read records and verification results; exports |
| Records custodian | Legal holds, retention extensions, examination bundles |
| Platform staff (managed mode) | **Break-glass only**, with customer approval, journaled to a stream the customer can read |

Every read of records outside normal product views and every export is journaled (`RecordsAccessed`,
`ExportCreated`; §9.13) before it is served. In hybrid mode the platform receives anchors (hashes)
only.

## 8. Replay, snapshots, and versioning

- **Replay** folds one stream from seq 1 (or a verified snapshot) in `seq` order. Configuration is
  resolved by `config_refs` from the immutable configuration store; a missing object halts replay.
- **Snapshots** are the canonical JSON of `{stream_id, seq, last_event_hash, fold_version, state}`,
  hashed. Starting from a snapshot requires `last_event_hash` to equal the stored hash at that seq.
  Snapshots are compared only within the same `fold_version`; a fold change requires a journaled
  re-derivation. A full replay from seq 1 runs periodically.
- **Schema versions:** stored events are never rewritten. Upcasters are pure, registered per
  `(event_type, from_version)`, and covered by golden tests on stored bytes. All versions are kept
  for the retention period. The envelope field set is frozen per `envelope_version`.
- **Late events** are appended at their arrival `seq`; replay never re-sorts by `event_time`.

## 9. Event catalogue

Payload schemas live in code with JSON Schema exported to `schemas/events/`. **Required
`config_refs`** keys are enforced at append (`fee` = `fee_config`, `cal` = `trading_calendar`,
`set` = `settlement_calendar`, `ins` = `instrument_snapshot`, `rule` = `rule_set`, `man` =
`mandate_version`, `mod` = `model_version`, `pol` = `policy_set`, `reg` = `model_registry`).
`policy_set` names the canonical effective-policy snapshot: an object with exactly
`kind: "policy_set"`, `policy_set_version: 1`, and `levels`. Each `levels` item is a complete
document valid against [`policy.schema.json`](../../schemas/policy.schema.json), ordered
`platform`, `organization`, `workspace`, with at most one of each. `model_registry` names an object
with exactly `kind: "model_registry"`, `model_registry_version: 1`, and `models`; each model has
exactly `model_id`, `model_version`, `content_hash`, `params`, and `admits_instruments`. Models are
strictly sorted and unique by `model_id`; `params` are strictly sorted and unique. These objects
are canonical configuration artifacts registered on the control stream before use.
At append, these two new kinds have an additional object-shape check: a `policy_set` or
`model_registry` reference whose stored object is absent is `missing_artifact` at that
`config_refs.<kind>`, and an object whose own `kind` differs is `config_ref_kind` at the same path.
The older kinds retain their existing object contracts. For a version-2 `ConfigSnapshotRegistered`
of either new kind, the corresponding paths are `payload.content_hash` and `payload.kind`.

`broker_profile` names a connector's capability profile ([trading spec §5.2](trading-domain.md),
[DEC-531](../project/decisions/DEC-531.md), [DEC-630](../project/decisions/DEC-630.md)): an object
with exactly `kind: "broker_profile"`, `profile_version` (an integer, at least 1), `rows`, and
`idempotency`.

- `rows` is a non-empty array of objects with exactly `asset_class` (`crypto`, `us_equity`),
  `session` (`overnight`, `pre_market`, `regular`, `after_hours`, `crypto`), and `cells`, strictly
  sorted and unique by `asset_class` then `session`.
- `cells` is a non-empty array of objects with exactly `order_type` (`limit`, `market`, `stop`,
  `stop_limit`), `quantity_form` (`fractional`, `notional`, `whole`), `times_in_force`, and
  `protection_forms`, strictly sorted and unique by `order_type` then `quantity_form`.
- `times_in_force` is a non-empty set of `day`, `gtc`, `ioc`, and `protection_forms` a set, possibly
  empty, of `bracket`, `oco`, `stop_limit`; each set is strictly ascending by bytes.
- `idempotency` has exactly `client_order_id` and `query_by_client_order_id` (booleans) and `retry`
  (`idempotent`, `not_idempotent`, `unknown`). Without a client order id, `retry` is
  `not_idempotent` and `query_by_client_order_id` is false.

Every comparison is by the bytes of the spelling. The profile is a canonical configuration artifact
registered on the control stream before use. No record names it in `config_refs` yet.
The producer enforces this shape: `mandate-domain`'s profile constructor refuses a profile that
breaks it (DEC-630 item 7), and the vectors' `invalid_artifacts` use its refusal codes
(`profile_version_zero`, `profile_no_rows`, `profile_empty_row`, `profile_duplicate_row`,
`profile_duplicate_cell`, `profile_no_time_in_force`, `profile_claim_without_client_id`). Two codes
are the vectors' own, for stored bytes the constructor cannot produce: `schema` for a member or
spelling outside the contract, and `non_canonical` for an array that is not strictly sorted, since
the constructor sorts what it is given. Append binds only the object's kind, its presence and its
hash (rule 21b), as rule 21a does for the version-2 kinds.

**Account stream** (owner: executor). Risk inputs also carry `risk_clock` (§2).

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `IntentReceived` | man | intent ID, agent, instrument, side, type, TIF, quantity, limit, purpose |
| `GateDecided` | fee, cal, ins, rule, man | intent ID, verdict, reason code, **`checks: [{id, result, inputs, computed}]`** with IDs matching trading spec §9.1 (`account_status`, `agent_mode`, `eligibility`, `concentration`, `order_size`, `session`, `halt`, `order_constraints`, `mark_freshness`, `collar`, `conduct`, `buying_power`, `gross_exposure`, `day_trade_budget`), `quotes_used`, `marks_used`, `data_profile`; at version 3 also `sized_qty`, `parked` and `held_long` (§9.5, [DEC-2848](../project/decisions/DEC-2848.md)) |
| `OrderSubmitted`, `OrderAbandoned` | — | client order ID, attempt, broker status, internal state; `OrderAbandoned` closed at schema version 1 with the intent and the reason (§9.5, [DEC-2803](../project/decisions/DEC-2803.md)) |
| `OrderStateChanged` | — | Closed at schema version 1: required `client_order_id`, `state`, and `risk_clock`; always-present nullable `attempted`, `broker_status`, `filled_qty`, `reject_code`, `replaces`, `replaced_by`, `replaced_by_broker_order_id`, and `lookup`; always-present booleans `ignored`, `cancel_requested`, `cancel_confirmed`, `cancel_overdue`, `adopted`, and `ladder_step`. `state` and non-null `attempted` use the complete executor order-state vocabulary; non-null `lookup` is `absent`; `filled_qty` is a decimal; opaque text is non-empty ([DEC-459](../project/decisions/DEC-459.md)). Version 2 adds `broker_order_id` last, closed in §9.16 ([DEC-869](../project/decisions/DEC-869.md)) |
| `OrderRequestRecorded` | — | the exact request a version-2 `OrderSubmitted` names (DEC-360 option (c)): agent, intent, purpose, extended hours, the protective trigger and legs, and a ladder rung's; closed in §9.5 |
| `OwnerConfirmationReceived` | — | the bid, bid size, and floor the owner confirmed for an owner's exit, journaled beside its `IntentReceived` ([DEC-2040](../project/decisions/DEC-2040.md)); closed in §9.5 |
| `BrokerExchangeRecorded` | — | direction, endpoint, `raw` (or artifact), status; credentials redacted |
| `FillApplied`, `LateFillApplied` | fee, cal, set, ins | fill ID, client order ID, gross quantity, price, fees, trade date; `FillApplied` closed at schema version 2, whose client order ID is `null` for an unattributed fill (§9.5, [DEC-2812](../project/decisions/DEC-2812.md)); at version 2, `ins` only for a fill that names an order (rule 300, [DEC-2817](../project/decisions/DEC-2817.md)) |
| `FeesCharged` | fee | family, day, accrued, charged |
| `MarkUpdated` | — | instrument, price, source, feed; version 2 adds the risk mark's `bid`, its `sane` verdict, and its `session`, closed in §9.30 ([DEC-2305](../project/decisions/DEC-2305.md)) |
| `SettlementPosted`, `DividendPaid`, `CashInLieuPosted` | set | date, instrument, amount |
| `CorporateActionPrepared`, `CorporateActionApplied` | ins | instrument, action, ratio or amount, ex-date |
| `ProtectionChanged` | — | instrument, action, orders, unprotected-interval start or end; closed in §9.5 |
| `BrokerPositionObserved` | — | Closed at schema version 1: instrument, broker and model quantities, mismatch flag, and risk clock (§9.6) |
| `ReconciliationRun` | — | result (`clean`, `adopted`, `mismatch`), nullable activity checkpoint, snapshot head, difference count, risk clock (DEC-453) |
| `CompensatingEvent`, `AccountSnapshotRecorded` | — | Closed `CompensatingEvent` version 1: order subject, order-state difference, from/to states, corrected event IDs, and risk clock (§9.6); daily snapshot, closed at versions 1 (§9.2) and 2 (§9.35: the declared account type and day-trading regime, and the broker's prior-close equity and maintenance margin) |
| `AccountStateObserved` | — | Closed at schema version 1: `status`, `crypto_status`, `trading_blocked`, `account_blocked`, `trade_suspended_by_user`, `multiplier`, `equity`, `cash`, `buying_power`, `non_marginable_buying_power`, `accrued_fees`, and `risk_clock` ([DEC-458](../project/decisions/DEC-458.md)); no broker account identifier or personal data |
| `RejectObserved`, `AccountRestrictionChanged` | — | reject code and message, restriction; `RejectObserved` closed at schema version 1 with the HTTP status (§9.5, [DEC-2803](../project/decisions/DEC-2803.md)); `AccountRestrictionChanged` closed at schema version 1 with its `cause` (`broker_reject`, `broker_notice`, `connection_unavailable`; [trading spec §7.3](trading-domain.md#73-account-restrictions), [DEC-441](../project/decisions/DEC-441.md) item 23), `reason_code`, and risk clock (§9.5, [DEC-2137](../project/decisions/DEC-2137.md)) |
| `AccountRestrictionLifted` | — | One cause of an account restriction lifted: the cause, the accepted acknowledgment copy, the evidence, and the account state left; closed at schema version 1 (§9.5, [DEC-2824](../project/decisions/DEC-2824.md)) |
| `ExternalActivityIngested`, `RelatedAccountsCoordination` | — | unattributed activity, closed at schema version 1 with its `source` (§9.5, [DEC-2803](../project/decisions/DEC-2803.md)); canceled opening orders across the group |
| `ConductBreachDetected` | rule | control, agent, instrument, measured value |
| `AccountCloseRequested` | — | Closed at schema version 1: instrument, quantity held, the account `KillSwitchActivated` it carries out, attempt, and risk clock; journaled before each close-position request (§9.5, [DEC-2337](../project/decisions/DEC-2337.md)) |
| `AgentModeApplied`, `TradingDayStarted`, `KillSwitchActivated` | — | Closed `AgentModeApplied` version 1: agent (including `*`), mode, restriction, originated flag, and risk clock (§9.6); other gating facts copied or originated (with `causation_id`); `KillSwitchActivated` closed at schema version 1 (§9.5, [DEC-2139](../project/decisions/DEC-2139.md)): scope, initiator, purpose, orders canceled (`cancels`, the orders whose cancel never lapses while they work, [DEC-2302](../project/decisions/DEC-2302.md) item 2), sells planned or deferred, the owner's confirmation, and `copied_from` |
| `ClockAdvanced` (copy) | — | The scheduler's tick, copied when it returns a risk record or crosses New York midnight (§2): closed at schema version 1 with `clock` and `risk_clock`, rules 301 and 302 (§9.5, [DEC-2808](../project/decisions/DEC-2808.md)) |
| `OwnerAcknowledged`, `OwnerAcknowledgmentRequested` | — | copied from the control stream (with `causation_id`); a risk input; judged at processing as §9.28 says |
| `ConnectionChecked`, `ConnectionStateChanged`, `ConnectionCredentialRefreshed` | — | The executor's permission checks and their results; the connection's state (`active`, `degraded`, `suspended`) and why; a refreshed token's scopes; each with `risk_clock`, closed in §9.8 |
| `ConnectionEstablished`, `ConnectionCredentialRotated` (copies) | — | The executor's copies of the control stream's establishment (version 2) and each rotation, with `risk_clock` and the original as `causation_id`; closed in §9.8 |
| `OwnerCommandRefused` | — | An acknowledgment the executor refused, for its step-up or, under `independent_approval_required`, because it is not independent ([mandate spec §6.1](mandate.md#61-purposes), [§6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)), or because its trigger or request does not hold (§9.28): the command (`acknowledge`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`, `not_independent`, `trigger_unknown`, `trigger_superseded`, `request_invalid`), and the effective time it was judged at; `causation_id` is the control stream's `OwnerAcknowledged`, copied at most once. The agent runtime records a refused resume or Stop the same way on the agent stream |
| `MandateVersionApplied`, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, `GoalCompleted` | man | agent risk state ([mandate spec §5.10](mandate.md#510-journal-events)): version result, classification, and allocation change; day-start equity; limit, action, E, H, drawdown, E₀, capital base C, inherited loss L, net contributed N, and for a tripwire (limit `tripwire:<id>`, reason `tripwire_condition`, [mandate spec §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)) its metric, threshold, and the value reached; reset evidence; released positions; stale-mark and removed-instrument changes with the reason; goal completion; `MandateVersionApplied` is closed in §9.3 and the other seven in §9.27 |
| `UniverseChanged` | man | The working universe changed ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)); a risk input, so it carries `risk_clock`: agent, instrument, change (`admitted`, `removed`), reason (`thesis_admitted`, `thesis_expired`, `thesis_invalidated`, `lineage_retired`, `eligibility_lost`, `operator_halt`, `version_applied`), thesis and lineage ids, working-universe size after |

**Agent stream** (owner: agent runtime)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `ObservationRecorded` | — | source, instrument, data (artifact) |
| `ModelInvocationRecorded` | mod | A gateway call's record (fast model, LLM signal model, research; the compiler's is on the control stream, §9.9): model, endpoint, request digest, sampling, prompt and response (artifacts), reported identity, provider request ID, outcome, attempts, tokens, cost, price table, cache hit, deadline, completion, the meter reservation, the retrieved context, and the candidates' verdicts; closed in §9.19 |
| `RunStarted`, `RunEnded` | mod | An agentic research run's start, with its kind, trigger, menu, and frozen scope, ceiling, and deadline, and its end, with the reason and counts ([agent harness spec §6.9](agent-harness.md#69-the-agentic-loop)); closed in §9.23 |
| `ToolCallRecorded`, `ToolResultRecorded` | mod | One tool call a turn asked for (the turn, its index, the tool and class, the arguments, an action's provenance) and its result (the result and code, content, bytes, `truncated`, observations); closed in §9.23 |
| `RunSummaryRecorded` | mod | A run's typed summary, the memory the next run reads as untrusted input; closed in §9.23 |
| `ModelOutputRecorded` | v1: man; v2: man, reg | signal model id, version, content hash, instrument, as_of, expires_at, direction, conviction, confidence, horizon, thesis (artifact) and, for the research agent, its thesis and lineage ids; `ignored` reason if not used |
| `ThesisInvalidated` | man | The invalidation verdict that removes a thesis's instrument ([mandate spec §8.6](mandate.md#86-thesis-lifetime-and-revision-lineages-dec-118-dec-111)): the thesis, its lineage and instrument, the cause (the model's judged verdict, or a revoked source), and the items or source it rests on; closed in §9.22 |
| `ThesisProposed`, `ThesisRevised` | man, mod | The research agent's output and its admission decision ([mandate spec §8.4, §8.5](mandate.md#84-the-research-agent-dec-97-adr-0002)): research agent id, version, and content hash; thesis id, lineage id, revision, and for `ThesisRevised` the `predecessor_thesis_id` and what the revision changed; instrument, asset class, direction, horizon, evidence and cited sources, corroboration kind, invalidation, conviction, confidence; the source-allowlist version; prompt and response (artifacts); `admitted` and the refusal reason from the ordered §8.5 checks; closed in §9.4 |
| `DecisionMade` | v1: man; v2: man, pol, reg; v3: man | proposed action, combined conviction and combined score, outputs used, model weights, clips applied, gate dry-run result, autonomy classification and its source (`rule:<id>`, `default`, built-in, the admission ceiling, or the client ceiling), `delegation_id` when a delegation lifted it ([mandate spec §6.5](mandate.md#65-delegations-dec-181-adr-0003)), and `requested_by` (`agent`, `owner`, or `client`) with the client's id when a connected client asked (mandate §6.2 step 5a, DEC-185); `ask_suppressed` (`budget`, `skipped_today`, `recent_timeout`) when an `ask` was classified but not asked ([mandate spec §6.4](mandate.md#64-approvals)) |
| `IntentProposed` | man | intent fields (its `event_id` is the intent ID); after a grant, `causation_id` is the `ApprovalRevalidated` |
| `ApprovalRequested` | man | Closed at schema versions 1, 2 and 3: approval (its `event_id`), instrument, asset class, side, quantity, limit price, purpose, mandate version, `decided_by`, score, approver requirements, nullable reference mark, deadline, timeout/default, recursively closed content, and its canonical hash; version 2's content also binds the order builder's execution plan (§9.6, rule 136), and version 3's also names the checks the dry run deferred to the binding gate (rules 291 and 292) |
| `ApprovalDelivered` | man | Closed at schema version 1: approval, `cli_inbox` channel, delivery status (`delivered`, `suppressed_quiet_hours`, `failed`), and nullable message ID (§9.6) |
| `ApprovalResponded` | man | approval, verdict (`approved`, `skipped`; a legacy `denied` reads as `skipped`), responder (opaque) and role, result (`admitted`, `counted`, `refused`; a legacy `recorded` or `refused` reads as terminal), reason, effective time, step-up evidence (assertion ID, authentication time, method), separation-of-duties result, and for a grant that reaches check 7 the approver count and independence it applied (the stricter of the bound values and the policy overlay, [mandate spec §6.4](mandate.md#64-approvals)); `causation_id` is the `ApprovalResponseSubmitted`, copied at most once; the delegation shape chosen, if any, with the new mandate version and delegation id (mandate §6.4, §6.5) |
| `ApprovalRevalidated` | man | approval, result (`act`, `skip`), reason, and every value compared: bound and current mandate version, mode, instrument restriction, `decided_by` then and now, dry-run verdict and reason, `m_req`, `m_now`, `band_bp` |
| `ApprovalTimedOut` | man | Closed at schema version 1: approval and `on_timeout: skip`; `causation_id` is the ask (§9.7, rule 135) |
| `RetirementHeld` | man | Closed at schema version 1: the risk clock the retirement began waiting at, the period, what it waits on (`position`, `working_order`), and risk clock; `causation_id` is what began the retirement (§9.34, rules 265 and 266) |
| `ApprovalCanceled` | man | Closed at schema version 1: approval, cancel reason (`version_applied`, `mode_tightened`, `owner_pause`, `owner_stop`, `kill_switch`; a legacy `rebound` is a cancellation for either of the first two, and §9.7 refuses it) |
| `AgentModeChanged`, `KillSwitchActivated` | — | from, to, reason; scope and initiator; `AgentModeChanged` version 2 also records the owner's hold (§9.11) |
| `OwnerExitRequested` | man | instrument or scope, bid shown and confirmed, user (opaque), step-up evidence |
| `OwnerCommandRefused` | — | A resume or Stop the runtime refused for its step-up ([mandate spec §6.1](mandate.md#61-purposes)): the command (`resume`, `stop`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`), and the effective time it was judged at; `causation_id` is the `OwnerCommandIssued`, copied at most once. The executor records a refused acknowledgment the same way on the account stream |

**Workspace control stream** (owner: workspace services)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `MandateDraftSaved` | — | One explicit save of a mandate draft: draft, the draft stored (artifact), origin, the save it replaces, the version it started from; closed in §9.9 |
| `ModelInvocationRecorded` | mod | The compiler's model call, for a draft ([inference spec §3.6](inference.md#36-journaling)); closed in §9.9 |
| `OwnerQuestionSubmitted` | — | A member's question to their agent at the desk ([agent harness spec §15](agent-harness.md#15-owner-and-explain-runs-the-desk)): agent, the question (artifact, never stored when the guard refused it, with the guard's rule); closed in §9.25. The runtime copies it and starts an `owner` run. Never a risk input |
| `MandateVersionCreated`, `MandateConfirmed` | — | per [mandate spec §10](mandate.md#10-records-dec-51-dec-97): source text (artifact), compiled fields, provenance per path with quoted spans, template, policy-set hashes, validation results and warnings, classification, diff; version hash, confirmed paths, rendered confirmation (artifact) and UI build, warnings acknowledged, step-up evidence, confirming user (opaque); `MandateConfirmed` version 2 also names the agent and the version it replaces (§9.9) |
| `AgentDeployed`, `DeploymentRejected`, `AgentStopped` | man | agent, mandate version, reason (`goal_complete`, `profit_stop_reached`, `end_date`, owner stop), net dollar loss added to the connection's loss carry; for `AgentDeployed`: the rendered go-live screen (artifact), backtest and paper-run IDs shown, performance legend and disclosure versions shown, approving users, step-up evidence ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)), and at version 2 the document's connection (§9.24) |
| `PolicyChanged`, `WorkspaceProfileAssigned` | — | level, diff, author (opaque), step-up evidence, affected agents; profile, basis, assigning user |
| `ConnectionEstablished`, `ConnectionRevoked` | — | broker, scopes granted, permission-check result; closed in §9.2, version 2 of `ConnectionEstablished` with `account_ref`, user, step-up, and the live margin attestation in §9.8 |
| `ConnectionRequested` | — | A connect's start: the pending connection's `connection_id` and `account_ref`, broker, environment, user (opaque), and step-up evidence; closed in §9.8, version 2 with the credential kind and an OAuth token's breadth and confirmed disclosure (rule 255) |
| `ConnectionRefused`, `ConnectionCredentialRotated` | — | A connect, reconnect, or credential replacement the permission checks refused, with the check and reason, or the teardown that ended it; a credential replaced on a connection that is not revoked; closed in §9.8 |
| `ClientConnected`, `ClientRevoked` | — | An owner-connected client issued or revoked ([identity spec §12.1](identity.md)): client, user, scopes, agents, step-up; closed in §9.10 |
| `DisclosureAccepted` | — | document and version hash, user (opaque), step-up evidence |
| `OwnerAlertSent` | — | Written by the owner of the subject event's stream, in that stream and in the subject's batch, on the agent, account, or control stream ([notifications spec §5.5](notifications.md#55-records)): subject event, kind (notifications spec §3.2), and for a kill switch the owner command it carries out, if any. It records that an alert was raised; delivery is the notice stream's. Closed in §9.15 |
| `NotificationAddressChanged` | — | A member set or removed one of their own push addresses ([workspace API spec §4.11, §5.7](workspace-api.md), [DEC-795](../project/decisions/DEC-795.md) item 5): member (opaque), channel (`web_push`), action (`added`, `removed`), `address_ref` (the `added` event's own id, listed in `pii_refs`), and step-up evidence (assertion ID, authentication time, method), `null` only for a `removed` that rides a deactivation's commit; written with its `OwnerAlertSent` (`notification_address_changed`) in one batch. The endpoint and keys stay in the vault. Closed in §9.15 (v0.51, [DEC-2505](../project/decisions/DEC-2505.md)) |
| `OwnerAcknowledgmentRequested` | — | An owner's request that one triggered limit be lifted, where an independent user must acknowledge it ([mandate spec §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)): agent, the `RiskLimitTriggered` named, or from version 2 the `AccountRestrictionChanged` (`subject`), the requesting user (opaque), the independence requirement as it stood, step-up evidence; closed in §9.28 |
| `OwnerAcknowledged` | — | user (opaque), the user who requested the lift (opaque) and the independence requirement as it stood when the lift was requested, carried so the executor applies the stricter of it and the overlay at processing ([mandate spec §5.8, §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)), the request it answers when an independent user is required, the `RiskLimitTriggered` named, or from version 2 the `AccountRestrictionChanged` (`subject`), step-up evidence (assertion ID, authentication time, method); closed in §9.28 |
| `StepUpConfirmed` | — | The CLI's record that the owner confirmed one workspace API step-up challenge on the host, for a paper grant made in the web app ([DEC-2104](../project/decisions/DEC-2104.md)) or a paper OAuth connect ([DEC-2125](../project/decisions/DEC-2125.md)): the challenge, its action kind and digest, the principal, the code shown, the second it was confirmed; closed in §9.29 |
| `RealMoneyGateChanged` | — | A platform operator set whether the workspace may reach an environment other than `paper`: on or off, the operator, and for on the decision that allows it; the latest one by `seq` is the setting, and none is off; closed in §9.36 ([DEC-2134](../project/decisions/DEC-2134.md)) |
| `RealMoneyRefused` | — | The workspace API refused a request that named an environment other than `paper` because the setting is off: the action, the environment requested, the principal, the reason; closed in §9.36 |
| `ApprovalResponseSubmitted` | — | The owner's answer to an approval ([mandate spec §6.4](mandate.md#64-approvals)): agent, approval, verdict (`approved`, `skipped`), content hash, `submitted_at`, step-up evidence (assertion ID, authentication time, method) or null, responder (opaque) and role |
| `OwnerRequestSubmitted` | — | An owner's request for an order (workspace API §4.6): agent, instrument, side, optional size, who asked; closed in §9.9 |
| `OwnerCommandIssued` | — | The owner's command ([mandate spec §6.1](mandate.md#61-purposes)): agent or kill-switch scope, command (`pause`, `resume`, `stop`, `kill_switch`, `owner_exit`, and `hold_openings` and `lift_hold`, closed in §9.11), the release choice and warning shown for a Stop with release, the bid, bid size, and floor confirmed for an owner exit, `submitted_at`, step-up evidence or null, user (opaque) |
| `ConfigSnapshotRegistered` | — | configuration kind (fee, calendar, instrument snapshot, rule set, mandate, model, policy set, model registry, broker profile), content hash |
| `SurveillanceReportGenerated` | rule | period, report (artifact), breaches |
| `BacktestRunRecorded` | rule, man | A backtest of a mandate version: data snapshot, model outputs and theses (digests), configuration, code build, the report (artifact), and the `backtest` marker; closed in §9.18 |
| `EvaluationRunRecorded` | rule | The forward-paper evaluation of the research agent's theses (E17-8), in an internal paper workspace only: the registered decision, cut-off, the thesis records read, closes, cost model, code build, the report (artifact), and whether it passed; closed in §9.18 |
| `PlatformOperatorAction` | — | action (stop, global kill switch, acceptable-use action, `model_withdrawn` with model and reason, `research_thesis_halt` with the instrument and optionally the research agent's pinned content hash, [DEC-100](../project/04-decision-log.md#decisions)), operator (opaque), approval |
| `AnchorComputed`, `VerificationRun`, `IntegrityIncidentRecorded` | — | leaves, root, timestamp token (artifact), closed in §9.14; scope and result, closed in §9.13; last good hash and anchor |
| `SegmentExported`, `SegmentEvicted`, `RetentionExtended`, `LegalHoldChanged` | — | manifest hash, range, retain-until, hold. `SegmentExported` is closed in §9.14 |
| `KeyRotated`, `KeyRevoked`, `RecordsAccessed`, `ExportCreated`, `PersonalDataErased` | — | key version; accessor (opaque), scope; export manifest; subject reference. `RecordsAccessed` and `ExportCreated` are closed in §9.13 |
| `MemberInvited`, `MemberInvitationRevoked`, `MemberActivated`, `MemberRoleChanged`, `MemberDeactivated`, `MemberReactivated`, `MemberRemoved` | — | A workspace membership ([identity spec §5](identity.md#5-membership-lifecycle)); closed in §9.12 |
| `CredentialEnrolled`, `CredentialRemoved` | — | member, credential id, public key (artifact, never inline), algorithm, counter, enrolment cool-off end, step-up ([identity spec §10.1, §12.1](identity.md#121-events-journal-9-control-stream)); closed in §9.31 |
| `StepUpAsserted` | — | The assertion behind one passkey step-up: challenge, credential, counter, raw assertion (artifact), immediately after the event it authorized in one batch ([DEC-2108](../project/decisions/DEC-2108.md) item 3); closed in §9.32 |
| `SessionOpened` | — | member, session (opaque), method, device (opaque), `first_seen_device`; the subject of the notifications spec's `new_device` kind |
| `SessionRevoked` | — | member, session (opaque), reason (`sign_out`, `deactivated`, `deprovisioned`, `refresh_reuse`, `refresh_failed`, `expired`, `admin`, as [identity spec §12.1](identity.md#121-events-journal-9-control-stream) defines them); with `deprovisioned`, the subject of the notifications spec's `deprovisioned` kind |
| `ServiceAccountIssued`, `ServiceAccountRevoked` | — | account, scopes, workspaces, expiry, issuing user |
| `HostCliRegistered`, `HostCliRevoked` | — | registration (its ULID), host (opaque), operating-system account (opaque), registering admin, step-up evidence ([identity spec §6.4](identity.md#64-the-risk-reduction-path)) |
| `BreakGlassRequested`, `BreakGlassGranted`, `BreakGlassEnded` | — | operator (opaque), reason code, window, approvers ([identity spec §10.3](identity.md#103-break-glass-for-platform-staff-managed-mode)) |
| `ControlPlaneEnrolled`, `LicenseApplied`, `LicenseStateChanged`, `ReleaseOffered`, `ReleaseInstalled`, `ReleaseWithdrawnNoticed`, `CatalogEntryRegistered`, `DataBundleImported`, `UsageReportSealed`, `ControlPlaneMessageRefused` | — | The deployment's records of its link to the global control plane ([control-plane design §3.8](../design/control-plane.md#38-the-closed-message-set)): enrollment, license, release, catalog, data-bundle, and usage-report facts by opaque ID, sequence, and digest, and each refused inbound message with its reason; written by workspace control services, never by `cp-agent`; closed in §9.17 |
| `ConnectionRolesMade`, `ConnectionRolesRefused`, `ConnectionRolesRemoved` | — | The role provisioner's records for one OAuth connect's Kubernetes-auth roles: connection, and for a refusal its reason, for a removal the role and the occasion; closed in §9.33 |
| `ProcessStartIssued`, `ProcessStopIssued`, `ProcessCrashLooped` | — | The deployment manager's starts, stops, and crash loops (infrastructure design §3.4): process (`runtime`, `executor`), subject (the agent or the `account_ref`), generation, cause; for a crash loop the restarts counted and their window; closed in §9.21 |

The identity records ([identity spec §12.1](identity.md#121-events-journal-9-control-stream),
DEC-437 item 9) are control-stream records. The membership records close in §9.12. The client
records `ClientConnected` and `ClientRevoked` are closed in §9.10 with the `client` actor (§3), journal
spec v0.22 ([DEC-671](../project/decisions/DEC-671.md)). The
credential, session, service-account, host-CLI, and break-glass records are listed with the members
the identity spec names and close in their own change, as §9.2's other records do; until then
`append` refuses them as `unknown_event_type`. `ScopeHalted` and `ScopeReenabled` are not catalogued: they
exist only if DEC-437 item 21 (Proposed) is accepted (identity spec §4.4).
`NotificationAddressChanged`, the identity spec's row for a member's own push address, is closed in
§9.15 with the notification records (v0.51, [DEC-2505](../project/decisions/DEC-2505.md),
[DEC-795](../project/decisions/DEC-795.md)).

**Scheduler stream:** `StreamOpened`, `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`,
`ClockToleranceExceeded`; closed in §9.20.

**Meter stream** (owner: the workspace's model-gateway meter writer; [inference spec §3.6, §7.3](inference.md#73-budget-enforcement))

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, workspace; closed in §9.19 |
| `MeterReserved` | — | A call's reservation of its maximum cost before its first attempt: call, agent, purpose, model, endpoint, key owner, price table, maximum cost, risk day; closed in §9.19 |
| `MeterSettled` | — | The call's settlement: call, reservation, outcome, attempts, tokens, cost; closed in §9.19 |

**Deployment meter stream** (owner: the deployment's model-gateway meter writer; [DEC-2207](../project/decisions/DEC-2207.md))

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, deployment; closed in §9.19 |
| `MeterReserved`, `MeterSettled` | — | Copies of every workspace's meter records, the meter stream's schemas; closed in §9.19 |

**News stream** (owner: the workspace's news reader; [DEC-2278](../project/decisions/DEC-2278.md))

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, workspace; closed in §9.37 |
| `NewsItemStored` | `item_ref` | source, allowlist version, item id, tickers, published and known times, the item's typed record, licensed; no text; closed in §9.37 |

**Notice stream** (owner: the workspace's notification dispatcher; [notifications spec §5.5](notifications.md#55-records))

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, workspace; closed in §9.15 |
| `NoticeIssued` | — | notice id (random, notifications spec §4.2), kind, class, cause (the `OwnerAlertSent`, `ApprovalRequested`, or (for `channel_lost`) `NoticeAttempted` it answers, with its stream), recipients (opaque); closed in §9.15 |
| `NoticeAttempted` | — | notice id, recipient (opaque), channel, attempt, status (`delivered`, `failed`, `suppressed_quiet_hours`, `deferred_quiet_hours`, `abandoned`), reason, provider message id, `coalesced_into`; closed in §9.15, whose version 2 adds a `failed` attempt's `verdict` ([DEC-706](../project/decisions/DEC-706.md)) and version 3 the web-push `address_ref` and the `address_retired` stop ([DEC-2505](../project/decisions/DEC-2505.md)) |

### 9.1 Agent-stream payload schemas ([DEC-177](../project/04-decision-log.md#decisions))

This subsection closes the payload schemas of the agent stream's `StreamOpened`,
`ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`, `IntentProposed`, `AgentModeChanged`,
`KillSwitchActivated`, and `OwnerExitRequested`. For these events it replaces the "Key payload fields"
column of §9, and the required `config_refs` stay as §9 lists them. Each schema is `schema_version` 1:
a record with exactly the listed members, every one present (§4.2). The test vectors' `agent_stream`
section holds at least one chain event per schema, an invalid draft for every rule below, and valid
drafts for the cases a rule might be misread to refuse. The other agent-stream events are not
closed yet: the approval events that v0.5 added (§9, mandate spec §6.4) close in their own change,
and `ModelInvocationRecorded` in §9.19. `ThesisProposed` and `ThesisRevised` are closed in
§9.4, and `ThesisInvalidated` in §9.22.
`OwnerCommandRefused` is closed in §9.2, on both streams that write it.

Journal spec v0.16 adds `schema_version` 2 for `ModelOutputRecorded` and `DecisionMade`. Each has
exactly its version-1 payload, with the stricter `config_refs` §9 lists. Rule 13a requires a
version-2 model output's `model_registry` object to contain exactly one entry whose `model_id`,
`model_version`, and `content_hash` equal the payload's. Version 1 remains registered and replayable;
a writer emits version 2 once it has the two new registered snapshots. The additive
`production_config_refs` vectors hold both version-2 drafts and the version-2 control records that
register their snapshots, without rewriting the historical version-1 chain.

**Types.**

| Type | Values | Refused as |
|---|---|---|
| `text` | A non-empty string; an empty value is `null` (§4.2) | Not a string: `schema`; empty: `non_canonical` |
| `id` | `[A-Za-z0-9_-]+` (§2) | Not a string: `schema`; otherwise `non_canonical` |
| `ulid` | An event ID (§3) | As `id` |
| `intent` | An intent's ID: a `ulid`, the agent stream's `IntentProposed`; or the executor's own exit's intent, trading spec §2.3: `w-` and a `ulid` (the triggered-stop watchdog's exit), or `k-`, a `ulid`, `-`, and `n` (a kill switch's flatten), `n` a decimal from 0 to 999 with no leading zero. Any other form, a ULID with a suffix or a prefix in another case included, is refused | As `id` |
| `decimal` | §4.6, normalized on the way in | As `id` |
| `integer` | §4.4 | `schema` |
| `boolean` | `true` or `false` (§4.5) | `schema` |
| `timestamp` | §4.7. **Every instant is a timestamp**, never risk-clock seconds or an integer | As `id` |
| `ref` | `sha256:` and 64 lowercase hex: an artifact (§6.3) or a content hash. It is listed in `artifact_refs` (§3), so the object must be stored and re-hashes (§11 check 6) | As `id` |
| `a` \| `b` | One of the listed strings | As `id` |
| `T?`, `[T]` | `T` or `null`; an array of `T` | The array itself: `schema` |

A member not listed, or a listed member absent, is `schema` at that member; an absent member is
never read as `null` (§4.2), even where `null` would be valid. The first violation is
reported, in this order: unlisted members (in key order), then the listed members in the order given,
then the numbered consistency rules in number order, each rule's clauses in the order given (only on
a payload whose members are all well typed), then `artifact_refs` and `pii_refs` (§3), then the
subject rules 14 and 15 (`stream_mismatch`), then the copy rule 16 (also only on a well-typed
payload). Rule 10's second clause spans a batch, so it is checked only once every draft in the batch
passes these checks, and is reported at the first `IntentProposed` in the batch that breaks it.
Paths are dotted from the envelope (`payload.step_up.authenticated_at`).

**`StreamOpened`** on the agent stream. The schema of `StreamOpened` and of `KillSwitchActivated` is
chosen by the stream type of `stream_id`; the account stream's `StreamOpened` is unchanged, and its
`KillSwitchActivated` is §9.5's (v0.90).

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `agent` | |
| `workspace_id`, `agent_id` | `id` | The subject: rule 14 |

**Who opens it** (v0.66, [DEC-2039](../project/decisions/DEC-2039.md)). The agent's runtime, as an
`agent` actor, when it first finds the stream empty. Or a workspace service that takes the owner's
Stop of an agent no process ran, as its own actor: that `StreamOpened` is the stream's **claim**
(§9.21), and nothing follows it (rule 246). A claim is told by `actor.kind` alone, never by which
process wrote it, so a runtime's restart or a second launch of its slot resumes a stream an `agent`
actor opened. A runtime that finds a claim when it starts appends nothing and sends nothing, and
exits as `stream_claimed`; so does one whose own `StreamOpened` at head 0 the journal answers
`HeadMismatch` because a claim landed first, since it sends nothing before its stream is open. A
claim takes no epoch, so a `Fenced` answer is another runtime's, and that exit stays `fenced`.

**`ObservationRecorded`**

| Member | Type | Meaning |
|---|---|---|
| `source` | `text` | The feed or connector the data came from |
| `instrument_id` | `text?` | `null` for data about no single instrument |
| `as_of` | `timestamp` | The data's cut-off |
| `data_ref` | `ref` | The observed data, stored as an artifact, never inline |

**`ModelOutputRecorded`** ([mandate spec §8.2](mandate.md#82-output)). An output is recorded as the
model gave it; one the agent does not use carries the reason in `ignored`.

| Member | Type | Meaning |
|---|---|---|
| `model_id`, `model_version` | `text` | As the output states them |
| `content_hash` | `ref` | The model's content hash ([mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97)); the content object is an artifact |
| `instrument_id` | `text` | |
| `as_of`, `expires_at` | `timestamp` | |
| `direction` | `text` | As given; a direction v1 does not allow is recorded and ignored |
| `conviction`, `confidence` | `decimal` | As given |
| `horizon_s` | `integer` | Seconds |
| `thesis_ref` | `ref?` | The thesis, as an artifact |
| `evidence` | `[ulid]` | Event IDs, in the order given |
| `invalidation` | `text?` | |
| `thesis_id`, `lineage_id` | `id?` | Research agent only: rule 13 |
| `ignored` | `not_pinned` \| `model_withdrawn` \| `output_limits` \| `not_in_universe` \| `direction_not_allowed` \| `horizon_mismatch` \| `revision_without_predecessor`, or `null` | `null` when the output is used. Otherwise why not: identity differs from the pinned values (§8.2), the version was withdrawn (§8.1), an LLM output broke the output limits (§8.1), the instrument is outside the working universe (§8.2), or the first three §8.5 checks |

**`DecisionMade`** (mandate spec §6.2, §8.3): one proposal and its outcome. The action members are
`IntentProposed`'s.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `side` | `buy` \| `sell` | Rule 1 |
| `type` | `limit` \| `market` | Rules 2 and 3 |
| `tif` | `day` \| `gtc` \| `ioc` | [Trading spec §5.1, §5.2](trading-domain.md#51-v1-order-policy-dec-29-dec-37) |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | Rule 2 |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` | The proposer's label: the order builder's, a goal completion's or a removed instrument's (`exit_origin`), or the risk engine's `trim_to_target`. The gate assigns the purpose it enforces ([trading spec §9.1](trading-domain.md#91-evaluation-order-and-reason-codes)) |
| `exit_origin` | `signal` \| `goal_completion` \| `removed_instrument`, or `null` | Which of the three origins [mandate spec §6.1](mandate.md#61-purposes)'s purpose table gives a `discretionary_exit`: the order builder's signal exit (§8.3 step 2), a goal's completion (§3.1), or a removed instrument (§2.3, including an expired or invalidated thesis, §8.6). `null` for every other purpose: rule 8 |
| `exit_conviction`, `buy_conviction`, `combined_score` | `decimal?` | c, b, and s of §8.3 step 1, for a decision a §8.3 evaluation produced (an opening, an increase, or a signal exit); `null` for every other: rule 8 |
| `outputs_used` | `[ulid]` | The `ModelOutputRecorded` event IDs of the fresh outputs combined; empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `model_weights` | `[{key: text, value: decimal}]` | The weight of **every** configured model, fresh or not (§4.1); empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `clips_applied` | `[max_order_usd` \| `position_cap` \| `gross_exposure_cap` \| `target_qty` \| `max_spend_usd` \| `max_avg_price]` | Each §8.3 bound that reduced the proposal (the position cap is cap − MV − working); empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `dry_run` | `allow` \| `deny` \| `defer` | The gate dry run: rules 4 to 6 |
| `reason_code` | `id?` | The gate's reason code (trading spec §9.1): rule 4 |
| `autonomy` | `auto` \| `ask` \| `deny`, or `null` | The §6.2 classification, reached only after an `allow`: rules 5 and 7 |
| `ask_suppressed` | `budget` \| `skipped_today` \| `recent_timeout`, or `null` | Why an `ask` was classified but not asked, with the reasons and precedence of [mandate spec §6.4](mandate.md#64-approvals) ("Asking is bounded"; [DEC-156](../project/04-decision-log.md#decisions) item 5). `null` for every other decision, including an `ask` that was asked. Non-null only when `autonomy` is `ask` and no approval was requested: rule 7 |
| `decided_by` | `text?` | What decided `autonomy` (mandate spec §6.2), one label of: `builtin_risk_reducing` (step 3), `rule:<id>` or `default` (step 4), `delegation:<id>` (step 4a), `admission_ceiling` (step 5), `client_ceiling` (step 5a), `review_ceiling` (step 5b), `policy_overlay` (step 5c), with `<id>` an `id`; the same labels as the approval content's `decided_by` ([mandate spec §6.4](mandate.md#64-approvals)). `null` exactly when `autonomy` is: rule 5 |
| `delegation_id` | `id?` | The delegation that lifted an `ask` to `auto` (mandate spec §6.5, [DEC-181](../project/04-decision-log.md#decisions)); delegation usage is counted from these events. Non-null exactly when `decided_by` is `delegation:` and this id: rule 7 |
| `requested_by` | `agent` \| `owner` \| `client` | Who asked for the order (mandate spec §6.2 step 5a, [DEC-185](../project/04-decision-log.md#decisions)): the order builder, the owner through the web app or CLI, or an owner-connected client, set from the authenticated channel and never from the request's content |
| `client_id` | `id?` | The connected client that asked, by the id it was connected under ([DEC-141](../project/04-decision-log.md#decisions)); non-null exactly when `requested_by` is `client`: rule 7 |

**`DecisionMade` version 3** (v0.85, [DEC-2388](../project/decisions/DEC-2388.md) item 10): the
cell's runtime's advisory dry run over the journaled facts, which defers to the executor's binding
gate each check whose facts it cannot see ([DEC-2378](../project/decisions/DEC-2378.md) item 11).
Its `config_refs` are version 1's. Its payload is version 1's members, then:

| Member | Type | Meaning |
|---|---|---|
| `deferred_to_binding` | `[account_exposure` \| `agent_equity` \| `agent_mode` \| `conduct_history` \| `day_trade_budget` \| `eligibility` \| `fee_reservation` \| `halt` \| `liquidity_floor` \| `other_orders` \| `participation` \| `quote_ask` \| `reentry_cooldown` \| `related_accounts]` | The checks, or parts of checks, the dry run left to the binding gate, never reported passed: the four DEC-2378 item 11 skips (`eligibility`, `liquidity_floor`, `participation`, `day_trade_budget`) and the ten parts DEC-2388 item 8 names. Rule 269 |
| `liquidity` | `{prior_close: decimal, median_dollar_volume_20d: decimal, adv_20d: decimal, trailing_5m_volume: decimal, read_at: timestamp}?` | The stored bars' figures the dry run evaluated the liquidity floor and participation on, and the risk clock it read them at ([DEC-2274](../project/decisions/DEC-2274.md) item 3); `null` when it read none. Rule 270 |

A version 3 is written by a dry run that defers a check on an allowed opening, or that read the
figures for an opening it denied; any other decision is written at version 1, or version 2 where
its stricter references are registered. A writer that holds version 2's references and must also
defer a check has no version yet: it is an accepted gap until a later version combines the two
(DEC-2388 item 10). Every reader of version 1 reads version 3 as version 1 and ignores the two
members. The executor's
binding gate evaluates every check named here: the names never relax it.

**`IntentProposed`**: exactly the `IntentReceived` vector's intent fields less `intent_id`, which is
this event's `event_id` (§2), and `agent_id`, which is the stream's. The executor's `IntentReceived`
copies the members and adds those two. Its `causation_id` is one of three events (rule 10): the
`DecisionMade` whose action members it repeats exactly; for an owner's exit of one instrument, the
`OwnerExitRequested`; or after an approval's grant, the `ApprovalRevalidated` with result `act` that
precedes it in the same batch (§9, [mandate spec §6.4](mandate.md#64-approvals)). Re-validation
never re-prices, re-sizes, or changes the order, so an approved intent repeats exactly the action
members bound in its approval's `ApprovalRequested` content object.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `side`, `type`, `tif`, `qty`, `limit_price` | As `DecisionMade` | Rules 1 to 3 |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` \| `owner_exit` | The proposer's label, as for `DecisionMade`; `protective` and the kill switch's flatten are never proposed intents |

**`AgentModeChanged`** (mandate spec §5.9)

| Member | Type | Meaning |
|---|---|---|
| `from`, `to` | `normal` \| `exits_only` \| `paused` \| `stopped` | The effective mode before and after; `to` obeys rule 11 |
| `reason` | `restriction_changed` \| `awaiting_reconciliation` \| `owner_pause` \| `owner_resume` \| `owner_stop` \| `kill_switch` | |
| `lifecycle` | `normal` \| `paused` \| `stopped` | The deployment's own state after the change, set only by the owner's pause, resume, or stop. Restrictions lift independently (§5.9), so an owner's pause taken while a stricter restriction held must survive that restriction lifting on replay; this event is the only place the agent stream can record it (§1 principle 4) |

**`KillSwitchActivated`** on the agent stream (trading spec §5.5). The account stream's is closed in
§9.5 (v0.90, [DEC-2139](../project/decisions/DEC-2139.md)). Its members include `copied_from`:
- for a switch an executor applied from the agent stream, the `event_id` of that stream's
  `KillSwitchActivated`;
- `null` for a switch the executor originated.

Whether a copied switch was already applied is read from the account stream's own records, live
and after a replay ([DEC-2003](../project/decisions/DEC-2003.md) item 4).

| Member | Type | Meaning |
|---|---|---|
| `scope` | `agent` \| `connection` \| `workspace` | |
| `subject` | `id` | Which agent, connection, or workspace: rule 15. "Touches only its scope" (`AGENTS.md` rule 13) is checkable only if the event names it |
| `initiator` | `owner` \| `risk_limit` \| `platform_operator` | |
| `mode_event` | `ulid?` | The `AgentModeChanged` this switch wrote, or `null` if it changed no mode. The executor's `AgentModeApplied` copy carries it as `causation_id` (§2), which is how this stream's fold knows the switch was applied, from its own events (§1 principle 4) |

**`OwnerExitRequested`** (trading spec §5.5, mandate spec §5.10, §6.1): the owner's close of a
position, or the owner's kill switch. Every one records who asked and what step-up they gave,
whether or not a bid was confirmed. A displayed bid is confirmed only where trading spec §5.5 needs
one (an equity sold outside the regular session); without confirmation, equity sells wait for the
regular session. `step_up_status` says which case of
[DEC-158](../project/04-decision-log.md#decisions) option (c) applied: a kill switch without valid
step-up still stops the agent and flattens as an automated flatten does, and only the owner-exit
privileges beyond that need `valid`. A correct writer can meet every rule here for every owner
exit, whatever its step-up: no member is required that only valid step-up or a confirmed bid would
supply, and a confirmed bid with `absent` or `stale` step-up is valid, recorded as given, and
unlocks nothing while the exit proceeds ([mandate spec §6.1](mandate.md#61-purposes), "Owner
controls and step-up": owner exit and kill switch). So no step-up state can hold a
reduction (`AGENTS.md` rule 13); the test vectors' `valid_drafts` hold these cases.

| Member | Type | Meaning |
|---|---|---|
| `scope` | `instrument` \| `agent` \| `connection` \| `workspace` | |
| `subject` | `id` | The instrument ID, or which agent, connection, or workspace: rule 15 |
| `confirmed` | `boolean` | Whether the owner confirmed a displayed bid: rule 12 |
| `bid`, `bid_size` | `decimal?` | The displayed bid and bid size the owner confirmed |
| `floor` | `decimal?` | The confirmed floor price, below which the exit price ladder never prices |
| `user` | `text` | The owner who gave the instruction (opaque), on every owner exit and owner kill switch |
| `step_up_status` | `valid` \| `absent` \| `stale` | The step-up at the moment the owner committed the command ([DEC-156](../project/04-decision-log.md#decisions) item 8): `valid` evidence, none (`absent`), or evidence older than its freshness window (`stale`): rule 12 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | The step-up evidence, exactly when `step_up_status` is `valid`: rule 12 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

1. `DecisionMade`, `IntentProposed`: `side` is `buy` exactly when `purpose` is `open` or `increase`
   (long-only, no short sales) — `payload.side`.
2. `limit_price` is non-null exactly when `type` is `limit` — `payload.limit_price`.
3. `type` `market` never has purpose `open` or `increase` (`AGENTS.md` rule 12, trading spec §5.1) —
   `payload.type`.
4. `DecisionMade`: `reason_code` is `null` exactly when `dry_run` is `allow` — `payload.reason_code`.
5. `DecisionMade`: `autonomy` is non-null exactly when `dry_run` is `allow`; no classification runs
   after a `deny` or `defer` (mandate spec §6.2) — `payload.autonomy`; `decided_by` is non-null
   exactly when `autonomy` is, and is then `builtin_risk_reducing` exactly when `purpose` is neither
   `open` nor `increase` — `payload.decided_by`; and a non-null `decided_by` is one of the member's
   labels — `non_canonical` at `payload.decided_by`.
6. `DecisionMade`: `defer` only for `discretionary_exit` — `payload.dry_run`.
7. `DecisionMade`: a non-null `autonomy` is `auto` unless `purpose` is `open` or `increase` (built-in
   AUTO, §6.2 step 3) — `payload.autonomy`; and `ask_suppressed` is non-null only when `autonomy` is
   `ask` — `payload.ask_suppressed`. So a suppression only ever skips an opening or an increase,
   never an exit. That no `ApprovalRequested` names a suppressed decision is checked when the
   approval events' schemas close here. Then: `delegation_id` is non-null exactly when `decided_by` is
   `delegation:` followed by it, and then `autonomy` is `auto` (mandate spec §6.2 step 4a, MI-26) —
   `payload.delegation_id`; `client_id` is non-null exactly when `requested_by` is `client` —
   `payload.client_id`; a decision `requested_by` `client` with purpose `open` or `increase` is
   never `auto`, whatever decided it (the client ceiling, mandate spec §6.2 step 5a, MI-30) —
   `payload.autonomy`; and `decided_by` is `client_ceiling` only when `requested_by` is `client` and
   `autonomy` is `ask` — `payload.decided_by`; and `decided_by` is `review_ceiling` only when
   `autonomy` is `ask` (the review ceiling, mandate spec §6.2 step 5b, MI-32) —
   `payload.decided_by`; and a decision `decided_by` `policy_overlay` is never `auto` (the policy
   overlay, mandate spec §6.2 step 5c and §4.3, which only narrows an `auto` to an `ask` or denies
   an opening or increase under [DEC-534](../project/decisions/DEC-534.md)) — `payload.decided_by`.
   So the record itself refuses a connected client's opening that ran unasked, a decision that
   claims the review ceiling and ran unasked, and one that claims the policy overlay and ran
   unasked.
8. `DecisionMade`: `exit_origin` is non-null exactly when `purpose` is `discretionary_exit` —
   `payload.exit_origin`. A decision was produced by a §8.3 evaluation exactly when `purpose` is
   `open` or `increase` or `exit_origin` is `signal`; `exit_conviction`, `buy_conviction`, and
   `combined_score` are each non-null exactly then, and otherwise `outputs_used`, `model_weights`,
   and `clips_applied` are also empty — the first offending, in that order. Every exit's writer
   holds what this needs: a signal exit has its evaluation's numbers, and a risk exit, a goal
   completion, or a removed instrument needs none, so the rule never holds an exit (`AGENTS.md`
   rule 13).
9. `DecisionMade`: `outputs_used` strictly ascending, `model_weights` keys strictly ascending by bytes,
   and `clips_applied` strictly in the table's order — `non_canonical` at the list.
10. `IntentProposed`: `causation_id` is non-null — `causation_id`; and when the `DecisionMade` it
    names is in the same `append` batch, its action members (`instrument_id`, `side`, `type`,
    `tif`, `qty`, `limit_price`, `purpose`, decimals compared by value) equal that decision's — on
    the `IntentProposed`, the first member that differs, in that order. Like any invalid draft, it
    refuses the whole batch (§5.1).
11. `AgentModeChanged`: `to` is at least as strict as `lifecycle` (`normal` < `exits_only` < `paused` <
    `stopped`) — `payload.to`.
12. `OwnerExitRequested`: `confirmed` is `true` exactly when `bid`, `bid_size`, and `floor` are all
    non-null — the first member, in that order, that disagrees; and `step_up` is non-null exactly
    when `step_up_status` is `valid` — `payload.step_up`. `confirmed` governs only the bid members:
    `user` and the step-up are recorded on every owner exit.
13. `ModelOutputRecorded`: `thesis_id` and `lineage_id` are `null` together — the one that is `null`.
13a. `ModelOutputRecorded` version 2: the canonical object named by
    `config_refs.model_registry` contains exactly one model whose `model_id`, `model_version`, and
    `content_hash` equal the payload's — `payload.model_version`; otherwise
    `config_ref_mismatch`.

**Subject rules** (reason `stream_mismatch`):

14. `StreamOpened`: `stream_id` equals `agent:{workspace_id}:{agent_id}` — `stream_id`.
15. `KillSwitchActivated`, `OwnerExitRequested`: scope `agent` names the stream's agent and scope
    `workspace` its workspace — `payload.subject`.

**Copy rule** (reason `schema`):

16. The agent runtime's copies of an owner command (§2) have a non-null `causation_id` —
    `causation_id`. The copies are every `OwnerExitRequested`, a `KillSwitchActivated` whose
    `initiator` is `owner`, an `AgentModeChanged` whose `reason` is `owner_pause`, `owner_resume`,
    or `owner_stop`, and every `OwnerCommandRefused`. `causation_id` is the `event_id` of the control stream's
    `OwnerCommandIssued` the copy was made from. That ID is from another stream, which §2 allows and
    which neither `append` nor §11's per-range checks resolve, since both read only this stream. On
    one agent stream a command is copied at most once into each of these event types: an owner's
    kill switch is copied as its `OwnerExitRequested` and its `KillSwitchActivated`, both naming the
    one command, and every other command has one copy: for a resume or a Stop, its
    `AgentModeChanged` or its `OwnerCommandRefused`, never both ([mandate spec §6.1](mandate.md#61-purposes)).
    The copy always has its command in hand, so the rule
    never holds a pause, a Stop, an owner exit, or a kill switch (`AGENTS.md` rule 13). A mode
    change that originates on the account stream, and the `AgentModeChanged` a kill switch writes,
    are not owner copies and are not covered here.
269. `DecisionMade` version 3 (v0.85): `deferred_to_binding` is strictly ascending by bytes —
     `non_canonical` at `payload.deferred_to_binding`; and it is non-empty exactly when `dry_run`
     is `allow` and `purpose` is `open` or `increase` — `payload.deferred_to_binding`. So an allowed
     opening never claims every check passed, and a denial or an exit names no deferral: an exit
     defers nothing (`AGENTS.md` rule 13).
270. `DecisionMade` version 3 (v0.85): `liquidity` is non-null only when `purpose` is `open` or
     `increase` — `payload.liquidity`. On an allowed opening, `liquidity_floor` and `participation`
     are both in `deferred_to_binding` or neither is — `payload.deferred_to_binding`; and `liquidity`
     is `null` exactly when they are — `payload.liquidity`. So figures are recorded exactly when
     both checks were evaluated on them, and a check is never named evaluated without its figures.

Two facts span events. `IntentProposed` repeats its `DecisionMade`'s action, which `append`
checks when both are in one batch (rule 10), and `mode_event` names this stream's
`AgentModeChanged` with reason `kill_switch`. Across batches `append` cannot see the other event,
so §11's per-range checks `intent_action_mismatch` and `mode_event_mismatch` verify both on the
stored chain, and `mandate journal verify` reports them.

### 9.2 Control-stream payload schemas ([DEC-261](../project/04-decision-log.md#decisions))

This subsection closes the payload schemas of the control-stream and account-stream records among
those `ValidationContext::from_journal` reads ([DEC-169](../project/04-decision-log.md#decisions)):
the control stream's `StreamOpened`,
`ConnectionEstablished`, `ConnectionRevoked`, `DisclosureAccepted`, `ConfigSnapshotRegistered`,
`MandateVersionCreated`, `MandateConfirmed`, `AgentDeployed`, and `AgentStopped`, and the account
stream's `AccountSnapshotRecorded`. It also closes `OwnerCommandRefused` on both streams that write
it ([DEC-291](../project/04-decision-log.md#decisions)). The fold also reads `UniverseChanged` and
`MandateVersionApplied` (account stream), which close in §9.3, and
`PlatformOperatorAction`, which stays open (below). For these events it replaces the "Key payload
fields" column of §9, and the required `config_refs` stay as §9 lists them. §9.1's types, its
absent-member rule, and its report order apply unchanged, with the rules below numbered on from
§9.1's. Each schema is `schema_version` 1; `AgentDeployed` also has versions 2 and 3 (§9.24), and
`MandateConfirmed` versions 2 and 3 (§9.9).

Every member comes from a writer that exists or from a clause that names it, and DEC-261 traces
each one. No member is inferred where neither exists. The test vectors' `control_stream` section
holds a hash-chained control stream from `StreamOpened`, with its artifacts; base drafts on the
account and agent streams; the `JournaledFact` each record maps to; an invalid draft for every rule;
and valid drafts for the cases a rule might be misread to refuse.

**No payload carries a credential or personal data** (`AGENTS.md` rules 6 and 7, §6.4). A connection
is named by its opaque ID and scopes, never a key, token, or account number, and a user by an opaque
ID. Every schema is closed, so a draft with any other member is refused as `schema` at that member.

**Not closed here.** `PlatformOperatorAction` stays open (**Proposed**, DEC-261 item 9). Its §9 row
lists five actions. Clauses name the members of only two of them: `model_withdrawn` names the model
and a reason (mandate spec §8.1), and `research_thesis_halt` an instrument and, optionally, the
research agent's pinned content hash ([DEC-100](../project/04-decision-log.md#decisions)). No clause
and no writer names the subject of an operator stop, the scope of the global kill switch, what an
acceptable-use action records, or what the row's "approval" is. So the schema closes when the
operator service is specified, not with a guess. Until then `append` refuses it as `unknown_schema`,
as it does today. This holds no risk reduction: an operator's stop or kill switch is applied by the
stream owners' own `KillSwitchActivated` and mode records (§2), not by this audit record. The other
control-stream events close with their own stories.

**Types**, beyond §9.1's:

| Type | Values | Refused as |
|---|---|---|
| `pointer` | A JSON Pointer (RFC 6901) into the mandate document: one or more `/`-prefixed reference tokens, with `~` only in `~0` or `~1`. It is never the empty pointer: a record names each path it means | Not a string: `schema`; otherwise `non_canonical` |
| `date` | `YYYY-MM-DD` (§4.7), a calendar date in years 1970 to 9999 | As `pointer` |
| `risk_clock` | A §4.7 timestamp on a whole second: the risk clock (§2, mandate spec §5.2). Never integer seconds | As `pointer` |

**`StreamOpened`** on the control stream.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `control` | |
| `workspace_id` | `id` | The subject: rule 25 |

**`ConnectionEstablished`**: the owner connected a broker account. FR-2.2 rejects a connection whose
permission check fails, so the record exists only once the check has passed and needs no result
member.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection, by the opaque ID mandates name it by ([mandate spec §3](mandate.md#3-structure)) |
| `broker` | `text` | The broker or venue |
| `environment` | `paper` \| `live` | The account the connection is, which V-001 matches against a mandate's `environment` |
| `scopes` | `[text]` | The scopes granted: rule 19 |

**`ConnectionRevoked`**

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |

**`DisclosureAccepted`** ([mandate spec §10](mandate.md#10-records-dec-51-dec-97), V-005)

| Member | Type | Meaning |
|---|---|---|
| `document` | `id` | Which disclosure, for example `leveraged_etp` |
| `version` | `ref` | The disclosure version's hash, the value a mandate's `leveraged_etp_disclosure_version` names. The document is stored, so the hash proves which text was accepted |
| `user` | `text` | The owner who accepted (opaque) |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: V-005 counts only an acceptance with step-up |

**`ConfigSnapshotRegistered`**: a configuration snapshot registered under its content hash. A
`config_refs` value names one of these.

| Member | Type | Meaning |
|---|---|---|
| `kind` | version 1: `fee_config` \| `trading_calendar` \| `settlement_calendar` \| `instrument_snapshot` \| `rule_set` \| `mandate_version` \| `model_version`; version 2 adds `policy_set` \| `model_registry`; version 3 adds `broker_profile` | §9's `config_refs` kinds. A signal model is `model_version`. Version 1's vocabulary stays closed |
| `content_hash` | `ref` | The snapshot. For a model, its content hash (mandate spec §8.1) |
| `model_id`, `model_version` | `text?` | A model's id and version, registered together with its hash (V-007): rule 21 |
| `params` | `[text]` | A model's declared parameters (V-007), empty for any other kind: rules 20 and 21 |
| `admits_instruments` | `boolean?` | Whether the model may admit instruments (mandate spec §8.4): rule 21 |

Journal spec v0.16 registers `ConfigSnapshotRegistered` schema version 2 with the same payload
members and consistency rules as version 1, plus the two new `kind` values. A writer uses version 2
for `policy_set` and `model_registry`; it may use either registered version for an older kind.
For either new kind, before a configuration reference is appended, its canonical object is stored
under `content_hash` and its object `kind` equals the registration payload's `kind`; older kinds
retain their existing object contracts. Rules 20 and 21 make the model-only members null or empty
for both new kinds.

Journal spec v0.19 registers `ConfigSnapshotRegistered` schema version 3 with the same payload
members and consistency rules as version 2, plus the `kind` value `broker_profile`. A writer uses
version 3 for `broker_profile`; it may use any registered version for an older kind. Rules 20 and 21
make the model-only members null or empty for it, and rule 21b binds its stored object.

**`MandateVersionCreated`** and **`MandateConfirmed`** carry, as members, the parts `JournaledFact`
reads: the version, the provenance per path, and the confirmed paths. The rest of each
[mandate spec §10](mandate.md#10-records-dec-51-dec-97) row is a record stored as an artifact and
named by its hash (`record_ref`). Mandate spec §10 stays the one source of what that record holds.
The document is stored too, under its own version (`mandate_version`, mandate spec §9.1). Both are
`ref`s, so each must be stored and must re-hash (§11 check 6). Replay can therefore prove which
document was confirmed and which record was shown. A bare ID or a path is refused as
`non_canonical`.

| Member | Type | Meaning |
|---|---|---|
| `mandate_version` | `ref` | `MandateVersionCreated`, `MandateConfirmed`: the version, the hash of the stored canonical mandate document |
| `provenance` | `[{path: pointer, source: user_stated \| user_entered \| template_structure \| platform_proposed \| platform_default}]` | `MandateVersionCreated`: each path's source (mandate spec §2.1): rule 17. Quoted spans and what was proposed are in the record |
| `confirmed_paths` | `[pointer]` | `MandateConfirmed`: the paths the owner confirmed; a path confirms itself and every path under it: rule 18 |
| `record_ref` | `ref` | Both: the rest of the event's mandate spec §10 row, as a stored record |

**`AgentDeployed`** ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)). The connection,
environment, allocation, and pinned instruments the deployment runs under are the document's. They
are read from the stored document that `mandate_version` names, never restated here. Version 2
(§9.24, v0.54) restates the connection alone, for a reader that holds only the journal, and rule
200 binds it to the document's. Version 3 (§9.24, v0.84) adds the passkey step-up evidence the
record also holds, for §9.32's rule 253.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `mandate_version` | `ref` | The version deployed: rule 22 |
| `record_ref` | `ref` | The rest of the row: the go-live screen and UI build, the backtest and paper-run IDs and the legend and disclosure versions shown, the approving users, and the step-up evidence |

**`AgentStopped`** ([mandate spec §5.7, §5.10](mandate.md#57-lifetime-loss-floor-dec-44-dec-55))

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `connection_id` | `id` | The connection whose loss carry the loss joins |
| `reason` | `goal_complete` \| `profit_stop_reached` \| `end_date` \| `owner_stop` | §9's reasons |
| `retired_on` | `date` | The America/New_York date the agent retired. §5.7's 90-day carry counts from it |
| `loss_added` | `decimal` | The net dollar loss max(0, N − E) at retirement added to the carry: rule 23 |

Its writer retires an agent that ran only once the account stream's fold shows no position
attributed to the agent and no working order of its own, working being every non-terminal state of
trading spec §5.7, `Unknown` included (mandate spec §3.1, [DEC-2344](../project/decisions/DEC-2344.md)). The deployment manager stops the agent's
runtime, and its account's executor once every agent of the account is retired (§9.21), on this
record alone, so nothing of the agent's may be left for a later kill switch to reach. An agent no
process ran holds nothing, and its retirement (§9.21, DEC-2039) reads no fold.

**`AccountSnapshotRecorded`** on the account stream: the broker's account as reconciliation reads it
([trading spec §11](trading-domain.md#11-reconciliation)). The members are the executor's. The last three record the
cash comparison, which only a reconciliation that had a base to compare against makes, and are
otherwise `null`. This is version 1; version 2 (§9.35) adds the account's day-trading facts.

| Member | Type | Meaning |
|---|---|---|
| `status`, `crypto_status` | `text` | As the broker reports them |
| `trading_blocked`, `account_blocked`, `trade_suspended_by_user` | `boolean` | |
| `multiplier` | `integer` | |
| `equity`, `cash`, `buying_power`, `non_marginable_buying_power`, `accrued_fees` | `decimal` | |
| `model_cash` | `decimal?` | The cash the executor's model expected |
| `cash_band` | `decimal?` | The tolerance |
| `cash_in_band` | `boolean?` | Whether the broker's cash fell inside it: rule 24 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen. The executor writes it on every account-stream event and folds each at it, so a snapshot without one could not be replayed ([DEC-302](../project/04-decision-log.md#decisions)). `append`'s check that it never decreases (§2) applies |

**`OwnerCommandRefused`** on the agent and account streams ([mandate spec §6.1](mandate.md#61-purposes),
DEC-291). The owner input it refused is its `causation_id`: rule 27.

| Member | Type | Meaning |
|---|---|---|
| `command` | `resume` \| `stop` \| `acknowledge` | Rule 26 |
| `reason` | `step_up_missing` \| `step_up_stale` \| `step_up_reused` \| `step_up_method` \| `not_independent` \| `trigger_unknown` \| `trigger_superseded` \| `request_invalid` | The last four only on the account stream: rule 28. The last three are §9.28's ([DEC-2509](../project/decisions/DEC-2509.md)) |
| `effective_at` | `timestamp` | The effective time the command was judged at. A §4.7 timestamp, never risk-clock seconds (§9.1's types). This supersedes DEC-291 item 1's whole second for this member (DEC-261 item 7) |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

17. `MandateVersionCreated`: `provenance` strictly ascending by `path` bytes, so no path is listed twice
    (`non_canonical` at `payload.provenance`).
18. `MandateConfirmed`: `confirmed_paths` strictly ascending by bytes (`non_canonical` at
    `payload.confirmed_paths`).
19. `ConnectionEstablished`: `scopes` strictly ascending by bytes (`non_canonical` at `payload.scopes`).
20. `ConfigSnapshotRegistered`: `params` strictly ascending by bytes (`non_canonical` at
    `payload.params`).
21. `ConfigSnapshotRegistered`: `model_id`, `model_version`, and `admits_instruments` are non-null
    exactly when `kind` is `model_version`, and `params` is empty when it is not. Reported at the first
    offending member, in that order.
21a. `ConfigSnapshotRegistered` version 2: `policy_set` and `model_registry` are valid kinds.
    The canonical object stored under `payload.content_hash` has a `kind` equal to
    `payload.kind`; absence is `missing_artifact` at `payload.content_hash`, and a kind mismatch is
    `config_ref_kind` at `payload.kind`. Version 1 refuses either new kind as `non_canonical` at
    `payload.kind`. §11 check 6 independently verifies that the stored bytes re-hash to their
    address and reports `artifact_mismatch`.
21b. `ConfigSnapshotRegistered` version 3: `broker_profile` is a valid kind. The canonical object
    stored under `payload.content_hash` has a `kind` equal to `payload.kind`; absence is
    `missing_artifact` at `payload.content_hash`, and a kind mismatch is `config_ref_kind` at
    `payload.kind`. Versions 1 and 2 refuse it as `non_canonical` at `payload.kind`. §11 check 6
    independently verifies that the stored bytes re-hash to their address and reports
    `artifact_mismatch`.
22. `AgentDeployed`: `mandate_version` equals `config_refs.mandate_version` (`payload.mandate_version`).
    A missing ref is already `missing_config_ref`.
23. `AgentStopped`: `loss_added` ≥ 0 (`payload.loss_added`). The loss carried is never negative
    (mandate spec §5.7).
24. `AccountSnapshotRecorded`: `cash_band` and `cash_in_band` are `null` exactly when `model_cash` is.
    Reported at the first that disagrees. When all three are present, `cash_band` ≥ 0
    (`payload.cash_band`), and `cash_in_band` is `true` exactly when |`cash` − `model_cash`| ≤
    `cash_band` (`payload.cash_in_band`). A snapshot's flag can never contradict its own numbers.

**Subject rules** (reason `stream_mismatch`):

25. Control-stream `StreamOpened`: `stream_id` equals `ctl:{workspace_id}` (`stream_id`).
26. `OwnerCommandRefused`: `command` is `acknowledge` on the account stream, and `resume` or `stop` on
    the agent stream (`payload.command`), or at version 2 also `lift_hold`, which is registered on the
    agent stream only (§9.11). The executor refuses acknowledgments, and the runtime
    refuses resumes and Stops (§2).
28. `OwnerCommandRefused` with reason `not_independent`, `trigger_unknown`, `trigger_superseded`, or
    `request_invalid` is on the account stream (`payload.reason`): only an acknowledgment names a
    trigger and a request (§9.28). `trigger_unknown` and `trigger_superseded`, like the `step_up_*`
    reasons, apply to every acknowledgment. `not_independent` and `request_invalid` apply only
    under `independent_approval_required`
    ([mandate spec §5.8, §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)). A resume or Stop is
    refused only for its step-up.

**Copy rule** (reason `schema`):

27. `OwnerCommandRefused`, on either stream, has a non-null `causation_id` (`causation_id`). On the
    agent stream that is the `OwnerCommandIssued` (rule 16 already requires it there), and on the
    account stream the control stream's `OwnerAcknowledged` (§2). The refusal is written from the
    input it refuses, so the rule never holds one.

**The mapping to `JournaledFact`** ([DEC-169](../project/04-decision-log.md#decisions)). Each record
maps to one fact, and the test vectors list them. `AccountSnapshotRecorded` does not name its
connection. Which connection an account stream belongs to is journaled by version 2 of
`ConnectionEstablished` (§9.8, DEC-800, closing DEC-261 item 10); the mapping still takes it as an
argument from its owner, who reads it from that binding. `ValidationContext` takes
what no event carries the same way.

| Record | Fact | From |
|---|---|---|
| `ConnectionEstablished`, `ConnectionRevoked` | `ConnectionEstablished`, `ConnectionRevoked` | `connection_id`, and for the first `environment` |
| `DisclosureAccepted` | `DisclosureAccepted` | `version` |
| `ConfigSnapshotRegistered` of kind `model_version` | `ModelRegistered` | `model_id`, `model_version`, `content_hash`, `params`, `admits_instruments`; other kinds map to none |
| `MandateVersionCreated` | `MandateVersionCreated` | `mandate_version`, `provenance` |
| `MandateConfirmed` | `MandateConfirmed` | `mandate_version`, `confirmed_paths`, at every version |
| `AgentDeployed` | `AgentVersionActive` | `agent_id`; `connection_id`, `environment`, `capital.allocation_usd`, and `universe.pinned_instruments[].asset_id` of the stored document `mandate_version` names. A document that is not stored maps to none, and the mapping refuses it. Versions 2 and 3 map the same, from the same document; one whose `connection_id` differs from the document's is refused (§9.24 rule 200) |
| `AgentStopped` | `AgentStopped` | `agent_id`, `connection_id`, `retired_on`, `loss_added` |
| `AccountSnapshotRecorded` | `AccountSnapshot` | `equity`, with the account stream's connection given |

### 9.3 Account-stream risk-state records ([DEC-403](../project/decisions/DEC-403.md))

This subsection closes the payload schemas of the account stream's `MandateVersionApplied` and
`UniverseChanged`, the two records `ValidationContext::from_journal` reads that §9.2 left to their
own story ([DEC-303](../project/decisions/DEC-303.md) item 6). For these two events it replaces the
"Key payload fields" column of §9, as §9.1 and §9.2 do for theirs. §9.2's conventions apply unchanged:
§9.1's and §9.2's types, the absent-member rule, the report order, the required `config_refs` as §9
lists them, and the rules numbered on from §9.2's. Each schema is `schema_version` 1. Both are risk
inputs, so each carries `risk_clock` (§2).

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `asset_id` | Mandate spec §3's asset ID: `8-4-4-4-12` lowercase hexadecimal with hyphens, the form `AssetId` parses. Uppercase is refused, never folded, so one asset has one spelling ([DEC-404](../project/decisions/DEC-404.md) item 9) | Not a string: `schema`; otherwise `non_canonical` |

Every member comes from mandate spec §5.10's row, from a writer that exists, or from §2, and DEC-403
traces each one: `mandate-spec`'s risk fold writes `MandateVersionApplied`'s result, rejection
reason, allocation change, and floor fraction, and `mandate-research` writes `UniverseChanged`'s
instrument, change, reason, thesis and lineage, and size. The test vectors' `risk_state` section
holds base drafts of both records on the account stream, the stored mandate documents they name (each
a valid mandate, with every version record stating the classification mandate spec §9.2 gives its two
documents, and every admission through a thesis naming an unpinned mandate), the
`JournaledFact` each maps to, an invalid draft for every rule, and valid drafts for the cases a rule
might be misread to refuse. No payload carries a credential or personal data: an agent and a
thesis are named by opaque IDs, and step-up evidence by its assertion.

**`MandateVersionApplied`** on the account stream ([mandate spec §2.2, §5.10](mandate.md#22-applying-a-new-version)):
a version took effect for an agent, or was refused at application.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The agent the version is for |
| `old_version` | `ref` | The version in force before |
| `new_version` | `ref` | The version applied, or refused |
| `classification` | `risk_increasing` \| `risk_reducing` \| `neutral` | Mandate spec §9.2's verdict of `new_version` against `old_version`. An invalid change never validates (V-031), so it never reaches application: rules 29 and 33 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | The owner's step-up evidence, as `DisclosureAccepted`'s: rule 29 |
| `result` | `applied` \| `rejected` | |
| `reason` | `increase_blocked_while_latched` \| `equity_below_exposure` \| `would_trigger_limit` \| `not_loosening` \| `waiting_period` \| `still_below_new_floor`, or `null` | Why it was refused (mandate spec §5.1, §5.7): rule 30 |
| `allocation_change` | `decimal?` | The signed dollar change in the allocation it applied (mandate spec §5.1): rule 30 |
| `max_loss_from_allocation` | `decimal?` | The fraction a floor-loosening version raised the floor to (mandate spec §5.7): rule 30 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`UniverseChanged`** on the account stream ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)):
an instrument was admitted to, or removed from, an agent's working universe.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The agent whose working universe changed |
| `instrument` | `asset_id` | The instrument's asset ID (mandate spec §3) |
| `change` | `admitted` \| `removed` | Rule 31 |
| `reason` | `thesis_admitted` \| `thesis_expired` \| `thesis_invalidated` \| `lineage_retired` \| `eligibility_lost` \| `operator_halt` \| `version_applied` | Mandate spec §5.10's reasons: rule 31 |
| `thesis_id`, `lineage_id` | `id?` | The thesis the change follows from, and its lineage: rule 32 |
| `universe_size_after` | `integer` | The working universe's size after the change |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

The executor writes `UniverseChanged` with its `causation_id` naming the agent-stream record it
copies: the `ThesisProposed` or `ThesisRevised` for `thesis_admitted`, and the `ThesisInvalidated`
(§9.22) for `thesis_invalidated`, which it copies after that record is stored, with the same
`instrument` (the record's `instrument_id`), `thesis_id`, and `lineage_id`, and the stream's agent as
`agent_id`. The two records are on two streams, so no rule checks the copy at append; the verifier
owns it (§9.22).

**Consistency rules** (reason `schema`; the path is the member named):

29. `MandateVersionApplied`: `step_up` is non-null when `classification` is `risk_increasing`
    (`payload.step_up`). Mandate spec §9.2: a risk-increasing version requires step-up.
30. `MandateVersionApplied`: `reason` is non-null exactly when `result` is `rejected`, and
    `allocation_change` and `max_loss_from_allocation` are `null` when it is. Reported at the first
    offending member, in that order. A refused version changes nothing.
31. `UniverseChanged`: `thesis_admitted` only admits; `thesis_expired`, `thesis_invalidated`,
    `lineage_retired`, `eligibility_lost`, and `operator_halt` only remove; `version_applied` does
    either (`payload.reason`). Mandate spec §8.5 admits only through a thesis or a version, and
    removes for the rest.
32. `UniverseChanged`: `thesis_id` and `lineage_id` are `null` together, reported at the one that is
    `null` (as rule 13). They are non-null for `thesis_admitted`, `thesis_expired`,
    `thesis_invalidated`, `lineage_retired`, and `operator_halt`, which each follow from a thesis
    (an operator halts per thesis, DEC-100), and `null` for an admission with `version_applied`,
    which follows from a pinned list and no thesis (`payload.thesis_id`). A removal with
    `version_applied` may name its thesis or not: DEC-121's pinning switch removes instruments the
    research agent had admitted through a thesis (mandate spec §9.2), and a pinned list's own
    removal has none. `eligibility_lost` may be either too: a pinned instrument can lose
    eligibility.
33. `MandateVersionApplied`: `classification` is `risk_increasing` when the record itself shows a
    risk-increasing change: an `applied` one with `allocation_change` greater than 0 or with a
    non-null `max_loss_from_allocation` (mandate spec §9.2's maximums row), or a `rejected` one with
    `reason` `increase_blocked_while_latched`, `waiting_period`, or `still_below_new_floor`, which
    the risk fold reaches only on an allocation increase or a floor raise (mandate spec §5.1, §5.7)
    (`payload.classification`). It reports after rules 29 and 30, so a record whose `step_up` is
    missing because its classification is wrong is reported here, at the classification. With rule
    29 it keeps a risk-increasing version from being journaled as applied without step-up.

**The mapping to `JournaledFact`**, as §9.2's:

| Record | Fact | From |
|---|---|---|
| `MandateVersionApplied` with `result` `applied` | `AgentVersionActive` | `agent_id`; `connection_id`, `environment`, `capital.allocation_usd`, and `universe.pinned_instruments[].asset_id` of the stored document `new_version` names, as for `AgentDeployed`. Applied or rejected, `old_version` and then `new_version` must each name a stored document that hashes to it; one that does not is refused, as malformed (`InvalidInput`) at that member: the context cannot be built, and nothing is skipped. Whatever its `result`, a record whose `classification` is not mandate spec §9.2's verdict of its two stored documents is refused the same way, at `classification` ([DEC-404](../project/decisions/DEC-404.md)) |
| `MandateVersionApplied` with `result` `rejected` | none | A refused version changes nothing: the record yields no fact and no error, once both documents are stored and its classification matches them |
| `UniverseChanged` | `UniverseChanged` | `agent_id`, `instrument`, and whether `change` is `admitted` |

### 9.4 Research-agent thesis records ([DEC-413](../project/decisions/DEC-413.md))

This subsection closes the payload schemas of the agent stream's `ThesisProposed` and
`ThesisRevised`, which §9.1 left to their own story. For these two events it replaces the "Key
payload fields" column of §9, as §9.1 to §9.3 do for theirs. §9.2's conventions apply unchanged:
§9.1's types and §9.3's `asset_id`, the absent-member rule, the report order, the required
`config_refs` as §9 lists them (`mandate_version` and `model_version`), and the rules numbered on
from §9.3's. Both events share
one schema, `schema_version` 1. The entry type follows the revision number, not the verdict, so an
ignored revision is still a `ThesisRevised` (rule 34).

A record is written for every thesis the admission judges, whether it is admitted, refused, or
ignored ([mandate spec §8.5](mandate.md#85-admission-and-removal-dec-97-dec-101-dec-103)): a refusal
admits nothing and is journaled, and an output the first three checks refuse is ignored and journaled
with its reason (mandate spec §8.2). So no rule refuses a record that a correct writer makes for a
thesis the admission judged, and the rules recompute from the record only the checks the record
itself decides: checks 1 to 3, and check 15 from the platform-derived corroboration (rules 36 and
37). The other checks read the mandate, the policy
overlay, the working universe, and facts no record carries, so they are the admission's, not this
schema's.

Every member comes from mandate spec §8.2's output fields, §8.4's thesis table and its
`ThesisProposed` sentence, or §9's row, and DEC-413 traces each one to `mandate-research`'s
`ThesisEntry` or to the writer that adds it. The test vectors' `research` section holds base drafts
of both records on a research agent's stream, the artifacts they name (the research agent's mandate,
which is the mandate reference cases' `research_equity` with its admitting model's content hash set
to the stored model content, and each thesis's prompt, response, evidence, and autopsy), an invalid
draft for every rule and member type, and valid drafts for the cases a rule might be misread to
refuse. Every thesis record the approved mandate reference cases journal passes these rules, and the
section lists them. The vectors also pin the report order between rule 34, its autopsy clause, rules
35 to 37, and rule 38. No payload carries a credential or a personal-data field, and the prompt,
response, evidence, and autopsy are carried only by artifact. Two members are the model's own text in
the payload: `invalidation`, and the cited `evidence_sources`.

**`ThesisProposed`, `ThesisRevised`** on the agent stream ([mandate spec §8.4, §8.5, §8.6](mandate.md#84-the-research-agent-dec-97-adr-0002)):
one thesis the research agent proposed, and the admission's verdict on it.

| Member | Type | Meaning |
|---|---|---|
| `model_id`, `model_version` | `text` | The research agent's id and version, as its output states them |
| `content_hash` | `ref` | The research agent's content hash ([mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97)); the content object is an artifact: rule 38 |
| `thesis_id`, `lineage_id` | `id` | The thesis and its lineage ([mandate spec §8.6](mandate.md#86-thesis-lifetime-and-revision-lineages-dec-118-dec-111)) |
| `revision` | `integer` | 0 for a first thesis: rules 34 and 36 |
| `predecessor_thesis_id` | `id?` | The thesis this one revises, as the output states it: rule 36 |
| `autopsy_ref` | `ref?` | What the autopsy of the predecessor's failure found and what this revision changes, as an artifact ([DEC-111](../project/04-decision-log.md#decisions)): rule 34 |
| `instrument_id` | `asset_id` | The instrument's asset ID (mandate spec §3, §9.3's type), resolved by the platform |
| `asset_class` | `us_equity` \| `crypto` | From instrument reference data, never the thesis's claim |
| `direction` | `text` | As given; a direction v1 does not allow is recorded and ignored: rule 36 |
| `as_of`, `expires_at` | `timestamp` | Mandate spec §8.2's data cut-off and expiry: rule 36 |
| `horizon_s` | `integer` | Seconds: rule 36 |
| `conviction`, `confidence` | `decimal` | As given |
| `evidence_ref` | `ref?` | The evidence, as an artifact |
| `evidence_sources` | `[text]` | The sources cited, in the order given, duplicates kept. Text, not `id`, and in no required order, because check 14 refuses a source off the allowlist and the record keeps what was cited (mandate case MC-N07 cites `src.filings` before `src.anonymous_blog`) |
| `corroboration` | `independent_source` \| `market_data`, or `null` | How the platform corroborated the thesis, never what the model asserted; `null` when it did not: rule 37 |
| `invalidation` | `text` | The conditions that end the thesis before its horizon, as the model gave them |
| `allowlist_version` | `integer` | The source allowlist's version in effect ([DEC-101](../project/04-decision-log.md#decisions)) |
| `prompt_ref`, `response_ref` | `ref` | The research agent's prompt and response, as artifacts |
| `admitted` | `boolean` | Whether the admission admitted or renewed the instrument: rule 35 |
| `reason` | one of mandate spec §8.5's seventeen reasons (`direction_not_allowed` to `universe_full`), or `null` | The first failing check: rules 35 to 37 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

34. `ThesisProposed` has `revision` 0 and `ThesisRevised` a `revision` above 0 (`payload.revision`);
    and `autopsy_ref` is `null` on `ThesisProposed` (`payload.autopsy_ref`). Mandate spec §8.4:
    "a revision is `ThesisRevised`". No rule requires the autopsy on a revision, so a revision whose
    output omits it is still recorded (DEC-413 item 5).
35. `reason` is `null` exactly when `admitted` is `true` (`payload.reason`). Mandate spec §8.5: the
    first failure decides and is the journaled reason, and a success has none.
36. Checks 1 to 3 recomputed from the record (`payload.reason`): check 1 fails when `direction` is
    not `long`, check 2 when `expires_at` is not `as_of` plus `horizon_s` seconds (compared exactly,
    to the nanosecond), and check 3 when `predecessor_thesis_id` is non-null and `revision` is 0, or
    `null` and `revision` above 0. When one fails, `reason` is the first that fails, in that order;
    when none fails, `reason` is none of the three.
37. Check 15 recomputed from the record (`payload.reason`): when `corroboration` is `null`, the
    thesis fails check 15, so `reason` is a check numbered 15 or lower and never `null`,
    `lineage_retired`, or `universe_full`; when it is non-null, `reason` is not `no_corroboration`.
38. `config_refs.model_version` equals `content_hash` (`payload.content_hash`), as rule 22 binds
    `AgentDeployed`'s mandate version. A record whose `model_version` ref is missing is reported by
    §9's required references, not here.

**No mapping to `JournaledFact`.** No risk state folds from these records: the risk-state fold
reads the account stream's `UniverseChanged` (§9.3), which the executor writes with its
`causation_id` naming the agent stream's `ThesisProposed` or `ThesisRevised`, or, for a removal by
invalidation, the `ThesisInvalidated` (§9.22). The agent runtime's fold reads them, with §9.22's
records, for each instrument's current thesis and when it ended (mandate spec §8.6,
[DEC-2391](../project/decisions/DEC-2391.md)), so that the order builder sells a position whose
thesis ended without waiting for that copy. The lineage fold's reading of these records (mandate
spec §8.6) follows with its own story.

### 9.5 Account-stream executor records ([DEC-446](../project/decisions/DEC-446.md), [DEC-447](../project/decisions/DEC-447.md))

This subsection closes the payload schemas of the account stream's `IntentReceived`,
`GateDecided`, and `OrderSubmitted` at `schema_version` 2, `GateDecided` also at 3 (v0.109,
[DEC-2848](../project/decisions/DEC-2848.md)), and of the account stream's
`OrderRequestRecorded`, `ProtectionChanged`, `OwnerConfirmationReceived`, `AccountCloseRequested`,
`AccountRestrictionChanged`, `KillSwitchActivated`, `ExternalActivityIngested`, `RejectObserved`,
`OrderAbandoned`, the `ClockAdvanced` copy, and (v0.102) `AccountRestrictionLifted` at `schema_version` 1, and `FillApplied` at `schema_version` 2 ([DEC-360](../project/decisions/DEC-360.md), option (c); [DEC-2040](../project/decisions/DEC-2040.md); [DEC-2337](../project/decisions/DEC-2337.md); [DEC-2808](../project/decisions/DEC-2808.md); [DEC-2812](../project/decisions/DEC-2812.md)). For these events it replaces the "Key
payload fields" column of §9, as §9.1 to §9.4 do for theirs. §9.2's conventions apply unchanged:
§9.1's types and §9.2's, the absent-member rule, the report order, the required `config_refs` as
§9 lists them, and the rules numbered on from §9.4's.

The three version-2 schemas are their version 1's members with `risk_clock` last and nothing else
moved: version 1 is never edited (§8), stays registered, and keeps replaying as it always has,
pinned byte for byte by the version-3 vectors. Each version-2 record carries `risk_clock` because
§9.2 says the executor writes it on every account-stream event and folds each at it, and the fold
requires it (DEC-302 item 1). `StreamOpened` carries none — at seq 1 the stream has seen no tick —
and `OwnerCommandRefused` none, whose schema is shared with the agent stream
([DEC-447](../project/decisions/DEC-447.md)). `FillApplied` and `MarkUpdated` carry it already at
version 1 (DEC-81), and `MarkUpdated` version 2 is §9.30's; the rest of the account catalogue
closes with its own stories. At either version the protective prices are never members of `IntentReceived`
([DEC-446](../project/decisions/DEC-446.md) item 5): they move to the `intended`
`ProtectionChanged`, journaled beside the intent in its own batch, so the copy of the proposal
stays exact and replay restores the protection the opening was handed in with (`AGENTS.md` rule
13).

`OrderRequestRecorded` is the companion DEC-360 option (c) rules for: journaled immediately
before its `OrderSubmitted` in the same `append` batch, named by its `causation_id` (rule 45). It
carries what the fold rebuilds the exact request and the order's owner from, so a resubmission
after a confirmed absence re-sends the same body under the same id (trading spec §5.7), and the
restrictions and the exit rules read the same agent and purpose after a restart. One batch writes
both, so a crash cannot leave one without the other (§5.1: a batch is refused whole).

`ProtectionChanged` is closed whole. A schema closes per record, not per action, and the fold
reads members across every action, so the `intended` action cannot close alone. The members are
the fold's reads, traced per member in [DEC-446](../project/decisions/DEC-446.md) item 6; no
member is inferred. Every member is present on every record (§4.2, §9.1's absent-member rule);
rules 41 to 44 say per action which are null or empty. A legacy record — written while the schema
was open, with `instrument` for `instrument_id`, `agent` for `agent_id`, or `orders` as space- or
comma-joined text — stays replayable: the fold's reads are unchanged, tolerance of name and form
included, and `append` is what tightens to the closed form from here on.

The test vectors' `account_stream` section holds a hash-chained account stream from
`StreamOpened` at seq 1, with at least one chain event per closed schema; the `intended` record
in its intent's batch; the companion and its order in the next; a `placed` and an
`unprotected_start` for replay; the fold oracle (`fold_oracle`) the chain replays to — the
restored protection and the request the companion rebuilds; an invalid draft for every rule and
each member-typing case; valid drafts for the cases a rule might be misread to refuse; and rule
45's valid and invalid batches. Its `protection_fraction` section holds `ProtectionChanged`
version 2: version-2 bases that are the chain's `placed` and `unprotected_start` with a null
fraction, the drafts that disclose one, and the drafts rule 44 and §9.1's decimal type refuse. The vectors stay version 3.

**`IntentReceived`** on the account stream at `schema_version` 2: §9's nine members, as the
version-1 schema registers them (DEC-174 item 5), and `risk_clock`. At both versions, and on
`GateDecided`'s, `intent_id` is §9.1's `intent` type.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` | `intent` | The intent: the agent stream's `IntentProposed` event (§2), or the executor's own exit's (trading spec §2.3) |
| `agent_id` | `id` | The agent the intent is for |
| `instrument_id` | `text` | |
| `side` | `text` | As proposed |
| `type` | `text` | `limit` for an intent with a limit; `market` for an exit proposed at the market, which only an exit may be (`AGENTS.md` rule 12; [DEC-2037](../project/decisions/DEC-2037.md) item 2): rule 245 |
| `tif` | `text` | As proposed, never the submission's (DEC-389 item 3) |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | Null exactly when `type` is `market`: rule 245 |
| `purpose` | `text` | As proposed |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rule** for `IntentReceived` at either version (reason `schema`; the path is the
member named):

245. `type` is `limit` or `market`, and `market` only for an exit: `purpose` is neither `open` nor
     `increase` (`payload.type`); then `limit_price` is non-null exactly when `type` is `limit`
     (`payload.limit_price`). Reported at the first that fails, in that order. An opening is a
     limit order (`AGENTS.md` rule 12), and an exit at the market carries no limit to journal
     ([DEC-2037](../project/decisions/DEC-2037.md) items 1 and 2).

**`GateDecided`** on the account stream at `schema_version` 2: the version-1 members and
`risk_clock`. The check id vocabulary is §9's, matching trading spec §9.1.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` | `intent` | The intent decided |
| `verdict` | `text` | `allow`, `deny`, `hold`, or `defer` |
| `reason_code` | `text?` | The gate's reason code (trading spec §9.1); `null` on `allow` |
| `data_profile` | `text` | The data profile the decision read |
| `quotes_used` | `[{instrument_id: text, bid: decimal, ask: decimal, as_of: timestamp, feed: text}]` | |
| `marks_used` | `[{instrument_id: text, price: decimal, source: text, kind: text}]` | |
| `checks` | `[{id: §9's check ids, result: text, inputs: {}, computed: {}}]` | Each check run, in trading spec §9.1's order |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`GateDecided`** on the account stream at `schema_version` 3 ([DEC-2848](../project/decisions/DEC-2848.md)):
version 2's members and three that the executor writes on its own decisions. Each of the three is
present (§4.2), `null` where the executor writes none. The executor writes version 3, and its fold
reads every version.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` … `checks` | | As version 2 |
| `sized_qty` | `decimal?` | A discretionary exit allowed and sized to what the rungs that exit ladders will still send leave of the position ([DEC-410](../project/decisions/DEC-410.md) item 3): the quantity it goes for, which the fold takes as the intent's: rule 313 |
| `parked` | `boolean?` | A ladder step held while no v1 session is open, which the fold neither gates again nor resends until the next open (DEC-260 (18)): rule 315 |
| `held_long` | `boolean?` | An exit still held past `max_intent_age_s`, journaled once with its hold's reason so the long hold is seen (the coordinator's ruling D2); a park is one from the start: rule 314 |
| `risk_clock` | `risk_clock` | As version 2 |

**Consistency rules** for `GateDecided` version 3 (reason `schema`; the path is the member named):

313. `sized_qty` is non-null only on an `allow`, and then above zero: the quantity the executor
     sized the exit to, whether or not a binding run bound the decision, which the fold applies to
     the intent on replay.
314. `held_long` is `true` or `null`, and `true` only on a `hold`.
315. `parked` is `true` or `null`, and `true` only on a `hold` whose `reason_code` is
     `session_closed` or `session_unknown` and whose `held_long` is `true`.

A park, which no gate run decides, carries the members every version requires as the executor's
own decision does: `data_profile` `account_stream_only`, empty `quotes_used` and `marks_used`, and
one `checks` entry, the `session` check `fail` with empty `inputs` and `computed`.

**`OrderSubmitted`** on the account stream at `schema_version` 2: the version-1 members and
`risk_clock`. Its `causation_id` names its `OrderRequestRecorded` (rule 45).

| Member | Type | Meaning |
|---|---|---|
| `client_order_id` | `text` | The broker's name for the order, derived from the intent (trading spec §2.3) |
| `attempt` | `integer` | The submission attempt under this id; 1 for the first |
| `instrument_id` | `text` | |
| `side` | `text` | |
| `type` | `text` | The order type submitted |
| `tif` | `text` | The time in force submitted |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | `null` for a market order |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`OrderRequestRecorded`** on the account stream at `schema_version` 1: the executor-only members
of the order that follows it (DEC-446 item 2).

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The order's owner: the restrictions and the exit rules read it |
| `intent_id` | `intent`? | The intent the order is for, when it is for one |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` \| `owner_exit` \| `protective` \| `flatten` | The order's purpose (trading-domain spec §6.1) |
| `extended_hours` | `boolean` | Whether the order may trade outside the regular session |
| `stop_price` | `decimal?` | A protective or stop order's trigger |
| `order_class` | `bracket` \| `oco`, or `null` | The protective legs' class: rule 39 |
| `take_profit` | `decimal?` | The take-profit leg: rule 39 |
| `stop` | `decimal?` | The stop leg: rule 39 |
| `rung` | `integer?` | The exit price ladder's rung, from 0; non-null exactly for a ladder's submission (trading spec §5.6): rule 40 |
| `at_floor` | `boolean?` | Whether the rung is the ladder's floor: rule 40 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for the companion (reason `schema`; the path is the member named):

39. `order_class` is null exactly when `take_profit` is, and `take_profit` null exactly when
    `stop` is, reported at the first offending member in the order `order_class`, `take_profit`,
    `stop`. The bracket's and the OCO's legs travel together: the fold rebuilds a class from both
    legs or from neither.
40. `at_floor` is null exactly when `rung` is (`payload.at_floor`). A ladder's rung carries its
    floor flag with it; there is no `laddered` member ([DEC-446](../project/decisions/DEC-446.md)
    item 2): `rung` non-null says a ladder rung, and a lone ladder's first rung writes `rung` 0.

**`OwnerConfirmationReceived`** on the account stream at `schema_version` 1: the owner's
confirmation of a displayed bid for an owner's exit ([DEC-2040](../project/decisions/DEC-2040.md)),
copied from the agent stream's `OwnerExitRequested` (§9.1), so that the exit may sell outside the
regular session and the exit price ladder never prices below the floor (trading spec §5.5). Like
the protective prices, it is never a member of `IntentReceived`, which stays the exact copy of the
proposal. One batch writes both records, so the fold reads the floor back after a restart.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` | `ulid` | The intent of the owner's exit it confirms, the one that follows from the `OwnerExitRequested`: rule 248 |
| `instrument_id` | `text` | The instrument confirmed, the intent's: rule 248 |
| `qty` | `decimal` | The quantity confirmed, the intent's; the confirmation never covers more: rules 247 and 248 |
| `bid` | `decimal` | The displayed bid the owner confirmed: rule 247 |
| `bid_size` | `decimal` | The displayed bid size the owner confirmed: rule 247 |
| `floor` | `decimal` | The price below which the exit never sells: rules 247 and 248 |
| `expires_at` | `integer` | The risk-clock second from which the confirmation no longer applies: the step-up's `authenticated_at` plus 300 s ([DEC-2040](../project/decisions/DEC-2040.md) item 7). At or after it, judged at the executor's journaled event time, the exit takes the unconfirmed path: rule 247 |
| `copied_from` | `ulid` | The `OwnerExitRequested` the confirmation was copied from, whose `step_up_status` was `valid` |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rule** (reason `schema`; the path is the member named):

247. `qty`, `bid`, `bid_size`, `floor`, and `expires_at` are each positive, reported at the first
     that is not, in that order. A zero or negative price or quantity confirms nothing, and a floor
     of zero would let the ladder price anywhere.

**`AccountCloseRequested`** on the account stream at `schema_version` 1 (v0.75,
[DEC-2337](../project/decisions/DEC-2337.md)): one request of the close-position of one instrument
under an account kill switch (trading spec §5.5). The executor journals it in its own batch, then
sends the broker's close-position for that instrument, and only for an instrument the switch's
cancel-all is confirmed to have cleared of every sell of ours
([DEC-2333](../project/decisions/DEC-2333.md) item 2). The fold reads it as sent: nothing re-places
protection in that instrument, and its unprotected interval ends with `unprotected_end` only once a
close is journaled and nothing is held. A request is never sent twice: a crash between the record
and the send, or a lost answer, leaves the instrument to a broker read taken after the record. Only
when that read shows the instrument still held and no sell of ours working in it does the executor
journal the next `attempt` and send again; every request is journaled before it is sent
([DEC-2337](../project/decisions/DEC-2337.md) item 3). The read is the executor's; `append` checks
rules 258 to 260.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | The instrument closed, as the executor names it: the same text the switch's `KillSwitchActivated` names in its `closes` or `deferred`, not a canonical asset id. Non-empty, as every `text` member (§9.1) |
| `qty` | `decimal` | The executor's held quantity when the request was journaled, informational: the close-position sells whatever the broker holds. Rule 258 |
| `kill_switch_event` | `ulid` | The account stream's `KillSwitchActivated` the close carries out: rule 259 |
| `attempt` | `integer` | 1 for the first request of the instrument under the switch, one more for each re-request: rules 258 and 260 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for `AccountCloseRequested` (reason `schema`; the path is the member named):

258. `qty` is positive (`payload.qty`), then `attempt` is at least 1 (`payload.attempt`), reported at
     the first that fails, in that order. A zero or negative quantity closes nothing, and a close
     never opens a short (`AGENTS.md` rule 12).
259. `causation_id` equals `kill_switch_event` (`causation_id`): the request is caused by the switch
     it carries out.
260. Account stream, one request per attempt: an `AccountCloseRequested` names a `kill_switch_event`,
     `instrument_id`, and `attempt` that no earlier `AccountCloseRequested` of the stream names
     together (`payload.attempt`), so `append` refuses a duplicate request and its second send never
     happens. It is a stream rule: `append` reads the stream's earlier records, and a range judges
     it from the stream's start.

**Checked by no rule at append**, though the account stream's `KillSwitchActivated` is closed (§9.5,
v0.90): that `kill_switch_event` names an earlier account-scope `KillSwitchActivated`
of the stream whose `closes` or `deferred` lists `instrument_id`.

**`KillSwitchActivated`** on the account stream at `schema_version` 1 (v0.90,
[DEC-2139](../project/decisions/DEC-2139.md)): a kill switch the executor applied on its account
(trading spec §5.5), journaled right after the `AgentModeApplied` that moved the agents it covers,
which is its `causation_id`, and before anything it sends. An agent-scope switch sells through the
executor's own exits; an account-scope switch sends the broker's cancel-all and then the
close-positions its `AccountCloseRequested` records carry out (§9.5, DEC-2337). The agent stream's
record is §9.1's and differs: it names its `mode_event`, and this one is named by its cause.

| Member | Type | Meaning |
|---|---|---|
| `scope` | `agent` \| `account` | What the switch reaches on this account. The executor applies these two scopes; a workspace switch is E7-4's and writes no record yet |
| `subject` | `id` | The agent, or the account's `account_ref` |
| `initiator` | `owner` \| `risk_limit` \| `platform_operator` | Who ordered it |
| `purpose` | `risk_exit` \| `owner_exit` | The purpose its sells carry: rule 281 |
| `cancels` | `[text]` | The client order ids it cancels by id, the orders whose cancel never lapses while they work ([DEC-2302](../project/decisions/DEC-2302.md) item 2): rule 294 |
| `closes` | `[text]` | The instruments it closes now, as the executor names them: rule 296 |
| `deferred` | `[text]` | The instruments it closes once they may close (an equity outside the regular session): rules 294 and 296 |
| `floor`, `bid`, `bid_size` | `decimal?` | The owner's confirmed displayed bid (trading spec §5.5), or `null` without one: rules 282 and 293 |
| `copied_from` | `ulid?` | The agent stream's `KillSwitchActivated` the executor applied, or `null` for a switch it originated (v0.59, [DEC-2003](../project/decisions/DEC-2003.md) item 4) |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for the account stream's `KillSwitchActivated` (reason `schema`; the path is
the member named), judged in this order and reported at the first that fails:

281. `purpose` is the initiator's: `owner_exit` for `owner`, `risk_exit` for `risk_limit` and
     `platform_operator` (`payload.purpose`), so an owner's switch is paced as an owner exit and every
     other as a risk exit (`AGENTS.md` rule 13).
282. `floor`, `bid`, and `bid_size` are null together (at the first of `bid` and `bid_size` whose
     nullness differs from `floor`'s): a displayed bid is confirmed whole or not at all.
293. A confirmed bid is the owner's: the three are null unless `initiator` is `owner`
     (`payload.floor`).
294. An `account` switch cancels by the broker's cancel-all and defers nothing: its `cancels` is
     empty (`payload.cancels`), then its `deferred` (`payload.deferred`).
295. `causation_id` is non-null (`causation_id`): the `AgentModeApplied` the switch applied, which
     the executor's fold and §2's copy rule read.
296. No instrument is in both `closes` and `deferred` (`payload.deferred`): it closes now or later,
     never twice.

**Checked by no rule at append**, as before: that an `agent` switch's `subject` is an agent of this
account, and that an `account` switch's is this stream's `account_ref`. Rule 260's request reads its
switch from the stream.

**`AccountRestrictionChanged`** on the account stream at `schema_version` 1 (v0.88,
[DEC-2137](../project/decisions/DEC-2137.md)): an account restriction the executor detected (trading
spec §7.3), journaled before anything else the executor sends or refuses, and, when it makes the
account state stricter, before the `AgentModeApplied` records that move every agent of the account. It carries the restriction's **cause**, one of a
closed list, so a restriction the broker never imposed is never journaled as the broker's, and causes
lift independently ([DEC-441](../project/decisions/DEC-441.md) item 23). No member names the broker's
account or holds the broker's text: a reject is `RejectObserved`'s, and the restriction does not name
it (its `causation_id` is `null`, since a run of 403s has no one reject to name). A restriction is
never lifted by another `AccountRestrictionChanged`: the record's values are restrictions only. A
cause lifts by `AccountRestrictionLifted` (v0.102, below), after the owner's acknowledgment and the
evidence §7.3's "Lifts when" names.

| Member | Type | Meaning |
|---|---|---|
| `restriction` | `closing_only` \| `blocked` | The account state it sets (trading spec §7.3): rule 279 |
| `cause` | `broker_reject` \| `broker_notice` \| `connection_unavailable` | Trading spec §7.3's cause: rows 2 and 3 a reject, rows 1 and 4 a status, flag, or notice, row 5 the connection: rule 280 |
| `reason_code` | `account_trading_blocked` \| `account_restricted` | §7.3's reason code, the one the agents' `AgentModeApplied` and the owner's alert carry: rule 279 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for `AccountRestrictionChanged` (reason `schema`; the path is the member named),
judged in this order and reported at the first that fails:

279. `restriction` is `blocked` exactly when `reason_code` is `account_trading_blocked`
     (`payload.reason_code`): §7.3's first row blocks the account, and every other row restricts it
     to closing only with `account_restricted`.
280. A `blocked` restriction's `cause` is `broker_notice` (`payload.cause`): only §7.3's first row, a
     status or flag the broker reports, blocks an account. A reject never blocks one, and an
     unavailable connection only restricts it to closing only (connections spec CN-6).

Together they admit exactly §7.3's rows: `blocked` with `broker_notice` and
`account_trading_blocked`, and `closing_only` with any cause and `account_restricted`.

**Every cause in force is on the stream** (v0.96, [DEC-2816](../project/decisions/DEC-2816.md)).
A cause is **in force** from the `AccountRestrictionChanged` that raised it, at the restriction that
record set, until an `AccountRestrictionLifted` names it (v0.102); the lift lifts it at every
restriction it was raised at. The executor journals a restriction whenever its
cause is not already in force at that restriction or a stricter one (`blocked` is stricter than
`closing_only`), whether or not the account state changes, so each cause §7.3 lifts on its own
evidence is on the stream for its lift to name. **The account state is the strictest restriction
any cause in force set**, never the last record's: every reader of the stream (the executor's fold,
the audit timeline, a read model) takes the strictest, so a `closing_only` record after a `blocked`
one leaves the account blocked. A record that leaves the account state unchanged moves no agent:
its `AgentModeApplied` would repeat the mode every agent already holds.

**Stream rule** on the account stream, judged by `append` over the stream before the batch and
the batch, after each draft's own rules (reason `schema`):

309. An `AccountRestrictionChanged` names a `cause` that no earlier `AccountRestrictionChanged` of
     its stream raised at the same `restriction` or a stricter one since the last
     `AccountRestrictionLifted` of that cause, if any (`payload.cause`): a cause in force is
     journaled once at each restriction, and again only when it restricts the account further, so
     the stream's causes in force are its records, each once per restriction. A cause lifted and
     raised again is journaled again (v0.102).

**`AccountRestrictionLifted`** on the account stream at `schema_version` 1 (v0.102,
[DEC-2824](../project/decisions/DEC-2824.md)): one cause of an account restriction lifted, as
trading spec §7.3's "Lifts when" says for it, at every restriction it was raised at. Causes lift
independently: a lift leaves every other cause in force. It is committed only after an
acknowledgment copy of `subject` `restriction` (§9.28) the executor accepted, and the evidence
below; its `causation_id` is that copy.

| Member | Type | Meaning |
|---|---|---|
| `cause` | `broker_reject` \| `broker_notice` \| `connection_unavailable` | The cause lifted: rule 310 |
| `acknowledgment` | `ulid` | The `event_id` of the accepted `OwnerAcknowledged` copy on this stream, as `causation_id`: rule 311 |
| `evidence` | `ulid` | The `event_id` of the record that completes the lift on this stream: rule 312 |
| `restriction` | `active` \| `closing_only` \| `blocked` | The account state the causes left in force set: rule 310 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**The evidence**, by cause:

- `broker_reject` and `broker_notice` (§7.3: "the owner acknowledges and the account is
  refreshed"): an `AccountStateObserved` after the acknowledgment copy whose `status` is `ACTIVE`
  and whose `trading_blocked`, `account_blocked` and `trade_suspended_by_user` are `false`, none of
  §7.3 row 1's conditions, and which is the latest `AccountStateObserved` before the lift. The
  executor reads the account again when it accepts the acknowledgment, and lifts in the batch of the
  first clean report. A report that still shows a row-1 condition lifts nothing; the acknowledgment
  stays accepted, and the next clean report lifts the cause. A report from before the
  acknowledgment never lifts it, and neither does a clean report that a later one contradicts.
- `connection_unavailable` (§7.3: "the connection's own condition clears, then the owner
  acknowledges"): a `ConnectionStateChanged` with reason `condition_cleared` after the
  `AccountRestrictionChanged` that raised the cause and before the acknowledgment copy, with no
  `ConnectionStateChanged` into `degraded` or `suspended` after it and before the lift. The
  connection's `acknowledged` state change, whose `causation_id` is the acknowledgment copy (rule
  61), comes after the copy and before the lift, in the lift's batch or an earlier one (§9.8), so
  a refused state change keeps the restriction. A reconnect alone lifts nothing.

**The agents follow the account state.** Agent restrictions are keyed by reason code
(`account_trading_blocked` for `blocked`, `account_restricted` for `closing_only`), and the
account entries the agents hold are those applied and not yet removed. A lift that loosens the
account state does two things in its batch, right after the lift:

1. it applies the state left's agent effect (§7.3), unless that entry is already held:
   `AgentModeApplied` for `*` to `exits_only` under `account_restricted` when `closing_only` is
   left;
2. it then removes every other account entry the agents hold: `AgentModeApplied` for `*` to
   `normal` under each such reason code, so a lift to `active` leaves no account entry, whichever
   restrictions the causes were raised at.

So no agent is looser than the account state at any record, and once every cause is lifted no
account entry is left behind. Removing an entry hands each agent back to the strictest restriction
it still holds, so an owner's pause, a kill switch, or a held run stays. A lift that leaves the
state unchanged moves no agent. Each record's agent effect follows it, in record order: a cause
raised again later in the lift's batch applies its own entry after the lift's removals. No owner
alert is sent for a lift: the owner's own acknowledgment caused it, and the agents' records say
what changed.

**Stream rules** on the account stream for `AccountRestrictionLifted`, judged by `append` over the
stream before the batch and the batch, after the draft's own schema, in this order (reason
`schema`):

310. The `cause` is in force: an earlier `AccountRestrictionChanged` of the stream raised it, and
     no `AccountRestrictionLifted` of it comes after that record (`payload.cause`). Then
     `restriction` is the strictest restriction of the causes still in force without this one, or
     `active` when none is (`payload.restriction`).
311. `acknowledgment` is the `event_id` of an earlier `OwnerAcknowledged` on the stream at
     `schema_version` 2 with `subject` `restriction` (`payload.acknowledgment`). That copy must
     meet both:
     - its `event` names an `AccountRestrictionChanged` of this `cause` on the stream, before the
       copy, after which no `AccountRestrictionChanged` of this `cause` and no
       `AccountRestrictionLifted` of it comes before this lift: §9.28's checks 1 and 2, held to the
       lift, so a cause the broker raised again after the acknowledgment is not lifted by it, and
       needs an acknowledgment of its own;
     - no `OwnerCommandRefused` with `command` `acknowledge` has the copy's `causation_id` as its
       own (§9.28: a refused acknowledgment lifts nothing; its refusal is the record right after
       the copy).

     A lift of the cause ends its record's standing, so an acknowledgment lifts once.
312. `evidence` is the `event_id` of a record of the stream that completes the lift
     (`payload.evidence`):
     - for `broker_reject` and `broker_notice`, an `AccountStateObserved` after the
       acknowledgment copy with `status` `ACTIVE` and `trading_blocked`, `account_blocked` and
       `trade_suspended_by_user` `false`, and no other `AccountStateObserved` between it and the
       lift;
     - for `connection_unavailable`, a `ConnectionStateChanged` with reason `condition_cleared`
       after the `AccountRestrictionChanged` the acknowledgment names and before the acknowledgment
       copy, with no `ConnectionStateChanged` whose `to` is `degraded` or `suspended` between it
       and the lift, and a `ConnectionStateChanged` with reason `acknowledged` whose `causation_id`
       is the acknowledgment copy between the copy and the lift.

`append` does not judge the copy's step-up or independence (§9.28 checks 3 and 4): the
executor judges those and refuses a failing copy by `OwnerCommandRefused`, which rule 311 reads.
That is the residual: `append` reads a copy as accepted when no refusal names it. The executor
commits each copy with its judgment, the refusal the record right after the copy (§9.28), so every
copy a lift can name has already been judged; a writer that broke that order could name an
unjudged copy, and only the executor's own judgment stands behind it, as for a limit's lift.
Nothing here holds an exit, a protective order, or a kill switch: a lift only adds risk, and a
refused one leaves the account restricted (`AGENTS.md` rules 3 and 13).

The restriction for an unavailable connection, `connection_unavailable`, and its distinct owner
alert are journaled by the account's executor (v0.107, [DEC-2826](../project/decisions/DEC-2826.md),
[DEC-2832](../project/decisions/DEC-2832.md)). The executor process counts the broker's answers per
class (connections spec §8.2): a lost answer is a network failure, and so is an unreadable one
while later answers keep the process up (DEC-85); a refused credential is an authorization
failure.
- **The restriction.** Once a count is at its threshold, the executor journals the restriction
  with its `OwnerAlertSent` in one batch, before it takes the next opening. When this makes the
  account state stricter, every agent's `AgentModeApplied` `exits_only` is in that batch too.
  Beside another cause at that restriction or a stricter one, only the cause and its alert are
  journaled (rule 309).
- **The state change.** In a later batch the executor journals the `ConnectionStateChanged` (§9.8):
  once per reason since the connection entered its state, or since the latest `condition_cleared`
  or `acknowledged`. A degrading cause while `suspended` journals no state change, and neither does
  anything before the stream's copy of `ConnectionEstablished` (rule 68); the restriction is
  journaled all the same.

Its lift is specified here.

**`ExternalActivityIngested`**, **`RejectObserved`** and **`OrderAbandoned`** on the account stream
at `schema_version` 1 (v0.93, [DEC-2803](../project/decisions/DEC-2803.md)): what the executor
journals of the broker's own activity it did not originate, of a reject the broker answered with, and
of an intent it gave up on. `append` refused all three as `unknown_schema` before, so the first
external order, broker reject or abandoned intent refused the executor's batch whole and stopped its
process. Each member the broker supplies is `text?`: a broker may send an empty string, which the
executor writes as `null` (§4.2), so a broker's empty field never refuses a batch. The members the
executor itself derives are never `null`.

`ExternalActivityIngested` is external activity (trading spec §7.1, §11): an open order the broker
lists that names no order of ours, or a fill that names none. `source` says which, and the members
the other source has are `null`.

| Member | Type | Meaning |
|---|---|---|
| `source` | `open_order` \| `fill` | What the broker reported: an open order in the snapshot, or a fill: rules 297 and 298 |
| `broker_order_id` | `text?` | The broker's id for the open order |
| `fill_id` | `text?` | The broker's id for the fill |
| `client_order_id` | `text?` | The client order id the open order carries, as the broker gave it, when it carries one that is not ours |
| `instrument` | `text?` | The open order's instrument, as the broker names it |
| `side` | `buy` \| `sell` \| `null` | The open order's side |
| `qty` | `decimal?` | The open order's quantity |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

The fill itself is journaled next, as `FillApplied` with its own members; this record says only that
it was unattributed.

`RejectObserved` is a reject the broker answered with (trading spec §7.3). The record holds the
broker's text as it came, which no notification ever carries (`AGENTS.md` rule 6); the restriction it
may set is `AccountRestrictionChanged`'s.

| Member | Type | Meaning |
|---|---|---|
| `http_status` | `integer` | The status the broker answered with |
| `code` | `text?` | The broker's reject code |
| `message` | `text?` | The broker's message |
| `client_order_id` | `text?` | The client order id the reject names, as the broker gave it; it may not be ours |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

`OrderAbandoned` is an intent the executor gave up on before or between submissions (trading spec
§5.7): its order goes no further, and its reservation is released.

| Member | Type | Meaning |
|---|---|---|
| `client_order_id` | `text` | The order's client order id, the one the executor derived from the intent: rule 303 |
| `intent_id` | `id` | The intent abandoned: an `IntentProposed` event, or the executor's own exit's id (`w-…`, `k-…`; trading spec §2.3) |
| `reason` | `intent_too_old` \| `gate_recheck` \| `released_to_owner` \| `unprotected_interval_limit` | Why: the intent outlived `max_intent_age` (§5.7); the gate's re-check after a confirmed absence denied it (§5.7); an exit found nothing left for it to sell beside a flatten, and is left to the owner with an `ExitStalled` alert; the instrument's unprotected interval reached its limit (§5.4) |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for `ExternalActivityIngested` (reason `schema`; the path is the member named), judged in this
order and reported at the first that fails:

297. `ExternalActivityIngested` with `source` `fill`: `broker_order_id`, `client_order_id`,
     `instrument`, `side` and `qty` are each `null`, in that order (`payload.<member>`).
298. `ExternalActivityIngested` with `source` `open_order`: `fill_id` is `null`, then `instrument`,
     `side` and `qty` are each non-null, in that order (`payload.<member>`). The broker's order id and
     client order id stay `text?`: a broker that sends them empty sends nothing to name.

`RejectObserved` has no rule: every combination of a reject's members is a reject a broker may
send. An abandonment's `client_order_id` is the executor's derivation from its intent, which trading
spec §2.3 states (v0.21, [DEC-2807](../project/decisions/DEC-2807.md)), and rule 303 binds the two
(v0.99, [DEC-2820](../project/decisions/DEC-2820.md)):

303. `OrderAbandoned`: `client_order_id` is `md-` followed by `intent_id`, exactly
     (`payload.client_order_id`, reason `schema`). An intent's own order is the one its id derives,
     the watchdog's and a kill switch's exits (`w-…`, `k-…`) alike, so an abandonment can name no
     other order: not another intent's, not a rung, protective order or replacement of its own.

**`FillApplied`** on the account stream at `schema_version` 2 (v0.94,
[DEC-2812](../project/decisions/DEC-2812.md)): one broker fill applied to the account (trading spec
§6.1). Version 2 is version 1's members, in version 1's order, with `client_order_id` `text?`, and
nothing else moved. It is `null` for a fill that names no order of ours, which trading spec §6.1 and
§7.1 make an unattributed fill of the account. Version 1 refused every such fill, so the
executor's batch for the first fill it did not originate was refused whole (DEC-2805). Version 1
stays registered and replays as it always has. `LateFillApplied` stays at version 1: a late fill is
of an order of ours, so it always names one. The executor writes version 2 for every fill it
applies. Its `config_refs` are §9's for `FillApplied`, except that at version 2 `instrument_snapshot`
is required only of a fill that names an order of ours (rule 300, v0.98,
[DEC-2817](../project/decisions/DEC-2817.md)). An unattributed fill may be in an instrument the
platform holds no snapshot of, such as a symbol the owner traded by hand that no agent trades. It
then names the fee configuration and both calendars, and no instrument snapshot. Every other fill
names all four, as version 1 does.

| Member | Type | Meaning |
|---|---|---|
| `fill_id` | `text` | The broker's id for the fill; fills are de-duplicated by it (trading spec §2.3) |
| `client_order_id` | `text?` | The order of ours the fill is of, or `null` when it names none: rule 299 |
| `instrument_id` | `text` | The fill's instrument |
| `side` | `text` | The fill's side |
| `qty_gross` | `decimal` | The gross quantity filled (trading spec §6.1) |
| `price` | `decimal` | The fill's price |
| `trade_date` | `date` | The fill's trade date (trading spec §2.2) |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |
| `fees` | list of `{kind: text, amount: decimal, asset: text, status: text}` | The fees the broker reported on the fill |

An unattributed fill moves the account's position and cash and no agent's: no order's filled
quantity changes, and a sell joins the unattributed sells an ended sell's report may net (DEC-421
item 5). The executor journals `ExternalActivityIngested` with `source` `fill` before it in the same
batch, then every agent's `exits_only` hold and the owner's alert (trading spec §7.1).

**Consistency rules** for `FillApplied` version 2 (the reason and path are each rule's), judged in
this order and reported at the first that fails:

299. `FillApplied` at `schema_version` 2: a non-null `client_order_id` is one of ours, as trading
     spec §2.3 states it: `md-` followed by one or more ASCII letters, digits or hyphens, at most
     128 characters in all (`payload.client_order_id`). A fill that names another system's id names
     no order of ours, so its `client_order_id` is `null`. Reason `schema`.
300. `FillApplied` at `schema_version` 2 that names an order (`client_order_id` non-null) names
     `instrument_snapshot` in its `config_refs` (reason `missing_config_ref`,
     `config_refs.instrument_snapshot`): a fill of an order of ours is in an instrument whose
     snapshot the order was gated on. An unattributed fill need not name one.

**`ClockAdvanced`** on the account stream at `schema_version` 1 (v0.94,
[DEC-2808](../project/decisions/DEC-2808.md)): the executor's copy of a scheduler-stream
`ClockAdvanced` (§9.20, §2). The executor journals it when the tick returns a risk record from the
account risk fold, or crosses New York midnight from the last copy it journaled, and at no other
tick ([DEC-2804](../project/decisions/DEC-2804.md) item 5; mandate spec §5.2). The account risk fold
derives `RiskDayStarted` from the copy that crosses midnight, and the records a copy returns follow
it in the next batch ([DEC-2399](../project/decisions/DEC-2399.md) item 5). `append` refused it as
`unknown_schema` before, so the risk day never rolled. The scheduler stream's record is §9.20's and
differs: it has no `risk_clock`, since no scheduler-stream record is a risk input.

| Member | Type | Meaning |
|---|---|---|
| `clock` | `risk_clock` | The scheduler's tick the copy carries |
| `risk_clock` | `risk_clock` | The same tick: rule 301. Every account-stream row the executor's fold reads carries `risk_clock` (§2), and `append`'s check that it never decreases applies |

**Consistency rules** for the account stream's `ClockAdvanced` (reason `schema`; the path is the
member named), judged in this order and reported at the first that fails:

301. `risk_clock` equals `clock` (`payload.risk_clock`): the copy is the tick, and the latest tick
     the executor had seen when it journaled the copy is that tick.
302. `causation_id` is non-null (`causation_id`): the scheduler stream's `ClockAdvanced` the row
     copies (§2's copy rule). `append` checks that one is named, not that it names a scheduler
     tick or that its `clock` is this one's: there is no global order across streams (§2), and the
     executor holds the link, as it does for the other copies.

**Checked by no rule at append**, as for §9.5's other executor records: the writer. The executor
writes the copy as a `system` actor, `executor`, with `config_refs` `{}`; `TradingDayStarted`'s
account-stream copy stays open (§9.20).

**`ProtectionChanged`** on the account stream at `schema_version` 1 and 2: §5.4's protective orders
and unprotected intervals, and the receipt-time protective prices of `intended` (DEC-446 item 4).
Version 2 ([DEC-2312](../project/decisions/DEC-2312.md)) is version 1's members with
`unprotected_fraction` before `risk_clock` and nothing else moved. Version 1 is never edited (§8),
stays registered, and carries no `unprotected_fraction`. Trading spec §5.4 protects only the
whole-share part of a fractional equity position and discloses the fraction left unprotected; the
fraction is that disclosure, on the records that start the position's interval, place its
protection, and end the interval with the position's final size. The executor's protection covers
the whole shares only and never the fraction. The fold does not read it.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `action` | `intended` \| `placed` \| `cancelled` \| `passive_start` \| `unprotected_start` \| `watchdog` \| `exit_unpriced` \| `ladder_floor` \| `expiry_unreplaceable` \| `rung_short` \| `interval_limit` \| `unprotected_end` | What changed: rule 41 on |
| `orders` | `[text]` | The protective orders the action names, as client order ids: rule 41 |
| `awaiting` | `[text]` | The orders an interval still awaits the confirmed cancel of: rule 41 |
| `qty` | `decimal?` | `placed`: the quantity the orders cover; `cancelled`: the quantity they uncovered, where journaled: rule 42 |
| `stop` | `decimal?` | The protective stop: rule 43 |
| `take_profit` | `decimal?` | The protective take-profit, `null` where the sequence has none (crypto's): rule 43 |
| `intent_id` | `intent`? | `intended`: the intent the prices came in with; `rung_short` and the starts: the exit's intent: rule 44 |
| `bracket` | `text?` | The entry a bracket action names: rule 44 |
| `entry` | `text?` | The exit sequence's entry: rule 44 |
| `agent_id` | `id?` | The exit sequence's owner: rule 44 |
| `replacing` | `boolean?` | The start replaces resting protection: rule 44 |
| `created_on` | `date?` | `placed`: the day the resting orders were created: rule 44 |
| `sent` | `decimal?` | `rung_short`: the quantity the short rung sent: rule 44 |
| `uncovered` | `boolean?` | The interval ends with the position uncovered: rule 44 |
| `acknowledged` | `boolean?` | `unprotected_end`: the owner acknowledged the interval: rule 44 |
| `unprotected_fraction` | `decimal?` | Version 2 only. The fraction of one share a fractional equity position holds beyond the whole shares protection covers (trading spec §5.4): rule 44 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for `ProtectionChanged` (reason `schema`; the path is the member named):

41. `orders` names at least one order on `placed` and `cancelled`, may be empty on
    `passive_start` and `unprotected_start`, and is empty on every other action
    (`payload.orders`); `awaiting` may be empty on `interval_limit` and `unprotected_end`, and is
    empty on every other action (`payload.awaiting`). A start may name no order (a bracket entry's
    first fill, a handed-on passive start, a passive wait, lost protection), and an interval may
    end, or reach its bound, awaiting no cancel ([DEC-859](../project/decisions/DEC-859.md)).
42. `qty` is non-null on `placed` — the record's covered quantity — and null on every action but
    `placed` and `cancelled` (`payload.qty`).
43. `stop` and `take_profit` are null on every action but `intended`, `placed`, `passive_start`,
    and `unprotected_start`, reported at `payload.stop`; and `take_profit` is null when `stop`
    is, reported at `payload.take_profit`.
44. The remaining members are null except where the fold reads them, each reported at its own
    member in the table's order, from `intent_id` to `acknowledged`: `intent_id` is non-null on
    `intended` and `rung_short`, and on `passive_start` and `unprotected_start` `intent_id`,
    `entry`, and `agent_id` are null together or non-null together — an exit sequence names all
    three — except where `replacing` is true, where `entry` and `agent_id` may be non-null, together,
    with `intent_id` null — a re-placement names its entry and owner and has no intent
    ([DEC-859](../project/decisions/DEC-859.md)) — reported at the first that disagrees, in the
    order `intent_id`, `entry`, `agent_id`;
    `replacing` may be non-null only on those two starts; `bracket` may be non-null only on
    `placed`, the two starts, and `unprotected_end`; `created_on` only on `placed`; `sent` only
    on `rung_short`; `uncovered` only on `interval_limit` and `unprotected_end`; and
    `acknowledged` only on `unprotected_end`. At version 2, `unprotected_fraction` may be non-null
    only on `unprotected_start`, `placed`, and `unprotected_end`, and where non-null is greater than
    0 and less than 1, read as the normalized decimal (`payload.unprotected_fraction`): a whole
    share is protected, never disclosed as a fraction.

**Batch rules** (reason `schema`):

45. An `OrderSubmitted` at `schema_version` 2 is immediately preceded in its `append` batch by
    exactly one `OrderRequestRecorded`, and names it as its `causation_id`. A submission with no
    companion immediately before it, or with more than one `OrderRequestRecorded` in the run of
    records immediately before it, is reported at `event_type`; one whose `causation_id` does not
    name that companion is reported at `causation_id`. An `OrderRequestRecorded` is then reported
    at its own `event_type` when no `OrderSubmitted` in the batch names it. Checked draft by
    draft first, in §9.1's order, then on the submissions in batch order, then the orphans. A
    version-1 `OrderSubmitted` carries no rule here: it precedes the companion (DEC-446 item 1).
    No §11 check repeats rule 45: the pairing sits inside one batch, which a range never splits
    (DEC-446 item 3).
248. An `OwnerConfirmationReceived` is immediately preceded in its `append` batch by an
     `IntentReceived` (reported at the confirmation's `event_type`) of the same intent (at its
     `payload.intent_id`), the same instrument (at its `payload.instrument_id`), and the same
     quantity, compared as decimals (at its `payload.qty`), whose `purpose` is `owner_exit` (at the
     intent's `payload.purpose`) and
     whose `limit_price` is null or not below the confirmation's `floor` (at the intent's
     `payload.limit_price`). Checked after each draft's own rules, on each confirmation in batch
     order, and reported at the first that fails, in that order. An intent needs no confirmation:
     an unconfirmed owner exit waits for the regular session (trading spec §5.5). No §11 check
     repeats rule 248, for rule 45's reason.

**No mapping to `JournaledFact`.** None of these records is a `ValidationContext` input: §9.2's
mapping covers the context's reads alone (DEC-446 item 7), and the executor's own fold reads
these directly.

### 9.6 Approval and reconciliation records ([DEC-460](../project/decisions/DEC-460.md))

These version-1 records are closed recursively: every listed member is present, `null` is used
only where the type is nullable, and any extra member is refused.

**`ApprovalRequested`** on the agent stream:

| Member | Type |
|---|---|
| `instrument` | `text` |
| `asset_class` | `us_equity` \| `crypto` |
| `side` | `buy` |
| `qty`, `limit` | `decimal` |
| `purpose` | `open` \| `increase` |
| `mandate_version` | `ref` |
| `decided_by` | `text` |
| `combined_score` | `decimal` |
| `reference_mark` | `{price: decimal, seq: integer}?` |
| `approvers_required` | `integer` |
| `independent_required` | `boolean` |
| `deadline` | `integer` risk-clock seconds |
| `timeout_s` | `integer` |
| `on_timeout` | `skip` |
| `content` | The closed object below |
| `content_hash` | `ref` |

`content` has exactly:

- `action`: `{instrument: text, asset_class: us_equity | crypto, side: buy, qty: decimal,
  limit: decimal, order_usd: decimal, purpose: open | increase}`;
- `trigger`: `{mandate_version: ref, decided_by: text}`;
- `evidence`: `{combined_score: {label: text, value: decimal}, outputs:
  [{event_id: ulid, artifact: ref?, label: Output of software you selected | platform-authored}]}`;
- `risk_impact`: `[{field: order_usd | position_usd_after | gross_usd_after | bought_today_usd |
  drawdown | daily_pnl_fraction, value: decimal, cap: decimal?}]`;
- `reference_mark: {price: decimal, seq: integer}?`, `deadline: risk_clock`,
  `default: text`, `choices: [approve | skip]`, and
  `approvers: {required: integer, independent: boolean}`.

The score label is exactly `combined model score, not a probability of profit`; `default` is
exactly `If you do nothing, this action is skipped`; and `choices` is exactly
`["approve", "skip"]`. Every top-level action, trigger, score, reference-mark, deadline, and
approver member equals its nested counterpart; the top-level integer deadline and nested
whole-second timestamp name the same instant. `content_hash` equals the `sha256:` reference of the
canonical `content` bytes.

**`ApprovalRequested` version 2** ([DEC-942](../project/decisions/DEC-942.md), v0.42) is version 1
with one more `content` member, last and required:

- `execution`: `{asset_class: us_equity | crypto, tif: day | gtc | ioc, protection_required:
  boolean, protection: {stop: decimal, take_profit: decimal?}?}`, the plan the deterministic order
  builder produced for the asked order: its asset class, time in force, whether protection is
  required, and the protective stop and take-profit (`null` when the mandate disables protection).

Version 1 refuses `execution` as an extra member, and version 2 refuses its absence. A writer uses
version 2 exactly when the builder produced a plan, and version 1 otherwise. `content_hash` covers
`execution` as it covers every other content member, so a grant that repeats it approves this plan
and no other, and a reader that recovers the plan recovers exactly these bytes or refuses
(DEC-942 item 3). One more rule, numbered on from §9.7's:

136. `ApprovalRequested` version 2 ([DEC-942](../project/decisions/DEC-942.md), v0.42; numbered on
     after rule 135 and renumbering none): `content.execution.asset_class` equals the top-level
     `asset_class` (`schema` at `payload.content.execution.asset_class`). Checked after the mirror
     rules and before the deadline rule.

**`ApprovalRequested` version 3** ([DEC-2515](../project/decisions/DEC-2515.md), v0.92) is version
2 with one more `content` member, last and required:

- `deferred_to_binding`: `[account_exposure` \| `agent_equity` \| `agent_mode` \| `conduct_history`
  \| `day_trade_budget` \| `eligibility` \| `fee_reservation` \| `halt` \| `liquidity_floor` \|
  `other_orders` \| `participation` \| `quote_ask` \| `reentry_cooldown` \| `related_accounts]`,
  the names `DecisionMade` version 3 gives them (§9.1): the checks, or parts of checks, the dry
  run left to the executor's binding gate. The approver is told they have not been run, and that
  they may still refuse the order ([mandate spec §6.4](mandate.md#64-approvals)).

Versions 1 and 2 refuse `deferred_to_binding` as an extra member, and version 3 refuses its
absence. A writer uses version 3 exactly when the `DecisionMade` that asks is version 3 and names a
deferral, which every allowed opening at version 3 does (rule 269). `content_hash` covers the
list as it covers every other content member, so a grant that repeats it approves the order
knowing exactly which checks are still to run. A request whose decision defers a check while the
builder produced no plan has no version yet: the journal refuses it, so the runtime asks no one
rather than hide the deferral (an accepted gap, as `DecisionMade` version 3's with version 2's
references is). Two more rules, numbered from the block the coordinator reserved for them:

291. `ApprovalRequested` version 3 ([DEC-2515](../project/decisions/DEC-2515.md), v0.92):
     `content.deferred_to_binding` is strictly ascending by bytes — `non_canonical` at
     `payload.content.deferred_to_binding`; it is non-empty — `schema` there; and it names
     `liquidity_floor` and `participation` both or neither — `schema` there, as rule 270 has the
     decision do. Checked after rule 136 and before the deadline rule.
292. `ApprovalRequested`, every version ([DEC-2515](../project/decisions/DEC-2515.md), v0.92), on
     the batch: when the `DecisionMade` its `causation_id` names is in the same `append` batch, its
     `content.deferred_to_binding` equals that decision's `payload.deferred_to_binding`, a
     version 1 or 2 member that is absent read as empty on either side — `schema` at
     `payload.content.deferred_to_binding`, and the batch is refused whole, as rule 10's second
     clause refuses one. So no request hides a deferral its decision names, and none names one
     the decision did not. The runtime writes the two in one batch. Across batches, §11's
     `approval_deferral_mismatch` reports it (v0.105, [DEC-2522](../project/decisions/DEC-2522.md)).

**`ApprovalDelivered`** on the agent stream:

| Member | Type |
|---|---|
| `approval` | `ulid` |
| `channel` | `cli_inbox` |
| `status` | `delivered` \| `suppressed_quiet_hours` \| `failed` |
| `message_id` | `text?` |

**`BrokerPositionObserved`** on the account stream:

| Member | Type |
|---|---|
| `instrument` | `text` |
| `broker_qty`, `model_qty` | `decimal` |
| `mismatch` | `boolean` |
| `risk_clock` | `risk_clock` |

**`AgentModeApplied`** on the account stream:

| Member | Type |
|---|---|
| `agent` | `text` (an agent id or the executor's `*` all-agent sentinel) |
| `to` | `normal` \| `exits_only` \| `paused` \| `stopped` |
| `restriction` | `text` |
| `originated` | `boolean` |
| `risk_clock` | `risk_clock` |

**`CompensatingEvent`** on the account stream:

| Member | Type |
|---|---|
| `subject` | `text` |
| `difference` | `order_state` |
| `from`, `to` | `intent` \| `submitting` \| `accepted` \| `partially_filled` \| `pending_cancel` \| `pending_replace` \| `unknown` \| `filled` \| `canceled` \| `rejected` \| `expired` \| `replaced` \| `abandoned` |
| `corrected_event_ids` | `[ulid]` |
| `risk_clock` | `risk_clock` |

### 9.7 Approval answers ([DEC-533](../project/decisions/DEC-533.md))

The owner's answer to an approval and the runtime's two records of it ([mandate spec
§6.4](mandate.md#64-approvals)), closed at schema version 1 as §9.6's records are: every listed
member is present, `null` only where the type is nullable, and any extra member is refused. §9.1's
types apply, with `null`, a member that is always `null` at this version. The rules number on from
§9.5's. The times are **integer risk-clock seconds**, as `ApprovalRequested.deadline` is and as the
CLI and the runtime write them ([DEC-257](../project/04-decision-log.md#decisions) item 5); a
`risk_clock` timestamp is refused there. §9.2's and §9.3's step-up evidence types `authenticated_at`
as a §4.7 timestamp instead; the two are unified only by a later version (DEC-533 item 2).

**`ApprovalResponseSubmitted`** on the control stream. It names no configuration.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent whose approval it answers |
| `approval` | `ulid` | The approval: its `ApprovalRequested`'s `event_id` |
| `verdict` | `approved` \| `skipped` | |
| `content_hash` | `ref` | The content hash the owner was shown, repeated (mandate spec §6.4 check 5) |
| `submitted_at` | `integer` | The risk-clock second the owner answered at |
| `step_up` | `{assertion_id: text, authenticated_at: integer, method: text}?` | The step-up evidence, or `null`; the runtime judges it (check 6) |
| `responder` | `text` | The owner who answered (opaque): rule 46 |
| `role` | `approver` | |

**`ApprovalResponded`** on the agent stream: the runtime's copy of one `ApprovalResponseSubmitted`,
which is its `causation_id` (rule 50), with the result admission gave it.

| Member | Type | Meaning |
|---|---|---|
| `approval`, `verdict`, `responder`, `role` | As `ApprovalResponseSubmitted`'s | Copied |
| `result` | `admitted` \| `counted` \| `refused` | |
| `reason` | `not_pending` \| `late` \| `not_an_approver` \| `not_delivered` \| `content_mismatch` \| `step_up_missing` \| `step_up_stale` \| `step_up_reused` \| `step_up_method` \| `duplicate_approver` \| `not_independent` (nullable) | The first admission check that failed: rules 47 and 49 |
| `effective_at` | `integer` | The risk-clock second the response was judged at |
| `step_up` | As `ApprovalResponseSubmitted`'s | Copied |
| `quorum` | `{independent: boolean, required: integer}?` | The approver count and independence check 7 applied, the stricter of the bound values and the policy overlay: rule 48 |
| `separation_of_duties` | `null` | The separation-of-duties result. Always `null` at this version; E8's delegation story gives it a type at a later one |
| `delegation` | `null` | The delegation shape chosen, with its new mandate version and delegation id (mandate spec §6.5). Always `null` at this version, as `separation_of_duties` |

**`ApprovalRevalidated`** on the agent stream: re-validation of an admitted grant (checks 8 to 12),
with every value compared.

| Member | Type | Meaning |
|---|---|---|
| `approval` | `ulid` | |
| `result` | `act` \| `skip` | |
| `reason` | `text?` | The first failing check's reason, or the gate's reason code (check 11): rule 51 |
| `mandate_version_bound`, `mandate_version_now` | `ref` | Check 8 |
| `mode` | `normal` \| `exits_only` \| `paused` \| `stopped` | The effective mode (check 9) |
| `instrument_restricted` | `boolean` | Check 9 |
| `decided_by_bound` | `text` | The bound trigger's label (check 10) |
| `decided_by_now` | `text?` | The re-classification's `ask` label, or `null` when it is not an `ask`, as an empty label is (§4.2) |
| `dry_run` | `allow` \| `deny` | The gate dry run (check 11) |
| `dry_run_reason` | `text?` | The gate's reason code: rule 52 |
| `m_req`, `m_now` | `decimal?` | The reference mark at the request and the latest folded mark (check 12) |
| `band_bp` | `integer` | The drift band: rule 53 |

**`ApprovalTimedOut`** on the agent stream ([DEC-944](../project/decisions/DEC-944.md), v0.41): the
runtime's record that an ask's deadline passed with no terminal answer, at the first tick at or after
it, and that its safe default applied ([mandate spec §6.4](mandate.md#64-approvals)). Its
`causation_id` is the ask: rule 135. It names no configuration beyond §9's row.

| Member | Type | Meaning |
|---|---|---|
| `approval` | `ulid` | The approval: its `ApprovalRequested`'s `event_id`, whose `deadline` passed |
| `on_timeout` | `skip` | The safe default that applied: nothing is acted on |

**`ApprovalCanceled`** on the agent stream ([DEC-950](../project/decisions/DEC-950.md), v0.43): the
runtime's record that a pending ask was canceled before any terminal answer, by a version applied
(mandate spec §2.2), a mode tightened to exits-only or stricter (§5.9), the owner's pause or Stop,
or a kill switch ([mandate spec §6.4](mandate.md#64-approvals)), so nothing it bound is acted on. It
names no configuration beyond §9's row, and no rule reads its `causation_id`.

| Member | Type | Meaning |
|---|---|---|
| `approval` | `ulid` | The approval: its `ApprovalRequested`'s `event_id` |
| `reason` | `version_applied` \| `mode_tightened` \| `owner_pause` \| `owner_stop` \| `kill_switch` | What canceled it. A legacy `rebound` is refused |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

46. `ApprovalResponseSubmitted`: `responder` equals the envelope's `actor.id`
    (`payload.responder`), so the answer names the identity that wrote it.
47. `ApprovalResponded`: `reason` is non-null exactly when `result` is `refused`
    (`payload.reason`).
48. `ApprovalResponded`: `quorum` is non-null exactly when check 7 was judged: `verdict` is
    `approved` and either `result` is `admitted` or `counted`, or `reason` is
    `duplicate_approver` or `not_independent` (`payload.quorum`).
49. `ApprovalResponded`: a `skipped` verdict runs checks 1 to 5 only. It is never `counted`
    (`payload.result`), and when refused its reason is one of `not_pending`, `late`,
    `not_an_approver`, `not_delivered`, and `content_mismatch` (`payload.reason`).
50. `ApprovalResponded`: `causation_id` is non-null: the `ApprovalResponseSubmitted` it copies
    (`causation_id`).
51. `ApprovalRevalidated`: `reason` is non-null exactly when `result` is `skip`
    (`payload.reason`).
52. `ApprovalRevalidated`: `dry_run_reason` is non-null exactly when `dry_run` is `deny`
    (`payload.dry_run_reason`).
53. `ApprovalRevalidated`: `band_bp` is 100 or 200 (`payload.band_bp`), and an `act` passed every
    check it records. Its members are checked in this order, and the first that shows a failure is
    reported, at its own path:
    `mandate_version_now` equals `mandate_version_bound`; `mode` is `normal`;
    `instrument_restricted` is false; `decided_by_now` is `null` or equals `decided_by_bound`
    (check 10: an `auto` passes, a different `ask` skips); `dry_run` is `allow`; `m_req` and
    `m_now` are non-null, each
    reported at itself; and |`m_now` − `m_req`| × 10 000 ≤ `band_bp` × `m_req`, on exact decimals
    (`payload.m_now`).
135. `ApprovalTimedOut` ([DEC-944](../project/decisions/DEC-944.md), v0.41; numbered on after rule
     134 and renumbering none): `causation_id` is non-null and equals `payload.approval`
     (`causation_id`, both clauses), so the timeout names the ask it ends. Checked after
     `artifact_refs` and `pii_refs`, as rule 50 is.

### 9.8 Connection records ([DEC-800](../project/decisions/DEC-800.md))

A connection's history, from connect to revocation ([connections spec §3, §8.1,
§9](connections.md), CN-10), closed at schema version 1 as §9.7's records are, except
`ConnectionEstablished`, which gains version 2. §9.1's types and report order apply, with §9.2's
`risk_clock`, and the rules number on from §9.7's. No member is a credential, a broker account
number, or the account fingerprint (CN-1, connections spec §3.1): a draft carrying one has no member
to sit in and is refused as `schema`. No `ConnectionEstablished` version 1 exists outside the
paper fixtures (CN-12): the first writer of a connection is E7-17's, and it writes version 2.

**Who writes what.** The workspace control services write what happens at the owner's request on
the control stream: a connect's start, a connect, a reconnect, a credential the owner replaces, and each one refused.
The account's executor writes what it finds on the account stream: its permission checks, the
connection's state, and a token it refreshes. It also copies the two control-stream records that
change what it may do, as §2 copies owner input: the version-2 `ConnectionEstablished` that binds
its stream, and each `ConnectionCredentialRotated`. Each account-stream record carries
`risk_clock`, as the executor writes it on every event (§2), and the `connection_id` it acts for,
since the stream exists before the connection is established.

**The connect sequence** (connections spec §5.2). The connection manager assigns the connection's
`connection_id` and `account_ref` when the connect starts, and journals them in
`ConnectionRequested` (rule 131) before the redirect, before any vault write, and before any process
starts. For an OAuth connect, the token-exchange
process (connections spec §5.2 step 4, DEC-821 item 2) redeems the code and stores the token in the
vault; it appends nothing to the journal. The connecting executor, started only once a credential is
stored, opens the account stream `acct:{workspace_id}:{account_ref}` with `StreamOpened`, runs §8.1
checks 1, 2, 3, and 7 against the credential in the vault, using only reads and, for a paper
connection, only paper hosts, and journals `ConnectionChecked` (occasion `connect`) there.
It stores the broker's account id in the personal-data vault and records only that reference
(`account_pii_ref`, §6.4), never the id. The control services read that record, have the vault
compute the account fingerprint from the reference (connections spec §3.1; they receive only the
keyed hash), complete check 3 against the connection's record and run check 4 (uniqueness), and
append `ConnectionEstablished` version 2, whose `causation_id` is that `ConnectionChecked` (rule
63), only after the passing results are journaled. The control services are the API process, which
never calls the broker (connections spec §5.2). Before appending, they confirm it is on the stream `account_ref` names, has the same
`connection_id`, an occasion of `connect` (or `reconnect`), every result `passed`, and, for an MCP
connection, the `contract` result. The executor then copies the establishment onto its stream
(rule 68): until that copy, the stream is `connecting`, and the executor may only run and record
the connect sequence's checks. Checks 5 and 6 are read from `AccountStateObserved` and never refuse
a connection (connections spec §8.1: connected, agents not deployable, status shown).

**A refused connect** is `ConnectionRefused`. Its `causation_id` is the failed `ConnectionChecked`
when the executor's check failed, and `null` when the control services refused (check 3's
fingerprint comparison, `account_mismatch`, or check 4) or when no check refused at all: the
teardown of connections spec §5.2 step 6, after the deadline (`timeout`), on an API restart past
it (`restart_past_deadline`), after an executor restarted with no token to check
(`executor_stopped`), or when no executor was started (`start_failed`: the token-exchange process or
the executor failed to start after the vault write, step 3, or the exchange stored no token, step 4), with `check` `null` (rules 54 and 65). Its account stream is an orphan: its
`account_ref` is never bound, nothing is written to it again, and it is kept like every stream,
under §6.2's retention, because it records the refusal's evidence. The executor that opened it has
exited, and its writer epoch is never reused (§5.1).

**`ConnectionRequested`** on the control stream: the pending connection (`connecting`, connections
spec §5.2 step 1 and §9.1). The connection manager commits it when the connect starts, in the
transaction that marks the connect's step-up challenge used (identity spec §7.2 step 5), so the
step-up spent on a connect is in the journal whether or not the connect completes. A start refused
before that (failed step-up, or a live Alpaca OAuth start, connections spec §5.2) commits nothing.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection the attempt is for: rule 131 |
| `account_ref` | `ulid` | The account stream `acct:{workspace_id}:{account_ref}` the attempt's executor opens: rule 131 |
| `broker` | `text` | |
| `environment` | `paper` \| `live` | As `ConnectionRefused`'s. The start route connects paper only (connections spec §5.2, DEC-693 item 2) |
| `user` | `text` | The workspace admin who started it (opaque) |
| `step_up` | As `ConnectionEstablished`'s | Never `null`: the connect started only after step-up (identity spec ID-4) |

No member is a credential, the authorization code, the PKCE verifier, the `state`, the vault path,
an account number, or the fingerprint (CN-1, CN-10). The establishment or refusal that closes it
repeats its `user` and `step_up`: that is one connect's evidence written again, not a second use
of the step-up.

**`ConnectionRequested` version 2** ([DEC-2621](../project/decisions/DEC-2621.md), E7-27): version
1's six members, then:

| Member | Type | Meaning |
|---|---|---|
| `credential` | `oauth` \| `api_key` | How the connection's credential is obtained |
| `breadth` | `{other_environment: may_reach, disclosure: {document: id, version: text}}` (nullable) | For `oauth`: the environment the token may also reach, and the disclosure the owner confirmed with the connect's step-up, by document and version (DEC-821 item 4, DEC-441 item 22). `null` for `api_key`: rule 255 |

Rule 255: a version-2 `ConnectionRequested` has `breadth` non-null exactly when `credential` is
`oauth`; an `oauth` request names `broker` `alpaca` and `environment` `paper` (DEC-821 item 7); and
its `breadth.disclosure.document` is `alpaca_oauth_token_breadth`. The first clause that fails is
reported, in that order, as `schema` at `payload.breadth`, `payload.broker`, `payload.environment`,
or `payload.breadth.disclosure.document`. `other_environment` is `may_reach` only: Alpaca does not
document that a paper token cannot reach live (connections spec §5.5, U-A4), and a value that says
so needs a decision. Version 1 stays registered, replayable, and unchanged, and reads as `api_key`:
no route that starts an OAuth connect existed before v0.68. The start route (workspace API §4.5),
the only writer of `ConnectionRequested`, writes version 2. The connect's step-up digest binds the
disclosure's document and version (DEC-2621 item 4), so the step-up proves which text the owner
confirmed.

E7-27's order, that no Alpaca OAuth connect appends `ConnectionEstablished` before the record of its
token's breadth, holds at the writer, not by a stream rule: an OAuth connect's establishment closes
its open request (rule 131), and the connection manager, its only writer, appends one only to close
an open version-2 `oauth` request (E10-13 slice 6 tests that writer). §11 cannot judge it, because
the CLI's `connection record`, the Phase-1 API-key path (DEC-529 item 3), writes an establishment
that names no request, with the same broker and environment.

A request is **open** from its commit until the record that closes it (rule 131), and every exit
from `connecting` closes it: the establishment (connections spec §5.2 step 5), or the
`ConnectionRefused` of a refused check or of step 6's teardown (`timeout`,
`restart_past_deadline`, `executor_stopped`, `start_failed`), or a `ConnectionRevoked` (below). A callback that is refused, or a
vault write that fails, starts nothing, and the deadline's teardown closes the request. After an
API restart, the open requests are the `connecting` records the connection manager re-reads
(step 6, Restarts). An open request is a connect in progress, so §11 does not report one; the
deadline bounds how long it stays open. A writer that records a connect in one step, as the CLI's
`connection record` does ([DEC-529](../project/decisions/DEC-529.md) item 3), commits the request
and the establishment in one batch.

**A revoke while `connecting`** (connections spec §9.1, DEC-699 item 5) is never refused for it: the
pending connection has no agent and no position. Its `ConnectionRevoked` closes the open request.
The connection manager then runs step 6's teardown (vault entry deleted, grant revoked, processes
stopped) and journals nothing further for that request: no establishment, no `ConnectionRefused`.
The connection was never established, so it has nothing to reconnect; the owner starts a new
connect, with a new request and `account_ref`.

**A `causation_id` may name an event on another stream** (§3: it is an event ID). The connect
sequence uses it from the control stream to the account stream, and the copies the other way, as
§2's copies do. §11's per-range checks follow a cause only within the range's own stream; the
full-chain run's `connection_cause_mismatch` follows each one across. A reconnect and a credential
replacement run the same sequence, with occasion `reconnect` or `reauthorize`.

**`ConnectionEstablished` version 2** on the control stream: version 1's four members, then:

| Member | Type | Meaning |
|---|---|---|
| `account_ref` | `ulid` | The account stream `acct:{workspace_id}:{account_ref}` this connection writes through (§2). This is the binding DEC-261 item 10 asked for: an account stream belongs to the connection whose establishment names its `account_ref` |
| `user` | `text` | The workspace admin who connected (opaque) |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: connecting needs step-up (identity spec ID-4). This is the connecting user and step-up DEC-261 item 8 owed the connect flow |
| `margin_attestation` | `cash_account` \| `margin_disabled` (nullable) | The owner's attestation, confirmed with the step-up, that the account is a cash account or has margin disabled ([DEC-529](../project/decisions/DEC-529.md) item 11, FR-2.6); non-null exactly for a `live` connection: rule 64 |

Rule 19 applies to both versions, and rules 63 and 64 to version 2: its `causation_id` is the
passing `ConnectionChecked` of the connect sequence. Version 1 stays registered and replayable, and
maps to its `JournaledFact` as before; version 2 maps the same way. A writer that has an
`account_ref` writes version 2.

**The account stream's copy of `ConnectionEstablished`** is version 2 only: version 2's members,
then `risk_clock`. Its `causation_id` is the control-stream original (rule 63), whose members it
carries unchanged.

**`ConnectionRefused`** on the control stream: a connect, reconnect, or credential replacement the
§8.1 checks refused, or the teardown that ended it. Nothing else is kept of a refused connect
beyond its `ConnectionRequested` and its orphan account stream (connections spec §9.1); the credential
was deleted from the vault before this record is written. With occasion `connect`, it closes the
connection's request (rule 131).

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection the attempt was for: a new id for a connect, the existing one otherwise |
| `broker` | `text` | |
| `environment` | `paper` \| `live` | |
| `occasion` | `connect` \| `reconnect` \| `reauthorize` | A new connection; a revoked one connected again; a credential replaced on a connection that is not revoked |
| `check` | `scope` \| `environment` \| `account` \| `uniqueness` \| `contract` (nullable) | The first §8.1 check that failed (checks 1, 2, 3, 4, and 7), or `null` for a teardown no check caused: rule 54 |
| `reason` | See the reasons table, or `timeout` \| `restart_past_deadline` \| `executor_stopped` \| `start_failed` | Rule 54 |
| `existing_connection_id` | `id?` | For `uniqueness`, the connection that already holds the account: rule 55 |
| `user` | `text` | The owner who attempted it (opaque) |
| `step_up` | As `ConnectionEstablished`'s | Never `null`: the attempt started only after step-up |

**`ConnectionCredentialRotated`** on the control stream: the owner replaced the credential of a
connection that is not revoked, and checks 1 to 4 and 7 passed against it. Its `causation_id` is
the executor's `ConnectionChecked` with occasion `reauthorize` and every result `passed` (rule 63),
which the control services confirm as for `ConnectionEstablished`. Check 3 compares the account the
new credential reaches with the connection's fingerprint, so the replacement is the same account;
the connection keeps its `connection_id`, and with it its `broker`, `environment`, and
`account_ref`, and its scopes stay the same or narrow (rule 67). The executor copies it onto the
account stream, with `risk_clock`, its `causation_id` the original (rule 63). It is how a
`suspended` connection gets a working credential again ([DEC-800](../project/decisions/DEC-800.md)
item 5): the copy alone does not leave `suspended`; rule 68 requires a `condition_cleared` that
names the copy, and then the owner's acknowledgment.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |
| `scopes` | `[text]` | The scopes the new credential grants, strictly ascending by bytes: rule 56 |
| `user` | `text` | The owner who replaced it (opaque) |
| `step_up` | As `ConnectionEstablished`'s | Never `null` (identity spec ID-4: changing a connection) |

**`ConnectionChecked`** on the account stream: the executor's §8.1 checks 1, 2, 3, and 7, at a
connect, a reconnect, or a credential replacement (before the control stream records it), at each
executor start, and daily. Check 4 is the control services' alone, and so is check 3's fingerprint
comparison, made from `account_pii_ref`: an executor never reports `account_mismatch` (rule 58).

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection the checks are for: rule 68 |
| `occasion` | `connect` \| `reconnect` \| `reauthorize` \| `executor_start` \| `daily` | |
| `results` | `[{check: scope \| environment \| account \| contract, result: passed \| failed, reason: (see the reasons table)?}]` | Every check run: rules 57 and 58. `uniqueness` is the control services' (connections spec §8.1) |
| `account_pii_ref` | `pii_ref?` | The personal-data vault reference of the broker account id the credential reaches (§6.4): `pii_` and a ULID, never the id; `null` exactly when the account could not be read: rule 62 |
| `risk_clock` | `risk_clock` | |

Checks 5 (1× buying power) and 6 (account status) are not here: `AccountStateObserved` (§9)
journals the multiplier and the status they read, trading spec §7.2 and §7.3 act on them, and
neither refuses a connection or moves its state.

**`ConnectionStateChanged`** on the account stream: the connection's state (connections spec §9.1)
on the executor's side. The stream is `connecting` until its copy of the establishment, and the
connection `active` from then. `connecting` is the control stream's: `ConnectionRequested` opens
it, and the establishment or a `ConnectionRefused` closes it (rule 131). `revoked` is the control
stream's `ConnectionRevoked`.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | As `ConnectionChecked`'s |
| `from`, `to` | `active` \| `degraded` \| `suspended` | Rules 59 and 60 |
| `reason` | `network_errors` \| `rate_headroom` \| `contract_drift` \| `authorization_failed` \| `credential_expired` \| `refresh_failed` \| `check_failed` \| `lease_expired` \| `condition_cleared` \| `acknowledged` | Why: rule 59 |
| `risk_clock` | `risk_clock` | |

The first three reasons are connections spec §8.2's degrading signals; the next five are §9.1's
`suspended` causes and §9.2's vault outage. A later `contract` check (occasion `executor_start` or
`daily`) that fails with `tools_missing` is connections spec §8.1 check 1's, so it suspends: it is
journaled with `check_failed` into `suspended`, never with `contract_drift`. A failed `contract`
check with reason `contract_drift` and no other failure degrades, and a failed `scope`,
`environment`, or `account` check suspends whatever else failed with it. A connect-time occasion
(`connect`, `reconnect`, `reauthorize`) moves no state: its failure is a refusal
([DEC-674](../project/decisions/DEC-674.md)).
`condition_cleared` records that the cause has cleared (good probes, a released connector version
for drift, or, out of `suspended`, a replaced credential the control services accepted) while the
state stays where it is. `acknowledged` returns the
connection to `active`, and its `causation_id` is the `OwnerAcknowledged` the executor copied (rule
61). A reconnect continues its account stream (CN-12), and with it the state that stream last
recorded: a connection revoked while `suspended` is still `suspended` when it is established again.
A reconnect journals a `ConnectionEstablished` copy and a `reconnect` check, never a rotation, so it
does not by itself leave `suspended`: after reconnecting, the owner still re-authorizes, and the
connection returns to `active` only as the next paragraph says, by `condition_cleared` and then
`acknowledged`, as trading spec §7.3 lifts its restriction.

**Out of `suspended`** ([DEC-800](../project/decisions/DEC-800.md) item 5), by re-authorization only,
in order, each on the account stream (rule 68): the owner replaces the credential; the executor journals a
`ConnectionChecked` with occasion `reauthorize`, after the `ConnectionStateChanged` that entered
`suspended`, in which every result passed (with `contract` for an MCP connection); the control
services confirm it, compare the fingerprint (the same account), and append
`ConnectionCredentialRotated` naming it; the executor copies the rotation; a `condition_cleared`
names that copy as its `causation_id`; and the owner's acknowledgment follows. A check the control
services refused is named by no rotation, so it never clears a suspension, and neither does a check
or a rotation from before the suspension.

**The halt is never behind one of these records** (CN-6, `AGENTS.md` rule 3). Into `degraded` or
`suspended`, the executor first commits `AccountRestrictionChanged` (`closing_only`, cause
`connection_unavailable`) and `AgentModeApplied` (trading spec §7.3), and only then the
`ConnectionChecked` that failed and the `ConnectionStateChanged`, in a later batch. A refused or
failed append of these records leaves openings halted. Out of it, the `acknowledged` state change is
committed in the batch that lifts the restriction, or before it, so a refused state change keeps
the restriction. No rule here applies to an exit, a protective order, or a kill switch.

**`ConnectionCredentialRefreshed`** on the account stream: the executor refreshed an expiring token
through the vault (connections spec §5.4), only where the broker issues refresh tokens (U-A3).

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | As `ConnectionChecked`'s |
| `scopes` | `[text]` | The scopes the refreshed token grants, strictly ascending by bytes: rule 56 |
| `risk_clock` | `risk_clock` | |

**Reasons** a check fails, by check. A reason belongs to its check: rule 54 (`ConnectionRefused`)
and rule 58 (`ConnectionChecked`).

| Check | Reasons |
|---|---|
| `scope` | `scope_mismatch` (the grant differs from the request), `fund_movement` (a permission, scope, or tool that can move funds out, CN-2), `permissions_unreadable` (a live credential whose permissions cannot be read) |
| `environment` | `wrong_environment` (it does not work against its own environment), `reaches_both` (the broker's documentation does not show it cannot reach the other, CN-3; DEC-441 item 21) |
| `account` | `account_unreadable` (the account could not be read), `account_mismatch` (another account than the connection names, by fingerprint; the control services' only), `not_dedicated` (for Robinhood, not the dedicated agentic account) |
| `uniqueness` | `already_connected` (CN-5) |
| `contract` | `tools_missing` (an allowlisted tool is absent), `contract_drift` (the pinned hash differs, or a later check finds no pin: [DEC-676](../project/decisions/DEC-676.md) item 2) |

An **MCP connection** is one whose broker connects through MCP (connections spec §3,
`mcp_oauth`): `robinhood`. Its checks always list `contract`; a connection that is not MCP has no
contract to check, so an absent `contract` passes only for it (rule 68, §11).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

54. `ConnectionRefused`: `reason` belongs to `check`, or `check` is `null` and `reason` is
    `timeout`, `restart_past_deadline`, `executor_stopped`, or `start_failed` (`payload.reason`).
55. `ConnectionRefused`: `existing_connection_id` is non-null exactly when `check` is `uniqueness`,
    and then differs from `connection_id` (`payload.existing_connection_id`).
56. `ConnectionCredentialRotated`, its copy, and `ConnectionCredentialRefreshed`: `scopes` strictly
    ascending by bytes (`non_canonical` at `payload.scopes`), as rule 19.
57. `ConnectionChecked`: `results` strictly ascending by `check` bytes, so no check is listed twice
    (`non_canonical` at `payload.results`), and it includes `scope`, `environment`, and `account`
    (`payload.results`). Whether `contract` must be listed depends on the connection's broker,
    which the record does not carry: rule 68 and §11 check it.
58. `ConnectionChecked`: in each result, `reason` is non-null exactly when `result` is `failed`, and
    then belongs to `check` and is not `account_mismatch` (`payload.results[i].reason`).
59. `ConnectionStateChanged`: `reason` fits `to` (`payload.reason`): the three degrading reasons
    only into `degraded`, the five suspending reasons only into `suspended`, `condition_cleared`
    only with `to` equal to `from` and not `active`, and `acknowledged` only into `active`.
60. `ConnectionStateChanged`: `from` fits `to` (`payload.from`): `active` only from `degraded` or
    `suspended`, and `degraded` never from `suspended`, since a credential that does not work is
    not made better by a network error.
61. `ConnectionStateChanged` with reason `acknowledged` has a non-null `causation_id`
    (`causation_id`).
62. `ConnectionChecked`: `account_pii_ref` is `null` exactly when the `account` result failed with
    `account_unreadable`, and when non-null it is listed in the envelope's `pii_refs` (§3)
    (`payload.account_pii_ref`). Its type, `pii_ref`, is `pii_` and a ULID (§6.4): anything else,
    an account number among them, is `non_canonical`.
63. `ConnectionEstablished` version 2 and `ConnectionCredentialRotated`, on either stream, have a
    non-null `causation_id` (`causation_id`): on the control stream, the passing
    `ConnectionChecked` of their connect sequence; on the account stream, the control-stream
    original the executor copied.
64. `ConnectionEstablished` version 2 and its copy: `margin_attestation` is non-null exactly when
    `environment` is `live` (`payload.margin_attestation`).
65. `ConnectionRefused` with `check` `null` has a `null` `causation_id` (`causation_id`): no check
    failed.

**Stream rules** (connections spec §3, CN-5, §9.1). These need the stream's earlier records, which
`append` does not fold. The stream's owner checks them against its own fold and refuses the draft
before appending; §11's `connection_lifecycle_mismatch` checks them on every range, from the
range's connection anchor, the fold's state before its trusted start, or failing closed without one
([DEC-885](../project/decisions/DEC-885.md)). Each holds on its own stream:

66. Control stream, `ConnectionEstablished`: when an earlier `ConnectionEstablished` names the same
    `connection_id`, the latest earlier `ConnectionEstablished` or `ConnectionRevoked` of that id is
    a `ConnectionRevoked`, and `broker`, `environment`, and `account_ref` equal the first
    establishment's. A version-1 first establishment has no `account_ref`, so it is never
    re-established. No earlier `ConnectionEstablished` of another `connection_id` names the same
    `account_ref` (CN-5).
67. Control stream: a `ConnectionCredentialRotated`, or a `ConnectionRefused` with `occasion`
    `reauthorize`, names a connection established and not since revoked; a `ConnectionRefused` with
    `reconnect` names one whose latest record is `ConnectionRevoked`, unless that revocation closed
    its request (rule 131); one with `connect` names an id with no earlier `ConnectionEstablished`
    and no earlier `ConnectionRevoked` other than revocations that each closed a request of it
    (rule 131 then judges the refusal, so a retry after such a revocation is torn down normally). For `reconnect` and `reauthorize`, `broker` and `environment` equal the
    establishment's. A rotation's `scopes` are a subset of the connection's latest scopes, its
    establishment's or the latest rotation's: a replaced credential never widens a grant.
68. Account stream: every connection record on it names the same `connection_id` (CN-5: one
    account stream, one connection), and:
    - **Binding.** The copy of `ConnectionEstablished` names this stream's `account_ref`, and the
      latest `ConnectionChecked` before it with occasion `connect` (`reconnect` for a second copy)
      passed every result, with `contract` for an MCP connection. Before the first copy, the stream
      holds only `ConnectionChecked` records of occasion `connect`, `reconnect`, or `reauthorize`;
      a `ConnectionStateChanged`, a `ConnectionCredentialRefreshed`, a rotation's copy, or an
      `executor_start` or `daily` check is refused.
    - **Contract.** After a copy whose broker is MCP, every `ConnectionChecked` lists `contract`.
    - **Rotation.** A rotation's copy follows a `ConnectionChecked` with occasion `reauthorize`,
      the latest before it, that passed every result (with `contract` for an MCP connection).
    - **State.** On `ConnectionStateChanged`: `from` equals the state the stream's previous
      `ConnectionStateChanged` left, or `active` when there is none; `acknowledged` follows a
      `condition_cleared`, with no other state change between them; and a `condition_cleared` out
      of `suspended` names as its `causation_id` a rotation's copy on this stream whose
      `reauthorize` check came after the `ConnectionStateChanged` that last entered `suspended`.
      The owner's acknowledgment never lifts a cause that has not cleared, and a suspended
      connection clears only on a credential the control services accepted after the suspension.
    - **Revocation** ([DEC-888](../project/decisions/DEC-888.md)). A `ConnectionRevoked` belongs
      to the control stream, so one on an account stream is refused, bound or not. `append`
      already refuses it there (`wrong_stream`); this clause is what §11 judges a stored chain by.

No rule here refuses a `ConnectionRevoked` on a control stream, so §5.6's compromised revocation,
whose kill switch shares its batch, is never held by one. Rule 68 refuses only one on an account
stream, which no writer can append. §9.8 adds no `ConnectionRevoked` version; version 2
arrives in §9.10 (v0.22, DEC-671), and these rules count a revocation of either version.

**Stream rule 131** ([DEC-699](../project/decisions/DEC-699.md)), held and checked as rules 66 to
68 are:

131. Control stream, the pending connection: checked after rules 66 and 67 on the same record,
     and only **from the stream's first `ConnectionRequested` on**, as `held_mismatch` judges a
     stream only from its first version-2 record (§11). A connect before it is not judged by this
     rule, so a stream written before v0.32 still verifies; after it, every connect closes a
     request, and a stream never leaves that state.
     - **Opening.** A `ConnectionRequested` names a `connection_id` with no open request and no
       earlier `ConnectionEstablished`, and an `account_ref` that no earlier `ConnectionRequested`
       or `ConnectionEstablished` names: each attempt has an account stream of its own, and an
       orphan's is never used again.
     - **Closing.** A `ConnectionEstablished` version 2 of an id with no earlier
       `ConnectionEstablished` (a connect), and a `ConnectionRefused` with occasion `connect`, each
       close that id's open request: one exists, and the record repeats its `broker`,
       `environment`, `user`, and `step_up`, and, for the establishment, its `account_ref`. A
       request is closed once. A reconnect, a credential replacement, and a version-1 establishment
       close none.
     - **Version 1.** From the stream's first request on, a version-1 `ConnectionEstablished` of
       an id with no earlier establishment breaks this rule: version 1 exists only in the paper
       fixtures, and no connect skips its request by writing it.
     - **Revocation.** A `ConnectionRevoked` of an id with an open request closes it, and is never
       refused. A later establishment or `ConnectionRefused` (occasion `connect`) of that id that
       has no request of its own breaks this rule.
     - **Ranges.** A range checks every clause, that a closing record has an open request
       included, from its connection anchor (§11), which carries the open requests and whether the
       stream holds a request; a range with no anchor fails closed at its first record this rule
       or rules 66 to 68 judge ([DEC-885](../project/decisions/DEC-885.md), amending DEC-699).

**No mapping to `JournaledFact`** beyond `ConnectionEstablished`'s (§9.2): the control-stream
version, never its copy. `ConnectionRequested` maps to none. The executor folds the account-stream records itself, and the connection
manager the control-stream ones.

### 9.9 Workspace API records ([DEC-670](../project/decisions/DEC-670.md))

Four control-stream records the [workspace API](workspace-api.md) commits (its §4.1, §4.6, §5.1;
[DEC-436](../project/decisions/DEC-436.md) item 14), closed as §9.7's are: every listed member is
present, `null` only where the type is nullable, and any extra member is refused. §9.1's and §9.2's
types apply, with §9.3's `asset_id`. The rules number on from §9.7's. Each record names the
principal that committed it in `actor` (§3, [identity spec ID-1](identity.md)), and rules 70, 76,
78, and 79 are the first that read `actor.kind`; each reports `schema` at `actor.kind`.

**What these records never do.** A draft, a compile, and a request change no envelope (`AGENTS.md`
rule 11): a draft is not a version, nothing reads it as one, and it may fail validation; a request
reaches only the order builder (rule 4). No member holds model text, a credential, or personal
data: the draft, the prompt, and the response are stored artifacts named by `ref` (§6.3, §6.4;
workspace API API-18), and users are opaque IDs.

**`MandateDraftSaved`**: one explicit save of a mandate draft (DEC-436 item 11). The draft is the
document as edited, with each path's provenance (mandate spec §2.1), stored under its hash.

| Member | Type | Meaning |
|---|---|---|
| `draft_id` | `id` | The draft, the same across its saves |
| `draft` | `ref` | This save's stored draft |
| `origin` | `description` \| `goal_answers` \| `template` \| `version` \| `edit` \| `compile` | How this save came to be. The first four start a draft; `edit` is the owner's change to one; `compile` is the compiler's result written into one |
| `base_draft` | `ref?` | The `draft` of the save this one replaces, as the API's `If-Match` named it (workspace API API-19): rule 69 |
| `base_version` | `ref?` | The stored mandate version a draft of origin `version` starts from: rule 69 |

**`ModelInvocationRecorded`** on the control stream: the compiler's model call. The compiler acts
for a user and has no agent, so its record is on the workspace's control stream, written by the
workspace services that called the gateway ([inference spec §3.6](inference.md#36-journaling)); this
answers inference spec §13 question 1. Its members are [DEC-432](../project/decisions/DEC-432.md)
item 11's, typed from inference spec §3.1 to §3.5. `config_refs.model_version` names the compiler's
registered model. On the agent stream it is closed in §9.19, with these members and the agent
harness's. Until a version 2 of this record makes `prompt_ref` nullable for `input_rejected` (rule
148's reading, DEC-998 item 2), the compiler makes no gateway call, since a guard-refused prompt
would otherwise be stored.

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The gateway's call (inference §3.1); its retries reuse it |
| `purpose` | `compiler` | The only purpose on the control stream |
| `draft_id` | `id` | The draft compiled |
| `draft` | `ref` | The saved draft the compiler read |
| `model` | `{model_id: text, model_version: text, content_hash: ref}` | The registry entry called: rule 71 |
| `endpoint` | `text` | The endpoint that served it |
| `request_digest` | `ref` | SHA-256 of the canonical request (inference §3.1), which is stored |
| `sampling` | `{temperature: decimal, seed: integer?}` | `seed` is `null` where the provider takes none |
| `prompt_ref` | `ref` | The rendered prompt |
| `response_ref` | `ref?` | The response, or `null` when no response bytes arrived: rules 72 to 74 |
| `reported_identity` | `text?` | What the provider says served the call (INF-2) |
| `provider_request_id` | `text?` | |
| `outcome` | `ok` \| `policy_denied` \| `budget_exhausted` \| `rate_limited_local` \| `meter_unavailable` \| `input_rejected` \| `model_withdrawn` \| `deadline_exceeded` \| `provider_unavailable` \| `rate_limited_provider` \| `credential_invalid` \| `content_refused` \| `schema_invalid` \| `identity_mismatch` | Inference §3.3. The API's `compile_failed` is `schema_invalid` |
| `attempts` | `integer` | Attempts sent to a provider: rules 73 and 74 |
| `tokens` | `{input: integer, output: integer, cached: integer}` | |
| `cost_usd` | `decimal` | From the price table in effect (inference §7.2): rule 75 |
| `price_table_ref` | `ref` | The price table version used |
| `cache_hit` | `boolean` | Served from the gateway's cache (inference §3.5): rule 74 |
| `deadline` | `timestamp` | The call's absolute deadline |
| `completed_at` | `timestamp` | When the gateway finished with it: rule 75 |

**`MandateConfirmed`** version 2: version 1's members, then the agent link (DEC-436 item 14). Rules
18 and §9.2's mapping apply to it as to version 1, and version 1 stays registered and unchanged
(§8). The workspace API writes version 2; a writer without an agent link (the CLI of E10-16) may
write version 1.

| Member | Type | Meaning |
|---|---|---|
| `mandate_version`, `confirmed_paths`, `record_ref` | As version 1's | |
| `agent_id` | `id?` | The deployed agent the version is for, or `null` for a new mandate |
| `base_version` | `ref?` | The agent's version in force when the owner confirmed (workspace API §5.1): rule 77 |

**`MandateConfirmed`** version 3 ([DEC-2131](../project/decisions/DEC-2131.md), v0.84): version 2's
members, then the passkey step-up that authorized the confirmation, so §9.32's rule 253 reads it
from the record as it reads every other authorized event's. Rules 18, 77, 78 and §9.2's mapping
apply to it as to version 2, and versions 1 and 2 stay registered and unchanged (§8).

| Member | Type | Meaning |
|---|---|---|
| `mandate_version`, `confirmed_paths`, `record_ref`, `agent_id`, `base_version` | As version 2's | |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: the passkey step-up the API judged, which the record `record_ref` names also holds: rule 267 |

**Who writes which version.** The workspace API writes version 3 exactly when it commits the
confirmation under **passkey** step-up, and the `StepUpAsserted` follows it in the same batch
(§9.32). Under any other step-up, or none, it writes version 2. The CLI writes version 1 or 2 as
before. Every reader of version 2 reads version 3 as version 2 and ignores `step_up`: what it needs
of the step-up is still the record's. A reader that matches version 2 alone would skip a version
3, so no writer writes version 3 until every reader reads it (DEC-2131 item 4).

**`OwnerRequestSubmitted`**: an owner's request for an order (workspace API §4.6). The runtime
hands it to the order builder, which sizes, clips, and classifies it as any proposal (§2).

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `instrument_id` | `asset_id` | |
| `side` | `buy` \| `sell` | |
| `quantity` | `decimal?` | The size asked for, or `null` for the builder to size: rule 80 |
| `requested_by` | `owner` \| `client` | Who asked, from the actor: rule 79. The builder carries it to `DecisionMade` (mandate spec §6.2 step 5a) |
| `client_id` | `id?` | The client that asked: rule 79 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

69. `MandateDraftSaved`: `base_draft` is `null` exactly when `origin` is `description`,
    `goal_answers`, `template`, or `version` (`payload.base_draft`); `base_version` is non-null
    exactly when `origin` is `version` (`payload.base_version`); and `causation_id` is non-null
    when `origin` is `compile`: the compiler's `ModelInvocationRecorded` (`causation_id`). The
    first that fails, in that order, is reported.
70. `MandateDraftSaved`: the actor is a `user`, or a `client` whose `origin` is `version`
    (`actor.kind`): a client's `propose` scope creates a draft from a base version and nothing
    else (workspace API §3.8).
71. `ModelInvocationRecorded` on the control stream: `model.content_hash` equals
    `config_refs.model_version` (`payload.model.content_hash`), as rule 38 binds a thesis's model.
    A missing ref is already `missing_config_ref`.
72. `ModelInvocationRecorded`: an `ok` has its response: `response_ref` and `reported_identity`
    are non-null, each reported at itself in that order.
73. `ModelInvocationRecorded`: a refusal (`policy_denied`, `budget_exhausted`,
    `rate_limited_local`, `meter_unavailable`, `input_rejected`, `model_withdrawn`) sent nothing
    (inference §3.3): `response_ref`, `reported_identity`, and `provider_request_id` are `null`,
    `attempts` is 0, every `tokens` member is 0 (reported at `payload.tokens`), and `cost_usd` is
    0; the first that fails, in that order.
74. `ModelInvocationRecorded`: `cache_hit` is true only when `outcome` is `ok`
    (`payload.cache_hit`), and a hit made no attempt and no provider request and cost nothing:
    `attempts` 0, `provider_request_id` `null`, and `cost_usd` 0, the first that fails in that
    order. Any other call that is not a refusal reached a provider: `attempts` ≥ 1
    (`payload.attempts`).
75. `ModelInvocationRecorded`: `cost_usd` ≥ 0 (`payload.cost_usd`), and an `ok` completed no later
    than its deadline: `completed_at` ≤ `deadline`, compared to the nanosecond
    (`payload.completed_at`). A response that completes late is `deadline_exceeded` and never used
    (inference INF-3).
76. `ModelInvocationRecorded` on the control stream: the actor is a `system` (`actor.kind`), the
    services that called the gateway.
77. `MandateConfirmed` versions 2 and 3: `base_version` is `null` exactly when `agent_id` is, and differs
    from `mandate_version` (`payload.base_version`): a confirmation for a deployed agent names the
    version it replaces, which is never itself.
78. `MandateConfirmed` versions 2 and 3: the actor is a `user` (`actor.kind`). Only a user confirms
    (workspace API §5.1, identity spec ID-11).
79. `OwnerRequestSubmitted`: the actor is a `user` or a `client` (`actor.kind`); `requested_by`
    is `owner` for a user and `client` for a client (`payload.requested_by`); and `client_id` is
    the actor's `id` for a client and `null` for a user (`payload.client_id`). So who asked is the
    authenticated principal's, never the request body's (workspace API API-6, DEC-185 item 2).
80. `OwnerRequestSubmitted`: a non-null `quantity` is greater than 0 (`payload.quantity`).
267. `MandateConfirmed` version 3 (v0.84): its `step_up` is a passkey's, fresh at the envelope's
     instant (identity spec §7.1): `method` is `passkey` (`payload.step_up.method`), and 0 ≤
     `event_time` − `authenticated_at` ≤ 300 seconds, compared to the nanosecond, so evidence
     authenticated after the instant fails closed (`payload.step_up.authenticated_at`), as rule 102
     judges it. That the assertion is the one its `StepUpAsserted` records is §9.32's rule 253, and
     that it was never used before is rule 254's.

These records map to no `JournaledFact` except `MandateConfirmed` versions 2 and 3, which map as
version 1 does (§9.2), from the same members.

### 9.10 Connection revocation and client records ([DEC-671](../project/decisions/DEC-671.md))

Closed as §9.9's are, on the control stream. Rules number on from §3's 81 to 83. No member carries
a credential or a token: a client is named by its opaque ID and scopes, and a connection by its ID
(§9.2).

**`ConnectionRevoked`** version 2: version 1's member, then why and with what step-up. Version 1
stays registered and unchanged (§8); the workspace API writes version 2.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |
| `reason` | `owner` \| `compromised` | `owner`: the ordinary revoke, which the API refuses while an agent on the connection holds positions or is not stopped (workspace API §4.5). `compromised`: the revoke now of workspace API §5.6, which follows its kill switch: rule 84 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: a revocation needs step-up (identity spec ID-4). Without it the API commits only the kill switch (workspace API §5.6) |

**`ClientConnected`**: a user issued a client its token (identity spec §12.1, workspace API §3.8).

| Member | Type | Meaning |
|---|---|---|
| `client_id` | `id` | The client, as its actor's `id` names it |
| `user` | `text` | The user it acts for (opaque), its actors' `on_behalf_of`: rule 88 |
| `scopes` | `[read \| request \| propose \| dry_run \| hold]` | Workspace API §3.8's closed list: rules 86 and 87 |
| `agents` | `[id]` | The agents it may see: rules 86 and 87 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: connecting a client needs step-up (identity spec ID-4) |

**`ClientRevoked`**: a client's token revoked. It needs no step-up, because it only removes access
(identity spec ID-5); revocation takes effect for every request authorized after it commits (ID-11).

| Member | Type | Meaning |
|---|---|---|
| `client_id` | `id` | |
| `user` | `text` | The user it acted for (opaque) |
| `reason` | `owner` \| `admin` \| `member_deactivated` \| `deprovisioned` \| `compromised` | Who ended it and why: the client's own user, a workspace admin (identity spec §4.2), the deactivating admin or the system on a member's deactivation (identity §5.2), the system on deprovisioning (§11.1), or a suspected compromise: rule 89 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

84. `ConnectionRevoked` version 2: `causation_id` is non-null exactly when `reason` is
    `compromised` (`causation_id`); an ordinary revoke has no cause. A compromised revocation's
    `causation_id` names an `OwnerCommandIssued` earlier in the same `append` batch whose `command`
    is `kill_switch`, whose `scope` is `connection`, and whose `subject` is the revoked
    `connection_id` (`causation_id`, as rule 45 checks its pair inside one batch). So the record
    shows this connection's kill switch ran first (workspace API §5.6), and a revocation citing any
    other event, a wider kill switch, or a cause outside its batch is refused. These are the
    members the CLI writes on a kill switch today; no §11 check repeats the clause, since a range
    never splits a batch.
85. `ConnectionRevoked` version 2: the actor is a `user` (`actor.kind`).
86. `ClientConnected`: `scopes` and `agents` are non-empty (the first that is empty).
87. `ClientConnected`: `scopes` and `agents` are each strictly ascending by bytes (`non_canonical`
    at the list, `scopes` first).
88. `ClientConnected`: the actor is a `user` (`actor.kind`), and `user` is its `id`
    (`payload.user`): a user connects their own client (identity spec §4.2).
89. `ClientRevoked`: the actor follows the `reason` (`actor.kind`): a `user` for `owner` and
    `admin`, the `system` for `deprovisioned`, and either a `user` or the `system` for
    `member_deactivated` (the deactivating admin, or a scheduled deactivation) and `compromised`. For `owner`, `user` is the actor's `id`; for `admin`, it is not
    (`payload.user`).

`ConnectionRevoked` version 2 maps to the `ConnectionRevoked` fact as version 1 does (§9.2). The
client records map to no `JournaledFact`.

### 9.11 The hold on new openings ([DEC-672](../project/decisions/DEC-672.md))

An owner, or a client the owner connected with the `hold` scope, may hold an agent's new openings:
the agent goes to `exits_only` and keeps every exit running. Only the owner lifts it, with step-up
([DEC-191](../project/04-decision-log.md#decisions), [mandate spec §6.1](mandate.md#61-purposes)).
The hold is owner state beside the lifecycle of §9.1's `AgentModeChanged`: it survives a pause, a
resume, and a restriction lifting, so the agent stream records it on every mode change. Closed as
§9.7's records are; times are integer risk-clock seconds as §9.7's are (DEC-533 item 2).

**`OwnerCommandIssued`**, for `hold_openings` and `lift_hold` only. The members, in order, are
the ones the CLI already writes for every owner command (`mandate-cli`'s `agent::issued`), so the
other commands close later (M7's agent-commands slice) without a member renamed; here the Stop's
and the owner exit's members are always `null`. **Every other command stays open at this
version:** `append` catalogues `OwnerCommandIssued` on the control stream, reads its `command`
(text, else `schema` at `payload.command`), refuses it from a client (rule 90), and judges nothing
else about it. So a pause or a kill switch is recorded from any other principal, including the
host CLI's `system` actor (identity spec §6.4), and is never refused for its members (`AGENTS.md`
rule 13); rule 84's batch reads a kill switch's `command`, `scope`, and `subject` from it.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent held or released |
| `command` | `hold_openings` \| `lift_hold` | |
| `scope` | `agent` | A hold is per agent |
| `subject` | `id` | The scope's subject, the agent itself: rule 91 |
| `release`, `warning_shown` | `null` | A Stop's release choice and warning; none here |
| `bid`, `bid_size`, `floor` | `null` | An owner exit's confirmed bid; none here |
| `user` | `text` | The human who asked (opaque): rule 91 |
| `submitted_at` | `integer` | The risk-clock second it was committed at |
| `step_up` | `{assertion_id: text, authenticated_at: integer, method: text}?` | A lift's evidence, judged by the runtime; `null` for a hold: rule 92 |

**`AgentModeChanged`** version 2 on the agent stream: version 1's members, with the reasons
`owner_hold` and `owner_lift_hold`, then `held`. Version 1 stays registered and unchanged (§8), and
a runtime that records a hold writes version 2 for every mode change after it.

| Member | Type | Meaning |
|---|---|---|
| `from`, `to`, `lifecycle` | As version 1's | `to` obeys rule 94, which replaces rule 11 at this version |
| `reason` | version 1's reasons \| `owner_hold` \| `owner_lift_hold` | |
| `held` | `boolean` | Whether the owner's hold stands after the change: rule 93 |

**`OwnerCommandRefused`** version 2: version 1's members, with the command `lift_hold`, on the agent
stream only (rule 26). Version 2 is registered on the agent stream alone, so a version-2 refusal on
the account stream is `unknown_schema` at `payload`, and rule 26's `stream_mismatch` judges version
1's commands. A lift whose step-up does not count is refused like a resume (rule 28 keeps
`not_independent` off it).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

90. `OwnerCommandIssued`: a `client` actor's command is `hold_openings` (`actor.kind`): a client
    never lifts a hold, pauses, resumes, stops, exits, or engages the kill switch (identity spec
    ID-11, DEC-141 item 3). A `hold_openings` is a `user`'s or a `client`'s, and a `lift_hold` a
    `user`'s (`actor.kind`). No other actor is constrained on another command, so the host CLI's
    kill switch is never refused here.
91. `OwnerCommandIssued`: `user` is the actor's human (§3) (`payload.user`), and `subject` is
    `agent` (`payload.subject`).
92. `OwnerCommandIssued`: a `hold_openings` has a `null` `step_up` (`payload.step_up`). A hold only
    reduces risk, so it needs none (`AGENTS.md` rule 2), and a client never presents one.
93. `AgentModeChanged` version 2: `held` is true when `reason` is `owner_hold` and false when it is
    `owner_lift_hold` (`payload.held`).
94. `AgentModeChanged` version 2: `to` is at least as strict as `lifecycle`, and at least
    `exits_only` while `held` (`payload.to`; `normal` < `exits_only` < `paused` < `stopped`). So a
    resume never clears a hold, and a lift never clears a pause. A lift that leaves `to` at
    `exits_only` is valid: a latched limit or a restriction may still hold the agent (MI-3).
95. `AgentModeChanged` version 2: a copy of an owner command, whose `reason` is `owner_pause`,
    `owner_resume`, `owner_stop`, `owner_hold`, or `owner_lift_hold`, has a non-null
    `causation_id`: its `OwnerCommandIssued` (`causation_id`). As rule 16 does at version 1, each
    command has one copy: its `AgentModeChanged`, or, for a lift, its `OwnerCommandRefused`.

**Carried, never dropped.** `held` changes only on `owner_hold` and `owner_lift_hold`: every other
version-2 copy (a restriction, a reconciliation, a kill switch, a pause, a resume, a Stop) carries
the last one's, and once a stream holds a version-2 `AgentModeChanged` every later one is version
2. `append` sees one record and cannot check either, so §11's per-range check `held_mismatch` does,
anchored on the stored chain before the range (or failing closed without it), and `mandate journal
verify` reports it; a stray `held: false` would otherwise drop the hold on replay.

**Lifecycle.** A hold is copied whatever the mode: on a paused or stopped agent the copy records
`held` with `to` unchanged, so command status reaches `applied`. A second hold, or a lift with no
hold standing, is copied too, with `held` as it already stood. A latched limit, a restriction, or a
version change never sets or clears the hold, and a restart folds it from the last copy.

### 9.12 Membership records ([DEC-437](../project/decisions/DEC-437.md) item 9, [DEC-648](../project/decisions/DEC-648.md))

The control-stream records of a workspace membership ([identity spec §5](identity.md#5-membership-lifecycle),
§12.1), closed at schema version 1 as §9.7's records are: every listed member is present, `null`
only where the type is nullable, and any extra member is refused. §9.1's types and report order
apply, and the rules number on from §9.11's. Instants are §4.7 timestamps, as the identity spec's
cool-off and expiry are wall-clock times. A record's own instant (`invited_at`, `activated_at`,
`changed_at`, `reactivated_at`) is the envelope's `event_time` exactly (rule 106), so a writer cannot
date a grant into the past to escape its cool-off or a stale step-up; every cool-off end and step-up
window is computed from `event_time`. Step-up evidence is §9.2's `DisclosureAccepted.step_up`
type, `null` only on a `MemberRoleChanged` that grants nothing (rule 101). The test vectors' `membership` section holds a base draft
of each record, an invalid draft for every member type and rule, and valid drafts for the cases a
rule might be misread to refuse.

The control stream is the workspace's, so no record names its workspace. A member is the opaque
ULID of a `user` principal (identity spec §3.1); clients, service accounts, agents, and platform
staff hold no membership. No payload carries an address, a name, an identity-provider subject, or an
invitation token (rule 6, ID-9, §6.4): an invitation is its opaque ULID, and the invited address is a
vault reference in the envelope's `pii_refs`.

Every membership record ends with `session_ref` (`text?`): the opaque reference of the session the
writing user acted through ([identity spec §6.2, §12.2](identity.md#122-actor-fields)), never a cookie
or a token, and `null` for a record the system writes (rule 105). It is a payload member here only.
Identity spec §12.2 asks every committed event to name its session, which is an envelope field and so
a new `envelope_version` (§8); that change is owed separately (DEC-648 item 8). Organization memberships and org roles close with the
organization story, not here.

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `role` | `approver` \| `auditor` \| `operator` \| `viewer` \| `workspace_admin`: the workspace roles of [identity spec §4.1](identity.md#41-roles) | As `id` |

**`MemberInvited`**: a workspace admin invited an address (identity spec §5.2).

| Member | Type | Meaning |
|---|---|---|
| `invitation` | `ulid` | The invitation |
| `roles` | `[role]` | The roles it names, granted only once accepted: rules 96 and 100 |
| `invited_by` | `text` | The inviting admin (opaque): rules 97 and 98 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Inviting needs step-up (identity spec §4.2): rule 102 |
| `invited_at` | `timestamp` | The instant the invitation was issued: rule 106 |
| `expires_at` | `timestamp` | The invitation expires unaccepted at this instant: rule 104 |
| `session_ref` | `text?` | Rule 105 |

**`MemberInvitationRevoked`**: an admin revoked an invitation before it was accepted (identity spec
§5.1, `invited` to `revoked`). An expired invitation needs no record: its `expires_at` ends it.

| Member | Type | Meaning |
|---|---|---|
| `invitation` | `ulid` | |
| `revoked_by` | `text` | The admin (opaque): rules 97 and 98 |
| `session_ref` | `text?` | Rule 105 |

**`MemberActivated`**: a membership begins, by an accepted invitation or by the founding grant that
creates a workspace (identity spec §3.2, ID-13). It enters `cooling_off`, and is `active` from
`cool_off_ends_at` (identity spec §5.1).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | The user principal |
| `invitation` | `ulid?` | The invitation accepted, or `null` for the founding grant: rule 99 |
| `reason` | `invitation_accepted` \| `founding` | Rules 98 and 99 |
| `roles` | `[role]` | Rules 96, 99, and 100 |
| `method` | `passkey` \| `oidc` \| `email_link` | How the member signed in (identity spec §6.1) |
| `activated_at` | `timestamp` | The instant the membership began: rule 106 |
| `independent_approval_required` | `boolean` | The workspace's effective `independent_approval_required` at the grant ([mandate spec §4.3](mandate.md#43-policy-hierarchy-dec-51-dec-98)), which decides the cool-off: rule 103 |
| `cool_off_ends_at` | `timestamp` | The end of the activation's cool-off ([identity spec §8.3](identity.md#83-cool-off-against-sock-puppets)): rule 103 |
| `session_ref` | `text?` | The invitee's session; `null` for the founding grant: rule 105 |

**`MemberRoleChanged`**: a workspace admin granted or removed roles of another member.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `changed_by` | `text` | The admin (opaque): rules 97 to 99 |
| `added` | `[{role: role, cool_off_ends_at: timestamp}]` | Each role granted, and the end of its cool-off: rules 96, 100, and 103 |
| `removed` | `[role]` | Rules 96 and 100 |
| `changed_at` | `timestamp` | The instant of the change: rule 106 |
| `independent_approval_required` | `boolean` | As on `MemberActivated`: rule 103 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | A grant needs step-up and a removal does not: rules 101 and 102 |
| `session_ref` | `text?` | Rule 105 |

**`MemberDeactivated`**: the member loses the workspace (identity spec §5.2, §11.1).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | Who committed it (opaque): rules 97 to 99 |
| `reason` | `admin` \| `left` \| `deprovisioned` \| `group_removed` | An admin deactivated them, they left, the customer's directory deprovisioned them (SCIM), or an identity-provider group removal did (identity spec §11.1) |
| `session_ref` | `text?` | Rule 105 |

**`MemberReactivated`**: an admin reactivated a deactivated member, with its kept roles. It enters
`cooling_off` again. It names the roles it restores, so its cool-off is decided by the record alone.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | The admin (opaque): rules 97 to 99 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Reactivating needs step-up (identity spec §5.1): rule 102 |
| `roles` | `[role]` | The kept roles it restores, exactly those the member held when deactivated: rules 96 and 100 |
| `reactivated_at` | `timestamp` | Rule 106 |
| `independent_approval_required` | `boolean` | As on `MemberActivated`: rule 103 |
| `cool_off_ends_at` | `timestamp` | Rule 103 |
| `session_ref` | `text?` | Rule 105 |

**`MemberRemoved`**: a deactivated member is removed. Identity records are kept for the records
period (§6.4).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | Who committed it (opaque): rules 97 to 99 |
| `reason` | `admin` \| `org_deleted` | An admin removed them, or the organization's deletion ended every membership (identity spec §5.2) |
| `session_ref` | `text?` | Rule 105 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

96. Role lists are strictly ascending by bytes, so no role is listed twice: `roles`, `removed`, and
    `added` by its `role` (`non_canonical` at the list).
97. The writer: `invited_by`, `revoked_by`, `changed_by`, and `by` equal the envelope's `actor.id`
    (at that member), so a record names the identity that wrote it (ID-1).
98. Who writes it (`actor.kind`): a `user` for `MemberInvited`, `MemberInvitationRevoked`,
    `MemberRoleChanged`, and `MemberReactivated`, for `MemberActivated` with reason
    `invitation_accepted`, for `MemberDeactivated` with reason `admin` or `left`, and for
    `MemberRemoved` with reason `admin`; `system` for the founding grant (ID-13), for
    `MemberDeactivated` with reason `deprovisioned` or `group_removed`, and for `MemberRemoved` with
    reason `org_deleted`. A client never changes a membership (ID-11).
99. Nobody changes their own membership but by leaving (ID-13). `MemberRoleChanged.changed_by`, and
    `by` on `MemberReactivated`, on `MemberDeactivated` with reason `admin`, and on `MemberRemoved`
    with reason `admin`, differ from `member` (at that member). `MemberDeactivated` with reason
    `left` has `by` equal to `member` (`payload.by`). `MemberActivated`: `invitation` is `null`
    exactly when `reason` is `founding` (`payload.invitation`), and an accepted invitation's
    `member` equals `actor.id`, the invitee who signed in (`payload.member`).
100. Roles: `MemberInvited.roles`, `MemberActivated.roles`, and `MemberReactivated.roles` are
    non-empty (`payload.roles`); the
    founding grant's include `workspace_admin`, since a workspace always has an `active` admin
    (identity spec §5.2; `payload.roles`); a `MemberRoleChanged` adds or removes at least one role
    (`payload.added`), and no role is both added and removed (`payload.removed`).
101. `MemberRoleChanged.step_up` is non-null exactly when `added` is non-empty (`payload.step_up`).
102. Step-up evidence is [mandate spec §6.1](mandate.md#61-purposes)'s, valid at the envelope's
    `event_time`: its method is `passkey` in a `live`
    envelope and `passkey` or `cli_confirm` otherwise ([identity spec §7.3](identity.md#73-rules);
    `payload.step_up.method`), and 0 ≤ `event_time` − `authenticated_at` ≤ 300 seconds, so evidence
    authenticated after the instant fails closed (`payload.step_up.authenticated_at`). That the
    assertion was never used before is a check across records, the identity crate's (identity spec
    §7.2).
103. Cool-off ([identity spec §8.3](identity.md#83-cool-off-against-sock-puppets), stated once there),
    exactly: each cool-off end is the envelope's `event_time` plus 86 400 seconds when the record's
    `independent_approval_required` is true and the grant adds `operator` or `approver` to an existing
    workspace, and is `event_time` itself otherwise (at that `cool_off_ends_at`). The grant adds them
    when a `MemberActivated` with reason `invitation_accepted` or a `MemberReactivated` holds either in
    its `roles`, or a `MemberRoleChanged` adds that role (each added role decided by itself). The
    founding grant creates the workspace, so it never cools off.
104. `MemberInvited.expires_at` is `invited_at` plus exactly 7 days (identity spec §5.2;
    `payload.expires_at`).
105. `session_ref` is non-null exactly when `actor.kind` is `user` (`payload.session_ref`): a user acts
    through a session, and the system writes without one.
106. A record's own instant (`invited_at`, `activated_at`, `changed_at`, `reactivated_at`) equals the
    envelope's `event_time` exactly (at that member).

**The fold** (identity spec §5.1, §5.3, ID-7). These records are the only source of a membership's
state. Folding the control stream in `seq` order: `MemberInvited` makes an invitation `invited`
before `expires_at` and `expired` from it; `MemberInvitationRevoked` makes it `revoked`; `MemberActivated` and
`MemberReactivated` make the member `cooling_off` until their `cool_off_ends_at` and `active` from it;
`MemberRoleChanged` adds and removes roles, each added role effective from its `cool_off_ends_at`
and each removal at once; on an activation or reactivation, `operator` and `approver` are effective
from its `cool_off_ends_at` and every other role at once;
`MemberDeactivated` makes the member `deactivated`, keeping its roles for a reactivation; and
`MemberRemoved` makes it `removed`. A `MemberRoleChanged` whose `added` is empty may remove kept
roles from a `deactivated` member, so an admin can strip a suspended member's roles while offboarding
([DEC-654](../project/decisions/DEC-654.md) item 7); a grant to a `deactivated` member is still
refused. A deactivated member whose kept roles are all removed cannot be reactivated, since rule 100
refuses a `MemberReactivated` with no roles: they return only by removal and a new invitation. `removed`, `expired`, and `revoked` are terminal: a removed
member comes back only through a new invitation, which starts a new membership with no role of the
old one. An invitation an activation used is `accepted`, the fold's own name for it: identity spec
§5.1 has no such state, because the membership it started takes over. A record that does not fit the
state it finds is refused: a `MemberInvited` whose invitation ULID was already issued; any record
naming an invitation never issued or a member never activated; a second activation of a member
whose membership is not `removed`; an activation by an invitation that is not `invited` at
`activated_at` (so `activated_at` < `expires_at`, and an invitation activates at most once) or with
roles other than the invitation's; a role change for a member who is not `active` or `cooling_off`,
other than a removal only from a `deactivated` member, or that removes a role the member does not
hold (for a `deactivated` member, one it does not keep) or adds one it does; a reactivation of a member who
is not `deactivated`, or with roles other than those kept; a deactivation of one who is not
`active` or `cooling_off`; a removal of one who is not `deactivated`; and a revocation of an
invitation that is not `invited`. Such a record is refused by workspace services before it is committed; a fold that meets one anyway reads the
membership as unreadable, and `workspace_users` as 1 (identity spec §5.3). The cross-record checks
are the identity crate's (E9-7), not `append`'s. Besides the state checks above, they are: the
author of an admin's record is an `active` member whose `workspace_admin` role is effective at
`event_time` ([DEC-654](../project/decisions/DEC-654.md) item 2); the last owner and last admin
rules (identity spec §5.2); that a step-up assertion was never used before (rule 102); that a
record's `independent_approval_required` equals the workspace's effective policy at `event_time`
(rule 103); and that no record carries an entry that changes nothing: `change_roles` passes one
(DEC-654 item 6), but the fold refuses granting a held role or removing one not held, and rule 100
refuses a record left with no entry, so workspace services drop such entries before committing and
commit nothing when none is left.

The state at an instant *t* folds the records whose `event_time` is at or before *t* (rule 106 makes
that each record's own instant), and reads each cool-off and expiry against *t*: `workspace_users`
(identity spec §5.3) counts the members `active` at *t*. The test vectors' `membership_fold` section
holds histories of membership records, valid under the rules above except a record a history lists
in `refused` with the refusal it expects (which the fold reads as out of state too), mixed with records by clients,
agents, and service accounts, which hold no membership and which the fold ignores. Each history
states, at named instants, each member's and invitation's state, each member's effective roles, and
the count, written by hand from identity spec §5 and §8.3. It also holds out-of-state histories that
read as unreadable, with a count of 1. The reference fold
([reference/journal/membership_fold.py](../../reference/journal/membership_fold.py)) reproduces
every one, and the identity crate's fold is tested against them.

**Order** ([DEC-659](../project/decisions/DEC-659.md), [DEC-657](../project/decisions/DEC-657.md)
item 4). A late event keeps its arrival `seq` (§8), so a membership record committed late, or after
the writer's clock stepped back, would follow one with a later `event_time`. The fold compares each
membership record with the membership record before it in `seq` order, whether or not either is at
or before the instant read: a `seq` not above that record's, or an `event_time` before it, is out of
order. An equal `event_time` is in order, by `seq`. A record out of order makes the fold unreadable
from its `event_time`, so `workspace_users` reads 1 from then on, as for a record that does not fit
its state, and a reading before that instant is unchanged. Records of other types are not compared:
their instants say nothing about the membership records' order. That is the fold's backstop. The
writer keeps a stream from reaching it:

- **One writer.** One writer commits the seven `Member*` types on `ctl:{workspace_id}`
  ([DEC-646](../project/decisions/DEC-646.md)); workspace services call it after their own checks
  and never append a membership record themselves.
- **Inside the append.** It checks order within the serialized append (§5.1, per control stream)
  that assigns the record's `seq` and commits it, against `last`, the stream's last record of the
  seven membership types as read in that append, and takes the record's `event_time` there too. A
  store that cannot serialize the append per workspace commits no membership record.
- **The rule.** It refuses the record when its `event_time` is before `last`'s or its `seq` is not
  above `last`'s; an equal `event_time` passes. The first membership record (there is no `last`)
  passes, whatever its `seq` and instant. Every one of the seven types is checked alike, as the
  record and as `last`, the founding grant included.
- **Refused, never clamped.** A refused record commits nothing. The writer never moves its
  `event_time` up to `last`'s, which rule 106 forbids and which would move the cool-off or expiry
  the record decides, and never commits it unchecked. The caller retries later, and nothing is
  granted meanwhile: after the writer's clock steps back, every membership record is refused until
  the clock is back at `last`'s `event_time`.
- **A moved head re-runs the check.** When the append finds the head moved (`HeadMismatch`), nothing
  is written, and the next attempt reads the head and `last` again, takes `event_time` again, and
  checks again, so a `last` read before a concurrent commit never admits a record. After a bounded
  number of such attempts the writer commits nothing and the caller retries later.

A writer that keeps the rule never commits a record the fold reads as out of order, so one late
membership record cannot latch a healthy workspace's `workspace_users` at 1 (ID-7). Like the other
cross-record checks, the rule is the identity crate's (E9-7), not `append`'s. Every
`membership_fold` history is in `seq` and `event_time` order, so the rule changes no vector; the
identity crate's tests hold the out-of-order cases.

### 9.13 Records access, export, and verification records ([DEC-780](../project/decisions/DEC-780.md))

The control stream's records of who read the records outside the product views, what was exported,
and what was verified (§7, §11, §12), closed at schema version 1 as §9.7's records are, and
`VerificationRun` at version 2 too (below): every listed
member is present, `null` only where the type is nullable, and any extra member is refused. §9.1's
types apply, with two more below. The rules number on from §9.12's. None of the three names a
configuration.

**Writers.** The control stream's single writer, workspace services (§2), appends all three. The
workspace API's control-stream writer appends a `RecordsAccessed` or an `ExportCreated` before it
serves the records or the export ([workspace API spec](workspace-api.md) §4.8, API-16), so a read
or export that cannot be journaled is not served. It appends a `VerificationRun` for a verification
a principal requested (`POST /verifications`) once the run ends and before its result is served.
Workspace services append one for each scheduled run of §11 and for a restore drill's run. The
`actor` is the principal that asked: a `user`, a service account (`system`, its opaque ID), a
`client` for its own reads (§3, rule 83; of these three records, `RecordsAccessed` only), or, for a scheduled run, the
verifying process (`system`). Every identifier is opaque (§6.4).

**What they carry.** A payload names streams by §2's identifiers, positions by `seq`, and contents
by hash: stream ranges, digests, artifact references, check codes, and opaque IDs. It never
carries an instrument, an order, a position, a quantity, a price, or mandate content, so a record of
an audit read discloses nothing the read itself did not authorize. **Tenant isolation:** every range
names a stream of the control stream's own workspace (rule 107). A trace that follows a
`causation_id` lists every stream it read and never follows one into another workspace's stream; a
request that would reach one is answered as absent ([workspace API spec](workspace-api.md) API-9)
and journals nothing about that stream.

**Types.**

| Type | Values | Refused as |
|---|---|---|
| `stream_id` | §2's form: `acct:{workspace_id}:{account_ref}`, `agent:{workspace_id}:{agent_id}`, `ctl:{workspace_id}`, `clock:{workspace_id}`, `ntf:{workspace_id}`, `meter:{workspace_id}`, `dmeter:{deployment_id}` (§9.19), or `news:{workspace_id}` (§9.37, since v0.103), each segment `[A-Za-z0-9_-]+` | As `id` |
| `digest` | 64 lowercase hex SHA-256: an event's `hash` or `prev_hash` (§3), or a digest of bytes the journal does not store, such as §12's verifier digest. It is not a `ref`: it names no stored artifact, so it is not listed in `artifact_refs` and §11 check 6 does not read it | As `id` |

**`range`**, the unit every record covers: one stream's events from `from_seq` to `to_seq`, both
included.

| Member | Type | Meaning |
|---|---|---|
| `stream_id` | `stream_id` | The stream: rule 107 |
| `from_seq`, `to_seq` | `integer` | The first and last `seq` covered |
| `prev_hash` | `digest` | The hash before `from_seq`: the trusted start (§11), 64 zeros for seq 1 |
| `to_hash` | `digest` | The hash of the event at `to_seq`, the head the read or export reflects (workspace API API-14) |

**`RecordsAccessed`**: a read outside the product views, journaled before it is served (§7).

| Member | Type | Meaning |
|---|---|---|
| `accessor` | `text` | The principal that read (opaque): rule 108. A client is its own accessor, and the human it acts for is the envelope's `actor.on_behalf_of` (§3, rule 81; [workspace API spec](workspace-api.md) §3.3, §3.8) |
| `operation` | `id` | The workspace API operation that read, by name ([workspace API spec](workspace-api.md) §4) |
| `ranges` | `[range]` | Every stream range the response was built from, with the hashes it reflects |
| `resources` | `[id]` | The opaque IDs of the other resources the operation read (an agent, a notice, an approval), strictly ascending by bytes: rule 108. Never a name, a ticker, or any other content |
| `result` | `ref?` | The response served, stored as an artifact (§6.3), when the operation keeps it (a replay's result, [workspace API spec](workspace-api.md) §4.1); otherwise `null` |

**`ExportCreated`**: an export (§12), journaled before it is served (API-16). Its `event_id` is the
export's ID.

| Member | Type | Meaning |
|---|---|---|
| `form` | `canonical` \| `json` \| `csv` | The canonical export, or a JSON-lines or CSV view derived from one ([workspace API spec](workspace-api.md) §4.8). The examination bundle is not a form at this version |
| `ranges` | `[range]` | The ranges exported, each with its trusted start and its head as the export's segment manifests (§6.2) record them |
| `verifier_digest` | `digest` | The canonical export's verifier digest (§12, [DEC-265](../project/04-decision-log.md#decisions) item 3): one SHA-256 over every segment manifest and file, anchor root, and timestamp token, length-prefixed. A view names the digest of the canonical export it is derived from. Anyone holding the export recomputes it |
| `view` | `digest?` | The SHA-256 of the view's bytes as served: rule 109 |

**`VerificationRun`**: one run of §11's verification and its result. Its `event_id` is the
verification's ID: for a requested run, the `verification_id` that `POST /verifications` returns
and `GET /verifications/{id}` resolves ([workspace API spec](workspace-api.md) §4.8.1,
[DEC-788](../project/decisions/DEC-788.md) item 2).

**Versions.** Version 2 (v0.36, [DEC-788](../project/decisions/DEC-788.md) item 1 and
[DEC-789](../project/decisions/DEC-789.md)) is version 1 with three members added to each range,
`start`, `checked`, and `incomplete`, and with `incomplete` as a third `result`, so a reader can
rebuild a run's trusted start, its count of events walked, and a check it could not finish from
the record alone. Every writer appends version 2. Version 1 is not edited (§8), stays registered,
and keeps replaying; no writer appends it, and none did before v0.36. Rules 107, 110, and 111
judge both versions, rule 112 version 1, and rules 132 and 133 version 2.

| Member | Type | Meaning |
|---|---|---|
| `trigger` | `startup` \| `segment_export` \| `weekly` \| `request` \| `restore_drill` | Why it ran: §11's schedule, a principal's request, or a restore drill: rule 110 |
| `ranges` | `[checked_range]` | Every range verified, each with its own result |
| `result` | `pass` \| `fail`; at version 2, `pass` \| `incomplete` \| `fail` | Rule 112; at version 2, rule 133 |

A `checked_range` is a `range` whose `to_hash` is `digest?` (the head the run verified, or
`null` when a failure left none: rule 111), followed by, at version 1, `failure`, and at version
2, `start`, `checked`, `failure`, and `incomplete`:

| Member | Type | Meaning |
|---|---|---|
| `start` | `{kind: genesis \| manifest \| anchor, manifest_hash: digest?, anchor_event_id: ulid?}` | Version 2. The trusted start the range was entered from (§9.14 "What a verifier reads"): seq 1 with 64 zeros, the `SegmentExported` whose `manifest_hash` this is, or the `AnchorComputed` whose `event_id` this is. The member of the kind named is non-null and the other is `null`: rule 132 |
| `checked` | `integer` | Version 2. The number of events the run walked from `from_seq` (§11; workspace API AU-8): rule 132 |
| `failure` | `{check, seq: integer?}?` | The first failure §11 reports for the range, or `null`. `check` is one of §11's codes: `non_canonical`, `column_mismatch`, `seq_gap`, `rehash_mismatch`, `prev_hash_mismatch`, `artifact_missing`, `artifact_mismatch`, `anchor_head_mismatch`, `anchor_root_mismatch`, `tsa_token_invalid`, `segment_manifest_mismatch`, `segment_gap`, `anchor_self_mismatch`, `break_glass_cause_mismatch`, `intent_action_mismatch`, `mode_event_mismatch`, `held_mismatch`, `connection_lifecycle_mismatch`, `connection_cause_mismatch`, `approval_deferral_mismatch` (v0.105), and at version 2 only `segment_rows_mismatch` (§11, [DEC-894](../project/decisions/DEC-894.md)); `seq` is where it was reported, for a per-event check the position walked (the previous `seq` plus 1, or `from_seq` first), whatever the body there says: rule 111 |
| `incomplete` | `{check: tsa_token_invalid, cause: token_unverifiable \| anchor_unstamped}?` | Version 2. The check §11 could not finish for the range, named by its code, and why, or `null` (§11 "Incomplete", [DEC-789](../project/decisions/DEC-789.md) items 7 and 9). At this version only the token check can be incomplete. `cause` is `token_unverifiable` for a stamped token whose imprint matches, or a start anchor's token whose artifact store cannot be read ([DEC-896](../project/decisions/DEC-896.md) item 3), and `anchor_unstamped` for an anchor the range checks whose `token` is `null`; a range with both records `anchor_unstamped`. It names a check reported for the range, so it carries no `seq` (rule 111's classes); a range that failed records its failure instead: rule 132 |

A verification a principal requested for an export names that export's `ExportCreated` as its
`causation_id`. A failed run does not repair anything and does not replace §11's incident path:
`IntegrityIncidentRecorded` still records it. An `incomplete` run is not a failure: it starts no
incident (§11 "Incomplete").

**Consistency rules** (reason `schema`; the path is the member named):

107. Every record's `ranges` is non-empty (`payload.ranges`). Each range, in array order, with its
    clauses in this order, at `payload.ranges[i].<member>`:
    its `stream_id`'s `{workspace_id}` segment equals the envelope `stream_id`'s (`stream_id`), so
    no record names another workspace's stream; `from_seq` is at least 1 (`from_seq`); `to_seq` is
    at least `from_seq` (`to_seq`); `prev_hash` is 64 zeros exactly when `from_seq` is 1
    (`prev_hash`); and after the first range, its `stream_id` sorts after the previous range's by
    bytes, or equals it with a `from_seq` greater than the previous range's `to_seq` (`stream_id`),
    so ranges are ordered and never overlap.
108. `RecordsAccessed`: `accessor` equals the envelope's `actor.id` (`payload.accessor`);
    `actor.kind` is neither `agent` nor `broker` (`actor.kind`), so a `client` actor, in §3's one
    shape (rules 81 to 83), records its own reads with its own `id` as `accessor`; a
    `platform_operator`'s read has a non-null `causation_id`, the `PlatformOperatorAction` that
    opened its customer-approved break-glass window (§7) (`causation_id`); and each of `resources`
    sorts after the one before it by bytes (`payload.resources`). `append` cannot see what that
    `causation_id` names, so §11's `break_glass_cause_mismatch` checks it; until DEC-261 item 9
    closes `PlatformOperatorAction`, every operator read this rule admits fails that check. Rule 83 already keeps
    a client off `ExportCreated` and `VerificationRun`, so rules 109 and 110 refuse it again only as a
    second statement of workspace API §3.8 (a client never exports or verifies).
109. `ExportCreated`: `actor.kind` is `user` or `system` (`actor.kind`), and `view` is non-null
    exactly when `form` is `json` or `csv` (`payload.view`).
110. `VerificationRun`: `actor.kind` is `system`, or, for a `request`, `user` or `system`
    (`actor.kind`). Rules 109 and 110 refuse a `platform_operator`, so a §7 break-glass export or
    verification by platform staff cannot be journaled and is not served: it fails closed on
    purpose. Platform staff's break-glass reads are journaled as `RecordsAccessed` (rule 108 admits
    them); an export or a verification for them is run by the workspace's own user or service
    account.
111. `VerificationRun`, either version: each range, in array order, at `payload.ranges[i].<member>`: a non-null
    `failure`'s `seq` is non-null exactly when its check is reported at an event (§11's per-event
    checks 1 to 6, `anchor_head_mismatch` at the anchored `seq`, `anchor_self_mismatch`,
    `break_glass_cause_mismatch`, `intent_action_mismatch`, `mode_event_mismatch`, `held_mismatch`,
    `connection_lifecycle_mismatch`, `connection_cause_mismatch`, and
    `approval_deferral_mismatch`) and null for `anchor_root_mismatch`, `tsa_token_invalid`,
    `segment_manifest_mismatch`, `segment_gap`, and, at version 2, `segment_rows_mismatch` (`failure.seq`); a non-null `seq` lies from
    `from_seq` to `to_seq` (`failure.seq`); and `to_hash` is non-null when `failure` is null
    (`to_hash`).
112. `VerificationRun` version 1: `result` is `pass` exactly when every range's `failure` is null
    (`payload.result`). Version 2's `result` is rule 133's.
132. `VerificationRun` version 2 ([DEC-788](../project/decisions/DEC-788.md) item 1,
    [DEC-789](../project/decisions/DEC-789.md)), after rule 111: each range, in array order, with
    its clauses in this order, at `payload.ranges[i].<member>`: `start.manifest_hash` is non-null
    exactly when `start.kind` is `manifest` (`start.manifest_hash`); `start.anchor_event_id` is
    non-null exactly when `start.kind` is `anchor` (`start.anchor_event_id`); a `genesis` start
    has `from_seq` 1, so by rule 107 its `prev_hash` is 64 zeros, and an `anchor` start has
    `from_seq` at least 2, since its leaf's `seq` is `from_seq − 1` and at least 1 (rule 113)
    (`start.kind`); `checked` is at most `to_seq − from_seq + 1`, and §4.4 already keeps it from 0
    (`checked`); `checked` is `to_seq − from_seq + 1` when `failure` is null, so a range that
    passed or ended incomplete walked every event of it (`checked`); `incomplete` is null when
    `failure` is non-null, so a range has one outcome (`incomplete`); and, while §11's token check
    cannot answer (DEC-789), a range with an `anchor` start has a non-null `failure` or
    `incomplete`, since its start's token is checked and cannot be proven (`incomplete`). A
    non-null `incomplete` always names its `cause`: the member is not nullable, so `cause` is
    non-null exactly when `incomplete` is, and its absence or `null` is refused `schema` at
    `incomplete.cause`. A rule
    cannot check that `start` names a record that exists or fits: §9.14 and workspace API §4.8.1
    resolve it before the run, and rule 107 already ties `prev_hash` to `from_seq`. On a failed
    range `checked` is only bounded, on purpose: how far a walk got before its first failure
    depends on the check, and the record needs only the bound to be read safely. Nor can a rule see
    an anchor inside a range: the record lists no anchors, so a `pass` recorded for a `genesis` or
    `manifest` range that holds a stamped anchor's leaf, or one with a `null` token, would pass
    `append`. The run's logic must never write it, and its AU-8 test must show it does not
    ([DEC-787](../project/decisions/DEC-787.md) item 8). For the same reason no rule can check that
    a range with both causes recorded `anchor_unstamped`; that test is DEC-787 item 8's too.
133. `VerificationRun` version 2: `result` is `fail` exactly when some range's `failure` is
    non-null, `incomplete` exactly when no range's `failure` is non-null and some range's
    `incomplete` is, and `pass` exactly when every range's `failure` and `incomplete` are null
    (`payload.result`), so a run is never `pass` over a check it could not finish, and never
    `fail` over one alone.

### 9.14 Anchor and segment records ([DEC-783](../project/decisions/DEC-783.md))

The control stream's records of an anchor (§10) and of a closed segment shipped to the cold store
(§6.2), closed at schema version 1 as §9.7's records are: every listed member is present, `null`
only where the type is nullable, and any extra member is refused. §9.1's types apply, with §9.13's
`stream_id` and `digest`. The rules number on from §9.13's. Neither record names a configuration.
Workspace services write both as a `system` actor: the anchoring job and the cold-store exporter.

Both take their shapes from what the code already reads. An anchor's `leaves` and `root` are the
anchor file the verification command reads "as `AnchorComputed` records it"
([DEC-115](../project/04-decision-log.md#decisions) item 6), member for member. A segment's members
are [DEC-263](../project/04-decision-log.md#decisions)'s six manifest fields, with the manifest's
hash beside them. So a verifier takes a trusted start from either record in the journal itself
(§11; [workspace API spec](workspace-api.md) §4.8.1).

**`AnchorComputed`**: one anchor of §10.

| Member | Type | Meaning |
|---|---|---|
| `leaves` | `[{hash: digest, seq: integer, stream_id: stream_id}]` | Every stream's head at the anchor, one per stream, strictly ascending by `stream_id` bytes. The control stream's leaf is its head **before** this event, which §11's `anchor_self_mismatch` checks: rule 113 |
| `root` | `digest` | §10's root over `leaves`: rule 114 |
| `token` | `ref?` | The RFC 3161 timestamp token, stored as an artifact (§10), or `null` while the timestamping authority is unavailable; the outage is retried and journaled as a gap (§10). An anchor whose `token` is `null` is not a trusted start (below) |

**`SegmentExported`**: one segment of §6.2, shipped and locked.

| Member | Type | Meaning |
|---|---|---|
| `stream_id` | `stream_id` | The segment's stream: rule 116 |
| `first_seq`, `last_seq` | `integer` | Its first and last `seq` |
| `first_prev_hash` | `digest` | The first event's `prev_hash`: the trusted start of a range entered at `first_seq` (§11) |
| `last_hash` | `digest` | The last event's `hash`, which the next segment chains from |
| `file_sha256` | `digest` | SHA-256 of the segment file |
| `manifest_hash` | `digest` | The hash of the segment's manifest (DEC-263 item 3): rule 117 |

The manifest and its file are stored in the cold store, not in the journal's artifact store, so
both hashes are `digest`s, not `ref`s, and §11 check 6 does not look for them; the cold per-range
checks do (`segment_manifest_mismatch`, `segment_gap`).

**What a verifier reads.** The trusted start of a range of stream `s` entered at `n` is
`{from_seq: n, prev_hash: first_prev_hash}` of a `SegmentExported` of `s` whose `first_seq` is `n`,
or `{from_seq: n, prev_hash: hash}` of the leaf for `s` in an `AnchorComputed` **that has a
`token`** and whose leaf `seq` is `n − 1`. An anchor whose `token` is `null` is not a trusted start:
without the timestamp nothing outside the journal vouches for it, and a start the journal vouches
for alone is what [DEC-115](../project/04-decision-log.md#decisions) item 5 refused. A range is then
entered from genesis, from a `SegmentExported`, or from a stamped anchor. The run verifies an
anchor start's token by §11's token check, as it does an in-range anchor's
([DEC-896](../project/decisions/DEC-896.md) item 1). A later record that
supplies the missing token (the anchor stamp record, backlog) makes such an anchor a start; this
version has none. A manifest start is one the cold store vouches for: the run reads that segment's manifest
and file from the cold store (§6.2) and checks them; a start that only the hot store's
`SegmentExported` vouches for, because the cold store cannot be read, is not a trusted start, as an
anchor without a `token` is not ([DEC-787](../project/decisions/DEC-787.md) item 5). Every start is looked up among the workspace's own control-stream records only,
so a record that is absent and one of another workspace give the same refusal (workspace API
§4.8.1, DEC-767). The test vectors' `cold_records.trusted_starts.rows` and `row_cases` hold these cases
over stored rows ([DEC-893](../project/decisions/DEC-893.md), [DEC-895](../project/decisions/DEC-895.md)).

**Consistency rules** (reason `schema`; the path is the member named):

113. `AnchorComputed`: `leaves` is non-empty (`payload.leaves`). Each leaf, in array order, with its
    clauses in this order, at `payload.leaves[i].<member>`: its `stream_id`'s `{workspace_id}`
    segment equals the envelope `stream_id`'s (`stream_id`); its `seq` is at least 1 (`seq`); and
    after the first leaf, its `stream_id` sorts after the previous leaf's by bytes (`stream_id`), so
    no stream has two leaves. Then, when there are leaves, one names the envelope's own control
    stream (`payload.leaves`): a control stream always holds its `StreamOpened` before any anchor, so an
    anchor that leaves its own stream out would dodge §11's `anchor_self_mismatch` by omission.
114. `AnchorComputed`: `root` is §10's root over `leaves`, recomputed at append (`payload.root`):
    leaf = SHA-256(0x00 ‖ canonical(leaf)), node = SHA-256(0x01 ‖ left ‖ right), split at the
    largest power of two below the count. An anchor whose leaves do not produce its root proves
    nothing ([DEC-265](../project/04-decision-log.md#decisions) item 2's `MalformedAnchor`).
115. `AnchorComputed`: `actor.kind` is `system` (`actor.kind`).
116. `SegmentExported`: `stream_id`'s `{workspace_id}` segment equals the envelope `stream_id`'s
    (`payload.stream_id`); `first_seq` is at least 1 (`payload.first_seq`); `last_seq` is at
    least `first_seq` (`payload.last_seq`); and `first_prev_hash` is 64 zeros exactly when
    `first_seq` is 1 (`payload.first_prev_hash`).
117. `SegmentExported`: `manifest_hash` is the SHA-256, as 64 lowercase hex, of the canonical JSON
    of DEC-263's six fields built from this record, `{stream, first_seq, last_seq,
    first_prev_hash, last_hash, file_sha256}` with `stream` its `stream_id` and every hash as 64
    lowercase hex (`payload.manifest_hash`), so the record and its manifest cannot disagree.
118. `SegmentExported`: `actor.kind` is `system` (`actor.kind`).

### 9.15 Alert and notice records ([DEC-720](../project/decisions/DEC-720.md))

The notice stream of §2 and v0.12, and the records that feed it ([notifications spec §5.5](notifications.md#55-records)):
the subject stream owner's `OwnerAlertSent`, the dispatcher's `StreamOpened`, `NoticeIssued`,
and `NoticeAttempted` on `ntf:{workspace_id}`, and the control stream's
`NotificationAddressChanged`, which says which web-push addresses a member holds (v0.51,
[DEC-2505](../project/decisions/DEC-2505.md)). Each is closed at schema version 1, and
`NoticeAttempted` also at versions 2 (v0.40, [DEC-706](../project/decisions/DEC-706.md)) and 3
(v0.51, DEC-2505), as §9.6
closes its records: every listed member is present, `null` only where the type is nullable, and any other
member is `schema` at that member. §9.1's types and report order apply. None of the five holds free
text: every string is an identifier, a closed vocabulary, or a provider's opaque token, so nothing
about an order, a position, a mandate, or an address can reach them (`AGENTS.md` rule 6,
notifications spec NT-1 and NT-2). A web-push address is named only by its `address_ref`, an opaque
vault reference listed in the envelope's `pii_refs` (§3).

**Streams.** `OwnerAlertSent` is catalogued on the agent, account, and control streams, written by
that stream's owner in the subject's own batch (notifications spec §3.2, §5.1); on the scheduler and
notice streams it is `wrong_stream` at `event_type`. It names no configuration and is not a risk
input, so it carries no `risk_clock`. The notice stream admits only its `StreamOpened`,
`NoticeIssued`, and `NoticeAttempted`; every other catalogued type is `wrong_stream` there, so no
stream owner copies a fact from it and the dispatcher writes nothing else.
`NotificationAddressChanged` is catalogued on the control stream only, and is `wrong_stream` at
`event_type` on every other: workspace services write it for a member's own request (workspace API
§5.7), and for a deactivation inside the deactivation's own batch (identity spec §5.2 step 3). It
names no configuration and is not a risk input, so it carries no `risk_clock`, and no trading path
reads it (NT-9).

**Types**, beside §9.1's:

| Type | Values | Refused as |
|---|---|---|
| `notice_id` | Exactly 32 lowercase hexadecimal digits: notifications spec §4.2's random notice id. Never an event id, which is a `ulid` and carries its creation time | As `id` |
| `stream_id` | A stream id in §2's grammar, of one of the eight stream types (the meter stream's since v0.46, the deployment meter stream's since v0.53, §9.19; the news stream's since v0.103, §9.37) | As `id` |
| `opaque` | 1 to 256 characters, each U+0021 to U+007E: a provider's message id, which has no space and so cannot carry a sentence | As `id` |
| `kind` | Notifications spec §3.2's closed set of 38 kinds, below | As `id` |

**Kinds and their classes** (notifications spec §3.2, in its order):

| Class | Kinds |
|---|---|
| `action` | `approval_requested`, `approval_reminder` |
| `safety` | `risk_limit`, `kill_switch`, `kill_switch_unapplied`, `agent_held`, `retirement_held`, `account_restriction`, `connection_unavailable`, `protection`, `protection_unattributed`, `attribution_deficit`, `exit_stalled`, `reconciliation`, `external_activity`, `account_state`, `data_feed_down`, `integrity_incident`, `credential_added`, `new_device`, `notification_address_changed`, `recovery_used`, `role_granted`, `member_deactivated`, `deprovisioned`, `break_glass`, `version_risk_increasing`, `delegation_added`, `connection_added`, `went_live`, `client_connected`, `channel_lost` |
| `info` | `daily_brief`, `delegation_ended`, `model_status`, `research_status`, `spend_cap`, `approval_closed` |

**`OwnerAlertSent`** (agent, account, or control stream)

| Member | Type | Meaning |
|---|---|---|
| `subject` | `ulid` | The subject event: rules 119 and 130 |
| `kind` | `kind`, except `approval_requested`, `approval_reminder`, and `channel_lost` | No alert causes those three (notifications spec §3.4); any of them is `non_canonical` at `payload.kind` |
| `owner_command` | `ulid?` | For a user's kill switch, the `OwnerCommandIssued` it carries out, on which the dispatcher de-duplicates (notifications spec §3.4): rule 120 |

**`NotificationAddressChanged`** (control stream; a member set or removed one of their own web-push
addresses, [workspace API spec §4.11, §5.7](workspace-api.md), [DEC-795](../project/decisions/DEC-795.md)
item 5). Its members are exactly workspace API §5.7's and identity spec §12.1's.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | The user principal whose address it is (identity spec §3.1), as §9.12's `member`. On the notice stream that principal's recipient is `p_` and this ULID in lowercase ([DEC-2502](../project/decisions/DEC-2502.md) item 3) |
| `channel` | `web_push` | The one push channel whose addresses a member sets in the workspace (notifications spec §4.6) |
| `action` | `added` \| `removed` | |
| `address_ref` | `ulid` | The address: the `added` event's own `event_id`, naming the vault entry that holds the endpoint, the keys, and the VAPID `key_id` (DEC-2502 item 1, [DEC-2503](../project/decisions/DEC-2503.md) item 5); never derived from the address: rules 210 and 211 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | §9.2's step-up evidence, bound to the request's digest (workspace API §5.7, API-17); `null` only for a `removed` that rides a deactivation: rules 212 to 214 |

**No part of an address.** The endpoint, `p256dh`, `auth`, the push service or its host, and a hash
of any of them are not members, so the closed schema refuses each as `schema` at that member (NT-2,
workspace API API-11), and the step-up digest's inputs are never journaled. Nor is the VAPID
`key_id`: it names the workspace's key, not the member's address, and it is the vault entry's, read
with the entry for each send (DEC-2503 item 5). A planned rotation's "no active address names it"
joins this record's active set to the entries by `address_ref`; it never lists the vault to find
addresses.

**The active set** (workspace API §4.11, notifications spec §4.6, DEC-2502 item 2). Folding the
control stream in `seq` order, an address is active from its `added` until the first later `removed`
that names its `address_ref` with the same `member`. A `removed` that names an `address_ref` that is
not active, or that another member added, changes nothing: workspace services never commit one
(workspace API §5.7, removing step 2), and reading it as a removal would let one record silence
another member's address. The set is never read from the vault, and a vault entry with no committed
`added` is never active. The limit of 10 active addresses, an endpoint already held, and a
deactivation's removal of every active address of the member are workspace services' checks across
records (workspace API §5.7, identity spec §5.2 step 3), not `append`'s; a fold that meets more than
10 active addresses sends to each, since sending to more addresses drops no notice. Whether an
address is `unreachable` is the notice stream's (below), never this record's.

**`StreamOpened`** on the notice stream. As on the agent stream (§9.1), the schema is chosen by the
stream type of `stream_id`.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `notice` | |
| `workspace_id` | `id` | The subject: rule 121 |

**`NoticeIssued`** (notice stream, before the first send)

| Member | Type | Meaning |
|---|---|---|
| `notice` | `notice_id` | The id the payload and the link carry |
| `kind` | `kind` | |
| `class` | `action` \| `safety` \| `info` | Rule 122 |
| `cause` | `ulid` | The event the notice answers: an `OwnerAlertSent`, an `ApprovalRequested`, or for `channel_lost` this stream's own `NoticeAttempted` (notifications spec §3.4). A kill switch's owner command is reached through its alert's `owner_command`, never named as a cause. Rule 124 |
| `cause_stream` | `stream_id` | The stream `cause` is on: rule 125 |
| `recipients` | `[id]` | Opaque user ids (NT-2): rule 123 |

**`NoticeAttempted`** (notice stream, one attempt's outcome on one push channel, and on web push at
one address)

| Member | Type | Meaning |
|---|---|---|
| `notice` | `notice_id` | |
| `recipient` | `id` | The opaque user id; never an address (NT-2) |
| `channel` | `email` \| `phone` \| `slack` \| `sms` \| `telegram` \| `web_push` | The mandate schema's push channels (notifications spec §4.1). The pull channels are never attempted: `ApprovalDelivered` and the journal itself are their record |
| `attempt` | `integer` | 1 for the first attempt on the channel, and at version 3 on web push for the first at the address: rule 126 |
| `status` | `delivered` \| `failed` \| `suppressed_quiet_hours` \| `deferred_quiet_hours` \| `abandoned` | `delivered` means the provider accepted the message, not that anyone read it |
| `reason` | `timeout` \| `rate_limited` \| `provider_error` \| `address_rejected` \| `auth_failed` \| `too_large` \| `recipient_not_permitted` \| `address_missing` \| `bounced` \| `complained` \| `unsubscribed` \| `not_pending` \| `retry_window_ended` \| `address_retired` (nullable; `address_retired` from version 3) | Notifications spec §5.2's closed enum, with the receipts `bounced`, `complained`, and `unsubscribed` (§5.6) and the dispatcher's own `address_missing` (§5.1, [DEC-795](../project/decisions/DEC-795.md) item 7), then §5.3's stops, `address_retired` from version 3 only (at versions 1 and 2 it is `non_canonical` at `payload.reason`): rules 127 and 216 |
| `provider_message_id` | `opaque?` | The provider's id for the accepted message, which a later receipt names: rule 128 |
| `coalesced_into` | `opaque?` | For a notice combined into one message (notifications spec §5.4), that message's provider id: rule 129 |
| `verdict` | `retryable` \| `permanent` (nullable) | Version 2. For a `failed` attempt, the provider's verdict of notifications spec §5.2, which decides whether that channel is tried again for the notice ([DEC-725](../project/decisions/DEC-725.md) item 6.3); `null` for every other status: rule 134 |
| `address_ref` | `ulid?` | Version 3. On web push, the address the attempt went to: the `address_ref` of a `NotificationAddressChanged` (DEC-2502 items 4 and 5); `null` on every other channel, where a recipient has one address per channel: rule 215 |

**Versions.** Version 2 (v0.40, [DEC-706](../project/decisions/DEC-706.md)) is version 1's
members in their order, and `verdict` last, so a reader of the notice stream alone, the dispatcher
after a restart (notifications spec §5.1 step 4) and NT-8's journal-only oracle among them, can tell
a `failed` attempt to retry from one that ended its channel. `provider_error` and `too_large` are
each `retryable` under one accepted decision and `permanent` under another (DEC-725 item 6.3,
[DEC-728](../project/decisions/DEC-728.md) item 1, [DEC-729](../project/decisions/DEC-729.md)
items 4, 5, 7, and 8), so `reason` cannot say which. The verdict is the outcome §5.2's `send`
returned: `permanent { reason }` is `permanent`; `retryable { reason }`, and a timeout with no
answer (the caller's `retryable { timeout }`), are `retryable`. A receipt's `failed` record is
`permanent`. `delivered`, `suppressed_quiet_hours`, `deferred_quiet_hours`, and `abandoned` carry
none. From v0.40 to v0.51 every writer appended version 2, and `append` refused `NoticeAttempted`
at version 1 as `unknown_schema` at `payload`, so no writer could leave the verdict out by writing
the older shape. Version 1 is not edited (§8) and still replays: a stored version-1 `failed` attempt
reads as `permanent`, so that channel is finished for that notice and nothing is sent again on a
guess. No deployment holds one: before v0.40 only the dispatcher's own step tests appended
`NoticeAttempted`, and no dispatcher process runs yet (E8-10).

Version 3 (v0.51, [DEC-2505](../project/decisions/DEC-2505.md)) is version 2's members in their
order, and `address_ref` last. On web push one handle fans out to each of the recipient's active
addresses, each with its own send, idempotency key, and retry schedule (notifications spec §4.6,
§5.1, §5.3), so each outcome must name its address, or a verdict on one browser would mark them all
or be lost (DEC-2502 item 5). Version 3 also admits `address_retired`, the dispatcher's stop for a
web-push send whose address was removed, or marked `unreachable` by another attempt, before its
next attempt (notifications spec §5.3); DEC-795's one last send to a removed address is never
retired, and keeps `retry_window_ended` when its entry is already gone (§5.1). Every writer appends
version 3, and from v0.51 `append` refuses `NoticeAttempted` at versions 1 and 2 as
`unknown_schema` at `payload`. A version-2 record on a channel with no address says what version 3
says with a `null` `address_ref`, but admitting version 2 would also admit one on web push that
names no address, which nothing could then attribute to an address; one shape per fact keeps a
writer from choosing the shape that says less, as v0.40 did for the verdict. Version 2 is not edited
(§8) and still replays, as version 1 does, with `address_ref` read as `null`. No deployment holds a
web-push `NoticeAttempted` at any version: the dispatcher makes no web-push attempt until version 3
exists and web push is turned on (notifications spec §4.6, §5.9,
[DEC-2504](../project/decisions/DEC-2504.md)), and no dispatcher process runs yet. A stored
version-1 or version-2 web-push attempt, were one read, is the attempt of no address: it ends no
address's sends and marks none, so nothing is marked on a guess and no address the fold makes active
loses a send.

A `NoticeAttempted` whose `reason` is `address_rejected`, `auth_failed`, `bounced`, `complained`, or
`unsubscribed` is the record that marks the address on that channel `unreachable` (notifications
spec §5.6), on web push only the one its `address_ref` names and never the member's other
addresses (DEC-2502 item 6), and causes its `channel_lost` notice; one whose `reason` is
`address_missing` causes a `channel_lost` notice once per address, on web push once per
`address_ref`, and marks nothing. Neither needs a member of its own, since a separate flag could only
disagree with the reason. `recipient_not_permitted` marks nothing and causes nothing (§5.2), and nor
does `address_retired`, which records only that the address left the set.

**Consistency rules** (reason `schema` unless stated; the path is the member named):

119. `OwnerAlertSent`: `causation_id` equals `subject` (`causation_id`).
120. `OwnerAlertSent`: `owner_command` is `null` unless `kind` is `kill_switch`
     (`payload.owner_command`). It is allowed on every alert stream: the v0.12 row lets a stream
     owner's alert name the command ("if any"), and refusing it would refuse the batch that holds
     the `KillSwitchActivated` beside it (`AGENTS.md` rule 13).
121. Notice-stream `StreamOpened`: `stream_id` equals `ntf:{workspace_id}` (`stream_mismatch` at
     `stream_id`).
122. `NoticeIssued`: `class` is the class of `kind` in the table above (`payload.class`).
123. `NoticeIssued`: `recipients` is strictly ascending by bytes, so none is listed twice
     (`payload.recipients`). It may be empty: a recipient with no push address keeps the pull
     channels.
124. `NoticeIssued`: `causation_id` equals `cause` (`causation_id`).
125. `NoticeIssued`: `cause_stream` is a stream of this stream's workspace: an agent stream for
     `approval_requested` and `approval_reminder`, this notice stream for `channel_lost`, and an
     agent, account, or control stream for every other kind (`stream_mismatch` at
     `payload.cause_stream`), so a notice never answers another workspace's event (NT-10).
126. `NoticeAttempted`: `attempt` is at least 1 (`payload.attempt`).
127. `NoticeAttempted`: `reason` is non-null exactly when `status` is `failed` or `abandoned`. An
     `abandoned` attempt's reason is a stop: `not_pending` (an `action` notice whose approval
     stopped being pending) or `retry_window_ended` (a `safety` notice's 24 hours or an `info`
     notice's 6 hours ran out, or the one last send to a removed address whose vault entry is
     gone; notifications spec §5.1, §5.3), and at version 3 also `address_retired` (rule 216); a
     `failed` one's is any other (`payload.reason`).
128. `NoticeAttempted`: `provider_message_id` is non-null exactly when `status` is `delivered` or
     `reason` is a receipt, `bounced`, `complained`, or `unsubscribed`
     (`payload.provider_message_id`).
129. `NoticeAttempted`: `coalesced_into` is `null` when `status` is `suppressed_quiet_hours` or
     `deferred_quiet_hours`, since nothing was sent (`payload.coalesced_into`). A combined message
     that failed or was abandoned keeps the link, so NT-6's oracle attributes every joined notice's
     outcome to its cause, recipient, and channel.
130. **Batch rule.** An `OwnerAlertSent`'s `subject` is the `event_id` of an earlier draft in the
     same `append` batch that is not itself an `OwnerAlertSent`, and no other `OwnerAlertSent` in
     the batch names the same subject (`payload.subject`, at the first alert that breaks it):
     notifications spec §3.2's "in the subject's own batch" and §3.4's "one subject, one kind",
     checked at append.
264. **Stream rule** (control stream, [DEC-2351](../project/decisions/DEC-2351.md), v0.78; numbered
     on after #2278's rules 261 to 263, renumbering none). An `OwnerAlertSent` whose
     `kind` is `kill_switch_unapplied` names as its `subject` an **unapplied crash loop**
     (`payload.subject`): a `ProcessCrashLooped` of an executor whose run is **held**, with no
     `AgentDeployed` on a connection of its account (by its `ConnectionEstablished`'s
     `account_ref`) lacking its `AgentStopped` at the loop. A run is held from a
     `ProcessStartIssued` with cause `kill_switch` journaled while every such deployment already
     had its `AgentStopped`, until a later start of the executor with cause `deployed`,
     `requested`, or `exchanged`, or a `ProcessStopIssued` of it: the manager journals a stop only
     for a generation it does not hold. A connect's `ConnectionRequested` neither holds nor frees a
     run, and a deployment on a connection revoked since still counts until its `AgentStopped`.
     So a deployment and retirement while the run goes on leave
     it held and unstopped (DEC-2344 item 3); a loop while an agent wants the executor is an
     ordinary one, `crash_loop` alone; and a later covering switch's start after an unapplied loop
     begins a new run, whose own loop is unapplied too ([DEC-2344](../project/decisions/DEC-2344.md)
     item 1). Once an executor has run, the only end of a hold the journal shows is a
     `ProcessStopIssued`: rule 167 holds `deployed`, `requested`, and `exchanged` to generation 1.
     The manager also ends a hold at the run's own `Succeeded` end, which is not journaled, so the
     rule admits an alert the manager may no longer write: rule 264 admits the alert, and the
     manager writes at most one per loop, in its batch, and only while it holds the run, which
     E8-17's tests pin. The
     `kill_switch_unapplied` vectors' streams are judged by the fold and by an independent oracle
     that reads each alert's subject backwards, and each seeded bug is shown caught.
134. `NoticeAttempted` version 2 ([DEC-706](../project/decisions/DEC-706.md)), numbered on after
     rule 133 and renumbering none: `verdict` is non-null exactly when `status` is `failed`, and
     where notifications spec §5.2 to §5.4 fix a reason's verdict, it is that one
     (`payload.verdict`, both clauses): `permanent` for `address_rejected`, `auth_failed`,
     `recipient_not_permitted`, `address_missing`, and the receipts `bounced`, `complained`, and
     `unsubscribed`, and `retryable` for `timeout` and `rate_limited`. `provider_error` and
     `too_large` take either, as the adapter answered. So a record never marks an address
     `unreachable` while its channel is still retried, and never stops a channel on a timeout or a
     rate limit. It holds at version 3 unchanged.

Rules 210 to 216 ([DEC-2505](../project/decisions/DEC-2505.md), v0.51) number on after rule 191,
which open changes hold, and renumber none:

210. `NotificationAddressChanged`: `address_ref` equals the envelope's `event_id` when `action` is
     `added`, and differs from it when `action` is `removed` (`payload.address_ref`), so an address
     is named by the record that added it and a removal names an earlier one.
211. `NotificationAddressChanged`: `pii_refs` is exactly `[address_ref]` (`pii_refs`), so redaction
     and erasure reach the vault entry (workspace API §5.7) and the record lists no other personal
     data.
212. `NotificationAddressChanged`: `step_up` is non-null when `action` is `added`
     (`payload.step_up`). Non-null evidence is valid at the envelope's `event_time` as rule 102
     reads it: its method is `passkey` in a `live` envelope and `passkey` or `cli_confirm`
     otherwise (`payload.step_up.method`), and 0 ≤ `event_time` − `authenticated_at` ≤ 300 seconds
     (`payload.step_up.authenticated_at`). That the assertion was never used before is workspace
     services' check across records (API-17); a replacement's `removed` repeats its `added`'s
     evidence, one request's evidence written twice and not a second use (rule 214).
213. `NotificationAddressChanged`: the actor. With a non-null `step_up` it is a `user`
     (`actor.kind`) whose `id` equals `member` (`payload.member`): a member sets and removes only
     their own addresses (workspace API §4.11, API-6). With a `null` `step_up`, a `removed` riding a
     deactivation, it is a `user` or `system` (`actor.kind`), as rule 98 has the deactivation's. A
     `client` actor is refused before the payload is read (rule 83).
214. **Batch rule** (workspace API §4.11, §5.7; identity spec §5.2 step 3). In one `append` batch:
     (a) the `NotificationAddressChanged` records with a non-null `step_up` are one request's: at
     most one `added` and at most one `removed`, with one `member` and equal `step_up`; one of each
     is a replacement (of an `unreachable` address, or of one subscribed under a retired key,
     workspace API §5.7, DEC-2503 item 7); (b) each `added`, and a `removed` with a non-null
     `step_up` in a batch with no `added`, is the `subject` of an `OwnerAlertSent` whose `kind` is
     `notification_address_changed`, and a replacement's `removed` is the subject of none, since the
     member is told once, by the `added`'s alert; (c) a `removed` with a `null` `step_up` rides a
     `MemberDeactivated` or `MemberRemoved` in the batch whose `member` equals its `member` and whose
     `actor` equals its `actor`, is the subject of no `OwnerAlertSent`, and shares its batch with no
     `NotificationAddressChanged` that has a non-null `step_up`; (d) no two `removed` records name
     one `address_ref`, and no `removed` names the `address_ref` of an `added` in the batch; and
     (e) an `OwnerAlertSent` whose `kind` is `notification_address_changed` names as its `subject` a
     `NotificationAddressChanged` that (b) gives an alert. A break of (a) to (d) is reported at the
     first `NotificationAddressChanged` in the batch that breaks one (`payload.address_ref`), and of
     (e) at the first such alert (`payload.subject`).
215. `NoticeAttempted` version 3: `address_ref` is non-null exactly when `channel` is `web_push`
     (`payload.address_ref`), and `pii_refs` is `[address_ref]` when it is non-null and empty when
     it is `null` (`pii_refs`).
216. `NoticeAttempted` version 3: `reason` `address_retired` is on an `abandoned` attempt on
     `web_push` only (`payload.reason`). At version 3 rule 127's stops are `not_pending`,
     `retry_window_ended`, and `address_retired`, so rule 127 already refuses `address_retired` on a
     `failed` attempt; this rule refuses it on any other channel, which has no address set to leave.

The first violation is reported in §9.1's order: unlisted members, the listed members in the order
given, then rules 119, 120, 122 to 124, 126 to 129, at versions 2 and 3 rule 134, at version 3
rules 215 and 216, and rules 210 to 213 (only on a well-typed payload), then `artifact_refs` and
`pii_refs`, then the subject rules 121 and 125. Rule 130 spans a batch, so it is
checked once every draft in the batch passes these, as rule 10's second clause is, and rule 214
after it.

**Lifecycle.** A cause is issued once: a cause that already has a `NoticeIssued` is skipped, and on
restart the dispatcher issues every cause without one and attempts again every notice that has no
terminal attempt (notifications spec §5.1). Attempts are judged per notice, recipient, channel, and
`address_ref` (`null` off web push), each address numbered from attempt 1. At each of them, an
attempt is terminal when it
is `delivered`, `suppressed_quiet_hours`, or `abandoned`, or `failed` with the verdict `permanent`
(a version-1 `failed` reads as `permanent`); a `deferred_quiet_hours` attempt, or a `failed` one
with the verdict `retryable`, leaves that address due (notifications spec §7's diagram,
[DEC-706](../project/decisions/DEC-706.md), DEC-2502 item 5). On web push the addresses due for a
notice are those active and not `unreachable` at the step that makes the channel's first attempt for
it, and the address a `notification_address_changed` notice's own `added` names (notifications spec
§4.6); NT-8's oracle reads the same set from the journal alone.

An address's life, from these records: an `added` makes it active, and its alert's notice reaches it
with the member's other addresses; a `removed` ends it, a send still due to it is `abandoned`
`address_retired` at its next step, and only the removal's own notice is sent to it once more
(DEC-795); a re-add of the same endpoint is a new `address_ref`, whose marks and `address_missing`
report start afresh; a replacement removes and adds in one batch, and sends nothing more to the old
address; a deactivation removes every active address in its own batch, with no alert and no last
send, and a send still due to any of them is `address_retired`; a `failed` `address_rejected`, or
any of the five marking reasons, marks that address `unreachable`, so a send still due to it is
`address_retired` and later notices skip it, while it stays active, listed, and counted until the
member removes or replaces it (workspace API §4.11); and
a retired VAPID key changes no record here: a send to an address whose key the dispatcher no longer
holds is `failed` `provider_error` with the verdict `permanent`, which marks nothing (DEC-2503 items
5 and 7).

A later receipt for a delivered message is a new `NoticeAttempted` naming the same
`provider_message_id`; it never retracts the earlier `delivered` (notifications spec §5.5). Nothing on the notice stream is a risk input, and no trading path waits on it (NT-9).

### 9.16 The broker's order id ([DEC-869](../project/decisions/DEC-869.md))

**`OrderStateChanged`** on the account stream at `schema_version` 2: version 1's members, in
their version-1 order, and `broker_order_id` last, nothing else moved. Version 1 is never
edited (§8), stays registered, and keeps replaying as it always has. §9.2's conventions apply
unchanged: every member is present on every record (§4.2), `null` only where the type is
nullable, any other member refused, and opaque text non-empty.

| Member | Type | Meaning |
|---|---|---|
| version 1's members | as §9's row | Unchanged ([DEC-459](../project/decisions/DEC-459.md)) |
| `broker_order_id` | `text?` | The broker's own id for the order named by `client_order_id`, as the broker record gave it |

**Who writes it.** The executor writes `broker_order_id` non-null exactly on a record it writes
from a broker order record that carries the order's broker id: a place answer, an order query's
answer, a cancel answer that is the order, and an adoption by list-and-match or reconciliation.
It is `null` on every other record: a fill, a timeout, a cancel request, a lookup's `absent`, and
any record written with no broker order record in hand. A replacement's new id stays in
`replaced_by_broker_order_id`, beside `replaced_by`, as at version 1, on the old order's record.
The replacement successor's own record (its `client_order_id` the successor's key, `replaces` the
old one) carries that same `replaced_by_broker_order_id` as its `broker_order_id`, or `null` when
the broker record gives none ([DEC-870](../project/decisions/DEC-870.md) item 1). The id is the one
the broker gave for that order in the same record, so nothing is inferred; a `null` leaves the
successor with no recoverable id, so its cancel is `NotSent` and escalates (below), and nothing
reads the member for risk.

**Who reads it.** A connector that cancels by the broker's id (connections spec §6.2) rebuilds
its `client_order_id` → order id map at start from the account stream: each `client_order_id`
maps to the `broker_order_id` of its version-2 records that carry one. The map is never held only
in memory ([DEC-860](../project/decisions/DEC-860.md) item 7). An order with no such record — one
on a stream written before v0.34, or one whose place answer was in doubt and is not yet adopted —
has no recoverable id, and so does an order whose records carry two different ids: its cancel is
refused before any call, `NotSent` (`no_order_id`), the shell stops and alerts the owner (DEC-85),
the order stays live in the executor's view, never assumed cancelled, and nothing is cancelled or
adopted by guess (CN-7).

**What the executor's fold reads from it** (v0.80, [DEC-2353](../project/decisions/DEC-2353.md)).
One thing only: the order a broker fill belongs to when the fill names no `client_order_id`, as
Alpaca's `FILL` activity names only the broker's order id. The fold keeps, for each order, the
`broker_order_id` its version-2 records carry. A fill with no `client_order_id` and a
`broker_order_id` is the order's whose records carry that id, and it is then judged as a fill that
named that order (instrument, side, and what the order has left). An order whose records carry two
different ids, or none, takes no such fill, and an id two orders' records carry names neither: the
fill is applied unattributed, as external activity, as before. No risk state, limit, or order state
is read from the member otherwise, and replay of every earlier record is unchanged: a fill already
journaled keeps the attribution its `FillApplied` recorded.

**Content.** The member is the broker's opaque order identifier and nothing else: never a
credential, token, account number, or vault path. It is not a notification payload (`AGENTS.md`
rule 6); it is in the journal, as `ExternalActivityIngested`'s `broker_order_id` already is.

**No mapping to `JournaledFact`**, and no vectors: `reference/journal` does not model
`OrderStateChanged` at either version, and the test vectors stay version 3.

### 9.17 Control-plane records ([DEC-981](../project/decisions/DEC-981.md))

The workspace deployment's records of what passed over its link to the global control plane
([control-plane design §3.8](../design/control-plane.md#38-the-closed-message-set)), closed at
schema version 1 as §9.7's records are: every listed member is present, `null` only where the type
is nullable, and any extra member is refused. §9.1's types apply, with §9.13's `digest`. The rules
number on from §9.16's (rule 136 is §9.6's). No record names a configuration.

**Who writes them.** Workspace control services, the control stream's single writer (§2, under the
fencing of §5.1), as a `system` actor: rule 137. The control-plane agent (`cp-agent`) verifies each
inbound instruction and hands it over; it never appends, holds no writer epoch, and no record here
is written in its name (design §3, CP-5). An instruction is journaled before it takes effect, and
one that is refused is journaled as `ControlPlaneMessageRefused` instead.

**Where they go.** Every record is on the workspace control stream, `ctl:{workspace_id}`. An
instruction that covers a deployment (an enrollment, a license, a release, a catalog entry, a data
bundle) is journaled on the control stream of each workspace it covers, before it takes effect in
that workspace, so each workspace's stream holds every control-plane fact that bounds it and no
workspace reads another's (§7). A workspace opened later on the same deployment has the
deployment's enrollment and the instruction of each kind in effect journaled on its own stream
before its first `AgentDeployed` (DEC-981 item 2). The site's own acts follow the same rule:
`ReleaseInstalled`, `ReleaseWithdrawnNoticed`, and `ControlPlaneMessageRefused` go on the control
stream of every workspace the deployment hosts, and `UsageReportSealed`, one report per deployment
and organization, on that of every workspace of its `org_id` on the deployment, so no workspace
holds another organization's counts. A `LicenseApplied` names every deployment of its organization
the license covers; those IDs are the organization's own metadata, visible only to its workspaces.

**No content.** No member is `text`. Every member is an `id`, a closed enumeration, an `integer`,
a `timestamp`, or a `digest`, so nothing here can hold an instrument, a quantity, a price, a
mandate field, or personal data (design CP-1, `AGENTS.md` rule 6). The enrollment token, the
private key, and the signed documents' bytes are never journaled; a document is named by the
SHA-256 of its bytes as received, a `digest`, not a `ref`, so it is not listed in `artifact_refs`
and §11 check 6 does not look for it.

**Sequence and anti-rollback (CP-9).** `LicenseApplied`, `ReleaseOffered`, `CatalogEntryRegistered`
(per `kind`), and `DataBundleImported` (per `dataset`) carry the signed document's `sequence`.
Workspace services judge an instruction against the highest `sequence` of its kind journaled on the
stream: a lower one, or an equal one with another digest, is refused and journaled
`ControlPlaneMessageRefused` with reason `replayed`; one whose digest equals the one in effect is
already applied and is not journaled again. A `ReleaseWithdrawnNoticed` carries no sequence and
is never refused as `replayed`: a repeated withdrawal only restricts again. A message whose digest
was already refused is not journaled again, so a flood of one bad message cannot fill the stream.
`append` and §11 do not compare sequences across records at this version (DEC-981 item 4).

**Ending a lapse.** A new valid license ends `lapsed` (design §3.3): its `LicenseApplied` is
journaled first, then the `LicenseStateChanged` out of `lapsed` that names it.

**No effect on an agent here.** A `LicenseStateChanged` into `lapsed` restricts no agent until
mandate spec §5.9 lists `license_lapsed` (E20-4; design §3.3, DEC-440 item 13 Proposed). None of
these records is read by `ValidationContext`, the gate, or any fold, and none maps to a
`JournaledFact`.

**`ControlPlaneEnrolled`**: the deployment enrolled (design §3.1).

| Member | Type | Meaning |
|---|---|---|
| `deployment_id`, `org_id` | `id` | The deployment the directory bound the certificate to, and its organization |
| `mode` | `managed_cell` \| `hybrid` | The deployment's mode (design §3.2) |
| `certificate_fingerprint` | `digest` | SHA-256 of the client certificate's DER bytes |

**`LicenseApplied`**: a verified license took effect (design §3.3).

| Member | Type | Meaning |
|---|---|---|
| `license_id` | `id` | |
| `sequence` | `integer` | Increases per organization: rule 139 |
| `org_id` | `id` | |
| `deployment_ids` | `[id]` | The deployments it covers, strictly ascending by bytes: rule 138 |
| `max_workspaces`, `max_agents_deployed` | `integer` | Entitlements |
| `features` | `[hybrid \| connected_clients]` | Feature flags, strictly ascending by bytes: rule 138 |
| `not_before`, `not_after` | `timestamp` | Validity: rule 138 |
| `grace_days` | `integer` | |
| `key_id` | `id` | The pinned license key that verified it |
| `document_digest` | `digest` | SHA-256 of the signed license as received |

**`LicenseStateChanged`**: the license's state moved (design §3.3).

| Member | Type | Meaning |
|---|---|---|
| `license_id` | `id` | |
| `from` | `valid` \| `renewal_due` \| `grace` \| `lapsed`, or `null` | `null` for the first state of a license: rule 140 |
| `to` | `valid` \| `renewal_due` \| `grace` \| `lapsed` | |
| `judged_at` | `timestamp` | The highest UTC time the stream had journaled, against which validity was judged (design §6) |

**`ReleaseOffered`**: a verified release manifest offered to the operator or the update window
(design §3.4).

| Member | Type | Meaning |
|---|---|---|
| `release_id` | `id` | |
| `sequence` | `integer` | Rule 139 |
| `manifest_digest` | `digest` | SHA-256 of the signed manifest as received |
| `key_id` | `id` | The pinned release key that verified it |

**`ReleaseInstalled`**: the site installed a release.

| Member | Type | Meaning |
|---|---|---|
| `release_id` | `id` | |
| `sequence` | `integer` | The manifest's: rule 139 |
| `manifest_digest` | `digest` | |
| `trigger` | `promotion` \| `operator` \| `update_window` \| `rollback` | Our managed promotion, the site operator's action, the site's update window, or an operator rollback under infrastructure §7.4, the one install CP-9 allows below the highest offered |

**`ReleaseWithdrawnNoticed`**: a verified withdrawal; new installs of the release are refused.

| Member | Type | Meaning |
|---|---|---|
| `release_id` | `id` | |
| `reason` | `defect` \| `security` \| `superseded` | |
| `key_id` | `id` | |
| `document_digest` | `digest` | |

**`CatalogEntryRegistered`**: a verified catalog entry, selectable after the local evaluation gate
and the owner's pin (design §3.8). A registry entry a mandate pins is still registered as
`ConfigSnapshotRegistered`.

| Member | Type | Meaning |
|---|---|---|
| `kind` | `connector_package` \| `model_registry_entry` | |
| `entry_id` | `id` | |
| `content_hash` | `digest` | SHA-256 of the entry's content |
| `sequence` | `integer` | Per `kind`: rule 139 |
| `key_id` | `id` | |

**`DataBundleImported`**: a verified shared-data bundle, journaled before any byte of it is used
([data plane spec §6.1](data-plane.md#61-managed)).

| Member | Type | Meaning |
|---|---|---|
| `dataset` | `id` | The dataset the bundle updates |
| `bundle_digest` | `digest` | The bundle's digest, which an observation names |
| `sequence` | `integer` | Per `dataset`: rule 139 |
| `key_id` | `id` | |

**`UsageReportSealed`**: one usage period's report, signed and queued (design §3.5).

| Member | Type | Meaning |
|---|---|---|
| `report_id`, `deployment_id`, `org_id` | `id` | One report per deployment, organization, and period (design §3.5) |
| `period_start`, `period_end` | `timestamp` | Rule 141 |
| `prev_report_hash` | `digest` | The previous report's digest, 64 zeros for the deployment's first |
| `report_digest` | `digest` | SHA-256 of the signed report |

**`ControlPlaneMessageRefused`**: an inbound message refused (design CP-5).

| Member | Type | Meaning |
|---|---|---|
| `message` | `license` \| `release_manifest` \| `release_withdrawn` \| `catalog_entry` \| `data_bundle` \| `unknown` | The kind it claimed; `unknown` for a type outside §3.8's list |
| `reason` | `unsigned` \| `bad_signature` \| `unknown_key` \| `replayed` \| `out_of_list` \| `malformed` \| `not_addressed` | No signature; a signature that does not verify; a key the installed release does not pin; a sequence not above the highest journaled (above); a type or effect outside §3.8's list; a member outside its schema; a document naming another organization or deployment |
| `sequence` | `integer?` | The sequence it claimed, `null` when none could be read: rule 142 |
| `document_digest` | `digest` | SHA-256 of the bytes received |

**Consistency rules** (reason `schema` unless named; the path is the member named), checked in
number order after the members' types:

137. Every §9.17 record: `actor.kind` is `system` (`actor.kind`).
138. `LicenseApplied`: `deployment_ids` is non-empty (`payload.deployment_ids`) and strictly
     ascending by bytes (`non_canonical` at `payload.deployment_ids`); `features` is strictly
     ascending by bytes (`non_canonical` at `payload.features`); and `not_before` is earlier than
     `not_after` (`payload.not_after`).
139. `LicenseApplied`, `ReleaseOffered`, `ReleaseInstalled`, `CatalogEntryRegistered`, and
     `DataBundleImported`: `sequence` is at least 1 (`payload.sequence`); so is a non-null
     `sequence` on `ControlPlaneMessageRefused`.
140. `LicenseStateChanged`: `from` differs from `to` (`payload.to`).
141. `UsageReportSealed`: `period_start` is earlier than `period_end` (`payload.period_end`).
142. `ControlPlaneMessageRefused`: when `message` is `unknown`, `reason` is `out_of_list` or
     `malformed` (`payload.reason`), since nothing else can be judged of a message outside the list,
     and `sequence` is `null` (`payload.sequence`), since it has no schema to read one from; and
     when `reason` is `replayed`, `sequence` is non-null (`payload.sequence`), since a replay is
     judged by its sequence.

The test vectors' `control_plane` section holds a base draft of each record, an invalid draft for
every member type and rule, and valid drafts for the cases a rule might be misread to refuse.

### 9.18 Backtest and evaluation records ([DEC-1001](../project/decisions/DEC-1001.md), [DEC-1002](../project/decisions/DEC-1002.md))

Both records are on the workspace control stream, at `schema_version` 1, and both name a report
by artifact (§6.3): the canonical JSON object of the report, stored under `sha256:` of its bytes,
before the record is appended (§5.2). The figures live in the report; the record holds what
reproduces it. §9.2's conventions and types apply: every member present on every record (§4.2),
`null` only where the type is nullable, any other member refused. Workspace services write
both; the envelope's `actor` is the `user` who asked for the run or the `system` runner or evaluator
that ran it, and any other kind is refused `schema` at `actor.kind`
([DEC-1141](../project/decisions/DEC-1141.md)).

**`BacktestRunRecorded`**: one backtest of a mandate version through its own decision path
([DEC-1000](../project/decisions/DEC-1000.md)).

| Member | Type | Meaning |
|---|---|---|
| `backtest_id` | `id` | The backtest, as `POST /backtests` named it (workspace API §4.2) |
| `mandate_version` | `ref` | The version backtested; equals `config_refs.mandate_version` |
| `data_snapshot` | `ref` | The digest of the bars the run read, with its coverage start and first-bar medians; the data plane's manifest digest once DP-14 lands |
| `model_outputs`, `theses` | `ref` | The digests of the recorded model outputs and thesis records the run replayed |
| `configuration` | `ref` | The digest of the run's configuration: the fill model, fees, the gate's settings, account type, starting cash, and metric settings |
| `code_build` | `ref` | The runner's build digest |
| `report` | `ref?` | The report artifact (`report_version` 2), `null` exactly when `result` is `refused` |
| `report_version` | `integer` | The report's version: 2 |
| `marker` | `backtest` | Always `backtest`: never a paper or live result |
| `result` | `completed` \| `refused` | Whether the run produced a report |
| `refusal` | `text?` | The run's stable reason code (ADR-0001 ES-09), `null` exactly when `result` is `completed` |

**`EvaluationRunRecorded`**: one forward-paper evaluation (E17-8), written only on the control
stream of one of the team's internal paper workspaces (DEC-103).

| Member | Type | Meaning |
|---|---|---|
| `evaluation_id` | `id` | The evaluation |
| `decision` | `text` | The recorded decision that registered the window, metric, and threshold (DEC-122) |
| `cut_off` | `timestamp` | The instant the evaluation reads up to: no thesis whose horizon ends after it is scored |
| `theses` | `ref` | The digest of the `ThesisProposed` and `ThesisRevised` records read |
| `theses_from`, `theses_to` | `[text]` | Per agent stream read, in stream order, `stream_id:seq` of the first and of the last record read; equal lengths |
| `theses_head` | `[digest]` | Per agent stream read, the `hash` (§3) of the last record read, in the same order |
| `closes`, `cost_model` | `ref` | The digests of the close series and the round-trip cost model |
| `code_build` | `ref` | The evaluator's build digest |
| `report` | `ref` | The evaluation report artifact (`report_version` 1) |
| `report_version` | `integer` | 1 |
| `passed` | `boolean` | True only when at least one signal model's group was scored and every scored group passed |

**Consistency rules** (reason `schema` unless stated):

143. `BacktestRunRecorded`: `report` is `null` and `refusal` non-null exactly when `result` is
     `refused` (`payload.report`, then `payload.refusal`).
144. `BacktestRunRecorded`: `mandate_version` equals `config_refs.mandate_version`
     (`payload.mandate_version`).
145. `EvaluationRunRecorded`: `theses_from`, `theses_to`, and `theses_head` have one entry per
     agent stream read, the same length, and are not empty (`payload.theses_from`).

`theses_head` is a `digest` (§9.13): a journal record's hash names no stored artifact, so it is
not listed in `artifact_refs` and §11 check 6 does not read it; every other digest member here is
a `ref` to a stored object ([DEC-1142](../project/decisions/DEC-1142.md)). The test vectors'
`backtest_records` section holds a base draft of each record, an invalid draft for every member
type and rule, and valid drafts for the cases a rule might be misread to refuse.

**No mapping to `JournaledFact`.** No fold reads either record: they are what the backtest and
evaluation produced, and the go-live record names a backtest by its id (`AgentDeployed`). The rule
numbers follow §9.17's rule 142.

### 9.19 Model gateway records ([DEC-998](../project/decisions/DEC-998.md))

The model gateway ([inference spec](inference.md)) is the one component that calls a model. It
writes none of the existing streams: the caller appends each call's `ModelInvocationRecorded` to its
own stream (inference spec §3.6), each workspace's meter writer owns that workspace's meter stream,
and the deployment's meter writer owns the deployment meter stream ([DEC-2207](../project/decisions/DEC-2207.md)). §9.2's
conventions apply unchanged: every member is present on every record (§4.2), `null` only where the
type is nullable, any other member refused, and opaque text non-empty. Each schema is
`schema_version` 1.

**`ModelInvocationRecorded`** on the agent stream: one record per gateway call the agent's runtime
made, whatever its outcome, appended before the call's output is used (inference spec INF-10). Its
members are §9.9's, with `purpose` for an agent and no draft, then the reservation, the guard's
rule, and the agent harness spec's two ([agent harness spec §6.6](agent-harness.md), DEC-432
item 22). The run id of a research call rides in `correlation_id`. `config_refs.model_version`
names the model called (§9's `mod`).

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The gateway's call (inference §3.1); its retries reuse it: rule 153 |
| `purpose` | `research` \| `llm_signal` \| `fast_signal` | Inference §3.1's purposes for an agent. The compiler's is on the control stream (§9.9) |
| `model` | `{model_id: text, model_version: text, content_hash: ref}` | The registry entry called: rule 146 |
| `endpoint` | `text` | The registry endpoint that served it, or the one it was refused for (`none` when no endpoint was chosen) |
| `request_digest` | `ref?` | SHA-256 of the canonical request (inference §3.1), which holds the rendered prompt; `null` exactly when the guard refused the prompt: rule 148 |
| `sampling` | `{temperature: decimal, seed: integer?}` | `seed` is `null` where the provider takes none |
| `prompt_ref` | `ref?` | The rendered prompt, or `null` exactly when the guard refused it: rule 148 |
| `guard_rule` | `credential` \| `account_identifier` \| `personal_data` \| `foreign_workspace` \| `input_contract` \| `null` | The guard rule a refused prompt broke (inference §8.3), `input_contract` for inputs that differ from the template's contract; non-null exactly for `input_rejected`: rule 148. The content is never recorded |
| `response_ref` | `ref?` | The response, or `null` when no response bytes were kept |
| `reported_identity` | `text?` | What the provider says served the call (INF-2), as `model@provider` |
| `provider_request_id` | `text?` | |
| `outcome` | as §9.9's | Inference §3.3 |
| `attempts` | `integer` | Attempts sent to a provider: rule 152 |
| `tokens` | `{input: integer, output: integer, cached: integer}` | |
| `cost_usd` | `decimal` | From the price table in effect (inference §7.2); not recomputed at append |
| `price_table_ref` | `ref` | The price table version used |
| `cache_hit` | `boolean` | Served from the gateway's cache (inference §3.5) |
| `deadline` | `timestamp` | The call's absolute deadline |
| `completed_at` | `timestamp` | When the gateway finished with it |
| `reservation` | `ulid?` | The `event_id` of the call's `MeterReserved`, or `null` when it reserved nothing: rule 149. The link crosses streams, so `append` does not resolve it |
| `observations` | `[ulid]` | The retrieved context: the `event_id`s of the `ObservationRecorded` records whose items the prompt holds, strictly ascending: rule 150. Empty for a call with no retrieval plan |
| `verdicts` | `[{candidate: integer, kind: thesis \| invalidation, result: judged \| unknown_instrument \| unknown_evidence \| duplicate_instrument \| forbidden_language \| revisions_not_enabled \| unknown_thesis}]` | One per candidate in the response, the theses first and then the invalidations, each in response order: rule 151. `judged` hands the candidate to admission (`ThesisProposed`, §9.4) or to its invalidation record (`ThesisInvalidated`, §9.22, rule 186); every other result is a harness check's refusal ([agent harness spec §6.5](agent-harness.md), HI-22). For an agentic run's turns, which calls are the candidates is §9.23's |

Rules 72, 73 and 75 apply to this record as to the control stream's, and rule 74 does except where
rule 152 says. Rules 71 and 76 are the control stream's alone.

**`StreamOpened`** on the meter stream: `{stream_type: "meter", workspace_id: id}`.

**The deployment meter stream** ([DEC-2207](../project/decisions/DEC-2207.md)),
`dmeter:{deployment_id}`, where `deployment_id` is the deployment's enrolled id (§9.17's
`ControlPlaneEnrolled`). It opens with `StreamOpened`:
`{stream_type: "deployment_meter", deployment_id: id}`, and its one writer is the deployment's meter
writer, fenced by epoch like every writer. It holds a copy of every workspace's `MeterReserved` and
`MeterSettled`, so the deployment's cap counts every workspace's calls:

- **A copy is its own record.** It has its own `event_id` (§3), and every member equal to the
  workspace record's except a settlement's `reservation`, which names the `event_id` of the
  deployment stream's copy of its `MeterReserved`. So rule 158 holds on this stream as written, and
  call ids, which are ULIDs, never repeat across workspaces. That a copy agrees with its workspace
  record is checked by no rule at append; the verifier owns it, as for the cross-stream links below.
- **The order.** The workspace's writer checks every workspace cap against its own stream and
  appends nothing; then the deployment's writer checks the deployment cap and appends the copy, its
  check and append one step; then the workspace's writer appends its reservation, and only then may
  the first attempt leave. The workspace's writer admits one reservation at a time, from its check to
  its own append, so no two calls are admitted against the same headroom on either stream. A
  settlement is appended to the workspace's stream and then copied here.
- **What a failure leaves.** A refusal at either check, or a deployment writer that cannot be
  reached or is fenced, appends nothing to either stream (`meter_unavailable`), or leaves an orphan if
  its append was in fact committed. A workspace append
  that fails after the copy leaves an **orphan**: a reservation on this stream with no workspace
  record and no `ModelInvocationRecorded` naming it, open and counted at its maximum, never settled;
  the call is `meter_unavailable` and sends nothing, and the operator is alerted. A settlement copy
  that fails leaves its reservation open at its maximum, as a crash does. So this stream never
  counts a call below its reserved maximum, nor below the workspace stream's count of it, except
  for a settlement above its reservation that the workspace stream holds and this stream does not.
- **Who reads it.** It holds other workspaces' call, agent, and model ids, so it is the platform
  operator's: its hot-store rows carry no `workspace_id` (§6.1), no workspace member's read, range,
  or export (§9.13) includes it, and no workspace's stream copies from it. A workspace's offboarding
  or erasure leaves its ids here, as ids and costs only, for the records retention period (§6.2).
  Rules 155 to 158 bind it as they bind a workspace's meter stream; rule 154 names its id. Its
  reference vectors follow with the meter streams' registration in `mandate-journal` (DEC-998
  item 8), before any live call.

**`MeterReserved`** on the meter stream: a call's reservation of its maximum cost (inference spec
§7.2), appended by the workspace's one meter writer before the call's first attempt leaves the
deployment, after every cap that applies admitted it (inference spec §7.3). A refusal reserves
nothing and has no record on the workspace's meter stream, `meter_unavailable` included (inference
spec §3.3), and neither does a cache hit, which reaches no provider and costs nothing. The one
refusal that leaves a record anywhere is the deployment meter stream's orphan above.

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The gateway's call: rule 158 |
| `agent_id` | `id?` | The calling agent, or `null` for the compiler and an evaluation: rule 156 |
| `purpose` | `research` \| `llm_signal` \| `fast_signal` \| `compiler` \| `evaluation` | Inference §3.1 |
| `model` | `{model_id: text, model_version: text, content_hash: ref}` | The registry entry the call is for |
| `endpoint` | `text` | The endpoint the call is sent to |
| `key_owner` | `platform` \| `customer` | Whose provider key pays (inference §7.1); both count toward every cap |
| `price_table_ref` | `ref` | The price table the maximum is computed from |
| `max_cost_usd` | `decimal` | The call's maximum cost (inference §7.2): rule 156 |
| `risk_day` | `date` | The risk day ([mandate spec §5.4](mandate.md#54-daily-loss-dec-40-dec-49-dec-54)) the call counts against, from the meter writer's clock at the reservation; its calendar month is the month it counts against. The writer asserts it and `append` does not check it against `event_time` |

**`MeterSettled`** on the meter stream: the call's actual cost, appended once when the gateway is
done with the call.

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The reserved call: rule 158 |
| `reservation` | `ulid` | The `event_id` of its `MeterReserved`: rule 158 |
| `outcome` | `ok` \| `deadline_exceeded` \| `provider_unavailable` \| `rate_limited_provider` \| `credential_invalid` \| `content_refused` \| `schema_invalid` \| `identity_mismatch` | Inference §3.3. A refusal never reserved, so it never settles |
| `attempts` | `integer` | Attempts sent to a provider: rule 157 |
| `tokens` | `{input: integer, output: integer, cached: integer}` | As the provider reported them |
| `cost_usd` | `decimal` | From the reservation's price table and the reported tokens, or the reservation's maximum when the reply's cost cannot be read (inference §3.4); `append` checks only that it is not negative (rule 157) |

**Spend** folds from the meter stream alone (inference spec §7.3). For a scope (the workspace, or
one `agent_id`, on the workspace's stream; the deployment, on the deployment meter stream) and a
period (one risk day, or every risk day of one calendar month; no other period exists at this version, so the license's cycle allotment of inference §7.3 is not yet folded), spend
is the sum, over the scope's `MeterReserved` records in the period, of the settlement's `cost_usd`
for a settled reservation and the reservation's `max_cost_usd` for an open one. An open reservation
is one with no `MeterSettled`: a call still in flight, or one whose process died. It stays counted at
its maximum and is never settled later, by a restart or by a new writer. So the only record that
lowers spend is a reservation's own settlement, down to that call's cost; a crash, a restart, or a
hand-over never does. A call is admitted only when, for every cap that applies, spend plus its own
`max_cost_usd` is at most the cap; so spend never exceeds a cap while each settlement is at most its
reservation. A settlement above its reservation (a provider that reports more tokens than the
request allowed) is still recorded as reported and counted, and the gateway alerts the operator; the
next call is then refused sooner. Every refusal and cache hit is on the caller's
`ModelInvocationRecorded` alone, so the meter counts exactly the calls that could cost money
(backlog E15-12 minor 7).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

146. `ModelInvocationRecorded` on the agent stream: `model.content_hash` equals
     `config_refs.model_version` (`payload.model.content_hash`), as rule 71 does on the control
     stream. A missing ref is already `missing_config_ref`.
147. `ModelInvocationRecorded` on the agent stream: the actor is an `agent` (`actor.kind`), the
     runtime that owns the stream.
148. `ModelInvocationRecorded` on the agent stream: when `outcome` is `input_rejected`,
     `prompt_ref` and `request_digest` are `null` and `guard_rule` is not; for every other outcome,
     `prompt_ref` and `request_digest` are not `null` and `guard_rule` is; each reported at itself,
     the first that fails in that order. A prompt the guard refused is never stored, and neither is
     the canonical request that holds it, so a planted credential or account identifier never
     reaches the artifact store (inference INF-8, INF-14); the record names the rule, never the
     content.
149. `ModelInvocationRecorded` on the agent stream: `reservation` is `null` exactly when the call
     reserved nothing: its `outcome` is a refusal (rule 73's six) or `cache_hit` is true
     (`payload.reservation`).
150. `ModelInvocationRecorded` on the agent stream: `observations` is strictly ascending by bytes
     (`payload.observations`, reason `non_canonical`), so no item is listed twice.
151. `ModelInvocationRecorded` on the agent stream: `verdicts` is empty unless `purpose` is
     `research` and `outcome` is `ok` (`payload.verdicts`). Then, for each item *i* in order, the
     first that fails: its `candidate` is *i* (`payload.verdicts[i].candidate`); no `thesis` item
     follows an `invalidation` item (`payload.verdicts[i].kind`); and its `result` fits its
     `kind` (`payload.verdicts[i].result`): `unknown_thesis` only for an invalidation, and
     `unknown_instrument`, `unknown_evidence`, `duplicate_instrument`, and
     `revisions_not_enabled` only for a thesis. So every candidate has exactly one verdict.
152. `ModelInvocationRecorded` on the agent stream, a call that is neither a refusal nor a cache
     hit: rule 74's `attempts` ≥ 1 holds except for a `deadline_exceeded` whose deadline passed, or
     a `credential_invalid` whose key the vault did not hold, before the first attempt left, which
     records `attempts` 0 (`payload.attempts`). A call with `attempts` 0 sent nothing:
     `response_ref`, `reported_identity`, and `provider_request_id` are `null`, every `tokens`
     member is 0 (reported at `payload.tokens`), and `cost_usd` is 0, the first that fails in that
     order. The control stream's rule 74 is unchanged.
153. Agent stream, one record per call: a `ModelInvocationRecorded` names a `call_id` that no
     earlier `ModelInvocationRecorded` of the stream names (`payload.call_id`), so no second record
     of a call, such as an `ok` after its `deadline_exceeded`, can bring a late or substituted output
     into the stream (inference §3.4). It is a stream rule: `append` reads the stream's earlier
     records, and a range judges it from the stream's start.
154. Meter-stream `StreamOpened`: `stream_id` equals `meter:{workspace_id}` (`stream_id`, reason
     `stream_mismatch`), as rule 25 does on the control stream; on the deployment meter stream,
     `stream_id` equals `dmeter:{deployment_id}` (the same path and reason).
155. `MeterReserved` and `MeterSettled`: the actor is a `system` (`actor.kind`), the gateway's meter
     writer.
156. `MeterReserved`: `agent_id` is `null` exactly when `purpose` is `compiler` or `evaluation`
     (`payload.agent_id`), and `max_cost_usd` ≥ 0 (`payload.max_cost_usd`); the first that fails.
     A local endpoint priced at zero reserves 0.
157. `MeterSettled`: `cost_usd` ≥ 0 (`payload.cost_usd`); `attempts` is 0 only for a
     `deadline_exceeded` or a `credential_invalid` (`payload.attempts`), as rule 152 allows; and a
     settlement with `attempts` 0 has every `tokens` member 0 (`payload.tokens`) and `cost_usd` 0
     (`payload.cost_usd`); the first that fails.
158. Meter stream, one reservation and one settlement per call: a `MeterReserved` names a `call_id`
     that no earlier `MeterReserved` of the stream names (`payload.call_id`); a `MeterSettled` names
     as `reservation` the `event_id` of an earlier `MeterReserved` of the stream
     (`payload.reservation`), with the same `call_id` (`payload.call_id`), that no earlier
     `MeterSettled` names (`payload.reservation`); the first that fails. It is a stream rule:
     `append` reads the stream's earlier records, and a range judges it from the stream's start.

**Checked by no rule at append**, so the verifier owns them (a §11 check is a follow-up, DEC-998
item 9): that a record's `reservation`, `call_id`, and cost agree with the meter stream's records of
that call (they are on two streams); that a settlement's `cost_usd` is its tokens at its price table;
and that every output event (`ModelOutputRecorded`, `ThesisProposed`) follows an `ok`
`ModelInvocationRecorded` that names its model (INF-10).

**No mapping to `JournaledFact`.** No risk state, limit, or order folds from these records. The
gateway folds the meter stream for its caps; `mandate-research`'s check 7 reads the research agent's
spend for the risk day from that fold (inference spec §7.3); the runtime folds
`ModelInvocationRecorded` only to know which outputs are fresh, through the output events that
follow an `ok`.

### 9.20 Scheduler-stream records ([DEC-1010](../project/decisions/DEC-1010.md))

The scheduler stream of §2, `clock:{workspace_id}`, and its five records: `StreamOpened`,
`ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`, and `ClockToleranceExceeded`. Each is
closed at schema version 1 on this stream as §9.15 closes its records: every listed member is
present, `null` only where the type is nullable, and any other member is `schema` at that member.
§9.1's types and report order apply, with §9.2's `date` and `risk_clock`: a member's type, a nested
member's included, is checked before any numbered rule. The rules number on from §9.19's. None of
the five names a configuration (`config_refs` is `{}`), none is an account-stream risk input, so
none carries a `risk_clock` member of that name, and none holds free text but a time server's name.
Offsets and delays are signed `decimal` seconds (§4.6), never integers, which §4.4 keeps
non-negative.

**Who writes them.** The workspace scheduler, the stream's single writer under §5.1's fencing, as a
`system` actor with `clock_source` `scheduler`: rule 159. A second scheduler takes the stream by
taking a newer writer epoch, and the older one's next append is `Fenced` before it writes anything
(infrastructure design §3.4). Nothing else appends to this stream. `StreamOpened` is written once,
at seq 1; a restarted scheduler continues the stream and writes none.

**The account-stream copies.** The executor's copies on an account stream (§2) are not closed
here, and this section binds only the scheduler stream's records. The `ClockAdvanced` copy is closed
in §9.5 at schema version 1, with `clock`, `risk_clock`, and rules 301 and 302 (v0.94,
[DEC-2808](../project/decisions/DEC-2808.md)). The `TradingDayStarted` copy keeps §9's row until
the story that writes it (E7-4 slice 5) closes it.

**`StreamOpened`** on the scheduler stream. As on the agent stream (§9.1), the schema is chosen by the
stream type of `stream_id`.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `scheduler` | |
| `workspace_id` | `id` | The subject: rule 160 |

**`ClockAdvanced`**: one tick of the risk clock (mandate spec §5.2).

| Member | Type | Meaning |
|---|---|---|
| `clock` | `risk_clock` | The tick |

The scheduler writes one tick for each second that elapses on its own monotonic clock, and each
tick's `clock` is the later of the previous tick plus one second and the host clock's UTC time
truncated to the whole second. So the risk clock never runs backwards or repeats, and never stops:
when the host clock steps back, or a restart lands within the last tick's second, the ticks go on
one second at a time from the last one rather than waiting for the host clock to pass it, so no
time-based exit or timer is held (`AGENTS.md` rules 3 and 13). When the host clock steps forward,
the next tick follows it; that step is what the measurements below detect. The first tick of a
stream is the host clock's second. `append` does not compare `clock` across records at this
version; the writer holds it, and its tests show it.

**`TradingDayStarted`**: a trading day began.

| Member | Type | Meaning |
|---|---|---|
| `date` | `date` | The America/New_York trading date |

It is written in the same batch as, and right after, the first `ClockAdvanced` whose `clock` falls
on a trading date (the trading calendar, [trading spec §2.2](trading-domain.md#22-time-and-calendars))
later than the stream's last `TradingDayStarted`, so each trading date is written once and in
order. `append` does not compare dates across records at this version.

**`ClockOffsetRecorded`**: one measurement of the host clock against its time servers (§5.4).

| Member | Type | Meaning |
|---|---|---|
| `trigger` | `startup` \| `before_session` \| `periodic` | Why it was measured: at start, before the regular session, or the measurement at least every 10 minutes (§5.4) |
| `samples` | `[clock_sample]` | The servers that answered, strictly ascending by `server`: rule 161. A server that did not answer, or answered unsynchronized (stratum 0 or 16), is not a sample |
| `offset` | `decimal?` | The host clock minus the servers' time, in seconds: the sample offset rule 162 selects, `null` when there is no sample |
| `tolerance` | `decimal` | The tolerance it was judged against, in seconds (default `0.05`, 50 ms): rule 161 |
| `within_tolerance` | `boolean` | Rule 163: `true` only when `offset` is non-null and its magnitude is at most `tolerance`. A measurement no server answered is out of tolerance (`AGENTS.md` rule 3) |

`clock_sample` is an object with exactly these members, each present:

| Member | Type | Meaning |
|---|---|---|
| `server` | `text` | The time server's configured name; never a credential |
| `stratum` | `integer` | Its NTP stratum, 1 to 15: rule 161 |
| `offset` | `decimal` | Its offset: the host clock minus its time, in seconds |
| `delay` | `decimal` | The round-trip delay in seconds, at least 0: rule 161 |

**`ClockToleranceExceeded`**: the measurement it follows was out of tolerance (§5.4).

| Member | Type | Meaning |
|---|---|---|
| `offset` | `decimal?` | The measurement's `offset`, `null` when no server answered: rule 164 |
| `tolerance` | `decimal` | The measurement's `tolerance`: rule 164 |

The scheduler writes it in the same batch as, and right after, every `ClockOffsetRecorded` whose
`within_tolerance` is `false`, with that record's `event_id` as its `causation_id` and the same two
members, and writes it at no other time.

**What reads the measurements.** Infrastructure design §3.4 makes the clock offset a start-up check
whose failure refuses to start trading. Read the stricter way, which adds no risk (DEC-176): while
the workspace's last `ClockOffsetRecorded` has `within_tolerance` `false`, none has been
written, or the last is older than 15 minutes by the risk clock (§5.4's 10-minute cadence with
slack, so a scheduler that stopped measuring is not read as a good clock), the workspace's long-running executor process (design §3.1) takes no new opening. The
one-shot runs that read no scheduler stream are not changed by it. Risk exits, protective orders, the kill switch, and the
ticks themselves are never held by a measurement (`AGENTS.md` rules 3 and 13), and broker
`event_time` stays authoritative for executions (§5.4). The executor's reading is its own story's
(E21-30); until it lands nothing reads these records.

**Flagged records.** A record is flagged in an export (§5.4) when its `recorded_at` falls inside an
out-of-tolerance interval of its workspace's scheduler stream: from the `recorded_at` of the last
`ClockOffsetRecorded` with `within_tolerance` `true` before a `ClockToleranceExceeded`, or from the
stream's `StreamOpened` when there is none, to the `recorded_at` of the next `ClockOffsetRecorded`
with `within_tolerance` `true`, or without end while there is none. Intervals are over the stream,
whatever writer epoch wrote them. The interval starts at the last good measurement rather than the
bad one, because the clock may have drifted at any time between them, so it flags more records,
never fewer. A flag is computed by the export from these records; no record is changed, and no risk
input reads the flag.

**Consistency rules** (reason `schema` unless named; the path is the member named), checked in
number order after the members' types:

159. Every §9.20 record: `actor.kind` is `system` (`actor.kind`), and `clock_source` is `scheduler`
     (`clock_source`).
160. Scheduler-stream `StreamOpened`: `stream_id` equals `clock:{workspace_id}` (`stream_mismatch` at
     `stream_id`).
161. `ClockOffsetRecorded`: `tolerance` is greater than 0 (`payload.tolerance`); each sample's
     `stratum` is 1 to 15 (`payload.samples[i].stratum`) and its `delay` at least 0
     (`payload.samples[i].delay`); and the samples are strictly ascending by the bytes of `server`,
     so none is listed twice (`non_canonical` at `payload.samples`).
162. `ClockOffsetRecorded`: `offset` is `null` exactly when `samples` is empty, and otherwise
     equals the offset of the sample at index n / 2, rounded down, of the n samples sorted by the
     magnitude of `offset` ascending, then by `offset` ascending (`payload.offset`). That is the
     median by magnitude, and of two samples the larger, so one server that disagrees with a
     second is never outvoted by it toward tolerance.
163. `ClockOffsetRecorded`: `within_tolerance` is `true` exactly when `offset` is non-null and its
     magnitude is at most `tolerance` (`payload.within_tolerance`).
164. `ClockToleranceExceeded`: `tolerance` is greater than 0 (`payload.tolerance`); `offset` is
     `null` or its magnitude exceeds `tolerance` (`payload.offset`); and `causation_id` is non-null
     (`causation_id`), naming the measurement it follows. `append` does not check at this version
     that it names a `ClockOffsetRecorded`; the writer does.

**No vectors.** As for §9.15 and §9.16, `reference/journal` does not model the scheduler stream, and
the test vectors stay version 3: `mandate-journal`'s tests hold an invalid draft for every member
type and rule above and valid drafts for the cases a rule might be misread to refuse, and the
scheduler's own tests hold the writer's rules for `clock`, `date`, and the tolerance batch.

### 9.21 Deployment manager records ([DEC-1170](../project/decisions/DEC-1170.md))

The deployment manager's record of the processes it starts and stops (infrastructure design §3.4,
§7.3), closed at schema version 1 as §9.7's records are: every listed member is present, `null`
only where the type is nullable, and any extra member is refused. §9.1's types apply. The rules
number on from §9.20's. No record names a configuration.

**Who writes them.** Workspace control services, the control stream's writer (§2, under the
fencing of §5.1), as a `system` actor: rule 165. The deployment manager is one of them (design
§3.1); it appends these three records and nothing else, so it never writes a mandate, a
deployment, an envelope, or a command.

**Where they go.** On the workspace control stream, `ctl:{workspace_id}`, of the workspace whose
process it is.

**Journal before acting.** A start or a stop is appended, and the manager calls the orchestrator
only after `Committed` or `AlreadyCommitted` (§5.2). A call that fails is retried for the same
`generation`, which is never appended twice. A slot (one `process` and `subject`) gets its next
`generation` only once every earlier one is seen exited or gone, so the manager never runs two
writers of one stream; each process still takes its stream's writer epoch, which fences any copy
the manager could not see (§5.1).

**Which account a deployment runs on** ([DEC-1171](../project/decisions/DEC-1171.md), §9.24). The
executor a deployment needs is named by its account's `account_ref` (`subject`). The manager reads
it from the control stream alone, never from the mandate document: a version-2 `AgentDeployed`
names its `connection_id`, and the account is the `account_ref` of that connection's version-2
`ConnectionEstablished` earlier on the same stream, read as a connection kill switch's scope is. A
deployment with no such account starts nothing, and the manager fails closed on it: a version-1
`AgentDeployed`, which names no connection, and a version-2 one that rule 201 refuses (its
connection not established with an `account_ref` on this stream before it, or revoked since). A
later establishment does not revive it. No store outside the journal stands between a deployment
and its processes, so a store's outage can delay none. The manager reads the account once, from
the deployment's record, and rule 66 keeps one `account_ref` for the life of a connection, so the
account the manager runs for a deployment never changes while it runs. That it is the account the
agent's own document names rests on rule 200, which the writer and §9.2's mapping judge, never the
manager; mandate spec V-031 keeps every later version on the same connection. A mismatched record
that reached the stream anyway fails closed (§9.24 rule 200).

**Which fact is a restart.** A `ProcessStartIssued` whose `cause` is `restart`. The crash-loop
bound of design §3.4 (Proposed: five restarts within ten minutes) counts the `recorded_at` of the
slot's restarts since its `deployed` start, never the orchestrator's count, which resets when a
pod is replaced. The orchestrator restarts nothing on its own, so every restart is journaled. A
start whose `cause` is `kill_switch` is not counted.

**Which fact is the pause.** `ProcessCrashLooped`. After it the manager starts the slot again only
for a kill switch, once for the covering switches not yet answered (v0.76), and
raises one opaque operator alert (design §8.2, RB-14). Nothing at this version ends the pause but
the agent's `AgentStopped`, or for an executor every agent of its account stopped (DEC-1170 item
3). An `AgentStopped` is final: no record here follows it for that agent's runtime, whether or
not the manager read the deployment it stops (a version-1 `AgentDeployed`, or a version-2 one rule
201 refuses), so a later `AgentDeployed` of that agent starts nothing. No writer writes one, since
an agent id is deployed once (§9.24, [DEC-1173](../project/decisions/DEC-1173.md)).

**Who retires an agent no process ran** ([DEC-2039](../project/decisions/DEC-2039.md)). An owner's
Stop reaches an agent whose runtime ran as an `OwnerCommandIssued`, which the runtime judges
(mandate spec §6.1). An agent whose runtime never ran has no process to judge it: a version-1
deployment, a version-2 one rule 201 refuses, or one the manager has not yet started. For such an
agent, the workspace service that takes the owner's Stop (the CLI's `agent stop`, the workspace
API's) writes the `AgentStopped` itself: reason `owner_stop`, `loss_added` 0, and the connection
its deployment names (version 2) or its stored document names (version 1).

- **The claim.** The control stream cannot prove a runtime never ran: one started outside the
  manager, as a founder's run of `mandate-daemon` is, journals no `ProcessStartIssued`. So the
  service first claims the agent's stream: it appends that stream's `StreamOpened` (§9.1) at
  expected head 0, as its own actor, under the writer epoch the stream's head holds (§5.1). It
  takes no epoch of its own, so it fences no runtime, and it claims nothing once the stream holds
  a record. The append is the emptiness check, and its outcome decides:
  - `Committed`: nothing ran for the agent, so it holds no order, fill, or position, and its net
    dollar loss (mandate spec §5.7) is 0. The service appends the `AgentStopped`.
  - `HeadMismatch`: the service reads the stream's first record. A claim is an earlier Stop that
    did not finish, and the service resumes it with the `AgentStopped`. A `StreamOpened` an
    `agent` actor wrote means the agent ran.
  - `Fenced`: a runtime took the stream after the service read its head, so the agent may run.
- **The retirement.** The service appends the `AgentStopped` at the control head it read (§5.2).
  A record appended in between fails the head check, and the service decides again from the new
  head; the claim stands.
- **Otherwise** it commits the Stop as an `OwnerCommandIssued`, as before: an agent that ran, one
  never deployed, a version-1 deployment whose document cannot be read, or a control stream that
  keeps moving. A Stop is never refused for this (`AGENTS.md` rule 13).
- **The runtime and the manager.** A runtime started on a claimed stream refuses it (§9.1) and
  exits as `stream_claimed`, which the orchestrator reports apart from any other exit. The manager
  neither restarts that exit nor counts it toward the crash-loop bound: it raises one opaque
  operator alert and waits for the agent's `AgentStopped`, which follows the claim at the
  service's next append, or at the owner's next Stop if the service stopped between the two.
- **What it leaves untouched.** It writes no record of §9.21's, touches no account stream, and
  changes no kill switch: a kill switch never needs a live runtime (below).

**The kill switch is never gated here.** No record here is written because of a kill switch except
a `kill_switch` start: the manager never stops, drains, or replaces a process for one, and never
waits on readiness (design §3.4, §3.6).

**An executor no agent wants** (v0.76, [DEC-2344](../project/decisions/DEC-2344.md)). A covering
kill switch reaches an account's executor after the last agent of the account is retired, so the
switch never goes unread:

- **The start.** An executor slot that no deployed agent wants, with no instance that may run, gets
  one `kill_switch` start for the covering switches it has not answered, whether or not its crash
  loop is journaled; each later covering switch that finds it down gets one more.
- **What answers a switch.** Only a start committed after it: the manager reads only the control
  stream, so it answers by starts alone, never by the executor's `KillSwitchActivated`. A
  generation that ends with success answers only the switches committed before its own start; the
  manager reads every covering switch committed since as not answered and starts the executor
  again for it.
- **The held stop.** The manager journals no `ProcessStopIssued` for an executor generation that a
  covering switch reached and has not seen answered, nor for one a `kill_switch` start made while
  no agent wanted it: that generation ends by its own exit, with success once no agent of the
  account is wanted and every covering switch it read is applied. So `ProcessStopIssued`'s causes
  are unchanged.
- **A crash before that** is restarted as any crash is, cause `restart`. If the crash-loop bound is
  reached first, the switch is unapplied: the operator alert is `kill_switch_unapplied` in place of
  `crash_loop`, a fixed key and no subject (`AGENTS.md` rule 6).

**No content.** Every member is an `id`, a closed enumeration, or an `integer`: no instrument,
quantity, price, mandate field, or personal data (`AGENTS.md` rule 6).

**`ProcessStartIssued`**: the manager is about to start one process.

| Member | Type | Meaning |
|---|---|---|
| `process` | `runtime` \| `executor` \| `token_exchange` | The agent's runtime, the account's executor, or an OAuth connect's token exchange (v0.70) |
| `subject` | `id` | The agent, for a runtime; the `account_ref`, for an executor; the `connection_id`, for a token exchange |
| `generation` | `integer` | 1 for the slot's first start, one more for each start after it: rules 166 and 167 |
| `cause` | `deployed` \| `restart` \| `kill_switch` \| `requested` \| `exchanged` | The first start; a start after the previous one exited; a start for a kill switch whose scope covers the slot; a token exchange's one start, for its connection's `ConnectionRequested`: rule 256; a connect's executor's first start, on its token exchange's success: rule 257 |

A `kill_switch` start names the switch's `OwnerCommandIssued` as its `causation_id`: of the
covering switches it answers, the one committed first (v0.76). Every other record here has a `null`
one. `append` does not check the link, which crosses no stream but
names an earlier event, as for §2's copies.

**The token exchange** (v0.70, [DEC-1175](../project/decisions/DEC-1175.md), [DEC-2627](../project/decisions/DEC-2627.md)).
On a control-stream `ConnectionRequested` at version 2 with `credential` `oauth`, the manager starts
one token-exchange process for the `connection_id` (connections spec §5.2 step 4), once it has read
the `ConnectionRolesMade` that names that request (§9.33, v0.77): the exchange logs in with a role
made for its connection alone, so it is never started before that role exists. A request with no
`ConnectionRolesMade`, or with a `ConnectionRolesRefused`, starts nothing, and the connect ends by
its deadline. It is started once, at generation 1 with cause `requested`, and never again: an exchange that failed or ran out of
time is not retried (one attempt; the code it read is spent), no kill switch starts it, and no
crash loop names it. It ends on its own by the request's deadline, and the manager stops a running
one, cause `request_closed`, as soon as its request closes (`ConnectionEstablished`,
`ConnectionRefused` with occasion `connect`, or `ConnectionRevoked`). It journals nothing itself
(§5.2 step 4).

**The connect's executor** (v0.74, [DEC-2335](../project/decisions/DEC-2335.md), DEC-2627 item 5).
When the request's token exchange exits successfully while the request is open, the manager starts
the pending connection's executor for the request's `account_ref`, at generation 1 with cause
`exchanged` (connections spec §5.2 steps 4 and 5). From then it is wanted until the connection
closes: through `connecting`, where it may only run checks and append their results, and after
`ConnectionEstablished`, where it acts on the account (§5.2 step 5), whether or not an agent is
deployed on it. A `ConnectionRefused` with occasion `connect`, or a `ConnectionRevoked`, ends it:
the manager stops it, cause `connection_closed`, unless a deployed agent of its account still wants
it, as §9.24's deployments keep theirs. It restarts, and answers kill switches, as any executor
does. A failed exchange starts nothing.

**`ProcessStopIssued`**: the manager is about to stop one running process.

| Member | Type | Meaning |
|---|---|---|
| `process`, `subject` | As `ProcessStartIssued`'s | |
| `generation` | `integer` | The generation stopped: rule 166 |
| `cause` | `agent_stopped` \| `request_closed` \| `connection_closed` | The agent's `AgentStopped`, or for an executor the last deployed agent of its account stopped; for a token exchange, its request closed: rule 256; for a connect's executor with no deployed agent, its connection refused or revoked: rule 257 |

**`ProcessCrashLooped`**: the crash-loop bound was reached; the manager stops restarting the slot.
Its `process` is `runtime` \| `executor` only.

| Member | Type | Meaning |
|---|---|---|
| `process`, `subject` | As `ProcessStartIssued`'s | |
| `generation` | `integer` | The slot's last generation: rule 166 |
| `restarts` | `integer` | The restarts counted within the window: rule 168 |
| `window_seconds` | `integer` | The bound's window: rule 168 |

**Consistency rules** (reason `schema`; the path is the member named), checked in number order
after the members' types:

165. Every §9.21 record: `actor.kind` is `system` (`actor.kind`).
166. Every §9.21 record: `generation` is at least 1 (`payload.generation`).
167. `ProcessStartIssued`: `cause` is `deployed`, `requested` or `exchanged` exactly when
     `generation` is 1 (`payload.cause`).
168. `ProcessCrashLooped`: `restarts` is at least 1 (`payload.restarts`), and so is
     `window_seconds` (`payload.window_seconds`).
256. `ProcessStartIssued` (v0.70): `cause` is `requested` exactly when `process` is
     `token_exchange` (`payload.cause`). `ProcessStopIssued`: `cause` is `request_closed` exactly
     when `process` is `token_exchange` (`payload.cause`).
257. `ProcessStartIssued` (v0.74): `cause` `exchanged` names `process` `executor`
     (`payload.cause`). `ProcessStopIssued`: `cause` `connection_closed` names `process` `executor`
     (`payload.cause`).

`append` sees one record and checks no `generation` against the stream's earlier ones at this
version. The test vectors' `deployment_manager` section holds a base draft of each record, an
invalid draft for every member type and rule, and valid drafts for the cases a rule might be
misread to refuse.

**Stream rule** (v0.66), the writer's own, as rule 243 is:

246. The owner's retirement of an agent no process ran, over the agent's stream and the control
     stream:
     - Agent stream: a stream whose `StreamOpened` has an `actor.kind` other than `agent` (a
       claim) holds no record after it (`actor.kind`, at the first record after the claim).
     - Control stream, `AgentStopped` whose `actor.kind` is not `agent`: the agent's stream opens
       with a claim (`payload.agent_id`); its `reason` is `owner_stop` (`payload.reason`); and its
       `loss_added` is 0 (`payload.loss_added`).

     So a workspace service retires only an agent whose stream it claimed before any runtime
     opened it, on the owner's Stop, with no loss. An `AgentStopped` the agent's runtime writes
     (`actor.kind` `agent`) is not judged here: its loss is the runtime's to read.

The writers check rule 246, the service against the agent stream it claimed and the runtime
against the stream it replays, and append nothing that would fail it. `append` sees one record and
does not check it, and §11 does not judge it at this version. The test vectors' `agent_deployed`
section holds its cases as `stops`, each an agent stream and a control stream judged from their
starts.

### 9.22 Thesis invalidation records ([DEC-2202](../project/decisions/DEC-2202.md))

[Mandate spec §8.6](mandate.md#86-thesis-lifetime-and-revision-lineages-dec-118-dec-111) removes
an instrument at once when its thesis's invalidation condition holds. The condition is the model's
own text, so the agent harness takes the verdict from the research call's response ([agent harness
spec §6.7](agent-harness.md)), and the executor removes the instrument (§9.3's `UniverseChanged`,
reason `thesis_invalidated`). This subsection closes the record the removal names as its cause.
§9.2's conventions apply unchanged: every member is present on every record (§4.2), `null` only
where the type is nullable, any other member refused, and opaque text non-empty. The schema is
`schema_version` 1, and §9 requires `config_refs.mandate_version`, the agent's mandate version in
force, which the executor's copy names too.

**`ThesisInvalidated`** on the agent stream: one thesis the agent's runtime invalidated, appended
before the executor removes its instrument. An invalidation never admits, never adds risk, and
needs no approval (`AGENTS.md` rule 2): it is taken on the model's word, with no corroboration
(agent harness spec §6.7). In `exits_only` a research run is review-only, and its invalidations are
the only candidates it hands on.

| Member | Type | Meaning |
|---|---|---|
| `thesis_id`, `lineage_id` | `id` | The invalidated thesis and its lineage, as the thesis record that admitted it states them: rule 187 |
| `instrument_id` | `asset_id` | The thesis's instrument (mandate spec §3, §9.3's type), as that record states it: rule 187 |
| `cause` | `model` \| `source_revoked` | `model`: the research call's judged invalidation verdict (§9.19 rule 151). `source_revoked`: a source the allowlist dropped, which invalidates every live thesis that cites it (DEC-433 item 19, Proposed for the founder; no writer emits it until the founder decides) |
| `candidate` | `integer?` | For `model`, the verdict's index in the call record's `verdicts`; `null` exactly for `source_revoked`: rules 184 and 186 |
| `evidence` | `[text]` | For `model`, the item ids the output cited as showing the condition holds, in the order given, duplicates kept; empty for `source_revoked`: rule 184 |
| `source` | `text?` | For `source_revoked`, the revoked source's id; `null` exactly for `model`: rule 184 |

In the call's judging batch, the `ThesisInvalidated` records follow the call record and precede its
thesis records (`ThesisProposed` or `ThesisRevised`, with their `ModelOutputRecorded`), because the
harness judges a response's invalidations before its theses ([agent harness spec §6.7](agent-harness.md)).
So rule 187 judges each invalidation against the theses the response found, and a thesis the same
response proposes on that instrument is a new admission, not a renewal. The `verdicts` array keeps
rule 151's index order, theses first; that is the record's order, not the judging order.

The envelope's `causation_id` names the cause: for `model`, the `event_id` of the call's
`ModelInvocationRecorded` earlier in the same `append` batch (rule 186); for `source_revoked`, the
`event_id` of the control stream's `ConfigSnapshotRegistered` of the first source allowlist version
that no longer lists `source` ([DEC-101](../project/04-decision-log.md#decisions)). The run id of a
model's invalidation rides in `correlation_id`, as its call record's does.

**Consistency rules** (reason `schema`; the path is the member named):

184. `ThesisInvalidated`: for `model`, `candidate` is not `null` (`payload.candidate`) and `source` is
     `null` (`payload.source`); for `source_revoked`, `candidate` is `null` (`payload.candidate`),
     `evidence` is empty (`payload.evidence`), and `source` is not `null` (`payload.source`); and
     for either, `causation_id` is not `null` (`causation_id`). The first that fails, in the order
     `candidate`, `evidence`, `source`, `causation_id`.
185. `ThesisInvalidated`: the actor is an `agent` (`actor.kind`), the runtime that owns the stream.
186. Agent stream, one record per judged invalidation verdict, over one `append` batch whose drafts
     each passed `Draft::parse`, judged draft by draft in batch order, the first that fails
     reported:
     - a `ThesisInvalidated` whose `cause` is `model` names as `causation_id` a
       `ModelInvocationRecorded` earlier in the batch (`causation_id`); that record's `verdicts`
       has an item at index `candidate` whose `kind` is `invalidation` and whose `result` is
       `judged` (`payload.candidate`); and no earlier `ThesisInvalidated` of the batch names the
       same `causation_id` and `candidate` (`payload.candidate`);
     - a `ModelInvocationRecorded` each of whose `verdicts` items *i* with `kind` `invalidation`
       and `result` `judged` is named, as `causation_id` and `candidate` *i*, by a later
       `ThesisInvalidated` of the batch (`payload.verdicts[i].result`, the first such *i*).

     So every judged invalidation is journaled with its call, exactly once, and no record can name
     a verdict the harness refused, another call's verdict, or no verdict at all. A
     `source_revoked` record names no call, and this rule does not read it.
187. Agent stream, one invalidation per admitted, current thesis: a `ThesisInvalidated` names a
     `thesis_id` that no earlier `ThesisInvalidated` of the stream names (`payload.thesis_id`);
     that an earlier `ThesisProposed` or `ThesisRevised` of the stream with `admitted` `true` names
     (`payload.thesis_id`), with the same `lineage_id` (`payload.lineage_id`) and `instrument_id`
     (`payload.instrument_id`); and that is the `thesis_id` of the latest such record for that
     `instrument_id` (`payload.thesis_id`), so a thesis a revision or a renewal superseded is never
     invalidated in its place. The first that fails, in that order. It is a stream rule: `append`
     reads the stream's earlier records, and a range judges it from the stream's start.

**Checked by no rule at append**, so the verifier owns them (a §11 check is a follow-up, DEC-2202):
that the executor's `UniverseChanged` (`thesis_invalidated`) follows its `ThesisInvalidated` and
copies its instrument, thesis, and lineage (§9.3), the two being on two streams; that a
`source_revoked` record's `causation_id` names an allowlist registration that no longer lists its
`source`, and that the thesis cited that source; and that the thesis was still current when it was
invalidated (not expired, retired, or removed), which the agent stream alone cannot show. A removal
of an instrument already removed changes nothing, so none of these can add risk.

**No mapping to `JournaledFact`.** No risk state, limit, or order folds from this record: the
risk-state fold reads the account stream's `UniverseChanged` (§9.3), which the executor writes from
it. The runtime folds it with the thesis records to know which theses are active (agent harness spec
§6.7), and while the agent holds the instrument its order builder proposes the discretionary exit
with `exit_origin` `removed_instrument` (mandate spec §8.3 step 2,
[DEC-2391](../project/decisions/DEC-2391.md)).

### 9.23 Run records ([DEC-2203](../project/decisions/DEC-2203.md))

An agentic research run ([agent harness spec §6.9](agent-harness.md#69-the-agentic-loop),
[ADR-0005](../adr/0005-agentic-harness.md)) is a session of model turns and tool calls. Each turn is
one gateway call with its own `ModelInvocationRecorded` (§9.19), unchanged: its position in the run
is its order among the run's records, so it needs no turn member. A turn's rule 151 candidates are
the final output's theses and invalidations, or, for an entry with action tools, the reply's
`propose_thesis` and `invalidate` calls that were mediated (a thesis's `judged` verdict hands it to
a provisional admission; a provisional refusal's thesis record is written in that batch, an
`admissible` one's only when the run ends `done`; not
refused `call_budget`,
`max_theses`, `max_repeat_denials`, or `run_ended`), mapped to the verdict list in reply order
within each kind; their verdicts sort theses first, each kind in reply order.
Five records carry the rest, all on the agent stream at `schema_version` 1 and closed as §9.2's
conventions say (every member present, `null` only where the type is nullable, any other member
refused, opaque text non-empty). Each has the run id as `correlation_id`, as the run's turn records
do, and `config_refs.model_version` naming the research entry (§9's `mod`).

**`RunStarted`**: the run's start, before its seed's `ObservationRecorded`, with what the run froze.

| Member | Type | Meaning |
|---|---|---|
| `kind` | `ideate` \| `monitor` \| `owner` \| `explain` | The run kind ([agent harness spec §6.9](agent-harness.md#69-the-agentic-loop); `owner` and `explain` are its §15's): rule 172 |
| `trigger` | `{kind: schedule \| event \| owner_question \| approval, cause: ulid?}` | What started it, and the `event_id` of the record that did (the copied owner question, the `ApprovalRequested`, or for an event the triggering item's `ObservationRecorded`, [DEC-2392](../project/decisions/DEC-2392.md)), `null` for a schedule: rules 172 and 271 |
| `output` | `final_output` \| `action_tools` | How the run gives its opinion (agent harness spec §6.9) |
| `menu` | `[text]` | The tool names the run may call, fixed at its start, strictly ascending: rule 172 |
| `cut_off` | `timestamp` | The run's cut-off (`as_of`) |
| `allowlist_version` | `integer` | The source allowlist version every read uses |
| `data_universe_ref` | `ref` | The data universe as of the start, as an artifact |
| `cost_ceiling_usd` | `decimal` | The run's cost ceiling from workspace configuration (agent harness spec §6.11) |
| `deadline` | `timestamp` | The cut-off plus the entry's `max_run_s`, on the risk clock |
| `memory` | `[ulid]` | The `event_id`s of the agent's own thesis records and earlier `RunSummaryRecorded` records placed in the prompt's prefix, strictly ascending: rule 172. An action's provenance includes them by reference (agent harness spec §6.12) |

**`ToolCallRecorded`**: one tool call a committed reply asked for, refused ones included, in the
batch its turn's record heads, in reply order.

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The turn: the gateway call whose reply asked for it: rule 175 |
| `index` | `integer` | Its position among that reply's tool calls, from 0: rule 176 |
| `tool` | `text` | A name in the run's `menu`: rule 175 |
| `class` | `read` \| `think` \| `action` | The tool's class ([agent harness spec §6.10](agent-harness.md#610-tools)) |
| `arguments_ref` | `ref` | The arguments as the reply gave them, as canonical JSON (§4), stored as an artifact |
| `provenance` | `[ulid]` | For an action, the `event_id`s of the run's `ToolResultRecorded` records with `untrusted` true that the prompt it was written from held, strictly ascending; empty otherwise: rule 177. The seed's items are named by the turn's `ModelInvocationRecorded.observations` |

**`ToolResultRecorded`**: a tool call's result, exactly once: an action's, and a refused read's or
think's, in its turn's batch; a dispatched read's or think's in a later batch with the `ObservationRecorded` of the items it returned, before
the next turn's call is sent (agent harness spec HI-24).

| Member | Type | Meaning |
|---|---|---|
| `tool_call` | `ulid` | The `event_id` of its `ToolCallRecorded`: rule 178 |
| `result` | `ok` \| `refused` \| `failed` | `ok`: the read or think ran, or the action was mediated, whatever its verdict. `refused`: the call returned nothing, refused by the harness or by the reader. `failed`: a read did not answer, or found only an excerpt: rule 179 |
| `code` | `admissible` \| `removed` \| `exit_requested` \| `not_held` \| `unknown_instrument` \| `unknown_evidence` \| `duplicate_instrument` \| `forbidden_language` \| `revisions_not_enabled` \| `unknown_thesis` \| `direction_not_allowed` \| `asset_class_not_allowed` \| `not_eligible` \| `source_not_allowed` \| `not_corroborated` \| `universe_full` \| `not_admitted` \| `out_of_universe` \| `unknown_item` \| `unavailable` \| `call_budget` \| `max_theses` \| `max_repeat_denials` \| `run_ended` \| `empty` \| `uncited_segment` \| `unknown_citation` \| `advice_language` \| `too_long` \| `shown` \| `null` | The model-visible code it was told (agent harness spec §6.13 item 5): an action's verdict, or a refusal's or failure's reason; for the desk's `answer` tool, its check result, one of the last six before `null` ([agent harness spec §15.5](agent-harness.md#15-owner-and-explain-runs-the-desk), DEC-2240). `null` only for an `ok` read or think: rule 179 |
| `untrusted` | `boolean` | The result holds third-party or model-written text (news, filings, theses, run summaries), so an action written after it records it as provenance (agent harness spec §6.12) |
| `content_ref` | `ref` | The result as the model was given it, stored as an artifact, with licensed text replaced by its digest once licensed sources are read (agent harness spec §6.6); for a refusal or failure, the `{"error": code}` object |
| `bytes` | `integer` | The byte length of the result as the model was given it |
| `truncated` | `boolean` | A harness cap removed something from a read's result; the model's own `limit` or `count` is not a cap: rule 179 |
| `observations` | `[ulid]` | The `event_id`s of the `ObservationRecorded` records of every item a read's result holds, earlier ones of this run or an earlier run included, strictly ascending; empty unless the call is a read and `result` is `ok`: rule 179 |

**`RunSummaryRecorded`**: the run's typed summary, the memory the next run reads as untrusted
input (agent harness spec §6.12), once per run that ends.

| Member | Type | Meaning |
|---|---|---|
| `instruments_read` | `[asset_id]` | The asset IDs (§9.3's type) the run's reads returned items about, strictly ascending |
| `watch` | `[{instrument_id: asset_id, reason: earnings \| filing_pending \| price_band \| thesis_expiry \| news_flow}]` | At most 10 entries, as the model gave them, with instruments outside the run's scope dropped: rule 181 |
| `proposed`, `admitted`, `invalidated` | `integer` | The run's theses proposed, theses admitted, and invalidations accepted |

**`RunEnded`**: why the run ended, once, after every other record of the run except a late turn
record (rule 183).

| Member | Type | Meaning |
|---|---|---|
| `reason` | `done` \| `bound` \| `deadline` \| `cost` \| `mode` \| `error` \| `answered` | The model ended it; a bound the core counts ended it; the risk clock passed its deadline, or a turn ended `deadline_exceeded` on a deadline the run's time left had cut; a turn ended `budget_exhausted` under the run's ceiling or any other cap; a mode change, a version change, a withdrawn model or disabled research, Stop, or the kill switch cancelled it; a turn ended with any other error outcome; or an `answer` was shown (§15) |
| `bound` | `max_turns` \| `max_tool_calls` \| `max_repeat_denials` \| `max_theses` \| `null` | For `reason` `bound`, the bound that cut the run off (never `max_turns`); for `done`, `max_turns` or `max_tool_calls` when the run's announced shape ran out rather than the model ending it, else `null`: rule 182 |
| `turns`, `tool_calls`, `result_bytes` | `integer` | The run's `ModelInvocationRecorded` records committed before this record, its `ToolCallRecorded` records, and its read results' bytes |
| `discarded` | `[ulid]` | The `event_id`s of the `ToolCallRecorded` records of the run's pending `propose_thesis` calls that this end discards, strictly ascending; empty for `done`, which commits them: rule 182 (agent harness spec §6.9) |

A run whose process crashed has no `RunSummaryRecorded` and no `RunEnded`; replay reads it as
ended at the crash.

**Consistency rules** (reason `schema` unless named; the path is the member named), checked in
number order after the members' types:

171. Every §9.23 record: the actor is an `agent` (`actor.kind`), the runtime that owns the stream,
     and `correlation_id` is not `null` (`correlation_id`).
172. `RunStarted`: `menu` and `memory` are strictly ascending by bytes (`payload.menu`,
     `payload.memory`, reason `non_canonical`);
     `trigger.cause` is `null` exactly when `trigger.kind` is `schedule` (`payload.trigger.cause`);
     and `kind` fits `trigger.kind`: `ideate` a schedule, `monitor` a schedule or an event, `owner`
     an owner question, `explain` an approval (`payload.trigger.kind`); the first that fails.
173. Agent stream: no earlier `RunStarted` has the same `correlation_id` (`correlation_id`), and an
     `explain` or `owner` run's `trigger.cause` names no earlier run's of the same kind
     (`payload.trigger.cause`), so an approval is explained once and an owner question answered by
     one run. A stream rule: `append` reads the stream's earlier records, and
     a range judges it from the stream's start; rules 174 to 183 are stream rules too.
174. Agent stream: a `ToolCallRecorded`, `ToolResultRecorded`, `RunSummaryRecorded`, or `RunEnded`
     has the `correlation_id` of an earlier `RunStarted` (`correlation_id`), and no `RunEnded` of
     that run precedes it (`correlation_id`); a second `RunSummaryRecorded` of a run is refused
     (`correlation_id`), and a `RunEnded` follows its run's `RunSummaryRecorded` (`correlation_id`).
175. `ToolCallRecorded`: `call_id` names an earlier `ModelInvocationRecorded` of the stream whose
     `purpose` is `research` and whose `outcome` is `ok` (`payload.call_id`), with the same
     `correlation_id` (`correlation_id`) and `config_refs.model_version`
     (`config_refs.model_version`); and `tool` is in the run's `RunStarted.menu` (`payload.tool`);
     the first that fails.
176. `ToolCallRecorded`: `index` equals the number of earlier `ToolCallRecorded` records naming the
     same `call_id` (`payload.index`), so a reply's tool calls are recorded in reply order from 0,
     each once.
177. `ToolCallRecorded`: `provenance` is strictly ascending by bytes (`payload.provenance`, reason
     `non_canonical`) and empty unless `class` is `action` (`payload.provenance`).
178. `ToolResultRecorded`: `tool_call` names an earlier `ToolCallRecorded` of the stream with the same
     `correlation_id`, and no earlier `ToolResultRecorded` names it (`payload.tool_call`).
179. `ToolResultRecorded`, read against its `ToolCallRecorded`'s `class`: `code` is `null` exactly
     when `result` is `ok` and the class is `read` or `think` (`payload.code`); a `failed` result is
     only a read's, and its `code` is `unavailable`; a `refused` result's `code` is one of
     `out_of_universe`, `unknown_item`, `call_budget`, `max_theses`, `max_repeat_denials`, or
     `run_ended`; and an `ok` action's `code` is a verdict: not `unknown_item`, `call_budget`, `max_theses`,
     `max_repeat_denials`, `run_ended`, or `unavailable` (`out_of_universe` may be a mediated
     thesis's, from §8.5 check 8) (`payload.result`);
     `observations` strictly ascending by bytes (`payload.observations`, reason `non_canonical`), and
     empty and `truncated` `false` unless the class is `read` and `result` is `ok`
     (`payload.observations`, `payload.truncated`); the first that fails.
180. `ToolResultRecorded`: among the results of one reply's tool calls (the same `call_id`), either
     every `code` is `call_budget` or none is (`payload.code`): a reply that asked for more tool calls
     than its run had left runs none of them.
181. `RunSummaryRecorded`: `instruments_read` strictly ascending by bytes
     (`payload.instruments_read`, reason `non_canonical`); `watch` has at most 10 entries, no
     instrument twice (`payload.watch`).
182. `RunEnded`: `bound` is not `null` when `reason` is `bound`, and is then not `max_turns`; it
     may be `max_turns` or `max_tool_calls` when `reason` is `done` (`max_turns` when both ran out
     on the last turn); and it is `null` for every other
     reason (`payload.bound`); and
     `discarded` is strictly ascending by bytes (`payload.discarded`, reason `non_canonical`) and
     empty when `reason` is `done` (`payload.discarded`); the first that fails. That each entry
     names a pending `propose_thesis` call of the run (one whose provisional result is `admissible`),
     and that every pending one is named, is the writer's.
183. Agent stream, a `ModelInvocationRecorded` with `purpose` `research` whose `correlation_id` is
     that of an earlier `RunStarted` (a `single_call` run has none, so this rule does not reach it;
     that an agentic turn's record follows its `RunStarted` at all is the writer's): no `RunEnded` of
     the run precedes it unless that `RunEnded`'s `reason` is `deadline` or `mode` and the record's
     `verdicts` is empty (`correlation_id`). A late reply to a cancelled run is so still journaled,
     and rule 175 keeps any tool call of it out (no tool call follows a `RunEnded`, rule 174), as
     agent harness spec §5.3 says. Every other §9.19 rule is unchanged.
271. Agent stream ([DEC-2392](../project/decisions/DEC-2392.md)): a `RunStarted` whose
     `trigger.kind` is `event` names as `trigger.cause` an earlier `ObservationRecorded` of the
     stream with the same `correlation_id` (`payload.trigger.cause`) and a non-null
     `instrument_id` (`payload.trigger.cause`), whose `data_ref` no earlier `RunStarted` with
     `trigger.kind` `event` names through its own cause (`payload.trigger.cause`); the first that
     fails. So an event run's trigger is journaled before it, as part of the run, and one item
     starts at most one run (agent harness spec §5.3). A stream rule, judged after rules 173 to
     183 in number order. That the observation is in the run's starting batch is the writer's,
     checked by no rule, since a range has no batches.

**Checked by no rule at append**, so the writer and its tests own them: that a reply has exactly as
many `ToolCallRecorded` records as tool calls, which needs the response artifact; that every listed
observation names an `ObservationRecorded` of the agent stream (an item first observed by an
earlier run is named, never journaled again) and every provenance entry a record of the run; that `bytes` is the artifact's length;
that the arguments satisfy the tool's pinned schema, which the gateway checked; that the next turn's
record follows every result of the turn before it (agent harness spec HI-24); that an action's `code`
equals its journaled verdict (HI-25); and that `RunEnded`'s counts match the run's records.

**No vectors.** As for §9.15, §9.16, and §9.20, the test vectors stay version 3:
`mandate-journal`'s tests hold an invalid draft for every member type and rule above, and valid
drafts for the cases a rule might be misread to refuse (a refusal with no observations, an `ok` read
with none, a think result with no code, an index after a refused call of the same reply, a turn
record with verdicts and action calls, an `explain` run per approval, a late turn record after a
`deadline` end).

### 9.24 The deployment's connection ([DEC-1171](../project/decisions/DEC-1171.md))

The deployment manager (§9.21) learns a deployment's account from the control stream alone. The
cell has no artifact store it shares with the workspace services, so the manager cannot read the
mandate document an `AgentDeployed` names. Version 2 of `AgentDeployed` restates that document's
connection, and the manager reads the account from that connection's own version-2
`ConnectionEstablished` on the same stream, as it already reads a connection kill switch's scope.
`AgentStopped` already names its connection the same way (§9.2). §9.2's conventions apply: every
member is present, none is nullable, and any other member is refused. Version 1 stays registered,
replayable, and unchanged (§8). The rules here are numbered 200 and 201; the numbers between
§9.23's rules and 200 are left to other changes.

**`AgentDeployed` version 2** on the control stream: version 1's members, then:

| Member | Type | Meaning |
|---|---|---|
| `agent_id`, `mandate_version`, `record_ref` | As version 1's | Rule 22 applies unchanged |
| `connection_id` | `id` | The `connection_id` of the stored mandate document `mandate_version` names ([mandate spec §3](mandate.md#3-structure)), which no later version of that mandate changes (V-031): rules 200 and 201 |

The connection, environment, allocation, and pinned instruments are still the document's. Version
2 restates the connection only, so that a reader without the document can find the account; a
reader that reads the document still takes every one of them from it.

**Who writes which version.** A writer that deploys a stored version (the CLI's `agent deploy`
and the workspace API's deployment, which judge it the same way) writes version 2, taking
`connection_id` from the document it deploys, exactly when rule 201 holds, and version 1 otherwise.
Until those writers move, in the code change that
follows this one, they write version 1, and the deployment manager starts nothing for it (below):
a writer that has not moved fails closed, never open.

**Consistency rule**, judged where the document is read:

200. `AgentDeployed` version 2: `connection_id` equals the `connection_id` of the stored mandate
     document `mandate_version` names. It is judged in two places, and by nothing else:
     - **By the writer, by construction.** Each writer of version 2 (the CLI's `agent deploy` and
       the workspace API's deployment) already holds the stored document it deploys and validates
       it, and takes `connection_id` from that document, so the two never differ and there is
       nothing to compare. It appends the draft at the control stream's head it read, so a record
       written in between (a revocation) fails the append's head check and nothing is appended.
       The CLI reads the rows and decides again from the new head, at most three decisions in
       all; when the stream keeps moving it commits nothing and returns a journal error. The
       workspace API answers `journal_unavailable` (503, retryable), and a retry reads the rows
       again.
     - **By §9.2's mapping.** Where the mapping reads the document for `AgentVersionActive`, a
       version-2 record whose `connection_id` differs from the document's is refused as malformed
       at `payload.connection_id`, as a record whose document is not stored is refused, never
       skipped. So every reader that folds the control stream through `ValidationContext::from_journal`
       (the writers' V-rule checks, the runner's control fold, replay, §8) refuses the whole
       control stream from that record on, and the fact the mapping gives, when it gives one, is
       the same for both versions.

     **`append` does not judge it**: it reads a stored object only for a configuration reference
     (§9, rules 21a and 21b), and a mandate document is not one, so a version-2 draft passes
     `Draft::parse` whichever connection it names. **§11 does not see it either**: its checks read
     the chain and the anchors, never a mandate document's members (check 6 only re-hashes the
     stored bytes). **The deployment manager does not judge it**: it acts on the record alone
     (§9.21). If a mismatch were stored anyway, the manager would start an executor for the
     record's connection's account, and a runtime for the agent, and that is fail-closed: the
     runtime reads its deployment through §9.2's mapping (today the runner's control fold, through
     `ValidationContext::from_journal`), which refuses it, so the runtime never starts trading and the agent proposes no intent for any
     executor; the executor started holds only what its own account stream already holds, adds no exposure,
     and is one slot per account, so it never runs beside another executor of that account. What is
     lost is the deployment itself, which does not run until the mismatch is corrected by a new
     deployment.

**Stream rule** (connections spec §3), held and checked as rules 66 to 68 are: the writer checks it
against its own fold of the stream before appending, and writes version 1 instead of a version-2
draft that would fail it, and §11's
`connection_lifecycle_mismatch` checks it on every range, from the range's connection anchor, which
already holds each id's first establishment and its latest establishment or revocation, or failing
closed without one:

201. Control stream, `AgentDeployed` version 2: an earlier `ConnectionEstablished` version 2 on the
     stream names its `connection_id`, and the latest earlier `ConnectionEstablished` or
     `ConnectionRevoked` of that id is a `ConnectionEstablished` (`payload.connection_id`). So the
     connection was established on this workspace's control stream with an `account_ref`, and not
     since revoked, when the agent was deployed. By rule 66 every establishment of an id whose first
     is version 2 is version 2 with the same `account_ref`, and a version-1 first establishment is
     never established again, so a deployment's account is one `account_ref`, fixed for the life of
     its connection. A version-1 `AgentDeployed` is not judged, so every stream written before
     v0.54 still verifies.

**A writer writes version 2 exactly when rule 201 holds, and version 1 otherwise.** Rule 201
judges only version 2, which adds risk, so it never holds a risk reduction (`AGENTS.md` rule 13),
and a version-2 draft it would refuse is never written: the writer writes version 1, which the
manager does not start (fail closed). Not every deployment written today meets it:

- **Live.** The CLI's `admitted()` already requires, for a live mandate, that the connection's
  latest `ConnectionEstablished` or `ConnectionRevoked` be a version-2 establishment, with an
  `account_ref`, that `connection record` committed ([DEC-529](../project/decisions/DEC-529.md)
  item 3). A live deployment the CLI writes meets rule 201 already.
- **Paper.** `admitted()` returns early for a paper mandate and never reads the connection. When no
  establishment or revocation names the connection, the CLI's V-rule check clears the connection's
  environment, so V-001 does not fire, and it tolerates V-002; Phase 1's paper connections journal
  no `ConnectionEstablished` (`mandate-spec`'s `unrecorded_stop_limit_asset_classes`). So the CLI's
  `agent deploy` deploys a paper agent today on a connection the stream records nothing about. The
  workspace API's deployment clears the environment the same way, though it refuses V-002 for a
  deployment. Such a paper deployment is written as version 1, as today, and is never refused for
  rule 201; the manager starts nothing for it until the owner stops it, records the connection
  (with `connection record`, which needs the connect sequence's `ConnectionChecked`), and deploys
  under a new agent id, since an agent id is deployed once (below).
  The CLI changes in the code change that follows only in writing version 2 when rule 201
  holds.
- **Revoked.** A connection whose latest record is a revocation is known to the fold, so V-001
  refuses a paper deployment on it (no environment, read as `live`), and `admitted()` a live one.

A revocation after the deployment is not judged here: what it stops is §9.10's and the kill
switch's, unchanged.

**An agent id is deployed once** ([DEC-1173](../project/decisions/DEC-1173.md)). A writer of
`AgentDeployed` (the CLI's `agent deploy`, the workspace API's deployment) refuses an agent that an
`AgentStopped` on the control stream names, whatever version it would write and whatever the stop's
`reason` (an owner stop, `goal_complete`, `profit_stop_reached`, or `end_date`), and appends
nothing; the CLI's code is `agent_stopped`, the workspace API's `invalid` at `/agent_id`. An agent
active with the same version is answered with its deployment, as before. So §9.21's final stop and
the writers agree, and a stopped agent's runtime is never wanted again. A new agent id leaves the
lifetime floor as it was: the loss carry is the connection's (mandate spec §5.7;
`AgentStopped.loss_added` adds to the connection's carry, and V-032 reads it), and the high-water
mark starts from the deployment under either id (§5.5). The per-agent daily and conduct state does
restart with a new agent id, as it does for any other agent deployed on the connection: the
`max_daily_loss` latch and its E₀ (§5.4), the drawdown ladder's latches (§5.5, §5.8), the
tripwire windows and a fired tripwire's latch (restriction `tripwire`, §5.8, §5.9, §6.7), the
`goal_complete` restriction, the re-entry cooldown, the order-to-fill limit (trading spec §9.6),
the per-agent part of the order-rate limit and `max_orders_per_day` (§9.7), and MI-25's per-agent
ask caps with their `skipped_today` and `recent_timeout` suppressions. The order-rate limit's
per-account part does not restart. That is bounded by the lifetime floor and by a deployment being the owner's
stepped-up gesture ([DEC-1173](../project/decisions/DEC-1173.md) item 6 leaves keying them by
connection to the founder). `append` does not judge it: it reads no stream.

**`AgentDeployed` version 3** ([DEC-2131](../project/decisions/DEC-2131.md), v0.84) on the
control stream: version 2's members, then the passkey step-up that authorized the deployment, so
§9.32's rule 253 reads it from the record as it reads every other authorized event's. Every member
is present and none is nullable.

| Member | Type | Meaning |
|---|---|---|
| `agent_id`, `mandate_version`, `record_ref`, `connection_id` | As version 2's | Rules 22, 200 and 201 apply as to version 2 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: the passkey step-up the API judged, which the record `record_ref` names also holds: rule 268 |

The workspace API writes version 3 exactly when it would write version 2 (rule 201 holds) and
commits the deployment under **passkey** step-up, and the `StepUpAsserted` follows it in the same
batch (§9.32). Under any other step-up, or none, it writes version 2 or version 1 as above. A
deployment under passkey step-up on a connection rule 201 refuses has no version that carries its
evidence (version 1 has no `step_up`, so rule 253 refuses its `StepUpAsserted`), and the API
refuses it and appends nothing, rather than commit it without its assertion
([DEC-2133](../project/decisions/DEC-2133.md)). That refuses only a deployment the manager would
never start (above), so it adds no risk. The CLI is unchanged. Versions 1 and 2 stay registered, replayable, and unchanged (§8).

**Consistency rule** (reason `schema`; the path is the member named), judged by `append` after rule 22:

268. `AgentDeployed` version 3 (v0.84): the actor is a `user` (`actor.kind`), the owner whose step-up
     it carries, and its `step_up` holds as rule 267's does: `method` is `passkey`
     (`payload.step_up.method`), and 0 ≤ `event_time` − `authenticated_at` ≤ 300 seconds, compared
     to the nanosecond (`payload.step_up.authenticated_at`).

**Every reader of `AgentDeployed`**, by version. Every reader reads version 3 as the version-2
column says, and ignores `step_up`; a reader that matches version 2 alone would read a version 3 as
version 1, or skip it, and drop the deployment from what its connection names, so no writer writes
version 3 until every reader reads it (DEC-2131 item 4):

| Reader | Version 1 | Version 2 |
|---|---|---|
| `append` (`Draft::parse`) | §9.2's schema and rule 22 | Version 2's schema and rule 22, and at version 3 its own schema and rule 268. Rule 200 is not judged (above) |
| §9.2's mapping, so `ValidationContext::from_journal`, the writers' V-rule folds, and replay | `AgentVersionActive` from the stored document, as before | The same fact from the same document; a `connection_id` that differs from the document's is refused (rule 200) |
| The writers' stream rules and §11's `connection_lifecycle_mismatch` | Not judged | Rule 201 |
| §9.21's deployment manager | Names no account: it starts nothing for the deployment, and never reads the document to find one | The account is the `account_ref` of that connection's version-2 `ConnectionEstablished` earlier on the stream; a deployment rule 201 refuses starts nothing |
| The folds that read only `agent_id` and `mandate_version` (the runner's control fold, the workspace API's and CLI's active-deployment reads, §9.4's binding of a thesis to the agent's version, §9.17's first deployment, §9.18's go-live record) | Unchanged | The same two members; `connection_id` is not read |

**Vectors** (v0.54; version 3's are the `stepped_up_records` section's, below). A new generated `agent_deployed` section holds base drafts of both versions with the
stored documents they name; an invalid draft for every member type of version 2 and for rule 22 at
version 2; valid drafts for the cases a rule might be misread to refuse, a mismatch rule 200 leaves
to the mapping among them; mapping cases for rule 200; control streams for rule 201, judged from
the stream's start, from a connection anchor, and with none; and the deployment manager's reading of
each stream. No other section changes, so the vectors stay version 3.

**Order of the changes (ES-22).** Spec and vectors first. One code change follows:
`mandate-journal` registers version 2, the mapping in `mandate-spec` refuses rule 200, the CLI and
the workspace API write version 2 and check rule 201, §11's verifier judges it, and the deployment
manager reads the account from the record and its connection's establishment instead of a resolver
over stored mandates (DEC-1170 items 5 and 9, superseded there by DEC-1171).

**Version 3's vectors** (v0.84). The generated `stepped_up_records` section holds a base draft of
`MandateConfirmed` version 3 and of `AgentDeployed` version 3; an invalid draft for every member
type of `step_up`, for each clause of rules 267 and 268, and for rules 18, 22, 77 and 78 at version
3; valid drafts at both ends of the window and for the cases a rule might be misread to refuse;
batches for §9.32's rule 253 with each version-3 record; §9.2's mapping of each version 3 to its
version 2's fact, rule 200 included; and control streams for rule 201 and §9.21's manager over
version-3 deployments.

**Order of the changes (DEC-2131 item 5).** Spec and vectors first. Then `mandate-journal`
registers both versions, judges rules 267 and 268, and reads version 3 as version 2 in rule 201's
fold, its tests reproducing the vectors (the spec guard keeps the two changes apart). Then every other reader, each with a test that a version 3 reads as its
version 2. Then the workspace API writes `MandateConfirmed` version 3, and then `AgentDeployed`
version 3, under passkey step-up.

### 9.25 Desk question records ([DEC-2240](../project/decisions/DEC-2240.md))

The desk ([agent harness spec §15](agent-harness.md#15-owner-and-explain-runs-the-desk)) is where a
member asks their agent about its own records. The run that answers is an `owner` run of E19-13's
loop, recorded on the agent stream by E19-13's run records. The question itself is owner input, so
it is journaled on the control stream first and copied by the runtime (§2), as `OwnerRequestSubmitted`
is. §9.2's conventions apply: every member is present, `null` only where the type is nullable, any
other member is refused, and opaque text is non-empty. The rules number from 190. The record is not
a risk input and maps to no `JournaledFact`.

**`OwnerQuestionSubmitted`**: a member's desk message (workspace API §4.6.1). Its `event_id` is the
idempotency key of the runtime's copy.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The agent asked |
| `question_ref` | `ref?` | The question as the member typed it, stored; `null` when the gateway's guard refused it, which is then never stored: rule 191 |
| `guard_rule` | as §9.19's | The guard rule a refused question broke; the content is never recorded: rule 191 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

190. `OwnerQuestionSubmitted`: the actor is a `user` (`actor.kind`). A client has no desk
     (workspace API §3.7), and a question is always a person's.
191. `OwnerQuestionSubmitted`: `question_ref` is `null` exactly when `guard_rule` is not
     (`payload.question_ref`), so a refused question is never stored and an accepted one always is;
     and `guard_rule` is never `input_contract` (`payload.guard_rule`), which judges a template's
     inputs, not a question's text.

**No vectors.** As for §9.15 and §9.20, `reference/journal` does not model this record, and the test
vectors stay version 3: `mandate-journal`'s tests hold an invalid draft for every member type and
rule above and valid drafts for the cases a rule might be misread to refuse.

### 9.27 Account-stream risk-limit records ([DEC-2301](../project/decisions/DEC-2301.md))

This subsection closes the payload schemas of the seven account-stream risk-state records §9.3 left
open: `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`,
`PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted` (mandate spec §5.10). It closes
them before anything writes them, so the executor's risk-state fold (E6-15) has a closed record to
write and the executor's own fold a closed record to read back. For these events it replaces the
"Key payload fields" column of §9. §9.3's conventions apply unchanged: §9.1's and §9.2's types,
§9.3's `asset_id`, the absent-member rule, the report order, the required `config_refs` as §9 lists
them, `schema_version` 1, and `risk_clock` on each, since every one is written at a risk input. The
rules number from 230.

The executor writes each record from `mandate-spec`'s risk-state fold and, for a tripwire,
`mandate-executor`'s §6.7 fold, at the input that produced it and in the fold's order (mandate spec
§5.2), before it acts on the change (`AGENTS.md` rule 5). Every member is the fold's event or the
risk state's snapshot after that input, so replaying the account stream's inputs reproduces each
record byte for byte. The risk state is **unreadable** when the executor meets any of these: a
record it cannot decode, a `schema_version` it does not know, a gap in the stream's `seq`, an event
type it does not fold, a journaled record its replay would not have produced, or a record it could not
append. An unreadable risk state holds new openings and never an exit
([DEC-2301](../project/decisions/DEC-2301.md) items 2 and 3). Writing first never delays risk
reduction: a risk-exit flatten, a protective order, or an automated kill switch goes ahead when the
record before it cannot be appended, and the failed append makes the risk state unreadable
(`AGENTS.md` rules 3 and 13). No payload carries a credential or
personal data: an agent, a user, and an order are named by opaque IDs, and step-up evidence by its
assertion.

**Types**, beyond §9.1's, §9.2's, and §9.3's:

| Type | Values | Refused as |
|---|---|---|
| `limit_key` | `max_daily_loss`, `lifetime_floor`, `profit_stop`, `drawdown_ladder[<i>]` with `<i>` from 0 to 4 in decimal without a leading zero (a ladder has at most five rungs), or `tripwire:<id>` with `<id>` matching `^[a-z][a-z0-9_]{0,31}$` (the mandate schema's tripwire `id`) | Not a string: `schema`; otherwise `non_canonical` |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}`, as §9.3's | As §9.3's |

**Members every record carries:** `agent_id` (`id`, the agent whose risk state changed) and
`risk_clock` (`risk_clock`, the input's tick, §2). The tables below list the rest.

**The snapshot members.** `RiskLimitTriggered` and `RiskLimitLifted` carry the risk state's values
after the input, which mandate spec §5.10 lists as E, H, DD, E₀, C, L and §9 adds N to:

| Member | Type | Meaning (mandate spec §5.1 to §5.7) |
|---|---|---|
| `agent_equity` | `decimal` | E |
| `high_water_mark` | `decimal` | H |
| `drawdown` | `decimal` | DD = (H − E) / H, as the risk state computes it |
| `day_start_equity` | `decimal` | E₀ |
| `capital_base` | `decimal` | C |
| `inherited_loss` | `decimal` | L, never negative: rule 231 |
| `net_contributed` | `decimal` | N |

**`RiskDayStarted`**: the risk day rolled over at 00:00 America/New_York (mandate spec §5.4).

| Member | Type | Meaning |
|---|---|---|
| `day` | `date` | The America/New_York date the risk day starts on |
| `day_start_equity` | `decimal` | E₀, the agent equity the day starts from |

**`RiskLimitTriggered`**: a limit, rung, or tripwire latched or took effect (mandate spec §5.5 to
§5.8, §6.7). The snapshot members above, then:

| Member | Type | Meaning |
|---|---|---|
| `limit` | `limit_key` | The limit |
| `action` | `scale_sizes` \| `exits_only` \| `flatten_and_pause` \| `end_delegations` | What it does while it holds: a rung's own action, the daily loss's `daily_loss_action`, or a tripwire's: rule 230 |
| `reason` | `hard_trigger` \| `hard_breach_pending` \| `resolved_at_rollover` \| `new_day_breach` \| `after_reset` \| `tripwire_condition`, or `null` | Why, when it is not plain confirmation by breach time: rule 230 |
| `breach_time_s` | `integer?` | The breach time (mandate spec §5.6) the limit's confirmation holds at the input that produced the record, in whole seconds. The risk fold's event carries no breach time, so the writer reads the confirmation at that input, before the fold's latch removes it. A trigger confirmed by breach time, a confirmed `new_day_breach` included, carries at least `breach_confirm_s`. A hard trigger and `hard_breach_pending` carry whatever had accumulated, which may be more than 0 and less than `breach_confirm_s`. It is 0 only where the limit has no confirmation at that input, as for a scale rung, which mandate spec §5.5 applies immediately, or an `after_reset` rung. `null` for a tripwire: rule 232 |
| `metric` | `consecutive_losing_exits` \| `realized_loss_usd` \| `new_instruments`, or `null` | A tripwire's metric: rule 232 |
| `threshold` | `decimal?` | A tripwire's threshold: rules 232 and 233 |
| `value` | `decimal?` | The metric's value reached: rules 232 and 233 |

**`RiskLimitLifted`**: a limit, rung, or tripwire stopped holding. The snapshot members above, then:

| Member | Type | Meaning |
|---|---|---|
| `limit` | `limit_key` | The limit |
| `action` | as `RiskLimitTriggered`'s, or `null` | The action a rung stops taking; `null` for the rest: rule 234 |
| `reason` | `owner_acknowledged` \| `version_loosened` \| `hard_breach_cleared`, or `null` | Why, when it is not the plain end of the condition: rule 234 |

A lift with `reason` `owner_acknowledged` names the account stream's `OwnerAcknowledged` copy that
lifted it as its `causation_id` (rule 235).

**`HighWaterMarkReset`**: the owner reset H after a ladder latch (mandate spec §5.8).

| Member | Type | Meaning |
|---|---|---|
| `from`, `to` | `decimal` | H before and after |
| `user_id` | `id` | The acknowledging user, opaque |
| `step_up` | `step_up` | The acknowledgment's step-up evidence |

Its `causation_id` names the `OwnerAcknowledged` copy (rule 235).

**`PositionReleased`**: an agent's position became the owner's (mandate spec §3.1).

| Member | Type | Meaning |
|---|---|---|
| `instrument` | `asset_id` | The instrument |
| `qty` | `decimal` | The quantity released: greater than 0 for an owner's release, and possibly 0 for a goal's, which completes whether or not the agent holds a position: rule 236 |
| `cause` | `owner_release` \| `goal_release` | The owner's release, or a goal completing with `on_complete` `release`: rule 236 |
| `canceled_order_ids` | `[id]` | The protective orders canceled with it, possibly none |
| `warning_ref` | `ref?` | The warning the owner was shown, stored: rule 236 |
| `user_id` | `id?` | The releasing user, opaque: rule 236 |
| `step_up` | `step_up?` | The release's step-up evidence: rule 236 |

**`InstrumentRestrictionChanged`**: one instrument restriction set or cleared; one record per
restriction that changed (mandate spec §5.9, §5.10).

| Member | Type | Meaning |
|---|---|---|
| `instrument` | `asset_id` | The instrument |
| `restriction` | `stale_mark` \| `removed_instrument` | Rule 237 |
| `reason` | `no_sane_mark` \| `sane_mark`, or one of §9.3's `UniverseChanged` reasons | Rule 237 |
| `active` | `boolean` | Set (`true`) or cleared (`false`): rule 237 |

**`GoalCompleted`**: a goal completed (mandate spec §3.1), in one of the risk state's two forms
(DEC-128 item 15): the goal-complete input's, which names the `on_complete` applied, and the one
that names what follows instead, with its reason when the risk state confirmed it (a `profit_stop`).

| Member | Type | Meaning |
|---|---|---|
| `reason` | `profit_stop_reached` \| `target_qty` \| `max_spend` \| `end_date`, or `null` | Why, for the second form: rule 238 |
| `then` | `hold_protected` \| `disarm_ladder` \| `release` \| `discretionary_exit_all_then_retire`, or `null` | Rule 238 |
| `on_complete` | `hold_protected` \| `disarm_ladder` \| `release`, or `null` | Rule 238 |

**Consistency rules** (reason `schema`; the path is the member named):

230. `RiskLimitTriggered`: a `tripwire:` limit has `reason` `tripwire_condition` and `action`
     `end_delegations` or `exits_only`, and no other limit has either `tripwire_condition` or
     `end_delegations` (`payload.reason`, then `payload.action`). `scale_sizes` is a rung's only
     (`payload.action`). Mandate spec §6.7: a tripwire never pauses (`AGENTS.md` rule 13).
231. `RiskLimitTriggered`, `RiskLimitLifted`: `inherited_loss` is not negative
     (`payload.inherited_loss`): L only ever adds a loss (mandate spec §5.7). `drawdown` has no bound
     here, since agent equity is not bounded below by an invariant this record could rely on.
232. `RiskLimitTriggered`: for a `tripwire:` limit, `metric`, `threshold`, and `value` are non-null
     and `breach_time_s` is `null`; for any other, `metric`, `threshold`, and `value` are `null` and
     `breach_time_s` is non-null (§4.4 keeps it non-negative). Reported at the first offending member, in table
     order. A tripwire counts fills, never breach time (mandate spec §6.7).
233. `RiskLimitTriggered`: `value` is at least `threshold`, since a tripwire fires when its metric
     reaches it; for `consecutive_losing_exits` and `new_instruments` both are whole numbers and
     `threshold` is from 1 to 1,000, and for `realized_loss_usd` `threshold` is whole cents greater
     than 0 (`payload.threshold`, then `payload.value`; mandate spec V-044). The value reached is not
     rounded to cents: it is the fold's sum of net realized, which keeps §8.1's precision.
234. `RiskLimitLifted`: `action` is non-null only as `scale_sizes`, on a `drawdown_ladder[<i>]`
     limit with `reason` `null` (a rung stops scaling); `version_loosened` is `lifetime_floor`'s
     only; a `tripwire:` limit lifts only with `owner_acknowledged` (`payload.action`, then
     `payload.reason`; mandate spec §5.6 to §5.8, §6.7).
235. `RiskLimitLifted` with `reason` `owner_acknowledged`, and `HighWaterMarkReset`: `causation_id`
     is non-null (`causation_id`). Only the owner's acknowledgment lifts a latch or resets H (MI-3).
236. `PositionReleased`: `qty` is greater than 0 when `cause` is `owner_release`, and not negative when
     it is `goal_release` (`payload.qty`); `warning_ref`, `user_id`, and
     `step_up` are non-null when `cause` is `owner_release` and `null` when it is `goal_release`,
     reported at the first offending member in table order. An owner's release is a step-up
     gesture after a shown warning; a goal's release follows from a confirmed version.
237. `InstrumentRestrictionChanged`: `stale_mark` changes only with `no_sane_mark` (set) or
     `sane_mark` (cleared); `removed_instrument` is set with one of §9.3 rule 31's removing reasons
     or `version_applied`, and cleared with `thesis_admitted` or `version_applied`
     (`payload.reason`, then `payload.active`).
238. `GoalCompleted`: exactly one of `then` and `on_complete` is non-null (`payload.then`), and
     `reason` is `null` unless `then` is (`payload.reason`). The goal-complete input names the
     `on_complete` it applies; a confirmed `profit_stop`, or a goal with no `on_complete`, names what
     follows instead (mandate spec §3.1).

**No `JournaledFact`.** None of the seven feeds `ValidationContext`; they are the risk state's own
record, read back only by the risk-state fold. **Vectors:** the test vectors' `risk_limits` section
holds a base draft of each record shaped as the mandate reference cases' risk-state events, an
invalid draft for every type and rule above, and valid drafts for the shapes the risk-state fold emits
that a rule might be misread to refuse. The vectors stay version 3 (the section is additive, as in
DEC-177 item 6).

### 9.28 The owner's acknowledgment and its request ([DEC-2509](../project/decisions/DEC-2509.md))

This subsection closes `OwnerAcknowledged` and adds `OwnerAcknowledgmentRequested`, on the
workspace control stream at schema version 1, with their account-stream copies. Until now §9
catalogued `OwnerAcknowledged` and registered no payload, so every append was refused
`unknown_schema` (DEC-257 item 17). It replaces the "Key payload fields" column of §9 for both.
§9.2's conventions apply: §9.1's and §9.2's types, the absent-member rule, the report order,
`schema_version` 1, and `risk_clock` as §2 requires of an owner command. The rules number from 239.

An acknowledgment lifts one latched limit, rung, or tripwire that a `RiskLimitTriggered` on the
agent's account stream recorded (mandate spec §5.8, §6.7). It takes effect only when the executor
processes its copy, and only under the stricter of the independence requirement it carries and the
overlay's at processing (DEC-351 items 4 to 6). **An `OwnerAcknowledged` is always an
acknowledgment that takes effect when it passes; it is never a request.** Where independence is
required, one user records the request as `OwnerAcknowledgmentRequested`, and a different user
acknowledges it, naming that request. So no fold decides from policy whether an acknowledgment was
"only" a request, and a policy misread on replay cannot turn a request into a lift.

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `step_up` | `{assertion_id: text, authenticated_at: integer, method: text}`, as §9.7's `ApprovalResponseSubmitted` writes it: the control stream's owner-command evidence, `authenticated_at` in whole seconds | As §9.7's |

**`OwnerAcknowledgmentRequested`**: an owner asked that one triggered limit be lifted, where an
independent user must acknowledge it.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent whose limit is triggered |
| `event` | `ulid` | The `event_id` of the account stream's `RiskLimitTriggered` the lift is asked for |
| `requested_by` | `id` | The requesting user, opaque |
| `independent_required` | `boolean` | The independence requirement as it stood when the lift was requested: `true` on every request (rule 239) |
| `submitted_at` | `integer` | The second the owner ran the command |
| `risk_clock` | `risk_clock` | The owner's tick (§2) |
| `step_up` | `step_up` | The requester's step-up evidence: rule 239 |

**`OwnerAcknowledged`**: an owner acknowledged one triggered limit.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent whose limit is triggered |
| `event` | `ulid` | The `event_id` of the account stream's `RiskLimitTriggered` acknowledged |
| `user` | `id` | The acknowledging user, opaque |
| `requested_by` | `id` | The requesting user, opaque: the acknowledging user when no independent user is required (rule 239) |
| `independent_required` | `boolean` | The independence requirement as it stood when the lift was requested (mandate spec §6.7) |
| `request` | `ulid?` | The `event_id` of the `OwnerAcknowledgmentRequested` it answers, or `null`: rule 239 |
| `submitted_at` | `integer` | The second the owner ran the command |
| `risk_clock` | `risk_clock` | The owner's tick (§2) |
| `step_up` | `step_up?` | The acknowledging user's step-up evidence, or `null`. The authentication method the §9 catalogue named is `step_up.method`; there is no second member |

**Consistency rules** (reason `schema`; the path is the member named):

239. `OwnerAcknowledgmentRequested`: `independent_required` is `true` (`payload.independent_required`):
     a request exists only where an independent user must acknowledge. Its `step_up` is non-null,
     since the request is half of a lift (`payload.step_up`). `OwnerAcknowledged`: when
     `independent_required` is `true`, `request` is non-null and `requested_by` differs from `user`
     (`payload.request`, then `payload.requested_by`); when it is `false`, `request` is `null` and
     `requested_by` equals `user` (`payload.request`, then `payload.requested_by`). A user never
     acknowledges their own request (mandate spec §6.7).

**Who writes them.** The writer (the CLI, and the workspace API once its route mounts) sets
`user` on an acknowledgment, and `requested_by` on a request, from the principal it authenticated
for the command, the same principal its step-up evidence names: the CLI's owner as D1b's flags give
it, and the API's session and step-up subject. Never from anything the caller supplies. An
acknowledgment's `requested_by` is copied from the request it names. A request carries step-up
evidence, so `step_up` is required on it (rule 239), and it is judged as §6.1 judges an owner
command's. That includes reuse: a request's assertion is spent like any other owner command's.
The assertion of a request, an acknowledgment, an `ApprovalResponseSubmitted`, or an
`OwnerCommandIssued` may appear on no later one of those four, whatever its outcome (mandate spec
§6.1). So a request's assertion is never replayed on a second request, a resume, a Stop, or an
acknowledgment, and a request reusing an earlier command's assertion is refused.

**The residual: the users are the writer's word.** A `step_up` names its assertion,
`authenticated_at`, and method, not its subject. The executor cannot check that `user` or
`requested_by` is the user the step-up authenticated, so it trusts the writer for that binding,
as it does for every owner command's `user`. A writer that set them from the caller's input could
let one user both request and acknowledge. Until the step-up evidence names its subject (E9-4's
signed assertions), the writer binding above is the control.

**Stream rules** on the control stream, judged before anything takes effect, and failing closed:

240. Control stream, `OwnerAcknowledged` with a non-null `request`: an earlier
     `OwnerAcknowledgmentRequested` on the same stream has that `event_id`, and its `agent`,
     `event`, `requested_by`, and `independent_required` equal the acknowledgment's
     (`payload.request`). A reordered stream, in which the acknowledgment comes before its request,
     breaks this rule.
241. Control stream: no two `OwnerAcknowledged` name the same non-null `request`
     (`payload.request`). A request is spent by the first acknowledgment that names it, **whether
     that acknowledgment then lifts the limit or is refused**: a retry needs a new request, and a
     second acknowledgment by a third user lifts nothing more.

The writer checks rules 239 to 241 against its own fold of the control stream before appending,
and appends nothing that would fail them. `append` sees one record and checks rule 239 only.

**The executor judges at processing, from its account stream.** It copies both events into the
agent's account stream, the request as well as the acknowledgment. Each copy is the original's
payload unchanged, with `causation_id` naming the control-stream original, and is copied at most
once. A copy has its own `event_id`, so on the account stream an acknowledgment copy's `request`
(the control-stream request's `event_id`) is matched to the request copy whose `causation_id`
equals it. The executor's fold holds every input it judges from but one, so a replay of the
account stream decides the same way. The exception is the reuse check of §6.1: an assertion is
spent by any earlier `ApprovalResponseSubmitted`, `OwnerCommandIssued`,
`OwnerAcknowledgmentRequested`, or `OwnerAcknowledged` on the control stream, before the copy's
original. So the executor judges reuse from the control stream it copies from, up to that
original's position, which is also fixed on replay. An acknowledgment that lifts its limit is
followed by the executor's `RiskLimitLifted` (§9.27, reason `owner_acknowledged`), so it
supersedes its own trigger for every later acknowledgment (check 2).

It judges an acknowledgment copy in this order, and at the first check that fails it records
`OwnerCommandRefused` with `command` `acknowledge`, the reason shown, and `causation_id` naming the
control-stream `OwnerAcknowledged`, and lifts nothing. The copy and its judgment are one batch, and
a refusal is the record right after the copy, with nothing between them (v0.102), so a reader that
folds the stream row by row decides a copy with one row of lookahead: refused when the next row is
its refusal, accepted otherwise. A request copy is never judged by itself and never followed by a
refusal of its own:

1. **The trigger exists:** `event` is the `event_id` of a `RiskLimitTriggered` of this `agent` on
   this account stream, before the acknowledgment copy. An unknown ULID, another agent's trigger,
   or another event type is refused `trigger_unknown`.
2. **The trigger stands:** no later `RiskLimitTriggered` of the same `limit`, and no
   `RiskLimitLifted` of that `limit`, comes between that trigger and the acknowledgment copy. "The
   same `limit`" is the same `limit` of the same agent: each agent's risk state latches and lifts
   its own limits (§9.27's `agent_id`), so another agent's trigger supersedes nothing.
   Otherwise `trigger_superseded`: a request and an acknowledgment are void once their trigger is
   superseded, so neither lifts a later latch of the same limit, which needs a new request.
3. **Its step-up holds:** §6.1's checks, as for every acknowledgment: the `step_up_*` reasons.
   They are judged when the executor processes the copy, as mandate spec §6.1's table says (the
   copy's `event_time`), not at the acknowledgment's `submitted_at`. Evidence fresh when the owner
   ran the command but more than 300 s old when the executor reaches it is stale. Its method is
   one the environment allows: `passkey` in every environment, as identity spec §6.1 and §7.3
   allow it, and `cli_confirm` in `paper`, and in `live` only on the founder's own connection
   (mandate spec §6.1, DEC-529 item 3, DEC-923) (v0.106,
   [DEC-2523](../project/decisions/DEC-2523.md)). A request's step-up is judged at its own
   `submitted_at` (check 4), since the request takes no effect of its own.
4. **Independence**, when the stricter of the copy's `independent_required` and the overlay's
   `independent_approval_required` at processing is `true`:
   - the copy names a `request`, and its `user` differs from its `requested_by`. Otherwise
     `not_independent`;
   - a request copy whose `causation_id` equals `request` comes before the acknowledgment copy.
     Its `agent`, `event`, `requested_by`, and `independent_required` equal the acknowledgment
     copy's, and its step-up passes §6.1's checks, judged at its own `submitted_at`. No earlier
     acknowledgment copy, refused or not, names the same `request`. Otherwise `request_invalid`:
     rules 240 and 241, restated on the account stream by `causation_id`.

Only an acknowledgment that passes all four lifts its limit. A request copy is never judged by
itself and never lifts anything. None of these refusals holds an exit, a protective order, or a
kill switch: an acknowledgment only ever lifts a limit, which adds risk, so refusing one never
delays risk reduction (`AGENTS.md` rules 3 and 13).

**Version 2: an acknowledgment of a restriction** (v0.102,
[DEC-2824](../project/decisions/DEC-2824.md)). `OwnerAcknowledgmentRequested` and
`OwnerAcknowledged` are also registered at `schema_version` 2, which adds one member to version 1's:

| Member | Type | Meaning |
|---|---|---|
| `subject` | `limit` \| `restriction` \| `reconciliation` | What `event` names: a `RiskLimitTriggered` (`limit`), an `AccountRestrictionChanged` (`restriction`, trading spec §7.3), or, from v0.104, the `AgentModeApplied` that applied a reconciliation hold (`reconciliation`, trading spec §7.1 and §11), on the account stream of `agent`'s account |

Version 1 stays registered and reads as `subject` `limit`; with `limit`, version 2 means exactly what
version 1 does. With `restriction`, `agent` is any agent of the restricted account, and it routes the
copy as version 1's does; the restriction is the account's, not the agent's. Rules 239 to 241 apply
to both versions, and rule 240's equal members include `subject` when both records carry it, so a
request for a limit is never answered by an acknowledgment of a restriction. The writer offers a
restriction for acknowledgment only while its cause is in force.

The executor judges a restriction's acknowledgment copy by the same four checks, in the same order,
with the same refusals, reading checks 1 and 2 for a restriction:

1. **The trigger exists:** `event` is the `event_id` of an `AccountRestrictionChanged` on this
   account stream, before the copy. Otherwise `trigger_unknown`.
2. **The trigger stands:** no later `AccountRestrictionChanged` of the same `cause`, and no
   `AccountRestrictionLifted` of it, comes between that record and the copy. Otherwise
   `trigger_superseded`: a restriction raised again needs a new acknowledgment.

Check 3 (step-up) is a limit's. Check 4 (independence) applies the strictest of the copy's
`independent_required` and the overlay's `independent_approval_required` at processing of **every**
agent deployed on the account, stopped or not, not only of `agent`: the restriction is the account's, so an owner cannot
choose the agent whose overlay asks least. An acknowledgment of a restriction that passes all
four is **accepted** and lifts nothing by itself: §9.5's `AccountRestrictionLifted` lifts its cause
once the evidence §7.3 names is on the stream (rules 310 to 312). A request copy is never judged by
itself, and an acknowledgment that is refused never lifts.

**An acknowledgment of a reconciliation hold** (v0.104,
[DEC-2830](../project/decisions/DEC-2830.md)). Trading spec §11 resumes a paused agent only on the
owner's acknowledgment with step-up, and §7.1 holds external activity until the owner acknowledges
it. The executor holds openings under a restriction `reconciliation:<subject>`, by an
`AgentModeApplied` that is not `normal`: an instrument's position mismatch, the cash or fee
difference, external activity (`external_activity`), and a run or batch that could not be recorded
whole (`incomplete`, DEC-2805, DEC-2821). With `subject` `reconciliation`, `event` names that
`AgentModeApplied`, and `agent` is any agent of the account, as for a restriction. The executor
judges the copy by the same four checks, in the same order, with the same refusals, reading checks 1
and 2 for the hold:

1. **The hold exists:** `event` is the `event_id` of an `AgentModeApplied` on this account stream,
   before the copy, that the executor originated (`originated` `true`), whose `to` is `exits_only`
   or `paused`, and whose `restriction` begins `reconciliation:`. Otherwise `trigger_unknown`: a
   copied mode, a Stop, and a removal are never holds this lifts.
2. **The hold stands:** no later `AgentModeApplied` of the same `agent` and `restriction` comes
   between that record and the copy: neither the hold applied again nor its removal. Otherwise
   `trigger_superseded`, and a hold applied again needs a new acknowledgment.

Checks 3 and 4 are a restriction's: the hold may name every agent (`*`), so independence takes the
strictest overlay of every agent deployed on the account. An acknowledgment of a hold that passes
all four lifts that hold and no other: the executor journals, as the very next record after the copy
in its batch, `AgentModeApplied` for the hold's `agent` to `normal` under the hold's `restriction`,
`originated` `true`, with the copy as its `causation_id`. Removing the entry hands each agent back
to the strictest restriction it still holds, so a kill switch, an owner's pause, a limit, an account
restriction, or another reconciliation hold stays. A refused acknowledgment of a hold is followed by
its `OwnerCommandRefused` instead, and lifts nothing. Lifting a hold only adds risk, so nothing here
holds an exit, a protective order, or a kill switch (`AGENTS.md` rules 3 and 13).

Each hold is its own `(agent, restriction)`, so one acknowledgment lifts one: an instrument held for
every agent (`*`) and for two agents of their own takes three. Lifting an instrument's hold,
`reconciliation:<instrument>`, also ends that instrument's recorded mismatch: the executor's fold
reads the lift as the owner's acknowledgment of the mismatch (trading spec §11), and the next run
compares the position afresh. An acknowledgment resumes the agents; it does not clear the cause. A
reconciliation that finds the cause again (a cash difference still past its runs, a position still
mismatched) applies a new hold with a new alert, which needs its own acknowledgment.

The writer offers every standing hold for acknowledgment, and only while it stands. The CLI and the workspace API
write it as they write a limit's acknowledgment, from the authenticated principal and step-up
(§9.28's writer binding).

**No `JournaledFact`.** Neither event feeds `ValidationContext`. **Vectors** come with the
registration change: `reference/journal` gains an `acknowledgments` section with base drafts of
both events, an invalid draft for every type and for rule 239, control streams for rules 240 and
241 (a reordered stream, a self-acknowledgment, a second acknowledgment of one request, and a
request whose `agent` or `event` differs). The executor's judgment gets account streams of its own:
an unknown trigger, a later trigger and a lift that supersede one, a request copy after its
acknowledgment copy, a request spent by a refused acknowledgment, and a request with failing
step-up. It also gets a fuzz over random orderings of requests, acknowledgments, triggers and lifts,
judged by an oracle written apart from the executor, and a seeded mutant for each rule and each
check. The vectors stay version 3.

### 9.29 The CLI's step-up confirmation ([DEC-2104](../project/decisions/DEC-2104.md))

This subsection adds `StepUpConfirmed` on the workspace control stream at schema version 1.
§9.2's conventions apply: §9.1's and §9.2's types, the absent-member rule, the report order, and
`schema_version` 1. The rules number from 242.

A paper grant made in the web app may carry `cli_confirm` step-up (workspace API spec §3.6,
DEC-2103 item 7). The owner confirms the challenge the API issued by running `mandate step-up
confirm` on the host, which reads the approval from this stream and appends one `StepUpConfirmed`.
From v0.82 a paper OAuth connect may carry it too, until passkey step-up confirms a connect
([DEC-2125](../project/decisions/DEC-2125.md)): the owner confirms the connect's challenge with the
CLI, which computes the connect's action object itself, from the workspace and the current
disclosure document and version, never from the browser, and appends one `StepUpConfirmed` of kind
`connection`.
**A confirmation is not step-up evidence and grants nothing.** Only the workspace API reads it,
when it verifies the grant's `cli_confirm` evidence (DEC-2104 item 6); the runtime and the
executor never read it, and the grant's own evidence is judged as mandate §6.1 says.

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `confirm_code` | 8 characters of Crockford base 32, `0-9A-HJKMNP-TV-Z`, upper case, no hyphen: the first 40 bits of the challenge's `webauthn_challenge` (DEC-2104 item 1) | Another JSON kind `schema`; another string `non_canonical` |

**`StepUpConfirmed`**: an owner confirmed one step-up challenge on the host.

| Member | Type | Meaning |
|---|---|---|
| `challenge` | `ulid` | The challenge's `challenge_id`, as the workspace API issued it |
| `action_kind` | `text` | The action the challenge is for: `approve` or `connection` (rule 242) |
| `action_digest` | `ref` | The action's digest: for `approve`, the approval's content hash, read by the CLI from this stream; for `connection`, the digest of the connect's action object, which the CLI computes and stores, the one the start route binds (workspace API spec §3.6, DEC-2623) |
| `principal` | `ulid` | The confirming principal, as the API writes its `actor.id` |
| `code` | `confirm_code` | The code the owner typed, normalized |
| `confirmed_at` | `integer` | The second the owner ran the command: `event_time`'s whole second (rule 242) |

**Consistency rules** (reason `schema`; the path is the member named):

242. `StepUpConfirmed`: `action_kind` is `approve` or `connection` (`payload.action_kind`); the
     envelope's `environment` is `paper` (`environment`): a `live` confirmation is never recorded;
     and the actor is a `user` whose `id` is `principal` (`actor.kind`, then `actor.id`), so no
     writer confirms for another principal; and `confirmed_at` is `event_time` in whole seconds,
     its fraction dropped (`payload.confirmed_at`), so the second the API judges the challenge's
     window by is the record's own time, never one the writer chose apart from it.

**Stream rule** on the control stream, judged before anything takes effect, and failing closed:

243. Control stream: no two `StepUpConfirmed` name the same `challenge` (`payload.challenge`). A
     challenge is confirmed at most once, whether or not a grant then uses it.

The CLI checks rules 242 and 243 against its own fold of the control stream before appending, and
appends nothing that would fail them. `append` sees one record and checks rule 242 only, so rule
243 is the writer's own: two writers racing could each append one. The API's verifier does not
trust it: it counts the confirmations naming a challenge and accepts exactly one, so a race that
leaves two refuses the grant (`step_up_missing`, DEC-2104 item 6 check 6) and never passes one.

**What binds a confirmation.** A confirmation names no workspace: it is bound to its workspace by
the stream it is on, the workspace's own control stream, which the API reads for that workspace
alone, and to its action by `action_digest` and `action_kind`, which must be the challenge's.

**What the API makes of it.** DEC-2104 item 6 is the order: the confirmation must name a challenge
the API holds, match its kind, digest, and principal, fall within its 300 s, and carry its code.
A confirmation for a challenge the API no longer holds, or never issued, verifies nothing.

**A connect started under `cli_confirm`.** Its `ConnectionRequested` version 2 (§9.8) carries
`step_up.method` `cli_confirm`, and no `StepUpAsserted` follows it: §9.32's record is the assertion
behind a passkey step-up, and the API appends one only with an event committed under passkey. The
connect's confirmation is the `StepUpConfirmed` of kind `connection` that names its challenge, so an
auditor reading §9.32 finds that start complete, not short of an assertion.

**No `JournaledFact`.** It feeds no `ValidationContext`. **Vectors**: `reference/journal`'s
`step_up_confirmations` section holds a base draft, an invalid draft for every member type and
for each clause of rule 242, valid drafts for the members no rule reads, its `connection_drafts`
(v0.82: a connect's confirmation, valid), and control streams for rule 243: a second confirmation
of one challenge, and two confirmations of two. The vectors stay version 3.

### 9.30 The risk mark's verdict ([DEC-2305](../project/decisions/DEC-2305.md))

This subsection closes the account stream's `MarkUpdated` at `schema_version` 2. Version 1 (§9's
row: `instrument_id`, `price`, `source`, `feed`, and `risk_clock`) is never edited (§8) and stays
registered, so `append` and verification accept both. §9.2's conventions apply: §9.1's and §9.2's
types, the absent-member rule, the report order, and no required `config_refs`. The rule numbers
from 244.

[Mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism) folds a `MarkUpdated` as a
risk input and reads three things from it that version 1 does not carry: the risk mark, which
[trading spec §8.2](trading-domain.md#82-marks-and-equity) makes the bid for a long (no short sales in
v1, `AGENTS.md` rule 12); whether the quote passed §8.2's checks, since only a sane mark updates
E and restarts the staleness count, and a mark that fails them restricts its instrument
(`stale_mark`); and the session the mark arrived in, since for an equity only a regular-session
mark updates E. Version 1's `price` is a price under a `source`, with no verdict, and the session
could be derived only from a trading calendar the record does not name. Version 2 records all
three, so the risk fold replays from the journal alone and never re-judges a quote the journal does
not hold.

**`MarkUpdated`** on the account stream at `schema_version` 2: the version-1 members, then `bid`,
`sane`, and `session`, with `risk_clock` last.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | As version 1 |
| `price` | `decimal` | As version 1: the mark's price under `source`. The risk fold does not read it |
| `source` | `text` | As version 1 |
| `feed` | `text` | As version 1 |
| `bid` | `decimal?` | The quote's bid: a long's risk mark (trading spec §8.2). `null` only for a quote with no bid, which is never sane (rule 244) |
| `sane` | `boolean` | Whether the quote met every condition trading spec §8.2 sets on a risk mark from a quote: 0 < bid ≤ ask, the spread within the data profile's limit, and the quote fresher than the profile's staleness threshold at `risk_clock` (freshness counts, DEC-2305 item 5) |
| `session` | `overnight` \| `pre_market` \| `regular` \| `after_hours` \| `crypto` | The market session the writer placed the mark in at `risk_clock`, by its trading calendar; `crypto` for a crypto instrument |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rule** (reason `schema`; the path is the member named):

244. `MarkUpdated` at `schema_version` 2: when `sane` is true, `bid` is non-null and greater than 0
     (`payload.bid`). Trading spec §8.2's checks require 0 < bid, so a sane verdict without a
     positive bid contradicts itself. A mark that is not sane may carry any bid, or none: the fold
     reads `bid` only from a sane mark.

No rule ties `bid` to `price`, `sane` to `session`, or `session` to the instrument: the journal
cannot see the quote's ask, the data profile, or the instrument's asset class, so those are the
writer's. A sane mark outside the regular session is well formed; mandate spec §5.2 decides that an
equity's does not update E.

**How the risk fold reads a mark** (E6-15). A version-2 mark is a risk input with its verdict:
mandate spec §5.2 ignores an equity's mark outside the regular session, sane or not; otherwise it
applies `bid` from a sane mark, and restricts a held instrument whose mark is not sane. A version-1
mark has **no verdict**. It is not a risk input: the fold applies nothing from it, as at a tick, so
whether the fold settles time at it changes no result (MI-13). It is never read as sane, which
would update E from a price no check judged and restart the staleness count, loosening §5.2. So E
is not updated from it, and the instrument's staleness keeps counting from its last sane
version-2 mark: once its age reaches `mark_max_age_s`, `stale_mark` holds openings and increases
in that instrument, and never its exits, protective orders, or kill switches (`AGENTS.md` rule 13).
A sane version-2 mark restarts the count only in a session §5.2 counts, so an equity's sane
after-hours mark does not, and a mark that is not sane restricts at once and restarts nothing.
A version-1 mark is not read as unreadable either (DEC-2301 item 3): a version-1 mark is a
well-formed record the stream holds legitimately, and holding every opening on any stream that
holds one would add nothing that staleness, per instrument, does not already hold.

**The calendar.** An equity's age counts the regular-session seconds of the trading calendar
([trading spec §2.2](trading-domain.md#22-time-and-calendars), §4.3), which the risk fold's
`SessionClock` is given by its caller (`mandate_spec::risk`): `mandate-time`'s committed NYSE data,
`crates/mandate-time/data/us-equities.calendar`, with its weekends, holidays, and early closes, so
a weekend, a `closed` day, and the hours after an `early_close` count nothing. Crypto's counts all
time.

**Who writes it.** Today the shell's paper path writes `MarkUpdated` at version 1, as the executor,
with the IEX quote's bid as `price` (DEC-943 item 1); the executor writes none and its own fold
applies nothing from one. Version 2 is written by the executor's mark path from the E6-15 slice
that builds it, once `mandate-journal` registers it; until then `append` refuses it as
`unknown_schema`, and version 1 continues. **A writer never records a false session:** a mark its
trading calendar places in no session, such as an equity's quote while the market is closed, is
never mapped to the nearest session. The writer writes no `MarkUpdated` for it, and the
instrument's staleness keeps counting (DEC-2305 item 6). Version 2's `bid` is a quote's bid, so no
writer records a last-trade risk mark (trading spec §8.2) until a later version carries one.

**No `JournaledFact`.** It feeds no `ValidationContext`. **Vectors**: `reference/journal`'s
`risk_marks` section holds a version-2 base draft and a version-1 base draft, both valid; an
invalid draft for every member type, each missing or extra member, each clause of rule 244, a
version-1 draft carrying a version-2 member, and version 3; valid drafts for the marks rule 244
might be misread to refuse (not sane with no bid, a zero bid, or a bid above the price) and for every
session, with `session: "closed"` refused at `payload.session`; the fold's reading of each base
and valid draft: `no_verdict` for version 1, and `sane`, `session`, and, when sane, `bid` for
version 2; and staleness sequences in which a held instrument with only version-1 marks since its
last sane mark, or since it opened, reaches `stale_mark` exactly at `mark_max_age_s`, with its
openings and increases denied and each exit path, protective order, and kill switch not held: an
equity within one session, across a close, from Friday to Monday, across Thanksgiving 2026, and
across the early close the day after; an equity's sane after-hours mark and its regular-session
mark that is not sane; and crypto over a weekend. The reference counts an equity's seconds by
reading the same `us-equities.calendar` file `mandate-time` embeds, so the two cannot drift. The
vectors stay version 3.

### 9.31 Passkey credentials ([DEC-2108](../project/decisions/DEC-2108.md))

This subsection closes `CredentialEnrolled` and `CredentialRemoved` on the workspace control
stream at schema version 1, and `CredentialEnrolled` also at version 2 (v0.71). §9.2's conventions apply: §9.1's and §9.2's types, the absent-member
rule, the report order, and `schema_version` 1. The rules number from 249.

Workspace services are the WebAuthn relying party for step-up (identity spec §6.1). A member's
passkeys are the fold of these two records over the stream. The API's passkey verifier judges an
assertion against that fold with `mandate_passkey::stepup::consume` (identity spec §7.2 step 4,
DEC-2108 item 8). The runtime and the executor never read them.

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `credential_id` | A WebAuthn credential id as unpadded base64url (`A-Z a-z 0-9 - _`), 22 to 1364 characters: 16 to 1023 bytes | Another JSON kind `schema`; another string `non_canonical` |
| `passkey_alg` | `es256` or `eddsa` (DEC-660) | Another JSON kind `schema`; another string `non_canonical` |
| `sign_count` | An integer from 0 to 4294967295, the authenticator's 32-bit counter | As `integer` |

**`CredentialEnrolled`**: a member enrolled a passkey with the workspace's relying party.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | The member's principal |
| `kind` | `text` | `passkey`, the one kind (identity spec §12.1) |
| `credential` | `credential_id` | The credential id the authenticator returned |
| `public_key` | `ref` | The credential's COSE public key, its bytes as an artifact: never inline |
| `alg` | `passkey_alg` | The key's algorithm |
| `sign_count` | `sign_count` | The counter the registration's authenticator data carried |
| `cool_off_ends` | `timestamp?` | When the credential may first step up (identity spec §10.1), or `null` for none |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | The passkey step-up that authorized it, or `null` for a member's first (DEC-2108 item 5) |

**`CredentialEnrolled` version 2** (v0.71, [DEC-2122](../project/decisions/DEC-2122.md)): version 1's
members, with `backup_eligible` after `sign_count`.

| Member | Type | Meaning |
|---|---|---|
| `backup_eligible` | `bool` | The registration's backup-eligible flag (WebAuthn's BE bit), which no later assertion of the credential may change |

`mandate_passkey::stepup::consume` holds every assertion to the flag recorded at enrolment, so the
flag is journaled rather than read back from the authenticator. Version 1 stays registered and
unchanged, and rule 249 reads both versions alike. The API writes version 2 only. A version 1
credential carries no flag, so the verifier never counts it for step-up. No deployment holds a
version 1 `CredentialEnrolled`. Every reader of the credentials (the verifier's fold and route 2's
lookup, DEC-2121) reads version 2 before any writer appends one, so a version 2 row never makes
the fold unbuildable ([DEC-2122](../project/decisions/DEC-2122.md) item 5). The `credential_enrolled_v2` section holds version 2's vectors; the
`credentials` section stays version 1's.

**`CredentialRemoved`**: a member removed one of their passkeys.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | The member's principal |
| `credential` | `credential_id` | The credential removed |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | The passkey step-up that authorized it: never `null` |

**Consistency rules** (reason `schema`; the path is the member named):

249. `CredentialEnrolled`:
     - `kind` is `passkey` (`payload.kind`);
     - the actor is a `user` whose `id` is `member` (`actor.kind`, then `actor.id`), so no one
       enrols for another;
     - `step_up`'s `method`, when present, is `passkey` (`payload.step_up.method`);
     - `cool_off_ends` is non-null exactly when `step_up` is null, and then is `event_time` plus
       86 400 s (`payload.cool_off_ends`). So a first enrolment always waits its cool-off, and one
       stepped up never does.
250. `CredentialRemoved`: the actor is a `user` whose `id` is `member` (`actor.kind`, then
     `actor.id`), and `step_up`'s `method` is `passkey` (`payload.step_up.method`).

**Stream rule** on the control stream, the writer's, and failing closed in the verifier's fold:

251. No two `CredentialEnrolled` name the same `credential`, and a `CredentialRemoved` names a
     credential the same member enrolled and has not removed (`payload.credential`). A removed
     credential is never enrolled again.

`append` sees one record and checks rules 249 and 250. Rule 251 needs the stream: the API checks it
against its own fold before it appends, and the verifier does not trust it. A member whose records
break it has no credential that counts. Every passkey step-up of theirs is then
`step_up_missing` until the stream is repaired, and never passes.

**Enrolment gating is the API's, not the journal's.** DEC-2108 item 5 says which enrolment needs
step-up: a first one only from a full session opened within 300 s, a later one only with passkey
step-up from a credential past its cool-off, and none while every credential is still cooling off.
The API enforces that before it appends; the journal does not. Rules 249 to 251 pin only the
cool-off's arithmetic and the actor binding, so a second enrolment with no step-up is admitted
here (the vectors' `second_unstepped_enrolment`). That a session alone never yields a working
credential early rests on the verifier's fold honouring `cool_off_ends`: a credential whose
cool-off has not ended never steps up. Removing a member's last credential returns them to a first
enrolment, which waits its cool-off again.

**Personal data.** Neither record carries `pii_refs`. The credential id is the authenticator's
random identifier and the public key is an artifact. Both are handled as identity records under
§6.4, as `actor.id` is, and kept for the retention period (DEC-2108 item 11).
`PersonalDataErased` does not touch them, and an append-only record is never rewritten.

**What binds a credential.** A credential names no workspace. The stream it is on binds it to its
workspace, and `member` binds it to its member, whom the verifier checks against the session's
principal. Each workspace enrols its own: a member of two workspaces enrols a passkey in each.

**No `JournaledFact`.** Neither feeds a `ValidationContext`. **Vectors**: `reference/journal`'s
`credentials` section holds a base draft of each record, an invalid draft for every member type and
for each clause of rules 249 and 250, valid drafts for the members no rule reads, and control
streams for rule 251. The vectors stay version 3.

### 9.32 Passkey assertions ([DEC-2108](../project/decisions/DEC-2108.md) item 3)

This subsection adds `StepUpAsserted` on the workspace control stream at schema version 1. §9.2's
and §9.31's conventions and types apply. The rules number from 252.

A passkey step-up's evidence keeps §9.2's three members, so no event that carries step-up changes.
`MandateConfirmed` and `AgentDeployed` carried theirs only in the record `record_ref` names, which
rule 253 cannot read; v0.84 adds version 3 of each, carrying `step_up` as a member (§9.9, §9.24,
[DEC-2131](../project/decisions/DEC-2131.md)).
The assertion behind it, with the signature counter it carried and the raw assertion an auditor
re-verifies (identity spec §7.2 step 5), is this record. It is appended in the same batch as the
event it authorized, immediately after it. The API's passkey verifier reads it for each
credential's last counter, and nothing else reads it.

**`StepUpAsserted`**: the assertion behind one passkey step-up.

| Member | Type | Meaning |
|---|---|---|
| `challenge` | `ulid` | The challenge, the authorized event's `step_up.assertion_id` |
| `credential` | `credential_id` | The credential that signed (§9.31) |
| `sign_count` | `sign_count` | The counter the assertion's authenticator data carried |
| `assertion` | `ref` | The raw assertion as an artifact: the canonical object of the credential id, authenticator data, client data and signature the request carried, each unpadded base64url (DEC-2108 item 7) |

Its `causation_id` is the event it authorized.

**Consistency rule** (reason `schema`; the path is the member named):

252. `StepUpAsserted`: `causation_id` is non-null (`causation_id`), and the actor is a `user`
     (`actor.kind`).

**Batch rule**, judged by `append` over the batch, after each draft's own rules:

253. A `StepUpAsserted` is immediately preceded in its `append` batch by the event its
     `causation_id` names (`event_type`). That event:
     - was written by the same actor (`actor.id`);
     - carries `payload.step_up` with `method` `passkey` (`payload.step_up`);
     - names this record's `challenge` as `assertion_id` (`payload.challenge`).

     The writer appends the two together, so no assertion is committed apart from the event it
     authorized.

**Stream rule** on the control stream, the writer's, and failing closed in the verifier's fold:

254. A `StepUpAsserted` names a credential its actor enrolled and has not removed
     (`payload.credential`). Its `sign_count` is greater than that credential's last counter, unless
     both are 0 (DEC-660: a counter the authenticator does not keep) (`payload.sign_count`). The last
     counter is the greatest of the credential's earlier `StepUpAsserted` and its
     `CredentialEnrolled`'s. No two `StepUpAsserted` on the stream name the same `challenge`
     (`payload.challenge`), so one assertion authorizes one event.

The API appends a `StepUpAsserted` with every event it commits under passkey step-up. That converse
is the writer's own, not an `append` rule: §9.12's membership records and others carried
`passkey` evidence before this record existed. Rule 254 needs the stream: the API checks it against
its own fold before it appends, and the verifier does not trust it. A credential whose assertions
break it no longer counts, so every passkey step-up with it is then `step_up_missing`.

**No `JournaledFact`.** **Vectors**: `reference/journal`'s `step_up_assertions` section holds a base
draft, an invalid draft for every member type and each clause of rule 252, batches for each clause
of rule 253, and control streams for rule 254. The vectors stay version 3.

### 9.33 Connection roles ([DEC-2350](../project/decisions/DEC-2350.md))

This subsection adds three records on the workspace control stream at schema version 1. §9.2's
conventions and types apply. The rules number from 261.

A connection's token exchange and executor log in with OpenBao's Kubernetes auth, each with a role
made for that connection alone and bound to its ServiceAccount (DEC-2060, infrastructure §5.2). The
**role provisioner**, a workspace process holding only the power to make and remove those roles
(DEC-2630 item 7), makes both when it reads a version-2 `ConnectionRequested` with `credential`
`oauth`, and journals the outcome; the deployment manager starts the exchange only once the roles
are made (§9.21). When the request closes without an establishment, and when the connection is
revoked, the provisioner removes the token exchange's; it never removes the executor's.

**`ConnectionRolesMade`**: both roles of one connect's connection exist.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The request's connection |

**`ConnectionRolesRefused`**: the roles could not be made, so nothing starts for the request.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The request's connection |
| `reason` | `vault_unavailable` \| `role_refused` | The vault could not be reached in time, or it refused a role |

**`ConnectionRolesRemoved`**: the token exchange's role of a connection was removed.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection |
| `role` | `token_exchange` | The role removed: only the exchange's, since the executor's is never removed (DEC-2350 item 3) |
| `occasion` | `request_closed` \| `revoked` | The request closed without an establishment, or the connection was revoked |

A `ConnectionRolesMade` or `ConnectionRolesRefused` names its request's `ConnectionRequested` as its
`causation_id`; a `ConnectionRolesRemoved` names the record that closed the request its roles were
made for (a `ConnectionRefused` with occasion `connect`) or revoked the connection
(`ConnectionRevoked`), journaled after that request, before those roles' record or after it (rule
263). `append` does not check
the links. None carries a credential, a role's name, or a reason's detail.

**Consistency rule** (reason `schema`; the path is the member named):

261. Every §9.33 record: `actor.kind` is `system` (`actor.kind`), and `causation_id` is non-null
     (`causation_id`).

**Stream rules** on the control stream, the provisioner's, and failing closed in the verifier's
fold:

262. A `ConnectionRolesMade` or `ConnectionRolesRefused` follows the version-2 `ConnectionRequested`
     with `credential` `oauth` its `causation_id` names, of the same `connection_id`
     (`payload.connection_id`), and no two of either name one request (`causation_id`).
263. A `ConnectionRolesRemoved` follows its connection's latest `ConnectionRolesMade`, at most once
     after it (`payload.role`), and its `causation_id` names a record of the same connection that
     follows the request that `ConnectionRolesMade` answers, its `causation_id`, whether journaled
     before the `ConnectionRolesMade` or after it (`payload.occasion`):
     - with `occasion` `request_closed`, a `ConnectionRefused` with occasion `connect`, which closed
       the request the roles were made for;
     - with `occasion` `revoked`, a `ConnectionRevoked`.

     So a removal never cites the closer of an earlier request of the same connection id (DEC-699
     lets a refused id be requested again): a request whose roles are made keeps them until its own
     close or revocation. Roles made for a request that had already closed, since the provisioner
     writes the vault before it journals, are owed their removal at once, citing that close.

The token exchange's role is removed at once on either occasion. The executor's role is never
removed: an executor started for any reason, a kill switch's to apply among them (DEC-2343,
DEC-2344), can always log in, and a revocation destroys its token instead, so the role grants only a
read of a token that no longer exists. A connect that the API established keeps both roles for its
connection's life. Rule 131's "nothing further is journaled"
for a request a revocation closed concerns the request's own records; a removal is the
provisioner's.

**No `JournaledFact`.** **Vectors**: `reference/journal`'s `connection_roles` section holds a base
draft of each record, an invalid draft for every member type and each clause of rule 261, valid
drafts for the cases it might be misread to refuse, and control streams for rules 262 and 263. The
vectors stay version 3.

### 9.34 A held retirement ([DEC-2352](../project/decisions/DEC-2352.md))

This subsection adds `RetirementHeld` on the agent stream at schema version 1. §9.2's and §9.7's
conventions and types apply. The rules number from 265, after #2294's rule 264.

A runtime retires an agent that ran only once the account stream's fold shows no position
attributed to the agent and no working order of its own (§9.2's `AgentStopped`,
[DEC-2344](../project/decisions/DEC-2344.md) item 2). While it waits, the runtime writes one
**sequence** of `RetirementHeld` records, one per further 24 hours of risk clock, each with an
`OwnerAlertSent` (`retirement_held`, its `subject` this record) as the next row of the same batch
(rule 130, notifications spec §3.2):

- **Periods count records, not days.** Period k+1 follows period k, at least 24 hours of risk
  clock after it (rule 266), and the first at least 24 hours after `held_since` (rule 265); the
  runtime writes each at its first risk tick once due. A runtime that was down for days writes the
  next period when it is back, and the one after a day later, never the missed ones in a burst, so a
  period's number is its place in the sequence and its `risk_clock` says when it was written.
- **It keeps writing in every agent mode,** `paused` and `stopped` included: a kill switch or a
  hold during the wait is when the owner most needs to hear that the agent still holds something.
- **One sequence per agent.** A second cause while a sequence is open starts nothing, and the
  first cause's sequence continues (rule 266), with one exception: an owner's Stop. Its
  `OwnerCommandIssued` ends the open sequence, and the Stop is the cause of its own, from 1.
  Repeated Stops each begin a new sequence; the owner is engaged, so the extra alerts are
  acceptable.
- **The sequence ends with the retirement, read from records already journaled**, all of this
  agent's own: its `AgentStopped`, or an owner's `OwnerCommandIssued` scoped to the agent with
  `command` `resume` or `stop` (control stream). No `RetirementHeld` of that cause follows. A
  `MandateVersionApplied` with `result` `applied` for the agent (account stream) ends nothing by
  itself. A version **cancels** the retirement when, from records a verifier replays, the cause no
  longer holds. For `profit_stop`, the cause is a `RiskLimitTriggered` (§9.27), and the version
  cancels it with a new `profit_level` above the return at the trigger, (`agent_equity` −
  `capital_base`) / `capital_base` from that record's snapshot (mandate spec §5). For a passed
  `end_date` (a `GoalCompleted`, reason `end_date`) or a goal's `release` (a `GoalCompleted`,
  `on_complete` `release`), it compares the `end_date` or `on_complete` of the version applied
  (its `new_version`) with those of the version the sequence's first record names in
  `config_refs.mandate_version`: a later `end_date`, or a changed `on_complete`, cancels it. No
  version cancels any other cause:

  | Cause of the retirement | Its record | A version cancels it by |
  |---|---|---|
  | `profit_stop` reached (`GoalCompleted` reason `profit_stop_reached`, after its limit) | The `RiskLimitTriggered` with `limit` `profit_stop` | A `profit_level` above the return at that trigger |
  | A passed `end_date` | The `GoalCompleted` with reason `end_date` | A later `end_date` |
  | A goal's `release` | The `GoalCompleted` with `on_complete` `release` | A changed `on_complete` |
  | `target_qty` or `max_spend` reached | The `GoalCompleted` with that reason | Nothing: the goal was met |
  | The owner's Stop | The `OwnerCommandIssued` with `command` `stop` | Nothing: only a resume or the owner's own later command ends it |
  | Any other limit's trigger (a drawdown rung, the lifetime floor, the daily loss, a tripwire) | The `RiskLimitTriggered` with that `limit` | Nothing: its return is the loss that triggered it, so no `profit_level` reads against it | The agent's current return is never read, so a dip below the level never cancels a
  retirement whose exits are unfilled. A version that does not cancel
  it leaves the runtime writing the same sequence under the new version. One that cancels it
  leaves the runtime writing nothing more of that cause (the writer's rule), and if the agent
  retires again, the new cause begins its own sequence from 1. So an applied version can add
  alerts, never silence one. A redeploy is a new agent id on its own stream
  ([DEC-1173](../project/decisions/DEC-1173.md)), so nothing carries over to it.
- **A restarted runtime folds its stream first**, so it never writes a period twice.
- **A silent sequence is a verifier finding.** An open sequence whose last `RetirementHeld` is more
  than 48 hours of risk clock old, with no end and no version that cancels it since, is not
  refused at append, since a down runtime writes nothing; the verifier reports it as
  `retirement_held_silent` for the agent, so a writer that stopped while the agent still held
  something is seen, and a cancelled retirement raises nothing. The `retirements` vectors' `silence`
  cases pin it: writing, silent, ended, cancelled by each kind of version, and a level still
  breached.

**`RetirementHeld`** (agent stream; `config_refs` names `mandate_version`)

| Member | Type | Meaning |
|---|---|---|
| `held_since` | `risk_clock` | The cause record's own risk clock, when the retirement began waiting: rules 265 and 266 |
| `period` | integer | The record's place in its sequence, from 1: rules 265 and 266 |
| `waiting_on` | `[position \| working_order]` | What the agent still holds of its own, each once, in that order: rule 265 |
| `risk_clock` | `risk_clock` | The tick it was written at: rule 265 |

Its `causation_id` is the record that began the retirement: the owner's Stop, a `profit_stop`
limit, a goal's `release`, or a passed `end_date` (mandate spec §3.1). It names no order,
position, instrument, or quantity (`AGENTS.md` rule 6).

265. `RetirementHeld`: its `actor` is the runtime's (`agent`, `actor.kind`), never a workspace
     service's or a user's; `causation_id` is non-null (`causation_id`); `period` is at least 1
     (`payload.period`); `risk_clock` is no sooner than `held_since` plus 86400 seconds
     (`payload.risk_clock`); and `waiting_on` is non-empty, each value once, in ascending order
     (`payload.waiting_on`).
266. **Stream rule** (the agent stream, read with the control and account streams' records about
     the agent, in commit order). (a) Each `RetirementHeld` is followed by its `OwnerAlertSent`
     (`retirement_held`, `subject` it), and each such alert follows the record it names. (b) At most
     one sequence is open. A `RetirementHeld` while one is open has its cause, the next period, the
     first one's `held_since`, a `config_refs.mandate_version` that is the sequence's or the
     `new_version` of a `MandateVersionApplied` (`result` `applied`) for this agent since its last
     record, and a `risk_clock` at least 86400 seconds after its last record's. One of another cause
     is refused, unless such a version was applied since the open sequence's last record: then the
     open sequence has ended, and the new one begins. A `RetirementHeld` with none open has period
     1. (c) This agent's `AgentStopped`, and an `OwnerCommandIssued` scoped to this agent with
     `command` `resume` or `stop`, end the open sequence, and no `RetirementHeld` of an ended
     sequence's cause follows. Another agent's records, another command, and a rejected version end
     and move nothing. The fold reports the first record that breaks it; other records are not read.

**Vectors.** The `retirements` section of the reference cases: a base draft, an invalid draft for
every member type and for each part of rule 265, valid drafts for a late period, both values, a
working order alone, and period 3, and 28 streams for rule 266, among them days down and a burst
after them, a second cause while one holds, each kind of end, another agent's end, a workspace resume, a pause, a
version that leaves it retiring and one that is rejected, and a record without its alert. The
reference fold and an independent oracle, which reads the ends itself and walks each segment's
consecutive records, agree on every stream, and each check is shown catching its seeded bug.

### 9.35 The account's day-trading facts ([DEC-2398](../project/decisions/DEC-2398.md))

This subsection closes the account stream's `AccountSnapshotRecorded` at `schema_version` 2.
Version 1 (§9.2) is never edited (§8) and stays registered, so `append` and verification accept
both. §9.2's conventions apply: §9.1's and §9.2's types, the absent-member rule, the report order,
and no required `config_refs`. No rule is added; rule 24 applies to version 2 as it does to
version 1.

[Trading spec §9.2](trading-domain.md#92-day-trading-regime)'s day-trade budget reads facts that
version 1 does not carry, and that no other record journals:
- **which regime applies, and to which account type:** the connector's declaration from the
  broker's published contract ([trading spec §7.2](trading-domain.md#72-account-state-and-buying-power),
  [DEC-840](../project/decisions/DEC-840.md) item 1), which never depends on a broker call;
- **the prior-close equity** that `legacy_pdt` compares with its threshold, which is the broker's
  `last_equity` under either regime (DEC-840 item 5);
- **the maintenance excess** `intraday_margin` checks, which is `equity` less the broker's
  `maintenance_margin` ([DEC-524](../project/decisions/DEC-524.md) item 2).

Without them a gate that reads only the journal, such as the cell's runtime's dry run, can only
defer the check ([DEC-2378](../project/decisions/DEC-2378.md) item 11). Version 2 records all four in
the same snapshot as the equity the figures are compared with.

**`AccountSnapshotRecorded`** on the account stream at `schema_version` 2: version 1's account
members, then `account_type`, `day_trading_regime`, `last_equity`, and `maintenance_margin`, then
version 1's cash comparison, with `risk_clock` last.

| Member | Type | Meaning |
|---|---|---|
| `status` … `accrued_fees` | as version 1 | As version 1 (§9.2) |
| `account_type` | `cash` \| `margin` \| `null` | The account type the connector declares (trading spec §7.2); `null` when it declares none |
| `day_trading_regime` | `intraday_margin` \| `legacy_pdt` \| `null` | The §9.2 regime the connector declares; `null` when it declares none |
| `last_equity` | `decimal` | The broker's equity at the prior session's close, as it reported it ([trading spec §7.2](trading-domain.md#72-account-state-and-buying-power)) |
| `maintenance_margin` | `decimal` | The broker's maintenance margin requirement, as it reported it |
| `model_cash`, `cash_band`, `cash_in_band` | as version 1 | As version 1: rule 24 |
| `risk_clock` | `risk_clock` | As version 1 |

Both figures are recorded as the broker states them, never derived, rounded, floored, or replaced:
no rule ties `last_equity` to `equity`, and a deficit (`maintenance_margin` above `equity`) or a
negative figure is well formed. A writer that has not read a figure writes version 1, never a
made-up value (`AGENTS.md` rule 3). The declaration is the connector's, never inferred from the
figures or from a broker's name, and no rule ties the regime to the type. The prior close's
maintenance margin is not journaled.

**How a reader takes the figures.** A version-1 snapshot has **no figures** and no declaration: a
reader that needs them treats them as unavailable and holds what depends on them, never reads them
as zero, as `equity`, or as any broker's regime. A `null` declaration is unavailable the same way.
The latest snapshot on the stream counts, whatever its version, so a version 1 after a version 2
leaves no figures, not the older ones. A version-2 snapshot's prior-close equity is `last_equity`
as it stands, and its maintenance excess is `equity` less `maintenance_margin`, signed, so a
deficit is negative, never floored at zero. A negative `maintenance_margin` gives no excess
(DEC-524 item 2: subtracting one would raise the excess above equity), and a reader holds what
depends on it as on a version 1. Version 2 does not carry everything §9.2 reads: under
`legacy_pdt` an account the broker flags as a pattern day trader has no budget below the threshold,
and that flag is journaled nowhere, nor is the trailing day-trade count. A reader that cannot see
one of them holds or defers the budget, never reads the flag as unset or the count as zero, so these
four facts alone never pass a `legacy_pdt` budget. Holding an opening for a missing fact only stops
risk being added; no exit, protective order, or kill switch reads any of the four (`AGENTS.md` rule
13).

**Who writes it.** The executor writes every `AccountSnapshotRecorded`, the cash step's and the fee
step's (§9.2), at version 2 from the change that builds it, once `mandate-journal` registers it;
until then `append` refuses version 2 as `unknown_schema`, and version 1 continues.

**The mapping to `JournaledFact`.** Version 2 maps as version 1 does (§9.2): `AccountSnapshot`, from
`equity`, with the account stream's connection given. None of the four is a `ValidationContext` fact.

**Vectors**: `reference/journal`'s `snapshot_prior_close` section holds two version-2 base drafts,
a reconciliation's with its cash comparison and a fee step's without one, and a version-1 base
draft, all valid; an invalid draft for each new member's types and forms (a declaration's name in
another case, unlisted, or another member's), each figure `null`, each new member missing, a
version-1 draft carrying any of them, an extra member, rule 24 at version 2, and version 3; valid
drafts for what no rule refuses (an undeclared type, regime, or both; `cash` with `legacy_pdt`, and
`margin` with it; a prior close above equity, negative, or zero; a margin of zero, equal to equity,
above it, or negative; figures with a trailing zero or an exponent, which §4.6 normalizes); the reading of each base and valid draft: `no_figures` for version 1, and
for version 2 the declaration, the prior-close equity, and the signed maintenance excess, `null`
when the margin is negative; and streams in which the latest snapshot wins across versions: a
version 1 then a version 2, a version 2 then a version 1 (no figures), two version 2s, and a
version 2, a version 1, and a version 2 again, and a version 2 then one with a negative margin
(no excess). The oracle recomputes each reading in exact
fractions, apart from the reference reading. The vectors stay version 3.
### 9.36 The customer real-money gate ([DEC-2134](../project/decisions/DEC-2134.md))

This subsection adds `RealMoneyGateChanged` and `RealMoneyRefused` on the workspace control stream
at schema version 1. §9.2's conventions apply: §9.1's and §9.2's types, the absent-member rule, the
report order, and `schema_version` 1. The rules number from 273, the range reserved for lane GL.

Until counsel signs off, every customer trades paper (compliance doc, "Regulatory posture";
DEC-102). The gate is the workspace's own setting of whether any request may name an environment
other than `paper`. **The setting is the `enabled` of the latest `RealMoneyGateChanged` on the
workspace's control stream, by `seq`; a stream with none is off.** Only a platform operator writes
the setting (rule 273), and no workspace route writes it (DEC-2134 item 3). While it is off, the
workspace API refuses every request that names another environment, and journals the refusal as
`RealMoneyRefused` before it answers (DEC-2134 item 4). **On is necessary, never sufficient:** a
request the gate lets through still meets every later check, and a version, deployment or
connection in another environment is refused as `live_unavailable` until a decision of the
founder's opens one (DEC-79, DEC-2134 item 5).

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `decision_id` | `DEC-` then a decimal number with no leading zero, `DEC-1` to `DEC-99999` | Another JSON kind `schema`; another string `non_canonical` |

**`RealMoneyGateChanged`**: a platform operator set the workspace's real-money gate.

| Member | Type | Meaning |
|---|---|---|
| `enabled` | `boolean` | Whether the gate is on from this record |
| `operator` | `ulid` | The platform operator who set it, as its `actor.id` (rule 273) |
| `decision` | `decision_id?` | The accepted decision that allows the workspace to leave paper; never `null` when `enabled` is `true` (rule 273), and may be `null` when it is `false`: turning the gate off needs no decision |

**`RealMoneyRefused`**: the workspace API refused a request that named another environment while
the gate was off.

| Member | Type | Meaning |
|---|---|---|
| `action` | `create_version` \| `deploy` \| `connect` \| `go_live` | What was refused: a mandate version created, an agent deployed, a broker connect started, or the owner's own request to move the workspace to real money |
| `requested` | `live` | The environment the request named |
| `principal` | `ulid` | Who asked, as its `actor.id` (rule 274) |
| `reason` | `gate_off` | Why: the gate's setting was off when the request was judged |

**Consistency rules** (reason `schema`; the path is the member named):

273. `RealMoneyGateChanged`: the actor is a `platform_operator` whose `id` is `operator`
     (`actor.kind`, then `actor.id`), so no workspace member, client, service account or service
     sets it; and when `enabled` is `true`, `decision` is not `null` (`payload.decision`).
274. `RealMoneyRefused`: the actor is a `user` whose `id` is `principal` (`actor.kind`, then
     `actor.id`): the workspace API records the refusal as the principal it refused, as it records
     every command it commits.

**What the gate does not do.** It moves no money and turns nothing on by itself: an on setting
admits a request only to the checks after it. A refusal changes no state: the request leaves no
other record. Its `event_id` is derived from the request's idempotency key (workspace API spec
§3.4), so a retry under the same key finds it recorded, and one under a new key is recorded again. A `RealMoneyRefused` grants nothing and
no reader but the gate's audit view reads it. No record here is step-up evidence.

**No `JournaledFact`.** Neither record feeds a `ValidationContext`. **Vectors**:
`reference/journal`'s `real_money` section holds a base draft of each record, an invalid draft for
every member type and each clause of rules 273 and 274, valid drafts for the cases a rule might be
misread to refuse (an off setting with no decision, an off setting with one), and control streams
the gate's fold reads, each with the setting it must give. The vectors stay version 3.

### 9.37 The news stream ([DEC-2278](../project/decisions/DEC-2278.md))

This subsection adds the news stream, `news:{workspace_id}`, its `StreamOpened`, and its one record,
`NewsItemStored`, at schema version 1. §9.2's conventions apply: §9.1's and §9.2's types, the
absent-member rule, the report order, and `schema_version` 1. The rules are 283 to 286, the range
reserved for lane NE.

**What it is for.** The workspace's news reader fetches Alpaca news through the vetted allowlist
([data-plane spec §4.2](data-plane.md#42-ingestion), [DEC-2394](../project/decisions/DEC-2394.md)),
with its own paper data key ([DEC-2277](../project/decisions/DEC-2277.md)). For each new item, the
reader:

1. keeps the raw response only in memory while it parses the item, and stores none of it: a
   durable raw store waits on the data plane's store and on DEC-433 item 20
   ([DEC-2280](../project/decisions/DEC-2280.md) item 4);
2. stores the item's typed record in the artifact store;
3. appends `NewsItemStored` naming the record, before the item can reach any run.

The typed record is the bytes `mandate-research-run`'s `RetrievedItem::record` writes for the whole
item, untrimmed. It holds the item's typed fields and, for a licensed item, no text: `headline`,
`summary`, and `link` are `null`, with only their combined `length_bytes` kept. So the journal pins no licensed text.

The agent runtime reads the stream and offers each recorded item as an event trigger
([agent harness spec §5.3](agent-harness.md), [DEC-2392](../project/decisions/DEC-2392.md),
[DEC-2275](../project/decisions/DEC-2275.md)). It builds the offer from the record's members and
takes `item_ref` as the offer's `data_ref` as it is. It never rebuilds the item, and never
re-derives the digest from a run's trimmed read. If the item starts a run, the agent stream's
`ObservationRecorded` names `item_ref` as its `data_ref`, so rule 271 binds one stored item to at
most one run. Its `instrument_id` is the instrument of the thesis the item matched, which is how an
item about several tickers still names one.

**`StreamOpened`** on the news stream: `{stream_type: "news", workspace_id: id}`. The reader writes
it in the same batch as its first `NewsItemStored`, so a reader with no key, or one that has stored
nothing, leaves no stream.

**`NewsItemStored`**: the reader stored one news item.

| Member | Type | Meaning |
|---|---|---|
| `source` | `text` | The allowlist source the item came from, as the allowlist names it (`alpaca.news`) |
| `allowlist_version` | `integer` | The allowlist version the fetch was permitted under |
| `item_id` | `text` | The item's id: the vendor's id and, where it has one, its version (data-plane spec §4.4), written `<vendor id>:<vendor version>` with the version as a timestamp (DEC-2280 item 3), as `[A-Za-z0-9._:-]{1,128}`, never starting `bar:`, `quote:`, or `instrument:`, which the research core's own items use; one record per stream: rule 284 |
| `symbols` | `[text]` | The vendor's tickers for the item, sorted, unique, one to 32: rule 283 |
| `published_at` | `timestamp` | When the vendor says it published the item |
| `knowledge_at` | `timestamp` | When the item became known to the platform: the later of its receipt, its `published_at`, and the stream's last `knowledge_at` (rules 283 and 285), so a vendor's clock ahead of ours, or a writer's behind the last, never puts an item before what was already known (DP-1) |
| `item_ref` | `ref` | The item's typed record in the artifact store, described above |
| `licensed` | `boolean` | Whether the item's text is licensed |

The record carries no headline, summary, link, or raw response, and neither does the artifact it
names for a licensed item. Data-plane spec §4.4's group key for syndicated copies is the data
plane's, kept with the item there, and is not journaled here: each copy is its own item, with its
own `item_id`.

**Consistency rules** (reason `schema`; the path is the member named):

283. `NewsItemStored`: `symbols` is sorted by bytes, holds no member twice, and holds one to 32
     members (`payload.symbols`); `item_id` matches the grammar above (`payload.item_id`); and
     `knowledge_at` is not before `published_at` (`payload.knowledge_at`).
284. No two `NewsItemStored` on a stream name the same `item_id` (`payload.item_id`). The reader
     stores an item once: a later fetch of the same item, by this process or after a restart or a
     failover, stores nothing, and a vendor's later version is a new `item_id` with its own record.
285. `knowledge_at` is not before the previous `NewsItemStored`'s on the stream
     (`payload.knowledge_at`). Equal is allowed: one response holds many items.
286. News-stream `StreamOpened`: `stream_id` equals `news:{workspace_id}` (`stream_id`, reason
     `stream_mismatch`); and every record on the stream is written by a `system` actor
     (`actor.kind`), the news reader.

**What the writer owes.** The reader folds the stream at start, under its new epoch, for the
`item_id`s it holds and the last `knowledge_at`. It asks the vendor about the tickers of the active
theses' and held instruments, each the `symbol` of the latest `instrument_snapshot` the control
stream registers for that instrument; an instrument with none is not asked about, and is counted
by asset id (DEC-2280 item 1). It asks the vendor only from that instant, or from
48 hours back for a stream with none (DEC-1130 item 5). It stores an item the rules would refuse
nowhere: one with no ticker or more than 32 is skipped and counted, by id only, in the reader's
metrics, never journaled and never silent. A refused append is the reader's failure, not a skipped
item: the reader stops and reports not ready, and the next process folds the stream again.

**Checked by no rule.** That a news-sourced `ObservationRecorded` on an agent stream names the
`item_ref` of a `NewsItemStored` crosses streams, so `append` does not check it, as for rule 271's
cause. The verifier owns it.

**Not a risk input.** No gate, limit, or fold of the account or the mandate reads the stream, and
no record on another stream copies from it. An agent stream's `ObservationRecorded` names an item's
record, as it names any read's. **No `JournaledFact`.**

**Vectors** follow with the record's registration in `mandate-journal`, before the reader runs, as
the meter streams' did (§9.19). They hold:
- a base `StreamOpened` and `NewsItemStored`;
- an invalid draft for each member type and each clause of rules 283 to 286: 0 and 33 tickers,
  unsorted and repeated tickers, an `item_id` outside the grammar and one with a reserved prefix,
  `knowledge_at` before `published_at`, a repeated `item_id`, a `knowledge_at` before the
  stream's last, a mismatched `stream_id`, and a `user` actor;
- valid drafts for the near-misses: 1 and 32 tickers, `knowledge_at` equal to `published_at`, and
  equal to the stream's last.

## 10. Anchoring

- **Frequency:** every 5 minutes (configurable) and at each end of day.
- **Leaves:** `[{stream_id, seq, hash}]` for every stream's head, sorted by `stream_id` bytes. The
  control stream's leaf is its head **before** the `AnchorComputed` event that records the anchor.
- **Tree (RFC 6962 style):** leaf = SHA-256(0x00 ‖ canonical(leaf)); node = SHA-256(0x01 ‖ left ‖
  right); for n > 1 leaves, split at the largest power of two below n; a single leaf is its own root.
- **Timestamp:** the imprint sent to an RFC 3161 timestamping authority is SHA-256 of the 32 raw
  root bytes. The token, certificate chain, and revocation data are stored as artifacts and
  re-stamped before expiry. In hybrid mode the root is also sent to the global control plane.
- `AnchorComputed` records all leaves and the root. Timestamping outages are queued and retried,
  never block trading, and are journaled as gaps.

## 11. Verification

Input: a stream range with a **trusted start** (`from_seq`, `trusted_prev_hash`) taken from a
manifest or anchor (seq 1 with 64 zeros for a full stream). Events are walked in `seq` order (file
line order for exports), reading `seq` from the body.

**Per-event checks, in order** (the first failure is reported):

1. `non_canonical` — the body parses (duplicate keys rejected) and re-canonicalizes to the same bytes.
2. `column_mismatch` — stored columns (`stream_id`, `seq`, `event_id`, `event_type`,
   `schema_version`, `environment`, `recorded_at`, `prev_hash`) equal the body.
3. `seq_gap` — `seq` equals the previous `seq` + 1.
4. `rehash_mismatch` — SHA-256(body) equals the stored `hash`.
5. `prev_hash_mismatch` — `prev_hash` equals the previous event's `hash` (or the trusted start).
6. `artifact_missing`, `artifact_mismatch` — every referenced artifact exists and re-hashes.

**Per-range checks** read more than the one event they judge. Each is reported in one of two ways,
which `VerificationRun`'s `failure.seq` records ([§9.13](#913-records-access-export-and-verification-records-dec-780)
rule 111):

- **for the range**, with no `seq`: `anchor_root_mismatch`, `tsa_token_invalid`,
  `segment_manifest_mismatch`, `segment_rows_mismatch`, and `segment_gap`;
  - `tsa_token_invalid`: the token of an anchor the range reads does not carry that anchor's
    `root` as its imprint, or does not verify. The anchors it reads include the trusted-start
    anchor of a range entered from an `AnchorComputed`, not only the anchors inside the range
    ([DEC-896](../project/decisions/DEC-896.md) item 1). Range-level; `seq` null. Workspace API
    §4.8.1's run order checks them at step 2.2, after `anchor_root_mismatch`. A token that cannot
    yet be proven either way is incomplete, not this failure ("Incomplete", below, [DEC-789](../project/decisions/DEC-789.md));
  - `segment_rows_mismatch`: the hot rows of a segment named by a manifest the range starts from
    or holds do not export (§6.2) to the bytes its `file_sha256` names. A hot-store rewrite that
    keeps the chain consistent from the trusted start onward is caught here, and by no per-event
    check. Range-level; `seq` null ([DEC-894](../project/decisions/DEC-894.md)). Workspace API §4.8.1's run order runs it
    right after `segment_manifest_mismatch`. The test vectors' `segment_rows` hold its cases;
- **at an event**, with that event's `seq`: `anchor_head_mismatch` (the event at each anchored
  `seq` exists with the anchored hash, reported at that `seq`) and every check listed below.

On the control stream ([§9.14](#914-anchor-and-segment-records-dec-783), [§9.13](#913-records-access-export-and-verification-records-dec-780)):

- `anchor_self_mismatch` — an `AnchorComputed` has a leaf for its own control stream, and that leaf
  names the event just before it: its `seq` is one less than the `AnchorComputed`'s own, and its
  `hash` is that event's `hash` (§10), reported at the `AnchorComputed`. The test vectors'
  `cold_records.range_checks` hold a case for each clause;
- `break_glass_cause_mismatch` — a `RecordsAccessed` whose actor is a `platform_operator` names, as
  its `causation_id`, an earlier `PlatformOperatorAction` on this control stream, so of the same
  workspace: the customer-approved break-glass action that opened its window (§7). Rule 108 checks
  at `append` only that the `causation_id` is non-null, since `append` cannot see earlier batches.
  A cause that names a later event or an event of another type fails, and so does one that names no
  event in a range that starts at seq 1, the full-chain run among them (below); each is reported at
  the `RecordsAccessed`. **Until DEC-261 item 9 closes `PlatformOperatorAction`,
  `append` refuses that record (`unknown_schema`), so no control stream holds one, and every
  operator read that `append` admits fails this check** wherever it is judged, always in the
  weekly full-chain run. It fails closed on purpose, as rules 109 and 110 do for an operator's
  export or verification. The test vectors' `records_access.range_checks` hold a case for each
  clause, the first of them the read `records_access`'s valid draft `read_by_an_operator`.

On an agent stream ([§9.1](#91-agent-stream-payload-schemas-dec-177)):

- `intent_action_mismatch` — an `IntentProposed` whose `causation_id` names a `DecisionMade` has
  that decision's action members (rule 10), and one whose `causation_id` names an
  `ApprovalRevalidated` has the action members bound in that approval's `ApprovalRequested`,
  reported at the `IntentProposed`. The second clause is checked, with vectors, once §9.1 closes
  the approval events' schemas; until then no range fails on it;
- `mode_event_mismatch` — a `KillSwitchActivated` whose `mode_event` is non-null names an earlier
  `AgentModeChanged` on this stream with reason `kill_switch`, reported at the
  `KillSwitchActivated`;
- `held_mismatch` — the owner's hold is carried, never dropped (§9.11, [DEC-672](../project/decisions/DEC-672.md),
  [DEC-673](../project/decisions/DEC-673.md)). Only `AgentModeChanged` records are read; any other
  event between them leaves the hold as it was. The hold carried is the **expected** one, derived
  from the reasons: `owner_hold` sets it, `owner_lift_hold` clears it, and every other record keeps
  it whatever it wrote. A hold or lift whose `held` contradicts its reason fails; any other
  version-2 `AgentModeChanged` whose `held` differs from the carried one fails; and no version-1
  `AgentModeChanged` follows a version-2 one. **The check anchors on the stored chain, not on the
  range:** the verifier is given an anchor that its caller derives from the stored chain before
  the range's trusted start, one of no version-2 `AgentModeChanged` before it, or the `held` the
  last one before it carried (from a hold, a lift, or a carried copy), or none when the caller
  cannot read that chain. It is folded only from a verified prefix bound to the range's start:
  records `1` to `from_seq − 1` pass checks 1 to 6 from genesis, and the last of them has `seq`
  `from_seq − 1` and the trusted start's `prev_hash` as its hash (DEC-892); a prefix that fails
  either gives no anchor. A full chain is anchored on nothing before it. With no anchor, the
  range's first version-2 record that is not a hold or a lift fails closed, reported as having no
  anchor, and a version-1 record before it is not judged: every stream written before §9.11 is all
  version 1, and the full-chain run or an anchored range catches a version 1 after an unseen
  version 2. Reported at the first record that breaks it.
- `approval_deferral_mismatch` — rule 292 across batches
  ([DEC-2522](../project/decisions/DEC-2522.md)): an `ApprovalRequested` names a `causation_id`,
  that cause is an earlier `DecisionMade` on this stream, and the request's
  `content.deferred_to_binding` equals the decision's `payload.deferred_to_binding`, an absent
  member read as empty on either side, each list as `append` admitted it (rules 269 and 291 fix its
  order), so the same names in another order differ. A cause is looked up only among the range's
  earlier events: one outside them fails on a full chain, so a request before its decision, or
  naming no event, fails there, and is left unchecked on a later range, whose earlier events are not
  in hand. Reported at the `ApprovalRequested`. `append` checks rule 292 only within one batch,
  which is how the runtime writes the two; this check catches a request written in another batch
  that hides or invents a deferral. The workspace API reads and refuses the same disagreement at the
  read and the grant ([DEC-2519](../project/decisions/DEC-2519.md),
  [DEC-2521](../project/decisions/DEC-2521.md)). The test vectors' `approval_deferrals.ranges` hold
  a case for each clause.

On a control or account stream ([§9.8](#98-connection-records-dec-800)):

- `connection_lifecycle_mismatch` — a record that breaks stream rule 66, 67, 68, 131, or 201
  (§9.24), reported at that record. A `ConnectionRequested` still open at the range's end is a connect in progress,
  not a mismatch. **The check anchors on the stored chain, not on the range**
  ([DEC-885](../project/decisions/DEC-885.md)): those rules read records a range can begin after (an
  establishment, a revocation, a request, a binding copy, a check, a state), so the verifier is
  given the range's **connection anchor**, the state the full-chain run's fold of rules 66, 67, 68,
  and 131 holds after the stream's records `1` to `from_seq − 1`. Its caller derives it by that
  fold over the stored chain (the hot store, or an export's segments from `seq` 1), never from a
  read model or a service's memory, and only from a verified prefix bound to the range's start, as
  the hold anchor is (DEC-892); a prefix that fails either check gives no anchor. A connection
  anchor covers one stream ([DEC-889](../project/decisions/DEC-889.md)): an anchored range fails closed, with cause
  `unanchored`, at the first record of another stream that the range rules judge, the same set the
  unanchored run judges (below). A control-stream `ConnectionRevoked` of another stream is never
  judged ([DEC-885](../project/decisions/DEC-885.md) I6); an account-stream one is
  ([DEC-888](../project/decisions/DEC-888.md)). An empty prefix's anchor (`from_seq` 1) takes its
  stream from the range's first judged record. On a control stream it holds each id's first establishment (with its
  version, which rule 201 reads), its latest establishment or revocation, and its latest scopes; each `account_ref`'s holder; the open
  requests with their members; every `account_ref` a request named; rule 67's ids whose latest
  revocation closed a request and ids revoked by one that closed none; and whether the stream holds
  a request. On an account stream it holds the owning `connection_id`; the latest copy of the
  establishment, or none while `connecting`; the latest check of each occasion, with its `seq`,
  whether it passed every result, and whether it listed `contract`; each rotation copy and the
  `seq` of its `reauthorize` check; the state the last `ConnectionStateChanged` left and whether it
  was a `condition_cleared`; and the `seq` of the last entry into `suspended`. A full chain is
  anchored on nothing before it.
  - **Anchored**, the range judges every record of the anchor's stream in it from that state, as the full-chain run
    would, rule 131's requirement that a connect's establishment or refusal closes an open request
    included: the anchor carries the requests before the range. An anchored range of the anchor's
    stream alone and the full chain report the same first record and rule.
  - **No anchor**, because the caller cannot read the chain before the range, or the fold breaks a
    rule before it (which the full-chain run reports): the range fails closed at its first record
    that rule 66, 67, 68, 131, or 201 judges, a `ConnectionRequested`, `ConnectionEstablished`,
    `ConnectionCredentialRotated`, `ConnectionRefused`, or version-2 `AgentDeployed` on a control
    stream or any connection record on an account stream, since a record before the range can break each. It is reported
    at that record as `connection_lifecycle_mismatch`, as having no anchor (cause `unanchored`,
    not a rule), as `held_mismatch` reports its own; records before it are not judged. A
    `ConnectionRevoked` on a control stream is never judged, so it never fails closed. One on an
    account stream is a connection record there, judged by rule 68
    ([DEC-888](../project/decisions/DEC-888.md)): refused when anchored, and failing closed when
    not. The test vectors' `connection_ranges` hold the anchored and unanchored cases, and
    `connection_revocations` the account-stream revocation's.

Across the control stream and its account streams, in the full-chain run only (a range never
holds the other stream):

- `connection_cause_mismatch` — a control-stream `ConnectionEstablished` version 2 whose
  `causation_id` is not a `ConnectionChecked` on the account stream its `account_ref` names, for the
  same `connection_id`, with occasion `connect` (`reconnect` for a later establishment of the
  same id on the same control stream, after an earlier one of either version), every result
  `passed`, and `contract` listed for an MCP connection; the same for a
  `ConnectionCredentialRotated`, with occasion `reauthorize`, for a connection established on its
  own control stream; or an account-stream copy of either
  whose `causation_id` is not a control-stream record of its type with the same payload but
  `risk_clock`. Reported at the record. The test vectors' `connections.chains` hold a case for
  each.

**Incomplete** ([DEC-789](../project/decisions/DEC-789.md), the founder): a check that cannot yet be
proven either way ends its range `incomplete`. It is neither a pass nor a failure, and it is not a
code of the list above: it names the check it could not finish. At this version one check can be
incomplete, the token check whose failure is `tsa_token_invalid`:

- **A token with the anchor's imprint.** `verify_tsa` checks the imprint and never answers `Ok`
  ([DEC-265](../project/04-decision-log.md#decisions) item 1): the RFC 3161 signature, the
  certificate chain, and revocation are not verified, so a token without the imprint fails
  `tsa_token_invalid` and one with it is incomplete. The library calls this
  `TsaVerificationIncomplete`; `mandate journal verify-cold` prints it as
  `result: failed, tsa_verification_incomplete` with a non-zero exit
  ([DEC-490](../project/decisions/DEC-490.md) item 6). That is the same outcome under the CLI's
  word: a command line has no third exit, and it never prints `verified` for it. Its `failed` is
  only that output form and its non-zero exit; this spec does not count it as a failure (no `fail`
  result, no SEV-1), and this version does not change the CLI. The CLI journals
  nothing, so no record holds its word; a `VerificationRun` records
  `incomplete: {check: tsa_token_invalid, cause: token_unverifiable}`. This holds for every
  anchor the token check reads, the trusted-start anchor among them
  ([DEC-896](../project/decisions/DEC-896.md) item 1). The start anchor lies before the range,
  so check 6 does not read its token: a start anchor's token whose artifact is absent from an
  artifact store that answers, or does not re-hash to its ref, cannot carry the imprint, so it
  fails `tsa_token_invalid`, a `fail` (DEC-896 item 2). An artifact store that cannot be read is
  an outage, not a failure: the check is incomplete with cause `token_unverifiable`, on which a
  restore drill passes as it does on the first cause (DEC-896 item 3, a reading of its own rather
  than DEC-789's).
- **An anchor the range must check whose `token` is `null`** (DEC-789 item 7). Nothing outside the
  journal vouches for it (§9.14), so its token check cannot finish either. It is not a failure: the
  outage is §10's journaled gap, already alerted. A `VerificationRun` records it with
  `cause: anchor_unstamped`. A range that meets both causes records `anchor_unstamped`, the one a
  drill treats as an incident (below).

An incomplete check never stops a run: the checks after it still run, and a failure any of them
finds is the range's result, which outranks it. `VerificationRun` version 2 records it in the
range's `incomplete` and, with no range failed, as the run's `result` `incomplete` (rules 132 and
133); version 1 cannot, which is why every writer appends version 2. **An incomplete run raises no
SEV-1** and starts none of the responses under "On failure" below: nothing was found wrong, and
until DEC-265 item 1's crypto half lands every stamped token is incomplete, so an alert on it would
fire on every run that reads an anchor. It pages no one: it is reported as the informational
"Journal verification incomplete" row of [infrastructure design](../design/infrastructure.md) §8.2,
and a `null` token is already alerted as §10's timestamping gap. **It is not a pass either:**
whatever requires a run to pass does not take an incomplete one as passing, with one exception the
founder made ([DEC-789](../project/decisions/DEC-789.md) item 9): a **restore drill** (infrastructure
design OPS-8, §6.3, §6.4) passes when every event of every range was walked (`checked` is the
whole range), no range failed, and the only check incomplete is the token check from the first
cause above, a stamped token with the imprint that cannot yet be verified. The second cause, an
anchor with a `null` token, was never vouched for: a drill that meets it is an integrity incident,
though the run still records `incomplete` and raises no SEV-1 (DEC-789 item 9, the narrower
reading the coordinator ruled under DEC-176). The drill reads the cause from the record: it
passes only on `pass`, or on `incomplete` with every event walked and every incomplete range's
`cause` `token_unverifiable`; any `anchor_unstamped` is an incident. A check that a later version lets end incomplete counts
for a drill only if a decision says so. Any other incomplete check, and any failure, is an
integrity incident for a drill. When the token
check can answer, the incomplete arm goes, as DEC-490 item 6 says of the CLI's.

A reference to an event before the range's trusted start is not checked by that range, except
through an anchor (`held_mismatch`'s, or the connection anchor, whose rotation copies a
`condition_cleared` may name); the weekly full-chain run checks every one, and there a `mode_event`, or an operator read's `causation_id`,
that names no earlier event fails. The test vectors' `agent_stream.range_verification` holds a case
for each for `mode_event`, and `records_access.range_checks` for the operator read's cause.

**Schedule:** the tail of every stream at startup and before each segment export; the full chain
weekly; results journaled as `VerificationRun`.

**On failure** (a `fail` result; never an `incomplete` one, above): SEV-1. Account- or agent-stream failures pause the affected agents (the kill switch
still works); control-stream failures freeze mandate and deployment changes, and the freeze never
holds risk reduction: risk exits, protective orders, owner exits, and the kill switch at any scope
still work (`AGENTS.md` rule 13), and the workspace API still records its risk-reducing operations
([workspace API spec](workspace-api.md#2-invariants) API-7). Affected segments are
put on legal hold. **Nothing is repaired in place:** a new writer epoch continues from an
`IntegrityIncidentRecorded` event that references the last good hash and anchor. The customer (for
advisers, their chief compliance officer) is notified within the configured deadline, and the
post-mortem is retained.

## 12. Export

- **Canonical export:** segment files, manifests, the relevant anchors with inclusion proofs,
  timestamp tokens with certificate chains, and the verifier's digest.
- **Examination bundle** (scoped by account, agent, and period): every related stream (account,
  agent, control, scheduler), referenced configuration objects and artifacts, schemas and upcasters,
  the verifier release and format specification, an index, and, for authorized requests, resolved
  identities. A deterministic human-readable report is generated with its own hash. Bundles are
  produced within the configured deadline; verifier releases and format documents are retained for
  the retention period.
- Human-readable views (for example, the causal trace from a fill to its observations) are derived
  from the canonical export, and each names its verifier digest, which identifies the canonical
  export ([DEC-265](../project/04-decision-log.md#decisions) item 3; `ExportCreated`, §9.13).

## 13. Open questions

1. Timestamping authority choice and fallback.
2. Whether hybrid customers may opt out of sending anchor roots to the global control plane.
3. Personal-data scanning approach for free-text artifacts.
4. Export deadline and customer-notification deadline values.
