import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { closedEnum, object, timestamp } from "./decode";
import { createMockServer } from "./mock-server";
import type { KillSwitchRequest } from "./types";
import type { Scenario } from "@/fixtures/types";

const WS = "ws_01J9ZQ4M0000000000000000AB";

const feed = object<{ state: "ok" | "stale" | "down"; as_of: string }>({ state: closedEnum(["ok", "stale", "down"]), as_of: timestamp });
const health = object<{ market_data: { state: string; as_of: string }; broker: { state: string; as_of: string } }>({ market_data: feed, broker: feed });

const kill: KillSwitchRequest = { scope: { kind: "workspace", id: null }, environment_shown: "paper", owner_exit: null, record: null, step_up: null };

let keys = 0;
function setup(scenario: Scenario = "normal") {
  const server = createMockServer({ workspaceId: WS, scenario });
  const api = createWorkspaceClient({ workspaceId: WS, fetch: server.fetch, newKey: () => `mock-key-${String(++keys).padStart(8, "0")}` });
  return { server, api };
}

function post(server: ReturnType<typeof setup>["server"], path: string, body: unknown, key: string | null) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (key) headers["Idempotency-Key"] = key;
  return server.fetch(`/v1/workspaces/${WS}${path}`, { method: "POST", headers, body: JSON.stringify(body) });
}

describe("the fixture-backed mock server", () => {
  it("pending E11-9: serves health from the fixtures with canonical timestamps and watermarks", async () => {
    const { api } = setup();
    const outcome = await api.read("/health", health);
    expect(outcome.ok).toBe(true);
    if (!outcome.ok) return;
    expect(outcome.as_of.length).toBeGreaterThan(0);
    expect(outcome.value.market_data.as_of).toBe("2026-09-28T18:05:18.000000000Z");
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

  it("pending E11-9: plays result-unknown as commands taken and never answered", async () => {
    const { server, api } = setup("result-unknown");
    expect((await api.read("/health", health)).ok).toBe(true);
    expect(await api.send(api.prepare("pause", { agent: "agt_01" }, { record: null }))).toMatchObject({ ok: false, error: { effect: "unknown" } });
    expect(server.recorded()).toHaveLength(0);
  });
});
