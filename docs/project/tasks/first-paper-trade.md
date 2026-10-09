# Task: the first real paper trade (DEC-502, DEC-509): one SPY order through the production cycle

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this brief is a **path brief** over the stories below, and each slice it lists is one
story's tests PR or implementation PR under DEC-77. Docs only: it writes no code.

The first trade is on **SPY** ([DEC-509](../decisions/DEC-509.md)), which supersedes DEC-450's
BTC/USD: on Alpaca's crypto feed BTC/USD's 30-day median daily dollar volume is about 127,900 USD,
below the gate's 1,000,000 USD floor. BTC/USD follows as a mandate input once the slices in
"BTC/USD follow-up" land.

## Story

- **Stories on the path:** E7-19 (the remainders of slices 2, 3 and 4, including Postgres's
  artifact-aware append and the account fields, and slice 5), and new rows this brief adds:
  E15-13 (the quant model host), E10-16 (the Postgres control-stream writer, `journal export`,
  and the paper confirmation commands), E19-11 (the deployment input from the control stream). **For the BTC/USD follow-up:** E7-21 (crypto
  minute bars), E7-22 (crypto pairs in the production assembly), and E7-4's crypto protection
  ([backlog](../06-backlog-v1.md)).
- **Acceptance criteria (verbatim, [DEC-502](../decisions/DEC-502.md) item 1):** "The first real
  paper trade is one order on the founder's Alpaca paper account that reaches the broker only
  through the production cycle API ([DEC-475](../decisions/DEC-475.md)), where: the deployment
  input is built from a **confirmed mandate version**, not a test fixture; the model output comes
  from a **pinned signal model** (mandate spec §8.1, V-007), run outside the production cycle and
  the shell, and handed to the cycle as its typed `ModelOutput`; the Alpaca paper adapter calls
  that API, and E7-7's AAPL assembly is deleted (E7-19 slice 5); the journal verifies afterwards,
  and a restart sends no second order." DEC-509 item 1 makes the instrument SPY.
- **PRD / HLD / spec anchors:** PRD FR-3.1, FR-3.5, FR-3.7, FR-4.3; HLD §5 (agent runtime),
  §6.B (decision cycle), §9 (intelligence layer); mandate spec §2.1, §4, §5.4, §6.1, §8.1 to §8.3,
  §9.1, §10; trading-domain spec §2.1, §2.2, §3.1, §3.2, §4.2 to §4.4, §5.1 to §5.4, §6.2, §7.2,
  §8.4, §9, §10, §11; journal spec §4, §5.1, §8, §9.1, §9.2, §11; agent harness spec §5.1;
  inference spec §1.3.
- **Decisions that apply:** DEC-52, DEC-77, DEC-79, DEC-129, DEC-155, DEC-157, DEC-176, DEC-261,
  DEC-466, DEC-470, DEC-471, DEC-475, DEC-484, DEC-502, DEC-509, and this brief's
  [DEC-503](../decisions/DEC-503.md), [DEC-504](../decisions/DEC-504.md) and
  [DEC-505](../decisions/DEC-505.md).

## Invariants first

Every slice keeps all of these. Each is a test (named in the slice that adds it), and each
oracle is shown to fail on a seeded bug before it is trusted.

| # | Invariant |
|---|---|
| FT-1 | **One door.** An order reaches the broker only through `ProductionCycle::run`, called by the paper adapter with `--place-one-order` after the founder's confirmation. No other shipping path holds a trading transport |
| FT-2 | **No product choice in shipping code** (DEC-475 item 3). No instrument, model id or version, deployment id, gate value, account-rule value or executor value is a constant or compile-time default in shipping code. The same binary runs two deployments on different equities that differ only in journaled inputs |
| FT-3 | **Confirmed version only** (DEC-505). A cycle starts only when the latest `AgentDeployed` for the agent names version V, the stored document re-hashes to V, `MandateConfirmed` for V confirms every envelope path with `cli_confirm` in paper, and every V-rule passes with the model registry present |
| FT-4 | **Pinned model only** (DEC-503 item 3). An output exists only if the mandate's pin, the one matching `model_registry` entry, and the host's compiled content hash agree. Any disagreement is no output, and no output is no buy |
| FT-5 | **Opinion, never order.** The model host emits only `ModelOutput`; it cannot reach sizing, the gate, the journal, the executor or the connector, by crate layering |
| FT-6 | **Journal before acting.** `ObservationRecorded` (the host's inputs) and `ModelOutputRecorded` (v2) commit before `DecisionMade` (v2), before `IntentProposed`, before `OrderSubmitted`, before the `POST`. Every effective input is a `config_refs` hash (DEC-484) |
| FT-7 | **At most one submission, and none on restart.** A run submits at most one opening. A restart, or a second run, submits nothing (E7-7's TI-1 and TI-12; DEC-470 item 1 refuses a second run against a non-flat account) |
| FT-8 | **Fail closed.** A fact that is missing, stale, ambiguous, or another instrument's is a refusal before any `POST` (`AGENTS.md` rule 3) |
| FT-9 | **No rule relaxed** (DEC-509 item 4). Regular-session openings outside the closing window, the eligibility floor with a dated ETP classification, the 1% collar, both participation caps, 1× buying power, and the day-trading regime all apply |
| FT-10 | **Paper only.** Only the paper trading host and the data host are compiled (ES-23); a `live` mandate is refused by the CLI and by V-001 |
| FT-11 | **No unprotected or working opening is left behind.** When a run exits, the opening is terminal, and if the mandate enables protection the filled quantity is covered by the bracket's legs or, after a partial fill, one GTC OCO (§5.4), or the unprotected interval is journaled and its alert key is in the run's report (`Report.alerts`, keys only, rule 6; no notification channel exists yet, so the founder reads it at the terminal). An entry still working at the bound is cancelled |
| FT-12 | **Replay never runs the host or calls the broker.** Replay of the run's journal folds to the same state with the host and the transport absent (journal spec §8) |

## Scope

- **Reference cases that must move from pending to passing:** none named by this path. Every
  passing mandate, trading-domain and journal case stays passing.
- **Invariants touched:** FT-1 to FT-12 above; mandate MI-1, MI-10, MI-11; inference INF-1,
  INF-2, INF-4, INF-5, INF-10 (replay half), INF-13, INF-15, INF-16 as host properties (DEC-503
  item 4); E7-7's TI-1 to TI-12.
- **Crates in scope:** `mandate-shell`; `mandate-runtime` (the `ObservationRecorded`
  correction, R0); `mandate-journal-pg` (an artifact-aware append, J0); `mandate-cli` (the
  Postgres control-stream adapter, `journal export`, and the registration and deployment
  commands: P0, V0, D1, D2); `mandate-alpaca` and `mandate-executor` (two account fields, A1);
  `mandate-backtest` (one module move, M0); and two new crates, `mandate-modelhost` and
  `mandate-paper`.
- **Crates out of scope:** `mandate-builder` (used, not changed); `mandate-risk` and
  `mandate-liquidity` (used, not changed on the SPY path; the BTC/USD follow-up changes them, and
  `mandate-alpaca` and `mandate-executor` further); `mandate-spec` and `mandate-journal` (used,
  not changed);
  `mandate-artifacts-fs` (used as the store), `web/`, the model gateway and inference registry
  (E15-6, E15-7), research, OAuth, the vault, approval channels.
- **New dependencies allowed:** none external. Both new crates use only workspace crates and
  dependencies already listed in `xtask/layers.toml`. `mandate-cli` gains two workspace crates
  from lower layers, `mandate-journal-pg` and `mandate-artifacts-fs` (P0), which needs no
  `docs/dependencies.md` row.
- **Safety-critical:** yes, every code slice. Each is a DEC-77 tests PR or implementation PR,
  except M0, a move that adds no behavior and no stub and is pinned by existing tests, and E3, a
  deletion under DEC-475 item 6.
- **Size budget:** under 400 non-generated changed lines in safety-critical crates per PR; the
  slice table gives each estimate.

## Where each piece lives

| Piece | Crate | Layer | Why there |
|---|---|---|---|
| The model's code: the crossover signal | `mandate-backtest` (moved into its own module, M0) | 7 | One implementation for the backtest and the paper output; DEC-504 hashes exactly that module |
| The model host | **new** `mandate-modelhost` (pure, safety-critical) | 8 | Beside the shell, so neither can depend on the other (DEC-503 item 2) |
| The confirmation and deployment records | `mandate-cli`: a **new** Postgres implementation of its `ControlJournal` trait (P0), whose only implementations today are test doubles (`approvals.rs` under `#[cfg(test)]`, `tests/common`), and new `clap` commands (D1, D2) | 7 | The founder's CLI plays the deployment manager in Phase 1 (agent harness spec §5.1) |
| Version-2 configuration records in Postgres | `mandate-journal-pg`: an artifact-aware append (J0) | 6 | `PgJournal::append` refuses every version-2 `ModelOutputRecorded`, `DecisionMade` and `policy_set` or `model_registry` registration as `missing_artifact` today (X-10) |
| The artifact store | `mandate-artifacts-fs`, one store root shared by the CLI and the paper adapter | 6 | Every `ref` is stored before the append that names it (journal spec §11 check 6) |
| Exporting a stream to verify it | `mandate-cli` `journal export` (V0) | 7 | `mandate journal verify` reads an exported segment file, and nothing exports one from Postgres (X-11) |
| Maintenance excess and prior-close equity | `mandate-alpaca` (parse) and `mandate-executor` (`BrokerAccount`, A1) | 7, 6 | The connector owns the broker read; the executor owns the account type |
| The deployment input from the control stream | `mandate-shell` | 8 | Assembly of authoritative inputs; uses `mandate-spec`'s fold |
| The `ObservationRecorded` draft | `mandate-runtime` | 6 | The agent stream's single writer |
| The account and market snapshots (judge, liquidity facts, gate template, contexts) | `mandate-shell` | 8 | Today's `paper/*`, generalized in place so the E7-7 path stays runnable (DEC-475 item 7) |
| The paper adapter binary | **new** `mandate-paper` (impure, safety-critical) | 9 | The only crate that sees both the host and the shell; holds the credential read and the one clock read |

**Founder review.** Both new crates add an `xtask/layers.toml` entry, a `CODEOWNERS` line, and a
workspace `Cargo.toml` member; `mandate-shell` drops its `mandate-backtest` dependency in E3. Each
of those files is founder-owned (ES-13), so every PR that touches one says so under "Decisions
needed", as existing practice does.

## The model host (DEC-503, DEC-504)

```text
control stream ──► E19-11 deployment input ──┐
stored daily bars ─► trusted closes ─► mandate-modelhost::evaluate(pin, registry, calendar, closes, now)
                                              │   └─► Option<(observation data, ModelOutput)>   (none = no buy)
broker GETs ─► account and market snapshots ─┤
                                              ▼
             mandate-paper ─► ProductionCycle::run ─► runtime ─► builder ─► gate ─► executor ─► POST
```

- **Input.** Typed complete daily closes of the pinned instrument through the last completed
  regular session, read by the shell's existing trusted-dataset check and handed over by the paper
  adapter. The host reads no file, no clock and no global calendar: `now` and the trading
  calendar are arguments. The calendar is `mandate-time`'s US-equity exchange calendar, the one
  the shell's dataset check already uses (`latest_completed_equity_day`), so both name the same
  last completed session, early closes included.
- **Checks, in order.** The pin's id and version have compiled content in the host; its content
  hash equals the host's computed hash and the `model_registry` entry's; the mandate's parameter
  keys equal the content object's schema, every value is set (no default) and in bounds; the
  closes are the pinned instrument's, complete, ending at the calendar's last completed session
  at `now`, and at least `slow_periods` long. A calendar that cannot name that session refuses.
  Any failure is a typed refusal and no output.
- **Output.** `Long` is direction `long`, conviction 1, confidence 1 (DEC-157 item 4); `Flat` and
  `Undecided` are no output. `as_of` is the last close's end, 16:00 New York on the last
  completed session (an early close's end on a short day); `expires_at` is `as_of +
  max_output_age_s` (DEC-503 item 5). So a run on the next session's morning needs a pinned
  `max_output_age_s` of at least the time since that close. A model that says `Flat` places
  nothing: the run reports it and ends. Nobody changes the windows or the mandate to get a `Long`
  (DEC-475, "outcome-chasing").
- **Journal.** After R0, the runtime records the host's inputs as `ObservationRecorded`, then
  the output as `ModelOutputRecorded` v2, before any decision (H3). **Who stores the artifact,
  and when:** the paper adapter puts the closes' canonical bytes into the deployment's
  `mandate-artifacts-fs` store (write, fsync, hard-link) and only then calls the cycle with the
  observation, whose `data_ref` is that digest. The data is never inline (journal spec §9.1). The
  shell's append is the artifact-aware one (J0), so an observation whose artifact is not stored is
  refused at append, before any decision (§11 check 6).

## The confirmed mandate (DEC-505)

None of these commands exists today. The CLI's `ControlJournal` is a trait whose only
implementations are test doubles, `mandate-cli` does not depend on `mandate-journal-pg`, and
`main.rs` wires only `download`, `inspect`, `journal verify`, `journal verify-cold` and
`artifact put` and `get`. P0 adds the Postgres implementation and the `--journal` (DSN) and
`--store` (artifact root) plumbing; D1 and D2 add the commands. Each command stores every object
it names in the artifact store before its append.

The founder runs them against the deployment's Postgres journal, after `mandate workspace open`
(D1c, DEC-527 item 7) has opened the workspace control stream once. Each invocation commits exactly
one control-stream event (DEC-155 item 5), so `config register` runs once per object, and every
command refuses a `live` environment:

1. `mandate config register` stores and registers each configuration object: fee schedule,
   trading calendar, instrument snapshot, rule set (version 1), and `policy_set` and
   `model_registry` (version 2, DEC-484 item 4). The instrument snapshot carries SPY's
   ETP classification (`plain`), its date, and its source (below).
2. `mandate model register` stores the quant content object and registers it, kind
   `model_version` (DEC-504 item 3).
3. `mandate version create` stores the canonical document and commits `MandateVersionCreated`,
   every path `user_entered`, since the founder wrote the file.
4. `mandate version confirm` shows the confirmation summary, takes a `cli_confirm` code, and commits
   `MandateConfirmed` naming every top-level envelope path.
5. `mandate agent deploy` takes a fresh `cli_confirm` and commits `AgentDeployed`.

The paper adapter then takes the opaque deployment ids, the journal, the stored-bars directory
and the two acknowledgement flags. It reads no mandate or configuration file: E19-11 folds the
control stream (DEC-505 item 1) and refuses on every FT-3 check before any credential is read,
except V-002, which needs the preflight's broker equity and runs after the GET-only preflight and
before any decision.

**Which registration counts** (DEC-505 item 1). Several `ConfigSnapshotRegistered` of one kind
can be on the stream. For each kind the effective one is the **latest by control-stream `seq`**
at the run's read of the stream, with two narrowings: an `instrument_snapshot` counts only if its
object names the pinned asset id, and a `model_version` counts only if it registers the pinned
`(model_id, model_version, content_hash)`. A fee schedule whose `effective_from` is after the
run's trade date is refused, never skipped for an older one. The run folds the stream once, so a
registration committed during a run applies from the next run. The D3 tests pin each rule.

**The instrument snapshot's shape.** D1 registers it with `etp_source` beside the members the
E7-7 file had. The E7-7 file reader (`paper/artifacts.rs`, which refuses any member it does not
read, DEC-466 item 2) is not changed; D3's new reader of the registered object takes exactly
D1's members, and E3 deletes the file reader.

**Envelope values are the founder's** (`AGENTS.md` rule 11): the pinned instrument (SPY,
DEC-509), allocation, `max_order_usd`, the two windows, `max_output_age_s`, protection, and its
distances. This brief proposes none of them. Two constraints the code imposes: `max_order_usd`
must cover at least one whole share, and with protection on, `take_profit_distance` must be set,
because a stop-only equity placement is still E7-4's stub (`bracket_of` answers `Unimplemented`
before `OrderSubmitted`, so the run refuses without sending).

## SPY specifics

| Rule | Spec | Today's code | Gap, and the slice that closes it |
|---|---|---|---|
| **Exchange** | Trading §3.1, §3.2 item 2: `ARCA` is eligible | The instrument snapshot's `exchange` maps `arca` to both the broker's and the gate's code | None |
| **Price and liquidity floors** | Trading §3.2 items 4 and 5: prior close ≥ the price floor (5 USD); 20-session median daily dollar volume ≥ the liquidity floor (1,000,000 USD) | `mandate-liquidity` computes both from the stored daily bars, but `paper/facts.rs` `liquidity_facts` refuses any instrument but `SYMBOL`, AAPL | **Gap** (X-8). Q1: the instrument comes from the deployment input |
| **ETP classification** | Trading §3.2 item 6: every listed security's ETP status from a source that flags all ETFs (for example the Nasdaq Trader symbol directory's ETF column) plus an ETN list; unclassified is complex; older than the configured age, openings are denied | The instrument snapshot carries `etp` and `etp_classified_at`; the gate denies every equity opening when the date is absent or older than `etp_classification_max_age_s`, 604,800 s in the reviewed rule set (`floor.rs`, DEC-129 items 10 and 33); nothing records the source | **Gap.** D1: the registered snapshot also names `etp_source`, and refuses without one. SPY is a plain ETF, not leveraged, inverse or volatility-linked. The founder re-registers the snapshot within seven days before each run |
| **Session** | Trading §4.3, §9.4: regular-session openings only; §9.6: no opening in the last 10 minutes (15:50 to 16:00 New York on full days) | `paper/judge.rs` requires the regular session outside the close window, at the preflight and again at submission | None. A run starts between 09:30 and 15:50 New York on a trading day, leaving E1b's bound before 15:50 |
| **Increment** | Trading §5.2: fractional equity orders are day-only and not allowed in a bracket or OCO; §5.4: protective legs whole shares | The instrument snapshot must say `whole`; SPY is fractionable, but the snapshot pins whole shares | None. `max_order_usd` must cover one share |
| **TIF** | Trading §5.1, §5.2 | A bracket entry is GTC (`intent.rs`); a plain opening is DAY | None; E1b handles a GTC entry that rests |
| **Collar** | Trading §9.6: 1% for a 20-day median dollar volume ≥ 50,000,000 USD | `conduct.rs` reads `collar_liquid_x` and `collar_liquid_threshold_usd` | None |
| **Participation caps** | Trading §9.6: order ≤ 5% of trailing 5-minute volume; day ≤ 5% of 20-day ADV | `recent_minute_bars` reads IEX minute bars; IEX understates consolidated volume, so the caps are tighter | None beyond Q1 |
| **Quote** | Trading §4.2, §10: paper uses the `iex` profile; an opening needs a fresh IEX quote | `latest_quote` with `iex_quote_max_age_s` (10 s) | None |
| **Fees** | Trading §6.2: SEC and TAF on sells, CAT on both, reserved per order | `PAPER_ONLY` in `mandate-executor/src/fees.rs` holds deliberately high equity figures, "Proposed (founder) as values" | F-1 |
| **Buying power** | Trading §7.2, §9.3, §9.5: 1× gross exposure | `one_x_buying_power` = min(cash, non-marginable buying power), so the 4× margin of the 1,000,000 USD paper account is never spent | None |
| **Day-trading regime** | Trading §9.2: `intraday_margin` or `legacy_pdt`, by the broker's transition | The gate template hard-codes `IntradayMargin` with zero maintenance excess, `prior_close_equity` 0 and `AccountType::Margin` | **Gap** (X-9). Trading §7.2 states two of these per broker: "Account type: Alpaca: always margin" and "Day-trading regime: Alpaca: `intraday_margin`". Q2 takes them from the connector, which declares them for Alpaca, rather than from the shell. Maintenance excess and prior-close equity need broker fields nothing reads today: `BrokerAccount` (`mandate-executor/src/types.rs`) has neither, and `wire::account` parses no `last_equity` or `maintenance_margin`, though the recorded account fixtures carry both. A1 adds them (§7.2 names `last_equity`), and the executor computes maintenance excess as equity less `maintenance_margin`; `AccountSnapshotRecorded`'s payload is unchanged, since its schema is closed. With 1,000,000 USD equity both regimes allow the first opening, so this changes no outcome today; DEC-475 item 3 still forbids the constants |
| **Settlement** | Trading §2.2, §8.4: equity T+1 | The fold books it | None for a first buy |
| **Protection** | Trading §5.4: a bracket entry's legs, whole shares, GTC; legs held until the entry fills completely; a partial remainder cancelled at `bracket_partial_fill_timeout_s` (60 s) or `max_unprotected_s` (60 s), then one GTC OCO | `bracket` in `mandate-executor/src/protection.rs`, driven by the executor's ticks | **A running executor is needed after a partial fill.** E1b keeps the run up until FT-11 holds |
| **Account state** | DEC-470 item 1 | The judge refuses any position, open order or accrued fee | The founder's paper account must be flat. A second run is refused by the same check (FT-7) |

## Lifecycle walk

| State | Entered | Blocks | Ends | Who | Session close, midnight, restart, version change |
|---|---|---|---|---|---|
| Version created, not confirmed | `MandateVersionCreated` | Every cycle (V-020) | `MandateConfirmed` | Founder | Nothing carries; a new version starts over |
| Confirmed, not deployed | `MandateConfirmed` | Every cycle (no `AgentDeployed`) | `AgentDeployed` | Founder | As above |
| Deployed | `AgentDeployed` | Nothing | `AgentStopped`, or a new version | Founder | A new version re-runs E19-11; the old pin's outputs are ignored `not_pinned` |
| ETP classification aging | Registered snapshot | Every opening once older than 7 days | A new registration | Founder | Restart changes nothing; the gate reads the date |
| Dry run | A run without `--place-one-order` | The `POST` | The report (would-place, or the refusal) | — | Keeps no journal (DEC-157 item 6) |
| No output | Host refusal, `Flat`, `Undecided` | Every buy | A later run whose closes say `Long` | Nobody | A completed session changes the last close; a dataset not ending on it is refused until re-downloaded |
| Outside the session | Before 09:30, from 15:50, or a holiday | Every opening | The next regular session | The calendar | — |
| Intent journaled, not sent | `IntentProposed` committed | A second cycle (TI-12) | `OrderSubmitted` or the intent's expiry (`max_intent_age_s`) | Executor | Restart re-hands nothing past its age |
| Submitted, answer lost (`Unknown`) | `OrderSubmitted` without an answer | Every order in SPY (§5.3 rule 9) | Query by `client_order_id` inside the run (E1b) | Executor | A crash here leaves the journal's order `Unknown`; the next run is refused by DEC-470 item 1, so no second `POST`; mapping existing broker state is E19-1's |
| Working entry (GTC bracket or DAY) | Accepted | Another opening (§5.3 rule 6) | Filled, or cancelled at E1b's bound | Executor, paper adapter | The adapter does not exit with it working (FT-11). A DAY order expires at the close |
| Partly filled bracket entry | First fill | — | Complete fill (legs active), or the remainder cancelled and one GTC OCO placed | Executor | Needs the running executor: E1b |
| Protected | `ProtectionChanged` placed | Adds (one side, one working order) | Exit or kill switch | Owner, risk engine | Legs rest across sessions; GTC equity orders expire after 90 days and are re-placed before expiry (DEC-367) only by a running executor |
| Kill switch | Agent-scoped kill switch (E7-4 slice 7) | Every opening | Owner acknowledgment | Owner | Outside the regular session, equity sells follow §5.5 |

## Adversary review

| Actor | Attempt | Blocked or disclosed by |
|---|---|---|
| Careless founder | Hand-edits the mandate file after confirming | The deployment reads the stored document by hash; an edited file is not V (FT-3) |
| Careless founder | Changes the windows until the model says `Long` | Each change is a new risk-increasing version with step-up, journaled; the brief forbids it as outcome-chasing (DEC-475) |
| Careless founder | Runs twice | The judge refuses a non-flat account (DEC-470 item 1); TI-12 refuses an open cycle |
| Careless founder | Registers an old ETP classification or none | The gate denies every equity opening on an absent or stale date (`floor.rs`) |
| Bad model code | Someone edits the crossover but keeps its version | DEC-504 item 2's pinned-hash test fails the build |
| Hand-fed output | A file or flag supplies a `Long` | No such input exists: the adapter's only output source is the host; the host's only input is trusted closes (FT-4) |
| Malicious insider | Registers a second hash under the same version | DEC-504 item 4; V-007 and DEC-484 item 5 require exactly one matching entry |
| Bad market tick | A wild quote or a thin minute | The 1% collar, a crossed or stale quote refused, participation caps |
| Lost answer | The `POST` times out | `Unknown`, query by id, never a second submission (FT-7) |
| Fill after exit | The entry fills, wholly or partly, after the process ends | E1b waits for a terminal state and cancels at its bound (FT-11) |
| Revoked connection | A `ConnectionRevoked` is on the stream | DEC-505 item 3 keeps the fold's refusal |

## Cross-spec trace

| Reference | Scope | Units | Outcome | Matches? |
|---|---|---|---|---|
| DEC-475 item 1 "a model gateway supplies" ↔ inference §1.3 (quant makes no call) | Quant only | — | Host supplies the same typed output | Read by DEC-503 |
| Mandate §8.1 content hash ↔ journal §9.2 `ConfigSnapshotRegistered.content_hash` ↔ `ModelOutputRecorded.content_hash` (`ref`) | Every model | SHA-256 of canonical JSON | Re-hashes at §11 check 6 | Yes, once the content object is stored (DEC-504 item 3) |
| Mandate §8.2 `as_of` ↔ the shell's `map::model_output` | Every output | Timestamp | Data cut-off | **No**: the shell uses the run clock (X-3); the host follows §8.2 |
| Trading §3.2 item 6 ↔ instrument snapshot `etp_classified_at` ↔ `GateConfig.etp_classification_max_age_s` | US equities | Seconds | Deny on absent or older | Yes; the source is not recorded (D1) |
| Trading §3.2 items 4, 5 ↔ `mandate-liquidity` ↔ `paper/facts.rs` | Pinned equity | USD, 20 sessions | Deny below | **No**: pinned to AAPL (X-8); Q1 |
| Trading §7.2, §9.2 ↔ gate template ↔ `BrokerAccount` | Account | USD | Regime and account type per broker; maintenance excess and `last_equity` from the broker | **No**: constants, and no broker fields (X-9); A1, Q2 |
| Journal §5.1, §11 check 6 (artifacts stored before append) ↔ `PgJournal::append` | Version-2 configuration records | — | Appended once their objects are stored | **No**: Postgres refuses them all as `missing_artifact` (X-10); J0 |
| DEC-502 "the journal verifies afterwards" ↔ `mandate journal verify` | The deployment's Postgres streams | — | Checks 1 to 7 | **No**: verify reads an exported segment file, and nothing exports one from Postgres (X-11); V0 |
| Journal §9.1 `ObservationRecorded` ↔ runtime draft | Agent stream | `instrument_id`, `as_of`, `data_ref` vs `instrument`, `at`, inline `data` | Append | **No** (X-2); R0, ruled by the coordinator |
| DEC-484 `model_registry` ↔ shell agent drafts | `ModelOutputRecorded`, `DecisionMade` | `config_refs` | v2 binds registry and policy | **No**: the shell writes version 1 with `mandate_version` only (X-1); D4c |
| Mandate §4.3, §6.2 step 5c (the policy overlay) ↔ journal §9.1 `decided_by` ↔ the shell's classifier | `DecisionMade` for an opening or increase | `policy_overlay` | The overlay's `ask`, and DEC-534's `deny` while nonconforming | **No**: the label is new in journal spec v0.18 (DEC-536), and neither `mandate-journal` nor the shell has it; J3, D4d |
| Agent harness §5.1 steps 1, 2, 4, 6 ↔ E19-11 | Construction | — | Exit on failure | Yes (DEC-505) |
| Mandate §6.1 `cli_confirm` ↔ identity spec methods per environment | Paper step-up | — | Paper only | Yes |

## Slices, in order

T is a tests PR (stubs and pending tests that fail on the stubs) and I its implementation PR,
under DEC-77. D is a docs PR. Estimates are non-generated changed lines; the range is honest
uncertainty, and a slice that would cross 400 splits before review, not after. Every slice leaves
E7-7's recorded tests and the existing dry run passing until E3 deletes them (DEC-475 item 7).

| # | Story | Kind | Crate | What | After | Estimate |
|---|---|---|---|---|---|---|
| S1 | — | D | docs | This brief, DEC-503 to DEC-505, DEC-509, backlog rows | — | this PR |
| M0 | E15-13 | refactor (one PR) | `mandate-backtest` | Move the crossover signal into `strategy/ma_crossover.rs` with no behavior change and no stub. The existing tests pin the moved behavior: `the_crossover_compares_sums_by_cross_multiplication` and `crossed_windows_are_an_error` (`tests/hand.rs`), `the_signal_matches_the_rational_comparison_oracle` and `the_signal_never_reads_a_later_close` (`tests/properties.rs`). Mutants run on the diff | — | 80–150 (mostly a move) |
| M1 | E15-13 | T | **new** `mandate-modelhost` | Crate, layer 8 (founder files); `evaluate` stub; pending tests: content object and its hash mechanism (DEC-518 item 1), pin vs registry vs host, parameter schema, the calendar's last completed session, output mapping, `as_of` (X-3), property tests for FT-4 and determinism. Two PRs to stay under 400 lines: M1a (#683: crate, content object, output, `as_of`) and M1b (refusals, typed refusal codes, properties) | M0 | 300–400 each |
| M2 | E15-13 | I | `mandate-modelhost` | Implementation (closes X-4 for new deployments) | M1 | 200–300 |
| M2b | E15-13 | T (one PR, live from the start: a test added over unchanged code has no stub to fail on) | `mandate-modelhost` | The literal hash pin (DEC-518 item 1): a test that `quant.ma_crossover` 1.0.0's content hash is the 64 hex characters M2's bytes give, so an edit to a listed file fails the build until a new version. Lands before D1 registers any hash | M2 | 20–40 |
| M1c | E15-13 | T (tests correction, DEC-77) | `mandate-modelhost` | #684's review minors, kept out of M2 under the freeze rule: the signal property generates ties (equal window means), so a `>=` bug is caught by the property and not only by M1a's hand case; rows for a clock before the calendar's range, a close on a non-trading day, closes out of order, and each of `ExpiryOverflow`, `SignalArithmetic`, `OutputUnrepresentable` and `ContentObject` where an input can reach it; FT-4 over all 63 identity masks rather than a sample; the order among the non-identity checks (parameters, instrument, closes, session, count), which no test pins, by two-change rows or a property over every pair of changes (#686's review); a determinism check that is not vacuous for a pure function; a doc comment on `Change::PinHash` (E7-7's old fixture hash); and DEC-518 item 6 to read "the New York exchange calendar handed in" | M2 | 60–150 |
| R0 | X-2 (DEC-503 item 7) | T, I | `mandate-runtime` | `ObservationRecorded` drafted as journal spec §9.1 and the registered schema say: `instrument_id`, `as_of`, `data_ref` | — | T 120–200, I 120–200 |
| J0 | E7-19 slice 4 remainder | T, I | `mandate-journal-pg` | An artifact-aware append that checks a version-2 draft's configuration objects in a given `ArtifactSource`, as `MemoryJournal` does, with the same outcomes; Postgres integration tests under `MANDATE_PG_URL`, which #644's reviewer said become required here (X-10) | — | T 200–300, I 150–250 |
| P0 | E10-16 | T, I | `mandate-cli` | A `mandate-journal-pg` implementation of `ControlJournal`, appending through J0 with the `mandate-artifacts-fs` store; the `--journal` and `--store` options; the `clap` skeleton the D1, D2 and V0 commands hang on | J0 | T 200–300, I 200–300 |
| V0 | E10-16 | T, I | `mandate-cli` | `mandate journal export`: one Postgres stream to the segment file `journal verify` reads (journal spec §6.2), so the run's journal can be verified with its store (X-11) | P0 | T 120–200, I 100–180 |
| D1 | E10-16 | T, I | `mandate-cli` | `config::register` (the instrument snapshot with `etp_source`, exactly DEC-523's object) and `config::register_model` (content and hash from `mandate_modelhost::content`): stored objects and `ConfigSnapshotRegistered` v1 and v2, paper only, as library functions (DEC-526) | P0, M2 (content object), M2b (the pinned hash) | T 300–400, I 250–350 |
| D1b | E10-16 | T, I | `mandate-cli` | The `clap` commands `config register` and `model register` over P0's `--journal` and `--store`, the source of the `Owner` they run as, and a binary test against Postgres (DEC-526 item 1) | D1 | T 150–250, I 100–200 |
| D1c | E10-16 | T, I | `mandate-cli` | `mandate workspace open`: commits only `StreamOpened` on `ctl:{workspace}` as the vectors' `control_services` opener, paper only, refused if the stream holds any event (DEC-527 item 7) | D1b | T 100–200, I 80–150 |
| D2a | E10-16 | T, I | `mandate-cli` | `version::create` and `version::confirm` (`cli_confirm`) with their records, as library functions; split three ways because the formatted tests for these two alone are about 350 lines ([DEC-530](../decisions/DEC-530.md)) | D1 | T 350–400, I 250–350 |
| D2b | E10-16 | T, I | `mandate-cli` | `agent deploy` as a library function, with its record (DEC-530 item 9) | D2a | T 250–350, I 150–250 |
| D2c | E10-16 | T, I | `mandate-cli` | The `clap` commands `version create`, `version confirm` and `agent deploy` over DEC-527's owner flags, the read-only display of a gesture's code and warnings, and a binary test against Postgres; no message names the DSN | D1b, D2b | T 200–300, I 150–250 |
| D3 | E19-11 | T, I | `mandate-shell` | The deployment input from the control stream (DEC-505), replacing `--mandate` and `--config-dir`; the selection rule for registrations; the reader of D1's instrument snapshot (DEC-523); V-007 checked (X-7). Three tests PRs to stay under 400 lines: D3a (the confirmed version: deployment, stop, document, V-rules with the registry, DEC-505 item 3's two facts, V-002 in a second phase), D3b (the registrations: latest by `seq` with the two narrowings, every object re-hashed, the fee date) and D3c (below) | — (builds its own control-stream fixtures) | T 300–400 each, I 300–400 |
| D3c | E19-11 | T | `mandate-shell` | The DEC-523 reader of the effective instrument snapshot: every member, type and value set, `arca` and `nasdaq`, and no silent fallback, so a later SPY snapshot that fails refuses rather than leaving an earlier one in force (rule 3); the fee date to the day. One implementation PR then covers D3b and D3c | D3b | T 150–250 |
| D4a | E7-19 slice 4 remainder | T, I | `mandate-spec` | A strict reader of `schemas/policy.schema.json` documents and of the `policy_set` object that holds them (DEC-484 item 4), so a registered policy set is validated before its levels fold into §4.3's overlay | — | T 300–400, I 150–250 |
| D4b | E7-19 slice 4 remainder | T, I | `mandate-shell` | The effective `policy_set` and `model_registry` registrations: the policy set read through D4a, the registry's shape and its one entry equal to the pinned model's registration (DEC-484 item 5), and `policy::check` against the confirmed mandate, which can never be loosened (stricter-of); a nonconforming mandate denies only actions that add risk (DEC-534) | D3, D4a | T 200–350, I 150–250 |
| D4c | E7-19 slice 4 remainder | T, I | `mandate-shell` | `ModelOutputRecorded` and `DecisionMade` v2 with `policy_set` and `model_registry` refs (X-1), appended through J0's `append_with_config_artifacts`; the run reads D4b's governance, the overlay and its violations, into the classifier, a registry or policy set missing from the store stops the run before any order, and every exit passes while the mandate is nonconforming (DEC-534). D4d owns the deny of an opening or increase. The overlay's narrowing of `auto` to `ask` stays in mandate spec §6.2 step 5c as defence in depth: it is unreachable, since a policy forbidding `auto` over a mandate that holds an `auto` or a delegation is itself a violation, which DEC-534 denies first | D4b, J0 | T 150–250, I 150–250 |
| J3 | E7-19 slice 4 remainder | D (spec, ES-22), T, I | `docs/specs`, then `mandate-journal` | Journal spec v0.18: `policy_overlay` joins §9.1's closed `decided_by` labels, for mandate spec §6.2 step 5c's overlay `ask` and DEC-534's `deny` ([DEC-536](../decisions/DEC-536.md)); the vectors' additive `policy_overlay` section; then `mandate-journal` accepts the label | — | D 300–450 (half generated), T 100–200, I 30–80 |
| D4d | E7-19 slice 4 remainder | T, I | `mandate-shell` | DEC-534's deny of an opening and of an increase while the mandate is nonconforming, recorded as `autonomy: deny` with `decided_by: policy_overlay` (mandate spec §6.2 step 5c, DEC-536); no intent, no `ApprovalRequested` and no order follow. The overlay's `ask` branch is unreachable and has no test (see D4c) | D4c, J3 | T 150–250, I 100–200 |
| D4e | E7-19 slice 4 remainder | T, I | `mandate-shell`, `mandate-runtime` | DEC-534 item 4's owner alert: `OwnerAlertSent` of kind `account_state` (notifications spec §3.2), opaque ids and generic text only (`AGENTS.md` rule 6), written by its subject stream's owner as notifications spec §5.5 says. Off the first trade's path, since the paper mandate conforms and the deny never fires | E8-9 (closes `OwnerAlertSent`'s payload and its streams), D4d | T 150–250, I 100–200 |
| Q1 | E7-19 slice 2 remainder | T, I | `mandate-shell` | Liquidity facts and every preflight check take the instrument from the deployment input, not `SYMBOL` (X-8): `Artifacts::from_registered` builds the run's artifacts from the phase-1 confirmed version and the effective registrations, judging each registered object as its file was, and `liquidity_facts` takes the symbol those artifacts bind | D3 | T 150–250, I 100–150 |
| A1 | E7-19 slice 3 remainder | T, I | `mandate-alpaca`, `mandate-executor` | `BrokerAccount` gains `last_equity` and `maintenance_margin`, parsed by `wire::account` from the existing recorded account fixtures; the executor computes maintenance excess; `AccountSnapshotRecorded`'s payload unchanged | — | T 150–250, I 100–200 |
| Q2 | E7-19 slice 3 remainder | T, I | `mandate-shell`; `mandate-alpaca` (the declaration, DEC-840) | Account type and regime from the connector's declared Alpaca facts (§7.2): `alpaca_account_rules`, carried by the preflight in `BrokerFacts`; maintenance excess and prior-close equity from A1; a reported deficit refuses the opening (DEC-840 item 4); no shell constants (X-9) | Q1, A1 | T 100–200, I 80–150 |
| H3 | E15-13 | T, I | `mandate-shell` | The cycle takes the host's observation, whose artifact the paper adapter stored first, with its output; the runtime journals `ObservationRecorded` before `ModelOutputRecorded` | R0, M2, D4c | T 200–300, I 150–250 |
| E1a | E7-19 slice 5 | T, I | **new** `mandate-paper` | The paper binary: E19-11 input, preflight, artifact store, host, `ProductionCycle::run`; `mandate-tracer` still builds | M2, D4c, Q1 | T 300–400, I 250–350 |
| E1b | E7-19 slice 5 | T, I | `mandate-paper` | After submission, bounded GETs and executor ticks until the entry is terminal and, after a partial fill, its OCO placed; an entry still working at a configured bound is cancelled (FT-11) | E1a | T 250–350, I 200–300 |
| E3 | E7-19 slice 5 | Deletion PR under DEC-475 item 6 (not a DEC-77 pair) | `mandate-shell`, `xtask` | Delete `mandate-tracer`, `paper.rs`'s constants, `Artifacts::load` and the file reader, `legacy_identity`, `tracer::run`'s signal path, `MovingAverage`, `MOVING_AVERAGE`, the bars and signal stages, `map::model_output` (X-3), the E7-7 model artifact (X-4), `tests/tracer.rs` with its row in `xtask`'s `BEHAVIOUR_ONLY_TESTS` (F-3), and the `mandate-backtest` dependency | E1b, Q2, H3 | 400–900 deleted; two PRs if over 400 |
| E2 | — | run, then D | — | The manual paper run (below); a docs PR records its evidence | E3, D1c, D2c, V0 | — |

**Totals.** 42 PRs (43 if E3 splits) and about 6,630 to 10,400 changed lines.

**Critical path.** Two chains meet at E2:

- the shell chain, D3 → D4a → D4b → D4c → Q1 → E1a → E1b → E3: 11 to 12 merges, with Q2 and H3 fitted into
  the shell between E1a and E3, since `mandate-shell` slices cannot overlap;
- the CLI chain, J0 → P0 → D1 → D1b → D1c → D2a → D2b → D2c (and V0): 16 to 18 merges. D4c also waits on J0, so J0 goes
  first.

M0 to M2, R0 and A1 run beside them, by crate. At the recent pace of two to three review rounds
per safety-critical PR, this is **about two to three weeks with two builders**, one on each chain,
and about four weeks with one. Taking the crypto slices off the path saved about a week of serial
shell work (C3 to C5) and the E7-4 crypto slice. The CLI chain is new in this revision (Major 1 of
review round 1): it was assumed to exist and does not.

**Relation to M7's "clap wiring" item.** The tracker's M7 remainder wires the inbox and owner
commands with `clap`, and those need the same Postgres `ControlJournal` P0 builds. P0 delivers the
adapter and the skeleton once; whichever of P0 and M7's wiring lands second reuses it and adds
only its own commands.

### The manual paper run (E2)

Only the founder runs it, with paper credentials and data keys the agents never see
(`AGENTS.md` rule 8):

1. `mandate download` SPY daily bars covering at least max(20, `slow_periods`) sessions through
   the last completed regular session, then `mandate inspect`: zero problems, zero true gaps,
   and no split inside the span.
2. The registration and confirmation commands above, with SPY's ETP classification dated within
   the last seven days. The paper account holds no position, open order or accrued fee.
3. Between 09:30 and 15:50 New York on a trading day, `mandate-paper` without
   `--place-one-order`: it reports the order it would place, or the exact refusal. A `Flat` model
   or a gate denial ends here; neither is worked around.
4. With the founder's explicit confirmation immediately before it (DEC-502 item 6),
   `--place-one-order`: one `POST`; the run stays until FT-11 holds.
5. `mandate journal export` for the agent, account and control streams, then
   `mandate journal verify <file> --store <root>` on each: clean.
6. The same command again: it is refused before any `POST` because the account holds the
   position (DEC-470 item 1), and submits nothing.

## BTC/USD follow-up (off the critical path)

BTC/USD comes back as a mandate input once these land (DEC-509 item 3). The slices are unchanged
in substance from this brief's first draft:

| # | Story | Kind | Crate | What | Estimate |
|---|---|---|---|---|---|
| C1 | E7-21 | T, I | `mandate-alpaca` | Crypto recent minute bars: `/v1beta3/crypto/us/bars`, the answer keyed by pair, a UTC window | T 200–300, I 100–200 |
| C2 | E7-22 | T, I | `mandate-liquidity` | 30-day median daily dollar volume (§3.2 item 7) | T 100–200, I 60–120 |
| C3 | E7-22 | T, I | `mandate-shell` | Crypto daily-bar trust over UTC days (§2.2); liquidity facts by asset class | T 250–350, I 200–300 |
| C4 | E7-22 | T, I | `mandate-shell` | Judge and gate template by asset class: continuous session, crypto feed and `crypto_quote_max_age_s`, `crypto_status`, increments from the asset record, quote currency, fee reservation, the 200,000 USD notional refusal (§5.2) | T 300–400, I 250–400 |
| C5 | E7-22 | T, I | `mandate-shell` | Run and executor contexts by asset class: `MarketSession::Crypto`, GTC, `fee_rate_asset` (mandate §8.3's β), `crypto_stop_limit_offset` | T 250–350, I 200–300 |
| C6 | E7-22 | T, I | `mandate-executor`, `mandate-risk` | Crypto limits on the asset record's `price_increment` (X-6) | T 150–250, I 100–200 |
| C7 | E7-4 crypto | T (X-5), I | `mandate-executor` | The first GTC stop-limit after a crypto opening fills (its tests are pending, `a_crypto_position_carries_one_stop_limit_for_the_whole_position`); no OCO on crypto | T 80–150, I 250–400 |

Their findings stay as recorded: a continuous session against the shell's equity-only judge; the
UTC trading day (§2.2) against the equity-session dataset check; fractional increments and the
venue's `price_increment` (§2.1, §3.1); a GTC opening (§5.2); the 2% collar (`collar_crypto_x`);
the 30-day floor, which today reads `median_dollar_volume_30d = None` and so denies every crypto
opening; crypto minute bars for the participation cap; crypto fees taken in the asset (§6.3); one
GTC stop-limit for the whole position (§5.4); the 200,000 USD notional cap that nothing
enforces; and the backlog's open row on normalizing `BTCUSD` and `BTC/USD` to one instrument
(§2.3) before reconciliation compares crypto positions. The stop condition carries over: if Alpaca's BTC/USD daily bars do not start at 00:00
UTC, the slice stops and writes a decision. E1b's FT-11 applies to a crypto opening unchanged.
F-2 below governs whether the trade can happen at all.

## Decisions

### Founder-reserved (DEC-79), listed separately

| # | What | Conservative course meanwhile |
|---|---|---|
| F-1 | `PAPER_ONLY`'s figures in `mandate-executor/src/fees.rs` are "Proposed (founder) as values". **It still matters for SPY**: the first buy's fee reservation is priced from the equity figures (SEC 0.001, TAF 0.01 a share, CAT 0.01 a share), each at least ten times the transcribed schedule | Use them: they overstate fees, so buying power is tighter |
| F-2 | **BTC/USD follow-up only.** BTC/USD's 30-day median daily dollar volume on Alpaca's crypto feed is about 127,900 USD against the 1,000,000 USD floor (DEC-509). Lowering a floor or cap weakens a safety rule | No BTC/USD trade; nothing lowered |
| F-3 | Founder-owned files: `xtask/layers.toml`, `CODEOWNERS` and the workspace `Cargo.toml` for the two new crates (M1, E1a); `mandate-shell`'s dependency change (E3); and `xtask/src/main.rs`, whose `BEHAVIOUR_ONLY_TESTS` names `tests/tracer.rs`, so E3 deletes that row with the file | Each PR names the change under "Decisions needed" |
| F-4 | The run itself: credentials, downloads, the ETP classification's registration, and the confirmation before the one submission (rule 8, DEC-502 item 6) | Agents prepare and stop; the founder runs E2 |

DEC-509, the instrument, is the founder's own decision, recorded in this PR. Not hit on this path:
a crypto-spot disclosure (mandate spec §12 open question 7). The founder's envelope values are
rule 11 choices, not DEC-79 decisions.

### Engineering readings (accepted by agents)

- [DEC-503](../decisions/DEC-503.md): the model host supplies the quant output outside the cycle
  and the shell; DEC-475 item 1 traced against inference spec §1.3; item 7 records the
  coordinator's ruling on X-2.
- [DEC-504](../decisions/DEC-504.md): what a quant model's content hash covers, and how it is
  registered.
- [DEC-505](../decisions/DEC-505.md): the deployment input from the control stream, which
  registration of a kind counts, and the two facts taken from their authoritative owners.

DEC-506 and DEC-507 are held by the E7-4 tests correction, and DEC-508 was offered to #669; this
brief uses none of them.

### Defects and contradictions found

| # | Finding | Evidence | Fixed by |
|---|---|---|---|
| X-1 | E7-19 slice 4 is merged only on the journal side. The shell still writes agent records at schema version 1 with `mandate_version` alone and never loads a policy set or model registry | #644's "Not done"; `tracer.rs` `append_agent`; no `model_registry` in `crates/mandate-shell/src` | D4c |
| X-2 | The runtime's `ObservationRecorded` draft has `source`, `instrument`, `at` and inline `data`; journal spec §9.1 and `mandate-journal`'s registered schema have `source`, `instrument_id`, `as_of` and `data_ref` | `mandate-runtime/src/step.rs` `observed`; `mandate-journal/src/agent.rs` | R0 (the coordinator ruled the code moves to the spec, DEC-503 item 7) |
| X-3 | The shell's model output takes the run clock as `as_of`; mandate spec §8.2 says the data cut-off | `mandate-shell/src/map.rs` `model_output` | M1 and M2 (the host follows §8.2); E3 deletes the old mapping |
| X-4 | The pinned content hash today is the digest of `{"id","version"}`, which covers no code (mandate spec §8.1) | `paper/artifacts.rs`; `tests/fixtures/tracer/config/model-artifact.json` | M2 and D1 (DEC-504); E3 deletes the fixture |
| X-7 | The paper validation context passes `registry: None`, so V-007 is not checked, and an empty provenance | `paper/context.rs` `run_context` | D3 |
| X-8 | E7-19 slice 2 left the liquidity facts pinned to AAPL: `liquidity_facts` refuses any minute bars but `SYMBOL`'s and reads the daily bars as `SYMBOL`'s, so SPY is refused | `mandate-shell/src/paper/facts.rs` | Q1 |
| X-9 | The gate template hard-codes account rules: `IntradayMargin` with zero maintenance excess, `prior_close_equity` 0, `AccountType::Margin` | `mandate-shell/src/paper/gate.rs` `gate_template`; `mandate-executor/src/types.rs` `BrokerAccount`; `mandate-alpaca/src/wire.rs` `account` | A1, Q2 |
| X-10 | `PgJournal::append` refuses every version-2 `ModelOutputRecorded`, `DecisionMade`, and `policy_set` or `model_registry` registration as `missing_artifact`: it has no artifact source. So neither the CLI's registrations nor the shell's version-2 records can reach the Postgres journal the paper run uses | `mandate-journal-pg/src/lib.rs` `missing_config_artifact`; `mandate-journal/src/draft.rs` `config_artifact_path`; #644's review note | J0 |
| X-11 | DEC-502's "the journal verifies afterwards" has no command over a Postgres journal: `mandate journal verify` reads an exported segment file, nothing exports one from Postgres, and `PgJournal`'s own walk skips checks 6 and 7 | `mandate-cli/src/journal.rs` `VerifyArgs`; `mandate-journal-pg/src/lib.rs` `check` | V0 |
| X-12 | The CLI has no production control-stream writer: `ControlJournal` is a trait whose implementations are test doubles, `mandate-cli` does not depend on `mandate-journal-pg`, and `main.rs` wires none of the commands D1 and D2 need (review round 1, Major 1) | `mandate-cli/src/control.rs`; `approvals.rs` under `#[cfg(test)]`; `mandate-cli/Cargo.toml`; `main.rs` | P0 |
| X-5 | Crypto: `re_place` sends an OCO for a crypto position when a take-profit price is set; trading spec §5.2 allows crypto simple orders only, and §5.4 leaves the take-profit to the runtime | `mandate-executor/src/protection.rs` `re_place` | C7 (follow-up) |
| X-6 | Crypto: the gate's doc says the executor rounds a crypto price onto the venue increment; the executor rounds only Reg NMS equity ticks | `mandate-risk/src/conduct.rs` `on_grid`; `mandate-executor/src/opening.rs` | C6 (follow-up) |

## Commands

```bash
cargo nextest run -p mandate-modelhost -p mandate-shell -p mandate-paper -p mandate-runtime
cargo nextest run -p mandate-cli -p mandate-journal-pg -p mandate-alpaca -p mandate-executor
MANDATE_PG_URL=<dsn> cargo nextest run -p mandate-journal-pg -p mandate-cli   # J0, P0, V0
cargo nextest run -p <crate> --run-ignored only   # every pending test fails on its stub
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo xtask check
```

## Stop conditions

Stop and write a decision rather than continuing if:

- a slice needs trading, sizing, pricing or gate logic in `mandate-shell` or `mandate-paper`, or
  model logic anywhere but `mandate-modelhost` and the crossover's module;
- the host, or anything in shipping code, could produce an output for a pin it did not compute,
  or take an output from a file, flag or environment variable;
- a cycle can start without FT-3;
- more than one registration of a kind could count under DEC-505 item 1's rule (latest by `seq`,
  narrowed by asset id or by model triple), or a fee schedule not yet effective would be passed
  over for an older one;
- A1 finds that Alpaca's account answer names no maintenance figure, or that carrying the new
  fields would change `AccountSnapshotRecorded`'s closed payload;
- H3 can name the observation in the output's `evidence` only by changing an output after the
  host produced it (journal spec §9.1: "recorded as the model gave it");
- the instrument snapshot cannot name a dated source for SPY's ETP classification;
- an order would have to be placed outside the regular session, or a rule relaxed, to happen;
- E1b's bound would have to be a constant because no registered configuration member fits;
- any live host, live credential or second submission becomes reachable;
- a new dependency seems necessary.

## Definition of done

- [ ] One SPY order placed on the founder's paper account by `mandate-paper` through
      `ProductionCycle::run`, inside the regular session, after the founder's explicit
      confirmation.
- [ ] The deployment came from the control stream: `AgentDeployed`, `MandateConfirmed` with
      `cli_confirm`, and the stored document re-hashing to its version.
- [ ] The output came from `mandate-modelhost` with a pin that matched the registry and the
      host's computed hash, and its inputs are on the journal as `ObservationRecorded`; no
      synthetic model, proof mode or test-only branch exists.
- [ ] No instrument, model, deployment identity, gate value, account-rule value or executor value
      is a constant in shipping code; `mandate-shell` no longer depends on `mandate-backtest`;
      E7-7's AAPL assembly and `mandate-tracer` are deleted.
- [ ] Every decision's journal envelope names `mandate_version`, `policy_set` and
      `model_registry`.
- [ ] The run exited with the entry terminal and, if protection was on, the filled quantity
      covered (FT-11).
- [ ] `mandate journal verify`, with the artifact store, is clean on every stream `mandate journal
      export` wrote, and a second run sent nothing.
- [ ] Every code slice: tests first, zero missed mutants, green CI, and an independent review on
      a different model.
