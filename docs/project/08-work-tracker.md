# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document. Updated at the end of every working session |
| **Last updated** | 2026-09-26, day 2 close (work graph added; Cursor paused, everything on `claude-code`): E2-4 and E5-3 complete, E2-3 slice 1, safe dataset writes, fractional seconds, DEC-111 (thesis revision loop), DEC-112 (CI short path for docs); earlier that day: E2-4 sessions and corporate actions merged, E4-1 tests merged, E5-2 complete, E5-3 tests merged, the pending-tests gate in `fast`; direction follow-ups: evidence loop, correlated-flow controls, input hardening, counsel now, and the Phase 1 thin slice (DEC-99 to DEC-103), after the direction change (DEC-97, DEC-98, ADR-0002) |

Where the project stands, what is waiting on whom, and what comes next. Plans live in
[02-milestones-and-wbs.md](02-milestones-and-wbs.md) and [06-backlog-v1.md](06-backlog-v1.md);
decisions in [04-decision-log.md](04-decision-log.md). This file only tracks progress against them.

## Milestones

| Milestone | State | Done | Next |
|---|---|---|---|
| Tier 1 specs | Approved (DEC-71); amended since | Journal spec v0.3 (DEC-81), trading domain spec v0.10 (DEC-86, DEC-92 to DEC-94) | — |
| M0 Foundations | Done | E1-1, E1-2; CI as two required checks (DEC-76); agent workflow (DEC-78 to DEC-80) | — |
| M1 Market data | In progress | E2-1 download, E2-2 inspect (#64), E2-4 sessions and corporate actions (#71, #85, #74, #96), safe concurrent writes (#90), fractional seconds (#93, #98), E2-3 slice 1 (#95) | The M1 exit run (`cursor`, once the founder adds market-data keys to the Cursor environment); E2-3 CLI slice; retire the marketdata RFC 3339 workaround |
| M2 Accounting | Done | E3-1, E3-2, E3-3 (#53, #67) | RC-08 and RC-18 pass when E6-3 interprets `propose_order`; a follow-up tests PR carries the #53 review minors |
| M3 Simulated execution and backtest | In progress | E4-1 brief (#61), E4-1 tests (#75), E4-1 implementation (open) | E4-1 status PR (RC-10, RC-12, RC-19), then E4-2 |
| M4 Journal | In progress | E5-1, E5-2 (#66, #84), E5-3 (#82, #92) | E5-4 verification CLI in review (#108); then the cold store, segment manifests, and the `segment_*` and `tsa_token_invalid` checks |
| M5 onward | Not started | — | Before M5: the mandate spec rewrite for DEC-97 and DEC-98 (spec-change PRs). M5 starts with DEC-17 (messaging), per ADR-0001 ES-20, and adds E17 (research agent) |

## Stories

| Story | State | PRs | Notes |
|---|---|---|---|
| E1-1, E1-2 Foundations | Merged | before the PR flow | Workspace, CI, xtask, conventions, templates |
| E5-1 Journal core | Merged | [#1](https://github.com/kunwarshivam/mandate/pull/1), [#7](https://github.com/kunwarshivam/mandate/pull/7), [#8](https://github.com/kunwarshivam/mandate/pull/8), [#10](https://github.com/kunwarshivam/mandate/pull/10) | Canonical JSON, decimals, timestamps, append protocol, verification, anchoring; 46 journal cases passing. #2 to #6 merged into stacked branches; #2 to #5 were re-landed as #7, and #6 as #8 |
| E5-3 Postgres journal | Merged | tests [#82](https://github.com/kunwarshivam/mandate/pull/82), implementation [#92](https://github.com/kunwarshivam/mandate/pull/92); DEC-109 reservation [#79](https://github.com/kunwarshivam/mandate/pull/79) | New `mandate-journal-pg` (sqlx, no query macros; DEC-109); the append suite shared with `MemoryJournal`; Postgres 18 in `full`, 17 nightly; no reference cases move |
| `risk_clock` (DEC-81) | Merged | [#12](https://github.com/kunwarshivam/mandate/pull/12), [#13](https://github.com/kunwarshivam/mandate/pull/13), [#14](https://github.com/kunwarshivam/mandate/pull/14) | Required on every risk input; journal vectors version 3 |
| E3-1 Accounting | Merged | [#16](https://github.com/kunwarshivam/mandate/pull/16), [#18](https://github.com/kunwarshivam/mandate/pull/18), [#19](https://github.com/kunwarshivam/mandate/pull/19), [#21](https://github.com/kunwarshivam/mandate/pull/21), [#23](https://github.com/kunwarshivam/mandate/pull/23), [#24](https://github.com/kunwarshivam/mandate/pull/24), [#25](https://github.com/kunwarshivam/mandate/pull/25), [#26](https://github.com/kunwarshivam/mandate/pull/26), spec [#28](https://github.com/kunwarshivam/mandate/pull/28) | `mandate-num`, trading calendars, `mandate-accounting`. Review caught three defects before merge (basis sign, negative fee cap, cap lowered mid-order); #20 and #22 were superseded |
| E2-1 Download | Merged | [#29](https://github.com/kunwarshivam/mandate/pull/29) to [#35](https://github.com/kunwarshivam/mandate/pull/35), [#37](https://github.com/kunwarshivam/mandate/pull/37) | `mandate-marketdata`, `mandate download`; exact Parquet; idempotent (verified live twice). Research basket in DEC-90 |
| E3-3 Cash-account settlement | Merged | [#53](https://github.com/kunwarshivam/mandate/pull/53), [#67](https://github.com/kunwarshivam/mandate/pull/67) | Account type, buying power per §7.2 (per-bucket ceiling, DEC-104), reservations, harness `buying_power`; no status PR (DEC-105). The tests PR took three review rounds including the interrogate pass; the implementation's diff mutation gate was vacuous, so seven hand-seeded bugs stand as evidence |
| E17-0 Research spike | Merged | [#44](https://github.com/kunwarshivam/mandate/pull/44), [#47](https://github.com/kunwarshivam/mandate/pull/47) | `python/research_spike`: Alpaca news and bars, Claude Sonnet 5 via OpenRouter, fixed sizing under caps, hash-chained JSONL journal, scorer versus SPY. Dry runs only; live paper runs await the founder's go |
| E2-2 Inspect | Merged | [#64](https://github.com/kunwarshivam/mandate/pull/64) | `cursor`: coverage, exact statistics, gaps, duplicates, untrusted partitions |
| E3-2 Corporate actions | Merged | [#36](https://github.com/kunwarshivam/mandate/pull/36), [#38](https://github.com/kunwarshivam/mandate/pull/38), [#39](https://github.com/kunwarshivam/mandate/pull/39) | Splits, cash in lieu, dividends long and short, 12-place adjusted marks. Spec text: trading domain v0.10 (#41) |
| E4-1 Simulated execution | Tests PR merged | [#61](https://github.com/kunwarshivam/mandate/pull/61), [#75](https://github.com/kunwarshivam/mandate/pull/75) | New `mandate-sim`: the §6.4 fill model as a pure function (RC-10, RC-12, RC-19), 51 pending tests with an independent oracle, 30 planted bugs caught. [Task brief](tasks/E4-1-simulated-execution.md); interpretations in DEC-106, tests-PR shapes in DEC-108. Three review rounds on Claude Fable 5.1; implementation PR open: the fill model and `mandate-num`'s arithmetic fill in every stub, so all 53 pending tests pass and RC-10, RC-12, and RC-19 pass on the code; DEC-114 records two readings neither DEC-106 nor DEC-108 covers. Status PR next |
| E4-2 Baseline backtest and metrics | Brief PR open | [#112](https://github.com/kunwarshivam/mandate/pull/112) | The loop end to end: bars, a division-free moving-average crossover, orders through the §6.4 fill model, fills through the accounting fold, marks at each bar's close, and one exact-decimal report (return, variance and a ceiling-rooted volatility, a squared Sharpe, maximum drawdown from the running peak, one-sided turnover, fees, and a tradable buy-and-hold comparison). [Task brief](tasks/E4-2-backtest-baseline.md); interpretations in DEC-127. Claim [#110](https://github.com/kunwarshivam/mandate/issues/110); no reference case moves, so no status PR |
| E5-2 Artifact store | Merged | [#60](https://github.com/kunwarshivam/mandate/pull/60) (DEC-107 reservation), [#66](https://github.com/kunwarshivam/mandate/pull/66) (tests), [#84](https://github.com/kunwarshivam/mandate/pull/84) (implementation) | Pure core in `mandate-journal`, filesystem backend in the new `mandate-artifacts-fs` (DEC-107): write-once, hard-linked into place, re-hashed on every read; zero missed mutants; no reference cases, so no status PR |
| E5-4 Journal verification CLI | PR open | [#108](https://github.com/kunwarshivam/mandate/pull/108) | `claude-code`: `mandate journal verify <export> [--store] [--anchor] [--from-seq --trusted-prev-hash]` runs journal spec §11 in order over a §6.2 segment and reports the first failure with the spec's code and a non-zero exit; `mandate artifact put`/`get` over the filesystem store, the get re-hashed. Every tamper vector an export can express is replayed through the command; `column_altered` and `seq_values_swapped` are asserted inexpressible, because an export stores no columns apart from the body. `mandate-journal` and `mandate-artifacts-fs` unchanged. Interpretations in DEC-115; [task brief](tasks/E5-4-verification-cli.md) |
| E2-3 Top-of-book quotes (Should) | Slice 2 open | [#95](https://github.com/kunwarshivam/mandate/pull/95), [#107](https://github.com/kunwarshivam/mandate/pull/107) | Slice 1 (`cursor`, #95): `Quote`, `Kind::Quotes`, the Alpaca stock (`sip`, `iex`) and crypto quotes client, `Decimal128(38, 9)` partitions; locked, crossed, and one-sided quotes kept as sent. Slice 2 (`claude-code`, #107): `download --kind quotes`, and `inspect` statistics for a quotes dataset (each quoted side, the signed spread, and the locked, crossed, one-sided, and unquoted counts, DEC-116), so `InspectError::Unsupported` is gone. Still open: a live quotes check in `tests/live.rs` and a quotes download in the M1 exit run, both needing market-data keys |
| E2-4 Sessions and corporate actions | Merged (all four PRs) | [#71](https://github.com/kunwarshivam/mandate/pull/71) (tests), [#85](https://github.com/kunwarshivam/mandate/pull/85) (implementation), [#74](https://github.com/kunwarshivam/mandate/pull/74) (corporate actions), [#96](https://github.com/kunwarshivam/mandate/pull/96) (`inspect` wiring) | `cursor`: the NYSE calendar 2018 to 2028 as data, four sessions per trading day, `session_at`, DST through the bundled tzdb; corporate actions adjusted through `mandate_num::SplitRatio::mark` and fetched by ex-date. #96: each missing bar slot classed as session closure, no trade, true gap, or unclassified by SIP and IEX venue hours checked in as data; `download` stores the corporate actions with the dataset; raw and split-adjusted prices in the report |

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
| E3-3 cash-account settlement | `claude-code` | #48 | Merged (#53 tests, #67 implementation, #80 follow-up tests); claim closed | `agent/e3-3-settlement-tests-2`, `agent/e3-3-settlement-impl`, `agent/e3-3-followup-tests` |
| E4-1 simulated execution (backtest fill model) | `claude-code` | #58, #105 (implementation) | Tests PR merged (#75) after three review rounds; the task brief and DEC-106 merged in #61; implementation PR open (claim #105), status PR follows (DEC-77) | `agent/e4-1-sim-impl-2` (implementation); `agent/e4-1-sim-tests-2` (#75); the stood-down `agent/e4-1-sim-tests`, `-impl`, and `-status` branches await the founder's deletion |
| E4-2 baseline backtest and metrics report (stream E) | `claude-code` | [#110](https://github.com/kunwarshivam/mandate/issues/110) | Brief PR open ([#112](https://github.com/kunwarshivam/mandate/pull/112), docs only, DEC-112): the task brief, DEC-127, the feature map, and these rows; the DEC-77 tests PR follows once the brief merges and the coordinator confirms the crate placement | `agent/e4-2-brief`, then `agent/e4-2-tests` |
| E17-0 research spike | `claude-code` | #49 | Merged (#44, #47); paper runs pending the founder's go | `python/research_spike/` |
| E5-3 Postgres journal | `cursor` | #77 | Merged (#82 tests, #92 implementation); DEC-109 recorded; claims #77 and #78 closed | `cursor/e5-3-pg-tests-e15e` (#82); implementation `cursor/e5-3-pg-impl-e15e` (#92) |
| Track C: mandate spec rewrite for DEC-97 and DEC-98 | `claude-code` | #50 | Not started; after the founder answers the rewrite questions and the spike's first findings | spec-change PRs |
| Direction follow-ups (DEC-99 to DEC-103, rewrite questions) | `cursor` | #54 | PR #57 reviewed PASS and rebased; the founder's confirmation of DEC-99 to DEC-103 pending | `cursor/direction-follow-ups-v2` (#57) |
| E2-4 market sessions and corporate actions | `cursor` | #68 (shared-crate claim #69 closed) | Tests (#71), sessions implementation (#85) and corporate actions (#74) merged; PR 4 (#96) in review, and the claim closes when it merges | `cursor/e2-4-session-tests-ab3f` (#71), `cursor/e2-4-session-impl-ab3f` (#85), `cursor/e2-4-corporate-actions-ab3f` (#74), `cursor/e2-4-inspect-sessions-ab3f` (#96) |
| E2-2 dataset inspect | `cursor` | #56 | Merged (#64) | `cursor/e2-2-inspect-2749` (#64) |
| #55 `install.sh` without `astral.sh` | `cursor` | #63 | Merged (#65) | `cursor/install-no-astral` (#65) |
| `install.sh` follow-ups from the #65 review | `cursor` | #70 | Merged (#73) | `cursor/install-followup` (#73) |
| E5-2 artifact store | `cursor` | #59 | Merged (#66 tests, #84 implementation); DEC-107 recorded; claim closed | `cursor/e5-2-artifact-tests-b0be` (#66), `cursor/e5-2-artifact-impl-b0be` (#84) |
| `xtask`: pending tests must fail on stubs (shared crate) | `cursor` | #76 | Merged (#81; DEC-110 reserved in #83); claim closed | `cursor/xtask-pending-fail-7e3b` (#81) |
| Cursor allocation: next stories | `cursor` | #87 | Merged (#88); E5-4 and the M1 exit run's keys await the founder | `cursor/allocation-next` (#88) |
| `mandate-marketdata` safe concurrent dataset writes | `cursor` | #86 | Claimed | — |
| `mandate-time`: `UtcNanos` fractional seconds (shared crate, for E2-1) | `cursor` | #91 | Merged (#93 tests, #98 implementation, the latter cherry-picked onto main by the coordinator while Cursor was paused); claim closed | `cursor/utcnanos-fraction-tests-2652` (#93), `agent/utcnanos-fraction-impl` (#98) |
| E2-3 top-of-book quotes | `cursor` | #94 | Slice 1 merged (#95); claim closed | `cursor/e2-3-quotes-v2-a075` (#95); `cursor/e2-3-quotes-a075` superseded |
| E2-3 CLI slice and the RFC 3339 workaround retirement (stream B) | `claude-code` | #106 | `download --kind quotes`, quote statistics in `inspect` (DEC-116), and the Alpaca timestamp parse routed through `UtcNanos::parse_rfc3339`; PR #107 open, checks green locally | `agent/e2-3-cli-quotes` (#107) |
| `mandate-marketdata`: safe concurrent dataset writes (E2-1 follow-up) | `cursor` | #86 | Merged (#90); claim closed | `cursor/marketdata-write-safety` (#90) |
| Thesis revision loop: DEC-111, E17-9, R-28 | `claude-code` | — | Merged (#99); the story waits on E17-8 | docs only |
| E5-4 journal verification and artifact commands | `claude-code` | #104 | PR #108 open with green checks, waiting on the merge coordinator's independent review; DEC-115 reserved | `agent/e5-4-verify-cli` (#108) |
| CI short path for documentation-only changes (DEC-112) | `claude-code` | — | Merged (#100) at the founder's request; a docs PR now costs under two runner minutes | `.github/scripts/` |
| `mandate-marketdata`: proactive rate limiting (E2-1 follow-up) | `cursor` | [#115](https://github.com/kunwarshivam/mandate/issues/115) | One PR (tests first, then implementation), open with green checks for the merge coordinator's review; the claim closes when it merges | `cursor/marketdata-rate-limit` |

## Waiting on the founder

- **Counsel**: engage securities counsel on the adviser question (compliance questions 31 to 35)
  now, during Phase 0 (DEC-102). Nothing trades live until this is answered.
- **Design questions**: answer the [design questions](09-mandate-rewrite-questions.md) (universe
  size, thesis lifetime, research weight and cost cap, the DEC-99 evaluation, the DEC-100 values, the
  Robinhood paper stage, retail `auto`, how theses are shown) before the mandate spec rewrite starts.
- **Robinhood**: open an agentic account yourself, on a desktop, from your own Robinhood login
  (OD-12: self-serve, no beta request). Agents never connect to it (rule 8); the connector story
  will use a paper or test path Robinhood has not yet published, so ask Robinhood support whether one
  exists.
- **GitHub Support**: purge `refs/pull/1/head` to `refs/pull/14/head`, which still hold commits with
  the old work email after the history rewrite.
- **DEC-99 to DEC-103**: confirm or amend them so #57 can merge.
- **E5-4 and the M1 exit run** (#88): confirm E5-4 as a Must story, and add Alpaca market-data keys
  to the Cursor environment's secrets (historical data only) so the M1 exit run can start.
- **Spike paper runs** (E17-0): add the Alpaca paper and OpenRouter keys as cloud environment
  secrets, allow egress to the Alpaca paper and data hosts and to OpenRouter, and say go.
- **Branches**: delete the stood-down `agent/e4-1-sim-tests`, `-impl`, and `-status` branches, and
  the merged `agent/*` and `cursor/*` branches the remote still carries; agents do not delete
  branches.

Nothing else is blocked on you. Agents decide engineering and process questions (DEC-79) and list
them in the decision log.

## Known issues and follow-ups

| Issue | Owner |
|---|---|
| A crypto fee rate above 10000 bps makes a crypto buy an error rather than a credit; decide whether to reject such configurations at load | Next accounting story |
| Fee reservations for buying power | E6-6 |
| The accounting fold copies the account on every input; measure before long backtests | E4-2 |
| ~~Market-data writes use a fixed `.partial` temporary name; concurrent writers to one partition need a lock or unique names~~ **Resolved (claim #86, PR #90):** each write holds an advisory lock on the dataset directory, uses a temporary name no other writer uses, and publishes a partition by hard link ([brief](tasks/marketdata-write-safety.md)) | `cursor` |
| `AssetClass` exists in both `mandate-accounting` and `mandate-marketdata`; move it to `mandate-domain` | The story that creates `mandate-domain` |
| Market data keeps prices as `DecStr` because `Price` holds 9 places and bars need up to 18 | Same |
| ~~`mandate-marketdata` keeps its own RFC 3339 parser (`timestamp::parse_rfc3339_utc`), the workaround for the fractional-seconds gap that #93 and #98 closed~~ **Resolved (claim #106, PR #107):** `parse_rfc3339_utc` now only narrows `UtcNanos::parse_rfc3339` to the UTC form Alpaca sends, and the duplicated parser is deleted; `mandate-time`'s differential test keeps a copy pinned to `c8efb09` as an oracle of its own | `claude-code` |
| GitHub Actions minutes: 90 percent of the month's 3,000 used by 2026-09-26; DEC-112 makes docs PRs cheap, and the ruleset's up-to-date rule still re-runs every open PR after each merge | Founder: decide whether to relax the up-to-date rule |
| Postgres in the agent environment: `.cursor/install.sh` installs PostgreSQL 18 only where apt.postgresql.org is reachable, and `MANDATE_PG_URL` must be exported by hand (`environment.json` cannot set it) | E5-3 (CI has it: a service container in `full` and nightly) |
| Branches are named `cursor/...` because the agent environment requires it; ADR-0001 ES-13 says `agent/...` | Amend ES-13 at the next ADR touch |
| ~~The market-data client's rate limiting is reactive: about 200 requests, then 429s and a 1 to 32 s ladder whose six attempts span 63 s against a 60 s window~~ **Resolved (claim #115):** the client paces to the `X-Ratelimit-*` headers, a 429 waits for the next window with a three-window budget, and a token bucket is the floor without headers; a live 1,540-page download went from 376 refused requests to none ([brief](tasks/marketdata-rate-limit.md)) | `cursor` |

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
- A tests PR (#66) carried a pending test that already passed on its stubs. The `fast` check now
  runs every pending test and fails if one passes (`cargo xtask ci pending`, DEC-110).
- A cloud builder routine stays subscribed to its PR after opening it and fixes review findings
  itself. The coordinator posts the verdict and checks the author run before launching a fix run;
  the duplicate run launched on #75 noticed the author's push, discarded its own commit, and
  verified the fix instead.
- Every pending test failing on the stubs proves only that the tests are wired in when they all
  stop at one guard (E5-3's migrations check). The planted-bug table in the brief carries the
  per-test evidence, and a tests PR without one does not merge.
- A PR whose test cannot fail on the wrong model (a bar capped at zero by DEC-106 item 4) passes
  every gate; only a reviewer asking "what would the wrong model do here" catches it.

## Work graph (2026-09-26 evening, `claude-code` runs everything)

The founder paused the Cursor sessions and asked the coordinating session to run the remaining
work with parallel cloud builders under the same review rule (DEC-79). Streams run in parallel when
they touch different crates; each ends at a PR the coordinator reviews (a reviewer on a different
model from the builder) and merges through the one queue. The ruleset's up-to-date rule is off
while the Actions budget is spent (DEC-113); the coordinator runs `cargo xtask check` on `main`
after each code merge.

| Stream | Work | Depends on | Reserved IDs | State |
|---|---|---|---|---|
| A | E4-1 implementation PR (`mandate-sim`, `mandate-num`; test files only lose `pending E4-1` markers), then the status PR moving RC-10, RC-12, RC-19 to passing | #75 (merged) | DEC-114 | implementation PR [#111](https://github.com/kunwarshivam/mandate/pull/111) open, ready for review; status PR after it merges |
| B | E2-3 CLI slice (`download --kind quotes`, quote statistics in `inspect`) and retiring `mandate-marketdata`'s own RFC 3339 parser for `UtcNanos::parse_rfc3339` | #95, #98 (merged) | DEC-116 | builder launched |
| C | E5-4: `mandate-cli journal verify` over an exported stream and its artifact store, and artifact put and fetch | E5-1 to E5-3 (merged); the founder's confirmation of E5-4 taken from the delegation | DEC-115 | PR [#108](https://github.com/kunwarshivam/mandate/pull/108) open, waiting on review |
| D | Track C: answer the rewrite questions as decisions proposed to the founder, then the mandate spec, schemas, reference implementation, and cases for DEC-97 to DEC-103 and DEC-111 (spec-change PR, no code) | nothing; the founder can veto any proposed answer before the spec PR merges | DEC-117 to DEC-126 | builder launched |
| E | E4-2 task brief and interpretations (docs only), then the DEC-77 tests PR for the baseline backtest and metrics: a new `mandate-backtest` crate if the founder takes the layer-7 entry, otherwise modules in `mandate-sim`, plus the `mandate-num` metric arithmetic under a shared-crate claim | #75, #111, #113 (all merged); the brief is written against that API and re-checked against #111 | DEC-127 | brief PR [#112](https://github.com/kunwarshivam/mandate/pull/112) open, claim #110 |
| Founder | Alpaca market-data keys in the Cursor environment (M1 exit run); paper and OpenRouter keys and egress in the cloud environment, then go for spike paper runs; counsel; branch cleanup; key rotation | | | waiting |

## Next, in order

1. **E4-1** implementation and status PRs (cloud builder), then **E4-2**.
2. **E2-4** PR 4 (gap classification in `inspect`, `cursor`): closes M1 with the M1 exit run once
   the founder adds market-data keys. **E2-3** (Should) later.
3. **E5-3** implementation PR (`cursor`), then **E5-4** once the founder confirms it.
4. The spec-only change for DEC-104 items 2 and 5 once the founder decides item 5.
5. **Before M5:** the mandate spec, schemas, reference implementation, and the 215 cases rewritten for
   DEC-97 and DEC-98 as spec-change PRs ([ADR-0002](../adr/0002-autonomous-ideation-and-retail.md)):
   the envelope fields and the universe as runtime state, the research agent
   contract, `ThesisProposed` and `UniverseChanged`, the retail profile, and V-020, V-022, and MI-12
   restated for envelope fields.
6. **M5** starts with decision DEC-17 (messaging) and the rewritten mandate reference cases, and adds
   E17 (research agent and dynamic universe), starting with the DEC-103 thin slice: the team's
   internal paper workspaces with the research basket as the fixed test data universe, every
   admission `ask`, paper only, scorecards on (E15-3), and the forward-paper evaluation (E17-8). The
   full E17-3, for users' agents under their own envelopes, follows only after that evaluation
   passes. **E17-9**, the thesis revision loop, follows E17-8 and one completed evaluation
   ([DEC-111](04-decision-log.md#decisions)); it is never built against backtests.

E4-1, E2-4, and E5-3 touch different crates and can run in parallel; reviews and merges run one at a
time.

## How the work runs

- Builders follow `.cursor/skills/mandate-mode/` (story, spec-change, correction, and ship
  playbooks) and prove work with `.cursor/skills/verify-mandate/`.
- Safety-critical stories ship as the DEC-77 sequence: tests PR, implementation PR (test files only
  lose pending markers), status PR.
- Each PR merges only after green CI and a pass from an independent review agent on a different
  model (DEC-79). Day 1's reviewers ran on GPT-5.6 Sol and failed five PRs before they merged; day
  2's ran as cloud routines on Claude Opus 5 for Cursor-authored PRs and on Claude Fable 5.1 for
  Opus-authored ones, and failed five rounds before eleven PRs merged.
