# Product-flow map, main at 57e24bc (2026-10-26 ~10:45 UTC), lane DR

How this was checked: code reading and grep on main, plus cell runs. Docs alone never count.

- **CI:** main's run 40281337291 at a5f2e0c (`cell-demo-web`).
- **Local:** the cell on 72e5f7d (#2538's ask and #2545's connection cap included), with a debug
  build of the executor to read its refused append.

"Library" means the logic exists and is tested. "Deployed" means a cell process actually runs it. UNVERIFIED marks anything not traced to the end. Sizes: S (<1 day for one agent), M (1–2 days), L (more). The previous map was at 1349070. The demo's own guide, with tomorrow's window, is [demo-2026-10-26.md](demo-2026-10-26.md).

## Where the cell stops today

The local run on 72e5f7d ran steps 1 and 2 (sign-in, connect check, record, deploy at version 2),
and both agents' runtimes journaled an `ObservationRecorded` and a `ModelOutputRecorded` (long,
conviction 1). It stopped at step 2's check that the account's executor answers `livez`: the
executor restarts on every attempt to journal the agent kill switch the Job's step 7 records (X10).
In order, what stands between the trade loop and a fill:

1. **No fresh output on a Monday.** The crossover's output from Friday's close expired on Saturday
   (X11), so the runtime decides nothing until Monday's close. The demo's window is Tuesday
   2026-10-27, 13:30 to 20:00 UTC. In that window the runtime asks on a deferring opening (#2538).
2. **The executor restarts on its kill switch.** The journal has no account-stream schema for
   `KillSwitchActivated` (X10).
3. ~~**The approve is refused.**~~ **Closed by #2474:** the API commits a paper `Approved` grant whose `cli_confirm` step-up verifies (X6).
4. **The executor holds every opening.** Its binding gate has no deployment ports or broker facts
   yet (X5).

## The cross-cutting blockers

| # | Blocker | Evidence on main | Steps | Lane | Size |
|---|---|---|---|---|---|
| X1 | ~~No risk clock~~ **Closed.** The scheduler runs in the cell (#2439) | deploy/cell/charts/journal-db/files/processes.json:189-197 `built: true`; deploy/cell/images/scheduler.Dockerfile; xtask/src/cell.rs:884-891, :982-984. Locally the `scheduler` pod runs. The executor takes the first tick, then `Input::Started` (executord/src/process.rs:322-334); the runtime hands it in `clock_pass` (daemon/src/runtime.rs:415) | — | SC | — |
| X2 | ~~No model outputs in the deployed runtime~~ **Closed by DEC-2383 item 7 (lane MC)** | The daemon's models pass queues `Observation` and `ModelOutput` ahead of each tick (daemon/src/runtime.rs:448-512). `mandate-runtime-models`, the thin layer-9 composer the founder chose, implements `Models` over `mandate_modelhost::evaluate` with the deployed document's pins, the folded `model_version` registrations, the calendar and the closes `mandate_bars::trusted_daily_closes` admits under `MANDATE_BARS`, and the `mandate-daemon` binary, now in that crate, wires it for a resolved deployment (runtime-models/src/main.rs). Only the cell's recording names a source; the closes object must already be in the read-only store (DEC-2385 item 5) | 3, 6 | — | — |
| X3 | **Bars yes, marks no** | `data` is built and deployed with SPY and QQQ (processes.json:236-243; values/k3d.yaml:22-30; #2467, #2487), but it appends to no stream (processes.json:237). The executor's `MarkUpdated` v2 writer landed (#2449), but executord's fixture wiring marks nothing (`marked: NoMarks`, executord/src/fixture.rs:102), and its quotes are `NoQuotes` (executord/src/quotes.rs:44) | 3, 4, 6, 7 | X | M |
| X4 | ~~The gate's dry run denies every opening~~ **Closed by #2538 (D1's B2)**: the runtime now asks on an opening whose only open checks are the deferred ones | The deployed runtime uses `production::wired` (runtime-models/src/main.rs). It has a real plan (production/plan.rs), flatten, references and view, and a real gate dry run: `Gate` over `evaluate_deferring` with the registered rule set's gate config (production.rs:43-68; production/gate.rs:69-91; #2489, DEC-2388). That gate defers `UNJOURNALED`, which is eligibility, the liquidity floor, participation and the day-trade budget (gate.rs:26-30), on every opening, and denies any opening with a deferred check. `Plan::classify` never answers auto (plan.rs:90-133), so an opening that passed would ask | — | D1 | — |
| X5 | **The executor knows nothing** | The fixture broker fills (`FixturePaper::filling`, executord/src/main.rs:218; paper.rs:201-206; #2468), and executord reads `IntentProposed` (follow.rs:463-499, process.rs:582). But its wiring is still NoMandate, NoInstruments, NoFacts, NoProtection, NoQuotes and NoMarks (executord/src/fixture.rs:29-102), and NoFacts holds every opening at the binding gate (:63-70) | 3, 4, 6 | X (G2b-1 deployment ports, #2547), XF (G2b-2 broker facts for the fixture and Alpaca paper), then X for the fill and the protective stop | L |
| X6 | ~~The approve is refused~~ **Closed by #2474 (DEC-2104, DEC-2129, DEC-2130)** | The responses route judges an `Approved` grant's step-up and commits it: `granted` asks `app.step_up.asserted` for the `approve` action bound to the approval's content hash and commits `Command::Grant` (api-server/src/writes.rs:545-600); a grant without step-up is `step_up_required` and a stale one `step_up_stale`, and nothing is recorded. Web's cli_confirm approve (#2435) is accepted by both the server and the mock API | — | L1 | — |
| X7 | ~~Exits never reach the executor~~ **Closed by DEC-2387** | Only the owner's exit of one instrument was unwired (see "X7 by path" below). The runtime now journals one `IntentProposed` per sell leg, caused by the `OwnerExitRequested` copy, which executord reads (follow.rs `proposals`, `confirmed`); `fixtures/contracts/owner-exit-agent-stream.json` pins the rows both processes read. Kill switches, discretionary and builder risk exits already reached it | — | EX | — |
| X8 | ~~Alerts are dropped~~ **Closed by DEC-2514 and DEC-2516** | Every core alert is a closed `OwnerAlertKind` the executor and the runtime journal as `OwnerAlertSent` in the subject's own batch (#2555, DEC-2514; see "X8 by path"). The dispatcher runs in the cell (`mandate-dispatcher`, processes.json `dispatcher`, deploy/cell/images/dispatcher.Dockerfile, templates/dispatcher.yaml): one per workspace, the single writer of `ntf:{workspace}`, following every agent and account stream its control stream names, and mailing only through the recorded transport, which sends nothing; web push is off (DEC-2504) | — | EX | — |
| X9 | ~~Deployed event ids are not unique across streams~~ **Closed by #2504** (`stream_event_id`, DEC-2389); a second demo run on one cell passes its connect check | executord's `fixture::Ids` is `1`, epoch, head, ordinal (executord/src/fixture.rs:18-26); the daemon's is `0`, epoch, head, ordinal (daemon/src/fixture.rs:20-24). The journal requires a globally unique `event_id` (journal spec, envelope table: "ULID; globally unique"). Every account stream's first batch therefore has the same ids, and so does every agent stream's. In one cell journal only the first agent's runtime can open its stream, and only the first account's connect check commits. Reproduced: a second demo run on one cell, connect check `append_refused`. The trade loop's agent is the second | — | DR | — |
| X10 | **The executor's account-stream records have no journal schema** | The executor writes `KillSwitchActivated` (kill.rs:69, :151), `ExternalActivityIngested` (orders.rs:645; reconcile.rs:200), `RejectObserved` (orders.rs:989) and `OrderAbandoned` (intent.rs:777) on the account stream. `mandate-journal` registers none of them there (`control::governs`, control.rs:126-135; `schema::payload_schema`, schema.rs:197-208), so the append is refused `UnknownSchema` at `payload` and the executor fails its cycle and restarts. Seen in the cell on the agent kill switch: `drafts ["AgentModeApplied", "KillSwitchActivated"]` | 4, 6, 7 | L1 | M |
| X11 | **No fresh output outside a weekday session** | ma_crossover's horizon is the pinned `max_output_age_s` (modelhost ma_crossover.rs:176), capped at 86400 (mandate.schema.json:657), and freshness needs `now < expires_at` (mandate spec §8.2). With daily closes, a Monday and the day after a holiday have no fresh output, and openings trade only in the regular session. The conservative reading stands; reading freshness in sessions is the founder's call (DEC-79), queued by the coordinator | 3 | founder | — |
| X12 | ~~The runtimes exhaust the journal's connections~~ **Closed by #2545**: each `PgStore` held up to sqlx's default ten connections under `block_on`, and four runtimes took the cell journal's 100 slots, so the next `cell up`'s migration was refused; it now holds one | — | — | DR | — |

## X7 by path (lane EX, main at 4e98608)

The journal is the only channel from the runtime to the executor in the cell (design §3.1); the daemon drops every `Effect::Intent` (daemon/src/runtime.rs:564) by design. Each exit therefore reaches the executor only through a record executord reads (follow.rs).

| Exit | Runtime writes | Executor reads | Reaches the executor? |
|---|---|---|---|
| Owner's exit of one instrument | `OwnerExitRequested` (escalation.rs:978), plus a `Flatten` handoff the daemon drops | `IntentProposed` only (follow.rs:`proposals`); it never reads an instrument-scope `OwnerExitRequested` as an order | **no**: fixed by DEC-2387, one `IntentProposed` per sell leg caused by the copy |
| Kill switch (any scope reaching the agent) | `KillSwitchActivated` (step.rs:`switched`) | `switches` (follow.rs:359), applied as the core's own agent-scoped switch (process.rs:`apply`, kill.rs:37): cancel by id, confirm, sell the sub-ledger | yes, once the executor has started (X1) |
| Discretionary exit (signal, goal, removed instrument) | `DecisionMade` and `IntentProposed` (step.rs:607) | `proposals`, handed whenever a risk exit is permitted (process.rs:362-371) | yes, once the plan proposes (X2, X4) |
| Builder risk exit (`trim_to_target`) and an approved exit | `IntentProposed` (step.rs:607, escalation.rs:722) | `proposals` | yes, as above |
| Executor-originated risk exits (watchdog, unprotected bound, drawdown flatten) | none: the executor originates them | its own state | in the executor already; the drawdown ladder is not yet called (risk_state.rs:7-9, lane RK) |

Once handed, the core runs trading spec §5.4's marketable exit sequence (cancel protection, confirm, re-gate, submit, priced by §5.6), and cancels the agent's resting opening buys first (§5.3 rule 5). The in-process path (mandate-paper) had the same hole: the core refuses every `Flatten` handoff, so DEC-2387 hands the legs as orders there too.

## X8 by path (lane EX)

The notifications spec's design (§5.1, §5.5, §5.9): each stream's owner writes `OwnerAlertSent` in the subject's own batch (journal spec §9.15, rules 119, 120 and 130), and a dispatcher process tails the journal and journals `NoticeIssued` and `NoticeAttempted` on `ntf:{workspace}`. The `ApprovalRequested` cause is already journaled.

| Piece | State | Gap |
|---|---|---|
| Executor owner alerts (`batch.notify`: protection, exit_stalled, reconciliation, agent_held, kill_switch keys) | journaled | `OwnerAlertSent` in the subject's batch, under its closed kind (#2555, DEC-2514) |
| Runtime owner alerts (step.rs:521, research.rs:1268-1280) | journaled | `RuntimeAlert` drafts `OwnerAlertSent` in the agent stream's batch (#2555) |
| `OwnerAlertSent` writers | every core alert (#2555) | the user kill switch's one notice (E8-9k), an account restriction (E7-2u), and cancel retries exhausted (E7-2x), each a backlog row |
| Operator alerts (`journal_unavailable`, `opening_held`, `exit_held`, `flatten_unplanned`) | process-only | infrastructure alerts (`mandate-alerts`, E21-39), not owner notices |
| Dispatcher | binary, image, `processes.json` entry, journal role, chart values and its configuration file (DEC-2500, DEC-2516) | it mails only through the recorded transport; the founder's SMTP transport and web push join with their adapters (DEC-2511, DEC-2504) |

Plan: (1) the processes write `OwnerAlertSent` for each core alert in its own batch, mapped by spec §3.2's closed key-to-kind table; (2) the dispatcher binary in the cell over the journal, sending only through the recorded transports and the pull channels.

## Step 1: the owner creates an agent in web (envelope, confirm, deploy)

Today: the CLI path runs end to end in the cell (CI 40281337291, step 2). From web/, a version can be created, but confirm and deploy are not sent.

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| `/agents/new` over the API | screen yes, create no | The real set-up screen over the API (#2533), with a no-model note; its create step sends nothing until `GET /instruments` and `GET /signal-models` land (spec #2535, then the server) | WB | S |
| Create the version | yes | web/src/lib/api-runtime.tsx, then http.rs:196, then version_routes.rs:339 | WB | — |
| Confirm | server yes, web no | The server takes passkey step-up (version_routes.rs:302-314; handler :529); web sends `step_up: null` (web/src/api/commands.ts:138; web/src/lib/api-runtime.tsx:407-430) | WB + L1 (#2475, deploy `step_up_assertion`) | M |
| Deploy, `POST /agents` | server yes, web no | version_routes.rs:769; web is `not_sent` (web/src/lib/api-runtime.tsx:432-443) | WB | S |
| The connection (Alpaca paper) | yes | web connects with cli_confirm (api-screens/connections.tsx; web/src/lib/api-runtime.tsx:285-300; #2453). Staging `alpacaOAuth.clientId` is still "" (deploy/gitops/clusters/staging/workspace.yaml:40): the founder registers the app | founder | — |
| Reviewed configuration registered | yes (cell) | Step 2 registers the reviewed `rule_set`, `fee_config` and `trading_calendar` before any version: one rule set for both gates (#2495) | DR | — |
| Runtime and executor started after deploy | yes | CI 40281337291: "the account's executor … answers livez" | — | — |

## Step 2: autonomous research (models through the gateway, agentic harness, ideas and theses)

Today: no real model is called anywhere on main. In the cell, the research agent's runtime is not started. These rows are unchanged since 517ee19 except where noted.

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Research loop, judge, theses | library yes | runtime/src/research.rs; research/agentic.rs; judge.rs | AH / I2 | — |
| Daemon `--research` on the recorded bundle | yes (fake) | runtime-models/src/main.rs; worker.rs RecordedTransport | I2 | — |
| A real model call; model-gateway service | no | gateway-http "not wired … `NoKeys`"; `mandate-gatewayd` has no bin | GW | L |
| Research agent's runtime in the cell | no | CI step 3: deployed, but at version 1, with no connection of its own, so the manager starts nothing | DR | S |
| Event-triggered research (E19-12) | no | `RunStarted.trigger` is always `schedule` | NE | L |

## Step 3: keeps deciding until it buys (signals, builder, gate, autonomy, executor, broker)

Today: in the cell, the runtime evaluates the crossover and, with a fresh output, asks (X2, X4 closed); nothing fills yet (X5), and on a Monday no output is fresh (X11).

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Model outputs into the runtime | deployed yes, in the cell | X2, DEC-2383 item 7 | MC | — |
| Plan and sizing over the journaled fold | deployed yes | production/plan.rs:75-108 (#2471) | D1 | — |
| Gate dry run | deployed, asks on deferring openings | #2538 | D1 | — |
| Autonomy: ask, `ApprovalRequested` | deployed yes, not yet seen in the cell (X11) | `Plan::classify` never auto (plan.rs:90-133); escalation.rs:288 | D1 | — |
| Runtime to executor handoff | yes | executord reads `IntentProposed` (follow.rs:463-499) | — | — |
| Executor opening, fill, protective stop | fill yes, opening no | X5 | X / LV | L |
| Broker: Robinhood (rh-sim) | no | #2434 waits on the founder's three DEC-2382 answers | founder | — |

## Step 4: monitors the position (marks, protection, risk)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Executor start, reconciliation, readiness | yes | ticks arrive (X1); CI step 2's executor is live | — | — |
| Marks in the journal | writer yes, deployed no | X3 (#2449, `NoMarks` in the fixture wiring) | X | S |
| Protection, OCO, watchdog | library yes | NoProtection (X5) | X / LV | L |
| Drawdown ladder, daily loss | library yes | not called deployed | RK | L |
| Owner sees positions | agent page yes | The agent page reads orders, positions and the journal from the API (web/src/lib/api-runtime.tsx:320-345; api-screens.tsx:279-335; #2459). The `/positions` screen is mock (positions-screen.tsx:6), and the API returns null cost basis, risk mark and protection (reads.rs) | WB | M |

## Step 5: acts on news and events

Unchanged since 517ee19: news tools exist on the recorded plane only, live news waits on the founder (DEC-1130 item 2), and E19-12 is lane NE's.

## Step 6: decides to sell (signal, goal, thesis, risk exit)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Agent flatten over the journaled fold | deployed yes | production.rs `Flatten` over the agent's own fills (#2447); an owner exit's legs are journaled as `IntentProposed` (DEC-2387); a kill switch's flatten is planned by the executor itself | — | — |
| Signal and goal exits | library yes | X2, X4 | D1 + DR | — |
| Thesis exit | no | the runtime fold ignores `ThesisInvalidated` (state.rs:758) | NE | L |
| Owner exit | journaled as its market sells | DEC-2387; sold once the executor holds the position (X5) | — | — |

## Step 7: the owner sees everything in web, and approves, intervenes or kills

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Approve | used | a `cli_confirm` paper grant, recorded by the server (#2474, X6) | L1 | — |
| Kill switch | runtime yes, executor no | web's and the CLI's are recorded, and the runtime applies the CLI's; the executor cannot journal its copy (X10) | L1 | M |
| Orders, positions, journal reads | used | the agent page (#2459); `/v1/me/session` by step-up (web/src/lib/api-runtime.tsx:272) | WB | — |
| Screens still on `ApiNotYetNotice` | — | positions, audit, alerts, messages, settings | WB | M |
| Resume, release, mandate change | no | unchanged | WB + L1 | M–L |

## Running the cell from a cloud session

Four things this sandbox needs, which CI does not:
1. `MANDATE_CELL_CA_BUNDLE=/root/.ccr/ca-bundle.crt`, because the egress proxy inspects TLS.
2. `MANDATE_CELL_RESTRICT_OOM=1`, because the VM refuses a negative `oom_score_adj`, so no pod sandbox starts without it (#2497).
3. `MANDATE_CELL_LOW_EVICTION=1`, because the session's disk allowance sits under the kubelet's default hard-eviction thresholds, so the node takes a disk-pressure taint and evicts the cell's pods while the disk still has room. It lowers `nodefs` and `imagefs` eviction to 1% on the cluster `cell up` creates (`cargo xtask cell down` first if one is running). Default off; cloud sessions only.
4. A host `kubectl`. Without one, the scenario never points the API and the manager at the run's workspace, and the manager starts no executor. One way to get it: `docker cp k3d-mandate-cell-server-0:/bin/k3s /usr/local/bin/k3s && ln -s k3s /usr/local/bin/kubectl`.

A second demo run on one cell now passes its connect check (X9 closed), and the runtimes no longer exhaust the journal's connections (X12 closed). Keep ample disk free: without item 3, the kubelet evicted this sandbox's pods at about 13.5 GB free. `cargo xtask ci pending` over a journal crate selects most of the workspace and does not fit beside a running cell here.

## Founder calls on the path

- DEC-2382: Robinhood's three open questions.
- The staging Alpaca OAuth app (DEC-2636).
- Alpaca news (DEC-1130 item 2).
- Whether a signal model's freshness is read in trading sessions (X11).
