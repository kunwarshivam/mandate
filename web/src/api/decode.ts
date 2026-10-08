/**
 * Decoders for the workspace API's members (spec §3.1, §3.2). They never round: a JSON number where
 * a decimal belongs is refused, and a closed safety enum refuses a value it does not list, as a
 * typed issue the screen can render rather than an exception.
 */
import type { ContentRef, Decoded, DecodeIssue, DecodeProblem, Decoder, Watermark } from "./types";

const CANONICAL_DECIMAL = /^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$/;
const MAX_FRACTION_DIGITS = 28;
/** Journal spec §4.6: the absolute value is below 7.9 × 10^28, so the whole part is too. */
const DECIMAL_BOUND = 79n * 10n ** 27n;
const TIMESTAMP = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$/;
const CONTENT_REF = /^sha256:[0-9a-f]{64}$/;

function fail(path: string, problem: DecodeProblem, value: string | null = null, allowed: readonly string[] | null = null): { ok: false; issue: DecodeIssue } {
  return { ok: false, issue: { path, problem, value, allowed } };
}

const ok = <T>(value: T): Decoded<T> => ({ ok: true, value });

function isCanonicalDecimal(text: string): boolean {
  if (!CANONICAL_DECIMAL.test(text) || text === "-0") return false;
  const [whole, fraction = ""] = text.replace(/^-/, "").split(".");
  return fraction.length <= MAX_FRACTION_DIGITS && BigInt(whole) < DECIMAL_BOUND;
}

/** A journal spec §4.6 canonical decimal string. */
export const decimal: Decoder<string> = (value, path) => {
  if (typeof value === "number") return fail(path, "decimal_number", String(value));
  if (typeof value !== "string") return fail(path, "wrong_type");
  return isCanonicalDecimal(value) ? ok(value) : fail(path, "not_canonical", value);
};

const DAYS_IN_MONTH = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/** Years 1970 to 9999, real calendar days, hours 00–23, minutes and seconds 00–59 (journal spec §4.7). */
function inCalendar(text: string): boolean {
  const [year, month, day, hour, minute, second] = [text.slice(0, 4), text.slice(5, 7), text.slice(8, 10), text.slice(11, 13), text.slice(14, 16), text.slice(17, 19)].map(Number);
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const days = month === 2 && leap ? 29 : DAYS_IN_MONTH[month - 1];
  return year >= 1970 && month >= 1 && month <= 12 && day >= 1 && day <= days && hour <= 23 && minute <= 59 && second <= 59;
}

/** A journal spec §4.7 timestamp: UTC, nine fractional digits, `Z`. */
export const timestamp: Decoder<string> = (value, path) => {
  if (typeof value !== "string") return fail(path, "wrong_type");
  return TIMESTAMP.test(value) && inCalendar(value) ? ok(value) : fail(path, "not_canonical", value);
};

export const contentRef: Decoder<ContentRef> = (value, path) => {
  if (typeof value !== "string") return fail(path, "wrong_type");
  return CONTENT_REF.test(value) ? ok(value as ContentRef) : fail(path, "not_canonical", value);
};

export const string: Decoder<string> = (value, path) => (typeof value === "string" ? ok(value) : fail(path, "wrong_type"));

/** A non-negative integer up to 2^53 − 1 (journal spec §4 item 4). */
export const integer: Decoder<number> = (value, path) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? ok(value) : fail(path, "wrong_type");

export function closedEnum<const T extends string>(allowed: readonly T[]): Decoder<T> {
  return (value, path) => {
    if (typeof value !== "string") return fail(path, "wrong_type");
    return (allowed as readonly string[]).includes(value) ? ok(value as T) : fail(path, "unknown_enum_value", value, allowed);
  };
}

export function nullable<T>(inner: Decoder<T>): Decoder<T | null> {
  return (value, path) => (value === null ? ok(null) : inner(value, path));
}

export function array<T>(item: Decoder<T>): Decoder<T[]> {
  return (value, path) => {
    if (!Array.isArray(value)) return fail(path, "wrong_type");
    const items: T[] = [];
    for (const [index, element] of value.entries()) {
      const decoded = item(element, `${path}/${index}`);
      if (!decoded.ok) return decoded;
      items.push(decoded.value);
    }
    return ok(items);
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** JSON Pointer escaping (RFC 6901). */
const pointer = (path: string, member: string) => `${path}/${member.replace(/~/g, "~0").replace(/\//g, "~1")}`;

/** Reads the named members and ignores every other one (spec §3.2). */
export function object<T extends object>(members: { [K in keyof T]: Decoder<T[K]> }): Decoder<T> {
  return (value, path) => {
    if (!isRecord(value)) return fail(path, "wrong_type");
    const out: Partial<T> = {};
    for (const key of Object.keys(members) as Array<keyof T & string>) {
      if (!Object.hasOwn(value, key)) return fail(pointer(path, key), "missing");
      const decoded = members[key](value[key], pointer(path, key));
      if (!decoded.ok) return decoded;
      out[key] = decoded.value;
    }
    return ok(out as T);
  };
}

export const watermark: Decoder<Watermark> = object<Watermark>({ stream_id: string, seq: integer, hash: contentRef, recorded_at: timestamp });
