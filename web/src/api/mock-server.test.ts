import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { closedEnum, integer, object, string, timestamp } from "./decode";
import { createMockServer } from "./mock-server";
import type { Decoder, KillSwitchRequest } from "./types";
import type { Scenario } from "@/fixtures/types";

const WS = "ws_01J9ZQ4M0000000000000000AB";

interface Freshness {
  observed_at: string;
  stale: boolean;
  age_seconds: number;
  limit_source: string;
}
interface Component {
  state: "ok" | "stale" | "down";
  freshness: Freshness;
}
interface Health {
  served_at: string;
  market_data: Component;
  broker: Component;
  deployment: Component;
  relay: Component;
}

const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : { ok: false, issue: { path, problem: "wrong_type", value: null, allowed: null } });
/** `read-models/health.schema.json` and `common.schema.json`'s Freshness, on A's S0a branch. */
const freshness = object<Freshness>({ observed_at: timestamp, stale: flag, age_seconds: integer, limit_source: string });
const component = object<Component>({ state: closedEnum(["ok", "stale", "down"]), freshness });
const health = object<Health>({ served_at: timestamp, market_data: component, broker: component, deployment: component, relay: component });

const kill: KillSwitchRequest = { scope: { kind: "workspace", id: null }, environment_shown: "paper", owner_exit: null, record: null, step_up: null };

let keys = 0;
function setup(scenario: Scenario = "normal") {
  const server = createMockServer({ workspaceId: WS, scenario });
  const sent: Headers[] = [];
  const fetch: typeof server.fetch = (input, init) => {
    sent.push(new Headers(init.headers));
    return server.fetch(input, init);
  };
  const api = createWorkspaceClient({ workspaceId: WS, fetch, newKey: () => `mock-key-${String(++keys).padStart(8, "0")}` });
  return { server, api, sent };
}

function post(server: ReturnType<typeof setup>["server"], path: string, body: unknown, key: string | null, csrf = true) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (key) headers["Idempotency-Key"] = key;
  if (csrf) headers["X-Mandate-Request"] = "1";
  return server.fetch(`/v1/workspaces/${WS}${path}`, { method: "POST", headers, body: JSON.stringify(body) });
}

describe("the fixture-backed mock server", () => {
  it("pending E11-9: serves health from the fixtures with canonical timestamps, freshness and watermarks", async () => {
    const { api } = setup();
    const outcome = await api.read("/health", health);
    expect(outcome.ok).toBe(true);
    if (!outcome.ok) return;
    expect(outcome.as_of.length).toBeGreaterThan(0);
    expect(outcome.value.served_at).toBe("2026-09-28T18:05:20.000000000Z");
    expect(outcome.value.market_data).toEqual({
      state: "ok",
      freshness: { observed_at: "2026-09-28T18:05:18.000000000Z", stale: false, age_seconds: 2, limit_source: "health.market_data" },
    });
    expect(outcome.value.relay.freshness).toEqual({ observed_at: "2026-09-28T18:05:10.000000000Z", stale: false, age_seconds: 10, limit_source: "health.relay" });
  });

  it("pending E11-9: marks a stale component stale in its freshness", async () => {
    const { api } = setup("stale");
    const outcome = await api.read("/health", health);
    expect(outcome.ok).toBe(true);
    if (!outcome.ok) return;
    const components = [outcome.value.market_data, outcome.value.broker, outcome.value.deployment, outcome.value.relay];
    expect(components.some((c) => c.state === "stale")).toBe(true);
    for (const c of components) {
      if (c.state === "ok") expect(c.freshness.stale).toBe(false);
      if (c.state === "stale") expect(c.freshness.stale).toBe(true);
    }
  });

  it("pending E11-9: records a kill switch and reports it through command status", async () => {
    const { server, api } = setup();
    const sent = await api.send(api.prepareKillSwitch(kill));
    expect(sent).toMatchObject({ ok: true, effect: "recorded", accepted: { phase: "recorded", step_up_status: "missing" } });
    if (!sent.ok) return;
    expect(server.recorded()).toHaveLength(1);
    expect(server.recorded()[0].event_id).toBe(sent.accepted.event_id);
    expect(await api.commandStatus(sent.accepted.event_id)).toMatchObject({ ok: true, value: { phase: "recorded", steps: [] } });
  });

  it("pending E11-9: resolves a repeated key to the first event and refuses a changed body", async () => {
    const { server } = setup();
    const key = "repeat-key-000000001";
    const first = await (await post(server, "/kill-switch", kill, key)).json();
    const again = await (await post(server, "/kill-switch", kill, key)).json();
    expect(again.command_id).toBe(first.command_id);
    expect(server.recorded()).toHaveLength(1);
    const changed = await post(server, "/kill-switch", { ...kill, environment_shown: "live" }, key);
    expect(changed.status).toBe(409);
    expect(await changed.json()).toMatchObject({ code: "idempotency_conflict", effect: "none" });
  });

  it("pending E11-9: refuses a command without an Idempotency-Key, recording nothing", async () => {
    const { server } = setup();
    const answer = await post(server, "/kill-switch", kill, null);
    expect(answer.status).toBe(422);
    expect(await answer.json()).toMatchObject({ code: "invalid", effect: "none" });
    expect(server.recorded()).toHaveLength(0);
  });

  it("pending E11-9: refuses a command without X-Mandate-Request: 1 as forbidden, recording nothing (spec §3.3)", async () => {
    const { server, api } = setup();
    for (const answer of [await post(server, "/kill-switch", kill, "csrf-key-0000000001", false), await post(server, "/agents/agt_01/pause", { record: null }, "csrf-key-0000000002", false)]) {
      expect(answer.status).toBe(403);
      expect(await answer.json()).toMatchObject({ code: "forbidden", effect: "none", retryable: false });
    }
    expect(server.recorded()).toHaveLength(0);
    expect(await api.send(api.prepareKillSwitch(kill))).toMatchObject({ ok: true, effect: "recorded" });
  });

  it("pending E11-9: answers 404 for an unknown route, another workspace, or any order route", async () => {
    const { server } = setup();
    for (const url of [`/v1/workspaces/${WS}/nothing-here`, "/v1/workspaces/ws_other/health"]) {
      const answer = await server.fetch(url, { method: "GET" });
      expect(answer.status, url).toBe(404);
      expect(await answer.json()).toMatchObject({ code: "not_found", effect: "none" });
    }
    const order = await post(server, "/orders", { side: "buy" }, "order-key-000000001");
    expect(order.status).toBe(404);
    expect(server.recorded()).toHaveLength(0);
  });

  it("pending E11-9: plays unreachable as a network that never answers", async () => {
    const { api } = setup("unreachable");
    expect(await api.read("/health", health)).toMatchObject({ ok: false, error: { code: "network", effect: "none" } });
    expect(await api.send(api.prepareKillSwitch(kill))).toMatchObject({ ok: false, error: { code: "network", effect: "unknown" } });
  });

  it("pending E11-9: plays result-unknown as commands recorded and never answered, found by a later status poll", async () => {
    const { server, api } = setup("result-unknown");
    expect((await api.read("/health", health)).ok).toBe(true);
    expect(await api.send(api.prepare("pause", { agent: "agt_01" }, { record: null }))).toMatchObject({ ok: false, error: { effect: "unknown" } });
    expect(server.recorded()).toHaveLength(1);
    expect(await api.commandStatus(server.recorded()[0].event_id)).toMatchObject({ ok: true, value: { phase: "recorded" } });
  });

  it("pending E11-9: treats the same key on another route as another command", async () => {
    const { server } = setup();
    const key = "shared-key-000000001";
    const pause = await (await post(server, "/agents/agt_01/pause", { record: null }, key)).json();
    const resume = await (await post(server, "/agents/agt_01/resume", { record: null }, key)).json();
    expect(server.recorded()).toHaveLength(2);
    expect(resume.command_id).not.toBe(pause.command_id);
  });

  it("pending E11-9: answers 404 for the status of an event it never recorded", async () => {
    const { api } = setup();
    expect(await api.commandStatus("01J9ZQ4M5KQ3W8X2Y7V6T5R4S3")).toMatchObject({ ok: false, error: { code: "not_found", effect: "none", status: 404 } });
  });

  it("pending E11-9: is reached with no bearer token", async () => {
    const { api, sent } = setup();
    await api.read("/health", health);
    await api.send(api.prepareKillSwitch(kill));
    expect(sent).toHaveLength(2);
    expect(sent.every((h) => h.get("Authorization") === null)).toBe(true);
  });
});
