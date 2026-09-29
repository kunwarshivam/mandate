import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { composite, contrastRatio, inGamut, parseOklch, rgbContrast, toRgb255 } from "./color";
import { apcaLc, passesApca } from "@/test/apca";
import { MARKERS } from "../../scripts/no-apca.mjs";
import { checkCvd, checkPalette, measure } from "./contrast";
import { CVD_DISTINCT, CVD_VISIONS, HATCH_MAX, HATCH_MIN, KUMO_PAIRS, type KumoScope, PAIRS, STOP_CONTRAST } from "./contrast-pairs";
import { LIGHTNESS, NEUTRAL_HUE, PALETTE, PALETTE_DARK, PALETTES, RAMPS, type RampId, STEPS, type Step, TOKEN_NAMES, type ThemeName, type TokenName, VOLT_HUE } from "./palette";

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
const CHROMATIC: RampId[] = ["volt", "green", "red", "amber", "cvd-teal", "cvd-rose", "cvd-orange", "crimson"];

/** The only tokens that may carry volt: the mandate's and the account's accent roles, the text selection and the highlight. */
const VOLT_TOKENS: TokenName[] = ["mandate", "mandate-soft", "mandate-strong", "mandate-marker", "mandate-edge", "lapis-soft", "lapis-line", "selection", "highlight"];
/** Neon volt as a fill: the highlight in both themes and the selection in light, always under ink type. */
const VIVID_FILLS: TokenName[] = ["highlight", "selection"];
/** Volt that is saturated enough to shout: lines, marks and labels, never a surface. */
const SATURATED_VOLT: TokenName[] = ["mandate-strong", "mandate-marker", "mandate-edge", "lapis-line"];

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

  it("tints paper and ink cool (hue 255), both between C 0.003 and 0.01: a cool white page", () => {
    expect(RAMPS.paper.hue).toBe(NEUTRAL_HUE);
    expect(RAMPS.ink.hue).toBe(NEUTRAL_HUE);
    expect(NEUTRAL_HUE).toBe(255);
    for (const id of ["paper", "ink"] as const) {
      for (const s of STEPS) {
        const { c } = parseOklch(RAMPS[id].steps[s]);
        expect(c, `${id}-${s}`).toBeGreaterThanOrEqual(0.003);
        expect(c, `${id}-${s}`).toBeLessThanOrEqual(0.01);
      }
    }
  });

  it("keeps volt a volt: the yellow-green of #ccff00, and at least 30 degrees from the gain's green", () => {
    expect(RAMPS.volt.hue).toBe(VOLT_HUE);
    expect(VOLT_HUE).toBeGreaterThanOrEqual(115);
    expect(VOLT_HUE).toBeLessThanOrEqual(125);
    expect(hueDistance(VOLT_HUE, RAMPS.green.hue)).toBeGreaterThanOrEqual(30);
    expect(hueDistance(VOLT_HUE, RAMPS.amber.hue)).toBeGreaterThanOrEqual(30);
  });

  it("makes volt neon from 200 to 400, deep enough from 500 to 700 for a line or a label, and quiet at 100, 850 and 900, where it is a field", () => {
    const c = (s: Step) => parseOklch(RAMPS.volt.steps[s]).c;
    for (const s of [200, 300, 400] as const) expect(c(s), `volt-${s}`).toBeGreaterThanOrEqual(0.17);
    for (const s of [500, 600, 700] as const) expect(c(s), `volt-${s}`).toBeGreaterThanOrEqual(0.1);
    for (const s of [100, 850, 900] as const) expect(c(s), `volt-${s}`).toBeLessThanOrEqual(0.06);
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

  /**
   * The gain was Okabe-Ito blue (245) until the accent became volt (266): 21 degrees apart,
   * a blue gain and volt text merge under red-green deficiency. Teal still reads as up (blues
   * and greens do, for people with CVD) and stays clear of volt, orange and raspberry.
   */
  it("keeps the colour-blind alternates at teal (between Okabe-Ito sky blue and bluish green), reddish purple and orange", () => {
    expect(RAMPS["cvd-teal"].hue).toBeGreaterThanOrEqual(200);
    expect(RAMPS["cvd-teal"].hue).toBeLessThanOrEqual(215);
    expect(hueDistance(RAMPS["cvd-teal"].hue, VOLT_HUE)).toBeGreaterThanOrEqual(50);
    expect(RAMPS["cvd-rose"].hue).toBeGreaterThanOrEqual(340);
    expect(RAMPS["cvd-rose"].hue).toBeLessThanOrEqual(355);
    expect(RAMPS["cvd-orange"].hue).toBeGreaterThanOrEqual(45);
    expect(RAMPS["cvd-orange"].hue).toBeLessThanOrEqual(60);
  });
});

describe.each(THEMES)("Ink and Volt, %s", (theme) => {
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

  it("gives volt only to the volt tokens", () => {
    const accent = TOKEN_NAMES.filter((n) => rampOf(refs[n]) === "volt");
    expect(accent.filter((n) => !VOLT_TOKENS.includes(n))).toEqual([]);
    for (const n of VOLT_TOKENS) expect(rampOf(refs[n]), n).toBe("volt");
  });

  it("carries no second volt: outside the volt tokens, nothing within 30 degrees of its hue is saturated", () => {
    for (const n of TOKEN_NAMES.filter((n) => !VOLT_TOKENS.includes(n))) {
      const { c, h } = parseOklch(t[n].value);
      expect(hueDistance(h, VOLT_HUE) < 30 && c > 0.02, `${n} ${t[n].value}`).toBe(false);
    }
  });

  it("never paints saturated volt as a surface: every volt background is a quiet tint, but for the vivid fills", () => {
    const backgrounds = new Set(PAIRS.map((p) => p.bg));
    expect(SATURATED_VOLT.filter((n) => backgrounds.has(n))).toEqual([]);
    for (const n of VOLT_TOKENS.filter((n) => !SATURATED_VOLT.includes(n) && !VIVID_FILLS.includes(n))) {
      const { l, c } = parseOklch(t[n].value);
      if (theme === "light") expect(l, n).toBeGreaterThanOrEqual(0.9);
      else expect(l, n).toBeLessThanOrEqual(0.4);
      expect(c, n).toBeLessThanOrEqual(0.08);
    }
  });

  it("paints no Kumo surface, fill or tint in saturated volt, in any scope", () => {
    const saturated = new Set(SATURATED_VOLT.map((n) => t[n].value));
    const surfaces = /^color-kumo-(canvas|elevated|recessed|base|tint|overlay|control|fill|fill-hover|contrast|brand|brand-hover|badge-[a-z]+|banner-[a-z]+|[a-z]+-tint)$/;
    for (const [scope, values] of Object.entries(SCOPES[theme])) {
      for (const [role, value] of Object.entries(values)) if (surfaces.test(role)) expect(saturated.has(value), `${scope}: --${role}`).toBe(false);
    }
  });

  it("puts only ink type on neon volt: the highlight and the selection carry the ink foreground at 7:1 or more", () => {
    for (const n of VIVID_FILLS) {
      const onIt = PAIRS.filter((p) => p.bg === n);
      expect(onIt.length, n).toBeGreaterThan(0);
      for (const p of onIt) expect(rampOf(refs[p.fg]), `${p.fg} on ${n}`).toBe(theme === "light" || n === "highlight" ? "ink" : "paper");
    }
    expect(contrastRatio(t["highlight-foreground"].value, t.highlight.value)).toBeGreaterThanOrEqual(STOP_CONTRAST);
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

describe("Ink and Volt, the light theme's accent", () => {
  const refs = PALETTE.refs;

  it("sets the mandate as a volt-100 field under volt-500 rules and markers, with volt-700 labels", () => {
    expect(refs.mandate).toBe("volt-100");
    expect(refs["mandate-edge"]).toBe("volt-500");
    expect(refs["mandate-marker"]).toBe("volt-500");
    expect(refs["mandate-strong"]).toBe("volt-700");
    expect(refs["lapis-line"]).toBe("volt-500");
  });

  it("never sets volt text lighter than volt-700", () => {
    const accentText = PAIRS.filter((p) => p.kind !== "mark" && rampOf(refs[p.fg]) === "volt");
    expect(accentText.length).toBeGreaterThan(0);
    for (const p of accentText) expect(stepOf(refs[p.fg]), `${p.fg} on ${p.bg}`).toBeGreaterThanOrEqual(700);
    for (const n of VOLT_TOKENS.filter((n) => n.endsWith("-strong"))) expect(stepOf(refs[n]), n).toBeGreaterThanOrEqual(700);
  });
});

describe("Ink and Volt, the dark theme", () => {
  const refs = PALETTE_DARK.refs;

  it("turns volt bright: volt-400 rules, markers and the account's line, volt-200 labels, a volt-850 field, neon volt-300 fills", () => {
    expect(refs.mandate).toBe("volt-850");
    expect(refs["mandate-edge"]).toBe("volt-400");
    expect(refs["mandate-marker"]).toBe("volt-400");
    expect(refs["lapis-line"]).toBe("volt-400");
    expect(refs["mandate-strong"]).toBe("volt-200");
    expect(refs.highlight).toBe(PALETTE.refs.highlight);
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

describe("Kumo in Ink and Volt", () => {
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

  it.each(THEMES)("paints the account surface in the account fill, the field in the volt tint under a volt line, and ink in ink, in %s", (theme) => {
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

  it.each(results.filter((c) => c.requiredHere).map((c) => [c.what, c] as const))("%s stays distinct under its simulated visions", (_, c) => {
    expect(c.requiredVisions.length).toBeGreaterThanOrEqual(2);
    for (const vision of c.requiredVisions) expect(c.byVision[vision], `${vision} ΔE ${c.byVision[vision].toFixed(3)}`).toBeGreaterThanOrEqual(CVD_DISTINCT);
  });

  it("keeps the accent's marks, gain, loss and crimson apart under deuteranopia, protanopia and tritanopia", () => {
    const core = [
      ["gain-cvd", "loss-cvd"],
      ["gain-cvd", "crimson"],
      ["loss-cvd", "crimson"],
      ["gain-cvd", "mandate-marker"],
      ["loss-cvd", "mandate-marker"],
      ["gain-cvd", "lapis-line"],
      ["loss-cvd", "lapis-line"],
    ];
    for (const [a, b] of core) {
      const c = results.find((r) => r.a === a && r.b === b)!;
      expect(c.requiredHere, `${a} / ${b}`).toBe(true);
      expect(c.requiredVisions, `${a} / ${b}`).toEqual(CVD_VISIONS);
    }
    expect(CVD_VISIONS).toEqual(["deuteranopia", "protanopia", "tritanopia"]);
  });

  it("needs the colour-blind friendly remap: default gain and loss merge under deuteranopia", () => {
    const def = results.find((c) => c.a === "gain" && c.b === "loss")!;
    expect(def.byVision.deuteranopia).toBeLessThan(CVD_DISTINCT);
  });

  it("keeps every colour-blind alternate apart from volt and crimson where it is required", () => {
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

  it("uses teal for a gain in both themes, raspberry for a loss in light and orange in dark", () => {
    expect(rampOf(PALETTE.refs["gain-cvd"])).toBe("cvd-teal");
    expect(rampOf(PALETTE_DARK.refs["gain-cvd"])).toBe("cvd-teal");
    expect(rampOf(PALETTE.refs["loss-cvd"])).toBe("cvd-rose");
    expect(rampOf(PALETTE_DARK.refs["loss-cvd"])).toBe("cvd-orange");
  });
});

describe("colour usage in components", () => {
  const files = sources(SRC).map((f) => ({ path: relative(SRC, f), text: readFileSync(f, "utf8") }));
  const specimens = /^(app\/\(app\)\/design|app\/\(app\)\/palette|components\/palette)\//;
  const all = [...files, { path: "app/globals.css", text: css }, { path: "app/kumo-theme.css", text: kumoCss }];

  it("keeps warning off every screen, including Kumo's warning and alert variants", () => {
    const warning = /\b(bg|text|border|ring|outline|fill|stroke|decoration)-(kumo-)?warning\b|\bkumo-(banner-)?warning\b|variant=["{]*["'](warning|alert)["']/;
    const measured = "lib/contrast-pairs.ts";
    // DEC-213: the signed-out landing shows no gain or loss, so its construction tape and edited record line can't be read as one.
    const landing = /^components\/site\//;
    const users = files.filter((f) => warning.test(f.text) && !specimens.test(f.path) && !landing.test(f.path) && f.path !== measured).map((f) => f.path);
    expect(users).toEqual([]);
  });

  it("reserves warning because amber would read as a loss: it sits within 25 degrees of the colour-blind orange loss", () => {
    expect(hueDistance(RAMPS.amber.hue, RAMPS["cvd-orange"].hue)).toBeLessThanOrEqual(25);
  });

  it("uses no raw colour values in components", () => {
    const raw = /(oklch|rgb|hsl)a?\(|#[0-9a-f]{6}\b/i;
    const offenders = files.filter((f) => f.path.startsWith("components/") && !specimens.test(f.path) && raw.test(f.text)).map((f) => f.path);
    expect(offenders).toEqual([]);
  });

  it("paints saturated volt only as thin fills: the envelope's rails, posts and ticks, a legend swatch and the page header's rule", () => {
    const fill = /\bbg-(mandate-strong|mandate-marker|mandate-edge|lapis-line)\b/;
    const allowed = ["components/domain/envelope.tsx", "components/charts/chart-parts.tsx", "components/kumo/page-header/page-header.tsx"];
    expect(files.filter((f) => fill.test(f.text) && !specimens.test(f.path) && !allowed.includes(f.path)).map((f) => f.path)).toEqual([]);
  });

  it("uses only one palette: no second token set and no palette switch", () => {
    expect(all.filter((f) => /data-palette|PALETTE_COOKIE|PALETTE_PARAM|\?palette=|KeyP\b/.test(f.text)).map((f) => f.path)).toEqual([]);
  });

  it("leaves no trace of the palettes and the system it replaced: no gold, navy, brass, slate, marigold or Placard", () => {
    const old = /\b(gold|navy|brass|slate|marigold|placard)\b/i;
    expect(all.filter((f) => old.test(f.text)).map((f) => `${f.path}: ${f.text.match(old)?.[0]}`)).toEqual([]);
  });
});
