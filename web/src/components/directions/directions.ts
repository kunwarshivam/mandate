/**
 * Three flat visual directions for the web UI, compared behind the `/directions` picker (dev only).
 * Each one maps its palette onto the app's existing token names so the shared shell, the Stop sheet,
 * and the step-up dialog restyle with it; `buildDirectionCss` scopes every set to the page that
 * renders `[data-direction]`, so nothing leaks into the production routes.
 */

import type { Theme } from "@/lib/tokens";

export type DirectionId = "vernier" | "placard" | "keel";

export type Motion = "snap" | "roll" | "settle";

export interface Direction {
  id: DirectionId;
  name: string;
  axis: string;
  rationale: string;
  fonts: { display: string; sans: string; mono: string };
  radius: string;
  motion: Motion;
  /** A direction without `dark` keeps its light set under the dark theme. */
  tokens: { light: Record<string, string>; dark?: Record<string, string> };
}

export function directionThemes(d: Direction): Theme[] {
  return d.tokens.dark ? ["light", "dark"] : ["light"];
}

const vernier: Direction = {
  id: "vernier",
  name: "Vernier",
  axis: "Precision instrument: dense, tabular, structure from alignment and hairlines, one signal colour",
  rationale:
    "Control. A vernier scale reads finer than the main scale, and Mandate's promise is exact limits. Every figure sits in a column beside the limit it is measured against; the page is drafting-film green and navy ink, and the one signal colour is international orange: it marks where a limit stops the agent and what is waiting for you.",
  fonts: {
    display: '"Archivo Variable", ui-sans-serif, system-ui, sans-serif',
    sans: '"Archivo Variable", ui-sans-serif, system-ui, sans-serif',
    mono: '"Azeret Mono Variable", ui-monospace, monospace',
  },
  radius: "0.125rem",
  motion: "snap",
  tokens: {
    light: {
      background: "oklch(0.955 0.024 165)",
      card: "oklch(0.978 0.013 165)",
      muted: "oklch(0.925 0.03 166)",
      border: "oklch(0.8 0.035 168)",
      foreground: "oklch(0.24 0.05 255)",
      "muted-foreground": "oklch(0.43 0.045 245)",
      primary: "oklch(0.24 0.05 255)",
      "primary-foreground": "oklch(0.955 0.024 165)",
      ultramarine: "oklch(0.32 0.06 255)",
      persimmon: "oklch(0.68 0.19 48)",
      "persimmon-text": "oklch(0.52 0.16 44)",
      lagoon: "oklch(0.62 0.12 160)",
      "lagoon-text": "oklch(0.46 0.1 158)",
      orchid: "oklch(0.62 0.08 250)",
      "orchid-text": "oklch(0.46 0.08 250)",
      rose: "oklch(0.6 0.19 5)",
      "rose-text": "oklch(0.5 0.18 5)",
      crimson: "oklch(0.47 0.19 27)",
      "crimson-foreground": "oklch(0.985 0.004 170)",
      ink: "oklch(0.22 0.05 255)",
      "ink-foreground": "oklch(0.955 0.024 165)",
      notice: "oklch(0.955 0.04 75)",
      "notice-border": "oklch(0.8 0.11 62)",
      signal: "oklch(0.68 0.19 48)",
      "signal-text": "oklch(0.52 0.16 44)",
      "hatch-ink": "oklch(0.68 0.19 48 / 0.5)",
      ring: "oklch(0.62 0.18 46)",
    },
    dark: {
      background: "oklch(0.21 0.035 250)",
      card: "oklch(0.245 0.038 250)",
      muted: "oklch(0.28 0.04 250)",
      border: "oklch(0.39 0.04 248)",
      foreground: "oklch(0.94 0.018 165)",
      "muted-foreground": "oklch(0.76 0.03 190)",
      primary: "oklch(0.94 0.018 165)",
      "primary-foreground": "oklch(0.21 0.035 250)",
      ultramarine: "oklch(0.86 0.012 190)",
      persimmon: "oklch(0.74 0.17 52)",
      "persimmon-text": "oklch(0.8 0.14 58)",
      lagoon: "oklch(0.72 0.12 158)",
      "lagoon-text": "oklch(0.8 0.11 155)",
      orchid: "oklch(0.72 0.07 250)",
      "orchid-text": "oklch(0.8 0.06 250)",
      rose: "oklch(0.66 0.17 8)",
      "rose-text": "oklch(0.79 0.12 8)",
      crimson: "oklch(0.55 0.2 27)",
      "crimson-foreground": "oklch(0.985 0.004 170)",
      ink: "oklch(0.94 0.018 165)",
      "ink-foreground": "oklch(0.21 0.035 250)",
      notice: "oklch(0.28 0.045 60)",
      "notice-border": "oklch(0.55 0.1 58)",
      signal: "oklch(0.74 0.17 52)",
      "signal-text": "oklch(0.8 0.14 58)",
      "hatch-ink": "oklch(0.74 0.17 52 / 0.45)",
      ring: "oklch(0.74 0.17 52)",
    },
  },
};

const placard: Direction = {
  id: "placard",
  name: "Placard",
  axis: "Confident colour blocking: large flat fields own regions, big numerals, generous type",
  rationale:
    "Calm under stress. Transit signage is read by people in a hurry, at a glance, before a word registers. Marigold always means your mandate, lapis is the account, ink is a stopped agent: the owner knows where they are and what binds the agent before reading the numbers.",
  fonts: {
    display: '"Big Shoulders Display Variable", ui-sans-serif, system-ui, sans-serif',
    sans: '"Atkinson Hyperlegible Next Variable", ui-sans-serif, system-ui, sans-serif',
    mono: '"Atkinson Hyperlegible Next Variable", ui-sans-serif, system-ui, sans-serif',
  },
  radius: "0rem",
  motion: "roll",
  tokens: {
    light: {
      background: "oklch(0.975 0.005 250)",
      card: "oklch(0.995 0.002 250)",
      muted: "oklch(0.935 0.01 250)",
      border: "oklch(0.8 0.02 255)",
      foreground: "oklch(0.21 0.035 258)",
      "muted-foreground": "oklch(0.44 0.035 258)",
      primary: "oklch(0.36 0.1 258)",
      "primary-foreground": "oklch(0.975 0.005 250)",
      ultramarine: "oklch(0.21 0.035 258)",
      persimmon: "oklch(0.36 0.1 258)",
      "persimmon-text": "oklch(0.46 0.1 62)",
      lagoon: "oklch(0.6 0.12 155)",
      "lagoon-text": "oklch(0.44 0.11 155)",
      orchid: "oklch(0.5 0.1 258)",
      "orchid-text": "oklch(0.36 0.1 258)",
      rose: "oklch(0.6 0.19 12)",
      "rose-text": "oklch(0.49 0.18 10)",
      crimson: "oklch(0.47 0.19 27)",
      "crimson-foreground": "oklch(0.985 0.004 250)",
      ink: "oklch(0.21 0.035 258)",
      "ink-foreground": "oklch(0.975 0.005 250)",
      notice: "oklch(0.94 0.03 245)",
      "notice-border": "oklch(0.36 0.1 258)",
      field: "oklch(0.85 0.155 84)",
      "field-foreground": "oklch(0.21 0.035 258)",
      "field-muted": "oklch(0.36 0.05 70)",
      lapis: "oklch(0.36 0.1 258)",
      "lapis-foreground": "oklch(0.975 0.005 250)",
      "lapis-muted": "oklch(0.84 0.03 255)",
      exits: "oklch(0.5 0.12 250)",
      "exits-foreground": "oklch(0.985 0.004 250)",
      "hatch-ink": "oklch(0.21 0.035 258 / 0.3)",
      ring: "oklch(0.36 0.1 258)",
    },
  },
};

const keel: Direction = {
  id: "keel",
  name: "Keel",
  axis: "Soft and tactile: warm panels on a cool desk, one level of elevation, a two-colour accent system",
  rationale:
    "Trust. A keel keeps a boat upright in a gust; this direction is the calm hardware of a well-made instrument you keep on the desk. Warm white panels sit one step above a steel-blue desk like devices on it, rails are slider tracks with a knob, petrol is what the owner controls and apricot is what needs attention.",
  fonts: {
    display: '"Funnel Display Variable", ui-sans-serif, system-ui, sans-serif',
    sans: '"Funnel Sans Variable", ui-sans-serif, system-ui, sans-serif',
    mono: '"Funnel Sans Variable", ui-sans-serif, system-ui, sans-serif',
  },
  radius: "0.875rem",
  motion: "settle",
  tokens: {
    light: {
      background: "oklch(0.905 0.02 238)",
      card: "oklch(0.99 0.006 80)",
      muted: "oklch(0.945 0.01 80)",
      border: "oklch(0.88 0.012 80)",
      foreground: "oklch(0.25 0.025 240)",
      "muted-foreground": "oklch(0.44 0.025 240)",
      primary: "oklch(0.43 0.075 205)",
      "primary-foreground": "oklch(0.985 0.005 80)",
      ultramarine: "oklch(0.43 0.075 205)",
      persimmon: "oklch(0.76 0.13 62)",
      "persimmon-text": "oklch(0.49 0.1 62)",
      lagoon: "oklch(0.6 0.1 150)",
      "lagoon-text": "oklch(0.46 0.1 150)",
      orchid: "oklch(0.55 0.07 205)",
      "orchid-text": "oklch(0.43 0.075 205)",
      rose: "oklch(0.62 0.17 12)",
      "rose-text": "oklch(0.5 0.17 12)",
      crimson: "oklch(0.47 0.19 27)",
      "crimson-foreground": "oklch(0.985 0.005 80)",
      ink: "oklch(0.25 0.025 240)",
      "ink-foreground": "oklch(0.985 0.006 80)",
      notice: "oklch(0.935 0.045 70)",
      "notice-border": "oklch(0.78 0.1 62)",
      petrol: "oklch(0.43 0.075 205)",
      "petrol-foreground": "oklch(0.985 0.005 80)",
      "petrol-soft": "oklch(0.92 0.03 205)",
      apricot: "oklch(0.8 0.12 62)",
      "apricot-soft": "oklch(0.93 0.05 68)",
      "hatch-ink": "oklch(0.76 0.13 62 / 0.55)",
      ring: "oklch(0.43 0.075 205)",
    },
    dark: {
      background: "oklch(0.2 0.022 240)",
      card: "oklch(0.26 0.012 75)",
      muted: "oklch(0.3 0.013 75)",
      border: "oklch(0.37 0.014 75)",
      foreground: "oklch(0.93 0.012 80)",
      "muted-foreground": "oklch(0.76 0.018 80)",
      primary: "oklch(0.82 0.1 66)",
      "primary-foreground": "oklch(0.2 0.022 240)",
      ultramarine: "oklch(0.72 0.06 205)",
      persimmon: "oklch(0.8 0.11 65)",
      "persimmon-text": "oklch(0.83 0.1 68)",
      lagoon: "oklch(0.7 0.1 150)",
      "lagoon-text": "oklch(0.8 0.1 150)",
      orchid: "oklch(0.7 0.05 205)",
      "orchid-text": "oklch(0.83 0.1 68)",
      rose: "oklch(0.66 0.15 12)",
      "rose-text": "oklch(0.79 0.11 12)",
      crimson: "oklch(0.55 0.2 27)",
      "crimson-foreground": "oklch(0.985 0.005 80)",
      ink: "oklch(0.93 0.012 80)",
      "ink-foreground": "oklch(0.2 0.022 240)",
      notice: "oklch(0.3 0.035 65)",
      "notice-border": "oklch(0.55 0.08 62)",
      petrol: "oklch(0.5 0.075 205)",
      "petrol-foreground": "oklch(0.985 0.005 80)",
      "petrol-soft": "oklch(0.32 0.03 205)",
      apricot: "oklch(0.82 0.1 66)",
      "apricot-soft": "oklch(0.32 0.04 65)",
      "hatch-ink": "oklch(0.8 0.11 65 / 0.45)",
      ring: "oklch(0.82 0.1 66)",
    },
  },
};

export const DIRECTIONS: Direction[] = [vernier, placard, keel];

export function directionAt(index: number): Direction {
  return DIRECTIONS[Math.min(Math.max(index, 0), DIRECTIONS.length - 1)];
}

/** Text/background pairs each direction must hold at WCAG AA, where both tokens exist. */
export const directionTextPairs: Array<[fg: string, bg: string]> = [
  ["foreground", "background"],
  ["foreground", "card"],
  ["foreground", "muted"],
  ["foreground", "notice"],
  ["muted-foreground", "background"],
  ["muted-foreground", "card"],
  ["primary", "background"],
  ["primary", "card"],
  ["primary-foreground", "primary"],
  ["persimmon-text", "card"],
  ["persimmon-text", "background"],
  ["lagoon-text", "card"],
  ["lagoon-text", "background"],
  ["rose-text", "card"],
  ["rose-text", "background"],
  ["orchid-text", "card"],
  ["crimson-foreground", "crimson"],
  ["ink-foreground", "ink"],
  ["signal-text", "background"],
  ["field-foreground", "field"],
  ["field-muted", "field"],
  ["lapis-foreground", "lapis"],
  ["lapis-muted", "lapis"],
  ["field", "lapis"],
  ["exits-foreground", "exits"],
  ["petrol-foreground", "petrol"],
  ["foreground", "petrol-soft"],
  ["foreground", "apricot-soft"],
];

const MOTION: Record<Motion, Record<string, string>> = {
  snap: {
    "--duration-press": "100ms",
    "--duration-hover": "120ms",
    "--duration-reveal": "0ms",
    "--spring-sheet": "cubic-bezier(0.32, 0.72, 0, 1)",
    "--spring-sheet-duration": "220ms",
    "--spring-dialog": "cubic-bezier(0.23, 1, 0.32, 1)",
    "--spring-dialog-duration": "180ms",
  },
  roll: {
    "--duration-press": "140ms",
    "--duration-hover": "160ms",
    "--duration-reveal": "200ms",
    "--spring-sheet": "cubic-bezier(0.32, 0.72, 0, 1)",
    "--spring-sheet-duration": "280ms",
    "--spring-dialog": "cubic-bezier(0.23, 1, 0.32, 1)",
    "--spring-dialog-duration": "220ms",
  },
  settle: {
    "--duration-press": "160ms",
    "--duration-hover": "200ms",
    "--duration-reveal": "240ms",
    "--spring-sheet":
      "linear(0, 0.1423, 0.3889, 0.6083, 0.7675, 0.8706, 0.9324, 0.9671, 0.9854, 0.9943, 0.9984, 1, 1)",
    "--spring-sheet-duration": "300ms",
    "--spring-dialog":
      "linear(0, 0.1784, 0.4749, 0.7182, 0.8736, 0.9569, 0.9943, 1.007, 1.0087, 1.0066, 1.004, 1.0021, 1.0009, 1.0003, 1)",
    "--spring-dialog-duration": "280ms",
  },
};

function declarations(vars: Record<string, string>, prefix = "--"): string {
  return Object.entries(vars)
    .map(([k, v]) => `${k.startsWith("--") ? k : prefix + k}: ${v};`)
    .join(" ");
}

/**
 * The CSS for one direction, scoped with `:root:has([data-direction="…"])` so it applies to the
 * whole document (the shell and portalled sheets included) only while the directions page renders.
 */
export function buildDirectionCss(d: Direction): string {
  const scope = `:root:has([data-direction="${d.id}"])`;
  const base = {
    ...d.tokens.light,
    radius: d.radius,
    "font-display": d.fonts.display,
    "font-sans": d.fonts.sans,
    "font-mono": d.fonts.mono,
    ...MOTION[d.motion],
  };
  const dark = d.tokens.dark ? `${scope}.dark { ${declarations(d.tokens.dark)} color-scheme: dark; }` : `${scope}.dark { ${declarations(d.tokens.light)} color-scheme: light; }`;
  return `${scope} { ${declarations(base)} color-scheme: light; } ${dark}`;
}
