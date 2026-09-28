/**
 * Seeded, deterministic price bars and equity curves for the fixture workspace. Nothing here is
 * market data: every bar is generated, and the generator is pinned to the workspace so the
 * pictures agree with the figures.
 *
 * - A symbol's closes are a Brownian bridge (in log price) between anchors: each fill of the
 *   agent that trades it, the mark of each position, the price that makes the agent's equity at
 *   the start of the day equal `equity_day_start`, and a peak where its equity touches the
 *   high-water mark.
 * - Between anchors the path is clamped so no resting order would have triggered (a buy limit or a
 *   protective stop stays unfilled, a take-profit stays untouched) and the agent's equity never
 *   rises above its high-water mark.
 * - US equities trade 04:00 to 20:00 ET on weekdays (pre-market, regular session, after hours);
 *   crypto trades every minute. The fixture window sits inside daylight time, so ET is UTC−4.
 *
 * `market.test.ts` checks each of those promises.
 */
import type { Agent, AssetClass, Fill, WorkingOrder, Workspace } from "./types";

export type Session = "pre" | "regular" | "post" | "always";

export interface Bar {
  /** Unix seconds at the start of the minute. */
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  session: Session;
}

export interface DailyBar {
  /** ET calendar date, YYYY-MM-DD. */
  day: string;
  open: number;
  high: number;
  low: number;
  close: number;
}

export interface Point {
  time: number;
  value: number;
}

export interface SymbolBars {
  symbol: string;
  assetClass: AssetClass;
  minute: Bar[];
  daily: DailyBar[];
}

export interface Market {
  /** Unix seconds of the last minute with a bar. */
  end: number;
  start: number;
  symbols: Record<string, SymbolBars>;
  /** Agent equity each minute from deployment to `end`. */
  equity: Record<string, Point[]>;
  /** The sum of agent equity each minute, with each allocation counted as cash before its agent deployed. */
  account: Point[];
}

const ET_OFFSET_S = 4 * 3600;
const MINUTE = 60;
const DAY = 86_400;
export const WINDOW_START_ISO = "2026-09-21T00:00:00-04:00";

/** A price a symbol last traded at when no agent holds it. */
const LAST_PRICE: Record<string, number> = { LMN: 45.6, XYZ: 141.2, QRS: 97.7, "BTC/USD": 56_780 };

/** Where an agent that holds one symbol touches its high-water mark while holding it. */
const PEAK_AT: Record<string, string> = {
  "BTC/USD": "2026-09-26T20:00:00-04:00",
  LMN: "2026-09-25T13:00:00-04:00",
};

const unix = (iso: string) => Math.floor(Date.parse(iso) / 1000);
const floorMinute = (s: number) => s - (s % MINUTE);

export function etParts(time: number): { date: string; minuteOfDay: number; weekday: number } {
  const local = new Date((time - ET_OFFSET_S) * 1000);
  return {
    date: local.toISOString().slice(0, 10),
    minuteOfDay: local.getUTCHours() * 60 + local.getUTCMinutes(),
    weekday: local.getUTCDay(),
  };
}

export function sessionOf(time: number, assetClass: AssetClass): Session | null {
  if (assetClass === "crypto") return "always";
  const { minuteOfDay, weekday } = etParts(time);
  if (weekday === 0 || weekday === 6) return null;
  if (minuteOfDay < 4 * 60 || minuteOfDay >= 20 * 60) return null;
  if (minuteOfDay < 9 * 60 + 30) return "pre";
  if (minuteOfDay < 16 * 60) return "regular";
  return "post";
}

function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

function seedOf(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h >>> 0;
}

function gaussian(rand: () => number): () => number {
  return () => {
    const u = Math.max(rand(), 1e-12);
    const v = rand();
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v);
  };
}

const round2 = (n: number) => Math.round(n * 100) / 100;

interface Band {
  from: number;
  to: number;
  lo?: number;
  hi?: number;
}

interface Owner {
  agent: Agent;
  fills: Fill[];
}

function gridFor(assetClass: AssetClass, start: number, end: number): number[] {
  const grid: number[] = [];
  for (let s = start; s <= end; s += MINUTE) if (sessionOf(s, assetClass)) grid.push(s);
  return grid;
}

/** The last grid index at or before `time`, or 0. */
function indexAtOrBefore(grid: number[], time: number): number {
  let lo = 0;
  let hi = grid.length - 1;
  if (time < grid[0]) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (grid[mid] <= time) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

function stepSigma(prev: number, time: number, assetClass: AssetClass): number {
  if (assetClass === "crypto") return 0.0006;
  if (time - prev > MINUTE) return 0.004;
  return sessionOf(time, assetClass) === "regular" ? 0.0007 : 0.00035;
}

interface Holdings {
  cash: number;
  qty: Record<string, number>;
}

function holdingsAt(agent: Agent, time: number): Holdings {
  const h: Holdings = { cash: Number(agent.state.capital_base), qty: {} };
  for (const f of agent.fills) {
    if (floorMinute(unix(f.at)) > time) break;
    const q = Number(f.qty);
    const cost = q * Number(f.price);
    const sym = f.instrument.symbol;
    h.qty[sym] = (h.qty[sym] ?? 0) + (f.side === "buy" ? q : -q);
    h.cash += f.side === "buy" ? -cost : cost;
  }
  return h;
}

function heldSymbols(h: Holdings): string[] {
  return Object.entries(h.qty)
    .filter(([, q]) => Math.abs(q) > 1e-12)
    .map(([s]) => s);
}

/** The single price that makes the agent's equity equal `target`, when it holds exactly one symbol. */
function solve(agent: Agent, time: number, target: number): { symbol: string; price: number } | null {
  const h = holdingsAt(agent, time);
  const held = heldSymbols(h);
  if (held.length !== 1) return null;
  return { symbol: held[0], price: (target - h.cash) / h.qty[held[0]] };
}

function bandsFor(order: WorkingOrder, until: number, agent: Agent): Band[] {
  const from = unix(order.submitted_at);
  const bands: Band[] = [];
  if (order.state === "Unknown" || order.state === "Filled") return bands;
  if (order.side === "buy" && order.limit_price) bands.push({ from, to: until, lo: Number(order.limit_price) });
  if (order.side === "sell" && order.stop_price) bands.push({ from, to: until, lo: Number(order.stop_price) });
  if (order.side === "sell" && order.limit_price && !order.stop_price && order.purpose !== "protective") bands.push({ from, to: until, hi: Number(order.limit_price) });
  if (order.purpose === "protective") {
    const position = agent.positions.find((p) => p.instrument.symbol === order.instrument.symbol);
    const tp = position?.protection.take_profit_price;
    if (tp && until >= unix(agent.deployed_at)) bands.push({ from, to: until, hi: Number(tp) });
  }
  return bands;
}

function bridge(grid: number[], anchors: Array<{ index: number; price: number }>, assetClass: AssetClass, rand: () => number): number[] {
  const normal = gaussian(rand);
  const x = new Array<number>(grid.length).fill(Number.NaN);
  const sigma = grid.map((t, k) => (k === 0 ? 0 : stepSigma(grid[k - 1], t, assetClass)));
  const first = anchors[0];
  x[first.index] = Math.log(first.price);
  for (let k = first.index - 1; k >= 0; k--) x[k] = x[k + 1] - sigma[k + 1] * normal();
  for (let a = 0; a < anchors.length - 1; a++) {
    const i = anchors[a].index;
    const j = anchors[a + 1].index;
    const xi = Math.log(anchors[a].price);
    const xj = Math.log(anchors[a + 1].price);
    x[i] = xi;
    if (j === i) continue;
    let w = 0;
    let v = 0;
    const W: number[] = [0];
    const V: number[] = [0];
    for (let k = i + 1; k <= j; k++) {
      w += sigma[k] * normal();
      v += sigma[k] * sigma[k];
      W.push(w);
      V.push(v);
    }
    for (let k = i + 1; k <= j; k++) {
      const f = V[k - i] / v;
      x[k] = xi + W[k - i] - f * (W[j - i] - (xj - xi));
    }
  }
  const last = anchors[anchors.length - 1];
  for (let k = last.index + 1; k < grid.length; k++) x[k] = x[k - 1] + sigma[k] * normal();
  return x.map((v) => Math.exp(v));
}

function applyBands(price: number, time: number, bands: Band[]): number {
  let p = price;
  for (const b of bands) {
    if (time < floorMinute(b.from) || time > b.to) continue;
    if (b.lo !== undefined && p < b.lo * 1.002) p = b.lo * 1.002;
    if (b.hi !== undefined && p > b.hi * 0.998) p = b.hi * 0.998;
  }
  return p;
}

function toBars(grid: number[], closes: number[], exact: Set<number>, bands: Band[], assetClass: AssetClass, rand: () => number): Bar[] {
  const normal = gaussian(rand);
  const bars: Bar[] = [];
  for (let k = 0; k < grid.length; k++) {
    const time = grid[k];
    const close = exact.has(k) ? closes[k] : round2(closes[k]);
    const prev = k === 0 ? close : bars[k - 1].close;
    const sigma = k === 0 ? 0.0005 : stepSigma(grid[k - 1], time, assetClass);
    const open = applyBands(round2(prev * (1 + (k > 0 && time - grid[k - 1] > MINUTE ? 0.3 : 0.15) * sigma * normal())), time, bands);
    const wick = () => Math.abs(normal()) * sigma * 0.6;
    const high = applyBands(round2(Math.max(open, close) * (1 + wick())), time, bands);
    const low = applyBands(round2(Math.min(open, close) * (1 - wick())), time, bands);
    bars.push({
      time,
      open,
      close,
      high: Math.max(high, open, close),
      low: Math.min(low, open, close),
      session: sessionOf(time, assetClass) ?? "always",
    });
  }
  return bars;
}

function dailyFrom(minute: Bar[], assetClass: AssetClass, symbol: string): DailyBar[] {
  const byDay = new Map<string, DailyBar>();
  for (const b of minute) {
    if (assetClass === "us_equity" && b.session !== "regular") continue;
    const day = etParts(b.time).date;
    const d = byDay.get(day);
    if (!d) byDay.set(day, { day, open: b.open, high: b.high, low: b.low, close: b.close });
    else {
      d.high = Math.max(d.high, b.high);
      d.low = Math.min(d.low, b.low);
      d.close = b.close;
    }
  }
  const recent = [...byDay.values()];
  const rand = mulberry32(seedOf(`${symbol}:daily`));
  const normal = gaussian(rand);
  const sigma = assetClass === "crypto" ? 0.025 : 0.016;
  const earlier: DailyBar[] = [];
  let next = recent[0]?.open ?? LAST_PRICE[symbol] ?? 100;
  let day = Date.parse(`${recent[0]?.day ?? WINDOW_START_ISO.slice(0, 10)}T12:00:00Z`);
  const count = assetClass === "crypto" ? 365 : 252;
  while (earlier.length < count) {
    day -= DAY * 1000;
    const weekday = new Date(day).getUTCDay();
    if (assetClass === "us_equity" && (weekday === 0 || weekday === 6)) continue;
    const close = next;
    const open = close / Math.exp(sigma * normal());
    const high = Math.max(open, close) * (1 + Math.abs(normal()) * sigma * 0.4);
    const low = Math.min(open, close) * (1 - Math.abs(normal()) * sigma * 0.4);
    earlier.push({ day: new Date(day).toISOString().slice(0, 10), open: round2(open), high: round2(high), low: round2(low), close: round2(close) });
    next = open;
  }
  return [...earlier.reverse(), ...recent];
}

interface Built extends SymbolBars {
  times: number[];
  /** Indexes of anchored minutes; their closes are exact and never clamped. */
  exact: Set<number>;
}

function symbolBars(symbol: string, assetClass: AssetClass, owner: Owner | undefined, ws: Workspace, start: number, end: number): Built {
  const grid = gridFor(assetClass, start, end);
  const rand = mulberry32(seedOf(symbol));
  const anchors = new Map<number, number>();
  const bands: Band[] = [];
  const dayStart = unix(`${ws.now.slice(0, 10)}T00:00:00-04:00`);

  const put = (time: number, price: number) => {
    const minute = floorMinute(time);
    if (minute < start || minute > end || !Number.isFinite(price)) return;
    anchors.set(indexAtOrBefore(grid, minute), price);
  };

  if (owner) {
    const { agent } = owner;
    const peak = PEAK_AT[symbol];
    if (peak) {
      const solved = solve(agent, unix(peak), Number(agent.state.high_water_mark));
      if (solved?.symbol === symbol) put(unix(peak), solved.price);
    }
    const opensAt = grid[indexAtOrBefore(grid, dayStart)];
    const opening = solve(agent, opensAt, Number(agent.state.equity_day_start));
    if (opening?.symbol === symbol) put(opensAt, opening.price);
    for (const f of owner.fills) if (f.instrument.symbol === symbol) put(unix(f.at), Number(f.price));
    for (const p of agent.positions) if (p.instrument.symbol === symbol) put(unix(p.mark_as_of), Number(p.mark));
    for (const o of agent.orders) if (o.instrument.symbol === symbol) bands.push(...bandsFor(o, end, agent));
    for (const o of agent.past_orders) if (o.instrument.symbol === symbol) bands.push(...bandsFor(o, unix(o.closed_at), agent));
  }
  if (!owner?.agent.positions.some((p) => p.instrument.symbol === symbol)) put(end, LAST_PRICE[symbol] ?? 100);

  const sorted = [...anchors.entries()].sort((a, b) => a[0] - b[0]).map(([index, price]) => ({ index, price }));
  const closes = bridge(grid, sorted, assetClass, rand).map((p, k) => (anchors.has(k) ? p : applyBands(p, grid[k], bands)));
  const exact = new Set(anchors.keys());
  return { symbol, assetClass, minute: toBars(grid, closes, exact, bands, assetClass, rand), daily: [], times: grid, exact };
}

/** Walk minute by minute and read each symbol's last close at or before the minute. */
function equityCurve(agent: Agent, symbols: Record<string, SymbolBars>, from: number, end: number): Point[] {
  const syms = [...new Set(agent.fills.map((f) => f.instrument.symbol))];
  const cursor: Record<string, number> = Object.fromEntries(syms.map((s) => [s, 0]));
  const fills = agent.fills.map((f) => ({ time: floorMinute(unix(f.at)), f }));
  let fi = 0;
  let cash = Number(agent.state.capital_base);
  const qty: Record<string, number> = {};
  const points: Point[] = [];
  const holding: boolean[] = [];
  for (let t = from; t <= end; t += MINUTE) {
    while (fi < fills.length && fills[fi].time <= t) {
      const { f } = fills[fi++];
      const q = Number(f.qty);
      qty[f.instrument.symbol] = (qty[f.instrument.symbol] ?? 0) + (f.side === "buy" ? q : -q);
      cash += (f.side === "buy" ? -1 : 1) * q * Number(f.price);
    }
    let value = cash;
    for (const s of syms) {
      const bars = symbols[s]?.minute ?? [];
      while (cursor[s] + 1 < bars.length && bars[cursor[s] + 1].time <= t) cursor[s]++;
      const bar = bars[cursor[s]];
      const px = bar ? (bar.time <= t ? bar.close : bar.open) : 0;
      value += (qty[s] ?? 0) * px;
    }
    points.push({ time: t, value });
    holding.push(syms.some((s) => Math.abs(qty[s] ?? 0) > 1e-12));
  }
  // The ledger rounds marked holdings to the cent; carrying that rounding ends the curve on the figure.
  const rounding = points.length > 0 ? Number(agent.state.equity) - points[points.length - 1].value : 0;
  return points.map((p, i) => ({ time: p.time, value: round2(holding[i] ? p.value + rounding : p.value) }));
}

/** Hold each agent below its high-water mark by trimming the one symbol it holds, minute by minute. */
function capAtHighWater(agent: Agent, symbols: Record<string, Built>, end: number) {
  const hwm = Number(agent.state.high_water_mark);
  const deployed = floorMinute(unix(agent.deployed_at));
  for (const sym of new Set(agent.fills.map((f) => f.instrument.symbol))) {
    const series = symbols[sym];
    if (!series) continue;
    series.minute.forEach((bar, k) => {
      if (bar.time < deployed || bar.time > end || series.exact.has(k)) return;
      const h = holdingsAt(agent, bar.time);
      const q = h.qty[sym] ?? 0;
      if (q <= 1e-12) return;
      let others = 0;
      for (const [s, qq] of Object.entries(h.qty)) {
        if (s === sym || Math.abs(qq) < 1e-12) continue;
        const other = symbols[s];
        if (!other) continue;
        others += qq * other.minute[indexAtOrBefore(other.times, bar.time)].close;
      }
      const cap = (hwm * 0.9995 - h.cash - others) / q;
      if (bar.high > cap) {
        bar.high = round2(Math.min(bar.high, cap));
        bar.close = Math.min(bar.close, bar.high);
        bar.open = Math.min(bar.open, bar.high);
        bar.low = Math.min(bar.low, bar.open, bar.close);
      }
    });
  }
}

const cache = new WeakMap<Workspace, Market>();

export function buildMarket(ws: Workspace): Market {
  const hit = cache.get(ws);
  if (hit) return hit;
  const start = unix(WINDOW_START_ISO);
  const stale = ws.health.market_data.state !== "ok";
  const end = floorMinute(stale ? unix(ws.health.market_data.as_of) : unix(ws.now));

  const owners = new Map<string, Owner>();
  const classes = new Map<string, AssetClass>();
  for (const agent of ws.agents) {
    for (const f of agent.fills) {
      owners.set(f.instrument.symbol, { agent, fills: agent.fills });
      classes.set(f.instrument.symbol, f.instrument.asset_class);
    }
    for (const p of agent.positions) classes.set(p.instrument.symbol, p.instrument.asset_class);
    for (const i of agent.mandate.universe.pinned_instruments) classes.set(i.symbol, i.asset_class);
  }
  for (const a of ws.approvals) classes.set(a.bound.symbol, a.bound.asset_class);

  const built: Record<string, Built> = {};
  for (const [symbol, assetClass] of classes) built[symbol] = symbolBars(symbol, assetClass, owners.get(symbol), ws, start, end);
  for (const agent of ws.agents) capAtHighWater(agent, built, end);
  const symbols: Record<string, SymbolBars> = {};
  for (const { symbol, assetClass, minute } of Object.values(built)) symbols[symbol] = { symbol, assetClass, minute, daily: dailyFrom(minute, assetClass, symbol) };

  const equity: Record<string, Point[]> = {};
  for (const agent of ws.agents) equity[agent.agent_id] = equityCurve(agent, symbols, Math.max(start, floorMinute(unix(agent.deployed_at))), end);

  const account: Point[] = [];
  for (let t = start, i = 0; t <= end; t += MINUTE, i++) {
    let value = 0;
    for (const agent of ws.agents) {
      const curve = equity[agent.agent_id];
      const offset = (t - curve[0].time) / MINUTE;
      value += offset < 0 ? Number(agent.state.capital_base) : curve[offset].value;
    }
    account.push({ time: t, value });
  }

  const market: Market = { start, end, symbols, equity, account };
  cache.set(ws, market);
  return market;
}

/** Every `step` seconds from `from`, always keeping the last point. */
export function sample(points: Point[], from: number, step: number): Point[] {
  const out: Point[] = [];
  let next = from;
  for (const p of points) {
    if (p.time < from) continue;
    if (p.time >= next) {
      out.push(p);
      next = p.time + step;
    }
  }
  const last = points[points.length - 1];
  if (last && out[out.length - 1] !== last && last.time >= from) out.push(last);
  return out;
}

/** Minute bars aggregated to `minutes`-minute bars, grouping by clock boundary. */
export function aggregate(bars: Bar[], minutes: number): Bar[] {
  const out: Bar[] = [];
  const size = minutes * MINUTE;
  for (const b of bars) {
    const bucket = b.time - (b.time % size);
    const last = out[out.length - 1];
    if (last && last.time === bucket) {
      last.high = Math.max(last.high, b.high);
      last.low = Math.min(last.low, b.low);
      last.close = b.close;
    } else out.push({ ...b, time: bucket });
  }
  return out;
}

export function unixOf(iso: string): number {
  return unix(iso);
}
