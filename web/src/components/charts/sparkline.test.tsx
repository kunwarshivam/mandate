import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { Point } from "@/fixtures/market";
import { Sparkline } from "./sparkline";

const HEIGHT = 40;
const LO = 10_000;
const HI = 10_120;
const POINTS: Point[] = [
  { time: 0, value: 10_040 },
  { time: 60, value: LO },
  { time: 120, value: 10_080 },
  { time: 180, value: HI },
  { time: 240, value: 10_060 },
];

afterEach(cleanup);

function drawWithLimit(limit: number | null) {
  render(<Sparkline points={POINTS} limit={limit} label="equity today" height={HEIGHT} />);
  const svg = screen.getByRole("img", { name: "equity today" });
  return {
    band: svg.querySelector("[data-slot=sparkline-limit-band]"),
    rule: svg.querySelector("[data-slot=sparkline-limit]"),
  };
}

describe("Sparkline's daily loss limit (critique C-11)", () => {
  it.each([
    ["far below the line's range", LO - 5_000],
    ["just below the line's range", LO - 0.01],
  ])("draws only the pale sliver at the foot when the limit is %s, with no dashed rule", (_, limit) => {
    const { band, rule } = drawWithLimit(limit);
    expect(rule, "a dashed rule under the lowest point reads as the agent touching its limit").toBeNull();
    expect(band).not.toBeNull();
    const top = Number(band!.getAttribute("y"));
    const tall = Number(band!.getAttribute("height"));
    expect(tall).toBeGreaterThan(0);
    expect(top + tall).toBe(HEIGHT);
  });

  it.each([
    ["inside the line's range", 10_060],
    ["exactly at the range's minimum", LO],
    ["exactly at the range's maximum", HI],
  ])("draws the dashed rule on top of the band when the limit is %s", (_, limit) => {
    const { band, rule } = drawWithLimit(limit);
    expect(band).not.toBeNull();
    expect(rule).not.toBeNull();
    expect(rule!.getAttribute("stroke-dasharray")).toBeTruthy();
    expect(rule!.getAttribute("class")).toContain("stroke-mandate-marker");
    expect(rule!.getAttribute("y1")).toBe(band!.getAttribute("y"));
    expect(rule!.getAttribute("y2")).toBe(band!.getAttribute("y"));
  });

  it("fills the chart with the band and keeps the rule on its top edge when the limit is above the line's range (DEC-738 item 3)", () => {
    const { band, rule } = drawWithLimit(HI + 5_000);
    expect(band).not.toBeNull();
    expect(Number(band!.getAttribute("y"))).toBe(0);
    expect(Number(band!.getAttribute("height"))).toBe(HEIGHT);
    expect(rule, "the whole day is past the limit, so the rule stays at the top").not.toBeNull();
    expect(Number(rule!.getAttribute("y1"))).toBe(0);
    expect(Number(rule!.getAttribute("y2"))).toBe(0);
  });

  it("draws neither band nor rule without a daily loss limit", () => {
    const { band, rule } = drawWithLimit(null);
    expect(band).toBeNull();
    expect(rule).toBeNull();
  });
});
