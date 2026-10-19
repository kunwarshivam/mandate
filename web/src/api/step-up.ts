/**
 * Step-up for a paper approval granted in the web app, by `cli_confirm` (workspace API spec §3.6,
 * DEC-2103, DEC-2104). The browser asks for a challenge bound to the one approval, shows its code and
 * the CLI line, and, once the owner has confirmed on the host, submits the grant with evidence naming
 * that challenge. The browser proves nothing itself: only the host's `StepUpConfirmed` does, which
 * the server reads, so a grant sent before the owner confirmed is refused `step_up_missing`.
 */
import type { WorkspaceClient } from "./client";
import { closedEnum, contentRef, object, string, timestamp } from "./decode";
import type { ContentRef, Decoder, IssueOutcome, PreparedCommand, StepUpEvidence } from "./types";

/** `StepUpAction`'s kinds that web/ asks for; `cli_confirm` covers `approve` only (DEC-2104 item 6.1). */
export const STEP_UP_KINDS = ["approve"] as const;
export type StepUpKind = (typeof STEP_UP_KINDS)[number];

/** `commands/step-up-challenge`'s 201, as far as the grant needs it. */
export interface StepUpChallenge {
  challenge_id: string;
  webauthn_challenge: string;
  action: { kind: StepUpKind; digest: ContentRef };
  issued_at: string;
  expires_at: string;
}

const EVENT_ID = /^[0-9A-HJKMNP-TV-Z]{26}$/;
const WEBAUTHN_CHALLENGE = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/;
const pattern =
  (re: RegExp): Decoder<string> =>
  (value, path) => {
    const read = string(value, path);
    return read.ok && !re.test(read.value) ? { ok: false, issue: { path, problem: "not_canonical", value: read.value, allowed: null } } : read;
  };

export const stepUpChallenge: Decoder<StepUpChallenge> = object<StepUpChallenge>({
  challenge_id: pattern(EVENT_ID),
  webauthn_challenge: pattern(WEBAUTHN_CHALLENGE),
  action: object<StepUpChallenge["action"]>({ kind: closedEnum(STEP_UP_KINDS), digest: contentRef }),
  issued_at: timestamp,
  expires_at: timestamp,
});

/** `POST /step-up/challenges` for one action; a retry of the prepared command answers the same challenge (§3.6). */
export function prepareStepUpChallenge(client: WorkspaceClient, kind: StepUpKind, digest: ContentRef): PreparedCommand {
  return client.prepare("step_up_challenge", {}, { action: { kind, digest } });
}

/** Asks for the challenge and checks it is bound to the action asked for, never another. */
export async function issueStepUpChallenge(client: WorkspaceClient, command: PreparedCommand, kind: StepUpKind, digest: ContentRef): Promise<IssueOutcome<StepUpChallenge>> {
  const issued = await client.issue(command, stepUpChallenge);
  if (issued.ok && (issued.value.action.kind !== kind || issued.value.action.digest !== digest)) {
    return { ok: false, error: { code: "invalid_response", effect: "unknown", status: null, event_id: null, retryable: false, violations: [], decode: { path: "/action", problem: "wrong_type", value: issued.value.action.digest, allowed: [digest] } } };
  }
  return issued;
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE64URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/** The first 40 bits of the unpadded base64url `webauthn_challenge`: its first 7 characters carry 42 bits. */
function first40Bits(webauthnChallenge: string): bigint | null {
  if (!WEBAUTHN_CHALLENGE.test(webauthnChallenge)) return null;
  let bits = 0n;
  for (const char of webauthnChallenge.slice(0, 7)) bits = (bits << 6n) | BigInt(BASE64URL.indexOf(char));
  return bits >> 2n;
}

/** DEC-2104 item 1's code, shown `XXXX-XXXX`: 40 bits as 8 Crockford base-32 characters. `null` for a challenge of any other shape. */
export function cliConfirmCode(webauthnChallenge: string): string | null {
  let bits = first40Bits(webauthnChallenge);
  if (bits === null) return null;
  let out = "";
  for (let i = 0; i < 8; i += 1) {
    out = CROCKFORD[Number(bits & 31n)] + out;
    bits >>= 5n;
  }
  return `${out.slice(0, 4)}-${out.slice(4)}`;
}

/** DEC-2104 item 5: the challenge's id, its `issued_at` in whole seconds, and the method. */
export function cliConfirmEvidence(challenge: StepUpChallenge): StepUpEvidence {
  return { assertion_id: challenge.challenge_id, authenticated_at: `${challenge.issued_at.slice(0, 19)}.000000000Z`, method: "cli_confirm" };
}

/** No route serves the session's principal yet, so the line names where it goes, never a guess. */
export const UNKNOWN_PRINCIPAL = "<your principal id>";

/** DEC-2104 item 2's command, which the owner runs on the host; paper only. */
export function cliConfirmLine(workspaceId: string, agentId: string, approvalId: string, challenge: StepUpChallenge): string {
  const code = cliConfirmCode(challenge.webauthn_challenge) ?? "";
  return `mandate step-up confirm --workspace ${workspaceId} --principal '${UNKNOWN_PRINCIPAL}' --agent ${agentId} --approval ${approvalId} --challenge ${challenge.challenge_id} --code ${code}`;
}
