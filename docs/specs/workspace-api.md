# Workspace Services API Spec (v0.1, draft)

| | |
|---|---|
| **Status** | Draft v0.1, not yet reviewed ([DEC-436](../project/decisions/DEC-436.md)). Round 1 fixes applied. Items 1 to 16 and 19 to 21 of DEC-436 are agent readings; items 17 and 18 are Proposed and wait for the founder. §4.8.1 adds the audit read contracts (E12-6; [DEC-760](../project/decisions/DEC-760.md) to [DEC-767](../project/decisions/DEC-767.md), agent readings) |
| **Implements** | [HLD §4](../HLD.md#workspace-deployment) (workspace control services), [§6 flows A and C](../HLD.md#6-key-flows), [§7](../HLD.md#7-logging-and-audit), [§8](../HLD.md#8-multi-tenancy-and-security); PRD FR-1.4, FR-2.1 to FR-2.4, FR-3.1 to FR-3.5, FR-4.4, FR-6.2 to FR-6.5, FR-7.1 to FR-7.5, FR-8.1 to FR-8.4; backlog E8, E10, E11, E12 |
| **Depends on** | [Mandate spec](mandate.md) §2, §6, §7, §9, §10; [journal spec](journal.md) §2, §5, §7, §9, §11, §12; [infrastructure design](../design/infrastructure.md) §3.6, §9; [product experience brief](../product/09-product-experience.md) §3 to §5 |
| **Siblings** | Identity, roles, sessions, and step-up ceremonies: the [identity spec](identity.md), gap 6. Notification delivery and approval deep links: the [notifications spec](notifications.md), gap 7. Broker connection flows: `docs/specs/connections.md` (gap 9) |

This spec is the contract between the backend and everything that takes the owner's input or shows
the owner their workspace: the web app, the CLI, and owner-connected agents over MCP. It adds no
trading rule. The runtime, the executor, and the gate keep every decision they make today; the API
records what a person asked for, in the journal, and shows what the journal says happened.

## Contents

1. Scope
2. Invariants
3. Conventions
4. Resources and operations
5. Safety-relevant request and response shapes
6. Consistency model
7. Lifecycle and failure walk
8. Adversaries
9. What exists and what is planned
10. Decisions
11. Backlog
12. Open questions

---

## 1. Scope

### 1.1 What the API is

The workspace services API is the one door into a workspace deployment for people and their tools.
It serves the services HLD §4 lists: the agent registry and spec compiler, the policy service, the
deployment manager, the approval service, the audit explorer backend, and the connection manager.

It does three things, and only these:

1. **Takes owner input and journals it.** Every call that asks for a change becomes an event on the
   workspace control stream (`ctl:{workspace_id}`, journal spec §2) before anything acts on it.
   The stream owners (the agent runtime and the account executor) copy and judge it, as they do
   today for the CLI (mandate spec §6.1).
2. **Serves read models.** Views of agents, positions, orders, approvals, and the journal, each
   derived from journaled events and stamped with how current it is (§6).
3. **Runs workspace-side tools that write no trading state:** compiling and validating mandates,
   rendering confirmation screens, dry runs, and exports.

### 1.2 Callers

| Caller | Principal | How it authenticates | What it may do |
|---|---|---|---|
| **Web app** (`web/`, DEC-200) | A user | Session from the identity spec (passkey or OIDC), in an HttpOnly cookie | Everything its role allows (§3.7) |
| **CLI** (`mandate-cli`) | A user | A sender-constrained (DPoP) token from the identity spec (§3.3 item 2) | As the web app. In Phase 1, and for on-site operators, the CLI may still append to the control stream directly (DEC-436 item 3) |
| **Owner-connected agent** over MCP (DEC-141, DEC-183) | A **client** acting for one user | Its own scoped, revocable, sender-constrained (DPoP) token; never a broker credential (DEC-141 item 4) | A closed list (§3.8): reads, owner requests, draft proposals, the dry run (DEC-190), and holding new openings (DEC-191). Never confirms, approves, picks a delegation, or uses an owner-only control |
| **Approval link** from a notification | Nobody until sign-in | The link holds only an opaque notice id; it grants nothing (§3.9) | Lands on the sign-in, then routes as a user |

The Owlhead MCP server (E10-6, E10-8) is a thin adapter: each MCP tool maps to one API operation
under the client's token. It has no path of its own.

### 1.3 Where it runs

- **In the workspace deployment, always** (HLD §4). Mandates, approvals, positions, the journal,
  and model output are strategy and trading intent, and never pass through the global control plane
  (HLD "Where data lives").
- **Managed:** behind the cell's API gateway and the identity provider (infrastructure §9).
  **Hybrid and on-prem:** on the customer's network; reaching it from a phone is HLD §12 item 7,
  open.
- **Process:** the workspace control services process of infrastructure §3.1. It holds the control
  stream's writer epoch (DEC-436 item 3), session keys, and no broker credential: an OAuth
  authorization code passes through it only in transit, during the callback, on its way to the
  vault (connections spec §5.2, CN-1). It reads the
  journal and its read-model tables; it writes only the control stream, the artifact store, and its
  own read-model and draft tables.

### 1.4 Non-goals

- **No trading authority.** The API never places, cancels, or sizes an order, never writes an agent
  or account stream, and never calls a broker or a runtime directly (`AGENTS.md` rule 12). Its
  checks are a convenience; the stream owners make every check again. This holds at connect too:
  the API checks the OAuth `state`, writes the authorization code or key straight to the vault,
  starts the pending connection's token-exchange process (OAuth) or executor (keys), and reports.
  The code exchange runs in the token-exchange process, which is not the executor, and the
  permission checks run in the executor; the API appends `ConnectionEstablished` only after it
  reads their passing results from the journal (connections spec §5.2, §8.1; DEC-690 item 1;
  [DEC-821](../project/decisions/DEC-821.md) item 2).
- **No identity design.** Sign-in, sessions, roles, separation of duties, and step-up ceremonies
  are the identity spec's. This spec states only what the API needs from them.
- **No notification delivery.** Channels, escalation, and quiet hours are the notifications spec's.
- **No third-party API in v1.** Enterprise access to the harness (DEC-149, E18) is a separate
  product decision (DEC-436 item 18).
- **No global control plane endpoints.** Directory, billing, fleet, and relay are gap 8.

---

## 2. Invariants

Each invariant is a property the API must always hold. Each has a test named in the right-hand
column; "fuzz" means a property test that drives random call sequences against an in-memory journal
and checks the property with an oracle of its own (`AGENTS.md`, "Independent oracles").

| ID | Invariant | How it is tested |
|---|---|---|
| **API-1** | **Authenticated.** Every call except the liveness probe and a CORS preflight (§3.3 item 5) carries a valid session or token. Without one the API returns 401, reveals no data, and says nothing about whether the resource exists | A route table test: every registered route, called without credentials, returns 401 with the same body |
| **API-2** | **Authorized on the server.** Each operation checks the principal's role in the workspace (§3.7) and, for a client, its scope (§3.8), on the server, before any read or write. The web app's role checks (`web/src/lib/roles.tsx`) are never authority | A matrix test: every route × every role and client scope, with the expected 403 or success from §3.7, computed from the table, not from the handler |
| **API-3** | **Journal before effect.** Every call that can change what an agent, connection, policy, member, client, or approval may do commits its control-stream event (`Committed` or `AlreadyCommitted`, journal spec §5.1), with the caller's identity in `actor`, before it reports success and before anything acts on it. The API writes no agent or account stream | Fault injection: fail the append at every step; no stream owner ever sees an effect without its control-stream cause, and no response says `recorded` without a committed event |
| **API-4** | **Idempotent.** Every mutating call carries an `Idempotency-Key`. The same principal, operation, and key always resolve to the same control-stream event. A repeat with the same body returns the first outcome; a different body returns 409 `idempotency_conflict`. Nothing is committed twice | Fuzz: random retries, duplicate submits, lost responses, and concurrent repeats; an independent counter of control-stream events per key never exceeds one |
| **API-5** | **The envelope changes only by a confirmed version.** No call changes an envelope field of a deployed agent except confirming a mandate version, by a **user** with the role for it. The server computes the classification itself (mandate spec §9.2) and requires step-up when it is risk-increasing; a client's claimed classification is never used (rule 11) | Fuzz random envelope edits through every operation; an oracle that diffs the confirmed documents shows every change came through a `MandateConfirmed` by a user, with step-up whenever its own §9.2 verdict is increasing |
| **API-6** | **A client is owner input, never the owner.** A client token reaches only the operations of §3.8. `requested_by` is set from the authenticated channel and never read from a request body (mandate spec §6.2 step 5a). A body naming `requested_by`, or any member its schema does not name, is refused `invalid` and journals nothing on a strict operation; on an API-7-lenient operation (§5) the member is dropped and listed in `dropped`, and the command still commits (DEC-681 item 6, DEC-682 item 27). A client can never confirm a version, answer an approval, create, widen, or pick a delegation, connect or revoke a connection, change a member, policy, or client, export, or use pause, resume, Stop, release, owner exit, acknowledgment, or the kill switch (DEC-141, DEC-185, DEC-191) | The route matrix for the client principal; a test that a client's owner request carrying `requested_by: owner` is refused `invalid` and commits nothing, and that a client's hold carrying it returns `202` with `dropped: ["/requested_by"]` and is journaled as `client` |
| **API-7** | **Risk reduction is never blocked by the API.** Pause, holding new openings, the kill switch at any scope (including the kill-switch half of a revoke on compromise, §5.6), an owner exit, Skip on an approval, ending a delegation, and away mode (the **API-7 operations**) are refused only for a failed authentication (including the CSRF check of §3.3 item 1), a role that may not act, a failed session-record read, or a malformed request. Never for a rate limit, a quota, a failed membership read (`membership_unavailable`: identity spec §4.5 authorizes them from the session's roles snapshot instead, and the event records `membership_unverified: true`), a stale or missing read model, missing step-up (except where mandate spec §6.1 requires it, below), a runtime, model, market-data, or global-control-plane outage, a pending approval, or a frozen control stream (journal spec §11). A kill switch or owner exit without valid step-up is still recorded and still stops or routes (mandate spec §6.1, DEC-158 option (c)) | A test per operation with every one of those conditions injected; each still commits its event. Resume, Stop, and acknowledgment are not risk reduction and may be refused without step-up, as mandate spec §6.1 says |
| **API-8** | **The kill-switch path needs only the API, its authentication, and Postgres.** Pause and the kill switch read no read model, call no model, runtime, market-data service, global control plane, or telemetry, and run on a reserved worker and database-connection pool that other traffic cannot exhaust (infrastructure §3.6) | A test with the model gateway, read-model tables, runtime, and metrics exporter all unavailable and every ordinary worker busy: the kill switch commits within its bound |
| **API-9** | **Tenants never see each other.** Every resource lives under one workspace. A principal reaches only workspaces it belongs to. An id from another workspace, or one that does not exist, returns the same 404. No response, error, log line, metric label, or notification carries another workspace's data | Cross-workspace tests at the route, database (row-level security), and artifact layers (OPS-6); a test that the 404 bodies and timings for "foreign" and "absent" match |
| **API-10** | **Nothing sensitive leaves through the API's side channels.** What the API hands to the relay or a notification provider is an opaque notice id and generic text only; approval links carry only that id; page titles, URLs, and error titles hold no instrument, size, price, thesis, or agent name (rule 6) | Payload capture tests on every notification the API emits; a URL lint over the route table |
| **API-11** | **Credentials never cross the API outward.** No response, log, journal event, or error carries a broker credential, OAuth token, or key. A key field is write-only and goes straight to the vault in the same request; a rejected key is explained without echoing it (rule 7) | Log and response scans with canary secrets; a test that every connection response matches a schema with no secret-shaped member |
| **API-12** | **An answer binds what was shown.** An approval response carries the content hash the client rendered, and the API forwards it unchanged. The API never computes a hash on the client's behalf, never answers for a principal, and never reports a grant as acting: only the runtime's `ApprovalResponded` and `ApprovalRevalidated` say what happened (mandate spec §6.4) | A test that an approve with a hash the request does not carry is journaled as sent and refused by the runtime as `content_mismatch`; a test that no response field reads "approved" before `ApprovalRevalidated` with `act` |
| **API-13** | **No optimistic success.** A mutating call reports `recorded` only after the append returned `Committed` or `AlreadyCommitted`. A lost or `Ambiguous` append reports `unknown`, never success and never "nothing happened". `applied` is reported only when the owning stream's copy exists (§6.3) | Fault injection on the append's response path; the web app's `result-unknown` scenario becomes a contract test |
| **API-14** | **Read models are derived and stamped.** Every read names, for each stream it read, the `seq` and hash it reflects, and the `recorded_at` of that event. No value comes from anywhere but the journal and stored artifacts. A value older than its freshness limit is returned marked stale with its age, never as current | Replay test: rebuilding every read model from the journal alone gives identical responses; a staleness test per freshness limit |
| **API-15** | **Journal pages are complete and checkable.** Paging a stream by cursor yields every event in the range exactly once, in `seq` order, with each event's `hash` and `prev_hash`, so a client can check that each page joins the last | Fuzz: random page sizes and concurrent appends; concatenated pages equal the stream range, and the chain check passes |
| **API-16** | **Exports are verifiable and recorded.** Every export is journaled (`ExportCreated`) before it is served. A canonical export verifies under journal spec §11 against its anchors; every derived JSON or CSV view names the verifier digest of the canonical export it came from | A test that runs the journal verifier over each export and fails a tampered one |
| **API-17** | **Step-up is bound to one action.** A step-up challenge the API issues names one action digest (§3.6). Evidence is accepted only on the call for that digest, and an assertion id is used once per workspace (DEC-173 item 3) | Tests that evidence for one approval, version, or command is refused on another; replay of a used assertion is refused; every step-up action **this API serves** (§1.4: identity spec ID-4's list and the **S** cells of the workspace-scope rows, never the org-scope rows, which are the global control plane's) has a kind and can issue a challenge |
| **API-18** | **Model text is never an action.** Model output (chat replies, theses, compiler notes) is returned only in members typed as quoted, attributed content with an author label. Action cards come only from deterministic operations (§4.6), and acting on one needs its own call (rule 4, DEC-192) | A schema test: no action or button member's type can hold model text |
| **API-19** | **Concurrent edits never merge silently.** A draft update and a version confirmation name the base they were made from; a call whose base is no longer current is refused with 409 and the current base, never applied over another person's change. Risk-reducing shortcuts (§4.5) rebase instead, and their result is re-classified | Fuzz with two writers; every version's recorded base is its real predecessor, and every rebased shortcut is still classified reducing |

The invariants close the "never" and "always" claims of this document. §7 walks each one through
outages and §8 through attackers.

---

## 3. Conventions

### 3.1 Transport

**JSON over HTTPS**, described by an OpenAPI 3.1 document generated from the Rust types
(DEC-436 item 2). Why not gRPC at the edge:

| | JSON over HTTPS | gRPC |
|---|---|---|
| Browsers | Native, with cookies, CSRF defences, and caching rules the web app already has | Needs gRPC-Web and a proxy |
| MCP adapter, CLI, approval links | Plain HTTP clients; links are URLs | Generated stubs per client |
| Decimals and timestamps | Strings in the journal's canonical forms (journal spec §4), exactly as events carry them | Needs custom message types to avoid floats |
| Live updates | Server-sent events over the same connection rules | Server streaming (better, but not needed at these rates) |

Internally the API talks to Postgres only. It never calls a runtime or an executor over the network;
the journal is the only channel (DEC-17).

- **Encoding.** UTF-8 JSON. Money, quantities, prices, and fractions are decimal strings in the
  journal's canonical form (journal spec §4.6); a JSON number in any such member is refused as
  `invalid`, never rounded. Timestamps are journal spec §4.7 strings. Hashes are `sha256:` refs.
- **Ids** are opaque: ULIDs for events, random ids for drafts, notices, and previews. No id embeds a
  name, an instrument, or a personal datum.
- **Change notices.** `GET /v1/workspaces/{ws}/changes` is a server-sent-event stream of
  `{stream_id, seq}` watermarks only. The client refetches what it shows. Polling with the same
  watermarks works when a proxy drops the stream.

### 3.2 Versioning

- Paths start with `/v1`. Within a major version, changes are additive: new operations, new optional
  request members, new response members, new error codes in an open set. Clients ignore response
  members they do not know.
- A change that removes, renames, or retypes anything, or that makes a request member required, is
  `/v2`, served beside `/v1` for at least two release cycles.
- **Safety enums are closed.** Members whose values change meaning (verdict, command, scope,
  classification, phase, effect) are closed sets; a new value is a new major version, so an old
  client never meets a verdict it cannot render.
- Each response carries `api_version` and the server build digest, so a record screen names what
  rendered it (mandate spec §10's UI build covers the client half). A problem document (§3.5) is
  the exception: it is not wrapped in the envelope, and its `type` and `code` say what it is.

### 3.3 Authentication and sessions

Owned by the [identity spec](identity.md). What the API requires of it:

1. **Browser sessions** in a `Secure`, `HttpOnly`, `SameSite=Strict` cookie, with a CSRF defence on
   every mutating call that the session cookie authenticates. The `Origin` header must equal the
   deployment's configured app origin (`https://app.owlhead.ai` for the demo), and the call must
   carry the custom request header `X-Mandate-Request: 1`, which a cross-site form cannot set.
   - The order is fixed: authentication first (401 `unauthenticated`, one body, API-1), then the
     CSRF check (403 `forbidden`, `effect: none`, `retryable: false`), before any other read or
     write.
   - A call authenticated by a sender-constrained token (item 2: the CLI, a client, a service
     account) sends no `Origin` and is not checked; a cross-site page cannot produce its proof.
   - The check applies to the API-7 operations too. A cookie call that fails it is not the owner's
     authenticated call, so its refusal is API-7's "failed authentication", not a new reason
     (DEC-682 item 19).
   - No bearer token is ever stored in browser storage (brief §5, rule 6 row).
2. **CLI, client, and service-account tokens are sender-constrained** (DPoP, RFC 9449; identity
   spec §6.3, §6.5, §6.6): each request carries a proof signed by the key the token was issued to,
   and the API refuses a token presented without a matching proof. A copied token alone is useless.
   Tokens are bound to one principal and one workspace, revocable at once, and stored by the server
   only as a hash. No plain bearer token is accepted on any route.
3. **Authentication works without the global control plane** (HLD §8), and the kill-switch path
   authenticates without a network call to the identity provider, by any of identity spec §6.4's
   three routes: a live session whose provider refresh failed (pause and kill switch only, until its
   absolute lifetime), a reduction-only session opened by a workspace-local passkey, or the host
   CLI. DEC-436 item 17 asks only about a session past its absolute lifetime.
4. Every journaled event names the principal in `actor`. A user is `{kind: "user", id}`. A client
   is `{kind: "client", id: <client id>}`, with the human it acts for named beside it as
   `on_behalf_of` (journal spec §3, DEC-671; DEC-436 item 9; the coordinator's settlement X2 on #560). A
   client is never recorded as a `user`, so the runtime's check 3 refuses a client's answer from the
   record alone. Wherever a rule compares humans (check 7's "not the mandate's author", mandate
   spec §5.8's requester, identity spec ID-6), a `client` actor counts as its `on_behalf_of` user,
   so a version an operator proposes through their own client and then confirms is still their own
   (mandate spec §6.4, amended in this change). Events also record the channel (`web`, `cli`,
   `mcp`) and the authentication method.
5. **Cross-origin requests** (the web app at `https://app.owlhead.ai` calls `https://api.owlhead.ai`
   with `credentials: include`; DEC-682 item 32):
   - Only the exact configured app origin is allowed: no wildcard and no reflected `Origin`. Its
     responses carry `Access-Control-Allow-Origin` with that origin,
     `Access-Control-Allow-Credentials: true`, and `Vary: Origin`, error responses (401, 403, and
     every other problem) included. Any other origin gets no CORS header.
   - The app origin must be same-site with the API: a subdomain of the same registrable domain,
     so the `SameSite=Strict` cookie is sent. The app is served on `app.owlhead.ai`; an origin on
     another site, such as `*.workers.dev`, can never work.
   - A preflight `OPTIONS` allows the methods `GET`, `POST`, `PUT`, `DELETE`, and `PATCH`, and the
     headers `Content-Type`, `Idempotency-Key`, `If-Match`, and `X-Mandate-Request`, with
     `Access-Control-Max-Age: 600`. A preflight is never authenticated and reads no data (API-1's
     exemption).
   - Exposed headers: `ETag`, `Location`, and `Retry-After`. (§3.2's version and build are body
     members, not headers.)
   - The session cookie is host-only, `__Host-` prefixed, `Secure`, `HttpOnly`, `SameSite=Strict`,
     and `Path=/`.

### 3.4 Idempotency

Every mutating call carries `Idempotency-Key`: 16 to 64 characters from `[A-Za-z0-9_-]`, chosen by
the client per user gesture. The API derives the control-stream `event_id` from it (DEC-436 item 4,
DEC-681 item 5), and a client can compute the same id before it sends the call:

- Build the JSON object `{"key", "operation", "principal", "workspace"}`: the key as sent; the
  operation's name from the route table; the actor's own id (a client's id, never its user's); and
  the workspace id. For each event of a call that commits several events, add `"position"`, an
  integer from 0 in batch order.
- Write it in journal spec §4's canonical form and take its SHA-256.
- The first 128 bits of that digest, most significant first, are the ULID. It is written as 26
  Crockford base-32 digits (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`), uppercase: 130 bits, so the
  128-bit value is left-padded with two zero bits and the first digit is `0` to `7`.
- A call that commits several events (§5.3, §5.6) reports position 0's id as its `command_id`: for
  a revoke on compromise, the kill switch's.
- **Worked vector.** Workspace `ws_01`, principal `user_7`, operation `kill_switch`, key
  `k1Z-9_aaaaaaaaaa`, one event: the canonical object is
  `{"key":"k1Z-9_aaaaaaaaaa","operation":"kill_switch","principal":"user_7","workspace":"ws_01"}`,
  and the event id is `7KXV6BGDW36KYC7RE5BPQG28Z0`.

The ULID's time component carries no meaning (journal spec §3), so this is a valid id. Then:

1. The API looks the id up first. If an event exists and its semantic members (everything but
   server-set times) equal the request's, it returns that event's outcome. If they differ, 409
   `idempotency_conflict`.
2. Otherwise it appends. The journal's own idempotency check (journal spec §5.1 step 1) catches a
   race between two copies of the same call.

This is DEC-290's rule for the CLI, carried to the API: the id is the idempotency key, so a retry
after a timeout, a fence, or a lost answer commits nothing twice. A call that writes several events
(§5.2's delegation) derives each id from the key plus the event's position in the batch.

### 3.5 Errors

Errors are RFC 9457 problem documents with these members:

| Member | Meaning |
|---|---|
| `type`, `status` | The problem type URI and HTTP status |
| `code` | A stable machine code, from the table below or an operation's own list |
| `title` | Generic text; never content (rule 6 holds for errors too) |
| **`effect`** | `none` (nothing was recorded), `recorded` (the event is in the journal; see its status), or `unknown` (the append's outcome could not be confirmed). The brief's Error state (§3.1) renders from this member: "nothing was sent", or "the result is unknown; we are checking". Never "try again" on `unknown` |
| `event_id` | The control-stream event, when `effect` is `recorded` or `unknown` |
| `retryable` | Whether the same request with the same key may be sent again |
| `violations` | For validation: `[{path, code, message}]`, with mandate V-codes, policy keys, and the nearest ancestor limit (mandate spec §4.3) |

| Code | Status | When |
|---|---|---|
| `unauthenticated` | 401 | No valid session or token |
| `forbidden` | 403 | The role or scope may not do this (never sent for a resource the principal cannot see: that is 404), or a cookie-authenticated mutating call failed the CSRF check of §3.3 item 1 |
| `not_found` | 404 | Absent, or in another workspace (API-9) |
| `invalid` | 422 | Schema, canonical form, or a value out of range |
| `idempotency_conflict` | 409 | Same key, different body |
| `stale_base` | 409 | The draft or version base moved (API-19); the body names the current base |
| `classification_changed` | 409 | The server's classification differs from the one the confirmation screen showed |
| `step_up_required` | 401 | A call that needs step-up presents no evidence at all (§3.6, DEC-686) |
| `step_up_missing` | 401 | (planned: E10-10) Evidence is presented but does not count: its challenge is unknown, its credential is not the principal's or is in its enrolment cool-off, or its assertion does not verify (identity spec §7.2 steps 4 and 6) |
| `step_up_stale` | 401 | (planned: E10-10) The challenge has expired, or is presented before its `issued_at` |
| `step_up_reused` | 401 | (planned: E10-10) The challenge was already used |
| `step_up_method` | 401 | (planned: E10-10) The method is not allowed in this environment: `cli_confirm` in `live` (identity spec §7.3) |
| `step_up_mismatch` | 401 | (planned: E10-10) The challenge names another principal, workspace, action kind, or action digest |
| `live_unavailable` | 409 | A live environment operation before counsel signs off (rule 8, B5) |
| `control_stream_frozen` | 503 | Journal spec §11 froze mandate and deployment changes; never sent for a risk-reducing call (API-7) |
| `journal_unavailable` | 503 | Postgres cannot take the append; `effect: none`, `retryable: true` |
| `rate_limited` | 429 | Over the principal's limit; never for an API-7 operation |
| `own_roles` | 403 | A role change grants or removes a role of its own author (identity spec ID-13, §4.5) |
| `owner_role_reserved` | 403 | Someone other than an org owner grants or removes the org owner role (identity spec §4.5) |
| `last_owner`, `last_admin` | 409 | The change leaves no `active` org owner or workspace admin (identity spec §5.2) |
| `reduction_only` | 403 | A reduction-only session asks for anything but pause or a kill switch (identity spec §4.5, §6.4) |
| `membership_unavailable` | 503 | The membership read, or the membership-index read of `GET /v1/me/workspaces` (§4.10), failed on an operation outside identity spec §4.5's risk-reducing set (API-7's operations, revoking a client, and tightening a policy); nothing was authorized; `retryable`. Never sent for an operation in that set |
| `outcome_unknown` | 503 | (planned: E10-10) The append's outcome could not be confirmed (an ambiguous commit, or `Fenced` during an upgrade, §7); `effect: unknown`, the derived `event_id` given, `retryable: false`. The client polls §5.5 with that `event_id` and never resends with a new key. A resend with the same key is still safe (API-4: it resolves to the original outcome), so `retryable: false` is a rule for the UI, not for correctness |
| `address_limit` | 409 | **(planned: E8-14)** A member already holds 10 notification addresses on the channel (§4.11). `effect: none`, `retryable: false`; title "Too many notification addresses". An endpoint already held is not an error: §5.7 answers `200` |
| `busy` | 503 | **(planned: E8-14)** The control stream moved under the call 5 times in a row (the expected-head retry of §5.7). `effect: none`, `retryable: true`; title "Try again" |

Success is not only `202`: a command that records a change answers `202` with `phase: "recorded"`,
and one whose change is already recorded answers `200` with that record and appends nothing, as
ending an already-ended delegation and §5.7's notification addresses do.

The six step-up codes have `retryable: false`, and `effect: none`: nothing the step-up guards is
committed (identity spec §7.2 step 6), and the same evidence fails the same way, so the client runs
a new ceremony first. The one exception is a revoke on compromise, whose kill-switch half is
committed anyway (`effect: recorded`, §5.6); the schema cannot tell that call apart, so it allows
both effects for these codes. None is ever sent for pause, the kill switch, an owner exit, or Skip
(API-7): there, failed step-up loses only the privilege it would have added (identity spec §7.3).
A call without evidence gets `step_up_required` before any other step-up check. Evidence that is
present is judged in the order of identity spec §7.2 step 4, and the first check that fails names
the code:

1. the challenge exists: else `step_up_missing`;
2. the method is allowed in this environment (§7.3, reported by step 6): else `step_up_method`;
3. the challenge is unused: else `step_up_reused`;
4. the challenge is not expired, and is not presented before its `issued_at`: else
   `step_up_stale`;
5. the challenge names this principal and workspace: else `step_up_mismatch`;
6. the credential belongs to this principal and its enrolment cool-off has ended, the signature
   verifies, user verification shows in the flags, and the counter has not gone backwards: else
   `step_up_missing`;
7. the submitted action's digest equals the challenge's: else `step_up_mismatch`.

These codes are returned only to the authenticated requester, with no detail beyond the code.

### 3.6 Step-up

The ceremony belongs to the [identity spec](identity.md) (E9-4). The API's part:

- `POST /v1/workspaces/{ws}/step-up/challenges` with `{action: {kind, digest}}` returns a challenge
  for that one action. It is the only source of the WebAuthn challenge the authenticator signs:
  the SHA-256 of the canonical challenge record (identity spec §7.2 steps 1 and 2),
  `mandate_passkey::stepup::ChallengeRecord::webauthn_challenge`. `kind` names one of the actions identity spec ID-4 lists, and the set grows
  with that list: `confirm_version` (risk-increasing, a delegation grant included), `deploy`,
  `approve`, `connection` (connect, change, or revoke), `resume`, `stop`,
  `acknowledge`, `owner_exit`, `kill_switch_privilege`, `disclosure`, `policy_loosen`,
  `member_invite`, `role_grant`, `client_connect`, `credential_enrol`, `notification_address`,
  `break_glass_approve`, and `lift_hold`. There is no kind for re-enabling a halted scope: the halted state is Proposed
  (DEC-437 item 21) and does not exist until the founder accepts it. API-17's test enumerates only
  the step-up actions this API serves: identity spec ID-4's list and the **S** cells of §3.7's
  workspace-scope rows. The org-scope **S** actions (SSO configuration, creating or archiving a
  workspace, org memberships and roles, service accounts, org policy, transferring or deleting the
  organization) belong to the global control plane's own API (gap 8, §1.4), so they have no kind
  here. `digest` is the SHA-256 of the action's canonical object: the approval's
  content hash, the mandate version hash, or the command's canonical members (DEC-279 item 1's
  object for commands).
- The evidence the call then carries is `{assertion_id, authenticated_at, method}`, the shape the
  journal records (journal spec §9.2). The API checks it is bound to the call's digest and unused,
  and records it. **Validity is judged by the stream owner** at the moment mandate spec §6.1 names;
  the API's check is a convenience, except that it never forwards evidence bound to another action.
- Until E9-4 ships, the only method is `cli_confirm`, paper only (mandate spec §6.1). The API
  refuses every `live` step-up with `live_unavailable`.

### 3.7 Roles

Roles, the permission matrix, and separation of duties belong to the [identity spec](identity.md).
The API enforces that matrix and no other (API-2; the coordinator's settlement X1 on #560). It is
printed here exactly as [identity spec §4.2](identity.md#42-permission-matrix) states it, so the
route test can parse it; if the two ever differ, identity spec §4.2 wins and this copy is a defect.
**S** marks a permission that needs step-up (§3.6). A blank cell is a denial. OO org owner, OA org
admin, Bill billing admin, WA workspace admin, Op operator, Ap approver, Vi viewer, Au auditor, Cl
client (DEC-141), SA service account, HC host CLI (identity spec §6.4 route 3), PO platform operator
inside an approved break-glass window (identity spec §10.3). There is no inheritance from org roles:
acting in a workspace needs a membership in it (identity spec §4.1). The HC and PO principals do not
call this API: the host CLI appends on site (DEC-436 item 3), and a platform operator acts through
break-glass. Their columns are printed so the copy stays exact. The cells read by identity spec
§4.2's grammar, and a refused authorization returns identity spec §4.5's codes (DEC-641, DEC-643).

| Permission | S | OO | OA | Bill | WA | Op | Ap | Vi | Au | Cl | SA | HC | PO |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| View agents, positions, decisions | | | | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | |
| Make an owner request through the owner-input API (builder, gate, and autonomy apply; DEC-141) | | | | | | ✓ | | | | ✓ | | | |
| Dry run of a request (DEC-190) | | | | | | ✓ | | | | ✓ | | | |
| Chat thread with the agent | | | | | | ✓ | | | | | | | |
| Read records, verification; export (journal §7) | | | | | ✓ | | | | ✓ | | ✓ | | |
| Draft a mandate version | | | | | | ✓ | | | | ✓ (propose only) | | | |
| Confirm a version: reducing or neutral | | | | | | ✓ | | | | | | | |
| Confirm a version: risk-increasing (incl. a delegation grant, V-022) | S | | | | | ✓ | | | | | | | |
| Deploy, go live (E10-4) | S | | | | | ✓ | | | | | | | |
| Pause | | | | | ✓ | ✓ | ✓ | | | | | ✓ | ✓ |
| Hold new openings (`exits_only`, DEC-191) | | | | | ✓ | ✓ | | | | ✓ | | | |
| Lift a hold a client set (DEC-191) | S | | | | ✓ | ✓ | | | | | | | |
| Resume, Stop, Stop with release | S | | | | ✓ | ✓ | | | | | | | |
| Kill switch, agent scope: engage | | | | | ✓ | ✓ | | | | | | ✓ | ✓ |
| Kill switch, connection or workspace scope: engage | | | | | ✓ | ✓ | | | | | | ✓ | ✓ |
| Kill switch, org scope: engage | | ✓ | ✓ | | | | | | | | | | |
| Kill switch, any scope: privileges beyond the stop (mandate §6.1), for a scope one may engage | S | ✓ | ✓ | | ✓ | ✓ | | | | | | | |
| Re-enable a halted scope (§4.4; only if DEC-437 item 21 creates the state) | S | ✓ (org) | ✓ (org) | | ✓ | | | | | | | | |
| Owner exit (close a position) | S | | | | | ✓ | | | | | | | |
| Acknowledge (ladder, tripwire, reconciliation) | S | | | | ✓ | ✓ | | | | | | | |
| Answer an approval: approve | S | | | | | | ✓ | | | | | | |
| Answer an approval: skip | | | | | | | ✓ | | | | | | |
| Remove or narrow a delegation | | | | | | ✓ | | | | | | | |
| Connect, change, or revoke a broker connection | S | | | | ✓ | | | | | | | | |
| Accept a disclosure (V-005) | S | | | | | ✓ | | | | | | | |
| Workspace policy: tighten | | | | | ✓ | | | | | | | | |
| Workspace policy: loosen (within the org's) | S | | | | ✓ | | | | | | | | |
| Invite, deactivate, remove a workspace member | S for invite | | | | ✓ | | | | | | | | |
| Grant or remove a workspace role | S for grant | | | | ✓ | | | | | | | | |
| Connect a client (issue its token) | S | | | | | ✓ | | | | | | | |
| Revoke a client | | | | | ✓ | ✓ | | | | | | | |
| Enrol or remove one's own passkey | S | own | own | own | own | own | own | own | own | | | | |
| Add or remove one's own notification address (a push subscription; later an email address) | S | | | | own | own | own | own | own | | | | |
| List one's own notification addresses (opaque references only) | | | | | own | own | own | own | own | | | | |
| List one's own workspace memberships | | self | self | self | self | self | self | self | self | | | | |
| Leave: deactivate one's own membership | | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | | | | |
| Org policy: tighten | | ✓ | ✓ | | | | | | | | | | |
| Org policy: loosen (within the platform's) | S | ✓ | ✓ | | | | | | | | | | |
| SSO configuration | S | ✓ | ✓ | | | | | | | | | | |
| Create or archive a workspace | S | ✓ | ✓ | | | | | | | | | | |
| Org memberships and org roles | S | ✓ | ✓ (not owner) | | | | | | | | | | |
| Issue or revoke a service account | S | ✓ | ✓ | | | | | | | | | | |
| Billing | | ✓ | | ✓ | | | | | | | | | |
| Transfer org ownership; delete the org | S | ✓ | | | | | | | | | | | |
| Approve a break-glass request (§10.3) | S | ✓ | | | ✓ | | | | | | | | |
| Restart a process; read verification results (break-glass operational set, §10.3) | | | | | | | | | | | | | ✓ |

How the API's operations map onto those rows:

| API operation | Matrix row |
|---|---|
| Read models (§4.7), the approvals inbox and content (§4.3) | View agents, positions, decisions |
| Journal, trace, timeline, gate decisions, exports, verification (§4.8) | Read records, verification; export |
| Drafts, compile, validate, create a version (§4.1) | Draft a mandate version |
| Confirm (§5.1) | Confirm a version, by the server's own classification |
| End a delegation, away mode (§4.4) | Remove or narrow a delegation |
| Pause, hold, lift a hold, resume, Stop (with or without release of positions), kill switch, owner exit, acknowledge (§4.2) | The row of the same name |
| Re-enable a halted scope | **Inactive.** No route exists until DEC-437 item 21 is accepted (§4.2) |
| Approve, Skip (§5.2) | Answer an approval, and listed in `autonomy.approval.approvers` (identity spec §4.1) |
| Connect, revoke, revoke on compromise (§4.5) | Connect, change, or revoke a broker connection |
| Policies, members, clients (§4.5) | The rows of the same names; a member deactivating their own membership is the leave row |
| One's own workspaces, `GET /v1/me/workspaces` (identity spec §4.5) | List one's own workspace memberships |
| One's own notification channels (a push subscription): set, remove | Add or remove one's own notification address |
| One's own notification channels: list | List one's own notification addresses |
| Owner request, dry run, chat (§4.6) | Make an owner request; dry run of a request; chat thread with the agent. A client also needs the `request` or `dry_run` scope (§3.8) and has no chat |

Separation of duties is enforced where the specs already enforce it: by the runtime at approval
(check 7) and by the executor at acknowledgment (mandate spec §5.8), with a client counted as its
human (§3.3 item 4). The API adds no second copy that could disagree.

### 3.8 Client scopes (owner-connected agents)

A client token names the user it acts for, the agents it may see, and some of these scopes. The
list is closed (DEC-436 item 9):

| Scope | Operations | Notes |
|---|---|---|
| `read` | Agent status, positions, plan view, daily brief, the client's own requests | Never the journal, exports, approvals' content, or other agents |
| `request` | Owner request (§4.6) | Journaled with `requested_by: client`; an `open` or `increase` always asks (MI-30) |
| `propose` | Create a draft from a base version | Only a user confirms it, in Owlhead, with step-up (DEC-185 item 4) |
| `dry_run` | The "can I?" dry run (DEC-190) | Read-only, rate-limited, journaled |
| `hold` | Set an agent to `exits_only` (DEC-191) | Lifting it is the user's, with step-up |

Every client call, reads included, is journaled with the client's identity (DEC-141 item 5): a
read as `RecordsAccessed` with the client as accessor and the resources read. A revoked token fails
its next call; revocation needs no step-up because it only removes access.

### 3.9 Approval links

The notification's payload is the [notifications spec](notifications.md)'s to define (its §4.2); under the coordinator's
settlement X3 on #560 it carries a random **notice id** and generic text only. This section states
what the API serves. The link is `/n/{notice_id}`. Opening it shows sign-in only (brief G4); after sign-in the API
resolves the notice to its approval for that user, if the user may see it, and otherwise returns
404. The notice id is random and is not the approval's event id: a ULID's first 48 bits are its
creation time, so an event id in a payload would tell the relay and the provider when the request
was made. A link grants no authority and holds no token. Mandate spec §6.4's payload sentence is
amended by the notifications spec's change (#558, DEC-438), whose wording also excludes the event
timestamp; this spec cites that sentence and does not edit it. What becomes of
`mandate_approval::ApprovalRef::of_requested_event` is said once, in notifications spec §4.2.

### 3.10 Rate limits and quotas

Per principal and per workspace (HLD §8 quotas), stricter for clients. No limit, quota, or load
shedding applies to the API-7 operations, which run on the reserved pool of API-8.

---

## 4. Resources and operations

Every path is under `/v1/workspaces/{workspace_id}` except those §4.10 lists: `/v1/me/*`, `/v1/reduction-sessions*`, and the OAuth callback of §4.5. "Event" names the control-stream event the call
commits (journal spec §9). **Journal change** marks an event or member the journal spec does not
define yet; §11's E10-15 adds them before the operation ships.

### 4.1 Mandate drafts and versions (agent registry and compiler)

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Create a draft | `POST /mandate-drafts` | `MandateDraftSaved` (journal spec §9.9) | From a description, the three goal answers (A0, E10-7), a template, or a base version. Returns `draft_id` and its `etag` |
| Read, list drafts | `GET /mandate-drafts/{id}`, `GET /mandate-drafts` | — | With provenance per path (mandate spec §2.1) |
| Save a draft | `PUT /mandate-drafts/{id}` with `If-Match` | `MandateDraftSaved` | Form and YAML are two views of this one document (FR-3.2). Stale `If-Match`: `stale_base` |
| Compile | `POST /mandate-drafts/{id}/compile` | The compiler's `ModelInvocationRecorded` on the control stream (journal spec §9.9) | Asynchronous job. Fills only `user_stated` and `platform_proposed` values, never `auto` or a delegation (V-022, V-038). Output failing the schema is `compile_failed` with the description kept (A2) |
| Validate | `POST /mandate-drafts/{id}/validate` | — | Schema, V-rules, the policy hierarchy; returns violations and warnings with each warning's code (A4). Writes nothing |
| Create a version | `POST /mandate-drafts/{id}/versions` | `MandateVersionCreated` | Requires a passing validation. Canonicalizes and hashes (mandate spec §9.1); records provenance, validation results, the classification against the base version, and the diff (§10) |
| Read a version | `GET /mandate-versions/{hash}` | — | The stored canonical document and its records |
| Diff | `GET /mandate-versions/{hash}/diff?against={hash}` | — | Every changed path with its §9.2 classification and when it applies (A6) |
| Confirmation screen | `GET /mandate-versions/{hash}/confirmation` | — | The deterministic data A5 renders: provenance badges, platform defaults, worst-case figures, the unasked dollars (DEC-189), the crypto gap disclosure when it applies, and its `screen_digest` |
| What would change | `POST /mandate-versions/{hash}/replay` | `RecordsAccessed` | DEC-186: counts only, over 30 days of `DecisionMade`; stored with its result hash |
| Confirm | `POST /mandate-versions/{hash}/confirm` | `MandateConfirmed` | §5.1. Users only |
| Accept a disclosure | `POST /disclosures/{document}/accept` | `DisclosureAccepted` | Step-up (V-005). Text is a named placeholder until counsel writes it (rule 9) |

### 4.2 Deployments and the agent lifecycle (deployment manager)

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| List, read agents | `GET /agents`, `GET /agents/{id}` | — | Read model (§4.7) |
| Run a backtest | `POST /backtests` | `BacktestRunRecorded` when done | Version and data snapshot (B1); report at `GET /backtests/{id}` |
| Deploy to paper | `POST /agents` | `AgentDeployed` or `DeploymentRejected` | Confirmed version, backtest id shown, rendered go-live record, step-up. V-032, V-006, V-002 rejections in words (B3) |
| Go live | `POST /agents/{id}/go-live` | — | Always `live_unavailable` until counsel signs off (B5, DEC-98). The route exists so the refusal is tested |
| Pause, resume | `POST /agents/{id}/pause`, `.../resume` | `OwnerCommandIssued` | Pause: no step-up, API-7. Resume: step-up, judged by the runtime |
| Hold new openings | `POST /agents/{id}/hold` | `OwnerCommandIssued` with command `hold_openings` (journal spec §9.11) | DEC-191. Sets `exits_only` and nothing else. Lifting it is the owner's alone, with step-up, as a `lift_hold` command (journal spec §9.11); it lifts only the hold, never a latched limit (MI-3) |
| Lift a hold | `POST /agents/{id}/hold/lift` | `OwnerCommandIssued` with command `lift_hold` (journal spec §9.11) | (planned: E10-12) Users only, never a client; step-up required (`lift_hold`); not an API-7 operation, since it adds risk back (DEC-684 item 1) |
| Stop | `POST /agents/{id}/stop` | `OwnerCommandIssued` | Step-up; `release` and the warning digest per DEC-290 item 2; the flat-or-release precondition is the runtime's (DEC-136) |
| Owner exit | `POST /agents/{id}/exits` | `OwnerCommandIssued` (`owner_exit`) | §5.4. Never refused for step-up (mandate spec §6.1) |
| Acknowledge | `POST /agents/{id}/acknowledgments` | `OwnerAcknowledged` | Names the event acknowledged; step-up; independence judged by the executor |
| Kill switch | `POST /kill-switch` | `OwnerCommandIssued` (`kill_switch`) | §5.4. Agent, connection, or workspace scope |
| Re-enable a halted scope | **None in v0.1** | — | The halted state is Proposed (DEC-437 item 21; interim: no halted state), so a kill switch leaves no scope halted and there is nothing to re-enable. If the founder accepts item 21, this spec adds a route, a step-up kind, and the events `ScopeHalted` and `ScopeReenabled` (identity spec §12.1) in its own change |
| Command status | `GET /commands/{event_id}` | — | §5.5 |

### 4.3 Approvals (approval service)

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Inbox | `GET /approvals?state=pending\|resolved` | — | Pending by deadline, then resolved with outcome (D5) |
| Read one | `GET /approvals/{id}` | — | The content object exactly as `ApprovalRequested` holds it, its `content_hash`, the deadline, the grants so far, and the status folded from the agent stream. Never a scorecard, profit estimate, or price target (DEC-126) |
| Delegation preview | `POST /approvals/{id}/delegation-previews` | — | §5.3. Offered only where mandate spec §6.4 allows a scope |
| Respond | `POST /approvals/{id}/responses` | `ApprovalResponseSubmitted` | §5.2. Users only |
| Resolve a notice | `GET /notices/{notice_id}` | `RecordsAccessed` | §3.9 |

### 4.4 Delegations and autonomy

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Read | `GET /agents/{id}/autonomy` | — | D15: the dial as a read-out, the unasked dollars, each delegation's use, remaining caps, expiry, and state (live, suspended and why, spent, expired), tripwires, the review date, connected clients |
| Create or widen | Only through a version (§4.1) or an approval response (§5.2) | `MandateConfirmed` with step-up | Never by a client (DEC-181 item 5) |
| End one | `POST /agents/{id}/delegations/{delegation_id}/end` | `MandateVersionCreated`, `MandateConfirmed` | §5.3. Reducing; no step-up (DEC-197) |
| Away mode | `POST /agents/{id}/away` | Same | DEC-194: a reducing version with an end date; no step-up. Restoring is a risk-increasing confirm |

### 4.5 Connections, policies, members, clients

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| List, read connections | `GET /connections`, `GET /connections/{id}` | — | Opaque id, broker, environment, scopes, the 1× check, data profile, restrictions, agents granted, loss carry (O4). **References only**: no account number, key, or token (API-11) |
| Connect | `POST /connections/oauth/start`, then the broker redirects to the fixed `GET /v1/oauth/alpaca/callback`, outside the workspace prefix because a registered redirect URI cannot carry a workspace id; the workspace comes from the single-use `state` | `ConnectionEstablished` | Step-up before start. PKCE and a single-use `state`; the callback refuses a `state` that does not name a server-issued `env=paper` request for that workspace (DEC-821 item 3), writes the code straight to the vault, and calls no broker; the token-exchange process exchanges it and the executor runs the permission checks, and the API appends the event once their passing results are journaled (§1.4); scopes beyond trading reject the connection (FR-2.2). Flow details are the connections spec's |
| Revoke | `POST /connections/{id}/revoke` | `ConnectionRevoked` | Step-up. Refused while any agent on it holds positions or is not stopped: without the connection nothing can exit or re-protect, so an ordinary revoke is not risk reduction. For a credential the owner believes is compromised, use the next row |
| Revoke now, on compromise | `POST /connections/{id}/revoke` with `compromised: true` | `OwnerCommandIssued` (`kill_switch`, connection scope), then `ConnectionRevoked` (reason `compromised`, journal spec §9.10), in one batch | §5.6. Never waits on positions: the kill switch runs first in the same command, then the credential is revoked |
| Policies | `GET`, `PUT /policies/workspace` | `PolicyChanged` | A value looser than its parent is refused naming the nearest ancestor (FR-1.5); the response lists agents made nonconforming (X1). Step-up |
| Members | `GET /members`, `POST /invitations`, `PATCH`, `DELETE /members/{id}` | Identity spec's events (journal change) | Removing a member ends their sessions and tokens at once |
| My workspaces | `GET /v1/me/workspaces` | — | (planned: E10-10) The signed-in user's own workspace memberships, at the principal's own scope and outside every workspace path: identity spec §4.5's *List one's own workspace memberships*, which [#811](https://github.com/kunwarshivam/mandate/pull/811) adds. Returns only the caller's memberships (API-9) |
| Clients | `GET /clients`, `POST /clients`, `DELETE /clients/{id}` | `ClientConnected`, `ClientRevoked` (journal spec §9.10) | Create needs step-up and shows the scopes in words (E10-8); revoke needs none |

### 4.6 Owner requests, the dry run, and the chat thread

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Dry run | `POST /agents/{id}/dry-run` | `RecordsAccessed` | DEC-190: the decision (`auto`, `ask`, `deny`) and the gate's reason code for a described order, with the client ceiling for a client. Places nothing, creates no approval |
| Owner request | `POST /agents/{id}/requests` | `OwnerRequestSubmitted` (journal spec §9.9) | An instrument, side, and optional size the owner or client asks for. The runtime copies it to its builder, which sizes, clips, and classifies it as for any proposal (rule 4); `requested_by` from the channel (API-6) |
| Chat | `POST /agents/{id}/chat`, `GET .../chat` | The model call, on the agent stream | A reply is quoted model output (API-18). When a message asks for an action, deterministic code returns a card: an owner request with the builder's proposal, gate verdict, and autonomy outcome, or a draft version that opens A6. Sending the card is a separate call carrying its `card_digest` (D14, DEC-192) |

### 4.7 Read models

| Resource | Path | Serves |
|---|---|---|
| Dashboard | `GET /dashboard` | D1: agents with environment, effective mode, positions, P&L (paper labeled simulated), open approvals, recent decisions, alerts |
| Agent | `GET /agents/{id}` | D2: effective mode and every restriction with what it blocks and how it lifts (brief §4.3), limits with headroom in dollars, ladder and size factor, `pending` breaches, the floor, the working universe or pinned list |
| Positions, orders | `GET /agents/{id}/positions`, `.../orders` | D3: ledger and broker quantities, cost basis, risk mark and its age, protection and unprotected intervals; orders with state, `Unknown` shown as unknown |
| P&L | `GET /agents/{id}/pnl` | Realized and unrealized from the ledger fold; disclosures named by document and version, never drafted text (rule 9) |
| Universe and theses | `GET /agents/{id}/universe` | D4: active, removed, and refused instruments with reasons; each thesis as platform-authored quoted content with confidence labeled uncalibrated |
| Plan view | `GET /agents/{id}/plan` | D12: what it watches, what it would do next and under which rule, whether that would run, ask, or be denied, and what would stop it. Writes nothing |
| Daily brief | `GET /agents/{id}/briefs/{date}` | D13: counts and sources only; no profit or outcome claims |
| Scorecards | `GET /agents/{id}/scorecards` | FR-8.4: the user's own results only. Never embedded in an approval, the model picker, or another workspace's view |
| Alerts | `GET /alerts` | G5, from `OwnerAlertSent` and the events it names |
| Health | `GET /health` | The status strip (G1): market data, broker, deployment, relay, each with its last observation time |

### 4.8 Journal and audit (audit explorer backend)

| Operation | Method and path | Notes |
|---|---|---|
| Streams | `GET /journal/streams` | The workspace's streams and heads |
| Events | `GET /journal/streams/{stream_id}/events?after_seq=&limit=` | API-15: `seq` order, each event's canonical body bytes as base64, `hash`, and `prev_hash`; the cursor is the last `seq` |
| One event | `GET /journal/events/{event_id}` | With its artifacts' refs. An event authorized from a session's roles snapshot during a membership-store outage shows its `membership_unverified: true` (identity spec §4.5), and the web audit trail displays it |
| Timeline | `GET /agents/{id}/timeline?types=&from=&to=` | J1: merged from the agent and account streams, with one cursor per stream; display order by `recorded_at` for readability only (journal spec §2) |
| Causal trace | `GET /journal/events/{event_id}/trace` | J2: the `causation_id` chain back to observations; model output as quoted, attributed content |
| Gate decision | `GET /journal/events/{event_id}/gate` | J6: every check with reason code, rule-set version, quotes and marks used |
| Exports | `POST /exports`, `GET /exports/{id}` | J3, journal spec §12: canonical export, or JSON and CSV views naming the verifier digest. `ExportCreated` before serving (API-16) |
| Verification | `POST /verifications`, `GET /verifications/{id}` | J4: runs journal spec §11; `VerificationRun` |
| Surveillance | `GET /surveillance/reports` | J5 |

Every read outside the product views (exports, examination bundles, break-glass) is journaled as
`RecordsAccessed` or `ExportCreated` (journal spec §7).

### 4.8.1 Audit read contracts

This subsection closes the rows above so that tests can be written from it
([DEC-760](../project/decisions/DEC-760.md) to [DEC-767](../project/decisions/DEC-767.md)). The
reads are served by the pure crate `mandate-audit`: it reads the journal and writes nothing. For an
export or a verification it returns the `ExportCreated` or `VerificationRun` draft, and the API's
control-stream writer appends it before anything is served (API-16). The payload schemas of
`ExportCreated`, `VerificationRun`, and `RecordsAccessed` are journal spec §9's.

**Shapes.** Every response composes the shared `Envelope` and every error is a `Problem`, both
defined in `schemas/workspace-api/envelope.schema.json` (**Planned:** #788; until it lands, §3.2
and §3.5 are the contract); ids, event ids, timestamps, decimals, and refs are
the `Id`, `EventId`, `Timestamp`, `Decimal`, and `ContentRef` of
`schemas/workspace-api/common.schema.json`. Every response carries `as_of`, an `AsOf`: one
`Watermark` `{stream_id, seq, hash, recorded_at}` per stream it read. Every union is tagged by a
`kind` member and every enum here is closed (§3.2). **Planned:** the JSON Schemas of the trace, the
gate view, exports, and verification will follow under `schemas/workspace-api/audit/`. The
timeline's response schema is the agent-timeline read model's (`schemas/workspace-api/read-models/`),
and the timeline rules below are its semantics.

**Hash forms.** An event's own chain hashes are bare: every `hash` and `prev_hash` of a page item,
a trace node, a gate view, or a JSON or CSV view row, and the `hash` of a stream `head`, is 64
lowercase hex exactly as journal spec §3 and the body re-hash give it. Everything else is a
`sha256:` `ContentRef`: the `hash` of an `as_of` `Watermark` (the same digest, with the API adding
the prefix), a segment manifest's `manifest_hash`, every `artifact_refs` value, a quoted item's `artifact`,
and every `config_refs` value. An export's `verifier_digest` is bare 64 hex where the journal
records it (`ExportCreated`, **Planned:** #772) and a `sha256:` ref where a view prints it. An anchor leaf's `hash` is bare, as
journal spec §10 records it. Everything inside `payload`, and `actor.build`, is served exactly as
recorded, in whatever form the event's schema gives it.

**Invariants.** Each one is tested with an oracle of its own, as §2 says.

| ID | Invariant | How it is tested |
|---|---|---|
| **AU-1** | **No link leaves the workspace.** A page, trace, timeline, gate view, export, or verification reads only streams whose `stream_id` names the path's workspace. An id of another workspace, an absent id, and a malformed id give the same result: 404 for the requested resource, `not_recorded` for a trace hop | Two workspaces in one journal, with forged `causation_id`s, `intent_id`s, and `evidence` ids in each naming the other's events; no byte of the other workspace appears in any response, and the 404 bodies are equal byte for byte |
| **AU-2** | **Pages are complete.** API-15, with the head read in the same snapshot as the page | Fuzz of page sizes with concurrent appends: concatenated pages equal the stream range, and a chain check over them passes from 64 zeros |
| **AU-3** | **A trace ends.** Every trace returns within its depth and node bounds, and visits each event once, whatever the links (cycles and self-links included) | Fuzz random link graphs with cycles; an independent breadth-first walk gives the same node set and hop statuses |
| **AU-4** | **Model text is only quoted.** In a trace, a gate view, or a timeline, model-authored content appears only in `quoted` members with an author (API-18) | A schema test over the response types; a test with injected text in every model-authored member |
| **AU-5** | **A timeline misses nothing and repeats nothing.** Paging a timeline with its per-stream cursor yields every matching event of each stream exactly once, in that stream's `seq` order | Fuzz with filters and concurrent appends against a k-way merge oracle over the two streams |
| **AU-6** | **Served after recorded.** No export byte and no verification result is served before its `ExportCreated` or `VerificationRun` is `Committed` or `AlreadyCommitted` | Fault injection on the append: a failed or `Ambiguous` append serves nothing |
| **AU-7** | **CSV cells are inert.** No CSV cell starts with `=`, `+`, `-`, `@`, a tab, or a carriage return | Fuzz payload strings over those characters; an independent scan of every cell |
| **AU-8** | **A verification covers its whole range.** A `pass` means every event from `from_seq` to `to_seq` was walked: `checked = to_seq − from_seq + 1`, the first and last `seq` are the range's, and a missing, reordered, or extra event fails | An oracle that counts the range independently; seeded deletions, duplicates, and truncations of the stored range each fail, and an out-of-range `from_seq` or `to_seq` is refused |
| **AU-9** | **Audit output stays in the workspace.** Pages, traces, gate views, timelines, export files, and verification results are served only from the workspace deployment and are never sent to the global control plane. No request log, metric label, or tracing span carries an event body, a payload member, or an export's content. A notice that an export is ready or a verification is done carries only an opaque id and generic text (rule 6, API-10) | A seeded payload string in the journal; after every route is exercised, a scan of the logs, metrics, spans, notices, and the global control plane's inbound traffic finds it nowhere |

**Who may call.** Every route here is the matrix row "Read records, verification; export": a
workspace admin, an auditor, or a service account (§3.7). The role is checked before any id is
resolved, so a viewer, an operator, or an approver gets 403 `forbidden` for every id, present or
not, and learns nothing about existence. This 403 denies the route itself, before and whatever any
id, so §3.5's rule that 403 is never sent for a resource one cannot see does not apply: no resource
is resolved. A client never reaches these routes (§3.8: `read` never
includes the journal) and gets 403. A principal with no membership in the path's workspace gets the
404 of API-9 for every route under it.

#### Ids and the 404 ([DEC-760](../project/decisions/DEC-760.md))

1. A `stream_id` or `event_id` is resolved only among the streams of the path's workspace
   (`acct:{ws}:…`, `agent:{ws}:…`, `ctl:{ws}`, `clock:{ws}`, `ntf:{ws}`, journal spec §2), by the
   same scoped lookup whatever its form. A malformed id is not rejected before the lookup: it is
   looked up and found absent. An id that is absent, malformed, or in another workspace returns
   §3.5's `not_found` problem document, the `Problem` of #788 (**Planned**) with exactly these
   members: `type` `https://mandate.dev/problems/not_found`, `status` 404, `code` `not_found`,
   `title` `Not found`, `effect` `none`, `event_id` `null`, `retryable` `false`, and `violations`
   `[]`. It is the same bytes for every such id; no id, `stream_id`, or other member is added.
2. Query members that are not ids are validated as usual: an out-of-range `limit`, a malformed
   timestamp, or an unknown event type is 422 `invalid` with its `violations`, and is checked
   before any id is resolved, so it reveals nothing about one.

#### Streams and pages (API-15, [DEC-760](../project/decisions/DEC-760.md))

`GET /journal/streams?after=&limit=` lists the workspace's streams in ascending byte order of
`stream_id`, after the `after` stream id if given. Each item is `{stream_id, stream_type, head:
{seq, hash, recorded_at}}`; `stream_type` is `account`, `agent`, `control`, `scheduler`, or
`notice`. A stream with no event yet is not listed. `after` is a stream id, compared by bytes and
never resolved, so it reveals nothing; `limit` has the event page's bounds: an integer from 1 to
1,000, default 100, and outside them is 422 `invalid`, never clamped. The response is `{streams,
next_after, as_of}`, with `next_after` the last listed `stream_id` or `null` when the list ends.
Each page is read in one snapshot, but a listing of several pages is best effort while streams are
being created: a stream whose first event commits after an earlier page was read and whose id sorts
before that page's last id is not in this listing, and appears in a fresh one.

`GET /journal/streams/{stream_id}/events?after_seq=&limit=` returns one page:

| Member | Meaning |
|---|---|
| `stream_id` | As requested |
| `head` | `{seq, hash, recorded_at}`: the stream's head in the snapshot the page was read from |
| `events` | Up to `limit` events with `after_seq < seq ≤ head.seq`, in ascending `seq`. Each is `{seq, event_id, event_type, recorded_at, prev_hash, hash, body}`: `body` is the stored canonical body bytes (journal spec §6.1), standard base64 with padding (RFC 4648 §4); `hash` and `prev_hash` are 64 lowercase hex, as journal spec §3. The other members repeat the body for convenience; the body is what a client checks |
| `next_after_seq` | The last returned `seq`, or `after_seq` when `events` is empty; the next page's cursor |
| `at_head` | `true` when the page ends at `head.seq` |
| `as_of` | API-14's `AsOf` for this stream: one `Watermark` with the `head`'s `seq` and `recorded_at` and its `hash` as a `sha256:` ref |

- `after_seq` is an integer from 0 to 2^53 − 1, default 0. `limit` is an integer from 1 to 1,000,
  default 100. Outside those bounds, or not an integer, is 422 `invalid`; a value is never clamped.
- The page and its `head` are read in one database snapshot, so an append that lands while the
  page is read appears in neither and is on the next page.
- `after_seq` at or beyond `head.seq` returns `200` with an empty `events`, `at_head: true`, and the
  head: the stream exists in the workspace, so this reveals nothing. It is never 404.
- A client checks a page by re-hashing each body against `hash` and chaining `prev_hash` from the
  previous page's last `hash` (64 zeros before `seq` 1). The API never re-serializes a body.

`GET /journal/events/{event_id}` returns one event in the same item shape, with its `stream_id` and
its `artifact_refs` and `config_refs` as stored.

#### Causal trace (J2, FR-7.2, E12-1)

`GET /journal/events/{event_id}/trace` walks from the start event back to its causes. It follows
the envelope's `causation_id` on every event, whatever its row, and the payload link members of the
table below, and nothing else ([DEC-761](../project/decisions/DEC-761.md)). The journal spec
requires a `causation_id` only on its copies of cross-stream facts (§2), on `IntentProposed` (§9.1
rule 10), on `OwnerCommandRefused` (rule 27), on `OrderSubmitted` version 2 (rule 45), and on
`ApprovalResponded` (rule 50). So the
walk never depends on any other `causation_id`. It uses the payload members the closed schemas
carry: a fill names its order, and an order request names its intent. Where a row names, in its
parentheses, the types its `causation_id` targets, those are the expected types, and a target of
another type is `unexpected_type`. Elsewhere any event in the workspace is followed.

Every lookup is scoped (AU-1). An id is resolved only by the workspace-scoped lookup of §4.8.1's
ids rule. A stream id the walk builds from a payload member, such as `agent:{ws}:{agent_id}`, uses
the path's workspace id and is resolved by the same scoped lookup, never by a cross-workspace index.
The lookups by payload member (`client_order_id`, `intent_id`, `thesis_id` on one stream) need an
index on those members in the Postgres adapter; the in-memory reader scans.

| From | Payload link member | Target and where it is looked up |
|---|---|---|
| `FillApplied`, `LateFillApplied` | `payload.client_order_id` | Every `OrderSubmitted` with that `client_order_id` and a lower `seq`, on the same account stream (one per attempt) |
| `OrderSubmitted` version 2 | none (its `causation_id` is its `OrderRequestRecorded`, rule 45) | — |
| `OrderSubmitted` version 1 | none | The walk ends here unless a `causation_id` is recorded: version 1 has no companion and names no intent |
| `OrderRequestRecorded` | `payload.intent_id` | The `IntentReceived` and every `GateDecided` with that `intent_id` on the same account stream, and the `IntentProposed` whose `event_id` is the `intent_id` on `agent:{ws}:{payload.agent_id}`. A null `intent_id` (protective orders, a flatten) is no link |
| `IntentReceived`, `GateDecided`, `ProtectionChanged` | `payload.intent_id` | As the row above, with `agent_id` from the `IntentReceived` (or the `ProtectionChanged`'s own) |
| `IntentProposed` | none (its `causation_id` is one of `DecisionMade`, `ApprovalRevalidated`, `OwnerExitRequested`, §9.1 rule 10) | — |
| `ApprovalRevalidated` | `payload.approval` | The `ApprovalRequested` whose `event_id` is `approval`, on the same agent stream. Its `causation_id` is followed as for any event; the runtime writes its `ApprovalResponded` there |
| `ApprovalResponded` | none (its `causation_id` is its `ApprovalResponseSubmitted` on `ctl:{ws}`, rule 50) | — |
| `ApprovalRequested` | `payload.content.evidence.outputs[].event_id` | Each `ModelOutputRecorded` the approval's content cites, on the same agent stream (§9.6) |
| `DecisionMade` | `payload.outputs_used[]` | Each `ModelOutputRecorded` on the same agent stream |
| `ModelOutputRecorded` | `payload.evidence[]`; `payload.thesis_id` | Each event named in `evidence`, on any stream of the workspace (observations, model invocations); and the `ThesisProposed` or `ThesisRevised` with that `thesis_id` on the same agent stream |
| Any other event, `OrderAbandoned` included | none | Only its `causation_id` (`OrderAbandoned` has no closed schema yet, so no payload member is a link; §12 question 7) |

The chain of a fill an agent proposed with no approval runs `FillApplied` → `OrderSubmitted` →
`OrderRequestRecorded` (`causation_id`) → `IntentReceived`, every `GateDecided`, and the agent
stream's `IntentProposed` → `DecisionMade` (`causation_id`, rule 10) → each `ModelOutputRecorded`
→ its evidence (`ObservationRecorded`, …) and its thesis.

With an owner's approval, `IntentProposed` → `ApprovalRevalidated` (`causation_id`, rule 10) →
`ApprovalRequested` (`payload.approval`) → each `ModelOutputRecorded` the approval cites → its
evidence and thesis. The answer's side runs `ApprovalRevalidated` → `ApprovalResponded` → the
control stream's `ApprovalResponseSubmitted`, through each `causation_id` as recorded.

No recorded member the journal spec requires links an approval to its `DecisionMade`. That hop is
shown only when `ApprovalRequested`'s `causation_id` names the decision, as the runtime writes it
today; otherwise the decision is not in the trace (§12 question 7).

`ModelInvocationRecorded` is reached when an event names it (in `evidence`). Its own link members
close with its schema (§12 question 7).

The walk is breadth first from the start event (depth 0): an event's `causation_id` first, then the
row's members in the table's order, and within a list member, in the list's order. All its reads
use one database snapshot, and `as_of` is one `Watermark` per stream the walk read. Bounds and
outcomes ([DEC-762](../project/decisions/DEC-762.md)):

- **Bounds.** A trace goes at most 16 hops deep, holds at most 256 events, and records at most 1,024
  hops. A link not followed because of any bound is a hop with status `beyond_bound`, and the
  response has `truncated: true`. Once 1,024 hops are recorded, the walk stops and adds no further
  hop.
- **Each event once.** A link to an event already in the trace is a hop with status
  `already_shown` naming that event; it is not followed again. Cycles and self-links end here.
- **A target that cannot be shown** (absent, malformed, in another workspace, or on a stream the row
  does not name) is a hop with status `not_recorded`, and nothing else: no id echoed beyond the link
  member's own value, which the caller's event already holds. Foreign and absent are identical.
- **A target of the wrong type** in the workspace (a `causation_id` that names, say, a `MarkUpdated`
  where the row names `DecisionMade`) is a hop with status `unexpected_type` and the found
  `event_type`; it is shown and not followed.

The response is `{start, nodes, hops, truncated, as_of}`. `nodes` are events in the page item shape
of the section above, each with its `stream_id` (a trace spans several streams, and `as_of` is per
stream, as the one-event read, the gate view, and the timeline carry it), its `depth`, and its
`quoted` list. `hops` are `{from, link, to, status}` with `from` an
`event_id`, `link` the member name (`causation_id`, `payload.intent_id`, …), and `to` the target's
`event_id` or `null` for `not_recorded`. A start event outside the workspace is the 404 above.

**Model output (API-18).** A node's `body` is the record, for checking. What a client renders as
model output comes only from the node's `quoted` list, one item per model-authored member the
node's event holds, in the order below. Each item is `{path, quoted}`: `path` is the JSON Pointer
(RFC 6901) of the quoted member in the event body (`/payload/invalidation`, …), and `quoted` is a
`QuotedContent` of `schemas/workspace-api/common.schema.json` exactly as that schema closes it,
with no member added. `path` sits on the wrapper because `QuotedContent` has no member for it. Its
members:

- `author`: `owner_selected` when the output is a signal model's that the agent's confirmed mandate
  names (the owner chose it: "Output of software you selected", mandate spec §6.4, journal spec
  §9's evidence label); `platform_authored` for every other output, the research agent's theses and
  the compiler's invocations included (mandate spec §8.1, §8.4). Where the trace cannot tell, it is
  `platform_authored`, which never presents platform output as the owner's choice.
- `model_id` and `model_version`: the model or research agent id and version the event records.
- `produced_at`: the event's `recorded_at`. `event_id`: the node's `event_id`.
- `text` and `artifact`: for a member that holds the text inline, `text` is its recorded value (a
  string as recorded, any other value as its canonical JSON, journal spec §4) and `artifact` is
  `null`. For a member that holds an artifact ref, `artifact` is that ref as a `sha256:`
  `ContentRef` and `text` is the empty string: artifact content is never inlined, and a client
  fetches it by ref and renders it as this item's quoted content.

The model's content hash is not a `QuotedContent` member; it stays in the node's `body`. The
quoted members are: in `ThesisProposed` and `ThesisRevised` `payload.invalidation`,
`payload.evidence_sources`, and the prompt, response, and autopsy artifacts; in
`ModelOutputRecorded` `payload.invalidation` and `payload.thesis_ref`; in
`ModelInvocationRecorded` its prompt and response artifacts. No other member of any node is ever
typed to hold model text, and no hop or status is derived from it.

#### Gate view (J6)

`GET /journal/events/{event_id}/gate` serves one `GateDecided`, allows included. Any other event
type in the workspace is 422 `invalid` with violation `not_a_gate_decision`; outside it, the 404.
Members ([DEC-763](../project/decisions/DEC-763.md)):

| Member | Source in the `GateDecided` |
|---|---|
| `event_id`, `stream_id`, `seq`, `recorded_at`, `hash` | The envelope |
| `intent_id`, `verdict`, `reason_code`, `data_profile`, `risk_clock` | The payload, as recorded |
| `checks` | `payload.checks` in recorded order, each `{id, result, inputs, computed, reason_code}`. `result` is a closed set: `pass`, `fail`, or `not_reached`, the values the executor writes. A recorded value outside the set is served as `other`, with the recorded text in `recorded_result`, and never interpreted. `reason_code` is the decision's `reason_code` on the first check whose result is `fail`, and `null` on every other, as trading spec §9.1 reports the first failing check's code. A verdict other than `allow` with no `fail` check (a `hold` or `defer` that no check records as failed) leaves every check's `reason_code` `null`. The decision's own `reason_code` is still served at the top, and `decisive_check` is `null` |
| `decisive_check` | The `id` of the first check whose result is `fail`, or `null` |
| `rule_set` | `config_refs.rule_set`: the rule-set version the gate ran |
| `config_refs` | Every `config_refs` entry as stored (`fee_config`, `trading_calendar`, `instrument_snapshot`, `mandate_version`, `rule_set`) |
| `quotes_used`, `marks_used` | The payload's, as recorded, each with its `as_of` or `source` |
| `body` | The canonical body, base64, to check the rest against |

The gate view adds nothing the record does not hold: no value is recomputed or looked up elsewhere.

#### Timeline (J1, FR-7.3, E12-2)

`GET /agents/{agent_id}/timeline?after=&types=&from=&to=&limit=` merges the agent's stream
`agent:{ws}:{agent_id}` with the account stream of each connection the agent was deployed on, as its
`AgentDeployed` events name them, through the connection's `account_ref` (connections spec §3,
CN-5). Until `ConnectionEstablished` journals `account_ref` (E7-17), the API reads it from the
connection record. An unknown agent is the 404.

- **Which account-stream events belong to the agent** ([DEC-764](../project/decisions/DEC-764.md)):
  one whose `payload.agent_id` (or `payload.agent`, as `AgentModeApplied` names it) is the agent
  or `*`; one whose `payload.intent_id` is an intent whose
  `IntentReceived` names the agent; one whose `payload.client_order_id` is an order whose
  `OrderRequestRecorded` names the agent; one whose `causation_id` names an event of the agent's
  stream; and the account-wide records that change what every agent there may do:
  `AccountRestrictionChanged`, and `KillSwitchActivated` at connection or workspace scope.
  Everything else on the account stream (marks, fees, settlement, snapshots) is not the agent's and
  is reached through the stream pages.
- **Cursor:** one per stream. `after` is repeated, `after=<stream_id>:<seq>`, one per stream; a
  stream not named starts at 0. The response's `next` is the same list for the next page. A
  `stream_id` in `after` that is not one of this timeline's streams is the 404.
- **Order:** a k-way merge. Each stream is read in `seq` order; at each step the next event is the
  stream head with the least `(recorded_at, stream_id)`. So events display by `recorded_at`, then
  `stream_id`, then `seq`, except that one stream's own events never leave `seq` order. The order is
  for readability only; nothing is inferred from it across streams (journal spec §2).
- **Filters:** `types` is a comma-separated list of journal spec §9 event types (an unknown one is
  422); `from` (inclusive) and `to` (exclusive) are journal spec §4.7 timestamps compared with
  `recorded_at`. An event a filter excludes is still consumed: its stream's cursor moves past it, so
  pages stay complete (AU-5).
- **Page:** `limit` 1 to 1,000, default 100. One page consumes at most 10,000 events per stream; a
  page that reaches that bound may return fewer than `limit` events, with `more: true` and the
  cursor advanced. The response is `{events, next, more, as_of}`; each event is the page item shape
  plus its `stream_id`, read in one snapshot per stream.

#### Exports and derived views (J3, FR-7.4, API-16)

`POST /exports` (with `Idempotency-Key`) takes `{scope, from, to, format}`:

- `scope` is `{kind: "agent", agent_id}` (the timeline's streams) or `{kind: "streams",
  stream_ids}`. `from` and `to` are timestamps. Per stream, the export holds the contiguous `seq`
  range from the first event with `recorded_at ≥ from` to the last with `recorded_at < to`, so each
  range has a trusted start (the previous event's `hash`) and verifies under journal spec §11.
  Every stream's range is found and read in one database snapshot, so an export's ranges, and its
  `as_of`, describe one moment, whatever is appended while it is built.
- `format` is `canonical`, `json`, or `csv`. Every export builds the canonical export first (journal
  spec §12: segment files, manifests, anchors with inclusion proofs). Its anchor is the canonical
  export's **verifier digest** (journal spec §12, DEC-265 item 3: `ExportBundle::verifier_digest` in
  `mandate-journal-cold`). `json` and `csv` are views derived from it, and each names that digest
  as `verifier_digest`.
- The API appends `ExportCreated` (its members are journal spec §9's; it records the bare-hex
  `verifier_digest`, **Planned:** added by journal spec #772) and serves
  nothing until it is committed (AU-6). Response `202 {export_id, phase: "recorded"}`;
  `GET /exports/{id}` serves the files. Each download is journaled as `RecordsAccessed` before its
  bytes are served ([DEC-766](../project/decisions/DEC-766.md)).

The views ([DEC-765](../project/decisions/DEC-765.md)), in event order per stream, streams in
ascending `stream_id` bytes:

- **JSON lines.** UTF-8, LF after every line. The first line is the canonical JSON (journal spec §4)
  of `{"kind": "audit_view", "view_version": 1, "format": "jsonl", "verifier_digest": "sha256:…"}`.
  Every other line is the canonical JSON of `{stream_id, seq, event_id, event_type, schema_version,
  environment, recorded_at, event_time, actor, causation_id, correlation_id, config_refs, payload,
  prev_hash, hash}`, the members copied from the body.
- **CSV.** RFC 4180: UTF-8 without a byte-order mark, comma separators, CRLF after every record,
  every field enclosed in double quotes and an embedded double quote doubled. The header row is
  `verifier_digest,stream_id,seq,event_id,event_type,schema_version,environment,recorded_at,event_time,actor_kind,actor_id,causation_id,correlation_id,prev_hash,hash,payload`.
  `verifier_digest` repeats on every row, as a `sha256:` ref, so a cut-out row still names its
  source. `payload` is the
  payload's canonical JSON. A null is an empty field.
- **Formula injection.** A CSV cell whose first character is `=`, `+`, `-`, `@`, a tab (U+0009), or a
  carriage return (U+000D) is written with a single quote (`'`) before it. The canonical export is
  the record; the view is for reading, and that leading quote is the view's, not the record's.
- **Only the canonical export is checkable.** Neither view holds the canonical body bytes, so
  neither can be re-hashed or chain-checked on its own; a reader checks a view's rows against the
  canonical export its `verifier_digest` names. The CSV view also leaves out `config_refs`; the JSON
  lines view and the canonical export carry them.

#### Verification (J4, FR-7.5, E12-3)

`POST /verifications` (with `Idempotency-Key`) takes `{stream_id, from_seq, to_seq, trusted_start}`:

- `from_seq` is at least 1; `to_seq` is an integer or `null` for the head read at start. A
  `from_seq` or `to_seq` above the stream's head at start, or `to_seq < from_seq`, is 422 `invalid`
  with violation `range`.
- `trusted_start` is `{kind: "genesis"}` (only with `from_seq` 1: 64 zeros), `{kind: "manifest",
  manifest_hash}` (a `SegmentExported` manifest of this stream whose `first_seq` is `from_seq`), or
  `{kind: "anchor", anchor_event_id}` (an `AnchorComputed` whose leaf for this stream has `seq` =
  `from_seq` − 1). The trusted `prev_hash` is read from that manifest or anchor in the workspace's
  journal and artifact store, never from the request ([DEC-767](../project/decisions/DEC-767.md)). A
  manifest or anchor that is absent, foreign, or does not fit the range is 422 `invalid` with
  violation `trusted_start`, the same for absent and foreign.
- One run covers at most 1,000,000 events; more is 422.
- **Coverage (AU-8).** The run reads the events of `from_seq` to `to_seq` in one snapshot and walks
  them in `seq` order. The first event read must be `from_seq` and the last `to_seq`; an event the
  walk expects and does not find fails `seq_gap` at the expected `seq`. `checked` is the number of
  events walked. `pass` requires `checked = to_seq − from_seq + 1` and no failure.
- **Per-event checks:** journal spec §11's, in its order, on every event; the first failure is
  reported.
- **Per-range checks**, run only when no per-event check failed, and reported in this order:
  - `anchor_head_mismatch`: for every `AnchorComputed` on `ctl:{ws}` (up to its head at start)
    whose leaf for this stream has a `seq` inside the range, reported at that `seq`.
  - `anchor_root_mismatch` and `tsa_token_invalid`: for those anchors and for the trusted-start
    anchor itself, reported at the anchored `seq` (`from_seq` − 1 for the trusted start).
  - `segment_manifest_mismatch`: for every `SegmentExported` manifest of this stream whose range
    lies wholly inside the run's range, and for the trusted-start manifest, reported at the
    segment's first `seq`.
  - `segment_gap`: between consecutive such manifests, reported at the first missing `seq`.
  - On an agent stream, `intent_action_mismatch` and `mode_event_mismatch` (§9.1, §11), only where
    the event they reference is inside the range. As §11 says, a reference to an event before the
    trusted start is not checked by that range. A `mode_event` that names no earlier event fails
    only on a full chain: `from_seq` 1 with `genesis`, to the head.
  - A `genesis` start runs these checks like any other kind; the kind changes only where the
    trusted `prev_hash` comes from and which trusted-start object is checked.
- **Result:** `{stream_id, from_seq, to_seq, trusted_start, result: "pass" | "fail", checked,
  first_failure: {seq, check} | null}`, with `check` one of §11's codes.
- The API appends `VerificationRun` with that result and serves the result only once it is
  committed (AU-6). Response `202 {verification_id, phase: "recorded" | "running"}`;
  `GET /verifications/{id}` returns the result. The API takes no other action on a failure. Journal
  spec §11's response to one (SEV-1; pausing the affected agents, or freezing mandate and deployment
  changes for a control stream; legal hold; a new writer epoch from `IntegrityIncidentRecorded`;
  the customer notice) is taken by whoever is on call (infrastructure design §8.4), through the
  "Journal verification failure" alert that a failing `VerificationRun` raises (infrastructure
  design §8.2, runbook RB-09).

### 4.9 How the web app's fixtures map

The web app runs on recorded fixtures today (`web/src/fixtures/`). Each fixture shape has its source
here, so replacing fixtures with calls changes no screen contract:

| Fixture (`web/src/fixtures/types.ts`) | Source |
|---|---|
| `Workspace.health` | `GET /health` |
| `Agent` (mandate, provenance, mode, restrictions, state, positions, orders, fills) | `GET /agents/{id}`, `.../positions`, `.../orders`, `GET /mandate-versions/{hash}` |
| `Approval` (bound order, risk impact, evidence, grants) | `GET /approvals/{id}` |
| `GateDecision`, `TimelineEvent` | `GET /journal/events/{id}/gate`, `GET /agents/{id}/timeline` |
| `Connection` | `GET /connections/{id}` |
| `mock-runtime.tsx` command phases (`sent`, `recorded`, `undelivered`, `unknown`) | §5.5's phases; `undelivered` is `effect: none` |

### 4.10 Routes outside the workspace prefix

These routes name no `{workspace_id}`: each acts for the caller's own principal or session, so no
workspace exists to name before it is authorized (identity spec §4.5, §6.4).

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| List one's own workspace memberships | `GET /v1/me/workspaces` | — | Authorized by the `self` row "List one's own workspace memberships" (identity spec §4.2): a full session of a user principal only. Returns the caller's `active` memberships as `{workspace_id, label, roles}` from the membership index, reading no workspace's data; absent and foreign workspaces look the same. A failed index read is `membership_unavailable` ([DEC-816](../project/decisions/DEC-816.md) item 1) |

Owed to the identity spec's lanes, and listed so the prefix rule above is complete: `GET /v1/me/session`,
`POST /v1/reduction-sessions/challenges`, and `POST /v1/reduction-sessions` (identity spec §6.4 route 2;
they sit outside the matrix, DEC-816 item 7). Every failure of the last is the one answer, 401 `unauthenticated` (§3.5), with no distinguishing timing, and `GET /v1/oauth/alpaca/callback` (§4.5).

### 4.11 Notification addresses (web push)

A member's own push address: where the dispatcher sends their opaque notices (notifications spec
§4.6, NT-2). The address is personal data held in the vault, never in the journal, a log, a metric,
or a response (API-11's shape). Unlike §4.10's routes, these name the workspace: every path below sits under `/v1/workspaces/{workspace_id}`, because the addresses belong to it. These routes add no trading rule and change no mandate (DEC-795).

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Set this browser's push address | `PUT /me/channels/web_push` | `NotificationAddressChanged` (`action: added`) with its `OwnerAlertSent` (`notification_address_changed`), one batch (journal change, owed by the journal spec change that closes §9.11's notification records) | §5.7. Step-up. The endpoint and keys go to the vault; the event carries only `address_ref`. An endpoint off the push-service allowlist is `invalid` (notifications spec §4.6, [DEC-792](../project/decisions/DEC-792.md)). An endpoint the member already holds as an active address answers `200` with its `address_ref` and appends nothing. A member holds at most 10 active addresses; an 11th is `address_limit` (409, §3.5) |
| Remove a push address | `POST /me/channels/web_push/{address_ref}/remove` | `NotificationAddressChanged` (`action: removed`) with its `OwnerAlertSent`, one batch | §5.7. Step-up. An `address_ref` that is not the member's own or does not exist is 404 exactly as any missing resource (API-9, NT-10). One already removed answers `200` and appends nothing. The pull channels (`web_inbox`, `cli_inbox`) are not addresses and are never affected |
| List one's push addresses | `GET /me/channels/web_push` | — | Each active `address_ref` with when it was added and its delivery status (`ok` or `unreachable`, notifications spec §5.6); never an endpoint, a key, or a hash of either |

**Active addresses.** An address is active while its last `NotificationAddressChanged` is
`added`. The set is derived from the journal, never from the vault: a vault entry with no committed
event, and one kept after its removal for the last send (§5.7), are not active. Only active
addresses count toward the limit, match an endpoint already held, or are listed, and a re-add after
a removal is a new address under a new `address_ref`. An active address marked `unreachable` (an
allowlist change or a rejection, notifications spec §5.6) stays active, counts toward the limit,
and shows its status until the member removes it, or sets the same endpoint again, which replaces
it under a new `address_ref` (§5.7).

**Not risk reduction.** Neither change is an API-7 operation. Someone holding a stolen session could
add their own browser to watch a member's notices, or remove the member's address to silence the
safety notices that are the member's out-of-band signal of a takeover (identity spec ID-14), so
both need step-up and may be refused for missing step-up, a rate limit, or a frozen control stream
like any other change.

**Only one's own.** `/me` resolves to the authenticated user; no route sets or removes another
member's address, and a client, a service account, the host CLI, and a platform operator have none
(API-6). The identity spec §4.2 row "Add or remove one's own notification address (a push
subscription; later an email address)" authorizes setting and removing: **S** for both,
`own` in the five workspace columns (workspace admin, operator, approver, viewer, auditor), at this
workspace's scope through a membership that reaches it, and blank everywhere else. The row "List
one's own notification addresses (opaque references only)", `own` in the same columns without
**S**, authorizes the listing. Both are in identity spec §4.2 ([#811](https://github.com/kunwarshivam/mandate/pull/811),
DEC-816 item 2), which also prints them in §3.7 and the step-up kind `notification_address` in §3.6.
Addresses belong to the workspace: each workspace's addresses live in its own vault namespace
(identity spec §9.1), and a member who belongs to two workspaces sets one in each.
Holding an address grants nothing: what a member receives stays the receive column's (identity
spec §4.1).

**Deactivation.** When a deactivation or removal ends a member's membership in this workspace,
workspace services append a `removed` `NotificationAddressChanged` for each of the member's active
addresses in this workspace inside the deactivation's own commit, all or nothing (identity spec
§5.2 step 3, [#811](https://github.com/kunwarshivam/mandate/pull/811), DEC-816 item 7). Each carries
the same actor as the `MemberDeactivated` or `MemberRemoved` it rides in: on the request path the
authenticated principal, the admin or the member who left (DEC-642, identity spec §4.5); when the
directory-sync or SCIM process deactivates, `system` (identity spec §11.1). Its step-up evidence is
`null`, which the journal row allows only for a `removed` riding in such a commit, and it raises no
`OwnerAlertSent`. The dispatcher sends the member nothing more, the workspace admins get
`member_deactivated` (notifications spec §3.2), and the vault entries are then swept as for any
removal. The event's members match identity spec §12.1's row: `member`, `channel`, `action`,
`address_ref` (the opaque address reference), and the step-up evidence or `null`.

---

## 5. Safety-relevant request and response shapes

Members are listed in full. Every request also carries `Idempotency-Key`. `step_up` is always
`{assertion_id, authenticated_at, method}` or `null`, the shape journal spec §9.2 records. `record`
is always `{artifact: "sha256:…", ui_build: "sha256:…"}`: the rendered record screen the client
uploaded to the artifact store, and the build that rendered it (brief §4.1, mandate spec §10).

**The API-7 operations are lenient about their bodies** (DEC-682 item 27). Their shapes below are
what a client should send, but only their hard members are strict: the path ids, the kill switch's
`scope`, an owner exit's `instrument`, and a Skip's `verdict` and `content_hash`. Any other member
that is omitted, unknown, or fails to parse is dropped, not refused, and the `202` lists each
dropped member's JSON pointer in `dropped`. An owner exit's bid confirmation is all or nothing: if
any part is missing or unparsable, every part sent is dropped and equities wait for the session.
This applies to pause, holding new openings, the kill switch, an owner exit, Skip, ending a
delegation, and away mode; an `approved` and every other operation is judged strictly.
- A pause or hold body that is not JSON at all is read as `{}`, with `dropped: [""]`.
- A workspace-scope kill switch naming a non-null `scope.id` has the id dropped and listed.
- Skip's `content_hash` stays strict: an answer binds what was shown (API-12).
- Idempotency (API-4) compares the members kept, not those dropped: a repeat that differs only in
  a dropped member is the same call.
- A repeat never applies a member the first call dropped. A repeat that differs only in dropped
  members replays the original outcome, `dropped` included, so the client sees its fix was not
  applied; one that now carries a valid value for a dropped member differs in a kept member and is
  refused `idempotency_conflict`. Either way, a client that fixes a dropped member sends it under a
  new key.

### 5.1 Confirm a mandate version

`POST /mandate-versions/{mandate_version}/confirm`

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | id or `null` | The deployed agent the version is for, or `null` for a new mandate |
| `base_version` | ref or `null` | The version the client believes is in force for that agent; `null` for a new mandate |
| `confirmed_paths` | `[pointer]` | Every path the owner confirmed (journal rule 18) |
| `warnings_acknowledged` | `[code]` | Each warning the screen showed and the owner acknowledged |
| `classification_shown` | `risk_increasing` \| `risk_reducing` \| `neutral` | What A6 showed |
| `screen_digest` | ref | From `GET .../confirmation`; proves the screen rendered the server's data |
| `record` | record | The rendered A5 or A6 screen |
| `step_up` | step-up or `null` | Bound to digest = SHA-256 of `{mandate_version, agent_id, base_version}` |

The server checks, in order, and refuses at the first failure: the principal is a user with the
role (API-5, API-6); the version exists in this workspace; `base_version` is the agent's version in
force, else `stale_base`; the server's own §9.2 classification of `base_version` → `mandate_version`
equals `classification_shown`, else `classification_changed`; every path V-020 requires is in
`confirmed_paths`; the screen digest matches; for `risk_increasing`, step-up is present, else
`step_up_required`, and passes identity spec §7.2 step 4, else that check's code (§3.5). While the
control stream is frozen (journal spec §11), every confirm is
refused with `control_stream_frozen`; the API-7 operations, including §4.4's reducing shortcuts,
are still recorded.

It then commits `MandateConfirmed` version 2 with the agent link, `agent_id` and `base_version`
(journal spec §9.9, DEC-436 item 14). Response `202`:

| Member | Meaning |
|---|---|
| `command_id` | The `MandateConfirmed` event id |
| `phase` | `recorded` |
| `classification` | The server's |
| `applies` | `now` (reducing, neutral) or `next_safe_point` (increasing), mandate spec §2.2 |

Application is the executor's `MandateVersionApplied`, which may still reject (for example
`increase_blocked_while_latched`); §5.5 reports it.

### 5.2 Respond to an approval

`POST /approvals/{approval_id}/responses`

| Member | Type | Meaning |
|---|---|---|
| `verdict` | `approved` \| `skipped` | |
| `content_hash` | ref | The content hash of the request the client rendered (API-12) |
| `record` | record | The rendered D6 screen, including whether model output was expanded |
| `step_up` | step-up or `null` | Required for `approved`, bound to the content hash (or to §5.3's digest with a delegation). `null` for `skipped` |
| `delegation` | `null` or `{preview_id, mandate_version}` | The shape chosen on the card (§5.3); always `null` for `skipped` (a Skip naming one has it dropped, below) |

The API refuses only: an unauthenticated or non-user principal, a user who is not listed in
`autonomy.approval.approvers` and holding an approving role (§3.7), an unknown approval, and, for
`approved` only, missing or unbound step-up: bound to the content hash, or with a delegation to the
preview's `step_up_digest` (§5.3), else 401 with the step-up code §3.5 names. It does not judge the deadline, the quorum,
independence, or re-validation: the runtime does (checks 1 to 12). A `skipped` is never refused for
anything else (API-7).

It commits `ApprovalResponseSubmitted` with `submitted_at` set by the server at receipt, never by the
client's clock. Response `202`: `{response_id, phase: "recorded"}`. The outcome arrives through
§5.5: `admitted`, `counted`, or `refused` with its reason from `ApprovalResponded`; then `act` or
`skip` from `ApprovalRevalidated`. The UI shows "approved" only for `act`; if it cannot confirm the
response was recorded before the deadline it shows "not recorded; the default applied" (brief §5,
rule 3 row).

### 5.3 Delegations on the card, and ending one

`POST /approvals/{approval_id}/delegation-previews`

| Member | Type | Meaning |
|---|---|---|
| `shape` | `until_close` \| `instrument_for_time` \| `kind_for_time` | Mandate spec §6.5's three shapes |
| `max_orders` | integer 1 to 1,000 | Entered by the owner |
| `max_order_usd`, `max_total_usd` | decimal or `null` | Entered for the two timed shapes; derived for `until_close` |
| `expires_at` | timestamp or `null` | Entered for the timed shapes, at most 30 days; `null` for `until_close` |

The server refuses a preview where mandate spec §6.4 offers no scope: an admission, a two-approver
ask, a live environment, or a client principal. Otherwise it builds the new mandate document
deterministically from the version in force plus the delegation, validates it (V-041 to V-045), and
returns `{preview_id, mandate_version, delegation: {every §6.5 field}, unasked_usd_after,
classification: "risk_increasing", step_up_digest}`, where `step_up_digest` is SHA-256 of
`{content_hash, mandate_version}`. One step-up over that digest approves the action and confirms the
version, as §6.5 requires. The response call then commits one batch: `MandateVersionCreated`,
`MandateConfirmed`, and `ApprovalResponseSubmitted` naming the shape; the new delegation's
`source_approval_id` is this approval.
All or nothing (journal spec §5.1). The version applies at the next safe point after this action
is submitted, so it neither skips nor covers the action it was offered on.

`POST /agents/{agent_id}/delegations/{delegation_id}/end` takes only `record` (or `null`). The
server builds the version in force minus that delegation, checks that §9.2 classifies it
risk-reducing (it always does for a removal), and commits `MandateVersionCreated` and
`MandateConfirmed` with no step-up. If the version in force moved since the owner tapped, the server
rebuilds from the current one instead of refusing (API-19). If the delegation is already gone, it
returns `200` with `already_ended` and commits nothing.

### 5.4 The kill switch and owner commands

`POST /kill-switch`

| Member | Type | Meaning |
|---|---|---|
| `scope` | `{kind: "agent" \| "connection" \| "workspace", id}` | `id` is the agent or connection id, or `null` for the workspace. The organization scope is the client issuing one workspace-scope call per workspace, each journaled on its own (DEC-436 item 13). Each such call is authorized at the organization's scope (identity spec §4.5, DEC-832): the route's workspace must be one of the organization's that the store lists, and the client retries each workspace until it reports the call committed |
| `environment_shown` | `paper` \| `live` | What the screen said. Recorded; a mismatch never refuses |
| `owner_exit` | `null` or `[{asset_id, bid, bid_size, quoted_at, floor}]` | The optional bid confirmation for equities outside the regular session (D10). Absent or stale, the switch still cancels and stops, and equity sells wait for the session |
| `record` | record or `null` | The rendered D10 screen. Optional: Stop must work when the dashboard has not loaded (brief §5, rule 13 row) |
| `step_up` | step-up or `null` | Needed only for the owner-exit privilege (DEC-158 option (c)) |

The API checks the principal, the role, and that the scope id is in this workspace. Nothing else
can refuse it. It commits `OwnerCommandIssued` (`kill_switch`) with `submitted_at` at receipt and
returns `202 {command_id, phase: "recorded", step_up_status}`, where `step_up_status` is `bound`,
`missing`, or `unbound`, advisory only: the stream owners judge it (mandate spec §6.1).

**Owner exit** (`POST /agents/{id}/exits`) has `instrument` (an `asset_id`), `bid`, `bid_size`,
`quoted_at`, `floor` (each `null` in the regular session or for crypto), `record`, and `step_up`.
Without valid step-up it is still routed and loses only the owner-exit privilege (mandate spec §6.1).

**Pause** has only `record` or `null`. **Resume, Stop, acknowledge** carry `step_up`; the API
forwards missing evidence and lets the stream owner refuse it with `OwnerCommandRefused`, so the
refusal is journaled where the specs put it.

### 5.5 Command status

`GET /commands/{event_id}` returns:

| Member | Meaning |
|---|---|
| `phase` | `recorded`: the control-stream event is committed. `taken`: the owning stream copied it (`causation_id` points to it). `applied`: its effect is journaled (`AgentModeChanged`, `KillSwitchActivated`, `MandateVersionApplied` applied, `ApprovalRevalidated` `act`). `refused`: the owner refused it (`OwnerCommandRefused`, `ApprovalResponded` refused, `MandateVersionApplied` rejected). `ended`: an approval response whose approval ended otherwise (`skip`, timeout, cancellation) |
| `steps` | Each owning-stream event caused by it, in `seq` order, with `stream_id`, `seq`, `event_type`, `recorded_at`, and a reason code. A kill switch lists mode applied, orders canceled and confirmed, sells submitted or deferred (D10 "In progress") |
| `as_of` | The watermarks read (§6.1) |

`unknown` is not a phase here: a command whose append was ambiguous has no confirmed event, so the
client keeps the `event_id` from §3.4 and polls this resource until the event exists or the client
gives up and says "the result is unknown; we are checking".

### 5.6 Revoke now, on a compromised credential

`POST /connections/{connection_id}/revoke` with:

| Member | Type | Meaning |
|---|---|---|
| `compromised` | `true` | Selects this path. `false` or absent is the ordinary revoke of §4.5 |
| `record` | record or `null` | The rendered confirmation, which states what the next paragraphs say |
| `step_up` | step-up or `null` | Bound to digest = SHA-256 of `{connection_id, compromised: true}` (kind `connection`) |

The API commits one batch, in this order: `OwnerCommandIssued` with `kill_switch` at the
connection's scope, then `ConnectionRevoked` with reason `compromised`. Revocation never waits on
positions, on fills, or on the regular session. The executor processes the batch in `seq` order:

1. **The kill switch first** (trading spec §5.5): every agent on the connection to `stopped`, the
   broker's cancel-all and close-position requests sent with the executor's current lease.
   Whatever the broker accepts, queues, or fills stands at the broker after revocation; nothing on
   our side holds or cancels it (rule 13).
2. **Then the revocation:** the executor sends nothing more with that credential, the vault
   destroys it and ends every lease (infrastructure §5.4), and, where the broker offers token
   revocation (OAuth), the platform revokes the token there.
3. **What is left is the owner's, and the owner is told so.** An exit the executor would have
   deferred and re-driven later (equities outside the regular session) is not re-driven, because no
   credential remains; protection was canceled by step 1. The confirmation and the alert say which
   positions remain at the broker, unprotected, and that the owner must also revoke the leaked key
   at the broker, since revoking it on our side does not stop someone else who holds it. This
   residual is listed for the threat model (#557).

The kill-switch half is an API-7 operation: without valid step-up, or while the control stream is
frozen, the API still commits the kill switch, and refuses only the revocation with
the step-up code §3.5 names (`effect: recorded` for the kill switch). The batch is otherwise all or nothing.

### 5.7 Set or remove a push address

`PUT /me/channels/web_push` with:

| Member | Type | Meaning |
|---|---|---|
| `endpoint` | string | The subscription's push endpoint, at most 2,048 characters, on the deployment's push-service allowlist (notifications spec §4.6) |
| `keys` | `{p256dh, auth}` | The subscription's keys as unpadded base64url: `p256dh` an uncompressed P-256 point (65 octets), `auth` 16 octets |
| `step_up` | step-up or `null` | Bound to digest = SHA-256 of `{channel: "web_push", action: "added", endpoint, keys}` (kind `notification_address`) |

`POST /me/channels/web_push/{address_ref}/remove` with `{step_up}`, bound to digest = SHA-256 of
`{channel: "web_push", action: "removed", address_ref}`. A request without evidence, a body an
intermediary dropped included, is refused `step_up_required` and removes nothing.

A change answers `202` with the committed watermark, `phase: "recorded"`, and the `address_ref`. A
PUT of an endpoint the member already holds as an active address, and a removal of an address
already removed, answer `200` with the `address_ref`, the watermark of the event that recorded it,
and no new event, as ending an already-ended delegation does. No response, problem document, log,
metric, or event carries the endpoint, a key, or a hash of either, and the response schema bars
secret-shaped member names (API-11).

**The record.** One control-stream batch, with ids derived from the `Idempotency-Key` (§3.4):

1. `NotificationAddressChanged` `{member, channel: "web_push", action, address_ref}`, with the
   step-up evidence `{assertion_id, authenticated_at, method}` recorded as other step-up-bound
   control events record it (journal spec §9.2, API-17). `address_ref` is listed in the event's
   `pii_refs` (journal spec §6.4), so redaction and erasure reach the vault entry. The step-up
   digest's inputs (the endpoint and keys) are never journaled, and the step-up challenge record
   keeps only the digest.
2. `OwnerAlertSent` `{subject: that event, kind: notification_address_changed}`, its cause
   (notifications spec §3.2, §3.4). Both commit together or not at all.

`address_ref` is the `added` event's own id: the ULID §3.4 derives from the member's random
`Idempotency-Key`, whose time component carries no meaning. It is random in journal spec §3's sense
for `pii_refs`: it is never derived from the address. The `NotificationAddressChanged` journal row is
written by the L3 lane's journal spec PR for §9.11's notification records; the routes stay Planned
until it merges (§9).

**The vault's part.** The vault holds each entry under its `address_ref` with the member, the
endpoint, the keys, the request digest it was written for, and a `version` and `written_at` that
every write of the entry bumps. It answers one question about
endpoints, "which of this member's entries hold this endpoint", from a keyed hash it computes
itself under a per-workspace key that never leaves it; the hash is scoped to the member, deleted
with the entry, and never logged, journaled, or returned. The NT-2 canary scan covers it.

**One member's addresses change one at a time.** Every check below reads the member's active set
as of the control stream's head `h`, and the batch is appended with `expected_head = h` (journal
spec §5.1). A `HeadMismatch` means something was appended meanwhile, perhaps by another member, since the
precondition is on the whole stream: the API reads the active set again, repeats only the set
checks (step 3 when setting, step 2 when removing), and appends again, without writing the vault
again or judging the step-up evidence again (it is judged once and is single use, API-17). After 5
attempts it answers `busy` (503), `effect: none`, `retryable: true`. So two tabs
cannot both add one endpoint, ten addresses cannot become eleven, and one address cannot be removed
twice.

**Setting, in order.**

1. Check the session, CSRF (§3.3), and the request's shape. Derive the event id and look it up
   (§3.4): once the event exists, the retry returns its original outcome, without judging the step-up
   evidence again and without writing the vault (so a retry after a lost `202` succeeds). No body
   is compared then: the journaled members hold no endpoint or keys, so a retry with a different
   body answers the original outcome too. Before the event exists, a vault entry under the derived
   `address_ref` written for a different request digest is `idempotency_conflict`. This is a deliberate exception
   to API-4 and §3.4 (a different body is a 409): once the event exists, the journaled members
   hold no endpoint or keys, so there is nothing to compare the new body against
   ([DEC-795](../project/decisions/DEC-795.md)).
2. Check the allowlist and the step-up evidence, which is judged only by a call that will append.
3. Ask the vault which of the member's entries hold this endpoint, and keep only the active ones
   (§4.11). An active match that is reachable answers `200` and writes nothing. An active match
   marked `unreachable` is replaced: the batch removes it and adds the new `address_ref` (below).
   Otherwise, ten active addresses is `address_limit`.
4. Write the entry under `address_ref`, put-if-absent, bound to the request digest. A failure is
   `effect: none`, `retryable: true`.
5. Append the batch with the expected head.
6. Put the entry again (idempotent: the same `address_ref` and digest), in this first attempt
   only. If that fails, answer `effect: unknown`, `retryable: true`. A retry finds the event at
   step 1 and answers its outcome without writing the vault; if the entry is gone, the dispatcher
   records `address_missing`, and the member repairs it by removing the address and setting it
   again.

**Replacing an unreachable address.** When step 3 finds the endpoint active but `unreachable`, the
batch is `NotificationAddressChanged` `removed` for the old `address_ref`, `NotificationAddressChanged`
`added` for the new one, and one `OwnerAlertSent` naming the `added` event, all or nothing. The
member is told once, on every push channel they hold after the change, the new address included and
the old one not: no last send goes to a replaced address, so its entry's sweep waits on none.

Refusals in steps 1 to 3 are `effect: none` and write nothing. The vault write before the append is
a deliberate exception to API-3's order, for setting only: an entry with no committed `added` event
is inert, because nothing reads an address that is not active (notifications spec §5.1). There is
no inline rollback.

| Stops after | Outcome |
|---|---|
| Steps 1 to 3 | Nothing written; `effect: none` |
| Step 4, before the append | An inert entry. A retry with the same key and body passes step 1, rewrites nothing (put-if-absent), and appends |
| Step 5, the append refused | `effect: none`; the entry stays inert |
| Step 5, the append's outcome unknown | `effect: unknown`; a retry with the same key resolves it |
| Step 6, the put fails | `effect: unknown`, `retryable: true`; the address is active, and if its entry is missing the dispatcher's attempt is `address_missing` until the member removes the address and sets it again |

**The sweep.** For an entry whose `written_at` is more than 24 hours old, workspace services check
the journal; if no `added` event names its `address_ref`, they delete it by compare-and-delete
inside the vault on the `version` they read, which fails if steps 4 or 6 have written it since. A
first attempt that loses that race regardless is repaired by its step 6; if step 6 also fails, the
dispatcher finds the entry missing, records `address_missing`, and tells the member to remove the
address and set it again (notifications spec §5.1, §5.6). Removing needs no vault entry.

**Removing, in order.** Journal first, as API-3 requires:

1. Check the session and CSRF. Derive the event id and look it up (§3.4): an event with the same
   members returns its original outcome, without judging the step-up again.
2. Check that the `address_ref` is the member's own and exists (otherwise 404, the same answer as
   for a random id), and only then that it is active (otherwise `200`), so the `200` for an
   already-removed address is never given for a foreign one.
3. Check the step-up evidence.
4. Append the batch with the expected head. From then on the dispatcher sends nothing to the
   address but the one last `notification_address_changed` notice about this removal
   (notifications spec §5.1).
5. Keep the entry until that last send's attempt is terminal or 24 hours have passed, then delete
   it. If the entry is missing when the last send is made, that attempt is a terminal `failed`
   that marks nothing and raises no `channel_lost`; after the sweep there is no exception left.

| Stops after | Outcome |
|---|---|
| Steps 1 to 3 | Nothing changed; `effect: none` |
| Step 4, the append refused or unknown | `effect: none` or `unknown`; the address stays active until a removal commits |
| Step 4, committed | The removal stands; the sweep deletes the entry after the last send |

**Races.** Every change is an append on the one control stream, ordered by `seq` and guarded by the
expected head: a removal committed after an add removes it, a removal of an unknown `address_ref`
is 404, and a PUT of the same endpoint after its removal adds it again under a new `address_ref`.

---

## 6. Consistency model

### 6.1 Read models come from the journal

- Each read model is a projection the API builds by tailing the streams it needs, woken by
  Postgres `LISTEN`/`NOTIFY` (DEC-17) and correct without it. It is never written by a request.
- Each response carries `as_of: [{stream_id, seq, hash, recorded_at}]` for every stream it read.
  Projections are per stream, so one response may be current on one stream and behind on another;
  the UI shows each value's own age.
- **Freshness limits** come from the specs, not from the API: a mark's age from the data profile,
  broker state from the last `AccountSnapshotRecorded`, the ledger from the executor's head. The API
  marks a value `stale` with its age when it passes the limit (brief §3.1, Stale).
- A read model can always be rebuilt from the journal (API-14). Losing every read-model table loses
  no fact and blocks no command (API-8).

### 6.2 What is authority

Nothing the API reads is authority for trading. The runtime and executor decide from their own
folded state. The API's read models decide only what to show and whether to refuse a call that
would add risk on a fact the owner can see (for example, a deploy whose version was never
confirmed). A refusal on a stale read model is allowed only for a risk-increasing call; a
risk-reducing call never consults one (API-7, API-8).

### 6.3 How the UI knows a command took effect

1. The call returns `recorded` with the control-stream `event_id` and `seq` (or an error with
   `effect`).
2. The change stream announces new watermarks; the UI refetches §5.5's status.
3. The status moves to `taken` when the owning stream's copy appears, then `applied` or `refused`.
4. If nothing moves within the command's expected bound (the runtime's tick for an agent command,
   the executor's for a kill switch), the UI says the command is recorded and not yet taken, which
   is the truth, and keeps Stop live. It never says "done" before `applied` (rule 5 row of brief §5).

---

## 7. Lifecycle and failure walk

| Situation | What happens | Invariant |
|---|---|---|
| **API process down** | No new owner input through the web app or MCP. Agents keep trading inside their mandates; protection rests at the broker. The web app shows "cannot reach your workspace" with no cached content (G1, G4) and, on Stop, how to reach the broker directly. Kill switch alternatives, in order: a second API replica (managed runs at least two); on site, the CLI's direct control-stream append (DEC-436 item 3); the broker's own controls, which reconciliation then ingests as external activity (trading spec §7.1) | API-8 |
| **Postgres down** | Nothing can be journaled, so no command can be recorded, the kill switch included (infrastructure §2, "Known limit"). Every mutating call returns `journal_unavailable` with `effect: none`. Protection resting at the broker is the backstop; the UI points to the broker | API-3, API-13 |
| **Identity provider down** | New sign-ins fail. Pause and the kill switch stay reachable by identity spec §6.4's routes (§3.3 item 3): a session whose refresh failed keeps those two until its absolute lifetime, a workspace-local passkey opens a reduction-only session, and the host CLI works on site. Whether a session past its absolute lifetime may still pause is DEC-436 item 17 (Proposed); until decided it may not | API-1, API-7 |
| **Global control plane down** | Nothing changes in the API; only relay push is lost, and the status strip says so (HLD §4) | API-8 |
| **Runtime down or lagging** | Commands are recorded and wait. The runtime judges an owner exit and a kill switch at the time the owner committed them (mandate spec §6.1), so a late read still applies them. The kill switch's account part is the executor's and does not wait for the runtime (infrastructure §3.6). Status stays `recorded` and says so | API-7, API-13 |
| **Stale read model** | Values are shown with their age. Risk-increasing calls that depend on a stale fact may be refused with the reason; risk-reducing calls never look | API-14, API-7 |
| **Duplicate submit** (double tap, retry after timeout) | Same key, same event; the second call returns the first outcome | API-4 |
| **Network lost after submit** | The client holds the key and the derived `event_id`, and polls §5.5. It shows "the result is unknown; we are checking", never "try again" with a new key | API-4, API-13 |
| **Step-up times out or is cancelled** | Nothing is sent. For an approval, the default still applies at the deadline (G3). Evidence older than 300 s by the time the runtime judges it is refused there as `step_up_stale`; the API does not stretch it | API-17 |
| **Step-up passes, deadline passes before the call lands** | Recorded with the server's `submitted_at`; the runtime refuses it as `late`. The UI shows "not recorded; the default applied" once the status says so | API-12 |
| **Concurrent draft edits** | The second save gets `stale_base` with the current draft; nothing merges silently | API-19 |
| **Two people confirm different versions for one agent** | The second confirm's `base_version` is stale; refused | API-19 |
| **Version change while an approval is pending** | The executor applies the version; the runtime cancels the approval (`version_applied`). A response already sent is refused `not_pending`. D6 shows "canceled by a new version" | API-12 |
| **Version confirmed, rejected at application** | Status `refused` with `MandateVersionApplied`'s reason in words (A6) | API-13 |
| **Control stream frozen** by a verification failure (journal spec §11) | Mandate and deployment changes are refused with `control_stream_frozen`. The API-7 operations are still recorded, as journal spec §11 keeps the kill switch working: otherwise a tampering incident would disable Stop | API-7 |
| **Broker credential suspected compromised** | The owner uses §5.6: kill switch and revocation in one command, never waiting on positions. Deferred equity sells are not re-driven; the owner is told which positions remain at the broker and to revoke the key there | API-7, API-11 |
| **Member removed or client revoked mid-flow** | Their next call fails authentication, so a removed user can no longer answer even while a version still lists them in `autonomy.approval.approvers`. A response they recorded before removal stands and is judged by the runtime like any other | API-1 |
| **Session expires on a record screen** | The confirm fails `unauthenticated` with `effect: none`; after sign-in the screen re-renders from fresh data and needs a fresh confirmation (brief §4.1) | API-13 |
| **API upgrade** | Replicas drain; `/v1` stays served; the control stream's writer epoch passes to the new process (journal spec §5.1) and an append from the old one is `Fenced`, which the API reports as `effect: unknown` and resolves by lookup | API-4 |

---

## 8. Adversaries

| Attacker | Attack | What stops it |
|---|---|---|
| **Stolen browser session** | Approve trades, confirm a looser version, connect a broker | Every risk-increasing call needs step-up bound to its action (API-17); a session alone can pause, hold, skip, or kill-switch, which add no risk (rule 2). A kill switch on a stolen session realizes losses at worst through an ordinary flatten; the step-up-gated owner-exit privilege is not available to it |
| **Stolen assertion** | Reuse one step-up for another approval or a version | Bound to one digest; single-use per workspace (DEC-173 item 3); 300 s life judged by the stream owner |
| **Stolen CLI or client token** | Read, request, propose, dry-run, or hold from another machine, through leaked agent logs or a prompt-injected session | Sender-constrained tokens (§3.3 item 2): without the private key, the token is refused. With the key too (a compromised host), the client's closed scopes still add no risk alone (API-6, MI-30), and revoking the client needs no step-up (identity spec ID-5) |
| **Leaked broker credential** | Trade the owner's account directly, outside the platform | Not stoppable from our side alone. §5.6 revokes at once without waiting on positions, with the kill switch first, and tells the owner to revoke the key at the broker. Keys never pass through the API outward (API-11) |
| **CSRF** | A third-party page posts to the API with the owner's cookie | `SameSite=Strict`, an `Origin` check, and a required custom header (§3.3) |
| **Replay of a captured request** | Re-send a recorded approve or kill switch | Idempotency returns the original outcome (API-4); the assertion is already used; `submitted_at` is the server's |
| **Insider with the viewer role** | Pause an agent, read the journal, export | The route matrix (API-2): a viewer acts on nothing and reads no journal; an auditor reads but acts on nothing. Exports are journaled (API-16) |
| **Insider who is an approver** | Approve their own agent's large orders | Runtime check 7 with `independent_approval_required` (mandate spec §6.4); the API adds no weaker copy |
| **Tenant probing** | Guess ids in other workspaces | Random ids; foreign and absent both 404 (API-9); row-level security under the request's workspace |
| **Forged cross-workspace link** | An event whose `causation_id`, `intent_id`, or `evidence` names another workspace's event, so a trace or timeline pulls it in | Every link is resolved only among the path's workspace's streams; the hop reads `not_recorded`, identical to an absent target (§4.8.1, AU-1) |
| **Auditor or viewer probing the audit routes** | A viewer reads the journal; anyone pages with a huge `limit` or an `after_seq` far past the head | The role is checked before any id is resolved (403 for every id); `limit` above 1,000 is 422, never clamped; an `after_seq` past the head returns an empty page with the head (§4.8.1) |
| **Causation cycle or fan-out bomb** | Events that link in a cycle, or a decision with thousands of outputs, to hang or exhaust a trace | Each event visited once; at most 16 hops and 256 events; `truncated` says so (AU-3) |
| **Spreadsheet formula in a CSV view** | A payload string such as `=HYPERLINK(…)` runs when the export is opened | Leading `=`, `+`, `-`, `@`, tab, and CR are prefixed with `'` (AU-7) |
| **Forged trusted start** | Verify a tampered range against a `prev_hash` the caller chose | The trusted start is read from a manifest or anchor in the workspace, never from the request (§4.8.1) |
| **Malicious owner-connected agent** | Confirm a version, approve its own ask, kill-switch, pause during a fall | Closed scope list (§3.8, API-6): it can request and propose, and hold openings, nothing else. Its openings always ask (MI-30); a hold never holds exits (DEC-191) |
| **Prompt-injected agent calling the API** | Flood asks; request buys in an illiquid name; propose a looser envelope | Every client opening asks a human (MI-30); the ask budget and suppression (mandate spec §6.4) bound the flood; a proposal is only a draft until a user confirms in Owlhead with step-up (DEC-185 item 4); the dry run and requests are rate-limited (§3.10) |
| **Injected text in model output** shown by the API | A thesis or chat reply that reads as an instruction or a button | Model text is typed quoted content (API-18); action cards come from deterministic code |
| **Phishing approval link** | A fake notification that leads to a look-alike page | The real link holds no content and no token (§3.9); approval happens only after sign-in on the deployment's own origin, with a passkey bound to that origin |
| **Notification provider or relay compromised** | Learn trades from payloads | Opaque notice id and generic text only (API-10) |
| **A bad market tick** | The API shows a price that makes the owner confirm a bad exit | The owner exit's floor bounds the price (mandate spec §6.1); quotes are shown with their age; a quote that moves needs a fresh render and confirmation (brief §4.1) |
| **Careless user** | Double-taps confirm or kill switch; edits a draft in two tabs | Idempotency (API-4); `stale_base` (API-19) |

---

## 9. What exists and what is planned

| Part | State |
|---|---|
| Owner input as control-stream events (`OwnerCommandIssued`, `ApprovalResponseSubmitted`, `OwnerAcknowledged`), judged by the runtime and executor | **Exists** (M7, DEC-155, DEC-173, DEC-290); written by the CLI |
| The CLI's owner commands, approvals list, show, approve, skip, and status, as library code (`crates/mandate-cli`: `agent.rs`, `approvals.rs`, `control.rs`) | **Exists**; becomes an API client for managed deployments |
| Derived control-stream event ids (DEC-290) | **Exists** in the CLI; §3.4 extends it |
| Journal verification and export (`mandate-journal`, `mandate-journal-cold`, the CLI's `journal verify`) | **Exists**; §4.8 serves it |
| Approval content, hash, and admission (`mandate-approval`) | **Exists** (pure core) |
| Mandate validation and classification (`mandate-spec`) | **Exists**; §4.1 calls it |
| The web app's screens on recorded fixtures (`web/`) | **Exists**; §4.9 maps fixtures to resources |
| The API process, routes, authentication middleware, idempotency, errors, change stream | **Planned** (E10-10) |
| Drafts, compile, versions, confirm endpoints | **Planned** (E10-11) |
| Deployment manager endpoints and the reserved kill-switch path | **Planned** (E10-12) |
| Approval service endpoints and delegation previews | **Planned** (E8-15) |
| Connection endpoints | **Planned** (E10-13, with gap 9) |
| Client tokens and scopes | **Planned** (E10-14, before E10-6) |
| Read-model projections | **Planned** (E11-9) |
| Journal queries, trace, exports over the API | **Planned** (E12-6); contracts in §4.8.1, served by the pure crate `mandate-audit` |
| Journal events this spec needs: `MandateDraftSaved`, the compiler's invocation on the control stream, `MandateConfirmed`'s agent link, and `OwnerRequestSubmitted` | **Specified** (journal spec v0.21 §9.9, DEC-670); `mandate-journal` registration planned (E10-15) |
| The `client` actor, `ConnectionRevoked`'s reason, and the client events | **Specified** (journal spec v0.22 §3 and §9.10, DEC-671); `mandate-journal` registration planned (E10-15) |
| `hold_openings` and `lift_hold` | **Specified** (journal spec v0.23 §9.11, DEC-672); `mandate-journal` registration planned (E10-15) |
| Sessions, roles, step-up ceremonies | **Planned** (E9, the identity spec) |
| Notification addresses (§4.11, §5.7) and `NotificationAddressChanged` | **Planned** (E8-14). Mounted once the API's authentication middleware lands and the journal spec change that writes the `NotificationAddressChanged` row (the L3 lane's §9.11 journal PR) merges. Merge order: #811, #763, this change (#827), then #833. The push-service allowlist's decision is DEC-792 (this spec change, #827); the web client's mirror of its table is #833, which merges after #827. Accepted when: a canary scan of responses, problems, logs, metrics and the journal finds no endpoint, key, or endpoint hash; the append is failed at every step of §5.7's tables (API-3's test) and crash injection gives each stated outcome; a replay after a lost `202` returns it; a retry with a different body, once its event exists, answers the original outcome and writes nothing to the vault; before the event exists, with the entry present, it is `idempotency_conflict`; a retry racing the sweep ends with the entry present or `address_missing` recorded; inert and removed entries neither match an endpoint nor count to the limit; two concurrent PUTs of one endpoint, an eleventh address, and a double removal are each settled by the expected head; a PUT of an unreachable address's endpoint replaces it in one batch with no last send to the old address; a stream busy for 5 attempts answers `busy` without a second vault write or step-up judgment; deactivation removes every address in its own commit, under the acting principal; a foreign `address_ref` answers byte-for-byte as a random one; the shared allowlist table (DEC-792) passes; and a replayed or wrongly bound step-up is refused |
| The relay's own allowlist check (DEC-792 item 3) | **Planned** (E20-8) |

---

## 10. Decisions

§4.8.1's readings are [DEC-760](../project/decisions/DEC-760.md) to
[DEC-767](../project/decisions/DEC-767.md), each Accepted: each closes an unclosed contract by
the reading that adds no risk (DEC-176 item 2). The rest are recorded in [DEC-436](../project/decisions/DEC-436.md). Items 1 to 16 and 19 to 21 are reversible
engineering readings an agent accepts (DEC-79, DEC-176): each adds no trading rule, or only tightens
one. Items 9, 19, 20, and 21 carry the coordinator's round-1 settlements X1, X2, X3, and X5 and its
ruling on M2 and M3.
Items 17 and 18 stay **Proposed** for the founder:

- **Item 17:** whether a session past its absolute lifetime, while the identity provider is
  unreachable, may still pause (identity spec §6.4 already keeps pause and the kill switch on a
  session whose refresh failed, until that lifetime). Recommended: yes, for pause only, up to 12 hours after its last successful authentication,
  never after revocation, journaled with the method `session_grace`. Until decided: no grace;
  pause needs a valid session, and the kill switch a locally verified passkey.
- **Item 18:** whether the API is offered to third parties (DEC-149, E18). Recommended: first-party
  only in v1 (the web app, the CLI, the Owlhead MCP server). Until decided: first-party only.

[DEC-690](../project/decisions/DEC-690.md) item 1 (agent-accepted under DEC-79) settles where
connect reaches the broker: never in the API process. The code exchange runs in the token-exchange
process, which is not the executor, and the permission checks run in the connection's executor
(§1.4; [DEC-821](../project/decisions/DEC-821.md) item 2). DEC-690 items 6 and 7 are accepted by
the founder for paper only (DEC-821): Alpaca OAuth connects paper accounts only, and a live Alpaca
OAuth connection needs a new decision.

---

## 11. Backlog

Stories continue the existing epics (DEC-436 item 15): E8-15, E10-10 to E10-15, E11-9, and E12-6 in
the [backlog](../project/06-backlog-v1.md). The round-1 review's minors are rows under "Spec
follow-ups" there (freeze rule). **SC** marks a safety-critical story.

---

## 12. Open questions

1. **The record of an independent version approval.** Mandate spec §5.7 and A6 need a second user's
   approval for some versions (loosening the floor; where policy requires it). `MandateConfirmed`
   names one user. Which event records the second? Belongs to the mandate and journal specs.
2. **Organization-scope roles.** Who may issue the organization kill switch across workspaces in
   different cells, and how the client learns the workspace list without the global control plane
   holding strategy. Identity spec and gap 8.
3. **Phone access in hybrid** (HLD §12 item 7): whether the API is reached over VPN or through the
   relay with end-to-end encryption changes §3.3 and §3.9.
4. **Rate-limit values** per principal and per client, and the kill-switch path's latency bound.
5. **Chat retention and personal data.** Chat messages are free text; journal spec §6.4's redaction
   applies, and the scanning method is journal spec §13 question 3.
6. **Server-sent events through corporate proxies** in hybrid: polling is the fallback; whether it
   is enough at the change rates of a busy workspace.
7. **Trace links the journal does not record yet.** `ModelInvocationRecorded` names no event it
   served, so a trace reaches it only when an output's `evidence` names it. A flatten's or a
   protective order's `OrderRequestRecorded` carries no link to the `KillSwitchActivated` or the
   opening it serves. A version-1 `OrderSubmitted` names no intent. `OrderAbandoned` has no closed
   schema, so no payload member of it is a link. No member the journal spec requires links an
   `ApprovalRequested` to its `DecisionMade`: the runtime writes the decision as its `causation_id`,
   which the spec does not require. Each needs a journal spec
   member; until then the trace reports the hop as `not_recorded` or ends (§4.8.1).
