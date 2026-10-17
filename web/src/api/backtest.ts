/**
 * `GET /backtests/{id}` (workspace API spec §4.2; `schemas/workspace-api/read-models/backtest.schema.json`;
 * DEC-1001, DEC-2400): one backtest of the workspace's own mandate version, decoded exactly as the
 * schema states it. Decimals stay canonical strings, counts are safe non-negative integers, every
 * enum is closed, and the state decides which of `refusal`, `report_ref` and `report` are present.
 * The report holds no thesis-quality figure (DEC-1001 item 3) and names its disclosures only
 * (DEC-1003). Unknown members are ignored (spec §3.2).
 */
import type { WorkspaceClient } from "./client";
import { array, closedEnum, contentRef, decimal, integer, isRecord, nullable, object, timestamp, watermark } from "./decode";
import type { ApiError, ContentRef, Decoded, DecodeIssue, Decoder, Watermark } from "./types";

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

const fail = <T,>(path: string, problem: DecodeIssue["problem"], value: string | null = null, allowed: readonly string[] | null = null): Decoded<T> => ({
  ok: false,
  issue: { path, problem, value, allowed },
});

function patterned(pattern: RegExp): Decoder<string> {
  return (value, path) => {
    if (typeof value !== "string") return fail(path, "wrong_type");
    return pattern.test(value) ? { ok: true, value } : fail(path, "not_canonical", value);
  };
}

/** A value the schema fixes (`const`): anything else is a value the closed set does not list. */
function constant<const T extends string | number>(expected: T): Decoder<T> {
  return (value, path) => (value === expected ? { ok: true, value: expected } : fail(path, typeof value === typeof expected ? "unknown_enum_value" : "wrong_type", String(value), [String(expected)]));
}

const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path, "wrong_type"));

/** `minItems: 1` when `least` is 1, and `uniqueItems`. */
function uniqueList<T extends string>(item: Decoder<T>, least: 0 | 1): Decoder<T[]> {
  return (value, path) => {
    const read = array(item)(value, path);
    if (!read.ok) return read;
    return read.value.length >= least && new Set(read.value).size === read.value.length ? read : fail(path, "not_canonical", JSON.stringify(read.value));
  };
}

/** `common.schema.json`'s `Id`, `AssetId`; the backtest schema's `Date` and `Code`. */
const id = patterned(/^[A-Za-z0-9_-]{1,64}$/);
const assetId = patterned(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
const date = patterned(/^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$/);
const CODE = /^[a-z][a-z0-9_]{0,63}$/;
const code = patterned(CODE);
const decidedBy = patterned(/^[a-z][a-z0-9_.:-]{0,127}$/);
const latchedLimit = patterned(/^(max_daily_loss|lifetime_floor|profit_stop|drawdown_ladder\[[0-9]{1,2}\])$/);
/** `Count`: a non-negative integer up to 2^53 − 1. */
const count = integer;

const periodsPerYear: Decoder<number> = (value, path) => {
  const read = integer(value, path);
  if (!read.ok) return read;
  return read.value >= 1 && read.value <= 366 ? read : fail(path, "not_canonical", String(read.value));
};

/**
 * A map from a reason code to a count (`propertyNames: Code`, `additionalProperties: Count`). A failure
 * inside it names the entry as `…/*`, never by its served key, so no served text reaches a screen
 * through the failure's path, and the issue carries no value.
 */
const reasonCounts: Decoder<Record<string, number>> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const out: Record<string, number> = {};
  const masked = `${path}/*`;
  for (const [key, entry] of Object.entries(value)) {
    if (!CODE.test(key)) return fail(masked, "not_canonical");
    const read = count(entry, masked);
    if (!read.ok) return read;
    out[key] = read.value;
  }
  return { ok: true, value: out };
};

const nullableDecimal = nullable(decimal);

const metrics = object<Metrics>({
  period_count: count,
  first_date: date,
  last_date: date,
  starting_equity: decimal,
  ending_equity: decimal,
  net_pnl: decimal,
  total_return: decimal,
  return_sum: decimal,
  return_sum_of_squares: decimal,
  mean_return: decimal,
  variance: nullableDecimal,
  volatility: nullableDecimal,
  periods_per_year: periodsPerYear,
  variance_annualized: nullableDecimal,
  volatility_annualized: nullableDecimal,
  risk_free_per_period: decimal,
  sharpe_squared: nullableDecimal,
  sharpe_sign: closedEnum<SharpeSign>(["negative", "zero", "positive"]),
  sharpe: nullableDecimal,
  sharpe_squared_annualized: nullableDecimal,
  sharpe_annualized: nullableDecimal,
  max_drawdown: decimal,
  max_drawdown_usd: decimal,
  max_drawdown_peak_period: count,
  max_drawdown_trough_period: count,
  buy_notional: decimal,
  sell_notional: decimal,
  traded_notional: decimal,
  fill_count: count,
  turnover: decimal,
  fees_total: decimal,
  fees_charged: decimal,
  fees_accrued: decimal,
  fees_asset: decimal,
  submitted_qty: decimal,
  filled_qty: decimal,
  absent: nullable(closedEnum(ABSENT)),
});

const outcome = closedEnum(OUTCOMES);

const decisionRow = object<DecisionRow>({
  at: timestamp,
  date,
  purpose: nullable(closedEnum(PURPOSES)),
  outcome,
  reason: nullable(code),
  decided_by: nullable(decidedBy),
  qty: nullableDecimal,
  limit: nullableDecimal,
  outputs_used: count,
});

const equityPoint = object<EquityPoint>({ date, equity: decimal });

/** The nine outcome codes, each a required member with its count, zeros included. */
const outcomeCounts = object<Record<Outcome, number>>(Object.fromEntries(OUTCOMES.map((o) => [o, count])) as Record<Outcome, Decoder<number>>);

const report = object<BacktestReport>({
  kind: constant("backtest_report"),
  report_version: constant(2),
  subject: constant("mandate_version"),
  mandate_version: contentRef,
  instrument: assetId,
  research_agent: flag,
  inputs: object({ bars: contentRef, model_outputs: contentRef, theses: contentRef, config: contentRef }),
  strategy: metrics,
  benchmark: metrics,
  excess_total_return: decimal,
  decisions: object({ count, outcomes: outcomeCounts, gate_reasons: reasonCounts, hold_reasons: reasonCounts, rows: array(decisionRow) }),
  risk: object({ limits_latched: uniqueList(latchedLimit, 0), risk_exits: count, most_restrictive_mode: closedEnum(MODES) }),
  equity: object({ strategy: array(equityPoint), benchmark: array(equityPoint) }),
  limitations: uniqueList(closedEnum(LIMITATIONS), 1),
  disclosures: uniqueList(closedEnum(DISCLOSURES), 1),
});

type Served = { backtest_id: string; mandate_version: ContentRef; state: Backtest["state"]; refusal: string | null; report_ref: ContentRef | null; report: BacktestReport | null };

const served = object<Served>({
  backtest_id: id,
  mandate_version: contentRef,
  state: closedEnum(["running", "completed", "refused"]),
  refusal: nullable(code),
  report_ref: nullable(contentRef),
  report: nullable(report),
});

/**
 * The schema's three conditionals: the state decides which of the three members are present. A member
 * present or absent against its state is `not_canonical` at that member: well typed, wrong for the state.
 */
const backtest: Decoder<Backtest> = (value, path) => {
  const read = served(value, path);
  if (!read.ok) return read;
  const b = read.value;
  const base = { backtest_id: b.backtest_id, mandate_version: b.mandate_version };
  const present = (member: "refusal" | "report_ref" | "report", want: boolean) => (b[member] === null) === want;
  for (const member of ["refusal", "report_ref", "report"] as const) {
    const want = b.state === "completed" ? member !== "refusal" : b.state === "refused" && member === "refusal";
    if (present(member, want)) return fail(`${path}/${member}`, "not_canonical");
  }
  switch (b.state) {
    case "running":
      return { ok: true, value: { ...base, state: "running", refusal: null, report_ref: null, report: null } };
    case "refused":
      return b.refusal === null ? fail(`${path}/refusal`, "not_canonical") : { ok: true, value: { ...base, state: "refused", refusal: b.refusal, report_ref: null, report: null } };
    case "completed":
      return b.report_ref === null || b.report === null ? fail(`${path}/report`, "not_canonical") : { ok: true, value: { ...base, state: "completed", refusal: null, report_ref: b.report_ref, report: b.report } };
    default: {
      const unhandled: never = b.state;
      throw new Error(`unhandled state ${String(unhandled)}`);
    }
  }
};

/** `AsOf`: one watermark for each stream the read reflects, at least one (spec §6.1, API-14). */
const asOf: Decoder<Watermark[]> = (value, path) => {
  const read = array(watermark)(value, path);
  if (!read.ok) return read;
  return read.value.length > 0 ? read : fail(path, "missing");
};

/** The body of `read-models/backtest`: the backtest and the streams it reflects (`AsOf`, at least one). */
export const backtestRead: Decoder<{ backtest: Backtest; as_of: Watermark[] }> = object({ backtest, as_of: asOf });

function invalid(issue: DecodeIssue): BacktestRead {
  return { ok: false, error: { code: "invalid_response", effect: "none", status: 200, event_id: null, retryable: true, violations: [], decode: issue } };
}

/** The answer is for the id asked, or it is refused: a screen never shows another backtest's report as this one. */
function forId(read: { backtest: Backtest }, backtestId: string): BacktestRead {
  const answered = read.backtest.backtest_id;
  return answered === backtestId ? { ok: true, backtest: read.backtest } : invalid({ path: "/backtest/backtest_id", problem: "not_canonical", value: answered, allowed: null });
}

/** Reads one backtest over the API; an answer for another id is refused. */
export async function readBacktestFrom(client: WorkspaceClient, backtestId: string): Promise<BacktestRead> {
  const outcome = await client.read(`/backtests/${encodeURIComponent(backtestId)}`, backtestRead);
  return outcome.ok ? forId(outcome.value, backtestId) : { ok: false, error: outcome.error };
}

/** A body held in the served shape (the fixtures), through the same decoder and the same id check. */
export function servedBacktest(body: unknown, backtestId: string): BacktestRead {
  const read = backtestRead(body, "");
  return read.ok ? forId(read.value, backtestId) : invalid(read.issue);
}

/** The answer to an id nothing holds: the API's 404 `not_found`. */
export function backtestNotFound(): BacktestRead {
  return { ok: false, error: { code: "not_found", effect: "none", status: 404, event_id: null, retryable: false, violations: [], decode: null } };
}
