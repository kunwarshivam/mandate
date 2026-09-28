/**
 * The semantic pairs the palette is checked on, their contrast targets, and the colour-vision
 * checks (web/COLOR.md). The measuring lives in `contrast.ts`; this file has no dependencies so the
 * production design page can list the pairs.
 */
import type { Vision } from "./color";
import type { TokenName } from "./palette";

/**
 * body: reading text at any size (WCAG 4.5:1, APCA Lc 75). large: display headings and bold labels
 * of 24px and up (3:1, Lc 60). mark: non-text UI such as rails, rules, rings and borders (3:1, Lc 45).
 */
export type PairKind = "body" | "large" | "mark";

export const REQUIREMENT: Record<PairKind, { wcag: number; apca: number }> = {
  body: { wcag: 4.5, apca: 75 },
  large: { wcag: 3, apca: 60 },
  mark: { wcag: 3, apca: 45 },
};

export interface Pair {
  fg: TokenName;
  bg: TokenName;
  kind: PairKind;
  use: string;
}

export const PAIRS: Pair[] = [
  { fg: "foreground", bg: "background", kind: "body", use: "Body text on the page" },
  { fg: "foreground", bg: "card", kind: "body", use: "Body text on a card field" },
  { fg: "foreground", bg: "muted", kind: "body", use: "Text on a quiet field" },
  { fg: "foreground", bg: "lapis-soft", kind: "body", use: "Account notice text" },
  { fg: "foreground", bg: "mandate-soft", kind: "body", use: "Mandate notice text" },
  { fg: "muted-foreground", bg: "background", kind: "body", use: "Secondary text on the page" },
  { fg: "muted-foreground", bg: "card", kind: "body", use: "Secondary text on a card field" },
  { fg: "muted-foreground", bg: "muted", kind: "body", use: "Secondary text on a quiet field" },
  { fg: "muted-foreground", bg: "lapis-soft", kind: "body", use: "Secondary text in an account notice" },
  { fg: "muted-foreground", bg: "mandate-soft", kind: "body", use: "Secondary text in a mandate notice" },
  { fg: "primary", bg: "background", kind: "body", use: "Links on the page" },
  { fg: "primary", bg: "card", kind: "body", use: "Links on a card field" },
  { fg: "primary-foreground", bg: "primary", kind: "body", use: "Primary action label" },
  { fg: "primary-foreground", bg: "lapis-strong", kind: "body", use: "Primary action label, pressed or hovered" },
  { fg: "lapis-foreground", bg: "lapis", kind: "body", use: "Text on brand: the account board and the sidebar account block" },
  { fg: "lapis-muted", bg: "lapis", kind: "body", use: "Secondary text on brand" },
  { fg: "lapis-foreground", bg: "lapis-strong", kind: "body", use: "Text on a quiet field inside a navy surface" },
  { fg: "lapis-muted", bg: "lapis-strong", kind: "body", use: "Secondary text on a quiet field inside a navy surface" },
  { fg: "mandate-foreground", bg: "mandate", kind: "body", use: "Text on the mandate panel" },
  { fg: "mandate-muted", bg: "mandate", kind: "body", use: "Secondary text on the mandate panel" },
  { fg: "mandate-strong", bg: "mandate", kind: "body", use: "Mandate heading, labels, and the \"Your mandate\" tag" },
  { fg: "card", bg: "mandate-strong", kind: "body", use: "A mandate level's price label on a chart axis" },
  { fg: "foreground", bg: "selection", kind: "body", use: "Selected text" },
  { fg: "ink-foreground", bg: "ink", kind: "body", use: "Stop control, Stop sheet header, stopped mode, a proposal's axis label" },
  { fg: "lapis-muted", bg: "ink", kind: "body", use: "Secondary text inside an ink surface" },
  { fg: "crimson-foreground", bg: "crimson", kind: "body", use: "Kill switch label and its description" },
  { fg: "gain", bg: "card", kind: "body", use: "A gain on a card field, and an up candle on a chart" },
  { fg: "gain", bg: "background", kind: "body", use: "A gain on the page" },
  { fg: "loss", bg: "card", kind: "body", use: "A loss on a card field, and a down candle on a chart" },
  { fg: "loss", bg: "background", kind: "body", use: "A loss on the page" },
  { fg: "gain", bg: "gain-soft", kind: "body", use: "Success text on its tint" },
  { fg: "loss", bg: "loss-soft", kind: "body", use: "Loss text on its tint" },
  { fg: "warning", bg: "warning-soft", kind: "body", use: "Warning text on its tint" },
  { fg: "info", bg: "info-soft", kind: "body", use: "Info text on its tint" },
  { fg: "gain-cvd", bg: "card", kind: "body", use: "A gain, colour-blind friendly, on a card field, and its up candle" },
  { fg: "loss-cvd", bg: "card", kind: "body", use: "A loss, colour-blind friendly, on a card field, and its down candle" },
  { fg: "gain-cvd", bg: "background", kind: "body", use: "A gain, colour-blind friendly, on the page" },
  { fg: "loss-cvd", bg: "background", kind: "body", use: "A loss, colour-blind friendly, on the page" },
  { fg: "gain-cvd", bg: "gain-cvd-soft", kind: "body", use: "Colour-blind gain on its tint" },
  { fg: "loss-cvd", bg: "loss-cvd-soft", kind: "body", use: "Colour-blind loss on its tint" },
  { fg: "mandate-marker", bg: "mandate", kind: "mark", use: "Limit rail fill and level marks on the mandate panel" },
  { fg: "mandate-strong", bg: "mandate", kind: "mark", use: "The post where a limit stops the agent" },
  { fg: "mandate-edge", bg: "background", kind: "mark", use: "The mandate panel's edge against the page" },
  { fg: "mandate-marker", bg: "card", kind: "mark", use: "A mandate price line on a chart, and the sparkline's limit rule" },
  { fg: "ink", bg: "background", kind: "mark", use: "A paused or stopped mode field against the page" },
  { fg: "ink", bg: "card", kind: "mark", use: "The exits-only ring on a card field, a proposal's dashed line on a chart" },
  { fg: "lapis", bg: "card", kind: "mark", use: "Paper badge border, focus ring, the account's equity line" },
  { fg: "lapis", bg: "background", kind: "mark", use: "The current tab's bar" },
  { fg: "crimson", bg: "card", kind: "mark", use: "Kill switch against the Stop sheet" },
];

/** A Kumo surface scope in `placard-kumo.css`: the root, or a `[data-surface]` region. */
export type KumoScope = "root" | "navy" | "field" | "ink";

export interface KumoPair {
  scope: KumoScope;
  /** Kumo custom properties, without the leading dashes. */
  fg: string;
  bg: string;
  kind: PairKind;
  use: string;
}

/**
 * The same checks in Kumo's own role names, resolved through `placard-kumo.css` in each surface
 * scope, so a Kumo component drawn in any of them keeps the targets.
 */
export const KUMO_PAIRS: KumoPair[] = [
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-canvas", kind: "body", use: "Kumo text on the page" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Kumo text on a surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-elevated", kind: "body", use: "Kumo text on an elevated surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-recessed", kind: "body", use: "Kumo text on a recessed surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-control", kind: "body", use: "Text in a Kumo control" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-overlay", kind: "body", use: "Text in a Kumo popover, dropdown or toast" },
  { scope: "root", fg: "text-color-kumo-strong", bg: "color-kumo-tint", kind: "body", use: "The sidebar's current page" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-fill-hover", kind: "body", use: "A hovered Kumo item" },
  { scope: "root", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Kumo secondary text" },
  { scope: "root", fg: "text-color-kumo-subtle", bg: "color-kumo-canvas", kind: "body", use: "Kumo secondary text on the page" },
  { scope: "root", fg: "text-color-kumo-subtle", bg: "color-kumo-tint", kind: "body", use: "Kumo secondary text on a tint" },
  { scope: "root", fg: "text-color-kumo-placeholder", bg: "color-kumo-control", kind: "body", use: "Placeholder in a Kumo input" },
  { scope: "root", fg: "text-color-kumo-link", bg: "color-kumo-base", kind: "body", use: "A Kumo link" },
  { scope: "root", fg: "text-color-kumo-brand", bg: "color-kumo-canvas", kind: "body", use: "Kumo brand text" },
  { scope: "root", fg: "text-color-kumo-inverse", bg: "color-kumo-brand", kind: "body", use: "A Kumo primary button's label" },
  { scope: "root", fg: "text-color-kumo-inverse", bg: "color-kumo-brand-hover", kind: "body", use: "A Kumo primary button's label on hover" },
  { scope: "root", fg: "text-color-kumo-inverse", bg: "color-kumo-contrast", kind: "body", use: "Text on Kumo's contrast fill" },
  { scope: "root", fg: "text-color-kumo-info", bg: "color-kumo-info-tint", kind: "body", use: "Kumo info text on its tint" },
  { scope: "root", fg: "text-color-kumo-success", bg: "color-kumo-success-tint", kind: "body", use: "Kumo success text on its tint" },
  { scope: "root", fg: "text-color-kumo-danger", bg: "color-kumo-danger-tint", kind: "body", use: "Kumo danger (ink) text on its tint" },
  { scope: "root", fg: "text-color-kumo-warning", bg: "color-kumo-warning-tint", kind: "body", use: "Kumo warning text on its tint (on no screen)" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-banner-info", kind: "body", use: "Text in a Kumo banner" },
  { scope: "root", fg: "text-color-kumo-badge-orange-subtle", bg: "color-kumo-badge-orange", kind: "body", use: "A mandate badge" },
  { scope: "root", fg: "text-color-kumo-badge-inverted", bg: "color-kumo-badge-inverted", kind: "body", use: "An inverted Kumo badge" },
  { scope: "root", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Kumo focus ring" },
  { scope: "root", fg: "color-kumo-brand", bg: "color-kumo-canvas", kind: "mark", use: "A Kumo primary button against the page" },
  { scope: "root", fg: "color-kumo-danger", bg: "color-kumo-base", kind: "mark", use: "Kumo danger (ink) against a surface" },
  { scope: "navy", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text in the sidebar's account block" },
  { scope: "navy", fg: "text-color-kumo-strong", bg: "color-kumo-base", kind: "body", use: "Strong text on navy" },
  { scope: "navy", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on navy" },
  { scope: "navy", fg: "text-color-kumo-link", bg: "color-kumo-base", kind: "body", use: "A link on navy" },
  { scope: "navy", fg: "text-color-kumo-default", bg: "color-kumo-tint", kind: "body", use: "A current or hovered item on navy" },
  { scope: "navy", fg: "text-color-kumo-subtle", bg: "color-kumo-tint", kind: "body", use: "Secondary text on a navy tint" },
  { scope: "navy", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Focus ring on navy" },
  { scope: "field", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text on the mandate's brass tint" },
  { scope: "field", fg: "text-color-kumo-strong", bg: "color-kumo-base", kind: "body", use: "Mandate labels in dark brass" },
  { scope: "field", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on the mandate tint" },
  { scope: "field", fg: "color-kumo-line", bg: "color-kumo-base", kind: "mark", use: "The brass rule and hairlines on the mandate tint" },
  { scope: "field", fg: "color-kumo-line", bg: "color-kumo-canvas", kind: "mark", use: "The brass rule against the page" },
  { scope: "ink", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text on ink" },
  { scope: "ink", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on ink" },
  { scope: "ink", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Focus ring on ink" },
];

/** Two colours that must never be confused need this OKLab distance under the simulated vision. */
export const CVD_DISTINCT = 0.1;
export const CVD_VISIONS: Vision[] = ["deuteranopia", "protanopia"];

export interface CvdCheck {
  a: TokenName;
  b: TokenName;
  what: string;
  /** Checks that are information only: the colour pair is not relied on to tell the two apart. */
  required: boolean;
  why: string;
}

export const CVD_CHECKS: CvdCheck[] = [
  { a: "gain-cvd", b: "loss-cvd", what: "Gain versus loss, colour-blind friendly", required: true, why: "Blue and orange keep the blue-yellow axis that red-green deficiencies preserve; candles use them too." },
  { a: "gain", b: "loss", what: "Gain versus loss, default", required: false, why: "Green and red at matched lightness merge; the sign and the word carry the meaning, and the preference remaps them." },
  { a: "muted", b: "ink", what: "Mode: running versus paused or stopped", required: true, why: "Lightness alone separates them." },
  { a: "card", b: "ink", what: "Mode: exits-only ring on a card field", required: true, why: "Lightness alone separates them." },
  { a: "ink", b: "crimson", what: "Pause (ink) versus kill switch (crimson)", required: true, why: "Side by side in the Stop sheet; protanopia darkens red toward ink." },
  { a: "lapis", b: "mandate-marker", what: "Account line versus mandate levels", required: true, why: "On every equity chart and on the equity ladder; the account and the mandate must stay apart." },
  { a: "lapis", b: "mandate", what: "Account board versus mandate field", required: true, why: "The Meaning Rule's two biggest fields." },
  { a: "mandate-edge", b: "background", what: "Mandate panel edge versus the page", required: true, why: "The panel must read as its own region." },
  { a: "mandate-marker", b: "gain-cvd", what: "Mandate level versus an up candle, colour-blind friendly", required: true, why: "Both are drawn on the position chart." },
  { a: "mandate-marker", b: "loss-cvd", what: "Mandate level versus a down candle, colour-blind friendly", required: false, why: "Brass and orange are neighbours in hue; the level is a labelled line, the candle a body." },
  { a: "mandate-marker", b: "gain", what: "Mandate accent versus a gain", required: false, why: "A shared hue family would blur two meanings." },
  { a: "mandate-strong", b: "gain", what: "Mandate label versus gain text", required: false, why: "Labels and gains sit close together on the agent card." },
  { a: "mandate-strong", b: "info", what: "Mandate label versus info text", required: false, why: "Info is the brand blue at text lightness." },
  { a: "mandate-strong", b: "gain-cvd", what: "Mandate label versus a colour-blind gain", required: false, why: "The same question with the preference on." },
  { a: "mandate-strong", b: "warning", what: "Mandate label versus warning text", required: false, why: "Brass and amber are neighbours in hue, so warning stays off the screens." },
  { a: "mandate-strong", b: "loss-cvd", what: "Mandate label versus a colour-blind loss", required: false, why: "Brass and orange are neighbours in hue." },
];

/**
 * The paper hatch's lines against a card: enough to read as simulated funds at a glance, never as
 * loud as text (WCAG ratio of the composited line colour to the card).
 */
export const HATCH_MIN = 1.5;
export const HATCH_MAX = 2.5;
