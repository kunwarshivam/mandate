import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { Agent, Environment } from "@/fixtures/types";
import { AGENT_IDS, SCENARIOS, buildWorkspace } from "@/fixtures/workspace";
import { headroomLine } from "@/lib/limits";
import { createWorkspaceClient, type Fetch } from "./client";
import {
  assembleAgent,
  createAgentSource,
  decodeAgent,
  decodeMandateVersion,
  decodeOrders,
  decodePnl,
  decodePositions,
  type AgentReads,
} from "./agent";
import { createMockServer } from "./mock-server";

const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const SERVED = "2026-09-28T18:05:20.000000000Z";
const HASH = `sha256:${"0f".repeat(32)}` as const;
const AGENT_STREAM = (id: string) => `agent:${WS}:${id}`;
const ACCOUNT_STREAM = `acct:${WS}:acc_01JB3K7N2P4R6T8V0X2Z4B6D8F`;
const options = { now: () => SERVED, environment: "paper" as const };

function example(name: string): Record<string, unknown> {
  const examples = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "schemas", "workspace-api", "examples");
  return JSON.parse(readFileSync(join(examples, `read-models.${name}.json`), "utf8")) as Record<string, unknown>;
}

const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

/** A fixture time as a journal spec §4.7 timestamp. */
const canonical = (iso: string) => `${new Date(iso).toISOString().slice(0, 23)}000000Z`;

/** Every fixture time in a value, canonical: what the API serves for the same fact. */
function canonicalTimes<T>(value: T): T {
  if (typeof value === "string") return (ISO.test(value) ? canonical(value) : value) as T;
  if (Array.isArray(value)) return value.map(canonicalTimes) as T;
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, canonicalTimes(v)])) as T;
  return value;
}

const fresh = (observed_at: string, limit_source: string) => ({ observed_at: canonical(observed_at), stale: false, age_seconds: 0, limit_source });
const watermark = (stream_id: string, seq: number) => ({ stream_id, seq, hash: HASH, recorded_at: SERVED });
const envelope = (...as_of: ReturnType<typeof watermark>[]) => ({ api_version: "v1", build: HASH, served_at: SERVED, as_of });

/** The five read-model bodies for a fixture agent, written independently of the adapter: the test's oracle. */
function bodiesOf(agent: Agent, environment: Environment) {
  const scoped = { agent_id: agent.agent_id, environment, simulated: environment === "paper" };
  const a = canonicalTimes(agent);
  return {
    agent: {
      ...envelope(watermark(AGENT_STREAM(agent.agent_id), 100), watermark(ACCOUNT_STREAM, 50)),
      ...scoped,
      label: a.label,
      mandate_version: a.mandate_version,
      mode: a.mode,
      restrictions: a.restrictions.map((r) => ({ code: r.code, since: r.since, symbol: r.symbol ?? null })),
      startup: a.startup,
      deployed_at: a.deployed_at,
      state: { ...a.state, freshness: fresh(SERVED, "executor.head") },
      goal_progress: a.goal_progress,
      versions: a.versions.map((v) => {
        const { result, ...rest } = v.application;
        return { ...v, application: { kind: result, ...rest } };
      }),
    },
    positions: {
      ...envelope(watermark(ACCOUNT_STREAM, 50)),
      ...scoped,
      positions: a.positions.map(({ mark_as_of, ...p }) => ({
        ...p,
        broker_freshness: fresh(SERVED, "account_snapshot.max_age_s"),
        mark_freshness: fresh(mark_as_of, "data_profile.max_mark_age_s"),
        protection: { ...p.protection, unprotected_since: null as string | null },
      })),
    },
    orders: { ...envelope(watermark(ACCOUNT_STREAM, 50)), ...scoped, orders: a.orders, past_orders: a.past_orders, fills: a.fills },
    pnl: {
      ...envelope(watermark(ACCOUNT_STREAM, 50)),
      ...scoped,
      pnl_total: a.pnl_total,
      pnl_today: a.pnl_today,
      realized_pnl: a.realized_pnl,
      unrealized_pnl: "0",
      marks_freshness: null,
      disclosures: [],
    },
    version: { ...envelope(watermark(`ctl:${WS}`, 9)), mandate_version: a.mandate_version, mandate: agent.mandate, provenance: agent.provenance },
  };
}

function decodeAll(bodies: ReturnType<typeof bodiesOf>): AgentReads {
  const decoded = {
    agent: decodeAgent(bodies.agent, ""),
    positions: decodePositions(bodies.positions, ""),
    orders: decodeOrders(bodies.orders, ""),
    pnl: decodePnl(bodies.pnl, ""),
    version: decodeMandateVersion(bodies.version, ""),
  };
  for (const [name, d] of Object.entries(decoded)) if (!d.ok) throw new Error(`${name} did not decode: ${JSON.stringify(d.issue)}`);
  return Object.fromEntries(Object.entries(decoded).map(([k, d]) => [k, (d as { ok: true; value: unknown }).value])) as unknown as AgentReads;
}

function fixtureAgent(id: string, scenario: Parameters<typeof buildWorkspace>[0] = "normal"): Agent {
  const agent = buildWorkspace(scenario).agents.find((a) => a.agent_id === id);
  if (!agent) throw new Error(`no fixture agent ${id}`);
  return agent;
}

/** A fetch answering each route under the workspace from a body, counting reads per route. */
function serving(routes: Record<string, () => unknown>) {
  const reads: Record<string, number> = {};
  const fetch: Fetch = async (input) => {
    const route = new URL(input, "http://api.invalid").pathname.replace(`/v1/workspaces/${WS}`, "");
    reads[route] = (reads[route] ?? 0) + 1;
    const answer = routes[route];
    if (!answer) return new Response(JSON.stringify({ code: "not_found", effect: "none" }), { status: 404 });
    const body = answer();
    if (body instanceof Error) throw body;
    return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
  };
  return { fetch, reads };
}

function routesFor(agent: Agent, bodies = bodiesOf(agent, "paper")) {
  return {
    [`/agents/${agent.agent_id}`]: () => bodies.agent,
    [`/agents/${agent.agent_id}/positions`]: () => bodies.positions,
    [`/agents/${agent.agent_id}/orders`]: () => bodies.orders,
    [`/agents/${agent.agent_id}/pnl`]: () => bodies.pnl,
    [`/mandate-versions/${agent.mandate_version}`]: () => bodies.version,
  };
}

describe("decoding the agent read models (§4.10)", () => {
  it.skip("pending E11-9: the schemas' own examples decode", () => {
    expect(decodeAgent(example("agent"), "").ok).toBe(true);
    expect(decodePositions(example("positions"), "").ok).toBe(true);
    expect(decodeOrders(example("orders"), "").ok).toBe(true);
    expect(decodePnl(example("pnl"), "").ok).toBe(true);
  });

  it.skip("pending E11-9: an order state outside the closed set is refused, never mapped to a known one", () => {
    const body = example("orders");
    const orders = structuredClone(body.orders) as Array<Record<string, unknown>>;
    orders[0].state = "Pending";
    expect(decodeOrders({ ...body, orders }, "")).toMatchObject({ ok: false, issue: { path: "/orders/0/state", problem: "unknown_enum_value", value: "Pending" } });
  });

  it.skip("pending E11-9: a final state among the working orders is refused", () => {
    const body = example("orders");
    const orders = structuredClone(body.orders) as Array<Record<string, unknown>>;
    orders[0].state = "Filled";
    expect(decodeOrders({ ...body, orders }, "").ok).toBe(false);
  });

  it.skip("pending E11-9: a quantity as a JSON number is refused (§3.1)", () => {
    const body = example("positions");
    const positions = structuredClone(body.positions) as Array<Record<string, unknown>>;
    positions[0].broker_qty = 0.12;
    expect(decodePositions({ ...body, positions }, "")).toMatchObject({ ok: false, issue: { path: "/positions/0/broker_qty", problem: "decimal_number" } });
  });

  it.skip("pending E11-9: a version application of an unknown kind is refused", () => {
    const body = example("agent");
    const versions = structuredClone(body.versions) as Array<Record<string, unknown>>;
    versions[0].application = { kind: "superseded" };
    expect(decodeAgent({ ...body, versions }, "").ok).toBe(false);
  });

  it.skip("pending E11-9: an ok limit state still carries its freshness", () => {
    const body = example("agent");
    const state = structuredClone(body.state) as Record<string, unknown>;
    delete state.freshness;
    expect(decodeAgent({ ...body, state }, "")).toMatchObject({ ok: false, issue: { path: "/state/freshness", problem: "missing" } });
  });
});

describe("assembling the existing Agent from the five reads (§4.9)", () => {
  it.skip("pending E11-9: every fixture agent in every scenario round-trips to the same Agent, so no screen contract changes", () => {
    const agents = SCENARIOS.flatMap((s) => buildWorkspace(s.id).agents);
    expect(agents.some((a) => a.restrictions.some((r) => r.symbol === undefined))).toBe(true);
    expect(agents.some((a) => a.restrictions.some((r) => r.symbol !== undefined))).toBe(true);
    for (const agent of agents) {
      const assembled = assembleAgent(decodeAll(bodiesOf(agent, "paper")));
      expect(assembled.ok).toBe(true);
      if (!assembled.ok) continue;
      expect(assembled.agent).toEqual(canonicalTimes(agent));
    }
  });

  it.skip("pending E11-9: headroom reads the same from the API as from the fixtures", () => {
    for (const agent of buildWorkspace("drawdown").agents) {
      const assembled = assembleAgent(decodeAll(bodiesOf(agent, "paper")));
      if (!assembled.ok) throw new Error("expected an agent");
      expect(headroomLine(assembled.agent)).toBe(headroomLine(agent));
    }
  });

  it.skip("pending E11-9: reads that name different agents are refused, never merged", () => {
    const swing = bodiesOf(fixtureAgent(AGENT_IDS.swing), "paper");
    const btc = bodiesOf(fixtureAgent(AGENT_IDS.btc), "paper");
    expect(assembleAgent(decodeAll({ ...swing, positions: btc.positions }))).toEqual({ ok: false, reason: "agent_mismatch" });
  });

  it.skip("pending E11-9: a mandate version other than the agent's is refused", () => {
    const swing = bodiesOf(fixtureAgent(AGENT_IDS.swing), "paper");
    const btc = bodiesOf(fixtureAgent(AGENT_IDS.btc), "paper");
    expect(assembleAgent(decodeAll({ ...swing, version: btc.version }))).toEqual({ ok: false, reason: "version_mismatch" });
  });

  it.skip("pending E11-9: reads that disagree on the environment are refused", () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const paper = bodiesOf(agent, "paper");
    const live = bodiesOf(agent, "live");
    expect(assembleAgent(decodeAll({ ...paper, pnl: live.pnl }))).toEqual({ ok: false, reason: "environment_mismatch" });
  });
});

describe("what Agent has no member for: freshness, intervals, and disagreement (§6.1)", () => {
  function details(edit: (b: ReturnType<typeof bodiesOf>) => void, agent = fixtureAgent(AGENT_IDS.swing)) {
    const bodies = bodiesOf(agent, "paper");
    edit(bodies);
    const assembled = assembleAgent(decodeAll(bodies));
    if (!assembled.ok) throw new Error(`expected an agent, got ${assembled.reason}`);
    return assembled;
  }

  it.skip("pending E11-9: an Unknown order stays Unknown and is listed as unknown, never guessed", () => {
    const agent = fixtureAgent(AGENT_IDS.swing, "unknown-order");
    const unknown = agent.orders.filter((o) => o.state === "Unknown").map((o) => o.client_order_id);
    expect(unknown.length).toBeGreaterThan(0);
    const assembled = details(() => {}, agent);
    expect(assembled.details.unknown_orders).toEqual(unknown);
    for (const id of unknown) expect(assembled.agent.orders.find((o) => o.client_order_id === id)?.state).toBe("Unknown");
  });

  it.skip("pending E11-9: no Unknown order means none listed", () => {
    expect(details(() => {}).details.unknown_orders).toEqual([]);
  });

  it.skip("pending E11-9: a stale risk mark is shown stale with its age to served_at", () => {
    const assembled = details((b) => {
      b.positions.positions[0].mark_freshness = { observed_at: "2026-09-28T18:02:11.000000000Z", stale: true, age_seconds: 189, limit_source: "data_profile.max_mark_age_s" };
    });
    expect(assembled.details.positions[0].mark).toEqual({ as_of: "2026-09-28T18:02:11.000000000Z", stale: true, age: "3 min" });
    expect(assembled.agent.positions[0].mark_as_of).toBe("2026-09-28T18:02:11.000000000Z");
  });

  it.skip("pending E11-9: ledger and broker quantities that disagree are both kept and flagged", () => {
    const assembled = details((b) => {
      b.positions.positions[0].broker_qty = "1.5";
      b.positions.positions[0].qty = "2";
    });
    expect(assembled.agent.positions[0]).toMatchObject({ qty: "2", broker_qty: "1.5" });
    expect(assembled.details.positions[0].quantity_mismatch).toBe(true);
    expect(assembled.details.positions.slice(1).every((p) => !p.quantity_mismatch)).toBe(true);
  });

  it.skip("pending E11-9: a stale broker snapshot is shown stale, not as the broker's current quantity", () => {
    const assembled = details((b) => {
      b.positions.positions[0].broker_freshness = { observed_at: "2026-09-28T17:55:20.000000000Z", stale: true, age_seconds: 600, limit_source: "account_snapshot.max_age_s" };
    });
    expect(assembled.details.positions[0].broker).toEqual({ as_of: "2026-09-28T17:55:20.000000000Z", stale: true, age: "10 min" });
  });

  it.skip("pending E11-9: an unprotected interval is measured from its start to served_at", () => {
    const assembled = details((b) => {
      b.positions.positions[0].protection = { ...b.positions.positions[0].protection, unprotected_fraction: "1", unprotected_since: "2026-09-28T18:04:50.000000000Z" };
    });
    expect(assembled.details.positions[0].unprotected_seconds).toBe(30);
    expect(assembled.agent.positions[0].protection.unprotected_fraction).toBe("1");
    expect(assembled.agent.positions[0].protection).not.toHaveProperty("unprotected_since");
  });

  it.skip("pending E11-9: a protected position has no unprotected interval", () => {
    expect(details(() => {}).details.positions.every((p) => p.unprotected_seconds === null)).toBe(true);
  });

  it.skip("pending E11-9: stale limit state is shown stale, so headroom is never read as current", () => {
    const assembled = details((b) => {
      b.agent.state.freshness = { observed_at: "2026-09-28T18:00:20.000000000Z", stale: true, age_seconds: 300, limit_source: "executor.head" };
    });
    expect(assembled.details.limits).toEqual({ as_of: "2026-09-28T18:00:20.000000000Z", stale: true, age: "5 min" });
  });

  it.skip("pending E11-9: paper P&L is labelled simulated", () => {
    const assembled = details(() => {});
    expect(assembled.details.pnl_label).toBe("Simulated");
    expect(assembled.details.served_at).toBe(SERVED);
  });
});

describe("loading, failing, and refetching one agent (G4, §3.1, §6.3, §7)", () => {
  it.skip("pending E11-9: a load reads the five views and keeps each view's own as_of", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const api = serving(routesFor(agent));
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    const state = await source.load();
    if (state.status !== "ready") throw new Error("expected a ready agent");
    expect(state.agent).toEqual(canonicalTimes(agent));
    expect(state.as_of.version).toEqual([watermark(`ctl:${WS}`, 9)]);
    expect(state.as_of.positions).toEqual([watermark(ACCOUNT_STREAM, 50)]);
  });

  it.skip("pending E11-9: any one read failing makes the agent unreachable, with nothing from an earlier load", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    let failing: string | null = null;
    const routes = Object.fromEntries(
      Object.entries(routesFor(agent)).map(([route, answer]) => [route, () => (route === failing ? new TypeError("Failed to fetch") : answer())]),
    );
    const api = serving(routes);
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    for (const route of Object.keys(routes)) {
      failing = null;
      expect((await source.load()).status).toBe("ready");
      failing = route;
      const state = await source.load();
      expect(state.status).toBe("unreachable");
      expect(JSON.stringify(source.state())).not.toContain(agent.label);
      expect(JSON.stringify(source.state())).not.toContain(agent.pnl_total);
    }
  });

  it.skip("pending E11-9: reads that disagree make the agent unreachable rather than half shown", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const other = bodiesOf(fixtureAgent(AGENT_IDS.btc), "paper");
    const api = serving({ ...routesFor(agent), [`/agents/${agent.agent_id}/orders`]: () => other.orders });
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    expect((await source.load()).status).toBe("unreachable");
  });

  it.skip("pending E11-9: an account-stream notice refetches the views that read it, never the mandate version", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const api = serving(routesFor(agent));
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    await source.load();
    expect(await source.notice([{ stream_id: ACCOUNT_STREAM, seq: 51 }])).toBe(true);
    const id = agent.agent_id;
    expect(api.reads).toEqual({
      [`/agents/${id}`]: 2,
      [`/agents/${id}/positions`]: 2,
      [`/agents/${id}/orders`]: 2,
      [`/agents/${id}/pnl`]: 2,
      [`/mandate-versions/${agent.mandate_version}`]: 1,
    });
  });

  it.skip("pending E11-9: an agent-stream notice refetches only the agent view", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const api = serving(routesFor(agent));
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    await source.load();
    expect(await source.notice([{ stream_id: AGENT_STREAM(agent.agent_id), seq: 101 }])).toBe(true);
    expect(api.reads[`/agents/${agent.agent_id}`]).toBe(2);
    expect(api.reads[`/agents/${agent.agent_id}/positions`]).toBe(1);
    expect(await source.notice([{ stream_id: AGENT_STREAM(agent.agent_id), seq: 100 }])).toBe(false);
  });

  it.skip("pending E11-9: a newly applied version is read once, by its new hash", async () => {
    const agent = fixtureAgent(AGENT_IDS.swing);
    const bodies = bodiesOf(agent, "paper");
    const next = bodiesOf({ ...agent, mandate_version: `sha256:${"ab".repeat(32)}` }, "paper");
    let applied = false;
    const api = serving({
      ...routesFor(agent, bodies),
      [`/agents/${agent.agent_id}`]: () => (applied ? { ...next.agent, as_of: [watermark(AGENT_STREAM(agent.agent_id), 101), watermark(ACCOUNT_STREAM, 50)] } : bodies.agent),
      [`/mandate-versions/sha256:${"ab".repeat(32)}`]: () => next.version,
    });
    const source = createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: api.fetch }), agent.agent_id, options);
    await source.load();
    applied = true;
    await source.notice([{ stream_id: AGENT_STREAM(agent.agent_id), seq: 101 }]);
    const state = source.state();
    if (state?.status !== "ready") throw new Error("expected a ready agent");
    expect(state.agent.mandate_version).toBe(`sha256:${"ab".repeat(32)}`);
    expect(api.reads[`/mandate-versions/sha256:${"ab".repeat(32)}`]).toBe(1);
  });

  it.skip("pending E11-9: B's mock server serves every fixture agent, and each round-trips", async () => {
    const mock = createMockServer({ workspaceId: WS, scenario: "normal" });
    const client = createWorkspaceClient({ workspaceId: WS, fetch: mock.fetch });
    for (const agent of buildWorkspace("normal").agents) {
      const state = await createAgentSource(client, agent.agent_id, options).load();
      if (state.status !== "ready") throw new Error(`expected ${agent.label} ready`);
      expect(state.agent).toEqual(canonicalTimes(agent));
    }
  });

  it.skip("pending E11-9: the mock's unknown-order scenario keeps the Unknown order unknown", async () => {
    const mock = createMockServer({ workspaceId: WS, scenario: "unknown-order" });
    const client = createWorkspaceClient({ workspaceId: WS, fetch: mock.fetch });
    const states = await Promise.all(buildWorkspace("unknown-order").agents.map((a) => createAgentSource(client, a.agent_id, options).load()));
    const unknown = states.flatMap((s) => (s.status === "ready" ? s.details.unknown_orders : []));
    expect(unknown.length).toBeGreaterThan(0);
  });

  it.skip("pending E11-9: the mock's unreachable scenario is unreachable with no agent data", async () => {
    const mock = createMockServer({ workspaceId: WS, scenario: "unreachable" });
    const state = await createAgentSource(createWorkspaceClient({ workspaceId: WS, fetch: mock.fetch }), AGENT_IDS.swing, options).load();
    expect(state.status).toBe("unreachable");
    if (state.status === "unreachable") expect(state.workspace.agents).toEqual([]);
  });
});
