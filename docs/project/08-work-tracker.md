# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document, one page. Updated at the end of every working session |
| **Last updated** | 2026-10-09, the coordinator handover: session `01BFjCWYU2cDwPmFkR3RAHQK` took the coordinator role, the founder decided DEC-833 and DEC-834, and the paper path was dispatched again |

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
| M5 Agent runtime and risk | In progress: the runtime and kill switches, the mandate document (validation, policy, change classification), the risk state and ladder, the gate with the US account rules, eligibility, conduct controls and restrictions, autonomy and the order builder, delegations, the client ceiling, the review date, V-047 | Tripwires (E6-13: MC-W01 to MC-W57) in the DEC-77 sequence; the Proposed readings under Waiting on the founder |
| M6 Alpaca connector (paper) and recovery | In progress. The executor, protective exits slices 1 to 7, and the paper path through D3, D4a, Q1, A1, D1 to D2b, P0, V0, M0 to M2b and R0 are merged. The live path's SP1, L0, K1a, B1 (tests and implementation) and the M1, S1 and X1 tests are merged | **First paper trade** ([brief](tasks/first-paper-trade.md)), in order: J3, D2c and D4b implementations, then D4c, D4d, Q2, H3, E1a, E1b, E3; then E2, the founder's run. **First live trade** ([brief](tasks/first-live-trade.md), DEC-529, target 2026-10-23): the implementations of M1, S1 and X1; then M2, O1a, S2, B2a, B3, K1b, then B2b and the C and G rows; rows E7-23 to E7-26 are in the backlog |
| M7 Escalation v0 | In progress: `mandate-approval` (E8-1 to E8-3), the runtime's approval path, owner commands, the MC-E lifecycle driver | The CLI's `clap` wiring of the inbox and owner commands; email and one chat channel; MC-E01, E06, E17 to E24, E29 |
| M9 Web app (started early, DEC-200) | On fixtures: the shell, Home, agents, approvals, Messages and the copilot, the set-up chat, sign-in, the landing page (its windows move as a Mac's, its name the product's logo, its buttons grey: DEC-903; a switch between Windows 98 and a System 7 desktop, Windows the default until the Mac is finished: DEC-904; the app's own screens in a second browser tab, a painting per visit, and desktop motion: DEC-905; the app itself live in front, driven by the browser, owls with their own minds, and no explaining copy: DEC-906); the design plan's first five decisions (DEC-511 to DEC-515) on #669 | `web/design/plan.md`, every unticked item, in its order; then the connection to a deployment; after the first paper trade, the founder's Home (signed-in, D1), holdings, search and watchlist stories (E11-10, E11-11; [DEC-528](decisions/DEC-528.md), which also places E10-19 and E19-12 later and defers adoption) |
| M8 and M9's demo lanes (DEC-820, demo about 2026-10-26) | In progress in five lanes (below): identity spec v0.3, the authn, identity and passkey crates' tests and the passkey implementation; the workspace API contract and schemas; journal spec to v0.21; notifications spec v0.2 and its crates' tests; the web client foundation; the audit read contracts | L1 E9-1 sessions, E9-2 I1 implementation, E9-4 step-up, E9-7; L2 the journal spec queue (after v0.21), E10-13, E10-10 implementation, E10-7, E7-17 (fold tests and DEC-687 merged, the tests' correction in review, then the fold implementation, then the state-machine slice); L3 the E8-9 and E8-14 implementations; L4 the E11-9 slices; L5 E12-6, waiting on E9-2's `authorize` (identity ID-8) |
| M8, M10 to M13 | Planned; the design layer drafted (DEC-431 to DEC-443) | ADR-0003's code stories (E8-8, E10-7 to E10-9, E11-4 to E11-8, E12-5) |

**Reference cases.** Journal 46 of 46. Trading domain 13 of 26 cases plus four variants; the rest
wait on the executor stories (E7-2 to E7-5) and the founder's DEC-129 items. Mandate 359 of 441,
the rest on E6-13, E8-3, E8-8 and DEC-444's harness. `crates/mandate-refcases/status.toml` is the
record.

**Open PRs.** About thirty; GitHub is the live record. Each merges on green CI and an independent
review on a different model (DEC-79), approved by the coordinator under DEC-175.

## Next, in order

1. **The first paper trade.** J3, D2c and D4b implementations are dispatched (claims #850, #851);
   D4c, D4d, Q2, H3, E1a, E1b and E3 follow in the [brief](tasks/first-paper-trade.md)'s order, then
   the founder's run (E2), which waits for the founder's explicit confirmation (DEC-450).
2. **The first live trade** (DEC-529, target 2026-10-23, deadline 2026-11-02): land M1, S1 and
   X1's implementations, then M2, O1a, S2, B2a, B3 and K1b's implementation, then B2b, the C and G
   rows and the rehearsal (R0). It needs the paper path's E1a and E1b.
3. **L1 identity:** the E9-1 session stack, E9-2's I1 implementation (it unblocks L5's E12-6),
   E9-4's step-up tests, E9-7 after its spec.
4. **L2 workspace API:** the open journal spec changes, renumbered after v0.21 and merged one at a
   time (E10-15's client actor, then its hold records, then E9-7's identity records, then E12-3's
   audit records), then E10-15's registration code; E10-13's tests and implementation; E10-10's
   implementation; E10-7.
5. **L3 and L4:** the E8-9 and E8-14 implementations; the next E11-9 slices on live data.
6. **Then** tripwires (E6-13), M7's remainder, M4's safety-critical half, as before.

Streams that touch different crates run in parallel; reviews and merges run one at a time.

## Waiting on the founder

- **Counsel** on the adviser question (compliance questions 31 to 41) during Phase 0 (DEC-102).
  Nothing trades live until this is answered.
- **Readings left Proposed**, on which agents take the most conservative option meanwhile:
  DEC-104 item 5; DEC-129 items 25 to 27; DEC-250; DEC-265 item 1; DEC-266 item 4; DEC-285
  item 6; DEC-476 item 7 (which model reads the owner's words at `/agents/new`, through which
  route, its spend cap and data terms); DEC-480 (acting from a message).
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
