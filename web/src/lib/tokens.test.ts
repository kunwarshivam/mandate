import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { contrastRatio, parseOklch, toHex } from "./color";
import { type Theme, colorTokens, textPairs, tokenValue } from "./tokens";

const css = readFileSync(resolve(process.cwd(), "src/app/globals.css"), "utf8");

function block(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(start, css.indexOf("}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/--([a-z-]+):\s*([^;]+);/g)) vars[m[1]] = m[2].trim();
  return vars;
}

const declared: Record<Theme, Record<string, string>> = { light: block(":root"), dark: block(".dark") };

describe("colour tokens", () => {
  it.each(colorTokens.map((t) => [t.name, t] as const))("%s matches globals.css in both themes", (_, token) => {
    expect(declared.light[token.name]).toBe(token.light);
    expect(declared.dark[token.name]).toBe(token.dark);
  });

  it("every colour is OKLCH", () => {
    for (const t of colorTokens) {
      expect(() => parseOklch(t.light)).not.toThrow();
      expect(() => parseOklch(t.dark)).not.toThrow();
    }
  });

  it.each(
    (["light", "dark"] as const).flatMap((theme) => textPairs.map((p) => [theme, p.fg, p.bg] as const)),
  )("%s: %s on %s reaches WCAG AA (4.5:1)", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(4.5);
  });

  it("converts OKLCH white and black to sRGB hex", () => {
    expect(toHex("oklch(1 0 0)")).toBe("#ffffff");
    expect(toHex("oklch(0 0 0)")).toBe("#000000");
    expect(contrastRatio("oklch(0 0 0)", "oklch(1 0 0)")).toBeCloseTo(21, 0);
  });

  it("keeps crimson for the kill switch distinct from the rose used for losses", () => {
    for (const theme of ["light", "dark"] as const) {
      expect(tokenValue("crimson", theme)).not.toBe(tokenValue("rose", theme));
    }
  });
});
