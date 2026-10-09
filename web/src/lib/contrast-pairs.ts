/**
 * The semantic pairs the palette is checked on, in both themes, their contrast targets, and the
 * colour-vision checks (web/COLOR.md). The measuring lives in `contrast.ts`; this file has no
 * dependencies so the production design page can list the pairs.
 */
import type { Vision } from "./color";
import type { ThemeName, TokenName } from "./palette";

/**
 * body: reading text at any size (WCAG 4.5:1, APCA Lc 75). figure: a gain or a loss, a signed figure
 * of weight 500 or more with its sign and word beside it (WCAG 4.5:1, APCA Lc 60, DEC-217). large: display headings and bold labels
 * of 24px and up (3:1, Lc 60). mark: non-text UI such as rails, rules, rings and borders (3:1, Lc 45).
 */
export type PairKind = "body" | "figure" | "large" | "mark";

export const REQUIREMENT: Record<PairKind, { wcag: number; apca: number }> = {
  body: { wcag: 4.5, apca: 75 },
  figure: { wcag: 4.5, apca: 60 },
  large: { wcag: 3, apca: 60 },
  mark: { wcag: 3, apca: 45 },
};

/** The Stop control and the kill switch are read under stress: their labels keep 7:1 in both themes. */
export const STOP_CONTRAST = 7;

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
  { fg: "foreground", bg: "lapis-soft", kind: "body", use: "Text on the account's pale azure: an approval card, an account notice" },
  { fg: "foreground", bg: "mandate-soft", kind: "body", use: "Mandate notice text" },
  { fg: "foreground", bg: "gain-soft", kind: "body", use: "Text on a success tint" },
  { fg: "foreground", bg: "loss-soft", kind: "body", use: "Text on a loss tint" },
  { fg: "foreground", bg: "info-soft", kind: "body", use: "Text on an info tint" },
  { fg: "muted-foreground", bg: "background", kind: "body", use: "Secondary text on the page" },
  { fg: "muted-foreground", bg: "card", kind: "body", use: "Secondary text on a card field" },
  { fg: "muted-foreground", bg: "muted", kind: "body", use: "Secondary text on a quiet field" },
  { fg: "muted-foreground", bg: "lapis-soft", kind: "body", use: "Secondary text on the account's pale azure" },
  { fg: "muted-foreground", bg: "background", kind: "body", use: "The Allowed verdict chip's label on its well fill" },
  { fg: "muted-foreground", bg: "mandate-soft", kind: "body", use: "Secondary text in a mandate notice" },
  { fg: "muted-foreground", bg: "info-soft", kind: "body", use: "Secondary text on an info tint" },
  { fg: "muted-foreground", bg: "gain-soft", kind: "body", use: "The time on the hero's change pill, for a gain" },
  { fg: "muted-foreground", bg: "loss-soft", kind: "body", use: "The time on the hero's change pill, for a loss" },
  { fg: "muted-foreground", bg: "gain-cvd-soft", kind: "body", use: "The time on the hero's change pill, colour-blind gain" },
  { fg: "muted-foreground", bg: "loss-cvd-soft", kind: "body", use: "The time on the hero's change pill, colour-blind loss" },
  { fg: "lapis", bg: "lapis-soft", kind: "body", use: "The current range and tab on their pale azure pill, a link in an approval card" },
  { fg: "primary", bg: "background", kind: "body", use: "Links on the page" },
  { fg: "primary", bg: "card", kind: "body", use: "Links on a card field" },
  { fg: "primary-foreground", bg: "primary", kind: "body", use: "Primary action label" },
  { fg: "primary-foreground", bg: "lapis-strong", kind: "body", use: "Primary action label, pressed or hovered" },
  { fg: "lapis-foreground", bg: "lapis", kind: "body", use: "Text on the account fill: a primary action, the approvals count" },
  { fg: "lapis-muted", bg: "lapis", kind: "body", use: "Secondary text on the account fill" },
  { fg: "lapis-foreground", bg: "lapis-strong", kind: "body", use: "Text on a quiet field inside an account surface" },
  { fg: "lapis-muted", bg: "lapis-strong", kind: "body", use: "Secondary text on a quiet field inside an account surface" },
  { fg: "mandate-foreground", bg: "mandate", kind: "body", use: "Text on the mandate field" },
  { fg: "mandate-muted", bg: "mandate", kind: "body", use: "Secondary text on the mandate field" },
  { fg: "mandate-strong", bg: "mandate", kind: "body", use: "Mandate heading, labels, the \"Your mandate\" tag, and a mandate level's price label on a chart axis" },
  { fg: "mandate-strong", bg: "mandate-soft", kind: "body", use: "A mandate label in a mandate notice" },
  { fg: "mandate-strong", bg: "card", kind: "body", use: "A mandate label on a card field, and the focus ring (`--ring`)" },
  { fg: "mandate-strong", bg: "background", kind: "body", use: "A mandate label on the page, the focus ring there, and the post that ends a phone headroom meter" },
  { fg: "foreground", bg: "selection", kind: "body", use: "Selected text" },
  { fg: "highlight-foreground", bg: "highlight", kind: "body", use: "A call to action on the highlight, and any label on the sun fill" },
  { fg: "ink-foreground", bg: "ink", kind: "body", use: "The loud Stop control, a paused or stopped mode pill, a proposal's axis label" },
  { fg: "ink", bg: "card", kind: "body", use: "The quiet Stop control's label and octagon on the dock and the tab bar" },
  { fg: "ink", bg: "background", kind: "body", use: "The quiet Stop control's label and octagon, hovered" },
  { fg: "lapis-muted", bg: "ink", kind: "body", use: "Secondary text inside an ink surface" },
  { fg: "crimson-foreground", bg: "crimson", kind: "body", use: "Kill switch label and its description" },
  { fg: "gain", bg: "card", kind: "figure", use: "A gain on a card field, and an up candle on a chart" },
  { fg: "gain", bg: "background", kind: "figure", use: "A gain on the page" },
  { fg: "loss", bg: "card", kind: "figure", use: "A loss on a card field, and a down candle on a chart" },
  { fg: "loss", bg: "background", kind: "figure", use: "A loss on the page" },
  { fg: "gain", bg: "gain-soft", kind: "figure", use: "Success text on its tint" },
  { fg: "loss", bg: "loss-soft", kind: "figure", use: "Loss text on its tint" },
  { fg: "warning", bg: "warning-soft", kind: "body", use: "Warning text on its tint" },
  { fg: "info", bg: "info-soft", kind: "body", use: "Info text on its tint" },
  { fg: "gain-cvd", bg: "card", kind: "body", use: "A gain, colour-blind friendly, on a card field, and its up candle" },
  { fg: "loss-cvd", bg: "card", kind: "body", use: "A loss, colour-blind friendly, on a card field, and its down candle" },
  { fg: "gain-cvd", bg: "background", kind: "body", use: "A gain, colour-blind friendly, on the page" },
  { fg: "loss-cvd", bg: "background", kind: "body", use: "A loss, colour-blind friendly, on the page" },
  { fg: "gain-cvd", bg: "gain-cvd-soft", kind: "body", use: "Colour-blind gain on its tint" },
  { fg: "loss-cvd", bg: "loss-cvd-soft", kind: "body", use: "Colour-blind loss on its tint" },
  { fg: "mandate-marker", bg: "mandate", kind: "mark", use: "Limit rail fill and level marks on the mandate field" },
  { fg: "mandate-strong", bg: "mandate", kind: "mark", use: "The post where a limit stops the agent" },
  { fg: "mandate-edge", bg: "background", kind: "mark", use: "The mandate field's edge against the page" },
  { fg: "mandate-marker", bg: "card", kind: "mark", use: "The sparkline's limit rule, and level marks on a card field" },
  { fg: "mandate-strong", bg: "muted", kind: "mark", use: "The post where a limit stops the agent, on a phone headroom meter's track" },
  { fg: "foreground", bg: "muted", kind: "mark", use: "The used share of a phone headroom meter against its track" },
  { fg: "lapis-line", bg: "card", kind: "mark", use: "The account's equity line on a chart" },
  { fg: "lapis-line", bg: "background", kind: "mark", use: "The current tab's bar" },
  { fg: "lapis-line", bg: "lapis-soft", kind: "mark", use: "The ring of the current tab and range on their pale azure pill" },
  { fg: "muted-foreground", bg: "card", kind: "mark", use: "A mandate level's dashed line on a chart; the performance disclosure symbol on a card field" },
  { fg: "muted-foreground", bg: "background", kind: "mark", use: "The performance disclosure symbol on a hovered agent row" },
  { fg: "muted-foreground", bg: "mandate", kind: "mark", use: "The performance disclosure symbol on the mandate field" },
  { fg: "ink", bg: "background", kind: "mark", use: "A paused or stopped mode field against the page" },
  { fg: "ink", bg: "card", kind: "mark", use: "The quiet Stop control's outline on the dock and the tab bar, the exits-only ring on a card field, a proposal's dashed line on a chart" },
  { fg: "lapis", bg: "card", kind: "mark", use: "Paper badge border, the account's marker on the equity ladder" },
  { fg: "lapis", bg: "mandate", kind: "mark", use: "The account's marker on the mandate field" },
  { fg: "muted-foreground", bg: "background", kind: "mark", use: "The Allowed verdict chip's hairline against its own well fill, and against the page on a hovered row" },
  { fg: "muted-foreground", bg: "card", kind: "mark", use: "The Allowed verdict chip's hairline against the card field around it" },
  { fg: "crimson-edge", bg: "card", kind: "mark", use: "The kill switch's edge against the Stop sheet" },
  { fg: "crimson-edge", bg: "background", kind: "mark", use: "The kill switch's edge against the page" },
];

/** A Kumo surface scope in `kumo-theme.css`: the root, or a `[data-surface]` region. */
export type KumoScope = "root" | "account" | "field" | "ink";

export interface KumoPair {
  scope: KumoScope;
  /** Kumo custom properties, without the leading dashes. */
  fg: string;
  bg: string;
  kind: PairKind;
  use: string;
}

/**
 * The same checks in Kumo's own role names, resolved through `kumo-theme.css` in each surface
 * scope and each theme, so a Kumo component drawn in any of them keeps the targets.
 */
export const KUMO_PAIRS: KumoPair[] = [
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-canvas", kind: "body", use: "Kumo text on the page" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Kumo text on a surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-elevated", kind: "body", use: "Kumo text on an elevated surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-recessed", kind: "body", use: "Kumo text on a recessed surface" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-control", kind: "body", use: "Text in a Kumo control" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-overlay", kind: "body", use: "Text in a Kumo popover, dropdown or toast" },
  { scope: "root", fg: "text-color-kumo-strong", bg: "color-kumo-tint", kind: "body", use: "A highlighted item in a Kumo menu, select or tab list" },
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
  { scope: "root", fg: "text-color-kumo-success", bg: "color-kumo-success-tint", kind: "figure", use: "Kumo success text on its tint, the gain colour" },
  { scope: "root", fg: "text-color-kumo-danger", bg: "color-kumo-danger-tint", kind: "body", use: "Kumo danger (ink) text on its tint" },
  { scope: "root", fg: "text-color-kumo-warning", bg: "color-kumo-warning-tint", kind: "body", use: "Kumo warning text on its tint (on no screen)" },
  { scope: "root", fg: "text-color-kumo-default", bg: "color-kumo-banner-info", kind: "body", use: "Text in a Kumo banner" },
  { scope: "root", fg: "text-color-kumo-badge-orange-subtle", bg: "color-kumo-badge-orange", kind: "body", use: "A mandate badge" },
  { scope: "root", fg: "text-color-kumo-badge-inverted", bg: "color-kumo-badge-inverted", kind: "body", use: "An inverted Kumo badge" },
  { scope: "root", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Kumo focus ring" },
  { scope: "root", fg: "color-kumo-brand", bg: "color-kumo-canvas", kind: "mark", use: "A Kumo primary button against the page" },
  { scope: "root", fg: "color-kumo-danger", bg: "color-kumo-base", kind: "mark", use: "Kumo danger (ink) against a surface" },
  { scope: "account", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text on an account fill" },
  { scope: "account", fg: "text-color-kumo-strong", bg: "color-kumo-base", kind: "body", use: "Strong text on an account fill" },
  { scope: "account", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on an account fill" },
  { scope: "account", fg: "text-color-kumo-link", bg: "color-kumo-base", kind: "body", use: "A link on an account fill" },
  { scope: "account", fg: "text-color-kumo-default", bg: "color-kumo-tint", kind: "body", use: "A current or hovered item on an account fill" },
  { scope: "account", fg: "text-color-kumo-subtle", bg: "color-kumo-tint", kind: "body", use: "Secondary text on an account tint" },
  { scope: "account", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Focus ring on an account fill" },
  { scope: "field", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text on the mandate's azure field" },
  { scope: "field", fg: "text-color-kumo-strong", bg: "color-kumo-base", kind: "body", use: "Mandate labels in deep azure" },
  { scope: "field", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on the mandate field" },
  { scope: "field", fg: "color-kumo-line", bg: "color-kumo-base", kind: "mark", use: "The azure rule and hairlines on the mandate field" },
  { scope: "field", fg: "color-kumo-line", bg: "color-kumo-canvas", kind: "mark", use: "The azure rule against the page" },
  { scope: "ink", fg: "text-color-kumo-default", bg: "color-kumo-base", kind: "body", use: "Text on ink" },
  { scope: "ink", fg: "text-color-kumo-subtle", bg: "color-kumo-base", kind: "body", use: "Secondary text on ink" },
  { scope: "ink", fg: "color-kumo-focus", bg: "color-kumo-base", kind: "mark", use: "Focus ring on ink" },
];

/** Two colours that must never be confused need this OKLab distance under the simulated vision. */
export const CVD_DISTINCT = 0.1;
/**
 * Deuteranopia and protanopia are the red-green deficiencies (about 1 in 12 men of northern European
 * descent); tritanopia, the blue-yellow one, is rare, but a blue accent is where it bites.
 */
export const CVD_VISIONS: Vision[] = ["deuteranopia", "protanopia", "tritanopia"];

export interface CvdCheck {
  a: TokenName;
  b: TokenName;
  what: string;
  /** The themes in which the pair must stay apart; empty for a check that is information only. */
  required: ThemeName[];
  /** The visions under which a required pair must stay apart, when not all of `CVD_VISIONS`. */
  visions?: Vision[];
  why: string;
}

const BOTH: ThemeName[] = ["light", "dark"];

export const CVD_CHECKS: CvdCheck[] = [
  { a: "gain-cvd", b: "loss-cvd", what: "Gain versus loss, colour-blind friendly", required: BOTH, why: "The remap exists for this pair; candles use it too. A teal gain merges with a raspberry loss at the same depth, so the light loss is a step darker." },
  { a: "gain", b: "loss", what: "Gain versus loss, default", required: [], why: "Green and red at matched lightness merge; the sign and the word carry the meaning, and the preference remaps them." },
  { a: "gain-cvd", b: "crimson", what: "Colour-blind gain versus the kill switch", required: BOTH, why: "A gain must never read as the kill switch." },
  { a: "loss-cvd", b: "crimson", what: "Colour-blind loss versus the kill switch", required: BOTH, why: "A dark orange loss merges with crimson in light mode, so the light loss is raspberry." },
  { a: "gain-cvd", b: "mandate-marker", what: "Colour-blind gain versus the mandate's azure marks", required: BOTH, visions: ["deuteranopia", "protanopia"], why: "Candles and mandate marks share the position chart and the envelope. Tritanopia merges blue and teal whatever their depth (DEC-217), so there the pair is information only: the marks are labelled and in fixed places, and a gain carries its plus sign." },
  { a: "loss-cvd", b: "mandate-marker", what: "Colour-blind loss versus the mandate's azure marks", required: BOTH, why: "The same, for a down candle." },
  { a: "gain-cvd", b: "lapis-line", what: "Colour-blind gain versus the account's azure line", required: BOTH, visions: ["deuteranopia", "protanopia"], why: "An up candle beside the account's line; under tritanopia, information only, as for the mandate's marks." },
  { a: "loss-cvd", b: "lapis-line", what: "Colour-blind loss versus the account's azure line", required: BOTH, why: "A down candle beside the account's line." },
  {
    a: "gain-cvd",
    b: "mandate-strong",
    what: "Colour-blind gain versus azure text",
    required: ["light"],
    visions: ["deuteranopia", "protanopia"],
    why: "Gains and mandate labels sit close together on the agent screen, which is why the gain is teal, not blue. Where blue and teal meet (tritanopia, and every pale colour of dark mode under red-green deficiency) the pair is information only: azure text is always a labelled word in a fixed place, and a gain always carries its plus sign.",
  },
  { a: "loss-cvd", b: "mandate-strong", what: "Colour-blind loss versus azure text", required: ["light"], why: "A loss and a mandate label can share a row." },
  {
    a: "loss-cvd",
    b: "mandate-strong",
    what: "Colour-blind loss versus azure text, dark",
    required: ["dark"],
    visions: ["deuteranopia", "protanopia"],
    why: "In dark both are pale, and under tritanopia pale orange and pale azure sit a hair apart. Azure text is always a labelled word in a fixed place, and a loss always carries its minus sign.",
  },
  { a: "muted", b: "ink", what: "Mode: running versus paused or stopped", required: BOTH, why: "Lightness alone separates them." },
  { a: "card", b: "ink", what: "Mode: exits-only ring on a card field", required: BOTH, why: "Lightness alone separates them." },
  { a: "ink", b: "crimson", what: "Pause (ink) versus kill switch (crimson)", required: BOTH, why: "Side by side in the Stop sheet; protanopia darkens red toward ink." },
  { a: "lapis-line", b: "muted-foreground", what: "Account line versus a mandate level's line", required: BOTH, why: "On every agent's equity chart: the account is an azure line, a mandate level a dashed grey one." },
  { a: "lapis", b: "mandate-marker", what: "Account marker versus mandate marks", required: BOTH, why: "On the equity ladder the account's dot sits among the mandate's ticks." },
  { a: "lapis", b: "mandate", what: "Account fill versus mandate field", required: BOTH, why: "The Meaning Rule's two biggest fields." },
  { a: "mandate-edge", b: "background", what: "Mandate field edge versus the page", required: BOTH, why: "The field must read as its own region." },
];

/**
 * The paper hatch's lines against a card: enough to read as simulated funds at a glance, never as
 * loud as text (WCAG ratio of the composited line colour to the card).
 */
export const HATCH_MIN = 1.5;
export const HATCH_MAX = 2.5;
