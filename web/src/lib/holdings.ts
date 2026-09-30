import type { Agent, AssetClass, Workspace } from "@/fixtures/types";
import { type Dec, ZERO, add, dec, div, sub, toFixed } from "./decimal";

/**
 * What the account is made of (DEC-217): each instrument the agents hold, their cash, and what no
 * agent manages. An instrument keeps one series colour on every chart of the dashboard, chosen by
 * its share of the account, so it reads the same across the account and in each agent.
 */
export type SliceTone = "series-1" | "series-2" | "series-3" | "series-4" | "series-5" | "muted";

export interface Slice {
  key: string;
  label: string;
  value: string;
  /** Of the whole, as a fraction of one. */
  share: string;
  tone: SliceTone;
  kind: "asset" | "cash" | "unmanaged";
  /** The agents holding it, for an asset; empty otherwise. */
  agents: string[];
}

const ASSET_TONES: SliceTone[] = ["series-1", "series-2", "series-3", "series-4"];
export const CASH_KEY = "cash";
export const UNMANAGED_KEY = "unmanaged";

const CLASS_WORD: Record<AssetClass, string> = { crypto: "Crypto", us_equity: "US equities" };

export function assetClassWord(c: AssetClass): string {
  return CLASS_WORD[c];
}

function shareOf(part: Dec, whole: Dec): string {
  return whole === ZERO ? "0" : toFixed(div(part, whole), 4);
}

export function agentCash(agent: Agent): Dec {
  return sub(dec(agent.state.equity), add(...agent.positions.map((p) => dec(p.market_value))));
}

/** The instruments the agents hold, largest first, each with its series colour. */
export function assetTones(ws: Workspace): Map<string, SliceTone> {
  const totals = new Map<string, Dec>();
  for (const p of ws.agents.flatMap((a) => a.positions)) totals.set(p.instrument.symbol, add(totals.get(p.instrument.symbol) ?? ZERO, dec(p.market_value)));
  const ranked = [...totals].sort(([sa, a], [sb, b]) => (a === b ? sa.localeCompare(sb) : a > b ? -1 : 1));
  return new Map(ranked.map(([symbol], i) => [symbol, ASSET_TONES[Math.min(i, ASSET_TONES.length - 1)]]));
}

/** The whole account: every held instrument, the agents' cash, and the part no agent manages. */
export function accountSlices(ws: Workspace): { total: string; slices: Slice[] } {
  const total = dec(ws.connection.account_equity);
  const managed = add(...ws.agents.map((a) => dec(a.state.equity)));
  const tones = assetTones(ws);
  const assets = [...tones].map(([symbol, tone]) => {
    const holders = ws.agents.filter((a) => a.positions.some((p) => p.instrument.symbol === symbol));
    const value = add(...holders.flatMap((a) => a.positions.filter((p) => p.instrument.symbol === symbol).map((p) => dec(p.market_value))));
    return { key: symbol, label: symbol, value: toFixed(value, 2), share: shareOf(value, total), tone, kind: "asset" as const, agents: holders.map((a) => a.label) };
  });
  const cash = add(...ws.agents.map(agentCash));
  const unmanaged = sub(total, managed);
  const slices: Slice[] = [
    ...assets,
    { key: CASH_KEY, label: "Agents' cash", value: toFixed(cash, 2), share: shareOf(cash, total), tone: "series-5", kind: "cash", agents: [] },
    { key: UNMANAGED_KEY, label: "Not managed by an agent", value: toFixed(unmanaged, 2), share: shareOf(unmanaged, total), tone: "muted", kind: "unmanaged", agents: [] },
  ];
  return { total: toFixed(total, 2), slices: slices.filter((s) => dec(s.value) > ZERO) };
}

/** One agent's equity: what it holds, in the account's colours, and its cash. */
export function agentSlices(agent: Agent, tones: Map<string, SliceTone>): Slice[] {
  const total = dec(agent.state.equity);
  const cash = agentCash(agent);
  const held: Slice[] = agent.positions.map((p) => ({
    key: p.instrument.symbol,
    label: p.instrument.symbol,
    value: toFixed(dec(p.market_value), 2),
    share: shareOf(dec(p.market_value), total),
    tone: tones.get(p.instrument.symbol) ?? "series-4",
    kind: "asset",
    agents: [agent.label],
  }));
  const slices: Slice[] = [...held, { key: CASH_KEY, label: "Cash", value: toFixed(cash, 2), share: shareOf(cash, total), tone: "series-5", kind: "cash", agents: [] }];
  return slices.filter((s) => dec(s.value) > ZERO);
}

export interface UniverseEntry {
  symbol: string;
  held: boolean;
}

/** What the agent's mandate lets it trade, and which of those it holds now. */
export function mandateUniverse(agent: Agent): { instruments: UniverseEntry[]; classes: string[] } {
  const { universe } = agent.mandate;
  const holding = new Set(agent.positions.map((p) => p.instrument.symbol));
  const instruments = universe.pinned ? universe.pinned_instruments.map((i) => ({ symbol: i.symbol, held: holding.has(i.symbol) })) : [];
  return { instruments, classes: universe.asset_classes.map(assetClassWord) };
}
