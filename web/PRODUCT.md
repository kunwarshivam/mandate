# Product

<!-- impeccable:product-schema 1 -->

Written with Impeccable's `init` flow. The flow's interview could not run in this session (no
question tool reached the founder), so every fact below comes from the repository and the founder's
brief of 2026-09-28. Facts taken from a document cite it; the few that are inferences are marked
*(inferred)*. Where this file and a spec or `docs/product/09-product-experience.md` disagree, those win.

## Platform

web

## Users

Designed for first ([brief §2.1](../docs/product/09-product-experience.md#21-who-we-design-for-first),
[personas](../docs/product/02-personas-and-journeys.md)):

- **Jordan, retail investor (managed).** The default experience: every workspace is retail unless
  shown otherwise (DEC-68). Paper only until counsel signs off. Sets an envelope ("never more than
  10% of my account, ask me before anything new") and lets an agent bring ideas inside it. Needs the
  plainest words and the clearest limits. *(inferred)* Evenings, on a laptop or phone at home.
- **Alex, professional systematic trader.** Wants agents that trade without being watched, a
  handful of approval pings a week, each decidable in seconds, and zero limit breaches. Reads dense
  numbers daily, at a desk with more than one screen during market hours.
- **Priya, emerging manager.** Approves from a phone, between other work, with evidence and a
  deadline; two approvers and separation of duties. The approval request is decidable in under a
  minute (P10).
- **Dana, compliance (often fractional).** Read-only. Follows a fill back to its causes, exports,
  verifies. Needs every value attributable and nothing that acts.
- Sam (IT) administers hybrid installs; not a v1 design driver for these screens.

## Product Purpose

Owlhead lets someone hand a clear mandate to an autonomous trading agent and trust it: the agent
acts inside limits the owner set and confirmed, asks only when the owner's rules say so, and every
decision is recorded ([vision](../docs/product/01-vision-and-strategy.md)). Success is an owner who
knows, at a glance, what each agent may do, what it is doing, and how to stop it.

The web app in `web/` is slice 1: the shell, the Stop sheet, the dashboard, agent detail, and
approvals, running on labelled fixture data. It never connects to a broker.

## Positioning

The combination, shown working: a binding mandate the agent cannot exceed, per-action approval with
expiry and a safe default, and an exportable decision record, on the owner's own paper account,
with no custody ([competitive landscape](../docs/product/03-competitive-landscape.md#summary)). Not
"the only one with controls": every element exists somewhere, and the pitch is that they work
together. Brokers now open accounts to agents and state that they do not supervise them; Owlhead is
the supervision the owner sets.

## Operating Context

- Paper trading only (AGENTS.md rule 8, DEC-98). Money on screen is simulated and labelled so. Live
  exists in designs only as a blocked state.
- Approvals arrive as a generic notification with an opaque ID (P5) and are answered on a phone,
  often under a deadline, sometimes outdoors *(inferred)*.
- Monitoring happens during US market hours and after them; stale data, a paused agent, a
  reconciliation hold, and an unreachable deployment are ordinary states, not edge cases.
- The Stop control is the panic path: two taps to the right stop, with its scope in words (J-E).

## Capabilities and Constraints

The safety principles constrain the visuals as much as the flows
([brief §1](../docs/product/09-product-experience.md#1-principles), [§5](../docs/product/09-product-experience.md#5-ux-rules-derived-from-the-safety-rules)):

| # | Principle | What it forces on screen |
|---|---|---|
| P1 | Reducing risk is always within reach | Stop is visible on every screen, high contrast, never disabled by loading, stale data, or errors |
| P2 | The owner sets the envelope and can see it | Provenance on every envelope field; proposed values look inactive until confirmed |
| P3 | The platform explains and never persuades | Approve and Skip carry equal weight, in size, colour, position, and motion; no "recommended", no profit estimate, no scorecard |
| P4 | Silence is safe, and the screen says so | "If you do nothing, this action is skipped" on every request; the deadline is neutral and static, never an alarm |
| P5 | Notifications carry nothing about trading | Generic page titles, opaque IDs in URLs, nothing stored on the device |
| P6 | Show the state truthfully | Stale values keep their age; unknown orders read "unknown"; no calm green "all good" the system cannot prove |
| P7 | Limits in dollars where decisions are made | Every limit is a dollar amount with headroom; `profit_stop` is a level, never a progress bar |
| P8 | What the owner saw is what gets recorded | Record screens render deterministically; nothing optimistic ("submitted" only after the server records it) |
| P9 | Paper and live can never be confused | The paper marking (the hatch and "PAPER · simulated funds") is on every screen that shows money |
| P10 | Ask only when it matters; answer fast | An approval is decidable in under a minute on a phone |

Also binding: the kill switch is the only crimson on any screen; gains and losses carry a sign and
a word, never colour alone; compliance text appears only as named placeholders such as
`[[DISCLOSURE-PERFORMANCE]]` (rule 9); model output is quoted and attributed, never a headline or a
button label (rule 4).

Stack (DEC-200): Next.js 16, React 19, TypeScript strict, Tailwind v4, Kumo components on Base UI,
Pixelarticons icons (DEC-478), Motion, Vitest. No third-party analytics, session replay, or telemetry.

## Brand Commitments

- **Name.** The product's public name is **Owlhead**, at owlhead.ai (DEC-201, founder,
  2026-09-28). "Mandate" stays the internal codename for code, crates and repo paths, and "mandate"
  stays the product's word for the owner's binding envelope ("Your mandate"). Every user-facing
  surface says Owlhead.
- **Mark and wordmark (DEC-203).** The mark is the founder's artwork, traced into one flat
  single-colour path (`src/components/brand/owlhead-mark.svg`); the shaded original is not used in
  the UI. The wordmark is lowercase "owlhead" in P052 Roman, committed as outlines only
  (`owlhead-wordmark.svg`): no font file is committed and the UI loads no font for it. The lockup is
  the mark and the wordmark side by side. The old M mark in `assets/brand/` is the codename's and is
  not used in the UI.
- **Ink on light, off-white on dark** (DEC-204, amending DEC-203). In the app the mark, wordmark
  and lockup take the type colour: ink #0F1113 on the light theme, off-white on the dark one. The
  favicon, app icons and share image are the ink mark on an off-white tile, the same in every
  browser theme. The traced mark is never azure or sun and never on a coloured block.
- **No tagline** (the founder, 2026-09-28). No line under or beside the name, and no `description`
  in the page metadata or the share card. The one exception is the landing page's `description`,
  for search results (DEC-212); its share card still carries the name and the image alone.
- **Palette (DEC-217, replacing DEC-214's Ink and Volt).** Azure and Sun, light and dark: ink
  #0F1113 type on cool paper #FBFDFE in light mode and the reverse in dark (night #07080A); azure
  #0858BC (azure-600) for the mandate's rules, rails and marks and the account's line (never text,
  never a block), deep azure #043E89 (azure-800) for the primary action, links, the mandate's
  labels and the focus ring, the pale azure #D0E3FE (azure-200) for the mandate's field, and sun
  #FED254 (sun-300) as the highlight, always under ink type; green and red for a gain and a loss;
  crimson for the kill switch alone. About 60/30/10 paper, ink, and azure and sun.
  The brand assets take these values from `src/lib/brand-palette.ts`, which derives them from the
  UI palette (`COLOR.md`).
- **Type (DEC-209).** Public Sans, one self-hosted variable family for everything, tabular
  figures except the display hero figure (DEC-208); the wordmark stays the one serif, as outlines.
- **The frame (DEC-208, DEC-215).** Glass on the frame only (the header and the phone tab bar,
  and on desktop the dock that takes its place), flat everywhere else, with no gradient
  (DEC-200). Nothing runs under the header while every feed answers; the status strip shows when
  one does not, and on record screens.
- **Founder's visual constraints (2026-09-28, binding).** No gradients of any kind, including
  gradient text, masks, fades, and glows. No generic AI aesthetics ("I don't want AI slop
  design"). Colour must stand out ("stand out in terms of colour palette"); neat and clean; micro
  animations; pretty; need not be minimalist ("I hate boring designs").
- **Register.** Impeccable's Operate mode: the owner came to check, decide, or stop, so scanability
  and truthful state outrank expression, and the brand lives in precise details.
- **Voice.** Plain, exact, and calm. Sentences say what happened and what the owner may do next, in
  dollars and clock times. The platform explains and never persuades or cheers: no "great job", no
  urgency theatre, no exclamation marks. Errors say what failed, whether anything changed, and the
  next step. Terms follow the [glossary](../docs/product/glossary.md).
- **Anti-references.**
  - Robinhood's and Public's neon green on black: trading as a game, colour as excitement.
  - Generic SaaS dashboards: rounded cards everywhere, cards inside cards, a hero banner, a
    big-number-small-label metric row, uniform spacing, a flat type hierarchy (the look the founder
    rejected in PR #253's screenshots).
  - The three looks AI-generated interfaces cluster around (Impeccable's calibration): warm cream
    ground with a high-contrast serif display and a terracotta or signal-red accent; near-black
    with one neon accent and glowing edges; broadsheet-editorial hairlines with an italic display
    serif and small tracked mono labels.

## Evidence on Hand

- Typed fixture data under `web/src/fixtures/`, labelled as fixtures on screen (DEC-200). Three
  agents on one Alpaca paper connection, approvals, gate decisions, and scenarios (normal, empty,
  loading, stale, paused, drawdown, reconciliation, unknown order, unreachable, approvals).
- No customers, testimonials, track record, or performance figures exist. Nothing may imply them:
  P&L is shown only as paper, simulated, beside `[[DISCLOSURE-PERFORMANCE]]` (behind an info
  symbol on screen, inline in print and on record screens, DEC-210).
- No drafted compliance wording exists; placeholders stand in for it.

## Product Principles

1. **Stopping is never harder than starting.** The way to reduce risk is on every screen, one tap
   away and never disabled, and it becomes the most visible thing on the screen when something
   needs you (DEC-206).
2. **Limits are the product.** The owner's envelope, in dollars, is what the screen is about; the
   agent's activity is read against it.
3. **Truth over reassurance.** Show age, uncertainty, and unknowns plainly; never decorate a state
   the system cannot prove.
4. **Neutral at the moment of decision.** Nothing on an approval leans the owner toward yes.
5. **Paper is unmistakable.** Simulated money never looks like real money.

## Accessibility & Inclusion

WCAG 2.2 AA: body text at least 4.5:1 and large text and marks 3:1 in both the light and the dark theme
(`src/lib/palette.test.ts` checks the shipped pairs); colour never carries meaning alone; every control reachable by keyboard
with a visible focus ring; layouts hold at 360 px wide; motion respects `prefers-reduced-motion`
(fewer, gentler animations, keeping opacity and colour changes that aid comprehension).
