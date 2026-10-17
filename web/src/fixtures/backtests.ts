/**
 * Fixture backtests, held in the shape `GET /backtests/{id}` serves (`read-models/backtest`, DEC-1001),
 * so the fixture runtime and the mock API pass them through the same decoder the API's answers go
 * through (DEC-2400). Fixture values, not a record: each report is a simulated run of a mandate
 * version, and none is a performance record.
 */
import { servedBacktest } from "@/api/backtest";
import { AGENT_IDS } from "./workspace";
import { INSTRUMENTS } from "./mandates";
import { VERSION } from "./versions";

export const BACKTEST_IDS = {
  /** The schema's own example: a research-agent version (`research_agent: true`). */
  research: "bt_01JB3K6F0C9R2V7N4M8Q1T5W3Z",
  /** The LMN core mandate's version in force, run to completion (a pinned universe). */
  lmnCompleted: "bt_01JBR2C4D6E8F0G2H4J6K8M0N2",
  /** The same version, run again and not finished. */
  lmnRunning: "bt_01JBR3P5Q7S9T1V3W5X7Y9Z1A3",
  /** The bitcoin mandate: crypto is refused until E4-4's slice 2 (DEC-1000 item 8). */
  btcRefused: "bt_01JBR4B6C8D0E2F4G6H8J0K2M4",
} as const;

/**
 * The backtests each fixture agent's Backtests section lists, newest first. The research-agent run is
 * on no agent: users do not see the research agent until the DEC-99 evaluation passes (PX-9 (a)); it
 * is served by its id, so its report still renders and is tested.
 */
export const BACKTESTS_BY_AGENT: Readonly<Record<string, readonly string[]>> = {
  [AGENT_IDS.btc]: [BACKTEST_IDS.btcRefused],
  [AGENT_IDS.swing]: [],
  [AGENT_IDS.lmn]: [BACKTEST_IDS.lmnRunning, BACKTEST_IDS.lmnCompleted],
};

const ENVELOPE = {
  api_version: "v1",
  build: `sha256:${"ab".repeat(32)}`,
  served_at: "2026-09-28T18:05:20.000000000Z",
  as_of: [{ stream_id: "ctl:ws_01J9ZQ4M0000000000000000AB", seq: 312, hash: `sha256:${"cd".repeat(32)}`, recorded_at: "2026-09-28T18:05:20.000000000Z" }],
};

const EVERY_LIMITATION_BUT_THESES = [
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
];

/** The schema example's metric block (`schemas/workspace-api/examples/read-models.backtest.json`). */
const EXAMPLE_METRICS = {
  period_count: 3,
  first_date: "2026-09-01",
  last_date: "2026-09-03",
  starting_equity: "10000",
  ending_equity: "10120.5",
  net_pnl: "120.5",
  total_return: "0.01205",
  return_sum: "0.012",
  return_sum_of_squares: "0.0001",
  mean_return: "0.004",
  variance: "0.00002",
  volatility: "0.004472135955",
  periods_per_year: 252,
  variance_annualized: "0.00504",
  volatility_annualized: "0.070992957397",
  risk_free_per_period: "0",
  sharpe_squared: "0.8",
  sharpe_sign: "positive",
  sharpe: "0.894427190999",
  sharpe_squared_annualized: "201.6",
  sharpe_annualized: "14.198591479439",
  max_drawdown: "0.002",
  max_drawdown_usd: "20.1",
  max_drawdown_peak_period: 1,
  max_drawdown_trough_period: 2,
  buy_notional: "5000",
  sell_notional: "0",
  traded_notional: "5000",
  fill_count: 1,
  turnover: "0.5",
  fees_total: "0.01",
  fees_charged: "0",
  fees_accrued: "0.01",
  fees_asset: "0",
  submitted_qty: "20",
  filled_qty: "20",
  absent: null,
};

const hold = (at: string, date: string, reason: string) => ({ at, date, purpose: null, outcome: "builder_hold", reason, decided_by: null, qty: null, limit: null, outputs_used: 1 });

/** The schema's example, as served: every outcome once, a research-agent version, all three disclosures. */
const RESEARCH = {
  ...ENVELOPE,
  backtest: {
    backtest_id: BACKTEST_IDS.research,
    mandate_version: `sha256:${"ef".repeat(32)}`,
    state: "completed",
    refusal: null,
    report_ref: `sha256:${"12".repeat(32)}`,
    report: {
      kind: "backtest_report",
      report_version: 2,
      subject: "mandate_version",
      mandate_version: `sha256:${"ef".repeat(32)}`,
      instrument: "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4",
      research_agent: true,
      inputs: { bars: `sha256:${"01".repeat(32)}`, model_outputs: `sha256:${"02".repeat(32)}`, theses: `sha256:${"03".repeat(32)}`, config: `sha256:${"04".repeat(32)}` },
      strategy: EXAMPLE_METRICS,
      benchmark: { ...EXAMPLE_METRICS, ending_equity: "10200", total_return: "0.02" },
      excess_total_return: "-0.00795",
      decisions: {
        count: 9,
        outcomes: { submitted: 1, builder_hold: 1, builder_refused: 1, gate_deny: 1, gate_defer: 1, gate_hold: 1, autonomy_deny: 1, ask_unanswered: 1, order_working: 1 },
        gate_reasons: {},
        hold_reasons: { at_or_above_target: 1 },
        rows: [
          { at: "2026-09-02T13:30:00.000000000Z", date: "2026-09-01", purpose: "open", outcome: "submitted", reason: null, decided_by: "rule:small_opening", qty: "20", limit: "250.05", outputs_used: 1 },
          hold("2026-09-03T13:30:00.000000000Z", "2026-09-02", "at_or_above_target"),
          { at: "2026-09-04T13:30:00.000000000Z", date: "2026-09-03", purpose: "increase", outcome: "builder_refused", reason: "crossed_quote", decided_by: null, qty: null, limit: null, outputs_used: 1 },
          { at: "2026-09-05T13:30:00.000000000Z", date: "2026-09-04", purpose: "increase", outcome: "gate_deny", reason: "position_cap", decided_by: null, qty: "4", limit: "251.1", outputs_used: 1 },
          { at: "2026-09-06T13:30:00.000000000Z", date: "2026-09-05", purpose: "discretionary_exit", outcome: "gate_defer", reason: "close_window", decided_by: null, qty: "24", limit: "249.9", outputs_used: 1 },
          { at: "2026-09-07T13:30:00.000000000Z", date: "2026-09-06", purpose: "risk_exit", outcome: "gate_hold", reason: "unknown_order", decided_by: null, qty: "24", limit: "249.9", outputs_used: 1 },
          { at: "2026-09-08T13:30:00.000000000Z", date: "2026-09-07", purpose: "open", outcome: "autonomy_deny", reason: null, decided_by: "rule:no_new_names", qty: "20", limit: "250.05", outputs_used: 1 },
          { at: "2026-09-09T13:30:00.000000000Z", date: "2026-09-08", purpose: "open", outcome: "ask_unanswered", reason: null, decided_by: "rule:large_opening", qty: "20", limit: "250.05", outputs_used: 1 },
          { at: "2026-09-10T13:30:00.000000000Z", date: "2026-09-09", purpose: null, outcome: "order_working", reason: null, decided_by: null, qty: null, limit: null, outputs_used: 1 },
        ],
      },
      risk: { limits_latched: ["drawdown_ladder[0]"], risk_exits: 0, most_restrictive_mode: "normal" },
      equity: { strategy: [{ date: "2026-09-01", equity: "10000" }], benchmark: [{ date: "2026-09-01", equity: "10000" }] },
      limitations: [...EVERY_LIMITATION_BUT_THESES, "mechanics_not_thesis_quality"],
      disclosures: ["LEGEND-HYPOTHETICAL", "NOTE-BACKTEST-MECHANICS", "NOTE-BACKTEST-LIMITS"],
    },
  },
};

const LMN_DATES = ["2026-09-14", "2026-09-15", "2026-09-16", "2026-09-17", "2026-09-18"];
/** Each decision is at the next trading day's first regular bar (DEC-1000 item 3), 09:30 in New York. */
const LMN_AT = ["2026-09-15", "2026-09-16", "2026-09-17", "2026-09-18", "2026-09-21"].map((d) => `${d}T13:30:00.000000000Z`);

/**
 * The LMN core mandate's version in force, over five sessions: one opening of 40 shares, one increase
 * the position cap denied, three holds; buy-and-hold puts the starting cash into 79 shares. The
 * statistics are recomputed from the closing equities at 12 places, as E4-2's report states them.
 */
const LMN_COMPLETED = {
  ...ENVELOPE,
  backtest: {
    backtest_id: BACKTEST_IDS.lmnCompleted,
    mandate_version: VERSION.lmn,
    state: "completed",
    refusal: null,
    report_ref: `sha256:${"5a".repeat(32)}`,
    report: {
      kind: "backtest_report",
      report_version: 2,
      subject: "mandate_version",
      mandate_version: VERSION.lmn,
      instrument: INSTRUMENTS.LMN.asset_id,
      research_agent: false,
      inputs: { bars: `sha256:${"a1".repeat(32)}`, model_outputs: `sha256:${"a2".repeat(32)}`, theses: `sha256:${"a3".repeat(32)}`, config: `sha256:${"a4".repeat(32)}` },
      strategy: {
        period_count: 5,
        first_date: "2026-09-14",
        last_date: "2026-09-18",
        starting_equity: "10000",
        ending_equity: "10042.18",
        net_pnl: "42.18",
        total_return: "0.004218",
        return_sum: "0.004218155433",
        return_sum_of_squares: "0.000018073218",
        mean_return: "0.000843631087",
        variance: "0.000003628663",
        volatility: "0.001904904985",
        periods_per_year: 252,
        variance_annualized: "0.000914423076",
        volatility_annualized: "0.030239429161",
        risk_free_per_period: "0",
        sharpe_squared: "0.196136541462",
        sharpe_sign: "positive",
        sharpe: "0.442873053324",
        sharpe_squared_annualized: "49.426408448424",
        sharpe_annualized: "7.030391770622",
        max_drawdown: "0.001913951912",
        max_drawdown_usd: "19.2",
        max_drawdown_peak_period: 1,
        max_drawdown_trough_period: 2,
        buy_notional: "5014",
        sell_notional: "0",
        traded_notional: "5014",
        fill_count: 1,
        turnover: "0.5014",
        fees_total: "0",
        fees_charged: "0",
        fees_accrued: "0",
        fees_asset: "0",
        submitted_qty: "40",
        filled_qty: "40",
        absent: null,
      },
      benchmark: {
        period_count: 5,
        first_date: "2026-09-14",
        last_date: "2026-09-18",
        starting_equity: "10000",
        ending_equity: "10105.4",
        net_pnl: "105.4",
        total_return: "0.01054",
        return_sum: "0.010506536726",
        return_sum_of_squares: "0.000043493166",
        mean_return: "0.002101307345",
        variance: "0.000005353926",
        volatility: "0.002313855225",
        periods_per_year: 252,
        variance_annualized: "0.001349189352",
        volatility_annualized: "0.036731312963",
        risk_free_per_period: "0",
        sharpe_squared: "0.824720505691",
        sharpe_sign: "positive",
        sharpe: "0.908141236451",
        sharpe_squared_annualized: "207.829567434132",
        sharpe_annualized: "14.416295204876",
        max_drawdown: "0.001440893553",
        max_drawdown_usd: "14.5",
        max_drawdown_peak_period: 1,
        max_drawdown_trough_period: 2,
        buy_notional: "9993.5",
        sell_notional: "0",
        traded_notional: "9993.5",
        fill_count: 1,
        turnover: "0.99935",
        fees_total: "0",
        fees_charged: "0",
        fees_accrued: "0",
        fees_asset: "0",
        submitted_qty: "79",
        filled_qty: "79",
        absent: null,
      },
      excess_total_return: "-0.006322",
      decisions: {
        count: 5,
        outcomes: { submitted: 1, builder_hold: 3, builder_refused: 0, gate_deny: 1, gate_defer: 0, gate_hold: 0, autonomy_deny: 0, ask_unanswered: 0, order_working: 0 },
        gate_reasons: { position_cap: 1 },
        hold_reasons: { at_or_above_target: 3 },
        rows: [
          { at: LMN_AT[0], date: LMN_DATES[0], purpose: "open", outcome: "submitted", reason: null, decided_by: "rule:small_opening", qty: "40", limit: "125.35", outputs_used: 1 },
          hold(LMN_AT[1], LMN_DATES[1], "at_or_above_target"),
          { at: LMN_AT[2], date: LMN_DATES[2], purpose: "increase", outcome: "gate_deny", reason: "position_cap", decided_by: null, qty: "8", limit: "125.9", outputs_used: 1 },
          hold(LMN_AT[3], LMN_DATES[3], "at_or_above_target"),
          hold(LMN_AT[4], LMN_DATES[4], "at_or_above_target"),
        ],
      },
      risk: { limits_latched: [], risk_exits: 0, most_restrictive_mode: "normal" },
      equity: {
        strategy: LMN_DATES.map((date, i) => ({ date, equity: ["10000", "10031.6", "10012.4", "10027.9", "10042.18"][i] })),
        benchmark: LMN_DATES.map((date, i) => ({ date, equity: ["10021", "10063.2", "10048.7", "10090.1", "10105.4"][i] })),
      },
      limitations: EVERY_LIMITATION_BUT_THESES,
      disclosures: ["LEGEND-HYPOTHETICAL", "NOTE-BACKTEST-LIMITS"],
    },
  },
};

const LMN_RUNNING = {
  ...ENVELOPE,
  backtest: { backtest_id: BACKTEST_IDS.lmnRunning, mandate_version: VERSION.lmn, state: "running", refusal: null, report_ref: null, report: null },
};

const BTC_REFUSED = {
  ...ENVELOPE,
  backtest: { backtest_id: BACKTEST_IDS.btcRefused, mandate_version: VERSION.btc, state: "refused", refusal: "asset_class_not_supported", report_ref: null, report: null },
};

/** Every fixture backtest's served body, by id. */
export const BACKTEST_FIXTURES: Readonly<Record<string, unknown>> = {
  [BACKTEST_IDS.research]: RESEARCH,
  [BACKTEST_IDS.lmnCompleted]: LMN_COMPLETED,
  [BACKTEST_IDS.lmnRunning]: LMN_RUNNING,
  [BACKTEST_IDS.btcRefused]: BTC_REFUSED,
};

/** A served body by id, or `undefined`; a copy, so nothing that reads it can change the fixture. */
export function servedBacktestFixture(backtestId: string): unknown {
  return Object.hasOwn(BACKTEST_FIXTURES, backtestId) ? structuredClone(BACKTEST_FIXTURES[backtestId]) : undefined;
}

/**
 * The backtest the deploy step shows for a version (B3): the fixtures' completed run of exactly that
 * version, or `null`. A research-agent run is never offered (PX-9 (a)).
 */
export function backtestOfVersion(version: string): string | null {
  for (const [id, body] of Object.entries(BACKTEST_FIXTURES)) {
    const read = servedBacktest(body, id);
    if (!read.ok) continue;
    const { backtest } = read;
    if (backtest.mandate_version === version && backtest.state === "completed" && !backtest.report.research_agent) return id;
  }
  return null;
}
