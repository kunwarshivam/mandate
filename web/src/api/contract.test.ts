import { readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { type Fetch, OPERATIONS, createWorkspaceClient } from "./client";
import { type CommandScope, prepareApprovalResponse, prepareAskAgent, prepareConfirm, prepareOwnerCommand, prepareOwnerExit } from "./commands";
import { createMockServer } from "./mock-server";
import { NOT_COMMANDS, WEB_OPERATIONS, operationFor } from "./operations";
import { type ApiView, loadApi } from "./read-models";
import type { Operation } from "./types";
import { prepareCreateVersion, readConfirmation, versionCreated } from "./versions";
import { buildWorkspace } from "@/fixtures/workspace";
import type { CommandKind, ConfirmRecord } from "@/lib/mock-runtime";

/**
 * The web app is one client of the published workspace API (DEC-967 item 13, DEC-975): every call it
 * makes is an operation the spec documents, under the workspace prefix, with a documented query. This
 * is E21-45's check from the web side; the OpenAPI document replaces the spec's tables when it is
 * published.
 */
const WEB = join(__dirname, "..", "..");
const SPEC = readFileSync(join(WEB, "..", "docs", "specs", "workspace-api.md"), "utf8");
const SCHEMAS = join(WEB, "..", "schemas", "workspace-api", "commands");
const WS = "ws_01J9ZQ4M0000000000000000AB";

/** `{agent}`, `{id}` and `{agent_id}` are one path id. */
const shape = (route: string) => route.replace(/\{[a-z_]+\}/g, "{}");

/**
 * Every `METHOD /path` the spec's operation tables and §5 headings document under the workspace
 * prefix, with its query members, the prefix itself dropped where the spec writes it out
 * (`/v1/workspaces/{ws}/...`); and every `/v1/me/...` route, whole. A `.../resume` cell continues
 * the path before it.
 */
function documented(): Map<string, Set<string>> {
  const out = new Map<string, Set<string>>();
  let last = "";
  for (const match of SPEC.matchAll(/`(?:(GET|POST|PUT|PATCH|DELETE),?\s+)?((?:\/|\.\.\.\/)[^`\s]*)`/g)) {
    const method = match[1] ?? (match[2].startsWith(".../") ? last.split(" ")[0] : null);
    if (!method) continue;
    let path = match[2];
    if (path.startsWith(".../")) path = `${last.split(" ")[1].replace(/\/[^/]+$/, "")}/${path.slice(4)}`;
    const [full, query = ""] = path.split("?");
    const route = full.replace(/^\/v1\/workspaces\/\{ws\}(?=\/)/, "");
    if (route.startsWith("/v1/") && !route.startsWith("/v1/me/")) continue;
    last = `${method} ${route}`;
    const key = `${method} ${shape(route)}`;
    const names = out.get(key) ?? new Set<string>();
    for (const part of query.split("&").filter(Boolean)) names.add(part.split("=")[0]);
    out.set(key, names);
  }
  return out;
}

/** Each command's request schema; the deploy body's lands with its `mandate-api` type (§3.11 item 11), so its members are checked against the spec's list instead. */
const SCHEMA_OF: Readonly<Record<Exclude<Operation, "deploy">, string>> = {
  pause: "pause-request",
  resume: "resume-request",
  hold_openings: "hold-request",
  stop: "stop-request",
  owner_exit: "owner-exit-request",
  ask_agent: "owner-request",
  acknowledge: "acknowledge-request",
  kill_switch: "kill-switch-request",
  approval_response: "approval-response-request",
  end_delegation: "end-delegation-request",
  confirm_version: "confirm-request",
  create_version: "../../mandate",
};

/** A new agent's A5 record, as the set-up's confirm sends it (DEC-2401). */
const CONFIRM_RECORD: ConfirmRecord = { screen: "A5", environment: "paper", shown: ["Your agent", "Version 1"], paths: ["/capital/allocation_usd", "/environment"], classification: "risk_increasing" };
const CONFIRM_VERSION = `sha256:${"1f".repeat(32)}` as const;
/** What `GET .../confirmation` would serve; the client passes it through (DEC-2401 item 4). */
const SERVED_DIGEST = `sha256:${"5d".repeat(32)}` as const;

interface Schema {
  required?: string[];
  properties?: Record<string, { enum?: unknown[]; const?: unknown; properties?: Record<string, { enum?: unknown[] }> }>;
  additionalProperties?: boolean;
}

function schemaFor(operation: Operation): Schema {
  if (operation === "deploy") throw new Error("the deploy body has no request schema yet; check it against DEPLOY_MEMBERS");
  return JSON.parse(readFileSync(join(SCHEMAS, `${SCHEMA_OF[operation]}.schema.json`), "utf8")) as Schema;
}

/** A client that records every request it makes, against the contract-faithful mock. */
function recording(scenario: "normal" | "approvals" = "approvals") {
  const server = createMockServer({ workspaceId: WS, scenario });
  const calls: Array<{ method: string; url: string; body: string | null }> = [];
  const fetch: Fetch = (input, init) => {
    calls.push({ method: init.method ?? "GET", url: input, body: typeof init.body === "string" ? init.body : null });
    return server.fetch(input, init);
  };
  return { server, calls, client: createWorkspaceClient({ workspaceId: WS, fetch, baseUrl: "https://api.owlhead.ai" }) };
}

async function loaded(client: ReturnType<typeof recording>["client"]): Promise<{ view: ApiView; scope: CommandScope }> {
  const result = await loadApi(client);
  if (!result.ok) throw new Error(`not loaded: ${result.error.code}`);
  return { view: result.view, scope: { agents: result.view.agents, connectionId: result.ws.connection.connection_id, environment: result.ws.environment } };
}

/** An owner exit's asset and screen, as the positions screen sends them (§5.4). */
const EXIT_ASSET = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const EXIT_RECORD = { screen: "D10" as const, environment: "paper" as const, title: "Exit", shown: ["Sell all of it."], modes: [] };

const KINDS: CommandKind[] = ["pause", "resume", "stop", "kill", "release", "pause_all", "stop_all", "close_all"];

describe("the web app calls only documented workspace API operations (E21-45, DEC-975)", () => {
  it("lists only operations the spec documents, with documented query members", () => {
    const spec = documented();
    for (const op of WEB_OPERATIONS) {
      const names = spec.get(`${op.method} ${shape(op.route)}`);
      expect(names, `${op.method} ${op.route} (${op.section})`).toBeDefined();
      for (const q of op.query) expect(names?.has(q), `${op.route}?${q}`).toBe(true);
    }
  });

  it("lists every command the client can prepare, and no order ticket (DEC-528)", () => {
    for (const operation of Object.keys(OPERATIONS)) expect(WEB_OPERATIONS.some((op) => op.id === operation && op.method === "POST")).toBe(true);
    const posts = WEB_OPERATIONS.filter((op) => op.method === "POST" && !NOT_COMMANDS.has(op.id)).map((op) => op.id);
    expect(posts.sort()).toEqual(Object.keys(OPERATIONS).sort());
  });

  it("matches a request to its operation only whole, under this workspace, and with documented query members", () => {
    expect(operationFor("GET", `https://api.owlhead.ai/v1/workspaces/${WS}/agents/agt_1`, WS)?.id).toBe("agent");
    expect(operationFor("GET", `/v1/workspaces/${WS}/approvals?state=pending`, WS)?.id).toBe("approvals");
    expect(operationFor("GET", `/v1/workspaces/${WS}/backtests/bt_01JBR2C4D6E8F0G2H4J6K8M0N2`, WS)?.id).toBe("backtest");
    expect(operationFor("GET", `/v1/workspaces/${WS}/backtests`, WS)).toBeNull();
    expect(operationFor("POST", `/v1/workspaces/${WS}/backtests`, WS)).toBeNull();
    expect(operationFor("POST", `/v1/workspaces/${WS}/agents/agt_1/pause`, WS)?.id).toBe("pause");
    expect(operationFor("POST", `/v1/workspaces/${WS}/kill-switch`, WS)?.id).toBe("kill_switch");
    expect(operationFor("GET", `/v1/workspaces/${WS}/approvals?state=pending&secret=1`, WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/${WS}/agents/agt_1?x=1`, WS)).toBeNull();
    expect(operationFor("POST", `/v1/workspaces/${WS}/agents/agt_1`, WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/${WS}/agents/agt_1/pause`, WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/other/agents`, WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/${WS}/agents/a/b/c`, WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/${WS}/agents/`, WS)).toBeNull();
    expect(operationFor("GET", `/v2/workspaces/${WS}/agents`, WS)).toBeNull();
    expect(operationFor("POST", `/v1/workspaces/${WS}/agents/agt_1/orders`, WS)).toBeNull();
    expect(operationFor("POST", `/v1/workspaces/${WS}/step-up/challenges`, WS)?.id).toBe("step_up_challenge");
    expect(operationFor("GET", "https://api.owlhead.ai/v1/me/session", WS)?.id).toBe("session");
    expect(operationFor("GET", "/v1/me/session?x=1", WS)).toBeNull();
    expect(operationFor("POST", "/v1/me/session", WS)).toBeNull();
    expect(operationFor("GET", `/v1/workspaces/${WS}/v1/me/session`, WS)).toBeNull();
  });

  it("makes no request outside the table while loading the workspace and sending every command and answer", async () => {
    const { client, calls } = recording();
    const { view, scope } = await loaded(client);
    const sent = [];
    for (const kind of KINDS) {
      const prepared = prepareOwnerCommand(client, scope, kind, kind.endsWith("_all") ? null : view.agents[0].agent_id, { screen: "D10", environment: scope.environment, title: "Stop", shown: ["one line"], modes: [] });
      if (prepared.ok) for (const command of prepared.commands) sent.push(await client.send(command));
    }
    for (const approval of view.approvals.filter((a) => a.state === "pending"))
      for (const response of ["approve", "skip"] as const) {
        const prepared = prepareApprovalResponse(client, approval, response, { screen: "D6", environment: scope.environment, shown: ["line"], modelOutputExpanded: false });
        if (prepared.ok) for (const command of prepared.commands) sent.push(await client.send(command));
      }
    const pendingOne = view.approvals.find((a) => a.state === "pending");
    if (!pendingOne) throw new Error("the approvals fixture has an open request");
    const challenge = await client.askChallenge({ kind: "approve", digest: pendingOne.content_hash });
    expect(challenge.ok).toBe(true);
    expect((await client.readSession()).ok).toBe(true);
    if (challenge.ok) {
      const confirmed = prepareApprovalResponse(client, pendingOne, "approve", { screen: "D6", environment: scope.environment, shown: ["line"], modelOutputExpanded: false }, challenge.value);
      if (confirmed.ok) for (const command of confirmed.commands) sent.push(await client.send(command));
    }
    const exit = prepareOwnerExit(client, view.agents[0].agent_id, EXIT_ASSET, EXIT_RECORD);
    if (!exit.ok) throw new Error(exit.reason);
    sent.push(await client.send(exit.commands[0]));
    sent.push(await client.send(prepareConfirm(client, { version: CONFIRM_VERSION, agentId: null, base: null, record: CONFIRM_RECORD, served: { screen_digest: SERVED_DIGEST, classification: "risk_increasing" }, stepUp: null })));
    const created = await client.send(prepareCreateVersion(client, buildWorkspace("normal").agents[0].mandate));
    const version = versionCreated(created.ok ? created.body : null);
    expect(version.ok).toBe(true);
    if (version.ok) expect((await readConfirmation(client, version.value.mandate_version)).ok).toBe(true);
    sent.push(created);
    for (const outcome of sent) if (outcome.ok) await client.commandStatus(outcome.accepted.event_id);
    expect(sent.length).toBeGreaterThan(KINDS.length);
    expect(calls.length).toBeGreaterThan(10);
    for (const call of calls) {
      const op = operationFor(call.method, call.url, WS);
      expect(op, `${call.method} ${call.url}`).not.toBeNull();
      expect(call.url.startsWith(op?.outside ? "https://api.owlhead.ai/v1/me/" : "https://api.owlhead.ai/v1/workspaces/"), call.url).toBe(true);
    }
  });

  it("sends each command body its request schema allows: every required member, and no other", async () => {
    const { client } = recording();
    const { view, scope } = await loaded(client);
    const bodies: Array<{ operation: Operation; body: Record<string, unknown> }> = [];
    for (const kind of KINDS) {
      const prepared = prepareOwnerCommand(client, scope, kind, kind.endsWith("_all") ? null : view.agents[0].agent_id, { screen: "D10", environment: scope.environment, title: "Stop", shown: ["one line"], modes: [] });
      if (prepared.ok) for (const c of prepared.commands) bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    }
    const approval = view.approvals.find((a) => a.state === "pending");
    if (!approval) throw new Error("the approvals fixture has an open request");
    for (const response of ["approve", "skip"] as const) {
      const prepared = prepareApprovalResponse(client, approval, response, { screen: "D6", environment: scope.environment, shown: ["line"], modelOutputExpanded: true });
      if (prepared.ok) for (const c of prepared.commands) bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    }
    const challenge = await client.askChallenge({ kind: "approve", digest: approval.content_hash });
    if (!challenge.ok) throw new Error("the mock issues a challenge");
    const confirmed = prepareApprovalResponse(client, approval, "approve", { screen: "D6", environment: scope.environment, shown: ["line"], modelOutputExpanded: true }, challenge.value);
    if (confirmed.ok) for (const c of confirmed.commands) bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    const evidence = JSON.parse(readFileSync(join(SCHEMAS, "..", "envelope.schema.json"), "utf8")) as { $defs: { StepUpEvidence: Schema } };
    const withStepUp = bodies.filter((b) => b.body.step_up !== null && b.body.step_up !== undefined);
    expect(withStepUp).toHaveLength(1);
    for (const { body } of withStepUp) {
      const step = body.step_up as Record<string, unknown>;
      expect(Object.keys(step).sort()).toEqual([...(evidence.$defs.StepUpEvidence.required ?? [])].sort());
      expect(evidence.$defs.StepUpEvidence.properties?.method.enum).toContain(step.method);
    }
    for (const classification of ["risk_increasing", "risk_reducing", "neutral"] as const) {
      const c = prepareConfirm(client, { version: CONFIRM_VERSION, agentId: null, base: null, record: CONFIRM_RECORD, served: { screen_digest: SERVED_DIGEST, classification }, stepUp: null });
      bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    }
    const exit = prepareOwnerExit(client, view.agents[0].agent_id, EXIT_ASSET, EXIT_RECORD);
    if (exit.ok) for (const c of exit.commands) bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    const ask = prepareAskAgent(client, view.agents[0].agent_id, EXIT_ASSET, "2.5");
    if (ask.ok) for (const c of ask.commands) bodies.push({ operation: c.operation, body: JSON.parse(c.body) as Record<string, unknown> });
    expect(new Set(bodies.map((b) => b.operation))).toEqual(new Set(["pause", "resume", "stop", "kill_switch", "approval_response", "confirm_version", "owner_exit", "ask_agent"]));
    for (const { operation, body } of bodies) {
      const schema = schemaFor(operation);
      for (const member of schema.required ?? []) expect(Object.hasOwn(body, member), `${operation} needs ${member}`).toBe(true);
      for (const member of Object.keys(body)) expect(Object.hasOwn(schema.properties ?? {}, member), `${operation} has no ${member}`).toBe(true);
      for (const [member, value] of Object.entries(body)) {
        const allowed = schema.properties?.[member]?.enum;
        if (allowed) expect(allowed, `${operation}.${member}`).toContain(value);
      }
      expect(JSON.stringify(body), operation).not.toMatch(/"requested_by"/);
    }
    for (const { operation, body } of bodies.filter((b) => b.operation === "kill_switch")) {
      const kinds = schemaFor(operation).properties?.scope.properties?.kind.enum ?? [];
      expect(kinds, operation).toContain((body.scope as { kind: string }).kind);
    }
  });

  it("keeps the API's paths in src/api: no other module names a /v1 path or calls fetch for the workspace", () => {
    const src = join(WEB, "src");
    const files = readdirSync(src, { recursive: true })
      .map((name) => join(src, String(name)))
      .filter((path) => /\.(ts|tsx)$/.test(path) && !/\.test\.tsx?$/.test(path));
    for (const path of files) {
      const rel = relative(src, path);
      if (rel.startsWith("api/") || rel.startsWith("app/v1/")) continue;
      expect(readFileSync(path, "utf8"), rel).not.toMatch(/["'`]\/v1\//);
    }
  });
});
