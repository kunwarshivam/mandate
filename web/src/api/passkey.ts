/**
 * Passkey step-up from the browser (workspace API spec §3.6; DEC-2108 items 7 and 8, DEC-2128): the
 * browser asks the server for a challenge bound to one action, the member's authenticator signs that
 * challenge's `webauthn_challenge`, and the call carries `step_up` naming the challenge beside
 * `step_up_assertion`, the authenticator's four values as unpadded base64url. The server judges
 * everything: the challenge, the passkey, its cool-off, the signature and the digest. The browser
 * only reports what the authenticator answered, and never sends an assertion for an action other
 * than the one the challenge names.
 *
 * Nothing here is stored: the assertion is spent with its challenge (300 s, once), and the browser
 * keeps no key; the private key never leaves the authenticator.
 */
import { array, closedEnum, isRecord, nullable, object, string, timestamp } from "./decode";
import type { ContentRef, Decoder, Timestamp } from "./types";
import type { StepUpChallenge } from "./step-up";
import { coolOffWaivedFrom } from "@/lib/cool-off-build";
import { sha256Hex } from "@/lib/sha256";

/** `envelope.schema.json`'s `StepUpEvidence` as a passkey call carries it (DEC-2108 item 7: `authenticated_at` is the challenge's `issued_at`). */
export interface PasskeyEvidence {
  assertion_id: string;
  authenticated_at: Timestamp;
  method: "passkey";
}

/** `envelope.schema.json`'s `StepUpAssertion` (DEC-2108 item 7). */
export interface StepUpAssertion {
  passkey_id: string;
  authenticator_data: string;
  client_data_json: string;
  signature: string;
}

/** What a call under passkey step-up carries beside its own members. */
export interface PasskeyStepUp {
  step_up: PasskeyEvidence;
  step_up_assertion: StepUpAssertion;
}

/** One of the member's passkeys as `GET /passkeys` serves it (`read-models/passkeys`): only what the ceremony needs. */
export interface Passkey {
  passkey_id: string;
  cool_off_ends: Timestamp | null;
}

/** `common.schema.json`'s `PasskeyId`: 16 to 1023 bytes as unpadded base64url. */
const PASSKEY_ID = /^(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2,3})?$/;

const passkeyId: Decoder<string> = (value, path) => {
  const read = string(value, path);
  if (!read.ok) return read;
  if (read.value.length < 22 || read.value.length > 1364 || !PASSKEY_ID.test(read.value)) return { ok: false, issue: { path, problem: "not_canonical", value: read.value, allowed: null } };
  return read;
};

/** `read-models/passkeys`, the members the ceremony reads; `alg` is checked, since a passkey of another algorithm is not the server's. */
export const passkeysRead: Decoder<{ passkeys: Passkey[] }> = (value, path) => {
  const read = object<{ passkeys: Array<Passkey & { alg: "es256" | "eddsa"; enrolled_at: Timestamp }> }>({
    passkeys: array(object({ passkey_id: passkeyId, alg: closedEnum(["es256", "eddsa"] as const), enrolled_at: timestamp, cool_off_ends: nullable(timestamp) })),
  })(value, path);
  if (!read.ok) return read;
  return { ok: true, value: { passkeys: read.value.passkeys.map((p) => ({ passkey_id: p.passkey_id, cool_off_ends: p.cool_off_ends })) } };
};

/**
 * The digest a deployment's step-up binds (spec §3.11 item 11, `versions::deploy_digest`): SHA-256 of
 * the canonical `{agent_id, mandate_version}`, its members in journal spec §4's key order. Both
 * values are plain ASCII strings, so this is the canonical form.
 */
export function deployStepUpDigest(agentId: string, version: ContentRef): ContentRef {
  return `sha256:${sha256Hex(JSON.stringify({ agent_id: agentId, mandate_version: version }))}`;
}

/** What a resume or Stop step-up confirms (DEC-2102 item 1): a Stop with release is its own action. */
export type OwnerCommandConfirms = "resume" | "stop" | "stop_release";

/**
 * The digest a resume's or Stop's step-up binds (spec §3.6, DEC-2102 item 1, DEC-2140 item 2):
 * SHA-256 of the canonical object of what it confirms, the agent, and the control stream's head as
 * text, its other members `null`, in journal spec §4's key order. The server binds the head at the
 * command's receipt, so the head must be read just before the challenge is asked; one appended in
 * between leaves the evidence unbound, and the runtime refuses the command. Agent ids are plain
 * ASCII, so this is the canonical form.
 */
export function ownerCommandStepUpDigest(confirms: OwnerCommandConfirms, agentId: string, controlHead: number): ContentRef {
  return `sha256:${sha256Hex(JSON.stringify({ agent: agentId, bid: null, bid_size: null, confirms, control_head: String(controlHead), event: null, floor: null, instrument: null }))}`;
}

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/** Bytes as unpadded base64url (RFC 4648 §5), as the server reads each member of the assertion. */
export function toBase64url(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    const chars = Math.min(4, Math.ceil(((bytes.length - i) * 8) / 6));
    for (let c = 0; c < chars; c += 1) out += B64[(n >> (18 - 6 * c)) & 63];
  }
  return out;
}

/** Unpadded base64url as bytes, or `null` for text that is not (a length whose remainder by 4 is 1 encodes no bytes). */
export function fromBase64url(text: string): Uint8Array | null {
  if (!/^[A-Za-z0-9_-]*$/.test(text) || text.length % 4 === 1) return null;
  const out = new Uint8Array(Math.floor((text.length * 6) / 8));
  let bits = 0;
  let width = 0;
  let at = 0;
  for (const char of text) {
    bits = (bits << 6) | B64.indexOf(char);
    width += 6;
    if (width >= 8) {
      width -= 8;
      out[at] = (bits >> width) & 0xff;
      at += 1;
    }
  }
  return out;
}

/** The browser's WebAuthn call, injectable so a test can stand in for an authenticator. */
export type Authenticator = (options: CredentialRequestOptions) => Promise<Credential | null>;

/** The browser's own authenticator, or `null` where the page has no WebAuthn. */
export function browserAuthenticator(): Authenticator | null {
  if (typeof window === "undefined" || typeof window.PublicKeyCredential === "undefined" || !navigator.credentials) return null;
  return (options) => navigator.credentials.get(options);
}

/** How long the authenticator may take, well inside the challenge's 300 s. */
export const ASSERTION_TIMEOUT_MS = 120_000;

/**
 * Why no assertion was made; nothing was sent to the server in any case.
 * - `passkey_unavailable`: this browser has no WebAuthn.
 * - `no_passkey`: the member has no passkey enrolled on the deployment.
 * - `passkey_cooling_off`: every passkey the member holds is still in its first 24 hours (DEC-2108 item 5).
 * - `passkey_canceled`: the owner dismissed the authenticator, or it timed out.
 * - `passkey_failed`: the authenticator answered with something that is not an assertion.
 */
export type AssertionRefusal = "passkey_unavailable" | "no_passkey" | "passkey_cooling_off" | "passkey_canceled" | "passkey_failed";

export type AssertionAnswer = { ok: true; value: PasskeyStepUp } | { ok: false; reason: AssertionRefusal };

/** A timestamp as milliseconds since the epoch, digits past the millisecond dropped, so a served nine-digit time compares with this page's clock. */
const instant = (t: Timestamp): number => Date.parse(`${t.slice(0, 23)}Z`);

/** An authenticator's buffer as bytes, whichever realm made it (a tag check, since `instanceof` fails across realms). */
const bytesOf = (buffer: unknown): Uint8Array | null =>
  Object.prototype.toString.call(buffer) === "[object ArrayBuffer]"
    ? new Uint8Array(buffer as ArrayBuffer)
    : ArrayBuffer.isView(buffer)
      ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
      : null;

/** DEC-2411's flag as this build was made with it; the only place it is read (`coolOffWaivedFrom`). */
const COOL_OFF_WAIVED = coolOffWaivedFrom(process.env.NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED);

/**
 * The member's passkeys that can sign now, by this page's clock (which runs on the server's): none
 * without WebAuthn or a passkey, and none while every one is in its cool-off (DEC-2108 item 5). A
 * passkey still cooling off is never offered, so the authenticator cannot pick one the server refuses,
 * except in a build that tries one, `waived` (DEC-2411), which leaves the verdict to the server.
 */
export function usablePasskeys(passkeys: readonly Passkey[], authenticator: Authenticator | null, now: Timestamp, waived = COOL_OFF_WAIVED): { ok: true; value: Passkey[] } | { ok: false; reason: AssertionRefusal } {
  if (!authenticator) return { ok: false, reason: "passkey_unavailable" };
  if (passkeys.length === 0) return { ok: false, reason: "no_passkey" };
  const at = instant(now);
  const usable = passkeys.filter((p) => waived || p.cool_off_ends === null || instant(p.cool_off_ends) <= at);
  return usable.length > 0 ? { ok: true, value: usable } : { ok: false, reason: "passkey_cooling_off" };
}

/**
 * Asks the authenticator to sign `challenge` with one of the usable `passkeys`, user verification
 * required. The RP ID is the page's own host, which is the deployment's (the server derives it from
 * the app's origin).
 */
export async function assertPasskey(challenge: StepUpChallenge, passkeys: readonly Passkey[], authenticator: Authenticator | null, now: Timestamp): Promise<AssertionAnswer> {
  const usable = usablePasskeys(passkeys, authenticator, now);
  if (!usable.ok) return usable;
  if (!authenticator) return { ok: false, reason: "passkey_unavailable" };
  const signed = fromBase64url(challenge.webauthn_challenge);
  if (!signed || signed.length !== 32) return { ok: false, reason: "passkey_failed" };
  const allow: PublicKeyCredentialDescriptor[] = [];
  for (const p of usable.value) {
    const id = fromBase64url(p.passkey_id);
    if (id) allow.push({ type: "public-key", id: id.slice().buffer });
  }
  let credential: Credential | null;
  try {
    credential = await authenticator({ publicKey: { challenge: signed.slice().buffer, allowCredentials: allow, userVerification: "required", timeout: ASSERTION_TIMEOUT_MS } });
  } catch {
    return { ok: false, reason: "passkey_canceled" };
  }
  const answer = assertionOf(credential);
  if (!answer) return { ok: false, reason: "passkey_failed" };
  return { ok: true, value: { step_up: { assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "passkey" }, step_up_assertion: answer } };
}

/** The four values of a WebAuthn assertion, or `null` for an answer that is not one. */
function assertionOf(credential: Credential | null): StepUpAssertion | null {
  if (!credential || !isRecord(credential) || credential.type !== "public-key") return null;
  const raw = bytesOf((credential as unknown as { rawId?: unknown }).rawId);
  const response = (credential as unknown as { response?: unknown }).response;
  if (!raw || raw.length < 16 || !isRecord(response)) return null;
  const authenticatorData = bytesOf(response.authenticatorData);
  const clientData = bytesOf(response.clientDataJSON);
  const signature = bytesOf(response.signature);
  if (!authenticatorData || !clientData || !signature) return null;
  return { passkey_id: toBase64url(raw), authenticator_data: toBase64url(authenticatorData), client_data_json: toBase64url(clientData), signature: toBase64url(signature) };
}
