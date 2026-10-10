import { describe, expect, it } from "vitest";
import { SPRING_VARS, type Spring, settleTime, springAt, springEasing, stepSpring } from "./spring";

/**
 * The long page's springs (DEC-907): the curves CSS settles pieces on are real damped springs, and
 * the stepped spring the owl flies on settles, overshoots only when underdamped, and survives a slow
 * frame. Each expectation comes from the closed form, not from the code under test.
 */

/** Peak overshoot of a unit step response, from the damping ratio alone. */
const peakOf = (zeta: number) => 1 + Math.exp((-zeta * Math.PI) / Math.sqrt(1 - zeta * zeta));

const stops = (css: string) => css.replace(/^linear\(|\)$/g, "").split(",").map(Number);

describe("the CSS spring curves", () => {
  it.each([0.38, 0.55, 0.7])("samples a spring with damping ratio %s from rest to rest, peaking where the closed form says", (zeta) => {
    const curve = stops(springEasing(zeta));
    expect(curve[0]).toBe(0);
    expect(curve.at(-1)).toBe(1);
    expect(Math.max(...curve)).toBeCloseTo(peakOf(zeta), 1);
    expect(Math.abs(springAt(zeta, settleTime(zeta)) - 1)).toBeLessThanOrEqual(0.004 + 1e-9);
  });

  it("gives the page a soft settle, a firmer one and a bounce, each bouncier than the last", () => {
    const peaks = [SPRING_VARS["--spring-soft"], SPRING_VARS["--spring"], SPRING_VARS["--spring-bounce"]].map((c) => Math.max(...stops(c)));
    expect(peaks[0]).toBeGreaterThan(1);
    expect(peaks[1]).toBeGreaterThan(peaks[0]!);
    expect(peaks[2]).toBeGreaterThan(peaks[1]!);
    for (const curve of Object.values(SPRING_VARS)) expect(curve).toMatch(/^linear\((-?\d+(\.\d+)?, )+1\)$/);
  });
});

describe("the stepped spring", () => {
  function run(stiffness: number, damping: number, seconds: number, frame = 1 / 60) {
    const s: Spring = { x: 0, v: 0 };
    let peak = 0;
    for (let t = 0; t < seconds; t += frame) {
      stepSpring(s, 100, frame, { stiffness, damping });
      peak = Math.max(peak, s.x);
    }
    return { s, peak };
  }

  it("comes to rest on its target", () => {
    const { s } = run(140, 15, 4);
    expect(s.x).toBeCloseTo(100, 1);
    expect(Math.abs(s.v)).toBeLessThan(0.1);
  });

  it("overshoots by the closed form's amount when underdamped, and never when critically damped", () => {
    const stiffness = 140;
    const zeta = 15 / (2 * Math.sqrt(stiffness));
    expect(run(stiffness, 15, 4).peak / 100).toBeCloseTo(peakOf(zeta), 1);
    expect(run(stiffness, 2 * Math.sqrt(stiffness), 4).peak).toBeLessThanOrEqual(100);
  });

  it("takes a slow frame in small steps, so it lands where a smooth run does", () => {
    const smooth = run(140, 15, 1, 1 / 240).s.x;
    const choppy = run(140, 15, 1, 1 / 8).s.x;
    expect(Number.isFinite(choppy)).toBe(true);
    expect(choppy).toBeCloseTo(smooth, 0);
  });

  it("ignores a negative or missing step", () => {
    const s: Spring = { x: 3, v: 2 };
    stepSpring(s, 100, -1, { stiffness: 140, damping: 15 });
    stepSpring(s, 100, 0, { stiffness: 140, damping: 15 });
    expect(s).toEqual({ x: 3, v: 2 });
  });
});
