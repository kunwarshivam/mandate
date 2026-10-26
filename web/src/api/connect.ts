/**
 * The Alpaca paper connect over the workspace API (§4.5; DEC-2620, DEC-2621, DEC-2623, DEC-2626):
 * the start, `POST /connections/oauth/start`, which commits the pending connection with the step-up it
 * spent and answers the authorization page to send the browser to; and the connection reads the
 * callback's redirect lands on, `GET /connections/{id}`. The browser never sees the code, the
 * `state`, or a token: Alpaca sends the browser to the API's callback, which redirects to the app.
 * Paper only.
 */
import { array, closedEnum, isRecord, nullable, object, string, timestamp } from "./decode";
import { CONNECT_DISCLOSURE, type StepUpChallenge, cliConfirmEvidence } from "./step-up";
import type { Decoded, DecodeProblem, Decoder, Timestamp } from "./types";

/** The route of the start (§4.5). */
export const CONNECT_START_ROUTE = "/connections/oauth/start";

/** `commands/oauth-start-request`, as the web app sends it: Alpaca, paper, the disclosure shown, and the confirmed step-up. */
export interface ConnectStartBody {
  broker: "alpaca";
  environment: "paper";
  disclosure: { document: string; version: string };
  step_up: ReturnType<typeof cliConfirmEvidence>;
}

/** The start's body for a challenge of kind `connection` the owner confirmed on the host; `null` for any other challenge. */
export function connectStartBody(challenge: StepUpChallenge): ConnectStartBody | null {
  if (challenge.action.kind !== "connection") return null;
  return { broker: "alpaca", environment: "paper", disclosure: { document: CONNECT_DISCLOSURE.document, version: CONNECT_DISCLOSURE.version }, step_up: cliConfirmEvidence(challenge) };
}

/** `commands/oauth-start-response` (DEC-2623 item 1). */
export interface ConnectStarted {
  authorization_url: string;
  expires_at: Timestamp;
  connection_id: string;
}

const fail = (path: string, problem: DecodeProblem, value: string | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed: null } });

/** `common.schema.json`'s `Id`. */
const ID = /^[A-Za-z0-9_-]{1,128}$/;
const id: Decoder<string> = (value, path) => (typeof value !== "string" ? fail(path, value === undefined ? "missing" : "wrong_type") : ID.test(value) ? { ok: true, value } : fail(path, "not_canonical", value));

/** An `https` URL with no whitespace (the schema's pattern), parsed: the one place the browser is sent off the app. */
const authorizationUrl: Decoder<string> = (value, path) => {
  if (typeof value !== "string") return fail(path, value === undefined ? "missing" : "wrong_type");
  if (!/^https:\/\/[^\s]+$/.test(value)) return fail(path, "not_canonical", null);
  try {
    return new URL(value).protocol === "https:" ? { ok: true, value } : fail(path, "not_canonical", null);
  } catch {
    return fail(path, "not_canonical", null);
  }
};

export const connectStarted: Decoder<ConnectStarted> = object<ConnectStarted>({ authorization_url: authorizationUrl, expires_at: timestamp, connection_id: id });

/** `read-models/connection`'s states (DEC-2620). Closed. */
export const CONNECTION_STATES = ["connecting", "established", "refused", "revoked"] as const;
export type ConnectionState = (typeof CONNECTION_STATES)[number];

export interface ConnectionRefusal {
  check: "scope" | "environment" | "account" | "uniqueness" | "contract" | null;
  reason: string;
}

/** `read-models/connection`'s `Connection` (DEC-2620 items 2 and 3). */
export interface ConnectionDetail {
  connection_id: string;
  broker: string;
  environment: "paper" | "live";
  state: ConnectionState;
  scopes: string[] | null;
  since: Timestamp;
  refusal: ConnectionRefusal | null;
  revoked_reason: "owner" | "compromised" | null;
  agents: string[];
}

const refusal: Decoder<ConnectionRefusal> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const check = nullable(closedEnum(["scope", "environment", "account", "uniqueness", "contract"] as const))(value.check ?? null, `${path}/check`);
  if (!check.ok) return check;
  const reason = value.reason;
  if (typeof reason !== "string" || !/^[a-z][a-z_]*$/.test(reason)) return fail(`${path}/reason`, typeof reason === "string" ? "not_canonical" : "wrong_type");
  return { ok: true, value: { check: check.value, reason } };
};

/** One connection, decoded as its schema states it: a refusal exactly when refused, a revoke reason only when revoked. */
export const connectionDetail: Decoder<ConnectionDetail> = (value, path) => {
  const read = object<ConnectionDetail>({
    connection_id: id,
    broker: string,
    environment: closedEnum(["paper", "live"] as const),
    state: closedEnum(CONNECTION_STATES),
    scopes: nullable(array(string)),
    since: timestamp,
    refusal: nullable(refusal),
    revoked_reason: nullable(closedEnum(["owner", "compromised"] as const)),
    agents: array(id),
  })(value, path);
  if (!read.ok) return read;
  const c = read.value;
  if ((c.state === "refused") !== (c.refusal !== null)) return fail(`${path}/refusal`, "not_canonical");
  if (c.state !== "revoked" && c.revoked_reason !== null) return fail(`${path}/revoked_reason`, "not_canonical");
  return read;
};

export const connectionRead: Decoder<{ connection: ConnectionDetail }> = object({ connection: connectionDetail });
export const connectionsRead: Decoder<{ connections: ConnectionDetail[] }> = object({ connections: array(connectionDetail) });

/** The connection id the start and the callback name: `conn_` and a ULID (DEC-2623 item 5). */
export const API_CONNECTION_ID = /^conn_[0-9A-HJKMNP-TV-Z]{26}$/;
