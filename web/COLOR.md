# Owlhead colour: navy and brass

The founder asked: "Are we sure about colors? Read colour theory and make the colours appropriate for a serious company." Three palettes were built on the principles below and compared side by side (see History), and the founder chose P1, navy and brass ([DEC-202](../docs/project/04-decision-log.md#decisions)). It is now the only palette. DEC-202's six values are ramp steps here: navy #183D73 is navy-800, brass #AC7D1B brass-500, dark brass #634606 brass-700, the brass tint #FDF1DC brass-100, slate ink #181C21 slate-950, and off-white #F7FAFE slate-50, the card; the page is slate-100, so body text measures 16.35:1 on a card and 15.32:1 on the page. This document gives the basis, the ramps and tokens, how they map onto Kumo, and how they measure.

- `src/lib/palette.ts` holds the ramps and the semantic tokens; `globals.css` declares the same values, and `tokens.test.ts` fails if the two drift.
- `src/lib/contrast.ts` and `contrast-pairs.ts` hold the pairs and the measurements; `palette.test.ts` enforces everything this document claims, including every Kumo role in every surface scope.
- `/palette` (development only) shows the palette in use, the Kumo surfaces, the ramps, the tokens, the contrast table and the colour-vision simulations. It is a `page.dev.tsx` route: it is not compiled into a production build and answers 404 outside development.
- Colour-blind friendly is a development preference until settings exist: `?cvd=1` or `?cvd=0` on any URL, Alt+Shift+C, or the checkbox in the scenario switcher. A production build renders the default gain and loss colours.

## Principles

### 1. Hue carries meaning before words do

Blue is the hue people most associate with competence, reliability and trust, across cultures, and it lowers arousal where money is at stake. Red and other warm, saturated hues raise arousal and urgency, and yellow reads as caution. Financial brands that want to look serious anchor on a deep blue or a deep green.

- Labrecque and Milne (2011; *Journal of the Academy of Marketing Science* 40(5), 2012), "Exciting red and competent blue": blue raises perceived competence and trust; red signals excitement.
- Su, Cui and Walsh (2019), "Trustworthy blue or untrustworthy red: the influence of colors on trust", *Journal of Marketing Theory and Practice* 27(3): blue lifts trust in a brand and red lowers it.
- Mehta and Zhu (2009), *Science* 323: red primes avoidance and vigilance, and blue primes approach. Bagchi and Cheema (2013), *Journal of Consumer Research* 39(5): red backgrounds make people more aggressive in bidding and haggling. We do not want the screen to push an owner toward an aggressive decision.

So the brand is navy, and warm saturated colour is kept for the one thing that must interrupt: the kill switch.

### 2. Proportion: 60/30/10

About 60% of the screen is tinted neutral, about 30% is brand, and about 10% is accent. Saturation pulls the eye, so it is spent only where attention is needed. "Your mandate" is on every agent, all the time, so it has to be restrained rather than loud: a pale brass panel under a 4px brass rule, with brass markers and dark brass labels.

### 3. One meaning per colour

| Colour | Means | Never used for |
|---|---|---|
| Navy (the `lapis` token) | The account and the platform: the account board, the paper hatch, primary actions, links, focus, info | Your mandate, results |
| Brass | Your mandate: the envelope, limits, rails, levels on charts, the "Your mandate" tag | Anything the account or platform imposed |
| Ink | A paused or stopped agent, and the Stop control | Decoration |
| Crimson | The kill switch, and nothing else | Errors, losses, warnings |

Status family: gain and success are green; loss is red, as text and markers only and never crimson (at least 15 degrees of hue away, and never a fill); warning is amber (hue 70), not yellow; info is the brand blue. What never gets colour: system states (stale, unreachable, loading), deadlines, the Approve and Skip buttons, and anything decorative.

### 4. A system, not a set of picks

- **OKLCH** (Ottosson 2020, "A perceptual color space for image processing"; CSS Color 4). Equal steps in L look like equal steps in lightness, so a ramp is predictable and contrast can be designed rather than found.
- **Eleven steps per hue (50 to 950)** at constant hue, on one lightness curve shared by every ramp, with chroma rising to a hump mid-ramp and clamped to the sRGB gamut.
- **Neutrals** are slate, tinted toward the brand (hue 255, C 0.006 to 0.015): nothing is pure black, white or grey.
- **Status colours** sit at the same L and C at every step and differ only in hue, so no status is louder than another.
- **Semantic tokens** map to ramp steps, and components use only the semantic tokens. A test fails on any raw colour value in `src/components/`.

### 5. Contrast: WCAG 2.2 AA is the floor, APCA Bronze is also required

- WCAG 2.2 (W3C Recommendation, 2023): 4.5:1 for body text (1.4.3), 3:1 for large text and for UI and non-text marks (1.4.11), and colour is never the only cue (1.4.1).
- APCA (Somers, `apca-w3` 0.1.9, the candidate method for WCAG 3), Bronze targets: Lc 75 for body text, Lc 60 for large text and UI text, Lc 45 for non-text marks. APCA is polarity-aware and tracks perceived contrast on light backgrounds better than the WCAG 2 ratio, which overrates mid-tones.

### 6. Colour-vision deficiency

About 1 in 12 men of northern European descent has a red-green deficiency. Bloomberg estimates that at least 20,000 Terminal users have one, and it ships alternate schemes for deuteranopia and protanomaly (`PDFU COLORS`). Its research found that users with CVD keep the semantic associations: blues and greens read as up, and reds, oranges and yellows read as down ("Designing the Terminal for color accessibility", Bloomberg UX, 2021).

- Gain and loss always carry a sign and a word ("+$123.45 gain", "−$67.89 loss").
- **Colour-blind friendly** remaps gain and loss to blue and orange, after Okabe and Ito's Color Universal Design palette (blue about #0072B2, orange about #E69F00), both moved onto our text lightness (L 0.415), where orange becomes a burnt orange, `#6f3d16`. The switch is `html[data-cvd="on"]` in `globals.css`; chart candles read the same attribute, since a canvas cannot read CSS.
- Verified by simulation: Machado, Oliveira and Fernandes (2009), *IEEE TVCG* 15(6), at full severity for deuteranopia and protanopia, measuring OKLab ΔE. Two things that must never be confused need ΔE ≥ 0.1 under each simulated vision, where about 0.02 is a just-noticeable difference.

### 7. What to avoid

- Saturated blue with bright yellow: the Ukrainian and Swedish flags, and IKEA. Placard's first palette paired lapis with a saturated marigold (L 0.85, C 0.155, hue 84), which is exactly this. A test fails on any bright yellow (hue 85 to 115, L above 0.75, C above 0.08) or any token between hue 60 and 115 above C 0.13.
- Neon, purple and violet (tested), glows, blends between colours, and pure black, white or grey (tested).

## Ramps

Every ramp uses the same lightness curve and holds its hue constant:

| Step | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| L | 0.985 | 0.962 | 0.925 | 0.865 | 0.76 | 0.62 | 0.52 | 0.415 | 0.365 | 0.295 | 0.225 |

Chroma follows a hump (0.12, 0.25, 0.45, 0.7, 0.9, 1, 1, 0.92, 0.82, 0.7, 0.55 of the peak), clamped to 97% of the sRGB gamut at each step.

| Ramp (hue) | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Slate (255), C | 0.006 | 0.008 | 0.01 | 0.012 | 0.014 | 0.015 | 0.015 | 0.015 | 0.014 | 0.013 | 0.012 |
| Slate hex | #f7fafe | #eff3f8 | #e2e7ed | #cdd3db | #abb2ba | #80878f | #636a72 | #464c54 | #3a3f46 | #282d33 | #181c21 |
| Navy (258), C | 0.006 | 0.017 | 0.035 | 0.064 | 0.112 | 0.125 | 0.125 | 0.115 | 0.102 | 0.087 | 0.068 |
| Navy hex | #f8fafe | #ecf3fe | #d8e8fe | #b9d5fd | #84b3f8 | #5487d0 | #3868b0 | #1f4a89 | #183d73 | #0d2b57 | #051b3b |
| Brass (80), C | 0.013 | 0.03 | 0.054 | 0.083 | 0.108 | 0.12 | 0.104 | 0.083 | 0.073 | 0.059 | 0.045 |
| Brass hex | #fff9f1 | #fdf1dc | #fae3be | #f0cd94 | #d5a95d | #ac7d1b | #88610b | #634606 | #523904 | #3c2902 | #271901 |
| Status, C (all six hues) | 0.006 | 0.017 | 0.035 | 0.064 | 0.119 | 0.13 | 0.109 | 0.087 | 0.076 | 0.061 | 0.047 |
| Green (150) hex | #f7fbf8 | #ebf6ed | #d7edda | #b6dfbd | #76c688 | #429c5a | #327a46 | #225931 | #1c4928 | #12351c | #09220f |
| Red (12) hex | #fef8f9 | #feeeef | #fddde0 | #fac2c7 | #f2909d | #c76171 | #9d4b58 | #73353f | #602c34 | #461e24 | #2e1116 |
| Amber (70) hex | #fdf9f6 | #faf1e7 | #f6e3ce | #eecca5 | #e1a356 | #b77610 | #905c0b | #694205 | #573604 | #402603 | #291701 |
| Blue, info (258) hex | #f8fafe | #ecf3fe | #d8e8fe | #b9d5fd | #81b3fc | #5286d3 | #3f69a7 | #2c4c7a | #243e66 | #182d4b | #0d1c31 |
| Colour-blind blue (245) hex | #f7fbfe | #e9f4fd | #d4e9fd | #b0d8fb | #6cb9f8 | #348dcf | #276ea3 | #1a5078 | #154264 | #0d2f49 | #061e30 |
| Colour-blind orange (55) hex | #fdf9f6 | #fcf0e8 | #fae1d1 | #f5c8ab | #eb9c64 | #c16e2d | #985622 | #6f3d16 | #5c3211 | #43230a | #2c1504 |
| Crimson (27), C | 0.007 | 0.018 | 0.037 | 0.07 | 0.138 | 0.2 | 0.2 | 0.164 | 0.144 | 0.116 | 0.088 |
| Crimson hex | #fff8f7 | #ffeeec | #feded9 | #fec2ba | #fd8c81 | #e6443d | #c2181d | #900810 | #79060b | #590406 | #3b0203 |

## Semantic tokens

Every token names a ramp step. The account's token is still called `lapis`, so class names stay stable; its colour is navy-800.

| Token | Step | Hex |
|---|---|---|
| `--background` | slate-100 | #eff3f8 |
| `--card` | slate-50 | #f7fafe |
| `--muted` (quiet fields, the chart grid) | slate-200 | #e2e7ed |
| `--border` | slate-300 | #cdd3db |
| `--foreground`, `--ink`, `--mandate-foreground` | slate-950 | #181c21 |
| `--muted-foreground`, `--mandate-muted`, `--ink-line` | slate-700 | #464c54 |
| `--primary`, `--lapis` | navy-800 | #183d73 |
| `--primary-foreground`, `--lapis-foreground`, `--ink-foreground`, `--crimson-foreground` | slate-50 | #f7fafe |
| `--lapis-muted` / `--lapis-soft` | navy-200 / navy-100 | #d8e8fe / #ecf3fe |
| `--lapis-strong` (pressed, and a tint inside navy) / `--lapis-line` (a line inside navy) | navy-900 / navy-600 | #0d2b57 / #3868b0 |
| `--mandate` (the field) | brass-100 | #fdf1dc |
| `--mandate-strong` (headings, labels, the tag, the limit post, a chart level's axis label) | brass-700 | #634606 |
| `--mandate-marker` (rail fill, level marks, chart levels) / `--mandate-edge` (the 4px rule) | brass-500 | #ac7d1b |
| `--mandate-soft` (a mandate notice) | brass-50 | #fff9f1 |
| `--selection` | brass-200 | #fae3be |
| `--crimson` | crimson-700 | #900810 |
| `--gain` / `--loss` | green-700 / red-700 | #225931 / #73353f |
| `--warning` / `--info` | amber-700 / blue-700 | #694205 / #2c4c7a |
| `--gain-soft` / `--loss-soft` / `--warning-soft` / `--info-soft` | the 100 steps | #ebf6ed / #feeeef / #faf1e7 / #ecf3fe |
| `--gain-cvd` / `--gain-cvd-soft` | cvd-blue-700 / cvd-blue-100 | #1a5078 / #e9f4fd |
| `--loss-cvd` / `--loss-cvd-soft` | cvd-orange-700 / cvd-orange-100 | #6f3d16 / #fcf0e8 |
| `--hatch-ink` | navy-800 at 0.3 | 1.74:1 against a card |

## Kumo

Kumo components read their own roles (`--color-kumo-*`, `--text-color-kumo-*`), which Kumo sets in `@layer base`. `placard-kumo.css` points every one of them at a palette token, unlayered, so it wins; a test fails if Kumo adds a role we do not re-point, or if the roles section holds a raw colour.

| Kumo role | Token |
|---|---|
| Brand, link, focus / brand hover | `lapis` (navy-800) / `lapis-strong` (navy-900) |
| Danger | `ink`. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning, warning tint, warning banner | `warning` (amber-700), `warning-soft`. No component may use them (tested): at text lightness amber and dark brass are almost the same colour |
| Info, info tint, info banner | `info` (blue-700), `info-soft` |
| Success, success tint | `gain`, `gain-soft` |
| Canvas, base, control, overlay, recessed, tint, fill | `background`, `card`, `muted` (slate) |
| Lines | `border` (slate-300) |
| Badge orange | `mandate` with `mandate-strong` text, for mandate fields only |

A custom property resolves where it is declared and inherits as a value, so each `[data-surface]` scope re-declares the roles that change inside it:

| Scope | Base | Text (default / strong / subtle) | Tint | Lines |
|---|---|---|---|---|
| `navy` (the sidebar's account block) | navy-800 | slate-50 / slate-50 / navy-200 | navy-900 | navy-600 |
| `field` (the mandate) | brass-100 | slate-950 / brass-700 / slate-700 | as the root | brass-500 |
| `ink` (the Stop control) | slate-950 | slate-50 / slate-50 / navy-200 | as the root | slate-700 |

## Charts

Lightweight Charts draws on a canvas, which cannot read CSS variables, so `src/components/charts/options.ts` converts the tokens to hex once, from a table that names each token (`CHART_TOKEN`); a test fails if a chart colour and its token drift. The account's line is navy over a navy-100 fill; a mandate level is a brass-500 line with a brass-700 axis label in card text (8.37:1); a proposal is an ink dashed line; the grid is slate-200; candles are gain and loss, or the blue and orange pair when `<html data-cvd="on">`. Every fill stays one flat colour.

## Usage rules

- **60/30/10.** Neutrals fill the page, cards and quiet fields. Navy is the account board, the account block in the navigation, primary actions and links. Brass is the rule on top of the mandate panel, the rail fills, the level marks, the mandate labels and the chart levels, and nothing else.
- **One meaning per colour.** A colour never appears outside its meaning. Crimson is the kill switch alone: the kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. A test fails if any other product file uses it. Loss is text and markers only, never a fill and never crimson.
- **No colour without words.** Gains and losses carry a sign and a word. Modes carry a label and an icon. A restriction carries a tag naming who imposed it. A chart level carries its name on the axis and in the legend.
- **Never coloured:** system states (stale, unreachable, loading, errors), deadlines, Approve and Skip, provenance, and decoration.
- **Warning is reserved.** No screen uses it, and a test keeps it off every screen, including Kumo's `warning` and `alert` variants. When a warning is first needed, it gets an icon and the word "Warning" on its tint, outside the mandate panel.

## Contrast results

49 semantic pairs (40 text pairs at body targets, 9 non-text marks) and 42 Kumo role pairs across the four scopes. All 91 pass WCAG 2.2 AA and APCA Bronze.

Lowest margins: `muted-foreground` on `muted` (and Kumo subtle text on a tint) 6.92:1, Lc 75.2; the brass marker on its tint 3.30:1, Lc 56.6; the brass rule against the page 3.31:1, Lc 56.8; a brass chart level on a card 3.54:1, Lc 61.0. The brass marker must not get lighter.

| Pair | WCAG | APCA Lc |
|---|---|---|
| Body text on the page | 15.32:1 | 96.6 |
| Text on the account board (navy) | 10.29:1 | −95.0 |
| Secondary text on the account board | 8.62:1 | −82.6 |
| Text on a navy tint (sidebar hover and current) | 13.39:1 | −99.9 |
| Text on the mandate tint | 15.29:1 | 96.4 |
| Mandate label (dark brass) on its tint | 7.82:1 | 82.1 |
| A chart level's axis label (card on dark brass) | 8.37:1 | −90.6 |
| Stop control label on ink | 16.38:1 | −102.8 |
| Kill switch label on crimson | 9.01:1 | −90.6 |
| Gain / loss on a card | 7.95:1 / 8.67:1 | 85.1 / 87.3 |
| Success / loss / warning / info text on its tint | 7.48 / 8.06 / 7.88 / 7.78:1 | 81.2 / 82.5 / 82.3 / 82.0 |
| Colour-blind gain / loss on a card | 8.22:1 / 8.53:1 | 85.8 / 86.9 |

The full table, with a sample of every pair, is on `/palette`.

## Colour-vision results

OKLab ΔE under simulated deuteranopia (d) and protanopia (p); required checks need 0.1 under both.

| Check | Required | d / p |
|---|---|---|
| Gain versus loss, colour-blind friendly (blue and orange) | yes | 0.159 / 0.150 |
| Gain versus loss, default (green and red) | information | 0.014 / 0.079 |
| Mode: running versus paused or stopped | yes | 0.702 / 0.701 |
| Mode: exits-only ring on a card | yes | 0.760 / 0.758 |
| Pause (ink) versus kill switch (crimson) | yes | 0.227 / 0.117 |
| Account line versus mandate levels (every equity chart) | yes | 0.356 / 0.299 |
| Account board versus mandate field | yes | 0.627 / 0.586 |
| Mandate edge versus the page | yes | 0.356 / 0.395 |
| Mandate level versus an up candle, colour-blind friendly | yes | 0.301 / 0.246 |
| Mandate level versus a down candle, colour-blind friendly | information | 0.213 / 0.218 |
| Mandate label versus gain text | information | 0.044 / 0.049 (normal 0.097) |
| Mandate label versus info text | information | 0.170 / 0.164 |
| Mandate label versus warning text | information | 0.001 / 0.008 (normal 0.015) |
| Mandate label versus colour-blind loss | information | 0.008 / 0.021 (normal 0.038) |

Every required check passes. Default green and red at matched lightness merge under deuteranopia (0.014), which is why every result carries a sign and a word, and why the colour-blind friendly remap exists; with it on, blue and orange stay apart (0.159). The crimson kill switch darkens toward ink under protanopia but still clears the bar at 0.117, and each kill-switch button is labelled. The mandate's labels sit close to warning (hence warning stays off the screens) and to the colour-blind loss text; the mandate is always a labelled panel in a fixed place, so hue is not what tells them apart.

## History

- **2026-09-28, three palettes compared.** On `cursor/web-palette`@a11642e, over the earlier shadcn screens, a dev panel switched the whole app between P0 Placard (lapis and a saturated marigold mandate field, values picked by hand), P1 navy and brass, and P2 navy and teal (the same system with a teal-195 mandate accent). P0 missed APCA on six pairs and its mandate field had no edge against the page (1.49:1); P1 and P2 passed everything. The recommendation was P1: navy and brass are complements (178 degrees apart), so the mandate never reads as more of the brand; teal sat 0.062 from gain green in every vision and 0.082 from info blue; and navy and brass is the idiom of institutions, at low enough chroma to avoid the blue-and-yellow flag look.
- **2026-09-28, P1 chosen.** The founder picked P1. It was ported onto the Kumo screens and the charts as the only palette: P0, P2, the palette switcher, Alt+Shift+P and `?palette` are gone; the colour-blind friendly preference and `/palette` stay.
