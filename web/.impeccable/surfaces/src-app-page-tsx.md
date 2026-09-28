---
version: 1
slug: "src-app-page-tsx"
primary_target: "src/app/page.tsx"
related_targets: ["src/app/agents/[agentId]/page.tsx","src/app/approvals/[approvalId]/page.tsx","src/components/shell/app-shell.tsx","src/components/stop/stop-sheet.tsx","src/components/charts/equity-chart.tsx"]
---

## Scope

The shell, the Stop sheet, Home (`/`, D1), agent detail (`/agents/[id]`, D2) and the approval request (`/approvals/[id]`, D6, phone first). Visitor mode: Operate. The founder asked on 2026-09-28 for a consumer-grade redesign (DEC-204): Robinhood, Public or Wealthfront polish, with no gamification. It supersedes Placard's structure and signage type; the navy and brass palette (DEC-202) and the logo (DEC-203) stay.

## Audience, job, constraints

The audience is Jordan, Alex, Priya and Dana (see PRODUCT.md). On Home they need to know in one glance what the account is worth, whether anything needs them, and whether each agent is inside its mandate. On an agent, how it is doing against its limits. On the approval request, they decide in time or let the default (skip) apply.

The constraints are P1-P10, DEC-200 to DEC-204 and the brief (`docs/product/09-product-experience.md`, which wins over design):
- Stop is always visible and never disabled, at every width and during transitions; crimson belongs only to the kill switch.
- Approve and Skip are identical; the deadline is static.
- Nothing optimistic; frozen records; role gates; generic titles and opaque IDs.
- Paper is labelled everywhere; `[[DISCLOSURE-PERFORMANCE]]` sits beside every P&L.
- No gradients, no gamification, no default Inter/Geist/Roboto.

## Direction contract: Calm

THESIS: One number, one line, room to breathe. It refuses the card-grid dashboard: no tiles, no bands, no signage; space separates, hairlines second, boxes only when they mean something.
OWN-WORLD: A near-white page, slate wells, a navy account line over a pale navy fill, brass only where the mandate speaks (pale brass field, dashed brass price lines), ink pills for stopped agents and Stop. Mona Sans in one tight scale, sentence case, 600 at most. Pill buttons and chips, 20px panels.
STORY: The owner reads their equity, scrubs back through the day, sees the agents inside their limits, and knows what is waiting for them.
FIRST VIEWPORT: The hero equity figure with its change and disclosure, the scrubbable chart and ranges beneath, "Waiting for you" in the right rail on desktop; Stop dominant at the top right.
FORM: Code-led consumer finance, founder-specified (no seed roll).
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved

- Dark mode (light only for now).
- The list screens (agents, approvals inbox, positions, records, account, audit and settings indexes) are restyled in the calm system but not yet redesigned screen by screen.
- The Impeccable interview did not run as a live conversation; the founder's brief stood in for it.
