# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document, one page. Updated at the end of every working session |
| **Last updated** | 2026-10-28 05:00 UTC: E21-64l reproduced the live canary's cloud-init failure at missing `/run/sshd`; the tested boot-order fix is in progress under claim #2867 while the debug window stays open and `CI_RUNNER` stays unset |

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
| M6 Alpaca connector (paper) and recovery | Code complete for paper, and every first-live-trade code row in the brief is merged (S2 #1349, B3 #1426, K1b #1346, B2b #1084/#1085, C1 to C4, G1a, G1b, the R0 gap rows R1 to R22; lane LV's map, 2026-10-18). The R0 rehearsal against `mandate-rh-sim` passes 1a to 2t; 2u expects today's not-flat refusal (#1518, #1524) | **First paper trade**: the founder's run (E2). **First live trade** (DEC-529, target 2026-10-23): the founder's R0 run and DEC-529 live-door items; R23 (DEC-1152 item 4) is the founder's; DEC-1152's open items: (a) a Robinhood sell's unpublished fill price, DEC-2300 Accepted (#1598, DEC-2605 row 6), booked at the limit once its journal spec PR and executor pairs land, (b) `mandate agent exit`, lane M7's S1 (#1686), (c) a fresh connection record per entry, by design. E6-15 slice C (M5) is the founder's timing call |
| M7 Escalation v0 | In progress: `mandate-approval` (E8-1 to E8-3), the runtime's approval path, owner commands, the MC-E lifecycle driver; MC-E01, E06, E17 to E24 and E29 pass; `approvals list/show/approve/skip` and `agent deploy/pause/kill` are wired in the CLI. Lane M7 (claim #1594) merged `agent status`, `resume`, `stop` and `exit` (#1602, #1686); journal spec §9.28's vectors and `mandate-journal`'s registration of the acknowledgment and its request (#1670, #1734, #1765); the email adapter on its recorded transport (#1691, #1715) and the mail guard (#1697, #1716, #1761) | Lane M7: the CLI's `agent request-ack` and `agent acknowledge` (tests first), then S5, the founder-only SMTP transport tested on a loopback fake. A §11 range check for stream rules 240 and 241 has no lane yet. Chat (E8-12) stays deferred (DEC-824) |
| M9 Web app (started early, DEC-200) | On fixtures: the shell, Home, agents, approvals, Messages and the copilot, the set-up chat, sign-in, the landing page (its windows move as a Mac's, its name the product's logo, its buttons grey: DEC-903; a switch between Windows 98 and a System 7 desktop, Windows the default until the Mac is finished: DEC-904; the app's own screens in a second browser tab, a painting per visit, and desktop motion: DEC-905; the app itself live in front, driven by the browser, owls with their own minds, and no explaining copy: DEC-906); the design plan's first five decisions (DEC-511 to DEC-515) on #669; the gate decision record is a ledger that fits the first screen (2026-10-17); the approval request's action is the page title, so the risk sits with the choice (DEC-2409) | `web/design/plan.md`, every unticked item, in its order; then the connection to a deployment; after the first paper trade, the founder's Home (signed-in, D1), holdings, search and watchlist stories (E11-10, E11-11; [DEC-528](decisions/DEC-528.md), which also places E10-19 and E19-12 later and defers adoption) |
| M8 and M9's demo lanes (DEC-820, demo about 2026-10-26) | In progress in five lanes (below): identity spec v0.3, the authn, identity and passkey crates' tests and the passkey implementation; the workspace API contract and schemas; journal spec to v0.21; notifications spec v0.2 and its crates' tests; the web client foundation; the audit read contracts | L1 E9-1 sessions, E9-2 I1 implementation, E9-4 step-up, E9-7; L2 the journal spec queue (after v0.21), E10-13, E10-10 implementation, E10-7, E7-17 (fold tests and DEC-687 merged, the tests' correction in review, then the fold implementation, then the state-machine slice); L3 the E8-9 and E8-14 implementations; L4 the E11-9 slices; L5 E12-6, waiting on E9-2's `authorize` (identity ID-8) |
| M8, M10 to M13 | Planned; the design layer drafted (DEC-431 to DEC-443) | ADR-0003's code stories (E8-8, E10-7 to E10-9, E11-4 to E11-8, E12-5) |
| Intelligence layer, lane I2 (research in the runtime, demo 2026-10-26) | On `main`: `mandate-research-run`, the runtime's `handle_research` (DEC-996), the loop through `mandate-gateway` over its recorded fake (DEC-1120), §9.19's registration (DEC-2201), E19-5 (#1543, #1559; DEC-2202, DEC-2205), `mandate-research-driver` (DEC-2204) and the daemon's research hook, deny-all by default (#1575, DEC-2208) | Lane I2 (claim #1531, DEC-2210): the cell's source, worker and sink, failing closed, and the opt-in on a resolved paper deployment are on `main` (#1753, #1757); the demo bundle in the interim runtime image and every managed runtime pod handed it (#1769, #1774, DEC-2246); the research model's registration from its recorded entry (#1779, #1782, DEC-2247); demo step 3 deploys the research agent alone in `<workspace>r`, and its run waits only on the manager starting runtimes (lane X). OpenRouter is never selected in a cell build |
| Hosted infrastructure, lanes IAC, GO, V2, RS (E21-56, E21-58, E21-32, E21-35; staging by 2026-10-26) | The founder chose OpenTofu, DOKS and GitOps, a $150 a month cap, staging for the demo and Cloudflare R2 with a bucket lock for the journal's archive (DEC-2605 rows 1 to 4, [DEC-2011](decisions/DEC-2011.md)). Merged: lane H's droplet bring-up (#1539); IAC's design and modules (#1639 to #1643: DEC-2012 and ADR-0006, the network and DOKS, the Spaces state bucket and budget, Cloudflare with the R2 archive); GO's Flux reconciliation and the founder's one bootstrap (#1642, #1651), and the hosted charts published, the vault chart among them (#1647, #1681); V2's OpenBao in the cell (#1646), the platform secrets' tests (#1655) and staging's self-unseal (#1662, DEC-2021, row 10). E21-64f's runner hardening merged; its debug canary proved cloud-init stopped before installation because `/run/sshd` was absent (E21-64l, #2867) | Merge E21-64l, then the founder removes the debug window and repeats the one-runner canary and staged smoke before routing CI. Open: IAC's B4 roots (#1644), C1 checks (#1648), C2 `infra.yml` (#1650), D runbook (#1652), E follow-ups (#1654), and F1, G1, G2 for rows 11 to 13 (#1690, #1701, #1703: each run opens staging's Kubernetes API to its own runner for the run, OpenTofu mints the archive's bucket-scoped R2 token, each run mints and deletes its own Spaces key); GO's bootstrap from IAC's workflow (#1695); V2's platform secrets (#1665) and provisioning (#1675 to #1683). Signing: the founder chose a CI key in a protected environment, staging running first (rows 8 and 9); the key is compiled in (#1689) and RS's signing job is #1673 |
| Intelligence layer, lane AH (agentic research loop, demo 2026-10-26) | The founder decided on 2026-10-17 that the research harness is agentic ([ADR-0005](../adr/0005-agentic-harness.md), [DEC-2203](decisions/DEC-2203.md), E19-13, claim #1534). On `main`: the specs (#1540), the gateway's tool calls (#1551), slice 3a (#1566), the run records (#1561) and I2's core side (#1569, DEC-2206) | Lane AH (DEC-2209): 3b a pure read and think dispatch, 3c the turn-to-gateway request builder, 3d the memory, records and mandate reads, 3e recorded multi-turn scenarios and E19-6's adversarial bench; each a DEC-77 pair. Nothing depends on OpenRouter or DEC-997. Then E19-16 under the same claim: the recorded plane serves `news.search` and `news.get` (S1, DEC-2327), the filings reads (S2, DEC-2328), and `market.quote` (S3, DEC-2329), each a DEC-77 pair |
| The demo end to end, lane E2E (E21-54, demo 2026-10-26) | Slices 1 to 11 merged (#1538, #1568, #1600, #1614, #1625, #1674, #1693, #1729, #1777, #1804, #1831; DEC-2002, DEC-2010, DEC-2023), with step 3's research gestures from lanes I2 and D (#1787, #1800). `cargo xtask cell demo --scenario demo` checks, in the cell: each run's own ULID workspace founded for the demo owner, the API pointed at it; the 401 with no session, the owner's sign-in through the stub issuer and the signed-in read of the agents; the confirmed envelope and the agent's deployment; the research agent deployed alone in its own workspace; the backtest run, recorded and read through the API as completed; the owner's exit at a confirmed floor, its code shown then typed (step 6); the agent-scoped kill switch through the CLI and, where web/ can run, from web/ on a second agent (step 7); and the control stream exported and verified. A test refuses a wait that names a slice already on `main` | Named gaps, each waiting on its lane: web/'s gestures (W2); every runtime and executor start, through X's connect check (DEC-1171); step 4's fill and stop, read from the demo account's journal once web/'s trade loop grants the opening, which waits on a run in the regular session (E2E, [DEC-2086](decisions/DEC-2086.md); the production ports ask, DEC-2378, and the executor fills, books and protects, #2624); step-up for the approval (L1); web/ sending the deploy, whose comment that an agent id must already name a stream is stale (#1949), and reading `GET /v1/me/session`, `GET /v1/me/workspaces`, and an agent's orders and positions, all served ([DEC-2111](decisions/DEC-2111.md), [DEC-2113](decisions/DEC-2113.md); W2, once the backend is done) |

**Reference cases.** Journal 46 of 46. Trading domain 13 of 26 cases plus four variants; the rest
wait on the executor stories (E7-2 to E7-5) and the founder's DEC-129 items. Mandate: the MC-W, MC-J
and MC-E cases pass; the rest wait on E8-8 and DEC-444's harness. `crates/mandate-refcases/status.toml`
is the record and outranks this line.

**Open PRs.** GitHub is the live record. Each merges on an independent review on a different
model (DEC-79), approved by the coordinator under DEC-175, which the founder relaxed (DEC-2605
row 14): an approved PR merges right after its independent review, without waiting on `fast`,
`full` or `web`. `main`'s own CI then shows a break, and the lane that caused it fixes `main`.

## Next, in order

1. **E21-64l's CI runner boot fix.** Merge the tested `/run/sshd` ordering correction, then the
   founder removes the debug window, repeats one runner and its three-job smoke, scales to three,
   repeats the smoke and only then sets `CI_RUNNER`.
2. **The product flow.** See [the handover](handover-2026-10-26.md) and [the product-flow
   map](product-flow-map.md): the founder's whole autonomous agent lifecycle on the managed cell,
   blockers X1–X8, owners, and every founder decision of 2026-10-25/26.
3. **The first live trade** (DEC-529, target 2026-10-23, deadline 2026-11-02): every code row is
   merged and the agent rehearsal passes; what remains is the founder's R0 run, the DEC-529
   live-door items, R23, and the founder's timing call on E6-15 slice C.
4. **The demo, 2026-10-26**: staging up on DOKS (IAC, GO, V2, RS); in the scenario, web/'s kill
   switch (W2, #1666), step-up (L1, #1698), the runtime started from the account (D1, #1704), the
   executor in the cell (X, #1694, #1702), research in the cell (I2, AH).
5. **E6-15** (lane TW): slice B2b-1 landed (#2007, the fold reads version-2 marks and binds to one
   instrument); what remains is the runtime and shell gate reading it, slice C, whose timing is the
   founder's.
6. **M7's remainder** (lane M7) and **the backtest and evaluation runs** (lane LV on claim #1463:
   the evaluation run append, then E4-4 slice 2).
7. **The lanes whose tests halves landed 2026-10-21** pick up their implementations: E21-62's
   framed exchange and gateway key are whole (#2038, #2047); E21-30's executor start is whole
   (#2008, #2009); E9-4 S3.1 (#2043) and E19-13 slice 3e (#2050) need their implementations, and
   E9-1 route 2's S0 spec (#2041) needs its routes.
8. **Then** L2's workspace API queue, M4's safety-critical half, as before.

Streams that touch different crates run in parallel; reviews and merges run one at a time.

## Waiting on the founder

- **Counsel** on the adviser question (compliance questions 31 to 41) during Phase 0 (DEC-102).
  Nothing trades live until this is answered.
- **Readings left Proposed**, on which agents take the most conservative option meanwhile:
  DEC-104 item 5; DEC-129 items 25 to 27; DEC-250; DEC-265 item 1; DEC-266 item 4; DEC-285
  item 6; DEC-476 item 7 (which model reads the owner's words at `/agents/new`, through which
  route, its spend cap and data terms); DEC-480 (acting from a message). The founder accepted
  [DEC-2355](decisions/DEC-2355.md) on 2026-10-25 (alert once per resting set; lane LV implements
  it).
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
- **Model spend and push egress:** [DEC-997](decisions/DEC-997.md) (OpenRouter, a $50 a month
  cap) and [DEC-2504](decisions/DEC-2504.md) (the managed cell's egress to the browsers' push
  services). The hosting, budget and signing questions (DEC-995, DEC-2001 items 6 and 7, DEC-971
  item 7) were answered by DEC-2605 rows 1 to 4, 8 and 9; their lanes amend those DECs to cite
  the rows.
- **E6-15 slice C's timing** (claim #1599): wire the account risk state (tripwires, the ladder
  latch) into the shell gate and the live door before the first live trade, re-running R0, or
  after it, recording today's gate (no drawdown, no latch, `Normal`) as a known limit of the one
  supervised order. Recommendation: before, if slices A and B land by 2026-10-21.
- **The executor's kill-switch reach** (DEC-2003 item 6, lane X): until E7-4's account and
  workspace scopes land, a kill switch reaches `mandate-executord` only through the runtime's
  copy; a backlog line makes those scopes a prerequisite before the executor runs beyond the dev
  and CI cells.
- **Decided 2026-10-18:** the `Co-authored-by` check is gone ([DEC-2601](decisions/DEC-2601.md));
  and the fourteen rows of [DEC-2605](decisions/DEC-2605.md): hosting, budget, the demo on staging,
  the archive (rows 1 to 4), `cli_confirm` step-up for paper grants (row 5, DEC-2103 item 7), a
  Robinhood sell booked at its limit price (row 6, DEC-2300, #1598), no hardware key and a CI
  signing key with staging first (rows 8, 9), staging's vault unsealing itself (row 10), and
  staging's automation needing no step from you (rows 11 to 13), and DEC-175 relaxed so an approved PR
  merges without waiting on CI (row 14). None of these waits on you.

Nothing else is blocked on you.

## Open issues

Resolved issues are in the [work log](11-work-log.md). Each row is one change for whoever picks
it up.

| Issue | Owner |
|---|---|
| The coordinator's secrets audit (2026-10-18): each finding's fix is assigned to its lane, IAC, GO, RS and CT, and SI's leak guard | Those lanes |
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
