# Task: the first real paper trade (DEC-502, DEC-509): one SPY order through the production cycle

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this brief is a **path brief** over the stories below, and each slice it lists is one
story's tests PR or implementation PR under DEC-77. Docs only: it writes no code.

The first trade is on **SPY** ([DEC-509](../decisions/DEC-509.md)), which supersedes DEC-450's
BTC/USD: on Alpaca's crypto feed BTC/USD's 30-day median daily dollar volume is about 127,900 USD,
below the gate's 1,000,000 USD floor. BTC/USD follows as a mandate input once the slices in
"BTC/USD follow-up" land.

## Story

- **Stories on the path:** E7-19 (the remainders of slices 2, 3 and 4, and slice 5), and new rows
  this brief adds: E15-13 (the quant model host), E10-16 (the paper confirmation writer), E19-11
  (the deployment input from the control stream). **For the BTC/USD follow-up:** E7-21 (crypto
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
| FT-11 | **No unprotected or working opening is left behind.** When a run exits, the opening is terminal, and if the mandate enables protection the filled quantity is covered by the bracket's legs or, after a partial fill, one GTC OCO (§5.4), or the unprotected interval is journaled and alerted. An entry still working at the bound is cancelled |
| FT-12 | **Replay never runs the host or calls the broker.** Replay of the run's journal folds to the same state with the host and the transport absent (journal spec §8) |

## Scope

- **Reference cases that must move from pending to passing:** none named by this path. Every
  passing mandate, trading-domain and journal case stays passing.
- **Invariants touched:** FT-1 to FT-12 above; mandate MI-1, MI-10, MI-11; inference INF-1,
  INF-2, INF-4, INF-5, INF-10 (replay half), INF-13, INF-15, INF-16 as host properties (DEC-503
  item 4); E7-7's TI-1 to TI-12.
- **Crates in scope:** `mandate-shell`, `mandate-runtime` (the `ObservationRecorded` correction,
  R0), `mandate-backtest` (one module move), `mandate-cli`, and two new crates:
  `mandate-modelhost` and `mandate-paper`.
- **Crates out of scope:** `mandate-builder`, `mandate-risk`, `mandate-executor`,
  `mandate-alpaca` and `mandate-liquidity` (used, not changed, on the SPY path; the BTC/USD
  follow-up changes the last four), `mandate-spec` and `mandate-journal` (used, not changed),
  `web/`, the model gateway and inference registry (E15-6, E15-7), research, OAuth, the vault,
  approval channels.
- **New dependencies allowed:** none. Both new crates use only workspace crates and dependencies
  already listed in `xtask/layers.toml`.
- **Safety-critical:** yes, every code slice. Each is a DEC-77 tests PR or implementation PR.
- **Size budget:** under 400 non-generated changed lines in safety-critical crates per PR; the
  slice table gives each estimate.

## Where each piece lives

| Piece | Crate | Layer | Why there |
|---|---|---|---|
| The model's code: the crossover signal | `mandate-backtest` (moved into its own module, M0) | 7 | One implementation for the backtest and the paper output; DEC-504 hashes exactly that module |
| The model host | **new** `mandate-modelhost` (pure, safety-critical) | 8 | Beside the shell, so neither can depend on the other (DEC-503 item 2) |
| The confirmation and deployment records | `mandate-cli` (existing control-stream writer, `ControlJournal`) | 7 | The founder's CLI plays the deployment manager in Phase 1 (agent harness spec §5.1) |
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
stored daily bars ─► trusted closes ─► mandate-modelhost::evaluate(pin, registry, closes, now)
                                              │   └─► Option<(Observation, ModelOutput)>   (none = no buy)
broker GETs ─► account and market snapshots ─┤
                                              ▼
             mandate-paper ─► ProductionCycle::run ─► runtime ─► builder ─► gate ─► executor ─► POST
```

- **Input.** Typed complete daily closes of the pinned instrument through the last completed
  regular session, read by the shell's existing trusted-dataset check and handed over by the paper
  adapter. The host reads no file and no clock; `now` is an argument.
- **Checks, in order.** The pin's id and version have compiled content in the host; its content
  hash equals the host's computed hash and the `model_registry` entry's; the mandate's parameter
  keys equal the content object's schema, every value is set (no default) and in bounds; the
  closes are the pinned instrument's, complete, ending at the last completed trading day at
  `now`, and at least `slow_periods` long. Any failure is a typed refusal and no output.
- **Output.** `Long` is direction `long`, conviction 1, confidence 1 (DEC-157 item 4); `Flat` and
  `Undecided` are no output. `as_of` is the last close's end, 16:00 New York on the last
  completed session (an early close's end on a short day); `expires_at` is `as_of +
  max_output_age_s` (DEC-503 item 5). So a run on the next session's morning needs a pinned
  `max_output_age_s` of at least the time since that close. A model that says `Flat` places
  nothing: the run reports it and ends. Nobody changes the windows or the mandate to get a `Long`
  (DEC-475, "outcome-chasing").
- **Journal.** After R0, the runtime records the host's inputs as `ObservationRecorded`
  (`data_ref`: the closes as a stored artifact), then the output as `ModelOutputRecorded` v2,
  before any decision (H3).

## The confirmed mandate (DEC-505)

The founder runs these CLI commands against the deployment's Postgres journal. Each invocation
commits exactly one control-stream event (DEC-155 item 5), so `config register` runs once per
object, and every command refuses a `live` environment:

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
| **Day-trading regime** | Trading §9.2: `intraday_margin` or `legacy_pdt`, by the broker's transition | The gate template hard-codes `IntradayMargin` with zero maintenance excess, `prior_close_equity` 0 and `AccountType::Margin` | **Gap** (X-9). Q2: from the broker account read and the registered rule set. With 1,000,000 USD equity both regimes allow the first opening, so it changes no outcome today; DEC-475 item 3 still forbids the constants |
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
| Trading §9.2 ↔ gate template | Account | — | Regime from the broker | **No**: constants (X-9); Q2 |
| Journal §9.1 `ObservationRecorded` ↔ runtime draft | Agent stream | `instrument_id`, `as_of`, `data_ref` vs `instrument`, `at`, inline `data` | Append | **No** (X-2); R0, ruled by the coordinator |
| DEC-484 `model_registry` ↔ shell agent drafts | `ModelOutputRecorded`, `DecisionMade` | `config_refs` | v2 binds registry and policy | **No**: the shell writes version 1 with `mandate_version` only (X-1); D4 |
| Agent harness §5.1 steps 1, 2, 4, 6 ↔ E19-11 | Construction | — | Exit on failure | Yes (DEC-505) |
| Mandate §6.1 `cli_confirm` ↔ identity spec methods per environment | Paper step-up | — | Paper only | Yes |

## Slices, in order

T is a tests PR (stubs and pending tests that fail on the stubs), I its implementation PR, D a
docs PR. Estimates are non-generated changed lines; the range is honest uncertainty, and a slice
that would cross 400 splits before review, not after. Every slice leaves E7-7's recorded tests
and the existing dry run passing until E3 deletes them (DEC-475 item 7).

| # | Story | Kind | Crate | What | After | Estimate |
|---|---|---|---|---|---|---|
| S1 | — | D | docs | This brief, DEC-503 to DEC-505, DEC-509, backlog rows | — | this PR |
| M0 | E15-13 | refactor | `mandate-backtest` | Move the crossover signal into `strategy/ma_crossover.rs`, no behavior change, mutants on the diff | — | 80–150 (mostly a move) |
| M1 | E15-13 | T | **new** `mandate-modelhost` | Crate, layer 8 (founder files); `evaluate` stub; pending tests: content object and pinned hash, pin vs registry vs host, parameter schema, input contract, output mapping, `as_of` (X-3), property tests for FT-4 and determinism | M0 | 300–400 |
| M2 | E15-13 | I | `mandate-modelhost` | Implementation (closes X-4 for new deployments) | M1 | 200–300 |
| R0 | X-2 (DEC-503 item 7) | T, I | `mandate-runtime` | `ObservationRecorded` drafted as journal spec §9.1 and the registered schema say: `instrument_id`, `as_of`, `data_ref` | — | T 120–200, I 120–200 |
| D1 | E10-16 | T, I | `mandate-cli` | `config register` (with the instrument snapshot's `etp_source`), `model register`: stored objects and `ConfigSnapshotRegistered` v1 and v2, paper only | M2 (content object) | T 300–400, I 250–350 |
| D2 | E10-16 | T, I | `mandate-cli` | `version create`, `version confirm` (`cli_confirm`), `agent deploy`, with their records | D1 | T 300–400, I 300–400 |
| D3 | E19-11 | T, I | `mandate-shell` | The deployment input from the control stream (DEC-505), replacing `--mandate` and `--config-dir`; V-007 checked (X-7) | — (builds its own control-stream fixtures) | T 300–400, I 300–400 |
| D4 | E7-19 slice 4 remainder | T, I | `mandate-shell` | `ModelOutputRecorded` and `DecisionMade` v2 with `policy_set` and `model_registry` refs (X-1) | D3 | T 150–250, I 150–250 |
| Q1 | E7-19 slice 2 remainder | T, I | `mandate-shell` | Liquidity facts and every preflight check take the instrument from the deployment input, not `SYMBOL` (X-8) | D3 | T 100–200, I 80–150 |
| Q2 | E7-19 slice 3 remainder | T, I | `mandate-shell` | Account type, day-trading regime, maintenance excess and prior-close equity from the broker account read and the registered rule set, not constants (X-9) | Q1 | T 100–200, I 80–150 |
| H3 | E15-13 | T, I | `mandate-shell` | The cycle takes the host's observation with its output; the runtime journals `ObservationRecorded` before `ModelOutputRecorded` | R0, M2, D4 | T 200–300, I 150–250 |
| E1a | E7-19 slice 5 | T, I | **new** `mandate-paper` | The paper binary: E19-11 input, preflight, host, `ProductionCycle::run`; `mandate-tracer` still builds | M2, D4, Q1 | T 300–400, I 250–350 |
| E1b | E7-19 slice 5 | T, I | `mandate-paper` | After submission, bounded GETs and executor ticks until the entry is terminal and, after a partial fill, its OCO placed; an entry still working at a configured bound is cancelled (FT-11) | E1a | T 250–350, I 200–300 |
| E3 | E7-19 slice 5 | I (deletions) | `mandate-shell` | Delete `mandate-tracer`, `paper.rs`'s constants, `Artifacts::load` and `legacy_identity`, `tracer::run`'s signal path, `MovingAverage`, `MOVING_AVERAGE`, the bars and signal stages, `map::model_output` (X-3), the E7-7 model artifact (X-4), and the `mandate-backtest` dependency | E1b, Q2, H3 | 400–900 deleted; two PRs if over 400 |
| E2 | — | run, then D | — | The manual paper run (below); a docs PR records its evidence | E3, D2 | — |

**Totals.** 27 PRs and about 5,000 to 7,700 changed lines.

**Critical path.** D3 → D4 → Q1 → E1a → E1b → E3 → E2: 12 to 13 sequential merges. The
`mandate-shell` slices (D3, D4, Q1, Q2, H3, E3) cannot overlap, so Q2 and H3 fit between E1a and
E3. In parallel, by crate: M0 to M2, R0, D1 and D2. At the recent pace of two to three review
rounds per safety-critical PR, this is **about two to three weeks**. Taking the crypto slices off
the path saved about a week of serial shell work (C3 to C5) and the E7-4 crypto slice.

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
5. `mandate journal verify` on the deployment's journal: clean.
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
| F-3 | Founder-owned files: `xtask/layers.toml`, `CODEOWNERS`, workspace `Cargo.toml` for the two new crates (M1, E1a), `mandate-shell`'s dependency change (E3) | Each PR names the change under "Decisions needed" |
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
- [DEC-505](../decisions/DEC-505.md): the deployment input from the control stream, and the two
  facts taken from their authoritative owners.

### Defects and contradictions found

| # | Finding | Evidence | Fixed by |
|---|---|---|---|
| X-1 | E7-19 slice 4 is merged only on the journal side. The shell still writes agent records at schema version 1 with `mandate_version` alone and never loads a policy set or model registry | #644's "Not done"; `tracer.rs` `append_agent`; no `model_registry` in `crates/mandate-shell/src` | D4 |
| X-2 | The runtime's `ObservationRecorded` draft has `source`, `instrument`, `at` and inline `data`; journal spec §9.1 and `mandate-journal`'s registered schema have `source`, `instrument_id`, `as_of` and `data_ref` | `mandate-runtime/src/step.rs` `observed`; `mandate-journal/src/agent.rs` | R0 (the coordinator ruled the code moves to the spec, DEC-503 item 7) |
| X-3 | The shell's model output takes the run clock as `as_of`; mandate spec §8.2 says the data cut-off | `mandate-shell/src/map.rs` `model_output` | M1 and M2 (the host follows §8.2); E3 deletes the old mapping |
| X-4 | The pinned content hash today is the digest of `{"id","version"}`, which covers no code (mandate spec §8.1) | `paper/artifacts.rs`; `tests/fixtures/tracer/config/model-artifact.json` | M2 and D1 (DEC-504); E3 deletes the fixture |
| X-7 | The paper validation context passes `registry: None`, so V-007 is not checked, and an empty provenance | `paper/context.rs` `run_context` | D3 |
| X-8 | E7-19 slice 2 left the liquidity facts pinned to AAPL: `liquidity_facts` refuses any minute bars but `SYMBOL`'s and reads the daily bars as `SYMBOL`'s, so SPY is refused | `mandate-shell/src/paper/facts.rs` | Q1 |
| X-9 | The gate template hard-codes account rules: `IntradayMargin` with zero maintenance excess, `prior_close_equity` 0, `AccountType::Margin` | `mandate-shell/src/paper/gate.rs` `gate_template` | Q2 |
| X-5 | Crypto: `re_place` sends an OCO for a crypto position when a take-profit price is set; trading spec §5.2 allows crypto simple orders only, and §5.4 leaves the take-profit to the runtime | `mandate-executor/src/protection.rs` `re_place` | C7 (follow-up) |
| X-6 | Crypto: the gate's doc says the executor rounds a crypto price onto the venue increment; the executor rounds only Reg NMS equity ticks | `mandate-risk/src/conduct.rs` `on_grid`; `mandate-executor/src/opening.rs` | C6 (follow-up) |

## Commands

```bash
cargo nextest run -p mandate-modelhost -p mandate-shell -p mandate-paper -p mandate-runtime
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
- [ ] `mandate journal verify` is clean, and a second run sent nothing.
- [ ] Every code slice: tests first, zero missed mutants, green CI, and an independent review on
      a different model.
