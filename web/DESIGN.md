---
name: Owlhead
description: The design system of the Owlhead web app. Paper trading only.
direction: Placard
---

# Owlhead design system: Placard

Placard is the design of the Owlhead web app, chosen by the founder on 2026-09-28 ("I like placard better, let's start there, reduce gutter space"). The product name is Owlhead ([DEC-201](../docs/project/04-decision-log.md#decisions)); "mandate" is the product's word for the owner's binding envelope. Every value below is shipped: `src/lib/palette.ts` holds the colour ramps and tokens (the reasoning is in [COLOR.md](COLOR.md)), `src/app/globals.css` holds the CSS, and `src/lib/tokens.test.ts` and `src/lib/palette.test.ts` fail when the two drift or a pair loses contrast. `/design` renders all of it over the real components.

## Overview

The screen is transit signage. Flat colour fields own whole regions and are read before words: a navy board for the account, a pale brass field under a brass rule for your mandate, ink for an agent that is stopped or paused, and crimson for the kill switch and nothing else. Condensed capitals name things; a hyperlegible sans says everything else in sentence case. Every corner is square.

**The Control Rule.** Stop is on every screen, ink on paper, one tap from anywhere, and never disabled by loading, stale data or errors. Nothing else may be ink-filled at that size in the header.

**The No-Nudge Rule.** Approve and Skip are the same outline button at the same size, side by side, with nothing preselected. The default (skip) is stated in words beside the deadline, and nothing counts down, pulses or changes colour as the deadline nears.

**The Meaning Rule.** A colour means one thing everywhere. If a region is brass, your mandate is speaking; if it is navy, the account is. Nothing is coloured for decoration.

## Colors

Strategy: Restrained. Navy and brass (P1 in [COLOR.md](COLOR.md), chosen by the founder): four meaning colours on slate neutrals, all OKLCH ramp steps, all flat. Light only. About 60% of a screen is neutral, 30% navy, 10% brass.

| Colour | Meaning | Token | Value | Used for |
|---|---|---|---|---|
| Brass | Your mandate | `--mandate` (field), `--mandate-edge` and `--mandate-marker` (rule, rails, levels), `--mandate-strong` (labels) | brass-100 `oklch(0.962 0.03 80)` #fdf1dc; brass-500 `oklch(0.62 0.12 80)` #ac7d1b; brass-700 `oklch(0.415 0.083 80)` #634606 | The envelope under a 4px brass rule, limit rails and level marks, mandate price lines on charts, the "Your mandate" tag, `::selection` (brass-200) |
| Navy | The account | `--lapis` (= `--primary`, `--ring`) | navy-800 `oklch(0.365 0.102 258)` #183d73 | The account block, the paper hatch, primary actions, links, focus rings, the account's equity line, empty boards |
| Ink | Stopped or paused agent | `--ink` | slate-950 `oklch(0.225 0.012 255)` #181c21 | Paused and stopped mode fields, the exits-only ring, the Stop control and the Stop sheet header |
| Crimson | Kill switch | `--crimson` | crimson-700 `oklch(0.415 0.164 27)` #900810 | The kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. Nothing else |

The token is still called `lapis`: it is the account's colour, and the name keeps the class names stable. Supporting tokens (every one a ramp step; the full list is in [COLOR.md](COLOR.md)):

| Token | Step | Role |
|---|---|---|
| `--background` / `--card` / `--muted` / `--border` | slate-100 #eff3f8 / slate-50 #f7fafe / slate-200 #e2e7ed / slate-300 #cdd3db | Page / reading fields / quiet fields and the chart grid / hairlines |
| `--foreground` / `--muted-foreground` | slate-950 #181c21 / slate-700 #464c54 | Text and 2px rules / secondary text, ages |
| `--lapis-muted` / `--lapis-soft` / `--lapis-strong` / `--lapis-line` | navy-200 / navy-100 / navy-900 / navy-600 | Secondary text on navy / an account notice / a tint or hover inside navy / a line inside navy |
| `--mandate-muted` / `--mandate-soft` | slate-700 / brass-50 | Secondary text on the mandate field / a mandate notice |
| `--gain` / `--loss` | green-700 #225931 / red-700 #73353f | Text and candles only, always with a sign and the word ("+$123.45 gain") |
| `--warning` / `--info` and their `-soft` tints | amber-700 / blue-700, over the 100 steps | Status text on its tint. Warning is on no screen (see Kumo) |
| `--gain-cvd` / `--loss-cvd` | cvd-blue-700 #1a5078 / cvd-orange-700 #6f3d16 | Gain and loss when colour-blind friendly is on |
| `--hatch-ink` | navy-800 at 30% | The paper hatch lines |

Status colours share L 0.415 and C 0.087 and differ only in hue (gain 150, loss 12, warning 70, info 258), so none is louder than another.

**Colour-blind friendly.** `<html data-cvd="on">` remaps gain and loss to the blue and orange pair in CSS, and charts read the same attribute. It is a development preference until settings exist: `?cvd=1` or `?cvd=0`, Alt+Shift+C, or the checkbox in the scenario switcher, kept in the `mandate-cvd` cookie. `/palette` (development only) shows the ramps, tokens, contrast and colour-vision results.

Rules the tests enforce: every token is a ramp step and every neutral is tinted slate (no pure black, white or grey); no purple or violet and no bright yellow; loss stays at least 15 degrees of hue away from crimson so the kill switch owns its red; every semantic pair and every Kumo role pair in each surface scope meets WCAG 2.2 AA (4.5:1 body, 3:1 marks) and APCA (Lc 75 body, Lc 45 marks); the colour-vision pairs that must stay apart do under simulated deuteranopia and protanopia; no component uses warning or a raw colour value; there is no `.dark` block and no `dark:` class anywhere in `src/`. `src/lib/crimson.test.tsx` renders every route in every scenario, the Stop sheet in every context, the passkey check, the result of every Stop choice, and each record screen through its passkey check and recorded result, and fails if crimson paints anything but a kill-switch choice (Kill switch, Activate the kill switch, Stop all agents, Close everything) or the kill-switch specimen on `/design`; in the source, only `globals.css`, the palette and its contrast pairs, `KillSwitchButton`'s two tones, and `/design` may name it.

No gradients of any kind (fills, text, masks, fades or glows). The paper hatch is an SVG mask over a flat token colour.

**Dark mode** is follow-up work. Placard is a daylight sign system and a dark variant needs its own design pass for the meaning colours; shipping half of one would break the Meaning Rule. The app sets `color-scheme: light` and stores nothing in the browser.

## Typography

| Role | Face | Size / line height | Weight | Case |
|---|---|---|---|---|
| Display (page titles, the Stop sheet title) | Big Shoulders Display | 3rem / 0.9 | 800 | Capitals |
| Title (agent names on detail, dialog titles) | Big Shoulders Display | 2rem / 0.95 | 800 | Capitals |
| Heading (sections, "Your mandate") | Big Shoulders Display | 1.5rem / 1 | 800 | Capitals |
| Figure (big money values) | Big Shoulders Display | 2.75rem | 700 | Tabular |
| Body | Atkinson Hyperlegible Next | 1rem (17px) / 1.5 | 400 | Sentence case |
| Small | Atkinson Hyperlegible Next | 0.875rem / 1.45 | 400 | Sentence case |
| Caption (ages, secondary facts) | Atkinson Hyperlegible Next | 0.8125rem / 1.35 | 400 | Sentence case |
| Label (field labels, chips) | Atkinson Hyperlegible Next | 0.75rem / 1.2 | 700 | Capitals (`label-caps`) |

The root size is 106.25%, so body reads at 17px when the browser default is 16px and follows the owner's own setting. `h1`–`h3` are set in the display face and uppercased in CSS, so the source text stays sentence case for screen readers and tests. Capitals appear only in display headings and field labels; body, buttons, notices and navigation are sentence case. No letter-spacing is added anywhere.

**Figures have a plain zero.** Atkinson Hyperlegible Next draws its zero with a slash and has no plain alternate (its OpenType features are `ccmp`, `frac`, `locl`, `pnum` and `tnum`; [googlefonts/atkinson-hyperlegible-next#9](https://github.com/googlefonts/atkinson-hyperlegible-next/issues/9) is open). A slashed zero reads as a code, not money. So the ten digits come from Public Sans (`@fontsource-variable/public-sans`, OFL), a neutral grotesque whose open, even figures sit well with Atkinson: an "Owlhead Figures" face limited to `unicode-range: U+0030-0039` leads `--font-sans`, `--font-mono` and the chart font, so every other character stays Atkinson. It is scaled with `size-adjust: 92.4%` so its figures match Atkinson's figure height, and given Atkinson's vertical metrics (`ascent-override: 106.5%`, `descent-override: 34.2%`, `line-gap-override: 0%`) so no line box grows. Big figures in Big Shoulders Display already have a plain zero. `tokens.test.ts` holds the stacks, the range, the scale and the font files, and fails if anything asks for `slashed-zero`.

Every figure uses tabular digits. Identifiers (connection ids, order ids) use the same sans with tabular figures and `translate="no"`; there is no monospace face. Reading text is held to about 65 characters (`max-w-prose` or narrower).

The brand is not set in these faces: the wordmark is drawn as outlines (see Brand).

## Brand

The Owlhead mark is the founder's artwork, traced into one flat path; the wordmark is lowercase "owlhead" in P052 Roman, as outlines ([DEC-203](../docs/project/04-decision-log.md#decisions)). The sources are `src/components/brand/owlhead-mark.svg` and `owlhead-wordmark.svg`, and `src/components/brand/Logo.tsx` inlines the same paths in `currentColor` as `OwlheadMark`, `OwlheadWordmark` and `OwlheadLockup`. No font file is committed and the UI loads no font for the wordmark.

- **Navy on light, always.** The brand is navy #183D73 on a light surface (off-white #F7FAFE or the page), never off-white on a navy block, and it does not change with the system's colour scheme. There is no tagline.
- **Lockup.** The mark, then the wordmark at half the mark's height after a gap of a quarter of it, centred vertically. The clear space around it is that same quarter. Minimum sizes: 16px for the mark, 96px wide for the lockup.
- **In the shell.** From 64rem up the sidebar header carries the lockup, and the mark alone when the sidebar collapses to icons. Below 64rem the top header carries the mark. Both are the link to the role's home.
- **Palette** ([DEC-202](../docs/project/04-decision-log.md#decisions)). Navy #183D73; brass #AC7D1B, the one accent, for rules, borders and large type only (3.5:1 on off-white); dark brass #634606 for brass as text; brass tint #FDF1DC; slate ink #181C21 for text; off-white #F7FAFE for the page. About 60% neutrals, 30% navy, at most 10% brass. `src/lib/brand-palette.ts` holds the six values for the brand assets and the `/design` Brand block; the UI tokens in Colors are ramp steps of the same palette ([COLOR.md](COLOR.md)).
- **Generated assets.** `npm run brand` (`scripts/brand-assets.mjs`, rendering with `@resvg/resvg-js`) writes `public/` from the mark and `brand/og-image.svg`:
  - `favicon.svg`, `favicon-16.png`, `favicon-32.png`, `favicon-48.png` and `favicon.ico` (the three PNGs in one ICO): the navy mark on an off-white square tile, as large as fits (87.5% of the tile's height, 1px above and below at 16px), the same in light and dark tabs.
  - `apple-touch-icon.png` (180), `pwa-192.png` and `pwa-512.png`: the navy mark at 76% of an off-white tile's height. `pwa-maskable-512.png` is scaled to sit inside the 80% safe circle.
  - `og-image.png`: the 1200×630 share image, the navy lockup centred on off-white.
  - `site.webmanifest`: Owlhead, theme and background #F7FAFE.

  The generated files are committed with the script. `src/components/brand/brand-assets.test.ts` regenerates them into a temporary directory and fails if a byte differs, and checks their pixels (the tile, the colours, the centring, the safe zone).
- **`/design`** renders the Brand block: the mark, wordmark and lockup on off-white, clear space, minimum sizes, do and don't, and the palette with its contrast.

## Layout and space

Operate first: the dashboard answers "does anything need me?" and "is each agent inside its mandate?" in its first viewport. The approval request is phone first, with the trigger, the risk in dollars and the default readable before the response controls, and the response area pinned above the tab bar. Layouts hold from 360px up.

Desktop has a side navigation that collapses to icons and a wide content column; below 64rem the navigation becomes a bottom tab bar (the sidebar opens as a sheet from the header) and the header keeps the mark, the paper badge and Stop.

The founder asked for less gutter space. Before and after:

| Token | Use | Before | After |
|---|---|---|---|
| `--nav-width` | Desktop side navigation (Kumo's `--sidebar-width`) | 15rem | 12rem |
| `--content-max` | Widest content column | 72rem | 90rem |
| `--page-x` | Page padding at phone / tablet (40rem) / desktop (64rem) | 1rem / 1.5rem / 2rem | 0.75rem / 1rem / 1.5rem |
| `--page-top` | Space above the page title | 1.5rem | 1rem / 1.25rem (desktop) |
| `--page-bottom` | Space below the last section (desktop; phones clear the tab bar) | 4rem | 2.5rem |
| `--section-gap` | Between sections of a screen | 2.5rem | 1.5rem |
| `--block-gap` | Between a heading and its content, and between rows of fields | 1rem to 1.5rem | 0.75rem |
| `--seam` | Between adjacent colour fields | 1rem (card gaps) | 0.375rem |

What did not shrink: touch targets are at least 44px on phones (Stop, sheet and dialog close buttons, large buttons, list rows and Stop-sheet choices are `h-11`/`min-h-11` or taller; tabs are 56px), and reading text keeps its measure even in the wider column.

## Elevation and depth

None. Depth comes from colour fields and 2px foreground rules, not shadows. Adjacent fields sit on a 6px seam of page colour; a field never sits inside another field. Overlays dim the page with foreground at 35%, with no blur. No glass, no hard offset shadows.

## Shapes

Radius is 0 everywhere (`--radius: 0rem`, every Tailwind radius token 0). A sign has no corners to round. Rules are 2px foreground for structure (section headings, the tab bar, dialog footers) and 1px `--border` hairlines between rows. A 4px foreground top rule marks a system notice (unreachable, error).

## Components

- **Header.** Always rendered, never held back by loading: the sidebar trigger and the mark (below 64rem), the workspace switcher (fixtures), breadcrumbs, the ⌘K trigger, the paper badge, the approvals count, Alerts, the user menu, and Stop in ink on the right. Below it, the status strip, then account banners. Stop is last, never shrinks, and is fully on screen at every width from 320px (`e2e/stop-visible.spec.ts`). As the header narrows, the other items give way first: below 80rem the breadcrumbs keep the last two crumbs, the workspace switcher becomes an icon, and Alerts and the user menu fold into a "More" menu; below 64rem search becomes an icon and the workspace switcher goes; below 48rem the breadcrumbs go; below 40rem the approvals count and "More" go. The sidebar sheet carries everything the header drops.
- **Paper badge.** `PAPER · simulated funds`: a 2px navy border over the navy hatch. Below 30rem the words "simulated funds" become screen-reader text so Stop never leaves the screen; the hatch and PAPER stay.
- **Side navigation.** Kumo's Sidebar, collapsible to icons. Its header is a light field with the brand (see Brand) and the account below it. Groups: Home, Approvals and Alerts without a label; Agents; Accounts; Audit; Workspace. On an agent's pages the sidebar slides to that agent's sections (Overview, Positions, Orders, Decisions, Approvals, Mandate, Prove, Activity) with a link back to all agents. The current page is a muted tint with strong text. The approvals count is a square foreground chip.
- **Page header.** The page title with the paper badge beside it, an optional description, route tabs as links (the current one underlined in navy), and actions. Record screens (an agent, a request) always carry the badge in the title row.
- **Command palette (⌘K).** "Stop…" is the first command for every role that may stop. Titles come from the screen list and owner-given agent labels; nothing typed is kept and there are no recents.
- **Tab bar.** A 2px foreground top rule; the current tab has a 4px navy bar on its top edge and bold text.
- **Account block.** Under the brand in the sidebar header, on the same light field: "Account" and the broker. The brand is never set on a navy block, so the account is not one either.
- **Stop control.** An ink button, 44px tall, with the stop icon. Opens the Stop sheet.
- **Stop sheet.** G2, the chooser. An ink header with the display title; sections under heading rules; per-agent rows that expand to Pause (ink), Kill switch (crimson) and Stop-and-release (outline); account-wide Pause (ink), Stop all (crimson) and Close everything (crimson outline). Pause, Resume and Stop of a flat agent act in the sheet. The kill switch, release, Stop all and Close everything are links to their record screens, and the sheet closes on the way. Account notices wear `lapis-soft` with an account tag.
- **Kill-switch and release record screens (D10, D11).** Pages at `/agents/{agent_id}/kill-switch`, `/agents/{agent_id}/release`, `/connections/{connection_id}/stop-all` and `/connections/{connection_id}/close-all`, with opaque IDs. The title carries the paper badge; the document title names the environment. Every list shows expanded: orders it cancels, positions it sells or releases, agents it stops, and what it leaves alone. Release carries its "yours and unprotected" warning on the mandate field, under a brass rule. Each agent's mode badge is part of the record. The passkey check (G3) opens from the page, and the command carries every line shown and every badge as it read, so the journal keeps what the owner confirmed.
- **Step-up dialog.** Title, the one action in a muted box under a 2px rule, and Cancel / Use passkey in a muted footer. The waiting message sits in the footer's live region.
- **Buttons.** Primary navy, outline (2px foreground border on card), ghost, link. Every button presses to 0.97.
- **Mode badge.** A square chip in the mode's field colour with an icon and the label in capitals.
- **Source tag.** Who imposed a restriction: brass "Your mandate" (brass tint, dark brass text, brass ring), navy "The account", ink "You", outlined "Market data".
- **Limit rail.** On the mandate field: the label, the dollar value against its cap, a track with a brass fill and a 4px dark brass post at the limit, and the headroom and consequence in words.
- **Provenance.** "You said", "You entered" and "From template" are bordered captions; anything the platform authored ("Proposed by the platform", "Platform default") has a dashed 2px border, so it reads as not yet yours by shape, not colour.

## Charts

TradingView Lightweight Charts (`lightweight-charts`, Apache-2.0, pinned exactly), styled in `src/components/charts/options.ts`. Canvas cannot read CSS variables, so the tokens are converted to hex once.

- **Flat.** The background is `ColorType.Solid`; an area's top and bottom colours are the same token; no series animates (`LastPriceAnimationMode.Disabled`). `charts.test.tsx` checks every builder for this, and that every chart colour is its token.
- **Colour follows ownership.** Account equity and your average cost in navy; one agent's equity as a foreground line over muted; every mandate level (loss limits, floor, stop, take-profit) as a brass-500 line with a brass-700 axis label; a proposal an agent asks about in ink, dashed. Candles are gain and loss, or the blue and orange pair when colour-blind friendly is on; pre-market and after-hours candles are the border colour.
- **Levels are labelled lines, never progress bars.** Each carries its name on the price axis, the price scale widens to include it, and the legend below lists every level in words, saying which are outside the range shown.
- **Readout.** Above the plot, in Atkinson with plain-zero tabular figures: the time in ET and the value under the crosshair, otherwise the latest value. The grid is the slate-200 tint; the crosshair is foreground with an ink label.
- **Motion.** No kinetic scrolling on touch under reduced motion; no mouse inertia ever.
- **Accessible.** The canvas is `role="img"` with a label, and a written summary (first, last, low, high) is its description.
- **States.** Loading draws the chart's outline and no line. Stale data keeps its age beside the chart. Empty, unreachable and error draw no chart and invent no values. A paused or restricted agent still shows its chart and levels. If the canvas cannot be drawn, the chart says so and the figures around it stay.
- **Attribution.** `attributionLogo: false` inside the chart; a text link to TradingView under the dashboard's account chart and on `/design`, and the notice in `web/NOTICE`.
- **Fixtures.** Bars, fills and equity curves come from a seeded generator (`src/fixtures/market.ts`) that reproduces the fixture's positions, P&L and equity; the account curve ends on the broker's equity.

## Motion

Emil Kowalski's rules: motion explains a change, stays under 300ms, is interruptible, and never animates keyboard-driven or high-frequency actions.

| Token | Value | Use |
|---|---|---|
| `--ease-out` | `cubic-bezier(0.23, 1, 0.32, 1)` | Entrances, press, reveals, number changes |
| `--ease-in-out` | `cubic-bezier(0.77, 0, 0.175, 1)` | Things that move on screen: chevrons, expand and collapse |
| `--ease-drawer` | `cubic-bezier(0.32, 0.72, 0, 1)` | The Stop sheet |
| `--duration-press` / `--duration-release` | 140ms / 80ms | Press to scale 0.97; the release is faster than the press |
| `--duration-hover` | 160ms | Colour changes on hover and on a mode change |
| `--duration-reveal` + `--stagger` | 200ms, 30ms apart | A field wipes in from its leading edge (clip-path), once, on first render |
| `--duration-sheet` | 280ms in, 200ms out | Stop sheet |
| `--duration-dialog` | 220ms in, 150ms out | Step-up dialog |
| Number roll | 200ms | A changed value rolls up and out; a stable screen-reader copy never animates |

Reduced motion drops every movement (translate, scale, clip-path, press) and keeps colour and opacity changes that help comprehension.

## States

Every state has one flat treatment inside the system. Agent modes and restrictions use meaning colours; system states carry none.

| State | Treatment |
|---|---|
| Normal | Muted mode field, foreground text |
| Exits only | Card field with an inset 2px ink ring: the agent is partly stopped |
| Paused | Solid ink field, ink-foreground text |
| Stopped | Solid ink field, ink-foreground text, "Stopped" label |
| Restriction from your mandate (drawdown, daily loss, floor, goal, hard breach, removed instrument) | Mandate-soft notice with a brass "Your mandate" tag |
| Restriction from the account (reconciliation hold, startup reconciliation, unknown order, activity outside Owlhead, account checks) | Lapis-soft notice with a navy "The account" tag |
| Restriction from you (owner pause, stopped) | Muted notice with an ink "You" tag |
| Stale market data | Muted notice with an outlined "Market data" tag; the value keeps its age behind a 2px-bordered "STALE" chip, and the status strip turns muted and counts what is degraded |
| Unreachable deployment, error | Muted field under a 4px foreground rule with a display heading; says what failed, whether anything changed, and the next step; no agent data is shown or kept |
| Loading | Skeleton fields in the shape of the screen (muted, with lapis-soft and mandate-soft hints where those fields will be); never a value from an earlier visit |
| Empty | A navy board with the one next step on it |
| Unknown order | Reads "unknown" in words inside the account notice; never a guessed status |

## Kumo

The components are Cloudflare's Kumo (`@cloudflare/kumo`, pinned exactly), on Base UI, restyled to Placard. The rules below are binding; the tests in `src/test/safety-static.test.ts`, `src/lib/roles.test.tsx`, `src/app/routes.test.tsx` and `src/components/shell/journal.test.tsx` hold them.

**Theme.** `src/app/globals.css` imports, in order, Kumo's sources, Kumo's Tailwind styles, Tailwind, then `placard-kumo.css`, which redefines every `--color-kumo-*` and `--text-color-kumo-*` token under `:root, [data-theme="placard"]`. The html element carries `data-theme="placard"` and `data-mode="light"`; the app root is `isolate`.

| Kumo role | Placard value |
|---|---|
| Brand, link, focus | Navy-800; brand hover navy-900 |
| Danger | Ink. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Warning | Amber-700 on amber-100, so a Kumo warning is honest amber, but no component may use it (a test fails on any `warning` class or `variant="warning"`): at text lightness amber and dark brass are the same colour |
| Info / success | Blue-700 / green-700 on their 100 tints |
| Canvas / base, control, overlay / recessed, tint, fill | Page / card / muted (slate) |
| Lines | Border hairline (slate-300) |
| Badge orange | The mandate: brass tint with dark brass text, for mandate fields only |
| Badge red, green, blue-family, neutral | Ink, gain, navy, muted foreground |

`[data-surface="navy" | "field" | "ink"]` rescopes Kumo's roles for content on a coloured field: on navy, text is slate-50, tints are navy-900 and lines navy-600; on the mandate field, the base is the brass tint, text slate-950, strong text dark brass and lines brass-500; on ink, lines are slate-700. Radius is 0, including Kumo's unlayered `rounded`, `rounded-full`, `rounded-[5px]` and `rounded-[10px]`. Shadows are none; the 1px shadow-edge hairline stays in the border colour.

**Flat fills.** Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, scroll masks on the sidebar, layer dialog and tab list, and a shimmer on skeletons. `placard-kumo.css` flattens each one: the button overlay is one solid brand colour (the end colour Kumo computes for primary, the only emphasis variant we use), masks are removed, and skeletons are static muted fields. A unit test checks that each override is present and that Kumo still ships the class names it targets. The Playwright suite (`e2e/flat-fills.spec.ts`) checks the result in Chromium against `next start`: on every route, at desktop and phone widths and with the Stop sheet, passkey dialog, command palette and phone sidebar open, no element or pseudo-element has a computed background image, mask, border image or list image containing a gradient; and on the Kumo surfaces rendered on `/design` (primary and destructive-styled buttons, a table with a sticky header, overflowing tabs and sidebar, skeletons, a layer dialog) the computed background image and mask are `none`, the button overlay is the solid brand fill with no inset shadow, and skeletons do not animate. The generic `linear-to-` rule alone already removes the overlay's image, so the button rule's own job is the solid fill and the missing shadow; removing it fails the button, destructive and layer-dialog tests.

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
8. Approve and Skip are both secondary, the same size, in a fixed order, with no autofocus; the deadline is static text.
9. A toast appears only after the mock journals the action, and names the action only. A request the deployment took without a journal entry shows the banner "The result is unknown; we are checking." and never a success.
10. Roles (PX-11) are a fixture switch: approvers pause only, viewers and auditors have no Stop, viewers see requests read-only, auditors see only Audit. Hiding a link is never the guard: `routeNeeds` in `src/lib/access.ts` names the capability each path needs, and the shell renders "Not available to your role" in place of any page the role may not open, so an auditor who types `/` or an agent URL sees that and a link to Audit. The Owlhead link goes to the role's home, and breadcrumbs drop crumbs the role cannot open. The kill-switch, release and close-position pages need `stop.full`. `src/app/routes.test.tsx` renders every route as every role and checks every link on screen.

## Do's and Don'ts

- Do give every colour field one meaning and keep it on every screen.
- Do draw every limit as a rail in dollars on the mandate field, with the point where it stops the agent marked and the headroom in words.
- Do put a sign and the word beside every gain and loss.
- Do say "simulated" wherever paper performance appears, beside its disclosure placeholder.
- Do keep Stop in the header on every screen, and keep touch targets at 44px or more on phones.
- Don't use crimson for anything but the kill switch, including errors and losses.
- Don't colour a system state (stale, unreachable, loading) with a meaning colour.
- Don't set body, buttons or navigation in capitals, and don't add letter-spacing.
- Don't round a corner, add a shadow, blur an overlay, or put a field inside a field.
- Don't show anything as approved or submitted until the runtime records it.
- Don't use colour, motion or size to steer a decision.
- Don't make it look like a retail trading app (neon on black) or a stock SaaS dashboard (rounded cards, hero banner, uniform spacing).

## History

- **2026-09-28, first look rejected.** PR #253's first screenshots (a generic card dashboard) were rejected by the founder as too plain.
- **2026-09-28, three directions.** Impeccable's `shape` flow produced three concepts over the same fixtures at a dev-only `/directions` route: Vernier (an engineering instrument panel: drafting-film green, one international-orange signal, tick-scale limits), Placard (transit signage: flat meaning fields, condensed capitals, square corners), and Keel (Braun-era hardware: two accents on a warm neutral, rounded panels and pill controls). Each was a full token set and component language, not a hue swap, and each was reviewed with the vendored skills before the founder saw it.
- **2026-09-28, Placard chosen.** The founder picked Placard and asked for tighter gutters. Vernier, Keel and the picker were deleted; Placard became the app's only design. The product was named Owlhead the same day (DEC-201).
- **2026-09-28, Kumo and a dashboard shell.** The founder asked for a product closer to a Cloudflare-style dashboard. shadcn/ui, Radix and lucide gave way to Kumo and Phosphor, themed to Placard, with a collapsible sidebar, breadcrumbs, ⌘K and agent-scoped navigation.
- **2026-09-28, navy and brass.** Asked whether the colours suited a serious company, the founder compared three palettes ([COLOR.md](COLOR.md); P0 Placard's lapis and marigold, P1 navy and brass, P2 navy and teal, side by side at `cursor/web-palette`@a11642e) and chose P1. Marigold is gone; the mandate became a pale brass panel under a brass rule, and P1 is the only palette. Figures gained a plain zero at the same time.
