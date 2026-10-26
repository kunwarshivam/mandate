import { describe, expect, it } from "vitest";
import type { JournalRecord } from "./records";
import { thesesOf } from "./theses";

const XYZ = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const ABC = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const NOW = "2026-10-26T15:00:00.000Z";

let seq = 0;
const at = (h: number) => `2026-10-26T${String(h).padStart(2, "0")}:00:00.000000000Z`;
/** A thesis record as journal spec §9.4 closes it, with only the members the view reads beyond the rest. */
function thesis(over: Record<string, unknown> & { thesis_id: string }, recorded: string, type = "ThesisProposed"): JournalRecord {
  seq += 1;
  const payload = { model_id: "research.equity", model_version: "1.0.0", lineage_id: `L-${over.thesis_id}`, revision: 0, predecessor_thesis_id: null, instrument_id: XYZ, asset_class: "us_equity", direction: "long", as_of: at(9), expires_at: "2026-10-27T09:00:00.000000000Z", horizon_s: 86400, admitted: true, reason: null, ...over };
  return { seq, event_id: `E${seq}`, event_type: type, recorded_at: recorded, payload, matches: true };
}
function invalidated(thesis_id: string, recorded: string, cause = "model", instrument_id = XYZ): JournalRecord {
  seq += 1;
  return { seq, event_id: `E${seq}`, event_type: "ThesisInvalidated", recorded_at: recorded, payload: { thesis_id, lineage_id: `L-${thesis_id}`, instrument_id, cause, candidate: cause === "model" ? 0 : null, evidence: [], source: cause === "model" ? null : "src_1" }, matches: true };
}

describe("an agent's theses from its stream's thesis records (journal spec §9.4, §9.22; DEC-2391 item 2)", () => {
  it("lists every thesis the admission judged, newest first: admitted and current, or rejected with its reason", () => {
    const view = thesesOf([thesis({ thesis_id: "T1" }, at(10)), thesis({ thesis_id: "T2", instrument_id: ABC, admitted: false, reason: "eligibility_floor" }, at(11))], NOW);
    expect(view.unverified).toBe(0);
    expect(view.theses.map((t) => [t.thesis_id, t.state])).toEqual([
      ["T2", "rejected"],
      ["T1", "current"],
    ]);
    expect(view.theses[0]).toMatchObject({ instrument_id: ABC, reason: "eligibility_floor", recorded_at: at(11) });
  });

  it("ends an admitted thesis at its invalidation, naming the cause and when", () => {
    const view = thesesOf([thesis({ thesis_id: "T1" }, at(10)), invalidated("T1", at(12), "source_revoked")], NOW);
    expect(view.theses[0]).toMatchObject({ thesis_id: "T1", state: "invalidated", ended_at: at(12), cause: "source_revoked" });
  });

  it("ends an admitted thesis when its expiry has passed, and keeps it current until then", () => {
    const record = thesis({ thesis_id: "T1", expires_at: "2026-10-26T14:00:00.000000000Z" }, at(10));
    expect(thesesOf([record], NOW).theses[0]).toMatchObject({ state: "expired", ended_at: "2026-10-26T14:00:00.000000000Z" });
    expect(thesesOf([record], "2026-10-26T13:59:59.000Z").theses[0]).toMatchObject({ state: "current", ended_at: null });
  });

  it("ends an admitted thesis when a later record of its lineage is refused lineage_retired", () => {
    const view = thesesOf([thesis({ thesis_id: "T1" }, at(10)), thesis({ thesis_id: "T2", lineage_id: "L-T1", revision: 1, predecessor_thesis_id: "T1", admitted: false, reason: "lineage_retired" }, at(12), "ThesisRevised")], NOW);
    expect(view.theses.find((t) => t.thesis_id === "T1")).toMatchObject({ state: "lineage_retired", ended_at: at(12) });
  });

  it("makes a later admitted thesis on the instrument the current one, the earlier replaced", () => {
    const view = thesesOf([thesis({ thesis_id: "T1" }, at(10)), thesis({ thesis_id: "T2", lineage_id: "L-T1", revision: 1, predecessor_thesis_id: "T1" }, at(12), "ThesisRevised")], NOW);
    expect(view.theses.map((t) => [t.thesis_id, t.state, t.ended_at])).toEqual([
      ["T2", "current", null],
      ["T1", "replaced", at(12)],
    ]);
  });

  it("never ends a thesis by an invalidation naming another instrument or another thesis", () => {
    const view = thesesOf([thesis({ thesis_id: "T1" }, at(10)), invalidated("T9", at(12)), invalidated("T1", at(13), "model", ABC)], NOW);
    expect(view.theses[0]).toMatchObject({ state: "current" });
  });

  it("reads nothing from a record whose body did not match its hash or whose payload does not decode, and counts it", () => {
    const tampered: JournalRecord = { ...thesis({ thesis_id: "T1" }, at(10)), matches: false, payload: null };
    const malformed = thesis({ thesis_id: "T2", admitted: "yes" }, at(11));
    const view = thesesOf([tampered, malformed, invalidated("T3", at(12)), { seq: 99, event_id: "E99", event_type: "OrderSubmitted", recorded_at: at(12), payload: {}, matches: true }], NOW);
    expect(view.theses).toEqual([]);
    expect(view.unverified).toBe(2);
  });
});
