/**
 * The workspace API's read models, decoded as they are served (workspace API spec §4.3 and §4.7;
 * `schemas/workspace-api/read-models/`, lane A1's #1446 and #1499; DEC-1096). A member the API does not
 * serve, or serves as `null`, is `null` here and "not reported" on screen: nothing is filled in from
 * the fixtures or computed in its place (rule 3). The reads the schemas do not define yet (health,
 * connections) are optional: a 404 is "not reported", never the deployment down.
 */
import type { WorkspaceClient } from "./client";
import type { ApiError, Decoder, Watermark } from "./types";
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
  /** `null`: the document or the account stream was not found. */
  positions: ReadPosition[] | null;
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
}

/** What the API's own screens read: every agent with its detail, and every approval with its content. */
export interface ApiView {
  agents: AgentDetail[];
  approvals: ApprovalDetail[];
}

export type LoadedApi = { ok: true; ws: Workspace; view: ApiView; as_of: Watermark[] } | { ok: false; error: ApiError };

export const agentsRead: Decoder<{ agents: AgentRow[] }> = () => {
  throw new Error("Unimplemented: E11-9");
};
export const agentRead: Decoder<{ agent: AgentDetail }> = () => {
  throw new Error("Unimplemented: E11-9");
};
export const approvalsRead: Decoder<{ approvals: ApprovalRow[] }> = () => {
  throw new Error("Unimplemented: E11-9");
};
export const approvalRead: Decoder<{ approval: ApprovalDetail }> = () => {
  throw new Error("Unimplemented: E11-9");
};

/** An approval's deadline as a time: its risk-clock seconds read as Unix seconds, UTC. */
export function deadlineIso(deadline: number): string {
  void deadline;
  throw new Error("Unimplemented: E11-9");
}

export async function loadApi(client: WorkspaceClient): Promise<LoadedApi> {
  void client;
  throw new Error("Unimplemented: E11-9");
}
