# Owlhead web

The owner's web app for Owlhead, the product's public name at owlhead.ai (DEC-201; "Mandate" stays
the codename for code and paths, and "mandate" the word for the owner's binding envelope): the shell with the always-present Stop control, the dashboard,
agent detail, and approvals. This first slice runs entirely on recorded fixture data. It talks to
no deployment, no broker, and no network service other than Supabase, for sign-in and for the
landing page's private beta requests, which is off unless it is configured (see Sign-in), and it
cannot place an order.

The screens follow the [product-experience brief](../docs/product/09-product-experience.md). The
stack is DEC-200: Next.js 16 (App Router), React 19, TypeScript strict, Tailwind CSS v4, Cloudflare's
Kumo components (`@cloudflare/kumo`, themed in `src/app/kumo-theme.css`), Base UI primitives through Kumo, Motion,
Pixelarticons icons (DEC-478), and Vitest with Testing Library.

## Run it

Node 22.18 or later, with npm; CI uses the exact release in `.nvmrc`. The Impeccable detector
(4.1.0) that CI runs needs 22.18 or later.

```sh
npm ci
npm run dev        # http://127.0.0.1:4317
npm run lint
npm run typecheck
npm test
npm run pending    # every pending test fails at its stub when un-skipped (DEC-750)
npm run build && npm start
npm run cf:build   # the Cloudflare Worker through OpenNext (DEC-823, DEC-731); npm run cf:preview runs it locally
                   # main deploys it through .github/workflows/web-deploy.yml alone, once the repository
                   # variable WEB_DEPLOY_ENABLED is true (DEC-1005)
npm run shots      # every screen as PNG files with a contact sheet in .shots/, against a running dev server (DEC-516)

npx playwright install --with-deps --only-shell chromium   # once
npm run test:e2e
npm run test:e2e:auth   # sign-in on, against a stub Supabase
npm run test:e2e:api    # the workspace API as the data source (DEC-975); npm run test:e2e runs it too
```

The e2e suite (`e2e/`, Playwright, Chromium only, in light and dark) builds the app into `.next-e2e/`
with `OWLHEAD_E2E_SCENARIOS=1` and starts it with `npm start`. That is the production build with the
scenario switch on, so a spec can load `?scenario=stale`. It reads computed styles: no route, overlay, or Kumo surface may paint a gradient or a mask, and each
Kumo override in `src/app/kumo-theme.css` must hold in the browser (DEC-200). Locally it reuses a
server already on port 4317, which may be `npm run dev`. The browser download comes from `cdn.playwright.dev`.

Next.js telemetry is off when `NEXT_TELEMETRY_DISABLED=1` is set; CI sets it. The app ships no
analytics and no session replay. Its one service worker, `public/push-sw.js` (E8-14, DEC-793),
only shows web push notifications: one of four fixed sentences for a push that carries exactly a
notice id and a text key, and on a tap it opens `/n/<notice>`. It has no `fetch` handler, so it
serves and caches nothing, and it uses no storage; `src/lib/push/worker.test.ts` pins that.
Settings › Notifications turns it on when `NEXT_PUBLIC_OWLHEAD_VAPID_PUBLIC_KEY` holds the
deployment's VAPID public key (unpadded base64url); until the workspace API takes a subscription,
the screen uses a labelled fixture sender.

### Scenarios (development and the e2e build only)

`npm run dev` shows a scenario switcher at the bottom right. It sets the `mandate-scenario` cookie,
and `?scenario=<id>` on any URL does the same. The scenarios are `normal`, `empty`, `loading`,
`stale`, `paused`, `drawdown`, `reconciliation`, `unknown-order`, `unreachable`, `approvals`, and
`result-unknown` (the deployment takes requests but never journals them). A production build always
renders `normal`: it ignores the parameter and the cookie, and does not contain the switcher, which
`npm run build` checks (`scripts/no-scenarios.mjs`). The e2e build takes the parameter and the cookie
but shows no switcher. `OWLHEAD_E2E_SCENARIOS` is read when the app is built, not when it starts.

The same panel switches the workspace role (owner, operator, approver, viewer, auditor; PX-11). The
role lives in React state only and resets on reload: approvers may pause but not stop, viewers and
auditors get no Stop control, viewers see requests read-only, and auditors see only the Audit group;
any other route renders "Not available to your role" for them instead of the page.

It also turns on colour-blind friendly gains and losses (the `mandate-cvd` cookie; `?cvd=1` or
`?cvd=0`, or Alt+Shift+C), and links to `/palette`, the palette reference, which exists only in
development.

### Sign-in (DEC-211)

Sign-in is Supabase Auth: Google (OIDC) creates the account and signs in the first time, and a
passkey signs in after that. It is on only when both Supabase variables are set and the build is not
the e2e build (`authEnabled` in `src/lib/auth-config.ts`, read when the app is built). With it off,
the app is exactly as it was: no sign-in, no redirects, and no request to Supabase; `/login` says
sign-in is off.

To run without sign-in, set neither variable (or leave `.env.local` out). To run with it, copy
`.env.example` to `.env.local`, which git ignores, and fill it in:

| Variable | What it is |
|---|---|
| `NEXT_PUBLIC_SUPABASE_URL` | The project's URL, from the dashboard's Connect dialog |
| `NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY` | The publishable key (`sb_publishable_…`, Project Settings > API Keys); never the secret key |
| `NEXT_PUBLIC_OWLHEAD_EMAIL_SIGNIN` | `1` shows the email link on `/login`; leave it unset until the project has SMTP. The deployed Worker's build reads it from the GitHub variable of that name (repository or `production` environment), like the two Supabase values (DEC-1005 item 6) |
| `NEXT_PUBLIC_OWLHEAD_SITE_ORIGIN`, `NEXT_PUBLIC_OWLHEAD_APP_ORIGIN` | The public site's and the app's origins (DEC-2405). With both set, the site serves only its public pages and sends sign-in and every app screen to the app with a 308, so the session lives on the app alone. Leave both unset locally; the Worker's build sets them in `web-deploy.yml` |

Next reads `.env.local` for `npm run dev`, `npm run build` and `npm start`; the variables are
inlined into the build, so rebuild after changing them. For the deployed Worker they are variables of
the GitHub `production` environment, which `.github/workflows/web-deploy.yml` builds with (DEC-1005
item 6). Open the app at `http://localhost:4317`
rather than `127.0.0.1`: a passkey only works on a host that matches its relying-party ID.

While sign-in is on, a signed-out visitor at `/` sees the welcome page (the address stays `/`), and
anywhere else goes to `/login?next=` and the path. `/login` offers Google and a passkey;
`/auth/callback` finishes a Google or email sign-in and goes on to `next`, by way of
`/auth/passkey` ("Add a passkey", or "Not now") when the account has none. Signed in, the account
menus and the More sheet show the address and Sign out, and Settings > Profile lists the passkeys.

The Supabase dashboard needs:

- **Authentication > URL Configuration.** Site URL `http://localhost:4317`. Redirect URLs
  `http://localhost:4317/**` and `http://127.0.0.1:4317/**`, plus any other port you run on (for
  example `http://localhost:4340/**`); at launch, `https://owlhead.ai/**`.
- **Authentication > Sign In / Providers > Google.** Enabled, with the client ID and secret of a
  Google Cloud OAuth client (type Web application) whose authorised redirect URI is
  `https://wyyxngmblqqokcvraflx.supabase.co/auth/v1/callback`.
- **Authentication > Passkeys.** Enable Passkey authentication; Relying Party Display Name
  `Owlhead`, Relying Party ID `localhost`, Relying Party Origins `http://localhost:4317` (up to five,
  so add `http://localhost:4340` for a second port). At launch the ID becomes `owlhead.ai`, and
  changing it invalidates every passkey made under `localhost`.

`npm run test:e2e:auth` (`playwright.auth.config.ts`, specs in `e2e-auth/`) builds with sign-in on
against a stub Supabase address into `.next-auth-e2e/`, starts it on port 4318
(`OWLHEAD_AUTH_E2E_PORT` changes it) and never reuses a running server. It checks what a signed-out
visitor meets, answering the browser's calls to Supabase itself. It cannot sign in: the server would
verify the session against the project's keys, which a browser stub cannot answer.

## Private beta requests

The landing page (`/welcome`, and `/` when signed out) ends with a form that asks for a place in
the private beta. It posts to `POST /api/beta` (`src/app/api/beta/route.ts`), which stores each
request, an email and an optional use, in one of two places:

- **Supabase**, in the `beta_requests` table, once you apply
  `supabase/migrations/20260929180000_beta_requests.sql` (in the dashboard's SQL editor, or with
  `supabase db push`). Row level security lets the publishable key add a row and never read one;
  read the list in the dashboard's table editor or with the secret key.
- **A local file**, `web/.data/beta-requests.jsonl`, one JSON line per request, while Supabase is
  not configured or the table does not exist yet, in development and tests only. Git ignores
  `.data/`. A production build has no file to fall back on (it runs on Cloudflare Workers,
  DEC-823): there the form is closed, and sends nothing, until both Supabase variables are set
  (DEC-987), and then answers unavailable until the table exists (DEC-731 item 4).

A repeat address counts as stored, so the form never says whether someone is already on the list.

## The workspace API client (E11-9)

`src/api/` is the typed client for the [workspace API](../docs/specs/workspace-api.md): paths under
`/v1/workspaces/{ws}`, one `Idempotency-Key` per gesture kept across retries, every failure as
`{code, effect}` with an unanswered command reported as `effect: "unknown"`, decimals refused as
JSON numbers, `as_of` watermarks on every read, and no order-placing method (DEC-528).
`src/api/mock-server.ts` is a fixture-backed `fetch` for tests, `npm run dev` and the e2e build.

### Where the screens read from (DEC-975)

The fixtures, unless the server's environment names a workspace deployment's API. It is read when a
request is served, never at build, and nothing of it is a `NEXT_PUBLIC_` variable:

| Variable | What it is |
|---|---|
| `OWLHEAD_DATA_SOURCE` | `fixtures` (the default when unset) or `api`. Anything else shows the deployment unreachable |
| `OWLHEAD_API_ORIGIN` | The API's origin: staging's is `https://api-staging.owlhead.ai` (DEC-2636); `http` only on a loopback host |
| `OWLHEAD_WORKSPACE_ID` | The workspace the app reads, until `GET /v1/me/workspaces` is wired: staging's is `01KDEM0000000000000000000W` (`deploy/gitops/clusters/staging/workspace.yaml`) |

The deployed Worker reads all three from its own variables, which the founder sets in the Cloudflare
dashboard (DEC-823 items 4 and 5). Nothing in the repository sets them, and `keep_vars` in
`wrangler.jsonc` keeps them across deploys. Unset, the Worker serves the fixtures.

Over the API (`src/lib/api-runtime.tsx`) the browser reads the workspace with its own session
cookie from the documented reads (`src/api/reads.ts`), polls them every five seconds, and sends
each command and answer through the API's idempotent write routes (`src/api/commands.ts`),
following it through `GET /commands/{id}`. A misconfigured or silent API shows the deployment
unreachable, never the fixtures. `src/api/operations.ts` lists every operation the app calls, and
`src/api/contract.test.ts` fails a call the spec does not document (E21-45). Setting up an agent is
three recorded steps (DEC-2401, DEC-2407): the version is created from the mandate document
(`POST /mandate-versions`), the confirm binds the confirmation screen the server serves for it (read
again just before), and the deploy (`POST /agents`) names an agent id the browser mints once per
set-up (`agt_` and a ULID). No model reads the owner's words over the API, since the compiler is not
served: the figures-only reader does, and the page says so. The account's room, who trades a symbol and
the independent-approval policy are not served either, so the server judges V-002, V-006 and V-047.
The confirm, when the served screen says it needs step-up, and the deploy each take passkey step-up
(DEC-2128, `src/api/passkey.ts`): the browser asks `POST /step-up/challenges` for a challenge of kind
`confirm_version` or `deploy` bound to the action's digest, reads the member's passkeys from
`GET /passkeys`, has the authenticator sign the challenge (`navigator.credentials.get`, user verification
required, the page's own host as the RP ID), and sends `step_up` with `step_up_assertion` beside it. A
canceled check, a browser without WebAuthn, no passkey, or one still in its first 24 hours sends nothing
and says so. Until the deployment serves its registered instruments and signal models, the create step
over the API sends nothing rather than a made-up asset id or model hash, and says why; changing a mandate is not sent yet,
and the charts and sample headlines are not drawn over the API: DEC-975 has the detail, including the
provisional read bodies the API server serves until `schemas/workspace-api/read-models/` lands.
Approving a request is step-up by `cli_confirm` (DEC-2104, DEC-2103 item 7): Approve first asks
`POST /step-up/challenges` for a challenge bound to the request's content hash and shows its code
(`XXXX-XXXX`, the challenge's first 40 bits) with the `mandate step-up confirm` line, naming the session's principal from
`GET /v1/me/session`, that the owner runs on the deployment's host. The browser cannot confirm it.
Its Approve then sends the grant with that challenge's evidence, and the screen says what the server answered
(`step_up_missing` while the host has not confirmed). A code's five minutes are counted from when it
was shown, never against this device's clock; paper only, and Skip still carries no step-up. The
mock API stands in for the host's confirmation. Connections lists every connection
`GET /connections` serves with its state, and the owner connects Alpaca paper trading there (DEC-2623): the
screen shows the token-breadth disclosure in DEC-2622's version 1 wording, asks for a code of kind
`connection` bound to that version (DEC-2621 item 4) with the `mandate step-up confirm-connect` line
(DEC-2125), then sends `POST /connections/oauth/start` and the browser to the authorization page it
answers, an `https` page only. Alpaca returns the browser to the API's callback, which redirects to
`/connections/{id}`, read until it settles. Stop-all and Close-all take only an established connection
as their scope, and the workspace scope when none is. An agent's screen lists its orders, newest first, and the net quantity its own fills hold per
instrument, from `GET /agents/{id}/orders` and `/positions` (DEC-2111), read every five seconds; a
state the journal has not given is "not reported", a list that cannot be read is never shown as
empty, and an agent with no account found says so. It also lists the latest 20 records on the
agent's stream from `GET /journal/streams/agent:{ws}:{agent}/events`, by sequence, type and time,
never a body (DEC-2377). Positions (`/positions`) reads each agent's `GET /agents/{id}/positions` every five seconds and
groups the agents by the account their reads name: each agent's own net quantity per instrument, its
cost basis, risk mark and protection shown as not reported until the API serves them (DEC-2111 item
5), and the account's holdings at its last reconciliation from the same reads.

Stop works over the API too (DEC-2408): the sheet lists the agents the API serves, each with its kill
switch, and the record screens build from the API's view, with open orders and per-agent positions
shown as not reported, never as none. Release is not offered there. `/demo` always reads the fixtures. With sign-in on, the browser first opens an API session with
`POST /v1/sessions` from its Supabase access token and refreshes it every four minutes, one tab at a
time (`src/api/session.ts`, DEC-1095); a read answered `membership_unavailable` shows that state, and no session
shows "Sign in to see your workspace".

To try it locally against the mock API the dev server serves at `/v1`:

```sh
OWLHEAD_DATA_SOURCE=api OWLHEAD_API_ORIGIN=http://127.0.0.1:4317 OWLHEAD_WORKSPACE_ID=ws_dev npm run dev
npm run test:e2e:api   # the e2e build with the API as its source, on port 4319
```

The scenario switcher's cookie picks what the mock API serves. A production build answers 404 at
`/v1` (`scripts/no-scenarios.mjs`).

#### Signing in to a dev cell (DEC-2406)

A dev or CI cell's API trusts E21-52's stub issuer, not Supabase. Under `next dev` only, two more
server-side variables make the stub the sign-in client's stand-in: the dev server asks the stub for
the subject's token on loopback at `POST /dev/stub-token`, and the browser trades it at
`POST /v1/sessions` as it would Supabase's.

| Variable | What it is |
|---|---|
| `OWLHEAD_STUB_ISSUER` | The stub's issuer exactly as it printed it: `http://127.0.0.1:<port>` or `http://[::1]:<port>` |
| `OWLHEAD_STUB_SUBJECT` | The demo subject an operator linked for that issuer (`mandate identity link --issuer`) |

A production build compiles no such route (`route.dev.ts`), its middleware answers 404 under `/dev/`,
and `scripts/no-scenarios.mjs` checks the e2e and production builds both answer 404 there.

## The mock-data rule

- Every figure comes from typed fixtures in `src/fixtures/`, which mirror
  `schemas/mandate.schema.json`: decimals are strings, IDs are opaque (ULIDs with a type prefix),
  and content references are `sha256:` hashes. The two main mandates are
  `reference/mandate/bases.py`'s `btc_accumulator` and `two_stock_swing`.
- The status strip carries a "Fixture data" tag (on a phone, the More sheet and the foot of the
  page do), and P&L values are fixture values, not a record.
- Money never passes through a float: `src/lib/decimal.ts` works on scaled integers.
- `src/lib/mock-runtime.tsx` stands in for the workspace deployment. Commands and approval responses
  are held in memory and shown as recorded only after a delay, so the screens never display an
  optimistic result.
- Nothing about approvals, positions, or mandates is written to `localStorage`,
  `sessionStorage`, or a cache. The app stores nothing in the browser, apart from Supabase's
  session cookies while sign-in is on.
- Charts draw seeded, deterministic fixture bars and equity (`src/fixtures/market.ts`), consistent
  with the fixture's fills, positions and equity. They are TradingView Lightweight Charts
  (Apache-2.0); the credit is in `web/NOTICE`, under the dashboard's account chart and on `/design`.
- Compliance text appears only as the placeholders `[[DISCLOSURE-PERFORMANCE]]`,
  `[[LEGEND-HYPOTHETICAL]]`, and `[[RETAIL-AUTO-LIVE]]`. Every P&L figure (a signed gain or loss,
  or a dollar figure labelled realized, unrealized, P&L, gain or loss) has
  `[[DISCLOSURE-PERFORMANCE]]` in the same section, card or list item: behind an info symbol that
  opens it on hover, click, tap or Enter, and inline on record screens and in print (DEC-210);
  `src/app/disclosure.test.tsx` renders every route in every scenario and fails otherwise. Counsel
  must confirm the symbol before launch; `PERFORMANCE_INLINE` in
  `src/components/domain/placeholders.tsx` puts the text back inline everywhere.

## Design system

The visual system is the calm, consumer-grade redesign of DEC-204: one hero number per screen, a
scrubbable equity chart at the centre, generous space, few boxes, soft corners, two densities (calm
and dense), in Ink and Ultramarine (DEC-205), light or dark, and with no gamification. `web/DESIGN.md` holds the rules, the tokens and the do and don't list, and
`web/design/reference.md` how each surface is drawn (DEC-511); `web/PRODUCT.md` holds the audience, voice, and the safety
rules that constrain visuals. `/design` (not linked from the navigation) renders the tokens with
their OKLCH values and computed contrast, the type scale, the spacing in both densities, the radius scale,
 every state treatment, the components, and motion samples.

- **Colour means one thing each** (Ink and Ultramarine, `web/COLOR.md`). Ultramarine is your mandate and the
  account's line, ink is the account's actions, a stopped agent and the Stop control (an outline
  until something needs you, then filled; DEC-206), crimson is the kill switch and nothing else. Gains and losses are the only other hues, as text beside a sign and the word. Values live
  once in `src/lib/palette.ts`, for both themes, mirrored in `src/app/globals.css`; `tokens.test.ts`
  and `palette.test.ts` fail if they drift or a pair drops below WCAG AA or APCA in either theme.
- **APCA is dev only.** `apca-w3` (its own limited licence) and its AGPL-3.0 dependency
  `colorparsley` are dev dependencies used only by the contrast tests (`src/test/apca.ts`). Lint
  bans them in app code, and `npm run build` ends with `scripts/no-apca.mjs`, which fails if either
  reached `.next`. `/design` and `/palette` show WCAG ratios only.
- **Light and dark.** Light, Dark or System from the theme menu in the header, kept in the
  `owlhead-theme` cookie. The dark theme is tokens alone: there is no `dark:` class in `src/`.
- **No gradients** (DEC-200). CI fails on any gradient in `src/` and runs the Impeccable detector
  (`.github/workflows/web.yml`).
- **Type.** Public Sans for everything (DEC-209), in sentence case, with tabular figures and a
  plain zero (the display hero figure alone is proportional, DEC-208); weight 600 at most. Self-hosted through `@fontsource-variable/public-sans`.
- **Brand.** The founder's Owlhead mark and the lowercase "owlhead" wordmark, as outlines
  (DEC-203), ink on light and off-white on dark (DEC-204), with no tagline. The icons, favicons and
  share image are the ink mark on off-white. `npm run brand` regenerates the favicons, app icons, share image and manifest in
  `public/` from `src/components/brand/owlhead-mark.svg` and `brand/og-image.svg`; commit its output,
  because `brand-assets.test.ts` fails when a committed file differs. `web/design/reference.md` ("Brand")
  has the detail.
- **Motion.** Emil Kowalski's rules: ease-out `cubic-bezier(0.23, 1, 0.32, 1)`, interactions under
  300 ms, press to 0.97 with a faster release, lists rise in once with a 30 ms stagger, changed numbers
  roll whole, and the equity line draws in once on load. Deadlines never move. `prefers-reduced-motion` drops movement and keeps colour changes.
- **Components.** Kumo components are imported one at a time (`@cloudflare/kumo/components/*`; the
  root barrel is lint-banned) and themed by `src/app/kumo-theme.css`. Blocks added with
  `npx @cloudflare/kumo add` land in `src/components/kumo/` (`kumo.json`). The app's own components
  are in `src/components/`. `web/design/reference.md` ("Kumo") lists the token mapping and the parts of Kumo
  the app does not use.
- **The frame** (DEC-208, DEC-215). The header and the phone tab bar are frosted glass, and
  nothing else is but the bar that joins them on desktop: from 64rem a labelled floating dock
  replaces the sidebar. Nothing runs under the header while every feed answers; the status strip
  shows there while a feed is stale or failing, and on record screens. The header carries a wide command bar and breadcrumbs
  that fold rather than truncate the current page. Stop's desktop pill and the command bar are
  both 40px; Stop's tap area stays 44px.
- **Navigation.** `src/lib/screens.ts` lists every screen with its purpose. The dock, the phone's
  tab bar and More sheet, the breadcrumbs, ⌘K, and the route-coverage test read it, so no link points at a missing page;
  screens not built yet say "Coming in the next slice" with what they will be for.
- **The phone is a remote control** (DEC-207). Below 64rem the header holds the mark, the paper
  badge and Stop; four tabs (Home, Approvals, Agents, More) are the one navigation, and More holds
  Search, the account switcher and every other screen. Home leads with what needs you, agents show
  their headroom rather than P&L, an agent page is its state, equity and headroom with its sections
  as links, and a request is one screen with Approve and Skip pinned. Tablet and desktop are
  unchanged. `web/DESIGN.md` has the rules and `web/design/reference.md` ("Phone") the layout; `e2e/phone.spec.ts` checks them at 320, 375,
  390 and 430px.
