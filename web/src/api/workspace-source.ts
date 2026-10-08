/**
 * Where the web app's workspace comes from (E11-9, E11-1; workspace API spec §4.7, §4.9, §6.1, §7):
 * the recorded fixtures, or the workspace API through the typed client. The source is chosen at build
 * time from public environment values, and a production build never falls back to fixtures on its
 * own: with no API configured it shows "cannot reach your workspace" (DEC-736). Which workspace is
 * read is never a build value: it comes from the signed-in principal's memberships at run time
 * (identity spec §3.2, §9.2), through `resolveWorkspace`.
 */
import type { Environment, Workspace } from "@/fixtures/types";
import type { Timestamp, Watermark } from "./types";

/** The build-time choice. `unconfigured` renders as unreachable, never as fixtures. */
export type DataSource = { kind: "fixtures" } | { kind: "api"; baseUrl: string } | { kind: "unconfigured"; reason: string };

/** The public values the choice reads; all are inlined into the bundle at build time. */
export interface SourceEnv {
  NODE_ENV?: string;
  NEXT_PUBLIC_WORKSPACE_SOURCE?: string;
  NEXT_PUBLIC_WORKSPACE_API_URL?: string;
}

/**
 * The workspaces the signed-in principal is an active member of, or `null` while no route serves
 * them. The identity service owns the answer (lane L1); the web app never takes a workspace id from
 * its build or its URL alone.
 */
export type WorkspaceResolver = () => Promise<{ workspace_ids: string[] } | null>;

/** Which workspace this session reads. `choose`: several memberships and none chosen yet (G1's switcher). */
export type WorkspaceResolution =
  | { kind: "fixtures" }
  | { kind: "workspace"; baseUrl: string; workspaceId: string }
  | { kind: "choose"; baseUrl: string; workspaceIds: string[] }
  | { kind: "unconfigured"; reason: string };

/** Common `Freshness` (spec §6.1, API-14): the server's judgment of one value's age, measured to `served_at`. */
export interface Freshness {
  observed_at: Timestamp;
  stale: boolean;
  age_seconds: number;
  limit_source: string;
}

/** How a value is shown: its time, its age in words, and whether it is stale. Never "current" when stale. */
export interface FreshnessView {
  as_of: Timestamp;
  stale: boolean;
  age: string;
}

/** One `/changes` notice (spec §3.1): watermarks only. */
export interface ChangeMark {
  stream_id: string;
  seq: number;
}

export function dataSourceFrom(env: SourceEnv): DataSource {
  void env;
  throw new Error("Unimplemented: E11-9");
}

/** Until the identity service serves the principal's memberships, there is no workspace to read. */
export const membershipsNotServedYet: WorkspaceResolver = async () => {
  throw new Error("Unimplemented: E11-9");
};

/**
 * The workspace to read: fixtures need none; the API reads one the principal is a member of, the
 * `chosen` one only when it is among them. A resolver that fails or answers nothing usable is
 * `unconfigured`, never fixtures.
 */
export async function resolveWorkspace(source: DataSource, resolver: WorkspaceResolver, chosen: string | null): Promise<WorkspaceResolution> {
  void source;
  void resolver;
  void chosen;
  throw new Error("Unimplemented: E11-9");
}

/**
 * The workspace when the deployment cannot be read (G1, G4, spec §7): no agent, approval, decision,
 * timeline, position, or account value, whatever an earlier load showed. Health says when the
 * deployment last answered, if it ever did.
 */
export function unreachableWorkspace(now: Timestamp, environment: Environment, lastAnswered: Timestamp | null): Workspace {
  void now;
  void environment;
  void lastAnswered;
  throw new Error("Unimplemented: E11-9");
}

export function freshnessView(freshness: Freshness): FreshnessView {
  void freshness;
  throw new Error("Unimplemented: E11-9");
}

/**
 * Whole seconds from the event a stream was read at to `served_at`, or `null` when the response did
 * not read that stream: one response can be current on one stream and behind on another (§6.1).
 */
export function streamAgeSeconds(asOf: readonly Watermark[], streamId: string, servedAt: Timestamp): number | null {
  void asOf;
  void streamId;
  void servedAt;
  throw new Error("Unimplemented: E11-9");
}

/** The label every P&L carries: paper is simulated (brief D1), live carries none. */
export function pnlLabel(environment: Environment): string | null {
  void environment;
  throw new Error("Unimplemented: E11-1");
}

/** Whether a change notice is newer than what a view shows: only then is the view fetched again (§3.1, §6.3). */
export function shouldRefetch(viewAsOf: readonly Watermark[], notice: readonly ChangeMark[]): boolean {
  void viewAsOf;
  void notice;
  throw new Error("Unimplemented: E11-9");
}
