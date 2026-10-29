# process

DEC-2600 to DEC-2699. The rules in force:

- **Decision numbers and shared lists**: numbers are allocated by category range so independent
  PRs do not both edit shared lists; a decision number is taken by its file (or by its section in
  a category file), and the guard refuses a number taken twice (DEC-2600, DEC-2650).
- **Pull requests open as drafts** and are marked ready only to land; a push cancels the CodeQL
  analysis of the head it replaced; CodeQL scans a ready PR once, skips docs-only PRs, and
  analyses one push to `main` at a time; CI skips drafts, and only a run that executed counts
  toward a merge (DEC-2640, DEC-2604, DEC-2606, DEC-2611, DEC-2613, DEC-610).
- **CI jobs are scoped to the diff**: the cell jobs start only when the diff reaches the cell,
  `cell-demo-web` never runs on a PR, every job finishes in under ten minutes (the one exception:
  `cell-demo-web` may run fifteen), and heavy sweeps are sharded and sized from observed cost
  (DEC-2609, DEC-2634, DEC-2648, DEC-2641, DEC-2607, DEC-2637–2639).
- **Landing**: merge `main` into a ready PR only when it conflicts; a merge that waits for no CI
  first passes `cargo xtask land-check`, which reruns a root-failed test unprivileged and runs
  gitleaks over the diff (DEC-2608, DEC-2642, DEC-2644, DEC-2645).
- **Secrets in CI**: gitleaks allowlist entries are exact and commit-pinned; the red gates
  (schema corpus, secret allowlists, lockfile) are repaired in-tree (DEC-2603, DEC-2647, DEC-2632).
- The founder's decisions in the coordinator session are recorded here and bind without a second
  statement (DEC-2605); the OAuth connection flow (connect start, callback to the vault, token
  exchange as one Pod, fixture provider in dev and CI cells) is DEC-2620–2636.


<a id="dec-2600"></a>

## DEC-2600: DEC numbers by category, and shared lists that independent PRs do not both edit

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | the agent, as a reversible process and tooling change (DEC-79) |
| **Story or claim** | E1-8, [#1494](https://github.com/kunwarshivam/mandate/issues/1494) |

### Decision

About twenty lanes land at once. Two things kept costing merge conflicts and CI cycles: lanes
reserving the same next DEC number, and every PR appending to the same few shared lists.

1. **DEC numbers by category.** From this decision on, each category has a range of 100 from
   DEC-2000 ([README](README.md#numbers-by-category)): 2000 infrastructure and the cell, 2100 the
   workspace API and identity, 2200 intelligence, 2300 the trading core, R0 and live, 2400 the
   web app, 2500 notifications and approvals, 2600 process and CI, 2700 the control plane, 2800
   spare. Within its category a lane takes the next free number and reserves it with its file, as
   DEC-344 has it. DEC-996 to DEC-1999 are no longer handed out.
2. **The interim lane blocks** (DEC-1010 to DEC-1209) are recorded in the README as history and
   stay with the lanes that hold them.
3. **No two decision files take one number.** `.github/scripts/decision-numbers.sh`, run by the
   lint job (`cargo xtask decisions`) and by the docs-only path's `docs-checks.sh`, refuses two
   files named `DEC-<n>…md` anywhere in the tree whose numbers are equal as integers, and a file
   whose `# DEC-<m>:` heading names another number. No such check existed: git's refusal of two
   PRs adding one path covered only the exact same path.
4. **The registry's cargo "Used by" lists are derived.** Each cargo row's cell is
   `` `cargo xtask deps --used-by` ``, which prints the list from `cargo metadata`; `cargo xtask
   deps` refuses any other text there and a name with two rows in one ecosystem. It still refuses a
   direct dependency without a row, so no check is weaker. Python and npm rows are unchanged.
5. **CODEOWNERS' crate lines are one a crate, sorted by path**, and `cargo xtask layers` checks the
   order and that every safety-critical crate has its line (both held already; now they are
   checked). `xtask/layers.toml` already has one entry a crate and is not reordered, which would
   conflict with every open PR that adds one.
6. **`merge=union` in `.gitattributes`** for `docs/dependencies.md`, `CODEOWNERS` and
   `xtask/layers.toml`, whose added lines are independent. It works only in local merges (`git
   merge main`), not GitHub's web merge or "Update branch". A union that keeps one edited row or
   key twice fails a check: a duplicate registry row (item 4), a duplicate TOML key, or an
   unsorted CODEOWNERS line.
7. **`xtask`'s job lists are slices**, `&[&str]` rather than `[&str; N]`, so adding a job no longer
   edits the line that holds the count.

### Rationale

A collision between categories becomes impossible and one within a category stays loud. Deriving
"Used by" removes the lines nearly every PR edited (the `proptest` and `thiserror` lists most)
while the registry still has to list every dependency. Each item is reversible and leaves every
check at least as strict.

### Alternatives considered

- **Keep coordinator-assigned blocks of ten:** needs the coordinator for every new lane.
- **Keep hand-kept "Used by" lists and rely on `merge=union`:** two PRs editing one row's list
  still conflict, and union does not apply on GitHub.
- **Sort `xtask/layers.toml`'s entries:** one large move that conflicts with every open PR adding
  a crate, for little gain over union merges.
- **A Rust copy of the decision check:** the docs-only path has no Rust toolchain, so one script
  serves both paths.

<a id="dec-2601"></a>

## DEC-2601: Commits on a branch may carry `Co-authored-by` trailers; CI no longer checks for them

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | The founder, in the coordinator session of 2026-10-18 ("I want it removed"); recorded by the coordinating agent |
| **Story or claim** | No story; process and CI. Unblocks [PR #1539](https://github.com/kunwarshivam/mandate/pull/1539), whose two commits carry the trailer |

### Decision

1. `cargo xtask ci spec-guard` no longer runs the `commit-trailers` check, and
   `.github/scripts/docs-checks.sh` no longer mirrors it. A commit on a branch may carry a
   `Co-authored-by` trailer.
2. The merge workflow (`.github/scripts/merge-approved.sh`) still builds `main`'s squash commit
   message from the PR description and drops any `Co-authored-by` line from it, so `main`'s history
   stays as it was: a trailer stays on its branch and never reaches `main`.
3. `.cursor/install.sh` still disables Cursor's hook that adds the trailer. Keeping it is a
   convenience, not a rule.

### Rationale

The check existed because Cursor's agent hook filled the trailer with the founder's email. The
founder chose not to keep it as a CI rule: it forced history rewrites on branches (#1539) for no
change in code. Item 2 already keeps the trailer out of `main`, the history that is published as the
project's record.

### Alternatives considered

- **Keep the rule and rewrite #1539's history** to remove the trailers. Declined by the founder.
- **Drop the merge workflow's stripping too.** Not asked for; it would let the trailer reach `main`.

<a id="dec-2602"></a>

## DEC-2602: The kill-switch drill runs in a CI cell job of its own

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79) |
| **Story or claim** | No story; CI. Keeps E21-34's kill-switch drill (DEC-2000) and every other cell job within DEC-464's ten minutes |

### Decision

1. `cargo xtask ci cell` runs the `boot`, `journal`, `egress` and `compose` drills, plain and
   seeded, and then `cell demo --no-web`. The `kill-switch` drill leaves it.
2. A new command, `cargo xtask ci cell-kill-switch`, brings up its own cell and runs the
   `kill-switch` drill, plain and seeded. CI runs it as the `cell-kill-switch` job, which matches
   `cell-journal` and `cell-failover`. It restores `cell`'s cache, saves none of its own, and is not
   required, as `cell` is not (DEC-993 item 10).
3. `cargo xtask cell drill` with no names still runs every drill.

### Rationale

`cell` reached its ten-minute limit on main. The nightly run on `b7fea875` was cancelled during
the demo, and #1587 was cancelled twice in the kill-switch drill. Bringing the cell up takes three
to four minutes with a cold cache. The drills take about three minutes plain and two seeded, and
the demo one more. The kill-switch drill is the longest, at about two minutes plain and seeded
together. DEC-464 says to add parallel jobs rather than remove tests. Moving that drill out gives
`cell` about two minutes of margin and gives the new job about four. No drill and no seeded bug is
dropped.

### Alternatives considered

- **Raise `cell`'s limit.** DEC-464 forbids it.
- **Drop the seeded runs from `cell`.** That would remove the proof that each drill catches its
  bug.
- **Move a different drill.** `journal` and `compose` are short, so moving either leaves `cell`
  close to the limit.

<a id="dec-2603"></a>

## DEC-2603: Exact gitleaks allowlist entries for six test-fixture values

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79) |
| **Story or claim** | No story; CI. The nightly `full-checks` on `b7fea875` failed in `supply-chain`'s gitleaks scan |

### Decision

1. `.gitleaks.toml` allows six tracked test-fixture values under `generic-api-key`, one entry per
   shape. Each entry is limited to the file that holds the value, so the same text anywhere else, or
   any other value in those files, is still reported:
   - the unknown-agent id `agt_<ULID>` in `web/src/components/api-screens/api-screens.test.tsx`;
   - the push tests' VAPID application server key, a public key by design, and the Web Push
     documentation's example `auth` value, in `web/src/components/notifications/push-section.test.tsx`
     and `web/src/lib/push/subscribe.test.ts`;
   - the placeholder API image tag (twelve fixed hex digits) in `xtask/src/cell.rs`.
2. `cargo xtask ci supply-chain` plants each allowed case. It also plants a stray copy in another
   file and a near miss in the same file, and both must be reported, as for the existing entries.

### Rationale

When a run has a diff base, as every pull request does, it scans only that PR's own commits.
`web/`-only changes skip `full-checks` altogether. With no base, the nightly scans the whole tree
and found these six values, added by #833, #1507 and #1560. None of them is a credential. Exact,
file-bound entries keep the scan strict everywhere else.

### Alternatives considered

- **Change the fixtures to values no rule matches.** This would change test data that the push
  tests check against the browser API's shapes. It would also hide the reason, where an allowlist
  entry records it.
- **Allowlist the test files wholesale.** Declined: a real key pasted into one of them would no
  longer be reported.

<a id="dec-2604"></a>

## DEC-2604: CodeQL scans a pull request once it is ready, not while it is a draft

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | The coordinating agent, under DEC-79 (a reversible CI process choice) |
| **Story or claim** | No story; CI capacity. The founder asked on 2026-10-18 to unblock the landing of approved work |

### Decision

1. `.github/workflows/codeql.yml` skips its `analyze` job on a draft pull request, and also runs on
   `ready_for_review`, so a pull request is scanned once it leaves draft. The coordinator's approval
   marks a pull request ready (DEC-175), so every pull request is scanned once, alongside the merge
   workflow. CodeQL is not a required check and does not gate the merge; its findings are read on
   `main`, as before.
2. Pushes to `main` and the weekly schedule scan as before.

### Rationale

Builders push draft heads many times an hour, and each push started a CodeQL run on the shared
runners. Those runs held runners that approved pull requests' required `ci` checks were queued
behind. CodeQL is not a required check and its findings are read on `main`; scanning a draft head
that will change again bought nothing the ready-time scan does not.

### Alternatives considered

- **Cancel draft runs by hand.** Done once on 2026-10-18; it recurs on every push.
- **Scan only `main`.** Drops the pre-merge scan altogether; not needed to free the runners.

<a id="dec-2605"></a>

## DEC-2605: The founder's decisions in the coordinator session are recorded here and bind without a second statement

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | The founder, in the coordinator session of 2026-10-18 (row 7: "I need you to stop involving me writing comments when I have already made the decision"); recorded by the coordinating agent |
| **Story or claim** | No story; process. Applies to DEC-79's founder-reserved decisions |

### Decision

1. Only the founder's own answer, given to the coordinating agent in its session, to a question that
   names the decision and, where it loosens a rule, the rule it loosens, is recorded here. Each row
   holds the question as asked and the founder's answer verbatim (for a choice among options, the
   option's label as shown), with its time. A paraphrase or an inferred answer is never a row; a
   decision that cannot be quoted stays Proposed.
2. A row is the founder's decision. The lane that owns the affected DEC amends it to cite the row
   ("decided by the founder, DEC-2605 row N"). Nobody asks the founder to restate it on GitHub.
3. A row records only what the founder decided. The design that follows is the lane's DEC, and it may
   only tighten what the row says.
4. Where a question turns out to have been put wrongly, the answer to the wrong question is not a row;
   the right question is asked.

### The founder's decisions of 2026-10-18

Times are UTC and approximate to the minute.

| Row | Time | Question as asked | Founder's answer, verbatim | Resolves |
|---|---|---|---|---|
| 1 | 19:58 | How should we build the hosted infrastructure? | "OpenTofu + DOKS + GitOps (Recommended)" | Lane IAC's DEC-2011 (E21-56, #1639) |
| 2 | 19:58 | What monthly cloud budget cap should agents plan within (staging + small production, with alerts)? | "Up to $150/mo (Recommended)" | DEC-2011 |
| 3 | 19:58 | What do we show at the demo on Oct 26? | "Staging by the 26th" | DEC-2011 |
| 4 | 20:13 | Where should the journal's backup archive (Postgres WAL and base backups) live? | "Cloudflare R2 + bucket lock (Recommended)" | DEC-2011; DEC-2006 item 3's bucket choice (lane H's open #1539) |
| 5 | 20:05 | DEC-2103 item 7: when you approve a paper-trading grant in the web app, may the API accept a step-up confirmed in the CLI (`cli_confirm`) instead of your passkey? | "Allow cli_confirm too" | DEC-2103 item 7 (lane L1), which loosens its conservative course (the API verifies passkey only) |
| 6 | 20:15 | DEC-2300: Robinhood doesn't report a sell's exact fill price to us. How should we book a sell's fill price? | "Book at the limit price (Recommended)" | DEC-2300 (lane LV), replacing its owner-attested price |
| 7 | 20:16 | (The founder's own statement, unprompted) | "I need you to stop involving me writing comments when I have already made the decision" | This DEC |
| 8 | 20:21 | DEC-971 item 7: binaries only start in staging or production if signed by the release key, and none is set up yet. How should staging (paper trading only) get running by the 26th? (Options: staging runs before the key; hardware key now; CI key behind your approval.) | "i dont have time and money for harware key" | DEC-971 item 7 (a), declining the hardware key (ES-17, H8) |
| 9 | 20:21 | Understood, no hardware key. Then which way for signing releases? (Option shown: paper-only staging runs before any key; before production, a release signing key lives in a protected GitHub environment and signs each release in CI only after the founder approves. The other option: only staging unsigned, decide later.) | "CI key + staging now (Recommended)" | DEC-971 item 7 (a) and (b) |
| 10 | 20:27 | The credentials vault (OpenBao) is sealed after every restart, and someone has to unseal it with key shares before the system can read any credential. How should staging handle that? (Options: auto-unseal for staging from a key kept in the infra-staging GitHub environment, production decided later; manual unseal always; no vault in staging yet.) | "Auto-unseal for staging (Recommended)" | DEC-977 item 5 for staging only (lane V2, E21-32); production's unseal stays open |
| 11 | 21:56 | Staging's Kubernetes control panel accepts only fixed addresses. So that setup runs fully automatically with no step from you, may each automated deploy run open it to just that run's own GitHub runner address for a few minutes, then close it again? | "Yes, per-run window (Recommended)" | Lane IAC's DEC-2028: each infra run admits its own runner's /32 to staging's Kubernetes API for the run and closes it on every exit path |
| 12 | 21:56 | The database backup archive needs a Cloudflare R2 token scoped to one bucket. How should it be made? (Option chosen, as shown: "Your existing Cloudflare token gains 'API Tokens Edit'. Nothing by hand, but that permission can create any token, including one that lifts the archive's bucket lock, so it's admin-level.") | "Let automation mint it" | Lane IAC's DEC-2029: OpenTofu mints the archive's bucket-scoped R2 token with the infra token, which gains Account API Tokens Edit |
| 13 | 22:09 | Staging keeps its infrastructure state in a DigitalOcean Spaces bucket. So that you never create that bucket or its access key by hand, may your DigitalOcean API key also gain the 'Spaces Keys' create/read/delete permission? Each run would mint a short-lived key, create the bucket if it's missing, then delete the key. | "Yes, add Spaces Keys (Recommended)" | Lane IAC's DEC-2030: the infra token gains Spaces Keys create/read/delete; each run mints and deletes its own key |
| 14 | 22:38 | DEC-175 says a coordinator-approved PR merges only once CI's `fast` and `full` checks are green on its head (and `web` when it touches web/). Do you relax DEC-175 so an approved PR merges right after its independent review, without waiting for `fast`, `full` or `web`? | "Yes, relax DEC-175 (Recommended)" | DEC-175's merge condition: a coordinator-approved head merges after its independent review without waiting on `fast`, `full` or `web` |

An earlier question at 22:35, which did not name DEC-175, was answered "Yes, CI doesn't gate merges (Recommended)"; under item 4 it is not a row, and row 14 is the properly named question.

Earlier, a question put as "should sign-in be passkey-only (no passwords) in v1?" was answered "Yes,
passkey-only (Recommended)". It was asked as DEC-2103 item 7, which it was not, so under item 4 it is
not a row.

### The founder's decisions of 2026-10-19

Times are UTC and approximate to the minute. Both rows answer one message that listed three
questions: "Staging still waits on your three answers: whether the release-signing environment and
its secrets exist; whether to make a new Supabase project for staging; whether to retire the old demo
droplet." The founder answered "Yes no yes". The first answer is a fact (the `release-signing`
environment exists), not a decision, so it is not a row.

| Row | Time | Question as asked | Founder's answer, verbatim | Resolves |
|---|---|---|---|---|
| 15 | 13:40 | Whether to make a new Supabase project for staging (asked earlier as: "For staging, should we make a new Supabase project (recommended) or reuse the site's?") | "no" (the second of "Yes no yes") | Staging's API issuer is the site's existing Supabase project (DEC-640), not a new one; lane GO's staging `api.*` values |
| 16 | 13:40 | Whether to retire the old demo droplet (asked earlier as: "Should we retire the old demo droplet (recommended)?") | "yes" (the third of "Yes no yes") | The DEC-822 demo host is retired, so the api-server's start-up check (DEC-971 items 2 and 3, E21-35, #1889 and #1894) has no carve-out for its unsigned binary |
| 17 | 14:13 | "E6-15: the account's risk state and tripwires (the loss ladder that cuts risk) are not yet read by the running agent or the risk gate — they only bind in tests. Start wiring them in now, alongside the staging backend work, so paper orders on staging are covered by them?" | "Start now (Recommended)" | E6-15 slice C's timing (the work tracker's founder item): lane TW wires the risk state, tripwires and ladder latch into the shell gate and the runtime now |
| 18 | 14:13 | "DEC-968 item 6: to run a real agent cycle in the cluster, lane D1 must add one constructor to a file in the R0 path (the code your first live trade was rehearsed on). It only adds a way to pass the journal in; the live path's behaviour is unchanged, but R0's rehearsal would need re-running before the live order. Allow it?" | "Allow (Recommended)" | DEC-968 item 6: the production-cycle constructor that takes its journal stage, in an R0 file; the R0 rehearsal is re-run on the simulator before the live order |
| 19 | 14:17 | "DEC-997 is still Proposed: OpenRouter as the one aggregator for every hosted model call, routing locked to one upstream with zero data retention, and model spend capped at $50 a month per workspace. You chose these on 10-17, but the decision needs your acceptance in your own words before any real model call is made. Accept DEC-997 as written?" | "Accept DEC-997" | DEC-997 becomes Accepted as written: OpenRouter, routing locked to one upstream with zero data retention, $50 a month per workspace |
| 20 | 14:20 | "Follow-up on the R0 file (row 18): to run real agents in the cluster without copying safety-critical code, lane D1 needs to move the builder/risk-gate/flatten glue out of the R0 file (mandate-shell adapters.rs) into a shared module that both your laptop runner and the cluster runtime use — not just add one constructor. Behaviour stays identical, tests first, every mutant killed, and the R0 rehearsal re-run green before your live order. The alternative (a second copy of the glue in the cluster runtime) would drift. Allow the extraction now?" | "Allow now (Recommended)" | DEC-968 item 6, widened from row 18: `mandate-shell`'s `adapters.rs` builder, gate and flatten glue moves to one shared module both callers use, behaviour unchanged; the R0 rehearsal is re-run before the live order |
| 21 | 14:23 | "Live order timing: the risk-state wiring (E6-15 slice C — loss ladder, tripwires, daily loss) changes the code your live order runs on, so the live-trade rehearsal must be re-run after it lands. Should your first live Robinhood order wait until slice C has landed and the rehearsal re-passed, or go ahead on today's gate (without the account-level loss ladder) if it's ready first?" | "Wait for slice C (Recommended)" | DEC-529's live order: it waits until E6-15 slice C has landed and the R0 rehearsal re-passes; the target may move past 2026-10-23, the deadline stays 2026-11-02 |
| 22 | 14:31 | "DEC-433 item 18 (Proposed): Alpaca's free market-data feed (IEX) may not carry trading-status/halt (LULD) messages. The safety rule blocks new openings when the status channel is absent. Allow PAPER agents to open positions on IEX data without a status channel? (Live stays blocked; exits are never blocked either way.)" | "Allow for paper only (Recommended)" | DEC-433 item 18: paper openings on IEX without a status channel, under the item's recommended conditions; live stays blocked (SIP-only, DEC-35); exits are never blocked |
| 23 | 15:05 | "Vault design, your call (weakens a safety boundary): for you to connect Alpaca from the web with no manual step, some service must be able to create the vault logins for each new connection — and whatever can do that can, in effect, read every connection's broker token. Where should that power live?" (Options: a dedicated provisioner whose only job is creating these logins, with vault access only and no internet; the connection manager; deploy-time only.) | "Dedicated provisioner (Recommended)" | Lane V2's DEC-2630: one dedicated provisioner process holds the scoped policy that creates each connection's executor and token-exchange vault logins on connect; it has no egress but the vault and no other duty |

### Rationale

The founder works with the coordinating agent directly, and asking them to restate each decision on
GitHub cost their time. A reviewer of this file cannot see the session, so item 1 limits a row to the
founder's verbatim answer to a named question; that is what makes the record checkable against the
session transcript, and what keeps a paraphrase from loosening a rule.

### Alternatives considered

- **A GitHub comment per decision.** The previous practice for DEC-995 and DEC-2103 item 7. Declined by
  the founder (row 7).

<a id="dec-2606"></a>

## DEC-2606: A new push cancels the CodeQL analysis of the pull request head it replaced

| | |
|---|---|
| **Status** | Accepted; item 2 amended by [DEC-2611](process.md#dec-2611) for pushes to `main` |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the agent, as a reversible CI change (DEC-79) |
| **Story or claim** | No story; CI throughput (lane CT). Approved pull requests' `ci` runs queued for over an hour on 2026-10-18 |

### Decision

1. A pull request's "CodeQL Advanced" runs (`.github/workflows/codeql.yml`) share one concurrency
   group, `codeql-<ref>`, and a new push cancels the run of the head it replaced, as `ci` and
   `release` already do for their pull request runs.
2. A push to `main` and the weekly schedule each have a group of their own (`github.run_id`), so
   none is cancelled or held behind another.
3. Nothing else changes: the same four languages, the same config and queries (DEC-500, DEC-794),
   the same triggers. Every pull request head that is still the head when its analysis runs is
   analysed. DEC-2604 separately skips drafts.

### Rationale

The account runs at most 40 jobs at once, and on 2026-10-18 it was full: 38 jobs in progress and
about 390 queued, while `merge` jobs that do twelve seconds of work waited a median of ten minutes
and up to thirty. Of about 9,400 runner-minutes between 2026-10-17 18:00 and 2026-10-18 20:45 UTC,
CodeQL took 17 percent (28 percent of the latest 300 runs), four `Analyze` jobs a push, and with no
concurrency group every push's analysis ran to the end even after a newer push replaced its head.
The analysis of a head nobody can merge any more finds nothing the analysis of the new head misses,
so cancelling it gives the runners back without testing less.

### Alternatives considered

- **Run CodeQL only on `main`.** Declined: a finding would surface after the merge rather than on
  the pull request that introduced it.
- **Skip CodeQL on documentation-only pull requests (`paths-ignore`).** Not taken here: it changes
  which ready heads are analysed. It can be decided on its own.
- **Buy more concurrency (larger runners or a higher plan).** Spending is the founder's (DEC-79).

<a id="dec-2607"></a>

## DEC-2607: Mutation shards carry several mutants, sized per crate from their observed cost

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79): no cap loosens and no mutant leaves the gate. Lane CT measured and builds it |
| **Story or claim** | No story; CI throughput (lane CT). Supersedes [DEC-498](earlier-303-599.md#dec-498) item 5 for shards of more than one mutant, and the one-mutant shard load of [DEC-538](earlier-303-599.md#dec-538) item 4 |

### What the runners measured

The account runs at most 40 jobs at once, and on 2026-10-18 it was full: 38 jobs in progress and
about 390 queued, 189 of them mutation shards. A diff of 102 mutants started 102 shards; one of
173 jobs filled every slot. Of 89 green shards sampled from 24 runs between 2026-10-17 18:00 and
2026-10-18 20:50 UTC (each one mutant, DEC-498 item 1), the fixed part (services, toolchain,
cache, the xtask build, the pre-flight `cargo nextest list` that builds the tested graph in place
(DEC-850), and the unmutated baseline) took 94 to 271 seconds, while the mutant itself took
1 to 26:

| Tested packages | Shards | Fixed, max (s) | One mutant, max (s) | Job, max (s) |
|---|---|---|---|---|
| mandate-daemon | 16 | 161 | 11 | 175 |
| mandate-api-server | 12 | 167 | 9 | 183 |
| mandate-cli | 10 | 231 | 14 | 251 |
| mandate-artifacts-fs, mandate-daemon | 8 | 151 | 11 | 171 |
| mandate-backtest-runner, mandate-cli, mandate-version-backtest | 8 | 271 | 2 | 283 |
| mandate-daemon, mandate-scheduler, mandate-store | 8 | 158 | 4 | 170 |
| mandate-deploy-manager, mandate-deployd | 8 | 159 | 4 | 172 |
| mandate-stub-issuer | 8 | 161 | 7 | 171 |
| mandate-api, mandate-passkey | 4 | 116 | 2 | 126 |
| mandate-evaluation | 3 | 94 | 2 | 103 |
| mandate-executor, mandate-executord, mandate-refcases | 3 | 190 | 26 | 221 |
| mandate-refcases, mandate-research-run | 1 | 129 | 1 | 140 |

So about nine tenths of every shard is the same work, repeated once a mutant. The costs also
differ by crate: `mandate-executor`'s baseline tests took 31 seconds here and 179.6 seconds on
[#1337](https://github.com/kunwarshivam/mandate/pull/1337), where its unoptimised suite reached
`MUTANT_TEST_TIMEOUT`. One number for every crate would be either wasteful or unsafe, so the
number of mutants a shard carries is sized per crate, from that crate's own history.

### Decision

1. **Every shard records what it cost.** `cargo xtask ci mutants` writes one record a shard
   (`target/mutant-cost.json`), read from cargo-mutants' own `outcomes.json`: the packages its
   slice mutated, its mutant count, the time from the gate's start (when it writes its diff) to
   the first mutant (the fixed part: the pre-flight build, the enumeration and the unmutated
   baseline), the longest single mutant (its build and its tests), cargo-mutants' outcome, and
   whether the shard ran out of time (item 5). Times come from file timestamps and
   cargo-mutants' phase durations, because xtask reads no clock (ADR-0001 ES-05). The shard
   uploads the record as the artifact `mutant-cost-<shard>`, on success and on failure. A record
   that cannot be written is said in the log and changes nothing about the shard's verdict.
2. **The history is one artifact.** A `mutants-costs` job after the matrix, whatever the matrix
   concluded, downloads the latest `mutant-costs` history and this run's records through the
   Actions REST API (`cargo xtask ci mutants-costs`), adds each record under every package it
   mutated, keeps each package's 20 most recent records, and uploads the result as the new
   `mutant-costs` (kept 14 days). Two runs that write at once can lose one run's records; the
   next run of that package adds new ones, and item 3 only ever reads a missing record as a
   reason for `k = 1`.
3. **The plan sizes each shard from the history.** `cargo xtask ci mutants --plan` reads the
   latest `mutant-costs` (the plan job gets `actions: read`). For each package the diff mutates,
   with `F` the largest fixed part and `M` the longest mutant over its kept records:
   `k = clamp(floor((480 - F) / (2 * M)), 1, 16)`, in seconds. A package with fewer than three
   records that ended `caught` or `missed`, or with any kept record that ran out of time, hit a
   cargo-mutants timeout or failed its baseline, gets `k = 1`, which is today's shard. A diff
   takes the smallest `k` of its packages. A history that cannot be read, or none at all, gives `k = 1` everywhere, so
   this decision can only shorten a matrix whose crates have shown they fit.
4. **The matrix is `ceil(n / w)` shards, `w = max(k, ceil(n / 192))`.** cargo-mutants' `slice`
   sharding then gives every shard at least one and at most `w` listed mutants, so at most `k`
   below the 192-shard ceiling, and never more than 192 shards. Past the ceiling this also drops
   the shards that 192-way slicing left empty: a 193-mutant diff started 192 shards, 95 of which
   tested nothing, and now starts the 97 that hold its mutants, each the same slice as before.
   The plan reports `per_shard=k` beside `mutants`, `shards` and `total`, CI passes it to every
   shard as `MANDATE_MUTANTS_PER_SHARD`, and a shard whose total is not the plan's for its count
   and `k` fails before it builds, as DEC-538 item 3 does today.
5. **A shard that runs out of time fails, loudly, and corrects the next plan.** When `k > 1`, xtask
   runs cargo-mutants under `timeout` with what is left of 480 seconds after the gate's start,
   which with the job's setup stays inside the ten-minute job limit (DEC-464). It then writes the record with `overran = true` and fails,
   naming the package set and its `k`. `full` fails with it. The next plan for that package set
   finds the record and gives it `k = 1` while the record is among its 20 kept (item 3). A shard
   that runs out of time at `k = 1` is the case DEC-498 item 8 already covers, and nothing here
   changes it.
6. **What stays.** The mutant set (DEC-538 items 2 and 4: every listed mutant is in exactly one
   shard), the per-mutant caps (`MUTANT_BUILD_TIMEOUT` 60 seconds, `MUTANT_TEST_TIMEOUT` 180
   seconds, DEC-498 items 3 and 4), one worker (item 2), the unmutated baseline in every shard,
   the pre-flight (DEC-850), the zero-survivor rule, the database and services every shard starts
   (DEC-519), the ten-minute job limit (DEC-464), the 192-shard ceiling, and the nightly sweep.
   `cargo xtask check` stays unsharded.
7. **What it supersedes.** DEC-498 item 5 priced a shard at what each phase is permitted to cost,
   which allows one mutant: one mutant 240 seconds, the baseline's tests 180, the rest 180. For a
   shard of more than one mutant, this decision prices it at what that package set's shards have
   been observed to cost, with a factor of two on the mutants, and makes the miss a failure that
   shrinks the next plan instead of a bound. DEC-498 item 9's rule still holds for every term this
   decision names: each is capped by configuration (the per-mutant caps, the 480-second stop),
   measured from the record it is priced at, or stated with its failure mode (item 5).

Built in two pull requests: the records and the history (items 1 and 2), which change no shard,
then the plan and the stop (items 3 to 5).

A history is written by pull request runs, so it is input a pull request can shape. Nothing in it
decides which mutants run: every listed mutant is in a shard whatever `k` is (item 4), the
per-mutant caps and the 480-second stop hold whatever it says, and `k` is at most 16. A false
history can make shards run out of time and fail, never make a mutant go untested.

### Rationale

The table counts the fixed part from the job's start; about 50 seconds of it come before xtask
starts, so item 3's `F` is about 50 less. With those costs, the 102-mutant run of 2026-10-18,
whose shards tested the executor's package set (`F` about 140, `M` 26), gets `k = 6` and 17
shards instead of 102, and a 10-mutant diff in `mandate-daemon` (`F` about 111, `M` 11) gets
`k = 16` and one shard instead of 10. The mutants are
the same and each is judged the same way. The risk DEC-498 item 5 closed, a slow mutant running a
shard past ten minutes, is held by the 480-second stop and turned into a red run that cannot
repeat for that crate, rather than into a matrix that pays the fixed part once a mutant forever.

### Alternatives considered

- **One `k` for every crate.** Declined: `mandate-executor`'s baseline alone has run 31 and 179.6
  seconds; a global `k` safe for it wastes every other crate's shards.
- **Lower the per-mutant caps so that several fit DEC-498's budget.** Declined: the executor's
  baseline already reaches the 180-second test cap, and a lower cap would cut it off.
- **Share one built tree across shards (an artifact of `target/`).** Not taken now: a shard's
  tree is gigabytes, and moving it costs most of what it saves; it can be measured separately.
- **More concurrency.** Spending is the founder's (DEC-79).

<a id="dec-2608"></a>

## DEC-2608: Merge `main` into a ready pull request only when it conflicts

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible process decision (DEC-79), in force for every builder from 2026-10-18 |
| **Story or claim** | No story; process and CI (lane CT). Runner measurements of 2026-10-17 and 2026-10-18 |

### Decision

1. **Merge `main` into a ready pull request only when GitHub reports it un-mergeable, or when a
   change on `main` it must build on has landed.** The merge commit's message says which. A merge
   of `main` that resolves no conflict and brings in nothing the change needs is not made.
2. Rebasing a shared branch stays forbidden as before; a needed update is a merge of `main`.

The ruleset does not require a branch to be up to date with `main`, and since DEC-2605 row 14 a
coordinator-approved pull request merges after its independent review without waiting for CI, so
a pull request that GitHub reports mergeable needs nothing more from `main`. A combination of
changes that breaks `main` is caught after the merge by `ci`'s run on `main` (DEC-2610) and before
it by `cargo xtask trial`, not by keeping every branch current.

### Rationale

Every number below is from the Actions REST API for 2026-10-17 18:00 to 2026-10-18 20:50 UTC.

- **The account was full.** Its plan runs 40 jobs at once. At 20:47 UTC 38 were in progress,
  about 390 were queued, and `merge` jobs, which do about 12 seconds of work, had waited a median
  of 10 minutes (p90 17, max 31) behind them: Actions queues an account's jobs in arrival order
  with no priority.
- **A ready pull request ran `ci` 2.3 times on average.** 149 runs on 25 ready branches: 7 ran
  once, 12 twice, 6 three to seven times. Runs cancelled by a newer push used 2,251 of 6,477
  sampled `ci` runner-minutes (35 percent).
- **37 percent of runs (55 of 149) were on a "Merge main into …" head.** Most resolved a real
  conflict on a shared file (journal spec versions, backlog rows, `ci.yml`), because the pull
  request beside it had just landed; the rest synced a branch that GitHub already reported
  mergeable.

Each merge of `main` starts a full `ci` run and cancels the one in flight part way through. A
merge that resolves no conflict buys nothing for that cost.

### Alternatives considered

- **Require branches to be up to date with `main`.** Declined: every landing would make every
  other approved pull request merge `main` and run `ci` again, the cost this decision removes.
- **A merge train that tests approved pull requests together before they land.** Proposed in an
  earlier draft of this decision and withdrawn: with DEC-2605 row 14 merges do not wait for CI, and
  DEC-2610's run on `main` after every merge is the post-merge check. If batching is ever needed,
  it is a new decision.

<a id="dec-2609"></a>

## DEC-2609: The cell jobs start only when the diff reaches the cell

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79), on lane CT's measurement |
| **Story or claim** | No story; CI throughput (lane CT). Extends DEC-1190's selection to the cell jobs of DEC-993 and DEC-2002 |

### Decision

1. `cargo xtask ci select` prints one more switch, `cell`. It is true when:
   - the selection selects everything (no diff base, as on `main` and the nightly; a build input;
     `xtask/`; `.github/`; an unknown path; DEC-1190 item 3), or
   - a changed path is under `deploy/` or is `python/pyproject.toml` or `python/uv.lock`, the
     environment `cargo xtask ci cell` runs the charts' invariant tests (`deploy/tests`) in
     (`CELL_INPUTS`), or
   - the selected crates include one of the packages the cell's images hold
     (`cell::IMAGE_PACKAGES`: `mandate-cli`, `mandate-backtest-runner`, `mandate-api-server`,
     `mandate-daemon`, `mandate-deployd`, `mandate-stub-issuer`). The selection already holds
     every crate depending on a changed crate, so this is true exactly when a changed crate is one
     of them or one they are built on, directly or not, or a crate whose sources name a changed
     path is.
2. `cell`, `cell-demo`, `cell-journal`, `cell-failover` and `cell-kill-switch` need `select` and
   start only on `cell`. They no longer run the documentation-only step of DEC-112: a
   documentation-only change selects no crate and no cell input, so `cell` is false for it.
3. `cell::IMAGE_PACKAGES` is the list the static build of the cell's binaries reads, so the build
   and the selection cannot name different packages.
4. Nothing else changes. No cell job is a required check (DEC-993 item 10), and none is removed,
   shortened or weakened. The nightly dispatches `ci` on `main` with no diff base, so it runs all
   five every night, and every change that can alter what they build or read runs them on its
   pull request.
5. A sixth cell job, `cell-demo-web`, runs `cargo xtask ci cell-demo` with `web/`'s tools
   installed and both `MANDATE_CELL_REQUIRED` and `MANDATE_DEMO_WEB_REQUIRED` set, so the demo's
   web gesture runs or the job fails; with the web switch alone, an API that cannot be asked would
   read "not run". It installs `web/` with `npm ci --ignore-scripts`, so no dependency's install
   script runs in a job that runs cargo, and the Chromium headless shell of the Playwright the
   lockfile pins, cached per that version. It holds no secret and is not required. `select`
   prints `cell_web`, true when `cell` is or when a path under `web/` other than its Markdown
   changed, and the job starts only on it. It adds a job to every change that starts the cell
   jobs and to `web/` changes, which start no cell job today; that is new testing, not
   de-duplication, and stays as small as the switch allows.

### Rationale

The account runs at most 40 jobs at once, and on 2026-10-18 it was full, with about 390 jobs
queued. The five cell jobs ran on every ready pull request, about 22 runner-minutes each time,
whatever it changed. A cell job builds the six image packages and reads `deploy/` and the pinned
Python environment; xtask and CI, which drive it, select everything when they change. A diff that
reaches none of those builds the same images from the same sources and runs the same drills as
`main`, so running them again tests nothing new. Skipping them there is DEC-1190's
de-duplication applied to five more jobs, not testing less.

### Alternatives considered

- **Run the cell jobs only on `main` and the nightly.** Declined: a change to a chart or an image
  package would surface after the merge rather than on its pull request.
- **Scope each cell job by its own drills.** Declined for now: every job brings up the same cell
  from the same images, so one switch is the honest grain.

<a id="dec-2610"></a>

## DEC-2610: `ci` runs on `main` after every merge, and reports when it fails

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79); lane CT builds it |
| **Story or claim** | No story; process and CI. Lane E2E found that `ci` never ran on `main` after a merge, only in the nightly, while merges no longer wait for CI (DEC-2605 row 14 relaxes DEC-175) |

### Decision

1. `ci.yml` runs on `push` to `main`. A run on `main`'s tip has no diff base, so `select` selects
   everything, exactly as the nightly's dispatch does (DEC-1190 item 5): every job and every
   required check runs, and each stays within DEC-464's ten minutes, as it already does nightly.
2. `report-main` opens its opaque issue (the event, the commit, the run's address and the two
   verdicts, never a log line) when a push run on `main` fails, as it does for a dispatch. It
   opens none when an open issue already has its title, `ci on main failed at <commit>`, so a push
   run and the nightly's dispatch failing on the same commit report it once; the nightly's
   `report` does the same for `nightly failed at <commit>`.
3. Post-merge runs on `main`, a push's and the ones `merge.yml` dispatches with the input
   `post_merge: true`, share the concurrency group `ci-refs/heads/main`, and on `main`
   `cancel-in-progress` is false: a run that has started finishes and reports, and GitHub keeps
   at most one more run waiting in the group, replacing it with each newer one. So every burst of
   merges, by people or by `merge.yml`, gets a verdict from the run in progress when it began and
   from its latest merge, at most one run more than cancelling would cost. Each run tests `main`
   with every merge before it in it, because it selects everything. A replaced waiting run
   reports nothing. On 2026-10-19, cancelling the run in progress left no run on `main` finishing
   through a burst of merges while two pull requests that passed alone broke `main`'s lint
   together. Pull request runs still cancel the older run of their ref. A cancelled run on `main` skips the
   aggregates `fast` and `full`, so it ends at once and releases the group: on 2026-10-19 a
   cancelled dispatch's two aggregates waited 1 hour 45 minutes for a runner and held every push
   run behind them. On a pull request they still run when the run is cancelled, so a cancelled
   head fails its required checks rather than reading skipped, which GitHub counts as passing. Any other dispatched run, the nightly's included, has a group of its own,
   `ci-<run id>`, so it is never cancelled and cancels nothing: the nightly always runs to its end.
   `release.yml` has its own groups and is untouched.
4. A merge `merge.yml` makes pushes with that workflow's own `GITHUB_TOKEN`, and GitHub starts no
   workflow for such a push. So after a run of `merge.yml` that merged at least one pull request,
   a second job, `ci-on-main`, dispatches `ci` on `main` once, with `post_merge: true`. It is the only job holding
   `actions: write`, holds nothing else, checks out nothing and reads no secret; the merging job
   only says that it merged, as an output. A merge made any
   other way, as every merge on 2026-10-18 was, under a person's token, starts the push run of
   item 1 instead.

5. The push run reads no secret and prints no environment: `ci.yml` names no secret at all, and
   on `main` it runs what the nightly already runs there.

### Rationale

Each pull request is tested on the base it was written against, but nothing tested the `main` it
landed on until the next night, and two changes that each pass can break `main` together, as #1616
and #1646 did. A run after every merge finds that within one run's time instead of within a day.

`push` was chosen as the trigger, with `merge.yml`'s dispatch only for its own merges, because
neither alone covers both ways a merge lands and the two never fire for the same merge: a push
made with `GITHUB_TOKEN` triggers no `push` run, and a merge made with a person's token never
passes through `merge.yml`'s dispatch. Dispatching from `merge.yml` alone would miss every manual
merge; `push` alone would miss every `merge.yml` merge.

### Alternatives considered

- **Scope the post-merge run to the merged commit's own diff.** Declined: with
  `cancel-in-progress`, the cancelled runs' changes would go untested; selecting everything makes
  the surviving run cover the whole burst.
- **Keep only the nightly.** Declined: a broken `main` would stay unnoticed for up to a day while
  every agent builds on it.

<a id="dec-2611"></a>

## DEC-2611: CodeQL analyses one push to `main` at a time, and the latest waits

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79); lane CT builds it |
| **Story or claim** | No story; CI throughput (lane CT). Amends [DEC-2606](process.md#dec-2606) item 2 for pushes to `main`, as [DEC-2610](process.md#dec-2610) item 3 does for `ci` |

### Decision

1. Pushes to `main` share one CodeQL concurrency group, `codeql-refs/heads/main`, with
   `cancel-in-progress` false. An analysis that has started finishes. GitHub keeps at most one
   more run waiting in the group and replaces it with each newer push, so a burst of merges is
   analysed by the run in progress when it began and by its latest push.
2. No commit goes unanalysed: the latest push's tree holds every earlier merge, and CodeQL
   analyses the whole tree, not the diff.
3. A pull request's runs keep their group per ref and still cancel the older run (DEC-2606
   item 1). The weekly schedule keeps a group of its own (`github.run_id`), so it never waits on
   or replaces a push's analysis.
4. Nothing else changes: the same four languages, config, queries and triggers (DEC-500,
   DEC-794).

### Rationale

The account runs at most 40 jobs at once. On 2026-10-19 at about 03:55 UTC, 8 of the 10 running
jobs were CodeQL `Analyze (rust)`, with 11 more push-to-`main` CodeQL runs queued, because DEC-2606
gave every push to `main` a group of its own and about 40 merges a day each queued four full
analyses. An analysis of a commit a newer push already contains finds nothing the newer one misses.

### Alternatives considered

- **Cancel the analysis in progress as well.** Declined: during a steady stream of merges no
  analysis of `main` would ever finish, as happened to `ci` on 2026-10-18 (DEC-2610 item 3).
- **Analyse `main` only on the schedule.** Declined: a finding would surface up to a week late.

<a id="dec-2612"></a>

## DEC-2612: A wrapped command word is judged against its own substitution

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | `claude-code` (lane SI), at the coordinator's direction, as a reversible CI change (DEC-79). It closes a hole and stops a false refusal, and it loosens no rule DEC-851 states (DEC-176) |
| **Story or claim** | No story; a defect lane RS found in the closed-world script reader (DEC-873) while reworking #1795. Fixes how [DEC-851](earlier-600-899.md#dec-851) item 6's `"$(dirname "$0")/<path>"` exception is looked up |

### Context

DEC-851 item 6 lets a command word that starts with an expansion through when it is
`"$(dirname "$0")/<literal path>"`. The reader checks that the substitution *at the command word's
position* is `dirname "$0"`. Behind a wrapper (`env -i A=1 B=2 …`), it found that position by
counting the wrapper's words in a list that had every `NAME=value` word removed. The command's
own word list drops only the leading ones. So each assignment behind the wrapper moved the
lookup one word on:
- `env -i A=1 B=2 "$(dirname "$0")/x.sh"` found no substitution and was refused, although item 6
  admits it.
- `env -i A=1 "$(cat cmd.txt)/x.sh" "$(dirname "$0")/y.sh"` judged the command word against the
  later word's `dirname` and was accepted. Its command word was the output of `cat cmd.txt`, which
  the closed world must refuse.

### Decision

1. The command word's position is read from the same word list the command is read from
   (`PipelineCommand::command_positions`, parallel to `command`). So a wrapped command word is
   judged against its own substitution, however many assignments or options the wrapper takes.
2. What this changes:
   - The second shape above is now refused. That tightens.
   - The first shape, and `env A=1 B=2 C=3 "$(dirname "$0")/x.sh" --flag`, are now read as item 6
     always meant. That widens only to item 6's own wording, so no shape item 6 refuses is newly
     accepted.
   - `xtask`'s `a_wrapped_command_word_reads_its_own_substitution` holds both.

### Alternatives considered

- **Refuse every expansion command word behind a wrapper:** this would refuse shapes item 6 admits
  today, `"$(dirname "$0")/x.sh"` behind `env`, for no gain once the position is read correctly.

<a id="dec-2613"></a>

## DEC-2613: CodeQL skips pull requests that change only documentation

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79); lane CT builds it |
| **Story or claim** | No story; CI throughput (lane CT), so the founder's `infra` runs wait behind less. Narrows when [DEC-2606](process.md#dec-2606)'s pull request analyses start; amends nothing else |

### Decision

1. `codeql.yml`'s `pull_request` trigger takes `paths-ignore: ["docs/**", "**/*.md"]`. GitHub
   skips the workflow only when **every** path the pull request changes matches one of them, so
   a pull request that changes any other file, code or not, is analysed exactly as before.
2. Neither pattern holds code CodeQL analyses: `docs/` holds Markdown, YAML, JSON and images,
   and a `.md` file anywhere is prose. A documentation-only pull request has nothing in the
   four analysed languages (actions, JavaScript and TypeScript, Python, Rust) that `main` does
   not already hold.
3. A push to `main` and the weekly schedule keep no filter, so `main` is analysed after every
   merge whatever it changed (DEC-2606 item 3, DEC-2611). No query, language or config changes
   (DEC-500, DEC-794).
4. No CodeQL check is required on `main` (its rules are the pull request, linear history, no
   force push and no deletion), so a skipped analysis blocks no merge.

### Rationale

The account runs at most 40 jobs at once, with no priority between workflows. On 2026-10-19 at
07:45 UTC about 233 jobs were queued or running (190 `ci`, 29 CodeQL, 14 `release`, 5 `merge`),
and the founder's `infra` staging plan waited behind them for the third time that day. Every
ready pull request started four CodeQL analyses, documentation-only ones (backlog rows,
decisions, the tracker, the work log) included. An analysis of a tree whose code is `main`'s
finds nothing the analysis of `main` misses.

### Alternatives considered

- **A list of code paths (`paths:`)** instead of documentation to skip. Declined: a new code
  directory would go unanalysed until someone added it; skipping only what is known to be prose
  fails safe.
- **Skip CodeQL on every pull request.** Declined: a finding would surface only after the merge.

<a id="dec-2615"></a>

## DEC-2615: The executor and vault drills run in a CI cell job of their own

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79); lane CT builds it |
| **Story or claim** | No story; CI. Keeps the cell's kill-switch, admission, executor (E21-30, DEC-2035) and vault (E21-32, DEC-2022) drills within DEC-464's ten minutes. Splits [DEC-2602](process.md#dec-2602)'s job; amends nothing else |

### Decision

1. `cargo xtask ci cell-kill-switch` brings up its cell and runs the `kill-switch` and
   `admission` drills, plain and seeded.
2. A new command, `cargo xtask ci cell-executor`, brings up its own cell and runs the `executor`
   and `vault` drills, plain and seeded. CI runs it as the `cell-executor` job. Like the other cell
   jobs, it starts when `select`'s `cell` output does (DEC-2609), restores `cell`'s cache, saves
   none of its own, and is not required, as `cell` is not (DEC-993 item 10).
3. `cargo xtask cell drill` with no names still runs every drill. No drill, seeded bug or limit
   changes.

### Rationale

Once the executor drill held (#1763, then lane X's fix), `cell-kill-switch` ran past its ten
minutes on `main` twice: runs 39231762100 (`17d13d6b`) and 39235225581 (`b4422099`) were
cancelled during the seeded executor drill. In the second, set-up took about 40 seconds and
bringing the cell up 3 minutes 19 seconds. The plain drills took 73, 36, 67 and 76 seconds
(kill-switch, admission, executor, vault), and the seeded kill-switch and admission 62 and 52;
the seeded executor and vault had still to run. That is about 12.5 minutes in all. Split this
way, each job brings the cell up once and stays near 8 minutes: about 7.7 for kill-switch and
admission, and about 8.7 for executor and vault, taking the seeded runs as long as the plain
ones. DEC-464 says to add parallel jobs rather than remove tests.

### Alternatives considered

- **Raise the job's limit.** DEC-464 forbids it.
- **Drop the seeded runs.** That would remove the proof that each drill catches its bug.
- **One job per drill.** That costs two more runners from the account's 40 per run for margin
  the two-way split already gives. It is the next step if `cell-executor` nears its limit, for
  example once the executor drill waits for a scheduler tick.

<a id="dec-2616"></a>

## DEC-2616: The vault drill runs in a CI cell job of its own

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the coordinating agent, as a reversible CI change (DEC-79); lane CT builds it |
| **Story or claim** | No story; CI. Takes the step [DEC-2615](process.md#dec-2615) named for when `cell-executor` nears its limit; amends DEC-2615 item 2 only |

### Decision

1. `cargo xtask ci cell-executor` brings up its cell and runs the `executor` drill, plain and
   seeded.
2. A new command, `cargo xtask ci cell-vault`, brings up its own cell and runs the `vault` drill,
   plain and seeded. CI runs it as the `cell-vault` job, gated, cached and not required as the
   other cell jobs are (DEC-2609, DEC-993 item 10).
3. No drill, seeded bug or limit changes (DEC-464).

### Rationale

Once the seeded executor drill opened its workspace (#1892), it reached its bug and caught it,
but it waits out the deployment manager's polls to show that no executor starts: 2 minutes 30
seconds on main run 39248288464 (`2aa10524`). With bringing the cell up at 3 minutes 23 seconds
and the plain executor and vault drills at 65 and 76 seconds, `cell-executor` was cancelled at
ten minutes in the seeded vault drill. Apart, the executor job is about 7.7 minutes and the vault
job about 6.7.

### Alternatives considered

- **Shorten the seeded executor's wait.** It is how the drill shows the manager starts nothing;
  a shorter wait proves less.
- **Move the vault drill into `cell-kill-switch`.** That job already takes about 8.5 minutes.

<a id="dec-2626"></a>

## DEC-2626: The OAuth callback writes the code straight to the vault, and nothing else

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176; each item reads the flow the way that accepts less, and none adds risk) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E10-13 slice 3, E7-1; [workspace API spec](../../specs/workspace-api.md) §4.5, §4.10; [connections spec](../../specs/connections.md) §5.2 step 3, CN-1; [DEC-821](DEC-821.md) item 3; [DEC-855](earlier-600-899.md#dec-855) items 2 to 4 (the founder's login callback, whose strictness this follows); [DEC-2623](DEC-2623.md) items 3, 4 and 9; [DEC-2628](process.md#dec-2628) item 1 |

### Context

`GET /v1/oauth/alpaca/callback` is where Alpaca sends the browser with the authorization code. It
sits outside the workspace prefix because a registered redirect URI cannot carry a workspace id;
the workspace comes from the single-use `state` (DEC-2623 item 3). The specs say it checks the
`state`, writes the code and verifier straight to the vault, and calls no broker. They do not say
how it reads the query, who the user is, what it answers, what it does when the owner declined at
Alpaca, or what it does when the request was closed while the owner was at Alpaca.

### Decision

1. **The user is the session's.** The browser returns with its session cookie (a top-level `GET`,
   which `SameSite=Lax` sends). Without a valid session the callback is the one `401` and spends
   nothing. The `state` is redeemed for that principal only: another user's `state` is refused and
   spent, as `PendingStates::redeem` does. No `Origin` or `X-Mandate-Request` is asked for, since
   the request is a navigation from Alpaca's page.
2. **The query, read strictly, in order** (after DEC-855 items 2 to 4):
   1. Each pair is decoded as `application/x-www-form-urlencoded`. A malformed escape refuses the
      whole query before any `state` is redeemed.
   2. `state` must appear exactly once; otherwise nothing is redeemed. It is then redeemed: an
      unknown or expired one, or another user's, is refused. A found `state` is spent either way.
   3. `error` present: the owner declined at Alpaca. Nothing is written.
   4. `code` must appear exactly once, as 1 to 512 bytes of visible ASCII (`!` to `~`); anything
      else is refused.

   Every other parameter is ignored, `iss` included: the deployment pins one authorization
   server, so there is no mix-up for it to prevent.
   - **The `state` is the CSRF guard.** Alpaca does not enforce PKCE (DEC-690 item 9). So what
     stops a forged callback is the `state`: 256 bits from the operating system's random source
     (DEC-2623 item 7), single use, bound to the user who started the connect, and expired after
     ten minutes. No refusal, log line, or answer carries the
   `state`, the code, or the query's text (CN-9).
3. **The owner must still hold the right, and the request must still be open.** After the redeem,
   the session's user is authorized again, as the start was, for *Connect, change, or revoke a
   broker connection* in the binding's workspace (DEC-2631 item 1). That workspace must be the
   hosted one. An admin demoted or removed during the ten minutes at Alpaca gets no code written.
   The control stream read for that check must then hold the connection's request open: a
   `ConnectionRequested` not yet closed by `ConnectionEstablished`, `ConnectionRefused` or
   `ConnectionRevoked` (journal spec §9.8 rule 131). A request closed while the owner was at
   Alpaca, by a revoke or the deadline teardown, gets no code. So does a control stream that
   cannot be read or folded. This is tighter than the specs ask, so DEC-176 covers it.
   - **The residual window.** A close journaled after that read and before the vault write
     returns can still leave a code for a closed request. It is closed from both sides:
     - the revoke and the deadline teardown delete the connection's vault entry after they
       journal the close (connections spec §5.2 step 6; backlog story E10-21 tracks it);
     - the manager deletes a running exchange's pod as soon as the request closes (DEC-1175
       item 3).

     An exchange that took the code before the close can still complete and write a token for a
     connection that is never established. E10-21's cleanup deletes that token, and revokes it at
     Alpaca where Alpaca allows. No process reads it in the meantime: the manager never
     establishes the connection, and the executor reads only established connections. A code or
     token left in that window is therefore deleted and never used once E10-21 lands.
4. **The vault write.** The code and the verifier redeemed with the `state` are written as one
   entry, through the API's vault port, under the binding's connection and environment. The API's
   identity may write only codes (`mandate-vault` policy; DEC-2628 item 1). Both pass through the
   process only as `SecretString`s and are dropped when the write returns. A failed write starts
   nothing, and the deadline's teardown closes the request.
5. **Nothing else.** The callback appends nothing to the journal, calls no broker, and does not
   start the exchange. What starts the token-exchange Job is lane D1's (DEC-1175).
6. **The answer.** Every outcome past the session is a `303`:
   - `Location: {app_origin}/connections/{connection_id}` once a redeemed `state` named a
     connection;
   - else `{app_origin}/connections`.

   Every answer carries `Cache-Control: no-store` and `Referrer-Policy: no-referrer`, so the code
   in this request's URL is neither cached nor sent onward. What happened is read from
   `GET /connections/{id}` (DEC-2620), never from the redirect.
7. **No `state` without a connect.** A deployment that offers no connect (DEC-2623 item 9) answers
   the callback `not_found`, before the session is read.

### Rationale

The callback is the one route an outside site sends a browser to, carrying a secret in the URL. It
reads only what it needs, in an order where a forged or replayed request learns nothing. It writes
the secret once to the one place that may hold it, and only for a request still open. It answers
with a redirect that carries neither the secret nor the outcome.

### Alternatives considered

- **Answer the outcome in the redirect** (`?connect=refused`). It would show a reason to whoever
  holds the URL; the connections read already tells the owner.
- **Journal a refusal at the callback.** A refused callback has no step-up and no account, and the
  request's own close (refusal, teardown) already records the outcome.
- **Require `iss`, or check it when present.** Alpaca documents none, so requiring it would refuse
  every real callback. With one pinned server, checking it guards against nothing.
- **Write the code whatever the request's state.** A revoked request's code would sit in the vault
  until its teardown, and a token-exchange Job started on it would exchange for a closed request.

<a id="dec-2628"></a>

## DEC-2628: The token exchange logs in by Kubernetes auth and consumes the code before it sends

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, approved by the coordinator on 2026-10-19; item 1 takes the vault login the coordinator approved for every workload, and item 2 tightens DEC-821's exchange order so a code is sent at most once, adding no risk; item 2's wrong-binding put-back amended by [DEC-2320](trading-core.md#dec-2320)) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lanes V2 and D1 |
| **Story or claim** | E10-13 slice 5b, E7-1; [connections spec](../../specs/connections.md) §5.2 steps 3, 4 and 6; [DEC-821](DEC-821.md) item 2; [DEC-2627](DEC-2627.md) items 2 and 4, which this amends; [DEC-2626](process.md#dec-2626) |

### Context

DEC-2627 item 2 handed the token-exchange Job a response-wrapped, single-use grant that the API
stored at the callback. Lane V2 has since moved the Job to the vault's Kubernetes auth, the login
every cell workload uses: the Job's service account logs in with its projected token under one
templated policy that reads the client secret, reads and deletes its own connection's code, and
writes its own token. That login is not single-use, and the grant was what stopped a second run of
the Job from exchanging a code again. The exchange stored the token first and deleted the code
after (DEC-821's order), so a run that stopped between the two left the code behind for a rerun,
and an authorization server may revoke the tokens issued from a code presented twice (RFC 6749
§4.1.2).

### Decision

1. **Kubernetes auth replaces the wrapped grant** (amends DEC-2627 item 2), as DEC-2060 (pending,
   #2013) sets it for the cell's workloads. The callback (E10-13 slice 3; DEC-2626, pending) writes
   only the code and the PKCE verifier, and the API holds no grant. The Job runs
   under its connection's token-exchange service account and logs in with its projected token. Its
   reach is unchanged: the client secret, its own code (read and delete), and its own token (write).
2. **The exchange takes the code before it sends** (amends DEC-2627 item 4 and DEC-821's
   order). `Vault::take_code` reads the code and verifier and deletes them in one atomic step, so
   of any number of runs, reruns or overlapping runs, exactly one gets them and every other finds
   none. Only then is the token request sent. A refused grant leaves nothing to delete; an
   accepted one is stored as the token. Every vault backend must make the take atomic:
   - The local vault renames the entry, without replacing any existing name, to a name drawn from
     16 random bytes that only the taker knows, then reads and deletes that name. A take that stops
     between the rename and the delete leaves a `.taken-` file, which the connection's `delete` and
     `delete_code` sweep.
   - The full vault must take the code under a check-and-set on the entry's version, so that a
     taker that loses the race gets none. Lane V2 provides it as `SecretSource::take_code` in
     `mandate-vault` (PRV-2c-3; DEC-2061 grants the exchange the update right it needs), and its
     conformance suite pins the property with a racing pair. The token-exchange binary (slice
     5b-3) calls it and maps none to `CodeMissing`.
   - A taker the code is not bound to (another connection's or environment's) puts the entry back,
     so it can hide the code from the rightful taker for that moment. The rightful run then finds
     none and loses the connect, but the code is never sent twice.
   - A run that stops after the take and before the request is sent loses the connect.
   - A run that stops after the request and before the token is stored loses it too.

   Either way the request stays open until the deadline teardown refuses it (§5.2 step 6), and a
   rerun finds no code and sends nothing.
3. **The orchestrator is the second fence, and stays.** Lane D1 runs the Job with
   `restartPolicy: Never` and `backoffLimit: 0`, and starts no second Job for a connection whose
   Job has run, until the request is closed; its tests pin that. Neither fence is dropped because
   the other holds: the take guards the code, and the fence keeps a second process from being
   started at all.

### Rationale

Single use now lives in the one place every run passes through, the code itself, taken in one
step. It no longer depends on a credential that the vault's standard login does not issue, or on
the orchestrator alone never restarting a pod. Losing a connect to a crash costs the owner a retry. Sending a code twice
could revoke a token the executor already holds.

### Alternatives considered

- **Read, then delete, in two calls.** Two overlapping runs could both read the code before
  either deleted it, and an idempotent delete refuses neither.
- **Keep the order and rely on the orchestrator.** It works only while no pod is ever restarted
  or recreated, a property of another lane's configuration that this crate cannot check.
- **Keep a single-use grant beside Kubernetes auth.** It would need a second login path and a
  per-connect secret in the vault only to guard an order the code can fix itself.

<a id="dec-2629"></a>

## DEC-2629: The token-exchange process and its HTTP adapter

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, approved by the coordinator on 2026-10-19 with item 3's three conditions; release builds can reach only the pinned live token URL, and the cell's override is compiled out of them, so no safety rule is weakened) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E10-13 slice 5b; [connections spec](../../specs/connections.md) §5.2 step 4; [DEC-821](DEC-821.md) items 2 and 4; [DEC-2627](DEC-2627.md); [DEC-2628](process.md#dec-2628); [DEC-2625](DEC-2625.md); [DEC-2100](DEC-2100.md) |

### Context

`mandate-connections` holds the exchange's rules and reaches the network only through
`exchange::TokenEndpoint`. The token-exchange process needs an adapter that sends the one live
request (`hosts::LiveTokenRequest`) over HTTPS. A dev or CI cell has no Alpaca client and must
reach the fixture provider instead (DEC-2625). That provider is not at the pinned URL.

### Decision

1. **A crate of its own.** `mandate-token-exchange` is safety-critical. It holds the adapter now
   and the process's binary later. It is the only crate besides `mandate-connections` that names
   the token endpoint, and no other process links it (DEC-821 item 2).
2. **The adapter sends exactly one request per exchange.**
   - The request is a `POST` to the `LiveTokenRequest`'s URL.
   - The body is `application/x-www-form-urlencoded` with exactly `grant_type`, `code`,
     `redirect_uri`, `client_id`, `client_secret` and `code_verifier`, each once and in that
     order. Each value is percent-encoded as RFC 3986's unreserved set allows.
   - It accepts HTTPS only and follows no redirect: a `3xx` is a failure. Connect and total
     timeouts are bounded, and it never retries.
   - It reads only `200` with an `application/json` body of at most 16 KiB, and only its
     `access_token`, `token_type` and `scope`. Any other answer is `endpoint_failed`.
   - No error, log line or `Debug` output carries the code, the verifier, the client secret or
     the token. The token is read through `mandate_canon`'s value tree, where it is a plain
     `String` until it is moved into a `SecretString`; that tree lives only inside the reader and
     is dropped when it returns.
3. **The cell's override**, under three conditions:
   1. **Debug builds only.** Its code exists only with debug assertions, so a release build has
      none. The process's release-build check pins that, as the stub issuer's does (DEC-2100
      item 2).
   2. **Dev and CI tiers only.** It is refused unless `MANDATE_DEPLOYMENT_TIER`, read as every
      process reads it, is `dev` or `ci`.
   3. **Loopback, never Alpaca.** It names only `http` or `https` on a loopback host (`127.0.0.1`,
      `[::1]`, or `localhost`), with a port if any, a path, and no user information, query or
      fragment, kept exactly as given. It never names `alpaca.markets` or any host under it, so
      it cannot send a real exchange anywhere but the pinned URL.

   Slice 2e's authorization page URL takes the same gate. How a cell exposes the loopback-bound
   stub issuer to other pods is lanes D1's and GO's to settle; until then the override stays
   loopback-only.

### Rationale

The pinned request stays the only one a release build can send. The cell runs the same adapter
against the provider DEC-2625 built, behind the same gates as the stub issuer itself.

### Alternatives considered

- **A configurable URL in every build.** It makes the live host a setting an operator could point
  elsewhere, which DEC-821 item 2 rules out.
- **DNS pinning of the live host to the fixture with a test CA.** It needs a CA the process trusts
  in the cell, a second trust path more complicated than the override.

<a id="dec-2630"></a>

## DEC-2630: the vault is provisioned on the product path: each identity's credentials as a Kubernetes Secret, mounted into its process

| | |
|---|---|
| **Status** | Accepted: items 1 to 6 by the agent (DEC-79, reversible engineering choices); item 7, where the on-connect provisioning lives, by the founder ([DEC-2605](process.md#dec-2605) row 23); item 3's CLI build superseded in part by [DEC-2116](workspace.md#dec-2116) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | the agent (Claude Code, lane V2), under DEC-79, at the coordinator's direction, for items 1 to 6; lanes LV (the mount) and GO (the apply's call site and the vault's ingress) agreed their parts. The founder, on 2026-10-19 at 15:05 UTC, for item 7: "Dedicated provisioner (Recommended)" (DEC-2605 row 23) |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); builds on [DEC-2022](infrastructure.md#dec-2022), [DEC-2052](infrastructure.md#dec-2052) item 4, DEC-977 items 2 and 4 |

### Context

DEC-2022 provisions the vault with `mandate vault provision`, which writes credential files, run by
hand or by the cell's one-off Job. Staging's apply sets the vault up from CI (DEC-2052), and its
item 4 leaves provisioning to come "from a short-lived root token generated from the stored
recovery keys". No process can read its credentials until they reach it as a mounted file, and
nobody runs a command by hand on staging.

### Decision

1. **Secrets.** Each identity's credentials are one `Opaque` Secret in the workspace's namespace,
   keys `role-id` and `secret-id`, labelled `app.kubernetes.io/managed-by:
   mandate-vault-provision` and annotated with its vault role (`mandate_vault::kube`).
   - A platform identity's is `vault-approle-<role>`.
   - An executor's is `vault-approle-executor-<hex>`, the connection id's bytes in lowercase
     hexadecimal: a connection id may hold capitals and underscores that an object name may not,
     and hex never makes two connections share a name.
   - A token exchange has none: its credential is a single-use grant (DEC-821 item 2).
2. **The CLI prints them to a pipe.** `mandate vault provision --secrets-namespace <ns>` writes no
   file and prints the Secrets as one `v1` `List`; it refuses a terminal and a namespace that is not
   a label before the token is read. `--operator-token-stdin` reads the token from a pipe by the
   token file's rules (`credentials::read_token`, at most 4096 bytes). The file modes stay (DEC-2022
   item 2), as the founder's break-glass path.
3. **Staging's apply provisions** (`xtask/src/vault_provision.rs`), after `vault_init` and before the
   verify, in the same window; only `bootstrap_staging` calls it, and a test pins the order.
   - When every Secret it would write is already there it changes nothing, so an apply re-issues no
     secret id. The check is by the Secrets' names alone: a Secret that is present but stale (its
     role re-created, its secret id destroyed) is not noticed; deleting it makes the next apply
     provision again.
   - Otherwise it reads the recovery keys from `vault-recovery/vault-init` (DEC-2052 item 3),
     cancels any root-generation attempt a crashed run left open, starts one, and feeds two keys,
     each on `kubectl exec`'s standard input to `bao write sys/generate-root/update key=-`. It
     decodes the root token itself: the vault's `encoded_token`, unpadded base64, XORed with the
     attempt's one-time pad, which only its standard output carries.
   - It runs `mandate vault provision --operator-token-stdin --secrets-namespace ws-ws1` through a
     `kubectl port-forward` to `vault-0` (the certificate names 127.0.0.1; a port-forward is the
     kubelet's, so no policy changes), its environment cleared, the CA from the ConfigMap
     `vault-ca`, and hands what it prints to `kubectl apply -f -` on standard input.
   - It revokes the root token on every path, a guard covering each early return and panic, and a
     failed revoke stops the run saying the token may be live.
   - Every key, pad, encoded token and token is masked the moment it exists and is never an
     argument, an environment variable, a file or a log line (DEC-2052 item 5).
   - The CLI is built before the control-plane window opens, so the step fits in it.
4. **Root generation on the pod's loopback only.** OpenBao 2.7.1 answers its unauthenticated
   `sys/generate-root/*` only on a listener with `disable_unauthed_generate_root_endpoints = false`,
   and `bao operator generate-root` uses an authenticated path a tokenless caller cannot. Staging's
   vault alone (the static seal) gets a second listener, `127.0.0.1:8210`, TLS 1.3 from the chart's
   certificate; only `kubectl exec` or `kubectl port-forward` into the pod reaches it, and no
   Service or policy names it. The listener every process reaches keeps root generation off, as by
   default. Whoever can exec or port-forward into the pod holds a cluster-admin-level grant
   (`pods/exec`, `pods/portforward`) and can already read the recovery keys (DEC-2052's accepted
   trade), so this adds no reach. `deploy/tests/test_vault.py` pins it.
5. **Which connections.** Until the on-connect trigger lands, staging provisions the paper
   connections listed in `deploy/hosted/vault-connections.json` (ids, which are not secret); a
   listed connection without its Secret makes the next apply provision.
6. **The mount.** A process reads its credentials with `mandate_vault::mounted`:
   - `read_mounted_credentials(dir, identity)` reads `<role>.role-id` and `<role>.secret-id`
     (DEC-2022's names); the pod's secret volume maps the keys to them with `items`, under
     `/run/mandate/vault/`, with `ca.crt` beside them.
   - The kubelet's files are links through `..data`, so `read_mounted` follows links only to a file
     inside the mount (an absolute or climbing target is refused), judges the mode on the file
     reached (no group or other bit), and refuses a file whose device and inode differ from the one
     checked.

### The on-connect trigger, the founder's

7. **A dedicated provisioner** (DEC-2605 row 23). A connection made from the web exists only from
   that moment, so item 5 cannot be the end state; today `provision` also makes a connection's
   `exchange-<c>` role only for the connections it is given. A spike on OpenBao 2.7.1 found:
   - one AppRole role whose secret ids carry the connection in their metadata is unsafe: the login's
     alias is per role, each login overwrites its metadata, and a token minted for one connection
     read another's after that connection logged in;
   - one templated executor policy, reading
     `mandate/data/connections/{{identity.entity.aliases.<accessor>.metadata.role_name}}/token`,
     with one role per connection on a dedicated AppRole mount, holds: each reads its own token only,
     across repeated logins;
   - a provisioner token that may only create those roles bound to that one policy (pinned with
     `allowed_parameters`, and `denied_parameters = {"*" = []}` on secret ids) holds: root, other
     policies, policy writes, token creation and every other path are refused.

   Any holder of that provisioner token can make an executor's credentials for any connection, and
   so read every connection's broker token. Neither the token-exchange process (it reads no token)
   nor the connection manager (it can destroy or overwrite tokens but read none) reaches that today,
   so the founder chose to give the power to neither: one dedicated provisioner process holds the
   scoped policy, creates each connection's executor and token-exchange roles, on their own mounts
   and bound to the templated policies, when the connection manager asks it over the journal, has
   no egress but the vault, and has no other duty. It is built as its own pairs, with a test that
   its policy refuses each path above; its network policy is lane GO's to agree. Until it lands,
   item 5 stands.

### Rationale

The CLI stays the one provisioning command (DEC-2022), now usable without a file, so staging, the
cell and the founder's break-glass all run the same code. Generating the root token for each run,
and revoking it, keeps no standing root token anywhere, as DEC-2052 promised.

### Alternatives considered

- **Linking `mandate-vault` into xtask.** Rejected: it would pull the vault's HTTP client into the
  tool crate, and two provisioning paths would drift.
- **Running the CLI in a pod with `kubectl run`.** Rejected: its standard output, the Secrets, would
  stay in the kubelet's container log.
- **`subPath` mounts**, which are plain files. Rejected: they never refresh.
- **The provisioner's power in the token-exchange process or the connection manager.** Rejected
  by the founder (item 7): either would widen what that process can reach.

<a id="dec-2631"></a>

## DEC-2631: Who may read the connections

| | |
|---|---|
| **Status** | Accepted (agent: under DEC-176, the reading of a gap that only tightens; it serves the connections read to fewer roles than the other reading would, adds no risk, and weakens no safety rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E10-13 slice 1b; [workspace API spec](../../specs/workspace-api.md) §3.7, §3.8, §4.5; [identity spec](../../specs/identity.md) §4.2; [DEC-2620](DEC-2620.md) |

### Context

§3.7 maps each route group to a row of identity spec §4.2. Two rows could govern `GET /connections`
and `GET /connections/{id}`:
- §4.5's connect, revoke and revoke-on-compromise map to *Connect, change, or revoke a broker
  connection*: the workspace admin, with step-up.
- The read models of §4.7 map to *View agents, positions, decisions*: the workspace admin, operator,
  approver, viewer, auditor, and a client.

The connections read is §4.5's, and its shapes are named `read-models/…`. §3.8 gives a client's
`read` scope agent status, positions and plans, and never the connections. DEC-2620 does not say
which row applies.

### Decision

1. **The connections read takes the broker-connection row.** Both routes are authorized as
   *Connect, change, or revoke a broker connection*, without step-up, since reading changes nothing.
   Only the workspace admin reads them; any other member is `forbidden`.
2. **The role before the id.** The permission is checked before the id is looked up, so a member
   without it learns nothing about which ids exist. An id the control stream does not name, an id
   another workspace's stream names, and an id that does not decode are each the same `not_found`.

### Rationale

The screen that reads connections is the one that connects and revokes them, and only the
workspace admin may do either. Serving the read to every role that views agents would hand a viewer
and a client the list of broker connections, which §3.8 keeps from a client. This reading serves
fewer roles than the other and changes nothing else; widening it later is a new decision.

### Alternatives considered

- **The view row.** It matches the shapes' `read-models` name, but it would serve a client what
  §3.8 leaves out, and the operator and viewer gain nothing they need from it.

<a id="dec-2632"></a>

## DEC-2632: main's red gates are repaired in the schema corpus, the secret scan's allowlists and the lockfile

| | |
|---|---|
| **Status** | Accepted: item 1 by the agent (a test's carve-out for a schema that landed ahead of its type, which [DEC-2103](DEC-2103.md) item 8 already provides for); items 2 to 4 by the agent under DEC-79 as reversible engineering repairs, none of which weakens a safety rule |
| **Date** | 2026-10-20 (UTC) |
| **Decided by** | an agent (`deepseek-ai/DeepSeek-V4.1-Flash-1017`, Cursor), acting as the coordinating session under DEC-79, because no builder session held a claim on this repair |
| **Story or claim** | no story; a repair of `main`. [workspace API spec](../../specs/workspace-api.md) §3.6, §4.5; [DEC-2103](DEC-2103.md) item 8; [DEC-2631](process.md#dec-2631); [DEC-2625](DEC-2625.md); [DEC-682](earlier-600-899.md#dec-682) item 31 |

### Context

`main` at `209e8935b` is red on its own `ci` run ([#2052](https://github.com/kunwarshivam/mandate/issues/2052))
and on the nightly ([#2054](https://github.com/kunwarshivam/mandate/issues/2054)). Twenty-four pull
requests are open and none has moved since 2026-10-19 18:17Z. Every ready PR's red CI is inherited
from `main`, not caused by its own diff: the failures below reproduce on a clean checkout of
`main`. Nothing merges while they stand (DEC-79 requires green CI, and the work log's lesson is
that nothing merges red).

Four independent faults, each from a change that merged while its own check was red or was not run:

1. **A request schema landed without its `.valid.json` example.** `b8809162` ([#2021](https://github.com/kunwarshivam/mandate/pull/2021))
   added `commands/passkey-registration-challenge-request.schema.json` and its `.json` and
   `.invalid.json` examples but not its `.valid.json`. `body_workspace.rs`'s `bodies()` reads that
   file for every stem unconditionally, so `the_bodies_reach_every_object_their_schema_describes`
   panics at `body_workspace.rs:41` on every run.
2. **Two schemas landed ahead of their types with no carve-out.** The same change added
   `passkey-registration-challenge-request`, and `#2021`'s sibling added `passkey-enrol-request`.
   Neither has a `mandate_api::requests` type or a decoder row. `SCHEMA_AHEAD_OF_TYPE`, the list
   [DEC-2103](DEC-2103.md) item 8 provides for exactly this three-step sequence, was emptied to
   `[&str; 0]` by `117cd8570` ([#1992](https://github.com/kunwarshivam/mandate/pull/1992)) while two
   schema-ahead stems were still in flight, so
   `the_decoders_cover_every_request_schema_and_request_type` fails at `body_workspace.rs:157`.
3. **The secret scan reports 54 findings, none of them a credential.** They are the WebAuthn
   challenges and authenticator data of the new passkey examples, the `credential` sentinels the
   `#1993` journal reference cases carry (21 in `journal.yaml`, 21 in `fixtures/refcases/journal.json`,
   2 in `reference/journal/credentials.py`), and one deliberately wrong `client_secret` in the stub
   issuer's test. `.gitleaks.toml` pins every exception to one value in one file; these values are
   new and had no entry.
4. **`cargo deny` fails on `RUSTSEC-2026-0382`.** `rustls-webpki` 0.103.15 accepts certificate paths
   through distrusted emSign intermediates whose private keys are public; 0.103.16 rejects them.
   This was masked: `full-checks` runs `cargo deny` and the secret scan in one step and the scan
   failed first, so the advisory went unreported.

A fifth fault is separate and left alone: `cell-demo-web`'s `web/` gesture fails on `main`. It is a
Playwright failure in a job this repair does not touch, and it is recorded in the tracker's open
issues rather than fixed here.

### Decision

1. **`SCHEMA_AHEAD_OF_TYPE` names both stems again.** It lists `passkey-enrol-request` and
   `passkey-registration-challenge-request`, which is the state DEC-2103 item 8 defines for a
   schema that landed before its type. The change that adds either type empties the list again, and
   the test's own first assertion refuses a listed stem that keeps its place once its type lands.
   This is a repair of a list that was emptied too early, not a new exception.
2. **The missing example is added as an empty case list.** `passkey-registration-challenge-request`
   describes a strict empty body, so its `.valid.json` is `[]`: there is no member a valid case could
   set, and every member the route refuses is the invalid file's. The file must exist because
   `bodies()` reads it for every stem; `[]` is the honest content.
3. **The mutant survivors are killed by examples, not by weakening the sweep.** 27 mutants survived
   the schema sweep: 5 in `commands/oauth-start-request.schema.json` and 22 in
   `read-models/connection.schema.json`, each an example the corpus was missing. They are killed by
   `.invalid.json` cases that name the constraint, and — for the four `Refusal.check` values and
   `revoked_reason.compromised` — by `.valid.json` cases that pin a value the enum must still admit.
   No entry is added to `DELIBERATE`. Two of the seven `Connection.required` survivors are masked by
   the schema's own `allOf` conditionals: a well-formed instance that omits `state` still fails
   because the `else` branch demands a null `refusal`, so the case has to set a `refusal` that only
   the mutant would then admit. Each such case says so in its label.
4. **The secret scan's allowlists name exact values in exact files.** Two entries: the passkey
   challenge values join the existing step-up challenge entry, and a new entry covers the passkey
   `authenticator_data` and the `credential` sentinels across the five files that carry them. Both
   pin the full matched text, so another value in those files, or the same value anywhere else, is
   still reported. `cargo xtask ci supply-chain` plants an allowed case, a near miss in the allowed
   file and a stray copy elsewhere for each value; they are added with the entries.
5. **`rustls-webpki` goes to 0.103.16.** A lockfile-only `cargo update -p rustls-webpki`, which is
   the advisory's own stated fix. No manifest changes; no API this workspace calls changed.

### Rationale

Green CI is the gate every other lane is waiting on, so this repair is worth doing before any new
story. Each item restores a check rather than relaxing one: items 1 and 2 restore the state
DEC-2103 item 8 defines, item 3 adds assertions, item 4 narrows an allowlist to exact values, and
item 5 takes a security fix. `DELIBERATE` is untouched, so the sweep is as strict as before.

The alternative — merging the open PRs over red CI — was refused: the work log's own lesson is that
a red check merged over hides the failures behind it, and sixteen hid behind #363 once.

### Alternatives considered

- **Split the change into a schema PR and a code PR, as ES-22 asks.** Refused, and this is the one
  place this change cannot follow the guard. The halves are mutually red:
  - the schema PR (the missing example and the mutant cases) still fails
    `the_decoders_cover_every_request_schema_and_request_type`, whose `SCHEMA_AHEAD_OF_TYPE` lives
    in `crates/mandate-api/tests/body_workspace.rs`, so it needs the tests file;
  - the tests PR needs the stems its schemas introduce to exist, or `schema_stems()` reads a shorter
    set than the file lists.

  This is [DEC-917](earlier-900-1999.md#dec-917)'s finding, one step further: DEC-917 proposes that a crate's
  `tests/` file stop counting as code, which would let the tests file travel with the schemas. That
  decision is **Proposed**, not Accepted, and its PR #1321 is closed. It is the founder's, so this
  repair cannot take it; and the author of a change may not relax the guard that judges it. The
  repair is submitted whole, under the coordinator's review, and the founder's ruling on DEC-917 is
  what lets the next such change land cleanly.
- **Add the missing request types instead of re-listing the stems.** Adding
  `PasskeyEnrolRequest` and `PasskeyRegistrationChallengeRequest` would empty the list honestly, but
  it is the route work of E9-4 S3.1 and S3.2, not a repair, and it needs the spec guard's
  schema-then-type sequence. Item 1 is the smaller, correct step and leaves that work to its lane.
- **Add the surviving mutants to `DELIBERATE` with reasons.** Refused: each survivor is killable by
  an example, so listing it would hide a missing assertion rather than explain one that no instance
  can reach.
- **Allowlist the whole passkey example directory.** Refused: it would stop reporting a real secret
  pasted into any passkey example, which the per-value discipline exists to prevent.
- **Ban `rustls-webpki` in `deny.toml` or ignore the advisory.** Refused: the advisory is a real
  certificate-validation defect with a published fix.

### Consequences

- `main`'s `ci` and nightly should go green once this lands, which unblocks the twenty-four open PRs'
  inherited red checks. Their own diffs still need their own green run and an independent review.
- The two schema-ahead stems stay listed until E9-4 S3 adds their types. The test refuses a listed
  stem whose type lands while it is still listed, so the list cannot go stale silently.
- `cell-demo-web` stays red until its `web/` gesture is fixed; it is a separate story.
- The spec-guard deadlock above is unresolved until the founder rules on DEC-917. Every later
  change that pairs a schema with the test that pins it hits the same wall.
- No journal event: this changes no runtime state.

<a id="dec-2633"></a>

## DEC-2633: A cell's token and authorization URLs may name the oauth-fixture pod

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, approved by the coordinator on 2026-10-19 in the tightened form below; it widens DEC-2629 item 3 condition 3 by one exact authority, only in debug builds and only in the dev and ci tiers, which no staging or production deployment runs; item 2's release gate made explicit by [DEC-2322](trading-core.md#dec-2322)) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lanes D1 and GO |
| **Story or claim** | E10-13 slices 2e and 5b; [DEC-2629](process.md#dec-2629) item 3, amended here; DEC-1176 (lane D1, the `oauth-fixture` pod); [DEC-2625](DEC-2625.md); DEC-2632 |

### Context

DEC-2629 item 3 let a dev or CI cell point the token exchange at the fixture OAuth provider, on a
loopback host only, until the cell's exposure of the provider was settled. Lanes D1 and GO have
since settled it as DEC-1176: a separate `oauth-fixture` pod, which the exchange's Job reaches at
`http://oauth-fixture:8091`. A loopback-only override cannot name it.

### Decision

1. **One more authority.** Besides a loopback host, a cell token URL (`MANDATE_TOKEN_URL`) may name
   exactly `http://oauth-fixture:8091`: plain `http`, the host `oauth-fixture` and the port `8091`,
   each matched byte for byte. No other single-label name, no trailing dot, no other case, no other
   port, and no `https` to that name. No Alpaca host is ever admitted (DEC-2629 item 3).
2. **The other gates are unchanged.** The override's code exists only in builds with debug
   assertions, and it is refused unless `MANDATE_DEPLOYMENT_TIER` is `dev` or `ci`. A release
   build, or a staging or production tier, refuses to start when the variable is set at all (the
   exchange binary, E10-13 slice 5b-3, pins that with its release-build check). D1's chart renders
   the variable in dev and ci only and refuses it in staging and production, a second fence.
3. **The authorization page URL takes the same gate.** `MANDATE_ALPACA_AUTHORIZE_URL`, which slice 2e
   adds to the API, admits the same authorities under the same conditions.

### Rationale

The pod's name, scheme and port are fixed by the cell's chart, so pinning all three admits the one
target the cell needs and nothing a mistyped or stray value could reach.

### Alternatives considered

- **Any single-label host.** Simpler, but it would admit any service a cell happens to run under a
  short name.

<a id="dec-2634"></a>

## DEC-2634: `cell-demo-web` does not run on a pull request

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79: a reversible CI-scope change that weakens no safety rule and no `AGENTS.md` non-negotiable; it narrows *when* a job runs and never what it checks) |
| **Date** | 2026-10-20 (UTC) |
| **Decided by** | an agent (`deepseek-ai/DeepSeek-V4.1-Flash-1017`, Cursor), acting as the coordinating session under DEC-79, and taking the founder's instruction of 2026-10-20 19:10 PDT to scope the job off the per-PR path |
| **Story or claim** | no story; CI throughput and the merge path. Amends [DEC-2609](process.md#dec-2609) item 5; reads against [DEC-1190](earlier-900-1999.md#dec-1190), [DEC-610](earlier-600-899.md#dec-610), [DEC-175](DEC-175.md), [DEC-993](DEC-993.md) item 10 |

### Context

`cell-demo-web` (DEC-2609 item 5) starts on a pull request whose diff is the cell jobs' or touches `web/` outside its Markdown. It has **never passed**: it merged in [#1666](https://github.com/kunwarshivam/mandate/pull/1666) on 2026-10-18 23:20Z and has concluded `failure` on every run since ([#1666](https://github.com/kunwarshivam/mandate/pull/1666), #2031's run, `main` at `209e8935b`, `main` at `71a5e2b1a`). Its one test times out at `web/e2e-cell/kill-switch.spec.ts:29` waiting for a `GET …/agents/{id}`.

The job is in **neither** `fast`'s nor `full`'s `needs`. The two aggregates that the merge path requires are:

- `fast` → `needs: [fast-lint, fast-test, fast-pending, fast-python]`
- `full` → `needs: [full-checks, mutants-plan, mutants, schema-mutants]`

So no required verdict includes it, and the workflow comment has said since DEC-993 item 10 that the demo is "not yet in `full`". But `.github/scripts/merge-approved.sh:108` does not merge on the aggregates alone. It reads the **whole** `ci` run:

```sh
latest_run ci.yml   # the latest run for the head
[ "$conclusion" = success ] || skip "has $workflow at $conclusion on $sha"
```

A run concludes `failure` if *any* job fails, required or not. So one non-required, never-passing job made every pull request's `ci` run red, and `merge-approved.sh` refused every one of them. On 2026-10-20 that was 20 open PRs, none of which had a fault of its own.

The script's own header states the narrower contract it means to keep: "the latest `ci` run … succeeded with its `fast` and `full` jobs run and green". `required_jobs()` checks exactly that. The whole-run check on the line above is what made a job nobody required into the merge gate.

### Decision

1. **A pull request's run withholds `cell-demo-web`.** `cargo xtask ci select` keeps computing `cell_web` as DEC-2609 item 5 defines it — the selection stays pure and its tests unchanged — and `selection()` clears it when `GITHUB_EVENT_NAME` is `pull_request`. The job's `if: needs.select.outputs.cell_web == 'true'` then reads false, so GitHub reports it `skipped`, and a skipped job does not fail a run.
2. **Every other run keeps it.** `main`'s own push, a `workflow_dispatch` (which is how the nightly reaches CI), and a local `cargo xtask ci select` all keep `cell_web` as the diff selects it. On `main` a red `cell-demo-web` is a finding, which is what it should be.
3. **Nothing about what the job checks changes.** The job, its steps, its Playwright gesture, its artifact and its ten-minute bound are untouched. Only the set of events that start it narrows.
4. **The `ci` run's conclusion is not redefined.** `merge-approved.sh` is not edited here. A separate decision may later align its whole-run read with the `fast`/`full` contract its header states; this one removes the only job that made the two disagree, which is the smaller and reversible fix.

### Rationale

A gate should require what it says it requires. The repository had already decided the demo is not part of `full` (DEC-993 item 10) and had already given `select` the power to scope jobs per diff (DEC-1190). What was missing was the event: a job that drives a live cell through a browser is a `main`-branch and nightly signal, not a per-pull-request one — its cost is a k3d cluster, CloudNativePG, Helm releases and Chromium, it sits at its ten-minute cap, and it had never once produced a green result to protect.

DEC-2609 item 5 chose "the cell jobs do, or `web/` changed" to *maximise coverage*, and that reading is right for `main`. On a pull request it had the opposite effect to the one intended: because the job could not pass, it did not add coverage, it removed the ability to merge — twenty pull requests' worth.

The conservative direction is preserved: the job still runs everywhere the coverage matters, the gesture is still required before `main` moves, and the diagnostic PR [#2057](https://github.com/kunwarshivam/mandate/pull/2057) makes its next failure name its cause.

### Alternatives considered

- **Edit `merge-approved.sh` to merge on `fast` and `full` only.** It matches the script's own stated contract and would fix the class of problem, not one instance. Refused for now: it changes the merge gate itself, which DEC-175 governs, and it would let *any* future non-required job fail silently on every PR. Narrowing one job's events is the smaller change, and it is reversible by deleting one function.
- **Skip the job only while it is known red.** A temporary carve-out keyed on a comment or a list of failing jobs needs someone to remember to remove it; an event scope does not.
- **Fix the gesture instead.** It cannot be reproduced locally (the cell needs linux/x86_64 and this host is macOS/arm64), and its cause is not yet known — that is what #2057 is for. Waiting on it blocks every merge in the meantime, which is the cost this decision refuses.
- **Leave it red and merge by hand through the API.** The ship playbook sanctions a direct merge, so this works, but it leaves every PR's `ci` run red and trains the eye to ignore red — the failure the work log records behind #363, where sixteen failures hid behind one.

### Consequences

- A pull request's `ci` run can now conclude `success` with `cell-demo-web` skipped. `merge-approved.sh` reads that as green, provided `fast` and `full` both ran and passed, which it already checks.
- The demo still gates `main`: a red `cell-demo-web` on a push to `main` is reported and visible, and the nightly dispatches CI on `main`.
- DEC-2609 item 5 stays true of the selection itself; this decision amends only the events that act on it.
- No journal event: this changes no runtime state.

<a id="dec-2635"></a>

## DEC-2635: J1 lands atomically, under a PR-scoped marker

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79: a reversible process decision, and the mechanism [DEC-462](earlier-303-599.md#dec-462) already established for exactly this case) |
| **Date** | 2026-10-21 (UTC) |
| **Decided by** | an agent (`deepseek-ai/DeepSeek-V4.1-Flash-1017`, Cursor), acting as the coordinating session under DEC-79, at the founder's direction to act autonomously |
| **Story or claim** | J1; [PR #2022](https://github.com/kunwarshivam/mandate/pull/2022); [DEC-1175](DEC-1175.md); ES-22 (ADR-0001); [DEC-917](earlier-900-1999.md#dec-917) |

### Context

PR #2022 changes journal spec v0.70, its reference cases and generated fixtures, the Python
reference, and `crates/mandate-journal/src/deployment.rs` — the parser that reads those records.
ES-22 refuses a protected path beside code, so `spec-guard` has failed the PR since it opened.

Four attempts to make it land, and what each showed:

1. **Split into spec and code PRs.** Refused: I opened the spec half as #2063 and it failed CI on
   `mandate-journal::deployment_manager_records` —
   `valid ProcessStartIssued.token_exchange: Invalid { reason: NonCanonical, path: "payload.process" }`.
   The new spec admits `token_exchange`, and the old parser refuses it. The code half alone passes;
   the spec half alone cannot. **The pair is genuinely mutually red**, and #2063 is closed.
2. **The code half alone first.** It passes its own crate's tests, but it registers a process and a
   `cause` value that the *old* spec does not list, so it fails the `reference` and `refcases` gates
   that check the reference implementation against the recorded cases. It is not independently green
   either.
3. **[DEC-917](earlier-900-1999.md#dec-917) (PR [#2062](https://github.com/kunwarshivam/mandate/pull/2062)).** That
   decision makes a crate's `tests/` directory not-code. It does **not** help here: this PR's code is
   `src/deployment.rs`, which DEC-917 deliberately leaves as code. I tested it against this PR's diff
   and the guard still refuses, correctly.
4. **A per-PR exception.** This decision.

I also recorded, in #2022's closing comment, that its halves were cleanly separable. That was wrong,
and CI disproved it within the hour: my local checks had run the parser's suite against the old spec
and the spec-only gates against a tree with no parser change, so neither combination that actually
fails was exercised. The PR was reopened and #2063 closed with that finding written down.

### Decision

1. **`ATOMIC_EXCEPTIONS` is a finite list of `(pull request number, marker)` pairs.** The
   spec-guard reads it in place of its single hard-coded E7-7 case, which stays as its own entry
   (`e77_atomic_exception`, unchanged). One entry today: PR #2022 with the marker
   `ES-22-atomic-exception: J1 PR #2022 (DEC-2635)`.
2. **Both the number and the marker must match.** The pull request's number must be the entry's, and
   its description must carry the entry's marker exactly, on a line of its own. A marker copied into
   another pull request's description does nothing; the right number with no marker does nothing; the
   wrong number with the right marker does nothing. `an_atomic_exception_needs_both_its_number_and_its_marker`
   pins all four cases.
3. **The exception covers no gate but the split.** #2022 still needs green CI, an independent review
   and the coordinator's approval like any other change (DEC-79, DEC-175).
4. **Each entry is one PR and is deletable.** Deleting the entry restores the guard for that PR
   exactly as it was; nothing else in ES-22 changes.

### Rationale

DEC-462 already established this shape and confined it to one PR, for the same reason: a spec and the
runtime that reads it can be mutually red, and no ordering of two PRs is green. Reusing the
mechanism rather than inventing a broader one keeps the guard's default intact and makes every
exception visible in one constant with a DEC behind it.

The broader alternative — letting any protected change travel with `src` code when it also edits the
reference cases that exercise it — was refused. It reads plausibly but it is a standing loosening:
its test ("the cases exercise the code") is exactly what a careless author would assert about their
own change, and it would retire ES-22's split for the class of change the split most needs to catch.

### Alternatives considered

- **Land the spec first with the missing parser, then the parser.** Refused: the spec half is red, as
  #2063 showed.
- **Land the parser first, then the spec.** Refused: the parser half fails `reference` and `refcases`
  against the old cases.
- **Widen DEC-917 to `src` files.** Refused: it would make every spec+code pair atomic always, which
  is ES-22's whole prohibition.
- **Merge #2022 with `spec-guard` red, as #2055 did.** Refused: that is precisely how `main` came to
  carry a red gate for a day (DEC-2632). An exception that is written down and scoped is better than
  a failure that is ignored.

### Consequences

- #2022 can merge once its CI is green and its review passes.
- The next mutually-red pair needs its own DEC and its own entry, which is the intended cost: each
  exception is argued for, mapped to one PR, and removable.
- `main`'s `spec-guard` goes green again once #2062 (DEC-917) and this land, and #2055's red gate
  closes with them.
- No journal event: this changes no runtime state.

<a id="dec-2636"></a>

## DEC-2636: How the Alpaca OAuth client id, redirect URI and client secret reach staging

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79, approved by the coordinator on 2026-10-22, with lanes V2, GO and D1; it adds one secret on the path DEC-2630 already uses for the model provider's key, widens no identity's reach, and spends nothing) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lanes V2 (the vault CLI), GO (the apply) and D1 (the chart) |
| **Story or claim** | E10-13; [connections spec](../../specs/connections.md) §5.2; [DEC-692](earlier-600-899.md#dec-692) item 6; [DEC-2630](process.md#dec-2630); [DEC-2623](DEC-2623.md) item 2; [DEC-2628](process.md#dec-2628) item 1; [DEC-2014](infrastructure.md#dec-2014) |

### Context

The web connect needs three values from the Alpaca OAuth app the founder registers for paper:
- the client id and the redirect URI, which the API's start and the token exchange both send;
- the client secret, which only the token exchange may read.

Nothing on main carried any of them into staging: no chart value, no infra secret, no vault
import. `mandate vault import alpaca-oauth-client-secret` writes the local vault only
(DEC-692 item 6). Staging's vault is OpenBao, provisioned by the apply (DEC-2630).

### Decision

1. **The client secret is a GitHub environment secret, imported by the apply.**
   - It is `ALPACA_OAUTH_CLIENT_SECRET` in the `infra-staging` environment.
   - `infra.yml` hands it to `xtask infra` as `MANDATE_ALPACA_OAUTH_CLIENT_SECRET`, never as a
     `TF_VAR_`, so OpenTofu never reads it. It joins `INFRA_SECRETS`, the secret step's
     environment list and the step secrets every child starts without.
   - `infra` masks it the moment it is read.
   - `vault_provision` writes it on every apply, when set, through the secret-import identity. The
     identity's credentials and the secret go on the standard input of
     `mandate vault put alpaca-oauth-client-secret`. That variant writes `SecretPath::ClientSecret`,
     `platform/alpaca-oauth-client-secret`; lane V2 adds it to the CLI.
   - When the secret is not set, the apply prints `alpaca oauth client secret: not set` and leaves
     the vault as it is, deleting nothing. A failed write fails the apply. Rotation means
     replacing the GitHub secret and running the apply again.
2. **Only the token exchange reads it.** The vault policy already lets only a connection's
   token-exchange identity read the path, and only the secret-import identity write it. The secret
   is never a chart value, a ConfigMap, a Kubernetes Secret, a log line or any process's
   environment. The exchange reads it from the vault into a `secrecy` value for its one request
   (DEC-821 item 2).
3. **The client id and redirect URI are chart values.** Both are non-secret and in the workspace
   chart.
   - `alpacaOAuth.clientId` and `alpacaOAuth.redirectUri` each have a strict pattern and an empty
     default.
   - When set, they are rendered as `MANDATE_ALPACA_CLIENT_ID` and `MANDATE_ALPACA_REDIRECT_URI`
     into the API and the token-exchange processes only.
   - The redirect URI is each deployment's own, as DEC-2623 item 2 says. Staging's API host is
     `api-staging.owlhead.ai`, so staging registers and sends
     `https://api-staging.owlhead.ai/v1/oauth/alpaca/callback`. Production keeps
     `https://api.owlhead.ai/v1/oauth/alpaca/callback`.
   - Staging's overlay names the redirect URI now and the client id once the founder sends it.
     Until both are set, the API offers no connect.
4. **The founder's one instruction.**
   1. Register a paper OAuth app at Alpaca with the redirect URI
      `https://api-staging.owlhead.ai/v1/oauth/alpaca/callback`.
   2. Add its client secret as `ALPACA_OAUTH_CLIENT_SECRET` in the `infra-staging` environment.
   3. Send the client id, which an agent commits to staging's overlay.

### Rationale

The model provider's key already travels this exact path (DEC-2630): a GitHub secret held only by
the apply step, masked at once, written to the vault on stdin through the one identity allowed to
write it, and read only by the process that uses it. A second secret on the same path adds no new
mechanism to review. The client id and redirect URI are public by design, so they are settings,
not secrets, and only the two processes that send them get them.

### Alternatives considered

- **A Kubernetes Secret holding the client secret, mounted into the exchange.** It would put the
  secret in etcd beside the vault and give the chart a secret, which DEC-822 item 4 rules out.
- **`mandate vault import` for staging.** `import` writes the local vault's files (DEC-692
  item 6), which staging does not run.
- **The client id as a GitHub variable.** It is not secret, and a committed chart value is
  reviewable and versioned with the deployment.

<a id="dec-2637"></a>

## DEC-2637: CI's schema mutation sweep runs in ten shards

| | |
|---|---|
| **Status** | Superseded by [DEC-2649](DEC-2649.md) (its shard count only) |
| **Date** | 2026-10-24 (UTC) |
| **Decided by** | an agent, under DEC-79 and DEC-464 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)), at the coordinator's request |
| **Story or claim** | Main's push run 40075522259; supersedes [DEC-1004](earlier-900-1999.md#dec-1004) item 1's shard count only |

### Decision

1. The `schema-mutants` matrix runs ten shards, `0/10` to `9/10`, `fail-fast: false`, in place of
   DEC-1004 item 1's six. Everything else in DEC-688 and DEC-1004 stands: each shard runs the three
   checkers on the unmutated tree first, the required `full` verdict passes only when every shard
   succeeded, and `cargo xtask check` and the nightly run the sweep unsharded.
2. The xtask test that pins the job's lines pins the ten shards.

### Rationale

Main's push run 40075522259 was cancelled when `schema-mutants (5/6)` hit its ten-minute job
timeout: the step passed after 9m21s, but the job ran from 07:48:08 to 07:58:10. On #2350's run,
shards 0, 3, 4 and 5 took about 9.5 to 9.9 minutes, and shards 1 and 2 about 6. The sweep is now
1,369 mutants, where DEC-1004 sized six shards for about 912. Each shard takes an even 228 by count,
so the spread is runner speed, and the slow runners leave no margin. Ten shards hold about 137
mutants each, so the slowest runner lands near six minutes, with room for the next schemas.

### Alternatives considered

- **A larger timeout.** Rejected: DEC-464's bound is per job.
- **Fewer examples or mutants.** Rejected: DEC-464 forbids removing tests or safety gates to meet
  the bound.
- **Sharding by measured cost.** Rejected for now: the spread is runner speed, not mutant cost, so
  even counts with more shards fix it with no new machinery.

<a id="dec-2638"></a>

## DEC-2638: The nightly's reference checks run in shards, and its sweep shards hold five mutants

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 that adds parallel shards and removes no seed, mutant or check, as DEC-464 directs) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent, under DEC-79 and DEC-464 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)), at the coordinator's request |
| **Story or claim** | [#2353](https://github.com/kunwarshivam/mandate/issues/2353) (the nightly at a4986bbe); supersedes [DEC-1190](earlier-900-1999.md#dec-1190) item 5's eight-mutant shard size only |

### Context

The nightly at a4986bbe (run 40088324106) had 26 jobs cancelled at the ten-minute job timeout:

- **`reference`** was cancelled in `fuzz.py 9`. One job ran the mandate reference's ten fuzz
  seeds, at about 68 seconds each, then `mutants.py`, then gitleaks over the whole history.
  `mutants.py` alone is 214 seeded bugs, each run against a fuzz probe of about 40 seconds, so
  roughly two and a half hours. It could not have fit the job since the table grew past a dozen
  bugs.
- **25 `sweep` shards** were cancelled mid-mutant. The shards that passed averaged 6.3 minutes
  with eight mutants, and the slowest finished at 9m58s, so eight mutants leave no margin.

### Decision

1. **`reference/mandate/mutants.py --shard K/N`**, counted from 0, runs the bugs whose index in
   the combined table is K modulo N. N shards together run every bug once. Every shard first
   checks the same unmutated baselines itself, so a shard never scores a broken baseline as
   catches. Without `--shard`, every bug runs, as before.
2. **The nightly's `reference` job becomes two jobs.**
   - `reference-fuzz` is a matrix of four shards, `0/4` to `3/4`. Each runs the seeds `s` of
     `1..=10` with `(s - 1) % 4` its index, through `cargo xtask ci nightly-fuzz` and
     `MANDATE_FUZZ_SHARD`. That is three or two seeds a shard, about three and a half minutes.
   - `reference-mutants` is a matrix of forty `mutants.py` shards, `0/40` to `39/40`, through
     `cargo xtask ci nightly-mutants` and `MANDATE_REFERENCE_SHARD`. Each holds five or six
     bugs, about four minutes after the baselines.
   - `gitleaks-history` runs gitleaks over the whole history (`cargo xtask ci
     nightly-gitleaks`).
   - `cargo xtask ci nightly`, run unsharded, still runs all of it, for a local run.
3. **The whole-crate sweep's shards hold at most five mutants** (`SWEEP_SHARD_MUTANTS`), in
   place of DEC-1190 item 5's eight. At the slowest shard's rate, about 0.9 minutes a mutant,
   five mutants land near seven minutes with setup. Everything else in DEC-1190 item 5 stands,
   the rotation's rule included.

### Consequences

- With smaller shards the sweep has about 1.6 times as many, and DEC-1190 item 5's rotation
  lengthens on its own past one 256-job matrix a night. At today's 2,626 eight-mutant shards it
  is eleven days, and it becomes about seventeen. Each mutant is still tested once a rotation,
  and no mutant is dropped. A shorter rotation needs a second matrix a night, which DEC-1190
  item 5's Proposed rotation leaves to the founder.
- The nightly gains 44 jobs: four fuzz shards, forty `mutants.py` shards and one gitleaks job,
  in place of the one `reference` job. The forty run at most eight at a time, as the sweep does.

### Alternatives considered

- **Fewer seeds or bugs.** Rejected: DEC-464 forbids dropping tests to meet the bound.
- **A longer job timeout.** Rejected: DEC-464's bound is per job.
- **Sizing the sweep's shards by measured cost** (`ci mutants-costs`). Deferred: an even count
  with smaller shards needs no new machinery tonight.

<a id="dec-2639"></a>

## DEC-2639: The nextest run takes six `fast-test` shards

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 that adds parallel shards and removes no test, check, baseline or gate, as DEC-464 directs) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent, under DEC-79 and DEC-464 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)), at the coordinator's request |
| **Story or claim** | #2360's run 40216211783; supersedes [DEC-2325](trading-core.md#dec-2325) item 1's shard count only |

### Context

On #2360 (run 40216211783), a change to `xtask/` selected the whole workspace. The four `fast-test` jobs
took 9m01s, 5m51s, 6m33s and 6m07s:

| Shard | Tests | Test time | Build and setup | Job |
|---|---|---|---|---|
| 1/4 | 2,590 | 232 s | about 4m50s | 9m01s |
| 4/4 | 2,118 | 93 s | about 4m35s | 6m07s |

The partitions are counted, so they balance tests, not time, and `1/4` holds most of the slow
ones. Building the test profile from the shared cache is about half of each job, and does not
shrink with more shards. A runner a minute slower than this one would put `1/4` over DEC-464's
ten-minute cap, as `schema-mutants` went over on 2026-10-24 (DEC-2637).

### Decision

1. **Six shards.** `fast-test`'s matrix is `1/6` to `6/6`, nextest's `--partition=count:K/6`.
   Only `1/6` saves the shared cache. Everything else in DEC-2325 stands: the longest tests start
   first, from `xtask/nextest.toml`.
2. **How it is pinned.** `the_fast_jobs_run_exactly_the_fast_group` already requires the matrix to
   be `1/N` to `N/N`, each once. It now pins `save-if` on `1/6`.

### Rationale

Six shards cut the slowest shard's tests from about 232 seconds to about 155, which puts the job
near seven minutes, three below the cap. The build is repeated on two more runners. That is what
DEC-464 asks for in place of dropping tests.

### Alternatives considered

- **Balancing the shards by recorded test times.** That needs a timing file kept current, and nextest
  partitions by count or hash only. Deferred, as DEC-2325 deferred it.
- **Five shards.** That leaves the slowest shard near 7m40s. Six leaves a minute more for a slow
  runner and the next long tests.

<a id="dec-2640"></a>

## DEC-2640: Every lane opens its pull requests as drafts, and marks one ready only to land it

| | |
|---|---|
| **Status** | Accepted (agent: a reversible process decision under DEC-79, at the coordinator's direction on 2026-10-26; it removes no check, test or gate) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane DR), at the coordinator's direction |
| **Story or claim** | Main's `ci` runs of 2026-10-26; [DEC-610](earlier-600-899.md#dec-610) (a draft runs nothing), [DEC-2610](process.md#dec-2610) (`ci` on every push to `main`), DEC-2605 row 14 (a PR merges on its independent review's PASS, without waiting for CI) |

### Context

On 2026-10-26 the account's runner pool was saturated, and no `ci` run on `main` gave a verdict for
over an hour. The numbers:

- **The queue.** About 19 `ci` runs sat `queued` at once, nearly all on ready pull requests, each
  fanning out to 30 to 45 jobs, and none was in progress.
- **The held group.** `main`'s run 40284218304 (07:10) held `main`'s concurrency group. Every job
  but its last, `report-main`, had finished by 08:02, but `report-main` waited for a runner from
  08:13.
- **The replaced runs.** Each later push to `main` waited behind it as the group's one pending
  run, and the next push replaced it, so five runs in a row ended `cancelled` without starting a
  job.

Since DEC-2605 row 14, a pull request lands on its independent review's PASS without waiting for
CI. So a ready pull request's `ci` runs mostly consume runners that `main`'s verdict needs.

### Decision

1. **Drafts until landing.** Every lane opens its pull requests as drafts. DEC-610 already makes a
   draft run nothing in `ci`, so a draft's pushes take no runner.
2. **Ready only to land.** A lane marks a pull request ready only when it is about to land it, once
   its independent review has passed and its merge-tree against `main` is clean, or earlier only
   when it needs a check that runs on CI's runners alone (the Postgres tests, the mutation shards),
   as [07 item 6](../07-quality-and-release.md) already allows. Marking it ready starts one `ci`
   run on that head (`ready_for_review`). A session that squash-merges through the API merges on
   the review's PASS without waiting for it (DEC-2605 row 14), and `main`'s run judges the merge;
   the `merge` workflow's own path still merges only once that run's `fast` and `full` passed
   (`merge-approved.sh`, DEC-610 item 3). Either way the author has run the checks locally first,
   since a draft runs none.
3. **`main`'s verdict.** `ci` still runs on every push to `main` and on the nightly (DEC-2610), and
   `report-main` still opens an issue when one fails.
4. **`report-main` stays its own job.** Folding it into `fast` and `full` was considered and set
   aside: a job holding a token may not write the runner's files (`xtask`'s leak rule L8), and
   `fast` and `full` write the selection's reasons to the step summary (DEC-1190). This decision
   removes the fan-out that starved it.

This narrows [DEC-610](earlier-600-899.md#dec-610) item 4, which let a builder mark a pull request ready earlier
whenever it wanted CI's runners. No workflow file changes; `ci.yml` already skips drafts and runs
on `ready_for_review`.

### Consequences

- A pull request shows no CI result until it is marked ready. Its author runs the repository's
  checks locally before asking for review, as the ship playbook already requires.
- If the pool is still saturated after this, the next step is spending on runners (DEC-79), which
  goes to the founder.

<a id="dec-2641"></a>

## DEC-2641: The cell jobs stay within ten minutes: `cell-executor` also split by drill, one image import, and Chromium saved as soon as it is installed

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | An agent (Claude Code, lane RN), under DEC-79: a reversible CI scheduling decision that removes no drill, test or gate. Lane RN took the cell jobs' budget over from lane DR at the coordinator's request, after DR's #2521 split every drill job by variant |
| **Story or claim** | [DEC-464](earlier-303-599.md#dec-464)'s ten-minute budget; #2521 (DR); [DEC-2615](process.md#dec-2615), [DEC-2609](process.md#dec-2609), [DEC-2065](infrastructure.md#dec-2065) |

### Context

On `main`'s run 40281337291 (2026-10-26), `cell-executor` and `cell-exchange` were cancelled at
ten minutes. #2521 then ran each drill job once per variant (`MANDATE_CELL_VARIANT`). From that
run's logs:

| Phase | Seconds |
|---|---|
| Checkout, toolchain, cache, building xtask | about 60 |
| `cell up`: the static build 26, the images 11, `k3d cluster create` 12, **eight `k3d image import`s 30**, the operator and its plugins 45, the journal 60, the vault 22, the workspace 27 | about 236 |
| `cell-executor`, plain: ticks 53, executor 130 | about 183 |
| `cell-executor`, seeded: ticks 131, executor 130 or more | at least 261 |
| `cell-exchange`, plain: 73 and 93; seeded: 83 and 60 or more | about 166, and at least 143 |

The seeded executor and exchange-late drills were cancelled mid-run, so those figures are lower
bounds. So after #2521, `cell-executor`'s seeded shard is at least 60 + 236 + 261 = 557 seconds,
under a minute from the limit. `cell-exchange`'s slower shard is about 60 + 236 + 166 = 462
seconds, which stays split by variant only. `cell-demo-web` downloaded Chromium on every run: one `actions/cache` step
saves only after a green job, and that job had been failing, so its cache was never written.

### Decision

1. **`cell-executor` is also split by drill.** Its matrix is `variant` × `drill` (`ticks`,
   `executor`), four shards. `cargo xtask ci <job>` reads `MANDATE_CELL_DRILL`, which names one
   drill of the job's own list. Unset, the job runs every drill, as before; any other value is
   refused, so a shard never runs nothing. Its slowest shard is now about 60 + 236 + 131 = 427
   seconds, less whatever item 3 saves.
2. **No drill or variant is dropped.** `the_sharded_cell_jobs_run_every_drill_in_both_variants`
   holds each of the six drill jobs' matrices to exactly `plain` and `seeded`, and a matrix that
   lists drills to exactly the job's own list, each handed to xtask and each shard run to its end.
   Seeded bug: `cell-executor`'s matrix without `ticks` fails it.
3. **One image import.** `cell up` imports every image in one `k3d image import`, which starts
   k3d's tools container once instead of eight times, then imports each image the node still lacks
   one by one, with the existing retries. The eight imports took 30 seconds; the saving is at most
   that, and is to be measured on the first runs after this lands.
4. **Chromium is saved as soon as it is installed.** `cell-demo-web` restores with
   `actions/cache/restore` and, on a miss, saves with `actions/cache/save` right after the
   install, so a later failure no longer loses it. The same cache serves GitHub's runners and
   the self-hosted ones (DEC-2065).
5. **Not done, and why.**
   - **Prebuilt cell images.** The static build (26 seconds, cached) and the image builds
     (11 seconds) are a small part of `up`. A job that built them once for the others would add a
     serial step of about a minute before every cell job, costing more than it saves.
   - **`cell-demo`** ran past ten minutes on run 40281337291 because step 7 polled 238 seconds for
     runtimes that crash-looped. That was a fault in the demo, not a budget overrun, and #2518
     (lane DR) fixed it.

### Rationale

Splitting keeps every drill and variant and costs runner time, not coverage. The import batching
and the Chromium cache remove work every run repeats. The other four drill jobs took 6 to 8.5
minutes on run 40249338048 before #2521 halved their drill time; lane RN confirms all of them on
the first runs on `main` after this lands. Two more jobs per run are the price.

### Alternatives considered

- **Raise the limit above ten minutes.** DEC-464 forbids it.
- **Reorder `cell-executor`'s seeded drills into two balanced shards by hand.** Fragile: the next
  drill added would unbalance them. One drill per shard follows the list.
- **Share one cell between shards.** Shards run on different machines; a cell does not travel.

<a id="dec-2642"></a>

## DEC-2642: a merge that waits for no CI first passes `cargo xtask land-check`

| | |
|---|---|
| **Status** | Accepted (agent: a tightening of the landing rule under DEC-176, at the coordinator's direction on 2026-10-26; it removes no check, test or gate). Amended by [DEC-2644](process.md#dec-2644): the test parts run as jobs of their own, and a test that failed as root runs once more unprivileged. Amended by [DEC-2645](process.md#dec-2645): gitleaks over the diff runs as the job `secrets` |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 and DEC-176 (`claude-code`, lane DR), at the coordinator's direction |
| **Story or claim** | No story; process and CI. DEC-2605 row 14 (a PR merges on its independent review's PASS without waiting for CI) and [DEC-2640](process.md#dec-2640) item 2 (review passed and merge-tree clean before landing) |

### Decision

1. **The landing rule gains a land-check.** A change that merges without waiting for CI (DEC-2605
   row 14) lands only once all three hold, on the head the review saw:
   - its independent review passed;
   - its merge-tree against `main` is clean (DEC-2640 item 2);
   - `cargo xtask land-check` ends with `verdict=pass`.

   Row 14's answer stands: CI still does not gate the merge, and the `merge` workflow's own path
   is unchanged (DEC-610 item 3).
2. **What it runs.**
   - It fetches `origin/main` and merges it into the committed HEAD in a scratch worktree,
     `target/land-check/work`, leaving the caller's checkout untouched. A conflict fails the check
     before anything runs.
   - On the merged tree it runs `cargo xtask ci spec-guard`, `ci lint` and `ci test`, with
     `MANDATE_BASE_REF` set to the merged base. The tests are therefore the parts `ci select`
     scopes to the change (DEC-1190).
   - Each job runs even after an earlier one fails, so the summary names every result.
   - It builds into its own `target/land-check/target`, without incremental artifacts or
     debuginfo, so a change that selects every crate fits a lane's disk;
     `rm -rf target/land-check` gives the space back.
   - No cell and no mutants. Mutants on safety-critical crates remain their own rule (DEC-79).
   - Every test runs after a failure (`MANDATE_NO_FAIL_FAST`), so the check names them all.
   - One land-check at a time in a checkout: two would share `work` and `target`. A lane has one
     checkout.
3. **One summary line, and what fails it.** It ends with:
   `land-check summary: head=… base=…@… fetched=yes|no|skipped merged=… merge=clean|conflict|failed spec-guard=… lint=… test=… verdict=pass|fail`
   - It exits non-zero on anything but `verdict=pass`, and a lane reports from that line, as from
     the mutants summary (#2648).
   - A base that names a branch of `origin` but could not be fetched (`fetched=no`) fails the
     check: the base may be stale, which is what the check exists to rule out.
4. **What the caller supplies.** spec-guard looks for a DEC in the PR description and accepts the
   PR's own exceptions. Before land-check, export `MANDATE_PR_BODY` (the description) and
   `MANDATE_PR_NUMBER`, which it passes through. Without them a change that cites its DEC only in
   the description fails spec-guard, which is the safe direction.

### Rationale

On 2026-10-26 three changes landed with faults that the checks only CI runs would have refused.
Row 14 had removed the wait, so nothing ran those checks first:
- GL's spec-guard violations;
- NE's stale hash pins, which xtask's own tests pin;
- a duplicate OpenTofu definition.

land-check runs the cheap part of those checks on the exact tree the merge will produce, in
minutes. It adds a condition to the landing rule and removes none, so under DEC-176 an agent may
record it.

**What it does not cover.** The first two faults are in its jobs: spec-guard, and `ci test`'s
scoped tests. The OpenTofu checks (`ci tofu`) are in `full` only, as are the fixtures, the
reference implementation, the schemas, supply chain and Postgres. A change to those areas still
needs its author to run that `full` part locally before landing.

### Alternatives considered

- **Wait for CI again (DEC-175).** Rejected: row 14 is the founder's answer, and CI's queue was
  the reason for it.
- **Run `cargo xtask check` before every merge.** Rejected: it includes the cell and the mutation
  shards, which take far longer than a landing can wait.
- **Merge in the caller's own checkout.** Rejected: it would leave the lane's branch in a merge
  state when a check fails, and it would test the caller's uncommitted files rather than the head
  that lands.

<a id="dec-2643"></a>

## DEC-2643: The ES-22 split breaches landed on 2026-10-26 stay, recorded as retroactive exceptions

| | |
|---|---|
| **Status** | Accepted (the coordinator, 2026-10-26: record the breaches rather than revert them, the first alternative below refused) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code` (lane GL), at the coordinator's direction under DEC-79: a reversible process decision |
| **Story or claim** | ES-22 (ADR-0001); [DEC-2635](process.md#dec-2635); [DEC-2605](process.md#dec-2605) row 14; lane IAC's audit of `main` |

### Context

ES-22's spec guard (`cargo xtask ci spec-guard`) has two ES-22 rules, beside ES-11's status-flip check. First, a protected path (`docs/specs/`,
`schemas/`, `reference/`, `fixtures/refcases/`, `crates/mandate-refcases/status.toml`) never lands in the
same pull request as code (`crates/**` except `crates/*/tests/**`, and `xtask/src/**`), unless an entry
in `ATOMIC_EXCEPTIONS` names that pull request (DEC-462, DEC-2635). Second, a change to a protected path
cites a DEC in its description or commits.

The guard runs only in CI. Since DEC-2605 row 14 relaxed DEC-175, an approved pull request merges after
its independent review without waiting for `fast` or `full`. So a pull request could land with
`spec-guard` red, and nothing local stopped it. Lane GL found this on its own branch on 2026-10-26.
Lane IAC then audited every first-parent squash on `main` since 00:00Z, 268 of them, using the guard's
own classification. Lane GL re-ran the audit over 271 squashes and got the same list.

Every pull request below was reviewed and cites a DEC. Only the split was skipped: each landed with
`spec-guard` red while CI did not gate the merge. None can be split after landing. Reverting would remove reviewed behaviour, much of it safety
behaviour, and land it again in two halves for no change in substance.

### Decision

1. **The eleven split breaches stay on `main`, recorded here as retroactive exceptions.** Each row gives
   the squash, the pull request, the lane, the protected paths, then the code, and what shipped together.

   | Squash | PR | Lane | Protected → code | What shipped together |
   |---|---|---|---|---|
   | `51b5f9aa3` | #2515 | GL | `docs/specs`, `reference/journal`, `fixtures/refcases` → `mandate-journal`, `mandate-api-server` | Journal spec v0.91's real-money records and the API's gate (DEC-2134) |
   | `833ae24b8` | #2516 | L1 | `docs/specs` → `mandate-api`, `mandate-api-server` | E9-4 S5.3, a deployment under passkey step-up, and its server |
   | `bcea7ea87` | #2555 | EX | `docs/specs` → `mandate-daemon`, `-executor`, `-executord`, `-notify`, `-runtime`, `-shell` | X8 part 1, closed alert kinds, and the cores that emit them |
   | `30b6d7e10` | #2621 | MC | `docs/specs`, `reference/journal`, `fixtures/refcases` → `-executor`, `-gate-context`, `-journal` | `FillApplied` version 2 (DEC-2812, journal v0.95) and the crates that read it |
   | `49a85eaf0` | #2638 | EX | `docs/specs` → `mandate-api-server`, `xtask/src` | `GET /alerts` and its route |
   | `0fac06e3b` | #2642 | MC | `docs/specs`, `reference/journal`, `fixtures/refcases` → `-executor`, `-executord`, `-gate-inputs`, `-journal`, `-shell` | DEC-2817 (journal v0.98) and its parser and executor |
   | `4e2b33e8f` | #2651 | MC | `docs/specs`, `reference/journal`, `fixtures/refcases` → `mandate-journal` | DEC-2820 (journal v0.99) and its parser |
   | `1a8d556ad` | #2654 | GL | `docs/specs`, `schemas/workspace-api` → `mandate-api-server` | DEC-2518, the approval read's `schema_version`, and the route that serves it |
   | `cb6e547da` | #2669 | GL | `docs/specs`, `schemas/workspace-api` → `mandate-api-server` | DEC-2519, the decision's deferred checks on the approval read, and the route |
   | `863419ee4` | #2672 | EX | `docs/specs`, `schemas/workspace-api` → `-api-server`, `-journal`, `-notify`, `-webpush` | DEC-2520, the `connection_unavailable` alert kind, and its emitters |
   | `541f54467` | #2682 | GL | `docs/specs`, `schemas/workspace-api` → `mandate-api`, `mandate-api-server` | DEC-2521, the `deferral_mismatch` refusal, and the route |

   These are not entries in `ATOMIC_EXCEPTIONS`. That list admits a pull request before it merges, and
   these have merged. The guard compares a pull request with its base, so none of them affects a later
   pull request's run, and the list stays as it is.
2. **One squash breached the citation rule.** `49f290686`, #2631 (lane DR, E21-54), changed
   `schemas/workspace-api/examples/read-models.confirmation.json`, a protected path, and cited no DEC in
   its description or its squash. The rest of its diff is under `crates/*/tests`, so it kept the split.
   Its checklist ran `cargo xtask ci lint` but not `spec-guard`. The change is a fixture correction (SPY's
   Alpaca asset id in place of AAPL's) and stays. This entry is its record.
3. **The cause.** DEC-2605 row 14 made CI non-gating for merges. A guard that runs only in CI is therefore
   advisory unless the author runs it. DEC-175's review condition did not include it, and reviewers
   checked the code, not the split.
4. **The rule from now on, for every lane:** before landing any pull request, run
   `cargo xtask ci spec-guard` and `cargo xtask ci lint` locally on its head, and land only when both pass.
   This is in addition to the review and the other checks the lane runs.
   - A change whose protected and code halves are mutually red is split the DEC-2635 way, or gets its own
     `ATOMIC_EXCEPTIONS` entry before it merges.
   - A change that can be ordered is split: the spec first, with the crate left stricter than the spec,
     then the code. Lane GL's DEC-2522 (#2716 and #2717) is the first split in flight under this rule.
5. **Nothing else changes.** ES-22, `ATOMIC_EXCEPTIONS`, DEC-175 and DEC-2605 row 14 stand as they are.

### Rationale

The breaches are in the record either way. Recording them keeps the audit honest and costs nothing;
reverting and re-landing eleven reviewed changes would cost a day and change no behaviour. The rule
closes the gap at its cause: the check exists and is fast, and it was simply not run. A local run before
landing restores what CI gating used to give, without reversing DEC-2605 row 14.

### Alternatives considered

- **Revert each and re-land it split.** Refused: no behaviour changes, much of it is safety behaviour
  with reviews behind it, and the churn would invite new mistakes.
- **Add the eleven to `ATOMIC_EXCEPTIONS`.** Refused: that list admits pull requests before they merge.
  Adding merged ones would read as approval granted in advance, and it changes no check.
- **Restore CI gating for `spec-guard` alone.** Not decided here. It would reverse part of DEC-2605 row
  14, which is the founder's. The local rule achieves the same without it.

<a id="dec-2644"></a>

## DEC-2644: land-check runs a test that failed as root once more as an unprivileged user

| | |
|---|---|
| **Status** | Accepted (the coordinator's ruling of 2026-10-26, option 3 of three lane DR put; it changes DEC-2642's outcome for a test that fails as root, which now passes the check if it passes once as an unprivileged user, and keeps every test required to pass as some user) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | the coordinating agent, on lane DR's options; written by lane DR (`claude-code`) |
| **Story or claim** | Amends [DEC-2642](process.md#dec-2642) item 2. Lane DR's land-check of #2729: `mandate-marketdata`'s `a_temporary_file_that_cannot_be_created_is_an_error_not_a_retry` refuses to run as root, by design, and every cloud session runs as root |

### Decision

1. **Test parts are jobs of their own.** This replaces DEC-2642 item 2's `ci test`, and item 3's
   summary names each part. land-check runs `ci test-nextest`, `ci test-doc`,
   `ci test-rh-sim` and `ci test-python` as separate jobs in place of `ci test`. A nextest
   failure therefore never keeps the other parts from running, and the summary names each part.
2. **A test that failed as root runs once more, as uid 65534.** This happens only when land-check
   runs as root.
   - The candidates are only the tests nextest's report names as failed.
   - Each such test runs exactly once more, through the job that failed: `cargo xtask ci
     test-nextest`, with its packages, build and environment, narrowed to the one test by
     `MANDATE_NEXTEST_FILTER` (binary id and name) and run with `--no-tests=fail`. The host's
     target runner, `setpriv --reuid=65534 --regid=65534 --clear-groups`, runs the test's
     process. It travels as `MANDATE_NEXTEST_RUNNER`, which `ci test-nextest` sets on its
     `cargo nextest` alone, so `cargo xtask` and the build stay root's: a runner on `cargo xtask`
     would start xtask itself as that uid, which cannot reach the toolchain under `/root`. The
     test gets a fresh TMPDIR that the uid owns.
   - The report must be whole before anything runs again. Every status line's label is a plain
     one (`PASS`, `FAIL`, `SKIP`, `SLOW`, `START`, `RETRY`). The summary reads only passed, failed
     and skipped, and passed plus failed is the number run. Its failed count is the number of
     tests the `FAIL` lines name. An abort, a timeout, a leak, a signal, a cancelled run, a part
     it cannot read, or no summary, and nothing runs again.
   - It counts only if it passes there. A failure there, for any reason, the toolchain under a
     root-only home included, leaves the job red.
   - A test that passed as root never runs again.
   - Before any test runs again, land-check asks, as that uid (`setpriv … test -x`), whether it
     can reach the build directory, the merged tree and its TMPDIR, every directory above each
     included. If it cannot, or `setpriv` cannot start, nothing runs again, the job stays red, and
     the summary says why: `rerun_unavailable=path_not_traversable` or
     `rerun_unavailable=setpriv_unavailable`. An unreachable path reads as itself, never as the test
     failing a second time (lane NE's `identity_link` test, 2026-10-26, run from a checkout under
     a mode-700 directory). land-check never changes a permission to make a path reachable.
3. **Every such test is named.** The summary line carries `rerun_unprivileged=[binary:test,…]`,
   always present and empty when no test needed it, so a pass that rests on the unprivileged run
   is never silent. `rerun_unavailable=<reason>` appears only when the re-run could not start.
4. **A shallow clone is refused.** The checks read main's history: the gitops test that
   production pins only what staging ran, and the diff base. In a shallow clone they would fail for
   the clone, not the change, so land-check refuses one before anything runs, naming
   `git fetch --unshallow origin`. Lane DR met this on 2026-10-26 in a session cloned at depth.
5. **Nothing on the container changes.** No permission on the session's files moves. The uid is
   `nobody`'s, which the session images already have.

### Rationale

This changes DEC-2642's outcome. Under it, a test that failed as root made `test=fail`; now it can
end in `verdict=pass`. It is the coordinator's ruling, not an agent's tightening under DEC-176. It
loosens no test: each still has to pass, run as the user whose permissions it tests.

The test refuses to run as root because its subject is a file permission, which root overrides.
In a root session it can only fail, on `main` too, so every land-check that selects its crate
read `test=fail`. Running every test binary as an unprivileged user would also break every test
that starts `cargo`: the toolchain lives under `/root`, which is mode 700. Running only the tests
that failed as root once more, as the user they were written for, keeps every test required.

**What it does not do.** It does not catch a test that passes as root but would fail unprivileged.
CI, whose runners are not root, still runs those as written.

### Alternatives considered

- **Every test binary as the unprivileged user.** Rejected: the toolchain under `/root` is
  unreachable to it, so every test that runs `cargo` would fail for the environment.
- **Make the toolchain reachable (`chmod o+x /root`).** Rejected: it changes the session's
  filesystem as a side effect of a check.
- **Accept a named root-only failure in the landing rule.** Rejected by the coordinator: it
  loosens the rule (DEC-79).

<a id="dec-2645"></a>

## DEC-2645: land-check runs gitleaks over the diff

| | |
|---|---|
| **Status** | Accepted (agent: a tightening of the landing rule under DEC-176, at the coordinator's direction on 2026-10-26; it adds a check and removes none) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | lane DR (`claude-code`), at the coordinator's direction |
| **Story or claim** | Amends [DEC-2642](process.md#dec-2642) items 2 and 3. gitleaks turned `main`'s `full` red twice on 2026-10-26 (#2617, #2747), each caught only after the landing |

### Decision

1. **gitleaks is a part of its own, `ci secrets`.** It scans the commits since the diff base
   (`gitleaks git --log-opts=<base>..HEAD`), or the tree when there is no base, then runs the
   `.gitleaks.toml` exceptions' planted cases (DEC-89). `ci supply-chain` runs it after
   cargo-deny and the dependency registry, so CI's `full` runs exactly what it ran before.
2. **land-check runs it**, after the spec guard and before lint, on the merged tree against the
   base. Its summary line names it: `secrets=pass|fail|not-run`. It needs `gitleaks` installed,
   as `bash .cursor/install.sh` does; without it the job fails, and so does the verdict.

### Rationale

A merge that waits for no CI skips `full`, where gitleaks ran. Twice in one day a secret-shaped
fixture reached `main` and turned `full` red there. gitleaks over a PR's commits takes seconds, so
land-check can run it before every landing. It loosens nothing: every check that ran still runs,
and CI's `supply-chain` is unchanged in what it scans.

### Alternatives considered

- **All of `supply-chain` in land-check.** Rejected for now: cargo-deny fetches the advisory
  database, which a restricted session may not reach, and the dependency checks are not what
  reached `main` red.

<a id="dec-2646"></a>

## DEC-2646: CI's Python tests run in four shards

| | |
|---|---|
| **Status** | Accepted (agent: a reversible CI decision under DEC-464, at the coordinator's direction on 2026-10-27; it removes no test, raises no limit, and keeps the required `fast` verdict's rules) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | lane DR (`claude-code`), which owns the CI job budgets, at the coordinator's direction |
| **Story or claim** | CI. `main`'s `fast` failed on its last two runs (40455372969 on 3bdeda44, 40470281060 on fb4bb947): every job passed but `fast-python`, which the ten-minute job limit cancelled after 582 s in `cargo xtask ci test-python` |

### Decision

1. **`fast-python` is a matrix of four shards**, `1/4` to `4/4`, like `fast-test` (DEC-871). Each
   runs `cargo xtask ci test-python` with `MANDATE_TEST_PARTITION` set to its shard. Only the first
   saves the Rust cache. The `fast` verdict is unchanged: a matrix reads `success` only when every
   shard succeeded, and is still required where `select` selects it and skipped where it does not.
2. **The split is pytest's own collection.** With a partition, `ci test-python` checks `K/N` as the
   nextest run does and loads the plugin `mandate_tools.pytest_shard`. Every shard collects the
   same tests, sorts their node ids, and keeps every `N`-th from position `K-1`, so the shards
   together run every test exactly once. A shard that keeps no test fails (pytest's exit status 5).
3. **By test, not by file.** The eight slowest tests (about 70 s each, the journal vectors'
   generator runs) come two to a file, so a split by file would keep each pair together. Node ids
   are not passed on the command line: pytest prints `deploy/tests`' ids relative to their own
   directory, outside the rootdir, so they would not resolve.
4. **`cargo xtask ci test-python` with no partition**, as `cargo xtask check`, `ci test` and
   land-check run it, still runs every test in one process.

### Rationale

The suite grew past the limit; it does not hang. Local whole runs on 2026-10-26 and 27 took 697 s
(733 tests), then 742 to 821 s (740 tests), and a timed run took 898 s. Every run passed. DEC-464
says to add parallel shards rather than drop tests or raise the limit. Over the timed run's
per-test durations, four shards split by test come to 204, 199, 257 and 234 s locally, leaving room
to grow; three would have reached 332 s.

### Alternatives considered

- **pytest-xdist in one job.** Rejected: a new dependency, and it would still bound the whole
  suite by one runner's cores and one job's limit.
- **Split by file.** Rejected: the slowest tests pair up within files (item 3).
- **Raise `timeout-minutes`.** Rejected by DEC-464.

<a id="dec-2647"></a>

## DEC-2647: Commit-pinned gitleaks fingerprints for test fixtures that only the history holds

| | |
|---|---|
| **Status** | Accepted (agent: a reversible CI change under DEC-79, approved by the coordinator on 2026-10-28) |
| **Date** | 2026-10-28 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane GO) |
| **Story or claim** | No story; CI. Issue #2870, the nightly `gitleaks-history` failure (run 40692029055) |

### Context

The nightly scans the whole history with `cargo xtask ci nightly-gitleaks` (DEC-2638), every
branch and tag a full checkout fetches. On run 40692029055 it reported six findings over 5,067
commits. A full fetch of every branch and tag reproduces them, six over 5,213:

| Rule | File | Commit | Date |
|---|---|---|---|
| `generic-api-key` | `crates/mandate-api-server/tests/catalogue_http.rs` | `7844e43b` | 2026-10-26 |
| `generic-api-key` | `web/src/api/client.test.ts` | `761e79b1` | 2026-10-26 |
| `alpaca-key-id` | `crates/mandate-executord/tests/paper_credential.rs` | `5b5dc187` | 2026-10-19 |
| `alpaca-key-id` | `crates/mandate-alpaca/tests/vault_secret.rs` | `4da6a99a` | 2026-10-19 |
| `generic-api-key` | `xtask/src/release.rs` (two lines) | `1c96b028` | 2026-10-19 |

Each is a test fixture, classified without printing the value:
- a JWT built for a client-credential case, with an ES256 header and only `sub` and `scope` claims,
  no issuer, audience or expiry;
- an example `webauthn_challenge` of 32 bytes (a WebAuthn challenge is public by design);
- two key-ID-shaped sentinels that carry a marker word, which the vault tests check never reach
  output;
- a 16-character stand-in for `RELEASE_SIGNING_KEY` inside `#[cfg(test)]`, handed to a stand-in
  cosign (a real signing key is a cosign PEM), and the test's assertion that stderr never shows it.

None of the six values is in `main`'s tree any more: each file changed after the commit that held
it, so only a scan of the history still reports them.

### Decision

1. **History only, by fingerprint.** `.gitleaksignore` names each finding as
   `<40-hex commit>:<path>:<rule>:<line>`. Gitleaks then skips that one rule on that one line of
   that file in that commit, and nothing else. The same value written again in the same file, a
   real key anywhere, and any other commit are still reported. No fixture text is copied into the
   repository, and `.gitleaks.toml` gains no entry.
2. **A commit-less fingerprint is refused.** Gitleaks also accepts `<path>:<rule>:<line>`, which
   skips that line of that file in every commit, so a key later written there would pass.
   `cargo xtask ci supply-chain` (its `secrets` part) refuses any line of `.gitleaksignore` that is
   not a comment, blank, or a commit-pinned fingerprint;
   `gitleaks_fingerprints_must_pin_a_commit` pins the format and checks the committed file.
3. **A planted repository proves the narrowness.** The same job builds a two-commit repository
   whose first commit adds a key-ID-shaped line and whose second adds another to the same file, and
   scans it with `.gitleaks.toml`. Without a fingerprint both are reported, so the case is not
   vacuous. With the first commit's fingerprint, only the second commit's line is reported.
4. **Values in today's tree keep the DEC-2603 rule:** an exact-value, file-bound entry in
   `.gitleaks.toml` with its planted cases. A fingerprint is only for a value the tree no longer
   holds.

### Rationale

A path-and-regex entry allows its value in its file at every commit, past and future. A fixture
that exists only in history needs less: one finding, in the commit that holds it. The fingerprint
also records where the finding was, without repeating the value it matched.

### Alternatives considered

- **Exact-value, file-bound `.gitleaks.toml` entries, as DEC-2603.** Declined for history-only
  values: wider than needed, since each would allow the value in that file in every later commit,
  and it copies the fixture text into the repository again.
- **A gitleaks baseline report (`--baseline-path`).** It holds the full findings, matched text
  included, so it would commit the values.
- **Rewriting history to drop the values.** It rewrites shared history, which is never done here,
  for values that are not credentials.
- **Scanning only `main`'s history in the nightly.** It would stop scanning every other branch, so
  a real key pushed to an unmerged branch would no longer be reported.

<a id="dec-2648"></a>

## DEC-2648: `cell-demo-web` may run fifteen minutes, the one exception to DEC-464's ten

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79: a reversible CI change that weakens no safety rule and no `AGENTS.md` non-negotiable; it lengthens one non-required, main-only job's cap and changes nothing it checks) |
| **Date** | 2026-10-28 (UTC) |
| **Decided by** | Lane DR (Claude Code), number and scope from the coordinator |
| **Story or claim** | E21-54 (the demo end to end); amends [DEC-464](earlier-303-599.md#dec-464) for one job; reads against [DEC-2609](process.md#dec-2609) item 5 and [DEC-2634](process.md#dec-2634) |

### Context

DEC-464 caps every required check and aggregated job at ten minutes. `cell-demo-web` is neither: it
runs only on `main` and on a dispatch (DEC-2634), and no required verdict needs it. Its cap was
ten minutes all the same.

On 2026-10-28 a passing run (40692056048) took 8.5 of them: about five to bring the cell up, and
the scenario with the trade loop reading `pending`, since outside the regular session the runtime
proposes nothing. In the regular session the whole loop runs instead: the approval, the host's
confirmation, the fill, the agent's screen, the positions screen, the owner's exit and its fill,
the audit, the kill switch and the alert. That adds one to three minutes, and its bound is 400 s.
A job GitHub kills at its cap names no step and skips the `if: failure()` upload of the trade
loop's log and result, the only evidence the regular session's few runs give.

### Decision

1. **`cell-demo-web`'s `timeout-minutes` is 15.** No other job's cap changes.
2. **Every required check and aggregated job keeps DEC-464's ten minutes**, and so does every
   other job in `ci.yml`. A test in `xtask` reads each job's cap and allows more than ten only to
   the jobs it names, with the cap it names: `cell-demo-web`, at 15. A job added past ten fails it
   until a decision adds it there.
3. **Revisit once the loop runs green.** When `cell-demo-web` passes in the regular session within
   ten minutes, its cap goes back to ten and the exception is removed.

### Not covered here

- Shortening the cell's start, which would let the job fit ten minutes again.
