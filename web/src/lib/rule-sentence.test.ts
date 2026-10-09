// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { AutonomyRule } from "@/fixtures/types";
import { ruleSentence } from "./labels";

const rule = (op: string, value: string | string[], field = "purpose"): AutonomyRule => ({ id: "r", when: { field, op, value }, then: "ask" });

/** Each sentence is written out here, not built from the code's words, so a change to the wording fails. */
describe("an autonomy rule's sentence", () => {
  it("names two excluded values with neither and nor", () => {
    expect(ruleSentence(rule("not_in", ["increase", "open"]))).toBe("ask when the purpose is neither increase nor open");
  });

  it("names one excluded value with not, and three or more with none of", () => {
    expect(ruleSentence(rule("not_in", ["open"]))).toBe("ask when the purpose is not open");
    expect(ruleSentence(rule("not_in", ["a", "b", "c"], "venue"))).toBe("ask when the venue is none of a, b or c");
  });

  it("names included values with or", () => {
    expect(ruleSentence(rule("in", ["increase", "open"]))).toBe("ask when the purpose is increase or open");
  });

  it("names an operator it does not know by a generic phrase, never the raw operator", () => {
    const said = ruleSentence(rule("matches", "^XY"));
    expect(said).toBe("ask when the purpose meets this rule's condition");
    expect(said).not.toContain("matches");
  });

  it("leaves no trailing space for an empty list of values", () => {
    expect(ruleSentence(rule("in", []))).toBe("ask when the purpose meets this rule's condition");
    expect(ruleSentence(rule("not_in", []))).toBe("ask when the purpose meets this rule's condition");
  });

  it("reads a single-value comparison given several values by the generic phrase, never one of them alone", () => {
    expect(ruleSentence(rule("lt", ["0.65", "0.5"], "combined_score"))).toBe("ask when the combined model score meets this rule's condition");
    expect(ruleSentence(rule("lt", "0.65", "combined_score"))).toBe("ask when the combined model score is below 0.65");
  });
});
