import { connection } from "next/server";

/**
 * Where the app's screens read from and send to (DEC-975): the recorded fixtures, or one workspace
 * deployment's API (workspace API spec §3). Chosen by server-side configuration alone, read when a
 * request is served and never inlined into the bundle (ES-12): `OWLHEAD_DATA_SOURCE` (`fixtures`, the
 * default, or `api`), `OWLHEAD_API_ORIGIN` and `OWLHEAD_WORKSPACE_ID`. Neither is a secret; the browser
 * reaches the API with its own session cookie (spec §3.3).
 *
 * An `api` choice whose origin or workspace does not parse, or any other value, is `misconfigured`
 * (an unset workspace is not: the session's memberships name it):
 * the app shows the deployment unreachable rather than falling back to fixtures, so fixture figures
 * never pass for a record (rule 3).
 */
export type DataSource =
  | { kind: "fixtures" }
  /** `workspaceId` is `null` when `OWLHEAD_WORKSPACE_ID` is unset: the browser reads it from the session's memberships (`GET /v1/me/workspaces`). */
  | { kind: "api"; origin: string; workspaceId: string | null }
  | { kind: "misconfigured"; reason: string };

/** What the browser is told: the API's origin and the workspace, never anything else. */
export type ApiSource = Extract<DataSource, { kind: "api" }>;

export const DATA_SOURCE_ENV = "OWLHEAD_DATA_SOURCE";
export const API_ORIGIN_ENV = "OWLHEAD_API_ORIGIN";
export const WORKSPACE_ENV = "OWLHEAD_WORKSPACE_ID";

const WORKSPACE_ID = /^[A-Za-z0-9_-]{1,128}$/;
const LOOPBACK = new Set(["127.0.0.1", "localhost", "[::1]"]);

/** An origin alone: https, or http on a loopback host; no credentials, path, query or fragment. */
function originOf(value: string): string | null {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  const secure = url.protocol === "https:" || (url.protocol === "http:" && LOOPBACK.has(url.hostname));
  if (!secure || url.username || url.password || url.search || url.hash || url.pathname !== "/") return null;
  if (value.replace(/\/$/, "") !== url.origin) return null;
  return url.origin;
}

export function readDataSource(env: Readonly<Record<string, string | undefined>>): DataSource {
  const kind = env[DATA_SOURCE_ENV] ?? "";
  if (kind === "" || kind === "fixtures") return { kind: "fixtures" };
  if (kind !== "api") return { kind: "misconfigured", reason: `${DATA_SOURCE_ENV} is neither fixtures nor api` };
  const origin = originOf(env[API_ORIGIN_ENV] ?? "");
  if (!origin) return { kind: "misconfigured", reason: `${API_ORIGIN_ENV} is not an https origin` };
  const workspaceId = env[WORKSPACE_ENV];
  if (workspaceId === undefined) return { kind: "api", origin, workspaceId: null };
  if (!WORKSPACE_ID.test(workspaceId)) return { kind: "misconfigured", reason: `${WORKSPACE_ENV} is not a workspace id` };
  return { kind: "api", origin, workspaceId };
}

/** The source for this request. `connection()` keeps the read at request time, so a build never fixes it. */
export async function getDataSource(): Promise<DataSource> {
  await connection();
  return readDataSource(process.env);
}
