import { describe, expect, it } from "vitest";
import type { AgentHolding } from "@/api/activity";
import type { CatalogueInstrument } from "@/api/catalogue";
import type { AgentDetail } from "@/api/read-models";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import type { AccountSnapshot, AgentPnl, Dashboard, EquityHistory } from "@/api/portfolio";
import { accountFigures, accountLine, apiPortfolio, breakdown, byAgent, byAssetClass, byInstrument, cashAndInvested, fixturePortfolio, grossPnl, lineChange, sumReported } from "./portfolio";

/**
 * The Portfolio page's figures (DEC-2413). Every expected value is an exact decimal string worked by
 * hand from the fixture's own figures, so a float anywhere on the path shows as a wrong last digit.
 */

const href = (id: string) => `/agents/${id}`;

describe("the fixture portfolio", () => {
  const p = fixturePortfolio(buildWorkspace("normal"));

  it("sums the account from the agents' decimals", () => {
    // Positions: BTC/USD 6814.68 (Agent 1), XYZ 1129.84 and QRS 488.25 (Agent 2); Agent 3 holds nothing.
    expect(p.invested).toBe("8432.77");
    // Equity 10123.45 + 9932.11 + 5010 = 25065.56, less what is invested.
    expect(p.cash).toBe("16632.79");
    // The broker's 28478.36 less the agents' 25065.56.
    expect(p.unmanaged).toBe("3412.80");
    expect(p.total).toBe("28478.36");
    // 73.45 − 17.89 + 0, and 123.45 − 67.89 + 10.
    expect(p.today).toBe("55.56");
    expect(p.pnl).toBe("65.56");
    // 148.01 + 9.84 − 5.55.
    expect(p.unrealized).toBe("152.30");
    expect(p.environment).toBe("paper");
  });

  it("keeps each holding's own figures and the agent holding it", () => {
    expect(p.holdings.map((h) => [h.symbol, h.agentName, h.qty, h.avgCost, h.last, h.marketValue, h.unrealized])).toEqual([
      ["BTC/USD", "Agent 1", "0.12", "55555.55", "56789.01", "6814.68", "148.01"],
      ["XYZ", "Agent 2", "8", "140", "141.23", "1129.84", "9.84"],
      ["QRS", "Agent 2", "5", "98.76", "97.65", "488.25", "-5.55"],
    ]);
  });

  it("breaks down what is invested by agent, largest first, each linking to its agent", () => {
    const b = byAgent(p, href);
    expect(b.total).toBe("8432.77");
    expect(b.slices.map((s) => [s.label, s.value, s.share, s.href])).toEqual([
      ["Agent 1", "6814.68", "0.8081", href(AGENT_IDS.btc)],
      ["Agent 2", "1618.09", "0.1919", href(AGENT_IDS.swing)],
      ["Agent 3", "0.00", "0.0000", href(AGENT_IDS.lmn)],
    ]);
  });

  it("breaks it down by instrument, each with a series colour by rank", () => {
    expect(byInstrument(p).slices.map((s) => [s.label, s.value, s.share, s.tone])).toEqual([
      ["BTC/USD", "6814.68", "0.8081", "series-1"],
      ["XYZ", "1129.84", "0.1340", "series-2"],
      ["QRS", "488.25", "0.0579", "series-3"],
    ]);
  });

  it("breaks it down by asset class, in neutral bars", () => {
    expect(byAssetClass(p).slices.map((s) => [s.label, s.value, s.share, s.tone])).toEqual([
      ["Crypto", "6814.68", "0.8081", "neutral"],
      ["US equities", "1618.09", "0.1919", "neutral"],
    ]);
  });

  it("splits the account into cash, invested and what no agent manages, the three summing to the account", () => {
    const b = cashAndInvested(p);
    expect(b.total).toBe("28478.36");
    expect(b.slices.map((s) => [s.label, s.value, s.share])).toEqual([
      ["Agents' cash", "16632.79", "0.5841"],
      ["Invested", "8432.77", "0.2961"],
      ["Not managed by an agent", "3412.80", "0.1198"],
    ]);
  });

  it("leaves out what no agent manages when there is none", () => {
    const b = cashAndInvested({ ...p, unmanaged: "0.00" });
    expect(b.slices.map((s) => s.key)).toEqual(["cash", "invested"]);
  });

  it("has no agents and no holdings in an empty workspace", () => {
    const empty = fixturePortfolio(buildWorkspace("empty"));
    expect(empty.agents).toEqual([]);
    expect(empty.holdings).toEqual([]);
    expect(empty.invested).toBe("0.00");
    expect(byInstrument(empty).slices).toEqual([]);
  });
});

describe("a breakdown", () => {
  it("shows no share when a part is not reported, since the whole is not known", () => {
    const b = breakdown([
      { key: "a", label: "A", value: "10" },
      { key: "b", label: "B", value: null },
    ]);
    expect(b.total).toBeNull();
    expect(b.slices.map((s) => [s.key, s.share])).toEqual([
      ["a", null],
      ["b", null],
    ]);
  });

  it("rounds each share half away from zero to four places, in fixed point", () => {
    // 1/3 and 2/3 of 0.3: a float would give 0.1 + 0.2 = 0.30000000000000004 for the whole.
    const b = breakdown([
      { key: "a", label: "A", value: "0.1" },
      { key: "b", label: "B", value: "0.2" },
    ]);
    expect(b.total).toBe("0.30");
    expect(b.slices.map((s) => s.share)).toEqual(["0.6667", "0.3333"]);
  });

  it("gives a zero whole zero shares rather than dividing by it", () => {
    expect(breakdown([{ key: "a", label: "A", value: "0" }]).slices[0].share).toBe("0.0000");
  });

  it("orders equal values by label, and a value not reported last", () => {
    const b = breakdown([
      { key: "n", label: "N", value: null },
      { key: "z", label: "Z", value: "5" },
      { key: "a", label: "A", value: "5" },
    ]);
    expect(b.slices.map((s) => s.key)).toEqual(["a", "z", "n"]);
  });

  it("sums to null when any value is not reported, and an empty list to zero", () => {
    expect(sumReported(["1.10", null])).toBeNull();
    expect(sumReported([])).toBe("0.00");
    expect(sumReported(["0.1", "0.2"])).toBe("0.30");
  });
});

const ASSET = "7b4a1c2e-1111-4a2b-9c3d-000000000001";

const detail = (over: Partial<AgentDetail>): AgentDetail => ({
  agent_id: "agt_1",
  environment: "paper",
  mandate_version: `sha256:${"ab".repeat(32)}`,
  deployed_at: "2026-10-18T00:00:00.000000000Z",
  mode: "normal",
  pending_approvals: 0,
  label: "btc-agent",
  mandate: null,
  account_positions: [],
  openings_held: null,
  ...over,
});

const holding = (over: Partial<AgentHolding>): AgentHolding => ({
  instrument_id: ASSET,
  filled_qty: "0.12",
  cost_basis: { qty: "0.12", basis: "6666.666" },
  risk_mark: { bid: "56789.01", sane: true, session: "crypto", marked_at: "2026-10-30T14:00:00.000000000Z" },
  market_value: "6814.6812",
  unrealized_pnl: "148.0152",
  protection: { orders: [], covered_qty: "0", stop: null, take_profit: null, unprotected_since: null },
  ...over,
});

const catalogue: CatalogueInstrument[] = [{ instrument_id: ASSET, symbol: "BTC/USD", asset_class: "crypto", snapshot: `sha256:${"cd".repeat(32)}`, registered_at: "2026-10-01T00:00:00.000000000Z" }];

describe("the API portfolio (DEC-2413 item 3)", () => {
  it("reports only what the served reads give: before the Portfolio reads answer, no account value, cash, change or P&L", () => {
    const p = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({})] } }, catalogue);
    expect([p.total, p.accountCash, p.observedAt, p.cash, p.unmanaged, p.today, p.pnl]).toEqual([null, null, null, null, null, null, null]);
    expect(p.pnlGross).toBe(true);
    expect(p.invested).toBe("6814.68");
    expect(p.unrealized).toBe("148.02");
  });

  it("takes the average cost from the sub-ledger's basis over its quantity, and names the instrument from the catalogue", () => {
    const [h] = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({})] } }, catalogue).holdings;
    expect([h.symbol, h.assetClass, h.qty, h.avgCost, h.last, h.lastAt]).toEqual(["BTC/USD", "crypto", "0.12", "55555.5500", "56789.01", "2026-10-30T14:00:00.000000000Z"]);
  });

  it("shows a figure the journal does not give as not reported, never zero, and a mark that is not sane as no price", () => {
    const p = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({ cost_basis: null, risk_mark: { bid: null, sane: false, session: "crypto", marked_at: "2026-10-30T14:00:00.000000000Z" }, market_value: null, unrealized_pnl: null })] } }, null);
    const [h] = p.holdings;
    expect([h.symbol, h.assetClass, h.avgCost, h.last, h.lastAt, h.marketValue, h.unrealized]).toEqual([null, null, null, null, null, null, null]);
    expect(p.invested).toBeNull();
    expect(byInstrument(p).total).toBeNull();
  });

  it("never reads an agent whose list could not be read, or has not answered, as holding nothing", () => {
    const agents = [detail({}), detail({ agent_id: "agt_2", label: null })];
    const failed = apiPortfolio("paper", agents, { agt_1: { ok: true, items: [holding({})] }, agt_2: { ok: false, reason: "Not reported: unavailable." } }, catalogue);
    expect(failed.agents.map((a) => a.unread)).toEqual([null, "Not reported: unavailable."]);
    expect(failed.invested).toBeNull();
    expect(byAgent(failed, href).slices.map((s) => [s.label, s.value])).toEqual([
      ["btc-agent", "6814.68"],
      ["Unnamed agent", null],
    ]);
    const waiting = apiPortfolio("paper", agents, { agt_1: { ok: true, items: [] } }, catalogue);
    expect(waiting.agents[1].unread).toBe("Reading…");
    expect(waiting.invested).toBeNull();
  });

  it("keeps the environment the runtime names, unknown included, never paper by default", () => {
    expect(apiPortfolio("live", [], {}, null).environment).toBe("live");
    expect(apiPortfolio("unknown", [], {}, null).environment).toBe("unknown");
  });
});

const AT = "2026-10-08T14:29:50.000000000Z";
const snap = (over: Partial<AccountSnapshot>): AccountSnapshot => ({ equity: "10069.97", cash: "9139.97", buying_power: "9139.97", last_equity: "10000", day_change: "69.97", environment: "paper", observed_at: AT, ...over });
const dashboard = (...snapshots: Array<AccountSnapshot | null>): { ok: true; value: Dashboard } => ({ ok: true, value: { agents: [], accounts: snapshots.map((snapshot, i) => ({ account_ref: `acct_${i + 1}`, snapshot })) } });
/** The figures on a page that shows paper, as every fixture agent is. */
const figures = (read: Parameters<typeof accountFigures>[0]) => accountFigures(read, "paper");

describe("the accounts' figures from the dashboard (DEC-2413)", () => {
  it("never sums a paper account with a live one, nor shows an account under another environment's label", () => {
    const none = { total: null, cash: null, today: null, observedAt: null, agree: false };
    expect(accountFigures(dashboard(snap({}), snap({ environment: "live" })), "paper")).toEqual(none);
    expect(accountFigures(dashboard(snap({}), snap({ environment: "live" })), "live")).toEqual(none);
    expect(accountFigures(dashboard(snap({ environment: "live" })), "paper")).toEqual(none);
    expect(accountFigures(dashboard(snap({})), "unknown")).toEqual(none);
    expect(accountFigures(dashboard(snap({ environment: "live" }), snap({ environment: "live", equity: "0.03" })), "live")).toMatchObject({ total: "10070.00", agree: true });
    const p = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({})] } }, catalogue, { dashboard: dashboard(snap({}), snap({ environment: "live" })), pnl: {} });
    expect([p.total, p.accountCash, p.today, p.accountsAgree]).toEqual([null, null, null, false]);
  });

  it("takes one account's equity, cash and change on the day as served, to the cent", () => {
    expect(figures(dashboard(snap({})))).toEqual({ total: "10069.97", cash: "9139.97", today: "69.97", observedAt: AT, agree: true });
  });

  it("sums several accounts in fixed point, where a float would drift, and dates them by the oldest snapshot", () => {
    // 0.1 + 0.2 is 0.30000000000000004 in binary floating point, and a double cannot hold 1e15 to the cent.
    const read = dashboard(snap({ equity: "0.1", cash: "1000000000000000.01", day_change: "-0.07" }), snap({ equity: "0.2", cash: "0.01", day_change: "0.05", observed_at: "2026-10-08T14:29:49.999999999Z" }));
    expect(figures(read)).toEqual({ total: "0.30", cash: "1000000000000000.02", today: "-0.02", observedAt: "2026-10-08T14:29:49.999999999Z", agree: true });
  });

  it("rounds a figure with more places than cents half away from zero", () => {
    expect(figures(dashboard(snap({ equity: "100.005", cash: "-0.005", day_change: "0.004" })))).toMatchObject({ total: "100.01", cash: "-0.01", today: "0.00" });
  });

  it("makes every total null when any account has no snapshot yet, never counting it as zero", () => {
    expect(figures(dashboard(snap({}), null))).toEqual({ total: null, cash: null, today: null, observedAt: null, agree: true });
  });

  it("makes the change today null when a version 1 snapshot gives no prior close, and keeps the rest", () => {
    expect(figures(dashboard(snap({}), snap({ last_equity: null, day_change: null })))).toEqual({ total: "20139.94", cash: "18279.94", today: null, observedAt: AT, agree: true });
  });

  it("reports nothing with no account, a read that failed, or one not answered", () => {
    const none = { total: null, cash: null, today: null, observedAt: null, agree: true };
    expect(figures(dashboard())).toEqual(none);
    expect(figures({ ok: false, reason: "journal_unavailable" })).toEqual(none);
    expect(figures(undefined)).toEqual(none);
  });

  it("reports a figure it cannot hold to twelve places as not reported, never rounded in silence", () => {
    expect(figures(dashboard(snap({ equity: "1.0000000000001" })))).toMatchObject({ total: null, cash: "9139.97" });
  });
});

const pnl = (agentId: string, realized: string | null, unrealized: string | null): { ok: true; value: AgentPnl } => ({ ok: true, value: { agent_id: agentId, environment: "paper", account_ref: "acct_1", instruments: [], realized_gross: realized, unrealized_pnl: unrealized } });

describe("the agents' gross P&L (DEC-2413)", () => {
  const agents = [{ agent_id: "agt_1" }, { agent_id: "agt_2" }];

  it("adds each agent's realized gross to its unrealized, and sums the agents", () => {
    // (25 + 30) + (-24.56 + 148.01) = 178.45.
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "25", "30"), agt_2: pnl("agt_2", "-24.56", "148.01") })).toBe("178.45");
    // (-0.1 + -0.2) + 0.3 is zero exactly, where floats leave -5.551115123125783e-17.
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "-0.1", "-0.2"), agt_2: pnl("agt_2", "0.3", "0") })).toBe("0.00");
  });

  it("is not reported when any agent's figure is null, its read failed, or it has not answered", () => {
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "25", "30"), agt_2: pnl("agt_2", null, "1") })).toBeNull();
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "25", "30"), agt_2: pnl("agt_2", "1", null) })).toBeNull();
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "25", "30"), agt_2: { ok: false, reason: "journal_unavailable" } })).toBeNull();
    expect(grossPnl(agents, { agt_1: pnl("agt_1", "25", "30") })).toBeNull();
  });

  it("feeds the page's total P&L, labelled gross, and its account figures from the dashboard", () => {
    const p = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({})] } }, catalogue, { dashboard: dashboard(snap({})), pnl: { agt_1: pnl("agt_1", "-24.56", "148.0152") } });
    expect([p.total, p.accountCash, p.today, p.pnl, p.pnlGross, p.observedAt]).toEqual(["10069.97", "9139.97", "69.97", "123.46", true, AT]);
    // The agents' own cash and what no agent manages need each agent's equity, which no read serves.
    expect([p.cash, p.unmanaged]).toEqual([null, null]);
    expect(cashAndInvested(p).total).toBeNull();
  });
});

/** The line's points, or `null` when it is not reported. */
const pointsOf = (h: EquityHistory) => {
  const line = accountLine(h);
  return line.ok ? line.points : null;
};

const history = (range: EquityHistory["range"], ...accounts: Array<Array<[string, string]>>): EquityHistory => ({
  range,
  accounts: accounts.map((points, i) => ({ account_ref: `acct_${i + 1}`, points: points.map(([at, equity]) => ({ at, equity })) })),
});

describe("the account's line from the history (DEC-2413)", () => {
  it("takes one account's points as served, timed in Unix seconds for drawing", () => {
    const line = pointsOf(history("1w", [["2026-10-07T20:59:50.000000000Z", "10000"], ["2026-10-08T14:29:50.000000000Z", "10069.97"]]));
    expect(line).toEqual([
      { at: "2026-10-07T20:59:50.000000000Z", time: Date.UTC(2026, 9, 7, 20, 59, 50) / 1000, equity: "10000" },
      { at: "2026-10-08T14:29:50.000000000Z", time: Date.UTC(2026, 9, 8, 14, 29, 50) / 1000, equity: "10069.97" },
    ]);
    expect(line && lineChange(line)).toBe("69.97");
  });

  it("sums accounts bucket by bucket in fixed point, timed at the latest, and leaves out a bucket one account has no point in", () => {
    // 1d buckets are 300 s from the epoch: 14:00:00 to 14:04:59, 14:05:00 to 14:09:59, 14:10:00 to 14:14:59.
    const line = pointsOf(
      history(
        "1d",
        [
          ["2026-10-08T14:00:10.000000000Z", "0.1"],
          ["2026-10-08T14:05:00.000000000Z", "100"],
          ["2026-10-08T14:14:59.000000000Z", "100.000000000001"],
        ],
        [
          ["2026-10-08T14:04:59.000000000Z", "0.2"],
          ["2026-10-08T14:14:00.000000000Z", "-0.000000000001"],
        ],
      ),
    );
    expect(line).toEqual([
      { at: "2026-10-08T14:04:59.000000000Z", time: Date.UTC(2026, 9, 8, 14, 4, 59) / 1000, equity: "0.3" },
      { at: "2026-10-08T14:14:59.000000000Z", time: Date.UTC(2026, 9, 8, 14, 14, 59) / 1000, equity: "100" },
    ]);
    expect(line && lineChange(line)).toBe("99.70");
  });

  it("buckets a week by the hour and longer ranges by the UTC day", () => {
    expect(pointsOf(history("1w", [["2026-10-08T14:00:00.000000000Z", "1"]], [["2026-10-08T14:59:59.000000000Z", "2"]]))?.map((p) => p.equity)).toEqual(["3"]);
    expect(pointsOf(history("1w", [["2026-10-08T14:00:00.000000000Z", "1"]], [["2026-10-08T15:00:00.000000000Z", "2"]]))).toEqual([]);
    for (const range of ["1m", "3m", "1y", "all"] as const) {
      expect(pointsOf(history(range, [["2026-10-08T00:00:00.000000000Z", "1"]], [["2026-10-08T23:59:59.000000000Z", "2"]]))?.map((p) => p.equity), range).toEqual(["3"]);
      expect(pointsOf(history(range, [["2026-10-07T23:59:59.000000000Z", "1"]], [["2026-10-08T00:00:00.000000000Z", "2"]])), range).toEqual([]);
    }
  });

  it("measures the change to a point held on the line, and none with fewer than two points", () => {
    const line = pointsOf(
      history("1w", [
        ["2026-10-07T20:00:00.000000000Z", "10000"],
        ["2026-10-07T21:00:00.000000000Z", "9987.5"],
        ["2026-10-08T14:00:00.000000000Z", "10069.97"],
      ]),
    );
    if (!line) throw new Error("no line");
    expect(lineChange(line, line[1])).toBe("-12.50");
    expect(lineChange(line.slice(0, 1))).toBeNull();
    expect(lineChange([])).toBeNull();
  });

  it("reports no line with no account or an equity it cannot hold, and an empty one with no snapshot in the range", () => {
    expect(accountLine(history("1d"))).toEqual({ ok: false, reason: "no account" });
    expect(accountLine(history("1d", [["2026-10-08T14:00:00.000000000Z", "1.0000000000001"]]))).toEqual({ ok: false, reason: "an equity has more places than the page can sum" });
    expect(accountLine(history("1d", []))).toEqual({ ok: true, points: [] });
    expect(accountLine(history("1d", [], []))).toEqual({ ok: true, points: [] });
  });

  it("is not reported, rather than none recorded, when one account has no point in the range while another has", () => {
    expect(accountLine(history("1d", [["2026-10-08T14:00:00.000000000Z", "1"]], []))).toEqual({ ok: false, reason: "an account has no snapshot in this range" });
    expect(accountLine(history("1d", [], [["2026-10-08T14:00:00.000000000Z", "1"]]))).toEqual({ ok: false, reason: "an account has no snapshot in this range" });
  });
});
