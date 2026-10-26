/**
 * Enrolling a workspace passkey from the browser (workspace API spec §3.6's passkey routes; DEC-2108
 * items 4 and 5, DEC-2115, DEC-2118): `POST /passkeys/registration-challenges` answers a registration
 * challenge, the authenticator makes a credential over it (`navigator.credentials.create`, user
 * verification required, attestation `none`), and `POST /passkeys` sends its id, attestation object
 * and client data. A member who already holds a passkey confirms the enrolment with one, a step-up of
 * kind `credential_enrol` bound to `{"op": "enrol", "registration": <challenge_id>}`; a first
 * enrolment's evidence is a fresh full session instead (300 s, DEC-2108 item 5).
 *
 * The private key never leaves the authenticator, and nothing here is stored: the server keeps the
 * public key, and the browser keeps nothing.
 */
import { array, closedEnum, isRecord, nullable, timestamp } from "./decode";
import { type PasskeyStepUp, fromBase64url, toBase64url } from "./passkey";
import type { ContentRef, Decoded, DecodeProblem, Decoder, JsonObject, Timestamp } from "./types";
import { sha256Hex } from "@/lib/sha256";

/** The registration challenge route (§3.6, operation `passkey_registration_challenge`); it records nothing. */
export const REGISTRATION_ROUTE = "/passkeys/registration-challenges";
/** The enrol route (§3.6, operation `passkey_enrol`); it commits `CredentialEnrolled`. */
export const ENROL_ROUTE = "/passkeys";

/** `commands/passkey-registration-challenge`. */
export interface RegistrationChallenge {
  challenge_id: string;
  webauthn_challenge: string;
  rp_id: string;
  user_handle: string;
  exclude: string[];
  step_up_needed: boolean;
  issued_at: Timestamp;
  expires_at: Timestamp;
}

/** `commands/passkey-enrolled`. */
export interface Enrolled {
  event_id: string;
  passkey_id: string;
  alg: "es256" | "eddsa";
  cool_off_ends: Timestamp | null;
}

const fail = (path: string, problem: DecodeProblem, value: string | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed: null } });

const EVENT_ID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;
const WEBAUTHN_CHALLENGE = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/;
const RP_ID = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$/;
const USER_HANDLE = /^[A-Za-z0-9_-]{21}[AQgw]$/;
const PASSKEY_ID = /^(?:[A-Za-z0-9_-]{4})*(?:[A-Za-z0-9_-]{2,3})?$/;

const patterned =
  (pattern: RegExp, max = 1364): Decoder<string> =>
  (value, path) => {
    if (typeof value !== "string") return fail(path, value === undefined ? "missing" : "wrong_type");
    return value.length <= max && pattern.test(value) ? { ok: true, value } : fail(path, "not_canonical", value);
  };
const passkeyId: Decoder<string> = (value, path) => {
  const read = patterned(PASSKEY_ID)(value, path);
  return read.ok && read.value.length < 22 ? fail(path, "not_canonical", read.value) : read;
};
const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path, value === undefined ? "missing" : "wrong_type"));

/**
 * `commands/passkey-registration-challenge`. The RP ID is the schema's dotted name, or `localhost`
 * for a dev cell (DEC-2132). Unknown members are ignored (§3.2).
 */
export const registrationChallenge: Decoder<RegistrationChallenge> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const members = [
    ["challenge_id", patterned(EVENT_ID, 26)],
    ["webauthn_challenge", patterned(WEBAUTHN_CHALLENGE, 43)],
    ["rp_id", patterned(RP_ID, 253)],
    ["user_handle", patterned(USER_HANDLE, 22)],
    ["exclude", array(passkeyId)],
    ["step_up_needed", flag],
    ["issued_at", timestamp],
    ["expires_at", timestamp],
  ] as const;
  const out: Record<string, unknown> = {};
  for (const [name, decode] of members) {
    const read = (decode as Decoder<unknown>)(value[name], `${path}/${name}`);
    if (!read.ok) return read;
    out[name] = read.value;
  }
  return { ok: true, value: out as unknown as RegistrationChallenge };
};

/** `commands/passkey-enrolled`. */
export const enrolled: Decoder<Enrolled> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const id = patterned(EVENT_ID, 26)(value.event_id, `${path}/event_id`);
  if (!id.ok) return id;
  const key = passkeyId(value.passkey_id, `${path}/passkey_id`);
  if (!key.ok) return key;
  const alg = closedEnum(["es256", "eddsa"] as const)(value.alg, `${path}/alg`);
  if (!alg.ok) return alg;
  const cool = nullable(timestamp)(value.cool_off_ends, `${path}/cool_off_ends`);
  if (!cool.ok) return cool;
  return { ok: true, value: { event_id: id.value, passkey_id: key.value, alg: alg.value, cool_off_ends: cool.value } };
};

/**
 * The digest a later enrolment's step-up binds (§3.6): SHA-256 of `{"op": "enrol", "registration":
 * <challenge_id>}` in the journal's canonical form; both values are plain ASCII, so this is it.
 */
export function enrolStepUpDigest(challengeId: string): ContentRef {
  return `sha256:${sha256Hex(JSON.stringify({ op: "enrol", registration: challengeId }))}`;
}

/** The browser's WebAuthn create call, injectable so a test can stand in for an authenticator. */
export type Creator = (options: CredentialCreationOptions) => Promise<Credential | null>;

export function browserCreator(): Creator | null {
  if (typeof window === "undefined" || typeof window.PublicKeyCredential === "undefined" || !navigator.credentials) return null;
  return (options) => navigator.credentials.create(options);
}

/** How long the authenticator may take, well inside the challenge's 300 s. */
export const CREATE_TIMEOUT_MS = 120_000;

/** What the authenticator made, as the enrol request carries it. */
export interface MadeCredential {
  passkey_id: string;
  attestation_object: string;
  client_data_json: string;
}

export type CreateAnswer = { ok: true; value: MadeCredential } | { ok: false; reason: "passkey_unavailable" | "passkey_canceled" | "passkey_failed" };

/** A buffer as bytes, whichever realm made it. */
const bytesOf = (buffer: unknown): Uint8Array | null =>
  Object.prototype.toString.call(buffer) === "[object ArrayBuffer]"
    ? new Uint8Array(buffer as ArrayBuffer)
    : ArrayBuffer.isView(buffer)
      ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
      : null;

/**
 * Asks the authenticator for a new credential over `challenge`: the deployment's RP ID, the user
 * handle it named, ES256 or Ed25519 (the algorithms the server enrols), every passkey the member
 * holds excluded, user verification required, and no attestation. `name` is what the authenticator
 * shows; it is the member's opaque id, never an email.
 */
export async function createPasskey(challenge: RegistrationChallenge, name: string, creator: Creator | null): Promise<CreateAnswer> {
  if (!creator) return { ok: false, reason: "passkey_unavailable" };
  const signed = fromBase64url(challenge.webauthn_challenge);
  const handle = fromBase64url(challenge.user_handle);
  if (!signed || signed.length !== 32 || !handle || handle.length !== 16) return { ok: false, reason: "passkey_failed" };
  const exclude: PublicKeyCredentialDescriptor[] = [];
  for (const id of challenge.exclude) {
    const bytes = fromBase64url(id);
    if (bytes) exclude.push({ type: "public-key", id: bytes.slice().buffer });
  }
  let credential: Credential | null;
  try {
    credential = await creator({
      publicKey: {
        challenge: signed.slice().buffer,
        rp: { id: challenge.rp_id, name: "Owlhead" },
        user: { id: handle.slice().buffer, name, displayName: name },
        pubKeyCredParams: [
          { type: "public-key", alg: -7 },
          { type: "public-key", alg: -8 },
        ],
        excludeCredentials: exclude,
        authenticatorSelection: { userVerification: "required", residentKey: "preferred" },
        attestation: "none",
        timeout: CREATE_TIMEOUT_MS,
      },
    });
  } catch {
    return { ok: false, reason: "passkey_canceled" };
  }
  if (!credential || !isRecord(credential) || credential.type !== "public-key") return { ok: false, reason: "passkey_failed" };
  const raw = bytesOf((credential as unknown as { rawId?: unknown }).rawId);
  const response = (credential as unknown as { response?: unknown }).response;
  if (!raw || raw.length < 16 || raw.length > 1023 || !isRecord(response)) return { ok: false, reason: "passkey_failed" };
  const attestation = bytesOf(response.attestationObject);
  const client = bytesOf(response.clientDataJSON);
  if (!attestation || !client) return { ok: false, reason: "passkey_failed" };
  return { ok: true, value: { passkey_id: toBase64url(raw), attestation_object: toBase64url(attestation), client_data_json: toBase64url(client) } };
}

/** The `passkey_enrol` request (`commands/passkey-enrol-request`), exactly its six members. */
export function enrolBody(challenge: RegistrationChallenge, made: MadeCredential, stepUp: PasskeyStepUp | null): JsonObject {
  return {
    challenge_id: challenge.challenge_id,
    passkey_id: made.passkey_id,
    attestation_object: made.attestation_object,
    client_data_json: made.client_data_json,
    step_up: stepUp ? { ...stepUp.step_up } : null,
    step_up_assertion: stepUp ? { ...stepUp.step_up_assertion } : null,
  };
}

/** One of the member's passkeys as `GET /passkeys` lists it (`read-models/passkeys`). */
export interface ListedPasskey {
  passkey_id: string;
  alg: "es256" | "eddsa";
  enrolled_at: Timestamp;
  cool_off_ends: Timestamp | null;
}

/** `read-models/passkeys`, every member of each passkey, for the list the member sees. */
export const passkeyList: Decoder<{ passkeys: ListedPasskey[] }> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const one: Decoder<ListedPasskey> = (item, at) => {
    if (!isRecord(item)) return fail(at, "wrong_type");
    const id = passkeyId(item.passkey_id, `${at}/passkey_id`);
    if (!id.ok) return id;
    const alg = closedEnum(["es256", "eddsa"] as const)(item.alg, `${at}/alg`);
    if (!alg.ok) return alg;
    const since = timestamp(item.enrolled_at, `${at}/enrolled_at`);
    if (!since.ok) return since;
    const cool = nullable(timestamp)(item.cool_off_ends, `${at}/cool_off_ends`);
    if (!cool.ok) return cool;
    return { ok: true, value: { passkey_id: id.value, alg: alg.value, enrolled_at: since.value, cool_off_ends: cool.value } };
  };
  const list = array(one)(value.passkeys, `${path}/passkeys`);
  return list.ok ? { ok: true, value: { passkeys: list.value } } : list;
};
