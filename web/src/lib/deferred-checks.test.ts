// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APPROVER_SENTENCE, DEFERRED_CHECK_LABELS, deferredChecks } from "./deferred-checks";

type Draft = { payload: { content: Record<string, unknown> } };
type Change = { path: string; value?: unknown; delete?: true };
type Case = { name: string; base_draft: string; changes: Change[] };
const vectors = JSON.parse(readFileSync(new URL("../../../fixtures/refcases/journal.json", import.meta.url), "utf8")) as {
  approval_deferrals: { approver_sentence: string; drafts: Record<string, Draft>; valid_drafts: Case[]; invalid_drafts: Case[] };
};
const section = vectors.approval_deferrals;

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
      expect(deferredChecks(contentOf(c)), c.name).toEqual({ kind: "named", names });
    }
    const v3 = section.drafts.requested_v3.payload.content;
    expect(deferredChecks(v3)).toEqual({ kind: "named", names: v3.deferred_to_binding });
  });

  it("finds nothing deferred in a version 1 or 2 request, which has no member", () => {
    for (const draft of ["requested_v1", "requested_v2"]) {
      const content = section.drafts[draft].payload.content;
      expect(content).not.toHaveProperty("deferred_to_binding");
      expect(deferredChecks(content), draft).toEqual({ kind: "none" });
    }
  });

  it("reads a version 3 request stripped of its list as nothing deferred, the one reading the journal's refusal covers", () => {
    const stripped = section.invalid_drafts.find((c) => c.name === "v3_without_its_deferrals");
    expect(stripped, "the vectors' v3_without_its_deferrals case").toBeTruthy();
    expect(deferredChecks(contentOf(stripped!))).toEqual({ kind: "none" });
  });

  it("never reads a member it cannot read as nothing deferred: null, empty, not a list, or not names", () => {
    const present = section.invalid_drafts.filter((c) => Object.hasOwn(contentOf(c), "deferred_to_binding"));
    expect(present.map((c) => c.name)).toContain("v3_with_null_deferrals");
    for (const c of present) expect(deferredChecks(contentOf(c)), c.name).not.toEqual({ kind: "none" });
    for (const value of [null, [], "halt", { halt: true }, [1], ["halt", null], [""], ["Halt"], ["halt", "halt"]]) {
      expect(deferredChecks({ deferred_to_binding: value }), JSON.stringify(value)).toEqual({ kind: "unreadable" });
    }
  });

  it("shows a name it has no label for as itself rather than dropping it", () => {
    expect(deferredChecks({ deferred_to_binding: ["halt", "new_check"] })).toEqual({ kind: "named", names: ["halt", "new_check"] });
  });
});
