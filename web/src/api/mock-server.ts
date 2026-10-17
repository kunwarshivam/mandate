/**
 * A fixture-backed stand-in for the workspace API: a `fetch` implementation for tests, for `npm run dev`
 * and for the e2e build (`src/app/v1/`). It answers from `src/fixtures/` in the shapes of the workspace
 * API spec, and its reads in DEC-975's provisional bodies (`reads.ts`), keeps
 * commands in memory by idempotency key and route (API-4), refuses a command without
 * `X-Mandate-Request: 1` as 403 `forbidden` before anything else (spec §3.3), plays `unreachable` as a network that
 * does not answer, and `result-unknown` as commands recorded whose answer is lost, so a later status
 * poll finds them (spec §5.5). A status is `recorded` when first read and `applied` after; a recorded
 * pause pauses its agent and a recorded Skip skips its request, so a screen can follow a command to its
 * effect. It refuses an approve without step-up as `step_up_required`, as spec §5.2 does.
 */
import { CSRF_HEADER, OPERATIONS, isIdempotencyKey, type Fetch } from "./client";
import type { Watermark } from "./types";
import { buildWorkspace } from "@/fixtures/workspace";
import type { Agent, Approval, Health, Scenario } from "@/fixtures/types";
import { sha256Hex } from "@/lib/sha256";

export interface MockServerOptions {
  workspaceId: string;
  scenario?: Scenario;
}

export interface RecordedCommand {
  event_id: string;
  path: string;
  idempotency_key: string;
  body: string;
}

export interface MockServer {
  fetch: Fetch;
  /** Commands the mock has recorded, in order. */
  recorded(): RecordedCommand[];
}

const API_VERSION = "v1";
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** A fixture time (an ISO string with an offset) as a journal spec §4.7 timestamp. */
function canonical(iso: string): string {
  return `${new Date(iso).toISOString().slice(0, 23)}000000Z`;
}

/** A ULID-shaped id from the first 128 bits of a SHA-256, as spec §3.4 derives event ids. */
function eventId(seed: string): string {
  let bits = BigInt(`0x${sha256Hex(seed).slice(0, 32)}`);
  let out = "";
  for (let i = 0; i < 26; i += 1) {
    out = CROCKFORD[Number(bits & 31n)] + out;
    bits >>= 5n;
  }
  return out;
}

function respond(status: number, body: unknown, type = "application/json"): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": type } });
}

function problem(status: number, code: string, effect: "none" | "recorded", retryable = false): Response {
  return respond(status, { type: "about:blank", status, title: "Request refused", code, effect, event_id: null, retryable }, "application/problem+json");
}

/** `read-models/health.schema.json`: each component's state and freshness, aged against `servedAt`. An `ok` component is never stale. */
function healthBody(health: Health, servedAt: string) {
  const component = (name: keyof Health) => {
    const { state, as_of } = health[name];
    const observed = canonical(as_of);
    const age = Math.max(0, Math.floor((Date.parse(servedAt) - Date.parse(observed)) / 1000));
    return { state, freshness: { observed_at: observed, stale: state !== "ok", age_seconds: age, limit_source: `health.${name}` } };
  };
  return { market_data: component("market_data"), broker: component("broker"), deployment: component("deployment"), relay: component("relay") };
}

const ROUTES = Object.entries(OPERATIONS).map(([operation, route]) => ({
  operation,
  pattern: new RegExp(`^${route.replace(/\{[a-z]+\}/g, "[^/]+")}$`),
}));

export function createMockServer(options: MockServerOptions): MockServer {
  const { workspaceId, scenario = "normal" } = options;
  const workspace = structuredClone(buildWorkspace(scenario));
  const prefix = `/v1/workspaces/${workspaceId}`;
  const commands: RecordedCommand[] = [];

  function asOf(): Watermark[] {
    const seq = commands.length;
    return [{ stream_id: "control", seq, hash: `sha256:${sha256Hex(`${workspaceId}:control:${seq}`)}`, recorded_at: canonical(workspace.now) }];
  }

  const servedAt = canonical(workspace.now);
  const read = (body: object) => respond(200, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, served_at: servedAt, as_of: asOf(), ...body });

  /** An agent's read without what its positions and orders routes serve (DEC-975). */
  function agentCore(agent: Agent) {
    const served: Array<keyof Agent> = ["positions", "orders", "past_orders", "fills"];
    return Object.fromEntries(Object.entries(agent).filter(([key]) => !served.includes(key as keyof Agent)));
  }

  const approvalHash = (approval: Approval) => `sha256:${sha256Hex(`approval:${approval.approval_id}:${approval.requested_at}:${approval.bound.mandate_version}`)}`;
  const agentOf = (id: string) => workspace.agents.find((a) => a.agent_id === id);
  const statusReads = new Map<string, number>();

  function get(route: string, query: URLSearchParams): Response {
    if ([...query.keys()].some((name) => !(route === "/approvals" && name === "state"))) return problem(404, "not_found", "none");
    if (route === "/health") return read(healthBody(workspace.health, servedAt));
    if (route === "/dashboard") return read({ decisions: workspace.decisions, external_positions: workspace.external_positions });
    if (route === "/agents") return read({ agents: workspace.agents.map((a) => ({ agent_id: a.agent_id })) });
    if (route === "/connections") return read({ connections: [{ ...workspace.connection, environment: workspace.environment }] });
    if (route === "/approvals") {
      const state = query.get("state");
      if (state !== "pending" && state !== "resolved") return problem(422, "invalid", "none");
      const listed = workspace.approvals.filter((a) => (a.status === "delivered") === (state === "pending"));
      return read({ approvals: listed.map((a) => ({ approval_id: a.approval_id })) });
    }
    const agentRoute = route.match(/^\/agents\/([^/]+)(\/positions|\/orders|\/timeline)?$/);
    if (agentRoute) {
      const agent = agentOf(decodeURIComponent(agentRoute[1]));
      if (!agent) return problem(404, "not_found", "none");
      switch (agentRoute[2]) {
        case "/positions":
          return read({ positions: agent.positions });
        case "/orders":
          return read({ orders: agent.orders, past_orders: agent.past_orders, fills: agent.fills });
        case "/timeline":
          return read({ events: workspace.timeline[agent.agent_id] ?? [] });
        default:
          return read({ agent: agentCore(agent) });
      }
    }
    const approvalRoute = route.match(/^\/approvals\/([^/]+)$/);
    if (approvalRoute) {
      const approval = workspace.approvals.find((a) => a.approval_id === decodeURIComponent(approvalRoute[1]));
      return approval ? read({ approval: { ...approval, content_hash: approvalHash(approval) } }) : problem(404, "not_found", "none");
    }
    const status = route.match(/^\/commands\/([0-9A-Z]{26})$/);
    const command = status ? commands.find((c) => c.event_id === status[1]) : undefined;
    if (command) {
      const reads = (statusReads.get(command.event_id) ?? 0) + 1;
      statusReads.set(command.event_id, reads);
      if (reads === 1) return read({ phase: "recorded", steps: [] });
      const step = { stream_id: "control", seq: commands.indexOf(command) + 1, event_type: "OwnerCommandApplied", recorded_at: servedAt, reason: null };
      return read({ phase: "applied", steps: [step] });
    }
    return problem(404, "not_found", "none");
  }

  /** What the deployment would show once it acted: a pause pauses its agent, a Skip skips its request. */
  function apply(operation: string, path: string, body: Record<string, unknown>) {
    const pause = path.match(/\/agents\/([^/]+)\/pause$/);
    const agent = pause ? agentOf(decodeURIComponent(pause[1])) : undefined;
    if (operation === "pause" && agent && agent.mode !== "stopped") {
      if (!agent.restrictions.some((r) => r.code === "owner_pause")) agent.restrictions.push({ code: "owner_pause", since: workspace.now });
      if (agent.mode === "normal" || agent.mode === "exits_only") agent.mode = "paused";
    }
    const answer = path.match(/\/approvals\/([^/]+)\/responses$/);
    const approval = answer ? workspace.approvals.find((a) => a.approval_id === decodeURIComponent(answer[1])) : undefined;
    if (operation === "approval_response" && approval && approval.status === "delivered" && body.verdict === "skipped") {
      approval.status = "rejected";
      approval.resolution = { at: workspace.now, text: "Skipped by you. Nothing was sent." };
    }
  }

  function post(route: string, headers: Headers, body: string): Response {
    if (headers.get(CSRF_HEADER) !== "1") return problem(403, "forbidden", "none");
    const match = ROUTES.find((r) => r.pattern.test(route));
    if (!match) return problem(404, "not_found", "none");
    const key = headers.get("Idempotency-Key") ?? "";
    if (!isIdempotencyKey(key)) return problem(422, "invalid", "none");
    let parsed: unknown;
    try {
      parsed = JSON.parse(body);
    } catch {
      return problem(422, "invalid", "none");
    }
    const members = typeof parsed === "object" && parsed !== null && !Array.isArray(parsed) ? (parsed as Record<string, unknown>) : {};
    if (match.operation === "approval_response" && members.verdict === "approved" && members.step_up == null) return problem(401, "step_up_required", "none");
    const path = prefix + route;
    let command = commands.find((c) => c.idempotency_key === key && c.path === path);
    if (command && command.body !== body) return problem(409, "idempotency_conflict", "none");
    if (!command) {
      command = { event_id: eventId(`${workspaceId}\n${match.operation}\n${key}`), path, idempotency_key: key, body };
      commands.push(command);
      apply(match.operation, path, members);
    }
    if (match.operation === "approval_response") return respond(202, { response_id: command.event_id, phase: "recorded" });
    return respond(202, { command_id: command.event_id, phase: "recorded", step_up_status: "missing" });
  }

  const fetch: Fetch = async (input, init) => {
    const method = (init.method ?? "GET").toUpperCase();
    if (scenario === "unreachable") throw new TypeError("Failed to fetch");
    const url = new URL(input, "http://mock.invalid");
    if (!url.pathname.startsWith(`${prefix}/`)) return problem(404, "not_found", "none");
    const route = url.pathname.slice(prefix.length);
    if (method === "GET") return get(route, url.searchParams);
    if (method === "POST") {
      const answer = post(route, new Headers(init.headers), typeof init.body === "string" ? init.body : "");
      if (scenario === "result-unknown") throw new TypeError("The deployment recorded the request and the answer was lost");
      return answer;
    }
    return problem(404, "not_found", "none");
  };

  return { fetch, recorded: () => commands.map((c) => ({ ...c })) };
}
