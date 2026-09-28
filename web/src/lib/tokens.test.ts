import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { CHART_FONT } from "@/components/charts/options";
import { contrastRatio, parseOklch, toHex } from "./color";
import { hatchInk } from "./palette";
import { colorTokens, markPairs, textPairs, tokenValue } from "./tokens";

const css = readFileSync(resolve(process.cwd(), "src/app/globals.css"), "utf8");

function block(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(start, css.indexOf("}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/--([a-z0-9-]+):\s*([^;]+);/g)) vars[m[1]] = m[2].trim();
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

  it("declares the paper hatch from the palette, and no colour token the palette does not name", () => {
    expect(declared["hatch-ink"]).toBe(hatchInk());
    const named = new Set(colorTokens.map((t) => t.name));
    const colours = Object.entries(declared).filter(([, v]) => /^oklch\(/.test(v)).map(([k]) => k);
    expect(colours.filter((k) => !named.has(k as never) && k !== "hatch-ink")).toEqual([]);
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

  it("rounds only by the radius scale, and never rounds past a pill", () => {
    const theme = block("@theme inline");
    const radii = Object.entries(theme).filter(([k]) => /^radius-/.test(k));
    expect(radii.map(([k]) => k)).toEqual(["radius-xs", "radius-sm", "radius-md", "radius-lg", "radius-xl", "radius-2xl", "radius-3xl", "radius-4xl"]);
    const rem = radii.map(([, v]) => Number(/^([\d.]+)rem$/.exec(v)?.[1]));
    for (let i = 1; i < rem.length; i++) expect(rem[i]).toBeGreaterThan(rem[i - 1]);
    expect(declared.radius).toBe(theme["radius-md"]);
  });

  it("casts no shadow on a control: only floating layers have one", () => {
    const theme = block("@theme inline");
    for (const k of ["shadow-2xs", "shadow-xs", "shadow-sm"]) expect(theme[k], k).toBe("0 0 #0000");
    for (const k of ["shadow-md", "shadow-lg", "shadow-xl", "shadow-2xl"]) expect(theme[k], k).not.toMatch(/oklch\(|rgb|#[0-9a-f]{3,6}\b/i);
  });
});

describe("figures with a plain zero", () => {
  const theme = block("@theme inline");
  const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf8")) as { dependencies: Record<string, string> };

  it("sets the body, the figures class and the charts in one face, Mona Sans, with no second family", () => {
    for (const stack of [theme["font-mono"], theme["font-sans"]]) expect(stack).toMatch(/^"Mona Sans Variable", ui-sans-serif,/);
    expect(CHART_FONT).toMatch(/^'Mona Sans Variable', ui-sans-serif,/);
    expect(Object.keys(pkg.dependencies).filter((d) => d.startsWith("@fontsource"))).toEqual(["@fontsource-variable/mona-sans"]);
    expect(css).not.toMatch(/@font-face/);
  });

  it("ships the face self-hosted from the package, with no font from a third-party host", () => {
    const file = resolve(process.cwd(), "node_modules/@fontsource-variable/mona-sans/files/mona-sans-latin-wght-normal.woff2");
    expect(existsSync(file)).toBe(true);
    const layout = readFileSync(resolve(process.cwd(), "src/app/layout.tsx"), "utf8");
    expect(layout).toContain('import "@fontsource-variable/mona-sans";');
    expect(layout).not.toMatch(/next\/font|fonts\.googleapis|fonts\.gstatic/);
  });

  it("uses weights 400 to 600 only: no heavy display weights", () => {
    for (const [k, v] of Object.entries(theme).filter(([k]) => k.endsWith("--font-weight"))) expect(Number(v), k).toBeLessThanOrEqual(600);
    for (const file of sources(resolve(process.cwd(), "src")).filter((f) => !/\/(design|palette)\//.test(f))) {
      expect(readFileSync(file, "utf8"), file).not.toMatch(/\bfont-(bold|extrabold|black)\b/);
    }
  });

  it("sets tabular figures on the figures class, and never asks for a slashed zero", () => {
    const mono = css.slice(css.indexOf(".font-mono {"), css.indexOf("}", css.indexOf(".font-mono {")));
    expect(mono).toContain("font-variant-numeric: tabular-nums;");
    const tabular = css.slice(css.indexOf("@utility tabular {"), css.indexOf("}", css.indexOf("@utility tabular {")));
    expect(tabular).toContain("font-variant-numeric: tabular-nums;");
    expect(css).not.toMatch(/slashed-zero|["']zero["']/);
  });
});
