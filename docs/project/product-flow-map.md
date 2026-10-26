# Product-flow map, main at 517ee19 (2026-10-26 ~01:00 UTC), lane DR

How this was checked: code reading and grep on main, plus CI run evidence where cited. Docs alone never count. "Library" means the logic exists and is tested. "Deployed" means a cell process actually runs it. UNVERIFIED marks anything not traced to the end. Sizes: S (<1 day for one agent), M (1–2 days), L (more).

## The cross-cutting blockers (they gate several steps; fix these first)

| # | Blocker | Evidence | Steps it blocks | Lane | Size |
|---|---|---|---|---|---|
| X1 | **No risk clock.** No process writes `ClockAdvanced`: `scheduler` is built=false (processes.json:189-193), there is no scheduler Dockerfile, and values.yaml:36 has `scheduler: ""`. The binary exists (crates/mandate-scheduler/src/main.rs). Without a tick: the executor never hands `Input::Started` (executord/src/process.rs:313-324), so it never reconciles, is never ready and applies no kill switch; the runtime never decides (mandate-runtime/src/step.rs:474-486); research returns early (daemon/src/runtime/research.rs:248-251); approvals never time out (step.rs:491) | 3, 4, 5, 6, 7 | SC / P2 (E21-29) | M |
| X2 | **No signal inputs.** The daemon never hands `Input::ModelOutput` or `Input::Observation`; it hands only Started, Journal, Tick and Research (daemon/src/runtime.rs:376-385). The model host (`mandate-modelhost::evaluate`) runs only in `mandate-paper` (lib.rs:664-690) over a local `--bars` file | 3, 6 | **DR (DEC-2383)** | L |
| X3 | **No bars or marks.** `data` is built=false (processes.json:233-237); `mandate-dataservice` is library only. No deployed process writes `MarkUpdated` (only the shell's tracer, shell/src/tracer.rs:681-724). The executor's quotes are `NoQuotes` on the fixture broker (executord/src/quotes.rs:44-54) | 3, 4, 6, 7 | SC (bars) / E21-44 | L |
| X4 | **The runtime is deny-all.** daemon/src/fixture.rs:26-65 has DenyAll, NoPlan (`classify` = Deny) and NothingHeld, and a fake mandate_version (`:67-77`), used on every path (main.rs:204, 210) | 3, 6 | D1 (DEC-2378; S1 = #2432 open) | L |
| X5 | **The executor knows nothing.** executord/src/fixture.rs:29-101 has NoMandate, NoInstruments, NoFacts, NoProtection and NoQuotes; "This wiring books no fill" (`:106`); the fixture broker never fills (paper.rs:8-9, 239-240) | 3, 4, 6 | X (E21-30 X1b-3) | L |
| X6 | **Step-up.** The passkey verifier is a stub (api-server/src/methods.rs:41 always `StepUpMissing`; `:51,:81` `Unimplemented{E9-4}`). `CliConfirm` accepts only Approve and Connection actions (confirm.rs:75-80). Every approval grant answers 401 before any verifier runs (writes.rs:397-403) | 1, 7 | L1 (E9-4 S4: #2391, #2403) | L |
| X7 | **Exits never reach the executor.** Mapped by lane EX at 4e98608 (see "X7 by path" below): only the owner's exit of one instrument is unwired. The runtime journals its `OwnerExitRequested` copy (escalation.rs:978) and hands an `IntentBody::Flatten` the daemon drops by design (daemon/src/runtime.rs:472), and that the executor's core would refuse anyway (executor/src/intent.rs:182, `Unimplemented E7-4`). No `IntentProposed` is ever written for it, though journal spec §9.1 allows one caused by the copy and executord already prices one from that chain (follow.rs:517-605) | 6, 7 | **EX** (DEC-2387) | M |
| X8 | **Alerts are dropped.** Both binaries discard `step()`'s alerts (executord/src/serve.rs:205; daemon/src/main.rs:256), and no dispatcher process exists (`mandate-dispatcher` is library only) | 4, 7 | **unowned** (E8-10) | S to stop dropping them, M for a dispatcher |

## X7 by path (lane EX, main at 4e98608)

The journal is the only channel from the runtime to the executor in the cell (design §3.1); the daemon drops every `Effect::Intent` (daemon/src/runtime.rs:472) by design. Each exit therefore reaches the executor only through a record executord reads (follow.rs).

| Exit | Runtime writes | Executor reads | Reaches the executor? |
|---|---|---|---|
| Owner's exit of one instrument | `OwnerExitRequested` (escalation.rs:978), plus a `Flatten` handoff the daemon drops | `IntentProposed` only (follow.rs:`proposals`); it never reads an instrument-scope `OwnerExitRequested` as an order | **no**: fixed by DEC-2387, one `IntentProposed` per sell leg caused by the copy |
| Kill switch (any scope reaching the agent) | `KillSwitchActivated` (step.rs:`switched`) | `switches` (follow.rs:359), applied as the core's own agent-scoped switch (process.rs:`apply`, kill.rs:37): cancel by id, confirm, sell the sub-ledger | yes, once the executor has started (X1) |
| Discretionary exit (signal, goal, removed instrument) | `DecisionMade` and `IntentProposed` (step.rs:607) | `proposals`, handed whenever a risk exit is permitted (process.rs:362-371) | yes, once the plan proposes (X2, X4) |
| Builder risk exit (`trim_to_target`) and an approved exit | `IntentProposed` (step.rs:607, escalation.rs:722) | `proposals` | yes, as above |
| Executor-originated risk exits (watchdog, unprotected bound, drawdown flatten) | none: the executor originates them | its own state | in the executor already; the drawdown ladder is not yet called (risk_state.rs:7-9, lane TW) |

Once handed, the core runs trading spec §5.4's marketable exit sequence (cancel protection, confirm, re-gate, submit, priced by §5.6), and cancels the agent's resting opening buys first (§5.3 rule 5). The in-process path (mandate-paper) had the same hole: the core refuses every `Flatten` handoff, so DEC-2387 hands the legs as orders there too.

## Step 1: the owner creates an agent in web (envelope, confirm, deploy)
Today: web can create a version through the API, but no confirm or deploy can succeed. The CLI path works in the cell (cell-demo 981eb90: version create, confirm and deploy all ran; DR-1 makes the deployment v2).

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| `/agents/new` reachable over the API | no | agents/new/page.tsx:9 is gated, so `ApiNotYetNotice` shows (components/screens/common.tsx:218) | same | W | S |
| Drafting the envelope | fixture only | new-agent-flow.tsx:13,26 `fixtureCompiler()` | same; model route UNVERIFIED (DEC-476 item 7 is Proposed) | W (+ founder for the model) | M |
| Create the version | yes | api-runtime.tsx:278-313, then http.rs:194, then version_routes.rs:329 | — | W | — |
| Confirm | no | api-runtime.tsx:316-338 sends `step_up: null`; version_routes.rs:297 answers StepUpRequired | X6 | L1, then W | L |
| Deploy, `POST /agents` | no | Server route at version_routes.rs:750-878; web never sends it (api-runtime.tsx:340-351 is `not_sent`) | X6; the web README's "needs an agent stream" excuse is stale (version_routes.rs:771) | W + L1 | M |
| The connection (Alpaca paper OAuth) | API yes, web no | oauth/start (#2018), callback (#2100); the cell has the oauth fixture and token exchange | No web connect screen (W-4); staging `alpacaOAuth.clientId: ""` (the founder registers the app, DEC-2636); connect start needs step-up (X6) | W, founder | M |
| Runtime started after deploy | yes from the CLI (DR-1, #2430) | connect check, then `connection record`, then AgentDeployed v2, then the manager starts the executor and runtime | the research agent's workspace is still v1 | DR | S |

## Step 2: autonomous research (models through the gateway, agentic harness, ideas and theses)
Today: no real model is called anywhere on main. In the cell the research agent never runs.

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| Research loop (single call and agentic), judge, theses | library yes | runtime/src/research.rs:534-781; research/agentic.rs:127-215; judge.rs:120-141 | — | AH / I2 | — |
| Daemon `--research` on the recorded bundle | yes (fake) | daemon/src/main.rs:175-228; worker.rs RecordedTransport only | other transports are `#[cfg(test)]` (worker.rs:672) | I2 | — |
| A real model call | no | demo bundle host `recorded.invalid`; gateway-http "not wired ... `NoKeys`" (lib.rs:16-19) | — | I2 + V2 (keys) + GO (egress) | L |
| model-gateway service | no | `mandate-gatewayd` has no bin; processes.json:203-207 built=false; E21-62 `--recorded` is only in open #2172/#2173 | — | I2 | M (recorded), L (live) |
| Agentic harness in the demo | no | the demo research.json has no `entry.agentic` or `caps.run_usd`, so it runs single-call | — | AH | S |
| Research agent started in the cell | no | scenario step 3 deploys v1 (no connection) | DR_RESEARCH gap | DR | S |
| Risk clock for research | no | X1 | — | SC/P2 | M |
| Admission into the working universe | partial | mandate-research admit (lib.rs:400-433); daemon fold_research (source.rs:136-240) | judge.rs:220 drops UniverseChanged; executor refuses it (fold.rs:281, E17-3) | I2 / X | M |
| filings.* and market.quote tools | no | recorded.rs implements only news and bars | ports default to Unavailable (research-run/src/ports.rs:84-86,146,166); demo filings.json is `[]` | AH | S each |

## Step 3: keeps deciding until it buys (signals, builder, gate, autonomy, executor, broker)
Today: the library is complete (only `mandate-paper`, the founder's one-shot CLI, runs it end to end through `mandate_shell::production_cycle`). In the cell nothing can propose or fill: X1, X2, X3, X4, X5.

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| quant.ma_crossover evaluated | deployed no | modelhost/src/ma_crossover.rs; only paper/src/lib.rs:680 calls it | X2 | DR (DEC-2383) | L |
| Builder and sizing, gate, autonomy, ApprovalRequested | deployed no | mandate-ports, mandate-risk:852, escalation.rs:288 | X4 (NoPlan classify=Deny, so no ask is ever journaled) | D1 | L |
| Daemon may not link the shell | by design | xtask/layers.toml:776 forbids it, which is why DEC-2378 builds ports over mandate-ports and mandate-risk | daemon is layer 8, the same as modelhost (DEC-503 item 2): DEC-2383 needs a layer change (founder review) | D1 / DR | — |
| Runtime to executor handoff | partial | executor reads IntentProposed (follow.rs:463-490) | none is written under NoPlan | D1 / X | — |
| Executor opening, fill, protective stop | no | X5; protection E7-4 stubs (executor/src/protection.rs:991,1070,2668,3174) | — | X | L |
| Broker: Alpaca paper | staging only | staging `--broker alpaca-paper` (gitops staging workspace.yaml:19); the cell uses the fixture | behind X5 | X | — |
| Broker: Robinhood, rh-sim | no | executord forbids mandate-robinhood (layers.toml:789); #2434 (DEC-2382) open | the CLI records Robinhood only in `live` (connection.rs `requested`): a founder call | X / founder | L |

## Step 4: monitors the position (marks, protection, risk)
Today: none of it runs deployed. The executor never starts (X1) and never holds a position (X5).

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| Executor start, reconciliation, readiness | no | process.rs:313-324, 768-777, 810-841 | X1 | SC/P2, then X | S once ticks exist |
| Quotes | partial | staging AlpacaQuotes REST (credential.rs:423-498), polled only for exits | `quote_wanted` covers exits only (executor/src/state.rs:527-533); `mark: None` (credential.rs:494) | X | S |
| Marks in the journal | no | X3 | risk_state.rs:7-9 "Nothing calls it yet" | TW (E6-15 slice C, the founder's timing) | L |
| Protection, OCO, watchdog | library yes | executor/src/protection.rs:100-125, 1390-1465, 1808-1826 | NoProtection (X5); exit_hold Unimplemented (protection.rs:3161-3174) | X / LV | L |
| Drawdown ladder, daily loss, exits-only | library yes | AccountRiskFold; gate.rs:740 | not called (risk_state.rs:7-9); fold `NotInterpreted E6-4` (executor fold.rs:275-280) | TW | L |
| Breach alerts | no | X8 | — | unowned | M |
| Owner sees positions and P&L | no | API positions return `cost_basis`, `risk_mark` and `protection` as null (reads.rs:420-461) | web positions screen runs on mock-runtime (positions-screen.tsx:6); `/positions` and `/orders` are not in web operations.ts | W | M |

## Step 5: acts on news and events (event-triggered research, news.search/get, thesis updates)
Today: news tools exist on the recorded plane only, and event triggers don't exist.

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| news.search / news.get | recorded only | daemon/src/recorded.rs:250-304 (#2156) | the demo bundle isn't agentic, so it never offers them | AH | S |
| Live news (Alpaca) | no | sources/src/fetch.rs:23 | allowlist-v1.toml `alpaca.news counsel_confirmed=false` (DEC-1130 item 2, the founder's) | founder, then I2 | blocked |
| Live SEC EDGAR | no | fetch.rs:24-25; nothing links mandate-sources | no ingest process | I2 (E17-7) | L |
| Event-triggered runs (E19-12) | no | `RunStarted.trigger` is hard-coded `schedule` (agentic.rs:152-157); runs.rs:424 accepts `event` but nothing emits it | no spec yet (backlog 06:2560-2582; DEC-2204 deferred it) | **unowned** | L |
| ThesisRevised / ThesisInvalidated | library yes | judge.rs:120-141, 200-240; daemon source.rs:152-158 | never reaches the cell (step 2) | I2 / AH | — |

## Step 6: decides to sell (signal, goal, thesis, risk exit)
Today: no sell path runs deployed.

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| Signal and goal exits | library yes | builder.rs:707, 965, 247-334 | X2, X4 | D1 + DR | (in L) |
| Thesis exit | no anywhere | ThesisInvalidated journaled (research.rs:1097), but the runtime fold ignores it (state.rs:758); no UniverseChanged producer | — | I2 / **unowned** | L |
| Risk exits (ladder, flatten_and_pause) | library yes | risk_state.rs | not called; rung C5 and fail-closed B2b-2b (`:549-562`) | TW | L |
| Agent flatten planner | deployed no | mandate_risk::agent_flatten (lib.rs:993) used only by the shell | NothingHeld (daemon fixture.rs:53-65) | D1 (S2) | M |
| Owner exit | journaled, never sold | X7 | — | EX (DEC-2387) | M |
| Exit priced at the confirmed floor | library yes | follow.rs:455-480; executord/tests/owner_exits.rs | never reached (X1, X5, X7) | — | — |

## Step 7: the owner sees everything in web, and approves, intervenes or kills
Today: web reads agents and approvals, and pause, kill switch and approval skip are recorded. Approve, resume, release, owner exit, mandate change, and the journal, positions and orders reads are not available from web.

| Item | Wired? | Evidence | Stub | Lane | Size |
|---|---|---|---|---|---|
| API-backed screens | partial | home, /agents, /agents/[id], /approvals (api-screens.tsx:23-25, 101-332), polled every 5 s | positions, audit, alerts, messages, backtests, connections and settings all show `ApiNotYetNotice` (common.tsx:168-176,218) | W | M |
| Approve | no | X6, writes.rs:397-403; web sends `step_up: null` (commands.ts:80) | — | L1 + W | L |
| Approvals read from agent streams | check | `ApprovalRequested`, `DecisionMade` and `ApprovalResponded` are on `agent:{ws}:{agent}` (D1) | web and `approvals list` must read them there | W / L1 | S to verify |
| Pause | records | stop-sheet.tsx:169, then writes.rs:48 | applied by the runtime: UNVERIFIED | W / D1 | S |
| Resume, release, owner exit, mandate change | no | resume needs step-up (writes.rs:449-475); release returns null (stop/api-record.ts:101); no `owner_exit` in web; `changeMandate` is `not_sent` (api-runtime.tsx:255-263) | hold, acknowledgment and delegation routes are not mounted (writes.rs:3) | W + L1 | M–L |
| Kill switch | records; CLI kill applied by the runtime (DR-1 asserts it; #2433 pins it to the runtime's `KillSwitchActivated` on the agent stream) | writes.rs:69-101; cell-demo-web kill-switch.spec.ts | executor workspace scope `Unimplemented E7-4` (executor/src/kill.rs:49, 138); the executor applies nothing before a tick (X1) | W, X, SC/P2 | M |
| Journal, activity, positions and orders reads | served, unused | http.rs:150-173, journal_reads.rs | no web caller (operations.ts:34-45) | W | M |
| `/v1/me/workspaces` and `/v1/me/session` | served, unused | http.rs:132-139 | web reads `OWLHEAD_WORKSPACE_ID` (data-source.ts:23-50) | W | S |

## Unowned today (need an owner)
1. X7: taken by lane EX (DEC-2387).
2. X8: alerts dropped, and no dispatcher process (S/M).
3. E19-12: event-triggered research, which has no spec (L).
4. Thesis exit end to end (`UniverseChanged` producer, runtime fold of `ThesisInvalidated`) (L).
5. The step 1 drafting model route (DEC-476 item 7 is the founder's).

## Founder calls on the path
DEC-503 item 2 layer change (DEC-2383's daemon to layer 9 or a composer crate); Robinhood's `live`-only rule against rh-sim (DEC-2382); the staging Alpaca OAuth app registration (DEC-2636); Alpaca news (DEC-1130 item 2); E6-15 slice C timing.
