/**
 * Which workspace the app reads when its configuration names none (DEC-975, workspace API spec §4.10,
 * DEC-2113): the session's own memberships, `GET /v1/me/workspaces`. Exactly one is that workspace.
 * None, or several, names none: the app says so and reads nothing, and never guesses among them.
 */
import { array, closedEnum, isRecord, nullable, object, string } from "./decode";
import type { Decoded, Decoder } from "./types";
import type { Fetch } from "./client";

/** One's own memberships, outside every workspace path (§4.10). */
export const MY_WORKSPACES_ROUTE = "/v1/me/workspaces";

const ROLES = ["workspace_admin", "operator", "approver", "viewer", "auditor"] as const;

export interface Membership {
  workspace_id: string;
  label: string | null;
  roles: Array<(typeof ROLES)[number]>;
}

const ID = /^[A-Za-z0-9_-]{1,128}$/;
const workspaceId: Decoder<string> = (value, path) => {
  const read = string(value, path);
  return read.ok && !ID.test(read.value) ? { ok: false, issue: { path, problem: "not_canonical", value: null, allowed: null } } : read;
};

/** `read-models/my-workspaces`; members beyond these are ignored (§3.2). */
export const myWorkspaces: Decoder<{ workspaces: Membership[] }> = object({
  workspaces: array(object<Membership>({ workspace_id: workspaceId, label: nullable(string), roles: array(closedEnum(ROLES)) })),
});

export type Resolved = { ok: true; workspaceId: string } | { ok: false; reason: "none" | "several" | "signed_out" | "unavailable" };

/** The one workspace the memberships name, or why there is none. */
export function pickWorkspace(read: Decoded<{ workspaces: Membership[] }>): Resolved {
  if (!read.ok) return { ok: false, reason: "unavailable" };
  const ids = [...new Set(read.value.workspaces.map((m) => m.workspace_id))];
  if (ids.length === 0) return { ok: false, reason: "none" };
  if (ids.length > 1) return { ok: false, reason: "several" };
  return { ok: true, workspaceId: ids[0] };
}

/** Reads one's memberships from `origin` with the browser's own session cookie, and picks the workspace. */
export async function resolveWorkspace(fetch: Fetch, origin: string): Promise<Resolved> {
  let response: Response;
  try {
    response = await fetch(origin + MY_WORKSPACES_ROUTE, { method: "GET", credentials: origin === "" ? "same-origin" : "include", headers: { Accept: "application/json" } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (response.status === 401) return { ok: false, reason: "signed_out" };
  if (!response.ok) return { ok: false, reason: "unavailable" };
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  return pickWorkspace(isRecord(body) ? myWorkspaces(body, "") : { ok: false, issue: { path: "", problem: "wrong_type", value: null, allowed: null } });
}
