/**
 * The colour tokens in Azure and Sun (DEC-217), one entry per CSS custom property in `globals.css`
 * (the test `tokens.test.ts` keeps the two in step, in both themes). Values come from `palette.ts`;
 * this file adds what each token means. The design page and the contrast checks read it.
 */
import { PAIRS } from "./contrast-pairs";
import { PALETTES, type RampRef, type ThemeName, TOKEN_NAMES, type TokenName } from "./palette";

export type Meaning = "surface" | "text" | "mandate" | "account" | "stopped" | "kill" | "result" | "status" | "series";

export interface ColorToken {
  name: TokenName;
  meaning: Meaning;
  role: string;
  value: string;
  ref: RampRef;
}

export const TOKEN_ROLES: Record<TokenName, { meaning: Meaning; role: string }> = {
  background: { meaning: "surface", role: "Wells, hover and pressed rows: paper in light, the deepest ink in dark" },
  card: { meaning: "surface", role: "The page and every reading surface: sheets, dialogs, charts. Off-white in light, ink in dark" },
  muted: { meaning: "surface", role: "Quiet fills: skeletons, the range pill track, chart grid" },
  border: { meaning: "surface", role: "Hairlines between rows" },
  foreground: { meaning: "text", role: "Text, the mark, and the ring on an exits-only mode" },
  "muted-foreground": { meaning: "text", role: "Secondary text, labels, ages, and a mandate level's dashed line on a chart" },
  primary: { meaning: "account", role: "The primary action and links: the deep azure in light, the bright azure in dark" },
  "primary-foreground": { meaning: "account", role: "Text on a primary action" },
  lapis: { meaning: "account", role: "The account as a fill: the approvals count, its marker on the equity ladder, the paper hatch. Ink in light, paper in dark" },
  "lapis-foreground": { meaning: "account", role: "Text on the account fill" },
  "lapis-muted": { meaning: "account", role: "Secondary text on the account fill and on ink; a hovered account pill" },
  "lapis-soft": { meaning: "account", role: "The account's field, pale sun in light and a raised charcoal in dark: the current tab and range pill, an approval card, an account notice" },
  "lapis-strong": { meaning: "account", role: "A pressed primary action, and a quiet field inside an account surface" },
  "lapis-line": { meaning: "account", role: "The account's line: its equity chart, the current tab's bar and pill ring, its legend swatch" },
  mandate: { meaning: "mandate", role: "Your mandate: the field the envelope, limits and rails sit on, pale azure in light and a raised charcoal in dark" },
  "mandate-foreground": { meaning: "mandate", role: "Text on the mandate field" },
  "mandate-muted": { meaning: "mandate", role: "Secondary text on the mandate field" },
  "mandate-strong": { meaning: "mandate", role: "Mandate headings, labels, the \"Your mandate\" tag, the limit post, a level's axis label: azure-800 in light, never lighter" },
  "mandate-marker": { meaning: "mandate", role: "Rail fill and level marks on the envelope and the equity ladder" },
  "mandate-edge": { meaning: "mandate", role: "Lines inside the mandate field: Kumo's line and hairline roles there" },
  "mandate-soft": { meaning: "mandate", role: "A mandate notice: a limit acted (drawdown, daily loss, floor, goal)" },
  selection: { meaning: "mandate", role: "Selected text" },
  highlight: { meaning: "mandate", role: "Sun: the one warm fill, for a call to action or a highlighted mark, always under ink type. The same sun-300 in both themes" },
  "highlight-foreground": { meaning: "mandate", role: "Ink type on the highlight, in both themes" },
  tide: { meaning: "surface", role: "The landing page's third colour beside ink and sun: one section of its long page and the flying owl's wings and beak, teal-800 in both themes. Never in the product (DEC-907)" },
  "tide-foreground": { meaning: "text", role: "Headings on tide" },
  "tide-muted": { meaning: "text", role: "Body text on tide" },
  ink: { meaning: "stopped", role: "A stopped or paused agent, and the Stop control: ink in light, paper in dark" },
  "ink-foreground": { meaning: "stopped", role: "Text on ink" },
  "ink-line": { meaning: "stopped", role: "Hairlines inside an ink surface" },
  crimson: { meaning: "kill", role: "The kill switch's fill, and nothing else" },
  "crimson-foreground": { meaning: "kill", role: "Text on crimson" },
  "crimson-edge": { meaning: "kill", role: "The kill switch's edge: crimson in light, a lighter crimson in dark so the switch clears 3:1 on the sheet" },
  gain: { meaning: "result", role: "A gain as text, always with a plus sign and the word; an up candle" },
  loss: { meaning: "result", role: "A loss as text, always with a minus sign and the word; a down candle" },
  warning: { meaning: "status", role: "Warning, amber: Kumo's warning role, on no screen" },
  info: { meaning: "status", role: "Info text: Kumo's info role, in the muted type" },
  "gain-soft": { meaning: "status", role: "Success tint" },
  "loss-soft": { meaning: "status", role: "Loss tint" },
  "warning-soft": { meaning: "status", role: "Warning tint" },
  "info-soft": { meaning: "status", role: "Info tint" },
  "gain-cvd": { meaning: "result", role: "A gain when colour-blind friendly is on: teal in both themes" },
  "loss-cvd": { meaning: "result", role: "A loss when colour-blind friendly is on: raspberry in light, orange in dark" },
  "gain-cvd-soft": { meaning: "status", role: "Success tint when colour-blind friendly is on" },
  "loss-cvd-soft": { meaning: "status", role: "Loss tint when colour-blind friendly is on" },
  "series-1": { meaning: "series", role: "The first asset in a chart of holdings: azure" },
  "series-2": { meaning: "series", role: "The second asset: sun" },
  "series-3": { meaning: "series", role: "The third asset: teal" },
  "series-4": { meaning: "series", role: "The fourth asset: sky" },
  "series-5": { meaning: "series", role: "Cash and everything else: a blue-grey" },
};

export function colorTokensFor(theme: ThemeName): ColorToken[] {
  return TOKEN_NAMES.map((name) => ({ name, ...TOKEN_ROLES[name], ...PALETTES[theme].tokens[name] }));
}

/** The light theme, the `:root` block of `globals.css`; `colorTokensFor("dark")` is its `data-mode="dark"` block. */
export const colorTokens: ColorToken[] = colorTokensFor("light");

/** Text/background pairs that carry reading text. Each must reach WCAG AA (4.5:1). */
export const textPairs = PAIRS.filter((p) => p.kind !== "mark");

/** Non-text marks that must reach 3:1 against their surface (WCAG 1.4.11). */
export const markPairs = PAIRS.filter((p) => p.kind === "mark");

export function tokenValue(name: string, theme: ThemeName = "light"): string {
  const token = colorTokensFor(theme).find((t) => t.name === name);
  if (!token) throw new Error(`unknown token ${name}`);
  return token.value;
}

export const typeScale = [
  { role: "display", className: "text-display proportional-nums lining-nums", sample: "$24,987.50", spec: "Public Sans 600, 2 to 4.75rem with its container / 0.95, -0.03em, proportional lining figures, cents at half size, muted and raised. The equity figure over a hero chart" },
  { role: "hero", className: "text-hero tabular", sample: "Buy 40 XYZ", spec: "Public Sans 600, 2.5 to 3.5rem / 1.05, -0.03em, tabular figures. One per screen: the approval's action" },
  { role: "h1", className: "text-h1", sample: "Approval request", spec: "Public Sans 600, 1.75rem / 1.2, -0.02em. The page title" },
  { role: "h2", className: "text-h2", sample: "Your mandate", spec: "Public Sans 600, 1.25rem / 1.3, -0.01em. A section" },
  { role: "h3", className: "text-h3", sample: "Working orders", spec: "Public Sans 600, 1rem / 1.4. A group inside a section" },
  { role: "figure", className: "text-figure tabular", sample: "$1,203.10", spec: "Public Sans 500, 1.375rem / 1.2, tabular figures. Key figures beside the hero" },
  { role: "body", className: "text-base", sample: "If you do nothing, this action is skipped.", spec: "Public Sans 400, 1rem / 1.5, sentence case" },
  { role: "small", className: "text-sm", sample: "Resting protection stays in place.", spec: "Public Sans 400, 0.875rem / 1.43" },
  { role: "caption", className: "text-caption text-muted-foreground", sample: "as of 14:02:11, 3 min ago", spec: "Public Sans 400, 0.8125rem / 1.4, muted" },
  { role: "label", className: "field-label", sample: "Daily loss limit", spec: "Public Sans 500, 0.8125rem / 1.35, muted, sentence case (no capitals-only labels)" },
  { role: "number", className: "font-mono tabular", sample: "0.015 BTC/USD @ $56,700.00", spec: "Public Sans with tabular figures and its own plain zero" },
];

export const motionTokens = [
  { name: "--ease-out", value: "cubic-bezier(0.23, 1, 0.32, 1)", use: "Entrances, press, reveals, number changes" },
  { name: "--ease-in-out", value: "cubic-bezier(0.77, 0, 0.175, 1)", use: "Things that move on screen: chevrons, the range pill" },
  { name: "--ease-drawer", value: "cubic-bezier(0.32, 0.72, 0, 1)", use: "The Stop sheet" },
  { name: "--ease-spring", value: "linear() spring, about 10% overshoot", use: "Dialogs settling in; never a deadline or a figure" },
  { name: "--duration-press / --duration-release", value: "140 ms / 80 ms", use: "Press to scale 0.97; the release is faster than the press" },
  { name: "--duration-hover", value: "160 ms", use: "Colour changes on hover and on a mode change" },
  { name: "--duration-reveal", value: "240 ms, 30 ms stagger", use: "A list settles in once: rise 6 px and fade" },
  { name: "--duration-number", value: "240 ms", use: "A figure that changes rolls to its new value; deadlines never move" },
  { name: "--duration-draw", value: "700 ms", use: "The equity line draws in from the left on first load" },
  { name: "--duration-sheet", value: "320 ms in, 200 ms out", use: "Stop sheet and the phone's More sheet" },
  { name: "--duration-dialog", value: "240 ms in, 150 ms out", use: "Step-up dialog" },
];

/** The two densities (DEC-204): calm for the screens an owner lives in, dense for audit and admin. */
export const spacingTokens = [
  { name: "--content-max", calm: "80rem", dense: "90rem", use: "Widest content column" },
  { name: "--container-measure", calm: "58ch", dense: "58ch", use: "Reading measure (max-w-measure): under 80 characters a line" },
  { name: "--page-x", calm: "1.25rem / 1.75rem / 2.5rem", dense: "the same", use: "Page padding at phone / tablet / desktop" },
  { name: "--page-top", calm: "1.5rem / 2.25rem", dense: "the same", use: "Space above the first line of a screen" },
  { name: "--section-gap", calm: "3rem / 3.5rem", dense: "2rem", use: "Between sections of a screen" },
  { name: "--block-gap", calm: "1rem", dense: "0.75rem", use: "Between a heading and its content" },
  { name: "--row-y", calm: "1rem", dense: "0.5rem", use: "Vertical padding of a list or table row" },
  { name: "--tab-bar", calm: "4rem", dense: "4rem", use: "The phone tab bar, plus the safe area" },
  { name: "--dock-h / --dock-gap", calm: "4rem / 1rem", dense: "the same", use: "The desktop dock and the space below it; content, scroll padding and toasts clear both (`--dock-clearance`)" },
  { name: "--status-row", calm: "2.125rem", dense: "2.125rem", use: "The status strip and the phone's feed banner under the header, one height so either can replace the other" },
];

export const radiusTokens = [
  { name: "--radius-xs / sm", value: "1 px / 2 px", use: "Rails, ticks, swatches and the header's rule; a count on a tab" },
  { name: "--radius-md / lg", value: "3 px / 4 px", use: "Chips and tags; buttons, the Stop control, the range pill and icon buttons" },
  { name: "--radius-xl / 2xl", value: "6 px / 8 px", use: "Menus, restriction notes, the Stop sheet's choices; panels, an approval card, a well" },
  { name: "--radius-3xl", value: "10 px", use: "Sheets and dialogs, the desktop dock" },
  { name: "full", value: "9999px", use: "Only a dot: a timeline event, the envelope's position" },
];
