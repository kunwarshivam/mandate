# web

DEC-2400 to DEC-2499. The rules in force:

- **The web app talks only to the workspace API**: set-up creates the version and sends the
  confirm; the Stop sheet and kill-switch record screens build from what the API serves; a
  backtest reads as `read-models/backtest` serves it (DEC-2400, DEC-2401, DEC-2407, DEC-2408).
- **Sign-in and the app share one app origin**; the public site never holds the session; a dev
  cell's `next dev` takes the sign-in token from the stub issuer, and no build serves it
  (DEC-2405, DEC-2406).
- **Confirmations stay plain**: the approval's action is the page title, so the risk sits with
  the choice; the web kill switch asks for a plain confirm, never a passkey (DEC-2409, DEC-2410).
- **Cool-off at the server**: a test or staging demo browser may try a cooling-off passkey, and
  the server decides — staging's ceiling is a fixed instant no later than its server's
  (DEC-2411, DEC-2412).


<a id="dec-2400"></a>

## DEC-2400: The web client reads a backtest as `read-models/backtest` serves it, on both data sources, and shows its report as figures, codes and named placeholders

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | The `claude-code` builder of lane W2, under DEC-79 and DEC-176, on the coordinator's direction of 2026-10-17 |
| **Story or claim** | E4-5's web client, claim [#1530](https://github.com/kunwarshivam/mandate/issues/1530); product brief B2; PRD FR-4.2; workspace API spec §4.2; DEC-1000, DEC-1001, DEC-1003, DEC-1096 |

### Decision

1. **The decoder is the schema.** `web/src/api/backtest.ts` decodes `read-models/backtest` exactly as
   `schemas/workspace-api/read-models/backtest.schema.json` states it: every required member; the
   closed enums (state, outcome, purpose, mode, `sharpe_sign`, `absent`, the limitation codes, the
   disclosure names) and the report's constants (`kind`, `report_version` 2, `subject`); the nine
   outcome counts as nine required members; `gate_reasons` and `hold_reasons` as maps from a `Code`
   to a count; `limits_latched` by its pattern and unique; `limitations` and `disclosures` with at
   least one item and unique; decimals as canonical strings (a JSON number is refused), nullable
   only where the schema allows `null`; counts as safe non-negative integers; `periods_per_year`
   from 1 to 366; dates, timestamps, content refs, the lowercase asset id, `decided_by`'s pattern;
   `as_of` with at least one watermark (`AsOf`'s `minItems`, checked here; each watermark's members
   go through the shared `watermark` decoder, unchanged by this slice); and the state's conditionals
   (`completed`: no refusal, a `report_ref` and a `report`; `refused`: a refusal code and neither;
   `running`: none of the three). A well-typed value the schema still refuses (a member present or
   absent against its state, `periods_per_year` out of range, an answer for another id) is
   `not_canonical`; `wrong_type` is kept for a value of the wrong JSON type. Unknown members are
   ignored (spec §3.2), as every other read is.
2. **The oracle is the schema's own examples.** The test applies every variant of
   `read-models.backtest.valid.json` (each must decode) and of `read-models.backtest.invalid.json`
   (each must be refused) to the example. The client reads eleven invalid variants by design, each
   of which only adds a member the schema does not name, which spec §3.2 has a client ignore:
   "an unknown top-level member", "an unknown backtest member", "an unknown report member", "an
   unknown input digest", "an unknown metric", "an unknown decisions member", "an unknown outcome
   count", "an unknown row member", "an unknown risk member", "an unknown equity member" and "an
   unknown equity point member". The test derives the set of add-only variants from the files
   (a new key in the two reason maps is an entry, not an unknown member) and requires it to equal
   this list, so a new invalid variant is refused unless it is added here on purpose. Nothing an
   unknown member carries is read or shown: a served `hit_rate` is dropped with the rest. The
   envelope's two `as_of` variants are refused by the body's `AsOf` check, so none is exempt. Each
   refusal must also name a member at or under a pointer the variant changed, so a variant refused
   for the wrong reason fails; the one widening is the state's conditionals, which may refuse at a
   sibling among `state`, `refusal`, `report_ref` and `report`. Seeded bugs (`limitations` allowed
   empty, 367 periods a year allowed, a conditional refused at `/backtest` rather than its member)
   each fail the test.
3. **One read, both runtimes.** `Runtime.readBacktest(id)` reads one backtest. The API runtime calls
   `GET /backtests/{id}` (listed in `web/src/api/operations.ts` as spec §4.2's read) when a screen
   asks for it, not with the workspace's refresh, and refuses an answer for another id
   (`invalid_response` at `/backtest/backtest_id`). The fixture runtime holds its backtests in the
   served shape (`web/src/fixtures/backtests.ts`) and passes them through the same decoder and the
   same id check; the mock API serves the same bodies at the same route. A 404 is "Backtest not
   found", and an answer the decoder refuses shows the problem and the member's path, and nothing
   from the answer, its value included. A failure inside `gate_reasons` or `hold_reasons` names the
   entry as `…/gate_reasons/*` or `…/hold_reasons/*`, never by its served key, so no served text
   reaches the screen through the path.
4. **The route and the section.** `/backtests/{id}` is one screen for both data sources
   (`BacktestReportScreen`), named "Backtest" by kind, open to whoever may see agents. On the
   fixtures, an agent's `prove/backtests` section is built and lists that agent's backtests from an
   explicit map, each linking to its report. Over the API the section keeps "Not available from your
   deployment yet": the API has no route that lists backtests, and nothing is filled from the
   fixtures. The fixtures hold a completed, a running and a refused run on the agents (LMN's
   version twice, bitcoin's refused `asset_class_not_supported` by DEC-1000 item 8) and the schema's
   research-agent example, which no agent lists: users do not see the research agent until the
   DEC-99 evaluation passes (PX-9 (a)), so it is served by its id and tested, never linked.
5. **What the report shows.** Each disclosure as its visible placeholder, in DEC-1003's order
   (`NOTE-BACKTEST-MECHANICS`, `NOTE-BACKTEST-LIMITS` are added to `placeholders.tsx`, the only place
   names are written), never fewer than DEC-1003 item 1 requires whatever the server lists: the legend
   `[[LEGEND-HYPOTHETICAL]]` always and first, `NOTE-BACKTEST-LIMITS` on every report, and
   `NOTE-BACKTEST-MECHANICS` on every research-agent version's report, with any other name the report
   lists. A run with no report (running or refused) shows the legend only: `NOTE-BACKTEST-LIMITS` and
   `NOTE-BACKTEST-MECHANICS` go on a report, as DEC-1003 item 1 says. This floor only adds placeholders
   and adds no risk (DEC-176). The header line "Simulated:
   this mandate version's own decisions, run over past prices." is a neutral label for what the page
   is (DEC-1003 item 2's "labelled as simulated"), not legal text, which stays in the placeholders; the run's state, id, mandate
   version, and for a completed run its report ref and instrument (the asset id: nothing is looked up
   in the fixtures), a refused run's code verbatim, and "No report yet." for a running one; FR-4.2's
   table for this version and buy-and-hold (dates, periods, periods a year, starting and ending
   equity, net P&L with the performance disclosure beside it, total return, volatility and the
   Sharpe ratio per period and annualized as served, maximum drawdown as a fraction and in dollars,
   turnover, fees, fills) and the excess total return; the decisions (the count, the nine outcome
   counts, the gate and hold reason counts, and one row per decision: date, purpose or "hold",
   outcome, reason, decided by, quantity, limit, outputs used, each row headed by its date, in a
   named, focusable region that scrolls sideways on a phone); the risk section (limits latched,
   risk exits, most restrictive mode); the limitations as their codes; and the input digests.
6. **Nothing is computed or worded.** Every figure is the served decimal written out exactly, by
   moving the point for a percentage and grouping digits, with no rounding and no arithmetic, so no
   money or quantity passes through a float. A `null` statistic is "Not computed" with the block's
   `absent` code. Codes stay codes and disclosures stay placeholders: no drafted legal text, no
   adjective, no projection or expected figure, no statement about the platform, and no
   thesis-quality figure (DEC-1001 item 3, DEC-1003 item 2). Net P&L is a sign, a dollar figure and
   the word, in the neutral ink.

### Not done

- **The equity per period** (`report.equity`) is decoded and not drawn: a chart is the next slice.
- **B1, running a backtest** (`POST /backtests`), is not sent from the web client.
- **A list of backtests over the API** waits for a list route; the section keeps its notice.

### Rationale

The report's schema and examples exist before either the server or the client, so the client can
be held to them exactly: a report that does not validate is refused, not shown in part. One screen
over one decoder, fed either by the API or by fixtures in the served shape, keeps the fixtures from
showing anything the API could not serve. Writing served decimals out exactly keeps the figures the
record's own, which is what FR-4.5's reproducibility relies on.

### Alternatives considered

- **Exempting the variants by reading only the body.** The body carries `as_of` with `AsOf`'s
  `minItems: 1`, so the decoder checks it and no `as_of` variant needs an exemption.
- **Refusing unknown members in the report.** Stricter, but it breaks spec §3.2 for every newer
  server, and nothing unknown is shown either way.
- **Formatting figures through `lib/decimal.ts`.** It holds 12 places, and a served decimal may have
  28; rounding would show another figure than the record's.
- **Listing the research-agent run under an agent on the fixtures.** Refused under PX-9 (a).

<a id="dec-2401"></a>

## DEC-2401: The web client sets up an agent in three recorded steps, create the version, confirm it, deploy it to paper, and over the API sends none of them until the routes and reads they need exist

| | |
|---|---|
| **Status** | Accepted; item 4 superseded by [DEC-2407](web.md#dec-2407) for steps 1 and 2 over the API |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | The `claude-code` builder of lane W2, under DEC-79 and DEC-176, on the coordinator's direction of 2026-10-17 ("Add a deploy flow (create version, confirm, deploy) behind the data-source switch on fixtures, for the routes lane A2 is building") |
| **Story or claim** | E10-11 and E10-12's web client, claim [#1530](https://github.com/kunwarshivam/mandate/issues/1530); product briefs A5 and B3; PRD FR-4.4; workspace API spec §4.1, §4.2, §5.1, §3.4, §3.6; DEC-477, DEC-1096, DEC-2400 |

### Decision

1. **Three steps, each its own record.** Under a new agent's summary card (DEC-477), setting it up
   is three steps, in order, each offered only once the one before it is recorded: create the
   version (`POST /mandate-drafts/{id}/versions`, `MandateVersionCreated`), confirm it (A5,
   `POST /mandate-versions/{hash}/confirm`, `MandateConfirmed`), and deploy it to paper (B3,
   `POST /agents`, `AgentDeployed` or `DeploymentRejected`). The runtime holds them on one `SetUp`
   record (`createVersion`, `confirmVersion`, `deployVersion`), each step with its own phase
   (`sent`, `recorded`, `rejected`, `undelivered`, `unknown`) and time; `Runtime.deploy` and the
   one-gesture `Deployment` are gone. The card's lines and record are unchanged; each step is gated on
   `agents.deploy`, and the workspace's independent-approval policy (V-047) and a draft the account
   no longer fits (V-002, V-006) offer no step at all, as before.
2. **A first version is risk-increasing.** A new agent's first version has no base
   (`base_version: null`). Its classification is `risk_increasing`, the reading that only tightens
   (DEC-176): every path goes from absent to set, and mandate spec §9.2 counts a larger maximum and
   every unlisted path as increasing. Confirming it therefore needs step-up, bound to SHA-256 of the
   canonical `{mandate_version, agent_id, base_version}` (§5.1, §3.6), which the confirm step records
   (`stepUp`). The version step shows the hash, that the validation passed, and the classification
   in words.
3. **On the fixtures, the in-memory journal judges each step.** Creating the version repeats the
   validation (V-047, V-002, V-006) and is refused in words if it fails, so no version exists. A
   confirm is refused `not_created` unless its version is recorded, and a deployment `not_confirmed`
   unless its confirmation is, before anything else is judged. A risk-increasing confirm without its
   step-up is `step_up_required`, as the API answers it; the in-memory journal takes the passkey
   check's digest as the step-up, so only here does a confirm record one. V-047, V-002 and V-006 are
   checked again as a deployment applies (`DeploymentRejected`). The `unreachable` and
   `result-unknown` scenarios make a step `undelivered` or `unknown`; in `result-unknown` the journal
   never answers, so an unknown step stays unknown, as a command does there.
4. **Over the API, no step is sent yet.** Version creation and deployment have no request schema yet
   (lane A2 is building their routes), so no body is invented: each answers `undelivered` with
   `not_sent`, and the screen says "Not sent: your deployment does not take this step from here yet",
   with what does and does not exist. The confirm waits too: its `screen_digest` and
   `classification_shown` are the values `GET /mandate-versions/{hash}/confirmation` serves, which the
   client does not read yet, and a digest computed from the client's own screen would claim it
   rendered the server's data when it did not. `confirmVersion` refuses `not_created` for a version
   not created (always, over the API today), and otherwise answers `not_sent`; it records no step-up,
   and the screen asks for no passkey over the API for a step it will not send. `prepareConfirm` is
   ready for the read: it builds exactly `confirm-request.schema.json`'s members, `agent_id` and
   `base_version` (`null` for a new agent), `confirmed_paths` (every path the card shows with a value,
   once each; a row with no provenance, "None chosen yet" or "Not set yet", is not one),
   `warnings_acknowledged` (empty: the card
   shows no warning), `record` (`{artifact, ui_build}`), `step_up` (`null` until E9-4), and
   `screen_digest` and `classification_shown` passed through from the served confirmation, never
   computed. `contract.test.ts` checks that body against the schema with the other commands, and
   `confirm.test.ts` against the schema's example. The mock API refuses every confirm `not_found`: it
   holds no version and serves no confirmation to judge a digest or classification against.
5. **The 202 is decoded as `confirm-accepted`, ready for when the confirm is sent.** `confirmAccepted`
   reads the envelope and the confirm's members as the schema states them (`api_version` v1, `build`,
   `served_at`, `as_of` with at least one watermark, `command_id` an event id, `phase` `recorded`, the
   closed `classification` and `applies`). The test applies every valid and invalid variant of the
   schema's examples; the two add-only invalid variants ("an extra member in the body", "an extra
   member in /as_of/0") are read, as spec §3.2 has a client ignore unknown members, and the test
   requires that list to be exactly the add-only variants. The client's command outcome keeps the 2xx
   body for this. Nothing calls it yet.
6. **The deploy step.** It shows the version, the backtest shown, and "Paper only: simulated funds.
   Going live is not available."; no go-live action exists (B5 is `live_unavailable`). The backtest
   is the fixtures' completed run of exactly that version (`backtestOfVersion`; never a research-agent
   run, PX-9 (a)), linked to `/backtests/{id}` from DEC-2400, or "None: no backtest of this version
   has run."; over the API none, since no route lists a version's backtests. The record
   (`screen: "B3"`, `environment: "paper"`) keeps every line shown and the backtest id. A rejection
   (V-002, V-006, V-047) is shown in words with the rule only as an attribute (C-6).
7. **Truthful at every step.** Nothing reads as recorded before the runtime says so. A refusal or a
   step not delivered says what was and was not recorded ("Version 1 was created and is not
   confirmed, and no agent exists"); a server's code stays out of the sentence. Only a step not
   delivered because the deployment did not answer says to try again; an unknown result never does,
   and an unknown deployment says to look in All agents first. The composer locks once the
   confirmation is sent and unlocks if the deployment is refused, so the owner can change the draft;
   a card replaced by a change says whether its version was created or confirmed. A toast follows
   each recorded step.

### Not done

- **The confirm over the API** waits for `GET /mandate-versions/{hash}/confirmation` (its
  `screen_digest` and classification), for lane A2's version-creation schema (no version exists to
  confirm before it), and for E9-4's step-up evidence (a risk-increasing confirm without it is
  `step_up_required`).
- **Version creation and deployment over the API** wait for lane A2's request schemas; then
  `createVersion` and `deployVersion` send them, and the API screens can offer the set-up.
- **Following an unknown step** to its outcome (`GET /commands/{id}`) is not built: no step can be
  unknown over the API while none is sent, and the fixtures' unknown is the journal not answering.
- **V-032 (loss carry)** is not checked on the fixtures: a new agent carries no loss.

### Rationale

The coordinator asked for the three operations the API spec defines to be three gestures, so the
journal's three records are each the owner's own act and each is shown only once it exists. The
confirm is the one step with a request schema, so its body is built and checked exactly, but it
binds values only the server's confirmation read gives; until that read and lane A2's schemas
exist, every step over the API says it was not sent rather than send a guess.

### Alternatives considered

- **Keeping `deploy` as a wrapper over the three steps.** Every caller moved to the steps, so a
  wrapper would be a second path to the same records with no user.
- **Classifying a first version as neutral.** It would let a deployment's confirmation skip step-up;
  refused under DEC-176.
- **Sending the version and deployment with bodies drawn from the spec's prose.** Refused: the
  coordinator's direction is not to invent them, and a body the server later defines differently
  would be refused or, worse, read differently.
- **Sending the confirm with a digest of the client's own screen.** Refused (the review of this
  slice): the digest's meaning is that the screen rendered the server's data, which a client-made
  value can never show, so the confirm waits for the served confirmation.

<a id="dec-2405"></a>

## DEC-2405: Sign-in and the app are served from one app origin; the public site never holds the session

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, lane W1 builder), under DEC-176: it narrows where the session cookie lives and changes no credential, spending or compliance text. Asked for by the coordinator for a bug the founder reported |
| **Story or claim** | E21-38, lane W1; DEC-211 ([decision log](../04-decision-log.md)) (sign-in, host-only cookies, `next` stays a same-origin path); [DEC-823](earlier-600-899.md#dec-823) item 6 |

### Context

One Worker serves `owlhead.ai`, `www.owlhead.ai` and `app.owlhead.ai`. Supabase's session cookies
are host-only (DEC-823 item 6). So signing in on `owlhead.ai/login` set the session on
`owlhead.ai`, and `app.owlhead.ai` stayed signed out.

### Decision

1. **Two origins, from the build.** `NEXT_PUBLIC_OWLHEAD_SITE_ORIGIN` (`https://owlhead.ai`) and
   `NEXT_PUBLIC_OWLHEAD_APP_ORIGIN` (`https://app.owlhead.ai`) are set in `web-deploy.yml`'s
   build step. Each must be a bare `http(s)` origin, and the two must differ. Otherwise, as in
   development, the tests and a pull request's build, nothing is routed by host.
2. **The middleware routes by host before it reads any session** (`hostRoute` in
   `web/src/lib/hosts.ts`):
   - `www.` plus the site's host: a 308 to the same path and query on the site.
   - The site: its public pages are served with no session read or written. `/` shows the welcome
     page, and the rest are `/welcome`, `/api/beta`, `/demo`, the public files and `/_next/`.
   - The site, any other path (`/login`, `/auth/*` and every app screen): a 308 to the same path and
     query on the app. `next` stays a same-origin path, as DEC-211 has it. The query keeps its meaning,
     though Next may re-encode it (`/` as `%2F`), which decodes to the same values.
   - The app, or any other host (a preview, `workers.dev`): the sign-in rules (`authRoute`) are
     unchanged.
3. **The site's sign-in links point at the app** (`SIGN_IN_HREF`): `https://app.owlhead.ai/login`,
   or `/login` with no origins configured.
4. **Cookies stay host-only.** They are not widened to `.owlhead.ai`: the public site never needs
   the session, and a narrower cookie reaches fewer hosts.

### Supabase settings this needs

- **Site URL:** `https://app.owlhead.ai`.
- **Redirect URLs:** `https://app.owlhead.ai/auth/callback**`. The callback carries `?next=…`, which
  the glob allows. Callbacks to `owlhead.ai` are no longer needed, and can be removed once no
  sign-in started before the deploy is still in flight.

### Alternatives considered

- **A shared `.owlhead.ai` cookie:** the session would reach every subdomain, `api.owlhead.ai` among
  them, which DEC-823 item 6 rules out.
- **Hosts hard-coded in the routing logic:** a preview or a second deployment could not change them.

<a id="dec-2406"></a>

## DEC-2406: In a dev cell, `next dev`'s server takes the sign-in token from E21-52's stub issuer; no build serves it

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It adds a token source only under `next dev`, changes nothing a production or e2e build serves, and weakens no safety rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (lane W2, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443)), under DEC-79 and DEC-176 |
| **Story or claim** | E11-9, claim #1443; the cell demo's steps 1 and 6 (`xtask/src/cell/scenario.rs`); [DEC-2100](DEC-2100.md) items 5, 7 and 11; [DEC-1095](earlier-900-1999.md#dec-1095); [workspace API spec](../../specs/workspace-api.md) §3.11 items 5 and 6 |

### Context

The web app trades its sign-in client's access token for the API's session cookie at
`POST /v1/sessions` (DEC-1095). That token comes only from Supabase. A dev or CI cell's API trusts
E21-52's stub issuer instead (DEC-2100), so web/ cannot open a session there, and the demo's web
gestures (the kill switch in step 6, the approval in step 4, the set-up in step 2) cannot reach the
API. The page cannot ask the stub itself. The stub sends no CORS headers, and its `POST /token`
needs a JSON body that a page could send only after a CORS preflight the stub never grants
(DEC-2100 item 7).

### Decision

1. **`next dev`'s server is the stub's caller.** `POST /dev/stub-token` asks
   `POST <issuer>/token` for the one subject the server's configuration names, and answers
   `{access_token}` with `Cache-Control: no-store`. The browser trades that token at
   `POST /v1/sessions` exactly as it trades Supabase's. The session keeper, its refresh and its
   outage handling are unchanged (`src/api/session.ts`). This route is the sign-in client's
   stand-in, as DEC-2100 item 11 describes the stub: it reads nothing from the cell and is not a
   route into it.
2. **`next dev` only.** The route is `route.dev.ts`, and a production build does not list `dev.ts` as a
   page extension, so it has no such route. The middleware answers 404 under `/dev/` whenever
   `NODE_ENV` is not `development`. The handler answers 404 unless `readStubSignIn` finds the
   configuration under `next dev`. `scripts/no-scenarios.mjs` checks that the production build and
   the e2e build both answer 404 there.
3. **The configuration is server-side and checked.** `OWLHEAD_STUB_ISSUER` must be exactly
   `http://127.0.0.1:<port>` or `http://[::1]:<port>`: an IP literal on loopback with a port from 1 to
   65535, as DEC-2100 item 5 binds the stub. `OWLHEAD_STUB_SUBJECT` must be 1 to 1024 bytes with no
   control character, as DEC-992 item 2 requires. Anything else leaves the stub sign-in off. Neither
   value is a secret, and neither reaches the bundle. The browser is told only whether the stub
   sign-in is on.
4. **Only the app's own page may ask.** The route requires `X-Mandate-Request: 1`, the API's own CSRF
   mark (§3.3), and an `Origin` whose host is the `Host` the request came to, on a loopback name:
   `localhost`, `127.0.0.1` or `[::1]`. A cross-site form can send neither, and a page on another
   origin is refused with 403.
5. **Failures read as the sign-in client's.** No route, or a refused ask, is "signed out". A stub that
   does not answer is a 502, which the browser reads as `unreachable`. The API session reads that
   as the provider's outage, so it keeps pause and the kill switch (§3.11 item 6). The stub is called
   with a ten-second timeout and no redirects.

### Attacks considered

- **The route ships to production.** It is not compiled there (item 2). If it were, the middleware
  still answers 404 outside `next dev`, and the handler is off without `NODE_ENV=development` and both
  variables. The postbuild probe fails a build that answers otherwise.
- **Another page in the developer's browser mints a token through the dev server.** It cannot set the
  CSRF header without a preflight, which the route never grants, and its `Origin` is not the app's
  (item 4).
- **A DNS-rebinding page.** A page on a name the attacker rebinds to `127.0.0.1` is same-origin with
  itself, so its `Origin` matches the `Host` it sends and it may set the CSRF header. The route takes
  only a loopback `Host` (item 4), so a rebound name is refused with 403, as the stub's own `Host`
  check refuses it (DEC-2100 item 7).
- **Another local process asks the dev server.** It can get a token for the configured subject. It
  could already ask the stub directly on loopback, which DEC-2100 discloses for dev and CI cells:
  paper and demo data only, never a live connection.
- **The token leaks into logs or caches.** The route logs nothing and sends `no-store`. The token
  lives 300 seconds (DEC-2100 item 7).

### Rationale

The API's own verification path, unchanged, is what the demo must exercise: the browser opens its
session at `POST /v1/sessions` with an ordinary access token, as it will with Supabase's. Only the
token's source differs, and only under `next dev`. A server-side caller is the one way to get the
stub's token to the page without weakening the stub's refusal of every page (DEC-2100 item 7).
Building it as a dev route keeps it out of every build by the mechanism the palette route already
uses, and the postbuild check proves it.

### Alternatives considered

- **CORS on the stub:** refused. DEC-2100 item 7 keeps the stub unreadable from any page on purpose.
- **A token pasted into the page, or put in a `NEXT_PUBLIC_` variable:** refused. It would put a
  bearer token into the bundle or into browser storage, and it expires in five minutes anyway.
- **Opening the session server-side and forwarding the cookie:** refused. The API's cookie belongs
  to the API's origin and the browser (§3.3 item 5). The app holding it would be a server-side
  shortcut into the cell.

<a id="dec-2407"></a>

## DEC-2407: Over the workspace API, web/'s set-up creates the version and sends the confirm; deploying waits for an agent-stream route and step-up

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It sends only what the server already serves, records nothing the server does not, and weakens no safety rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (lane W2, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443)), under DEC-79 and DEC-176 |
| **Story or claim** | E11-9, E10-11, E10-12; claim #1443; the cell demo's step 2 ("the same gestures from web/"); supersedes [DEC-2401](web.md#dec-2401) item 4 for steps 1 and 2 over the API; [DEC-2101](DEC-2101.md) items 2 to 6; [workspace API spec](../../specs/workspace-api.md) §3.11 item 11, §4.1, §5.1 |

### Context

DEC-2401 item 4 left all three set-up steps unsent over the API: creating a version and deploying
had no request shape, and the confirm needed the served confirmation's digest. DEC-2101 (#1552)
then gave the server `POST /mandate-versions`, which takes the mandate document itself,
`GET …/confirmation`, the confirm, and `POST /agents`. It also refuses every confirm and deploy
that needs step-up until E9-4 binds its step-up port (item 2). And `POST /agents` takes an
`agent_id` that must already name an agent stream (item 6), which no API route creates.

### Decision

1. **Step 1 is sent.** `POST /mandate-versions` carries the set-up's mandate document as it stands.
   The step is `recorded` only on the server's 202 (or its 200 for a version already recorded),
   under the version the server hashed, never one computed in the browser. The classification
   shown is the served confirmation's, read once the version is recorded.
2. **Step 2 is sent, bound to the screen the server serves now.** Before the confirm, the browser
   reads `GET …/confirmation` again, because its digest can move (DEC-2101 item 4). The confirm then
   passes the served `screen_digest` and `classification` through unchanged (DEC-2401). `step_up` is
   `null` until E9-4 gives the browser its ceremony. A first version is risk-increasing, so the
   server answers `step_up_required` and commits nothing, and the set-up says so. Binding E9-4
   changes only that member.
3. **Step 3 stays unsent.** A new agent has no agent stream for `POST /agents` to name, and no route
   creates one. Deploying also needs step-up (DEC-2101 item 2). The step answers `not_sent`, and no
   agent exists. It is sent once both exist.
4. **The mock workspace API follows the server.** It creates a version from the document and
   serves that version's confirmation as a new mandate's. It refuses a confirm without step-up as
   `step_up_required`, and a version it never created as `not_found`.

### Rationale

Every step that is sent records only what the server records, under the server's own version and
digest, and a refusal is shown as the server gave it. The CLI stays the path for the confirm and
the deployment until step-up exists, as DEC-2101 item 2 says.

### Alternatives considered

- **Keep step 1 unsent until step-up exists:** refused. The server serves it now, and creating a
  version changes no risk.
- **Mint an agent id in the browser and send the deploy:** refused. The server refuses an id that
  names no agent stream, and a refusal the client could foresee is noise, not a gesture.

<a id="dec-2408"></a>

## DEC-2408: Over the workspace API, the Stop sheet and the kill switch's record screens build from what the API serves; what it does not serve reads "not reported"

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering decision under DEC-79 and DEC-176. It makes the kill switch reachable where it was not, and weakens no safety rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | `claude-code` (lane W2, claim [#1443](https://github.com/kunwarshivam/mandate/issues/1443)), under DEC-79 and DEC-176 |
| **Story or claim** | E11-9, E10-10; claim #1443; the cell demo's step 6; AGENTS.md rule 13; [DEC-1096](earlier-900-1999.md#dec-1096); [DEC-1007](DEC-1007.md); [workspace API spec](../../specs/workspace-api.md) §5.4 |

### Context

Over the API data source, screens render the API's read models (`view`, DEC-1096), and the
fixture-shaped workspace holds no agents. The Stop sheet and the D10 record screens read only the
fixture-shaped workspace. So over the API the sheet offered no agent's kill switch. The account's
"Stop all agents" and "Close everything" screens said "No agent is running" and offered nothing to
confirm. The runtime would have sent the switch correctly (DEC-1007), but no control on screen
reached it. Rule 13 says the kill switch is always available.

### Decision

1. **The sheet lists the agents the API serves.** Each agent gets Pause and its kill switch, which
   opens the agent's D10 record screen. A stopped agent shows that it is stopped. The account's
   choices are unchanged.
2. **D10 over the API builds from the API's view.** This covers the agent's kill switch, "Stop all
   agents" and "Close everything". The agents and their modes come from what the API serves, and
   the connection is the one it lists.
3. **What the API does not serve reads "not reported", never "none".** The API serves no open
   orders, and its positions are the account's, not one agent's. So "Orders it cancels" and an
   agent's "Positions it sells" say they are not reported. A list that read empty would claim that
   the switch cancels and sells nothing. The confirm is still offered, because the kill switch never
   waits on a read (rule 13).
4. **A failed read never hides the switch.** The sheet lists every agent the API serves, whatever its
   mode reads. When the agents cannot be read, the sheet says the per-agent list is unavailable and
   keeps the account's choices. "Stop all agents" builds its record with no read at all: its agents
   list says it is not available. If no connection has been read either, the record's route names
   none, and the screen says this is the workspace's kill switch, the only scope left, before it is
   confirmed (spec §5.4: Stop must work when the dashboard has not loaded). "Close everything" is the
   broker's account-wide switch, so it is offered only once a connection is read.
5. **Release (D11) is not offered over the API.** Its warning names the positions it hands over
   unprotected, and the API cannot show them per agent. So it has no record screen there.

### Rationale

The switch the screen sends is the same `kill_switch` the API records, scoped to the agent or the
connection. Only what the screen shows depends on the data source. Saying "not reported" keeps the
record honest about what the owner saw (mandate spec §10), without holding back the one action
that must never wait.

### Alternatives considered

- **Block the kill switch over the API until orders and per-agent positions are served:** refused.
  It contradicts rule 13.
- **Fill the fixture-shaped workspace from the API's reads:** refused. DEC-1096 keeps fixture
  shapes off the API path, so that nothing the API did not serve passes for its data.

<a id="dec-2410"></a>

## DEC-2410: The web kill switch asks for a plain confirm, never a passkey

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | The agent, under DEC-79 and DEC-176: it tightens the screen to the specs it already had to follow, and adds no risk |
| **Story or claim** | Demo audit item 4 (#2750). Product experience D10 and G2. Amends D10's "Activate (step-up)" |

### Decision

1. **A kill switch at any scope is confirmed with one press.** D10's agent kill switch, the
   account's stop-all and its close-all open a plain confirm, "Confirm the kill switch", with
   Cancel and Activate. No passkey is asked, and the screen says so: "No passkey needed." A
   browser with no passkey, or a passkey check that would fail, cannot hold one back. Cancel,
   Escape or Close send nothing and say so.
2. **The release stays a Stop.** D11's "Stop and release to me" is an owner Stop, so it keeps its
   passkey (PX-4 (b), DEC-136). So do resume and Stop on the Stop sheet.
3. **It announces only what happened.** A kill switch's entry reads "Recorded" once the deployment
   records it, then waits for the runtime, and says orders were canceled and positions sold only
   once the runtime applies it (`awaitsRuntime`, as resume and Stop already do).
4. **The privileges beyond the stop are not offered here.** Selling equities outside the regular
   session at a confirmed bid is the one part of a kill switch that needs step-up (mandate §6.1).
   The web kill switch does not offer it. A later screen that does must step up for that part
   alone, never for the stop and flatten.

### Rationale

Identity ID-5 says no step-up is needed to engage a kill switch at any scope. Workspace API API-7
records a kill switch without a valid step-up. Mandate §6.1 says it is never refused (DEC-158
option (c)), and PX-4 says the web kill switch inherits that rule. D10 still said "Activate
(step-up)", and the screen asked for a passkey whose cancel or failure sent nothing. That put a
check in front of risk reduction, which `AGENTS.md` rule 13 forbids. It also announced "positions
sold, agent stopped" as soon as the deployment recorded the command, before the runtime had done it.

### Alternatives considered

- **Keep the passkey, and send the kill switch anyway when it fails.** The owner would still be
  asked for a check the spec says they never need, and a failure that sends anyway reads as broken.
- **No confirm at all.** A kill switch cannot be undone. One deliberate press on a screen that
  names the scope is the least the record screen (brief §4.1) asks for.

<a id="dec-2412"></a>

## DEC-2412: The staging demo's browser tries a cooling-off passkey until a fixed instant, no later than the server's ceiling

| | |
|---|---|
| **Status** | Accepted (carries out the founder's "waive on staging", which DEC-2150 records; items 1 to 6 are lane W's readings under DEC-79 and DEC-176, each the one that adds no risk) |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | the founder (the waiver, through DEC-2150); `claude-code` (lane W) for how the browser follows it; number taken from the web range (2400s) |
| **Story or claim** | E9-4; amends [DEC-2411](DEC-2411.md); follows [DEC-2150](DEC-2150.md) |

### Context

[DEC-2150](DEC-2150.md) lets the staging server waive a first passkey's 24-hour cool-off for the
demo, until an expiry no later than its compiled ceiling, 2026-11-05T00:00:00Z. The browser has a
pre-check of its own: `usablePasskeys` never offers a passkey still cooling off, so the
authenticator cannot pick one the server would refuse. [DEC-2411](DEC-2411.md) lets only a test
cell's `next dev` drop that pre-check (`NEXT_PUBLIC_OWLHEAD_COOL_OFF_WAIVED`), and `next.config.ts`
refuses the flag in any other build. The deployed Worker is a production build, so on staging the
browser kept refusing a new passkey for 24 hours while the server would have accepted it: a new
owner could not confirm or deploy on the day they enrolled.

### Decision

1. **A second flag, an instant.** `NEXT_PUBLIC_OWLHEAD_STAGING_COOL_OFF_UNTIL`, written exactly as
   `YYYY-MM-DDTHH:MM:SSZ` in UTC. Strictly before that instant, by the page's clock, which runs on
   the server's, the browser offers a passkey still in its cool-off and leaves the verdict to the
   server. At or after it the flag is inert. It drops the pre-check and nothing else.
2. **The ceiling is the server's, compiled in.** `src/lib/cool-off-build.ts` holds
   `STAGING_WAIVER_CEILING`, 2026-11-05T00:00:00Z, the same const as `mandate-api-server`'s, and a
   test checks that the two match. `next.config.ts` refuses a build whose value is later than the
   ceiling or in any other form, before the config is made. Moving it needs a code change and a new
   founder decision, as DEC-2150 item 3 says for the server.
3. **Only the deploy sets it.** `web-deploy.yml` sets it to the ceiling. No other workflow, config or
   `.env` file names it, and a test checks that for `web.yml` and both Playwright configs. DEC-2411's
   own flag keeps its refusal outside `next dev`, unchanged.
4. **The server still judges.** A passkey the browser offers under the flag is refused
   `step_up_missing` by a server past its own expiry, or by any cell that does not waive, and the
   screen says so as for any refused step-up. The flag cannot lift a refusal; it only stops the
   browser from refusing first.
5. **Production is the same build.** The app's one deployment reads staging today. The flag is in
   that bundle until E9-4's row removes it, and the ceiling ends it on 2026-11-05 even if the row
   waits.
6. **It is removed with the server's waiver.** The backlog row
   `docs/project/backlog/E9-4-remove-staging-cool-off-waiver.md` takes the server's staging tier out.
   Its change also deletes this flag, its line in `web-deploy.yml`, and its allow-list entries in
   `xtask` (`main.rs`'s workflow env names and `leak.rs`'s `PRE_ENV`).

### Rationale

The pre-check exists so the authenticator is never asked for a passkey the server will refuse.
While the staging server accepts that passkey, the browser refusing it is the only thing in the
way. An instant capped by the server's own ceiling keeps the two in step: the browser can never
waive for longer than the server can, and once either ends, the server refuses as before.
