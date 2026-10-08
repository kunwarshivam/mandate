// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { Agent, Workspace } from "./types";
import { type Bar, buildMarket, etParts, unixOf } from "./market";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "./workspace";

const minuteOf = (iso: string) => unixOf(iso) - (unixOf(iso) % 60);

function barAt(bars: Bar[], iso: string): Bar {
  const t = minuteOf(iso);
  const bar = bars.find((b) => b.time === t);
  if (!bar) throw new Error(`no bar at ${iso}`);
  return bar;
}

function agent(ws: Workspace, id: string): Agent {
  const a = ws.agents.find((x) => x.agent_id === id);
  if (!a) throw new Error(id);
  return a;
}

/** Average-cost accounting: buys move the average, sells realize against it. */
function replay(a: Agent) {
  const book: Record<string, { qty: number; avg: number }> = {};
  let realized = 0;
  for (const f of a.fills) {
    const q = Number(f.qty);
    const p = Number(f.price);
    const b = (book[f.instrument.symbol] ??= { qty: 0, avg: 0 });
    if (f.side === "buy") {
      b.avg = (b.avg * b.qty + p * q) / (b.qty + q);
      b.qty += q;
    } else {
      realized += (p - b.avg) * q;
      b.qty -= q;
    }
  }
  return { book, realized };
}

const ws = buildWorkspace("normal");
const market = buildMarket(ws);

describe("fills agree with the figures", () => {
  it.each(ws.agents.map((a) => [a.label, a] as const))("%s: fills reproduce positions, realized P&L and equity", (_label, a) => {
    const { book, realized } = replay(a);
    expect(realized).toBeCloseTo(Number(a.realized_pnl), 2);
    for (const p of a.positions) {
      expect(book[p.instrument.symbol].qty).toBeCloseTo(Number(p.qty), 9);
      expect(book[p.instrument.symbol].avg).toBeCloseTo(Number(p.avg_cost), 2);
    }
    for (const [symbol, b] of Object.entries(book)) if (!a.positions.some((p) => p.instrument.symbol === symbol)) expect(b.qty).toBeCloseTo(0, 9);
    const unrealized = a.positions.reduce((s, p) => s + Number(p.market_value) - Number(p.qty) * Number(p.avg_cost), 0);
    expect(Number(a.state.capital_base) + realized + unrealized).toBeCloseTo(Number(a.state.equity), 1);
  });

  it("gives every filled order exactly one fill at the time it closed", () => {
    for (const a of ws.agents) {
      for (const o of a.past_orders.filter((x) => x.state === "Filled")) {
        const fills = a.fills.filter((f) => f.client_order_id === o.client_order_id);
        expect(fills).toHaveLength(1);
        expect(fills[0].at).toBe(o.closed_at);
      }
    }
  });
});

describe("bars", () => {
  it("are the same on every build", () => {
    const again = buildMarket(buildWorkspace("normal"));
    expect(again.symbols.XYZ.minute.slice(-50)).toEqual(market.symbols.XYZ.minute.slice(-50));
    expect(again.symbols["BTC/USD"].daily.slice(0, 5)).toEqual(market.symbols["BTC/USD"].daily.slice(0, 5));
  });

  it("pass through every fill at its minute", () => {
    for (const a of ws.agents) {
      for (const f of a.fills) {
        const bar = barAt(market.symbols[f.instrument.symbol].minute, f.at);
        expect(bar.close).toBeCloseTo(Number(f.price), 6);
        expect(bar.low).toBeLessThanOrEqual(Number(f.price));
        expect(bar.high).toBeGreaterThanOrEqual(Number(f.price));
      }
    }
  });

  it("end at each position's mark, at the mark's minute", () => {
    for (const a of ws.agents) {
      for (const p of a.positions) {
        const bars = market.symbols[p.instrument.symbol].minute;
        expect(bars[bars.length - 1].time).toBe(minuteOf(p.mark_as_of));
        expect(bars[bars.length - 1].close).toBeCloseTo(Number(p.mark), 6);
      }
    }
  });

  it("stop at the market data's as-of time when it is stale", () => {
    const stale = buildWorkspace("stale");
    const m = buildMarket(stale);
    expect(m.end).toBe(minuteOf(stale.health.market_data.as_of));
    for (const s of Object.values(m.symbols)) expect(s.minute[s.minute.length - 1].time).toBeLessThanOrEqual(m.end);
  });

  it("trade US equities 04:00 to 20:00 ET on weekdays only, labelled by session, and crypto every minute", () => {
    for (const b of market.symbols.XYZ.minute) {
      const { minuteOfDay, weekday } = etParts(b.time);
      expect(weekday).not.toBe(0);
      expect(weekday).not.toBe(6);
      expect(minuteOfDay).toBeGreaterThanOrEqual(240);
      expect(minuteOfDay).toBeLessThan(1200);
      expect(b.session).toBe(minuteOfDay < 570 ? "pre" : minuteOfDay < 960 ? "regular" : "post");
    }
    const btc = market.symbols["BTC/USD"].minute;
    expect(btc.some((b) => etParts(b.time).weekday === 6)).toBe(true);
    for (let i = 1; i < btc.length; i++) expect(btc[i].time - btc[i - 1].time).toBe(60);
  });

  it("keep high at or above low, open and close", () => {
    for (const s of Object.values(market.symbols)) {
      for (const b of s.minute) {
        expect(b.high).toBeGreaterThanOrEqual(Math.max(b.open, b.close) - 1e-9);
        expect(b.low).toBeLessThanOrEqual(Math.min(b.open, b.close) + 1e-9);
      }
    }
  });

  it("never touch a resting order: buy limits and stops stay unfilled, take-profits untouched", () => {
    for (const a of ws.agents) {
      const orders = [...a.orders.map((o) => ({ ...o, until: market.end })), ...a.past_orders.filter((o) => o.state !== "Filled").map((o) => ({ ...o, until: unixOf(o.closed_at) }))];
      for (const o of orders) {
        const from = minuteOf(o.submitted_at);
        const bars = market.symbols[o.instrument.symbol].minute.filter((b) => b.time > from && b.time < o.until);
        const floor = o.side === "buy" ? o.limit_price : o.stop_price;
        if (floor) for (const b of bars) expect(b.low, `${o.client_order_id} at ${b.time}`).toBeGreaterThan(Number(floor));
      }
      for (const p of a.positions) {
        const tp = p.protection.take_profit_price;
        const leg = a.orders.find((o) => o.purpose === "protective" && o.instrument.symbol === p.instrument.symbol);
        if (!tp || !leg) continue;
        for (const b of market.symbols[p.instrument.symbol].minute.filter((x) => x.time > minuteOf(leg.submitted_at))) expect(b.high).toBeLessThan(Number(tp));
      }
    }
  });

  it("carry a year of daily bars ending today", () => {
    const xyz = market.symbols.XYZ.daily;
    expect(xyz.length).toBeGreaterThan(250);
    expect(xyz[xyz.length - 1].day).toBe("2026-09-28");
    expect(xyz.some((d) => d.day === "2026-09-26")).toBe(false);
    expect(market.symbols["BTC/USD"].daily.some((d) => d.day === "2026-09-26")).toBe(true);
  });
});

describe("equity curves", () => {
  it.each(ws.agents.map((a) => [a.label, a.agent_id] as const))("%s ends at its equity, starts the day at its day-start equity to the cent, moves today by its P&L today, and never passes its high-water mark", (_label, id) => {
    const a = agent(ws, id);
    const curve = market.equity[id];
    expect(curve[curve.length - 1].value).toBeCloseTo(Number(a.state.equity), 2);
    const dayStart = curve.find((p) => p.time === unixOf("2026-09-28T00:00:00-04:00"));
    expect(dayStart?.value).toBeCloseTo(Number(a.state.equity_day_start), 2);
    expect((curve[curve.length - 1].value - dayStart!.value).toFixed(2)).toBe(Number(a.pnl_today).toFixed(2));
    const peak = Math.max(...curve.map((p) => p.value));
    expect(peak).toBeLessThanOrEqual(Number(a.state.high_water_mark) + 0.01);
    expect(peak).toBeGreaterThan(Number(a.state.high_water_mark) - 0.5);
    expect(curve[0].time).toBe(unixOf(a.deployed_at));
    expect(curve[0].value).toBe(Number(a.state.capital_base));
  });

  it("sums to the account curve", () => {
    const last = market.account[market.account.length - 1].value;
    expect(last).toBeCloseTo(ws.agents.reduce((s, a) => s + Number(a.state.equity), 0), 1);
  });

  it("follows the scenario: in drawdown, Agent 1 ends at its lower equity", () => {
    const dd = buildWorkspace("drawdown");
    const m = buildMarket(dd);
    const curve = m.equity[AGENT_IDS.btc];
    expect(curve[curve.length - 1].value).toBeCloseTo(9530, 1);
  });

  it.each(SCENARIOS.map((s) => s.id))("builds for the %s scenario", (scenario) => {
    expect(() => buildMarket(buildWorkspace(scenario))).not.toThrow();
  });

  it.each(SCENARIOS.map((s) => s.id))("in the %s scenario no curve moves more than 2% in one minute, so a scenario's figures are reached along a path and never by a cliff at the right edge", (scenario) => {
    const ws = buildWorkspace(scenario);
    const m = buildMarket(ws);
    const curves = [...ws.agents.map((a) => [`Agent ${a.agent_id}`, m.equity[a.agent_id]] as const), ["the account", m.account] as const];
    for (const [name, curve] of curves) {
      for (let i = 1; i < curve.length; i++) {
        if (curve[i - 1].value === 0) continue;
        const step = Math.abs(curve[i].value - curve[i - 1].value) / curve[i - 1].value;
        expect(step, `${name} at ${new Date(curve[i].time * 1000).toISOString()}`).toBeLessThanOrEqual(0.02);
      }
    }
  });
});
