// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APPROVER_SENTENCE, DEFERRED_CHECK_LABELS, deferredChecks } from "./deferred-checks";

type Draft = { schema_version: number; payload: { content: Record<string, unknown> } };
type Change = { path: string; value?: unknown; delete?: true };
type Case = { name: string; base_draft: string; changes: Change[] };
type Batch = { name: string; decision: string; request: { base_draft: string; changes: Change[] }; expect: { outcome: "Valid" | "Invalid" } };
const vectors = JSON.parse(readFileSync(new URL("../../../fixtures/refcases/journal.json", import.meta.url), "utf8")) as {
  approval_deferrals: {
    approver_sentence: string;
    drafts: Record<string, Draft>;
    decisions: Record<string, { payload: { deferred_to_binding?: string[] } }>;
    valid_drafts: Case[];
    invalid_drafts: Case[];
    batches: Batch[];
  };
};
const section = vectors.approval_deferrals;

/**
 * The vectors' batch whose request names a decision outside it: the journal admits it, since rule
 * 292 judges one batch, but no such decision is on the stream, so the read serves `null` and the
 * screen refuses it (DEC-2519).
 */
const ELSEWHERE = "request_naming_another_decision";

/** The version of a case's draft. */
const versionOf = (c: Case) => section.drafts[c.base_draft].schema_version;

/** A draft's content with the case's changes to `payload.content.deferred_to_binding` applied. */
function contentOf(c: Case): Record<string, unknown> {
  const content = structuredClone(section.drafts[c.base_draft].payload.content);
  for (const change of c.changes) {
    if (change.path !== "payload.content.deferred_to_binding") continue;
    if (change.delete) delete content.deferred_to_binding;
    else content.deferred_to_binding = change.value;
  }
  return content;
}

describe("the checks an approval's dry run deferred (DEC-2515 item 5, mandate spec §6.4)", () => {
  it("introduces them with the vectors' sentence, exactly", () => {
    expect(APPROVER_SENTENCE).toBe(section.approver_sentence);
  });

  it("labels every name the journal spec allows, and no other", () => {
    const every = section.valid_drafts.find((c) => c.name === "every_deferral");
    expect(every, "the vectors' every_deferral case").toBeTruthy();
    expect(Object.keys(DEFERRED_CHECK_LABELS).sort()).toEqual(contentOf(every!).deferred_to_binding);
    for (const label of Object.values(DEFERRED_CHECK_LABELS)) expect(label).not.toMatch(/pass|clear|ok\b|safe/i);
  });

  it("reads every valid version 3 request's names, in the order it gives them", () => {
    for (const c of section.valid_drafts) {
      const names = contentOf(c).deferred_to_binding as string[];
      expect(versionOf(c), c.name).toBe(3);
      expect(deferredChecks(contentOf(c), 3, names), c.name).toEqual({ kind: "named", names });
    }
    const v3 = section.drafts.requested_v3;
    expect(v3.schema_version).toBe(3);
    expect(deferredChecks(v3.payload.content, 3, v3.payload.content.deferred_to_binding as string[])).toEqual({ kind: "named", names: v3.payload.content.deferred_to_binding });
  });

  it("finds nothing deferred in a version 1 or 2 request, which has no member", () => {
    for (const draft of ["requested_v1", "requested_v2"]) {
      const { schema_version, payload } = section.drafts[draft];
      expect(payload.content).not.toHaveProperty("deferred_to_binding");
      expect(deferredChecks(payload.content, schema_version, []), draft).toEqual({ kind: "none" });
    }
  });

  it("reads a version 3 request stripped of its list as unreadable, never as nothing deferred (DEC-2518)", () => {
    const stripped = section.invalid_drafts.find((c) => c.name === "v3_without_its_deferrals");
    expect(stripped, "the vectors' v3_without_its_deferrals case").toBeTruthy();
    expect(versionOf(stripped!)).toBe(3);
    expect(deferredChecks(contentOf(stripped!), 3, [])).toEqual({ kind: "unreadable" });
  });

  it("reads a version 1 or 2 request that names deferred checks as unreadable: those versions name none", () => {
    const named = section.invalid_drafts.find((c) => c.name === "v2_with_deferrals");
    expect(named, "the vectors' v2_with_deferrals case").toBeTruthy();
    expect(versionOf(named!)).toBe(2);
    expect(deferredChecks(contentOf(named!), 2, ["halt", "liquidity_floor", "participation"])).toEqual({ kind: "unreadable" });
    expect(deferredChecks({ deferred_to_binding: ["halt"] }, 1, ["halt"])).toEqual({ kind: "unreadable" });
  });

  it("never reads a member it cannot read as nothing deferred: null, empty, not a list, or not names", () => {
    expect(section.invalid_drafts.map((c) => c.name)).toContain("v3_with_null_deferrals");
    for (const c of section.invalid_drafts) expect(deferredChecks(contentOf(c), versionOf(c), []), c.name).not.toEqual({ kind: "none" });
    for (const value of [null, [], "halt", { halt: true }, [1], ["halt", null], [""], ["Halt"], ["halt", "halt"]]) {
      expect(deferredChecks({ deferred_to_binding: value }, 3, ["halt"]), JSON.stringify(value)).toEqual({ kind: "unreadable" });
    }
  });

  it("shows a name it has no label for as itself rather than dropping it", () => {
    expect(deferredChecks({ deferred_to_binding: ["halt", "new_check"] }, 3, ["halt", "new_check"])).toEqual({ kind: "named", names: ["halt", "new_check"] });
  });

  it("reads each pairing of a decision and its request as the journal judges it: one it refuses is unreadable, wherever the two were written (DEC-2519)", () => {
    expect(section.batches.length).toBeGreaterThan(0);
    for (const b of section.batches) {
      const content = contentOf({ name: b.name, ...b.request });
      const version = section.drafts[b.request.base_draft].schema_version;
      const own = deferredChecks(content, version, (content.deferred_to_binding as string[] | undefined) ?? []);
      if (b.name === ELSEWHERE) {
        expect(deferredChecks(content, version, null), b.name).toEqual({ kind: "unreadable" });
        continue;
      }
      const decided = section.decisions[b.decision].payload.deferred_to_binding ?? [];
      const read = deferredChecks(content, version, decided);
      if (b.expect.outcome === "Invalid") expect(read, b.name).toEqual({ kind: "unreadable" });
      else expect(read, b.name).toEqual(own);
    }
    expect(section.batches.map((b) => b.name), "the vector whose decision is elsewhere, which the read serves as null").toContain(ELSEWHERE);
    const decidedBy = (b: Batch) => {
      const content = contentOf({ name: b.name, ...b.request });
      return deferredChecks(content, section.drafts[b.request.base_draft].schema_version, (content.deferred_to_binding as string[] | undefined) ?? []).kind;
    };
    expect(
      section.batches.filter((b) => b.expect.outcome === "Invalid" && decidedBy(b) !== "unreadable").map((b) => b.name),
      "pairings the request alone reads as fine, so only the comparison refuses them",
    ).toEqual(expect.arrayContaining(["v2_request_hides_a_deferral", "v1_request_hides_a_deferral", "v3_request_names_another_deferral", "v3_request_on_a_plain_decision"]));
    expect(section.batches.filter((b) => b.expect.outcome === "Invalid").map((b) => b.name)).toEqual(
      expect.arrayContaining(["v2_request_hides_a_deferral", "v1_request_hides_a_deferral", "v3_request_names_another_deferral", "v3_request_on_a_plain_decision"]),
    );
  });

  it("compares the lists in order, so the same names in another order disagree", () => {
    expect(deferredChecks({ deferred_to_binding: ["halt", "liquidity_floor", "participation"] }, 3, ["participation", "liquidity_floor", "halt"])).toEqual({ kind: "unreadable" });
    expect(deferredChecks({ deferred_to_binding: ["halt", "participation"] }, 3, ["halt", "liquidity_floor", "participation"])).toEqual({ kind: "unreadable" });
  });

  it("never reads a request as nothing deferred when its decision cannot be found or its list cannot be read", () => {
    for (const decided of [null, "halt", [""], [1], ["halt", "halt"]]) {
      expect(deferredChecks({}, 2, decided), JSON.stringify(decided)).toEqual({ kind: "unreadable" });
      expect(deferredChecks({ deferred_to_binding: ["halt"] }, 3, decided), JSON.stringify(decided)).toEqual({ kind: "unreadable" });
    }
  });
});
