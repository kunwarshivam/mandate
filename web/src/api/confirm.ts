/**
 * Confirming a mandate version (workspace API spec §5.1, §4.1; `commands/confirm-request` and
 * `commands/confirm-accepted`; DEC-2401). The confirm's 202 is decoded exactly as its schema states
 * it, and the step-up a risk-increasing confirm needs is bound to `{mandate_version, agent_id,
 * base_version}`.
 */
import { array, closedEnum, contentRef, isRecord, timestamp, watermark } from "./decode";
import type { ContentRef, Decoded, DecodeProblem, Decoder, Watermark } from "./types";
import { sha256Hex } from "@/lib/sha256";

/** Mandate spec §9.2's classification of a version against its base, as §5.1 names it. Closed. */
export const CLASSIFICATIONS = ["risk_increasing", "risk_reducing", "neutral"] as const;
export type Classification = (typeof CLASSIFICATIONS)[number];

/** When a confirmed version applies (mandate spec §2.2). Closed. */
export const APPLIES = ["now", "next_safe_point"] as const;
export type Applies = (typeof APPLIES)[number];

/** The 202 of a confirm (§5.1): the `MandateConfirmed` event, the server's classification, and when it applies. */
export interface ConfirmAccepted {
  command_id: string;
  phase: "recorded";
  classification: Classification;
  applies: Applies;
  as_of: Watermark[];
}

const fail = (path: string, problem: DecodeProblem, value: string | null = null, allowed: readonly string[] | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed } });

/** `common.schema.json`'s `EventId`: a ULID in Crockford base32. */
const EVENT_ID = /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/;

function constant<const T extends string>(expected: T): Decoder<T> {
  return (value, path) => (value === expected ? { ok: true, value: expected } : fail(path, typeof value === "string" ? "unknown_enum_value" : "wrong_type", String(value), [expected]));
}

const classification = closedEnum(CLASSIFICATIONS);
const applies = closedEnum(APPLIES);

/** `AsOf`: at least one watermark (DEC-682: a command's 202 carries the committed control-stream watermark). */
const asOf: Decoder<Watermark[]> = (value, path) => {
  const read = array(watermark)(value, path);
  if (!read.ok) return read;
  return read.value.length > 0 ? read : fail(path, "missing");
};

/**
 * `confirm-accepted.schema.json`: the envelope (`api_version` v1, `build`, `served_at`, `as_of` with at
 * least one watermark) and the confirm's own members. Unknown members are ignored (spec §3.2).
 */
export const confirmAccepted: Decoder<ConfirmAccepted> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const version = constant("v1")(value.api_version, `${path}/api_version`);
  if (!version.ok) return version;
  const build = contentRef(value.build, `${path}/build`);
  if (!build.ok) return build;
  const served = timestamp(value.served_at, `${path}/served_at`);
  if (!served.ok) return served;
  const watermarks = asOf(value.as_of, `${path}/as_of`);
  if (!watermarks.ok) return watermarks;
  const id = value.command_id;
  if (typeof id !== "string") return fail(`${path}/command_id`, id === undefined ? "missing" : "wrong_type");
  if (!EVENT_ID.test(id)) return fail(`${path}/command_id`, "not_canonical", id);
  const phase = constant("recorded")(value.phase, `${path}/phase`);
  if (!phase.ok) return phase;
  const server = classification(value.classification, `${path}/classification`);
  if (!server.ok) return server;
  const when = applies(value.applies, `${path}/applies`);
  if (!when.ok) return when;
  return { ok: true, value: { command_id: id, phase: phase.value, classification: server.value, applies: when.value, as_of: watermarks.value } };
};

/**
 * The digest a confirm's step-up is bound to: SHA-256 of the canonical `{mandate_version, agent_id,
 * base_version}` (§5.1, §3.6). The members are written in journal spec §4's key order, which for
 * these three plain members is the canonical form.
 */
export function confirmStepUpDigest(version: ContentRef, agentId: string | null, base: ContentRef | null): ContentRef {
  const canonical = JSON.stringify({ agent_id: agentId, base_version: base, mandate_version: version });
  return `sha256:${sha256Hex(canonical)}`;
}
