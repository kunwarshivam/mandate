import { describe, expect, it } from "vitest";
import type { AgentHolding } from "@/api/activity";
import type { CatalogueInstrument } from "@/api/catalogue";
import type { AgentDetail } from "@/api/read-models";
import { AGENT_IDS, buildWorkspace } from "@/fixtures/workspace";
import { apiPortfolio, breakdown, byAgent, byAssetClass, byInstrument, cashAndInvested, fixturePortfolio, sumReported } from "./portfolio";

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
  it("reports only what the served reads give: no account value, cash, history or P&L", () => {
    const p = apiPortfolio("paper", [detail({})], { agt_1: { ok: true, items: [holding({})] } }, catalogue);
    expect([p.total, p.cash, p.unmanaged, p.today, p.pnl]).toEqual([null, null, null, null, null]);
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
