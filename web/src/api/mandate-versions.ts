/**
 * An agent's mandate version history (`GET /agents/{id}/mandate-versions`, workspace API spec §4.7,
 * DEC-2147, DEC-2148): the version its deployment names, then each later confirmation for it, oldest
 * first, and the version in effect. The read writes nothing. Each entry is decoded as
 * `read-models/agent-mandate-versions` states it, and the history as the spec orders it: the
 * deployment first and only first, and `in_effect` a listed version or `null`. A body that breaks
 * either is refused whole, never shown in part (DEC-2148).
 */
import { array, closedEnum, contentRef, isRecord, nullable, string, timestamp } from "./decode";
import type { ContentRef, Decoded, DecodeProblem, Decoder, Timestamp } from "./types";

/** Journal spec §9.3 `MandateVersionApplied.reason`: why the executor refused a version (mandate spec §5.1, §5.7). */
export const REJECTION_REASONS = ["increase_blocked_while_latched", "equity_below_exposure", "would_trigger_limit", "not_loosening", "waiting_period", "still_below_new_floor"] as const;
export type RejectionReason = (typeof REJECTION_REASONS)[number];

/** Mandate spec §9.2's verdict of a later version over its base; `null` when the server could not compute it. */
export type LaterClassification = "risk_increasing" | "risk_reducing" | "neutral";

export type VersionApplication = { result: "applied"; reason: null; recorded_at: Timestamp } | { result: "rejected"; reason: RejectionReason; recorded_at: Timestamp };

/** The version the agent's `AgentDeployed` names. */
export interface DeployedVersion {
  kind: "deployed";
  mandate_version: ContentRef;
  classification: "first_version";
  created_at: Timestamp | null;
  confirmed_at: Timestamp | null;
  confirmation_id: string | null;
  deployed_at: Timestamp;
}

/** A later `MandateConfirmed` naming the agent, with the base it replaces and the executor's application, if any yet. */
export interface ConfirmedVersion {
  kind: "confirmed";
  mandate_version: ContentRef;
  base_version: ContentRef;
  classification: LaterClassification | null;
  created_at: Timestamp | null;
  confirmed_at: Timestamp;
  confirmation_id: string;
  application: VersionApplication | null;
}

export interface MandateVersions {
  agent_id: string;
  /** The account stream read for applications; `null` when it was not found, and then nothing in effect can be told. */
  account_ref: string | null;
  in_effect: ContentRef | null;
  stopped_at: Timestamp | null;
  deployed: DeployedVersion;
  /** Oldest first; a version confirmed twice is listed twice. */
  later: ConfirmedVersion[];
}

/** The route, under the workspace prefix. */
export const mandateVersionsRoute = (agentId: string) => `/agents/${encodeURIComponent(agentId)}/mandate-versions` as const;

const ID = /^[A-Za-z0-9_-]{1,64}$/;
const EVENT_ID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;

function fail(path: string, problem: DecodeProblem): { ok: false; issue: { path: string; problem: DecodeProblem; value: null; allowed: null } } {
  return { ok: false, issue: { path, problem, value: null, allowed: null } };
}

const patterned =
  (pattern: RegExp): Decoder<string> =>
  (value, path) => {
    const text = string(value, path);
    if (!text.ok) return text;
    return pattern.test(text.value) ? text : fail(path, "not_canonical");
  };
const id = patterned(ID);
const eventId = patterned(EVENT_ID);
const isNull: Decoder<null> = (value, path) => (value === null ? { ok: true, value: null } : fail(path, "wrong_type"));

/** Each named member, present, through its decoder; members the schema does not name are ignored (spec §3.2). */
function members<T extends Record<string, unknown>>(value: Record<string, unknown>, path: string, decoders: { [K in keyof T]: Decoder<T[K]> }): Decoded<T> {
  const out: Partial<T> = {};
  for (const key of Object.keys(decoders) as Array<keyof T & string>) {
    if (!Object.hasOwn(value, key)) return fail(`${path}/${key}`, "missing");
    const decoded = decoders[key](value[key], `${path}/${key}`);
    if (!decoded.ok) return decoded;
    out[key] = decoded.value;
  }
  return { ok: true, value: out as T };
}

const application: Decoder<VersionApplication> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const result = Object.hasOwn(value, "result") ? closedEnum(["applied", "rejected"] as const)(value.result, `${path}/result`) : fail(`${path}/result`, "missing");
  if (!result.ok) return result;
  if (result.value === "applied") {
    const read = members<{ reason: null; recorded_at: Timestamp }>(value, path, { reason: isNull, recorded_at: timestamp });
    return read.ok ? { ok: true, value: { result: "applied", ...read.value } } : read;
  }
  const read = members<{ reason: RejectionReason; recorded_at: Timestamp }>(value, path, { reason: closedEnum(REJECTION_REASONS), recorded_at: timestamp });
  return read.ok ? { ok: true, value: { result: "rejected", ...read.value } } : read;
};

const deployed: Decoder<DeployedVersion> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const read = members<Omit<DeployedVersion, "kind"> & { base_version: null; application: null }>(value, path, {
    mandate_version: contentRef,
    base_version: isNull,
    classification: closedEnum(["first_version"] as const),
    created_at: nullable(timestamp),
    confirmed_at: nullable(timestamp),
    confirmation_id: nullable(eventId),
    deployed_at: timestamp,
    application: isNull,
  });
  if (!read.ok) return read;
  const { mandate_version, classification, created_at, confirmed_at, confirmation_id, deployed_at } = read.value;
  return { ok: true, value: { kind: "deployed", mandate_version, classification, created_at, confirmed_at, confirmation_id, deployed_at } };
};

const confirmed: Decoder<ConfirmedVersion> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const read = members<Omit<ConfirmedVersion, "kind"> & { deployed_at: null }>(value, path, {
    mandate_version: contentRef,
    base_version: contentRef,
    classification: nullable(closedEnum(["risk_increasing", "risk_reducing", "neutral"] as const)),
    created_at: nullable(timestamp),
    confirmed_at: timestamp,
    confirmation_id: eventId,
    deployed_at: isNull,
    application: nullable(application),
  });
  if (!read.ok) return read;
  const { mandate_version, base_version, classification, created_at, confirmed_at, confirmation_id, application: applied } = read.value;
  return { ok: true, value: { kind: "confirmed", mandate_version, base_version, classification, created_at, confirmed_at, confirmation_id, application: applied } };
};

/** `read-models/agent-mandate-versions`, the history in the spec's order, or the first member that breaks it. */
export const mandateVersionsRead: Decoder<MandateVersions> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const head = members<{ agent_id: string; account_ref: string | null; in_effect: ContentRef | null; stopped_at: Timestamp | null; versions: unknown[] }>(value, path, {
    agent_id: id,
    account_ref: nullable(id),
    in_effect: nullable(contentRef),
    stopped_at: nullable(timestamp),
    versions: array((item) => ({ ok: true, value: item })),
  });
  if (!head.ok) return head;
  const [first, ...rest] = head.value.versions;
  if (first === undefined) return fail(`${path}/versions`, "not_canonical");
  const deployment = deployed(first, `${path}/versions/0`);
  if (!deployment.ok) return deployment;
  const later: ConfirmedVersion[] = [];
  for (const [index, item] of rest.entries()) {
    const entry = confirmed(item, `${path}/versions/${index + 1}`);
    if (!entry.ok) return entry;
    later.push(entry.value);
  }
  const { agent_id, account_ref, in_effect, stopped_at } = head.value;
  const listed = [deployment.value.mandate_version, ...later.map((v) => v.mandate_version)];
  if (in_effect !== null && !listed.includes(in_effect)) return fail(`${path}/in_effect`, "not_canonical");
  return { ok: true, value: { agent_id, account_ref, in_effect, stopped_at, deployed: deployment.value, later } };
};
