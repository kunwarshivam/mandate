/**
 * The Placard colour tokens, one entry per CSS custom property in `globals.css` (the test
 * `tokens.test.ts` keeps the two in step). The design page and the contrast checks read this file.
 * Placard is light only; dark mode is follow-up work (web/DESIGN.md).
 */

export type Meaning = "surface" | "text" | "mandate" | "account" | "stopped" | "kill" | "result";

export interface ColorToken {
  name: string;
  meaning: Meaning;
  role: string;
  value: string;
}

export const colorTokens: ColorToken[] = [
  { name: "background", meaning: "surface", role: "Page: a cool, barely tinted white", value: "oklch(0.975 0.005 250)" },
  { name: "card", meaning: "surface", role: "Fields that hold reading text: sheets, dialogs, agent identity", value: "oklch(0.995 0.002 250)" },
  { name: "muted", meaning: "surface", role: "Quiet fields: a running agent's mode, system notices, skeletons", value: "oklch(0.935 0.01 250)" },
  { name: "border", meaning: "surface", role: "Hairlines between rows", value: "oklch(0.8 0.02 255)" },
  { name: "foreground", meaning: "text", role: "Text and 2 px rules", value: "oklch(0.21 0.035 258)" },
  { name: "muted-foreground", meaning: "text", role: "Secondary text, labels, ages", value: "oklch(0.44 0.035 258)" },
  { name: "primary", meaning: "account", role: "Lapis as the primary action, links, and focus", value: "oklch(0.36 0.1 258)" },
  { name: "primary-foreground", meaning: "account", role: "Text on a primary action", value: "oklch(0.975 0.005 250)" },
  { name: "lapis", meaning: "account", role: "The account: its board, its connection, paper hatch, the current page", value: "oklch(0.36 0.1 258)" },
  { name: "lapis-foreground", meaning: "account", role: "Text on lapis", value: "oklch(0.975 0.005 250)" },
  { name: "lapis-muted", meaning: "account", role: "Secondary text on lapis", value: "oklch(0.84 0.03 255)" },
  { name: "lapis-soft", meaning: "account", role: "An account notice: reconciliation, unknown order, activity at the broker", value: "oklch(0.93 0.03 255)" },
  { name: "marigold", meaning: "mandate", role: "Your mandate: limits, rails, and the envelope", value: "oklch(0.85 0.155 84)" },
  { name: "marigold-foreground", meaning: "mandate", role: "Text and rail fill on marigold", value: "oklch(0.21 0.035 258)" },
  { name: "marigold-muted", meaning: "mandate", role: "Secondary text on marigold", value: "oklch(0.36 0.05 70)" },
  { name: "marigold-soft", meaning: "mandate", role: "A mandate notice: a limit acted (drawdown, daily loss, floor, goal)", value: "oklch(0.955 0.05 90)" },
  { name: "ink", meaning: "stopped", role: "A stopped or paused agent, and the Stop control", value: "oklch(0.21 0.035 258)" },
  { name: "ink-foreground", meaning: "stopped", role: "Text on ink", value: "oklch(0.975 0.005 250)" },
  { name: "crimson", meaning: "kill", role: "The kill switch, and nothing else", value: "oklch(0.47 0.19 27)" },
  { name: "crimson-foreground", meaning: "kill", role: "Text on crimson", value: "oklch(0.985 0.004 250)" },
  { name: "gain", meaning: "result", role: "A gain as text, always with a plus sign and the word", value: "oklch(0.44 0.11 155)" },
  { name: "loss", meaning: "result", role: "A loss as text, always with a minus sign and the word", value: "oklch(0.49 0.18 10)" },
];

/** Text/background pairs that carry reading text. Each must reach WCAG AA (4.5:1). */
export const textPairs: Array<{ fg: string; bg: string; use: string }> = [
  { fg: "foreground", bg: "background", use: "Body text on the page" },
  { fg: "foreground", bg: "card", use: "Body text on a card field" },
  { fg: "foreground", bg: "muted", use: "Text on a quiet field" },
  { fg: "foreground", bg: "lapis-soft", use: "Account notice text" },
  { fg: "foreground", bg: "marigold-soft", use: "Notice text when your mandate acted" },
  { fg: "muted-foreground", bg: "background", use: "Secondary text on the page" },
  { fg: "muted-foreground", bg: "card", use: "Secondary text on a card field" },
  { fg: "muted-foreground", bg: "muted", use: "Secondary text on a quiet field" },
  { fg: "muted-foreground", bg: "lapis-soft", use: "Secondary text in an account notice" },
  { fg: "muted-foreground", bg: "marigold-soft", use: "Secondary text in a mandate notice" },
  { fg: "primary", bg: "background", use: "Links on the page" },
  { fg: "primary", bg: "card", use: "Links on a card field" },
  { fg: "primary-foreground", bg: "primary", use: "Primary action label" },
  { fg: "lapis-foreground", bg: "lapis", use: "Text on the account board" },
  { fg: "lapis-muted", bg: "lapis", use: "Secondary text on the account board" },
  { fg: "marigold", bg: "lapis", use: "A mode that is not normal, on the account board" },
  { fg: "marigold-foreground", bg: "marigold", use: "Text on the mandate field" },
  { fg: "marigold-muted", bg: "marigold", use: "Secondary text on the mandate field" },
  { fg: "ink-foreground", bg: "ink", use: "Stopped mode label, Stop control label" },
  { fg: "crimson-foreground", bg: "crimson", use: "Kill switch label" },
  { fg: "gain", bg: "card", use: "Gains on a card field" },
  { fg: "gain", bg: "background", use: "Gains on the page" },
  { fg: "loss", bg: "card", use: "Losses on a card field" },
  { fg: "loss", bg: "background", use: "Losses on the page" },
];

/** Non-text marks that must reach 3:1 against their surface (WCAG 1.4.11). */
export const markPairs: Array<{ fg: string; bg: string; use: string }> = [
  { fg: "marigold-foreground", bg: "marigold", use: "Rail fill and limit wall on the mandate field" },
  { fg: "ink", bg: "background", use: "An ink mode field against the page" },
  { fg: "lapis", bg: "card", use: "Paper badge border, focus ring" },
  { fg: "crimson", bg: "card", use: "Kill switch against a sheet" },
];

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
