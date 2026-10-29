import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createWorkspaceClient } from "./client";
import { createMockServer } from "./mock-server";
import { PRICE_REASONS, priceRead, priceRoute } from "./price";
import { INSTRUMENTS } from "@/fixtures/mandates";

/**
 * An instrument's last trusted close (§4.1, DEC-2152) against its schema's own example and variants
 * (`schemas/workspace-api/examples/read-models.instrument-price*.json`), read through the client so
 * the envelope is checked as every read's is: an oracle written against the schema, not this client.
 */
const ROOT = join(__dirname, "..", "..", "..", "schemas", "workspace-api");
const json = (path: string): unknown => JSON.parse(readFileSync(join(ROOT, path), "utf8"));
const BASE = json("examples/read-models.instrument-price.json") as Record<string, unknown>;
const SCHEMA = json("read-models/instrument-price.schema.json") as { properties: { reason: { enum: string[] } } };

interface Variant {
  label: string;
  set?: Record<string, unknown>;
  remove?: string[];
}
const VALID = json("examples/read-models.instrument-price.valid.json") as Variant[];
const INVALID = json("examples/read-models.instrument-price.invalid.json") as Variant[];
/** The invalid variants the client reads by design: each only adds a member the schema does not name, which §3.2 has a client ignore. */
const UNKNOWN_MEMBER_ONLY = ["a bid member"];
/**
 * The envelope's own rule, an `as_of` naming the control stream: the client's envelope reader reads
 * `as_of` for every read alike and requires no particular stream, and the price uses none of it.
 */
const ENVELOPE_ONLY = ["no control-stream watermark"];
const WS = "ws_01JB3K6F0C9R2V7N4M8Q1T5W3Y";

function apply(base: unknown, variant: Variant): unknown {
  const doc = structuredClone(base) as Record<string, unknown>;
  const key = (pointer: string) => pointer.slice(1).replace(/~1/g, "/").replace(/~0/g, "~");
  for (const [pointer, value] of Object.entries(variant.set ?? {})) {
    if (pointer.split("/").length > 2) throw new Error(`a nested pointer: ${pointer}`);
    doc[key(pointer)] = structuredClone(value);
  }
  for (const pointer of variant.remove ?? []) delete doc[key(pointer)];
  return doc;
}

async function served(body: unknown) {
  const client = createWorkspaceClient({ workspaceId: WS, fetch: async () => new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } }), newKey: () => "price-key-000000001" });
  return client.read(priceRoute(String(BASE.instrument_id)), priceRead);
}

describe("an instrument's last trusted close, decoded as its schema states it (DEC-2152)", () => {
  it("decodes the example as a reported daily close", async () => {
    expect(await served(BASE)).toMatchObject({ ok: true, value: { reported: true, instrument_id: BASE.instrument_id, symbol: "QQQ", close: "512.34", bar_start: BASE.bar_start, timeframe: "1Day", source: "alpaca-iex" } });
  });

  it.each(VALID.map((v) => [v.label, v] as const))("decodes the valid variant %s", async (_label, variant) => {
    expect(await served(apply(BASE, variant))).toMatchObject({ ok: true });
  });

  it.each(INVALID.filter((v) => !UNKNOWN_MEMBER_ONLY.includes(v.label) && !ENVELOPE_ONLY.includes(v.label)).map((v) => [v.label, v] as const))("refuses the invalid variant %s", async (_label, variant) => {
    expect(await served(apply(BASE, variant))).toMatchObject({ ok: false });
  });

  it("reads past a member the schema does not name, and never shows it", async () => {
    for (const label of UNKNOWN_MEMBER_ONLY) {
      const variant = INVALID.find((v) => v.label === label);
      if (!variant) throw new Error(`no variant ${label}`);
      const read = await served(apply(BASE, variant));
      expect(read, label).toMatchObject({ ok: true });
      expect(Object.keys(read.ok ? read.value : {}), label).not.toContain("bid");
    }
  });

  it("knows exactly the schema's reasons", () => {
    expect([...PRICE_REASONS].sort()).toEqual([...SCHEMA.properties.reason.enum].sort());
  });

  it("is served by the mock deployment: a close for a held equity, not covered for crypto, not found for an unlisted id", async () => {
    const server = createMockServer({ workspaceId: WS, scenario: "normal" });
    const client = createWorkspaceClient({ workspaceId: WS, fetch: server.fetch, newKey: () => "price-key-000000002" });
    expect(await client.read(priceRoute(INSTRUMENTS.XYZ.asset_id), priceRead)).toMatchObject({ ok: true, value: { reported: true, symbol: "XYZ", source: "recorded" } });
    expect(await client.read(priceRoute(INSTRUMENTS.BTC.asset_id), priceRead)).toMatchObject({ ok: true, value: { reported: false, reason: "not_covered" } });
    expect(await client.read(priceRoute("7b4a1c2e-9999-4a2b-9c3d-000000000009"), priceRead)).toMatchObject({ ok: false, error: { code: "not_found" } });
  });
});
