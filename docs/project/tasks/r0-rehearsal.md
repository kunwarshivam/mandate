# R0 rehearsal against `mandate-rh-sim`: what passes on `main`, and what refuses where

A status record for R0 of the [first live trade brief](first-live-trade.md) (steps 1 and 2: the
build, then the rehearsal against the simulator, DEC-124). An agent ran every founder command in
order against `mandate-rh-sim` on loopback and a private Postgres journal, never a broker
([DEC-922](../decisions/DEC-922.md): the `rh-sim` build is the only one that dials loopback, and
it refuses any endpoint that is not plain `http` on `127.x`, [DEC-937](../decisions/DEC-937.md)
item 2). It used no credential and placed no order (`AGENTS.md` rules 7 and 8). Claim
[#1371](https://github.com/kunwarshivam/mandate/issues/1371).

**Result with R1b fixed (`claude/r0-live-workspace`, [DEC-952](../decisions/DEC-952.md)):** the
workspace is opened live (`workspace open --environment live`) and every owner command states
`--environment live`, so the whole configuration and gesture sequence passes against the
simulator: the registrations, the connect check (2d2), `connection record` (2e, 2f), `version
create` (2g), `version confirm` (2h, 2i) and `agent deploy` (2j, 2k). The rehearsal now stops at
the first run, 2l: the live door's lapse refuses `lapse_evidence_not_supplied` because the run
hands it no account snapshot ([R2](#the-blocker-chain-beyond-step-2l)). The placing run (2m) and
the restart (2s) stop there too, so no approval is asked (2n) and the agent stream stays empty
(2q, 2r). Behind R2, R3 to R5 still stand, and the grant (item 9) and the restart's `Unknown`
adoption (item 10) are being built in their lanes.

**Result with R1 fixed** (`claude/r0-connection-checked`, [DEC-946](../decisions/DEC-946.md),
#1382): the connect check and `connection record`'s show passed; the rehearsal stopped at 2f, the
journal refusing the live `ConnectionEstablished` on a control stream `workspace open` had opened
in `paper` (`environment_mismatch`, R1b).

**Result on `main` at `bf150da`** (before R1): the rehearsal stopped at `connection record`,
which refused `check_missing` because no command wrote the `ConnectionChecked` it requires.

**Re-run on #1390's branch (`claude/r0-live-run-wiring` at `7980209`, R1 and R1b merged, R2 to
R4 fixed):** 1a to 2k pass. 2l, the dry run, logs in on the simulator's page, reads the agentic
account's snapshot, passes the lapse and the shell's full fold (a live deployment refused
admission would have stopped at `DeploymentRefusal::Live`), passes the configuration, and stops at
`the tracer stopped at Validate: a paper mandate` ([R9](#the-blocker-chain-beyond-step-2e)): the
shell's artifacts accept a paper mandate only. 2m and 2s stop there too, so nothing is sent; 2n
and the agent stream's export find nothing, and the control and account streams export and
verify. The runner's step needs `MANDATE_RH_ACCOUNT_NUMBER`, which the script sets.

**Re-run on `claude/r0-live-artifacts` (R9 fixed, [DEC-957](../decisions/DEC-957.md), claim
[#1398](https://github.com/kunwarshivam/mandate/issues/1398)):** 1a to 2k pass as before. 2l
passes R9: the live version, confirmed on all four conditions, binds the model, fee schedule,
calendar, rule set and instrument snapshot the live workspace `ws_r0` registered on its own
`ctl:ws_r0`, and stops at `the tracer stopped at Validate: a fee schedule for the mandate's
environment` ([R10](#the-blocker-chain-beyond-step-2e)): the fee schedule the script registers
names `paper`. 2m and 2s stop there too, so nothing is sent; 2n and the agent stream's export
find nothing; the control and account streams export and verify.

**Re-run on `claude/r0-live-fee-schedule` (R10 fixed, [DEC-958](../decisions/DEC-958.md), claim
[#1405](https://github.com/kunwarshivam/mandate/issues/1405)):** 1a to 2k pass, with 2c's fee
schedule now the live Robinhood one (`config/fees/robinhood-live-2026-10-16.json`). 2l passes R10:
the artifacts are built and priced by Robinhood's posted fees. It then logs in on the simulator's
page, passes the lapse on the account snapshot, and stops at `the tracer stopped at Reconcile: the
agentic account's snapshot` ([R11](#the-blocker-chain-beyond-step-2l)). A diagnostic build, not
committed, showed what happens: the lapse's `get_portfolio`, `get_equity_positions` and
`get_equity_orders` answer, and then the preflight's own `get_portfolio` is refused by the MCP
client with `Throttled`, so `RobinhoodConnector::preflight_facts` reads it as `Unreadable
{ code: "portfolio" }`. 2m and 2s stop there too, so nothing is sent; 2n and the agent stream's
export find nothing; the control and account streams export and verify.

**Re-run on `claude/r0-mcp-read-budget` (R11 fixed, [DEC-959](../decisions/DEC-959.md), claim
[#1408](https://github.com/kunwarshivam/mandate/issues/1408)), without any Alpaca key in the
environment:** 1a to 2k pass. 2l logs in, reads the snapshot, passes the lapse and the fold, and
stops at `the tracer stopped at Reconcile: the Alpaca market reads a Robinhood preflight takes
(DEC-933)` ([R5](#the-blocker-chain-beyond-step-2l), `MARKET_UNREAD`), which `Linked::preflight`
checks before any Robinhood read. A diagnostic build, not committed, that runs the Robinhood
reads first and then stops showed them all answering: the preflight stands on the lapse's
snapshot and reads only the tradability and the quote, the last two waiting about a second each
for their token, and nothing is refused `Throttled`. 2m and 2s stop at `MARKET_UNREAD` too, so
nothing is sent; 2n and the agent stream's export find nothing; the control and account streams
export and verify. The next blocker on an agent's machine is R5; on the founder's, with the data
keys, the run reaches the Alpaca market reads and then the chain's R6 to R8.

**Re-run on `claude/r0-sim-market-data` (R5 fixed for the rehearsal, [DEC-961](../decisions/DEC-961.md),
claim [#1414](https://github.com/kunwarshivam/mandate/issues/1414)), with no Alpaca key in the
environment, on Saturday 2026-10-17 at about 01:10 UTC:** 1a to 2k pass, with the new 1c (the
daily-bars writer, an example) and 2a0 (the fixture: `market.json` and the stored daily bars). 2l
passes R5: the `rh-sim` runner reads the asset record, the IEX quote and the minute bars from the
fixture (`MANDATE_RH_SIM_MARKET`) and the daily closes from `--bars`, the Robinhood reads answer as
on DEC-959's run, and the shell's judge accepts the fixture's facts (the asset record bound to the
instrument, a current, uncrossed IEX quote) and then stops at `the tracer stopped at Validate: a
run clock inside the regular session` ([R12](#the-blocker-chain-beyond-step-2l)): the run was made
on a Saturday, and an opening is only judged inside the regular session (rule 12, trading spec
§9.6). This refusal is correct and needs no code. 2m and 2s stop there too, so nothing is sent, no
approval is asked (2n to 2p) and the agent stream stays empty (2q, 2r agent); the control and
account streams export and verify. The rehearsal stops here; the `ask` opening, the grant with
`cli_confirm`, the fill and the refused second run need a run between 09:30 and 15:50 New York
time on a trading day.

**Re-run on `claude/r0-sim-clock` (R12 passed on the rehearsal's clock, [DEC-962](../decisions/DEC-962.md),
claim [#1416](https://github.com/kunwarshivam/mandate/issues/1416)), with no Alpaca key in the
environment, on Saturday 2026-10-17, with `--sim-clock 2026-10-19T15:00:00Z` (Monday, 11:00 New
York time):** every runner and CLI process ran on one clock starting at that instant, and the
simulator stamped its orders on it. 1a (the CLI now with `--features mandate-cli/rh-sim`), 1b, 1c,
2a0 and 2a to 2k pass. 2l passes R12: the judge reads the run clock inside the registered
calendar's regular session and outside the close window, and the run stops one check later, at
`the tracer stopped at Validate: a mandate with no validation or policy violations`
([R13](#the-blocker-chain-beyond-step-2l)): the shell's run context tells the mandate validator
the connection is `paper`, so a `live` mandate breaks V-001. A diagnostic build, not committed,
printed the rejection: `violations: {V001}, warnings: {W002}`, no policy violation. 2m and 2s stop
there too, so nothing is sent, no approval is asked (2n; 2o and 2p not run) and the agent stream
stays empty (2q, 2r agent); the control and account streams export and verify. The same binaries
without `--sim-clock` still stop at R12, so the override is what moves the run past it. **The
rehearsal stops here, at R13**: the `ask` opening, the grant with `cli_confirm`, the fill from the
simulator and the refused second run were not reached.

**Re-run on `claude/r0-context-environment` (R13 fixed, [DEC-965](../decisions/DEC-965.md), claim
[#1421](https://github.com/kunwarshivam/mandate/issues/1421)), with no Alpaca key in the
environment, on Saturday 2026-10-17, with `--sim-clock 2026-10-19T15:00:00Z`:** the run context,
the cycle's admission and every draft's envelope now take the environment of the connection the
live door chose, never the constant `paper`. 1a to 2k pass as before. 2l passes V-001 and the
admission and exits 0, but prints nothing, so it fails the script's `would place` check
([R15](#the-blocker-chain-beyond-step-2l)): the model's one output is from Friday's close and
expires one `max_output_age_s` (86400 s) later, on Saturday, so on Monday's clock the builder
proposes nothing and the dry run decides nothing. The placing run 2m stops at `the runtime
refused its input: ConnectionChecked is not interpreted until the story that writes it`
([R14](#the-blocker-chain-beyond-step-2l)): the cycle replays the account stream, whose second
record is the connect check's `ConnectionChecked` (2d2), into the runtime, which refuses an
event type it does not interpret. So no approval is asked (2n; 2o and 2p not run), nothing is
sent, and the fill is not reached. The shell's agent stream now exists, opened `live` (2q and 2r
agent pass, `seq 1 to 1`), and the control and account streams export and verify. 2s stops at
R14 too, so the refused second run is refused before any place call, but by R14, not by the
spent record. **The rehearsal stops here, at R14 and R15.** An extra run with `--sim-clock
2026-10-20T15:00:00Z` (a Tuesday, so Monday's close is fresh) moved 2l one step: it stopped at
`the tracer stopped at Classify: the proposal classified ask, and the tracer acts only on auto`,
the dry run's answer to an `ask` opening (`crates/mandate-shell/src/tracer.rs:428`: a run without
`--place-one-order` never asks), which the script's `would place` expectation does not allow
for; 2m and 2s stopped at R14 as on Monday.

**Re-run on `claude/r0-runtime-connection-checked` (R14 fixed and R15's expectation corrected,
[DEC-969](../decisions/DEC-969.md), claim [#1428](https://github.com/kunwarshivam/mandate/issues/1428)),
with no Alpaca key in the environment, on Saturday 2026-10-17, first with `--sim-clock
2026-10-19T15:00:00Z` (Monday) as dispatched, then with `--sim-clock 2026-10-20T15:00:00Z`
(Tuesday):** the runtime now folds `ConnectionChecked`: the latest check of the deployment's
connection refuses an opening while it failed or is a day old, and never an exit (rule 13); the
executor folds it as a record. **On Monday**, 1a to 2k pass; 2m passes R14 (the account stream's
`ConnectionChecked` replays, and the run reads, reconciles and journals the model output, agent
stream `seq 1 to 5`, account stream `seq 1 to 6`) and exits 0 having decided nothing, and so do
2l and 2s: Friday's output is stale on Monday by the age rule, which no script change can meet
without relaxing it, since the schema caps `max_output_age_s` at one day (DEC-969 item 7). So 2l
(now expecting the ask), 2m and 2s fail by design, 2n finds no approval, and the three streams
export and verify. **On Tuesday**, 1a to 2k pass; 2l passes: the dry run stops at `Classify: the
proposal classified ask`, sending nothing, the spec's answer for the R0 mandate's `ask` opening
(DEC-529); 2m asks (agent stream: `DecisionMade`, `ApprovalRequested`, `ApprovalDelivered`), and
2n, 2o and 2p find it, show its code and commit the founder's `cli_confirm` grant on the control
stream (`granted … at seq 11`). The runtime then refuses the grant `step_up_method`
([R16](#the-blocker-chain-beyond-step-2l)) and the ask times out (`ApprovalTimedOut`; 2m exits 1,
`the ask reached its deadline with no grant`), so no order is sent and the fill is not reached.
2q passes for all three streams; 2r passes for the account stream (`seq 1 to 125`) and fails for
the control and agent streams with `artifact_missing` at the grant (ctl `seq 11`) and the request
(agent `seq 7`) ([R17](#the-blocker-chain-beyond-step-2l)). 2s asks again and times out the same
way, so it is refused before any place call, but by R16, not by the spent record. **The
rehearsal stops here, at R16 and R17.**

**Re-run on `claude/r0-runtime-connection-checked` (R16 and R17 fixed, [DEC-970](../decisions/DEC-970.md),
claim #1428), with no Alpaca key in the environment, on Saturday 2026-10-17, with `--sim-clock
2026-10-20T15:00:00Z` (Tuesday), `main` at `3efbd51` with B3 (#1426):**

- **Passed.**
  - 1a to 2k pass.
  - 2l passes: the dry run stops at the ask.
  - 2n and 2o pass.
  - 2p passes R16: the founder's `cli_confirm` grant is answered `Admitted and sent to the
    executor; the gate still decides.` The runtime re-validates it (`ApprovalRevalidated`) and
    proposes the opening (`IntentProposed`).
  - The executor receives it, the gate decides it, and the entry is sent to the simulator, which
    accepts it: `OrderSubmitted` of a day limit buy of 1 at 85.01, then `OrderStateChanged` to
    `accepted` as `rh-sim-000001`, with B3's protection `intended` at stop 80.76.
- **Stopped at R18.** 2m fails before any fill is read: `the tracer stopped at Connector: the
  request was not sent (not_offered)` ([R18](#the-blocker-chain-beyond-step-2l)).
- **Behind it.** The fill, B3's protective stop-limit and the take-profit reading are not reached.
- **Verify.** 2q and 2r now pass for all three streams (R17): ctl `seq 1 to 11`, agent `seq 1 to
  14`, account `seq 1 to 14`.
- **2s** passes: it is refused before any place call with `the account the connection binds is not
  flat`, the lapse's answer for the accepted entry still at the broker.
- **Diagnostic builds.** Two builds, not committed, named the refused request:
  `GetOrderByClientId`, the lookup the executor keeps for an order that is not `Unknown`.
- **A second ask.** The agent stream also shows a second `DecisionMade` and `ApprovalRequested` in
  the same run after the granted `IntentProposed`. That is one more ask while the first opening is
  working, so the run context's `working_opening_cost` of zero (`crates/mandate-shell/src/paper/context.rs`)
  needs a look in R18's lane. It was answered by nothing and sent nothing.

**The rehearsal stops here, at R18.**

**Re-run on `claude/r0-runtime-connection-checked` (R18 and the second ask fixed,
[DEC-1150](../decisions/DEC-1150.md), claim #1428), with no Alpaca key in the environment, on
Saturday 2026-10-17, with `--sim-clock 2026-10-20T15:00:00Z`:**

- **Passed.**
  - 1a to 2l, 2n, 2o and 2p pass as before.
  - 2m passes R18. The watch reads the accepted entry by the profile's listing, and its record is
    named ours by the broker id the place answered (the simulator does not echo `ref_id`). The
    executor adopts it as `filled`, 1 share (`OrderStateChanged`, account seq 15).
  - The second ask is held: agent seq 12 is a `DecisionMade` denied `working_order_limit`, and no
    second `ApprovalRequested`.
  - 2q and 2r pass for all three streams (ctl `seq 1 to 11`, agent `1 to 12`, account `1 to 23`).
  - 2s is refused before any place call: `the account the connection binds is not flat`.
- **Stopped at R19.** 2m exits non-zero: `the startup reconciliation found a mismatch; the agent
  stays paused` ([R19](#the-blocker-chain-beyond-step-2l)).
  - No `FillApplied` is ever journaled, so the ledger holds no shares while the broker holds 1:
    `BrokerPositionObserved` with `broker_qty` 1 and `mismatch`, then `AgentModeApplied` to
    `paused` and `ReconciliationRun` with `mismatch`.
  - B3's protective stop is not placed. The protection records `unprotected_start`,
    `expiry_unreplaceable` and `unprotected_end` with `uncovered`, and the run is paused, which is
    the safe stop.
- **Not reached:** the protective stop resting, its trigger, and the flatten.
- **Earlier diagnosis corrected:** two diagnostic runs, not committed, named the request that
  stopped R18 as the shell watch's own `GetOrderByClientId` (`crates/mandate-shell/src/tracer.rs`,
  `watch`), not the executor's `query_unknown` alone, which had the same gap.

**The rehearsal stops here, at R19.**

**Re-run on `claude/r0-runtime-connection-checked` (R19 to R22 fixed,
[DEC-1151](../decisions/DEC-1151.md), claim #1428), with no Alpaca key in the environment, on
Saturday 2026-10-17, with `--sim-clock 2026-10-20T15:00:00Z`:**

- **Every step passes** ([the step table](#the-step-table-r19-to-r22-fixed-run-on-2026-10-17-with---sim-clock-2026-10-20t150000z)).
- **2m passes R19.**
  - The watch adopts the entry `filled` from its record (account seq 15).
  - The reconciliation that follows books the fill from that record and the position, as
    `LateFillApplied` with `fill_id` `record:<entry>:1`, 1 share at 85, before it compares
    positions: `BrokerPositionObserved` `mismatch: false`.
  - Cash is in band: model 15, broker 15. `ReconciliationRun` reads `adopted`, then `clean` on the
    next round.
- **B3's stop rests.**
  - The stop-limit sell of 1 is journaled and sent: GTC, stop 80.76, limit 79.9524.
  - The simulator accepts it as `rh-sim-000002`.
  - Its interval ends `acknowledged`, and 2m prints `submitted` for the entry and for the stop.
- **Fixed on the way.**
  - **R22:** the first run with R19 alone journaled `expiry_unreplaceable`, because the rehearsal
    mandate set no stop-limit offset.
  - **R20:** with the offset set, the run stopped at `the tracer places one order per run, and the
    executor asked for another` before the stop was sent.
  - **R21:** behind R20, the rh-sim watched test read the resting stop as the broker's own order,
    and the simulator's unmoving cash put the booked fill out of the cash band.
- **2q and 2r** pass for all three streams: ctl `seq 1 to 11`, agent `1 to 12`, account `1 to 31`.
- **2s** is refused before any place call: `the account the connection binds is not flat`. That is
  R0 step 7's answer for the held share.
- **Not scripted, so not reached.**
  - The stop's trigger: the script's quote never moves.
  - A flatten: B3's bound flattens only an entry it could not cover, and this one is covered; the
    script has no owner exit or kill switch step.
  - Neither is a refusal. A step for each would be a new script feature, which this change does
    not add.

**The rehearsal passes through B3's resting stop.**

**Re-run on `claude/r0-runtime-connection-checked` (R22's V-008 code fixed and the stop's trigger
scripted, [DEC-1152](../decisions/DEC-1152.md), claim #1428), with no Alpaca key in the
environment, on Saturday 2026-10-17, with `--sim-clock 2026-10-20T15:00:00Z`:**

- **1a to 2s pass as before.** 2h and 2i confirm the rehearsal mandate: it sets the stop-limit
  offset that V-008 now requires on the Robinhood connection 2f records. 2m books the entry's
  fill from its record and sends B3's stop.
- **2t passes.** The simulator's market moves `RHETF` to 80.5, through the stop of 80.76. The
  script's sell fill takes the stop at 80.5, inside its limit of 79.9524. The simulator answers
  the stop `filled`, and its position is zero.
- **Stopped at R23.** 2u, the same placing command on the account the stop left flat, is refused
  before any place call with `the account the connection binds is not flat`
  ([R23](#the-blocker-chain-beyond-step-2l)).
  - The simulator lists the sold-out position as a row of quantity zero, and the lapse counts
    rows.
  - The designed answer is `the connection record is spent`: the entry's fill spends the record
    (DEC-910 item 3).
  - Either refusal is the safe stop, and nothing was sent.
- **Not reached, and not scripted:** the owner's flatten of a second entry.
  - DEC-529 item 13 and DEC-910 item 3 refuse a second entry on a spent record by design.
  - The shipping CLI offers no owner exit or kill command.
  - Robinhood publishes no sale price to book the stop's fill with (DEC-1152, not closed).

**The rehearsal stops here, at R23.**

R23 stays Proposed (DEC-1152 item 4), so the stricter refusal stands. The script's 2u now
expects today's refusal, `the account the connection binds is not flat`, and passes; the runbook
(first live trade, F.4) tells the founder a spent record refuses as `not flat` until R23 is
decided. When R23 is decided for a zero row read as flat, 2u's expectation moves back to `the
connection record is spent`. Re-run on 2026-10-17 with `--sim-clock 2026-10-20T15:00:00Z`: every
step passes and the script exits 0; 2u reads `the account the connection binds is not flat`
before any place call, and verify reads ctl 1 to 11, agent 1 to 12, acct 1 to 31 as before.

**Result on `main` at `eb19c3b` (2026-10-29): R23 is decided, and every step passes.** The
founder decided R23 on 2026-10-26: a sold-out position's row of quantity zero is flat
([DEC-2390](../decisions/DEC-2390.md) item 1, #2510). So 2u now reads the designed refusal, `the
connection record is spent` (DEC-910 item 3), and the script's 2u expects it again, as the
paragraph above said it would. 2s, run while the share is still held, still reads `the account the
connection binds is not flat`. Run with `--sim-clock 2026-10-29T15:00:00Z`: all 33 steps pass and
the script exits 0. Verify reads ctl 1 to 11, agent 1 to 11, and acct 1 to 31. Both refusals come
before any place call.

## The step table (R22's code fixed and the stop's trigger scripted, run on 2026-10-17 with `--sim-clock 2026-10-20T15:00:00Z`)

| Step | R0 | Result | Output (last line) |
|---|---|---|---|
| 1a, 1b, 1c | 1 | PASS | `Finished` |
| 2a0 to 2k | 2, 3 | PASS | `deployed as event … at seq 10` |
| 2l | 2, 4 | PASS | `the tracer stopped at Classify: the proposal classified ask, and the tracer acts only on auto` |
| 2n, 2o, 2p | 2, 5 | PASS | `Admitted and sent to the executor; the gate still decides.` |
| 2m | 2, 5 | PASS | `submitted` the entry, then the stop; exit 0. `LateFillApplied` `record:<entry>:1` at 85, `ReconciliationRun` `adopted` then `clean` |
| 2q, 2r ctl, agent, acct | 2, 6 | PASS | `verified`: seq 1 to 11, 1 to 12, 1 to 31 |
| 2s | 2, 7 | PASS | refused before any place call: `the account the connection binds is not flat` |
| 2t | 2 (the stop) | PASS | the market at 80.5: the stop `rh-sim-000002` reads `sell`, `filled`, 1 of 1 |
| 2u | 2, 7 | **FAIL** (R23) | refused before any place call, but `the account the connection binds is not flat`, not `the connection record is spent` |

## The step table (R19 to R22 fixed, run on 2026-10-17 with `--sim-clock 2026-10-20T15:00:00Z`)

| Step | R0 | Result | Output (last line) |
|---|---|---|---|
| 1a, 1b, 1c | 1 | PASS | `Finished` |
| 2a0 to 2k | 2, 3 | PASS | `deployed as event … at seq 10` |
| 2l | 2, 4 | PASS | `the tracer stopped at Classify: the proposal classified ask, and the tracer acts only on auto` |
| 2n | 2, 5 | PASS | `<approval> agent_r0 pending deadline … remaining 594` |
| 2o | 2, 5 | PASS | `code …` |
| 2p | 2, 5 | PASS | `Admitted and sent to the executor; the gate still decides.` |
| 2m | 2, 5 | PASS (was the reconciliation's mismatch: R19; then `one_order_only`: R20) | `submitted md-…08000002`, then `submitted md-…08000002-p…14000004`, the stop; exit 0 |
| 2q, 2r ctl | 2, 6 | PASS | `verified, stream ctl:ws_r0, seq 1 to 11, …` |
| 2q, 2r agent | 2, 6 | PASS | `verified, stream agent:ws_r0:agent_r0, seq 1 to 12, …` |
| 2q, 2r acct | 2, 6 | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 31, …` |
| 2s | 2, 7 | PASS | refused before any place call: `the account the connection binds is not flat` |

## How to run it

```bash
scripts/r0-rehearsal.sh                # builds, then rehearses; exits 0 only if every step passes
scripts/r0-rehearsal.sh --no-build     # reuse the binaries in CARGO_TARGET_DIR
scripts/r0-rehearsal.sh --sim-clock 2026-10-19T15:00:00Z   # outside market hours (DEC-962)
```

`--sim-clock` runs the whole rehearsal on one clock that reads the instant when the script starts
and moves with the system clock: each CLI and runner process gets `MANDATE_RH_SIM_CLOCK` at its
start, the simulator its script's `clock`, the daily-bars writer its `<at>`, and the CLI is built
with its `rh-sim` feature. Only the `rh-sim` builds read the variable; the `live` runner and the
founder's CLI never do (DEC-962).

The script builds into its own `CARGO_TARGET_DIR` with `CARGO_INCREMENTAL=0` unless one is set.
It uses `MANDATE_PG_URL` (a superuser DSN; a schema of its own is created and dropped) or else
starts a private PostgreSQL cluster on a random loopback port and stops it at exit, and migrates
`migrations/0001_journal.sql` as `mandate_journal_owner`, as the journal tests do. It writes each
command with its exit status, stdout and stderr to `$WORK/steps.md`. The simulator's OAuth login
page is answered with the same two requests a browser makes when the founder presses Approve.

**Why not a CI job.** The rehearsal runs a binary built with `live`, and DEC-529 item 3 (enforced
by `cargo xtask live-feature`, X1) allows exactly one CI job to enable that feature, and only to
compile it; DEC-964 adds one job that runs the runner's tests in the `live,rh-sim`
build, never the rehearsal binary itself. So the script is the harness; CI only ShellChecks it (`cargo xtask ci fast`'s lint now
covers `scripts/`). The whole run takes under a minute after the builds.

## The rehearsal mandate and script

- **Mandate:** the CLI's SPY fixture made `live` on connection `conn_rh_rehearsal_01`, with
  DEC-529's limits: `max_order_usd` 100 (also the allocation, `max_position_usd` and
  `max_gross_exposure_usd`), one pinned US equity ETF, and `ask` for every opening (no `auto`
  rule). The ticker is the founder's (DEC-529 item 2); the rehearsal uses a made-up symbol,
  `RHETF`, which matters at [R5](#the-blocker-chain-beyond-step-2l).
- **Simulator script:** one agentic account with 100 USD cash and buying power, the regular
  session, `RHETF` quoted at 85, and one scripted buy fill at 85.

## The step table (R16 and R17 fixed, run on 2026-10-17 with `--sim-clock 2026-10-20T15:00:00Z`)

| Step | R0 | Result | Output (last line) |
|---|---|---|---|
| 1a, 1b, 1c | 1 | PASS | `Finished` |
| 2a0 to 2k | 2, 3 | PASS | `deployed as event … at seq 10` |
| 2l | 2, 4 | PASS | `the tracer stopped at Classify: the proposal classified ask, and the tracer acts only on auto` |
| 2n | 2, 5 | PASS | `<approval> agent_r0 pending deadline … remaining 594` |
| 2o | 2, 5 | PASS | `code …` |
| 2p | 2, 5 | PASS (was `Refused (step_up_method)`: R16) | `Admitted and sent to the executor; the gate still decides.` |
| 2m | 2, 5 | **FAIL** (R18) | `the tracer stopped at Connector: the request was not sent (not_offered)`, after `OrderSubmitted` and `accepted` (`rh-sim-000001`); no fill read |
| 2q, 2r ctl | 2, 6 | PASS (was `artifact_missing` at seq 11: R17) | `verified, stream ctl:ws_r0, seq 1 to 11, …` |
| 2q, 2r agent | 2, 6 | PASS (was `artifact_missing` at seq 7: R17) | `verified, stream agent:ws_r0:agent_r0, seq 1 to 14, …` |
| 2q, 2r acct | 2, 6 | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 14, …` |
| 2s | 2, 7 | PASS | refused before any place call: `the account the connection binds is not flat` |

## The step table (`claude/r0-runtime-connection-checked`, run on 2026-10-17 with `--sim-clock 2026-10-19T15:00:00Z` and `2026-10-20T15:00:00Z`)

`$C`, `$M`, `$B`, `$J` and `$O` are as in the tables below. The Monday and Tuesday columns are the
two runs' results.

| Step | R0 | Command | Monday 2026-10-19 | Tuesday 2026-10-20 | Output (last line), Tuesday unless stated |
|---|---|---|---|---|---|
| 1a, 1b, 1c | 1 | as below | PASS | PASS (`--no-build`, the Monday binaries) | `Finished` |
| 2a0 to 2k | 2, 3 | as below | PASS | PASS | `deployed as event … at seq 10` |
| 2l | 2, 4 | the dry run, as below | **FAIL** (R15, by design: no fresh output) | PASS | `the tracer stopped at Classify: the proposal classified ask, and the tracer acts only on auto` (Monday: exit 0, nothing decided) |
| 2m | 2, 5 | the same with `--place-one-order` | **FAIL** (R15: exit 0, nothing decided; R14 passed) | **FAIL** (R16) | `the ask reached its deadline with no grant` |
| 2n | 2, 5 | `$C mandate approvals list --agent agent_r0 $O $J` | **FAIL** (from 2m) | PASS | `<approval> agent_r0 pending deadline … remaining 594` |
| 2o | 2, 5 | `$C mandate approvals show <approval> --agent agent_r0 $O $J` | not run | PASS | `code …` |
| 2p | 2, 5 | `$C mandate approvals approve <approval> --agent agent_r0 --code <code> $O $J` (`cli_confirm`) | not run | PASS as scripted (the grant committed); refused by the runtime | `granted … at seq 11 for sha256:…`, then `Refused (step_up_method); nothing was sent.` |
| 2q, 2r ctl | 2, 6 | export and verify `ctl:ws_r0` | PASS (`seq 1 to 10`) | 2q PASS, **2r FAIL** (R17) | `failed, seq 11, artifact_missing` |
| 2q, 2r agent | 2, 6 | export and verify `agent:ws_r0:agent_r0` | PASS (`seq 1 to 5`) | 2q PASS, **2r FAIL** (R17) | `failed, seq 7, artifact_missing` |
| 2q, 2r acct | 2, 6 | export and verify `acct:ws_r0:<ref>` | PASS (`seq 1 to 6`) | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 125, …` |
| 2s | 2, 7 | the placing run again | **FAIL** (R15: exit 0) | **FAIL** (R16) | refused before any place call, but by the ask's deadline, not by the spent record |

2p's script check passes on `^granted `, the CLI's own commit; the refusal it prints next is the
runtime's `ApprovalResponded` (agent `seq 9`, `result` `refused`, `reason` `step_up_method`).

## The step table (`claude/r0-context-environment`, run on 2026-10-17 with `--sim-clock 2026-10-19T15:00:00Z`)

`$C`, `$M`, `$B`, `$J` and `$O` are as in the table below.

| Step | R0 | Command | Result | Output (last line) |
|---|---|---|---|---|
| 1a, 1b, 1c | 1 | as below | PASS | `Finished` |
| 2a0, 2a | 2 | as below | PASS | the dataset's directory; the resource metadata |
| 2b to 2k | 2, 3 | as below, each with `$C` | PASS | `opened ctl:ws_r0 … at seq 1` to `deployed as event … at seq 10` |
| 2d2 | 2, 3 | the connect check with `$C` | PASS | the login, then `checked …` |
| 2l | 2, 4 | `$C $M MANDATE_RH_SIM_ENDPOINT=<sim> mandate-paper --workspace ws_r0 --agent agent_r0 --account-ref <ref> $J $B --confirm-live` | **FAIL** (R15) | exit 0 with no line: no `would place` (was V-001: R13) |
| 2m | 2, 5 | the same with `--place-one-order` | **FAIL** (R14) | `the runtime refused its input: ConnectionChecked is not interpreted until the story that writes it` |
| 2n | 2, 5 | `$C mandate approvals list --agent agent_r0 $O $J` | **FAIL** (from 2m) | `no approvals`; 2o and 2p not run |
| 2q, 2r ctl | 2, 6 | export and verify `ctl:ws_r0` | PASS | `verified, stream ctl:ws_r0, seq 1 to 10, …` |
| 2q, 2r agent | 2, 6 | export and verify `agent:ws_r0:agent_r0` | PASS | `verified, stream agent:ws_r0:agent_r0, seq 1 to 1, …` (its `StreamOpened`, environment `live`; was `export_stream_empty`) |
| 2q, 2r acct | 2, 6 | export and verify `acct:ws_r0:<ref>` | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 2, …` |
| 2s | 2, 7 | the placing run again | **FAIL** (R14) | refused before any place call, but by R14, not by the spent record |

## The step table (`claude/r0-sim-clock`, run on 2026-10-17 with `--sim-clock 2026-10-19T15:00:00Z`)

`$C` is `MANDATE_RH_SIM_CLOCK=<the rehearsal clock at the process's start>`, which every runner and
CLI process got; `$M`, `$B`, `$J` and `$O` are as below.

| Step | R0 | Command | Result | Output (last line) |
|---|---|---|---|---|
| 1a | 1 | `cargo build --bins -p mandate-cli -p mandate-rh-sim --features mandate-cli/rh-sim` | PASS | `Finished` |
| 1b, 1c | 1 | as below | PASS | `Finished` |
| 2a0 | 2 | `rehearsal_bars $WORK/market/daily RHETF 85 2026-10-19T15:00:34.987144790Z` | PASS | the dataset's directory |
| 2a | 2 | the simulator with `"clock"` in its script, then the resource metadata | PASS | `{"authorization_servers":[…],"resource":"http://127.0.0.1:<port>/mcp"}` |
| 2b to 2k | 2, 3 | as below, each with `$C` | PASS | as below: `opened ctl:ws_r0 … at seq 1` to `deployed as event … at seq 10` |
| 2d2 | 2, 3 | the connect check with `$C` | PASS | `checked 10000100000000000000000001` |
| 2l | 2, 4 | `$C $M MANDATE_RH_SIM_ENDPOINT=<sim> mandate-paper --workspace ws_r0 --agent agent_r0 --account-ref <ref> $J $B --confirm-live` | **FAIL** (R13) | `the tracer stopped at Validate: a mandate with no validation or policy violations` (was `a run clock inside the regular session`: R12) |
| 2m | 2, 5 | the same with `--place-one-order` | **FAIL** (R13) | the same |
| 2n | 2, 5 | `$C mandate approvals list --agent agent_r0 $O $J` | **FAIL** (from 2m) | `no approvals`; 2o and 2p not run |
| 2q, 2r ctl | 2, 6 | export and verify `ctl:ws_r0` | PASS | `verified, stream ctl:ws_r0, seq 1 to 10, …` |
| 2q, 2r acct | 2, 6 | export and verify `acct:ws_r0:<ref>` | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 2, …` |
| 2q, 2r agent | 2, 6 | export and verify `agent:ws_r0:agent_r0` | **FAIL** (from 2m) | `export_stream_empty` |
| 2s | 2, 7 | the placing run again | **FAIL** (R13) | refused before any place call, but by V-001, not by the spent record |

## The step table (`claude/r0-sim-market-data`, run on 2026-10-17 at about 01:10 UTC)

`$M` is `MANDATE_RH_SIM_MARKET=$WORK/market`, and `$B` is `--bars
$WORK/market/daily/alpaca/iex/bars-1Day/RHETF`. 1a to 2k are as in the table below, all PASS.

| Step | R0 | Command | Result | Output (last line) |
|---|---|---|---|---|
| 1c | 1 | `cargo build -p mandate-paper --example rehearsal_bars` | PASS | `Finished` |
| 2a0 | 2 | `rehearsal_bars $WORK/market/daily RHETF 85`, after writing `$WORK/market/market.json` | PASS | the dataset's directory |
| 1a to 2k | 1 to 3 | as below | PASS | as below |
| 2l | 2, 4 | `$M MANDATE_RH_SIM_ENDPOINT=<sim> mandate-paper --workspace ws_r0 --agent agent_r0 --account-ref <ref> $J $B --confirm-live` | **FAIL** (R12) | `the tracer stopped at Validate: a run clock inside the regular session` (was `MARKET_UNREAD`: R5) |
| 2m | 2, 5 | the same with `--place-one-order` | **FAIL** (R12) | the same |
| 2n | 2, 5 | `mandate approvals list --agent agent_r0 $O $J` | **FAIL** (from 2m) | `no approvals`; 2o and 2p not run |
| 2q, 2r ctl | 2, 6 | export and verify `ctl:ws_r0` | PASS | `verified, stream ctl:ws_r0, seq 1 to 10, …` |
| 2q, 2r acct | 2, 6 | export and verify `acct:ws_r0:<ref>` | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 2, …` |
| 2q, 2r agent | 2, 6 | export and verify `agent:ws_r0:agent_r0` | **FAIL** (from 2m) | `export_stream_empty` |
| 2s | 2, 7 | the placing run again | **FAIL** (R12) | refused before any place call, but by the session, not by the spent record |

## The step table (`claude/r0-live-workspace`, run at `674a29a`)

`ws_r0`, `founder`, `agent_r0`, the account ref and the check id are the rehearsal's opaque ids.
A row may group several of the script's step ids: 2c is `2c`, `2c-fee`, `2c-cal` and `2c-rules`;
2q and 2r are `2q-ctl`, `2r-ctl`, `2q-agent`, `2r-agent`, `2q-acct` and `2r-acct`.
`$J` is `--journal $DSN --store $WORK/store`, and `$O` is `--workspace ws_r0 --user founder
--environment live`, which every owner command takes (DEC-952).

| Step | R0 | Command | Result | Output (last line) |
|---|---|---|---|---|
| 1a | 1 | `cargo build --bins -p mandate-cli -p mandate-rh-sim` | PASS | `Finished` |
| 1b | 1 | `cargo build --bins -p mandate-paper --features live,rh-sim,mandate-mcp/rh-sim` | PASS | `Finished` |
| 2a | 2 | `mandate-rh-sim --script sim.json`, then `GET /.well-known/oauth-protected-resource/mcp` | PASS | `{"authorization_servers":["http://127.0.0.1:<port>/oauth"],"resource":"http://127.0.0.1:<port>/mcp"}` |
| 2b | 2 | `mandate workspace open --workspace ws_r0 --environment live $J` | PASS | `opened ctl:ws_r0 as event … at seq 1` (in `live`) |
| 2c | 2, 3 | `mandate config register --kind instrument_snapshot instrument.json $O $J` (and `fee_config`, `trading_calendar`, `rule_set` from the tracer fixtures) | PASS | `registered as event … at seq 2` to `5` |
| 2d | 2, 3 | `mandate model register quant.ma_crossover 1.0.0 $O $J` | PASS | `registered as event … at seq 6` |
| 2d2 | 2, 3 | `MANDATE_RH_SIM_ENDPOINT=<sim> MANDATE_RH_ACCOUNT_NUMBER=<number> mandate-paper --connect-check --workspace ws_r0 --account-ref <ref> --connection conn_rh_rehearsal_01 --journal $DSN --confirm-live`, the login answered on the simulator's page | PASS | `checked 10000100000000000000000001` (was missing on `main`: R1) |
| 2e | 2, 3 | `mandate connection record --environment live --connection conn_rh_rehearsal_01 --broker robinhood --account-ref <ref> --checked <2d2's id> --margin-attestation cash_account --scope …` (the nine allowlisted tools; show the code) | PASS | `code …` (was `refused (check_missing)`) |
| 2f | 2, 3 | the same with `--code` | PASS | `recorded conn_rh_rehearsal_01 as event … at seq 7` (was `environment_mismatch`: R1b) |
| 2g | 2, 3 | `mandate version create mandate.json $O $J` | PASS | `created sha256:… as event … at seq 8` |
| 2h, 2i | 2, 3 | `mandate version confirm <version> $O $J` (show, then `--code`) | PASS | `warnings W-002`, then `confirmed as event … at seq 9` |
| 2j, 2k | 2, 3 | `mandate agent deploy agent_r0 <version> $O $J` (show, then `--code`) | PASS | `warnings W-002`, then `deployed as event … at seq 10` |
| 2l | 2, 4 | `MANDATE_RH_SIM_ENDPOINT=<sim> mandate-paper --workspace ws_r0 --agent agent_r0 --account-ref <ref> $J --bars <dir> --confirm-live` | **FAIL** | `the lapse was handed no account-stream read and no account snapshot` (R2) |
| 2m | 2, 5 | the same with `--place-one-order`, the login answered on the simulator's page | **FAIL** (R2) | the same; no login URL was printed |
| 2n to 2p | 2, 5 | `mandate approvals list`, `show`, `approve <id> --code <code>` (`cli_confirm`) | **FAIL** (from 2m) | `no approvals`; nothing to show or grant |
| 2q, 2r ctl | 2, 6 | `mandate journal export --journal $DSN ctl:ws_r0 ctl.jsonl`; `mandate journal verify ctl.jsonl --store …` | PASS | `verified, stream ctl:ws_r0, seq 1 to 10, …` |
| 2q, 2r acct | 2, 6 | the same for `acct:ws_r0:<ref>` | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 2, …` (2d2's `StreamOpened` and `ConnectionChecked`) |
| 2q, 2r agent | 2, 6 | the same for `agent:ws_r0:agent_r0` | **FAIL** (from 2m) | `export_stream_empty` |
| 2s | 2, 7 | the placing run again: must refuse before any place call | **FAIL** (R2) | refused, but by the unsupplied lapse evidence, not by the spent record |

### The refusals observed, and the line responsible

| Step | Refusal | Where | Cause | Lane |
|---|---|---|---|---|
| 2l, 2m, 2s | `lapse_evidence_not_supplied` (`the lapse was handed no account-stream read and no account snapshot`) | `crates/mandate-paper/src/door.rs:174` (`unsupplied`), reached through `the_live_door`'s `lapse_read` (`crates/mandate-paper/src/lib.rs:654`), which `run()` calls at `:469` with no snapshot | The door's lapse needs the agentic account's `AccountSnapshot`, which only the Robinhood connector reads, and `run()` hands none (DEC-933 item 3, DEC-919) | **None.** See [R2](#the-blocker-chain-beyond-step-2l) |

## The blocker chain beyond step 2l

R1 and R1b are fixed and observed passing; R2 is observed at 2l. The rest are not observed,
because 2l stops the rehearsal first; each is read from the code, in the order a run reaches it
once the one before is fixed.

| # | Refusal | Where | Why | Lane | Proposed fix |
|---|---|---|---|---|---|
| R1 | `check_missing` at `connection record` | `crates/mandate-cli/src/connection.rs:294` | No connect sequence journaled `ConnectionChecked` | **Fixed** in `claude/r0-connection-checked` ([DEC-946](../decisions/DEC-946.md)): steps 2d2 and 2e pass | The fix as built: a connect-check mode in the one binary allowed to dial the broker, the runner (for example `mandate-paper --connect-check --workspace … --account-ref … --connection … --confirm-live`), which logs in (O1b), lists the tools and hashes the contract, reads `get_accounts`, runs `mandate_connections::checks::run`, and appends `StreamOpened` and `ConnectionChecked` (occasion `connect`) on `acct:{workspace}:{account_ref}`, printing only the event id for `--checked`. Safety-critical (connectors, OAuth): a DEC-921 row with its tests first. The CLI cannot do it (`forbidden_internal` keeps it off the connectors) |
| R1b | `environment_mismatch` at `connection record --code` (2f) | `crates/mandate-journal/src/lib.rs:703` | `workspace open` opened the control stream in `paper` only, and the journal fixes a stream's environment at its `StreamOpened`, so no `live` control record could be appended to it | **Fixed** in `claude/r0-live-workspace` ([DEC-952](../decisions/DEC-952.md)): steps 2b to 2k pass | The fix as built: paper and live never share a stream. `workspace open --environment live` opens a live workspace's control stream in `live`; an id whose stream holds any event is refused either way; every owner command takes `--environment` (`paper` by default), and one whose environment is not its control stream's is refused `workspace_environment` before anything is stored or appended |
| R2 | `lapse_evidence_not_supplied`, observed at 2l before the fix | `crates/mandate-paper/src/lib.rs` passed `None` as the snapshot, so `door.rs` called `unsupplied` | The door's lapse needs the agentic account's `AccountSnapshot`, which only the Robinhood connector reads | **Fixed**, [#1390](https://github.com/kunwarshivam/mandate/pull/1390) (claim [#1378](https://github.com/kunwarshivam/mandate/issues/1378); [DEC-947](../decisions/DEC-947.md) items 1 and 2) | `run()` now runs, for a live connection: the door's `chosen` and `account_stream` → `acknowledge` and `harden` → the dial → the shell's `checked_before_credentials` (the whole fold but the door's condition) → the one `connect` → `Session::account_snapshot` → `lapse_read(Some(snapshot))` → `confirmed_version` with the door passed. FT-3 is amended for a live run under DEC-929: the login comes after every check that needs no credential except the configuration and artifacts, and the session only reads until the cycle |
| R3 | `DeploymentRefusal::Live` | `crates/mandate-paper/src/lib.rs` handed `LiveAdmission::refusing()` | DEC-934 item 4 | **Fixed**, #1378 (DEC-947 item 3) | `live_admission(BUILT_WITH, --confirm-live, the door's answer, the dialed endpoint's host, pinned_host(Build::CURRENT))`. The `rh-sim` build pins `REHEARSAL_HOST` (`127.0.0.1`, the simulator's address) and no longer compiles Robinhood's `MCP_HOST`; the `live` build alone pins `agent.robinhood.com`. DEC-934 item 3's exact equality is unchanged |
| R4 | `Cause::Absent` with `SESSION_UNBUILT` | `crates/mandate-paper/src/robinhood.rs` (`Linked`) | No MCP session carried the login's token | **Fixed**, #1378 (DEC-947 items 4 and 5) | the session is the login, the pinned transport carrying the token as its bearer (DEC-946's `with_bearer`), `McpClient::connect` (first-connection contract check), one `RobinhoodConnector` scoped to `MANDATE_RH_ACCOUNT_NUMBER`, in the shell's `RobinhoodBridge`. Its preflight is DEC-933 item 2's, its cycle the shell's new `robinhood_production_cycle`, its broker ask #1380's `BrokerAsk::robinhood` |
| R5 | `Stage::Reconcile` refusals of the market reads (**fixed for the rehearsal** in `claude/r0-sim-market-data`, [DEC-961](../decisions/DEC-961.md): the `rh-sim` build reads them from a fixture, every other build from Alpaca as below) | `mandate_shell::paper::market_facts` (`crates/mandate-shell/src/paper/facts.rs:145`) and `robinhood_facts` (`:201`) | A Robinhood run's preflight reads the asset record, IEX quote and minute bars from **Alpaca's** data API (DEC-933 item 2), and `--bars` must come from `mandate download`, so the rehearsal needs the founder's Alpaca data keys, which no agent has (rule 8); and the simulator's quote must name the same instrument and agree with the real IEX quote within the collar (DEC-529 item 12), so the rehearsal must pin the founder's real ticker and script the simulator's quote near its price, not `RHETF` | Not a code defect | The founder runs the rehearsal on their own machine with their data keys, after `mandate download` and `inspect`, and sets the simulator's `quotes` from the latest IEX ask (the script's `sim.json`, one line) |
| R12 | `the tracer stopped at Validate: a run clock inside the regular session`, **observed at 2l, 2m and 2s** on 2026-10-17 (a Saturday) | `crates/mandate-shell/src/paper/judge.rs:95` (`regular_session_outside_close_window`), reached from `judge` (`:62`) through `load_contexts_with_clock`, which `run()` calls at `crates/mandate-paper/src/lib.rs:697` | An opening is judged only inside the regular session and outside its last `close_window_minutes` (rule 12, trading spec §9.6), and the run's clock is the system's | Not a code defect; **passed on the rehearsal's clock** in `claude/r0-sim-clock` ([DEC-962](../decisions/DEC-962.md), claim #1416) | Run the rehearsal between 09:30 and 15:50 New York time on a trading day, or with `--sim-clock <instant inside a regular session>`, which only the `rh-sim` builds read |
| R13 | `the tracer stopped at Validate: a mandate with no validation or policy violations`, **observed at 2l, 2m and 2s** on the rehearsal's clock (V-001; W-002 only a warning; no policy violation) | `crates/mandate-shell/src/adapters.rs:898` (the refusal's text in `validated_mandate`, `:894`, called at `:951`), on the run context `crates/mandate-shell/src/paper/context.rs:372` builds in `run_context` (`:294`): `connection_environment: Some(mandate_domain::Environment::Paper)` | The shell's run context hard-codes the connection as `paper`, a lock from the paper adapter's first slice like R9's, so V-001 (`crates/mandate-spec/src/validate.rs:545`) rejects every `live` mandate at the cycle's validation, though `version confirm` and `agent deploy` validated it against the recorded live connection | **Fixed** in `claude/r0-context-environment` ([DEC-965](../decisions/DEC-965.md), claim #1421): 2l passes V-001 | The fix as built: `load_contexts` takes the chosen connection's environment, which `run()` passes from the door's `chosen` (`crates/mandate-paper/src/lib.rs:708`) and `run_context` hands V-001 (`crates/mandate-shell/src/paper/context.rs:386`); the cycle's admission refuses `environment_mismatch` unless the mandate is in the same environment (`crates/mandate-shell/src/tracer.rs:127`, replacing `non_paper_environment`); every draft's envelope says that environment (`crates/mandate-shell/src/envelope.rs:114`, replacing the constant `paper`). A paper run is unchanged; a mismatch still refuses V-001 |
| R14 | `the runtime refused its input: ConnectionChecked is not interpreted until the story that writes it`, **observed at 2m and 2s** (not reached by the dry run 2l, whose journal is in memory) | `crates/mandate-runtime/src/state.rs:689` (`fold`'s catch-all, `RuntimeError::NotInterpreted`), reached from the cycle's account-stream replay, `crates/mandate-shell/src/tracer.rs:818` (`replay`) through `fold` (`:1410`) | The connect check (2d2) journals `ConnectionChecked` on the account stream (journal catalogue: `ConnectionChecked` v1 on `StreamType::Account`), and the runtime's fold knows no such event type, so it refuses rather than guess (DEC-85). R1 added the record after the runtime's list was written | **Fixed** in `claude/r0-runtime-connection-checked` ([DEC-969](../decisions/DEC-969.md), claim #1428): 2m and 2s replay it on both clocks | The fix as built: the runtime's fold reads `ConnectionChecked` strictly (journal spec §9.8's closed shape, else `non_canonical_payload`) and holds the latest check of the deployment's connection; while it failed or is a day old, an `open` or `increase` is denied `account_restricted` in the dry run and at a grant's re-validation, and no exit, protective order or kill switch is ever read against it; the executor folds it as a record. `ConnectionStateChanged` and `ConnectionCredentialRefreshed` still refuse `not_interpreted`. Was proposed: the runtime's fold interprets the account stream's connection records (`ConnectionChecked`, and `ConnectionStateChanged` and `ConnectionCredentialRefreshed` beside it) as facts it holds no state for, with the executor's fold checked for the same record; a safety-critical runtime change with its tests first |
| R15 | none: **observed at 2l** on the Monday clock, exit 0 and no line printed | `crates/mandate-builder/src/builder.rs:220` (an output is fresh only while `now < expires_at`), so `decide` (`crates/mandate-runtime/src/step.rs:510`) gets no proposal and returns with nothing journaled | The model's output is from the last daily close before the run, Friday 2026-10-16 20:00 UTC, and expires one `max_output_age_s` (the mandate's 86400 s) later, before Monday's session opens. A diagnostic build, not committed, printed `ModelOutputRecorded` with `expires_at` `2026-10-17T20:00:00Z` and an empty report | **Expectation fixed** in `claude/r0-runtime-connection-checked` (DEC-969 item 7): 2l now expects the ask and passes on Tuesday; on Monday it fails by design | The age rule is unchanged: on a Monday no daily-close output is fresh, and the schema caps `max_output_age_s` at 86 400, so not even the envelope can cover the weekend. Was proposed: run the rehearsal on a Tuesday to Friday clock (`--sim-clock 2026-10-20T15:00:00Z` moved 2l to the `ask` stop); the live run on a Monday needs a `max_output_age_s` covering the weekend, an envelope field the founder sets. Then 2l's dry run of an `ask` opening exits non-zero at `Classify` (`crates/mandate-shell/src/tracer.rs:428`), so the script's `would place` check for 2l needs to accept that answer |
| R16 | `Refused (step_up_method); nothing was sent.`, **observed at 2p** on Tuesday's clock: the runtime's `ApprovalResponded` refuses the founder's `cli_confirm` grant, and the ask times out (2m, 2s: `the ask reached its deadline with no grant`) | `crates/mandate-approval/src/stepup.rs:107` (`cli_confirm` on `live` is `StepUpRefusal::Method`), which `crates/mandate-approval/src/admit.rs:241` lifts only when the admission context carries DEC-923's live scope; the shell builds that context's settings with `live_cli_confirm: None` (`crates/mandate-shell/src/adapters.rs:937`, in the `MandateView` the shell hands the runtime), read at `crates/mandate-runtime/src/escalation.rs` (`answered`, `live_cli_confirm: settings.live_cli_confirm…`) | The live `cli_confirm` scope (DEC-923: Robinhood, DEC-529's limits, the one approver) exists in `mandate-approval` and the runtime but no run supplies it, so every live `cli_confirm` grant is refused `step_up_method` | **Fixed** (DEC-970 items 1 to 3): 2p passes | The fix as built: `mandate_shell::approvers::live_cli_confirm` reads the scope as the CLI does, the runner puts it on the run context for a live connection, and the view hands it to the runtime. Was proposed: the shell's mandate view takes the `LiveScope` from the confirmed live version and its connection record (the same facts `mandate approvals approve` checked), never a constant; a safety-critical change (step-up) with its tests first |
| R17 | `failed, seq 11, artifact_missing` (ctl) and `failed, seq 7, artifact_missing` (agent), **observed at 2r** on Tuesday's clock | `mandate journal verify` (`crates/mandate-journal/src/verify.rs`, `ArtifactMissing`); the agent stream's `ApprovalRequested` lists its own content hash in `artifact_refs` because the shell's envelope collects every digest in the payload (`crates/mandate-shell/src/envelope.rs:103`, `collect_digest_refs`), and the CLI's `ApprovalResponseSubmitted` cites the same hash, but the content object, carried inline in the request's `content`, is never put in the `--store` the run and the CLI share | An exported stream with an approval cannot be verified against the store | **Fixed** (DEC-970 item 4): 2r passes for all three streams | The fix as built: the placing run's journal puts each request's canonical content under its `content_hash` before the append, refusing a mismatch or a failed put. Was proposed: either store the request's content object (canonical bytes, its hash the `content_hash`) in the run's store when the `ApprovalRequested` is appended, or keep an inline content's hash out of `artifact_refs`; which is the journal spec's reading (§3 `artifact_refs`, §11 check 6) needs reading first |
| R18 | `the tracer stopped at Connector: the request was not sent (not_offered)`, **observed at 2m** on Tuesday's clock, after the entry was sent and accepted (`rh-sim-000001`) and before any fill was read; **fixed** (DEC-1150): the watch and the executor read a working order by the profile's listing, named by the broker id the place answered | `crates/mandate-robinhood/src/connector.rs:352` (`call`: `GetOrderByClientId` is not served), reached from the executor's `query_unknown` (`crates/mandate-executor/src/listing.rs:91`), which keeps the by-id lookup for an order whose state is not `Unknown`. Its callers include `overdue` (`crates/mandate-executor/src/protection.rs:704`), which queries a working order whose wait ran past its bound. Two diagnostic builds, not committed, showed the one refused request was `GetOrderByClientId` | Robinhood's profile cannot query by client order id (`query_by_client_order_id: false`), and DEC-529 item 4 and DEC-862 give an `Unknown` order the `ListOrders` fallback, but a working (`accepted`) order's lookup still goes by id, which this connector refuses unsent, and the tracer stops at `Stage::Connector` | **Fixed** in `claude/r0-runtime-connection-checked` (DEC-1150 items 1 to 3 and 7): 2m reads the fill | Was proposed: give a non-`Unknown` order's lookup the same `ListOrders` fallback on a profile that cannot query by id, or keep the order's state from the broker's own order listing, so the fill is read; a safety-critical executor change (reconciliation) with its tests first |
| R19 | `the startup reconciliation found a mismatch; the agent stays paused`, **observed at 2m** on Tuesday's clock after R18's fix, the entry adopted `filled` from the listing; **fixed** (DEC-1151 items 1 to 4) | The executor booked a fill only from a fill report, and on Robinhood's path none reaches it: the connector serves no activities read (`ListActivities` is among the requests `RobinhoodConnector::call` refuses unsent), and the order record carries the filled quantity alone. The reconciliation then read the broker's position against an empty ledger (`BrokerPositionObserved` `mismatch`) and paused the agent | The fill was real at the broker and absent from the journal, so the agent paused, which was correct and safe, and B3's protective stop was never sized | **Fixed** in `claude/r0-runtime-connection-checked` (DEC-1151): 2m books the fill and reconciles clean | As built: `mandate_executor::recorded_fills` reads the fill from the order record's filled quantity and the position's average price, only on a ledger flat in the instrument with no other unbooked fill and a position of exactly that quantity, under the id `record:<key>:<qty>`; Robinhood's reconciler hands it to the reconciliation, which books it before positions are compared. Anything else derives nothing |
| R20 | `the tracer places one order per run, and the executor asked for another`, **observed at 2m** on Tuesday's clock with R19 and R22 fixed, after the fill and before the stop was sent; **fixed** (DEC-1151 item 5) | `crates/mandate-shell/src/tracer.rs` (`broker`: every submission after the first was `SecondSubmission`) | On a profile with no bracket, B3's stop-limit is a second submission after the entry's fill, and the tracer's one-order rule counted it, against DEC-940 item 2's "a protective order … always passes" and `AGENTS.md` rule 13 | **Fixed** in `claude/r0-runtime-connection-checked`: the rule counts only a submission that adds risk, and the watch watches the run's opening | — |
| R21 | `the startup reconciliation found a mismatch; the agent stays paused` and `Append { outcome: "Invalid" }`, **observed in** the rh-sim watched test (`crates/mandate-shell-rh-sim/tests/restart.rs`) with R19 and R20 fixed; **fixed** (DEC-1151 items 6 and 7) | `crates/mandate-robinhood/src/preflight.rs` (`account_snapshot` named no order ours) and `crates/mandate-rh-sim/src/lib.rs` (`fill` moved no cash) | The snapshot's working stop, whose record echoes no `ref_id`, read as the broker's own order (`ExternalActivityIngested`, which the journal refuses); and the simulator's cash stayed at 750.25 after a 655.20 buy, so the booked fill fell out of the cash band | **Fixed** in `claude/r0-runtime-connection-checked`: the snapshot names a record as DEC-1150 item 7 names a listed one; the simulator spends a buy's cost from cash and buying power and adds a sale's proceeds to neither | — |
| R22 | `ProtectionChanged expiry_unreplaceable`, then `unprotected_end` `uncovered`, **observed at 2m** with R19 fixed; **fixed** in the rehearsal mandate (DEC-1151 item 8) and in V-008's code (DEC-1152 items 1 and 2) | `crates/mandate-spec/src/validate.rs` (`document_rules`: V-008 required the offset only for crypto) | Robinhood protects US equities with a stop-limit only, and the rehearsal mandate set no `stop_limit_offset`, which V-008 requires for that profile and, without the profile, for every class; `version confirm` accepted it | **Fixed**: V-008 and W-002 read the classes the connection's broker protects with a stop-limit (DEC-539 item 2's table) and fail closed without one; a live Robinhood mandate with no offset is refused `version_invalid` at `version confirm` | — |
| R23 | `the account the connection binds is not flat`, **observed at 2u** after the stop filled at the simulator (2t) and the account was flat there | `crates/mandate-paper/src/connection.rs` (`lapse`: `held.positions` counts position rows, a sold-out row of quantity zero included) | The simulator lists a sold-out position as a zero row (DEC-902 item 3), and the lapse counts it; the designed refusal is `the connection record is spent` (DEC-910 item 3: the entry's fill spends the record) | **Decided** 2026-10-26 by the founder: a zero row is flat ([DEC-2390](../decisions/DEC-2390.md) item 1, #2510); 2u now reads `the connection record is spent` | Done. Behind it, by design: no second entry on a spent record (DEC-529 item 13). `mandate agent` now offers `pause`, `kill`, `stop` and `exit`, which only a running stream owner applies, and R0's runner has exited by then. A Robinhood sell is booked at its limit where an executor reads it (DEC-2300 items 1 to 3, #2554) |
| R6 | the `ask` opening denied at its deadline | `crates/mandate-paper/src/wiring.rs:296` (`wait_for_the_grant`) | Until item 9 lands, the runtime is never handed the founder's grant, so the opening is skipped at `timeout_s` (`on_timeout: skip`, rule 3) even after `mandate approvals approve` commits it | **Item 9**, [#1367](https://github.com/kunwarshivam/mandate/pull/1367) (`claude/live-grant-cycle-answers`, DEC-940) | — |
| R7 | the restart (R0 step 7) | the startup reconciliation | A restart after a lost place answer must adopt exactly one matching order or keep it `Unknown` (DEC-529 item 4) | **Item 10** (`claude/unknown-adoption-run-path`) | — |

| R8 | the opening refused by the broker, or read as external activity | the Robinhood cycle's instrument names | The executor sends the asset id as the order's symbol, which Robinhood does not accept, and `RobinhoodReconciler` reads its snapshot past `BrokerSymbols` (DEC-938), so the account's positions name tickers | None named (DEC-947's "Not decided here") | Map the asset id to the registered ticker in `RobinhoodShellConnector`'s requests, and the snapshot's tickers back to asset ids in `RobinhoodReconciler`, as `Platform` does for Alpaca's answers |
| R9 | `the tracer stopped at Validate: a paper mandate`, **observed at 2l** before the fix | `crates/mandate-shell/src/paper/artifacts.rs` (`Artifacts::from_registered`) | The shell's run artifacts refused every mandate whose environment is not `paper`: a lock from the paper adapter's first slice, which the live admission (DEC-924, DEC-947) now reaches | **Fixed** in `claude/r0-live-artifacts` ([DEC-957](../decisions/DEC-957.md), claim #1398): 2l passes it | The fix as built: a live mandate is accepted only inside a `ConfirmedVersion`, which exists for one only after all four of DEC-924's conditions; each control record carries its stream and environment (`Origin`), and the deployment and every binding (model, fee schedule, calendar, rule set, instrument snapshot) must come from one stream in the mandate's environment, so a paper object in a live run, a live object in a paper run, or any object from another workspace refuses before anything is sent. A live run's fee schedule must name `live` |
| R10 | `the tracer stopped at Validate: a fee schedule for the mandate's environment`, **observed at 2l** before the fix | `crates/mandate-shell/src/paper/artifacts.rs` (`from_registered`'s fee check) and `crates/mandate-executor/src/fees.rs` (`paper_only_fee_config`) | The only fee configuration was `PAPER_ONLY`, which refuses any environment but `paper` (DEC-526 item 4) | **Fixed** in `claude/r0-live-fee-schedule` ([DEC-958](../decisions/DEC-958.md), claim #1405): 2l passes it | The fix as built: Robinhood's posted fees (zero commission; SEC 0.0000206 × proceeds and TAF 0.000195 a share capped at 9.79 per execution on sells; CAT 0.000003 a share on buys and sells), each cited in `config/fees/robinhood-live-2026-10-16.toml`, as `robinhood_live_fee_config` in `mandate-executor`, which refuses any environment but `live` and any broker but `robinhood`. The registered object names `schedule` `robinhood_live_2026_10_16` and `broker` `robinhood`; the script registers it. Open: R10b |
| R10b | none at run time: a live sell's fees can be booked under two cents below Robinhood's own figure | `crates/mandate-accounting/src/account.rs` (`equity_fees`, `charge`) | Robinhood rounds each fee of a trade to cents separately (SEC up, TAF and CAT to the nearest cent), and §6.2 rounds the day's total once (DEC-958 item 4). R0's one order is a buy, which is never understated | **Fixed** by [DEC-960](../decisions/DEC-960.md) (claim #1409): §6.2's `fee_rounding: per_fee_per_fill` and RC-26 (spec PR #1410), then the fold on `claude/fee-rounding-code`: `ROBINHOOD_LIVE` rounds each fee of each fill up to the cent, so a live sell is never booked below Robinhood's figure; paper and Alpaca stay `daily_total` | A per-fill, per-fee ceiling for the Robinhood schedule: a trading-domain spec PR first (§6.2 and a reference case, ES-22), then the fold; the same spec PR changes §6.2's "transcribed from Alpaca's fee schedule" to the broker's |
| R11 | `the tracer stopped at Reconcile: the agentic account's snapshot`, **observed at 2l** before the fix | `crates/mandate-shell/src/robinhood.rs` (`RobinhoodBridge::preflight_facts`), reached from `crates/mandate-paper/src/robinhood.rs` (`Linked::preflight`); the refusal was `mandate-mcp`'s `Throttled` on the preflight's first `get_portfolio` | The live start read the account snapshot (three scoped reads, six calls) for the lapse, and the preflight then read it again at once, together with tradability and the quote. After the connect's own calls, the ordinary bucket (`crates/mandate-mcp/src/transport.rs`: 10 tokens, one a second) had no token left, and a throttled read was a refusal, never a wait | **Fixed** in `claude/r0-mcp-read-budget` ([DEC-959](../decisions/DEC-959.md), claim #1408): 2l's Robinhood reads all answer | The fix as built: the live start keeps the lapse's snapshot with the run clock read before it was sent, and the same session's preflight stands on it while it is at most the rule set's quote age bound old (reading only the tradability and the quote), and reads the account again otherwise; every broker read is `CallClass::Read`, which on an empty ordinary bucket waits for its next token when it is due within `read_wait` (2 s) and is refused at once otherwise. Openings and exits never wait |

On the rehearsal's clock (DEC-962) R12 to R14 are passed and R15 holds by design on a Monday (DEC-969); on a Tuesday clock R16 (the grant, 2p) and R17 (verify, 2r) were the first refusals and are fixed (DEC-970), R18 (the working order's lookup, 2m) is fixed (DEC-1150), R19 to R22 (the fill, the stop's submission and recognition, and the offset, 2m) are fixed (DEC-1151, DEC-1152), and R23 (the run after the stop's fill, 2u) is the first now. Before DEC-969, R14 (2m, 2s) and R15 (2l) were the first; what lies behind them
(the `ask` opening and the grant, R6's lane, DEC-940; the fill and R8's ticker mapping, DEC-956;
the restart, R7's lane, DEC-941) is on `main` but not yet observed. Before DEC-962: the rehearsal
could pass only after R8 lands (R9 by DEC-957, R10 by DEC-958, R11 by DEC-959), with R1 and R1b fixed on `main` and R2 to R4 by #1390, in addition to items
9 and 10, and inside the regular session (R12). R5 no longer needs the founder's machine for
the rehearsal: the `rh-sim` build reads the market from a fixture (DEC-961); the live order still
reads Alpaca's with the founder's data keys.

## Findings that need no code

- **The `rh-sim` feature does not imply `mandate-mcp/rh-sim`** (DEC-926 item 2, DEC-937 item 2):
  the runner must be built with `--features live,rh-sim,mandate-mcp/rh-sim`. The brief's R0 step 1
  says `--features live`; the rehearsal build needs all three.
- **`mandate-cli` has no features**: it is built without any, for the rehearsal and the live run.
- **A simulator quote is canonical decimal text**: `"85"` parses, `"85.00"` refuses the whole
  script (`quotes` is "missing, unreadable"), by `Script::parse`'s strict reading (DEC-926).
- **The runner needs the agentic account's number** (DEC-947 item 5): `MANDATE_RH_ACCOUNT_NUMBER`
  in the runner's environment, beside `MANDATE_RH_SIM_ENDPOINT`; the script sets the simulator's
  `RHSIM00000001`. Missing or empty, the run refuses `robinhood_account_unset` before anything is
  sent.
- **A revoked live connection reads as `live` in the fold** (found by #1378's tests, not fixed
  there): `ValidationContext` reads a revoked connection's environment as `live` so that V-001
  fires on a paper mandate, which means a live mandate on a revoked connection passes V-001. The
  runner's door refuses it first (`connection_revoked`, DEC-910 item 2), so no run reaches the
  fold with one; a spec reading that refuses it in the fold too would close the gap for every
  caller.
- **The rehearsal runner reads no Alpaca key** ([DEC-961](../decisions/DEC-961.md) item 1): the
  `rh-sim` build's Robinhood session takes its market reads from the fixture
  `MANDATE_RH_SIM_MARKET` names, whatever keys the environment holds; the `live` build without
  `rh-sim` reads them from Alpaca with `MANDATE_ALPACA_PAPER_KEY_ID` and
  `MANDATE_ALPACA_PAPER_SECRET` (DEC-947 item 5) and holds no fixture path. An agent's run still
  unsets both (`env -u MANDATE_ALPACA_PAPER_KEY_ID -u MANDATE_ALPACA_PAPER_SECRET
  scripts/r0-rehearsal.sh`), so it reads no credential (rule 8, DEC-929 item 3).
- **The private journal works on PostgreSQL 16** with the one migration applied by `psql` as the
  owner role; CI's floor is 17.

## Reproducing a single step

Each step's full output is in `$WORK/out/<step>.out` and `.err`, and `$WORK/steps.md` holds them
all with their commands. Event ids repeat across runs because the CLI derives them (DEC-290).
