import type { AgentHolding } from "@/api/activity";
import type { CatalogueInstrument } from "@/api/catalogue";
import { type AgentPnl, type Dashboard, type EquityHistory, type HistoryRange, bucketOf } from "@/api/portfolio";
import type { AgentDetail } from "@/api/read-models";
import type { AgentMode, AssetClass, ShownEnvironment, Workspace } from "@/fixtures/types";
import type { EquityRange } from "./chart-data";
import { type Dec, ZERO, add, dec, div, sub, toDecimalString, toFixed } from "./decimal";

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
  /** The account's value at the broker: over the API, every account's latest snapshot `equity` summed. */
  total: string | null;
  /**
   * The broker's cash, every account's snapshot `cash` summed. Absent (`undefined`) where the source
   * holds no such figure, as the fixtures do, so the page leaves it out rather than say "not reported".
   */
  accountCash?: string | null;
  /** When the broker's figures were observed: the oldest account snapshot's `observed_at`. Absent over the fixtures. */
  observedAt?: string | null;
  /** `false` when an account's snapshot names another environment than the page shows: its value and history are then not reported. */
  accountsAgree?: boolean;
  /** What the agents hold, at market value. */
  invested: string | null;
  /** The agents' cash: each agent's equity less what it holds. */
  cash: string | null;
  /** The part of the account no agent manages: the account's value less the agents' equity. */
  unmanaged: string | null;
  today: string | null;
  pnl: string | null;
  /** The P&L is gross, before fees: over the API, which attributes no fee to an agent (§4.7). */
  pnlGross: boolean;
  unrealized: string | null;
  agents: PortfolioAgent[];
  holdings: Holding[];
}

const money = (value: Dec): string => toFixed(value, 2);

/** A served decimal as a fixed-point value, or `null` when it has more places than the arithmetic keeps: such a figure is not reported, never rounded in silence. */
function readDec(value: string): Dec | null {
  try {
    return dec(value);
  } catch {
    return null;
  }
}

/** The exact sum of every value, or `null` when any one is not reported or cannot be read. An empty list sums to zero. */
function exactSum(values: ReadonlyArray<string | null>): Dec | null {
  const read = values.map((v) => (v === null ? null : readDec(v)));
  return read.some((v) => v === null) ? null : add(...(read as Dec[]));
}

/** The sum of every value to the cent, or `null` when any one is not reported. An empty list sums to zero. */
export function sumReported(values: ReadonlyArray<string | null>): string | null {
  const sum = exactSum(values);
  return sum === null ? null : money(sum);
}

/** A served read, or why it was not: absent until it answers. */
export type ServedRead<T> = { ok: true; value: T } | { ok: false; reason: string };

/** The broker's figures across every account the dashboard serves. */
export interface AccountFigures {
  total: string | null;
  cash: string | null;
  today: string | null;
  observedAt: string | null;
  /**
   * Whether every account's snapshot names the environment the page shows. `false` when they differ
   * from each other or from it: paper and live money are never summed, nor labelled as one another.
   */
  agree: boolean;
}

/**
 * The accounts' value, cash and change on the day (DEC-2413), each summed in fixed point over every
 * account's latest snapshot. An account with no snapshot, a read that failed or has not answered, a
 * `day_change` a version 1 snapshot does not give, and a workspace with no account each make the sum
 * `null`: no part is ever read as zero. So does a snapshot whose environment is not the one the page
 * shows (`environment`), since a paper and a live account summed would carry one label for both.
 */
export function accountFigures(read: ServedRead<Dashboard> | undefined, environment: ShownEnvironment): AccountFigures {
  const none = { total: null, cash: null, today: null, observedAt: null };
  if (!read?.ok || read.value.accounts.length === 0) return { ...none, agree: true };
  const snapshots = read.value.accounts.map((a) => a.snapshot);
  const agree = snapshots.every((s) => s === null || s.environment === environment);
  if (!agree || snapshots.some((s) => s === null)) return { ...none, agree };
  const all = snapshots as NonNullable<(typeof snapshots)[number]>[];
  return {
    total: sumReported(all.map((s) => s.equity)),
    cash: sumReported(all.map((s) => s.cash)),
    today: sumReported(all.map((s) => s.day_change)),
    observedAt: all.map((s) => s.observed_at).reduce((oldest, at) => (at < oldest ? at : oldest)),
    agree,
  };
}

/**
 * The agents' total P&L, gross: each agent's `realized_gross` plus its `unrealized_pnl`, summed in
 * fixed point. An agent whose read failed or has not answered, or whose figure is `null`, makes the
 * total `null`. Fees are the account's and no agent's (§4.7), so this is never a net figure.
 */
export function grossPnl(agents: readonly { agent_id: string }[], reads: Readonly<Record<string, ServedRead<AgentPnl> | undefined>>): string | null {
  const parts = agents.flatMap((a) => {
    const read = reads[a.agent_id];
    return read?.ok ? [read.value.realized_gross, read.value.unrealized_pnl] : [null];
  });
  return sumReported(parts);
}

/** The page's ranges as the history read names them. */
export const HISTORY_RANGE_OF: Readonly<Record<EquityRange, HistoryRange>> = { "1D": "1d", "1W": "1w", "1M": "1m", "3M": "3m", "1Y": "1y", All: "all" };

/** One point of the account's line: the exact equity, and its time in Unix seconds for drawing. */
export interface AccountPoint {
  at: string;
  time: number;
  equity: string;
}

const unixSeconds = (at: string) => Date.parse(`${at.slice(0, 19)}Z`) / 1000;

/** The account's line, or why it is not reported, in words the page shows after "Not reported:". */
export type AccountLine = { ok: true; points: AccountPoint[] } | { ok: false; reason: string };

/**
 * The account's line over the range read (DEC-2413): with one account, its points; with more, one
 * point for each bucket where every account has one, their equity summed in fixed point and timed at
 * the latest of them. A bucket missing an account is left out, since no snapshot is carried into a
 * bucket it is not in (§4.7). An empty line is a range in which no account has a snapshot. It is not
 * reported with no account, when one account has no point in the range while another has, since
 * that account's part of every point is missing, or with an equity that cannot be read.
 */
export function accountLine(history: EquityHistory): AccountLine {
  if (history.accounts.length === 0) return { ok: false, reason: "no account" };
  const empty = history.accounts.filter((a) => a.points.length === 0).length;
  if (empty === history.accounts.length) return { ok: true, points: [] };
  if (empty > 0) return { ok: false, reason: "an account has no snapshot in this range" };
  const byBucket = history.accounts.map((a) => new Map(a.points.map((p) => [bucketOf(p.at, history.range), p])));
  const buckets = [...byBucket[0].keys()].filter((b) => byBucket.every((m) => m.has(b))).sort((x, y) => x - y);
  const points: AccountPoint[] = [];
  for (const b of buckets) {
    const parts = byBucket.map((m) => m.get(b) as { at: string; equity: string });
    const sum = exactSum(parts.map((p) => p.equity));
    if (sum === null) return { ok: false, reason: "an equity has more places than the page can sum" };
    const at = parts.map((p) => p.at).reduce((latest, t) => (t > latest ? t : latest));
    points.push({ at, time: unixSeconds(at), equity: toDecimalString(sum) });
  }
  return { ok: true, points };
}

/** The change over the line, last less first, to the cent; `null` with fewer than two points. */
export function lineChange(line: readonly AccountPoint[], upTo: AccountPoint | null = null): string | null {
  const last = upTo ?? line.at(-1);
  if (line.length < 2 || !last) return null;
  return money(sub(dec(last.equity), dec(line[0].equity)));
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
    pnlGross: false,
    unrealized: sumReported(holdings.map((h) => h.unrealized)),
    agents: ws.agents.map((a) => ({ agentId: a.agent_id, name: a.label, mode: a.mode, unread: null })),
    holdings,
  };
}

/** One agent's `GET /agents/{id}/positions`, as the runtime answered it; absent until it answers. */
export type HoldingsRead = { ok: true; items: AgentHolding[] } | { ok: false; reason: string };

/** The Portfolio reads beside the positions (§4.7, DEC-2413): the dashboard's snapshots and each agent's P&L, each absent until it answers. */
export interface PortfolioReads {
  dashboard?: ServedRead<Dashboard>;
  pnl: Readonly<Record<string, ServedRead<AgentPnl> | undefined>>;
}

const NO_READS: PortfolioReads = { pnl: {} };

/**
 * The workspace's figures over the workspace API (DEC-2413 item 3): only what the served reads give.
 * The account's value, cash and change today are the dashboard's snapshots summed; the total P&L is
 * each agent's gross P&L summed (`grossPnl`). What is invested and unrealized is summed from the
 * agents' positions only when every agent's list was read and every figure in it is reported. The
 * agents' cash and what no agent manages need each agent's equity, which no read serves, so they stay
 * `null`. The average cost is the sub-ledger's basis over its quantity, and the last price the latest
 * risk mark, only when it is sane.
 */
export function apiPortfolio(
  environment: ShownEnvironment,
  agents: readonly AgentDetail[],
  reads: Readonly<Record<string, HoldingsRead | undefined>>,
  instruments: readonly CatalogueInstrument[] | null,
  served: PortfolioReads = NO_READS,
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
  const account = accountFigures(served.dashboard, environment);
  return {
    environment,
    total: account.total,
    accountCash: account.cash,
    observedAt: account.observedAt,
    accountsAgree: account.agree,
    invested: allRead ? sumReported(holdings.map((h) => h.marketValue)) : null,
    cash: null,
    unmanaged: null,
    today: account.today,
    pnl: grossPnl(agents, served.pnl),
    pnlGross: true,
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
