import { describe, expect, it, vi } from "vitest";
import { confirmStepUpDigest } from "./confirm";
import { ASSERTION_TIMEOUT_MS, type Authenticator, type Passkey, assertPasskey, coolOffWaivedFrom, deployStepUpDigest, fromBase64url, passkeysRead, toBase64url, usablePasskeys } from "./passkey";
import type { StepUpChallenge } from "./step-up";

/** 32 bytes 0..31 as unpadded base64url: what a served `webauthn_challenge` looks like. */
const CHALLENGE_BYTES = Uint8Array.from({ length: 32 }, (_, i) => i);
const CHALLENGE: StepUpChallenge = {
  challenge_id: "01JC0000000000000000000001",
  webauthn_challenge: Buffer.from(CHALLENGE_BYTES).toString("base64url"),
  action: { kind: "deploy", digest: `sha256:${"ab".repeat(32)}` },
  issued_at: "2026-10-26T10:00:00.000000000Z",
  expires_at: "2026-10-26T10:05:00.000000000Z",
};
const NOW = "2026-10-26T10:00:01.000Z";
const RAW_ID = Uint8Array.from({ length: 16 }, (_, i) => 200 + i);
const PASSKEY: Passkey = { passkey_id: Buffer.from(RAW_ID).toString("base64url"), cool_off_ends: null };

/** What a browser's `navigator.credentials.get` answers: a `public-key` credential with its assertion response. */
function credential(overrides: Record<string, unknown> = {}) {
  return {
    type: "public-key",
    id: PASSKEY.passkey_id,
    rawId: RAW_ID.slice().buffer,
    response: { authenticatorData: Uint8Array.of(1, 2, 3).buffer, clientDataJSON: new TextEncoder().encode('{"type":"webauthn.get"}').buffer, signature: Uint8Array.of(9, 8, 7, 6).buffer, userHandle: null },
    ...overrides,
  } as unknown as Credential;
}

describe("unpadded base64url, the encoding of every assertion member", () => {
  it("round-trips every length, with no padding, the URL alphabet, and the same text Node's encoder writes", () => {
    for (let n = 0; n < 70; n += 1) {
      const bytes = Uint8Array.from({ length: n }, (_, i) => (i * 37 + n) & 0xff);
      const text = toBase64url(bytes);
      expect(text).toBe(Buffer.from(bytes).toString("base64url"));
      expect(text).not.toMatch(/[=+/]/);
      expect(fromBase64url(text)).toEqual(bytes);
    }
  });

  it("refuses text that encodes no bytes: a foreign character, or a length whose remainder by 4 is 1", () => {
    expect(fromBase64url("ab+c")).toBeNull();
    expect(fromBase64url("abcde")).toBeNull();
    expect(fromBase64url("ab=")).toBeNull();
  });
});

describe("the deployment's step-up digest (spec §3.11 item 11)", () => {
  it("is SHA-256 of the canonical {agent_id, mandate_version}, as an independent canonical JSON writer computes it", () => {
    expect(deployStepUpDigest("agt_01JC0000000000000000000000", `sha256:${"ab".repeat(32)}`)).toBe("sha256:1fd9d274f86641c62e68d8823fdab741b99a7fb84d9cc765fa7b373bbf91e4b3");
  });

  it("matches §5.1's confirm digest for a new mandate, from the same independent writer", () => {
    expect(confirmStepUpDigest(`sha256:${"ab".repeat(32)}`, null, null)).toBe("sha256:a87530c6f2a2c5d3f0711b0fc06101969e772a3e0a81db33b25be0ddc63c95dd");
  });

  it("binds both members: another agent or another version is another digest", () => {
    const base = deployStepUpDigest("agt_A", `sha256:${"ab".repeat(32)}`);
    expect(deployStepUpDigest("agt_B", `sha256:${"ab".repeat(32)}`)).not.toBe(base);
    expect(deployStepUpDigest("agt_A", `sha256:${"cd".repeat(32)}`)).not.toBe(base);
  });
});

describe("read-models/passkeys, as the ceremony reads it", () => {
  const served = (p: unknown) => passkeysRead({ passkeys: [p] }, "");
  it("keeps each passkey's id and cool-off, and nothing else", () => {
    expect(served({ passkey_id: PASSKEY.passkey_id, alg: "es256", enrolled_at: "2026-10-25T10:00:00.000000000Z", cool_off_ends: null })).toEqual({ ok: true, value: { passkeys: [PASSKEY] } });
  });

  it("refuses an id that is not a passkey's, an unknown algorithm, and a cool-off that is not a timestamp", () => {
    expect(served({ passkey_id: "short", alg: "es256", enrolled_at: "2026-10-25T10:00:00.000000000Z", cool_off_ends: null }).ok).toBe(false);
    expect(served({ passkey_id: PASSKEY.passkey_id, alg: "rs256", enrolled_at: "2026-10-25T10:00:00.000000000Z", cool_off_ends: null }).ok).toBe(false);
    expect(served({ passkey_id: PASSKEY.passkey_id, alg: "es256", enrolled_at: "2026-10-25T10:00:00.000000000Z", cool_off_ends: "tomorrow" }).ok).toBe(false);
  });
});

describe("the passkey ceremony (DEC-2108 item 7, DEC-2128)", () => {
  it("signs exactly the served challenge's bytes, with user verification required, offering only the member's passkeys", async () => {
    const get = vi.fn<Authenticator>(async () => credential());
    await assertPasskey(CHALLENGE, [PASSKEY], get, NOW);
    expect(get).toHaveBeenCalledTimes(1);
    const options = get.mock.calls[0][0].publicKey as PublicKeyCredentialRequestOptions;
    expect(new Uint8Array(options.challenge as ArrayBuffer)).toEqual(CHALLENGE_BYTES);
    expect(options.userVerification).toBe("required");
    expect(options.timeout).toBe(ASSERTION_TIMEOUT_MS);
    expect(options.rpId).toBeUndefined();
    expect(options.allowCredentials?.map((c) => new Uint8Array(c.id as ArrayBuffer))).toEqual([RAW_ID]);
  });

  it("answers the challenge's evidence and the authenticator's four values, each unpadded base64url", async () => {
    const answer = await assertPasskey(CHALLENGE, [PASSKEY], async () => credential(), NOW);
    expect(answer).toEqual({
      ok: true,
      value: {
        step_up: { assertion_id: CHALLENGE.challenge_id, authenticated_at: CHALLENGE.issued_at, method: "passkey" },
        step_up_assertion: { passkey_id: PASSKEY.passkey_id, authenticator_data: "AQID", client_data_json: Buffer.from('{"type":"webauthn.get"}').toString("base64url"), signature: "CQgHBg" },
      },
    });
  });

  it("asks nothing of an authenticator a browser does not have, or for a member with no passkey", async () => {
    expect(await assertPasskey(CHALLENGE, [PASSKEY], null, NOW)).toEqual({ ok: false, reason: "passkey_unavailable" });
    const get = vi.fn<Authenticator>(async () => credential());
    expect(await assertPasskey(CHALLENGE, [], get, NOW)).toEqual({ ok: false, reason: "no_passkey" });
    expect(get).not.toHaveBeenCalled();
  });

  it("asks nothing while every passkey is in its cool-off, and offers only the passkeys past it", async () => {
    const get = vi.fn<Authenticator>(async () => credential());
    const cooling = { ...PASSKEY, cool_off_ends: "2026-10-27T09:00:00.000000000Z" };
    expect(await assertPasskey(CHALLENGE, [cooling], get, NOW)).toEqual({ ok: false, reason: "passkey_cooling_off" });
    expect(get).not.toHaveBeenCalled();
    const pastId = Uint8Array.from({ length: 16 }, (_, i) => 100 + i);
    const past = { passkey_id: Buffer.from(pastId).toString("base64url"), cool_off_ends: "2026-10-26T09:59:59.999999999Z" };
    expect((await assertPasskey(CHALLENGE, [cooling, past], get, NOW)).ok).toBe(true);
    expect(get).toHaveBeenCalledTimes(1);
    const offered = (get.mock.calls[0][0].publicKey as PublicKeyCredentialRequestOptions).allowCredentials ?? [];
    expect(offered.map((c) => new Uint8Array(c.id as ArrayBuffer))).toEqual([pastId]);
  });

  it("tries a passkey in its cool-off only in a build that waives it, and changes no other refusal (DEC-2411)", () => {
    const get = vi.fn<Authenticator>(async () => credential());
    const cooling = { ...PASSKEY, cool_off_ends: "2026-10-27T09:00:00.000000000Z" };
    expect(usablePasskeys([cooling], get, NOW, false)).toEqual({ ok: false, reason: "passkey_cooling_off" });
    expect(usablePasskeys([cooling], get, NOW)).toEqual({ ok: false, reason: "passkey_cooling_off" });
    expect(usablePasskeys([cooling], get, NOW, true)).toEqual({ ok: true, value: [cooling] });
    expect(usablePasskeys([cooling], null, NOW, true)).toEqual({ ok: false, reason: "passkey_unavailable" });
    expect(usablePasskeys([], get, NOW, true)).toEqual({ ok: false, reason: "no_passkey" });
    expect(usablePasskeys([PASSKEY], get, NOW, false)).toEqual(usablePasskeys([PASSKEY], get, NOW, true));
    expect(get).not.toHaveBeenCalled();
  });

  it("reads the waiver only from the exact value 1", () => {
    expect(coolOffWaivedFrom("1")).toBe(true);
    for (const value of [undefined, "", "0", "true", " 1", "1 ", "yes"]) expect(coolOffWaivedFrom(value)).toBe(false);
  });

  it("reports a dismissed or timed-out authenticator as canceled, and sends nothing", async () => {
    const dismissed = async (): Promise<Credential | null> => {
      throw new DOMException("The operation either timed out or was not allowed.", "NotAllowedError");
    };
    expect(await assertPasskey(CHALLENGE, [PASSKEY], dismissed, NOW)).toEqual({ ok: false, reason: "passkey_canceled" });
  });

  it("refuses an answer that is not a public-key assertion, and a challenge that is not 32 bytes", async () => {
    for (const answer of [null, credential({ type: "password" }), credential({ rawId: Uint8Array.of(1).buffer }), credential({ response: { authenticatorData: null } })])
      expect(await assertPasskey(CHALLENGE, [PASSKEY], async () => answer, NOW)).toEqual({ ok: false, reason: "passkey_failed" });
    const get = vi.fn<Authenticator>(async () => credential());
    expect(await assertPasskey({ ...CHALLENGE, webauthn_challenge: "AQID" }, [PASSKEY], get, NOW)).toEqual({ ok: false, reason: "passkey_failed" });
    expect(get).not.toHaveBeenCalled();
  });
});
