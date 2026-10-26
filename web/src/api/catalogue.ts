/**
 * The catalogue reads (workspace API spec §4.1 and §3.11 item 11, DEC-2136): the instruments a version
 * may pin and the signal models it may name, as the deployment has registered them. The set-up builds
 * its mandate document from these, so no asset id or content hash is ever made up in the browser
 * (the create refuses an unregistered instrument, and V-007 an unregistered model). Read only.
 */
import { array, closedEnum, contentRef, isRecord, object, timestamp } from "./decode";
import type { ContentRef, Decoded, DecodeProblem, Decoder, Timestamp } from "./types";

/** `read-models/instruments`' `Instrument`. */
export interface CatalogueInstrument {
  instrument_id: string;
  symbol: string;
  asset_class: "us_equity" | "crypto";
  snapshot: ContentRef;
  registered_at: Timestamp;
}

/** A parameter as the content's `params_schema` declares it (DEC-518 item 3), each member only when served. */
export interface CatalogueParam {
  name: string;
  type: string | null;
  min: number | null;
  max: number | null;
}

/** `read-models/signal-models`' `Model`, its `content` projection read for what the set-up asks. */
export interface CatalogueModel {
  model_id: string;
  model_version: string;
  content_hash: ContentRef;
  params: string[];
  admits_instruments: boolean;
  /** `null` when the deployment could not give the content, or gave a parameter list this page cannot read, so the model is not offered. */
  content: { methodology: string | null; params: CatalogueParam[]; constraints: string[] } | null;
  registered_at: Timestamp;
}

const fail = (path: string, problem: DecodeProblem, value: string | null = null): Decoded<never> => ({ ok: false, issue: { path, problem, value, allowed: null } });

const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const MODEL_ID = /^(fast|llm|quant)\.[a-z0-9_]{1,48}$/;
const VERSION = /^(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})$/;
const PARAM = /^[a-z][a-z0-9_]{0,63}$/;

const matching =
  (pattern: RegExp, max: number): Decoder<string> =>
  (value, path) =>
    typeof value !== "string" ? fail(path, value === undefined ? "missing" : "wrong_type") : value.length <= max && pattern.test(value) ? { ok: true, value } : fail(path, "not_canonical", value);
const symbol: Decoder<string> = (value, path) => (typeof value !== "string" ? fail(path, "wrong_type") : value.length >= 1 && value.length <= 32 ? { ok: true, value } : fail(path, "not_canonical", value));
const flag: Decoder<boolean> = (value, path) => (typeof value === "boolean" ? { ok: true, value } : fail(path, value === undefined ? "missing" : "wrong_type"));
const whole = (value: unknown): number | null => (typeof value === "number" && Number.isSafeInteger(value) ? value : null);

const instrument = object<CatalogueInstrument>({
  instrument_id: matching(ASSET_ID, 36),
  symbol,
  asset_class: closedEnum(["us_equity", "crypto"] as const),
  snapshot: contentRef,
  registered_at: timestamp,
});

/** `read-models/instruments`. */
export const instrumentsRead: Decoder<{ instruments: CatalogueInstrument[] }> = object({ instruments: array(instrument) });

/**
 * The served projection of a model's content: anything outside it is ignored here (§3.2), a
 * malformed scalar reads as absent, and a parameter entry this page cannot read makes the content
 * unreadable for that model alone, never the whole list.
 */
const content: Decoder<CatalogueModel["content"]> = (value, path) => {
  if (value === null) return { ok: true, value: null };
  if (!isRecord(value)) return fail(path, "wrong_type");
  const methodology = typeof value.methodology === "string" ? value.methodology : null;
  const schema = isRecord(value.params_schema) ? value.params_schema : {};
  const params: CatalogueParam[] = [];
  for (const p of Array.isArray(schema.params) ? schema.params : []) {
    if (!isRecord(p) || typeof p.name !== "string" || !PARAM.test(p.name)) return { ok: true, value: null };
    params.push({ name: p.name, type: typeof p.type === "string" ? p.type : null, min: whole(p.min), max: whole(p.max) });
  }
  const constraints = (Array.isArray(schema.constraints) ? schema.constraints : []).filter((c): c is string => typeof c === "string");
  return { ok: true, value: { methodology, params, constraints } };
};

const model = object<CatalogueModel>({
  model_id: matching(MODEL_ID, 54),
  model_version: matching(VERSION, 20),
  content_hash: contentRef,
  params: array(matching(PARAM, 64)),
  admits_instruments: flag,
  content,
  registered_at: timestamp,
});

/** `read-models/signal-models`. */
export const signalModelsRead: Decoder<{ models: CatalogueModel[] }> = object({ models: array(model) });

/** Both reads, as the set-up needs them: or why the catalogue is not reported. */
export type CatalogueAnswer = { ok: true; instruments: CatalogueInstrument[]; models: CatalogueModel[] } | { ok: false; reason: string };
