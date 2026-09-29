# Owlhead colour: Ink and Volt

Owlhead's palette is Ink and Volt, in a light and a dark theme ([DEC-214](../docs/project/04-decision-log.md#decisions), the founder, 2026-09-29; it replaces [DEC-205](../docs/project/04-decision-log.md#decisions)'s ultramarine accent with volt, and the rest of DEC-205 and [DEC-204](../docs/project/04-decision-log.md#decisions) stands). Cool paper surfaces and ink type carry the product. Volt is the one accent: your mandate, and the account's line. Crimson is the kill switch and nothing else. In dark mode the roles swap ends of the same ramps: ink surfaces, paper type, and volt lighter. Neon volt appears only as the highlight, a flat fill that always carries ink type.

The key values, as ramp steps:

| Role | Light | Dark |
|---|---|---|
| The body and cards (`card`); the frame's glass is this at 72% | paper-50 #FBFDFE | ink-950 #14161A |
| Wells (`background`) | paper-100 #F5F7F9 | ink-975 #0B0D11 |
| Type, primary action, Stop | ink-950 #14161A | paper-100 #F5F7F9 |
| Secondary type (`muted-foreground`) | ink-800 #3F4348 | paper-300 #D5D8DB |
| Volt line and marker | volt-500 #7C9217 | volt-400 #A4C025 |
| Volt text and the focus ring (`mandate-strong`) | volt-700 #4C5A09 | volt-200 #D2F34A |
| Volt field (the mandate) | volt-100 #F2FCD7 | volt-850 #2C350E |
| Selected text | volt-200 #D2F34A | volt-800 #3D4814 |
| The highlight, always under ink type (`highlight`) | volt-300 #C8E928 | volt-300 #C8E928 |
| Colour-blind gain | cvd-teal-700 #0B5E65 | cvd-teal-300 #72EDFA |
| Kill switch fill / edge | crimson-700 #9C0C12 / the same | crimson-700 #9C0C12 / crimson-400 #FD8C81 |

The files:

- `src/lib/palette.ts` holds the ramps and both themes' semantic tokens. `globals.css` declares the same values in `:root` and in `html:root[data-mode="dark"]`, and `tokens.test.ts` fails if either block drifts.
- `src/lib/contrast-pairs.ts` holds the pairs, their targets and the colour-vision checks, and `src/lib/contrast.ts` measures them for either theme. `palette.test.ts` enforces everything this document claims, in both themes, including every Kumo role in every surface scope. APCA is measured only in the tests, through `src/test/apca.ts` (see Contrast).
- `/palette` (development only) shows the palette in use, the Kumo surfaces, the ramps, both themes' tokens, and the contrast and colour-vision tables for each theme. It is a `page.dev.tsx` route: it is not compiled into a production build and answers 404 outside development.
- Colour-blind friendly is a development preference until settings exist: `?cvd=1` or `?cvd=0` on any URL, Alt+Shift+C, or the checkbox in the scenario switcher. A production build renders the default gain and loss colours.

## Principles

### 1. No hue pushes

Red and other warm, saturated hues raise arousal and urgency. Mehta and Zhu (2009, *Science* 323) found that red primes avoidance and vigilance (and blue approach and calm); Bagchi and Cheema (2013, *Journal of Consumer Research* 39(5)) found that red backgrounds make people bid and haggle more aggressively. We do not want the screen to push an owner toward an aggressive decision. So most of the screen is near-neutral: cool paper and ink. The one accent, volt, is a yellow-green: olive as text, a quiet tint as a field, and neon only in the highlight, which is always a small fill under ink type. Red and warm, saturated colour is kept for the one control that must interrupt: the kill switch.

### 2. Proportion: 60/30/10

About 60% of the screen is paper (ink in dark mode), about 30% is type and ink actions, and about 10% is volt. Saturation pulls the eye, so it is spent only where attention is needed. "Your mandate" is on every agent all the time, so it has to be restrained rather than loud: a pale volt field under a volt rule, with volt markers and deep volt labels.

### 3. One meaning per colour

| Colour | Means | Never used for |
|---|---|---|
| Volt (`mandate-*`) | Your mandate: the envelope, limits, rails, the "Your mandate" tag, a mandate level's axis label | Anything the account or the platform imposed |
| Volt line (`lapis-line`) and ink fill (`lapis`) | The account: its equity line, primary actions, links, the current tab and range | Your mandate |
| Ink | A paused or stopped agent, and the Stop control | Decoration |
| Crimson | The kill switch, and nothing else | Errors, losses, warnings |

The account's token is still called `lapis`, so class names stay stable. Its fill is ink in light mode and paper in dark. Its line is volt, the same hue as the mandate but never beside a mandate mark without a name: on a chart the account is a solid volt line and a mandate level is a dashed grey line with a volt label.

The status family: gain is green; loss is red, as text and markers only, never crimson (at least 15 degrees of hue away) and never a fill; warning is amber, on no screen; info is the muted type. What never gets colour: system states (stale, unreachable, loading), deadlines, the Approve and Skip buttons, and anything decorative.

### 4. A system, not a set of picks

- **OKLCH** (Ottosson 2020, "A perceptual color space for image processing"; CSS Color 4). Equal steps in L look like equal steps in lightness, so a ramp is predictable and contrast can be designed rather than found.
- **Thirteen steps per hue (50 to 975)** at constant hue, on one lightness curve shared by every ramp. Chroma rises to a hump mid-ramp and is clamped to 97% of the sRGB gamut. Light mode reads from the top of each ramp and dark mode from the bottom; steps 850 and 975 exist for dark mode's borders and wells.
- **Neutrals** are two ramps at the same cool hue (255): paper, the cool white of the light page, and ink, both at C 0.003 to 0.01. Nothing is pure black, white or grey.
- **Gain and loss** share L and C at every step and differ only in hue, so neither is louder than the other.
- **Semantic tokens** map to ramp steps in each theme, and components use only the semantic tokens. A test fails on any raw colour value in `src/components/`.

### 5. Contrast: WCAG 2.2 AA is the floor, APCA Bronze is also required

- WCAG 2.2 (W3C Recommendation, 2023): 4.5:1 for body text (1.4.3), 3:1 for large text and for UI and non-text marks (1.4.11), and colour is never the only cue (1.4.1).
- APCA (Somers, `apca-w3` 0.1.9, the candidate method for WCAG 3), Bronze targets: Lc 75 for body text, Lc 60 for large text and UI text, Lc 45 for non-text marks. APCA is polarity-aware, which is what sets dark mode's steps: light text on a dark ground needs L 0.85 or more for Lc 75, and a mark needs about L 0.7 for Lc 45. That is why dark text sits at step 300 (L 0.88) and dark marks at step 400 (L 0.76).
- The Stop control's label, quiet (ink on the header) or loud (on ink), and the kill switch's label keep 7:1 in both themes (`STOP_CONTRAST`).
- **APCA is dev only and never ships.** `apca-w3` is published under its "Limited W3 License" (unmodified use for WCAG contrast checks of web content, kept current; AGPL-3.0 for anything else), and its dependency `colorparsley` is AGPL-3.0. Both are dev dependencies used only by the contrast tests, through `src/test/apca.ts`. Lint bans importing either, or that helper, from app code; `palette.test.ts` fails if any app file imports them or if `apca-w3` becomes a dependency; and `npm run build` runs `scripts/no-apca.mjs`, which fails if `.next/static` or `.next/server` holds APCA's constants or colorparsley's colour table. `/design` and `/palette` show the WCAG 2.2 ratios only.

### 6. Colour-vision deficiency

About 1 in 12 men of northern European descent has a red-green deficiency. Bloomberg estimates that at least 20,000 Terminal users have one, and it ships alternate schemes for deuteranopia and protanomaly (`PDFU COLORS`). Its research found that users with CVD keep the semantic associations: blues and greens read as up, and reds, oranges and yellows read as down ("Designing the Terminal for color accessibility", Bloomberg UX, 2021). Tritanopia, the blue-yellow deficiency, is rare, but a yellow-green accent sits on its axis, so it is simulated too.

- Gain and loss always carry a sign and a word ("+$123.45 gain", "−$67.89 loss").
- **Colour-blind friendly** remaps gain and loss to alternates after Okabe and Ito's Color Universal Design palette. A gain is teal (hue 205, between Okabe-Ito's sky blue and bluish green) in both themes. A loss is raspberry (hue 350, near Okabe-Ito's reddish purple) in light mode and orange (hue 50) in dark mode. Each alternate has to stay apart from volt and from crimson as well as from the other:
  - The gain was blue (hue 245) under gold. Ultramarine (DEC-205) sat 21 degrees away, and a blue gain merged with its text under red-green deficiency (ΔE 0.04), so the gain moved to teal. Volt (DEC-214), at hue 120, sits 85 degrees from the teal.
  - A teal gain and a raspberry loss at the same depth merge for deuteranopes, so the light loss is one step darker (cvd-rose-850).
  - In light mode a dark orange loss merges with crimson for deuteranopes, so the light loss is raspberry.
  - In dark mode a pale raspberry merges with the pale teal gain, so the dark loss is orange.
  The switch is `html[data-cvd="on"]` in `globals.css`; chart candles read the same attribute, since a canvas cannot read CSS.
- Verified by simulation: Machado, Oliveira and Fernandes (2009), *IEEE TVCG* 15(6), at full severity for deuteranopia, protanopia and tritanopia, measuring OKLab ΔE. Two things that must never be confused need ΔE ≥ 0.1 under each simulated vision, where about 0.02 is a just-noticeable difference. The accent's marks, gain, loss and crimson stay apart under all three.

### 7. What to avoid

- Neon, purple and violet (tested), glows, blends between colours, and pure black, white or grey (tested).
- A second volt: outside the volt tokens no token within 30 degrees of volt's hue is above C 0.02 (tested).
- A volt block (see Volt usage rules).

## Ramps

Every ramp uses the same lightness curve and holds its hue constant:

| Step | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| L | 0.992 | 0.975 | 0.91 | 0.88 | 0.76 | 0.62 | 0.52 | 0.44 | 0.38 | 0.31 | 0.24 | 0.2 | 0.16 |

Chroma follows a hump (0.1, 0.25, 0.45, 0.7, 0.9, 1, 1, 0.92, 0.82, 0.62, 0.42, 0.3, 0.2 of the peak), clamped to 97% of the sRGB gamut at each step. Paper, ink and volt have their own chroma tables. sRGB holds the most yellow-green high up the ramp, so volt peaks at 200 and 300 (C 0.19 and 0.2): that neon is the highlight and dark mode's volt text. It steps down to olive by 700 (C 0.1), the light theme's volt text, and fades fast from 800, so the dark selection and the dark mandate field are calm tints rather than blocks.

| Ramp (hue) | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Paper (255), C | 0.003 | 0.004 | 0.006 | 0.006 | 0.006 | 0.007 | 0.007 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 |
| Paper hex | #fbfdfe | #f5f7f9 | #dfe1e5 | #d5d8db | #aeb1b5 | #83868a | #66696d | #505357 | #404347 | #2e3134 | #1d1f23 | #14161a | #0b0d11 |
| Ink (255), C | 0.003 | 0.004 | 0.006 | 0.007 | 0.008 | 0.009 | 0.01 | 0.01 | 0.01 | 0.01 | 0.01 | 0.008 | 0.008 |
| Ink hex | #fbfdfe | #f5f7f9 | #dfe1e5 | #d4d8dc | #aeb1b6 | #83868c | #65696f | #4f5358 | #3f4348 | #2d3135 | #1c2024 | #14161a | #0b0d11 |
| Volt (120), C | 0.012 | 0.05 | 0.19 | 0.2 | 0.17 | 0.14 | 0.118 | 0.1 | 0.075 | 0.06 | 0.045 | 0.035 | 0.025 |
| Volt hex | #fbfef5 | #f2fcd7 | #d2f34a | #c8e928 | #a4c025 | #7c9217 | #61720e | #4c5a09 | #3d4814 | #2c350e | #1d2207 | #141805 | #0c0f04 |
| Green and red, C | 0.003 | 0.011 | 0.045 | 0.062 | 0.135 | 0.15 | 0.138 | 0.117 | 0.101 | 0.082 | 0.063 | 0.045 | 0.03 |
| Green (150) hex | #fbfdfb | #f2f9f3 | #cdead2 | #bbe4c2 | #6cc982 | #2e9e52 | #0f7e3a | #09642d | #065123 | #043b18 | #02270e | #051c0a | #041107 |
| Red (12) hex | #fefcfc | #fef4f5 | #fed6d9 | #fec7cd | #fa8b9a | #d05a6e | #a94053 | #863141 | #6d2734 | #521b25 | #371017 | #270c11 | #18080a |
| Amber (70) hex | #fffcf9 | #fff5eb | #fedbb3 | #fece97 | #e7a045 | #b77610 | #905c0b | #724807 | #5d3a05 | #452902 | #2e1a01 | #221201 | #150b01 |
| Colour-blind teal (205) hex | #f7feff | #e5fcfe | #a8f0f8 | #72edfa | #21c7d7 | #1697a3 | #0d7780 | #0b5e65 | #074c52 | #03383d | #022428 | #011a1d | #011013 |
| Colour-blind raspberry (350) hex | #fffbfd | #fef3f8 | #fed4e6 | #fec4de | #f489be | #ca5794 | #a83876 | #89255e | #711a4c | #521537 | #350f24 | #250c19 | #17070f |
| Colour-blind orange (50) hex | #fffcfa | #fff4ef | #fed8c4 | #fdcbb0 | #f99357 | #ce6312 | #a34d0a | #823c07 | #6a2f04 | #4f2202 | #351501 | #270e02 | #180902 |
| Crimson (27), C | 0.003 | 0.011 | 0.045 | 0.061 | 0.138 | 0.2 | 0.2 | 0.173 | 0.15 | 0.122 | 0.084 | 0.06 | 0.04 |
| Crimson hex | #fefcfb | #fef4f3 | #fed7d1 | #fec9c2 | #fd8c81 | #e6443d | #c2181d | #9c0c12 | #80070d | #600407 | #3e0707 | #2c0806 | #1b0605 |

## Semantic tokens

Every token names a ramp step in each theme.

| Token | Light | Dark |
|---|---|---|
| `--background` | paper-100 #f5f7f9 | ink-975 #0b0d11 |
| `--card` | paper-50 #fbfdfe | ink-950 #14161a |
| `--muted` (quiet fields, the chart grid) | paper-200 #dfe1e5 | ink-900 #1c2024 |
| `--border` | paper-200 #dfe1e5 | ink-850 #2d3135 |
| `--foreground`, `--mandate-foreground` | ink-950 #14161a | paper-100 #f5f7f9 |
| `--muted-foreground`, `--mandate-muted`, `--info` | ink-800 #3f4348 | paper-300 #d5d8db |
| `--primary`, `--lapis`, `--ink` (the account fill, primary actions, Stop) | ink-950 #14161a | paper-100 #f5f7f9 |
| `--primary-foreground`, `--lapis-foreground`, `--ink-foreground` | paper-50 #fbfdfe | ink-950 #14161a |
| `--lapis-muted` (secondary text on the fill) | paper-200 #dfe1e5 | ink-850 #2d3135 |
| `--lapis-strong` (pressed, and a tint inside the fill) | ink-800 #3f4348 | paper-300 #d5d8db |
| `--lapis-soft` (the current tab and range, an approval card, an account notice) | volt-100 #f2fcd7 | volt-850 #2c350e |
| `--lapis-line` (the account's line, the current tab's bar) | volt-500 #7c9217 | volt-400 #a4c025 |
| `--mandate` (the field) | volt-100 #f2fcd7 | volt-850 #2c350e |
| `--mandate-soft` (a mandate notice) | volt-100 #f2fcd7 | volt-900 #1d2207 |
| `--mandate-strong` (headings, labels, the tag, the limit post, a level's axis label, the focus ring) | volt-700 #4c5a09 | volt-200 #d2f34a |
| `--mandate-marker` (rail fill, level marks) / `--mandate-edge` (the rule) | volt-500 #7c9217 | volt-400 #a4c025 |
| `--selection` | volt-200 #d2f34a | volt-800 #3d4814 |
| `--highlight` / `--highlight-foreground` (a call to action, a hovered link) | volt-300 #c8e928 / ink-950 #14161a | the same |
| `--ink-line` (a line inside ink, the paper hatch in dark) | ink-700 #4f5358 | paper-500 #83868a |
| `--crimson` / `--crimson-foreground` | crimson-700 #9c0c12 / paper-50 #fbfdfe | the same |
| `--crimson-edge` (the kill switch's 2px border) | crimson-700 #9c0c12 | crimson-400 #fd8c81 |
| `--gain` / `--loss` | green-700 #09642d / red-700 #863141 | green-300 #bbe4c2 / red-300 #fec7cd |
| `--warning` (on no screen) | amber-700 #724807 | amber-300 #fece97 |
| `--gain-soft` / `--loss-soft` / `--warning-soft` | the 100 steps | the 900 steps |
| `--info-soft` | paper-100 | ink-900 |
| `--gain-cvd` / `--gain-cvd-soft` | cvd-teal-700 #0b5e65 / cvd-teal-100 | cvd-teal-300 #72edfa / cvd-teal-900 |
| `--loss-cvd` / `--loss-cvd-soft` | cvd-rose-850 #521537 / cvd-rose-100 | cvd-orange-300 #fdcbb0 / cvd-orange-900 |
| `--hatch-ink` | ink-950 at 0.3 | paper-500 at 0.4 |
| `--logo` | the foreground: ink | the foreground: off-white |

## Volt usage rules

Volt is allowed only through the volt tokens, and the tests hold each rule:

- **Only the volt tokens are volt.** `mandate`, `mandate-soft`, `mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-soft`, `lapis-line`, `selection` and `highlight` are the only tokens on the volt ramp, in either theme. Outside them no token within 30 degrees of its hue is above C 0.02 (`palette.test.ts`).
- **Never volt text below volt-700 in light mode.** Every text pair whose colour is volt uses volt-700 or darker (`mandate-strong`), and the browser check finds no text set in a volt mark colour (`volt-500`) on any route. In dark mode volt text is volt-200, which reads on the dark ground.
- **Never a large block.** Saturated volt (`mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-line`) is never a background in a measured pair, never a Kumo surface, fill or tint in any scope, and is painted as a fill only by the envelope's rails, posts and ticks, the chart legend's swatch and the page header's current-tab bar. In the browser, `e2e/flat-fills.spec.ts` fails on any element or pseudo-element on any route, at desktop and phone widths and in both themes, painted in saturated volt and thicker than 8px on both sides. Every volt surface is a tint: L 0.9 or more with C at most 0.08 in light mode, and L 0.4 or less with C at most 0.08 in dark.
- **The highlight is the one saturated fill.** Neon volt-300 (`highlight`) is the same in both themes, always carries ink type (`highlight-foreground`, 13.07:1), and is used for a call to action or a hovered link, never as a surface a screen sits on. Today only the landing page uses it.
- **Volt means the mandate or the account's line.** The account's volt is a line, a bar, or the pale pill of the current tab and range. On a chart, a mandate level is never volt: it is a dashed grey line whose axis label is volt.

## Kumo

Kumo components read their own roles (`--color-kumo-*`, `--text-color-kumo-*`), which Kumo sets in `@layer base`. `kumo-theme.css` points every one of them at a palette token, unlayered, so it wins. A test fails if Kumo adds a role we do not re-point, or if the roles section holds a raw colour. Because the roles point at tokens, dark mode needs only three role changes: on an inverted fill (the brand and contrast fills turn off-white), the inverse and inverted-badge text is `ink-foreground` rather than the now-dark card, and inactive text dims to `ink-line`.

| Kumo role | Token |
|---|---|
| Brand, link / brand hover | `lapis` / `lapis-strong` |
| Focus | `ring`, which is `mandate-strong` |
| Danger | `ink`. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning, warning tint, warning banner | `warning`, `warning-soft`. No component may use them (tested): at text lightness amber sits 20 degrees from the colour-blind orange loss and would read as a loss |
| Info, info tint, info banner | `info`, `info-soft` |
| Success, success tint | `gain`, `gain-soft` |
| Canvas, base, control, overlay / recessed / tint, fill | `card` / `background` / `muted` |
| Fill hover | `lapis-soft` |
| Lines | `border` |
| Badge orange | `mandate` with `mandate-strong` text, for mandate fields only |

A custom property resolves where it is declared and inherits as a value, so each `[data-surface]` scope re-declares the roles that change inside it:

| Scope | Base | Text (default / strong / subtle) | Tint | Lines |
|---|---|---|---|---|
| `account` (content on the account's ink) | `lapis`: ink-950, dark paper-100 | `lapis-foreground` / the same / `lapis-muted` | `lapis-strong` | `lapis-line` (volt) |
| `field` (the mandate) | `mandate`: volt-100, dark volt-850 | `mandate-foreground` / `mandate-strong` / `mandate-muted` | as the root | `mandate-edge` (volt) |
| `ink` (the loud Stop control) | `ink`: ink-950, dark paper-100 | `ink-foreground` / the same / `lapis-muted` | as the root | `ink-line` |

## Charts

Lightweight Charts draws on a canvas, which cannot read CSS variables, so `src/components/charts/options.ts` converts both themes' tokens to hex once, from a table that names each token (`CHART_TOKEN`). A chart calls `setChartMode` before it draws and redraws when the mode changes, and a test fails if a chart colour and its token drift in either theme.

- The account is a solid 2px volt line (`lapis-line`) over a flat fill.
- A mandate level is a dashed grey line (`muted-foreground`) with a pale volt axis label in deep volt type.
- An account level (the average cost) is a solid volt line with an ink label.
- A proposal is a dashed ink line.
- The grid is `muted`.
- Candles are gain and loss, or the colour-blind alternates when `<html data-cvd="on">`.

Every fill stays one flat colour.

## The frame's glass

The sticky header, the agent wire under it, the phone tab bar and the desktop dock are frosted glass ([DEC-208](../docs/project/04-decision-log.md#decisions)); every other surface stays flat. Two derived tokens, not ramp steps, carry it: `--glass`, the card mixed with transparency at 72% in both themes (in OKLab: in OKLCH, Chromium drops the hue when mixing with transparent, and the ink glass turns faintly pink), and `--glass-edge`, the type colour at 8%, for the hairline. The `glass` utility paints them over a backdrop filter of `blur(22px) saturate(1.8)`, and falls back to the solid card where the browser cannot blur, under `prefers-reduced-transparency: reduce`, and in forced colours. The desktop dock swaps in denser values, `--dock-glass` (the card at 85%) and `--dock-edge` (the type colour at 15%), and marks the current section with `--dock-current`, the type colour at 14%, and hover with `--dock-hover` at 7%: tints of the type colour, never solid ink (Stop's) or volt (the mandate's).

The blur only averages what scrolls underneath, so the worst case is a solid colour under the glass: ink (a primary action, the hero figure) in light mode, paper in dark. `tokens.test.ts` composites the glass over every token in each theme and requires 4.5:1 for body and muted text and 3:1 for the Stop pill against it:

| Over the darkest (light) or lightest (dark) token | Light, 72% | Dark, 72% |
|---|---|---|
| Body text, and the Stop pill against the glass | 9.51:1 | 6.75:1 |
| Muted text (breadcrumbs, icons) | 5.23:1 | 5.07:1 |
| The same muted text at 60% | 3.84:1, fails | 3.37:1, fails |

The paper badge, the command bar (the muted fill) and Stop keep their own solid fills, so their labels read as before. The agent wire is ink and muted text only, never a gain, loss or crimson colour, so the glass's own measure covers it.

## Usage rules

- **60/30/10.** Paper (ink in dark) fills the page, cards and quiet fields. Ink (paper in dark) is the type, primary actions, the account block in the navigation and the Stop control (an outline when quiet, a fill when loud, DEC-206). Volt is the mandate's rule, rails, marks and labels, and the account's line and current-place markers.
- **One meaning per colour.** Crimson is the kill switch alone: the kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. A test fails if any other product file uses it, and a render test fails if crimson paints anything else on any route in any scenario. Loss is text and markers only, never a fill and never crimson.
- **No colour without words.** Gains and losses carry a sign and a word. Modes carry a label and an icon. A restriction carries a tag naming who imposed it. A chart level carries its name on the axis and in the legend.
- **Never coloured:** system states (stale, unreachable, loading, errors), deadlines, Approve and Skip, provenance, and decoration.
- **Warning is reserved.** No screen uses it, and a test keeps it off every screen, including Kumo's `warning` and `alert` variants. When a warning is first needed, it gets an icon and the word "Warning" on its tint, outside the mandate panel.

## Contrast results

72 semantic pairs (54 text pairs at body targets, 18 non-text marks) and 42 Kumo role pairs across the four scopes, measured in each theme: 228 checks. All pass WCAG 2.2 AA and APCA Bronze (APCA measured in the tests). In the pair names, "the page" is `background` (wells) and "a card field" is `card` (the body and cards; the frame's glass is measured separately, above); every reading colour is measured on both.

Lowest margins:

- **Light.** Secondary text on a quiet field (`muted-foreground` on `muted`): 7.66:1, Lc 75.4. A gain on the page: 6.85:1, Lc 80.0. The volt line and the mandate rule against the page: 3.28:1, Lc 57.6. The volt marks must not get lighter than volt-500.
- **Dark.** Secondary text on the account's pressed fill: 9.17:1, Lc 76.4. Volt text on the dark field: 10.28:1, Lc −85.6. The volt marker on the dark field: 6.26:1, Lc −56.5. The kill switch's edge on the dark sheet: 7.97:1, Lc −56.8.

The focus ring is `mandate-strong`: at least 7.05:1 against the page, cards and the tinted fields in light and 10.28:1 in dark; on a quiet field (`muted`) it is 5.80:1 and 13.02:1. The same colour is the post that ends a phone headroom meter, on its `muted` track, where the ink fill measures 13.86:1 and 15.30:1.

| Pair | Light WCAG / Lc | Dark WCAG / Lc |
|---|---|---|
| Body text on the page | 16.84:1 / 99.9 | 18.06:1 / −102.2 |
| Body text on a card | 17.69:1 / 103.4 | 16.84:1 / −101.5 |
| Secondary text on a card | 9.78:1 / 91.7 | 12.61:1 / −81.8 |
| Primary action label | 17.69:1 / −105.4 | 16.84:1 / 99.9 |
| Text on the mandate field | 17.01:1 / 100.3 | 12.09:1 / −96.9 |
| Mandate label on its field | 7.12:1 / 81.7 | 10.28:1 / −85.6 |
| Mandate label (and focus ring) on a card | 7.41:1 / 84.8 | 14.33:1 / −90.2 |
| Ink type on the highlight | 13.07:1 / 83.9 | 13.07:1 / 83.9 |
| Stop control label on ink (loud) | 17.69:1 / −105.4 | 16.84:1 / 99.9 |
| Stop control label and outline on the header (quiet) | 17.69:1 / 103.4 | 16.84:1 / −101.5 |
| Kill switch label on crimson | 8.31:1 / −90.3 | 8.31:1 / −90.3 |
| Kill switch edge on a card | 8.31:1 / 85.7 | 7.97:1 / −56.8 |
| Gain / loss on a card | 7.19:1 / 83.5, 8.09:1 / 86.6 | 12.89:1 / −83.2, 12.31:1 / −80.1 |
| Colour-blind gain / loss on a card | 7.35:1 / 84.2, 13.47:1 / 98.3 | 13.09:1 / −84.3, 12.40:1 / −80.4 |
| Volt line on a card | 3.45:1 / 61.1 | 8.72:1 / −61.1 |
| Mandate rule against the page | 3.28:1 / 57.6 | 9.35:1 / −61.8 |

The WCAG table, with a sample of every pair in each theme, is on `/palette`; the APCA values above come from the tests.

## Colour-vision results

OKLab ΔE under simulated deuteranopia (d), protanopia (p) and tritanopia (t); a required check needs 0.1 under each vision it names, all three unless the table says otherwise.

| Check | Light: required, d / p / t | Dark: required, d / p / t |
|---|---|---|
| Gain versus loss, colour-blind friendly | yes, 0.127 / 0.198 / 0.226 | yes, 0.111 / 0.102 / 0.220 |
| Gain versus loss, default | information, 0.019 / 0.107 / 0.223 | information, 0.011 / 0.055 / 0.117 |
| Colour-blind gain versus the kill switch | yes, 0.127 / 0.146 / 0.267 | yes, 0.443 / 0.584 / 0.531 |
| Colour-blind loss versus the kill switch | yes, 0.174 / 0.122 / 0.186 | yes, 0.431 / 0.509 / 0.419 |
| Colour-blind gain versus the mandate's volt marks | yes, 0.243 / 0.220 / 0.188 | yes, 0.223 / 0.245 / 0.182 |
| Colour-blind loss versus the mandate's volt marks | yes, 0.338 / 0.394 / 0.332 | yes, 0.146 / 0.143 / 0.134 |
| Colour-blind gain versus the account's volt line | yes, 0.243 / 0.220 / 0.188 | yes, 0.223 / 0.245 / 0.182 |
| Colour-blind loss versus the account's volt line | yes, 0.338 / 0.394 / 0.332 | yes, 0.146 / 0.143 / 0.134 |
| Colour-blind gain versus volt text | yes under d and p, 0.118 / 0.114; information under t, 0.073 | information, 0.218 / 0.213 / 0.146 |
| Colour-blind loss versus volt text | yes, 0.161 / 0.217 / 0.170 | yes under d and p, 0.109 / 0.150; information under t, 0.094 |
| Mode: running versus paused or stopped | yes, 0.710 / 0.709 / 0.710 | yes, 0.735 / 0.732 / 0.734 |
| Mode: exits-only ring on a card | yes, 0.794 / 0.792 / 0.793 | yes, 0.776 / 0.775 / 0.776 |
| Pause (ink) versus kill switch (crimson) | yes, 0.275 / 0.159 / 0.335 | yes, 0.528 / 0.634 / 0.542 |
| Account line versus a mandate level's line | yes, 0.277 / 0.281 / 0.244 | yes, 0.185 / 0.200 / 0.120 |
| Account marker versus mandate marks | yes, 0.446 / 0.448 / 0.425 | yes, 0.253 / 0.264 / 0.213 |
| Account fill versus mandate field | yes, 0.778 / 0.775 / 0.776 | yes, 0.664 / 0.665 / 0.663 |
| Mandate field edge versus the page | yes, 0.368 / 0.373 / 0.352 | yes, 0.628 / 0.631 / 0.606 |

Every required check passes in both themes, and the accent's marks, gain, loss and crimson stay apart under all three visions. Default green and red at matched lightness merge under deuteranopia, which is why every result carries a sign and a word, and why the colour-blind friendly remap exists. Two pairs are information only where they are hardest to separate: the colour-blind gain against volt text under tritanopia in light mode, and the colour-blind loss against volt text under tritanopia in dark mode, where pale orange and pale volt sit a hair apart. Volt text is always a labelled word ("Your mandate", a level's name) in a fixed place, and a gain or loss always carries its sign.

## History

- **2026-09-28, the first palette chosen (DEC-202).** Three palettes were compared side by side in a dev panel, and the founder chose a blue brand with a warm metallic accent for the mandate. It shipped as the only palette, with this document's method: OKLCH ramps on one lightness curve, WCAG and APCA in the tests, and simulated colour vision.
- **2026-09-28, Ink and Gold (DEC-204).** With the consumer-grade redesign the founder previewed Ink and Gold, in light and dark, and approved it. The blue brand gave way to ink; the accent became gold; neutrals split into warm paper and cool ink; the ramps grew to thirteen steps so dark mode could reach APCA; the kill switch gained its dark-mode edge; and the colour-blind alternates were chosen per theme so they stay apart from gold and crimson. The method and the tests were kept and extended to both themes.
- **2026-09-29, Ink and Ultramarine (DEC-205).** The founder compared seven accents in a live preview (gold, iris, ultramarine, petrol, jade, plum and graphite), with page colours, and chose ultramarine on a cool-white page. The accent ramp moved to hue 266 with its own chroma table, and paper moved to hue 255, the same as ink. The colour-blind gain moved from blue to teal and the light-mode loss one step darker, so both stay apart from the new accent; tritanopia joined the simulated visions; and the reason warning stays off every screen became the orange loss rather than the gold. Everything else in DEC-204 stands.
- **2026-09-29, the frame's glass and the phone meter ([DEC-208](../docs/project/04-decision-log.md#decisions), [DEC-207](../docs/project/04-decision-log.md#decisions)).** The header, the wire, the phone tab bar and the dock became glass: two derived tokens, the card at 72% (85% for the dock) and the type colour at 8% (15%) for the edge, measured over every token in each theme with the combined palette. The dock marks the current section with a tint of the type colour, never ink or volt. The phone's headroom meter joined the pairs: its ink fill on the muted track, and its post in the deep accent, since the accent's marker measured 2.87:1 on the light track.
- **2026-09-29, Ink and Volt ([DEC-214](../docs/project/04-decision-log.md#decisions)).** The founder moved the accent from ultramarine to volt, across the app and the landing page. The accent ramp moved to hue 120 with its own chroma table: olive volt-700 as light mode's volt text and focus ring, volt-200 as dark mode's, and neon volt-300 as the highlight, a new token that is always a fill under ink type. Every other token kept its name, step and meaning, and every pair, Kumo role and colour-vision check was measured again in both themes and still passes. The volt marks against the light page dropped from 3.49:1 to 3.28:1, still above 3:1.
