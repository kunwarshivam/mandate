/**
 * The dashboard on the workspace API (E11-1, E11-9; workspace API spec §4.7, §4.9, §4.10, §6.1, §7):
 * `GET /dashboard` and `GET /health`, decoded strictly and mapped onto the screens' existing view
 * types, so no screen contract changes. A read that fails leaves nothing from an earlier load
 * (G1, G4): the state becomes unreachable, with no agent data at all.
 */
import type { ActiveRestriction, Agent, AgentMode, Environment, Health, Workspace } from "@/fixtures/types";
import type { WorkspaceClient } from "./client";
import type { Decoder, Timestamp, Watermark } from "./types";
import type { ChangeMark, Freshness, FreshnessView } from "./workspace-source";

/** `read-models/health` `Component`: an `ok` component is never stale. */
export interface HealthComponent {
  state: "ok" | "stale" | "down";
  freshness: Freshness;
}

export interface HealthReadModel {
  served_at: Timestamp;
  market_data: HealthComponent;
  broker: HealthComponent;
  deployment: HealthComponent;
  relay: HealthComponent;
}

/** `read-models/agents` `AgentSummary`. */
export interface AgentSummary {
  agent_id: string;
  label: string;
  environment: Environment;
  simulated: boolean;
  mode: AgentMode;
  restrictions: Array<{ code: ActiveRestriction["code"]; since: Timestamp; symbol: string | null }>;
  startup: "reconciling" | "ready";
  positions_count: number;
  pnl_total: string;
  pnl_today: string;
  marks_freshness: Freshness | null;
}

export interface DashboardReadModel {
  served_at: Timestamp;
  agents: AgentSummary[];
  open_approvals: number;
  unread_alerts: number;
  recent_decisions: string[];
}

/** One agent as the dashboard's cards read it: the existing `Agent` members a summary carries, plus its labels. */
export type AgentSummaryView = Pick<Agent, "agent_id" | "label" | "mode" | "restrictions" | "startup" | "pnl_total" | "pnl_today"> & {
  environment: Environment;
  pnl_label: string | null;
  positions_count: number;
  marks: FreshnessView | null;
};

export type DashboardState =
  | {
      status: "ready";
      served_at: Timestamp;
      agents: AgentSummaryView[];
      open_approvals: number;
      unread_alerts: number;
      recent_decisions: string[];
      health: Health;
      /** What each view read, kept per view so a change notice refetches only the view it is newer than. */
      as_of: { dashboard: Watermark[]; health: Watermark[] };
    }
  | { status: "unreachable"; workspace: Workspace };

export interface DashboardSource {
  /** Reads `/dashboard` and `/health`; either failing makes the whole state unreachable. */
  load(): Promise<DashboardState>;
  /** A `/changes` notice: refetches each view it is newer than, and returns whether anything was fetched. */
  notice(marks: readonly ChangeMark[]): Promise<boolean>;
  state(): DashboardState | null;
}

export interface DashboardSourceOptions {
  /** The viewer's clock, used only for the unreachable workspace's `now`. */
  now: () => Timestamp;
  environment: Environment;
}

export const decodeHealth: Decoder<HealthReadModel> = (value, path) => {
  void value;
  void path;
  throw new Error("Unimplemented: E11-9");
};

export const decodeDashboard: Decoder<DashboardReadModel> = (value, path) => {
  void value;
  void path;
  throw new Error("Unimplemented: E11-9");
};

/** G1's strip: each component with its last observation time (`freshness.observed_at`). */
export function healthFromReadModel(health: HealthReadModel): Health {
  void health;
  throw new Error("Unimplemented: E11-9");
}

export function agentSummaryView(summary: AgentSummary): AgentSummaryView {
  void summary;
  throw new Error("Unimplemented: E11-1");
}

export function createDashboardSource(client: WorkspaceClient, options: DashboardSourceOptions): DashboardSource {
  void client;
  void options;
  throw new Error("Unimplemented: E11-9");
}
