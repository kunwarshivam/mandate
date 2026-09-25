# Mandate Spec (v1)

| | |
|---|---|
| **Status** | Draft v0.2: requires founder approval before implementation (safety-critical) |
| **Implements** | PRD 6.3 (FR-3.1 to FR-3.7), 6.5 (FR-5.2 to FR-5.5), 6.6 (FR-6.1); backlog E6, E10 |
| **Schemas** | [mandate.schema.json](../../schemas/mandate.schema.json), [policy.schema.json](../../schemas/policy.schema.json) (structural rules) |
| **Reference cases** | [reference-cases/mandate.yaml](reference-cases/mandate.yaml) |
| **Related** | [Trading domain spec](trading-domain.md), [journal spec](journal.md), decisions [DEC-39 to DEC-52](../project/04-decision-log.md#decisions) |

A **mandate** is the binding specification the owner sets for an agent: its goal, instruments,
capital, signal models, sizing, protection, risk limits, autonomy rules, and notifications. This
spec defines its structure, validation, the policy hierarchy, the risk state and its limits, the
autonomy rule language, signal models and the order builder, versioning, change classification,
and the records kept.

## 1. Principles

1. **The mandate is binding** (DEC-03). The agent cannot act outside it; the risk gate enforces it
   independently of agent logic.
2. **The user supplies every judgment** (DEC-38, DEC-45). Instruments, goal, allocation, signal
   models and their parameters and weights, sizing, protection, risk limits, and every `auto` come
   from the user. The compiler only extracts what the user stated; the platform never proposes
   values for judgment fields.
3. **Reducing risk never needs approval** (DEC-05, DEC-48). Risk exits are unconditional.
   Discretionary exits are never denied, but are paced by market-conduct controls.
4. **Losses are bounded.** Daily loss, drawdown, and a lifetime loss floor each have a defined
   response; nothing resets the lifetime floor (DEC-44).
5. **Deterministic.** Every field is typed; conditions are structured; ratios are rounded by fixed
   rules; the same mandate and journaled inputs always give the same result.
6. **Versioned and hashed.** A mandate version is the SHA-256 of its canonical form (journal spec
   §4); deployments pin a version.

## 2. Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Draft: plain-language description or form
    Draft --> Compiled: compiler extracts stated values (judgment fields left blank if unstated)
    Compiled --> Reviewed: user enters and confirms every judgment field
    Reviewed --> Validated: schema + V-rules + policy hierarchy pass; warnings acknowledged
    Validated --> Versioned: canonical hash = mandate_version
    Versioned --> Deployed: backtest and paper requirements met; owner approves (step-up)
    Deployed --> Deployed: new version applied (§2.2)
    Deployed --> Holding: goal complete or end date (§3.1)
    Holding --> Retired: owner releases or closes positions
    Deployed --> Retired: agent stopped, or profit_stop reached and flat
    Retired --> [*]
```

### 2.1 Provenance and confirmation

Provenance is recorded per field (JSON Pointer) in `MandateVersionCreated`, not in the hashed
document:

| `source` | Meaning |
|---|---|
| `user_stated` | Extracted by the compiler from the user's words; the quoted source span is recorded |
| `user_entered` | Entered by the user in the form or YAML editor |
| `template_structure` | Present because a template included the field or rule (templates carry no values) |
| `platform_default` | Filled by the platform (allowed only for the fields in §7) |

Each field also has `confirmed` (the user confirmed it on the review screen). `MandateConfirmed`
binds the version hash to the list of confirmed paths and the rendered confirmation (§10).

### 2.2 Applying a new version

- **Risk-reducing and neutral versions apply immediately.** Pending approvals are canceled
  (`ApprovalCanceled`) and re-proposed under the new version; working opening orders that violate
  the new limits are canceled.
- **Risk-increasing versions apply at a safe point:** the next evaluation with no `Unknown` orders
  for the agent. Pending approvals are canceled when the version applies.
- **Latched limits are never cleared by a version change** (§5.8); `environment` and
  `connection_id` never change (V-031).
- **Allocation changes** follow §5.1 and may be rejected at application.
- **Removed instruments** keep their claim (trading spec §7.1) until the agent is flat in them.
  Until then the instrument is exits-only for the agent: protection stays, risk exits apply, and
  model outputs are still requested so discretionary exits work.
- Application is journaled as `MandateVersionApplied` (result `applied` or `rejected` with reason).

## 3. Structure

The schema defines structure; this table defines meaning. Money is USD. Fraction fields are decimal
strings; the schema gives each field's bounds.

| Path | Meaning |
|---|---|
| `mandate_schema_version` | `1` |
| `name` | Agent name (lowercase, hyphens) |
| `source_text_ref` | Artifact hash of the user's plain-language description, or `null` if authored as a form |
| `environment`, `connection_id` | `paper` or `live`; the broker connection. Immutable across versions |
| `capital.allocation_usd` | The agent's capital allocation A (§5.1) |
| `capital.max_loss_from_allocation` | Lifetime loss floor as a fraction of contributed capital (§5.7) |
| `goal` | `continuous`, `accumulate`, or `profit_stop` (§3.1) |
| `universe.instruments[]` | Instruments the agent may open or increase (1–50), sorted by `asset_id` |
| `universe.leveraged_etps_enabled`, `leveraged_etp_disclosure_version` | Opt-in for complex ETPs (trading spec §3.2) and the accepted disclosure version |
| `behavior.description` | The user's description of the strategy; given to LLM signal models |
| `behavior.signal_models[]` | Selected signal models: id, version, content hash, parameters, fixed weight |
| `behavior.max_output_age_s` | Outputs older than this are ignored (§8.2) |
| `behavior.cadence` | Scheduled interval and the event sources that trigger evaluation |
| `behavior.sizing` | Sizing method, entry and exit thresholds, rebalance band (§8.3) |
| `protection` | Resting protection: stop distance and take-profit distance as fractions of entry price; crypto stop-limit offset as a fraction of the stop price |
| `risk.*` | Limits and timings (§5) |
| `autonomy.rules[]`, `default`, `approval` | Autonomy rules and approval settings (§6) |
| `notifications` | Channels and quiet hours |

**Set-like arrays are sorted and unique** (V-009) so that equal mandates hash equally:
instruments by `asset_id`, signal models by `id`, parameters by `key`, and `event_sources`,
`channels`, and `approvers` lexically. Rule order is significant (first match) and rule ids are
unique.

### 3.1 Goals (DEC-46)

`end_date` is the last risk day of the goal (§5.4); the goal ends at 00:00 America/New_York after
it. `null` means no end.

| Type | Parameters | Behavior | Done when | Then |
|---|---|---|---|---|
| `continuous` | `end_date` | Trades the universe | `end_date` passes | Holding |
| `accumulate` | instrument, `target_qty`, `max_avg_price` (or null), `max_spend_usd`, `end_date` | Buys only the goal instrument; the universe is exactly that instrument (V-003). **Discretionary exits are disabled**; risk exits and protection apply. Buys are clipped (§8.3) so that position ≤ `target_qty`, goal spend ≤ `max_spend_usd`, and projected average cost ≤ `max_avg_price` | Position ≥ `target_qty`; remaining spend below the minimum order; or `end_date` passes | Holding |
| `profit_stop` | `profit_level`, `end_date` | Trades the universe | Agent return (E − C) ÷ C ≥ `profit_level`, compared as E − C ≥ `profit_level` × C (C = contributed capital, §5.1); or `end_date` passes | Discretionary exit of every position, then Retired (`AgentStopped`, reason `goal_complete`) |

- `profit_stop` is a level at which the agent stops, not a target or an expectation.
- **Goal spend** is the sum of the agent's buy fills in the goal instrument, including fees. Sales
  never reduce it. Projected average cost = (cost basis + qty × limit price + fee reservation) ÷
  (position + qty).
- **Holding:** restriction `goal_complete` (mode `exits_only`, §5.9). Protection is maintained. The
  owner then either **releases** the positions (step-up: the agent cancels its protective orders,
  the ledger records `PositionReleased`, the positions become the owner's external holdings, and
  claims are released) or **closes** them (discretionary exits). Either way the agent then retires.

## 4. Validation

A mandate is valid when it passes the JSON Schema, every V-rule, and the policy hierarchy (§4.3).
Failures return all violated codes. Warnings (§4.2) do not block, but each must be acknowledged
by the user and is recorded in `MandateConfirmed`.

### 4.1 V-rules

| Code | Rule |
|---|---|
| V-001 | `connection_id` belongs to the workspace and matches `environment` (paper or live account) |
| V-002 | Other active agents' allocations on the account + this allocation ≤ account equity. Checked at validation and again atomically when a version is applied |
| V-003 | `accumulate`: the universe is exactly the goal instrument |
| V-005 | `leveraged_etps_enabled = true` requires `leveraged_etp_disclosure_version`, a `DisclosureAccepted` by the owner (with step-up) for exactly that version, and policy allowing it (§4.3). A new disclosure version makes leveraged-ETP openings inactive until the owner accepts it |
| V-006 | No instrument's **instrument group** (trading spec §7.1) is claimed by another agent on the account |
| V-007 | Each signal model's id, version, and content hash are registered together; parameters are exactly the model's declared parameters and match its parameter schema |
| V-008 | If protection is enabled and the universe contains crypto, `crypto_stop_limit_offset` is set. If protection is disabled, `stop_distance`, `take_profit_distance`, and `crypto_stop_limit_offset` are null |
| V-009 | Set-like arrays are sorted and unique (§3); rule ids are unique |
| V-010 | Ladder `at` strictly increasing; actions in non-decreasing severity (`scale_sizes`, `exits_only`, `flatten_and_pause`); `factor` set for `scale_sizes` and null otherwise |
| V-011 | Exactly one `flatten_and_pause` rung, last, with `at = max_drawdown` |
| V-012 | `hysteresis` < the first rung's `at` |
| V-013 | `max_order_usd ≤ max_position_usd ≤ max_gross_exposure_usd ≤ allocation_usd` |
| V-014 | `max_loss_from_allocation ≥ max_drawdown` |
| V-015 | Dates are valid calendar dates |
| V-016 | Quiet hours `start ≠ end` |
| V-017 | Conditions nest at most 4 levels |
| V-020 | Every judgment field (§7) is `user_stated` or `user_entered`, and confirmed |
| V-022 | Every `auto` (the default or a rule's `then`) is `user_entered` and confirmed; the compiler and templates never produce `auto` |
| V-023 | Condition values match the field type (§6.3): enums take listed values (`purpose` only `open` or `increase`); decimal fields take canonical decimal strings with `eq`, `ne`, `gt`, `gte`, `lt`, `lte`; `in`/`not_in` take non-empty arrays and only on enum and string fields; booleans take `eq`/`ne`; `combined_score` and `drawdown` values are in [0, 1] |
| V-024 | Approvers resolve to at least one user with the approver role; if `two_approver_above_usd` is set, to at least two distinct users |
| V-030 | `end_date`, if set, is not before the validation date |
| V-031 | `environment` and `connection_id` equal the previous version's |

### 4.2 Warnings

| Code | Warning |
|---|---|
| W-001 | An instrument fails the eligibility floor at validation time (the gate enforces at runtime) |
| W-002 | Worst-case loss of one full position at its stop exceeds the daily loss budget: min(`max_position_usd`, `max_position_fraction` × A) × (`stop_distance` + crypto offset if crypto) > `max_daily_loss` × A. The confirmation screen shows both amounts and notes that gaps and exit pricing can exceed the stop |
| W-003 | Protection is disabled: no resting protective orders at the broker |
| W-004 | A rule uses `unusual_input`, which evaluates `true` until the input-drift detector ships (§6.3) |

### 4.3 Policy hierarchy (DEC-51)

Platform → organization → workspace → mandate. **A child may only tighten.** Each level is
checked against every level above it, so a workspace policy looser than its organization's is
itself invalid. A violation reports the key, the violating level and value, and the **nearest**
ancestor whose value it breaks (for example, "max_drawdown 0.09 exceeds workspace limit 0.08").
Policies are documents validated by [policy.schema.json](../../schemas/policy.schema.json).

| Kind | Keys | Rule |
|---|---|---|
| Maximums | `allocation_usd`, `max_loss_from_allocation`, `max_position_usd`, `max_position_fraction`, `max_gross_exposure_usd`, `max_order_usd`, `max_orders_per_day`, `max_daily_loss`, `max_drawdown`, `breach_confirm_s`, `max_output_age_s`, `exit_threshold`, `stop_distance_max`, `exits_only_at_max` (the first rung with action `exits_only` or stricter), `two_approver_above_usd` (the mandate must set one at or below it) | Child ≤ parent |
| Minimums | `entry_threshold`, `hysteresis`, `cadence_interval_s`, `approval_timeout_s`, `reentry_cooldown_s`, `daily_breach_min_s`, `scale_lift_after_s` | Child ≥ parent |
| Permissions | `leveraged_etps_allowed`, `auto_allowed` (any `auto` in the mandate) | Child may be `true` only if every ancestor is `true` |
| Requirements | `protection_required`, `independent_approval_required` | Once `true` at a level, every child is `true` |
| Sets | `asset_classes`, `signal_model_types` (`fast`, `llm`, `quant`), `goal_types`, `channels` | Child ⊆ parent |

- `independent_approval_required` (maker-checker): deployment, risk-increasing changes, and
  high-water-mark resets need approval by a user other than the requester.
- **Retail profile:** a platform-level policy applied to retail workspaces. v1 values, **pending
  counsel** ([compliance questions 20 to 29](../product/08-compliance-and-regulatory.md)):
  `leveraged_etps_allowed: false`, `protection_required: true`, `approval_timeout_s ≥ 120`.

## 5. Risk state and limits

The executor computes the risk state from the account ledger, so it is part of the **account
stream** fold (journal spec §2). It belongs to the agent and survives restarts and version
changes. Deploying a new agent (owner action with step-up) starts a new risk state.

### 5.1 Capital, equity, and allocation changes (DEC-39, DEC-50)

- **Allocation** A: Σ allocations on an account ≤ account equity (V-002).
- **Contributed capital** C: the initial allocation plus every applied allocation change.
- **Agent equity** E = A + agent realized P&L (net of fees) + agent income + agent unrealized P&L
  at risk marks (§5.2). The agent sub-ledger is the account ledger filtered to the agent's orders.
- **An allocation change** of Δ, when its version applies:
  1. **Rejected** if Δ > 0 while any latched limit is active (§5.8), if E + Δ ≤ 0, or if
     E + Δ < the agent's gross exposure (Σ |MV| + working opening orders).
  2. Otherwise E′ = E + Δ, A′ = A + Δ, C′ = C + Δ, and the high-water mark and day-start equity
     scale proportionally: H′ = round₁₂(H × E′ ÷ E), E₀′ = round₁₂(E₀ × E′ ÷ E). Drawdown and the
     daily P&L fraction are preserved, so an allocation change can neither lift nor trigger a limit.

### 5.2 Inputs, marks, and determinism

- **Inputs** are journaled events only: `MarkUpdated` (risk marks), `FillApplied`, `FeesCharged`,
  `DividendPaid`, `MandateVersionApplied`, `RiskDayStarted`, and owner acknowledgments. Each
  input is evaluated at its event time.
- **Risk marks** (trading spec §8.2: bid for longs). For **equities, only regular-session sane
  marks update E**; extended-hours quotes are ignored, so E₀ at 00:00 reflects the last
  regular-session mark. Crypto uses sane marks at all times.
- **A mark that fails freshness or sanity** is not used. If the agent holds the instrument, it gets
  restriction `stale_mark` (mode `exits_only`) until the next sane mark. A stale mark never
  triggers a flatten.
- **Comparisons are exact:** drawdown triggers when H − E ≥ `at` × H and lifts when
  H − E < (`at` − `hysteresis`) × H. Daily loss: E − E₀ ≤ −`max_daily_loss` × E₀. Floor:
  E ≤ C × (1 − `max_loss_from_allocation`).
- **Reported ratios** (DD, daily P&L fraction, the `drawdown` and `daily_pnl_fraction` condition
  fields) are rounded half-even to 12 places.
- **Evaluation order per input:** update E, then H = max(H, E); ladder rungs in ascending `at`;
  daily loss (trigger or lift); lifetime floor; then the effective mode (§5.9). Journal events
  follow this order.

### 5.3 Position, exposure, order size, count, and cooldown

These limits apply to **opening and increasing orders only**; exits, protective orders, and the
kill switch are never denied by them (AGENTS.md rule 13).

| Limit | Definition | Gate check (trading spec §9.1) | Reason code |
|---|---|---|---|
| Per-instrument position | MV(instrument) + max cost of working opening orders in it + the proposed order ≤ min(`max_position_usd`, `max_position_fraction` × E) | 2 | `concentration_limit` |
| Order size | Limit price × quantity ≤ `max_order_usd` | 2 | `max_order_size` |
| Re-entry cooldown | No opening order in an instrument until `reentry_cooldown_s` after the agent's last exit fill in it (risk, discretionary, or protective) | 2 | `reentry_cooldown` |
| Orders per day | Opening and increasing orders submitted by the agent in the risk day (each `client_order_id` once, including rejected ones) + 1 ≤ `max_orders_per_day`. Denies the order; the platform rate limits of trading spec §9.7 are separate | 6 | `max_orders_per_day` |
| Agent gross exposure | Σ \|MV\| of the agent's positions + max cost of its working opening orders + the proposed order ≤ min(`max_gross_exposure_usd`, E) | 7 | `gross_exposure_limit` |

### 5.4 Daily loss (DEC-40)

- The **risk day** runs from 00:00 to 00:00 America/New_York (23 or 25 hours on daylight-saving
  change days). The scheduler's `ClockAdvanced` across midnight is copied into the account stream,
  where the executor journals `RiskDayStarted` and sets E₀ = E.
- **Breach:** E − E₀ ≤ −`max_daily_loss` × E₀, confirmed per §5.6. The action is
  `daily_loss_action`:
  - `exits_only`: restriction `daily_loss` (mode `exits_only`).
  - `flatten_and_pause`: the agent-scoped kill switch (§5.5), restriction `daily_loss` (mode
    `paused`). Owner acknowledgment (step-up) changes the restriction to `exits_only`.
- **Lift:** automatically, once a new risk day has started **and** at least `daily_breach_min_s`
  has passed since the breach (and, for `flatten_and_pause`, after the acknowledgment). A crypto
  breach at 23:50 therefore does not lift at 00:00.

### 5.5 Drawdown and the ladder (DEC-41, DEC-44)

- **High-water mark** H = the maximum E since deployment or the last reset (§5.8).
  **Drawdown** DD = (H − E) ÷ H.
- **Rungs** trigger when H − E ≥ `at` × H:

| Action | Trigger | Effect | Lifts when |
|---|---|---|---|
| `scale_sizes` | Immediately | Order-builder targets multiplied by `factor`; active rungs multiply | H − E < (`at` − `hysteresis`) × H continuously for `scale_lift_after_s` (automatic, journaled) |
| `exits_only` | Confirmed (§5.6) | Restriction `drawdown_exits_only` (mode `exits_only`) | Owner acknowledgment (§5.8) |
| `flatten_and_pause` | Confirmed (§5.6) | Agent-scoped kill switch (trading spec §5.5); restriction `drawdown_flatten` (mode `paused`) | Owner acknowledgment (§5.8) |

- **Agent-scoped flatten** (trading spec §5.5): mode `paused` is applied first. The executor cancels
  only the agent's orders (by `client_order_id`, confirming each), never the broker's cancel-all,
  then sells exactly the agent's sub-ledger quantity, never the broker's close-position.
  Kill-switch orders are exempt from the agent's mode. **Equity sells outside the regular session
  wait for the regular session** (protection stays in place); crypto sells go immediately. Other
  agents' orders and the owner's unattributed shares are untouched.

### 5.6 Breach confirmation (DEC-49)

`exits_only` rungs, `flatten_and_pause`, daily loss, and the lifetime floor trigger only when the
breach condition holds at every input from the first breaching input t₀ through an input at or
after t₀ + `breach_confirm_s`. Any input where the condition is false restarts confirmation.
`breach_confirm_s = 0` triggers on the first breaching input. Limits awaiting confirmation are
visible in the agent's state (the `pending` list in the reference cases); they are not journaled
until they trigger.

### 5.7 Lifetime loss floor (DEC-44)

**Breach:** E ≤ C × (1 − `max_loss_from_allocation`), confirmed per §5.6 → the agent-scoped kill
switch and restriction `lifetime_floor` (mode `paused`). **It cannot be acknowledged or reset.**
Only a risk-increasing mandate version that raises `max_loss_from_allocation` enough that E is
above the new floor lifts it. Because allocation increases are rejected while it is latched, adding
capital cannot lift it.

### 5.8 Acknowledgment and high-water-mark reset (DEC-44)

- **Latched limits:** `drawdown_exits_only`, `drawdown_flatten`, `daily_loss` (until its lift), and
  `lifetime_floor`.
- **Acknowledging the drawdown ladder** (owner, step-up; with `independent_approval_required`, by
  a different user than the requester) resets H := E (`HighWaterMarkReset`, old and new H),
  unlatches the `exits_only` and `flatten_and_pause` rungs, and sets every `scale_sizes` rung
  active, lifting after `scale_lift_after_s`. Sizes therefore return gradually.
- Resets do not touch C or the lifetime floor, which bound cumulative loss across any number of
  resets.

### 5.9 Restrictions and the effective mode

Each restriction has a mode and lifts independently: `daily_loss`, `drawdown_exits_only`,
`drawdown_flatten`, `lifetime_floor`, `stale_mark`, `goal_complete`, and the trading spec's
account, external-activity, reconciliation, and rate-limit restrictions (§7.3 to §7.4, §9.7).
The **effective mode** is the strictest (`normal` < `exits_only` < `paused` < `stopped`).
`AgentModeApplied` is journaled only when the effective mode changes. On entering `exits_only` or
stricter, the executor cancels the agent's working opening orders and pending approvals.

### 5.10 Journal events

| Event (account stream) | When | Payload |
|---|---|---|
| `MandateVersionApplied` | A version takes effect or is rejected (§2.2) | agent, old and new version, classification, step-up evidence, allocation change, result and reason |
| `RiskDayStarted` | 00:00 America/New_York | agent, E₀ |
| `RiskLimitTriggered`, `RiskLimitLifted` | A limit or rung changes state | agent, limit (`max_daily_loss`, `drawdown_ladder[i]`, `lifetime_floor`), action, reason, E, H, DD, E₀, C, t₀ |
| `HighWaterMarkReset` | Owner acknowledgment (§5.8) | agent, old and new H, acknowledging user (opaque), step-up evidence |
| `AgentModeApplied` | The effective mode changes | agent, from, to, restrictions; copied by the agent runtime into the agent stream as `AgentModeChanged` |
| `KillSwitchActivated` | A flatten (`scope: agent`) | initiator (the limit), orders canceled, sells planned or deferred |
| `PositionReleased` | Owner releases positions (§3.1) | agent, instrument, quantity, user (opaque) |

## 6. Autonomy (DEC-42, DEC-48)

### 6.1 Purposes

The **gate assigns** each order's purpose from its side and the agent's position, not from the
proposer's label: a buy is `open` (no position) or `increase`; a sell of at most the position is an
exit; a sell above the position is rejected (no short sales). Protective orders are `protective`.
The originating component determines the exit type:

| Purpose | Origin | Approval | Market-conduct controls |
|---|---|---|---|
| `open`, `increase` | Order builder | Autonomy rules (§6.2) | Apply (trading spec §9.6) |
| `discretionary_exit` | Order builder (signal exit), goal completion, removed instruments, owner close | Built-in AUTO; never denied | **Paced, never denied:** price collar, participation caps (slices continue in later intervals or days), the close-window control; equities in the regular session only |
| `risk_exit` | Risk engine: limits, flatten, kill switch, stop watchdog (trading spec §5.4) | Built-in AUTO; never denied | Exempt |
| `protective` | Executor: placing and re-placing protection | Built-in AUTO; never denied | Exempt |

### 6.2 Evaluation

1. The order builder proposes an action already **clipped to the limits** (§8.3).
2. **Gate dry run.** If the gate would deny it, the action is skipped and journaled; no approval
   is ever requested for an order the gate would deny.
3. **Built-in:** purposes other than `open` and `increase` are AUTO.
4. Otherwise, evaluate `autonomy.rules` **in order**; the first matching rule decides (`auto`,
   `ask`, `deny`). If none matches, `autonomy.default`.
5. `auto` → submit (the gate runs again at submission). `deny` → skip. `ask` → approval (§6.4).

### 6.3 Condition language

Conditions are JSON objects: `{"all": [...]}`, `{"any": [...]}`, `{"not": {...}}`, or a comparison
`{"field", "op", "value"}`. Rules see only `open` and `increase` actions.

| Field | Type | Meaning |
|---|---|---|
| `purpose` | enum | `open`, `increase` |
| `order_usd` | decimal | Limit price × quantity |
| `combined_score` | decimal in [0, 1] | The order builder's combined model score (§8.3); **not a probability of profit** |
| `instrument` | string | Instrument `asset_id` |
| `asset_class` | enum | `us_equity`, `crypto` |
| `session` | enum | `pre_market`, `regular`, `after_hours`, `crypto` |
| `unusual_input` | boolean | The input-drift detector flagged the inputs. **Until the detector ships it is always `true`** (W-004) |
| `first_trade_in_instrument` | boolean | The agent has no prior fill in this instrument |
| `drawdown`, `daily_pnl_fraction` | decimal | §5.2 values (12 places) |
| `position_usd_after` | decimal | MV of the instrument + working opening orders in it + this order |
| `gross_usd_after` | decimal | Agent gross exposure including this order |
| `bought_today_usd` | decimal | Opening and increasing order value submitted in the risk day, including this order |
| `position_pnl_fraction` | decimal | (MV − cost basis) ÷ cost basis of the current position; 0 if none |

Operators: `eq`, `ne`, `gt`, `gte`, `lt`, `lte` (decimals compare numerically), `in`, `not_in`
(arrays). Type rules are V-023. The exposure fields let users bound what order splitting could
otherwise evade (for example, `bought_today_usd gt 2000 → ask`).

### 6.4 Approvals

- **Content:**
  - the proposed action (instrument, side, quantity, limit price, order value), its purpose, the
    mandate version, and the rule that triggered it;
  - each model's output, labeled by author (platform-authored or user);
  - the combined score, labeled "combined model score, not a probability of profit";
  - the deadline, and "If you do nothing, this action is skipped."
  - Never platform-authored alternatives, persuasive language, or profit estimates.
- **Binding:** an approval binds the quantity, limit price, and mandate version. On approval the
  gate runs again; if it denies, or the version has changed, the action is skipped. No re-pricing
  in v1.
- **Step-up:** live approvals require fresh step-up authentication.
- **Two approvers:** an ASKed action with `order_usd` above `two_approver_above_usd` needs two
  distinct approvers. With `independent_approval_required`, neither may be the mandate's author.
  `deny` is never overridden.
- **Timeout:** `on_timeout` is always `skip`. Approval requests are not delivered during quiet
  hours, so they time out and are skipped. **Risk-limit alerts ignore quiet hours.**

## 7. Compiler (DEC-45)

**Judgment fields:**

- `capital`, `goal`, `universe.instruments`, `universe.leveraged_etps_enabled`;
- `behavior.signal_models`, `max_output_age_s`, `cadence`, `sizing`;
- `protection`, all of `risk`, `autonomy.rules`;
- `autonomy.approval.timeout_s` and `two_approver_above_usd`;
- `autonomy.default` when it is `auto`.

- **Input:** the user's plain-language description (stored as an artifact), optional form fields,
  and optional templates. Templates set **which fields and rules are present, never values**.
- **For judgment fields the compiler only extracts values the user explicitly stated,** recording
  the quoted source span (`user_stated`). Unstated judgment fields are left empty and the user must
  enter them (`user_entered`). The compiler never proposes instruments, signal models, numbers, or
  `auto`.
- **Platform defaults** are allowed only for non-judgment fields: `name`, `notifications`,
  `approval.approvers` (`role:approver`), `approval.on_timeout` (`skip`), `autonomy.default`
  (`ask`), and `leveraged_etps_enabled` (`false`).
- **Unenforced constraints:** if the description contains a constraint the mandate cannot express
  (for example, "avoid trading around macro releases"), the compiler flags it on the review screen
  as **not enforced**; it reaches LLM models only as description text.
- The compiler is a model invocation (`ModelInvocationRecorded`); its output must pass the schema
  before review. **Quality metric:** extraction accuracy on a maintained evaluation set
  (PRD "mandate compile accuracy").

## 8. Signal models and the order builder

### 8.1 Signal model contract (DEC-52)

- A signal model is a registered component with an id (`quant.`, `fast.`, or `llm.` prefix), a
  semantic version, and a **content hash** of its code, prompt, and parameter schema. The mandate
  pins all three (V-007).
- **Signal models never place orders;** they emit outputs.
- **Parameter schemas have no defaults;** every parameter is set by the user.
- **Documentation describes methodology only,** with no performance claims, rankings, or
  "recommended" labels.
- LLM models receive only `behavior.description`, the universe, and their market-data and news
  inputs.
- Each model carries an authorship label: platform-authored, or user-authored (later).

### 8.2 Output

| Field | Type | Rule |
|---|---|---|
| `model_id`, `model_version`, `content_hash` | string | Must equal the pinned values; otherwise ignored |
| `instrument_id` | uuid | In the universe, or a held removed instrument (§2.2) |
| `as_of` | timestamp | Data cut-off used (no look-ahead) |
| `expires_at` | timestamp | |
| `conviction` | decimal in [−1, 1] | Positive favors holding; negative favors exiting (long-only in v1) |
| `confidence` | decimal in [0, 1] | The model's self-reported confidence (uncalibrated in v1) |
| `horizon_s` | integer | Intended holding horizon |
| `thesis_ref`, `evidence` | artifact hash; event IDs | Why; which observations |
| `invalidation` | string | What would invalidate the output (informational) |

**Fresh** means `as_of ≤ now < expires_at` and `now − as_of ≤ max_output_age_s`. Only the latest
fresh output per model counts (latest `as_of`, ties by journal sequence). Outputs are journaled
(`ModelOutputRecorded`); ignored outputs are journaled with the reason.

### 8.3 Order builder: `conviction_linear` (DEC-47)

The user selects the sizing method (v1 has one), and it is shown in plain language on the
confirmation screen. For each instrument at each evaluation:

1. **Combine.** W = Σ weights of **all** configured models; a model without a fresh output
   contributes 0, so a missing model makes the agent more cautious, never less.
   - c = round₁₂(Σ wᵢ · convictionᵢ · confidenceᵢ ÷ W)
   - s = round₁₂(Σ wᵢ · confidenceᵢ ÷ W), the **combined score**
   - Weights are fixed user-set values; **there is no calibration in v1.**
   - No fresh outputs: hold.
2. **Decide.**
   - c ≤ −`exit_threshold`: discretionary exit of the whole position (minus working exits). The
     limit is the bid within the collar; working opening orders in the instrument are canceled
     first. Disabled for `accumulate`.
   - −`exit_threshold` < c < `entry_threshold`: hold (no new buys, no sells).
   - c ≥ `entry_threshold`: target T = c × cap × ladder factor, where
     cap = min(`max_position_usd`, `max_position_fraction` × E) and the ladder factor is the
     product of active `scale_sizes` factors.
3. **Size.** Delta = T − MV (at the risk mark) − max cost of working opening orders.
   - Hold if Delta ≤ 0. **There are no trims in v1:** positions shrink only by exits.
   - Hold if Delta < `rebalance_band` × cap.
   - Otherwise buy at the ask with value clipped to min(Delta, `max_order_usd`, cap − MV − working,
     min(`max_gross_exposure_usd`, E) − gross exposure), then to the goal constraints (§3.1).
   - Truncate the quantity to the instrument's increment; hold if the result is below the minimum
     order.
4. The proposal (purpose, quantity, limit price, s, which outputs were used, which clips applied)
   goes to the gate dry run and autonomy (§6.2).

## 9. Versioning and change classification (DEC-43)

### 9.1 Version

`mandate_version` = `sha256:` + SHA-256 of the mandate's canonical JSON (journal spec §4).
Provenance is not part of the hashed document; `MandateConfirmed` binds it to the hash.

### 9.2 Classification

Comparing old and new versions, each changed path is classified; the version is **risk-increasing
if any path is**, otherwise risk-reducing if any path is, otherwise neutral. Changing `environment`
or `connection_id` is invalid (V-031).

| Change | Classification |
|---|---|
| Maximums: allocation, `max_loss_from_allocation`, `max_*`, `breach_confirm_s`, goal quantity, spend, `max_avg_price`, `profit_level`, `stop_distance`, `max_output_age_s`, `exit_threshold` | Increasing if larger (null means unbounded); otherwise reducing |
| Minimums: `entry_threshold`, `rebalance_band`, `hysteresis`, `reentry_cooldown_s`, `daily_breach_min_s`, `scale_lift_after_s` | Increasing if smaller; otherwise reducing |
| Ladder | Increasing if the actions change, or any `at` or `factor` is larger; otherwise reducing |
| Universe | Increasing if any instrument is added; reducing if only removed |
| `end_date` | Increasing if later or removed (null); reducing if earlier |
| `leveraged_etps_enabled` on, `protection.enabled` off | Increasing (the reverse is reducing) |
| Autonomy | Reducing only if every change is one of the following; anything else (removing or reordering rules, changing a field, operator, or compound condition, or other approval changes) is increasing:<br>• a `then` or the `default` made stricter (auto → ask → deny);<br>• a rule added whose `then` is at least as strict as every later rule and the default;<br>• in a single-comparison `auto` rule, one value changed so it matches less often;<br>• in a single-comparison `ask`/`deny` rule, one value changed so it matches more often, when no later rule and not the default is stricter;<br>• `two_approver_above_usd` set or lowered |
| Notifications | Removing a channel: increasing. Adding a channel or changing quiet hours: neutral |
| `name` | Neutral |
| Signal models (any change), sizing method, `description`, cadence interval, `daily_loss_action`, `take_profit_distance`, goal type, approvers, approval timeout, and every path not listed | Increasing (fail safe) |

- **Risk-increasing versions require step-up authentication** (and independent approval where
  policy requires it); reducing and neutral versions take effect on owner confirmation (§2.2).

## 10. Records (DEC-45, DEC-51)

| Event | Stream | Contents |
|---|---|---|
| `MandateVersionCreated` | workspace control | Source text (artifact); compiled fields; provenance per path with quoted spans; template id and version; policy-set hashes in effect; validation results including warnings; classification against the previous version; diff |
| `MandateConfirmed` | workspace control | Version hash; confirmed paths; the rendered confirmation screen (artifact) and UI build; warnings acknowledged; step-up evidence; confirming user (opaque) |
| `DisclosureAccepted` | workspace control | Disclosure document and version hash; user (opaque); step-up evidence |
| `MandateVersionApplied`, `HighWaterMarkReset`, `PositionReleased` | account | §5.10 |
| `ApprovalRequested` … `ApprovalCanceled` | agent | Content shown (artifact), bound quantity and price, approvers, step-up evidence |

A mandate version and its records are retained at least 6 years after the later of its
supersession and the closing of every position opened under it (trading spec §13).

## 11. Reference cases

[reference-cases/mandate.yaml](reference-cases/mandate.yaml) holds the base mandates, the
canonical-form hash vector, a signal-model registry, and 173 cases that implementations must
reproduce exactly. A case patches a base mandate with an RFC 6902 JSON Patch.

| Family | IDs | Covers |
|---|---|---|
| Schema | MC-S01 to MC-S19 | Structural rejects: bounds, canonical decimals, removed goal type, integer parameters, dates |
| Semantic | MC-V01 to MC-V42 | Every V-rule and warning, provenance, boundaries, multiple violations |
| Policy | MC-P01 to MC-P12 | Nearest-level reporting, each key kind, retail profile, auto and LLM bans |
| Risk state | MC-R01 to MC-R10 | Ladder and hysteresis, confirmation, reset and floor, allocation scaling, daily loss and lift delay, session marks, stale marks, mode composition |
| Risk day | MC-T01 to MC-T05 | Daylight-saving boundaries |
| Gate | MC-G01 to MC-G11 | Position cap (dollar and fraction), order size, cooldown, orders per day, gross exposure, exits exempt |
| Order builder | MC-B01 to MC-B23 | Combination, freshness, duplicates, rounding, clipping, band, no trims, dry run, accumulate clips |
| Autonomy | MC-A01 to MC-A11 | Built-in AUTO, rule order, thresholds, default, two approvers, exposure fields |
| Agent flatten | MC-F01 to MC-F02 | Shared account, owner's shares untouched, session deferral |
| Goal | MC-L01 to MC-L07 | `profit_stop`, `accumulate`, end-date boundary |
| Change | MC-C01 to MC-C31 | Every classification row, including autonomy edge cases |

## 12. Open questions

1. Evaluation set and target for extraction accuracy.
2. Input-drift detector definition for `unusual_input`.
3. Instrument-group source and maintenance (trading spec §7.1).
4. Calibration (post-v1): a user-selected, versioned method; each change journaled and classified
   risk-increasing (DEC-47).
5. Partial trims and volatility-scaled sizing as additional user-selectable sizing methods.
