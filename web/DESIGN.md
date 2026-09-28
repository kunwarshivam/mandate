---
name: Mandate
description: Autonomous trading agents under a mandate you set. Paper trading only.
---

<!-- SEED: established with the user before implementation; re-run /impeccable document once there's code to capture the actual tokens and components. -->

# Mandate design system (seed)

This is a skeleton. Three candidate directions (Vernier, Placard, Keel) are built at the dev-only `/directions` route, with their contracts in `.impeccable/surfaces/src-app-page-tsx.md`. When the founder chooses one, `/impeccable document` rewrites this file from the built world. Until then only the invariants below are settled: every direction obeys them.

## Overview

Mandate is an operating surface for people who have handed trading to an agent and need to stay in control of it. The interface earns trust by being exact about limits and never persuasive. Colour is committed and flat, and it carries meaning at page scale: the chosen direction gives each colour one job, and the owner learns that job once.

**The Control Rule.** Stop is on every screen, high contrast, and one tap from anywhere. Nothing may compete with it for that contrast.

**The No-Nudge Rule.** Approve and Skip look identical. The default (skip) is stated in words beside the deadline, and nothing counts down, pulses or changes colour as the deadline nears.

Motion grammar: `[to be resolved during implementation]`. The chosen direction brings one motion personality (snap, roll or settle). In every direction, UI motion stays under 300ms, is interruptible, and respects reduced motion by keeping opacity and dropping movement.

## Colors

Strategy: `[to be resolved during implementation]`. Vernier is Restrained (one signal colour), Placard is Committed (flat fields own regions), and Keel is Full palette (two accents on a tinted neutral).

Settled for every direction:

- **Kill-switch crimson** is the only crimson anywhere in the product, and only on the kill switch.
- **Tinted neutrals.** No pure black, white or untinted grey; every neutral carries the direction's hue.
- **Gain and loss** have their own green and rose text colours, always paired with a sign and a word, never colour alone.
- **Paper hatch.** The environment badge's diagonal hatch is reserved for "paper, simulated funds" and must read at a glance in both themes.
- No gradients of any kind (fills, text, masks or glows), no purple or violet, and no cyan on dark.

## Typography

Pairing: `[to be resolved during implementation]`.

Settled: every figure uses tabular digits, and money never sets in a face without them. The pairing must not use Inter, Geist or Roboto. A monospace face may carry identifiers and figures but never labels or headings.

## Layout

Operate first: the dashboard answers two questions in its first viewport, "does anything need me?" and "is each agent inside its mandate?" The approval request is phone first. The trigger, the risk in dollars and the default are readable before the response controls, and the response area stays pinned above the tab bar. Layout holds from 360px up.

## Elevation & Depth

`[to be resolved during implementation]`. Settled: no box inside a box, no glass or blur surfaces, and no hard offset shadows.

## Shapes

`[to be resolved during implementation]`. Vernier uses 2px corners, Placard square corners, and Keel 14px panels with pill controls.

## Do's and Don'ts

- Do draw every limit as a rail in dollars, with the point where it stops the agent marked.
- Do say "simulated" wherever paper performance appears, beside its disclosure placeholder.
- Don't show anything as approved or submitted until the runtime records it.
- Don't use colour, motion or size to steer a decision.
- Don't make it look like a retail trading app (neon on black) or a stock SaaS dashboard (rounded cards, hero banner, uniform spacing).
