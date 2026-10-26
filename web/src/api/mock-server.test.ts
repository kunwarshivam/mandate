import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { closedEnum, integer, object, string, timestamp } from "./decode";
import { MOCK_PASSKEY_ID, type MockServer, createMockServer } from "./mock-server";
import { deployStepUpDigest } from "./passkey";
import { sha256Hex } from "@/lib/sha256";
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

describe("the mock's passkey step-up for a confirm and a deployment (DEC-2128)", () => {
  const MANDATE = { environment: "paper", name: "mock" };

  /** Creates and reads a version, then returns the confirm body's members the screen binds. */
  async function version(server: MockServer) {
    const created = (await (await post(server, "/mandate-versions", MANDATE, "create-key-0000001")).json()) as { mandate_version: `sha256:${string}` };
    const v = created.mandate_version;
    const screen = (await (await server.fetch(`/v1/workspaces/${WS}/mandate-versions/${encodeURIComponent(v)}/confirmation`, { method: "GET" })).json()) as { screen_digest: string };
    return { v, screen_digest: screen.screen_digest };
  }
  async function challenge(server: MockServer, kind: string, digest: string, key: string) {
    return (await (await post(server, "/step-up/challenges", { action: { kind, digest } }, key)).json()) as { challenge_id: string; webauthn_challenge: string; issued_at: string };
  }
  const assertion = (signed: string, type = "webauthn.get", passkey = MOCK_PASSKEY_ID) => ({
    passkey_id: passkey,
    authenticator_data: "AQ",
    signature: "Ag",
    client_data_json: Buffer.from(JSON.stringify({ type, challenge: signed })).toString("base64url"),
  });
  /** §5.1's digest for a new mandate's version, written independently of the client's helper. */
  const confirmDigest = (v: string) => `sha256:${sha256Hex(`{"agent_id":null,"base_version":null,"mandate_version":"${v}"}`)}`;

  it("serves the member's passkeys, references only", async () => {
    const { server } = setup();
    const body = (await (await server.fetch(`/v1/workspaces/${WS}/passkeys`, { method: "GET" })).json()) as { passkeys: Array<Record<string, unknown>> };
    expect(body.passkeys).toEqual([{ passkey_id: MOCK_PASSKEY_ID, alg: "es256", enrolled_at: expect.any(String), cool_off_ends: null }]);
  });

  it("deploys only a confirmed version, under a deploy challenge for exactly that agent and version, once", async () => {
    const { server } = setup();
    const { v } = await version(server);
    const agent = "agt_01JC0000000000000000000009";
    const c = await challenge(server, "deploy", deployStepUpDigest(agent, v), "chal-key-00000001");
    const body = { agent_id: agent, mandate_version: v, backtest_id: null, record: null, step_up: { assertion_id: c.challenge_id, authenticated_at: c.issued_at, method: "passkey" }, step_up_assertion: assertion(c.webauthn_challenge) };
    const unconfirmed = await post(server, "/agents", body, "deploy-key-0000001");
    expect(unconfirmed.status).toBe(422);
    expect(server.deployed()).toEqual([]);
  });

  it("refuses a passkey answer bound to another action, another challenge's bytes, a create rather than a get, or a spent challenge", async () => {
    const { server } = setup();
    const { v, screen_digest } = await version(server);
    const digest = confirmDigest(v);
    const confirm = (step: unknown, signed: unknown, key: string) =>
      post(server, `/mandate-versions/${encodeURIComponent(v)}/confirm`, { agent_id: null, base_version: null, confirmed_paths: [], warnings_acknowledged: [], classification_shown: "risk_increasing", screen_digest, record: null, step_up: step, step_up_assertion: signed }, key);
    const codeOf = async (r: Response) => ((await r.json()) as { code: string }).code;

    expect(await codeOf(await confirm(null, null, "confirm-key-000001"))).toBe("step_up_required");
    const wrongKind = await challenge(server, "deploy", digest, "chal-key-00000010");
    expect(await codeOf(await confirm({ assertion_id: wrongKind.challenge_id, authenticated_at: wrongKind.issued_at, method: "passkey" }, assertion(wrongKind.webauthn_challenge), "confirm-key-000002"))).toBe("step_up_mismatch");
    const right = await challenge(server, "confirm_version", digest, "chal-key-00000011");
    const evidence = { assertion_id: right.challenge_id, authenticated_at: right.issued_at, method: "passkey" };
    expect(await codeOf(await confirm(evidence, assertion(wrongKind.webauthn_challenge), "confirm-key-000003"))).toBe("step_up_missing");
    expect(await codeOf(await confirm(evidence, assertion(right.webauthn_challenge, "webauthn.create"), "confirm-key-000004"))).toBe("step_up_missing");
    expect(await codeOf(await confirm(evidence, assertion(right.webauthn_challenge, "webauthn.get", "c29tZW9uZS1lbHNlcy1rZXk"), "confirm-key-000005"))).toBe("step_up_missing");
    expect(await codeOf(await confirm({ ...evidence, method: "cli_confirm" }, assertion(right.webauthn_challenge), "confirm-key-000006"))).toBe("invalid");
    expect(server.recorded().filter((c) => c.path.endsWith("/confirm"))).toHaveLength(0);
    const accepted = await confirm(evidence, assertion(right.webauthn_challenge), "confirm-key-000007");
    expect(accepted.status).toBe(202);
    expect(await codeOf(await confirm(evidence, assertion(right.webauthn_challenge), "confirm-key-000008"))).toBe("step_up_reused");
    expect((await confirm(evidence, assertion(right.webauthn_challenge), "confirm-key-000007")).status).toBe(202);
    expect(server.recorded().filter((c) => c.path.endsWith("/confirm"))).toHaveLength(1);

    const agent = "agt_01JC0000000000000000000009";
    const forOther = await challenge(server, "deploy", deployStepUpDigest("agt_01JC000000000000000000000A", v), "chal-key-00000012");
    const deploy = (c: typeof forOther, key: string) => post(server, "/agents", { agent_id: agent, mandate_version: v, backtest_id: null, record: null, step_up: { assertion_id: c.challenge_id, authenticated_at: c.issued_at, method: "passkey" }, step_up_assertion: assertion(c.webauthn_challenge) }, key);
    expect(await codeOf(await deploy(forOther, "deploy-key-0000002"))).toBe("step_up_mismatch");
    const mine = await challenge(server, "deploy", deployStepUpDigest(agent, v), "chal-key-00000013");
    expect((await deploy(mine, "deploy-key-0000003")).status).toBe(202);
    expect(server.deployed()).toEqual([agent]);
    const again = await challenge(server, "deploy", deployStepUpDigest(agent, v), "chal-key-00000014");
    expect((await deploy(again, "deploy-key-0000004")).status).toBe(422);
  });

  it("asks nothing of a passkey still in its cool-off: a confirm under it is refused", async () => {
    const server = createMockServer({ workspaceId: WS, passkeys: [{ passkey_id: MOCK_PASSKEY_ID, cool_off_ends: "2999-01-01T00:00:00.000000000Z" }] });
    const { v, screen_digest } = await version(server);
    const digest = confirmDigest(v);
    const c = await challenge(server, "confirm_version", digest, "chal-key-00000020");
    const r = await post(server, `/mandate-versions/${encodeURIComponent(v)}/confirm`, { screen_digest, step_up: { assertion_id: c.challenge_id, authenticated_at: c.issued_at, method: "passkey" }, step_up_assertion: assertion(c.webauthn_challenge) }, "confirm-key-000020");
    expect(((await r.json()) as { code: string }).code).toBe("step_up_missing");
  });
});

describe("the mock's passkey step-up for a resume and a Stop (DEC-2140)", () => {
  async function challenge(server: MockServer, kind: string, digest: string, key: string) {
    return (await (await post(server, "/step-up/challenges", { action: { kind, digest } }, key)).json()) as { challenge_id: string; webauthn_challenge: string; issued_at: string };
  }
  const assertion = (signed: string) => ({
    passkey_id: MOCK_PASSKEY_ID,
    authenticator_data: "AQ",
    signature: "Ag",
    client_data_json: Buffer.from(JSON.stringify({ type: "webauthn.get", challenge: signed })).toString("base64url"),
  });
  /** The server's bound object for `confirms` at `head`, written independently of the client's helper. */
  const bound = (agent: string, confirms: string, head: number) =>
    `sha256:${sha256Hex(`{"agent":"${agent}","bid":null,"bid_size":null,"confirms":"${confirms}","control_head":"${head}","event":null,"floor":null,"instrument":null}`)}`;
  async function status(server: MockServer, eventId: string) {
    let last: { phase: string; steps: Array<{ event_type: string; reason: string | null }> } = { phase: "", steps: [] };
    for (let read = 0; read < 3; read += 1) last = await (await server.fetch(`/v1/workspaces/${WS}/commands/${eventId}`, { method: "GET" })).json();
    return last;
  }

  it("serves the control stream's head, which each recorded command moves on, as the server's does", async () => {
    const { server } = setup();
    const head = server.controlHead();
    const read = async () => ((await (await server.fetch(`/v1/workspaces/${WS}/journal/streams/${encodeURIComponent(`ctl:${WS}`)}/events?after_seq=0&limit=1`, { method: "GET" })).json()) as { head: { seq: number } }).head.seq;
    expect(await read()).toBe(head);
    await post(server, "/agents/agt_01/pause", { record: null }, "head-key-00000001");
    expect(server.controlHead()).toBe(head + 1);
    expect(await read()).toBe(head + 1);
  });

  it("records a resume whose passkey verifies with its evidence, and the runtime applies it", async () => {
    const { server } = setup();
    const agent = "agt_01";
    const issued = await challenge(server, "resume", bound(agent, "resume", server.controlHead()), "resume-chal-00001");
    const step_up = { assertion_id: issued.challenge_id, authenticated_at: issued.issued_at, method: "passkey" };
    const answer = await post(server, `/agents/${agent}/resume`, { record: null, step_up, step_up_assertion: assertion(issued.webauthn_challenge) }, "resume-key-000001");
    expect(answer.status).toBe(202);
    const [recorded] = server.recorded();
    expect(recorded.step_up).toEqual(step_up);
    expect(server.challenges()[0].used).toBe(true);
    expect((await status(server, recorded.event_id)).phase).toBe("applied");
  });

  it("records a Stop whose passkey does not verify with null evidence, still 202, and the runtime refuses it step_up_missing", async () => {
    const { server } = setup();
    const agent = "agt_01";
    const issued = await challenge(server, "stop", bound(agent, "stop", server.controlHead() + 1), "stop-chal-0000001");
    const step_up = { assertion_id: issued.challenge_id, authenticated_at: issued.issued_at, method: "passkey" };
    const answer = await post(server, `/agents/${agent}/stop`, { release: false, warning_shown: null, record: null, step_up, step_up_assertion: assertion(issued.webauthn_challenge) }, "stop-key-00000001");
    expect(answer.status).toBe(202);
    const [recorded] = server.recorded();
    expect(recorded.step_up).toBeNull();
    expect(server.challenges()[0].used).toBe(false);
    const settled = await status(server, recorded.event_id);
    expect(settled).toMatchObject({ phase: "refused", steps: [{ event_type: "OwnerCommandRefused", reason: "step_up_missing" }] });
  });

  it("refuses an assertion beside evidence that is not a passkey's, recording nothing", async () => {
    const { server } = setup();
    const issued = await challenge(server, "resume", bound("agt_01", "resume", server.controlHead()), "resume-chal-00002");
    const answer = await post(server, "/agents/agt_01/resume", { record: null, step_up: null, step_up_assertion: assertion(issued.webauthn_challenge) }, "resume-key-000002");
    expect(answer.status).toBe(422);
    expect(server.recorded()).toHaveLength(0);
  });
});
