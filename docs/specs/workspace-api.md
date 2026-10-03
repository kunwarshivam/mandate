# Workspace Services API Spec (v0.1, draft)

| | |
|---|---|
| **Status** | Draft v0.1, not yet reviewed ([DEC-436](../project/decisions/DEC-436.md)). Items 1 to 16 of DEC-436 are agent readings; items 17 and 18 are Proposed and wait for the founder |
| **Implements** | [HLD §4](../HLD.md#workspace-deployment) (workspace control services), [§6 flows A and C](../HLD.md#6-key-flows), [§7](../HLD.md#7-logging-and-audit), [§8](../HLD.md#8-multi-tenancy-and-security); PRD FR-1.4, FR-2.1 to FR-2.4, FR-3.1 to FR-3.5, FR-4.4, FR-6.2 to FR-6.5, FR-7.1 to FR-7.5, FR-8.1 to FR-8.4; backlog E8, E10, E11, E12 |
| **Depends on** | [Mandate spec](mandate.md) §2, §6, §7, §9, §10; [journal spec](journal.md) §2, §5, §7, §9, §11, §12; [infrastructure design](../design/infrastructure.md) §3.6, §9; [product experience brief](../product/09-product-experience.md) §3 to §5 |
| **Siblings** | Identity, roles, sessions, and step-up ceremonies: the [identity spec](https://github.com/kunwarshivam/mandate/pull/556) (`docs/specs/identity.md`, #556), gap 6. Notification delivery and approval deep links: the [notifications spec](https://github.com/kunwarshivam/mandate/pull/558) (`docs/specs/notifications.md`, #558), gap 7. Broker connection flows: `docs/specs/connections.md` (gap 9) |

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
| **CLI** (`mandate-cli`) | A user | A device-bound token from the identity spec | As the web app. In Phase 1, and for on-site operators, the CLI may still append to the control stream directly (DEC-436 item 3) |
| **Owner-connected agent** over MCP (DEC-141, DEC-183) | A **client** acting for one user | Its own scoped, revocable token; never a broker credential (DEC-141 item 4) | A closed list (§3.8): reads, owner requests, draft proposals, the dry run (DEC-190), and holding new openings (DEC-191). Never confirms, approves, picks a delegation, or uses an owner-only control |
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
  stream's writer epoch (DEC-436 item 3), session keys, and no broker credential. It reads the
  journal and its read-model tables; it writes only the control stream, the artifact store, and its
  own read-model and draft tables.

### 1.4 Non-goals

- **No trading authority.** The API never places, cancels, or sizes an order, never writes an agent
  or account stream, and never calls a broker or a runtime directly (`AGENTS.md` rule 12). Its
  checks are a convenience; the stream owners make every check again.
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
| **API-1** | **Authenticated.** Every call except the liveness probe carries a valid session or token. Without one the API returns 401, reveals no data, and says nothing about whether the resource exists | A route table test: every registered route, called without credentials, returns 401 with the same body |
| **API-2** | **Authorized on the server.** Each operation checks the principal's role in the workspace (§3.7) and, for a client, its scope (§3.8), on the server, before any read or write. The web app's role checks (`web/src/lib/roles.tsx`) are never authority | A matrix test: every route × every role and client scope, with the expected 403 or success from §3.7, computed from the table, not from the handler |
| **API-3** | **Journal before effect.** Every call that can change what an agent, connection, policy, member, client, or approval may do commits its control-stream event (`Committed` or `AlreadyCommitted`, journal spec §5.1), with the caller's identity in `actor`, before it reports success and before anything acts on it. The API writes no agent or account stream | Fault injection: fail the append at every step; no stream owner ever sees an effect without its control-stream cause, and no response says `recorded` without a committed event |
| **API-4** | **Idempotent.** Every mutating call carries an `Idempotency-Key`. The same principal, operation, and key always resolve to the same control-stream event. A repeat with the same body returns the first outcome; a different body returns 409 `idempotency_conflict`. Nothing is committed twice | Fuzz: random retries, duplicate submits, lost responses, and concurrent repeats; an independent counter of control-stream events per key never exceeds one |
| **API-5** | **The envelope changes only by a confirmed version.** No call changes an envelope field of a deployed agent except confirming a mandate version, by a **user** with the role for it. The server computes the classification itself (mandate spec §9.2) and requires step-up when it is risk-increasing; a client's claimed classification is never used (rule 11) | Fuzz random envelope edits through every operation; an oracle that diffs the confirmed documents shows every change came through a `MandateConfirmed` by a user, with step-up whenever its own §9.2 verdict is increasing |
| **API-6** | **A client is owner input, never the owner.** A client token reaches only the operations of §3.8. `requested_by` is set from the authenticated channel, never from the request body (mandate spec §6.2 step 5a). A client can never confirm a version, answer an approval, create, widen, or pick a delegation, connect or revoke a connection, change a member, policy, or client, export, or use pause, resume, Stop, release, owner exit, acknowledgment, or the kill switch (DEC-141, DEC-185, DEC-191) | The route matrix for the client principal; a test that a client request carrying `requested_by: owner` in its body is journaled as `client` |
| **API-7** | **Risk reduction is never blocked by the API.** Pause, holding new openings, the kill switch at any scope, an owner exit, Skip on an approval, ending a delegation, and away mode (the **API-7 operations**) are refused only for a failed authentication, a role that may not act, or a malformed request. Never for a rate limit, a quota, a stale or missing read model, missing step-up (except where mandate spec §6.1 requires it, below), a runtime, model, market-data, or global-control-plane outage, a pending approval, or a frozen control stream (journal spec §11). A kill switch or owner exit without valid step-up is still recorded and still stops or routes (mandate spec §6.1, DEC-158 option (c)) | A test per operation with every one of those conditions injected; each still commits its event. Resume, Stop, and acknowledgment are not risk reduction and may be refused without step-up, as mandate spec §6.1 says |
| **API-8** | **The kill-switch path needs only the API, its authentication, and Postgres.** Pause and the kill switch read no read model, call no model, runtime, market-data service, global control plane, or telemetry, and run on a reserved worker and database-connection pool that other traffic cannot exhaust (infrastructure §3.6) | A test with the model gateway, read-model tables, runtime, and metrics exporter all unavailable and every ordinary worker busy: the kill switch commits within its bound |
| **API-9** | **Tenants never see each other.** Every resource lives under one workspace. A principal reaches only workspaces it belongs to. An id from another workspace, or one that does not exist, returns the same 404. No response, error, log line, metric label, or notification carries another workspace's data | Cross-workspace tests at the route, database (row-level security), and artifact layers (OPS-6); a test that the 404 bodies and timings for "foreign" and "absent" match |
| **API-10** | **Nothing sensitive leaves through the API's side channels.** What the API hands to the relay or a notification provider is an opaque notice id and generic text only; approval links carry only that id; page titles, URLs, and error titles hold no instrument, size, price, thesis, or agent name (rule 6) | Payload capture tests on every notification the API emits; a URL lint over the route table |
| **API-11** | **Credentials never cross the API outward.** No response, log, journal event, or error carries a broker credential, OAuth token, or key. A key field is write-only and goes straight to the vault in the same request; a rejected key is explained without echoing it (rule 7) | Log and response scans with canary secrets; a test that every connection response matches a schema with no secret-shaped member |
| **API-12** | **An answer binds what was shown.** An approval response carries the content hash the client rendered, and the API forwards it unchanged. The API never computes a hash on the client's behalf, never answers for a principal, and never reports a grant as acting: only the runtime's `ApprovalResponded` and `ApprovalRevalidated` say what happened (mandate spec §6.4) | A test that an approve with a hash the request does not carry is journaled as sent and refused by the runtime as `content_mismatch`; a test that no response field reads "approved" before `ApprovalRevalidated` with `act` |
| **API-13** | **No optimistic success.** A mutating call reports `recorded` only after the append returned `Committed` or `AlreadyCommitted`. A lost or `Ambiguous` append reports `unknown`, never success and never "nothing happened". `applied` is reported only when the owning stream's copy exists (§6.3) | Fault injection on the append's response path; the web app's `result-unknown` scenario becomes a contract test |
| **API-14** | **Read models are derived and stamped.** Every read names, for each stream it read, the `seq` and hash it reflects, and the `recorded_at` of that event. No value comes from anywhere but the journal and stored artifacts. A value older than its freshness limit is returned marked stale with its age, never as current | Replay test: rebuilding every read model from the journal alone gives identical responses; a staleness test per freshness limit |
| **API-15** | **Journal pages are complete and checkable.** Paging a stream by cursor yields every event in the range exactly once, in `seq` order, with each event's `hash` and `prev_hash`, so a client can check that each page joins the last | Fuzz: random page sizes and concurrent appends; concatenated pages equal the stream range, and the chain check passes |
| **API-16** | **Exports are verifiable and recorded.** Every export is journaled (`ExportCreated`) before it is served. A canonical export verifies under journal spec §11 against its anchors; every derived JSON or CSV view names the manifest hash it came from | A test that runs the journal verifier over each export and fails a tampered one |
| **API-17** | **Step-up is bound to one action.** A step-up challenge the API issues names one action digest (§3.6). Evidence is accepted only on the call for that digest, and an assertion id is used once per workspace (DEC-173 item 3) | Tests that evidence for one approval, version, or command is refused on another; replay of a used assertion is refused |
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
  rendered it (mandate spec §10's UI build covers the client half).

### 3.3 Authentication and sessions

Owned by the [identity spec](https://github.com/kunwarshivam/mandate/pull/556) (`docs/specs/identity.md`, #556). What the API requires of it:

1. **Browser sessions** in a `Secure`, `HttpOnly`, `SameSite=Strict` cookie, with a CSRF defence on
   every mutating call: the `Origin` header must be the app's own origin, and the call must carry a
   custom request header a cross-site form cannot set. No bearer token is ever stored in browser
   storage (brief §5, rule 6 row).
2. **CLI and client tokens** are bearer tokens bound to one principal and one workspace, revocable
   at once, and stored by the server only as a hash.
3. **Authentication works without the global control plane** (HLD §8), and the kill-switch path
   must authenticate without a network call to the identity provider: a locally verified passkey or
   a session the deployment can verify itself (§7, DEC-436 item 17).
4. Every journaled event names the principal in `actor`: `user` with the user's opaque id, or, for
   a client, a new actor kind `client` with the client's opaque id and the user it acts for in the
   payload (journal change, DEC-436 item 9). A client is never recorded as a `user`, so even a bug
   that let a client answer an approval would be refused by the runtime's check 3. Events also
   record the channel (`web`, `cli`, `mcp`) and the authentication method.

### 3.4 Idempotency

Every mutating call carries `Idempotency-Key`: 16 to 64 characters from `[A-Za-z0-9_-]`, chosen by
the client per user gesture. The API derives the control-stream `event_id` from it (DEC-436 item 4):
the 128 bits of the ULID are the first 128 bits of SHA-256 over the workspace id, the principal id,
the operation name, and the key. The ULID's time component carries no meaning (journal spec §3), so
this is a valid id. Then:

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
| `forbidden` | 403 | The role or scope may not do this (never sent for a resource the principal cannot see: that is 404) |
| `not_found` | 404 | Absent, or in another workspace (API-9) |
| `invalid` | 422 | Schema, canonical form, or a value out of range |
| `idempotency_conflict` | 409 | Same key, different body |
| `stale_base` | 409 | The draft or version base moved (API-19); the body names the current base |
| `classification_changed` | 409 | The server's classification differs from the one the confirmation screen showed |
| `step_up_required` | 401 | A risk-increasing call without evidence bound to its digest; never sent for pause, kill switch, owner exit, or Skip |
| `live_unavailable` | 409 | A live environment operation before counsel signs off (rule 8, B5) |
| `control_stream_frozen` | 503 | Journal spec §11 froze mandate and deployment changes; never sent for a risk-reducing call (API-7) |
| `journal_unavailable` | 503 | Postgres cannot take the append; `effect: none`, `retryable: true` |
| `rate_limited` | 429 | Over the principal's limit; never for an API-7 operation |

### 3.6 Step-up

The ceremony belongs to the [identity spec](https://github.com/kunwarshivam/mandate/pull/556) (`docs/specs/identity.md`, #556) (E9-4). The API's part:

- `POST /v1/workspaces/{ws}/step-up/challenges` with `{action: {kind, digest}}` returns a challenge
  for that one action. `kind` is `approve`, `confirm_version`, `deploy`, `command`, `acknowledge`,
  `connect`, `disclosure`, `client_create`, or `policy`. `digest` is the SHA-256 of the action's
  canonical object: the approval's content hash, the mandate version hash, or the command's
  canonical members (DEC-279 item 1's object for commands).
- The evidence the call then carries is `{assertion_id, authenticated_at, method}`, the shape the
  journal records (journal spec §9.2). The API checks it is bound to the call's digest and unused,
  and records it. **Validity is judged by the stream owner** at the moment mandate spec §6.1 names;
  the API's check is a convenience, except that it never forwards evidence bound to another action.
- Until E9-4 ships, the only method is `cli_confirm`, paper only (mandate spec §6.1). The API
  refuses every `live` step-up with `live_unavailable`.

### 3.7 Roles

Roles and separation of duties belong to the [identity spec](https://github.com/kunwarshivam/mandate/pull/556) (`docs/specs/identity.md`, #556). This is the API's reading of HLD §8, PRD
FR-1.3, PX-11 (b), and journal spec §7, for its route matrix; the identity spec wins where they
differ. "Owner" in the specs means a workspace admin or operator acting on the agent.

| Operation group | Workspace admin | Operator | Approver | Viewer | Auditor | Client (§3.8) |
|---|---|---|---|---|---|---|
| Read agents, positions, approvals, plan, brief | Yes | Yes | Yes | Yes | No | Its agents only |
| Read journal, trace, timeline, gate decisions | Yes | Yes | Yes | No | Yes | No |
| Exports, verification runs | Yes | No | No | No | Yes | No |
| Drafts, compile, validate, create version | Yes | Yes | No | No | No | Propose only |
| Confirm a version, deploy | Yes | Yes | No | No | No | No |
| Pause | Yes | Yes | Yes | No | No | No |
| Hold new openings (`exits_only`, DEC-191) | Yes | Yes | Yes | No | No | Yes |
| Resume, Stop, release, owner exit, kill switch, acknowledge | Yes | Yes | No | No | No | No |
| Approve or Skip | If listed in `autonomy.approval.approvers` and holding any of the first three roles | Same | Same | No | No | No |
| End a delegation, away mode | Yes | Yes | No | No | No | No |
| Connections: connect, revoke | Yes | No | No | No | No | No |
| Policies, members, clients | Yes | No | No | No | No | No |

An organization owner or admin acts in each workspace as its workspace admin. Separation of duties
(FR-1.6) is enforced where the specs already enforce it: by the runtime at approval (check 7) and
by the executor at acknowledgment (mandate spec §5.8). The API adds no second copy that could
disagree.

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

Deep-link delivery belongs to the [notifications spec](https://github.com/kunwarshivam/mandate/pull/558) (`docs/specs/notifications.md`, #558); this section states what the API serves. A notification carries a random **notice id**, generic text, and nothing else (rule 6, mandate spec
§6.4). The link is `/n/{notice_id}`. Opening it shows sign-in only (brief G4); after sign-in the API
resolves the notice to its approval for that user, if the user may see it, and otherwise returns
404. The notice id is not the approval's event id, so a link reveals no creation time and no event
reference. A link grants no authority and holds no token.

### 3.10 Rate limits and quotas

Per principal and per workspace (HLD §8 quotas), stricter for clients. No limit, quota, or load
shedding applies to the API-7 operations, which run on the reserved pool of API-8.

---

## 4. Resources and operations

All paths are under `/v1/workspaces/{workspace_id}`. "Event" names the control-stream event the call
commits (journal spec §9). **Journal change** marks an event or member the journal spec does not
define yet; §11's E10-15 adds them before the operation ships.

### 4.1 Mandate drafts and versions (agent registry and compiler)

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Create a draft | `POST /mandate-drafts` | `MandateDraftSaved` (journal change) | From a description, the three goal answers (A0, E10-7), a template, or a base version. Returns `draft_id` and its `etag` |
| Read, list drafts | `GET /mandate-drafts/{id}`, `GET /mandate-drafts` | — | With provenance per path (mandate spec §2.1) |
| Save a draft | `PUT /mandate-drafts/{id}` with `If-Match` | `MandateDraftSaved` | Form and YAML are two views of this one document (FR-3.2). Stale `If-Match`: `stale_base` |
| Compile | `POST /mandate-drafts/{id}/compile` | The compiler's `ModelInvocationRecorded` on the control stream (journal change) | Asynchronous job. Fills only `user_stated` and `platform_proposed` values, never `auto` or a delegation (V-022, V-038). Output failing the schema is `compile_failed` with the description kept (A2) |
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
| Hold new openings | `POST /agents/{id}/hold` | `OwnerCommandIssued` with command `hold_openings` (journal change) | DEC-191. Sets `exits_only` and nothing else. Lifting it is the owner's alone, with step-up, as a `lift_hold` command (journal change); it lifts only the hold, never a latched limit (MI-3) |
| Stop | `POST /agents/{id}/stop` | `OwnerCommandIssued` | Step-up; `release` and the warning digest per DEC-290 item 2; the flat-or-release precondition is the runtime's (DEC-136) |
| Owner exit | `POST /agents/{id}/exits` | `OwnerCommandIssued` (`owner_exit`) | §5.4. Never refused for step-up (mandate spec §6.1) |
| Acknowledge | `POST /agents/{id}/acknowledgments` | `OwnerAcknowledged` | Names the event acknowledged; step-up; independence judged by the executor |
| Kill switch | `POST /kill-switch` | `OwnerCommandIssued` (`kill_switch`) | §5.4. Agent, connection, or workspace scope |
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
| Connect | `POST /connections/oauth/start`, then the broker redirects to `GET /connections/oauth/callback` | `ConnectionEstablished` | Step-up before start. PKCE and a single-use `state`; the token exchange goes straight to the vault; scopes beyond trading reject the connection (FR-2.2). Flow details are the connections spec's |
| Revoke | `POST /connections/{id}/revoke` | `ConnectionRevoked` | Step-up. Refused while any agent on it holds positions or is not stopped: without the connection nothing can exit or re-protect, so revoking is not risk reduction. The kill switch comes first |
| Policies | `GET`, `PUT /policies/workspace` | `PolicyChanged` | A value looser than its parent is refused naming the nearest ancestor (FR-1.5); the response lists agents made nonconforming (X1). Step-up |
| Members | `GET /members`, `POST /invitations`, `PATCH`, `DELETE /members/{id}` | Identity spec's events (journal change) | Removing a member ends their sessions and tokens at once |
| Clients | `GET /clients`, `POST /clients`, `DELETE /clients/{id}` | `ClientConnected`, `ClientRevoked` (journal change) | Create needs step-up and shows the scopes in words (E10-8); revoke needs none |

### 4.6 Owner requests, the dry run, and the chat thread

| Operation | Method and path | Event | Notes |
|---|---|---|---|
| Dry run | `POST /agents/{id}/dry-run` | `RecordsAccessed` | DEC-190: the decision (`auto`, `ask`, `deny`) and the gate's reason code for a described order, with the client ceiling for a client. Places nothing, creates no approval |
| Owner request | `POST /agents/{id}/requests` | `OwnerRequestSubmitted` (journal change) | An instrument, side, and optional size the owner or client asks for. The runtime copies it to its builder, which sizes, clips, and classifies it as for any proposal (rule 4); `requested_by` from the channel (API-6) |
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
| One event | `GET /journal/events/{event_id}` | With its artifacts' refs |
| Timeline | `GET /agents/{id}/timeline?types=&from=&to=` | J1: merged from the agent and account streams, with one cursor per stream; display order by `recorded_at` for readability only (journal spec §2) |
| Causal trace | `GET /journal/events/{event_id}/trace` | J2: the `causation_id` chain back to observations; model output as quoted, attributed content |
| Gate decision | `GET /journal/events/{event_id}/gate` | J6: every check with reason code, rule-set version, quotes and marks used |
| Exports | `POST /exports`, `GET /exports/{id}` | J3, journal spec §12: canonical export, or JSON and CSV views naming the manifest hash. `ExportCreated` before serving (API-16) |
| Verification | `POST /verifications`, `GET /verifications/{id}` | J4: runs journal spec §11; `VerificationRun` |
| Surveillance | `GET /surveillance/reports` | J5 |

Every read outside the product views (exports, examination bundles, break-glass) is journaled as
`RecordsAccessed` or `ExportCreated` (journal spec §7).

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

---

## 5. Safety-relevant request and response shapes

Members are listed in full. Every request also carries `Idempotency-Key`. `step_up` is always
`{assertion_id, authenticated_at, method}` or `null`, the shape journal spec §9.2 records. `record`
is always `{artifact: "sha256:…", ui_build: "sha256:…"}`: the rendered record screen the client
uploaded to the artifact store, and the build that rendered it (brief §4.1, mandate spec §10).

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
`confirmed_paths`; the screen digest matches; for `risk_increasing`, step-up is present and bound,
else `step_up_required`. While the control stream is frozen (journal spec §11), every confirm is
refused with `control_stream_frozen`; the API-7 operations, including §4.4's reducing shortcuts,
are still recorded.

It then commits `MandateConfirmed` with the agent link (journal change: `agent_id` and
`base_version`, DEC-436 item 14). Response `202`:

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
| `delegation` | `null` or `{preview_id, mandate_version}` | The shape chosen on the card (§5.3) |

The API refuses only: an unauthenticated or non-user principal, a user who is not listed in
`autonomy.approval.approvers` and holding an approving role (§3.7), an unknown approval, and, for
`approved` only, missing or unbound step-up. It does not judge the deadline, the quorum,
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
| `scope` | `{kind: "agent" \| "connection" \| "workspace", id}` | `id` is the agent or connection id, or `null` for the workspace. The organization scope is the client issuing one workspace-scope call per workspace, each journaled on its own (DEC-436 item 13) |
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
| **Identity provider down** | Existing sessions keep working until they expire. New sign-ins fail. The kill switch and pause need authentication the deployment can verify without the provider (§3.3 item 3): a locally verified passkey. Whether an expired session may still pause is DEC-436 item 17 (Proposed); until decided it may not | API-1, API-7 |
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
| **Member removed or client revoked mid-flow** | Their next call fails authentication, so a removed user can no longer answer even while a version still lists them in `autonomy.approval.approvers`. A response they recorded before removal stands and is judged by the runtime like any other | API-1 |
| **Session expires on a record screen** | The confirm fails `unauthenticated` with `effect: none`; after sign-in the screen re-renders from fresh data and needs a fresh confirmation (brief §4.1) | API-13 |
| **API upgrade** | Replicas drain; `/v1` stays served; the control stream's writer epoch passes to the new process (journal spec §5.1) and an append from the old one is `Fenced`, which the API reports as `effect: unknown` and resolves by lookup | API-4 |

---

## 8. Adversaries

| Attacker | Attack | What stops it |
|---|---|---|
| **Stolen browser session** | Approve trades, confirm a looser version, connect a broker | Every risk-increasing call needs step-up bound to its action (API-17); a session alone can pause, hold, skip, or kill-switch, which add no risk (rule 2). A kill switch on a stolen session realizes losses at worst through an ordinary flatten; the step-up-gated owner-exit privilege is not available to it |
| **Stolen assertion** | Reuse one step-up for another approval or a version | Bound to one digest; single-use per workspace (DEC-173 item 3); 300 s life judged by the stream owner |
| **CSRF** | A third-party page posts to the API with the owner's cookie | `SameSite=Strict`, an `Origin` check, and a required custom header (§3.3) |
| **Replay of a captured request** | Re-send a recorded approve or kill switch | Idempotency returns the original outcome (API-4); the assertion is already used; `submitted_at` is the server's |
| **Insider with the viewer role** | Pause an agent, read the journal, export | The route matrix (API-2): a viewer acts on nothing and reads no journal; an auditor reads but acts on nothing. Exports are journaled (API-16) |
| **Insider who is an approver** | Approve their own agent's large orders | Runtime check 7 with `independent_approval_required` (mandate spec §6.4); the API adds no weaker copy |
| **Tenant probing** | Guess ids in other workspaces | Random ids; foreign and absent both 404 (API-9); row-level security under the request's workspace |
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
| Journal queries, trace, exports over the API | **Planned** (E12-6) |
| Journal events this spec needs (`MandateDraftSaved`, the compiler's invocation on the control stream, `MandateConfirmed`'s agent link, `OwnerRequestSubmitted`, `hold_openings`, client events) | **Planned** (E10-15, journal spec change first) |
| Sessions, roles, step-up ceremonies | **Planned** (E9, the identity spec) |

---

## 10. Decisions

Recorded in [DEC-436](../project/decisions/DEC-436.md). Items 1 to 16 are reversible engineering
readings an agent accepts (DEC-79, DEC-176): each adds no trading rule, or only tightens one.
Items 17 and 18 stay **Proposed** for the founder:

- **Item 17:** whether a session that expired while the identity provider is unreachable may still
  pause. Recommended: yes, for pause only, up to 12 hours after its last successful authentication,
  never after revocation, journaled with the method `session_grace`. Until decided: no grace;
  pause needs a valid session, and the kill switch a locally verified passkey.
- **Item 18:** whether the API is offered to third parties (DEC-149, E18). Recommended: first-party
  only in v1 (the web app, the CLI, the Owlhead MCP server). Until decided: first-party only.

---

## 11. Backlog

Stories continue the existing epics (DEC-436 item 15): E8-15, E10-10 to E10-15, E11-9, and E12-6 in
the [backlog](../project/06-backlog-v1.md). **SC** marks a safety-critical story.

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
