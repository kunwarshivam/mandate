import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { type AgentRow, type ApprovalRow, agentRead, agentsRead, approvalRead, approvalsRead, deadlineIso, loadApi } from "./read-models";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";

const WS = "ws_01J9ZQ4M0000000000000000AB";
/** The API names an approval by its event id, a bare ULID; the fixtures prefix theirs with `apr_`. */
const servedId = (id: string) => id.replace(/^apr_/, "");
const HASH = `sha256:${"ab".repeat(32)}` as const;
const AT = "2026-10-08T14:30:00.000000000Z";

const ROW: AgentRow = {
  agent_id: "agt_01JB3K6F0C9R2V7N4M8Q1T5W3Z",
  environment: "paper",
  mandate_version: HASH,
  deployed_at: AT,
  mode: "normal",
  pending_approvals: 1,
  label: "btc-accumulator",
  mandate: {
    allocation_usd: "10000",
    max_loss_from_allocation: "0.1",
    goal: "accumulate",
    goal_end_date: "2026-12-31",
    asset_classes: ["crypto"],
    max_position_usd: "5000",
    max_order_usd: "1000",
    max_orders_per_day: 10,
    max_daily_loss: "0.02",
    max_drawdown: "0.1",
  },
};
const POSITION = { instrument: "BTC/USD", qty: "0.015", broker_qty: "0.015", mismatch: false, observed_at: AT };
const ORDER = { instrument: "BTC/USD", asset_class: "crypto", side: "buy", qty: "0.015", limit: "56700", order_usd: "850.5", purpose: "open", tif: null, protection: null };
const APPROVAL: ApprovalRow = { approval_id: "01JBH3BTZ8K2M4N6P8R0S2T4V6", agent_id: ROW.agent_id, state: "pending", requested_at: AT, deadline: 1791476040, content_hash: HASH, order: ORDER as ApprovalRow["order"] };

const ok = <T,>(decoded: { ok: boolean; value?: T }) => decoded.ok;
const problem = (decoded: { ok: true } | { ok: false; issue: { problem: string; path: string } }) => (decoded.ok ? "ok" : `${decoded.issue.problem} ${decoded.issue.path}`);

describe("the read models as the API serves them (#1446, #1499; DEC-1096)", () => {
  it("reads every member the schemas name, with nulls where the API has none", () => {
    expect(agentsRead({ agents: [ROW, { ...ROW, agent_id: "agt_2", mode: null, label: null, mandate: null }] }, "")).toEqual({
      ok: true,
      value: { agents: [ROW, { ...ROW, agent_id: "agt_2", mode: null, label: null, mandate: null }] },
    });
    expect(agentRead({ agent: { ...ROW, account_positions: [POSITION] } }, "")).toEqual({ ok: true, value: { agent: { ...ROW, account_positions: [POSITION] } } });
    expect(ok(agentRead({ agent: { ...ROW, account_positions: null } }, ""))).toBe(true);
    expect(approvalsRead({ approvals: [APPROVAL] }, "")).toEqual({ ok: true, value: { approvals: [APPROVAL] } });
    const protectedOrder = { ...ORDER, tif: "day", protection: { stop: "54000", take_profit: null } };
    expect(ok(approvalRead({ approval: { ...APPROVAL, order: protectedOrder, content: { action: { side: "buy" } }, schema_version: 2, decision_deferred_to_binding: [] } }, ""))).toBe(true);
  });

  it("reads the ApprovalRequested version an approval's content comes from, and refuses a read without one (DEC-2518)", () => {
    const detail = { ...APPROVAL, content: { action: { side: "buy" } }, decision_deferred_to_binding: [] };
    for (const version of [1, 2, 3]) expect(approvalRead({ approval: { ...detail, schema_version: version } }, ""), String(version)).toMatchObject({ ok: true, value: { approval: { schema_version: version } } });
    expect(problem(approvalRead({ approval: detail }, ""))).toBe("missing /approval/schema_version");
    for (const version of [0, 4, "3", null, 2.5]) expect(problem(approvalRead({ approval: { ...detail, schema_version: version } }, "")), String(version)).toMatch(/^(unknown_enum_value|wrong_type) \/approval\/schema_version$/);
  });

  it("reads the deferred checks of the decision behind an approval, or null without one, and refuses a read without them (DEC-2519)", () => {
    const detail = { ...APPROVAL, content: { action: { side: "buy" } }, schema_version: 2 };
    for (const decided of [null, [], ["halt"], ["anything", 7]]) {
      expect(approvalRead({ approval: { ...detail, decision_deferred_to_binding: decided } }, ""), JSON.stringify(decided)).toMatchObject({ ok: true, value: { approval: { decision_deferred_to_binding: decided } } });
    }
    expect(problem(approvalRead({ approval: detail }, ""))).toBe("missing /approval/decision_deferred_to_binding");
    for (const decided of ["halt", 3, {}]) expect(problem(approvalRead({ approval: { ...detail, decision_deferred_to_binding: decided } }, "")), JSON.stringify(decided)).toBe("wrong_type /approval/decision_deferred_to_binding");
  });

  it("refuses a decimal sent as a JSON number, and a count that is not one", () => {
    expect(problem(agentsRead({ agents: [{ ...ROW, mandate: { ...ROW.mandate, max_order_usd: 1000 } }] }, ""))).toBe("decimal_number /agents/0/mandate/max_order_usd");
    expect(problem(agentRead({ agent: { ...ROW, account_positions: [{ ...POSITION, qty: 0.015 }] } }, ""))).toBe("decimal_number /agent/account_positions/0/qty");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, order: { ...ORDER, limit: 56700 } }] }, ""))).toBe("decimal_number /approvals/0/order/limit");
    expect(problem(agentsRead({ agents: [{ ...ROW, pending_approvals: -1 }] }, ""))).toBe("wrong_type /agents/0/pending_approvals");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, deadline: "1791476040" }] }, ""))).toBe("wrong_type /approvals/0/deadline");
  });

  it("refuses a value a closed enum does not list, so nothing renders as something else", () => {
    expect(problem(agentsRead({ agents: [{ ...ROW, mode: "halted" }] }, ""))).toBe("unknown_enum_value /agents/0/mode");
    expect(problem(agentsRead({ agents: [{ ...ROW, environment: "sandbox" }] }, ""))).toBe("unknown_enum_value /agents/0/environment");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, state: "approved" }] }, ""))).toBe("unknown_enum_value /approvals/0/state");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, order: { ...ORDER, side: "sell" } }] }, ""))).toBe("unknown_enum_value /approvals/0/order/side");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, order: { ...ORDER, purpose: "risk_exit" } }] }, ""))).toBe("unknown_enum_value /approvals/0/order/purpose");
  });

  it("refuses a missing member, an unshaped label, and an approval without the content hash it binds (API-12)", () => {
    const noMandate: Record<string, unknown> = { ...ROW };
    delete noMandate.mandate;
    expect(problem(agentsRead({ agents: [noMandate] }, ""))).toBe("missing /agents/0/mandate");
    expect(problem(agentsRead({ agents: [{ ...ROW, label: "BTC Accumulator" }] }, ""))).toBe("not_canonical /agents/0/label");
    const noHash: Record<string, unknown> = { ...APPROVAL };
    delete noHash.content_hash;
    expect(problem(approvalsRead({ approvals: [noHash] }, ""))).toBe("missing /approvals/0/content_hash");
    expect(problem(agentRead({ agent: ROW }, ""))).toBe("missing /agent/account_positions");
    expect(problem(agentsRead({ agents: [{ ...ROW, agent_id: "agt/../x" }] }, ""))).toBe("not_canonical /agents/0/agent_id");
    expect(problem(approvalsRead({ approvals: [{ ...APPROVAL, approval_id: "apr_01JBH3BTZ8K2M4N6P8R0S2T4V6" }] }, ""))).toBe("not_canonical /approvals/0/approval_id");
    expect(problem(agentsRead({ agents: [{ ...ROW, mandate: { ...ROW.mandate, asset_classes: [] } }] }, ""))).toBe("not_canonical /agents/0/mandate/asset_classes");
    expect(problem(agentsRead({ agents: [{ ...ROW, mandate: { ...ROW.mandate, asset_classes: ["crypto", "crypto"] } }] }, ""))).toBe("not_canonical /agents/0/mandate/asset_classes");
  });

  it("shows a deadline as the UTC time its risk-clock seconds name", () => {
    expect(deadlineIso(1791476040)).toBe("2026-10-08T16:14:00.000Z");
    expect(deadlineIso(0)).toBe("1970-01-01T00:00:00.000Z");
  });
});

/** The mock API over a fixture scenario, with `status` free to answer a route itself. */
type Body = Record<string, unknown>;

function served(scenario: Scenario, status?: (route: string) => number | null, rewrite?: (body: Body, route: string) => Body) {
  const server = createMockServer({ workspaceId: WS, scenario });
  const routes: string[] = [];
  const fetch: Fetch = async (input, init) => {
    const url = new URL(input, "http://x.invalid");
    const route = url.pathname.slice(`/v1/workspaces/${WS}`.length);
    routes.push(`${init.method} ${route}${url.search}`);
    const forced = status?.(route);
    if (forced) return new Response(JSON.stringify({ type: "about:blank", status: forced, title: "Refused", code: forced === 404 ? "not_found" : "membership_unavailable", effect: "none", retryable: forced !== 404 }), { status: forced });
    const answer = await server.fetch(input, init);
    if (!rewrite) return answer;
    return new Response(JSON.stringify(rewrite((await answer.json()) as Body, route)), { status: answer.status, headers: answer.headers });
  };
  return { client: createWorkspaceClient({ workspaceId: WS, fetch }), routes };
}

describe("the workspace from the API's read models (DEC-1096)", () => {
  it("holds every agent and approval the fixtures hold, as the read models carry them, and nothing else", async () => {
    for (const scenario of ["normal", "approvals", "paused"] as const) {
      const fixture = buildWorkspace(scenario);
      const loaded = await loadApi(served(scenario).client);
      expect(loaded.ok, scenario).toBe(true);
      if (!loaded.ok) continue;
      const { view, ws } = loaded;
      expect(view.agents.map((a) => a.agent_id), scenario).toEqual(fixture.agents.map((a) => a.agent_id));
      for (const agent of fixture.agents) {
        const read = view.agents.find((a) => a.agent_id === agent.agent_id)!;
        expect(read.mode, scenario).toBe(agent.mode);
        expect(read.mandate?.allocation_usd).toBe(agent.mandate.capital.allocation_usd);
        expect(read.mandate?.max_order_usd).toBe(agent.mandate.risk.max_order_usd);
        expect(read.account_positions?.map((p) => p.instrument), "the account's holdings, every agent's").toEqual(fixture.agents.flatMap((a) => a.positions).map((p) => p.instrument.symbol));
        expect(read.pending_approvals).toBe(fixture.approvals.filter((a) => a.agent_id === agent.agent_id && a.status === "delivered").length);
      }
      expect(view.approvals.map((a) => a.approval_id).sort(), scenario).toEqual(fixture.approvals.map((a) => servedId(a.approval_id)).sort());
      for (const approval of fixture.approvals) {
        const read = view.approvals.find((a) => a.approval_id === servedId(approval.approval_id))!;
        expect(read.order.qty).toBe(approval.bound.qty);
        expect(read.order.limit).toBe(approval.bound.limit);
        expect(read.order.instrument).toBe(approval.bound.symbol);
        expect(read.state === "pending").toBe(approval.status === "delivered");
      }
      expect(ws.status).toBe("ready");
      expect(ws.agents).toEqual([]);
      expect(ws.approvals).toEqual([]);
      expect(ws.environment).toBe(fixture.environment);
      expect(ws.connection.connection_id).toBe(fixture.connection.connection_id);
      expect(loaded.as_of.length).toBeGreaterThan(0);
    }
  });

  it("reports health as not reported, and connections as not reported when the API answers 404", async () => {
    const loaded = await loadApi(served("approvals", (route) => (route === "/connections" ? 404 : null)).client);
    expect(loaded.ok).toBe(true);
    if (!loaded.ok) return;
    expect(loaded.ws.connection.connection_id).toBe("");
    for (const name of ["market_data", "broker", "deployment", "relay"] as const) {
      expect(loaded.ws.health[name].state).toBe("stale");
      expect(loaded.ws.health[name].as_of, `${name} carries no client-clock time`).toBeNull();
    }
    expect(loaded.view.agents.length).toBeGreaterThan(0);
    expect(loaded.ws.environment, "every agent the API serves names paper").toBe("paper");
  });

  it("names the environment only when something served names it, never defaulting to paper", async () => {
    const unserved = (route: string) => (route === "/health" || route === "/connections" ? 404 : null);
    const second = `/agents/${buildWorkspace("approvals").agents[1].agent_id}`;
    const live = (body: Body, route: string) => (route === second ? { ...body, agent: { ...(body.agent as object), environment: "live" } } : body);

    const disagree = await loadApi(served("approvals", unserved, live).client);
    expect(disagree.ok).toBe(true);
    if (disagree.ok) {
      expect(new Set(disagree.view.agents.map((a) => a.environment))).toEqual(new Set(["paper", "live"]));
      expect(disagree.ws.environment, "agents that disagree name no environment").toBe("unknown");
    }

    const empty = (body: Body, route: string) => (route === "/agents" ? { ...body, agents: [] } : route === "/approvals" ? { ...body, approvals: [] } : body);
    const none = await loadApi(served("approvals", unserved, empty).client);
    expect(none.ok ? null : none.error).toBeNull();
    if (none.ok) expect(none.ws.environment, "no agent and no connection name none").toBe("unknown");

    const connected = await loadApi(served("approvals", (route) => (route === "/health" ? 404 : null), live).client);
    expect(connected.ok).toBe(true);
    if (connected.ok) expect(connected.ws.environment, "the connection's environment wins over agents that disagree").toBe("paper");
  });

  it("never reads the unrouted /health, so the API's 401 for it cannot hide the agents or their details", async () => {
    const { client, routes } = served("approvals", (route) => (route === "/health" ? 401 : null));
    const loaded = await loadApi(client);
    expect(loaded.ok ? null : loaded.error).toBeNull();
    expect(routes).not.toContain("GET /health");
    if (!loaded.ok) return;
    expect(loaded.view.agents.length).toBeGreaterThan(0);
    for (const agent of loaded.view.agents) expect(routes).toContain(`GET /agents/${agent.agent_id}`);
    for (const name of ["market_data", "broker", "deployment", "relay"] as const) expect(loaded.ws.health[name]).toEqual({ state: "stale", as_of: null });
  });

  it("reads only the documented GET routes, both halves of the inbox, and each id's detail once", async () => {
    const { client, routes } = served("approvals");
    await loadApi(client);
    expect(routes.every((r) => r.startsWith("GET "))).toBe(true);
    expect(routes).toContain("GET /approvals?state=pending");
    expect(routes).toContain("GET /approvals?state=resolved");
    expect(new Set(routes).size).toBe(routes.length);
  });

  it("fails as the API answered: membership_unavailable and unauthenticated keep their codes", async () => {
    const membership = await loadApi(served("approvals", (route) => (route === "/agents" ? 503 : null)).client);
    expect(membership.ok).toBe(false);
    if (!membership.ok) expect(membership.error.code).toBe("membership_unavailable");
    const down = await loadApi(served("unreachable").client);
    expect(down.ok).toBe(false);
    if (!down.ok) expect(down.error.code).toBe("network");
  });
});
