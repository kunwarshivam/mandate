# Task: the first real paper trade (DEC-502): one BTC/USD order through the production cycle

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this brief is a **path brief** over the stories below, and each slice it lists is one
story's tests PR or implementation PR under DEC-77. Docs only: it writes no code.

## Story

- **Stories:** E7-19 (slice 4's shell half and slice 5), and new rows this brief adds:
  E15-13 (the quant model host), E10-16 (the paper confirmation writer), E19-11 (the deployment
  input from the control stream), E7-21 (crypto minute bars), E7-22 (crypto pairs in the
  production assembly). It also needs E7-4's crypto protection, whose tests are already pending
  ([backlog](../06-backlog-v1.md)).
- **Acceptance criteria (verbatim, [DEC-502](../decisions/DEC-502.md) item 1):** "The first real
  paper trade is one order on the founder's Alpaca paper account that reaches the broker only
  through the production cycle API ([DEC-475](../decisions/DEC-475.md)), where: the deployment input is built
  from a **confirmed mandate version**, not a test fixture; the model output comes from a **pinned
  signal model** (mandate spec §8.1, V-007), run outside the production cycle and the shell, and
  handed to the cycle as its typed `ModelOutput`; the Alpaca paper adapter calls that API, and
  E7-7's AAPL assembly is deleted (E7-19 slice 5); the journal verifies afterwards, and a restart
  sends no second order."
- **PRD / HLD / spec anchors:** PRD FR-3.1, FR-3.5, FR-3.7, FR-4.3; HLD §5 (agent runtime),
  §6.B (decision cycle), §9 (intelligence layer); mandate spec §2.1, §4, §6.1, §8.1 to §8.3,
  §9.1, §10; trading-domain spec §2.1 to §2.3, §3, §4.2, §4.3, §5.1 to §5.4, §6.3, §9, §11;
  journal spec §4, §5.1, §8, §9.1, §9.2, §11; agent harness spec §5.1; inference spec §1.3.
- **Decisions that apply:** DEC-36, DEC-52, DEC-77, DEC-79, DEC-155, DEC-157, DEC-176, DEC-261,
  DEC-450, DEC-466, DEC-470, DEC-471, DEC-475, DEC-484, DEC-502, and this brief's
  [DEC-503](../decisions/DEC-503.md), [DEC-504](../decisions/DEC-504.md) and
  [DEC-505](../decisions/DEC-505.md).

## Invariants first

Every slice keeps all of these. Each is a test (named in the slice that adds it), and each
oracle is shown to fail on a seeded bug before it is trusted.

| # | Invariant |
|---|---|
| FT-1 | **One door.** An order reaches the broker only through `ProductionCycle::run`, called by the paper adapter with `--place-one-order` after the founder's confirmation. No other shipping path holds a trading transport |
| FT-2 | **No product choice in shipping code** (DEC-475 item 3). No instrument, model id or version, deployment id, gate value or executor value is a constant or compile-time default in shipping code. The same binary runs a BTC/USD and an equity deployment that differ only in journaled inputs |
| FT-3 | **Confirmed version only** (DEC-505). A cycle starts only when the latest `AgentDeployed` for the agent names version V, the stored document re-hashes to V, `MandateConfirmed` for V confirms every envelope path with `cli_confirm` in paper, and every V-rule passes with the model registry present |
| FT-4 | **Pinned model only** (DEC-503 item 3). An output exists only if the mandate's pin, the one matching `model_registry` entry, and the host's compiled content hash agree. Any disagreement is no output, and no output is no buy |
| FT-5 | **Opinion, never order.** The model host emits only `ModelOutput`; it cannot reach sizing, the gate, the journal, the executor or the connector, by crate layering |
| FT-6 | **Journal before acting.** `ModelOutputRecorded` (v2) commits before `DecisionMade` (v2), before `IntentProposed`, before `OrderSubmitted`, before the `POST`. Every effective input is a `config_refs` hash (DEC-484) |
| FT-7 | **At most one submission, and none on restart.** A run submits at most one opening. A restart, or a second run in the same cycle, reconciles by `client_order_id` and submits nothing (E7-7's TI-1 and TI-12) |
| FT-8 | **Fail closed.** A fact that is missing, stale, ambiguous, or another instrument's is a refusal before any `POST` (`AGENTS.md` rule 3). Every crypto addition below is refusal-only |
| FT-9 | **Nothing about crypto is relaxed** (DEC-450 item 3). USD pair, `crypto_status = ACTIVE`, the 30-day liquidity floor at or above the platform minimum, the 2% collar, both participation caps, and the 200,000 USD notional cap all apply |
| FT-10 | **Paper only.** Only the paper trading host and the data host are compiled (ES-23); a `live` mandate is refused by the CLI and by V-001 |
| FT-11 | **No unprotected or working opening is left behind.** When a run exits, the opening is terminal, and if the mandate enables protection the filled position carries one GTC stop-limit for its whole net quantity (§5.4, DEC-36), or the unprotected interval is journaled and alerted; a remainder still working at the bound is cancelled. Never an OCO or bracket on crypto (§5.2) |
| FT-12 | **Replay never runs the host or calls the broker.** Replay of the run's journal folds to the same state with the host and the transport absent (journal spec §8) |

## Scope

- **Reference cases that must move from pending to passing:** none named by this path. Every
  passing mandate, trading-domain and journal case stays passing. The E7-4 crypto slice turns
  its own pending tests live.
- **Invariants touched:** FT-1 to FT-12 above; mandate MI-1, MI-10, MI-11; inference INF-1,
  INF-2, INF-4, INF-5, INF-10 (replay half), INF-13, INF-15, INF-16 as host properties (DEC-503
  item 4); E7-7's TI-1 to TI-12.
- **Crates in scope:** `mandate-shell`, `mandate-alpaca`, `mandate-liquidity`,
  `mandate-executor`, `mandate-risk` (snapshot fields only), `mandate-backtest` (one module move),
  `mandate-cli`, and two new crates: `mandate-modelhost` and `mandate-paper`.
- **Crates out of scope:** `mandate-runtime` (unless X-2 is ruled a runtime fix), `mandate-builder`,
  `mandate-spec` and `mandate-journal` (used, not changed), `web/`, the model gateway and inference
  registry (E15-6, E15-7), research, OAuth, the vault, approval channels.
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
| Crypto minute bars | `mandate-alpaca` `data.rs` | 7 | The connector owns every broker and data-host read (ES-23) |
| 30-day crypto median dollar volume | `mandate-liquidity` | 1 | Owns every liquidity figure (DEC-471) |
| Asset-class-aware snapshots (judge, gate template, run and executor contexts) | `mandate-shell` | 8 | Generalizes today's `paper/*` in place, so the E7-7 path stays runnable (DEC-475 item 7) |
| Crypto limit on the venue's price increment | `mandate-executor` | 6 | The executor owns rounding against the order (§2.1) |
| The paper adapter binary | **new** `mandate-paper` (impure, safety-critical) | 9 | The only crate that sees both the host and the shell; holds the credential read and the one clock read |

**Founder review.** Both new crates add an `xtask/layers.toml` entry, a `CODEOWNERS` line, and a
workspace `Cargo.toml` member; `mandate-shell` drops its `mandate-backtest` dependency in E3. Each
of those files is founder-owned (ES-13), so every PR that touches one says so under "Decisions
needed", as existing practice does.

## The model host (DEC-503, DEC-504)

```text
control stream ──► E19-11 deployment input ──┐
stored daily bars ─► trusted closes ─► mandate-modelhost::evaluate(pin, registry, closes, now)
                                              │   └─► Option<ModelOutput>   (no output = no buy)
broker GETs ─► account and market snapshots ─┤
                                              ▼
             mandate-paper ─► ProductionCycle::run(output) ─► runtime ─► builder ─► gate ─► executor ─► POST
```

- **Input.** Typed complete daily closes of the pinned instrument, read by the shell's trusted
  dataset check (C3) and handed over by the paper adapter. The host reads no file and no clock;
  `now` is an argument.
- **Checks, in order.** The pin's id and version have compiled content in the host; its content
  hash equals the host's computed hash and the `model_registry` entry's; the mandate's parameter
  keys equal the content object's schema, every value is set (no default) and in bounds; the
  closes are the pinned instrument's, complete, ending at the last completed trading day at
  `now`, and at least `slow_periods` long. Any failure is a typed refusal and no output.
- **Output.** `Long` is direction `long`, conviction 1, confidence 1 (DEC-157 item 4); `Flat` and
  `Undecided` are no output. `as_of` is the last close's end; `expires_at` is `as_of +
  max_output_age_s` (DEC-503 item 5). A model that says `Flat` places nothing: the run reports it
  and ends. Nobody changes the windows or the mandate to get a `Long` (DEC-475, "outcome-chasing").
- **Journal.** The runtime records the output as `ModelOutputRecorded` v2 before any decision. The
  input record (`ObservationRecorded`) waits on X-2.

## The confirmed mandate (DEC-505)

The founder runs these CLI commands against the deployment's Postgres journal. Each invocation
commits exactly one control-stream event (DEC-155 item 5), so `config register` runs once per
object, and every command refuses a `live` environment:

1. `mandate config register` stores and registers each configuration object: fee schedule,
   trading calendar, instrument snapshot, rule set (version 1), and `policy_set` and
   `model_registry` (version 2, DEC-484 item 4).
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

**Envelope values are the founder's** (`AGENTS.md` rule 11): allocation, `max_order_usd`, the two
windows, `max_output_age_s`, protection, `crypto_stop_limit_offset` (V-008 when protection is
on), and `take_profit_distance`. This brief proposes none of them.

## BTC/USD specifics

| Rule | Spec | Today's code | Gap, and the slice that closes it |
|---|---|---|---|
| **Session: continuous** | Trading §4.3 ("Crypto trades continuously"); mandate spec's `crypto` session | `paper/judge.rs` requires the New York regular session outside the close window; the builder market is `Regular` | **Equity-only.** C4: no session or close-window refusal for crypto; C5: `MarketSession::Crypto`. The risk day stays 00:00 New York (mandate §5.4) |
| **Trading day for bars** | Trading §2.2: the crypto reporting day ends 00:00 UTC | `adapters::trusted` ends the span on the last completed **equity** session | **Equity-only.** C3: the last completed UTC day, every UTC day present. The download's day boundary must be read from the bars, never assumed (stop condition) |
| **Increment: fractional** | Trading §2.1 (≤ 9 places, per-instrument increment), §3.1 (`min_order_size`, `min_trade_increment` from the asset record), §5.3 rule 2 | The instrument artifact must say `us_equity` and `whole`; the gate snapshot's `min_order_size` and `qty_increment` are one share | **Equity-only.** C4: both from the asset record, `ShareIncrement::Fractional` (nine places) |
| **Price increment** | Trading §2.1: crypto uses the broker's `price_increment`; a buy limit rounds down | The gate leaves a crypto price at the collar bound "because the instrument snapshot carries no venue increment"; the executor rounds only `RegNmsEquity` ticks | **Gap** (X-6). C6: the executor's snapshot carries `price_increment`; a crypto buy rounds down, a sell up (`TickRule::Increment`) |
| **TIF** | Trading §5.1 (crypto openings are limit orders in the collar; IOC only for adds), §5.2 (crypto: GTC or IOC) | The executor's `order_tif` gives GTC for crypto, but `TrustedPaperContext::asset_class` answers only `UsEquity`; the advisory gate and `OrderExecution` say `Day` | **Equity-only.** C5: the opening is GTC; the asset class is answered as crypto |
| **Collar** | Trading §9.6: 2% for crypto, passive band 20% | `collar_crypto_x` exists in `GateConfig` and the rule set (`0.02`) and `conduct.rs` reads it for crypto | None, once the snapshot says crypto (C4) |
| **Liquidity floor** | Trading §3.2 item 7: USD pair, `crypto_status = ACTIVE`, **30-day** median daily dollar volume (USD) ≥ the crypto floor, platform minimum 1,000,000 USD | `floor.rs` implements it over `median_dollar_volume_30d`; the shell sets that field `None` and `mandate-liquidity` computes only 20 sessions, so every crypto opening is denied `below_liquidity_floor` | **Gap.** C2: a 30-day median over UTC days; C3 and C4 fill the field |
| **Participation caps** | Trading §9.6: order ≤ 5% of trailing 5-minute volume; day ≤ 5% of 20-day ADV | `conduct.rs` reads `trailing_5m_volume` and `adv_20d` for any class; `recent_minute_bars` refuses a pair and a window that crosses a New York date | **Gap.** C1: crypto minute bars; C3: ADV over 20 UTC days, still truncated to whole units, which only tightens |
| **Quote freshness** | Trading §4.2: the `crypto` profile; an opening needs a fresh quote | `latest_quote` reads the crypto feed; `judge` requires `Feed::Iex`; the rule set names only `iex_quote_max_age_s` | **Equity-only.** C4: the rule set gains `crypto_quote_max_age_s` (1 to 60 s, the same bound as IEX's) |
| **Fees** | Trading §6.3: maker/taker bps; a buy's fee in the asset; mandate §8.3's β = 1 − asset fee rate | The gate template's reservation prices a `UsEquity` order; the builder's `fee_rate_asset` is 0; `PAPER_ONLY` holds 250 bps for crypto (F-1) | **Gap.** C4: reservation by asset class; C5: `fee_rate_asset` from the schedule's taker rate |
| **Protection** | Trading §5.4 (crypto: one GTC stop-limit for the whole position; take-profit managed by the runtime), mandate V-008 | `crypto_stop_limit_offset` answers `None`; `a_crypto_position_carries_one_stop_limit_for_the_whole_position` is `#[ignore = "pending E7-4"]`; `re_place` sends an OCO for crypto when a take-profit is set (X-5) | **Gap.** C5 reads the offset from the mandate; C7 implements E7-4's crypto placement and fixes X-5; E1b keeps the run up until it is placed (FT-11) |
| **Notional cap** | Trading §5.2: 200,000 USD per crypto order | Enforced nowhere | **Gap.** C4 refuses a crypto deployment whose `max_order_usd` exceeds it; a gate check is a later E7-22 row |
| **Day trades, settlement** | Trading §9.2 (crypto never counts), §8.4 | `daytrades.rs` skips crypto | None |
| **Symbol** | Trading §2.3: `BTC/USD` and `BTCUSD` are one instrument; the id is the asset UUID | The connector sends a pair without its slash on close; positions may come back as `BTCUSD` | **Verify** in E1's recorded fixtures that reconciliation matches the position to the pinned asset id |

## Lifecycle walk

| State | Entered | Blocks | Ends | Who | Midnight, restart, version change |
|---|---|---|---|---|---|
| Version created, not confirmed | `MandateVersionCreated` | Every cycle (V-020) | `MandateConfirmed` | Founder | Nothing carries; a new version starts over |
| Confirmed, not deployed | `MandateConfirmed` | Every cycle (no `AgentDeployed`) | `AgentDeployed` | Founder | As above |
| Deployed | `AgentDeployed` | Nothing | `AgentStopped`, or a new version | Founder | A new version re-runs E19-11; the old pin's outputs are ignored `not_pinned` |
| Dry run | A run without `--place-one-order` | The `POST` | The report (would-place, or the refusal) | — | Keeps no journal (DEC-157 item 6) |
| No output | Host refusal, `Flat`, `Undecided` | Every buy | A later run whose closes say `Long` | Nobody | UTC midnight changes the last completed day; a stale dataset is refused until re-downloaded |
| Intent journaled, not sent | `IntentProposed` committed | A second cycle (TI-12) | `OrderSubmitted` or the intent's expiry (`max_intent_age_s`) | Executor | Restart re-hands nothing past its age |
| Submitted, answer lost (`Unknown`) | `OrderSubmitted` without an answer | Every order in BTC/USD (§5.3 rule 9) | Query by `client_order_id` or reconciliation | Executor | Restart reconciles first; never a second `POST` |
| Working (GTC opening) | Accepted | Another opening (§5.3 rule 6) | Filled, or cancelled at E1b's bound | Executor, paper adapter | **The adapter does not exit with it working** (FT-11); a crash leaves it to the next run's reconciliation |
| Filled, unprotected | First fill | — | The GTC stop-limit is placed, or `max_unprotected_s` alerts | Executor | Restart re-covers from the journal (`re_cover`); NY midnight starts a risk day |
| Protected | `ProtectionChanged` placed | Adds (E7-4 crypto add sequence, stubbed) | Exit or kill switch | Owner, risk engine | GTC rests across days |
| Kill switch | Agent-scoped kill switch (E7-4 slice 7) | Every opening | Owner acknowledgment | Owner | Crypto sells go immediately (§5.5) |

## Adversary review

| Actor | Attempt | Blocked or disclosed by |
|---|---|---|
| Careless founder | Hand-edits the mandate file after confirming | The deployment reads the stored document by hash; an edited file is not V (FT-3) |
| Careless founder | Changes the windows until the model says `Long` | Each change is a new risk-increasing version with step-up, journaled; the brief forbids it as outcome-chasing (DEC-475) |
| Careless founder | Runs twice | TI-12: a second run in an open cycle refuses; reconciliation finds the first order |
| Bad model code | Someone edits the crossover but keeps 1.0.0 | DEC-504 item 2's pinned-hash test fails the build |
| Hand-fed output | A file or flag supplies a `Long` | No such input exists: the adapter's only output source is the host; the host's only input is trusted closes (FT-4) |
| Malicious insider | Registers a second hash under the same version | DEC-504 item 4; V-007 and DEC-484 item 5 require exactly one matching entry |
| Bad market tick | A wild quote or a thin minute | The 2% collar, a crossed or stale quote refused, participation caps, and the 30-day floor |
| Thin venue | Alpaca's crypto volume is below the floor or the caps bind | The gate denies; nobody lowers a floor or cap (F-2) |
| Lost answer | The `POST` times out | `Unknown`, query by id, never a second submission (FT-7) |
| Fill after exit | The opening fills after the process ends | E1b waits for a terminal state and cancels at its bound (FT-11) |
| Revoked connection | A `ConnectionRevoked` is on the stream | DEC-505 item 3 keeps the fold's refusal |

## Cross-spec trace

| Reference | Scope | Units | Outcome | Matches? |
|---|---|---|---|---|
| DEC-475 item 1 "a model gateway supplies" ↔ inference §1.3 (quant makes no call) | Quant only | — | Host supplies the same typed output | Read by DEC-503 |
| Mandate §8.1 content hash ↔ journal §9.2 `ConfigSnapshotRegistered.content_hash` ↔ `ModelOutputRecorded.content_hash` (`ref`) | Every model | SHA-256 of canonical JSON | Re-hashes at §11 check 6 | Yes, once the content object is stored (DEC-504 item 3) |
| Mandate §8.2 `as_of` ↔ the shell's `map::model_output` | Every output | Timestamp | Data cut-off | **No**: the shell uses the run clock (X-3); the host follows §8.2 |
| Trading §3.2 item 7 ↔ `mandate-liquidity` | Crypto floor | USD, 30 UTC days | Deny below | **No**: 20 sessions only; C2 |
| Trading §2.1 crypto `price_increment` ↔ gate and executor | Crypto limit | Price | Round against the order | **No** (X-6); C6 |
| Trading §5.2 crypto simple orders ↔ executor `re_place` | Crypto protection | — | One stop-limit | **No** (X-5); C7 |
| Journal §9.1 `ObservationRecorded` ↔ runtime draft | Agent stream | `instrument_id`, `as_of`, `data_ref` vs `instrument`, `at`, inline `data` | Append | **No** (X-2); stop and report |
| DEC-484 `model_registry` ↔ shell agent drafts | `ModelOutputRecorded`, `DecisionMade` | `config_refs` | v2 binds registry and policy | **No**: the shell writes version 1 with `mandate_version` only (X-1); D4 |
| Agent harness §5.1 steps 1, 2, 4, 6 ↔ E19-11 | Construction | — | Exit on failure | Yes (DEC-505) |
| Mandate §6.1 `cli_confirm` ↔ identity spec methods per environment | Paper step-up | — | Paper only | Yes |

## Slices, in order

T is a tests PR (stubs and pending tests that fail on the stubs), I its implementation PR, D a
docs PR. Estimates are non-generated changed lines; the range is honest uncertainty, and a slice
that would cross 400 splits before review, not after. "Runnable" means E7-7's recorded tests and
the existing dry run still pass after the slice (DEC-475 item 7).

| # | Story | Kind | Crate | What | After | Estimate |
|---|---|---|---|---|---|---|
| S1 | — | D | docs | This brief, DEC-503 to DEC-505, backlog rows | — | this PR |
| M0 | E15-13 | refactor | `mandate-backtest` | Move the crossover signal into `strategy/ma_crossover.rs`, no behavior change, mutants on the diff | — | 80–150 (mostly a move) |
| M1 | E15-13 | T | **new** `mandate-modelhost` | Crate, layer 8 (founder files); `evaluate` stub; pending tests: content object and pinned hash, pin vs registry vs host, parameter schema, input contract, output mapping, `as_of`, property tests for FT-4 and determinism | M0 | 300–400 |
| M2 | E15-13 | I | `mandate-modelhost` | Implementation | M1 | 200–300 |
| D1 | E10-16 | T, I | `mandate-cli` | `config register`, `model register`: stored objects and `ConfigSnapshotRegistered` v1 and v2, paper only | M2 (content object) | T 300–400, I 250–350 |
| D2 | E10-16 | T, I | `mandate-cli` | `version create`, `version confirm` (`cli_confirm`), `agent deploy`, with their records | D1 | T 300–400, I 300–400 |
| D3 | E19-11 | T, I | `mandate-shell` | The deployment input from the control stream (DEC-505), replacing `--mandate` and `--config-dir` | D2 for fixtures | T 300–400, I 300–400 |
| D4 | E7-19 slice 4b | T, I | `mandate-shell` | `ModelOutputRecorded` and `DecisionMade` v2 with `policy_set` and `model_registry` refs (DEC-484) | D3 | T 150–250, I 150–250 |
| C1 | E7-21 | T, I | `mandate-alpaca` | Crypto recent minute bars: `/v1beta3/crypto/us/bars`, the answer keyed by pair, a UTC window | — | T 200–300, I 100–200 |
| C2 | E7-22 | T, I | `mandate-liquidity` | 30-day median daily dollar volume | — | T 100–200, I 60–120 |
| C3 | E7-22 | T, I | `mandate-shell` | Crypto daily-bar trust over UTC days; liquidity facts by asset class | C1, C2 | T 250–350, I 200–300 |
| C4 | E7-22 | T, I | `mandate-shell` | Judge and gate template by asset class: session, crypto feed and `crypto_quote_max_age_s`, `crypto_status`, increments from the asset record, quote currency, fee reservation, the 200,000 USD notional refusal | C3 | T 300–400, I 250–400 |
| C5 | E7-22 | T, I | `mandate-shell` | Run and executor contexts by asset class: `MarketSession::Crypto`, GTC, `fee_rate_asset`, `crypto_stop_limit_offset` | C4 | T 250–350, I 200–300 |
| C6 | E7-22 | T, I | `mandate-executor`, `mandate-risk` | Crypto limits on `price_increment` | — | T 150–250, I 100–200 |
| C7 | E7-4 crypto | T (X-5), I | `mandate-executor` | The first GTC stop-limit after a crypto opening fills (existing pending tests); no OCO on crypto | — | T 80–150, I 250–400 |
| E1a | E7-19 slice 5 | T, I | **new** `mandate-paper` | The paper binary: E19-11 input, preflight, host, `ProductionCycle::run`; `mandate-tracer` still builds | M2, D4, C5, C1 | T 300–400, I 250–350 |
| E1b | E7-19 slice 5 | T, I | `mandate-paper` | After submission, bounded GETs until the opening is terminal and protection placed, or the remainder cancelled (FT-11) | E1a, C7 | T 250–350, I 200–300 |
| E3 | E7-19 slice 5 | I (deletions) | `mandate-shell` | Delete `mandate-tracer`, `paper.rs`'s constants, `Artifacts::load` and `legacy_identity`, `tracer::run`'s signal path, `MovingAverage`, `MOVING_AVERAGE`, the bars and signal stages, `map::model_output`, and the `mandate-backtest` dependency | E1b | 400–900 deleted; split into two PRs if over 400 |
| E2 | — | run, then D | — | The manual paper run (below); a docs PR records its evidence | E3, C6 | — |

**Totals.** About 30 PRs and 7,000 to 9,500 changed lines. The shell chain (D3, D4, C3, C4, C5,
E3) is serial and sets the critical path; M, D1 and D2, C1, C2, C6 and C7 run in parallel by
crate. At the recent pace of two to three review rounds per safety-critical PR, that is **two to
four weeks**, longer than DEC-502's "one to two weeks".

### The manual paper run (E2)

Only the founder runs it, with paper credentials and data keys the agents never see
(`AGENTS.md` rule 8):

1. `mandate download` BTC/USD daily bars covering at least max(30, `slow_periods`) completed
   UTC days, then `mandate inspect`: zero problems and zero true gaps.
2. The five registration and confirmation commands above.
3. `mandate-paper` without `--place-one-order`: it reports the order it would place, or the exact
   refusal. A `Flat` model or a gate denial ends here; neither is worked around.
4. With the founder's explicit confirmation immediately before it (DEC-502 item 6),
   `--place-one-order`: one `POST`; the run stays until FT-11 holds.
5. `mandate journal verify` on the deployment's journal: clean.
6. The same command again: it reconciles, submits nothing, and reports the order it found.

## Decisions

### Founder-reserved (DEC-79), listed separately

| # | What | Conservative course meanwhile |
|---|---|---|
| F-1 | `PAPER_ONLY`'s crypto fee figures (250 bps maker and taker in `mandate-executor/src/fees.rs`) are marked "Proposed (founder) as values". The first trade's fee reservation and the builder's asset-fee rate rest on them | Use them: they overstate fees, so sizing and buying power are tighter |
| F-2 | If Alpaca's crypto feed shows BTC/USD below the 1,000,000 USD platform floor over 30 days, or the participation caps bind at the founder's order size, the gate denies. Lowering a floor or cap weakens a safety rule. (M1's exit run saw 3,709 of 7,150 BTC/USD minute bars with zero volume, so a thin venue is likely) | No trade, nothing lowered; report the denial |
| F-3 | Founder-owned files: `xtask/layers.toml`, `CODEOWNERS`, workspace `Cargo.toml` for the two new crates (M1, E1a), `mandate-shell`'s dependency change (E3) | Each PR names the change under "Decisions needed" |
| F-4 | The run itself: credentials, downloads, and the confirmation before the one submission (rule 8, DEC-502 item 6) | Agents prepare and stop; the founder runs E2 |

Not hit on this path: a crypto-spot disclosure (mandate spec §12 open question 7) is legal text,
but the first trade is the founder's own paper account with no user shown anything. The founder's
envelope values are rule 11 choices, not DEC-79 decisions.

### Engineering readings (accepted by agents)

- [DEC-503](../decisions/DEC-503.md): the model host supplies the quant output outside the cycle
  and the shell; DEC-475 item 1 traced against inference spec §1.3.
- [DEC-504](../decisions/DEC-504.md): what a quant model's content hash covers, and how it is
  registered.
- [DEC-505](../decisions/DEC-505.md): the deployment input from the control stream, and the two
  facts taken from their authoritative owners.

### Defects and contradictions found (reported, not chosen)

| # | Finding | Evidence |
|---|---|---|
| X-1 | E7-19 slice 4 is merged only on the journal side. The shell still writes agent records at schema version 1 with `mandate_version` alone and never loads a policy set or model registry | #644's "Not done"; `tracer.rs` `append_agent`; no `model_registry` in `crates/mandate-shell/src` |
| X-2 | The runtime's `ObservationRecorded` draft has `source`, `instrument`, `at` and inline `data`; journal spec §9.1 and `mandate-journal`'s registered schema have `source`, `instrument_id`, `as_of` and `data_ref` | `mandate-runtime/src/step.rs` `observed`; `mandate-journal/src/agent.rs` |
| X-3 | The shell's model output takes the run clock as `as_of`; mandate spec §8.2 says the data cut-off | `mandate-shell/src/map.rs` `model_output` |
| X-4 | The pinned content hash today is the digest of `{"id","version"}`, which covers no code (mandate spec §8.1) | `paper/artifacts.rs`; `tests/fixtures/tracer/config/model-artifact.json` |
| X-5 | `re_place` sends an OCO for a crypto position when a take-profit price is set; trading spec §5.2 allows crypto simple orders only, and §5.4 leaves the take-profit to the runtime | `mandate-executor/src/protection.rs` `re_place` |
| X-6 | The gate's doc says the executor rounds a crypto price onto the venue increment; the executor rounds only Reg NMS equity ticks | `mandate-risk/src/conduct.rs` `on_grid`; `mandate-executor/src/opening.rs` |
| X-7 | The paper validation context passes `registry: None`, so V-007 is not checked, and an empty provenance | `paper/context.rs` `run_context` (closed by D3) |

## Commands

```bash
cargo nextest run -p mandate-modelhost -p mandate-shell -p mandate-paper
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
- Alpaca's BTC/USD daily bars do not start at 00:00 UTC (trading spec §2.2), or a minute or day
  is missing that the venue's continuous calendar says exists;
- a crypto rule would have to be relaxed to place the order (F-2);
- X-2 is needed before the coordinator rules on it;
- any live host, live credential or second submission becomes reachable;
- a new dependency seems necessary.

## Definition of done

- [ ] One BTC/USD order placed on the founder's paper account by `mandate-paper` through
      `ProductionCycle::run`, after the founder's explicit confirmation.
- [ ] The deployment came from the control stream: `AgentDeployed`, `MandateConfirmed` with
      `cli_confirm`, and the stored document re-hashing to its version.
- [ ] The output came from `mandate-modelhost` with a pin that matched the registry and the
      host's computed hash; no synthetic model, proof mode or test-only branch exists.
- [ ] No instrument, model, deployment identity, gate value or executor value is a constant in
      shipping code; `mandate-shell` no longer depends on `mandate-backtest`; E7-7's AAPL assembly
      and `mandate-tracer` are deleted.
- [ ] Every decision's journal envelope names `mandate_version`, `policy_set` and
      `model_registry`.
- [ ] The run exited with the opening terminal and, if protection was on, one GTC stop-limit
      resting (FT-11).
- [ ] `mandate journal verify` is clean, and a second run sent nothing.
- [ ] Every code slice: tests first, zero missed mutants, green CI, and an independent review on
      a different model.
