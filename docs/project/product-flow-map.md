# Product-flow map, main at 1349070 (2026-10-26 ~08:00 UTC), lane DR

How this was checked: code reading and grep on main, plus cell runs. Docs alone never count.

- **CI:** main's run 40281337291 at a5f2e0c (`cell-demo-web`).
- **Local:** the cell on 60c26eb plus #2495's registrations.

"Library" means the logic exists and is tested. "Deployed" means a cell process actually runs it. UNVERIFIED marks anything not traced to the end. Sizes: S (<1 day for one agent), M (1–2 days), L (more). The previous map was at 517ee19.

## Where the cell stops today

`cell-demo-web` on main (run 40281337291) ran steps 1 to 3:
- the API's 401, 204 and 200, and web/'s sign-in;
- the connect check, the connection record, and the deploy at version 2;
- the manager's executor, live.

It stopped in **step 4**: the trade loop "stopped before approval", because no approval request ever reached web/. The run was locally reproduced up to step 2 on this sandbox. In order, what stands between the trade loop and a fill:

1. **No ask.** The runtime never proposes an opening:
   - no model output reaches it (X2);
   - the gate's dry run denies every opening while any check is deferred, and four always are (X4).
2. **The approve is refused.** The API answers 401 to every Approved grant (X6).
3. **The second agent's runtime cannot open its stream.** Deployed event ids are not unique across streams (X9). The trade loop runs on the second agent, `WEB_AGENT`.
4. **The executor holds every opening.** Its fixture wiring knows no mandate, instruments or facts (X5).

## The cross-cutting blockers

| # | Blocker | Evidence on main | Steps | Lane | Size |
|---|---|---|---|---|---|
| X1 | ~~No risk clock~~ **Closed.** The scheduler runs in the cell (#2439) | deploy/cell/charts/journal-db/files/processes.json:189-197 `built: true`; deploy/cell/images/scheduler.Dockerfile; xtask/src/cell.rs:884-891, :982-984. Locally the `scheduler` pod runs. The executor takes the first tick, then `Input::Started` (executord/src/process.rs:322-334); the runtime hands it in `clock_pass` (daemon/src/runtime.rs:415) | — | SC | — |
| X2 | **No model outputs in the deployed runtime** | The daemon's models pass is wired: it queues `Observation` and `ModelOutput` ahead of each tick (daemon/src/runtime.rs:448-512, `ModelsWiring` at :54; #2448, DEC-2383). Deployed, `Wiring::models` is `None` (daemon/src/fixture.rs:106), and nothing outside tests implements `Models` (daemon/tests/runtime.rs:1140) | 3, 6 | DR (S3, the composer) | M, after the founder's DEC-503 item 2 layer call |
| X3 | **Bars yes, marks no** | `data` is built and deployed with SPY and QQQ (processes.json:236-243; values/k3d.yaml:22-30; #2467, #2487), but it appends to no stream (processes.json:237). The executor's `MarkUpdated` v2 writer landed (#2449), but executord's fixture wiring marks nothing (`marked: NoMarks`, executord/src/fixture.rs:102), and its quotes are `NoQuotes` (executord/src/quotes.rs:44) | 3, 4, 6, 7 | X | M |
| X4 | **The gate's dry run denies every opening** | The deployed runtime uses `production::wired` (daemon/src/main.rs:212-218). It has a real plan (production/plan.rs), flatten, references and view, and a real gate dry run: `Gate` over `evaluate_deferring` with the registered rule set's gate config (production.rs:43-68; production/gate.rs:69-91; #2489, DEC-2388). That gate defers `UNJOURNALED`, which is eligibility, the liquidity floor, participation and the day-trade budget (gate.rs:26-30), on every opening, and denies any opening with a deferred check. `Plan::classify` never answers auto (plan.rs:90-133), so an opening that passed would ask | 3, 6 | D1 (DEC-2378 item 11: journal those four facts, then the `DecisionMade` v2 spec) | L |
| X5 | **The executor knows nothing** | The fixture broker fills (`FixturePaper::filling`, executord/src/main.rs:218; paper.rs:201-206; #2468), and executord reads `IntentProposed` (follow.rs:463-499, process.rs:582). But its wiring is still NoMandate, NoInstruments, NoFacts, NoProtection, NoQuotes and NoMarks (executord/src/fixture.rs:29-102), and NoFacts holds every opening at the binding gate (:63-70) | 3, 4, 6 | X (G1b to G2b: #2458, #2463, #2470, #2478) + LV | L |
| X6 | **The approve is refused** | Passkey enrolment and assertion are implemented (api-server/src/passkeys.rs:187; methods.rs:117), and confirm, deploy and connect judge step-up (version_routes.rs:302-314; connect.rs:352). The responses route still answers 401 to every `Approved` grant, `StepUpMissing` with evidence and `StepUpRequired` without (api-server/src/writes.rs:414-419). Web's cli_confirm approve (#2435) is real, but only the mock API accepts it | 1, 4, 7 | L1 (#2469/#2474, DEC-2129) | M |
| X7 | **Exits never reach the executor** | The daemon drops `Effect::Intent` (daemon/src/runtime.rs:564). The core journals `IntentProposed` only for builder proposals and approvals (runtime/src/step.rs:607; escalation.rs:722). Flatten and owner-exit legs are only `Effect::Intent` (step.rs:121, :942), and an owner exit only `OwnerExitRequested` (escalation.rs:978) | 6, 7 | EX (session started 06:26) | M |
| X8 | **Alerts are dropped** | The daemon prints each alert to stderr (daemon/src/main.rs:266-267); executord discards them (serve.rs:205). `mandate-dispatcher` has no binary and no process | 4, 7 | EX | S/M |
| X9 | **NEW: deployed event ids are not unique across streams** | executord's `fixture::Ids` is `1`, epoch, head, ordinal (executord/src/fixture.rs:18-26); the daemon's is `0`, epoch, head, ordinal (daemon/src/fixture.rs:20-24). The journal requires a globally unique `event_id` (journal spec, envelope table: "ULID; globally unique"). Every account stream's first batch therefore has the same ids, and so does every agent stream's. In one cell journal only the first agent's runtime can open its stream, and only the first account's connect check commits. Reproduced: a second demo run on one cell, connect check `append_refused`. The trade loop's agent is the second | 4, 7 | unowned (X and D1 crates) | S |

## Step 1: the owner creates an agent in web (envelope, confirm, deploy)

Today: the CLI path runs end to end in the cell (CI 40281337291, step 2). From web/, a version can be created, but confirm and deploy are not sent.

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| `/agents/new` over the API | no | page.tsx wraps `WorkspaceGate` without `api`, so `ApiNotYetNotice` shows (components/screens/common.tsx:212-218); the compiler is the fixture one (web/src/components/new-agent/new-agent-flow.tsx:13) | W | S |
| Create the version | yes | web/src/lib/api-runtime.tsx, then http.rs:196, then version_routes.rs:339 | W | — |
| Confirm | server yes, web no | The server takes passkey step-up (version_routes.rs:302-314; handler :529); web sends `step_up: null` (web/src/api/commands.ts:138; web/src/lib/api-runtime.tsx:407-430) | W + L1 (#2475 waits on DEC-2131's slices) | M |
| Deploy, `POST /agents` | server yes, web no | version_routes.rs:769; web is `not_sent` (web/src/lib/api-runtime.tsx:432-443) | W | S |
| The connection (Alpaca paper) | yes | web connects with cli_confirm (api-screens/connections.tsx; web/src/lib/api-runtime.tsx:285-300; #2453). Staging `alpacaOAuth.clientId` is still "" (deploy/gitops/clusters/staging/workspace.yaml:40): the founder registers the app | founder | — |
| Reviewed configuration registered | yes (cell) | Step 2 registers the reviewed `rule_set`, `fee_config` and `trading_calendar` before any version: one rule set for both gates (#2495) | DR | — |
| Runtime and executor started after deploy | yes | CI 40281337291: "the account's executor … answers livez" | — | — |

## Step 2: autonomous research (models through the gateway, agentic harness, ideas and theses)

Today: no real model is called anywhere on main. In the cell, the research agent's runtime is not started. These rows are unchanged since 517ee19 except where noted.

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Research loop, judge, theses | library yes | runtime/src/research.rs; research/agentic.rs; judge.rs | AH / I2 | — |
| Daemon `--research` on the recorded bundle | yes (fake) | daemon/src/main.rs:174-187, :218-236; worker.rs RecordedTransport | I2 | — |
| A real model call; model-gateway service | no | gateway-http "not wired … `NoKeys`"; `mandate-gatewayd` has no bin | unowned | L |
| Research agent's runtime in the cell | no | CI step 3: deployed, but at version 1, with no connection of its own, so the manager starts nothing | DR | S |
| Event-triggered research (E19-12) | no | `RunStarted.trigger` is always `schedule` | unowned | L |

## Step 3: keeps deciding until it buys (signals, builder, gate, autonomy, executor, broker)

Today: in the cell, the runtime decides on every tick, but nothing can propose (X2, X4) or fill (X5).

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Model outputs into the runtime | port yes, deployed no | X2 | DR | M |
| Plan and sizing over the journaled fold | deployed yes | production/plan.rs:75-108 (#2471) | D1 | — |
| Gate dry run | deployed, denies openings | X4 | D1 | L |
| Autonomy: ask, `ApprovalRequested` | library yes | `Plan::classify` never auto (plan.rs:90-133); escalation.rs:288 | D1 | — |
| Runtime to executor handoff | yes | executord reads `IntentProposed` (follow.rs:463-499) | — | — |
| Executor opening, fill, protective stop | fill yes, opening no | X5 | X / LV | L |
| Broker: Robinhood (rh-sim) | no | #2434 waits on the founder's three DEC-2382 answers | founder | — |

## Step 4: monitors the position (marks, protection, risk)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Executor start, reconciliation, readiness | yes | ticks arrive (X1); CI step 2's executor is live | — | — |
| Marks in the journal | writer yes, deployed no | X3 (#2449, `NoMarks` in the fixture wiring) | X | S |
| Protection, OCO, watchdog | library yes | NoProtection (X5) | X / LV | L |
| Drawdown ladder, daily loss | library yes | not called deployed | TW | L |
| Owner sees positions | agent page yes | The agent page reads orders, positions and the journal from the API (web/src/lib/api-runtime.tsx:320-345; api-screens.tsx:279-335; #2459). The `/positions` screen is mock (positions-screen.tsx:6), and the API returns null cost basis, risk mark and protection (reads.rs) | W | M |

## Step 5: acts on news and events

Unchanged since 517ee19: news tools exist on the recorded plane only, live news waits on the founder (DEC-1130 item 2), and E19-12 is unowned.

## Step 6: decides to sell (signal, goal, thesis, risk exit)

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Agent flatten over the journaled fold | deployed yes | production.rs `Flatten` over the agent's own fills (#2447); its legs never reach the executor (X7) | EX | M |
| Signal and goal exits | library yes | X2, X4 | D1 + DR | — |
| Thesis exit | no | the runtime fold ignores `ThesisInvalidated` (state.rs:758) | unowned | L |
| Owner exit | journaled, never sold | X7 | EX | M |

## Step 7: the owner sees everything in web, and approves, intervenes or kills

| Item | Wired? | Evidence | Lane | Size |
|---|---|---|---|---|
| Approve | web yes, server no | X6 | L1 | M |
| Kill switch | yes | web's and the CLI's are recorded, and the runtime applies the CLI's (cell steps 4 and 7) | — | — |
| Orders, positions, journal reads | used | the agent page (#2459); `/v1/me/session` by step-up (web/src/lib/api-runtime.tsx:272) | W | — |
| Screens still on `ApiNotYetNotice` | — | positions, audit, alerts, messages, settings | W | M |
| Resume, release, mandate change | no | unchanged | W + L1 | M–L |

## Running the cell from a cloud session

Three things this sandbox needs, which CI does not:
1. `MANDATE_CELL_CA_BUNDLE=/root/.ccr/ca-bundle.crt`, because the egress proxy inspects TLS.
2. `MANDATE_CELL_RESTRICT_OOM=1`, because the VM refuses a negative `oom_score_adj`, so no pod sandbox starts without it (DR's open PR).
3. A host `kubectl`. Without one, the scenario never points the API and the manager at the run's workspace, and the manager starts no executor. One way to get it: `docker cp k3d-mandate-cell-server-0:/bin/k3s /usr/local/bin/k3s && ln -s k3s /usr/local/bin/kubectl`.

A second demo run on one cell fails its connect check until X9 is fixed, so delete the cluster between runs.

## Founder calls on the path

- DEC-503 item 2: the layer change for the model composer (X2).
- DEC-2382: Robinhood's three open questions.
- The staging Alpaca OAuth app (DEC-2636).
- Alpaca news (DEC-1130 item 2).
