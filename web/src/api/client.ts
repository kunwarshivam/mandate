/**
 * The typed workspace API client's foundation (E11-9, with E10-10's conventions; workspace API spec
 * §3, §5, §6). Every path is under `/v1/workspaces/{ws}`; every command carries one
 * `Idempotency-Key` per gesture; every failure decodes to `{code, effect}`, and a call whose
 * outcome could not be confirmed is `effect: "unknown"`, never success and never "nothing happened"
 * (API-13). It has no order-placing method (DEC-528).
 */
import { array, closedEnum, integer, isRecord, nullable, object, string, timestamp, watermark } from "./decode";
import {
  COMMAND_PHASES,
  EFFECTS,
  STEP_UP_STATUSES,
  type Accepted,
  type ApiError,
  type CommandOutcome,
  type CommandStatus,
  type CommandStep,
  type DecodeIssue,
  type Decoder,
  type Effect,
  type JsonObject,
  type KillSwitchRequest,
  type Operation,
  type PreparedCommand,
  type ReadOutcome,
  type Violation,
  type Watermark,
} from "./types";
import { CONNECT_START_ROUTE, type ConnectStartBody, type ConnectStarted, connectStarted } from "./connect";
import { ENROL_ROUTE, type Enrolled, REGISTRATION_ROUTE, type RegistrationChallenge, enrolled, registrationChallenge } from "./enrol";
import { type StepUpAction, type StepUpChallenge, sessionPrincipal, stepUpChallenge } from "./step-up";

export type Fetch = (input: string, init: RequestInit) => Promise<Response>;

export interface ClientOptions {
  workspaceId: string;
  fetch: Fetch;
  /**
   * Origin of the API; empty for the app's own origin. Another origin is called with
   * `credentials: "include"`, so the browser sends the API's own session cookie (spec §3.3 item 5).
   */
  baseUrl?: string;
  /** A fresh `Idempotency-Key` per gesture; tests pass a deterministic one. */
  newKey?: () => string;
  /** Called on a 401 `unauthenticated`, never on `step_up_required`. */
  onUnauthenticated?: () => void;
  /** The shared envelope and problem shapes; see `CommonSchema`. */
  schema?: Partial<CommonSchema>;
}

/** What every read carries (spec §3.2, §6.1). */
export interface ReadEnvelope {
  api_version: string;
  as_of: Watermark[];
}

/** The §3.5 members a problem document states; `null` where it does not state one. */
export interface ProblemFields {
  code: string | null;
  effect: Effect | null;
  event_id: string | null;
  retryable: boolean | null;
  violations: Violation[];
}

/**
 * The seam for the types generated from `schemas/workspace-api/common/`. The defaults follow only
 * the prose of spec §3.5 and §6.1; the generated decoders replace them when the schemas land. Either
 * way the client, not the decoder, decides the fallback: an unstated effect on a command is
 * `unknown`.
 */
export interface CommonSchema {
  envelope: Decoder<ReadEnvelope>;
  problem: (body: unknown) => ProblemFields;
}

export interface WorkspaceClient {
  readonly workspaceId: string;
  /** `/v1/workspaces/{ws}` followed by `route`. */
  path(route: `/${string}`): string;
  read<T>(route: `/${string}`, decoder: Decoder<T>): Promise<ReadOutcome<T>>;
  /** Fixes the key, path and body of one gesture; `params` fill the operation's route. */
  prepare(operation: Operation, params: Record<string, string>, body: JsonObject): PreparedCommand;
  /** The kill switch (spec §5.4). Reads nothing first, so it works before any read has loaded. */
  prepareKillSwitch(request: KillSwitchRequest): PreparedCommand;
  /** Sends a prepared command; sending it again is a retry with the same key and body. */
  send(command: PreparedCommand): Promise<CommandOutcome>;
  commandStatus(eventId: string): Promise<ReadOutcome<CommandStatus>>;
  /**
   * `POST /step-up/challenges` (spec §3.6, DEC-2103): one challenge bound to `action`, under a key of
   * its own. Nothing is journaled when a challenge is issued, so a failure is `effect: "none"`.
   */
  askChallenge(action: StepUpAction): Promise<ReadOutcome<StepUpChallenge>>;
  /** `GET /v1/me/session` (§4.10, DEC-2113): the session's own principal, outside the workspace prefix. */
  readSession(): Promise<ReadOutcome<{ principal_id: string }>>;
  /** Fixes the key and body of one connect's start (§4.5, DEC-2623); sending it again is a retry that answers the same start (item 8). */
  prepareConnect(body: ConnectStartBody): PreparedConnect;
  /**
   * Sends a prepared start. It commits `ConnectionRequested`, so an answer that does not say nothing
   * was recorded is `effect: "unknown"`, never "nothing happened" (API-13).
   */
  sendConnect(prepared: PreparedConnect): Promise<ConnectOutcome>;
  /**
   * `POST /agents/{id}/go-live` (§4.2): the owner's request to move an agent to real money. Until
   * counsel signs off it is always refused `live_unavailable`, and while the workspace's real-money
   * gate is off the refusal is journaled first (DEC-2134). An answer that is not a refusal is
   * reported as unknown, never as live.
   */
  askRealMoney(agentId: string): Promise<GoLiveOutcome>;
  /** `POST /passkeys/registration-challenges` (§3.6): a registration challenge under a key of its own; it records nothing. */
  askRegistration(): Promise<ReadOutcome<RegistrationChallenge>>;
  /**
   * `POST /passkeys` (§3.6): the made credential. It commits `CredentialEnrolled`, so a lost answer is
   * `effect: "unknown"`, and the screen reads the member's passkeys again rather than send it twice.
   */
  enrolPasskey(body: JsonObject): Promise<ReadOutcome<Enrolled>>;
}

/** What a go-live request came to: always a refusal, or an answer the client could not read. */
export type GoLiveOutcome = { ok: false; error: ApiError };

/** One connect's start: its key and body, fixed when prepared. */
export interface PreparedConnect {
  readonly idempotency_key: string;
  readonly body: string;
}

export type ConnectOutcome = { ok: true; value: ConnectStarted } | { ok: false; error: ApiError };

/** The step-up challenge route under the workspace (§3.6). It is not a command: it records nothing. */
export const STEP_UP_CHALLENGE_ROUTE = "/step-up/challenges";
/** The owner's request to move an agent to real money (§4.2, DEC-2134). */
const GO_LIVE_ROUTE = "/agents/{agent}/go-live";
/** One's own session (§4.10). */
export const SESSION_ROUTE = "/v1/me/session";

/** Each operation's route under the workspace (spec §4.2 to §4.4, §5). */
export const OPERATIONS: Readonly<Record<Operation, `/${string}`>> = {
  pause: "/agents/{agent}/pause",
  resume: "/agents/{agent}/resume",
  hold_openings: "/agents/{agent}/hold",
  stop: "/agents/{agent}/stop",
  owner_exit: "/agents/{agent}/exits",
  acknowledge: "/agents/{agent}/acknowledgments",
  kill_switch: "/kill-switch",
  approval_response: "/approvals/{approval}/responses",
  end_delegation: "/agents/{agent}/delegations/{delegation}/end",
  confirm_version: "/mandate-versions/{version}/confirm",
  create_version: "/mandate-versions",
  deploy: "/agents",
};

/** Spec §3.3's CSRF defence (L2's pin, DEC-681): a header a cross-site form cannot set, on every command. */
export const CSRF_HEADER = "X-Mandate-Request";
const IDEMPOTENCY_KEY = /^[A-Za-z0-9_-]{16,64}$/;
const WORKSPACE_ID = /^[A-Za-z0-9_-]+$/;
const KEY_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const KEY_LENGTH = 32;

/** Spec §3.4: 16 to 64 characters from `[A-Za-z0-9_-]`. */
export function isIdempotencyKey(key: string): boolean {
  return IDEMPOTENCY_KEY.test(key);
}

/** 32 characters of 64 symbols each: 192 random bits from the platform's CSPRNG. */
export function newIdempotencyKey(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(KEY_LENGTH));
  return Array.from(bytes, (b) => KEY_ALPHABET[b & 63]).join("");
}

const effectOf = closedEnum(EFFECTS);
const violationsOf = array(object<Violation>({ path: string, code: string, message: string }));
const PROSE_SCHEMA: CommonSchema = {
  envelope: object<ReadEnvelope>({ api_version: string, as_of: array(watermark) }),
  problem: (body) => {
    const doc = isRecord(body) ? body : {};
    const effect = effectOf(doc.effect, "/effect");
    const violations = violationsOf(doc.violations ?? [], "/violations");
    return {
      code: typeof doc.code === "string" ? doc.code : null,
      effect: effect.ok ? effect.value : null,
      event_id: typeof doc.event_id === "string" ? doc.event_id : null,
      retryable: typeof doc.retryable === "boolean" ? doc.retryable : null,
      violations: violations.ok ? violations.value : [],
    };
  },
};
const acceptedPhase = closedEnum(["recorded"]);
const stepUpStatus = nullable(closedEnum(STEP_UP_STATUSES));
const commandStatusOf = object<CommandStatus>({
  phase: closedEnum(COMMAND_PHASES),
  steps: array(object<CommandStep>({ stream_id: string, seq: integer, event_type: string, recorded_at: timestamp, reason: nullable(string) })),
});

/** The command's event id, from `command_id` (spec §5.1, §5.4) or `response_id` (§5.2). */
function eventIdOf(body: unknown): string | null {
  if (!isRecord(body)) return null;
  const id = body.command_id ?? body.response_id;
  return typeof id === "string" ? id : null;
}

function decodeAccepted(body: unknown): { ok: true; value: Accepted } | { ok: false; issue: DecodeIssue } {
  if (!isRecord(body)) return { ok: false, issue: { path: "", problem: "wrong_type", value: null, allowed: null } };
  const eventId = eventIdOf(body);
  if (eventId === null) return { ok: false, issue: { path: "/command_id", problem: "missing", value: null, allowed: null } };
  const phase = acceptedPhase(body.phase, "/phase");
  if (!phase.ok) return phase;
  const status = stepUpStatus(body.step_up_status ?? null, "/step_up_status");
  if (!status.ok) return status;
  return { ok: true, value: { event_id: eventId, phase: phase.value, step_up_status: status.value } };
}

async function parseBody(response: Response): Promise<unknown> {
  try {
    const text = await response.text();
    return text === "" ? undefined : (JSON.parse(text) as unknown);
  } catch {
    return undefined;
  }
}

const UNREADABLE: DecodeIssue = { path: "", problem: "wrong_type", value: null, allowed: null };

function networkError(effect: Effect): ApiError {
  return { code: "network", effect, status: null, event_id: null, retryable: true, violations: [], decode: null };
}

function invalidResponse(status: number, effect: Effect, issue: DecodeIssue, eventId: string | null): ApiError {
  return { code: "invalid_response", effect, status, event_id: eventId, retryable: effect === "unknown", violations: [], decode: issue };
}

/**
 * A non-2xx answer as `{code, effect}`. `effect` is the server's when it states a listed value;
 * otherwise `fallback`, which is `unknown` for a command: an answer that does not say nothing was
 * recorded is never read as "nothing happened".
 */
function problemError(status: number, doc: ProblemFields, fallback: Effect): ApiError {
  const effect = doc.effect ?? fallback;
  return {
    code: doc.code ?? (status === 401 ? "unauthenticated" : "invalid_response"),
    effect,
    status,
    event_id: doc.event_id,
    retryable: doc.retryable ?? effect === "unknown",
    violations: doc.violations,
    decode: null,
  };
}

function fillRoute(route: string, params: Record<string, string>): `/${string}` {
  return route.replace(/\{([a-z]+)\}/g, (_, name: string) => {
    const value = Object.hasOwn(params, name) ? params[name] : undefined;
    if (value === undefined || value === "" || value === "." || value === "..") throw new Error(`route parameter ${name} is missing or not a segment`);
    return encodeURIComponent(value);
  }) as `/${string}`;
}

export function createWorkspaceClient(options: ClientOptions): WorkspaceClient {
  const { workspaceId, fetch, baseUrl = "", newKey = newIdempotencyKey, onUnauthenticated } = options;
  const schema: CommonSchema = {
    envelope: options.schema?.envelope ?? PROSE_SCHEMA.envelope,
    problem: options.schema?.problem ?? PROSE_SCHEMA.problem,
  };

  /** A decoder that throws is an answer the client could not read, never an exception out of a call. */
  function decodeProblem(body: unknown): ProblemFields | null {
    try {
      return schema.problem(body);
    } catch {
      return null;
    }
  }
  if (!WORKSPACE_ID.test(workspaceId)) throw new Error("not a workspace id");
  const credentials: RequestCredentials = baseUrl === "" ? "same-origin" : "include";

  const path = (route: `/${string}`) => `/v1/workspaces/${workspaceId}${route}`;

  function noteAuth(error: ApiError) {
    if (error.status === 401 && error.code === "unauthenticated") onUnauthenticated?.();
  }

  async function read<T>(route: `/${string}`, decoder: Decoder<T>): Promise<ReadOutcome<T>> {
    return answered(() => fetch(baseUrl + path(route), { method: "GET", credentials, headers: { Accept: "application/json" } }), decoder);
  }

  /** A read's answer, or one of a call that records nothing: every failure is `effect: "none"`. */
  async function answered<T>(call: () => Promise<Response>, decoder: Decoder<T>, lost: Effect = "none"): Promise<ReadOutcome<T>> {
    let response: Response;
    try {
      response = await call();
    } catch {
      return { ok: false, error: networkError(lost) };
    }
    const body = await parseBody(response);
    if (!response.ok) {
      const doc = decodeProblem(body);
      const error = doc ? problemError(response.status, doc, lost) : invalidResponse(response.status, lost, UNREADABLE, null);
      noteAuth(error);
      return { ok: false, error };
    }
    let head: ReturnType<CommonSchema["envelope"]>;
    try {
      head = schema.envelope(body, "");
    } catch {
      head = { ok: false, issue: UNREADABLE };
    }
    if (!head.ok) return { ok: false, error: invalidResponse(response.status, lost, head.issue, null) };
    const value = decoder(body, "");
    if (!value.ok) return { ok: false, error: invalidResponse(response.status, lost, value.issue, null) };
    return { ok: true, value: value.value, as_of: head.value.as_of, api_version: head.value.api_version };
  }

  function freeze(operation: Operation, route: `/${string}`, body: JsonObject): PreparedCommand {
    const key = newKey();
    if (!isIdempotencyKey(key)) throw new Error("not an idempotency key");
    return Object.freeze({ operation, method: "POST", path: path(route), idempotency_key: key, body: JSON.stringify(body) });
  }

  function prepare(operation: Operation, params: Record<string, string>, body: JsonObject): PreparedCommand {
    if (!Object.hasOwn(OPERATIONS, operation)) throw new Error("not an operation of the workspace client");
    return freeze(operation, fillRoute(OPERATIONS[operation], params), body);
  }

  function prepareKillSwitch(request: KillSwitchRequest): PreparedCommand {
    const { scope, environment_shown, owner_exit, record, step_up } = request;
    return freeze("kill_switch", OPERATIONS.kill_switch, { scope, environment_shown, owner_exit, record, step_up });
  }

  async function send(command: PreparedCommand): Promise<CommandOutcome> {
    let response: Response;
    try {
      response = await fetch(baseUrl + command.path, {
        method: command.method,
        credentials,
        headers: { Accept: "application/json", "Content-Type": "application/json", "Idempotency-Key": command.idempotency_key, [CSRF_HEADER]: "1" },
        body: command.body,
      });
    } catch {
      return { ok: false, error: networkError("unknown"), command };
    }
    const body = await parseBody(response);
    if (!response.ok) {
      const doc = decodeProblem(body);
      const error = doc ? problemError(response.status, doc, "unknown") : invalidResponse(response.status, "unknown", UNREADABLE, null);
      noteAuth(error);
      return { ok: false, error, command };
    }
    const accepted = decodeAccepted(body);
    if (!accepted.ok) return { ok: false, error: invalidResponse(response.status, "unknown", accepted.issue, eventIdOf(body)), command };
    return { ok: true, effect: "recorded", accepted: accepted.value, body };
  }

  const commandStatus = (eventId: string) => read(fillRoute("/commands/{event}", { event: eventId }), commandStatusOf);

  function askChallenge(action: StepUpAction): Promise<ReadOutcome<StepUpChallenge>> {
    const key = newKey();
    if (!isIdempotencyKey(key)) throw new Error("not an idempotency key");
    const body = JSON.stringify({ action: { kind: action.kind, digest: action.digest } });
    const headers = { Accept: "application/json", "Content-Type": "application/json", "Idempotency-Key": key, [CSRF_HEADER]: "1" };
    return answered(() => fetch(baseUrl + path(STEP_UP_CHALLENGE_ROUTE), { method: "POST", credentials, headers, body }), stepUpChallenge(action));
  }

  /** A POST of `body` under a fresh key with the CSRF mark, answered as `decoder` reads it. */
  function posted<T>(route: `/${string}`, body: JsonObject, decoder: Decoder<T>, lost: Effect): Promise<ReadOutcome<T>> {
    const key = newKey();
    if (!isIdempotencyKey(key)) throw new Error("not an idempotency key");
    const headers = { Accept: "application/json", "Content-Type": "application/json", "Idempotency-Key": key, [CSRF_HEADER]: "1" };
    return answered(() => fetch(baseUrl + path(route), { method: "POST", credentials, headers, body: JSON.stringify(body) }), decoder, lost);
  }
  const askRegistration = () => posted(REGISTRATION_ROUTE, {}, registrationChallenge, "none");
  const enrolPasskey = (body: JsonObject) => posted(ENROL_ROUTE, body, enrolled, "unknown");

  const readSession = () => answered(() => fetch(baseUrl + SESSION_ROUTE, { method: "GET", credentials, headers: { Accept: "application/json" } }), sessionPrincipal);

  function prepareConnect(body: ConnectStartBody): PreparedConnect {
    const key = newKey();
    if (!isIdempotencyKey(key)) throw new Error("not an idempotency key");
    return Object.freeze({ idempotency_key: key, body: JSON.stringify(body) });
  }

  async function sendConnect(prepared: PreparedConnect): Promise<ConnectOutcome> {
    let response: Response;
    try {
      response = await fetch(baseUrl + path(CONNECT_START_ROUTE), {
        method: "POST",
        credentials,
        headers: { Accept: "application/json", "Content-Type": "application/json", "Idempotency-Key": prepared.idempotency_key, [CSRF_HEADER]: "1" },
        body: prepared.body,
      });
    } catch {
      return { ok: false, error: networkError("unknown") };
    }
    const body = await parseBody(response);
    if (!response.ok) {
      const doc = decodeProblem(body);
      const error = doc ? problemError(response.status, doc, "unknown") : invalidResponse(response.status, "unknown", UNREADABLE, null);
      noteAuth(error);
      return { ok: false, error };
    }
    const started = connectStarted(body, "");
    return started.ok ? { ok: true, value: started.value } : { ok: false, error: invalidResponse(response.status, "unknown", started.issue, null) };
  }

  async function askRealMoney(agentId: string): Promise<GoLiveOutcome> {
    const key = newKey();
    if (!isIdempotencyKey(key)) throw new Error("not an idempotency key");
    let response: Response;
    try {
      response = await fetch(baseUrl + path(fillRoute(GO_LIVE_ROUTE, { agent: agentId })), {
        method: "POST",
        credentials,
        headers: { Accept: "application/json", "Content-Type": "application/json", "Idempotency-Key": key, [CSRF_HEADER]: "1" },
        body: "{}",
      });
    } catch {
      return { ok: false, error: networkError("unknown") };
    }
    const body = await parseBody(response);
    const doc = response.ok ? null : decodeProblem(body);
    const error = doc ? problemError(response.status, doc, "unknown") : invalidResponse(response.status, "unknown", UNREADABLE, null);
    noteAuth(error);
    return { ok: false, error };
  }

  return Object.freeze({ workspaceId, path, read, prepare, prepareKillSwitch, send, commandStatus, askChallenge, readSession, prepareConnect, sendConnect, askRealMoney, askRegistration, enrolPasskey });
}
