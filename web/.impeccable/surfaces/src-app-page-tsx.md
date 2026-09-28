---
version: 1
slug: "src-app-page-tsx"
primary_target: "src/app/page.tsx"
related_targets: ["src/app/approvals/[approvalId]/page.tsx"]
---

## Scope

The dashboard (`/`) and the approval request (`/approvals/[id]`, phone first). Visitor mode: Operate. Three candidate directions are built side by side at the dev-only `/directions` route over the real fixtures. The founder picks one; the other two are then deleted.

## Audience, job, constraints

The audience is Jordan, Alex, Priya and Dana (see PRODUCT.md). On the dashboard they need to know within one glance whether anything needs them and whether each agent is inside its mandate. On the approval request they decide in time or let the default (skip) apply.

The constraints are P1-P10 and DEC-200:
- Stop is high contrast and always visible, and crimson belongs only to the kill switch.
- Approve and Skip have equal weight.
- The deadline is neutral and static.
- The paper hatch is unmistakable.
- Gains and losses carry a sign and a word.
- No gradients, no cards inside cards, no purple, no cyan on dark, no glowing dark mode, no default Inter/Geist/Roboto.

## Direction contract

Seed key `3f31a2f2` (operate). The assigned form is #5 on the ordered list (Braun-era tactile hardware), built as Keel. Vernier (#1, the engineering instrument panel) and Placard (#3, transit wayfinding) were built as challengers so the founder can compare concepts, not hues.

### Vernier (challenger, list #1)

THESIS: The dashboard is a measuring instrument. It refuses the card grid; structure comes from columns, hairlines and tick scales.
OWN-WORLD: Drafting-film green ground, navy ink, and a single international-orange signal. Limits are drawn as tick scales with an orange wall, type is condensed Archivo with Azeret figures, and radius is 2px.
STORY: The owner reads every agent against its limits in one table and sees orange only where something stops the agent or waits for them.
FIRST VIEWPORT: The title sits over a four-cell readout row (equity, agents, running, waiting in orange), then waiting rows with an orange mark, then the agents table with rails. The primary action is the waiting row.
FORM: #1 instrument panel. Numbers snap and leave a decaying orange underline.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

### Placard (challenger, list #3)

THESIS: The dashboard is transit signage. It refuses neutral dashboards; flat colour fields own whole regions and are read before words.
OWN-WORLD: A lapis field for the account and the departures-style mode board, a marigold field for the mandate, and ink fields for stopped or paused agents. Big Shoulders sets uppercase signs, Atkinson Hyperlegible Next sets the text, and every corner is square.
STORY: The owner knows where they are (lapis), what binds the agent (marigold) and what is halted (ink) before reading a number.
FIRST VIEWPORT: The lapis board (title, agent-by-mode rows) sits beside a white field with the waiting request and its "Open request" action, then the agent bands of mode, identity and mandate fields.
FORM: #3 wayfinding. Fields wipe in along the reading direction, and figures roll.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

### Keel (assigned, list #5)

THESIS: The dashboard is calm desk hardware: one console, modules divided by rules, controls you could touch. It refuses the rounded card grid.
OWN-WORLD: A putty ground, one elevated panel style with 14px corners, and a petrol faceplate for what the owner controls. Apricot is for attention and limits, rails are slider tracks with a knob, and type is Funnel Display over Funnel Sans.
STORY: The owner feels the system is steady and theirs to set, and sees apricot only where it needs them.
FIRST VIEWPORT: A console panel with the petrol faceplate (equity, agent lights) beside the waiting request with its "Review" control, then one agents panel split into three modules.
FORM: #5 tactile hardware. Panels settle up on a spring, and figures cross-soften.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved

- Which direction ships.
- The DESIGN.md tokens for that direction, which the documenter writes after the choice.
- Dark mode for Placard (light only for now).
- The Impeccable interview and decision page did not run, because the founder was not in the loop.
