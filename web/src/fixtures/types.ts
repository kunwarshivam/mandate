/**
 * Fixture shapes. Mandate fields use the names and value formats of
 * `schemas/mandate.schema.json`: decimals are strings, ids are opaque, hashes are `sha256:` refs.
 * Runtime shapes (modes, restrictions, orders, approvals) use the vocabulary of the trading-domain
 * spec (§5.7, §7.4, §9.1), the mandate spec (§5.9, §6.4), and the M7 approval lifecycle.
 */

export type Decimal = string;
export type ContentRef = `sha256:${string}`;
export type Iso = string;

export type Environment = "paper" | "live";
export type AssetClass = "crypto" | "us_equity";
export type AgentMode = "normal" | "exits_only" | "paused" | "stopped";

export interface InstrumentRef {
  asset_id: string;
  symbol: string;
  asset_class: AssetClass;
}

export interface LadderRung {
  at: Decimal;
  action: "scale_sizes" | "exits_only" | "flatten_and_pause";
  factor: Decimal | null;
}

export type Goal =
  | { type: "continuous"; end_date: string | null; on_complete: OnComplete }
  | {
      type: "accumulate";
      instrument: string;
      target_qty: Decimal;
      max_avg_price: Decimal | null;
      max_spend_usd: Decimal;
      end_date: string | null;
      on_complete: OnComplete;
    }
  | { type: "profit_stop"; profit_level: Decimal; end_date: string | null };

export type OnComplete = "hold_protected" | "release" | "disarm_ladder";

export interface SignalModelRef {
  id: string;
  version: string;
  content_hash: ContentRef;
  params: Array<{ key: string; value: string }>;
  weight: Decimal;
  max_output_age_s: number;
  admits_instruments: boolean;
}

export interface AutonomyRule {
  id: string;
  when: { field: string; op: string; value: string | string[] };
  then: "auto" | "ask" | "deny";
}

export interface Mandate {
  mandate_schema_version: 1;
  name: string;
  source_text_ref: ContentRef | null;
  environment: Environment;
  connection_id: string;
  capital: { allocation_usd: Decimal; max_loss_from_allocation: Decimal };
  goal: Goal;
  universe: {
    pinned: boolean;
    pinned_instruments: InstrumentRef[];
    max_instruments: number;
    asset_classes: AssetClass[];
    leveraged_etps_enabled: boolean;
    leveraged_etp_disclosure_version: ContentRef | null;
  };
  behavior: {
    description: string;
    signal_models: SignalModelRef[];
    research: null;
    cadence: { interval_s: number; event_sources: string[] };
    sizing: { method: "conviction_linear"; entry_threshold: Decimal; exit_threshold: Decimal; rebalance_band: Decimal };
  };
  protection: {
    enabled: boolean;
    stop_distance: Decimal;
    take_profit_distance: Decimal | null;
    crypto_stop_limit_offset: Decimal | null;
  };
  risk: {
    max_position_usd: Decimal;
    max_position_fraction: Decimal;
    max_gross_exposure_usd: Decimal;
    max_order_usd: Decimal;
    max_orders_per_day: number;
    max_daily_loss: Decimal;
    daily_loss_action: "exits_only" | "flatten_and_pause";
    max_drawdown: Decimal;
    drawdown_ladder: LadderRung[];
    hysteresis: Decimal;
    scale_action: "limit_buys" | "trim_to_target";
    breach_confirm_s: number;
    daily_breach_min_s: number;
    scale_lift_after_s: number;
    reentry_cooldown_s: number;
  };
  autonomy: {
    rules: AutonomyRule[];
    default: "ask" | "deny" | "auto";
    admission: "ask" | "deny" | "auto";
    approval: {
      timeout_s: number;
      on_timeout: "skip";
      approvers: string[];
      two_approver_above_usd: Decimal | null;
    };
  };
  notifications: {
    channels: string[];
    quiet_hours: { start: string; end: string; timezone: string } | null;
  };
}

/** Mandate spec §2.1. Recorded per JSON Pointer, outside the hashed document. */
export type Provenance = "user_stated" | "user_entered" | "template_structure" | "platform_proposed" | "platform_default";

export interface FieldProvenance {
  path: string;
  provenance: Provenance;
  quote?: string;
}

/** Mandate spec §5.9 and the trading-domain spec §7.3, §7.4, §9.7, §11. */
export type RestrictionCode =
  | "drawdown_scale_sizes"
  | "drawdown_exits_only"
  | "drawdown_flatten"
  | "daily_loss"
  | "hard_breach"
  | "lifetime_floor"
  | "goal_complete"
  | "external_activity"
  | "account_closing_only"
  | "account_blocked"
  | "reconciliation_mismatch"
  | "startup_reconciliation"
  | "leverage_check_failed"
  | "owner_pause"
  | "stopped"
  | "stale_mark"
  | "removed_instrument";

export interface ActiveRestriction {
  code: RestrictionCode;
  since: Iso;
  /** Instrument restrictions name the instrument they block. */
  symbol?: string;
}

export interface PendingBreach {
  limit: string;
  seconds_in_breach: number;
  confirm_after_s: number;
}

/** Trading-domain spec §5.7. */
export type OrderState =
  | "Intent"
  | "Submitting"
  | "Accepted"
  | "PartiallyFilled"
  | "PendingCancel"
  | "PendingReplace"
  | "Filled"
  | "Canceled"
  | "Rejected"
  | "Expired"
  | "Replaced"
  | "Abandoned"
  | "Unknown";

/** Mandate spec §6.1: assigned by the gate, never by the proposer. */
export type Purpose = "open" | "increase" | "discretionary_exit" | "owner_exit" | "risk_exit" | "protective";

export interface Protection {
  kind: "bracket" | "oco" | "crypto_stop_limit" | "none";
  stop_price: Decimal | null;
  limit_price: Decimal | null;
  take_profit_price: Decimal | null;
  unprotected_fraction: Decimal | null;
}

export interface Position {
  instrument: InstrumentRef;
  qty: Decimal;
  broker_qty: Decimal;
  avg_cost: Decimal;
  mark: Decimal;
  mark_as_of: Iso;
  market_value: Decimal;
  unrealized_pnl: Decimal;
  protection: Protection;
}

export interface WorkingOrder {
  client_order_id: string;
  instrument: InstrumentRef;
  side: "buy" | "sell";
  qty: Decimal;
  filled_qty: Decimal;
  limit_price: Decimal | null;
  stop_price: Decimal | null;
  purpose: Purpose;
  state: OrderState;
  submitted_at: Iso;
  time_in_force: "day" | "gtc";
}

/** Registered in docs/specs/reference-cases/trading-domain.yaml. */
export type ReasonCode =
  | "account_restricted"
  | "account_trading_blocked"
  | "agent_exits_only"
  | "agent_paused"
  | "agent_stopped"
  | "not_in_universe"
  | "concentration_limit"
  | "max_order_size"
  | "reentry_cooldown"
  | "session_not_allowed"
  | "extended_hours_opening_not_allowed"
  | "auction_window"
  | "instrument_halted"
  | "unknown_order_in_flight"
  | "stale_mark"
  | "price_outside_collar"
  | "max_orders_per_day"
  | "close_window"
  | "discretionary_exit_regular_session_only"
  | "owner_confirmation_required"
  | "gross_exposure_limit"
  | "insufficient_buying_power";

export interface GateDecision {
  event_id: string;
  at: Iso;
  agent_id: string;
  /**
   * The mandate version the gate decided under: `GateDecided`'s `config_refs.mandate_version`
   * (journal spec §9, `man`), the hash of the stored mandate document (mandate spec §9.1). The
   * owner's rules are read from that version, never from the agent's current one.
   */
  mandate_version: ContentRef;
  verdict: "allow" | "deny" | "defer";
  reason_code: ReasonCode | null;
  action: { side: "buy" | "sell"; qty: Decimal; symbol: string; limit_price: Decimal; purpose: Purpose };
  /** For an allow: what happened next in plain words. */
  then?: string;
  /** The order an allow led to, if one was sent. */
  client_order_id?: string;
  /** The approval request an allow led to, if the mandate asked the owner. */
  approval_id?: string;
}

/** An execution at the broker, as journaled. */
export interface Fill {
  fill_id: string;
  client_order_id: string;
  instrument: InstrumentRef;
  side: "buy" | "sell";
  qty: Decimal;
  price: Decimal;
  at: Iso;
}

/** An order that reached a final state. `note` says why, in owner words. */
export interface PastOrder extends WorkingOrder {
  state: "Filled" | "Canceled" | "Rejected" | "Expired" | "Replaced" | "Abandoned";
  closed_at: Iso;
  note: string;
}

export type TimelineKind = "fill" | "order" | "mode" | "approval" | "gate" | "protection" | "reconciliation" | "version";

export interface TimelineEvent {
  event_id: string;
  at: Iso;
  kind: TimelineKind;
  text: string;
}

/** Mandate spec §9.2: how a changed path, or a whole version, moves risk. */
export type ChangeClass = "risk_increasing" | "risk_reducing" | "neutral";

/** One changed path of a version's diff, classified, as `MandateVersionCreated` records it (mandate spec §10). */
export interface MandateChange {
  path: string;
  from: Decimal | number | null;
  to: Decimal | number | null;
  classification: ChangeClass;
}

/**
 * `MandateVersionApplied` (mandate spec §2.2): applied, with the approvals it canceled, or rejected
 * with the reason. `pending`: a confirmed risk-increasing version that has not reached a safe point.
 */
export type VersionApplication =
  | { result: "applied"; at: Iso; approvals_canceled: number }
  | { result: "rejected"; at: Iso; reason: string }
  | { result: "pending" };

/**
 * A confirmed version of an agent's mandate. The first has no `previous` and no diff; every later one
 * is diffed against the version in effect when it was confirmed.
 */
export interface MandateVersionRecord {
  mandate_version: ContentRef;
  previous: ContentRef | null;
  confirmed_at: Iso;
  /** Confirmed with a passkey: deploying, and every risk-increasing version (mandate spec §9.2). */
  step_up: boolean;
  classification: ChangeClass | null;
  changes: MandateChange[];
  application: VersionApplication;
}

export interface LimitState {
  equity: Decimal;
  equity_day_start: Decimal;
  high_water_mark: Decimal;
  capital_base: Decimal;
  inherited_loss: Decimal;
  orders_today: number;
  size_factor: Decimal;
  pending: PendingBreach[];
}

export interface Agent {
  agent_id: string;
  /** Platform-assigned opaque label (PX-6 (c)); the name stays off notifications and titles. */
  label: string;
  mandate: Mandate;
  mandate_version: ContentRef;
  provenance: FieldProvenance[];
  mode: AgentMode;
  restrictions: ActiveRestriction[];
  startup: "reconciling" | "ready";
  pnl_total: Decimal;
  pnl_today: Decimal;
  realized_pnl: Decimal;
  state: LimitState;
  positions: Position[];
  orders: WorkingOrder[];
  past_orders: PastOrder[];
  fills: Fill[];
  goal_progress: { spent_usd: Decimal; held_qty: Decimal } | null;
  deployed_at: Iso;
  /** Oldest first. The last applied one is `mandate_version`. */
  versions: MandateVersionRecord[];
}

/** M7 brief, "The approval lifecycle". */
export type ApprovalStatus = "delivered" | "acted" | "gate_skipped" | "rejected" | "expired" | "superseded";

export type CancelReason = "version_applied" | "mode_tightened" | "owner_pause" | "owner_stop" | "kill_switch";

export interface RiskFigure {
  field: "order_usd" | "position_usd_after" | "gross_usd_after" | "bought_today_usd";
  value: Decimal;
  cap: Decimal | null;
}

export interface ModelOutput {
  author: "owner_selected" | "platform";
  model_id: string;
  version: string;
  produced_at: Iso;
  lines: string[];
}

export interface Approval {
  approval_id: string;
  agent_id: string;
  status: ApprovalStatus;
  requested_at: Iso;
  deadline: Iso;
  bound: {
    symbol: string;
    asset_class: AssetClass;
    side: "buy";
    qty: Decimal;
    limit: Decimal;
    purpose: "open" | "increase";
    mandate_version: ContentRef;
    decided_by: string;
    combined_score: Decimal;
  };
  trigger: string;
  risk_impact: RiskFigure[];
  evidence: ModelOutput[];
  approvers_required: number;
  approvals_so_far: Array<{ user_label: string; at: Iso }>;
  resolution?: { at: Iso; text: string; cancel_reason?: CancelReason };
}

export interface ExternalPosition {
  instrument: InstrumentRef;
  qty: Decimal;
}

export type HealthState = "ok" | "stale" | "down";

export interface Health {
  market_data: { state: HealthState; as_of: Iso };
  broker: { state: HealthState; as_of: Iso };
  deployment: { state: HealthState; as_of: Iso };
  relay: { state: HealthState; as_of: Iso };
}

export interface Connection {
  connection_id: string;
  broker: "Alpaca paper";
  account_equity: Decimal;
  day_trading_regime: "intraday_margin";
}

export type Scenario =
  | "normal"
  | "empty"
  | "loading"
  | "stale"
  | "paused"
  | "drawdown"
  | "reconciliation"
  | "unknown-order"
  | "unreachable"
  | "approvals"
  | "result-unknown";

export interface Workspace {
  scenario: Scenario;
  status: "ready" | "loading" | "unreachable";
  /** Whether the deployment answers commands with a journal entry, or goes quiet after taking them. */
  journal: "answers" | "silent";
  now: Iso;
  environment: Environment;
  connection: Connection;
  /**
   * Distinct active members with the approver role, which V-024 counts for a two-approver threshold
   * and V-047 reads as a lower bound on the workspace's users. A pending invitation or a deactivated
   * account is not counted.
   */
  approver_users: number;
  /**
   * The workspace's effective `independent_approval_required` (policy schema; mandate spec §4.3:
   * once `true` at a level it is `true` below it). `null` when not known. Anything but a stated
   * `false`, including the field missing, counts as required (rule 3, V-047).
   */
  independent_approval_required: boolean | null;
  health: Health;
  agents: Agent[];
  approvals: Approval[];
  decisions: GateDecision[];
  timeline: Record<string, TimelineEvent[]>;
  external_positions: ExternalPosition[];
}
