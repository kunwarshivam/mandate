/**
 * The Portfolio page's reads (workspace API spec §4.7, DEC-2413): the dashboard's account snapshots
 * (`read-models/dashboard`), each account's equity over a range (`read-models/equity-history`), and
 * an agent's gross P&L (`read-models/pnl`). Every member the schemas require is required here, and a
 * figure the journal does not give is `null`, never a zero: a version 1 snapshot names no prior close,
 * so its `last_equity` and `day_change` are `null`, and an agent's P&L is `null` where its basis is not
 * served. The P&L is gross: the journal attributes fees and income to no agent, so no net figure is
 * served or derived. A member the schemas do not name is ignored (§3.2), as every read's is.
 */
import { array, closedEnum, decimal, isRecord, nullable, object, timestamp } from "./decode";
import { type AgentRow, agentRow, id } from "./read-models";
import type { Decimal, Decoded, DecodeProblem, Decoder, Timestamp } from "./types";
import type { Environment } from "@/fixtures/types";

/** `read-models/dashboard`'s `Snapshot`: the broker's figures as the latest `AccountSnapshotRecorded` records them (journal spec §9.35). */
export interface AccountSnapshot {
  equity: Decimal;
  cash: Decimal;
  buying_power: Decimal;
  /** The prior session's close, from a version 2 snapshot; `null` from a version 1, never zero and never `equity`. */
  last_equity: Decimal | null;
  /** `equity` less `last_equity`, signed; `null` when `last_equity` is. */
  day_change: Decimal | null;
  environment: Environment;
  /** The snapshot row's `recorded_at`. */
  observed_at: Timestamp;
}

export interface DashboardAccount {
  account_ref: string;
  /** `null` while the account stream holds no snapshot: not reported, never an empty account. */
  snapshot: AccountSnapshot | null;
}

export interface Dashboard {
  agents: AgentRow[];
  accounts: DashboardAccount[];
}

/** `?range=` (§4.7): each a span back from `served_at`, `all` to the stream's first row. */
export const HISTORY_RANGES = ["1d", "1w", "1m", "3m", "1y", "all"] as const;
export type HistoryRange = (typeof HISTORY_RANGES)[number];

/** Seconds per bucket (§4.7): 300 for `1d`, 3 600 for `1w`, a UTC day otherwise, counted from the Unix epoch. */
export const BUCKET_SECONDS: Readonly<Record<HistoryRange, number>> = { "1d": 300, "1w": 3600, "1m": 86_400, "3m": 86_400, "1y": 86_400, all: 86_400 };

export interface EquityPoint {
  at: Timestamp;
  equity: Decimal;
}

export interface HistoryAccount {
  account_ref: string;
  /** Oldest first, one per bucket that holds a snapshot; none interpolated or carried. */
  points: EquityPoint[];
}

export interface EquityHistory {
  range: HistoryRange;
  accounts: HistoryAccount[];
}

/** One instrument the agent's own fills touched, a closed position included. */
export interface PnlInstrument {
  instrument_id: string;
  filled_qty: Decimal;
  /** Trading spec §8.1's realized column, fees not taken off; `null` where the positions read's basis is. */
  realized_gross: Decimal | null;
  /** §8.2's at the positions read's risk mark, `0` when flat; `null` where the basis or a sane mark is not served. */
  unrealized_pnl: Decimal | null;
}

export interface AgentPnl {
  agent_id: string;
  environment: Environment;
  /** `null` when the agent's account stream is not found: then nothing was traded on it. */
  account_ref: string | null;
  instruments: PnlInstrument[];
  /** The instruments' sum, `null` when any one is. Gross: before fees, which no agent is attributed. */
  realized_gross: Decimal | null;
  /** The instruments' sum, `null` when any one is. */
  unrealized_pnl: Decimal | null;
}

export const DASHBOARD_ROUTE = "/dashboard" as const;
export const historyRoute = (range: HistoryRange) => `/dashboard/history?range=${range}` as const;
export const pnlRoute = (agentId: string) => `/agents/${encodeURIComponent(agentId)}/pnl` as const;

const fail = (path: string, problem: DecodeProblem, value: string | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed: null } });

const environment = closedEnum<Environment>(["paper", "live"]);

const snapshot = object<AccountSnapshot>({
  equity: decimal,
  cash: decimal,
  buying_power: decimal,
  last_equity: nullable(decimal),
  day_change: nullable(decimal),
  environment,
  observed_at: timestamp,
});

/** `read-models/dashboard`: the agents as `GET /agents` lists them, and every account with its latest snapshot or `null`. */
export const dashboardRead: Decoder<Dashboard> = object<Dashboard>({
  agents: array(agentRow),
  accounts: array(object<DashboardAccount>({ account_ref: id, snapshot: nullable(snapshot) })),
});

const point = object<EquityPoint>({ at: timestamp, equity: decimal });

/** Points oldest first, each in a later bucket than the one before: a timestamp's fixed width orders as its time does. */
const points: Decoder<EquityPoint[]> = (value, path) => {
  const read = array(point)(value, path);
  if (!read.ok) return read;
  const back = read.value.findIndex((p, i) => i > 0 && p.at <= read.value[i - 1].at);
  return back === -1 ? read : fail(`${path}/${back}/at`, "not_canonical", read.value[back].at);
};

/** `read-models/equity-history` for the range asked: an answer for another range is refused, so a chart never draws one range as another. */
export function historyRead(range: HistoryRange): Decoder<EquityHistory> {
  const body = object<EquityHistory>({ range: closedEnum(HISTORY_RANGES), accounts: array(object<HistoryAccount>({ account_ref: id, points })) });
  return (value, path) => {
    const read = body(value, path);
    if (!read.ok) return read;
    return read.value.range === range ? read : fail(`${path}/range`, "not_canonical", read.value.range);
  };
}

const instrumentId: Decoder<string> = (value, path) =>
  typeof value !== "string" ? fail(path, "wrong_type") : value.length >= 1 && value.length <= 200 ? { ok: true, value } : fail(path, "not_canonical", value);

const pnlBody = object<AgentPnl>({
  agent_id: id,
  environment,
  account_ref: nullable(id),
  instruments: array(object<PnlInstrument>({ instrument_id: instrumentId, filled_qty: decimal, realized_gross: nullable(decimal), unrealized_pnl: nullable(decimal) })),
  realized_gross: nullable(decimal),
  unrealized_pnl: nullable(decimal),
});

/** `read-models/pnl` for the agent asked: an answer naming another agent is refused, so one agent's P&L is never shown as another's. */
export function pnlRead(agentId: string): Decoder<AgentPnl> {
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    const read = pnlBody(value, path);
    if (!read.ok) return read;
    return read.value.agent_id === agentId ? read : fail(`${path}/agent_id`, "not_canonical", read.value.agent_id);
  };
}
