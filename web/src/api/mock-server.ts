/**
 * A fixture-backed stand-in for the workspace API: a `fetch` implementation for tests and for
 * `npm run dev`. It answers from `src/fixtures/` in the shapes of the workspace API spec, keeps
 * commands in memory by idempotency key (API-4), and plays the `unreachable` and `result-unknown`
 * scenarios as a network that does not answer.
 */
import { OPERATIONS, isIdempotencyKey, type Fetch } from "./client";
import type { Watermark } from "./types";
import { buildWorkspace } from "@/fixtures/workspace";
import type { Health, Scenario } from "@/fixtures/types";
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

const API_VERSION = "1";
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

function healthBody(health: Health) {
  const feed = (f: Health["market_data"]) => ({ state: f.state, as_of: canonical(f.as_of) });
  return { market_data: feed(health.market_data), broker: feed(health.broker), deployment: feed(health.deployment), relay: feed(health.relay) };
}

const ROUTES = Object.entries(OPERATIONS).map(([operation, route]) => ({
  operation,
  pattern: new RegExp(`^${route.replace(/\{[a-z]+\}/g, "[^/]+")}$`),
}));

export function createMockServer(options: MockServerOptions): MockServer {
  const { workspaceId, scenario = "normal" } = options;
  const workspace = buildWorkspace(scenario);
  const prefix = `/v1/workspaces/${workspaceId}`;
  const commands: RecordedCommand[] = [];

  function asOf(): Watermark[] {
    const seq = commands.length;
    return [{ stream_id: "control", seq, hash: `sha256:${sha256Hex(`${workspaceId}:control:${seq}`)}`, recorded_at: canonical(workspace.now) }];
  }

  const read = (body: object) => respond(200, { api_version: API_VERSION, build: `sha256:${sha256Hex("mock-server")}`, as_of: asOf(), ...body });

  function get(route: string): Response {
    if (route === "/health") return read(healthBody(workspace.health));
    const status = route.match(/^\/commands\/([0-9A-Z]{26})$/);
    if (status && commands.some((c) => c.event_id === status[1])) return read({ phase: "recorded", steps: [] });
    return problem(404, "not_found", "none");
  }

  function post(route: string, headers: Headers, body: string): Response {
    const match = ROUTES.find((r) => r.pattern.test(route));
    if (!match) return problem(404, "not_found", "none");
    const key = headers.get("Idempotency-Key") ?? "";
    if (!isIdempotencyKey(key)) return problem(422, "invalid", "none");
    try {
      JSON.parse(body);
    } catch {
      return problem(422, "invalid", "none");
    }
    const path = prefix + route;
    let command = commands.find((c) => c.idempotency_key === key && c.path === path);
    if (command && command.body !== body) return problem(409, "idempotency_conflict", "none");
    if (!command) {
      command = { event_id: eventId(`${workspaceId}\n${match.operation}\n${key}`), path, idempotency_key: key, body };
      commands.push(command);
    }
    if (match.operation === "approval_response") return respond(202, { response_id: command.event_id, phase: "recorded" });
    return respond(202, { command_id: command.event_id, phase: "recorded", step_up_status: "missing" });
  }

  const fetch: Fetch = async (input, init) => {
    const method = (init.method ?? "GET").toUpperCase();
    if (scenario === "unreachable") throw new TypeError("Failed to fetch");
    if (scenario === "result-unknown" && method === "POST") throw new TypeError("The deployment took the request and never answered");
    const url = new URL(input, "http://mock.invalid");
    if (!url.pathname.startsWith(`${prefix}/`)) return problem(404, "not_found", "none");
    const route = url.pathname.slice(prefix.length);
    if (method === "GET") return get(route);
    if (method === "POST") return post(route, new Headers(init.headers), typeof init.body === "string" ? init.body : "");
    return problem(404, "not_found", "none");
  };

  return { fetch, recorded: () => commands.map((c) => ({ ...c })) };
}
