/**
 * Every workspace API operation the web app calls, each with the spec section that documents it
 * (DEC-975, E21-45). The app is one client of the published API and has no route of its own:
 * `contract.test.ts` fails any entry whose method and path the spec does not document, and any request
 * the app makes that `operationFor` matches to no entry. Commands are `client.ts`'s `OPERATIONS`; there
 * is no order ticket (DEC-528).
 */
import { OPERATIONS } from "./client";
import type { Operation } from "./types";

export interface DocumentedOperation {
  /** A read's name, or the command's `Operation`. */
  id: string;
  method: "GET" | "POST";
  /** The route under `/v1/workspaces/{ws}`, with `{name}` for each path id. */
  route: `/${string}`;
  /** The query members the call may send, all of them documented. */
  query: readonly string[];
  /** Where the workspace API spec documents it. */
  section: string;
}

export type ReadId =
  | "health"
  | "agents"
  | "agent"
  | "approvals"
  | "approval"
  | "connections"
  | "backtest"
  | "command_status"
  | "confirmation";

const READS: ReadonlyArray<DocumentedOperation & { id: ReadId }> = [
  { id: "health", method: "GET", route: "/health", query: [], section: "§4.7" },
  { id: "agents", method: "GET", route: "/agents", query: [], section: "§4.2" },
  { id: "agent", method: "GET", route: "/agents/{agent}", query: [], section: "§4.7" },
  { id: "approvals", method: "GET", route: "/approvals", query: ["state"], section: "§4.3" },
  { id: "approval", method: "GET", route: "/approvals/{approval}", query: [], section: "§4.3" },
  { id: "connections", method: "GET", route: "/connections", query: [], section: "§4.5" },
  { id: "backtest", method: "GET", route: "/backtests/{backtest}", query: [], section: "§4.2" },
  { id: "command_status", method: "GET", route: "/commands/{event}", query: [], section: "§5.5" },
  { id: "confirmation", method: "GET", route: "/mandate-versions/{version}/confirmation", query: [], section: "§4.1, §3.11 item 11" },
];

const COMMAND_SECTIONS: Readonly<Record<Operation, string>> = {
  pause: "§4.2, §5.4",
  resume: "§4.2, §5.4",
  hold_openings: "§4.2",
  stop: "§4.2, §5.4",
  owner_exit: "§4.2, §5.4",
  acknowledge: "§4.2",
  kill_switch: "§4.2, §5.4",
  approval_response: "§4.3, §5.2",
  end_delegation: "§4.4, §5.3",
  confirm_version: "§4.1, §5.1",
  create_version: "§3.11 item 11",
};

export const WEB_OPERATIONS: readonly DocumentedOperation[] = [
  ...READS,
  ...(Object.keys(OPERATIONS) as Operation[]).map((id) => ({ id, method: "POST" as const, route: OPERATIONS[id], query: [], section: COMMAND_SECTIONS[id] })),
];

/** The documented operation a request is, or `null`: the path must sit under this workspace and match a route whole. */
const MATCHERS = WEB_OPERATIONS.map((op) => ({ op, pattern: new RegExp(`^${op.route.replace(/\{[a-z]+\}/g, "[^/]+")}$`) }));

export function operationFor(method: string, url: string, workspaceId: string): DocumentedOperation | null {
  const parsed = new URL(url, "http://relative.invalid");
  const prefix = `/v1/workspaces/${workspaceId}`;
  if (!parsed.pathname.startsWith(`${prefix}/`)) return null;
  const route = parsed.pathname.slice(prefix.length);
  const found = MATCHERS.find(({ op, pattern }) => op.method === method.toUpperCase() && pattern.test(route));
  if (!found) return null;
  for (const name of parsed.searchParams.keys()) if (!found.op.query.includes(name)) return null;
  return found.op;
}
