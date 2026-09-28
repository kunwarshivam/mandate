# Owlhead web

The owner's web app for Owlhead, the product's public name at owlhead.ai (DEC-201; "Mandate" stays
the codename for code and paths, and "mandate" the word for the owner's binding envelope): the shell with the always-present Stop control, the dashboard,
agent detail, and approvals. This first slice runs entirely on recorded fixture data. It talks to
no deployment, no broker, and no network service, and it cannot place an order.

The screens follow the [product-experience brief](../docs/product/09-product-experience.md). The
stack is DEC-200: Next.js 16 (App Router), React 19, TypeScript strict, Tailwind CSS v4, Cloudflare's
Kumo components (`@cloudflare/kumo`, themed to Placard), Base UI primitives through Kumo, Motion,
Phosphor icons, and Vitest with Testing Library.

## Run it

Node 22.18 or later, with npm; CI uses the exact release in `.nvmrc`. The Impeccable detector
(4.1.0) that CI runs needs 22.18 or later.

```sh
npm ci
npm run dev        # http://127.0.0.1:4317
npm run lint
npm run typecheck
npm test
npm run build && npm start

npx playwright install --with-deps --only-shell chromium   # once
npm run build && npm run test:e2e
```

The e2e suite (`e2e/`, Playwright, Chromium only) starts the production build with `npm start` and
reads computed styles: no route, overlay, or Kumo surface may paint a gradient or a mask, and each
Kumo override in `src/app/placard-kumo.css` must hold in the browser (DEC-200). Locally it reuses a
server already on port 4317. The browser download comes from `cdn.playwright.dev`.

Next.js telemetry is off when `NEXT_TELEMETRY_DISABLED=1` is set; CI sets it. The app ships no
analytics, no session replay, and no service worker.

### Scenarios (development only)

`npm run dev` shows a scenario switcher at the bottom right. It sets the `mandate-scenario` cookie,
and `?scenario=<id>` on any URL does the same. The scenarios are `normal`, `empty`, `loading`,
`stale`, `paused`, `drawdown`, `reconciliation`, `unknown-order`, `unreachable`, `approvals`, and
`result-unknown` (the deployment takes requests but never journals them). A production build always
renders `normal`.

The same panel switches the workspace role (owner, operator, approver, viewer, auditor; PX-11). The
role lives in React state only and resets on reload: approvers may pause but not stop, viewers and
auditors get no Stop control, viewers see requests read-only, and auditors see only the Audit group;
any other route renders "Not available to your role" for them instead of the page.

It also turns on colour-blind friendly gains and losses (the `mandate-cvd` cookie; `?cvd=1` or
`?cvd=0`, or Alt+Shift+C), and links to `/palette`, the palette reference, which exists only in
development.

## The mock-data rule

- Every figure comes from typed fixtures in `src/fixtures/`, which mirror
  `schemas/mandate.schema.json`: decimals are strings, IDs are opaque (ULIDs with a type prefix),
  and content references are `sha256:` hashes. The two main mandates are
  `reference/mandate/bases.py`'s `btc_accumulator` and `two_stock_swing`.
- The status strip carries a "Fixture data" tag, and P&L values are fixture values, not a record.
- Money never passes through a float: `src/lib/decimal.ts` works on scaled integers.
- `src/lib/mock-runtime.tsx` stands in for the workspace deployment. Commands and approval responses
  are held in memory and shown as recorded only after a delay, so the screens never display an
  optimistic result.
- Nothing about approvals, positions, or mandates is written to `localStorage`,
  `sessionStorage`, or a cache. The app stores nothing in the browser.
- Charts draw seeded, deterministic fixture bars and equity (`src/fixtures/market.ts`), consistent
  with the fixture's fills, positions and equity. They are TradingView Lightweight Charts
  (Apache-2.0); the credit is in `web/NOTICE`, under the dashboard's account chart and on `/design`.
- Compliance text appears only as the placeholders `[[DISCLOSURE-PERFORMANCE]]`,
  `[[LEGEND-HYPOTHETICAL]]`, and `[[RETAIL-AUTO-LIVE]]`.

## Design system

The visual system is **Placard** (the founder's pick, 2026-09-28): transit signage, flat colour
fields, square corners, big display type. `web/DESIGN.md` holds the rules, the tokens, the state
treatments, and the do and don't list; `web/PRODUCT.md` holds the audience, voice, and the safety
rules that constrain visuals. `/design` (not linked from the navigation) renders the tokens with
their OKLCH values and computed contrast, the type scale, the spacing before and after the gutter
change, every state treatment, the components, and motion samples.

- **Colour means one thing each** (navy and brass, `web/COLOR.md`). Brass is your mandate, navy is
  the account, ink is a stopped agent and the Stop control, crimson is the kill switch and nothing
  else. Gains and losses are the only other hues, as text beside a sign and the word. Values live
  once in `src/lib/palette.ts`, mirrored in `src/app/globals.css`; `tokens.test.ts` and
  `palette.test.ts` fail if they drift or a pair drops below WCAG AA or APCA.
- **Light only.** Dark mode is follow-up work; there is no theme toggle.
- **No gradients** (DEC-200). CI fails on any gradient in `src/` and runs the Impeccable detector
  (`.github/workflows/web.yml`).
- **Type.** Big Shoulders Display for headings and big figures, in capitals; Atkinson Hyperlegible
  Next for everything read, in sentence case. Self-hosted through `@fontsource-variable/*`.
- **Motion.** Emil Kowalski's rules: ease-out `cubic-bezier(0.23, 1, 0.32, 1)`, nothing past 300 ms,
  press to 0.97 with a faster release, fields wipe in once with a 30 ms stagger, changed numbers roll
  whole. Deadlines never move. `prefers-reduced-motion` drops movement and keeps colour changes.
- **Components.** Kumo components are imported one at a time (`@cloudflare/kumo/components/*`; the
  root barrel is lint-banned) and themed by `src/app/placard-kumo.css`. Blocks added with
  `npx @cloudflare/kumo add` land in `src/components/kumo/` (`kumo.json`). The app's own components
  are in `src/components/`. `web/DESIGN.md` ("Kumo") lists the token mapping and the parts of Kumo
  the app does not use.
- **Navigation.** `src/lib/screens.ts` lists every screen with its purpose. The sidebar, the
  breadcrumbs, ⌘K, and the route-coverage test read it, so no link points at a missing page;
  screens not built yet say "Coming in the next slice" with what they will be for.
