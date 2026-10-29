import { describe, expect, it } from "vitest";
import { type StepUpAction, CONNECT_DISCLOSURE, approvalConfirmCommand, approveWorkflowInput, connectWorkflowInput, connectConfirmCommand, connectStepUpDigest, challengeLifetimeMs, cliConfirmEvidence, confirmCode, sessionPrincipal, stepUpChallenge } from "./step-up";

const DIGEST = `sha256:${"ab".repeat(32)}` as const;
const ASKED: StepUpAction = { kind: "approve", digest: DIGEST };
/** 32 bytes, 0x01 to 0x20, unpadded base64url. */
const RISING = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";
const ISSUED = "2026-10-26T14:00:00.123456789Z";
const EXPIRES = "2026-10-26T14:05:00.123456789Z";

const served = (patch: Record<string, unknown> = {}) => ({
  challenge_id: "01JBR2C4D6E8F0G2H4J6K8M0N2",
  webauthn_challenge: RISING,
  action: { kind: "approve", digest: DIGEST },
  issued_at: ISSUED,
  expires_at: EXPIRES,
  ...patch,
});

/** An independent oracle for DEC-2104 item 1: bytes to a big-endian integer, top 40 bits, base 32 by division. */
function oracle(bytes: number[]): string {
  let n = 0n;
  for (const b of bytes.slice(0, 5)) n = n * 256n + BigInt(b);
  const digits: string[] = [];
  for (let i = 0; i < 8; i += 1) {
    digits.unshift("0123456789ABCDEFGHJKMNPQRSTVWXYZ"[Number(n % 32n)]);
    n /= 32n;
  }
  return `${digits.slice(0, 4).join("")}-${digits.slice(4).join("")}`;
}

const b64url = (bytes: number[]) => Buffer.from(bytes).toString("base64url");

describe("the confirm code (DEC-2104 item 1)", () => {
  it("matches the server's vectors: all zero, all one, and a rising run", () => {
    expect(confirmCode(b64url(Array(32).fill(0)))).toBe("0000-0000");
    expect(confirmCode(b64url(Array(32).fill(0xff)))).toBe("ZZZZ-ZZZZ");
    expect(confirmCode(RISING)).toBe("0410-6105");
    expect(confirmCode(b64url([0xf3, 0xa9, 0x1c, 0x0e, 0x7d, ...Array(27).fill(0)]))).toBe("YEMH-R3KX");
  });

  it("agrees with an independent oracle on many byte strings, and reads only the first 40 bits", () => {
    let seed = 7;
    for (let run = 0; run < 500; run += 1) {
      const bytes = Array.from({ length: 32 }, () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) & 0xff);
      expect(confirmCode(b64url(bytes)), bytes.join(",")).toBe(oracle(bytes));
      const tail = [...bytes.slice(0, 5), ...bytes.slice(5).map((b) => b ^ 0xff)];
      expect(confirmCode(b64url(tail))).toBe(oracle(bytes));
    }
  });

  it("uses Crockford's alphabet only: never I, L, O or U", () => {
    let seed = 11;
    for (let run = 0; run < 200; run += 1) {
      const bytes = Array.from({ length: 32 }, () => (seed = (seed * 69069 + 1) % 2 ** 32) & 0xff);
      expect(confirmCode(b64url(bytes))).toMatch(/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/);
    }
  });

  it("has no code for a value that is not 32 bytes of unpadded base64url", () => {
    for (const bad of ["", RISING.slice(1), `${RISING}=`, `${RISING.slice(0, 42)}B`, RISING.replace("A", "+"), b64url(Array(31).fill(1)), b64url(Array(33).fill(1))]) expect(confirmCode(bad), bad).toBeNull();
  });
});

describe("the challenge answer (commands/step-up-challenge)", () => {
  const decode = stepUpChallenge(ASKED);

  it("reads a challenge bound to the action asked for", () => {
    const read = decode(served(), "");
    expect(read).toEqual({ ok: true, value: served() });
  });

  it("ignores members it does not name (spec §3.2)", () => {
    expect(decode(served({ api_version: "v1", as_of: [], later: 1 }), "").ok).toBe(true);
  });

  it("refuses an answer bound to another kind or digest, so no code is shown for an action not asked about", () => {
    expect(decode(served({ action: { kind: "connection", digest: DIGEST } }), "")).toMatchObject({ ok: false, issue: { path: "/action/kind" } });
    expect(decode(served({ action: { kind: "approve", digest: `sha256:${"cd".repeat(32)}` } }), "")).toMatchObject({ ok: false, issue: { path: "/action/digest" } });
    expect(decode(served({ action: { kind: "confirm_version", digest: DIGEST } }), "")).toMatchObject({ ok: false, issue: { path: "/action/kind" } });
    expect(decode(served({ action: undefined }), "")).toMatchObject({ ok: false, issue: { path: "/action", problem: "missing" } });
  });

  it("refuses a challenge id, challenge or instant that is not canonical", () => {
    expect(decode(served({ challenge_id: "01jbr2c4d6e8f0g2h4j6k8m0n2" }), "")).toMatchObject({ ok: false, issue: { path: "/challenge_id" } });
    expect(decode(served({ challenge_id: undefined }), "")).toMatchObject({ ok: false, issue: { path: "/challenge_id", problem: "missing" } });
    expect(decode(served({ webauthn_challenge: `${RISING}=` }), "")).toMatchObject({ ok: false, issue: { path: "/webauthn_challenge" } });
    expect(decode(served({ issued_at: "2026-10-26T14:00:00Z" }), "")).toMatchObject({ ok: false, issue: { path: "/issued_at" } });
    expect(decode(served({ expires_at: 1 }), "")).toMatchObject({ ok: false, issue: { path: "/expires_at" } });
  });

  it("refuses a challenge that expires before it is issued, or as it is", () => {
    expect(decode(served({ expires_at: ISSUED }), "")).toMatchObject({ ok: false, issue: { path: "/expires_at" } });
    expect(decode(served({ expires_at: "2026-10-26T13:59:59.000000000Z" }), "")).toMatchObject({ ok: false, issue: { path: "/expires_at" } });
  });
});

describe("the grant's evidence (DEC-2104 item 5)", () => {
  it("names the challenge and carries its issued_at exactly as served", () => {
    const read = stepUpChallenge(ASKED)(served(), "");
    if (!read.ok) throw new Error("decodes");
    expect(cliConfirmEvidence(read.value)).toEqual({ assertion_id: "01JBR2C4D6E8F0G2H4J6K8M0N2", authenticated_at: ISSUED, method: "cli_confirm" });
  });

  it("lives from issued_at to expires_at, whatever this device's clock says", () => {
    const read = stepUpChallenge(ASKED)(served(), "");
    if (!read.ok) throw new Error("decodes");
    expect(challengeLifetimeMs(read.value)).toBe(300_000);
    const short = stepUpChallenge(ASKED)(served({ expires_at: "2026-10-26T14:00:01.123456789Z" }), "");
    if (!short.ok) throw new Error("decodes");
    expect(challengeLifetimeMs(short.value)).toBe(1_000);
  });
});

describe("the host command (DEC-2104 item 2)", () => {
  it("names every flag the CLI requires, the code as shown, and no secret", () => {
    const read = stepUpChallenge(ASKED)(served(), "");
    if (!read.ok) throw new Error("decodes");
    const line = approvalConfirmCommand({ workspaceId: "ws_1", principalId: "01JBR2C4D6E8F0G2H4J6K8M0P3", agentId: "agt_1", approvalId: "01JBR2C4D6E8F0G2H4J6K8M0Q4", shown: { challenge: read.value, code: "0410-6105" } });
    expect(line).toBe(
      "mandate step-up confirm --workspace ws_1 --principal 01JBR2C4D6E8F0G2H4J6K8M0P3 --agent agt_1 --approval 01JBR2C4D6E8F0G2H4J6K8M0Q4 --challenge 01JBR2C4D6E8F0G2H4J6K8M0N2 --code 0410-6105 --journal <journal DSN> --store <artifact store>",
    );
    expect(line).not.toContain(RISING);
  });
});

describe("the step-up workflow's one input (lane GO's step-up.yml)", () => {
  it("is approve:<challenge>:<code>:<agent>:<approval> and connect:<challenge>:<code>, the same ids the host command names", () => {
    const read = stepUpChallenge(ASKED)(served(), "");
    if (!read.ok) throw new Error("decodes");
    const shown = { challenge: read.value, code: "0410-6105" };
    expect(approveWorkflowInput({ agentId: "agt_1", approvalId: "01JBR2C4D6E8F0G2H4J6K8M0Q4", shown })).toBe("approve:01JBR2C4D6E8F0G2H4J6K8M0N2:0410-6105:agt_1:01JBR2C4D6E8F0G2H4J6K8M0Q4");
    expect(connectWorkflowInput(shown)).toBe("connect:01JBR2C4D6E8F0G2H4J6K8M0N2:0410-6105");
  });

  it("shows no string the workflow would refuse: an id with a colon, a space or over 128 characters, or a malformed code", () => {
    const read = stepUpChallenge(ASKED)(served(), "");
    if (!read.ok) throw new Error("decodes");
    const shown = { challenge: read.value, code: "0410-6105" };
    for (const agentId of ["agt:1", "agt 1", "", "a".repeat(129)]) expect(approveWorkflowInput({ agentId, approvalId: "01JBR2C4D6E8F0G2H4J6K8M0Q4", shown }), agentId).toBeNull();
    expect(approveWorkflowInput({ agentId: "a".repeat(128), approvalId: "apr_1", shown })).not.toBeNull();
    for (const code of ["0410 6105", "04106105", "0410-610"]) expect(connectWorkflowInput({ ...shown, code }), code).toBeNull();
  });
});

describe("the session's principal (read-models/session)", () => {
  it("reads a ULID principal id and refuses anything else", () => {
    expect(sessionPrincipal({ principal_id: "01JBR2C4D6E8F0G2H4J6K8M0P3", kind: "full" }, "")).toEqual({ ok: true, value: { principal_id: "01JBR2C4D6E8F0G2H4J6K8M0P3" } });
    expect(sessionPrincipal({ principal_id: "usr_1" }, "").ok).toBe(false);
    expect(sessionPrincipal({}, "").ok).toBe(false);
  });
});

describe("a connect's step-up (DEC-2621 item 4, DEC-2125)", () => {
  it("binds the canonical connect object the server and the CLI hash, byte for byte", () => {
    expect(CONNECT_DISCLOSURE).toEqual({ document: "alpaca_oauth_token_breadth", version: "1" });
    expect(connectStepUpDigest("ws_01")).toBe("sha256:3df08a75075094b6caf39b686e7262825ead70b36711631621c47a14e32749c2");
    expect(connectStepUpDigest("ws_02")).not.toBe(connectStepUpDigest("ws_01"));
  });

  it("names the confirm-connect command's flags, and no digest: the CLI computes it", () => {
    const read = stepUpChallenge({ kind: "connection", digest: connectStepUpDigest("ws_1") })(served({ action: { kind: "connection", digest: connectStepUpDigest("ws_1") } }), "");
    if (!read.ok) throw new Error("decodes");
    const line = connectConfirmCommand({ workspaceId: "ws_1", principalId: "01JBR2C4D6E8F0G2H4J6K8M0P3", shown: { challenge: read.value, code: "0410-6105" } });
    expect(line).toBe("mandate step-up confirm-connect --workspace ws_1 --principal 01JBR2C4D6E8F0G2H4J6K8M0P3 --challenge 01JBR2C4D6E8F0G2H4J6K8M0N2 --code 0410-6105 --journal <journal DSN> --store <artifact store>");
    expect(line).not.toContain("sha256:");
  });
});
