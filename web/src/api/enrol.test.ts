import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { CREATE_TIMEOUT_MS, type Creator, type RegistrationChallenge, createPasskey, enrolBody, enrolStepUpDigest, enrolled, passkeyList, registrationChallenge } from "./enrol";

const CHALLENGE: RegistrationChallenge = {
  challenge_id: "01JC0000000000000000000002",
  webauthn_challenge: Buffer.from(Uint8Array.from({ length: 32 }, (_, i) => i)).toString("base64url"),
  rp_id: "app.owlhead.ai",
  user_handle: Buffer.from(Uint8Array.from({ length: 16 }, (_, i) => 100 + i)).toString("base64url"),
  exclude: [Buffer.from(Uint8Array.from({ length: 16 }, (_, i) => 200 + i)).toString("base64url")],
  step_up_needed: false,
  issued_at: "2026-10-26T10:00:00.000000000Z",
  expires_at: "2026-10-26T10:05:00.000000000Z",
};
const RAW = Uint8Array.from({ length: 16 }, (_, i) => 30 + i);
const made = (overrides: Record<string, unknown> = {}) =>
  ({ type: "public-key", rawId: RAW.slice().buffer, response: { attestationObject: Uint8Array.of(0xa3, 1).buffer, clientDataJSON: Uint8Array.of(0x7b, 0x7d).buffer }, ...overrides }) as unknown as Credential;
const SCHEMAS = join(__dirname, "..", "..", "..", "schemas", "workspace-api", "commands");

describe("the registration challenge and the enrolment, as the server answers them (§3.6)", () => {
  it("reads the registration challenge, the dev cell's localhost RP ID included", () => {
    expect(registrationChallenge(CHALLENGE, "")).toEqual({ ok: true, value: CHALLENGE });
    expect(registrationChallenge({ ...CHALLENGE, rp_id: "localhost" }, "").ok).toBe(true);
  });

  it("refuses a challenge that is not 32 bytes, a user handle that is not 16, an RP ID with a scheme, or a missing step_up_needed", () => {
    expect(registrationChallenge({ ...CHALLENGE, webauthn_challenge: "AQID" }, "").ok).toBe(false);
    expect(registrationChallenge({ ...CHALLENGE, user_handle: "AQID" }, "").ok).toBe(false);
    expect(registrationChallenge({ ...CHALLENGE, rp_id: "https://app.owlhead.ai" }, "").ok).toBe(false);
    expect(registrationChallenge({ ...CHALLENGE, step_up_needed: undefined }, "").ok).toBe(false);
  });

  it("reads the enrolled answer and the list, and refuses another algorithm", () => {
    const answer = { event_id: "01JC0000000000000000000003", passkey_id: CHALLENGE.exclude[0], alg: "es256", cool_off_ends: null };
    expect(enrolled(answer, "")).toEqual({ ok: true, value: answer });
    expect(enrolled({ ...answer, alg: "rs256" }, "").ok).toBe(false);
    expect(passkeyList({ passkeys: [{ passkey_id: CHALLENGE.exclude[0], alg: "eddsa", enrolled_at: CHALLENGE.issued_at, cool_off_ends: CHALLENGE.expires_at }] }, "").ok).toBe(true);
  });

  it("binds a later enrolment's step-up to SHA-256 of the canonical {op: enrol, registration}", () => {
    expect(enrolStepUpDigest("01JC0000000000000000000002")).toBe("sha256:eb22bda1137879fd3d3776631afd179476a2abf9946cb08b160683166cd3a0ee");
  });
});

describe("making the credential (DEC-2108 item 4)", () => {
  it("asks for a credential over exactly the challenge, for the deployment's RP ID and the member's handle, ES256 or Ed25519, UV required, no attestation, excluding the member's passkeys", async () => {
    const create = vi.fn<Creator>(async () => made());
    await createPasskey(CHALLENGE, "01JBR2C4D6E8F0G2H4J6K8M0P3", create);
    const pk = create.mock.calls[0][0].publicKey!;
    expect(new Uint8Array(pk.challenge as ArrayBuffer)).toEqual(Uint8Array.from({ length: 32 }, (_, i) => i));
    expect(pk.rp).toEqual({ id: "app.owlhead.ai", name: "Owlhead" });
    expect(new Uint8Array(pk.user.id as ArrayBuffer)).toEqual(Uint8Array.from({ length: 16 }, (_, i) => 100 + i));
    expect(pk.user.name).toBe("01JBR2C4D6E8F0G2H4J6K8M0P3");
    expect(pk.pubKeyCredParams.map((p) => p.alg)).toEqual([-7, -8]);
    expect(pk.authenticatorSelection?.userVerification).toBe("required");
    expect(pk.attestation).toBe("none");
    expect(pk.timeout).toBe(CREATE_TIMEOUT_MS);
    expect(pk.excludeCredentials?.map((c) => new Uint8Array(c.id as ArrayBuffer))).toEqual([Uint8Array.from({ length: 16 }, (_, i) => 200 + i)]);
  });

  it("answers the credential's id, attestation object and client data, each unpadded base64url", async () => {
    expect(await createPasskey(CHALLENGE, "x", async () => made())).toEqual({ ok: true, value: { passkey_id: Buffer.from(RAW).toString("base64url"), attestation_object: "owE", client_data_json: "e30" } });
  });

  it("makes nothing without WebAuthn, and reports a canceled or unreadable answer", async () => {
    expect(await createPasskey(CHALLENGE, "x", null)).toEqual({ ok: false, reason: "passkey_unavailable" });
    expect(await createPasskey(CHALLENGE, "x", async () => Promise.reject(new DOMException("no", "NotAllowedError")))).toEqual({ ok: false, reason: "passkey_canceled" });
    expect(await createPasskey(CHALLENGE, "x", async () => Promise.reject(new DOMException("held", "InvalidStateError")))).toEqual({ ok: false, reason: "passkey_already_held" });
    expect(await createPasskey(CHALLENGE, "x", async () => Promise.reject(new DOMException("rp", "SecurityError")))).toEqual({ ok: false, reason: "passkey_failed" });
    for (const answer of [null, made({ type: "password" }), made({ rawId: Uint8Array.of(1).buffer }), made({ response: { clientDataJSON: Uint8Array.of(1).buffer } })]) expect(await createPasskey(CHALLENGE, "x", async () => answer)).toEqual({ ok: false, reason: "passkey_failed" });
  });

  it("sends exactly the enrol request's members, its step-up null for a first passkey", async () => {
    const schema = JSON.parse(readFileSync(join(SCHEMAS, "passkey-enrol-request.schema.json"), "utf8")) as { required: string[]; properties: Record<string, unknown> };
    const answer = await createPasskey(CHALLENGE, "x", async () => made());
    if (!answer.ok) throw new Error("made");
    const body = enrolBody(CHALLENGE, answer.value, null);
    expect(Object.keys(body).sort()).toEqual([...schema.required].sort());
    expect(Object.keys(body).every((k) => k in schema.properties)).toBe(true);
    expect(body).toMatchObject({ challenge_id: CHALLENGE.challenge_id, step_up: null, step_up_assertion: null });
  });
});
