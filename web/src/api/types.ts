/**
 * The workspace API's shared shapes, as the web client sees them (workspace API spec §3, §5, §6).
 * Decimals and timestamps stay strings in the journal's canonical forms (journal spec §4.6, §4.7);
 * the per-route read-model shapes come from `schemas/workspace-api/` in a later slice (E11-9).
 */

export type Decimal = string;
export type Timestamp = string;
export type ContentRef = `sha256:${string}`;

/** One stream's position a response reflects (spec §6.1, API-14). */
export interface Watermark {
  stream_id: string;
  seq: number;
  hash: ContentRef;
  recorded_at: Timestamp;
}

/** Spec §3.5: what the call did to the journal. Closed. */
export type Effect = "none" | "recorded" | "unknown";
export const EFFECTS = ["none", "recorded", "unknown"] as const satisfies readonly Effect[];

/** Spec §5.4: advisory only, judged by the stream owners. Closed. */
export type StepUpStatus = "bound" | "missing" | "unbound";
export const STEP_UP_STATUSES = ["bound", "missing", "unbound"] as const satisfies readonly StepUpStatus[];

/** Spec §5.5. Closed. */
export type CommandPhase = "recorded" | "taken" | "applied" | "refused" | "ended";
export const COMMAND_PHASES = ["recorded", "taken", "applied", "refused", "ended"] as const satisfies readonly CommandPhase[];

/** Why a response could not be read. `unknown_enum_value` is a closed safety enum meeting a value it does not list (spec §3.2). */
export type DecodeProblem = "missing" | "wrong_type" | "decimal_number" | "not_canonical" | "unknown_enum_value";

export interface DecodeIssue {
  /** JSON Pointer to the member, `""` for the whole body. */
  path: string;
  problem: DecodeProblem;
  /** The value met, as text, for `unknown_enum_value` and `not_canonical`. */
  value: string | null;
  /** The values the closed enum allows, for `unknown_enum_value`. */
  allowed: readonly string[] | null;
}

export interface Violation {
  path: string;
  code: string;
  message: string;
}

/**
 * Every failure, decoded to the spec §3.5 pair the screens render from: `code` and `effect`.
 * Server codes are an open set; the client adds `network` (no answer), `invalid_response` (an
 * answer it could not read, with `decode`), and keeps the server's `unauthenticated`.
 */
export interface ApiError {
  code: string;
  effect: Effect;
  status: number | null;
  event_id: string | null;
  retryable: boolean;
  violations: Violation[];
  decode: DecodeIssue | null;
}

export type Json = null | boolean | number | string | Json[] | { [member: string]: Json };
export type JsonObject = { [member: string]: Json };

export type Decoded<T> = { ok: true; value: T } | { ok: false; issue: DecodeIssue };

/** Reads a JSON value at a pointer; ignores members it does not name (spec §3.2). */
export type Decoder<T> = (value: unknown, path: string) => Decoded<T>;

export type ReadOutcome<T> =
  | { ok: true; value: T; as_of: Watermark[]; api_version: string }
  | { ok: false; error: ApiError };

/**
 * The commands the web client can send, and nothing else. There is no order ticket (DEC-528,
 * brief §5 rule 12): buying goes through an agent, never through this table.
 */
export type Operation =
  | "pause"
  | "resume"
  | "hold_openings"
  | "stop"
  | "owner_exit"
  | "acknowledge"
  | "kill_switch"
  | "approval_response"
  | "end_delegation"
  | "confirm_version";

/** One user gesture: its key, route and body are fixed when it is prepared, so every retry sends the same call (spec §3.4, API-4). */
export interface PreparedCommand {
  readonly operation: Operation;
  readonly method: "POST";
  readonly path: string;
  readonly idempotency_key: string;
  readonly body: string;
}

/** A 2xx answer to a command: the control-stream event that recorded it (spec §5). */
export interface Accepted {
  event_id: string;
  phase: "recorded";
  step_up_status: StepUpStatus | null;
}

export type CommandOutcome =
  | { ok: true; effect: "recorded"; accepted: Accepted }
  | { ok: false; error: ApiError; command: PreparedCommand };

/** Spec §5.4. `record` and `step_up` may be `null`: Stop works before anything has loaded. */
export interface KillSwitchRequest {
  scope: { kind: "agent" | "connection"; id: string } | { kind: "workspace"; id: null };
  environment_shown: "paper" | "live";
  owner_exit: null;
  record: { artifact: ContentRef; ui_build: ContentRef } | null;
  step_up: null;
}

export interface CommandStep {
  stream_id: string;
  seq: number;
  event_type: string;
  recorded_at: Timestamp;
  reason: string | null;
}

/** Spec §5.5. */
export interface CommandStatus {
  phase: CommandPhase;
  steps: CommandStep[];
}
