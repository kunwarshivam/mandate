# Coordinator handoff: mandate, 2026-10-14 (the sessions of 10-13 and 10-14, state at close)

You are taking over as **merge coordinator** for `kunwarshivam/mandate`. Read `AGENTS.md`
first: it is binding and overrides anything here. This file **supersedes** the previous
handoff on branch `claude/gallant-mccarthy-a3qgpx` (commit `283188e4`) — that branch's older
sections still carry the process detail worth keeping (the DEC-175 approval-and-merge
pipeline, `.github/scripts/merge-approved.sh`, `merge.yml`, the squash-merge gotchas, the
DEC-77 tests/implementation discipline, the DEC ranges); everything state-like lives here.

## What the 10-13/10-14 sessions merged (nine PRs, each through the full pipeline: green CI,
independent different-model review, DEC-175 approval; oldest first — every review verdict and
its `Coordinator-approved-head` line is in the merged PR body)

- **#1323** (`29d0914c`) + **#1324** (`3c57494b`): the docs sweep of the #1295/#1315 review
  notes, split because the spec guard refuses spec and code files in one PR (#1324 carries the
  agent-harness spec line alone, DEC-475 item 6).
- **#1325** (`7f849845`): the G1a live door tests — three stubbed frames in
  `crates/mandate-paper/src/door.rs` with final signatures, 7 pending pins; DEC-918 filled
  Reserved → Accepted here.
- **#1326** (`518db19a`): the shell-side LT-10 tests — the pure exported `watch_may_end` seam
  and 9 pending pins (DEC-912 item 3; DEC-475's private-Session boundary ruled no-breach).
- **#1327** (`e04f9521`): #1313's review Minor 2 closed — the PG-gated call-site evidence pin
  through a real schema-per-test Postgres journal, asserting the `Some` at run()'s outcome with
  full value equality (DEC-914 item 6, DEC-109 (7)).
- **#1329** (`cf88e7f2`): the control-stream tail tests — the `tailed` fold stub and 4 pending
  pins. **Row identity:** the R0-gap lists' "runtime grant→submit continuation (stream L /
  M7's remainder)" resolves to **the shell's control-stream tail** per the M7 brief's
  Decisions-needed 5 and DEC-155 items 2 and 5.
- **#1328** (`14939ab3`): the G1a implementation — the door in `run()` ahead of the shell's
  fold and every credential read. Carries a **coordinator ruling under DEC-77's 2026-09-27
  amendment**: one arm of the merged pin `every_refusal_before_the_credentials_reads_none`
  (`crates/mandate-paper/tests/run.rs`, the `"live"` case) moved from the shell's fold to the
  door's refusal — that arm had frozen run()'s pre-G1a answer, inverted against the
  now-Accepted DEC-918 (it passed all four wrong wirings and failed the faithful one).
- **#1330** (`72c67ec6`): the tail implementation — the fold per row verbatim, and the
  **open-replay install** in `Session::open_cycle` (one read of `ctl:{workspace}` at the
  cycle open, no cursor of the shell's own). The per-wake re-read's carrier
  `wait_for_the_grant` has no production caller and the `live` arm stays compile-only —
  disclosed in the body, ruled letter-supported. **Disclosed finding, still open:** the
  **EI-11 ledger gap** — the install hands rows but does not state-fold them, so check 6's
  `step_up_reused` cannot fire across two events of one read; closing it is a **runtime
  row's** (a §9.2 fold catalogue or an owner-input fold arm).
- **#1331** (`c1d09835`): the LT-10 implementation — the two-point-per-wake ask replacing the
  bare `TERMINAL_STATUSES` read, four in-module probes, and the fresh
  `CoreExecutor::protected` read. **This row is coordinator-salvaged:** both builder sessions
  were cancelled mid-run without reports; snapshot commits (`47d84f85`, `15abfe3b`) salvaged
  their work and the coordinator completed the row (provenance disclosed in the PR body).
  **Discovered:** the terminal-uncovered state is unreachable through the production watch
  today (one bracket submission; an in-doubt placement counts as covering; positions fold only
  via fills) — which is why the CI-missed `protected → Ok(true)` mutant lived; the
  adapter-level hold
  `the_core_executors_protection_answer_is_its_own_fact_over_a_held_unprotected_position`
  kills it.

Every implementation PR took one FAIL review round before its PASS, every fix a
review-ruled in-module probe under DEC-77's amendment primary path (#1328: the missed
`AgentStopped` arm; #1330: the unpinned install — CI's `Ok(())` mutant and three wiring
escapes; #1331: the missed covered mutant). Both `mandate-paper` and `mandate-shell` hold
**zero pending tests**.

## Session docs (merge early in your session)
Branch `claude/session-docs-1014` (docs-only, base `main`, the #1322 precedent): the M6
tracker row trued to the nine merges, the tracker's "Next, in order" list corrected to the
merged state, the header's Last-updated line, and the two sessions' work-log paragraph. The
task brief `docs/project/tasks/first-live-trade.md` was verified and **left alone** — its
M6/R0 rows name the completed rows only as plan prerequisites and targets, never as open.

## Where the paths stand
- **Paper:** code complete through E3, every E2 prerequisite verified satisfied. **E2, the
  founder's manual run, is fully unblocked and is the ask**; record the open-orders read after
  the fill (DEC-878).
- **Live (DEC-529, target 2026-10-23, deadline 2026-11-02):** every coordinator-drivable R0-gap
  row the tracker and this handoff's predecessor recorded is merged (the G1a door, the LT-10
  watch, the tail). The remaining rows: **S2, B3, K1b, B2b and the C and G rows**; E7-23 to
  E7-25 stay in the backlog; then the R0 rehearsal — which needs the founder's DEC-529
  live-door items and the paper path's E2. Open follow-ons disclosed in PR bodies, both
  legitimate later rows: the **EI-11 runtime fold** (above) and the **G1b frames' live-arm
  call sites** (the per-wake re-read), which wait on a live carrier.

## The founder-held board (none of it was coordinator-mergeable; post or carry)
1. **DEC-917 (#1321)** — one word frees #1320. #1321 (the ES-22 spec-guard tests-carveout) is
   PASS-reviewed with its Coordinator-approved-head line already written; #1320 (the E7-17
   `connection_ranges` vector) is PASS-ready behind it. Accept = merge #1321, merge `main`
   into #1320's branch, then merge #1320. Decline = #1320 stays blocked; nothing else at risk.
2. **#878** — the Cloudflare Workers/OpenNext cutover (DEC-731), held for the founder as before.
3. **DEC-877 item 4** — the shell refuses every post-entry submission as `SecondSubmission`, a
   protective OCO after a partial fill included, against rule 13; unreachable while the run
   sizes one share. Recommendation (a): exempt protective orders.
4. **DEC-914 item 3 (collar_x)** — paper re-implements the tier byte-identical (disclosed,
   double-pinned); accept and backlog an exposure PR, or direct the exposure now.
5. **The E7-28 build-guard offer** — a guard inside the build itself refusing to compile `live`
   outside the one compile-only CI job; it only tightens, so build it on the founder's yes.
6. **DEC-915's legal numbering gap** — `docs/project/decisions/` on `main` holds no DEC-915
   (the files jump DEC-914 → DEC-916 → DEC-918; DEC-917 is #1321's open branch). The founder
   decides whether the skipped number's record is backfilled or the gap stands; check open
   branches before reserving numbers near it.
   Still carried from the earlier lists: **#1296 keep-or-revert** (recommendation: keep).

## Backlog fixtures named in this session's PR bodies
- The **WAAPI watcher id-matching** precision minor (#1318's review).
- The stale **`error.rs` SecondSubmission message** — "the tracer places one order per run, and
  the executor asked for another" and the doc line above it still name the deleted binary
  (#1323's listed-not-fixed; pinned printed output, so it needs its own behavior change plus
  test pins).
- The **extreme-load vitest flake** class (the owl-runs' e2e: `mode.test.tsx`, agent-header,
  routes under extreme load — quiet runs green).
- The **door-level refusal-order pin** (#1325's review's named backlog fixture) — the four
  run-level wiring vectors are held by probes; a tests-level door-order pin remains a fixture
  to write when a fixture can express it.
- The **docs sweep items**: `tests/lt10.rs`'s now-stale pending-era module doc (#1331 restored
  it to #1326's bytes on purpose; a later docs sweep trues it); #1323's ragged-reflow note; the
  historical `tests/fixtures/tracer/` path names.

## Reviewer routes, as of 2026-10-14
- **Step-5** and **Mimo-2.6**: both carried heavyweight two-round reviews this session — each
  found a real missed mutant or escape set and verified its kills by re-planting in scratch.
  First choices for heavyweight rows. (Mimo-2.6 passed #1326, #1329, #1330 and #1331; Step-5
  passed #1328 after its FAIL round.)
- **Ling-3.1 flash**: passed #1327's review (careful, heavyweight), then rate-limited; retry it
  before falling back. Model id: **`opencode/ling-3.1-flash-free`** — the `-3.1-free` id is
  stale.
- Builders on the default GLM route were healthy throughout. Wrong-tree/two-dot review misreads
  stay a known free-tier failure class: the dispatcher's worktree-first, `git rev-parse HEAD`
  check and three-dot-diff brief template prevents them (carried from the prior handoff).

## Operational warnings
- **The `gh` keyring flips its active account between `kunwarshivam` and `kunwar-vp`. ALWAYS
  check `gh api user` before any label or approval op.** The merge script refuses a
  wrong-identity label — and GitHub's auto-merge may then merge anyway once a second, correct
  label add aligns with green CI (#1329's case; verified compliant post-hoc;
  `merge-approved.sh` remains the intended path). At this session's start the keyring
  resolved to `kunwarshivam`; do not trust that to persist.
- **Cancelled builder sessions:** two long builder sessions on the #1331 row were cancelled
  mid-run without reports. **Snapshot-salvage their worktrees before reaping** — the salvage
  became the merged row; an unreaped worktree is the only copy of uncommitted work.
- **Squash-merge gotcha (carried):** merging `main` into a branch stacked on a squash-merged PR
  resurrects deleted ignore lines and rows; after every such merge, delete them again and
  check `git diff origin/main -- '*/tests/*'`.
- Worktree discipline that held all session: private worktrees with private `CARGO_TARGET_DIR`,
  `CARGO_INCREMENTAL=0`, build only the touched crates, never the whole workspace locally,
  disk floor 5 GB, remove worktrees after.

## Session-end duties (what this session already did, what yours owes)
- This session trued the M6 row, the Next list and the header, appended the log paragraph
  (`claude/session-docs-1014`), and pushed this handoff branch
  (`claude/coordinator-handoff-1014`). Yours: post the summary on issue #165, carry the
  founder's morning list (the board above), update the tracker and log at close, and write the
  next handoff branch fresh.
