# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document, one page. Updated at the end of every working session |
| **Last updated** | 2026-10-08, after the design conversation and the first five decisions of its plan (#669) |

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
| M4 Journal | In progress: core, Postgres, artifacts, verification CLI, cold store and its checks merged; E5-8 `verify-cold` tests merged (#654), implementation open (#662) | E5-5 the personal-data vault; E5-7 to E5-9 the cold store's operational half, CLI and examination bundle; all safety-critical; DEC-265 item 1 waits on the founder |
| M5 Agent runtime and risk | In progress: the runtime and kill switches, the mandate document (validation, policy, change classification), the risk state and ladder, the gate with the US account rules, eligibility, conduct controls and restrictions, autonomy and the order builder, delegations, the client ceiling, the review date, V-047 | Tripwires (E6-13: MC-W01 to MC-W57) in the DEC-77 sequence; the Proposed readings under Waiting on the founder |
| M6 Alpaca connector (paper) and recovery | In progress: the executor's intent, state machine and reconciliation; the protective-exit slices 1 to 6; the agent-, control- and account-stream schemas | The tracer (E7-7): #598 through its gates, then the one external order under DEC-450; E7-9's implementation |
| M7 Escalation v0 | In progress: `mandate-approval` (E8-1 to E8-3), the runtime's approval path, owner commands, the MC-E lifecycle driver | The CLI's `clap` wiring of the inbox and owner commands; email and one chat channel; MC-E01, E06, E17 to E24, E29 |
| M9 Web app (started early, DEC-200) | On fixtures: the shell, Home, agents, approvals, Messages and the copilot, the set-up chat, sign-in, the landing page; the design plan's first five decisions (DEC-511 to DEC-515) on #669 | `web/design/plan.md`, every unticked item, in its order; then the connection to a deployment |
| M8, M10 to M13 | Planned; the design layer drafted (DEC-431 to DEC-443) | ADR-0003's code stories (E8-8, E10-7 to E10-9, E11-4 to E11-8, E12-5) |

**Reference cases.** Journal 46 of 46. Trading domain 13 of 26 cases plus four variants; the rest
wait on the executor stories (E7-2 to E7-5) and the founder's DEC-129 items. Mandate 359 of 441,
the rest on E6-13, E8-3, E8-8 and DEC-444's harness. `crates/mandate-refcases/status.toml` is the
record.

**Open PRs.** #669 (the design plan, DEC-511 to DEC-515), #662 (E5-8 implementation), #647 (the
xtask reference checks, DEC-493), #514 (DEC-422, amends trading spec §5.4), #603 (the tracer's
paper assembly). Each merges on green CI and an independent review on a different model (DEC-79).

## Next, in order

1. **Merge #669**, then revise the development process (the founder, 2026-10-08: the process has
   become the bottleneck): one branch per plan item; `npm run shots` and a golden-path contact
   sheet so review is visual; the test sort DEC-511 item 5 calls for; a lighter path for
   reversible product changes. Decisions already taken may be revised.
2. **The tracer (E7-7).** #598 through its full local check, green CI and review; then the one
   external order only with explicit founder confirmation, paper-only credentials and a
   founder-selected instrument (DEC-450).
3. **The web plan** (`web/design/plan.md`), in its order: the process and codebase items (I), the
   remaining Home and agent-page items (F), craft (G), the website (H).
4. **Tripwires (E6-13)**, then M7's remainder, then M4's safety-critical half, then ADR-0003's code
   stories, each in the DEC-77 sequence with its own claim.

Streams that touch different crates run in parallel; reviews and merges run one at a time.

## Waiting on the founder

- **Counsel** on the adviser question (compliance questions 31 to 41) during Phase 0 (DEC-102).
  Nothing trades live until this is answered.
- **Readings left Proposed**, on which agents take the most conservative option meanwhile:
  DEC-104 item 5; DEC-129 items 25 to 27; DEC-250; DEC-265 item 1; DEC-266 item 4; DEC-285
  item 6; DEC-476 item 7 (which model reads the owner's words at `/agents/new`, through which
  route, its spend cap and data terms); DEC-480 (acting from a message).
- **Decided, record still to land:** DEC-360 (option (c)), DEC-410 item 3, DEC-422 (#514 open).
- **Design calls the plan parks for you:** the landing wordmark (DEC-467 chose the block letters;
  the plan prefers one era of type); the short set-up summary (counsel, DEC-477 question 23);
  the phone chart's axis (a canvas option against the no-script-media-query rule).
- **Robinhood:** whether a paper or test path exists for agentic accounts.
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
| The Azure and Sun palette (#363) has no recorded decision: `palette.ts` cites DEC-217, which was never written, and `web/COLOR.md` still describes Ink and Volt | W4 |
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
| `08-work-tracker.md` had lines 7,800 characters long; it is now one page, with the record in the work log, and stays that way | Every session |

## How the work runs

- Builders follow `.cursor/skills/mandate-mode/` (story, spec-change, correction and ship
  playbooks) and prove work with `.cursor/skills/verify-mandate/`.
- Safety-critical stories ship as the DEC-77 sequence: tests PR, implementation PR (test files only
  lose pending markers), status PR.
- Each PR merges only after green CI and a pass from an independent review agent on a different
  model (DEC-79).
- This file is one page. A session ends by updating the milestone row it touched, the Next list,
  and one dated paragraph in the work log; PR numbers and the story of a change go in the log, the
  PR or the decision file, never here.
