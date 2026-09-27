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
- The fixture **lists `ma-crossover` among its envelope signal models**, with its fixed weight,
  thresholds, and cadence. Signal models are envelope fields the owner confirms (AGENTS.md rule 11),
  so a tracer that fed a model the mandate does not name would be acting outside the envelope even
  though the order itself was within the limits.

The instrument and the size are internal test data. They are not an instrument recommendation and
not a product choice (AGENTS.md rule 11, DEC-90's own wording).

### Stream boundaries

| This stream owns | This stream does not touch |
|---|---|
| `mandate-shell`: the process shell, the stage adapters, the effect runner, the host controls, the `mandate-tracer` binary, and the fail-closed and end-to-end tests | Every crate it binds. No sizing, no gating, no pricing, no state machine, no accounting, and no schema lives in `mandate-shell` |
| Its own rows in `xtask/layers.toml`, `CODEOWNERS`, the decision log, the backlog, the tracker, and the feature map | Any other coordinator's rows in those files |
| Its own fixtures under `crates/mandate-shell/tests/fixtures/` | `fixtures/refcases/`, `docs/specs/`, `schemas/`, `reference/`, and `crates/mandate-refcases/status.toml` — the tracer moves no reference case |

### Invariants touched

The properties the tracer must always hold. Every one is a test (see "The verification condition"),
and every one is re-checked after any change to the stage list, not only after the change that
prompted a finding.

| # | Invariant |
|---|---|
| TI-1 | No order reaches the connector unless a `Journal` effect recording its intent was appended and answered `Committed` or `AlreadyCommitted` first, in the same run, with the same `client_order_id` |
| TI-2 | If **any** stage of the path fails to answer — its crate's `Unimplemented` error, any other error, an absence, or an ambiguity — no order reaches the connector, no intent is handed to the sink, and no `IntentProposed` or `OrderSubmitted` is journaled. The assertion is made at the **furthest boundary the failing stage could have reached**, never only at the connector, because a stubbed executor makes "zero submissions" true of every bug (finding 1) |
| TI-3 | No adapter in the shell can map a failure, an absence, an ambiguity, or a timeout to a verdict that permits an order. The mapping functions are total and have no permitting arm |
| TI-4 | A **risk-reducing** stage that cannot answer (the flatten planner, the protective sequence) stops the tracer **before** it opens a position, and never yields an empty or invented plan. The tracer never opens what it cannot exit. Because `FlattenPlanner::plan` is infallible, this is held by a startup probe and a poisoned adapter, not by a `Result` (finding 2, and Decisions needed 5) |
| TI-5 | Every request goes to `mandate_alpaca::http::PAPER_HOST` and passes `is_paper_trading_path`, because that is the only URL `AlpacaPaperHttp` can build. `mandate-shell` cannot reach any host of its own: no host literal appears in it, and its `allowed_external` has no HTTP client, so it cannot implement `TradingTransport` itself. No environment variable, argument, or configuration file introduces a host |
| TI-6 | A restart at any point sends **zero** further `POST /v2/orders`, and the whole run holds exactly one submission and one `IntentProposed`. `Input::Started` queries and never resubmits (DEC-133 item 2), so the broker's `DuplicateClientOrderId` dedupe is the **backstop**, never the assertion — an assertion that tolerates a second POST cannot see an `IdGen` reset, which reproduces the same id (finding 6) |
| TI-7 | Every journaled draft carries `environment = paper`. A draft with any other environment is a refusal, not a warning |
| TI-8 | No credential, account number, or account id appears in any journal draft, log line, error message, fixture, or file the tracer writes |
| TI-9 | `Autonomy::Ask` and `Autonomy::Deny` send no order. The tracer has no escalation, so ask resolves to the safe default (skip), journaled |
| TI-10 | The tracer is deterministic: the same fixtures, mandate, and injected clock produce byte-identical journal drafts |
| TI-11 | A gate verdict counts only when every check of [trading-domain §9.1](../../specs/trading-domain.md#9-risk-gate)'s order is **enforced by a landed story**. `gate::evaluate` records a check no story owns yet as `CheckOutcome::Passed` and returns `Verdict::Allow` (`mandate-risk/src/gate.rs`, the `_ => Ok(None)` arm on #160), so `Allow` is not evidence on a half-built gate. The shell refuses on any check that passed because it is unowned |
| TI-12 | Running the tracer twice never buys a second share: it refuses when the agent stream already carries an open order or a position in the instrument, unless the operator asks for a new cycle, and a journal that has lost the record of a position the broker holds is a reconciliation mismatch that pauses and alerts |

### Oracles

Each property test computes its expectation its own way, and each oracle is shown to fail on a
seeded bug before it is trusted ([AGENTS.md](../../../AGENTS.md) "Independent oracles").

| Oracle | What it counts, independently of the code under test | Catches |
|---|---|---|
| **Submission counter in the transport** | The scripted transport itself counts every `POST /v2/orders` it is handed, before `mandate-alpaca` interprets anything. The shell cannot reach it | TI-6, and TI-2 for the stages downstream of the sink |
| **Sink counter** | Counts every `IntentSink::hand` call, in the test's own sink. This is the boundary the stages **upstream** of the executor can reach, and it is the only one that means anything while `mandate_executor::handle` is a stub returning `Unimplemented` | TI-2 for steps 1 to 8 (finding 1) |
| **Committed-draft ledger** | Counts committed drafts by `event_type` — `IntentProposed`, `OrderSubmitted` — by parsing bytes with `mandate_journal::Draft::parse`, never from in-memory state | TI-1, TI-2, TI-6, TI-10 |
| **Shadow order book from the drafts' canonical bytes** | Rebuilds the intended order set from the same parsed drafts | TI-1, TI-10: a draft that does not say what the code did |
| **Environment scanner over the drafts** | Parses every committed draft and asserts `Draft::environment() == Environment::Paper`. This is **not** `verify_events`'s job: `columns_match` only checks that the stored column equals the body's field (`mandate-journal/src/verify.rs`), so a consistently `Live` journal verifies (finding 5) | TI-7 |
| **Journal verifier** | The end-to-end test exports the journal and runs `mandate_journal::verify_events` with a `TrustedStart` over it — separate code, for the chain and the artifact references | TI-1: a broken chain |
| **Host scanner** | Reads `crates/mandate-shell/src/**` as text and asserts no host literal appears, and that nothing in the crate implements `TradingTransport` | TI-5 |
| **Stage enumerator** | An exhaustive `match` over the `Stage` enum builds the fail-closed suite's case list, so a new stage fails to compile until it has a case | TI-2, TI-3 |
| **Enforced-check list** | A shell-owned list of the §9.1 checks a landed story enforces, compared against `Decision.checks`; the test fails if the list and the gate disagree | TI-11 (finding 3) |

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
| 2 | Read the stored daily bars for AAPL and check what they cover | `mandate_marketdata::dataset::read_manifest(dir)` and `mandate_marketdata::dataset::read(path, Kind::Bars(Timeframe::Day))` — `dataset::partition` is a private module re-exported as `dataset::read`, and `Kind::Bars` carries a `Timeframe` | merged (E2-1, E2-2) |
| 3 | The signal at the last period's close | `mandate_backtest::Strategy::MovingAverageCrossover(StrategyConfig { fast_periods: 5, slow_periods: 20, collar, target_notional }).signal(&closes) -> Result<Signal, BacktestError>` | **#163** (E4-2 implementation) |
| 4 | Wrap the signal as a signal-model output with an explicit expiry, and feed it in | the shell builds `mandate_runtime::ModelOutput { model: "ma-crossover", version, instrument, as_of, expires_at, content }` and calls `handle` with `Input::ModelOutput(..)` | this stream |
| 5 | Size the order and classify its autonomy | `mandate_runtime::OrderPlan::plan(&MandateView, &SignalInputs) -> Option<Proposal>` and `classify(..) -> Autonomy`, the shell's adapter over `mandate-builder` | stream **H** (tests, then implementation) |
| 6 | The gate's **advisory** pass inside the runtime, which can only narrow | `mandate_runtime::GateDryRun::check(&Proposal) -> DryRunVerdict`, the shell's adapter over `mandate_risk::evaluate(&GateInput) -> Result<Decision, GateError>`. The adapter permits only when every §9.1 check is enforced (TI-11) | **#157**, **#160**, E6-3's third PR, **and E6-6** (`SessionAndHalt`, `DayTradeBudget`), **E6-7** (the eligibility floor inside `UniverseAndLimits`), **E6-8** (`MarkAndCollar`, `ConductControls`) |
| 7 | The decision cycle: one `handle` call returns an ordered effect list; the runner appends each `Effect::Journal` and hands `Effect::Intent` **only after** its draft answered `Committed` or `AlreadyCommitted` | `mandate_runtime::handle(&mut RuntimeState, Input, &Ports) -> Result<Vec<Effect>, RuntimeError>`; the runner is the shell's | merged (**#151**) |
| 8 | Append the agent-stream drafts | the shell's `JournalWriter` port: `mandate-journal-pg` for the manual run, an in-crate writer that answers the same append protocol for CI; drafts validated with `mandate_journal::Draft::parse` | merged (E5-1, E5-3) |
| 9 | Hand the intent across, converting between the **two distinct** `IntentHandoff` types | the shell's `IntentSink` adapter: `mandate_runtime::IntentSink::hand(&mandate_runtime::IntentHandoff)` → `mandate_executor::Input::Intent(mandate_executor::IntentHandoff)`. The runtime's carries `{ intent_id: EventId, body }`; the executor's carries `{ intent_id: IntentId, agent: AgentId, body }`, so the **shell supplies the `AgentId`** and wraps the `EventId` — the one place the two crates are joined, and a conversion the shell owns rather than either crate | **#152**, then stream K's implementation |
| 10 | The **binding** gate, on fresh folded account state | `mandate_executor::gate::decide(..)` — crate-private inside the executor, a direct dependency on `mandate-risk`, not injected and not replaceable (AGENTS.md rules 1 and 12) | stream **K** implementation, with **#157**, **#160**, **E6-6**, **E6-7**, **E6-8** (the same checks step 6 needs; a binding gate on unenforced checks is TI-11's hazard, not a safeguard) |
| 11 | The idempotency key, derived from journaled facts alone | `mandate_executor::ClientOrderId::for_intent(&IntentId) -> Result<ClientOrderId, ExecutorError>` | **#152** |
| 12 | Journal `OrderSubmitted`, then request the submission | `mandate_executor::handle(..) -> Result<Vec<Effect>, ExecutorError>` returning `Effect::Journal(OrderSubmitted)` before `Effect::Broker(BrokerRequest::Submit(..))`; the shell's runner preserves that order and stops at the first append that is neither `Committed` nor `AlreadyCommitted` | **#152**, then stream K implementation |
| 13 | The paper connector sends it | `mandate_alpaca::TradingClient::new(transport, pause, retry)` implementing `mandate_executor::BrokerConnector::call(&BrokerRequest)`, which is **async** (`-> impl Future<Output = Result<BrokerOutcome, BrokerUnknown>>`), so the shell owns the runtime that drives it. CI: the scripted transport over recorded fixtures. Manual run: `mandate_alpaca::AlpacaPaperHttp::new(Credentials::from_env()?)`, which builds every URL as `format!("{PAPER_HOST}{path}")` and holds no base to override | **#152**, then stream K implementation |
| 14 | Fold the answer | `Input::Broker(Ok(BrokerOutcome::Submitted(order)))` → `OrderAcknowledged`; `DuplicateClientOrderId` folds as already submitted; `Err(BrokerUnknown)` **queries and never resubmits** | stream **K** implementation |
| 15 | The journal record, verified | the test exports both streams and runs `mandate_journal::verify_events` with a `TrustedStart` over them | merged (E5-1, E5-4) |
| 16 | Restart and reconcile | `Input::Started(epoch)` on both cores, `mandate_runtime::fold` replaying the agent stream, then `mandate_executor::reconcile(&state, &snapshot, &ports)`; a mismatch pauses and alerts and nothing in the tracer lifts it. `Started` queries and never resubmits, so the bar is **zero** further submissions (TI-6) | stream **K** implementation |
| 17 | **Before any of the above**: probe that the exit path can be planned, and refuse to start if it cannot (TI-4) | `mandate_risk::agent_flatten(..)` once against a synthetic request built from the mandate fixture, and the executor's `protection` sequence, both checked for `Err(Unimplemented)` before the tracer arms anything | `agent_flatten` (**E6-3**, still `Unimplemented("agent_flatten", "E6-3")` on #160) and `mandate_executor::protection` (**E7-4**, still `Unimplemented { story: "E7-4" }` on #152) |

Steps **2, 7, 8, and 15** are real on `main` today. Every other step is a stub or does not exist:
steps **1, 3, 5, 6, 17** are stubs in merged or open crates, and steps **9, 10, 11, 12, 13, 14, 16**
need `mandate-executor` and `mandate-alpaca`, which are **not on `main` at all** — they arrive with
#152. That is why the tracer can land immediately and be useful immediately: four real steps are
enough to prove that the other twelve refuse.

Step 17 is numbered last because it is easiest to read after the path it guards, but it runs
**first**: the tracer probes the exit path before it arms anything, so it can never open a position
whose exit it could not have planned.

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
| Risk gate, advisory | `Err(GateError::Unimplemented)`, or `Verdict::Deny`, or `Verdict::Allow` **with any §9.1 check `Passed` because no story owns it yet** | `DryRunVerdict::Deny { reason_code }`, the code carried through, or `gate_check_unenforced` naming the check | no order. This is the stage's real hazard: `gate::evaluate`'s `_ => Ok(None)` arm passes an unowned check, so `Allow` is not evidence (TI-11) |
| Risk gate, binding | `Err(ExecutorError::Unimplemented)` | the executor emits no `Effect::Broker` | no order |
| Journal append | anything but `Committed` / `AlreadyCommitted` | the runner stops and discards the rest of the effect list | no order (this is rule 5, mechanically) |
| Flatten planner, at startup | `mandate_risk::agent_flatten` answers `Err(Unimplemented)` to the probe | **refuse to start**, `exit_path_unavailable` | the tracer never opens a position it cannot flatten |
| Flatten planner, mid-run | `FlattenPlanner::plan` is **infallible** (`mandate-runtime/src/ports.rs`), so the adapter has no `Err` to return | the adapter records the failure, sets a poison flag, and the effect runner **halts and alerts**. It never returns an empty plan and never invents one | the run stops; the position keeps whatever protection already rests at the broker (finding 2) |
| Protective sequence | `Unimplemented` | **refuse to start**, `protection_unavailable` | same |
| Connector transport | `Err(BrokerUnknown)` | query by `client_order_id`; one absence never resubmits | no duplicate |
| Repeat run | the agent stream already carries an open order or a position in the instrument | refuse, `cycle_already_open`, unless the operator passes `--new-cycle` | no second share (TI-12) |

### How the mapping is made unrepresentable, not remembered

Rungs 1 and 2 of the [trust ladder](../../../AGENTS.md#the-trust-ladder), because a convention a
reviewer checks is exactly what this stream exists to replace:

1. Each mapping is a **total function with no permitting arm**: `fn verdict_of(e: &GateError) -> DryRunVerdict`
   constructs only `Deny`. `DryRunVerdict` belongs to `mandate-runtime`, which this stream leaves
   unchanged, so the brief claims nothing about that type's `Default`; a shell test over the mapping
   function holds the property instead. There is no `_ =>` arm and no `unwrap_or`,
   `unwrap_or_default`, `unwrap_or_else`, or `ok()` **anywhere in `crates/mandate-shell/src/`** — the
   whole crate, not only gate and plan results; a clippy lint and a source scan test hold that.
2. The stage list is an enum, and the fail-closed suite's cases are built by an **exhaustive match**
   over it. Adding a stage without a fail-closed case does not compile.
3. `mandate-shell` is `safety_critical = true`: the lint header, a CODEOWNERS line, an enumerated
   `allowed_external`, and zero missed mutants on the diff.
4. **The shell cannot reach a host.** `allowed_external` names no HTTP client — no `reqwest`, no
   `hyper` — so nothing in `mandate-shell` can implement `TradingTransport` or open a connection, and
   `cargo xtask layers` fails the build if that list grows. Every URL is built inside
   `mandate-alpaca` as `format!("{PAPER_HOST}{path}")`, from a client configured `https_only(true)`
   with `redirect::Policy::none()`, and `is_paper_trading_path` runs before anything is sent. The
   brief previously said the binary should compare "the transport's base" to `PAPER_HOST`; there is
   no base to compare — that check would compare a constant with itself (finding 4). What remains
   real is the refusal of any *attempt to configure* a host, below.
5. **A `ValidatedMandate` cannot be forged.** Its only constructor is `ValidatedMandate::new`, so no
   production path reaches the runtime-facing stages without validation. The fail-closed doubles need
   to bypass that, which is why the suite lives in a `#[cfg(test)]` module inside `src/` rather than
   in `tests/`: an integration test is compiled without `cfg(test)`, so a double reachable from
   `tests/` would need a cargo feature, and a feature can be switched on in a build that ships.

### The test that proves it

A `#[cfg(test)]` module in `crates/mandate-shell/src/stages.rs`, one case per `Stage`, generated
from the exhaustive match. The first draft of this brief said each case keeps "every other stage as
it is today" — which would have made the suite **vacuous**: today every other stage is a stub, so
the first stub in path order refuses, no later stage is ever reached, only `Stage::Validate`'s
`expected_code` could match, and `transport.submissions() == 0` is true of every bug because
`mandate_executor::handle` itself returns `Unimplemented` (finding 1). The suite is built the other
way round:

```
for stage in Stage::ALL:
    build the tracer with this one stage stubbed to return its crate's Unimplemented error,
      and EVERY OTHER stage a permissive double at the shell's own stage trait
      (the doubles answer "yes, proceed" with fixture values, so the path
       reaches the stubbed stage and would reach the broker if the stub leaked)
    run the whole path against the recorded happy-path fixtures
    assert sink.hands() == 0                   # the boundary stages 1-8 can reach
    assert drafts.count("IntentProposed") == 0
    assert transport.submissions() == 0        # meaningful for stages 9-16
    assert drafts.count("OrderSubmitted") == 0
    assert the run's error code == stage.expected_code()
    if stage runs after the runtime started:
        assert the last committed draft names the stage
```

**How each case is shown to bite today**, with #152's executor still a stub. A case bites when the
permissive doubles carry the path to the stubbed stage, so the stub is what stops the run and its
error code is what the run reports. The suite therefore asserts the *reason* for the zero, not only
the zero:

| Case | What carries the path to it | What the case would see if the stub leaked |
|---|---|---|
| `Validate` | nothing upstream of it | the double-supplied view reaches the sink: `hands() == 1` |
| `MarketData`, `Signal` | a doubled validation | `hands() == 1` and one `IntentProposed` |
| `Size`, `Classify`, `GateDryRun` | doubled validation, data, signal | `hands() == 1` and one `IntentProposed` |
| `Journal` | doubled stages 1 to 6 | an intent handed with no committed draft — the TI-1 violation |
| `Sink` | doubled stages 1 to 8 | a doubled executor's `Effect::Broker`, so `submissions() == 1` |
| `BindingGate`, `Idempotency`, `Submit`, `Connector` | a **doubled executor**, which is what makes these cases bite while `mandate_executor::handle` returns `Unimplemented` — without the double there is no path past step 9 and the case proves nothing | `submissions() == 1` |
| `Reconcile` | doubled stages 1 to 14, then a restart | a second submission, which TI-6 forbids |
| `FlattenProbe`, `ProtectionProbe` | nothing upstream; they run first | the opening proceeds with no exit plan — PB-8 |

Each row's right-hand column is what the **all-doubles** case actually produces, which is why that
case is the suite's keystone: it is the same harness with nothing stubbed, and it must place exactly
one order. If it cannot, every zero in the table above is meaningless and the suite is the vacuous
one review finding 1 caught.

The last assertion of the loop is conditional because validation, market data, the flatten probe, and
the protection probe all refuse **before any stream exists**, so there is no draft to name them
(finding 10). For those stages the refusal is the process's typed exit code and one line on stderr;
inventing an event type for them would touch `docs/specs/`, which this stream does not.

Three more cases sit beside the per-stage ones:

- **All stubs at once** — the state the repository is in today — asserts the same counters are zero.
- **All doubles, no stub** — every stage permissive. This case asserts exactly **one** hand, one
  `IntentProposed`, one submission, and one `OrderSubmitted`, which is what proves the suite is not
  vacuous: if the harness cannot place an order even with everything permissive, a zero in the other
  cases means nothing.
- **No stubs and no doubles** — the real end-to-end happy path in `tests/tracer.rs`, asserting the
  journal order of TI-1 and the golden journal byte for byte. It carries `#[ignore = "pending E7-7"]`
  until every upstream stage is real.

  Three requirements come from **#172's stricter gate**, which this brief takes as binding because
  #172 merges first:
  1. The pending test's failure output must show a stub marker — `Unimplemented`, `unimplemented`,
     `not implemented`, `implemented yet`, `not yet implemented` — or name its own story, or name an
     error its own crate's stubs return. So the test must **fail by the upstream error propagating**,
     with `ShellError` carrying the source error's `Display` and `code()`, not by an `assert_eq!` on a
     value the shell invented. A bespoke "expected one order, saw none" assertion would fail the gate.
  2. The pending marker must be on a **plain written-out function**, because the gate reads the
     source and a macro-generated marker is never gated. The happy path is therefore written longhand.
     The per-stage fail-closed cases are generated from the exhaustive match, which is allowed
     precisely because none of them is pending: they pass today.
  3. `mandate-shell` is safety-critical **with** a pending test, so under #172 it runs the mutation
     gate rather than being skipped, and only stub bodies are exempt. The tests PR must expect
     `cargo xtask ci mutants` to run on it and to fail on any missed mutant outside a stub body.

And two property tests. For TI-3: for every value of each source error type, and for every `Verdict`
and `Decision` the gate can return, the mapped verdict is never permitting — the mapping functions
are under test, so it holds even for errors no fixture produces. For TI-11: for every subset of
enforced checks, an `Allow` whose `checks` list contains a `Passed` check outside that subset is
refused.

### What "no regression" also rules out

- No test is skipped, disabled, or weakened. The only `#[ignore]` is the DEC-77 pending marker with
  the story id on the end-to-end happy path, which `cargo xtask ci pending` proves fails on the PR's
  code — and its failure must be the upstream `Unimplemented` surfacing, not an assertion on a value
  the shell invented, so that #172's stricter pending gate accepts it. The per-stage fail-closed cases
  carry **no** marker: they pass today, because today every stage really does refuse.
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
`crates/mandate-shell/tests/fixtures/tracer/`. The per-stage fail-closed suite is **not** here: it
lives in a `#[cfg(test)]` module inside `src/`, because its permissive doubles must not be reachable
from a build that ships (see "How the mapping is made unrepresentable" item 5).

- **Alpaca paper scenarios** in the shape stream K already established for `mandate-alpaca`
  (`requests.txt` with `<METHOD> <path and query>[ <canonical body>]` per line, `statuses.txt`,
  `response-<n>.json` byte for byte), through the scripted `TradingTransport`. `AlpacaPaperHttp` is
  never constructed in a test, so no test can reach a network even by mistake (ES-19).
- **A one-symbol bar dataset** written by `mandate-marketdata`'s own writer (`dataset::Store::put_day`)
  at fixture-build time, so the tracer reads the real format through the public
  `dataset::read(path, Kind::Bars(Timeframe::Day))` rather than a hand-typed imitation.
- **The mandate fixture**, which the test validates with `mandate_spec::validate` and asserts has
  zero violations *before* it is used, so a fixture that breaks its own rules fails loudly
  ([AGENTS.md](../../../AGENTS.md) "Validate fixtures against the rules").
- **An injected clock and an injected `IdGen`**, so the run is deterministic and the golden journal
  is byte-stable (ES-21, TI-10).
- Scenarios: `happy` (one order, acknowledged), `gate_denies`, `gate_check_unenforced`,
  `autonomy_ask`, `signal_flat`, `signal_undecided`, `oversized_proposal`, `outlier_close`,
  `duplicate_after_restart`, `fresh_journal_with_broker_position`, `broker_unknown_then_absent`, and
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
- **The non-paper refusal.** The first draft of this brief had the binary compare "the transport's
  base" with `PAPER_HOST`. There is no base: `AlpacaPaperHttp` holds only a `reqwest::Client` and
  credentials, and every URL is `format!("{PAPER_HOST}{path}")`, so that comparison would test a
  constant against itself and `refuses_non_paper` would test a function nothing on the send path
  calls (finding 4). The controls that are real:
  1. **The type.** `mandate-alpaca` compiles no host but `PAPER_HOST`, has no `live` feature, has no
     deposit, withdrawal, or transfer endpoint, builds its client `https_only(true)` with
     `redirect::Policy::none()`, and runs `is_paper_trading_path` before anything is sent.
  2. **The dependency list.** `mandate-shell`'s `allowed_external` names no HTTP client, so the shell
     cannot implement `TradingTransport` or open a connection of its own, and `cargo xtask layers`
     fails if the list grows. This is the rung-1 control that replaces the comparison.
  3. **The refusal of any attempt to configure a host.** There is no `--host` argument, no host
     environment variable, and no config file. Any environment variable whose name contains `ALPACA`
     and whose value looks like a URL is itself a refusal with exit code `non_paper_host`, so an
     operator who tries to point the tracer elsewhere gets a stop rather than a silent redirect.
     This is a real check because it refuses an *input*, not a constant.
  4. **Defence in depth, optional:** refuse a key id without Alpaca's `PK` paper prefix. It proves
     nothing on its own — the host already decides — but it catches an operator pasting a live key.

  Attacks tried against this and blocked: a base-URL environment variable (nothing reads one), an
  HTTP redirect to another host (`Policy::none()`), a path outside the allowlist
  (`TransportError::RefusedPath` before the send), and a live key sent to the paper host (Alpaca
  rejects it).
- The mandate's environment and every journal draft's `Environment` must be `Paper`; anything else
  refuses (TI-7).
- A cloud routine can run the same command unattended once the founder puts the paper keys in the
  environment and says go (the tracker's founder row already carries that request).

## The DEC-77 sequence

| Stage | Branch | Contents |
|---|---|---|
| 1. **Brief** (this PR) | `agent/tracer-brief` | This document, the DEC-138 row and its Reserved-identifiers row, the E7-7 backlog story, the tracker's stream L row and Claims row, and the feature-map entry. No `crates/`, `schemas/`, `docs/specs/`, `reference/`, `fixtures/`, or `Cargo.*`, so CI takes the documentation-only short path (DEC-112) |
| 2. **Tests** | `agent/tracer-tests` | The `mandate-shell` skeleton (safety-critical lint header, module docs, every adapter and the effect runner as stubs returning `ShellError::Unimplemented`), the `xtask/layers.toml` entry and the CODEOWNERS line, all fixtures, the fail-closed suite and its permissive doubles in a `#[cfg(test)]` module inside `src/`, the property tests, the host and environment scanners, the golden journal, green `cargo xtask check` including the pending gate, and the planted-bug report in the PR body. Only the end-to-end happy path carries `#[ignore = "pending E7-7"]`; the fail-closed cases pass on this PR's code, because today every stage really does refuse |
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

## Attack it yourself

Required before review ([AGENTS.md](../../../AGENTS.md) "Attack it yourself"): how each adversary
could exceed intended risk, and what blocks it. The first draft of this brief omitted this section,
which is how review findings 1 to 4 got in.

| Adversary | The attack | What blocks it |
|---|---|---|
| **A careless user** | Runs `--place-one-order` twice and buys two shares; or points it at a fresh journal DSN so the journal forgets the position the broker holds; or runs it during a halt | TI-12: a refusal (`cycle_already_open`) when the agent stream carries an open order or a position, overridable only by an explicit `--new-cycle`; a fresh journal against an existing broker position is a reconciliation mismatch that pauses and alerts, never a clean start; a halt is `SessionAndHalt`'s business once E6-6 lands, and until then TI-11 refuses the whole opening because that check is unenforced |
| **A bad model or a bad builder** | Returns a `Proposal` with `qty` zero, a quantity above the mandate's cap, a limit far from the mark, or the wrong side | The shell refuses an impossible proposal rather than correcting it (PB-14), the advisory gate narrows, and the binding gate inside the executor denies. The shell does no sizing, so it cannot enlarge one |
| **A half-built gate** | Answers `Verdict::Allow` for an opening while four of §9.1's eight checks are `Passed` only because no story owns them — which is what `gate::evaluate` does today | TI-11 and PB-13: the shell's enforced-check list, and a refusal naming the unenforced check. This is the attack the first draft missed entirely, because it assumed a half-built gate would fail with `Unimplemented` rather than allow |
| **A malicious insider** | Adds an HTTP client to the shell to reach a live host; adds a `_ =>` arm that permits; widens `is_paper_trading_path`; puts a credential in a draft | `allowed_external` plus `cargo xtask layers` (rung 1), the source scan and the clippy lint, CODEOWNERS on the founder-owned files, the host scanner, and the credential scan (PB-9). None of these depends on a reviewer noticing |
| **A bad market tick** | One outlier close moves the moving average or the collar, so the tracer opens at a price nothing supports | PB-15: the market-data stage refuses coverage it cannot trust, and mark-and-collar refuses the limit. E4-2's baseline compares window sums without dividing, so one tick cannot round the signal either |
| **The platform itself** | The tracer's single paper order gets read as evidence that the strategy works | The brief says it is not: one order is not forward-paper evidence (DEC-99), the tracer emits no scorecard and no metrics, and its instrument is internal test data, never a recommendation (rule 11, DEC-90) |

## Planted bugs

Each row is seeded in the implementation, the named test is shown to fail, and the bug is removed.
An oracle that does not fail on its seeded bug is not trusted
([AGENTS.md](../../../AGENTS.md) "Independent oracles"). The tests PR's body carries the table with
its results.

| # | Planted bug | Caught by |
|---|---|---|
| PB-1 | **An order sent before its intent is journaled.** The effect runner sorts effects by variant, or hands `Effect::Intent` before awaiting the append's answer | `tracer::happy` golden-journal ordering assertion, and the TI-1 property: every `Submit` the transport saw is preceded in the same run by a committed draft with the matching `client_order_id`. The shadow order book is rebuilt from the drafts, so in-memory state cannot cover for the missing draft |
| PB-2 | **A gate denial ignored.** The `GateDryRun` adapter maps `Verdict::Deny` to `DryRunVerdict::Allow`, or drops the reason code | `tracer::gate_denies` asserts zero submissions; the TI-3 property fails for every `Deny` value, not just the fixture's |
| PB-3 | **A duplicate order after a restart**, planted in the **shell** where this stream can plant it: (a) the restart skips folding the journal before `Input::Started`, so the run forgets the submitted order; (b) the `IdGen` is reset to its seed, which re-derives the *same* `event_id` | `tracer::duplicate_after_restart` asserts **zero** submissions after the restart and exactly one submission and one `IntentProposed` across the whole run. (b) is why the bar is zero rather than "the broker deduped it": a seed reset reproduces the id, so `DuplicateClientOrderId` would hide the bug from any assertion that tolerates a second POST (finding 6). Deriving `ClientOrderId` from a fresh id is `mandate-executor`'s own planted bug, not this stream's |
| PB-4 | **A live host reached**, replanted as a bug that can actually exist: a shell-local `TradingTransport` implementation that sends somewhere else, or `reqwest` added to the shell's `allowed_external` to make one possible | `cargo xtask layers` fails on the `allowed_external` change; the host-scanner oracle fails on the host literal and on any `impl TradingTransport` inside `mandate-shell`. Separately, `shell::host::refuses_configured_host` feeds `ALPACA_HOST=https://api.alpaca.markets` and `https://paper-api.alpaca.markets.evil.example` and asserts `non_paper_host` before any request — a check on an *input*, not on a constant |
| PB-5 | **An unimplemented stage treated as allow.** One adapter gains `_ => Allow` or `.unwrap_or(Allow)` | `fail_closed::stage[..]` for that stage asserts zero submissions and the stage's error code; the TI-3 property fails for the whole error type |
| PB-6 | **Escalation silently auto-approved.** `Autonomy::Ask` is mapped to `Auto` because there is no escalation to send it to | `tracer::autonomy_ask` asserts zero submissions and one journaled `ApprovalRequested` with no submission following it (TI-9) |
| PB-7 | **`Undecided` treated as `Long`.** The signal adapter treats "not enough periods" as a buy | `tracer::signal_undecided`, with a fixture holding fewer than `slow_periods` closes, asserts zero submissions |
| PB-8 | **An exit path silently empty.** Because `FlattenPlanner::plan` is infallible, the adapter has somewhere to hide: it swallows `agent_flatten`'s `Err` and returns an empty `FlattenPlan`, or the binary skips the startup probe | `fail_closed::stage[Flatten]` asserts the tracer refused with `exit_path_unavailable` and that **no opening order** was placed; `shell::flatten_poison_halts` asserts a mid-run failure halts and alerts rather than yielding an empty plan (TI-4) |
| PB-9 | **A credential in the record.** The key id reaches an error message or a draft | the log-and-draft scan test (ES-09) over every byte the run writes, with the fixture credentials set to distinctive sentinels |
| PB-10 | **A non-paper environment journaled.** A draft is built with `Environment::Live` | the **environment scanner** over the parsed drafts. Not `verify_events`: its `columns_match` only checks the stored column against the body's field, so a consistently `Live` journal verifies cleanly (finding 5) |
| PB-11 | **A second submission after `Unknown`.** `Err(BrokerUnknown)` resubmits instead of querying | `tracer::broker_unknown_then_absent` asserts one submission and one query, and that a single `Absent` does not resubmit |
| PB-12 | **A reconciliation mismatch written away.** The shell resumes after a position mismatch | `tracer::reconcile_mismatch_pauses` asserts the agent is paused, the alert is emitted, and nothing lifts it |
| PB-13 | **A gate `Allow` trusted while checks are unenforced.** The adapter permits on `Verdict::Allow` without consulting the enforced-check list, so an opening passes with `SessionAndHalt`, `MarkAndCollar`, `ConductControls` and `DayTradeBudget` merely `Passed` | `fail_closed::gate_check_unenforced` and the TI-11 property. This is the "half-built gate" row of the attack table, and it is not hypothetical: on #160 the gate really does answer `Allow` here |
| PB-14 | **A bad builder trusted.** The shell passes through a `Proposal` with `qty` zero, or above the mandate's per-order cap, instead of refusing before the sink | `shell::proposal_sanity` asserts a refusal, and the binding gate denies it in `tracer::oversized_proposal`. The shell does no sizing, so its job is to refuse an impossible proposal, not to correct it |
| PB-15 | **A bad tick trusted.** One outlier close in the bar fixture drives the collar, so the limit is priced far from the market | `tracer::outlier_close` asserts the mark-and-collar check refuses, and the market-data stage refuses coverage it cannot trust |
| PB-16 | **A careless second run.** `--place-one-order` is run twice, or once against a fresh journal DSN while the broker already holds the position, and buys a second share | `shell::repeat_run_refused` asserts `cycle_already_open` when the agent stream carries an open order or a position; `tracer::fresh_journal_with_broker_position` asserts the startup reconciliation treats it as a mismatch, pauses, and alerts (TI-12) |

## Decisions needed

Interpretations are recorded as **DEC-138**, Accepted (agent, under DEC-79): they are reversible
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

**Settled by the coordinator, recorded here for the record:**

3. **The flatten planner: the probe and the halt, no port change.** `FlattenPlanner::plan` returns
   `FlattenPlan`, not `Result`, so an adapter over `mandate_risk::agent_flatten` cannot report that it
   could not plan (review finding 2). The coordinator ruled for the startup probe plus the poisoned
   adapter over a change to `mandate-runtime`'s port, and this brief takes that. It **can** be made
   safe: the probe runs before the tracer arms anything, so a planner that cannot answer stops the run
   before a position exists, and mid-run the poison flag makes the effect runner halt and alert rather
   than act on a plan nobody computed. What it cannot do is make the failure *unrepresentable*: the
   poison flag is rung-3 enforcement (a rule in this brief and a test), not rung 1, so a later edit
   could forget to check it. `shell::flatten_poison_halts` and PB-8 are what hold it, and a fallible
   `plan` stays available to stream I as an improvement rather than an ask from this stream.

Two questions for the merge coordinator, not the founder, and one item it has already ruled on:

4. **Whether the implementation PR is split per stage.** One PR per flipped adapter finds the
   breaking merge faster; one PR is less queue traffic. The coordinator's go comment decides.
5. **Whether `mandate-shell` also becomes M7's soak harness now or later.** This brief builds it so
   it can, and scopes it so it does not have to.
6. ~~Whether this PR may correct stream K's stale layer-7 wording.~~ **Ruled: yes.** The
   [coordination playbook](../../../.cursor/skills/mandate-mode/playbooks/coordination.md) §4 keeps
   agents out of another stream's rows, so this PR asked first; the coordinator directed the alignment
   here. DEC-138 amends DEC-133 item 1, and the tracker's K rows, the M6-K brief's `IntentSink`
   paragraph, and the feature map's executor entry now say layer 8 with the amendment cited. Nothing
   else of stream K's changes.

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
- A fail-closed case can only be made to pass by weakening its assertion, or the "all doubles, no
  stub" case cannot place an order. Either means the harness, not the path, is what the suite is
  testing — which is the vacuity review finding 1 caught, and it must not come back.
- An upstream crate answers `Allow`, `Passed`, or an empty plan where the brief expected an error.
  Say so: a stage that fails open is a finding for that stream, and the shell's refusal is a
  containment measure, never the fix.

## Definition of done

The brief PR is done when: this document, the DEC-138 row and its Reserved-identifiers row, the
E7-7 backlog story, the tracker's stream L and Claims rows, and the feature-map entry are in;
`cargo xtask ci lint` and the spec guard are green; no `crates/`, `schemas/`, `docs/specs/`,
`reference/`, `fixtures/`, or `Cargo.*` path is touched; and an independent review on a different
model has passed it.

The **story** is done when every one of these holds on `main`:

1. The path of "The path, step by step" runs end to end in CI against recorded Alpaca paper
   fixtures, with no network, and places exactly one order.
2. Every invariant TI-1 to TI-12 has a passing test, and every oracle has been shown to fail on its
   seeded bug.
3. The fail-closed suite covers every `Stage`, all stages stubbed at once, and all stages doubled with
   none stubbed. Each stubbed case asserts zero `IntentSink::hand` calls, zero `IntentProposed` and
   `OrderSubmitted` drafts, zero submissions, and the stage's error code; the all-doubles case places
   exactly one order, which is what proves the suite is not vacuous.
4. The manual paper run has placed one order on the founder's paper account, the journal verifies,
   and a restart placed no second order.
5. `cargo xtask check` is green, the diff has zero missed mutants, no test is skipped or weakened,
   no reference case moved, and `docs/dependencies.md` is unchanged.
6. An independent review agent on a different model has passed the diff against this brief, the
   specs, and `AGENTS.md` (DEC-79).
