---
name: Owlhead
description: The design system of the Owlhead web app. Paper trading only.
direction: Calm (DEC-204)
---

# Owlhead design system

The Owlhead web app is calm and consumer-grade: the polish of Robinhood, Public or Wealthfront, with none of their gamification ([DEC-204](../docs/project/04-decision-log.md#decisions), the founder, 2026-09-28). It replaces Placard, the transit-signage direction, and keeps its palette (navy and brass, [DEC-202](../docs/project/04-decision-log.md#decisions)), the founder's logo ([DEC-203](../docs/project/04-decision-log.md#decisions)) and every safety behaviour. The product name is Owlhead (DEC-201); "mandate" is the product's word for the owner's binding envelope. The [product-experience brief](../docs/product/09-product-experience.md) binds: where this file and the brief disagree, the brief wins.

Every value below is shipped. `src/lib/palette.ts` holds the colour ramps and tokens (the reasoning is in [COLOR.md](COLOR.md)), `src/app/globals.css` holds the CSS, `src/lib/tokens.ts` describes each token, and `src/lib/tokens.test.ts` and `src/lib/palette.test.ts` fail when they drift or a pair loses contrast. `/design` renders all of it over the real components.

## Overview

Each screen has one hero number, a chart as its centrepiece, and generous space around both. Space separates things before a line does, and a line before a box. Boxes are few and each one means something: the pale brass field is your mandate, a pale navy card is the account asking for you, a slate well holds secondary facts. Type is one family in one tight scale, sentence case throughout. Motion answers the owner and then gets out of the way.

**The Control Rule.** Stop is on every screen, an ink pill at the right of the header, one tap from anywhere, at every width from 320px, and never disabled by loading, stale data, errors or a page transition. Nothing else in the header is ink-filled.

**The No-Nudge Rule.** Approve and Skip are the same button: the same variant, size, weight and width, side by side in a fixed order, with nothing preselected and no autofocus. The default (skip) is stated in words beside a static deadline. Nothing counts down, pulses or changes colour as the deadline nears.

**The Meaning Rule.** A colour means one thing everywhere. Brass is your mandate; navy is the account; ink is a stopped agent and the Stop control; crimson is the kill switch and nothing else. Nothing is coloured for decoration.

**No gamification.** No confetti, streaks, badges, levels, celebratory motion or "you're on a roll". A gain is shown exactly as plainly as a loss.

## Colors

Strategy: Restrained. Navy and brass (P1 in [COLOR.md](COLOR.md), chosen by the founder): four meaning colours on slate neutrals, all OKLCH ramp steps, all flat. Light only. Most of a screen is the near-white page; navy appears as the account's line and its links; brass only where the mandate speaks.

| Colour | Meaning | Token | Value | Used for |
|---|---|---|---|---|
| Brass | Your mandate | `--mandate` (field), `--mandate-marker` (rails, level lines), `--mandate-strong` (labels), `--mandate-edge` (lines inside the field) | brass-100 `oklch(0.962 0.03 80)` #fdf1dc; brass-500 `oklch(0.62 0.12 80)` #ac7d1b; brass-700 `oklch(0.415 0.083 80)` #634606 | The mandate field (envelope, limit rails, the approval's risk), mandate price lines on charts with pale brass axis labels, the "Your mandate" tag, `::selection` (brass-200) |
| Navy | The account | `--lapis` (= `--primary`, `--ring`) | navy-800 `oklch(0.365 0.102 258)` #183d73 | The account's equity line and fill, primary actions, links, focus rings, the paper hatch, the tab bar's current pill (navy-50) |
| Ink | Stopped or paused agent | `--ink` | slate-950 `oklch(0.225 0.012 255)` #181c21 | Paused and stopped mode pills, the exits-only ring, the Stop control, an agent's equity line |
| Crimson | Kill switch | `--crimson` | crimson-700 `oklch(0.415 0.164 27)` #900810 | The kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. Nothing else |

The token is still called `lapis`: it is the account's colour, and the name keeps the class names stable. Supporting tokens (every one a ramp step; the full list is in [COLOR.md](COLOR.md)):

| Token | Step | Role |
|---|---|---|
| `--card` / `--background` / `--muted` / `--border` | slate-50 #f7fafe / slate-100 #eff3f8 / slate-200 #e2e7ed / slate-300 #cdd3db | The page and every reading surface / wells, the sidebar, hover / skeletons and the chart grid / hairlines (drawn at 70%) |
| `--foreground` / `--muted-foreground` | slate-950 #181c21 / slate-700 #464c54 | Text / secondary text, labels, ages |
| `--lapis-muted` / `--lapis-soft` / `--lapis-strong` / `--lapis-line` | navy-200 / navy-100 / navy-900 / navy-600 | Secondary text on navy / an approval or account notice / a pressed primary action / a line inside navy |
| `--mandate-muted` / `--mandate-soft` | slate-700 / brass-50 | Secondary text on the mandate field / a mandate notice |
| `--gain` / `--loss` | green-700 #225931 / red-700 #73353f | Text and candles only, always with a sign. Headline figures show the word too ("+$123.45 gain"); in table and list rows the word is read to screen readers |
| `--warning` / `--info` and their `-soft` tints | amber-700 / blue-700, over the 100 steps | Status text on its tint. Warning is on no screen (see Kumo) |
| `--gain-cvd` / `--loss-cvd` | cvd-blue-700 #1a5078 / cvd-orange-700 #6f3d16 | Gain and loss when colour-blind friendly is on |
| `--hatch-ink` | navy-800 at 30% | The paper hatch lines |

Status colours share L 0.415 and C 0.087 and differ only in hue (gain 150, loss 12, warning 70, info 258), so none is louder than another.

**Colour-blind friendly.** `<html data-cvd="on">` remaps gain and loss to the blue and orange pair in CSS, and charts read the same attribute. It is a development preference until settings exist: `?cvd=1` or `?cvd=0`, Alt+Shift+C, or the checkbox in the scenario switcher, kept in the `mandate-cvd` cookie. `/palette` (development only) shows the ramps, tokens, contrast and colour-vision results.

Rules the tests enforce: every token is a ramp step and every neutral is tinted slate (no pure black, white or grey); no purple or violet and no bright yellow; loss stays at least 15 degrees of hue away from crimson so the kill switch owns its red; every semantic pair and every Kumo role pair in each surface scope meets WCAG 2.2 AA (4.5:1 body, 3:1 marks) and APCA (Lc 75 body, Lc 45 marks); the colour-vision pairs that must stay apart do under simulated deuteranopia and protanopia; no component uses warning or a raw colour value; there is no `.dark` block and no `dark:` class anywhere in `src/`. `src/lib/crimson.test.tsx` renders every route in every scenario, the Stop sheet in every context, the passkey check, the result of every Stop choice, and each record screen through its passkey check and recorded result, and fails if crimson paints anything but a kill-switch choice (Kill switch, Activate the kill switch, Stop all agents, Close everything) or the kill-switch specimen on `/design`; in the source, only `globals.css`, the palette and its contrast pairs, `KillSwitchButton`'s two tones, and `/design` may name it.

No gradients of any kind (fills, text, masks, fades or glows). CI greps `src/` for them, and `e2e/flat-fills.spec.ts` reads computed styles on every route. The paper hatch is an SVG mask over a flat token colour.

**Dark mode** is follow-up work. A dark variant needs its own pass for the meaning colours; shipping half of one would break the Meaning Rule. The app sets `color-scheme: light` and stores nothing in the browser.

## Typography

**Mona Sans**, one variable family for everything (`@fontsource-variable/mona-sans`, SIL OFL 1.1, self-hosted; the Latin file is about 40 KB). Why it:

- **Calm and current.** A grotesque with a humanist touch, drawn for product UI: it reads warm at 16px and crisp and confident at the hero size, closer to the consumer-finance apps the founder named than a neutral system face, without a display face's personality getting in the way of numbers.
- **Figures.** Tabular figures (`tnum`) with a plain zero, so money lines up and never reads as a code. The true minus sign (U+2212) is in the font. This retires the Placard stack's borrowed figure face (Atkinson's zero is slashed).
- **One family, full range.** Weights 200 to 900 and a width axis in one file, so the scale is built from size and three weights, never from a second family.
- **Licence and hosting.** OFL, from npm, no font from a third-party host.

| Role | Class | Size / line height | Weight | Tracking | Use |
|---|---|---|---|---|---|
| Hero | `text-hero` | `clamp(2.5rem, 1.75rem + 2.75vw, 3.5rem)` / 1.05 | 600 | -0.035em | One per screen: account equity, an agent's equity, the approval's action |
| H1 | `text-h1` | 1.75rem / 1.2 | 600 | -0.02em | A page title |
| H2 | `text-h2` | 1.25rem / 1.3 | 600 | -0.01em | A section |
| H3 | `text-h3` | 1rem / 1.4 | 600 | 0 | A group or a row's name |
| Figure | `text-figure` | 1.375rem / 1.2 | 500 | -0.015em | Key figures beside the hero |
| Body | `text-base` | 1rem / 1.5 | 400 | 0 | Reading text |
| Small | `text-sm` | 0.875rem / 1.43 | 400 | 0 | Rows, notices |
| Caption | `text-caption` | 0.8125rem / 1.4 | 400 | 0 | Ages, disclosures, secondary facts |
| Label | `text-label`, `field-label` | 0.8125rem / 1.35 | 500 | 0 | Field labels and chips, muted, sentence case |

Weights are 400, 500 and 600 and never bolder; `tokens.test.ts` fails on a heavier weight token or a `font-bold` class in the product (the design and palette references aside). Sentence case everywhere: there are no capitals-only labels or headings (the one exception is PAPER in the paper badge, a proper label for the environment). Headings balance their lines and paragraphs wrap pretty. Every figure uses tabular digits (`tabular`, and `font-mono` maps to the same face with tabular figures; there is no monospace family). Identifiers use tabular figures and `translate="no"`. Reading text is held under 80 characters a line with `max-w-measure` (58ch: `ch` is a zero's width, and Mona Sans letters run narrower). The root size is the browser's own, so the owner's setting carries.

The brand is not set in this face: the wordmark is drawn as outlines (see Brand).

## Brand

The Owlhead mark is the founder's artwork, traced into one flat path; the wordmark is lowercase "owlhead" in P052 Roman, as outlines ([DEC-203](../docs/project/04-decision-log.md#decisions)). The sources are `src/components/brand/owlhead-mark.svg` and `owlhead-wordmark.svg`, and `src/components/brand/Logo.tsx` inlines the same paths in `currentColor` as `OwlheadMark`, `OwlheadWordmark` and `OwlheadLockup`. No font file is committed and the UI loads no font for the wordmark.

- **Navy on light, always.** The brand is navy #183D73 on a light surface (off-white #F7FAFE or the page), never off-white on a navy block, and it does not change with the system's colour scheme. There is no tagline.
- **Lockup.** The mark, then the wordmark at half the mark's height after a gap of a quarter of it, centred vertically. The clear space around it is that same quarter. Minimum sizes: 16px for the mark, 96px wide for the lockup.
- **In the shell.** From 64rem up the sidebar header carries the lockup, and the mark alone when the sidebar collapses to icons. Below 64rem the top header carries the mark. Both are the link to the role's home.
- **Palette** ([DEC-202](../docs/project/04-decision-log.md#decisions)). Navy #183D73; brass #AC7D1B, the one accent, for lines, marks and large type only (3.5:1 on off-white); dark brass #634606 for brass as text; brass tint #FDF1DC; slate ink #181C21 for text; off-white #F7FAFE for the page. `src/lib/brand-palette.ts` holds the six values for the brand assets and the `/design` Brand block; the UI tokens in Colors are ramp steps of the same palette ([COLOR.md](COLOR.md)).
- **Generated assets.** `npm run brand` (`scripts/brand-assets.mjs`, rendering with `@resvg/resvg-js`) writes `public/` from the mark and `brand/og-image.svg`:
  - `favicon.svg`, `favicon-16.png`, `favicon-32.png`, `favicon-48.png` and `favicon.ico` (the three PNG files in one ICO): the navy mark on an off-white square tile, as large as fits (87.5% of the tile's height, 1px above and below at 16px), the same in light and dark tabs.
  - `apple-touch-icon.png` (180), `pwa-192.png` and `pwa-512.png`: the navy mark at 76% of an off-white tile's height. `pwa-maskable-512.png` is scaled to sit inside the 80% safe circle.
  - `og-image.png`: the 1200×630 share image, the navy lockup centred on off-white.
  - `site.webmanifest`: Owlhead, theme and background #F7FAFE.

  The generated files are committed with the script. `src/components/brand/brand-assets.test.ts` regenerates them into a temporary directory and fails if a byte differs, and checks their pixels (the tile, the colours, the centring, the safe zone).
- **`/design`** renders the Brand block: the mark, wordmark and lockup on off-white, clear space, minimum sizes, do and don't, and the palette with its contrast.

## Layout, space and density

Mobile first, from 360px. The phone has a top header (sidebar trigger, mark, paper badge, Stop) and a bottom tab bar; from 64rem a quiet sidebar replaces the tab bar. Screens are one column on a phone and, from 64rem, a main column with a 20rem rail beside it for "Waiting for you" and key figures.

**Two densities** share every token (DEC-204). Calm is the default and is for the screens an owner lives in: Home, agents, approvals. Dense is for audit, settings and connections, where more rows on screen matter more than air; the shell sets `data-density="dense"` on those routes.

| Token | Use | Calm | Dense |
|---|---|---|---|
| `--nav-width` | Desktop side navigation (Kumo's `--sidebar-width`) | 14rem | 14rem |
| `--content-max` | Widest content column | 68rem | 90rem |
| `--container-measure` | Reading measure (`max-w-measure`): under 80 characters a line | 58ch | 58ch |
| `--page-x` | Page padding at phone / tablet (40rem) / desktop (64rem) | 1.25rem / 1.75rem / 2.5rem | the same |
| `--page-top` | Space above the first line of a screen | 1.5rem / 2.25rem (desktop) | the same |
| `--page-bottom` | Space below the last section (desktop; phones clear the tab bar) | 4rem | the same |
| `--section-gap` | Between sections of a screen | 3rem / 3.5rem (desktop) | 2rem |
| `--block-gap` | Between a heading and its content | 1rem | 0.75rem |
| `--row-y` | Vertical padding of a list or table row | 1rem | 0.5rem |
| `--tab-bar` | The phone tab bar, plus the safe area | 4rem | 4rem |
| `--seam` | The gap between swatches on `/palette` only | 0.5rem | 0.5rem |

Touch targets are at least 44px on phones in both densities: Stop, sheet and dialog close buttons, large buttons, list rows (which are whole-row links) and Stop-sheet choices are `h-11`/`min-h-11` or taller; tabs are 64px.

**Few boxes.** A list is rows on the page separated by 1px hairlines (`border-border/70`), with a slate hover. A section heading has no rule under it; space sets it apart. A box appears only when it carries meaning: the mandate field (`rounded-2xl bg-mandate`), an approval or account notice (`rounded-2xl bg-lapis-soft`), a well for secondary facts or a recorded outcome (`rounded-2xl bg-background`). A box never sits inside another box of the same kind.

## Shape and depth

Soft, consistent corners, rounder the larger the surface:

| Radius | Value | Use |
|---|---|---|
| `xs` / `sm` | 0.25rem / 0.375rem | Placeholder and fixture chips, keyboard hints, chart ticks; Kumo's own 5px radius maps to `sm` |
| `lg` / `xl` | 0.75rem / 1rem | Menus, restriction notes, the Stop sheet's choices, the kill-switch buttons, an unknown order |
| `2xl` | 1.25rem | Panels: the mandate field, an approval card, a well, a hovered agent row |
| `3xl` | 1.5rem | The Stop sheet's leading edge, the step-up dialog |
| `full` | 9999px | Buttons, mode and source pills, provenance, the Stop control, the paper badge, the range pill, the tab bar's current pill |

Controls and panels are flat. Only what floats above the page casts a shadow: Kumo's menus and popovers (its `shadow-md` and `shadow-lg`), the Stop sheet and step-up dialog (`shadow-2xl`), each a soft navy-tinted shadow with no hard offset. Overlays dim the page with ink at 40%, with no blur. No glass.

## Components

- **Header.** Always rendered, never held back by loading: 64px, the page colour, a 70% hairline below. The sidebar trigger and the mark (below 64rem), the workspace switcher (fixtures), breadcrumbs, the ⌘K trigger (an icon below 64rem, a slate pill with a ⌘K hint above), the paper badge, the approvals count, Alerts, the user menu, and Stop. Stop is last, dominant, never shrinks, and is fully on screen at every width from 320px (`e2e/stop-visible.spec.ts`). As the header narrows, the other items give way first: below 80rem the breadcrumbs keep the last two crumbs, the workspace switcher becomes an icon, and Alerts and the user menu fold into a "More" menu; below 64rem search becomes an icon and the workspace switcher goes; below 48rem the breadcrumbs go; below 40rem the approvals count and "More" go. The sidebar sheet carries everything the header drops.
- **Stop control.** An ink pill, 44px tall, with a filled octagon and "Stop". Opens the Stop sheet.
- **Paper badge.** A navy-outlined pill over the navy hatch: `PAPER · simulated funds`. Below 30rem "simulated funds" becomes screen-reader text so Stop never leaves the screen; the hatch and PAPER stay.
- **Side navigation.** Kumo's Sidebar on the slate background with no border, collapsible to icons. Its header carries the brand (see Brand) and the account below it. Groups: Home, Approvals and Alerts without a label; Agents; Accounts; Audit; Workspace. On an agent's pages the sidebar slides to that agent's sections (Overview, Positions, Orders, Decisions, Approvals, Mandate, Prove, Activity) with a link back to all agents. The current page is a soft tint with strong text; the approvals count is a navy pill.
- **Tab bar.** Phones only: the page colour, a hairline above, five tabs 64px tall. The current tab's icon fills and sits on a pale navy pill that slides between tabs (a 300ms spring with 10% bounce; instant under reduced motion); its label turns navy.
- **Page header.** The title with the paper badge beside it, an optional description, route tabs as links (the current one underlined in navy), and actions. Record screens (an agent, a request) always carry the badge in the title row.
- **Command palette (⌘K).** "Stop…" is the first command for every role that may stop. Titles come from the screen list and owner-given agent labels; nothing typed is kept and there are no recents.
- **Hero equity chart.** See Charts.
- **Agent row (Home).** A whole-row link: the name and mode pill, "mandate · holdings" in muted text, a sparkline against the daily loss limit, equity with today's change, and a caption line with the simulated paper P&L and its disclosure. Restrictions follow as small tinted notes in the colour of whoever imposed them. Hover is the slate well.
- **Waiting for you.** In the rail on desktop and after the hero on phones: each open approval as a pale navy card with the request in a sentence, the static deadline and "Open request".
- **Stop sheet.** G2, the chooser, from the right with a 24px leading radius. A calm header (an ink octagon, the title, the paper badge); sections with plain headings; per-agent rows that expand to Pause (ink), Kill switch (crimson) and Stop-and-release (outline); account-wide Pause (ink), Stop all (crimson) and Close everything (crimson outline). Pause, Resume and Stop of a flat agent act in the sheet. The kill switch, release, Stop all and Close everything are links to their record screens, and the sheet closes on the way. Account notices wear `lapis-soft` with an account tag.
- **Kill-switch and release record screens (D10, D11).** Pages at `/agents/{agent_id}/kill-switch`, `/agents/{agent_id}/release`, `/connections/{connection_id}/stop-all` and `/connections/{connection_id}/close-all`, with opaque IDs, in a centred column. The title carries the paper badge; the document title names the environment. Every list shows expanded: orders it cancels, positions it sells or releases, agents it stops, and what it leaves alone. Release carries its "yours and unprotected" warning on the mandate field. Each agent's mode pill is part of the record. The passkey check (G3) opens from the page, and the command carries every line shown and every badge as it read, so the journal keeps what the owner confirmed.
- **Step-up dialog.** A 24px-radius dialog: the title, the one action in a slate well, and Cancel / Use passkey as pills. The waiting message sits in the footer's live region.
- **Buttons.** Pills. Primary navy, secondary slate, outline (a hairline border on the page), ghost, link. Every button presses to 0.97.
- **Mode pill.** A 24px pill with an icon and the mode in sentence case: running is slate with muted text; exits only is outlined in ink; paused and stopped are solid ink with a filled icon.
- **Source tag.** Who imposed a restriction, as a pill: brass "Your mandate" (brass tint, dark brass text, brass ring), navy "The account", ink "You", outlined "Market data".
- **Limit rail.** On the mandate field: the label, the dollar value against its cap, an 8px rounded track with a brass fill and a thin dark brass post at the limit, and the headroom and consequence in words.
- **Provenance.** "You said", "You entered" and "From template" are hairline pills; anything the platform authored ("Proposed by the platform", "Platform default") has a dashed border, so it reads as not yet yours by shape, not colour.

## Charts

TradingView Lightweight Charts (`lightweight-charts`, Apache-2.0, pinned exactly), styled in `src/components/charts/options.ts`. Canvas cannot read CSS variables, so the tokens are converted to hex once.

**The hero equity chart** (`src/components/charts/equity-chart.tsx`) is the signature of the system: on Home, account equity; on an agent, that agent's equity.

- **One hero number.** Above the line: the value in the hero size, then the change from the start of the range with its sign, the word ("gain", "loss"), its colour, "today" or the range in words, and `[[DISCLOSURE-PERFORMANCE]]` beside it.
- **Scrub.** Hold or hover on the line and the hero value, the change and the date follow the pointer, instantly (no roll while scrubbing). Let go and they return to now, where a live change rolls in. The crosshair is a hairline with no labels, because the hero figure reads it out. On touch, a horizontal drag scrubs and a vertical one still scrolls the page (`touch-pan-y`).
- **Ranges.** 1D, 1W, 1M, 3M, 1Y and All as a quiet segmented control; the current range sits on a pale navy pill that glides to the next (the same 300ms spring, a jump under reduced motion). A new range redraws the line in place.
- **Draw-in.** On first load the line draws in from the left over 700ms. Under reduced motion it is simply there.
- **Mandate levels.** The agent's daily loss limit, drawdown floor, lifetime floor, stop and take-profit are 1px brass price lines, each labelled on the axis in pale brass with dark brass text. When two levels sit closer than their labels are tall, the lower one keeps its line and drops its label. The price scale widens to include them, and a compact legend below lists every level in words and says which are outside the range shown.
- **Stale.** When the latest point is old, its age is shown beside the date ("as of 14:02, 3 min ago"). Nothing is extrapolated.
- **Flat.** The background is `ColorType.Solid`; an area's top and bottom colours are the same token; no series animates (`LastPriceAnimationMode.Disabled`). `charts.test.tsx` checks every builder for this, and that every chart colour is its token.
- **Colour follows ownership.** Account equity is a 2px navy line over a pale navy fill; an agent's equity a 2px ink line over the page colour; mandate levels brass; a proposal an agent asks about in ink, dashed. Candles are gain and loss, or the blue and orange pair when colour-blind friendly is on; pre-market and after-hours candles are the border colour.
- **Levels are labelled lines, never progress bars.**
- **Accessible.** The canvas is `role="img"` with a label, and a written summary (first, last, low, high) is its description. The ranges are a labelled group of pressed-state buttons, 44px tall on phones. The hero value has a stable screen-reader copy that never animates, and it is not a live region, so scrubbing does not flood a screen reader.
- **States.** Loading shows the chart's outline and no line. Empty, unreachable and error draw no chart and invent no values. A paused or restricted agent still shows its chart and levels. If the canvas cannot be drawn, the chart says so and the figures around it stay.
- **Attribution.** `attributionLogo: false` inside the chart; a text link to TradingView under the account chart and on `/design`, and the notice in `web/NOTICE`.
- **Fixtures.** Bars, fills and equity curves come from a seeded generator (`src/fixtures/market.ts`) that reproduces the fixture's positions, P&L and equity; the account curve ends on the broker's equity.

Sparklines (agent rows) are SVG: a 1.5px line that scales with its box, and the daily loss limit as a 1px dashed brass line.

## Motion

Emil Kowalski's rules: motion answers an action or shows what changed; it is quick, interruptible, starts from where it is, and never animates keyboard-driven or high-frequency actions. Interactions stay under 300ms; the line's draw-in, once on load, is the one longer moment.

| Token | Value | Use |
|---|---|---|
| `--ease-out` | `cubic-bezier(0.23, 1, 0.32, 1)` | Entrances, press, reveals, number changes |
| `--ease-in-out` | `cubic-bezier(0.77, 0, 0.175, 1)` | Things that move on screen: chevrons, the range pill |
| `--ease-drawer` | `cubic-bezier(0.32, 0.72, 0, 1)` | The Stop sheet |
| `--ease-spring` | `linear()` spring, about 10% overshoot | The step-up dialog settling in; never a deadline or a figure |
| `--duration-press` / `--duration-release` | 140ms / 80ms | Press to scale 0.97; the release is faster than the press |
| `--duration-hover` | 160ms | Colour changes on hover and on a mode change |
| `--duration-reveal` + `--stagger` | 240ms, 30ms apart (at most 8 steps) | A list settles in once: each row rises 6px and fades in (`reveal`) |
| `--duration-number` | 240ms | A changed value rolls up and out; a stable screen-reader copy never animates |
| `--duration-draw` | 700ms | The hero line draws in from the left on first load (`draw-in`) |
| `--duration-sheet` | 320ms in, 200ms out | Stop sheet |
| `--duration-dialog` | 240ms in, 150ms out | Step-up dialog |
| Page change | 200ms | The page content cross-fades (React's `ViewTransition`). The root is not captured and the overlay lets presses through, so the header and Stop stay live throughout; `e2e/stop-visible.spec.ts` checks Stop takes a press in every frame but the snapshot's own |

Deadlines, the figures in an approval, and Stop never move. Reduced motion drops every movement (translate, scale, clip-path, press, the draw-in, the tab pill's slide) and keeps colour and opacity changes that help comprehension.

## States

Every state has one flat treatment. Agent modes and restrictions use meaning colours; system states carry none.

| State | Treatment |
|---|---|
| Running | Slate mode pill, muted text |
| Exits only | Page-colour pill with a 1px ink ring: the agent is partly stopped |
| Paused | Solid ink pill, filled icon |
| Stopped | Solid ink pill, filled icon, "Stopped" |
| Restriction from your mandate (drawdown, daily loss, floor, goal, hard breach, removed instrument) | A pale brass note with a brass "Your mandate" tag |
| Restriction from the account (reconciliation hold, startup reconciliation, unknown order, activity outside Owlhead, account checks) | A pale navy note with a navy "The account" tag |
| Restriction from you (owner pause, stopped) | A slate note with an ink "You" tag |
| Stale market data | A slate note with an outlined "Market data" tag; the value keeps its age in a small "Stale" chip, and the status strip counts what is degraded |
| Unreachable deployment, error | A slate well with a heading: what failed, whether anything changed, and the next step; no agent data is shown or kept |
| Loading | Skeletons in the shape of the screen (the hero, the chart, the rail, rows); never a value from an earlier visit |
| Empty | "No agents yet" and one pill link: describe your first agent |
| Unknown order | Reads "unknown" in words inside a pale navy box; never a guessed status |

## Kumo

The components are Cloudflare's Kumo (`@cloudflare/kumo`, pinned exactly), on Base UI, themed to the calm system. The rules below are binding; the tests in `src/test/safety-static.test.ts`, `src/lib/roles.test.tsx`, `src/app/routes.test.tsx` and `src/components/shell/journal.test.tsx` hold them.

**Theme.** `src/app/globals.css` imports, in order, Kumo's sources, Kumo's Tailwind styles, Tailwind, then `kumo-theme.css`, which redefines every `--color-kumo-*` and `--text-color-kumo-*` token under `:root, [data-theme="owlhead"]`. The html element carries `data-theme="owlhead"` and `data-mode="light"`; the app root is `isolate`.

| Kumo role | Value |
|---|---|
| Brand, link, focus | Navy-800; brand hover navy-900 |
| Danger | Ink. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning | Amber-700 on amber-100, so a Kumo warning is honest amber, but no component may use it (a test fails on any `warning` class or `variant="warning"`): at text lightness amber and dark brass are the same colour |
| Info / success | Blue-700 / green-700 on their 100 tints |
| Canvas, base, control, overlay | The page (card) |
| Recessed | Slate (background) |
| Tint, fill | Muted; fill hover is pale navy |
| Lines | Border hairline (slate-300) |
| Badge orange | The mandate: brass tint with dark brass text, for mandate fields only |
| Badge red, green, blue-family, neutral | Ink, gain, navy, muted foreground |

`[data-surface="navy" | "field" | "ink"]` rescopes Kumo's roles for content on a coloured field: on navy, text is slate-50, tints are navy-900 and lines navy-600; on the mandate field, the base is the brass tint, text slate-950, strong text dark brass and lines brass-500; on ink, lines are slate-700. Kumo's arbitrary radii (5px, 10px) join the radius scale. Kumo's drop shadow is transparent; the 1px shadow-edge hairline stays in the border colour.

**Flat fills.** Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, scroll masks on the sidebar, layer dialog and tab list, and a shimmer on skeletons. `kumo-theme.css` flattens each one: the button overlay is one solid brand colour (the end colour Kumo computes for primary, the only emphasis variant we use), masks are removed, and skeletons are static muted fields. A unit test checks that each override is present and that Kumo still ships the class names it targets. The Playwright suite (`e2e/flat-fills.spec.ts`) checks the result in Chromium against `next start`: on every route, at desktop and phone widths and with the Stop sheet, passkey dialog, command palette and phone sidebar open, no element or pseudo-element has a computed background image, mask, border image or list image containing a gradient; and on the Kumo surfaces rendered on `/design` (primary and destructive-styled buttons, a table with a sticky header, overflowing tabs and sidebar, skeletons, a layer dialog) the computed background image and mask are `none`, the button overlay is the solid brand fill with no inset shadow, and skeletons do not animate.

**Imports.** One component per import (`@cloudflare/kumo/components/button`); the root barrel is lint-banned and `optimizePackageImports` covers Kumo and Phosphor. Phosphor icons come from `@phosphor-icons/react/ssr` in server components. `LinkProvider` routes Kumo links through `next/link`; `Toasty` and `KumoLocaleProvider` wrap the app. Inputs are 16px on coarse pointers so iOS does not zoom.

**Not used.** Cloudflare's logo and "Powered by Cloudflare"; Kumo's destructive and secondary-destructive variants (lint-banned); Meter (limits are rails in dollars, and goals and profit stops are never progress bars); clipboard copy of agent names or instruments; recents or stored history; select-all on proposals; "Recommended" or "New" badges on models; green "healthy" dots (status shows "as of" times instead); Collapsible or Tabs that hide required content on a record screen (only "View model output" collapses).

**Safety resolutions.**

1. Record screens are pages, never modals: the approval request (D6), and the kill switch and release (D10, D11), which the Stop sheet links to. Dialogs are for the Stop sheet, the passkey step-up and short admin actions. A record screen's content is fixed at first render (`useFrozen` in `src/lib/frozen.ts`) and is what its artifact is built from. If the state underneath changes before the owner confirms, the action is withdrawn (an open passkey check closes and a late answer is ignored) and the screen offers "Show the current version"; it never updates silently. After they confirm, the record stays as they saw it, and live progress (command phases, current modes, who has approved since, the outcome) sits in a dashed "After you confirmed" or "After you responded" area outside it.
2. Pause, Stop and Kill are never `disabled` or `loading` (lint-banned); progress is status text in a live region. Sidebar loading never holds back the header.
3. No typed confirmation. Stop, kill and release ask for a passkey; Pause does not.
4. Crimson is the kill switch alone, through `KillSwitchButton`.
5. Badge orange (the brass mandate tint) marks mandate fields only.
6. Titles are generic ("Agent", "Approval request", "Orders"); IDs are opaque; model text never becomes a palette title, page title or button label.
7. The paper badge is in the header, the Stop sheet title and every record-screen title.
8. Approve and Skip are both secondary, the same size and class, full-width pills in a fixed order pinned above the tab bar on phones, with no autofocus; the deadline is static text.
9. A toast appears only after the mock journals the action, and names the action only. A request the deployment took without a journal entry shows the banner "The result is unknown; we are checking." and never a success.
10. Roles (PX-11) are a fixture switch: approvers pause only, viewers and auditors have no Stop, viewers see requests read-only, auditors see only Audit. Hiding a link is never the guard: `routeNeeds` in `src/lib/access.ts` names the capability each path needs, and the shell renders "Not available to your role" in place of any page the role may not open, so an auditor who types `/` or an agent URL sees that and a link to Audit. The Owlhead link goes to the role's home, and breadcrumbs drop crumbs the role cannot open. The kill-switch, release and close-position pages need `stop.full`. `src/app/routes.test.tsx` renders every route as every role and checks every link on screen.

## Do's and Don'ts

- Do give each screen one hero number, with its change, the word for it, "simulated" where it is paper, and its disclosure on the next line.
- Do let space separate things; use a hairline before a box, and a box only when it carries a meaning.
- Do draw every limit as a rail in dollars on the mandate field, with the point where it stops the agent marked and the headroom in words.
- Do give every gain and loss its sign (and, on a headline figure, the word), and put `[[DISCLOSURE-PERFORMANCE]]` beside every P&L.
- Do keep Stop in the header on every screen, and keep touch targets at 44px or more on phones.
- Do use tabular figures wherever numbers line up or change.
- Don't use crimson for anything but the kill switch, including errors and losses, or brass for anything but the mandate.
- Don't colour a system state (stale, unreachable, loading) with a meaning colour.
- Don't set anything in capitals-only, add heavy rules or bands of colour, or go above weight 600.
- Don't use gradients, glass, glows, or a shadow on anything that does not float.
- Don't gamify: no confetti, streaks, badges, celebratory motion, or a gain shown louder than a loss.
- Don't show anything as approved or submitted until the runtime records it.
- Don't use colour, motion or size to steer a decision, and never animate a deadline or a figure being decided on.

## History

- **2026-09-28, first look rejected.** PR #253's first screenshots (a generic card dashboard) were rejected by the founder as too plain.
- **2026-09-28, three directions.** Impeccable's `shape` flow produced three concepts over the same fixtures at a dev-only `/directions` route: Vernier (an engineering instrument panel), Placard (transit signage: flat meaning fields, condensed capitals, square corners), and Keel (Braun-era hardware: rounded panels and pill controls).
- **2026-09-28, Placard chosen.** The founder picked Placard and asked for tighter gutters. The product was named Owlhead the same day (DEC-201).
- **2026-09-28, Kumo and a dashboard shell.** shadcn/ui, Radix and lucide gave way to Kumo and Phosphor, with a collapsible sidebar, breadcrumbs, ⌘K and agent-scoped navigation.
- **2026-09-28, navy and brass.** The founder compared three palettes ([COLOR.md](COLOR.md)) and chose P1, navy and brass (DEC-202). The logo followed (DEC-203).
- **2026-09-28, the calm redesign (DEC-204).** The founder asked for a consumer-grade product at the level of Robinhood, Public or Wealthfront, without gamification. Placard's structure and signage type were superseded: Big Shoulders, Atkinson Hyperlegible Next and Public Sans gave way to Mona Sans in one tight scale; square fields and 2px rules to soft corners, hairlines and space; the dashboard to one hero number over a scrubbable equity chart; and one density to two. The palette, the logo and every safety behaviour stayed.
