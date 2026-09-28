/**
 * Recorded fixture mandates, taken from `reference/mandate/bases.py` (btc_accumulator,
 * two_stock_swing) plus one flat agent built from the same risk block. XYZ, QRS, LMN, and ABC are
 * the reference cases' fictional instruments. The research agent's base is left out: users do not
 * see the research agent until the DEC-99 evaluation passes (PX-9 (a)).
 */
import type { AutonomyRule, FieldProvenance, LadderRung, Mandate } from "./types";

export const INSTRUMENTS = {
  BTC: { asset_id: "7b4a1c2e-1111-4a2b-9c3d-000000000001", symbol: "BTC/USD", asset_class: "crypto" },
  XYZ: { asset_id: "7b4a1c2e-2222-4a2b-9c3d-000000000002", symbol: "XYZ", asset_class: "us_equity" },
  QRS: { asset_id: "7b4a1c2e-3333-4a2b-9c3d-000000000003", symbol: "QRS", asset_class: "us_equity" },
  LMN: { asset_id: "7b4a1c2e-4444-4a2b-9c3d-000000000004", symbol: "LMN", asset_class: "us_equity" },
  ABC: { asset_id: "7b4a1c2e-5555-4a2b-9c3d-000000000005", symbol: "ABC", asset_class: "us_equity" },
} as const;

const H_MR = `sha256:${"1".repeat(64)}` as const;
const H_MOM = `sha256:${"2".repeat(64)}` as const;
const H_NEWS = `sha256:${"3".repeat(64)}` as const;

const LADDER: LadderRung[] = [
  { at: "0.03", action: "scale_sizes", factor: "0.5" },
  { at: "0.06", action: "exits_only", factor: null },
  { at: "0.08", action: "flatten_and_pause", factor: null },
];

const RULES: AutonomyRule[] = [
  { id: "large_orders", when: { field: "order_usd", op: "gt", value: "900" }, then: "ask" },
  { id: "low_score", when: { field: "combined_score", op: "lt", value: "0.65" }, then: "ask" },
  { id: "routine", when: { field: "purpose", op: "in", value: ["increase", "open"] }, then: "auto" },
];

const RISK: Mandate["risk"] = {
  max_position_usd: "10000",
  max_position_fraction: "1",
  max_gross_exposure_usd: "10000",
  max_order_usd: "1000",
  max_orders_per_day: 50,
  max_daily_loss: "0.02",
  daily_loss_action: "exits_only",
  max_drawdown: "0.08",
  drawdown_ladder: LADDER,
  hysteresis: "0.01",
  scale_action: "limit_buys",
  breach_confirm_s: 60,
  daily_breach_min_s: 3600,
  scale_lift_after_s: 600,
  reentry_cooldown_s: 3600,
};

export const btcAccumulator: Mandate = {
  mandate_schema_version: 1,
  name: "btc-accumulator",
  source_text_ref: "sha256:60fc25912e951fe42f1be657ff72f3a8eb3903f3c666f85fcd63dc2b117d0145",
  environment: "paper",
  connection_id: "conn_alpaca_paper_01",
  capital: { allocation_usd: "10000", max_loss_from_allocation: "0.1" },
  goal: {
    type: "accumulate",
    instrument: INSTRUMENTS.BTC.asset_id,
    target_qty: "0.15",
    max_avg_price: "58000",
    max_spend_usd: "9000",
    end_date: "2026-12-31",
    on_complete: "hold_protected",
  },
  universe: {
    pinned: true,
    pinned_instruments: [INSTRUMENTS.BTC],
    max_instruments: 1,
    asset_classes: ["crypto"],
    leveraged_etps_enabled: false,
    leveraged_etp_disclosure_version: null,
  },
  behavior: {
    description: "Buy dips; avoid trading 30 minutes around major macro releases.",
    signal_models: [
      {
        id: "quant.mean_reversion",
        version: "1.0.0",
        content_hash: H_MR,
        params: [
          { key: "lookback_bars", value: "20" },
          { key: "z_entry", value: "1.5" },
        ],
        weight: "1",
        max_output_age_s: 1800,
        admits_instruments: false,
      },
    ],
    research: null,
    cadence: { interval_s: 900, event_sources: ["news", "price"] },
    sizing: { method: "conviction_linear", entry_threshold: "0.3", exit_threshold: "0.3", rebalance_band: "0.05" },
  },
  protection: { enabled: true, stop_distance: "0.08", take_profit_distance: null, crypto_stop_limit_offset: "0.005" },
  risk: { ...RISK },
  autonomy: {
    rules: RULES,
    default: "ask",
    admission: "ask",
    approval: { timeout_s: 600, on_timeout: "skip", approvers: ["role:approver"], two_approver_above_usd: null },
  },
  notifications: {
    channels: ["email", "web_push"],
    quiet_hours: { start: "23:00", end: "07:00", timezone: "America/New_York" },
  },
};

export const twoStockSwing: Mandate = {
  ...btcAccumulator,
  name: "two-stock-swing",
  source_text_ref: null,
  goal: { type: "profit_stop", profit_level: "0.1", end_date: null },
  universe: {
    pinned: true,
    pinned_instruments: [INSTRUMENTS.XYZ, INSTRUMENTS.QRS],
    max_instruments: 2,
    asset_classes: ["us_equity"],
    leveraged_etps_enabled: false,
    leveraged_etp_disclosure_version: null,
  },
  behavior: {
    description: "Swing trade two stocks on momentum and news.",
    signal_models: [
      { id: "llm.news_research", version: "0.3.0", content_hash: H_NEWS, params: [], weight: "0.4", max_output_age_s: 3600, admits_instruments: false },
      {
        id: "quant.momentum",
        version: "1.0.0",
        content_hash: H_MOM,
        params: [{ key: "lookback_bars", value: "50" }],
        weight: "0.6",
        max_output_age_s: 900,
        admits_instruments: false,
      },
    ],
    research: null,
    cadence: { interval_s: 300, event_sources: ["fills", "news", "price"] },
    sizing: { method: "conviction_linear", entry_threshold: "0.3", exit_threshold: "0.3", rebalance_band: "0.05" },
  },
  protection: { enabled: true, stop_distance: "0.05", take_profit_distance: "0.1", crypto_stop_limit_offset: null },
  risk: { ...RISK, max_position_usd: "1500", max_position_fraction: "0.2", max_gross_exposure_usd: "2000" },
};

export const lmnCore: Mandate = {
  ...twoStockSwing,
  name: "lmn-core",
  source_text_ref: "sha256:38e2a4bea8b3e27268475eda40ce71b0d4f897f364cbdfbe2d11ab202c4a538e",
  capital: { allocation_usd: "5000", max_loss_from_allocation: "0.1" },
  goal: { type: "continuous", end_date: null, on_complete: "hold_protected" },
  universe: {
    pinned: true,
    pinned_instruments: [INSTRUMENTS.LMN],
    max_instruments: 1,
    asset_classes: ["us_equity"],
    leveraged_etps_enabled: false,
    leveraged_etp_disclosure_version: null,
  },
  behavior: {
    ...twoStockSwing.behavior,
    description: "Hold a core position in LMN; add on momentum, never more than $1,000 an order.",
    signal_models: [twoStockSwing.behavior.signal_models[1]],
  },
  risk: { ...RISK, max_position_usd: "2500", max_position_fraction: "0.5", max_gross_exposure_usd: "2500" },
  autonomy: {
    ...btcAccumulator.autonomy,
    approval: { timeout_s: 900, on_timeout: "skip", approvers: ["role:approver"], two_approver_above_usd: "400" },
  },
};

export const provenance: Record<string, FieldProvenance[]> = {
  "btc-accumulator": [
    { path: "/capital/allocation_usd", provenance: "user_stated", quote: "put $10,000 of paper money on it" },
    { path: "/capital/max_loss_from_allocation", provenance: "user_entered" },
    { path: "/goal/target_qty", provenance: "user_stated", quote: "build up to 0.15 BTC" },
    { path: "/goal/max_avg_price", provenance: "user_stated", quote: "never pay more than $58k on average" },
    { path: "/goal/max_spend_usd", provenance: "platform_proposed" },
    { path: "/universe/pinned_instruments", provenance: "user_stated", quote: "only bitcoin" },
    { path: "/protection/stop_distance", provenance: "template_structure" },
    { path: "/risk/max_order_usd", provenance: "user_entered" },
    { path: "/risk/max_daily_loss", provenance: "platform_proposed" },
    { path: "/risk/drawdown_ladder", provenance: "template_structure" },
    { path: "/autonomy/default", provenance: "platform_default" },
    { path: "/environment", provenance: "platform_default" },
  ],
  "two-stock-swing": [
    { path: "/capital/allocation_usd", provenance: "user_entered" },
    { path: "/capital/max_loss_from_allocation", provenance: "user_entered" },
    { path: "/goal/profit_level", provenance: "user_entered" },
    { path: "/universe/pinned_instruments", provenance: "user_entered" },
    { path: "/protection/stop_distance", provenance: "user_entered" },
    { path: "/risk/max_position_usd", provenance: "user_entered" },
    { path: "/risk/max_order_usd", provenance: "user_entered" },
    { path: "/risk/max_daily_loss", provenance: "platform_proposed" },
    { path: "/risk/drawdown_ladder", provenance: "template_structure" },
    { path: "/autonomy/default", provenance: "platform_default" },
    { path: "/environment", provenance: "platform_default" },
  ],
  "lmn-core": [
    { path: "/capital/allocation_usd", provenance: "user_stated", quote: "$5,000" },
    { path: "/capital/max_loss_from_allocation", provenance: "user_entered" },
    { path: "/universe/pinned_instruments", provenance: "user_stated", quote: "a core position in LMN" },
    { path: "/protection/stop_distance", provenance: "template_structure" },
    { path: "/risk/max_order_usd", provenance: "user_stated", quote: "never more than $1,000 an order" },
    { path: "/risk/max_daily_loss", provenance: "platform_proposed" },
    { path: "/risk/drawdown_ladder", provenance: "template_structure" },
    { path: "/autonomy/approval/two_approver_above_usd", provenance: "user_entered" },
    { path: "/autonomy/default", provenance: "platform_default" },
    { path: "/environment", provenance: "platform_default" },
  ],
};
