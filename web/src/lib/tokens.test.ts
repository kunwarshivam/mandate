import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { CHART_FONT } from "@/components/charts/options";
import { composite, contrastRatio, parseOklch, rgbContrast, toHex, toRgb255 } from "./color";
import { PALETTES, TOKEN_NAMES, type ThemeName, hatchInk } from "./palette";
import { colorTokens, colorTokensFor, markPairs, textPairs, tokenValue, typeScale } from "./tokens";

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
const declaredDark = block('html:root[data-mode="dark"]');
const BLOCKS: Record<ThemeName, Record<string, string>> = { light: declared, dark: declaredDark };
const THEMES: ThemeName[] = ["light", "dark"];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : [];
  });
}

describe("colour tokens", () => {
  it.each(THEMES.flatMap((theme) => colorTokensFor(theme).map((t) => [theme, t.name, t] as const)))("%s %s matches globals.css", (theme, _, token) => {
    expect(BLOCKS[theme][token.name]).toBe(token.value);
  });

  it.each(THEMES)("declares the %s paper hatch from the palette, and no colour token the palette does not name", (theme) => {
    const block = BLOCKS[theme];
    expect(block["hatch-ink"]).toBe(hatchInk(PALETTES[theme]));
    const named = new Set<string>(TOKEN_NAMES);
    const colours = Object.entries(block).filter(([, v]) => /^oklch\(/.test(v)).map(([k]) => k);
    expect(colours.filter((k) => !named.has(k) && k !== "hatch-ink")).toEqual([]);
  });

  it("every colour is OKLCH", () => {
    for (const t of colorTokens) expect(() => parseOklch(t.value)).not.toThrow();
  });

  it.each(THEMES.flatMap((theme) => textPairs.map((p) => [theme, p.fg, p.bg] as const)))("%s: %s on %s reaches WCAG AA (4.5:1)", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(THEMES.flatMap((theme) => markPairs.map((p) => [theme, p.fg, p.bg] as const)))("%s: %s marks on %s reach 3:1", (theme, fg, bg) => {
    expect(contrastRatio(tokenValue(fg, theme), tokenValue(bg, theme))).toBeGreaterThanOrEqual(3);
  });

  it("converts OKLCH white and black to sRGB hex", () => {
    expect(toHex("oklch(1 0 0)")).toBe("#ffffff");
    expect(toHex("oklch(0 0 0)")).toBe("#000000");
    expect(contrastRatio("oklch(0 0 0)", "oklch(1 0 0)")).toBeCloseTo(21, 0);
  });

  it.each(THEMES)("keeps crimson for the kill switch distinct from the colour of a loss in %s", (theme) => {
    for (const kill of ["crimson", "crimson-edge"]) {
      expect(tokenValue(kill, theme)).not.toBe(tokenValue("loss", theme));
      expect(Math.abs(parseOklch(tokenValue(kill, theme)).h - parseOklch(tokenValue("loss", theme)).h)).toBeGreaterThanOrEqual(15);
    }
  });

  it.each(THEMES)("tints every neutral in %s: no pure grey, black, or white", (theme) => {
    for (const t of colorTokensFor(theme).filter((c) => c.meaning === "surface" || c.meaning === "text")) {
      expect(parseOklch(t.value).c, t.name).toBeGreaterThan(0);
    }
  });

  it.each(THEMES)("has no purple or violet hue in %s", (theme) => {
    for (const t of colorTokensFor(theme)) {
      const { c, h } = parseOklch(t.value);
      if (c > 0.04) expect(h < 280 || h > 330, `${t.name} hue ${h}`).toBe(true);
    }
  });

  it("switches to dark by tokens alone: one dark block re-declares every colour, and our source has no dark: class", () => {
    expect(Object.keys(declaredDark).filter((k) => /^oklch\(/.test(declaredDark[k])).sort()).toEqual([...TOKEN_NAMES, "hatch-ink"].sort());
    expect(declared["color-scheme"]).toBeUndefined();
    expect(css.slice(css.indexOf(":root {"), css.indexOf("}", css.indexOf(":root {")))).toContain("color-scheme: light");
    expect(css.slice(css.indexOf('html:root[data-mode="dark"] {'), css.indexOf("}", css.indexOf('html:root[data-mode="dark"] {')))).toContain("color-scheme: dark");
    expect(css).not.toMatch(/^\.dark\s*\{/m);
    for (const file of sources(resolve(process.cwd(), "src"))) {
      expect(readFileSync(file, "utf8"), file).not.toMatch(/(^|[\s"'`])dark:[a-z[]/m);
    }
  });

  it("keeps the dark variant only for Kumo, whose own classes use it, and points it at the class the theme script sets", () => {
    expect(css).toContain("@custom-variant dark (&:where(.dark, .dark *));");
    const kumo = readFileSync(resolve(process.cwd(), "node_modules/@cloudflare/kumo/dist/styles/theme-kumo.css"), "utf8");
    const kumoJs = readdirSync(resolve(process.cwd(), "node_modules/@cloudflare/kumo/dist"), { recursive: true, withFileTypes: true })
      .filter((e) => e.isFile() && e.name.endsWith(".js"))
      .some((e) => /\bdark:/.test(readFileSync(join(e.parentPath, e.name), "utf8")));
    expect(kumoJs || /\.dark\b/.test(kumo)).toBe(true);
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

describe("the frame's glass", () => {
  const utility = css.slice(css.indexOf("@utility glass {"), css.indexOf("\n}\n", css.indexOf("@utility glass {")));
  const mix = /^color-mix\(in oklab, var\(--card\) (\d+)%, transparent\)$/;

  it.each(THEMES)("frosts the frame in %s with the card, translucent", (theme) => {
    const glass = BLOCKS[theme].glass;
    expect(glass).toMatch(mix);
    expect(declared["glass-edge"]).toBe("color-mix(in oklab, var(--foreground) 8%, transparent)");
  });

  // The blur only averages what scrolls underneath, so a solid token under the glass is the worst case.
  it.each(THEMES)("keeps the header's text at 4.5:1 and Stop's pill at 3:1 over any token scrolling under it, in %s", (theme) => {
    const alpha = Number(mix.exec(BLOCKS[theme].glass)![1]) / 100;
    const card = tokenValue("card", theme);
    const under = colorTokensFor(theme).map((t) => t.value);
    const worst = (fg: string) => Math.min(...under.map((u) => rgbContrast(toRgb255(fg), composite(card, alpha, u))));
    for (const text of ["foreground", "muted-foreground"]) expect(worst(tokenValue(text, theme)), text).toBeGreaterThanOrEqual(4.5);
    expect(worst(tokenValue("ink", theme)), "ink").toBeGreaterThanOrEqual(3);
  });

  describe("the dock's denser glass", () => {
    const pct = (name: string, of: string) => {
      const m = new RegExp(`^color-mix\\(in oklab, var\\(--${of}\\) (\\d+)%, transparent\\)$`).exec(declared[name] ?? "");
      if (!m) throw new Error(`--${name} is not a mix of --${of}`);
      return Number(m[1]) / 100;
    };
    const glassAlpha = pct("dock-glass", "card");
    const current = pct("dock-current", "foreground");
    const hover = pct("dock-hover", "foreground");

    it("is the card with the type colour for its edge, a 14% tint for the current pill and half that on hover", () => {
      expect(glassAlpha, "how opaque is look (DEC-739 item 1); the labels' contrast below is the rule").toBeGreaterThan(0);
      expect(pct("dock-edge", "foreground"), "how firm the edge is is look (DEC-739 item 1)").toBeGreaterThan(0);
      expect(current).toBe(0.14);
      expect(hover).toBe(current / 2);
      expect(declaredDark["dock-glass"], "one declaration serves both themes").toBeUndefined();
    });

    it.each(THEMES)("keeps every label at 4.5:1 and the pill visible over any token scrolling under the dock, in %s", (theme) => {
      const card = tokenValue("card", theme);
      const fg = tokenValue("foreground", theme);
      const under = colorTokensFor(theme).map((t) => t.value);
      const tint = (alpha: number, u: string) => {
        const glass = composite(card, glassAlpha, u);
        const ink = toRgb255(fg);
        return ink.map((v, i) => Math.round(v * alpha + glass[i] * (1 - alpha))) as typeof glass;
      };
      for (const u of under) {
        const glass = composite(card, glassAlpha, u);
        expect(rgbContrast(toRgb255(tokenValue("muted-foreground", theme)), glass), `idle label over ${u}`).toBeGreaterThanOrEqual(4.5);
        expect(rgbContrast(toRgb255(fg), tint(hover, u)), `hovered label over ${u}`).toBeGreaterThanOrEqual(4.5);
        expect(rgbContrast(toRgb255(fg), tint(current, u)), `current label over ${u}`).toBeGreaterThanOrEqual(4.5);
        expect(rgbContrast(tint(current, u), glass), `the pill against the glass over ${u}`).toBeGreaterThanOrEqual(1.3);
      }
      expect(rgbContrast(toRgb255(tokenValue("muted", theme)), toRgb255(card)), "muted alone would not show the pill").toBeLessThan(1.3);
    });
  });

  it("blurs and saturates what is behind, with the WebKit prefix", () => {
    expect(utility).toContain("background-color: var(--glass);");
    expect(utility).toContain("border-color: var(--glass-edge);");
    expect(utility).toContain("-webkit-backdrop-filter: blur(22px) saturate(1.8);");
    expect(utility).toMatch(/\n\s+backdrop-filter: blur\(22px\) saturate\(1\.8\);/);
  });

  it.each([
    ["no backdrop filter", "@supports not ((backdrop-filter: blur(1px)) or (-webkit-backdrop-filter: blur(1px)))"],
    ["reduced transparency", "@media (prefers-reduced-transparency: reduce)"],
    ["forced colours", "@media (forced-colors: active)"],
  ])("falls back to the solid card under %s", (_what, condition) => {
    const start = utility.indexOf(`${condition} {`);
    expect(start, condition).toBeGreaterThan(0);
    const body = utility.slice(start, utility.indexOf("}", start));
    expect(body).toContain("background-color: var(--card);");
  });
});

// DEC-213: the signed-out landing is a period piece and loads its own faces; the product stays in Public Sans.
// DEC-907: the long page under its desktop adds Manrope and Sorts Mill Goudy, through the same faces file.
const LANDING_FONTS = ["@fontsource-variable/pixelify-sans", "@fontsource/dotgothic16", "@fontsource/vt323", "@fontsource-variable/manrope", "@fontsource/sorts-mill-goudy"];

describe("figures with a plain zero", () => {
  const theme = block("@theme inline");
  const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf8")) as { dependencies: Record<string, string> };

  it("sets the body, the figures class and the charts in one face, Public Sans, with no second family", () => {
    for (const stack of [theme["font-mono"], theme["font-sans"]]) expect(stack).toMatch(/^"Public Sans Variable", ui-sans-serif,/);
    expect(CHART_FONT).toMatch(/^'Public Sans Variable', ui-sans-serif,/);
    expect(Object.keys(pkg.dependencies).filter((d) => d.startsWith("@fontsource") && !LANDING_FONTS.includes(d))).toEqual(["@fontsource-variable/public-sans"]);
    expect(css).not.toMatch(/@font-face/);
  });

  it("keeps the landing page's period faces on the signed-out pages, and lends the product only its chrome face", () => {
    const faces = readFileSync(resolve(process.cwd(), "src/components/site/faces.ts"), "utf8");
    for (const font of LANDING_FONTS) {
      expect(pkg.dependencies[font], font).toBeDefined();
      expect(faces, font).toContain(`import "${font}`);
    }
    const src = resolve(process.cwd(), "src");
    const importers = (readdirSync(src, { recursive: true }) as string[])
      .filter((f) => /\.tsx?$/.test(f) && /^import "(\.\/|@\/components\/site\/)faces";$/m.test(readFileSync(join(src, f), "utf8")))
      .map((f) => `src/${f}`)
      .sort();
    expect(importers, "DEC-469: the landing page and the sign-in pages' logon window").toEqual(["src/components/site/landing.tsx", "src/components/site/logon.tsx"]);
    const root = readFileSync(resolve(process.cwd(), "src/app/layout.tsx"), "utf8");
    expect(root, "DEC-452: the pixel-face titles' face, loaded beside Public Sans and before globals.css").toMatch(
      /import "@fontsource-variable\/pixelify-sans";\nimport "\.\/globals\.css";/,
    );
    expect(root).not.toMatch(/dotgothic|vt323/i);
    expect(readFileSync(resolve(process.cwd(), "src/components/shell/app-frame.tsx"), "utf8")).not.toMatch(/fontsource/);
    expect(css).toMatch(/@utility pixel-face \{\s*font-family: "Pixelify Sans Variable", /);
  });

  it("ships the face self-hosted from the package, with no font from a third-party host", () => {
    for (const subset of ["latin", "latin-ext"]) {
      const file = resolve(process.cwd(), `node_modules/@fontsource-variable/public-sans/files/public-sans-${subset}-wght-normal.woff2`);
      expect(existsSync(file), subset).toBe(true);
    }
    const layout = readFileSync(resolve(process.cwd(), "src/app/layout.tsx"), "utf8");
    expect(layout).toContain('import "@fontsource-variable/public-sans";');
    expect(layout).not.toMatch(/next\/font|fonts\.googleapis|fonts\.gstatic/);
  });

  it("never tracks a role tighter than -0.04em, where the hero's figures start to touch", () => {
    for (const [k, v] of Object.entries(theme).filter(([k]) => k.endsWith("--letter-spacing"))) {
      expect(Number.parseFloat(v), k).toBeGreaterThanOrEqual(-0.04);
    }
  });

  it("uses weights 400 to 600 only: no heavy display weights", () => {
    for (const [k, v] of Object.entries(theme).filter(([k]) => k.endsWith("--font-weight"))) expect(Number(v), k).toBeLessThanOrEqual(600);
    for (const file of sources(resolve(process.cwd(), "src")).filter((f) => !/\/(design|palette)\//.test(f))) {
      expect(readFileSync(file, "utf8"), file).not.toMatch(/\bfont-(bold|extrabold|black)\b/);
    }
  });

  it("sets the display hero in proportional lining figures and every other figure role in tabular ones", () => {
    const display = typeScale.find((t) => t.role === "display")!;
    expect(display.className.split(" ")).toEqual(expect.arrayContaining(["proportional-nums", "lining-nums"]));
    expect(display.className.split(" ")).not.toContain("tabular");
    expect(display.spec).toMatch(/^Public Sans 600, .* -0\.03em, proportional lining figures/);
    for (const role of ["hero", "figure", "number"]) expect(typeScale.find((t) => t.role === role)!.className.split(" "), role).toContain("tabular");
    expect(theme["text-display--letter-spacing"]).toBe("-0.03em");
  });

  it("sets tabular figures on the figures class, and never asks for a slashed zero", () => {
    const mono = css.slice(css.indexOf(".font-mono {"), css.indexOf("}", css.indexOf(".font-mono {")));
    expect(mono).toContain("font-variant-numeric: tabular-nums;");
    const tabular = css.slice(css.indexOf("@utility tabular {"), css.indexOf("}", css.indexOf("@utility tabular {")));
    expect(tabular).toContain("font-variant-numeric: tabular-nums;");
    expect(css).not.toMatch(/slashed-zero|["']zero["']/);
  });
});
