/**
 * Step-up from the browser by `cli_confirm` (workspace API spec §3.6; DEC-2103, DEC-2104, DEC-2125):
 * the browser asks for a challenge bound to one action, shows the code made from it, and the owner
 * confirms that code on the host with `mandate step-up confirm`. Only the host appends the
 * confirmation, so the browser cannot make one; it then sends the grant with evidence naming the
 * challenge. Paper only, and only for an approval or an OAuth connect's start: the server refuses any
 * other kind as `step_up_method`.
 *
 * Nothing here is a secret or is stored: the code is the challenge's own first 40 bits, spent with
 * the challenge, which lives 300 s and verifies once (DEC-2104 item 1).
 */
import { closedEnum, contentRef, isRecord, timestamp } from "./decode";
import type { ContentRef, Decoded, DecodeProblem, Decoder, Timestamp } from "./types";
import { sha256Hex } from "@/lib/sha256";

/** `envelope.schema.json`'s `StepUpAction` kinds the host confirms by `cli_confirm`. */
export const CLI_CONFIRM_KINDS = ["approve", "connection"] as const;
export type CliConfirmKind = (typeof CLI_CONFIRM_KINDS)[number];

/** The kinds a passkey confirms from the browser: a version's confirmation and a deployment (DEC-2128), and a later passkey's enrolment (DEC-2108 item 5). */
export const PASSKEY_KINDS = ["confirm_version", "deploy", "credential_enrol", "resume", "stop"] as const;
export type PasskeyKind = (typeof PASSKEY_KINDS)[number];

/** Every `StepUpAction` kind this client asks for. */
export type StepUpKind = CliConfirmKind | PasskeyKind;

/** The one action a challenge is bound to (§3.6): its kind and the SHA-256 of its canonical object. */
export interface StepUpAction {
  kind: StepUpKind;
  digest: ContentRef;
}

/** `commands/step-up-challenge`'s members (DEC-2103 item 3). */
export interface StepUpChallenge {
  challenge_id: string;
  webauthn_challenge: string;
  action: StepUpAction;
  issued_at: Timestamp;
  expires_at: Timestamp;
}

/** `envelope.schema.json`'s `StepUpEvidence`, as a `cli_confirm` grant carries it (DEC-2104 item 5). */
export interface StepUpEvidence {
  assertion_id: string;
  authenticated_at: Timestamp;
  method: "cli_confirm";
}

/** A challenge the server answered for one action, with the code the owner types on the host. */
export interface ShownChallenge {
  challenge: StepUpChallenge;
  /** `XXXX-XXXX` (DEC-2104 item 1). */
  code: string;
}

/** `common.schema.json`'s `EventId`: a ULID in Crockford base32. */
const EVENT_ID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;
/** 32 bytes as unpadded base64url: 43 characters, the last carrying 4 bits (`step-up-challenge.schema.json`). */
const WEBAUTHN_CHALLENGE = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/;
const BASE64URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/** The code's characters: 40 bits of 5 bits each (DEC-2104 item 1). */
const CODE_LEN = 8;
const CODE_BITS = 40;

const fail = (path: string, problem: DecodeProblem, value: string | null = null, allowed: readonly string[] | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed } });

const kind = closedEnum<StepUpKind>([...CLI_CONFIRM_KINDS, ...PASSKEY_KINDS]);

/**
 * `commands/step-up-challenge`, decoded against the action it was asked for: an answer bound to any
 * other kind or digest is refused, so a code is never shown for an action the owner did not ask
 * about. Unknown members are ignored (spec §3.2); the envelope is the client's.
 */
export function stepUpChallenge(asked: StepUpAction): Decoder<StepUpChallenge> {
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    const id = value.challenge_id;
    if (typeof id !== "string") return fail(`${path}/challenge_id`, id === undefined ? "missing" : "wrong_type");
    if (!EVENT_ID.test(id)) return fail(`${path}/challenge_id`, "not_canonical", id);
    const bytes = value.webauthn_challenge;
    if (typeof bytes !== "string") return fail(`${path}/webauthn_challenge`, bytes === undefined ? "missing" : "wrong_type");
    if (!WEBAUTHN_CHALLENGE.test(bytes)) return fail(`${path}/webauthn_challenge`, "not_canonical", bytes);
    const action = value.action;
    if (!isRecord(action)) return fail(`${path}/action`, action === undefined ? "missing" : "wrong_type");
    const served = kind(action.kind, `${path}/action/kind`);
    if (!served.ok) return served;
    if (served.value !== asked.kind) return fail(`${path}/action/kind`, "unknown_enum_value", served.value, [asked.kind]);
    const digest = contentRef(action.digest, `${path}/action/digest`);
    if (!digest.ok) return digest;
    if (digest.value !== asked.digest) return fail(`${path}/action/digest`, "not_canonical", digest.value);
    const issued = timestamp(value.issued_at, `${path}/issued_at`);
    if (!issued.ok) return issued;
    const expires = timestamp(value.expires_at, `${path}/expires_at`);
    if (!expires.ok) return expires;
    if (expires.value <= issued.value) return fail(`${path}/expires_at`, "not_canonical", expires.value);
    return { ok: true, value: { challenge_id: id, webauthn_challenge: bytes, action: { kind: served.value, digest: digest.value }, issued_at: issued.value, expires_at: expires.value } };
  };
}

/**
 * The code for a challenge (DEC-2104 item 1): the first 40 bits of `webauthn_challenge`'s 32 bytes as
 * 8 Crockford base-32 characters, shown `XXXX-XXXX`, the server's `confirm::code` and the CLI's
 * normalized form. `null` for a value that is not a challenge's.
 */
export function confirmCode(webauthnChallenge: string): string | null {
  if (!WEBAUTHN_CHALLENGE.test(webauthnChallenge)) return null;
  let bits = 0n;
  let width = 0;
  for (const char of webauthnChallenge) {
    bits = (bits << 6n) | BigInt(BASE64URL.indexOf(char));
    width += 6;
    if (width >= CODE_BITS) break;
  }
  bits >>= BigInt(width - CODE_BITS);
  let code = "";
  for (let digit = CODE_LEN - 1; digit >= 0; digit -= 1) code += CROCKFORD[Number((bits >> BigInt(digit * 5)) & 31n)];
  return `${code.slice(0, 4)}-${code.slice(4)}`;
}

/**
 * The evidence a grant carries once the owner has confirmed on the host (DEC-2104 item 5): the
 * challenge's id, and its `issued_at` exactly as served, which the server compares as an instant.
 */
export function cliConfirmEvidence(challenge: StepUpChallenge): StepUpEvidence {
  return { assertion_id: challenge.challenge_id, authenticated_at: challenge.issued_at, method: "cli_confirm" };
}

/** A §4.7 timestamp as milliseconds since the epoch, its sub-millisecond digits dropped. */
function epochMs(timestamp: Timestamp): number {
  return Date.parse(`${timestamp.slice(0, 23)}Z`);
}

/**
 * How long `challenge` lives, from `issued_at` to `expires_at` (DEC-2103 item 5: 300 s). The screen
 * counts it from when the answer arrived, never against this device's clock, which may differ from
 * the server's; the server judges the window itself (DEC-2104 item 6 check 4).
 */
export function challengeLifetimeMs(challenge: StepUpChallenge): number {
  return Math.max(0, epochMs(challenge.expires_at) - epochMs(challenge.issued_at));
}

/** What the approval's host command names (DEC-2104 item 2). The journal's DSN and store stay the operator's. */
export interface ApprovalConfirm {
  workspaceId: string;
  principalId: string;
  agentId: string;
  approvalId: string;
  shown: ShownChallenge;
}

/**
 * The command the owner runs on the host for an approval (DEC-2104 item 2). It reads the approval
 * from the control stream and shows it, so the owner confirms what the journal holds; `--journal`
 * and `--store` are the deployment's, left for the operator to fill in.
 */
export function approvalConfirmCommand(input: ApprovalConfirm): string {
  return [
    "mandate step-up confirm",
    `--workspace ${input.workspaceId}`,
    `--principal ${input.principalId}`,
    `--agent ${input.agentId}`,
    `--approval ${input.approvalId}`,
    `--challenge ${input.shown.challenge.challenge_id}`,
    `--code ${input.shown.code}`,
    "--journal <journal DSN> --store <artifact store>",
  ].join(" ");
}

/** The token-breadth disclosure the owner confirms with a connect's step-up (DEC-2621 item 5, DEC-2622 version 1). */
export const CONNECT_DISCLOSURE = { document: "alpaca_oauth_token_breadth", version: "1" } as const;

/**
 * The digest a connect's step-up binds (DEC-2621 item 4): SHA-256 of the canonical `{action:
 * "connect", broker, disclosure: {document, version}, environment: "paper", workspace_id}`. Its
 * members are written in journal spec §4's key order; every value is a plain string, so this is the
 * canonical form `oauth_connect_object` writes.
 */
export function connectStepUpDigest(workspaceId: string): ContentRef {
  const canonical = JSON.stringify({ action: "connect", broker: "alpaca", disclosure: { document: CONNECT_DISCLOSURE.document, version: CONNECT_DISCLOSURE.version }, environment: "paper", workspace_id: workspaceId });
  return `sha256:${sha256Hex(canonical)}`;
}

/** What a connect's host command names (DEC-2125 item 3): the command computes the digest itself. */
export interface ConnectConfirm {
  workspaceId: string;
  principalId: string;
  shown: ShownChallenge;
}

/** The command the owner runs on the host for a paper connect (`mandate step-up confirm-connect`, DEC-2125 item 3). */
export function connectConfirmCommand(input: ConnectConfirm): string {
  return [
    "mandate step-up confirm-connect",
    `--workspace ${input.workspaceId}`,
    `--principal ${input.principalId}`,
    `--challenge ${input.shown.challenge.challenge_id}`,
    `--code ${input.shown.code}`,
    "--journal <journal DSN> --store <artifact store>",
  ].join(" ");
}

/** Staging's step-up workflow (lane GO's `step-up.yml`): the founder dispatches it from main with one input, `confirm`. */
export const STEP_UP_WORKFLOW_URL = "https://github.com/kunwarshivam/mandate/actions/workflows/step-up.yml";
/** The ids the workflow accepts: 1 to 128 of `[A-Za-z0-9_-]`, so no `:` or space can appear in one. */
const WORKFLOW_ID = /^[A-Za-z0-9_-]{1,128}$/;
/** `XXXX-XXXX`, as shown. */
const WORKFLOW_CODE = /^[0-9A-Z]{4}-[0-9A-Z]{4}$/;

function workflowInput(kind: "approve" | "connect", shown: ShownChallenge, ids: readonly string[]): string | null {
  if (!EVENT_ID.test(shown.challenge.challenge_id) || !WORKFLOW_CODE.test(shown.code) || !ids.every((id) => WORKFLOW_ID.test(id))) return null;
  return [kind, shown.challenge.challenge_id, shown.code, ...ids].join(":");
}

/**
 * The step-up workflow's one input for an approval's grant, `approve:<challenge>:<code>:<agent>:<approval>`:
 * the same confirmation `approvalConfirmCommand` runs, as a staging Job. The workspace and the
 * principal come from staging's values, never from the string. `null` when an id is outside what the
 * workflow accepts, so a string it would refuse is never shown.
 */
export function approveWorkflowInput(input: Pick<ApprovalConfirm, "agentId" | "approvalId" | "shown">): string | null {
  return workflowInput("approve", input.shown, [input.agentId, input.approvalId]);
}

/** The step-up workflow's one input for a connect's start, `connect:<challenge>:<code>`. */
export function connectWorkflowInput(shown: ShownChallenge): string | null {
  return workflowInput("connect", shown, []);
}

/** `read-models/session`'s `principal_id` (§4.10, DEC-2113): the id the CLI's `--principal` takes. */
export const sessionPrincipal: Decoder<{ principal_id: string }> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const id = value.principal_id;
  if (typeof id !== "string") return fail(`${path}/principal_id`, id === undefined ? "missing" : "wrong_type");
  if (!EVENT_ID.test(id)) return fail(`${path}/principal_id`, "not_canonical", id);
  return { ok: true, value: { principal_id: id } };
};
