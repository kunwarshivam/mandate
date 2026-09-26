# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document. Updated at the end of every working session |
| **Last updated** | 2026-09-26, day 2: M2 closed (E3-3), research spike merged, coordination playbook and cloud builders in use |

Where the project stands, what is waiting on whom, and what comes next. Plans live in
[02-milestones-and-wbs.md](02-milestones-and-wbs.md) and [06-backlog-v1.md](06-backlog-v1.md);
decisions in [04-decision-log.md](04-decision-log.md). This file only tracks progress against them.

## Milestones

| Milestone | State | Done | Next |
|---|---|---|---|
| Tier 1 specs | Approved (DEC-71); amended since | Journal spec v0.3 (DEC-81), trading domain spec v0.10 (DEC-86, DEC-92 to DEC-94) | — |
| M0 Foundations | Done | E1-1, E1-2; CI as two required checks (DEC-76); agent workflow (DEC-78 to DEC-80) | — |
| M1 Market data | In progress | E2-1 download, E2-2 inspect (#64) | E2-4 sessions and corporate actions (`cursor`); E2-3 (Should) later |
| M2 Accounting | Done | E3-1, E3-2, E3-3 (#53, #67) | RC-08 and RC-18 pass when E6-3 interprets `propose_order`; a follow-up tests PR carries the #53 review minors |
| M3 Simulated execution and backtest | In progress | E4-1 brief (#61) | E4-1 tests PR (cloud builder), then implementation and status; E4-2 |
| M4 Journal | In progress | E5-1 | E5-2 tests PR (#66, `cursor`, in review), E5-3 |
| M5 onward | Not started | — | Before M5: the mandate spec rewrite for DEC-97 and DEC-98 (spec-change PRs). M5 starts with DEC-17 (messaging), per ADR-0001 ES-20, and adds E17 (research agent) |

## Stories

| Story | State | PRs | Notes |
|---|---|---|---|
| E1-1, E1-2 Foundations | Merged | before the PR flow | Workspace, CI, xtask, conventions, templates |
| E5-1 Journal core | Merged | [#1](https://github.com/kunwarshivam/mandate/pull/1), [#7](https://github.com/kunwarshivam/mandate/pull/7), [#8](https://github.com/kunwarshivam/mandate/pull/8), [#10](https://github.com/kunwarshivam/mandate/pull/10) | Canonical JSON, decimals, timestamps, append protocol, verification, anchoring; 46 journal cases passing. #2 to #6 merged into stacked branches; #2 to #5 were re-landed as #7, and #6 as #8 |
| `risk_clock` (DEC-81) | Merged | [#12](https://github.com/kunwarshivam/mandate/pull/12), [#13](https://github.com/kunwarshivam/mandate/pull/13), [#14](https://github.com/kunwarshivam/mandate/pull/14) | Required on every risk input; journal vectors version 3 |
| E3-1 Accounting | Merged | [#16](https://github.com/kunwarshivam/mandate/pull/16), [#18](https://github.com/kunwarshivam/mandate/pull/18), [#19](https://github.com/kunwarshivam/mandate/pull/19), [#21](https://github.com/kunwarshivam/mandate/pull/21), [#23](https://github.com/kunwarshivam/mandate/pull/23), [#24](https://github.com/kunwarshivam/mandate/pull/24), [#25](https://github.com/kunwarshivam/mandate/pull/25), [#26](https://github.com/kunwarshivam/mandate/pull/26), spec [#28](https://github.com/kunwarshivam/mandate/pull/28) | `mandate-num`, trading calendars, `mandate-accounting`. Review caught three defects before merge (basis sign, negative fee cap, cap lowered mid-order); #20 and #22 were superseded |
| E2-1 Download | Merged | [#29](https://github.com/kunwarshivam/mandate/pull/29) to [#35](https://github.com/kunwarshivam/mandate/pull/35), [#37](https://github.com/kunwarshivam/mandate/pull/37) | `mandate-marketdata`, `mandate download`; exact Parquet; idempotent (verified live twice). Research basket in DEC-90 |
| E17-0 Research spike | PR open | [#44](https://github.com/kunwarshivam/mandate/pull/44) (code and docs), tests in a second PR | `python/research_spike/`: LLM theses over news and prices, fixed sizing, paper orders, hash-chained JSON Lines journal, score report. Two-to-three-week timebox; exits with a decision-log entry |
| E3-3 Cash-account settlement | Merged | [#53](https://github.com/kunwarshivam/mandate/pull/53), [#67](https://github.com/kunwarshivam/mandate/pull/67) | Account type, buying power per §7.2 (per-bucket ceiling, DEC-104), reservations, harness `buying_power`; no status PR (DEC-105). The tests PR took three review rounds including the interrogate pass; the implementation's diff mutation gate was vacuous, so seven hand-seeded bugs stand as evidence |
| E17-0 Research spike | Merged | [#44](https://github.com/kunwarshivam/mandate/pull/44), [#47](https://github.com/kunwarshivam/mandate/pull/47) | `python/research_spike`: Alpaca news and bars, Claude Sonnet 5 via OpenRouter, fixed sizing under caps, hash-chained JSONL journal, scorer versus SPY. Dry runs only; live paper runs await the founder's go |
| E2-2 Inspect | Merged | [#64](https://github.com/kunwarshivam/mandate/pull/64) | `cursor`: coverage, exact statistics, gaps, duplicates, untrusted partitions |
| E3-2 Corporate actions | Merged | [#36](https://github.com/kunwarshivam/mandate/pull/36), [#38](https://github.com/kunwarshivam/mandate/pull/38), [#39](https://github.com/kunwarshivam/mandate/pull/39) | Splits, cash in lieu, dividends long and short, 12-place adjusted marks. Spec text: trading domain v0.10 (#41) |
| E4-1 Simulated execution | Brief PR open | [#61](https://github.com/kunwarshivam/mandate/pull/61) | New `mandate-sim`: the §6.4 fill model as a pure function (RC-10, RC-12, RC-19). [Task brief](tasks/E4-1-simulated-execution.md); interpretations in DEC-106 |
| E2-2 Inspect | PR open | claim [#56](https://github.com/kunwarshivam/mandate/issues/56) | `mandate inspect`: coverage, exact statistics, gaps with exact timestamps, duplicates, untrusted partitions. Gaps stay unclassified until E2-4 |

## Reference cases

`crates/mandate-refcases/status.toml` is the record; `cargo nextest run -p mandate-refcases` runs
the passing ones and `cargo test -p mandate-refcases -- --include-ignored` shows the rest.

| Suite | Passing | Pending, with the owning stories |
|---|---|---|
| Journal (46) | All 46 | — |
| Trading domain | `schema_version`, RC-01, RC-02, RC-03, RC-05, RC-06 and its `short_position_generic_broker` variant, RC-13, RC-23 and its `forward_3_for_1_non_terminating_mark` variant | RC-08 (E3-3); RC-10, RC-12, RC-19 (E4-1); RC-04 and RC-06 `protective_orders_kept_through_dividend` (E7-2 to E7-4, E6-9); RC-07, RC-11 (E7-3, E7-5, E6-9; their accounting parts are covered by hand tests); the gate, executor, and agent cases (E6-3, E6-5 to E6-9, E7-2 to E7-5). Each pending case names its owner when run |
| Mandate (215) | Not harnessed yet | Harnessed by the E6 and E10 stories |

## Claims

Who holds what, across coordinating sessions (Claude Code and Cursor cloud agents). The claim
issues are the record; this table is the summary
([coordination playbook](../../.cursor/skills/mandate-mode/playbooks/coordination.md)).

| Story | Coordinator | Claim | Stage | Branches and PRs |
|---|---|---|---|---|
| E3-3 cash-account settlement | `claude-code` | #48 | Merged (#53 tests, #67 implementation); follow-up tests PR open | `agent/e3-3-settlement-tests-2`, `agent/e3-3-settlement-impl`, `agent/e3-3-followup-tests` |
| E17-0 research spike | `claude-code` | #49 | Merged (#44, #47); paper runs pending the founder's go | `python/research_spike/` |
| E4-1 simulated execution (backtest fill model) | `claude-code` | #58 | Brief PR open (task brief, DEC-106); tests, implementation, and status PRs follow | #61 (brief); built and pushed: `agent/e4-1-sim-tests`, `agent/e4-1-sim-impl`, `agent/e4-1-sim-status` |
| Track C: mandate spec rewrite for DEC-97 and DEC-98 | `claude-code` | #50 | Not started; after the founder answers the rewrite questions and the spike's first findings | spec-change PRs |
| Direction follow-ups (DEC-99 to DEC-103, rewrite questions) | `cursor` | #54 | PR #57 reviewed PASS; rebase and the founder's confirmation pending | `cursor/direction-follow-ups-v2` (#57) |
| E2-4 market data; E5-2, E5-3 journal | `cursor` | to open | Allocated, not yet claimed | — |
| E2-2 dataset inspect | `cursor` | #56 | One PR (not safety-critical), open with CI green, awaiting review | `cursor/e2-2-inspect-2749` |
| #55 `install.sh` without `astral.sh` | `cursor` | #63 | Merged (#65) | `cursor/install-no-astral` (#65) |
| `install.sh` follow-ups from the #65 review | `cursor` | #70 | PR open, ready for review | `cursor/install-followup` (#73) |

## Waiting on the founder

- **Counsel**: engage securities counsel on the adviser question (compliance questions 31 to 34)
  before the Phase 1 exit (DEC-98). Nothing trades live until this is answered.
- **Robinhood**: open an agentic account yourself, on a desktop, from your own Robinhood login
  (OD-12: self-serve, no beta request). Agents never connect to it (rule 8); the connector story
  will use a paper or test path Robinhood has not yet published, so ask Robinhood support whether one
  exists.
- **GitHub Support**: purge `refs/pull/1/head` to `refs/pull/14/head`, which still hold commits with
  the old work email after the history rewrite.

Nothing else is blocked on you. Agents decide engineering and process questions (DEC-79) and list
them in the decision log.

## Known issues and follow-ups

| Issue | Owner |
|---|---|
| A crypto fee rate above 10000 bps makes a crypto buy an error rather than a credit; decide whether to reject such configurations at load | Next accounting story |
| Fee reservations for buying power | E6-6 |
| The accounting fold copies the account on every input; measure before long backtests | E4-2 |
| Market-data writes use a fixed `.partial` temporary name; concurrent writers to one partition need a lock or unique names | Before any parallel download |
| `AssetClass` exists in both `mandate-accounting` and `mandate-marketdata`; move it to `mandate-domain` | The story that creates `mandate-domain` |
| Market data keeps prices as `DecStr` because `Price` holds 9 places and bars need up to 18 | Same |
| `UtcNanos` parses RFC 3339 only without fractional seconds; Alpaca sends up to nine | E2-4 (a shared `mandate-time` change; E2-2 reads stored nanoseconds and did not need it) |
| Postgres is not installed in the agent environment or CI | E5-3 (add it to `.cursor/install.sh` and a CI service first) |
| Branches are named `cursor/...` because the agent environment requires it; ADR-0001 ES-13 says `agent/...` | Amend ES-13 at the next ADR touch |

## Lessons encoded today

- A PR based on another PR's branch never reaches `main` when merged. Every PR now targets `main`
  (ship playbook).
- Cloud agents cannot launch other agents, so delegated builders stop at an open PR and the
  coordinating session runs the independent review (ship playbook step 3).
- Cursor's cloud-agent hook adds the invoking user as `Co-authored-by`, and it comes back on new
  machines even after `.cursor/install.sh` disables it. CI's spec-guard job rejects the trailer;
  agents run `chmod -x` on the hook before committing.
- `git push -u` with a token-bearing URL writes the token into `.git/config`. Push with the token
  URL only for single commands, never with `-u`.
- Merge squash commits with an explicit `commit_message`; GitHub's default copies trailers.

## Lessons encoded on day 2

- Two coordinating sessions minted the same DEC numbers within an hour; the Reserved identifiers
  table and claim issues (coordination playbook) now precede any new identifier.
- The ruleset requires branches to be current with `main`, so merges are serial: bring a branch up to
  date, wait for CI, merge, next. Merge `main` into a branch (never force-push) when the update
  conflicts.
- A diff mutation gate can pass while testing nothing (every mutant unviable); the implementation PR
  then hand-seeds bugs and says so.
- A cloud routine is created with every account connector attached, including a live brokerage MCP;
  clear them before the first run.
- Running pending property tests writes `*.proptest-regressions`; ignored from now on.

## Next, in order

1. **E4-1** tests, implementation, and status PRs (cloud builders), then **E4-2**.
2. **E2-4** market sessions and corporate actions in the data, then **E2-2** `inspect` (gaps versus
   session closures, duplicates, statistics): closes M1. E2-4 first, because `inspect` needs the
   session model. Resolve the RFC 3339 fractional-seconds gap here.
3. **E5-2** (tests PR #66 in review), then **E5-3** the Postgres journal (environment work first).
4. **E3-3 follow-up tests PR** for the #53 review minors, and the spec-only change for DEC-104
   items 2 and 5 once the founder decides item 5.
5. **Before M5:** the mandate spec, schemas, reference implementation, and the 215 cases rewritten for
   DEC-97 and DEC-98 as spec-change PRs ([ADR-0002](../adr/0002-autonomous-ideation-and-retail.md)):
   the envelope fields and the universe as runtime state, the research agent
   contract, `ThesisProposed` and `UniverseChanged`, the retail profile, and V-020, V-022, and MI-12
   restated for envelope fields.
6. **M5** starts with decision DEC-17 (messaging) and the rewritten mandate reference cases, and adds
   E17 (research agent and dynamic universe).

E3-3, E2-4, and E5-2 touch different crates and can run in parallel; reviews run one at a time.

## How the work runs

- Builders follow `.cursor/skills/mandate-mode/` (story, spec-change, correction, and ship
  playbooks) and prove work with `.cursor/skills/verify-mandate/`.
- Safety-critical stories ship as the DEC-77 sequence: tests PR, implementation PR (test files only
  lose pending markers), status PR.
- Each PR merges only after green CI and a pass from an independent review agent on a different
  model (DEC-79). Today's reviewers ran on GPT-5.6 Sol and failed five PRs before they merged.
