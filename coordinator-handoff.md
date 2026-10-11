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

## State right now (updated 2026-10-10 ~02:40 UTC)

**Merged tonight (latest first):** #1273, #1234, #1271, #1268, #1261, #1242, #1267, #1258, #1266, #1265,
#1254, #1264, plus earlier: #1263, #1262, #1260, #1259, #1257, #1256, #1255, #1253, #1251, #1250,
#1249, #1248, #1247, #1246, #1245, #1240.

**Approved, waiting on CI or merge:**
- **#1169** (E7-28 implementation). Head `46f63d0b`.
- **#1274** (L5, CLI judged-lists switch). Head `2375f854`.
- **#1275** (L2, links merged DECs in place of the stale "pending" words; docs only). Head `1b86398c`.
- **#1279** (L5, E12-3 T1a part A: run-plan stub plus tests). Head `6a3fca0c`. Part B (already reviewed, PASS at `bd8ea682`) opens after it merges.
- **#1282** (L5, tests-only prep: trusted-start tests read their workspace from the rows). Head `5471c118`. The re-key of the vectors to a valid workspace id (already reviewed, PASS at `486dcd45`) follows; check that every case keeps its outcome.
- **#1281** (L5, E12-3 T1a part B: trusted-start and refusal-order tests). Head `e07e22c3`.
- **#1278** (L5, `segment_rows_mismatch` implementation). Head `60997566`.
- **#1270** (bracket-leg implementation). Head `020b6904`.
- **#1277** (L5, DEC-897: verification-run tightenings; docs only). Head `38a9d6b4`.
- **#1276** (L5, `segment_rows_mismatch` tests; the implementation follows). Head `0832cfda`.

**Needs action now:**
1. **#1169** (E7-28 implementation): main merged in, resurrected rows and ignores re-deleted, and approved at `46f63d0b`. #1219 is closed as superseded. Nothing left except merging on green CI.
2. **#1270** (bracket-leg implementation): main merged in (clean, same patch-id as reviewed), and APPROVED at `020b6904`. #1269 has merged. After #1270 merges, dispatch **E3** (paper path).
3. **#1272** (L2, S1a implementation): APPROVED at `4df3fac7`. Next from L2 is DEC-903: a ref PR, a tests-correction row, then a #1272 follow-up fix.
4. **#878** stays held for the founder (Cloudflare cutover).

**Next rows to dispatch (no builder is running for them):**
- **C4 (live path):** first a tests correction for #1264's review minors:
  - trailing zeros on every money and quantity field;
  - a sell order;
  - an empty order id;
  - the DEC-902 item 6 citation, as a Status-line note.
  Then the C4 implementation, plus `mandate-rh-sim` serving `get_portfolio`, `get_equity_positions`,
  `get_equity_tradability` and `get_equity_quotes`, plus the end-to-end preflight test.
- **E3 (paper path):** after #1270 merges. Then the founder's **E2** run. E2 should record the open-orders read after
  its fill, because Alpaca's nested-leg shape is unconfirmed.
- **Follow-ups:**
  - **Alpaca cancel of a bracket's legs:** `client.rs::cancel` looks up `{entry}-p…`, which 404s. The in-doubt lookup
    does the same. Both must be fixed before any exit runs against a filled bracket.
  - **E7-28 minors:** listed in #1234's "Not done".
  - **E1b part 2 edge case:** a suspend past the session end.

**Lanes (Claude Code sessions; they open PRs and write review verdicts into PR bodies):**
- **L2** (`session_018LpuiyKntPJenZ4jZnEdTz`): #1272, then DEC-903, then the E7-17 follow-up (c) (delete the raw
  `fold`), then the mixed-stream prefix tightening.
- **L3+L4** (`session_01BtCufaGcvxBnm9h6eebWxE`):
  - L3: DEC-706 step 3 (register `NoticeAttempted` v2, refuse v1, move `step.rs`'s writer to v2), then D4 (retries).
  - L4: C-2, C-16 and C-17.
- **L5** (`session_01GiEDxyxqyByvuwCrETeC6G`): run logic, in this order:
  1. T1a/I1a: plan, refusals, trusted start, replay.
  2. T1b/I1b: walk, `draft`, `from_event`, `served`.
  3. V2: run vectors, including DEC-787 item 8 and DEC-896's token cases.
  4. T2/I2: anchors, segments, incomplete.
  5. T3/I3: prefix bind, hold, connection, control.
  Before I2, a journal T/I pair adds `segment_rows_mismatch` to `CHECKS`/`RANGE_CHECKS` (L5, coordinated with L2).
  The pending spec text for DEC-892, DEC-889 and DEC-894 is #1275.
- **DEC numbers in use tonight:** 873–879 (coordinator), 878 (bracket legs), 879 (C2 allowlist), 896 (L5), 900–903
  (L2), 902 (C4, inside L2's range), 706 (L3). Reserve new numbers per the decisions README.

**Paths:**
- **Paper:** #1269/#1270 → E3 → E2 (the founder's run).
- **Live:** C4 (tests correction → implementation + rh-sim) → G1a/G1b → R0. C2 and C3 are done.

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
11. **FYI: an artifact-store outage during a verification walk.** This is check 6, `artifact_missing`. It is a
    fail with a SEV-1, not `incomplete` (DEC-896 treats the same outage as `incomplete`, but only for the start
    token). That is the stricter reading, so a restore drill fails, not passes, if the store is down mid-walk.
    Loosening it is yours. **Consequence, now recorded in DEC-897 (#1277):** the SEV-1 pauses agents on the account
    or agent streams it covers, and under rule 13 a paused agent may hold exits. So an infrastructure outage could
    hold an exit. Relaxing this needs a §11 change. **Recommended: decide this before live trading.**
10. **FYI: a truncation bug in ref.py's §6.2 decision.** Whole-second truncation can lift an ask up to 1 s before a
    delegation's sub-second `starts_at`, which §6.2 forbids. It is recorded in DEC-903 for E8-8's owner, and E8-8's
    Rust must compare instants exactly. It exists only in the reference model; no Rust order path uses it yet.

## Session end (when the night is over)
- Update `docs/project/08-work-tracker.md` and `docs/project/11-work-log.md`.
- Post a summary on issue #165.
- Delete the hourly trigger `trig_01VyZUXMo91A8HPRYZjMBMBN` in claude.ai Routines; it fires into
  the old Claude session.

---

# Coordinator handoff: state at 2026-10-11 ~02:50 UTC (GLM-5.3 coordinator session)

Read everything above for process; this section supersedes "State right now" above.

## Merged since the last handoff (by me, DEC-175, through the founder account)
- **#1280** (L2, E7-17 delete the raw `ConnectionAnchor::fold`) — merged main in first (`671a85ba`), GLM-5.3 reviewed (author Claude, DEC-79 ok), squash-merged as `4190a8d1`.
- **#1292** (E7-4 bracket-leg cancel tests: 3 behaviour-only rows + 3 pending tests, DEC-878) — builder GLM-5.3 (local), reviewer Longcat-2.5-preview, merged as `e569c5a1`.
- **#1291** (E7-6 C4 tests correction: trailing zeros on 8 more fields, sell side, empty id, DEC-902 item 6 Status-line note) — builder GLM-5.3 (local), reviewer Step-5-preview, merged as `022a9b64`.

## Open right now
- **#1293** (E7-4 bracket-leg cancel FIX — gates E3 and FT-11). Builder GLM-5.3 (local). Head `0f8a56d1` after one CI-fix round (first head `db29cdbd` broke two mandate-shell watch tests by reaching the `Unimplemented { story: "E7-3" }` stub through the new doubt-settling wiring; the fix gates the entry-id lookup behind `ledger_current` so an unapplied fill waits for the gathered reconcile instead of refusing — shell protective path restored, 243 shell tests green locally). Longcat review of the OLD head is in flight and will be stale; **the new head needs a re-review before approving** (DEC-175 re-approval on moved heads). One flagged deviation for the reviewer to rule on: the builder changed ONE expectation in #1270's live test `only_the_whole_bracket_resting_under_its_filled_entry_keeps_the_placement` (it expected lookups by the handle; #1292's merged pin forbids that) citing an amendment it called "DEC-77's #174" — verify that citation exists.
- **C4 implementation** (E7-6: `account_snapshot` + `preflight_facts`, `mandate-rh-sim` serving the four reads, e2e preflight test, DEC-902 items 1–8, DEC-875 item 6): builder running in my session (GLM-5.3, local), branch `claude/c4-preflight-impl`. Reviewer must be non-GLM (Step-5 or Longcat routes).
- **#878** stays held for the founder (Cloudflare cutover).

## Infrastructure state (important)
- **The opencode cloud dispatch server is down** (`opencode.tail93399.ts.net` unreachable / half-broken API). It needs a VM/process restart by the founder. I ran everything locally instead.
- **The founder's claude.ai plan hit its weekly limit** — that is why the original lane sessions went silent overnight. I created four REPLACEMENT cloud sessions with full briefs (they are NOT the old sessions; those are unrecoverable — "Session not found"): L5 `session_01LzYjz5upn54KqbG8H7UDJF`, L2 `session_01AHuCQq2p2oe6QHQ1FRUFQS`, L3+L4 `session_013FZoRAb4AekasBL385ZeVy`, live-path `session_01EFpKMrXAEZ51e5Wa476meA`. All four are **parked on auto-continue: they resume Oct 14 at 1pm** (or immediately if the founder adds usage credits — a founder spending decision). One inert probe session also exists: `session_012G6QTgKS6hX6fEZ95B16r1` (safe to delete in the web UI).
- **Local model routes:** kimi-k3 and glm-5.3-flash were both connection-erroring tonight; the GLM-5.3 GB300 route (builders) and the free opencode provider routes (Longcat, Step-5 — good reviewers) work. DEC-79 held throughout: Claude-authored #1280 reviewed by GLM-5.3; GLM-authored 1291/1292 reviewed by Step-5/Longcat.
- Local machine is fine for this pipeline: M4 Pro, 48 GB RAM, 219 GB free; every agent uses a private worktree + private CARGO_TARGET_DIR and removed it afterward.

## DEC numbering warnings
- Main reached **DEC-909** — the landing wave (#1283–#1290) took DEC-903 through DEC-909, which collides with L2's plan and its in-flight branch `claude/l2-e10-7-dec-903-exact-window` (its DEC-903.md will conflict loudly; renumber per the README's next-free rule when it opens).
- DEC-910 was left free by #1292. Check `origin/main` before reserving anything.
- The AGENTS.md `cargo xtask check` list changed on main: cite nothing stale; the work log `docs/project/11-work-log.md` is now an explicit expectation ("how the work ran, with every PR").

## Queue after the current builders
1. #1293 re-review at `0f8a56d1` → approve → merge. That + the cancel fix unblocks **E3** (paper path). E2 (the founder's run) should record the open-orders read after its fill (Alpaca's nested-leg shape unconfirmed).
2. C4 implementation PR → review (non-GLM) → approve → merge. Live path then continues **G1a/G1b**, then R0 (founder's step 4 records the real Robinhood shapes).
3. When the Claude lanes wake (Oct 14 or credits): L5 continues its verification-run queue (T1b/I1b, V2, T2/I2, T3/I3) after opening the vectors re-key PR on `claude/l5-e12-3-start-vectors-workspace` (tip `577b9685`, reviewed PASS at `486dcd45` pre-#1282 — merge main in, re-run review at the new head); L2 continues the mixed-stream prefix tightening (founder-approved; scope: `ConnectionAnchor::from_verified` returns `Err(Broken)` on multi-stream prefixes; `VerifiedPrefix::bind` stays L5's).

## Founder morning list — additions to the ten items above
12. **DECISION: usage limits.** Your claude.ai weekly limit stopped all cloud lanes; four replacement sessions are parked to resume Oct 14 1pm, or earlier if you add credits. The local GLM pipeline kept everything moving without them.
13. **FYI: dispatch server down** (`opencode.tail93399.ts.net`); restart when convenient — until then reviews/builds run locally.
14. **FYI: the DEC-903–909 landing-wave collision** with L2's reserved-range plan (above); L2's delegation-window PR must renumber to the next free.
15. **FYI: #1293's CI-fix story** — the shell previously refused a protective action through an E7-3 stub door opened by the doubt-settling wiring; fixed by waiting for the gathered reconcile. Worth a glance at the ruling: a doubt behind an unapplied fill asks nothing and waits (never the handle, never Absent).
16. **FYI: AGENTS.md updated on main** (work-log expectation, live feature gate DEC-529, schema-mutants, ten-minute check budget DEC-464).
