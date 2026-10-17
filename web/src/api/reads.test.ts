import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { emptyWorkspace, loadWorkspace } from "./reads";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";

const WS = "ws_01J9ZQ4M0000000000000000AB";

type Tamper = (route: string, body: Record<string, unknown>) => Record<string, unknown>;

/** The mock API, with `tamper` free to rewrite one read's JSON body before the client sees it. */
function served(scenario: Scenario = "normal", tamper?: Tamper, status?: (route: string) => number | null) {
  const server = createMockServer({ workspaceId: WS, scenario });
  const routes: string[] = [];
  const fetch: Fetch = async (input, init) => {
    const route = new URL(input, "http://x.invalid").pathname.slice(`/v1/workspaces/${WS}`.length);
    routes.push(`${init.method} ${route}${new URL(input, "http://x.invalid").search}`);
    const forced = status?.(route);
    if (forced) return new Response(JSON.stringify({ type: "about:blank", status: forced, title: "Refused", code: "unauthenticated", effect: "none", retryable: false }), { status: forced, headers: { "Content-Type": "application/problem+json" } });
    const answer = await server.fetch(input, init);
    if (!tamper || init.method !== "GET" || !answer.ok) return answer;
    const body = (await answer.json()) as Record<string, unknown>;
    return new Response(JSON.stringify(tamper(route, structuredClone(body))), { status: answer.status, headers: answer.headers });
  };
  return { client: createWorkspaceClient({ workspaceId: WS, fetch }), routes };
}

/** Rewrites one member of the first agent's read, at a path of member names. */
function onAgent(path: string[], value: unknown): Tamper {
  return (route, body) => {
    if (!/^\/agents\/[^/]+$/.test(route)) return body;
    let at = body.agent as Record<string, unknown>;
    for (const key of path.slice(0, -1)) at = at[key] as Record<string, unknown>;
    if (value === undefined) delete at[path[path.length - 1]];
    else at[path[path.length - 1]] = value;
    return body;
  };
}

async function refusal(tamper: Tamper) {
  const loaded = await loadWorkspace(served("normal", tamper).client);
  expect(loaded.ok).toBe(false);
  if (loaded.ok) throw new Error("loaded");
  return loaded.error;
}

describe("the workspace from the workspace API (E11-9, DEC-970)", () => {
  it("holds exactly what the fixtures hold, read from documented routes alone", async () => {
    for (const scenario of ["normal", "approvals", "paused", "drawdown", "unknown-order", "stale"] as const) {
      const fixture = buildWorkspace(scenario);
      const loaded = await loadWorkspace(served(scenario).client);
      expect(loaded.ok, scenario).toBe(true);
      if (!loaded.ok) continue;
      const { ws } = loaded;
      expect(ws.agents, scenario).toEqual(fixture.agents);
      expect(ws.approvals.map(({ content_hash, ...rest }) => (expect(content_hash).toMatch(/^sha256:[0-9a-f]{64}$/), rest)), scenario).toEqual(fixture.approvals);
      expect(ws.decisions, scenario).toEqual(fixture.decisions);
      expect(ws.timeline, scenario).toEqual(Object.fromEntries(fixture.agents.map((a) => [a.agent_id, fixture.timeline[a.agent_id] ?? []])));
      expect(ws.external_positions, scenario).toEqual(fixture.external_positions);
      expect(ws.connection, scenario).toEqual(fixture.connection);
      expect(ws.environment, scenario).toBe(fixture.environment);
      for (const name of ["market_data", "broker", "deployment", "relay"] as const) {
        expect(ws.health[name].state, `${scenario} ${name}`).toBe(fixture.health[name].state);
        expect(Date.parse(ws.health[name].as_of), `${scenario} ${name}`).toBe(Date.parse(fixture.health[name].as_of));
      }
      expect(Date.parse(ws.now), scenario).toBe(Date.parse(fixture.now));
      expect(ws.status).toBe("ready");
      expect(ws.journal).toBe("answers");
      expect(ws.scenario).toBe("normal");
      expect(loaded.as_of.length).toBeGreaterThan(0);
    }
  });

  it("does not know what it was not told: approvers and the independence policy stay at their conservative reading", async () => {
    const loaded = await loadWorkspace(served().client);
    if (!loaded.ok) throw new Error("not loaded");
    expect(loaded.ws.approver_users).toBe(0);
    expect(loaded.ws.independent_approval_required).toBeNull();
  });

  it("asks only GET, and asks both halves of the approvals inbox", async () => {
    const { client, routes } = served("approvals");
    await loadWorkspace(client);
    expect(routes.every((r) => r.startsWith("GET "))).toBe(true);
    expect(routes).toContain("GET /approvals?state=pending");
    expect(routes).toContain("GET /approvals?state=resolved");
  });

  it("refuses a decimal sent as a JSON number, anywhere in a read, and keeps integer members integers", async () => {
    expect((await refusal(onAgent(["pnl_total"], 12.5))).decode?.problem).toBe("decimal_number");
    expect((await refusal(onAgent(["mandate", "risk", "max_order_usd"], 500))).decode?.problem).toBe("decimal_number");
    expect((await refusal(onAgent(["state", "equity"], 1000))).decode?.problem).toBe("decimal_number");
    expect((await refusal(onAgent(["mandate", "risk", "max_orders_per_day"], 1.5))).decode?.problem).toBe("wrong_type");
    expect((await refusal(onAgent(["mandate", "risk", "max_orders_per_day"], "5"))).decode?.problem).toBe("wrong_type");
    const fine = await loadWorkspace(served("normal", onAgent(["mandate", "risk", "max_orders_per_day"], 7)).client);
    expect(fine.ok).toBe(true);
  });

  it("refuses a value a closed safety enum does not list, rather than render it as something else", async () => {
    const issue = (await refusal(onAgent(["mode"], "halted"))).decode;
    expect(issue).toMatchObject({ problem: "unknown_enum_value", value: "halted" });
    expect((await refusal(onAgent(["restrictions"], [{ code: "regulator_hold", since: "2026-10-08T14:30:00.000000000Z" }]))).decode?.problem).toBe("unknown_enum_value");
    expect((await refusal(onAgent(["mandate", "environment"], "sandbox"))).decode?.problem).toBe("unknown_enum_value");
    expect((await refusal(onAgent(["startup"], "warming"))).decode?.problem).toBe("unknown_enum_value");
    const order: Tamper = (route, body) => {
      if (/\/orders$/.test(route)) (body.orders as Array<Record<string, unknown>>).push({ ...(body.orders as Array<Record<string, unknown>>)[0], state: "Lost" });
      return body;
    };
    const unknownOrder = await loadWorkspace(served("unknown-order", order).client);
    expect(unknownOrder.ok).toBe(false);
    const approval: Tamper = (route, body) => (/^\/approvals\/[^/]+$/.test(route) ? { ...body, approval: { ...(body.approval as object), status: "approved" } } : body);
    expect((await refusal(approval)).decode?.problem).toBe("unknown_enum_value");
    const health: Tamper = (route, body) => (route === "/health" ? { ...body, broker: { ...(body.broker as object), state: "fine" } } : body);
    expect((await refusal(health)).decode?.problem).toBe("unknown_enum_value");
    const connection: Tamper = (route, body) => (route === "/connections" ? { ...body, connections: (body.connections as object[]).map((c) => ({ ...c, environment: "sandbox" })) } : body);
    expect((await refusal(connection)).decode?.problem).toBe("unknown_enum_value");
  });

  it("refuses an approval without the content hash an answer must bind (API-12)", async () => {
    const tamper: Tamper = (route, body) => {
      if (/^\/approvals\/[^/]+$/.test(route)) delete (body.approval as Record<string, unknown>).content_hash;
      return body;
    };
    expect((await refusal(tamper)).decode?.problem).toBe("missing");
  });

  it("refuses a read without its watermarks (API-14)", async () => {
    const tamper: Tamper = (route, body) => (route === "/dashboard" ? { ...body, as_of: undefined } : body);
    expect((await refusal(tamper)).code).toBe("invalid_response");
  });

  it("reports a deployment that does not answer, or a session it does not know, as the error it is", async () => {
    const unreachable = await loadWorkspace(served("unreachable").client);
    expect(unreachable.ok).toBe(false);
    if (!unreachable.ok) expect(unreachable.error).toMatchObject({ code: "network", effect: "none" });
    const signedOut = await loadWorkspace(served("normal", undefined, (route) => (route === "/agents" ? 401 : null)).client);
    expect(signedOut.ok).toBe(false);
    if (!signedOut.ok) expect(signedOut.error).toMatchObject({ code: "unauthenticated", status: 401 });
  });

  it("shows nothing before the first answer: no agent, request, decision or holding", () => {
    const ws = emptyWorkspace("loading", "2026-10-08T14:30:00.000000000Z");
    expect(ws).toMatchObject({ status: "loading", agents: [], approvals: [], decisions: [], timeline: {}, external_positions: [], approver_users: 0, independent_approval_required: null });
    expect(emptyWorkspace("unreachable", "2026-10-08T14:30:00.000000000Z").status).toBe("unreachable");
  });
});
