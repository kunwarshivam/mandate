# Work tracker

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document. Updated at the end of every working session |
| **Last updated** | 2026-09-26, night: the four M5 briefs merged after multi-round reviews (F #129, G #127, H #128, I #126) and their tests PRs are go; the E4-2 tests PR #118 merged and its implementation is in progress; Cursor's #119 and #121 merged; the repository is public until the Actions quota resets on 2026-10-01 (interaction limits on, collaborators only) and the README (#130) is the technical front door; earlier that evening: the five parallel streams merged in one queue (E2-3 slice 2 #107, E5-4 #108, E4-1 implementation #111 and status #113, the E4-2 brief #112 with DEC-127, the mandate spec v0.6 #109 with DEC-117 to DEC-126 proposed to the founder), so E4-1, E5-4, and E2-3 are complete and Track C's spec rewrite is in; DEC-113 (up-to-date rule off) and DEC-114 to DEC-116 recorded; earlier that day (work graph added; Cursor paused, everything on `claude-code`): E2-4 and E5-3 complete, E2-3 slice 1, safe dataset writes, fractional seconds, DEC-111 (thesis revision loop), DEC-112 (CI short path for docs); earlier that day: E2-4 sessions and corporate actions merged, E4-1 tests merged, E5-2 complete, E5-3 tests merged, the pending-tests gate in `fast`; direction follow-ups: evidence loop, correlated-flow controls, input hardening, counsel now, and the Phase 1 thin slice (DEC-99 to DEC-103), after the direction change (DEC-97, DEC-98, ADR-0002) |

Where the project stands, what is waiting on whom, and what comes next. Plans live in
[02-milestones-and-wbs.md](02-milestones-and-wbs.md) and [06-backlog-v1.md](06-backlog-v1.md);
decisions in [04-decision-log.md](04-decision-log.md). This file only tracks progress against them.

## Milestones

| Milestone | State | Done | Next |
|---|---|---|---|
| Tier 1 specs | Approved (DEC-71); amended since | Journal spec v0.3 (DEC-81), trading domain spec v0.10 (DEC-86, DEC-92 to DEC-94) | — |
| M0 Foundations | Done | E1-1, E1-2; CI as two required checks (DEC-76); agent workflow (DEC-78 to DEC-80) | — |
| M1 Market data | In progress | E2-1 download, E2-2 inspect (#64), E2-4 sessions and corporate actions (#71, #85, #74, #96), safe concurrent writes (#90), fractional seconds (#93, #98), E2-3 (#95, #107), the marketdata RFC 3339 parser retired (#107) | The M1 exit run (once the founder adds market-data keys to an agent environment); proactive rate limiting in the Alpaca client (claim #115, opened by a `cursor` session) |
| M2 Accounting | Done | E3-1, E3-2, E3-3 (#53, #67) | RC-08 and RC-18 pass when the risk gate lands (E6-3 and E6-6, stream G): both supply an explicit order and expect a gate verdict — a deny with reason code `insufficient_settled_buying_power` for RC-08 and RC-18's `generic_cash_account` variant, an `allow` with buying power `449.97` for RC-18's main path — so neither waits on the order builder (DEC-130 item 4); a follow-up tests PR carries the #53 review minors |
| M3 Simulated execution and backtest | In progress | E4-1 complete: brief (#61), tests (#75), implementation (#111), status (#113); RC-10, RC-12, RC-19 passing; the E4-2 brief and DEC-127 (#112) | E4-2 tests PR (stream E, `agent/e4-2-tests`, new `mandate-backtest` crate at layer 7 with the `mandate-num` metric arithmetic under claim #114), then its implementation PR |
| M4 Journal | In progress | E5-1, E5-2 (#66, #84), E5-3 (#82, #92), E5-4 verification CLI (#108) | The cold store, segment manifests, and the `segment_*` and `tsa_token_invalid` checks |
| M5 Agent runtime and risk | Started | The four briefs: F `mandate-spec` and `mandate-domain` ([#129](https://github.com/kunwarshivam/mandate/pull/129), DEC-128), G `mandate-risk` ([#127](https://github.com/kunwarshivam/mandate/pull/127), DEC-129), H `mandate-builder` ([#128](https://github.com/kunwarshivam/mandate/pull/128), DEC-130), I the runtime skeleton and kill switches ([#126](https://github.com/kunwarshivam/mandate/pull/126), DEC-131); the mandate spec v0.6 and its 298 cases are the contract; the DEC-17 recommendation stands Proposed | The DEC-77 tests PRs per stream (F first, since G, H, and I build on its type names; H after E4-2's implementation merges), then the implementation PRs; stream J's brief (E17 thin slice); stream K (the Alpaca paper connector) waits on the founder |

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
| E4-1 Simulated execution | Merged (all four PRs) | [#61](https://github.com/kunwarshivam/mandate/pull/61), [#75](https://github.com/kunwarshivam/mandate/pull/75), [#111](https://github.com/kunwarshivam/mandate/pull/111), [#113](https://github.com/kunwarshivam/mandate/pull/113) | New `mandate-sim`: the §6.4 fill model as a pure function (RC-10, RC-12, RC-19 passing), 53 tests with an independent oracle, 30 planted bugs caught; `mandate-num` gained the fill arithmetic (`Fraction::parse`, `Qty::portion`, `Bps::sqrt_impact`, `Price::slipped`). [Task brief](tasks/E4-1-simulated-execution.md); interpretations in DEC-106, tests-PR shapes in DEC-108, two further readings in DEC-114. Reviews on Claude Fable 5.1: three rounds on the tests, one on the implementation (148 mutants, 0 missed). Follow-up: a `SessionOffAssetClass` validation (Continuous only for crypto), see Known issues |
| E4-2 Baseline backtest and metrics | Tests PR open | [#112](https://github.com/kunwarshivam/mandate/pull/112), [#118](https://github.com/kunwarshivam/mandate/pull/118) | The loop end to end: bars, a division-free moving-average crossover, orders through the §6.4 fill model over the whole bar slice, fills through the accounting fold, marks at each bar's close, and one exact-decimal report (return, variance and a ceiling-rooted volatility, a squared Sharpe, maximum drawdown from the running peak, one-sided turnover, fees, and a tradable buy-and-hold comparison). [Task brief](tasks/E4-2-backtest-baseline.md); 22 interpretations in DEC-127. The tests PR ([#118](https://github.com/kunwarshivam/mandate/pull/118)) adds that crate at layer 7, safety-critical and pure, with the `mandate-num` metric arithmetic and 74 pending tests that all fail on the stubs; its `xtask/layers.toml` and CODEOWNERS entries are the founder's to veto. Claim [#110](https://github.com/kunwarshivam/mandate/issues/110), shared-crate claim [#114](https://github.com/kunwarshivam/mandate/issues/114); no reference case moves, so no status PR |
| E6-2 Autonomy and the order builder | Brief PR open | [#128](https://github.com/kunwarshivam/mandate/pull/128) | Stream H of M5. New `mandate-builder` (layer 5, safety-critical, pure): the §6 autonomy classification (purposes, the rule walk, the admission ceiling, approver counts) and the §8.3 `conviction_linear` order builder (combine, decide, size, clip, the accumulate clips), with the gate's dry-run verdict arriving as a value so the builder never calls the gate. [Task brief](tasks/E6-2-autonomy-and-order-builder.md); 21 interpretations in DEC-130; claim [#125](https://github.com/kunwarshivam/mandate/issues/125). The 16 `MC-A` and 28 `MC-B` cases move in a harness-and-status PR after stream F's tests PR (no `mandate` harness module yet) and, for the `B` family, stream G's gate, since every `B` case states a `gate_state` and expects a `gate_dry_run` verdict; the `A` family needs neither the gate nor a quote and could move on F's harness alone. DEC-130 items 1 to 21; independent review approved on 29827a07 with one round of six documentation fixes, taken in the same PR. `MC-B17` and `MC-B30` to `MC-B32` (the `trim_to_target` guards) and `RC-08` and `RC-18` (buying power) are stream G's, not this stream's (DEC-130 items 3 and 4) |
| E10-1 Mandate validation (schema, V-rules, policy) | Brief open; tests PR next | [#129](https://github.com/kunwarshivam/mandate/pull/129) | Stream F. The rule engine behind the clause, not E10-1's compiler or E10-2's editor: `Mandate::parse` over canonical JSON accepting exactly what the JSON Schema accepts (ES-22, families S, 31 cases), the 31 V-rules and 5 warnings with the confirmation screen's four dollar figures (family V, 67 cases), and the policy hierarchy with the §4.3 runtime overlay (family P, 22 cases). [Task brief](tasks/M5-F-mandate-spec.md); 21 interpretations in DEC-128, and each decimal checked against its own `$def` grammar (`DecStr` normalises rather than rejects). New `mandate-spec` (layer 3) and `mandate-domain` (layer 1), both safety-critical and pure; founder veto on the `layers.toml` and CODEOWNERS entries. Claim [#124](https://github.com/kunwarshivam/mandate/issues/124) |
| E10-3 Mandate versions and change classification | Brief open; tests PR next | [#129](https://github.com/kunwarshivam/mandate/pull/129) | Stream F. `sha256:` of the canonical JSON as the version (spec §9.1, the fixture's version vector), the changed-path walk that compares arrays whole, every §9.2 row including the DEC-121 pinning switch in both directions, and `step_up_required` (family C, 48 cases). MI-11 is proved by a behavioural oracle that evaluates generated actions under both rule sets. The diff view and the step-up flow are later stories |
| E6-4 Daily loss, the drawdown ladder, and goals | Brief open; tests PR next | [#129](https://github.com/kunwarshivam/mandate/pull/129) | Stream F. The risk state as a fold over §5.2's inputs: the ladder, breach confirmation with DEC-63's two-quote hard trigger, the rollover, the lifetime floor with its inherited loss, allocation scaling rounded up at 12 places, acknowledgment and the stepwise lift, per-instrument staleness, the effective mode, and goal completion (families R 24, T 5, L 5). Family F (agent flatten) stays with stream G's claim [#123](https://github.com/kunwarshivam/mandate/issues/123) |
| E17-1 The envelope and strategy field split | Brief open; tests PR next | [#129](https://github.com/kunwarshivam/mandate/pull/129) | Stream F. The hashed envelope document with provenance outside it, V-034 to V-039, `platform_proposed` as a source that is inactive until confirmed (MI-12), and the working universe kept out of the document entirely: `mandate-spec` names only the `WorkingUniverse` shape the risk state reads, and admission stays stream J's |
| E17-3 Admission into the working universe | Brief open; tests PR next | [#133](https://github.com/kunwarshivam/mandate/pull/133) | Stream J of M5, as the DEC-103 thin slice. The seventeen ordered §8.5 checks as one pure function over a validated mandate, the policy overlay, the folded universe, a thesis, and the facts other streams supply (the eligibility floor's failures, group claims, operator halts, accepted disclosures, the pinned data universe, the day's research spend); a refusal admits nothing and the one exception, `lineage_retired`, removes; expiry, invalidation, and retirement as the three removals of §8.6 (MC-N20 to MC-N22); the deterministic stagger offset of §8.4 and DEC-123 (MC-N23), hashed through `mandate-canon` so no `sha2` is added. [Task brief](tasks/M5-J-research-thin-slice.md); 21 interpretations in DEC-132. New `mandate-research` (layer 5, safety-critical, pure); founder veto on the `layers.toml` and CODEOWNERS entries. Claim [#132](https://github.com/kunwarshivam/mandate/issues/132)
| E17-7 Vetted sources and corroboration | Brief open; tests PR next | [#133](https://github.com/kunwarshivam/mandate/pull/133) | Stream J. Checks 14 and 15 of §8.5 (DEC-101): every cited evidence source on the versioned allowlist, and corroboration by an independent source or by market data. DEC-132 item 7 reads DEC-101's "recorded in `ThesisProposed`" as platform-derived rather than agent-asserted, so a model cannot satisfy check 15 by writing `independent_source` into its own output; item 6 does the same for `asset_class` and the leveraged-ETP flag, which come from instrument reference data. Earning the corroboration fact (source independence and market-data consistency) stays E17-7 data-plane work, named under Not done |
| E17-9 Revision lineages (the fold only) | Brief open; tests PR next | [#133](https://github.com/kunwarshivam/mandate/pull/133) | Stream J. The §8.6 lineage fold: the revision cap, retirement on a journaled `lineage_retired` refusal, the instrument a retired lineage holds removed in the same fold step, an earlier check's refusal retiring nothing (MC-N27), and retirement never taking what another lineage now holds (MC-N28). No score is carried forward by construction (MI-18). The loop itself — proposing a revision, the autopsy, the `OwnerAlertSent` — waits on E17-8, per DEC-111 item 5 |
| E5-2 Artifact store | Merged | [#60](https://github.com/kunwarshivam/mandate/pull/60) (DEC-107 reservation), [#66](https://github.com/kunwarshivam/mandate/pull/66) (tests), [#84](https://github.com/kunwarshivam/mandate/pull/84) (implementation) | Pure core in `mandate-journal`, filesystem backend in the new `mandate-artifacts-fs` (DEC-107): write-once, hard-linked into place, re-hashed on every read; zero missed mutants; no reference cases, so no status PR |
| E5-4 Journal verification CLI | Merged | [#108](https://github.com/kunwarshivam/mandate/pull/108) | `claude-code`: `mandate journal verify <export> [--store] [--anchor] [--from-seq --trusted-prev-hash]` runs journal spec §11 in order over a §6.2 segment and reports the first failure with the spec's code and a non-zero exit; `mandate artifact put`/`get` over the filesystem store, the get re-hashed. Every tamper vector an export can express is replayed through the command; `column_altered` and `seq_values_swapped` are asserted inexpressible. Input refusals carry stable codes; an anchor never vouches for an export it cannot cover (two review findings). `mandate-journal` and `mandate-artifacts-fs` unchanged. Interpretations in DEC-115; [task brief](tasks/E5-4-verification-cli.md) |
| E2-3 Top-of-book quotes (Should) | Merged (both slices) | [#95](https://github.com/kunwarshivam/mandate/pull/95), [#107](https://github.com/kunwarshivam/mandate/pull/107) | Slice 1 (`cursor`, #95): `Quote`, `Kind::Quotes`, the Alpaca stock (`sip`, `iex`) and crypto quotes client, `Decimal128(38, 9)` partitions; locked, crossed, and one-sided quotes kept as sent. Slice 2 (`claude-code`, #107): `download --kind quotes`, and `inspect` statistics for a quotes dataset (each quoted side, the signed spread, and the locked, crossed, one-sided, and unquoted counts, DEC-116), so `InspectError::Unsupported` is gone; the marketdata RFC 3339 parser now delegates to `UtcNanos::parse_rfc3339`. Still open: a live quotes check in `tests/live.rs` and a quotes download in the M1 exit run, both needing market-data keys |
| E6-3 Independent risk gate (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | New `mandate-risk` (layer 4, pure, safety-critical): one pure function running trading-domain spec §9.1's eight checks in order, assigning the purpose itself, and reporting the first failing check's code. Earns MC-G01 to MC-G16, MC-F01 to MC-F04, and the `propose_order` steps of RC-03, RC-08 and RC-18. [Task brief](tasks/E6-3-risk-gate.md); interpretations in DEC-129. Claim [#123](https://github.com/kunwarshivam/mandate/issues/123) |
| E6-4 Daily loss and drawdown ladder (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | In `mandate-risk`, what the order path does with the ladder: `size_factor`, the `trim_to_target` proposals, and the mode effects on the gate. The MC-R, MC-T and MC-L families are `kind: risk_state`, `risk_day` and `goal` cases and belong to stream F's fold; the split is in the brief's Dependencies for the coordinator to confirm |
| E6-6 US account rules (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | Sessions, the v1 order policy, buying power (DEC-34, DEC-104, with the fee reservation the tracker's known-issues list assigned here), and the day-trading regime, whose `legacy_pdt` ledger is folded in `mandate-risk`. Earns RC-09, RC-09B |
| E6-7 Eligibility floor (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | Trading-domain spec §3.2's seven items in list order, fail-closed for an unclassified ETP. Earns RC-16 and its `leveraged_etps_enabled` variant |
| E6-8 Market-conduct controls (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | Trading-domain spec §9.6: the collar on aggressiveness only, resting time, participation caps, order-to-fill, the opposite-fill interval, the close window, self-trade prevention, and the daily surveillance report, which states figures and makes no judgement. Every control is a comparison against the supplied risk clock, so the crate reads no clock. Earns RC-22, RC-25 |
| E6-9 Restrictions and halts (stream G) | Brief PR #127 open | [#127](https://github.com/kunwarshivam/mandate/pull/127) | Trading-domain spec §7.3's account states and §4.4's halts at gate check 1 and check 3, so a blocked account holds even a risk exit while a restricted one still allows exits. Earns RC-15 and its three variants |
| E2-4 Sessions and corporate actions | Merged (all four PRs) | [#71](https://github.com/kunwarshivam/mandate/pull/71) (tests), [#85](https://github.com/kunwarshivam/mandate/pull/85) (implementation), [#74](https://github.com/kunwarshivam/mandate/pull/74) (corporate actions), [#96](https://github.com/kunwarshivam/mandate/pull/96) (`inspect` wiring) | `cursor`: the NYSE calendar 2018 to 2028 as data, four sessions per trading day, `session_at`, DST through the bundled tzdb; corporate actions adjusted through `mandate_num::SplitRatio::mark` and fetched by ex-date. #96: each missing bar slot classed as session closure, no trade, true gap, or unclassified by SIP and IEX venue hours checked in as data; `download` stores the corporate actions with the dataset; raw and split-adjusted prices in the report |
| E6-1 Agent runtime, E6-5 kill switches | Brief PR open | [#126](https://github.com/kunwarshivam/mandate/pull/126) | Stream I, the first M5 code stream. The runtime as a deterministic core (`fold` replays and emits nothing, `handle` is the only producer of effects, `resume` re-hands only what a crash left owed) plus a thin shell that is not built here; kill switches at agent, connection, and workspace scope apply the final mode first, cancel every pending approval, and hand exactly one flatten to `IntentSink`, the runtime itself cancelling and selling nothing. [Task brief](tasks/E6-1-agent-runtime-and-kill-switches.md); 20 interpretations in DEC-131, item 1 (the new layer-6 `mandate-runtime` entry in `xtask/layers.toml` and CODEOWNERS) proposed to the founder. Built on the DEC-17 recommendation, and interpretation 5 (the journal is the channel, a notification only a hint) is what keeps a NATS answer confined to the shell. No reference case moves: RC-14's `kill_switch` variant also needs E7-2's `actions` and E6-9's `agent_mode`, and the mandate suite's flatten family MC-F01 to MC-F04 is stream G's. Claim [#122](https://github.com/kunwarshivam/mandate/issues/122) |

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
| E4-1 simulated execution (backtest fill model) | `claude-code` | #58, #62, #105 | Merged (#61 brief, #75 tests, #111 implementation, #113 status); all three claims closed | `agent/e4-1-sim-tests-2` (#75), `agent/e4-1-sim-impl-2` (#111), `agent/e4-1-sim-status-2` (#113); the stood-down `agent/e4-1-sim-tests`, `-impl`, and `-status` branches await the founder's deletion |
| E4-2 baseline backtest and metrics report (stream E) | `claude-code` | [#110](https://github.com/kunwarshivam/mandate/issues/110), [#114](https://github.com/kunwarshivam/mandate/issues/114) (shared crate `mandate-num`) | Brief merged ([#112](https://github.com/kunwarshivam/mandate/pull/112), DEC-127); the DEC-77 tests PR is open ([#118](https://github.com/kunwarshivam/mandate/pull/118)): the new layer-7 `mandate-backtest`, the `mandate-num` additions, and 74 pending tests; the implementation PR follows | `agent/e4-2-brief` (#112), `agent/e4-2-tests` (#118) |
| E6-2 autonomy and the order builder (stream H) | `claude-code` | [#125](https://github.com/kunwarshivam/mandate/issues/125) | Brief PR [#128](https://github.com/kunwarshivam/mandate/pull/128) open, waiting on the merge coordinator; the DEC-77 tests PR follows on `agent/m5-builder-tests` with the new `mandate-builder` crate and the `mandate-num` additions of DEC-130 item 7 | `agent/m5-builder-brief` (#128), then `agent/m5-builder-tests` |
| E6-1 agent runtime and E6-5 kill switches (stream I) | `claude-code` | [#122](https://github.com/kunwarshivam/mandate/issues/122) | Brief PR [#126](https://github.com/kunwarshivam/mandate/pull/126) open, waiting on the merge coordinator; the DEC-77 tests PR follows on `agent/m5-runtime-tests` with the new `mandate-runtime` crate | `agent/m5-runtime-brief` (#126), then `agent/m5-runtime-tests` |
| E6-3, E6-4, E6-6 to E6-9: `mandate-risk`, the risk gate (stream G) | `claude-code` | [#123](https://github.com/kunwarshivam/mandate/issues/123) | Brief PR [#127](https://github.com/kunwarshivam/mandate/pull/127) open, waiting on the merge coordinator; the DEC-77 tests PR follows on `agent/m5-risk-tests` with the new `mandate-risk` crate (layer 4, founder veto on the `xtask/layers.toml` and CODEOWNERS entries) | `agent/m5-risk-brief` (#127), then `agent/m5-risk-tests` |
| Stream F: `mandate-spec` and `mandate-domain` (E10-1, E10-3, E6-4, E17-1) | `claude-code` | [#124](https://github.com/kunwarshivam/mandate/issues/124) | Brief PR [#129](https://github.com/kunwarshivam/mandate/pull/129) open, waiting on the merge coordinator; DEC-128 recorded there. The tests PR follows on the coordinator's go comment, proposed as three PRs against `main` (S/V/P, then R/T/L, then C) for the ES-13 size rule. Shared-crate additions: `mandate-num` (the exact comparison predicates and the one-rounding allocation scaling, reusing the `Ratio` and `Usd::ratio_to` that #118 landed), `mandate-accounting` (one line re-exporting `AssetClass`), `mandate-refcases` (the new `mandate` suite); `mandate-canon` is unchanged. The first review round corrected DEC-128 item 3 (`DecStr` normalises rather than rejects, so each decimal is checked against its own `$def` grammar) and settled five shared types with streams G and H as item 21 | `agent/m5-spec-brief` (#129), then `agent/m5-spec-tests` |
| Stream J: the E17 thin slice, `mandate-research` (E17-3, E17-7, the E17-9 fold) | `claude-code` | [#132](https://github.com/kunwarshivam/mandate/issues/132) | Brief PR [#133](https://github.com/kunwarshivam/mandate/pull/133) open, waiting on the merge coordinator; DEC-132 recorded there. The tests PR follows on the coordinator's go comment on `agent/m5-research-tests`, with the new `mandate-research` crate at layer 5 (founder veto on the `xtask/layers.toml` and CODEOWNERS entries). The family-N harness and its status rows are a later PR: they need stream F's `mandate` module and, for the three cases stating `first_order_autonomy`, stream H's `classify` | `agent/m5-research-brief` (#133), then `agent/m5-research-tests` |
| E17-0 research spike | `claude-code` | #49 | Merged (#44, #47); paper runs pending the founder's go | `python/research_spike/` |
| E5-3 Postgres journal | `cursor` | #77 | Merged (#82 tests, #92 implementation); DEC-109 recorded; claims #77 and #78 closed | `cursor/e5-3-pg-tests-e15e` (#82); implementation `cursor/e5-3-pg-impl-e15e` (#92) |
| Track C: mandate spec rewrite for DEC-97 to DEC-103 and DEC-111 | `claude-code` | #50, #103 | Merged ([#109](https://github.com/kunwarshivam/mandate/pull/109), no code, ES-22) after three review rounds: the ten rewrite questions answered as DEC-117 to DEC-126 (Proposed, the founder may veto after the fact); mandate spec v0.6, both schemas, the journal spec's event catalogue, the reference implementation, and 298 reference cases (from 215, every id kept). Claims closed. The Rust harness for the mandate cases follows in a later story | `agent/track-c-mandate-rewrite` (#109) |
| Direction follow-ups (DEC-99 to DEC-103, rewrite questions) | `cursor` | #54 | PR #57 reviewed PASS and rebased; the founder's confirmation of DEC-99 to DEC-103 pending | `cursor/direction-follow-ups-v2` (#57) |
| E2-4 market sessions and corporate actions | `cursor` | #68 (shared-crate claim #69 closed) | Merged (#71 tests, #85 sessions implementation, #74 corporate actions, #96 `inspect` wiring); claim closed; the M1 rehearsal's follow-ups are claim #116 | `cursor/e2-4-session-tests-ab3f` (#71), `cursor/e2-4-session-impl-ab3f` (#85), `cursor/e2-4-corporate-actions-ab3f` (#74), `cursor/e2-4-inspect-sessions-ab3f` (#96) |
| E2-2 dataset inspect | `cursor` | #56 | Merged (#64) | `cursor/e2-2-inspect-2749` (#64) |
| #55 `install.sh` without `astral.sh` | `cursor` | #63 | Merged (#65) | `cursor/install-no-astral` (#65) |
| `install.sh` follow-ups from the #65 review | `cursor` | #70 | Merged (#73) | `cursor/install-followup` (#73) |
| E5-2 artifact store | `cursor` | #59 | Merged (#66 tests, #84 implementation); DEC-107 recorded; claim closed | `cursor/e5-2-artifact-tests-b0be` (#66), `cursor/e5-2-artifact-impl-b0be` (#84) |
| `xtask`: pending tests must fail on stubs (shared crate) | `cursor` | #76 | Merged (#81; DEC-110 reserved in #83); claim closed | `cursor/xtask-pending-fail-7e3b` (#81) |
| Cursor allocation: next stories | `cursor` | #87 | Merged (#88); E5-4 and the M1 exit run's keys await the founder | `cursor/allocation-next` (#88) |
| `mandate-marketdata` safe concurrent dataset writes | `cursor` | #86 | Claimed | — |
| `mandate-time`: `UtcNanos` fractional seconds (shared crate, for E2-1) | `cursor` | #91 | Merged (#93 tests, #98 implementation, the latter cherry-picked onto main by the coordinator while Cursor was paused); claim closed | `cursor/utcnanos-fraction-tests-2652` (#93), `agent/utcnanos-fraction-impl` (#98) |
| E2-3 top-of-book quotes | `cursor` | #94 | Slice 1 merged (#95); claim closed | `cursor/e2-3-quotes-v2-a075` (#95); `cursor/e2-3-quotes-a075` superseded |
| E2-3 CLI slice and the RFC 3339 workaround retirement (stream B) | `claude-code` | #106 | Merged ([#107](https://github.com/kunwarshivam/mandate/pull/107)); DEC-116 recorded; claims #106 and #94 closed | `agent/e2-3-cli-quotes` (#107) |
| `mandate-marketdata`: safe concurrent dataset writes (E2-1 follow-up) | `cursor` | #86 | Merged (#90); claim closed | `cursor/marketdata-write-safety` (#90) |
| Thesis revision loop: DEC-111, E17-9, R-28 | `claude-code` | — | Merged (#99); the story waits on E17-8 | docs only |
| E5-4 journal verification and artifact commands | `claude-code` | #104 | Merged ([#108](https://github.com/kunwarshivam/mandate/pull/108)) after two review rounds; DEC-115 recorded; claim closed | `agent/e5-4-verify-cli` (#108) |
| CI short path for documentation-only changes (DEC-112) | `claude-code` | — | Merged (#100) at the founder's request; a docs PR now costs under two runner minutes | `.github/scripts/` |
| `mandate-marketdata`: proactive rate limiting in the Alpaca client (E2-1 follow-up) | `cursor` | [#115](https://github.com/kunwarshivam/mandate/issues/115) | Claimed 2026-09-26 19:20Z; the founder asked at 20:00Z to finish it (claim comment). One PR, tests first: [#121](https://github.com/kunwarshivam/mandate/pull/121) open for the merge coordinator's review; the claim closes when it merges | `cursor/marketdata-rate-limit` (#121) |
| `inspect` data-quality reporting (M1 rehearsal follow-up to E2-2 and E2-4) | `cursor` | #116 | PR #119 open, waiting on the merge coordinator's review: the IEX early-close evening closed from 17:00, records while the venue is closed, zero-volume and single-trade-spread warnings, inconsistent bars as problems, closed-day counts, the restatement note, and the live test behind `live-alpaca` | `cursor/inspect-quality-39ba` |

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
| GitHub Actions minutes: 90 percent of the month's 3,000 used by 2026-09-26; DEC-112 makes docs PRs cheap and DEC-113 turned the up-to-date rule off, so a merge no longer re-runs every open PR; the coordinator runs `cargo xtask check` on `main` after each code merge instead. GitHub creates no `pull_request` run for a PR that conflicts with `main`, so a branch behind a merge that touched the shared tables shows no checks until `main` is merged into it | Founder: raise or reset the Actions budget on 2026-10-01 |
| Postgres in the agent environment: `.cursor/install.sh` installs PostgreSQL 18 only where apt.postgresql.org is reachable, and `MANDATE_PG_URL` must be exported by hand (`environment.json` cannot set it) | E5-3 (CI has it: a service container in `full` and nightly) |
| Branches are named `cursor/...` because the agent environment requires it; ADR-0001 ES-13 says `agent/...` | Amend ES-13 at the next ADR touch |
| The repository is public since 2026-09-26 20:5xZ (the private-repo Actions quota ran out; history scanned, no leaks); interaction limits restrict issues, PRs, and comments to collaborators until 2027-03-26; fork workflows need approval; it goes private again after the quota resets on 2026-10-01 | Founder |
| `mandate-sim` accepts `Session::Continuous` bars for a `UsEquity` instrument, so an equity stop on continuous bars fills where spec §4.3 says such input cannot exist (DEC-114 item 2 discloses it); add a `SessionOffAssetClass` validation (Continuous only for crypto) in a tests PR | E4-2 tests PR, or the next `mandate-sim` story |
| `Bps::sqrt_impact` returns `too_precise` for an impact coefficient with more than 10 decimal places; the backtest config validation should bound the coefficient's scale, or the doc should say so | E4-2 |
| `mandate journal verify`: an unreadable export or anchor path is reported through anyhow context with no `Refusal` code (an I/O failure, exit non-zero); scripts that key on the first word need one there too | Next `mandate-cli` touch |
| `mandate inspect` on a quotes dataset with corporate actions applied and no row quoting any side prints no split-adjusted line and no reason | Next `mandate-cli` touch |
| The mandate reference cases (298, spec v0.6) have no Rust harness yet: the mandate crates that would read `mandate.yaml` do not exist; DEC-117 to DEC-126 stay Proposed (founder) until confirmed | The story that creates `mandate-spec` |
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
| A | E4-1 implementation PR (`mandate-sim`, `mandate-num`; test files only lose `pending E4-1` markers), then the status PR moving RC-10, RC-12, RC-19 to passing | #75 (merged) | DEC-114 | merged: [#111](https://github.com/kunwarshivam/mandate/pull/111), [#113](https://github.com/kunwarshivam/mandate/pull/113) |
| B | E2-3 CLI slice (`download --kind quotes`, quote statistics in `inspect`) and retiring `mandate-marketdata`'s own RFC 3339 parser for `UtcNanos::parse_rfc3339` | #95, #98 (merged) | DEC-116 | merged: [#107](https://github.com/kunwarshivam/mandate/pull/107) |
| C | E5-4: `mandate-cli journal verify` over an exported stream and its artifact store, and artifact put and fetch | E5-1 to E5-3 (merged); the founder's confirmation of E5-4 taken from the delegation | DEC-115 | merged: [#108](https://github.com/kunwarshivam/mandate/pull/108) |
| D | Track C: answer the rewrite questions as decisions proposed to the founder, then the mandate spec, schemas, reference implementation, and cases for DEC-97 to DEC-103 and DEC-111 (spec-change PR, no code) | nothing; the founder can veto any proposed answer after the fact | DEC-117 to DEC-126 | merged: [#109](https://github.com/kunwarshivam/mandate/pull/109) |
| E | E4-2 task brief and interpretations (docs only), then the DEC-77 tests PR for the baseline backtest and metrics in a new layer-7 `mandate-backtest` crate, plus the `mandate-num` metric arithmetic under shared-crate claim #114, then the implementation PR | #75, #111, #113 (all merged) | DEC-127 | brief merged: [#112](https://github.com/kunwarshivam/mandate/pull/112); tests PR open: [#118](https://github.com/kunwarshivam/mandate/pull/118) |
| Founder | Alpaca market-data keys in the Cursor environment (M1 exit run); paper and OpenRouter keys and egress in the cloud environment, then go for spike paper runs; counsel; branch cleanup; key rotation | | | waiting |

## Work graph, M5 (2026-09-26 night, Phase 1 starts)

Phase 0 is down to E4-2 (tests PR #118 in review) and the M1 exit run (founder keys). Phase 1 starts
now so that the agent trades a paper account as early as the plan allows: M5 (agent runtime and
risk) is the largest block, so its stories run as parallel streams, each in the DEC-77 shape (a
docs-only **brief PR** with interpretations first, then the **tests PR** with pending stubs, then
the **implementation PR**), reviewed on a different model and merged through the one queue. The
briefs run in parallel now; tests and implementation follow per stream as each brief merges. The
mandate spec v0.6 (#109) and its 298 reference cases are the contract; DEC-117 to DEC-126 stay
`Proposed (founder)` and a veto reopens the affected brief.

| Stream | Work | Depends on | Reserved IDs | State |
|---|---|---|---|---|
| F | `mandate-spec` (new, layer 3) and `mandate-domain` (new, layer 1): the mandate document as typed data, schema and semantic validation (families S, V), policy resolution and the runtime overlay (P), change classification and the version vector (C), risk state and limits, risk days, and goals (R, T, L); the Rust harness that reads `docs/specs/reference-cases/mandate.yaml` for those 202 cases, with every other family failing as "not interpreted until" its owning story. Family F is *agent flatten*, family L is *goals* (spec §11), and stream G's merged brief [#127](https://github.com/kunwarshivam/mandate/pull/127) owns MC-F01 to MC-F04, so this stream leaves them there (DEC-128 item 19) | #109 (merged) | DEC-128 | merged: [#129](https://github.com/kunwarshivam/mandate/pull/129); tests PR next (`agent/m5-spec-tests`) |
| G | `mandate-risk` (new, layer 4, pure): the gate over the working universe (family G), forced flatten (F), US account rules (E6-6), the eligibility floor (E6-7, RC-16), restrictions and halts (E6-9, RC-15), and the conduct controls of trading-domain spec §9.6 (E6-8); E6-3 and E6-4 acceptance | F's types (the brief may start from the spec; the tests PR waits for F's tests PR) | DEC-129 | merged: [#127](https://github.com/kunwarshivam/mandate/pull/127); tests PR next (`agent/m5-risk-tests`), on F's type names |
| H | `mandate-builder` (new, layer 5): autonomy classification (families A and B, E6-2) and the order builder of mandate spec §8 (combined score, sizing, clipping) | F's types, as for G | DEC-130 | merged: [#128](https://github.com/kunwarshivam/mandate/pull/128); tests PR next (`agent/m5-builder-tests`), after E4-2's implementation |
| I | E6-1 and E6-5: the agent runtime skeleton (one process per agent deployment, in-process `IntentSink` and `TimerSource` per ADR-0001 ES-20, journal-driven state, kill switches per agent, connection, and workspace) and the DEC-17 recommendation the runtime is built on | DEC-17 (proposed below, founder veto before the tests PR) | DEC-131 | merged: [#126](https://github.com/kunwarshivam/mandate/pull/126); tests PR next (`agent/m5-runtime-tests`) |
| J | E17 thin slice (DEC-103): the research-agent contract of mandate spec §8 as code shape (thesis, corroboration, admission through the eligibility floor, `max_instruments`, and the autonomy rules; family N: admission, lineage, expiry, stagger), the fixed research-basket universe, every admission `ask`, paper only, scorecards on (E15-3); the spike's lessons folded in | F and H briefs (the tests PR waits for their tests PRs) | DEC-132 | brief PR [#133](https://github.com/kunwarshivam/mandate/pull/133) open under claim [#132](https://github.com/kunwarshivam/mandate/issues/132): [task brief](tasks/M5-J-research-thin-slice.md), 21 interpretations accepted and 6 items in Decisions needed (three founder: the layer-5 placement with its layer-6 alternative, §8.4's `corroboration` wording, and check 5's unreachability under V-036 with V-037; three coordinator: one shared universe change reason with stream F, the two-crate family-N harness, and which stream closes E17-3). The crate never calls a model: a thesis arrives as typed input. Tests PR next on `agent/m5-research-tests` |
| E (continues) | E4-2 tests PR #118 in its fix round, then the implementation PR; closes Phase 0 with the M1 exit run | #112 (merged) | DEC-127 | tests PR merged: [#118](https://github.com/kunwarshivam/mandate/pull/118); implementation PR in progress (`agent/e4-2-impl`) |
| Founder | Confirm or veto DEC-117 to DEC-126 and the DEC-17 recommendation; approve the stream K routine (the Alpaca paper connector and idempotent executor brief, refused by the tooling's permission classifier; prompt held by the coordinator); market-data keys for the M1 exit run; paper and OpenRouter keys and egress for the spike paper runs; counsel; branch cleanup; key rotation; make the repository private again after 2026-10-01 and renew or drop the interaction limit then | | | waiting |

After M5: M6 (the Alpaca paper connector, idempotent intents, reconciliation, crash recovery) and
M7 (escalation v0) run the same way, and the Phase 1 exit is an agent trading an Alpaca paper
account unattended through a soak.

## Next, in order

1. **E4-2** tests PR (stream E, builder running: the `mandate-backtest` crate skeleton with the
   layers.toml entry, the `mandate-num` metric arithmetic under claim #114, every test
   `#[ignore = "pending E4-2"]`), then its implementation PR; both reviewed on a different model
   and merged through the one queue.
2. **M1 exit run** once the founder adds market-data keys to an agent environment; the Alpaca
   client's proactive rate limiting (claim #115) if the founder confirms that `cursor` claim.
3. **Journal:** the cold store, segment manifests, and the `segment_*` and `tsa_token_invalid`
   checks (E5 follow-ups).
4. The spec-only change for DEC-104 items 2 and 5 once the founder decides item 5.
5. **Founder:** confirm or veto DEC-117 to DEC-126 (the rewrite answers the spec v0.6 is built on)
   and DEC-99 to DEC-103; confirm the `mandate-backtest` layer-7 entry (DEC-127 item 1).
6. **M5** starts with decision DEC-17 (messaging) and the Rust harness for the 298 mandate reference
   cases, and adds E17 (research agent and dynamic universe), starting with the DEC-103 thin slice:
   the team's internal paper workspaces with the research basket as the fixed test data universe,
   every admission `ask`, paper only, scorecards on (E15-3), and the forward-paper evaluation
   (E17-8). The full E17-3, for users' agents under their own envelopes, follows only after that
   evaluation passes. **E17-9**, the thesis revision loop, follows E17-8 and one completed
   evaluation ([DEC-111](04-decision-log.md#decisions)); it is never built against backtests.

Streams that touch different crates run in parallel; reviews and merges run one at a time.

## How the work runs

- Builders follow `.cursor/skills/mandate-mode/` (story, spec-change, correction, and ship
  playbooks) and prove work with `.cursor/skills/verify-mandate/`.
- Safety-critical stories ship as the DEC-77 sequence: tests PR, implementation PR (test files only
  lose pending markers), status PR.
- Each PR merges only after green CI and a pass from an independent review agent on a different
  model (DEC-79). Day 1's reviewers ran on GPT-5.6 Sol and failed five PRs before they merged; day
  2's ran as cloud routines on Claude Opus 5 for Cursor-authored PRs and on Claude Fable 5.1 for
  Opus-authored ones, and failed five rounds before eleven PRs merged.
