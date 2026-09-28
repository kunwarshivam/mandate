---
name: Owlhead
description: The design system of the Owlhead web app. Paper trading only.
direction: Placard
---

# Owlhead design system: Placard

Placard is the design of the Owlhead web app, chosen by the founder on 2026-09-28 ("I like placard better, let's start there, reduce gutter space"). The product name is Owlhead ([DEC-201](../docs/project/04-decision-log.md#decisions)); "mandate" is the product's word for the owner's binding envelope. Every value below is shipped: `src/app/globals.css` holds the CSS, `src/lib/tokens.ts` mirrors the colours for `/design` and the tests, and `src/lib/tokens.test.ts` fails when the two drift or a pair loses contrast. `/design` renders all of it over the real components.

## Overview

The screen is transit signage. Flat colour fields own whole regions and are read before words: a lapis board for the account, a marigold field for your mandate, ink for an agent that is stopped or paused, and crimson for the kill switch and nothing else. Condensed capitals name things; a hyperlegible sans says everything else in sentence case. Every corner is square.

**The Control Rule.** Stop is on every screen, ink on paper, one tap from anywhere, and never disabled by loading, stale data or errors. Nothing else may be ink-filled at that size in the header.

**The No-Nudge Rule.** Approve and Skip are the same outline button at the same size, side by side, with nothing preselected. The default (skip) is stated in words beside the deadline, and nothing counts down, pulses or changes colour as the deadline nears.

**The Meaning Rule.** A colour means one thing everywhere. If a region is marigold, your mandate is speaking; if it is lapis, the account is. Nothing is coloured for decoration.

## Colors

Strategy: Committed. Four meaning colours on tinted neutrals, all OKLCH, all flat. Light only.

| Colour | Meaning | Token | Value | Used for |
|---|---|---|---|---|
| Marigold | Your mandate | `--marigold` | `oklch(0.85 0.155 84)` #fdc43f | The envelope field, limit rails, restriction tags a limit imposed, `::selection` |
| Lapis | The account | `--lapis` (= `--primary`, `--ring`) | `oklch(0.36 0.1 258)` #173c70 | The account block, the paper hatch, the current page in navigation, primary actions, links, focus rings, the equity knob, empty boards |
| Ink | Stopped or paused agent | `--ink` | `oklch(0.21 0.035 258)` #0e1928 | Paused and stopped mode fields, the exits-only ring, the Stop control and the Stop sheet header |
| Crimson | Kill switch | `--crimson` | `oklch(0.47 0.19 27)` #ac0311 | The kill-switch choices in the Stop sheet and the switch on the kill-switch record screens. Nothing else |

Supporting tokens:

| Token | Value | Role |
|---|---|---|
| `--background` | `oklch(0.975 0.005 250)` #f4f7fa | Page |
| `--card` | `oklch(0.995 0.002 250)` #fcfdff | Fields that hold reading text: sheets, dialogs, panels |
| `--muted` | `oklch(0.935 0.01 250)` #e5eaf0 | Quiet fields: a running agent's mode, system notices, skeletons |
| `--border` | `oklch(0.8 0.02 255)` #b5bfcb | Hairlines between rows |
| `--foreground` | `oklch(0.21 0.035 258)` #0e1928 | Text and 2px rules |
| `--muted-foreground` | `oklch(0.44 0.035 258)` #475366 | Secondary text, ages |
| `--lapis-muted` / `--lapis-soft` | `oklch(0.84 0.03 255)` / `oklch(0.93 0.03 255)` | Secondary text on lapis / an account notice field |
| `--marigold-muted` / `--marigold-soft` | `oklch(0.36 0.05 70)` / `oklch(0.955 0.05 90)` | Secondary text on marigold / a mandate notice field |
| `--marigold-foreground`, `--ink-foreground`, `--lapis-foreground`, `--crimson-foreground` | | Text on each meaning colour |
| `--gain` / `--loss` | `oklch(0.44 0.11 155)` / `oklch(0.49 0.18 10)` | Text only, always with a sign and the word ("+$123.45 gain") |
| `--hatch-ink` | lapis at 30% | The paper hatch lines |

Rules the tests enforce: every neutral is tinted toward lapis (no pure black, white or grey); no purple or violet; loss stays at least 15 degrees of hue away from crimson so the kill switch owns its red; body pairs reach 4.5:1 and marks 3:1; there is no `.dark` block and no `dark:` class anywhere in `src/`. `src/lib/crimson.test.tsx` renders every route in every scenario, the Stop sheet in every context, the passkey check, the result of every Stop choice, and each record screen through its passkey check and recorded result, and fails if crimson paints anything but a kill-switch choice (Kill switch, Activate the kill switch, Stop all agents, Close everything) or the kill-switch specimen on `/design`; in the source, only `globals.css`, `KillSwitchButton`'s two tones, and `/design` may name it.

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

Every figure uses tabular digits. Identifiers (connection ids, order ids) use the same sans with tabular figures and `translate="no"`; there is no monospace face. Reading text is held to about 65 characters (`max-w-prose` or narrower).

The brand is not set in these faces: the wordmark is drawn as outlines (see Brand).

## Brand

The Owlhead mark is the founder's artwork, traced into one flat path; the wordmark is lowercase "owlhead" in P052 Roman, as outlines ([DEC-203](../docs/project/04-decision-log.md#decisions)). The sources are `src/components/brand/owlhead-mark.svg` and `owlhead-wordmark.svg`, and `src/components/brand/Logo.tsx` inlines the same paths in `currentColor` as `OwlheadMark`, `OwlheadWordmark` and `OwlheadLockup`. No font file is committed and the UI loads no font for the wordmark.

- **Navy on light, always.** The brand is navy #183D73 on a light surface (off-white #F7FAFE or the page), never off-white on a navy block, and it does not change with the system's colour scheme. There is no tagline.
- **Lockup.** The mark, then the wordmark at half the mark's height after a gap of a quarter of it, centred vertically. The clear space around it is that same quarter. Minimum sizes: 16px for the mark, 96px wide for the lockup.
- **In the shell.** From 64rem up the sidebar header carries the lockup, and the mark alone when the sidebar collapses to icons. Below 64rem the top header carries the mark. Both are the link to the role's home.
- **Palette** ([DEC-202](../docs/project/04-decision-log.md#decisions)). Navy #183D73; brass #AC7D1B, the one accent, for rules, borders and large type only (3.5:1 on off-white); dark brass #634606 for brass as text; brass tint #FDF1DC; slate ink #181C21 for text; off-white #F7FAFE for the page. About 60% neutrals, 30% navy, at most 10% brass. `src/components/brand/palette.ts` holds the six values for the brand assets and `/design`; the UI tokens in Colors move onto them in the palette port, which replaces lapis and marigold.
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
- **Paper badge.** `PAPER · simulated funds`: a 2px lapis border over the lapis hatch. Below 30rem the words "simulated funds" become screen-reader text so Stop never leaves the screen; the hatch and PAPER stay.
- **Side navigation.** Kumo's Sidebar, collapsible to icons. Its header is a light field with the brand (see Brand) and the account below it. Groups: Home, Approvals and Alerts without a label; Agents; Accounts; Audit; Workspace. On an agent's pages the sidebar slides to that agent's sections (Overview, Positions, Orders, Decisions, Approvals, Mandate, Prove, Activity) with a link back to all agents. The current page is a solid lapis field. The approvals count is a square foreground chip.
- **Page header.** The page title with the paper badge beside it, an optional description, route tabs as links (the current one underlined in lapis), and actions. Record screens (an agent, a request) always carry the badge in the title row.
- **Command palette (⌘K).** "Stop…" is the first command for every role that may stop. Titles come from the screen list and owner-given agent labels; nothing typed is kept and there are no recents.
- **Tab bar.** A 2px foreground top rule; the current tab has a 4px lapis bar on its top edge and bold text.
- **Account block.** Under the brand in the sidebar header, on the same light field: "Account" and the broker.
- **Stop control.** An ink button, 44px tall, with the stop icon. Opens the Stop sheet.
- **Stop sheet.** G2, the chooser. An ink header with the display title; sections under heading rules; per-agent rows that expand to Pause (ink), Kill switch (crimson) and Stop-and-release (outline); account-wide Pause (ink), Stop all (crimson) and Close everything (crimson outline). Pause, Resume and Stop of a flat agent act in the sheet. The kill switch, release, Stop all and Close everything are links to their record screens, and the sheet closes on the way. Account notices wear lapis-soft with an account tag.
- **Kill-switch and release record screens (D10, D11).** Pages at `/agents/{agent_id}/kill-switch`, `/agents/{agent_id}/release`, `/connections/{connection_id}/stop-all` and `/connections/{connection_id}/close-all`, with opaque IDs. The title carries the paper badge; the document title names the environment. Every list shows expanded: orders it cancels, positions it sells or releases, agents it stops, and what it leaves alone. Release carries its "yours and unprotected" warning in a marigold field. Each agent's mode badge is part of the record. The passkey check (G3) opens from the page, and the command carries every line shown and every badge as it read, so the journal keeps what the owner confirmed.
- **Step-up dialog.** Title, the one action in a muted box under a 2px rule, and Cancel / Use passkey in a muted footer. The waiting message sits in the footer's live region.
- **Buttons.** Primary lapis, outline (2px foreground border on card), ghost, link. Every button presses to 0.97.
- **Mode badge.** A square chip in the mode's field colour with an icon and the label in capitals.
- **Source tag.** Who imposed a restriction: marigold "Your mandate", lapis "The account", ink "You", outlined "Market data".
- **Limit rail.** On marigold: the label, the dollar value against its cap, a track with an ink fill and a 4px post at the limit, and the headroom and consequence in words.
- **Provenance.** "You said", "You entered" and "From template" are bordered captions; anything the platform authored ("Proposed by the platform", "Platform default") has a dashed 2px border, so it reads as not yet yours by shape, not colour.

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
| Restriction from your mandate (drawdown, daily loss, floor, goal, hard breach, removed instrument) | Marigold-soft notice with a marigold "Your mandate" tag |
| Restriction from the account (reconciliation hold, startup reconciliation, unknown order, activity outside Owlhead, account checks) | Lapis-soft notice with a lapis "The account" tag |
| Restriction from you (owner pause, stopped) | Muted notice with an ink "You" tag |
| Stale market data | Muted notice with an outlined "Market data" tag; the value keeps its age behind a 2px-bordered "STALE" chip, and the status strip turns muted and counts what is degraded |
| Unreachable deployment, error | Muted field under a 4px foreground rule with a display heading; says what failed, whether anything changed, and the next step; no agent data is shown or kept |
| Loading | Skeleton fields in the shape of the screen (muted, with lapis-soft and marigold-soft hints where those fields will be); never a value from an earlier visit |
| Empty | A lapis board with the one next step on it |
| Unknown order | Reads "unknown" in words inside the account notice; never a guessed status |

## Kumo

The components are Cloudflare's Kumo (`@cloudflare/kumo`, pinned exactly), on Base UI, restyled to Placard. The rules below are binding; the tests in `src/test/safety-static.test.ts`, `src/lib/roles.test.tsx`, `src/app/routes.test.tsx` and `src/components/shell/journal.test.tsx` hold them.

**Theme.** `src/app/globals.css` imports, in order, Kumo's sources, Kumo's Tailwind styles, Tailwind, then `placard-kumo.css`, which redefines every `--color-kumo-*` and `--text-color-kumo-*` token under `:root, [data-theme="placard"]`. The html element carries `data-theme="placard"` and `data-mode="light"`; the app root is `isolate`.

| Kumo role | Placard value |
|---|---|
| Brand, link, info, warning | Lapis (warning is a lapis notice, never amber) |
| Danger | Ink. Crimson is not a Kumo colour; only `KillSwitchButton` draws it |
| Success | Gain green |
| Canvas / base, control, overlay / recessed, tint, fill | Page / card / muted |
| Lines, focus | Border hairline / lapis |
| Badge orange | Marigold, for mandate fields only |
| Badge red, green, blue-family, neutral | Ink, gain, lapis, muted foreground |

`[data-surface="lapis" | "field" | "ink"]` rescopes the text tokens for content on a coloured field. Radius is 0, including Kumo's unlayered `rounded`, `rounded-full`, `rounded-[5px]` and `rounded-[10px]`. Shadows are none; the 1px shadow-edge hairline stays in the border colour.

**Flat fills.** Kumo paints an overlay on emphasis buttons, fades on sticky table cells and tab scroll buttons, scroll masks on the sidebar, layer dialog and tab list, and a shimmer on skeletons. `placard-kumo.css` flattens each one: the button overlay is one solid brand colour (the end colour Kumo computes for primary, the only emphasis variant we use), masks are removed, and skeletons are static muted fields. A unit test checks that each override is present and that Kumo still ships the class names it targets. The Playwright suite (`e2e/flat-fills.spec.ts`) checks the result in Chromium against `next start`: on every route, at desktop and phone widths and with the Stop sheet, passkey dialog, command palette and phone sidebar open, no element or pseudo-element has a computed background image, mask, border image or list image containing a gradient; and on the Kumo surfaces rendered on `/design` (primary and destructive-styled buttons, a table with a sticky header, overflowing tabs and sidebar, skeletons, a layer dialog) the computed background image and mask are `none`, the button overlay is the solid brand fill with no inset shadow, and skeletons do not animate. The generic `linear-to-` rule alone already removes the overlay's image, so the button rule's own job is the solid fill and the missing shadow; removing it fails the button, destructive and layer-dialog tests.

**Imports.** One component per import (`@cloudflare/kumo/components/button`); the root barrel is lint-banned and `optimizePackageImports` covers Kumo and Phosphor. Phosphor icons come from `@phosphor-icons/react/ssr` in server components. `LinkProvider` routes Kumo links through `next/link`; `Toasty` and `KumoLocaleProvider` wrap the app. Inputs are 16px on coarse pointers so iOS does not zoom.

**Not used.** Cloudflare's logo and "Powered by Cloudflare"; Kumo's destructive and secondary-destructive variants (lint-banned); Meter (limits are rails in dollars, and goals and profit stops are never progress bars); clipboard copy of agent names or instruments; recents or stored history; select-all on proposals; "Recommended" or "New" badges on models; green "healthy" dots (status shows "as of" times instead); Collapsible or Tabs that hide required content on a record screen (only "View model output" collapses).

**Safety resolutions.**

1. Record screens are pages, never modals: the approval request (D6), and the kill switch and release (D10, D11), which the Stop sheet links to. Dialogs are for the Stop sheet, the passkey step-up and short admin actions. A record screen's content is fixed at first render (`useFrozen` in `src/lib/frozen.ts`) and is what its artifact is built from. If the state underneath changes before the owner confirms, the action is withdrawn (an open passkey check closes and a late answer is ignored) and the screen offers "Show the current version"; it never updates silently. After they confirm, the record stays as they saw it, and live progress (command phases, current modes, who has approved since, the outcome) sits in a dashed "After you confirmed" or "After you responded" area outside it.
2. Pause, Stop and Kill are never `disabled` or `loading` (lint-banned); progress is status text in a live region. Sidebar loading never holds back the header.
3. No typed confirmation. Stop, kill and release ask for a passkey; Pause does not.
4. Crimson is the kill switch alone, through `KillSwitchButton`.
5. Badge orange (marigold) marks mandate fields only.
6. Titles are generic ("Agent", "Approval request", "Orders"); IDs are opaque; model text never becomes a palette title, page title or button label.
7. The paper badge is in the header, the Stop sheet title and every record-screen title.
8. Approve and Skip are both secondary, the same size, in a fixed order, with no autofocus; the deadline is static text.
9. A toast appears only after the mock journals the action, and names the action only. A request the deployment took without a journal entry shows the banner "The result is unknown; we are checking." and never a success.
10. Roles (PX-11) are a fixture switch: approvers pause only, viewers and auditors have no Stop, viewers see requests read-only, auditors see only Audit. Hiding a link is never the guard: `routeNeeds` in `src/lib/access.ts` names the capability each path needs, and the shell renders "Not available to your role" in place of any page the role may not open, so an auditor who types `/` or an agent URL sees that and a link to Audit. The Owlhead link goes to the role's home, and breadcrumbs drop crumbs the role cannot open. The kill-switch and release record screens need `stop.full`. `src/app/routes.test.tsx` renders every route as every role and checks every link on screen.

## Do's and Don'ts

- Do give every colour field one meaning and keep it on every screen.
- Do draw every limit as a rail in dollars on marigold, with the point where it stops the agent marked and the headroom in words.
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
