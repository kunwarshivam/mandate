# Mandate Spec (v1)

| | |
|---|---|
| **Status** | Draft v0.1: requires founder approval before implementation (safety-critical) |
| **Implements** | PRD 6.3 (FR-3.1 to FR-3.7), 6.5 (FR-5.2 to FR-5.5), 6.6 (FR-6.1); backlog E6, E10 |
| **Schema** | [schemas/mandate.schema.json](../../schemas/mandate.schema.json) (structural rules) |
| **Reference cases** | [reference-cases/mandate.yaml](reference-cases/mandate.yaml) |
| **Related** | [Trading domain spec](trading-domain.md), [journal spec](journal.md), decisions [DEC-39 to DEC-43](../project/04-decision-log.md#decisions) (proposed) |

A **mandate** is the binding specification an agent runs under: its goal, instruments, capital,
behavior, protection, risk limits, autonomy rules, and notifications. It is the contract between
the owner and the agent. This spec defines the mandate's structure, semantic validation, the
policy hierarchy, exact risk-limit definitions, the autonomy rule language, the advisor and signal
interface, the v1 decider, versioning, and how changes are classified.

## 1. Principles

1. **The mandate is the contract** (DEC-03). The agent cannot act outside it; the risk gate enforces
   it independently of agent logic.
2. **The user supplies the judgment** (DEC-38). Instruments, strategy, sizing, and limits come from
   user-confirmed fields. Inferred fields are inactive until confirmed. Platform defaults only
   restrict. Templates carry structure, never instruments or parameter values.
3. **Reducing risk never needs approval** (DEC-05). Risk-reducing actions are always AUTO; no
   mandate can make them ask or deny.
4. **Deterministic and typed.** Every field has a type and a meaning; conditions are structured, not
   free text; the same mandate and inputs always yield the same classification.
5. **Versioned and hashed.** A mandate version is the SHA-256 of its canonical form (journal spec
   §4); deployments pin a version.

## 2. Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft: plain-language description or form
    Draft --> Compiled: compiler produces fields with provenance
    Compiled --> Reviewed: user confirms fields (inferred fields inactive until confirmed)
    Reviewed --> Validated: schema + V-rules + policy hierarchy pass
    Validated --> Versioned: canonical hash = mandate_version
    Versioned --> Deployed: backtest and paper requirements met; owner approves (step-up)
    Deployed --> Superseded: new version applied at a safe point
    Superseded --> [*]
    Deployed --> Retired: agent stopped or goal complete
    Retired --> [*]
```

- **Provenance** is recorded per field in `MandateVersionCreated` (not in the mandate document):
  `source` ∈ `user`, `compiler_inferred`, `template_structure`, `platform_default`; `confirmed` and
  confirming user. A version cannot be deployed while any field sourced `compiler_inferred` is
  unconfirmed (V-020).
- **Deployment** requires the trading spec's go-live conditions (backtest, paper run, owner step-up
  approval) for `environment: live`.
- **Applying a new version** to a running agent happens at a **safe point**: no `Unknown` orders and
  no pending approvals for affected instruments. Positions in instruments removed from the universe
  may only be reduced.

## 3. Structure

The schema defines structure; this table defines meaning. All money is USD; all fractions are
decimal strings in `[0, 1]` (for example `"0.08"` = 8%).

| Path | Meaning |
|---|---|
| `mandate_schema_version` | `1` |
| `name` | Agent name (lowercase, hyphens) |
| `source_text_ref` | Artifact hash of the user's plain-language description, or `null` if authored as a form |
| `environment` | `paper` or `live` |
| `connection_id` | Broker connection the agent trades through |
| `capital.allocation_usd` | The agent's **capital allocation** (§5.1) |
| `goal` | One of `accumulate`, `distribute`, `return_target` (§3.1) |
| `universe.instruments[]` | Instruments the agent may open or increase (1–50), by broker `asset_id` |
| `universe.leveraged_etps_enabled`, `leveraged_etp_disclosure_version` | Opt-in for complex ETPs (trading spec §3.2) and the acknowledged disclosure version |
| `behavior.description` | User's description of the strategy (informational to advisors) |
| `behavior.advisors[]` | Selected advisors with version, parameters (`{key, value}` list), and trust-weight bounds |
| `behavior.cadence` | Scheduled interval (60 s – 1 day) and event sources that trigger evaluation |
| `behavior.decision_threshold` | Minimum combined conviction to act (§8.3) |
| `protection` | Whether entries are protected; stop distance and take-profit distance as fractions of entry price; crypto stop-limit offset |
| `risk.*` | Risk limits (§5) |
| `autonomy.rules[]`, `default` | Autonomy rules (§6) |
| `autonomy.approval` | Timeout, `on_timeout` (always `skip`), approvers, two-approver threshold |
| `notifications` | Channels and quiet hours |

### 3.1 Goals

| Type | Parameters | Behavior | Done when |
|---|---|---|---|
| `accumulate` | instrument, `target_qty`, `max_avg_price` (or null), `max_spend_usd`, `end_date` | Only buys the goal instrument (sells only for risk rules or protection); never buys if the projected average price would exceed `max_avg_price` or spend would exceed `max_spend_usd` | Position ≥ `target_qty`, spend reaches `max_spend_usd`, or `end_date` |
| `distribute` | instrument, `sell_qty`, `min_avg_price` (or null), `end_date` | Only sells the goal instrument; never sells below a projected average of `min_avg_price` | Sold ≥ `sell_qty`, or `end_date` |
| `return_target` | `target_return` (fraction of allocation), `end_date` | Trades the universe per advisors | Agent return ≥ `target_return` (then flattens), or `end_date` |

On done, the agent stops opening, and for `return_target` flattens; it then moves to `Retired`
(journaled `AgentStopped` with reason `goal_complete`).

## 4. Validation

A mandate is valid when it passes the JSON Schema, every V-rule, and the policy hierarchy (§4.2).
Failures return all violated rule codes.

### 4.1 V-rules

| Code | Rule |
|---|---|
| V-001 | `connection_id` belongs to the workspace and matches `environment` (paper or live account) |
| V-002 | Σ allocations of active agents on the account + this allocation ≤ account equity at validation time |
| V-003 | Goal instrument (accumulate, distribute) is in the universe |
| V-004 | Universe instruments pass the eligibility floor at validation time (warning only; the gate enforces at runtime) |
| V-005 | `leveraged_etps_enabled = true` requires `leveraged_etp_disclosure_version` and org policy allowing it |
| V-006 | Deployment would not claim an instrument already claimed on the account (trading spec §7.1) |
| V-007 | Advisor ids and versions exist in the registry; parameters match each advisor's parameter schema; `weight_min ≤ weight_max` |
| V-008 | `protection.enabled` requires `stop_distance`; if any crypto instrument is in the universe, `crypto_stop_limit_offset` is required |
| V-010 | Drawdown ladder rungs have strictly increasing `at`; `factor` is required for `scale_sizes` (in `(0, 1)`) and null otherwise |
| V-011 | The last rung is `flatten_and_pause` with `at = max_drawdown`; no rung's `at` exceeds `max_drawdown` |
| V-012 | `hysteresis < ` the first rung's `at` |
| V-013 | `max_order_usd ≤ max_position_usd ≤ max_gross_exposure_usd ≤ allocation_usd` |
| V-014 | `max_daily_loss > 0` and `max_drawdown > 0` |
| V-020 | No field sourced `compiler_inferred` is unconfirmed |
| V-021 | No rule targets risk-reducing actions: a comparison on `purpose` with `eq` or `in` whose value includes `reduce`, `close`, or `protective` is invalid anywhere in a condition. (Rules never see those actions, because the built-in AUTO runs first, §6.1; such a rule would mislead its reader.) `ne` and `not_in` on those values are allowed |
| V-022 | `autonomy.default = auto` requires the field to be user-sourced and confirmed |
| V-023 | Condition values match the field type (§6.2): enums take listed values; `gt`/`gte`/`lt`/`lte` take decimals and only on decimal fields; `in`/`not_in` take arrays and only on enum and string fields; `confidence` and `drawdown` values are in [0, 1] |
| V-024 | Approvers resolve to at least one user with the approver role |
| V-030 | `end_date`, if set, is after the validation date |

### 4.2 Policy hierarchy

Organization policy → workspace policy → mandate. **A child may only tighten.**

| Kind | Rule |
|---|---|
| Numeric maximums (`max_*`, `allocation_usd`, ladder `at`, `factor`, `target_qty`, `max_spend_usd`) | Child ≤ parent |
| Numeric minimums (`decision_threshold`, `hysteresis`, cadence `interval_s`) | Child ≥ parent |
| Booleans that enable risk (`leveraged_etps_enabled`) | Child may be `true` only if parent is `true` |
| Sets (allowed asset classes, instruments, channels allowed) | Child ⊆ parent |

Each level is checked against every level above it, so a workspace policy looser than its
organization's is itself invalid. A violation reports the path, the violating level and value, and
the nearest level whose limit it breaks (for example, "max_drawdown 0.09 exceeds workspace limit
0.08").

## 5. Risk limits

### 5.1 Agent capital and equity (proposed DEC-39)

- Each agent has a **capital allocation**. Σ allocations on an account ≤ account equity (V-002).
- **Agent equity** E = allocation + agent realized P&L (net of fees) + agent income + agent
  unrealized P&L, where unrealized uses **risk marks** (trading spec §8.2: bid for longs). The agent
  sub-ledger is the account ledger filtered to the agent's orders.
- **Allocation changes** are mandate changes (`capital.allocation_usd`, §9). When the new version is
  applied, the change is added to E, to the high-water mark H, and to the day-start equity E₀, so
  an allocation change is never counted as profit or loss.

### 5.2 Position, exposure, order size, and order count

These limits apply to **opening and increasing orders only**. Risk-reducing, protective, and
kill-switch orders are never denied by them (AGENTS.md rule 13); an exit larger than
`max_order_usd` is submitted whole (exits are sliced only by the market-conduct controls, trading
spec §9.6).

| Limit | Definition | Gate check (trading spec §9.1) | Reason code |
|---|---|---|---|
| Per-instrument position | MV(instrument) + max cost of working opening orders in it + the proposed order ≤ min(`max_position_usd`, `max_position_fraction` × E), and ≤ the account concentration cap | 2 (concentration) | `concentration_limit` |
| Order size | Proposed order's limit price × quantity ≤ `max_order_usd` | 2 | `max_order_size` |
| Agent gross exposure | Σ \|MV\| of the agent's positions + max cost of its working opening orders + the proposed order ≤ min(`max_gross_exposure_usd`, E) | 7 (gross exposure) | `gross_exposure_limit` |
| Orders per day | Orders submitted by the agent in the risk day ≤ `max_orders_per_day` (exit-sequence and kill-switch orders excluded) | 6 (rate limits) | `order_rate_limited` |

### 5.3 Daily loss (proposed DEC-40)

- The **risk day** starts at 00:00 America/New_York. At the start, the runtime records day-start
  equity E₀ (`RiskDayStarted`, §5.5).
- Daily P&L = E − E₀, evaluated on every mark update and fill.
- **Breach:** daily P&L ≤ −`max_daily_loss` × E₀ → agent mode `exits_only` until the next risk day
  (lifted automatically and journaled), owner alerted.

### 5.4 Drawdown and the ladder (proposed DEC-41)

- **High-water mark** H = the maximum E since deployment (or since an owner reset, which requires
  step-up and is journaled), evaluated on every mark update and fill.
- **Drawdown** DD = (H − E) ÷ H.
- **Ladder actions,** triggered when DD ≥ the rung's `at`:

| Action | Effect | Lifts when |
|---|---|---|
| `scale_sizes` | Decider target sizes multiplied by `factor` (multiple active rungs multiply) | DD < `at` − `hysteresis` (automatic, journaled) |
| `exits_only` | Agent mode `exits_only` | Owner acknowledgment with step-up |
| `flatten_and_pause` | The kill-switch procedure (trading spec §5.5) scoped to the agent, ending in agent mode `paused` instead of `stopped` | Owner acknowledgment with step-up |

- `scale_sizes` rungs trigger and lift independently; the size factor is the product of the active
  rungs' factors. A rung that re-triggers is journaled again.
- `exits_only` and `flatten_and_pause` stay in effect after DD recovers, until the owner
  acknowledges. If both the daily-loss limit and a ladder rung apply, the stricter mode holds; the
  next risk day lifts only the daily-loss `exits_only`.

### 5.5 Journal events

The executor computes E, H, DD, and E₀ from the account ledger, so the risk state is part of the
**account stream** fold (journal spec §2):

| Event | When | Payload |
|---|---|---|
| `MandateVersionApplied` | A new mandate version takes effect for the agent (safe point, §2) | agent, old and new version, allocation change |
| `RiskDayStarted` | 00:00 America/New_York, from the scheduler | agent, E₀ |
| `RiskLimitTriggered`, `RiskLimitLifted` | A limit or rung changes state | agent, limit (`max_daily_loss` or `drawdown_ladder[i]`), action, E, H, DD, E₀ |
| `AgentModeApplied` | A risk limit changes the agent mode (`reason: risk_limit` or `risk_limit_lifted`) | agent, from, to, reason; copied by the agent runtime into the agent stream as `AgentModeChanged` |
| `KillSwitchActivated` | `flatten_and_pause` (`scope: agent`, initiator the rung) | as trading spec §5.5 |

## 6. Autonomy rules (proposed DEC-42)

### 6.1 Evaluation

For each proposed action:

1. **Built-in:** if `purpose` ∈ {`reduce`, `close`, `protective`} → **AUTO** (not overridable).
2. Otherwise, evaluate `autonomy.rules` **in order**; the first rule whose condition is true decides
   (`auto`, `ask`, or `deny`).
3. If no rule matches → `autonomy.default`.
4. **ASK** creates an approval request (trading and HLD escalation flow). On timeout the action is
   skipped (`on_timeout: skip`, the only allowed value). Orders above `two_approver_above_usd`
   require two approvers.

Autonomy runs **before** the risk gate: an AUTO or approved action is still subject to every gate
check.

### 6.2 Condition language

Conditions are JSON objects: `{"all": [...]}`, `{"any": [...]}`, `{"not": {...}}`, or a comparison
`{"field", "op", "value"}`.

| Field | Type | Meaning |
|---|---|---|
| `purpose` | enum | `open`, `increase` (risk-increasing); `reduce`, `close`, `protective` |
| `order_usd` | decimal | Limit price × quantity |
| `confidence` | decimal in [0, 1] | The decider's combined, calibrated confidence (§8.3) |
| `instrument` | string | Instrument `asset_id` |
| `session` | enum | `pre_market`, `regular`, `after_hours`, `crypto` |
| `unusual_input` | boolean | The runtime's input-drift detector flagged the inputs as unusual |
| `first_trade_in_instrument` | boolean | The agent has never traded this instrument |
| `drawdown` | decimal | Current DD (§5.4) |
| `daily_pnl_fraction` | decimal | (E − E₀) ÷ E₀ (§5.3) |

Operators: `eq`, `ne`, `gt`, `gte`, `lt`, `lte` (decimals compare numerically), `in`, `not_in`
(arrays). An unknown field or a type mismatch is invalid (V-023).

## 7. Compiler

- Input: plain-language description (stored as an artifact), optional form fields, and templates
  (structure only).
- Output: a mandate plus per-field provenance. Every field the description does not state is either
  a **platform default that only restricts** (for example, conservative risk limits) or marked
  `compiler_inferred` and highlighted for confirmation.
- The compiler is a model invocation (journal `ModelInvocationRecorded`); its output must pass
  validation before review.
- **Quality metric:** compile accuracy on a maintained evaluation set of descriptions with expected
  mandates (PRD metric "mandate compile accuracy").

## 8. Advisors, signals, and the v1 decider

### 8.1 Advisor contract

An advisor is a registered, versioned component with a parameter schema. Types: `quant`
(deterministic, in-process), `fast` (decision models with hard deadlines), `llm` (asynchronous
research). **Advisors never place orders**; they only emit signals.

### 8.2 Signal

| Field | Type | Rule |
|---|---|---|
| `advisor_id`, `advisor_version` | string | Registered |
| `instrument_id` | uuid | In the mandate universe |
| `as_of` | timestamp | Data cut-off used (no look-ahead) |
| `expires_at` | timestamp | After this the signal is ignored |
| `conviction` | decimal in [−1, 1] | Positive = favorable to holding; negative = favorable to exiting (long-only in v1) |
| `confidence` | decimal in [0, 1] | Advisor's confidence (calibrated when calibration is available) |
| `horizon_s` | integer | Intended holding horizon |
| `thesis_ref`, `evidence` | artifact hash; event IDs | Why; which observations |
| `invalidation` | string | What would invalidate the thesis (informational in v1) |

Signals are journaled (`AdvisorOpinionRecorded`). Late or invalid signals are ignored and journaled.

### 8.3 v1 decider (Phase 1)

For each instrument at each evaluation:

1. Take fresh signals (not expired). Each advisor has a trust weight wᵢ within its
   [`weight_min`, `weight_max`], initially the midpoint (calibration may adjust it within bounds
   later).
2. **Combined conviction** c = Σ wᵢ · convictionᵢ · confidenceᵢ ÷ Σ wᵢ; **combined confidence**
   k = Σ wᵢ · confidenceᵢ ÷ Σ wᵢ.
3. If c ≥ `decision_threshold`: target value T = c × cap × ladder factor, where
   cap = min(`max_position_usd`, `max_position_fraction` × E) and the ladder factor is the product
   of active `scale_sizes` factors (1 if none). If c ≤ −`decision_threshold`: T = 0 (exit).
   Otherwise hold.
4. Delta = T − current MV. Buy quantity = truncate(delta ÷ limit price, increment); sell quantity for
   exits = the reduction needed. No action if the order would be below the minimum order size.
5. Goal constraints (§3.1) and protection (trading spec §5.4) apply; the result is a proposed
   action with `purpose`, quantity, limit price (openings: the ask, within the collar), and
   confidence k, which then goes through autonomy (§6) and the risk gate.

## 9. Versioning and change classification (proposed DEC-43)

- `mandate_version` = `sha256:` + SHA-256 of the mandate's canonical JSON (journal spec §4).
  Provenance is not part of the hashed document.
- A change creates a new version. Changes are classified by comparing old and new versions:

| Change | Classification |
|---|---|
| Any numeric maximum, allocation, ladder `at` or `factor`, goal quantity or spend, or `end_date` later | Risk-increasing if larger |
| Any numeric minimum (`decision_threshold`, `hysteresis`, cadence interval) | Risk-increasing if smaller |
| Universe | Risk-increasing if any instrument is added; reducing if only removed |
| `leveraged_etps_enabled` false → true; `protection.enabled` true → false; stop distance larger | Risk-increasing |
| Autonomy | Reducing only if every change is one of: a rule's `then` or the `default` made stricter (auto → ask → deny); one comparison value in an `auto` rule changed so the rule matches less often; or one comparison value in an `ask`/`deny` rule changed so it matches more often, when no later rule and not the default is stricter than it. Anything else (adding, removing, or reordering rules; changing fields or operators) is risk-increasing |
| Advisors (any change, including parameters), approvers, approval timeout, `two_approver_above_usd` (higher or removed), goal type | Risk-increasing |
| Notifications, description | Neutral |
| Any field not listed | Risk-increasing (fail safe) |

- **Risk-increasing changes require step-up authentication** and, for live agents, respect org
  policy; **reducing and neutral changes take effect on owner confirmation.** A mixed change is
  risk-increasing.

## 10. Reference cases

[reference-cases/mandate.yaml](reference-cases/mandate.yaml) holds two base mandates (a crypto
accumulator and a two-stock swing agent), the canonical-form hash vector, and 81 cases that
implementations must reproduce exactly. A case patches a base mandate with an RFC 6902 JSON Patch.

| Family | IDs | Covers |
|---|---|---|
| Schema | MC-S01 to MC-S10 | Structural rejects: `on_timeout`, fractions, canonical decimals, unknown fields, JSON numbers |
| Semantic | MC-V01 to MC-V25 | Every V-rule, boundaries (allocations exactly equal to equity), multiple violations |
| Policy | MC-P01 to MC-P05 | Maximums, minimums, risk-enabling booleans, a workspace looser than its org |
| Risk state | MC-R01 to MC-R03 | Drawdown ladder with hysteresis, allocation change, daily-loss boundary and next-day lift |
| Gate | MC-G01 to MC-G07 | Position cap (dollar and fraction), order size, orders per day, gross exposure, exits exempt |
| Decider | MC-D01 to MC-D08 | Combined conviction and confidence, ladder factor, hold, close, increase, expiry, autonomy |
| Autonomy | MC-A01 to MC-A09 | Built-in AUTO, rule order, threshold boundary, default, two approvers |
| Change | MC-C01 to MC-C14 | Classification of limits, universe, autonomy thresholds, protection, advisors, mixed changes |

## 11. Open questions

1. Evaluation set and target for compile accuracy.
2. Calibration service design (P1) and how weight adjustments are bounded and journaled.
3. Input-drift detector definition for `unusual_input`.
