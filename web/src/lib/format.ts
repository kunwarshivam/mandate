import { type Dec, dec, sign, toFixed } from "./decimal";

export const MINUS = "\u2212";

function group(whole: string): string {
  return whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function toDec(value: string | Dec): Dec {
  return typeof value === "string" ? dec(value) : value;
}

/** "$1,234.50"; negative values carry a true minus sign. */
export function usd(value: string | Dec, places = 2): string {
  const fixed = toFixed(toDec(value), places);
  const negative = fixed.startsWith("-");
  const [whole, fraction] = (negative ? fixed.slice(1) : fixed).split(".");
  const body = `$${group(whole)}${fraction ? `.${fraction}` : ""}`;
  return negative ? `${MINUS}${body}` : body;
}

/** "+$123.45" or "−$67.89"; zero has no sign. Colour never carries the sign alone. */
export function signedUsd(value: string | Dec): string {
  const v = toDec(value);
  const s = sign(v);
  const body = usd(s < 0 ? -v : v);
  return s > 0 ? `+${body}` : s < 0 ? `${MINUS}${body}` : body;
}

export type Direction = "gain" | "loss" | "flat";

export function direction(value: string | Dec): Direction {
  const s = sign(toDec(value));
  return s > 0 ? "gain" : s < 0 ? "loss" : "flat";
}

export function directionWord(value: string | Dec): string {
  const d = direction(value);
  return d === "gain" ? "gain" : d === "loss" ? "loss" : "no change";
}

/** Prices keep the instrument's own precision; at least two places. */
export function price(value: string): string {
  const places = Math.max(2, (value.split(".")[1] ?? "").length);
  return usd(value, places);
}

export function quantity(value: string): string {
  const [whole, fraction] = value.split(".");
  return `${group(whole)}${fraction ? `.${fraction}` : ""}`;
}

/** A fraction of one as a percentage, e.g. "0.075" → "7.5%". */
export function percent(value: string | Dec, places = 1): string {
  const scaled = toDec(value) * 100n;
  const fixed = toFixed(scaled, places).replace(/\.0+$/, "");
  return `${fixed.startsWith("-") ? MINUS + fixed.slice(1) : fixed}%`;
}

/**
 * Wall-clock time as written in the fixture's own offset, "14:02:11". Read from the ISO string
 * itself so rendering never depends on the viewer's time zone setting.
 */
export function clock(iso: string): string {
  const match = /T(\d{2}:\d{2}:\d{2})/.exec(iso);
  if (!match) throw new Error(`not an ISO timestamp: ${iso}`);
  return match[1];
}

export function clockShort(iso: string): string {
  return clock(iso).slice(0, 5);
}

export function dateLabel(iso: string): string {
  const [y, m, d] = iso.slice(0, 10).split("-").map(Number);
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return `${months[m - 1]} ${d}, ${y}`;
}

/** The zone the record's days are counted in: Eastern time, the market's, which every "ET" label names. */
export const RECORD_ZONE = "America/New_York";

const ZONED = new Map<string, Intl.DateTimeFormat>();

function inZone(iso: string, zone: string): { day: string; time: string } {
  let format = ZONED.get(zone);
  if (!format) {
    format = new Intl.DateTimeFormat("en-US", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });
    ZONED.set(zone, format);
  }
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) throw new Error(`not an ISO timestamp: ${iso}`);
  const part = Object.fromEntries(format.formatToParts(ms).map((p) => [p.type, p.value]));
  return { day: `${part.year}-${part.month}-${part.day}`, time: `${part.hour}:${part.minute}:${part.second}` };
}

/**
 * A time on the record, placed in its day: the clock alone, "14:01:12", when it falls on the same
 * day as `now` in `zone`, and "Sep 26, 2026, 15:12" from any other day, so a time from another day
 * never reads as a time today (C-23). Every restriction's "since" uses it; the timelines place their
 * times under day headings instead (`byRecordDay`). `now` and `zone` are inputs, so it reads neither
 * the clock nor the viewer's zone setting.
 */
export function datedClock(at: string, now: string, zone: string): string {
  const when = inZone(at, zone);
  if (when.day === inZone(now, zone).day) return when.time;
  return `${dateLabel(when.day)}, ${when.time.slice(0, 5)}`;
}

const MONTH_NAMES = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/** A day's heading on a timeline: "Today", "25 September", or "31 December 2025" from another year. */
function dayHeading(day: string, today: string): string {
  if (day === today) return "Today";
  const [y, m, d] = day.split("-").map(Number);
  return `${d} ${MONTH_NAMES[m - 1]}${day.slice(0, 4) === today.slice(0, 4) ? "" : ` ${y}`}`;
}

/** One entry under its day: the clock alone, "14:01:12", and its date, "Sep 25, 2026", for assistive tech. */
export interface RecordEntry<T> {
  item: T;
  time: string;
  date: string;
}

export interface RecordDay<T> {
  heading: string;
  entries: RecordEntry<T>[];
}

/**
 * A timeline's entries under day headings (C-19): "Today", then each earlier day, with the day
 * boundary and the times both read in `zone`, and "Today" taken from `now`, never the machine's
 * clock. Consecutive entries on one day share a heading, so a list sorted newest first reads one
 * heading per day, in order.
 */
export function byRecordDay<T>(items: T[], at: (item: T) => string, now: string, zone: string): RecordDay<T>[] {
  const today = inZone(now, zone).day;
  const days: (RecordDay<T> & { day: string })[] = [];
  for (const item of items) {
    const when = inZone(at(item), zone);
    let last = days.at(-1);
    if (last?.day !== when.day) {
      last = { day: when.day, heading: dayHeading(when.day, today), entries: [] };
      days.push(last);
    }
    last.entries.push({ item, time: when.time, date: dateLabel(when.day) });
  }
  return days.map(({ heading, entries }) => ({ heading, entries }));
}

export function zoneLabel(iso: string): string {
  return iso.endsWith("-04:00") || iso.endsWith("-05:00") ? "ET" : "UTC";
}

/**
 * "3 min ago", "40 s ago", "2 h ago". Under a minute the age moves in 5-second steps, so the text
 * changes at most every five seconds. Steps round down, so the age shown is within 5 s of the real
 * one; the absolute time written beside it stays exact.
 */
export function ago(fromIso: string, nowIso: string): string {
  return `${age(fromIso, nowIso)} ago`;
}

/** How old something is, "under 5 s", "15 s", "3 min" or "2 h": seconds in steps of five, so it ticks calmly. */
export function age(fromIso: string, nowIso: string): string {
  const seconds = Math.max(0, Math.floor((Date.parse(nowIso) - Date.parse(fromIso)) / 1000));
  if (seconds < 5) return "under 5 s";
  if (seconds < 60) return `${seconds - (seconds % 5)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h`;
}

/** Remaining time until a deadline in whole minutes, neutral wording. */
export function remaining(deadlineIso: string, nowIso: string): string {
  const seconds = Math.round((Date.parse(deadlineIso) - Date.parse(nowIso)) / 1000);
  if (seconds <= 0) return "deadline passed";
  if (seconds < 60) return "less than 1 min left";
  const minutes = Math.floor(seconds / 60);
  return `${minutes} min left`;
}

export function seconds(n: number): string {
  if (n % 3600 === 0) return `${n / 3600} h`;
  if (n % 60 === 0) return `${n / 60} min`;
  return `${n} s`;
}

/** A natural list, without the serial comma: "A", "A and B", "A, B and C". */
export function andList(items: readonly string[]): string {
  return items.length <= 2 ? items.join(" and ") : `${items.slice(0, -1).join(", ")} and ${items.at(-1)}`;
}
