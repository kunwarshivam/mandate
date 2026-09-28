/**
 * Owlhead's palette, navy and brass (web/COLOR.md): OKLCH ramps, and the semantic tokens mapped
 * onto their steps. `globals.css` writes the same values, and `tokens.test.ts` fails when the two
 * drift. Components use only the semantic tokens.
 */
import { formatOklch, maxChroma } from "./color";

export const STEPS = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;
export type Step = (typeof STEPS)[number];

/** One perceptual lightness curve for every ramp: small steps at the light end, where tints live. */
export const LIGHTNESS: Record<Step, number> = {
  50: 0.985,
  100: 0.962,
  200: 0.925,
  300: 0.865,
  400: 0.76,
  500: 0.62,
  600: 0.52,
  700: 0.415,
  800: 0.365,
  900: 0.295,
  950: 0.225,
};

/** Chroma rises to a hump in the middle of the ramp and falls off toward white and black. */
const HUMP: Record<Step, number> = { 50: 0.12, 100: 0.25, 200: 0.45, 300: 0.7, 400: 0.9, 500: 1, 600: 1, 700: 0.92, 800: 0.82, 900: 0.7, 950: 0.55 };

/** Neutrals lean toward the brand hue, between C 0.006 and 0.015. */
const NEUTRAL_CHROMA: Record<Step, number> = { 50: 0.006, 100: 0.008, 200: 0.01, 300: 0.012, 400: 0.014, 500: 0.015, 600: 0.015, 700: 0.015, 800: 0.014, 900: 0.013, 950: 0.012 };

export type RampId = "slate" | "navy" | "brass" | "green" | "red" | "amber" | "blue" | "cvd-blue" | "cvd-orange" | "crimson";

export interface Ramp {
  id: RampId;
  name: string;
  hue: number;
  use: string;
  steps: Record<Step, string>;
}

const floor3 = (v: number) => Math.floor(v * 1000) / 1000;

function build(hue: number, chroma: (step: Step) => number): Record<Step, string> {
  const steps = {} as Record<Step, string>;
  for (const step of STEPS) {
    const l = LIGHTNESS[step];
    const c = floor3(Math.min(chroma(step), maxChroma(l, hue) * 0.97));
    steps[step] = formatOklch({ l, c, h: hue });
  }
  return steps;
}

export const STATUS_HUES = { green: 150, red: 12, amber: 70, blue: 258, "cvd-blue": 245, "cvd-orange": 55 } as const;

/**
 * The status colours share lightness and chroma at every step and differ only in hue, so none of
 * them is louder than another: chroma is the most the weakest hue (amber) can show there.
 */
function statusChroma(step: Step): number {
  const l = LIGHTNESS[step];
  const gamut = Math.min(...Object.values(STATUS_HUES).map((h) => maxChroma(l, h) * 0.97));
  return floor3(Math.min(HUMP[step] * 0.15, gamut));
}

function ramp(id: RampId, name: string, hue: number, use: string, chroma: (step: Step) => number): Ramp {
  return { id, name, hue, use, steps: build(hue, chroma) };
}

export const RAMPS: Record<RampId, Ramp> = {
  slate: ramp("slate", "Slate", 255, "Neutrals: page, fields, text, ink", (s) => NEUTRAL_CHROMA[s]),
  navy: ramp("navy", "Navy", 258, "Brand: the account and the platform", (s) => HUMP[s] * 0.125),
  brass: ramp("brass", "Brass", 80, "Your mandate: the tint, its rule and markers, its labels", (s) => HUMP[s] * 0.12),
  green: ramp("green", "Green", STATUS_HUES.green, "Gain and success", statusChroma),
  red: ramp("red", "Red", STATUS_HUES.red, "Loss, as text and markers only", statusChroma),
  amber: ramp("amber", "Amber", STATUS_HUES.amber, "Warning; on no screen yet", statusChroma),
  blue: ramp("blue", "Blue", STATUS_HUES.blue, "Info, in the brand hue", statusChroma),
  "cvd-blue": ramp("cvd-blue", "Colour-blind blue", STATUS_HUES["cvd-blue"], "Gain when colour-blind friendly is on (Okabe-Ito blue)", statusChroma),
  "cvd-orange": ramp("cvd-orange", "Colour-blind orange", STATUS_HUES["cvd-orange"], "Loss when colour-blind friendly is on (Okabe-Ito orange)", statusChroma),
  crimson: ramp("crimson", "Crimson", 27, "The kill switch, and nothing else", (s) => HUMP[s] * 0.2),
};

export const TOKEN_NAMES = [
  "background",
  "card",
  "muted",
  "border",
  "foreground",
  "muted-foreground",
  "primary",
  "primary-foreground",
  "lapis",
  "lapis-foreground",
  "lapis-muted",
  "lapis-soft",
  "lapis-strong",
  "lapis-line",
  "mandate",
  "mandate-foreground",
  "mandate-muted",
  "mandate-strong",
  "mandate-marker",
  "mandate-edge",
  "mandate-soft",
  "selection",
  "ink",
  "ink-foreground",
  "ink-line",
  "crimson",
  "crimson-foreground",
  "gain",
  "loss",
  "warning",
  "info",
  "gain-soft",
  "loss-soft",
  "warning-soft",
  "info-soft",
  "gain-cvd",
  "loss-cvd",
  "gain-cvd-soft",
  "loss-cvd-soft",
] as const;
export type TokenName = (typeof TOKEN_NAMES)[number];

export type RampRef = `${RampId}-${Step}`;

export interface TokenValue {
  value: string;
  ref: RampRef;
}

export interface Palette {
  name: string;
  summary: string;
  tokens: Record<TokenName, TokenValue>;
  /** The paper hatch: this colour at this opacity, masked into diagonal lines. */
  hatch: { ref: TokenName; alpha: number };
}

function refValue(ref: RampRef): TokenValue {
  const at = ref.lastIndexOf("-");
  const id = ref.slice(0, at) as RampId;
  const step = Number(ref.slice(at + 1)) as Step;
  return { value: RAMPS[id].steps[step], ref };
}

/**
 * Navy for the account, slate neutrals, the status family, crimson for the kill switch. The
 * mandate is a pale brass tint under a brass rule, with brass markers and dark brass labels, so it
 * stays present without shouting. `lapis` keeps its name: it is the account's colour, now navy-800.
 */
export const TOKEN_REFS: Record<TokenName, RampRef> = {
  background: "slate-100",
  card: "slate-50",
  muted: "slate-200",
  border: "slate-300",
  foreground: "slate-950",
  "muted-foreground": "slate-700",
  primary: "navy-800",
  "primary-foreground": "slate-50",
  lapis: "navy-800",
  "lapis-foreground": "slate-50",
  "lapis-muted": "navy-200",
  "lapis-soft": "navy-100",
  "lapis-strong": "navy-900",
  "lapis-line": "navy-600",
  mandate: "brass-100",
  "mandate-foreground": "slate-950",
  "mandate-muted": "slate-700",
  "mandate-strong": "brass-700",
  "mandate-marker": "brass-500",
  "mandate-edge": "brass-500",
  "mandate-soft": "brass-50",
  selection: "brass-200",
  ink: "slate-950",
  "ink-foreground": "slate-50",
  "ink-line": "slate-700",
  crimson: "crimson-700",
  "crimson-foreground": "slate-50",
  gain: "green-700",
  loss: "red-700",
  warning: "amber-700",
  info: "blue-700",
  "gain-soft": "green-100",
  "loss-soft": "red-100",
  "warning-soft": "amber-100",
  "info-soft": "blue-100",
  "gain-cvd": "cvd-blue-700",
  "loss-cvd": "cvd-orange-700",
  "gain-cvd-soft": "cvd-blue-100",
  "loss-cvd-soft": "cvd-orange-100",
};

/** Ink and gold: the values `globals.css` writes, which take precedence over the ramp steps above. */
export const INK_AND_GOLD: Record<TokenName, string> = {
  background: "oklch(0.975 0.004 85)",
  card: "oklch(0.992 0.003 85)",
  muted: "oklch(0.91 0.006 85)",
  border: "oklch(0.91 0.006 85)",
  foreground: "oklch(0.19 0.008 255)",
  "muted-foreground": "oklch(0.43 0.01 255)",
  primary: "oklch(0.19 0.008 255)",
  "primary-foreground": "oklch(0.992 0.003 85)",
  lapis: "oklch(0.19 0.008 255)",
  "lapis-foreground": "oklch(0.992 0.003 85)",
  "lapis-muted": "oklch(0.91 0.006 85)",
  "lapis-soft": "oklch(0.965 0.035 90)",
  "lapis-strong": "oklch(0.43 0.01 255)",
  "lapis-line": "oklch(0.66 0.13 80)",
  mandate: "oklch(0.965 0.035 90)",
  "mandate-foreground": "oklch(0.19 0.008 255)",
  "mandate-muted": "oklch(0.43 0.01 255)",
  "mandate-strong": "oklch(0.52 0.105 78)",
  "mandate-marker": "oklch(0.76 0.135 85)",
  "mandate-edge": "oklch(0.76 0.135 85)",
  "mandate-soft": "oklch(0.965 0.035 90)",
  selection: "oklch(0.965 0.035 90)",
  ink: "oklch(0.19 0.008 255)",
  "ink-foreground": "oklch(0.992 0.003 85)",
  "ink-line": "oklch(0.43 0.01 255)",
  crimson: "oklch(0.44 0.17 27)",
  "crimson-foreground": "oklch(0.992 0.003 85)",
  gain: "oklch(0.53 0.13 155)",
  loss: "oklch(0.56 0.18 25)",
  warning: "oklch(0.415 0.087 70)",
  info: "oklch(0.43 0.01 255)",
  "gain-soft": "oklch(0.962 0.017 155)",
  "loss-soft": "oklch(0.962 0.017 25)",
  "warning-soft": "oklch(0.962 0.017 70)",
  "info-soft": "oklch(0.975 0.004 85)",
  "gain-cvd": "oklch(0.415 0.087 245)",
  "loss-cvd": "oklch(0.415 0.087 55)",
  "gain-cvd-soft": "oklch(0.962 0.017 245)",
  "loss-cvd-soft": "oklch(0.962 0.017 55)",
};

/**
 * Ink and gold in the dark: the `html:root[data-mode="dark"]` block in `globals.css`. Ink surfaces
 * and off-white type; primary actions, the Stop control and stopped states become an off-white fill
 * with ink type; gold carries the account's line, selection and the mandate's labels.
 */
export const INK_AND_GOLD_DARK: Record<TokenName, string> = {
  background: "oklch(0.16 0.008 255)",
  card: "oklch(0.2 0.009 255)",
  muted: "oklch(0.24 0.01 255)",
  border: "oklch(0.31 0.01 255)",
  foreground: "oklch(0.96 0.004 85)",
  "muted-foreground": "oklch(0.78 0.006 85)",
  primary: "oklch(0.96 0.004 85)",
  "primary-foreground": "oklch(0.19 0.008 255)",
  lapis: "oklch(0.96 0.004 85)",
  "lapis-foreground": "oklch(0.19 0.008 255)",
  "lapis-muted": "oklch(0.43 0.01 255)",
  "lapis-soft": "oklch(0.28 0.04 85)",
  "lapis-strong": "oklch(0.86 0.006 85)",
  "lapis-line": "oklch(0.8 0.125 85)",
  mandate: "oklch(0.28 0.04 85)",
  "mandate-foreground": "oklch(0.96 0.004 85)",
  "mandate-muted": "oklch(0.78 0.006 85)",
  "mandate-strong": "oklch(0.8 0.125 85)",
  "mandate-marker": "oklch(0.8 0.125 85)",
  "mandate-edge": "oklch(0.8 0.125 85)",
  "mandate-soft": "oklch(0.23 0.02 85)",
  selection: "oklch(0.34 0.06 85)",
  ink: "oklch(0.96 0.004 85)",
  "ink-foreground": "oklch(0.19 0.008 255)",
  "ink-line": "oklch(0.64 0.008 85)",
  crimson: "oklch(0.55 0.19 27)",
  "crimson-foreground": "oklch(0.96 0.004 85)",
  gain: "oklch(0.74 0.15 155)",
  loss: "oklch(0.7 0.17 25)",
  warning: "oklch(0.78 0.12 70)",
  info: "oklch(0.78 0.006 85)",
  "gain-soft": "oklch(0.26 0.04 155)",
  "loss-soft": "oklch(0.26 0.04 25)",
  "warning-soft": "oklch(0.26 0.04 70)",
  "info-soft": "oklch(0.24 0.01 255)",
  "gain-cvd": "oklch(0.74 0.12 245)",
  "loss-cvd": "oklch(0.74 0.13 55)",
  "gain-cvd-soft": "oklch(0.26 0.04 245)",
  "loss-cvd-soft": "oklch(0.26 0.04 55)",
};

function tokensOf(values: Record<TokenName, string>): Record<TokenName, TokenValue> {
  return Object.fromEntries(TOKEN_NAMES.map((n) => [n, { ...refValue(TOKEN_REFS[n]), value: values[n] }])) as Record<TokenName, TokenValue>;
}

export const PALETTE: Palette = {
  name: "Ink and gold",
  summary: "Ink for text and actions, gold for your mandate and the account's line: a pale gold panel, gold markers, dark gold labels.",
  hatch: { ref: "lapis", alpha: 0.3 },
  tokens: tokensOf(INK_AND_GOLD),
};

/** The paper hatch in the dark is the mid grey over the gold tint, so it still reads as paper. */
export const PALETTE_DARK: Palette = {
  name: "Ink and gold, dark",
  summary: "Ink surfaces and off-white type; off-white primary actions; gold for your mandate, the account's line and selection.",
  hatch: { ref: "ink-line", alpha: 0.4 },
  tokens: tokensOf(INK_AND_GOLD_DARK),
};

export const PALETTES = { light: PALETTE, dark: PALETTE_DARK } as const;

export function hatchInk(palette: Palette = PALETTE): string {
  return palette.tokens[palette.hatch.ref].value.replace(")", ` / ${palette.hatch.alpha})`);
}
