/**
 * The Placard colour tokens in navy and brass, one entry per CSS custom property in `globals.css`
 * (the test `tokens.test.ts` keeps the two in step). Values come from `palette.ts`; this file adds
 * what each token means. The design page and the contrast checks read it. Placard is light only;
 * dark mode is follow-up work (web/DESIGN.md).
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
  background: { meaning: "surface", role: "Page: slate, barely tinted toward navy" },
  card: { meaning: "surface", role: "Fields that hold reading text: sheets, dialogs, agent identity, charts" },
  muted: { meaning: "surface", role: "Quiet fields: a running agent's mode, system notices, skeletons, chart grid" },
  border: { meaning: "surface", role: "Hairlines between rows" },
  foreground: { meaning: "text", role: "Text and 2 px rules" },
  "muted-foreground": { meaning: "text", role: "Secondary text, labels, ages" },
  primary: { meaning: "account", role: "Navy as the primary action, links, and focus" },
  "primary-foreground": { meaning: "account", role: "Text on a primary action" },
  lapis: { meaning: "account", role: "The account, in navy: its board, its connection, the paper hatch, its chart line" },
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
  "mandate-edge": { meaning: "mandate", role: "The 4 px brass rule on top of the mandate tint" },
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
  { role: "display", className: "font-display text-display uppercase", sample: "Dashboard", spec: "Big Shoulders Display 800, 3rem / 0.9, capitals" },
  { role: "title", className: "font-display text-title uppercase", sample: "Agent 2", spec: "Big Shoulders Display 800, 2rem / 0.95, capitals" },
  { role: "heading", className: "font-display text-heading uppercase", sample: "Your mandate", spec: "Big Shoulders Display 800, 1.5rem / 1, capitals" },
  { role: "figure", className: "font-display text-[2.75rem] leading-none font-bold tabular", sample: "$10,123.45", spec: "Big Shoulders Display 700, 2.75rem, tabular figures" },
  { role: "body", className: "text-base", sample: "If you do nothing, this action is skipped.", spec: "Atkinson Hyperlegible Next 400, 1rem (17 px) / 1.5, sentence case" },
  { role: "small", className: "text-sm", sample: "Resting protection stays in place.", spec: "Atkinson Hyperlegible Next 400, 0.875rem / 1.45" },
  { role: "caption", className: "text-caption", sample: "as of 14:02:11, 3 min ago", spec: "Atkinson Hyperlegible Next 400, 0.8125rem / 1.35" },
  { role: "label", className: "label-caps", sample: "Your mandate", spec: "Atkinson Hyperlegible Next 700, 0.75rem, capitals (field labels only)" },
  { role: "number", className: "font-mono tabular", sample: "0.015 BTC/USD @ $56,700.00", spec: "Atkinson Hyperlegible Next, tabular figures" },
];

export const motionTokens = [
  { name: "--ease-out", value: "cubic-bezier(0.23, 1, 0.32, 1)", use: "Entrances, press, reveals, number changes" },
  { name: "--ease-in-out", value: "cubic-bezier(0.77, 0, 0.175, 1)", use: "Things that move on screen: chevrons, expand and collapse" },
  { name: "--ease-drawer", value: "cubic-bezier(0.32, 0.72, 0, 1)", use: "The Stop sheet" },
  { name: "--duration-press / --duration-release", value: "140 ms / 80 ms", use: "Press to scale 0.97; the release is faster than the press" },
  { name: "--duration-hover", value: "160 ms", use: "Colour changes on hover and on a mode change" },
  { name: "--duration-reveal", value: "200 ms, 30 ms stagger", use: "A field wipes in from its leading edge, once" },
  { name: "--duration-sheet", value: "280 ms in, 200 ms out", use: "Stop sheet" },
  { name: "--duration-dialog", value: "220 ms in, 150 ms out", use: "Step-up dialog" },
];

/** The gutters before and after the founder's "reduce gutter space" (2026-09-28). */
export const spacingTokens = [
  { name: "--nav-width", before: "15rem", after: "12rem", use: "Desktop side navigation, handed to Kumo's Sidebar" },
  { name: "--content-max", before: "72rem", after: "90rem", use: "Widest content column" },
  { name: "--page-x", before: "1rem / 1.5rem / 2rem", after: "0.75rem / 1rem / 1.5rem", use: "Page padding at phone / tablet / desktop" },
  { name: "--page-top", before: "1.5rem", after: "1rem / 1.25rem", use: "Space above the page title" },
  { name: "--page-bottom", before: "4rem (desktop)", after: "2.5rem (desktop)", use: "Space below the last section" },
  { name: "--section-gap", before: "2.5rem", after: "1.5rem", use: "Between sections of a screen" },
  { name: "--block-gap", before: "1rem to 1.5rem", after: "0.75rem", use: "Between a heading and its content, and between rows of fields" },
  { name: "--seam", before: "1rem (card gaps)", after: "0.375rem", use: "Between adjacent colour fields" },
];
