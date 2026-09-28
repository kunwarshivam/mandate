---
version: 2
slug: "src-app-page-tsx"
primary_target: "src/app/page.tsx"
related_targets: ["src/app/approvals/[approvalId]/page.tsx", "src/components/shell/app-shell.tsx", "src/components/stop/stop-sheet.tsx"]
---

## Scope

The shell, the Stop sheet, the dashboard (`/`) and the approval request (`/approvals/[id]`, phone first). Visitor mode: Operate. The founder chose Placard on 2026-09-28; the other two candidates (Vernier and Keel) and the `/directions` picker are deleted, and their history lives in DESIGN.md.

## Audience, job, constraints

The audience is Jordan, Alex, Priya and Dana (see PRODUCT.md). On the dashboard they need to know within one glance whether anything needs them and whether each agent is inside its mandate. On the approval request they decide in time or let the default (skip) apply.

The constraints are P1-P10, DEC-200 and DEC-201:
- Stop is high contrast and always visible, and crimson belongs only to the kill switch.
- Approve and Skip have equal weight.
- The deadline is neutral and static.
- The paper hatch is unmistakable.
- Gains and losses carry a sign and a word.
- The product is called Owlhead in the UI; "mandate" names the owner's envelope.
- No gradients, no fields inside fields, no purple, no glowing dark mode, no default Inter/Geist/Roboto.

## Direction contract: Placard

THESIS: The app is transit signage. It refuses neutral dashboards; flat colour fields own whole regions and are read before words.
OWN-WORLD: A lapis board for the account, a marigold field for your mandate, ink fields for stopped or paused agents and the Stop control, crimson for the kill switch only. Big Shoulders Display sets capital signs, Atkinson Hyperlegible Next sets the text in sentence case, and every corner is square.
STORY: The owner knows where they are (lapis), what binds the agent (marigold) and what is halted (ink) before reading a number.
FIRST VIEWPORT: The lapis board (title, agents by mode) beside the waiting request and its "Open request" action, then the agent bands of mode, identity and mandate fields.
FORM: #3 wayfinding. Fields wipe in along the reading direction, and figures roll.
DENSITY: Tight gutters and a wide column (founder, 2026-09-28); 6px seams between fields; 44px touch targets on phones.

## Unresolved

- Dark mode for Placard (light only for now).
- The Owlhead mark and favicon (founder-approved design, not agent-invented).
- The Impeccable interview and decision page did not run, because the founder was not in the loop.
