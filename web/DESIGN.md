---
name: Owlhead
description: The design system of the Owlhead web app. Paper trading only.
direction: Calm, in Ink and Volt, light and dark (DEC-204, DEC-205, DEC-214)
---

# Owlhead design system

The principles and tokens of the Owlhead web app: what must hold on every screen, and the values the
tokens take. It is short on purpose ([DEC-502](../docs/project/decisions/DEC-502.md)): the
rendering of each surface, component and page is described in
[design/reference.md](design/reference.md), and the dated record of founder feedback in
[design/history.md](design/history.md). A rule here is binding; a sentence in the reference says how
the rule is drawn today and may change without a decision. The
[product-experience brief](../docs/product/09-product-experience.md) binds over both.

Every value here is shipped. `src/lib/palette.ts` holds the colour ramps and both themes' tokens
(measured in [COLOR.md](COLOR.md)), `src/app/globals.css` the CSS, `src/lib/tokens.ts` the meaning of
each token; `tokens.test.ts` and `palette.test.ts` fail when they drift or a pair loses contrast.
`/design` renders all of it over the real components.

## Overview

Owlhead is calm and consumer-grade: the polish of Robinhood, Public or Wealthfront, with none of
their gamification (DEC-204). Each screen has one hero number, a chart as its centrepiece, and
generous space around both. Space separates things before a line does, and a line before a box; a box
appears only when it carries meaning. Type is one family in one tight scale, sentence case
throughout. Motion answers the owner and then gets out of the way. The product name is Owlhead
(DEC-201); "mandate" is the owner's binding envelope.

Five rules hold everywhere:

1. **The Control Rule.** Stop is on every screen, at the end of the dock on desktop and of the tab
   bar on a phone, one tap from anywhere at every width from 320px, never disabled by loading, stale
   data, errors or a page transition, and never hidden (DEC-206, DEC-452). It is quiet on a calm
   screen and loud only for a risk reason from `stopAttention`, in the same place and size. Nothing
   else on the dock or the tab bar is filled in ink.
2. **The No-Nudge Rule.** Approve and Skip are the same button: the same class, size, weight and
   width, side by side in a fixed order, nothing preselected, no autofocus. The default (skip) is
   stated in words beside a static deadline. Nothing counts down, pulses or changes colour as a
   deadline nears, and nothing is shown as approved or submitted until the runtime records it.
3. **The Meaning Rule.** A colour means one thing everywhere. Volt is your mandate and the account's
   line; ink is the account's actions, a stopped agent and Stop; crimson is the kill switch and
   nothing else. System states (stale, unreachable, loading) carry no meaning colour. Nothing is
   coloured for decoration.
4. **No gamification.** No confetti, streaks, badges, levels, celebratory motion or "you're on a
   roll". A gain is shown exactly as plainly as a loss, each with its sign, and every P&L carries the
   performance disclosure (DEC-210).
5. **Records are pages.** A request, a kill switch, a release and a close are pages, never modals;
   their content is frozen at first render and is what the journal keeps; a change underneath
   withdraws the action rather than updating silently. Pause, Stop and Kill are never `disabled` or
   `loading`. Stop, kill and release ask for a passkey; Pause does not. Titles are generic, IDs are
   opaque, and model text never becomes a title or a label. Hiding a link is never the guard:
   `routeNeeds` names what each path needs.

## Colors

Ink and Volt ([COLOR.md](COLOR.md)): cool paper and ink neutrals, one volt accent, crimson for the
kill switch, all OKLCH ramp steps, all flat. About 60% of a screen is paper (ink in dark), 30% type
and ink actions, 10% volt.

| Colour | Meaning | Tokens | Light | Dark |
|---|---|---|---|---|
| Volt | Your mandate | `--mandate`, `--mandate-marker`, `--mandate-strong`, `--mandate-edge` | volt-100 #F2FCD7 / volt-500 #7C9217 / volt-700 #4C5A09 | volt-850 #2C350E / volt-400 #A4C025 / volt-200 #D2F34A |
| Ink, with a volt line | The account | `--lapis` (= `--primary`), `--lapis-line`, `--lapis-soft` | ink-950 #14161A, volt-500, volt-100 | paper-100 #F5F7F9, volt-400, volt-850 |
| Ink | Stopped or paused agent, Stop | `--ink` | ink-950 | paper-100 |
| Crimson | Kill switch | `--crimson`, `--crimson-edge` | crimson-700 #9C0C12 | crimson-700, edge crimson-400 #FD8C81 |
| Gain / loss | Signed figures only | `--gain` / `--loss`; `--gain-cvd` / `--loss-cvd` when colour-blind friendly is on | green-700 / red-700 | green-300 / red-300 |
| Surfaces | The page, wells, hairlines, text | `--card`, `--background`, `--muted`, `--border`, `--foreground`, `--muted-foreground` | paper and ink steps | the same ramps, inverted |

Rules the tests hold in both themes: every token is a ramp step and every neutral is paper or ink;
no purple or violet; loss stays 15 degrees of hue from crimson; every semantic and Kumo pair meets
WCAG 2.2 AA (4.5:1 body, 3:1 marks) and APCA; the colour-vision pairs stay apart; no component uses
Kumo's warning. Volt only through the volt tokens, never a large block (saturated volt is lines,
rails and ticks no thicker than 8px; surfaces are the pale tint), and in light mode never text
lighter than `--mandate-strong`. The one saturated volt fill is the highlight, always under ink
type. Crimson paints only a kill-switch choice (`crimson.test.tsx`). No gradients, glows, masks or
fades anywhere (`e2e/flat-fills.spec.ts`). Dark mode is tokens alone: `html[data-mode="dark"]`
re-points every token and there is no `dark:` class in `src/`.

## Typography

**Public Sans**, one variable family for everything (DEC-209), self-hosted from `@fontsource`.
Weights 400, 500 and 600 and never bolder. Sentence case everywhere (PAPER in the paper badge is
the one exception). Tabular lining figures wherever numbers line up or change; the display hero
alone is proportional, on a line of its own. The true minus sign. Reading text under 80 characters a
line (`max-w-measure`). The root size is the browser's own.

| Role | Class | Size / line height | Weight | Tracking |
|---|---|---|---|---|
| Display | `text-display` | `clamp(2rem, 14cqi, 4.75rem)` / 0.95 | 600 | -0.03em |
| Hero | `text-hero` | `clamp(2.5rem, 1.75rem + 2.75vw, 3.5rem)` / 1.05 | 600 | -0.03em |
| H1 / H2 / H3 | `text-h1` / `text-h2` / `text-h3` | 1.75 / 1.25 / 1rem | 600 | -0.02 / -0.01 / 0 |
| Figure | `text-figure` | 1.375rem / 1.2 | 500 | -0.015em |
| Body / Small | `text-base` / `text-sm` | 1rem / 0.875rem | 400 | 0 |
| Caption / Label | `text-caption` / `text-label` | 0.8125rem | 400 / 500 | 0 |

Pixelify Sans is lent by the landing page to two things in the product (the empty board's title and
the all-clear line, `pixel-face`); DotGothic16 and VT323 stay on the landing page and the logon
window alone (DEC-452). The wordmark is drawn as outlines, never a font (DEC-203).

## Layout

Mobile first, from 320px. Below 64rem the phone is a remote control (DEC-207): a two-item header,
a four-tab bar ending in Stop, Needs you first on Home, one request per screen with Approve and
Skip pinned. From 64rem a floating glass dock replaces the tab bar and screens use one page grid
(`PAGE_GRID`): a main column with a 20rem rail. Two densities share every token: calm for the
screens an owner lives in, dense (`data-density="dense"`) for audit, settings and connections.

| Token | Calm | Dense | Use |
|---|---|---|---|
| `--content-max` | 68rem | 90rem | Widest content column |
| `--container-measure` | 58ch | 58ch | Reading measure |
| `--page-x` | 1.25 / 1.75 / 2.5rem | the same | Page padding at phone / tablet / desktop |
| `--page-top` / `--page-bottom` | 1.5 (2.25 desktop) / 4rem | the same | Above the first line / below the last section |
| `--section-gap` / `--block-gap` | 3 (3.5 desktop) / 1rem | 2 / 0.75rem | Between sections / heading and content |
| `--row-y` | 1rem | 0.5rem | Row padding |
| `--dock-h`, `--tab-bar`, `--status-row` | 4rem, 4rem, 2.125rem | the same | The frame's fixed heights; content and toasts clear them |

Touch targets are 44px or more on phones. Nothing moves the page: live values sit in fixed-width
tabular slots (`e2e/no-layout-jitter.spec.ts`).

## Shapes

Flat controls and panels; soft corners, rounder the larger the surface: `xs`/`sm` for chips and
ticks, `lg`/`xl` for buttons and menus, `2xl` for panels (the mandate field, a well), `3xl` for the
sheet, dialog and dock, `full` for pills. In light mode only what floats casts a soft ink-tinted
shadow; in dark mode nothing does. Glass is on the frame only (the header, the tab bar, the dock;
DEC-208), at 72% so muted text keeps AA over anything under it, and solid where the browser cannot
blur. Everything else is flat.

## Components

One line each; the reference has the rest.

- **Buttons** (DEC-469). One key for every page action (`src/components/kumo/key.ts`); Approve and
  Skip are `DECISION_KEY`, identical. Stop, the Stop sheet's choices, Pause and the kill switch keep
  their own shapes.
- **Icons** (DEC-478). Pixelarticons at 24 or 48px, one import per icon; Phosphor and Lucide are
  lint-banned in app code.
- **Owls** (DEC-217, DEC-505). A 16-pixel sprite whose ears and markings come from the agent's ID
  and whose eyes say its mode and nothing else; drawn only at 16, 32, 48, 64 or 80px, where a sprite
  pixel is whole, so the owls and the icons share one pixel grid. A new agent's owl hatches once,
  when the runtime records it; under reduced motion it is simply there.
- **Stop control and sheet** (DEC-206). The octagon pill, quiet or loud; one sheet for every opener,
  Pause before Stop before Close everything.
- **Kill-switch button.** The only thing that draws crimson.
- **Mode chip, verdict chip, source tag, provenance** (DEC-503). A mode is named by what the agent
  may do (Trading, Selling only, Paused, Stopped) and drawn as a dot and a word, never a glyph that
  reads as a control: Trading on the well, Selling only ringed in ink, Paused and Stopped solid ink.
  A gate's verdict is one chip in one width, first in its row; "Asked you" is distinct from
  "Allowed". On a list a restriction is one line; the agent page carries its detail. Who imposed a
  restriction names its tag. What the platform authored has a dashed border, so it reads as not yet
  yours by shape.
- **Limit rail.** Every limit is a rail in dollars on the mandate field, the point where it stops
  the agent marked, the headroom in words. Never a progress bar.
- **Performance disclosure** (DEC-210). An info symbol beside every P&L; the full tag inline in print
  and on record screens.
- **Hero equity chart.** One hero number over a scrubbable line; levels are labelled lines; nothing
  is extrapolated; empty, unreachable and error invent no values.
- **Needs you.** First on a phone and first in Home's rail: requests, soonest first, each with the
  static time it is skipped at; a request appears once on Home.
- **Status strip.** Nothing under the header while every feed answers; the strip while one is
  degraded, and always on record screens.

## Motion

Motion answers an action or shows what changed; it is quick, interruptible, starts from where it is,
and never animates keyboard-driven or high-frequency actions. Interactions stay under 300ms; the hero
line's draw-in on first load (700ms) is the one longer moment. Deadlines, the figures in an approval,
and Stop never move, and nothing in the frame moves on its own (DEC-215). Reduced motion drops every
movement and keeps the colour and opacity changes that help comprehension. The tokens (`--ease-*`,
`--duration-*`) are in `globals.css` and listed in the reference.

## Do's and Don'ts

- Do give each screen one hero number, with its change, the word for it, "simulated" where it is
  paper, and its disclosure beside it.
- Do let space separate things; a hairline before a box, and a box only when it carries meaning.
- Do keep Stop at the end of the dock and the tab bar on every screen, quiet until a risk reason.
- Do use tabular figures wherever numbers line up or change, and give live values a fixed slot.
- Do check every screen in light and dark; a new colour is a token with a value in each theme.
- Don't use crimson for anything but the kill switch, including errors and losses.
- Don't use volt outside the volt tokens, as a block, or in light mode as text lighter than deep volt.
- Don't write a `dark:` class, a gradient, a glow, a shadow on anything that does not float, or
  glass anywhere but the frame.
- Don't colour a system state with a meaning colour, set anything in capitals, or go above 600.
- Don't gamify, and don't use colour, motion or size to steer a decision or animate a deadline.
- Don't show anything as approved or submitted until the runtime records it.
- Don't let a sign-in message say whether an account or an email address exists.
- Don't pulse, glow, badge or resize Stop, make it loud for a waiting request, or fill anything else
  on the dock or the tab bar with ink.

## Changing this file

A rule enters this file only when the product relies on it (safety, compliance, or a founder
decision recorded as a DEC). Founder feedback goes to [design/history.md](design/history.md) as one
line; what it changed goes to the reference. A test pins what this file states and nothing more:
the invariant ("the owner's request comes before the chart on a phone"), not the current rendering
(a height at four viewports). Measurements that only describe today's look belong in the reference,
and a test of them is a test that notices the look changed, which is not a defect (DEC-502).
