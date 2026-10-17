/**
 * The workspace as the screens read it, built from the workspace API's documented reads (spec §4.3,
 * §4.5, §4.7, §4.9) instead of `src/fixtures/` (E11-9, DEC-970). Until `schemas/workspace-api/read-models/`
 * lands, each read's body carries the fixture type of §4.9's table under one member, which DEC-970
 * pins; the decoders refuse what would mislead a screen: a decimal sent as a JSON number, a value a
 * closed safety enum does not list, an approval without the content hash an answer must bind.
 */
import type { WorkspaceClient } from "./client";
import { array, closedEnum, contentRef, isRecord, nullable, object, string, timestamp } from "./decode";
import type { ApiError, DecodeIssue, Decoder, ReadOutcome, Watermark } from "./types";
import type {
  ActiveRestriction,
  Agent,
  AgentMode,
  Approval,
  ApprovalStatus,
  Connection,
  Environment,
  ExternalPosition,
  Fill,
  GateDecision,
  Health,
  HealthState,
  PastOrder,
  Position,
  Purpose,
  ReasonCode,
  RestrictionCode,
  TimelineEvent,
  TimelineKind,
  WorkingOrder,
  Workspace,
} from "@/fixtures/types";
import { APPROVAL_STATUS_LABEL, MODE_LABEL, ORDER_STATE_LABEL, PURPOSE_LABEL } from "@/lib/labels";
import { RESTRICTIONS } from "@/lib/restrictions";

export type Loaded = { ok: true; ws: Workspace; as_of: Watermark[] } | { ok: false; error: ApiError };

/** Every value of a closed union, kept complete by the compiler: a missing or extra key fails the build. */
const valuesOf = <T extends string>(all: Record<T, unknown>): T[] => Object.keys(all) as T[];

const REASON_CODES: Record<ReasonCode, true> = {
  account_restricted: true,
  account_trading_blocked: true,
  agent_exits_only: true,
  agent_paused: true,
  agent_stopped: true,
  not_in_universe: true,
  concentration_limit: true,
  max_order_size: true,
  reentry_cooldown: true,
  session_not_allowed: true,
  extended_hours_opening_not_allowed: true,
  auction_window: true,
  instrument_halted: true,
  unknown_order_in_flight: true,
  stale_mark: true,
  price_outside_collar: true,
  max_orders_per_day: true,
  close_window: true,
  discretionary_exit_regular_session_only: true,
  owner_confirmation_required: true,
  gross_exposure_limit: true,
  insufficient_buying_power: true,
};
const TIMELINE_KINDS: Record<TimelineKind, true> = { fill: true, order: true, mode: true, approval: true, gate: true, protection: true, reconciliation: true, version: true };
const HEALTH_STATES: Record<HealthState, true> = { ok: true, stale: true, down: true };
const ENVIRONMENTS: Record<Environment, true> = { paper: true, live: true };

const mode = closedEnum<AgentMode>(valuesOf(MODE_LABEL));
const environment = closedEnum<Environment>(valuesOf(ENVIRONMENTS));
const restrictionCode = closedEnum<RestrictionCode>(valuesOf(RESTRICTIONS));
const orderState = closedEnum(valuesOf(ORDER_STATE_LABEL));
const purpose = closedEnum<Purpose>(valuesOf(PURPOSE_LABEL));
const approvalStatus = closedEnum<ApprovalStatus>(valuesOf(APPROVAL_STATUS_LABEL));
const healthState = closedEnum<HealthState>(valuesOf(HEALTH_STATES));

/**
 * The members that are counts, seconds or versions (`src/fixtures/types.ts`): the only places a JSON
 * number may stand, and only as a non-negative integer. A changed path's `from` and `to` may hold either
 * a decimal string or such a count. Everywhere else a number is a decimal sent as a float, refused.
 */
const INTEGERS = new Set([
  "mandate_schema_version",
  "max_instruments",
  "max_output_age_s",
  "interval_s",
  "max_orders_per_day",
  "breach_confirm_s",
  "daily_breach_min_s",
  "scale_lift_after_s",
  "reentry_cooldown_s",
  "timeout_s",
  "orders_today",
  "seconds_in_breach",
  "confirm_after_s",
  "approvals_canceled",
  "approvers_required",
  "seq",
]);
const EITHER = new Set(["from", "to"]);

const isCount = (value: unknown) => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const issue = (path: string, problem: DecodeIssue["problem"], value: string | null = null): DecodeIssue => ({ path, problem, value, allowed: null });
const pointer = (path: string, member: string) => `${path}/${member.replace(/~/g, "~0").replace(/\//g, "~1")}`;

/** The first number in `value` that is not a count where a count belongs, or a count member that is not one. */
function numbersIn(value: unknown, path: string, member: string): DecodeIssue | null {
  if (INTEGERS.has(member)) return isCount(value) ? null : issue(path, "wrong_type");
  if (typeof value === "number") return EITHER.has(member) && isCount(value) ? null : issue(path, "decimal_number", String(value));
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      const found = numbersIn(item, `${path}/${index}`, member);
      if (found) return found;
    }
    return null;
  }
  if (isRecord(value)) {
    for (const [key, item] of Object.entries(value)) {
      const found = numbersIn(item, pointer(path, key), key);
      if (found) return found;
    }
  }
  return null;
}

/**
 * A fixture shape read whole: the named members decoded, every number checked, and the value kept as
 * sent. The named members are those a screen branches on, so an unlisted value never renders as another.
 */
function typed<T>(members: Record<string, Decoder<unknown>>): Decoder<T> {
  const check = object<Record<string, unknown>>(members);
  return (value, path) => {
    const numbers = numbersIn(value, path, "");
    if (numbers) return { ok: false, issue: numbers };
    const checked = check(value, path);
    return checked.ok ? { ok: true, value: value as T } : checked;
  };
}

const restriction = object<Pick<ActiveRestriction, "code" | "since">>({ code: restrictionCode, since: string });
const workingOrder = typed<WorkingOrder>({ client_order_id: string, state: orderState, purpose, side: closedEnum(["buy", "sell"]) });
const pastOrder = typed<PastOrder>({ client_order_id: string, state: closedEnum(["Filled", "Canceled", "Rejected", "Expired", "Replaced", "Abandoned"]), purpose, side: closedEnum(["buy", "sell"]) });
const position = typed<Position>({ instrument: object({ asset_id: string, symbol: string, asset_class: closedEnum(["crypto", "us_equity"]) }), qty: string });
const fill = typed<Fill>({ fill_id: string, client_order_id: string, side: closedEnum(["buy", "sell"]) });

type AgentCore = Omit<Agent, "positions" | "orders" | "past_orders" | "fills">;
const agentCore = typed<AgentCore>({
  agent_id: string,
  label: string,
  mode,
  startup: closedEnum(["reconciling", "ready"]),
  restrictions: array(restriction),
  mandate: object({ environment, connection_id: string }),
  mandate_version: contentRef,
});

const approval = typed<Approval & { content_hash: `sha256:${string}` }>({
  approval_id: string,
  agent_id: string,
  status: approvalStatus,
  content_hash: contentRef,
  deadline: string,
  bound: object({ side: closedEnum(["buy"]), purpose: closedEnum(["open", "increase"]), symbol: string, qty: string, limit: string }),
});

const decision = typed<GateDecision>({
  event_id: string,
  agent_id: string,
  verdict: closedEnum(["allow", "deny", "defer"]),
  reason_code: nullable(closedEnum(valuesOf(REASON_CODES))),
  action: object({ side: closedEnum(["buy", "sell"]), purpose }),
});
const timelineEvent = typed<TimelineEvent>({ event_id: string, at: string, kind: closedEnum(valuesOf(TIMELINE_KINDS)), text: string });
const externalPosition = typed<ExternalPosition>({ qty: string });

interface ServedComponent {
  state: HealthState;
  freshness: { observed_at: string; stale: boolean };
}
const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : { ok: false, issue: issue(path, "wrong_type") });
const component = object<ServedComponent>({ state: healthState, freshness: object({ observed_at: timestamp, stale: flag }) });
const health = object<{ served_at: string } & Record<keyof Health, ServedComponent>>({ served_at: timestamp, market_data: component, broker: component, deployment: component, relay: component });

const connection = typed<Connection & { environment: Environment }>({ connection_id: string, broker: closedEnum(["Alpaca paper"]), environment });

const agentIdList = object<{ agents: Array<{ agent_id: string }> }>({ agents: array(object({ agent_id: string })) });
const approvalIdList = object<{ approvals: Array<{ approval_id: string }> }>({ approvals: array(object({ approval_id: string })) });

/** Reads every route in parallel; the first failure, in route order, is the answer. */
async function all<T extends readonly unknown[]>(reads: { [K in keyof T]: Promise<ReadOutcome<T[K]>> }): Promise<{ ok: true; values: T; as_of: Watermark[] } | { ok: false; error: ApiError }> {
  const outcomes = await Promise.all(reads);
  const values: unknown[] = [];
  const marks: Watermark[] = [];
  for (const outcome of outcomes) {
    if (!outcome.ok) return { ok: false, error: outcome.error };
    values.push(outcome.value);
    marks.push(...outcome.as_of);
  }
  return { ok: true, values: values as unknown as T, as_of: marks };
}

/** The newest watermark per stream, so the workspace says how current each stream it read is (API-14). */
function newest(marks: Watermark[]): Watermark[] {
  const by = new Map<string, Watermark>();
  for (const mark of marks) {
    const seen = by.get(mark.stream_id);
    if (!seen || mark.seq > seen.seq) by.set(mark.stream_id, mark);
  }
  return [...by.values()];
}

/** A read that answered for another id than the one asked: the pairing cannot be trusted, so nothing is shown. */
function mismatch(path: string): Loaded {
  return { ok: false, error: { code: "invalid_response", effect: "none", status: 200, event_id: null, retryable: true, violations: [], decode: issue(path, "wrong_type") } };
}

export async function loadWorkspace(client: WorkspaceClient): Promise<Loaded> {
  const first = await all<[
    { served_at: string } & Record<keyof Health, ServedComponent>,
    { decisions: GateDecision[]; external_positions: ExternalPosition[] },
    { agents: Array<{ agent_id: string }> },
    { approvals: Array<{ approval_id: string }> },
    { approvals: Array<{ approval_id: string }> },
    { connections: Array<Connection & { environment: Environment }> },
  ]>([
    client.read("/health", health),
    client.read("/dashboard", object({ decisions: array(decision), external_positions: array(externalPosition) })),
    client.read("/agents", agentIdList),
    client.read("/approvals?state=pending", approvalIdList),
    client.read("/approvals?state=resolved", approvalIdList),
    client.read("/connections", object({ connections: array(connection) })),
  ]);
  if (!first.ok) return first;
  const [served, dashboard, agentList, pending, resolved, connections] = first.values;
  const held = connections.connections[0];
  if (!held) return { ok: false, error: { code: "invalid_response", effect: "none", status: 200, event_id: null, retryable: true, violations: [], decode: issue("/connections", "missing") } };

  const enc = encodeURIComponent;
  const agentIds = agentList.agents.map((a) => a.agent_id);
  const approvalIds = [...new Set([...pending.approvals, ...resolved.approvals].map((a) => a.approval_id))];
  const second = await all<unknown[]>([
    ...agentIds.flatMap((id) => [
      client.read(`/agents/${enc(id)}`, object({ agent: agentCore })),
      client.read(`/agents/${enc(id)}/positions`, object({ positions: array(position) })),
      client.read(`/agents/${enc(id)}/orders`, object({ orders: array(workingOrder), past_orders: array(pastOrder), fills: array(fill) })),
      client.read(`/agents/${enc(id)}/timeline`, object({ events: array(timelineEvent) })),
    ]),
    ...approvalIds.map((id) => client.read(`/approvals/${enc(id)}`, object({ approval }))),
  ]);
  if (!second.ok) return second;

  const agents: Agent[] = [];
  const timeline: Workspace["timeline"] = {};
  for (let i = 0; i < agentIds.length; i += 1) {
    const [core, positions, orders, events] = second.values.slice(i * 4, i * 4 + 4) as [
      { agent: AgentCore },
      { positions: Position[] },
      { orders: WorkingOrder[]; past_orders: PastOrder[]; fills: Fill[] },
      { events: TimelineEvent[] },
    ];
    if (core.agent.agent_id !== agentIds[i]) return mismatch(`/agents/${i}`);
    agents.push({ ...core.agent, positions: positions.positions, orders: orders.orders, past_orders: orders.past_orders, fills: orders.fills } as Agent);
    timeline[core.agent.agent_id] = events.events;
  }
  const approvals = (second.values.slice(agentIds.length * 4) as Array<{ approval: Approval }>).map((a) => a.approval);
  const wrong = approvals.findIndex((a, i) => a.approval_id !== approvalIds[i]);
  if (wrong >= 0) return mismatch(`/approvals/${wrong}`);
  /** A component marked stale in its freshness is stale, whatever its state says (API-14). */
  const at = (c: ServedComponent) => ({ state: c.state === "ok" && c.freshness.stale ? ("stale" as const) : c.state, as_of: c.freshness.observed_at });
  const { environment: env, ...conn } = held;
  const ws: Workspace = {
    scenario: "normal",
    status: "ready",
    journal: "answers",
    now: served.served_at,
    environment: env,
    connection: conn,
    approver_users: 0,
    independent_approval_required: null,
    health: { market_data: at(served.market_data), broker: at(served.broker), deployment: at(served.deployment), relay: at(served.relay) },
    agents,
    approvals,
    decisions: dashboard.decisions,
    timeline,
    external_positions: dashboard.external_positions,
  };
  return { ok: true, ws, as_of: newest([...first.as_of, ...second.as_of]) };
}

/** The workspace the screens show before the first read answers, or when none can: no agent, no request, no figure. */
export function emptyWorkspace(status: Workspace["status"], now: string): Workspace {
  const feed = { state: "stale" as const, as_of: now };
  return {
    scenario: "normal",
    status,
    journal: "answers",
    now,
    environment: "paper",
    connection: { connection_id: "", broker: "Alpaca paper", account_equity: "0", day_trading_regime: "intraday_margin" },
    approver_users: 0,
    independent_approval_required: null,
    health: { market_data: feed, broker: feed, deployment: status === "unreachable" ? { state: "down", as_of: now } : feed, relay: feed },
    agents: [],
    approvals: [],
    decisions: [],
    timeline: {},
    external_positions: [],
  };
}
