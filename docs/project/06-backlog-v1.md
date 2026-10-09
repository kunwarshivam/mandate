# Backlog: v1

| | |
|---|---|
| **Owner** | Product and project management |
| **Status** | Draft v0.1 |

Epics map to [milestones](02-milestones-and-wbs.md) and [PRD](../product/04-prd-v1.md)
requirements. Priority uses MoSCoW: **Must**, **Should**, **Could**, **Won't (v1)**.
Stories follow "As a … I want … so that …" with acceptance criteria.

## Epic overview

| Epic | Milestone | PRD | Priority |
|---|---|---|---|
| E1 Foundations | M0 | — | Must |
| E2 Market data | M1 | 6.4 | Must |
| E3 Accounting | M2 | 6.4 | Must |
| E4 Simulated execution and backtest | M3 | 6.4 | Must |
| E5 Journal | M4 | 6.7 | Must |
| E6 Agent runtime and risk | M5 | 6.5 | Must |
| E7 Alpaca connector and recovery | M6, M8 | 6.2, 6.5 | Must |
| E8 Escalation and approvals | M7, M10 | 6.6 | Must |
| E9 Identity, tenancy, and policy | M8 | 6.1 | Must |
| E10 Mandate authoring | M8, M9 | 6.3 | Must |
| E11 Web app: dashboard and controls | M9 | 6.8 | Must |
| E12 Audit explorer | M9 | 6.7 | Must |
| E13 Hybrid deployment | M11 | 6.9 | Must |
| E14 Billing | M12 | 6.10 | Must |
| E15 Signal models: LLM and fast models, scorecards | M5 (LLM research, scorecards); Phase 3 (fast models) | 6.3, 6.5 | Must (E15-1, E15-3, E15-6 to E15-10); Should |
| E16 Kraken Derivatives US connector | Phase 3 | 6.2 (FR-2.5) | Should |
| E17 Research agent and dynamic universe | M5 | 6.3 (FR-3.9), 6.5 | Must |
| E19 Agent harness | M5 (construction, the research loop, the bench); before live trading (E19-8) | 6.3 (FR-3.9), 6.5 (FR-5.1) | Must (E19-1 to E19-6, E19-8, E19-10); Should |

## Stories

### E1 Foundations

- **E1-1 (Must)** As an engineer, I want a Rust workspace and Python package with CI so that
  every change is built, linted, and tested.
  *Accepted when:* CI runs build, tests, and lint on every push; main is protected.
- **E1-2 (Must)** As an engineer, I want an ADR template and coding conventions so that
  decisions and code stay consistent.
- **E1-3 (Should)** As an engineer, I want the pending-test gate to read a failure's *cause* rather
  than its text, so that no wording in a test can stand in for the stub it is meant to reach
  (DEC-137, gap 1's remainder; the coordinator's round-2 ruling item 4).
  *Accepted when:* `cargo xtask ci pending` accepts only the `Err` value printed after
  `called \`Result::unwrap()\` on an \`Err\` value:`, the `todo!`/`unimplemented!` panic line, or the
  `Err(..)` `Debug` inside a proptest failure, and a marker written into a test's own assertion
  message no longer satisfies it; `BEHAVIOUR_ONLY_TESTS`, which has had no `mandate-risk` row
  since E6-8 (DEC-163), is retired as E7-4 lands, or replaced by a rule that reads the cause; and the planted cases of #172's reviews all fail the
  gate. Since DEC-164 a failing property is already read by proptest's report of its minimal
  failure alone, and `mandate-executor`'s three E7-4 properties that passed only on a stub report
  from a case shrinking moved past have their rows; E1-3 narrows that report, and every other
  test's output, to the cause.
- **E1-4 (Should)** As an engineer, I want `shellcheck` over `.github/scripts/` and `actionlint`
  over `.github/workflows/` in `cargo xtask ci lint`, installed at pinned versions by `install.sh`
  and CI, so that a shell or workflow mistake in the merge path (DEC-175) fails a check rather
  than waiting for a reviewer (the #316 reviews).
  *Accepted when:* both run in `ci lint`, pinned in `.github/workflows/ci.yml` and `install.sh`,
  and a planted `SC2086` or an unknown workflow key fails the job.
  *Follow-up* (#434's round-2 review, minor m1, backlogged under the freeze rule): three `xtask`
  tests, each failing on the plant that review left uncaught. One fails when `lint_paths` skips a
  missing `shellcheck` or `actionlint` binary silently (the `let Ok(out) = … else { return Ok(()) }`
  plant), for each tool; one fails when `shellcheck_scripts` loses its empty-file-list guard; and
  one fails when `actionlint_workflows` loses its empty-file-list guard.
  *Follow-up (#450 review, minor 1; DEC-381):* the lint job's fixture test (the one #434 added
  for shellcheck and actionlint) also fails when `workspace_lint` discards `proptest_seeds()`'s
  result, or when that check's git call's error is swallowed. Today `let _ = proptest_seeds();`
  passes every xtask test, as `let _ = markers();` and `let _ = feature_map();` do.
- **E1-5 (Should)** As an engineer, I want the merge script's remaining gaps from #316's
  round-3 review closed, so that the only path from approval to `main` (DEC-175) is tested as
  GitHub actually answers it. The items:
  - The stub `gh` serves only the first page unless `--paginate` is passed, and a case lists
    `web.yml` past file 100.
  - A fixture lists workflow runs newest first, as GitHub does, so that `.[-1]` in place of
    `max_by(.id)` fails.
  - A merge GitHub refuses is a skip, not a failed job. That covers a sweep and a per-PR run racing
    after one of them has merged, a ruleset block, and a head that moved between the read and the
    merge call.
  - The approval line is not read inside an HTML comment, an indented code block (four or more
    leading spaces; a bullet indented up to three spaces is still read, as DEC-175 allows), or a
    four-backtick fence.
  - The web path rule comes from `web.yml`'s `paths` filter, not a second copy, and a file renamed
    out of `web/` counts by its `previous_filename` too.
  - Optionally, the latest `labeled coordinator-approved` event must be newer than the head
    commit, so that a description line alone approves nothing.

  *Accepted when:* each item has a refusal or merge case in `xtask`'s merge-script tests, and each
  fails when its fix is reverted.
- **E1-6 (Should)** As an engineer, I want the reference-case harness cheaper to run, so that
  [DEC-498](decisions/DEC-498.md) item 3's 180-second per-mutant cap can come down and the matrix
  with it. DEC-497 runs the whole of `mandate-refcases` for any mutation of a crate it is built
  on, which is what puts a core crate's mutant test phase at the 83 seconds DEC-498 measured; the
  cap is sized on that, and the shard count on the cap. On a shard whose own mutants are in
  `mandate-refcases` the harness is the unmutated baseline's test phase as well, since
  cargo-mutants picks the baseline's packages from the slice's mutants — so it shortens that
  capped phase too, but not DEC-498's remainder, which is the baseline's *build* and the setup
  around it. It does nothing for a shard with no mutant to test.
  *Accepted when:* `cargo nextest run -p mandate-refcases` on a warm build takes at most half the
  time it takes on the same machine before the change, measured as the median of three
  consecutive runs each way — the baseline for comparison, not a target, is the 31.7, 31.8 and
  31.9 seconds three such runs took on a development VM when this story was written, and the
  runner is slower; the same 691 tests run, so the saving comes from the harness and not from
  running fewer cases; and DEC-498's 180-second cap and its 192 shards are re-derived from a
  fresh `ubuntu-24.04` measurement, or a successor decision records that they stand.
- **E1-7 (Should)** As an engineer, I want `xtask`'s own checks under the mutants gate, so that a
  safety check written in `xtask`, such as X1's live-feature check (E7-26, DEC-529 item 3), cannot
  lose a rule without a test failing. `xtask/layers.toml` marks `xtask` `safety_critical = false`,
  so `cargo xtask ci mutants` never mutates it today; X1's mutants were run by hand on its diff
  (#738 review, finding 3).
  *Accepted when:* CI runs `cargo mutants -p xtask` on the diff of every PR that changes
  `xtask/src`, as a shard within DEC-464's ten-minute budget, and fails on any missed mutant; the
  mutants of `xtask` code that predates the job are either caught or listed, each with its reason,
  in a follow-up story, so the job starts green.

### E2 Market data

- **E2-1 (Must)** As a researcher, I want to download historical bars and trades for US stocks,
  ETFs, and crypto (starting with a stock/ETF basket and BTC/USD) for a date range so that I can
  backtest.
  *Accepted when:* `download` fetches Alpaca historical data into Parquet; re-running is idempotent.
- **E2-4 (Must)** As a researcher, I want corporate actions (splits, dividends) and market
  sessions recorded with the data so that stock history and gaps are interpreted correctly.
  *Accepted when:* `inspect` distinguishes session closures from true gaps; split-adjusted and
  raw prices are both available.
- **E2-2 (Must)** As a researcher, I want to inspect a dataset for coverage, gaps, duplicates,
  and summary statistics so that I trust it before using it.
  *Accepted when:* `inspect` reports gaps with exact timestamps; tests cover gap and duplicate detection.
- **E2-3 (Should)** As a researcher, I want order-book top-of-book data so that slippage models
  can use spreads.

*The live data plane* ([data plane spec](../specs/data-plane.md), [DEC-433](decisions/DEC-433.md)).
Stories marked SC are safety-critical (connectors, and anything the risk gate reads). Each names
the spec invariants (DP-n) its tests cover.

- **E2-5 (Must, M6; SC)** As an owner, I want my agents to receive live quotes, trades, bars,
  trading statuses, and LULD bands from my own Alpaca connection, so that marks and halt checks are
  current (PRD FR-2.7).
  *Accepted when:* the stream client connects, authenticates, subscribes, and reconnects with
  capped backoff against recorded fixtures with no network; the connection states of spec §3.2 hold,
  with openings blocked outside `live`; every gap window is recorded and backfilled; duplicates,
  out-of-order quotes, and corrections follow §3.3; the channels each feed carries are verified
  first (spec §11 question 1); DP-4, DP-5, DP-7 pass under fault injection.
- **E2-6 (Must, M6; SC)** As an owner, I want one workspace data service that normalizes market
  data, stamps receive times, and keeps a bounded hot cache, so that every runtime and the executor
  read the same current prices.
  *Accepted when:* records are spec §3.1's types with `vendor_time` and `received_at`; quote age and
  skew follow DEC-433 item 5; instrument status follows §3.5; conflation never drops status, LULD,
  or correction messages; the subscription set and its overflow follow §3.2; a journal spec PR adds
  the quote's vendor and receive times to `MarkUpdated` (spec §11 question 2); DP-3 and DP-10 pass.
- **E2-7 (Must, M5)** As a researcher, I want the point-in-time store to answer "as of" queries by
  knowledge time, with corrections and revised history as new versions, so that no backtest or
  research query sees the future.
  *Accepted when:* every new dataset kind has a knowledge time (spec §5.2); revised history is a new
  dataset version and the old one stays readable; corrections are records naming the original; a
  property test against an independent oracle shows DP-1 and DP-2; a backtest records its snapshot
  digest and reruns bit for bit (DP-14).
- **E2-8 (Must, M6)** As an owner, I want the eligibility floor's inputs (prior close, 20-day median
  dollar volume, ETP classification) computed point in time and versioned, so that the floor decides
  on data it could have known.
  *Accepted when:* the statistics come from daily bars with knowledge time before the decision and
  appear in the gate's `checks` inputs; the ETP list's source is decided (trading spec §15 items 5
  and 8) and an over-age list denies ETP openings; the calendar end alert fires 90 days ahead.
- **E2-9 (Must, M5)** As the research agent's owner, I want SEC filings and XBRL facts ingested from
  EDGAR with deterministic tagging, so that the agent reads primary documents without a paid vendor.
  *Accepted when:* fetches reach only allowlisted endpoints and respect the SEC's fair-access rules;
  raw bytes are stored by digest before parsing; tags come from filer CIK through a versioned table,
  resolved as of knowledge time; amended filings and restated facts are new records (DP-1, DP-2,
  DP-8, DP-9, DP-12).
- **E2-10 (Must, M5)** As an owner, I want news read through my own Alpaca connection, de-duplicated
  and grouped by story, so that one story repeated by many outlets counts once.
  *Accepted when:* vendor updates are linked versions; syndicated copies share a group key (spec
  §4.4) and the corroboration check counts a group once (DEC-433 item 14, with mandate spec §8.5
  check 15); prompt-injection fixtures in news text never reach an order (DP-8, E17-7).
- **E2-11 (Must, M5)** As an owner, I want the research agent to read data only through one as-of
  query interface that journals what it returns, so that every thesis's inputs are on the record.
  *Accepted when:* spec §4.6's parameters are enforced; every returned item is journaled as
  `ObservationRecorded` before the model reads it; the same observations feed the E17-5 drift
  detector; a query never returns an item from outside the allowlist version (DP-1, DP-3, DP-9).
- **E2-12 (Should, M11)** As an operator, I want the shared plane to publish signed whole-dataset
  bundles that cells and hybrid sites pull, and an offline bundle for air-gapped sites, so that
  public data is computed once without learning any workspace's interests.
  *Accepted when:* bundles carry a manifest and signature verified before use; nothing
  workspace-specific reaches the shared plane in a two-workspace test (DP-10); a dataset without a
  recorded redistribution grant cannot publish (DP-11); no record has a directional field (DP-12).
- **E2-13 (Must, M7; SC)** As the founder, I want a data-plane fault-injection suite that walks every
  row of spec §7 to its exit, so that feed failures are proven safe, not assumed.
  *Accepted when:* each row of §7 is a named test; with the shared plane and every news source down
  the exit suites still pass (DP-13); one outlier quote anywhere in a random sequence changes no
  latched limit, trim, flatten, or lifted restriction, and no admitted opening order's value, each
  compared by an oracle with its own equity ledger (DP-6). The H and E₀ cases are pending tests on
  DEC-433 items 17 and 21, and the admitted-value case on item 22; none is left out.
- **E2-14 (Must, after DEC-433 items 17, 21, and 22; SC)** As an owner, I want one wrong high print
  never to set my high-water mark or my day's starting equity, or to enlarge a buy, so that later
  real prices cannot confirm a loss that did not happen and no order is sized on a wrong price.
  *Accepted when:* the founder rules on items 21 and 22; the mandate spec and its reference cases
  change in their own PR first; DP-6's pending cases pass. *Note (#553 review, finding 3 and minor
  10):* item 17's E₀ mechanism cannot work as drafted. If the founder takes item 21's official-close
  option, the E₀ criterion becomes "E₀ is set from the official close, with the last confirmed mark
  as the fallback", and the H criterion follows whichever H option is chosen.
- **E2-15 (Must, M5, before E2-10 is accepted; SC)** As an owner, I want mandate spec §8.5 check 15
  to count syndicated copies of one story as one source, so that a planted story repeated by several
  vetted outlets cannot corroborate itself at admission (DEC-433 item 14; #553 review, finding 6).
  *Accepted when:* a mandate spec and reference-case PR states the rule (a tightening, DEC-176);
  then tests first; then `crates/mandate-research`'s check 15 counts story groups, not raw cited
  sources; a fixture with one story under five vetted sources is refused `no_corroboration`.

*Data plane spec follow-ups* (minors of the post-merge review on
[#553](https://github.com/kunwarshivam/mandate/pull/553), deferred by the freeze rule):

- **E2-16 (Should)** Minor 7: spec §3.2's overflow report and §7's owner alerts say they go through
  the notification path under rule 6 (opaque IDs and generic text; details load in the workspace).
- **E2-17 (Should)** Minor 8: make DEC-433 item 6 visible from the trading spec: amend §4.4 so a
  presumed halt also blocks openings, or extend the data plane spec's §11 question 4 to cover it.
- **E2-18 (Should)** Minor 9: name the backlog story per invariant in spec §2 (DP-11, DP-12, and
  DP-14 name a test kind but no story).
- **E2-19 (Should)** Minor 11: name the story that holds the allowlist's contents (E17-7 or E2-9);
  nothing holds them today.

### E3 Accounting

- **E3-1 (Must)** As a trader, I want positions, cash, fees, and realized and unrealized P&L
  computed correctly so that every later number is right.
  *Accepted when:* property-based tests and hand-calculated cases pass, including partial fills
  and position flips.
- **E3-2 (Must)** As a trader, I want splits and dividends applied to positions and cash so that
  stock P&L is correct.
- **E3-3 (Must)** As a trader, I want settlement tracked for cash accounts so that the system
  knows which cash is available to trade.

### E4 Simulated execution and backtest

- **E4-1 (Must)** As a researcher, I want market and limit orders filled with configurable
  slippage and fees so that backtests are realistic.
- **E4-2 (Must)** As a researcher, I want a baseline strategy and a metrics report (return,
  volatility, Sharpe, maximum drawdown, turnover, fees, buy-and-hold comparison) so that the
  loop is proven end to end.
  *Accepted when:* identical inputs produce identical outputs.
- **E4-3 (Must)** As a researcher, I want the fill model to refuse a bar labelled with a session its
  instrument's asset class never trades, so that no backtest fills on input spec §4.3 says cannot
  exist (the gap DEC-114 item 2 discloses; [DEC-377](decisions/DEC-377.md)).
  *Accepted when:* crypto bars carry only the continuous session and US-equity bars only the four
  New York sessions; any other bar is refused with `session_off_asset_class` naming the first such
  bar's index; an empty bar sequence is accepted; `simulate` runs the check after the order-policy
  checks, so an order the v1 policy refuses keeps its own cause; and every sequence the table allows
  fills exactly as before. Tests PR [#437](https://github.com/kunwarshivam/mandate/pull/437)
  (merged), then the implementation PR, which replaces the stub's body, adds the call, retires the
  stub's `SimError::Unimplemented`, deletes only the pending tests' `#[ignore]` lines, and adds one
  assertion each on the reason code and the message ([DEC-386](decisions/DEC-386.md)).

### E5 Journal

- **E5-1 (Must)** As an auditor, I want every event appended to a hash-chained journal with
  causal links so that history cannot be silently altered.
  *Accepted when:* the [journal test vectors](../specs/reference-cases/journal.yaml) (chain, string
  escaping, decimals, export line, Merkle anchor) reproduce byte for byte; the verification tool
  reports each tamper case's expected first failure; and the append protocol returns each append
  case's expected outcome ([journal spec](../specs/journal.md)).
- **E5-3 (Must)** As an operator, I want the Postgres journal hardened (body stored as exact
  canonical bytes with a hash check, append-only roles and triggers including TRUNCATE, stream
  heads with writer fencing) so that records cannot be altered by application code.
- **E5-2 (Must)** As an engineer, I want large artifacts stored by content hash so that the
  journal stays small and verifiable.
- **E5-5 (Must)** As an owner, I want my personal data (the broker's account number and account
  id, names, emails) held in the workspace's personal-data vault under a per-person key, with each
  journal event carrying only random vault references in `pii_refs`, so that the records can be
  kept and my data erased separately ([journal spec](../specs/journal.md) §3, §6.4). It replaces
  DEC-142's interim: `mandate-alpaca`'s record discards a redacted value and labels it by position.
  *Accepted when:* a recorded exchange's redacted values are in the vault, its `pii_refs` are the
  vault's random references passed into the record as an input (ES-21), sorted, and none is a hash
  of the value.
- **E5-4 (Must)** As an auditor, I want `mandate-cli journal verify` over an exported stream and
  its artifact store, and a CLI command to put and fetch artifacts, so that I can check a journal
  without writing code (M4's verification tool; deferred from the E5-1 and E5-2 briefs).
  *Accepted when:* for each [tamper case](../specs/reference-cases/journal.yaml) its input can
  express, the command reports the expected first failure; a deleted or altered artifact reports
  `artifact_missing` or `artifact_mismatch` ([journal spec](../specs/journal.md) §11).
- **E5-6 (Must)** As an auditor, I want closed journal ranges held as cold-store segments with
  manifests, and the per-range verify checks, so that records can be checked years later without
  the hot store ([journal spec](../specs/journal.md) §6.2, §11, §12). The pure half is complete:
  `mandate-journal-cold` — tests [#391](https://github.com/kunwarshivam/mandate/pull/391),
  implementation [#404](https://github.com/kunwarshivam/mandate/pull/404), the E5-6a property
  suite [#407](https://github.com/kunwarshivam/mandate/pull/407); DEC-263 to DEC-265 and DEC-287.
  The operational halves are E5-7, the CLI wiring E5-8, the examination bundle E5-9, and RFC 3161
  signature verification stays Proposed in DEC-265 item 1 (a crypto dependency; the founder's
  call).
  *Accepted when:* the manifest's canonical form and hash, the segment walk's checks in DEC-264's
  order, the timestamp token's structural check, and the canonical export's digest are pure
  functions over `mandate-journal`'s own walks, fail closed, and pass their hand and property
  suites.
- **E5-6a (Should)** As an auditor, I want the cold store's checks held to their invariants over
  random segment sequences, not only the hand-calculated cases, so that the walk cannot drift
  between the cases (E5-6's follow-up, owed by AGENTS.md's "property-based tests for invariants"
  and deferred by #404's review; a DEC-77 tests PR on claim
  [#374](https://github.com/kunwarshivam/mandate/issues/374), merged as
  [#407](https://github.com/kunwarshivam/mandate/pull/407)).
  *Accepted when:* proptest drives `mandate-journal-cold` over random segment sequences —
  contiguous runs split at random boundaries, tampered files, lying manifests, gaps and
  overlapping copies, mid-segment entries — and an independent oracle (its own accumulator,
  never the implementation's walk) checks the invariants: a verified range's state is the
  trusted start advanced by exactly the events inside it; every tamper is refused by the check
  that owns it, at the `seq` the range expected; nothing before the trusted start is checked;
  and the token entry point never answers `Ok` (DEC-263 to DEC-265).
- **E5-7 (Must)** As an operator, I want closed segments shipped to object storage with their
  manifests and retention enforced, so that trading records survive the hot store and meet the
  6-year write-once requirement (FR-7.6; [journal spec](../specs/journal.md) §6.2; E5-6's
  operational follow-up). Phase 0 ships the export job: a segment closes (daily is acceptable in
  Phase 0; within 1 minute of closing before live capital), its file and manifest are written to
  object storage, and `SegmentExported` carries the manifest hash (DEC-263 item 3). Before live
  capital: object lock in compliance mode with a 6-year retain-until, the `RetentionExtended` job
  while a supported position, lot, or account remains open, `LegalHoldChanged`, a second-region
  replica, and a quarterly restore-and-verify drill, each journaled; a later evictor may delete
  only hot rows with `seq ≤` the last verified cold segment, journaling `SegmentEvicted` (§6.1).
  Safety-critical (the journal write path); an object-storage dependency row is owed (none by
  default — the founder's call), DEC-77 sequence.
  *Accepted when:* a closed range exports, re-imports, and verifies through E5-6's checks end to
  end; retention extends and a legal hold blocks deletion, both journaled; the drill verifies a
  replica; and no hot row above the last verified cold segment is ever deleted.
- **E5-8 (Should)** As an auditor, I want `mandate journal verify` to check a cold-store export —
  a directory of segment files and manifests against an anchor and a trusted start — so that I
  can verify long-lived records without writing code (FR-7.5; E5-4's command over one exported
  stream is the hot-store half; E5-6's checks through the CLI, which the E5-6 brief left to "the
  CLI calls them when its story says"). `mandate-cli` is safety-critical, DEC-77 sequence. In
  progress on claim [#645](https://github.com/kunwarshivam/mandate/issues/645): DEC-490, the
  [task brief](tasks/E5-8-cold-verify-cli.md), and the tests PR
  ([#654](https://github.com/kunwarshivam/mandate/pull/654), merged; it replaced #651, whose
  description the coordinator could not write the approval line into). The implementation PR
  ([#662](https://github.com/kunwarshivam/mandate/pull/662), DEC-77 stage 2) is open; it
  deletes the 38 `#[ignore = "pending E5-8"]` lines and changes no other test line.
  *Accepted when:* the command runs §11's per-range checks through `mandate-journal-cold` over a
  cold export, reports the first failure with its code and a non-zero exit, never vouches for an
  export it cannot cover, and never answers `Ok` for a timestamp token until DEC-265 item 1's
  crypto half lands.
- **E5-9 (Should)** As an auditor, I want an examination bundle scoped by account, agent, and
  period — every related stream (account, agent, control, scheduler), the referenced
  configuration objects and artifacts, schemas and upcasters, the verifier release and format
  specification, an index, and, for authorized requests, resolved identities — with a
  deterministic human-readable report of its own hash, so that an examination can be answered
  from the bundle alone ([journal spec](../specs/journal.md) §12's second bullet; E5-6's
  canonical export is the bundle's per-stream part). Resolved identities need E9's identity
  records, and the production deadline and retention values are journal spec §13's open
  question 4.
  *Accepted when:* a bundle is produced within the configured deadline, its report reproduces
  byte for byte from the same inputs, and every part verifies through the cold checks.
- **E5-10 (Should)** As an auditor, I want every closed record's step-up evidence and times typed
  one way, so that a reader never has to know which record wrote integer risk-clock seconds and
  which a §4.7 timestamp ([DEC-533](decisions/DEC-533.md) item 2). Journal spec v0.17's §9.7
  writes `authenticated_at`, `submitted_at` and `effective_at` as integer seconds, while §9.2's and
  §9.3's step-up evidence writes a timestamp. The fix is a version bump of the records on one side,
  with their writers and readers, and the old versions kept registered (§8).
  *Accepted when:* one type serves every step-up's `authenticated_at` at the records' newest
  versions, and the vectors show the old versions still read.

### E6 Agent runtime and risk

- **E6-1 (Must)** As an operator, I want an agent to run a mandate continuously so that it
  trades without supervision.
- **E6-2 (Must)** As an operator, I want every proposed action classified AUTO, ASK, or DENY
  per the mandate so that autonomy matches my rules. *Accepted when:* mandate reference cases
  MC-A01 to MC-A11 and MC-B01 to MC-B29 pass.
- **E6-3 (Must)** As an owner, I want an independent risk gate enforcing all limits so that no
  agent logic can exceed them.
  *Accepted when:* simulation fuzzing across random market paths and mandates never produces
  an order outside limits; the mandate invariants MI-1 to MI-11 hold under property-based tests;
  mandate reference cases MC-G01 to MC-G13 and MC-F01 to MC-F04 pass.
- **E6-4 (Must)** As an owner, I want a daily-loss limit and a drawdown ladder so that losses
  trigger automatic de-risking. *Accepted when:* MC-R01 to MC-R15, MC-T01 to MC-T05, and MC-L01
  to MC-L09 pass.
- **E6-5 (Must)** As an owner, I want kill switches per agent, connection, and workspace so
  that I can stop everything immediately.
- **E6-6 (Must)** As an owner, I want US account rules (day-trading regime, settlement, short
  sales, market hours) enforced by the risk gate so that agents never get my account restricted.
  *Accepted when:* simulation tests for each rule pass; blocked orders are journaled with the rule.
- **E6-7 (Must)** As an owner, I want an instrument eligibility floor (exchange-listed, no OTC or
  IPO-day, price and liquidity floors, leveraged ETPs only with opt-in) so that agents stay in
  liquid, suitable instruments. *Accepted when:* RC-16 passes.
- **E6-8 (Must)** As an owner, I want market-conduct controls (one working order per side,
  minimum resting time, price collars, participation caps, order-to-fill limits, close-window
  rules, workspace self-trade prevention) and a daily surveillance report, so that agents cannot
  produce manipulation-like patterns. *Accepted when:* the gate's checks 5 and 6, the pacing of an
  allowed exit, the cancel rule and the surveillance report pass their `tests/` suites and in-module
  boundary tests with zero missed mutants (#311, DEC-163). RC-22 and RC-25 cannot run yet: the
  trading-domain harness has no gate driver, RC-25's steps carry no `quote` or volumes, and RC-22
  also needs E7-2, E7-4 and E6-11; each passes when those land (the "RC-22 and RC-25, blocked in the
  trading-domain harness" follow-up).
- **E6-9 (Must)** As an owner, I want account restrictions and trading halts checked before every
  order, so that agents stop adding risk when the broker restricts the account. *Accepted when:*
  RC-15 passes.
- **E6-10 (Must)** As an owner, I want the gate to admit crypto **USD pairs only** (trading-domain
  §3.2 item 7), so that an agent cannot open a stablecoin-quoted pair the floor was never written
  for. `AssetId` is a UUID, so the quote currency is its own input,
  `InstrumentSnapshot::quote_currency`, where only a stated USD admits (DEC-254). Until the
  implementation reads it the gate keeps a crypto opening owed at check 2 and refuses it
  fail-closed (DEC-129 item 34). *Accepted when:* a crypto opening in a non-USD pair is denied, a
  USD pair passes the floor, and check 2 is whole for crypto (`crates/mandate-risk/tests/usd_pairs.rs`).
  *Done:* check 2 in `mandate-risk` (#390, #394); the trading-domain harness decides crypto
  proposals (#419, DEC-285; RC-09 and MC-B26 to MC-B28 passing in #422), and refuses a short in
  its day-trade fold and reads `crypto_status` (#438 staged, DEC-314, DEC-315; implemented under
  DEC-395).
- **E6-11 (Must)** As an owner, I want the daily surveillance report delivered to me and a conduct
  breach to move the agent to `exits_only`, so that §9.6's "breach → agent `exits_only`" and its
  "threshold breaches are routed to the owner, whose acknowledgment is journaled" hold. E6-8 computes
  the report (`mandate_risk::surveillance`) and denies the breaching opening; nothing consumes
  either yet. The runtime owns the mode transition (mandate §5.9), and the notification carries only
  opaque IDs (`AGENTS.md` rule 6). *Accepted when:* an order-to-fill breach switches the agent to
  `exits_only` with an owner alert, risk-reducing orders continue, the day's report is journaled and
  routed per workspace, the owner's acknowledgment is journaled, and RC-22's `conduct_breach` step
  passes.
- **E6-12 (Must, before E10-8)** As an owner, I want an order my connected agent asked for always to
  reach me, and asks capped per day, so that a prompt-injected or looping agent can neither trade
  unasked nor wear me down ([ADR-0003](../adr/0003-earned-autonomy.md) part 10, [DEC-185](04-decision-log.md#decisions), [DEC-195](04-decision-log.md#decisions)). The client ceiling is
  mandate spec §6.2 step 5a and MI-30 ([#328](https://github.com/kunwarshivam/mandate/pull/328)); the
  per-client ask budget waits on its spec change (MI-33, [DEC-251](04-decision-log.md#decisions)). Safety-critical (autonomy policy), DEC-77 sequence.
  *Accepted when:* `requested_by` is set from the authenticated channel and journaled in
  `DecisionMade`; a client-requested opening is `ask` under every `auto` rule, `auto` default, live
  delegation, and `auto` admission, and a `deny` still denies; owner and agent requests decide as
  before; past 10 client-requested asks per client per risk day (the owner may lower it), further
  asks from that client are suppressed as `client_budget` and journaled, never notified, on top of
  §6.4's per-agent budget of 10; risk-limit alerts are never capped.
- **E6-13 (Should)** As an owner, I want tripwires I set in advance to end my delegations or hold new
  openings when their condition is met, so that trust does not outlive the conditions I gave it
  under ([DEC-187](04-decision-log.md#decisions)). The spec change is MI-31, V-044, §6.7, and the MC-W cases
  ([DEC-350](decisions/DEC-350.md) to [DEC-352](decisions/DEC-352.md); claim [#439](https://github.com/kunwarshivam/mandate/issues/439)).
  **Prerequisite for the account-stream risk-state mapping (#482 round 1, M2; DEC-404 item 7):** `mandate-spec`'s
  classifier change for DEC-353's two shapes (#444, #471) is a journal-affecting change, because `MandateVersionApplied`'s
  mapping refuses a record whose stated classification differs from `change::classify`'s. So it lands before any path
  builds a `ValidationContext` from a real journal (DEC-169's wiring).
  Owed after it: `mandate-spec` parsing the field, V-044, and the §9.2 row (DEC-77 tests then implementation), the executor's
  fold of §6.7, and the `kind: tripwire` harness arm. Also owed, from #443's round 1 (m4): run ruff over `reference/` in
  `cargo xtask ci lint`, so a duplicated definition such as a second `main()` in `reference/mandate/mutants.py` (F811) fails
  the lint rather than reaching review. ~~Also (#443 round 3): the per-PR `cargo xtask ci reference` runs `fuzz.py` but not
  `reference/mandate/mutants.py`, which only the nightly job runs, so a stale mutation anchor passed `full` on #443's round-2
  head. Make a missing anchor fail per-PR CI, for example with an anchor-only check that runs in seconds.~~ Done
  ([#647](https://github.com/kunwarshivam/mandate/pull/647), DEC-493: `cargo xtask ci reference` runs
  `mandate_tools.mutation_anchors`, every `old` text of every mutant table against `ref.py`, in under a second).
  ~~Also owed (#443 round 3): journal spec rule 28 and `OwnerCommandRefused.reason`'s `not_independent`~~ Done in two PRs:
  the code ([#474](https://github.com/kunwarshivam/mandate/pull/474): `mandate-journal`'s schema and rule 28), then the
  reference (the validator check in `reference/journal/control.py`, its two vectors, and its seeded bug). Also owed (#474
  round 1, m1): a test that pins every closed value list in `mandate-journal`'s §9.2 schemas against the spec's, so
  widening one, as #474 widened `OwnerCommandRefused.reason`, fails unless the spec says so. Copy `REQUIRED` in
  `crates/mandate-journal/src/control.rs`: a list written out from the spec's tables, not read back from the schema, since a
  bare `OneOf` checks nothing against widening ([#476](https://github.com/kunwarshivam/mandate/pull/476) round 1, m3).
  Also owed (#476 round 1, m2), in the next `mandate-journal` code PR: raise `control::tests`' two draft-count floors
  (`parsed >= 6`, `checked >= 49`) to the measured counts, so dropping a vector fails them as their messages say.
  ~~Also owed (#443 round 3, m14; #476 round 1, m4): no CI job runs `reference/journal/generate.py` or its seeded bugs, since
  `cargo xtask ci reference` runs only `reference/mandate/`. Its first cost: rule 28's report order went unpinned on
  the reference side until #476 round 1. Run `generate.py --check` per PR.~~ Done
  ([#647](https://github.com/kunwarshivam/mandate/pull/647), DEC-493: `cargo xtask ci reference` runs
  `reference/journal/generate.py --check` in the reference environment, beside
  `python/mandate_tools/tests/test_journal_agent_vectors.py`, which runs it in `fast`).
  ~~Also owed (#444, DEC-353; E6-13's code half, tests first)~~ Done ([#516](https://github.com/kunwarshivam/mandate/pull/516),
  [#523](https://github.com/kunwarshivam/mandate/pull/523), [#527](https://github.com/kunwarshivam/mandate/pull/527);
  [DEC-420](decisions/DEC-420.md)). Owed from #523's review: ~~the next stream H tests PR pins that the delegations row
  matches by `id`, not position (`[d1, d2]` to `[d2]` is reducing)~~ (pinned with V-047's tests PR, DEC-428); and `reference/mandate/ref.py` gets three fixes,
  none of which the Rust shares: `T()` drops fractional seconds and raises on a calendar-less instant where V-041 should
  refuse; V-042's withheld previous document (identity only) is not modelled, where DEC-420 item 4 refuses every
  delegation; and an absent list against `[]` classifies reducing where the Rust says neutral.
  The original row: `mandate-spec`'s §9.2 classifier takes DEC-353's rule, so
  MC-J01, MC-J03 and MC-J05 pass, with MC-J06 and MC-J09, which stay reducing. Size it with what comes first: `mandate-spec` has no delegations at
  all. `Autonomy` has no `delegations` member and `parse::autonomy`'s member list is closed, so a mandate carrying one is
  refused at parse (`unknown_member`); the type, the parser and §9.2's `autonomy.delegations` row, which the Rust
  classifier also lacks, come before the rule has anything to read (#471 round 1, m9).
  **Founder question, when this is picked up** ([#471](https://github.com/kunwarshivam/mandate/pull/471) round 2, the
  reviewer's note): a risk-reducing rule change can move routine orders onto a delegation granted for something else and
  spend it. Removing an `auto` rule ahead of a delegated `ask` (MC-J06) sends the small orders the rule decided to the
  delegation, so the orders it was granted for escalate once it is spent. Every decision is stricter, so every invariant
  holds, but the owner is not told. Should the change's confirmation screen say which delegations the new version's
  orders will draw on? Product wording, so the founder's (§6.4's approval card already carries delegation shapes).
  ~~**Founder question** (#443 round 2): should a mandate be refused at validation
  when `independent_approval_required` is on and the workspace has one user?~~ Decided yes (the founder, 2026-10-02):
  spec V-047 and MC-V69 to MC-V71 ([DEC-411](decisions/DEC-411.md)); the code is merged (#536, #580) and MC-V69 to MC-V71 pass.
  **Founder question** (DEC-411 item 6): a workspace that loses its second user, or turns
  `independent_approval_required` on with one user, after a version is confirmed keeps its agents running with nothing
  they latched liftable, and no new version validates there but a risk-reducing one (DEC-444, below; §4.3's conforming
  version passes when it classifies as reducing). Should removing the second user be
  refused, or flag the agents `policy_nonconforming`? ~~And should a reducing version be exempt from V-047?~~ Decided
  yes, reducing only (the founder, 2026-10-03; [DEC-444](decisions/DEC-444.md)): a version §9.2 rates risk-reducing
  passes V-047 there; a neutral one is still refused.
  Recorded beside it (DEC-411 item 2; #528 round 1, M3): two users of whom only the author is an approver pass V-047
  and V-024, but an ask needing an independent approver times out (§6.4). Not decided: it would extend V-047 past the
  founder's decision to approvers, which validation does not read independently of the author.
  Actions are `end_delegations` or `exits_only`, never `paused` (rule 13). *Accepted when:* a fired
  tripwire acts at its next evaluation, journals the event, alerts with opaque text, and lifts only
  by the owner's acknowledgment with step-up; adding or tightening one applies at once.
- **E6-14 (Should)** As an owner, I want every `auto` and every delegation to fall back to `ask` once
  my mandate passes its review date unconfirmed, so that an abandoned account stops acting alone
  ([DEC-188](04-decision-log.md#decisions)). Waits on the spec change for `autonomy.review_by` (MI-32). *Accepted when:*
  past the date, autonomous paths decide `ask`, positions and exits are untouched, and re-confirming
  (a version moving the date later, with step-up) restores them. The spec change is MI-32, V-046, and §6.2 step 5b
  with the MC-D cases ([DEC-271](04-decision-log.md#decisions) to [DEC-273](04-decision-log.md#decisions): re-confirming is
  risk-increasing and carries the delegations over). Owed after it: `mandate-spec` parsing the field, V-046, and its
  §9.2 row (stream F), then the spec change that makes the date required on every version holding an `auto` or a
  delegation, with the base mandates (DEC-272 item 3). Also owed, from #380's rulings: the vectors for journal spec §9.1
  rule 7's clause that `decided_by: review_ceiling` requires `autonomy: ask` (the clause is in the spec). One code PR
  carries the invalid draft `decision_review_ceiling_on_auto` and the valid draft `decision_review_ceiling_asked`, the
  validator rule `7.review_ceiling_label` and its mutant in `reference/journal/generate.py`, the regenerated
  `journal.yaml` and `fixtures/refcases/journal.json`, and the agent-stream harness's count
  (`crates/mandate-refcases/src/journal/agent_stream.rs`, 106 to 108), as DEC-177 items 6 and 14 did (ES-22).

### E7 Alpaca connector and recovery

- **E7-1 (Must)** As an operator, I want to connect my Alpaca paper and live accounts through
  OAuth, granting trading access only, so that agents can trade without Mandate ever being able
  to move my funds.
  *Accepted when:* the OAuth request contains only trading and account-read scopes; tokens are
  stored only in the workspace vault.
- **E7-2 (Must)** As an owner, I want order intents journaled with idempotency keys so that
  crashes never duplicate orders.
- **E7-3 (Must)** As an owner, I want the agent to reconcile with the exchange after a restart
  so that its state matches reality.
  *Accepted when:* fault injection at every submission step yields zero duplicates and full
  reconciliation; mismatches pause the agent and alert.
- **E7-4 (Must)** As an owner, I want protective exits resting at the broker as OCO or bracket
  orders, with defined exit and kill-switch sequences, so that positions keep protection if the
  platform is down.
  *Accepted when:* RC-14 passes; unprotected windows are journaled and alerted beyond the limit.
- **E7-6 (Must, M8)** As a retail user, I want to connect a Robinhood agentic trading account over
  MCP so that my agent trades US equities and crypto spot with the funds I deposited there and
  nothing else ([DEC-98](04-decision-log.md#decisions),
  [OD-12](04-decision-log.md#open-decisions)).
  *Accepted when:* the connector can place, cancel, and reconcile equity orders in the dedicated
  account only; every MCP exchange is journaled; beta terms are recorded; the connector requests and
  stores only the agentic account's data; account numbers are held by reference (journal spec §6.4)
  and never logged.
  *Refined by the [connections spec](../specs/connections.md) §6 ([DEC-441](decisions/DEC-441.md);
  safety-critical; tests first, DEC-77):* **blocked on E7-15** and on DEC-441 item 15; not built
  unless the tool contract gives an idempotent client order id and query by it (U-R1, U-R2; item 10).
  Tests run only against fixtures synthesized from the published contract and the simulated broker
  with Robinhood's rules (DEC-124); no test or run calls Robinhood (rule 8; item 11). The connector
  implements `BrokerConnector` through the E7-16 MCP client; calls only allowlisted tools, never an
  options, exercise, watchlist, alert, or other write tool (CN-9); names the agentic account on every
  request and drops other accounts' data before hashing (CN-8); maps unconfirmed statuses to
  `blocked` (U-R6); keeps a reserved budget for exits, cancels, and kill-switch orders (§6.5);
  refuses a tool list carrying any fund-movement tool (CN-2); and a fixture with injection text in
  tool descriptions shows the text reaches no journal text, notification, or model (CN-9).
- **E7-5 (Must)** As an owner, I want one account ledger per broker account and one agent per
  instrument per account, so that agents never overspend or cross each other.
  *Accepted when:* RC-17 passes; external activity switches agents to exits-only (RC-15).
- **E7-7 (Must, M6)** As the founder, I want one order placed end to end on my Alpaca paper account
  through the real crates, so that integration defects appear before the Phase 1 soak
  ([task brief](tasks/E7-7-tracer-bullet.md), [DEC-138](04-decision-log.md#decisions)).
  *Accepted when:* the whole path — validated mandate, stored market data, the E4-2 moving-average
  baseline as the signal, the order builder's sizing, the risk gate, the runtime's decision cycle with
  journal-before-acting, the executor's idempotent intent, the Alpaca paper connector, the journal
  record, and a reconciliation after a restart — runs in CI against recorded Alpaca paper fixtures and
  touches no network; with any one stage replaced by a stub returning its crate's `Unimplemented`
  error, zero orders reach the connector and nothing is journaled as submitted; and a manual paper run
  places exactly one order, journals its intent before sending it, and refuses any host that is not
  Alpaca's paper host.
- **E7-8 (Must, M6)** As the founder, I want the tracer to read an instrument's asset record and its
  latest quote from Alpaca, so that sizing, the collar and the executor's instrument snapshot have a
  production source ([DEC-168](04-decision-log.md#decisions), the coordinator's ruling on #171).
  *Accepted when:* `TradingClient::asset` reads trading-domain §3.1's fields from `GET /v2/assets/{symbol}`
  and `DataClient::latest_quote` reads the IEX or crypto latest quote from the data host, both as exact
  values; a missing, unreadable, one-sided, other-instrument or stale answer is a typed refusal and
  never a value (rule 3); the data host is reachable only through its own request type; and the tests
  in `crates/mandate-alpaca/tests/reads.rs` pass against the recorded fixtures with no network.
- **E7-9 (Must, M6)** As the founder, I want the agent-stream payload schemas registered in
  `mandate-journal`, so that the tracer's journal records parse against the journal spec's vectors
  (DEC-168, the coordinator's ruling on #171). *Accepted when:* each agent-stream event the runtime
  and the executor write has a registered schema, tested first against the journal spec's vectors.
  *Unblocked in half* ([DEC-177](04-decision-log.md#decisions)): journal spec v0.4 closed none of the
  eleven agent-stream schemas the runtime writes (DEC-174). Journal spec v0.6 §9.1 closes the agent
  stream's `StreamOpened`, `ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`,
  `IntentProposed`, `AgentModeChanged`, `KillSwitchActivated`, and `OwnerExitRequested`, with
  vectors in `journal.yaml`'s `agent_stream` section, so their tests can be written first now. The
  approval events, which journal spec v0.5 added (M7, claim
  [#213](https://github.com/kunwarshivam/mandate/issues/213)), are not closed yet (DEC-177 item 23).
- **E7-10 (Must, M6)** As the founder, I want the control-stream payload schemas registered and mapped
  to stream F's `JournaledFact`, so that `ValidationContext::from_journal` has a production source
  (DEC-168, DEC-169, the coordinator's ruling on #124). *Accepted when:* `AccountSnapshotRecorded`,
  `AgentDeployed`, `AgentStopped`, `ConnectionEstablished`, `DisclosureAccepted`,
  `MandateVersionCreated`, `MandateConfirmed`, `ConfigSnapshotRegistered` and
  `PlatformOperatorAction` have registered schemas, and each maps to its `JournaledFact`, tests first.
  *Unblocked except `PlatformOperatorAction`* ([DEC-261](04-decision-log.md#decisions)): journal
  spec v0.7 §9.2 closes the other eight, with `ConnectionRevoked` and `OwnerCommandRefused` (#411,
  #413). It maps each record to its fact, and the vectors are in `journal.yaml`'s `control_stream`
  section. `PlatformOperatorAction` stays open (DEC-261 item 9, Proposed), so its schema and the
  `ModelWithdrawn` mapping wait for the operator service's specification.
  *Implementation* ([DEC-303](decisions/DEC-303.md) items 8 to 15), in two PRs under ES-13. The first
  (#445, merged) registers every §9.2 schema but `AccountSnapshotRecorded` in `mandate-journal`. The
  second makes the `JournaledFact` mapping live in `mandate-spec`. *Left open:* the
  `AccountSnapshotRecorded` registration with rule 24, once stream K's fee-step writer conforms
  (#441); `PlatformOperatorAction`; and, as a follow-up, removing `SpecError::Unimplemented` and
  `ParseError::Unimplemented`, which no `mandate-spec` code returns any more but `mandate-shell`'s test
  doubles still name (DEC-303 item 15). *Tests PR*: 17 pending tests against stubs in
  `mandate-journal`'s `control` module and `mandate-spec`'s `JournaledFact::from_record`. Three live
  tests keep `AccountSnapshotRecorded` unregistered until stream K's writer conforms (DEC-261 item 7).

The rows below come from the [connections spec](../specs/connections.md)
([DEC-441](decisions/DEC-441.md) item 13). All are safety-critical (broker connectors, OAuth
scopes, key-permission checks, credentials) and go tests first (DEC-77). E7-1 (Alpaca OAuth) follows
spec §5.2 to §5.4: scopes `trading` and `data` only, never `account:write`; a grant that differs from
the request is refused; and live Alpaca is OAuth only, API keys paper only (DEC-441 items 3 and 5),
after U-A1 to U-A5 are recorded.

- **E7-11 (Must, M8)** As a workspace admin, I want a connection record that holds references
  only, so that no credential can leak through the platform's own records (spec §3, CN-1, CN-5).
  *Accepted when:* the record has the §3 fields and no secret-shaped member; a canary-secret scan
  of journal, logs, artifacts, and API responses finds nothing; a second connection with the same
  account fingerprint in the deployment is refused; the fingerprint is never journaled or returned.
- **E7-12 (Must, M8)** As an owner, I want every credential checked at connect, at each executor
  start, and daily, so that the platform never holds a permission that can move my funds or reach
  the wrong environment or account (spec §8.1, CN-2, CN-3, CN-10).
  *Accepted when:* for each connector, fixtures with each fund-movement permission, a wider grant,
  a credential answering the other environment, and another account are refused before any vault
  write; a live key whose permissions cannot be read is refused; each result, refusals included, is
  journaled without the credential (*depends on E7-17* for the check-result events); a later
  failure moves the connection to `suspended`; a token documented as reaching both environments is
  refused with no request to the other host (DEC-441 item 21).
- **E7-13 (Must, M8)** As an owner, I want a degraded or invalid connection to stop new openings
  at once while exits keep going, so that losing a broker link never adds risk (spec §8.2, §9,
  CN-6). *Accepted when:* fault injection that revokes, expires, or fails refresh at every step of an
  open, an exit, and a kill switch sends no opening after the event and every exit the fake broker
  still accepts; `AccountRestrictionChanged` (`closing_only`, `account_restricted`, cause
  `connection_unavailable`) and `AgentModeApplied` are committed before anything else (trading
  §7.3 v0.15, connections spec §9.1; DEC-441 items 7 and 23); a broker reject or notice is journaled
  with `broker_reject` or `broker_notice`, never the connection cause, and the reverse; the owner
  alert for the connection cause is distinct from the broker-restriction alert; the restriction
  lifts only after the connection's own condition clears and then the owner acknowledges, an
  account refresh alone never lifts it, and clearing it leaves any broker restriction standing.
  *Reference cases:* RC-15 is unchanged; the tests PR adds a trading-domain case for the
  connection row and its lift order (YAML, then `cargo xtask refcases --write`), and the journal
  spec closes `AccountRestrictionChanged` with `cause` when that schema is registered.
- **E7-14 (Must, M8)** As an owner, I want reconnecting my account to keep its connection, so that
  revoking and reconnecting can never reset my loss carry (spec §9.2, CN-12).
  *Depends on E7-17* (the reconnect rule). *Accepted when:* revoke and reconnect of the same
  account keeps `connection_id`, `account_ref`,
  the stream, and the loss carry, and V-032 still binds; reconciliation runs before any agent
  resumes.
- **E7-15 (Must, M8, before E7-6; no code)** As the founder, I want Robinhood's tool contract and
  platform terms confirmed in writing, so that the connector is built on facts rather than guesses
  (spec §6.6). *Accepted when:* U-R1 to U-R12 each have a recorded answer with its source, from
  published documentation or Robinhood's written reply, gathered without any call to a real account;
  DEC-441 items 15, 16, 17, and 20 are decided.
- **E7-16 (Must, M8)** As an owner, I want the broker MCP client limited to an allowlist of tools
  pinned by contract hash, so that a malicious server or poisoned tool metadata cannot steer it
  (spec §6.2, CN-9). *Accepted when:* a fixture server that adds a tool, changes a schema, or carries
  injection text in descriptions or errors: the new tool is never called, a changed hash halts
  openings, and the text appears in no journal text, notification, or model input; only the pinned
  host is reachable and redirects are refused.
- **E7-17 (Must, M8)** As an auditor, I want connection state changes, permission-check results,
  refusals, and credential rotation journaled, so that every connection's history can be replayed
  (spec §3, §9, CN-10). *Accepted when:* the journal spec defines their schemas; adds `account_ref`
  to `ConnectionEstablished` as a new `schema_version` (DEC-261 item 10's binding clause); and
  allows a second `ConnectionEstablished` for a `connection_id` only after its `ConnectionRevoked`,
  with the same broker, environment, and `account_ref` (spec §3), with vectors, tests first.
- **E7-18 (Must, M8)** As an auditor, I want a broker exchange record to say that it is filtered
  as well as redacted, so that nobody reads it as the broker's raw bytes in a dispute (spec §6.2
  rules 4 and 6; #563 round 1, minor 2). *Accepted when:* the journal spec's `BrokerExchangeRecorded`
  text (§6.3) says the stored exchange has other accounts' data dropped before redaction and
  hashing, that the dropped parts cannot be recovered, and that reconciliation disputes rest on
  the filtered record; the connections spec §6.2 says the same.
- **E7-19 (Must, M6; SC)** As an owner, I want paper deployments to run through a production cycle
  API over my confirmed mandate and effective configuration, so that an instrument, model or
  deployment can change without a release and paper proves the path the product uses
  ([DEC-475](decisions/DEC-475.md)).
  *Accepted when:* the production cycle takes a validated deployment, authoritative account and
  market snapshots, and a registered `ModelOutput`; no production shell assembly selects an
  instrument, model, deployment identity, gate configuration or executor configuration; every
  effective input is content addressed in the journal; recorded paper tests and the manual Alpaca
  paper run use that same API with no synthetic model or test-only execution branch; and restart
  reconciliation sends no duplicate.
  *Open (#644 review round 2, major 3):* DEC-484 item 4's object shape — `policy_set_version`,
  the levels in platform, organization, workspace order with at most one of each, and models and
  their parameter names strictly sorted and unique — is checked nowhere yet. The journal does not
  check it at append and will not: the spec's append-time check for these two kinds is only an
  absent object (`missing_artifact`) and a differing top-level `kind` (`config_ref_kind`), and
  §5.1 and §11 define no refusal code for a malformed shape. **The validated production input
  owns it**, beside DEC-484 item 5's second sentence, which already puts the mandate's parameter
  keys and admission capability there; that slice adds the structural checks and their cases, and
  a refusal code for them needs an approved reference case first.
  *Remaining slices* ([first paper trade brief](tasks/first-paper-trade.md)): slice 2's
  remainder (the liquidity facts still read `SYMBOL`, AAPL), slice 3's (the account-rule
  constants in the gate template, and the `last_equity` and `maintenance_margin` fields no read
  carries yet), slice 4's shell half (the agent records at version 2 with `policy_set` and
  `model_registry`, DEC-484, which needs an artifact-aware append in `mandate-journal-pg`), and
  slice 5 as the new `mandate-paper` adapter, its bounded wait for a terminal entry, and the
  deletion of E7-7's AAPL assembly.
  *Follow-up (A1, [DEC-524](decisions/DEC-524.md)):* nothing yet turns a broker-reported
  maintenance deficit, a negative `BrokerAccount::maintenance_excess`, into trading-domain spec
  §9.2's `exits_only` for every agent and an owner alert. The gate deliberately denies nothing per
  order on it (`mandate-risk`'s `a_reported_deficit_is_an_account_state_not_a_denial`), so the
  account-state path owns it, beside §7.3's restriction table in the executor's reconciliation.
  With 1× long-only exposure no approved order creates a deficit, so the first trade does not
  need it.
- **E7-20 (Must, M7)** As the founder, I want CodeQL to flag a credential written to a log by
  its type rather than its name, so that excluding the name-keyed `rust/cleartext-logging` query
  ([DEC-500](decisions/DEC-500.md)) leaves no gap. *Accepted when:* a query under
  `.github/codeql/queries/`, read by `.github/codeql/codeql-config.yml`, reports any value returned
  by `expose_secret()` (directly, through a reference, or inside a format argument) reaching one of
  CodeQL's logging or print sinks; a seeded bug that prints an exposed `SecretString` is caught and
  the query raises nothing on `main`; the proof is recorded in the change, since the seeded bug is
  not committed; and the query runs in the existing CodeQL workflow within its time budget.
- **E7-21 (Must, M6, before the BTC/USD paper trade, DEC-509; SC)** As the founder, I want the
  connector to read a crypto pair's recent one-minute bars, so that the order-size participation
  cap has a trailing volume for BTC/USD ([first paper trade brief](tasks/first-paper-trade.md)).
  *Accepted when:* `BarsRequest` builds the crypto bars read on the data host
  (`/v1beta3/crypto/us/bars`, the pair percent-encoded in `symbols`, `timeframe=1Min`, the window,
  `limit`, `sort=asc`) and reads the answer keyed by the pair; a crypto window is bounded in UTC,
  not by a New York date; the equity read is unchanged; and an answer for another pair, a second
  page, a bar off the minute grid, outside the window or still open, or no bar at all is a typed
  refusal, tested against recorded fixtures with no network.
- **E7-22 (Must, M6, before the BTC/USD paper trade, DEC-509; SC)** As the founder, I want the
  production assembly to read a crypto pair as the trading-domain spec defines it, so that a
  BTC/USD deployment is gated by the crypto rules and not refused by equity-only ones
  ([first paper trade brief](tasks/first-paper-trade.md), "BTC/USD follow-up"). *Accepted when:*
  `mandate-liquidity` computes the 30-day median daily dollar volume (§3.2 item 7); the shell
  trusts daily bars over UTC days for a pair (§2.2); the judge, gate template, and run and
  executor contexts take the session, feed and quote age (`crypto_quote_max_age_s`),
  `crypto_status`, quantity increments, quote currency, fee reservation and asset-fee rate, TIF,
  and `crypto_stop_limit_offset` from the asset class and the confirmed inputs; a crypto limit is
  put on the asset record's `price_increment` against the order (§2.1); a crypto deployment whose
  `max_order_usd` exceeds §5.2's 200,000 USD is refused; every existing equity test still passes;
  and no crypto rule is relaxed (DEC-450 item 3). A gate check of the 200,000 USD cap is a later
  row.
- **E7-23 (Must, M6, the first live trade, DEC-529; SC)** As the founder, I want each broker to
  declare what it supports as a capability profile, so that shared code never branches on a broker
  and a new broker is one profile, not new rules ([DEC-531](decisions/DEC-531.md),
  [DEC-630](decisions/DEC-630.md), [ADR-0004](../adr/0004-broker-capability-profiles.md), trading spec §5.2; the
  [first live trade](tasks/first-live-trade.md) rows SP1, B1, B2a, B2b, B3). *Accepted when:*
  `CapabilityProfile` and its canonical hash live in `mandate-domain`; `BrokerConnector::profile`
  hands each connector's profile to the executor; Alpaca declares trading spec §5.2's table as its
  profile with no Alpaca outcome changed (LT-14); protection, reconciliation and the builder's
  quantity form read the profile, and the executor's `asset_class == Crypto` protection branch is
  gone (LT-2); policy and profile intersect, never override (LT-3); and a property test over
  generated profiles checks every order sent is one the profile allows.
- **E7-24 (Must, M6, the first live trade, DEC-529; SC)** As the founder, I want to log in to
  Robinhood with OAuth for one run, holding the token only in the connector process's memory, so
  that no credential reaches a disk, a log, the journal or an agent (CN-1, `AGENTS.md` rule 7; the
  [first live trade](tasks/first-live-trade.md) rows O1a, O1b). *Accepted when:* the PKCE login
  runs in the founder's browser through a loopback redirect; the token is a `SecretString` in the
  connector process only and is gone at exit (a restart logs out); a canary-token test scans every
  output, error and artifact (LT-9); expiry mid-run leaves the deployment `closing_only` and the
  resting stop untouched; and no test or CI job reaches a Robinhood host (LT-1).
- **E7-25 (Must, M6, the first live trade, DEC-124; SC)** As the founder, I want a simulated
  Robinhood server that speaks the published contract over loopback MCP with Robinhood's rules, so
  that every Robinhood test and the founder's rehearsal run without touching Robinhood (DEC-124's
  paper stage; the [first live trade](tasks/first-live-trade.md) rows S1, S2, R0). *Accepted
  when:* `mandate-rh-sim` is a `tool` crate no production crate depends on; it serves the nine
  allowlisted tools with the contract's shapes, its order states and its rules (no query by
  `ref_id`, one GTC stop-limit as protection); its pure core carries the property tests
  of S1's tests PRs; and the rehearsal (R0) runs the live build against it on a journal separate from
  the live one.
- **E7-26 (Must, M6, the first live trade, DEC-529 item 3; SC)** As the founder, I want one
  deployment runner for any environment and broker, with live hosts only behind a `live` feature
  that only the runner may enable, so that no other build can reach a live broker (ES-23; the
  [first live trade](tasks/first-live-trade.md) rows X1, G1a, G1b). *Accepted when:*
  `cargo xtask live-feature` lets only the runner declare a `live` feature, and no CI or release
  build enables it except one compile-only job (ES-23 as DEC-529 item 3 narrows it), so the
  default build contains no Robinhood host (LT-1); the runner built from the
  paper path's E1a takes any broker connector and environment through `ProductionCycle::run`
  (LT-4); and a restart after a run sends no second order (LT-6).
  *Follow-up ([DEC-851](decisions/DEC-851.md) item 5, #976's review):* X1's word scan does not
  read through `time -p`, the wrappers `setsid`, `flock`, `ionice`, `taskset`, `unbuffer`,
  `doas`, `su -c` and `runuser`, `env -S`, or a dynamic `printf -v "$N"`. They are disclosed
  residuals; the build-file and live-feature checks and review stand behind them. Close them with
  a tests correction that pins each form as refused, then the implementation that refuses it.
  *Follow-up ([DEC-851](decisions/DEC-851.md) item 6, #979's review):* the live-feature scan's
  shell reading still has limits: a here-doc body fed to a command other than a shell is read as
  commands, `case` arm patterns and `[[ =~ ]]` regex parentheses split a command, and a
  single-quoted string outside the here-doc and text reading is not joined. None hides a refusal
  rule 1′ makes today; fix each with a pin when a real line needs it. Rule 2 judges pipelines
  only, so a shell fed from a file (`sh < <(echo $C)`, `echo $C > f; sh f`) is a disclosed
  residual (DEC-851 item 5, #985's review); pin and refuse it the same way.
- **E7-27 (Must, M8, before any Alpaca OAuth connection completes: E7-1, E10-13)** As an owner, I
  want an Alpaca OAuth token's possible breadth journaled with the connection and disclosed to me,
  so that a token that may reach both environments is on the record before it is used
  ([DEC-821](decisions/DEC-821.md) item 4, DEC-441 item 22, spec §5.3; follows E7-17,
  [DEC-800](decisions/DEC-800.md) item 14). *Accepted when:* a journal spec change after v0.20
  defines the event that records, with the connection, whether the token may reach the other
  environment, and the disclosure the owner confirmed, with vectors, tests first; and no Alpaca
  OAuth connect appends `ConnectionEstablished` before that event.

- **E7-28 (Should, M6, after E7-26; tooling)** As the founder, I want X1's shell reading to
  parse `case` arm patterns, `[[ … ]]` tests (their `|` and parentheses), single-quoted strings
  that span lines, and `${…}` holding a space, so that the live-feature check's interim exact-line
  list ([DEC-851](decisions/DEC-851.md) item 6) can be emptied. *Accepted when:* a tests
  correction pins each form on real-line shapes, the tokenizer reads them, the exact-line list in
  `xtask/src/main.rs` is empty, and `cargo xtask live-feature` still exits 0 on the repository.
  Beside it, the stronger artifact-level check DEC-851 item 6's threat model names: assert from
  `cargo metadata` and the build plan that no CI job resolves the `live` feature.

### E8 Escalation and approvals

- **E8-1 (Must, M7)** As an approver, I want requests with the proposed action, alternatives,
  evidence, risk impact, deadline, and default so that I can decide quickly
  ([task brief](tasks/M7-escalation-v0.md), [DEC-155](04-decision-log.md#decisions)). "Alternatives"
  means the owner's choices (approve or skip, with the default stated), never platform-authored
  alternative trades ([mandate spec §6.4](../specs/mandate.md#64-approvals), FR-6.2).
  *Follow-up (DEC-165 item 3, #236):* the content's Trigger row still lacks "the rule as the owner
  wrote it". §6.4 now fixes it (DEC-173 item 2): `trigger.rule` is the owner's confirmed rule
  `{id, when, then}` exactly as the mandate holds it, or null for `default` and
  `admission_ceiling`. It joins the content object in a tests correction, with a test that
  scans owner-written text apart from the platform's own in
  `the_content_never_carries_advice_wording`, so an owner's rule named `target_weight` is shown as
  written and never read as platform advice.
  *Follow-up (#250 review, minor 1):* `RequestContent.risk_impact` is a `Vec<RiskFigure>`, so a
  caller could list fewer than §6.3's six figures, or one twice. Make it one figure per
  `RiskField` by type (`[RiskFigure; 6]` in `RiskField` order, or a map keyed by field), with a
  tests correction for `every_bound_field_moves_the_content_hash`, which truncates the list.
  *Done (#250 review, major; DEC-165 item 13):* `ApprovalRef::of_requested_event` accepts only a
  ULID-shaped event id, so free text cannot reach a notification through it. The runtime or CLI
  PR that first calls it must prove the id is that `ApprovalRequested` event's own.
  *Follow-up (#254 review, minor 1):* `mandate-approval`'s `is_ulid` is a copy of
  `mandate_journal::schema::is_ulid`. Layering stops the approval crate from depending on the
  journal: it is at layer 1 and the journal at layer 2. So nothing keeps the two copies in step.
  Pin them at rung 2, with an `xtask` check that compares the two function bodies, or at rung 1,
  by moving the shape into a layer-0 helper that both crates call.
- **E8-2 (Must, M7)** As an owner, I want timeouts to apply the safe default so that silence never
  adds risk ([task brief](tasks/M7-escalation-v0.md), [DEC-156](04-decision-log.md#decisions)).
  *Follow-up (#250 review, minor 4):* EI-13's first bound, one pending risk-adding approval per
  agent, is not in `mandate_approval::ask_permit`; it stays in the runtime's
  `awaiting_risk_approval`, and the runtime's tests PR must assert it.
- **E8-3 (Must, M7)** As an owner, I want approved actions re-validated for drift so that stale
  approvals are not executed blindly ([task brief](tasks/M7-escalation-v0.md),
  [DEC-156](04-decision-log.md#decisions)). The same brief covers M7's CLI owner control.
  *Done (#240 review, round 2, minor 2; the M7 tests correction, DEC-173 item 14):* the step-up
  generators in `tests/grant_properties.rs` reached an age of exactly 300 s only by chance
  (`-350..20` and `-400..60`), so a planted exclusive window survived about half of all seeds.
  Both now draw -300 one time in five (`at_the_window_edge_or`); the same planted window fails both
  properties it reaches on every one of 20 seeds, where the old generators missed it on 10.
  *Done (E8-3 implementation, the do-nothing sweep; the M7 tests correction, DEC-173 item 14):*
  `drift_exactly_at_the_band_is_inside_and_one_unit_over_either_way_is_not` and
  `no_mark_is_outside_the_band` checked the drift result of `revalidate` in one direction only, so
  a constant `Skip(Drift)` passed both. Each now opens with the paired positive, "the unchanged
  fixture acts", and that constant fails both.
  *Follow-up (#275 review, minor 2; the coordinator's ruling, rule 13):* when
  `owner_command(OwnerExit, …)` returns `CommandAuthority::Refused`, only the owner-exit
  privilege is withdrawn: selling equities outside the regular session at the confirmed bid.
  The exit itself is never withdrawn. The runtime must still route that owner exit, either as a
  regular-session exit or as the displayed-bid-confirmed exit once the owner confirms a fresh bid.
  The refusal never holds it, drops it, or turns it into a no-op. **Test obligation, in the
  runtime's tests PR (3 of 4):** a test commits an owner exit whose step-up is stale at commit
  and asserts two things. First, the refusal is journaled. Second, the exit is still routed as
  a regular-session exit and reaches the executor, so no step-up outcome can remove an owner's
  risk reduction. The mandate spec §6.1 wording is in ("Owner controls and step-up", DEC-173
  item 5).
  *Follow-up (M7 spec PR, DEC-173 item 1):* the MC-E cases (MC-E01 to MC-E31) are not yet in
  `mandate.yaml`. Two changes, in order. *Done (the tests correction,
  [#343](https://github.com/kunwarshivam/mandate/pull/343)):* `mandate_harness.rs` counts only the seven families it
  owns, by case-ID prefix (MC-S, MC-V, MC-P, MC-C, MC-R, MC-T, MC-L), and
  `a_kind_no_arm_interprets_fails_naming_it` accepts a kind no arm interprets as long as its cases
  fail, so a new family changes no harness test while a case added to or dropped from an owned
  family still fails. Still open: an MC-E spec PR that generates the cases from
  `reference/mandate/ref.py`'s escalation model (already fuzzed and mutation-checked), with
  `cargo xtask refcases --write`, and no `status.toml` row. Give the family a kind of its own: the
  family A, B, G, F, and P count tests select their cases by kind, so a new family reusing one of
  those kinds would change their counts. The same PR corrects mandate spec §11's and §1's sentences
  that MC-U "lands in its own tests-first change, because the shared harness pins the case count":
  since #343 it no longer does (#343 review, minor 4).
  *Done (the M7 spec PR, DEC-280):* MC-E01 to MC-E32 are generated from the reference model as
  `kind: escalation` cases, checked by `check_cases.py`, exported to `fixtures/refcases`, with no
  `status.toml` row; §11 lists the family and no longer says the harness pins the count.
  *Follow-up (DEC-280):* a harness arm for `kind: escalation` in `mandate-refcases`, tests first,
  driving `mandate-approval` (and the runtime for `op: lifecycle`), then the status PR that flips
  the rows it passes.
  *Part done (DEC-292):* the arm interprets `ask_permit` and `deliver_now` through
  `mandate-approval`, and MC-E25 to MC-E28, MC-E30 and MC-E32 pass; their `status.toml` rows are the
  status PR's. Still open: the `lifecycle` op's runtime driver for the other 26 cases, which fail
  naming the op until it lands.
  *Part done (DEC-317, DEC-366):* the `lifecycle` op drives `mandate-runtime`'s `handle` and
  `fold` under one published map, landed in three PRs (#440, #453, #475). Fifteen cases pass
  (MC-E02 to MC-E05, MC-E07 to MC-E16, MC-E31), and the status PR marks them passing. It lists the
  other eleven as pending.
  *Follow-up (DEC-317 item 7, E8-3):* the runtime's `ApprovalResponded` records no `quorum`, the
  approver count and independence check 7 applied, which journal spec §9 requires for a grant that
  reaches check 7 (`mandate_approval::quorum` already computes it). Tests first in
  `mandate-runtime`, then the implementation; MC-E01, MC-E06, MC-E17, MC-E19 to MC-E24 and MC-E29
  fail on that member alone and flip in the status PR that follows.
  *Decided (DEC-318 option (a), the founder, 2026-10-02; DEC-430):* MC-E18 is restated as §6.4's
  cancellation. Its response step cancels the pending grant as `mode_tightened` and refuses it as
  `not_pending`, and the reference model, the fuzz oracle and the seeded bugs follow (reference PR,
  #538, merged). The harness sets the response step's `AgentModeChanged` (`restriction_changed`)
  aside, so MC-E18 passes (#550, merged), and the status PR moves it to `passing` (in review). Still
  open: a §6.4 sentence saying check 9's `mode` arm is defence in depth that no named step reaches
  (DEC-430 item 2). It is a spec PR and changes no rule.
  *Tests in progress (the `quorum` tests PRs, DEC-488, DEC-489): the first of two is merged
  ([#653](https://github.com/kunwarshivam/mandate/pull/653), which replaced #650), carrying the
  two hand cases on the live one-approver binding, the property and the harness test; the
  second carries the two stricter hand cases, then the implementation follows:* DEC-488
  fixes the member as `{required, independent}`, written exactly on a grant check 7 judged
  (`admitted`, `counted`, `duplicate_approver`, `not_independent`) from the bound requirement the
  fold holds and the overlay admission read, and absent otherwise. Four hand cases in
  `crates/mandate-runtime/tests/approvals.rs` (the admitted grant records it and a skip and every
  earlier refusal do not; a restart rebuilds it from the journal; a request bound to two approvers,
  `counted` then `duplicate_approver` then admitted; independence refusing the author), one
  property in `tests/approval_properties.rs`, and one harness test in
  `crates/mandate-refcases/tests/mandate_lifecycle_harness.rs` (the ten cases pass whole and their
  `quorum` is compared) are pending E8-3 as behaviour-only rows of the pending gate (DEC-489): the
  member joins a record the live grant path already writes, so no stub can sit on it. They land in
  two tests PRs under ES-13: the first the one-approver cases, the property and the harness test;
  the second the two stricter-binding cases with their fixtures. The implementation PR writes the
  member, deletes the six markers and rows, moves the ten cases into the lifecycle harness's
  passing list (DEC-489 item 4), stops its `retailed` and `granted_with` helpers striking `quorum`
  (DEC-499), and the status PR flips their rows.
  *Done (the `quorum` implementation, DEC-488, DEC-489 items 3 and 4, DEC-499):*
  `escalation::answered` writes the member through `quorum_applied` on exactly the grants check 7
  judged, the six tests and their `BEHAVIOUR_ONLY_TESTS` rows are live, and the lifecycle harness
  passes all twenty-six cases whole. Next: the status PR flipping MC-E01, MC-E06, MC-E17, MC-E19
  to MC-E24 and MC-E29.
  *Done (the #550 review, minors 1 and 2; in the `quorum` tests PR):*
  `mc_e18_holds_only_as_the_cancellation` pins both plants with `fails_naming` and the review's
  messages, and `apply_now`'s doc comment says it is true whenever it folded a new mode, cancelling
  or not.
  *Follow-up (the #416 review, minor 3):* every `ask_permit` case asks for one instrument, so
  family E cannot see the budget counted per instrument rather than per agent (`mandate-approval`'s
  own suite does). A future MC-E case should spread its ten asks across instruments. It changes
  `mandate.yaml`, so it goes to the founder under DEC-176 unless it only tightens a rule.
  *Follow-up (DEC-280 item 7):* `mandate-journal`'s catalogue gains `OwnerCommandRefused` (agent and
  account streams), and the runtime journals it for a refused resume or Stop, tests first, with a
  test that a refused Stop leaves exactly that event (the #395 review, major 1; DEC-278 item 12).
  *Tests done (the M7 `OwnerCommandRefused` tests PR, DEC-291); the runtime's implementation
  follows:* the catalogue admits it on the account and agent streams; three tests in
  `crates/mandate-runtime/tests/approvals.rs` are pending E8-3: a refused Stop leaves exactly its
  `OwnerCommandRefused` for each of missing, stale and reused evidence, a stale resume does the
  same, and a refused command re-tailed after a restart writes nothing. The executor's half (a
  refused acknowledgment) rides with E7-4 slice 5's copy of `OwnerAcknowledged` (DEC-291 item 4).
  *Done (the runtime's half, DEC-291):* the five tests pass. A resume or Stop whose step-up does
  not count writes exactly its `OwnerCommandRefused` (`command`, `effective_at`, `reason`, with the
  `OwnerCommandIssued` as `causation_id`), and the fold reads it as a copy, so a re-tailed command
  writes nothing.
  *Follow-up (the #413 review, minor 3):* give `mandate-approval` one `From<StepUpRefusal> for
  Refusal` mapping and one reason-code table, retiring the three tables kept by hand today
  (`mandate-runtime`'s `refusal_code`, its `OwnerCommandRefused` reasons, and `step_up_status`'s
  input), so a new refusal cannot be coded differently in two places.
  *Follow-up (the #397 review, minors 1 to 5; one M7 tests PR before the `clap` wiring makes the
  commands reachable):* pin the closed key set of every control payload the CLI commits
  (`OwnerCommandIssued`, `ApprovalResponseSubmitted`, `OwnerAcknowledged`) in `tests/agent.rs` and
  `tests/approvals.rs`, so a `typed_code` member fails; give `Command::Stop` the `--release` choice
  and the warning-shown record (DEC-136) and `status` the agent's restrictions, tests first; make
  `commit`'s idempotency hold across invocations by deriving the event id from the command's
  content and the head (or record why it cannot), add backoff between attempts, and word the
  exhausted report to match; and make `message(Outcome::Refused)` say whether the deadline has
  passed.
  *Tests done (the M7 CLI follow-up tests PR, DEC-290); the implementation follows:* the closed
  member sets, the derived event id and its independent oracle, and the three retry arms are live
  tests; 13 tests pending E8-3 in `tests/agent.rs` and `tests/approvals.rs` cover Stop's release
  and its warning, `status`'s restrictions, the re-run that finds its committed event (with another
  writer's event interleaved too), the kill switch always committed, the repeated acknowledgment,
  the runtime-recorded command committed anew, the backoff, and the refusal's deadline wording. `Command::Stop` gains `release`, `status` takes the account,
  `message` takes the owner's clock, `Ids` no longer mints the event id, and `ControlJournal` gains
  `wait`; the `clap` wiring still waits for the implementation.
  *Follow-up (the #409 review, minor 4; in the implementation PR):* `mandate-cli` carries the
  approval flow, step-up and the owner commands (`AGENTS.md`'s safety-critical list), so set
  `safety_critical = true` for it in `xtask/layers.toml`, add its CODEOWNERS line and the lint
  header `cargo xtask layers` checks, and let the mutation gate judge its diff.
  *Done (the M7 CLI follow-up implementation, DEC-290):* the 13 tests pass. A Stop with release
  records `release: true` and the warning's content reference, and its code binds the choice;
  `status` folds the account stream's restrictions; a re-run of the control stream's last,
  unrecorded command reports it, whatever head it was decided at, while a kill switch skips that
  search and is always committed; retries back off from 100 ms, doubling to 2 s, with a jitter
  drawn from the event id; and a refusal's message says whether the deadline has passed.
  `mandate-cli` is safety-critical (`xtask/layers.toml`, CODEOWNERS, the lint header). Still open:
  the `clap` wiring and a `mandate-journal-pg` `ControlJournal` (DEC-279 item 10).
  *Done (the #414 review, minor 1 and nits 1 and 4):* an exhausted kill switch's report says
  running it again commits another, which a test pins against a pause's; `recorded` reads the
  workspace with `strip_prefix`; `ulid`'s dead fallbacks are documented.
  *Follow-up (the #414 review, nit 2):* cap `earlier`'s scan of heads before the last event, which
  today runs one derivation per earlier event of the control stream.
  *Follow-up (the #414 review, nit 3):* `backoff`'s doc says the jitter makes two racing
  invocations stop fencing each other; it differs between two different commands only, since the
  same command derives the same id and so the same jitter.
  *Follow-up (#343 review, minor 1; a tests correction):* a new family that reuses an owned family's
  kind (for example an `MC-E01` of kind `semantic`) now moves no count in `mandate_harness.rs` and
  runs through that family's arm, where on `main` before #343 it failed two counts. `unread_keys`
  still refuses any member it ignores, so it cannot pass half-read, but the loud failure is gone.
  Assert that the owned-by-kind id set equals the owned-by-prefix set in
  `the_fixture_holds_the_families_this_stream_expects`; the reviewer's four-line version passes on
  today's fixture and fails on that scenario.
  *Follow-up (#343 review, nits):* make `INTERPRETED` a `pub const` in `src/mandate.rs` that
  `run_listed`'s dispatch and the test both read; the `{prefix}{n:02}` ids with a lexicographic sort
  break past 99 cases in a family; the uninterpreted-kind branch asserts only `is_err()`, not that
  the message names the kind.
  *Follow-up (M7 spec PR, DEC-173 item 11):* the `mandate-journal` catalogue (`src/catalogue.rs`,
  `tests/catalogue.rs`) needs `ApprovalRevalidated` (agent, `man`), `ApprovalResponseSubmitted`
  (ctl), and `OwnerCommandIssued` (ctl) from journal spec v0.5 before the runtime's tests PR can
  journal them.
  *Done (#321 review, major; DEC-173 items 13 to 15; the M7 tests correction, then E8-3's
  `quorum`, DEC-257):* `mandate-approval`'s admission read only the bound
  `approvers_required` and `independent_required`. It must judge check 7 against the stricter of
  those and the workspace policy overlay current at the effective time: independence if either
  requires it, the larger approver count, and an author's earlier `counted` grant not counting once
  independence is required. The tests correction gave `AdmissionContext.policy` the overlay and
  stubbed `mandate_approval::quorum`, which `admit` failed closed on under any overlay but
  `PolicyOverlay::NONE`, with eight tests in `tests/quorum.rs` pending E8-3 against
  `reference/mandate/ref.py`'s `approval_quorum`. The implementation PR implemented `quorum`,
  calls it for every overlay, leaves the author out of the grants that count while independence
  is required, and deleted the eight `#[ignore]` lines.
  *Tests done (M7 tests PR 3 of 4, DEC-257 items 5 to 12); the runtime's implementation follows:*
  `crates/mandate-runtime/tests/approvals.rs` and `approval_properties.rs` hold 20 tests pending
  E8-3 for the grant path, owner commands on the control stream, and the #281 owner-exit
  obligation. The implementation PR needs, first, the `mandate-journal` catalogue entries above
  (a real journal refuses `ApprovalRevalidated` until then), and it fills `PendingApproval` with the
  bound request, folds `MarkUpdated`, the control stream's assertions and its copies, and hands a
  one-instrument owner exit as an agent-scoped flatten (DEC-257 item 7).
  *Done (E8-3's runtime implementation, DEC-278):* the 20 tests pass; `mandate-runtime` asks with
  the content object, its hash, the `cli_inbox` delivery and one opaque notification, admits each
  `ApprovalResponseSubmitted` through `mandate_approval::admit` and re-validates a grant in the same
  batch, and copies each `OwnerCommandIssued`, never refusing a kill switch or an owner exit. The
  `mandate-journal` prerequisite below is still open, so the shell cannot yet append these events
  through a real journal.
  *Follow-up (DEC-278 item 13; DEC-156 item 5):* the runtime does not yet bound asking: at most 10
  requests per agent per America/New_York risk day, no re-ask of an instrument the owner skipped
  until the next risk day or applied version, none for one `timeout_s` after a timeout, and each
  suppressed ask recorded as `DecisionMade.ask_suppressed`. `mandate_approval::ask_permit` and
  `AskLedger` exist; fold the ledger and call them before `escalation::ask`, tests first (MC-E25 to
  MC-E28). The same field must also name a request that cannot bind (DEC-278 item 2: no
  `DecidedBy` label, a combined score that is not a decimal, or a content object that cannot be
  represented), which today leaves `ask` on `DecisionMade` with no reason anywhere in the journal
  (the #395 review, minor 3).
  *Follow-up (DEC-278 item 12; the #395 review, major 1):* a refused owner resume, Stop, or
  acknowledgment journals nothing. The M7 spec PR adds an agent-stream `OwnerCommandRefused`
  (the command's control-stream id as `causation_id`, its kind, and the reason `step_up_missing`,
  `step_up_stale`, `step_up_reused`, or `step_up_method`); the next M7 implementation PR journals
  it, with a test that a refused Stop leaves exactly that event.
  *Follow-up (the #395 review, nit):* `IntentProposed` carries no `mandate_version`, so EI-4's
  version equality is enforced by check 8 but readable only from `ApprovalRevalidated`; add it.
  *Follow-up (DEC-278 item 11; a tests correction):* bump `FOLD_VERSION` and regenerate
  `tests/golden-journal.json` with an approval's request, delivery, response, and re-validation and
  a folded mark, since the fold now derives approvals, marks, and the control stream's assertions
  and copies from them.
  *Follow-up (DEC-278 item 3):* the request's `risk_impact` is empty. When the gate's dry run
  exposes the §6.3 figures (`position_usd_after`, `gross_usd_after`, `bought_today_usd`, `drawdown`,
  `daily_pnl_fraction`) and the mandate's caps, put them in the content object.
  *Follow-up (DEC-257 item 12):* `PolicyChanged`'s payload is only "level, diff" in journal spec §9,
  so the runtime neither folds the overlay check 7 reads nor has a two-approver runtime case
  (PB-22). Close the payload, then fold it and add the cases in a tests correction.
  *Follow-up (DEC-257 item 12):* no agent-stream event records a refused resume or Stop; specify
  one (or say a refusal leaves only the control-stream command) so the runtime can journal it.
  *Follow-up (DEC-257 item 12; stream I):* `Input::Command` still carries the owner's pause,
  resume, Stop, exit and kill switch with no step-up judged; retire those for the control stream's
  `OwnerCommandIssued` once the shell tails it, leaving only the risk-limit and operator switches.
  *Tests done (M7 tests PR 4 of 4, DEC-257 items 13 to 17); the CLI's implementation follows:*
  `crates/mandate-cli/tests/approvals.rs` and `agent.rs` hold 15 tests pending E8-3 for
  `mandate approvals list`, `show`, `approve` and `skip`, and `mandate agent status`, `pause`,
  `resume`, `stop`, `kill`, `exit` and `acknowledge`, over the `ControlJournal` stubs in
  `crates/mandate-cli/src/control.rs`. The implementation PR also wires the commands into `clap`
  and `main.rs`, and gives `ControlJournal` a `mandate-journal-pg` adapter.
  *Done (E8-3's CLI implementation, DEC-279):* the 20 tests pass; `mandate-cli` lists and shows
  the inbox from the agent streams, commits `approve`, `skip` and every `agent` command as exactly
  one control-stream event under one minted id, binds each code to what it confirms and the
  control stream's head, and never refuses a kill switch or an owner exit.
  *Follow-up (DEC-279 item 10):* wire `approvals` and `agent` into `clap` and `main.rs` with a
  `mandate-journal-pg` `ControlJournal`, once the prerequisite below registers these events'
  schemas; until then a real journal refuses every command they would send. Add Stop's
  `--release` choice and the warning shown to `Command::Stop` and the payload (journal spec §9).
  *Prerequisite (DEC-257 item 17):* `mandate-journal` registers no payload schema for any agent- or
  control-stream event (`ApprovalRequested`, `ApprovalResponseSubmitted`, `OwnerCommandIssued`,
  `OwnerAcknowledged`, `OwnerCommandRefused`, and the rest), so a real journal refuses each one as
  `UnknownSchema` and neither the runtime's nor the CLI's implementation can append through one.
  Register the schemas as journal spec §9 closes them, with the catalogue entries above, before
  either implementation.
  *Follow-up (#321 review, minor 1):* broaden §6.1's single-use assertion ledger to any
  control-stream event carrying step-up evidence (`DisclosureAccepted`, `PolicyChanged`), which
  would make MI-24 true as written.
  *Follow-up (#321 review, minor 2):* §6.1's and §6.4's "one assertion per approval" should read "per grant",
  because a two-approver approval takes two assertions.
  *Follow-up (#321 review, minor 3):* §5.9 says the executor cancels pending approvals, but §6.4 and
  journal spec §2 put that in the runtime's step; align them.
  *Follow-up (#321 review, minor 4):* journal spec §9's `ApprovalRevalidated` row lacks the
  working-universe membership that check 9 compares.
  *Follow-up (#321 review, minor 5):* check 3's `role:` approver entries are resolved at an unstated
  moment; state it.
  *Follow-up (#321 review, minor 6):* the reference model's assertion ledger never fills `used` from
  `OwnerCommandIssued` or `OwnerAcknowledged`.
  *Follow-up (#321 review, minor 7):* `notifications.channels` cannot express `cli_inbox`.
  *Follow-up (#321 review, minor 8):* `recent_timeout` does not say whose `timeout_s` it uses.
  *Follow-up (#321 review, minor 9):* `reference/mandate/mutants.py` runs only in
  `cargo xtask ci nightly`, not in `cargo xtask check`.
  *Done (#321 round 2, minor 1; the M7 tests correction's reference half, DEC-173 item 13):*
  nothing tested that `ApprovalResponded` records the approver count and independence check 7
  applied, so dropping the member survived the whole fuzz. `fuzz_escalation` now asserts that every
  grant reaching check 7, and no other response, records its own `own_quorum`, and `mutants.py`
  gains two planted bugs, the member dropped and the bound quorum recorded in place of the applied
  one, which `origin/main`'s fuzz let through and this one catches.
  *Follow-up (#321 round 2, minor 2):* say which stream the policy overlay is folded from.
  `PolicyChanged` is on the workspace control stream, and §2's copy list for the agent runtime does
  not include it. State either that the runtime copies it into the agent stream, or that replay
  reads the recorded quorum rather than re-deriving the overlay. Every interleaving only
  over-tightens today, because check 7 takes the maximum with the bound values.
  *Follow-up (#321 round 2, minor 3):* the reference model reads absolute
  `independent_approval_required` and `two_approver_above_usd` from `PolicyChanged`, where journal §9
  gives `level`, `diff` and `affected agents`. State how a partial diff resolves, at which level,
  and whether the agent must be listed in `affected agents`.
  *Follow-up (#321 round 2, nits):*
  - reword the admission sentence as "the grants that count … now number check 7's approver count";
  - say once that "at the effective time" and "folded before the step" coincide because the clock
    fold advances on every event;
  - the approval surface shows the current requirement, not only the bound `approvers` (a PX item);
  - add the per-order approval to §4.3's list of what `independent_approval_required` scopes.
- **E8-4 (Must)** As an approver, I want notifications through web push, email, and a chat
  channel, with escalation chains and quiet hours.
  *Spec:* [notifications spec](../specs/notifications.md) ([DEC-438](decisions/DEC-438.md)). The
  story is split into E8-9 to E8-14 below; E8-4 is done when they are.
- **E8-5 (Must)** As a fund, I want notifications to carry only opaque IDs, with details loaded
  from our workspace deployment, so that trading intent stays private.
  *Accepted when:* captured relay and provider payloads contain no instrument, size, price, or thesis.
  *Spec:* NT-1's canary test ([notifications spec §2](../specs/notifications.md#2-invariants)) is this
  acceptance, run for every channel as E8-11, E8-12, and E8-14 land.
- **E8-6 (Should)** As a fund, I want two approvers above a threshold.
- **E8-7 (Should)** As an approver, I want SMS and phone escalation.
  *Spec:* same payload and records as every push channel (notifications spec §4.1); the ordered
  chain needs a mandate schema field first (spec §12 item 5).
- **E8-8 (Should)** As an owner, I want to answer an ask with "let it do this for a while", within
  caps I set in dollars, orders, and days, so that the agent stops asking me about what I have
  already said yes to ([ADR-0003](../adr/0003-earned-autonomy.md) parts 2 and 3, [DEC-181](04-decision-log.md#decisions)). The spec is
  mandate §6.4 and §6.5 ([#328](https://github.com/kunwarshivam/mandate/pull/328)); the MC-U
  reference cases come first, in their own tests-first change. Safety-critical (autonomy policy and
  the approval flow). *Accepted when:* the MC-U cases pass; a delegation lifts only the `ask` it
  names, within its caps and window, and never while suspended (MI-26 to MI-28); choosing a shape
  approves this action exactly as "Approve just this" would and creates a version confirmed with
  the same step-up, which applies at the next safe point; the sum of a version's delegation caps
  stays within the allocation ([DEC-196](04-decision-log.md#decisions), V-045); no scope is offered for an admission, a
  two-approver ask, a live environment, or a client session.
  Also for E8-8 ([#516](https://github.com/kunwarshivam/mandate/pull/516) round 1, minor 8): `mandate-spec`'s V-043
  bounds each delegation on its own, so twenty delegations can each carry `max_total_usd` equal to the allocation. That is
  the spec's reading, and the gate enforces every limit regardless (§6.5). The approval card and the MC-U family should
  consider the aggregate, which is the V-045 the criteria above name and the mandate spec does not yet define.
- **E8-9 (Must, M7; SC)** As an owner, I want every notice, alerts included, built from one closed
  payload type so that nothing about my trading can reach a provider
  ([notifications spec §3, §4.2, §5.5](../specs/notifications.md), DEC-438 items 1, 2, 5, 17).
  Tests first. *Accepted when:* `GenericText` holds `approval_needed`, `attention_needed`,
  `account_changed`, and `brief_ready`; the payload is `{"notice", "text"}` with a minted
  `NoticeId` that has no constructor from an event id (NT-1, NT-4); a closed kind enum replaces
  `NotificationRef.message_key` in `mandate-runtime` and `mandate-executor`, and every kind in spec
  §3.2 maps to its class and text key; each stream owner writes `OwnerAlertSent` with the kind in
  the subject's batch, and none for a `KillSwitchActivated` caused by an owner command (spec §3.4);
  the payload schemas of journal v0.12's `OwnerAlertSent`, `NoticeIssued`, and `NoticeAttempted`
  are closed in the same tests PR; a wording test holds NT-12.
- **E8-10 (Must, M7; SC)** As an owner, I want a dispatcher that turns committed events into sends,
  retries them, and records every outcome, so that no alert is lost and none adds risk
  (spec §5.1 to §5.8, DEC-438 items 4 to 9, 15, 16, 27 to 29). Tests first, against a fixture
  channel. *Accepted when:* the dispatcher runs as its own process, writes only the notice stream,
  sends only about committed causes, and journals every attempt (NT-8, crash injection at every
  step, and a second dispatcher fenced by epoch); one user kill switch is one notice (spec §3.4);
  recipients match the identity spec's receive column read as data (NT-10); quiet hours act by class (NT-7, the DST cases); safety
  storms are coalesced and never dropped, with the journal-derived oracle of NT-6 seeded with a
  dropping bug first to show it fails; with every channel failing, a soak's intents and modes match
  perfect delivery except asks that skip (NT-5); the kill-switch and exit suites pass with the
  dispatcher hung (NT-9); a lost address alerts on the other channels (spec §5.6).
- **E8-11 (Must, M7; SC)** As an approver, I want email notices (spec §4.4). Provider per DEC-438
  item 21, decided in DEC-820 item 3 for the founder's own address only; every other recipient waits
  for counsel's general footer.
  *Accepted when:* NT-1's canary test passes on captured messages; links match
  `<origin>/n/<notice id>` (NT-4); no reply is read (NT-3 fuzz); tracking is off in the provider
  configuration check; the founder-only SMTP transport (spec §4.4, DEC-820 items 3 and 4) sends only
  to the one founder address the deployment's configuration names, read from the vault, with exactly
  `Sent by your Mandate workspace to its owner.` as the footer; a notice for any other recipient, a
  workspace owner who is not the founder included, is refused `recipient_not_permitted` before any
  connection, is not retried, marks no address `unreachable`, and raises no `channel_lost`; the
  `xtask` email-footer check (DEC-700 item 4) exists, runs in `cargo xtask ci fast`'s lint, and is
  shown to fail on a planted third mail transport and on a planted deny-listed mail crate (`lettre`)
  outside the founder-only transport while the general footer is unresolved.
- **E8-12 (Deferred: not in v1; SC)** As an approver, I want one chat channel (spec §4.5). The
  founder did not take DEC-438 item 20 (2026-10-08, DEC-824): v1 has no chat channel, and Telegram
  at M10 is a later option. If it is built, *accepted when:* NT-1's canary test passes; every inbound message, button, or
  callback leaves the control stream unchanged (NT-3); the webhook URL or bot token is read only
  from the vault and appears in no log (rule 7). For Telegram (spec §4.5, DEC-700 item 2): a linking
  code is refused once 10 minutes have passed since it was shown; it is spent by the first message
  that carries it, even when no address is then recorded; showing a new code revokes the earlier
  one; and the replies to an unknown, an expired, and a spent code are byte-identical.
- **E8-13 (Must, M9 and M10; SC)** As an approver, I want to open a notice, sign in, and answer
  inside my workspace (spec §6, G4), with `web_inbox` as a pull channel (DEC-438 item 3). Depends on
  the workspace API and identity specs (DEC-436, DEC-437). *Accepted when:* a captured link with no
  session reaches only sign-in (NT-4); another workspace's subject answers as a missing one (NT-10);
  a grant needs step-up per mandate spec §6.1 and a skip does not; a closed request shows its
  terminal state; the service worker and pages cache no approval content (P5).
- **E8-14 (Must, M10; SC)** As an approver, I want web push, through the relay where a deployment's
  egress requires it (spec §4.6, DEC-438 item 13). *Accepted when:* payloads are encrypted to the
  subscription; the relay refuses ciphertext over 512 bytes and stores none; NT-1's canary test
  passes on relay and push-service captures; a relay outage changes no trading state (NT-9).
- **E8-16 (Should, M7)** As a reviewer, I want the minors of the notifications spec's round 1
  ([#558](https://github.com/kunwarshivam/mandate/pull/558); freeze rule) fixed in the spec:
  - HLD §6 flow C step 4 still says "an escalation chain (push → SMS → phone call)"; align it with
    DEC-438 item 6's fan-out.
  - The Telegram linking code: state its lifetime, entropy, and single use, and add the residual to
    §9 (whoever obtains the code binds their own chat and receives the opaque notices).
  - NT-1's wording against the relay envelope: the `urgency` and TTL the relay and push service see
    are fixed per class, and the push endpoint is an address under NT-2.
  - A rung-2 guard that no mail adapter ships while `[[EMAIL-FOOTER]]` is unresolved.
  - NT-5: state why check 4 is the only effect (no exit, protective order, or risk exit is ever
    gated by an approval, rule 13), so the claim survives a later autonomy change.
  - NT-6: say that a notice joining a coalescing window meets the 60-second bound at the window's
    end.
  - Round 2 nits: §3.2's preamble says every non-approval kind is caused by an `OwnerAlertSent`,
    but `channel_lost` is caused by the dispatcher's own `NoticeAttempted` (§3.4 has it right);
    say where the journal change is described that `StreamType` and `StreamId::parse` in
    `mandate-journal` are a closed four-variant type E8-10 must extend first; E8-9 reconciles the
    mandate reference cases' `OwnerAlertSent` `{subject, text: "tripwire_fired"}` with §3.2's
    `kind`; keep one sentence, here or in the workspace API spec §3.9, for what
    `ApprovalRef::of_requested_event` becomes.
- **E8-15 (Must, M8, after E10-10; SC)** As an approver, I want to see and answer approval requests
  through the workspace API, so that the web app and the CLI share one approval service
  ([workspace API spec](../specs/workspace-api.md) §4.3, §5.2, §5.3; [DEC-436](decisions/DEC-436.md)).
  *Accepted when:* `GET /approvals/{id}` returns the content object and `content_hash` exactly as
  `ApprovalRequested` holds them, with no scorecard, profit estimate, or price target (DEC-126); a
  response commits one `ApprovalResponseSubmitted` with the client's content hash unchanged and the
  server's `submitted_at`; `approved` needs step-up bound to that hash, and `skipped` is refused only
  for authentication, role, or a malformed request (API-7); a client token cannot respond or
  preview (API-6); a delegation preview is refused for an admission, a two-approver ask, a live
  environment, or a client, and the chosen shape commits `MandateVersionCreated`, `MandateConfirmed`,
  and the response in one batch under one step-up; the UI-facing status says "approved" only after
  `ApprovalRevalidated` with `act` (API-12); `/n/{notice_id}` resolves a random notice id, never
  the request's event id, and only after sign-in (spec §3.9, DEC-436 item 19; the payload is the
  notifications spec's, E8-9).

### E9 Identity, tenancy, and policy

The [identity spec](../specs/identity.md) (v0.1 draft, [DEC-437](decisions/DEC-437.md)) defines these
stories. Rows marked **SC** are safety-critical: tests first under
DEC-77, an independent review on a different model, and zero missed mutants. The identity provider
for managed mode, SAML, and organization recovery stay with the founder (DEC-437 items 15 to 21); no
story buys a service, and none uses a real identity-provider account in tests (spec §1.3).

- **E9-1 (Must; SC)** As a user, I want to sign in with passkey or OIDC SSO.
  *Accepted when:* passkey sign-in requires user verification and OIDC sign-in checks signature,
  issuer, audience, expiry, nonce, and `email_verified` against the configured issuer only (spec §6.1),
  each refusal tested against an in-memory issuer and a software authenticator; sessions meet spec
  §6.2 (5-minute access tokens, uncached membership re-check, refresh rotation with reuse revoking the
  family, idle and absolute limits an org can only shorten); the risk-reduction path of §6.4 pauses and
  engages a kill switch with the identity provider unreachable (ID-10), while a refusal from the
  provider (`invalid_grant`, a disabled subject, a back-channel logout) ends the session with every
  permission and blocks the local passkey route (§6.4, §11.1); the host CLI commits only as its
  registered principal (`HostCliRegistered`, ID-1); and the log scan finds no
  canary token from any path (ID-9).
- **E9-2 (Must; SC)** As an admin, I want organizations, workspaces, and roles.
  *Accepted when:* `authorize` matches spec §4.2's matrix exactly, checked by an exhaustive test over
  every role set, permission, and scope against a table parsed from the spec, not from the code (ID-2);
  no principal changes its own roles (ID-13); and the last-owner and last-admin refusals hold (§5.2).
- **E9-3 (Must)** As an admin, I want org-level limits that workspaces and agents can only
  tighten.
- **E9-4 (Must; SC)** As a security-conscious user, I want step-up authentication for sensitive
  actions. *Accepted when:* every permission marked S in spec §4.2 is refused, with nothing committed,
  for a missing, stale, reused, wrong-method, wrong-principal, and wrong-digest assertion (ID-4, §7.2);
  none of the ID-5 actions asks for step-up or fails without it; a challenge is consumed in the same
  transaction as the event it authorizes; and the raw assertion artifact re-verifies against the stored
  public key.
- **E9-5 (Should; SC)** As a fund, I want separation of duties between agent creators and approvers.
  *Accepted when:* under `independent_approval_required`, no grant, acknowledgment, or approval counts
  where `human(responder)` equals `human(requester)` or, for approvals, `human(author)`, with clients
  and service accounts mapped to their humans (ID-6, spec §8.2), checked by a fuzz whose oracle maps
  principals independently; and the 24-hour cool-off of spec §8.3 holds for operator and approver grants.
- **E9-6 (Must)** As a retail user, I want the retail profile (`auto` allowed, LLM ideas allowed,
  protection required, no leveraged ETPs, counsel-set loss ceiling and approval timeout minimum)
  applied to my workspace by default ([DEC-98](04-decision-log.md#decisions)).
- **E9-7 (Must, M8; SC)** As an admin, I want memberships with states (invited, cooling off, active,
  deactivated, removed) journaled on the control stream, so that who can act, and the user count V-047
  reads, come from the record (spec §5). *Accepted when:* the tests PR adds spec §12.1's events to
  journal §9 with schemas; `workspace_users` equals an independent fold of `Member*` events at
  validation and at application over random histories, with invited, cooling-off, deactivated, and
  removed members, clients, service accounts, and agents counting zero and an unreadable count reading
  as one (ID-7); and no request authorized after a deactivation commits succeeds, with open streams
  closed within 60 s (ID-3).
  *Follow-ups* (#789's second review, minors backlogged under the freeze rule):
  - Identity spec §5.1's state diagram has no edge for removing a role from a `deactivated` member,
    which journal spec §9.12's fold accepts (DEC-654 item 7); add the self-edge.
  - DEC-654 item 7 lets `change_roles` remove a role from an `invited` membership, but §9.12 has no
    record for it: an invitation's roles are fixed by `MemberInvited`. Until a record exists, the
    writer maps it to `MemberInvitationRevoked` then `MemberInvited` with the reduced roles (merge
    coordinator, 2026-10-09); whether to add a record stays open.
  - Fold vectors (#812's review, access-reducing): a reactivation with a strict subset of the kept
    roles, an activation with a strict subset of the invitation's roles, and a grant of a role held
    but still cooling off are each refused, but only disjoint roles are tested; add a valid and a
    refused history and a fold mutant for each.
  - Rule 103's non-cooling branch has no vector with a gap under 1 s (#789's delta review); add an
    invalid 1 ns gap and a mutant.
- **E9-8 (Must, M8; SC)** As a workspace owner, I want my data unreachable from any other workspace.
  *Accepted when:* data APIs take only a `TenantContext` the authorization step constructs, with
  compile-fail tests for a bare workspace ID; and cross-workspace attack tests fail at the API, row-level
  security, journal stream prefixes, NATS accounts, cache keys, the inference cache, and the vault
  (ID-8, spec §9.1).
- **E9-9 (Must, M8; SC; before E10-6)** As an owner, I want my connected agent to hold a scoped,
  sender-constrained, revocable token (DEC-141). *Accepted when:* clients connect through OAuth 2.1
  with PKCE and DPoP (spec §6.6); the ID-2 test passes for the client column; a client cannot approve,
  confirm, present step-up, pause, resume, stop, release, make an owner exit, or change a connection or
  membership (ID-11); the tests PR adds the `client` actor kind with `on_behalf_of` to journal §3 and
  makes the mandate spec's independence checks compare `human(…)` (spec §8.2, §12.2; DEC-437 item 10),
  with a case showing check 3 refuses a `client` actor's approval from the record alone; no client
  token is issued before those edits land; and revocation applies to every request authorized after it
  commits.
- **E9-10 (Should, M8; SC)** As a user, I want account recovery and, in managed mode, audited
  break-glass. *Accepted when:* recovery codes are stored only as salted slow hashes and shown once; a
  passkey enrolled through recovery cannot present step-up for 24 hours and its owner is notified
  (opaque) (spec §10.1); and a `platform_operator` is refused every permission outside break-glass's
  operational set, inside an approved, time-bound window only, with each step journaled to the
  workspace's control stream (ID-12, §10.3). Organization recovery waits for DEC-437 item 18.
- **E9-11 (Must, M8; SC)** As the founder, I want the identity invariants ID-1 to ID-15 each backed by
  a property test with an independent oracle. *Accepted when:* each oracle is shown to fail on a
  seeded bug (for example, a cached membership read, a role inherited from the org, a challenge not
  bound to its digest, a client counted as a second user, an unprefixed cache key) before it is trusted.
- **E9-12 (Should, M8)** Round-1 minors of the identity spec's review ([#556](https://github.com/kunwarshivam/mandate/pull/556),
  freeze rule). *Accepted when the spec settles each:* (1) the workspace admin holds the kill switch's
  privileges beyond the stop (mandate §6.1, selling equities outside the session) but not the owner
  exit; make the two rows agree; (2) ID-2's exhaustive test needs a matrix row for every operation the
  workspace API spec defines; the owner request, dry run, and chat thread rows were added by the
  round-2 ruling, and saving a draft, running a backtest, and resolving a notice still need rows, or
  ID-2 is scoped to the rows the matrix names; (3) a row for lifting a hold an operator
  set, with its step-up; (4) ID-15's carve-out for the notification relay's envelope
  (`{relay_id, endpoint, ciphertext}`, a capability URL) as HLD "Where data lives" lists it; (5) "release"
  names both Stop with release (DEC-136) and re-enabling a halted scope, so one is renamed (round 2
  renamed the second; confirm no other use remains); (6) the Status row matches DEC-437 after round 2.
  ID-13's founding-grant exception and ID-10's note on item 15 were taken in round 2 because the
  blocker fixes touched those lines.

### E10 Mandate authoring

- **E10-1 (Must)** As an operator, I want to describe an agent in plain language and get a
  compiled mandate with inferred fields highlighted. *Accepted when:* compiled mandates validate
  against the [mandate spec](../specs/mandate.md) (schema, V-rules, policy hierarchy; reference
  cases MC-S, MC-V, and MC-P pass); proposed envelope values are marked as proposed and no envelope
  field activates unconfirmed ([DEC-97](04-decision-log.md#decisions)).
  Merged (claim #124, stream H4; [DEC-428](decisions/DEC-428.md), tests #536 then implementation #580):
  `mandate-spec`'s `ValidationContext` carries the effective `independent_approval_required`, V-047 refuses it in a
  workspace of fewer than two users at validation, and the harness reads the cases' context member, so MC-V69 to
  MC-V71 pass. V-047 also refuses again when a version is applied, as V-002 does (`validate::recheck_at_application`
  rechecks V-002 and V-047), and a Rust test through `from_journal` covers a second user deactivated between
  confirmation and application (DEC-428 item 4; #528 round 2, major 1; the reference models V-047 at validation only,
  DEC-411 item 5).
  Owed when `Membership` gets a producer (#536 round 1, minor 4): a test pins that pending invitations and deactivated
  accounts do not count as users (spec V-047, DEC-411 item 2). `mandate-spec` receives a count, so it cannot pin it.
  The founder's DEC-444 (2026-10-03) lets a version §9.2 rates risk-reducing through V-047; the spec, `ref.py` and
  MC-V72 to MC-V77 carry it first (ES-22), then the tests PR (#536) follows. The exception reads the agent's current
  version matched by hash (DEC-444 item 3); `PreviousVersion` has no digest yet, so the Rust side refuses every version
  under V-047 until it does, and the test that a reducing version passes lands with the digest.
  When the digest lands (#536 round 2, minor 1), `v047_refuses_a_reducing_version_it_cannot_match_to_the_current_version`
  is re-aimed, never inverted: its context builder `after()` supplies a digest that does not match the agent's current
  `mandate_version` (or no current version), so it keeps stating that an unmatched document is refused, beside the new
  test in which a matched reducing version passes. Also owed then (#536 round 2, minor 3): the two V-047 property tests
  draw `previous_version` (absent, identity only, matched, unmatched, reducing, neutral, increasing), so the DEC-444
  surface does not rest on five hand-written shapes. #536's description listed its "DEC-444 without a digest" plant as
  caught by both new tests; only the reducing one catches it, by design (#536 round 2, minor 2).
- **E10-2 (Must)** As an operator, I want to edit the mandate as a form or YAML, kept in sync.
- **E10-3 (Must)** As an operator, I want mandates versioned with viewable diffs, and changes that
  increase risk to require step-up. *Accepted when:* the version vector and MC-C01 to MC-C48 pass.
- **E10-4 (Must)** As an operator, I want going live to require a backtest, a paper run, and
  step-up approval.
- **E10-5 (Should)** As a new user, I want templates for common mandates.
- **E10-6 (Should, M8, pulled forward)** As an owner who already runs my own agent (for example
  Claude), I want to connect it through a Mandate MCP server that exposes the same API Mandate uses
  to take my input, so that my agent can work inside my mandate without a separate path around it
  ([DEC-141](04-decision-log.md#decisions), [DEC-148](04-decision-log.md#decisions)). An optional
  channel and an adoption on-ramp, never the only or the main path: the complete product leads, and
  the platform's research agent brings the ideas. **Placement (DEC-148):** the first M8 story after
  the owner-input API (M8's mandate registry, approval service, and owner controls), E9-1, E9-2,
  E9-4, and E10-3, ahead of M8's other Should stories (E9-5, E10-5); it does not wait for M9 or M10,
  and its earlier dependencies (E5, E6-2, E6-3, E7-2, E7-3, E7-5, and the tracer bullet E7-7) land
  before the Phase 1 gate. No change to milestone order. *Accepted when:* every client request
  passes through the same order builder, autonomy rules, risk gate, account ledger, and journal as
  the owner's own input; the client cannot change an envelope field (it may only propose a mandate
  version that the human confirms with step-up); ASK approvals go only to the human owner, and a
  client cannot approve its own proposal; owner-only privileges (owner exits outside the regular
  session at a confirmed bid, Stop and release, the kill switch) stay with the human; the client
  authenticates with its own scoped, revocable token and no broker credential crosses MCP; every
  client call is journaled with the client's identity; and the owner can revoke the client at any
  time.
  *Amended by [DEC-183](04-decision-log.md#decisions) (founder, 2026-09-30):* packaged as the Owlhead plugin for OpenAI
  Dots, Meta Muse, and Grok Bot (E10-8), and every client-requested opening meets the client ceiling
  (E6-12).
- **E10-7 (Must)** As a new owner, I want to answer three questions (how much money, what goal, how
  much I can stand to lose) and get a complete mandate drafted for me to confirm on one card, so
  that I do not have to write a mandate to start ([ADR-0003](../adr/0003-earned-autonomy.md) part 1, [DEC-182](04-decision-log.md#decisions)). Builds on E10-1's
  compiler; within mandate spec §7. *Accepted when:* the three answers become `user_stated` fields
  (`capital.allocation_usd`, `goal`, `capital.max_loss_from_allocation`) with their quoted spans;
  every other drafted field is `platform_proposed` and inactive until confirmed (MI-12, V-020); no
  `auto`, delegation, pinned instrument, environment, or connection is ever proposed (V-022,
  V-038); what the goal types cannot express is flagged not enforced; the card shows the unasked
  dollars ([DEC-189](04-decision-log.md#decisions)); and it binds the version hash on confirmation.
- **E10-8 (Should, after E10-6 and E6-12)** As an owner who lives in OpenAI Dots, Meta Muse, or Grok
  Bot, I want Owlhead as a plugin there, so that my everyday agent can work with my money through
  Owlhead's gate ([DEC-183](04-decision-log.md#decisions)). Listed publicly as Owlhead, linking
  owlhead.ai (DEC-171). *Accepted when:* E10-6's acceptance holds for each host; the listing and its
  consent screen name the scopes in words; confirmations and approvals happen only in Owlhead's own
  app or CLI; each host's plugin terms are recorded in the competitive landscape.
- **E10-9 (Should, with E10-8)** As a connected agent, I want to ask "would this be allowed?" before
  asking the owner, so that I do not flood them with asks the gate would deny ([DEC-190](04-decision-log.md#decisions)).
  *Accepted when:* the dry-run tool returns the decision and the gate's reason code with the client
  ceiling applied, places nothing, creates no approval, counts against the client's rate limit, and
  is journaled. **Also ([DEC-191](04-decision-log.md#decisions)):** a hold-new-openings tool that sets
  `exits_only` and nothing else; lifting it is the owner's alone, with step-up.

*The workspace services API* ([spec](../specs/workspace-api.md) v0.1 draft, [DEC-436](decisions/DEC-436.md)). These
are the M8 owner-input API that E10-6 waits for (DEC-148). **SC** marks a safety-critical story.

- **E10-10 (Must, M8, after E10-15; SC)** As the owner, I want one authenticated, idempotent API in
  front of my workspace deployment, so that every surface takes my input the same way (spec §1 to
  §3). *Accepted when:* every route without credentials returns 401 with one body (API-1); a route ×
  role × client-scope matrix test matches spec §3.7 and §3.8, computed from the tables (API-2);
  every mutating call commits its control-stream event before reporting `recorded`, and fault
  injection on the append shows no effect without its cause (API-3, API-13); a fuzz of retries and
  concurrent repeats commits at most one event per key (API-4); foreign and absent ids return the
  same 404 (API-9); errors carry `code` and `effect` (spec §3.5); CSRF defences hold for every
  mutating route (spec §3.3); the OpenAPI document is generated and checked in CI.
- **E10-11 (Must, M8, after E10-10; SC)** As an operator, I want drafts, compile, validate,
  versions, diffs, and confirmation through the API (spec §4.1, §5.1). *Accepted when:* the server
  classifies every confirm itself and refuses `classification_changed` and `stale_base`; a
  risk-increasing confirm without step-up bound to `{mandate_version, agent_id, base_version}` is
  refused; a client can create a draft and nothing after it (API-5, API-6); a fuzz of random edits
  shows every envelope change of a deployed agent came through a user's `MandateConfirmed`; the
  compiler never proposes `auto`, a delegation, pinned instruments, the environment, or the
  connection (V-022, V-038); two writers never merge a draft silently (API-19).
- **E10-12 (Must, M8, after E10-10; SC)** As an operator, I want deploy, pause, resume, hold, Stop,
  owner exit, acknowledgment, and the kill switch through the API, with command status (spec §4.2,
  §5.4, §5.5). *Accepted when:* pause, hold, the kill switch, and owner exit are recorded with every
  condition of API-7 injected (rate limit, stale or missing read model, missing step-up, runtime,
  model gateway, market data, global control plane down, frozen control stream); the kill switch
  commits within its bound with every ordinary worker busy and the read-model tables gone (API-8);
  a kill switch without step-up is recorded and stops (DEC-158 (c)); go-live returns
  `live_unavailable`; status moves `recorded` → `taken` → `applied` or `refused` from the owning
  streams' events only.
- **E10-13 (Must, M8, with the connections spec; SC)** As a workspace admin, I want to connect and
  revoke broker accounts through the API without any credential ever coming back (spec §4.5).
  *Accepted when:* responses match a schema with no secret-shaped member and canary-secret scans of
  responses, logs, and the journal find nothing (API-11); connect needs step-up and a single-use
  OAuth `state`; scopes beyond trading reject the connection (FR-2.2); an ordinary revoke is refused
  while an agent on the connection holds positions or is not stopped; a revoke with `compromised:
  true` commits the connection-scope kill switch and then `ConnectionRevoked` in one batch, never
  waits on positions, commits the kill switch even without step-up or with the control stream
  frozen, destroys the credential after the kill switch's requests are sent, and tells the owner
  which positions remain at the broker (spec §5.6, DEC-436 item 20).
- **E10-14 (Must, M8, before E10-6; SC)** As an owner, I want my connected agent's token limited to
  `read`, `request`, `propose`, `dry_run`, and `hold`, so that it is owner input and never the owner
  (spec §3.8, DEC-141). *Accepted when:* every other route refuses a client token; `requested_by` is
  `client` whatever the body says; client events carry the `client` actor kind with `on_behalf_of`;
  a token presented without a matching DPoP proof is refused, as are CLI and service-account tokens
  without one (spec §3.3 item 2); every client call, reads included, is journaled; revocation fails
  the next call; creating a client needs step-up and revoking needs none.
- **E10-15 (Must, M8, first; journal spec change, tests first)** As an engineer, I want the journal
  events the API needs defined before the API writes them (DEC-436 item 14). *Accepted when:* the
  journal spec closes `MandateDraftSaved`, the compiler's `ModelInvocationRecorded` on the control
  stream, `MandateConfirmed`'s `agent_id` and `base_version`, `OwnerRequestSubmitted`, the
  `hold_openings` and `lift_hold` commands, the `client` actor kind with `on_behalf_of` (refused by
  approval check 3, counted as its user by check 7), `ConnectionRevoked`'s reason `compromised`,
  and `ClientConnected` and `ClientRevoked` (with the identity spec's §12.1; `ScopeHalted` and
  `ScopeReenabled` only if DEC-437 item 21 is accepted), each
  with test vectors and an invalid draft per rule.
- **E10-16 (Must, M6, before the first paper trade; SC)** As the founder, I want to register,
  confirm and deploy a paper mandate version from the CLI, so that a paper deployment rests on a
  confirmed version before the workspace API exists ([first paper trade brief](tasks/first-paper-trade.md),
  [DEC-505](decisions/DEC-505.md)). The CLI has no production control-stream writer today: its
  `ControlJournal` has only test implementations. *Accepted when:* a `mandate-journal-pg`
  implementation of `ControlJournal` appends through Postgres's artifact-aware append with the
  `mandate-artifacts-fs` store, with `--journal` and `--store` options, tested against Postgres
  under `MANDATE_PG_URL`; `journal export` writes one Postgres stream as the segment file
  `journal verify` reads; `config register`, `model register`,
  `version create`, `version confirm` and `agent deploy` each commit exactly one control-stream
  event (DEC-155 item 5) with journal spec §9.2's payload, after storing every object a `ref`
  names; `version confirm` and `agent deploy` take fresh `cli_confirm` evidence and record it in
  the event's record; every command refuses a `live` mandate or connection; no command writes
  `ConnectionEstablished` (its permission check is E7-12's); a re-run of a committed command
  commits nothing (DEC-290); and the records fold, through `JournaledFact::from_record`, to a
  context in which the mandate validates.
- **E10-17 (Should, M6; SC)** As an engineer, I want a test that pins the text of the CLI's
  journal errors, so that a change to `the journal failed: <code>` is caught rather than only
  documented (DEC-520 item 1; #680's review, minor 2). *Accepted when:* a test drives
  `PgControlJournal` to each answerless outcome (`unavailable`, `integrity`, and an append's failed
  §11 check) and asserts the exact message, and no message carries the DSN.
- **E10-18 (Should, M6; SC)** As the founder, I want the CLI to say why it could not reach the
  journal, so that a refused password, a missing database or a TLS failure is not reported as the
  same `unavailable` as a database that is down (#680's review, minor 3). *Accepted when:*
  connection-time failures map to distinct stable codes in `ControlError::Journal`, tested against
  Postgres, and none carries the DSN or its password.

- **E10-19 (Should, Phase 3 unless pulled into Phase 2, after the first paper trade; SC)** As an
  owner, I want an agent that only monitors (it watches the instruments and conditions I confirm and
  alerts me, and never opens a position), so that I can follow an instrument before I let an agent
  trade it ([DEC-528](decisions/DEC-528.md) item 4; PRD FR-11.1; product experience brief D16).
  *Depends on:*
  - a mandate spec change, which is not written yet and touches a protected path. It must say how a
    monitor agent is expressed (an envelope with no allocation, or autonomy that denies `open` and
    `increase`), what it may alert on, and which validation rules and reference cases change;
  - E8-5's opaque notifications.

  *Accepted when:*
  - the gate and the autonomy policy deny every opening and increase for a monitor agent, whatever
    its signal models say, tested over random inputs;
  - it holds nothing, so it has nothing to exit;
  - each alert is journaled with the facts or the quoted, attributed model output behind it;
  - every notification carries only an opaque ID and generic text (rule 6);
  - turning a monitor agent into one that trades is a new mandate the owner confirms, never an
    in-place change.
- **E10-20 (Must, M6, blocker for starting an agent on the L2 host; SC)** As the founder, I want
  `mandate agent pause` and `mandate agent kill` reachable from the CLI, so that the SSH
  kill-switch fallback of [DEC-822](decisions/DEC-822.md) item 7 works and the rehearsal in
  FOUNDER-STEPS step 16 (`deploy/README.md`) can pass. The code exists in
  `crates/mandate-cli/src/agent.rs` and `control.rs`; `AgentCommand` in `gestures.rs` does not name
  them. *Accepted when:* `AgentCommand` carries `pause` and `kill`, wired to that code; an agent
  kill touches only its own scope (rule 13); and a test runs each command through the binary
  against the journal. No agent starts on the L2 host before this lands.

### E11 Web app: dashboard and controls

- **E11-1 (Must)** As an operator, I want a dashboard of agents, state, positions, P&L, open
  approvals, and recent decisions.
- **E11-2 (Must)** As an operator, I want pause, resume, stop, and kill switches in the UI.
- **E11-3 (Must)** As an operator, I want alerts for risk rungs, reconciliation mismatches,
  stale data, and paused agents.
- **E11-4 (Should)** As an owner, I want a plan view that says what my agent is watching, what it
  would do next under which rule, whether that would run on its own or ask me, and what would stop
  it ([DEC-184](04-decision-log.md#decisions)). A read of the mandate and the runtime's state; changes nothing.
- **E11-5 (Should)** As an owner, I want one daily brief of what ran, what was skipped, what my
  delegations let through, which guardrails fired, and what changed, so that only urgent things
  interrupt me ([DEC-184](04-decision-log.md#decisions); PX-16). The notification says only that the brief is ready
  (rule 6).
- **E11-6 (Should)** As an owner, I want to talk to my agent in a chat thread, where a message
  becomes an owner request (builder, gate, and autonomy rules) or a proposed mandate version
  (confirmed with step-up), never an order by itself ([DEC-184](04-decision-log.md#decisions), [DEC-192](04-decision-log.md#decisions)).
  *Accepted when:* a model reply never renders as an action control; action cards are built by
  deterministic code; "why" answers come from the journal (E12-5).
- **E11-7 (Should)** As an owner, I want to see the unasked dollars (what can trade without asking
  right now) on the card, the dial, and the brief, and to switch on away mode in one action, so
  that I can size my agent's autonomy and turn it down when I cannot answer ([DEC-189](04-decision-log.md#decisions),
  [DEC-194](04-decision-log.md#decisions)). *Accepted when:* the figure matches the reference model; away mode writes a
  risk-reducing version that applies at once, and at its end date asks me to restore rather than
  restoring itself.
- **E11-8 (Should, after E10-3)** As an owner, I want to see what would have changed over the last
  30 days before I confirm a version that adds autonomy, so that I widen it on evidence
  ([DEC-186](04-decision-log.md#decisions)). Counts only, replayed from the journal; no profit, loss, or outcome; wording
  waits for compliance question 39.
- **E11-9 (Must, M9, after E10-10)** As an operator, I want the dashboard, agent, position, order,
  P&L, universe, plan, brief, autonomy, and alert views served from the journal with their age, so
  that the web app leaves its fixtures and never shows old data as current
  ([workspace API spec](../specs/workspace-api.md) §4.7, §4.9, §6; [DEC-436](decisions/DEC-436.md)). *Accepted when:*
  rebuilding every read model from the journal alone gives identical responses (API-14); every
  response carries per-stream watermarks; values past their freshness limit are marked stale with
  their age; model text appears only in quoted, attributed members (API-18); scorecards appear only
  at their own route (FR-8.4); each fixture type of `web/src/fixtures/types.ts` has its source route.
- **E11-10 (Should, M9, after the first paper trade and E11-9)** As an owner, I want Home
  (signed-in, D1) to show my connected accounts, their holdings and "no agents deployed", so that I
  start from what I have ([DEC-528](decisions/DEC-528.md) item 4; PRD FR-8.5; product experience brief J-H, D1,
  H1).
  *Depends on:*
  - E11-9's journal-served read models;
  - E7-6 for Robinhood accounts;
  - a workspace API spec change for the holdings route (protected; not written yet).

  *Accepted when:*
  - holdings are read-only and per connection, each with its age;
  - a request names only the connection's own account, so for Robinhood only the agentic account
    appears (CN-8), tested with the multi-account fixtures;
  - a holding no agent bought is labeled the owner's and is never attributed to an agent or offered
    for adoption (CN-7, DEC-528 item 3);
  - an unreadable connection is named while the others still show;
  - no buy or sell control appears on the view.
- **E11-11 (Should, M9, after the first paper trade and E11-9)** As an owner, I want to search
  instruments by name or ticker and keep my own watchlists, so that I can find an instrument and ask
  an agent about it or set one up ([DEC-528](decisions/DEC-528.md) item 2; PRD FR-8.6; product
  experience brief I1 to I3).
  *Depends on:*
  - an instrument directory read: data-plane and workspace API spec changes, protected and not
    written yet;
  - one of these, recorded in the same spec change: a control-stream record for watchlist changes
    (a journal spec change), or a decision that watchlists are workspace configuration outside the
    journal.

  *Accepted when:*
  - results are ordered by match to the query text, then alphabetically (the order DEC-528
    proposes), and a test shows the order is unchanged by any price, volume, return or model output;
  - no route returns a ranking, score, flag or platform-authored list
    ([data-plane spec §1.4](../specs/data-plane.md#14-non-goals));
  - the instrument page offers "Ask an agent" (an owner request through the builder, gate and
    autonomy rules, DEC-141) and "Set up an agent", and never "buy" (Rule 12);
  - a watchlist steers no agent's universe and is never read by an agent: no agent-side code path
    reads a watchlist, tested at the read-model boundary (rule 11);
  - "Set up an agent" opens the ordinary goal questions (A0); an instrument enters an agent only
    through a mode the owner confirms in a mandate version (a pinned universe), never as a hint;
  - the Robinhood connector still calls no watchlist or scan tool
    ([connections spec §6.2](../specs/connections.md#62-how-mcp-maps-to-the-connector-interface)
    rule 2).

### E12 Audit explorer

- **E12-1 (Must)** As an auditor, I want a causal trace from any fill back to its causes.
  *Follow-ups (A2 review):* the quoted items' `author` reads `owner_selected` for an owner-selected
  signal model's output through `config_refs.mandate_version`, with the founder's label, once
  [DEC-773](decisions/DEC-773.md) is decided. Slice A2b tests: `payload.client_order_id` with its
  lower-`seq` filter, `OrderRequestRecorded`, `payload.approval`, `payload.outputs_used[]`, the
  approval's `outputs[]`, `payload.thesis_id`, `OrderSubmitted` version 1, the quoted members of
  `ThesisProposed` and `ModelInvocationRecorded`, a missing singular `IntentReceived`, and
  [DEC-772](decisions/DEC-772.md) items 8 and 9.
- **E12-2 (Must)** As an auditor, I want per-agent timelines with filters and JSON/CSV export.
- **E12-3 (Should)** As an auditor, I want to run chain verification from the UI.
  *Follow-ups ([#772](https://github.com/kunwarshivam/mandate/pull/772) review, journal spec):* rule 81
  does not check that a client's `on_behalf_of` differs from its own `id` (predates #772); rule
  108's break-glass check requires only a `causation_id` for a `platform_operator` read, not that it
  cites a `PlatformOperatorAction` (with the E12-3 tests PR); "never a ticker" in
  `RecordsAccessed.resources` and `operation` is prose only, both being `id`-typed; and the client
  clause of rules 109 and 110 is unreachable, since rule 83 refuses a client first.
- **E12-4 (Could, not yet planned)** As an owner, I want a monthly record of every mandate breach
  and near-breach on my account, derived from the journal and its anchors, so that I can see the
  mandate held ([strategy options §8](../product/10-strategy-options.md#defensible-differentiators),
  DEC-145). Publishing it beyond the owner needs counsel's answer and the founder (DEC-79).
  *Extended by [DEC-193](04-decision-log.md#decisions) (founder, 2026-09-30):* the record is checked against the hash
  chain and lists the versions in force, the actions under each by purpose and autonomy source,
  every delegation used, and every guardrail that fired, with no performance figure; its text waits
  for compliance question 40.
- **E12-5 (Should)** As an owner, I want "why did it do that?" answered from the journal, not from a
  model's memory ([DEC-192](04-decision-log.md#decisions)). *Accepted when:* each answer links the `DecisionMade`, thesis,
  rule or delegation, and gate result it cites; a model may phrase it but adds no fact; a missing
  fact reads "not recorded"; wording waits for compliance question 41.
- **E12-6 (Must, M9, after E10-10)** As an auditor, I want journal reads, traces, timelines, gate
  decisions, exports, and verification through the API, complete and checkable
  ([workspace API spec](../specs/workspace-api.md) §4.8; [DEC-436](decisions/DEC-436.md)). *Accepted when:* a fuzz of page
  sizes with concurrent appends shows concatenated pages equal the stream range exactly once, in
  `seq` order, with a passing chain check across pages (API-15); every export is journaled as
  `ExportCreated` before it is served, its canonical form passes the journal verifier, and derived
  JSON and CSV name their manifest hash (API-16); a viewer can read none of it and an auditor can act
  on nothing (API-2).
  *Follow-up (#797 review; [DEC-770](decisions/DEC-770.md) item 5):* journal spec §2 defines the
  notice stream `ntf:{workspace_id}`, but `mandate_journal::StreamId::parse` does not parse `ntf:`,
  so `mandate-audit`'s `StreamType::Notice` is unreachable and an `ntf:` id reads as absent. Owed:
  `StreamId` parses notice streams (journal, E5), then the audit reads list and page them, with a
  case in `crates/mandate-audit/tests/scope.rs`.

### E13 Hybrid deployment

- **E13-1 (Must)** As an IT admin, I want to install the workspace deployment with Helm or
  Docker Compose using an enrollment token, with outbound-only connectivity.
- **E13-2 (Must)** As an IT admin, I want signed releases and upgrades that preserve agent state.
- **E13-3 (Must)** As an IT admin, I want SSO against our identity provider and a fallback
  approval channel through our own mail server.

### E14 Billing

- **E14-1 (Must)** As an org owner, I want to subscribe to a plan and be billed per
  organization.
- **E14-2 (Must)** As an org owner, I want a hybrid license key tied to my organization.
- **E14-3 (Should)** As an org owner, I want usage metering visible in the app.

From the [billing design](../design/billing.md) (v0.1 draft, DEC-442). **SC** marks a story on a
safety-critical path, to which the `AGENTS.md` safety-critical rules apply. No story here opens a
provider account, sets a price, or charges anyone until DEC-442 items 14 to 20 are decided; each is
built against a recorded provider fake.

- **E14-4 (Must, M12)** As an org owner, I want my usage counted from my workspace's own records, so
  that I can check what I am billed. *Accepted when:* the usage builder seals one report per
  deployment per period from the closed counter list (design §4.1), `UsageReportSealed` before it
  is signed; a property test shows two journals that differ only in orders and fills give identical
  reports (BL-2); the canary run finds no canary byte in any report (BL-1); and
  `mandate-cli usage recount` equals every sealed report, failing on a seeded builder off-by-one
  (BL-7).
- **E14-5 (Must, M12)** As the platform, I want usage ingested exactly once and reconciled, so that
  no organization is billed twice or by estimate. *Accepted when:* fuzzed duplicate, reordered, and
  delayed deliveries with ingest crashes give clean-run totals (BL-6); a chain gap, a late
  air-gapped file, or a bounds-check failure holds the line and never estimates it (design §4.3);
  and corrections are new reports and new traced lines, never edits (BL-11).
- **E14-6 (Must, M12)** As the platform, I want the billing provider behind one interface with
  idempotent writes and verified webhooks, so that the provider choice stays reversible.
  *Accepted when:* every write carries the deterministic key of design §5.1 and a retry after a
  timeout bills once; an outbox resends after a crash; a forged, replayed, or contradicted webhook
  changes no state the provider API does not confirm (§5.3); the fake records every request and a
  scan finds only BL-1's fields.
- **E14-7 (Must, M12)** As an org owner, I want prices fixed for each cycle and every invoice line
  explained, so that I am never re-billed at a new price. *Accepted when:* price versions are
  immutable and pinned per cycle (BL-10); re-rating every closed cycle reproduces each line to the
  cent; each line names its counter, period, version, and report digest (BL-11); money is
  fixed-point with one rounding per line (BL-12); and bring-your-own-key tokens rate to zero with
  unchanged quota use (BL-9), with `key_owner` on the metering record (inference spec §7.1).
- **E14-8 (Must, M12; SC)** As an org owner, I want non-payment to follow a noticed ladder that never
  touches my positions or exits. *Accepted when:* licenses are issued from the plan through E20-4's
  format; the ladder of design §6.2 runs on a simulated clock with no early step and a notice
  before each; only an API-confirmed non-payment withholds renewal and a provider outage renews
  (BL-5, BL-14); and through every state, paper agents' exits, protective orders, owner exits, and
  kill switches pass, while after lapse every covered agent takes the `license_lapsed` restriction
  (`exits_only`, #562 CP-6), an opening is refused by the gate's existing mode check, and no new gate
  reason exists (design §3.4, BL-4); and every billing notice Mandate sends is `{notice,
  text}` through the dispatcher, with no canary or amount in any captured byte (BL-15).
- **E14-9 (Must, M12; SC)** As a billing admin, I want quotas and an organization spend cap enforced
  before any spend, so that the bill never exceeds the cap. *Accepted when:* workspace, deployment,
  seat, model-spend, and agent-hours quotas are checked at the points of design §3.4 with the
  control plane unreachable; a fuzz over calls, deployments, stops, restarts, and cycle boundaries
  shows spend never exceeds a cap by a separate accumulator; and no refusal appends a mode change,
  cancel, or exit (BL-8).
- **E14-10 (Should, M12)** As an org owner, I want sign-up, trial, plan changes, cancellation,
  refunds, and organization deletion to behave as the design's lifecycle walk says. *Accepted
  when:* each row of design §6.1 has a test from entry to exit; a downgrade below what runs stops no
  agent; a refund is a traced credit to the original method; and deletion keeps billing records for
  the retention period and the journal under its own rules.
- **E14-11 (Should, M12)** As a reader of the billing design, I want the round 1 review's text
  findings fixed (#565). *Accepted when:* BL-10 cites the price-change notice term (DEC-442 item 18)
  instead of pricing principle 3; the `model_cost_usd` counter row says that for `key_owner =
  customer` it is our price table applied to the customer's call, not anyone's actual cost; the
  Lapsed row of design §6.1 names the terminal state for an organization that never pays (lapsed,
  with read and export access for the records period); the free paper tier gets its own §6.1 row
  with its renewal and payment state; and design §10 either lists its rows inline or is retitled.
- **E14-12 (Should, M12)** As the coordinator, I want the billing design reconciled with the control
  plane (#562) and identity (#556) designs once they merge. *Accepted when:* #562 §3.5's counter
  list gains `key_owner`, `data_units`, and `cached_tokens` and renames `decisions` to
  `decision_cycles`; CP-6 lists resumes into an opening mode, as its own §3.3 does; identity §11.2
  takes the shared lapse wording (after grace a license refuses only new deployments and new
  openings, and never blocks an exit, a protective order, the kill switch, or any risk reduction),
  subject to the founder's DEC-440 item 13; and the identity invite walk checks `max_seats`, or the
  seat row of design §3.4 moves to where identity places it.

### E15 Signal models: LLM and fast models, scorecards

- **E15-1 (Must)** As an operator, I want an LLM research signal model that writes theses
  asynchronously without blocking trading.
- **E15-2 (Should)** As an operator, I want a fast decision model with a hard deadline.
- **E15-3 (Must, Phase 1)** As an operator, I want each signal model's confidence measured against
  outcomes and shown in scorecards ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* every thesis is scored after its horizon against the pre-registered baselines
  (buy-and-hold of the eligible basket, and a broad index ETF), net of modeled costs; scores come
  from forward paper trading only, never from historical backtests of LLM theses; the scorecard
  shows each signal model's results beside the baselines.
- **E15-4 (Could)** As an operator, I want shadow mode for a new mandate version.
- **E15-5 (Should, after the Phase 1 exit)** As an owner, I want to run up to three variants of my
  mandate at once, one live and the others in shadow, so that I can compare strategy changes before
  risking money ([strategy option 16](../product/10-strategy-options.md#option-16-mandate-experiments-multi-variant-shadow-mode-founder-2026-09-27)).
  Subsumes E15-4 (one shadow candidate is the one-variant case) and is pulled forward from Phase 3;
  no change to the v1 milestones. *Accepted when:*
  each variant is its own owner-confirmed mandate version that differs only in strategy fields (signal
  models, weights, thresholds, cadence); shadow variants see the same market data and pass the same
  gate, evaluated against their own simulated account state, never consuming or holding the live
  account's buying power, day-trade budget, or reservations (so no shadow variant can hold a live
  exit, rule 13); they keep a simulated book labelled as simulated, any comparison between variants is
  labelled as hypothetical performance, and they send nothing to the broker; each variant's
  hypothesis and success criterion are journaled before it runs; promotion is only an owner action
  that creates a new confirmed mandate version, with no automatic winner-picking; and no mandate holds
  more than three variants.

The stories below implement the [inference spec](../specs/inference.md) v0.2
([DEC-432](decisions/DEC-432.md)) once it is reviewed. **SC** marks a safety-critical story (the
model gateway feeds the order builder, holds provider credentials, and enforces spend caps). E15-2
follows the founder's decision on DEC-432 item 13: a hosted fast model in v1, through the gateway.

- **E15-6 (Must, M5; SC)** As an operator, I want a model gateway that is the only component able to
  call a model, so that every call is pinned, bounded by a deadline, and never substitutes a model.
  *Accepted when:* the call contract of spec §3 is implemented behind a provider adapter interface
  with a recorded-fixture adapter; property tests with independent oracles cover INF-1 to INF-5,
  INF-9, INF-11, and INF-13 (spec §2.1), each oracle shown to catch a seeded bug; an output after its
  deadline, from another identity, or failing its schema never reaches the caller; no request body
  carries a tool or function member; the agent runtime has no direct route to a model endpoint;
  live calls run only in internal paper workspaces (DEC-432 item 14).
- **E15-7 (Must, M5; SC)** As an owner, I want each selectable model to be a registry entry whose
  content hash covers its pinned content (spec §4.1: identity, template, retrieval plan, output
  schema, validation bounds, deadline, and the rest) and excludes `endpoints` and `status`, so that
  what I pin is exactly what runs and a routing change never invalidates my pin (DEC-432 items 17
  and 18). *Accepted when:* entries with a floating alias are refused; a test changes an entry's
  endpoints and status and asserts the hash is unchanged, and changes each pinned member and asserts
  it differs;
  an endpoint is added only after the spec §4.3 identity probe; deprecation and withdrawal follow
  spec §4.4 and never replace a pinned model; V-007 checks the mandate against the registry.
- **E15-8 (Must, M5; SC; journal spec first)** As an auditor, I want every model call journaled with
  its cost and outcome, so that replay never calls a model and spend folds from the journal.
  *Accepted when:* a journal spec change closes `ModelInvocationRecorded` with DEC-432 item 11's
  members and the two the agent harness spec adds (DEC-432 item 22), names the stream that holds the
  compiler's record (DEC-432 item 19; until then the compiler makes no gateway call), and adds the
  gateway's meter stream with one writer per workspace and the `meter_unavailable` refusal (DEC-432
  item 20), with test vectors; the registration lands tests first
  (DEC-77); replay of a journal with every model call failing yields the same fold (INF-10).
- **E15-9 (Must, M5; SC)** As an owner, I want model spend metered and capped per agent and per
  workspace, so that cost is bounded and a cap never adds risk. *Accepted when:* reservations precede
  the first attempt, settle on completion, and stay counted after a crash (DEC-432 item 4); a fuzz of
  calls, failures, cache hits, restarts, and risk-day changes shows spend never above any cap, by a
  separate accumulator over journaled records; `research_spend_usd_today` for check 7 folds from the
  meter stream; a refused call yields no output and changes no envelope field; the open question of a
  per-model cap for fast and LLM signal models is decided (spec §13 item 3), with a mandate spec change
  if it becomes an envelope field.
- **E15-10 (Must, M5; SC)** As a workspace admin, I want to choose which model endpoints and regions my
  workspace may use, including local-only, so that prompts never leave my site unless I allow it.
  *Accepted when:* a policy key for allowed endpoints and regions (child ⊆ parent) is specified in the
  mandate spec and `policy.schema.json` first; hybrid and on-prem default to local-only (DEC-432
  item 8); a call to a disallowed endpoint sends no byte (INF-12); the deterministic prompt guard of
  spec §8.3 refuses planted credentials, account identifiers, personal data, and another workspace's
  values in every input source; egress to model hosts is an allowlist enforced at the network layer.
- **E15-11 (Should, M12)** As an org owner, I want model usage in my bill at cost plus margin, so that
  I pay for what my agents used. *Accepted when:* the billing feed of spec §7.4 sends counts and cost
  only, never content, outputs, or instruments; hybrid sends signed reports; the result appears in
  E14-3's usage view.
- **E15-12 (Should; spec follow-ups, deferred by the freeze rule)** The minors of the independent
  review of inference spec v0.1 (PR #551, round 1), for the spec's next revision:
  - Minor 6: "withdrawal empties the cache" must not delete a write-once artifact a journaled record
    names. Say withdrawal invalidates cache index entries only.
  - Minor 7: INF-6's "exactly one metering record" against the meter stream's two appends
    (reservation and settlement). State which the INF-6 and INF-7 oracle counts, and that a
    `meter_unavailable` refusal has no meter-stream entry.
  - Minor 8: spec §7.3 says a crashed call's reservation "stays counted"; §9's crash row says it is
    "settled as spent at their maximum". Pick one fold and make the accumulator reproduce it.
  - Minor 9: INF-15 and §4.4 rely on `PlatformOperatorAction`'s `model_withdrawn`, whose payload
    schema the journal spec leaves open (DEC-261 item 9). Say so, so E15-7 assumes no closed schema.
  - Minor 10: no longer a scope change. The founder chose a hosted fast model for v1 (DEC-432
    item 13), so FR-3.7's P1 stands; update OD-02 to that decision.
  - Nit: spec §11 places the spike's 60 s timeout in the client; it is in `http.py`.
- **E15-13 (Must, M6, before the first paper trade; SC)** As an owner, I want my pinned quant
  model run by a model host outside the production cycle and the shell, so that its output is the
  pinned code's and nothing else's ([first paper trade brief](tasks/first-paper-trade.md),
  [DEC-503](decisions/DEC-503.md), [DEC-504](decisions/DEC-504.md)). Quant models make no model
  call and do not go through the gateway (inference spec §1.3); E15-6 and E15-7 stay with the LLM
  model. *Accepted when:* a new `mandate-modelhost` crate (layer 8, pure; its `xtask/layers.toml`
  and `CODEOWNERS` lines in the same change) computes each model's content object from its
  source bytes and a test pins the hash per registered version, so changed code under an old
  version fails the build; `evaluate` returns an output only when the mandate's pin, the one
  matching `model_registry` entry and the host's hash agree, every parameter is set and in bounds,
  and the closes are the pinned instrument's, complete, and end at the last completed session of
  the trading calendar passed in;
  every refusal is no output; `as_of` is the last close's end and `expires_at` is `as_of` plus
  `max_output_age_s`; a property test with an independent oracle shows the same inputs always give
  the same output; and the crate depends on no crate that sizes, gates, journals, executes or
  connects.

### E17 Research agent and dynamic universe

Design: [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) ([DEC-97](04-decision-log.md#decisions)).
The mandate spec, schemas, reference implementation, and cases are rewritten first in spec-change PRs.
The first delivery is the [DEC-103](04-decision-log.md#decisions) thin slice: the research agent
runs only in the team's internal paper workspaces, with the research basket (DEC-90) as its fixed
test data universe, every admission `ask`, paper only, and scorecards on. The basket is never a
user's instrument choice. The full E17-3, for users' agents under their own envelopes, follows only
after the DEC-99 evaluation (E17-8) passes on the thin slice.

- **E17-0 (Must, now)** research spike: an LLM loop over news and prices, paper-traded on the
  research basket with fixed sizing, to de-risk E17-2 before it is product code
  ([task brief](tasks/RS-1-research-spike.md); `python/research_spike/`).
  *Accepted when:* a report with hit rate, expectancy versus SPY, and cost per thesis after two to
  three weeks of paper trading.
- **E17-1 (Must)** As an owner, I want the mandate split into envelope fields I confirm and a working
  universe the platform produces at runtime, so that I set the risk and the agent brings the ideas.
  *Accepted when:* the rewritten mandate spec's cases pass; the compiler may propose envelope values,
  each marked as proposed; no envelope field activates unconfirmed.
- **E17-2 (Must)** As an owner, I want a research agent that turns market data, news, filings, and
  the agent's memory into theses (instrument, direction, horizon, evidence, invalidation), journaled
  as `ThesisProposed`, so that the agent has ideas without me.
  *Journal half ([DEC-413](decisions/DEC-413.md)):* journal spec v0.9 §9.4 closes
  `ThesisProposed` and `ThesisRevised` in one shared schema with rules 34 to 38, and the vectors
  gain a generated `research` section (merged in [#490](https://github.com/kunwarshivam/mandate/pull/490),
  stream J; #490 round 2's three minors, a `tighten.*` mutant kind with the sorted-sources rule that
  refused MC-N07, rule 36's internal order, and the two nits, on `agent/j3-thesis-vectors-minors`). Next,
  the registration in `mandate-journal` under DEC-77: a tests PR with stubs and pending tests, then
  the implementation. **Required, and blocking that registration's acceptance (#490 round 1, M2;
  as #470 round 2 minor 5 for §9.3):**
  - the registration re-derives mandate spec §8.5 checks 4, 5, 6, 10, and 16's cap from the
    mandate document the record's `config_refs.mandate_version` names, and refuses a record whose
    verdict passes over a check that mandate fails: an `admitted: true`, or a `reason` later than
    the first check the document fails;
  - **(#503 round 1, m3)** §9.4's `instrument_id` is typed looser than an asset ID, the latent twin
    of §9.3's `instrument` before v0.10. No mapping parses it yet. Journal spec v0.11 types it as
    `asset_id` with four `research` drafts (DEC-413 item 7, [#513](https://github.com/kunwarshivam/mandate/pull/513)), so the
    registration types it `Ty::AssetId` from the start and never appends a value a later mapping cannot read.
  - the `man` ref on these two records means the mandate in force when the thesis was judged, and
    the tests PR pins that;
  - the tests PR comes first (DEC-77), and its cases include a pinned universe (MI-20) and
    `admission: deny`.

  *Registration ([DEC-414](decisions/DEC-414.md)):* the tests PR merged in
  [#510](https://github.com/kunwarshivam/mandate/pull/510); the implementation PR, on
  `agent/j3-thesis-registration-impl`, registers the shared schema (`instrument_id` as
  `Ty::AssetId`) and rules 34 to 38 in `mandate-journal`, implements
  `mandate_spec::context::check_thesis_record`, removes `InvalidReason::Unimplemented`, and deletes
  only the ten `#[ignore]` lines. It lands after journal spec v0.11
  ([#513](https://github.com/kunwarshivam/mandate/pull/513), DEC-414 item 7). Every reader of a
  thesis record (the lineage fold, the writer's read-back) calls `check_thesis_record` before acting
  on it; no reader exists yet.
  *Follow-ups (#510 review round 1):*
  - Minor 1: `thesis_tests`' stored mandates are parsed, not validated, and break V-036 (the admitting
    model keeps `quant.momentum`). Make each valid with a second patched path (an `llm.` model id),
    as #482 round 2's m1 asks for `record_tests`.
  - **M1's rule, one rung up (the trust ladder):** a never-null test has missed a list's
    elements again (#445, three rounds running, for §9.2; #510 for §9.4). Add a test helper, or an xtask
    check, under which a never-null test derives its paths from the schema's own members, including
    the first and second element of every list member, instead of from a hand-written array.
  *Follow-ups (#513 review round 1):*
  - Minor 5: `ModelOutputRecorded.instrument_id` is §9.1 `text`, where mandate spec §8.2's output
    table types it `uuid`. Decide whether it becomes §9.3's `asset_id`: a §9.1 change, and its own
    story, spec first (ES-22, DEC-176). Type `PlatformOperatorAction`'s `research_thesis_halt`
    instrument (§9, DEC-100) the same way when that schema closes.
  *Follow-ups (#519 review round 1):*
  - Minor 1, done on `agent/j3-thesis-impl-minors`: the `order_rule_37_before_rule_38` draft pins rule
    37's report order before rule 38's when it is the only rule of 35 to 37 that fails.
  - Minor 2 (with minor 4): mandate spec §8.5's ordered seventeen refusal reasons live in three Rust
    places with nothing pinning them equal: `mandate-research`'s `RefusalReason`,
    `mandate-journal`'s `control::THESIS_REFUSALS`, and `mandate-spec`'s
    `context::THESIS_REFUSALS`. Give the list one home on a higher rung: a type in `mandate-domain`
    that the three read, or an xtask check that they agree. The same change should also give check
    numbers one form: `control.rs` compares a 0-based index with `CORROBORATION_CHECK` (15), where
    `context.rs` uses the 1-based check number.
    *Under way* ([DEC-415](decisions/DEC-415.md)): the type is `mandate_domain::ThesisRefusal`, live
    with its tests on `agent/j3-refusal-home-tests`; the implementation PR moves the three readers onto
    it, names every check by its number from 1, and changes no test file.
  - Minor 5: `control::horizon_agrees` fails closed (check 2 fails) on an instant or horizon it
    cannot read, where the reference validator skips the comparison. The schema guarantees both
    types today, so no record reaches the difference; if the schema ever stops guaranteeing them,
    pick one reading and pin it with a vector.

  The writer that adds the model's identity, the instants, and the artifact references to
  `ThesisEntry` is a story of its own. DEC-413 item 5 lists the readings not taken, each a later
  tightening. One is for E17-9's loop: whether a revision without an autopsy is a refusal reason,
  which would be a mandate spec change.
  *Follow-ups (#490 review round 1, under the freeze rule):*
  - Minor 1: `research.py`'s stored mandate fails V-007 against `reference/mandate/bases.py`'s
    registry, which pins `llm.research_agent` at the placeholder hash. That is expected, since the
    vectors store the model object, but DEC-413 item 4 should say so. Once stream L lands
    `jsonschema` in `python/` (founder-approved 2026-10-02), `documents.valid` moves to
    `reference/mandate`'s full validators with a registry that pins the stored content hash.
  - Minor 3: `invalidation` is §9.1's `text` (non-empty) while `Invalidation::new` refuses blank
    text, so `"   "` passes §9.4. Tighten to non-blank, or record why not.
  - Minor 5: §9.4 should say the records change no envelope field (MI-16), and put that obligation
    on the lineage fold that later reads them.
  - Minor 6: nine reason codes appear in no base or valid draft. Checks 5 and 10, which the oracle
    requires to fail by the document, are unrepresentable under the one stored mandate: add a second
    stored document (`pinned: true`, or `asset_classes: [crypto]`) and a base draft for each.
  - Nits: `test_a_hand_edited_research_vector_is_refused` asserts on the literal
    `allowlist_version: 7`; the PR-size note (ES-13, 1,199 hand-written lines, mostly fixture tables).
  - Two questions for a later mandate-spec change: whether §8.2's `conviction` and `confidence`
    ranges should be §8.5 refusals, and whether journal `text` gets a maximum length.
- **E17-3 (Must)** As an owner, I want instruments admitted into the working universe only through
  the eligibility floor, the policy's asset classes, `max_instruments`, instrument-group claims, and
  my autonomy rules (`new_instrument`, `thesis_confidence`; default `ask`), journaled as
  `UniverseChanged`, and removed to exits-only when a thesis is invalidated.
  *Accepted when:* simulation fuzzing over random theses never admits an ineligible instrument or
  exceeds the envelope; prompt-injection fixtures never reach an order.
- **E17-4 (Must)** As a fund, I want a bring-your-own-strategy mode that pins the universe and
  disables the research agent, so that today's behavior stays available.
- **E17-5 (Must)** As an owner, I want the input-drift detector (`unusual_input`, V-018) so that
  unusual inputs escalate before the research agent acts on them
  ([DEC-101](04-decision-log.md#decisions)).
- **E17-6 (Must)** As an operator, I want to see and stop research-agent flow across the accounts
  of a deployment, so that one thesis cannot concentrate orders from many accounts in one instrument
  unnoticed ([DEC-100](04-decision-log.md#decisions)).
  *Accepted when:* no workspace's gate reads another workspace's state (a two-workspace test shows
  one's positions never change the other's decisions); the aggregate-flow monitor sums research-agent
  exposure per instrument over its deployment's workspaces, in dollars and as a share of average
  daily dollar volume, alerts the operator above the thresholds, writes to no workspace, and sends
  nothing to the global control plane; the operator per-thesis halt, journaled in each workspace as
  a `PlatformOperatorAction`, stops matching research-agent admissions and openings in every
  workspace of the deployment while exits and protection continue, and never permits anything a
  workspace's own limits deny; openings on a new thesis wait for the workspace's deterministic
  stagger offset within the conduct controls; bring-your-own-strategy agents keep per-account
  controls only.
  *Follow-up (#415 review, blocker 2; DEC-293 item 8):* the gate-level two-workspace run —
  acceptance sentence 1, no workspace's gate reading another workspace's state — is stream G's
  under claim [#123](https://github.com/kunwarshivam/mandate/issues/123), not this story's;
  E17-6 covers the research-plane seam, where the monitor is the only cross-workspace view and a
  halt reaches a workspace only through its own resolved halt set.
- **E17-7 (Must)** As an owner, I want the research agent to read only vetted sources and to admit an
  instrument only on corroborated evidence, so that one planted source cannot admit an instrument
  ([DEC-101](04-decision-log.md#decisions)).
  *Accepted when:* the source allowlist is versioned configuration; the agent reads no source outside
  it; an admission without corroboration by an independent source, or by market data consistent with
  the thesis, is rejected; the corroboration is recorded in `ThesisProposed`; prompt-injection
  fixtures for every input source never reach an order.
- **E17-8 (Must)** As the founder, I want a forward paper evaluation harness, so that thesis quality
  is judged on outcomes the model cannot have seen ([DEC-99](04-decision-log.md#decisions)).
  *Accepted when:* the evaluation window, metric, and pass threshold come from a recorded decision
  made before the evaluation starts; it runs on the team's internal paper workspaces (DEC-103), so
  no user's results are aggregated; every thesis is scored after its horizon against buy-and-hold of
  the eligible basket and a broad index ETF, net of the cost model; the report states pass or fail
  against the threshold and is reproducible from the journal.
  *Follow-up (#410 review, minor 3; DEC-282 item 9):* an evaluation whose scoreable set is empty
  against a registered `minimum_scoreable` of zero refuses with `Num(DivisionByZero)` — fail-loud,
  but a code that tells a caller nothing. A tests PR names the refusal (a `ResearchError` arm of
  its own, `empty_scoreable_set`; the registry's codes are add-only) where the nameless code
  answered: the named code reaches a caller only at a registered `minimum_scoreable` of zero. Below
  a positive minimum an empty scoreable set still refuses `window_not_closed`, which does not tell
  "no theses at all" from "theses, none scoreable" (DEC-336); the frozen surface is otherwise
  unchanged. Tests merged in [#435](https://github.com/kunwarshivam/mandate/pull/435); the
  implementation PR (DEC-77 stage 2) replaces the stub with the named arm and deletes only the
  three `#[ignore]` lines.
  *Founder question (#435 review, minor 1):* whether the named refusal should answer an empty
  scoreable set at every minimum, ahead of the count refusal. That changes two pinned ES-09
  refusal codes (`a_thesis_whose_closes_all_lie_outside_its_window_is_unscoreable` and
  `a_degenerate_window_is_unscoreable_never_a_figure` pin `window_not_closed`, set by DEC-282
  item 8), so it is a surface change for the founder, not an agent reading; until then DEC-336's
  narrower reach holds.
  *Follow-up (#435 review, minor 3):* `score::basket_return` over an empty `members` slice refuses
  with the nameless `Num(DivisionByZero)`, pinned live by `an_empty_basket_is_an_error`: loud, so no
  figure escapes, but a code that tells a caller nothing. A tests PR names it the way DEC-335 named
  the empty scoreable set (an add-only `ResearchError` arm and code). Tests merged in [#455](https://github.com/kunwarshivam/mandate/pull/455) (DEC-380): `ResearchError::EmptyBasket`, code
  `empty_basket`; the implementation PR (DEC-77 stage 2) replaces the stub with the arm, deletes
  only the three `#[ignore]` lines, and adds #455's minors 1 and 2 as in-module tests.
  *Follow-up (#455 review, nit):* `no_basket_reaches_division_by_zero`'s `excess_by_size[0]` is
  `""`, and `r("")` panics; the `reported` branch never reaches a zero-member basket today, so
  it never fires, but a later generator change would make it a fixture panic. A tests change
  gives index 0 a real figure or removes it. *Done* on `agent/j3-e17-8-basket-nit`: the oracle
  now takes the expected excess from a closed match on the basket's size (`0.2` for one member,
  `0.15` for two). Any other size fails the case rather than defaulting a figure.
- **E17-9 (Should)** As an owner, I want the research agent to revise a thesis that failed on
  forward paper, with its autopsy recorded, so that the platform improves its ideas without hiding
  its failures ([DEC-111](04-decision-log.md#decisions)). *Accepted when:* a revision is journaled
  as `ThesisRevised` linked to its predecessor and names the failure it addresses; it starts with an
  empty scorecard and is scored only by the E17-8 evaluator; it passes the eligibility floor,
  corroboration, and the autonomy rules like a new thesis and cannot loosen any envelope field; past
  `max_revisions_per_lineage` the lineage is retired and the owner is told. Depends on E17-8 and
  on one completed DEC-99 evaluation on the DEC-103 thin slice. The lineage fold that reads `ThesisProposed` and
  `ThesisRevised` calls `mandate_spec::context::check_thesis_record` on each record before acting on
  it (DEC-414 item 3), as every reader of a thesis record must.
  When that first reader lands, the re-derivation moves up the trust ladder: a reader gets a thesis
  payload only through a type that carries `check_thesis_record`'s verdict, so none can act on one
  unchecked (#519 review). The same change completes `check_thesis_record`'s doc comment so it lists
  every `Err` it returns (#519 round 1, minor 3).

### E16 Kraken Derivatives US connector (Phase 3)

- **E16-1 (Should)** As an operator, I want to connect a Kraken Derivatives US account (demo and
  live) with a trading-only key so that agents can trade CFTC-regulated crypto perpetuals.
  *Accepted when:* keys with withdrawal or transfer permissions are rejected before storage.
- **E16-2 (Should)** As a trader, I want perpetuals accounting (funding every eight hours,
  margin, liquidation thresholds) so that perpetual P&L and risk are correct.
- **E16-3 (Should)** As an operator, I want a funding/carry signal model for perpetuals.

### E19 Agent harness (DEC-431)

Design: the [agent harness spec](../specs/agent-harness.md) v0.1 ([DEC-431](decisions/DEC-431.md)):
how a confirmed mandate version becomes a running agent process, and the research loop inside it.
It is the caller side of the [inference spec](../specs/inference.md) and assumes its INF-1 to INF-16.
These stories cover only what E6-1, E15-1, E15-3, E15-6 to E15-10, E17-2, E17-5, E17-7, and E17-8 do
not. **SC** marks a story on a safety-critical path (`AGENTS.md`): tests against approved reference
cases first, property tests for its invariants (spec §3), an independent review by an agent on a
different model, zero missed mutants, and green CI. No live model call runs outside the team's
internal paper workspaces (DEC-432 item 14). The founder set the internal paper phase's budgets on
2026-10-03: $5 per agent per risk day and $20 per internal workspace per day (DEC-431 item 15). No
research run starts before E19-5 and E15-8 land (spec §6.2 preconditions).

- **E19-1 (Must, M5; SC)** As an operator, I want an agent process built from a confirmed mandate
  version, so that it runs exactly what the owner confirmed. *Accepted when:* spec §5.1's eight steps
  are implemented with a test per failure row; the mandate's hash, the pinned models, and the
  research entry are re-checked before the writer is taken; a research-only failure disables
  research and leaves the agent running; a guard hit on the description disables research and sends
  an opaque owner alert; the process states and exits of spec §5.2 are covered by a state-machine
  property test, including a crash in every state and a kill switch arriving in every state the
  process is up (HI-20); HI-12 holds with the vault and provider keys absent from the environment.
- **E19-2 (Must, M5)** As an owner, I want the research agent to read only what its pinned retrieval
  plan names, so that what the model is shown is fixed and bounded. *Accepted when:* spec §6.3's
  readers run in `mandate-research-run` (layer 5, DEC-431 item 20) against a data-plane double at a
  cut-off; the core journals every item as `ObservationRecorded` before it emits the call effect
  (HI-2, data-plane spec §4.6); every cap is enforced, oldest items dropped first, and the drop
  recorded; a test shows every data port only reads (HI-4); every read filters on knowledge time
  (HI-13, DP-1); E17-5's `DriftState` folds the typed per-item observation the shell builds, by knowledge time, with each item observed once, never text (spec §6.3); a licensed item's text is
  kept only when a thesis cites it (HI-23, spec §6.6); the retrieval plan is inside the research
  entry's content hash (DEC-431 item 5), with the matching inference spec revision first.
- **E19-3 (Must, M5; SC)** As an owner, I want research runs scheduled, gated, and cancelled by the
  runtime core, so that research never blocks or outlives the trading loop. *Accepted when:* the
  core's new start, cancel, and result inputs and effects (spec §10.3) are added tests first
  (DEC-77); spec §5.3 and §5.4's run states and boundaries are property-tested over random schedules,
  mode changes, version changes, crashes, and midnight; HI-7 (research latency never changes the
  non-research drafts), HI-8, HI-15, HI-16, and HI-17 hold, each oracle shown to catch a seeded bug;
  the schedule counts from the later of the last call and the last reservation; the call's deadline
  is measured from its dispatch (DEC-431 item 24); `mandate-research-run` gets its `xtask/layers.toml`
  entry, and an xtask check that it depends on no journal crate (HI-16), in the change that creates
  it; no run starts before spec §6.2's preconditions hold.
- **E19-4 (Must, M5; SC)** As an owner, I want every model output checked and its facts filled by the
  platform before admission, so that a model cannot decide an admission check for itself.
  *Accepted when:* spec §6.5's field split and six harness checks are implemented with a case per
  check; the judging batch journals the candidates' verdicts, the thesis records (journal spec
  §9.4), and `ModelOutputRecorded` all-or-nothing; HI-2, HI-3 (replay with a gateway that fails the
  test on any call), HI-6, HI-18, and HI-22 hold against a compromised model.
- **E19-5 (Must, M5, before the first research run; SC; journal spec first)** As an owner, I want
  an invalidated thesis to remove its instrument at once, so that a position whose reason is gone is
  exited. *Accepted when:* a journal spec change adds the agent-stream invalidation verdict and the
  executor's `UniverseChanged` (`thesis_invalidated`) copy from it, with vectors, covering DEC-433
  item 19's revoked source too; `exits_only` runs are review-only; an invalidation never admits,
  never adds risk, and is journaled before the removal; MI-19 and HI-19 hold under a fuzz of random
  invalidations and renewals. Until it lands no research run starts (DEC-431 items 13 and 23).
- **E19-6 (Must, M5)** As the founder, I want an adversarial bench for the research loop, so that a
  fully compromised model is shown to breach nothing. *Accepted when:* injection fixtures exist for
  every reader and source (news, filings, screens, the description, memory); with a compromised
  model, zero limit breaches and zero orders without a dry-run allow (HI-10); the canary scan of HI-11
  finds nothing in any prompt artifact. It is the v1 subset of E18-7 and feeds E17-3's and E17-7's
  injection clauses.
- **E19-7 (Should, M5)** As the founder, I want regression evaluations for research entries, so that a
  template, plan, or model change is checked for mechanics before it is offered. *Accepted when:* spec
  §8.3's measures run on a fixed set of recorded runs in the internal paper workspaces; the report
  states each pass condition; it is never shown to owners and never stands in for E17-8 (DEC-99).
- **E19-8 (Must before live trading; SC)** As an owner, I want my agent's kill switch to work while
  its process is down, so that rule 13 holds without the runtime. *Accepted when:* spec §13 item 1 is
  decided in a DEC (the executor flattens from the control stream after a deadline, or the
  deployment manager guarantees a restart that handles the kill switch first); a fault-injection test
  kills the agent process, pulls the agent kill switch, and shows the agent-scoped flatten completes
  with no cancel-all or close-position (trading spec §5.5).
- **E19-9 (Should)** As a maintainer, I want `mandate-research`'s doc comments to say that the runtime
  core appends thesis records and the executor copies `UniverseChanged`, so that the next agent copies
  the single-writer rule (DEC-431 item 2). *Accepted when:* the comments on the crate and on
  `ResearchEvent` match journal spec §2, with no code change.
- **E19-10 (Must before any fast-tier model runs; SC)** As an owner, I want the hosted fast tier's
  call kept off the trading tick, so that no exit waits on a model provider (DEC-432 item 13; DEC-431
  item 18, Proposed, whose conservative reading is in force). *Accepted when:* a fast worker calls
  the gateway off the tick and the output enters the core as `Input::ModelOutput` after its record
  commits; a missing or late output is missing (rule 3, MI-10); HI-21 holds with a gateway double
  that stalls for ever: the decide step stays within ES-24's budget and every exit, protective-order,
  and kill-switch draft is unchanged. Beside E15-2, which defines the model.
- **E19-11 (Must, M6, before the first paper trade; SC)** As the founder, I want a paper
  deployment's input built from the confirmed mandate version on the control stream, so that the
  first trade runs what I confirmed and nothing a file says
  ([first paper trade brief](tasks/first-paper-trade.md), [DEC-505](decisions/DEC-505.md)). The
  subset of E19-1 the first trade needs: spec §5.1 steps 1, 2, 4 and 6, with no process states.
  *Accepted when:* the input comes only from the latest `AgentDeployed` with no `AgentStopped`
  after it, a stored document that re-hashes to its version, that version's
  `MandateVersionCreated` and `MandateConfirmed`, and the registered configuration objects, folded
  by `ValidationContext::from_journal` with the registry present; the effective registration of
  each kind is the latest by `seq`, narrowed by the pinned asset id and model triple, and a fee
  schedule not yet effective refuses (DEC-505 item 1), each with a test; a test per failure (no
  deployment, a stopped agent, a document missing or not re-hashing, an unconfirmed path, an
  unregistered or mismatched model, a configuration object that does not re-hash, a `live`
  mandate) refuses before any credential is read; the account equity is the run's broker read and
  the connection's environment is `paper` only when the stream holds no fact about the connection,
  so a `ConnectionRevoked` still refuses (DEC-505 item 3); and the paper path reads no mandate or
  configuration file.
- **E19-12 (Should, after the research agent's DEC-99 evaluation passes and E19-3; SC)** As an
  owner, I want a market event (news, a filing, a price move, an earnings date or call transcript)
  to bring my agent's next research run forward, so that its theses respond to what happened rather
  than waiting for the interval ([DEC-528](decisions/DEC-528.md) item 4; PRD FR-11.2).
  *Depends on:*
  - agent harness spec §5.3's change (protected; not written yet): an event edge from `Idle` to
    `Due` with its debounce, and an amendment of invariant HI-8 (no run starts before
    `next_proposal_at`) so that a trigger never bypasses the run's preconditions, the mode, or
    E19-3's caps;
  - a mandate spec change: the event sources, debounce and caps under `behavior.cadence`;
  - a source evaluation for an earnings calendar and transcripts. A paid vendor is spending, which
    the founder decides (DEC-79), and any new source joins the allowlist (DEC-101).

  *Accepted when:*
  - event sources, debounce and caps are envelope fields under `behavior.cadence` (mandate spec);
    no event triggers a run unless a confirmed mandate version enables it (rule 11), and a change
    that adds event sources classifies as increasing;
  - triggers are debounced, and rate- and cost-capped per agent;
  - only allowlisted sources can trigger, and a triggered run's admissions still need DEC-101's
    corroboration;
  - prompt-injection fixtures in every triggering source never reach an order (E17-7);
  - a burst of events never starts more runs than the caps allow, tested over random event
    sequences;
  - a run still produces opinions only (rule 4) and never sizes or places an order.

*Follow-ups (#554 review round 1, minors, deferred by the freeze rule):*

- Minor 1: say which clock each research field uses. The cut-off comes from the risk clock, which is
  whole-second and can lag; the call's `deadline` is wall-clock from dispatch (the M1 fix covers the
  deadline; spec §6.2 step 1 still needs the clock named for the cut-off).
- Minor 2: a refused call (`policy_denied`, `input_rejected`, `credential_invalid`,
  `rate_limited_local`, `model_withdrawn`) leaves no reservation, so a crash before its record
  commits lets the next start call again at once. Say a refusal advances the schedule, or journal the
  due-time advance before the call effect (spec §5.3).
- Minor 3: `mandate_research::next_proposal_at`'s parameter is `last_proposal` and its doc comment
  says "propose"; the spec reads it as the last call. Add that doc comment to E19-9's scope.
- Minor 4: HI-3 is circular as stated; restate it as its test does (replaying through the core with a
  failing gateway re-derives identical drafts and verdicts).
- Minor 5: HI-7's carve-out nearly vacates it; restate over instruments with no research output in
  the window, plus ES-24's bound on tick latency.
- Minor 6: spec §6.5's closing list of §8.5 checks omits checks 4, 6, 8, 10, 11, and 13; say "every
  §8.5 check, in order".
- Minor 7: qualify bare cross-spec section numbers (HI-10's "§6.2 step 5" is the mandate spec's;
  likewise the `exits_only` row and other bare "§8.5"/"§8.3").
- Minor 8: the drop rule ("oldest items go first") is not deterministic; order by knowledge time,
  then item id.
- Minor 9: an `exits_only` run spends and, before E19-5, can do nothing; say no run starts in
  `exits_only` until E19-5 lands (the round-1 preconditions now hold every run until then).
- Minor 10: construction step 5 needs the gateway's guard callable without a call (spec §10.1 ask 4);
  state the interim: research disabled until it is.
- Minor 11: post-merge staleness: `DEC-431.md` item 14's "(DEC-434, in review)" and item 17's "data-plane
  spec, in draft"; and E21 is missing from the epic overview table. The spec's own references to the
  data-plane spec were updated with B2.

*Follow-ups (#554 review round 2, minors, deferred by the freeze rule):*

- Round-2 minor 1: §6.2 precondition 3 allows "market data and public filings" while §6.3's news row
  says only "licensed news waits". Say whether an allowlisted news source whose terms permit full
  retention may be read before §10.1 ask 5 lands.
- Round-2 minor 2: §6.2 step 1 does not check the preconditions. Put the check there and on §5.3's
  `Due --> Idle` edge, which is what HI-19's and HI-22's "a start attempted before the record exists
  emits no call" tests.
- Round-2 minor 3: HI-15 bars calls only in `paused` and `stopped`, while §5.2 and DEC-431 item 9 say
  Holding makes no call, and Holding's effective mode is `exits_only`. Name Holding in HI-15.
- Round-2 minor 4: decide `mandate-research-run`'s `xtask/layers.toml` entry now: `pure = true` (layer
  5 holds only pure crates, and the core names its result types, so its ports pull in no
  `impure_crates`) and `safety_critical = true`, with the lint header, a CODEOWNERS line, and mutants
  on its diff. DEC-431 item 20 says "if safety-critical" without deciding.
- Round-2 minor 5: DEC-431 item 17 cites "journal spec §6.3, six years" for retention; retention is
  §6.2.
- Round-2 cross-document: data-plane spec §4.6 said the drift detector folds `ObservationRecorded`;
  corrected in data-plane spec v0.2 to the typed per-item observation the observation's artifact
  holds (agent harness spec §6.3). Still open: add `mandate-research`'s drift doc comment ("the shell
  records") to E19-9's scope with the same reading.

*Follow-up (#690 review, the E15-13 slice R0 tests PR; for E19-1):*

- **A journal `Invalid` wedges the runtime until restart.** `handle` remembers every batch it
  emits as an `UnresolvedAppend` before the append answers (`mandate-runtime` `step.rs`
  `remember`), and while one is remembered it answers `AppendUnresolved` to every other input,
  commands and the kill switch included. That is right for an append in doubt (journal spec §5.1),
  but an append the journal refuses as `Invalid` is not in doubt: nothing committed, so retrying the
  same input gets the same refusal, and the runtime refuses everything else until a restart folds
  the journal again. Any `AgentModeChanged` or `ApprovalCanceled` drafts in that batch are dropped
  with it. Nothing reaches the journal, so rule 5 holds, but a runtime that refuses a kill switch is
  a rule 13 problem. R0 closes the one path it found by refusing an empty `source` in the step; the
  general rule belongs to E19-1's process states (agent harness spec §5.2): say what the process does
  when an append answers `Invalid` (exit to a restart, or a typed input that clears the remembered
  batch), with a test that a kill switch after an `Invalid` append is honoured.

### E20 Global control plane (proposed, DEC-440)

From the [control-plane design](../design/control-plane.md) (v0.1 draft). The epic joins the
overview when the founder accepts it; until then each story is **(Proposed)** with the milestone it
would serve. **SC** marks a story on a safety-critical path. License terms, vendors, and hosting stay
with the founder (DEC-440 items 13 to 17); no story here buys a service or touches live money.

- **E20-1 (Proposed, M8)** As an org admin, I want a directory of organizations, workspaces,
  deployments, members, and role names, holding no personal data, so that seats and routing work
  without the control plane learning who anyone is (design §3.2).
  *Accepted when:* a schema test shows no table or message field for email, name, or IdP subject;
  a client with a cached route reaches its deployment with the directory down.
- **E20-2 (Proposed, M8; SC)** As an IT admin, I want to enroll a deployment with a single-use
  token, a key generated on site, and a client certificate bound to its `deployment_id` (design §3.1,
  E13-1).
  *Accepted when:* a reused or expired token is refused; the private key never leaves the site's
  vault; revoking the certificate behaves exactly as an outage in the CP-2 drill.
- **E20-3 (Proposed, M8; SC; journal spec change first)** As an engineer, I want the closed message
  set of design §3.8 and its journal events (`ControlPlaneEnrolled`, `LicenseApplied`,
  `LicenseStateChanged`, `ReleaseOffered`, `ReleaseInstalled`, `ReleaseWithdrawnNoticed`,
  `CatalogEntryRegistered` (kind, content hash, sequence), `DataBundleImported`, `UsageReportSealed`,
  `ControlPlaneMessageRefused`) registered in the journal
  spec with vectors, then implemented: verification in `cp-agent`, appends by workspace control
  services, the control stream's single writer.
  *Accepted when:* CP-1's type test and canary scan pass; CP-4's layering check and fuzz test pass;
  CP-5's per-type tests show an unsigned, wrongly signed, replayed, or out-of-list instruction
  refused and journaled, and a valid one journaled before its effect; CP-9's replay test passes for
  all four kinds (license, release manifest, catalog entry, data bundle); `cp-agent` reaches the site
  only through a port it declares and workspace services implement; a test shows `cp-agent` never
  takes a writer epoch, and the kill switch commits with `cp-agent` hung.
- **E20-4 (Proposed, M8; SC; mandate spec change first)** As an org owner, I want licenses
  verified on site, with states `valid`, `renewal_due`, `grace`, and `lapsed`, where after grace a
  lapsed license refuses only new deployments and new openings; it never blocks an exit, a
  protective order, the kill switch, or any risk reduction (design §3.3; DEC-440 item 13 Proposed).
  *Accepted when:* mandate spec §5.9 lists `license_lapsed` (mode `exits_only`) with its reference
  cases before code; CP-6's test passes (expiry mid-session leaves positions and protection
  untouched; exits and the kill switch pass; after grace every covered agent journals
  `AgentModeApplied` into `exits_only` and an opening is refused by the existing mode check, with no
  new gate check); CP-9's replay test passes; lowering an entitlement stops no running agent.
- **E20-5 (Proposed, M8)** As an operator, I want heartbeats carrying versions and health only, and
  a fleet view, where missing heartbeats only mark a site `unreachable` and alert (design §3.4,
  CP-8).
  *Accepted when:* a site silent for a day is shown `unreachable` and nothing about it is revoked.
- **E20-6 (Proposed, M11; SC)** As an IT admin, I want signed release manifests offered over the
  outbound link and installed only by my action or in my update window, by drain and hand-over
  (design §3.4; DEC-434 item 7; with E21-8 signing).
  *Accepted when:* a manifest not signed by the pinned release key is refused; the upgrade drill
  (OPS-7) passes on a hybrid site; a withdrawal refuses new installs and changes no running process.
- **E20-7 (Proposed, M12)** As an org owner, I want usage metered from signed, chained hourly
  reports built from journaled facts and the inference meter (design §3.5).
  *Accepted when:* CP-10's test passes (random cuts and duplicates bill the same totals; a gap is
  flagged, never estimated); the canary scan finds no instrument or agent name in any report.
- **E20-8 (Proposed, M10; SC)** As an approver on a hybrid site, I want the relay to forward
  encrypted web push, keep nothing after the attempt, and log opaque IDs only (notifications spec
  §4.6, E8-14).
  *Accepted when:* a payload over 512 bytes is refused; captured relay storage and logs after a
  test day hold no ciphertext and no canary string; the exit suites pass with the relay down.
- **E20-9 (Proposed, M11)** As an IT admin, I want releases, connector packages, and model-registry
  entries served by digest and verified on site against per-kind keys (design §3.6).
  *Accepted when:* a bundle with a wrong digest or key is refused before any byte is used; a new
  registry entry never changes an existing pin.
- **E20-10 (Proposed, M11)** As an auditor, I want anchor roots received and receipted by the
  control plane, so a restore can be compared with a copy held outside the site (journal spec §10;
  infrastructure §6.3).
  *Accepted when:* roots queued during an outage arrive in order; the restore drill compares against
  the witness copy and flags a restored head behind a witnessed root.
- **E20-11 (Proposed, M8; SC)** As the founder, I want the outage drill of design §4: the outbound
  link cut for a simulated week, a half-open partition, and a revoked certificate (CP-2, CP-3, CP-7).
  *Accepted when:* paper and kill-switch suites give the same outcomes as with the link up, except
  relay push and queued reports; a connection attempt from the control-plane network into the site
  fails at the network layer.
- **E20-12 (Proposed, M8; SC)** As the founder, I want the managed global kill switch issuable only
  from each cell's operator tooling with step-up, never from the global control plane (design §3.7;
  DEC-440 item 8; members per DEC-261 item 9).
  *Accepted when:* the control plane has no message type that maps to `PlatformOperatorAction`; a
  hybrid site refuses one from outside its own operators.
- **E20-13 (Proposed, M8)** As an org owner, I want one rule for the license clock: validity is
  checked against the highest UTC time the site has journaled (#562 review minor 1).
  *Accepted when:* setting the site clock back never extends a license; a frozen clock still raises
  `renewal_due` from the journaled high-water time, and the design says so.
- **E20-14 (Proposed, M8)** As an IT admin, I want the enrollment certificate's lifetime and renewal
  threshold named, so CP-8 is checkable (#562 review minor 4).
  *Accepted when:* the design states both; a test renews at the threshold; the CP-2 drill outlasts
  the threshold with only an alert.
- **E20-15 (Proposed, M8)** As the founder, I want the control plane's recovery targets stated, as a
  Proposed item or as a reading under DEC-440 item 16 (#562 review minor 6).
  *Accepted when:* design §5.1 and §8 agree on where the targets live and what they are.

### E21 Operations and infrastructure (proposed, DEC-434)

From the [infrastructure design](../design/infrastructure.md) (v0.2 draft). The epic joins the
overview when the founder accepts it; until then each story is **(Proposed)** with the milestone it
would serve. **SC** marks a story on a safety-critical path, to which the `AGENTS.md`
safety-critical rules apply. Hosting, vendor, and budget choices stay with the founder (DEC-434
items 13 to 20); no story here buys a service or touches live money.

- **E21-1 (Proposed, M6; SC)** As the founder, I want the paper/live boundary held at the network
  as well as in the build, so that no non-production environment can reach live money (OPS-5).
  *Accepted when:* each non-production environment's egress allow-list is default-deny per process
  type (design §3.1, §9); a test in the paper environment shows a request to each live trading host
  fails at the network layer; and an agent runtime has no route to any broker host.
- **E21-2 (Proposed, M6; SC)** As the founder, I want the Phase 1 paper environment run as
  supervised processes (runtime per agent, executor per account, scheduler) with restart policy,
  liveness, readiness, and a crash-loop bound, so that the soak runs unattended (design §3.4).
  *Accepted when:* killing any process at any step recovers it from the journal with zero
  duplicates; a process that is not ready takes no opening but still runs the kill switch; a
  crash-looping agent is left `Paused` with an alert; and two copies of one executor leave the
  older one `Fenced` before it sends.
- **E21-3 (Proposed, M6)** As an operator, I want metrics through the OpenTelemetry API with a
  Prometheus pull exporter (DEC-73, ES-18), with only opaque labels, so that I can watch the system
  without leaking strategy. *Accepted when:* the exporter runs air-gapped; a lint fails any label
  outside the allowed set (design §8.1); and a test shows the decision cycle unchanged with the
  exporter failing (OPS-11).
- **E21-4 (Proposed, M7)** As an operator, I want the safety alerts of design §8.2 raised from
  journal events and metrics with opaque payloads, so that every FR-8.3 condition reaches someone.
  *Accepted when:* each alert in the table fires in a fault-injection or fixture test, and a
  payload capture finds no symbol, price, quantity, or mandate content (OPS-10).
- **E21-5 (Proposed, M7; SC)** As the founder, I want the journal backed up by WAL archiving and
  base backups, and a restore procedure that verifies before anything trades, so that a lost
  database costs no record silently (design §6). *Accepted when:* a monthly drill restores the
  paper journal, passes journal spec §11 over every stream, compares heads with the latest anchors
  and cold manifests, and journals the result; a restore older than the last anchor takes the
  integrity-incident path and no agent resumes (OPS-8); and a canary scan finds no secret in the
  restored data. Blocked on E21-25, which defines the events the result is journaled as.
- **E21-6 (Proposed, M13)** As on-call, I want runbooks RB-01 to RB-18 (design §8.4), so that the
  Phase 2 gate's "runbooks exist for every alert in FR-8.3" holds. *Accepted when:* each runbook
  names its alert, its checks, its safe actions, and its exit, and is exercised once in staging.
- **E21-7 (Proposed, M8; SC)** As an owner, I want agents upgraded by drain and hand-over, so that
  an upgrade never drops protection (OPS-7, design §7.3). *Accepted when:* an upgrade drill with
  open positions, an exit sequence in flight, pending approvals, and a kill switch issued
  mid-hand-over shows no protective order canceled by the deploy, every unprotected interval within
  `max_unprotected_s`, the kill switch applied by the new process, and zero duplicates.
- **E21-8 (Proposed, M6 then M11)** As the founder, I want release builds from `main` that are
  reproducible, carry an SBOM, and from M11 are signed with my hardware key and checked at start
  (ES-14, ES-17, OPS-15). *Accepted when:* two builds of one commit are byte-identical; each process
  journals its build digest; and outside dev and CI an unsigned binary refuses to start.
- **E21-9 (Proposed, M8; SC)** As an owner, I want my broker credential held in the workspace vault,
  leased only to the executor for my connection, and checked for scope, environment, and account at
  every start, so that no other process can use it (design §5). *Accepted when:* a runtime's
  identity cannot read any credential; an executor's can read exactly one; a credential with
  withdrawal or transfer permission, the wrong environment, or the wrong account is refused and the
  refusal journaled without the credential; and a vault outage stops no running executor before its
  lease expires. Blocked on DEC-434 item 14.
- **E21-10 (Proposed, M8)** As the founder, I want a written threat model per process type and
  deployment mode (design §9), so that the M13 penetration test has a scope. *Accepted when:* it
  covers the attackers `AGENTS.md` names plus a compromised dependency, operator laptop, tenant, and
  backup, and each threat is blocked, detected, or disclosed.
- **E21-11 (Proposed, M8; SC)** As an operator, I want the live journal on a synchronous standby
  with fenced failover, drilled under load, so that a database failure loses no acknowledged append
  (OPS-2, design §4.1). *Accepted when:* a staging drill fails the primary during intents in flight
  with zero duplicates and zero lost acknowledged appends, and with no synchronous standby, appends
  return `Unavailable` rather than commit asynchronously. Blocked on DEC-434 items 13, 15, and 19.
- **E21-12 (Proposed, M12)** As the founder, I want usage counted per agent and per workspace
  (agent-hours, events, artifacts, model tokens) against the cost model's variables (design §10),
  so that budgets and pricing rest on measured numbers. *Accepted when:* counts reach metering
  without content, and a workspace's monthly counts reproduce from its journal.
- **E21-24 (Proposed, M8)** As the founder, I want the cost model (`docs/product/12-cost-model.md`, DEC-443) kept current. *Accepted when:* `T_in` and `T_out` are measured from the research spike's call records and replace the assumptions; `active_seconds_per_day` is one explicit row per use (research around the clock, the equities session); §2.4's storage notation matches §3.2 and the hot-store term is carried or shown to be negligible; scenario C's storage row names its year; DEC-443 item 7's lean-workspace design moves to an infrastructure row, leaving only the pricing dependency with the founder; and a staleness check (vendor price or pinned model changed) is considered for `cargo xtask`. From #566's round-1 review, minors 1, 2, 3, 7 and 8 (freeze rule). Numbered after #557's E21-13 to E21-23.

Rows E21-13 to E21-22 come from the [threat model](../security/threat-model.md) v0.1
([DEC-439](decisions/DEC-439.md)); E21-10 is that document. Rows blocked on a Proposed DEC-439 item
wait for the founder.

- **E21-13 (Proposed, now)** As the founder, I want the coordinator to act through its own GitHub
  identity, the only login in `MERGE_APPROVERS`, so that a builder session cannot approve its own
  PR (threat model §7.5). *Accepted when:* a label or approved-head line set by a builder's identity
  leaves the PR unmerged, shown by a dry run of `merge-approved.sh`; and the coordinator's identity
  holds no push access to agent branches. Blocked on DEC-439 item 9.
- **E21-14 (Proposed, now)** As the founder, I want `merge-approved.sh` to refuse a PR that touches
  the self-protecting paths (DEC-439 items 7 and 12) unless the founder approved its current head,
  so that a merged change cannot weaken the checks that guard every other change. *Accepted when:*
  dry runs show such a PR skipped without the founder's approving review on its head, skipped again
  after a push moves the head, and merged with it; and ES-13's text matches what is enforced.
  Blocked on DEC-439 item 12.
- **E21-15 (Proposed, now)** As the coordinator, I want the untrusted-author rule (DEC-439 items 2
  and 3) in the coordination and review playbooks, so that public text never steers an agent
  session. *Accepted when:* the playbooks say it; the monitor acts only on coordination lines from
  accounts with write access; review briefs are built from the story, specs, and diff; and a planted
  comment from an outside account, tried once in a test PR, is ignored.
- **E21-16 (Proposed, now)** As the founder, I want merging on `main` restricted to the merge
  workflow and the founder, the ruleset's admin bypass limited to pull requests or removed, and
  collaborators at Triage with fork-based PRs, so that neither write access nor the account agents
  use can change `main` outside the PR path. *Accepted when:* a collaborator account's attempt to
  merge a green PR is refused by GitHub; the ruleset shows no bypass with mode "always"; a direct
  push to `main` from the account agents use is refused; and `COLLABORATION.md` describes the fork
  flow. Blocked on DEC-439 items 18, 10, and 11.
- **E21-17 (Proposed, M8)** As the founder, I want the aggregator's exposure bounded (DEC-432 item
  14, DEC-439 items 6 and 14), so that one third party in every prompt path costs as little as it
  can. *Accepted when:* each environment has its own key with a provider-side spend limit; users'
  agents never share a key with development sessions; routing requests zero retention where offered
  and the terms are on file per endpoint; and a canary probe per pinned model runs on a schedule and
  alerts on drift. Blocked on DEC-439 item 14.
- **E21-18 (Proposed, now)** As the founder, I want each development lane to hold its own capped
  paper and model keys, so that a steered session can spend little and leak only what one rotation
  fixes (DEC-439 item 17). *Accepted when:* no two lanes share a model key; each key has a
  provider-side cap; and a rotation runbook names the trigger (any suspected injection). Blocked on
  DEC-439 item 17.
- **E21-19 (Proposed, M8)** As the founder, I want the `web/` npm tree held to the same bar as the
  Rust tree, so that a compromised package cannot reach owner sessions (threat model §7.6).
  *Accepted when:* a check fails a top-level package in `web/package.json` without a row in
  `docs/dependencies.md`; CI installs with lifecycle scripts disabled except for named packages;
  and registry signatures are verified in CI.
- **E21-20 (Proposed, M8)** As the founder, I want the threat model cross-linked to the identity,
  notifications, and workspace API specs by invariant number, and their out-of-band notices in
  place (DEC-439 items 4 and 5), so that each gap has one owner. *Accepted when:* every gap in
  threat model §6.1 to §6.3 names an `ID-`, `NT-`, or `API-` invariant or a backlog row; the
  notifications spec lists the risk-increasing events of DEC-439 item 5; and who approves a
  source-allowlist change is written down.
- **E21-21 (Proposed, M10; SC)** As an approver, I want model text in an approval card shown as
  quoted, plain, length-capped text with only platform-resolved source links, so that text written
  to persuade me is visibly the research agent's (threat model §6.2). *Accepted when:* a fixture
  thesis containing urgent instructions, markup, and links renders as inert quoted text, with no
  link that the platform did not resolve from an allowlisted source.
- **E21-22 (Proposed, M13)** As the founder, I want an external penetration test and a research-path
  red-team scoped by the threat model, so that the Phase 2 gate's test has a defined target.
  *Accepted when:* the scope lists every boundary of threat model §4.3 in the deployed modes; findings
  of high severity are fixed and retested before the first design partner; and §8 is re-ranked from
  the results. Blocked on DEC-439 item 13.
- **E21-23 (Proposed, now)** As the founder, I want the threat model's round-1 minor findings (#557
  review, freeze rule) applied in its next version, so that the register stays exact. *Accepted
  when:* §6.12's Merge-button row says `web` is not a required status check, so a hand merge of a
  `web/` PR also skips the web checks; §7.5 adds that ADR-0001 ES-13's "signs merges with a
  hardware-backed key" is not what the squash-merge path does; the §4.2 diagram has an edge from
  public and agent-written text into the reviewer; rank 12 (an injected owner-connected agent
  flattens the book) is re-rated M/M; the aggregator residual says prompts also reveal timing;
  §6.11 carries break-glass without the customer as a gap until #556's ruling lands; §6.1's
  email-link gap moves to the control column citing `identity.md` §6.1 and §7.3, and its E row
  names `identity.md` (#556) as the authoritative role matrix (settlement X1); and every control row
  in §6 is marked built or specified.

Rows E21-25 to E21-27 come from the post-merge review of the infrastructure design (#552) and its
v0.2 fixes ([DEC-434](decisions/DEC-434.md) items 21 to 24).

- **E21-25 (Proposed, M7; SC; journal spec first)** As an auditor, I want backup runs and restore,
  failover, and evacuation drills recorded as journal events, so that OPS-8's "journaled" has
  somewhere to go (design §6.4, DEC-434 item 24). *Accepted when:* a journal spec change adds the
  backup and drill events to §9's catalogue on the control stream, each naming what was restored or
  exercised, the `VerificationRun` it relied on, and pass or fail, with test vectors; the
  registration lands tests first (DEC-77); and an unregistered drill event is still rejected at
  append. Blocks E21-5.
- **E21-26 (Proposed, M6; SC)** As an owner, I want the journal-outage hold tested for exactly what
  the design discloses, so that OPS-4's test is honest (design §2.1). *Accepted when:* with the
  journal unavailable and a risk exit owed, a fault-injection test shows no order of any kind is
  sent, no resting protective order is canceled, the hold raises its alert, and the owed exit is
  the first order sent once appends succeed. What the executor may do beyond that waits for the
  founder (DEC-434 item 21); the test changes with that decision.
- **E21-27 (Proposed, M8)** As the founder, I want the minor findings of the #552 review applied in
  the design's next version (freeze rule), so that the document stays exact. *Accepted when:* §8.2
  or §8.4 says RB-17 and RB-18 are procedures reached from the failure walk, not alert
  destinations; §4.1 or §6.1 says the Phase 1 paper recovery point can lose committed order
  intents up to the WAL archive's lag, that this is accepted for paper, and that a restore then
  treats those orders as external activity; DEC-434 or the design says who decides the engineering
  defaults marked "(Proposed)" in prose (the crash-loop bound, the drain bound, the vault lease
  length, two-person break-glass, the retention rows of §4.5); §12's process row names the open
  executor implementation (#174); and §14 and DEC-434 item 12 say once whether E21 is in the epic
  overview.

## Won't (v1)

Live retail trading before counsel signs off; users outside the US; options; Interactive Brokers and Coinbase connectors; native mobile apps; WebAssembly plug-ins; SAML and SCIM;
fully on-prem control plane; shared data plane; strategy marketplace.

## Later (wanted after v1)

- **Custom signals for power users** ([DEC-20](04-decision-log.md#decisions)). Today a mandate chooses the
  platform's registered signal models, their declared parameters, fixed weights, thresholds, universe,
  autonomy, protection and cadence, but it cannot define a new signal formula, combine signals other than
  by fixed linear weights, or change sizing. Two steps, in order:
  1. A **declarative signal-expression layer**: arithmetic and comparisons over the platform's indicators
     and data, stored in the mandate, validated, bounded, and replayed deterministically. It keeps the
     mandate checkable.
  2. **WebAssembly plug-ins**, only if step 1 falls short: sandboxed, resource-limited, versioned by content
     hash, with every output journaled. A plug-in emits only an opinion (a signal or a thesis), which enters
     through the eligibility floor, the autonomy rules and the risk gate like the platform's own ideas. It
     never sizes or places an order (rule 4).

- **Agents over holdings the owner already has (adoption)** ([DEC-528](decisions/DEC-528.md) item 3;
  PRD FR-11.3). Deferred: [DEC-46](04-decision-log.md#decisions) stands, and an agent's sub-ledger
  holds only what it bought. Prerequisites, each its own story when it is written:
  1. Importing lots and their cost basis from the broker, with each lot's source recorded.
  2. Wash-sale handling across adopted lots and the agent's own trades (PRD FR-5.11 is
     informational only today).
  3. An answer for how an adopted position meets
     [trading-domain spec §7.1](../specs/trading-domain.md#71-account-ledger-dec-26) (activity the
     platform did not originate is external activity, which puts the account's agents in
     `exits_only`) and [connections spec](../specs/connections.md) CN-7 (never adopted as ours).
     Both are protected spec changes.

## Enterprise harness (proposed, DEC-149)

Epic **E18**, from the [harness engineering research](../product/11-harness-engineering.md#7-recommendations)
of 2026-09-27. [DEC-149](04-decision-log.md#decisions) makes the harness (gate, autonomy rules,
journal, executor, connectors, conformance suite, and MCP channel) an enterprise product that the
retail platform runs through. Every story here is **(Proposed, not scheduled)**: none is in the epic
overview, none is assigned a milestone, and v1's milestones do not change. **SC** marks a story that
touches a safety-critical path (gate, autonomy, journal, executor, connectors, credentials, auth, or
tenant isolation): the `AGENTS.md` safety-critical rules apply to it (tests written or verified
against approved reference cases first, property tests for its invariants, an independent review by
an agent on a different model, zero missed mutants, and green CI).

- **E18-1 (Proposed, not scheduled; SC)** As a broker or fintech, I want a versioned tenant policy
  overlay on my customers' mandates, with inline tests, so that my house rules apply to every agent.
  *Accepted when:* the overlay can only tighten a mandate, never loosen it (as E9-3's org limits
  do), and property tests prove that for any mandate and overlay the effective limits are at least
  as strict as both.
- **E18-2 (Proposed, not scheduled; SC)** As an enterprise SRE, I want OpenTelemetry GenAI spans
  derived from the journal so that agent runs show in my tracing stack. *Accepted when:* spans are
  derived from journaled events only, content attributes are off by default (no order details,
  positions, or mandate content leave the deployment unless the customer turns them on, rule 6 and
  "Do not"), and the semantic-conventions version is pinned.
- **E18-3 (Proposed, not scheduled; SC)** As a compliance officer, I want SIEM export and a signed
  per-period evidence pack so that I can file the period's record. *Accepted when:* the pack holds
  the chain segment, anchor proofs, mandate versions, gate verdicts, and reconciliation results,
  verifies offline with `journal verify`, and exports in at least one documented SIEM format.
- **E18-4 (Proposed, not scheduled; SC)** As an agent builder, I want the MCP channel to be a policy
  enforcement point so that my agent can act only through Mandate's rules. *Accepted when:* it
  accepts only tokens issued to Mandate and never passes a token through to a broker or another
  service; its tools are intent-shaped (`propose_intent`, `explain_verdict`), never a raw
  `place_order`; scopes start read-only and elevate incrementally; and tool schemas are pinned, failing
  closed on a change. Builds on E10-6.
- **E18-5 (Proposed, not scheduled; SC)** As an owner, I want each mandate version and each ASK
  approval signed with my step-up credential and journaled, in the manner of AP2's signed mandates,
  so that my intent is provable. *Accepted when:* an unsigned or wrongly signed version or approval
  is refused and journaled, and the signature verifies from the journal alone. Any legal wording
  about what a signature means is reserved for the founder (DEC-79).
- **E18-6 (Proposed, not scheduled)** As a connector or agent vendor, I want the conformance suite
  packaged as a certification kit so that I can show my integration is safe to connect.
  *Accepted when:* a third party runs it against its connector or agent and gets a report of pass^k
  over seeded fuzz runs, reproducible from the seed.
- **E18-7 (Proposed, not scheduled)** As a buyer, I want an adversarial bench so that I can see what
  a compromised model can do. *Accepted when:* with injected news, filings, and tool outputs, it
  measures the limit-breach rate given a fully compromised model (every model output adversarial),
  and the pass condition is zero breaches.
- **E18-8 (Proposed, not scheduled; SC)** As a platform operator, I want tenant isolation as a
  checked invariant so that no tenant can read or affect another's state. *Accepted when:* each
  tenant has its own chains and anchors, type or layer rules make cross-tenant access
  unrepresentable where possible, and a cross-tenant fuzz finds no leak.
- **E18-9 (Proposed, not scheduled; SC)** As an enterprise admin, I want OIDC and SAML SSO, SCIM,
  and AUDITOR and SUPERVISOR roles with ASK routing to supervisors so that the harness fits my
  identity and supervision model. *Accepted when:* each role's permissions are tested, and an ASK
  can route to a supervisor without letting anyone approve their own proposal. SAML and SCIM stay
  after v1 ([DEC-18](04-decision-log.md#decisions)).
- **E18-10 (Proposed, not scheduled; SC)** As a regulated firm, I want a self-hosted or VPC
  deployment with a pluggable external anchor (my WORM store or a transparency log) so that my
  records stay under my control. *Accepted when:* anchors written to the customer's store verify
  with `journal verify`. Relates to E13 and strategy option 15; a fully on-prem control plane stays
  Won't (v1).
- **E18-11 (Proposed, not scheduled; SC)** As a quant team, I want a sandbox for my strategy code so
  that it can propose but never trade. *Accepted when:* sandboxed code has no broker egress and no
  vault access and can emit only intents, which enter through the builder, autonomy rules, and gate
  (rule 4). Relates to DEC-20's later WebAssembly plug-ins.
- **E18-12 (Proposed, not scheduled; SC)** As a risk officer, I want sequence and flow policies over
  journaled intents so that order splitting and churn are caught. *Accepted when:* a split order
  that would breach a limit as one order is denied, and the policy reads journal state only.
- **E18-13 (Proposed, not scheduled; SC)** As an auditor, I want `mandate replay <range>` so that I
  can reproduce every gate verdict in a range. *Accepted when:* replay from the journal gives
  byte-identical verdicts, and any difference is reported, never silently accepted.
- **E18-14 (Proposed, not scheduled; SC)** As an agent builder, I want a denial to carry its reason
  codes and the tightest compliant alternative so that my agent can recover without guessing.
  *Accepted when:* the alternative is computed deterministically, passes the gate against the same
  state if proposed, and never adds risk beyond the denied request.
- **E18-15 (Proposed, not scheduled)** As a compliance buyer, I want a published compliance mapping
  so that I can trace each control to evidence. *Accepted when:* each control maps to a test ID and a
  journal event type, and CI fails if a mapped test or event type disappears. Its legal and
  compliance wording is reserved for the founder and counsel (DEC-79).

## Spec follow-ups (minor review findings, deferred by the freeze rule)

From the review of workspace API spec §4.8.1 (the audit read contracts, #767):

- Exports: define the file states `GET /exports/{id}` reports (recorded, building, ready). Map a
  failed or `Ambiguous` `ExportCreated` append to §3.5's `journal_unavailable` or `effect:
  unknown`, retryable with the same `Idempotency-Key`. Define the per-stream range when
  `recorded_at` steps back (DEC-764 notes the clock may), for example by seq bounds read from a
  monotone index. Make the view header's `format` match the request's (`json` against `jsonl`).
- `mandate-audit` enforces tenant isolation, so it is safety-critical. Its `xtask/layers.toml`
  entry, CODEOWNERS line, and lint header land with the crate (#797).

From the final review of mandate spec v0.3:

- Tie the loss carry to the broker account rather than `connection_id`; show the carry and its
  expiry at deployment.
- On the `disarm_ladder` confirmation screen, state that the lifetime floor becomes the only
  automated limit, as a percentage of the held position.
- Set a platform or retail minimum for `scale_lift_after_s` (at 0, stepwise lifts happen at once).
- Pace `trim_to_target` sells like discretionary exits (participation caps).
- Define whether the 1.25× floor hard level scales C × f or the remaining loss budget when L > 0.
- Harness default for `first_trade_in_instrument` (position quantity) versus the spec (no prior fill).
- Show the hard-trigger multiple and the 90-day carry window on the confirmation screen.
- Add fuzz coverage for multiple agents, trims, and owner exits.

From [ADR-0003](../adr/0003-earned-autonomy.md)'s guardrails (DEC-185 to DEC-197), spec changes that tighten, each with its invariant
fuzzed and seeded bugs caught before code:

- `autonomy.tripwires` (DEC-187): conditions over recorded outcomes, actions `end_delegations` or
  `exits_only`, risk-reducing to add; MI-31 and V-044.
- `autonomy.review_by` (DEC-188): a §7 platform default of 90 days, at most 180; past it every `auto`
  and delegation reads as `ask`; MI-32.
- The delegation shape chosen on `ApprovalResponded` joins journal spec §9.1 when the approval
  events close (M7); `DecisionMade`'s delegation and client members are closed (DEC-252).
- The per-client ask budget (DEC-195, DEC-251): at most 10 client-requested asks per client per
  risk day, which the owner may lower, suppressed as `client_budget` after §6.4's per-agent `budget`; MI-33.
- The delegation total (DEC-196): the sum of `max_total_usd` over a version's delegations is at most
  `capital.allocation_usd`; V-045.
- The unasked-dollars figure (DEC-189): its formula in mandate spec §4.2 beside the confirmation
  screen's worst-case figures, with a reference-model function the web figure is tested against.

From the independent reviews of stream J's implementation (`mandate-research`, #158 and #159):

- `reference/mandate/ref.py`'s `lineage_fold` starts from an empty lineage map and ignores the
  `lineages` input, while `fold_theses` continues from the `LineageState` it is given. No case
  exercises the difference; align `ref.py` or record the continuation in DEC-132.
- `ref.py`'s `T()` reads instants to whole seconds while the crate compares nanoseconds, so the two
  differ one nanosecond past a horizon. The crate is right; every fixture uses whole seconds.
- `ref.py`'s `_group_claimed` defaults an ungrouped instrument's group to its asset id, so a group id
  spelling that asset id claims it; DEC-132 item 12 and the merged test admit it. Align `ref.py`.
- `ResearchError::Unimplemented` is returned by no entry point, but stays until
  `crates/mandate-research/tests/rules.rs` stops constructing it (a tests correction).

From E10-3's implementation (DEC-172 items 1 and 12):

- Align `reference/mandate/ref.py`'s `classify` with the crate where the crate reads more strictly or more exactly: pinned instruments compared by whole entry (a symbol or asset-class change is an added instrument), an absent member and a `null` one reported as a changed path, and a same-set reordering of `asset_classes` neutral. No MC-C case exercises any of the three, and the reference's reading of the first is the one that could skip step-up.
- From the review of #318 and #319, three minors for the next `mandate-spec` tests correction:
  - replace the four near-identical `ValidationContext` fixtures in `tests/change.rs`, `tests/validate.rs`, `tests/goal.rs` and `tests/risk.rs` with one `validation_context()` in `tests/common/mod.rs`;
  - give `tests/change.rs` a `runner_with(cases)` instead of the MI-11 property's inline copy of `runner()`'s four fields;
  - `ChangeClass`'s derived `Ord` is now what `join` relies on (DEC-172 item 8). Only `the_class_order_is_the_severity_order` and four MC-C cases catch a reorder, so either give the variants explicit discriminants or point the enum's doc at that test.

From the independent review of E10-1's slice-S implementation ([#225](https://github.com/kunwarshivam/mandate/pull/225)
round 1), as the coordinator ruled there:

- `tests/document.rs`'s `two_documents_that_differ_only_in_order_hash_the_same` parses one value twice,
  and a canonical `Object` is a `BTreeMap`, so it pins determinism, not the order-independence its name
  claims. Fix it in the next tests correction that touches the file, from key-shuffled JSON text read
  through `mandate_canon::parse`.
- Done in E10-3's tests PR (`tests/change.rs::the_version_vector_is_pinned_by_its_literal_digest`):
  a live `mandate-spec` test now pins `btc_accumulator`'s literal `sha256:9fb03f7e…`, so a
  non-canonical writer no longer survives the canonical tests.

- `tests/vocabulary.rs::every_error_variant_has_its_own_stable_code` lacks `(ParseError::Diverged, "diverged")`.
  The code is pinned by the module test `a_mandate_changed_after_parsing_has_no_version`, but not in the
  table that asserts one code per variant. Add the row in the next tests correction that touches the file
  (#225, the coordinator's note after merge).

From E10-1's slice-V implementation (DEC-161):

- **`reference/mandate/ref.py`: `violates` reads a level's `null` as a limit** (#263 round 2). For
  `two_approver_above_usd` a level stating `null` should state nothing (DEC-128 item 30(b)); `ref.py`'s
  `violates` compares against it. Align the reference with the crate.
- **`PolicyOverlay::effective`'s `(Some(ceiling), Absent)` arm returns the ceiling without checking its
  type against the key** (#263 round 2, minor). A wrong-typed ceiling is refused elsewhere
  (`invalid_input`, item 30(d)); this arm should refuse it too.
- **Stream H: a gate holding a `Purpose` narrows only through `AddingPurpose::try_from`** (#263 round 2,
  nit 2). `PolicyOverlay::narrow` takes an `AddingPurpose`, so an exit's built-in AUTO cannot reach it
  (DEC-128 item 30(i)); the builder and the gate must convert with `try_from` and leave an exit's
  decision untouched on `Err`, never map an exit onto `Open` or `Increase`.
- **`ValidationContext::from_journal` (stream F, DEC-169):** implemented; its 17 tests are live.
  Stream L's E7-10 (DEC-168) maps the records to `JournaledFact`: `AccountSnapshotRecorded`,
  `ConnectionEstablished`, `ConnectionRevoked`, `DisclosureAccepted`, `AgentDeployed` and
  `MandateVersionApplied` (both `AgentVersionActive`), `UniverseChanged`, `AgentStopped`,
  `ConfigSnapshotRegistered`, `PlatformOperatorAction` (`model_withdrawn`), `MandateVersionCreated`, and
  `MandateConfirmed`. `AgentFlat` needs a source there too (the account ledger's flat-in-every-instrument
  signal). A record left unmapped is a fact the fold never sees, so the mapper's completeness is what
  covers the facts that only add (DEC-169 item 2). **Journal spec v0.7 §9.2 leaves
  `MandateVersionApplied` and `UniverseChanged` (account stream) to their own story** (DEC-303 item 6):
  E7-10 maps the records §9.2 closes, and those two go with the "account-stream risk-state records" row
  below. Until it lands they map to none, so the fold fails closed: no version change after the
  deployment and no admitted instrument are seen.
- **Journal spec v0.7 §9.2's follow-ups ([DEC-261](04-decision-log.md#decisions)).** Until each
  lands, the drafts it names stay refused at `append`, which adds no risk (rule 3).
  - **Stream K:** the executor's fee-step `AccountSnapshotRecorded` writes `model_cash`, `cash_band`,
    and `cash_in_band` as `null`, not absent (§4.2), and every snapshot writes `risk_clock` as a
    whole-second timestamp, not integer seconds (DEC-302; `payload::clock` writes integers on every
    account-stream event). **Done: the writer landed in #456, and `AccountSnapshotRecorded` registered
    with rule 24 together with `fees` journaling that writer, tests first (#467), then implemented
    ([DEC-402](decisions/DEC-402.md)).** DEC-261 item 7's ordering held throughout: the fee step pauses
    every agent and alerts the owner, and refusing its snapshot at `append` never stops it (rules 3 and
    13). `an_account_snapshot_is_closed_and_checked_by_rule_24` turns on, in Rust, all 15 snapshot
    drafts' own `expect` (2 valid and 13 invalid), `account_snapshot_recorded_refuses_an_unlisted_member`
    closes the schema, and `the_fee_steps_snapshot_is_never_refused_for_its_members` pins the
    registration and the fee step's writer together; DEC-303 item 4's three live ordering pins are
    superseded by them.
    The writer's pending pins are #441's (DEC-305 to DEC-307): the fee-step snapshot's payload
    member for member and type for type, the `risk_clock` stamp, the pause and alert whether or not
    the snapshot recorded, `IntentReceived` as §9.1's nine members, and `OrderSubmitted`'s
    `limit_price` as `null`. What the executor's fold reads beyond the registered `IntentReceived`
    and `OrderSubmitted` waits on DEC-360 (Proposed, the founder).
    The implementation PR after #441 (DEC-389, DEC-390) makes all five writers and the seven pins
    live, stamps `risk_clock` as the timestamp on every account-stream event (the fold reads both
    forms), and takes the fee step's pause off its snapshot's `?`. Still to wire:
    `fee_step_snapshot_fields` into `fees`, in the registration change that corrects the live pair
    pin `the_fee_steps_snapshot_is_never_refused_for_its_members` with `mandate-journal`'s partner
    (DEC-389 item 2); and the `IntentReceived` and `OrderSubmitted` writers, with DEC-360's ruling
    and a tests PR that brings the proposed `tif` to `IntentHandoff` (DEC-389 item 3).
    Follow-ups from its review (#456 round 1):
    - **Before E7-2, E7-3, and E7-4 remove their `#[ignore]` lines:** `tests/properties.rs`'
      `number()` readers of `risk_clock` (`:398`, `:2637`, `:2363`) read `Value::as_int`, which the
      timestamp stamp never matches. Give them a timestamp-aware read first, or the planted-bug-14
      and planted-bug-19 oracles pass while checking nothing (DEC-390 item 4).
    - `fee_step_pause_and_alert` repeats `orders::every_agent_alerted`'s `AgentModeApplied` body
      because that helper drops the draft's id; have `every_agent_alerted` answer the `EventId` and
      keep one writer for the record, in the change that next touches either.
    - `fees`' snapshot-error arm matches `Err(_)` and records nothing about why the snapshot was
      left out; narrow the match or journal the reason, so a defect (an `Unimplemented`, a key
      refusal) cannot pass as a refused snapshot.
    - The pin `the_fee_steps_pause_and_alert_run_whether_or_not_the_snapshot_recorded` still says
      `fees` journals the snapshot behind a `?`; correct the sentence in the change that corrects
      the pair pin and wires the snapshot payload (DEC-389 item 2).
  - **Stream I / M7:** the runtime's `OwnerCommandRefused` writes `effective_at` as a §4.7 timestamp
    rather than risk-clock seconds (`escalation.rs`, `payload::seconds`). §9.2 supersedes DEC-291
    item 1's "same second" for this member. `crates/mandate-runtime/src/escalation/tests.rs`'s assertion
    that `effective_at` is an integer changes with this writer.
    The writer's pending pins are the M7 tests PR's (DEC-308 to DEC-310): the refusal's
    `effective_at` the §4.7 timestamp of the judged second, for a resume refusal, a Stop refusal,
    each of the four reasons, and the `refused_stop` vector's own form, read from the committed
    vectors. The in-module late-Stop pin goes pending with them (the coordinator-named test
    change), and `refused_only` reads the judged second in either form, so the implementation PR
    deletes `#[ignore]` lines only (DEC-309 item 3). Tests merged (#462) and the implementation
    merged (#469): `refused` stamps through `payload::stamp`, and the refusal drafts commit at
    `append`; the follow-up on `agent/m7-refused-timestamp-followup` pins the stamp's §4.7 range
    and corrects the stale doc comments. Still open from the two reviews:
    - DEC-308 item 2 and DEC-310 item 1 name the integer form's refusal at `append` as
      `non_canonical`; the vectors and `mandate_journal::Draft::parse` give `schema` at
      `payload.effective_at` (the #469 review, minor 2). The code's doc comment is corrected; the
      decisions need a note in a docs change.
    - #462's description says three live tests read `refused_only`; there are four (its round-2
      review, minor 7). Description wording on a merged PR, recorded here only.
    This follow-up row needs its own story id: its pins and stub cite E8-3, which
    `cargo xtask ci pending` holds to agree but which the tracker records as finished (#395, #397).
  - **E7-1:** the connect flow's `ConnectionEstablished` records the connecting user and step-up
    (HLD §8), as a new `schema_version` with its own vectors. Specified as version 2 in journal
    §9.8 ([DEC-800](decisions/DEC-800.md) item 3); E7-1 writes it.
  - **Proposed, item 9:** `PlatformOperatorAction` closes with the operator service's specification,
    which must name each action's members: the operator stop's subject, the global kill switch's
    scope, the acceptable-use action, and the row's "approval".
  - **Item 10, closed by [DEC-800](decisions/DEC-800.md):** `ConnectionEstablished` version 2's
    `account_ref` binds an account stream to its connection (journal §9.8); the mapping reads its
    argument from that binding.
  - **Account-stream risk-state records (stream K with stream L; DEC-303 item 6):** journal spec v0.8
    §9.3 closes `MandateVersionApplied` and `UniverseChanged` (mandate spec §5.10, §2.3), with the
    vectors' `risk_state` section ([DEC-403](decisions/DEC-403.md)). The tests and implementation that
    follow register them and map them to `AgentVersionActive` (a deployed agent's new version) and
    `UniverseChanged`. **Done ([DEC-404](decisions/DEC-404.md)):** tests first in #482, then the
    registration and the mapping on `agent/l-risk-state-impl`, with the requirement below.
    **Required, and met by the implementation (#470 round 2, minor 5):** the Rust
    registration re-derives mandate spec §9.2's classification of `new_version` against
    `old_version` from the two stored documents and refuses a `MandateVersionApplied` whose
    `classification` differs. Rule 33 covers only what the record itself carries (the allocation
    change, the floor, and three rejections), so until this lands a risk-increasing version through
    any other §9.2 row can be journaled as applied, labelled `neutral`, with no step-up (DEC-403
    item 5). **Two limits on it (#482 round 1, M2; DEC-404 item 7):**
    - **Coverage.** The re-derivation inherits `change::classify`'s coverage, and DEC-353's two
      shapes (#444, #471) are not in the Rust classifier yet, so E6-13's classifier code PR is part
      of closing this hole.
    - **Order.** A change to `classify` is journal-affecting: a record labelled by an older
      classifier can become unmappable, and a context that will not build is not a hold an exit may
      have (rule 13). So E6-13's classifier change lands before any path builds a context from a
      real journal (DEC-169's wiring). That order is a named prerequisite here and on E6-13's row.
    - **Unexercised shapes (#482 round 2, m3):** no pair reaches `classify`'s `join` with more than
      one class over several rows (the only multi-path pair is the pinning switch, which returns
      first), and no pair is two identical documents.
    - **`asset_id` (#497 round 1, m3; DEC-404 item 9):** done. Journal spec v0.10 types `instrument`
      as an asset ID (#503), `mandate-journal` enforces it with `result` matched exhaustively (#509),
      and the reference validator, its `asset_id` vectors, and #503's m1 and m2 text follow on
      `agent/l-risk-state-asset-id-vectors`; the differential test that pins the journal's predicate to
      `AssetId::parse` (#509 round 1, m1) is its own code PR, `agent/l-asset-id-differential`.
    - **Members appended under a looser type than the mapping parses (#503 round 1, m3).** Each has
      the same shape as `instrument` had: it appends, then makes the stream's `ValidationContext`
      unbuildable, and a context that will not build is not a hold an exit may have (rule 13).
      - `ConfigSnapshotRegistered.model_id`: `text?` at append, but `ModelId::parse` at the mapping.
        This is the widest of the three.
      - `agent_id` and `connection_id`: `id` with no length bound at append, but 1 to 64 characters at
        the mapping (DEC-303 item 16).

      Take both after the DEC-360 change, as one DEC-176 tightening with the same three steps: the
      spec, then `mandate-journal` test first, then the reference vectors.
    - **The pairs are not valid mandates (#482 round 2, m1):** six of
      `a_version_maps_only_under_the_classification_its_documents_give`'s pairs break a V-rule (V-008,
      V-013, V-034, V-036). Make each valid with a second patched path, so every fixture passes
      every rule it is not meant to fail.
- **MC-V status PR (stream F, after the E17-1 slice):** V-003, V-034 to V-037, V-039, W-006, and
  `worst_case_stop_distance` landed in their own slice (DEC-161 items 1 and 10), so all 67 MC-V cases pass
  locally; a status-only PR moves them to `passing` (DEC-77 item 3).
- **Confirmation-screen PR: the stop distance and the figures disagree on precision** (#252 review,
  minor 3). `worst_case_stop_distance` sums at a `Ratio`'s 24 places, while the four figures stop at
  `Fraction`'s 9 (`out_of_range` past it, DEC-161 item 3). A document with a 10- to 24-place stop gets a
  distance but no figures. The screen that shows both must pick one boundary.
- **Blocks any production caller of `validate`: an unmentioned envelope path reads as confirmed**
  (#252 review, minor 4; the coordinator's ruling there). `ProvenanceMap::at` defaults an unmentioned
  path to `user_entered` and confirmed, and V-020 and V-022 read only the entries present, so a
  document with no entries passes both, even with `admission: auto`. The fix lands in the
  `ValidationContext::from_journal` implementation PR (DEC-169): an unmentioned envelope path is
  unconfirmed, fires V-020, and an `auto` under it fires V-022. No production caller may use
  `validate` or `ValidatedMandate::new` until then.
- **Stream H:** `ConditionField::is_unit_bounded` and `mandate-builder`'s `well_typed` omit
  `thesis_confidence`, which §6.3 types "decimal in [0, 1]"; `validate` bounds it (DEC-161 item 5), so the
  order path's re-check is looser than the load check. Fix both with the builder's
  `oracle_is_unit_bounded` in one change.
- **`reference/mandate/ref.py`:** `PLATFORM_DEFAULTABLE` gives `leveraged_etp_disclosure_version` the value
  `None`, which the reference reads as "any value"; §7 allows only `null` (DEC-161 item 8). Give the
  reference a sentinel for "any value" so `None` can mean `null`.
- **W-005 misses a wrapped catch-all** (#238 review, round 1, minor 1). `Condition::is_catch_all` reads only
  a top-level `purpose in [increase, open]`, so `{"all": [{"field": "purpose", "op": "in", "value": ["open",
  "increase"]}]}` and `{"all": []}`, which also match every action a later rule could, warn of nothing.
  Warning-only, never blocking; widen it to any condition that holds for both purposes.
- **`validate::tests::oracle_default_allowed` returns `true` for `/environment` whatever its value**
  (#238 review, round 1, minor 1), so the V-020 property never exercises §7's `paper`-only bound there
  (`v020_reads_the_source_the_confirmation_and_the_listed_value` does). Make the oracle check `paper` in a
  later tests correction.
- The worst-case figures multiply by `Fraction`, which holds nine places, so a schema-valid fraction with
  ten or more is `out_of_range` (DEC-161 item 3). Move to an exact `Usd × Ratio` when `mandate-num` has
  one (stream H's `UsdExact` is the candidate).

From the independent review of E4-2's implementation ([#163](https://github.com/kunwarshivam/mandate/pull/163)
round 2, verdict approve), whose first two minors are closed by the third tests correction
(DEC-127 item 26) and whose third waits on another story:

- `NumError::Unimplemented` stays: it is returned by the fourteen `mandate-num::sizing` stubs E6-2 owes,
  so it is live code, not a leftover, and its row in `num::error_codes_are_stable` holds the wire
  spelling those stubs return. Drop the variant and the row together in a tests correction once E6-2's
  sizing is implemented and nothing constructs it.

From the independent review of stream H's tests PR (`mandate-builder`, [#175](https://github.com/kunwarshivam/mandate/pull/175)
round 2, verdict approve), each deferred by the freeze rule and none of them a gap in what the tests
assert:

- **Floor the accumulate goal clip per bound, not as a whole.** `properties::accumulate_coverage_reached`
  requires a goal clip; it does not require each of §8.3 step 4's three bounds to have been the binding
  one. The review measured them over 256 cases: the remaining quantity bound 151 times, the spend 7 and
  the average price 2. A floor at 2 in 256 would be flaky across seeds, so the fix is not a counter on
  its own — the `Accumulate` shape has to make each bound bind reliably first, which today it cannot,
  because it always sets `max_avg_price` to the ask and so leaves `a − max_avg × β` at zero and the
  average-price **clip** off (only its guard runs). Each bound is pinned exactly by
  `hand::accumulate_clipped_to_the_remaining_target_quantity`, `hand::accumulate_clipped_by_max_spend`
  and `hand::accumulate_clipped_by_max_avg_price`, so this buys explicit fuzz evidence, not new coverage.
- **Carry fees in the rational sizing oracle.** It takes `a = ask` and `β = 1`, so the fee arithmetic of
  §8.3 step 4 is pinned by `hand::accumulate_with_fees_counts_the_spend_and_the_quantity_received` and
  `MC-B28` rather than by the oracle. Widening the oracle means widening the generator to fee rates,
  which changes the rational magnitudes the `i128` oracle carries; worth doing deliberately.
- **The coverage counters are process-global statics.** Under `cargo test`'s shared process another
  property's cases could feed the gate. `cargo nextest` gives each test its own process and is what
  both `cargo xtask check` and CI run, so the gate is sound as used; a per-run counter would make it
  sound under either runner.

From the independent reviews of stream K's tests (`mandate-executor`, `mandate-alpaca`, #152):

- Key the reference-case partition check (`refcases::the_fixture_partition_is_driven_plus_dropped`)
  to the scopes the suite actually drives, not to the `executor` and `reconciliation` labels. Five
  of the sixteen driven entries sit outside that filter (`RC-06`'s
  `protective_orders_kept_through_dividend` is `accounting`; `RC-15` and its three variants are
  `gate`), so a new `gate`-scoped case still leaves the suite silently; renames and removals of
  every named entry are already caught (round-3 review finding 3).
- Normalize a crypto symbol `mandate-alpaca` reads back from the broker. Position paths now write
  `BTC/USD` as `BTCUSD` (`http::position_path`, the positions read and the account-wide close), but
  `wire` keeps whatever symbol a response names, so a position reported as `BTCUSD` and an order
  placed as `BTC/USD` would be two instrument ids, where trading-domain spec §2.3 makes them one.
  Only a recorded crypto position shows which form the paper host sends; normalize on
  `asset_class: crypto` before E7-3's reconciliation compares crypto positions.
- Read a working external notional order's exposure. `wire` ingests an external order placed by
  notional with `qty` equal to its `filled_qty`, so a working one understates what it can still
  buy, and its `notional` is read nowhere and is not in `record::RECORDED_FIELDS`. The exposure is
  bounded meanwhile: external activity puts every agent on the account in `exits_only` and blocks
  claiming the instrument until the owner acknowledges it (trading-domain spec §7.1), so no agent
  adds risk beside it (#195 review, round 1, finding 6).
- Make "only the fold writes folded state" a type guarantee in `mandate-executor`. `ExecutorState`'s
  fields are `pub(crate)`, so the rule that only `fold.rs` writes folded state and `step.rs` writes
  only the process-local fields (the epoch, `started`, the latest tick, the unresolved append) is a
  convention the review holds. A `FoldedState` newtype with private fields, written only through the
  fold and read through accessors, moves it to rung 1 (#194 review, round 1, finding 5).
- **E7-7, once stream E registers agent-stream payload schemas:** move `mandate-shell`'s
  committed-draft ledger from `mandate_canon::parse` to `mandate_journal::Draft::parse`, the
  oracle the brief names, and run `verify_events` over the in-module keystone's streams. Today no
  agent-stream event parses there (DEC-157 item 7; #227 review, round 1, minor 3), and the
  executor's account-stream drafts do not match the registered schemas either (DEC-174 item 5).
- **E7-7, blocking the slice that lets the crossover drive an order:** bound the stored bars'
  staleness. Check the span's last day against the run's `setup.now` (the last completed session
  before it) and refuse coverage that ends earlier. Today `Bars::closes` reads no clock, so a
  months-old dataset is trusted and feeds the signal. That is harmless only while every downstream
  stage refuses (DEC-166; #241 review, round 1, minor 5).
- **E2-14, blocking `tests/tracer.rs::outlier_close` (PB-15):** the wrong-high-print story owns the
  market-data trust rule that refuses a close too far from its neighbours. The shell may not judge
  one, because that is price arithmetic (DEC-138 item 3, DEC-166 item 5). The test stays pending on
  E2-14 until its founder-gated price-trust rule lands in the owning market-data path, or until the
  binding gate refuses the limit end to end (the coordinator's ruling on #171).
- **E7-7, when streams F and H land:** a drift check for
  `crates/mandate-shell/tests/fixtures/tracer/generate.py`, like `reference/mandate/generate.py`'s,
  so the fixture's one share at 255.20, AUTO by `rule:routine`, stays recomputed from the rules
  (#227 review, round 1, minor 4).
- **Before stream G's gate is wired in:** register `startup_reconciliation_pending` in the
  trading-domain `reason_codes` registry, or record why not. It is a third partial-gate reason code
  outside the registry, beside `instrument_not_in_universe` and `broker`, so ES-09's stable reason
  codes do not yet cover what the partial gate journals ([DEC-129](04-decision-log.md#decisions)
  items 23 and 27, ADR-0001 ES-09; #206 review).
- **Before E6-10's implementation merges:** register `crypto_pair_not_usd` in the trading-domain
  `reason_codes` registry and name it in §3.2 item 7, or record why `not_in_working_universe`
  stands. Mandate spec §5.3 defines that code as "the instrument is in the working universe", which
  a BTC/USDT pair the research agent admitted is, so the journaled denial would say something false
  about it. Registering a code adds no risk and closes a gap, so DEC-176 lets an agent do it in its
  own spec PR; the tests' `PAIR_CODE` constant flips with it ([DEC-254](04-decision-log.md#decisions)
  item 3; DEC-129 items 25 and 27; #342 review, minors 1 and 2). *Done (#352,
  [DEC-255](04-decision-log.md#decisions)):* the code is registered and §3.2 item 7 names it; the
  gate's emission and the `PAIR_CODE` flip stay with E6-10's implementation.
- **The broker symbol's quote currency is read exactly** (E6-10; #342 review, minor 3). The gate's
  USD-pair rule rests on the §3.1 loader mapping a pair to `QuoteCurrency`, and E7-8's
  `TradingClient::asset` criterion does not name it. The loader matches `USD` exactly and
  case-sensitively, with tests that `usd`, `USDT`, `USDC`, a padded code, and an absent symbol all
  land on `Other` or `None` (DEC-254 item 1).
- **E6-10's tests nits** (#342 review): `usd_pairs.rs`'s property sets `quote_currency` twice for a
  crypto draw; DEC-254 item 3's alternatives omit DEC-129 item 27's "assert the verdict, leave the
  code unasserted" option; and `cargo xtask ci pending` accepts any `Unimplemented` report rather
  than the story its `#[ignore]` label names, which is how `hand::crypto_never_counts` sat labelled
  E6-6 while failing at E6-10's stub. Compare the stub's story with the label if it recurs.
- **`crypto_pair_not_usd`'s wording follow-ups** (#352 review, minors 2 to 4 and nits), in one
  docs change after #352 merges:
  - DEC-255's opening parenthetical says DEC-254 item 3 is "not yet on `main`"; #342 merged as
    `e7c870b` before DEC-255 was written. Drop the clause (minor 2);
  - `docs/project/08-work-tracker.md` still names trading-domain spec v0.12; every earlier bump
    updated it in the same PR. Say v0.13 and cite DEC-255 (minor 3);
  - trading-domain §3.2 item 7 names the code only for a pair "quoted in anything else", but
    DEC-254 item 1 and `usd_pairs.rs`'s `NOT_USD = [Some(Other), None]` deny an unstated quote
    currency the same way. Say "quoted in anything other than USD, or whose quote currency is not
    stated" (minor 4);
  - the v0.13 change-history entry's "No existing code changes (ES-09)" means no registered reason
    code changes; say so (nit);
  - the E6-3 brief's check-2 row carries an inline parenthetical in an otherwise bare list of
    codes; the Story column already names E6-10 (nit).
- **DEC-253's mutation-scope wording** (#345 review, minors and nits), one docs change:
  - ADR-0001 ES-13 names `risk_gate.rs` and `order_builder.rs` as the recorded exceptions, but
    ES-13's limit is per change, so read alone it licenses a later 900-line change to either file.
    Say the exceptions are the two merged changes that added them, and that neither exempts a
    later change (minor 1);
  - the `verify-mandate` skill's rule 5 states that pending-only harness lines survive the gate but
    not the remedy. Add: drive the line from a live doctored-case test in
    `crates/mandate-refcases/tests/`, as `mandate_gate_harness.rs` does (DEC-253 item 2) (minor 2);
  - DEC-253 item 3 splits a large harness arm into stacked 400-line PRs without saying each slice
    must carry the doctored-case tests item 2 requires, or the gate fails it (minor 3);
  - the family-B survivors row calls the `listing` site `qty_increment < 1`; the code reads
    `stated.increment < one` (nit);
  - "the 14 mutants that survive in merged harness code" is what two sampled diffs found, not a
    census; say so where it is repeated (nit);
  - the xtask fixture names a crate `core`, shadowing `std`'s; `base` or `product` reads better
    (nit);
  - `mutated_crates` no longer parses `layer`, so a typo there surfaces in `lint`, not `mutants`;
    intended, recorded so it is not mistaken for an oversight (nit).
  Also worth knowing: a change that touches only `crates/mandate-refcases/tests/` never starts the
  gate, so a tests correction that kills survivors proves it by re-running the gate over the
  original diff locally.
- **The journal generator's mutant coverage** (#340 round-2 review, minors and nits), one change to
  `reference/journal/generate.py` and its docs:
  - the "every oracle check has a registered vector mutant" guard filters on
    `check.startswith("owner_copies.")`, so 20 of the 44 `ORACLE_CHECKS` entries (for example
    `kill_switch.stopped`, `goal_exit.origin`, `owner_exits.sell`, `owner_exits.user`) can be deleted
    with `--check` still green. Require a mutant for every entry, with an explicit allow-list for
    the chain checks that re-verify the generator with its own `canon` and `sha256_hex`
    (`chain.seq`, `chain.canonical`, `chain.hash`, `chain.stream`, `chain.opened`) (minor 1);
  - `received.intent` is its own family, which silences the masking rule rather than isolating a
    mutant, and it has no mutant of its own, so deleting its oracle leaves `--check` green. Add
    one only it catches; the review showed "intent recorded after the account stream copied it"
    (`recorded_at` of the `intent` body moved to `2026-09-21T14:00:00.900000000Z`) does (minor 2);
  - `VALIDATOR_MUTANTS` lists `rule.16.mode.owner_pause`, `owner_resume`, and `owner_stop` by hand
    while the skip key derives from the reason, so a fourth `OWNER_MODE_REASONS` entry would get no
    mutant. Generate the entries from `OWNER_MODE_REASONS` (minor 3);
  - DEC-177 item 25(b) and #340's description say three mutants were re-seeded; two were, and the
    third was renamed into `received.intent`'s family, its own mutant still owed (nit);
  - `docs/specs/journal.md`'s rule-16 prose has a 107-character line; rewrap it (nit).
- **Family B's clock fix follow-ups** (#351 and #347 reviews, minors and nits; the fold-back,
  [#359](https://github.com/kunwarshivam/mandate/pull/359), removed `CLOCKED` and closed three of them):
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359), which deletes the function):* `labels_agree_with_calendar` in
    `crates/mandate-refcases/src/mandate/order_builder.rs` reached its answer through the harness's own `mandate_risk::session_at`, `test_default_gate_config()`
    and `market_session`, so a bug in any of them (the close window set to 0, `AfterHours` mapped to
    `Regular`, or `session::derive` reporting `Regular` after hours) flips oracle and harness
    together; only the literal-clock test caught them. Compute the expected session from a literal
    New York clock, or give each `CLOCKED` row its agreeing instant (#351 review, minor);
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359)):* `labels_agree_with_calendar` re-walked `family_b` for a case its caller
    holds; take `&case` (#351 review, nit);
  - **open:** `reference/mandate/check_cases.py`'s `calendar_at` assumes a full trading day for
    every builder case. On 2026-11-26 (closed) and 2026-11-27 (early close 13:00, after hours to 17:00) it
    disagrees with `crates/mandate-time/data/us-equities.calendar` both ways. Refuse, naming the
    case, when `now`'s date is not a full day in that calendar, or read the calendar (#347 review,
    minor);
  - *done ([#359](https://github.com/kunwarshivam/mandate/pull/359), now 25):* `.cursor/skills/verify-mandate/feature-map.md`'s family-B bullet
    said 23 pass and that MC-B22 and MC-B23 fail on a contradicting label (#347 review, minor);
  - **open:** `reference/mandate/generate.py`'s `at_now` moves only the outputs' `as_of` and
    `expires_at`, not `gate_state.last_exit_fill_at`; assert that map is empty, so a later caller
    cannot move `now` past a re-entry cooldown unnoticed (#347 review, nit);
  - **open (#359 review, nits):** DEC-250 item 12's earlier amendment still names
    `the_two_session_cases_pass_once_now_agrees_with_their_labels` and its MC-B23 doctoring, both
    gone; the renamed `the_two_session_cases_pass_as_stated_and_compare_every_sibling` repeats what
    `every_expected_member_is_compared_and_required` and `every_builder_case_passes_or_fails_at_its_owner`
    already run, so fold it into them or say in its doc that it only names the two comparisons; and
    `CLOCKED`'s needles asserted the conflict message's whole wording ("states `after_hours`", "the
    calendar says `regular`"), where MC-B01's doctored proof in `the_builder_and_the_gate_see_one_scene`
    asserts only the member name, so pin the full message there. Replace `order_builder.rs`'s own
    `fails_naming` with an exact-message helper, as #360 did for family A.
- **E7-4 slice 1's tests correction:** close the do-nothing gap in `mandate-executor`'s generator
  properties. 29 of the 33 pass when every reachable stub returns `Ok(())`, so a no-op executor
  would satisfy them; each property must also assert a positive effect a no-op cannot produce
  (#231 review, follow-up a).
- **Before E7-3's buying-power path reads them:** the properties' model broker reports `equity` and
  `buying_power` that follow its `cash_moved`, as `cash` already does; today they stay at 20000
  whatever its fills (#231 review, follow-up b).
- Assert the ready precondition in `mandate-executor`'s properties script: after its start, an
  account has been observed and the startup `ReconciliationRun` recorded, so a script that stops
  starting ready fails at its start rather than at a later assertion; and update `play`'s doc to say
  it starts ready (#231 review, follow-up c).
- **Blocks E7-4 slice 5 (the trading day):** `mandate-executor` must copy the cross-stream facts
  journal spec §2 gives it (`AgentModeApplied` from the agent stream's `AgentModeChanged`,
  `TradingDayStarted`, `ClockAdvanced` crossing midnight America/New_York, `OwnerAcknowledged` from the
  control stream), each with its `causation_id`. An acknowledgment whose step-up does not count is
  copied as `OwnerCommandRefused` instead (command `acknowledge`, the reason, `effective_at`), lifts
  nothing, and is tested first to leave exactly that event (DEC-291 item 4, journal spec §2). Before E7-4 slice 1, `step`'s `Input::Journal(_) => Ok(())` copied
  nothing, silently, so `properties::every_copied_draft_cites_its_origin` sees no copied draft under any
  script and passes vacuously. The slice that adds the producer also adds a generator step (a clock
  advance crossing midnight New York, an owner acknowledgment) and asserts `seen > 0` on scripts
  containing it, shown failing under the do-nothing plant (#244 round 1, finding 3). Until then
  `Input::Journal` answers a loud `Unimplemented { story: "E7-4" }` naming slice 5, landing first in
  E7-4 slice 1 rather than dropping the fact (the coordinator's ruling on #244, 5861479849).
- **E7-4, the slice that reconciles protective legs (stream K):** make `mandate-alpaca`'s `wire.rs`
  keep each leg's `client_order_id` instead of reading `legs[].id` only, with its own `ready()` tests
  correction first, since `BrokerOrder.legs` changes type (#229's pattern). E7-4 slice 1 aligns
  `ClientOrderId::for_protection` to the §2.3 grammar (`{entry}-p{protection}`, legs `-tp` and `-sl`)
  and reads no leg id from a `BrokerOrder`, which an in-module test pins. Until the wire change lands,
  a broker-reported leg is attributed by the single holder or fails closed for openings; exits are
  untouched ([DEC-160](04-decision-log.md#decisions) 3a, #243 round 1, the coordinator's ruling (b)
  on #174, 5861764910).
- **E7-4 slices 2 and 3 (stream K):** `properties::protective_sell_quantity_never_exceeds_the_position_in_any_script`
  wants the `ProtectionChanged placed` at or after the entry's completion with no lag. That is right
  on the normal path (§5.4's legs activate at completion), but a re-placement after a
  cancelled-then-filled entry may lag by up to `max_unprotected_s`. If a slice turns it red there,
  allow that bound rather than loosening the assertion elsewhere (#244 round 3, minor 3).
- **E7-4 slice 2's tests correction (stream K):** a `properties` oracle expects `OwnerAlertSent`
  among the executor's drafts, but it is a control-stream event the executor never writes; the
  executor's alert is its own record (`ProtectionChanged interval_limit`) plus `Effect::Notify`.
  Correct the oracle before slice 2 un-ignores it (found building slice 3a, #267).
- **E7-4 slice 2 (stream K), #242's plants that go live with it:** plant 7 (held quantity 10 → 5)
  and plant 8 (held limit 150 → 100), and plant 9 (`fault::protected` on a plain `restart`), which
  the coordinator moved from 3a to slice 2 (#267, comment 5862923162): under rule 13 no exit waits
  for the startup reconciliation, so the plant gets weight only with the first opening through
  `fault::protected` (slice 2's add). Slice 2's PR shows each of the three red.
- ~~**E7-4 slice 7's tests PR (stream K; moved from slice 6 when slice 6 took #400 round 2's two items), from [#373](https://github.com/kunwarshivam/mandate/pull/373)
  round 1 (major 1):**~~ Done: the test landed with #655 and slice 7's agent-scoped implementation
  makes it live, narrowing `climbs` and the gate together through `kill::mode_holds` ([DEC-485](decisions/DEC-485.md) item 12). §5.5 exempts kill-switch and mandate-limit flatten exits from the agent's
  mode, but slice 4a's ladder stops stepping while the agent is `paused` or `stopped`
  ([DEC-260](04-decision-log.md#decisions) (3)), and the gate's `mode_failure` holds every
  risk-reducing order at `paused` or stricter. Add a pending test: while the agent is paused, a
  flatten's ladder steps, its step cancel does not end the sequence, and `mode_failure` lets that
  flatten through. Slice 7 narrows `climbs` and the gate together to make it pass.
- ~~**E7-4 slice 7 (stream K; moved from slice 5 by the coordinator's ruling D3 on #174, then from slice 6), from
  [#373](https://github.com/kunwarshivam/mandate/pull/373) round 1 (minor 1):**~~ Done for a kill switch's confirmed
  floor (`hand::an_owner_flattens_rung_rests_at_the_confirmed_floor`, DEC-485 item 11); an owner exit handed over as
  an intent carries no floor until `OwnerExitRequested` reaches the executor. wire the owner exit's floor (`OwnerExitRequested`'s confirmed floor) into
  `exit_limit` and `next_rung`, which pass none today, so §5.6's "never below an owner exit's
  floor" holds on the live paths and not only in `ladder_tests`. A rung the floor clamps sets
  `at_floor` and rests (§5.5's "any remainder rests at the floor"), rather than being cancelled and
  resubmitted at the same price every `exit_step_s`.
- **E7-4 slice 5 (stream K), from slice 4b ([DEC-260](04-decision-log.md#decisions) (12)):**
  a crypto stop-limit is watchdogged as soon as a sane mark is below its limit price, once slice 5
  places stop-limits. The session condition landed with slice 5's session part (DEC-260 (15)).
- **E7-4 slice 7's tests PR (stream K, moved from slice 6), from slice 4b (DEC-160 (11), (24)):** the agent-scoped half
  is done (`hand::an_agent_kill_switch_cancels_a_watchdog_exit_of_no_agent_and_sells_only_its_own_lots`, #655, live with
  slice 7's agent scope; DEC-485 items 5 and 13); the account and workspace scopes' half stays open. Add pending tests
  that every kill switch whose scope covers an instrument cancels a working `*` watchdog exit
  there by its own `client_order_id` (`md-w-<record>`), an agent-scoped one included when it
  closes that instrument (§5.5's table), and that an agent-scoped kill switch never treats it as
  that agent's own: it is never counted in the agent's sub-ledger sell and never reached by
  cancel-all.
- **E7-4 slice 5's session part, left open (stream K, DEC-260 (13), (14)):** the closing auction
  window is §4.3's 10-minute default as a constant in `mandate-executor`'s `session`; read it from
  the gate's effective-dated configuration (`close_window_minutes`) once the executor holds it. A
  presumed halt is read from the latest quote only (older than `exit_step_s`, or not sane); add
  the trading-status and LULD feed §4.4 names once the shell subscribes to it.
- **The US-equities calendar's end (stream K, DEC-260 (13); the coordinator's ruling on #174,
  5926945398):** alert the owner and the operator well ahead of `crates/mandate-time/data/
  us-equities.calendar`'s last valid date (2028-12-31), for example 90 days before it, so the file
  is extended before every equity exit starts being held `session_unknown`.
- **`mandate-executor`'s clock-0 tests onto the calendar (stream K, DEC-260 (13)):** the hand,
  coverage, fault and properties suites run at risk-clock seconds near 0 (1970), before the
  calendar's first date, which `session` reads as the regular session. Move them onto calendar
  dates in their own DEC-77 tests PR; after it, an instant before the calendar's range is held
  `session_unknown` like one after it. Among them `hand::a_risk_exit_submits_inside_the_close_window` runs at
  clock 25, so its venue is the regular session: it proves the conduct exemption, not the close
  window its name claims (#400 round 1, minor 4).
- **E7-4 slice 7 (stream K), moved from slice 5 by the coordinator's ruling D3 on
  [#174](https://github.com/kunwarshivam/mandate/pull/174) (5926142854):** the four kill-switch
  session tests (`hand::an_automated_flatten_defers_equity_sells_to_the_session`,
  `an_automated_flatten_sells_crypto_at_once`,
  `an_owner_exit_outside_the_session_prices_from_the_confirmed_bid`,
  `an_unconfirmed_owner_exit_waits_for_the_session`) and the owner-confirmed extended-hours path:
  outside the regular session an owner exit sells equities through the ladder only on the
  confirmed bid, bid size and floor, and waits for the session otherwise (§5.5).
- **E7-4's tests correction (stream K), from slice 2 ([DEC-346](decisions/DEC-346.md) item 7):**
  `properties::every_unprotected_interval_has_a_journaled_start_and_end` asserts that no interval
  is open when a script ends. A script that ends while the protected lead's partly filled entry is
  still inside its interval (no completing fill, no timeout reached and confirmed) fails it on any
  implementation. Judge an interval that is still open against what could have closed it, then
  delete its `BEHAVIOUR_ONLY_TESTS` row.
- **E7-4 (stream K), from slice 2 ([DEC-346](decisions/DEC-346.md), left open):** (1) a re-placement
  after an exit covers every tranche at the prices of the latest `placed`, because the fold keeps
  one price pair per instrument; decide this for tranches at different prices (coordinator). (2)
  Bracket legs and the partial-fill OCO record no `created_on`, so slice 5's trading-day part must
  date §5.4's re-placement before expiry from the placement's own record. (3) The slice that
  reconciles protective legs maps `{entry}-p{record}` to the broker's leg ids, and it must land
  before any shell path hands the executor a protected intent.
- **E7-4 (stream K), from [#463](https://github.com/kunwarshivam/mandate/pull/463) round 1
  (minor 2), its own story:** after a re-placement the broker refuses or cancels unacknowledged, the
  `unprotected_end` carrying `awaiting` has already ended the sequence, so nothing re-places
  protection again, and §5.4's bound alerts once per interval. The position then stays unprotected
  indefinitely, with that one owner alert as its only signal. Re-place (or escalate) when an
  awaited order is refused, and keep alerting while the interval stays open.
  The same holds for a re-placement before expiry ([DEC-367](decisions/DEC-367.md)), where the
  executor itself chose to open the interval: a refused re-placement is alerted once at the bound
  and never retried (#468 round 1, minor 5).
- **E7-4 (stream K), from #463 round 1 (minor 1):** a protective order the broker replaced
  (`Accepted → Replaced`, §5.7's `ReplacedPair`) is not counted as acknowledged. Its successor is
  live under another `client_order_id` that `awaiting` does not name, so the interval stays open
  on a position that is in fact protected. Follow the replacement link when deciding the
  acknowledgment.
- **E7-4 (stream K), from #463 round 1 (minor 4):** `acknowledged` journals an `unprotected_end`
  even when no interval is open for the instrument. The fold makes it a no-op, but the journal
  carries an end that ends nothing. Guard it with `interval_open`.
- **E7-3 (stream K), from [#457](https://github.com/kunwarshivam/mandate/pull/457) round 1:** a
  split prepared but not applied when the broker has already posted it is a real state after a
  crash (the model at Q, the broker at Q × new). §8.5's `pending_corporate_action` window does not
  cover it: the window runs from the application to the posting. It has two safe outcomes: the model
  corrected to the posted quantity, or the agents holding the instrument paused with an alert
  (§11's default). Choose one in a recorded decision before any test pins it.
- **E7-4 slice 5 (stream K), from [#457](https://github.com/kunwarshivam/mandate/pull/457) round 1
  (M2), a prerequisite:** add an executor-scoped reference case, or hand tests, for a reverse split
  and for a split with a fractional result. Pin that the protective sell quantity is re-scaled and
  never exceeds the position (rule 12). RC-04 reaches only a forward split on whole shares, and
  RC-05 and RC-23 are accounting-only. The slice-5 corporate-actions implementation PR does not
  merge before this pin exists.
- **E7-4 (stream K), from [#468](https://github.com/kunwarshivam/mandate/pull/468) round 1
  (minors 3, 4, 6):** (3) `expiring`'s date-arithmetic error resolves toward re-placing early
  (`.unwrap_or(true)` in `new_day`), but nothing pins it; it fails only at `Date::next`'s upper
  bound, so no test reaches it today. (4) `expiring` never checks that the order is GTC, though its
  doc says so; §5.2 makes protective orders GTC, so a `tif` check would make it unrepresentable.
  (6) `ExchangeCalendar::us_equities()` is parsed inside `expiring`, once per resting protective
  order per instrument per trading day, as `session.rs` also does; parse it once.
- **The US-equities calendar's horizon (stream K), from #468 round 2 (M1):** extend
  `crates/mandate-time/data/us-equities.calendar` well past the GTC window the executor can reach,
  and alert the owner and the operator when the calendar's end is closer than `gtc_expiry_days`
  plus `protective_replace_buffer_trading_days`. DEC-367 item 2's two counts are the net under this
  fix, not the fix.
- **E7-4 (stream K), from #468 round 2 (minors 3 and 5):**
  - (3) `working_exits` never checks the side, unlike `still_selling`. A buy with an exit purpose,
    such as a `Flatten` of a short, would count as selling. v1 has no shorts; copy
    `still_selling`'s `side == Side::Sell` filter.
  - (5) `new_day`'s `if let Some(prices)` cannot fail now: protection with no prices is alerted
    and skipped just before it (DEC-367 item 4 (a′)). Make the unreachable branch
    unrepresentable. Minor (4) went with `exit_working`, which the founder's decision on #468
    removed.
- **E7-4's tests correction (stream K), from the acknowledgment PR ([DEC-348](decisions/DEC-348.md)
  item 2):** the refcase harness's guard that an `unprotected_end` naming what it is `awaiting` is
  not read as the interval's end is reached by no live test, since every case that lists the end
  after a `submit_protective` also has the step's own acknowledgment end it, and the matcher finds
  that later record either way. Add a harness test that feeds an awaiting end with no
  acknowledgment and asserts the step does not match `journal: unprotected_window_end`.
- **E7-4 (stream K), from [#448](https://github.com/kunwarshivam/mandate/pull/448) round 1 (minor
  2, [DEC-349](decisions/DEC-349.md)):** a crypto stop-limit is sized net of the taker fee, the
  larger rate, because `BrokerFill` does not say maker or taker. After a maker fill the stop
  covers `gross × (taker − maker)` less than the holding, and nothing bounds that remainder.
  Carry `liquidity` on `BrokerFill` (the connector reads it from Alpaca's fill activity), or
  re-size the stop when the crypto asset fee posts (`FeesCharged crypto_asset`), so it covers
  exactly the holding.
- **E7-4 (stream K), from [#385](https://github.com/kunwarshivam/mandate/pull/385)'s review (minor
  4):** the rule-13 oracle's waiting-exit `limit` branch
  (`protection::sequence_tests::rule_13_script`) is dormant: no script holds an exit unexcused long
  enough to reach it. The follow-up either makes the branch bite, with a script that holds an exit
  unexcused and a plant it catches, or deletes it along with `quiet_since`.
- ~~**E7-4 (stream K), CI's `fast` red at random on `main`** (reported on
  [#445](https://github.com/kunwarshivam/mandate/pull/445), 2026-10-02):
  `rule_13_holds_over_random_scripts` refused a risk exit held `session_closed` at 2018-01-01
  00:00 ET (the New Year holiday's overnight) in a script from the eve of 2018.~~ **Fixed on
  `agent/k-rule13-holiday-risk-exit` ([DEC-392](decisions/DEC-392.md)).** The executor's hold is
  DEC-260 (13)'s. The oracle fixed the calendar's coverage at the script's start, so it now reads
  coverage at each instant, and it knows the trading days of 2018-01-02 to 05. CI's minimal input
  is the named test `rule_13_holds_for_a_risk_exit_on_the_new_year_holiday_overnight`.
- **E7-4 (stream K), found by slice 4a's rule-13 oracle (`protection::sequence_tests::rule_13_holds_over_random_scripts`):**
  (1) a passive exit waits on its OCO's cancel confirmation with no bound and no alert: a broker
  that never confirms holds the exit for good, with the protection still resting (rule 13's broker
  hold, so no risk is added, but rule 3 wants the wait bounded). Bound it as rule 5's wait is
  (`unknown_absent_window_s`, then query the order and alert), with a test. (2) `release_held`
  journals only an allow, so when a held exit's cause changes (paused, then nothing to price; or
  an allow that would now over-sell beside other exits) the journal's last verdict names a cause
  that has gone; journal a changed hold reason once. (3) A later rung is sized from the fold's
  filled quantity when the step's cancel is confirmed; a fill the broker reports after that
  confirmation (`LateFillApplied`) is not netted, so the rung could sell more than the position
  by that fill (rule 12). The same lag sizes §5.4's re-placement. Size from the broker's own
  report of the cancelled order, or hold the rung for it, before slice 2 lets protected
  positions exist outside the tests (DEC-160 (4)).
- ~~**E7-4 slice 7's tests correction, before the agent-scoped kill switch's implementation can pass `xtask ci pending`
  (stream K, [DEC-485](decisions/DEC-485.md))**~~ Done by [DEC-506](decisions/DEC-506.md) (#671, #672, #673): nine
  corrected properties and both session hand tests go live with #668; E1, E2 and the confirmed owner's pre-market
  pricing stay pending in `BEHAVIOUR_ONLY_TESTS` (DEC-485 items 11 and 17). The record as written before it: with the
  switch live, no property script stops at its stub any more, so every executor property marked pending runs to its
  own verdict. Those that now pass go live with the implementation (DEC-485 item 17).
  These still need a DEC-77 tests correction, or a coordinator-approved `BEHAVIOUR_ONLY_TESTS` row, because each passes
  vacuously or fails away from any stub on behaviour that predates the switch:
  - the `risk_clock` readers (the #456 round 1 row above): `no_interval_exceeds_the_limit_without_an_alert` and
    `no_submission_carries_an_intent_older_than_its_maximum_age` pass while checking nothing,
    `no_recovery_submits_without_a_confirmed_absence` would fail any resubmission's window check, and
    `every_risk_input_draft_carries_a_non_decreasing_risk_clock` fails on `FillApplied carries no risk_clock`;
  - `a_client_order_id_is_a_function_of_the_intent_id_alone` rejects every script (`OrderSubmitted` version 2 carries no
    `intent_id`; read it from the `OrderRequestRecorded` companion, DEC-446 rule 45) and aborts after 385 s;
  - `no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding`: the rule-5 carve-out in the row below, and DEC-346
    item 6's protective placement (minimal script: a protected entry partly filled, a kill switch, its cancel confirmed);
  - `a_reservation_is_never_released_before_a_terminal_state` and `folding_the_journaled_drafts_reproduces_the_live_state`
    (a broker-created leg `…-p…` in `pending_cancel`: the oracle says unreserved, the fold reserved),
    `no_resting_order_is_submitted_inside_an_unprotected_interval` (a plain opening beside the protected lead's interval),
    `protective_sell_quantity_never_exceeds_the_position_in_any_script` (an exit beside a bracket that completes), and
    `protective_sell_quantity_never_exceeds_the_position` (passes at the pending gate's seed but fails at seeds 1 and 77:
    an exit, its cancel confirmed, then fills leave CPHC protected for 1 against a position of 0):
    each minimal script has no kill switch in it, so the oracle or the behaviour needs a ruling;
  - `an_owner_exit_outside_the_session_prices_from_the_confirmed_bid`: the clock-0 row above; the suite's clock is the
    regular session, so the confirmed bid and `extended_hours` cannot be reached.
- **E7-4 (stream K), from slice 7 ([DEC-485](decisions/DEC-485.md) items 13 and 15):** raise a kill switch's close again
  after `max_unprotected_s` ended its sell; register the executor's own intent ids (`w-<record>`, `k-<switch>-<n>`)
  in journal spec §9.5's `IntentReceived`; and, with the account scope, add `KillSwitchActivated` to
  `properties::every_catalogue_event_is_interpreted_or_named`'s `INTERPRETED`.
- **E7-4 slices 5 and 6's tests correction (stream K), from #286 round 1 (minor 2):**
  `properties::no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding` counts a cancel as
  outstanding until the order is terminal, abandoned or its protection cancelled, so it would fail on
  rule 5's ruled carve-out: an exit that goes once its opening's cancel is overdue, answered or not,
  with the opening still resting ([DEC-160](04-decision-log.md#decisions) (7), (13), (18)).
  Carve that case out through a DEC-77 tests correction before slice 5 or 6 lets the property run.
  Done by E7-4 slice 7's tests correction ([DEC-506](decisions/DEC-506.md) item 7, stack 2 of 3).
- **E7-4 (stream K), E1 from E7-4 slice 7's tests correction ([DEC-506](decisions/DEC-506.md)):
  an exit is submitted beside a just-activated bracket's legs, leaving protection above the
  position.** Minimal script (`properties::protective_sell_quantity_never_exceeds_the_position_in_any_script`):
  `Intent 0 (AAPL open), Acknowledge, Fill, Intent 1 (CPHC protected open), Acknowledge, Fill,
  Intent 2 (CPHC risk exit), Fill, Fill` (a CPHC bracket of 2 partly filled at 1; a risk exit of 1; the
  entry's remainder fills before its cancel confirms). In that step the executor journals
  `OrderSubmitted` for the exit and then `ProtectionChanged placed` for the legs (qty 2); once the
  exit fills, the position is 1 and the resting protection 2, and nothing later corrects it, so a
  triggered stop would sell short. Trading spec §5.4 (the tranche model's Σ protective sell
  quantity ≤ position, and the marketable exit sequence: cancel every protective order, confirm,
  re-gate, submit) and `AGENTS.md` rule 12. Fix tests-first: a bracket entry with fills whose legs
  are not yet recorded `placed` counts as live protection for any exit sequence in the instrument,
  so the exit waits a step and cancels the legs through §5.4 before it submits, as DEC-485 item 16
  does for the flatten. Until then that property fails on #668's code.
  The same root cause, held legs counted by no cap, reaches protection sizing too
  ([DEC-521](decisions/DEC-521.md) item 2, #689's review): `re_place` (reached from `settle` →
  `replace` after an exit sequence, and from `new_day`'s re-placement before expiry) sizes on
  `long`, which includes a working bracket's ingested partial fills, while `covered` excludes its
  held legs. Then a later OCO for another entry, placed while that bracket's cancel is unconfirmed
  (DEC-346 item 6), oversells once the bracket completes: with B = 10 shares outside two entries, a
  re-placement over the first entry's f1 = 1 covers 11; the second entry ends with f2 = 1 and its
  OCO is 1; the first completes (Q1 = 2) and its legs activate, so sells are 11 + 1 + 2 = 14 against
  a position of 13. The fix counts held legs, sized to their entry, in every cap and sizing,
  re-placement included, and pins the arithmetic with a hand case.
  Two more paths size the same way (#689's round-2 review): `passive_exit`'s rest OCO, reachable
  without E5 whenever another agent's bracket is working, and `re_cover` → `re_place`. The
  re-placement alone oversells by f1 once the bracket completes, with no second entry: it covers
  B + f1, and the bracket's legs then sell Q1 against a position of B + Q1. E1's tests
  ([DEC-532](decisions/DEC-532.md)) pin all four paths as pending hand cases under
  `BEHAVIOUR_ONLY_TESTS` rows, each with the protected 10 and a bracket holding 4 of 10:
  `an_exits_re_placement_leaves_a_held_brackets_shares_to_its_legs` (8, not 12),
  `a_re_placement_before_expiry_leaves_a_held_brackets_shares_to_its_legs` (10, not 14),
  `a_passive_exits_rest_leaves_a_held_brackets_shares_to_its_legs` (7, not 11) and
  `a_re_cover_leaves_a_held_brackets_shares_to_its_legs` (2, not 6). The fix sizes each path on
  the position less every working bracket entry's filled quantity whose legs are held, deletes
  the rows and the `#[ignore]` lines, and covers the original exit-beside-activated-legs case above.
- **E7-4 (stream K), E2 from E7-4 slice 7's tests correction ([DEC-506](decisions/DEC-506.md)
  item 8): an opening rests inside an unprotected interval.** Minimal script
  (`properties::no_resting_order_is_submitted_inside_an_unprotected_interval`):
  `Intent 0 (AAPL open), Acknowledge, Fill, Intent 1 (CPHC protected open), Acknowledge, Fill, Fill,
  Intent 2 (CPHC risk exit), Cancelled, Intent 3 (CPHC open)` (a completed CPHC bracket, a risk exit whose protection's
  cancel is confirmed, then a plain opening of 1 at 150 against a 150.2 ask, submitted while the
  protection is cancelled). Trading spec §5.4: "Orders submitted while protection is canceled must
  be marketable at submission"; §5.6. The coordinator's ruling on this item's scope: the sentence
  covers every order submitted in the instrument while its protection is cancelled for a sequence,
  not only the sequence's own orders, which tightens the rule and adds no risk (DEC-176). Fix
  tests-first: the gate holds such an opening until protection is placed again, never repricing
  it, with a hold reason a DEC names (§9.1); exits are untouched. Until then that property fails on
  #668's code.
- **E7-4 (stream K), E4 from E7-4 slice 7's second tests correction
  ([DEC-521](decisions/DEC-521.md) item 3): a second bracket's end closes the first bracket's
  unprotected interval, so the bound alerts late.** Minimal script
  (`properties::no_interval_exceeds_the_limit_without_an_alert`, seed 101):
  `Intent 0 (AAPL open), Acknowledge, Fill, Intent 1 (CPHC protected open), Acknowledge, Fill,
  Intent 2 (CPHC protected open), Fill, Fill, Wait, Intent 0, Intent 0, Cancelled, Intent 0`
  (bracket 01 partly filled at 34; bracket 02 partly filled at 42 and complete at 46, its legs
  placed; 01's share is still unprotected at 96 with no alert). The fold's `unprotected_end` ends the
  first open interval in the instrument and ignores the `bracket` it names, so 02's end closes 01's
  interval and leaves 02's open; `bound` measures from 42, not 34. Trading spec §5.4 ("Bounded
  unprotected intervals": every interval is journaled from start to end and alerts at
  `max_unprotected_s`). The acknowledgment path has the same defect: a partly filled second
  bracket's OCO ends its interval at the broker's acknowledgment (DEC-348 item 2), with an
  `unprotected_end` that names no bracket, and that arm too ends the first open interval. Fix
  tests-first, as its own PR: `UnprotectedInterval` carries the bracket entry its start names; an
  end that names a bracket, and the acknowledgment of the OCO placed for one, close only that
  bracket's interval. The cases are `hand::a_second_brackets_end_leaves_the_first_brackets_interval_bounded`
  and `hand::an_acknowledged_oco_for_a_second_bracket_leaves_the_first_brackets_interval_bounded`,
  which the fix takes live with their `BEHAVIOUR_ONLY_TESTS` rows; the property goes live with #668.
  Done by E4's fix: each interval carries its bracket entry, and both the end that names a bracket
  and the acknowledgment of the OCO placed for one close only that bracket's interval.
- **E7-4 (stream K), E4b from E4's fix ([DEC-521](decisions/DEC-521.md) item 3, #698's review):
  while a bracket's OCO awaits its acknowledgment, a new interval's start ends the first open
  interval in the instrument, not the awaited one.** The fold's `unprotected_start` arm ends an
  open interval when an acknowledgment is awaited, but picks the first open one in the instrument.
  Hand case `hand::a_new_brackets_start_ends_the_awaited_interval_not_the_first_brackets`: bracket 1
  partly fills at 10; bracket 2 partly fills at 20 and is cancelled at 30, so its OCO is awaited;
  bracket 3 partly fills at 32 and ends bracket 1's interval, so nothing alerts at 70. Trading spec
  §5.4 ("Bounded unprotected intervals"). Fix tests-first: key that arm, like E4's acknowledged
  end, to the bracket whose awaited OCO it supersedes (`protected_entry()`), then delete the case's
  `BEHAVIOUR_ONLY_TESTS` row and `#[ignore]` line. Minors from the same review, for the same PR or
  the backlog: `interval_limit` marks the first open interval alerted, which can re-alert a later
  one each tick; the fold's doc for these arms; and `awaiting.insert` replaces an instrument's
  awaited set rather than adding to it. #771's review added two tests before the fix: a hand case
  with the awaited interval between two others (`a_new_start_ends_the_awaited_middle_interval_only`)
  and a property lead where the awaited interval is the first open one and another is the latest
  (`AWAITED_FIRST_LEAD`), so a fix that ends the latest open interval fails both. Done by E4b's
  fix: that arm ends the open interval of a bracket whose OCO's acknowledgment was awaited (or an
  unbracketed one), never another bracket's; `no_interval_exceeds_the_limit_without_an_alert` and
  both hand cases are live again. The three minors stay open, with a fourth from that review: the
  fix's match lets an unbracketed open interval (an exit's or a re-placement's) be ended for any
  awaited bracket OCO; no script here has reached it yet, so pin it with a case if one does.
- **E7-4 (stream K), E5 from E7-4 slice 7's second tests correction
  ([DEC-521](decisions/DEC-521.md) item 4): an overdue cancel of a bracket entry ends an exit's wait
  while no cap sees that entry's legs (the coordinator rules on it with E1 and E2).** Script, on
  #668's head: `Intent 0 (AAPL open), Acknowledge, Fill, Intent 1 (CPHC protected open),
  Acknowledge, Fill, Intent 3 (CPHC risk exit), Intent 2 (CPHC protected open), Acknowledge, Fill,
  Wait, Cancelled, Cancelled`. At 84 both entries' cancels are journaled `cancel_overdue`, and the
  exit of 1 is submitted, sized on their fills, while both still rest at the broker
  (DEC-160 (7), (13), (18)). At 88 02's cancel is confirmed and its OCO placed for 1 (room 2 − 1,
  DEC-346 item 6). If 01's last share then fills, the broker activates its legs for 2: sells of
  1 + 1 + 2 against a position of 3, so a short of 1 is possible; a whole-position exit gets there
  with no OCO. This is arithmetic: the harness cannot fill 01 behind the newer orders. Trading spec
  §5.4 (Σ protective sells ≤ position) and `AGENTS.md` rule 12.
- **E7-4 (stream K), E5b from #668's round-2 review: the exit ladder steps a rung the broker has
  not acknowledged, and another exit goes beside that cancel.** Script, the same on `main`
  (`properties::no_order_is_submitted_while_an_unconfirmed_cancel_is_outstanding`, seed 209):
  `Intent 0 (AAPL open), Acknowledge, Fill, Intent 1 (CPHC protected open), Acknowledge, Fill, Fill,
  Intent 3 (CPHC risk exit), Cancelled, Cancelled, Intent 2 (CPHC risk exit)`. At 46 the OCO's
  cancel is confirmed and exit 03 is submitted; the second `Cancelled` re-delivers the OCO's
  `canceled` (journaled `ignored`); at 54, 03 still `submitting`, its ladder step asks
  `Cancel{03}` (`ladder_step`, `attempted: pending_cancel`, `ignored`), a cancel of an order the
  broker has not acknowledged, and exit 02 is submitted in the same step while that cancel is
  outstanding. CI's shard-81 script on #668 (`Intent 0, Acknowledge, Fill, Intent 1 (CPHC
  protected), Acknowledge, Fill, Fill, Intent 2 (CPHC exit), Cancelled, Intent 0, Intent 3 (CPHC
  exit)`) is the same shape. No short follows from it alone: the gate sizes 02 on the position less
  every live sell, 03 included (`available`). Trading spec §5.6 (a rung steps by cancel, confirm,
  resubmit) and §5.7 (an order the broker has not acknowledged is queried, never cancelled blind);
  `AGENTS.md` rule 13. Decide whether the ladder may step an unacknowledged rung, and whether another
  exit may go beside a rung's step cancel, then fix tests-first; until then the property is pending
  under a `BEHAVIOUR_ONLY_TESTS` row. To be fixed before any real-money run.
- **E7-4 (stream K), from E7-4 slice 7's tests correction: an unconfirmed owner exit pre-market
  cancels protection for a sell that cannot fill before 09:30 (open question for the
  coordinator).** At 2026-09-22 08:00 ET an owner kill switch without confirmation cancels the
  resting OCO and, once that is confirmed, submits its close as a regular-session limit the broker
  queues to the open (DEC-260 (13)). The position is then unprotected until the open, and §5.4's
  `max_unprotected_s` bound would end the queued close well before 09:30. Trading spec §5.5
  ("Without confirmation, equity sells wait for the session") says nothing about protection here,
  where it says protection stays in place for an automated switch. Decide whether an unconfirmed
  owner close outside the session leaves protection resting until the session, as the automated
  one does.
- **Trading-domain spec §5.7's missing `PendingCancel` edges (stream K), from #286 round 2
  (minor 3):** a spec PR that adds `PendingCancel → Expired` and `PendingCancel → Rejected` to
  §5.7's transition table, with reference cases, and then the executor change that follows it.
  Until then such a report is journaled and ignored ([DEC-160](04-decision-log.md#decisions)
  (13)), so an exit waiting on the order goes at rule 5's bound (item (18)), but the residue
  stays: a day order that expired, or was rejected, while its cancel was outstanding is left in
  `PendingCancel` and keeps its buying-power reservation until reconciliation or a later report
  moves it.
- **`mandate-executor`, from #286 round 2 (nit 1):** marking an accepted order's cancel overdue
  (`protection::overdue`) journals a self-transition, which `orders::transition` records as an
  attempted edge with `ignored: true` so the fold applies only `cancel_overdue`. The record reads
  as a refused transition. A dedicated flag-only record needs the fold's `Unknown` branch
  (which resets `unknown_since` and the absence count) kept out of it, so it is not a one-line
  change; do it with the §5.7 edges above.
- **`mandate-executor` fees (stream K), from #259 round 1:** (1) a typed `Environment` in place of
  the stream's environment text, so `paper_only_fee_config` refuses a live stream by its type
  (rung 1) rather than by a string comparison; (2) `mandate_accounting::Config` carries the
  schedule's `effective_from` and refuses to price a trade date before it, where today
  `fee_config` validates the date only against the calendar's range and then drops it;
  (3) the ruling's flat per-order cost for the paper-only overestimate (#174, 5861904579) cannot be
  represented, because `EquityFees` has no per-order field, so the overestimate is carried by the
  per-share and rate figures, each at least ten times the transcribed schedule.
- **E7-4:** gate `mandate-executor`'s `resubmit` for an order with no `intent_id`. It sends again without running the gate; no slice through 6 writes such an order, but protective orders will, so it must be gated before they ship (#202 review, the coordinator's ruling, comment 5857629810).
- Fold `crypto_status` in `mandate-executor`. `AccountStateObserved` journals it, and §7.3 requires it `ACTIVE` for crypto orders, but the fold keeps no field for it until the gate's crypto check reads one; the journal holds it, so the fold can add it without a new event (#198 review, round 1, finding 8a).
- Give a §7.3 account restriction in `mandate-executor` a lift path. §7.3 says a detected
  restriction stands "until the owner acknowledges and the account is refreshed", but
  `state::restriction_for` only yields `reconciliation:{subject}`, so no `OwnerAcknowledged` can
  name `account_trading_blocked`, and a later `ACTIVE` read leaves `account_state` `Blocked` and the
  restriction in place. Pre-existing on main; the cash slice is the first to raise it from every
  reconciliation run (#205 review, round 2, minor 3).
- Pin or drop the two unreachable overflow sites in `ExecutorState::buying_power`: the reservations sum and the final `min(model, broker) − reserved`. `Usd` is signed, so each fails only at the decimal range, which no reservation reaches, and replacing either `None` with zero passes every test; the reachable site, the model's cash, is pinned (#198 review, round 2, finding 3).
- **Blocks running the executor across a session boundary:** fold `RiskDayStarted` in
  `mandate-executor`. `TradingDayStarted` is folded since E7-4 slice 5's trading-day part
  ([DEC-367](decisions/DEC-367.md)). `RiskDayStarted` still answers the later slice's
  `Unimplemented` stub, so the first risk-day rollover stops the executor, failing closed. Interpret
  it, move it back into `properties::INTERPRETED` with a live test that fails when its arm is
  stubbed, and land that before the executor runs across a session boundary (#194 review, round 2).
- Report a safety-critical function whose only mutants are unviable. `ci mutants` counted the one
  mutant of `mandate-executor`'s `every_agent` (a body of `Ok(Default::default())`, which does not
  compile because `EventId` has no `Default`) as unviable, so "0 missed" said nothing about the
  function that enforces §7.1's `exits_only`. Such a function needs a named live test, and the gate
  or the review checklist should list every function whose mutants are all unviable, so a reviewer
  names that test (#196 review, round 1, finding 2).
- Question for the spec owner: does §10's simulated regulatory fee apply to an **unattributed**
  paper equity fill (external activity, §7.1)? §10 books paper's regulatory fees in the shadow
  ledger without distinguishing, and an external fill has no client order for DEC-87's per-order
  TAF cap. Since #196 the executor books one only for a fill attributed to one of its orders,
  stated in `orders::simulated_fee`'s doc; booking it too would lower paper buying power, the
  conservative side (#196 review, round 1, finding 7).
- Pin the calendar roll of the simulated `FeesCharged` event's `day` in `mandate-executor`. The
  hour cutoff is pinned (a fill at 20:00 New York belongs to the next trade date), but every fixture
  fill trades on a Tuesday, so `first_on_or_after(date, is_trading_day)` is the identity and a
  `day` taken straight from the broker's `trade_date`, bypassing the account's calendar, passes the
  suite. A fixture with a Saturday or holiday `trade_date` pins it (#196 review, round 2, finding 1).
- Reconcile journal spec §9's "daily snapshot" with `AccountSnapshotRecorded`'s cadence in
  `mandate-executor`: since the cash slice every reconciliation run that has a base records one
  (at startup, at each `Unknown`, at session boundaries, at fee postings). Either the spec line
  names every run, or the executor records the daily one only and carries the comparison's base
  another way (#205 review, round 1, finding 7a).
- Give `AccountSnapshotRecorded` one shape in `mandate-executor`: a fee posting with no cash base
  records it through `fees()`'s fallback without `model_cash`, `cash_band` or `cash_in_band`,
  while a run with a base records all three (#205 review, round 1, finding 7b).
- Answer an underflow in `mandate-executor`'s `asset_fees` with a typed error: a posted crypto asset
  fee larger than the one held is folded as `checked_sub(charged).unwrap_or(Qty::ZERO)`, which errs
  strict but silently, where the crate's convention is a typed error (#205 review, round 1,
  finding 7c).
- Wire the gate's buying-power port in `mandate-executor`. `ExecutorState::buying_power` and
  `crypto_buying_power` are computed and pinned but only tests read them; the gate still checks no
  buying power. The port must read the equity row for an equity order and the crypto row for a
  crypto order, and treat `None` (no account reported, an incomplete report, or an overflow) as no
  buying power, failing closed (`AGENTS.md` rule 3; #205 review, round 1).
- Make the reproduction line `ci pending` prints runnable as printed: quote the `-E` filterset
  (it contains parentheses, so a shell refuses it) and add `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`,
  which the gate sets and nextest requires for libtest JSON. Both fail loudly today rather than
  giving a different verdict (#199 review, round 2, minor).

From the independent reviews of stream I's implementation (`mandate-runtime`, #151), each deferred by
a coordinator ruling rather than left undone (DEC-131 item 25):

- Match a kill switch's lift to the acknowledgment **of that switch**. A confirmed `RiskLimit` switch
  is lifted today by a looser account-stream copy caused by any `OwnerAcknowledged` the fold has
  seen, because `KillSwitchActivated` does not name the limit that pulled it. When it carries its
  limit, the lift must match the acknowledgment's subject to the switch (round-4 review finding 1).
- Give a kill switch pulled while the agent is already `paused` a way to lift. It writes no
  `AgentModeChanged`, so it has no `mode_event`, nothing on the account stream can confirm it, and
  only a new deployment clears it. It fails closed, but two limits in one cycle then keep the agent
  paused until a redeploy (round-4 review finding 2).
- Shrink `outstanding` on a terminal account-stream outcome (`GateDecided` denial, `OrderAbandoned`,
  a terminal order state), which needs the executor's `client_order_id`-to-intent mapping from M6.
  Until then a restart after any order waits one clean `ReconciliationRun` before the agent can trade
  or propose a discretionary exit (DEC-131 item 25(d), round-2 review minor 3).
- Record an approval response that arrives in the same step as the tightening which cancels its
  approval as `refused` rather than `recorded`; nothing is handed either way, so it is record
  accuracy (DEC-131 item 25(j), round-1 review finding 9).
- Act on a granted ASK. `PendingApproval` binds the instrument, version, deadline, and whether the
  action adds risk, and carries no order body, so the runtime records a response and re-proposes at
  the next evaluation rather than placing the bound order; the bound content is M7's (DEC-131 item
  25(a)).

From the independent review of stream G's mandate limits (`mandate-risk`, #160):

- Give `Computed` the account's own figures (`account_gross`, `account_equity`), so an account-1×
  `gross_exposure_limit` denial journals the figures it compared; today it carries none and is told
  apart from the agent-limit denial only by `computed.gross` being unset. It is an addition to
  #136's public API, so it needs its own ruling.
- Settle what DEC-129 item 2's "`computed` blocks included" means: `ref.py`'s keys only, or every
  figure the gate computed. The flat `Computed` also reports `order_usd`, `instrument_total` and
  `cap` on branches whose reference block omits them, and `compare_computed` checks only the keys a
  case states, so nothing asserts either reading.
- Give §3.3's "organization ceiling" on the per-instrument cap a `GateConfig` field and an owning
  story; check 2 has no value to bound the mandate's cap with today.
- Pin that the **agent's** gross exposure counts only its own working openings
  (`limits.rs`'s `working_cost(input, None, false)`): the substitution `false` → `true` survives
  every live test (#160 round-2 finding 4). It is conservative, since the whole-account sum is a
  superset and can only deny an opening earlier, so it is not a rule-1 breach; one assertion in
  `the_account_bound_counts_other_agents_working_orders` would close it.
- Decide what `agent_flatten` does when `max_exit_offset = 1` (#176 round-1 nit 3): the floor
  `bid × 0` fails `positive()`, so the owner's kill switch returns `GateError` where `ref.py` plans a
  floor of `0`, the one way the function can refuse an owner kill switch (AGENTS.md rule 13, "the
  kill switch is always available"). Unreachable today, since §5.6's fixed tier table sets the
  offset at 0.03 or 0.05 and no code path sets it to 1; a fix either bounds the offset below 1
  where it is configured or plans the flatten without a floor.
- Correct the E6-3 brief's test names (#176 round-1 nit 4): the clause table (line 583) and mutant
  row 33 name `properties::an_agent_flatten_never_touches_another_agent`, which lives in `hand.rs`.
- Delete `crates/mandate-risk/tests/refcases.rs` once the families G and F status rows (DEC-178)
  have merged: `crates/mandate-refcases/src/mandate/risk_gate.rs` now runs MC-G01 to MC-G16 and
  MC-F01 to MC-F04 against the same `evaluate` and `agent_flatten`, and the file's own doc says it
  moves there. Its doc on `FULL_GATE_ONLY` still calls MC-G13 pending on E6-8, which #311 made
  live; deleting the file retires that too, and moving any figure it pins that
  `crates/mandate-risk/tests/hand.rs` does not goes in the same tests correction.
- Reconcile MC-G02 with its header under DEC-176: the header says "every other check passes", yet
  its working opening order in the proposal's own instrument trips trading-domain §5.3 rule 6
  (`working_order_limit`), which is why [DEC-150](04-decision-log.md#decisions) item 1 lists it on
  `FULL_GATE_ONLY`. Stating the same $1,300 as a position (`positions_mv` 1300, no working order in
  that instrument) keeps `instrument_total` and `gross` at 1500, so the case would pin `gross` on an
  allow path, the window DEC-150 records, and the entry could expire. Restated, MC-G02 becomes an
  allowed opening (an increase), so like MC-G13 it runs through checks 5 and 6 and check 7, and the
  full gate allows it: against the restated state the harness reports the entry expired
  (DEC-178 item 11). The same pull request deletes the entry, and `FULL_GATE_ONLY` with it if
  nothing else is listed. The change touches the YAML,
  the reference implementation's checks, and the regenerated fixtures, so it cannot share a pull
  request with code (ES-22).
- Done in the families G and F harness tightening ([#348](https://github.com/kunwarshivam/mandate/pull/348);
  `an_allowed_exit_the_gate_paces_fails_since_no_case_states_a_pacing`,
  `an_allow_fails_unless_it_reports_every_check_passed_in_order`; DEC-178 items 11, 12 and 14):
  the gate arm takes `Decision` apart whole and requires no `pacing` on every `gate` case, so the
  always-marketable `pacing` below now fails MC-G08 to MC-G10, MC-G13 and MC-G15; item 12 says why
  the five values are typed again; the allow pin has its own edit test and item 11 says it holds
  only for an allowed opening (checks 2 and 5 to 8 are not run for an exit); and group ids are built
  from the case's own names, so the unreachable rank error is gone. The row as it was:
  Tighten the families G and F harness (#317 re-review, minor 2 and nits 1 to 4), in one tests
  correction of `crates/mandate-refcases/src/mandate/risk_gate.rs` and DEC-178:
  - compare `pacing` as `None` on every allowed `gate` case and destructure `Decision`, so a new
    member does not compile until it is compared; today a `pacing` that always sets
    `marketable_limit_required` leaves every F, G and L case green, and only `mandate-risk`'s own
    tests catch it (DEC-178 item 14);
  - reword DEC-178 item 12: the five non-fixture fields are compared with the same values typed
    again, because `mandate-risk`'s `test_default_config` is test-only and another crate cannot
    call it;
  - add an edit test for item 11's pin on an allowed order's whole `checks` list, which today no
    harness test of its own guards;
  - note in item 11 that the pin is only meaningful for an allowed opening (MC-G13), since the gate
    reports checks 5 to 8 as `Passed` for an exit without running them;
  - drop the unreachable typed error for an unknown group rank in `Scene::read`, or state why it
    stays.
- Tighten the family-B harness (#331 review, minors 4 and 5 and the nits), in one tests correction
  of `crates/mandate-refcases/src/mandate/order_builder.rs` and DEC-250. *Done (E6-2,
  [#346](https://github.com/kunwarshivam/mandate/pull/346); DEC-250 items 16 to 18), all but the shared gate helpers,
  which stay open:*
  - *done:* `an_unreadable_input_is_refused_naming_it` now also sweeps the `gate_state.positions_mv`
    map and its values, `gate_state.last_exit_fill_at` (whole, and a planted entry, since no case
    states one), `gate_state.working_universe` and its entries, and each working order's
    `instrument` and `max_cost`, in the gate state and restated (minor 4);
  - *done:* a cash fee rate above zero is refused for crypto as for equities, since the gate's
    `fee_reservation` is `Usd::ZERO`; a stated 0 is read and passes (minor 5, DEC-250 item 17);
  - *done:* `crates/mandate-builder/tests/refcases.rs`'s module doc says this crate's harness hands
    `decide` the case's verdict because it does not depend on `mandate-risk`, and points at the
    shared harness's propose, gate and `decide` composition (nit);
  - **open:** `test_default_gate_config` and `gate_mandate` are a third copy of the gate helpers,
    beside `risk_gate.rs` and `trading_domain/gate.rs`. Share them, and when they are shared, add
    DEC-178 item 12's check against `configs.test_default.gate` to the family-B arm (nit);
  - *done:* `INPUT_KEYS` keeps `fee_rate_cash`, which `a_cash_fee_rate_above_zero_is_refused` shows
    read, and drops `drawdown`, `daily_pnl_fraction` and `bought_today_usd`, which no base's
    autonomy rule reads, until a case states one (nit, DEC-250 item 16).
- **Family B's three contradicting clocks, as a reference-case PR under DEC-176** (DEC-250 item 12,
  #331 review). MC-B22 (`session: after_hours`), MC-B23 (`in_close_window: true`) and MC-B31
  (`session: after_hours`) all put `now` at 2026-09-22T14:00Z, the regular session, so the harness
  fails them naming the conflict. Move `now`, and each output's `as_of` and `expires_at` with it, to
  21:00Z for MC-B22, 19:55Z for MC-B23, and an after-hours instant for MC-B31. No expectation
  changes, and each case then tests the condition its title claims;
  `the_two_session_cases_pass_once_now_agrees_with_their_labels` already shows MC-B22 and MC-B23
  pass so moved on unmodified code, and their status rows follow. The YAML,
  `cargo xtask refcases --write`, and the reference checks land together, apart from code (ES-22).
  Whether `session` and `in_close_window` stay case-file inputs at all is a separate question,
  Proposed to the founder in DEC-250.
  *Done (DEC-250's 2026-09-30 amendment):* `generate.py` moves the three clocks and keeps each
  output's offsets from `now`, no expectation changes, and `check_cases.py` now fails any builder
  case whose `session` or `in_close_window` contradicts its `now`. MC-B22 and MC-B23 pass, and their
  status rows are a separate status-only PR. MC-B31 still fails at `mandate_risk::trim_proposals`
  (pending E6-4). The founder question above stays Proposed.
- **Tests correction for family B's moved clocks** (DEC-250's 2026-09-30 amendment; crate code, so
  not in the reference-case PR, ES-22). `crates/mandate-refcases/src/mandate/order_builder.rs`.
  *Done (`cursor/family-b-clock-owed-tests-v2-138b`, which lands before #347):* moving MC-B22 and
  MC-B23 from `OWED` to `PASSING` would be red on the fixture before #347, so a `CLOCKED` table
  judges them by the fixture's own consistency instead. When a case's `session` and
  `in_close_window` agree with `mandate_risk::session_at` at its `now`, it must pass; otherwise it
  must fail naming the conflict, as before. The test compares the labels itself, so a harness that
  stops comparing them is still caught. MC-B31 stays in `OWED`: a `trim_to_target` case stops at
  `trim_proposals` before its labels are compared. `the_two_session_cases_pass_once_now_agrees_with_their_labels`
  is kept, because on the fixture before #347 its clock moves are what give its sibling-swap proof
  (the only one for a sell's `order_type` and for a deferred outcome) a case that passes.
- Fold the clocked cases back once #347 and #346 have both merged (the tests correction above,
  follow-up). With the clocks moved, `labels_agree_with_calendar` is true for both cases, so:
  - move MC-B22 and MC-B23 into `PASSING`, where `every_expected_member_is_compared_and_required`
    also sweeps them, and delete `CLOCKED` and `labels_agree_with_calendar`;
  - drop `the_two_session_cases_pass_once_now_agrees_with_their_labels`'s clock doctoring, which
    re-sets MC-B22's clock to the values it already holds and changes only MC-B23's `expires_at`,
    and run its sibling-swap proof on the two cases as the fixture states them;
  - reword that test's doc, which says the two cases fail only because their labels contradict their
    clock. DEC-250 item 3's label-against-clock comparison stays proved by
    `the_builder_and_the_gate_see_one_scene`'s doctored MC-B01.

  *Done (E6-2, [#359](https://github.com/kunwarshivam/mandate/pull/359); DEC-250's 2026-09-30 amendment):* MC-B22 and
  MC-B23 are in `PASSING`, `CLOCKED` and `labels_agree_with_calendar` are gone, and the renamed
  `the_two_session_cases_pass_as_stated_and_compare_every_sibling` runs its sibling swap on the
  fixture as stated. The sweeps' counts, recomputed from the fixture, grow with the two cases (768
  doctorings, 150 plants, 981 refused inputs). With the harness's label comparison skipped,
  `the_builder_and_the_gate_see_one_scene` fails on MC-B01's doctored `session`.
- Derive the family-B sibling counts (#331 round-2 review, nit). *Done (E6-2,
  [#346](https://github.com/kunwarshivam/mandate/pull/346); DEC-250 item 18):* the hand-written `siblings == 87` and
  `siblings == 11` are replaced by an assertion that every enum-valued expectation a swept case
  states (a word, a null, or a non-empty list of words in some family-B case) has a `sibling` arm,
  `on_timeout` and `action` excepted, so a case that gains or loses an enum expectation needs no
  count edit.
- Give the family-B sibling classifier its own oracle, in a tests correction of
  `crates/mandate-refcases/src/mandate/order_builder.rs` (#346 review, minor and nits):
  - `every_sibling_fails_its_comparison` trusts `is_enum_value` with nothing checking it: blinding
    the classifier to `false` and dropping a `sibling` arm together leaves the suite green, which
    the old `siblings == 87` would have caught. Pin the enum-valued member count the fixture yields
    (12 today) beside the derived assertion, as the file already pins 899 and 698 (minor);
  - `NO_SIBLING` exempts the bare member names `on_timeout` and `action`, so it would also exempt
    a future `/expect/gate_dry_run/action`. Key it on the `(pointer, member)` pair, as
    `enum_valued` is (nit);
  - reading `gate_state.working_universe` reports a missing member as "is not a list"; say "fixture
    has no" for a missing one, as `list_at` did (nit);
  - DEC-250 item 15's ES-13 exception records 1,338 non-test `src` lines; the file has 1,352 since
    #346. Refresh the figure or state that the exception is not tied to an exact count (nit);
  - DEC-250 item 18 names `origin` and `trim_withheld` as the enum-valued members the trim cases
    will need `sibling` arms for; MC-B17's `reason: trim_to_target` is a third, not in
    `HOLD_REASONS`, so the guard fires when E6-4 turns MC-B17 green (nit; it fails safe).
- **Settle what `safety_critical = true` means for a `tool`-layer crate** (#331 round-2 review, for
  the founder's after-the-fact look). `xtask/layers.toml` marks `mandate-refcases`
  `safety_critical = true`, and CODEOWNERS lists it, but two checks read it as not safety-critical:
  - `cargo xtask ci mutants` skips it, because `mutated_crates` requires a `Product` layer;
  - DEC-250 item 15 applied ES-13's 800-line limit for crates outside the safety-critical list,
    not the 400 the flag implies.

  Neither changed #331's outcome, but a harness change could land with no mutation gate while its
  entry claims otherwise. The conservative reading is that the flag governs: `ci mutants` also
  mutates safety-critical `tool` crates on the diff, and ES-13's safety-critical limit applies,
  with DEC-178's `risk_gate.rs` and DEC-250's `order_builder.rs` as recorded exceptions or split.
  Record the reading in a decision (a file under `decisions/`, DEC-344) in the same change as
  the xtask edit.
  *Done (DEC-253):* the flag governs. `mutated_crates` reads `safety_critical` alone, pinned by
  `the_gate_mutates_every_safety_critical_crate_tool_layer_included`; ADR-0001's ES-11 and ES-13
  say so, and ES-13 records `risk_gate.rs` and `order_builder.rs` as exceptions, not split.
- `parse_status` returning an empty map survives every test (DEC-253's proof run: `cargo xtask ci
  mutants` on a diff touching it missed `replace parse_status -> Result<BTreeMap<String,
  CaseStatus>, String> with Ok(BTreeMap::new())`). `tests/refcases.rs` treats a case the map does
  not name as pending and ignores it, so a harness that read no status would run no case and pass,
  and ES-11's "a passing case never regresses" would hold vacuously. Add a live test in
  `crates/mandate-refcases/src/lib.rs` that parses a two-suite sample and asserts the whole map,
  and one that reading the committed `status.toml` yields at least one passing case. Until then
  the mutation gate fails any change whose diff touches `parse_status`.
- Family B's harness arm has 13 mutants no live test catches (DEC-253's timing run of the gate over
  DEC-250's diff of `crates/mandate-refcases/src/mandate/order_builder.rs`: 79 mutants, 40 caught,
  26 unviable, 13 missed). `scaling_rung` (five: every return value, its `==`, its `&&`) is reached
  only by the trim cases, which fail pending E6-4, so no live test runs it; `judge`'s
  `nothing_proposed` guard can be `true`; `Inputs::read`'s `has_prior_fill` default can lose its `!`
  (DEC-250 item 13: no base rule reads the flag); `listing`'s
  `qty_increment < 1` (fractionable) can be `==`, `>` or `<=`; and `builder_error` can return any
  string or flip its `==`. One tests correction of the arm adds doctored-case tests in
  `crates/mandate-refcases/tests/`, as `mandate_gate_harness.rs` does for families G and F, so each
  survivor is caught; until then the gate fails any change whose diff touches those lines.
- **RC-22 and RC-25, blocked in the trading-domain harness** (E6-8's implementation PR, DEC-163;
  the gate driver since E6-9, DEC-199). `crates/mandate-refcases/src/trading_domain/gate.rs` now
  decides `propose_order` steps with `mandate_risk::evaluate`, states the market data a case omits
  (a quote at the limit price, volumes that pass check 6) and the origin each purpose maps to, each
  with its own tests. RC-25 still waits for the instrument fields `prior_close` and
  `median_dollar_volume_20d` (E6-7's rows in the harness) and for `owner_confirmed_bid`, which the
  header writes as `true` and the gate takes as a price (DEC-199 Q3). RC-22 needs more:
  `broker_order_update` (E7-2), the exit sequence's `actions` (E7-4), and the `conduct_breach`
  step's switch to `exits_only` with an owner alert, a mode transition the runtime owns (§5.9,
  §9.6's "breach → agent `exits_only`", E6-11); the pure gate only denies the breaching opening
  `conduct_limit_breached`. RC-16 also needs the case file's `not_in_universe` reconciled with the
  gate's `not_in_working_universe` (DEC-199 Q1). All three state more than one proposal, so each
  also waits for E7-4 and E7-5 (DEC-199 item 3).
- Done in the trading-domain gate driver tightening ([#349](https://github.com/kunwarshivam/mandate/pull/349);
  `the_listing_and_market_are_dec_199_item_6s`,
  `the_filled_median_is_the_liquid_collar_tier_to_the_cent`,
  `the_filled_trailing_volume_is_the_order_size_cap_to_the_share`,
  `the_filled_minimum_order_is_one_share_to_the_last_place`,
  `a_second_proposal_in_one_case_waits_for_e7_4_and_e7_5`,
  `an_agent_mode_where_nothing_moves_it_names_the_account_ledger`; DEC-199 items 3, 6 and 12):
  every listing and market value is pinned, and the median, the trailing volume and the minimum
  order are shown deciding at their edge, the ADV as at least 1,000,000 (its own value cannot bind
  before the order cap); a case with a second proposal fails naming E7-4 and E7-5; and an
  `agent_mode` after an event `MODE_OWNERS` does not list fails naming E7-5 (`MODE_HOLDER`). The
  row as it was:
  Tighten the trading-domain gate driver (#333 review, minors 1 and 2 and nit 2), in one tests
  correction of `crates/mandate-refcases/src/trading_domain/gate.rs` and DEC-199:
  - pin, or better, show taking effect, the values DEC-199 item 6 fills: `median_dollar_volume_20d`
    (the collar tier), `min_order_size`, and the two participation volumes. Today changing any of
    them leaves every test green, although none of the three changes is looser than the spec;
  - refuse a second `propose_order` step in the same case until E7-4 and E7-5 land. DEC-199 item 3
    decides each proposal alone, against an account with no working, unknown or related resting
    orders, and nothing fails if a case adds a second proposal;
  - add `agent_mode` to `check()`'s allowed keys, so that an `agent_mode` expectation on an event
    outside `MODE_OWNERS` fails naming its owning story rather than as `expect: unknown key`.
- The case-file side of DEC-199 Q1 to Q3, in one reference-case change for the founder:
  - RC-16 says `not_in_working_universe`, the registered code mandate spec §5.3 defines;
  - the header says the eligibility floor is checked against the limit price when no
    `prior_close` is given (or a case below $5 states its own);
  - RC-25 carries `owner_confirmed_bid` as the displayed bid's price rather than `true`.
- **trading-domain §9.6: state that the opposite-fill interval includes its last instant**
  (DEC-163 item 3; DEC-176 clarification). §9.6's "within 60 seconds after" an opposite-side fill
  is read inclusively, so an opening exactly 60 s after the fill is denied; the spec text should
  say so, as a clarification that tightens nothing the code does not already enforce.
- **`Ratio` to `Fraction` in `mandate-num`.** The surveillance report turns a concentration
  `Ratio` into a `Fraction` by printing and re-parsing it (`surveillance.rs`'s `share_of_equity`),
  because `mandate-num` has no exact conversion. The text round trip is exact, but a typed
  `Fraction::try_from(Ratio)` with its own tests would retire it (DEC-163 item 8).
- **A concentration threshold for the surveillance report** (founder: compliance-visible). §9.6
  lists concentration among the report's checks and §3.3 supplies no number, so the report states
  each concentration figure and flags none; `SurveillanceBreach::Concentration` is not raised
  (DEC-163 item 8). A founder-set threshold would let it be.
- **`CancelCause` in place of `CancelInput`'s two booleans** (a follow-up story, with its own tests
  correction). `CancelInput` tells an exempt cancel by `precedes_risk_reducing_order` and a
  marketable order by `marketable`, both set by the executor. A required
  `CancelCause { RiskReducing, Replace { marketable }, Discretionary }` would make the cause
  explicit and let the journal say which (DEC-163 item 7).
- **Does a resting protective order count for self-trade prevention?** `related_account_resting`
  lists protective orders too (DEC-163 item 11), the conservative reading, so a bracket opening
  beside a resting protective sell in the same instrument is denied `conduct_limit_breached` though
  §5.3 rule 8 lets a bracket add a tranche. A stop is not in the book until triggered; deciding
  whether it counts would reopen that path.
- **E6-6 slice 2:** the account-wide fold is `mandate_risk::fold_day_trades` (DEC-259). The
  trading-domain harness reads RC-09's and RC-09B's `regime`, `prior_day_trades`, `last_equity`,
  `multiplier` and `day_trade_count` and drives the ledger through `fold_day_trades` from the
  case's fills (DEC-284), after the tests correction #408. A later `propose_order` step is decided
  once every earlier allowed one was filled in full on its side (DEC-259 item 7); the three tests
  that reading owed are in `trading_domain::gate::tests`:
  `a_later_proposal_waits_for_a_full_fill_on_its_side` (identical buys with no fill, a partial
  fill, and more), and `the_conduct_figures_are_the_fill_steps` with
  `the_opposite_fill_interval_runs_from_the_fill_step`, which catch the plant that takes the
  conduct figures from the proposal. E6-10's harness reads RC-09's crypto step (DEC-285). The
  executor (E7-3) calls the fold account-wide and journals `DayTradeFold::today`. **A fold error
  refuses openings only** (#370 review, major 1): E7-3 must still route exits and protective
  orders when `fold_day_trades` fails (`AGENTS.md` rule 13), and E7-3's tests PR carries the
  pending test `a_failed_day_trade_fold_refuses_openings_and_still_routes_exits` for it.
- **Refuse a short position in the trading-domain day-trade fold structurally** (#412 review,
  nit 3). `trading_domain::gate::Gate::fold` reads each equity position as `position.qty().abs()`,
  so a short would fold as if held long and understate the day-trade count. No v1 case holds one
  (`AGENTS.md` rule 12) and `Gate::decide` refuses a short, but a `day_trade_count` expectation
  reaches the fold without that refusal. Refuse a negative `SignedQty` there instead of taking
  its magnitude. **Tests staged** in [#438](https://github.com/kunwarshivam/mandate/pull/438)
  (DEC-314): the refusal is pending E6-10 behind `held_overnight`'s stub, and a flat (`0`)
  position, which is not a short, is pinned live as folding as no shares held. *Done (DEC-395):*
  the fold refuses with DEC-314's text, down to a billionth of a share.
- **Read `crypto_status` in the trading-domain harness** (DEC-285 item 5). The driver hands the gate
  a crypto-active account because no crypto-proposing case states `crypto_status`, and
  `trading_domain_gate_harness.rs` pins it as pending E6-10, initial and in an update. When a case
  needs it, a tests correction (DEC-77) drops those two assertions, and the harness reads it as
  check 1's `crypto_active`. **Tests staged** in [#438](https://github.com/kunwarshivam/mandate/pull/438)
  (DEC-315): the two assertions are dropped, the gate carries a `crypto_active` field its
  snapshot reads, the read is a stub pending E6-10, and two in-module tests pin the stub's report
  live until the implementation PR replaces them. *Done (DEC-395):* only an exact `ACTIVE` leaves
  crypto openings open, a non-string status is refused, and the stub pins are replaced by live
  tests that the status never refuses an equity decision or a crypto risk exit.
- **RC-09's `alpaca_intraday_margin` variant cannot pass as written** (founder; DEC-285 item 6).
  Its `expect_overrides.step_1: { decision: { verdict: allow } }` merges into a decision that
  keeps `reason_code: legacy_pdt_day_trade_budget`, so the expectation is an allow with a deny's
  code. Under DEC-259 item 7, its step 3 would also wait, because step 1 is allowed and never
  filled. The fix is a YAML change: state the whole decision in the override, and fill step 1 or
  drop step 3 in the variant.
- **Pin DEC-285 item 3's `gtc` for a crypto proposal with no `tif`** (#419 review, minor 2). The
  trading-domain driver reads an absent crypto `tif` as `gtc`, but nothing in the gate reads a
  crypto order's `tif` today, so `day` and `gtc` decide alike and no test can tell them apart. When
  E6-8's session rules or the executor first read a crypto order's `tif`, add the test that a
  crypto proposal with no `tif` is decided as `gtc`.
- **Drive the trading-domain gate cases through the real order path once E7-4 and E7-5 do**
  (the coordinator's ruling on #370, item 2). DEC-259 item 7 is a reading of the reference harness
  only: it decides a later proposal once the earlier allowed ones were filled in full by `fill`
  steps, and carries their conduct figures from those fills. When E7-4's `actions` and E7-5's
  account ledger give a submission its working order, reservation and partial fills, switch
  `mandate-refcases`' trading-domain gate driver to that path, delete the reading and its
  `Order` bookkeeping in `trading_domain/gate.rs`, and let partial fills and overlapping orders be
  decided as the order path decides them. Also read a `broker_account_update` that changes the
  regime or its figures (DEC-284 item 4).
- **Check the trading-domain pin of pending reasons for completeness** (#408 review, not a
  finding). `every_other_rc_15_variant_and_gate_case_names_the_story_it_waits_for` is a fixed list,
  so nothing makes a newly pending gate case join it. A rung-2 check that derives the list from the
  pending gate cases, less those a ruling excepts (RC-09, RC-09B), would close the gap.
- **Lift §2.2's trade date onto `ExchangeCalendar`** (#370 review, minor 1). `mandate-risk`'s
  `daytrades::trading_day` re-states `TradingCalendar::equity_trade_date`'s 20:00 ET cutoff over
  the committed exchange calendar, and `the_trade_date_agrees_with_the_accounting_calendar` pins
  the two together across 2026's holidays, both daylight-saving changes and the cutoff. Give
  `mandate-time`'s `ExchangeCalendar` an `equity_trade_date` of its own and call it from both, so
  the rule lives once.

From the independent review of E6-2's autonomy slice ([#216](https://github.com/kunwarshivam/mandate/pull/216)
round 1, verdict approve; minor 2, deferred by the coordinator's ruling):

- **V-023 at load must bound a decimal's precision (stream F).** The schema's `decimal` admits 28
  fractional digits and `Ratio` holds 24, so a rule comparing `order_usd` against a 26-digit value
  passes `mandate-builder`'s `check_rules`/`well_typed` re-check and is refused mid-walk by
  `Condition::matches` with `too_precise`. It is still refused, so rule 3 holds, but DEC-152 (1)
  promises the whole rule set is re-checked before any rule is read. Stream F's V-023-at-load in
  `mandate-spec::validate` refuses such a value up front (landed with E10-1's slice V, DEC-161 item 5),
  and the order path's `well_typed` gains the same bound so both report it by name.
- **E6-6:** make a skipped collar countable without a replay. When DEC-383 skips a collar that
  cannot be computed, `applied` does not name it and nothing is journaled, so the decision is
  identical to one whose collar computed and bound nothing; a named entry in the decision or a
  journaled fact would let the skips be counted (#452 review, m2).
- **E6-6 tests correction:** the doc comment of `properties::an_exit_over_extreme_figures_is_still_routed`
  still says "Pending E6-6" though the test is live since #452; DEC-77 item 2 kept the
  implementation PR from touching it (#452 review, m5).
  *Done (`agent/g-e6-6-tests-pins`):* the comment says the test is live since E6-6's exit routing.
- **E6-6:** decide a proposal of zero quantity by name. The gate allows one on every path whose
  checks pass (a zero sell is a reduction, a zero buy passes every limit), so an `Allow` can
  carry an order of nothing, which the broker refuses; only a discretionary exit over an
  uncomputable collar keeps the collar's error for it (DEC-383 item 3). Found by the E6-6
  exit-routing fix's differential matrix (6840 allowed zero rows on `main`); refusing it denies
  no reduction, since a zero order reduces nothing.
  *Tests staged (DEC-401):* `evaluate` refuses a zero proposal before any check, as
  `Unimplemented` until the implementation names it `GateError::ZeroQuantity`; two pending tests
  in `crates/mandate-risk/tests/hand.rs`.
  *Done (DEC-401, `agent/g-e6-6-zero-qty-impl`):* the refusal is `GateError::ZeroQuantity`, both
  tests are live, and the in-module property asserts the named refusal.
- **E7: the gate port's adapter handles `ZeroQuantity`** (stream K, when §9.1's gate port into
  `mandate-executor` is wired; DEC-401 item 5, #486 review, m4). `mandate_risk::evaluate` refuses
  a proposal of zero quantity with `GateError::ZeroQuantity`. The port's adapter must either filter
  a zero quantity before calling the gate or treat that refusal as nothing to do. It must never let
  the refusal abort a multi-order step (a flatten, an exit-price ladder rung, an exit sequence).
  A test in the port's own suite pins it.
- **E6-6:** pin the rest of a re-priced exit and of the close window. The tests assert
  `marketable_limit_required` and the quantity of a market exit re-priced in an auction window but
  not its `limit_price` or `applied`, and `hand::the_close_window_follows_the_early_close_calendar`
  asserts only times inside the window, so it passes against a `close_window` that is always true
  (#228 review, round 1, minor 3; E6-6's bug list when it lands).
  *Done (`agent/g-e6-6-tests-pins`):* the auction test asserts the re-priced risk exit keeps the
  proposal's limit with no §9.6 control applied, and the close-window test asserts a nanosecond
  before the window, its first and last instants, and the close, on a full and an early-close day.
- **E6-6: pin a presumed-halt exit's price and controls** (#484 review, round 1, m2).
  `hand::a_dropped_status_feed_is_a_presumed_halt` asserts that the re-priced market risk exit is
  allowed with `marketable_limit_required`, but not its `limit_price` or `applied`. §4.4 and §5.6
  re-price that exit as the auction window does, so it is the same unpinned pair the auction test
  now pins: the proposal's limit, and no §9.6 control.
  *Done (`agent/g-e6-6-halt-pins`):* the test asserts the quantity, the proposal's limit and an
  empty `applied`.
- **E6-4 harness: family B's trim arm compares the trim** (DEC-250 item 11, DEC-399 item 6).
  `mandate_risk::trim_proposals` answers since E6-4's implementation PR, but
  `crates/mandate-refcases/src/mandate/order_builder.rs`'s `trim_first` still fails every
  `trim_to_target` case, and its `OWED` rows pin the answer (MC-B17 a 3-share `RiskExit`, MC-B30 to
  MC-B32 none). The arm owes the comparison: a trim's `action`, `qty`, `limit_price` at the bid,
  `order_usd`, `purpose`, `origin: risk_engine` and `autonomy` (`builtin_risk_reducing`), and, when
  no trim is proposed, `trim_withheld` and the builder's own answer after it (§6.2 step 1). The
  guards are not in `trim_proposals`' answer, so naming them needs a reading in its own decision:
  derive each guard the harness can state from the case (`scale_active_s`, `holding`, the session,
  the minimum) and require the gate's empty answer to agree, or widen the API in a tests PR first
  (DEC-77). MC-B17 and MC-B30 to MC-B32 then pass; their status rows follow in a status-only PR.
  *Done (DEC-400):* the arm compares the trim and derives `ref.py`'s guards from the case; the
  four cases pass in the harness. Their `status.toml` rows follow in a status-only PR.
- **E6-4 reference: drop `ref.py`'s `factor < 1` trim guard and its dollar minimum** (#466 review,
  round 1, m2; DEC-399 items 3 and 5). `reference/mandate/ref.py`'s `builder` trims only below a
  factor of 1 and compares the sell's notional with `min_order_usd`. `mandate_risk::trim_proposals`
  follows §5.5's text (no guard) and trading spec §5.3 rule 2's quantity minimum, `min_order_size`,
  which the review agrees is right. Removing the guard only tightens (DEC-176). The follow-up is a
  reference PR on its own (ES-22): `ref.py`, a regenerated `mandate.yaml` and
  `fixtures/refcases/mandate.json` if any case changes, and `check_cases.py`, `fuzz.py` and
  `mutants.py` passing, and the same two readings removed from the harness's
  `crates/mandate-refcases/src/mandate/order_builder.rs::trim_guards` (DEC-400), which mirrors
  `ref.py`. A rung factor of 1 cannot be written: the schema's `open_fraction` excludes it, and
  `scaling_rung` requires a case's factor to be a rung's. So only the dollar minimum can diverge,
  and no trim case states a sub-minimum trim today.
  *The guard half is closed with no change:* `ref.py`'s `factor < 1` means "a `scale_sizes` rung is
  active", since every rung factor is an `open_fraction` below one, and the gate trims only under
  an active `trim_to_target` rung (DEC-399 item 2), so the two agree on every input the schema can
  write. Dropping the guard would make `ref.py` withhold a trim `rung_not_confirmed` with no rung
  active. Only the minimum's reading remains, a question to the coordinator on claim
  [#123](https://github.com/kunwarshivam/mandate/issues/123).
  *The dollar-minimum half, harness side, closed in #498* (DEC-399 item 8, option (a) on #123).
  `trim_guards` judges `below_minimum_order` by the instrument's `min_order_size`, and **that
  quantity reading is the one that stays**: do not remove it.
  *Done (the minimum half, reference side):* #504 moves `ref.py` to the same quantity minimum, has
  §5.5 name it, and adds MC-B33 and MC-B34, which pin the two readings' difference.
- **E6-4: a trim of the whole position is never withheld for the minimum
  ([DEC-423](decisions/DEC-423.md); #504 review, M1, the coordinator's ruling there, and its
  narrowing on #520). Required next in stream G, ahead of MC-B33 and MC-B34's status PR and the
  harness cleanup.**
  `trim::proposals` (DEC-399 item 5) and, since #504, `ref.py` withhold a trim below
  `min_order_size` even when it is the whole position held. Trading spec §5.3 rule 2 exempts a
  sell closing the full position by its exact quantity, and nothing further down the gate refuses
  a risk exit, so this is a risk exit held by a minimum the broker does not apply. A remainder
  beside one of the agent's own resting sells is not that sell, and stays withheld. In order:
  (1) a DEC-77 tests PR in `mandate-risk` (#520): a trim of the whole position is proposed below
  `min_order_size` (equity, under a protective stop, and crypto at a zero and a non-zero target),
  and a sub-minimum trim that is not the whole position is still withheld (beside a resting sell,
  one increment short, and 3 of 10);
  (2) the gate change;
  (3) §5.5 gains the exemption, `ref.py` takes it, a new case states the full-close trim, and the
  harness's `trim_guards` moves with them, as #498 did. The same PR corrects DEC-399 item 8's
  known-defect clause, which says the #504 case's trim is the whole position: on the 1e-9 grid it
  is not (the coordinator's ruling on #520).
  *Done:* (1) in #520; (2) in the implementation PR, which replaces the stub with the condition
  itself, so the exemption cannot fail.
  *(3), harness side (`agent/g-e6-4-full-close-harness`):* `trim_guards` exempts a trim of the
  whole position, MC-B17 reshaped as 1 share at a 2-share minimum pins it, and MC-B35 is awaited.
  The reference PR follows.
  The #504 review's own case (0.0002 BTC, a cap of 100, factor 0.5, a 0.001 minimum) is **not**
  released by this: on the gate's 1e-9 grid its trim is 0.000116667, not the whole position. The
  "real quantity grid" row below closes it.
- **E6-4 reference: `ref.py` judges the trim's minimum on the whole excess, not on the remainder
  after open sells** (#504 review, m2). `ref.py`'s builder has no open-sell input, so it sizes the
  trim on the whole excess. The gate subtracts the agent's own resting sells first (DEC-399 items 5
  and 7, pinned by #507). For example, 10 shares held, 2 resting on an agent sell, an excess of 3
  shares and a `min_order_size` of 3: the gate proposes nothing (a 1-share remainder), and `ref.py`
  proposes 3. No reference case states a resting sell. The oracle errs toward selling more, so a
  case that reached it would fail loudly. Carry an open-sell input into `ref.py` and add a case.
  *Harness side (`agent/g8-e6-4-open-sell-harness`):* the family-B harness reads an optional
  `open_sell_qty` on a trim base, rests it as one non-protective agent sell in the gate's scene,
  and sizes `trim_guards`' trim on what it leaves, as the gate does. It awaits MC-B36 and MC-B37
  (`AWAITED`, with the counts for both fixtures). The reference PR follows, then the status rows,
  then a cleanup that drops `AWAITED` and makes the input required.
  *Done:* the reference side in #567 (MC-B36, MC-B37, §5.5 states the subtraction); the cleanup
  (`agent/g8-e6-4-open-sell-cleanup`) drops `AWAITED`, so family B's thirty-seven cases and the
  sweep counts state one fixture, and refuses a trim case with no `open_sell_qty`.
- **E6-4 harness cleanup: drop the transitional branches once MC-B33 and MC-B34 are on `main`**
  (#498 review, M1). In `crates/mandate-refcases/src/mandate/order_builder.rs`:
  (a) drop `AWAITED`, `awaited`, `counted` and the without-branch, so the case list and the sweep
  counts state only the fixture with both cases;
  (b) remove the `min_order_size` default (`stated.increment`) in `trim_first`, once the reference
  PR makes the input required on a trim base.
  Until then, the case-list pin is open: dropping both awaited cases from the fixture fails
  nothing, because `counted` falls back to the counts without them.
  *Since #526:* MC-B33 and MC-B34 are passing cases and `AWAITED` is MC-B35 alone; once #530
  lands, (a) applies to MC-B35. The same cleanup rewrites `trim_guards`' doc comment, which still
  says the exemption is one "which `ref.py` takes up in the reference PR after this harness"
  (#530 review, m5).
  *Done (`agent/g8-e6-4-harness-cleanup`):* `AWAITED`, `awaited` and `counted` are gone, so family
  B's thirty-five cases and the three sweep counts state one fixture; `trim_first` refuses a trim
  case with no `min_order_size`, which a doctoring that drops it pins; and both doc comments say
  what the harness does now.
- **ES-22: the minors of #530's review, round 1** (DEC-423; freeze rule; m1, §5.5's binding clause, was done in #530):
  (1) §11 says 427 cases where `mandate.yaml` holds 430, and nothing compares the prose with the
  file. Correct the count, or have `generate.py` or an xtask check state it (m2);
  (2) the spec's DEC-399 change-history entry still says the #504 example's trim is the whole
  position. #530 corrected the same sentence in DEC-399 and only forward-referenced it in the
  spec; add the parenthesis there too (m3);
  (3) `docs/project/decisions/README.md` rule 4 says to supersede an accepted decision rather than
  edit it, yet #504 and #530 edited DEC-399 item 8 on the coordinator's rulings. Record the
  departure: either DEC-423 item 5 names the DEC-399 correction, or rule 4 gains a clause for a
  correction a coordinator rules (m4);
  (4) the trim's quantity grid differs between the reference and the gate. `ref.py` and the
  harness read the case's `qty_increment`, and the gate derives 1e-9 or 1 from `fractionable`. On
  a fractionable instrument they can disagree on `sell == held`: for example, 0.0002 held at a
  600,000 bid, factor 0.02, a 0.001 minimum, where `ref.py` proposes 0.0002 on a 0.0001 grid and
  the gate proposes nothing on 1e-9. No family-B case is fractionable, and one would fail the
  harness loudly. The "real quantity grid" row closes it; pin it there with such a case (m6);
  (5) add `<=` for `<` in the trim's minimum as a registered `mutants.py` trim mutant. MC-B33
  catches it today, but nothing registers it (the review's plants).
  *Done (`agent/g8-530-minors`):* (1) §11 says 433, and `check_cases.py` now fails when §11's
  count differs from the file; (2) the spec's DEC-399 entry names both grids; (3)
  [DEC-426](decisions/DEC-426.md) records the two in-place edits as departures and keeps rule 4,
  and DEC-399's status line points to it; (4) stays with the "real quantity grid" row; (5)
  `mutants.py` registers "a trim at the minimum is withheld", which MC-B33 catches.
- **E6-4 harness: one scene for both gate calls on a trim base** (#498 review, m3). `trim_first`
  sets the scene instrument's `min_order_size` from the case, but `judge` builds its own scene in
  `Scene::read`, which takes the minimum from `qty_increment`. A case where a trim is withheld and
  the builder then proposes would run §5.3 rule 2 against a different minimum from the trim's.
  Unreachable today, since every withheld-trim case ends in a hold. `holding` and `scale_active_s`
  have the same shape. Read the trim inputs in `Scene::read`, so both gate calls get one
  instrument.
  *Done (`agent/g8-e6-4-one-trim-scene`):* `Inputs` reads `TrimInputs` on a trim base, and
  `Scene::read` applies the minimum, the active rung and the Holding goal for both gate calls.
  `both_gate_calls_on_a_trim_base_see_one_scene` moves MC-B01's 7-share buy onto the trim base: at
  an 8-share minimum the dry run meets §5.3 rule 2, which it fails on a `qty_increment` minimum.
  Holding and the rung reach the dry run too, but only `trim_proposals` reads them.
- **E6-4: pin that the trim's minimum is judged on the remainder after open sells** (#498 review,
  m8; DEC-399 items 5 and 7). No test combines a resting sell with a minimum above one increment,
  so a gate that judged the whole excess passes every test in the workspace. Close it in
  `mandate-risk`'s `trim::tests`: `Scene::new("10", "1000")` with `resting(7, "2", Side::Sell,
  false, true)` and `min_order_size = 3` leaves a 1-share remainder, which must not be proposed,
  while a whole-excess reading (3 shares) proposes it. Next in stream G, before the reference
  PR's status change.
  *Done:* `trim::tests::the_minimum_is_judged_on_the_remainder_after_resting_sells`, which fails on
  a seeded whole-excess reading.
- **E6-4: read the instrument's real quantity grid for a trim** (#466 review, round 1, m3).
  `trim.rs` and `conduct::slice` both take whole shares or nine places from `fractionable`, because
  `InstrumentSnapshot` carries no increment. DEC-128 item 27 found that reading wrong for the
  builder (increments of `0.0001` and `0.000001` exist). Once the snapshot carries the increment,
  round the trim up on it, so a trim is never off the grid that §5.3 rule 2 refuses. The
  reference model and the harness already read a grid from the case (`qty_increment`), so the
  gate's reading must then agree with theirs on `sell == held` for a fractionable instrument
  (#530 review, m6). **This is
  also what releases the #504 review's case** ([DEC-423](decisions/DEC-423.md), Rationale): 0.0002
  BTC, a cap of 100, factor 0.5 and a 0.001 minimum give a trim of 0.000116667 on the 1e-9 grid,
  withheld below the minimum, but 0.0002 on the venue's 0.0001 grid, the whole position, which
  DEC-423 exempts. Add that case as a trim test when this lands.
  *Tests (`agent/g8-e6-4-qty-grid-tests`, [DEC-427](decisions/DEC-427.md)):* `InstrumentSnapshot`
  carries `qty_increment`; `trim::quantity_grid` is the stub, and three pending tests pin the trim
  (the #504 case included) and the participation slice on a venue grid. The implementation PR, then
  a harness PR stating the case's grid, follow.
  *Round 1 of #571's review:* the #504 case's named figures, B2's truncation and B3's slice
  ([DEC-445](decisions/DEC-445.md)), M1's on-grid minimum, M3's rows, and the property
  `the_trim_and_the_slice_stay_on_the_venue_s_grid` are pending in the tests PR. Two live rows that
  asserted DEC-445's superseded figures moved into pending tests.
  *Implementation (`agent/g8-e6-4-qty-grid-impl`):* `quantity_grid` is the field read; the trim
  truncates an off-grid remainder that is not the whole position (`down_onto`), and `slice` tests
  the untruncated cap for zero, truncates it onto the grid and floors it at the minimum rounded up
  onto the grid (`up_onto`). The six pending tests are live. The harness PR stating the case's grid
  follows.
  *Harness (`agent/g8-e6-4-qty-grid-harness`):* family B's listing states the case's
  `qty_increment` as the grid, and the trading-domain gate driver a crypto pair's
  `min_trade_increment` (an equity's stays whole or fractional shares), so the reference, the
  harness and the gate share one grid (#530 review, m6), and `trim_guards` sizes the trim in the
  gate's order and truncates an off-grid remainder (DEC-445 item 2). It awaits MC-B38 and MC-B39
  (`AWAITED`), which the reference PR adds (`agent/g8-e6-4-grid-ref`); then the status rows and a
  cleanup. DEC-427 item 6's ordering constraint on fractionable family-B trim cases no longer binds.
- **E6-4: refuse a grid that is not above zero, per instrument** (#571 review, round 1, minor 2).
  `InstrumentSnapshot::qty_increment` is a `Qty`, so zero is representable. `ceiled_quotient` and
  `truncated_quotient` refuse it as `NotPositive`, but that error would leave `trim::proposals`
  whole and withhold every instrument's trim (DEC-423 item 4 forbids it), and from `slice` it errors
  the decision, denying an exit (rule 13). Give the field a type that cannot hold zero (trust ladder
  rung 1), or refuse it per instrument where the snapshot is built from connector data, with a test.
  Unreachable today: no production caller builds the snapshot. Also: `conduct::slice` computes the
  on-grid minimum even when there is no cap to truncate, so a non-positive grid errors a decision
  nothing would have paced; compute it only where a cap exists (#578 review, round 1, minor 1).
- **E6-4: the reference and the family-B driver carry DEC-445 item 2 before the harness states the
  case's grid** (#578 review, round 1, minor 2). `ref.py` and `order_builder.rs`' `trim_guards`
  clamp to what is unsold without truncating an off-grid remainder onto the grid, and `ref.py`
  rounds up before subtracting the resting sells where the gate subtracts first. The harness PR
  that states each case's own grid (DEC-427 item 6) brings both: §5.5's sentence and `ref.py`
  first, with a case whose remainder is off a coarse grid, then the harness.
- **E6-4: draw the daily cap in the grid property** (#578 review, round 1, minor 3).
  `properties::the_trim_and_the_slice_stay_on_the_venue_s_grid` sets `adv_20d` to `None`, so the
  daily cap's grid behaviour rests on `a_slice_truncates_to_the_venue_s_quantity_grid`'s M4 row.
  Draw `adv_20d` and today's participation too.
- **E6-4: one ingest field for the quantity grid** (#571 review, round 1, minor 4).
  `mandate-builder`'s `Market::increment` (DEC-128 item 27) and `InstrumentSnapshot::qty_increment`
  are two grid fields that must come from the same instrument-master field. When ingest lands, read
  both from it and add a check that they agree (DEC-427, Alternatives).
- **E6-4: `trim::tests`' factor overrides disagree with the fixture's rung** (#571 review, round 1,
  minor 5). Several `trim::tests` rows set `risk.size_factor` to a value no rung of the `Scene`'s
  one-rung ladder (factor 0.5) carries, an internally inconsistent snapshot. Give `Scene` a rung
  whose factor matches each row, or derive the factor from the rung.
- **E6-4 nits** (#466 review, round 1): `UsdExact::times_size_fraction`'s doc still lists "the
  ladder size factor applied to the target" though the gate's 24-place `Ratio` factor enters
  through `UsdExact::of_ratio`; say which number each serves (n2). `of_ratio` takes any `Ratio`,
  and stream F's V-040 is what bounds the factor at one.
  *Done (`agent/g-e6-4-nits`):* both doc comments say which factor each serves and why any
  `Ratio` is safe for `of_ratio`.

From E6-2's builder slice (stream H; found while implementing §8.3, not by a review):

- **§8.3 step 2's "whole position (minus working exits)" has no input (stream H, with stream I).**
  `AccountSnapshot` carries no working exit quantity, so `propose` sells the whole position, as
  `reference/mandate/ref.py` does, and a second discretionary exit while one is working is proposed at
  the full quantity. Until the field lands, the backstop is the risk gate's trading spec §5.3 rule 4
  (`sell_exceeds_available`, merged in #221), which binds every reduction and denies the over-sell,
  so the builder cannot turn a position short on its own. The field arrives by a DEC-77 tests
  correction ahead of the slice that consumes it (every `AccountSnapshot` literal in
  `crates/mandate-builder/tests/` names it); that slice then subtracts working exits in
  `discretionary_exit` and holds when nothing is left to sell (#234 review, round 1, M1).
- **The runtime filters model outputs by instrument before `combine` (stream I).** `combine` refuses
  the whole call on an output for another instrument (`output_instrument_mismatch`, DEC-130), the exit
  branch included, so handing it a tick's unfiltered output buffer would stop a discretionary exit
  the way a crossed quote did before #234 moved that refusal to the buy path. E6-1's evaluation loop
  passes only the outputs whose `instrument_id` is the instrument being sized, with a test that a
  foreign output in the buffer never blocks an exit (#234 review, round 1, m2).
- **`BuilderError::Unimplemented` and `NumError::Unimplemented` are now constructed by nothing.**
  Both stay because `tests/vocabulary.rs` and `num::error_codes_are_stable` pin their codes, and an
  implementation PR may not edit a test. Drop each variant with its row in the next tests correction
  that touches those files (E6-2; the `NumError` half is the E4-2 row above).

From E6-4's V-040 spec change (stream H; the coordinator's ruling on #251, round 1):

- **Add V-040's boundary pair to `mandate.yaml`, with the harness counts, in one approved change
  (founder).** The rule's boundaries are two ladders: factors of 12 places in total (valid) and 13
  (V-040). The 2¹³ × 5¹³ ladder, where the whole product fits and a subset does not, belongs there
  too. For now all three are pinned in `reference/mandate/fuzz.py::fuzz_ladder_precision`, which runs
  on every seed, and as in-module rows in `crates/mandate-spec/src/validate/tests.rs`. They are not
  reference cases because a case changes counts that live `mandate_harness.rs` tests assert (67
  semantic, 202 owned, and the member sweeps), and the spec guard keeps the fixture and
  those tests in separate PRs. The founder-owned YAML (ES-22) and the counts must change together.
- **Three minors from #251's round 2, deferred by the freeze rule.** (1) `docs/specs/mandate.md`'s
  front matter still says a change needs founder approval with no qualification; DEC-167 item 3
  records that a stricter V-rule is agent-accepted under DEC-79, and the front matter should say so
  in one clause. (2) `fuzz_ladder_precision` draws 1 to 4 scale rungs plus two fixed ones: it never
  reaches the five-rung maximum that item 3's "five 2-place rungs still fit" relies on, and at four
  it builds a six-rung ladder `maxItems: 5` forbids. Draw 1 to 3 scale rungs, and pin the
  five-rung, 2-place ladder. (3) The function imports `combinations` inside its body; move the
  import to the top of `reference/mandate/fuzz.py`.

From E6-4's slice L (stream H; the coordinator's ruling on #279, round 1, minor 3):

- **The caller of `goal::status` pins the sane-and-fresh ask (stream G).** `GoalInputs::ask` values a
  remainder against the minimum order, and `goal::status` cannot tell a bad tick from a real quote.
  One print far below the market would make any remainder look worth less than the minimum and
  finish the goal, and with `on_complete: release` that cancels protection and retires the agent
  (§3.1). The order path's goal evaluation in `mandate-risk` passes only an ask from a quote that
  passed §5.6's sane-and-fresh filter. It needs a test that a single bad tick never finishes a
  `release` goal.

From E6-4's slice R1 (stream H2; #289's review round 2 approved it, and the freeze rule defers
these to R2, where the fold starts reading `Limits::conditions`):

- **Ignore the directory proptest writes (#289 round 2, nit 2).** `.gitignore`'s
  `*.proptest-regressions` does not match the `proptest-regressions/` directory proptest writes
  beside a crate's `src/` (a failing in-module property leaves
  `crates/mandate-spec/proptest-regressions/risk/fold/tests.txt`); ignore the directory. R2 stays
  inside `crates/mandate-spec`, so it did not change `.gitignore`.

From E6-4's slice R2 (stream H2; DEC-167 item 6):

- **`ref.py` counts a fill as a sane quote (reference fix).** `RiskState.step` sets
  `quote = kind in ("mark", "fill") …`, so a fill re-reads the last mark as a second quote and can arm,
  latch, or clear a hard breach. §5.6 says "sane quote", and a fill quotes nothing: E moves by the
  fill's price against the mark, not to a new mark. One flash print followed by any fill would latch
  a limit on one print, which DEC-63 rules out. The crate follows the spec (DEC-167 item 6), and no
  reference case changes either way. Drop `"fill"` from the tuple and regenerate with
  `reference/mandate/generate.py`, which must leave `fixtures/refcases/mandate.json` unchanged.
- **The `ref.py` reading the #124 handover left for R4.** Settling time before an allocation change
  only when `at > self.t` is equivalent to settling always; settle always. Readings 1 and 2 were
  R3's and are DEC-167 item 7 (a) and (c); handover items 4 (one cash sum) and 5 (the post-loop lift
  reset) are R2's and are in `risk/fold.rs`. *Done in R4 (DEC-167 item 8 (d)):* the fold settles
  always.
- **R3's status PR: fifteen MC-R cases pass.** MC-R01 to MC-R08, MC-R13, MC-R15, MC-R18 to MC-R20,
  MC-R22, and MC-R24 pass `cargo test -p mandate-refcases --test refcases -- --include-ignored
  mandate::MC-R` on R3 (DEC-167 item 7 (l)); proposing them for `status.toml` is founder-owned.
  *Done ([#356](https://github.com/kunwarshivam/mandate/pull/356), under DEC-77 item 3 as #335
  and #337 were):* exactly these fifteen are marked; the other nine stop at R4's inputs.
- **DEC-167's wording after R3's tests correction** (#357 review, nits). *Done in R4:* both edits
  are made. Item 6(c) keeps the
  superseded sentence "so it stands when the step's own sale leaves the book flat, as in
  `ref.py`", whose attribution is wrong (`ref.py` discards `stale_mark` on a flat book); strike the
  clause now that 7(e) supersedes it. Item 7(a)'s "only the latch can come earlier" should say
  earlier than what: a wait restarted at the rollover.
- **The families G and F harness after its tightening** (#348 review, nits), one tests change to
  `crates/mandate-refcases/src/mandate/risk_gate.rs`: (1) compare `pacing` after the verdict and
  reason, so a case whose verdict is wrong fails naming the verdict rather than a pacing the wrong
  verdict brought; (2) name `compare_computed`'s `c` parameter for what it holds (the gate's
  figures); (3) extend the group-id test to a case with two groups, so the rank of a name among
  several is pinned, not only the one group's id; (4) shorten the done row's verbatim "The row as it
  was" copy to a pointer at #317's re-review. From the round-2 review: (5)
  `a_denial_fails_unless_exactly_one_check_failed_with_its_reason` ends with a bare `Ok(())`; bind
  its failing shapes to a name and finish with `expect_eq("failing shapes", shapes.len(), 3)`, as its
  allow sibling counts its edits; (6) DEC-178 item 11 records only the allow pin; add a sentence for
  the denial pin and its test.
- **The trading-domain gate driver after its tightening** (#349 review, nits), one tests change to
  `crates/mandate-refcases/src/trading_domain.rs` and its gate tests, plus DEC-199's wording:
  (1) `pending()` counts the second `propose_order` before a backtest case is dispatched, so say in
  `SUBMISSION_STORIES`'s doc that the refusal applies to every case kind; (2) a case whose earlier
  proposals are all denied is refused too, though a denial leaves no working order, so either admit
  it or record why the refusal stays uniform; (3) DEC-199 item 6 says the gate harness shows "four"
  members deciding at their edge where it lists three (the median, the trailing volume, and the
  minimum order; the ADV is shown only as at least 1,000,000); (4) rename the `num` closure in
  `the_listing_and_market_are_dec_199_item_6s`, which shadows the crate's `num`; (5) shorten the
  done row's verbatim "The row as it was" copy to a pointer at its review.
- **E8-3's check 7 tests after the overlay correction** (#354 and #355 review, nits; nits 2 to 5
  done in E8-3's `quorum` PR, DEC-257 item 4): `crates/mandate-approval/tests/quorum.rs` draws a
  `two_approver_above_usd` of `"0"`, which `schemas/policy.schema.json` excludes
  (`positive_decimal`); draw only ceilings a workspace can hold, as DEC-173 item 15 already does
  for grant sets. And (#368 review, minor 3) `tests/quorum.rs`'s module doc still says its tests
  are pending and fail on `quorum`'s `ApprovalError::Unimplemented`, which E8-3's `quorum` made
  false. A tests correction, since DEC-77 keeps `tests/` edits out of an implementation PR.
- **Family A's harness after its tightening** (#360 review, nits), one tests change to
  `crates/mandate-refcases/src/mandate/autonomy.rs` plus DEC-162's wording: (1) DEC-162 item 4 says
  the property test runs "over generated well-typed policies and facts", but `action_context()` pins
  `drawdown` and `daily_pnl_fraction` to zero and `well_typed_leaf()` never builds a condition over
  `instrument`, `session`, `drawdown`, `daily_pnl_fraction` or `position_pnl_fraction`; name the
  space the strategies cover (the claim holds, since §6.2 step 3 decides on `purpose` first);
  (2) the module doc names only `no_rule_set_ever_denies_or_asks_a_reducing_purpose`; add
  `hand::every_reducing_purpose_is_auto_by_the_builtin` for the other three reducing purposes, as the
  row does; (3) the placeholder refusal is the module's only message prefixed with the case id,
  which the suite already reports; drop the prefix and the `id` parameter it needed; (4) give
  `unread_instrument` a doc comment; (5) assert the second premise behind `PLANTED = "input"`, that
  no `action` or `expect` carries an `input` member.

From E6-4's slice R3 (stream H2; DEC-167 item 7):

- **`ref.py` drops a daily hard wait at the rollover (reference fix).** `RiskState.step` pops
  `hard_first["max_daily_loss"]` into the rollover record and never reads it again, so `hard_breach`
  can stay applied with nothing to clear it. The crate keeps the wait under the new day (DEC-167
  item 7 (a)). Keep it in `hard_first` and regenerate with `reference/mandate/generate.py`, which must
  leave `fixtures/refcases/mandate.json` unchanged (checked from the crate's side: putting `ref.py`'s
  reading into the fold leaves the same fifteen MC-R cases passing).

From E6-4's slice R4 (stream H2; DEC-167 item 8):

- **`ref.py` holds a re-triggered scale rung back during the stepped lift (reference fix).** After
  an acknowledgment `RiskState._ack` queues the scale rungs highest `at` first, and a rung that
  triggers again is removed from `reset_queue` and may lift only once the queue is empty, so the 3%
  rung can lift while the 4% rung stays active. §5.8 says highest `at` first, and the fold lifts the
  highest active rung next (DEC-167 item 8 (b)). Re-insert a re-triggered rung in its place, or lift
  only the highest active rung while stepping, and regenerate with `reference/mandate/generate.py`,
  which must leave `fixtures/refcases/mandate.json` unchanged. In `agent/h2-dec-270-reference` (DEC-275).
- **`ref.py` journals `hold_protected` for a completed `profit_stop` goal (reference fix).** A
  `goal_complete` input on a `profit_stop` goal (its end date) writes
  `{"type": "GoalCompleted", "on_complete": "hold_protected"}`; the fold writes its one §3.1
  outcome, `then: discretionary_exit_all_then_retire` (DEC-167 item 8 (f)). No reference case has
  one. In `agent/h2-dec-270-reference` (DEC-275).
- **`release` journals the loss carry: the reference side (DEC-270; stream H2, first).** One
  reference and reference-case PR, kept apart from crates (ES-22): `reference/mandate/ref.py`'s
  `goal_complete` arm journals `AgentStopped` with `loss_carry_usd` = max(0, N − E) after
  `PositionReleased` when `on_complete` is `release`; MC-R17's expected journal gains it; any §3.1 or
  §5.7 wording the spec needs under DEC-270; `generate.py`, `check_cases.py`, and the fuzz rerun, and
  `cargo xtask refcases --write`. Only MC-R17 moves. Merged in [#386](https://github.com/kunwarshivam/mandate/pull/386)
  (DEC-274), with MC-R25, MC-R26, and MC-V68 for the carry and the redeploy, after the harness PR
  [#381](https://github.com/kunwarshivam/mandate/pull/381) (DEC-276, DEC-277).
- **`release` journals the loss carry: the `mandate-spec` side (DEC-270; stream H2, after the
  reference PR).** `Fold::complete_goal`'s `Release` arm journals `AgentStopped` (reason
  `goal_complete`) with the loss carry, as `retire` does, and a test shows that releasing and
  redeploying on the same connection opens the new agent at the carried L (through
  `ValidationContext::from_journal` and V-032). Merged in [#387](https://github.com/kunwarshivam/mandate/pull/387).
- **MC-R17 back to passing (DEC-277; E6-4, stream H2).** The DEC-270 reference PR changes MC-R17's
  expected journal and marks it `pending`, the one flip DEC-277 allows. The `mandate-spec` side above
  must bring it back: the status PR that follows it marks MC-R17 `passing` again, with MC-R25,
  MC-R26, and MC-V68. Done in `agent/h2-dec-270-status` (DEC-77 item 3), after [#387](https://github.com/kunwarshivam/mandate/pull/387).
- **DEC-277's exception for the `journal` and `trading_domain` suites (the #381 review, round 2,
  minor 1).** `fixture_entry_changed` in `xtask/src/main.rs` finds a case by an object whose `id` is
  its `status.toml` key, which matches the `mandate` suite but no `journal` key and not
  `trading_domain`'s compound `RC-NN::sub_case` or version rows, so a flip there always reads
  "unchanged" and is refused. Extend the lookup to those key shapes, with fixture-repository tests in
  each suite.
- **Narrow DEC-277's exception to the case's expectation (the #381 review, round 2, minor 2).** The
  guard allows a `passing` → `pending` flip when any part of the case's fixture entry changes; allow
  it only when the entry's expectation members change (`expect`, or a risk-state step's `expect`), a
  tightening, so a title or note edit cannot take a case back to pending.
- **§5.10's `GoalCompleted` reason for a completed `profit_stop` goal (the #386 review, minor 1).** The
  journal table names `GoalCompleted`'s reasons as `profit_stop_reached`, `target_qty`, `max_spend`,
  and `end_date`, but a `profit_stop` goal completed by its end date journals no reason in either
  `ref.py` or the fold (`then: discretionary_exit_all_then_retire` only, DEC-275). Decide whether it
  carries `end_date`, as an accumulate goal's `goal::status` would, and move the spec, `ref.py`, and
  the fold together.
- **MC-R25's note pins E before the release (the #386 review, minor 2).** MC-R25 expects
  `agent_equity` 9850 after the release: the released position stays valued on the agent's books at
  the last mark, which is what makes the carry "E before the release" (DEC-274 item 2). Add a
  sentence to its note in `generate.py` saying so, so a later change that zeroes the released
  quantity is seen to break the reading rather than taken as a fix. Say too why the quantity must
  stay: a mark after the retirement still moves `agent_equity` and journals the ladder, the daily
  loss, and the floor on a holding the owner has taken, which nothing acts on while the agent is
  `stopped` (the #387 review, minor 2). With it, rewrap `complete_goal`'s doc comment in
  `crates/mandate-spec/src/risk/fold/owner.rs` to the file's 100-column width (minor 1).
- **R4's status PR: the last nine MC-R cases pass.** MC-R09 to MC-R12, MC-R14, MC-R16, MC-R17,
  MC-R21, and MC-R23 pass `cargo test -p mandate-refcases --test refcases -- --include-ignored
  mandate::MC-R` on R4 (DEC-167 item 8 (i)); marking them in `status.toml` follows under DEC-77
  item 3, as #356 did for fifteen.

Minor findings from the independent review of slice R2 ([#324](https://github.com/kunwarshivam/mandate/pull/324);
held back by the freeze rule; R3 took minors 1, 2, and 4, DEC-167 item 7 (i) and (j), and minor 6,
`SpecError::Unimplemented`'s doc):

- **The `strictest` oracle reads the daily-loss mode by hand** (#324, minor 3; a tests correction).
  `tests/risk.rs`'s `strictest` maps `DailyLoss` to `exits_only`, while the fold reads
  `daily_loss_action`. Its property still walks `ladder_only`, whose daily action is the base's
  `exits_only`, and R3's renewal walks are in-module tests that do not use it; take the action from
  the mandate before a walk over a `flatten_and_pause` daily loss does.
- **A `Qty::checked_sub` failure is always reported as a short sale** (#324, minor 5). True while a
  negative result is its only failure; name the error by its kind if `Qty` gains another.
- **Wrap `M5-F-mandate-spec.md`'s long line** (#324, minor 7), the R2 row that runs past the file's
  wrap width.

Nits from the independent review of slice R3 ([#344](https://github.com/kunwarshivam/mandate/pull/344);
held back by the freeze rule; its two minors and its DEC-167 nits went into R3's tests correction):

- **R3's remaining nits** (#344 review). (a) A step that leaves the book flat clears `stale_mark`
  with reason `sane_mark` though no mark arrived. No change: DEC-167 item 7 (e) records it, since
  `sane_mark` is the only clearing reason §5.10 names. (b) `Limits::daily_loss`, like
  `Limits::conditions`, re-parses `HARD_TRIGGER_MULTIPLE` on every call; parse it once into
  `Limits` (an implementation change). (c) DEC-167 item 7 (k) counts 441 added non-blank non-test
  lines where a recount gives 464. Immaterial: R3 is over ES-13's 400 either way, and item 5 fixes
  its contents.

From journal spec v0.6 §9.1, the agent-stream payload schemas ([DEC-177](04-decision-log.md#decisions);
DEC-174 items 4 and 5). Until each lands, the drafts it names stay refused at `append`, which adds no
risk (rule 3):

- **Stream I: the runtime writes §9.1's payloads.** In `mandate-runtime`'s `step.rs`, `payload.rs`
  and `state.rs`: `instrument_id` and `limit_price`, with `type` and `tif`, on `DecisionMade` and
  `IntentProposed`; `null` rather than empty strings (`reason_code` on an allow, the unconfirmed
  `OwnerExitRequested`); timestamps rather than risk-clock seconds (`ObservationRecorded.as_of`,
  `ModelOutputRecorded.as_of` and `expires_at`); `data_ref` and `content_hash` as stored artifacts
  rather than inline data; `DecisionMade`'s convictions, outputs used, model weights, and clips
  applied; `step_up` as `{assertion_id, authenticated_at, method}`; and the agent stream's
  `StreamOpened` at seq 1, which nothing writes today. From round 1 of the review (DEC-177 items 9,
  11, 12, and 13): `user` and `step_up_status` on every `OwnerExitRequested`, including the owner kill
  switch's, with `step_up` only when valid; `exit_origin` on every `DecisionMade`, with the
  convictions and score `null`, and the evaluation's lists empty, on a decision no §8.3 evaluation
  produced (a goal completion, a removed instrument, a risk exit); `ask_suppressed` when a
  classified `ask` is not asked (DEC-156 item 5); and `lifecycle` as `normal`, `paused`, or
  `stopped`, never `exits_only`. From the v0.5 reconciliation (DEC-177 item 24): `causation_id` on
  each copy of an owner command, the control stream's `OwnerCommandIssued` (§9.1 rule 16).
- **Stream L: the shell's envelope carries the required `config_refs` and the `artifact_refs`**
  (DEC-174 item 5). `mandate-shell` writes `config_refs: {}` and `artifact_refs: []` on every draft,
  so every event that requires `mandate_version` or `model_version` is `missing_config_ref`, and every
  event with a `ref` member is `artifact_refs`.
- **Stream K: the executor's account-stream drafts match the registered schemas and vectors**
  (DEC-174 item 5): `risk_clock` as a timestamp string (`batch.rs` writes an integer), and only on
  the risk inputs §2 lists; `IntentReceived` as `agent_id`, `instrument_id`, `limit_price`, `type`,
  and `tif` rather than `agent`, `kind`, `instrument`, and `limit`; `OrderSubmitted` writing `null`
  for its empty members rather than omitting them (§4.2).
- **E7-9's tests PR: the harness reads `agent_stream`, then the vectors become version 4.**
  `mandate-refcases`' journal module reproduces the section's chain and artifacts, refuses each
  invalid draft with its reason and path (a change may `delete` a member), accepts each valid draft
  and valid batch, refuses each invalid batch at its `draft_index`, and fails each
  `range_verification` case with its code at its seq; `mandate-journal` gains a boolean type, the
  schema choice by stream type for `StreamOpened` and `KillSwitchActivated`, and the §9.1 rules.
  `journal::version` pins 3 and is passing, so the bump is a code PR that accepts 4, then a one-line
  spec change (ES-22).
- **`mandate-journal` and the verifier check the agent stream's cross-event facts** (DEC-177 item
  14). `append` refuses a batch whose `IntentProposed` differs in an action member from the
  `DecisionMade` it names in the same batch (§9.1 rule 10), and §11's `intent_action_mismatch` and
  `mode_event_mismatch` run in `mandate journal verify` and the scheduled verification, against the
  vectors' `invalid_batches` and `range_verification`.
- **A generated `range_verification` vector with a forward reference (#384 review, major 1; DEC-168
  item 13).** `verify_agent_stream` indexes the whole range first, so an `IntentProposed` naming a
  `DecisionMade` later in the range is compared, and a `mode_event` naming a later event or its own
  switch fails, at any `from_seq`. Only `mandate-journal`'s in-module tests pin this today; the next
  journal reference PR adds generated range cases for both, at a `from_seq` above 1.
- **Pin §9.1's ordering of `artifact_refs` and `pii_refs` ahead of rules 14 to 16 (#384 review, minor
  2).** No vector or test breaks both an envelope check and a subject or copy rule, so swapping their
  order passes. A tests-correction PR from stream L adds a draft that breaks both and expects the
  envelope check's reason and path.
- **A reference past a bounded range's end is exempted like one before its start (#384 review round
  2, minor 3).** `verify_agent_stream` leaves unchecked a `causation_id` or `mode_event` that names no
  event of the range, whichever side of the range the event lies on. §11 exempts only a reference
  before the trusted start; a range with a `to_seq` needs the rule stated and a check or a test.
- **Duplicate `event_id`s in `verify_agent_stream`'s index are unpinned (#384 review round 2, minor
  3).** The index keeps the last row with an ID. `append` refuses a repeated `event_id`, so a stored
  range never holds one, but no test says what a tampered range with a duplicate reports.
- **An `invalid_drafts` vector for rule 12's `bid_size` (#384 review round 3, minor 1).** The journal
  vectors refuse a confirmed exit without `bid` or `floor`, but none without `bid_size`; only
  `mandate-journal`'s in-module `rule_12_needs_each_member_of_a_confirmed_bid` holds it. The next
  journal-spec PR adds the draft (ES-22 keeps generated vectors out of a code PR).
- **The model registry stores each pinned model's content object as an artifact.**
  `ModelOutputRecorded.content_hash` is a `sha256:` reference, so it is in `artifact_refs` (§3), and
  §11 check 6 fails `artifact_missing` unless the object is in the artifact store.
- **Mandate spec §8.2: say what happens to an output outside its ranges** (conviction in [−1, 1],
  confidence in [0, 1]): ignored with an `ignored` reason, or refused. §9.1 records the values as
  given and does not rule.
- **The account stream's `KillSwitchActivated` and `AgentModeApplied` schemas** are not closed by
  §9.1; they close with the executor's account-stream schemas.
- **Close the approval events' schemas in §9.1, with vectors** (DEC-177 item 23). `ApprovalRequested`,
  `ApprovalDelivered`, `ApprovalResponded`, `ApprovalRevalidated`, `ApprovalTimedOut`, and
  `ApprovalCanceled` (journal spec v0.5, mandate spec §6.4) are listed in §9 but not closed. The
  change that closes them also adds the chain events an approved order needs (`DecisionMade` with
  `autonomy: ask`, `ApprovalRequested`, `ApprovalResponded` naming an `ApprovalResponseSubmitted`,
  `ApprovalRevalidated` with result `act`, and its `IntentProposed` in the same batch); a
  `range_verification` case in which the approved intent differs from the bound content object in
  one action member, failing §11's `intent_action_mismatch` second clause; a mutant that skips that
  clause; and rule 7's check that no `ApprovalRequested` names a decision with `ask_suppressed`.
- **Verify owner copies against the control stream** (DEC-177 item 24). §9.1 rule 16 checks only
  that an owner copy's `causation_id` is non-null, because `append` and §11's per-range checks read
  one stream. A cross-stream check in `mandate journal verify` resolves it on `ctl:{workspace_id}`
  and fails unless it names an `OwnerCommandIssued` of the copy's command and subject, submitted
  before the copy, copied at most once into each event type on the agent stream, as the generator's
  `check_owner_copies` does for the vectors; §11 names the check and its code, with a vector.

Minor and nit findings from round 1 of the independent review of the journal spec v0.5 change
([DEC-177](04-decision-log.md#decisions); held back by the freeze rule, one row each):

- **A copied `AgentModeChanged` names its `AgentModeApplied`.** When the agent runtime copies a
  mode change the executor originated, the copy's `causation_id` names the originating
  `AgentModeApplied` (§2); add the rule to §9.1 and a vector for it.
  *Narrowed (DEC-177 item 24):* §9.1 rule 16 covers the owner's pause, resume, and Stop, which name
  their `OwnerCommandIssued`. What remains is the copy of an account-stream mode change (reasons
  `restriction_changed` and `awaiting_reconciliation`).
- **Bound the model-supplied free text.** `ModelOutputRecorded.model_id`, `model_version`,
  `direction`, and `invalidation` are any non-empty text: bound their length or check them against
  the model registry and the directions v1 allows, and scan them for personal data as §6.4
  requires.
- **`KillSwitchActivated` records the initiator's step-up**, or names its `OwnerExitRequested`
  (for example as `causation_id`), so the switch's own record shows what authorized it.
  *Done for the owner's switch (DEC-177 item 24):* §9.1 rule 16 makes an owner's
  `KillSwitchActivated` name its `OwnerCommandIssued`, which carries the step-up evidence (§9), as its
  `OwnerExitRequested` does. What remains is a `platform_operator` switch, which names no owner
  command.
- **`journal.yaml`'s header notes the DEC-176 exception.** Its line "Changing these vectors requires
  founder approval" predates DEC-176, under which agents accept changes that only tighten or
  reconcile.
- **Tidy the journal generator.** Add `from __future__ import annotations` to
  `reference/journal/generate.py`, whose forward reference in `T` fails on Python before 3.14, and
  run ruff over `reference/`.
- **Correct `mandate-refcases`' journal module doc.** `crates/mandate-refcases/src/journal.rs` says
  the vectors are version 2; they are version 3.

Minor findings from round 2 of the same review (#320 round 2;
[DEC-177](04-decision-log.md#decisions) item 19; held back by the freeze rule, one row each):

- **Test both directions of §9.1's biconditionals (#320 round 2).** Rules 1, 3, 4, 5, and 13 each
  have an invalid draft for one direction only (for example a sell labelled `open`, but no buy
  labelled an exit), so a validator checking only that direction passes. Add a draft for the other
  direction of each, and split each rule's mutant into one per direction, each caught only by its
  own draft.
- **Vectors for the untested envelope members (#320 round 2).** No draft tests that `artifact_refs`
  is exact (sorted, de-duplicated, and no ref the payload does not hold), that `pii_refs` is
  ordered, or that `actor.build` is required for a `system` or `agent` actor. Add an invalid draft
  and a mutant for each.
- **Assert §9.1's report order (#320 round 2).** "The first violation, in this order" is never
  tested: every invalid draft breaks one rule. Add drafts that each break two rules of different
  ranks (for example an extra member and a rule-4 breach, or rules 4 and 5 together) and expect the
  earlier one, with a mutant that reverses the order.
- **Trace mandate spec §5.10's `OwnerExitRequested` row to §9.1 (#320 round 2).** Its field list
  ("instrument or scope, bid shown and confirmed, user (opaque), step-up evidence") lacks
  `step_up_status`, which §9.1 requires on every owner exit (DEC-177 item 11). Add it there, and
  check the §10 row the same way.
- **Correct §9.1's citation for a removed instrument (#320 round 2).** `exit_origin`'s row cites
  mandate spec "(§2.2, §2.3)" for a removed instrument, where mandate spec §6.1 cites §2.3 and §5.9
  cites §2.3 and §8.6. Cite the sections the mandate spec gives.
  *Done (DEC-177 item 22):* the row cites mandate spec §6.1's purpose table, §2.3, and §8.6.
- **Reconcile #320 and #321 when the second merges (#320 round 2; DEC-177 item 20).** Both call
  themselves journal spec v0.5. Whichever merges second:
  (a) resolves the textual conflicts in the Status line and the v0.5 change-history bullet and
  renumbers itself to v0.6, in the §9.1 heading, the `journal.yaml` header, and `generate.py`'s
  `spec` string, then runs `reference/journal/generate.py --write` and `cargo xtask refcases --write`;
  (b) adds `ApprovalRevalidated` to §9.1 as a third allowed cause of `IntentProposed` (rule 10), and
  records a decision on whether `intent_action_mismatch` compares an approved intent against the
  approval's bound fields;
  (c) states a §9.1 causation rule for #321's §2 copy rule ("`causation_id` pointing to the owner
  command"), which the chain's `OwnerExitRequested` events (seqs 6, 9, 12) and `KillSwitchActivated`
  (seq 13) break with `causation_id: null`, and regenerates the vectors to meet it;
  (d) keeps one definition of `ask_suppressed`, which both add, and drops #320's hedge "once M7's
  change lands" in `DecisionMade`'s table;
  (e) re-checks each §9.1 citation of mandate spec §6.1 against the merged text, since §9.1 cites
  §6.1 for a rule only #321 states.
  *Done (DEC-177 items 21 to 24), with (b)'s vectors deferred:* #320's change is v0.6 (a); rule 10
  and §11 allow and bind the `ApprovalRevalidated` cause, whose range case waits for the row
  "Close the approval events' schemas in §9.1, with vectors" (b); rule 16 and the chain's causation
  ids (c); one `ask_suppressed` definition, citing mandate spec §6.4, and no hedge (d); and §9.1's
  §6.1 citations name what §6.1 states (e).

Minor and nit findings from round 1 of the independent review of the v0.5 reconciliation
([#340](https://github.com/kunwarshivam/mandate/pull/340); [DEC-177](04-decision-log.md#decisions)
item 25; held back by the freeze rule, one row each):

- **Name the members an approved intent is compared on (#340 round 1).** Rule 10 compares the
  seven intent fields, but the `IntentProposed` sentence and §11's second `intent_action_mismatch`
  clause mean the five `ApprovalRequested` binds (`instrument_id`, `side`, `qty`, `limit_price`,
  `purpose`; not `type` or `tif`). List the compared members in both places, so an approved intent
  is never refused over a `tif` its approval did not bind.
- **Enforce that `ApprovalRevalidated` precedes its intent in the same batch (#340 round 1).**
  §9.1 allows an `IntentProposed` caused by an `ApprovalRevalidated` with result `act` "that
  precedes it in the same batch", and no rule checks either the order or the batch.
- **State `OwnerCommandIssued`'s scope and subject (#340 round 1).** `check_owner_copies` models the
  command as carrying `scope` (including `instrument`) and `subject`, which §9's control-stream row
  does not state; add them to the row, or change the oracle to the members the row names.
- **Test rule 16's guard and report position (#340 round 1).** No draft shows that rule 16 is
  checked only on a well-typed payload, or that it reports after the subject rules; add a draft
  that breaks rule 15 and rule 16 together, and one with an ill-typed payload and no cause, each
  with a mutant.
- **A range case with `from_seq` above 1 (#340 round 1).** Every `range_verification` case has
  `from_seq: 1`, so `mode_event_mismatch.unresolved`'s full-chain guard is untested: add a case
  verifying from a later seq whose `mode_event` names an earlier event, expecting no failure, with
  a mutant that drops the guard.
- **`outside_session_exit_defer_code()` checks with `assert` (#340 round 1),** which `python -O`
  removes; raise instead.
- **Note: the `exit_origin` citation fix pulled a round-2 minor forward (#340 round 1, nit).** Item
  22 corrected the removed-instrument citation that #320 round 2 had backlogged; no action.
- **`propose` hard-codes `RequestedBy::Agent` (#369 round 1, owed by E10-6).** DEC-262 item 2
  stamps every buy `propose` sizes as the agent's own, because no owner or client request path
  calls the builder yet. The request path E10-6 adds must carry the authenticated channel's
  requester to the order it proposes (§6.2 step 5a) rather than reuse `propose`'s stamp, with a
  test that a client's request reaches `decide` as `client`.
- **The client-ceiling sweep has no delegation dimension (#369 round 1, owed by E8-8's tests PR).**
  `Autonomy` holds no `delegations` until E8-8 (DEC-262 item 5), so
  `every_rule_default_admission_and_requester_obeys_the_client_ceiling` and
  `a_client_opening_is_never_auto_and_every_other_request_decides_as_before` cover rules, defaults,
  admission and requester only. E8-8's tests PR adds live, spent, expired and suspended delegations
  to both and asserts that none lifts a client's order (MI-26, MI-30).

From journal spec v0.8 §9.3's review (#470 round 2, DEC-403):

- *Decision needed (founder, Proposed under DEC-79; the dependency registry is founder-owned):*
  add `jsonschema` to the `python/` project, so `reference/journal/risk_state.py`'s
  `documents.valid` can run `reference/mandate`'s full schema and semantic validators instead of
  its narrow check (#470 round 2, minor 5). Until then the dependency is not added. The narrow
  check holds a stored version to the proven base changed at its one listed path, with V-013's
  order and a floor in (0, 1]. It does not stand in for the mandate schema, for any other V-rule,
  or for a wrong entry in `VERSION_PATHS` itself, which it shares with the document builder (minor
  4). `drafts.classification` catches the dangerous sub-case, a wrong value that changes the §9.2
  verdict. All six of today's documents were checked against the full validators by hand.
- **§9.3's vectors: three coverage gaps (#470 round 2, minors 1, 7 and 8).**
  - **Report orders.** Done in journal spec v0.10: the 29-then-30, 30-then-33 and 31-then-32 orders
    each have a draft and an `order.*` seeded bug (DEC-404 item 9). Rules 29 and 33 can never both
    fire, since 29 needs `risk_increasing` and 33 needs anything else.
  - **`universe_size_after`** is checked against nothing, not even the fold of the section's own
    drafts.
  - **`risk_state.classify`** raises `ValueError` on a path it does not encode instead of reporting
    a `drafts.classification` problem. It fails closed either way, but a reported problem reads
    better.

From the #485 chain's round-3 review (#494 and #496; the coordinator's ruling, 23:58Z, freeze rule), and #508's round 2 (m3′):

- **E7-4: pin that a crowded-out discretionary exit's denial is terminal** (#494 round 3, minor 1;
  DEC-410 item 3). `a_discretionary_exit_with_nothing_left_is_held_overnight_then_refused_at_the_open`
  ends one tick after the denial, and the oracle keeps only the latest verdict, so a denial repeated
  at every tick would pass. Assert one `GateDecided deny` record for that intent over several ticks.
- **E7-3: the kill switch between rungs** (#494 round 3, minor 2; DEC-410 item 5). Its only script,
  `a_kill_switch_between_rungs_never_over_sells`, is `pending E7-3`; the claim in §5.5's agent row is
  pinned only when E7-3 lands.
- **E7-4: a sell beside a parked remainder that ends unsold before the rung is due** (#494 round 3,
  minor 4; DEC-410 item 6). With the agent paused across the open, the sell's refusal frees its room
  and the rung then sends the whole remainder. No script pins that direction: the existing ones shrink
  the position (`Move::Triggered`) or refuse the ladder's own rung (`Move::Reject`).
- **E7-4: the window guard's doc says no script reaches an unparked remainder today** (#496 round 3,
  minor 1). `Desk::within_position`'s parked-only assertion should say in its doc that it holds by
  construction today and exists to turn red if a change opens the window.
- **E7-4: `Held` gains an `Unknown` state** (#496 round 3, minor 2). The oracle's
  `unknown_order_in_flight` check fails every such hold because the script's broker never leaves an
  order unknown. With an `Unknown` state in `Held`, it becomes a coincidence check against the
  oracle's own venue record.
- **E7-4: hoist `overtaken` out of `release_waiting`'s per-exit loop** (#508 round 2, m3′). It runs
  once per waiting exit, not once per instrument; with the `Unknown` arm in its guard the repeats ask
  nothing, so this is wasted work only.

From #468's round-5 review (the coordinator's ruling, 09:34Z on #468; freeze rule):

- **E7-4: the oracle checks the park alert its exemption relies on** (#468 round 5, m2).
  `Desk::within_position` accepts a parked remainder held by a paused or stopped agent because rule
  13 permits the hold and the park alerts it, but `Desk::alerted` is set only by
  `expiry_unreplaceable`, so the alert half is asserted, not checked. Record the park's
  `GateDecided … parked` and its notification in the oracle and require it, or drop the clause
  from the doc. The park's single alert is pinned elsewhere today.
- **E7-4: wrap `within_position`'s long doc line** (#468 round 5, m5). A sentence spliced onto an
  existing line left a line of about 190 characters; `fmt` does not wrap doc comments.

From #518's round-2 review (the coordinator's ruling, 00:28Z on #518; freeze rule):

- **E7-4: bound DEC-421's correction to the reconciliation the report asked for** (#518 round 2,
  major 2; DEC-421 items 1 and 5). Its own story, tests first. Today an unapplied report is netted
  against any unattributed sell applied after the order was submitted, so a sale the owner makes at
  the broker cancels the correction and protection over-covers (Σ 7 against 4 held). A report that
  overstates its fills under-covers with no bound. Ending the correction when the reconciliation the
  report asked for completes gets both right: a fill it attributes closes the correction, and one it
  cannot attribute has already left the position. The same story decides whether the gate's
  `available` takes the reading.
- **E7-4: pool a sell fill whose named order the fold does not know** (#518 round 2, minor 3).
  `fill_applied` pools only when the payload names no order, so a `FillApplied` naming an order the
  fold does not know takes shares off the position and joins neither. `orders::fill` never writes
  that payload today; keying the pool on "no order found" makes the fold total over it.

From #524's ruling ((A), 05:52Z on #524) and the reviews of #534 and #535 (the coordinator's ruling,
05:08Z on #534; freeze rule):

- **E7-4: count an exit allowed with no order yet in the gate's `available`** (#524, claim 1; a
  possible tightening, with no defect behind it). On seed 15 the gate allows 7 of 10 and never
  over-commits. The defect there is the wait, which DEC-425 fixes. Counting an allowed exit that
  waits with no order as an open sell would deny a later exit that cannot fit, rather than let it
  be allowed and wait. Decide it in its own story, with a test that a denied exit is not lost if the
  waiting one is then abandoned.
- **E7-4: DEC-424 item 5 states the lone entry's lifecycle as the code implements it** (#534 review,
  minor 1). The entry is removed in two cases (absorption into a sequence, a `rung_short` that sends
  nothing), replaced in one (the exit's own next rung), and in every other case stays while counting
  zero. Say so, so the item stops implying the entry ends with its exit.
- **E7-4: move the saturation pin and make it fail loudly** (#534 review, minor 3; #535 review,
  minor 3). `gate::remainder_tests::an_over_filled_rung_counts_nothing_and_the_gate_still_decides`
  belongs with the fix it pins. Carry it in the next tests correction to `remainder_tests`, and make
  it fail, not pass silently, if `parked`'s rung id changes: today `get_mut` on a missing id skips
  the over-fill and the assertions still hold.
- **E7-4: `ladders` grows without bound when a lone exit finishes by filling** (#535 review,
  minor 1). Nothing removes the entry of a lone ladder whose rung fills completely, so it stays,
  counting zero. Remove it when its exit's last rung fills, or when the intent is no longer live.
- **E7-4: `remainders`' long doc line, and one source for a lone ladder's intent** (#535 review,
  minors 2 and 4). Wrap the doc line past 100 columns. And `ladders`' key `(InstrumentId, IntentId)`
  repeats `LoneLadder.intent`, so the two can disagree: enforce the agreement where entries are
  inserted, or drop the field and read the intent from the key.

From #559's round-1 review (#524's tests; the coordinator's ruling, 09:35Z on #559; freeze rule):

- **E7-4: model #524's refused-cancel answer as §5.7's query answer** (#559 round 1, minor 2).
  `a_refused_protective_cancel_found_live_is_asked_again` feeds the answer as
  `Input::BrokerUpdate(BrokerUpdate::Order(..))`. The path is the same, but its sibling
  `a_refused_cancel_is_queried_then_asked_again_once` uses `Input::Broker(Ok(BrokerOutcome::Order(..)))`,
  which is literally the answer to the query the test asserts. Use that.
- **E7-4: #524's invariant fails on a resting id with no order record** (#559 round 1, minor 3).
  `an_exit_waiting_on_protection_always_has_its_cancel_asked` reads an id in `protection.resting`
  that `orders` does not know as outstanding, so an exit waiting on it forever would go unreported.
  Fail on the unknown id instead.
- **E7-4: ask the per-move check of the initial state and after the refold** (#559 round 1,
  minor 4). `rule_13_script_checked` asks it only inside the step loop. Nothing is reachable there
  today, since `protected(..)` starts with no sequence.
- **E7-4: two doc lines in #559 run to 101 columns** (#559 round 1, minor 6). Wrap them with the
  `remainders` row above.
- **The saturation-pin row's wording** (#559 round 1, minor 7). The 05:08Z ruling put it in the
  tests-correction row #532 started; the row added here points at the next tests correction to
  `remainder_tests`. The substance is the same, so no change is needed.

From #576's round-1 review (DEC-425; the coordinator's ruling, 12:12Z on #576; freeze rule):

- **E7-4: pin or remove `settle`'s `!handed` guard on the re-ask** (#576 round 1, minor 1).
  Calling `cancel_resting` for a handed-on sequence too leaves every test green, and the diff's
  mutants include none that drops the guard. The review found no reachable state where it
  diverges: `protection_holds && handed` implies `!fits`, so an unheld waiting exit has already had
  `overtaken` ask in the same step, or the instrument is over-committed, where cancelling is what
  rule 12 wants. Either pin a state where it matters or drop the guard and say why.
- **E7-4: a `fits` unit pin with a held exit beside the placement** (#576 round 1, minor 4).
  `a_reported_fill_not_yet_applied_makes_protection_overhang` has no waiting intents, so it reads
  the same under the old and the new measure. One case with a held exit beside the placement would
  pin the subtraction DEC-425 item 5 made the one measure.

From #528's round-2 review (DEC-411; the coordinator's ruling, 05:19Z on #528; freeze rule):

- **The reference does not model V-002's apply-time re-check** (#528 round 2, major 1). V-002 says it is "checked at
  validation and again atomically when a version is applied", but `reference/mandate/ref.py` checks it in `semantic()`
  only, and no case or fuzz covers a version that validated and then fails V-002 when applied. V-047 has the same gap,
  owed on E10-1's row; this row is V-002's.
- ~~**§5.7's parenthetical says a workspace, where V-047 refuses a version** (#528 round 2, minor 1).~~ Done ([DEC-429](decisions/DEC-429.md)).
  `docs/specs/mandate.md`'s §5.7 says "a single-user workspace (never one under `independent_approval_required`, which
  V-047 refuses at validation)", but such a workspace exists after confirmation (DEC-411 item 6). Reword to "a
  loosening version is refused there under `independent_approval_required` (V-047)".
- ~~**"Pin" for cases that are pending** (#528 round 2, minor 2).~~ Done ([DEC-429](decisions/DEC-429.md)). §6.7 and DEC-411 item 6 say MC-W50, MC-W53, MC-W54
  and MC-W56 pin the item-6 state, but all MC-W cases are `pending` on E6-13 and enforce nothing yet. Say "specify",
  or name E6-13 as the story that makes them bind.
- ~~**V-047's reason list drops risk-increasing changes** (#528 round 2, minor 3).~~ Done ([DEC-429](decisions/DEC-429.md)). The row rests on deployment, the
  high-water-mark reset and the tripwire lift; §4.3 also lists a risk-increasing change, which a lone user cannot
  make either. Restore that clause (the latched-floor reason stays out, per round 1's M1).
- ~~**`mutants.py` cannot carry two natural V-047 bugs** (#528 round 2, minor 4).~~ Done ([DEC-429](decisions/DEC-429.md)). Inverting the policy, or ignoring it,
  makes V-047 fire on the base mandates, so `bases.py`'s import-time assertion crashes the probe and `verdict()`
  scores it `ERROR`, not caught. Say so in the module docstring, so nobody adds one and reads the `ERROR` as a catch.
- **The README's mandate case counts lag `main`** (#570 round 1, minor 2). `README.md`'s mandate row and its spec
  table give 433 cases where the file holds 441 after #570, and were already two behind before it. Update them, or
  derive them from the case file as the §11 row below asks for the spec.
- **§11's case count is unchecked prose** (#528 round 2, minor 5). `main` said 427 where the file held 429. Add a
  `cargo xtask` assertion that §11's count matches `docs/specs/reference-cases/mandate.yaml`.

From the round-1 review of the workspace services API spec ([#560](https://github.com/kunwarshivam/mandate/pull/560), minors; [DEC-436](decisions/DEC-436.md)):

- **The CLI as a second writer** (minor 1). Carry DEC-436 item 3's sentence into spec §1.3: the
  writer epoch fences whichever writer is stale, and the API is `Fenced` until it re-takes the
  epoch, so the cost of the CLI fallback is visible.
- **`mock-runtime.tsx`'s path** (minor 2). Spec §4.9 names it under the fixtures column; it is
  `web/src/lib/mock-runtime.tsx`.
- **One list of reducing shortcuts** (minor 3). Make API-7's list and §5.1's frozen-stream sentence
  name the same set, so the freeze check cannot be built before the shortcut carve-out.
- **Who makes the organization kill switch's fan-out calls** (minor 4). Identity spec §4.2 gives the
  org scope to org owners and admins, who need no workspace membership for it; spec §5.4 says the
  client issues workspace-scope calls. Name the principal and route.
- **Cite the notice payload, do not restate it** (minor 5). Spec §3.9 should point at the
  notifications spec §4.2 for the payload's members.
- **API-2's test reads the workspace from the path only** (minor 6). Assert no route reads the
  workspace from a body member.
- **One wording for a client's reads** (minor 7). §3.7's client column and §3.8's `read` scope say
  the same rule two ways; keep one.

From the round-2 review of the workspace services API spec ([#560](https://github.com/kunwarshivam/mandate/pull/560), nits and cross-document notes; [DEC-436](decisions/DEC-436.md)):

- **Offer the kill switch alone on the revoke-on-compromise screen** (workspace API spec §5.6), as
  the alternative that keeps the connection so protection can be re-placed and exits re-driven.

From the round-6 review of the flatten adapter's implementation PR ([#596](https://github.com/kunwarshivam/mandate/pull/596), minors; [DEC-449](decisions/DEC-449.md), [DEC-451](decisions/DEC-451.md)):

- **Expose the journal's unsealing rule from `mandate-journal`** — `stored_draft` (or a
  `StoredEvent::draft()` that also checks the columns) is private, so the flatten adapter holds a
  second copy of the three assigned-field names, the digest form, and the re-seal, the exact
  things that must not drift from `seal`.

From the workspace API contract's drift rule (DEC-683, E10-10):

- **A `cargo xtask` check for stale planned markers.** List every `(planned: <story>)` in a spec
  table and every `x-planned` value in `schemas/`, with its story's state, and fail on a marker
  whose story is done. Until it exists, removing a story's markers is part of its done-definition.
- ~~**Rust JSON-pointer checks refuse control characters**, as the schemas' pointer pattern does
  (`[^/~\u0000-\u001f]`), wherever `mandate-api` checks a path (E10-10 implementation).~~ Done
  ([#1013](https://github.com/kunwarshivam/mandate/pull/1013): `is_pointer` refuses `U+0000` to
  `U+001F` in a violation's `path`; A1 implementation part 3 applies it to a confirm's paths and a
  `202`'s `dropped`).
- **Journal the members an API-7 operation dropped** (DEC-682 item 27): the command event names the
  JSON pointers its `202` listed in `dropped`, so the record shows what the server ignored. A
  journal spec change first.
- **Run `schemas/workspace-api/`'s checkers in CI** (`check_examples.py`, `check_planned.py`, and
  the mutation sweep) from a `cargo xtask` job; until then reviewers run them.

From the independent review of the E10-10 A1 implementation, part 1 ([#993](https://github.com/kunwarshivam/mandate/pull/993), minors; [DEC-681](decisions/DEC-681.md) item 10):

- **Locate a custom refusal on an object's last member by its name.** serde_json reports a custom
  error such as `non_canonical` after the object's closing brace when the bad member is the last
  one, so `decode` points at the parent. Locate custom errors by member name, as DEC-681 item 10
  says ("where serde can name it"). The body is still refused, and no sibling is ever named.
- **Refuse invalid UTF-8 inside a string value as `malformed` at `""`.** Today it is refused as
  `type` at the member. It is still refused either way.

From the independent reviews of three CI and xtask conflict-and-queue fixes ([#768](https://github.com/kunwarshivam/mandate/pull/768), [DEC-538](decisions/DEC-538.md); [#770](https://github.com/kunwarshivam/mandate/pull/770), the behaviour-only rows as one file a row; [#773](https://github.com/kunwarshivam/mandate/pull/773), the feature map as one file a feature; minors):

- **The mutation plan's tests** (#768).
  - `the_planned_shards_test_the_gates_mutants_and_no_more_per_shard` seeds a diff whose mutants
    are all in one package. Add a second mutated crate to its fixture, so the plan's sum across
    packages is pinned as well as its listing.
  - `ci_sizes_the_mutation_matrix_from_the_plan` pins exact `ci.yml` lines, so rewording one
    fails the test even when the wiring still holds. It fails safe; parse the jobs' keys instead
    when it next gets in the way.
- **The behaviour-only rows** (#770, `xtask/behaviour-only/`).
  - Pin the exact refusal message for a non-`.toml` file in `a_malformed_or_misnamed_row_is_refused`,
    as the other refusals' messages are pinned.
  - Read the rows through `git ls-files` rather than `read_dir`, as `ci pending` reads test files, so
    an untracked or ignored file in the directory cannot change the gate's verdict.
  - Say in the README that two tests whose paths differ only by `::` against `__` derive the same
    file name, so the second row cannot be added until one is renamed.
- **The feature map's directory** (#773, `.cursor/skills/verify-mandate/features/`).
  - Check or refuse what else sits in the directory: today the README and any non-`.md` file are
    skipped without a word.
  - Refuse two feature files with the same `# ` title, which `--index` would list twice.
  - Add a README to the drift oracle's fixture directory beside the feature files, so the test shows
    the README is never read as a feature.

From E7-16's M2 implementation (`mandate-mcp`, claim #859; the shared check is lane L2's):

- **E7-16: switch `mandate-mcp`'s private fund-movement tokenizer to `mandate_domain::fund_movement`**
  once lane L2 lands it (E7-12, DEC-839 item 3). `client.rs`'s `moves_funds`, `words` and
  `FUND_TOKENS` then go, and the crate gains its `mandate-domain` dependency, so the connector and
  the scope check cannot disagree on a name.

- **E7-24: require `iss` when the server sets `authorization_response_iss_parameter_supported`**
  (RFC 9207 §3, [DEC-855](decisions/DEC-855.md) item 4): a callback without `iss` is then refused.

From E7-23 B2a's implementation ([DEC-838](decisions/DEC-838.md) item 5, [DEC-841](decisions/DEC-841.md)):

- **Delete the transitional Alpaca profile** (`mandate-executor`'s `shape::transitional_alpaca`,
  which `ExecutorState::new` starts with, and the default body of the shell's
  `Connector::profile`) in B3, before the first non-Alpaca executor
  path merges. Until then an executor built without a profile is Alpaca-only by contract. Once every
  constructor passes a profile, `ExecutorState`'s profile stops being an `Option` and DEC-841 item
  2's defensive path goes with it.

From the closure of the anchor and segment records ([DEC-783](decisions/DEC-783.md)):

- Close `SegmentEvicted` (journal spec §6.2, §9): which segment left the hot store, by its
  `SegmentExported` manifest hash, and when. Left out of DEC-783 by the lead's ruling, so a hot-store
  eviction record stays prose until a story needs to read it.
- The anchor stamp record (§10, DEC-783 item 8): a control-stream record, `AnchorStamped`, that names
  an `AnchorComputed` as its `causation_id` and carries a timestamp token for its root. It backfills
  the token of an anchor appended during a timestamping outage, which until then is not a trusted
  start (§9.14), and re-stamps an anchor before its token expires.
