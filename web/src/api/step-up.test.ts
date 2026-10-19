import { describe, expect, it } from "vitest";
import { createWorkspaceClient, type Fetch } from "./client";
import { createMockServer } from "./mock-server";
import { UNKNOWN_PRINCIPAL, cliConfirmCode, cliConfirmEvidence, cliConfirmLine, issueStepUpChallenge, prepareStepUpChallenge, stepUpChallenge, type StepUpChallenge } from "./step-up";

const WS = "ws_01J9ZQ4M0000000000000000AB";
const DIGEST = `sha256:${"2c".repeat(32)}` as const;

/**
 * Vectors computed apart from this module, in Python: `base64.urlsafe_b64encode(sha256(b"vector"))`
 * unpadded, and the first five bytes as a big-endian integer read five bits at a time into
 * Crockford's alphabet. The all-zero and all-one challenges are the server's own vectors
 * (`crates/mandate-api-server/tests/cli_confirm.rs`).
 */
const VECTORS: ReadonlyArray<[string, string]> = [
  ["sNUcWMi5wfRY-t8Wx9N1Yw71HaTfgZFYk7BcD6Tti8Y", "P3AH-RP68"],
  ["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "0000-0000"],
  ["__________________________________________8", "ZZZZ-ZZZZ"],
];

const CHALLENGE: StepUpChallenge = {
  challenge_id: "01JBM9S346Q3D25VT4F5V37E3S",
  webauthn_challenge: VECTORS[0][0],
  action: { kind: "approve", digest: DIGEST },
  issued_at: "2026-10-19T12:00:07.123456789Z",
  expires_at: "2026-10-19T12:05:07.123456789Z",
};

describe("cli_confirm step-up (DEC-2104)", () => {
  it("derives the shown code from the challenge's first 40 bits, and has none for any other shape", () => {
    for (const [challenge, code] of VECTORS) expect(cliConfirmCode(challenge)).toBe(code);
    expect(cliConfirmCode(VECTORS[0][0].slice(1))).toBeNull();
    expect(cliConfirmCode(`${VECTORS[0][0].slice(0, 42)}B`)).toBeNull();
    expect(cliConfirmCode(`${VECTORS[0][0].slice(0, 42)}+`)).toBeNull();
  });

  it("submits the challenge's id and its issued_at in whole seconds, as cli_confirm", () => {
    expect(cliConfirmEvidence(CHALLENGE)).toEqual({ assertion_id: CHALLENGE.challenge_id, authenticated_at: "2026-10-19T12:00:07.000000000Z", method: "cli_confirm" });
  });

  it("names every flag of the CLI command, with the principal left for the owner to fill", () => {
    expect(cliConfirmLine(WS, "agt_1", "01JBM9S346Q3D25VT4F5V37E3T", CHALLENGE)).toBe(
      `mandate step-up confirm --workspace ${WS} --principal '${UNKNOWN_PRINCIPAL}' --agent agt_1 --approval 01JBM9S346Q3D25VT4F5V37E3T --challenge ${CHALLENGE.challenge_id} --code P3AH-RP68`,
    );
  });

  it("decodes the 201 strictly: an id that is no event id, or a challenge of another shape, is refused", () => {
    expect(stepUpChallenge(CHALLENGE, "").ok).toBe(true);
    expect(stepUpChallenge({ ...CHALLENGE, challenge_id: "chl_1" }, "").ok).toBe(false);
    expect(stepUpChallenge({ ...CHALLENGE, webauthn_challenge: "short" }, "").ok).toBe(false);
    expect(stepUpChallenge({ ...CHALLENGE, action: { kind: "deploy", digest: DIGEST } }, "").ok).toBe(false);
  });

  it("asks once per gesture, and refuses a challenge bound to another action", async () => {
    const server = createMockServer({ workspaceId: WS });
    const fetch: Fetch = (input, init) => server.fetch(input, init);
    const client = createWorkspaceClient({ workspaceId: WS, fetch });
    const command = prepareStepUpChallenge(client, "approve", DIGEST);
    const first = await issueStepUpChallenge(client, command, "approve", DIGEST);
    const again = await issueStepUpChallenge(client, command, "approve", DIGEST);
    expect(first.ok && again.ok && first.value.challenge_id === again.value.challenge_id).toBe(true);
    const other = await issueStepUpChallenge(client, command, "approve", `sha256:${"3d".repeat(32)}`);
    expect(other.ok ? null : other.error).toMatchObject({ code: "invalid_response", decode: { path: "/action" } });
  });
});
