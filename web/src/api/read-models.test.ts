import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { type AgentRow, type ApprovalRow, agentRead, agentsRead, approvalRead, approvalsRead, deadlineIso, loadApi } from "./read-models";
import type { Scenario } from "@/fixtures/types";
import { buildWorkspace } from "@/fixtures/workspace";

const WS = "ws_01J9ZQ4M0000000000000000AB";
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
    expect(agentRead({ agent: { ...ROW, positions: [POSITION] } }, "")).toEqual({ ok: true, value: { agent: { ...ROW, positions: [POSITION] } } });
    expect(ok(agentRead({ agent: { ...ROW, positions: null } }, ""))).toBe(true);
    expect(approvalsRead({ approvals: [APPROVAL] }, "")).toEqual({ ok: true, value: { approvals: [APPROVAL] } });
    const protectedOrder = { ...ORDER, tif: "day", protection: { stop: "54000", take_profit: null } };
    expect(ok(approvalRead({ approval: { ...APPROVAL, order: protectedOrder, content: { action: { side: "buy" } } } }, ""))).toBe(true);
  });

  it("refuses a decimal sent as a JSON number, and a count that is not one", () => {
    expect(problem(agentsRead({ agents: [{ ...ROW, mandate: { ...ROW.mandate, max_order_usd: 1000 } }] }, ""))).toBe("decimal_number /agents/0/mandate/max_order_usd");
    expect(problem(agentRead({ agent: { ...ROW, positions: [{ ...POSITION, qty: 0.015 }] } }, ""))).toBe("decimal_number /agent/positions/0/qty");
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
    const { mandate: _m, ...noMandate } = ROW;
    expect(problem(agentsRead({ agents: [noMandate] }, ""))).toBe("missing /agents/0/mandate");
    expect(problem(agentsRead({ agents: [{ ...ROW, label: "BTC Accumulator" }] }, ""))).toBe("not_canonical /agents/0/label");
    const { content_hash: _h, ...noHash } = APPROVAL;
    expect(problem(approvalsRead({ approvals: [noHash] }, ""))).toBe("missing /approvals/0/content_hash");
    expect(problem(agentRead({ agent: ROW }, ""))).toBe("missing /agent/positions");
  });

  it("shows a deadline as the UTC time its risk-clock seconds name", () => {
    expect(deadlineIso(1791476040)).toBe("2026-10-08T14:54:00.000Z");
    expect(deadlineIso(0)).toBe("1970-01-01T00:00:00.000Z");
  });
});

/** The mock API over a fixture scenario, with `status` free to answer a route itself. */
function served(scenario: Scenario, status?: (route: string) => number | null) {
  const server = createMockServer({ workspaceId: WS, scenario });
  const routes: string[] = [];
  const fetch: Fetch = async (input, init) => {
    const url = new URL(input, "http://x.invalid");
    const route = url.pathname.slice(`/v1/workspaces/${WS}`.length);
    routes.push(`${init.method} ${route}${url.search}`);
    const forced = status?.(route);
    if (forced) return new Response(JSON.stringify({ type: "about:blank", status: forced, title: "Refused", code: forced === 404 ? "not_found" : "membership_unavailable", effect: "none", retryable: forced !== 404 }), { status: forced });
    return server.fetch(input, init);
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
        expect(read.positions?.map((p) => p.instrument)).toEqual(agent.positions.map((p) => p.instrument.symbol));
        expect(read.pending_approvals).toBe(fixture.approvals.filter((a) => a.agent_id === agent.agent_id && a.status === "delivered").length);
      }
      expect(view.approvals.map((a) => a.approval_id).sort(), scenario).toEqual(fixture.approvals.map((a) => a.approval_id).sort());
      for (const approval of fixture.approvals) {
        const read = view.approvals.find((a) => a.approval_id === approval.approval_id)!;
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

  it("reads health and connections when served, and reports them as not reported when the API answers 404", async () => {
    const loaded = await loadApi(served("approvals", (route) => (route === "/health" || route === "/connections" ? 404 : null)).client);
    expect(loaded.ok).toBe(true);
    if (!loaded.ok) return;
    expect(loaded.ws.connection.connection_id).toBe("");
    for (const name of ["market_data", "broker", "deployment", "relay"] as const) expect(loaded.ws.health[name].state).toBe("stale");
    expect(loaded.view.agents.length).toBeGreaterThan(0);
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
