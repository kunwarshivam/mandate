# Product-flow map, main at 430f5ac (2026-10-26 ~16:00 UTC), lane DR

How this was checked: code reading and grep on main, plus cell runs. Docs alone never count. The
refresh at 430f5ac re-read X3, X5, X6 and X10 and the step rows below against main's code, with no
new cell run; the next is the rehearsal at 13:35Z on Tuesday 2026-10-27.

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
2. ~~**The executor restarts on its kill switch.**~~ Closed: the account stream's records have
   their schemas (X10).
3. **The approve.** Served on main and verified through the step-up port (X6); not yet seen
   granted in a cell run, which needs the window in item 1.
4. ~~**The executor holds every opening.**~~ Closed: its binding gate reads the deployment's ports,
   and the fixture broker fills and protects a deployed opening (X5).

## The cross-cutting blockers

| # | Blocker | Evidence on main | Steps | Lane | Size |
|---|---|---|---|---|---|
| X1 | ~~No risk clock~~ **Closed.** The scheduler runs in the cell (#2439) | deploy/cell/charts/journal-db/files/processes.json:189-197 `built: true`; deploy/cell/images/scheduler.Dockerfile; xtask/src/cell.rs:884-891, :982-984. Locally the `scheduler` pod runs. The executor takes the first tick, then `Input::Started` (executord/src/process.rs:322-334); the runtime hands it in `clock_pass` (daemon/src/runtime.rs:415) | — | SC | — |
| X2 | ~~No model outputs in the deployed runtime~~ **Closed by DEC-2383 item 7 (lane MC)** | The daemon's models pass queues `Observation` and `ModelOutput` ahead of each tick (daemon/src/runtime.rs:448-512). `mandate-runtime-models`, the thin layer-9 composer the founder chose, implements `Models` over `mandate_modelhost::evaluate` with the deployed document's pins, the folded `model_version` registrations, the calendar and the closes `mandate_bars::trusted_daily_closes` admits under `MANDATE_BARS`, and the `mandate-daemon` binary, now in that crate, wires it for a resolved deployment (runtime-models/src/main.rs). Only the cell's recording names a source; the closes object must already be in the read-only store (DEC-2385 item 5) | 3, 6 | — | — |
| X3 | ~~Bars yes, marks no~~ **Closed by #2547 and #2624** | The executor marks each deployed instrument, as its deployment reads now or last read (`Marked for Deployment`, executord/src/deployment.rs:579), from its broker's quote asked by symbol (`QuoteFeed for Deployment`, :590), wired over the fixture's `NoMarks` and `NoQuotes` by `..wiring` (:244-251). `data` still appends to no stream, by design: it serves bars and closes read-only (processes.json `data`) | — | X | — |
| X4 | ~~The gate's dry run denies every opening~~ **Closed by #2538 (D1's B2)**: the runtime now asks on an opening whose only open checks are the deferred ones | The deployed runtime uses `production::wired` (runtime-models/src/main.rs). It has a real plan (production/plan.rs), flatten, references and view, and a real gate dry run: `Gate` over `evaluate_deferring` with the registered rule set's gate config (production.rs:43-68; production/gate.rs:69-91; #2489, DEC-2388). That gate defers `UNJOURNALED`, which is eligibility, the liquidity floor, participation and the day-trade budget (gate.rs:26-30), on every opening, and denies any opening with a deferred check. `Plan::classify` never answers auto (plan.rs:90-133), so an opening that passed would ask | — | D1 | — |
| X5 | ~~The executor knows nothing~~ **Closed by #2547, #2581, #2624 and #2641** | executord reads its production ports from the account's deployments, in the cell and in its fixture mode alike (main.rs:192-215, `deployment::production`): mandate, instruments, the binding gate's facts, protections, quotes and marks (deployment.rs:244-251), and the risk fold reads the risk records (#2581). The fixture paper broker fills, books and protects a deployed opening by asset id (#2624), and the cell's trade loop runs on the demo's own deployment (#2641). Not yet seen in a cell run, which waits on a weekday session (X11) | — | X | — |
| X6 | **The approve: served on main, being confirmed in the cell** | The responses route records a grant only once the step-up port verifies it as an `approve` bound to the request's content hash, and answers `StepUpRequired`, `StepUpStale` or the port's refusal otherwise (`granted`, api-server/src/writes.rs:545; #2474, DEC-2104, DEC-2129). The cell's trade loop confirms the browser's code on the host (`mandate step-up confirm`). Not yet seen granted in a cell run (X11) | 1, 4, 7 | L1 | S |
| X7 | ~~Exits never reach the executor~~ **Closed by DEC-2387** | Only the owner's exit of one instrument was unwired (see "X7 by path" below). The runtime now journals one `IntentProposed` per sell leg, caused by the `OwnerExitRequested` copy, which executord reads (follow.rs `proposals`, `confirmed`); `fixtures/contracts/owner-exit-agent-stream.json` pins the rows both processes read. Kill switches, discretionary and builder risk exits already reached it | — | EX | — |
| X8 | ~~Alerts are dropped~~ **Closed by DEC-2514 and DEC-2516** | Every core alert is a closed `OwnerAlertKind` the executor and the runtime journal as `OwnerAlertSent` in the subject's own batch (#2555, DEC-2514; see "X8 by path"). The dispatcher runs in the cell (`mandate-dispatcher`, processes.json `dispatcher`, deploy/cell/images/dispatcher.Dockerfile, templates/dispatcher.yaml): one per workspace, the single writer of `ntf:{workspace}`, following every agent and account stream its control stream names, and mailing only through the recorded transport, which sends nothing; web push is off (DEC-2504) | — | EX | — |
| X9 | ~~Deployed event ids are not unique across streams~~ **Closed by #2504** (`stream_event_id`, DEC-2389); a second demo run on one cell passes its connect check | executord's `fixture::Ids` is `1`, epoch, head, ordinal (executord/src/fixture.rs:18-26); the daemon's is `0`, epoch, head, ordinal (daemon/src/fixture.rs:20-24). The journal requires a globally unique `event_id` (journal spec, envelope table: "ULID; globally unique"). Every account stream's first batch therefore has the same ids, and so does every agent stream's. In one cell journal only the first agent's runtime can open its stream, and only the first account's connect check commits. Reproduced: a second demo run on one cell, connect check `append_refused`. The trade loop's agent is the second | — | DR | — |
| X10 | ~~The executor's account-stream records have no journal schema~~ **Closed by DEC-2803 (#2569, #2577)** | `mandate-journal` registers the account stream's `KillSwitchActivated` (DEC-2139), `ExternalActivityIngested`, `RejectObserved` and `OrderAbandoned` (control.rs:74-128, :1528-1600). #2642 books an unattributed fill in an instrument with no snapshot (DEC-2817), and #2651 binds an abandonment's client order id to its intent (rule 303, DEC-2820) | — | MC | — |
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

Today: in the cell, the runtime evaluates the crossover and, with a fresh output, asks (X2, X4 closed); the executor fills and protects a deployed opening on its fixture broker (X5 closed), and on a Monday no output is fresh (X11).

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Model outputs into the runtime | deployed yes, in the cell | X2, DEC-2383 item 7 | MC | — |
| Plan and sizing over the journaled fold | deployed yes | production/plan.rs:75-108 (#2471) | D1 | — |
| Gate dry run | deployed, asks on deferring openings | #2538 | D1 | — |
| Autonomy: ask, `ApprovalRequested` | deployed yes, not yet seen in the cell (X11) | `Plan::classify` never auto (plan.rs:90-133); escalation.rs:288 | D1 | — |
| Runtime to executor handoff | yes | executord reads `IntentProposed` (follow.rs:463-499) | — | — |
| Executor opening, fill, protective stop | yes, on the fixture paper broker | X5 (#2624, #2641) | X | — |
| Broker: Robinhood (rh-sim) | no | #2434 waits on the founder's three DEC-2382 answers | founder | — |

## Step 4: monitors the position (marks, protection, risk)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Executor start, reconciliation, readiness | yes | ticks arrive (X1); CI step 2's executor is live | — | — |
| Marks in the journal | deployed yes | X3 (#2449, #2547: the deployment's marks over `NoMarks`) | X | — |
| Protection, OCO, watchdog | deployed: the protective stop | the deployment's protections (X5); the fixture broker places the stop as the bracket's legs (#2624, #2641) | X / LV | — |
| Drawdown ladder, daily loss | library yes | not called deployed | RK | L |
| Owner sees positions | yes | The agent page reads orders, positions and the journal from the API (#2459), and `/positions` shows each position's DEC-2138 figures from the API (#2660) with the owner's exit on each of the agent's own (#2645) | WB | — |

## Step 5: acts on news and events

Unchanged since 517ee19: news tools exist on the recorded plane only, live news waits on the founder (DEC-1130 item 2), and E19-12 is lane NE's.

## Step 6: decides to sell (signal, goal, thesis, risk exit)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Agent flatten over the journaled fold | deployed yes | production.rs `Flatten` over the agent's own fills (#2447); an owner exit's legs are journaled as `IntentProposed` (DEC-2387); a kill switch's flatten is planned by the executor itself | — | — |
| Signal and goal exits | library yes | X2, X4 | D1 + DR | — |
| Thesis exit | yes | the runtime fold reads `ThesisInvalidated` (runtime/src/state.rs:799, DEC-2391), end to end through the journal's own append (#2578); in the cell, step 3's research loop reads it as its own link (#2652, #2661) | NE | — |
| Owner exit | yes, from web, journaled as its market sells | DEC-2387; the executor holds the position (X5 closed); `web/`'s positions screen sends it (#2645), and the cell's trade loop proves it as `owner_exit`, scenario step 6 (#2662) | — | — |

## Step 7: the owner sees everything in web, and approves, intervenes or kills

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Approve | web yes, server yes | X6 | L1 | S |
| Kill switch | yes | web's and the CLI's are recorded and the runtime applies them; the executor journals its account-stream copy (X10 closed), and web/'s alerts screen shows it as one alert (#2638) | — | — |
| Orders, positions, journal reads | used | the agent page (#2459); `/v1/me/session` by step-up (web/src/lib/api-runtime.tsx:272) | WB | — |
| Screens still on `ApiNotYetNotice` | none | positions (#2660), alerts (#2638) and the rest read the API; `ApiNotYetNotice` is defined (screens/common.tsx) but no screen uses it | WB | — |
| Resume, release, mandate change | resume and stop yes; release and mandate change no | resume and stop from web with the owner's passkey (#2655, DEC-2140) | WB + L1 | M |

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
