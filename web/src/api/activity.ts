/**
 * An agent's activity over the workspace API (§4.7 D3, DEC-2111): its orders as the account stream
 * journals them, and the net quantity its own fills left per instrument. Read only, and nothing
 * computed in a missing member's place: a state or filled quantity the journal has not given is
 * `null` ("not reported"), never assumed, and `unknown` is shown as unknown (trading spec §5.7). The
 * cost basis, risk mark and protection are always `null` until their folds are served (DEC-2111 item
 * 5), so the screen shows none of them.
 */
import { array, closedEnum, decimal, integer, isRecord, nullable, object, string, timestamp } from "./decode";
import { type ReadPosition, reconciledHolding } from "./read-models";
import type { Decimal, Decoded, DecodeProblem, Decoder, Timestamp } from "./types";

/** `read-models/orders`' states (journal spec §9.5). Closed. */
const STATES = ["intent", "submitting", "accepted", "partially_filled", "pending_cancel", "pending_replace", "unknown", "filled", "canceled", "rejected", "expired", "replaced", "abandoned"] as const;
export type AgentOrderState = (typeof STATES)[number];

const PURPOSES = ["open", "increase", "discretionary_exit", "risk_exit", "owner_exit", "protective", "flatten"] as const;

export interface AgentOrder {
  client_order_id: string;
  purpose: (typeof PURPOSES)[number];
  instrument_id: string;
  side: "buy" | "sell";
  type: string;
  tif: string;
  qty: Decimal;
  limit_price: Decimal | null;
  stop_price: Decimal | null;
  state: AgentOrderState | null;
  filled_qty: Decimal | null;
  submitted_at: Timestamp;
  updated_at: Timestamp;
}

export interface AgentHolding {
  instrument_id: string;
  filled_qty: Decimal;
}

export interface Activity {
  agent_id: string;
  /** `null` when the agent's document or account stream is not found: then nothing is reported, which is not "none". */
  account_ref: string | null;
}

const fail = (path: string, problem: DecodeProblem): Decoded<never> => ({ ok: false, issue: { path, problem, value: null, allowed: null } });

const bounded =
  (max: number): Decoder<string> =>
  (value, path) =>
    typeof value !== "string" ? fail(path, value === undefined ? "missing" : "wrong_type") : value.length >= 1 && value.length <= max ? { ok: true, value } : fail(path, "not_canonical");

const order = object<AgentOrder>({
  client_order_id: bounded(128),
  purpose: closedEnum(PURPOSES),
  instrument_id: bounded(200),
  side: closedEnum(["buy", "sell"] as const),
  type: bounded(32),
  tif: bounded(32),
  qty: decimal,
  limit_price: nullable(decimal),
  stop_price: nullable(decimal),
  state: nullable(closedEnum(STATES)),
  filled_qty: nullable(decimal),
  submitted_at: timestamp,
  updated_at: timestamp,
});

const holding = object<AgentHolding>({ instrument_id: bounded(200), filled_qty: decimal });

/** `read-models/orders` and `read-models/positions`, each with the agent it names; an answer for another agent is refused. */
export function activityRead(agentId: string) {
  const head = (value: unknown, path: string): Decoded<Activity> => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    if (value.agent_id !== agentId) return fail(`${path}/agent_id`, "not_canonical");
    const ref = nullable(string)(value.account_ref ?? null, `${path}/account_ref`);
    return ref.ok ? { ok: true, value: { agent_id: agentId, account_ref: ref.value } } : ref;
  };
  const listed =
    <T,>(member: string, item: Decoder<T>): Decoder<Activity & { items: T[] }> =>
    (value, path) => {
      const top = head(value, path);
      if (!top.ok) return top;
      const items = array(item)((value as Record<string, unknown>)[member], `${path}/${member}`);
      return items.ok ? { ok: true, value: { ...top.value, items: items.value } } : items;
    };
  return { orders: listed("orders", order), holdings: listed("positions", holding) };
}

/**
 * `read-models/positions` whole (DEC-2111): the agent's own positions with the account's holdings at
 * its last reconciliation (`account_positions`, `null` when its stream is not found), from one read,
 * so the two the positions screen shows side by side are of one moment.
 */
export function holdingsRead(agentId: string): Decoder<Activity & { items: AgentHolding[]; account_positions: ReadPosition[] | null }> {
  const own = activityRead(agentId).holdings;
  return (value, path) => {
    const read = own(value, path);
    if (!read.ok) return read;
    const account = nullable(array(reconciledHolding))((value as Record<string, unknown>).account_positions ?? null, `${path}/account_positions`);
    return account.ok ? { ok: true, value: { ...read.value, account_positions: account.value } } : account;
  };
}

/** One event of a journal page, without its body: the screen names what happened and when, and decodes no payload (API-18). */
export interface JournalLine {
  seq: number;
  event_id: string;
  event_type: string;
  recorded_at: Timestamp;
}

export interface JournalPage {
  stream_id: string;
  head_seq: number;
  events: JournalLine[];
}

/** `GET /journal/streams/{id}/events` (§4.8.1, API-15): the page, for the stream asked; members beyond these are ignored (§3.2). */
export function journalPageRead(streamId: string): Decoder<JournalPage> {
  const line = object<JournalLine>({ seq: integer, event_id: string, event_type: bounded(128), recorded_at: timestamp });
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    if (value.stream_id !== streamId) return fail(`${path}/stream_id`, "not_canonical");
    const head = isRecord(value.head) ? integer(value.head.seq, `${path}/head/seq`) : fail(`${path}/head`, "missing");
    if (!head.ok) return head;
    const events = array(line)(value.events, `${path}/events`);
    if (!events.ok) return events;
    if (events.value.some((e, i) => e.seq > head.value || (i > 0 && e.seq <= events.value[i - 1].seq))) return fail(`${path}/events`, "not_canonical");
    return { ok: true, value: { stream_id: streamId, head_seq: head.value, events: events.value } };
  };
}
