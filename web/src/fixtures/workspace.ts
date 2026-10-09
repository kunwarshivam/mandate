/**
 * The recorded fixture workspace and its scenarios. Figures are fixture values chosen to be
 * internally consistent (equity = allocation + realized + market value − cost basis) and to sit
 * inside every limit they are not meant to break; none is a performance record.
 */
import { add, dec, toFixed } from "@/lib/decimal";
import { BTC_HISTORY, LMN_HISTORY, SWING_HISTORY } from "./history";
import { INSTRUMENTS, btcAccumulator, lmnCore, provenance, twoStockSwing } from "./mandates";
import type { Agent, Approval, CancelReason, GateDecision, Health, Scenario, TimelineEvent, Workspace } from "./types";
import { BTC_RAISE_REJECTED, BTC_VERSIONS, LMN_VERSIONS, SWING_VERSIONS, VERSION } from "./versions";

export const NOW = "2026-09-28T14:05:20-04:00";

export const AGENT_IDS = {
  btc: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC",
  swing: "agt_01JB3K9P2H6SD4F8G1E3W7XYZB",
  lmn: "agt_01JB3KAQ5R8TV2N4M6P9S1W3XD",
} as const;

export const APPROVAL_IDS = {
  swingXyz: "apr_01JBM9S346Q3D25VT4F5V37E3S",
  btc: "apr_01JB3E28JT97KB6CQ643DZVMXX",
  lmn: "apr_01JBQKFBF5KZNWJ47TAN9ZT24M",
} as const;

const t = (hms: string, date = "2026-09-28") => `${date}T${hms}-04:00`;

/**
 * In the drawdown scenario BTC has fallen about 8% from the high-water mark by 14:01, so the normal
 * scenario's buy resting at $55,900 would have filled on the way down; it rests at this limit, below
 * the price's path, which keeps the fixture's bars on one path and off a cliff (`market.test.ts`).
 */
const DRAWDOWN_RESTING_LIMIT = "51700";

const btc: Agent = {
  agent_id: AGENT_IDS.btc,
  label: "Agent 1",
  mandate: btcAccumulator,
  mandate_version: VERSION.btc,
  provenance: provenance["btc-accumulator"],
  mode: "normal",
  restrictions: [],
  startup: "ready",
  pnl_total: "123.45",
  pnl_today: "73.45",
  realized_pnl: "-24.56",
  state: {
    equity: "10123.45",
    equity_day_start: "10050",
    high_water_mark: "10150",
    capital_base: "10000",
    inherited_loss: "0",
    orders_today: 3,
    size_factor: "1",
    pending: [],
  },
  positions: [
    {
      instrument: INSTRUMENTS.BTC,
      qty: "0.12",
      broker_qty: "0.12",
      avg_cost: "55555.55",
      mark: "56789.01",
      mark_as_of: t("14:05:18"),
      market_value: "6814.68",
      unrealized_pnl: "148.01",
      protection: { kind: "crypto_stop_limit", stop_price: "51111.11", limit_price: "50855.55", take_profit_price: null, unprotected_fraction: null },
    },
  ],
  orders: [
    {
      client_order_id: "cid_01JBH3BV4H15G5E4G7X0NTH82F",
      instrument: INSTRUMENTS.BTC,
      side: "buy",
      qty: "0.01",
      filled_qty: "0",
      limit_price: "55900",
      stop_price: null,
      purpose: "increase",
      state: "Accepted",
      submitted_at: t("13:12:09"),
      time_in_force: "gtc",
    },
    {
      client_order_id: "cid_01JB7AG3BCKKDJWBHP1G201CYF",
      instrument: INSTRUMENTS.BTC,
      side: "sell",
      qty: "0.12",
      filled_qty: "0",
      limit_price: "50855.55",
      stop_price: "51111.11",
      purpose: "protective",
      state: "Accepted",
      submitted_at: t("09:44:31", "2026-09-24"),
      time_in_force: "gtc",
    },
  ],
  ...BTC_HISTORY,
  goal_progress: { spent_usd: "6666.67", held_qty: "0.12" },
  deployed_at: t("09:30:00", "2026-09-21"),
  versions: BTC_VERSIONS,
};

const swing: Agent = {
  agent_id: AGENT_IDS.swing,
  label: "Agent 2",
  mandate: twoStockSwing,
  mandate_version: VERSION.swing,
  provenance: provenance["two-stock-swing"],
  mode: "normal",
  restrictions: [],
  startup: "ready",
  pnl_total: "-67.89",
  pnl_today: "-17.89",
  realized_pnl: "-72.18",
  state: {
    equity: "9932.11",
    equity_day_start: "9950",
    high_water_mark: "10040",
    capital_base: "10000",
    inherited_loss: "0",
    orders_today: 7,
    size_factor: "1",
    pending: [],
  },
  positions: [
    {
      instrument: INSTRUMENTS.XYZ,
      qty: "8",
      broker_qty: "8",
      avg_cost: "140",
      mark: "141.23",
      mark_as_of: t("14:05:17"),
      market_value: "1129.84",
      unrealized_pnl: "9.84",
      protection: { kind: "bracket", stop_price: "133", limit_price: null, take_profit_price: "154", unprotected_fraction: null },
    },
    {
      instrument: INSTRUMENTS.QRS,
      qty: "5",
      broker_qty: "5",
      avg_cost: "98.76",
      mark: "97.65",
      mark_as_of: t("14:05:16"),
      market_value: "488.25",
      unrealized_pnl: "-5.55",
      protection: { kind: "bracket", stop_price: "93.82", limit_price: null, take_profit_price: "108.64", unprotected_fraction: null },
    },
  ],
  orders: [
    {
      client_order_id: "cid_01JBW6VZSKDENC8SP3804GVA35",
      instrument: INSTRUMENTS.XYZ,
      side: "sell",
      qty: "8",
      filled_qty: "0",
      limit_price: null,
      stop_price: "133",
      purpose: "protective",
      state: "Accepted",
      submitted_at: t("10:02:18", "2026-09-25"),
      time_in_force: "gtc",
    },
    {
      client_order_id: "cid_01JBRJFJ2XBAHW0GQNMF2KDPB0",
      instrument: INSTRUMENTS.QRS,
      side: "sell",
      qty: "5",
      filled_qty: "0",
      limit_price: null,
      stop_price: "93.82",
      purpose: "protective",
      state: "Accepted",
      submitted_at: t("10:12:51"),
      time_in_force: "gtc",
    },
  ],
  ...SWING_HISTORY,
  goal_progress: null,
  deployed_at: t("09:30:00", "2026-09-22"),
  versions: SWING_VERSIONS,
};

const lmn: Agent = {
  agent_id: AGENT_IDS.lmn,
  label: "Agent 3",
  mandate: lmnCore,
  mandate_version: VERSION.lmn,
  provenance: provenance["lmn-core"],
  mode: "normal",
  restrictions: [],
  startup: "ready",
  pnl_total: "10",
  pnl_today: "0",
  realized_pnl: "10",
  state: {
    equity: "5010",
    equity_day_start: "5010",
    high_water_mark: "5025",
    capital_base: "5000",
    inherited_loss: "0",
    orders_today: 0,
    size_factor: "1",
    pending: [],
  },
  positions: [],
  orders: [],
  ...LMN_HISTORY,
  goal_progress: null,
  deployed_at: t("09:30:00", "2026-09-23"),
  versions: LMN_VERSIONS,
};

const pendingSwing: Approval = {
  approval_id: APPROVAL_IDS.swingXyz,
  agent_id: AGENT_IDS.swing,
  status: "delivered",
  requested_at: t("14:04:58"),
  deadline: t("14:14:58"),
  bound: {
    symbol: "XYZ",
    asset_class: "us_equity",
    side: "buy",
    qty: "2",
    limit: "141.3",
    purpose: "increase",
    mandate_version: swing.mandate_version,
    decided_by: "rule:low_score",
    combined_score: "0.574",
  },
  trigger: "Your rule: ask when the combined model score is below 0.65.",
  risk_impact: [
    { field: "order_usd", value: "282.6", cap: "1000" },
    { field: "position_usd_after", value: "1412.44", cap: "1500" },
    { field: "gross_usd_after", value: "1900.69", cap: "2000" },
    { field: "bought_today_usd", value: "282.6", cap: null },
  ],
  evidence: [
    {
      author: "owner_selected",
      model_id: "quant.momentum",
      version: "1.0.0",
      produced_at: t("14:04:51"),
      lines: ["score: 0.71 (weight 0.6)", "lookback_bars: 50", "close above its 50-bar average for 6 bars"],
    },
    {
      author: "owner_selected",
      model_id: "llm.news_research",
      version: "0.3.0",
      produced_at: t("14:01:12"),
      lines: ["score: 0.37 (weight 0.4)", "two news items about XYZ in the last hour", "tone of the items: mixed"],
    },
  ],
  approvers_required: 1,
  approvals_so_far: [],
};

const pendingBtc: Approval = {
  approval_id: APPROVAL_IDS.btc,
  agent_id: AGENT_IDS.btc,
  status: "delivered",
  requested_at: t("14:03:40"),
  deadline: t("14:13:40"),
  bound: {
    symbol: "BTC/USD",
    asset_class: "crypto",
    side: "buy",
    qty: "0.015",
    limit: "56700",
    purpose: "increase",
    mandate_version: btc.mandate_version,
    decided_by: "rule:low_score",
    combined_score: "0.61",
  },
  trigger: "Your rule: ask when the combined model score is below 0.65.",
  risk_impact: [
    { field: "order_usd", value: "850.5", cap: "1000" },
    { field: "position_usd_after", value: "8224.18", cap: "10000" },
    { field: "gross_usd_after", value: "8224.18", cap: "10000" },
    { field: "bought_today_usd", value: "1409.5", cap: null },
  ],
  evidence: [
    {
      author: "owner_selected",
      model_id: "quant.mean_reversion",
      version: "1.0.0",
      produced_at: t("14:03:31"),
      lines: ["score: 0.61 (weight 1)", "lookback_bars: 20, z_entry: 1.5", "z-score of the last close: −1.62"],
    },
  ],
  approvers_required: 1,
  approvals_so_far: [],
};

const pendingLmn: Approval = {
  approval_id: APPROVAL_IDS.lmn,
  agent_id: AGENT_IDS.lmn,
  status: "delivered",
  requested_at: t("14:01:05"),
  deadline: t("14:16:05"),
  bound: {
    symbol: "LMN",
    asset_class: "us_equity",
    side: "buy",
    qty: "10",
    limit: "45.67",
    purpose: "open",
    mandate_version: lmn.mandate_version,
    decided_by: "rule:low_score",
    combined_score: "0.52",
  },
  trigger: "Your rule: ask when the combined model score is below 0.65. Orders above $400 need two approvers.",
  risk_impact: [
    { field: "order_usd", value: "456.7", cap: "1000" },
    { field: "position_usd_after", value: "456.7", cap: "2500" },
    { field: "gross_usd_after", value: "456.7", cap: "2500" },
    { field: "bought_today_usd", value: "456.7", cap: null },
  ],
  evidence: [
    {
      author: "owner_selected",
      model_id: "quant.momentum",
      version: "1.0.0",
      produced_at: t("14:00:58"),
      lines: ["score: 0.52 (weight 1)", "lookback_bars: 50", "close crossed its 50-bar average 2 bars ago"],
    },
  ],
  approvers_required: 2,
  approvals_so_far: [{ user_label: "Priya (approver)", at: t("14:02:31") }],
};

const resolved: Approval[] = [
  {
    ...pendingSwing,
    approval_id: "apr_01JBNPZX45HY43KWJRP1XPA7Z3",
    status: "acted",
    requested_at: t("10:08:02"),
    deadline: t("10:18:02"),
    bound: { ...pendingSwing.bound, symbol: "QRS", qty: "5", limit: "98.76", purpose: "open", combined_score: "0.62" },
    risk_impact: [{ field: "order_usd", value: "493.8", cap: "1000" }],
    resolution: { at: t("10:12:44"), text: "Approved by you. The gate re-ran and allowed it; the order was submitted and filled 5 at $98.76." },
  },
  {
    ...pendingBtc,
    approval_id: "apr_01JBQY77ZXYYK596NGYA1DQ91K",
    status: "rejected",
    requested_at: t("11:15:40"),
    deadline: t("11:25:40"),
    bound: { ...pendingBtc.bound, qty: "0.01", limit: "56100", combined_score: "0.58" },
    risk_impact: [{ field: "order_usd", value: "561", cap: "1000" }],
    resolution: { at: t("11:20:03"), text: "Skipped by you. Nothing was sent." },
  },
  {
    ...pendingBtc,
    approval_id: "apr_01JBDJ8FSSZ5AWSH8VHTPRE95B",
    status: "expired",
    requested_at: t("08:31:10"),
    deadline: t("08:41:10"),
    bound: { ...pendingBtc.bound, qty: "0.015", limit: "55400", combined_score: "0.6" },
    risk_impact: [{ field: "order_usd", value: "831", cap: "1000" }],
    resolution: { at: t("08:41:10"), text: "Skipped at the deadline. Nothing was sent." },
  },
  {
    ...pendingSwing,
    approval_id: "apr_01JB9EE0ZBGJ09TQM83XSSSS6Y",
    status: "gate_skipped",
    requested_at: t("15:44:30", "2026-09-25"),
    deadline: t("15:54:30", "2026-09-25"),
    bound: { ...pendingSwing.bound, qty: "3", limit: "139.1", combined_score: "0.55" },
    risk_impact: [{ field: "order_usd", value: "417.3", cap: "1000" }],
    resolution: {
      at: t("15:50:12", "2026-09-25"),
      text: "Approved by you. The gate re-ran and did not allow it: no opening orders in the last 10 minutes of the regular session. Nothing was sent.",
    },
  },
  {
    ...pendingLmn,
    approval_id: "apr_01JBS3C4DWA7N36096Q14DR9GP",
    status: "superseded",
    requested_at: t("08:58:21"),
    deadline: t("09:13:21"),
    bound: { ...pendingLmn.bound, qty: "12", limit: "44.9", combined_score: "0.49", mandate_version: VERSION.lmnFirst },
    trigger: pendingSwing.trigger,
    risk_impact: [{ field: "order_usd", value: "538.8", cap: "1000" }],
    approvers_required: 1,
    approvals_so_far: [],
    resolution: {
      at: t("09:05:00"),
      text: "Canceled: a new mandate version applied. Anything still wanted is proposed again under the new version.",
      cancel_reason: "version_applied",
    },
  },
];

const decisions: GateDecision[] = [
  {
    event_id: "01JB5GQAPENECFSECZP11HYGCP",
    at: t("14:04:58"),
    agent_id: AGENT_IDS.swing,
    verdict: "allow",
    reason_code: null,
    action: { side: "buy", qty: "2", symbol: "XYZ", limit_price: "141.3", purpose: "increase" },
    then: "Asked you for approval (your rule “low_score”).",
    approval_id: APPROVAL_IDS.swingXyz,
  },
  {
    event_id: "01JBWPQ5E6EYCNDY0YP57RCYBV",
    at: t("13:40:02"),
    agent_id: AGENT_IDS.btc,
    verdict: "deny",
    reason_code: "max_order_size",
    action: { side: "buy", qty: "0.02", symbol: "BTC/USD", limit_price: "56650", purpose: "increase" },
  },
  {
    event_id: "01JB8XE6SZAEAVSNTCPM5Q1NXW",
    at: t("13:12:09"),
    agent_id: AGENT_IDS.btc,
    verdict: "allow",
    reason_code: null,
    action: { side: "buy", qty: "0.01", symbol: "BTC/USD", limit_price: "55900", purpose: "increase" },
    then: "Submitted without asking (your rule “routine”); resting at the broker.",
    client_order_id: "cid_01JBH3BV4H15G5E4G7X0NTH82F",
  },
  {
    event_id: "01JBN5SXS5AA819X9YP981068V",
    at: t("12:31:15"),
    agent_id: AGENT_IDS.lmn,
    verdict: "deny",
    reason_code: "reentry_cooldown",
    action: { side: "buy", qty: "11", symbol: "LMN", limit_price: "45.1", purpose: "open" },
  },
  {
    event_id: "01JBYFGCW8T7SWM4FV4DK79Q9G",
    at: t("10:12:44"),
    agent_id: AGENT_IDS.swing,
    verdict: "allow",
    reason_code: null,
    action: { side: "buy", qty: "5", symbol: "QRS", limit_price: "98.76", purpose: "open" },
    then: "Approved by you; submitted and filled.",
    client_order_id: "cid_01JCGPZ78Y1223KHAFF3AMAPB9",
  },
  {
    event_id: "01JBCD1GDJFMGT83PXT891WB09",
    at: t("09:27:10"),
    agent_id: AGENT_IDS.swing,
    verdict: "deny",
    reason_code: "extended_hours_opening_not_allowed",
    action: { side: "buy", qty: "2", symbol: "XYZ", limit_price: "139.8", purpose: "increase" },
  },
  {
    event_id: "01JBB9Y73MY63FCH26W14WMCHW",
    at: t("08:02:44"),
    agent_id: AGENT_IDS.swing,
    verdict: "defer",
    reason_code: "discretionary_exit_regular_session_only",
    action: { side: "sell", qty: "2", symbol: "QRS", limit_price: "97.9", purpose: "discretionary_exit" },
  },
];

const timeline: Record<string, TimelineEvent[]> = {
  [AGENT_IDS.btc]: [
    { event_id: "01JB1RNJ47E65GH2BH8VGS9ZM5", at: t("13:40:02"), kind: "gate", text: "Buy 0.02 BTC/USD not allowed: larger than the $1,000.00 order limit." },
    { event_id: "01JBMZ9J92V81E5128Q6RW31FZ", at: t("13:12:09"), kind: "order", text: "Buy 0.01 BTC/USD at $55,900.00 submitted; resting." },
    { event_id: "01JBG0X454YG4GFDEXZR4YJ2C4", at: t("11:20:03"), kind: "approval", text: "You skipped buy 0.01 BTC/USD." },
    { event_id: "01JB9NGK80Y3ZH6DZJJXXX7CK5", at: t("08:41:10"), kind: "approval", text: "Buy 0.015 BTC/USD skipped at the deadline." },
    { event_id: "01JBY1JX4WHRDD459GQ8H7QEZZ", at: t("09:44:31", "2026-09-24"), kind: "protection", text: "Stop-limit placed: 0.12 BTC/USD, stop $51,111.11, limit $50,855.55." },
  ],
  [AGENT_IDS.swing]: [
    { event_id: "01JBS1A0ZWSK9TPRM7N0MNS7C0", at: t("14:04:58"), kind: "approval", text: "Asked you to approve buy 2 XYZ at $141.30." },
    { event_id: "01JBJGQ4SR4QVH3H63J9FHVMCQ", at: t("10:12:51"), kind: "protection", text: "Bracket placed for 5 QRS: stop $93.82, take-profit $108.64." },
    { event_id: "01JBV1SD53TW8JZ38AYTNJKGGS", at: t("10:12:47"), kind: "fill", text: "Filled buy 5 QRS at $98.76." },
    { event_id: "01JBFKYS7AA4DZEWNWV8CF5BN5", at: t("09:27:10"), kind: "gate", text: "Buy 2 XYZ not allowed: opening orders only in the regular session." },
    { event_id: "01JBMFQGC1TRTDRHN3ZHQ8D5HF", at: t("08:02:44"), kind: "gate", text: "Sell 2 QRS waiting for the regular session." },
    { event_id: "01JBK3W5TQ8N2C6X4R7V9Y1ZAB", at: t("08:15:04", "2026-09-25"), kind: "version", text: "Mandate version applied at the next safe point; no approvals were waiting." },
  ],
  [AGENT_IDS.lmn]: [
    { event_id: "01JBRSWVK182VYZ04SXWF6E996", at: t("12:31:15"), kind: "gate", text: "Buy 11 LMN not allowed: within the 1 h re-entry cooldown after an exit." },
    { event_id: "01JBX5208E2K8GV764KCRGE00K", at: t("11:31:02"), kind: "fill", text: "Filled sell 20 LMN at $45.42 (signal exit)." },
    { event_id: "01JBXHMFYFF1TK31CZT5GEVQEZ", at: t("09:05:00"), kind: "version", text: "Mandate version applied; one pending approval was canceled." },
  ],
};

const healthy: Health = {
  market_data: { state: "ok", as_of: t("14:05:18") },
  broker: { state: "ok", as_of: t("14:05:15") },
  deployment: { state: "ok", as_of: t("14:05:20") },
  relay: { state: "ok", as_of: t("14:05:10") },
};

/** Cash and the owner's own ABC shares: the part of the broker's equity that no agent manages. */
const UNMANAGED_EQUITY = "3412.8";

function brokerEquity(agents: Agent[]): string {
  return toFixed(add(dec(UNMANAGED_EQUITY), ...agents.map((a) => dec(a.state.equity))), 2);
}

function base(scenario: Scenario): Workspace {
  return structuredClone({
    scenario,
    status: "ready",
    journal: "answers",
    now: NOW,
    environment: "paper",
    connection: { connection_id: "con_01JB3K7M9Q2W4E6R8T0Y1V3X5P", broker: "Alpaca paper", account_equity: brokerEquity([btc, swing, lmn]), day_trading_regime: "intraday_margin" },
    approver_users: 2,
    health: healthy,
    agents: [btc, swing, lmn],
    approvals: [pendingSwing, ...resolved],
    decisions,
    timeline,
    external_positions: [{ instrument: INSTRUMENTS.ABC, qty: "20" }],
  } satisfies Workspace);
}

function agent(ws: Workspace, id: string): Agent {
  const found = ws.agents.find((a) => a.agent_id === id);
  if (!found) throw new Error(`fixture agent ${id} missing`);
  return found;
}

function supersede(ws: Workspace, approvalId: string, at: string, text: string, reason: CancelReason) {
  const approval = ws.approvals.find((a) => a.approval_id === approvalId);
  if (!approval) return;
  approval.status = "superseded";
  approval.resolution = { at, text, cancel_reason: reason };
}

export const SCENARIOS: Array<{ id: Scenario; label: string }> = [
  { id: "normal", label: "Normal" },
  { id: "empty", label: "Empty workspace" },
  { id: "loading", label: "Loading" },
  { id: "stale", label: "Stale data" },
  { id: "paused", label: "Agent paused" },
  { id: "drawdown", label: "Drawdown selling-only" },
  { id: "reconciliation", label: "Reconciliation hold" },
  { id: "unknown-order", label: "Unknown order" },
  { id: "unreachable", label: "Deployment unreachable" },
  { id: "approvals", label: "Pending approvals" },
  { id: "result-unknown", label: "Result unknown" },
];

export function isScenario(value: unknown): value is Scenario {
  return SCENARIOS.some((s) => s.id === value);
}

export function buildWorkspace(scenario: Scenario = "normal"): Workspace {
  const ws = base(scenario);
  switch (scenario) {
    case "normal":
      return ws;
    case "empty":
      return { ...ws, agents: [], approvals: [], decisions: [], timeline: {}, external_positions: [] };
    case "loading":
      return { ...ws, status: "loading", agents: [], approvals: [], decisions: [], timeline: {} };
    case "unreachable":
      return {
        ...ws,
        status: "unreachable",
        health: { ...ws.health, deployment: { state: "down", as_of: t("13:58:02") }, broker: { state: "stale", as_of: t("13:58:02") } },
        agents: [],
        approvals: [],
        decisions: [],
        timeline: {},
        external_positions: [],
      };
    case "stale": {
      const staleAt = t("14:02:11");
      ws.health.market_data = { state: "stale", as_of: staleAt };
      ws.health.relay = { state: "down", as_of: t("13:51:40") };
      for (const a of ws.agents) for (const p of a.positions) p.mark_as_of = staleAt;
      const s = agent(ws, AGENT_IDS.swing);
      s.restrictions = [
        { code: "stale_mark", since: t("14:03:11"), symbol: "XYZ" },
        { code: "stale_mark", since: t("14:03:11"), symbol: "QRS" },
      ];
      ws.decisions.unshift({
        event_id: "01JB2NTQSC0J4DZCKCEXEGJ6ZB",
        at: t("14:04:40"),
        agent_id: AGENT_IDS.swing,
        verdict: "deny",
        reason_code: "stale_mark",
        action: { side: "buy", qty: "1", symbol: "QRS", limit_price: "97.7", purpose: "increase" },
      });
      return ws;
    }
    case "paused": {
      const s = agent(ws, AGENT_IDS.swing);
      s.mode = "paused";
      s.restrictions = [{ code: "owner_pause", since: t("14:03:02") }];
      supersede(ws, APPROVAL_IDS.swingXyz, t("14:03:02"), "Canceled: you paused the agent.", "owner_pause");
      ws.decisions = ws.decisions.filter((d) => d.event_id !== "01JB5GQAPENECFSECZP11HYGCP");
      return ws;
    }
    case "drawdown": {
      const b = agent(ws, AGENT_IDS.btc);
      b.mode = "exits_only";
      b.restrictions = [
        { code: "drawdown_scale_sizes", since: t("15:12:40", "2026-09-26") },
        { code: "drawdown_exits_only", since: t("14:01:12") },
      ];
      b.pnl_total = "-470";
      b.pnl_today = "-70";
      b.state = { ...b.state, equity: "9530", equity_day_start: "9600", size_factor: "0.5" };
      b.positions[0] = { ...b.positions[0], mark: "51843.58", market_value: "6221.23", unrealized_pnl: "-445.44" };
      ws.connection.account_equity = brokerEquity(ws.agents);
      const canceled = b.orders.filter((o) => o.purpose !== "protective");
      b.orders = b.orders.filter((o) => o.purpose === "protective");
      b.past_orders.unshift(
        ...canceled.map((o) => ({ ...o, limit_price: DRAWDOWN_RESTING_LIMIT, state: "Canceled" as const, closed_at: t("14:01:12"), note: "Canceled on entering selling only." })),
      );
      for (const d of ws.decisions) {
        if (d.agent_id === AGENT_IDS.btc && d.client_order_id && canceled.some((o) => o.client_order_id === d.client_order_id)) d.action = { ...d.action, limit_price: DRAWDOWN_RESTING_LIMIT };
      }
      for (const e of ws.timeline[AGENT_IDS.btc]) e.text = e.text.replace("$55,900.00", "$51,700.00");
      ws.decisions.unshift({
        event_id: "01JBEZT39S3D19T33BSWM75ANC",
        at: t("14:04:30"),
        agent_id: AGENT_IDS.btc,
        verdict: "deny",
        reason_code: "agent_exits_only",
        action: { side: "buy", qty: "0.01", symbol: "BTC/USD", limit_price: "51700", purpose: "increase" },
      });
      b.versions = [...b.versions, BTC_RAISE_REJECTED];
      ws.timeline[AGENT_IDS.btc].unshift(
        { event_id: "01JBF6P2M8XKQ4D7N9R3T5W1YC", at: t("14:03:34"), kind: "version", text: "Mandate version rejected: an allocation increase is refused while a limit is latched." },
        { event_id: "01JBBX2KRQNWA605H5PT7DRPKV", at: t("14:01:12"), kind: "mode", text: "Mode changed from trading to selling only: drawdown reached 6% of the high-water mark." },
        { event_id: "01JB53YCQWCMQY1TFS2R2X43GC", at: t("14:01:12"), kind: "order", text: "Buy 0.01 BTC/USD canceled on entering selling only." },
      );
      return ws;
    }
    case "reconciliation": {
      const s = agent(ws, AGENT_IDS.swing);
      s.mode = "paused";
      s.startup = "reconciling";
      s.restrictions = [{ code: "startup_reconciliation", since: t("14:04:40") }];
      supersede(ws, APPROVAL_IDS.swingXyz, t("14:04:40"), "Canceled: the agent restarted and is checking with the broker.", "mode_tightened");
      ws.timeline[AGENT_IDS.swing].unshift({
        event_id: "01JB4NQHN2GMHK041E6YXRGVZ8",
        at: t("14:04:40"),
        kind: "reconciliation",
        text: "Runtime restarted; checking the ledger against the broker before any new order.",
      });
      return ws;
    }
    case "unknown-order": {
      const s = agent(ws, AGENT_IDS.swing);
      s.orders.unshift({
        client_order_id: "cid_01JBZB0K9FMMXQ5CSAFT42YMAV",
        instrument: INSTRUMENTS.QRS,
        side: "buy",
        qty: "3",
        filled_qty: "0",
        limit_price: "97.5",
        stop_price: null,
        purpose: "increase",
        state: "Unknown",
        submitted_at: t("14:04:51"),
        time_in_force: "day",
      });
      ws.decisions.unshift({
        event_id: "01JB64G5D6TZWBE8TXF7JJHHQG",
        at: t("14:05:02"),
        agent_id: AGENT_IDS.swing,
        verdict: "deny",
        reason_code: "unknown_order_in_flight",
        action: { side: "sell", qty: "5", symbol: "QRS", limit_price: "97.6", purpose: "discretionary_exit" },
      });
      ws.timeline[AGENT_IDS.swing].unshift({
        event_id: "01JBM9S346Q3D25VT4F5V37E3T",
        at: t("14:04:57"),
        kind: "order",
        text: "Buy 3 QRS: no answer from the broker. Order state unknown; checking.",
      });
      return ws;
    }
    case "approvals":
      ws.approvals = [pendingSwing, pendingBtc, pendingLmn, ...resolved];
      return ws;
    case "result-unknown":
      return { ...ws, journal: "silent" };
    default: {
      const unhandled: never = scenario;
      throw new Error(`unhandled scenario ${String(unhandled)}`);
    }
  }
}

export function findAgent(ws: Workspace, id: string): Agent | undefined {
  return ws.agents.find((a) => a.agent_id === id);
}

export function findApproval(ws: Workspace, id: string): Approval | undefined {
  return ws.approvals.find((a) => a.approval_id === id);
}

export function openApprovals(ws: Workspace): Approval[] {
  return ws.approvals
    .filter((a) => a.status === "delivered")
    .sort((a, b) => Date.parse(a.deadline) - Date.parse(b.deadline));
}
