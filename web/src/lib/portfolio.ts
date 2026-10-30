import type { AgentHolding } from "@/api/activity";
import type { CatalogueInstrument } from "@/api/catalogue";
import type { AgentDetail } from "@/api/read-models";
import type { AgentMode, AssetClass, ShownEnvironment, Workspace } from "@/fixtures/types";
import { type Dec, ZERO, add, dec, div, sub, toFixed } from "./decimal";

/**
 * The Portfolio page's figures (DEC-2413), from the fixtures or from the workspace API's served reads,
 * in one shape. Money stays a decimal string throughout: every sum and share here is fixed-point
 * (`decimal.ts`), and a figure no read gives is `null`, shown as "not reported", never a zero.
 */

/** One instrument one agent holds. */
export interface Holding {
  agentId: string;
  /** The owner's name for the agent, `null` when the read names none. */
  agentName: string | null;
  instrumentId: string;
  /** `null` when no read names the instrument's symbol: the screen shows its id. */
  symbol: string | null;
  assetClass: AssetClass | null;
  qty: string;
  avgCost: string | null;
  last: string | null;
  lastAt: string | null;
  marketValue: string | null;
  unrealized: string | null;
}

export interface PortfolioAgent {
  agentId: string;
  name: string | null;
  mode: AgentMode | null;
  /** Why its holdings could not be read, or `null` when they were: an unread list is never "holds nothing". */
  unread: string | null;
}

export interface Portfolio {
  environment: ShownEnvironment;
  /** The account's value at the broker. */
  total: string | null;
  /** What the agents hold, at market value. */
  invested: string | null;
  /** The agents' cash: each agent's equity less what it holds. */
  cash: string | null;
  /** The part of the account no agent manages: the account's value less the agents' equity. */
  unmanaged: string | null;
  today: string | null;
  pnl: string | null;
  unrealized: string | null;
  agents: PortfolioAgent[];
  holdings: Holding[];
}

const money = (value: Dec): string => toFixed(value, 2);

/** The sum of every value, or `null` when any one is not reported. An empty list sums to zero. */
export function sumReported(values: ReadonlyArray<string | null>): string | null {
  if (values.some((v) => v === null)) return null;
  return money(add(...(values as string[]).map(dec)));
}

/** The workspace's figures over the fixtures, where every one is given. */
export function fixturePortfolio(ws: Workspace): Portfolio {
  const holdings: Holding[] = ws.agents.flatMap((a) =>
    a.positions.map((p) => ({
      agentId: a.agent_id,
      agentName: a.label,
      instrumentId: p.instrument.asset_id,
      symbol: p.instrument.symbol,
      assetClass: p.instrument.asset_class,
      qty: p.qty,
      avgCost: p.avg_cost,
      last: p.mark,
      lastAt: p.mark_as_of,
      marketValue: p.market_value,
      unrealized: p.unrealized_pnl,
    })),
  );
  const equity = add(...ws.agents.map((a) => dec(a.state.equity)));
  const invested = add(...holdings.map((h) => dec(h.marketValue as string)));
  const total = dec(ws.connection.account_equity);
  return {
    environment: ws.environment,
    total: money(total),
    invested: money(invested),
    cash: money(sub(equity, invested)),
    unmanaged: money(sub(total, equity)),
    today: money(add(...ws.agents.map((a) => dec(a.pnl_today)))),
    pnl: money(add(...ws.agents.map((a) => dec(a.pnl_total)))),
    unrealized: sumReported(holdings.map((h) => h.unrealized)),
    agents: ws.agents.map((a) => ({ agentId: a.agent_id, name: a.label, mode: a.mode, unread: null })),
    holdings,
  };
}

/** One agent's `GET /agents/{id}/positions`, as the runtime answered it; absent until it answers. */
export type HoldingsRead = { ok: true; items: AgentHolding[] } | { ok: false; reason: string };

/**
 * The workspace's figures over the workspace API (DEC-2413 item 3): only what the served reads give.
 * The account's value, cash, today's change and total P&L have no served read, so they are `null`;
 * what is invested and unrealized is summed from the agents' positions only when every agent's list
 * was read and every figure in it is reported. The average cost is the sub-ledger's basis over its
 * quantity, and the last price the latest risk mark, only when it is sane.
 */
export function apiPortfolio(
  environment: ShownEnvironment,
  agents: readonly AgentDetail[],
  reads: Readonly<Record<string, HoldingsRead | undefined>>,
  instruments: readonly CatalogueInstrument[] | null,
): Portfolio {
  const byId = new Map((instruments ?? []).map((i) => [i.instrument_id, i]));
  const holdings: Holding[] = [];
  const listed: PortfolioAgent[] = agents.map((a) => {
    const read = reads[a.agent_id];
    if (read?.ok) {
      for (const h of read.items) {
        const known = byId.get(h.instrument_id);
        const basis = h.cost_basis;
        const qty = basis ? dec(basis.qty) : ZERO;
        holdings.push({
          agentId: a.agent_id,
          agentName: a.label,
          instrumentId: h.instrument_id,
          symbol: known?.symbol ?? null,
          assetClass: known?.asset_class ?? null,
          qty: h.filled_qty,
          avgCost: basis && qty !== ZERO ? toFixed(div(dec(basis.basis), qty), 4) : null,
          last: h.risk_mark?.sane && h.risk_mark.bid !== null ? h.risk_mark.bid : null,
          lastAt: h.risk_mark?.sane ? h.risk_mark.marked_at : null,
          marketValue: h.market_value,
          unrealized: h.unrealized_pnl,
        });
      }
    }
    return { agentId: a.agent_id, name: a.label, mode: a.mode, unread: read === undefined ? "Reading…" : read.ok ? null : read.reason };
  });
  const allRead = listed.every((a) => a.unread === null);
  return {
    environment,
    total: null,
    invested: allRead ? sumReported(holdings.map((h) => h.marketValue)) : null,
    cash: null,
    unmanaged: null,
    today: null,
    pnl: null,
    unrealized: allRead ? sumReported(holdings.map((h) => h.unrealized)) : null,
    agents: listed,
    holdings,
  };
}

/** A series colour is an asset's share (DESIGN.md, the Meaning Rule); everything else in a breakdown is neutral. */
export type SliceTone = "series-1" | "series-2" | "series-3" | "series-4" | "neutral";

export interface Slice {
  key: string;
  label: string;
  value: string | null;
  /** Of the breakdown's whole, as a fraction of one to four places; `null` when the whole is not known. */
  share: string | null;
  tone: SliceTone;
  /** Where its row links: the agent's page, for an agent. */
  href?: string;
}

export interface Breakdown {
  /** The sum of the slices, or `null` when any one is not reported, so no share is shown. */
  total: string | null;
  slices: Slice[];
}

interface Part {
  key: string;
  label: string;
  value: string | null;
  href?: string;
}

/** Largest first, then by label; a value not reported goes last. */
function ranked<T extends { value: string | null; label: string }>(parts: T[]): T[] {
  return [...parts].sort((a, b) => {
    if (a.value === null || b.value === null) return a.value === b.value ? a.label.localeCompare(b.label) : a.value === null ? 1 : -1;
    const x = dec(a.value);
    const y = dec(b.value);
    return x === y ? a.label.localeCompare(b.label) : x > y ? -1 : 1;
  });
}

/** Each part with its share of the whole, every share in fixed point; with a part not reported, none has a share. */
export function breakdown(parts: Part[], tone: (part: Part, rank: number) => SliceTone = () => "neutral"): Breakdown {
  const total = sumReported(parts.map((p) => p.value));
  const whole = total === null ? null : dec(total);
  const slices = ranked(parts).map((p, i) => ({
    ...p,
    share: whole === null || p.value === null ? null : whole === ZERO ? "0.0000" : toFixed(div(dec(p.value), whole), 4),
    tone: tone(p, i),
  }));
  return { total, slices };
}

function grouped(holdings: readonly Holding[], keyOf: (h: Holding) => string, labelOf: (h: Holding) => string): Part[] {
  const groups = new Map<string, { label: string; values: Array<string | null> }>();
  for (const h of holdings) {
    const key = keyOf(h);
    const group = groups.get(key) ?? { label: labelOf(h), values: [] };
    group.values.push(h.marketValue);
    groups.set(key, group);
  }
  return [...groups].map(([key, g]) => ({ key, label: g.label, value: sumReported(g.values) }));
}

const SERIES: SliceTone[] = ["series-1", "series-2", "series-3", "series-4"];

/** The name an agent is shown by: the owner's name for it, or a generic one. */
export function agentName(name: string | null): string {
  return name ?? "Unnamed agent";
}

/** What is invested, by the agent holding it; an agent whose list was not read is not reported. */
export function byAgent(p: Portfolio, href: (agentId: string) => string): Breakdown {
  return breakdown(
    p.agents.map((a) => ({
      key: a.agentId,
      label: agentName(a.name),
      value: a.unread === null ? sumReported(p.holdings.filter((h) => h.agentId === a.agentId).map((h) => h.marketValue)) : null,
      href: href(a.agentId),
    })),
  );
}

/** What is invested, by instrument across every agent; each instrument keeps its series colour by rank. */
export function byInstrument(p: Portfolio): Breakdown {
  return breakdown(
    grouped(p.holdings, (h) => h.instrumentId, (h) => h.symbol ?? h.instrumentId),
    (_, rank) => SERIES[Math.min(rank, SERIES.length - 1)],
  );
}

const CLASS_WORD: Record<AssetClass, string> = { crypto: "Crypto", us_equity: "US equities" };

export function assetClassWord(c: AssetClass | null): string {
  return c === null ? "Asset class not reported" : CLASS_WORD[c];
}

/** What is invested, by asset class. */
export function byAssetClass(p: Portfolio): Breakdown {
  return breakdown(grouped(p.holdings, (h) => h.assetClass ?? "unreported", (h) => assetClassWord(h.assetClass)));
}

/** The account: what is invested, the agents' cash, and what no agent manages when there is any. */
export function cashAndInvested(p: Portfolio): Breakdown {
  const parts: Part[] = [
    { key: "invested", label: "Invested", value: p.invested },
    { key: "cash", label: "Agents' cash", value: p.cash },
  ];
  if (p.unmanaged === null || dec(p.unmanaged) !== ZERO) parts.push({ key: "unmanaged", label: "Not managed by an agent", value: p.unmanaged });
  return breakdown(parts);
}

/** The colour each instrument wears in the holdings, the same as in its breakdown. */
export function instrumentTones(p: Portfolio): Map<string, SliceTone> {
  return new Map(byInstrument(p).slices.map((s) => [s.key, s.tone]));
}
