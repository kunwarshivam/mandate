import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";
import { FINAL_UNDELIVERED, refusedText, undeliveredText } from "./grant-refusals";

/**
 * Every reason `ApprovalResponded` may carry, read from the journal spec's own field table (§9,
 * `ApprovalResponded`'s `reason`), so a code the runtime gains reaches this test without anyone
 * copying it here.
 */
function journaledReasons(): string[] {
  const spec = readFileSync(join(process.cwd(), "../docs/specs/journal.md"), "utf8");
  const section = spec.slice(spec.indexOf("**`ApprovalResponded`** on the agent stream"));
  const row = section.split("\n").find((line) => line.startsWith("| `reason` |"));
  if (!row) throw new Error("the journal spec's ApprovalResponded reason row");
  return [...row.split(/(?<!\\)\|/)[2].matchAll(/`([a-z_]+)`/g)].map((m) => m[1]);
}

const SUCCESS = /\b(you approved|approved:|allowed it|was sent)\b/i;

describe("a refused grant in words (DEC-2129 item 7, journal spec rule 47)", () => {
  it("reads every refusal the runtime can journal in its own words, never as a success", () => {
    const reasons = journaledReasons();
    expect(reasons).toContain("not_an_approver");
    expect(reasons.length).toBeGreaterThanOrEqual(11);
    const fallback = refusedText(null);
    const seen = new Set<string>();
    for (const reason of reasons) {
      const text = refusedText(reason);
      expect(text, reason).toMatch(/^Refused: /);
      expect(text, reason).not.toBe(fallback);
      expect(text, reason).not.toMatch(SUCCESS);
      seen.add(text);
    }
    expect(seen.size, "one sentence per reason").toBe(reasons.length);
  });

  it("names a refusal from someone who is no longer an approver", () => {
    expect(refusedText("not_an_approver")).toMatch(/not an approver/);
  });

  it("reads a reason it has no words for, or none, as refused with nothing approved", () => {
    for (const reason of [null, "", "a_reason_from_later"]) {
      expect(refusedText(reason), String(reason)).toBe("Refused by your deployment. Nothing was approved.");
    }
  });

  it("reads the API's refusal of a grant from someone the mandate does not list, and it is final", () => {
    for (const reason of ["forbidden", "role_forbidden"]) {
      expect(undeliveredText(reason), reason).toMatch(/^Not recorded: .*not an approver.*nothing was approved\.$/);
      expect(FINAL_UNDELIVERED.has(reason), reason).toBe(true);
    }
    expect(FINAL_UNDELIVERED.has("step_up_missing"), "a missing confirmation can be confirmed and sent again").toBe(false);
    expect(undeliveredText("network")).toBe("Not recorded: your deployment answered that nothing was recorded, so nothing was approved.");
  });
});
