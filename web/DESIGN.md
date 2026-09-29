---
name: Owlhead
description: The design system of the Owlhead web app. Paper trading only.
direction: Calm, in Ink and Gold, light and dark (DEC-204)
---

# Owlhead design system

The Owlhead web app is calm and consumer-grade: the polish of Robinhood, Public or Wealthfront, with none of their gamification ([DEC-204](../docs/project/04-decision-log.md#decisions), the founder, 2026-09-28). It is set in Ink and Gold, in a light and a dark theme, with the founder's logo ([DEC-203](../docs/project/04-decision-log.md#decisions), amended by DEC-204) and every safety behaviour of the earlier direction. The product name is Owlhead (DEC-201); "mandate" is the product's word for the owner's binding envelope. The [product-experience brief](../docs/product/09-product-experience.md) binds: where this file and the brief disagree, the brief wins.

Every value below is shipped. `src/lib/palette.ts` holds the colour ramps and both themes' tokens (the reasoning and the measurements are in [COLOR.md](COLOR.md)), `src/app/globals.css` holds the CSS, `src/lib/tokens.ts` describes each token, and `src/lib/tokens.test.ts` and `src/lib/palette.test.ts` fail when they drift or a pair loses contrast in either theme. `/design` renders all of it over the real components, in whichever theme is on.

## Overview

Each screen has one hero number, a chart as its centrepiece, and generous space around both. Space separates things before a line does, and a line before a box. Boxes are few and each one means something: the pale gold field is your mandate, a pale gold card with an ink action is the account asking for you, a quiet well holds secondary facts. Type is one family in one tight scale, sentence case throughout. Motion answers the owner and then gets out of the way.

**The Control Rule.** Stop is on every screen, an ink pill (off-white in dark mode) at the right of the header, one tap from anywhere, at every width from 320px, and never disabled by loading, stale data, errors or a page transition. Nothing else in the header is filled in ink.

**The No-Nudge Rule.** Approve and Skip are the same button: the same variant, size, weight and width, side by side in a fixed order, with nothing preselected and no autofocus. The default (skip) is stated in words beside a static deadline. Nothing counts down, pulses or changes colour as the deadline nears.

**The Meaning Rule.** A colour means one thing everywhere. Gold is your mandate, and the account's line; ink is the account's actions, a stopped agent and the Stop control; crimson is the kill switch and nothing else. Nothing is coloured for decoration.

**No gamification.** No confetti, streaks, badges, levels, celebratory motion or "you're on a roll". A gain is shown exactly as plainly as a loss.

## Colors

Strategy: Restrained. Ink and Gold ([COLOR.md](COLOR.md)): paper and ink neutrals, one gold accent, crimson for the kill switch, all OKLCH ramp steps, all flat. About 60% of a screen is paper (ink in dark), 30% type and ink actions, 10% gold.

| Colour | Meaning | Tokens | Light | Dark | Used for |
|---|---|---|---|---|---|
| Gold | Your mandate | `--mandate` (field), `--mandate-marker` (rails, marks), `--mandate-strong` (labels), `--mandate-edge` (lines inside the field) | gold-100 #FFF6E6 / gold-500 #AB7D13 / gold-700 #6A4D08 | gold-850 #3C2E14 / gold-400 #D9A948 / gold-300 #F9D28A | The mandate field (envelope, limit rails, the approval's risk), the axis labels of mandate levels, the "Your mandate" tag, `::selection`, the focus ring |
| Ink and a gold line | The account | `--lapis` (= `--primary`), `--lapis-line`, `--lapis-soft` | ink-950 #14161A, gold-500, gold-100 | paper-100 #F8F7F4, gold-400, gold-850 | Primary actions and counts in ink; the account's equity line, the current tab's bar and the range pill's ring in gold |
| Ink | Stopped or paused agent | `--ink` | ink-950 #14161A | paper-100 #F8F7F4 | Paused and stopped mode pills, the exits-only ring, the Stop control, an agent's equity line |
| Crimson | Kill switch | `--crimson`, `--crimson-edge` | crimson-700 #9C0C12, edge the same | crimson-700 #9C0C12, edge crimson-400 #FD8C81 | The kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. Nothing else |

The account's token is still called `lapis`, so class names stay stable. On a chart the account is a solid gold line and a mandate level a dashed grey line with a gold label, so the two never read as one.

Supporting tokens, every one a ramp step (the full list, with both themes, is in [COLOR.md](COLOR.md)):

| Token | Light / dark | Role |
|---|---|---|
| `--card` / `--background` | paper-50 / paper-100; ink-950 / ink-975 | The body and cards (and, at 72%, the frame's `--glass`) / wells, the sidebar, hover |
| `--muted` / `--border` | paper-200; ink-900 / ink-850 | Skeletons, the chart grid / hairlines (drawn at 70%) |
| `--foreground` / `--muted-foreground` | ink-950 / ink-800; paper-100 / paper-300 | Text / secondary text, labels, ages |
| `--gain` / `--loss` | green-700 / red-700; green-300 / red-300 | Text and candles only, always with a sign. Headline figures show the word too ("+$123.45 gain"); in table and list rows the word is read to screen readers |
| `--gain-cvd` / `--loss-cvd` | cvd-blue-700 / cvd-rose-800 (raspberry); cvd-blue-300 / cvd-orange-300 | Gain and loss when colour-blind friendly is on |
| `--warning` / `--info` and their `-soft` tints | amber-700 / ink-800; amber-300 / paper-300 | Status text on its tint. Warning is on no screen (see Kumo) |
| `--hatch-ink` | ink at 30%; paper-500 at 40% | The paper hatch lines |

**Gold usage rules** (tested; details in [COLOR.md](COLOR.md#gold-usage-rules)):

- Gold only through the gold tokens. No other token is warm and saturated, apart from the warning amber that no screen uses.
- In light mode, gold is never text lighter than gold-700 (`--mandate-strong`). Gold-500 is for lines, marks and rails only.
- Gold is never a large block. Saturated gold is painted only as lines, rails, ticks and swatches no thicker than 8px; every gold surface is a pale tint (dark in dark mode). `e2e/flat-fills.spec.ts` measures this on every route in both themes.

**Colour-blind friendly.** `<html data-cvd="on">` remaps gain to blue and loss to raspberry in light mode or orange in dark mode (each alternate stays apart from gold and crimson under simulated deuteranopia and protanopia), and charts read the same attribute. It is a development preference until settings exist: `?cvd=1` or `?cvd=0`, Alt+Shift+C, or the checkbox in the scenario switcher, kept in the `mandate-cvd` cookie. `/palette` (development only) shows the ramps, tokens, contrast and colour-vision results for each theme.

Rules the tests enforce, in both themes: every token is a ramp step and every neutral is paper or ink (no pure black, white or grey); no purple or violet; loss stays at least 15 degrees of hue away from crimson so the kill switch owns its red; every semantic pair and every Kumo role pair in each surface scope meets WCAG 2.2 AA (4.5:1 body, 3:1 marks) and APCA (Lc 75 body, Lc 45 marks, in the tests only); the colour-vision pairs that must stay apart do; no component uses warning or a raw colour value. `src/lib/crimson.test.tsx` renders every route in every scenario, the Stop sheet in every context, the passkey check, the result of every Stop choice, and each record screen through its passkey check and recorded result, and fails if crimson paints anything but a kill-switch choice (Kill switch, Activate the kill switch, Stop all agents, Close everything) or the kill-switch specimen on `/design`; in the source, only `globals.css`, the palette and its contrast pairs, `KillSwitchButton`'s two tones, and `/design` may name it.

No gradients of any kind (fills, text, masks, fades or glows). CI greps `src/` for them, and `e2e/flat-fills.spec.ts` reads computed styles on every route in both themes. The paper hatch is an SVG mask over a flat token colour.

## Dark mode

Light, Dark or System (follows `prefers-color-scheme`), from the theme menu in the header, or the sidebar footer on phones. The choice is the only thing the browser stores, in the `owlhead-theme` cookie, so the server renders an explicit choice and a head script resolves System before first paint.

- **Tokens alone.** `html:root[data-mode="dark"]` in `globals.css` re-points every token to the other end of the same ramps; components do not change. There is no `dark:` class anywhere in `src/` (a test fails on one). `<html>` also carries `.dark`, only because Kumo's own classes use the `dark` variant.
- **Every meaning stays.** Ink surfaces with paper type; paper primary actions and Stop control with ink type; gold a step lighter for lines (gold-400) and text (gold-300); the mandate field a dark gold tint (gold-850); crimson still the only filled crimson, with a lighter edge (crimson-400) so the kill switch holds 3:1 against the sheet.
- **Flat.** Nothing floats on a shadow in dark mode: menus, sheets and dialogs sit on their hairline.
- **Charts** redraw from the dark palette when the mode changes (`setChartMode`).
- **The brand** is off-white on dark (`--logo`), and the browser's theme colour follows (off-white #FDFCFA, night #0B0D11).
- Playwright runs every e2e spec in a light and a dark project.

## Typography

**Public Sans**, one variable family for everything ([DEC-209](../docs/project/04-decision-log.md#decisions); `@fontsource-variable/public-sans`, SIL OFL 1.1, self-hosted; the Latin file is about 26 KB and Latin Extended about 18 KB). Why it:

- **Serious, not a default.** The U.S. Web Design System's grotesque, drawn from Libre Franklin, so it comes from the Franklin Gothic line of newspapers and financial pages. It reads plain and sturdy where Inter, Geist and Mona Sans read as the generic product default (Impeccable's reflex list).
- **Open when small, steady when large.** It stays even at 13 to 14 px in a dense price table and holds together at the hero size; the other candidates were either narrow small or loud large.
- **Figures.** Tabular, lining figures (`tnum`; lining is the default) with a flagged 1, a straight 7 and a plain oval zero narrower than the O, so money lines up and never reads as a code. The tabular feature sets the digits only, so commas and points keep their own widths. The true minus sign (U+2212) is in the font.
- **One family, full range.** Weights 100 to 900 in one file, so the scale is built from size and three weights, never from a second family. It covers Latin Extended-A (all but Ĳ, ŉ and ſ) and Vietnamese.
- **Licence and hosting.** OFL, from npm, no font from a third-party host.

**No figure face.** The hero number stays in Public Sans. A serif hero that echoed the P052 wordmark was tried (Literata, Brygada 1918 and Alegreya, with raised cents) and turned down: money would be set in two faces, since the key figures, rows and mandate rails stay sans; warm paper under a serif display is a look the product brief lists as an AI cliché; and it competes with the wordmark, which stays the product's one serif.

| Role | Class | Size / line height | Weight | Tracking | Use |
|---|---|---|---|---|---|
| Hero | `text-hero` | `clamp(2.5rem, 1.75rem + 2.75vw, 3.5rem)` / 1.05 | 600 | -0.03em | One per screen: account equity, an agent's equity, the approval's action |
| H1 | `text-h1` | 1.75rem / 1.2 | 600 | -0.02em | A page title |
| H2 | `text-h2` | 1.25rem / 1.3 | 600 | -0.01em | A section |
| H3 | `text-h3` | 1rem / 1.4 | 600 | 0 | A group or a row's name |
| Figure | `text-figure` | 1.375rem / 1.2 | 500 | -0.015em | Key figures beside the hero |
| Body | `text-base` | 1rem / 1.5 | 400 | 0 | Reading text |
| Small | `text-sm` | 0.875rem / 1.43 | 400 | 0 | Rows, notices |
| Caption | `text-caption` | 0.8125rem / 1.4 | 400 | 0 | Ages, disclosures, secondary facts |
| Label | `text-label`, `field-label` | 0.8125rem / 1.35 | 500 | 0 | Field labels and chips, muted, sentence case |

Weights are 400, 500 and 600 and never bolder; `tokens.test.ts` fails on a heavier weight token or a `font-bold` class in the product (the design and palette references aside). Sentence case everywhere: there are no capitals-only labels or headings (the one exception is PAPER in the paper badge, a proper label for the environment). Headings balance their lines and paragraphs wrap pretty. Every figure uses tabular digits (`tabular`, and `font-mono` maps to the same face with tabular figures; there is no monospace family). Identifiers use tabular figures and `translate="no"`. Reading text is held under 80 characters a line with `max-w-measure` (58ch: `ch` is a zero's width, and Public Sans letters run narrower). The hero's -0.03em keeps the figures of `$28,478.36` close without the comma touching them; no role is tracked tighter than -0.04em, and `tokens.test.ts` fails on one that is. The root size is the browser's own, so the owner's setting carries.

The brand is not set in this face: the wordmark is drawn as outlines (see Brand).

## Brand

The Owlhead mark is the founder's artwork, traced into one flat path; the wordmark is lowercase "owlhead" in P052 Roman, as outlines ([DEC-203](../docs/project/04-decision-log.md#decisions)). The sources are `src/components/brand/owlhead-mark.svg` and `owlhead-wordmark.svg`, and `src/components/brand/Logo.tsx` inlines the same paths in `currentColor` as `OwlheadMark`, `OwlheadWordmark` and `OwlheadLockup`. No font file is committed and the UI loads no font for the wordmark.

- **Ink on light, off-white on dark** (DEC-204). In the app the brand takes `--logo`, which is the type colour: ink #14161A on the light theme, off-white on the dark one. It is never gold and never on a coloured block. There is no tagline.
- **Lockup.** The mark, then the wordmark at half the mark's height after a gap of a quarter of it, centred vertically. The clear space around it is that same quarter. Minimum sizes: 16px for the mark, 96px wide for the lockup.
- **In the shell.** The top header carries the brand at every width: the lockup from 64rem, the mark alone below. It is the link to the role's home.
- **Palette.** `src/lib/brand-palette.ts` derives the brand values from the UI palette, so they cannot drift: ink #14161A, off-white #FDFCFA, gold #AB7D13, dark gold #6A4D08, gold tint #FFF6E6, night #0B0D11. The `/design` Brand block shows them with their contrast on off-white and on night.
- **Generated assets.** `npm run brand` (`scripts/brand-assets.mjs`, rendering with `@resvg/resvg-js`) writes `public/` from the mark and `brand/og-image.svg`:
  - `favicon.svg`, `favicon-16.png`, `favicon-32.png`, `favicon-48.png` and `favicon.ico` (the three PNG files in one ICO): the ink mark on an off-white square tile, as large as fits (87.5% of the tile's height, 1px above and below at 16px), the same in light and dark tabs, so the tile carries its own ground.
  - `apple-touch-icon.png` (180), `pwa-192.png` and `pwa-512.png`: the ink mark at 76% of an off-white tile's height. `pwa-maskable-512.png` is scaled to sit inside the 80% safe circle.
  - `og-image.png`: the 1200×630 share image, the ink lockup centred on off-white.
  - `site.webmanifest`: Owlhead, theme and background #FDFCFA.

  The generated files are committed with the script. `src/components/brand/brand-assets.test.ts` regenerates them into a temporary directory and fails if a byte differs, and checks their pixels (the tile, the colours, the centring, the safe zone).
- **`/design`** renders the Brand block: the mark, wordmark and lockup on off-white and on night, clear space, minimum sizes, do and don't, and the palette with its contrast.

## Layout, space and density

Mobile first, from 360px. The phone has a top header (sidebar trigger, mark, paper badge, Stop) and a bottom tab bar; from 64rem a floating dock at the bottom centre replaces the tab bar, and no sidebar takes width from the content. Screens are one column on a phone and, from 64rem, a full-width main column with a 20rem rail beside it for "Waiting for you" and key figures. From 64rem the content's bottom padding and the scroll padding clear the dock, so the last row and a focused control are never under it.

**Two densities** share every token (DEC-204). Calm is the default and is for the retail-facing screens an owner lives in: Home, agents, approvals. Dense is for audit, settings and connections, where more rows on screen matter more than air; the shell sets `data-density="dense"` on those routes.

| Token | Use | Calm | Dense |
|---|---|---|---|
| `--nav-width` | The phone and tablet navigation sheet (Kumo's `--sidebar-width`) | 14rem | 14rem |
| `--dock-h` / `--dock-gap` | The desktop dock and the space below it; content and scroll padding clear both | 3.75rem / 1rem | the same |
| `--status-row` | The status strip and the agent wire under the header, one height so either can replace the other | 2.125rem | 2.125rem |
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

**Few boxes.** A list is rows on the page separated by 1px hairlines (`border-border/70`), with a well-coloured hover. A section heading has no rule under it; space sets it apart. A box appears only when it carries meaning: the mandate field (`rounded-2xl bg-mandate`), an approval or account notice (`rounded-2xl bg-lapis-soft`), a well for secondary facts or a recorded outcome (`rounded-2xl bg-background`). A box never sits inside another box of the same kind.

**Nothing moves the page.** Live values (ages, deadlines, figures that roll) sit in fixed-width tabular slots, so a tick never shifts the layout. `e2e/no-layout-jitter.spec.ts` ticks the market data's age across each boundary where its wording changes (10 s, a minute, ten minutes, an hour), at every 8px width from 640px to 1600px, and fails if the header's height or the top of `main` moves.

## Shape and depth

Soft, consistent corners, rounder the larger the surface:

| Radius | Value | Use |
|---|---|---|
| `xs` / `sm` | 0.25rem / 0.375rem | Placeholder and fixture chips, keyboard hints, chart ticks; Kumo's own 5px radius maps to `sm` |
| `lg` / `xl` | 0.75rem / 1rem | Menus, restriction notes, the Stop sheet's choices, the kill-switch buttons, an unknown order |
| `2xl` | 1.25rem | Panels: the mandate field, an approval card, a well, a hovered agent row |
| `3xl` | 1.5rem | The Stop sheet's leading edge, the step-up dialog, the desktop dock |
| `full` | 9999px | Buttons, mode and source pills, provenance, the Stop control, the paper badge, the range pill, the tab bar's current pill |

Controls and panels are flat. In light mode only what floats above the page casts a shadow: Kumo's menus and popovers (its `shadow-md` and `shadow-lg`), the desktop dock (`shadow-lg`), the Stop sheet and step-up dialog (`shadow-2xl`), each a soft ink-tinted shadow with no hard offset. In dark mode nothing casts a shadow. Overlays dim the page with ink at 40%, with no blur.

**Glass on the frame only** (the founder, 2026-09-29; decision record pending). The sticky header, the agent wire under it, the phone tab bar and the desktop dock are frosted glass: the page scrolls underneath them and shows through, blurred. The `glass` utility in `globals.css` paints `--glass`, the card at 72% in both themes, blurs and saturates what is behind (`blur(22px) saturate(1.8)`), and draws the hairline in `--glass-edge`, the type colour at 8%. 72% is the least that keeps muted header text at AA over anything that scrolls under it: over solid ink in light mode, or solid paper in dark, muted text holds at least 5.1:1 and body text 6.8:1 (at 60% muted text would fall to 3.81:1 in light and 3.35:1 in dark); `tokens.test.ts` measures the glass over every token in each theme. The frame is the solid card where the browser cannot blur, under `prefers-reduced-transparency: reduce`, and in forced colours. Everything else stays flat: cards, the phone sidebar sheet, controls, menus, popovers, sheets and dialogs have no glass. `e2e/glass-frame.spec.ts` checks that only the frame blurs at each width (the header, the wire and the dock on desktop; the header and the tab bar on phones), that the page passes under it, and the solid fallbacks.

## Components

- **Header.** Always rendered, never held back by loading: 64px, sticky, frosted glass over the page (see Shape and depth), a hairline below. On the left the sidebar trigger (below 64rem), the brand (the mark, and the lockup from 64rem), the workspace switcher (fixtures, from 64rem) and breadcrumbs; in the middle the command bar; on the right the theme menu, the paper badge, the approvals count, Alerts, the user menu, and Stop. Stop is last, dominant, never shrinks, and is fully on screen at every width from 320px (`e2e/stop-visible.spec.ts`). As the header narrows, the other items give way first: below 100rem the paper badge keeps "simulated funds" for screen readers only; below 100rem the command bar narrows from 460px to 380px; below 90rem the workspace switcher becomes an icon; below 80rem Alerts and the user menu fold into a "More" menu. The current page's crumb never truncates: where the trail is narrower than 20rem the earlier crumbs fold into an "Earlier pages" menu (…) before it, and under 10rem the trail hides; below 64rem the command bar becomes an icon and the workspace switcher goes; below 48rem the breadcrumbs go; below 40rem the approvals count, the theme menu and "More" go. The sidebar sheet carries everything the header drops on phones and tablets, the dock on desktop.
- **Command bar.** From 64rem, the ⌘K trigger is a 40px bar in the middle of the header, 380 to 460px wide: a search icon, "Jump to an agent or screen…" in muted text (the palette jumps to screens and agents; it searches nothing else) and a ⌘K key, on the muted fill with a hairline and a 12px radius. It is centred in the header wherever both sides fit beside it (from 1440px) and narrows rather than overlap the brand, the switcher, the badge, the header's buttons or Stop at 1024, 1280 and 1440px (`e2e/command-bar.spec.ts`). Below 64rem it is a 44px icon. Both open the command palette, as ⌘K does.
- **Stop control.** An ink pill (paper in dark), 44px tall, with a filled octagon and "Stop". Opens the Stop sheet.
- **Paper badge.** A pill outlined in the account's ink over the ink hatch: `PAPER · simulated funds`. Below 30rem "simulated funds" becomes screen-reader text so Stop never leaves the screen; the hatch and PAPER stay.
- **Dock.** From 64rem, the primary navigation (`nav` "Primary") is a glass dock floating 1rem above the bottom edge, centred, with a 24px radius and 46px items with 22px icons: Home, Approvals (with its count as a pill in the account's ink), Alerts, All agents, Positions and Connections, then an Audit menu and a "More screens" menu. Audit holds the audit overview and every audit screen; "More screens" opens with the account (Alpaca paper, the account switcher's place on desktop), then the remaining agent and workspace screens with the workspace overview. Both are Kumo menus, and every item has a Kumo tooltip naming it. The current place is a raised card tile with a filled icon and `aria-current`. Every screen a role may open is on the dock or one menu away (`routes.test.tsx`); an agent's own sections are the page's tabs. `e2e/dock.spec.ts` checks that no focused control or approval choice sits under it, and `e2e/sticky-nav.spec.ts` that it stays put at 1024, 1280 and 1920px.
- **Side navigation.** Phones and tablets only, as a sheet from the header's sidebar trigger: Kumo's Sidebar on the well colour with no border. Groups: Home, Approvals and Alerts without a label; Agents; Accounts; Audit; Workspace. On an agent's pages the sheet slides to that agent's sections (Overview, Positions, Orders, Decisions, Approvals, Mandate, Prove, Activity) with a link back to all agents. The current page is a soft tint with strong text; the approvals count is an ink pill.
- **Tab bar.** Phones only: frosted glass like the header, a hairline above, five tabs 64px tall. The current tab's icon fills and sits on a pale gold pill with a gold ring that slides between tabs (a 300ms spring with 10% bounce; instant under reduced motion); its label turns ink.
- **Status strip.** A 34px row under the header: each feed's state and age, and what is degraded, with a "Fixture data" tag. It shows on phones, and on desktop whenever the agent wire does not.
- **Agent wire.** From 40rem, while every feed answers, a 34px glass strip under the header in place of the status strip: what the agents are doing, as a region named "Agent activity" with one list. Requests waiting for you come first, then today's gate decisions (the same journal as the dashboard's recent activity) and your own pauses and stops, newest first, at most 12. Each item is a link (a request to its page, a decision to the page recent activity opens): the time (muted, tabular, "14:04"), the agent's label (semibold) and a phrase built from the existing gate rules, action sentences, orders and approval labels ("Agent 2 asked you to buy 2 XYZ at $141.30", "Agent 1 blocked: orders are at most $1,000.00", "Agent 2 bought 5 QRS at $98.76", "Agent 2 paused by you"), then its state as a small glyph and a word: Waiting for you, Blocked, Held or Waiting (exits are never blocked), Done, Paused, Stopped. An order that has not filled reads "placed an order to", never "bought". Ink and muted text only: no gain or loss colour, never crimson, and no amount won or lost, so it carries no performance disclosure. With nothing today it reads "No agent activity yet today". It drifts left as a CSS marquee over a duplicated row (hidden from assistive technology and inert, so never focused), holds still under the pointer or keyboard focus, and under reduced motion stands still and scrolls by hand. On its left, "Live · N s" (the oldest feed's age, in 5-second steps, in a fixed-width slot) opens the four feeds and their ages in a Kumo popover on hover or keyboard focus; there is no "healthy" dot. A stale or failing feed, a loading, unreachable or empty workspace, a role that cannot see agents, or a frozen record screen (a request, a kill switch, a release, a close, Stop all, Close everything) shows the full status strip instead, at the same height, so nothing moves (`e2e/wire.spec.ts`).
- **Page header.** The title with the paper badge beside it, an optional description, route tabs as links (the current one underlined with a 2px gold bar), and actions. Record screens (an agent, a request) always carry the badge in the title row.
- **Theme menu.** Light, Dark and System, as a menu in the header, and in the sidebar sheet's footer below 40rem.
- **Command palette (⌘K).** "Stop…" is the first command for every role that may stop. Titles come from the screen list and owner-given agent labels; nothing typed is kept and there are no recents.
- **Hero equity chart.** See Charts.
- **Agent row (Home).** A whole-row link: the name and mode pill, "mandate · holdings" in muted text, a sparkline against the daily loss limit, equity with today's change, and a caption line with the simulated paper P&L and its disclosure. Restrictions follow as small tinted notes in the colour of whoever imposed them. Hover is the well.
- **Waiting for you.** In the rail on desktop and after the hero on phones: each open approval as a pale gold card with the request in a sentence, the static deadline and an ink "Open request" pill.
- **Stop sheet.** G2, the chooser, from the right with a 24px leading radius. A calm header (an ink octagon, the title, the paper badge); sections with plain headings; per-agent rows that expand to Pause (ink), Kill switch (crimson) and Stop-and-release (outline); account-wide Pause (ink), Stop all (crimson) and Close everything (crimson outline). Pause, Resume and Stop of a flat agent act in the sheet. The kill switch, release, Stop all and Close everything are links to their record screens, and the sheet closes on the way. Account notices wear `lapis-soft` with an account tag.
- **Kill-switch button.** `KillSwitchButton` alone draws crimson: filled (crimson with off-white type, a 2px `crimson-edge`) or outline (the card with a 2px `crimson-edge` and ink type). Its label keeps 7:1 in both themes.
- **Kill-switch and release record screens (D10, D11).** Pages at `/agents/{agent_id}/kill-switch`, `/agents/{agent_id}/release`, `/connections/{connection_id}/stop-all` and `/connections/{connection_id}/close-all`, with opaque IDs, in a centred column. The title carries the paper badge; the document title names the environment. Every list shows expanded: orders it cancels, positions it sells or releases, agents it stops, and what it leaves alone. Release carries its "yours and unprotected" warning on the mandate field. Each agent's mode pill is part of the record. The passkey check (G3) opens from the page, and the command carries every line shown and every badge as it read, so the journal keeps what the owner confirmed.
- **Step-up dialog.** A 24px-radius dialog: the title, the one action in a well, and Cancel / Use passkey as pills. The waiting message sits in the footer's live region.
- **Buttons.** Pills. Primary ink (paper in dark), secondary on the muted fill, outline (a hairline border on the page), ghost, link. Every button presses to 0.97.
- **Mode pill.** A 24px pill with an icon and the mode in sentence case: running is the well with muted text; exits only is outlined in ink; paused and stopped are solid ink with a filled icon.
- **Source tag.** Who imposed a restriction, as a pill: "Your mandate" (gold tint, dark gold text, gold ring), "The account" (gold tint, ink text), "You" (solid ink), "Market data" (outlined).
- **Limit rail.** On the mandate field: the label, the dollar value against its cap, an 8px rounded track with a gold fill and a thin dark gold post at the limit, and the headroom and consequence in words.
- **Provenance.** "You said", "You entered" and "From template" are hairline pills; anything the platform authored ("Proposed by the platform", "Platform default") has a dashed border, so it reads as not yet yours by shape, not colour.

## Charts

TradingView Lightweight Charts (`lightweight-charts`, Apache-2.0, pinned exactly), styled in `src/components/charts/options.ts`. Canvas cannot read CSS variables, so the tokens are converted to hex once per theme (`CHART_COLORS`), and `setChartMode` switches between them.

**The scrubbable hero equity chart** (`src/components/charts/equity-chart.tsx`) is the signature of the system: on Home, account equity; on an agent, that agent's equity.

- **One hero number.** Above the line: the value in the hero size, then the change from the start of the range with its sign, the word ("gain", "loss"), its colour, "today" or the range in words, and `[[DISCLOSURE-PERFORMANCE]]` beside it.
- **Scrub.** Hold or hover on the line and the hero value, the change and the date follow the pointer, instantly (no roll while scrubbing). Let go and they return to now, where a live change rolls in. The crosshair is a hairline with no labels, because the hero figure reads it out. On touch, a horizontal drag scrubs and a vertical one still scrolls the page (`touch-pan-y`).
- **Ranges.** 1D, 1W, 1M, 3M, 1Y and All as a quiet segmented control; the current range sits on a pale gold pill with a gold ring that glides to the next (the same 300ms spring, a jump under reduced motion). A new range redraws the line in place.
- **Draw-in.** On first load the line draws in from the left over 700ms. Under reduced motion it is simply there.
- **Mandate levels.** The agent's daily loss limit, drawdown floor, lifetime floor, stop and take-profit are 1px dashed grey price lines, each labelled on the axis with a pale gold tag in dark gold type. When two levels sit closer than their labels are tall, the lower one keeps its line and drops its label. The price scale widens to include them, and a compact legend below lists every level in words and says which are outside the range shown.
- **Stale.** When the latest point is old, its age is shown beside the date ("as of 14:02, 3 min ago"). Nothing is extrapolated.
- **Flat.** The background is `ColorType.Solid`; an area's top and bottom colours are the same token; no series animates (`LastPriceAnimationMode.Disabled`). `charts.test.tsx` checks every builder for this, and that every chart colour is its token in each theme.
- **Colour follows ownership.** Account equity is a 2px gold line over a pale gold fill; an agent's equity a 2px ink line over the card; mandate levels dashed grey with gold labels; average cost a solid gold line; a proposal an agent asks about in ink, dashed. Candles are gain and loss, or the colour-blind alternates when colour-blind friendly is on; pre-market and after-hours candles are the border colour.
- **Levels are labelled lines, never progress bars.**
- **Accessible.** The canvas is `role="img"` with a label, and a written summary (first, last, low, high) is its description. The ranges are a labelled group of pressed-state buttons, 44px tall on phones. The hero value has a stable screen-reader copy that never animates, and it is not a live region, so scrubbing does not flood a screen reader.
- **States.** Loading shows the chart's outline and no line. Empty, unreachable and error draw no chart and invent no values. A paused or restricted agent still shows its chart and levels. If the canvas cannot be drawn, the chart says so and the figures around it stay.
- **Attribution.** `attributionLogo: false` inside the chart; a text link to TradingView under the account chart and on `/design`, and the notice in `web/NOTICE`.
- **Fixtures.** Bars, fills and equity curves come from a seeded generator (`src/fixtures/market.ts`) that reproduces the fixture's positions, P&L and equity; the account curve ends on the broker's equity.

Sparklines (agent rows) are SVG: a 1.5px line that scales with its box, and the daily loss limit as a 1px dashed gold line.

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

Deadlines, the figures in an approval, and Stop never move. The agent wire is the one thing that moves on its own: a slow linear drift (12 s per item) that stops under the pointer or focus, and is never shown on a record screen. Reduced motion drops every movement (translate, scale, clip-path, press, the draw-in, the tab pill's slide, the wire's drift) and keeps colour and opacity changes that help comprehension.

## States

Every state has one flat treatment. Agent modes and restrictions use meaning colours; system states carry none.

| State | Treatment |
|---|---|
| Running | Mode pill on the well, muted text |
| Exits only | Card-colour pill with a 1px ink ring: the agent is partly stopped |
| Paused | Solid ink pill, filled icon |
| Stopped | Solid ink pill, filled icon, "Stopped" |
| Restriction from your mandate (drawdown, daily loss, floor, goal, hard breach, removed instrument) | A pale gold note with a "Your mandate" tag in dark gold |
| Restriction from the account (reconciliation hold, startup reconciliation, unknown order, activity outside Owlhead, account checks) | A pale gold note with an ink "The account" tag |
| Restriction from you (owner pause, stopped) | A well-coloured note with an ink "You" tag |
| Stale market data | A well-coloured note with an outlined "Market data" tag; the value keeps its age in a small "Stale" chip, and the status strip counts what is degraded, and replaces the agent wire |
| Unreachable deployment, error | A well with a heading: what failed, whether anything changed, and the next step; no agent data is shown or kept |
| Loading | Skeletons in the shape of the screen (the hero, the chart, the rail, rows); never a value from an earlier visit |
| Empty | "No agents yet" and one pill link: describe your first agent |
| Unknown order | Reads "unknown" in words inside a pale gold box; never a guessed status |

## Kumo

The components are Cloudflare's Kumo (`@cloudflare/kumo`, pinned exactly), on Base UI, themed to the calm system. The rules below are binding; the tests in `src/test/safety-static.test.ts`, `src/lib/roles.test.tsx`, `src/app/routes.test.tsx` and `src/components/shell/journal.test.tsx` hold them.

**Theme.** `src/app/globals.css` imports, in order, Kumo's sources, Kumo's Tailwind styles, Tailwind, then `kumo-theme.css`, which redefines every `--color-kumo-*` and `--text-color-kumo-*` token under `:root, [data-theme="owlhead"]` as a semantic token, so each role follows the theme. The html element carries `data-theme="owlhead"` and `data-mode="light"` or `"dark"`; the app root is `isolate`.

| Kumo role | Value |
|---|---|
| Brand, link | The account's ink (paper in dark); brand hover ink-800 (paper-300) |
| Focus | Dark gold (`--ring`, gold-700; gold-300 in dark) |
| Danger | Ink. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning | Amber on its tint, so a Kumo warning is honest amber, but no component may use it (a test fails on any `warning` class or `variant="warning"`): at text lightness amber and dark gold are neighbours |
| Info / success | The muted type / the gain green, on their tints |
| Canvas, base, control, overlay | The card |
| Recessed | The well (background) |
| Tint, fill | Muted; fill hover is the pale gold tint |
| Lines | The border hairline |
| Badge orange | The mandate: gold tint with dark gold text, for mandate fields only |
| Badge red, green, blue-family, neutral | Ink, gain, the account's ink, muted foreground |

`[data-surface="account" | "field" | "ink"]` rescopes Kumo's roles for content on a coloured field: on the account's ink, text is paper, tints are ink-800 and lines gold; on the mandate field, the base is the gold tint, strong text dark gold and lines gold; on ink, lines are ink-700. In dark mode the same scopes follow their tokens, with inverse text on the paper fills. Kumo's arbitrary radii (5px, 10px) join the radius scale. Kumo's drop shadow is transparent; the 1px shadow-edge hairline stays in the border colour.

**Flat fills.** Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, scroll masks on the sidebar, layer dialog and tab list, and a shimmer on skeletons. `kumo-theme.css` flattens each one: the button overlay is one solid brand colour (the end colour Kumo computes for primary, the only emphasis variant we use), masks are removed, and skeletons are static muted fields. A unit test checks that each override is present and that Kumo still ships the class names it targets. The Playwright suite (`e2e/flat-fills.spec.ts`) checks the result in Chromium against `next start`, in both themes: on every route, at desktop and phone widths and with the Stop sheet, passkey dialog, command palette and phone sidebar open, no element or pseudo-element has a computed background image, mask, border image or list image containing a gradient; and on the Kumo surfaces rendered on `/design` (primary and destructive-styled buttons, a table with a sticky header, overflowing tabs and sidebar, skeletons, a layer dialog) the computed background image and mask are `none`, the button overlay is the solid brand fill with no inset shadow, and skeletons do not animate.

**Imports.** One component per import (`@cloudflare/kumo/components/button`); the root barrel is lint-banned and `optimizePackageImports` covers Kumo and Phosphor. Phosphor icons come from `@phosphor-icons/react/ssr` in server components. `LinkProvider` routes Kumo links through `next/link`; `Toasty` and `KumoLocaleProvider` wrap the app. Inputs are 16px on coarse pointers so iOS does not zoom.

**Not used.** Cloudflare's logo and "Powered by Cloudflare"; Kumo's destructive and secondary-destructive variants (lint-banned); Meter (limits are rails in dollars, and goals and profit stops are never progress bars); clipboard copy of agent names or instruments; recents or stored history; select-all on proposals; "Recommended" or "New" badges on models; green "healthy" dots (status shows "as of" times instead); Collapsible or Tabs that hide required content on a record screen (only "View model output" collapses).

**Safety resolutions.**

1. Record screens are pages, never modals: the approval request (D6), and the kill switch and release (D10, D11), which the Stop sheet links to. Dialogs are for the Stop sheet, the passkey step-up and short admin actions. A record screen's content is fixed at first render (`useFrozen` in `src/lib/frozen.ts`) and is what its artifact is built from. If the state underneath changes before the owner confirms, the action is withdrawn (an open passkey check closes and a late answer is ignored) and the screen offers "Show the current version"; it never updates silently. After they confirm, the record stays as they saw it, and live progress (command phases, current modes, who has approved since, the outcome) sits in a dashed "After you confirmed" or "After you responded" area outside it.
2. Pause, Stop and Kill are never `disabled` or `loading` (lint-banned); progress is status text in a live region. Sidebar loading never holds back the header.
3. No typed confirmation. Stop, kill and release ask for a passkey; Pause does not.
4. Crimson is the kill switch alone, through `KillSwitchButton`.
5. Badge orange (the mandate's gold tint) marks mandate fields only.
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
- Do use tabular figures wherever numbers line up or change, and give live values a fixed slot so nothing shifts.
- Do check every screen in light and dark; a new colour is a token with a value in each theme.
- Don't use crimson for anything but the kill switch, including errors and losses.
- Don't use gold outside the gold tokens, as a block, or (in light mode) as text lighter than dark gold.
- Don't write a `dark:` class; the dark theme is tokens alone.
- Don't colour a system state (stale, unreachable, loading) with a meaning colour.
- Don't set anything in capitals-only, add heavy rules or bands of colour, or go above weight 600.
- Don't use gradients, glows, or a shadow on anything that does not float, and don't use glass anywhere but the frame (the header, the agent wire, the phone tab bar and the desktop dock).
- Don't gamify: no confetti, streaks, badges, celebratory motion, or a gain shown louder than a loss.
- Don't show anything as approved or submitted until the runtime records it.
- Don't use colour, motion or size to steer a decision, and never animate a deadline or a figure being decided on.

## History

- **2026-09-28, first look rejected.** PR #253's first screenshots (a generic card dashboard) were rejected by the founder as too plain.
- **2026-09-28, three directions.** Impeccable's `shape` flow produced three concepts over the same fixtures. The founder picked a transit-signage direction and asked for tighter gutters. The product was named Owlhead the same day (DEC-201).
- **2026-09-28, Kumo and a dashboard shell.** shadcn/ui, Radix and lucide gave way to Kumo and Phosphor, with a collapsible sidebar, breadcrumbs, ⌘K and agent-scoped navigation.
- **2026-09-28, a first palette and the logo** (DEC-202, DEC-203), both since superseded or amended by DEC-204.
- **2026-09-28, the calm redesign in Ink and Gold (DEC-204).** The founder asked for a consumer-grade product at the level of Robinhood, Public or Wealthfront, without gamification. The signage structure and type were superseded: three families gave way to Mona Sans in one tight scale; square fields and 2px rules to soft corners, hairlines and space; the dashboard to one hero number over a scrubbable equity chart; one density to two. The palette became Ink and Gold with a dark theme, and the logo ink on light and off-white on dark. Every safety behaviour stayed.
- **2026-09-29, glass on the frame only** (decision record pending). From a mockup, the founder approved frosted glass for the sticky header and the phone tab bar, and nothing else; the opacity went from the mockup's 58% to 72% so the header's text keeps AA over whatever scrolls under it.
- **2026-09-29, a dock, a ticker and a command bar** (decision record pending). The founder approved three changes to the frame: on desktop a floating glass dock replaces the sidebar, the lockup moves to the header and the content takes the full width; a ticker tape of held instruments' prices and day changes replaces the status strip under the header while every feed answers; and the ⌘K trigger becomes a wide command bar in the middle of the header.
- **2026-09-29, an agent wire instead of a ticker** (decision record pending). The founder turned the price ticker down as too much like a brokerage: the strip under the header became a wire of what the agents are doing, in ink and muted text. The same day, the current page's crumb stopped truncating beside the command bar, and the bar's prompt became "Jump to an agent or screen…", because the palette does not search orders.
- **2026-09-29, Public Sans (DEC-209).** The founder called Inter "an AI smell tell" and asked for the face to be chosen with Impeccable. Mona Sans is on the same reflex list, so it gave way to Public Sans after Archivo, Golos Text and Public Sans were rendered in the built app; the hero's tracking went from -0.035em to -0.03em. A serif figure face for the hero, echoing the wordmark, was tried and turned down.
