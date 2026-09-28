import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { composite, contrastRatio, inGamut, parseOklch, rgbContrast, toRgb255 } from "./color";
import { apcaLc, passesApca } from "@/test/apca";
import { MARKERS } from "../../scripts/no-apca.mjs";
import { checkCvd, checkPalette, measure } from "./contrast";
import { CVD_DISTINCT, HATCH_MAX, HATCH_MIN, KUMO_PAIRS, type KumoScope } from "./contrast-pairs";
import { LIGHTNESS, PALETTE, RAMPS, STEPS, TOKEN_NAMES, TOKEN_REFS } from "./palette";

const SRC = resolve(process.cwd(), "src");
const css = readFileSync(join(SRC, "app/globals.css"), "utf8");
const kumoCss = readFileSync(join(SRC, "app/kumo-theme.css"), "utf8");
const kumoTheme = readFileSync(resolve(process.cwd(), "node_modules/@cloudflare/kumo/dist/styles/theme-kumo.css"), "utf8");
const t = PALETTE.tokens;

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

const root = resolveScope({ ...declarations(css, ":root"), ...declarations(kumoCss, ':root,\n[data-theme="owlhead"]') }, {});
const SCOPES: Record<KumoScope, Record<string, string>> = {
  root,
  navy: resolveScope(declarations(kumoCss, '[data-surface="navy"]'), root),
  field: resolveScope(declarations(kumoCss, '[data-surface="field"]'), root),
  ink: resolveScope(declarations(kumoCss, '[data-surface="ink"]'), root),
};

const hueDistance = (a: number, b: number) => Math.min(Math.abs(a - b), 360 - Math.abs(a - b));

describe("ramps", () => {
  it.each(Object.values(RAMPS).map((r) => [r.id, r] as const))("%s has eleven in-gamut steps at one hue on the shared lightness curve", (_, ramp) => {
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

  it.each(Object.values(RAMPS).map((r) => [r.id, r] as const))("%s peaks in chroma mid-ramp and falls toward both ends", (_, ramp) => {
    const cs = STEPS.map((s) => parseOklch(ramp.steps[s]).c);
    const peak = cs.indexOf(Math.max(...cs));
    expect(peak).toBeGreaterThanOrEqual(STEPS.indexOf(400));
    expect(peak).toBeLessThanOrEqual(STEPS.indexOf(700));
    expect(cs[0]).toBeLessThan(cs[peak]);
    expect(cs[cs.length - 1]).toBeLessThan(cs[peak]);
  });

  it("tints the neutrals to hue 255, toward the brand, at C 0.006 to 0.015", () => {
    expect(RAMPS.slate.hue).toBe(255);
    expect(hueDistance(RAMPS.slate.hue, RAMPS.navy.hue)).toBeLessThanOrEqual(5);
    for (const s of STEPS) {
      const { c } = parseOklch(RAMPS.slate.steps[s]);
      expect(c).toBeGreaterThanOrEqual(0.006);
      expect(c).toBeLessThanOrEqual(0.015);
    }
  });

  it("matches the status colours in lightness and chroma, differing only in hue", () => {
    const status = [RAMPS.green, RAMPS.red, RAMPS.amber, RAMPS.blue, RAMPS["cvd-blue"], RAMPS["cvd-orange"]];
    for (const s of STEPS) {
      const values = status.map((r) => parseOklch(r.steps[s]));
      for (const v of values) {
        expect(v.l).toBe(values[0].l);
        expect(v.c).toBe(values[0].c);
      }
    }
  });

  it("keeps the colour-blind alternates near Okabe-Ito blue and orange", () => {
    expect(RAMPS["cvd-blue"].hue).toBeGreaterThanOrEqual(235);
    expect(RAMPS["cvd-blue"].hue).toBeLessThanOrEqual(250);
    expect(RAMPS["cvd-orange"].hue).toBeGreaterThanOrEqual(45);
    expect(RAMPS["cvd-orange"].hue).toBeLessThanOrEqual(75);
  });
});

describe("navy and brass", () => {
  it("defines every semantic token as an opaque OKLCH ramp step", () => {
    for (const n of TOKEN_NAMES) {
      expect(() => parseOklch(t[n].value), n).not.toThrow();
      expect(t[n].ref, n).toMatch(/^[a-z-]+-\d+$/);
    }
  });

  it("puts the brand and the account on navy-800", () => {
    expect(TOKEN_REFS.primary).toBe("navy-800");
    expect(TOKEN_REFS.lapis).toBe("navy-800");
  });

  it("keeps the mandate restrained: a brass-100 tint, brass-500 rules and markers, brass-700 labels", () => {
    expect(TOKEN_REFS.mandate).toBe("brass-100");
    expect(TOKEN_REFS["mandate-edge"]).toBe("brass-500");
    expect(TOKEN_REFS["mandate-marker"]).toBe("brass-500");
    expect(TOKEN_REFS["mandate-strong"]).toBe("brass-700");
    const tint = parseOklch(t.mandate.value);
    expect(tint.l).toBeGreaterThanOrEqual(0.95);
    expect(tint.c).toBeLessThanOrEqual(0.035);
    expect(RAMPS.brass.hue).toBeGreaterThanOrEqual(75);
    expect(RAMPS.brass.hue).toBeLessThanOrEqual(80);
  });

  it("draws every neutral from the slate ramp", () => {
    for (const n of ["background", "card", "muted", "border", "foreground", "muted-foreground", "ink", "ink-line"] as const) expect(TOKEN_REFS[n], n).toMatch(/^slate-/);
  });

  it("uses the matched status family at L 0.415 and C 0.087: gain 150, loss 12, warning 70, info 258", () => {
    const want = { gain: 150, loss: 12, warning: 70, info: 258 } as const;
    for (const [name, hue] of Object.entries(want)) {
      expect(parseOklch(t[name as keyof typeof want].value), name).toEqual({ l: 0.415, c: 0.087, h: hue });
    }
    expect(hueDistance(parseOklch(t.loss.value).h, parseOklch(t.crimson.value).h)).toBeGreaterThanOrEqual(15);
  });

  it("gives crimson-700 to the kill switch and to no other token", () => {
    expect(TOKEN_REFS.crimson).toBe("crimson-700");
    const others = TOKEN_NAMES.filter((n) => n !== "crimson" && TOKEN_REFS[n].startsWith("crimson-"));
    expect(others).toEqual([]);
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

  it("has no bright yellow to sit beside the navy, and no saturated warm field", () => {
    for (const n of TOKEN_NAMES) {
      const { l, c, h } = parseOklch(t[n].value);
      expect(h >= 85 && h <= 115 && l > 0.75 && c > 0.08, n).toBe(false);
      expect(h >= 60 && h <= 115 && c > 0.13, `${n} is too saturated for a warm accent`).toBe(false);
    }
  });

  it("keeps the Stop control and the kill switch at 7:1 or more", () => {
    expect(contrastRatio(t["ink-foreground"].value, t.ink.value)).toBeGreaterThanOrEqual(7);
    expect(contrastRatio(t["crimson-foreground"].value, t.crimson.value)).toBeGreaterThanOrEqual(7);
  });

  it("keeps the paper hatch unmistakable but quiet on a card", () => {
    const lines = composite(t[PALETTE.hatch.ref].value, PALETTE.hatch.alpha, t.card.value);
    const ratio = rgbContrast(lines, toRgb255(t.card.value));
    expect(ratio).toBeGreaterThanOrEqual(HATCH_MIN);
    expect(ratio).toBeLessThanOrEqual(HATCH_MAX);
  });
});

describe("contrast, WCAG 2.2 and APCA", () => {
  it("measures APCA with the apca-w3 reference: black on white is Lc 106, white on black about -108", () => {
    expect(apcaLc("oklch(0 0 0)", "oklch(1 0 0)")).toBeCloseTo(106.04, 1);
    expect(apcaLc("oklch(1 0 0)", "oklch(0 0 0)")).toBeCloseTo(-107.88, 1);
  });

  it.each(checkPalette().map((r) => [`${r.fg} on ${r.bg} (${r.kind})`, r] as const))("%s meets WCAG and APCA", (_, r) => {
    expect(r.passWcag, `WCAG ${r.ratio.toFixed(2)}`).toBe(true);
    const lc = apcaLc(t[r.fg].value, t[r.bg].value);
    expect(passesApca(lc, r.kind), `APCA Lc ${lc.toFixed(1)}`).toBe(true);
  });

  it.each(KUMO_PAIRS.map((p) => [`${p.scope}: ${p.fg} on ${p.bg} (${p.kind})`, p] as const))("Kumo %s meets WCAG and APCA", (_, p) => {
    const scope = SCOPES[p.scope];
    const m = measure(scope[p.fg], scope[p.bg], p.kind);
    expect(m.passWcag, `WCAG ${m.ratio.toFixed(2)}`).toBe(true);
    const lc = apcaLc(scope[p.fg], scope[p.bg]);
    expect(passesApca(lc, p.kind), `APCA Lc ${lc.toFixed(1)}`).toBe(true);
  });
});

describe("apca-w3 stays in the tests", () => {
  const pkg = JSON.parse(readFileSync(resolve(process.cwd(), "package.json"), "utf8"));
  const APCA_IMPORT = /["'](apca-w3|colorparsley)["']|["'][^"'\n]*test\/apca["']/;

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

describe("Kumo in navy and brass", () => {
  const kumoRoles = [...new Set(Array.from(kumoTheme.matchAll(/--((?:text-)?color-kumo-[a-z0-9-]+):/g), (m) => m[1]))].filter((n) => !n.includes("neutral"));

  it("re-points every colour role Kumo defines, so none of Kumo's own colours shows", () => {
    expect(kumoRoles.length).toBeGreaterThan(40);
    expect(kumoRoles.filter((n) => root[n] === undefined)).toEqual([]);
  });

  it.each(Object.keys(SCOPES) as KumoScope[])("resolves every Kumo role in the %s scope to a palette value", (scope) => {
    const values = new Set([...TOKEN_NAMES.map((n) => t[n].value), "transparent"]);
    for (const n of kumoRoles) expect(values.has(SCOPES[scope][n]), `--${n}: ${SCOPES[scope][n]}`).toBe(true);
  });

  it("maps brand to navy-800, danger to ink, warning to amber, info to the status blue, success to gain", () => {
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
  });

  it("paints the navy surface navy, the field in the brass tint under a brass line, and ink in ink", () => {
    expect(SCOPES.navy["color-kumo-base"]).toBe(t.lapis.value);
    expect(SCOPES.field["color-kumo-base"]).toBe(t.mandate.value);
    expect(SCOPES.field["color-kumo-line"]).toBe(t["mandate-edge"].value);
    expect(SCOPES.ink["color-kumo-base"]).toBe(t.ink.value);
    expect(kumoCss).not.toContain('[data-surface="lapis"]');
  });

  it("writes no raw colour into the Kumo theme", () => {
    const end = kumoCss.indexOf(" * Kumo's arbitrary radii");
    expect(end).toBeGreaterThan(0);
    const roles = kumoCss.slice(0, end);
    expect(roles).not.toMatch(/(oklch|rgb|hsl)a?\(|#[0-9a-f]{3,8}\b|color-mix\(/i);
  });
});

describe("colour-vision deficiency", () => {
  it.each(checkCvd().filter((c) => c.required).map((c) => [c.what, c] as const))("%s stays distinct under deuteranopia and protanopia", (_, c) => {
    for (const [vision, d] of Object.entries(c.byVision)) expect(d, `${vision} ΔE ${d.toFixed(3)}`).toBeGreaterThanOrEqual(CVD_DISTINCT);
  });

  it("needs the colour-blind friendly remap: default gain and loss merge under deuteranopia", () => {
    const def = checkCvd().find((c) => c.a === "gain" && c.b === "loss")!;
    expect(def.byVision.deuteranopia).toBeLessThan(CVD_DISTINCT);
  });

  it("remaps gain and loss to the colour-blind alternates when the preference is on", () => {
    const body = declarations(css, 'html:root[data-cvd="on"]');
    expect(body).toEqual({ gain: "var(--gain-cvd)", loss: "var(--loss-cvd)", "gain-soft": "var(--gain-cvd-soft)", "loss-soft": "var(--loss-cvd-soft)" });
  });
});

describe("colour usage in components", () => {
  const files = sources(SRC).map((f) => ({ path: relative(SRC, f), text: readFileSync(f, "utf8") }));
  const specimens = /^(app\/design|app\/palette|components\/palette)\//;

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

  it("uses only one palette: no marigold, no second token set, no palette switch", () => {
    const all = [...files, { path: "app/globals.css", text: css }, { path: "app/kumo-theme.css", text: kumoCss }];
    expect(all.filter((f) => /marigold/i.test(f.text)).map((f) => f.path)).toEqual([]);
    expect(all.filter((f) => /data-palette|PALETTE_COOKIE|PALETTE_PARAM|\?palette=|KeyP\b/.test(f.text)).map((f) => f.path)).toEqual([]);
  });
});
