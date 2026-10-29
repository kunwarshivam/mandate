/**
 * Every workspace API operation the web app calls, each with the spec section that documents it
 * (DEC-975, E21-45). The app is one client of the published API and has no route of its own:
 * `contract.test.ts` fails any entry whose method and path the spec does not document, and any request
 * the app makes that `operationFor` matches to no entry. Commands are `client.ts`'s `OPERATIONS`; there
 * is no order ticket (DEC-528).
 */
import { OPERATIONS, SESSION_ROUTE, STEP_UP_CHALLENGE_ROUTE } from "./client";
import { CONNECT_START_ROUTE } from "./connect";
import { ENROL_ROUTE, REGISTRATION_ROUTE } from "./enrol";
import { MY_WORKSPACES_ROUTE } from "./memberships";
import type { Operation } from "./types";

export interface DocumentedOperation {
  /** A read's name, or the command's `Operation`. */
  id: string;
  method: "GET" | "POST";
  /** The route under `/v1/workspaces/{ws}`, with `{name}` for each path id; or, when `outside`, the whole path. */
  route: `/${string}`;
  /** A route outside the workspace prefix (§4.10). */
  outside?: true;
  /** The query members the call may send, all of them documented. */
  query: readonly string[];
  /** Where the workspace API spec documents it. */
  section: string;
}

export type ReadId =
  | "health"
  | "agents"
  | "agent"
  | "agent_orders"
  | "agent_positions"
  | "mandate_versions"
  | "journal_events"
  | "approvals"
  | "approval"
  | "connections"
  | "connection"
  | "backtest"
  | "command_status"
  | "confirmation"
  | "passkeys"
  | "instruments"
  | "signal_models";

const READS: ReadonlyArray<DocumentedOperation & { id: ReadId }> = [
  { id: "health", method: "GET", route: "/health", query: [], section: "§4.7" },
  { id: "agents", method: "GET", route: "/agents", query: [], section: "§4.2" },
  { id: "agent", method: "GET", route: "/agents/{agent}", query: [], section: "§4.7" },
  { id: "agent_orders", method: "GET", route: "/agents/{agent}/orders", query: [], section: "§4.7, DEC-2111" },
  { id: "agent_positions", method: "GET", route: "/agents/{agent}/positions", query: [], section: "§4.7, DEC-2111" },
  { id: "mandate_versions", method: "GET", route: "/agents/{agent}/mandate-versions", query: [], section: "§4.7, DEC-2147" },
  { id: "journal_events", method: "GET", route: "/journal/streams/{stream}/events", query: ["after_seq", "limit"], section: "§4.8.1" },
  { id: "approvals", method: "GET", route: "/approvals", query: ["state"], section: "§4.3" },
  { id: "approval", method: "GET", route: "/approvals/{approval}", query: [], section: "§4.3" },
  { id: "connections", method: "GET", route: "/connections", query: [], section: "§4.5" },
  { id: "connection", method: "GET", route: "/connections/{connection}", query: [], section: "§4.5, DEC-2620" },
  { id: "backtest", method: "GET", route: "/backtests/{backtest}", query: [], section: "§4.2" },
  { id: "command_status", method: "GET", route: "/commands/{event}", query: [], section: "§5.5" },
  { id: "confirmation", method: "GET", route: "/mandate-versions/{version}/confirmation", query: [], section: "§4.1, §3.11 item 11" },
  { id: "passkeys", method: "GET", route: "/passkeys", query: [], section: "§3.6" },
  { id: "instruments", method: "GET", route: "/instruments", query: [], section: "§4.1, DEC-2136" },
  { id: "signal_models", method: "GET", route: "/signal-models", query: [], section: "§4.1, DEC-2136" },
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
  deploy: "§4.2, §3.11 item 11",
};

/**
 * The calls that are neither a read of the workspace nor a command of `OPERATIONS`: the step-up
 * challenge, which records nothing (§3.6, DEC-2103), a connect's start, whose answer is the
 * authorization page rather than a command's 202 (§4.5, DEC-2623), and one's own session (§4.10,
 * DEC-2113).
 */
const OTHERS: readonly DocumentedOperation[] = [
  { id: "step_up_challenge", method: "POST", route: STEP_UP_CHALLENGE_ROUTE, query: [], section: "§3.6" },
  { id: "connect_start", method: "POST", route: CONNECT_START_ROUTE, query: [], section: "§4.5, DEC-2623" },
  { id: "session", method: "GET", route: SESSION_ROUTE, query: [], section: "§4.10", outside: true },
  { id: "my_workspaces", method: "GET", route: MY_WORKSPACES_ROUTE, query: [], section: "§4.10, DEC-2113", outside: true },
  { id: "passkey_registration_challenge", method: "POST", route: REGISTRATION_ROUTE, query: [], section: "§3.6, DEC-2108" },
  { id: "passkey_enrol", method: "POST", route: ENROL_ROUTE, query: [], section: "§3.6, DEC-2108" },
];

/** The ids of `OTHERS`, which are not commands of `client.ts`'s `OPERATIONS`. */
export const NOT_COMMANDS: ReadonlySet<string> = new Set(OTHERS.map((op) => op.id));

export const WEB_OPERATIONS: readonly DocumentedOperation[] = [
  ...READS,
  ...OTHERS,
  ...(Object.keys(OPERATIONS) as Operation[]).map((id) => ({ id, method: "POST" as const, route: OPERATIONS[id], query: [], section: COMMAND_SECTIONS[id] })),
];

/** The documented operation a request is, or `null`: the path must sit under this workspace and match a route whole. */
const MATCHERS = WEB_OPERATIONS.map((op) => ({ op, pattern: new RegExp(`^${op.route.replace(/\{[a-z]+\}/g, "[^/]+")}$`) }));

export function operationFor(method: string, url: string, workspaceId: string): DocumentedOperation | null {
  const parsed = new URL(url, "http://relative.invalid");
  const whole = MATCHERS.find(({ op }) => op.outside && op.method === method.toUpperCase() && op.route === parsed.pathname);
  if (whole) return [...parsed.searchParams.keys()].length === 0 ? whole.op : null;
  const prefix = `/v1/workspaces/${workspaceId}`;
  if (!parsed.pathname.startsWith(`${prefix}/`)) return null;
  const route = parsed.pathname.slice(prefix.length);
  const found = MATCHERS.find(({ op, pattern }) => !op.outside && op.method === method.toUpperCase() && pattern.test(route));
  if (!found) return null;
  for (const name of parsed.searchParams.keys()) if (!found.op.query.includes(name)) return null;
  return found.op;
}
