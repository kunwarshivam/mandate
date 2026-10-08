/**
 * Where the web app's workspace comes from (E11-9, E11-1; workspace API spec §4.7, §4.9, §6.1, §7):
 * the recorded fixtures, or the workspace API through the typed client. The source is chosen at build
 * time from public environment values, and a production build never falls back to fixtures on its
 * own: with no API configured it shows "cannot reach your workspace" (DEC-736). Which workspace is
 * read is never a build value: it comes from the signed-in principal's memberships at run time
 * (identity spec §3.2, §9.2), through `resolveWorkspace`.
 */
import type { Environment, Workspace } from "@/fixtures/types";
import type { Fetch } from "./client";
import type { Timestamp, Watermark } from "./types";

/** The build-time choice. `unconfigured` renders as unreachable, never as fixtures. */
export type DataSource = { kind: "fixtures" } | { kind: "api"; baseUrl: string } | { kind: "unconfigured"; reason: string };

/** The public values the choice reads; all are inlined into the bundle at build time. */
export interface SourceEnv {
  NODE_ENV?: string;
  NEXT_PUBLIC_WORKSPACE_SOURCE?: string;
  NEXT_PUBLIC_WORKSPACE_API_URL?: string;
}

/** One active membership, as `GET /v1/me/workspaces` lists it (identity spec §4.5, #811). */
export interface Membership {
  workspace_id: string;
  label: string;
  /** The effective roles: those already past their cool-off (identity spec §8.3); possibly none. */
  roles: string[];
}

/**
 * `GET /v1/me/session` (identity spec §4.5): a full session (§6.2) learns its workspaces from
 * `/v1/me/workspaces`; a reduction-only session (§6.4 route 2) covers exactly the workspaces it
 * lists, and never reads the membership index.
 */
export type SessionInfo =
  | { kind: "full"; expires_at: Timestamp }
  | { kind: "reduction_only"; workspaces: Array<{ workspace_id: string; label: string }>; expires_at: Timestamp };

/** An identity read's outcome. `retryable` failures (`membership_unavailable`, 503) are unreachable, never fixtures. */
export type IdentityAnswer<T> = { ok: true; value: T } | { ok: false; code: string; retryable: boolean };

/** The two identity routes the web app reads before it can name a workspace. `null`: no route serves it yet. */
export interface IdentityApi {
  session(): Promise<IdentityAnswer<SessionInfo> | null>;
  memberships(): Promise<IdentityAnswer<Membership[]> | null>;
}

/**
 * Which workspace this session reads. `choose`: several workspaces, offered by label (G1's
 * switcher), with the last-used one as the default only while it is still among them.
 */
export type WorkspaceResolution =
  | { kind: "fixtures" }
  | { kind: "workspace"; baseUrl: string; workspaceId: string; label: string }
  | { kind: "choose"; baseUrl: string; workspaces: Array<{ workspace_id: string; label: string }>; preferred: string | null }
  | { kind: "unreachable"; reason: string; retryable: boolean }
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

/** Until the identity routes are served, there is no session or membership to read. */
export const identityNotServedYet: IdentityApi = {
  session: async () => {
    throw new Error("Unimplemented: E11-9");
  },
  memberships: async () => {
    throw new Error("Unimplemented: E11-9");
  },
};

/** The identity routes over HTTP, at the API's origin, with the session cookie (identity spec §4.5). */
export function createIdentityApi(options: { fetch: Fetch; baseUrl: string }): IdentityApi {
  void options;
  throw new Error("Unimplemented: E11-9");
}

/**
 * The workspace to read. Fixtures need none and ask nothing. Otherwise `/v1/me/session` comes first:
 * a reduction-only session's own list is the answer and `/v1/me/workspaces` is never read; a full
 * session reads its memberships. `lastUsed` is an opaque id remembered on this device, offered as the
 * default only while it is listed. Any failure is unreachable or unconfigured, never fixtures.
 */
export async function resolveWorkspace(source: DataSource, identity: IdentityApi, lastUsed: string | null): Promise<WorkspaceResolution> {
  void source;
  void identity;
  void lastUsed;
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
