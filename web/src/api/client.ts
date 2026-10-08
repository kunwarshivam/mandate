/**
 * The typed workspace API client's foundation (E11-9, with E10-10's conventions; workspace API spec
 * §3, §5, §6). Every path is under `/v1/workspaces/{ws}`; every command carries one
 * `Idempotency-Key` per gesture; every failure decodes to `{code, effect}`, and a call whose
 * outcome could not be confirmed is `effect: "unknown"`, never success and never "nothing happened"
 * (API-13). It has no order-placing method (DEC-528).
 */
import type { CommandOutcome, CommandStatus, Decoder, Effect, JsonObject, KillSwitchRequest, Operation, PreparedCommand, ReadOutcome, Violation, Watermark } from "./types";
import { UNIMPLEMENTED } from "./unimplemented";

export type Fetch = (input: string, init: RequestInit) => Promise<Response>;

export interface ClientOptions {
  workspaceId: string;
  fetch: Fetch;
  /** Origin of the API; empty for the app's own origin. */
  baseUrl?: string;
  /** A fresh `Idempotency-Key` per gesture; tests pass a deterministic one. */
  newKey?: () => string;
  /** Called on a 401 `unauthenticated`, never on `step_up_required`. */
  onUnauthenticated?: () => void;
  /** The shared envelope and problem shapes; see `CommonSchema`. A member left undefined keeps its default. */
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
 * `unknown`, and a decoder that throws never turns a sent command into a rejection.
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
}

/** Spec §3.3's CSRF defence (L2's pin, DEC-681): a header a cross-site form cannot set, on every command. */
export const CSRF_HEADER = "X-Mandate-Request";

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
};

/** Spec §3.4: 16 to 64 characters from `[A-Za-z0-9_-]`. */
export function isIdempotencyKey(key: string): boolean {
  void key;
  throw new Error(UNIMPLEMENTED);
}

/** Random from the platform's CSPRNG (`crypto.getRandomValues`): a key must not be guessable. */
export function newIdempotencyKey(): string {
  throw new Error(UNIMPLEMENTED);
}

export function createWorkspaceClient(options: ClientOptions): WorkspaceClient {
  void options;
  throw new Error(UNIMPLEMENTED);
}
