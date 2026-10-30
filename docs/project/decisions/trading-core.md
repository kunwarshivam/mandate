# trading-core

DEC-2300 to DEC-2399 and DEC-2800 to DEC-2899. The rules in force:

- **The executor runs as its own process**, reads the runtime only through the journal, hands
  agents' proposed orders to the core, and journals before acting; an exit at the market goes
  with no limit and the core sends it at market or better (DEC-2003 sits in infrastructure; here:
  DEC-2033, DEC-2037, DEC-2387).
- **The account risk fold is the one instrument's risk state**: binds one instrument only,
  opens only on a canonical asset id, reads the agent's own fills and the fees charged to them,
  returns only records it can prove, and an unreadable state holds openings and nothing else; a
  deployment that would leave it unable to read is refused at deploy (DEC-2303–2315, DEC-2393,
  DEC-2809).
- **Positions are per agent**: several agents and the owner may hold one instrument in one
  account, each agent's lots its own; a fill is attributed by the broker's order id so a kill
  switch sells the agent's shares; a switch's flatten is sized against the sub-ledger at each
  send (DEC-2849, DEC-2353, DEC-2360).
- **The executor reads its hot streams only past the head it last read**, verified again every
  minute; the runtime and its dry run read the same folds the binding gate reads; every gate
  decision names the deployment's five references (DEC-2847, DEC-2854, DEC-2851, DEC-2846).
- **Robinhood, not Alpaca, carries the founder's live account** (DEC-2382 replaces DEC-2306 item
  1): the connector's reads reach only the one agentic account and fail closed otherwise, the
  preflight reads exactly the assumed shapes, and a rehearsal build reaches only a loopback
  simulator (DEC-875, DEC-902, DEC-922).
- **Restrictions and holds**: every account-restriction cause in force is journaled and lifts
  cause by cause; the executor restricts the account first when its connection is unusable; the
  owner is alerted once per episode of held openings; an owner acknowledges a reconciliation hold
  (DEC-2816, DEC-2824, DEC-2826, DEC-2855, DEC-2830).
- **The kill switch closes only what its cancel-all is confirmed to have cleared**, journals each
  close-position request before sending, never waits on an `Unknown` order for what no `Unknown`
  could still sell, and prices a flatten from the latest mark, alerting when no mark prices it
  (DEC-2333, DEC-2337, DEC-2857, DEC-2853).
- Day-trade facts are journaled on `AccountSnapshotRecorded` v2 and the dry run evaluates the
  day-trade budget on the declared facts; day-trading, settlement and conduct rules live in the
  risk gate, not optional checks (DEC-2398, DEC-2811, DEC-2301).


<a id="dec-2304"></a>

## DEC-2304: The account risk fold reads only what it can prove, and an unreadable risk state holds openings and nothing else (E6-15 slice B)

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-19: every item tightens or resolves a gap by the reading that adds no risk, and none touches an exit, a protective order, a risk exit, or a kill switch) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's rulings |
| **Story or claim** | E6-15 slices B1 to B4; [DEC-2301](DEC-2301.md) items 2 and 3; mandate spec §5.2 and §5.9; journal spec §2 and §9.27 |

### Context

E6-15's slice B is `mandate_executor::risk_state`: a pure fold of the account stream's rows into
`mandate_spec`'s `RiskState` and the executor's tripwire fold, which returns the records they append
and a view the gate can read (DEC-2301). DEC-2301 item 3 lists what makes that view unreadable and
says an unreadable view holds new openings as `exits_only` does, never as `paused`. Building it left
six questions the specs do not answer. Nothing calls the fold until slice C wires it in, after R0
closes (DEC-967 item 2).

### Decision

1. **A refusal to decide is unreadable.** When `RiskState::step` returns an error, which its own
   contract calls a refusal to decide that the caller must fail closed on, the view becomes unreadable
   with the cause `refused_to_decide`. The fold never skips the input and steps the next one.
2. **Unreadable is for good.** Once the view is unreadable, the fold keeps the first cause and the head
   it stopped at, appends nothing for any later row, and does not become readable again on its own.
   Only an operator's recovery, which a later story defines, does that.
3. **An unreadable view's mode is at least `exits_only`**, and the last readable view's mode when that
   was `paused` or `stopped`. `exits_only` holds new openings and never holds an owner's exit, a
   discretionary exit, a risk exit, a protective order, or a kill switch (`AGENTS.md` rule 13), and
   `RiskView::holds` says so; a kill switch is held by no mode at all. The tests pin it for every cause.
4. **A failed append is unreadable now and on restart.** `append_failed` makes the view unreadable with
   the cause `append_failed`. On restart the record the fold would have appended is missing, so replay
   finds a record it cannot reproduce (`not_reproduced`) and the view stays unreadable until recovery.
   Openings wait; exits go on (DEC-2301 item 3).
5. **The confirmed version is the caller's input.** A version enters the fold as a record carrying the
   validated mandate, which the caller resolves from the control stream's confirmed version. The fold
   reads no store, so it stays a function of its inputs.
6. **The reference cases, through journaled records.** MC-R and MC-W cases are driven as account-stream
   rows and compared with the harness's snapshot and events. MC-T and MC-L are not stream folds, so
   "through journaled records" means checking the `RiskDayStarted` record's `day` (MC-T) and the
   `GoalCompleted` and `PositionReleased` records (MC-L) that the fold writes.
7. **The executor's copy of `ClockAdvanced` carries the scheduler's `clock`.** Journal spec §9.13
   leaves the account-stream copy unclosed until E7-4 slice 5. The fold reads the copy's `clock`, the
   one member the scheduler's record has, and a copy without a whole-second `clock` is undecodable,
   which makes the view unreadable.
8. **A mark carries its own verdict.** Treating a journaled mark as sane would loosen §5.2, so the fold
   reads the sanity verdict and the bid from `MarkUpdated` version 2, a separate journal spec change
   that comes before slice B2. Until then the fold reads no mark.

### Consequences

- B1 (#1760) adds the types, `open`, the `Unimplemented` stub, and the pending tests; B2 fills in the
  fold. B3 and B4 add `replay`, with the restart, fault-injection, latch-oracle, and reference-case
  property tests.
- B2 also needs `mandate_spec::risk::Outcome.breach_time_s`, a tests-first pair in `mandate-spec`, for
  §9.27's `breach_time_s` member (DEC-2301 item 1).
- No R0 file changes. `mandate-executor` gains one `pub mod` line and nothing its run loop reads.

### Alternatives considered

- **Hold openings with `paused`.** That was rejected: `paused` holds the agent's own exits, which rule
  13 forbids for an unreadable state (DEC-2301 item 3).
- **Read a version-1 mark as sane.** That was rejected as a loosening reading of §5.2 (item 8).
- **Let an unreadable view recover once a later row reads cleanly.** That was rejected: every figure
  after the unread row would be folded over a stream that is not the journal's.

<a id="dec-2306"></a>

## DEC-2306: The executor reaches Alpaca's paper trading host with its connection's credential from the vault

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79: `AGENTS.md` rule 8 names Alpaca paper as a permitted environment, and nothing here reaches live money, a live host, or a credential outside the vault. If the founder reads it as loosening a safety rule, it returns to Proposed and the executor stays on the fixture). Item 1 is replaced by [DEC-2382](trading-core.md#dec-2382) for the founder's own Robinhood agentic account; for every other connection it stands |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | Lane LV (Claude Code), at the coordinator's assignment (the full paper push) |
| **Story or claim** | E21-30; claim [#1897](https://github.com/kunwarshivam/mandate/issues/1897); amends [DEC-2007](infrastructure.md#dec-2007) and [DEC-2033](infrastructure.md#dec-2033) item 7; design §3.4, §5.2; [DEC-977](earlier-900-1999.md#dec-977), [DEC-2022](infrastructure.md#dec-2022) |

### Context

DEC-2007 and DEC-2033 item 7 keep `mandate-executord` on its in-process fixture broker: no source
names a host transport or a credential, and `tests/fixture_only.rs` pins it. The founder wants the
whole paper trade, from web/ to a fill, on staging. That needs the executor to place the order on
Alpaca's paper account. `mandate-alpaca` already holds the only transport that may reach a broker
from this workspace's Rust: `AlpacaPaperHttp`, with Alpaca's paper trading host the only trading
host compiled in, no live base URL, and no `live` feature. The vault (DEC-977) already gives an
executor a lease on exactly its own connection's secret.

### Decision

1. **The broker is chosen by the run configuration.** `fixture`, the default, keeps the
   in-process fixture: the dev cell, the local cell and CI run only it. `alpaca-paper` uses
   `mandate-alpaca`'s `AlpacaPaperHttp`, which dials Alpaca's paper trading host and no other.
   The executor still declares no `live` feature, names no host, and builds no URL. One module of
   the executor may name the paper transport and the credentials. A test pins that module as the
   only one, and pins that no source names a host or an HTTP client.
2. **The credential is the connection's vault secret, and only that.** The founder chose OAuth:
   the token exchange writes the connection's access token at `connections/{connection_id}/token`
   as [DEC-2624](DEC-2624.md)'s token entry, format 1, and `mandate-connections`'
   `token_entry::decode` is its one reader, refusing any other bytes as `token_entry_invalid`.
   The executor hands the decoded token to `mandate-alpaca`'s `Credentials::bearer`, which sends
   it as one `Authorization: Bearer` header, marked sensitive, and no key header beside it. There
   is no key pair in the vault and no second codec for one. The token reaches no error, `Debug`,
   `Display`, log, or journal record. The executor never reads a credential from its environment
   or a file. Its own vault identity is the AppRole credential mounted as a file (DEC-2022), read
   by `mandate-vault`'s mounted reader.
3. **No lease, no broker.** In `alpaca-paper`, the executor takes its lease before it takes the
   account stream and refuses to start without one. It renews the lease as `LeaseKeeper` says.
   When the lease cannot be renewed and expires, the executor submits no new order. Cancels, the
   kill switch and risk exits still go out while the transport answers (`AGENTS.md` rules 3 and
   13); without a credential they cannot, and the process stops for its supervisor.
4. **Egress names one broker host.** The executor's network policy admits Alpaca's paper trading
   host on 443 and the cell's vault, nothing else. Alpaca's live host is never admitted anywhere.
5. **CI never dials a broker.** Every test of the paper path runs `mandate-alpaca`'s client over a
   scripted or recorded transport. The real transport is exercised only in a paper deployment.
6. **The executor process takes the vault; the connector crates never do.** `mandate-executord`
   moves from layer 8 to layer 9 in `xtask/layers.toml`, above `mandate-vault` (layer 8), and is
   allowed `secrecy` to read the lease's secret. This is the leasing process the vault entry's own
   rule names. `mandate-executor` (layer 6) and `mandate-alpaca` (layer 7) stay below the vault, so
   neither can depend on it, and a test pins that. The coordinator approved it as an agent
   decision.

### Slices

DEC-77 pairs, each about 400 lines or less: (LV-1) item 2's bearer credential in
`mandate-alpaca` (the entry's codec is lane X's, DEC-2624); (LV-2) items 1 and 3 in the
executor, with the module pin; (LV-3) recorded paper fill bodies for the fill reconciliation of
E21-30's X1b-3; (LV-4) the chart's run configuration and the mounted vault credential. Item 4's
network policy is lane GO's.

### Rationale

The transport, its host guard and the vault lease already exist and are tested. The executor only
has to choose between two transports it already speaks to through one client. Keeping `fixture`
the default keeps every build and test as it is, and puts the paper host behind a deliberate
configuration that only a paper deployment carries.

### Alternatives considered

- **Credentials from the environment, as the paper runner reads them.** Rejected: design §5.2 and
  E21-32 keep every secret out of the environment, and the vault lease already exists.
- **A second transport inside the executor.** Rejected: it would duplicate the host guard and
  redirect refusal that `mandate-alpaca` holds and tests.
- **The paper runner (`mandate-paper`) places the staging orders.** Rejected: the executor is the
  one process that holds the journal-before-acting and reconciliation the trade must prove.

<a id="dec-2307"></a>

## DEC-2307: E6-15 slice C binds the account risk state on the paper path through thin R0 hooks; the fold returns mode records and fails closed on a restriction it cannot name; the account stream gets its own tick

| | |
|---|---|
| **Status** | Accepted. Item 1 lifts DEC-967 item 2's R0-file freeze on the founder's call, DEC-2605 row 17, with the R0 re-pass row 21 requires; the agent records only how far it reaches. Items 2 to 5 are the agent's, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-19: each tightens or resolves a gap by the reading that adds no risk. None holds an exit, a protective order, or a risk exit except through agent mode `paused` or `stopped`, which item 3's `AgentModeApplied` records (`AGENTS.md` rule 13), and none holds a kill switch; item 4's promised test of a restriction the fold cannot name is retired by [DEC-2313](trading-core.md#dec-2313) item 2 |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's rulings |
| **Story or claim** | E6-15 slices B2b and C; [DEC-2605](process.md#dec-2605) rows 17, 20 and 21; [DEC-2301](DEC-2301.md), [DEC-2304](trading-core.md#dec-2304), [DEC-2305](DEC-2305.md); DEC-967 item 2 |

### Context

DEC-2605 row 17 (the founder, 2026-10-19) starts E6-15 slice C now: the account stream's risk state,
tripwires and ladder latch bind the shell's gate and the runtime, so paper orders on staging are held
by them. Row 21 holds the first live order until slice C has landed and the R0 rehearsal passes again.
DEC-967 item 2 froze the R0 files until R0 closes, and the E6-15 backlog entry said slice C waits for
R0 or the founder's call. Building B2b and planning C left four questions the specs do not answer.

### Invariants first

- An unreadable or undecided risk view is at least `exits_only`, never `normal` (DEC-2304 item 3).
- No risk-state reading holds an exit, a protective order, a risk exit, or a kill switch on its own
  account (`AGENTS.md` rule 13).
- The fold stays pure: it returns records and never acts (DEC-2304).
- Every risk record is journaled before the next intent is gated (`AGENTS.md` rule 5).

### Decision

1. **Row 17 lifts DEC-967 item 2 for slice C only, and only for the three files the backlog names**:
   `mandate-shell/src/adapters.rs`, `paper/gate.rs` and `paper/context.rs`. They get thin hooks that
   pass the risk view in; the logic lives outside R0 (`mandate-executor`, a new
   `mandate-shell/src/risk.rs`, and D1's `mandate-ports` from DEC-2605 row 20, whose
   `gate_context(run, risk, mode)` takes both with no default). `control.rs` and
   `mandate-paper/src/lib.rs` are not touched. Under row 21, the R0 rehearsal is re-run on the
   simulator after slice C lands and before the first live order.
2. **Slice C binds only after B2b, B2c, B3 and B4.** Before B2b and B2c, the first fill or version row
   makes the view unreadable, which fails closed but stops every paper opening; before B3 and B4, a
   fold opened fresh on each one-shot cycle forgets latched limits, which loosens the gate.
3. **The fold returns `AgentModeApplied` as a `RiskAppend`**, in the step's order, when a step changes
   the effective mode, with the payload journal spec §9 closes for it. It never runs a kill switch:
   `KillSwitchActivated`, and the cancels and closes a `flatten_and_pause` rung asks for, are slice C5's
   action path (`Initiator::RiskLimit`, agent scope; an agent-scoped kill switch never uses cancel-all
   or close-position).
4. **`InstrumentRestrictionChanged.instrument` is the mark or fill row's `instrument_id` as written.**
   A restriction record the fold cannot name, because the row carries no identifier in §9.27's space,
   makes the view unreadable (`refused_to_decide`) until B2c's universe input maps the identifiers:
   openings are held and exits are not. Items 3 to 5 are tested in the later tests-first PRs, from
   B2b-2 on, before any implementation reads them.
5. **The account stream gets its own `ClockAdvanced` in slice C**, as one more tests-first pair that
   this lane owns, instead of waiting for E7-4 slice 5. Without it, confirmed rungs and the daily loss
   never latch on paper, the risk day never starts, and staleness never counts. The record is the
   one DEC-2304 item 7 reads: the scheduler's `clock`, a whole second.

### Consequences

- Slice C is C0 (the executor fold reads the risk records), C1 (the view's mapping to the gate's
  risk snapshot and mode), C2 (the shell's risk cell and the append before the next intent), the tick
  pair, C4 (restart through B3's replay), C3 (the R0 hooks, on D1's R1), and C5 (risk actions).
- The first live order waits for slice C and the R0 re-run (DEC-2605 row 21).

### Alternatives considered

- **Wire slice C before B2b to B4.** That was rejected (item 2).
- **Let the fold run the kill switch.** That was rejected: the fold would act, so replay would act
  again.
- **Leave time-confirmed limits unbound on paper until E7-4.** That was rejected as leaving a known gap
  in a safety rule (item 5).

<a id="dec-2308"></a>

## DEC-2308: The account risk fold holds one instrument and refuses a second; its restrictions name that instrument, its mode records name the rung, and a missing midnight tick is the stream's, not the case's (E6-15 B2b)

| | |
|---|---|
| **Status** | Accepted; items 4, 5 (last clause) and 7 corrected by [DEC-2310](trading-core.md#dec-2310) (agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-19: each item tightens or resolves a gap by the reading that adds no risk; none holds an exit, a protective order, or a risk exit except through agent mode `paused` or `stopped` (`AGENTS.md` rule 13), and none holds a kill switch. No reference case changes) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's rulings |
| **Story or claim** | E6-15 slice B2b; [DEC-2304](trading-core.md#dec-2304) item 7, [DEC-2305](DEC-2305.md), [DEC-2307](trading-core.md#dec-2307) items 3 and 4; mandate spec §5.2 and §5.4; journal spec §9.6 and §9.27 |

### Context

B2b-1 reads version-2 marks into `mandate_spec::risk::RiskState`, and B2b-2 writes the records its
steps journal. `RiskState` holds a single position (`Opening` has one quantity, cost, and asset
class), while a mandate allows up to `universe.max_instruments` (1 to 20). Writing B2b-2's tests left
three readings open, and the single position left a fourth.

### Invariants first

- E moves only on a sane mark of the position it prices (§5.2).
- An unreadable view holds openings and never an exit (DEC-2304 item 3).
- The fold stays pure and journals in step order (DEC-2304, DEC-2307 item 3).

### Decision

1. **A missing midnight tick is the stream's, not the case's.** MC-R13 runs from 2026-09-21 to
   09-22 with no `risk_day_started` step, and the fold starts the day at the first tick past New York
   midnight (DEC-2304 item 7). The test that walks MC-R13 in full inserts the midnight tick a real
   account stream carries and asserts its `RiskDayStarted`, with `day_start_equity` 10000. The
   fixture is unchanged, and no expected figure in the case changes.
2. **A restriction names the instrument its rows named.** A `stale_mark` set or lifted by a mark
   names that mark's `instrument_id`; one a tick sets names the instrument the earlier marks or
   fills named. A tick that would restrict an instrument no row has named is a restriction the fold
   cannot name, and the view becomes unreadable (DEC-2307 item 4). Until item 4's single position
   is lifted (item 5), this holds within one instrument.
3. **The risk state's `AgentModeApplied`** has `originated` true, `restriction` the rung's action
   name (for example `flatten_and_pause`), and its causation is the row whose step changed the mode.
   The tests pin the value, not only its type.
4. **One instrument.** The fold binds to the first instrument a mark (and, from B2b-2b, a fill)
   names. A row naming any other instrument makes the view unreadable (`refused_to_decide`): stepping
   it would price the bound position at another instrument's quote. Openings wait; exits, protective
   orders, risk exits, and the kill switch go on. The binding is set once and never reset: not by a
   later mark, a version change, a `UniverseChanged`, a fill, or a replay, which reads the same rows
   from seq 1 and so derives the same binding from the journal alone. It applies to every row type that names an
   instrument. `Opening` names no instrument today, so the first row to name one sets it; when the
   opening carries one, the first row must match it.
5. **A multi-instrument risk state is its own change.** Per-instrument positions, marks, and staleness
   under one account E and H are a mandate spec change with their own reference cases, recorded in
   the backlog. Until it lands, paper agents run with `universe.max_instruments` 1, and the founder's
   first live order is single-instrument; the coordinator tells the founder.
6. **The fold never starts the risk day at a mark.** A mark past New York midnight with no tick
   between is stepped against the closing day's E₀, as MC-R13 steps its next-morning pre-market mark,
   and the next tick starts the day (§5.4, DEC-2304 item 7). The executor's mark writer (slice C or
   E7) journals the midnight tick before any post-midnight mark, with a writer-side test. Once MC-R13
   gains its tick step, the fold fails closed on a post-midnight mark with no tick before it.
7. **A not-sane mark's bid is never read.** §5.2 reads no bid from a not-sane mark, so the fold steps
   it with a fixed placeholder price, and a test shows two bids give equal views. The cleaner shape,
   `Input::Mark { bid: Option<Price> }`, is a `mandate-spec` change recorded in the backlog.

### Consequences

- #2007 (B2b-1) carries items 4, 6 and 7 and their tests, as #1906 carried its review fix.
- B2b-2a's tests pin items 1 to 3 and item 4's second-instrument case.

### Alternatives considered

- **Step every instrument's mark into the one position.** That was rejected: E would move on the
  wrong instrument's price.
- **Amend MC-R13 with a `risk_day_started` step.** That was rejected for now: it changes an approved
  fixture to state what a real stream carries anyway (item 1).

<a id="dec-2309"></a>

## DEC-2309: E7-4's remaining readings: the account and workspace kill switches, and the stop-only equity opening refused

| | |
|---|---|
| **Status** | Accepted. Items 1, 3 and 4: agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-22: each resolves a gap by the reading that adds no risk; none holds an exit, a protective order, or the kill switch, and none weakens a safety invariant or a non-negotiable. Item 2: founder, 2026-10-25: the interim is the rule, and the alternative is rejected |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137) (E7-4, released by lane X), on the coordinator's rulings; item 2 awaits the founder |
| **Story or claim** | E7-4; trading-domain spec §5.4, §5.5, §5.6; [DEC-346](earlier-303-599.md#dec-346) item 2, [DEC-349](earlier-303-599.md#dec-349), [DEC-485](DEC-485.md) item 1, [DEC-2003](DEC-2003.md) item 6, [DEC-2040](DEC-2040.md) item 3 |

### Context

E7-4's last slices are crypto's stop-limit sequence, the fractional disclosure, the account and
workspace kill-switch scopes, and the stop-only equity opening. Crypto's stop-limit is already
decided: [DEC-349](earlier-303-599.md#dec-349) places it once the entry's fills are ingested and sizes it net of
the larger fee rate, and its item 3 gives the add sequence. Trading-domain spec §5.4 and §5.5
leave three things unsaid: how an account-scoped switch closes an equity outside the regular
session, what the account scope's cancel-all does to resting protection, and what an equity
opening with a stop and no take-profit becomes on a profile that offers brackets. A fourth is how
a workspace scope reaches an executor that owns one account.

### Invariants first

- Risk reduction is never denied; the kill switch is always available, touches only its scope, and
  never depends on model state (`AGENTS.md` rule 13).
- Owner exits may sell equities outside the regular session only once the owner confirms the
  displayed bid (rule 13, §5.5); every other equity exit waits for the regular session.
- An agent-scoped switch never uses cancel-all or close-position (§5.5, DEC-485 item 1).
- No position the switch cannot close yet is left without protection it already had.
- No opening is ever placed unprotected (§5.4, `AGENTS.md` rule 2).

### Decision

1. **An account-scoped switch closes equities outside the regular session only on the owner's
   confirmation.** Alpaca's close-position sends a market order, which extended hours refuse. In
   the regular session, and for crypto at any hour, the account scope closes with close-position
   per instrument (§5.5's table). Outside it, an equity sells only once the owner confirms the
   displayed bid and its floor (rule 13, §5.5, DEC-2040 item 3); it then goes through the agent
   path's own `md-k-<switch>-<n>` ladder orders, priced and floored as §5.6 says. Without that
   confirmation, its sell waits for the regular session and goes then.
2. **What the account scope's cancel-all does to resting protection outside the regular session**
   (the founder's, 2026-10-25: the interim below is the rule).
   - **The rule (first the interim, in force until the founder decided):** the account
     and workspace switch keeps a position's resting protective orders wherever its close cannot go
     as a market order now: outside the regular session, in an auction window, or with stale status
     data (§5.5 sends a market close only in the regular session outside auction windows with
     current status data, and otherwise uses the exit ladder). It releases that protection only
     when the position's close goes out, so no position is left uncovered meanwhile. It cancels
     everything else by id, `Unknown` orders included. A crypto position, which closes at once at
     any hour, has its protection cancelled as its close goes out. The owner is told, in the
     switch's own alert and in the runbook, which protection the switch kept and why.
   - **Orders the platform did not place** (the owner's own, which the broker's cancel-all would
     hit): the latest reconciliation read lists every open order with its broker order id, so the
     interim cancels them by that id, as it cancels the platform's own. One exception keeps to the
     rule above: an owner's sell resting on a position the switch cannot close yet is kept with
     that position's protection and released with its close, since it can only reduce risk and may
     be the owner's own protection. The account scope's slice adds a cancel by broker order id to
     the connector for this.
   - **The alternative, rejected by the founder on 2026-10-25:** §5.5's literal cancel-all at
     once, protection included.
     It is simpler and matches the broker's endpoint, but every equity position the switch cannot
     close yet is left unprotected until the open.
3. **The workspace scope applies per account, through each account's own executor.** An executor
   owns one account stream and one connection. A workspace switch is applied by each account's
   executor to its own account, exactly as an account switch is, with the workspace id as the
   record's subject. No executor touches another account. When A4 lands, a test pins that one
   workspace switch reaches every account's executor, and that an executor it cannot reach is
   reported to the owner rather than skipped silently.
4. **A stop-only equity opening on a profile that offers brackets is refused, never sent.** Such an
   entry has no bracket to go as (DEC-346 item 2). It is refused: no order is sent, the refusal is
   journaled with its reason, and one opaque alert is raised. It is never placed unprotected, and
   it is never an error that stops the process. An opening may always be refused (`AGENTS.md`
   rule 2), and refusing it is the reading that adds no risk.

### Consequences

- The crypto slices land first under DEC-349, then the fractional disclosure, then the account
  and workspace scopes as four DEC-77 pairs (A1 to A4), then the stop-only refusal. Each gets an
  independent review.
- The account scope implements item 2's interim; if the founder takes the alternative, a later
  change drops the kept protection, and nothing else moves.
- Item 2's disclosure lands with the account scope's implementation and RB-03.

### Alternatives considered

- **Have the account scope sell equities outside the session without the owner's confirmation.**
  Rejected: rule 13 lets only an owner exit sell outside the regular session, and only once the
  owner confirms the displayed bid.
- **Leave the owner's own orders untouched.** Rejected: an owner's buy left working would add risk
  the switch exists to remove, and reconciliation already has the ids to cancel it by.
- **Re-place protection after an account kill switch's cancel-all.** Rejected: protection
  re-placed beside the switch's closes would sell the same shares twice. Keeping it until the
  close goes out, as item 2's interim does, never has both working.
- **Treat a stop-only opening as an error, or send it plain and protect it after its fill.**
  Rejected: an error stops the process for an opening that can simply be refused, and a plain entry
  is unprotected until its fill posts.

<a id="dec-2310"></a>

## DEC-2310: The account risk fold binds its instrument when it opens, and refuses an opening that cannot (E6-15 B2b-1, the #2007 review)

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-19: each item tightens DEC-2308 and adds no risk; none holds an exit, a protective order, a risk exit, or a kill switch) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's rulings on #2007's review |
| **Story or claim** | E6-15 slice B2b-1; corrects [DEC-2308](trading-core.md#dec-2308) items 4, 5 and 7, whose body stays as accepted (decisions README rule 4) |

### Context

#2007's independent review found that DEC-2308 item 4's binding, set by the first instrument a mark
names, let a held opening's first mark name another instrument and move E: a held BTC/USD opening
(MC-R20) and a first mark for ETH/USD at 59000 gave a readable view with E at 9900. That breaks
DEC-2308's own first invariant, that E moves only on a sane mark of the position it prices. #2007
merged before the fix, so this decision and its change carry it.

### Decision

1. **The fold binds its instrument when it opens.** `AccountRiskFold::open` takes the position's
   instrument and binds it at once. It refuses (`invalid_input`) a held opening that names none, so no
   first mark can name another instrument and move E, and a fold opened past seq 1 that names none,
   since a row before its head may have bound one. A flat opening at seq 1 still binds to the first
   instrument a mark (and, from B2b-2b, a fill) names. This replaces DEC-2308 item 4's first sentence;
   the rest of item 4 stands.
2. **Instrument ids are compared exactly**, which relies on the journal's canonical asset ids.
3. **`universe.max_instruments` above 1 is refused at deploy** by slice C until the multi-instrument
   risk state lands (DEC-2308 item 5), since a mandate with more would leave its agent `exits_only`
   for good. DEC-2308 item 5's last clause reads: the coordinator informs the founder that the first
   live order is therefore single-instrument; the item decides nothing about live money.
4. **The not-sane-bid test is vacuous on a flat position** (DEC-2308 item 7), so B2b-2 adds its
   held-position version.

### Consequences

- `open` takes `instrument: Option<&str>`; the tests pin both refusals, the flat opening at seq 1,
  and a held opening's first mark for another instrument.
- The backlog records items 3 and 4.

<a id="dec-2313"></a>

## DEC-2313: The account risk fold opens only on a canonical asset id (E6-15 B2b-2)

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-22: each item only refuses an opening or names a writer's obligation, and adds no risk; none holds an exit, a protective order, a risk exit, or a kill switch) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's ruling on B2b-2a's flag |
| **Story or claim** | E6-15 slice B2b-2; [DEC-2307](trading-core.md#dec-2307) item 4, [DEC-2308](trading-core.md#dec-2308) item 2, [DEC-2310](trading-core.md#dec-2310) items 1 and 2 |

### Context

DEC-2310 binds the fold's instrument at `open` and compares ids exactly, which relies on the journal's
canonical asset ids. `open` accepted any string, so a held opening named by a symbol (`BTC/USD`) or an
asset id in uppercase was bound, and failed only at its first instrument restriction, which DEC-2307
item 4 cannot name.

### Invariants first

- An instrument restriction always names a §9.27 `asset_id` (DEC-2307 item 4).
- The fold binds one instrument, compared exactly (DEC-2310).
- Nothing here holds an exit, a protective order, a risk exit, or a kill switch (`AGENTS.md` rule 13).

### Decision

1. **`AccountRiskFold::open` refuses (`invalid_input`, `what: "instrument"`) an instrument that is not
   a canonical asset id**: `mandate_domain::AssetId::parse`, lowercase `8-4-4-4-12` hexadecimal.
   Uppercase is refused, never folded (DEC-404 item 9).
2. **The test of a restriction the fold cannot name on a held opening is dropped.** With item 1 a
   held opening binds a canonical id, and every restriction on it names that id. A flat opening takes
   no restriction (mandate spec §5.2 restricts a held instrument only), so on its own it reaches none.
   One path remains: a flat opening at seq 1 is bound by the first mark that names an instrument, and
   `bind` does not check that name, so a later fill on a non-canonical id would hold a position whose
   restriction cannot be named. That path needs a fill, which B2b-2b folds; its tests cover it there.
3. **Writers journal canonical asset ids in `instrument_id`.** The executor's mark writer and fill
   writer (slice C, E7) write the instrument's canonical asset id, never a symbol. A row that names
   anything else never matches the fold's binding, so the view becomes unreadable: openings wait and
   exits go on. That fails closed, and it is the writer's obligation, with a writer-side test.

### Consequences

- `open`'s refusal is pinned by a pending test in B2b-2a's tests PR; its stub returns
  `SpecError::Unimplemented` until the implementation PR.
- The risk fold's tests name the reference bases' asset ids, never symbols.
- The backlog records item 2's remaining path and item 3's writer obligation.

### Alternatives considered

- **Fold an uppercase id to lowercase.** That was rejected: one asset would have two spellings, which
  DEC-404 item 9 refuses.
- **Keep the unnamed-restriction test on a symbol.** That was rejected: item 1 refuses the opening it
  needs, so the test would pin a state `open` can no longer reach.

<a id="dec-2314"></a>

## DEC-2314: The account risk fold binds a flat opening only to a canonical asset id (E6-15 B2b-2b)

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176, as the coordinator ruled on 2026-10-22: the item only refuses a binding and adds no risk; it holds new openings and never an exit, a protective order, a risk exit, or a kill switch) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane TW (Claude Code), claim [#1599](https://github.com/kunwarshivam/mandate/issues/1599), on the coordinator's ruling on B2b-2b |
| **Story or claim** | E6-15 slice B2b-2b; [DEC-2313](trading-core.md#dec-2313) item 2, [DEC-2310](trading-core.md#dec-2310) item 1, [DEC-2308](trading-core.md#dec-2308) item 4 |

### Context

DEC-2313 item 1 makes `open` refuse an instrument that is not a canonical asset id, so a held opening
always binds one. DEC-2313 item 2 left one path open: a flat opening at seq 1 is bound by the first
mark that names an instrument (DEC-2310 item 1), and `bind` did not check that name. A later fill on
that id would hold a position whose instrument restriction the fold cannot name (DEC-2307 item 4).

### Invariants first

- Every instrument the fold binds is a canonical asset id, so every instrument restriction it writes
  names a §9.27 `asset_id`.
- The fold binds one instrument, compared exactly (DEC-2310 item 2).
- Nothing here holds an exit, a protective order, a risk exit, or a kill switch (`AGENTS.md` rule 13).

### Decision

1. **`bind` refuses a first instrument that is not a canonical asset id** (`mandate_domain::AssetId::parse`,
   lowercase `8-4-4-4-12` hexadecimal) as a refusal to decide: the view becomes unreadable, openings
   wait, and exits, protective orders, risk exits, and the kill switch go on. It applies to every row
   that binds, a mark now and a fill from B2b-2b. This closes DEC-2313 item 2's remaining path, so no
   test of a restriction the fold cannot name on a bound position remains to be written.

### Consequences

- B2b-2b-1's tests PR pins the refusal with a pending test on a flat opening whose first mark names
  the symbol or the asset id in uppercase; its stub reports `Unimplemented` until the implementation PR.
- A writer that journals a symbol in `instrument_id` fails closed at the first mark, which DEC-2313
  item 3 already makes the writer's obligation.

### Alternatives considered

- **Keep binding any name and refuse at the restriction.** That was rejected: the view would stay
  readable on an instrument the journal cannot name until the first restriction, and only then fail.

<a id="dec-2316"></a>

## DEC-2316: The cell gateway's per-agent rate cap is derived from the pinned loop's turns plus a margin, under a fixed workspace cap

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79; the coordinator ruled it engineering, since a run's own bounds and cost ceiling still bind) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-6, found by its bench ([DEC-2271](intelligence.md#dec-2271) item 6); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534) |

### Context

The cell's worker builds its gateway with a fixed rate cap of 6 calls a minute, per workspace and
per agent (`RATE` in `crates/mandate-daemon/src/worker.rs`). The cap's doc comment gives the reason
from before the agentic loop: a one-shot run makes one call per research interval, so a small cap
never binds it and bounds a fault that would call again.

An agentic run makes one call per turn, and the `v0_2` loop pins `max_turns` 8. A run that reaches
its seventh turn within a minute is refused `rate_limited_local` and ends `error`. That fails
closed, but no cell run can use its last two turns. DEC-2271's bench found this: most of its
results-steered and random trials end there.

### Decision

1. **The per-agent cap is derived.** It is the turns one run of the entry may take, plus a margin
   of 2 (`RATE_MARGIN`):
   - for an agentic entry, the turns are the pinned loop's `max_turns`;
   - for a one-shot entry, the turns are 1, so its cap is 3 a minute, below the old 6.

   A run that uses every turn its loop allows is never cut by the rate cap.
2. **The per-workspace cap is fixed at 12 a minute** (`WORKSPACE_RATE`), whatever the entry pins.
   Each cell worker owns its gateway, so both counters see the same agent's calls. A loop that pins
   more than 10 turns is bounded by the workspace cap: its calls past 12 in one minute are refused
   and its run ends `error`, which still fails closed.
3. **What the cap is for.** It bounds a fault that would call again. A run's own `max_turns`,
   tool-call bounds, deadline and cost ceiling bind the run, and the core enforces them whatever
   the cap is.
4. **Tests first** (DEC-77):
   - a recorded run with seven scripted turns and the final reply takes the `v0_2` loop's eight
     turns within one minute, each `ok`, and ends `done`;
   - within one minute the worker answers the loop's eight turns and the margin's two, and refuses
     the eleventh `rate_limited_local`;
   - in DEC-2271's bench, a results-steered run under the injected fixture takes its eight turns
     and ends `done` with `bound` `max_turns`. The model double now gives a final output the
     entry's schema admits, so the last turn is judged.

   The three fail on today's fixed cap rather than at a stub, since no code is stubbed, so each has
   a behaviour-only row until the implementation lands.
5. **The bench gains HI-25 near its bound and from the meter** (the review of DEC-2271).
   - A trial under a 0.16 USD run cap, with replies spending 30,300 micros each, ends `cost` with
     at least half the cap spent and none of it exceeded.
   - HI-25's oracle now reads the spend twice: from the call records and from the meter's
     settlements. Agent harness spec HI-8 counts spend from the journal and the meter stream. The
     cell's meter is in memory until E21-61 journals it, so the bench reads its records through a
     test-only `with_model_metered`. The two readings must agree call by call, and each must stay
     within the ceiling.
   - Two more seeded breaches show it: a settlement whose cost differs from its record, and a
     reserved call never settled.

   These pass on today's code.

### Rationale

The cap's purpose, bounding a repeated fault, is unchanged. Only its size now follows the shape of
the run it must admit. A fixed workspace cap keeps a ceiling that no entry's pin can raise. The
one-shot cap falls from 6 to 3, which tightens it. Nothing calls that often: a one-shot run makes
one call per research interval.

### Alternatives considered

- **Raise the fixed cap to cover `v0_2`:** a later loop pinning more turns would hit the same wall,
  and a one-shot entry would keep a looser cap than it needs.
- **Derive both caps from the entry:** an entry's pin could then raise the workspace's ceiling
  without bound.
- **No cap for agentic runs:** a fault that calls again would be bounded only by the cost ceiling.

<a id="dec-2320"></a>

## DEC-2320: A refused take of an authorization code is discarded, never put back

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, at the coordinator's ruling on the review of #2121 and #2122; it only tightens DEC-2628 item 2, so a code is never readable after it is superseded, and the cost is a lost connect the owner retries) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E10-13 slice 5b-1; [DEC-2628](process.md#dec-2628) item 2, which this amends |

### Context

DEC-2628 item 2 says a taker that the code is not bound to (another connection's or environment's)
puts the entry back. In the local vault that put-back renames the taken entry to its original name.
Review found the interleaving where that resurrects a superseded code:
1. A wrong taker holds the old entry under its take name.
2. A fresh code is written, and the rightful run takes and consumes it.
3. The wrong taker's put-back finds the name free and restores the old code, readable again.

A generation check before the rename cannot close it: a write and a take can both land between the
check and the rename.

### Decision

1. **A refused take is discarded.** A take that the vault cannot decrypt, such as one bound to
   another connection or environment, unlinks its entry under its take name and never renames it
   back. A failed unlink leaves the name for the next delete's sweep. This amends DEC-2628 item 2's
   wrong-binding line.
2. **The rightful run fails closed.** After a wrong-binding take, the rightful run finds no code
   and loses the connect, and the owner retries. A code written after the refused take is left for
   the rightful taker untouched. No superseded code is ever readable again.
3. **A racing sweep gives the code or none.** A delete whose sweep races a take gives that take
   the code, when it read the code first, or none, which the exchange reads as `CodeMissing`. It
   never gives a code twice.

The tests in `mandate-vault-local` pin each item:
- a refused take is unlinked, and nothing is renamed back;
- a code written meanwhile is left for the rightful taker;
- the resurrection interleaving above brings nothing back;
- after a wrong-binding take, the rightful run finds none.

### Rationale

A wrong-binding taker means a misconfigured vault key, which is never the normal path. Losing one
connect to it is cheap; letting a superseded code come back could exchange a code the owner
replaced.

### Alternatives considered

- **Restore only when no write happened since the take.** The check and the rename cannot be one
  atomic step.
- **Leave the refused entry under its take name for the sweep.** It keeps a decrypt-refused secret
  on disk until the next delete. Unlinking at once is the same outcome with less lingering.

<a id="dec-2321"></a>

## DEC-2321: a take destroys the version it took and keeps the code's metadata, so the version count never restarts

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent (Claude Code, lane V2), under DEC-79, at the coordinator's direction (option (a) of the review of #2117): it swaps one capability on the exchange's own code for two narrower ones on the same entry, and adds no reach |
| **Story or claim** | E21-32, claim [#1442](https://github.com/kunwarshivam/mandate/issues/1442); amends [DEC-2061](infrastructure.md#dec-2061) items 2 and 4 |

### Context

DEC-2061 item 4's take reads the code at version `v`, writes a tombstone with `cas = v`, then
deletes the code's metadata. On OpenBao 2.7.1 a metadata delete resets the entry's version count,
so the next code stored, by a re-connect, is version 1 again. A taker that read the first code's
version 1 and stalled before its overwrite then wins its `cas = 1` write against the new code: it
tombstones a code it never read, and the take that follows finds no code.

### Decision

1. **The take keeps the metadata.** It reads the code and its version `v`, writes the tombstone
   with `cas = v`, destroys version `v` (`{kv}/destroy/connections/x<hex>/code`,
   `{"versions": [v]}`), then soft-deletes the tombstone's version alone
   (`{kv}/delete/connections/x<hex>/code`, `{"versions": [v + 1]}`). The version count carries
   on, so a code stored later is `v + 2` or above, and a stalled `cas = v` write always loses. No
   version of a taken code stays readable: `v` is destroyed and `v + 1` is the tombstone, deleted.
   A code a re-connect stores between the tombstone and its delete is a later version, which the
   delete never names, so it is never hidden; a plain `DELETE` of the data path would delete
   whatever version is latest, that code included.
2. **The exchange's grant.** In its templated policy (DEC-2060 item 4), its code's data path has
   `read` and `update`, and its code's `delete` and `destroy` paths `update`, all on its own
   connection only. The `delete` on its code's metadata is removed. It never has `create` on its
   code, nor any `list` or metadata capability, and tests pin the exact policy and that no grant
   reaches another connection's data, delete or destroy path.
3. **A lost race.** A refused overwrite is read again, as before: no code left is a lost race,
   and anything else is a denial (DEC-2061 item 4).

### Residuals, disclosed

- Revocation deletes a connection's entries with their metadata (the connection manager's
  explicit delete), and that restarts the code's version count. A taker stalled across a revoke
  and a re-connect could then overwrite the new code at version 1. The connection manager stops
  the connection's exchange Job on revoke, before a re-connect can store a code, so no taker of
  the old connection is left to stall; and the worst outcome is a code that is not redeemed, so
  the connect fails and is retried, never a code redeemed twice.
- The exchange's delete and destroy of named versions of its own code could hide or destroy a
  version it never read, were it to name one. Both are on its own connection only, which it can already break by not
  redeeming the code, and neither lets it read or write another's.

### Rationale

The version count is what makes `cas` single use. Keeping it costs the exchange two capabilities
on the entry it already reads and overwrites, and removes the one that could restart the count.

### Alternatives considered

- **Keep the metadata delete and recheck after the overwrite.** Rejected: the stalled write
  already lands on the new code before any recheck.
- **Store each code at a fresh path per connect attempt.** Rejected: it changes the layout of
  DEC-2060 item 3 and every reader of it.

<a id="dec-2322"></a>

## DEC-2322: The token exchange's release build keeps debug assertions off, and the release check proves it

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, at the coordinator's review of #2109 and #2110; it adds a check and narrows nothing, so no safety rule is weakened) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E10-13 slice 5b-3a; [DEC-2633](process.md#dec-2633) item 2, whose release gate this makes explicit; [DEC-2629](process.md#dec-2629) item 3; [DEC-2100](DEC-2100.md) item 2 (the stub issuer's release check, which this follows) |

### Context

DEC-2633 item 2 says a release build, or a staging or production tier, refuses to start when
`MANDATE_TOKEN_URL` is set at all. The cell override's code is compiled only with debug assertions,
so that refusal holds only while the release build really has them off. Nothing said so, and
nothing checked the environment the release check runs in.

### Decision

1. **The image's build keeps the default release profile.** The token-exchange image is built with
   the release profile and no debug-assertions override. That means:
   - no `debug-assertions` in `Cargo.toml`'s or `.cargo/config.toml`'s release profiles (xtask's
     `release_builds_keep_debug_assertions_off` already pins those);
   - no `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS`;
   - no `-C debug-assertions` in `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS`.
2. **The release check proves it.** `cargo xtask ci exchange-release` refuses to run in an
   environment that sets any of those variables. It then runs the release binary three ways, and
   requires exit 2 with `token_exchange_token_url_in_release` alone on standard error each time:
   - a dev tier with a token URL its debug build admits;
   - a dev tier with an empty token URL;
   - a production tier with a token URL.
3. **The token URL is read raw.** The process reads `MANDATE_TOKEN_URL` as `std::env::var_os` does,
   so a value that is not UTF-8 is still a value set and refuses the start in every tier, never
   reads as unset.

### Rationale

The gate is only as strong as the build it rests on. Checking the environment as well as the binary
closes the one way a release build could carry the override's code without anyone editing a
manifest.

### Alternatives considered

- **Add these lines to DEC-2633.** DEC-2633 is Accepted on main, so only its Status line may
  change; the rule lives here.

<a id="dec-2323"></a>

## DEC-2323: The credentials fold reads `CredentialEnrolled` version 2 and `StepUpAsserted`

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering readings under DEC-79 and DEC-176 where journal spec v0.71 is silent; each takes the reading that adds no risk, and none weakens a rule). Supersedes [DEC-2121](DEC-2121.md) item 4's `AssertionUnread` |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane L1, claim [#759](https://github.com/kunwarshivam/mandate/issues/759)), with the coordinator's rulings on items 3 and 5 |
| **Story or claim** | E9-4 S3; claim #759; [journal spec](../../specs/journal.md) v0.71 §9.31, §9.32; [DEC-2108](DEC-2108.md) items 3 and 8; [DEC-2121](DEC-2121.md); [DEC-2122](DEC-2122.md) item 5 |

### Context

DEC-2122 item 5 requires the fold (`mandate_passkey::credentials`, DEC-2121) to read
`CredentialEnrolled` version 2 and `StepUpAsserted` before, or with, the first writer of either.
Until now the fold mapped version 1 only, and mapped a `StepUpAsserted` to an unread record that
step-up's view refused as `AssertionUnread`. §9.32's stream rule 254 is the writer's, and the
verifier's fold fails closed on it, but the spec does not say which members a breaking assertion
disables, or how the two views read assertions.

### Decision

1. **The flag.** `Enrolment::backup_eligible` is `Some(flag)` for a version 2 enrolment and `None`
   for a version 1 one. The verifier never counts `None` for step-up (DEC-2122 item 3).
2. **The assertion.** `CredentialRecord::Asserted` carries an `Assertion`: its `seq`, its member
   (the record's `actor.id`), credential, challenge and counter. It is mapped whole or not at all,
   refused at the member at fault.
3. **Rule 254 in step-up's view.** An assertion keeps the rule only when three things hold: its
   own member holds the credential, its counter is above the last kept one (or is 0 over a last
   of 0), and its challenge is one no kept assertion used. Otherwise it disables step-up for the
   asserting member and, when another member holds the credential, the holder too, from its `seq`
   on. This is DEC-2121 item 2's reading for rule 251, carried over.
4. **The counter.** `CredentialFold::last_count(credential)` is the greatest counter a kept
   assertion carried, else the enrolment's. It is `None` when the credential is not held, and
   `Broken` when its holder is disabled.
   Journal spec §9.32's rule 254 names the greatest counter of the credential's *earlier*
   assertions, breaking ones included, while this counts *kept* ones only. No step-up result
   differs: an assertion that breaks rule 254 disables its holder, and a disabled holder's every
   step-up is `step_up_missing` whatever its counter (#2180's review).
5. **Route 2's view reads no assertion.** `held` never reads a `StepUpAsserted`, kept or
   breaking, so no assertion can disable or change what route 2 answers (`AGENTS.md` rule 13).
   `step_up_reads_the_greatest_kept_counter_and_fails_closed_on_rule_254` pins it: on every
   rule-254 stream, breaking ones included, `held` equals the fold of the same stream with every
   assertion removed.
6. **The writer's check.** `admits` judges an assertion by rule 254, as it judges an enrolment or
   removal by rule 251. `AssertionUnread` is gone; it supersedes DEC-2121 item 4's.

`a_version_1_and_a_version_2_credential_keep_their_own_flag_and_counter` pins items 1 and 4 across
both versions on one stream.

### Rationale

The counter rule is what makes a cloned authenticator detectable. A fold that took an assertion's
counter without judging rule 254 would let a replayed or out-of-order assertion lower the bar. One
that ignored assertions would take the enrolment's counter for the last. Disabling both sides of a
breaking assertion only ever refuses a step-up. Route 2 opens nothing but pause and the kill
switch, so it must never be denied by a writer's fault.

### Alternatives considered

- **Disable only the asserting member.** Rejected: a breaking assertion of another member's
  credential would leave the holder's step-up counting a credential the stream calls into doubt.
- **Let route 2 skip a breaking assertion's members.** Rejected, against rule 13.

### Follow-ups

- The verifier slice: `stepup::EnrolledCredential` from a version 2 enrolment, its key artifact,
  and `last_count`.

<a id="dec-2325"></a>

## DEC-2325: the nextest run takes four `fast-test` shards, and its longest tests start first

| | |
|---|---|
| **Status** | Accepted; item 1's shard count superseded by [DEC-2639](process.md#dec-2639) (six shards) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-464, the number assigned by the coordinator: a reversible engineering decision that removes no test, check, baseline or gate and weakens no safety rule |
| **Story or claim** | DEC-464 follow-up; amends [DEC-871](earlier-600-899.md#dec-871) item 2's shard count; #2091's `fast-test (3/3)` |

### Context

On #2091, a change to `xtask/` selected the whole workspace. Its `fast-test (3/3)` took about five
minutes of setup and build, then 293 seconds of tests: 2,697 tests, all passing. The job hit the
ten-minute cap DEC-464 sets during post-job cleanup and was cancelled. The shard's last test,
xtask's `leak::tests::the_repository_passes_and_every_seeded_line_is_refused`, takes 104 seconds,
and nextest started it last, so it ran alone at the end.

### Decision

1. **Four shards.** `fast-test`'s matrix is `1/4` to `4/4`, nextest's `--partition=count:K/4`.
   Together the four run every test once, and each runs a quarter. Only `1/4` saves the shared
   cache. Everything else in DEC-871 is unchanged: the parts, the aggregate `fast`, and
   `MANDATE_TEST_PARTITION`'s check.
2. **The longest tests start first.** `xtask/nextest.toml` gives two of xtask's tests
   `priority = 100`: the leak test (about 104 s) and `tests::the_repository_resolves_no_live_build`
   (about 36 s). Each starts as soon as its shard begins and overlaps the rest. A priority changes
   only the order, never which tests run or in which partition. `cargo xtask` passes the file to
   every nextest run with `--config-file`, so it also applies to local `cargo xtask check` and the
   nightly, which run unpartitioned. It lives under `xtask/`, not at nextest's default
   `.config/nextest.toml`: a top-level `.config/` is a path DEC-1190's outside-path scan would read
   in every `.config` field access, such as `input.config` in `mandate-risk`. A bare
   `cargo nextest run` doesn't read the file, which only affects order.
3. **How it is pinned.** `the_fast_jobs_run_exactly_the_fast_group` already requires the matrix to
   be `1/N` to `N/N`, each once, and pins the cache's `save-if` on the first shard. It now pins
   `1/4`.

### Rationale

DEC-464 says to add parallel shards rather than drop tests. A fourth shard cuts each shard's test
time by a quarter, at the cost of one more runner, which builds the same cached test profile. The
priority override removes the tail that a single long test left at the end of one shard, with no
change to what runs. Measuring each test's time and balancing the shards by it would need a
recorded timing file kept up to date. Counting partitions and a priority on the known long tests
gets the shard well under the cap without one.

### Alternatives considered

- **Five or more shards.** Each extra shard repeats the build, which is about half of a shard's time,
  so the gain falls off. Four is enough for now, and a later decision can add more.
- **Making the leak test faster.** That is a separate change to xtask's leak check, and it doesn't
  help the next long test that lands. The priority override covers any test named in the file.

<a id="dec-2326"></a>

## DEC-2326: The model-gateway binary's interim start: a recorded bundle, caps from the environment, and HTTPS only with a registry file

| | |
|---|---|
| **Status** | Accepted (agent; interim, reversible engineering choices under DEC-79 that set no spending value, choose no live model and add no egress) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), on the coordinator's rulings for E21-62 c3b part 2 on [claim #2028](https://github.com/kunwarshivam/mandate/issues/2028) |
| **Story or claim** | E21-62; DEC-2257 items 2 and 7; DEC-2265 item 10; DEC-2272; DEC-79 |

### Context

`mandate-gatewayd` now has its library parts: the key read once, the start gate on both meters,
the serve loop, the daily-cap source, and the service holding each research call to that cap. What
is missing is the binary that puts them together. Three of its inputs are not this lane's to set:

- **Which model it serves live.** The registry entry is the founder's (DEC-2265 item 10, DEC-2257
  item 7), and it stays Proposed.
- **How much it may spend.** The workspace's and the deployment's monthly model budgets are
  spending values (DEC-79).
- **Whether it calls a provider at all.** `HttpsTransport` with the vault's key would spend real
  money.

This decision records how the binary starts until the founder decides those, holding the
conservative line on each.

### Decision

1. **The registry comes from a mounted file, and nothing ships one.** The binary loads its registry
   entry, prices and endpoint from a bundle file mounted into the pod. Without that file it refuses
   to start live. No chart value, image or test renders the file, so the live model stays the
   founder's to choose under DEC-2265 item 10.
2. **The caps come from required variables, with no defaults.** The binary reads
   `MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD` and `MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD` at start.
   It refuses to start, with each one's code, when either is unset, empty or not an amount `Usd6`
   holds (`mandate_gatewayd::config`). The chart leaves both unset until the founder names them.
   The daily research cap comes from the deployed mandate, as DEC-2272 says.
3. **Recorded first, HTTPS only behind item 1.** `--recorded <bundle>` runs the service over the
   recorded transport, and every test and the cell use that mode. The `HttpsTransport` path is
   wired, but the binary builds no HTTPS client unless item 1's registry file is present, and a
   test shows that without the file none is built. The vault's key is read only on that path.
4. **A cap read that fails for an agent deployed after start.** The service learns of an agent
   deployed after it started only from that agent's first call. If that first read fails, the agent
   holds no cap yet, so the meter refuses its research call `budget_exhausted`, not
   `meter_unavailable` as DEC-2272 item 5 says for an agent whose cap the service already held.
   Either way nothing is reserved or sent. Only the refusal's code differs.
5. **Zeroizing the provider key waits.** `ProviderKey` holds its key in a plain `String`, which is
   not wiped when dropped. Wiping it needs `secrecy` in `mandate-gateway`, a pure layer-1 crate
   that may not use it today, and no runtime test can observe the wipe. It is a backlog row under
   E21-62. Until then the key is redacted in every `Debug` and error (`HeldKey`, `ProviderKey`),
   which covers the logging that rule 7 forbids.

### Rationale

Each item takes the choice that adds no risk. The live model, the budgets and the decision to
spend all stay with the founder. Without the file and the variables, the service cannot spend at
all, and it says why at start rather than running on numbers no one named.

### Alternatives considered

- **Defaults for the caps.** Rejected: a default is a spending value no one confirmed.
- **No HTTPS path in the binary until the founder decides.** Rejected as unnecessary: gating it
  behind the absent file keeps it unreachable, and a test pins that, while the wiring is reviewed
  now rather than in a hurry later.
- **A registry entry compiled into the binary.** Rejected: that would choose the live model.

### Consequences

- When the founder names the model and the budgets, a later decision supersedes items 1 to 3. That
  decision mounts the registry file and sets the two variables.
- The chart's `model-gateway` process gets its arguments and probe in the binary's PR. It renders
  only once its image is built.

<a id="dec-2327"></a>

## DEC-2327: The cell's recorded plane serves `news.search` and `news.get`, matching terms inside the plane as data-plane spec §4.6 says

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79; recorded fixtures only); the coordinator approved the plan. Item 5 is corrected by [DEC-2328](trading-core.md#dec-2328) item 6 |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-16, slice S1 (news); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2270 and DEC-2271, whose bench found the gap |

### Context

The cell's recorded plane implements `NewsReader::news`, the seed's retrieval. It keeps the
trait's defaults for `search` and `news_text`, which answer `unavailable`. So an agentic run in
the cell is offered `news.search` and `news.get` but reads nothing through them. DEC-2270's canary
scan asks both reads for every canary by name, but the plane answers nothing, so the scan has
nothing to judge.

### Decision

1. **`search`** answers the items the recording holds, at their ages before the cut-off, as
   `news` does. It keeps only items for which every term is held, case-folded, as a whole word or
   phrase, in the stored headline or in the stored summary (data-plane spec §4.6):
   - a term must be bounded at each end by the text's edge or by a character that is not an ASCII
     letter or digit;
   - a term with spaces matches as a phrase, and only inside one field: a phrase across the end of
     the headline and the start of the summary is no match, since the two are separate fields;
   - a term may hold the hyphens and periods §4.6 admits, and they must match as written;
   - no terms narrow nothing.

   Terms are matched in the plane and sent nowhere (HI-28). The dispatch still filters the answer
   by the run's scope, window, allowlist, licensing and limit.
2. **`news_text`** answers one recorded item, at its age before the cut-off. Its stored text is
   its headline and summary, joined by a blank line: the text the store holds for it, which
   §4.6's matching reads. It answers `unavailable` in two cases:
   - for an item the recording does not hold;
   - for an allowlist version other than the one the recording was made under. The plane now
     carries that version.

   The dispatch still refuses an answer past the cut-off, off the run's allowlist, or licensed.
3. **No new member in any recording.** Every existing bundle loads unchanged, the demo's included,
   and a test says so.
4. **Tests first** (DEC-77):
   - the term match against an oracle with its own scanner, over named cases and over every word,
     pair of words, and adjacent two-word phrase the recording holds;
   - the get's text and stamp;
   - the get's refusals.

   They fail on the plane's `unavailable` rather than at a stub. A panicking stub would break the
   bench and injection tests, which already ask these reads. So each has a behaviour-only row
   until the implementation lands.
5. **The bench now judges what these reads return.** Once implemented, DEC-2270's injection scan
   and DEC-2271's bench run with `news.search` and `news.get` answering. Their canary scans must
   still find nothing, and their seeded breaches must still be caught.

### Rationale

§4.6 defines the match, so the recorded plane follows it rather than inventing a looser one. A
recorded get answers only the version it was recorded under, the conservative reading of "as of
the allowlist version". The dispatch's own scope checks stay as the second line, so the plane
adds a reader without removing a check.

### Alternatives considered

- **An optional `text` member in news.json:** no recording needs text beyond what the store
  holds, which §4.6 says is the headline and summary. A member can come with the first recording
  that needs one.
- **Substring matching:** looser than §4.6. A term would match part of a word, and the bench
  would judge reads no real data service gives.

<a id="dec-2328"></a>

## DEC-2328: The cell's recorded plane serves `filings.search` and `filings.get`, with a filing's full text an optional recording member

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79; recorded fixtures only); the coordinator approved the plan |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-16, slice S2 (filings); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2327 |

### Context

The recorded plane answers `filings`, the seed's retrieval. It keeps the trait's defaults for
`search` and `filing_text`, which answer `unavailable`. A recording holds each filing's excerpt
and no full text. Data-plane spec §4.6 gives `filings.get` two distinct answers:
- `out_of_range` when the offset is past the end;
- `excerpt_only` when the store holds only an excerpt.

Agent harness spec §6.10 says a range is "cut on UTF-8 boundaries".

### Decision

1. **A recording may give a filing its full text** as an optional `text` member of its
   filings.json row. Absent, the store holds only the excerpt. Present, it must be non-empty
   text, or the bundle is refused `Malformed { member: "text" }`. Every existing bundle still
   loads unchanged.
2. **`search`** answers the filings of an asked form for which every term is in the form or in the
   excerpt, as §4.6 says, with DEC-2327's whole-word, case-folded match. A phrase must sit inside
   one field, never across the form and the excerpt. Each filing comes at its ages before
   the cut-off, with `stored_bytes`: the byte length of its full text when recorded, else of its
   excerpt. The dispatch still filters by scope, window, allowlist, licensing, form and limit.
3. **`filing_text`** answers at most `length` bytes of a filing's full text from byte `offset`:
   - **The end** is cut back to the nearest character boundary, so no character is split.
   - **`offset` equal to the length** answers empty text.
   - **`offset` past the end** answers `out_of_range`.
   - **A filing recorded without text** answers `excerpt_only`.
   - **An offset inside a character** answers `unavailable`. The dispatch refuses an answer at any
     offset other than the one it asked, so a range cannot start anywhere else.
   - **An item the recording lacks, or another allowlist version,** answers `unavailable`, as
     DEC-2327's `news_text` does.
4. **Tests first** (DEC-77):
   - the search against an oracle with its own scanner;
   - every offset of a text holding two-, three- and four-byte characters, with several lengths,
     against the oracle's own slicing;
   - the refusals;
   - the optional member.

   Each fails on the plane's present answer rather than at a stub, with a behaviour-only row, as
   in DEC-2327.
5. **The implementation PR extends DEC-2270's adversarial fixture** with filing text that carries
   injections, and off-scope filing text that carries canaries. The bench's scans must still find
   no canary, and their seeded breaches must still be caught.
6. **A correction to DEC-2327 item 5: the bench now runs well-formed reads.** That item said
   DEC-2271's bench judges what the news reads return once they answer. It did not. The
   compromised models' calls rarely form a valid search, and no bench trial's tool result held
   injected text. DEC-2270's injection test was what judged the real reads. So the bench gains a
   trial whose model makes only well-formed reads, scripted:
   - searches the injected news and filings answer;
   - a get for an injected item;
   - a get and a search for each canary by name.

   It runs in the runtime process under every oracle. It asserts that the injected news reaches
   the model and that nothing is breached; this passes on S1. Its filings twin asserts that the
   injected filings reach the model too. That twin is pending, with a behaviour-only row, until
   this slice's implementation lands.
7. **A term's edge spaces match as written, so they only narrow.** The dispatch's argument check
   (§6.10, DEC-2213 item 1) admits a term with a leading or trailing space. The plane matches its
   spaces literally, still bounded by characters that are not letters or digits. So `" apple"`
   matches only where the stored text holds a space before "apple" with no letter or digit
   before that space, as in ", apple". Wherever such a term matches, the term without its edge
   spaces matches too, so it never keeps an item the bare word would drop. It fails closed.

### Rationale

An optional member keeps every recording valid and lets a bundle hold full text only where a test
needs one. Cutting the end back, never the start, is the reading that cannot answer bytes the
model did not ask for. An offset inside a character has no whole-character answer at that offset,
so the plane answers nothing rather than a range at another offset.

### Alternatives considered

- **Treat the excerpt as the stored text:** `excerpt_only` would never be answered, and §4.6 names
  it.
- **Snap a mid-character offset forward:** the answer would start at an offset the model did not
  ask for, which the dispatch already refuses.

<a id="dec-2332"></a>

## DEC-2332: The token exchange's account kept to the one the manager makes, and its pod's environment, CA and launch pinned

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering decisions under DEC-79 and DEC-176; each narrows what the deployment manager may run or leave behind, and none touches an order, a limit, or an exit) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane D1, the staging push), under DEC-79 and DEC-176; number assigned by the coordinator |
| **Story or claim** | DEC-2627 items 1 and 5; E21-33; amends DEC-1177 (items 4, 11 and 12); the security review of #2169 (findings F1, F2) and residual R1 |

### Context

DEC-1177 has the Kubernetes adapter make a token exchange's ServiceAccount before its pod and delete
it before the pod at a stop, and has the manager stop every listed instance once the request
closes. The review of #2169 found two gaps: an account whose pod the API server refused stays
behind, and an account of the exchange's name that already exists is used whatever it holds. The
review of #2077 left one residual, R1: the pod policy pins no environment, so a pod could set
another token host or vault address. The exchange also needs the vault's address and CA.

### Decision

1. **An account the manager did not make is never used.** When creating the exchange's account
   finds one of its name (409), the adapter reads it and goes on only if it is what the manager
   makes: its manager, workspace and process labels, `automountServiceAccountToken: false`, and no
   Secret or pull secret. Otherwise, or if it cannot be read, no pod is asked for.
2. **A refused pod takes its new account with it.** When the API server refuses the exchange's pod
   outright (a client error other than a conflict or a rate limit), the adapter deletes the account
   it made for it. A server error or a lost answer may have made the pod, so its account is kept
   for it, and point 3 deletes it at the close.
3. **A closed request's account is swept.** Once the request closes, the manager stops the last
   generation it started even when no instance of it is listed, and asks again until it has seen
   one stop carried out; a restarted manager asks once more, since a stop is idempotent. So no
   exchange's account outlives its request, whether or not its pod ever ran. A refused pod deletes an account only its own start made, never one that was there
   before. The sweep answers the request's closing record, which the connection manager's teardown
   journals at the deadline (connections spec §5.2 step 6); the deadline alone stops nothing, and
   a connection id is one connect's alone (journal spec rule 131), so a closed one never starts
   again.
4. **Each managed container's environment is pinned** (delivered by the chart change that follows #2178, R1; until it lands, the pod policy pins no environment). The pod policy admits a container of a pod
   the manager makes only with exactly its template's entries, names and values, and the
   manager's own names (`MANDATE_WORKSPACE`, `MANDATE_PROCESS`, `MANDATE_SUBJECT`,
   `MANDATE_GENERATION`, and for a token exchange `MANDATE_SUBJECT_ACCOUNT`) exactly once each; an
   init container gets none of the manager's. So no pod can name another token host, tier or
   vault address through its environment.
5. **The vault's CA alone** (delivered by R1; until it lands, no pod mounts it). A process that reaches the vault mounts the vault chart's CA
   certificate, the ConfigMap `vault-ca`'s `ca.crt` only, mode 0444, read-only at
   `/run/mandate/vault`: the token exchange always, the executor when its broker is
   `alpaca-paper`. The pod policy admits exactly that ConfigMap volume for those processes: not
   optional, one item, no item mode. No key material is ever mounted.
6. **The exchange's launch** (delivered by R1; until it lands, the exchange's template takes no argument). `/mandate-token-exchange run --vault-address <url> --vault-dir
   /run/mandate/vault`, exactly, which its binary requires: the URL is the executor's
   `vaultAddress` value, or else this namespace's vault Service, `https://vault.<namespace>.svc:8200`.

### Rationale

Each point closes a way a careless or compromised manager, or a stale object, could give the one
process that reaches the live token host another account, a key, or another destination. Reading
an existing account back costs one request; deleting a refused pod's account, and sweeping at the
close, keep the namespace holding only accounts an open request needs.

### Alternatives considered

- **Delete any existing account of the exchange's name and make it again.** Rejected: it could
  delete the account a running exchange is logged in as.
- **Sweep every account with no pod.** Rejected for now: the manager reads the journal, not the
  namespace's accounts, and point 3 reaches the same end from its records.
- **Pin only the environment's names.** Rejected: a value such as the tier is as much a knob as a
  name.

<a id="dec-2334"></a>

## DEC-2334: `cargo xtask ci full`'s parts run in two CI jobs, `full-checks` and `full-checks-2`

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79 and DEC-464, the number assigned by the coordinator: a reversible engineering decision that removes no test, check, baseline or gate and weakens no safety rule |
| **Story or claim** | DEC-464 follow-up; #2142's `full-checks` at 72a6297e |

### Context

On #2142, a change that selects everything, `full-checks` ran all of `cargo xtask ci full` in one
job. It started at 04:26:08, reached `ci postgres` at 04:31:42, `ci openbao` at 04:34:38,
`ci objectstore` at 04:35:13, `ci tofu` at 04:35:24, `ci stub-release` at 04:35:51 and
`ci exchange-release` at 04:36:19. The ten-minute cap (DEC-464) cancelled it three seconds later.
Each part passed. `exchange-release` (#2110) had just been added, and main's full selection runs
the same parts.

### Decision

1. **Two jobs, each part once.**
   - `full-checks` keeps the parts that need one of its services (PostgreSQL, OpenBao, the S3
     store), and `refcases`, whose build the Postgres tests reuse: `refcases`, `postgres`,
     `openbao`, `objectstore`, each as its own step.
   - `full-checks-2` runs the parts that need none of them, on a runner of its own: `reference`,
     `schemas`, `supply-chain`, `tofu`, `stub-release`, `exchange-release`. cargo-deny, gitleaks,
     OpenTofu, tflint and `MANDATE_TOFU_REQUIRED` move with the parts that use them.
   - A first split, which kept `reference`, `schemas` and `supply-chain` in `full-checks`, ran it
     in 9 min 57 s on a full selection (#2195 at 8ead7513), with `full-checks-2` at 1 min 12 s.
     Moving those three parts balances the two jobs.
2. **The same selection.** `full-checks-2` needs `select` and starts on
   `needs.select.outputs.full_checks`, the switch `full-checks` starts on, so it runs exactly when
   `full-checks` does.
3. **`full` needs both.** The required `full` verdict passes only when each of them succeeded where
   selected and was skipped where not, beside a `select` that succeeded.
4. **Locally, nothing changes.** `cargo xtask ci full` and `cargo xtask check` still run every part,
   unsplit.
5. **Pinned.**
   - `the_full_jobs_run_exactly_the_full_group`: the two jobs' parts are `FULL_JOB`'s, each once and
     in order. No other job runs one. Each job is capped at ten minutes, needs `select`, starts on
     the switch, and is needed by `full`. Removing `full-checks-2` from `full`'s needs, or running
     a part twice, fails it.
   - `full_accepts_a_skipped_matrix_only_beside_a_plan_of_zero` runs `full`'s script over
     `full-checks-2`'s results too.
   - The checkout count, the guards, the select gates, and the database and S3 gates (now on
     `ci postgres` and `ci objectstore`) follow.

### Rationale

DEC-464 says to add parallel jobs rather than drop checks. The split follows the services: the
parts that need them stay with the job that starts them, so no service is started twice. The other
parts need only the toolchain, uv and their own tools, so they share a runner of their own. Per-job
timings on a full selection are recorded in the PR that lands this.

### Alternatives considered

- **Raising the cap for `full-checks`.** DEC-464 forbids it.
- **A matrix over `full`'s parts.** Each shard would rebuild and restart services. Two fixed jobs
  match how the parts group by what they need.

<a id="dec-2336"></a>

## DEC-2336: How the deployment manager reads an exchange's success, a connect's executor's lifetime, and the account each executor runs as

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering decisions under DEC-79 and DEC-176, items 1 to 5 accepted by the coordinator on 2026-10-22 for K2a and items 6 to 10 for K2b; each fails safe: it starts an executor only on a journaled success, or stops one only when nothing wants it, and none touches an order, a limit, or an exit) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane D1, the staging push), under DEC-79 and DEC-176; number assigned by the coordinator |
| **Story or claim** | E21-33 (K2a, K2b); DEC-2335; DEC-2060 items 2, 7 and 9; DEC-1177 items 5 and 6; DEC-2627 item 5; journal spec §9.21 v0.74, rules 167 and 257; connections spec §5.2 steps 4 to 6, CN-5, CN-12; claim #1901 |

### Context

DEC-2335 has the deployment manager start an OAuth connect's executor, journaled first with cause
`exchanged`, when the connect's token exchange exits successfully while its request is open, and
want it until the connection is refused or revoked. This decision fixes how the manager reads that
success, which account it starts, when it declines to, how it picks a stop's cause, and what a kill
switch reaches before establishment. K2a is the manager's part; K2b brings the executor's account,
vault volume and admission pin, and the Kubernetes adapter's reading of a pod's success.

### Decision

1. **Success is the orchestrator's `Succeeded`.** The orchestrator port gains `Phase::Succeeded`
   beside `Exited`: an instance that exited with success, never restarted and never live. A token
   exchange's pod that succeeded (exit 0) is the exchange's success; a failure, a deadline passed
   (`activeDeadlineSeconds` fails the pod), or a pod gone starts nothing. The manager's garbage
   collection and stops treat it as an exited instance.
2. **`exchanged` only at generation 1.** The manager journals the start only while the request is
   open, its `exchanged` start not yet journaled, and the manager holds no executor slot for the
   account (no start, stop or crash loop of it read or journaled), since rule 167 holds
   `exchanged` to generation 1. Two connects of one account that succeed in the same pass journal
   one start and ask the orchestrator for it once. An account whose executor already ran, as on a
   connections-spec CN-12 reconnect that reuses its `account_ref`, gets no start: the connect's
   checks never run and the connect fails by its deadline, safe until a reconnect start is
   specified (backlog, E21-33).
3. **The request names the account.** The manager reads the account from `ConnectionRequested`
   version 2's `account_ref`. The executor is wanted while an agent of its account is deployed and
   not stopped, or while an exchanged request of that account has seen neither a
   `ConnectionRefused` with occasion `connect` nor a `ConnectionRevoked`. A version-1
   `ConnectionEstablished` closes the request but not the connection.
4. **The stop's cause is the last reason taken away.** The manager journals an executor's stop with
   the cause of the last record that took a reason to want it away: `agent_stopped` for an
   `AgentStopped` of an agent running on the account, `connection_closed` for a refusal or a
   revocation of a connection whose `exchanged` start is journaled. This is §9.21's reading of the
   two causes: the last deployed agent stopped, or the connection closed with no deployed agent.
5. **A connection's kill switch reaches its connecting executor.** A kill switch scoped to a
   connection covers the executor of the account its request names as well as the one its
   establishment names, so it reaches the connect's executor before establishment, the reason the
   coordinator chose DEC-2335's lifetime (a).
6. **The manager names each executor's connection** (K2b). The orchestrator port's start takes a
   `Launch` of the token exchange's lifetime and the executor's connection, and its stop the
   connection. An executor's start names the connection a running agent's deployment of its
   account names, else the connect whose `exchanged` start is journaled and not ended, else none,
   and then it is not started; a revoked connection no deployment names is never chosen. The
   manager reads it when it folds each generation's start, from the journal alone, so a restarted
   manager names the same one, and that generation's stop names it too, so the stop deletes the
   account its start made. A runtime and a token exchange name none.
7. **An executor runs as its connection's own account** (DEC-2060 items 2 and 9). The Kubernetes
   adapter names it by the connection id's bytes in lowercase hex, makes it before the pod and
   deletes it, then the pod, at a journaled stop, as it does the token exchange's; a crash and a
   restart keep it. Its pod names it as `serviceAccountName` and as `MANDATE_SUBJECT_ACCOUNT`, and
   carries `MANDATE_WORKSPACE_NAMESPACE`, the namespace its vault token is bound to. An executor
   whose connection the manager cannot name is not started.
8. **Admission pins the form, the executor checks the connection.** The deploy-manager's pod
   policy holds a manager-started executor pod to an even-length lowercase hex account that its
   environment names once, and its accounts policy admits that form with the executor's label and
   the `x` form with the token exchange's, each refused under the other's label. CEL cannot hex a
   connection id, so, as DEC-1177 item 6 has the exchange do, `mandate-executord` checks that the
   account is its connection's (lane V2; Risks).
9. **The vault token only on a pod the manager starts.** A `vault_token` process mounts its
   projected vault token only as a template the manager starts, the executor's only where its
   broker reaches the vault (`alpaca-paper`). The chart's Deployment executor, which renders when the
   manager does not, runs as `default`, which no vault role names, and mounts none; it keeps the
   vault CA, which is public.
10. **A succeeded pod is `Succeeded`.** The adapter reads a pod's phase `Succeeded` as
    `Phase::Succeeded` and `Failed` as `Exited`, being deleted or not, so item 1 holds in the cell.

### Risks

- **A reconnect that reuses its `account_ref` fails by its deadline** (item 2). It reaches the
  staging reconnect path; it is a backlog bullet under E21-33, to be specified before reconnects
  are offered.
- Until K2b lands, the Kubernetes adapter reports a succeeded pod as `Exited`, so no connect's
  executor starts in the cell: safe, and the connect fails by its deadline.
- **K2b depends on an executor self-check it does not hold.** CEL cannot hex an executor's subject,
  so, as DEC-1177 item 6 has the token exchange do, `mandate-executord` must refuse to start unless
  `MANDATE_SUBJECT_ACCOUNT` is the hex of its connection id. The coordinator gave it to lane V2,
  beside #2196's namespace check; until it lands, admission still pins the account's name form.
- **The chart's Deployment executor stays on `default`** until the manager owns every executor;
  K2b pins only manager-started executor pods to their `<hex>` account and keeps the vault-token
  volume off every other pod, so `default` never logs in to the vault.

### Rationale

Each item makes the executor's start depend on a journaled fact and its stop on nothing wanting
it, so a restart of the manager rebuilds both from the journal, and every unclear case starts
nothing.

### Alternatives considered

- **Read success from a record the exchange journals.** Rejected: the exchange journals nothing
  (journal spec §9.21, DEC-1175), and the orchestrator's exit status is what the manager already
  reads.
- **Start a reconnect's executor at the next generation with `exchanged`.** Rejected here: rule 167
  refuses it; it needs a journal spec change of its own.
- **Name the stop's cause by what wants the executor at the stop.** Rejected: at the stop nothing
  wants it, so only the order of the records tells the two causes apart.

<a id="dec-2340"></a>

## DEC-2340: The vault's CA is read as a public file, which refuses only a foreign write

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176; a CA certificate is public, so reading one that others may read loosens nothing that protects a secret, and every secret file keeps its owner-only rule) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane V2, the vault), under DEC-79; number assigned by the coordinator |
| **Story or claim** | E21-32 (claim #1442); amends DEC-2630's mounted reader; found by lane D1 against #2198 (R1, DEC-2332 item 5) |

### Context

DEC-2630's mounted reader, `mandate_vault::mounted::read_mounted`, refuses a file with any group
or other permission bit (`0o077`), the right rule for an AppRole role id or secret id. The
executor read the vault's CA, `ca.crt`, through the same reader. But #2198 mounts the vault chart's
ConfigMap `vault-ca` at 0444, and under an `fsGroup` a mount cannot go below 0440, so an
`alpaca-paper` executor on staging would fail `vault_ca_unreadable` before its login. No test ran
the reader against the mode the chart renders.

### Decision

1. **A CA has a reader of its own.** `read_mounted_ca(dir)` reads `dir/ca.crt` whole by the
   mounted reader's rules (one name inside the mount, the kubelet's links followed only inside it,
   a plain file judged on the opened handle, at most 65536 bytes), but refuses only a group or
   other write bit (`0o022`), as the projected ServiceAccount token's reader already does. 0444,
   0440 and owner-only modes are read.
2. **Only `ca.crt` is read that way.** The reader takes no file name: it reads `CA_FILE`,
   `ca.crt`, and nothing else. A role id, a secret id, a key, or any other file keeps
   `read_mounted`'s owner-only rule. The files a public reader may read are exactly the vault's CA
   (`read_mounted_ca`) and the projected token and namespace (`read_service_account_token`,
   `read_service_account_namespace`, DEC-2060).
3. **Every CA the vault's clients read goes through it.** `read_mounted_ca` is public in
   `mandate-vault` so that every client calls the one reader. Today that is the executor's
   `vault_config`. The token exchange reads no CA yet; when lane X adds its vault client (5b-3b),
   it reads the same mount (#2198) with `read_mounted_ca`, never `read_mounted(dir, "ca.crt")`.
4. **The chart's mode is a test.** A `mandate-vault` test reads the workspace chart's `vault-ca`
   volume's `defaultMode` from `deploy/cell/charts/workspace/templates/_helpers.tpl`, and reads a
   CA at that mode, and at that mode with the group read bit an `fsGroup` adds. Because the test
   names the chart file, a chart change selects the crate's tests (DEC-1190), so a chart mode the
   reader refuses fails in CI, not on a cluster.
5. **An AppRole Secret under an `fsGroup` is copied, never read in place.** Every workspace pod sets
   `fsGroup`, so the kubelet makes a 0400 Secret file 0440, which `read_mounted` refuses, as it
   must. An AppRole Secret is copied by an init container to a memory-backed directory, owner-only,
   as the journal role's pgpass already is, and read there (lane D1, #2227). A test pins it:
   `read_mounted_credentials` reads 0400 and 0600 and refuses 0440.

### Why this loosens nothing that protects a secret

A CA certificate is public: the vault chart already publishes it in the ConfigMap `vault-ca` for
every client. What a client needs from it is integrity: no one but the file's owner may
change which CA the client trusts. A group or other write bit is still refused, and so is a link
out of the mount, a swapped file, and a pipe. No secret's reader changes.

### Consequences

- An executor reads the CA #2198 mounts, at 0444 or 0440.
- A reviewer can find every public-file read from the three functions named in item 2.

<a id="dec-2341"></a>

## DEC-2341: The executor opens no risk until its account stream holds its establishment's copy, and writes that copy itself

| | |
|---|---|
| **Status** | Accepted (agent: a tightening under DEC-79 and DEC-176; it refuses openings the executor admitted before, blocks no risk reduction, and changes no limit, order or exit) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane V2), under DEC-79 and DEC-176; number assigned by the coordinator |
| **Story or claim** | E21-32 (claim #1442), E2b of DEC-2339's exchanged executor; journal spec §9.8 and rule 68 |

### Context

Journal spec rule 68 keeps an account stream `connecting` until it holds the copy of its
`ConnectionEstablished`, and while connecting the executor may only run and record the connect
sequence's checks. No code wrote that copy: `mandate-executord` served as soon as its account
stream's latest `ConnectionChecked` passed. On the API-key path the deployment manager starts an
executor only after the control stream holds `ConnectionEstablished` version 2 (DEC-1171, D1's
manager fold), so nothing traded before an establishment existed, but the account stream never
held its copy. On the OAuth path (DEC-2335, DEC-2336, DEC-2339) the manager starts the executor
before any check, so serving on a passed check alone would let it trade before the control
services established anything.

### Decision

1. **The establishment state.** From its account stream and the control stream, the executor reads
   one of four states for its connection, the `connection_id` of its stream's latest passed
   `connect` check:
   - **established**: the account stream holds a `ConnectionEstablished` copy;
   - **to copy**: the control stream holds a version-2 `ConnectionEstablished` whose
     `causation_id` is that check, with the same `connection_id` and this stream's `account_ref`;
   - **closed**: the control stream holds a `ConnectionRefused` (occasion `connect`) or a
     `ConnectionRevoked` for that connection after the check;
   - **connecting**: none of these.
   A record it must read and cannot is never skipped (DEC-85): the start is refused.
2. **The copy.** In state *to copy*, the executor appends the copy on its account stream: the
   original's members, `risk_clock` (the copy's time on its whole second, §4.7), and the original
   as `causation_id`, as §9's table and rules 61 and 68 say. The journal's schema for the account
   stream's `ConnectionEstablished` judges it, and `mandate-executor`'s fold reads it as a record
   only, beside `ConnectionChecked`: it changes no account state (a test pins the state equal to a
   record-only event's), and its cause is the writer's, as the copy's own test pins. It is not in
   the fold's `COPIED` list, so the fold keeps no `copied` entry for it; journal rules 61 and 63
   hold its cause. E7-3's later slice interprets it if it ever needs to; until then it is a
   record.
3. **Connecting opens no risk, and never blocks risk reduction** (`AGENTS.md` rule 13). While
   *connecting*, the executor runs, and the binding gate gives no input to any request whose
   purpose adds risk, so every opening is refused as it is today without a binding-gate fact.
   A purpose that does not add risk (a kill switch's flatten, a protective order, a risk, owner or
   discretionary exit) is passed to the wiring's own source unchanged, and `binding_checks` admits
   such a proposal even without an input, so all of them run as ever. The executor's own
   protective orders never ask the binding gate at all: only a received intent does, so the gate
   cannot reach them. The executor re-reads the state each pass and leaves *connecting* by the copy.
   On the OAuth path the executor does not run while connecting: DEC-2346 holds it, before its
   process starts, until the stream is established, due its copy, or closed.
4. **Closed, or unreadable, never stops the executor itself** (`AGENTS.md` rules 3 and 13). In
   state *closed* the gate stays shut, and the executor goes on running: every kill switch,
   protective order and exit of what the account holds still goes, and no opening. A connection
   record it cannot read is ambiguity, and its safe default is the same: the pass reports
   `connection_record_unreadable`, opens nothing, writes no copy, and stops nothing. The executor
   never drains itself on the gate's word, since a drain takes no further input and so would
   abandon the exits it still owes. Its stop is the deployment manager's: `connection_closed`
   once the connection is refused or revoked, unless a deployed agent on the account still wants
   it (DEC-2335), by the same SIGTERM drain as any stop, with a kill switch covering it applied
   first (DEC-2336 item 5).
5. **The upgrade case.** An API-key executor that served before this decision has no copy on its
   stream, but the control stream already holds its establishment (the manager started it only
   after one). On its first start it reads *to copy*, writes the copy, and serves: it never waits
   and never stops serving risk reduction. A test pins it.
6. **Every route ends.** A passed `connect` check is followed by an establishment or a refusal: on
   the API-key path the establishment comes first, and on the OAuth path the API's teardown (§5.2
   step 6) refuses a connect past its deadline. A test pins that an executor left connecting with
   neither opens no risk, however long it waits.

### Consequences

- An account stream's first trading record follows its establishment's copy, as rule 68 says.
- `checked_connection` still names the connection the executor leases; the state of item 1 gates
  what it may open.
- An opening refused for the shut gate is held, not refused: the executor asks it again every pass
  (`held`), so once the copy opens the gate it goes, as any opening waiting on its binding input.
- Two DEC-77 pairs deliver it. E2b-1: the state of item 1, the copy's draft, the gate of item 3
  and the fold of item 2, with the rule-13 test that every exit goes under a shut gate. E2b-2: the
  pass that writes the copy and opens the gate, and its place in the executor's loop, with items 4
  to 6's tests.

<a id="dec-2345"></a>

## DEC-2345: The executor asks the vault for its account's fingerprint at connect, and the connection manager reads only the hash

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176; it adds no reader of the account id, the key never leaves the vault, and the connection manager sees only the hash, so it is no looser than connections spec §3.1) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane V2, the vault), under DEC-79; number assigned by the coordinator |
| **Story or claim** | E21-32 (claim #1442), F3 of DEC-2339's exchanged connect; connections spec §3.1, §5.2 step 5, check 4; DEC-2339; lane X's connect finish (DEC-2342) |

### Context

Connections spec §3.1 keeps the fingerprint key in the vault: the connection manager asks the
vault for the keyed hash of an account id and receives only the result, and the key is never
stored beside the record. §5.2 step 5 has the manager compute the fingerprint from the reference
the executor journaled, and check 4 refuses a connect whose account another connection holds.
DEC-2339 keeps the account id in an entry that the executor creates and no identity reads. OpenBao
cannot hash a value held at another of its paths, so if the manager asked, some process would have
to read the id: either the manager's own, which would loosen DEC-2339, or a new service.

### Decision

1. **The key.** An OpenBao transit key, `account-fingerprint` (`fingerprint::FINGERPRINT_KEY`), of
   type HMAC-SHA256, never exportable, on the cell's transit mount (`Mounts::transit`, `transit`).
   Provisioning enables the mount and creates the key. The key never leaves OpenBao.
2. **The executor asks, at connect.** The executor holds the account id in memory for its connect
   check already (DEC-2339). In the same step it asks transit for the HMAC of the id
   (`transit/hmac/account-fingerprint`) and creates
   `personal/fingerprints/x<hex connection>/<account_ref>` once, holding the fingerprint and the
   `pii_` reference of its account entry: one entry per request, beside the account entry, as
   DEC-2339 item 1 keys that one, so a request again on the same connection to another account
   stream (DEC-699) has its own and nothing is overwritten. Its templated policy gains exactly two
   grants: create on `personal/fingerprints/x{its own name}/+`, the same deliberate one-segment
   exception DEC-2339 item 1 names for its account entries, below its own connection and create
   only, and update on that one transit path. It has no other transit path: it cannot read, export or rotate the key, or verify a hash
   against it. A compromised executor could hash a wrong id, the same risk DEC-2339 already
   accepts for a wrong id in its entry, and it never learns which connection holds which account,
   since the registry is the manager's alone. This is §3.1's "the manager asks the vault" moved to
   the executor at connect: the connections spec's sentence is amended to say so (ES-22).
3. **The manager reads the result.** The connection manager reads each request's fingerprint
   entry and nothing of the account entry. No policy can name an entry before its request
   exists, and nothing rewrites a policy when a request is made, so its policy reads them by
   DEC-2349's prefix grant, `personal/fingerprints/+/+`, read only: the entries are pseudonymous
   hashes, and the manager already reads every hash through the registry. The registry and the
   marker are fixed paths of its generated policy, spelled out. It gets the hash and the reference, and must find the
   reference equal to the `ConnectionChecked`'s `account_pii_ref` before it uses the hash. A
   mismatch, a missing entry or a rotating key answers `cannot_answer`, and the connect fails
   closed (lane X).
4. **The registry is one entry, changed by check-and-set.** `personal/claims` maps each claimed
   fingerprint to the connection that holds it, and only the connection manager reads or changes
   it. A claim reads the entry and writes it back with the one more pair under KV v2's
   check-and-set on the version it read, so of two claims of one fingerprint one wins, and one
   asked again for the same connection answers `Claimed`. A release writes it back without the
   connection's pair, under the same check-and-set, and is idempotent; a release by a connection
   that does not hold the fingerprint changes nothing. The connection manager is the registry's
   only writer. Two concurrent claims of one fingerprint contend on the version: the one whose
   write lands first wins, and the other's check-and-set fails, so it reads the entry again,
   retrying a bounded number of times, and answers `HeldBy` the winner. One entry keeps every policy written out
   path by path with no wildcard: a path per fingerprint would need one, since no policy can name
   a hash before it exists.
5. **Personal data.** A fingerprint is pseudonymous personal data. The fingerprint entry and the
   registry are kept in the personal-data vault and follow DEC-2339's retention and erasure:
   E5-5 erases a connection's fingerprint entry and its pair in the registry with its account
   entry. A claim is released when the connect is refused and when the connection is revoked, and
   kept while the connection is established.
6. **Rotation.** While the marker `fingerprints/rotating` exists, the manager refuses every new
   connect, as §3.1 says (`FingerprintError::Rotating`). The marker's read and the claim's write
   are not one atomic step: a rotation that starts between them lets that one claim, computed
   under the old key, land. The rotation job therefore sets the marker, then recomputes every
   claim in the registry, the late one among them, before it clears it. The job that recomputes every fingerprint
   under a new key from the account entries, under an operator identity of its own, is a backlog
   row, not this decision.
7. **The slices.** F3a: the paths and the grants. F3b: the key's provisioning and the vault's
   operations (the executor's hash and entry, the manager's fingerprint read, claim and release). F3c: the
   executor writes its fingerprint in the connect check, any refusal journaling the account as
   unreadable, as DEC-2339 item 6 fails a refused create closed.

### Consequences

- Check 4 runs over hashes only: no process but the executor ever holds an account id, and no
  process but OpenBao the key.
- The manager's policy gains read on each request's fingerprint entry, read and change on the
  registry, and read on the marker; the executor's, the two grants of item 2.

<a id="dec-2346"></a>

## DEC-2346: An executor whose connection is still connecting waits before its process starts, and appends nothing while it waits

| | |
|---|---|
| **Status** | Accepted (agent: a tightening under DEC-79 and DEC-176; it holds back a process that ran before, adds no opening, and owes no exit, since no agent can be deployed while it waits) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane V2), under DEC-79 and DEC-176; number assigned by the coordinator |
| **Story or claim** | E21-32 (claim #1442), E2c of DEC-2339's exchanged executor; connections spec §5.2 step 4; journal spec rule 68; amends DEC-2341 item 3 for the OAuth path |

### Context

Connections spec §5.2 step 4 says that while a connection is `connecting` its executor does
nothing but its checks: no order, no reconciliation, and no append but `StreamOpened`,
`AccountStateObserved` and `ConnectionChecked`. DEC-2341 item 3 lets the executor's process run
while its account stream is connecting, behind a gate that holds every opening. On the API-key
path that never shows: the deployment manager starts the executor only once the control stream
holds the establishment, so the first pass finds the copy due. On the OAuth path the manager starts
the executor before any check (DEC-2335), and `ExecutorProcess::start` reconciles at once, appending
`ReconciliationRun` and `AccountSnapshotRecorded` while the connection is still connecting.

### Decision

1. **The wait.** On the paper broker, after the exchanged start's check (DEC-2339) and the lease on
   the checked connection, and before `ExecutorProcess::start`, the executor waits until DEC-2341's
   state of its account stream is *established* or *to copy*. Each pass it reads the account and
   control streams, ticks the lease, and publishes a lifecycle that is live and not ready, so the
   probes neither restart it nor route to it. It appends nothing and asks the broker nothing.
   Once the state is reached, the process starts, and its first pass writes the copy as DEC-2341
   item 2 says.
2. **Every end is fail-safe.** A connection refused or revoked before its copy ends the wait as
   `connection_closed` while an agent is deployed on the account, for the deployment manager; with
   none deployed, nothing was started and nothing is owed, so it ends cleanly, exiting 0, as
   DEC-2347 has an agentless executor whose connection is closed end, and a kill-switch start of
   such an account (DEC-2344) never crash-loops; a connection record that does not read ends it with the establishment's
   code; a lease the vault no longer holds, or a vault that refuses its tick, ends it with the
   lease's code. Each exits before anything started. SIGTERM ends the wait with nothing started,
   and the process exits cleanly. The fixture broker does not wait. From its first instant to the
   wait's first pass (the vault login, the exchanged check and the lease) the process is starting
   and live for `serve::STARTUP`, 120 seconds, and no longer, so a start that hangs, a vault that
   never answers its login among them, is restarted by its probe rather than held live.
3. **Rule 13 holds** (`AGENTS.md` rule 13). While the executor waits, no agent can be deployed on
   the connection (the manager deploys only on an established connection), so the platform owes
   no exit, protective order or flatten on it. Positions the broker already holds are the owner's
   external holdings, which only the account-wide path (A2b) reaches, never this executor. A kill
   switch of the connection's or the account's scope that arrives during the wait has no agent here
   to apply to: the executor ends or goes on waiting by the rules above, and nothing the platform
   owes is dropped, since it owes nothing yet. Once the process starts, every kill switch already
   journaled is applied by its first pass, as any process does after a start.
4. **DEC-2341 item 3** keeps the process running while connecting on the API-key path, where it
   is never seen, and points here for the OAuth path.
5. **The scope a token entry holds.** The token exchange stores a token only when its `scope` is
   exactly the request (§5.2 step 4), and the token entry's one reader refuses any other. So a
   stored token never fails the scope check through its entry: an entry that is not the request's
   does not decode, and the exchanged start refuses it as `credential_malformed`, before any
   create or record.
6. **The loop is the library's.** The run's loop and the wait live in `mandate-executord`'s
   `serve` module, handed the binary's clock, lease tick, SIGTERM and stderr as closures, so a
   test drives them with a wiring that would admit an opening: the gate the run installs, the pass
   after each step, and each gate code said once are each pinned there, not left to the binary.

### Consequences

- An OAuth connection's account stream holds nothing but its check's records until the control
  services establish it, as §5.2 step 4 says.
- A connect that is never established ends with the API's teardown (DEC-2341 item 6): the refusal
  ends the wait.

<a id="dec-2347"></a>

## DEC-2347: The executor exits `completed` only when the account owes nothing more

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176, on the coordinator's rulings of 2026-10-22. It only tightens [DEC-2344](DEC-2344.md) (A)'s exit: the executor exits in fewer cases than that wording allows, and a case it does not cover keeps it running, which adds no risk) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137) (E7-4), on the coordinator's rulings, for lane D1's DEC-2344 |
| **Story or claim** | E7-4; [DEC-2344](DEC-2344.md) (A); [DEC-2335](DEC-2335.md) item 2; [DEC-2343](DEC-2343.md) "Not covered here"; infrastructure design §3.4; `AGENTS.md` rules 3 and 13 |

### Context

[DEC-2344](DEC-2344.md) (A) has the deployment manager keep an account's executor running while a
kill switch covering only stopped agents is unapplied, and read the executor's exit status 0 as
succeeded. It leaves the exit itself to the executor: exit 0 only when no agent it follows is
desired, every covering switch it has read is applied, nothing is in flight, and no `Unknown` order
is in that scope. `mandate-process`, the lifecycle every daemon shares, has one status-0 exit,
`Drained`, which means a drain the orchestrator asked for.

### Invariants first

1. The executor never exits on its own while the account owes risk reduction: a kill switch, a
   close or exit of a followed agent's lots, or an order whose state is not final (`AGENTS.md`
   rule 13).
2. Ambiguity keeps it running (rule 3): an unread stream, an unknown order, a reconciliation not
   done, or a process that is not ready is never read as nothing owed.
3. No other daemon can exit `completed`: only the executor sends its input.
4. The judgement reads the journal's rows and the fold alone, so a running executor and a restarted
   one judge the same, and a restart after the exit applies a later switch and exits again.

### Decision

1. **A new exit.** `mandate-process` gets `Exit::Completed`, code `completed`, status 0, and
   `Input::Completed`, which exits `Completed` from any state not yet exited. The manager reads
   status 0 as succeeded (DEC-2344 (A)); the code tells it apart from `drained`.
2. **Only the executor sends it.** A test in `mandate-process` reads every crate's sources and
   fails if any but `mandate-executord` names `Input::Completed`, under any alias.
3. **The predicate.** At the end of a pass that ran to its end, the executor exits `Completed` only
   when all of these hold:
   1. **No desired agent:** every agent it follows has an `AgentStopped`, which is final.
   2. **A followed agent, or a closed connection** (amended on lane D1's deadlock finding): it
      follows at least one agent, or it follows none and the account's connection is closed. That
      is: no `exchanged` start is open (item 3.9), and either one was journaled or the account had a
      connection established, and no connection of the account is still live (each one's latest
      establishment was followed by a `ConnectionRevoked`; a connect refused never established
      one). An executor that follows none while its connection is live, or with no connection or
      connect in the journal at all, never exits `Completed`, so the manager never reads it as
      succeeded. Without the closed-connection branch, an agentless executor whose stop DEC-2344
      item 3 holds for a switch would run for ever.
   3. **Every switch applied:** the pass read no kill switch to apply and no proposal the core does
      not hold, and every followed agent's stream parsed and read. A stream that does not read may
      hold a switch, so it keeps the executor running; it is not a stop (the coordinator's review).
   4. **Nothing in flight:** nothing in doubt and nothing unsent (a pass with either returns before
      the end).
   5. **Every order terminal:** every order the account stream holds, not only the covered agents',
      so `Unknown`, `PendingCancel`, a resting protective order and a working flatten each keep it
      running; and the broker, read just before the exit, lists no order on the account in a status
      that is not final (`filled`, `canceled`, `expired`, `rejected`, `replaced`), so an order the
      fold never held (one the platform did not place is external activity, never a fold order)
      keeps it running too. A read that fails keeps it running (lane D1's finding).
   6. **No lots held:** no followed agent may hold lots in an instrument the account holds: its
      sub-ledger, the quantity its kill switch would sell (trading-domain spec §5.5), is zero, and
      none of its buys there, in any state but `Rejected` or `Abandoned`, has other than its
      quantity applied to it, as when the broker's fill named no order the fold could attribute it
      to (Alpaca's fill activity names only the broker's order id): filled, or partly filled and
      then canceled or expired (the review of #2270). An unfilled buy canceled while the account
      holds the instrument only holds the exit longer, which is safe. A close its switch has not raised yet (outside the session, say)
      or an exit held keeps it running. Shares the owner holds outside the platform are no agent's
      lots and do not, unless they sit in an instrument where an agent's buy filled unattributed:
      then the executor keeps running, which is safe (lane D1's finding).
   7. **No reconciliation due.**
   8. **Ready:** its last reconciliation passed.
   9. **No open connect** (lane D1's finding): no `ProcessStartIssued` of this account's executor
      with cause `exchanged` (DEC-2335) is open on the control stream. A start is open until the
      connection of the latest `ConnectionRequested` for this account before it has a
      `ConnectionRefused` with occasion `connect` or a `ConnectionRevoked` after it; a start with no
      such request is open. DEC-2335 item 2 wants that executor until its connection closes, agent
      or none, and the manager restarts a wanted executor that exits, so without this the connect
      would loop into the crash-loop bound.
      An `exchanged` start with no `ConnectionRequested` for this account before it stays open for
      ever, so an agentless executor in that case never exits `Completed`. That is safe: the
      manager stops it.
4. **The executor may run indefinitely.** If the account never reconciles cleanly, an order stays
   `Unknown`, a stopped agent's lots are never sold, or a connect never closes, the executor never
   exits `Completed`. That is safe: it keeps reading switches and handing exits. DEC-2344's
   `retirement_held` alert tells the owner.

5. **A command after the pass's read.** The pass reads the control stream once. A kill-switch
   command committed after that read is not seen by the pass that exits; it is closed only by the
   manager re-checking after the exit 0 and starting the executor again for it (DEC-2344 item 1).
6. **The account-scope switch.** `owes_nothing` reads agent-scoped work only. When DEC-2333's slice
   A2b lands the account-scope switch, an account switch still closing (`closing_all`) must keep
   the executor running too; the backlog holds the row.
7. **Landing order.** Item 3.9 reads `ProcessStartIssued`'s cause `exchanged` before the journal
   admits it: DEC-2335's journal change (#2200) lands first, and until it does no record carries
   that cause, so item 3.9 holds nothing.
9. **The kill switch's own sub-ledger has the same blindness.** A buy whose fill named no order
   leaves nothing in `sub_ledger`, so the agent's kill switch would sell none of those shares
   (`AGENTS.md` rule 13). This decision only keeps the executor from exiting over them; the
   switch's sizing is the backlog row's.
8. **Released positions** (lane D1's check). Item 3.6's lots are the kill switch's own
   sub-ledger, the agent's filled buys net of its sells, capped at the position. A
   `PositionReleased` (DEC-2344 item 2, mandate spec §3.1) makes the shares the owner's, but the
   executor's fold does not interpret it yet: it refuses the record as `NotInterpreted` naming
   E6-4 (`mandate-executor`'s `every_catalogue_event_is_interpreted_or_named`), so an executor
   whose account stream holds one stops at replay, before this predicate is asked. When E6-4's
   fold slice interprets it, the released quantity must leave the agent's sub-ledger, so the
   switch sells none of it and item 3.6 no longer holds the exit for it, with tests that released
   lots let the executor exit and unreleased lots still hold it; the backlog holds the row.

Items 3.2 and 3.5 to 3.9 tighten DEC-2344 (A)'s wording, which names only `Unknown` orders in the switch's
scope: under DEC-176 an agent takes the reading that adds no risk.

### Consequences

- `mandate-process` and every daemon matching on its exits (`mandate-agentd`'s phases) gain the
  variant; no daemon but the executor reaches it.
- `mandate-executor` exposes `ExecutorState::holds_lots`, the kill switch's own sub-ledger.
- `mandate-executord`'s pass asks the predicate at its end; `agents_of` also reads the control
  stream's `ConnectionRequested`, `ConnectionRefused` and `ProcessStartIssued` for item 3.9.
- The tests PR adds pending tests for each item, a test that a restart after the exit applies a
  later switch and exits again, and the guard of item 2.

### Alternatives considered

- **Reuse `Drained`.** Rejected: the code would say a drain was asked for when none was.
- **Every order of the covered agents only, as DEC-2344 words it.** Rejected: an order outside the
  switch's scope that is not final may still be the account's risk, and the executor is its only
  ledger.
- **A flat account.** Rejected: shares the owner holds outside the platform would keep the executor
  running for ever, with nothing for it to do.

<a id="dec-2349"></a>

## DEC-2349: The API's and the connection manager's vault policies reach a connection made after provisioning, by a one-segment `+` on their write and delete paths, and the manager's on fingerprint reads

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79, accepted by the coordinator on 2026-10-22 in place of a provisioner that rewrites policies; shown to the founder as an FYI, since it touches DEC-2630's vault design. It widens no read of a credential: its one reading `+` is the manager's `personal/fingerprints/+/+`, over pseudonymous hashes alone, and no identity but these two gains anything; the executor's two create-only `+` paths under `x{its own name}` are DEC-2339's and DEC-2345's) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | `claude-code` (lane D1, the staging push); number assigned by the coordinator |
| **Story or claim** | E21-33, E10-13; infrastructure design §5.2; DEC-2630 items 5 to 7; DEC-2338; DEC-2348; claim #1901 |

### Context

`openbao_policy` writes each identity's policy with every path spelled out, for the connections the
cell holds when `provision` runs: in staging, the ids in `deploy/hosted/vault-connections.json`. A
connect from the web mints its connection id at `ConnectionRequested`, and the owner has the
request's 600 seconds to finish the authorization, so the API's write of the callback's code, and
the connect finish's deletes of the code and the token (lane X, DEC-2348, which the API runs as the
connection manager), are refused: neither policy names the new connection.

A provisioner that rewrites the two policies on each connect was considered and rejected. OpenBao
cannot bound a policy's content, so a token that may write the Api policy can write any path into
it, every connection's token and `sys/` included, and the API runs that policy: a power beyond the
one DEC-2630 item 7 accepted for its provisioner.

### Decision

1. **A `+` for the connection segment, on exactly these paths.** The Api policy holds
   `<kv>/data/connections/+/code` with `create` and `update`, and
   `<kv>/metadata/connections/+/code` and `<kv>/metadata/connections/+/token` with `delete`. The
   connection manager's holds the two `delete` paths alone. These replace each policy's
   per-connection `code` and `token` blocks; the connection manager's per-connection exchange-grant
   paths (`auth/<approle>/role/exchange-<c>/...`) are unchanged. What [`policy::allows`] permits
   does not change: it already ignores which connections are listed.
2. **No `+` on a read, but one, and no `*` anywhere.** The one exception is named: the connection
   manager's `<kv>/data/personal/fingerprints/+/+` with `read` (DEC-2345), rendered only while
   [`policy::allows`] permits the manager a fingerprint read. It covers each request's account
   fingerprint, a keyed hash under an opaque connection id and account ref, and never an account
   entry, the claims, the rotation marker, or a credential. No other `+` reads or lists, and no
   other identity's AppRole policy holds a `+`. The executor's and the token exchange's
   Kubernetes-auth policies stay templated on the login's own ServiceAccount name (DEC-2060); the
   executor's two create-only `+` paths, a last segment under `x{its own name}` for its account
   entries and its fingerprints, are DEC-2339 item 1's and DEC-2345's, not this decision's.
3. **The rule is stated where the policy is written:** `openbao_policy`'s doc comment and
   `mandate-vault`'s crate doc say "every path spelled out, except the Api's and the connection
   manager's one-segment `+` on their write and delete paths, and the manager's on fingerprint
   reads (DEC-2349)", and that no `+` is ever on a read of a credential.
4. **Tests pin it** (`src/tests/policy_prefix.rs`): for any connections listed, both policies grant
   exactly what `allows` permits on a connection no provisioning named, read the way OpenBao reads
   a `+`, as OpenBao's ACL does (any one segment, an empty one included); their connection blocks
   are the prefixes alone; only these two policies hold a `+`, only on those paths, never listing,
   and reading only the manager's fingerprints exactly as `allows` permits; nothing holds a `*`; and
   a templated policy's `+` only creates a last segment under the login's own name.
5. **The cell's vault drill admits exactly these `+` stanzas, each in its own policy, and nothing
   wider** (DEC-2022 item 3). The drill's scan prints every stanza of a `mandate-*` policy that
   holds a `*` or a `+`, and `xtask`'s `VAULT_DRILL_PREFIX_GRANTS` lists the six stanzas items 1
   and 2 make: the Api's code write and its code and token deletes in `mandate-api`, and the
   manager's two deletes and its fingerprint read in `mandate-connection-manager`, each with its
   path and its capabilities as written. Any other stanza with a wildcard is still a failure: a
   read of a credential, another path, an added capability, a `*`, one of these stanzas in another
   policy, or a wildcard outside a stanza. The drill's unit test seeds each of those. The grant
   cannot be written without a wildcard: the connections it reaches are made after the vault is
   provisioned, so no path can name them in advance, and OpenBao has no other way to match an
   unnamed segment.

### Rationale

The Api policy already writes and deletes every listed connection's code and token, and the
connection manager's deletes them; a `+` adds only connections that do not exist yet, with the same
capabilities and no read of a credential. It needs no new process, no policy-writing power, and nothing to remove on
a revoke, and it leaves the credentials themselves exactly as hard to read as they are.

### Alternatives considered

- **The DEC-2630 item 7 provisioner rewrites both policies on each connect.** Rejected above: it
  hands a policy-writing token arbitrary power through the API's role.
- **An operator lists the new id and applies again.** Rejected: it cannot fit inside the request's
  600 seconds.
- **Drop the drill's wildcard rule, or admit any `+` in these two policies.** Rejected: either would
  let a `+` on a read of a credential, or on another path, pass unseen. Item 5 admits the six
  stanzas alone.

### Risks

- The API may now write a code for, or delete the code and token of, a connection id that no
  connection holds yet. Its writes are reached only through the connect's callback, which takes a
  single-use `state` naming a server-issued request (DEC-821 item 3), and a delete of an entry that
  does not exist is a no-op.
- OpenBao's `+` matches any one segment, an empty one and one shaped like no connection included,
  so the two policies also write or delete at paths such as `connections//code` or
  `connections/anything/token`. That is harmless: nothing reads those paths, the token exchange and
  the executor read only their own connection's through their templated policies, and `allows`
  already permits these two identities every connection.
- The executor's and the token exchange's Kubernetes-auth roles are still made per connection, by
  DEC-2630 item 7's provisioner, which is not built; nothing creates them today. That is a separate
  staging blocker, reported to the coordinator.

<a id="dec-2354"></a>

## DEC-2354: The executor folds `PositionReleased`, taking released shares off the agent's lots

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176, on the coordinator's approval of 2026-10-23: it reads a record journal spec §9.27 already closes, and only narrows what the kill switch sells and what holds the exit to the agent's own shares) |
| **Date** | 2026-10-23 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), for E6-4, from [DEC-2347](trading-core.md#dec-2347) item 8 |
| **Story or claim** | E6-4; journal spec §9.27 (`PositionReleased`); mandate spec §3.1; trading-domain spec §5.5; [DEC-2347](trading-core.md#dec-2347) items 3.6 and 8; [DEC-2344](DEC-2344.md) items 1 and 2; `AGENTS.md` rule 13 |

### Context

An owner's release (mandate spec §3.1) makes an agent's position, or part of it, the owner's. The
executor's fold refused `PositionReleased` as `NotInterpreted` (E6-4), so an account stream holding
one stopped the executor at replay: it could neither apply that account's kill switch nor exit. Once
read, the released shares must leave the agent's lots, or the agent's kill switch would sell the
owner's shares and DEC-2347's exit would wait on them for ever.

### Invariants first

1. The agent's kill switch never sells shares the agent released to the owner (rule 13 reaches the
   agent's sub-ledger only).
2. The kill switch never sells more than the agent's attributed lots: nothing an upper bound
   assumes ever feeds its size.
3. A release is folded from the journal, so a restart reads the same lots.
4. One agent's release takes nothing off another agent's lots.

### Decision

1. **The fold reads `PositionReleased`**, and only it of §9.27's records; the others stay
   `NotInterpreted` (E6-4). It keeps, per agent and instrument, the sum of the quantities released.
2. **The kill switch's sub-ledger** is the agent's attributed lots (its own orders' fills, buys net
   of sells) less what it released, capped at the long position.
3. **DEC-2347 item 3.6's exit bound** is the agent's buys net of its sells and of what it released,
   capped at the position, where a buy in any state but `Rejected` or `Abandoned` with other than
   its quantity applied counts at its whole quantity. It only holds the exit longer, and never
   sizes a sell: an unattributed partial fill that was released still holds the exit, which is
   safe, while the switch sells nothing of it. So, while the owner holds shares of the
   instrument, an agent with an unattributed partial fill and a release keeps `holds_lots` true,
   and the executor waits rather than exit; this is expected, not a defect.

### Consequences

- `mandate-executor`: the fold, `kill.rs`'s `sub_ledger`, and `ExecutorState::holds_lots`.
- `properties::every_catalogue_event_is_interpreted_or_named` moves `PositionReleased` to the
  interpreted list in the implementation, as DEC-137 asks of the slice that interprets an event.
- Lane D1's `released.rs` (#2291) runs: the protection the agent holds, the opening bracket's
  stop leg its fill places since DEC-2353, is named in the release's `canceled_order_ids` and
  cancelled with it, since §9.27's release cancels the protection with it, and an order still
  resting at the broker rightly holds the exit (DEC-2347 item 3.5).
- Not covered here: a release written after an agent's kill switch has sized its flatten sells
  does not shrink those sells (backlog, E6-4).

<a id="dec-2355"></a>

## DEC-2355: Alert a kept, un-replaceable protection once per resting set, not every trading day

| | |
|---|---|
| **Status** | Accepted (founder, 2026-10-25): alert once per resting set. Lane LV implements it |
| **Date** | 2026-10-23 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137) (E7-4), from the independent review of #2139 |
| **Story or claim** | E7-4; trading-domain spec §5.4; [DEC-367](earlier-303-599.md#dec-367) item 4 (a′); [DEC-2312](DEC-2312.md) item 2 |

### Context

At a copied `TradingDayStarted`, `mandate-executor`'s `new_day` looks at protection on or past its
buffer day. Some of it cannot be re-placed:

- protection with no recorded prices (DEC-367 item 3);
- an OCO the profile offers no form to place again (DEC-2312 item 2).

Such protection is kept rather than cancelled. Instead the executor journals `ProtectionChanged
expiry_unreplaceable`, naming the orders, and alerts the owner (`protection_expiring`). DEC-367
item 4 (a′) says it does so "on every trading day it is due".

Nothing changes from one day to the next until the protection is cancelled, filled, or expires at
the broker. So the same record and the same alert repeat every trading day. The review of #2139
asked to send it once per instrument.

### Invariants first

1. The owner is alerted when kept protection first becomes due, and again whenever what is kept
   changes. The shortfall is never silent (DEC-367 item 4 (a′)).
2. The decision is folded from the journal, so a restarted executor alerts exactly as an unbroken
   one would.
3. Nothing about what is kept, cancelled or placed changes: only how often the record and alert
   repeat.

### Decision

1. **The fold keeps the instrument, cleared by any change to the resting set.** It marks the
   instrument when `new_day` journals its `expiry_unreplaceable`, and clears the mark when a
   protective order of the instrument is placed or cancelled, or leaves the protection (filled or
   expired). It keeps the instrument rather than the orders named: the journal's closed schema
   (rule 41) carries no `orders` on the `expiry_unreplaceable` change the fold reads, so the set
   cannot be stored. Any change to the resting set passes through one of those events.
   `re_cover`'s and `re_place`'s shortfall record has the same action and members, so `new_day`'s
   record names the day's copied `TradingDayStarted` as its `causation_id`, and the fold marks the
   instrument only for a record caused by the latest one. A shortfall record never marks it, so it
   never silences the due-day alert (the independent review of #2370).
2. **`new_day` alerts once per resting set.** It skips the record and the alert while the
   instrument is marked. Any change to the resting set clears the mark, so the protection alerts
   again on the next trading day it is due: an order cancelled, filled or expired, or a new
   tranche placed.
3. **Other alerts are untouched.** `re_cover`'s and `re_place`'s `expiry_unreplaceable`, which
   names no order, keeps alerting once a step as DEC-367 says. §5.4's bound keeps governing an open
   interval.

**Until the founder decides,** the executor keeps today's behaviour, the conservative option: a
due, kept protection is journaled and alerted on every trading day it is due.

### Alternatives

- **Keep the daily alert (today).** The owner is reminded each day until the protection is
  resolved or expires at the broker. The cost is a repeated alert and record for one unchanged
  fact.
- **Alert once, then again at expiry only.** Rejected: an expiry at the broker is already alerted
  by `re_cover` when the order ends. A second, dated rule adds surface without a new fact.

### Consequences

- None until the founder accepts it. Then the implementation is a DEC-77 pair:
  - tests: the second due day after an alert writes no record and sends no alert; a changed
    resting set alerts again; a replay of the first day folds to the same answer;
  - code: the fold's map and `new_day`'s check.
- The code before this decision passed `orders` to `new_day`'s `expiry_unreplaceable`, which
  rule 41 forbids on that action. `protection::changed` writes only the members rule 41 allows an
  action, so the record never carried them and the journal never refused it, but the code read as
  if it named the orders. The implementation passes none, so the code says what the record holds.
- From the same review: `replaceable()` already agrees with `re_place`. Every non-terminal
  protective order is in `protection.resting` when `new_day` runs, and `new_day` cancels all of
  them, so `re_place`'s `covered` is zero once those cancels are confirmed. Its doc comment says so;
  no code changes.

<a id="dec-2356"></a>

## DEC-2356: Protection placed at the bound beside an account kill switch, and when it is cancelled

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: it carries out DEC-2333 item 3 as decided and only tightens. It never places more than the held quantity less every sell not confirmed cancelled, never places once the close is requested, and keeps the position covered until the close can go) |
| **Date** | 2026-10-23 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137) (E7-4 A1c) |
| **Story or claim** | E7-4 A1c; trading-domain spec §5.4 ("Bounded unprotected intervals") and §5.5; [DEC-2333](DEC-2333.md) items 3, 5 and 6; [DEC-2337](DEC-2337.md) |

### Context

DEC-2333 item 3 decided what an account kill switch's wait does at §5.4's bound,
`max_unprotected_s`: "protection is placed again for the held quantity less every sell in the
instrument not confirmed cancelled". It also says that when a later read confirms the instrument,
that protection is cancelled by its id and confirmed before the close goes. It left A1c to place
it. Until now, A1b placed nothing in an instrument the switch closes.

Three details were left open:

- what counts as a sell not confirmed cancelled, for a sell the broker shows that the fold does not
  hold;
- where the prices come from once the protection that rested has been cancelled;
- how the "later read" is told apart from any read. Today every read after the switch cancels each
  working order of ours by its id. It would cancel the new protection at once, even while another
  sell still blocks the close.

### Invariants first

1. Protection placed at the bound never covers more than the held quantity less every sell that
   may still fill. A sell counts until a read shows it gone or the broker confirms its cancel.
2. Nothing is placed in an instrument once its close-position is requested (DEC-2333 item 5).
3. The close never goes while that protection may still fill.
4. While another sell blocks the close, that protection stays and the position stays covered.
5. A restart reads the same: what was placed at the bound is folded from its records.

### Decision

1. **What is not confirmed cancelled.** Counted at the quantity each may still sell:
   - every sell of ours sent and not terminal. That includes the protection that rested before the
     switch, until a read shows it gone or its cancel is confirmed (a lost cancel-all confirms
     nothing), every `Unknown` order, and working exits;
   - every sell the switch's last read showed working that the fold does not hold, at its quantity
     less what filled. The read keeps those quantities per instrument.

   If the sum cannot be taken (an overflow), it covers everything and nothing is placed. Placing
   less is the safe side; rule 12 allows no short sale.
2. **Size.** `whole(long − sum)`: the whole-share part of what the position the fold holds leaves
   once that sum is taken off, in the strongest form the profile offers, as `re_place` sizes it.
   Where that leaves nothing, nothing is placed.
3. **Prices.** The resting protection's recorded prices. If the protection that rested has since
   been cancelled, the latest protective order of the instrument whose request recorded them,
   taken as the last by id. If there are none, or the profile offers no form, nothing is placed:
   the bound has already journaled `interval_limit` and alerted the owner.
4. **What was placed at the bound** is folded from its `placed` record. Any protection placed
   while the instrument is one the account switch closes is the bound's, since nothing else places
   there (DEC-2333 item 6). The switch keeps the set until its mode is lifted.
5. **Its cancel.** A read after the switch still cancels each working order of ours by its id, as
   DEC-2333 item 3 says, except protection the bound placed: that is cancelled only on a read that
   shows no other sell working in its instrument. The close then waits for that cancel's
   confirmation, as it already waits for any order of ours the read showed.
6. **An exit sequence at the bound does not stop it.** The bound ends the instrument's exits
   (`end_exits`), and nothing re-places protection in an instrument the switch closes once they
   end. So the bound places what the exits still working leave uncovered, counting them as not
   confirmed cancelled (item 1): a flatten rung in flight covers the position, and nothing is
   placed beside it.
7. **Once per interval.** The bound fires once per interval, as it does today. Its placement ends
   the interval, awaiting the acknowledgment (DEC-348 item 2). If that protection is later
   cancelled for the close, a new interval opens, and its bound places nothing once the close is
   requested.

8. **No close beside a sell a read cannot show** (the review of #2327). A read may not yet list a
   sell of ours the broker has not acknowledged (`Submitting`), such as the bound's own protection
   just sent. So the close waits while any such sell of ours in the instrument is missing from the
   read, as it waits for a listed one's confirmed cancel. An `Unknown` order does not hold it: an
   order a read omits is what the executor reads as `Unknown`, so that read already confirms it
   gone, as DEC-2333 item 2 has a read confirm the cancel-all.

Carried from A1b: `note_answer` takes the next broker answer after the switch for the cancel-all's.
It relies on the broker answering in order, which `mandate-executord`'s synchronous calls
guarantee; its doc comment says so. A test pins a restart after a lost cancel-all: the startup
reconciliation confirms it and the close goes, and the cancel-all is never sent again.

### Alternatives

- **Cancel the bound's protection on any read, as every other order of ours is.** Rejected: the
  position goes uncovered again whenever another sell still blocks the close, against invariant 4.
- **Size against the stop that rested before the switch only.** Rejected: a foreign sell or an
  exit still working could then over-cover the position, against invariant 1.

### Consequences

- `mandate-executor`:
  - `protection.rs`'s `bound` places protection through the new `protect_at_bound`, and shares the
    submit step with `re_place`;
  - `kill.rs`'s `note_read` keeps the read's foreign sell quantities, `open_orders_read` holds the
    bound's protection while another sell works, and `note_answer`'s doc comment changes;
  - the fold keeps `AccountSwitch.bound`.
- The A1b test `protection_the_cancel_all_cancels_opens_a_bounded_alerted_interval` asserts only
  that nothing is sold and nothing is placed beside the cancel. What the bound places is pinned by
  A1c's own tests.
- The protection is sized against the position the fold holds, as `re_place` sizes it. A crypto fee
  withheld in the asset lowers that position once it posts. If the broker refuses a sell above what
  it holds, the interval stays open and bounded (DEC-348 item 2).

<a id="dec-2357"></a>

## DEC-2357: An unreadable connection record holds openings only, never the executor's exits

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176, on the coordinator's ruling of 2026-10-23: it only tightens, since every exit and kill switch keeps going and openings are held, as `AGENTS.md` rules 3 and 13 require) |
| **Date** | 2026-10-23 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), on the coordinator's ruling, from lane V2's #2288 finding |
| **Story or claim** | E7-4; [DEC-2343](DEC-2343.md); [DEC-2347](trading-core.md#dec-2347) items 3.2 and 3.9; DEC-85; `AGENTS.md` rules 3 and 13 |

### Context

`mandate-executord`'s `follow::agents_of` reads which agents the executor follows from the control
stream. Since #2300 (DEC-2343, DEC-2347) it also reads the connection records:
`ConnectionEstablished`, `ConnectionRevoked`, `ConnectionRequested`, `ConnectionRefused` and
`ProcessStartIssued`.

When any record it reads is missing a member, it answers `Unreadable`, and `ExecutorProcess`
stops: "a control record that does not read stops the process at once". So since #2300, a
malformed `ConnectionRefused` (one with no `connection_id`, say) stops the executor. It then takes
no risk exit and applies no kill switch. Lane V2's #2288 test
`an_unreadable_record_keeps_the_gate_shut_and_every_exit_going` caught it. Before #2300 the
connection records were not read, so the exit went.

### Invariants first

1. Ambiguity never adds risk (rule 3): a record the executor cannot read never lets an opening
   through.
2. Risk reduction is never denied by it (rule 13): every risk exit, protective order, owner exit
   and kill switch keeps going.
3. The executor never exits `completed` on a reading it is unsure of (DEC-2347).

### Decision

1. **A connection record that does not read is set aside, never a stop.** This covers
   `ConnectionEstablished`, `ConnectionRevoked`, `ConnectionRequested`, `ConnectionRefused` and
   `ProcessStartIssued`. `agents_of` notes it as `unsure`:
   - every agent it follows has its openings held, as a revoked connection's are (DEC-2343
     item 1);
   - `Following::released` answers false, so the executor keeps running;
   - its exits and kill switches are handed as before.
2. **The other control records still stop the process when they do not read** (DEC-85):
   `AgentDeployed`, `AgentStopped` and a kill-switch `OwnerCommandIssued`. They name the agents the
   executor follows and the switches it may owe, so skipping one could drop an agent the account
   owes a kill switch. This is unchanged by #2300, and those stops take no exit either. That case
   is put to the coordinator, not changed here.
3. **While unsure, nothing the record may have said is assumed away** (the review of #2329):
   - every agent with a version-2 `AgentDeployed` is followed, whether or not the connection it
     names is one of this account's read so far. An establishment that did not read may have been
     its only one. Following one too many only reads that agent's exits and switches, which act on
     its own ledger here (DEC-2003), and its openings are held;
   - every connection- and workspace-scope kill-switch command covering a followed agent is
     applied by the executor itself, as if a revocation of its connection came at or after it. A
     revocation that did not read may have been the compromise revocation DEC-2343 reads a command
     for. This costs the owner's confirmed bid, as DEC-2343's revoked path already does.
4. **Nothing a connection record that does read says is lost.** A record set aside adds nothing to
   the reading, and the reading is as if it were not journaled, but held and unreleased. The
   records it reads are each read in full before anything is noted from them, so a record that
   fails part-way notes nothing.

### Follow-ups

- **An unreadable kill-switch command** (`OwnerCommandIssued` `kill_switch`) still stops the process
  (DEC-85 class). A tightening for a later decision: read it as a kill switch for every agent
  deployed before it, so the account is reduced rather than stopped.

### Consequences

- `mandate-executord` `follow.rs`: `agents_of` sets a connection record that does not read aside
  as `unsure`. `Following.unsure` holds every followed agent's openings and keeps `released` false.
- `tests/process.rs`'s `an_unreadable_control_record_stops_the_process` keeps the deployment and
  stop records. The two connection records move to `tests/unreadable.rs`, which pins the new
  reading for each of the five.

### Alternatives

- **Keep stopping on every unreadable control record (today).** Rejected: no exit goes (rule 13).
- **Skip the unreadable record silently, as if it were not journaled.** Rejected: a revocation or
  a refusal it may have held could open the gate to an agent the owner cut off (rule 3).

<a id="dec-2358"></a>

## DEC-2358: A discretionary exit with its own limit and nothing to price from goes at that limit

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176, on the coordinator's ruling of 2026-10-23), flagged to the founder for review after the fact under DEC-79. It narrows [DEC-160](../04-decision-log.md#decisions) (12); the founder may revert it to DEC-160 (12)'s hold once `mandate-executord` has a quote feed. The founder reviewed it on 2026-10-25 and keeps it |
| **Date** | 2026-10-23 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), on the coordinator's ruling, from lane V2's #2288 finding |
| **Story or claim** | E7-4; trading-domain spec §5.6 step 4; DEC-160 (12); [DEC-2037](DEC-2037.md) item 5; [DEC-2353](DEC-2353.md); `AGENTS.md` rules 2, 3 and 13 |

### Context

DEC-160 (12) covers an exit with nothing to price from: no fresh sane bid, no sane bid within five
minutes, and no last trade. A risk exit, a protective order, a kill-switch flatten or an owner exit
falls back to its intent's own limit, journaled and alerted. A discretionary exit is held and
re-checked every tick until a quote arrives.

`mandate-executord` hands the core no quote at all: its inputs are ticks, starts, commands,
intents, broker answers and broker snapshots. So a discretionary exit there that §5.4's sequence
prices is held for ever, and in practice that is any exit beside resting protection.

Until DEC-2353 (#2300), the fill of a bracketed entry was usually unattributed, so no protection
read as resting. Such an exit went at its own limit, unsequenced. Once the fill is attributed, the
bracket's stop leg rests, the exit takes the sequence, and it is held. Lane V2's #2288 test of a
discretionary exit beside a bracket caught it.

### Invariants first

1. An exit is never denied for want of a price (rule 13: paced, never denied). A hold with no end
   is a denial.
2. Nothing here adds risk (rules 2 and 3). A limit sell never fills below its own limit, and an
   opening is untouched.
3. The fallback is never silent: it is journaled (`exit_unpriced`) and the owner alerted with the
   generic key, which carries nothing sensitive (rule 6).
4. A fallback price is never where the ladder ends. It steps again at the next priceable quote.

### Decision

1. **A discretionary exit with a limit of its own and nothing to price from falls back to that
   limit**, as DEC-160 (12) has every other exit do. It is journaled `exit_unpriced` and the owner
   is alerted (`exit_unpriced`). `exit_price`'s `Fallback` arm no longer excludes the discretionary
   purpose.
2. **An exit with no limit of its own is still held** for want of a price, whatever its purpose
   (DEC-2037 item 5). It has no limit to fall back to.
3. **An opening is unchanged.** `exit_price` prices only exits, and an opening keeps its own limit.
4. **This narrows DEC-160 (12)**, which held the discretionary exit. Holding it for ever keeps
   exposure the agent chose to cut, against rules 2, 3 and 13, which outrank DEC-160 (12). The
   change loosens no risk limit; it bounds a wait for a better price. That makes it a reading
   DEC-176 lets an agent take, flagged to the founder under DEC-79.
5. **The founder may revert it** to DEC-160 (12)'s hold once `mandate-executord` hands the core
   quotes (backlog, E21-30), so the hold ends when a quote arrives as DEC-160 (12) meant.

### Alternatives

- **Wire a quote feed into `mandate-executord` first.** This keeps DEC-160 (12) as written, and is
  the proper follow-up (backlog row). It means a data-host transport, a vault credential, a poll,
  and a fake for tests, which is too large for the rule-13 fix.
- **Keep the hold.** Rejected: every discretionary exit beside resting protection is held for
  ever in a running executor.

### Consequences

- `mandate-executor` `protection.rs`'s `exit_price`: the fallback covers every exit with a limit
  of its own.
- The unit tests that pinned DEC-160 (12)'s discretionary hold keep their scenarios with an exit of
  no limit, which is still held:
  - `a_discretionary_exit_with_nothing_to_price_from_is_held_then_goes`;
  - `an_exit_whose_price_vanishes_mid_sequence_re_protects_at_once`;
  - `an_unpriced_exit_beside_another_sequence_re_places_nothing`;
  - `a_long_hold_alerts_once_and_the_exit_still_goes`;
  - `a_held_exit_counts_nothing_toward_the_gates_room`, whose script's `Move::Exit` takes a limit
    of 0 as none.

  `exit_price_never_refuses_any_purpose` leaves the discretionary unpriced case to the new pending
  test, since a pending test may not be macro-generated.
- Lane V2's #2288 case `every_risk_reduction_goes_alike_under_a_shut_and_an_open_gate` should pass
  unchanged.

<a id="dec-2359"></a>

## DEC-2359: the executor reads quotes for its waiting exits by REST, until the data service pushes them

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent (Claude Code, lane V2), under DEC-79, on the coordinator's ruling on claim [#2354](https://github.com/kunwarshivam/mandate/issues/2354); a reversible engineering choice that adds no risk. Independent review before merge |
| **Story or claim** | E21-30 (executor process), the debt row [DEC-2358](trading-core.md#dec-2358) added; trading-domain spec §4.2, §5.4, §5.6, §8.2; data-plane spec §3.6; infrastructure design §3.1; [DEC-2059](infrastructure.md#dec-2059) item 2; [DEC-2306](trading-core.md#dec-2306) items 1 and 4; [DEC-821](DEC-821.md) item 4; DEC-168 item 2; `AGENTS.md` rules 3, 6, 7 and 13 |

### Context

`mandate-executord` hands its core no quote. Its inputs are ticks, starts, commands, intents,
broker answers and broker snapshots. So §5.6's ladder never prices an exit there. An exit beside
resting protection goes at its own limit (DEC-2358), or is held for want of a price if it has no
limit (DEC-2037 item 5).

The design has the workspace data service push quotes to the executor over a host-local channel
(data-plane spec §3.6, infrastructure §3.1), which is E21-44, not yet built. Until then the
executor has no source of quotes.

### Invariants first

1. A quote is never needed to sell. A failed, stale or insane read hands nothing, and the core
   falls back exactly as it does today: DEC-2358's own limit, or DEC-2037 item 5's hold (rule 3).
2. The read never adds risk, and nothing in it can place an order. Its client reads market data
   only, and the core prices only exits from it.
3. The executor reaches no live trading host (DEC-821 item 4).
4. No quote value, symbol or position leaves the deployment in a log (rule 6). Quotes are
   process-local and never journaled.
5. Reading the set of instruments to quote changes no state.

### Decision

1. **A `QuoteFeed` port in `mandate-executord`.** On each step, before the tick, the process asks
   the core which instruments have an exit waiting on a price
   (`ExecutorState::quote_wanted`, read-only). Those are an exit in §5.4's sequence, a lone exit
   ladder, a held exit or flatten, and an instrument an agent's kill switch has yet to raise its
   close in: §5.5 names that close's limit from a sane quote when neither the protection's stop
   nor a confirmed floor does, so with none of them the close waits for a quote. It reads each one's latest quote through the port, at once
   when it is first wanted and then at most once per `exit_step_s`, and hands each read as
   `Input::Market`.
2. **The interim feed is a REST read.** `AlpacaQuotes` calls `mandate-alpaca`'s
   `DataClient::latest_quote` on Alpaca's market-data host, `data.alpaca.markets`, on the paper
   equity profile's IEX feed (trading-domain spec §4.2). It uses the connection's own transport and
   credential (DEC-168 item 2: the paper keys read market data too), so no new secret is held.
3. **This is a temporary deviation from data-plane §3.6**, which has the data service push quotes
   to the executor over the host-local channel. When E21-44 builds that channel, it replaces
   `AlpacaQuotes` behind the same port, and the executor's egress loses the data host again.
4. **Only a sane, fresh quote is handed.** A read that fails, is older than the profile's bound,
   names another instrument, or is not a quote at all hands nothing (`quote_unread`). A quote that
   is not sane as §8.2 says (`0 < bid ≤ ask`, and the spread within the profile's bound) hands
   nothing too (`quote_insane`). An older sane quote then ages out of the core's freshness bound
   within one exit step. A quote stamped ahead of the clock is observed at the clock's now.
   The paper profile's bounds are the reviewed rule set's IEX quote age, ten seconds, and a spread
   of at most 2 % of the bid, the collar's width outside the liquid tier. §4.2 leaves both to the
   profile's configuration, and the executor reads none yet, so they are fixed beside the feed
   (`credential::paper_quote_bounds`), and a test pins both and their edges. A tighter bound only
   hands fewer quotes.
5. **The executor's egress gains the data host, on 443, by exact name.** Staging's
   `egress-fqdn-executor` adds `data.alpaca.markets`, and the cell's `processes.json` gives the
   executor `market-data`. This supersedes DEC-2059 item 2's "the executor reaches only
   `paper-api.alpaca.markets`": it reaches that paper trading host and the data host, and still no
   live host. `data.alpaca.markets` is the paper data profile's host (ADR-0001 ES-23 compiles it
   in), serves no trading or account call, and the read sends only `GET` of a latest quote. The
   streaming host stays the data service's alone.
6. **It amends DEC-2306 items 1 and 4 to match.** The executor's credential module, the one module
   that may name the paper transport and the credentials, may now name the data client too, and
   `AlpacaQuotes` lives there. No other source names it, a host, or an HTTP client, and
   `tests/paper_only.rs` pins both. The executor's egress names one broker host and the data host.
7. **Instruments with only resting protection are not read.** A stop or take-profit resting at the
   broker needs no price from the executor. Repricing or re-protecting them on a quote is lane
   LV's backlog row, not this feed's.
8. **With the feed down, a waiting exit stays where it was.** An exit held `exit_unpriced` is
   journaled as held and the owner is alerted with the generic `exit_unpriced` key (DEC-2037
   item 5, rule 6). A kill switch's close with nothing to price from is journaled the same way,
   caused by its `KillSwitchActivated`, and alerted once per switch and instrument
   ([DEC-2361](trading-core.md#dec-2361)); its instrument is wanted (item 1), so the first quote the feed reads
   raises the close at it. Whether a bounded fallback is owed when no quote comes is DEC-2361's
   question, not this one's.
9. **The paper serve path cannot be given a wiring without the feed by accident.** The binary's
   serve path takes a `Run`, a broker paired with its wiring, which only `run::fixture` (the
   fixture broker, reading no quote) and `run::paper` (a broker whose transport reads market
   data, with the feed over it) make. The serve path that takes any wiring takes only a
   `ScriptedBroker`, which no library broker is, and the orphan rule keeps the binary from making
   one of them so. Swapping one run for the other on the paper path, or serving the paper broker
   with the fixture's wiring, does not compile. This guards against accident, not intent: a type
   of the binary's own wrapping a broker could implement `Broker` and `ScriptedBroker` and be
   served with any wiring. `tests/paper_only.rs` therefore refuses any implementation of either
   trait in `src/main.rs`, so that bypass fails CI too.
10. **DEC-2358 stands.** The founder may revert it to DEC-160 (12)'s hold now that a quote can end
   that hold. This decision does not.

### Alternatives

- **Build E21-44's pushed channel first.** That is the design, but it needs the data service's
  socket, its fan-out and its tests, which is a story of its own. Meanwhile every unpriced exit in
  a running executor goes at its own limit or waits.
- **Read quotes in the data service and journal them for the executor.** This puts every quote in
  the journal, which the executor's core keeps process-local, and adds a writer to the account
  stream.
- **Hand an insane quote with `sane` false.** The core already refuses to price from it. But
  handing it would make it the instrument's current quote, displacing an older sane one before
  that one ages out. Handing nothing is simpler and adds no risk either way.

### Consequences

- An exit beside resting protection is priced by the ladder once its quote reads, instead of going
  at its own limit or waiting.
- The executor holds one more egress destination until E21-44. A compromised executor could read
  market data with the connection's keys, which it already holds.
- Each wanted instrument costs one REST call per exit step. That is well within Alpaca's data rate
  limit for the handful of instruments a mandate holds.
- Tests: `mandate-executor/tests/quote_wanted.rs` (the wanted set, computed from what each case
  builds) and `mandate-executord/tests/quotes.rs` (the poll's pacing, the sanity verdict, every
  failure handing nothing, and a held exit beside its protection priced once its quote is read).

<a id="dec-2361"></a>

## DEC-2361: A kill switch's close held for want of a price is journaled and alerted once

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: it adds an alert and a record, and changes no order, hold or price, so it tightens rule 13's "never silent" and adds no risk) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), for E7-4, on the coordinator's ask after [DEC-2360](DEC-2360.md)'s executord probe |
| **Story or claim** | E7-4; trading-domain spec §5.5 and §5.6; [DEC-160](../04-decision-log.md#decisions) (12); [DEC-2037](DEC-2037.md) item 5; [DEC-485](DEC-485.md) item 2; `AGENTS.md` rules 6 and 13 |

### Context

An agent's kill switch raises one flatten per instrument it closes, `k-<switch>-<n>`, once a limit
can be named for it: the lowest sane quote, else the resting protection's stop, the owner's
confirmed floor applied (`kill.rs`'s `flatten_closes`, DEC-485 item 2). With none of them, the
close is not raised and nothing is journaled or alerted. It waits, silent, for a quote.

That is reachable: the protection was cancelled before the switch (by a release, or an exit that
ended), and no quote has been seen. `mandate-executord` hands the core no quote yet (E21-30), so
there it waits until the quote feed lands. Every other exit held for want of a price is journaled
`exit_unpriced` and alerted (DEC-2037 item 5, DEC-160 (12)); this one is not.

### Invariants first

1. A switch's close held only for want of a price is never silent: the owner is told once.
2. The alert changes nothing else: no order, hold, price or pacing, and the close is raised as
   soon as a limit can be named, as today.
3. A replay alerts exactly as an unbroken executor would: the once is folded from the journal.

### Decision

1. **Journal and alert once.** After every step, a switch's close that would be raised but for a
   limit (pending, nothing of the switch's working in the instrument, not deferred, the agent's
   sub-ledger above zero) is journaled `ProtectionChanged exit_unpriced` for its instrument, with
   the switch's `KillSwitchActivated` as its `causation_id`, and alerted with the executor key
   `exit_unpriced` (the notifications spec's `exit_stalled` row). The notice names the record
   only (rule 6).
2. **Once per switch and instrument.** The fold notes the instrument on the switch when it reads
   an `exit_unpriced` caused by that switch, and the close is not alerted again while the note
   stands. A raised close ends the wait as it does today.
3. **Nothing else changes.** The close is raised the step a limit can be named, at that limit.

### Alternatives

- **Raise the close at the market.** Rejected: an order path change, which the coordinator ruled
  out for this pair, and §5.6 prices every exit through the ladder.
- **Alert on every tick.** Rejected: the same fact repeats, as DEC-2355 found for the kept OCO.

### Consequences

- `mandate-executor`: `kill.rs`'s `flatten_closes` journals and alerts; the fold keeps the note on
  the switch.
- No journal change: `exit_unpriced` already closes with the instrument alone, and `causation_id`
  is free on it.
- Tests in `mandate-executor/tests/hand.rs`: `a_switch_close_with_nothing_to_price_from_is_alerted_once`,
  pending E7-4 until the implementation, and `a_switch_close_alerted_unpriced_sells_once_a_quote_arrives`,
  which holds on `main` already.

<a id="dec-2362"></a>

## DEC-2362: Protection is never sized over shares the owner released and still holds

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: it only keeps the agent's protective orders off the owner's released shares, which rule 13 and DEC-2354 invariant 1 already require), except item 5, which is Proposed and reserved for the founder (DEC-79: it states an edge where the agent's shares are under-covered, a weakening of a protection invariant) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), for E6-4, from [DEC-2360](DEC-2360.md) item 4 |
| **Story or claim** | E6-4; trading-domain spec §5.4; mandate spec §3.1; journal spec §9.27; [DEC-2354](trading-core.md#dec-2354); [DEC-2360](DEC-2360.md) item 4; `AGENTS.md` rules 12 and 13 |

### Context

An owner's release (mandate spec §3.1) makes shares the owner's external holding, unprotected, as
the release's warning says. The executor still sizes the protection it places at the long
position (`protection.rs`'s `long`), so wherever it places protection again it covers the owner's
released shares with the agent's protective order, whose stop would sell them:

- the end of an exit sequence (`re_place`), as DEC-2360 item 4 found after an abandoned flatten;
- `re_cover`, after a protective order is lost at the broker;
- `new_day`'s re-placement at the buffer day.

### Invariants first

1. The protection the executor places never covers a share the owner released and still holds.
2. It covers every other share it covers today, except shares the fold reads as the owner's:
   released shares the owner still holds, and, once the owner has sold released shares, an
   agent's fills the fold could not attribute (item 5).
3. A share the owner released and has since sold leaves nothing to subtract: the agent's own
   shares stay covered.
4. Folded from the journal, so a replay sizes the same.

### Decision

1. **One rule, `protectable`.** Protection the executor places in an instrument is sized from the
   long position less the released shares the owner still holds, never from the long position
   alone. The owner still holds `min(released, long − agents)`, where `released` is every agent's
   `PositionReleased` quantity in the instrument and `agents` is the agents' own lots there: each
   agent's filled buys net of its filled sells and of what it released, floored at zero, summed.
   Every site that sizes protection from the long position takes it: the sequence end's
   `re_place`, `re_cover`, and `new_day`'s replacement through `re_place`. A size of zero places
   nothing.
2. **Nothing else changes.** Exits are sized as before (an exit is DEC-2360's), the bound's alert
   and the unprotected interval are untouched, and no release, mode or journal record changes.
3. **Not covered here.** The plain-entry stop (`stop_after`) and the bracket's legs cover the
   entry they protect, which is the agent's own; the bound's own risk exit is an exit. Neither
   reads the released quantity, and both are left as they are.

4. **What the lots read.** Each agent's lots are its own orders' filled buys and what its sells
   sold, as `long` reads a sell that ended. A sell whose order names no agent (the `*` of an
   account switch, or an order the fold adopted) counts toward no agent: no path of `re_cover` or
   `new_day` was found to reach one beside a release (the independent review of #2399).

5. **The edge invariant 2 names (Proposed, for the founder).** The owner's holding is read as the
   long position less the agents' own lots, so an agent's fill the fold could not attribute reads
   as the owner's. Once the owner sells released shares, that fill can be taken out of the
   agent's protection: with 10 bought, 2 more filled unattributed, 4 released and 3 of those sold
   by the owner, the code covers 6 of the agent's 8. No sizing rule removes it without the
   opposite error, covering released shares the owner still holds whenever the attributed lots
   under-count. Two things bound it: DEC-2353 attributes a fill by the broker's order id, so an
   unattributed fill is rare, and the edge needs both a release and an external sale by the
   owner. The code stays as items 1 to 4 say until the founder chooses between under-covering in
   this edge and over-covering the owner's shares (the independent review of #2399, minor A).

### Alternatives

- **Size at the agents' sub-ledgers alone.** Rejected: a fill the fold could not attribute leaves
  the agent's shares out of its sub-ledger (DEC-2347 item 9), so they would lose protection.
- **Size at the long position less everything released.** Rejected: once the owner sells released
  shares, the subtraction would take the agent's own shares out of its protection (invariant 3).

### Consequences

- `mandate-executor`: `protection.rs` gains `protectable`, read where protection is sized from the
  long position.
- Tests in `mandate-executor/tests/hand.rs`, pending E6-4 until the implementation:
  `an_exits_re_placement_after_a_release_protects_only_the_agents_shares`,
  `a_re_cover_after_a_release_protects_only_the_agents_shares`,
  `a_release_of_all_the_agent_keeps_places_no_protection`,
  `a_buffer_day_re_placement_after_a_release_protects_only_the_agents_shares`,
  `another_instruments_lots_leave_this_ones_release_as_it_is` and
  `an_agent_that_sold_more_than_it_holds_adds_nothing_to_the_agents_shares`; plain, because they
  hold on `main` already: `released_shares_the_owner_sold_leave_the_agents_protection_whole` and
  `a_release_in_another_instrument_leaves_this_ones_protection_whole`.

<a id="dec-2363"></a>

## DEC-2363: The token exchange waits for its code until the request's own deadline

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79, option (a) as the coordinator chose it on 2026-10-25; it tightens nothing away and loosens no safety rule: rule 256's single start stays) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent (Claude Code, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lane D1 for the manager's half |
| **Story or claim** | E10-13, E10-22; [DEC-1175](DEC-1175.md) and [DEC-2350](DEC-2350.md) (the manager starts one exchange once the request's roles are made, journal spec rule 256); [DEC-2628](process.md#dec-2628) (the code is taken once); [DEC-2627](DEC-2627.md) item 1 (the request's deadline); [DEC-2330](DEC-2330.md); [DEC-2063](infrastructure.md#dec-2063) |

### Context

The deployment manager starts a connection's token exchange exactly once, as soon as its
`ConnectionRolesMade` is read, and never restarts it (rule 256). The exchange takes the code from
the vault once and, with none waiting, ends `code_missing` (`exchange_code`). The code is written
only by the API's callback, after the owner authorizes at Alpaca, which takes longer than the
role provisioner and the manager take to start the exchange. So a real connect's exchange runs
before its code exists and the connect never completes. The cell drill (E10-22) found the
sequence; reading the code shows it holds for every real connect.

### Decision

1. **The deadline is the request's, never the pod's.** The manager renders
   `MANDATE_REQUEST_EXPIRES_AT` on the token-exchange pod it starts from the request deadline it
   already folds: the `ConnectionRequested` row's `recorded_at` plus 600 s, as
   [DEC-2627](DEC-2627.md) item 1 reads it, the same value that sets the pod's
   `activeDeadlineSeconds`, in the journal's UTC timestamp form. It reads no clock of its own for
   it, and a regenerated pod for the same request gets the same value, so a restart gives no fresh
   lifetime. The API's pending state expires 600 s after its own clock at the start, at or before
   `recorded_at`, so the deadline can trail the state by the API-to-journal delay: a wait that long
   only ends `code_missing` that much later, and never redeems a code the API would not. (Lane D1,
   `mandate-deploy-manager`.)
2. **The exchange refuses to start without a usable deadline.** A missing value is
   `request_deadline_missing`, one not in the journal's timestamp form is
   `request_deadline_malformed`, and one at or before the pod's clock is `request_expired`, which also
   covers a pod whose clock runs ahead. Each is written alone to standard error with exit 2, as
   every start refusal is today, before the vault or the network.
3. **The wait.** Before each take, the exchange ends `code_missing` once the clock reaches the
   deadline, having taken nothing more and sent nothing. Otherwise it takes; a code found is
   exchanged as before; none found, it pauses and tries again. The first pause is 1 s, each next
   one doubles, up to 15 s, and no pause runs past the deadline, so there is no busy loop and no
   wait beyond it. The wait never runs past the pod's own start plus `STATE_LIFETIME_SECS` either,
   so a deadline far ahead, from a pod clock behind or a bad value, still ends within one lifetime.
4. **The code is taken once.** The take is the vault's read-and-delete (DEC-2628 item 2); two
   exchanges racing for one code find it once between them, and the other ends `code_missing` at
   its deadline. A test pins it.
5. **The request closing ends it too.** The manager stops the exchange's pod when its request
   closes (rule 256), which ends the wait at once.
6. **No secret reaches a log.** The process still writes only a code; the wait adds no output.
7. **The vault session outlives the wait.** The exchange logs in after the request started, and its
   session lasts `GRANT_SECS` (600 s), so it ends after the request's deadline: the wait needs no
   second login.
8. **Rule 256 is unchanged:** one start per request.

### Rationale

The exchange is the one process that holds the client secret and sends the live request; letting
it wait for the code keeps that in one place and keeps the manager's single start. Bounding the
wait by the request's own deadline means a restart, a slow start or a skewed pod clock never
extends the time a code may be redeemed beyond what the API's pending state allows.

### Consequences

- The deadline trails the API's pending state by the API-to-journal delay, milliseconds in the
  cell: the API reads its clock for the state before the journal stamps `recorded_at`. The gap
  errs late only. A code exists only once the API's callback accepted the state before it
  expired, so the longer wait can redeem only a code the API already accepted, never one written
  after the state expired; past the state, a wait just ends `code_missing` that much later.
- A connect whose owner authorizes after the deadline ends `code_missing` and the request closes;
  the owner starts a new connect.

### Alternatives considered

- **Start the exchange only once the callback wrote the code.** Needs a new journaled fact from
  the API and a spec change; the wait reaches the same result inside one process.
- **Restart an exchange that ended `code_missing`.** Loosens rule 256's single start and spreads
  the timing across restarts.

<a id="dec-2364"></a>

## DEC-2364: An account kill switch still closing keeps the executor running

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-176: the executor exits in fewer cases than DEC-2347 lets it, and running longer adds no risk) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | Lane LV (Claude Code), on claim [#137](https://github.com/kunwarshivam/mandate/issues/137), for E7-4, from [DEC-2347](trading-core.md#dec-2347) item 6 |
| **Story or claim** | E7-4; [DEC-2347](trading-core.md#dec-2347) items 3 and 6; [DEC-2333](DEC-2333.md); [DEC-2337](DEC-2337.md) items 3 and 4; trading-domain spec §5.5; `AGENTS.md` rules 3 and 13 |

### Context

`mandate-executord` exits `completed` when the account owes nothing more (DEC-2347). Its
`owes_nothing` reads every order of the account stream, each followed agent's lots, and the
broker's open orders, but not the account-scope kill switch (DEC-2333): item 6 left that to a
backlog row.

An account switch closes every instrument the account holds, the owner's shares included, by the
broker's close-position, one instrument at a time (DEC-2337). The close is an order of the
broker's own, so the fold holds no order for it. While it works, the broker lists it open and the
executor keeps running. If the broker then expires or cancels it unfilled, the shares are still
held and the switch still owes their close: DEC-2337 item 3 asks it again on the next read the
in-doubt hold takes, every `unknown_absent_window_s`. Between that expiry and that read, every
check passes. The same holds when the close's answer is lost and it then expires.

A close not yet sent because the switch's read is outstanding does not arise there: an account
switch reaches `mandate-executord` only by replay, and its confirming read is the startup
reconciliation (DEC-2333 items 2 and 3), so a read that has not come back leaves the process not
ready, which already holds the exit. A close the broker refuses unsent fails the cycle, a failed
exit, never `completed`. No fold order is working, no agent holds lots (the shares are the owner's, released,
or never an agent's), the broker lists nothing working, and no reconciliation is due. So the
executor exits `completed`, and the close is never asked again.

Today `mandate-executord` hands the core only agent-scope switches, so an account switch reaches
it only as an account stream an earlier process journaled. The gap is therefore latent until the
account scope is handed there, but the exit must not depend on that.

### Invariants first

1. An executor never exits `completed` while an account switch it has read still has an instrument
   to close that the account holds (`AGENTS.md` rule 13, DEC-2347 invariant 1).
2. Once every instrument the switch closes reads flat, the switch owes nothing, and it no longer
   holds the exit (DEC-2347 item 4 does not apply to a switch that is done).
3. The judgement reads the fold alone, so a restarted executor judges the same.

### Decision

1. **A held instrument of an account switch holds the exit.** DEC-2347 item 3 gains a tenth
   condition: no account switch is applied whose closes or deferrals name an instrument the
   fold's position reads above zero (`ExecutorState`'s `closing_all`, the instruments the switch's
   `closes` and `deferred` name, read against its positions, as `kill.rs`'s `reread_due` already
   does).
2. **Flat ends it.** An instrument whose position reads zero or below no longer holds the exit,
   whether the switch's close sold it or anything else did. A switch the owner lifts clears
   `closing_all`, and holds nothing either.
3. **Nothing else changes.** No order, request, read or price changes. The close is asked again
   only as DEC-2337 item 3 asks it.

### Not covered here

- **The re-ask loop.** A close-position the broker accepts and then drops at once (rejected, or
  expired before the next read) leaves the account holding the instrument with no sell listed. The
  read that confirms the accepted answer then asks the close again, and that answer asks for another
  read, within the same pass, until the process's chain bound fails the cycle. The tests PR found
  this in a fake that accepts a close and drops it. The backlog holds a row: ask a close again only
  on the in-doubt hold's reads (DEC-2337 item 3), never on the read that confirms the previous
  request's answer. It is latent for the same reason as the gap above: no process hands the core
  an account-scope switch today. It blocks handing that scope to `mandate-executord` or the shell.

### Alternatives considered

- **Any account switch holds the exit until the owner lifts it.** Rejected: an executor whose
  switch has closed everything would run for ever with nothing to do (DEC-2347's rejected "flat
  account").
- **Request a reconciliation before exiting.** Rejected: it would also find the shares, but only
  by adding a read to every exit, and the fold already knows what is held.

### Consequences

- `mandate-executor`: `ExecutorState` exposes whether an account switch still has a held instrument
  to close.
- `mandate-executord`: `owes_nothing` reads it.
- Tests in `mandate-executord/tests/released.rs`, each pending E7-4 until the implementation (a
  behaviour-only row each), for a switch still holding the exit:
  `an_account_switch_still_closing_keeps_the_executor_running` (its close expired unfilled) and
  `an_account_switch_whose_close_answer_is_lost_keeps_the_executor_running` (its answer lost, then
  expired). `an_account_switch_whose_close_is_refused_unsent_never_exits_completed` holds on
  `main` already. Two more hold on `main` and pin the implementation against holding the exit for
  ever:
  `an_account_switch_whose_close_filled_lets_the_executor_exit` (flat ends it) and
  `an_instrument_the_account_switch_does_not_close_never_holds_the_exit` (only the switch's own
  instruments hold it). `tests/support`'s fake broker accepts each cancel-all and close-position
  when a test asks, a close is an order of its own that works, fills, or expires, and a test can
  lose every close's answer.

<a id="dec-2365"></a>

## DEC-2365: A paper executor runs only as the deployment manager's pod

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: the chart refuses one configuration it rendered before, and adds no account, credential or egress; no safety rule is weakened) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)) |
| **Story or claim** | E21-33 (K2b's leftover in the backlog); [DEC-2336](trading-core.md#dec-2336); [DEC-2306](trading-core.md#dec-2306) item 1; [DEC-1170](DEC-1170.md); [DEC-1172](earlier-900-1999.md#dec-1172) |

### Context

The backlog held a K2b leftover: move the chart's Deployment executor from the `default`
ServiceAccount to its connection's `<hex>` account, so that it trades the account the owner
connected. The chart renders that Deployment only when `images.deploy-manager` is not named. In
every other case the executor is the manager's pod template, and the manager starts it as its
connection's own account. Admission pins that account to the lowercase hex of the connection id,
and to the `MANDATE_SUBJECT_ACCOUNT` the pod names (DEC-2336). Staging's promotion names every
charted image together, the manager's included, so a staging executor is always the manager's.

Giving the Deployment a `<hex>` account would need the chart to make a ServiceAccount without the
manager. DEC-2336 has the manager alone make and delete an executor's account, and the vault's
`executor-{connection}` role is made through the flow the manager drives, so such an executor
still could not log in for its lease. One configuration was nevertheless open:
`executor.broker=alpaca-paper` with no manager rendered an executor that reaches the vault as
`default`.

### Decision

1. **A paper executor needs the manager.** The chart fails to render a Deployment executor whose
   broker is `alpaca-paper`, the one broker that reaches the vault for a connection's lease, and
   names this decision in its message. A paper executor therefore runs only as the pod the
   deployment manager starts as its connection's own account, never as `default` and never under
   another connection's account.
2. **The fixture executor stays a Deployment on `default`.** Without the manager, the fixture
   broker's executor keeps running as `default`, with no vault token and no vault CA. It holds no
   connection and no credential.
3. **No account is added.** The chart makes no ServiceAccount for an executor, and the backlog row
   becomes this rule instead of the move.

### Rationale

The literal move would add a ServiceAccount outside DEC-2336's lifecycle, and it would still leave
the executor unable to log in. Refusing the one configuration that reached the vault as `default`
closes the gap the move was for, with no new identity.

### Alternatives considered

- **Render a `<hex>` ServiceAccount for the Deployment executor from `runtime.connection`.**
  Rejected: it breaks DEC-2336's rule that the manager makes and deletes an executor's account,
  and the vault role it would need is not made without the manager.
- **Drop the Deployment executor entirely.** Rejected for now: a cell that pulls its `mandate`
  image runs without the manager, and its fixture executor needs no account.

<a id="dec-2366"></a>

## DEC-2366: The fixture names its unrouted answers and its issued tokens in words alone

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It changes only the stub issuer's fixture mode, which runs only where `MANDATE_DEPLOYMENT_TIER` is `dev` or `ci` and is compiled out of every build without debug assertions; the answers on the wire do not change, and no safety rule is weakened. The coordinator reserved the number) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)) |
| **Story or claim** | E10-23; [DEC-1179](earlier-900-1999.md#dec-1179) item 5 (amended here, for fixture mode only); [DEC-2625](DEC-2625.md); `AGENTS.md` rule 7 |

### Context

With DEC-1179 in, the connect drill's token exchange still ends `endpoint_failed`, and the
fixture's standard error is empty. DEC-1179 writes a line only for a token refusal, a 415 and a
413. A 421 (another `Host`), a 505 (not HTTP/1.1), a 404 (no route), and an issued token all leave
nothing behind, so an empty log cannot tell a misdirected or unrouted request, or a token the
exchange then failed to parse, from a request that never arrived. The drill's report passes only
lowercase words separated by single spaces, so a line can carry a word but never a number or a
value.

### Decision

1. **The unrouted answers.** In fixture mode, the mode in which the provider runs without the
   sign-in issuer, the fixture writes one line to standard error for each of these answers:
   `fixture_answered misdirected` for a 421, `fixture_answered http_version` for a 505, and
   `fixture_answered not_found` for a 404. The sign-in issuer writes none of them.
2. **An issued token.** Each issued token writes `token_issued` alone, never the token, its scope,
   or anything else from the request or the answer. This amends DEC-1179 item 5, which said an
   issued token writes nothing.
3. **Words alone.** Every line is lowercase ASCII words and underscores, separated by single
   spaces. A test pins the lines byte for byte, checks that every line is words alone, and checks
   that no value from the request or the answer appears in them: the code, the token, the client id
   and secret, the state, the verifier, and the `Host`.
4. **Not written.** The 500 for a clock before 1970 writes nothing, since nothing can reach it. The
   authorize and account routes write nothing still.

### Rationale

With these lines, every answer the drill's token exchange can get has a word in the fixture's log
except a transport failure. An empty log then means the request never reached the fixture.

### Alternatives considered

- **Writing the status number.** Rejected: the drill's filter passes words alone, so a number
  would be dropped.
- **Logging every request.** Rejected: a request carries the code and the client secret, and a
  redaction that misses a field leaks it.

<a id="dec-2367"></a>

## DEC-2367: A shard that runs out of time always reaches the mutation cost history

| | |
|---|---|
| **Status** | Accepted (agent: a reversible CI change under DEC-79. No mutant leaves the gate, no budget grows past DEC-464's ten minutes, and the plan can only size shards smaller; the coordinator reserved the number and approved the plan) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1, claim [#1901](https://github.com/kunwarshivam/mandate/issues/1901)) |
| **Story or claim** | No story; CI throughput. Amends [DEC-2607](process.md#dec-2607) items 1, 2 and 5 |

### Context

On #2416, `ci mutants` planned five `mandate-executor` mutants a shard, shard 0/2 ran out of
DEC-2607's 480 s, and the next run planned five again. DEC-2607 already sizes a package at one
mutant a shard while any of its kept records ran out of time, so the plan repeated because that
record never reached the history the next plan read. There are two ways it was lost:

1. **A lost update.** Each `mutants-costs` job reads the one latest `mutant-costs` artifact,
   folds its own run's records in, and uploads a new one. When two pull requests' fold jobs overlap,
   both start from the same history, and the one uploaded last holds none of the other's records.
2. **A shard stopped before it writes its record.** The 480 s stop counts from the gate's start,
   after the job's setup. When GitHub stops the job at its ten-minute limit first, xtask never
   writes the shard's record, and the upload finds no file.

### Decision

1. **The recent histories are read together.** The plan and the fold job each read the 10 most
   recent unexpired `mutant-costs` histories and fold them into one, oldest first. A record held by
   several is counted once, since the fold keys each record by its run and shard, and the one from
   the latest history wins, so a re-run's clean record replaces its first attempt's `unfinished`
   one. A history that
   cannot be read is skipped and said in the log, never a failure, so the history alone never fails
   a job.
2. **A shard writes a provisional record first.** Before cargo-mutants starts, a shard of more
   than one mutant writes a record whose outcome is `unfinished` and which counts as having run out
   of time. The shard's real record replaces it when xtask finishes, on time or not, so only a
   shard stopped before then leaves it behind, and its packages' next plans size at one mutant. A
   shard of one writes none, since no stop times it and no plan sizes it smaller.
3. **A shard that ran out of time costs its share.** Each mutant such a shard ran is recorded at no
   less than what the stop left after the fixed part, divided by the mutants it ran (at least one),
   so the history stays truthful once the record ages out.
4. **No per-crate floor.** A package with fewer than three clean records already gets one mutant a
   shard (DEC-2607 item 3), so a cold history cannot over-size.

### Consequences

A run cancelled while a shard of more than one mutant is still running, for example by a newer
push, also leaves an `unfinished` record. Its packages then get one mutant a shard until the record
ages out of their last 20. That is more shards, never fewer mutants, so the gate is never weaker.
It costs runner time, which was accepted because a cancel is a weak signal either way.

### Alternatives considered

- **Serialise the fold jobs with a concurrency group.** Rejected: it would not cover a shard that
  wrote no record, and it would queue every pull request's fold behind the others.
- **A per-crate cost floor for `mandate-executor`.** Rejected for now (item 4): it would be a second
  sizing rule to keep in step with the history.

<a id="dec-2371"></a>

## DEC-2371: Every process that reaches a public TLS host gets the pinned CA bundle, and no other container

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79; it grants no network reach, adds no secret, and weakens no safety rule; the DEC number was reserved by the coordinator on 2026-10-25) |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), with the coordinator |
| **Story or claim** | E21-58; related: [DEC-2063](infrastructure.md#dec-2063) item 8 (the cell's token-exchange image), RS's finding 1 (the API's bundle) |

### Context

Every release image is one static binary on an empty base, with no CA roots (`xtask/src/image.rs`,
E21-35). A process whose HTTP client verifies against the platform's roots cannot even build that
client without them. The workspace chart gave a bundle to the API alone, for the identity
provider's key set (RS's finding 1), and `test_cell` pinned exactly that: "only the API gets the CA
bundle".

Staging runs the release images. There, the token exchange could not reach Alpaca's token host, so
every connect would end `endpoint_failed` before any request. The executor could not reach
`paper-api.alpaca.markets`, and the control runs could not reach the identity provider. DEC-2063
item 8 puts a bundle in the local cell's own token-exchange image, which does not reach staging's
pods.

### Decision

1. **The invariant.** Every process whose egress names a public TLS destination gets the pinned
   bundle, and no other container does, sidecars included. The public destinations are
   `identity-provider`, `broker`, `market-data`, `token-host`, `model-providers`, `object-storage`
   and `timestamping`. Today that is `control`, `executor` and `token-exchange`, plus the API by
   its own pod; `model-gateway`, `data` and `exporter` once they are built. The vault, Postgres,
   the Kubernetes API and the cell's own services each have their own CA, which this does not
   touch.
2. **How.** The chart (`mandate.pod`, keyed by `mandate.publicTls`) adds a `ca-bundle` init
   container to such a pod:
   - it copies `/etc/ssl/certs/ca-certificates.crt` from the pinned postgres image into a 1 MiB
     emptyDir;
   - the process's container mounts that emptyDir read-only and names it in `SSL_CERT_FILE`, as the
     API's pod already did.

   The chart, not the image, carries it, so a release image, a cell build and any later image
   behave alike, and the deployment manager's admission lists, built from the templates, admit
   its pods unchanged.
3. **Why the postgres image.** It is already pinned by digest, already pulled on every node for
   the journal and the init containers, and already in every pod's admission list. A new image
   only for a bundle would add a pull, a digest and an admission entry for one file. Its bundle is
   Debian's `ca-certificates`, the public Mozilla set.
4. **How it is refreshed.** The bundle is as fresh as that image. The image's one digest is pinned
   in four files:
   - three values files: `workspace` `images.postgres`, `journal-db` `image` and `archive-store`
     `initImage`;
   - `deploy/cell/compose/compose.json`, generated from the workspace's by
     `cargo xtask cell generate --write`.

   It has a row in `docs/dependencies.md`, and it is bumped by a pull request that moves the three
   values files together and regenerates the Compose file. No bot bumps it today. That bump is due at least each quarter, and at
   once for a CA distrust or a CVE in the image. A backlog row records automating it, with a test that the four agree.
5. **The test.** `test_cell`'s `test_each_public_tls_process_gets_the_ca_bundle_and_no_other`
   reads the public destinations from a table of its own, not from the chart. It holds the
   invariant over every workload and every pod template the deployment manager holds, with seeded
   bugs on the API, the token exchange and the executor, and a leak into the role provisioner.

### Rationale

The bundle is public data: it names whom a process trusts, not where it may go. Each process's
egress stays pinned by its network policy and, on staging, by its FQDN policy, so giving the bundle
widens no reach. Without it, the processes whose whole job is a public call cannot do it.

### Alternatives considered

- **A CA bundle in each release image.** The images are a pure function of their binary (E21-35),
  and every process would then carry roots it never uses.
- **Compiled-in roots (`webpki-roots`).** A new dependency, and roots that only a rebuild of every
  binary can refresh.
- **The token exchange alone.** It would leave the executor and the control runs broken on
  staging.

<a id="dec-2372"></a>

## DEC-2372: The coordinator runs the first paper trade's manual run

| | |
|---|---|
| **Status** | Accepted by the founder on 2026-10-25, who answered the coordinator's question directly in the coordinator's session (relayed by the coordinator, as DEC-529 was). Founder-reserved under DEC-79, since it loosens the brief's "agents never see" the paper credentials |
| **Date** | 2026-10-25 (UTC) |
| **Decided by** | The founder, in the coordinator's session on 2026-10-25, answering the coordinator's question; recorded by lane V2 (Claude Code) |
| **Story or claim** | The first paper trade, [docs/project/tasks/first-paper-trade.md](../tasks/first-paper-trade.md) §"The manual paper run (E2)"; [DEC-502](earlier-303-599.md#dec-502) item 6; `AGENTS.md` rules 7 and 8 |

### Context

The brief's manual paper run (E2) says only the founder runs it, with paper credentials and data
keys the agents never see. The founder has placed an Alpaca paper key and secret in the
coordinator session's environment so that session can run it.

### Decision

1. **The founder authorizes the coordinator session to run the manual paper run** in the brief's
   §"The manual paper run (E2)", using the Alpaca paper key and secret the founder placed in that
   session's environment. No other session receives them.
2. **Paper only**, on Alpaca's paper endpoints. Step 4 (`--place-one-order`) still needs the
   founder's explicit yes in the coordinator session immediately before it (DEC-502 item 6,
   unchanged). One order. Steps 5 and 6 as written.
3. **The keys are never printed, logged, committed, sent to another session, or written outside
   the process environment.**
4. **Live credentials and production secrets stay out of every agent's reach.** `AGENTS.md` rule 8
   is unchanged.

### Consequences

- The brief's E2 line cites this decision; nothing else changes.
- Any session but the coordinator's that is asked to hold or use these keys refuses.

<a id="dec-2373"></a>

## DEC-2373: New backlog rows go one file each under docs/project/backlog/

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane V2), under DEC-79, on the coordinator's ask and its approval of the design; a reversible process decision |
| **Story or claim** | The backlog, [06-backlog-v1.md](../06-backlog-v1.md); `AGENTS.md` "How to work" and the trust ladder |

### Context

Nearly every PR adds a row to 06-backlog-v1.md: a story, or a debt row its review left. Each
landing then makes the next PR that touches the file conflict, and every lane merges main in to
resolve the same kind of conflict again.

### Decision

1. **A new row is one file under `docs/project/backlog/`**, named `<story>-<slug>.md`, holding
   the row as it would read in 06. The directory's README gives the format.
2. **06-backlog-v1.md takes no new rows.** A pointer at its head says where they are. An edit to a
   row already in 06 stays an edit of 06.
3. **Rows are folded in periodically**, by the coordinator or any lane in a docs-only PR that
   moves the rows into 06 and deletes their files in the same change.
4. **Checked, not only guided (rung 2).** `cargo xtask lint` compares 06's rows with the diff
   base's and refuses any row new to 06 that the change does not fold in. A row is a bullet
   opening in bold at the margin, an indented bold bullet naming a story (`E<n>-<n>`), or a table
   row naming one; it is keyed by the story its bold lead opens with, else by that lead, else by
   a table row's first cell, so editing a row keeps its key. A row new to 06 is folded in only
   when the change deletes a row file, not moves it out of the directory, that held that very
   row at the base under a name starting with the row's story, and each file folds its row once.
   With no diff base, as on a push to main, it skips.
5. **Every reader of the backlog reads the directory too.** The schema sweep's path trigger
   (`SCHEMA_SWEEP_INPUTS`) lists both, and the cell scenario's lane check reads both. Because the
   spec guard keeps schema files and code apart, `check_planned.py` (which looks a story up) and
   the schema mutation sweep (which copies the backlog) read the directory in a separate
   schema-only PR. The spec guard
   counts neither as protected or code, and `ci select` treats both as documentation.

### Alternatives

- **`merge=union` on 06.** It keeps both sides of an edited line, so a row whose status changed on
  two branches is left twice, contradicting itself, with no check to catch it; and GitHub's web
  merge and "Update branch" ignore it.
- **An append-only section at the end of 06.** Two PRs appending still touch the same last lines
  and conflict.

### Consequences

- PRs that add rows no longer conflict on the backlog.
- Until a fold, a row lives in the directory; the story playbook and `AGENTS.md` name both places.

<a id="dec-2375"></a>

## DEC-2375: How the web app binds a `cli_confirm` step-up to the action it shows

| | |
|---|---|
| **Status** | Accepted (agent: reversible web engineering readings under DEC-79 and DEC-176; each only tightens) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code`, lane W, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443); number given by the coordinator |
| **Story or claim** | E11-9 slice W-2a ([#2435](https://github.com/kunwarshivam/mandate/pull/2435)), W-4 ([#2453](https://github.com/kunwarshivam/mandate/pull/2453)); workspace API spec §3.6, §5.2; [DEC-2103](DEC-2103.md), [DEC-2104](DEC-2104.md), [DEC-2125](DEC-2125.md) |

### Decision

1. **One challenge per press.** Approve, and Connect with Alpaca, each ask `POST /step-up/challenges`
   under a new key when pressed. Nothing is asked ahead of the press, and a challenge is never reused
   for a second action.
2. **The answer must name the action asked.** A challenge whose `action.kind` or `action.digest`
   differs from the one sent is refused as an unreadable answer. No code is shown for it.
3. **The grant must name the challenge's action.** An approve is prepared only with a challenge
   of kind `approve` whose digest is the approval's content hash. A connect's start is prepared
   only with a challenge of kind `connection`. Anything else is refused before sending, as
   `step_up_mismatch`, and a Skip never carries step-up (API-7).
4. **`authenticated_at` is the challenge's `issued_at` exactly as served.** It is never rounded,
   since the server compares instants at full precision.
5. **A code's life is counted on this device from when it was shown.** Its length is
   `expires_at − issued_at`, and the device's clock is never compared with the server's instants.
   A code whose time has passed cannot be sent from the screen; the server judges the window
   itself.
6. **The command line names the journal and store as placeholders.** The deployment's DSN and
   store path are the operator's, never the browser's, and no clipboard copy is offered, as
   everywhere in the app.

### Rationale

Each item keeps what the owner sees and what the server is asked for bound to one action. The
binding is checked at each end the browser controls: what it is answered, and what it sends. None
of this changes a server rule.

### Alternatives considered

- **Fetch a challenge when the screen opens.** This saves a round trip, but spends the 300 s life
  before the owner has chosen, and holds a challenge for an action that may never be taken.
- **Compare `expires_at` with the device's clock.** A skewed device would show a live code as
  expired, or an expired one as live.

<a id="dec-2376"></a>

## DEC-2376: Where the browser goes in an Alpaca connect, and how it comes back

| | |
|---|---|
| **Status** | Accepted (agent: reversible web engineering readings under DEC-79 and DEC-176; items 3 and 4 tighten) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code`, lane W, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443); number given by the coordinator |
| **Story or claim** | E10-13, E11-9 slice W-4 ([#2453](https://github.com/kunwarshivam/mandate/pull/2453)); workspace API spec §4.5; [DEC-2620](DEC-2620.md), [DEC-2621](DEC-2621.md), [DEC-2622](DEC-2622.md), [DEC-2623](DEC-2623.md), [DEC-2626](process.md#dec-2626) |

### Decision

1. **The disclosure comes first, in the founder's words.** Connections shows DEC-2622's version 1
   text before anything is asked, and the step-up digest binds that version (DEC-2621 item 4). A new
   wording is a new version in the web app's constant and its component together.
2. **The start is sent once per confirmed code.** A lost answer is sent again only with the same key
   and body, which answers the same start (DEC-2623 item 8). It is never sent under a new key, and an
   answer that does not say nothing was recorded is reported as unknown.
3. **The browser is sent only to an `https` page.** That is what `commands/oauth-start-response`
   states, so a page of any other scheme is refused and the browser stays in the app. A cell's
   `http` fixture page is refused too, until the schema admits it.
4. **The return lands on `/connections/{id}`.** The API's callback redirects there (DEC-2626). The
   page reads `GET /connections/{id}` until the connection leaves `connecting`. Only an
   established connection is the workspace's connection, and so the scope of Stop-all and
   Close-all. With none established, those take the workspace scope (spec §5.4).
5. **Owner only.** The connect is offered to the owner alone, the most conservative reading of
   §3.7's row "Connect, change, or revoke a broker connection".

### Rationale

The browser never holds the authorization code, the `state` or a token. All it carries is a URL
the API answered, and its only way back is the API's own redirect. Items 3 and 4 keep a malformed
answer, or a pending or refused connect, from becoming somewhere the browser goes or the target of
a kill switch.

### Alternatives considered

- **Treat the first connection listed as the workspace's.** That was the earlier reading, and a
  pending connect listed first would have been Stop-all's scope.
- **Admit `http` loopback pages for the cell.** That is looser than the schema. It belongs in a
  schema change, not in the client.

<a id="dec-2377"></a>

## DEC-2377: How the web app shows an agent's served orders, positions and journal

| | |
|---|---|
| **Status** | Accepted (agent: reversible web engineering readings under DEC-79 and DEC-176) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code`, lane W, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443); number given by the coordinator |
| **Story or claim** | E11-9 slice W-6 ([#2459](https://github.com/kunwarshivam/mandate/pull/2459)); workspace API spec §4.7 D3, §4.8.1; [DEC-2111](DEC-2111.md), [DEC-760](DEC-760.md) |

### Decision

1. **Read on the agent's screen only.** `GET /agents/{id}/orders`, `/positions`, and the agent
   stream's tail (`GET /journal/streams/agent:{ws}:{id}/events`) are read when the screen opens and
   every 5 s while it stays open. They are not part of the workspace refresh.
2. **Not reported is never none.** A read that fails shows "not reported". So does an answer naming
   another agent or stream, and an `account_ref` of `null`. Only a served, empty list shows as
   "No order yet" or "This agent holds nothing".
3. **Nothing is filled in.** A `null` state or filled quantity is shown as not reported. `unknown`
   is shown as unknown. Cost basis, risk mark and protection are not shown until their folds are
   served.
4. **The journal shows types and times, never bodies.** The tail is the stream's last 20 events.
   One page of one event finds the head, and a second page reads up to it. Each line is the
   sequence, event type and time. No body is decoded, so no model-authored text reaches the screen
   (API-18). A 403 for the role says "Not available to your role".

### Rationale

The trade loop's evidence, the order, its fill, and the records the gate and executor wrote, is
shown from the deployment's own reads. A read that cannot answer never passes for an empty record
(rule 3).

### Alternatives considered

- **Fold the reads into the workspace refresh.** Every screen would pay for one agent's history.
- **Decode journal bodies for richer lines.** That would need a per-type renderer and API-18's
  quoting for model text. It is a later slice, if the owner needs more than types and times.

<a id="dec-2379"></a>

## DEC-2379: The demo's scenario records its paper connection, so the manager starts the agent's processes

| | |
|---|---|
| **Status** | Accepted (agent: a reversible change to the demo's scenario in `xtask`; paper and the in-process fixture broker only, no credential, and no trading path changes) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane DR (Claude Code), number from the coordinator, for E21-54 |
| **Story or claim** | E21-54 (the demo end to end); [DEC-2002](infrastructure.md#dec-2002), [DEC-2004](infrastructure.md#dec-2004), [DEC-2010](infrastructure.md#dec-2010); [DEC-2035](infrastructure.md#dec-2035) (the executor drill); DEC-1171 (a version-2 deployment is what the manager starts); journal spec §9.24 rule 201 |

### Context

`cargo xtask cell demo --scenario demo` deployed `demo-spy` with no connection recorded, so its
`AgentDeployed` was version 1 and the deployment manager started nothing. Every step after the
deployment named "X (E21-30): the executor's connect check" as its gap. That check landed on
2026-10-19 (#1759, #1763), and the `executor` drill already uses it: the connect check on the
fixture broker, `connection record`, then a version-2 deployment, whose executor the manager
starts and which answers `livez`. The scenario never picked it up, so the demo showed a kill
switch that no running process received.

### Invariants first

1. The scenario never fakes a step: a check passes only on what the cell itself answers.
2. Paper and fixtures only: the connect check reaches the executor's in-process fixture broker,
   and no credential is read (`AGENTS.md` rule 8).
3. Every typed code is the code the CLI showed just before it (DEC-530 item 4).
4. A run never depends on an earlier one: its account is fresh, as its workspace is.

### Decision

1. **The connect check runs before the gestures' Job.** `cell::scenario::run` runs the
   executor's `connect-check` as its own one-off Job, for the run's workspace and a fresh account
   (`01KDEA<run id>000000000A`), using the drill's code (`cell::executor::connect_check`). A check
   that does not pass fails the run. A cell that pulls its images (`MANDATE_CELL_IMAGE`) has no
   executor or deployment manager image, so it runs no check and plans as item 7's dry run does.
2. **Step 2 records the connection first.** `connection record` of `conn_demo_paper_01` (Alpaca,
   paper, scope `trading`), its code shown, then typed. The deployment that follows is version 2.
   The code is the CLI's (`record_code`, pinned to the code the CLI shows).
3. **Step 2 checks the executor.** After the Job, the manager's executor pod for the run's account
   must run and answer `livez` (`cell::executor::executor_live`, the drill's own poll).
4. **Step 7 checks the kill switch applied.** After the Job, the CLI's agent-scope kill switch of
   `demo-spy` is read back with the owner's session, `GET /v1/workspaces/{ws}/commands/{id}`,
   every two seconds for up to 90 seconds, until its phase is `applied`, which only a
   `KillSwitchActivated` on the agent's stream gives (the runtime's copy). Its gap is gone. The
   web read of the second agent's kill switch still accepts any accepted phase.
5. **The exit's code binds one more record.** The connection record is a control record, so the
   head the owner's exit binds is 10 (`EXIT_CONTROL_HEAD`).
6. **What still waits.** The owner's exit waits on the scheduler (E21-29), whose tick makes the
   executor ready. Fills wait on X1b-3 and the post-R0 hook (DEC-2378). The research workspace
   still deploys at version 1 until it records its own connection, which is lane DR's next.
7. **The staging dry run names no connection.** It reaches no cluster, so it runs no connect
   check: step 2 names the connection's gap, the exit binds head 9, and step 7 keeps its gap.
   Staging's connection comes from the owner's OAuth connect.

### Not covered here

- The API-level OAuth connect (`oauth/start`, the fixture, the callback) in the scenario: lane DR's
  next slice.
- Whether the executor applies an agent-scope kill switch while it is not ready: it is not ready
  until a scheduler tick (DEC-2035), so step 7 checks the runtime's copy only.

<a id="dec-2380"></a>

## DEC-2380: The cell's executor fills a paper order on the fixture broker, on real ports

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79; scope set by the coordinator on 2026-10-26. It adds no risk: every opening still passes the binding gate and the mandate, and only the broker is a fixture) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lane LV for the fills and lane D1 for the shared gate context (DEC-2378) |
| **Story or claim** | E21-30 slice X1b-3 ([backlog row](../backlog/E21-30-x1b-3-fixture-fills.md)); [DEC-2007](infrastructure.md#dec-2007), [DEC-2305](DEC-2305.md), [DEC-2306](trading-core.md#dec-2306), [DEC-2359](trading-core.md#dec-2359) |

### Context

`mandate-executord` runs `fixture::wiring` in every mode: no mandate, no instrument, no
binding-gate fact and no protection, so the gate holds every opening, on the fixture broker and on
Alpaca's paper host alike (`credential::paper_wiring` swaps only the quotes). The fixture broker
never fills, and no process writes the `MarkUpdated` version 2 the risk fold and the daemon's gate
context read (journal spec §9.30), which `mandate-journal` does not yet register. So no paper
order can fill in the cell, and the founder's end-to-end trade cannot run.

### Decision

1. **`MarkUpdated` version 2 is registered** as journal spec §9.30 closes it: version 1's members,
   then `bid` (`decimal?`), `sane` (boolean) and `session` (`overnight`, `pre_market`, `regular`,
   `after_hours` or `crypto`), with `risk_clock` last, and rule 244: a sane mark carries a bid
   greater than 0. Version 1 stays registered. The vectors' `risk_marks` section is the test. No
   spec change.
2. **The executor's real ports.** The executor's mandate view, instrument snapshot (with the fill
   references the journal requires of a `FillApplied`), binding-gate source, protections and quote
   source are built from the journal and the deployment's mandate document in place of
   `fixture::wiring`'s, for the fixture and the paper broker alike. The binding gate's account,
   mandate version and mark come from `mandate-gate-context` (DEC-2378), the one fold the daemon
   uses too, never a second one. The asset record, the account's day-trade rules and liquidity,
   which the daemon's advisory gate defers to the binding one (DEC-2378 option b), come from the
   broker or the cell's fixtures, and an opening missing any of them is held. So are the agent's
   journaled risk (its risk snapshot, mode, restrictions, exits' last fills, orders today, day
   trades and conduct counters, from the account risk view) and the instrument's halt status:
   the gate never judges a limit on an assumed figure, so an opening without them is held. The
   account's positions and working orders are not yet read, so an opening is answered only from
   a flat account, and only where the account's day trades could not bind: under §9.2's
   `intraday_margin`, which counts none, while the broker reports no maintenance deficit (a
   deficit puts the account's agents in exits only), or under the legacy regime at or above its
   equity threshold; only a US-equity deployment is judged. An order that
   reduces risk is never answered: the core keeps its own decision (`AGENTS.md` rule 13).
   The process reads its deployments as the paper shell does, from the control stream's
   confirmed version and registrations and the read-only artifact store (`MANDATE_ARTIFACTS`),
   each port reading the store when asked (`deployment::Deployment`); the stored bars come from
   `MANDATE_BARS` (DEC-2274). While nothing has traded on the account, the agent's allocation is
   its equity and every counter is empty, so the gate reads those; once anything has traded the
   opening is held until the account risk view reads them, and the gate judges only the version
   the journal has in force ([DEC-2802](trading-core.md#dec-2802)). The broker's asset record, halt status
   and day-trade rules are unread until the broker serves them, so until then every opening is
   held.
3. **Marks.** The executor journals `MarkUpdated` version 2 on its account stream for each deployed
   instrument from its quote source at each scheduler tick, judged as trading spec §8.2 says.
   The mark is stamped with that tick's clock, never the wall clock: a pass that reads no newer
   tick marks nothing new. The feed judges a quote's freshness against its own wall clock, the
   instant it reads the quote, not against the risk clock. A quote it reads but judges not sane
   (crossed, or wider than the profile's spread) is marked `sane` false at its bid, so a held
   position is restricted at once (§9.30); the process still hands the core none of those
   (DEC-2359). A quote it cannot read, or one older than its bound, gives no price and no mark.
   The tick is handed before the marks, so a mark's append never holds the pass's exit ladder.
   A mark whose append goes in doubt is kept and offered again, the same bytes, before any other
   append on the next pass (journal spec §5.1), so one that committed with its answer lost is found
   committed rather than stopping the executor on a stale head.
4. **Fills on the fixture broker** (lane LV). The fixture broker answers a placed limit or bracket
   opening with a fill at its limit, keeps the protective stop leg resting, and reports the fill as
   Alpaca's activities, positions and account would, so the executor's reconciliation books it.
   Its bodies are hand-built from Alpaca's documented shapes, with provenance rows, in place of
   DEC-2306's recorded LV-3 bodies, which need live paper credentials. The fixture refuses a sell
   larger than the holding less what open sells already hold (an OCO's or a bracket's legs counted
   once), with Alpaca's 403 `40310000` "insufficient qty available for order", and decides before
   changing anything. So while the protective legs rest, an exit or a second protective order over
   the same shares is refused, as on Alpaca; the executor's §5.4 sequence (cancel the agent's
   protection, confirmed by read-back, then send the exit) is what lets an exit through. The
   fixture also answers `GET /v2/orders/{id}`, the client's read-back of a cancel by broker id.
5. **One reading of the gate's reviewed inputs.** The rule set's gate and executor configurations
   and quote age, the fee schedule, the config artifacts' exact members, their references, the
   mandate's limits as the gate reads them, and the fee reservation move unchanged from
   `mandate-shell`'s paper assembly into a new pure crate, `mandate-gate-inputs` (layer 7, above
   `mandate-executor`, whose configuration it parses; `mandate-ports` may not reach the executor).
   The shell calls them with its reason codes unchanged, and the executor process reads the same
   functions for item 2, since it may not depend on the shell.
   The control stream's fold of the confirmed version, the effective registrations and the
   approver set (`control`, `approvers`) moves with them, whole, so the executor finds its
   deployment's mandate and configuration the way the shell does. The reading of one
   deployment's reviewed artifacts from them (`Reviewed::from_registered`) moves too; the shell's
   `Artifacts` adds only the broker's exchange and the approver set. So do the executor ports'
   reading of one deployment (`DeployedView`: its mandate version, its instrument, the
   configuration its fills name, and the protection an opening is handed with), and the shell's
   fill references, stop-limit offset and protection prices, which now call it.

### Rationale

The founder's demo needs one real trade loop in the cell with only the broker a fixture. Building
the executor's ports once, for both brokers, means the staging paper path gets them too, and taking
the gate's facts from the daemon's own fold keeps the two processes' views of an opening the same.

### Alternatives considered

- **A fixture-only gate that allows every opening.** Rejected: it would prove nothing about the
  gate, and a fixture admitting what the mandate refuses must never reach a paper path.
- **Keep `MarkUpdated` at version 1.** Rejected: version 1 carries no verdict, so the risk fold and
  the gate context ignore it and every opening stays held.

<a id="dec-2381"></a>

## DEC-2381: Cloudflare Access passes CORS preflights alone to staging's API

| | |
|---|---|
| **Status** | Accepted (founder: the web app calls the API across origins with credentials; the Access setting that makes it work is an agent's reversible engineering choice under DEC-79 that admits no request carrying credentials or reading data); item 1's description corrected 2026-10-26 (R2); the decision is unchanged |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | the founder (cross-origin calls from `app.owlhead.ai`, 2026-10-26, relayed by the coordinator); the setting, an agent under DEC-79 (`claude-code`, lane GO) |
| **Story or claim** | E21-56; PR #2429 |

### Context

The web app at `https://app.owlhead.ai` reads and writes the workspace through
`https://api-staging.owlhead.ai` with the browser's own session cookie (workspace API spec §3.3,
`web/src/lib/data-source.ts`). That is a cross-origin request with credentials, so the browser first
sends a CORS preflight: an `OPTIONS` request that carries no cookie and no `Authorization` header.

Cloudflare Access sits in front of the API host and admits only the founder's account members
(DEC-2030 item 2). Without a session it answers every request, the preflight included, with a
redirect to its login. A browser treats a redirected preflight as failed and never sends the real
call, so the web app could not reach the API at all.

The API already answers CORS itself, exactly for its configured `appOrigin` and with credentials
(`mandate-api-server`, `http.rs`).

### Decision

1. **Only `OPTIONS` bypasses Access.** The `api` Access application sets
   `options_preflight_bypass = true`. Access then forwards every unauthenticated `OPTIONS` request
   on the API host to the API, preflight or not. `GET`, `POST` and every other method still need
   the Access session, exactly as before.
2. **The API answers every such request without reading anything.** A preflight (`OPTIONS` with
   `Origin` and `Access-Control-Request-Method`) gets a `204` with an empty body, carrying CORS
   headers only when the origin is exactly `appOrigin`. Any other `OPTIONS` gets the same `401` as
   an unauthenticated `GET` (workspace API spec §3.3 item 5). A preflight carries no credentials,
   so nothing in it can read API data or change state. `mandate-api-server`'s
   `a_preflight_from_the_app_origin_is_allowed_and_reads_nothing`,
   `a_preflight_from_any_other_origin_gets_no_cors_header` and
   `an_options_call_that_is_not_a_preflight_is_the_one_401` pin it.
3. **The API alone names the allowed origin.** The Access application sets no `cors_headers` of its
   own, so there is one place that says which origin may call with credentials: the API's
   `appOrigin`.
4. **How it is pinned.** `deploy/tofu/modules/cloudflare/tests/policy.tftest.hcl` requires
   `options_preflight_bypass == true` and `cors_headers == null` on the `api` application;
   `cargo xtask ci tofu` runs it.

### Rationale

The bypass is the narrowest change that lets the browser make the call: one method, which by the
CORS standard carries no credentials, on the one host, answered by an API that reads nothing for
it. Setting `cors_headers` in Access instead would answer preflights at the edge, but would put a
second list of allowed origins beside the API's, which could drift from it.

### Alternatives considered

- **Access `cors_headers` with `allow_credentials` and the app's origin.** Rejected: two places
  naming the allowed origin, as above.
- **Serving the API under the app's own origin through a proxy.** Rejected for v1: the app would
  then hold or forward the session, and the web app has no route in the API's place
  (`web/src/app/v1/[...path]/route.ts` answers 404 in a production build).
- **A bypass policy for every method.** Rejected: it would remove Access from the API host.
- **A bypass for preflights alone.** Access has no such setting: its bypass is by method, so the
  API's own `401` covers the `OPTIONS` requests that are not preflights.

### Consequences

- Applying it is the founder's `infra` dispatch: `plan` on `staging` should show one in-place
  update of `cloudflare_zero_trust_access_application.api`, `options_preflight_bypass` from `false`
  to `true`, and nothing else; then `apply`.
- Production's Access application takes the same setting from the same module.

<a id="dec-2382"></a>

## DEC-2382: The cell's executor may reach Robinhood for the founder's own account (replaces DEC-2306 item 1)

| | |
|---|---|
| **Status** | Accepted by the founder on 2026-10-26, who answered the coordinator's question directly in the coordinator's session (relayed by the coordinator, as DEC-529 and DEC-2372 were). The founder then answered the three questions items 1 to 5 left open, in the coordinator's session on 2026-10-26 at 08:13 UTC: "Accept all three (Recommended)" (items 6 to 8; item 9 orders the work). Item 8's paragraph "DEC-529's 'for this order only' items" was lane LV's reading, held Proposed; the founder accepted it on 2026-10-26 ("Carry over, $100 cap (Recommended)"), recorded in [DEC-2135](workspace.md#dec-2135). Founder-reserved under DEC-79: live money and narrowing a safety rule |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | The founder, in the coordinator's session on 2026-10-26, answering the coordinator's questions; items 1 to 5 recorded by lane V2 and items 6 to 9 by lane LV (Claude Code) |
| **Story or claim** | E7-6, E7-15, E7-16, E21-30; replaces [DEC-2306](trading-core.md#dec-2306) item 1; amends [DEC-529](DEC-529.md); [DEC-531](DEC-531.md), DEC-124, [DEC-441](DEC-441.md), [DEC-964](earlier-900-1999.md#dec-964), DEC-2386 ([#2473](https://github.com/kunwarshivam/mandate/pull/2473)); `AGENTS.md` rules 2, 3, 7, 8, 12 and 13 |

### Context

DEC-2306 item 1 lets `mandate-executord` reach its in-process fixture or Alpaca's paper trading
host and nothing else, and `tests/paper_only.rs` pins it. DEC-529 allows one founder-run live
order on the founder's own Robinhood agentic account, through the paper path's deployment runner,
with the runner the only crate that may declare a `live` feature. The founder wants their
Robinhood orders to go through the cell's executor, the same process that places paper orders,
rather than a path of its own, and wants the agent to keep running after its first order.

Items 1 to 5 left three questions open. The coordinator asked: "For Robinhood (DEC-2382, PR
#2434), do you accept the three recommendations? (a) A `simulated` environment for rh-sim, since
Robinhood has no paper environment. (b) The Robinhood credential kept in the vault. (c) Lifting
DEC-529's 'one live order then stop' for an always-on agent, keeping the $100 cap, one ETF and ASK
on openings. Until you answer, the cell trades on the Alpaca fixture broker only." The recommended
option's note read: "Lane LV builds the rh-sim path, then the vault credential. Live orders still
need your own run under DEC-529's other items." The founder answered, verbatim: **"Accept all three
(Recommended)"**. Items 6 to 9 record that answer and what it changes.

### Decision

1. **`mandate-executord` may carry the Robinhood connector beside Alpaca paper**: `mandate-robinhood`,
   its capability profile ([DEC-531](DEC-531.md)), and its MCP client with the pinned tool allowlist
   (E7-16). It may do so only for the founder's own Robinhood agentic account, whose mandate holds
   every narrowing DEC-529 sets. This replaces DEC-2306 item 1's "the fixture or Alpaca's paper
   trading host, and nothing else" for that one account; for every other connection item 1 stands.
2. **Agents build and test it only against the Robinhood simulator** (`mandate-rh-sim`, DEC-124).
   `AGENTS.md` rule 8 is unchanged: no agent places a real order, holds or reads a live
   credential, or runs the live deployment.
3. **Going live is the founder's alone.**
   - The credential will be the founder's own, placed in the vault by the founder (item 7).
   - The founder flips the live switch.
   - Every opening waits for the founder's grant in the approval flow, bound to the order's
     content hash (mandate spec §6.1).
   - DEC-529's cap holds: `max_order_usd` 100 USD, on one broad ETF.
4. **The executor's `live` build is a separate artifact, gated explicitly.** CI only compiles it,
   as DEC-529 item 3 has it for the runner today (as [DEC-964](earlier-900-1999.md#dec-964) narrows it). The
   `xtask/layers.toml` `live_feature` change that lets the executor declare it lands in the same
   story as the code, under this decision. The default build holds no live host. No agent and no
   CI job builds the live artifact for running or deploys it; the founder does.
5. **Every safety rule and every gate is unchanged.** That covers the confirmed mandate version,
   the order builder, the risk gate with the US account rules, the account ledger, journaling
   before acting, idempotency, tests first (DEC-77), an independent review on a different model,
   and zero missed mutants. DEC-529's other items stand as written, except where items 6 to 9
   change them.
6. **(a) A `simulated` environment for rh-sim.** Robinhood has no paper environment (DEC-441
   item 11, connections spec §4), and its paper stage is the simulated broker with Robinhood's
   rules (DEC-124). A connection to `mandate-rh-sim` is recorded with its own environment,
   `simulated`, beside `paper` and `live`, so nothing reads a simulator connection as live or as
   Alpaca paper. A `simulated` connection reaches only a loopback `mandate-rh-sim` and holds no
   credential. The value is added by a spec-only PR before the code, which amends every rule
   keyed on `paper` or `live`: connections spec §4 ("`paper` or `live`, for life") and CN-3, the
   mandate schema's `environment` enum, V-001, and DEC-155 item 4. Until that PR lands, and
   wherever a rule keyed on environment is not yet amended, `simulated` is never read as `live`.
   A `simulated` run is the Robinhood paper stage DEC-124 names; it is not Alpaca paper.
7. **(b) The Robinhood credential is kept in the vault.** On the executor's path the founder's
   Robinhood credential is held in the vault, not only in one connector process's memory on the
   founder's machine (DEC-529 item 10). Only the executor process takes its lease, never an agent
   (DEC-2306 item 2, CN-1). It still reaches no repository, log, journal, artifact, fixture or
   agent context (`AGENTS.md` rule 7). Agents design and test the vault path with fixture
   credentials only; putting the live credential in the vault is the founder's own step. The
   design is its own decision.
8. **(c) An always-on agent, not one order then stop.** For the founder's own Robinhood agentic
   account only, the agent may keep running and place further orders. This lifts DEC-529's "one
   order (item 1), then stop" (item 13), item 13's "A restart or a second run submits nothing",
   item 1's attempt counting, and item 3's spent connection record that enforced them. What
   stays:
   - `max_order_usd` 100 USD, on one broad ETF (DEC-529 item 2), in the confirmed mandate;
   - the mandate's autonomy rules say `ask` for every opening, so each opening waits for its own
     grant from the founder in the approval flow, bound to the order's content hash (mandate spec
     §6.1); an unanswered ask times out to no order (`AGENTS.md` rule 3);
   - every other DEC-529 item, and items 3 and 4 above: going live, and the live switch, stay the
     founder's alone.

   **Starts and restarts.** The agent's first start still needs a flat account (DEC-470 item 1;
   DEC-529 items 3 and 11). A restart of the same running agent on the same account is not a new
   run: it reconciles first; until the account ledger reconciles with no `Unknown` order it places
   no new opening, while exits and protective orders follow `AGENTS.md` rules 2 and 13 unchanged.
   A restart means the same agent id on the same account stream. An agent the founder stopped
   while it held a position is not restarted by a new deployment: a new agent needs a flat
   account, so the founder exits the position first.
   DEC-529 item 4's rule that an `Unknown` order blocks every new order in its instrument until
   the founder reconciles it stands.

   **DEC-529's "for this order only" items.** The narrowings and resolutions DEC-529 grants for
   its one order (item 3's narrowings, item 4's recovery rule, items 5, 8 and 12) apply to every
   order of this one always-on agent on the founder's own account, and to no other agent or
   account. This is the reading lane LV took as the one an always-on agent needs; it loosens, so
   under DEC-176 it is recorded here as **Proposed**, for the founder to confirm or narrow. Until
   then no live order is placed, since going live is the founder's alone (item 3). Item 9's
   counsel answer stays the founder's own call for their own account, unchanged.

   *Risk:* real money moves over time rather than once. Each order is bounded by the 100 USD cap
   and the one ETF, and each opening by its own grant; the agent's total exposure is bounded by the
   allocation, `max_position_usd` and `max_position_fraction` the founder confirms in the mandate,
   which the founder sets at an amount they accept to lose. DEC-529 item 3's mitigations change:
   the token moves from memory to the vault (item 7) and no spent record stops the agent; the build
   stays founder-built from `main`, unsigned, with no M13 test.
9. **Order of the work.** Lane LV builds the rh-sim path first (DEC-2386) with the `simulated`
   environment (item 6), then designs the vault credential (item 7).

### What this amends

- **DEC-2306 item 1**, as item 1 above says. Its status line points here.
- **DEC-529 item 3's "only the runner may declare a `live` feature"**: the executor may declare one
  too, under item 4.
- **DEC-529 item 10's credential handling**: on the executor's path, the founder's Robinhood
  credential is held in the vault (item 7) rather than only in one connector process's memory on
  the founder's machine.
- **DEC-529 items 1, 3 and 13, and its "Revisit when"**: the one-order limit, the restart rule, the
  spent connection record, and the lapse of item 3's narrowings at the first order are lifted for
  the founder's always-on agent (item 8). Item 3's narrowings lapse instead when the founder stops
  the agent or the M8 path replaces the `live` feature. DEC-529's title, item 1 heading and
  Rationale read "one order" as "one order per grant" for this agent. DEC-529's status line points
  here.
- **DEC-529's "for this order only" items** (item 3's narrowings, item 4's recovery rule, and
  items 5, 8 and 12): extended to every order of the founder's always-on agent, as item 8 reads
  it; Proposed until the founder confirms. Item 9 is unchanged.
- **DEC-441 item 11 and DEC-124**: unchanged. Item 6 names the simulated broker's environment;
  it adds no route to Robinhood.

### Rationale

The founder wants the product, not a one-off proof: an agent that keeps working inside its
envelope on the founder's own account. Running Robinhood through the cell's executor keeps one
production path for paper and live. A `simulated` environment keeps a simulator connection from
being read as live or as Alpaca paper. The vault keeps the credential out of every agent's reach
while letting the executor run unattended. The cap, the one ETF and ASK on every opening keep each
order small and the decision with the person whose money it is.

### Alternatives considered

- **Accept (a) and (b) only, keeping "one live order then stop":** offered to the founder and
  declined. It proves the path once but is not the always-on product.
- **Not yet:** offered and declined. Robinhood would stay parked and the cell would use Alpaca
  paper and the fixture broker only.
- **Keep the runner's separate live path (DEC-529 as written):** declined by the founder in items
  1 to 5, as a path of its own beside the production executor.
- **Read rh-sim connections as `paper`:** rejected by lane LV for item 6; it would let a simulator run pass
  where Alpaca paper is meant, and blur DEC-124's distinction.

### Consequences

- The code is lanes X's and LV's: the executor's connector wiring, the `live` feature and its
  `layers.toml` entry, and the change to `tests/paper_only.rs` that the feature needs. This
  decision changes no code; `tests/paper_only.rs`'s doc points here until that story lands.
- The founder's orders run through the production executor rather than a path of their own, and
  the agent keeps running after its first order (item 8).

<a id="dec-2385"></a>

## DEC-2385: One trust rule for stored bars below the shell and the runtime, and the cell's data process stores the recorded series

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering decisions under DEC-79 and DEC-176. Item 1 moves a rule unchanged and keeps it safety-critical; items 3 to 6 add a fixture only the local and CI cell can name; nothing loosens a limit or a safety rule) Items 3 and 7 are superseded by [DEC-2283](DEC-2283.md) for the `alpaca-iex` source: on managed cells the data process reads Alpaca IEX bars with the platform data key, not the account's connection, and so holds a vault identity, a read of the control stream's registrations and the data host's egress. The recording keeps item 3. |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane SC); the coordinator reserved the number and approved the plan, agreed with lanes D1, DR and X |
| **Story or claim** | E21-29 lane SC (the cell's `data` process); data-plane spec §3.6, §3.7; design §3.1; DEC-846 items 2 and 3, DEC-470 item 4, DEC-503 items 5 and 6, DEC-961 item 3, DEC-1016, DEC-2383 |

### Context

The first ASK in the local cell needs bars. The demo strategy's model host reads the last
completed sessions' daily closes. The advisory and binding gates read the 20-day median dollar
volume, ADV and the prior close from daily bars, and `trailing_5m_volume` from minute bars. Today
only `mandate-shell` reads stored bars, through a trust check that is `pub(crate)` there. The
runtime process may not link the shell (`xtask/layers.toml`), and both sit at layer 8. The cell
has no network bar source (the executor's broker is in-process, DEC-2007), and no credential may
be read (rule 8). Data-plane spec §3.6's host-local push channel is E21-44, which needs its own
decision and security review first.

### Decision

1. **`mandate-bars`, layer 7, safety-critical.** The trust check on a stored daily-bar dataset
   (`trusted_daily_bars`, `trusted_daily_closes`, with `trusted`, `split_inside` and
   `listed_days`) moves unchanged from `mandate-shell`, and its tests move with it. The shell calls
   it; `Cause` maps its `BarsError` variant by variant, so every refusal keeps its words. It sits
   below both the shell and the runtime process, so every reader shares one rule. It stays under
   the mutation gate; `mandate-marketdata` is not safety-critical, so the rule does not move there.
2. **One closes object.** `closes_object`, the canonical bytes an `ObservationRecorded`'s
   `data_ref` digests, moves unchanged from `mandate-paper` into `mandate-bars`. The runner, the
   runtime process and the data process build the same bytes from one function.
3. **The cell's data process stores the bars.** `mandate-datad` runs as the chart's `data` process.
   It stores each listed instrument's daily bars as `mandate-marketdata`'s dataset under the
   artifact store's `datasets/cell-recorded/`, at start (its start-up check) and every 60 s. It
   holds no journal role and no vault token, appends to no stream, and reaches no host. It mounts
   the artifact store read-write, the only process besides `control` (and the API's layout step)
   that does. It writes only `datasets/` and content-addressed objects, whose put never replaces
   one. A sub-path mount cannot hold both, which item 5 needs. Every new reader of the bars
   (runtime, executor) mounts the store read-only.
4. **The cell's series is a recording, never market data.** The last 30 completed regular
   sessions. Each session's open, high, low, close and VWAP are one price: the anchor's close plus
   0.1 for each trading day after the anchor (less before it), at 50,000,000 shares a session.
   Because a close depends on its date alone, a stored session never changes, so a later clock
   adds sessions and rewrites none. The chart requires `data.instruments` with the image, a list
   of `{symbol, assetId, anchor}`, each symbol once: `assetId` is the instrument id its closes
   object names (item 5). Only the local cell's values list any: the demo agent's SPY
   (`2026-10-23=150`) and the second demo agent's QQQ (`2026-10-23=140`), on which the trade
   loop's browser spec runs. 150 and 140 each keep one share inside the demo mandates' 300 USD
   order cap with whole shares. The rising series puts the
   crossover's fast mean above its slow one on every session, as DEC-961 item 3's rehearsal bars
   do. The path names the recording (`cell-recorded/`), and the observation's `source` is
   `cell_recorded_daily_bars`.
5. **The data process puts the closes object.** For the last completed session, the data process
   puts each instrument's `closes_object` of the closes it stored. The runtime's store mount stays read-only
   (DEC-1016, and the manager's admission policy admits a managed pod's claim only read-only). The
   runtime journals `ObservationRecorded` naming that digest only after confirming the store holds
   those bytes; a missing or different object is no output (MI-10, DEC-2383).
6. **Minute bars for the trailing volume.** In the cell, the data process also stores a 1-minute
   dataset of each instrument, from the same recording: the whole regular session of the latest
   trading day on or before the current New York date (today's on a trading day, the last
   session's on a closed one, so the dataset exists to inspect on any day), an early close's
   included, one bar a minute at that day's close and 100,000 shares. Like the daily series, the
   day is one function of its date. So its partition is written once, and `Store::put_day`'s
   refusal to rewrite a stored day never fires. Because the partition holds minutes still to
   come, `mandate_bars::trailing_minutes` is the only sanctioned reader of a minute dataset: no
   other code reads its partitions for a figure. It hands out only the bars that have ended by
   the reader's clock and started inside the five minutes before it, from a dataset of the
   instrument's one-minute bars that inspects clean. A stored bar for a later minute is never
   read, as a vendor would not yet have served it. Outside the regular session none are in the
   window, and the participation cap fails closed, which is correct: openings are
   regular-session only.
7. **What this does not decide.** E21-44's host-local channel, quotes, and marks (the executor's
   `MarkUpdated` stays the risk mark; a bar never marks, DP-7) are unchanged. On staging the data
   process will read Alpaca IEX bars over the account's connection in a later slice; until then
   no tier but the local cell can name the recording.

### Rationale

- One rule below every reader means the runtime, the executor and the shell cannot disagree on
  whether a series is trusted, and the rule stays mutation-tested.
- Storing the existing dataset format reuses the trust check, the inspection and the fixtures,
  with no new parser and no new staleness rule.
- A series fixed by date never conflicts with `Store::put_day`'s refusal to rewrite a stored day.

### Alternatives considered

- **A bespoke bar artifact with a name index.** It needs a new parser and a new staleness rule
  beside the trust check that already exists. Rejected for the dataset.
- **Moving the trust check into `mandate-marketdata`.** It drops out of the mutation gate.
  Rejected.
- **The runtime putting the closes object.** It needs a writable claim the manager's admission
  policy refuses for managed pods. Rejected for item 5.
- **The recorded series written into the runtime image (as the research bundle is).** It keeps
  a second bar source beside the data process. The coordinator ruled the data process the single
  bar reader.

<a id="dec-2386"></a>

## DEC-2386: The executor process runs over the Robinhood connector against the simulator, on one account mapping shared with the shell

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176, scope set by the coordinator on 2026-10-26; it adds no path to a real broker: loopback and the simulator only, no `live` feature and no credential) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | `claude-code` (builder of lane LV, claim [#137](https://github.com/kunwarshivam/mandate/issues/137)), with lane X for the executor's wiring |
| **Story or claim** | E21-30 slice X1b-3 (with [DEC-2380](trading-core.md#dec-2380)); DEC-2382 ([#2434](https://github.com/kunwarshivam/mandate/pull/2434)); [DEC-928](earlier-900-1999.md#dec-928), [DEC-933](earlier-900-1999.md#dec-933) item 1, [DEC-1151](earlier-900-1999.md#dec-1151); connections spec §6.2, U-R8; trading-domain spec §11 |

### Context

The founder's end-to-end trade is to be shown on the Robinhood connector, tested against
`mandate-rh-sim` (DEC-2382). `mandate-executord`, the cell's executor process, runs only over
Alpaca's client today, and `xtask/layers.toml` forbids it `mandate-robinhood`, `mandate-mcp`,
`mandate-shell` and `mandate-rh-sim`.

Robinhood's connector answers only `Submit`, `Cancel` and `ListOrders`. Its account reads are
`RobinhoodConnector::account_snapshot` (cash, buying power, positions, working orders). Robinhood
publishes no account activity, so the shell books a reconciliation's fills from the order records
and positions (`mandate_executor::recorded_fills`, DEC-1151) and maps the snapshot to the
executor's account with `robinhood_account` (DEC-933 item 1), a function that lives in
`mandate-shell`. `mandate-executord`'s reconciliation asks for the four Alpaca reads, activities
included, so over Robinhood it would stop at the first read the connector does not offer.

### Invariants first

1. No code path in `mandate-executord` reaches a host other than loopback, reads a credential or
   token, or compiles a `live` feature because of this decision.
2. The shell and the executor process read a Robinhood account through one implementation: the
   same reads and the same mapping to `BrokerAccount`, never two that can drift.
3. A reconciliation over a broker that publishes no activity books exactly the fills
   `recorded_fills` implies, as the shell's does, and moves no activity cursor.

### Decision

1. **Dependencies.** `mandate-executord` may depend on `mandate-robinhood` and `mandate-mcp` (the
   `Tools` seam the connector calls through), and on `mandate-rh-sim` as a dev-dependency only.
   It still may not depend on `mandate-shell`, `mandate-paper` or `mandate-cli`. No `live` feature
   is added. A production MCP client holding the founder's token is DEC-2382's step, the founder's,
   and is not built here.
2. **One account mapping.** `robinhood_account`, the DEC-933 item 1 mapping from
   `AccountSnapshot` to `BrokerAccount`, moves unchanged from `mandate-shell` to
   `mandate-robinhood` as `broker_account`. The shell re-exports it under its old name, so its
   callers (the reconciler and the paper facts) change nothing. The move is the first PR, alone,
   so it can be reviewed as a move.
3. **The process's Robinhood broker.** `mandate-executord` gains a `RobinhoodBroker`, an
   `executord::Broker` over one `RobinhoodConnector` and the runtime its tools were connected on
   (DEC-928 item 2), shaped like `AlpacaBroker`. Its profile is `robinhood_profile`. `Submit`,
   `Cancel` and `ListOrders` go to the connector; the snapshot's reads come from one
   `account_snapshot` read and `broker_account`.
4. **A snapshot without activity.** When the broker publishes no account activity, the process's
   reconciliation snapshot leaves the cursor where it was and takes its fills from
   `recorded_fills` over the positions it read, as the shell's `without_activities` does.
5. **Tests first.** A tests PR adds `crates/mandate-executord/tests/rh_sim_fills.rs`, pending E21-30:
   an executor process over the Robinhood broker against an in-process `SimServer` whose script
   fills the agent's limit opening at its limit, asserting the agent's fill, the GTC stop-limit
   placed after it from the profile (no OCO; DEC-966, DEC-1151), and a reconciliation that books
   the fill and then runs clean with the cash in band. The implementation PR follows with zero
   missed mutants and an independent review.

### Alternatives considered

- **Duplicate the account reads and mapping in `mandate-executord`.** Rejected (invariant 2, the
  founder's no-shortcuts rule): two mappings of one account would drift.
- **Let `mandate-executord` depend on `mandate-shell`.** Rejected: the shell is the runner's, and
  the layering keeps the process below it.
- **Wait for DEC-2382's live wiring.** Rejected: the simulator path proves the loop without the
  founder's credential, and the live step stays the founder's.

### Consequences

- `xtask/layers.toml`: `mandate-executord`'s `forbidden_internal` drops `mandate-robinhood`,
  `mandate-mcp` and `mandate-rh-sim` (the last as a dev-dependency only).
- `mandate-robinhood`: `broker_account`.
- `mandate-shell`: `robinhood_account` re-exported from `mandate-robinhood`.
- `mandate-executord`: `RobinhoodBroker`; the snapshot without activity; `tests/rh_sim_fills.rs`.
- The cell keeps the Alpaca fixture loop (DEC-2380) until this path lands. The founder answered
  DEC-2382's open questions on 2026-10-26 (its items 6 to 9); the `simulated` environment (its
  item 6) follows this path.

<a id="dec-2387"></a>

## DEC-2387: The runtime proposes an owner's exit of one instrument as `IntentProposed` legs, so the cell's executor sells it

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It takes the reading journal spec §9.1 already gives, an `IntentProposed` caused by the `OwnerExitRequested`, and wires its one missing writer. Nothing loosens a limit or a safety rule; item 4 only stops handing a body the executor refuses) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX); the number was reserved through the coordinator, which coordinates the range with lane D1 |
| **Story or claim** | Product-flow blocker X7 (`docs/project/product-flow-map.md`); trading spec §5.3 rule 5, §5.4, §5.5, §5.6; journal spec §9.1 (`IntentProposed`, `OwnerExitRequested`); mandate spec §6.1; DEC-257 item 7, DEC-2033, DEC-2037, DEC-2040; `AGENTS.md` rules 5 and 13 |

### Context

In the cell the runtime and the executor are separate processes, and the journal is the only
channel between them (design §3.1). The daemon runs the runtime's effects and drops every
`Effect::Intent` by design. The executor process reads, from each followed agent's stream:

- every `KillSwitchActivated`, which it applies as its own agent-scoped kill switch (DEC-2003
  item 4);
- every `IntentProposed`, which it hands to its core, an exit whenever the lifecycle allows a risk
  exit (DEC-2033 item 1). An owner's exit at the market is priced from the `OwnerExitRequested` on
  its causation chain: its floor, or with valid step-up its confirmation (DEC-2040).

Journal spec §9.1 says an `IntentProposed` may be caused by "for an owner's exit of one
instrument, the `OwnerExitRequested`", and its reference vectors hold such legs (seq 7 and 10). No
writer produces them. The runtime records the owner's exit of one instrument as its
`OwnerExitRequested` copy and hands an `IntentBody::Flatten` identified by the copy (DEC-257
item 7):

- in the cell the daemon drops that handoff, so the confirmed exit is never sold;
- in one process the executor's core refuses every `Flatten` handoff (`intent::flatten_plan`,
  `Unimplemented E7-4`), so it is not sold there either.

### Decision

1. **One `IntentProposed` per sell.** In the batch that journals the owner's exit's
   `OwnerExitRequested` copy, the runtime journals, right after it, one `IntentProposed` for each
   sell of the agent-scoped plan of that instrument (the same `FlattenPlanner` request as today:
   exactly the agent's sub-ledger, never the account's). Each one records:
   `instrument_id`, `side` `sell`, `type` `market`, `qty` the plan's quantity, `limit_price`
   `null`, `purpose` `owner_exit`, and `causation_id` the copy. `tif` is `day` for an equity and
   `gtc` for a crypto pair, which takes no `day` (trading spec §5.1; the order-execution check
   `mandate_ports::validate_order_execution` refuses crypto at `day`).
   - The executor finds the owner's floor or confirmation on that chain (DEC-2040), so the record
     needs no price of its own.
   - A plan with no sell (the agent holds none of it) journals the copy alone.
   - **Where the cell's planner cannot plan, it says so.** A held instrument with no asset class
     in the universe, and now also a plan asked before the process follows its account, raise
     `flatten_unplanned` (DEC-2378 item 8) rather than pass as "nothing held". The exit is then
     not proposed: a leg needs the asset class for its time in force, and the runtime has no other
     quantity to sell. The kill switch is unaffected, since the executor plans its own flatten.
     Until the cell's alerts reach the owner (X8), that alert is operator-only. Sizing from the
     runtime's journaled fold, not the executor's ledger, means a fill the runtime has not yet
     read is not in the leg; the owner's next exit of the instrument sells it.
2. **The executor sells it as any exit.** The cell's executor takes each leg as it takes every
   `IntentProposed`. Its core then runs the marketable exit sequence of trading spec §5.4: cancel
   the instrument's protective orders, confirm the read-back, re-run the gate on fresh state, and
   submit, priced by §5.6 where a market order is not allowed. It also cancels the agent's own
   resting opening buys in the instrument first (§5.3 rule 5). So what the flatten plan's
   cancels did for an owner's exit is done by the executor, from its own ledger.
3. **One process hands the same intents.** The runtime hands each leg as an `IntentBody::Order`
   identified by its `IntentProposed`, with no limit, purpose `owner_exit`, the leg's asset class,
   its `tif` and no protection, right after its draft. The order body's limit becomes optional
   for this: `None` only for an exit at the market (journal spec §9.1 rules 2 and 3); the fold
   refuses a market opening or increase as non-canonical. A retry re-hands the legs from the
   drafts, and a restart re-hands them as every committed `IntentProposed` the account stream has
   not taken (`pending_handoffs`). A restart re-hands an owner's exit in every mode, `paused` and
   `stopped` included, as it re-handed the owner's flatten before (DEC-131 item 22): the owner
   asked for it, and the executor's own mode gate is what may hold it (rule 13). In one process a
   re-handed order carries no execution policy, which the shell's sink refuses for every order;
   that gap predates this decision and is backlog row E7-2r. The cell reads the journal, so it is
   not affected.
4. **The owner's exit is no longer handed as a `Flatten`.** Its handoff and its restart re-hand
   (`untaken_owner_exits`) go. The executor refused that body, and handing both would sell twice
   once a core takes it. The kill switch keeps its flatten: the executor plans that one itself
   from the `KillSwitchActivated`.
5. **The copy's step-up evidence is a timestamp.** `OwnerExitRequested` types the evidence's
   `authenticated_at` as a §4.7 timestamp (journal spec §9.1), and the executor reads it as one to
   bound a stepped-up exit's confirmation (DEC-2040 item 7). The runtime wrote whole seconds, the
   control stream's form, so a real journal refused every owner exit and owner kill switch with
   valid step-up, and the runtime process stopped. The copy now writes the timestamp;
   `ApprovalResponded` keeps the seconds §9.7 gives it.
6. **An instrument exit journaled before this change** has no legs. Nothing re-sells it on a
   restart. Only paper and fixture journals hold one, and no executor could ever take it.

7. **The two processes share a contract file.** They may not link each other
   (`xtask/layers.toml`), so `mandate-daemon`'s test writes the runtime's own rows for three owner
   exits (unconfirmed, stepped up, stale) to `fixtures/contracts/owner-exit-agent-stream.json`, and
   `mandate-executord`'s test reads that file through `follow::proposals`. A drift on either side
   fails one of them; `MANDATE_WRITE_CONTRACTS=1` rewrites the file from the runtime.

### Consequences

- An owner's exit of one instrument reaches the cell's executor and is sold under §5.4 and §5.6:
  at the market in the regular session, at the confirmed floor or with the stepped-up
  confirmation outside it, and otherwise held for the session.
- Discretionary exits, the builder's `risk_exit` (`trim_to_target`) and approved exits already
  reach it as `IntentProposed`. Kill switches reach it as `KillSwitchActivated`. Risk exits the
  executor originates (the watchdog, the unprotected bound, the ladder's flatten) are its own.
- In one process an owner's exit is handed with no confirmation, since the runtime's handoff has
  no field for one. It waits for the regular session there: stricter, never looser.

<a id="dec-2389"></a>

## DEC-2389: Deterministic event ids derive from their stream

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: journal spec §3 already requires an `event_id` to be globally unique, and these writers' ids now are) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane DR, assigned by the coordinator) |
| **Story or claim** | Lane DR's product-flow map, X9; journal spec §3 (`event_id`: a ULID, globally unique) and §5.1 (a retry offers the same draft); `AGENTS.md` rule 5 |

### Context

The deployed runtime (`mandate-daemon`), the executor (`mandate-executord`) and the scheduler
(`mandate-scheduler`) each derive their drafts' event ids deterministically, so that a retry after
an append in doubt re-derives the same ids. Each built the id from a space digit and the writer
epoch, head and ordinal alone:
- `0` for the agent stream;
- `1` for the account stream;
- `2` for the clock stream.

Every stream of one kind therefore wrote the same ids for its first batch. The journal refuses an
`event_id` another stream already holds: Postgres's `event_ids` primary key, and the memory journal
with `duplicate_event_id`. So in one cell journal:
- only the first agent's runtime could open its agent stream;
- only the first account's connect check could commit;
- only the first workspace's scheduler could open its clock stream.

The demo's trade loop runs on its second agent. A second demo run on one cell failed its connect
check with `append_refused`.

### Decision

1. **One derivation, `mandate_canon::stream_event_id(stream, epoch, head, ordinal)`.**
   - The input is the SHA-256 of a domain tag (`mandate.stream_event_id.v1\0`), then the stream id's
     length (8 bytes, big-endian) and bytes, then the epoch and the head (8 bytes each) and the
     ordinal (4 bytes), all big-endian.
   - The id is that digest's first 128 bits, spelled as a canonical ULID. Its time component
     carries no meaning (journal spec §3).
   - The same position always gives the same id, so a retry offers the journal the same draft.
   - The length goes before the bytes, so no stream and position can spell another's preimage.
   - It sits beside `Digest` and `encode_ulid`, which the deployment manager already combines the
     same way for its own ids.
2. **Each writer derives from its own stream.**
   - The daemon's `fixture::Ids::on(agent stream)`: `fixture::wiring` and `wiring_for` take the
     agent stream.
   - executord's `fixture::Ids::of(scope)` for `acct:<workspace>:<account>`: `fixture::wiring`,
     `run::fixture`, `run::paper` and `credential::paper_wiring` take the account scope.
   - The scheduler's `Writer::event_id` for `clock:<workspace>`.
3. **Records already journaled keep their ids.** Nothing re-derives the id of a record already
   written:
   - A retry is only ever of the in-memory pending batch of the same process (DEC-131 item 6).
   - A restarted process folds what the journal holds and appends after it.
   - A cell upgraded mid-run therefore never derives a different id for a record it already wrote.
     At worst, a batch still in doubt when the old process stopped is not retried; the new process
     reads the journal as it stands.

   The old ids are 26 decimal digits starting `0`, `1` or `2`. A new id meets one only by a
   2^-128-scale chance, so old and new ids coexist in one journal.
4. **Not covered.** The paper shell's `envelope::Ids` (`mandate-shell`, used by `mandate-paper`)
   keeps the old scheme. It writes one agent stream and one account stream per run. It moves to
   this derivation in its own change, before any second agent or account shares a journal with it.

### Tests

- `mandate-canon/tests/stream_event_id.rs`:
  - a table worked out in Python;
  - a property against an independent `sha2` oracle, checking the id is a canonical ULID and
    deterministic;
  - a property that no two streams and positions share an id.
- `mandate-daemon/tests/runtime.rs::two_agents_runtimes_on_one_journal_both_open_their_streams`.
- `mandate-executord/tests/journal.rs::two_accounts_executors_on_one_journal_both_open_their_streams`.
- `mandate-scheduler/tests/event_ids.rs`.

Each of the last three fails on the old derivation.

<a id="dec-2800"></a>

## DEC-2800: Before a one-read broker's reconciliation, the process lists each of our working orders the read no longer shows

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision that only adds facts before a reconciliation and adds no risk, DEC-79, DEC-176) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane LV (Claude Code), with the coordinator's agreement on the design |
| **Story or claim** | E21-30 X1b-3; DEC-2386 slice C, [DEC-2390](DEC-2390.md), [DEC-2395](DEC-2395.md), [DEC-2382](trading-core.md#dec-2382) |

### Context

DEC-2386 slice C puts the Robinhood connector behind the executor process's `Broker` trait.
Robinhood answers only submit, cancel and the order listing; the account (open orders, positions,
cash) is one read, `account_snapshot`, and it reports a fill only on the order's own record, not as
an activity. An order that fills between two reads drops out of the open-order list while its
position appears, so a reconciliation that compared positions first would see an unexplained
position and pause the agent on every fill.

DEC-2386 item 5 planned the slice's tests on `rh_sim_fills` and the simulator's server. The
coordinator has since directed lane LV to do no new simulator work, relaying a founder instruction
lane LV has not seen in the founder's own words; DEC-2390 item 2, which rests on the same relay, is
still Proposed. Fixtures are synthesized from Robinhood's published contract only, never recorded
from any account (DEC-529 item 6, `AGENTS.md` rule 8). Item 1 below holds either way: a scripted
seam adds no risk and reaches nothing, and should the founder want the simulator back, its tests
can be added beside these.

### Decision

1. **The slice's tests use a scripted tool seam shaped by the published contract**, in place of
   DEC-2386 item 5's `rh_sim_fills` and simulator server. The seam answers the connector's MCP tool
   calls from a small in-memory book; `mandate-mcp` is a dev-dependency of `mandate-executord` for
   it and never a dependency of the process, which `tests/paper_only.rs` pins. This narrows
   DEC-2386 item 1, which let the executor depend on `mandate-mcp` too: the process depends on
   `mandate-robinhood` alone, and the production MCP client stays DEC-2382's step.
2. **Before a one-read broker's snapshot is reconciled, the process lists each of our orders that
   was sent (not still an `Intent`, which no broker has seen), is not terminal, and is absent from
   the read's working orders**, by the core's own
   listing (DEC-862), and hands each answer to the core, which adopts only the one record naming it
   at our prices. When any answer was handed, the account is read again before the snapshot, so a
   protective order placed on that fill is in it.
3. **An unanswered listing holds and never adopts.** A listing that is lost, cannot be sent, or whose
   answer does not commit ends the pass without reconciling: the reconcile flag stays set, nothing
   is marked `Unknown`, and the next pass tries again.

### Rationale

The listing only adds facts the broker already holds about our own orders; it changes no limit and
sends nothing that the core would not send on the same answer. Holding on an unanswered listing is
the safe default (rule 3): reconciling without it would read a filled order's position as
unexplained.

### Alternatives considered

- **Compare positions first and explain them afterwards:** pauses the agent on every fill.
- **Keep the simulator for the tests:** declined by the founder's no-simulator instruction.

<a id="dec-2801"></a>

## DEC-2801: `quant.ma_crossover` 1.1.0, whose `Flat` is an exit output with conviction −1

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: a new model version that only reduces risk, reaching an agent only through a confirmed mandate version that pins it; the coordinator approved the tie reading on 2026-10-26) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane MC (Claude Code), with the coordinator; first proposed by lane LV as DEC-2390's draft item 7, which did not survive that decision's rewrite |
| **Story or claim** | [DEC-2390](DEC-2390.md); [DEC-503](DEC-503.md), [DEC-504](DEC-504.md), [DEC-518](earlier-303-599.md#dec-518), DEC-157 item 4; mandate spec §8.2, §8.3 |

### Context

`quant.ma_crossover` 1.0.0 emits an output only on `Long` (DEC-157 item 4). A crossover down is no
output, so mandate spec §8.3's exit conviction `c` is 0 and the builder never proposes a
discretionary exit: an agent on 1.0.0 leaves a position only by its risk exits, protection, a goal,
or the owner. DEC-2390 needs the always-on agent to sell on its signal, as the baseline backtest of
the same strategy does (`mandate-backtest`'s `Signal::Flat` closes the position).

### Decision

1. **A new version, 1.1.0.** It has 1.0.0's crossover, parameters, inputs and `Long` output
   (direction `long`, conviction 1, confidence 1). On `Flat` it answers direction `long`,
   conviction −1 and confidence 1, which favors exiting (§8.2: long-only in v1, negative conviction
   favors exiting). `Undecided` is no output, as before.
2. **A tie is an exit.** `Flat` is `mandate-backtest`'s own: the fast mean at or below the slow
   one. 1.1.0 takes that signal unchanged, so there is one definition of the crossover, and the
   model sells exactly when the backtest of the same strategy sells.
3. **1.0.0 does not move.** Its two listed source files stay byte for byte, so its pinned content
   hash (`sha256:cf36eb…c469d`) and every mandate that pins it are unchanged. 1.1.0's code is a
   third file, `mandate-modelhost/src/ma_crossover_exit.rs`. Its content object (DEC-518) is
   1.0.0's with its own `model_version`, methodology, `output.flat` and listed sources: the two
   1.0.0 files and its own. Its hash, `sha256:ee5806e6…e5a4`, is pinned as a literal in the host's
   tests (`tests/exit.rs`), so any later edit to a listed file needs a new version (DEC-504 item
   2). As for 1.0.0, the host's `lib.rs`, which dispatches on the version and maps the signal to
   `Long`, `Exit` or no output, is not a listed file; its tests hold each version's answer.
4. **The host's answer.** `mandate_modelhost::evaluate` returns the exit output as
   `Evaluation::Exit`, apart from `Evaluation::Long`, so no caller can take it for a buy. The
   host still refuses unless the pin, the registry entry and the host's own content for that
   version agree (DEC-503 item 3), so 1.1.0's code never runs under 1.0.0's hash, or the reverse.
5. **Who reads it.** The deployed runtime's composer (`mandate-runtime-models`, DEC-2383 item 7)
   hands an exit output to the core like any output; the core journals it. In §8.3 its term
   `w · (−1) · 1` lowers both `c` and `b`; as an agent's only model, `c = b = −1`, so
   `c ≤ −exit_threshold` exits the whole position less working exits, and no buy follows. The R0 paper runner (`mandate-paper`), which DEC-2390 item 2 leaves
   as it is, reads `Exit` as it reads 1.0.0's `Flat`: no output.
6. **How an agent gets it.** Only through a confirmed mandate version that pins 1.1.0 with its
   hash, after `model register` records it (the CLI computes the hash from the host's content).
   No envelope changes otherwise (`AGENTS.md` rule 11). Lane DR pins it in the cell's demo
   mandates.

### Rationale

- An exit output only lowers risk: in §8.3 it can lower `c` and `b`, never raise them, so it can
  start an exit and never a buy (rule 2, rule 13). The worst case of a bad signal is an early sale.
- Taking the backtest's `Flat` as is keeps the live model and its backtest on one comparison. A
  strict "below" would need a second comparison written beside the backtest's, which could drift
  from it, and the backtest's file cannot change without moving 1.0.0's hash.
- A separate `Evaluation` variant makes "exit" a fact of the type, so a caller that matches only
  `Long` drops an exit rather than buying on it.

### Alternatives considered

- **Change 1.0.0 to answer `Flat`.** Rejected: it moves 1.0.0's pinned hash, so every confirmed
  mandate pinning it would be refused, and a model's code may not change under its version.
- **A tie is no output** (LV's draft). Rejected for the drift above. Both readings only reduce
  risk, and an exact tie of two rational means is rare.
- **Return the exit as `Evaluation::Long` with conviction −1.** Rejected: a caller reading the
  variant as "buy" would be wrong on every exit.

<a id="dec-2802"></a>

## DEC-2802: The executor's binding gate reads an untraded account's allocation as the agent's risk

| | |
|---|---|
| **Status** | Accepted (agent: a reading that only tightens, DEC-176; the coordinator approved it on 2026-10-26. It adds no risk: every other case holds the opening) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)), with lane RK for the loss carry |
| **Story or claim** | E21-30 slice X1b-3 G2b, PR #2547; [DEC-2380](trading-core.md#dec-2380) item 2 |

### Decision

1. The binding gate reads the agent's risk (equity, high-water mark, day start, capital base,
   inherited loss, latched limits, ladder rungs, size factor and mode), its instrument
   restrictions, its exits' last fills, its orders today, its day trades and its conduct counters
   from the account risk view (lane RK's `risk_snapshot`, R4). Until that view is wired, the
   executor answers them itself in exactly one case, where each is a fact rather than an
   assumption: the account's stream holds no fill and no order request or submission of any
   agent's (`mandate_gate_context::Journaled::untraded`). Then the agent's whole allocation is its
   equity, its high-water mark, its day start and its capital base, its mode is normal, nothing
   is latched or restricted, every counter is empty, and the account is flat with no order
   working.
2. The inherited loss in that case is the connection's loss carry the deployment was validated
   with (mandate spec §5.7, MI-14; `ConfirmedVersion::connection_loss_carry`), never zero, so a
   retire and redeploy on the same connection cannot reset the lifetime floor.
3. In every other case, once anything has traded on the account, the gate's input is `None` and
   the opening is held, until the account risk view supplies the agent's risk. An exit is never
   held by this: the binding gate is never asked for one (`AGENTS.md` rule 13).
4. The gate judges only the mandate version the journal has in force for the agent (the latest
   applied on the account stream, else the deployed one): an opening whose deployment reads
   another version is held.
5. What an exit, a fill or a mark reads (the version, the instrument's class, grid and fill
   references, the instrument marked) comes from the agent's deployment as it reads now, else as
   it last read in this process, from the control rows that parse; an opening reads only a
   deployment every control row confirms. A restart while the artifact store is out has no last
   read, so a held position's fills are refused until the store answers: an accepted limitation,
   recorded as a backlog row (`E21-30-executor-fill-refs-across-restart.md`). A control stream
   whose deployments the process cannot read at all stops the process at once (its `agents_of`
   refusal), so the ports never answer for it.
6. "Flat with no order working" is what the account stream shows. An order or a position placed
   outside the platform is not journaled, so it is unseen here until reconciliation; the broker's
   open orders and positions are added once the brokers serve their facts
   (`E21-30-binding-gate-broker-flatness.md`).

### Rationale

The first G2a input assumed the allocation snapshot for any flat account, and its independent
review failed it: a flat agent down 30% read as no drawdown, and a re-entry right after an exit
passed the conduct limits. A flat-snapshot shortcut was proposed to unblock the cell and refused
for the same reason. On an account where nothing has ever traded the same figures are true, so the
cell's first opening can reach the gate without weakening any limit, and every later one waits for
the view that reads them.

### Alternatives considered

- **The allocation snapshot for any flat account.** Rejected: drawdown, the conduct counters and
  the exits' fills are history, not position, so flatness does not make them facts.
- **Hold every opening until R4.** Safe, but it blocks the trade loop on work in another lane while
  the untraded case is exact.
- **Read flatness from the agent's own orders only.** Rejected in review: another agent's working
  order on the same account spends its buying power unseen.

<a id="dec-2805"></a>

## DEC-2805: A batch the journal refuses holds the account's openings and keeps the executor running

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: the reading that adds no risk where the specs are silent; it replaces a stop that also stopped every exit, protective order and kill switch with a hold of openings only. The coordinator assigned it) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane MC (Claude Code), number from the coordinator's trading-core range |
| **Story or claim** | X10 follow-up; trading spec §7.1, §11; `AGENTS.md` rules 3, 5 and 13; DEC-140's slice-5 amendment (the `incomplete` hold) |

### Context

The executor process journals each input's drafts as one batch before it acts on any of them. When
the journal refused a batch as `Invalid`, the process marked itself failed and exited. A restart
reads the same broker activity, drafts the same batch, and is refused again: a crash loop. While it
loops nothing runs, not exits, not protective orders, not the kill switch, which `AGENTS.md` rule 13
says are never held but by an agent's mode, an `Unknown` order, or the broker.

Such refusals happen today. Every external fill's batch is refused at its `FillApplied`: journal spec
§9's version 1 requires a client order id, which an unattributed fill has none of (DEC-938's
backlog), and requires the fill configuration and a non-empty fill id besides. So the first fill
that is not ours, from the owner trading the same account by hand, stops the executor. Neither the
trading spec nor the journal spec says what the executor does when the journal refuses its batch.

### Decision

1. **A refused batch holds every agent's openings, once, and the process goes on.** Nothing of the
   batch committed and none of its broker requests is sent (rule 5). The core forgets the batch,
   so later inputs are handled rather than refused as an unresolved append. It then journals one
   `AgentModeApplied` of every agent (`*`) to `exits_only` under `reconciliation:incomplete`, and
   one `OwnerAlertSent` of kind `reconciliation` naming it. This is the hold an incomplete
   reconciliation run places (DEC-140's slice-5 amendment), and the owner's acknowledgment of
   `incomplete` lifts it in the same way (§11: resuming needs the owner). While the hold is in
   force a second refusal adds nothing. The clocks go back to where the refused input found them,
   so the next tick is judged against the last tick that committed and a window it crosses still
   fires. Exits, protective orders and kill switches go on, since `exits_only` holds only what
   adds risk: an exit whose own batch was refused is handed again at the next pass and sent once
   its batch commits. The operator is alerted once per process (`journal_refused`), and a refused
   reconciliation counts as one that did not pass.
2. **External fills fall under item 1 until they can be recorded.** The executor still drafts them
   as §7.1 says: `ExternalActivityIngested`, `FillApplied`, every agent `exits_only`, the owner
   alerted. The journal refuses that batch, so item 1's hold stands in for §7.1's until journal spec
   §9 gains a `FillApplied` that can record an unattributed fill (the backlog holds the row). The
   executor does not skip the fill to get the batch accepted. Protection nets an exit against such
   fills, and an exit sized without them could sell more than the account holds. Until then, the
   activity cursor does not pass the refused fill, so every later reconciliation that reads it is
   refused too: the hold stays, and comes back after an owner's acknowledgment, and fills of ours
   that only a read reports wait with it. The backlog row is high priority for that reason.
3. **A refused mark is dropped.** The executor writes `MarkUpdated` outside the core. A mark the
   journal refuses is dropped, alerted to the operator once, and the next interval marks again. A
   missing mark only leaves the risk mark stale, which holds openings at the gate and never an exit.
4. **What still stops the process.** The hold itself refused, since nothing safe is left to
   record; a refused start (`Input::Started`), which has already marked the core started and
   would lose the lookups of orders in doubt, the resent cancels and the startup reconciliation
   that the restart redoes, before any trading (a start whose drafts are refused every time
   still loops, as before this decision, with no trading in the loop); a refused `StreamOpened` or establishment copy,
   which also come before any trading; a head mismatch, idempotency conflict or fence, which mean
   another writer, not a bad draft; and an error from the core itself, unchanged here (the
   backlog holds the row).
5. **What this does not cover.** An exit, protective order or kill switch whose own drafts the
   journal refuses on every pass is held by that refusal, which rule 13 does not list; the
   operator and owner alerts above are raised once, not per pass. Before this decision the
   process stopped instead, which held every exit. The remedy is the schema fix for the refused
   record, as item 2's row is for external fills.

### Rationale

- Rule 13 makes exits, protection and the kill switch the last things to stop. A crash loop stops
  them all; a hold of openings stops nothing that reduces risk.
- Rule 3 resolves the ambiguity to a safe default: the account takes no new risk, the owner is told,
  and resuming waits for the owner, as for any reconciliation that could not complete.
- The hold is journaled before anything else happens, so a replay reaches the same state (rule 5).

### Alternatives considered

- **Keep stopping.** Rejected: the restart refuses the same batch, and nothing reduces risk while it
  loops.
- **Drop the refused draft and journal the rest.** Rejected: which draft the journal refused, and
  what the rest of the batch means without it, is not known in general; a batch is one decision.
- **Skip an unattributed fill's `FillApplied`.** Rejected (item 2): the position goes unnetted.
- **A new alert kind or restriction subject.** Rejected: `reconciliation` and `incomplete` already
  mean "the account's record could not be completed; the owner must look", and new vocabulary
  would be a notifications and journal spec change for no gain.

<a id="dec-2806"></a>

## DEC-2806: The agent runtime folds the executor's account-wide restriction

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: the runtime now holds itself in every restriction the executor holds it in, each lifted only by its own cause, as the executor lifts it) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | E21-54 (lane DR's follow-up to [#2579](https://github.com/kunwarshivam/mandate/pull/2579)); trading spec §7.3; journal spec §9.6 |

### Context

The executor writes `AgentModeApplied` with `agent` `*` (`EVERY_AGENT`) for:
- an account kill switch;
- an order status it cannot map;
- a fee-step pause;
- a reconciliation mismatch that no order names.

Every agent of the account is then held in it: the executor's `effective_mode` for an agent is the
strictest of the account's mode, `*`'s, and the agent's own. The runtime's `addressed_here` read a
`*` as addressed to no one, so a runtime went on proposing what the executor would refuse. Lane DR
found this in the cell while fixing #2579.

### Decision

1. **Keyed as the executor keys them.** `RuntimeState` folds each `AgentModeApplied` under its
   `restriction`, in two maps:
   - `own_restrictions`, for the records naming this agent (or no agent);
   - `every_restrictions`, for the records naming `*`.

   A record's mode replaces only its own restriction's, and a `normal` removes only that
   restriction. This is how the executor's `agent_mode_applied` and `recompute` key
   `(agent, restriction)`, because causes lift independently (trading spec §7.3). The single slot
   this replaces let a later, looser record for another cause overwrite a stricter one: the
   review of #2584 found the account-wide case, and the same flaw sat in this agent's own slot.
2. **The effective mode, and the re-hand mode a restart uses,** take the strictest restriction in
   either map, then the lifecycle, the startup hold, and the kill switch's final mode, as the
   executor's `effective_mode` does.
3. **Neither map lifts the other.** An account-wide `normal` lifts only that account-wide
   restriction, never one of this agent's; this agent's `normal` never lifts the account's. A
   sibling's record moves neither.
4. **A `*` record never confirms or retires this agent's kill switch** (`retired`), so a lift of
   another cause can never lift a switch.
5. **Copies:** a tightening is journaled as any account-stream mode change is: an
   `AgentModeChanged` citing the record, and every pending approval cancelled
   (`mode_tightened`).
6. **`FOLD_VERSION` 3.** A journal holding a `*` record, or two causes for this agent, folds to a
   stricter mode than before. So the fold version moves, and the golden journal pins an
   account-wide lift that leaves another account-wide cause in force (ADR-0001 ES-21).

### Rationale

The runtime must never be looser than the account stream (`AGENTS.md` rules 1 and 3), and keeping
the executor's own slots means each record lifts only what it set.

### Alternatives considered

- **Read `*` as this agent's, or keep one slot per subject.** Simpler, but a single slot lets one
  cause's `normal`, or a looser record for another cause, lift a restriction still in force, which
  adds risk.

<a id="dec-2810"></a>

## DEC-2810: A deferring opening with no plan is denied `plan_required`

| | |
|---|---|
| **Status** | Accepted; item 6's silent returns resolved by [DEC-2814](trading-core.md#dec-2814) (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: an opening that was already never asked or placed is now refused on the record, and the owner is told) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | Backlog `E21-30-unasked-deferral-notice` (closed here); amends [DEC-2515](DEC-2515.md) item 1 |

### Context

`ApprovalRequested` version 3 binds the builder's plan and names the dry run's deferrals (DEC-2515).
A request that would name a deferral without a plan has no version. Until now the runtime handled
such an opening silently:
- it journaled the decision `ask`;
- `escalation::ask` returned without a request;
- nothing told the owner;
- no approval was pending, so the next tick decided the same opening again, unseen.

### Decision

1. **The runtime refuses it itself.** In `step::dry_run`, an opening or increase the gate allowed
   with checks deferred, for which the builder produced no plan, is denied `plan_required`. This
   is a runtime check, as the dry run's `account_restricted` and `working_order_limit` are.
2. **It is journaled and notified as any denial:**
   - a `DecisionMade` at version 1, with `dry_run` `deny`, reason `plan_required`, and nothing
     deferred;
   - the opaque `decision_skipped` notice (`AGENTS.md` rule 6).

   Nothing is asked, proposed, or left pending. A later tick decides it again visibly, as any denied
   opening is.
3. **Re-validation** runs the same dry run, so a grant whose re-validation now defers a check with
   no plan is skipped `plan_required`. Such a grant would have handed the executor no plan, which
   its sink refuses. A grant with a plan acts as before
   (`a_grant_without_a_plan_that_now_defers_is_skipped_plan_required`).
4. **`escalation::ask`** no longer carries its own no-plan check: the dry run makes it
   unreachable.
5. **A deny-classified opening** that would also defer a check with no plan is now recorded as the
   dry run's `deny` `plan_required`, where it was the dry run's `allow` and the classification's
   `deny`. Both refuse it and neither places anything. The new record is the stricter one, though
   it no longer shows the owner's DENY rule.
6. **`ask`'s other silent returns** are not covered here: a decision it cannot bind, a plan for
   another asset class, and a content object it cannot hash. They are backlog
   `E21-30-ask-silent-returns`.
7. **The mandate spec's §6.4 sentence** ("has no version, so the runtime asks no one and places no
   order") still holds, and is unchanged.

### Rationale

A refusal that is journaled with its reason and notified is the platform's ordinary way of not
acting. A silent skip that repeats every tick is not (backlog row, DEC-2515 item 1).

### Alternatives considered

- **A new record or reason member for an unasked decision.** That needs a journal spec change and
  new rule numbers, for what a denial already records.

<a id="dec-2811"></a>

## DEC-2811: The cell's dry run evaluates the day-trade budget on a version 2 snapshot's declared facts

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens what the owner is told, never what is allowed: the dry run reports the budget evaluated only where trading spec §9.2's check provably cannot refuse, and the executor's binding gate still evaluates it) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | [DEC-2378](DEC-2378.md) item 11 condition 2 (the day-trade budget turns on when its facts are journaled); [DEC-2398](DEC-2398.md) (`AccountSnapshotRecorded` version 2, journal spec v0.89 §9.35) |

### Context

The cell's runtime dry run deferred the day-trade budget because no fact it reads was journaled.
Since #2580, the executor writes `AccountSnapshotRecorded` version 2 with:
- the connector's declared `account_type` and `day_trading_regime`;
- the broker's `last_equity` and `maintenance_margin`.

Trading spec §9.2's check, `mandate_risk`'s `day_trade_budget`, refuses only under `legacy_pdt`,
on a margin account, in a US equity, with a prior-close equity below the rule set's threshold. Its
answer there also reads the pattern-day-trader flag and the trailing day-trade count, which nothing
journals (DEC-2398's review).

### Decision

1. **The fold.** `mandate-gate-context` reads a snapshot's version 2 members into
   `AccountFacts::figures`:
   - account type and regime, each `None` when undeclared;
   - prior-close equity and maintenance margin, both signed, with
     `SnapshotFigures::maintenance_excess` taking equity less margin.

   A version 1 snapshot has `None` figures. The latest snapshot wins whatever its version, exactly
   as the vectors' `snapshot_prior_close` readings and streams state.
2. **The context.** The daemon's advisory context takes the declared account type, falling back to
   the multiplier as before. A buying-power refusal names the type it read: the settled refusal for
   cash, the other for margin. The regime and the prior-close equity stay at their defaults
   (`legacy_pdt`, zero). Only the day-trade check reads them, and item 3 evaluates that check only
   where it cannot refuse whatever they say. Copying them in would be code no test could observe
   (the review's mutants on #2616). They are wired, with tests, in the slice that folds the
   day-trade ledger and so lets the check refuse. That wiring must land in the same change as the
   ledger, or a margin context on its defaults would refuse where the declared facts say it cannot.
3. **Leaving the deferred set.** `day_trade_budget` leaves the dry run's deferred set exactly when
   the check provably cannot refuse:
   - the instrument is crypto;
   - or the declared regime is `intraday_margin`;
   - or the declared account is `cash`;
   - or the declared regime is `legacy_pdt` with a prior-close equity at or above the threshold.

   Then the opening is asked with eleven parts deferred, and the budget is reported evaluated.
4. **Otherwise it stays deferred,** never passed: a version 1 snapshot, an undeclared regime on a
   margin or undeclared account, or `legacy_pdt` below the threshold. The executor's binding gate
   evaluates it (condition 3).

### Rationale

The dry run may name a check evaluated only on facts it can see (DEC-2388 item 7). Under the four
cases the check's answer is fixed by the declared facts alone. In every other case it turns on the
unseen ledger.

### Alternatives considered

- **Evaluate `legacy_pdt` below the threshold too, reading the ledger as empty.** That would report
  a pass the dry run cannot see (the lesson of #2471).

<a id="dec-2814"></a>

## DEC-2814: An ask the runtime cannot build is refused on the record

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: an opening that was already never asked or placed is now refused on the record, and the owner is told) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | Backlog `E21-30-ask-silent-returns` (closed here), found by the review of #2604; resolves [DEC-2810](trading-core.md#dec-2810) item 6 |

### Context

`mandate-runtime`'s `escalation::ask` returned without a request, a notice, or a record of why
when it could not build the approval request. The decision had already been journaled `ask`, so
nothing followed it and the next tick decided the same opening again, unseen. There were three
cases:
- the decision cannot be bound (no `decided_by` label, or a `combined_score` that is not a
  decimal);
- the builder's plan names another asset class than the order (rule 136's condition);
- the content object or its hash cannot be written, as for an order value past a decimal's range.

### Decision

1. **Built before it is journaled.** An allowed opening or increase classified `ask` has its
   request built in full (`escalation::prepared`) before its `DecisionMade` is journaled.
2. **Refused on the record.** When the request cannot be built, the decision journals the dry
   run's `deny` with the reason. `ask_unbound`, `plan_mismatch`, and `content_unhashable` are
   runtime reason codes, as `plan_required` is (DEC-2810). The decision is notified once as
   `decision_skipped`. Nothing is asked, proposed, or left pending.
3. **No other way through.** What follows an allowed decision is one value, `step::Next`: ask with
   the built request, propose, or nothing. No path can propose an opening classified `ask`, and
   `ask` itself has no way to return silently.
4. **Exits.** An exit is never built into an ask, so none of the three ever refuses an exit
   (`AGENTS.md` rule 13).

### Rationale

The owner learns of every opening the runtime decided to ask about and did not, with its reason
(the coordinator's ruling: a silent drop adds no risk but is an approval-flow hole).

### Alternatives considered

- **Leave the decision `ask` and journal a separate refusal.** That needs a new record, and so a
  journal spec change, for what a denial already records.

<a id="dec-2818"></a>

## DEC-2818: The runtime rereads its approvers on every control pass

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It only tightens: while it runs, the runtime admits only those approvers the control stream still resolves to, never beyond the set admitted at its start, as the shell does) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | Backlog `E21-30-daemon-approvers-reread` (closed here); amends the "When" of [DEC-2388](DEC-2388.md) item 11 |

### Context

[DEC-2388](DEC-2388.md) item 11 has `mandate-daemon` resolve its mandate's approvers once, when the
process resolves its deployment. A member deactivated or stripped of the approver role while the
process runs, or the owner of a connection revoked while it runs, stayed in the runtime's
`ApprovalSettings` until it restarted. The shell narrows its set at every read of the control
stream (`ApproverSource::current`, DEC-2384 item 6). The API's grant check (DEC-2129 item 7) already
refused such a grant before it reached the runtime, so this closes a second line, not the only one.

### Decision

1. **The source.** `deployment::resolved` also returns an `ApproverSource`: the approvers admitted
   at the start, the mandate's connection, and its approver references.
2. **The narrowing.** `ApproverSource::current(control)` resolves the references afresh over a read
   of the control stream through the same reading as DEC-2388 item 11 (every row in `seq` order,
   any unreadable row resolving no one) and returns the admitted set intersected with it. Nobody
   outside the set admitted at the start is ever added: a role granted while the process runs
   reaches it at its next start, as for the shell. A start approver whom the stream resolves again
   (a connection re-established by the same owner, a member reactivated) approves again.
3. **When.** Every control pass of `RuntimeProcess` sets the view's approvers to
   `source.current(rows)` as soon as it reads the control stream, before it folds the rows or hands
   any of them to the core, so a grant in the same pass as the deactivation is already admitted
   against the narrowed set (`AGENTS.md` rule 3). The one input handed before a pass's narrowing is
   an input whose append is in doubt, handed again at the top of `step` and in `drain`: it is
   re-handed on the view it was first decided with, so the journal is offered the same bytes
   (journal spec §5.1). Narrowing first could turn a response whose first append committed into a
   different record. The response was judged against the set the control stream resolved when it
   was read, and the next pass narrows before anything newer is handed.
4. **The wiring.** `Wiring::approvers` is an `Option<ApproverSource>`; the `mandate-daemon` binary
   (`mandate-runtime-models`) sets it from the resolved deployment. The fixture wiring has none and
   keeps its view's approvers, which grant nothing the deny-all wiring asks for.
5. **No record changes.** The narrowing reads the control stream the pass already reads and
   journals nothing, so no spec, schema, or reference case moves.

### Consequences

- The runtime's admitted approvers are always a subset of those admitted at its start, and of
  those the control stream resolves at the pass, as the shell's are.
- Tests: `crates/mandate-daemon/tests/deployment.rs`
  (`the_admitted_approvers_narrow_at_each_read_and_never_grow`) and
  `crates/mandate-daemon/tests/runtime.rs`
  (`a_connection_revoked_while_running_leaves_no_approver`, and
  `a_grant_in_the_pass_that_revokes_its_approver_is_refused`, which fails if the narrowing moves
  after the pass's records are handed).

### Alternatives considered

- **Re-resolving without the intersection.** It would admit a member granted the role mid-run
  without the restart the shell needs, so the two readers would disagree.
- **Restarting the process on a membership change.** It drops in-flight state for a change a read
  already handles.

<a id="dec-2821"></a>

## DEC-2821: A broker answer the executor's core cannot apply holds the account's openings

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: it replaces a stop that also stopped every exit, protective order and kill switch, and restarted into the same broker answer, with a hold of openings only; amends DEC-2805 item 4. The coordinator assigned it) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane MC (Claude Code), number from the coordinator's trading-core range |
| **Story or claim** | X10d (backlog `X10-core-error-fail-safe.md`, from DEC-2805 item 4); `AGENTS.md` rules 3 and 13 |

### Context

DEC-2805 held a batch the journal refuses, but left an error from the executor's core itself
stopping the process. The core returns an error when it cannot apply a broker answer: for example a
fill whose quantity times price is beyond what a decimal holds, so the fold cannot book it. On a
paper account that is one malformed broker payload. The process stopped, the restart read the same
broker activity and stopped again, and while it looped no exit, protective order or kill switch ran.

### Decision

1. **A core error on a broker input is held** (`mandate-executord`). The broker's answers to the
   executor's requests, its streamed updates, its reconciliation snapshots and the quotes its feed
   sends are the inputs held: the ones the restart would read again. A quote is held because the
   watchdog's exit is handed from it, so a quote the core cannot apply would otherwise stop exits. The process hands the core `Input::InputFailed`, which
   holds every agent's openings under `reconciliation:incomplete` with the owner's
   `reconciliation` alert, exactly as DEC-2805's refused batch does, once while the hold is in force.
   The owner's acknowledgment of `incomplete` lifts it. The operator is alerted once per process
   (`input_refused`), and a snapshot the core could not apply is a reconciliation that did not pass.
2. **Nothing of the failed input is journaled or sent.** The core drafted nothing for it, and none of
   its broker requests is sent. The failed input may have been the answer to a submission, which the
   core then never learns of, so `InputFailed` also moves an order still awaiting its answer to
   `Unknown` and looks it up by its own id, exactly as on a broker that answers nothing (trading spec
   §5.7): its acceptance or fill is found and its protection placed, and it is never sent again.
   Two process-local notes taken before the core's step stay: that an account kill switch's
   cancel-all was answered, and the broker read a switch's close is judged from. Neither adds an
   opening: at most a switch's close waits for its next read, or is tried from a read the core
   could not book, and a close only reduces risk (`AGENTS.md` rule 13).
3. **What still stops the process**: a core error on any other input (a tick, a handoff, a command,
   a journal row, the start), and on the hold itself, as before. Those are the
   executor's own inputs, not a broker's, and an error there is a defect to see, not data to ride
   out. DEC-2805 item 4's "an error from the core itself" now reads: on any input but a broker's.

### Not covered

- An answer to a cancel, a lookup or a listing that the core cannot apply is dropped like any
  other; its order stays as the journal last left it. An `Unknown` order is looked up again on its
  cadence; a cancel left unconfirmed is asked again by the next reconciliation or restart.
- A reconciliation snapshot that replaces one whose append is still in doubt, and then fails, stops
  the process: the hold cannot be journaled past a batch in doubt, so it stops as before.

- A broker answer that fails on every pass, such as a malformed activity a reconciliation reads
  each time, keeps the hold in force and every reconciliation from passing, as DEC-2805's
  persistent refusal does: the executor stays up with its exits, and the operator alert says why.

### Rationale

- `AGENTS.md` rule 13 makes exits, protection and the kill switch the last things to stop, and rule
  3 resolves the ambiguity of an answer the executor cannot read to a hold of what adds risk.

### Alternatives considered

- **Skip the input silently.** Rejected: the account may have moved in a way the executor cannot
  see, so openings must wait for the owner.
- **Hold every core error, whatever the input.** Rejected: a tick or a handoff failing is the
  executor's own defect, which a hold would hide.

<a id="dec-2823"></a>

## DEC-2823: The executor copies a research agent's working universe onto the account stream

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: it writes the record mandate spec §8.5 and journal spec §9.3 already give the executor, and takes the readings below where they are silent; each only records, and none opens, sizes or holds an order. The coordinator assigned it; file boundaries agreed with lane X) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | Lane MC (Claude Code), number from the coordinator's trading-core range |
| **Story or claim** | E17-3 (backlog `E17-3-executor-writes-universe-changed.md`, from DEC-2819 item 5); mandate spec §2.3, §8.5, §8.6; journal spec §9.3 |

### Context

Mandate spec §8.5 has the executor write `UniverseChanged` on the account stream for each admitted
thesis and each removal §8.6 names, with a `causation_id` naming the agent stream's record, so the
account stream is a self-contained fold of the working universe. Journal spec §9.3 closes the record.
No executor wrote it: the executor's fold refused it as E17-3's, and the executor process read a
research agent's universe from the agent stream through `mandate_research::replay_theses`
(DEC-2819).

### Decision

1. **The route.** Each pass, the executor process hands the core every followed agent's thesis
   records (`ThesisProposed`, `ThesisRevised`, `ThesisInvalidated`), in stream order, as
   `Input::Theses`, after the pass's proposals, so a copy whose append is in doubt never holds that
   pass's exits, as a mark in doubt does not (#2449). An opening on an instrument just admitted waits
   a pass for its copy, which only holds. A stream that does not read holds that agent as the
   proposals' hold does.
2. **The copy is a difference.** The core replays the records at its risk clock with
   `replay_theses`, the one replay the runtime and the process already use, and journals each
   difference from the universe the account stream folds to: first each instrument a thesis held
   that no surviving thesis holds now, then each surviving thesis the stream does not hold yet. The
   same records handed again, on the next pass or after a restart, add nothing, because the fold
   already holds what was written. So each change is copied once, without a second fold or a
   ledger of copied record ids.
3. **What each copy names**, where §9.3 is silent:
   - an admission: `thesis_admitted`, naming the last record that admitted the thesis. A renewal
     or a newer thesis on an instrument the stream already holds is copied as an admission of the
     new thesis, with the universe's size unchanged.
   - a removal: `thesis_invalidated`, naming the `ThesisInvalidated`; else `lineage_retired`, naming
     the refusal that retired the lineage (§8.6 item 4); else `thesis_expired`, naming the record
     that admitted the thesis, since an expiry has no record of its own. This is the replay's own
     order of reasons.
4. **The fold.** The executor's fold reads `UniverseChanged` into each agent's working universe,
   with the thesis and lineage that hold each instrument. An instrument a pinned list admitted
   (`version_applied`, no thesis) is the version's: the research replay never removes it.
5. **Nothing is written, and nothing stops,** while there is no risk clock, no confirmed mandate
   version for the agent (the record's `man` reference), records that do not replay, or an
   instrument that is not an id; a later pass writes it. The copy's input is the research agent's,
   so a core error on `Input::Theses` is held like a broker's (DEC-2821): openings held, exits on,
   never a crash loop. The process's own reading of the stream holds openings meanwhile
   (DEC-2819). No order is sized from the copy: the runtime reads a removal to end its thesis
   (mandate spec §8.6), which proposes a sale and so only reduces risk. A record that does not
   replay, or a malformed row on the agent's stream, writes no copy for that agent, expiry removals
   included, until it reads again; that only delays the record. The replay runs at the risk clock's
   whole second, so an expiry with a fractional second is copied up to a second late.
6. **Readers.** The executor process keeps its own reading of the agent stream for its universe
   (DEC-2819 item 2), as lane X agreed; a property test pins the account stream's universe to what
   the thesis records derive. The account risk fold (`AccountRiskFold`, R2b, DEC-2804) maps
   `UniverseChanged` as a risk input it does not read yet, so once it is wired a copy makes its
   view unreadable and holds openings, never exits. It has no production caller today; backlog row
   `E17-3-risk-fold-reads-universe-changed` holds the reading.
7. **Pinned and research instruments are disjoint.** A thesis is admitted only under an unpinned
   mandate (mandate spec §8.5), so a thesis never replaces a pinned holding; were one to, it would
   become removable with its thesis.

### Rationale

- A difference against the fold is idempotent by construction, so a restart, a re-read or an
  append in doubt never writes a second copy, and the account stream alone replays the universe.
- Reusing `replay_theses` keeps one judgement of §8.6's lifetime rules for the runtime, the process
  and the copy.

### Alternatives considered

- **Copy each record as it arrives, deduplicated by its id.** Rejected: expiry has no record, and
  a ledger of copied ids is a second state to keep in step with the fold.
- **Have the executor process read the account stream's universe now.** Deferred to lane X: the
  replay already reads it, and the property test keeps the two equal.

<a id="dec-2825"></a>

## DEC-2825: A research pass that cannot read its context says which input it could not read

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79, asked for by the coordinator on 2026-10-26. It adds no risk: research still holds exactly when it held, and the alert carries an input's name, never its content) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane X, claim [#754](https://github.com/kunwarshivam/mandate/issues/754)) |
| **Story or claim** | E21-54, the research workspace's demo loop; [DEC-2208](intelligence.md#dec-2208), [DEC-2214](intelligence.md#dec-2214), [DEC-2251](intelligence.md#dec-2251) |

### Context

In the cell's demo run, the research agent's runtime went `normal` after its executor reconciled.
Then it journaled nothing for minutes, with no research call. The runtime's research step asks its
source for the pass's context (DEC-2208). When the source cannot read any one of a dozen inputs, it
answers nothing, and the step holds. The inputs are the control, agent and meter streams, the
deployment, the risk clock, the day's spend, the thesis records and the bundle's facts. The step
raises no alert, writes no log line and journals no record. So a held research agent looks exactly
like an idle one, and finding which input held it took a full capture of the cell's streams.

### Decision

1. **The source names the input.** `ResearchSource::parts` answers
   `Result<ResearchParts, ContextUnread>`. `ContextUnread` is a closed set of the inputs a context
   is read from, each with a stable code (ADR-0001 ES-09):
   - `stream_name`, `control`, `agent_stream` and `meter`;
   - `reservation`, `deployment`, `envelope` and `event_triggers`;
   - `deployments`, and `another_agent` (research does not run beside another deployed agent,
     DEC-2214 item 2);
   - `clock`, `spend`, `theses` and `pinned_with_theses`;
   - `instrument_facts`, `halted` and `arrivals`.

   The cell's source maps each input it cannot read to the first one, in the order it reads them.
2. **The runtime says it once.** The first pass that cannot read its context raises one operator
   alert, `research_context_unread`, with the reason's code as its subject. A later pass with the
   same reason raises nothing. A different reason raises one more. A pass that reads its context
   clears the reason, so the next one it cannot read alerts again. The alert's subject is the code
   alone, generic text that names no stream, instrument, amount or content (`AGENTS.md` rule 6).
3. **Nothing else changes.** Research holds exactly when it held, and the hold still keeps a retry
   in doubt on the context it was built with (DEC-2208 item 4).

### Rationale

A hold that cannot be seen is a defect whatever its cause. Naming the input turns the next cell run's
silence into the one alert that says where to look, at no risk: the alert reads nothing more than
the hold already decided.

### Alternatives considered

- **Log the reason, with no alert.** Rejected: the operator would not see it without the pod's logs,
  which is the gap this closes.
- **Alert on every pass.** Rejected: a held agent would raise one alert a tick.

<a id="dec-2828"></a>

## DEC-2828: The executor copies and judges the owner's acknowledgments

| | |
|---|---|
| **Status** | Accepted (an agent, under DEC-79: an engineering decision that builds journal spec §9.28's copy and judgment as written; where it reads the spec, it takes the reading that lifts nothing more) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane L1), assigned by the coordinator; the executor's boundaries agreed with lanes X and RK |
| **Story or claim** | Backlog E7-4 slice 5, the acknowledgment copy only, ahead of E7-2y slice C ([DEC-2824](DEC-2824.md)); [journal spec](../../specs/journal.md) §9.28 |

### Context

Journal spec §9.28 has the executor copy the control stream's `OwnerAcknowledged` and
`OwnerAcknowledgmentRequested` into the account stream and judge each acknowledgment copy by four
checks. The executor has never done either:

- the executor's step for a followed fact answers `Unimplemented` (E7-4);
- executord hands it no control-stream record of either type;
- the fold's `OwnerAcknowledged` reader reads a pre-§9.28 shape, with a `subject` member and a text
  `step_up`, which `append` has refused since §9.28 closed the schema.

So no owner can lift a latched limit.

Two of §9.28's inputs are not on the account stream:

- the assertions the control stream spent before the original, for §6.1's reuse check;
- the overlay's independence requirement at processing.

Neither is in the executor's ports. Neither is the environment's set of step-up methods.

### Decision

1. **A new input, `Input::Acknowledgment(AcknowledgmentHandoff)`.** The shell hands one control
   record of either type with:
   - `schema_version`: the original's, which the copy keeps;
   - `processed_at`: the whole second of the shell's clock when it hands the record;
   - `spent`: the assertions §6.1's four record types spent on the control stream before it;
   - `request_spent`: for an acknowledgment that names a request, the same set before that request,
     which check 4 judges the request's own step-up against. It is handed with the acknowledgment,
     so a restart between the request's copy and the acknowledgment's loses nothing;
   - `independent_overlay`: the overlay's `independent_approval_required` at processing. For a
     limit, this is the overlay of `agent`. For a restriction, it is the strictest over every agent
     deployed on the account, as DEC-2824 item 1 says;
   - `methods`: the methods the environment allows (mandate spec §6.1, DEC-529 item 3).

   The spent sets are fixed by the control stream. The overlay and methods are read at processing,
   and the shell keeps the handoff it built until the copy's batch resolves, so a retry hands the
   same input. The executor journals its judgment, and a replay reads that record and never judges
   again.
2. **The copy and its judgment are one batch.** The batch is:
   - the copy first: the original's payload and version unchanged, `causation_id` the original;
   - then, for an acknowledgment copy that fails a check, `OwnerCommandRefused` (`command`
     `acknowledge`, the reason) as the very next record, as §9.28 now states (v0.102).

   A request copy is never judged by itself and is never followed by a refusal. An original the
   account stream already holds a copy of is never copied again, so a shell that hands the control
   stream again after a restart judges nothing twice.
3. **The checks are judged at the later of the handoff's `processed_at` and the executor's risk
   clock.** This stands for the copy's `event_time`, which the shell stamps at append and the core
   never sees.
   - The reference judges at the whole second of `event_time`.
   - A risk clock that lags the shell's clock is never used alone, so a stale step-up is never
     judged fresh because ticks were late.
   - The shell reads its clock once per copy. It hands that reading's whole second as
     `processed_at`, and stamps the same reading as the `event_time` of the copy's batch. The
     second judged at is therefore exactly `event_time`'s whole second, as §9.28 and the reference
     judge, and never earlier. This binds executord's hand-off (the next slice).
   - A step-up's `authenticated_at` is in whole seconds, and the window is inclusive at 300 s.
   - The refusal's `effective_at` is the second judged at.
4. **The fold reads the closed schema.** Copies and refusals are read as journaled. The pre-§9.28
   reader is removed:
   - it cleared reconciliation holds (a mismatched instrument, external activity, an incomplete
     batch) by a `subject` member;
   - no record `append` admits carries that member any longer.

   Those holds now stay until a spec names how an owner acknowledges them. That is the reading that
   lifts nothing. Backlog row E7-4b, `docs/project/backlog/E7-4-reconciliation-acknowledgments.md`,
   holds it.
5. **What an accepted acknowledgment does is not this slice's.**
   - A limit's lift is the risk book's (lane RK's R2b). It reads the copy as accepted when the next
     row is not its refusal, and appends `RiskLimitLifted` (`owner_acknowledged`).
   - A restriction's lift is E7-2y slice C's.
   - Version 2 records reach the executor only once `mandate-journal` registers them (E7-2y slice
     B). Until then a copy of `subject` `restriction` is refused `trigger_unknown`, since no
     `RiskLimitTriggered` has its id. That lifts nothing.

### Rationale

- Handing the control-stream inputs with the record keeps the executor's core pure and its judgment
  replayable. The core never reads the control stream itself, as it never reads the broker
  directly.
- One batch, with the refusal adjacent to its copy, lets every reader decide an acknowledgment
  from the account stream alone, with one row of lookahead.

### Alternatives considered

- **Fold the control stream's spending records into the executor's state.** Rejected: the state
  must be derivable from the account stream on replay, and these records are not on it.
- **Copy only accepted acknowledgments.** Rejected: §9.28 and its vectors copy every one, and a
  refused copy on the stream is the audit record that the owner tried.

<a id="dec-2831"></a>

## DEC-2831: A held retirement is answered only by its own `retirement_held` alert

| | |
|---|---|
| **Status** | Accepted (agent: a reading under DEC-79 and DEC-176 that only tightens: it refuses a stream the reference fold admits, and admits none it refuses) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | E8-17; [journal spec](../../specs/journal.md) §9.34 rule 266(a); [DEC-2352](DEC-2352.md) item 3 |

### Context

Rule 266(a) says each `RetirementHeld` "is followed by its `OwnerAlertSent` (`retirement_held`,
`subject` it)". The reference fold (`reference/journal/retirements.py`, `stream_failure`) checks that
the next row is an `OwnerAlertSent` naming the record, but not its `kind`. It leaves the kind to rule
130's "one subject, one kind" at append, which stops two alerts on one subject but not one alert of
the wrong kind. `mandate-journal`'s `retirement_failure` (#2697) first followed the reference, and
the mutation sweep found that no vector tells the two readings apart.

### Decision

1. **The fold reads the rule as written.** A held record is answered only by an `OwnerAlertSent` of
   kind `retirement_held` that names it. A next row of another kind, such as `kill_switch` naming
   the record, leaves it unanswered, and the fold refuses the held record.
2. **An alert of another kind is no retirement's.** It is not read by rule 266, whatever it names.
3. **No vector changes.** The reference fold and its 28 streams still agree with this fold, since no
   stream holds an alert of another kind after a held record. The edge cases are pinned by
   `crates/mandate-journal/tests/retirements.rs`
   (`a_held_record_is_answered_only_by_its_own_retirement_held_alert`). When the spec's vectors are
   next regenerated, the reference fold should take the same check.

### Rationale

An owner told of a held retirement through another kind's text is not told of it: `kill_switch`
says a switch was activated. The tighter reading refuses that, and adds no risk.

### Alternatives considered

- **Keep the reference's reading.** It admits a stream the spec's sentence refuses.

<a id="dec-2835"></a>

## DEC-2835: The silence finding reads only the agent's own ends and applied versions

| | |
|---|---|
| **Status** | Accepted (agent: a reading under DEC-79 and DEC-176 that only tightens: the finding is reported in every case the reference reports it, and in a few more) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane D1) |
| **Story or claim** | E8-17; [journal spec](../../specs/journal.md) §9.34's `retirement_held_silent`; [DEC-2352](DEC-2352.md) item 4; [DEC-2831](trading-core.md#dec-2831) |

### Context

§9.34 says an open sequence is silent when its last `RetirementHeld` is more than 48 hours old,
"with no end and no version that cancels it since". The ends are rule 266's: this agent's
`AgentStopped`, or an owner's `resume` or `stop` scoped to it. A version cancels only once applied to
this agent. The reference's `silent()` (`reference/journal/retirements.py`) clears the sequence on
any `AgentStopped` or `OwnerCommandIssued` and on any `MandateVersionApplied`, whoever it names and
whatever its result. Its `silence` vectors hold only this agent's records, so no case tells the two
readings apart.

### Decision

1. **`mandate-journal`'s `retirement_held_silent` reads the ends as rule 266 does.** Another agent's
   stop, an owner's `pause`, or a workspace-scoped command ends nothing.
2. **A version cancels only once applied to this agent.** A rejected version, or another agent's,
   cancels nothing.
3. **A version whose terms cannot be read cancels nothing**, so the finding is reported rather than
   missed (`AGENTS.md` rule 3). The same holds for a trigger whose figures do not read, or whose
   `capital_base` is not positive.
4. **Each sequence is read against its own first record's version.** An end or a cancelling
   version closes the sequence and forgets the version it was held under, so a later sequence's
   `end_date` or `on_complete` is compared with its own first record's, as §9.34 says. The
   reference's `silent()` keeps the first sequence's for good, which would let a version cancel a
   second sequence it does not cancel and drop a real finding. A record whose risk clock cannot be
   read is as old as can be, so it is reported as silent rather than ending the sequence.
5. **The return is compared without division.** `profit_level` × `capital_base` above
   `agent_equity` − `capital_base`, in fixed-point `Ratio`, which is the reference's comparison for a
   positive base.
6. **The reference follows at the next regeneration** of the `retirements` vectors, with a case
   for each of items 1, 2 and 4, as `docs/project/backlog/E8-17-reference-retirement-alert-kind.md`
   records for DEC-2831.

### Rationale

The finding exists so that a writer that stopped while the agent still held something is seen. A
reading that lets another agent's records or a rejected version silence it would hide exactly that.
The tighter reading can only add findings, never drop one.

### Alternatives considered

- **Follow the reference's looser reading.** It agrees with every vector, but it lets records about
  another agent end this agent's finding.

<a id="dec-2837"></a>

## DEC-2837: The executor process probes its connection's health on a fixed cadence

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176. A read-only call that only feeds the restriction DEC-2832 already writes; it places, alters and lifts nothing, and adds no tunable) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | E7-2z (backlog `E7-2-connection-restriction.md`): the scheduled health probe. Connections spec §8.2; DEC-687; DEC-2826; DEC-2832; E21-26 (the journal-outage hold); X1b-3 (the scheduled reconciliation) |

### Context

DEC-2832 counts the broker's answers per class, but only answers to requests the process made for
other reasons. An executor with nothing working reads nothing, so an idle agent on a revoked
credential, or on a broker that stopped answering, was never restricted. Connections spec §8.2
says the executor runs a health probe on a schedule (Proposed: every 60 seconds while any agent is
deployed, every 15 minutes otherwise), with a read the connector already has (`GetAccount`), never
from the reserved exit budget. Lane EX's coordinator asked that the probe add no new tunable.

### Decision

1. **One fixed cadence.** Every process probes every 60 seconds (`PROBE_EVERY_S`). That is the
   faster of §8.2's two cadences, so no executor probes less often than §8.2 asks. The process
   reads no deployment state to choose between them, and nothing configures it.
2. **A read that only counts.** The probe reads the broker's account: `GetAccount`, or a one-read
   broker's account reads. Its answer feeds the per-class counts of DEC-2832, so:
   - a lost or unreadable answer is a network failure, never healthy;
   - a refused credential is an authorization failure.

   The answer is never handed to the core, and a failed probe never stops the process. The probe
   places, alters, and cancels nothing.
3. **Last in a pass that went through.** It runs where the scheduled reconciliation runs: after
   the pass's kill switches, proposals and marks, so no read goes ahead of what the pass sent. The
   troubles it finds are reported to the core at once (DEC-2826), so the restriction is journaled
   in the same pass. A pass that finds the journal down returns before the probe, so while an
   append is in doubt nothing reaches the broker (E21-26). An idle executor, which appends
   nothing, cannot know the journal is down, so its probe still reads. A restriction that probe
   leads to is then held in doubt like any append, and journaled first once appends succeed.
4. **What it costs.**
   - **Alpaca:** one read a minute, well inside Alpaca's 200 requests a minute. §8.2's read budget
     is not yet metered separately from the exit budget (§6.5), so the probe is one read.
   - **Robinhood:** its connector offers no single account read (`GetAccount` is `NotSent`), so the
     probe uses its account reads: three tool calls, portfolio, positions and orders. A record in
     them that does not parse is `Unreadable`, a network failure. So an account whose records stay
     unreadable for three probes is restricted to closing even if the connection answers. That
     follows the reading the rest of E7-2z takes: an answer the platform cannot read is never
     healthy, and halting openings adds no risk (`AGENTS.md` rule 3). The owner's acknowledgment
     lifts it once the records read again (E7-2y). A refused Robinhood credential also counts as a
     network failure, not an authorization one, as the backlog row records.
5. **When it does not run.** The probe needs a pass that went through, so it does not run:
   - before the first tick;
   - while an append is in doubt, or the control stream cannot be read, or a row of an agent's
     stream does not parse (an agent stream that cannot be read at all does not stop the pass);
   - in a pass a handoff stopped;
   - once the process exits.

   Each of those is already a hold or a stop that alerts on its own. A process restarted more
   often than once a minute never probes, and its restarts are what the operator sees.

### Consequences

- An idle executor on a dead or revoked connection restricts its account within three probes for
  network failures, or two for a refused credential, at most about three minutes.
- §8.2's recovery count (three good probes leading to `condition_cleared`) can read these probes
  when E7-2y builds it.

<a id="dec-2838"></a>

## DEC-2838: The CLI reads rule 240's subject with version 1 as a limit

| | |
|---|---|
| **Status** | Accepted (an agent, under DEC-176: the reading only tightens rule 240 and adds no risk) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane L1) |
| **Story or claim** | Backlog E7-4b; [journal spec](../../specs/journal.md) §9.28 (v0.104, v0.106); [DEC-2824](DEC-2824.md) item 1; [DEC-2830](DEC-2830.md); [DEC-2512](notifications.md#dec-2512) items 2 and 4 |

### Context

Journal spec §9.28 says rule 240's equal members include `subject` "when both records carry it",
"so a request for a limit is never answered by an acknowledgment of a restriction". Read word for
word, the first clause compares nothing when one of the records is version 1, which has no
`subject`. A version-1 request, which reads as a limit's, could then be answered by a version-2
acknowledgment of a restriction or a reconciliation hold, which is exactly what the second clause
says never happens. `mandate agent acknowledge --subject` (E7-4b) must pick one reading before it
checks a request.

### Decision

1. The CLI compares the subject of the request and of the acknowledgment on every acknowledgment
   that names a request. A version-1 record, and a command given no `--subject`, names `limit`, as
   §9.28 reads version 1. An acknowledgment that names another subject than its request is refused
   `request_invalid` and commits nothing, as for another agent or event.
2. A request whose `subject` is present and not text matches no acknowledgment.
3. This tightens rule 240 only where the spec's two clauses disagree, and follows the second.
   It adds no risk: a refused acknowledgment lifts nothing, and the owner can request again with
   the right subject.
4. The executor's own rule-240 comparison (`MATCHED` in `mandate-executor`'s `acknowledge.rs`)
   is unchanged here; whether it compares `subject` the same way is a separate change, the backlog
   row `E7-4b-executor-request-subject.md`.
5. **The founder's live carve-out is not yet enforceable in the executor process.** Journal spec
   v0.106 §9.28 check 3 (DEC-2523) admits `cli_confirm` on the founder's own live connection
   (DEC-529 item 3, DEC-923). `mandate-executord` cannot yet tell that connection from another, so
   it hands every live acknowledgment with `methods` `{passkey}` alone (#2734): a live
   acknowledgment requires passkey until the executor process can identify the founder's connection
   from the connection's own journaled facts. This fails closed and narrows a founder decision in
   practice; it is on the founder's list, and the backlog row
   `E7-4b-founder-connection-step-up.md` owns the fix.

### Consequences

- The CLI writer of §9.28 version 2 (#2739) refuses mismatched subjects before append.
- The workspace API's acknowledgment route should apply the same reading, so both writers agree.

<a id="dec-2839"></a>

## DEC-2839: A lost credential restricts the account before the executor drains

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176. It journals a restriction the specs already require, before a stop the process already made; it lifts, loosens, and holds no exit) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | E7-2z (backlog `E7-2-connection-restriction.md`): the suspending causes of connections spec §9.1. §9.2 "Vault outage"; DEC-2306 item 3 (the lease); DEC-2826; DEC-2832; DEC-2821 |

### Context

Connections spec §9.2 says that at a vault outage the executor keeps its lease until it expires,
and "at lease expiry it is treated as `suspended` for openings". `mandate-executord` drains the run
when its lease loses the credential (`credential_lost`). Nothing was journaled, so the supervisor's restart,
which cannot lease while the vault is down, left the account with no restriction on its stream,
and any other reader of the stream saw an account free to open.

E7-2z's other suspending causes have no source here yet:
- `credential_expired` and `refresh_failed`: Alpaca documents no token lifetime or refresh (U-A3,
  connections spec §5.4). An expired token shows as a `401`, which DEC-2832 already counts as an
  authorization failure.
- `check_failed`: `mandate-executord` runs §8.1's checks only at connect, not at every start or
  daily, so no later check can fail yet.

### Decision

1. **Restrict, record, then drain.** When the run's lease stops it with `credential_lost`, the
   process hands the core `Input::Connection(Suspended(LeaseExpired))` twice before it drains.
   - The first hand journals the `connection_unavailable` restriction, every agent `exits_only`
     and the owner's `OwnerAlertSent`, in one batch (DEC-2826, DEC-2514). The cell's dispatcher
     delivers that alert from the journal, so the owner is told although the process then stops.
   - The second hand journals the connection's `ConnectionStateChanged` to `suspended`.
   - So the restarted process folds both, and the connection is `suspended`, not `active` with a
     restriction in force.
2. **An input in doubt is never replaced.** An input whose append is already in doubt is handed
   again first, as a pass does. While it stays in doubt nothing more is handed, and the drain
   retries it. A process that has exited hands nothing.
   - **The cost of retrying only once.** The drain retries only the input in doubt, once. So with
     the journal down at the lapse, the restriction commits through the drain, if at all, and the
     state change does not. The restart then folds an `active` connection with the restriction in
     force. That is safe, since openings stay halted, and it is the state E7-2y's lift must also
     accept (the backlog row says so).
3. **`lease_expired` for every lost credential.** §9.2's vault outage is `lease_expired`, as
   DEC-800 item 5's reading of §8.2 and §9.1 names it. The lease loses its credential when:
   - it expired, the vault having refused to renew it until then;
   - the vault refused a renewal outright;
   - a renewed secret does not read.

   None of these is a refused credential at the broker, so none is `authorization_failed`.
4. **Not a rotation, not a stop.** A rotated credential (`credential_rotated`) and SIGTERM drain
   with no restriction. Neither says the connection cannot be used.
5. **Held, never a stop of its own.** A core error on the input is held (DEC-2821). The drain runs
   as before, so the exits it owes still go (`AGENTS.md` rule 13).

### Consequences

- An executor that loses its credential leaves its account restricted to closing and its
  connection `suspended`, with the owner told by `connection_unavailable`.
- The restriction is sticky. §9.1 ends `suspended` only by re-authorization (DEC-800 item 5,
  DEC-824 item 6), then the owner's acknowledgment (E7-2y). That holds even when the vault comes
  back and the same credential leases again. It is the reading that adds no risk.
- `check_failed` waits for the daily and at-start permission checks. `credential_expired` and
  `refresh_failed` wait for U-A3. Both are on the backlog row.

<a id="dec-2840"></a>

## DEC-2840: The executor process runs the permission checks at start and daily

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176. Read-only checks the spec already requires, whose only effect is a restriction it already defines; a check that cannot run is never read as passed) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | E7-2z (backlog `E7-2-connection-restriction.md`): the suspending cause `check_failed`. Connections spec §8.1 ("at connect, at every executor start, and daily"), §9.1; journal spec §9.8 (`ConnectionChecked` occasions `executor_start` and `daily`, rule 62, rule 68); DEC-674; DEC-2826; DEC-2832 |

### Context

Connections spec §8.1 runs checks 1, 2, 3 and 7 at connect, at every executor start, and daily, and
a later check that fails moves the connection to `suspended` with reason `check_failed` (journal
spec §9.8, DEC-674). `mandate-executord` ran them only at connect, in a check that takes the
account stream's writer epoch, so it cannot run inside a running executor. Nothing produced
`check_failed`.

### Decision

1. **The same checks, from the same code.** The Alpaca connect check's input is now one function,
   `connect::alpaca_report`. The connect check and the process's later checks both call it, on
   their own occasion. `connect::checked` journals the report's own occasion, no longer always
   `connect`. A broker says it runs later checks with `Broker::has_later_checks`, which answers
   `false` by default, and answers them with `Broker::later_check`. The Alpaca broker does both,
   and `Named`, which wraps every broker in the binary, passes both through. A broker without them
   (Robinhood today) is never read for one (backlog row).
2. **When.** Once the stream holds its copy of the establishment (rule 68), the process checks:
   - with occasion `executor_start`, at its first pass that went through;
   - with occasion `daily`, at the first such pass on each later New York calendar date.

   It runs last in the pass, like the probe, so no read goes ahead of what the pass sent.
3. **What it reads.** It reads the account with `GetAccount`, counted toward the connection's health
   (DEC-2832), and places, alters and cancels nothing.
   - **Could not run.** A read whose answer was lost, whose credential was refused, that was never
     sent, or whose answer could not be read means the check could not run. Nothing is journaled,
     its failure counts toward the network or authorization class only, and it is tried again at
     most once every 60 s (`PROBE_EVERY_S`). Such a read is never a pass, and never a failed check.
   - **Refused.** A broker that answered and refused the account read says the credential no
     longer reaches the account, which fails check 3 (`account_unreadable`). The Alpaca connector
     answers a `401` as an unknown outcome (DEC-2832), so a refusal here is the broker's own, such
     as a `403` or a `404`. One such answer is enough: halting openings adds no risk (`AGENTS.md`
     rule 3), and exits go on.
   - **The account's name.** The check names the account by the reference the stream's latest
     check that read it gave (rule 62).
4. **What it proves.** For a paper Alpaca API-key connection, checks 1 and 2 pass by construction:
   `KeyPermissions(None)`, and paper reach. Check 3 proves only that the credential still reads an
   account. It reuses the earlier reference, so it does not prove the account is the same one. A
   paper OAuth connection (DEC-821) is checked as an API key, the same input its connect check
   uses. Each of these is a limit of the connect check too.
5. **Journaled as the process's own record.** It is journaled as `ConnectionChecked`, under the
   process's epoch, with its `account_pii_ref` in the envelope's `pii_refs` (rule 62). It goes
   through the mark writer's path:
   - a refused batch is alerted once and never stops the process (DEC-2805);
   - one in doubt is offered again, the same bytes, before any other append.

   Either way the day's check counts as done.
6. **A failed check suspends.** If any result fails, the connection's health holds the trouble
   `Suspended(CheckFailed)`. The core then journals the `connection_unavailable` restriction and,
   in a later batch, `ConnectionStateChanged` to `suspended` with reason `check_failed` (DEC-2826).
   A later passing check does not clear it, because only a replaced credential ends `suspended`
   (§9.1). An MCP connection's contract drift alone should degrade rather than suspend
   (`CheckReport::later_state`). Alpaca runs no contract check, so this matters only when MCP
   later checks are built.

### Consequences

- A connection whose permissions or account no longer check out is restricted to closing within a
  day, or at the next executor start.
- `credential_expired` and `refresh_failed` still have no source. Alpaca documents no token
  lifetime or refresh (U-A3), and an expired token already counts as an authorization failure
  (DEC-2832).

<a id="dec-2841"></a>

## DEC-2841: The daemon's order plan opens on a research agent's working universe

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It adds no risk: an unpinned mandate opens only on the one instrument the executor would open in, read the same way, within the envelope's asset classes; anything unreadable opens nothing; exits and the flatten only gain instruments) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-26 |
| **Story or claim** | E21-2, the research workspace's demo loop. Tests first in #2759 (lane GW). [DEC-2378](DEC-2378.md) item 10, [DEC-2819](DEC-2819.md), [DEC-2827](DEC-2827.md) |

### Context

The deployed runtime's production ports (`mandate-daemon`'s `production`, DEC-2378) read the
mandate's `universe.pinned_instruments` alone in four places: the order plan's opening and its
classification, the gate's dry run (check 2's working universe), the view's `working_universe`
(which an approval's revalidation reads), and the flatten's asset classes. A research mandate pins
none (`universe.pinned` false), so the cell's research agent, with an admitted SPY thesis and both
models' `Long` on SPY, never decided (lane GW's faketime replay of 2026-10-27). DEC-2819 already has
the executor, and DEC-2827 the models, read the working universe from the agent stream's thesis
records.

### Decision

1. **One reading.** The production fold (`Facts`) keeps the agent stream's thesis records
   (`agent:{workspace}:{agent}`, the workspace the clock stream names) and replays them with
   `mandate_research::replay_theses` at the fold's risk clock. The instrument an unpinned mandate
   may open in is the one entry that survives, as the executor's `openable` reads it. Two or
   more, none, a record that does not read, or no clock yet, give none.
2. **The plan.** An unpinned mandate's opening is sized as a pinned one is (DEC-2378 item 10), on
   that instrument only, with the asset class the runtime's fold of its thesis gives, and only
   when the envelope's `universe.asset_classes` allows that class. Its classification denies a
   proposal on any other instrument, one whose class differs from its latest admitted thesis
   record's, or one outside the envelope's classes, so a thesis that ends between the plan and its
   classification denies the opening. An opening is never AUTO (DEC-2378 item 10).
3. **The gate's dry run.** Check 2 reads that instrument alone, with `pinned` false, and none when
   it cannot be read, which denies every opening. The liquidity figures are read by a pin's symbol
   only, so a working instrument's are deferred to the binding gate, which reads its own.
4. **The view.** The process reads the agent stream at each control and clock pass and sets the
   view's working universe to that instrument, or to none when the stream does not read, so an
   owner's grant is acted on only while its instrument is still the working one.
5. **The flatten.** A held instrument the mandate does not pin takes the asset class its latest
   admitted thesis record states, so a kill switch or owner exit of a research position is planned
   rather than counted `flatten_unplanned` (`AGENTS.md` rule 13). One whose record states no class
   that reads is still left unplanned and raises `flatten_unplanned`; the executor's kill switch
   sells its own sub-ledger of it either way.
6. **Pinned mandates are unchanged.** None of the above is read for a pinned mandate.
7. **Left to the executor.** The daemon does not read the account's opening hold
   (`Journaled::opening_hold`, DEC-2819 item 4), for pinned mandates as before: once the account has
   traded, the owner may be asked about an opening the executor then holds. That and two smaller
   edges are `docs/project/backlog/E21-2-working-universe-hardening.md`. Each fails safe.

### Alternatives considered

- **Infer the instrument from the inputs** (a `Long` plus a thesis class). A `Long` stays in the
  fold until it expires, after its thesis may have ended, and the research model's own output is
  not filtered by DEC-2827. The executor would then refuse what the owner was asked about.
- **Resolve the symbol from the registered snapshot for the dry run's liquidity figures.** That
  needs the artifact store in the production ports. The binding gate already reads the figures, so
  deferring them adds no risk; it is left as a follow-up.

<a id="dec-2842"></a>

## DEC-2842: The executor process counts the broker's rate-limit headroom

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 and DEC-176. A read of headers the broker already sends, whose only effect is the restriction §8.2 already defines; a missing reading is never read as healthy headroom) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | E7-2z (backlog `E7-2-connection-restriction.md`): the degrading cause `rate_headroom`. Connections spec §8.2 ("Rate-limit headroom: under 20% of the published or default budget → `degraded`"), §6.5; DEC-687; DEC-2826; DEC-2832 |

### Context

Connections spec §8.2 degrades a connection whose rate-limit headroom falls under 20% of the
budget, and DEC-687 counts headroom as a class of its own. Alpaca reports the budget and what is
left on every trading response, as `X-RateLimit-Limit` and `X-RateLimit-Remaining`. Nothing read
them. The transport kept only the status and the body, and a `429` counted only as a lost answer
(DEC-2832).

### Decision

1. **Recorded by the transport.** `AlpacaPaperHttp` records the limit and the remaining count of
   its latest trading response in a `RateLimitSeen`, shared by its clones. The record holds only
   when both headers read as whole numbers and the limit is above zero; anything else records none.
   A request with no response forgets the last record. Data-host reads are not recorded, since
   their budget is another one. `TradingTransport::rate_limit` reports the record, `None` by
   default.
2. **Reported by the broker.** `Broker::rate_limit` reports it, `None` by default. Alpaca's broker
   reads its transport's, and `Named` passes it through.
3. **Counted per class.** After every broker request, the process counts the rate limit in the
   headroom class:
   - under a fifth of the budget left (`remaining × 5 < limit`) is a failure;
   - a fifth or more is a good result;
   - no rate limit counts neither way.

   One failure is enough (`HEADROOM_FAILURES = 1`), because §8.2 sets no run for headroom. While
   the class fails, the connection's health holds `Degraded(RateHeadroom)`. The core restricts the
   account first, then records `ConnectionStateChanged` with reason `rate_headroom` (DEC-2826).
4. **No reading is not good headroom.** A response with no rate limit leaves the class as it was.
   Only a reading of a fifth or more clears it.
5. **Risk reduction is never held (`AGENTS.md` rule 13).** Low headroom degrades the connection,
   and a degraded connection restricts the account to closing (`exits_only`, trading spec §7.3 row
   5, DEC-2826), never to blocked. So a risk exit, a fill's protection and a kill switch go on while
   it holds. `connection_health.rs` pins each one after the restriction is journaled, and each test
   fails if the restriction is made `blocked`.
6. **A `429` both degrades and counts as lost (R3's review of #2764).** The transport records a
   response's rate-limit headers whatever its status, before reading it. So a `429` that carries
   `X-RateLimit-Remaining: 0` is a headroom failure, and degrades at once. The client reads the
   `429` itself as a lost answer (DEC-2832), one failure toward the network class's three.
   `health::tests::a_429_with_none_left_degrades_and_counts_as_lost` pins both.

### Contract drift

Contract drift (§8.2, `contract_drift`) is a signal of MCP connections only (§8.1 check 7).
Alpaca's API-key connection has no contract, so there is nothing to read for it. Robinhood's
connector, the one MCP connection, stays on the backlog row with its error folding and its later
checks.

### Consequences

- An executor that is using up its rate budget restricts openings before the broker starts
  refusing. Exits go on, and §6.5 keeps the exit budget apart.
- The class clears in the process once a later response leaves a fifth or more. The connection
  stays `degraded` until E7-2y's `condition_cleared` and the owner's acknowledgment.

<a id="dec-2843"></a>

## DEC-2843: An opening is not re-decided while the runtime's own opening works in its instrument

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It adds no risk: it takes nothing from an exit, a protective order or a kill switch, and the opening it stops journaling was always denied) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-26 |
| **Story or claim** | E21-2, the research workspace's demo loop. Amends [DEC-1150](earlier-900-1999.md#dec-1150) item 5's "journaled as the `DecisionMade` deny" |

### Context

DEC-1150 item 5 denies a second opening or increase in an instrument while one the runtime
proposed still works there (trading spec §5.3 rule 6), journaled as a `DecisionMade` deny
`working_order_limit` and notified as `decision_skipped`. The order builder keeps proposing the same
opening at every evaluation while the first waits, so lane GW's cell replay journaled 367 such
decisions, each with its alert, in about ten minutes while a granted intent waited at the
executor. That holds safely, but it floods the journal and the owner's notices with the same
refusal.

### Decision

1. **No evaluation while one works.** While an opening or increase the runtime proposed in an
   instrument still works (DEC-1150 item 4's reading, unchanged), an evaluation whose proposal is
   an opening or increase in that instrument journals nothing: no `DecisionMade`, no
   `decision_skipped`, nothing asked or proposed. A pending risk-adding approval already holds an
   opening in the same way (`awaiting_risk_approval`).
2. **What stays.** A grant's re-validation still denies `working_order_limit` (mandate spec §6.4
   check 11), so a grant read while an opening works is skipped and journaled as before. The
   binding gate keeps its own check. An exit, a protective order and a kill switch are never held
   by it (`AGENTS.md` rule 13).
3. **The first record.** The working opening's own `DecisionMade`, `IntentProposed`, and its
   account-stream records are what the journal shows of the wait. Nothing about the hold is lost:
   it ends, as before, only on the records DEC-1150 item 4 names.

4. **What the hold hides.** An opening held this way is not judged on anything else either, so an
   unusable connection's `account_restricted` deny is not journaled for it, and a hold that never
   ends (an `Unknown` order that is never reconciled) repeats no refusal. Neither adds risk: the
   executor's `connection_unavailable` restriction, the `Unknown` order and reconciliation alerts
   still reach the owner.

### Alternatives considered

- **Keep the decisions and alert once per change of reason.** The journal still grows by one
  decision per evaluation, and the alert's dedupe would need state the fold does not keep.

<a id="dec-2844"></a>

## DEC-2844: The executor marks an instrument at half its quote staleness limit

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering fix under DEC-79. It changes how often a mark is written, never what a mark says or how stale a mark may be; the reviewed rule set's 10 s staleness, an envelope value reserved to the founder, is untouched) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane EX) |
| **Story or claim** | Demo blocker found in DR's 13:42Z rehearsal: `InstrumentRestrictionChanged` `stale_mark` flapping on the demo instrument. Mandate spec §5.2; DEC-2380 item 3 (the mark writer); DEC-2804 (the risk book) |

### Context

From 13:54:19 in the rehearsal, the demo instrument's `stale_mark` restriction turned on
(`no_sane_mark`) and off (`sane_mark`) every 5 to 10 seconds. While it is on, the agent cannot open
anything.

Executord's risk book opens each agent's risk fold with `mark_max_age_s` set to the deployment's
`quote_max_age`, which is the reviewed rule set's `iex_quote_max_age_s`, 10 s. At every tick the
fold adds the session seconds since the last sane mark. A held instrument becomes `stale_mark` once
that age reaches 10 s, and a sane mark resets the age and clears it (mandate spec §5.2).

The executor wrote marks only every `MARK_EVERY`, 15 s, while the scheduler ticks once a second
(journal spec §9.20). In every 15 s cycle a mark reset the age to 0, the age reached 10 s ten ticks
later and the instrument went stale, and the next mark, at 15 s, cleared it. So the instrument was
stale for about 5 s of every 15, however fresh the quotes were. This is a cadence fault, not a
staleness one.

### Decision

1. **Half the limit.** The executor marks each instrument at half the smallest quote staleness
   limit among the deployments it serves, within 1 s and 15 s (`marks::mark_every`). For the
   demo's 10 s that is every 5 s. Marks are written at ticks, so with ticks a second apart the
   fold's age at a tick stays at about half the limit, a margin of about half the limit. The margin
   holds while ticks are never more than about half the limit apart; a limit of 2 s or less leaves
   none, and still flaps at one-second ticks.
2. **Where it comes from.** `Marked::every` says the interval: `MARK_EVERY` by default, and the
   deployment's from its deployments' `quote_max_age`. The process sets its mark poll from it at
   every pass.
3. **What does not change.** A mark still needs a quote no older than the rule set's bound, by
   the quote's own timestamp, or none is written. The fold's staleness limit, and what a mark that
   is not sane does, are unchanged.

### Consequences

- With ticks no more than half the limit apart and quotes no older than the bound, a held instrument is no
  longer marked stale between two good marks.
- If the IEX quote itself is older than 10 s by its own timestamp, which can happen on a quiet
  book, no mark is written, and the instrument goes stale as the rule says. Only a fresher quote
  source, or the founder changing the rule, changes that.
- The fold still ages at ticks. If ticks were ever as far apart as the limit, the age would reach
  it at a tick whatever the mark cadence.
- More marks are journaled, and more quotes read: one an instrument every 5 s instead of every
  15 s. For a handful of instruments that is far under Alpaca's request limit; a deployment
  marking many instruments would have to weigh it.

<a id="dec-2846"></a>

## DEC-2846: Every gate decision names the deployment's five references, and the executor's tests judge appends by the journal's rules

| | |
|---|---|
| **Status** | Accepted (agent, under [DEC-176](../04-decision-log.md#decisions), agreed by the coordinator on 2026-10-27), except item 2, which is **Proposed** for the founder (DEC-79: rule 13 names the holds an exit may have, and this is not one of them). Until the founder decides, item 2 stands as the conservative option. The rest only adds references journal spec §9 already requires and adds no risk; the founder may review it after the fact |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane X), asked for by the coordinator |
| **Story or claim** | The demo rehearsal's lost owner exit on main 0d276abf (2026-10-27 13:55Z); [DEC-457](earlier-303-599.md#dec-457), [DEC-2034](infrastructure.md#dec-2034) item 2, [DEC-2037](DEC-2037.md) item 5, [DEC-2805](trading-core.md#dec-2805), [DEC-2845](DEC-2845.md) |

### Context

Journal spec §9 requires every `GateDecided` to name five references: the fee configuration, the
trading calendar, the instrument snapshot, the rule set and the mandate version. The executor
named them only when a binding run read a snapshot.

The deployment answers no binding snapshot for an order that reduces risk (`AGENTS.md` rule 13,
DEC-2274 item 6). So every exit's decision was journaled with `config_refs: {}`. That includes a
DEC-2037 `exit_unpriced` hold and a local allow (DEC-457's `account_stream_only`).

A real journal refused the whole batch, the `IntentReceived` included. DEC-2805 then held every
agent's openings, and the exit was proposed again and refused again on every pass.

In the rehearsal the owner's exit at the market was lost this way. The account stream kept its
reconciliation rows, and nothing said why. A local allow also journaled `reason_code` as `""`,
which the journal refuses as well, where the spec says `null` on `allow`.

None of this reached a test. The executor daemon's test journal accepted any bytes.

### Decision

1. **Every gate decision names five references.** When no binding run bound a decision to
   references, the decision takes the deployment's (`BindingGateSource::config_refs`). This
   covers an intent's decision, and a ladder step parked while no session is open (DEC-260 (18)):
   - all five come from the agent's own deployment of the instrument, as an exit reads it: its
     current one, else its last, else a research agent's admitted instrument as it last resolved;
   - never from another agent's deployment of the same instrument.

   They read no quote, bars or broker fact, so an exit never waits on them. The mandate reference
   must be the agent's version in force, as a binding run's must.
2. **Where the deployment reads no references, the exit is held** (Proposed for the founder). The
   decision is `BindingGateInputMissing`, as an opening's missing input is, and nothing the
   journal would refuse is journaled.
   - **On a handoff,** the intent is held alone and alerted (`exit_held`, with the intent's id).
   - **A kill switch's flatten** is checked before it is raised. The switch, the agent's stop and
     its cancels are journaled and applied at once (`AGENTS.md` rule 13: the kill switch never
     depends on model state). Only the flatten waits, journaled `exit_unpriced` and alerted once,
     and it is raised at the first pass that reads its configuration (backlog
     `E7-flatten-mandate-dependency` asks whether it should need the mandate at all).
   - **An input's re-gates hold one intent alone.** At a tick, or after an order's change, the
     executor gates again every held or waiting intent, parks a ladder step, and journals a long
     hold. One whose configuration does not read is left as it is and decided again at the next
     step, and every other intent, protective order and kill switch of the input goes on. The gate
     journals nothing before it reads the configuration, so nothing of that intent is drafted.
   - **The executor's other own exits wait the same way** (review round 4). The stop watchdog's
     exit and the bound's exit of a plain entry's position, from the bound itself or from an entry
     that ends after it, are checked before anything that raises them is journaled. While their
     configuration does not read, no `watchdog`, `interval_limit` or `unprotected_end` record and
     no `w-` intent is drafted. The breach or interval is left unspent, the owner is alerted once
     (`exit_unpriced`), and the first step that reads the configuration raises the exit as before.
     The rest of the tick or quote goes on.
   - **No other input meets the error.** The exits the executor raises itself wait for their
     configuration in the core, and an input's re-gates hold their intent alone. A broker answer
     that does meet it, a resubmission (item 8), is held as a failed input under DEC-2821. Before
     this decision, `received` (intent.rs) already refused an intent whose mandate version did not
     read. When that happened on any input but a handoff, a tick's watchdog or bound or a kill
     switch's flatten, the process stopped.
   - The core folds only what commits, so a held intent changed nothing. The next step decides it
     again, and it goes once the references read.

   This is not one of rule 13's named holds. But the old path's outcome was a refused batch on
   every pass, the same hold, unexplained, and an account-wide hold of openings with it. A
   deployed agent's instruments always have registered references: the opening that bought the
   position was decided under them. The one exit with no single agent is item 8's.
3. **A decision with no reason journals `null`.** `reason_code` is `null` exactly when the
   decision has no reason, an allow, binding or not.
4. **A refusal says why.** The process alerts each refusal cause once a process, as
   `journal_refused_cause` with the refused record's type, the journal's reason code and the path.
   The cause carries no payload, so it says nothing of the order, the position or the mandate
   (`AGENTS.md` rules 6 and 7). The once-a-process `journal_refused` alert is unchanged.
   `serve` logs every operator alert as `alert <key> [<subject>]`, where it used to drop them all.
5. **The tests judge appends by the journal's rules.** The executor daemon's test journal judges
   every batch appended to a stream the executor opened itself. It uses the in-memory journal's
   own rules, as the Postgres journal does, and answers the refusal the real journal would.
   - A stream a test wrote or seeded by hand is not judged, since its rows were never drafts. A
     later change moves those tests onto executor-opened streams (backlog row
     `E7-executord-judged-streams`).
   - Test ids are now canonical ULIDs: the test writer's and every fixture's. `support::ulid`
     maps a short test id to one, deterministically.
   - The test ports answer the references the production deployment does.
   - It lands in two steps (ES-13). First the fix, whose own tests judge the streams they open.
     Then judging is turned on for every test.
6. **The invariants have tests.** Each is shown to fail on a seeded bug:
   - an owner exit at the market after the opening fills, and one proposed while the mark is
     stale, is received and sold;
   - the stale one is first held, journaled and alerted, and goes once a price returns (DEC-2037
     item 5);
   - the owner's agent kill switch, copied as the runtime copies it, is applied and its flatten
     sells the position.

   All three run under the journal's rules. Without this decision's references the owner-exit
   tests fail. With the journal's intent type of DEC-2845 reverted, the kill switch's test fails.

7. **Not covered here.** The executor also writes members that the closed `GateDecided`
   version 2 does not admit, so a real journal refuses those records as well:
   - `sized_qty`, on a discretionary exit sized beside an exit ladder's rungs (DEC-410 item 3);
   - `parked` and `held_long`, which its fold reads;
   - `evaluation`.

   A sized discretionary exit is therefore still lost the same way. A parked ladder step and a
   `held_long` record lose their owner alert. Fixing it needs a journal spec change: a later
   decision adds `GateDecided` version 3. Until then, item 1's references do not make those
   records journal.

8. **Residuals** (review rounds 3 and 4). Each but the last is safe under rule 13 and drops no
   exit; the last is a gap carried to the founder:
   - **A reference read that changes within one step.** An exit's first gate decision reads its
     references, then its protective cancels are drafted, then the decision before submission
     reads them again. If the second read failed after the first succeeded, the exit would be
     held with its cancels journaled, and the next step would re-gate it. The deployment reads its
     references from one control-stream length per pass, so both reads see the same answer.
   - **A broker answer that resubmits** an order a confirmed absence returned to `Intent`
     (`orders.rs` `resubmit`). With its references unread it is held as a failed input (DEC-2821),
     not alone. The process goes on and the broker answers again.
   - **An opening at a tick.** An opening whose binding input does not read is left held without
     a new alert, as rule 3 allows. Its `opening_held` alert sounded when it was first held.
   - **A watchdog on a position no single agent holds** (review round 4). Its exit is nobody's
     (`*`) wherever the position is not exactly one agent's own lots: two agents' lots, or one
     agent's beside shares the owner bought or was released (DEC-2354), or any other difference
     between the lots and the position (`single_holder`). The executor process resolves no mandate
     version for `*`. So that exit waits, alerted once, for as long as the position stays so: a
     triggered stop's risk exit does not go. That is a rule-13 gap, not one of its holds, and it is
     carried, not accepted, until the founder's direction below is built:
     - It is not new. Before this decision the same tick failed and the process stopped, every
       other exit with it.
     - DEC-2804 item 8's interim, one agent on an account, removes the two-agent case: a second
       followed agent holds every opening. It does not remove the owner's own shares beside one
       agent's, which nothing at deployment can refuse. So no deployment guard closes the gap, and
       none is added.
     - The resting stop still protects the position at the broker, and the bound and the kill
       switch, which name an agent, are unaffected.
     - **The founder's direction (2026-10-27, implementation pending):** positions are per agent,
       and two agents may hold the same stock. A shared position's watchdog, stop or bound exit
       becomes one exit per holding agent, each sized to that agent's own lots, never selling
       another holder's or the owner's shares. DEC-2849 (lane RK, in review) designs per-agent
       positions; backlog `E7-watchdog-shared-position`, ranked high, implements this exit on
       it.

### Rationale

The references are the configuration the decision is recorded under, and the deployment holds
them whether or not a binding run read a snapshot. Requiring them of every decision is what the
journal spec already says. The only change is that the executor now writes what the journal reads.
The rule-13 cost of a missing reference is a hold the operator sees, where before it was a refusal
on every pass that nobody saw.

### Alternatives considered

- **Let `account_stream_only` decisions omit the references (journal spec change).** Rejected. It
  loosens what every gate record proves, and the deployment always has the references to give.
- **Reuse the references the instrument's opening was journaled under.** Rejected. A decision
  must name the configuration in force now. A mandate version change in between would bind the
  exit to a version that no longer applies.

<a id="dec-2850"></a>

## DEC-2850: A crypto asset fee names no buy, so it cannot come off the buying agent's lots yet; the journal keeps refusing crypto `FeesCharged` until it can (lane RK)

| | |
|---|---|
| **Status** | Accepted (the founder, 2026-10-27, relayed by the coordinator: "Keep crypto off; build fix"). Item 3 holds: the journal keeps refusing a crypto asset fee. Item 1 is the fix, ranked normally on backlog row E6-15u (not demo-critical). The interim reading that takes the fee from R is rejected as a design and stays only while unreachable |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | The founder; proposed by lane RK (Claude Code) |
| **Story or claim** | E6-15, [DEC-2849](DEC-2849.md) item 5 and its definition of Lᵢ; trading domain spec §6.3, §8.1, §11; journal spec §9 (`FeesCharged`, `FillApplied` version 2); backlog row E6-15u |

### Context

DEC-2849 defines agent i's lots Lᵢ for crypto as net of the asset fee taken on a buy (§6.3: received
= gross − `fee_qty`). The executor's fold has no way to do that today:
- The fee reaches it only as `FeesCharged` with `family: crypto_asset`, which carries family, day,
  accrued and charged, and names no order or fill.
- `FillApplied`'s `fees` list carries what the broker reported on the fill, and Alpaca's `FILL`
  reports no fee.
- The fold does not hold the fee schedule or the fill's liquidity, so it cannot compute the fee at
  the fill either.

The journal's `FeesCharged` schema admits only `family: equities` today, so no crypto asset fee
reaches the fold.

### The code's interim reading, and why it is not a tightening

`mandate_executor::holders` and the fold's `fees_charged` read a crypto asset fee as lowering N and
no agent's lots, so it comes out of the owner's residual R. Against DEC-2849's figures (the buyer's
Lᵢ lower by the fee f):
- **Exits are never smaller.** I4 sizes rule-13 sells to min(Lᵢ, N), and Lᵢ is never lower than
  DEC-2849's.
- **With a deficit, it tightens.** When R ≤ 0 by DEC-2849's figures, D rises by f, the buyer's Sᵢ
  is equal and every other holder's falls by up to f, Σ Pᵢ is equal, and openings hold (I6).
- **With an owner's residual, it loosens.** When R > 0 by DEC-2849's figures, D falls short of f, so
  the buyer's Sᵢ and Pᵢ, and Σ Pᵢ, are up to f above DEC-2849's. Example: the owner holds 5 of their
  own, and agent A buys 10 with a fee of 1. DEC-2849 gives L_A = 9, S_A = P_A = 9; the interim
  reading gives 10. A's discretionary sell authority and protection reach 1 of the owner's shares.

No single reading without the buyer is conservative in every direction: lowering every holder's
lots by the fee shrinks rule-13 exits, which rule 13 forbids, and leaving them shrinks no one's
authority over the owner's shares.

### Decision

1. **The fee comes off the buying agent's lots** (DEC-2849's own reading), by one of:
   - the executor computes the fee at the buy's fill from the effective fee configuration and the
     fill's liquidity, as `mandate-accounting` does, and records it against the order; or
   - the fee's record names the buy.
2. **Until then, the code's interim reading stands only while it is unreachable.**
3. **The most conservative option, which agents take now:** the journal keeps refusing
   `FeesCharged` with `family: crypto_asset`. The change that admits it must carry item 1 and its
   test (backlog row E6-15u), so the loosening above never reaches a running account.

### Consequences

- No account changes today: no crypto asset fee reaches the fold.
- The holders fuzz asserts the interim reading as the code has it, and a test pins the example
  above, so item 1's change shows up as a deliberate edit of both.

### Alternatives considered

- **Spread the fee over the holders pro rata, or take it from the largest holder.** Refused: each
  can charge an agent for a fee another agent's buy paid, and each shrinks that agent's rule-13 exit.
- **Lower every holder's lots by the fee.** Refused: it shrinks every rule-13 exit (rule 13).

<a id="dec-2851"></a>

## DEC-2851: The runtime's dry run reads the exit fills and the opening hold the binding gate reads

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It only tightens: the dry run now refuses what the binding gate already refused, and no exit reads either) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-27 |
| **Story or claim** | Lane DR's rehearsal re-run, 2026-10-27 19:02Z: the runtime asked its owner to buy back the second their exit filled. [DEC-2388](DEC-2388.md) item 8, [DEC-2143](DEC-2143.md), [DEC-2819](DEC-2819.md) item 4, [DEC-2833](DEC-2833.md) |

### Context

Mandate spec §5.3 allows no opening in an instrument group until `reentry_cooldown_s` after the
agent's last exit fill there, at gate check 2, and an owner exit's fill is an exit fill. The
binding gate in `mandate-executord` reads the last exit fills from the fold's counters once the
account has traded (DEC-2833), and holds every opening by the account's opening hold
(`account_traded`, `research_agent_traded`).

The runtime's advisory dry run (`mandate-daemon` `production::gate`) read neither:
- it passed an empty map of exit fills and named the re-entry cooldown deferred (DEC-2388 item 8);
- it never read the opening hold;
- its fold took no `recorded_at`, so its counters never read.

So in lane DR's re-run the runtime decided again in the second the owner's exit filled, allowed
the opening and asked the owner to buy back, which the executor would have refused at binding.

### Decision

1. **One fold, read as the binding gate reads it.** The production fold folds each row with the
   instant the store recorded it, as the executor's does, so its counters read.
2. **The opening hold.** After the working-orders check, the dry run refuses an opening or an
   increase with the account's opening hold, `Journaled::opening_hold`, by its code.
3. **The re-entry cooldown is seen.** The dry run reads `last_exit_fill_at` from
   `Journaled::counted` once the account has traded, and evaluates check 2's cooldown on it. The
   re-entry cooldown leaves `UNSEEN`: an untraded account has no exit fill, and a traded one
   whose counters do not read is `facts_held`. This narrows DEC-2388 item 8's ten unseen parts to
   nine; `reentry_cooldown` stays a value `deferred_to_binding` may carry (journal spec §9.1), so
   records already written still read.
4. **Nothing beyond the spec.** No hold on re-entry is added beyond the cooldown and the opening
   hold, which together hold a re-entry in the instrument after an owner exit. No exit, protective
   order or kill switch reads either (`AGENTS.md` rule 13).

### Alternatives considered

- **Hold re-entry until a new thesis is admitted after an owner exit.** It is stricter than the
  spec. The cooldown and DEC-2819 item 4's hold already refuse the re-entry, so it would add a
  rule without adding protection.

<a id="dec-2852"></a>

## DEC-2852: The runtime process reads its streams only past the head it last read

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It adds no risk: each record is read once and folded exactly as before, and a read that fails holds as before) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-27 |
| **Story or claim** | Lane DR's rehearsal re-run, 2026-10-27: both runtimes at or over their 250m CPU limit. [DEC-2847](DEC-2847.md), the executor's |

### Context

DEC-2847 found the executor re-reading and re-folding whole streams at every ask. The runtime
process (`mandate-daemon` `runtime`) does the same at every pass, about once a second. It calls
`Store::rows`, a read of the whole stream from Postgres verified from its first record, for:
- the control stream, twice;
- the account stream;
- the scheduler's clock stream;
- the agent stream, twice.

So a pass cost grew with uptime, and lane DR's re-run read the runtimes at 297m and 251m of their
250m limit.

### Decision

1. **The store reads past a head.** `Store` gains `rows_from(stream, start)`.
   - The journal's own stores read only what follows, their chain checked from the held hash
     (`PgJournal::rows_from`, DEC-2847).
   - Any other store answers from `rows`: whole, from the first record, or as the records past
     the start when the first of them follows it, else `None`.
2. **Each stream is followed from its head.** The process keeps per stream the seq and hash of
   the last record it read and asks only for what follows:
   - **Control and agent streams:** kept whole, for the ports that read them so (the approvers,
     the control copy, the models, `fresh`).
   - **Account stream:** keeps nothing. A pass folds every record past the core's head or stops
     the process, so what follows the tail's head is what follows the core's.
   - **Clock stream:** keeps only its latest tick. On the pass the production fold first follows
     the account, the clock is read again from its first record, so the fold takes every record in
     order, as a fold of the whole stream does, and a tick earlier than the last still makes it
     unreadable however late it followed. After that, each pass feeds it only what follows. A
     re-read from the first record recomputes the latest tick.
3. **Exactly the old reading.** A read past the head that the store refuses is read again from
   the first record, and what follows the head is read from it. A read that fails gives what it
   gave before, with nothing kept read.
4. **The models port next.** The models port and the research source still read the kept agent
   stream whole in memory at each evaluation. That costs no store read, but it is O(the agent
   stream). Its replay over kept thesis records follows in a change of its own.

5. **Still per pass.** The kept control and agent records are cloned for the ports that read them,
   which costs memory copies and no store read. The models port's change removes the agent
   stream's (`docs/project/backlog/E21-runtime-kept-rows-clone.md`).
6. **History is verified again every minute.** A record already read is not read again on a pass,
   so on its own a tail would find a rewrite of what it read only at its next start (DEC-2847 item
   4). So every `60` s of the pass's injected time, each tail reads its stream from the first
   record once, as the API's cache does (DEC-2145), and the store verifies the whole chain.
   - **The bound.** A check falls due on the first pass at least `60` s after the last one, or on
     any pass whose time is earlier than the last one's, so a clock set back defers nothing. Each
     tail takes it at its next read, so a rewrite before the held head is found within `60` s of
     pass time plus whatever passes end before reaching that stream (an input in doubt, a failed
     retry). A store that refuses a read past the head is read from the first record at once
     (item 3), and that read must hold the head too.
   - **The cost.** One whole read of each of the four streams a minute, where the process read
     them whole at every pass before this decision. At the 100k-record streams the API measured
     (about 0.4 s to fold, about 39 MB read), each such stream costs about 0.4 s a minute, under
     3% of the runtime's 250m CPU limit.
     So the bound does not scale with stream size yet; a stream that grows past that is the
     archive's to bound, not the check's.
   - **A chain that does not verify.** The store refuses the read. The tail reads nothing,
     neither the records past its head nor the head itself, and asks again from the first record at
     every pass until the read verifies. That is what any read that fails gives (item 3), and
     nothing past the head is read on a chain not verified again.
   - **A chain hashed again, or cut short.** The read verifies, but the record at the held head no
     longer reads as it was, or is gone. The history the process folded is no longer the
     stream's, so no input is handed to the core for the rest of the pass, nothing on the account
     or the models included, and the process fails (`AGENTS.md` rule 3). The next process reads
     the streams whole at its start, and either verifies them or does not start. A kill switch
     written meanwhile still meets the executor's gate, which reads the control stream itself.

### Alternatives considered

- **Keep every stream whole in memory.** The clock stream gains a record a second, so the
  process's 256 MiB would fill in days.

<a id="dec-2853"></a>

## DEC-2853: A kill switch's flatten prices from the latest mark the executor holds, and a flatten no mark prices is alerted

| | |
|---|---|
| **Status** | Accepted (agent, under [DEC-176](../04-decision-log.md#decisions) clause (1), on the coordinator's ruling of 2026-10-27). It tightens how a flatten is priced and makes an unpriced one visible. It loosens nothing: no exit waits longer, and none is refused |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane X), asked for by the coordinator |
| **Story or claim** | DR's cell run 3 on a363ddeef; trading spec §5.5 and §5.6; [DEC-2361](trading-core.md#dec-2361); [DEC-2359](trading-core.md#dec-2359); DEC-160 (12); `AGENTS.md` rule 13 |

### Context

In DR's run 3 the owner's kill switch flattened one SPY with a sell limit at 551.11, the bracket's
stop, while the bid was 580.11. Its `GateDecided` used no quote. The executor process journals
marks (`MarkUpdated`, 580.11, sane), but it hands its core a quote only for an instrument whose exit
already waits on a price (`read_quotes`, DEC-2359). A kill switch raises its flatten in the step
that applies it, so the core held no quote. `lowest_sane` found none, and the flatten took its
fallback, the resting stop.

At a venue that sell is marketable and fills near the bid. But a stop-priced flatten is a far wider
collar than §5.6's first rung, and it went with no alert. The fixture broker then filled it at its
limit, and the loss tripped a drawdown. That fill is the fixture's fault, fixed apart (E21-30).

### Decision

1. **The flatten prices from the held mark.** The executor process keeps each instrument's latest
   sane observation, from the reads it already makes: its marks and its waiting exits' quotes.
   Before it applies a kill switch, the core holds every observation the process holds
   (`ExecutorState::hold_quote`). The flatten's limit, and its ladder's first rung, then price from
   the market as any exit's do.
2. **Nothing comes before the switch.** Holding a quote reads nothing and runs no step. It records
   the quote as a market input does, but runs no watchdog, drafts nothing and sends nothing (review
   round 1: a market input would run the stop watchdog, whose broker calls could fail and hold the
   switch back). A feed that is slow or down, or a broker that errs, cannot delay or fail the
   switch, its stop, its cancels or its flatten (rule 13).
3. **With no mark held, the flatten goes at the stop, and is alerted.** A close no sane mark prices
   is raised at once at the resting stop, as before. It is no longer silent: it is journaled
   `exit_unpriced`, caused by its switch, and the owner is alerted, once per switch and instrument
   (DEC-2361's record and alert). A close the owner's floor alone prices goes at the price the
   owner chose, and is not alerted.
4. **Accepted residual: a held mark has no age cap.** A mark held through a long feed outage still
   prices the flatten, as the core's own sane bids already do. It is the newest the process read,
   and a marketable flatten priced from it is still nearer the market than the entry's stop.

### Rationale

The mark is already read, on the process's own cadence, so pricing from it costs nothing and adds
no wait. A stale mark still prices a flatten nearer the market than a stop set at the entry. An
alert on a stop-priced flatten tells the owner the one case where the collar is the stop.

### Alternatives considered

- **Read a quote in the kill's path.** Rejected. Rule 13 says the kill switch never depends on a
  feed, and a slow read would delay the stop.
- **Wait for the next pass's quote before raising the flatten.** Rejected. It delays the sell, and
  a feed that is down would hold it forever.
- **Hand the held marks as market inputs before the switch.** Rejected (review round 1). A market
  input runs the stop watchdog, so a watchdog exit and its broker calls would come before the
  switch, and a broker error would hold the switch back.
- **Hand every mark to the core as it is written.** Deferred. It would also feed the stop watchdog
  on every mark, which changes behaviour beyond this decision.

<a id="dec-2854"></a>

## DEC-2854: The executor reads its streams only past the head it last read, verified again every minute

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It only tightens: each record is folded once and exactly as a fold of the whole stream folds it, and a read that fails holds as before. A check that fails holds every opening, or, on proof that the history was rewritten, stops the process, whose restart reads every stream from its first record; the coordinator asked for that stop on 2026-10-27, and item 5 says how a kill switch reaches the account meanwhile) |
| **Date** | 2026-10-27 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-27 |
| **Story or claim** | The executor's per-pass reads of whole streams, found after [DEC-2847](DEC-2847.md); the coordinator's ask for a bounded periodic re-verify, 2026-10-27, as the API's cache keeps ([DEC-2145](DEC-2145.md)) and the runtime keeps ([DEC-2852](trading-core.md#dec-2852) item 6). Supersedes DEC-2847 items 3 and 4 |

### Context

DEC-2847 made the binding gate's ports read only past the head they held. The executor's own pass
(`mandate-executord` `process`) still read whole streams, each read verified from its first record:
- the scheduler's clock stream, for the latest tick;
- the control stream;
- each followed agent's stream, with every record parsed again (`folded`) for its kill switches,
  proposals and theses;
- the account and control streams again, for the establishment's gate (journal spec rule 68).

So a pass cost grew with uptime. The clock stream alone gains a record a second.

A tail that reads only past its head does not see again what it read. On its own, it finds a
rewrite of that history only at its next start (DEC-2847 item 4).

### Decision

1. **One tail.** The crate's tail (`tail::Tail`) reads a stream past its held head and folds each
   new record once. The binding gate's ports use it too.
   - A read past the head that the store refuses is read from the first record. That read must
     still hold the head, and what follows it is folded.
   - Before this decision, DEC-2847's tails folded that read again onto fresh state.
2. **What each stream keeps.**
   - **Clock:** the latest tick. A later record replaces it on a tie, as the whole stream's
     maximum does.
   - **Control stream:** kept whole. It is small.
   - **Each followed agent's stream:** its records of this environment, each parsed once as first
     read. Once one does not read, the stream reads as unreadable, as the whole stream would. A
     stream no longer followed is dropped.
   - **Account stream:** only the `ConnectionEstablished` and `ConnectionChecked` records the
     establishment reads. `establish::pass` now asks the process (`ExecutorProcess::establishment`)
     rather than reading the store itself. The wait before the start still reads whole; it ends
     once the stream is established.
3. **A check every minute.** A check falls due on the first pass at least `REVERIFY_S` (60 s) after
   the last one, by the pass's injected time, or on any pass whose time is earlier than the last
   one's. Each tail's next read then reads from the first record, which the store verifies whole.
   - **The bound:** a rewrite before a held head is found within 60 s of pass time, plus any passes
     that end before reaching that stream. A rewrite under a stream that gains a record is found at
     once: the read past the head no longer follows it (`PgJournal::rows_from`).
   - **The cost:** one whole read of each stream a minute, where the pass read them whole at every
     pass before this decision. At the 100k-record streams the API measured (about 0.4 s to fold,
     about 39 MB read), that is about 0.4 s a minute per stream. So the bound does not scale with
     stream size yet.
4. **The safe state of a check that fails.**
   - **The stream will not read from its first record** (a chain the store will not verify, or an
     outage, which the process cannot tell apart):
     - nothing past its head is read, and the stream's `stream_unreadable` alert is raised once,
       the account stream's included, whose read is the establishment's (raised with the next
       pass's alerts);
     - every opening on the account is held until the stream reads. Owner acknowledgments are
       still handed, since they only lift holds and place nothing;
     - exits, protective orders and kill switches read from the other streams still go (`AGENTS.md`
       rule 13);
     - when that stream is the control stream, the pass reads nothing, as in any outage of it.
   - **The stream verifies, but no longer holds the head the process read it to** (a rewrite that
     hashed the chain again, or a stream cut short):
     - the history the process folded is no longer the stream's, and the process proves it;
     - nothing more is journaled, so nothing more is sent: the account stream takes no append
       once a tail no longer holds its head, the pass's own tick copy and marks included;
     - the `journal_rewritten` alert is raised once (and the establishment's code of that name),
       and the process stops (`CycleFailed`), never `Completed`, even on a pass that would have
       completed;
     - the stop cancels nothing, so orders resting at the broker, protective orders included, stay
       there.
5. **How an owner's kill switch still reaches the account.**
   - **Normally:** it is applied on the next pass wherever it lands: before a restart, between a
     read and its fold, on a check's pass, or past the head after many passes.
   - **While a stream will not verify:** a kill switch is read from the stream it is written to.
     The runtime's copy is read from the agent's stream. A command read directly from the control
     stream, for an agent whose connection a compromise revoked (DEC-2343), is read in the same
     agent's turn of the pass, after its stream reads. So while the agent's stream, or the
     control stream, will not verify, its kill switch is not read, as in any outage of that
     stream.
     - Every opening on the account is held, and the stream's alert is raised.
     - The kill switch is applied on the first pass after the stream reads again.
     - A restart would not read it sooner, since the new process's first read of that stream is
       the same read from the first record.
     - Meanwhile protection rests at the broker, and the owner can act there directly.
   - **After a rewrite:** the deployment manager restarts the stopped executor. The new process
     reads every stream from its first record at its start and applies the kill switch. If the
     chain does not verify, the process does not start, and then nothing trades: no opening can be
     taken, and protection rests at the broker. The owner can always act at the broker directly,
     since the account is theirs.
6. **The binding gate's ports get no check of their own.** They read the same control, clock and
   followed agent streams the process's tails read. So a rewrite the process proves stops it within
   the bound, the ports' reads with it.
   - A read past a port's head that is refused must still hold the head. If it does not, the port
     reads nothing from then on, asks its store nothing more, and every opening it is read for is
     held.
   - A port's head can be ahead of the process's. A rewrite between the two heads is then proved by
     the port alone, which holds the openings, and is folded by the process, which never read the
     original record. The process stays up, and the openings stay held until it next starts, when
     every read is from the first record.

### Alternatives considered

- **Stay up on the last verified view after a rewrite.** Refused. The process would trade on
  history the journal no longer holds.
- **Stop on a refused check too.** Refused. An outage of the journal reads the same, and a stop at
  every outage would restart the executor into the same outage. Holding openings while it lasts
  adds no risk and stops no exit.
- **Scale the bound with the stream's size.** Not yet. At the sizes measured, the check costs well
  under the pass's own budget.

### Consequences

- A pass reads what its streams gained since the last pass read them, and parses each agent record
  once.
- An executor whose journal history was rewritten stops within a minute, with an alert, and sends
  nothing more.
- The startup reads, the risk book's opening of a missing fold (`RiskBook::follow`), and the
  permission check's account reference (`account_pii_ref`) still read whole streams. Each is rare,
  not per pass.

<a id="dec-2856"></a>

## DEC-2856: The runtime keeps of the agent stream only its thesis records and journaled outputs

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79. It adds no risk: each port gets exactly what it read of the whole stream before, and every record is folded once, in order) |
| **Date** | 2026-10-28 (UTC) |
| **Decided by** | an agent (Claude Code, lane D1), asked for by the coordinator on 2026-10-28 |
| **Story or claim** | [DEC-2852](trading-core.md#dec-2852) item 4, `docs/project/backlog/E21-runtime-kept-rows-clone.md` |

### Context

DEC-2852 made the runtime read each stream only past its head, and kept the agent stream whole,
because three readers read it whole at every pass:
- the models port (`Models::evaluate`);
- `models::fresh`, which keeps the evaluations the stream
  holds no `ModelOutputRecorded` for;
- the production fold's working universe.

So the kept stream, and its per-pass clone, grew with every record the runtime appends: the
observations and outputs of each tick, and every hand-off.

### Decision

1. **What the models port reads is the thesis records.**
   - `Streams::agent` is the agent stream's `ThesisProposed`, `ThesisRevised` and
     `ThesisInvalidated` records, in stream order (`models::THESES`).
   - Those are the only records of it the port reads: a research deployment replays them for its
     working universe (DEC-2827), and a pinned one reads none.
   - The process keeps those records and no other.
2. **The journaled outputs are folded as read.** `models::Outputs` folds each `ModelOutputRecorded`
   key a record at a time, as the agent stream is read past its head.
   - `Outputs::fresh` gives what `fresh` gives over the whole stream.
   - A record that does not read makes it unreadable from then on, as it makes every whole read.
   - `fresh` over a whole stream stays, built on the same fold, so its tests stay the reference.
3. **The production fold is fed what follows the head.** It takes each record once by its
   per-stream head (DEC-2378 S2), so feeding it the records read past the head feeds it what
   feeding the whole kept stream did.
   - The fold takes nothing until it follows the account, and the runtime may read the agent stream
     before then.
   - So on the read the fold first follows, the agent stream is read again from its first record,
     its kept records and outputs folded again from nothing, as DEC-2852 item 2 reads the clock
     stream.
   - As before, it is fed only for an unpinned mandate.
4. **Still open.** The kept control stream is cloned for its ports at each pass. It is the
   workspace's small control stream, not one the runtime appends to. The backlog row keeps it.

### Consequences

- The agent stream the runtime keeps grows with its thesis records and the outputs it journaled,
  never with its other records.
- A pass's reads and folds of the agent stream are bounded by what was appended since the last.
