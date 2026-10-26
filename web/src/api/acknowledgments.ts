/**
 * The owner's acknowledgment of a hold over the workspace API (spec §3.11 item 12, DEC-2144; journal
 * spec §9.28): the holds that stand now (`GET /acknowledgments?state=open`), each in words, the
 * step-up digest an acknowledgment binds, and its command. An acknowledgment only adds risk back, so
 * it is never an API-7 operation: it needs passkey step-up, and the executor judges it again when it
 * processes the copy. Only the executor's `RiskLimitLifted` or lifting `AgentModeApplied` lifts a
 * hold; the command's `recorded` lifts nothing.
 */
import type { WorkspaceClient } from "./client";
import { array, closedEnum, isRecord, nullable, object, string, timestamp } from "./decode";
import type { PasskeyStepUp } from "./passkey";
import type { ContentRef, Decoder, PreparedCommand, Timestamp } from "./types";
import { sha256Hex } from "@/lib/sha256";

export const HOLD_SUBJECTS = ["limit", "restriction", "reconciliation"] as const;
export type HoldSubject = (typeof HOLD_SUBJECTS)[number];

export const RECONCILIATION_KINDS = ["instrument", "cash", "external_activity", "incomplete"] as const;
export type ReconciliationKind = (typeof RECONCILIATION_KINDS)[number];

/** What holds: a limit's code, a restriction's cause, or a reconciliation hold's restriction, its kind, and the instrument it holds. */
export interface HoldReason {
  limit?: string;
  cause?: string;
  restriction?: string;
  kind?: ReconciliationKind;
  instrument?: string | null;
}

/** One hold the owner may acknowledge (`read-models/acknowledgments`'s `Hold`). */
export interface Hold {
  event: string;
  subject: HoldSubject;
  /** The agent the acknowledgment is posted to, which the server chose. */
  agent_id: string;
  account: string;
  reason: HoldReason;
  /** The mode the hold holds the agent in; `null` for a limit. */
  mode: "exits_only" | "paused" | null;
  raised_at: Timestamp;
  /** Whether another member must acknowledge it; `null` while the server does not read the policy. */
  independent_required: boolean | null;
  request: { event_id: string; requested_by: string } | null;
}

const text: Decoder<string> = (value, path) => {
  const read = string(value, path);
  if (!read.ok) return read;
  return read.value.length >= 1 && read.value.length <= 200 ? read : { ok: false, issue: { path, problem: "not_canonical", value: read.value, allowed: null } };
};

/** The reason by subject, as the schema's `if`/`then` pins it: a limit names its `limit`, a restriction its `cause`, a reconciliation hold its restriction, kind and instrument. */
const reasonOf = (subject: HoldSubject): Decoder<HoldReason> => {
  switch (subject) {
    case "limit":
      return object<HoldReason>({ limit: text });
    case "restriction":
      return object<HoldReason>({ cause: text });
    case "reconciliation":
      return object<HoldReason>({ restriction: text, kind: closedEnum(RECONCILIATION_KINDS), instrument: nullable(text) });
    default: {
      const unhandled: never = subject;
      throw new Error(`unhandled subject ${String(unhandled)}`);
    }
  }
};

const independent: Decoder<boolean | null> = (value, path) =>
  value === null || typeof value === "boolean" ? { ok: true, value } : { ok: false, issue: { path, problem: "wrong_type", value: null, allowed: null } };

const hold: Decoder<Hold> = (value, path) => {
  const subject = isRecord(value) ? closedEnum(HOLD_SUBJECTS)(value.subject, `${path}/subject`) : null;
  const read = object<Hold>({
    event: string,
    subject: closedEnum(HOLD_SUBJECTS),
    agent_id: string,
    account: text,
    reason: subject?.ok ? reasonOf(subject.value) : () => ({ ok: false, issue: { path: `${path}/reason`, problem: "wrong_type", value: null, allowed: null } }),
    mode: subject?.ok && subject.value === "limit" ? (v, p) => (v === null ? { ok: true, value: null } : { ok: false, issue: { path: p, problem: "wrong_type", value: null, allowed: null } }) : closedEnum(["exits_only", "paused"] as const),
    raised_at: timestamp,
    independent_required: independent,
    request: nullable(object<{ event_id: string; requested_by: string }>({ event_id: string, requested_by: string })),
  })(value, path);
  return read;
};

/** `GET /acknowledgments?state=open`, every member required, as the schema requires them. */
export const acknowledgmentsRead = object<{ acknowledgments: Hold[] }>({ acknowledgments: array(hold) });

/** A hold in words: what holds and what it blocks. Never a raw code: one this page does not know reads generically. */
export function holdWords(h: Hold): { what: string; blocks: string } {
  const blocks = h.mode === "paused" ? "The agent is paused: it places no orders until the hold is lifted." : h.mode === "exits_only" ? "It holds new openings; exits and protective orders still run." : "It holds what the limit's action holds until you acknowledge it.";
  switch (h.subject) {
    case "limit":
      return { what: limitWords(h.reason.limit ?? ""), blocks };
    case "restriction":
      return { what: restrictionWords(h.reason.cause ?? ""), blocks };
    case "reconciliation":
      return { what: reconciliationWords(h.reason.kind, h.reason.instrument ?? null), blocks };
    default: {
      const unhandled: never = h.subject;
      throw new Error(`unhandled subject ${String(unhandled)}`);
    }
  }
}

function limitWords(limit: string): string {
  if (limit === "max_daily_loss") return "The daily loss limit tripped.";
  if (limit === "lifetime_floor") return "The loss floor on the money it was given tripped.";
  if (limit === "profit_stop") return "The profit stop was reached.";
  if (/^drawdown_ladder\[\d\]$/.test(limit)) return "A step of the drawdown ladder tripped.";
  if (limit.startsWith("tripwire:")) return "One of your tripwires tripped.";
  return "A risk limit tripped.";
}

function restrictionWords(cause: string): string {
  if (cause === "broker_reject") return "Your broker rejected an order in a way that restricts the account.";
  if (cause === "broker_notice") return "Your broker restricted the account.";
  if (cause === "connection_unavailable") return "The connection to your broker became unavailable.";
  return "The account is restricted.";
}

function reconciliationWords(kind: ReconciliationKind | undefined, instrument: string | null): string {
  switch (kind) {
    case "instrument":
      return `The broker's position${instrument ? ` in ${instrument}` : ""} did not match the journal's.`;
    case "cash":
      return "The broker's cash or fees did not match the journal's.";
    case "external_activity":
      return "Activity on the account came from outside the platform.";
    case "incomplete":
      return "A reconciliation run or a broker answer could not be recorded whole.";
    default:
      return "A reconciliation found a difference.";
  }
}

/** What an acknowledgment's step-up confirms: the acknowledgment, or the request another member answers. */
export type AcknowledgeConfirms = "acknowledge" | "request_ack";

/**
 * The digest an acknowledgment's or its request's step-up binds (spec §3.11 item 12, DEC-279 item 1,
 * DEC-2144): SHA-256 of the canonical object of what it confirms, the agent, the control stream's head
 * as text, the hold's event and subject, and the request only when one is named, the other members
 * `null`, in journal spec §4's key order. Every value is plain ASCII, so this is the canonical form.
 */
export function acknowledgeStepUpDigest(confirms: AcknowledgeConfirms, agentId: string, controlHead: number, event: string, subject: HoldSubject, request: string | null): ContentRef {
  const members: Record<string, string | null> = { agent: agentId, bid: null, bid_size: null, confirms, control_head: String(controlHead), event, floor: null, instrument: null, subject };
  if (request !== null) members.request = request;
  const canonical = Object.fromEntries(Object.keys(members).sort().map((k) => [k, members[k]]));
  return `sha256:${sha256Hex(JSON.stringify(canonical))}`;
}

/** `POST /agents/{id}/acknowledgments` for `hold`, to the agent the server named, with the passkey evidence and no request. */
export function prepareAcknowledge(client: WorkspaceClient, h: Hold, stepUp: PasskeyStepUp): PreparedCommand {
  return client.prepare("acknowledge", { agent: h.agent_id }, { acknowledged: h.event, subject: h.subject, request: null, record: null, step_up: { ...stepUp.step_up }, step_up_assertion: { ...stepUp.step_up_assertion } });
}
