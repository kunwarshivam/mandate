import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { buildWorkspace } from "@/fixtures/workspace";
import { createWorkspaceClient, type Fetch } from "./client";
import { agentSummaryView, createDashboardSource, decodeDashboard, decodeHealth, healthFromReadModel } from "./dashboard";
import { createMockServer } from "./mock-server";

const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const NOW = "2026-09-28T18:05:30.000000000Z";

/** The read-model examples the shared checker validates against their schemas (DEC-740). */
function example(name: string): Record<string, unknown> {
  const examples = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "schemas", "workspace-api", "examples");
  return JSON.parse(readFileSync(join(examples, `read-models.${name}.json`), "utf8")) as Record<string, unknown>;
}

function withMember(body: Record<string, unknown>, path: string[], value: unknown): Record<string, unknown> {
  const copy = structuredClone(body);
  let target = copy as Record<string, unknown>;
  for (const step of path.slice(0, -1)) target = target[step] as Record<string, unknown>;
  target[path[path.length - 1]] = value;
  return copy;
}

/** A fetch that answers each route from a body, counting the reads of each. */
function serving(routes: Record<string, () => unknown>) {
  const reads: Record<string, number> = {};
  const fetch: Fetch = async (input) => {
    const route = new URL(input, "http://api.invalid").pathname.replace(`/v1/workspaces/${WS}`, "");
    const answer = routes[route];
    reads[route] = (reads[route] ?? 0) + 1;
    if (!answer) return new Response(JSON.stringify({ code: "not_found", effect: "none" }), { status: 404 });
    const body = answer();
    if (body instanceof Error) throw body;
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  return { fetch, reads };
}

const options = { now: () => NOW, environment: "paper" as const };

describe("decoding the dashboard and health read models", () => {
  it.skip("pending E11-9: the schema's own examples decode", () => {
    expect(decodeHealth(example("health"), "").ok).toBe(true);
    expect(decodeDashboard(example("dashboard"), "").ok).toBe(true);
  });

  it.skip("pending E11-9: a health state outside the closed set is refused, not guessed", () => {
    const decoded = decodeHealth(withMember(example("health"), ["broker", "state"], "degraded"), "");
    expect(decoded).toMatchObject({ ok: false, issue: { path: "/broker/state", problem: "unknown_enum_value", value: "degraded" } });
  });

  it.skip("pending E11-9: an ok component that says it is stale is refused", () => {
    const decoded = decodeHealth(withMember(example("health"), ["relay", "freshness", "stale"], true), "");
    expect(decoded.ok).toBe(false);
  });

  it.skip("pending E11-9: P&L as a JSON number is refused, never rounded (§3.1)", () => {
    const body = example("dashboard");
    const agents = structuredClone(body.agents) as Array<Record<string, unknown>>;
    agents[0].pnl_total = 123.45;
    expect(decodeDashboard({ ...body, agents }, "")).toMatchObject({ ok: false, issue: { path: "/agents/0/pnl_total", problem: "decimal_number" } });
  });

  it.skip("pending E11-9: a paper agent not labelled simulated is refused", () => {
    const body = example("dashboard");
    const agents = structuredClone(body.agents) as Array<Record<string, unknown>>;
    agents[0].simulated = false;
    expect(decodeDashboard({ ...body, agents }, "").ok).toBe(false);
  });

  it.skip("pending E11-9: a mode outside the closed set is refused (§3.2)", () => {
    const body = example("dashboard");
    const agents = structuredClone(body.agents) as Array<Record<string, unknown>>;
    agents[1].mode = "halted";
    expect(decodeDashboard({ ...body, agents }, "")).toMatchObject({ ok: false, issue: { path: "/agents/1/mode", problem: "unknown_enum_value" } });
  });
});

describe("mapping onto the existing view types (§4.9, §4.10)", () => {
  it.skip("pending E11-9: the G1 strip reads each component's state and last observation time", () => {
    const decoded = decodeHealth(withMember(example("health"), ["market_data"], {
      state: "stale",
      freshness: { observed_at: "2026-09-28T18:02:11.000000000Z", stale: true, age_seconds: 189, limit_source: "health.market_data" },
    }), "");
    if (!decoded.ok) throw new Error("the example must decode");
    expect(healthFromReadModel(decoded.value)).toEqual({
      market_data: { state: "stale", as_of: "2026-09-28T18:02:11.000000000Z" },
      broker: { state: "ok", as_of: "2026-09-28T18:05:15.000000000Z" },
      deployment: { state: "ok", as_of: "2026-09-28T18:05:19.000000000Z" },
      relay: { state: "ok", as_of: "2026-09-28T18:05:10.000000000Z" },
    });
  });

  it.skip("pending E11-1: a paper summary keeps its exact P&L and is labelled simulated", () => {
    const view = agentSummaryView({
      agent_id: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC",
      label: "Agent 1",
      environment: "paper",
      simulated: true,
      mode: "exits_only",
      restrictions: [
        { code: "drawdown_exits_only", since: "2026-09-28T17:41:03.000000000Z", symbol: null },
        { code: "stale_mark", since: "2026-09-28T18:03:11.000000000Z", symbol: "XYZ" },
      ],
      startup: "ready",
      positions_count: 1,
      pnl_total: "123.45",
      pnl_today: "-0.000000001",
      marks_freshness: { observed_at: "2026-09-28T18:02:11.000000000Z", stale: true, age_seconds: 189, limit_source: "data_profile.max_mark_age_s" },
    });
    expect(view).toEqual({
      agent_id: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC",
      label: "Agent 1",
      environment: "paper",
      pnl_label: "Simulated",
      mode: "exits_only",
      restrictions: [
        { code: "drawdown_exits_only", since: "2026-09-28T17:41:03.000000000Z" },
        { code: "stale_mark", since: "2026-09-28T18:03:11.000000000Z", symbol: "XYZ" },
      ],
      startup: "ready",
      positions_count: 1,
      pnl_total: "123.45",
      pnl_today: "-0.000000001",
      marks: { as_of: "2026-09-28T18:02:11.000000000Z", stale: true, age: "3 min" },
    });
  });

  it.skip("pending E11-1: a live summary carries no simulated label, and no marks means no age", () => {
    const view = agentSummaryView({
      agent_id: "agt_01JB3KAQ5R8TV2N4M6P9S1W3XD",
      label: "Agent 3",
      environment: "live",
      simulated: false,
      mode: "normal",
      restrictions: [],
      startup: "reconciling",
      positions_count: 0,
      pnl_total: "0",
      pnl_today: "0",
      marks_freshness: null,
    });
    expect(view.pnl_label).toBeNull();
    expect(view.marks).toBeNull();
    expect(view.startup).toBe("reconciling");
  });
});

describe("loading, failing, and refetching (G1, G4, §3.1, §6.3, §7)", () => {
  it.skip("pending E11-9: a load reads both views and keeps each view's own as_of", async () => {
    const dashboard = example("dashboard");
    const health = example("health");
    const api = serving({ "/dashboard": () => dashboard, "/health": () => health });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    const state = await source.load();
    if (state.status !== "ready") throw new Error("expected a ready dashboard");
    expect(state.agents.map((a) => a.label)).toEqual(["Agent 1", "Agent 2"]);
    expect(state.agents.every((a) => a.pnl_label === "Simulated")).toBe(true);
    expect(state.open_approvals).toBe(1);
    expect(state.served_at).toBe("2026-09-28T18:05:20.000000000Z");
    expect(state.as_of).toEqual({ dashboard: dashboard.as_of, health: health.as_of });
    expect(state.health.deployment).toEqual({ state: "ok", as_of: "2026-09-28T18:05:19.000000000Z" });
  });

  it.skip("pending E11-9: when the API cannot be reached, nothing from the earlier load survives", async () => {
    let reachable = true;
    const api = serving({
      "/dashboard": () => (reachable ? example("dashboard") : new TypeError("Failed to fetch")),
      "/health": () => (reachable ? example("health") : new TypeError("Failed to fetch")),
    });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    expect((await source.load()).status).toBe("ready");
    reachable = false;
    const state = await source.load();
    expect(state.status).toBe("unreachable");
    if (state.status !== "unreachable") return;
    expect(state.workspace.agents).toEqual([]);
    expect(state.workspace.approvals).toEqual([]);
    expect(JSON.stringify(state)).not.toContain("123.45");
    expect(JSON.stringify(source.state())).not.toContain("Agent 1");
  });

  it.skip("pending E11-9: a response that cannot be read is unreachable too, never half shown", async () => {
    const api = serving({ "/dashboard": () => withMember(example("dashboard"), ["open_approvals"], "1"), "/health": () => example("health") });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    const state = await source.load();
    expect(state.status).toBe("unreachable");
    expect(JSON.stringify(state)).not.toContain("Agent 1");
  });

  it.skip("pending E11-9: a newer watermark refetches only the view it is newer than", async () => {
    const api = serving({ "/dashboard": () => example("dashboard"), "/health": () => example("health") });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    await source.load();
    expect(api.reads).toEqual({ "/dashboard": 1, "/health": 1 });
    const agentStream = "agent:ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y:agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";
    expect(await source.notice([{ stream_id: agentStream, seq: 1843 }])).toBe(true);
    expect(api.reads).toEqual({ "/dashboard": 2, "/health": 1 });
  });

  it.skip("pending E11-9: a watermark newer only for health refetches health and not the dashboard", async () => {
    const control = "ctl:ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";
    const health = example("health");
    const behind = (health.as_of as Array<Record<string, unknown>>).map((m) => (m.stream_id === control ? { ...m, seq: 300 } : m));
    const api = serving({ "/dashboard": () => example("dashboard"), "/health": () => ({ ...health, as_of: behind }) });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    await source.load();
    expect(await source.notice([{ stream_id: control, seq: 305 }])).toBe(true);
    expect(api.reads).toEqual({ "/dashboard": 1, "/health": 2 });
  });

  it.skip("pending E11-9: a watermark that is not newer refetches nothing", async () => {
    const api = serving({ "/dashboard": () => example("dashboard"), "/health": () => example("health") });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), options);
    await source.load();
    const agentStream = "agent:ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y:agt_01JB3K8Y4N7QW2M6R9T5V0XZAC";
    expect(await source.notice([{ stream_id: agentStream, seq: 1842 }, { stream_id: "clock:ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y", seq: 99 }])).toBe(false);
    expect(api.reads).toEqual({ "/dashboard": 1, "/health": 1 });
  });

  it.skip("pending E11-9: B's mock server serves the dashboard and health from the fixtures", async () => {
    const mock = createMockServer({ workspaceId: WS, scenario: "normal" });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: mock.fetch }), options);
    const state = await source.load();
    if (state.status !== "ready") throw new Error("expected a ready dashboard");
    expect(state.agents.map((a) => a.label)).toEqual(buildWorkspace("normal").agents.map((a) => a.label));
    expect(state.health.deployment.state).toBe("ok");
  });

  it.skip("pending E11-9: the mock's unreachable scenario renders as unreachable with no agent data", async () => {
    const mock = createMockServer({ workspaceId: WS, scenario: "unreachable" });
    const source = createDashboardSource(createWorkspaceClient({ workspaceId: WS, fetch: mock.fetch }), options);
    const state = await source.load();
    expect(state.status).toBe("unreachable");
    if (state.status !== "unreachable") return;
    expect(state.workspace.status).toBe("unreachable");
    expect(state.workspace.agents).toEqual([]);
  });
});
