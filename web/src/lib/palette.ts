/**
 * Owlhead's palette, Azure and Sun (DEC-217, superseding DEC-214's volt; web/COLOR.md): OKLCH ramps on one lightness curve, and
 * the semantic tokens of both themes mapped onto their steps. `globals.css` writes the same values,
 * and `tokens.test.ts` fails when the two drift. Components use only the semantic tokens.
 *
 * The colour theory: azure (258) is the brand, and gain green (150) and loss red (36) sit roughly a
 * third of the wheel either side of it, a near-triad, so the three read as a set and never as each
 * other. Sun (90), near azure's complement, is the one warm pop: the highlight, always under ink type.
 * Asset series add teal and sky from azure's cool side, so a chart of holdings never borrows the
 * gain or loss hue.
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
  400: 0.78,
  500: 0.62,
  600: 0.48,
  700: 0.44,
  800: 0.38,
  850: 0.29,
  900: 0.215,
  950: 0.175,
  975: 0.135,
};

/** Chroma rises to a hump in the middle of the ramp and falls off toward white and black. */
const HUMP: Record<Step, number> = { 50: 0.1, 100: 0.25, 200: 0.45, 300: 0.7, 400: 0.9, 500: 1, 600: 1, 700: 0.92, 800: 0.82, 850: 0.62, 900: 0.42, 950: 0.3, 975: 0.2 };

/**
 * Paper and ink share the cool hue 255. Paper is a cool white; ink falls to a near-neutral black in
 * dark mode's surfaces (850 to 975, C 0.008 and less), so the dark theme reads black rather than blue.
 */
const PAPER_CHROMA: Record<Step, number> = { 50: 0.003, 100: 0.004, 200: 0.006, 300: 0.006, 400: 0.006, 500: 0.007, 600: 0.007, 700: 0.008, 800: 0.008, 850: 0.008, 900: 0.008, 950: 0.008, 975: 0.008 };
const INK_CHROMA: Record<Step, number> = { 50: 0.003, 100: 0.004, 200: 0.006, 300: 0.007, 400: 0.008, 500: 0.009, 600: 0.01, 700: 0.01, 800: 0.009, 850: 0.008, 900: 0.007, 950: 0.006, 975: 0.005 };

/**
 * Azure is vivid through the middle of the ramp, where it carries lines, links and labels, and
 * fades at both ends, so the mandate's field is a pale sky in light mode and a calm deep blue in dark.
 */
const AZURE_CHROMA: Record<Step, number> = { 50: 0.012, 100: 0.03, 200: 0.075, 300: 0.11, 400: 0.15, 500: 0.19, 600: 0.2, 700: 0.18, 800: 0.14, 850: 0.055, 900: 0.04, 950: 0.03, 975: 0.02 };

/** Sun is a sunflower that sRGB only makes vivid when it is light: it peaks at 300, the highlight. */
const SUN_CHROMA: Record<Step, number> = { 50: 0.015, 100: 0.045, 200: 0.12, 300: 0.17, 400: 0.17, 500: 0.15, 600: 0.13, 700: 0.11, 800: 0.09, 850: 0.06, 900: 0.045, 950: 0.03, 975: 0.02 };

export type RampId = "paper" | "ink" | "azure" | "sun" | "teal" | "sky" | "green" | "red" | "amber" | "cvd-teal" | "cvd-rose" | "cvd-orange" | "crimson";

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
export const AZURE_HUE = 258;
export const SUN_HUE = 90;
export const SERIES_HUES = { teal: 192, sky: 232 } as const;
export const STATUS_HUES = { green: 150, red: 36 } as const;
export const CRIMSON_HUE = 20;
export const CVD_HUES = { "cvd-teal": 205, "cvd-rose": 350, "cvd-orange": 50 } as const;

/**
 * Gain and loss share lightness and chroma at every step and differ only in hue, so a loss is
 * never louder than a gain or the other way round: chroma is the most the weaker hue can show there.
 */
function statusChroma(step: Step): number {
  const l = LIGHTNESS[step];
  const gamut = Math.min(...Object.values(STATUS_HUES).map((h) => maxChroma(l, h) * 0.97));
  return floor3(Math.min(HUMP[step] * 0.21, gamut));
}

function ramp(id: RampId, name: string, hue: number, use: string, chroma: (step: Step) => number): Ramp {
  return { id, name, hue, use, steps: build(hue, chroma) };
}

export const RAMPS: Record<RampId, Ramp> = {
  paper: ramp("paper", "Paper", NEUTRAL_HUE, "Light neutrals: the page, cards and hairlines in light mode, and the type in dark mode", (s) => PAPER_CHROMA[s]),
  ink: ramp("ink", "Ink", NEUTRAL_HUE, "Dark neutrals: type, primary actions, the Stop control and the mark in light mode, and the surfaces in dark mode", (s) => INK_CHROMA[s]),
  azure: ramp("azure", "Azure", AZURE_HUE, "The brand: primary actions, links, the focus ring, your mandate's field, rules, markers and labels, and the first asset series", (s) => AZURE_CHROMA[s]),
  sun: ramp("sun", "Sun", SUN_HUE, "Azure's complement: the highlight, always under ink type, and an asset series", (s) => SUN_CHROMA[s]),
  teal: ramp("teal", "Teal", SERIES_HUES.teal, "An asset series, and tide, the landing page's third colour", (s) => HUMP[s] * 0.13),
  sky: ramp("sky", "Sky", SERIES_HUES.sky, "An asset series", (s) => HUMP[s] * 0.13),
  green: ramp("green", "Green", STATUS_HUES.green, "Gain, and the hero line on a day that is up", statusChroma),
  red: ramp("red", "Red", STATUS_HUES.red, "Loss, and the hero line on a day that is down", statusChroma),
  amber: ramp("amber", "Amber", 70, "Kumo's warning role; on no screen", (s) => HUMP[s] * 0.15),
  "cvd-teal": ramp("cvd-teal", "Colour-blind teal", CVD_HUES["cvd-teal"], "Gain when colour-blind friendly is on, in both themes", (s) => HUMP[s] * 0.16),
  "cvd-rose": ramp("cvd-rose", "Colour-blind raspberry", CVD_HUES["cvd-rose"], "Loss when colour-blind friendly is on, light mode", (s) => HUMP[s] * 0.16),
  "cvd-orange": ramp("cvd-orange", "Colour-blind orange", CVD_HUES["cvd-orange"], "Loss when colour-blind friendly is on, dark mode", (s) => HUMP[s] * 0.16),
  crimson: ramp("crimson", "Crimson", CRIMSON_HUE, "The kill switch, and nothing else", (s) => HUMP[s] * 0.2),
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
  "tide",
  "tide-foreground",
  "tide-muted",
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
  "series-1",
  "series-2",
  "series-3",
  "series-4",
  "series-5",
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
 * Light: paper surfaces and ink type. Azure-800 is the primary action and links (the deep azure,
 * as dark's primary is the bright one, so no primary text sits in the saturated azure-600 the
 * mandate's rules and markers use); ink is the
 * account's fill, the Stop control and a stopped agent. Azure is the mandate (a pale azure-200 field
 * under azure-600 rules and markers, azure-800 labels). Sun is too light for a line or a label on
 * paper, so it appears as the highlight, a sun-300 fill that always carries ink type, and as an
 * asset series. `lapis` keeps its name as the account's role.
 */
export const TOKEN_REFS: Record<TokenName, RampRef> = {
  background: "paper-100",
  card: "paper-50",
  muted: "paper-200",
  border: "paper-200",
  foreground: "ink-950",
  "muted-foreground": "ink-800",
  primary: "azure-800",
  "primary-foreground": "paper-50",
  lapis: "ink-950",
  "lapis-foreground": "paper-50",
  "lapis-muted": "paper-200",
  "lapis-soft": "sun-100",
  "lapis-strong": "ink-800",
  "lapis-line": "azure-600",
  mandate: "azure-200",
  "mandate-foreground": "ink-950",
  "mandate-muted": "ink-800",
  "mandate-strong": "azure-800",
  "mandate-marker": "azure-600",
  "mandate-edge": "azure-600",
  "mandate-soft": "azure-100",
  selection: "azure-200",
  highlight: "sun-300",
  "highlight-foreground": "ink-950",
  tide: "teal-800",
  "tide-foreground": "paper-50",
  "tide-muted": "teal-200",
  ink: "ink-950",
  "ink-foreground": "paper-50",
  "ink-line": "ink-700",
  crimson: "crimson-700",
  "crimson-foreground": "paper-50",
  "crimson-edge": "crimson-700",
  gain: "green-600",
  loss: "red-600",
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
  "series-1": "azure-500",
  "series-2": "sun-400",
  "series-3": "teal-500",
  "series-4": "sky-400",
  "series-5": "ink-500",
};

/**
 * Dark: near-black surfaces and paper type. The primary action is a bright azure-300 fill under ink
 * type; the Stop control and stopped states turn to a paper fill with ink type. Azure turns bright:
 * azure-400 rules and markers, azure-300 labels, and the same sun-300 highlight under ink type. The
 * mandate's field and the account's are a raised ink-850 charcoal, so a risk panel or an approval
 * card sits on the near-black page without a blue cast; azure stays in their bars, rules and labels.
 * The first asset series runs brighter than the rules (azure-200), so a holdings bar never
 * paints a block in the mandate's own azure. Crimson keeps its fill; its edge lightens so the kill
 * switch still clears 3:1 on the dark sheet while its label keeps 7:1 on the fill.
 */
export const TOKEN_REFS_DARK: Record<TokenName, RampRef> = {
  background: "ink-975",
  card: "ink-950",
  muted: "ink-900",
  border: "ink-850",
  foreground: "paper-100",
  "muted-foreground": "paper-300",
  primary: "azure-300",
  "primary-foreground": "ink-950",
  lapis: "paper-100",
  "lapis-foreground": "ink-950",
  "lapis-muted": "ink-850",
  "lapis-soft": "ink-850",
  "lapis-strong": "azure-200",
  "lapis-line": "azure-400",
  mandate: "ink-850",
  "mandate-foreground": "paper-100",
  "mandate-muted": "paper-300",
  "mandate-strong": "azure-300",
  "mandate-marker": "azure-400",
  "mandate-edge": "azure-400",
  "mandate-soft": "ink-900",
  selection: "azure-800",
  highlight: "sun-300",
  "highlight-foreground": "ink-950",
  tide: "teal-800",
  "tide-foreground": "paper-50",
  "tide-muted": "teal-200",
  ink: "paper-100",
  "ink-foreground": "ink-950",
  "ink-line": "paper-500",
  crimson: "crimson-700",
  "crimson-foreground": "paper-50",
  "crimson-edge": "crimson-400",
  gain: "green-400",
  loss: "red-400",
  warning: "amber-300",
  info: "paper-300",
  "gain-soft": "green-950",
  "loss-soft": "red-950",
  "warning-soft": "amber-900",
  "info-soft": "ink-900",
  "gain-cvd": "cvd-teal-300",
  "loss-cvd": "cvd-orange-300",
  "gain-cvd-soft": "cvd-teal-900",
  "loss-cvd-soft": "cvd-orange-900",
  "series-1": "azure-200",
  "series-2": "sun-300",
  "series-3": "teal-400",
  "series-4": "sky-300",
  "series-5": "paper-500",
};

function tokensOf(refs: Record<TokenName, RampRef>): Record<TokenName, TokenValue> {
  return Object.fromEntries(TOKEN_NAMES.map((n) => [n, { ref: refs[n], value: rampValue(refs[n]) }])) as Record<TokenName, TokenValue>;
}

export const PALETTE: Palette = {
  theme: "light",
  name: "Azure and Sun",
  summary: "Cool paper surfaces and ink type; azure for actions, links and your mandate; sun for the highlight under ink type; green and red for gains and losses.",
  refs: TOKEN_REFS,
  tokens: tokensOf(TOKEN_REFS),
  hatch: { ref: "lapis", alpha: 0.3 },
};

/** The paper hatch in the dark is the mid grey, so it still reads as paper on an ink card. */
export const PALETTE_DARK: Palette = {
  theme: "dark",
  name: "Azure and Sun, dark",
  summary: "Midnight surfaces and paper type; bright azure for actions and your mandate's marks; sun for the highlight; green and red for gains and losses.",
  refs: TOKEN_REFS_DARK,
  tokens: tokensOf(TOKEN_REFS_DARK),
  hatch: { ref: "ink-line", alpha: 0.4 },
};

export const PALETTES: Record<ThemeName, Palette> = { light: PALETTE, dark: PALETTE_DARK };

export function hatchInk(palette: Palette = PALETTE): string {
  return palette.tokens[palette.hatch.ref].value.replace(")", ` / ${palette.hatch.alpha})`);
}
