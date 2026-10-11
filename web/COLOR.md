# Owlhead colour: Azure and Sun

Owlhead's palette is Azure and Sun, in a light and a dark theme ([DEC-217](../docs/project/decisions/DEC-217.md), the founder, by merging #363 on 2026-09-30, with #364's corrections). It replaces [DEC-214](../docs/project/04-decision-log.md#decisions)'s volt. Where neither replaced them, [DEC-205](../docs/project/04-decision-log.md#decisions) and [DEC-204](../docs/project/04-decision-log.md#decisions) stand.

- Cool paper surfaces and ink type carry the product.
- Azure is the brand: primary actions and links, your mandate, and the account's line.
- Sun is the one warm accent: the highlight, a flat fill that always carries ink type.
- Green and red are a gain and a loss.
- Crimson is the kill switch and nothing else.

In dark mode the roles swap ends of the same ramps: near-black ink surfaces, paper type, and a brighter azure.

The key values, as ramp steps:

| Role | Light | Dark |
|---|---|---|
| The body and cards (`card`); the frame's glass is this at 72% | paper-50 #FBFDFE | ink-950 #0F1113 |
| Wells and the page (`background`) | paper-100 #F5F7F9 | ink-975 #07080A |
| Type, the account fill, Stop (`foreground`, `lapis`, `ink`) | ink-950 #0F1113 | paper-100 #F5F7F9 |
| Secondary type (`muted-foreground`) | ink-800 #3F4347 | paper-300 #D5D8DB |
| Primary action and links (`primary`) | azure-800 #043E89 | azure-300 #C1D9FE |
| Azure line and marker (`mandate-marker`, `mandate-edge`, `lapis-line`) | azure-600 #0858BC | azure-400 #8BB9FD |
| Azure text and the focus ring (`mandate-strong`) | azure-800 #043E89 | azure-300 #C1D9FE |
| The mandate's field (`mandate`) | azure-200 #D0E3FE | ink-850 #292C2F |
| The account's pale field (`lapis-soft`): the current tab and range, an approval card | sun-100 #FFF6E1 | ink-850 #292C2F |
| Selected text (`selection`) | azure-200 #D0E3FE | azure-800 #043E89 |
| The highlight, always under ink type (`highlight`) | sun-300 #FED254 | sun-300 #FED254 |
| Gain / loss | green-600 #0B7133 / red-600 #983D24 | green-400 #78CE8C / red-400 #FD997E |
| Colour-blind gain | cvd-teal-700 #0B5E65 | cvd-teal-300 #72EDFA |
| Kill switch fill / edge | crimson-700 #9B0A28 / the same | crimson-700 #9B0A28 / crimson-400 #FD9696 |

The files:

- `src/lib/palette.ts` holds the ramps and both themes' semantic tokens. `globals.css` declares the same values in `:root` and in `html:root[data-mode="dark"]`, and `tokens.test.ts` fails if either block drifts. `src/lib/brand-palette.ts` takes the brand's colours from the same tokens.
- `src/lib/contrast-pairs.ts` holds the pairs, their targets and the colour-vision checks, and `src/lib/contrast.ts` measures them for either theme. `palette.test.ts` enforces the rules this document states, in both themes, including every Kumo role in every surface scope. APCA is measured only in the tests, through `src/test/apca.ts` (see Contrast).
- `/palette` (development only) shows the palette in use, the Kumo surfaces, the ramps, both themes' tokens, and the contrast and colour-vision tables for each theme. It is a `page.dev.tsx` route: it is not compiled into a production build and answers 404 outside development.
- Colour-blind friendly is a development preference until settings exist: `?cvd=1` or `?cvd=0` on any URL, Alt+Shift+C, or the checkbox in the scenario switcher. A production build renders the default gain and loss colours.

## Principles

### 1. No hue pushes

Red and other warm, saturated hues raise arousal and urgency. Mehta and Zhu (2009, *Science* 323) found that red primes avoidance and vigilance (and blue approach and calm). Bagchi and Cheema (2013, *Journal of Consumer Research* 39(5)) found that red backgrounds make people bid and haggle more aggressively. We do not want the screen to push an owner toward an aggressive decision.

- **Most of the screen is near-neutral:** cool paper and ink.
- **Azure is cool:** deep as text, vivid only in thin lines and marks, and a pale tint as a field.
- **Sun is the one warm accent.** It appears only as a small fill under ink type (the highlight: the current chart range, a call to action on the landing page) and, in light mode, as the account's pale field.
- **Red is a loss**, as text, a down candle, or a hero line on a range that fell.
- **Crimson, the most saturated red, is kept for the one control that must interrupt:** the kill switch.

### 2. Proportion: 60/30/10

About 60% of the screen is paper (ink in dark mode), about 30% is type and ink, and about 10% is azure and sun. Saturation pulls the eye, so it is spent only where attention is needed. "Your mandate" is on every agent all the time, so it has to be restrained rather than loud: a pale azure field (a charcoal one in dark), azure rules and markers, and deep azure labels.

### 3. One meaning per colour

| Colour | Means | Never used for |
|---|---|---|
| Azure (`mandate-*`) | Your mandate: the envelope, limits, rails, the "Your mandate" tag, a mandate level's axis label | Anything the account or the platform imposed |
| Azure (`primary`) | A primary action and a link | A surface |
| Azure line (`lapis-line`) and ink fill (`lapis`) | The account: its equity line, the current tab's bar | Your mandate |
| Sun (`highlight`, light `lapis-soft`) | The current range, a call to action, the account's pale field | Text or a line on paper (too light to read) |
| Series (`series-1` to `series-5`) | An instrument's share of the account, cash, and the owls' feathers | A gain or a loss |
| Green and red (`gain`, `loss`) | A gain and a loss, and the hero line's direction | Anything else |
| Ink | A paused or stopped agent, and the Stop control | Decoration |
| Crimson | The kill switch, and nothing else | Errors, losses, warnings |

The account's token is still called `lapis`, so class names stay stable. Its fill is ink in light mode and paper in dark. Its line is azure, the same hue as the mandate, but never beside a mandate mark without a name. On a chart the account is a solid azure line, and a mandate level is a dashed grey line with an azure label.

The status family:
- **Gain** is green and **loss** is red: text, candles and the hero line, never a fill. Red sits at hue 36, at least 15 degrees from crimson's 20 (tested).
- **Warning** is amber, on no screen.
- **Info** is the muted type.

What never gets colour: system states (stale, unreachable, loading), deadlines, the Approve and Skip buttons, and anything decorative.

**Asset series** ([DEC-217](../docs/project/decisions/DEC-217.md) items 3 and 5). On the dashboard's Assets section and its charts, each instrument keeps one series colour, chosen by its share of the account: `series-1` (azure) for the largest, then `series-2` (sun), `series-3` (teal, hue 192) and `series-4` (sky, hue 232). The agents' cash is `series-5` (the neutral grey), and what no agent manages is `muted` (`src/lib/holdings.ts`). Teal and sky come from azure's cool side, so a chart of holdings never borrows the gain or loss hue (tested). In dark, `series-1` is azure-200, a step off the mandate's rules, so a holdings bar never paints a block in the mandate's azure.

**The owls** (DEC-217 item 6). An agent's pixel owl takes its feathers from `series-1` to `series-4`, chosen by its ID, and never from gain or loss, so an owl never reads as a result.

### 4. A system, not a set of picks

- **OKLCH** (Ottosson 2020, "A perceptual color space for image processing"; CSS Color 4). Equal steps in L look like equal steps in lightness, so a ramp is predictable and contrast can be designed rather than found.
- **Thirteen steps per hue (50 to 975)** at constant hue, on one lightness curve shared by every ramp. Chroma rises to a hump mid-ramp and is clamped to 97% of the sRGB gamut. Light mode reads from the top of each ramp and dark mode from the bottom; steps 850 and 975 exist for dark mode's borders and the page.
- **Neutrals** are two ramps at the same cool hue (255): paper, the cool white of the light page, and ink, its dark steps nearly neutral (C 0.008 and less from 850), so dark mode reads black rather than blue. Nothing is pure black, white or grey (tested).
- **Azure, gain and loss form a near-triad:** each pair is 100 to 140 degrees apart (azure 258, green 150, red 36; tested). Sun (90) sits within 20 degrees of azure's complement (tested).
- **Gain and loss** share L and C at every step and differ only in hue, so neither is louder than the other.
- **Semantic tokens** map to ramp steps in each theme, and components use only the semantic tokens. A test fails on any raw colour value in `src/components/`.

### 5. Contrast: WCAG 2.2 AA is the floor, APCA Bronze is also required

- WCAG 2.2 (W3C Recommendation, 2023):
  - 4.5:1 for body text (1.4.3);
  - 3:1 for large text and for UI and non-text marks (1.4.11);
  - colour is never the only cue (1.4.1).
- APCA (Somers, `apca-w3` 0.1.9, the candidate method for WCAG 3), Bronze targets:
  - Lc 75 for body text;
  - Lc 60 for large text, UI text, and a signed gain or loss figure (the `figure` pair kind, DEC-217);
  - Lc 45 for non-text marks.

  APCA is polarity-aware, which is what sets dark mode's steps: light text on a dark ground needs a light step for Lc 75, and a mark needs a mid one for Lc 45. That is why dark text sits at step 300 (L 0.88) and dark marks at step 400 (L 0.78).
- **The figure kind.** A gain or a loss is a figure of weight 500 or more, with its sign and word beside it. It is held to WCAG 4.5:1 and APCA Lc 60.
- **7:1 labels.** The Stop control's label, quiet (ink on the header) or loud (on ink), and the kill switch's label keep 7:1 in both themes (`STOP_CONTRAST`).
- **APCA is dev only and never ships.**
  - **Licence.** `apca-w3` is published under its "Limited W3 License": unmodified use for WCAG contrast checks of web content, kept current, and AGPL-3.0 for anything else. Its dependency `colorparsley` is AGPL-3.0. Both are dev dependencies, used only by the contrast tests through `src/test/apca.ts`.
  - **Enforcement.** Lint bans importing either, or that helper, from app code. `palette.test.ts` fails if any app file imports them or if `apca-w3` becomes a dependency. `npm run build` runs `scripts/no-apca.mjs`, which fails if `.next/static` or `.next/server` holds APCA's constants or colorparsley's colour table.
  - **What the pages show.** `/design` and `/palette` show the WCAG 2.2 ratios only.

### 6. Colour-vision deficiency

About 1 in 12 men of northern European descent has a red-green deficiency. Bloomberg estimates that at least 20,000 Terminal users have one, and it ships alternate schemes for deuteranopia and protanomaly (`PDFU COLORS`). Its research found that users with CVD keep the semantic associations: blues and greens read as up, and reds, oranges and yellows read as down ("Designing the Terminal for color accessibility", Bloomberg UX, 2021). Tritanopia, the blue-yellow deficiency, is rare, but a blue accent is where it bites, so it is simulated too.

- Gain and loss always carry a sign and a word ("+$123.45 gain", "−$67.89 loss").
- **Colour-blind friendly** remaps gain and loss to alternates after Okabe and Ito's Color Universal Design palette.
  - A gain is teal (hue 205, between Okabe-Ito's sky blue and bluish green) in both themes.
  - A loss is raspberry (hue 350, near Okabe-Ito's reddish purple) in light mode and orange (hue 50) in dark mode.

  Each alternate has to stay apart from azure and from crimson as well as from the other:
  - A blue gain would merge with azure text under red-green deficiency, so the gain stays teal, at least 50 degrees from azure (tested).
  - A teal gain and a raspberry loss at the same depth merge for deuteranopes, so the light loss is darker (cvd-rose-850).
  - In light mode a dark orange loss merges with crimson for deuteranopes, so the light loss is raspberry.
  - In dark mode a pale raspberry merges with the pale teal gain, so the dark loss is orange.

  The switch is `html[data-cvd="on"]` in `globals.css`. Chart candles and the hero line read the same attribute, since a canvas cannot read CSS.
- **Verified by simulation.** The method is Machado, Oliveira and Fernandes (2009), *IEEE TVCG* 15(6), at full severity for deuteranopia, protanopia and tritanopia, measuring OKLab ΔE. Two things that must never be confused need ΔE ≥ 0.1 under each simulated vision, where about 0.02 is a just-noticeable difference.
  - The accent's marks, gain, loss and crimson stay apart under deuteranopia and protanopia.
  - They stay apart under tritanopia too, but for one pair: tritanopia merges teal and blue whatever their depth. So the colour-blind gain against the azure marks and line is required under red-green deficiency only (DEC-217).

### 7. What to avoid

- Gradients, glows, blends between colours, purple and violet (tested), and pure black, white or grey (tested).
- **A second blue.** Outside the azure tokens, no token within 20 degrees of azure's hue is above C 0.03 (tested).
- **An azure block** (see Azure and sun usage rules).

## Ramps

Every ramp uses the same lightness curve and holds its hue constant:

| Step | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| L | 0.992 | 0.975 | 0.91 | 0.88 | 0.78 | 0.62 | 0.48 | 0.44 | 0.38 | 0.29 | 0.215 | 0.175 | 0.135 |

**Chroma.**
- **The shared hump.** Chroma follows a hump (0.1, 0.25, 0.45, 0.7, 0.9, 1, 1, 0.92, 0.82, 0.62, 0.42, 0.3, 0.2 of the peak), clamped to 97% of the sRGB gamut at each step. The C values below are the clamped ones.
- **Paper, ink, azure and sun** have their own chroma tables.
- **Azure** is vivid from 500 to 700 (C 0.15 or more, tested), where it carries lines, links and labels. It is quiet at 100, 850 and 900 (C 0.08 or less, tested), where it is a field.
- **Sun** is a sunflower that sRGB makes vivid only when it is light, so it peaks around 300 and 400. Sun-300 is the highlight.
- **Gain and loss** take the same chroma at every step: the most the weaker of the two hues can show there.

| Ramp (hue) | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 850 | 900 | 950 | 975 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Paper (255), C | 0.003 | 0.004 | 0.006 | 0.006 | 0.006 | 0.007 | 0.007 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 | 0.008 |
| Paper hex | #fbfdfe | #f5f7f9 | #dfe1e5 | #d5d8db | #b5b8bb | #83868a | #5b5e62 | #505357 | #404347 | #292c2f | #171a1d | #0e1114 | #06080b |
| Ink (255), C | 0.003 | 0.004 | 0.006 | 0.007 | 0.008 | 0.009 | 0.01 | 0.01 | 0.009 | 0.008 | 0.007 | 0.006 | 0.005 |
| Ink hex | #fbfdfe | #f5f7f9 | #dfe1e5 | #d4d8dc | #b4b8bc | #83868c | #5a5e63 | #4f5358 | #3f4347 | #292c2f | #171a1d | #0f1113 | #07080a |
| Azure (258), C | 0.003 | 0.011 | 0.042 | 0.057 | 0.109 | 0.19 | 0.172 | 0.158 | 0.136 | 0.055 | 0.04 | 0.03 | 0.02 |
| Azure hex | #fbfcfe | #f2f7fe | #d0e3fe | #c1d9fe | #8bb9fd | #3082f6 | #0858bc | #064ea7 | #043e89 | #192c46 | #0d1a2c | #08111e | #040810 |
| Sun (90), C | 0.009 | 0.03 | 0.111 | 0.149 | 0.154 | 0.122 | 0.095 | 0.087 | 0.075 | 0.057 | 0.042 | 0.03 | 0.02 |
| Sun hex | #fffcf6 | #fff6e1 | #fede89 | #fed254 | #deb21c | #a38213 | #725a08 | #655007 | #524004 | #362a02 | #211801 | #161002 | #0b0802 |
| Teal (192) hex | #f5fffe | #dffefc | #b5eeeb | #8cebe7 | #43cfcb | #159996 | #0e6b69 | #095f5d | #064d4b | #033331 | #021e1e | #011414 | #000b0a |
| Sky (232) hex | #fafdff | #eff9fe | #c0e8fe | #aae0fe | #5fc4f5 | #1592c3 | #0d6689 | #085a7a | #054963 | #033043 | #011c29 | #01131d | #000a11 |
| Green and red, C | 0.003 | 0.012 | 0.046 | 0.063 | 0.127 | 0.165 | 0.128 | 0.117 | 0.101 | 0.077 | 0.057 | 0.046 | 0.036 |
| Green (150) hex | #fbfdfb | #f1f9f3 | #cdead2 | #bbe4c2 | #78ce8c | #15a04c | #0b7133 | #09642d | #065123 | #033515 | #01200a | #011605 | #000c02 |
| Red (36) hex | #fefcfb | #fff4f1 | #fed7cd | #fecabb | #fd997e | #d65938 | #983d24 | #87351f | #6e2a18 | #4a1a0d | #2f0e05 | #210803 | #140301 |
| Amber (70) hex | #fffcf9 | #fff5eb | #fedbb3 | #fece97 | #eea74c | #b77610 | #81520a | #724807 | #5d3a05 | #3e2503 | #261501 | #1a0d01 | #0f0600 |
| Colour-blind teal (205) hex | #f7feff | #e5fcfe | #a8f0f8 | #72edfa | #1fcede | #1697a3 | #0c6a73 | #0b5e65 | #074c52 | #023237 | #011e21 | #011416 | #000b0c |
| Colour-blind raspberry (350) hex | #fffbfd | #fef3f8 | #fed4e6 | #fec4de | #fb8fc4 | #ca5794 | #9b2b6a | #89255e | #711a4c | #4c0f32 | #2f091e | #1f0613 | #11040a |
| Colour-blind orange (50) hex | #fffcfa | #fff4ef | #fed8c4 | #fdcbb0 | #fd9c63 | #ce6312 | #924408 | #823c07 | #6a2f04 | #471e02 | #2d1001 | #1f0901 | #120501 |
| Crimson (20) hex | #fefcfb | #fef4f4 | #fed6d5 | #fec8c7 | #fd9696 | #e54151 | #ae0e2e | #9b0a28 | #7f061f | #560412 | #370108 | #250306 | #150203 |

## Semantic tokens

Every token names a ramp step in each theme.

| Token | Light | Dark |
|---|---|---|
| `--background` | paper-100 #f5f7f9 | ink-975 #07080a |
| `--card` | paper-50 #fbfdfe | ink-950 #0f1113 |
| `--muted` (quiet fields, the chart grid) | paper-200 #dfe1e5 | ink-900 #171a1d |
| `--border` | paper-200 #dfe1e5 | ink-850 #292c2f |
| `--foreground`, `--mandate-foreground` | ink-950 #0f1113 | paper-100 #f5f7f9 |
| `--muted-foreground`, `--mandate-muted`, `--info` | ink-800 #3f4347 | paper-300 #d5d8db |
| `--primary` (primary actions and links) | azure-800 #043e89 | azure-300 #c1d9fe |
| `--primary-foreground` | paper-50 #fbfdfe | ink-950 #0f1113 |
| `--lapis`, `--ink` (the account fill, Stop) | ink-950 #0f1113 | paper-100 #f5f7f9 |
| `--lapis-foreground`, `--ink-foreground` | paper-50 #fbfdfe | ink-950 #0f1113 |
| `--lapis-muted` (secondary text on the fill) | paper-200 #dfe1e5 | ink-850 #292c2f |
| `--lapis-strong` (pressed, and a tint inside the fill) | ink-800 #3f4347 | azure-200 #d0e3fe |
| `--lapis-soft` (the current tab and range, an approval card, an account notice) | sun-100 #fff6e1 | ink-850 #292c2f |
| `--lapis-line` (the account's line, the current tab's bar) | azure-600 #0858bc | azure-400 #8bb9fd |
| `--mandate` (the field) | azure-200 #d0e3fe | ink-850 #292c2f |
| `--mandate-soft` (a mandate notice) | azure-100 #f2f7fe | ink-900 #171a1d |
| `--mandate-strong` (headings, labels, the tag, the limit post, a level's axis label, the focus ring) | azure-800 #043e89 | azure-300 #c1d9fe |
| `--mandate-marker` (rail fill, level marks) / `--mandate-edge` (the rule) | azure-600 #0858bc | azure-400 #8bb9fd |
| `--selection` | azure-200 #d0e3fe | azure-800 #043e89 |
| `--highlight` / `--highlight-foreground` (the current chart range, a call to action) | sun-300 #fed254 / ink-950 #0f1113 | the same |
| `--ink-line` (a line inside ink, the paper hatch in dark) | ink-700 #4f5358 | paper-500 #83868a |
| `--crimson` / `--crimson-foreground` | crimson-700 #9b0a28 / paper-50 #fbfdfe | the same |
| `--crimson-edge` (the kill switch's 2px border) | crimson-700 #9b0a28 | crimson-400 #fd9696 |
| `--gain` / `--loss` | green-600 #0b7133 / red-600 #983d24 | green-400 #78ce8c / red-400 #fd997e |
| `--warning` (on no screen) | amber-700 #724807 | amber-300 #fece97 |
| `--gain-soft` / `--loss-soft` / `--warning-soft` | the 100 steps | the 950 steps (warning: amber-900) |
| `--info-soft` | paper-100 | ink-900 |
| `--gain-cvd` / `--gain-cvd-soft` | cvd-teal-700 #0b5e65 / cvd-teal-100 | cvd-teal-300 #72edfa / cvd-teal-900 |
| `--loss-cvd` / `--loss-cvd-soft` | cvd-rose-850 #4c0f32 / cvd-rose-100 | cvd-orange-300 #fdcbb0 / cvd-orange-900 |
| `--series-1` to `--series-5` | azure-500 #3082f6, sun-400 #deb21c, teal-500 #159996, sky-400 #5fc4f5, ink-500 #83868c | azure-200 #d0e3fe, sun-300 #fed254, teal-400 #43cfcb, sky-300 #aae0fe, paper-500 #83868a |
| `--hatch-ink` | ink-950 at 0.3 | paper-500 at 0.4 |
| `--ring` | `--mandate-strong` | the same |
| `--logo` | the foreground: ink | the foreground: off-white |
| `--tide` / `--tide-foreground` / `--tide-muted` (the landing page's third colour: one section, the opening's pixel sea, the flying owl's trim and the lock screen's night, never in the product) | teal-800 #064d4b / paper-50 #fbfdfe / teal-200 #b5eeeb | the same |
| `--tide-line` (the long page's pixel thread on the page and on cards) | teal-800 #064d4b | teal-400 #43cfcb |

## Azure and sun usage rules

Azure and sun are allowed only through their tokens, and the tests hold each rule:

- **Only the azure tokens are azure.** `primary`, `mandate`, `mandate-soft`, `mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-line`, `selection` and `series-1` may sit on the azure ramp, and in dark also `lapis-strong`. In dark, `mandate` and `mandate-soft` are charcoal ink instead.
- **Only the sun tokens are sun.** In light these are `highlight`, `lapis-soft` and `series-2`; in dark, `highlight` and `series-2` (`palette.test.ts`).
- **Never azure text lighter than azure-600 in light mode.**
  - Every text pair whose colour is azure uses azure-600 or darker (`palette.test.ts`). In practice it is azure-800: `primary` and `mandate-strong`.
  - The browser check finds no text set in an azure mark colour (`mandate-marker`, `mandate-edge`, `lapis-line`) on any route.
  - In dark mode azure text is azure-300, which reads on the dark ground.
- **Never a large block.** Saturated azure (`mandate-strong`, `mandate-marker`, `mandate-edge`, `lapis-line`):
  - is never a background in a measured pair;
  - is never a Kumo surface, fill or tint in any scope;
  - is painted as a fill only by the envelope's rails, posts and ticks, the chart legend's swatch and the page header's current-tab bar (`palette.test.ts`).

  In the browser, `e2e/flat-fills.spec.ts` fails on any element or pseudo-element painted in those tokens and thicker than 8px on both sides. It checks every route, at desktop and phone widths and in both themes. Its helper still carries the old accent's name (`voltMisuse`) but reads the tokens.

  Every mandate surface is a tint: L 0.9 or more with C at most 0.08 in light mode, and L 0.4 or less with C at most 0.08 in dark.
- **The highlight is the one saturated warm fill.**
  - Sun-300 (`highlight`) is the same in both themes and always carries ink type (`highlight-foreground`, 13.18:1).
  - It is used for the current range on a chart's range control and for a call to action on the landing page, never as a surface a screen sits on.
  - Sun is too light for a line or a label on paper, so it is never text.
- **Tide is the landing page's third colour, and it is spent sparingly** ([DEC-907](../docs/project/decisions/DEC-907.md)).
  - Ink and sun leave a third leg of the triad to choose. Sun's complement, violet, is banned (hue 280 to 330), and of the two remaining legs teal (192, the existing teal ramp) was chosen over rose (345), which sits too near crimson and the colour-blind loss.
  - Teal-800 is deep enough to carry paper type (9.49:1) and pale teal type (7.59:1), and it is the same in both themes, as the highlight is.
  - It fills exactly one section, the long page's part about asking you, and the pixel sea the opening's picture floats on ([DEC-908](../docs/project/decisions/DEC-908.md) item 6), whose crests and foam are `tide-muted` (7.59:1, as a mark). Beyond them, tide is the flying owl's trim (its facial rim, wing edges and chest marks), the dithered sea of the lock screen's pixel night, and the long page's pixel thread. `long-page.test.tsx` fails on a second tide section or on tide in any file outside the long page.
  - The thread is the one line in tide: `tide-line` on the page and on cards (9.03:1 or more in light, 9.94:1 or more in dark), `tide-muted` across the tide section, and `tide` across the sun part (6.74:1). It is decoration, so it is measured as a mark.
  - It is never in the product and never text on paper.
- **Azure means an action, the mandate, or the account's line.** The account's azure is a line, a bar, or the current tab's rule. On a chart, a mandate level is never azure: it is a dashed grey line whose axis label is a pale azure tag in deep azure type.

## Kumo

Kumo components read their own roles (`--color-kumo-*`, `--text-color-kumo-*`), which Kumo sets in `@layer base`. `kumo-theme.css` points every one of them at a palette token, unlayered, so it wins. A test fails if Kumo adds a role we do not re-point, or if the roles section holds a raw colour.

Because the roles point at tokens, dark mode needs only three role changes. On an inverted fill (the brand and contrast fills turn off-white), the inverse and inverted-badge text is `ink-foreground` rather than the now-dark card. Inactive text dims to `ink-line`.

| Kumo role | Token |
|---|---|
| Brand, link / brand hover | `lapis` / `lapis-strong` |
| Focus | `ring`, which is `mandate-strong` |
| Danger | `ink`. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning, warning tint, warning banner | `warning`, `warning-soft`. No component may use them (tested): amber sits within 25 degrees of the colour-blind orange loss and would read as a loss |
| Info, info tint, info banner | `info`, `info-soft` |
| Success, success tint | `gain`, `gain-soft` |
| Canvas, base, control, overlay / recessed / tint, fill | `card` / `background` / `muted` |
| Fill hover | `lapis-soft` |
| Lines | `border` |
| Badge orange | `mandate` with `mandate-strong` text, for mandate fields only |

A custom property resolves where it is declared and inherits as a value, so each `[data-surface]` scope re-declares the roles that change inside it:

| Scope | Base | Text (default / strong / subtle) | Tint | Lines |
|---|---|---|---|---|
| `account` (content on the account's ink) | `lapis`: ink-950, dark paper-100 | `lapis-foreground` / the same / `lapis-muted` | `lapis-strong` | `lapis-line` (azure) |
| `field` (the mandate) | `mandate`: azure-200, dark ink-850 | `mandate-foreground` / `mandate-strong` / `mandate-muted` | as the root | `mandate-edge` (azure) |
| `ink` (the loud Stop control) | `ink`: ink-950, dark paper-100 | `ink-foreground` / the same / `lapis-muted` | as the root | `ink-line` |

## Charts

Lightweight Charts draws on a canvas, which cannot read CSS variables, so `src/components/charts/options.ts` converts both themes' tokens to hex once, from a table that names each token (`CHART_TOKEN`). A chart calls `setChartMode` before it draws and redraws when the mode changes, and a test fails if a chart colour and its token drift in either theme.

- **A hero line follows its change** (DEC-217): green up, red down and ink when flat, or the colour-blind alternates. It is a smooth 3px line with nothing under it, read against a faint dotted rule (`muted-foreground`) at the range's opening value.
- **The account** elsewhere is a solid 2px azure line (`lapis-line`) over a flat `lapis-soft` fill.
- **A mandate level** is a dashed grey line (`muted-foreground`) with a pale azure axis label (`mandate`) in deep azure type (`mandate-strong`).
- **An account level** (the average cost) is a solid azure line with an ink label.
- **A proposal** is a dashed ink line.
- **The grid** is `muted`.
- **Candles** are gain and loss, or the colour-blind alternates when `<html data-cvd="on">`.

Every fill stays one flat colour.

## The frame's glass

The sticky header, the phone tab bar and the desktop dock are frosted glass ([DEC-208](../docs/project/04-decision-log.md#decisions)); every other surface stays flat. Two derived tokens, not ramp steps, carry it:
- `--glass`, the card mixed with transparency at 72% in both themes. It is mixed in OKLab: in OKLCH, Chromium drops the hue when mixing with transparent, and the ink glass turns faintly pink.
- `--glass-edge`, the type colour at 8%, for the hairline.

The `glass` utility paints them over a backdrop filter of `blur(22px) saturate(1.8)`. It falls back to the solid card where the browser cannot blur, under `prefers-reduced-transparency: reduce`, and in forced colours.

The desktop dock swaps in denser values: `--dock-glass` (the card at 85%) and `--dock-edge` (the type colour at 15%). It marks the current section with `--dock-current`, the type colour at 14%, and hover with `--dock-hover` at 7%. These are tints of the type colour, never solid ink (Stop's) or azure (the mandate's).

The blur only averages what scrolls underneath, so the worst case is a solid colour under the glass. `tokens.test.ts` composites the glass over every token in each theme and requires 4.5:1 for body and muted text and 3:1 for the Stop pill against it:

| Over the token that is worst for each | Light, 72% | Dark, 72% |
|---|---|---|
| Body text, and the Stop pill against the glass | 9.82:1 | 7.19:1 |
| Muted text (breadcrumbs, icons) | 5.18:1 | 5.40:1 |
| The same muted text at 60% | 3.75:1, fails | 3.53:1, fails |

The paper badge, the command bar (the muted fill) and Stop keep their own solid fills, so their labels read as before.

## Usage rules

- **60/30/10.**
  - Paper (ink in dark) fills the page, cards and quiet fields.
  - Ink (paper in dark) is the type, the account block in the navigation and the Stop control: an outline when quiet, a fill when loud (DEC-206).
  - Azure is the primary action and links, the mandate's rule, rails, marks and labels, and the account's line and current-tab bar.
  - Sun is the highlight.
- **One meaning per colour.**
  - Crimson is the kill switch alone: the kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. A test fails if any other product file uses it, and a render test fails if crimson paints anything else on any route in any scenario.
  - Loss is text, candles and a hero line, never a fill and never crimson.
- **No colour without words.**
  - Gains and losses carry a sign and a word.
  - Modes carry a label and a mark.
  - A restriction carries a tag naming who imposed it.
  - A chart level carries its name on the axis and in the legend.
- **Never coloured:** system states (stale, unreachable, loading, errors), deadlines, Approve and Skip, provenance, and decoration.
- **Warning is reserved.** No screen uses it, and a test keeps it off every screen, including Kumo's `warning` and `alert` variants. When a warning is first needed, it gets an icon and the word "Warning" on its tint, outside the mandate panel.

## Contrast results

The checks, measured in each theme:
- 80 semantic pairs: 57 text pairs (body and figure targets) and 23 non-text marks;
- 42 Kumo role pairs across the four scopes;
- 244 checks in all, measured with WCAG 2.2 and APCA (APCA in the tests).

`palette.test.ts` requires every one to pass WCAG 2.2 AA and APCA Bronze. In the pair names, "the page" is `background` and "a card" is `card`; the frame's glass is measured separately, above. Every reading colour is measured on both.

Lowest margins:

- **Light.**
  - Secondary text on a quiet field (`muted-foreground` on `muted`): 7.66:1, Lc 75.5.
  - Secondary text and azure labels on the mandate field: 7.67:1, Lc 75.7, and 7.82:1, Lc 75.6.
  - A gain on the page: 5.73:1, Lc 75.1, against the figure target of Lc 60.
  - The azure marker on the mandate field: 5.14:1, Lc 64.8.
- **Dark.**
  - A loss on its tint: 9.12:1, Lc −61.0, against the figure target of Lc 60. Dark gains and losses are the figures nearest their floor.
  - Azure labels and secondary text on the charcoal fields: 9.84:1, Lc −78.3, and 9.83:1, Lc −78.5.
  - The azure marker on the mandate field and the account's line on its pale field: 7.04:1, Lc −59.3.
  - The kill switch's edge on a card: 9.00:1, Lc −60.9.

**The focus ring** is `mandate-strong`. Against the page, cards and the tinted fields it is at least 7.82:1 in light and 9.84:1 in dark; on a quiet field (`muted`) it is 7.81:1 and 12.21:1. The same colour is the post that ends a phone headroom meter, on its `muted` track, where the ink fill measures 14.52:1 and 16.30:1.

| Pair | Light WCAG / Lc | Dark WCAG / Lc |
|---|---|---|
| Body text on the page | 17.65:1 / 100.4 | 18.62:1 / −102.3 |
| Body text on a card | 18.54:1 / 104.0 | 17.65:1 / −101.9 |
| Secondary text on a card | 9.78:1 / 91.8 | 13.22:1 / −82.2 |
| Primary action label | 9.97:1 / −95.6 | 13.23:1 / 82.2 |
| Links on a card | 9.97:1 / 91.6 | 13.23:1 / −82.0 |
| Text on the mandate field | 14.53:1 / 87.9 | 13.13:1 / −98.3 |
| Mandate label on its field | 7.82:1 / 75.6 | 9.84:1 / −78.3 |
| Mandate label (and focus ring) on a card | 9.97:1 / 91.6 | 13.23:1 / −82.0 |
| Ink type on the highlight | 13.18:1 / 82.1 | 13.18:1 / 82.1 |
| Paper type / pale teal type on tide | 9.49:1 / −94.7, 7.59:1 / −78.3 | the same |
| The thread on the page / on a card | 9.03:1 / 87.1, 9.49:1 / 90.6 | 10.49:1 / −66.5, 9.94:1 / −66.1 |
| Tide on the highlight (the thread and the owl's trim on sun) | 6.74:1 / 68.7 | the same |
| Stop control label on ink (loud) | 18.54:1 / −105.8 | 17.65:1 / 100.4 |
| Stop control label and outline on the header (quiet) | 18.54:1 / 104.0 | 17.65:1 / −101.9 |
| Kill switch label on crimson | 8.32:1 / −90.3 | 8.32:1 / −90.3 |
| Kill switch edge on a card | 8.32:1 / 85.8 | 9.00:1 / −60.9 |
| Gain / loss on a card | 6.02:1 / 78.7, 6.79:1 / 81.9 | 9.95:1 / −65.8, 9.05:1 / −61.1 |
| Colour-blind gain / loss on a card | 7.35:1 / 84.2, 14.41:1 / 99.5 | 13.71:1 / −84.7, 12.99:1 / −80.8 |
| Azure line on a card | 6.55:1 / 80.9 | 9.46:1 / −63.0 |
| Mandate rule against the page | 6.24:1 / 77.4 | 9.99:1 / −63.4 |

The WCAG table, with a sample of every pair in each theme, is on `/palette`; the APCA values above come from the same measure the tests use (`apca-w3` 0.1.9).

## Colour-vision results

The table gives OKLab ΔE under simulated deuteranopia (d), protanopia (p) and tritanopia (t). A required check needs 0.1 under each vision it names, all three unless the table says otherwise.

| Check | Light: required, d / p / t | Dark: required, d / p / t |
|---|---|---|
| Gain versus loss, colour-blind friendly | yes, 0.146 / 0.219 / 0.239 | yes, 0.111 / 0.102 / 0.220 |
| Gain versus loss, default | information, 0.028 / 0.091 / 0.247 | information, 0.029 / 0.085 / 0.238 |
| Colour-blind gain versus the kill switch | yes, 0.108 / 0.130 / 0.266 | yes, 0.439 / 0.578 / 0.532 |
| Colour-blind loss versus the kill switch | yes, 0.180 / 0.118 / 0.200 | yes, 0.431 / 0.507 / 0.421 |
| Colour-blind gain versus the mandate's azure marks | yes under d and p, 0.145 / 0.150; information under t, 0.062 | yes under d and p, 0.120 / 0.139; information under t, 0.119 |
| Colour-blind loss versus the mandate's azure marks | yes, 0.243 / 0.289 / 0.282 | yes, 0.203 / 0.154 / 0.194 |
| Colour-blind gain versus the account's azure line | yes under d and p, 0.145 / 0.150; information under t, 0.062 | yes under d and p, 0.120 / 0.139; information under t, 0.119 |
| Colour-blind loss versus the account's azure line | yes, 0.243 / 0.289 / 0.282 | yes, 0.203 / 0.154 / 0.194 |
| Colour-blind gain versus azure text | yes under d and p, 0.123 / 0.128; information under t, 0.058 | information, 0.004 / 0.037 / 0.102 |
| Colour-blind loss versus azure text | yes, 0.152 / 0.180 / 0.199 | yes under d and p, 0.114 / 0.105; information under t, 0.119 |
| Mode: running versus paused or stopped | yes, 0.733 / 0.733 / 0.733 | yes, 0.760 / 0.758 / 0.760 |
| Mode: exits-only ring on a card | yes, 0.817 / 0.816 / 0.817 | yes, 0.799 / 0.798 / 0.799 |
| Pause (ink) versus kill switch (crimson) | yes, 0.289 / 0.173 / 0.350 | yes, 0.526 / 0.630 / 0.543 |
| Account line versus a mandate level's line | yes, 0.190 / 0.200 / 0.146 | yes, 0.154 / 0.125 / 0.142 |
| Account marker versus mandate marks | yes, 0.340 / 0.367 / 0.337 | yes, 0.233 / 0.200 / 0.223 |
| Account fill versus mandate field | yes, 0.731 / 0.742 / 0.733 | yes, 0.684 / 0.683 / 0.684 |
| Mandate field edge versus the page | yes, 0.533 / 0.495 / 0.479 | yes, 0.644 / 0.673 / 0.646 |

Every required check passes in both themes.
- **Default green and red** at matched lightness merge under deuteranopia. That is why every result carries a sign and a word, and why the colour-blind friendly remap exists.
- **Information-only pairs.** Some pairs are information only where blue and teal meet:
  - the colour-blind gain against the azure marks and line under tritanopia;
  - the colour-blind gain against azure text, under tritanopia in light and in every vision in dark, where both are pale;
  - the colour-blind loss against azure text under tritanopia in dark.

  Azure text is always a labelled word ("Your mandate", a level's name) in a fixed place, the azure marks are labelled and in fixed places, and a gain or loss always carries its sign.

## History

- **2026-09-28, the first palette chosen (DEC-202).** Three palettes were compared side by side in a dev panel, and the founder chose a blue brand with a warm metallic accent for the mandate. It shipped as the only palette, with this document's method: OKLCH ramps on one lightness curve, WCAG and APCA in the tests, and simulated colour vision.
- **2026-09-28, Ink and Gold (DEC-204).** With the consumer-grade redesign the founder previewed Ink and Gold, in light and dark, and approved it. The blue brand gave way to ink; the accent became gold; neutrals split into warm paper and cool ink; the ramps grew to thirteen steps so dark mode could reach APCA; the kill switch gained its dark-mode edge; and the colour-blind alternates were chosen per theme so they stay apart from gold and crimson. The method and the tests were kept and extended to both themes.
- **2026-09-29, Ink and Ultramarine (DEC-205).** The founder compared seven accents in a live preview (gold, iris, ultramarine, petrol, jade, plum and graphite), with page colours, and chose ultramarine on a cool-white page. The accent ramp moved to hue 266 with its own chroma table, and paper moved to hue 255, the same as ink. The colour-blind gain moved from blue to teal and the light-mode loss one step darker, so both stay apart from the new accent; tritanopia joined the simulated visions; and the reason warning stays off every screen became the orange loss rather than the gold. Everything else in DEC-204 stands.
- **2026-09-29, the frame's glass and the phone meter ([DEC-208](../docs/project/04-decision-log.md#decisions), [DEC-207](../docs/project/04-decision-log.md#decisions)).** The header, the wire, the phone tab bar and the dock became glass: two derived tokens, the card at 72% (85% for the dock) and the type colour at 8% (15%) for the edge, measured over every token in each theme with the combined palette. The dock marks the current section with a tint of the type colour, never ink or volt. The phone's headroom meter joined the pairs: its ink fill on the muted track, and its post in the deep accent, since the accent's marker measured 2.87:1 on the light track.
- **2026-09-29, Ink and Volt ([DEC-214](../docs/project/04-decision-log.md#decisions)).** The founder moved the accent from ultramarine to volt, across the app and the landing page. The accent ramp moved to hue 120 with its own chroma table: olive volt-700 as light mode's volt text and focus ring, volt-200 as dark mode's, and neon volt-300 as the highlight, a new token that is always a fill under ink type. Every other token kept its name, step and meaning, and every pair, Kumo role and colour-vision check was measured again in both themes and still passes. The volt marks against the light page dropped from 3.49:1 to 3.28:1, still above 3:1.
- **2026-09-30, Azure and Sun ([DEC-217](../docs/project/decisions/DEC-217.md)).** The founder merged #363, which replaced volt with Azure and Sun across the app and the landing page; #364 corrected two of its tokens the next pass found. The decision was recorded on 2026-10-09.
  - **The accent.** Azure (hue 258) became the brand: the primary action and links (azure-800 in light after #364, azure-300 in dark), the mandate's rules and marks, and the account's line. Sun (hue 90) became the highlight, and in light mode the account's pale field.
  - **The rest of the palette.**
    - Five asset series were added.
    - The lightness curve moved at six steps.
    - The red loss moved to hue 36 and crimson to hue 20.
    - Gain and loss moved to step 600 in light and 400 in dark.
    - Dark mode turned near black (ink-950 #0F1113, the page ink-975 #07080A), with the mandate's and the account's fields a charcoal ink-850.
  - **Colour vision.** The teal gain against the azure marks is required under red-green deficiency only, since tritanopia merges teal and blue at any depth.
  - **The figure kind.** A gain or loss figure became its own pair kind, held to APCA Lc 60.
  - **#364's corrections.** In light, `primary` moved from azure-600 to azure-800, so no primary text sits in the mark colour. In dark, `series-1` moved from azure-400 to azure-200, so a holdings bar never paints the mandate's azure.
- **2026-10-10, tide ([DEC-907](../docs/project/decisions/DEC-907.md)).** The founder asked for a third colour beside ink and sun, chosen by colour theory, and to use it sparingly. Tide is teal-800 with paper and teal-200 type, the same in both themes, and only the long page under the landing page's desktop uses it: one section and the flying owl. Later that day the founder asked for something pixelated or threaded in contrasting teal: `tide-line` (teal-800 in light, teal-400 in dark) became the long page's pixel thread, and tide became the owl's trim and the sea of the lock screen's pixel night.
