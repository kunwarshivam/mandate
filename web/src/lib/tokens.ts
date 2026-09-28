/**
 * The colour tokens, one entry per CSS custom property in `globals.css` (the test
 * `tokens.test.ts` keeps the two in step). The design page and the contrast checks read this file.
 */

export type Theme = "light" | "dark";

export interface ColorToken {
  name: string;
  role: string;
  light: string;
  dark: string;
}

export const colorTokens: ColorToken[] = [
  { name: "background", role: "Porcelain (light) and midnight (dark) page surface", light: "oklch(0.985 0.006 255)", dark: "oklch(0.17 0.035 268)" },
  { name: "card", role: "Raised surface: cards, sheets, dialogs", light: "oklch(0.998 0.003 255)", dark: "oklch(0.205 0.04 268)" },
  { name: "muted", role: "Sunken surface: rails, table stripes, skeletons", light: "oklch(0.955 0.012 262)", dark: "oklch(0.25 0.045 268)" },
  { name: "border", role: "Hairlines and dividers", light: "oklch(0.905 0.016 262)", dark: "oklch(0.32 0.045 268)" },
  { name: "foreground", role: "Body text", light: "oklch(0.235 0.035 268)", dark: "oklch(0.955 0.008 260)" },
  { name: "muted-foreground", role: "Secondary text, labels, ages", light: "oklch(0.47 0.035 266)", dark: "oklch(0.76 0.03 262)" },
  { name: "primary", role: "Ultramarine: primary actions, links, focus", light: "oklch(0.52 0.22 268)", dark: "oklch(0.74 0.14 268)" },
  { name: "primary-foreground", role: "Text on ultramarine", light: "oklch(0.985 0.006 255)", dark: "oklch(0.17 0.035 268)" },
  { name: "ultramarine", role: "Brand hue as a fill: rails, envelope, charts", light: "oklch(0.52 0.22 268)", dark: "oklch(0.62 0.2 268)" },
  { name: "persimmon", role: "Warm accent fill: limit walls, the paper hatch", light: "oklch(0.72 0.17 48)", dark: "oklch(0.74 0.16 50)" },
  { name: "persimmon-text", role: "Persimmon as text", light: "oklch(0.52 0.15 42)", dark: "oklch(0.8 0.13 52)" },
  { name: "lagoon", role: "Information and gains as a fill", light: "oklch(0.74 0.12 195)", dark: "oklch(0.74 0.12 195)" },
  { name: "lagoon-text", role: "Gains and information as text", light: "oklch(0.48 0.085 205)", dark: "oklch(0.82 0.1 195)" },
  { name: "orchid", role: "Platform-authored labels as a fill (sparingly)", light: "oklch(0.7 0.15 330)", dark: "oklch(0.7 0.15 330)" },
  { name: "orchid-text", role: "Platform-authored labels as text", light: "oklch(0.5 0.17 330)", dark: "oklch(0.82 0.11 330)" },
  { name: "rose", role: "Losses as a fill", light: "oklch(0.63 0.2 15)", dark: "oklch(0.66 0.18 15)" },
  { name: "rose-text", role: "Losses as text (always with a minus sign and the word)", light: "oklch(0.51 0.19 15)", dark: "oklch(0.8 0.12 15)" },
  { name: "crimson", role: "The kill switch, and nothing else", light: "oklch(0.47 0.19 27)", dark: "oklch(0.55 0.2 27)" },
  { name: "crimson-foreground", role: "Text on crimson", light: "oklch(0.985 0.006 255)", dark: "oklch(0.985 0.006 255)" },
  { name: "ink", role: "Stop control surface (brand ink #171717 in light, ivory in dark)", light: "oklch(0.205 0 0)", dark: "oklch(0.97 0.011 88)" },
  { name: "ink-foreground", role: "Text on the Stop control", light: "oklch(0.97 0.011 88)", dark: "oklch(0.205 0 0)" },
  { name: "notice", role: "Mode banner surface (paused, restricted)", light: "oklch(0.965 0.03 70)", dark: "oklch(0.26 0.05 55)" },
  { name: "notice-border", role: "Mode banner edge", light: "oklch(0.82 0.1 60)", dark: "oklch(0.5 0.1 55)" },
];

/** Text/background pairs that carry reading text. Each must reach WCAG AA (4.5:1). */
export const textPairs: Array<{ fg: string; bg: string; use: string }> = [
  { fg: "foreground", bg: "background", use: "Body text on the page" },
  { fg: "foreground", bg: "card", use: "Body text on cards" },
  { fg: "foreground", bg: "muted", use: "Body text on sunken surfaces" },
  { fg: "foreground", bg: "notice", use: "Mode banner text" },
  { fg: "muted-foreground", bg: "background", use: "Secondary text on the page" },
  { fg: "muted-foreground", bg: "card", use: "Secondary text on cards" },
  { fg: "muted-foreground", bg: "muted", use: "Secondary text on sunken surfaces" },
  { fg: "primary", bg: "background", use: "Links on the page" },
  { fg: "primary", bg: "card", use: "Links on cards" },
  { fg: "primary-foreground", bg: "primary", use: "Primary button label" },
  { fg: "persimmon-text", bg: "card", use: "Persimmon labels" },
  { fg: "lagoon-text", bg: "card", use: "Gains on cards" },
  { fg: "lagoon-text", bg: "background", use: "Gains on the page" },
  { fg: "orchid-text", bg: "card", use: "Platform-authored labels" },
  { fg: "rose-text", bg: "card", use: "Losses on cards" },
  { fg: "rose-text", bg: "background", use: "Losses on the page" },
  { fg: "crimson-foreground", bg: "crimson", use: "Kill switch label" },
  { fg: "ink-foreground", bg: "ink", use: "Stop control label" },
];

export function tokenValue(name: string, theme: Theme): string {
  const token = colorTokens.find((t) => t.name === name);
  if (!token) throw new Error(`unknown token ${name}`);
  return token[theme];
}

export const typeScale = [
  { role: "display", className: "text-display", sample: "$10,123.45", spec: "Bricolage Grotesque 650, 2.5rem / 1.05, −0.02em, tabular" },
  { role: "title", className: "text-title", sample: "Agent detail", spec: "Bricolage Grotesque 600, 1.75rem / 1.1, −0.015em" },
  { role: "heading", className: "text-heading", sample: "Limits in dollars", spec: "Bricolage Grotesque 600, 1.25rem / 1.2, −0.01em" },
  { role: "body", className: "text-base", sample: "If you do nothing, this action is skipped.", spec: "Hanken Grotesk 400, 1rem / 1.55" },
  { role: "small", className: "text-sm", sample: "Resting protection stays in place.", spec: "Hanken Grotesk 400, 0.875rem / 1.5" },
  { role: "caption", className: "text-caption", sample: "as of 14:02:11, 3 min ago", spec: "Hanken Grotesk 500, 0.8125rem / 1.4" },
  { role: "figure", className: "font-mono tabular-nums", sample: "0.015 BTC/USD @ $56,700.00", spec: "JetBrains Mono 450, tabular figures, slashed zero" },
];

export const motionTokens = [
  { name: "--ease-out-quart", value: "cubic-bezier(0.25, 1, 0.5, 1)", use: "Hover, press, reveals" },
  { name: "--ease-in-out", value: "cubic-bezier(0.65, 0, 0.35, 1)", use: "Two-way changes (expand, collapse)" },
  { name: "--spring-sheet", value: "400ms linear(…) from spring(stiffness 380, damping 36)", use: "Stop sheet" },
  { name: "--spring-dialog", value: "450ms linear(…) from spring(stiffness 520, damping 38)", use: "Dialogs (step-up)" },
  { name: "--duration-press", value: "150ms", use: "Press feedback (scale 0.98)" },
  { name: "--duration-hover", value: "180ms", use: "Hover colour and border" },
  { name: "--duration-reveal", value: "240ms", use: "List reveals, 40 ms stagger" },
  { name: "--duration-drift", value: "32s", use: "Ambient drift of the envelope gradient" },
];
