import { describe, expect, it } from "vitest";
import { type Fetch, createWorkspaceClient } from "./client";
import { prepareApprovalResponse } from "./commands";
import { MOCK_PRINCIPAL, createMockServer } from "./mock-server";
import { loadApi } from "./read-models";
import { type StepUpChallenge, cliConfirmEvidence } from "./step-up";
import type { ApprovalRecord } from "@/lib/mock-runtime";

/**
 * An approve with `cli_confirm` step-up from the browser's side (DEC-2104, workspace API spec §3.6
 * and §5.2), against the contract-faithful mock: the challenge is bound to the request's content hash,
 * the grant carries the challenge's evidence and nothing else, and every refusal records nothing.
 */
const WS = "ws_01J9ZQ4M0000000000000000AB";
const RECORD: ApprovalRecord = { screen: "D6", environment: "paper", shown: ["line"], modelOutputExpanded: false };

async function setUp(hostConfirms = true) {
  const server = createMockServer({ workspaceId: WS, scenario: "approvals", hostConfirms });
  const bodies: string[] = [];
  const fetch: Fetch = (input, init) => {
    if (init.method === "POST" && typeof init.body === "string") bodies.push(init.body);
    return server.fetch(input, init);
  };
  const client = createWorkspaceClient({ workspaceId: WS, fetch, baseUrl: "https://api.owlhead.ai" });
  const loaded = await loadApi(client);
  if (!loaded.ok) throw new Error(`not loaded: ${loaded.error.code}`);
  const approval = loaded.view.approvals.find((a) => a.state === "pending");
  if (!approval) throw new Error("the approvals fixture has an open request");
  return { server, client, approval, bodies };
}

async function challengeFor(client: ReturnType<typeof createWorkspaceClient>, digest: StepUpChallenge["action"]["digest"]): Promise<StepUpChallenge> {
  const answer = await client.askChallenge({ kind: "approve", digest });
  if (!answer.ok) throw new Error(`no challenge: ${answer.error.code}`);
  return answer.value;
}

async function approve(client: ReturnType<typeof createWorkspaceClient>, approval: { approval_id: string; content_hash: StepUpChallenge["action"]["digest"] }, challenge: StepUpChallenge | null) {
  const prepared = prepareApprovalResponse(client, approval, "approve", RECORD, challenge);
  if (!prepared.ok) throw new Error(prepared.reason);
  return client.send(prepared.commands[0]);
}

describe("an approve confirmed on the host (cli_confirm, DEC-2104)", () => {
  it("is recorded with the challenge's evidence, bound to the request's content hash", async () => {
    const { client, approval, bodies, server } = await setUp();
    const challenge = await challengeFor(client, approval.content_hash);
    expect(challenge.action).toEqual({ kind: "approve", digest: approval.content_hash });
    const outcome = await approve(client, approval, challenge);
    expect(outcome).toMatchObject({ ok: true, effect: "recorded" });
    const sent = JSON.parse(bodies.at(-1) ?? "{}") as Record<string, unknown>;
    expect(sent.step_up).toEqual(cliConfirmEvidence(challenge));
    expect(sent.step_up).toEqual({ assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "cli_confirm" });
    expect(Object.keys(sent).sort()).toEqual(["content_hash", "delegation", "record", "step_up", "verdict"]);
    expect(server.challenges()).toHaveLength(1);
    expect(server.challenges()[0].used).toBe(true);
  });

  it("names the session's principal, which the host command takes", async () => {
    const { client } = await setUp();
    expect(await client.readSession()).toMatchObject({ ok: true, value: { principal_id: MOCK_PRINCIPAL } });
  });

  it("is refused step_up_missing while the host has not confirmed it, and nothing is recorded", async () => {
    const { client, approval, server } = await setUp(false);
    const before = server.recorded().length;
    const outcome = await approve(client, approval, await challengeFor(client, approval.content_hash));
    expect(outcome).toMatchObject({ ok: false, error: { code: "step_up_missing", effect: "none" } });
    expect(server.recorded()).toHaveLength(before);
  });

  it("spends a challenge once: a second grant with it is step_up_reused", async () => {
    const { client, approval } = await setUp();
    const challenge = await challengeFor(client, approval.content_hash);
    expect((await approve(client, approval, challenge)).ok).toBe(true);
    expect(await approve(client, approval, challenge)).toMatchObject({ ok: false, error: { code: "step_up_reused", effect: "none" } });
  });

  it("sends nothing for a challenge bound to another request or kind", async () => {
    const { client, approval, bodies } = await setUp();
    const other = await challengeFor(client, `sha256:${"ee".repeat(32)}`);
    const sentBefore = bodies.length;
    expect(prepareApprovalResponse(client, approval, "approve", RECORD, other)).toEqual({ ok: false, reason: "step_up_mismatch" });
    const connect = { ...other, action: { kind: "connection" as const, digest: approval.content_hash } };
    expect(prepareApprovalResponse(client, approval, "approve", RECORD, connect)).toEqual({ ok: false, reason: "step_up_mismatch" });
    expect(bodies).toHaveLength(sentBefore);
  });

  it("is refused step_up_required with no evidence, as before", async () => {
    const { client, approval } = await setUp();
    expect(await approve(client, approval, null)).toMatchObject({ ok: false, error: { code: "step_up_required", effect: "none" } });
  });

  it("never puts step-up on a Skip, whatever challenge it is handed (API-7)", async () => {
    const { client, approval } = await setUp();
    const challenge = await challengeFor(client, approval.content_hash);
    const prepared = prepareApprovalResponse(client, approval, "skip", RECORD, challenge);
    if (!prepared.ok) throw new Error(prepared.reason);
    expect((JSON.parse(prepared.commands[0].body) as { step_up: unknown }).step_up).toBeNull();
    expect((await client.send(prepared.commands[0])).ok).toBe(true);
  });

  it("answers the same challenge for a retry under the same key, and a conflict for another action under it", async () => {
    const { server } = await setUp();
    const post = (body: unknown, key: string) =>
      server.fetch(`/v1/workspaces/${WS}/step-up/challenges`, { method: "POST", headers: { "Idempotency-Key": key, "X-Mandate-Request": "1" }, body: JSON.stringify(body) });
    const action = { action: { kind: "approve", digest: `sha256:${"aa".repeat(32)}` } };
    const first = (await (await post(action, "retry-key-000000001")).json()) as { challenge_id: string };
    const again = (await (await post(action, "retry-key-000000001")).json()) as { challenge_id: string };
    expect(again.challenge_id).toBe(first.challenge_id);
    expect((await post({ action: { kind: "approve", digest: `sha256:${"bb".repeat(32)}` } }, "retry-key-000000001")).status).toBe(409);
    expect((await post(action, "retry-key-000000002")).status).toBe(201);
  });
});
