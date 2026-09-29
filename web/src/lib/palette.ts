/**
 * Owlhead's palette, Ink and Volt (DEC-214, superseding DEC-205's hue; web/COLOR.md): OKLCH ramps on one lightness curve, and
 * the semantic tokens of both themes mapped onto their steps. `globals.css` writes the same values,
 * and `tokens.test.ts` fails when the two drift. Components use only the semantic tokens.
 */
import { formatOklch, maxChroma } from "./color";

/** Thirteen steps: 850 and 975 give dark mode its borders and its page, below the usual eleven. */
export const STEPS = [50, 100, 200, 300, 400, 500, 600, 700, 800, 850, 900, 950, 975] as const;
export type Step = (typeof STEPS)[number];

/**
 * One perceptual lightness curve for every ramp. Light mode reads from the top (paper surfaces at
 * 50 to 200, ink text at 800 and 950); dark mode reads from the bottom (ink surfaces at 850 to 975,
 * paper text at 100 and 300). 300 and 400 are where dark-mode text and marks clear APCA.
 */
export const LIGHTNESS: Record<Step, number> = {
  50: 0.992,
  100: 0.975,
  200: 0.91,
  300: 0.88,
  400: 0.76,
  500: 0.62,
  600: 0.52,
  700: 0.44,
  800: 0.38,
  850: 0.31,
  900: 0.24,
  950: 0.2,
  975: 0.16,
};

/** Chroma rises to a hump in the middle of the ramp and falls off toward white and black. */
const HUMP: Record<Step, number> = { 50: 0.1, 100: 0.25, 200: 0.45, 300: 0.7, 400: 0.9, 500: 1, 600: 1, 700: 0.92, 800: 0.82, 850: 0.62, 900: 0.42, 950: 0.3, 975: 0.2 };

/** Paper and ink share the cool hue 255, both between C 0.003 and 0.01: paper is a cool white, ink a cool near-black. */
const PAPER_CHROMA: Record<Step, number> = { 50: 0.003, 100: 0.004, 200: 0.006, 300: 0.006, 400: 0.006, 500: 0.007, 600: 0.007, 700: 0.008, 800: 0.008, 850: 0.008, 900: 0.008, 950: 0.008, 975: 0.008 };
const INK_CHROMA: Record<Step, number> = { 50: 0.003, 100: 0.004, 200: 0.006, 300: 0.007, 400: 0.008, 500: 0.009, 600: 0.01, 700: 0.01, 800: 0.01, 850: 0.01, 900: 0.01, 950: 0.008, 975: 0.008 };

/**
 * Volt is a yellow-green that sRGB can only make vivid when it is light: it peaks at 300, the neon
 * of the highlight and of dark mode's marks, and darkens toward olive below 500, where it can carry
 * a line or a label on paper. 800 to 975 fade fast, so the dark selection and the mandate's field
 * in dark mode are calm dark tints, not green blocks.
 */
const VOLT_CHROMA: Record<Step, number> = { 50: 0.012, 100: 0.05, 200: 0.19, 300: 0.2, 400: 0.17, 500: 0.14, 600: 0.118, 700: 0.1, 800: 0.075, 850: 0.06, 900: 0.045, 950: 0.035, 975: 0.025 };

export type RampId = "paper" | "ink" | "volt" | "green" | "red" | "amber" | "cvd-teal" | "cvd-rose" | "cvd-orange" | "crimson";

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

export const NEUTRAL_HUE = 255;
export const VOLT_HUE = 120;
export const STATUS_HUES = { green: 150, red: 12 } as const;
export const CVD_HUES = { "cvd-teal": 205, "cvd-rose": 350, "cvd-orange": 50 } as const;

/**
 * Gain and loss share lightness and chroma at every step and differ only in hue, so a loss is
 * never louder than a gain or the other way round: chroma is the most the weaker hue can show there.
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
  paper: ramp("paper", "Paper", NEUTRAL_HUE, "Light neutrals: the page, cards and hairlines in light mode, and the type in dark mode", (s) => PAPER_CHROMA[s]),
  ink: ramp("ink", "Ink", NEUTRAL_HUE, "Dark neutrals: type, primary actions, the Stop control and the mark in light mode, and the surfaces in dark mode", (s) => INK_CHROMA[s]),
  volt: ramp("volt", "Volt", VOLT_HUE, "The one accent: your mandate's field, rules, markers and labels, the account's line, and the highlight", (s) => VOLT_CHROMA[s]),
  green: ramp("green", "Green", STATUS_HUES.green, "Gain", statusChroma),
  red: ramp("red", "Red", STATUS_HUES.red, "Loss, as text and candles only", statusChroma),
  amber: ramp("amber", "Amber", 70, "Kumo's warning role; on no screen", (s) => HUMP[s] * 0.15),
  "cvd-teal": ramp("cvd-teal", "Colour-blind teal", CVD_HUES["cvd-teal"], "Gain when colour-blind friendly is on, in both themes", (s) => HUMP[s] * 0.16),
  "cvd-rose": ramp("cvd-rose", "Colour-blind raspberry", CVD_HUES["cvd-rose"], "Loss when colour-blind friendly is on, light mode", (s) => HUMP[s] * 0.16),
  "cvd-orange": ramp("cvd-orange", "Colour-blind orange", CVD_HUES["cvd-orange"], "Loss when colour-blind friendly is on, dark mode", (s) => HUMP[s] * 0.16),
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
  "highlight",
  "highlight-foreground",
  "ink",
  "ink-foreground",
  "ink-line",
  "crimson",
  "crimson-foreground",
  "crimson-edge",
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

export type ThemeName = "light" | "dark";

export interface Palette {
  theme: ThemeName;
  name: string;
  summary: string;
  refs: Record<TokenName, RampRef>;
  tokens: Record<TokenName, TokenValue>;
  /** The paper hatch: this colour at this opacity, masked into diagonal lines. */
  hatch: { ref: TokenName; alpha: number };
}

export function rampValue(ref: RampRef): string {
  const at = ref.lastIndexOf("-");
  const id = ref.slice(0, at) as RampId;
  const step = Number(ref.slice(at + 1)) as Step;
  return RAMPS[id].steps[step];
}

/**
 * Light: paper surfaces and ink type. Ink is the primary action, the account's fill, the Stop
 * control and a stopped agent. Volt is the mandate (a pale volt field under deep volt rules
 * and markers at 500, olive volt-700 labels) and the account's chart line. On paper, neon volt is
 * too light for a line or a label, so it appears only as the highlight: a volt-300 fill that always
 * carries ink type. `lapis` keeps its name as the account's role.
 */
export const TOKEN_REFS: Record<TokenName, RampRef> = {
  background: "paper-100",
  card: "paper-50",
  muted: "paper-200",
  border: "paper-200",
  foreground: "ink-950",
  "muted-foreground": "ink-800",
  primary: "ink-950",
  "primary-foreground": "paper-50",
  lapis: "ink-950",
  "lapis-foreground": "paper-50",
  "lapis-muted": "paper-200",
  "lapis-soft": "volt-100",
  "lapis-strong": "ink-800",
  "lapis-line": "volt-500",
  mandate: "volt-100",
  "mandate-foreground": "ink-950",
  "mandate-muted": "ink-800",
  "mandate-strong": "volt-700",
  "mandate-marker": "volt-500",
  "mandate-edge": "volt-500",
  "mandate-soft": "volt-100",
  selection: "volt-200",
  highlight: "volt-300",
  "highlight-foreground": "ink-950",
  ink: "ink-950",
  "ink-foreground": "paper-50",
  "ink-line": "ink-700",
  crimson: "crimson-700",
  "crimson-foreground": "paper-50",
  "crimson-edge": "crimson-700",
  gain: "green-700",
  loss: "red-700",
  warning: "amber-700",
  info: "ink-800",
  "gain-soft": "green-100",
  "loss-soft": "red-100",
  "warning-soft": "amber-100",
  "info-soft": "paper-100",
  "gain-cvd": "cvd-teal-700",
  "loss-cvd": "cvd-rose-850",
  "gain-cvd-soft": "cvd-teal-100",
  "loss-cvd-soft": "cvd-rose-100",
};

/**
 * Dark: ink surfaces and paper type. Primary actions, the Stop control and stopped states turn to a
 * paper fill with ink type. Volt turns bright: volt-400 for the account's line, rules and markers
 * (a step below neon, so a colour-blind candle never merges with them), volt-200 for labels, and the
 * same volt-300 highlight under ink type. Crimson keeps its fill; its edge lightens so the kill switch still clears 3:1 on
 * the dark sheet while its label keeps 7:1 on the fill.
 */
export const TOKEN_REFS_DARK: Record<TokenName, RampRef> = {
  background: "ink-975",
  card: "ink-950",
  muted: "ink-900",
  border: "ink-850",
  foreground: "paper-100",
  "muted-foreground": "paper-300",
  primary: "paper-100",
  "primary-foreground": "ink-950",
  lapis: "paper-100",
  "lapis-foreground": "ink-950",
  "lapis-muted": "ink-850",
  "lapis-soft": "volt-850",
  "lapis-strong": "paper-300",
  "lapis-line": "volt-400",
  mandate: "volt-850",
  "mandate-foreground": "paper-100",
  "mandate-muted": "paper-300",
  "mandate-strong": "volt-200",
  "mandate-marker": "volt-400",
  "mandate-edge": "volt-400",
  "mandate-soft": "volt-900",
  selection: "volt-800",
  highlight: "volt-300",
  "highlight-foreground": "ink-950",
  ink: "paper-100",
  "ink-foreground": "ink-950",
  "ink-line": "paper-500",
  crimson: "crimson-700",
  "crimson-foreground": "paper-50",
  "crimson-edge": "crimson-400",
  gain: "green-300",
  loss: "red-300",
  warning: "amber-300",
  info: "paper-300",
  "gain-soft": "green-900",
  "loss-soft": "red-900",
  "warning-soft": "amber-900",
  "info-soft": "ink-900",
  "gain-cvd": "cvd-teal-300",
  "loss-cvd": "cvd-orange-300",
  "gain-cvd-soft": "cvd-teal-900",
  "loss-cvd-soft": "cvd-orange-900",
};

function tokensOf(refs: Record<TokenName, RampRef>): Record<TokenName, TokenValue> {
  return Object.fromEntries(TOKEN_NAMES.map((n) => [n, { ref: refs[n], value: rampValue(refs[n]) }])) as Record<TokenName, TokenValue>;
}

export const PALETTE: Palette = {
  theme: "light",
  name: "Ink and Volt",
  summary: "Cool paper surfaces, ink type and actions, volt for your mandate and the account's line: a pale volt field, deep volt rules and markers, olive labels, and a neon volt highlight under ink type.",
  refs: TOKEN_REFS,
  tokens: tokensOf(TOKEN_REFS),
  hatch: { ref: "lapis", alpha: 0.3 },
};

/** The paper hatch in the dark is the mid grey, so it still reads as paper on an ink card. */
export const PALETTE_DARK: Palette = {
  theme: "dark",
  name: "Ink and Volt, dark",
  summary: "Ink surfaces, paper type and paper primary actions; neon volt for your mandate's marks and the account's line.",
  refs: TOKEN_REFS_DARK,
  tokens: tokensOf(TOKEN_REFS_DARK),
  hatch: { ref: "ink-line", alpha: 0.4 },
};

export const PALETTES: Record<ThemeName, Palette> = { light: PALETTE, dark: PALETTE_DARK };

export function hatchInk(palette: Palette = PALETTE): string {
  return palette.tokens[palette.hatch.ref].value.replace(")", ` / ${palette.hatch.alpha})`);
}
