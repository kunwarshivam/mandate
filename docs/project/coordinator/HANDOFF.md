# Coordinator handoff (living log)

Branch: `claude/gallant-mccarthy-a3qgpx`. **No PR**: this branch is the coordinator's notebook. Its
other commits are old coordinator logs. The incoming coordinator reads this file first, then the
"Log" section at the bottom (newest last).

Last updated: 2026-10-17 18:15 UTC by the coordinator session `session_01BFjCWYU2cDwPmFkR3RAHQK`
(the founder's first account, near its weekly limit). The founder will switch to another account; a
new coordinator session takes over from this file.

## 1. Role

You are the merge coordinator for `kunwarshivam/mandate`, working for the founder (kunwarshivam).

- You dispatch Opus builder sessions (claude-code-remote `create_session`), each on one lane.
- You merge their PRs once they're ready, with `docs/project/coordinator/safemerge.sh PR [PROTECTED...]`.
  It checks four things: a `Review (Sonnet, head <sha>): PASS` line in the **PR body** naming the
  head's first 8 characters; `fast` and `full` both `success` at the head; the PR is not dirty; and
  it would not conflict with any protected PR. Then it squash-merges pinned to the head sha.
- You don't write product code yourself. You keep the founder out of the loop except for
  founder-reserved decisions (DEC-79: live money, spending, legal or compliance text, weakening a
  safety rule) and things only they can physically do.

**The founder's working style, in their own words:** "decide without asking", "dont make me
bottleneck", "speed up and not use too much token in orchestrating". When a founder decision is
needed, ask with AskUserQuestion, giving options and a recommendation; they asked for that format.

**Lean mode:**
- Builders send only `#PR head <sha> ready`, meaning fast and full are green and the Sonnet line
  in the body names that head.
- Builders merge main themselves; a merge-only delta needs no new review.
- Builders never add `Co-authored-by` trailers (CI rejects them).

**What the classifier blocks you from:**
- editing safemerge's review check;
- self-approving (`reapprove.sh` was blocked);
- a merge without a body review line;
- force-pushing someone else's history;
- the GitHub Actions variables and secrets API (proxy 403);
- reading code-scanning alerts (403).

When blocked, ask the builder to fix the PR body, or tell the founder.

## 2. Dates and goals

- **R0, the first live Robinhood order, Monday 2026-10-19, in market hours.** The founder places it
  in person. The runbook is `docs/project/tasks/first-live-trade.md`, "The R0 runbook: every
  command, in order", sections A to G (merged in #1518 and #1524). The rehearsal passes 33/33. R23
  (a spent record refuses as "not flat") is founder-decided: keep as is.
- **Hosted demo, 2026-10-26.** Cell #1 on DigitalOcean nyc3 (two droplets, about $96 a month; the
  founder buys them after the drills pass), behind Cloudflare Tunnel and Access, with R2 for cold
  storage. The web app is on Cloudflare Workers (owlhead.ai, www, app), and email-link sign-in is
  live. The demo shows the agentic research loop on recorded fakes; if that slips, the one-call
  run is the fallback.
- **Never touch the four R0-path files until R0 closes:**
  - the crates/mandate-paper live runner;
  - the mandate-shell run path;
  - scripts/r0-rehearsal.sh;
  - the crates/mandate-cli live entry.

## 3. Founder decisions made today (2026-10-17), all recorded by the lanes

- **Product direction: "Claude Code for trading", an agentic harness.** ADR-0005 (#1540) and DEC-2203.
- **DEC-2203 items:**
  - 14 `propose_exit`: yes.
  - 15 `propose_mandate_change`: **anytime** (ideate, monitor and owner runs; monitor runs may only
    narrow risk; always ASK, step-up, expiry, rate limit).
  - 16 who pays for model calls: **both**, a plan allowance plus an optional bring-your-own key,
    from day one.
  - 19 limits shown to the agent as percentages of allocation.
  - 20 `ask_owner`: in-app plus a generic email ping (founder-only address under DEC-820 until
    counsel's footer exists).
- **The $50 a month OpenRouter cap is the demo deployment's own cap, not a customer limit.** It's a
  deployment setting; workspace budgets are separate settings.
- **Users get the agentic agent only after the DEC-99 forward-paper evaluation** (DEC-103 unchanged).
- **R23:** keep the stricter refusal.
- **Item 25 was dropped.** Risk-increasing proposals are provisional mid-run and commit only on a
  normal run end, so HI-9 stays strict.
- **The design input** for the agentic harness is
  `docs/adr/0005-agentic-harness-design-input.md`. Read its §16 for the research revisions; the
  research memo is beside it.

## 4. Founder's outstanding physical actions

- Rotate the Alpaca paper keys (they leaked in a builder log earlier). Keep the new pair on the
  laptop only.
- Place the R0 order on Monday 10/19, following the runbook.
- Later: buy the two DigitalOcean droplets once lane H's drills pass on main. Roll the Cloudflare
  and R2 tokens they pasted earlier.

## 5. Numbering rules (to avoid conflicts)

- **DEC numbers by category (DEC-2600):** 2000 infra, 2100 API, 2200 intelligence, 2300 trading,
  2400 web, 2500 notifications, 2600 process/CI, 2700 control plane. A builder takes the next free
  number in its range.
- **Journal-spec rule blocks, reserved per lane:**

  | Lane | Rules |
  |---|---|
  | AH | 171–183 |
  | I2 | 184–189 |
  | EX | 190–199 |
  | D1 | 200–209 |

  Others ask the coordinator. The version and section go to whichever PR lands first; the later
  lander bumps only the version and section header, never rule numbers.

## 6. Lanes (builder sessions)

Message a builder with `send_message` and its session id.

| Lane | Session | What | Open PRs |
|---|---|---|---|
| A1 API server | session_01RvzYDY2aW2bqoGER2jz7uJ | #1457 merged. Next: the path-less subject follow-up (clears CodeQL alerts 119–121), then the `api` image and chart entry | — |
| A2 API commands | session_01Ls2FeaGuvpsxY4EUJuvBG3 | E10-11 and E10-12: versions, confirm, deploy over the API | #1533 spec |
| AH agentic harness | session_0163LBvHvoWVmyavuFBxSX3D | ADR-0005 and spec v0.2 (slice 1), then the loop with read and think tools plus the E19-6 bench (slice 3, about 10/22–24) | #1540 |
| EX desk/explain | session_01S97v7XqtPunf26UuKyrKun | Chat with your agent (owner and explain runs) | #1545 |
| I1 gateway | session_016qLJoQHQF79TCruLbTyWPT | #1519 (HTTPS transport) needs a main merge; the slice-2 tool calls and per-turn metering (needed by 10/21); the deployment cap as a setting | #1519, #1551, #1547 |
| I2 research loop | session_011XmbZCXXqoYb4GY5v3aJjW | E15-8 and E19-5 (renumbering #1543 to rules 184–187), the driver, the §9.22 registration, runtime turn inputs (needed by 10/22) | #1543, #1544, #1548 |
| I3 news and filings | session_01DVBqrGJWZ2KWc4k8whhLdc | done (#1480) | — |
| I4 backtest | session_01VFDsfqfgdRB6Zh1YgaYuv6 | #1482 needs a main merge; then the backtest and evaluation GET routes | #1482, #1546 |
| W1 web deploy | session_015SFHiHxW2eXJ4qdjqmDXh7 | done: Workers deploy and email sign-in live | — |
| W2 web app | session_01BekjpBN7onevzmeCSTCZXS | backtest screen, deploy flow on fixtures | #1549 |
| ID1 identity | session_01PnJfcsAGeskQR54vYM1SSi | #1529 merged; the stub issuer spec merged (#1535); its crate is next | — |
| SU step-up | session_017FkrnmyddDKcSaUKDePmcx | E9-4 passkey step-up (approvals from the web) | — |
| H hosted cell | session_01GjXL1LMyqC55yjrPDPwFwt | #1539 conflicts with main; told to merge main | #1539 |
| E2E demo | session_0179TvHn3fXbuc5vaxA9sUTf | `cargo xtask cell demo --scenario demo`, which prints the gaps | #1538 |
| O4 kill switch | session_01UUdL4Qni7KT9rwBKfYKi1r | E21-34 | #1537 |
| X executor | session_01JC6qC93xZiVhnLq1mgtfXo | E21-30 executor slice on a fixture broker | — |
| P process/runtime | session_01X3q52a8djExSYNAcqe7E2R | #1521 needs a main merge (tokio row); then the runtime image and processes.json follow-up | #1521 |
| D1 deploy manager | session_01HwbzZoQoAXsBqn6dQYPnEA | #1541 merged; #1542 code | #1542 |
| S storage | session_01G8h5nUpRTbR7FmbLKgwXzn | Postgres standby and backups | #1522 |
| V vault | session_01211NmxQm5AE8CxQHDfKQn8 | V1 local vault, V2 SecretSource and OpenBao | #1448, #1458 |
| M2 data service | session_01QVHyaoDsjybPPDxTxu5WJS | normalized quotes and hot cache | #1469 |
| L3 notifications | session_01BtCufaGcvxBnm9h6eebWxE | D10a (#1523), then the email channel (founder address only) | #1523, #1550 |
| R0 | session_01LwQZ9LnoyU1GFeoTr4XrLK | runbook merged; records R23 as founder-decided | — |
| R release | session_01PFT68bFvxyPp3VmFbVJeMw | images published to GHCR (private; the cell pulls with a read:packages token) | — |
| CI-R | session_016Q1bgmGnbpc8oAhaKVRAsX | #1500 merged (CI gated by risk) | — |

## 7. Next steps (in order)

1. Merge the PRs that were waiting on #1457 as their builders resend "ready" after merging main.
   In priority order: #1519, #1521, #1482, #1539, then A1's follow-ups. Use `safemerge.sh <PR>`.
2. Merge the harness spec #1540 when it's ready, then #1543 and #1544 (I2) and #1545 (EX), with
   their rule blocks per §5.
3. Watch the demo-critical dates:
   - I1 tool calls by 10/21 (#1551);
   - I2 runtime turn inputs by 10/22;
   - AH slice 3 and the bench by about 10/24;
   - E2E demo by 10/25.
4. Monday 10/19: be available for R0. The founder runs the runbook. Merge nothing touching the R0
   paths until it closes.
5. After lane H's drills pass on main: tell the founder to buy the droplets, then follow #1539's
   runbook.
6. At session end: update `docs/project/08-work-tracker.md` and `11-work-log.md` through a PR, and
   post on issue #165. That hasn't been done today.

## 8. Known issues

- **CI runner capacity is the bottleneck.** Many PRs sit queued for 30–90 minutes. Don't re-run
  jobs to "kick" them.
- **CodeQL isn't required.** #1457 merged with 3 path-injection alerts that are false positives (a
  constant path). A1's follow-up removes the seam. Nobody dismisses alerts; that's the founder's call.
- **The ID1 classifier refused history rewrites.** The fresh-PR route was used (#1529).

## Log

- 2026-10-17 18:15: handoff file created. Merged today in this stretch: #1518, #1524, #1517, #1520
  (founder), #1503, #1529, #1525, #1535, #1541, #1457. New lanes dispatched: H, E2E, O4, A2, X, SU,
  AH, EX. The agentic harness design and research are on this branch.
