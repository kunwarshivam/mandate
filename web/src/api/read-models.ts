/**
 * The workspace API's read models, decoded as they are served (workspace API spec §4.3 and §4.7;
 * `schemas/workspace-api/read-models/`, lane A1's #1446 and #1499; DEC-1096). A member the API does not
 * serve, or serves as `null`, is `null` here and "not reported" on screen: nothing is filled in from
 * the fixtures or computed in its place (rule 3). The reads the schemas do not define yet (health,
 * connections) are optional: a 404 is "not reported", never the deployment down.
 */
import { CONNECTION_STATES, type ConnectionState } from "./connect";
import type { WorkspaceClient } from "./client";
import { array, closedEnum, contentRef, decimal, integer, isRecord, nullable, object, string, timestamp } from "./decode";
import { emptyWorkspace } from "./reads";
import type { ApiError, Decoded, DecodeIssue, Decoder, ReadOutcome, Watermark } from "./types";
import type { AgentMode, AssetClass, Environment, Workspace } from "@/fixtures/types";

export interface MandateSummary {
  allocation_usd: string;
  max_loss_from_allocation: string;
  goal: "continuous" | "accumulate" | "profit_stop";
  goal_end_date: string | null;
  asset_classes: AssetClass[];
  max_position_usd: string;
  max_order_usd: string;
  max_orders_per_day: number;
  max_daily_loss: string;
  max_drawdown: string;
}

export interface AgentRow {
  agent_id: string;
  environment: Environment;
  mandate_version: `sha256:${string}`;
  deployed_at: string;
  /** `null`: no mode change journaled yet; never read as `normal`. */
  mode: AgentMode | null;
  pending_approvals: number;
  label: string | null;
  mandate: MandateSummary | null;
}

export interface ReadPosition {
  instrument: string;
  qty: string;
  broker_qty: string;
  mismatch: boolean;
  observed_at: string;
}

export interface AgentDetail extends AgentRow {
  /**
   * The account's holdings at its last reconciliation (#1499's `account_positions`): account-wide, so
   * an account two agents share shows both agents' holdings. `null`: the document or the account
   * stream was not found.
   */
  account_positions: ReadPosition[] | null;
}

export type ApprovalState = "pending" | "acted" | "skipped" | "timed_out" | "canceled";

export interface BoundOrder {
  instrument: string;
  asset_class: AssetClass;
  side: "buy";
  qty: string;
  limit: string;
  order_usd: string;
  purpose: "open" | "increase";
  tif: "day" | "gtc" | "ioc" | null;
  protection: { stop: string; take_profit: string | null } | null;
}

export interface ApprovalRow {
  approval_id: string;
  agent_id: string;
  state: ApprovalState;
  requested_at: string;
  /** Risk-clock seconds as `ApprovalRequested` journals it; shown as a UTC time. */
  deadline: number;
  content_hash: `sha256:${string}`;
  order: BoundOrder;
}

export interface ApprovalDetail extends ApprovalRow {
  content: Record<string, unknown>;
  /** The `ApprovalRequested` version `content` comes from: version 3's names the checks its dry run deferred (DEC-2518). */
  schema_version: ApprovalVersion;
  /** The deferred checks of the decision the request names, as served, or `null` when the API found none (DEC-2519); judged by `deferredChecks`. */
  decision_deferred_to_binding: unknown[] | null;
}

/** The `ApprovalRequested` versions the journal spec defines (§9.6). */
export type ApprovalVersion = 1 | 2 | 3;

/** What the API's own screens read: every agent with its detail, and every approval with its content. */
export interface ApiView {
  agents: AgentDetail[];
  approvals: ApprovalDetail[];
}

export type LoadedApi = { ok: true; ws: Workspace; view: ApiView; as_of: Watermark[] } | { ok: false; error: ApiError };

const fail = <T,>(path: string, problem: DecodeIssue["problem"], value: string | null = null): Decoded<T> => ({ ok: false, issue: { path, problem, value, allowed: null } });

const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path, "wrong_type"));

function patterned(pattern: RegExp): Decoder<string> {
  return (value, path) => {
    if (typeof value !== "string") return fail(path, "wrong_type");
    return pattern.test(value) ? { ok: true, value } : fail(path, "not_canonical", value);
  };
}

/** A count: a non-negative integer, at least `least`. */
function count(least: number): Decoder<number> {
  return (value, path) => {
    const read = integer(value, path);
    if (!read.ok) return read;
    return read.value >= least ? read : fail(path, "wrong_type");
  };
}

const record: Decoder<Record<string, unknown>> = (value, path) => (isRecord(value) ? { ok: true, value } : fail(path, "wrong_type"));
const instrument = patterned(/^.{1,200}$/s);
const day = patterned(/^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$/);
const label = patterned(/^[a-z0-9][a-z0-9-]{0,62}$/);
const assetClass = closedEnum<AssetClass>(["crypto", "us_equity"]);
/** `common.schema.json`'s `Id` and `EventId`: an opaque id, and a ULID. */
const id = patterned(/^[A-Za-z0-9_-]{1,64}$/);
const eventId = patterned(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/);

/** At least one asset class, each once (`minItems: 1`, `uniqueItems`). */
const assetClasses: Decoder<AssetClass[]> = (value, path) => {
  const read = array(assetClass)(value, path);
  if (!read.ok) return read;
  return read.value.length > 0 && new Set(read.value).size === read.value.length ? read : fail(path, "not_canonical", JSON.stringify(read.value));
};

const mandateSummary = object<MandateSummary>({
  allocation_usd: decimal,
  max_loss_from_allocation: decimal,
  goal: closedEnum(["continuous", "accumulate", "profit_stop"]),
  goal_end_date: nullable(day),
  asset_classes: assetClasses,
  max_position_usd: decimal,
  max_order_usd: decimal,
  max_orders_per_day: count(1),
  max_daily_loss: decimal,
  max_drawdown: decimal,
});

const agentMembers = {
  agent_id: id,
  environment: closedEnum<Environment>(["paper", "live"]),
  mandate_version: contentRef,
  deployed_at: timestamp,
  mode: nullable(closedEnum<AgentMode>(["normal", "exits_only", "paused", "stopped"])),
  pending_approvals: count(0),
  label: nullable(label),
  mandate: nullable(mandateSummary),
};
const agentRow = object<AgentRow>(agentMembers);
export const reconciledHolding = object<ReadPosition>({ instrument, qty: decimal, broker_qty: decimal, mismatch: flag, observed_at: timestamp });
const agentDetail = object<AgentDetail>({ ...agentMembers, account_positions: nullable(array(reconciledHolding)) });

const boundOrder = object<BoundOrder>({
  instrument,
  asset_class: assetClass,
  side: closedEnum(["buy"]),
  qty: decimal,
  limit: decimal,
  order_usd: decimal,
  purpose: closedEnum(["open", "increase"]),
  tif: nullable(closedEnum(["day", "gtc", "ioc"])),
  protection: nullable(object<{ stop: string; take_profit: string | null }>({ stop: decimal, take_profit: nullable(decimal) })),
});
const approvalMembers = {
  approval_id: eventId,
  agent_id: id,
  state: closedEnum<ApprovalState>(["pending", "acted", "skipped", "timed_out", "canceled"]),
  requested_at: timestamp,
  deadline: count(0),
  content_hash: contentRef,
  order: boundOrder,
};
const approvalRow = object<ApprovalRow>(approvalMembers);
const APPROVAL_VERSIONS: readonly ApprovalVersion[] = [1, 2, 3];
const approvalVersion: Decoder<ApprovalVersion> = (value, path) => {
  const read = integer(value, path);
  if (!read.ok) return read;
  return (APPROVAL_VERSIONS as readonly number[]).includes(read.value) ? { ok: true, value: read.value as ApprovalVersion } : fail(path, "unknown_enum_value", String(read.value));
};
/** A list as served, its items unread here, or `null`: what is in it is the screen's to judge, so a bad item never hides the approval. */
const servedList: Decoder<unknown[] | null> = (value, path) => (value === null || Array.isArray(value) ? { ok: true, value } : fail(path, "wrong_type"));
const approvalDetail = object<ApprovalDetail>({ ...approvalMembers, content: record, schema_version: approvalVersion, decision_deferred_to_binding: servedList });

export const agentsRead = object<{ agents: AgentRow[] }>({ agents: array(agentRow) });
export const agentRead = object<{ agent: AgentDetail }>({ agent: agentDetail });
export const approvalsRead = object<{ approvals: ApprovalRow[] }>({ approvals: array(approvalRow) });
export const approvalRead = object<{ approval: ApprovalDetail }>({ approval: approvalDetail });

/** An approval's deadline as a time: its risk-clock seconds read as Unix seconds, UTC. */
export function deadlineIso(deadline: number): string {
  return new Date(deadline * 1000).toISOString();
}

/**
 * The connections read (`read-models/connections`, DEC-2620), only the members the workspace needs:
 * each connection's id, environment and state. Health
 * (`GET /health`, workspace API spec §4.7) is not read until the API routes it: the API answers an
 * unrouted path with its one 401 (API-1), which would read as signed out and hide every agent, so
 * web/ reads routed paths only and the status strip shows health as not reported (backlog E21-63).
 */
const connections = object<{ connections: Array<{ connection_id: string; environment: Environment; state: ConnectionState }> }>({
  connections: array(object({ connection_id: string, environment: closedEnum<Environment>(["paper", "live"]), state: closedEnum(CONNECTION_STATES) })),
});

/** A read the API may not serve yet: its 404 is "not reported" (`null`), any other failure is the load's. */
async function optional<T>(read: Promise<ReadOutcome<T>>): Promise<{ ok: true; value: T | null; as_of: Watermark[] } | { ok: false; error: ApiError }> {
  const outcome = await read;
  if (outcome.ok) return { ok: true, value: outcome.value, as_of: outcome.as_of };
  if (outcome.error.status === 404 && outcome.error.code === "not_found") return { ok: true, value: null, as_of: [] };
  return outcome;
}

function mismatch(path: string): LoadedApi {
  return { ok: false, error: { code: "invalid_response", effect: "none", status: 200, event_id: null, retryable: true, violations: [], decode: { path, problem: "wrong_type", value: null, allowed: null } } };
}

/** The newest watermark per stream the reads reflect (API-14). */
function newest(marks: Watermark[]): Watermark[] {
  const by = new Map<string, Watermark>();
  for (const mark of marks) {
    const seen = by.get(mark.stream_id);
    if (!seen || mark.seq > seen.seq) by.set(mark.stream_id, mark);
  }
  return [...by.values()];
}

/** The agents' one environment when they all name the same; otherwise none, so the badge says "unknown" rather than guess. */
function agreedEnvironment(agents: AgentDetail[]): Environment | null {
  const named = new Set(agents.map((a) => a.environment));
  return named.size === 1 ? [...named][0] : null;
}

export async function loadApi(client: WorkspaceClient): Promise<LoadedApi> {
  const [agents, pending, resolved, held] = await Promise.all([
    client.read("/agents", agentsRead),
    client.read("/approvals?state=pending", approvalsRead),
    client.read("/approvals?state=resolved", approvalsRead),
    optional(client.read("/connections", connections)),
  ]);
  for (const outcome of [agents, pending, resolved, held]) if (!outcome.ok) return { ok: false, error: outcome.error };
  if (!agents.ok || !pending.ok || !resolved.ok || !held.ok) return mismatch("");

  const enc = encodeURIComponent;
  const agentIds = agents.value.agents.map((a) => a.agent_id);
  const approvalIds = [...new Set([...pending.value.approvals, ...resolved.value.approvals].map((a) => a.approval_id))];
  const [details, approvals] = await Promise.all([
    Promise.all(agentIds.map((id) => client.read(`/agents/${enc(id)}`, agentRead))),
    Promise.all(approvalIds.map((id) => client.read(`/approvals/${enc(id)}`, approvalRead))),
  ]);
  const marks = [...agents.as_of, ...pending.as_of, ...resolved.as_of, ...held.as_of];
  const view: ApiView = { agents: [], approvals: [] };
  for (const [i, outcome] of details.entries()) {
    if (!outcome.ok) return { ok: false, error: outcome.error };
    if (outcome.value.agent.agent_id !== agentIds[i]) return mismatch(`/agents/${i}`);
    view.agents.push(outcome.value.agent);
    marks.push(...outcome.as_of);
  }
  for (const [i, outcome] of approvals.entries()) {
    if (!outcome.ok) return { ok: false, error: outcome.error };
    if (outcome.value.approval.approval_id !== approvalIds[i]) return mismatch(`/approvals/${i}`);
    view.approvals.push(outcome.value.approval);
    marks.push(...outcome.as_of);
  }

  const now = new Date().toISOString();
  const unreported = { state: "stale" as const, as_of: null };
  /**
   * The workspace's connection is an established one: a pending, refused or revoked connection is
   * never the scope of Stop-all or Close-all. With none established the workspace names none, and
   * those take the workspace scope instead, which covers every connection (spec §5.4).
   */
  const connection = held.value?.connections.find((c) => c.state === "established") ?? null;
  const base = emptyWorkspace("ready", now);
  const ws: Workspace = {
    ...base,
    environment: connection?.environment ?? agreedEnvironment(view.agents) ?? base.environment,
    connection: { ...base.connection, connection_id: connection?.connection_id ?? "" },
    health: { market_data: unreported, broker: unreported, deployment: unreported, relay: unreported },
  };
  return { ok: true, ws, view, as_of: newest(marks) };
}
