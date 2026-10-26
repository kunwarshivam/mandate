import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { recordsRead } from "./records";

const STREAM = "agent:ws_1:agt_1";
const sha = (text: string) => createHash("sha256").update(text, "utf8").digest("hex");
const record = (seq: number, payload: unknown, overrides: Record<string, unknown> = {}) => {
  const body = JSON.stringify({ event_type: "DecisionMade", payload });
  return { seq, event_id: `E${seq}`, event_type: "DecisionMade", recorded_at: "2026-10-26T10:00:00.000000000Z", prev_hash: "0".repeat(64), hash: sha(body), body: Buffer.from(body).toString("base64"), ...overrides };
};
const page = (events: unknown[]) => ({ stream_id: STREAM, head: { seq: events.length, hash: "0".repeat(64), recorded_at: "2026-10-26T10:00:00.000000000Z" }, events });

describe("a stream's records, each body checked against its hash (§4.8.1)", () => {
  it("reads the payload of a body that matches its hash, checked by an independent SHA-256", () => {
    const read = recordsRead(STREAM)(page([record(1, { dry_run: "allow", side: "buy" })]), "");
    expect(read).toMatchObject({ ok: true, value: { head_seq: 1, events: [{ seq: 1, matches: true, payload: { dry_run: "allow", side: "buy" } }] } });
  });

  it("reads no payload from a body that does not match its hash: changed bytes, a wrong hash, or bytes that are not UTF-8", () => {
    const good = record(1, { dry_run: "deny" });
    const tampered = { ...good, body: Buffer.from(JSON.stringify({ event_type: "DecisionMade", payload: { dry_run: "allow" } })).toString("base64") };
    const wrongHash = { ...good, hash: "f".repeat(64) };
    const notUtf8 = { ...good, body: Buffer.from([0xff, 0xfe, 0x00]).toString("base64"), hash: createHash("sha256").update(Buffer.from([0xff, 0xfe, 0x00])).digest("hex") };
    for (const e of [tampered, wrongHash, notUtf8]) {
      const read = recordsRead(STREAM)(page([e]), "");
      expect(read.ok && read.value.events[0]).toMatchObject({ matches: false, payload: null });
    }
  });

  it("refuses a page for another stream, records out of order, or a hash that is not 64 hex", () => {
    expect(recordsRead("agent:ws_1:agt_2")(page([record(1, {})]), "").ok).toBe(false);
    expect(recordsRead(STREAM)(page([record(2, {}), record(1, {})]), "").ok).toBe(false);
    expect(recordsRead(STREAM)(page([record(1, {}, { hash: "abc" })]), "").ok).toBe(false);
  });
});
