/**
 * The colour tokens in navy and brass (DEC-202), one entry per CSS custom property in `globals.css`
 * (the test `tokens.test.ts` keeps the two in step). Values come from `palette.ts`; this file adds
 * what each token means. The design page and the contrast checks read it. Dark values live in
 * `INK_AND_GOLD_DARK` and the `data-mode="dark"` block of `globals.css`.
 */
import { PAIRS } from "./contrast-pairs";
import { PALETTE, type RampRef, TOKEN_NAMES, type TokenName } from "./palette";

export type Meaning = "surface" | "text" | "mandate" | "account" | "stopped" | "kill" | "result" | "status";

export interface ColorToken {
  name: TokenName;
  meaning: Meaning;
  role: string;
  value: string;
  ref: RampRef;
}

export const TOKEN_ROLES: Record<TokenName, { meaning: Meaning; role: string }> = {
  background: { meaning: "surface", role: "Wells, the sidebar, hover and pressed rows: slate, barely tinted toward navy" },
  card: { meaning: "surface", role: "The page and every reading surface: sheets, dialogs, charts" },
  muted: { meaning: "surface", role: "Quiet fills: skeletons, the range pill track, chart grid" },
  border: { meaning: "surface", role: "Hairlines between rows" },
  foreground: { meaning: "text", role: "Text, and the ring on an exits-only mode" },
  "muted-foreground": { meaning: "text", role: "Secondary text, labels, ages" },
  primary: { meaning: "account", role: "Navy as the primary action, links, and focus" },
  "primary-foreground": { meaning: "account", role: "Text on a primary action" },
  lapis: { meaning: "account", role: "The account, in navy: its chart line, its connection, the paper hatch, links" },
  "lapis-foreground": { meaning: "account", role: "Text on navy" },
  "lapis-muted": { meaning: "account", role: "Secondary text on navy and on ink" },
  "lapis-soft": { meaning: "account", role: "An account notice: reconciliation, unknown order, activity at the broker" },
  "lapis-strong": { meaning: "account", role: "A pressed primary action, and a quiet field inside a navy surface" },
  "lapis-line": { meaning: "account", role: "Hairlines inside a navy surface" },
  mandate: { meaning: "mandate", role: "Your mandate: the pale brass tint the envelope, limits and rails sit on" },
  "mandate-foreground": { meaning: "mandate", role: "Text on the mandate tint" },
  "mandate-muted": { meaning: "mandate", role: "Secondary text on the mandate tint" },
  "mandate-strong": { meaning: "mandate", role: "Mandate headings, labels, the \"Your mandate\" tag, the limit post, a level's axis label" },
  "mandate-marker": { meaning: "mandate", role: "Rail fill, level marks, and mandate price lines on charts" },
  "mandate-edge": { meaning: "mandate", role: "Lines inside the mandate field: Kumo's line and hairline roles there" },
  "mandate-soft": { meaning: "mandate", role: "A mandate notice: a limit acted (drawdown, daily loss, floor, goal)" },
  selection: { meaning: "mandate", role: "Selected text" },
  ink: { meaning: "stopped", role: "A stopped or paused agent, and the Stop control" },
  "ink-foreground": { meaning: "stopped", role: "Text on ink" },
  "ink-line": { meaning: "stopped", role: "Hairlines inside an ink surface" },
  crimson: { meaning: "kill", role: "The kill switch, and nothing else" },
  "crimson-foreground": { meaning: "kill", role: "Text on crimson" },
  gain: { meaning: "result", role: "A gain as text, always with a plus sign and the word; an up candle" },
  loss: { meaning: "result", role: "A loss as text, always with a minus sign and the word; a down candle" },
  warning: { meaning: "status", role: "Warning, amber: Kumo's warning role, on no screen yet" },
  info: { meaning: "status", role: "Info text, in the brand hue: Kumo's info role" },
  "gain-soft": { meaning: "status", role: "Success tint" },
  "loss-soft": { meaning: "status", role: "Loss tint" },
  "warning-soft": { meaning: "status", role: "Warning tint" },
  "info-soft": { meaning: "status", role: "Info tint" },
  "gain-cvd": { meaning: "result", role: "A gain when colour-blind friendly is on (blue)" },
  "loss-cvd": { meaning: "result", role: "A loss when colour-blind friendly is on (orange)" },
  "gain-cvd-soft": { meaning: "status", role: "Success tint when colour-blind friendly is on" },
  "loss-cvd-soft": { meaning: "status", role: "Loss tint when colour-blind friendly is on" },
};

export const colorTokens: ColorToken[] = TOKEN_NAMES.map((name) => ({ name, ...TOKEN_ROLES[name], ...PALETTE.tokens[name] }));

/** Text/background pairs that carry reading text. Each must reach WCAG AA (4.5:1). */
export const textPairs = PAIRS.filter((p) => p.kind !== "mark");

/** Non-text marks that must reach 3:1 against their surface (WCAG 1.4.11). */
export const markPairs = PAIRS.filter((p) => p.kind === "mark");

export function tokenValue(name: string): string {
  const token = colorTokens.find((t) => t.name === name);
  if (!token) throw new Error(`unknown token ${name}`);
  return token.value;
}

export const typeScale = [
  { role: "hero", className: "text-hero tabular", sample: "$24,987.50", spec: "Mona Sans 600, 2.5 to 3.5rem / 1.05, -0.035em, tabular figures. One per screen" },
  { role: "h1", className: "text-h1", sample: "Approval request", spec: "Mona Sans 600, 1.75rem / 1.2, -0.02em. The page title" },
  { role: "h2", className: "text-h2", sample: "Your mandate", spec: "Mona Sans 600, 1.25rem / 1.3, -0.01em. A section" },
  { role: "h3", className: "text-h3", sample: "Working orders", spec: "Mona Sans 600, 1rem / 1.4. A group inside a section" },
  { role: "figure", className: "text-figure tabular", sample: "$1,203.10", spec: "Mona Sans 500, 1.375rem / 1.2, tabular figures. Key figures beside the hero" },
  { role: "body", className: "text-base", sample: "If you do nothing, this action is skipped.", spec: "Mona Sans 400, 1rem / 1.5, sentence case" },
  { role: "small", className: "text-sm", sample: "Resting protection stays in place.", spec: "Mona Sans 400, 0.875rem / 1.43" },
  { role: "caption", className: "text-caption text-muted-foreground", sample: "as of 14:02:11, 3 min ago", spec: "Mona Sans 400, 0.8125rem / 1.4, muted" },
  { role: "label", className: "field-label", sample: "Daily loss limit", spec: "Mona Sans 500, 0.8125rem / 1.35, muted, sentence case (no capitals-only labels)" },
  { role: "number", className: "font-mono tabular", sample: "0.015 BTC/USD @ $56,700.00", spec: "Mona Sans with tabular figures and its own plain zero" },
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
  { name: "--duration-sheet", value: "320 ms in, 200 ms out", use: "Stop sheet" },
  { name: "--duration-dialog", value: "240 ms in, 150 ms out", use: "Step-up dialog" },
];

/** The two densities (DEC-204): calm for the screens an owner lives in, dense for audit and admin. */
export const spacingTokens = [
  { name: "--nav-width", calm: "14rem", dense: "14rem", use: "Desktop side navigation, handed to Kumo's Sidebar" },
  { name: "--content-max", calm: "68rem", dense: "90rem", use: "Widest content column" },
  { name: "--container-measure", calm: "58ch", dense: "58ch", use: "Reading measure (max-w-measure): under 80 characters a line" },
  { name: "--page-x", calm: "1.25rem / 1.75rem / 2.5rem", dense: "the same", use: "Page padding at phone / tablet / desktop" },
  { name: "--page-top", calm: "1.5rem / 2.25rem", dense: "the same", use: "Space above the first line of a screen" },
  { name: "--section-gap", calm: "3rem / 3.5rem", dense: "2rem", use: "Between sections of a screen" },
  { name: "--block-gap", calm: "1rem", dense: "0.75rem", use: "Between a heading and its content" },
  { name: "--row-y", calm: "1rem", dense: "0.5rem", use: "Vertical padding of a list or table row" },
  { name: "--tab-bar", calm: "4rem", dense: "4rem", use: "The phone tab bar, plus the safe area" },
];

export const radiusTokens = [
  { name: "--radius-xs / sm", value: "0.25rem / 0.375rem", use: "Placeholder and fixture chips, keyboard hints, chart ticks" },
  { name: "--radius-lg / xl", value: "0.75rem / 1rem", use: "Menus, restriction notes, the Stop sheet's choices, an unknown order" },
  { name: "--radius-2xl", value: "1.25rem", use: "Panels: the mandate field, an approval card, a well, a hovered agent row" },
  { name: "--radius-3xl", value: "1.5rem", use: "Sheets and dialogs" },
  { name: "full", value: "9999px", use: "Buttons, chips, the Stop control, the paper badge, the range pill" },
];
