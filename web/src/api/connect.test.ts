import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { API_CONNECTION_ID, connectStartBody, connectStarted, connectionDetail } from "./connect";
import { MOCK_AUTHORIZATION_ORIGIN, createMockServer } from "./mock-server";
import { loadApi } from "./read-models";
import { CONNECT_DISCLOSURE, type StepUpChallenge, connectStepUpDigest } from "./step-up";

/**
 * The Alpaca paper connect's start from the browser (§4.5; DEC-2621, DEC-2623, DEC-2125), against the
 * contract-faithful mock: bound to this workspace and the disclosure shown, retried only under its own
 * key, and never answered as "nothing happened" when its answer is lost.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";

function setUp(options: { hostConfirms?: boolean; lose?: number } = {}) {
  const server = createMockServer({ workspaceId: WS, scenario: "normal", hostConfirms: options.hostConfirms ?? true });
  let lose = options.lose ?? 0;
  const posts: Array<{ path: string; key: string | null; body: string }> = [];
  const fetch: Fetch = async (input, init) => {
    const answer = await server.fetch(input, init);
    if (init.method === "POST") {
      posts.push({ path: new URL(input).pathname, key: new Headers(init.headers).get("Idempotency-Key"), body: String(init.body) });
      if (new URL(input).pathname.endsWith("/connections/oauth/start") && lose > 0) {
        lose -= 1;
        throw new TypeError("The deployment recorded the start and the answer was lost");
      }
    }
    return answer;
  };
  return { server, posts, client: createWorkspaceClient({ workspaceId: WS, fetch, baseUrl: "https://api.owlhead.ai" }) };
}

async function challengeIn(client: ReturnType<typeof createWorkspaceClient>, kind: "connection" | "approve" = "connection"): Promise<StepUpChallenge> {
  const answer = await client.askChallenge({ kind, digest: connectStepUpDigest(WS) });
  if (!answer.ok) throw new Error(answer.error.code);
  return answer.value;
}

describe("a connect's start (DEC-2623)", () => {
  it("sends Alpaca, paper, the disclosure version shown and the challenge's evidence, and answers where to send the browser", async () => {
    const { client, posts, server } = setUp();
    const challenge = await challengeIn(client);
    const body = connectStartBody(challenge);
    if (!body) throw new Error("a connection challenge has a body");
    expect(body).toEqual({ broker: "alpaca", environment: "paper", disclosure: CONNECT_DISCLOSURE, step_up: { assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "cli_confirm" } });
    const outcome = await client.sendConnect(client.prepareConnect(body));
    if (!outcome.ok) throw new Error(outcome.error.code);
    expect(outcome.value.authorization_url.startsWith(`${MOCK_AUTHORIZATION_ORIGIN}/`)).toBe(true);
    expect(outcome.value.connection_id).toMatch(API_CONNECTION_ID);
    expect(posts.at(-1)?.path).toBe(`/v1/workspaces/${WS}/connections/oauth/start`);
    expect(server.started()).toHaveLength(1);
  });

  it("binds the step-up to this workspace's connect digest: a challenge for another digest is step_up_mismatch", async () => {
    const { client } = setUp();
    const other = await client.askChallenge({ kind: "connection", digest: connectStepUpDigest("ws_other") });
    if (!other.ok) throw new Error(other.error.code);
    const body = connectStartBody(other.value);
    if (!body) throw new Error("a connection challenge has a body");
    expect(await client.sendConnect(client.prepareConnect(body))).toMatchObject({ ok: false, error: { code: "step_up_mismatch", effect: "none" } });
  });

  it("has no body for a challenge of another kind, so nothing is sent", async () => {
    const { client } = setUp();
    expect(connectStartBody(await challengeIn(client, "approve"))).toBeNull();
  });

  it("is refused step_up_missing while the host has not confirmed, and starts nothing", async () => {
    const { client, server } = setUp({ hostConfirms: false });
    const body = connectStartBody(await challengeIn(client));
    if (!body) throw new Error("body");
    expect(await client.sendConnect(client.prepareConnect(body))).toMatchObject({ ok: false, error: { code: "step_up_missing", effect: "none" } });
    expect(server.started()).toHaveLength(0);
  });

  it("answers the same start for a retry under the same key, and spends the challenge for any new key", async () => {
    const { client, server } = setUp();
    const body = connectStartBody(await challengeIn(client));
    if (!body) throw new Error("body");
    const prepared = client.prepareConnect(body);
    const first = await client.sendConnect(prepared);
    const again = await client.sendConnect(prepared);
    expect(first.ok && again.ok && first.value.connection_id === again.value.connection_id).toBe(true);
    expect(await client.sendConnect(client.prepareConnect(body))).toMatchObject({ ok: false, error: { code: "step_up_reused", effect: "none" } });
    expect(server.started()).toHaveLength(1);
  });

  it("reads a lost answer as unknown, never as nothing happened", async () => {
    const { client } = setUp({ lose: 1 });
    const body = connectStartBody(await challengeIn(client));
    if (!body) throw new Error("body");
    expect(await client.sendConnect(client.prepareConnect(body))).toMatchObject({ ok: false, error: { code: "network", effect: "unknown" } });
  });
});

describe("the start's answer and the connection reads (DEC-2620, DEC-2623)", () => {
  const started = { authorization_url: "https://paper-api.alpaca.markets/oauth/authorize?state=x", expires_at: "2026-10-26T14:10:00.000000000Z", connection_id: "conn_01JBR2C4D6E8F0G2H4J6K8M0N2" };
  const row = { connection_id: "conn_01JBR2C4D6E8F0G2H4J6K8M0N2", broker: "alpaca", environment: "paper", state: "connecting", scopes: null, since: "2026-10-26T14:00:00.000000000Z", refusal: null, revoked_reason: null, agents: [] };

  it("sends the browser only to an https page", () => {
    expect(connectStarted(started, "").ok).toBe(true);
    for (const url of ["http://paper-api.alpaca.markets/oauth/authorize", "javascript:alert(1)", "https://", "https://a b", "HTTPS://paper-api.alpaca.markets/", "https://[", "/relative", 7]) expect(connectStarted({ ...started, authorization_url: url }, "").ok, String(url)).toBe(false);
  });

  it("reads a connection as its schema states it: a refusal exactly when refused, a revoke reason only when revoked", () => {
    expect(connectionDetail(row, "").ok).toBe(true);
    expect(connectionDetail({ ...row, state: "refused", refusal: { check: "scope", reason: "scope_too_broad" } }, "").ok).toBe(true);
    expect(connectionDetail({ ...row, state: "refused" }, "").ok).toBe(false);
    expect(connectionDetail({ ...row, refusal: { check: null, reason: "teardown" } }, "").ok).toBe(false);
    expect(connectionDetail({ ...row, revoked_reason: "owner" }, "").ok).toBe(false);
    expect(connectionDetail({ ...row, state: "revoked", revoked_reason: "compromised" }, "").ok).toBe(true);
    expect(connectionDetail({ ...row, state: "pending" }, "").ok).toBe(false);
  });

  it("names as the workspace's connection only an established one, never a pending or refused connect (Stop-all's scope)", async () => {
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const over = (rewrite: (rows: Array<Record<string, unknown>>) => Array<Record<string, unknown>>): Fetch => async (input, init) => {
      const answer = await server.fetch(input, init);
      if (!new URL(input).pathname.endsWith("/connections")) return answer;
      const read = (await answer.json()) as { connections: Array<Record<string, unknown>> };
      return new Response(JSON.stringify({ ...read, connections: rewrite(read.connections) }), { status: 200 });
    };
    const load = async (fetch: Fetch) => {
      const loaded = await loadApi(createWorkspaceClient({ workspaceId: WS, fetch, baseUrl: "https://api.owlhead.ai" }));
      if (!loaded.ok) throw new Error(loaded.error.code);
      return loaded.ws.connection.connection_id;
    };
    const ahead = (state: string, extra: Record<string, unknown> = {}) => (rows: Array<Record<string, unknown>>) => [{ ...rows[0], connection_id: "conn_01JBR2C4D6E8F0G2H4J6K8M0N2", state, ...extra }, ...rows];
    const established = await load(over((rows) => rows));
    expect(established).toMatch(/^con_/);
    expect(await load(over(ahead("connecting")))).toBe(established);
    expect(await load(over(ahead("refused", { refusal: { check: null, reason: "teardown" } })))).toBe(established);
    expect(await load(over((rows) => rows.map((c) => ({ ...c, state: "revoked", revoked_reason: "owner" }))))).toBe("");
  });
});
