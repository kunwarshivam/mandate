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

/** Each account once (§4.7 follows each account stream once): a repeated `account_ref` is refused, so no account is summed twice. */
function distinctAccounts<T extends { account_ref: string }>(accounts: Decoder<T[]>): Decoder<T[]> {
  return (value, path) => {
    const read = accounts(value, path);
    if (!read.ok) return read;
    const seen = new Set<string>();
    for (const [i, a] of read.value.entries()) {
      if (seen.has(a.account_ref)) return fail(`${path}/${i}/account_ref`, "not_canonical", a.account_ref);
      seen.add(a.account_ref);
    }
    return read;
  };
}

/** `read-models/dashboard`: the agents as `GET /agents` lists them, and every account once with its latest snapshot or `null`. */
export const dashboardRead: Decoder<Dashboard> = object<Dashboard>({
  agents: array(agentRow),
  accounts: distinctAccounts(array(object<DashboardAccount>({ account_ref: id, snapshot: nullable(snapshot) }))),
});

const point = object<EquityPoint>({ at: timestamp, equity: decimal });

/** A timestamp's bucket for the range (§4.7): whole buckets of the range's width, counted from the Unix epoch. */
export function bucketOf(at: Timestamp, range: HistoryRange): number {
  return Math.floor(Date.parse(`${at.slice(0, 19)}Z`) / 1000 / BUCKET_SECONDS[range]);
}

/** Points oldest first, one per bucket of the range (§4.7): a point in the same bucket as the one before, or earlier, is refused. */
function points(range: HistoryRange): Decoder<EquityPoint[]> {
  return (value, path) => {
    const read = array(point)(value, path);
    if (!read.ok) return read;
    const back = read.value.findIndex((p, i) => i > 0 && (p.at <= read.value[i - 1].at || bucketOf(p.at, range) === bucketOf(read.value[i - 1].at, range)));
    return back === -1 ? read : fail(`${path}/${back}/at`, "not_canonical", read.value[back].at);
  };
}

/** `read-models/equity-history` for the range asked: an answer for another range is refused, so a chart never draws one range as another. */
export function historyRead(range: HistoryRange): Decoder<EquityHistory> {
  const body = object<EquityHistory>({ range: closedEnum(HISTORY_RANGES), accounts: distinctAccounts(array(object<HistoryAccount>({ account_ref: id, points: points(range) }))) });
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

/** A canonical decimal (journal spec §4.6, at most 28 places) as an integer of 10^-28 units: exact, so a sum is compared without rounding. */
const UNITS = 28;
function units(text: Decimal): bigint {
  const negative = text.startsWith("-");
  const [whole, fraction = ""] = text.replace(/^-/, "").split(".");
  const scaled = BigInt(whole) * 10n ** BigInt(UNITS) + BigInt(fraction.padEnd(UNITS, "0"));
  return negative ? -scaled : scaled;
}

/** The instruments' exact sum, or `null` when any one is `null` (§4.7). An empty list sums to zero. */
function exactSum(values: ReadonlyArray<Decimal | null>): bigint | null {
  return values.some((v) => v === null) ? null : (values as Decimal[]).reduce((sum, v) => sum + units(v), 0n);
}

/** A top-level figure that is not its instruments' sum: `null` exactly when any instrument's is, and otherwise equal to their sum. */
function sumMismatch(total: Decimal | null, parts: ReadonlyArray<Decimal | null>): boolean {
  const sum = exactSum(parts);
  return sum === null ? total !== null : total === null || units(total) !== sum;
}

/**
 * `read-models/pnl` for the agent asked: an answer naming another agent is refused, so one agent's
 * P&L is never shown as another's, and so is one whose top-level `realized_gross` or
 * `unrealized_pnl` is not its instruments' sum (§4.7), so the page never shows a total its own
 * parts contradict.
 */
export function pnlRead(agentId: string): Decoder<AgentPnl> {
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    const read = pnlBody(value, path);
    if (!read.ok) return read;
    const p = read.value;
    if (p.agent_id !== agentId) return fail(`${path}/agent_id`, "not_canonical", p.agent_id);
    if (sumMismatch(p.realized_gross, p.instruments.map((i) => i.realized_gross))) return fail(`${path}/realized_gross`, "not_canonical", p.realized_gross);
    if (sumMismatch(p.unrealized_pnl, p.instruments.map((i) => i.unrealized_pnl))) return fail(`${path}/unrealized_pnl`, "not_canonical", p.unrealized_pnl);
    return read;
  };
}
