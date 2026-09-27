# Task: E7-7 the tracer bullet — one order end to end on Alpaca paper

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15), **stream L**. This is the
thinnest end-to-end path that places **one** order on the owner's Alpaca **paper** account, wired
through the real crates rather than through a demonstration of its own, so that integration defects
show up now instead of at the Phase 1 soak (M7).

The founder approved the tracer bullet on 2026-09-27 with one condition: **no regression in
verification**. "The tracer's verification condition" below says what that condition means as code,
and the Definition of done holds it.

The tracer is not a demo and not a throwaway. It lands with every stage stubbed, proving that a
half-built platform sends no orders, and each upstream stream's merge flips exactly one stage from
stub to real. When the last stage flips, the same test that proved "no order" becomes the test that
proves "one order, journaled before it was sent". That is the whole value: the integration is
exercised on the day each piece lands, not months later.

## Story

- **Story:** E7-7 ([backlog](../06-backlog-v1.md#e7-alpaca-connector-and-recovery)), minted by this
  brief and reserved in the [decision log](../04-decision-log.md#reserved-identifiers).
- **Acceptance criteria (verbatim, as this brief sets them):** "As the founder, I want one order
  placed end to end on my Alpaca paper account through the real crates, so that integration defects
  appear before the Phase 1 soak." *Accepted when:* the whole path runs in CI against recorded Alpaca
  paper fixtures and touches no network; with any one stage replaced by a stub that returns its
  crate's `Unimplemented` error, **zero** orders reach the connector and nothing is journaled as
  submitted; a manual paper run places exactly one order, journals its intent before sending it, and
  refuses to run against any host that is not Alpaca's paper host.
- **PRD / HLD / spec anchors:** [PRD v1](../../product/04-prd-v1.md) FR-6 (the agent runs a mandate
  continuously) and FR-7 (journal before acting); [HLD](../../HLD.md) §5 (the runtime's components in
  order: perception, memory, signal models, order builder, autonomy policy, risk gate, executor,
  journal; "Durability": the write-ahead intent, event-sourced state), §6.B (the decision cycle's six
  steps, which are the tracer's spine), §6.C (escalation — cited for what the tracer deliberately
  leaves out), §6.D (crash recovery: replay, reconcile, resume, pause on anything unaccounted for);
  [trading domain spec](../../specs/trading-domain.md) §5.1 (the v1 order policy: limit openings in
  the regular session), §5.7 (the order lifecycle and the broker status mapping), §9.1 (the gate's
  evaluation order, run as the binding gate by the executor), §10 (paper mode), §11
  (reconciliation), §12 (the account-stream events); [mandate spec](../../specs/mandate.md) §5.3 (the
  gate's place), §6 (autonomy classification), §8 (signal-model outputs and the order builder);
  [journal spec](../../specs/journal.md) §2 (streams and the account stream's single writer), §3 (the
  envelope), §5.1 (append, idempotency, fencing), §5.2 (write before acting), §8 (replay and
  `fold_version`), §11 (verification); [ADR-0001](../../adr/0001-engineering-setup.md) ES-01 (`web/`
  is M9), ES-02 (the crate layers), ES-06 (`handle(&mut State, Input) -> Vec<Effect>`), ES-09 (typed
  errors with stable codes, `secrecy`, the log scan), ES-12 (the CI jobs), ES-19 (injected transport
  and clock, no network in tests), ES-20 (`IntentSink` and `TimerSource`), ES-21 (determinism), ES-23
  (the paper/live boundary, vendor numbers never through `f64`), ES-24 (the latency budget);
  [glossary](../../product/glossary.md) (intent, idempotency key, reconciliation, working universe).
- **Decisions that apply:** DEC-07 (journal the intent before sending any order; idempotency keys;
  event-sourced recovery), DEC-08 (one process per agent deployment), DEC-23 (Alpaca first), DEC-26
  (one serialized account ledger per broker account), DEC-29 and DEC-37 (limit-only openings within
  a collar), DEC-77 (the brief / tests / implementation sequence this story follows), DEC-79 (agents
  land their own work; what stays with the founder), DEC-90 (the research basket, which is where the
  tracer's instrument comes from — internal test data, not a product instrument choice), DEC-99
  (forward paper is the only evidence of thesis quality — the tracer produces none and claims none),
  DEC-103 (the Phase 1 thin slice runs in the team's internal paper workspaces only), DEC-110 (the
  pending-tests gate), DEC-112 (the documentation-only CI short path, which this brief PR takes),
  DEC-127 (E4-2's baseline strategy, the tracer's signal), DEC-128 to DEC-133 (the crates the tracer
  binds), DEC-136 (the owner `Stop`'s flat-or-release precondition).
- **Non-negotiables this story is mostly about:** rule 1 (no code path acts outside the mandate),
  rule 3 (timeouts and ambiguity resolve to a safe default that never adds risk — the tracer's
  central mechanism), rule 4 (LLMs produce opinions, never orders — the tracer has no LLM in it at
  all), rule 5 (journal before acting), rule 8 (**never place real orders**: paper only, and the
  crate compiles no other host), rule 12 (every account-level action goes through the account
  ledger; opening orders are limit orders in the regular session; no shorts), rule 13 (risk
  reduction is never denied — which is why a stubbed *exit* path stops the tracer before it opens a
  position rather than being treated as "nothing to do").

## Scope

### The tracer's one job

Place one opening limit order for one liquid US equity, tiny size, on the founder's Alpaca paper
account, through: a validated mandate → stored market data → E4-2's moving-average baseline as the
signal → the order builder's sizing → the risk gate → the runtime's decision cycle with
journal-before-acting → the executor's idempotent intent → the Alpaca paper connector → the journal
record → a reconciliation after a restart.

Everything the tracer needs that does not exist yet is stubbed, and every stub refuses.

### The instrument and the size

**AAPL**, one share, from the research basket (DEC-90). Why this one:

- It is **common stock**, so the eligibility floor has no ETP-class question to answer
  ([trading domain spec §9](../../specs/trading-domain.md#9-risk-gate), `EtpClass`), and the tracer
  exercises the ordinary path rather than the interesting one.
- It is above 1.00 USD, so the tick rule is the single Reg NMS increment of 0.01 and the collar
  arithmetic has one case.
- It is already in the basket, so `mandate download` fetches its daily bars with the keys and the
  code that exist, and E2-4's split handling already covers its history.
- **One share** is the smallest size that can be filled at all. The mandate fixture's allocation and
  per-order notional cap are chosen so that the order builder's `conviction_linear` sizing clips to
  exactly one share at the fixture's price, and the fixture is **recomputed from the rules, not
  typed**: the tests PR asserts the share count with a hand-calculated test and the mandate passes
  `mandate_spec::validate` with no violations before any other test runs.

The instrument and the size are internal test data. They are not an instrument recommendation and
not a product choice (AGENTS.md rule 11, DEC-90's own wording).

### Stream boundaries

| This stream owns | This stream does not touch |
|---|---|
| `mandate-shell`: the process shell, the stage adapters, the effect runner, the paper-host guard, the `mandate-tracer` binary, and the fail-closed and end-to-end tests | Every crate it binds. No sizing, no gating, no pricing, no state machine, no accounting, and no schema lives in `mandate-shell` |
| Its own rows in `xtask/layers.toml`, `CODEOWNERS`, the decision log, the backlog, the tracker, and the feature map | Any other coordinator's rows in those files |
| Its own fixtures under `crates/mandate-shell/tests/fixtures/` | `fixtures/refcases/`, `docs/specs/`, `schemas/`, `reference/`, and `crates/mandate-refcases/status.toml` — the tracer moves no reference case |

### Invariants touched

The properties the tracer must always hold. Every one is a test (see "The verification condition"),
and every one is re-checked after any change to the stage list, not only after the change that
prompted a finding.

| # | Invariant |
|---|---|
| TI-1 | No order reaches the connector unless a `Journal` effect recording its intent was appended and answered `Committed` or `AlreadyCommitted` first, in the same run, with the same `client_order_id` |
| TI-2 | If **any** stage of the path returns its crate's `Unimplemented` error, no order reaches the connector and no `OrderSubmitted` is journaled |
| TI-3 | No adapter in the shell can map a failure, an absence, an ambiguity, or a timeout to a verdict that permits an order. The mapping functions are total and have no permitting arm |
| TI-4 | A stubbed **risk-reducing** stage (the flatten planner, the protective sequence) stops the tracer **before** it opens a position, rather than producing an empty plan. The tracer never opens what it cannot exit |
| TI-5 | Every request the connector makes goes to `mandate_alpaca::http::PAPER_HOST` and passes `mandate_alpaca::http::is_paper_trading_path`. No other host appears anywhere in `mandate-shell`, and no environment variable, argument, or configuration file can introduce one |
| TI-6 | A restart at any point sends no second order for the same intent: the `client_order_id` is a pure function of the intent's `event_id`, so the broker answers `DuplicateClientOrderId` rather than accepting a twin |
| TI-7 | Every journaled draft carries `environment = paper`. A draft with any other environment is a refusal, not a warning |
| TI-8 | No credential, account number, or account id appears in any journal draft, log line, error message, fixture, or file the tracer writes |
| TI-9 | `Autonomy::Ask` and `Autonomy::Deny` send no order. The tracer has no escalation, so ask resolves to the safe default (skip), journaled |
| TI-10 | The tracer is deterministic: the same fixtures, mandate, and injected clock produce byte-identical journal drafts |

### Oracles

Each property test computes its expectation its own way, and each oracle is shown to fail on a
seeded bug before it is trusted ([AGENTS.md](../../../AGENTS.md) "Independent oracles").

| Oracle | What it counts, independently of the code under test | Catches |
|---|---|---|
| **Submission counter in the transport** | The scripted transport itself counts every `POST /v2/orders` it is handed, before `mandate-alpaca` interprets anything. The shell cannot reach it | TI-2, TI-6: a submission the shell believes it did not make |
| **Shadow order book from the drafts' canonical bytes** | Rebuilds the intended order set by parsing the committed drafts with `mandate_journal::Draft::parse`, never from in-memory state | TI-1, TI-10: a draft that does not say what the code did |
| **Journal verifier** | The end-to-end test exports the journal and runs `mandate_journal::verify_events` (E5-4's verification, separate code) over it | TI-1, TI-7: a broken chain, a wrong environment |
| **Host scanner** | Reads `crates/mandate-shell/src/**` as text and asserts the only `https://` literal is a test fixture path, never a host | TI-5 |
| **Stage enumerator** | An exhaustive `match` over the `Stage` enum builds the fail-closed suite's case list, so a new stage fails to compile until it has a case | TI-2, TI-3 |

### Crates

| Crate | Layer | What the tracer does with it |
|---|---|---|
| **`mandate-shell` (new)** | **8**, `safety_critical = true`, `pure = false` | Everything this stream owns |
| `mandate-backtest` | 7 | `Strategy::MovingAverageCrossover(..).signal(&closes)` — E4-2's baseline, the tracer's only signal |
| `mandate-runtime` | 6 | `handle`, `fold`, `Ports`, `IntentSink`, `TimerSource` — the decision cycle |
| `mandate-executor` | 6 | `handle`, `reconcile`, `ClientOrderId::for_intent`, `BrokerConnector` — the intent protocol and the binding gate's caller |
| `mandate-alpaca` | 6 | `TradingClient`, `AlpacaPaperHttp`, `http::PAPER_HOST`, `Credentials::from_env` — the paper connector |
| `mandate-journal-pg` | 6 | The append protocol for the manual paper run |
| `mandate-marketdata` | 6 | `dataset::read_manifest`, `dataset::partition::read` — stored daily bars |
| `mandate-builder` | 5 | Sizing and autonomy classification, behind the shell's `OrderPlan` adapter |
| `mandate-risk` | 4 | `evaluate` behind the shell's `GateDryRun` adapter; the **binding** call stays inside `mandate-executor` |
| `mandate-spec`, `mandate-domain` | 3, 1 | `validate`, `ValidatedMandate` |
| `mandate-journal` | 2 | `Draft::parse`, `verify_events`, `Environment` |
| `mandate-num`, `mandate-time` | 0 | Parsing and display of the fixture's numbers and times. **No arithmetic on money or quantity happens in `mandate-shell`** |

Layer 8 is new, and it is the lowest layer that works: `mandate-backtest` is already at layer 7, a
crate may depend only on strictly lower layers, and the shell must see both `mandate-backtest` (the
signal) and `mandate-executor`, `mandate-runtime`, and `mandate-alpaca` (the order path).
`xtask/layers.toml` already anticipates exactly this move in its own note — "`mandate-cli` (the CLI
moves up when a command needs `mandate-backtest`)" — so the tracer follows the rule the file
already states rather than inventing one. The entry and the CODEOWNERS line are founder-owned:
see Decisions needed 1.

No new external dependency. `clap`, `tokio`, and `thiserror` are already registered in
[docs/dependencies.md](../../dependencies.md); the tests PR confirms each row exists and adds none.

## The path, step by step

One pass of the tracer, in order. "First needed from" names the PR or stream that must merge before
the stage can be real; until then the stage is a stub and the path sends no order.

| # | Step | Crate and function | First needed from |
|---|---|---|---|
| 1 | Load the mandate fixture and validate it: one instrument, tiny size, paper environment | `mandate_spec::validate(&Mandate, &ValidationContext) -> Result<ValidationReport, SpecError>`, then `ValidatedMandate::new` | stream **F** (tests PRs merged for S, V, P; the implementation PR for family V) |
| 2 | Read the stored daily bars for AAPL and check what they cover | `mandate_marketdata::dataset::read_manifest(dir)`, `mandate_marketdata::dataset::partition::read(path, Kind::Bars)` | merged (E2-1, E2-2) |
| 3 | The signal at the last period's close | `mandate_backtest::Strategy::MovingAverageCrossover(StrategyConfig { fast_periods: 5, slow_periods: 20, collar, target_notional }).signal(&closes) -> Result<Signal, BacktestError>` | **#163** (E4-2 implementation) |
| 4 | Wrap the signal as a signal-model output with an explicit expiry, and feed it in | the shell builds `mandate_runtime::ModelOutput { model: "ma-crossover", version, instrument, as_of, expires_at, content }` and calls `handle` with `Input::ModelOutput(..)` | this stream |
| 5 | Size the order and classify its autonomy | `mandate_runtime::OrderPlan::plan(&MandateView, &SignalInputs) -> Option<Proposal>` and `classify(..) -> Autonomy`, the shell's adapter over `mandate-builder` | stream **H** (tests, then implementation) |
| 6 | The gate's **advisory** pass inside the runtime, which can only narrow | `mandate_runtime::GateDryRun::check(&Proposal) -> DryRunVerdict`, the shell's adapter over `mandate_risk::evaluate(&GateInput) -> Result<Decision, GateError>` | **#157**, **#160**, and E6-3's third PR |
| 7 | The decision cycle: one `handle` call returns an ordered effect list; the runner appends each `Effect::Journal` and hands `Effect::Intent` **only after** its draft answered `Committed` or `AlreadyCommitted` | `mandate_runtime::handle(&mut RuntimeState, Input, &Ports) -> Result<Vec<Effect>, RuntimeError>`; the runner is the shell's | merged (**#151**) |
| 8 | Append the agent-stream drafts | the shell's `JournalWriter` port: `mandate-journal-pg` for the manual run, an in-crate writer that answers the same append protocol for CI; drafts validated with `mandate_journal::Draft::parse` | merged (E5-1, E5-3) |
| 9 | Hand the intent across | the shell's `IntentSink` adapter: `mandate_runtime::IntentSink::hand(&IntentHandoff)` → `mandate_executor::Input::Intent(handoff)` | **#152**, then stream K's implementation |
| 10 | The **binding** gate, on fresh folded account state | `mandate_executor::gate::decide(..)` — crate-private inside the executor, a direct dependency on `mandate-risk`, not injected and not replaceable (AGENTS.md rules 1 and 12) | stream **K** implementation, with **#157**/**#160** |
| 11 | The idempotency key, derived from journaled facts alone | `mandate_executor::ClientOrderId::for_intent(&IntentId) -> Result<ClientOrderId, ExecutorError>` | **#152** |
| 12 | Journal `OrderSubmitted`, then request the submission | `mandate_executor::handle(..) -> Result<Vec<Effect>, ExecutorError>` returning `Effect::Journal(OrderSubmitted)` before `Effect::Broker(BrokerRequest::Submit(..))`; the shell's runner preserves that order and stops at the first append that is neither `Committed` nor `AlreadyCommitted` | **#152**, then stream K implementation |
| 13 | The paper connector sends it | `mandate_alpaca::TradingClient::new(transport, pause, retry)` implementing `mandate_executor::BrokerConnector::call(&BrokerRequest)`. CI: the scripted transport over recorded fixtures. Manual run: `mandate_alpaca::AlpacaPaperHttp::new(Credentials::from_env()?)`, whose only host is `http::PAPER_HOST` | **#152**, then stream K implementation |
| 14 | Fold the answer | `Input::Broker(Ok(BrokerOutcome::Submitted(order)))` → `OrderAcknowledged`; `DuplicateClientOrderId` folds as already submitted; `Err(BrokerUnknown)` **queries and never resubmits** | stream **K** implementation |
| 15 | The journal record, verified | the test exports both streams and runs `mandate_journal::verify_events` with a `TrustedStart` over them | merged (E5-1, E5-4) |
| 16 | Restart and reconcile | `Input::Started(epoch)` on both cores, `mandate_runtime::fold` replaying the agent stream, then `mandate_executor::reconcile(&state, &snapshot, &ports)`; a mismatch pauses and alerts and nothing in the tracer lifts it | stream **K** implementation |

Steps 1, 3, 5, 6, 10, and 13 are the ones that are stubs today. Steps 2, 7, 8, and 15 are real now,
which is why the tracer can land immediately and be useful immediately.

## The verification condition: fail-closed wiring

The founder's condition was **no regression in verification**. Read positively, that is this
stream's whole design: the tracer adds checks and removes none.

### What fail-closed means for each stage

Every stage that can **add** risk maps every non-answer to "no order". Every stage that can
**reduce** risk maps every non-answer to "do not start", because an empty exit plan is not a safe
default — it is an unprotected position (AGENTS.md rule 13, TI-4).

| Stage | Its failure | The shell's mapping | Consequence |
|---|---|---|---|
| Mandate validation | `Err(SpecError::Unimplemented)`, or a report with any violation | refuse to start, exit code `mandate_not_validated` | no order |
| Market data | missing partition, gap, or untrusted coverage | refuse to start, `market_data_untrusted` | no order |
| Signal | `Err(BacktestError::Unimplemented)`; also `Signal::Undecided` and `Signal::Flat` while flat | no model output is produced | no order |
| Order builder — sizing | `Unimplemented` | `OrderPlan::plan` returns `None` | no proposal, so no order |
| Order builder — classification | `Unimplemented` | `classify` returns `Autonomy::Deny` | no order |
| Risk gate, advisory | `Err(GateError::Unimplemented)`, or `Verdict::Deny` | `DryRunVerdict::Deny { reason_code }`, the code carried through | no order |
| Risk gate, binding | `Err(ExecutorError::Unimplemented)` | the executor emits no `Effect::Broker` | no order |
| Journal append | anything but `Committed` / `AlreadyCommitted` | the runner stops and discards the rest of the effect list | no order (this is rule 5, mechanically) |
| Flatten planner | `Unimplemented` | **refuse to start**, `exit_path_unavailable` | the tracer never opens a position it cannot flatten |
| Protective sequence | `Unimplemented` | **refuse to start**, `protection_unavailable` | same |
| Connector transport | `Err(BrokerUnknown)` | query by `client_order_id`; one absence never resubmits | no duplicate |
| Host guard | any host but `PAPER_HOST`, any path failing `is_paper_trading_path` | refuse **before** the first request, `non_paper_host` | nothing is sent at all |

### How the mapping is made unrepresentable, not remembered

Rungs 1 and 2 of the [trust ladder](../../../AGENTS.md#the-trust-ladder), because a convention a
reviewer checks is exactly what this stream exists to replace:

1. Each mapping is a **total function with no permitting arm**: `fn from(e: GateError) -> DryRunVerdict`
   constructs only `Deny`, and the type `DryRunVerdict` has no `Default`. There is no `_ =>` arm and
   no `unwrap_or`, `unwrap_or_default`, `unwrap_or_else`, or `ok()` anywhere in
   `crates/mandate-shell/src/`; a clippy lint and a source scan test hold that.
2. The stage list is an enum, and the fail-closed suite's cases are built by an **exhaustive match**
   over it. Adding a stage without a fail-closed case does not compile.
3. `mandate-shell` is `safety_critical = true`: the lint header, a CODEOWNERS line, an enumerated
   `allowed_external`, and zero missed mutants on the diff.

### The test that proves it

`crates/mandate-shell/tests/fail_closed.rs`, one case per `Stage`, generated from the exhaustive
match:

```
for stage in Stage::ALL:
    build the tracer with every other stage as it is today,
      and this one stage replaced by a stub returning its crate's Unimplemented error
    run the whole path against the recorded happy-path fixtures
    assert transport.submissions() == 0        # the independent oracle
    assert no committed draft has event_type "OrderSubmitted"
    assert the run's error code == stage.expected_code()
    assert the last committed draft names the stage
```

Two more cases sit beside the per-stage ones:

- **All stubs at once** — the state the repository is in today — asserts the same three things.
- **No stubs** — the happy path, which asserts exactly **one** submission, the journal order of
  TI-1, and the golden journal byte for byte. It carries `#[ignore = "pending E7-7"]` until every
  upstream stage is real, which also satisfies the pending-tests gate (DEC-110): with any stage
  stubbed the test *must* fail, because the tracer *must* send no order.

And one property test for TI-3: for every value of each source error type, and for every `Verdict`
and `Decision` the gate can return, the mapped verdict is never permitting. It is the mapping
functions under test, so it holds even for errors no fixture produces.

### What "no regression" also rules out

- No test is skipped, disabled, or weakened. The only `#[ignore]` is the DEC-77 pending marker with
  the story id, which `cargo xtask ci pending` proves fails on the PR's code.
- No reference case moves. `crates/mandate-refcases/status.toml` is untouched by every PR of this
  stream.
- No existing crate loses a test, an assertion, or a lint. The tracer reads them; it replaces none.
- The CI jobs keep their shape: the new tests run inside `cargo xtask ci fast`'s `test` step; the
  end-to-end test needs no Postgres and no network, so `full` gains nothing that could be flaky.
- No new external dependency, so `docs/dependencies.md` is unchanged.
- The fail-closed suite is itself verified by the planted bugs below; an oracle that cannot fail is
  not an oracle.

## Where the wiring lives

**A new layer-8 crate, `mandate-shell`, with a `mandate-tracer` binary — not a `mandate-cli`
subcommand.** Four reasons, and the first is decisive:

1. `mandate-cli` is `safety_critical = false`. The order path is safety-critical. Putting the tracer
   in the CLI means either an unmarked safety-critical path (a regression in verification, which the
   founder's condition forbids) or making the whole CLI safety-critical — which drags `download`,
   `inspect`, `journal verify`, and `artifact` under the safety-critical lint header and the mutation
   gate. Neither is acceptable; a separate crate is.
2. The audit tool should not hold the paper credentials or reach the order path. An operator runs
   `mandate journal verify` to check what happened; that binary having a code path that can place an
   order is the wrong shape.
3. `mandate-cli` sits at layer 7 and would have to move to 8 anyway to see `mandate-backtest`, so
   nothing is saved by reusing it.
4. `mandate-shell` is where the shell has to live regardless. Stream K's merged brief and the
   tracker already say "the adapter implementing stream I's `IntentSink` lives in the layer-7
   shell" and "the shell that binds runtime, executor, and connector is not here". This stream is
   that shell, at the layer the dependency graph actually allows. M7's soak harness and M9's
   deployment both need it, so it is not throwaway code.

`mandate-tracer` is a binary inside `mandate-shell`, not its own crate: the library holds the shell
and the adapters, the binary holds argument parsing and the host guard, and the tests exercise the
library.

### The CI end-to-end test (never the network)

`crates/mandate-shell/tests/tracer.rs`, over fixtures under
`crates/mandate-shell/tests/fixtures/tracer/`:

- **Alpaca paper scenarios** in the shape stream K already established for `mandate-alpaca`
  (`requests.txt` with `<METHOD> <path and query>[ <canonical body>]` per line, `statuses.txt`,
  `response-<n>.json` byte for byte), through the scripted `TradingTransport`. `AlpacaPaperHttp` is
  never constructed in a test, so no test can reach a network even by mistake (ES-19).
- **A one-symbol bar dataset** written by `mandate-marketdata`'s own writer at fixture-build time,
  so the tracer reads the real format rather than a hand-typed imitation.
- **The mandate fixture**, which the test validates with `mandate_spec::validate` and asserts has
  zero violations *before* it is used, so a fixture that breaks its own rules fails loudly
  ([AGENTS.md](../../../AGENTS.md) "Validate fixtures against the rules").
- **An injected clock and an injected `IdGen`**, so the run is deterministic and the golden journal
  is byte-stable (ES-21, TI-10).
- Scenarios: `happy` (one order, acknowledged), `gate_denies`, `autonomy_ask`, `signal_flat`,
  `signal_undecided`, `duplicate_after_restart`, `broker_unknown_then_absent`, and
  `reconcile_mismatch_pauses`.

### The manual paper run

```
cargo run -p mandate-shell --bin mandate-tracer -- \
  --mandate <path> --dataset <dir> --journal <dsn> --confirm-paper [--place-one-order]
```

- **Nothing is sent without `--place-one-order`.** The default runs every stage, journals the
  decision, and stops before the submission, printing the order it *would* place. That is the safe
  default (rule 3): the flag is the only way to reach a broker.
- `--confirm-paper` is a second, separate acknowledgement; both flags are required to submit.
- Credentials come from `mandate_alpaca::Credentials::from_env()`, reading
  `MANDATE_ALPACA_PAPER_KEY_ID` and `MANDATE_ALPACA_PAPER_SECRET`, held as `SecretString`, sent only
  as headers marked sensitive. They are never printed, logged, journaled, written to a fixture, or
  committed (rule 7, TI-8), and ES-09's log scan covers the new crate.
- **The non-paper refusal, three layers deep:**
  1. `mandate-alpaca` compiles no host but `PAPER_HOST`, has no `live` feature, and has no deposit,
     withdrawal, or transfer endpoint. `mandate-shell` adds no host of its own.
  2. Before the first request, the binary asserts the transport's base equals `PAPER_HOST` by **exact
     string equality** — never `starts_with`, `contains`, or a suffix match — and asserts every
     planned path passes `is_paper_trading_path`. A failure is exit code `non_paper_host` and nothing
     is sent.
  3. There is **no host override**: no `--host` argument, no host environment variable, no config
     file. Any environment variable whose name contains `ALPACA` and whose value looks like a URL is
     itself a refusal, so an operator who tries to point the tracer elsewhere gets a stop, not a
     silent redirect.
- The mandate's environment and every journal draft's `Environment` must be `Paper`; anything else
  refuses (TI-7).
- A cloud routine can run the same command unattended once the founder puts the paper keys in the
  environment and says go (the tracker's founder row already carries that request).

## The DEC-77 sequence

| Stage | Branch | Contents |
|---|---|---|
| 1. **Brief** (this PR) | `agent/tracer-brief` | This document, the DEC-137 row and its Reserved-identifiers row, the E7-7 backlog story, the tracker's stream L row and Claims row, and the feature-map entry. No `crates/`, `schemas/`, `docs/specs/`, `reference/`, `fixtures/`, or `Cargo.*`, so CI takes the documentation-only short path (DEC-112) |
| 2. **Tests** | `agent/tracer-tests` | The `mandate-shell` skeleton (safety-critical lint header, module docs, every adapter and the effect runner as stubs returning `ShellError::Unimplemented`), the `xtask/layers.toml` entry and the CODEOWNERS line, all fixtures, the fail-closed suite, the property tests, the host-refusal tests, the golden journal, every test that cannot pass yet `#[ignore = "pending E7-7"]`, green `cargo xtask check` including the pending gate, and the planted-bug report in the PR body |
| 3. **Implementation** | `agent/tracer-impl` | The adapters filled in, on the coordinator's signal, once the upstream streams have landed. Test files change **only** by deleting `#[ignore = "pending E7-7"]` lines |

There is **no status PR**: this story moves no reference case.

Unlike the other streams, the tests PR here is useful on its own, because the state it asserts — all
stages stubbed, zero orders — is the state the repository is actually in. The implementation PR
follows each upstream merge; the coordinator may split it per stage, one flipped adapter at a time,
which is the cheapest way to find out which merge broke the integration.

### Reference cases

The tracer introduces none and moves none. It cites these, read-only, as the cases that already
cover the behaviour each stage owes it, so that a tracer failure can be traced to the case that
should have caught it:

| Case | What it already covers for the tracer |
|---|---|
| `trading_domain::RC-04` | Order submission through the ledger |
| `trading_domain::RC-11` | The order lifecycle and the broker status mapping |
| `trading_domain::RC-14` | The kill switch and the exit sequences the tracer refuses to open without |
| `trading_domain::RC-17` | One agent per instrument per account |
| `trading_domain::RC-09`, `RC-09B` | Day-trading regime and market hours at the gate |
| `trading_domain::RC-16` | The eligibility floor for the tracer's instrument |
| `mandate::MC-G*` | The gate over the working universe |
| `mandate::MC-A*`, `MC-B*` | Autonomy classification, including the `ask` the tracer resolves to skip |
| `journal::*` append and idempotency vectors | The append protocol the effect runner obeys |

If a tracer scenario needs a behaviour none of these covers, that is a finding for the owning
stream's case file, not a new case in this stream (the freeze rule and the ownership rule in the
[coordination playbook](../../../.cursor/skills/mandate-mode/playbooks/coordination.md) §2).

## Planted bugs

Each row is seeded in the implementation, the named test is shown to fail, and the bug is removed.
An oracle that does not fail on its seeded bug is not trusted
([AGENTS.md](../../../AGENTS.md) "Independent oracles"). The tests PR's body carries the table with
its results.

| # | Planted bug | Caught by |
|---|---|---|
| PB-1 | **An order sent before its intent is journaled.** The effect runner sorts effects by variant, or hands `Effect::Intent` before awaiting the append's answer | `tracer::happy` golden-journal ordering assertion, and the TI-1 property: every `Submit` the transport saw is preceded in the same run by a committed draft with the matching `client_order_id`. The shadow order book is rebuilt from the drafts, so in-memory state cannot cover for the missing draft |
| PB-2 | **A gate denial ignored.** The `GateDryRun` adapter maps `Verdict::Deny` to `DryRunVerdict::Allow`, or drops the reason code | `tracer::gate_denies` asserts zero submissions; the TI-3 property fails for every `Deny` value, not just the fixture's |
| PB-3 | **A duplicate order after a restart.** `ClientOrderId` is derived from a fresh id, or from the retry attempt, instead of the intent's `event_id` | `tracer::duplicate_after_restart`: the recorded scenario answers `DuplicateClientOrderId` only for the original id, so a new id produces a second `POST /v2/orders` and the transport's submission counter reads 2 |
| PB-4 | **A live host accepted.** The host guard uses `starts_with` instead of equality, or honours an `ALPACA_HOST` environment variable | `shell::host::refuses_non_paper` feeds `https://api.alpaca.markets` and `https://paper-api.alpaca.markets.evil.example` and asserts `non_paper_host` **before** any request; the host-scanner oracle asserts `PAPER_HOST` is the only host in the crate |
| PB-5 | **An unimplemented stage treated as allow.** One adapter gains `_ => Allow` or `.unwrap_or(Allow)` | `fail_closed::stage[..]` for that stage asserts zero submissions and the stage's error code; the TI-3 property fails for the whole error type |
| PB-6 | **Escalation silently auto-approved.** `Autonomy::Ask` is mapped to `Auto` because there is no escalation to send it to | `tracer::autonomy_ask` asserts zero submissions and one journaled `ApprovalRequested` with no submission following it (TI-9) |
| PB-7 | **`Undecided` treated as `Long`.** The signal adapter treats "not enough periods" as a buy | `tracer::signal_undecided`, with a fixture holding fewer than `slow_periods` closes, asserts zero submissions |
| PB-8 | **A stubbed exit path silently empty.** The `FlattenPlanner` adapter returns an empty `FlattenPlan` on `Unimplemented` instead of refusing to start | `fail_closed::stage[Flatten]` asserts the tracer refused with `exit_path_unavailable` and that **no opening order** was placed (TI-4) |
| PB-9 | **A credential in the record.** The key id reaches an error message or a draft | the log-and-draft scan test (ES-09) over every byte the run writes, with the fixture credentials set to distinctive sentinels |
| PB-10 | **A non-paper environment journaled.** A draft is built with `Environment::Live` | the journal verifier oracle rejects it (TI-7); the append path refuses |
| PB-11 | **A second submission after `Unknown`.** `Err(BrokerUnknown)` resubmits instead of querying | `tracer::broker_unknown_then_absent` asserts one submission and one query, and that a single `Absent` does not resubmit |
| PB-12 | **A reconciliation mismatch written away.** The shell resumes after a position mismatch | `tracer::reconcile_mismatch_pauses` asserts the agent is paused, the alert is emitted, and nothing lifts it |

## Decisions needed

Interpretations are recorded as **DEC-137**, Accepted (agent, under DEC-79): they are reversible
engineering choices and none touches live money, spending, legal text, or a safety invariant. The
founder may veto any of them after the fact.

Two items need the founder, neither of which is a new decision about spending or live trading:

1. **`xtask/layers.toml` and `CODEOWNERS` are founder-owned.** The tests PR adds
   `[crates.mandate-shell]` with `layer = 8`, `safety_critical = true`, `pure = false`,
   `allowed_external = ["clap", "thiserror", "tokio"]`, adds an `8  mandate-shell` line to the
   planned-crates comment, and adds the CODEOWNERS line. Layer 8 is new to the file. The founder may
   veto the layer, the name, or the external list.
2. **The paper keys and the go for an unattended cloud run.** Already on the tracker's founder row:
   `MANDATE_ALPACA_PAPER_KEY_ID` and `MANDATE_ALPACA_PAPER_SECRET` in the cloud environment, plus
   egress to `paper-api.alpaca.markets`. Nothing in this stream needs them to land: CI runs on
   recorded fixtures, and the manual run is the founder's to start. **No live credential, no live
   host, no spending, and no real order is involved** (rule 8), so there is nothing here for DEC-79
   to reserve.

Two questions for the merge coordinator, not the founder:

3. **Whether the implementation PR is split per stage.** One PR per flipped adapter finds the
   breaking merge faster; one PR is less queue traffic. The coordinator's go comment decides.
4. **Whether `mandate-shell` also becomes M7's soak harness now or later.** This brief builds it so
   it can, and scopes it so it does not have to.

## Not done

Deliberate omissions. Each is somebody else's story, and the tracer's fail-closed wiring is what
makes leaving it out safe rather than merely incomplete.

| Left out | Whose it is | What the tracer does instead |
|---|---|---|
| **Escalation** — approval requests, deadlines, channels, drift re-validation, two approvers | M7, E8-1 onward, [HLD §6.C](../../HLD.md) | `Autonomy::Ask` journals the request and applies the safe default (skip). Nothing is asked and nothing waits (TI-9) |
| **The research agent's live theses** — ideation, corroboration, admission into a working universe | E17-3 and the DEC-103 thin slice, stream J | The universe is pinned by the mandate to one instrument, as bring-your-own-strategy pins it (AGENTS.md rule 11). `mandate-research` is not in the path and no LLM is called anywhere (rule 4) |
| **The web app** | M9, E11, and W1/W2's designs; `web/` code stays at M9 per ES-01 | A binary with two required flags. No UI, no API, no server |
| **Live trading, OAuth, and fund movement** | E7-1 is M8; the vault is M13 | Paper host only, keys from the environment, no transfer endpoint compiled in (rule 8) |
| **Multi-instrument, multi-agent, crypto, and shorts** | E7-5, E16, and the v1 policy | One equity, one agent, one order, long only (rule 12) |
| **Cross-process messaging** (Postgres `LISTEN`/`NOTIFY`, journal tailing between processes) | DEC-17, M6 follow-ups | One process, the in-process `IntentSink` and `TimerSource` of ES-20 |
| **Scorecards, forward-paper evaluation, and metrics reports** | E15-3, E17-8, E4-2's report | Nothing. The tracer produces no evidence of strategy quality and claims none; a single paper order is not evidence (DEC-99) |
| **New protective or kill-switch logic** | E7-4 and E6-5, already merged or in stream K | It uses the executor's sequencing unchanged, and **refuses to open** if that path is stubbed (TI-4) |
| **The soak** — unattended running, forced restarts, continuous escalation | M7 and the Phase 1 exit | One pass, plus one restart to prove reconciliation and no duplicate |
| **Recording fixtures against the live paper host in CI** | a founder-run step outside CI | Fixtures are committed; CI replays them and touches no network |

## Commands

```
cargo xtask ci lint
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
```

For the tests and implementation PRs:

```
cargo xtask check
cargo nextest run -p mandate-shell
cargo nextest run -p mandate-shell --include-ignored          # the pending cases must fail
cargo xtask ci pending
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
```

The manual paper run, for the founder or a cloud routine with the paper keys in the environment:

```
cargo run -p mandate-shell --bin mandate-tracer -- \
  --mandate fixtures/tracer/mandate.json --dataset data/alpaca/bars/1d \
  --journal "$MANDATE_JOURNAL_DSN" --confirm-paper                      # plans, sends nothing
cargo run -p mandate-shell --bin mandate-tracer -- ... --confirm-paper --place-one-order
mandate journal verify --stream <agent-stream> --trusted-start <file>   # the record, after
```

## Stop conditions

Stop and ask the coordinator rather than working around any of these:

- A stage cannot be made to refuse without changing a merged crate. The change belongs to that
  crate's stream, not here.
- The fail-closed suite needs a stage that is not in the `Stage` enum, meaning the path has a step
  this brief did not name. Update the brief first; the enum is the contract.
- A recorded fixture cannot be built without a live call. Say so; do not call.
- The founder vetoes layer 8 or the crate name.
- `mandate-shell` starts to need arithmetic on money or quantity, or a decision of its own. That is
  the signal that logic is leaking into the shell, which is the one thing this crate must not hold.

## Definition of done

The brief PR is done when: this document, the DEC-137 row and its Reserved-identifiers row, the
E7-7 backlog story, the tracker's stream L and Claims rows, and the feature-map entry are in;
`cargo xtask ci lint` and the spec guard are green; no `crates/`, `schemas/`, `docs/specs/`,
`reference/`, `fixtures/`, or `Cargo.*` path is touched; and an independent review on a different
model has passed it.

The **story** is done when every one of these holds on `main`:

1. The path of "The path, step by step" runs end to end in CI against recorded Alpaca paper
   fixtures, with no network, and places exactly one order.
2. Every invariant TI-1 to TI-10 has a passing test, and every oracle has been shown to fail on its
   seeded bug.
3. `fail_closed.rs` covers every `Stage` and all stages at once, and each case asserts zero
   submissions, no `OrderSubmitted`, and the stage's error code.
4. The manual paper run has placed one order on the founder's paper account, the journal verifies,
   and a restart placed no second order.
5. `cargo xtask check` is green, the diff has zero missed mutants, no test is skipped or weakened,
   no reference case moved, and `docs/dependencies.md` is unchanged.
6. An independent review agent on a different model has passed the diff against this brief, the
   specs, and `AGENTS.md` (DEC-79).
