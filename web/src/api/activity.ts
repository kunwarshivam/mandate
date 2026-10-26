/**
 * An agent's activity over the workspace API (§4.7 D3, DEC-2111): its orders as the account stream
 * journals them, and the net quantity its own fills left per instrument. Read only, and nothing
 * computed in a missing member's place: a state or filled quantity the journal has not given is
 * `null` ("not reported"), never assumed, and `unknown` is shown as unknown (trading spec §5.7). The
 * cost basis, risk mark and protection are always `null` until their folds are served (DEC-2111 item
 * 5), so the screen shows none of them.
 */
import { array, closedEnum, decimal, isRecord, nullable, object, string, timestamp } from "./decode";
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
