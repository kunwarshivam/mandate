/**
 * A research agent's theses, folded from its stream's thesis records (journal spec §9.4, §9.22):
 * one `ThesisProposed` or `ThesisRevised` for every thesis the admission judged, admitted or
 * refused with the first failing check (mandate spec §8.5), and one `ThesisInvalidated` for every
 * thesis the runtime invalidated. An admitted thesis is current until the first of DEC-2391 item 2's
 * ends: its invalidation, its expiry passing, a later refusal of its lineage as `lineage_retired`,
 * or a later admitted thesis on its instrument, which replaces it. Only records whose body matched
 * its hash are read (`records.ts`); any other, or one whose payload does not decode, is counted and
 * never shown as a thesis.
 */
import type { JournalRecord } from "./records";

/** Mandate spec §8.5's refusal reasons, in check order. */
export const ADMISSION_REASONS = [
  "direction_not_allowed",
  "horizon_mismatch",
  "revision_without_predecessor",
  "research_disabled",
  "universe_pinned",
  "admission_denied",
  "cost_cap_reached",
  "not_in_data_universe",
  "operator_halt",
  "not_allowed_asset_class",
  "leveraged_etp_not_enabled",
  "eligibility_floor",
  "instrument_group_claimed",
  "source_not_allowlisted",
  "no_corroboration",
  "lineage_retired",
  "universe_full",
] as const;
export type AdmissionReason = (typeof ADMISSION_REASONS)[number];

export type ThesisState = "current" | "rejected" | "invalidated" | "expired" | "lineage_retired" | "replaced";

export interface Thesis {
  thesis_id: string;
  lineage_id: string;
  revision: number;
  instrument_id: string;
  direction: string;
  as_of: string;
  expires_at: string;
  /** When the journal recorded the admission's verdict. */
  recorded_at: string;
  state: ThesisState;
  /** The first failing check, for a rejected thesis. */
  reason: AdmissionReason | null;
  /** When an admitted thesis ended; `null` while it is current, and for a rejected one. */
  ended_at: string | null;
  /** What invalidated it, for an invalidated thesis. */
  cause: "model" | "source_revoked" | null;
}

export interface ThesesView {
  /** Newest verdict first. */
  theses: Thesis[];
  /** Thesis records not read: a body that did not match its hash, or a payload that does not decode. */
  unverified: number;
}

const THESIS_TYPES: ReadonlySet<string> = new Set(["ThesisProposed", "ThesisRevised"]);
const INVALIDATED = "ThesisInvalidated";

const text = (value: unknown): value is string => typeof value === "string" && value.length > 0;
const isReason = (value: unknown): value is AdmissionReason => typeof value === "string" && (ADMISSION_REASONS as readonly string[]).includes(value);
const instant = (iso: string) => Date.parse(iso.slice(0, 23) + "Z");

interface Verdict {
  record: JournalRecord;
  thesis_id: string;
  lineage_id: string;
  revision: number;
  instrument_id: string;
  direction: string;
  as_of: string;
  expires_at: string;
  admitted: boolean;
  reason: AdmissionReason | null;
}

/** A thesis record's members the view reads, or `null` when they do not decode as §9.4 closes them (rule 35 included). */
function verdictOf(record: JournalRecord): Verdict | null {
  const p = record.payload;
  if (!p) return null;
  const { thesis_id, lineage_id, revision, instrument_id, direction, as_of, expires_at, admitted, reason } = p;
  if (!text(thesis_id) || !text(lineage_id) || !text(instrument_id) || typeof direction !== "string" || !text(as_of) || !text(expires_at)) return null;
  if (typeof revision !== "number" || !Number.isSafeInteger(revision) || revision < 0 || typeof admitted !== "boolean") return null;
  if (Number.isNaN(instant(as_of)) || Number.isNaN(instant(expires_at))) return null;
  if (admitted ? reason !== null : !isReason(reason)) return null;
  return { record, thesis_id, lineage_id, revision, instrument_id, direction, as_of, expires_at, admitted, reason: admitted ? null : (reason as AdmissionReason) };
}

interface Invalidation {
  record: JournalRecord;
  thesis_id: string;
  instrument_id: string;
  cause: "model" | "source_revoked";
}

function invalidationOf(record: JournalRecord): Invalidation | null {
  const p = record.payload;
  if (!p) return null;
  const { thesis_id, instrument_id, cause } = p;
  if (!text(thesis_id) || !text(instrument_id) || (cause !== "model" && cause !== "source_revoked")) return null;
  return { record, thesis_id, instrument_id, cause };
}

/** The theses `records` hold, oldest record first in, as of `now`. */
export function thesesOf(records: readonly JournalRecord[], now: string): ThesesView {
  let unverified = 0;
  const verdicts: Verdict[] = [];
  const invalidations: Invalidation[] = [];
  for (const record of records) {
    const isThesis = THESIS_TYPES.has(record.event_type);
    if (!isThesis && record.event_type !== INVALIDATED) continue;
    const read = !record.matches ? null : isThesis ? verdictOf(record) : invalidationOf(record);
    if (read === null) {
      if (isThesis) unverified += 1;
      continue;
    }
    if (isThesis) verdicts.push(read as Verdict);
    else invalidations.push(read as Invalidation);
  }
  const nowAt = instant(now);
  const theses = verdicts.map((v): Thesis => {
    const base = { thesis_id: v.thesis_id, lineage_id: v.lineage_id, revision: v.revision, instrument_id: v.instrument_id, direction: v.direction, as_of: v.as_of, expires_at: v.expires_at, recorded_at: v.record.recorded_at };
    if (!v.admitted) return { ...base, state: "rejected", reason: v.reason, ended_at: null, cause: null };
    const ends: Array<{ at: string; state: ThesisState; cause: Thesis["cause"] }> = [];
    const killed = invalidations.find((i) => i.thesis_id === v.thesis_id && i.instrument_id === v.instrument_id && i.record.seq > v.record.seq);
    if (killed) ends.push({ at: killed.record.recorded_at, state: "invalidated", cause: killed.cause });
    if (instant(v.expires_at) <= nowAt) ends.push({ at: v.expires_at, state: "expired", cause: null });
    const retired = verdicts.find((o) => o.lineage_id === v.lineage_id && o.record.seq > v.record.seq && o.reason === "lineage_retired");
    if (retired) ends.push({ at: retired.record.recorded_at, state: "lineage_retired", cause: null });
    const replaced = verdicts.find((o) => o.admitted && o.instrument_id === v.instrument_id && o.record.seq > v.record.seq);
    if (replaced) ends.push({ at: replaced.record.recorded_at, state: "replaced", cause: null });
    const first = ends.sort((a, b) => instant(a.at) - instant(b.at))[0];
    return first ? { ...base, state: first.state, reason: null, ended_at: first.at, cause: first.cause } : { ...base, state: "current", reason: null, ended_at: null, cause: null };
  });
  return {
    theses: theses
      .map((t, i) => ({ t, seq: verdicts[i].record.seq }))
      .sort((a, b) => b.seq - a.seq)
      .map(({ t }) => t),
    unverified,
  };
}
