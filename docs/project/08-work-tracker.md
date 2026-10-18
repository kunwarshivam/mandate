# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document, one page. Updated at the end of every working session |
| **Last updated** | 2026-10-18 evening: a coordinator session landed the CI unblocks (#1587, #1595), #1501, #1588 and #1590 merged, and #1539, #1591, #1592 and #1597 approved, and restarted eleven builder lanes; the M5 to M7 and intelligence rows below were re-read against `main` the same day |

Where the project stands, what is waiting on whom, and what comes next, on one page. How the work
ran, with every PR number, lives in [11-work-log.md](11-work-log.md). Plans live in
[02-milestones-and-wbs.md](02-milestones-and-wbs.md) and [06-backlog-v1.md](06-backlog-v1.md);
decisions in [decisions/](decisions/README.md) and, before DEC-344, [04-decision-log.md](04-decision-log.md).
Open PRs and claim issues on GitHub are the live record of who holds what.

## Where we are

| Milestone | State | What is left |
|---|---|---|
| Tier 1 specs | Approved (DEC-71), amended since: trading domain v0.14, journal v0.13, mandate v0.6 with its amendments | — |
| M0 Foundations | Done | — |
| M1 Market data | Done, exit run 2026-09-26 | — |
| M2 Accounting | Done | RC-18's main path (see Reference cases) |
| M3 Simulated execution and backtest | Done | The backtest runner's remaining pieces (DEC-127 items 15, 24, 25) have no story row yet |
| M4 Journal | In progress: core, Postgres, artifacts, verification CLI, cold store and its checks, E5-8 `verify-cold` (#654, #662) merged; E7-19's artifact-aware Postgres append in tests (#676, DEC-510) | E5-5 the personal-data vault; E5-7 and E5-9 the cold store's operational half and examination bundle; all safety-critical; DEC-265 item 1 waits on the founder |
| M5 Agent runtime and risk | In progress: the runtime and kill switches, the mandate document (validation, policy, change classification), the risk state and ladder, the gate with the US account rules, eligibility, conduct controls and restrictions, autonomy and the order builder, delegations, the client ceiling, the review date, V-047. **Tripwires (E6-13) are done**: #613 tests, #618 implementation, #621 status; MC-W01 to MC-W57 and MC-J01 to MC-J10 pass | **E6-15** (lane TW, claim #1599): the account stream's risk state, tripwires and the ladder latch, binds only in the harness today; no runtime or shell gate reads it, and §9.3 closes none of its seven records. Slices A (journal spec and registration) and B (a pure, fail-closed `AccountRiskFold`) are off R0's path; slice C wires the shell gate and the live door, timing is the founder's (Waiting on the founder) |
| M6 Alpaca connector (paper) and recovery | Code complete for paper, and every first-live-trade code row in the brief is merged (S2 #1349, B3 #1426, K1b #1346, B2b #1084/#1085, C1 to C4, G1a, G1b, the R0 gap rows R1 to R22; lane LV's map, 2026-10-18). The R0 rehearsal against `mandate-rh-sim` passes 1a to 2t; 2u expects today's not-flat refusal (#1518, #1524) | **First paper trade**: the founder's run (E2). **First live trade** (DEC-529, target 2026-10-23): the founder's R0 run and DEC-529 live-door items; R23 (DEC-1152 item 4) is the founder's; DEC-1152's open items: (a) a Robinhood sell's unpublished fill price, DEC-2300 Proposed (#1598), (b) `mandate agent exit`, lane M7's S1, (c) a fresh connection record per entry, by design. E6-15 slice C (M5) is the founder's timing call |
| M7 Escalation v0 | In progress: `mandate-approval` (E8-1 to E8-3), the runtime's approval path, owner commands, the MC-E lifecycle driver; MC-E01, E06, E17 to E24 and E29 pass; `approvals list/show/approve/skip` and `agent deploy/pause/kill` are wired in the CLI | Lane M7 (claim #1594): S1 `agent status`, `resume`, `stop` and `exit` in the CLI; S2 `agent acknowledge`, which needs `OwnerAcknowledged`'s closed schema (journal spec first); S3 to S5 E8-11's email adapter on a recorded-fixture transport and a founder-only SMTP transport tested on a loopback fake. Chat (E8-12) stays deferred (DEC-824) |
| M9 Web app (started early, DEC-200) | On fixtures: the shell, Home, agents, approvals, Messages and the copilot, the set-up chat, sign-in, the landing page (its windows move as a Mac's, its name the product's logo, its buttons grey: DEC-903; a switch between Windows 98 and a System 7 desktop, Windows the default until the Mac is finished: DEC-904; the app's own screens in a second browser tab, a painting per visit, and desktop motion: DEC-905; the app itself live in front, driven by the browser, owls with their own minds, and no explaining copy: DEC-906); the design plan's first five decisions (DEC-511 to DEC-515) on #669; the gate decision record is a ledger that fits the first screen (2026-10-17) | `web/design/plan.md`, every unticked item, in its order; then the connection to a deployment; after the first paper trade, the founder's Home (signed-in, D1), holdings, search and watchlist stories (E11-10, E11-11; [DEC-528](decisions/DEC-528.md), which also places E10-19 and E19-12 later and defers adoption) |
| M8 and M9's demo lanes (DEC-820, demo about 2026-10-26) | In progress in five lanes (below): identity spec v0.3, the authn, identity and passkey crates' tests and the passkey implementation; the workspace API contract and schemas; journal spec to v0.21; notifications spec v0.2 and its crates' tests; the web client foundation; the audit read contracts | L1 E9-1 sessions, E9-2 I1 implementation, E9-4 step-up, E9-7; L2 the journal spec queue (after v0.21), E10-13, E10-10 implementation, E10-7, E7-17 (fold tests and DEC-687 merged, the tests' correction in review, then the fold implementation, then the state-machine slice); L3 the E8-9 and E8-14 implementations; L4 the E11-9 slices; L5 E12-6, waiting on E9-2's `authorize` (identity ID-8) |
| M8, M10 to M13 | Planned; the design layer drafted (DEC-431 to DEC-443) | ADR-0003's code stories (E8-8, E10-7 to E10-9, E11-4 to E11-8, E12-5) |
| Intelligence layer, lane I2 (research in the runtime, demo 2026-10-26) | On `main`: `mandate-research-run`, the runtime's `handle_research` (DEC-996), the loop through `mandate-gateway` over its recorded fake (DEC-1120), §9.19's registration (DEC-2201), E19-5 (#1543, #1559; DEC-2202, DEC-2205), `mandate-research-driver` (DEC-2204) and the daemon's research hook, deny-all by default (#1575, DEC-2208) | Lane I2 (claim #1531, DEC-2210): 4a the cell's research source, worker and sink in `mandate-daemon`, failing closed; 4b an explicit opt-in on a resolved paper deployment; 4c demo step 3 checks the run, call and thesis records. OpenRouter is never selected in a cell build |
| Intelligence layer, lane AH (agentic research loop, demo 2026-10-26) | The founder decided on 2026-10-17 that the research harness is agentic ([ADR-0005](../adr/0005-agentic-harness.md), [DEC-2203](decisions/DEC-2203.md), E19-13, claim #1534). On `main`: the specs (#1540), the gateway's tool calls (#1551), slice 3a (#1566), the run records (#1561) and I2's core side (#1569, DEC-2206) | Lane AH (DEC-2209): 3b a pure read and think dispatch, 3c the turn-to-gateway request builder, 3d the memory, records and mandate reads, 3e recorded multi-turn scenarios and E19-6's adversarial bench; each a DEC-77 pair. Nothing depends on OpenRouter or DEC-997 |
| The demo end to end, lane E2E (E21-54, demo 2026-10-26) | Slices 1 (#1538, DEC-2002) and 2 (#1568) merged: `cargo xtask cell demo --scenario demo` runs the six steps on the local cell and verifies the control stream; CI's `cell-demo` job runs it. Sign-in: web/'s dev-cell sign-in is on `main` (#1588); the stub issuer (#1591) and web/ against the cell (#1597) are approved and merging | Slice 3 (#1600): each step reads ready, partial or skipped with the lanes it waits on (today: 1, 2 and 4 partial; 3, 5 and 6 skipped). Then step 6's kill switch through the CLI, step 1's signed-in read, step 5's backtest (the runner is in the image, #1581), and the rest as lanes land: L1's step-up, D1's runtime start (waits on lane X's connect check), X's executor, I2 and AH's research. A web deploy of a new agent also needs an agent-creation route (DEC-2101 item 6, lane A1 #1444, no builder) |

**Reference cases.** Journal 46 of 46. Trading domain 13 of 26 cases plus four variants; the rest
wait on the executor stories (E7-2 to E7-5) and the founder's DEC-129 items. Mandate: the MC-W, MC-J
and MC-E cases pass; the rest wait on E8-8 and DEC-444's harness. `crates/mandate-refcases/status.toml`
is the record and outranks this line.

**Open PRs.** GitHub is the live record. Each merges on green CI and an independent review on a
different model (DEC-79), approved by the coordinator under DEC-175.

## Next, in order

1. **The first paper trade.** The paper path's code is complete through E3 and every E2
   prerequisite is verified satisfied; what remains is the founder's run (E2), which waits for
   the founder's explicit confirmation (DEC-450; record the open-orders read after the fill,
   DEC-878).
2. **The first live trade** (DEC-529, target 2026-10-23, deadline 2026-11-02): every code row is
   merged and the agent rehearsal passes; what remains is the founder's R0 run, the DEC-529
   live-door items, R23, and the founder's timing call on E6-15 slice C.
3. **The demo, 2026-10-26** (lane E2E): sign-in (SI, W2), step-up (L1, E9-4 steps 2 and 3), the
   runtime started from the account (D1, DEC-1171, after lane X's connect check), the executor in
   the cell (X, `mandate-executord`), research in the cell (I2, AH).
4. **E6-15** (lane TW): the risk state read by the runtime and the shell gate, slices A and B now.
5. **M7's remainder** (lane M7) and **the backtest and evaluation runs** (lane LV on claim #1463:
   the evaluation run append, then E4-4 slice 2).
6. **Then** L2's workspace API queue, M4's safety-critical half, as before.

Streams that touch different crates run in parallel; reviews and merges run one at a time.

## Waiting on the founder

- **Counsel** on the adviser question (compliance questions 31 to 41) during Phase 0 (DEC-102).
  Nothing trades live until this is answered.
- **Readings left Proposed**, on which agents take the most conservative option meanwhile:
  DEC-104 item 5; DEC-129 items 25 to 27; DEC-250; DEC-265 item 1; DEC-266 item 4; DEC-285
  item 6; DEC-476 item 7 (which model reads the owner's words at `/agents/new`, through which
  route, its spend cap and data terms); DEC-480 (acting from a message).
- **E21-2's long-running process** ([DEC-968](decisions/DEC-968.md), claim #1424): item 9, one
  PR over ES-13's 400 lines instead of DEC-77's two; and item 6's hook in an R0 file (a
  production-cycle constructor that takes its journal stage) before the process can host a real
  cycle.
- **E17-7's allowlist** ([DEC-1130](decisions/DEC-1130.md), claim #1460): item 2, confirming
  Alpaca news (counsel's answer under DEC-433 item 20) in a new allowlist version; until then the
  research agent reads SEC EDGAR filings only.
- **E21-28's lifecycle core** ([DEC-994](decisions/DEC-994.md), claim #1433): item 8, one PR
  over ES-13's 400 lines, as DEC-968 item 9.
- **CI's nightly mutation sweep** ([DEC-1190](decisions/DEC-1190.md) item 5, claim #1481): a
  seventh of the whole-crate sweep each night (about 12 runner-hours), every mutant once a week,
  or the full sweep every night as worded (about 45 to 50 runner-hours on the shared pool;
  `SWEEP_ROTATION = 1`). Meanwhile the seven-run rotation runs.
- **DEC-773** (causal trace authorship): (a) should an owner-selected signal model's output read
  `owner_selected`? (b) What does the label say? Meanwhile every quoted item is `platform_authored`.
- **Decided, record still to land:** DEC-360 (option (c)), DEC-410 item 3, DEC-422 (#514 open).
- **Design calls the plan parks for you:** when the Mac desktop is finished and becomes the default
  (DEC-904 item 5); the short set-up summary (counsel, DEC-477 question 23);
  the phone chart's axis (a canvas option against the no-script-media-query rule).
- **Robinhood:** whether a paper or test path exists for agentic accounts.
- **Contract drift's "released connector version"** (connections spec §8.2 and §9.1): what a
  released version is, who confirms its new pin, and how it clears a drifted connection. Until
  then drift does not clear on that account's stream in this slice: a new connection on the same
  account is a reconnect that continues the stream (journal §9.8 rule 66, DEC-800 item 6).
  Openings stay halted, exits open ([DEC-687](decisions/DEC-687.md) item 3).
- **Decided 2026-10-09:** DEC-833 (route 2 serves both key kinds) and DEC-834 item 2 (route 2's
  limits never refuse a valid key).
- **GitHub Support:** purge the closed PRs' `refs/pull/*/head` refs from before the 2026-10-03
  history rewrite.
- **Spike paper runs** (E17-0): the go-ahead.
- **Dependabot:** six alerts on `main` (one high) to review.
- **Cell #1 and model spend:** [DEC-995](decisions/DEC-995.md) (two DigitalOcean droplets, about
  $104 a month; Proposed, from #1456), [DEC-997](decisions/DEC-997.md) (OpenRouter, a $50 a month
  cap) and [DEC-2504](decisions/DEC-2504.md) (the managed cell's egress to the browsers' push
  services); and, on #1539's branch only, DEC-2001 items 6 (the hosted tier is `dev` until release
  signing) and 7 (`doctl`).
- **E6-15 slice C's timing** (claim #1599): wire the account risk state (tripwires, the ladder
  latch) into the shell gate and the live door before the first live trade, re-running R0, or
  after it, recording today's gate (no drawdown, no latch, `Normal`) as a known limit of the one
  supervised order. Recommendation: before, if slices A and B land by 2026-10-21.
- **DEC-2300** (#1598, Proposed): how a Robinhood sell whose fill price is not published is booked.
  Meanwhile nothing is booked and the agent stays paused.
- **The executor's kill-switch reach** (DEC-2003 item 6, lane X): until E7-4's account and
  workspace scopes land, a kill switch reaches `mandate-executord` only through the runtime's
  copy; a backlog line makes those scopes a prerequisite before the executor runs beyond the dev
  and CI cells.
- **Decided 2026-10-18:** the `Co-authored-by` check is gone ([DEC-2601](decisions/DEC-2601.md)), so
  #1539 lands without a history rewrite; #1501 carries its `wrangler` 4.151.1 registry row.
- **#1582** (the deployment manager in the cell, merged) took option 1: the manager starts the
  runtime, and starts none until DEC-1171's account read lands, so the cell runs no runtime until
  then. Option 2 keeps #1560's static runtime meanwhile, one line in `processes.json`.

Nothing else is blocked on you.

## Open issues

Resolved issues are in the [work log](11-work-log.md). Each row is one change for whoever picks
it up.

| Issue | Owner |
|---|---|
| CI minutes: 3,000 Actions minutes a month (DEC-256); Playwright runs weekly and on dispatch (`web-e2e.yml`); the coordinator dispatches it before merging a web change that moves layout, tokens or the specs | Coordinator |
| A local e2e run against a server already on the port tests the production build with the scenario switch off; let Playwright build `.next-e2e` itself | Next e2e touch |
| A stale or absent ETP classification denies every US-equity opening (DEC-129 item 33); RC-16's harness and the AAPL tracer need a fresh `etp_classified_at` | RC-16 with E6-7; the tracer with E7-7 |
| The pending gate reads a failure's text, not its cause (DEC-137, partly closed) | xtask |
| A crypto fee rate above 10000 bps makes a buy an error, not a credit; reject at load or document | Next accounting story |
| Fee reservations for buying power are the caller's (E6-6 check 7) | E7-3 |
| The accounting fold copies the account on every input; measure before long backtests | E4-2 |
| The backtest runner's dataset adapter, `BacktestRunRecorded`, CLI and crate split have no story row | Write the story before the first owner-facing backtest |
| `NumError::Unimplemented` lives only in `mandate-num::sizing`'s E6-2 stubs | E6-2 |
| `AssetClass` exists in both `mandate-accounting` and `mandate-marketdata`; prices are `DecStr` in market data | The story that creates `mandate-domain` |
| Postgres in the agent environment needs `MANDATE_PG_URL` by hand | E5-3 |
| Branches are named `cursor/...` and `claude/...` where ADR-0001 ES-13 says `agent/...` | Amend ES-13 at the next ADR touch |
| The repository is public (unlimited Actions minutes, the founder's decision of 2026-10-02); commits stay free of personal data and infrastructure details | Founder |
| Stale doc comments on E4-3's and E6-10's harness tests say the check is pending; DEC-77 item 2 kept the implementation PRs from editing them | The next tests change in `mandate-sim` and `mandate-refcases` |
| DEC-316 stays Reserved; mark it Released | Its holder, or the coordinator |
| `Bps::sqrt_impact` returns `too_precise` past 10 decimal places; bound the coefficient's scale or say so | E4-2 |
| `mandate journal verify` reports an unreadable path with no `Refusal` code; `mandate inspect` on a quotes dataset with corporate actions and no quoted side prints no split-adjusted line | Next `mandate-cli` touch |
| The web app's largest chunk is 906 KB and `static/chunks` 4.2 MB; no per-route budget | `web/design/plan.md` G |
| `web-e2e` run 37888126991: the Stop cross-fade test (`stop-visible.spec.ts`, chromium-dark, 1280 px) counted 3 view transitions over 4 hops, so one hop committed without a cross-fade and Stop went untested in a fade there; every per-hop Stop check passed. The count's message now names each hop and its count (#975); the next failure says which hop | L4 |
| `08-work-tracker.md` had lines 7,800 characters long; it is now one page, with the record in the work log, and stays that way | Every session |

## How the work runs

- **Lanes.** The paper and live paths ([paper brief](tasks/first-paper-trade.md),
  [live brief](tasks/first-live-trade.md)), and the demo lanes of DEC-820: L1 identity and tenancy
  (E9-1, E9-2, E9-4, E9-7, E9-8; DEC-640 to DEC-669), L2 workspace API and connections (E10-10 to
  E10-15, E7-11, E7-12, E7-17; DEC-670 to DEC-699), L3 notifications and approvals (E8-9 to E8-16;
  DEC-700 to DEC-729), L4 web on live data (E11-9, E11-1 to E11-3; DEC-730 to DEC-759), L5 audit
  (E12-1 to E12-3, E12-6; DEC-760 to DEC-789). One coordinating session dispatches builders,
  runs the independent reviews and approves; the claim issues and the coordinator log (#165) are
  the record of who holds what.
- Builders follow `.cursor/skills/mandate-mode/` (story, spec-change, correction and ship
  playbooks) and prove work with `.cursor/skills/verify-mandate/`.
- Safety-critical stories ship as the DEC-77 sequence: tests PR, implementation PR (test files only
  lose pending markers), status PR.
- Each PR merges only after green CI and a pass from an independent review agent on a different
  model (DEC-79).
- This file is one page. A session ends by updating the milestone row it touched, the Next list,
  and one dated paragraph in the work log; PR numbers and the story of a change go in the log, the
  PR or the decision file, never here.
