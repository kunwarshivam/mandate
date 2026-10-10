# Coordinator handoff: mandate, overnight lanes (2026-10-10 ~01:10 UTC)

You are taking over as **merge coordinator** for `kunwarshivam/mandate`. The founder asked for all
lanes to keep moving through the night. Read `AGENTS.md` first: it is binding, and it overrides
anything here.

## Your role
- **Review and approve** PRs on the paper path, the live path and lanes L2 to L5.
- **Dispatch builders** for the next brief rows, and get an independent review of each PR from an
  agent on a **different model** than the author (DEC-79). So far reviews used Sonnet; builders
  used Opus.
- **Push to PR branches** only to merge `main` in or to fix CI. Use merge commits. Never rebase,
  amend or force-push someone else's branch.
- **Post nothing to the founder** unless a founder decision is needed. Put FYIs in a morning
  list instead.

## Hard rules (beyond AGENTS.md)
- Never place real orders. Never ask for, read or use live credentials.
- **DEC-79:** founder-reserved decisions (live money, spending, legal or compliance text,
  weakening a safety rule) stay **Proposed**. Proceed with the most conservative option.
- **DEC-176:** an agent may accept a reading only if it tightens, or adds no risk. Any loosening
  goes to the founder.
- **Rule 13:** risk reduction (exits, protective orders, kill switch) is never denied.
- **Accepted DECs:** never edit one's body; a Status-line note is the only allowed edit. New
  decisions get one file each: `docs/project/decisions/DEC-<n>.md`, reserved per that README.
- **Commits:** NO `Co-authored-by` trailer (the CI trailer check rejects it).
- **DEC-77 order:**
  1. The tests PR comes first: `Unimplemented` stubs plus plain-fn `#[ignore = "pending <story>"]`
     tests, or `xtask/behaviour-only/` rows when the code is already implemented. A pending test
     must fail with the stub's own report, or by behaviour for a row (DEC-137).
  2. The implementation PR may only delete those ignore lines and rows.
  3. Gaps found in review mean a tests-correction PR first.
- **Squash-merge gotcha:** merging `main` into a branch stacked on a squash-merged PR resurrects
  deleted ignore lines and rows. After every such merge, delete them again and check
  `git diff origin/main -- '*/tests/*'`.
- **Builders and reviewers:**
  - use a private worktree and `CARGO_TARGET_DIR`, with `CARGO_INCREMENTAL=0`;
  - build only the touched crates;
  - never build the whole workspace or run `cargo xtask ci pending` locally, since CI does;
  - stop below 5 GB free disk;
  - never share a target dir across worktrees (it serves stale binaries).

## How to approve and merge (DEC-175, `.github/scripts/merge-approved.sh`)
1. **Check before approving:**
   - CI `fast` and `full` are green on the head (plus `web.yml` if `web/` changed);
   - the review verdict names that exact head;
   - `git merge-tree --write-tree origin/main <head>` prints the tree only, with no CONFLICT;
   - `git log origin/main..<head> | grep -ci co-authored` gives 0;
   - for an implementation PR, the test diff is only deleted ignore lines.
2. **Rewrite the PR body.** Keep the author's body, then add before the 🤖 footer:
   - a "Coordinator check:" line;
   - exactly one `Coordinator-approved-head: <40-hex sha>` line.
3. **Add the label** `coordinator-approved`.
4. **Who may do it:** the label and the body edit must come from an approver, the repo owner's
   login `kunwarshivam`. Do both through the founder's GitHub account.
5. **The merge itself:** `merge.yml` squash-merges on events. Its cron sweep has not fired since
   about 19:00 UTC. If a green, approved PR sits unmerged, squash-merge it yourself with the
   expected head SHA. The commit message is the PR body without any Co-authored-by line.
6. **Moved heads:** re-approve whenever the head moves. Drafts get no CI.

## Lane sessions (Claude Code cloud sessions; they open PRs and run their own Sonnet reviews)
- **L2** (workspace API, connections, journal spec): `session_018LpuiyKntPJenZ4jZnEdTz`. DEC range 900–909.
- **L3+L4** (notifications, web): `session_01BtCufaGcvxBnm9h6eebWxE`. L3 DEC range 70x; L4 71x.
- **L5** (audit, verification): `session_01GiEDxyxqyByvuwCrETeC6G`. DEC range 890–899.
- **L1:** stood down.
- **Messaging:** the lanes report to the old coordinator session by `send_message`, and you
  probably can't receive those. Watch the open PRs instead: each lane writes its review verdict
  into the PR body. Judge each PR on its own evidence.
- **Coordinator DEC range 868–879** is used up (879 = C2 filter allowlist). For a new coordinator
  decision, reserve the next free number per `docs/project/decisions/README.md`.

## State right now

**Merged tonight (selection):**
- C2: #1246, #1251 and #1253 (DEC-875, DEC-879).
- DEC-876 tests: #1240.
- E1b part 2 tests: #1245.
- Dispatcher D2 and D3: #1249, #1255.
- S1b: #1250, #1252.
- E7-17 tests correction: #1247.
- Journal verification: #1248 (DEC-896), #1256, #1259 (v0.39) and #1260.

**Approved, waiting on CI or merge:**
- **#1263:** L3, DEC-706 plus journal spec v0.40 (`NoticeAttempted` gets a `verdict`). Docs only.
  Head `dce20280`.
- **#1262:** the DEC-876 implementation (finishes C3). The Sonnet review passed. Head `e1c1cf06`.
- **#1257:** L4, C-26 web chips; web-e2e green. Head `b76f3965`.
- **#1264:** C4 tests (DEC-902). Head `367f1cb5`.
- **#1266:** L5, PR 4a, retires `resolve_trusted_start`. Head `2586e144`.

**New lane PRs to check:** #1265 (L3, DEC-706 step 2 pending tests). Next from L5: PR 4b, which removes the old vectors and includes the `journal.md:3166` sentence.

**Open PRs needing a review verdict, then approval:**
- **#1258 + #1261** (E1b part 2 tests-correction plus implementation, stacked; paper path).
  - A Sonnet review was running; if no verdict appears in the bodies, run one.
  - Once #1258 squash-merges, merge `main` into #1261 and re-check that its test diff is only
    the 5 ignore deletions plus the 5 deleted rows.
  - Judge the edge case in #1261's body: a wake past both the bound and the session end stops
    without cancelling. It needs a clock jump to happen at a 5 s interval.
- **#1242** (L2, E7-17 implementation): needs main merged in (now that #1247 has landed), one
  more ignore line deleted, a doc rewrap and a delta verdict from L2.

**Being reworked:**
- **#1254: bracket-leg reconciliation tests (DEC-878).** This blocks FT-11 for the founder's E2
  paper run.
  - **The defect:** after a bracket fill, reconciliation matches orders only by our client id.
    Alpaca's legs carry broker-assigned ids nested under the entry, so the protection is adopted
    as `Unknown`, and later risk exits are held as `unknown_order_in_flight`.
  - **Review 1 failed.** "At least one live leg" would hide a missing stop.
  - **Coordinator ruling** (a tightening): the protection counts as present only when all of
    these hold:
    - the entry is ours, was sent as a bracket, and is `filled`;
    - both legs are nested and live per §5.7: one stop leg with `stop_price`, and one
      take-profit limit leg;
    - each leg's qty and price match the placement.
    Anything else stays `Unknown`, as on main.
  - **Tests to add:** cases for a sent OCO with an absent order, a restricted status, an unknown
    status, TP-only, stop-only, a qty mismatch, a price mismatch, and a cancelled or rejected
    entry.
  - **DEC-878** gets a "Not settled" item: what a half-legged bracket becomes. If the answer
    loosens anything, it goes to the founder.
  - **Round 2** is pushed at `f4cc05d8`: the rule above, plus 20 listing cases and the round-1
    survivors now caught. A Sonnet delta review was running; if there's no verdict in the body,
    run one, then approve.
- **E7-28 round 6** (CI live-feature reader; #1234 tests, #1169 implementation; close #1219 as
  superseded when #1234 lands).
  - **Scope ruling:** DEC-851's threat model covers accidents and careless changes, not
    deliberate obfuscation.
  - **Round 6 closes:**
    - B1: leading redirects;
    - B2: heredoc command substitution;
    - B3: an env-key allowlist;
    - B4: plain `shell:` only;
    - B5: no writes to build inputs;
    - B6: non-local `uses:` only from a list;
    - B7: normalized `web/` paths, with `npx` and `npm exec` refused;
    - B8: scripts called by allowlisted scripts are read transitively;
    - the round-5 surviving mutants M03, M04, M05, M07, M10, M11, M13, M18, M19 and M20.
  - DEC-873 must drop its "fails closed on every spelling" overclaim and add an Out-of-scope
    paragraph.
  - Builder branches: `claude/live-e7-28-tests-7` and `claude/live-e7-28-v2`. Review against
    that scope; don't reopen the obfuscation chase.

**C4 (live path):** the tests PR **#1264** is approved (head `367f1cb5`, DEC-902). **Next:**
1. a C4 tests correction for the review minors listed in #1264's "Not done": trailing zeros on
   every money and quantity field, a sell order, an empty order id, and the DEC-902 item 6
   citation;
2. the C4 implementation, plus `mandate-rh-sim` serving `get_portfolio`, `get_equity_positions`,
   `get_equity_tradability` and `get_equity_quotes`, plus the end-to-end preflight test.

**Lanes' next items:**
- **L2:** #1242 delta, then the DEC-900 fix.
- **L3:** DEC-706 step 2 (`notices.rs` pending tests), then step 3: register v2, refuse v1, and
  move `step.rs`'s writer to v2 in one PR. Then D4 (retries), whose writer derives `verdict` from
  the `Outcome` variant.
- **L5:** PR 4a (pin tests first, then delete the old `resolve_trusted_start`), then 4b (old
  vectors), then the CLI judged-types pair after #1231, then run-logic T1/I1. T1/I1 includes
  DEC-896's start-token cases (re-sequenced; backlog E12-8) and DEC-787 item 8.

**Paths:**
- **Paper:** E1b part 2 (#1258/#1261) and the bracket-leg fix (#1254 tests, then implementation),
  then E3, then **E2, the founder's run**. E2 should record the open-orders read after its fill:
  Alpaca's nested-leg shape is unconfirmed.
- **Live:** C4 (tests, then implementation; rh-sim must serve those reads), then G1a and G1b,
  then R0 (the founder's rehearsal and live run). C2 and C3 are done; DEC-876 (#1262) finishes C3.
- **Known follow-ups:**
  - Cancelling bracket legs 404s at Alpaca (`client.rs::cancel` looks up `{entry}-p…`), and so
    does the in-doubt lookup. Both must be fixed before any exit runs against a filled bracket;
    neither blocks E2's one-share run.
  - DEC-877 item 4 (below).

## Founder morning list (open items from tonight; earlier items were answered at ~15:40 UTC)
1. **DECISION: DEC-877 item 4.** The shell refuses every submission after the entry as
   `SecondSubmission`, a protective OCO after a partial fill included. That conflicts with rule
   13. It can't happen while the run sizes one share. The recommendation is (a): exempt
   protective orders.
2. **OFFER: the E7-28 build guard.** A guard inside the build itself that refuses to compile
   `live` outside the one compile-only job. It is the real control; the CI reader is only defence
   in depth. It only tightens, so build it on the founder's yes.
3. **FYI: bracket-leg defect and ruling** (#1254, DEC-878), plus the cancel and in-doubt 404s.
4. **FYI: DEC-896 drill effect.** An unreadable token store makes a restore drill pass as
   `incomplete/token_unverifiable`. The founder accepted the "token-only incomplete" drill rule
   (DEC-789 item 9).
5. **FYI: DEC-874** can release orders held in doubt only toward DEC-872 item 3 (cancel only).
6. **FYI: C2 refuses a second agentic account** (`AmbiguousAgenticAccount`).
7. **FYI: #1212** moved a `layers.toml` entry (a founder-owned file).
8. **FYI:** `merge.yml`'s cron sweep isn't firing; direct merges were used.
9. **Still held for the founder:** #878 (Cloudflare cutover).

## Session end (when the night is over)
- Update `docs/project/08-work-tracker.md` and `docs/project/11-work-log.md`.
- Post a summary on issue #165.
- Delete the hourly trigger `trig_01VyZUXMo91A8HPRYZjMBMBN` in claude.ai Routines; it fires into
  the old Claude session.
