/**
 * `GET /backtests/{id}` (workspace API spec §4.2; `schemas/workspace-api/read-models/backtest.schema.json`;
 * DEC-1001, DEC-2400): one backtest of the workspace's own mandate version, decoded exactly as the
 * schema states it. Decimals stay canonical strings, counts are safe non-negative integers, every
 * enum is closed, and the state decides which of `refusal`, `report_ref` and `report` are present.
 * The report holds no thesis-quality figure (DEC-1001 item 3) and names its disclosures only
 * (DEC-1003). Unknown members are ignored (spec §3.2).
 */
import type { WorkspaceClient } from "./client";
import type { ApiError, ContentRef, Decoded, Decoder, Watermark } from "./types";

export const OUTCOMES = ["submitted", "builder_hold", "builder_refused", "gate_deny", "gate_defer", "gate_hold", "autonomy_deny", "ask_unanswered", "order_working"] as const;
export type Outcome = (typeof OUTCOMES)[number];

export const PURPOSES = ["open", "increase", "discretionary_exit", "risk_exit"] as const;
export type Purpose = (typeof PURPOSES)[number];

export const MODES = ["normal", "exits_only", "paused", "stopped"] as const;
export type RestrictiveMode = (typeof MODES)[number];

export const LIMITATIONS = [
  "no_lookahead_by_receive_time",
  "recorded_model_outputs_only",
  "admissions_replayed_not_rejudged",
  "fills_within_bar_range",
  "spread_from_fill_model",
  "single_instrument",
  "daily_decisions",
  "asks_unanswered_skip",
  "participation_from_bar_volume",
  "marks_at_bar_close",
  "trading_status_assumed",
  "mechanics_not_thesis_quality",
] as const;
export type Limitation = (typeof LIMITATIONS)[number];

/** DEC-1003's names: placeholders, never text. */
export const DISCLOSURES = ["LEGEND-HYPOTHETICAL", "NOTE-BACKTEST-MECHANICS", "NOTE-BACKTEST-LIMITS"] as const;
export type Disclosure = (typeof DISCLOSURES)[number];

export const ABSENT = ["fewer_than_two_periods", "zero_variance"] as const;
export type Absent = (typeof ABSENT)[number];

export type SharpeSign = "negative" | "zero" | "positive";

/** E4-2's metric block (DEC-127), unchanged. A `null` statistic was not computed, and `absent` says why. */
export interface Metrics {
  period_count: number;
  first_date: string;
  last_date: string;
  starting_equity: string;
  ending_equity: string;
  net_pnl: string;
  total_return: string;
  return_sum: string;
  return_sum_of_squares: string;
  mean_return: string;
  variance: string | null;
  volatility: string | null;
  periods_per_year: number;
  variance_annualized: string | null;
  volatility_annualized: string | null;
  risk_free_per_period: string;
  sharpe_squared: string | null;
  sharpe_sign: SharpeSign;
  sharpe: string | null;
  sharpe_squared_annualized: string | null;
  sharpe_annualized: string | null;
  max_drawdown: string;
  max_drawdown_usd: string;
  max_drawdown_peak_period: number;
  max_drawdown_trough_period: number;
  buy_notional: string;
  sell_notional: string;
  traded_notional: string;
  fill_count: number;
  turnover: string;
  fees_total: string;
  fees_charged: string;
  fees_accrued: string;
  fees_asset: string;
  submitted_qty: string;
  filled_qty: string;
  absent: Absent | null;
}

/** One decision instant: what the version's own path decided and why. A hold has a `null` purpose. */
export interface DecisionRow {
  at: string;
  date: string;
  purpose: Purpose | null;
  outcome: Outcome;
  reason: string | null;
  decided_by: string | null;
  qty: string | null;
  limit: string | null;
  outputs_used: number;
}

export interface EquityPoint {
  date: string;
  equity: string;
}

/** The report artifact, `report_version` 2 (DEC-1001 item 2). */
export interface BacktestReport {
  kind: "backtest_report";
  report_version: 2;
  subject: "mandate_version";
  mandate_version: ContentRef;
  instrument: string;
  research_agent: boolean;
  inputs: { bars: ContentRef; model_outputs: ContentRef; theses: ContentRef; config: ContentRef };
  strategy: Metrics;
  benchmark: Metrics;
  excess_total_return: string;
  decisions: {
    count: number;
    outcomes: Record<Outcome, number>;
    gate_reasons: Record<string, number>;
    hold_reasons: Record<string, number>;
    rows: DecisionRow[];
  };
  risk: { limits_latched: string[]; risk_exits: number; most_restrictive_mode: RestrictiveMode };
  equity: { strategy: EquityPoint[]; benchmark: EquityPoint[] };
  limitations: Limitation[];
  disclosures: Disclosure[];
}

interface BacktestBase {
  backtest_id: string;
  mandate_version: ContentRef;
}

/** `running` until `BacktestRunRecorded` commits, then `completed` or `refused` (DEC-1001 item 5). */
export type Backtest =
  | (BacktestBase & { state: "running"; refusal: null; report_ref: null; report: null })
  | (BacktestBase & { state: "refused"; refusal: string; report_ref: null; report: null })
  | (BacktestBase & { state: "completed"; refusal: null; report_ref: ContentRef; report: BacktestReport });

/** What a read of one backtest gives a screen: the backtest asked for, or the API's failure. */
export type BacktestRead = { ok: true; backtest: Backtest } | { ok: false; error: ApiError };

/** The body of `read-models/backtest`: the backtest and the streams it reflects (`AsOf`, at least one). */
export const backtestRead: Decoder<{ backtest: Backtest; as_of: Watermark[] }> = (value, path): Decoded<{ backtest: Backtest; as_of: Watermark[] }> => {
  void value;
  void path;
  throw new Error("Unimplemented: E4-5");
};

/** Reads one backtest over the API; an answer for another id is refused. */
export async function readBacktestFrom(client: WorkspaceClient, backtestId: string): Promise<BacktestRead> {
  void client;
  void backtestId;
  throw new Error("Unimplemented: E4-5");
}

/** A body held in the served shape (the fixtures), through the same decoder and the same id check. */
export function servedBacktest(body: unknown, backtestId: string): BacktestRead {
  void body;
  void backtestId;
  throw new Error("Unimplemented: E4-5");
}

/** The answer to an id nothing holds: the API's 404 `not_found`. */
export function backtestNotFound(): BacktestRead {
  throw new Error("Unimplemented: E4-5");
}
