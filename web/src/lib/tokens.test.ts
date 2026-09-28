import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { contrastRatio, parseOklch, toHex } from "./color";
import { colorTokens, markPairs, textPairs, tokenValue } from "./tokens";

const css = readFileSync(resolve(process.cwd(), "src/app/globals.css"), "utf8");

function block(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(start, css.indexOf("}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/--([a-z-]+):\s*([^;]+);/g)) vars[m[1]] = m[2].trim();
  return vars;
}

const declared = block(":root");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : [];
  });
}

describe("colour tokens", () => {
  it.each(colorTokens.map((t) => [t.name, t] as const))("%s matches globals.css", (_, token) => {
    expect(declared[token.name]).toBe(token.value);
  });

  it("every colour is OKLCH", () => {
    for (const t of colorTokens) expect(() => parseOklch(t.value)).not.toThrow();
  });

  it.each(textPairs.map((p) => [p.fg, p.bg] as const))("%s on %s reaches WCAG AA (4.5:1)", (fg, bg) => {
    expect(contrastRatio(tokenValue(fg), tokenValue(bg))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(markPairs.map((p) => [p.fg, p.bg] as const))("%s marks on %s reach 3:1", (fg, bg) => {
    expect(contrastRatio(tokenValue(fg), tokenValue(bg))).toBeGreaterThanOrEqual(3);
  });

  it("converts OKLCH white and black to sRGB hex", () => {
    expect(toHex("oklch(1 0 0)")).toBe("#ffffff");
    expect(toHex("oklch(0 0 0)")).toBe("#000000");
    expect(contrastRatio("oklch(0 0 0)", "oklch(1 0 0)")).toBeCloseTo(21, 0);
  });

  it("keeps crimson for the kill switch distinct from the colour of a loss", () => {
    expect(tokenValue("crimson")).not.toBe(tokenValue("loss"));
    expect(parseOklch(tokenValue("crimson")).h - parseOklch(tokenValue("loss")).h).toBeGreaterThanOrEqual(15);
  });

  it("tints every neutral: no pure grey, black, or white", () => {
    for (const t of colorTokens.filter((c) => c.meaning === "surface" || c.meaning === "text")) {
      expect(parseOklch(t.value).c, t.name).toBeGreaterThan(0);
    }
  });

  it("has no purple or violet hue", () => {
    for (const t of colorTokens) {
      const { c, h } = parseOklch(t.value);
      if (c > 0.04) expect(h < 280 || h > 330, `${t.name} hue ${h}`).toBe(true);
    }
  });

  it("is light only: no dark theme block, no dark variant, no dark: class", () => {
    expect(css).not.toMatch(/^\.dark\s*\{/m);
    expect(css).not.toContain("@custom-variant dark");
    expect(css).toContain("color-scheme: light");
    for (const file of sources(resolve(process.cwd(), "src"))) {
      expect(readFileSync(file, "utf8"), file).not.toMatch(/\bdark:/);
    }
  });

  it("uses no rounded corners", () => {
    for (const m of css.matchAll(/--radius[a-z0-9-]*:\s*([^;]+);/g)) expect(m[1].trim()).toBe("0rem");
  });
});
