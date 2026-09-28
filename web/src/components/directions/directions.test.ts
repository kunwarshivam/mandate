import { describe, expect, it } from "vitest";
import { contrastRatio, parseOklch } from "@/lib/color";
import { DIRECTIONS, buildDirectionCss, directionTextPairs, directionThemes } from "./directions";

const opaque = (v: string) => v.replace(/\s*\/\s*[\d.]+\s*\)$/, ")");

describe.each(DIRECTIONS.map((d) => [d.name, d] as const))("direction %s", (_, d) => {
  const themes = directionThemes(d);

  it.each(themes.flatMap((t) => directionTextPairs.map(([fg, bg]) => [t, fg, bg] as const)))("%s: %s on %s reaches WCAG AA", (theme, fg, bg) => {
    const set = d.tokens[theme] ?? d.tokens.light;
    if (!(fg in set) || !(bg in set)) return;
    expect(contrastRatio(set[fg], set[bg])).toBeGreaterThanOrEqual(4.5);
  });

  it.each(themes)("%s: every colour is OKLCH and the kill switch is the only crimson", (theme) => {
    const set = d.tokens[theme] ?? d.tokens.light;
    for (const [name, value] of Object.entries(set)) {
      const { c, h } = parseOklch(opaque(value));
      if (name.startsWith("crimson")) continue;
      const crimsonLike = c > 0.12 && h >= 20 && h <= 34 && parseOklch(opaque(value)).l < 0.6;
      expect(crimsonLike, `${name} reads as crimson`).toBe(false);
    }
  });

  it.each(themes)("%s: no purple or violet, and neutrals are tinted, never pure grey", (theme) => {
    const set = d.tokens[theme] ?? d.tokens.light;
    for (const [name, value] of Object.entries(set)) {
      const { c, h } = parseOklch(opaque(value));
      expect(c > 0.04 && h > 275 && h < 335, `${name} is purple`).toBe(false);
      expect(c, `${name} is an untinted grey`).toBeGreaterThan(0);
    }
  });

  it("scopes its CSS to the directions page", () => {
    const css = buildDirectionCss(d);
    expect(css.startsWith(`:root:has([data-direction="${d.id}"])`)).toBe(true);
  });
});

describe("the three directions", () => {
  it("differ in type, radius, and motion, not only in hue", () => {
    const keys = (f: (d: (typeof DIRECTIONS)[number]) => string) => new Set(DIRECTIONS.map(f)).size;
    expect(keys((d) => d.fonts.display)).toBe(3);
    expect(keys((d) => d.radius)).toBe(3);
    expect(keys((d) => d.motion)).toBe(3);
  });
});
