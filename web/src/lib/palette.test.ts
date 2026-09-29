import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { composite, contrastRatio, inGamut, parseOklch, rgbContrast, toRgb255 } from "./color";
import { apcaLc, passesApca } from "@/test/apca";
import { MARKERS } from "../../scripts/no-apca.mjs";
import { checkCvd, checkPalette, measure } from "./contrast";
import { CVD_DISTINCT, HATCH_MAX, HATCH_MIN, KUMO_PAIRS, type KumoScope, PAIRS, STOP_CONTRAST } from "./contrast-pairs";
import { GOLD_HUE, LIGHTNESS, PALETTE, PALETTE_DARK, PALETTES, RAMPS, type RampId, STEPS, type Step, TOKEN_NAMES, type ThemeName, type TokenName } from "./palette";

const SRC = resolve(process.cwd(), "src");
const css = readFileSync(join(SRC, "app/globals.css"), "utf8");
const kumoCss = readFileSync(join(SRC, "app/kumo-theme.css"), "utf8");
const kumoTheme = readFileSync(resolve(process.cwd(), "node_modules/@cloudflare/kumo/dist/styles/theme-kumo.css"), "utf8");
const THEMES: ThemeName[] = ["light", "dark"];
const DARK = 'html:root[data-mode="dark"]';

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return sources(path);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : [];
  });
}

/** The custom properties declared directly in the first block that opens with this selector. */
function declarations(text: string, selector: string): Record<string, string> {
  const start = text.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} block`);
  const body = text.slice(start, text.indexOf("}", start));
  return Object.fromEntries(Array.from(body.matchAll(/--([a-z0-9-]+):\s*([^;]+);/g), (m) => [m[1], m[2].trim()]));
}

/**
 * Custom properties resolve where they are declared and inherit as values, so a surface that
 * re-points `--foreground` does not change a Kumo role it does not re-declare. This follows that.
 */
function resolveScope(declared: Record<string, string>, inherited: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = { ...inherited };
  const resolve = (name: string, seen: string[]): string => {
    if (seen.includes(name)) throw new Error(`cycle ${[...seen, name].join(" -> ")}`);
    const raw = declared[name];
    if (raw === undefined) {
      if (inherited[name] === undefined) throw new Error(`--${name} is not defined`);
      return inherited[name];
    }
    return raw.replace(/var\(--([a-z0-9-]+)\)/g, (_, ref: string) => resolve(ref, [...seen, name]));
  };
  for (const name of Object.keys(declared)) out[name] = resolve(name, []);
  return out;
}

/** The dark block sits on `<html>` itself, so it re-declares on the root before any surface inherits. */
const rootDeclared: Record<ThemeName, Record<string, string>> = {
  light: { ...declarations(css, ":root"), ...declarations(kumoCss, ':root,\n[data-theme="owlhead"]') },
  dark: { ...declarations(css, ":root"), ...declarations(css, DARK), ...declarations(kumoCss, ':root,\n[data-theme="owlhead"]'), ...declarations(kumoCss, DARK) },
};

function scopesFor(theme: ThemeName): Record<KumoScope, Record<string, string>> {
  const root = resolveScope(rootDeclared[theme], {});
  return {
    root,
    account: resolveScope(declarations(kumoCss, '[data-surface="account"]'), root),
    field: resolveScope(declarations(kumoCss, '[data-surface="field"]'), root),
    ink: resolveScope(declarations(kumoCss, '[data-surface="ink"]'), root),
  };
}
const SCOPES: Record<ThemeName, Record<KumoScope, Record<string, string>>> = { light: scopesFor("light"), dark: scopesFor("dark") };

const hueDistance = (a: number, b: number) => Math.min(Math.abs(a - b), 360 - Math.abs(a - b));
const rampOf = (ref: string) => ref.slice(0, ref.lastIndexOf("-")) as RampId;
const stepOf = (ref: string) => Number(ref.slice(ref.lastIndexOf("-") + 1)) as Step;
const CHROMATIC: RampId[] = ["gold", "green", "red", "amber", "cvd-blue", "cvd-rose", "cvd-orange", "crimson"];

/** The only tokens that may carry gold: the mandate's and the account's gold roles, and the text selection. */
const GOLD_TOKENS: TokenName[] = ["mandate", "mandate-soft", "mandate-strong", "mandate-marker", "mandate-edge", "lapis-soft", "lapis-line", "selection"];
/** Gold that is saturated enough to shout: lines, marks and labels, never a surface. */
const SATURATED_GOLD: TokenName[] = ["mandate-strong", "mandate-marker", "mandate-edge", "lapis-line"];

describe("ramps", () => {
  it.each(Object.values(RAMPS).map((r) => [r.id, r] as const))("%s has thirteen in-gamut steps at one hue on the shared lightness curve", (_, ramp) => {
    expect(Object.keys(ramp.steps).map(Number)).toEqual([...STEPS]);
    for (const step of STEPS) {
      const c = parseOklch(ramp.steps[step]);
      expect(c.h).toBe(ramp.hue);
      expect(c.l).toBe(LIGHTNESS[step]);
      expect(inGamut(c), `${ramp.id}-${step}`).toBe(true);
    }
  });

  it("uses a lightness curve that falls at every step", () => {
    const ls = STEPS.map((s) => LIGHTNESS[s]);
    for (let i = 1; i < ls.length; i++) expect(ls[i]).toBeLessThan(ls[i - 1]);
  });

  it.each(CHROMATIC.map((id) => [id, RAMPS[id]] as const))("%s peaks in chroma mid-ramp and falls toward both ends", (_, ramp) => {
    const cs = STEPS.map((s) => parseOklch(ramp.steps[s]).c);
    const peak = cs.indexOf(Math.max(...cs));
    expect(peak).toBeGreaterThanOrEqual(STEPS.indexOf(300));
    expect(peak).toBeLessThanOrEqual(STEPS.indexOf(700));
    expect(cs[0]).toBeLessThan(cs[peak]);
    expect(cs[cs.length - 1]).toBeLessThan(cs[peak]);
  });

  it("tints paper warm (hue 85) and ink cool (hue 255), both between C 0.003 and 0.01", () => {
    expect(RAMPS.paper.hue).toBe(85);
    expect(RAMPS.ink.hue).toBe(255);
    for (const id of ["paper", "ink"] as const) {
      for (const s of STEPS) {
        const { c } = parseOklch(RAMPS[id].steps[s]);
        expect(c, `${id}-${s}`).toBeGreaterThanOrEqual(0.003);
        expect(c, `${id}-${s}`).toBeLessThanOrEqual(0.01);
      }
    }
  });

  it("keeps gold a warm gold, not a yellow and not an orange", () => {
    expect(RAMPS.gold.hue).toBe(GOLD_HUE);
    expect(GOLD_HUE).toBeGreaterThanOrEqual(75);
    expect(GOLD_HUE).toBeLessThanOrEqual(90);
  });

  it("matches gain and loss in lightness and chroma, differing only in hue", () => {
    for (const s of STEPS) {
      const g = parseOklch(RAMPS.green.steps[s]);
      const r = parseOklch(RAMPS.red.steps[s]);
      expect(r.l).toBe(g.l);
      expect(r.c).toBe(g.c);
    }
    expect(hueDistance(RAMPS.red.hue, RAMPS.crimson.hue)).toBeGreaterThanOrEqual(15);
  });

  it("keeps the colour-blind alternates near Okabe-Ito blue, reddish purple and orange", () => {
    expect(RAMPS["cvd-blue"].hue).toBeGreaterThanOrEqual(235);
    expect(RAMPS["cvd-blue"].hue).toBeLessThanOrEqual(250);
    expect(RAMPS["cvd-rose"].hue).toBeGreaterThanOrEqual(340);
    expect(RAMPS["cvd-rose"].hue).toBeLessThanOrEqual(355);
    expect(RAMPS["cvd-orange"].hue).toBeGreaterThanOrEqual(45);
    expect(RAMPS["cvd-orange"].hue).toBeLessThanOrEqual(60);
  });
});

describe.each(THEMES)("Ink and Gold, %s", (theme) => {
  const palette = PALETTES[theme];
  const t = palette.tokens;
  const refs = palette.refs;

  it("defines every semantic token as an opaque OKLCH ramp step", () => {
    expect(palette.theme).toBe(theme);
    for (const n of TOKEN_NAMES) {
      expect(() => parseOklch(t[n].value), n).not.toThrow();
      expect(refs[n], n).toMatch(/^[a-z-]+-\d+$/);
    }
  });

  it("draws the surfaces, the type, the primary action, the account fill and the Stop control from paper and ink", () => {
    for (const n of ["background", "card", "muted", "border", "foreground", "muted-foreground", "primary", "primary-foreground", "lapis", "lapis-foreground", "ink", "ink-foreground", "ink-line"] as const) {
      expect(["paper", "ink"], n).toContain(rampOf(refs[n]));
    }
    const surface = theme === "light" ? "paper" : "ink";
    for (const n of ["background", "card", "muted", "border"] as const) expect(rampOf(refs[n]), n).toBe(surface);
    expect(refs.primary).toBe(refs.lapis);
    expect(refs.primary).toBe(refs.ink);
    expect(refs.primary).toBe(refs.foreground);
  });

  it("gives gold only to the gold tokens", () => {
    const gold = TOKEN_NAMES.filter((n) => rampOf(refs[n]) === "gold");
    expect(gold.filter((n) => !GOLD_TOKENS.includes(n))).toEqual([]);
    for (const n of GOLD_TOKENS) expect(rampOf(refs[n]), n).toBe("gold");
  });

  it("carries no other yellow or gold: outside the gold tokens, nothing warm is saturated (warning is on no screen)", () => {
    for (const n of TOKEN_NAMES.filter((n) => !GOLD_TOKENS.includes(n) && n !== "warning" && n !== "warning-soft")) {
      const { c, h } = parseOklch(t[n].value);
      expect(h >= 60 && h <= 110 && c > 0.02, `${n} ${t[n].value}`).toBe(false);
    }
  });

  it("never paints saturated gold as a surface: every gold background is a quiet tint", () => {
    const backgrounds = new Set(PAIRS.map((p) => p.bg));
    expect(SATURATED_GOLD.filter((n) => backgrounds.has(n))).toEqual([]);
    for (const n of GOLD_TOKENS.filter((n) => !SATURATED_GOLD.includes(n))) {
      const { l, c } = parseOklch(t[n].value);
      if (theme === "light") expect(l, n).toBeGreaterThanOrEqual(0.9);
      else expect(l, n).toBeLessThanOrEqual(0.4);
      expect(c, n).toBeLessThanOrEqual(0.08);
    }
  });

  it("paints no Kumo surface, fill or tint in saturated gold, in any scope", () => {
    const saturated = new Set(SATURATED_GOLD.map((n) => t[n].value));
    const surfaces = /^color-kumo-(canvas|elevated|recessed|base|tint|overlay|control|fill|fill-hover|contrast|brand|brand-hover|badge-[a-z]+|banner-[a-z]+|[a-z]+-tint)$/;
    for (const [scope, values] of Object.entries(SCOPES[theme])) {
      for (const [role, value] of Object.entries(values)) if (surfaces.test(role)) expect(saturated.has(value), `${scope}: --${role}`).toBe(false);
    }
  });

  it("gives crimson to the kill switch and to no other token", () => {
    const crimson = TOKEN_NAMES.filter((n) => rampOf(refs[n]) === "crimson");
    expect(crimson).toEqual(["crimson", "crimson-edge"]);
    expect(refs.crimson).toBe("crimson-700");
  });

  it("has no purple or violet, and no pure black, white or grey", () => {
    for (const n of TOKEN_NAMES) {
      const { l, c, h } = parseOklch(t[n].value);
      if (c > 0.04) expect(h < 280 || h > 330, `${n} hue ${h}`).toBe(true);
      expect(c, n).toBeGreaterThan(0);
      expect(l, n).toBeGreaterThan(0.05);
      expect(l, n).toBeLessThan(1);
    }
  });

  it(`keeps the Stop control and the kill switch at ${STOP_CONTRAST}:1 or more`, () => {
    expect(contrastRatio(t["ink-foreground"].value, t.ink.value)).toBeGreaterThanOrEqual(STOP_CONTRAST);
    expect(contrastRatio(t.ink.value, t.card.value), "the quiet Stop control on the header").toBeGreaterThanOrEqual(STOP_CONTRAST);
    expect(contrastRatio(t.ink.value, t.background.value), "the quiet Stop control, hovered").toBeGreaterThanOrEqual(STOP_CONTRAST);
    expect(contrastRatio(t["crimson-foreground"].value, t.crimson.value)).toBeGreaterThanOrEqual(STOP_CONTRAST);
  });

  it("keeps the paper hatch unmistakable but quiet on a card", () => {
    const lines = composite(t[palette.hatch.ref].value, palette.hatch.alpha, t.card.value);
    const ratio = rgbContrast(lines, toRgb255(t.card.value));
    expect(ratio).toBeGreaterThanOrEqual(HATCH_MIN);
    expect(ratio).toBeLessThanOrEqual(HATCH_MAX);
  });

  it("writes the hatch colour into globals.css", () => {
    const block = declarations(css, theme === "light" ? ":root" : DARK);
    expect(block["hatch-ink"]).toBe(t[palette.hatch.ref].value.replace(")", ` / ${palette.hatch.alpha})`));
  });
});

describe("Ink and Gold, the light theme's gold", () => {
  const refs = PALETTE.refs;

  it("sets the mandate as a gold-100 field under gold-500 rules and markers, with gold-700 labels", () => {
    expect(refs.mandate).toBe("gold-100");
    expect(refs["mandate-edge"]).toBe("gold-500");
    expect(refs["mandate-marker"]).toBe("gold-500");
    expect(refs["mandate-strong"]).toBe("gold-700");
    expect(refs["lapis-line"]).toBe("gold-500");
  });

  it("never sets gold text lighter than gold-700", () => {
    const goldText = PAIRS.filter((p) => p.kind !== "mark" && rampOf(refs[p.fg]) === "gold");
    expect(goldText.length).toBeGreaterThan(0);
    for (const p of goldText) expect(stepOf(refs[p.fg]), `${p.fg} on ${p.bg}`).toBeGreaterThanOrEqual(700);
    for (const n of GOLD_TOKENS.filter((n) => n.endsWith("-strong"))) expect(stepOf(refs[n]), n).toBeGreaterThanOrEqual(700);
  });
});

describe("Ink and Gold, the dark theme", () => {
  const refs = PALETTE_DARK.refs;

  it("keeps gold's meanings a step lighter: gold-400 rules and markers, gold-300 labels, a gold-850 field", () => {
    expect(refs.mandate).toBe("gold-850");
    expect(refs["mandate-edge"]).toBe("gold-400");
    expect(refs["mandate-marker"]).toBe("gold-400");
    expect(refs["lapis-line"]).toBe("gold-400");
    expect(refs["mandate-strong"]).toBe("gold-300");
  });

  it("keeps the kill switch's crimson fill and lightens only its edge", () => {
    expect(refs.crimson).toBe(PALETTE.refs.crimson);
    expect(refs["crimson-foreground"]).toBe(PALETTE.refs["crimson-foreground"]);
    expect(stepOf(refs["crimson-edge"])).toBeLessThan(stepOf(refs.crimson));
  });
});

describe.each(THEMES)("contrast in %s, WCAG 2.2 and APCA", (theme) => {
  const palette = PALETTES[theme];
  const t = palette.tokens;

  it.each(checkPalette(palette).map((r) => [`${r.fg} on ${r.bg} (${r.kind})`, r] as const))("%s meets WCAG and APCA", (_, r) => {
    expect(r.passWcag, `WCAG ${r.ratio.toFixed(2)}`).toBe(true);
    const lc = apcaLc(t[r.fg].value, t[r.bg].value);
    expect(passesApca(lc, r.kind), `APCA Lc ${lc.toFixed(1)}`).toBe(true);
  });

  it.each(KUMO_PAIRS.map((p) => [`${p.scope}: ${p.fg} on ${p.bg} (${p.kind})`, p] as const))("Kumo %s meets WCAG and APCA", (_, p) => {
    const scope = SCOPES[theme][p.scope];
    const m = measure(scope[p.fg], scope[p.bg], p.kind);
    expect(m.passWcag, `WCAG ${m.ratio.toFixed(2)}`).toBe(true);
    const lc = apcaLc(scope[p.fg], scope[p.bg]);
    expect(passesApca(lc, p.kind), `APCA Lc ${lc.toFixed(1)}`).toBe(true);
  });
});

describe("apca-w3 stays in the tests", () => {
  const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf8"));
  const APCA_IMPORT = /["'](apca-w3|colorparsley)["']|["'][^"'\n]*test\/apca["']/;

  it("measures APCA with the apca-w3 reference: black on white is Lc 106, white on black about -108", () => {
    expect(apcaLc("oklch(0 0 0)", "oklch(1 0 0)")).toBeCloseTo(106.04, 1);
    expect(apcaLc("oklch(1 0 0)", "oklch(0 0 0)")).toBeCloseTo(-107.88, 1);
  });

  it("is a dev dependency, never a dependency", () => {
    expect(pkg.devDependencies["apca-w3"]).toBeDefined();
    expect(pkg.dependencies["apca-w3"]).toBeUndefined();
    expect(pkg.dependencies.colorparsley).toBeUndefined();
  });

  it("is imported by no app file, only by src/test/apca.ts and the tests", () => {
    const app = sources(SRC).filter((f) => !relative(SRC, f).startsWith("test/"));
    expect(app.length).toBeGreaterThan(50);
    expect(app.filter((f) => APCA_IMPORT.test(readFileSync(f, "utf8"))).map((f) => relative(SRC, f))).toEqual([]);
  });

  it("is named by the build check's markers, which match APCA's and colorparsley's own source", () => {
    const apca = readFileSync(resolve(process.cwd(), "node_modules/apca-w3/src/apca-w3.js"), "utf8");
    const parsley = readFileSync(resolve(process.cwd(), "node_modules/colorparsley/src/colorparsley.js"), "utf8");
    expect(MARKERS.apca.test(apca)).toBe(true);
    expect(MARKERS.colorparsley.test(parsley)).toBe(true);
  });
});

describe("Kumo in Ink and Gold", () => {
  const kumoRoles = [...new Set(Array.from(kumoTheme.matchAll(/--((?:text-)?color-kumo-[a-z0-9-]+):/g), (m) => m[1]))].filter((n) => !n.includes("neutral"));

  it("re-points every colour role Kumo defines, so none of Kumo's own colours shows", () => {
    expect(kumoRoles.length).toBeGreaterThan(40);
    expect(kumoRoles.filter((n) => SCOPES.light.root[n] === undefined)).toEqual([]);
  });

  it.each(THEMES.flatMap((theme) => (Object.keys(SCOPES[theme]) as KumoScope[]).map((scope) => [theme, scope] as const)))("resolves every Kumo role in %s, %s scope, to that theme's palette", (theme, scope) => {
    const t = PALETTES[theme].tokens;
    const values = new Set([...TOKEN_NAMES.map((n) => t[n].value), "transparent"]);
    for (const n of kumoRoles) expect(values.has(SCOPES[theme][scope][n]), `--${n}: ${SCOPES[theme][scope][n]}`).toBe(true);
  });

  it.each(THEMES)("maps brand to the account, danger to ink, warning to amber, info to the muted type, success to gain, in %s", (theme) => {
    const t = PALETTES[theme].tokens;
    const root = SCOPES[theme].root;
    expect(root["color-kumo-brand"]).toBe(t.lapis.value);
    expect(root["text-color-kumo-brand"]).toBe(t.lapis.value);
    expect(root["color-kumo-danger"]).toBe(t.ink.value);
    expect(root["text-color-kumo-danger"]).toBe(t.ink.value);
    expect(root["color-kumo-warning"]).toBe(t.warning.value);
    expect(root["text-color-kumo-warning"]).toBe(t.warning.value);
    expect(root["color-kumo-warning-tint"]).toBe(t["warning-soft"].value);
    expect(root["color-kumo-info"]).toBe(t.info.value);
    expect(root["color-kumo-success"]).toBe(t.gain.value);
    expect(root["color-kumo-badge-orange"]).toBe(t.mandate.value);
    expect(root["color-kumo-focus"]).toBe(t["mandate-strong"].value);
  });

  it.each(THEMES)("labels an inverted fill in the fill's own foreground in %s", (theme) => {
    const t = PALETTES[theme].tokens;
    const root = SCOPES[theme].root;
    expect(root["text-color-kumo-inverse"]).toBe(t[theme === "light" ? "card" : "ink-foreground"].value);
    expect(contrastRatio(root["text-color-kumo-inverse"], root["color-kumo-brand"])).toBeGreaterThanOrEqual(STOP_CONTRAST);
  });

  it.each(THEMES)("paints the account surface in the account fill, the field in the gold tint under a gold line, and ink in ink, in %s", (theme) => {
    const t = PALETTES[theme].tokens;
    expect(SCOPES[theme].account["color-kumo-base"]).toBe(t.lapis.value);
    expect(SCOPES[theme].field["color-kumo-base"]).toBe(t.mandate.value);
    expect(SCOPES[theme].field["color-kumo-line"]).toBe(t["mandate-edge"].value);
    expect(SCOPES[theme].ink["color-kumo-base"]).toBe(t.ink.value);
  });

  it("names its surfaces by meaning, not by an old colour", () => {
    expect(kumoCss).not.toMatch(/\[data-surface="(lapis|navy|brass)"\]/);
  });

  it("writes no raw colour into the Kumo theme", () => {
    const end = kumoCss.indexOf(" * Kumo's arbitrary radii");
    expect(end).toBeGreaterThan(0);
    const roles = kumoCss.slice(0, end);
    expect(roles).not.toMatch(/(oklch|rgb|hsl)a?\(|#[0-9a-f]{3,8}\b|color-mix\(/i);
  });
});

describe.each(THEMES)("colour-vision deficiency, %s", (theme) => {
  const results = checkCvd(PALETTES[theme]);

  it.each(results.filter((c) => c.requiredHere).map((c) => [c.what, c] as const))("%s stays distinct under deuteranopia and protanopia", (_, c) => {
    for (const [vision, d] of Object.entries(c.byVision)) expect(d, `${vision} ΔE ${d.toFixed(3)}`).toBeGreaterThanOrEqual(CVD_DISTINCT);
  });

  it("needs the colour-blind friendly remap: default gain and loss merge under deuteranopia", () => {
    const def = results.find((c) => c.a === "gain" && c.b === "loss")!;
    expect(def.byVision.deuteranopia).toBeLessThan(CVD_DISTINCT);
  });

  it("keeps every colour-blind alternate apart from gold and crimson where it is required", () => {
    const required = results.filter((c) => c.requiredHere && (c.a.endsWith("-cvd") || c.b.endsWith("-cvd")));
    const against = new Set(required.flatMap((c) => [c.a, c.b]));
    for (const n of ["crimson", "mandate-marker", "lapis-line", "mandate-strong"] as const) expect(against.has(n), n).toBe(true);
  });
});

describe("colour-blind friendly", () => {
  it("remaps gain and loss to the alternates when the preference is on, in both themes", () => {
    const body = declarations(css, 'html:root[data-cvd="on"]');
    expect(body).toEqual({ gain: "var(--gain-cvd)", loss: "var(--loss-cvd)", "gain-soft": "var(--gain-cvd-soft)", "loss-soft": "var(--loss-cvd-soft)" });
  });

  it("uses blue for a gain in both themes, raspberry for a loss in light and orange in dark", () => {
    expect(rampOf(PALETTE.refs["gain-cvd"])).toBe("cvd-blue");
    expect(rampOf(PALETTE_DARK.refs["gain-cvd"])).toBe("cvd-blue");
    expect(rampOf(PALETTE.refs["loss-cvd"])).toBe("cvd-rose");
    expect(rampOf(PALETTE_DARK.refs["loss-cvd"])).toBe("cvd-orange");
  });
});

describe("colour usage in components", () => {
  const files = sources(SRC).map((f) => ({ path: relative(SRC, f), text: readFileSync(f, "utf8") }));
  const specimens = /^(app\/design|app\/palette|components\/palette)\//;
  const all = [...files, { path: "app/globals.css", text: css }, { path: "app/kumo-theme.css", text: kumoCss }];

  it("keeps warning off every screen, including Kumo's warning and alert variants", () => {
    const warning = /\b(bg|text|border|ring|outline|fill|stroke|decoration)-(kumo-)?warning\b|\bkumo-(banner-)?warning\b|variant=["{]*["'](warning|alert)["']/;
    const measured = "lib/contrast-pairs.ts";
    const users = files.filter((f) => warning.test(f.text) && !specimens.test(f.path) && f.path !== measured).map((f) => f.path);
    expect(users).toEqual([]);
  });

  it("uses no raw colour values in components", () => {
    const raw = /(oklch|rgb|hsl)a?\(|#[0-9a-f]{6}\b/i;
    const offenders = files.filter((f) => f.path.startsWith("components/") && !specimens.test(f.path) && raw.test(f.text)).map((f) => f.path);
    expect(offenders).toEqual([]);
  });

  it("paints saturated gold only as thin fills: the envelope's rails, posts and ticks, a legend swatch and the page header's rule", () => {
    const fill = /\bbg-(mandate-strong|mandate-marker|mandate-edge|lapis-line)\b/;
    const allowed = ["components/domain/envelope.tsx", "components/charts/chart-parts.tsx", "components/kumo/page-header/page-header.tsx"];
    expect(files.filter((f) => fill.test(f.text) && !specimens.test(f.path) && !allowed.includes(f.path)).map((f) => f.path)).toEqual([]);
  });

  it("uses only one palette: no second token set and no palette switch", () => {
    expect(all.filter((f) => /data-palette|PALETTE_COOKIE|PALETTE_PARAM|\?palette=|KeyP\b/.test(f.text)).map((f) => f.path)).toEqual([]);
  });

  it("leaves no trace of the palettes and the system it replaced: no navy, brass, slate, marigold or Placard", () => {
    const old = /\b(navy|brass|slate|marigold|placard)\b/i;
    expect(all.filter((f) => old.test(f.text)).map((f) => `${f.path}: ${f.text.match(old)?.[0]}`)).toEqual([]);
  });
});
