# Owlhead colour: Ink and Gold

Owlhead's palette is Ink and Gold, in a light and a dark theme ([DEC-204](../docs/project/04-decision-log.md#decisions), the founder, 2026-09-28; it supersedes DEC-202). Paper surfaces and ink type carry the product. Gold is the one accent: your mandate, and the account's line. Crimson is the kill switch and nothing else. In dark mode the roles swap ends of the same ramps: ink surfaces, paper type, and gold a step lighter.

The key values, as ramp steps:

| Role | Light | Dark |
|---|---|---|
| The body, header and cards (`card`) | paper-50 #FDFCFA | ink-950 #14161A |
| Wells and the sidebar (`background`) | paper-100 #F8F7F4 | ink-975 #0B0D11 |
| Type, primary action, Stop | ink-950 #14161A | paper-100 #F8F7F4 |
| Gold line and marker | gold-500 #AB7D13 | gold-400 #D9A948 |
| Gold text | gold-700 #6A4D08 | gold-300 #F9D28A |
| Gold field (the mandate) | gold-100 #FFF6E6 | gold-850 #3C2E14 |
| Kill switch fill / edge | crimson-700 #9C0C12 / the same | crimson-700 #9C0C12 / crimson-400 #FD8C81 |

The files:

- `src/lib/palette.ts` holds the ramps and both themes' semantic tokens. `globals.css` declares the same values in `:root` and in `html:root[data-mode="dark"]`, and `tokens.test.ts` fails if either block drifts.
- `src/lib/contrast-pairs.ts` holds the pairs, their targets and the colour-vision checks, and `src/lib/contrast.ts` measures them for either theme. `palette.test.ts` enforces everything this document claims, in both themes, including every Kumo role in every surface scope. APCA is measured only in the tests, through `src/test/apca.ts` (see Contrast).
- `/palette` (development only) shows the palette in use, the Kumo surfaces, the ramps, both themes' tokens, and the contrast and colour-vision tables for each theme. It is a `page.dev.tsx` route: it is not compiled into a production build and answers 404 outside development.
- Colour-blind friendly is a development preference until settings exist: `?cvd=1` or `?cvd=0` on any URL, Alt+Shift+C, or the checkbox in the scenario switcher. A production build renders the default gain and loss colours.

## Principles

### 1. No hue pushes

Red and other warm, saturated hues raise arousal and urgency. Mehta and Zhu (2009, *Science* 323) found that red primes avoidance and vigilance; Bagchi and Cheema (2013, *Journal of Consumer Research* 39(5)) found that red backgrounds make people bid and haggle more aggressively. We do not want the screen to push an owner toward an aggressive decision. So most of the screen is near-neutral: paper and ink. The one accent, gold, is deep and low in chroma in light mode. Warm, saturated colour is kept for the one control that must interrupt: the kill switch.

### 2. Proportion: 60/30/10

About 60% of the screen is paper (ink in dark mode), about 30% is type and ink actions, and about 10% is gold. Saturation pulls the eye, so it is spent only where attention is needed. "Your mandate" is on every agent all the time, so it has to be restrained rather than loud: a pale gold field under a gold rule, with gold markers and dark gold labels.

### 3. One meaning per colour

| Colour | Means | Never used for |
|---|---|---|
| Gold (`mandate-*`) | Your mandate: the envelope, limits, rails, the "Your mandate" tag, a mandate level's axis label | Anything the account or the platform imposed |
| Gold line (`lapis-line`) and ink fill (`lapis`) | The account: its equity line, primary actions, links, the current tab and range | Your mandate |
| Ink | A paused or stopped agent, and the Stop control | Decoration |
| Crimson | The kill switch, and nothing else | Errors, losses, warnings |

The account's token is still called `lapis`, so class names stay stable. Its fill is ink in light mode and paper in dark. Its line is gold, the same hue as the mandate but never beside a mandate mark without a name: on a chart the account is a solid gold line and a mandate level is a dashed grey line with a gold label.

The status family: gain is green; loss is red, as text and markers only, never crimson (at least 15 degrees of hue away) and never a fill; warning is amber, on no screen; info is the muted type. What never gets colour: system states (stale, unreachable, loading), deadlines, the Approve and Skip buttons, and anything decorative.

### 4. A system, not a set of picks

- **OKLCH** (Ottosson 2020, "A perceptual color space for image processing"; CSS Color 4). Equal steps in L look like equal steps in lightness, so a ramp is predictable and contrast can be designed rather than found.
- **Thirteen steps per hue (50 to 975)** at constant hue, on one lightness curve shared by every ramp. Chroma rises to a hump mid-ramp and is clamped to 97% of the sRGB gamut. Light mode reads from the top of each ramp and dark mode from the bottom; steps 850 and 975 exist for dark mode's borders and wells.
- **Neutrals** are two ramps: paper, warm (hue 85), and ink, cool (hue 255), both at C 0.003 to 0.01. Nothing is pure black, white or grey.
- **Gain and loss** share L and C at every step and differ only in hue, so neither is louder than the other.
- **Semantic tokens** map to ramp steps in each theme, and components use only the semantic tokens. A test fails on any raw colour value in `src/components/`.

### 5. Contrast: WCAG 2.2 AA is the floor, APCA Bronze is also required

- WCAG 2.2 (W3C Recommendation, 2023): 4.5:1 for body text (1.4.3), 3:1 for large text and for UI and non-text marks (1.4.11), and colour is never the only cue (1.4.1).
- APCA (Somers, `apca-w3` 0.1.9, the candidate method for WCAG 3), Bronze targets: Lc 75 for body text, Lc 60 for large text and UI text, Lc 45 for non-text marks. APCA is polarity-aware, which is what sets dark mode's steps: light text on a dark ground needs L 0.85 or more for Lc 75, and a mark needs about L 0.7 for Lc 45. That is why dark text sits at step 300 (L 0.88) and dark marks at step 400 (L 0.76).
- The Stop control's label and the kill switch's label keep 7:1 in both themes (`STOP_CONTRAST`).
- **APCA is dev only and never ships.** `apca-w3` is published under its "Limited W3 License" (unmodified use for WCAG contrast checks of web content, kept current; AGPL-3.0 for anything else), and its dependency `colorparsley` is AGPL-3.0. Both are dev dependencies used only by the contrast tests, through `src/test/apca.ts`. Lint bans importing either, or that helper, from app code; `palette.test.ts` fails if any app file imports them or if `apca-w3` becomes a dependency; and `npm run build` runs `scripts/no-apca.mjs`, which fails if `.next/static` or `.next/server` holds APCA's constants or colorparsley's colour table. `/design` and `/palette` show the WCAG 2.2 ratios only.

### 6. Colour-vision deficiency

About 1 in 12 men of northern European descent has a red-green deficiency. Bloomberg estimates that at least 20,000 Terminal users have one, and it ships alternate schemes for deuteranopia and protanomaly (`PDFU COLORS`). Its research found that users with CVD keep the semantic associations: blues and greens read as up, and reds, oranges and yellows read as down ("Designing the Terminal for color accessibility", Bloomberg UX, 2021).

- Gain and loss always carry a sign and a word ("+$123.45 gain", "−$67.89 loss").
- **Colour-blind friendly** remaps gain and loss to alternates after Okabe and Ito's Color Universal Design palette. A gain is blue (hue 245) in both themes. A loss is raspberry (hue 350, near Okabe-Ito's reddish purple) in light mode and orange (hue 50) in dark mode. Each alternate has to stay apart from gold and from crimson as well as from the other:
  - In light mode a dark orange loss merges with crimson for deuteranopes, so the light loss is raspberry.
  - In dark mode a pale raspberry merges with the pale blue gain, so the dark loss is orange.
  The switch is `html[data-cvd="on"]` in `globals.css`; chart candles read the same attribute, since a canvas cannot read CSS.
- Verified by simulation: Machado, Oliveira and Fernandes (2009), *IEEE TVCG* 15(6), at full severity for deuteranopia and protanopia, measuring OKLab ΔE. Two things that must never be confused need ΔE ≥ 0.1 under each simulated vision, where about 0.02 is a just-noticeable difference.

### 7. What to avoid

- Neon, purple and violet (tested), glows, blends between colours, and pure black, white or grey (tested).
- A second yellow or gold: outside the gold tokens no token is warm (hue 60 to 110) above C 0.02, apart from the warning amber that no screen uses (tested).
- A gold block (see Gold usage rules).

## Ramps

Every ramp uses the same lightness curve and holds its hue constant:

| Step | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| L | 0.992 | 0.975 | 0.91 | 0.88 | 0.76 | 0.62 | 0.52 | 0.44 | 0.38 | 0.31 | 0.24 | 0.2 | 0.16 |

Chroma follows a hump (0.1, 0.25, 0.45, 0.7, 0.9, 1, 1, 0.92, 0.82, 0.62, 0.42, 0.3, 0.2 of the peak), clamped to 97% of the sRGB gamut at each step. Paper, ink and gold have their own chroma tables: gold fades faster than the hump at the dark end, so the dark mandate field is a quiet tint rather than a brown block.

| Ramp (hue) | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Paper (85), C | 0.003 | 0.004 | 0.006 | 0.006 | 0.006 | 0.007 | 0.007 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 |
| Paper hex | #fdfcfa | #f8f7f4 | #e3e1dd | #d9d7d3 | #b3b1ad | #888681 | #6b6964 | #54524e | #44423e | #32302c | #211f1b | #181612 | #0f0d09 |
| Ink (255), C | 0.003 | 0.004 | 0.006 | 0.007 | 0.008 | 0.009 | 0.01 | 0.01 | 0.01 | 0.01 | 0.01 | 0.008 | 0.008 |
| Ink hex | #fbfdfe | #f5f7f9 | #dfe1e5 | #d4d8dc | #aeb1b6 | #83868c | #65696f | #4f5358 | #3f4348 | #2d3135 | #1c2024 | #14161a | #0b0d11 |
| Gold (82), C | 0.007 | 0.023 | 0.065 | 0.1 | 0.126 | 0.123 | 0.103 | 0.087 | 0.075 | 0.045 | 0.03 | 0.022 | 0.015 |
| Gold hex | #fffcf7 | #fff6e6 | #f7deb1 | #f9d28a | #d9a948 | #ab7d13 | #86620d | #6a4d08 | #563e05 | #3c2e14 | #261e0e | #1b150a | #100d06 |
| Green and red, C | 0.003 | 0.011 | 0.045 | 0.062 | 0.135 | 0.15 | 0.138 | 0.117 | 0.101 | 0.082 | 0.063 | 0.045 | 0.03 |
| Green (150) hex | #fbfdfb | #f2f9f3 | #cdead2 | #bbe4c2 | #6cc982 | #2e9e52 | #0f7e3a | #09642d | #065123 | #043b18 | #02270e | #051c0a | #041107 |
| Red (12) hex | #fefcfc | #fef4f5 | #fed6d9 | #fec7cd | #fa8b9a | #d05a6e | #a94053 | #863141 | #6d2734 | #521b25 | #371017 | #270c11 | #18080a |
| Amber (70) hex | #fffcf9 | #fff5eb | #fedbb3 | #fece97 | #e7a045 | #b77610 | #905c0b | #724807 | #5d3a05 | #452902 | #2e1a01 | #221201 | #150b01 |
| Colour-blind blue (245) hex | #fbfdfe | #f0f8ff | #c9e5fe | #b7ddfe | #65b9fc | #138dda | #0e6eac | #085789 | #064670 | #033353 | #022139 | #01182a | #020e1a |
| Colour-blind raspberry (350) hex | #fffbfd | #fef3f8 | #fed4e6 | #fec4de | #f489be | #ca5794 | #a83876 | #89255e | #711a4c | #521537 | #350f24 | #250c19 | #17070f |
| Colour-blind orange (50) hex | #fffcfa | #fff4ef | #fed8c4 | #fdcbb0 | #f99357 | #ce6312 | #a34d0a | #823c07 | #6a2f04 | #4f2202 | #351501 | #270e02 | #180902 |
| Crimson (27), C | 0.003 | 0.011 | 0.045 | 0.061 | 0.138 | 0.2 | 0.2 | 0.173 | 0.15 | 0.122 | 0.084 | 0.06 | 0.04 |
| Crimson hex | #fefcfb | #fef4f3 | #fed7d1 | #fec9c2 | #fd8c81 | #e6443d | #c2181d | #9c0c12 | #80070d | #600407 | #3e0707 | #2c0806 | #1b0605 |

## Semantic tokens

Every token names a ramp step in each theme.

| Token | Light | Dark |
|---|---|---|
| `--background` | paper-100 #f8f7f4 | ink-975 #0b0d11 |
| `--card` | paper-50 #fdfcfa | ink-950 #14161a |
| `--muted` (quiet fields, the chart grid) | paper-200 #e3e1dd | ink-900 #1c2024 |
| `--border` | paper-200 #e3e1dd | ink-850 #2d3135 |
| `--foreground`, `--mandate-foreground` | ink-950 #14161a | paper-100 #f8f7f4 |
| `--muted-foreground`, `--mandate-muted`, `--info` | ink-800 #3f4348 | paper-300 #d9d7d3 |
| `--primary`, `--lapis`, `--ink` (the account fill, primary actions, Stop) | ink-950 #14161a | paper-100 #f8f7f4 |
| `--primary-foreground`, `--lapis-foreground`, `--ink-foreground` | paper-50 #fdfcfa | ink-950 #14161a |
| `--lapis-muted` (secondary text on the fill) | paper-200 #e3e1dd | ink-850 #2d3135 |
| `--lapis-strong` (pressed, and a tint inside the fill) | ink-800 #3f4348 | paper-300 #d9d7d3 |
| `--lapis-soft` (the current tab and range, an approval card, an account notice) | gold-100 #fff6e6 | gold-850 #3c2e14 |
| `--lapis-line` (the account's line, the current tab's bar) | gold-500 #ab7d13 | gold-400 #d9a948 |
| `--mandate` (the field) | gold-100 #fff6e6 | gold-850 #3c2e14 |
| `--mandate-soft` (a mandate notice) | gold-100 #fff6e6 | gold-900 #261e0e |
| `--mandate-strong` (headings, labels, the tag, the limit post, a level's axis label, the focus ring) | gold-700 #6a4d08 | gold-300 #f9d28a |
| `--mandate-marker` (rail fill, level marks) / `--mandate-edge` (the rule) | gold-500 #ab7d13 | gold-400 #d9a948 |
| `--selection` | gold-200 #f7deb1 | gold-800 #563e05 |
| `--ink-line` (a line inside ink, the paper hatch in dark) | ink-700 #4f5358 | paper-500 #888681 |
| `--crimson` / `--crimson-foreground` | crimson-700 #9c0c12 / paper-50 #fdfcfa | the same |
| `--crimson-edge` (the kill switch's 2px border) | crimson-700 #9c0c12 | crimson-400 #fd8c81 |
| `--gain` / `--loss` | green-700 #09642d / red-700 #863141 | green-300 #bbe4c2 / red-300 #fec7cd |
| `--warning` (on no screen) | amber-700 #724807 | amber-300 #fece97 |
| `--gain-soft` / `--loss-soft` / `--warning-soft` | the 100 steps | the 900 steps |
| `--info-soft` | paper-100 | ink-900 |
| `--gain-cvd` / `--gain-cvd-soft` | cvd-blue-700 #085789 / cvd-blue-100 | cvd-blue-300 #b7ddfe / cvd-blue-900 |
| `--loss-cvd` / `--loss-cvd-soft` | cvd-rose-800 #711a4c / cvd-rose-100 | cvd-orange-300 #fdcbb0 / cvd-orange-900 |
| `--hatch-ink` | ink-950 at 0.3: 1.96:1 against a card | paper-500 at 0.4: 1.82:1 against a card |
| `--logo` | the foreground: ink | the foreground: off-white |

## Gold usage rules

Gold is allowed only through the gold tokens, and the tests hold each rule:

- **Only the gold tokens are gold.** `mandate`, `mandate-soft`, `mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-soft`, `lapis-line` and `selection` are the only tokens on the gold ramp, in either theme. Outside them no token is a warm hue above C 0.02, apart from the warning amber that no screen uses (`palette.test.ts`).
- **Never gold text below gold-700 in light mode.** Every text pair whose colour is gold uses gold-700 or darker (`mandate-strong`), and the browser check finds no text set in a gold mark colour (`gold-500`) on any route. In dark mode gold text is gold-300, which reads on the dark ground.
- **Never a large block.** Saturated gold (`mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-line`) is never a background in a measured pair, never a Kumo surface, fill or tint in any scope, and is painted as a fill only by the envelope's rails, posts and ticks, the chart legend's swatch and the page header's current-tab bar. In the browser, `e2e/flat-fills.spec.ts` fails on any element or pseudo-element on any route, at desktop and phone widths and in both themes, painted in saturated gold and thicker than 8px on both sides. Every gold surface is a tint: L 0.9 or more with C at most 0.08 in light mode, and L 0.4 or less in dark.
- **Gold means the mandate or the account's line.** The account's gold is a line, a bar, or the pale pill of the current tab and range. On a chart, a mandate level is never gold: it is a dashed grey line whose axis label is gold.

## Kumo

Kumo components read their own roles (`--color-kumo-*`, `--text-color-kumo-*`), which Kumo sets in `@layer base`. `kumo-theme.css` points every one of them at a palette token, unlayered, so it wins. A test fails if Kumo adds a role we do not re-point, or if the roles section holds a raw colour. Because the roles point at tokens, dark mode needs only three role changes: on an inverted fill (the brand and contrast fills turn off-white), the inverse and inverted-badge text is `ink-foreground` rather than the now-dark card, and inactive text dims to `ink-line`.

| Kumo role | Token |
|---|---|
| Brand, link / brand hover | `lapis` / `lapis-strong` |
| Focus | `ring`, which is `mandate-strong` |
| Danger | `ink`. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning, warning tint, warning banner | `warning`, `warning-soft`. No component may use them (tested): at text lightness amber and dark gold are neighbours |
| Info, info tint, info banner | `info`, `info-soft` |
| Success, success tint | `gain`, `gain-soft` |
| Canvas, base, control, overlay / recessed / tint, fill | `card` / `background` / `muted` |
| Fill hover | `lapis-soft` |
| Lines | `border` |
| Badge orange | `mandate` with `mandate-strong` text, for mandate fields only |

A custom property resolves where it is declared and inherits as a value, so each `[data-surface]` scope re-declares the roles that change inside it:

| Scope | Base | Text (default / strong / subtle) | Tint | Lines |
|---|---|---|---|---|
| `account` (the sidebar's account block) | `lapis`: ink-950, dark paper-100 | `lapis-foreground` / the same / `lapis-muted` | `lapis-strong` | `lapis-line` (gold) |
| `field` (the mandate) | `mandate`: gold-100, dark gold-850 | `mandate-foreground` / `mandate-strong` / `mandate-muted` | as the root | `mandate-edge` (gold) |
| `ink` (the Stop control) | `ink`: ink-950, dark paper-100 | `ink-foreground` / the same / `lapis-muted` | as the root | `ink-line` |

## Charts

Lightweight Charts draws on a canvas, which cannot read CSS variables, so `src/components/charts/options.ts` converts both themes' tokens to hex once, from a table that names each token (`CHART_TOKEN`). A chart calls `setChartMode` before it draws and redraws when the mode changes, and a test fails if a chart colour and its token drift in either theme.

- The account is a solid 2px gold line (`lapis-line`) over a flat fill.
- A mandate level is a dashed grey line (`muted-foreground`) with a pale gold axis label in dark gold type.
- An account level (the average cost) is a solid gold line with an ink label.
- A proposal is a dashed ink line.
- The grid is `muted`.
- Candles are gain and loss, or the colour-blind alternates when `<html data-cvd="on">`.

Every fill stays one flat colour.

## Usage rules

- **60/30/10.** Paper (ink in dark) fills the page, cards and quiet fields. Ink (paper in dark) is the type, primary actions, the account block in the navigation and the Stop control. Gold is the mandate's rule, rails, marks and labels, and the account's line and current-place markers.
- **One meaning per colour.** Crimson is the kill switch alone: the kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. A test fails if any other product file uses it, and a render test fails if crimson paints anything else on any route in any scenario. Loss is text and markers only, never a fill and never crimson.
- **No colour without words.** Gains and losses carry a sign and a word. Modes carry a label and an icon. A restriction carries a tag naming who imposed it. A chart level carries its name on the axis and in the legend.
- **Never coloured:** system states (stale, unreachable, loading, errors), deadlines, Approve and Skip, provenance, and decoration.
- **Warning is reserved.** No screen uses it, and a test keeps it off every screen, including Kumo's `warning` and `alert` variants. When a warning is first needed, it gets an icon and the word "Warning" on its tint, outside the mandate panel.

## Contrast results

61 semantic pairs (47 text pairs at body targets, 14 non-text marks) and 42 Kumo role pairs across the four scopes, measured in each theme: 206 checks. All pass WCAG 2.2 AA and APCA Bronze (APCA measured in the tests). In the pair names, "the page" is `background` (wells and the sidebar) and "a card field" is `card` (the body, the header and cards); every reading colour is measured on both.

Lowest margins:

- **Light.** Secondary text on a quiet field (`muted-foreground` on `muted`): 7.66:1, Lc 75.6. A gain on the page: 6.84:1, Lc 80.1. The gold line and the mandate rule against the page: 3.43:1, Lc 59.6. The gold marks must not get lighter than gold-500.
- **Dark.** Secondary text on the account's pressed fill: 9.16:1, Lc 76.1. Dark gold text on the dark field: 9.15:1, Lc −77.3. The gold marker on the dark field: 6.09:1, Lc −54.7. The kill switch's edge on the dark sheet: 7.97:1, Lc −56.8.

| Pair | Light WCAG / Lc | Dark WCAG / Lc |
|---|---|---|
| Body text on the page | 16.84:1 / 100.0 | 18.06:1 / −102.3 |
| Body text on a card | 17.69:1 / 103.1 | 16.84:1 / −101.7 |
| Secondary text on a card | 9.78:1 / 91.4 | 12.61:1 / −81.5 |
| Primary action label | 17.69:1 / −105.0 | 16.84:1 / 100.0 |
| Text on the mandate field | 16.83:1 / 100.0 | 12.28:1 / −97.4 |
| Mandate label on its field | 7.29:1 / 82.2 | 9.15:1 / −77.3 |
| Stop control label on ink | 17.69:1 / −105.0 | 16.84:1 / 100.0 |
| Kill switch label on crimson | 8.31:1 / −89.9 | 8.31:1 / −89.9 |
| Kill switch edge on a card | 8.31:1 / 85.3 | 7.97:1 / −56.8 |
| Gain / loss on a card | 7.19:1 / 83.1, 8.09:1 / 86.2 | 12.89:1 / −83.2, 12.31:1 / −80.1 |
| Colour-blind gain / loss on a card | 7.54:1 / 84.4, 10.50:1 / 92.1 | 12.69:1 / −82.3, 12.40:1 / −80.4 |
| Gold line on a card | 3.61:1 / 62.6 | 8.34:1 / −59.0 |
| Mandate rule against the page | 3.43:1 / 59.6 | 8.95:1 / −59.6 |

The WCAG table, with a sample of every pair in each theme, is on `/palette`; the APCA values above come from the tests.

## Colour-vision results

OKLab ΔE under simulated deuteranopia (d) and protanopia (p); a required check needs 0.1 under both.

| Check | Light: required, d / p | Dark: required, d / p |
|---|---|---|
| Gain versus loss, colour-blind friendly | yes, 0.103 / 0.146 | yes, 0.114 / 0.104 |
| Gain versus loss, default | information, 0.019 / 0.107 | information, 0.011 / 0.055 |
| Colour-blind gain versus the kill switch | yes, 0.194 / 0.191 | yes, 0.446 / 0.563 |
| Colour-blind loss versus the kill switch | yes, 0.125 / 0.109 | yes, 0.431 / 0.509 |
| Colour-blind gain versus the mandate's gold marks | yes, 0.299 / 0.242 | yes, 0.212 / 0.242 |
| Colour-blind loss versus the mandate's gold marks | yes, 0.279 / 0.315 | yes, 0.135 / 0.147 |
| Colour-blind gain versus the account's gold line | yes, 0.299 / 0.242 | yes, 0.212 / 0.242 |
| Colour-blind loss versus the account's gold line | yes, 0.279 / 0.315 | yes, 0.135 / 0.147 |
| Colour-blind gain versus gold text | yes, 0.189 / 0.179 | yes, 0.159 / 0.153 |
| Colour-blind loss versus gold text | yes, 0.117 / 0.164 | information, 0.046 / 0.054 |
| Mode: running versus paused or stopped | yes, 0.712 / 0.708 | yes, 0.736 / 0.732 |
| Mode: exits-only ring on a card | yes, 0.793 / 0.789 | yes, 0.777 / 0.774 |
| Pause (ink) versus kill switch (crimson) | yes, 0.275 / 0.159 | yes, 0.529 / 0.633 |
| Account line versus a mandate level's line | yes, 0.282 / 0.246 | yes, 0.163 / 0.190 |
| Account marker versus mandate marks | yes, 0.449 / 0.411 | yes, 0.240 / 0.272 |
| Account fill versus mandate field | yes, 0.779 / 0.770 | yes, 0.664 / 0.676 |
| Mandate field edge versus the page | yes, 0.368 / 0.402 | yes, 0.628 / 0.589 |

Every required check passes in both themes. Default green and red at matched lightness merge under deuteranopia, which is why every result carries a sign and a word, and why the colour-blind friendly remap exists. One pair is information only, in dark mode: the orange colour-blind loss against gold text (0.046 / 0.054). In dark mode every readable colour is pale, and pale orange and pale gold meet under red-green deficiency; a raspberry loss would merge with the blue gain instead, which matters more. Gold text is always a labelled word ("Your mandate", a level's name) in a fixed place, and a loss always carries its minus sign.

## History

- **2026-09-28, the first palette chosen (DEC-202).** Three palettes were compared side by side in a dev panel, and the founder chose a blue brand with a warm metallic accent for the mandate. It shipped as the only palette, with this document's method: OKLCH ramps on one lightness curve, WCAG and APCA in the tests, and simulated colour vision.
- **2026-09-28, Ink and Gold (DEC-204).** With the consumer-grade redesign the founder previewed Ink and Gold, in light and dark, and approved it. The blue brand gave way to ink; the accent became gold; neutrals split into warm paper and cool ink; the ramps grew to thirteen steps so dark mode could reach APCA; the kill switch gained its dark-mode edge; and the colour-blind alternates were chosen per theme so they stay apart from gold and crimson. The method and the tests were kept and extended to both themes.
