/**
 * A stream's records with their bodies (workspace API spec §4.8.1, API-15): each event's stored
 * canonical body, standard base64, checked here against its own `hash` (journal spec §3: the hash
 * is the SHA-256 of the body), so a screen shows what a body says only when the body is the one the
 * journal hashed. The chain across records (`prev_hash`) is the audit's Verify screen's to check, not
 * this read's. A body that does not match its hash is reported as such, never read.
 */
import { array, integer, isRecord, timestamp } from "./decode";
import type { Decoded, DecodeProblem, Decoder, Timestamp } from "./types";
import { sha256Hex } from "@/lib/sha256";

/** One record, its payload read only when its body matched its hash. */
export interface JournalRecord {
  seq: number;
  event_id: string;
  event_type: string;
  recorded_at: Timestamp;
  /** The body's `payload`, or `null` when the body did not match its hash or did not parse. */
  payload: Readonly<Record<string, unknown>> | null;
  /** Whether the body is the one its `hash` names. */
  matches: boolean;
  /** The envelope's `causation_id`: the event this one follows from (journal spec §2), or `null` when it names none or the body was not read. */
  causation_id: string | null;
}

export interface RecordsPage {
  stream_id: string;
  head_seq: number;
  events: JournalRecord[];
}

const fail = (path: string, problem: DecodeProblem): Decoded<never> => ({ ok: false, issue: { path, problem, value: null, allowed: null } });

const HASH = /^[0-9a-f]{64}$/;
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

/** The body's bytes as text, and its payload when the bytes are the ones `hash` names. */
function readBody(body: string, hash: string): { payload: Record<string, unknown> | null; matches: boolean; causation_id: string | null } {
  let text: string;
  try {
    const bytes = Uint8Array.from(atob(body), (c) => c.charCodeAt(0));
    text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return { payload: null, matches: false, causation_id: null };
  }
  if (sha256Hex(text) !== hash) return { payload: null, matches: false, causation_id: null };
  try {
    const parsed: unknown = JSON.parse(text);
    const payload = isRecord(parsed) && isRecord(parsed.payload) ? parsed.payload : null;
    const causation_id = isRecord(parsed) && typeof parsed.causation_id === "string" ? parsed.causation_id : null;
    return { payload, matches: true, causation_id };
  } catch {
    return { payload: null, matches: true, causation_id: null };
  }
}

/** `GET /journal/streams/{id}/events` with its bodies, for the stream asked; an answer for another stream is refused. */
export function recordsRead(streamId: string): Decoder<RecordsPage> {
  const one: Decoder<JournalRecord> = (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    const seq = integer(value.seq, `${path}/seq`);
    if (!seq.ok) return seq;
    const at = timestamp(value.recorded_at, `${path}/recorded_at`);
    if (!at.ok) return at;
    const { event_id, event_type, body, hash } = value;
    if (typeof event_id !== "string" || typeof event_type !== "string" || event_type.length === 0 || event_type.length > 128) return fail(path, "wrong_type");
    if (typeof hash !== "string" || !HASH.test(hash)) return fail(`${path}/hash`, "not_canonical");
    if (typeof body !== "string" || !BASE64.test(body)) return fail(`${path}/body`, "not_canonical");
    return { ok: true, value: { seq: seq.value, event_id, event_type, recorded_at: at.value, ...readBody(body, hash) } };
  };
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    if (value.stream_id !== streamId) return fail(`${path}/stream_id`, "not_canonical");
    const head = isRecord(value.head) ? integer(value.head.seq, `${path}/head/seq`) : fail(`${path}/head`, "missing");
    if (!head.ok) return head;
    const events = array(one)(value.events, `${path}/events`);
    if (!events.ok) return events;
    if (events.value.some((e, i) => e.seq > head.value || (i > 0 && e.seq <= events.value[i - 1].seq))) return fail(`${path}/events`, "not_canonical");
    return { ok: true, value: { stream_id: streamId, head_seq: head.value, events: events.value } };
  };
}
