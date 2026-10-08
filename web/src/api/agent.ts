/**
 * One agent on the workspace API (E11-9; workspace API spec §4.1, §4.2, §4.7, §4.9, §4.10, §6.1, §7):
 * `GET /agents/{id}`, `.../positions`, `.../orders`, `.../pnl`, and `GET /mandate-versions/{hash}`,
 * decoded strictly and assembled into the screens' existing `Agent`, so no screen contract changes.
 * What `Agent` has no member for (each value's freshness, an unprotected interval's start, the
 * ledger and broker quantities disagreeing) travels beside it in `AgentDetails`. Reads that disagree
 * with each other are refused, never merged; a failed read leaves nothing from an earlier load.
 */
import type { Agent, Environment, FieldProvenance, Mandate, PastOrder, Position, WorkingOrder, Fill, Workspace } from "@/fixtures/types";
import type { WorkspaceClient } from "./client";
import type { Decoder, Timestamp, Watermark } from "./types";
import type { ChangeMark, Freshness, FreshnessView } from "./workspace-source";

/** `read-models/agent`: the agent and its limit state, without positions, orders, or P&L. */
export interface AgentReadModel {
  served_at: Timestamp;
  agent_id: string;
  label: string;
  environment: Environment;
  simulated: boolean;
  mandate_version: string;
  mode: Agent["mode"];
  restrictions: Array<{ code: Agent["restrictions"][number]["code"]; since: Timestamp; symbol: string | null }>;
  startup: Agent["startup"];
  deployed_at: Timestamp;
  state: Agent["state"] & { freshness: Freshness };
  goal_progress: Agent["goal_progress"];
  versions: Array<
    Omit<Agent["versions"][number], "application"> & {
      application: { kind: "applied"; at: Timestamp; approvals_canceled: number } | { kind: "rejected"; at: Timestamp; reason: string } | { kind: "pending" };
    }
  >;
}

export interface PositionReadModel extends Omit<Position, "mark_as_of" | "protection"> {
  broker_freshness: Freshness;
  mark_freshness: Freshness;
  protection: Position["protection"] & { unprotected_since: Timestamp | null };
}

export interface PositionsReadModel {
  served_at: Timestamp;
  agent_id: string;
  environment: Environment;
  simulated: boolean;
  positions: PositionReadModel[];
}

export interface OrdersReadModel {
  served_at: Timestamp;
  agent_id: string;
  environment: Environment;
  simulated: boolean;
  orders: WorkingOrder[];
  past_orders: PastOrder[];
  fills: Fill[];
}

export interface PnlReadModel {
  served_at: Timestamp;
  agent_id: string;
  environment: Environment;
  simulated: boolean;
  pnl_total: string;
  pnl_today: string;
  realized_pnl: string;
  unrealized_pnl: string;
  marks_freshness: Freshness | null;
  disclosures: Array<{ document: string; version: string }>;
}

/**
 * `GET /mandate-versions/{hash}` (spec §4.1): the canonical document and its provenance. Its read-model
 * schema is not written yet; this is the shape DEC-737 proposes.
 */
export interface MandateVersionReadModel {
  served_at: Timestamp;
  mandate_version: string;
  mandate: Mandate;
  provenance: FieldProvenance[];
}

export interface AgentReads {
  agent: AgentReadModel;
  positions: PositionsReadModel;
  orders: OrdersReadModel;
  pnl: PnlReadModel;
  version: MandateVersionReadModel;
}

/** One position's facts the `Position` view type has no member for. */
export interface PositionDetails {
  asset_id: string;
  mark: FreshnessView;
  broker: FreshnessView;
  /** The ledger and the broker disagree on the quantity: both are shown, neither is picked. */
  quantity_mismatch: boolean;
  /** Seconds from the start of the current unprotected interval to `served_at`; `null` while protected. */
  unprotected_seconds: number | null;
}

export interface AgentDetails {
  served_at: Timestamp;
  pnl_label: string | null;
  limits: FreshnessView;
  marks: FreshnessView | null;
  positions: PositionDetails[];
  /** Working orders whose outcome is not known (trading spec §5.7): shown as unknown, never guessed. */
  unknown_orders: string[];
}

export type Assembled = { ok: true; agent: Agent; details: AgentDetails } | { ok: false; reason: "agent_mismatch" | "environment_mismatch" | "version_mismatch" };

export type AgentState =
  | { status: "ready"; agent: Agent; details: AgentDetails; as_of: Record<keyof AgentReads, Watermark[]> }
  | { status: "unreachable"; workspace: Workspace };

export interface AgentSource {
  load(): Promise<AgentState>;
  /** Refetches each view a notice is newer than; a mandate version is content-addressed and never refetched. */
  notice(marks: readonly ChangeMark[]): Promise<boolean>;
  state(): AgentState | null;
}

export interface AgentSourceOptions {
  now: () => Timestamp;
  environment: Environment;
}

function unimplemented(): never {
  throw new Error("Unimplemented: E11-9");
}

export const decodeAgent: Decoder<AgentReadModel> = () => unimplemented();
export const decodePositions: Decoder<PositionsReadModel> = () => unimplemented();
export const decodeOrders: Decoder<OrdersReadModel> = () => unimplemented();
export const decodePnl: Decoder<PnlReadModel> = () => unimplemented();
export const decodeMandateVersion: Decoder<MandateVersionReadModel> = () => unimplemented();

/** Builds the existing `Agent` from the five reads, refusing reads that name different agents, environments, or versions. */
export function assembleAgent(reads: AgentReads): Assembled {
  void reads;
  return unimplemented();
}

export function createAgentSource(client: WorkspaceClient, agentId: string, options: AgentSourceOptions): AgentSource {
  void client;
  void agentId;
  void options;
  return unimplemented();
}
