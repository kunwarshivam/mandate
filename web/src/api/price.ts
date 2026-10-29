/**
 * An instrument's last trusted close (`GET /instruments/{id}/price`, workspace API §4.1, DEC-2152):
 * the close of the last completed regular session as `mandate-bars` admits it, or `reported: false`
 * with its reason. It is a settled daily close, never a live bid or a price now, and a body that
 * reports no price yet carries a stub member is refused, so a made-up figure is never shown.
 */
import { closedEnum, decimal, isRecord, string, timestamp } from "./decode";
import type { Decoded, DecodeProblem, Decoder, Timestamp } from "./types";

export const PRICE_REASONS = ["no_store", "not_covered", "no_bars", "untrusted"] as const;
export type PriceReason = (typeof PRICE_REASONS)[number];

export type InstrumentPrice =
  | { reported: true; instrument_id: string; symbol: string; close: string; bar_start: Timestamp; timeframe: "1Day"; source: "alpaca-iex" | "recorded" }
  | { reported: false; instrument_id: string; symbol: string; reason: PriceReason };

export const priceRoute = (instrumentId: string) => `/instruments/${encodeURIComponent(instrumentId)}/price` as const;

const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const BAR_MEMBERS = ["close", "bar_start", "timeframe", "source"] as const;

const fail = (path: string, problem: DecodeProblem): Decoded<never> => ({ ok: false, issue: { path, problem, value: null, allowed: null } });

function member<T>(value: Record<string, unknown>, key: string, path: string, decoder: Decoder<T>): Decoded<T> {
  return Object.hasOwn(value, key) ? decoder(value[key], `${path}/${key}`) : fail(`${path}/${key}`, "missing");
}

/** `read-models/instrument-price`: a reported close with every bar member and no reason, or a reason and no bar member at all. */
export const priceRead: Decoder<InstrumentPrice> = (value, path) => {
  if (!isRecord(value)) return fail(path, "wrong_type");
  const id = member(value, "instrument_id", path, string);
  if (!id.ok) return id;
  if (!ASSET_ID.test(id.value)) return fail(`${path}/instrument_id`, "not_canonical");
  const symbol = member(value, "symbol", path, string);
  if (!symbol.ok) return symbol;
  if (symbol.value.length < 1 || symbol.value.length > 32) return fail(`${path}/symbol`, "not_canonical");
  const reported = member(value, "reported", path, (v, at) => (typeof v === "boolean" ? { ok: true, value: v } : fail(at, "wrong_type")));
  if (!reported.ok) return reported;
  if (!reported.value) {
    const stub = BAR_MEMBERS.find((key) => Object.hasOwn(value, key));
    if (stub) return fail(`${path}/${stub}`, "not_canonical");
    const reason = member(value, "reason", path, closedEnum(PRICE_REASONS));
    return reason.ok ? { ok: true, value: { reported: false, instrument_id: id.value, symbol: symbol.value, reason: reason.value } } : reason;
  }
  if (Object.hasOwn(value, "reason")) return fail(`${path}/reason`, "not_canonical");
  const close = member(value, "close", path, decimal);
  if (!close.ok) return close;
  const barStart = member(value, "bar_start", path, timestamp);
  if (!barStart.ok) return barStart;
  const timeframe = member(value, "timeframe", path, closedEnum(["1Day"] as const));
  if (!timeframe.ok) return timeframe;
  const source = member(value, "source", path, closedEnum(["alpaca-iex", "recorded"] as const));
  if (!source.ok) return source;
  return { ok: true, value: { reported: true, instrument_id: id.value, symbol: symbol.value, close: close.value, bar_start: barStart.value, timeframe: timeframe.value, source: source.value } };
};

