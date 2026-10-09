# Broker Connections Spec (v0.2, draft)

| | |
|---|---|
| **Status** | Draft v0.2, not yet reviewed (v0.2: §4, §6.2, §6.6 and §12 from Robinhood's published contract, [DEC-529](../project/decisions/DEC-529.md) and [DEC-531](../project/decisions/DEC-531.md)) |
| **Owner** | Engineering |
| **Decisions** | [DEC-441](../project/decisions/DEC-441.md) (items 1 to 14, 21, and 23 Accepted by the agent; items 15 to 20 and 22 Proposed for the founder; for paper Alpaca OAuth, DEC-821 supersedes item 21 for `env=paper` grants, resolves item 18, and adopts item 22); [DEC-529](../project/decisions/DEC-529.md) (founder, Accepted 2026-10-08: the founder's one live Robinhood order); [DEC-531](../project/decisions/DEC-531.md) (capability profiles); [DEC-690](../project/decisions/DEC-690.md) (Alpaca's documentation read for U-A1 to U-A5; items 1 to 5 and 8 to 10 Accepted by the agent; items 6 and 7 Accepted by the founder, paper only, see DEC-821); [DEC-821](../project/decisions/DEC-821.md) (founder, Accepted 2026-10-08: Alpaca paper OAuth, the token-exchange process, and the narrowings of CN-3, ES-23 and DEC-441 item 21 for paper Alpaca OAuth only) |
| **Backlog** | E7-1, E7-6, E7-11 to E7-18 ([backlog](../project/06-backlog-v1.md#e7-alpaca-connector-and-recovery)); E16-1 for Kraken |
| **Safety-critical** | Yes: broker connectors, OAuth scopes, key-permission checks, and credential handling (`AGENTS.md`, "Safety-critical paths") |

This spec covers how a workspace connects a broker or venue account, what the platform stores about
it, which permissions it may hold, how it checks them, how it watches a connection's health, and
what happens at every point in a connection's life. It closes design gap 9
([register](../project/10-design-gaps.md)).

It adds **no risk-gate rule of its own**. Where it touches orders, accounts, or records, the
[trading domain spec](trading-domain.md), the [mandate spec](mandate.md), and the
[journal spec](journal.md) win (DEC-441 item 1). It says what a connection must guarantee so that
their rules hold. The API routes that start a connection are the workspace API spec's
(`docs/specs/workspace-api.md`, open PR #560, §4.5); who may connect is the identity spec's
(`docs/specs/identity.md`, open PR #556); where the credential lives is the
[infrastructure design](../design/infrastructure.md) §5.

## Contents

0. Change history
1. Scope
2. Invariants
3. The connection object
4. Connector capabilities
5. Alpaca
6. Robinhood Agentic Trading over MCP
7. Kraken Derivatives US (later)
8. Permission checks and health checks
9. Lifecycle walk
10. Adversaries
11. What exists and what is planned
12. Decisions
13. Open questions

---

## 0. Change history

- **v0.2, amended by [DEC-699](../project/decisions/DEC-699.md)** ([DEC-694](../project/decisions/DEC-694.md)
  item 4): §5.2 step 1's pending record is journaled as `ConnectionRequested` (journal spec §9.8,
  rule 131), the event the connect's step-up commits with (identity spec §7.2 step 5); step 6's
  API restart re-reads the open requests; and §9.1 no longer says a refused connect keeps no
  record beyond the refusal, since its request is kept too. Each only tightens (DEC-176).
- **v0.2, amended by [DEC-687](../project/decisions/DEC-687.md):** §8.2 states how the health
  signals count (per signal class) and how several causes are tracked; §9.1's halt paragraph no
  longer lists a reconnect as lifting the restriction (DEC-800 item 5, journal §9.8 rule 68), and
  contract drift clears only as DEC-687 item 3 says until the released-connector-version path is
  decided. Each only tightens (DEC-176); no exit, protective order, cancel, or kill switch is held.
- **v0.2:** §4's Robinhood column, §6.2's mapping, and §6.6's status column are filled from
  Robinhood's published tool contract ([robinhood-contract.md](../project/tasks/robinhood-contract.md),
  E7-15); §4's capability table is the connector's capability profile (trading spec §5.2,
  [DEC-531](../project/decisions/DEC-531.md)). §6.2 maps `GetOrderByClientId` to the shared
  list-and-match fallback, and §12 records how [DEC-529](../project/decisions/DEC-529.md) reads
  DEC-441 items 10, 15 to 17 and 20 for the founder's one live order. A pre-trade alert refuses
  only an opening or an increase (`AGENTS.md` rule 13). Customers stay blocked.
- **v0.1:** first draft (DEC-441).

## 1. Scope

**In scope:**

- The **connection object**: an account reference, the scopes granted, the environment, and a
  pointer into the vault. Never a secret.
- The **connector capabilities matrix**: Alpaca (API keys for paper in Phase 1, OAuth from M8),
  Robinhood Agentic Trading over the Model Context Protocol (MCP, E7-6), and Kraken Derivatives US
  later (E16).
- **Onboarding and revocation**, from both sides: through the platform and at the broker.
- **Permission and scope checks** at connect time, at every executor start, and daily.
- **Health checks** and the degraded states they lead to.

**Out of scope:**

- Order rules, sessions, protection, reconciliation tolerances: [trading domain spec](trading-domain.md).
- Mandate fields, the loss carry, V-001 and V-031: [mandate spec](mandate.md).
- Event envelopes and storage: [journal spec](journal.md).
- The vault product, leases, egress: [infrastructure](../design/infrastructure.md) §5, §9.
- Sign-in, roles, step-up: identity spec (#556). API shapes: workspace API spec (#560).
- Market data through a connection: [data plane spec](data-plane.md).
- Owner-connected agents over MCP (DEC-141). Those are **clients of Mandate**. This spec's MCP is the
  other direction: Mandate as an MCP **client of a broker**. The two never share a token or a code
  path.

## 2. Invariants

Each invariant has a test, written before the code (DEC-77). "Never" and "always" mean every
deployment mode, every environment, and every state of §9.

| ID | Invariant | Source | Test |
|---|---|---|---|
| **CN-1** | **Credentials live only in the vault** and in the memory of the one executor process that uses them. Two narrow exceptions apply. An OAuth authorization code and PKCE verifier pass through the API process in transit, during the callback, before the vault write. During an Alpaca paper OAuth exchange, the token-exchange process (§5.2 step 4; DEC-821 item 2), which is not an executor, holds the platform's OAuth client secret through a single-use grant, the code and verifier it reads once from the vault, and the token it receives, for that one exchange only, as `secrecy` values, until the token's vault write commits. It holds no token lease. None of these is ever logged or persisted outside the vault. No credential, token, refresh token, API secret, or authorization code appears in a journal event, artifact, log, metric, trace, prompt, model context, notification, API response, error message, fixture, or backup outside the vault's own snapshots | Rule 7; FR-2.4; OPS-1; API-11 (#560) | Canary-secret scans of logs, journal, artifacts, API responses, and restored backups; a type test that every connection payload schema has no secret-shaped member; the Alpaca `Debug` test extended to every connector |
| **CN-2** | **No permission that can move funds out.** A connection never holds a scope, key permission, or MCP tool that can withdraw, transfer, or send funds or assets out of the account. A credential that carries one is refused at connect time, before storage, with instructions for a trading-only credential. A live credential whose fund-movement permission cannot be shown absent is refused | HLD §6 A step 2; FR-2.2; infrastructure §5.3 (tightened, DEC-441 item 4) | Per connector: a fixture grant or key with each fund-movement permission is refused before any vault write; an MCP tool list containing a transfer tool is refused; a live key from a venue that cannot report permissions is refused |
| **CN-3** | **Paper and live are distinct connections, and paper never reaches live.** A connection has one environment for life. A paper connection's executor has only paper hosts, and no executor's egress contains a live host; a non-production build has no live host, except the one Alpaca token endpoint that only the token-exchange process reaches (§5.2; the ES-23 narrowing below). A credential that does not work against its own environment is refused, and so is one that also reaches the other environment, until the broker's documentation shows it cannot (U-A4; DEC-441 item 21). The other environment is never probed to find out. **Narrowing for paper Alpaca OAuth only ([DEC-821](../project/decisions/DEC-821.md) items 1 and 5):** a grant requested with `env=paper`, whose `state` names a paper request the server issued (§5.2 step 3), is treated as paper-only. The residual risk that Alpaca's live host might honour it is accepted by the founder, journaled with the connection, and disclosed to the owner (§5.2). A grant that names `live` or both environments is still refused, and nothing else in CN-3 changes | Rule 8; V-001; V-031; OPS-5; ES-23; DEC-441 item 21; DEC-821 | Build test that no live host is compiled outside production, the token-exchange client's one URL aside; a staging egress test that the executor reaches no live host; a fixture test that a token documented as reaching both environments is refused for a paper and for a live connection, with no request to the other host; a fixture test that a `state` not naming a server-issued `env=paper` request is refused before any exchange |
| **CN-4** | **Agents never reach the broker.** Every account-level action goes through the account's executor and its ledger. The runtime holds no credential and has no route to a broker; a connector is reachable only from the executor | Rule 12; trading §7.1; infrastructure §3.1, §3.5 | Crate layering (`xtask/layers.toml`); a vault policy test that a runtime identity reads no connection; an egress test |
| **CN-5** | **One account, one executor, one connection.** A broker account maps to exactly one `account_ref` and one active connection per environment in a workspace deployment, and its account stream has one writer, enforced by the writer epoch. A second connection to an account already connected is refused; a reconnect reuses the same connection | DEC-26; journal §5.1; OPS-3; DEC-441 item 6 | Two concurrent executors for one account: the older is `Fenced` before it can send; connecting the same account twice, from the same or a second workspace in the deployment, is refused |
| **CN-6** | **Losing a credential stops openings at once and never blocks a risk reduction the broker still accepts.** Expiry, revocation, a failed refresh, or a failed permission check sets the account state `closing_only` (trading §7.3, reason `account_restricted`, cause `connection_unavailable`), which moves every agent on the account to `exits_only`; the executor journals `AccountRestrictionChanged` and `AgentModeApplied` before it acts, in the same step (§9.1). While any credential still works, exits, protective re-placement, and kill-switch orders keep going | Rules 3, 13; trading §1 principle 4 | Fault injection: revoke, expire, and fail refresh at every step of an open, an exit, and a kill switch; `AccountRestrictionChanged` (`closing_only`) is committed before anything else, no opening is sent after it, and every exit the fake broker still accepts is sent |
| **CN-7** | **Activity the platform did not originate is external activity.** An order, fill, or position change on the account that has no `client_order_id` of ours, or that the connector cannot attribute, is ingested as external activity (trading §7.1), never adopted as ours | Trading §7.1, §11; DEC-26 | Reconciliation fixtures per connector with an owner order, an order from another platform, and an unattributable fill |
| **CN-8** | **The allocation boundary is the connected account.** Every request a connector sends names the connection's own account. A connector never reads, stores, or acts on another account of the same customer; data about other accounts that a broker returns anyway is dropped at the connector before it is hashed, stored, or journaled | E7-6; R-25; journal §6.4 | Robinhood fixtures where reads return several accounts: only the agentic account's data reaches the executor or the journal; a request naming another account is `NotSent` |
| **CN-9** | **Broker metadata is data, never instructions.** Tool names, tool descriptions, schemas, error text, and any text field a broker returns never reach a model, a prompt, or a notification, and never change what the connector may call. The connector calls only an allowlist of tools pinned by contract hash | R-05; rule 4; DEC-441 item 8 | A fixture MCP server whose tool descriptions carry injection text and whose tool list adds a tool: the text appears nowhere downstream and the new tool is never called; a changed contract hash halts openings |
| **CN-10** | **Every connection change is journaled before it takes effect,** without secrets: connect, each permission-check result (including refusals), state changes, credential rotation, revocation. Journal spec §9.8 (v0.20, DEC-800) carries them: `ConnectionRefused`, `ConnectionCredentialRotated`, `ConnectionChecked`, `ConnectionStateChanged`, and `ConnectionCredentialRefreshed`, beside connect, revoke, and the account state a failure sets | Rule 5; FR-2.2 acceptance; journal §9.2 | Fault injection on the append: no state change is acted on without its committed event; refusal events carry no credential |
| **CN-11** | **A connection cannot be switched under a running agent.** `connection_id` and `environment` never change across mandate versions (V-031); moving an agent to another account means stopping it and deploying a new agent | V-031; mandate §5.7 | A version draft that changes `connection_id` is invalid; a deployment on the new connection starts with that connection's loss carry |
| **CN-12** | **Reconnecting cannot reset a limit.** A reconnect, a token refresh, or a revoke-and-reconnect of the same account keeps the connection's `connection_id`, `account_ref`, account stream, and loss carry | Mandate §5.7 (MI-14, V-032); DEC-441 item 6 | Revoke and reconnect the same account; the loss carry and stream are unchanged and V-032 still binds |

**Global re-check.** CN-6 and the workspace API's refusal to revoke while an agent holds positions
(#560 §4.5) agree: platform-side revocation is never a risk reduction, because it removes the
executor's ability to exit (identity spec #556, "Revoking a connection needs step-up"). The kill
switch comes first. Revocation **at the broker** cannot be refused, so CN-6 says what the executor
does when it happens (§9). CN-12 keeps CN-11's "a new agent on another account" from becoming a way
around the loss carry, because a reconnect is never a new connection.

## 3. The connection object

A connection is a **reference**, never a credential (HLD §3: "a reference to its stored
credential"). The workspace API returns these fields only (#560, API-11).

| Field | Meaning | Where it lives | Journaled today? |
|---|---|---|---|
| `connection_id` | Opaque id; mandates name it (V-001) | Control stream | Yes: `ConnectionEstablished`, `ConnectionRevoked` (journal §9.2) |
| `broker` | `alpaca`, `robinhood`, `kraken_derivatives_us` | Control stream | Yes: `ConnectionEstablished` |
| `environment` | `paper` or `live`, for life (CN-3) | Control stream | Yes: `ConnectionEstablished` |
| `scopes` | Granted scopes, strictly ascending (journal §9.2 rule 19). For MCP, the allowlisted tool names | Control stream | Yes: `ConnectionEstablished` |
| `auth_kind` | `api_key`, `oauth`, `mcp_oauth` | Connection record | No; not needed for replay |
| `account_ref` | Opaque ULID naming the account stream `acct:{workspace_id}:{account_ref}` (journal §2) | Connection record; control stream | Yes: `ConnectionEstablished` version 2 (journal §9.8, DEC-800), which binds the account stream to its connection (DEC-261 item 10). Version 2 carries `account_ref`, the connecting `user`, `step_up`, and `margin_attestation` (`cash_account` or `margin_disabled`), which is non-null exactly when the environment is `live` (rule 64): null for paper, so null for every Alpaca OAuth paper connection |
| `account_fingerprint` | A keyed hash of (broker, broker account id), §3.1. Detects a second connection to the same account (CN-5) without storing the number in clear | Connection record only | Never journaled, never returned |
| `vault_path` | Derived from workspace id and `connection_id`; never sent to a client | Derived, not stored | Never |
| `state` | §9 | Connection record | Through the account state it sets: `AccountRestrictionChanged` and `AgentModeApplied` (§9.1), then `ConnectionStateChanged` on the account stream (journal §9.8) |
| `contract_hash` | MCP only: hash of the pinned tool contract (§6.2 rule 3) | Connection record | **No**; a drift is journaled as a failed `contract` check and the state change it causes (journal §9.8) |
| `checks` | Latest result of each check in §8, with time | Connection record | Yes: `ConnectionChecked` (account stream) and `ConnectionRefused` (control stream), journal §9.8. A failed check still acts through `AccountRestrictionChanged`, committed first |
| `terms_version` | Hash of the broker terms text the owner saw at connect, where a broker has platform terms (Robinhood) | Control stream | Through the existing `DisclosureAccepted` (journal §9.2): `document` `robinhood_agentic_terms`, `version` the terms hash, the owner, and step-up. No new event is needed |

**Reconnect (answers §13 question 2).** A reconnect is a second `ConnectionEstablished` for the same
`connection_id`, valid only after that id's `ConnectionRevoked` and only with the same `broker`,
`environment`, and `account_ref` (journal §9.8 rule 66, DEC-800). Replacing the credential of a
connection that is not revoked, as a `suspended` connection needs, is not a reconnect: it is
`ConnectionCredentialRotated` (DEC-800 item 5, accepted by the founder in DEC-824 item 6).

### 3.1 The fingerprint key

The key is a per-deployment key held in the vault. It never leaves the vault: the connection
manager asks the vault to compute the keyed hash and receives only the result. It is never stored
beside the connection record, so a copy of the records cannot be reversed by trying every account
number. On rotation, every fingerprint is recomputed under the new key from the account ids in the
personal-data vault; until that finishes, every new connect is refused, so uniqueness is never
checked against a mix of keys. A fingerprint is derived from personal data, so it stays inside the
workspace deployment; moving it to the global control plane (§13 question 3) falls under the
control plane's ban on personal data, not outside it.

The broker's account number and its internal account id are **personal data**, held in the
personal-data vault and referenced by `pii_refs` (journal §6.4), as the Alpaca connector already
does in `mandate-alpaca`'s `record` module.

## 4. Connector capabilities

Every connector implements `mandate-executor`'s `BrokerConnector` trait: one request in, one
outcome or a `ConnectorError` out (`Unknown`, `Unreadable`, `NotSent`). The executor never learns
which broker it talks to. A connector also declares a **capability profile** (trading spec §5.2,
[DEC-531](../project/decisions/DEC-531.md)) that the trading spec's order policy (§5.1) is
intersected with at deployment: an agent whose mandate needs a capability the connection lacks is
not deployable. The table below summarizes each connector's profile and the facts beside it.

| Capability | Alpaca paper (keys, Phase 1) | Alpaca live and paper (OAuth, M8) | Robinhood Agentic (MCP, M8) | Kraken Derivatives US (E16) |
|---|---|---|---|---|
| Authentication | API key and secret, local config outside the repo (ES-19) | OAuth 2 authorization code with the platform's client secret, single-use `state`, `env` always set; PKCE not documented by Alpaca, so sent but never relied on (§5.5) | OAuth through Robinhood login at its MCP server (OD-12) | API key; permissions queried from the venue |
| Paper environment | Yes | Yes | **No.** Paper stage is the simulated broker with Robinhood's rules (DEC-124) | Demo environment |
| Asset classes in v1 | US equities, ETFs, crypto spot (DEC-24) | Same | US equities and crypto spot; options excluded (DEC-24) | Perpetuals (Phase 3) |
| Client order id, idempotent | Yes (`client_order_id`) | Yes | **`ref_id`**, deduplicated by Robinhood; what a retry returns is unknown (U-R1) | To confirm |
| Query by client order id | Yes | Yes | **No** (U-R2): the shared list-and-match fallback (§6.2) | To confirm |
| Limit, stop-limit, GTC | Yes (trading §5.2) | Yes | **Yes** for equities; limit quantities in whole shares (U-R3). Crypto unknown | To confirm |
| OCO or bracket | Yes | Yes | **No** (U-R4): one resting GTC stop-limit (trading §5.4) | Not applicable |
| Account restriction signal | Status flags and rejects (trading §7.3) | Same | `review_equity_order` pre-trade alerts; status fields and error codes unknown (U-R6) | To confirm |
| 1× check (FR-2.6) | `multiplier` | Same: `multiplier` is readable with no extra scope (U-A5, answered, §5.5); changing it needs `account:write`, never requested | **No margin field** (U-R7); for DEC-529's order, the founder's attestation | Margin by design; E16 |
| Activities for reconciliation | `/v2/account/activities` | Same | Orders, positions, tax lots; no deposit, withdrawal or dividend feed (U-R8) | To confirm |
| Fund-movement permission | None used; the crate has no such endpoint | **Not shown absent (U-A2 open, §5.5)**: live is refused (CN-2); paper records and discloses it | None in the tool list (U-R11); a tool list that gains one is refused (CN-2) | Queried per key |
| Rate limits | Broker-published | Same | **Unknown (U-R9)** | To confirm |

A capability marked **Unknown** is treated as **absent** until it is confirmed in writing and
recorded in the connector's capability table (rule 3). §6.6 says which unknowns block building.

## 5. Alpaca

### 5.1 Phase 1: API keys, paper only

What exists: `mandate-alpaca` compiles in only the paper trading host and the data host, allows a
fixed list of twelve endpoints with no deposit, withdrawal, or transfer endpoint, reads the key from
an injected lookup into a `SecretString`, redacts the account number and id before hashing, and
forbids a `live` feature. Keys come from local configuration outside the repository (ES-19).

**API keys are paper only (DEC-441 item 3).** An Alpaca API key cannot be narrowed to trading and
its permissions cannot be queried, so the platform cannot show that a live key lacks fund-movement
rights (CN-2). Live Alpaca connections therefore use OAuth only. This tightens HLD §6 A, which
describes key checks for venues that can report permissions; Alpaca keys cannot.

### 5.2 M8: OAuth

The flow, with the workspace API's routes (#560 §4.5). **The API process never calls Alpaca**
(workspace API spec §1.4; DEC-690 item 1). It has no broker host in its egress
(infrastructure §3.1). It takes the owner's input, journals it on the control stream, and reports.
Two other processes reach Alpaca, each for one job:

- **The token-exchange process** makes the code exchange, and nothing else
  ([DEC-821](../project/decisions/DEC-821.md) item 2). It is **not the executor**. It holds the
  single-use grant on the platform's client secret, and its egress is the one token endpoint,
  `POST https://api.alpaca.markets/oauth/token`. It holds no order client, no connector, and no
  token lease (DEC-821 item 4; infrastructure §3.1).
- **The account executor** started for the pending connection runs the broker-facing §8.1 checks.
  It is the one process type that holds a vault lease for one connection and reaches that
  connection's broker hosts. For a paper connection those are paper hosts only: its egress contains
  no live host, and it never holds the client secret.

**Paper only.** Alpaca OAuth connects paper accounts only (DEC-821 item 7). The connection manager
refuses to start an Alpaca OAuth connect whose environment is `live`, before any redirect (CN-2,
DEC-690 item 4; DEC-441 item 21). A live Alpaca OAuth connection needs a new decision.

1. **Start.** A workspace admin presents step-up (identity spec #556, ID-4) and starts the connect.
   The connection manager creates the pending connection record (`connecting`, §9.1) with its
   `connection_id`, `account_ref`, and vault path, and journals it on the control stream as
   `ConnectionRequested` (journal spec §9.8, rule 131), without the vault path, in the transaction
   that marks the step-up challenge used (identity spec §7.2 step 5). It also creates a single-use `state` bound to the
   user, workspace, intended environment, and a PKCE verifier, with a short expiry (Proposed
   default: 10 minutes). The server records that it issued that `state`, for that workspace, with
   `env=paper`. Before the redirect, the owner is shown the token's possible breadth (the residual
   risk below; DEC-821 item 4).
2. **Authorize.** The browser goes to Alpaca's authorization page with these parameters:
   - the platform's client id;
   - the registered redirect URI;
   - the `state`;
   - the PKCE challenge;
   - **only the scopes in §5.3**;
   - `env` set to the connection's environment, always, which for Alpaca OAuth is `paper`
     (DEC-821 item 7). Without `env`, Alpaca prompts for a live and a paper account together
     (DEC-690 item 3).
3. **Callback.** The API checks `state`: it exists, is unexpired and unused, belongs to the same
   user, and names a request the server itself issued with `env=paper`, for that workspace
   (DEC-821 item 3). Any other code is refused before the exchange: the API writes nothing to the
   vault and starts nothing. Otherwise it marks the `state` used and:
   - writes the authorization code and the PKCE verifier **straight to the vault**, write-only,
     under the pending connection's path, in the same request. The code and verifier pass through
     the API process only in transit, as `secrecy` values, and are dropped when the vault write
     commits (CN-1);
   - asks for the pending connection's token-exchange process to start (infrastructure §3.5), and
     reports `connecting`.

   It sends nothing to Alpaca. If the vault write fails, nothing is started. If the token-exchange
   process or the executor fails to start after the vault write, the API deletes the vault entry
   and tears the connection down at once (step 6, reason `start_failed`).
4. **Exchange (token-exchange process), then checks (executor, `connecting`).**

   **The exchange.** The connection manager starts one token-exchange process for the pending
   connection; it serves that one exchange and stops (infrastructure §3.1, §3.5). It:
   - redeems the single-use grant on the platform's client secret, valid only while the connection
     is `connecting` (infrastructure §5.2);
   - reads the code and PKCE verifier once from the pending connection's vault path;
   - exchanges the code at once, since a code lives 10 minutes (§5.5), with the one call of the
     token-exchange client below;
   - parses the response only for the token fields (`access_token`, `token_type`, `scope`), and
     refuses, before any vault write, a token whose `scope` is not exactly the request (§5.3), so
     an over-scoped token is never stored (CN-2);
   - writes the token and the granted `scope` straight to the vault under the pending connection's
     path, write-only, and deletes the code and verifier;
   - drops every secret and stops. It reports to the connection manager only whether a token was
     stored, never a value. It appends nothing to the journal.

   It never retries with a code it has read, because a code is single-use. If no token is stored,
   the connection manager tears the connection down at once (step 6, reason `start_failed`, since
   no executor was started; §13 item 6). Only once a token is stored does the connection manager
   start the pending connection's executor.

   **The token-exchange client** (DEC-821 item 2) is a separate type that can be built only with
   that one URL, and it is never used for anything else:
   - Its allowlist is one method and path: `POST /oauth/token` on `api.alpaca.markets`, over HTTPS.
   - The form body carries only the members the OAuth exchange needs: `grant_type`, `code` or
     `refresh_token`, `client_id`, `client_secret`, `redirect_uri`, and the PKCE `code_verifier`
     when PKCE is used. Nothing else is sent: no account, order, or trading data.
   - It sends no query string and follows no redirect.
   - Every other request to that host is refused before it leaves the process.

   The pinned tests are: the allowlist is that one method and path; the body holds exactly the
   members above; every other request to the host is refused before it leaves the process. Agents
   never call the live host, and their tests use fixtures (rule 8). The first real exchange is the
   founder's own act, when they connect (DEC-821 item 2).

   **The checks.** The executor opens the pending connection's account stream,
   `acct:{workspace_id}:{account_ref}`, with `StreamOpened`, and reads the token and its granted
   `scope` through its lease on its one connection's vault entry. It then runs checks 1 (scope),
   2 (environment), 3 (account), and 7 (contract) of
   §8.1 against the grant and the account, using only reads, through the paper-host request type that `mandate-connections` will provide
   (to be built, E10-13). It stores the broker's account id in the personal-data vault (§3.1,
   journal §6.4). Checks 5 (1× buying power) and 6 (account status) are read from the
   `AccountStateObserved` the executor appends from the same account read (journal §9.2), and
   never refuse a connect: a failed check 5 means
   connected with agents not deployable, and check 6 shows the status (§8.1).

   While `connecting`, the executor does **nothing else**:
   - no order, cancel, or other write to the broker;
   - no reconciliation;
   - no write to the connection record;
   - no lease beyond its own vault entry, and no read of the client secret;
   - no request to any host but the paper trading host the connection names;
   - no append other than `StreamOpened`, `AccountStateObserved`, and `ConnectionChecked`.
5. **Record the results.** The executor appends `ConnectionChecked` (journal spec change E7-17,
   PR #786) on that account stream. It carries the result of checks 1, 2, 3, and 7, pass or
   refusal, and `account_pii_ref`, the personal-data reference of the account id (null if the
   account could not be read), never the id or the token (CN-10). The API then acts on it as the
   connection manager (journal §9.8):
   - It reads the results from the journal.
   - It asks the vault to compute the account fingerprint from that reference, receiving only the
     result (§3.1), and writes the fingerprint to the pending connection record.
   - It completes check 3 by comparing that fingerprint with the account the connection names
     in its record. The connection manager alone refuses `account_mismatch`; the executor never
     reports it.
   - It runs check 4 against every other record, because only the connection manager sees them all
     (CN-5).
   - Its own refusals (check 3's `account_mismatch`, and check 4) have null causation, since no
     executor check failed.

   For an Alpaca OAuth connection, the token's possible breadth (the residual risk below) is
   journaled with the connection, and has been disclosed to the owner (step 1), before
   `ConnectionEstablished` (DEC-821 item 4; DEC-441 item 22). The journal spec change that adds
   that event comes first; until it lands, an Alpaca OAuth connect cannot complete.

   Then:
   - **if all pass**, the API appends `ConnectionEstablished` (version 2, E7-17) on the control
     stream, carrying `account_ref`, `user`, `step_up`, and `margin_attestation` (null for a paper
     connection, rule 64), with `causation_id` set to the passing `ConnectionChecked`. That cross-stream
     causation is permitted by E7-17 (#786, journal §9.8). The executor leaves `connecting` only
     when it reads that event from the control stream and copies it to the account stream, which
     stays `connecting` until then (E7-17, §9.8 rule 68). From then on it runs as the account's
     executor.
   - **if check 1, 2, 3, 7, or 4 refuses**, the API appends `ConnectionRefused` on the control
     stream (E7-17), naming the check. Its causation is the failed `ConnectionChecked` when the
     executor's check failed. The teardown in step 6 runs at once.

   **Until E7-17 lands, steps 4 to 6 cannot be built**, and for Alpaca OAuth neither can step 5
   until the breadth event above is in the journal spec.
6. **Teardown, the only exit from `connecting` other than step 5.** The connection manager tears a
   pending connection down when any check refuses, or when no passing results arrive within the
   code's lifetime plus one minute (Proposed: 11 minutes). Teardown:
   - deletes the vault entry (code or token) under the pending connection's path (infrastructure
     §3.1). The account id stays in the personal-data vault until its retention ends and it is
     erased with `PersonalDataErased` (journal §6.4);
   - revokes the client-secret grant, and stops the token-exchange process if it still runs, and
     the executor;
   - journals `ConnectionRefused` on the control stream, without the token (CN-10). After a
     refused check it names the check. Otherwise `check` is null and `reason` is `timeout`,
     `restart_past_deadline`, `executor_stopped`, or `start_failed`, with null causation (E7-17,
     #786, journal §9.8).

   A refused connection leaves its account stream behind: `StreamOpened` and the failed
   `ConnectionChecked`, if any. That `account_ref` is never bound to a connection and never used
   again (E7-17).

   **Restarts.**
   - *API restart:* the connection manager re-reads every `connecting` record: the
     `ConnectionRequested` records that no establishment or refusal has closed (journal spec §9.8,
     rule 131). It appends
     `ConnectionEstablished` for one whose results all passed and whose check 4 still passes, and
     tears down every other one that is past its deadline.
   - *Token-exchange process restart:* it never re-runs an exchange. A code it has read is spent,
     so the teardown follows at once (reason `start_failed`), unless a token was already stored.
   - *Executor restart while `connecting`:* the executor never exchanges anything. If a token is
     stored, it re-runs the checks; if none is, it stops, and the teardown follows (reason
     `executor_stopped`). An API restart that finds a record past its deadline tears it down with
     reason `restart_past_deadline`.

**The redirect URI is fixed:** `https://api.owlhead.ai/v1/oauth/alpaca/callback`, on the domain in
[DEC-820](../project/decisions/DEC-820.md) and [DEC-822](../project/decisions/DEC-822.md). A registered URI cannot carry a workspace id, so the
callback sits outside the `/v1/workspaces/{workspace_id}` prefix. The workspace, user,
environment, and PKCE verifier are bound server-side to the single-use `state` (workspace API spec
§4.5). A hybrid or on-prem
deployment would need its own registered URI; that is open (§13).

**What DEC-821 narrows, for paper Alpaca OAuth only.** U-A4 is open (§5.5), and Alpaca's token
endpoint is on its live host (`api.alpaca.markets`) for paper grants too. The founder accepted, on
2026-10-08, DEC-690 items 6 and 7 for paper only ([DEC-821](../project/decisions/DEC-821.md)). Each
narrowing below cites DEC-821 and changes nothing else in the rule it narrows (DEC-821 item 5):

- **ES-23** (ADR-0001) and **infrastructure §2**: non-production builds compile in one live-host
  URL, `POST https://api.alpaca.markets/oauth/token`, and only in the token-exchange client of
  step 4. No other live host or path is compiled in, and no executor's egress contains a live host.
- **CN-3**: a grant requested with `env=paper`, whose `state` names a paper request the server
  issued, is treated as paper-only (DEC-821 item 1).
- **DEC-441 item 21**: superseded for `env=paper` grants only. A grant that names `live` or both
  environments is still refused.

DEC-441 item 22's conditions apply in full (DEC-821 item 4): the executor is bound to its own
environment's host; no live trading host is compiled outside production, the token endpoint aside;
egress is limited to the paper trading host and that one endpoint; and the token's possible breadth
is journaled with the connection and disclosed to the owner (steps 1 and 5). The grant requests
`env=paper` and only the scopes of §5.3 (FR-2.2, exact scope refusal). The token stays in the vault,
and in memory only as `SecretString` (rule 7, OPS-1). §8.1's checks still run.

**The residual risk, stated plainly (DEC-821 items 1 and 4).**

- A token granted with `env=paper` might be honoured by Alpaca's live host. Nothing in the
  documentation says otherwise (U-A4).
- `api.alpaca.markets` is both the token host and the live trading API, and network allow lists
  work on addresses, not paths. The token-exchange process can therefore reach the whole live API
  at the network level. In that process, the dedicated client type of step 4 is the only barrier.

Neither can become an order. Every order, and every trading or account call, goes only to the
paper trading host the connection's environment names (`paper-api.alpaca.markets`), from the
executor, whose egress has no live host. No code path sends an order, or any trading or account
call, to the live host with any token. The token-exchange process holds no order client and no token lease.

**Scope (DEC-821 item 7).** This covers paper connections only; a live Alpaca OAuth connection
needs a new decision. The founder may still ask Alpaca in writing whether a paper-environment token
is refused by the live host. A written "yes" retires the residual risk, and a "no" is reported at
once.

### 5.3 Scopes

| Scope | Requested | Why |
|---|---|---|
| `trading` | Yes | Place, cancel, and read orders and positions |
| `data` | Yes | Market data through the user's own account (HLD §12 risk 3) |
| `account:write` | **Never** | A write scope over account settings; FR-2.2 allows trading and account read only |
| Anything else | Never | |

Alpaca's scope names are `account:write`, `trading`, and `data`; reads need no scope (§5.5, U-A1).
The token response carries the granted `scope`. A grant whose scopes differ from the request, by
addition or removal, is refused (CN-2). The questions below were read against Alpaca's public
documentation on 2026-10-08; §5.5 has each answer, its source, and what stays open:

- **U-A1:** the exact scope names, and which scope reading the account needs.
- **U-A2:** whether the `trading` scope reaches any fund-movement endpoint, crypto transfers
  included. If it does, live Alpaca crypto cannot be connected (CN-2) and the founder is asked.
- **U-A3:** whether access tokens expire, whether a refresh token is issued, and how revocation is
  signalled (a 401 on the next call, or a notice).
- **U-A4:** whether one token reaches both paper and live. Until Alpaca's documentation shows a
  token reaches only the environment it was issued for, an Alpaca OAuth grant is refused for both
  paper and live connections (CN-3; DEC-441 item 21), and no request is ever sent to the other
  environment's host to find out. For paper only, the founder accepted an `env=paper` grant as
  paper-only, under DEC-441 item 22's conditions with the token-endpoint exception (DEC-821;
  §5.2). A live grant, or one naming both environments, is still refused.
- **U-A5:** whether an OAuth app may read the 1× setting (trading §15 q3).

### 5.4 Refresh, rotation, and revocation

Alpaca documents no token lifetime, no refresh token, and no revoke call (U-A3, open, §5.5). Until
it does, the executor treats a token as valid until a call fails authorization, never assumes a
refresh exists, and the rules below apply as written.

- If tokens refresh, the refresh runs ahead of expiry (Proposed: at 80% of lifetime) in the
  token-exchange process, with its token-exchange client and a new single-use grant, never in the
  executor or the API process (DEC-821 item 2). Nothing is built while U-A3 is open. A failed refresh is retried with back-off until the
  token expires; at expiry the connection is `suspended` (§9) and CN-6 applies.
- The executor picks up a rotated credential at its next lease renewal without a restart
  (infrastructure §5.4).
- **Revocation by the user at Alpaca** shows as an authorization failure on the next call. Two
  consecutive authorization failures on any request (Proposed) move the connection to `suspended`.
  One is treated as a possible transient: the request's outcome is `Unknown` if it was a submit, and
  is resolved by query once a credential works again (trading §5.7).
- **Revocation through the platform** revokes the token at Alpaca where Alpaca offers a revoke
  call; none is documented (§5.5), and for a paper OAuth connection any such call would be a second
  live-host request, which DEC-821 does not allow. So the confirmation tells the owner to remove the
  app at Alpaca too. It deletes the vault entry, and journals `ConnectionRevoked`. It is refused while an agent on
  the connection holds positions or is not stopped (#560 §4.5).

### 5.5 What Alpaca's documentation says (DEC-690)

Read on 2026-10-08 from Alpaca's public documentation only. Nothing here comes from signing in,
calling an Alpaca API, or a credential. Sources:

- **[O]** "Using OAuth2 and Trading API",
  <https://docs.alpaca.markets/docs/using-oauth2-and-trading-api> (page `updatedAt` 2026-03-02).
- **[A]** Trading API reference, "Get Account" (`/v2/account`) and "Get Account Configurations"
  (`/v2/account/configurations`), <https://docs.alpaca.markets/us/reference/getaccount-1> and
  <https://docs.alpaca.markets/us/reference/getaccountconfig-1>.
- **[W]** Trading API reference, "Request a New Withdrawal" (`POST /v2/wallets/transfers`),
  <https://docs.alpaca.markets/us/reference/createcryptotransferforaccount>.
- **[F]** Alpaca community forum, staff replies of 2020-02-18 and 2020-02-19,
  <https://forum.alpaca.markets/t/using-oauth2-with-both-live-and-paper-accounts/807>. A forum post
  is not documentation: it is recorded as evidence and never settles a question.

| Question | What the documentation says | Status |
|---|---|---|
| **U-A1** scope names; which reads the account | [O] "Allowed Scopes": `account:write` "Write access for account configurations and watchlists."; `trading` "Place, cancel or modify orders."; `data` "Access to the Data API." For `scope`: "Read-only endpoint access is assumed by default." The token response carries `"scope"`. No scope is needed to read the account | **Answered.** Request `trading data`; compare the response's `scope` with the request (§8.1 check 1) |
| **U-A2** does `trading` reach fund movement, crypto transfers included | [O] maps no scope to endpoints and does not mention transfers. The Trading API, on the same hosts, has `POST /v2/wallets/transfers` and `POST /v2/wallets/whitelists` [W]; the reference lists only API-key security, not OAuth scopes. [W] marks the withdrawal endpoint deprecated: "Use the Alpaca web application to initiate withdrawals." Since 2026-07-09, sunset 2026-10-09. Nothing says the whitelist endpoint or a later replacement is out of an OAuth token's reach | **Open.** Fund movement cannot be shown absent, so CN-2 refuses every **live** Alpaca OAuth grant, every asset class; a paper grant records and discloses it (infrastructure §5.3) |
| **U-A3** expiry, refresh, revocation signal | [O]'s token response is `access_token`, `token_type`, `scope`: no `expires_in`, no `refresh_token`. No lifetime, refresh, or revoke call is documented. [F] (staff, 2020): "The token also currently does not expire"; "Authorization codes actually expire in 10 minutes" | **Open.** §5.4's reading holds: valid until an authorization failure; exchange the code at once |
| **U-A4** does one token reach both paper and live | [O]: "An single Alpaca OAuth token may authorize access to either: One live account; One paper account; One live account and one paper account". `env`: "If provided, must be one of `live` or `paper`. If not specified, the user will be prompted to authorized both a live and a paper account." And: "If you specify a value for the `env` parameter when redirecting to us, we will ask the user to authorize only a live or a paper account". [O] does **not** say a host refuses a token not authorized for its environment, and the token response names no environment. [F] (staff, 2020) says the opposite of binding: the token "belongs to the user instead of a specific account and can be used for both paper and live accounts (using api.alpaca.markets vs paper-api.alpaca.markets)". [O]'s token endpoint is `POST https://api.alpaca.markets/oauth/token`, the live host, for either environment | **Open.** The documentation describes what the owner authorizes, not what a host enforces, so it does not show a token reaches only its own environment. For paper only, the founder accepted an `env=paper` grant as paper-only and the one token-endpoint call (DEC-690 items 6 and 7, see DEC-821); DEC-441 item 21 still refuses every grant naming `live` or both |
| **U-A5** may an OAuth app read the 1× setting | [A]: `GET /v2/account` returns `multiplier` ("valid values 1 ... 2 ... 4"); `GET /v2/account/configurations` returns `max_margin_multiplier`. Both are reads, and [O] says "Read-only endpoint access is assumed by default"; no page names these two endpoints for OAuth, so this is that general rule applied. Writing the configuration needs `account:write` | **Answered**, by the general rule; a refused read fails §8.1 check 5 closed (agents not deployable). FR-2.6 is checked by reading `multiplier`; the platform never sets it, and the owner changes it at Alpaca (trading §15 q3) |
| PKCE (not a U-A question) | [O] documents no `code_challenge`; the exchange authenticates with `client_secret` from "your backend server" | **Open.** PKCE is sent but never relied on; the client secret and the single-use `state` are the controls |

## 6. Robinhood Agentic Trading over MCP

### 6.1 What Robinhood offers (from ADR-0002 and OD-12)

- A customer opens a **dedicated agentic account**, funds it with what they are willing to risk, and
  connects a third-party agent through Robinhood's MCP server, authenticating with their Robinhood
  login. Stocks trade in the agentic account; crypto through a matching Robinhood Crypto account
  (not in New York; no transfer, staking, or lending).
- The MCP session **reads every Robinhood account of the customer**, including account numbers,
  positions, balances, and transactions (OD-12, RAID R-25).
- Robinhood states that it does not control, supervise, monitor, or audit connected agents, and that
  the customer is responsible for the agent's trades.
- There is no paper or test environment (DEC-124).

### 6.2 How MCP maps to the connector interface

The Robinhood connector is a `BrokerConnector` like Alpaca's. Inside it, an MCP client speaks to
Robinhood's server; nothing outside the connector knows MCP exists.

| `BrokerRequest` (executor) | MCP mapping |
|---|---|
| `Submit` | `review_equity_order`, then `place_equity_order`, with `account_number` the founder-typed agentic account and `ref_id` derived deterministically from the journaled intent's idempotency key, never omitted. A pre-trade alert refuses an opening or an increase before the place. For a sell or a protective order the alert is journaled and the order is placed anyway, so the broker accepts or rejects it (`AGENTS.md` rule 13); C1's tests pin that an alert on the protective stop-limit does not refuse it. An unparsable or unexpected answer to the place is `Unknown`, never rejected and never re-sent |
| `Cancel` | `cancel_equity_order`, by the broker `order_id` from our record |
| `GetOrderByClientId` | Not offered (U-R2). The shared fallback ([DEC-529](../project/decisions/DEC-529.md) item 4), used only when the account ledger shows the account dedicated to one agent: `get_equity_orders` with the intent's `symbol`, `placed_agent` `agentic`, and `created_at_gte` the intent's `OrderSubmitted` time less a fixed margin, all pages; exactly one record matching side, type, quantity, limit price and time in force (and `ref_id`, if records carry it) is adopted and followed by `order_id`; zero or several leave the order `Unknown` (trading §5.3 rule 9). The connector never re-sends on its own |
| `ListOpenOrders`, `ListPositions`, `GetAccount` | `get_equity_orders`, `get_equity_positions`, `get_accounts`, `get_portfolio`, filtered to the agentic account (CN-8) |
| `ListActivities` | No feed in the contract (U-R8) |
| `CancelAll`, `ClosePosition` | Only if the contract offers an account-scoped equivalent; otherwise the kill switch cancels and sells order by order, which trading §5.5 already does for agent scope |
| `AcknowledgeReplace` | Not applicable unless Robinhood replaces orders itself |

Robinhood's `state` reads as: `new`, `queued`, `confirmed`, `partially_filled` working;
`unconfirmed` working, never absent; `filled` filled; `cancelled` cancelled with any filled
quantity; `rejected`, `failed` rejected and `voided` cancelled, each re-read once by `order_id`
before the intent closes; any other value `Unknown`.

Rules for the MCP client:

1. **Transport.** The streamable HTTP transport to Robinhood's published endpoint only, pinned by
   host; no redirects; TLS verified; no other MCP server is ever configured for a broker connection.
2. **Tool allowlist.** The connector calls only the tools it was built for: today the nine of
   the contract's allowlist (`get_accounts`, `get_portfolio`, `get_equity_positions`,
   `get_equity_quotes`, `get_equity_tradability`, `get_equity_orders`, `review_equity_order`,
   `place_equity_order`, `cancel_equity_order`); crypto tools join when a story needs them. It never calls options, option
   exercise, watchlist, alert, scan, or any write tool outside that list, even though the token may
   allow them (CN-9).
3. **Contract pinning.** At connect, at each session start, and at each health check, the connector
   lists the server's tools and hashes the canonical form of the allowlisted tools' names and
   input and output schemas. A hash that differs from the pinned `contract_hash` is **contract
   drift** (§9).
4. **Metadata never reaches a model.** The executor has no model. Tool descriptions, error text,
   and free-text fields reach no model, prompt, notification, or research input (CN-9). They are
   journaled only inside the redacted exchange records (`BrokerExchangeRecorded`, journal §6.3),
   which no model, prompt, or notification ever reads. Tool names are journaled deliberately, as
   `scopes` and inside the contract hash.
5. **Numbers.** Prices and quantities are parsed from text by `mandate-num`; a JSON number that has
   been through a float is `Unreadable` (ES-23).
6. **Data minimization.** Results naming any account other than the agentic account (and its
   matching crypto account) are dropped inside the connector before redaction, hashing, or storage
   (CN-8). Only the agentic account's number is stored, by reference (journal §6.4).

### 6.3 The dedicated account is the allocation boundary

The agent can reach only what the customer deposited in the agentic account (ADR-0002). The
platform still enforces the mandate's allocation inside it: the account ledger and gate treat the
agentic account as one broker account (trading §7.1). The 1× requirement (FR-2.6) applies: if
the account has margin enabled and no enforced 1× cap can be confirmed (U-R7), agents are paused and
the owner is prompted, as for Alpaca (trading §7.2).

Other activity in the agentic account, by the owner or another agent, is external activity (CN-7)
and switches the account's agents to `exits_only` until acknowledged (trading §7.1). The owner is
told at connect that the agentic account must not be shared with another agent.

### 6.4 What Robinhood's beta terms require us to monitor

| What | How | Who acts |
|---|---|---|
| Terms and beta status | The terms text the owner saw is hashed into `terms_version` at connect. A change found in the published terms is reviewed before the next connect | Founder (legal text, DEC-79) |
| Whether one platform may act for many customers | Written answer from Robinhood before any customer connects (DEC-441 item 15; the founder's own order is excepted, DEC-529 item 5) | Founder |
| Tool contract | `contract_hash` checked at each session and health check (§6.2 rule 3) | Connector; drift halts openings |
| Rate limits | Observed limits and throttle responses counted per connection; the executor's order-rate limit (trading §9.7) is set below the published limit with headroom | Connector |
| Customer responsibility statement | Shown to the owner at connect and in the disclosure (compliance question 32) | Founder with counsel |
| Account scope | Each read checked against CN-8 | Connector |

### 6.5 Rate limits

Until Robinhood publishes limits (U-R9), the connector uses a conservative per-connection token
bucket for all calls and a separate, reserved budget for exits, cancels, and kill-switch orders so
that reads can never starve a risk reduction (rule 13). A throttle response to a submit is treated
as `Unknown` and resolved by query, never by a blind retry, unless the contract states the request
was not processed.

### 6.6 What must be confirmed before building

None of these may be learned by calling Robinhood with a real account: there is no test
environment, and an agent never touches a live account (rule 8). They come from Robinhood's
published documentation or a written answer to the founder. The status column is from Robinhood's
published tool contract as of 2026-10-08 ([robinhood-contract.md](../project/tasks/robinhood-contract.md),
which gives each answer's source); "from contract" is not a written answer.

| ID | Question | Status | If the answer is no or unknown |
|---|---|---|---|
| U-R1 | Do order tools accept a client order id, and is a retry with the same id idempotent? | **In part from contract:** `ref_id`, deduplicated by Robinhood; what a retry returns, the window and the scope need a written answer | Customers: **E7-6's order path is not built for them** (DEC-441 item 10). DEC-529's order: `ref_id` from the intent's key, never re-sent (DEC-529 item 4) |
| U-R2 | Can an order be queried by that id? | **Absent from contract**; whether records carry `ref_id` needs a written answer | As U-R1; DEC-529's order uses §6.2's list-and-match |
| U-R3 | Limit, stop-limit, GTC, and extended-hours flags for equities; limit and stop-limit for crypto | Equities **from contract** (limit, stop-limit, gfd, gtc; extended hours limit only); crypto open | Missing types make the mandate's order policy undeployable on Robinhood |
| U-R4 | OCO or bracket orders, or at least a resting stop-limit | Resting GTC stop-limit **from contract**; OCO and bracket placement **absent** from the tool list | One GTC stop-limit for the whole position (trading §5.4; DEC-529 item 7 for DEC-529's order). With no resting stop at all, no protected equity mandate can deploy on Robinhood |
| U-R5 | Token lifetime, refresh, and how revocation shows | **Open** | Treated as short-lived: re-check before every session |
| U-R6 | Account status fields and restriction rejects | **In part:** `review_equity_order`'s pre-trade alerts; status fields and error codes open | An alert refuses an opening or an increase before the place; a sell or protective order is placed anyway, with the alert journaled (rule 13). An unrecognized status is `blocked` (trading §7.3 row 1; principle 3) |
| U-R7 | Margin and 1× status | **Open:** no margin field in the contract | Paused and prompted (FR-2.6); DEC-529's order rests on the founder's attestation (DEC-529 item 11) |
| U-R8 | Fills, fees, dividends, deposits, and withdrawals as an activity feed | **In part:** orders, positions and tax lots; no deposit, withdrawal or dividend feed; fill fields unpublished | Reconciliation (trading §11) cannot run; not deployable for customers. DEC-529's order on a flat, dedicated account compares the order's state and filled quantity and the position |
| U-R9 | Rate limits | **Open** | §6.5's conservative default |
| U-R10 | Platform terms: one platform acting for many customers; data use; attribution | **Open**; `placed_agent` `agentic` shows attribution | DEC-441 item 15; it does not block DEC-529's order (DEC-529 item 5) |
| U-R11 | Whether the tool list or the token can move funds out | **No fund-movement tool** in the list; the token's scope beyond the tools is open | If yes, refused (CN-2) |
| U-R12 | Whether the session can be restricted to the agentic account | **Not restricted for reads** (from contract) | CN-8's filtering is the control, and the owner is told the token can read every account |

## 7. Kraken Derivatives US (later)

Phase 3 (E16). The connector follows HLD §6 A: an API key whose permissions are queried from the
venue at connect, at each executor start, and daily; a key with withdrawal or transfer permission
is refused before storage (E16-1, CN-2). Demo and live are distinct connections (CN-3).
Perpetuals accounting and margin are outside v1 (trading §16). This spec adds nothing else for
Kraken now.

## 8. Permission checks and health checks

### 8.1 At connect, at every executor start, and daily

Each check's result is journaled without the credential (CN-10; infrastructure §5.3). Checks 1, 2,
3, and 7 run in the connection's account executor, never in the API process, which holds no
credential and reaches no broker (workspace API spec §1.4; DEC-690 item 1), and never in the
token-exchange process, which only exchanges the code (§5.2 step 4; DEC-821 item 2). The executor appends
their results on its account stream as `ConnectionChecked` (E7-17). Checks 5 and 6 are read from
`AccountStateObserved` and never refuse a connect. Check 4 runs in the connection
manager, the only component that sees every connection's fingerprint. At connect, the API reads the
results from the journal and only then appends `ConnectionEstablished` or tears the pending
connection down (§5.2).

| # | Check | Failure at connect | Failure later |
|---|---|---|---|
| 1 | **Scope:** granted scopes equal the requested set; no fund-movement permission; for MCP, the allowlisted tools are present and no fund-movement tool exists (CN-2) | Refused; vault entry deleted | `suspended`; agents `exits_only`; owner alerted |
| 2 | **Environment:** the credential works against the connection's environment, and the broker's documentation shows it does not reach the other; for a paper Alpaca OAuth grant, its `state` named a server-issued `env=paper` request instead (CN-3's narrowing, DEC-821). The other host is never probed (CN-3, U-A4) | Refused | `suspended` |
| 3 | **Account:** the account the credential reaches is the one the connection names, by fingerprint; for Robinhood, the dedicated agentic account | Refused | `suspended`; reconciliation runs |
| 4 | **Uniqueness:** no other active connection in the deployment has the same fingerprint (CN-5) | Refused, with the existing connection named by id | Not applicable |
| 5 | **1× buying power** (FR-2.6, trading §7.2) | Connected, agents not deployable until fixed | Agents `paused`; owner prompted |
| 6 | **Account status** (trading §7.3) | Connected; status shown | Trading §7.3's table |
| 7 | **Contract hash** (MCP only) | Refused if the allowlisted tools are missing | Contract drift (§9) |

### 8.2 Health

The executor runs a health probe on a schedule (Proposed: every 60 seconds while any agent is
deployed, every 15 minutes otherwise) using a read the connector already has (`GetAccount`). The
probe draws from the read budget, never from the reserved exit budget (§6.5). It
records latency, error class, rate-limit headroom, and, for MCP, the contract hash. It changes state
only through the transitions in §9; health never adds risk and never alone blocks an exit.

| Signal | Threshold (Proposed) | State |
|---|---|---|
| Probe or call errors, network class | 3 consecutive | `degraded` |
| Authorization failure | 2 consecutive | `suspended` |
| Rate-limit headroom | Under 20% of the published or default budget | `degraded` |
| Contract hash differs | Any | `degraded` with openings halted (contract drift) |
| Recovery | 3 consecutive good probes | The connection's condition has cleared; the state returns to `active` only as §9.1 says, after the owner's acknowledgment |

**How the signals count ([DEC-687](../project/decisions/DEC-687.md)).** The Proposed thresholds are
the interim defaults and stay Proposed. Counting is per signal class (network, authorization,
headroom): a failure count resets only on a good result of the same class, and a good-result count
only on a failure of the same class; another class's result changes neither. Each new cause that
arrives while the connection is already `degraded` or `suspended` is journaled, a degrading cause
while `suspended` stays `suspended` and is outstanding, and the condition clears only once every
outstanding cause has cleared. A new cause after the condition cleared refuses the owner's
acknowledgment until it clears again (DEC-687 items 1, 2, and 5). None of this holds an exit, a
protective order, a cancel, or the kill switch.

## 9. Lifecycle walk

### 9.1 States

| State | Entered when | Openings | Exits, protection, kill switch | Ends when | Who ends it |
|---|---|---|---|---|---|
| `connecting` | Step-up and connect started (`ConnectionRequested`, journal §9.8 rule 131) | No agent yet; the token-exchange process may only exchange the code, and the executor may only run checks and append their results (§5.2 step 4) | — | `ConnectionEstablished` (`active`), or the teardown of §5.2 step 6 on a refusal, a timeout, or a restart past the deadline (refused: the journal keeps the `ConnectionRequested`, the refusal event, and the orphan account stream, and nothing else) | System |
| `active` | All §8.1 checks pass | As the gate allows | Yes | Any transition below | — |
| `degraded` | Network errors, low headroom, or contract drift | **Halted**: account state `closing_only`, agents `exits_only` | Yes, while the broker accepts | Good probes, or for drift a released connector version (not yet decided: until it is, drift clears only as [DEC-687](../project/decisions/DEC-687.md) item 3 says), **then** the owner's acknowledgment (trading §7.3, cause `connection_unavailable`) | Owner, with step-up |
| `suspended` | Credential invalid: expired, revoked at the broker, refresh failed, or a later permission check failed | **Halted**: account state `closing_only`, agents `exits_only` | Attempted while any call succeeds; otherwise protection rests at the broker | Re-authorization only (DEC-800 item 5, accepted by the founder in DEC-824 item 6): the owner replaces the credential, a `reauthorize` check after the suspension passes every §8.1 check, the control services accept it for the same account as `ConnectionCredentialRotated`, and the executor clears the cause on that rotation (journal §9.8 rule 68); the owner then acknowledges (trading §7.3, cause `connection_unavailable`). A revoke and reconnect of the same account (§3) does not by itself leave `suspended`: the reconnected connection is still `suspended`, and the owner still re-authorizes | Owner, with step-up |
| `revoked` | Platform-side revoke, refused unless every agent on it is stopped with no positions | None | None (no agents) | Reconnect of the same account reuses the record (CN-12) | Owner, with step-up |

**How `degraded` and `suspended` halt openings (DEC-441 items 7 and 23).** Trading spec §7.3
(v0.15) has a row for exactly this: the connector reporting `degraded` or `suspended` sets the
account state `closing_only` (reason `account_restricted`), which makes every agent on the account
`exits_only` (mandate §5.9 lists the trading spec's account restrictions). `blocked` is not used,
because `paused` holds exits. `AccountRestrictionChanged` carries the cause
`connection_unavailable`, so the journal never says the broker restricted the account when it did
not, and the owner gets a distinct alert. The executor journals
`AccountRestrictionChanged` and then `AgentModeApplied` before it sends or refuses anything else,
so replay reaches the same mode from the journal alone; the health signals themselves are never an
unjournaled input to the mode machine. A degrading cause the executor holds unjournaled while
`suspended` ([DEC-687](../project/decisions/DEC-687.md) item 1) feeds only the decision to write
`condition_cleared` and to accept the acknowledgment, never the mode machine. It lifts on the connection's own condition clearing, as the table above
says (good probes for `degraded`, re-authorization for `suspended`, and for contract drift
[DEC-687](../project/decisions/DEC-687.md) item 3), and then the owner's acknowledgment; a reconnect
lifts nothing by itself (DEC-800 item 5, journal §9.8 rule 68), and an account refresh is not the
condition. Recovery therefore needs the owner even when the cause was
transient. A restriction that lifts with no acknowledgment would be a separate spec-first change
(ES-22).

### 9.2 Walk

| Step | What happens | Invariants |
|---|---|---|
| **Connect** | Step-up; pending record (`connecting`), journaled as `ConnectionRequested`; OAuth or key entry; code or key to vault (API); for OAuth, the code exchanged by the token-exchange process; executor started in `connecting`, which may only run checks and append their results (§5.2 step 4); check 4 and `ConnectionEstablished`, whose causation cites the passing results (API); only then does the executor act on the account, starting with the first reconciliation. A refusal, a timeout, or a restart follows §5.2 step 6 | CN-1, CN-2, CN-3, CN-5, CN-10 |
| **Verify permissions** | §8.1 at every executor start and daily | CN-2, CN-3 |
| **Healthy** | `active`; health probe; daily 1× and permission checks | — |
| **Degraded** | `closing_only`, so agents are `exits_only`; exits continue. The owner is alerted after a configured period (Proposed: 5 minutes) for network errors and low headroom, and **at once** for contract drift, which is not transient | CN-6, rule 13 |
| **Token expiry** | Refreshed ahead of time where refresh exists. If refresh fails and the token expires, `suspended`; until expiry the old token keeps serving exits | CN-6 |
| **Revoked by the user at the broker** | Authorization failures → `suspended` → `closing_only`, agents `exits_only`, owner alerted at once. Protection already resting at the broker stays (trading §5.4). Any submit in flight is `Unknown` and blocks that instrument until resolved after reconnect (trading §5.7) | CN-6, CN-7 |
| **Broker outage** | `degraded`. Submits that time out are `Unknown`; reconciliation runs when the broker returns (trading §11). The kill switch keeps trying; the owner is told to act at the broker if needed, which is then ingested as external activity | CN-6, CN-7, OPS-4 |
| **Account restricted** | Trading §7.3's table: `blocked` pauses agents, `closing_only` makes them `exits_only`. For Robinhood, any status not in the confirmed table is `blocked` (U-R6) | Rule 3 |
| **Disconnect (platform)** | Refused while agents hold positions or are not stopped (#560). Otherwise: revoke at the broker where possible, delete the vault entry, `ConnectionRevoked`, executor stopped | CN-10 |
| **Reconnect** | Same account, by fingerprint: the same `connection_id`, `account_ref`, stream, and loss carry; full reconciliation before any agent resumes; resuming a paused agent needs step-up acknowledgment (trading §11) | CN-5, CN-12 |
| **Mandate version that switches connection** | Invalid (V-031). The owner stops the agent and deploys a new one on the other connection, which starts with that connection's loss carry (V-032) | CN-11 |
| **Executor restart or failover** | New writer epoch fences the old one; permission checks; reconciliation before any agent leaves `Recovering` | CN-5, OPS-3, OPS-9 |
| **Vault outage** | A running executor keeps its lease until expiry (infrastructure §5.2); at lease expiry it is treated as `suspended` for openings | CN-6 |
| **Session close, midnight** | Nothing connection-specific; the daily permission and 1× checks run in the overnight window | — |

## 10. Adversaries

| Attacker or failure | Attempt | Blocked or disclosed by |
|---|---|---|
| **Stolen token** (from a backup, a log, an executor) | Trade the account from elsewhere | CN-1 keeps tokens out of everything but the vault and one process; trading-only scope means no withdrawal (CN-2); trades placed elsewhere are external activity and stop the agents (CN-7); the owner revokes at the broker. Residual: a thief can still trade the account until revoked. Disclosed; threat model (#557) row 7 |
| **Over-scoped key or grant** | Owner pastes a full-access key, or a grant comes back wider than requested | Refused before storage (CN-2, §8.1 check 1); live Alpaca keys are refused outright (§5.1) |
| **Malicious or compromised MCP server, or prompt injection in tool metadata** | Tool descriptions telling an agent to transfer funds; a new tool; changed schemas; text in error messages | No model sees tool metadata (CN-9); only allowlisted tools are called; contract drift halts openings; a server offering a fund-movement tool is refused (CN-2); host pinned, no redirects (§6.2 rule 1). Residual: a compromised server can lie about state, covered below |
| **Broker returning inconsistent state** | Wrong positions, duplicate fills, another account's data, a missing order | Reconciliation by client order id and fill id; mismatches pause the agent (trading §11); data naming another account is dropped (CN-8); `Unknown` orders block only their instrument (rule 13) |
| **User connecting someone else's account** | Uses another person's broker login or keys | The broker's own authentication proves control of the login, not ownership. The owner attests ownership with step-up at connect; whether to match the account holder's name against the workspace user is DEC-441 item 19 (Proposed) |
| **Same account connected twice** | Two workspaces, or two connections, to reset limits or run two executors on one account | Fingerprint uniqueness (CN-5) inside a deployment. Across deployments (another cell, a customer site, another platform) it cannot be detected; the second platform's orders are external activity (CN-7) |
| **Revoke and reconnect to reset the loss carry** | Retire agents, revoke, reconnect as a "new" connection | The reconnect is the same connection (CN-12) |
| **Paper deployment pointed at live** | A paper mandate on a live connection, or a token that answers both | V-001; one environment per connection; no live host outside production, the token-exchange client's one URL aside; a token that reaches both is refused (CN-3, DEC-441 item 21). For a paper Alpaca OAuth token that might also reach live, the residual risk is disclosed (§5.2), and no executor can reach the live host |
| **Compromised or buggy token-exchange process** | Use the token it receives, or the client secret, against the live trading API | It holds no order client and no token lease, and stops after its one exchange; its dedicated client can build only `POST /oauth/token` and refuses every other request before it leaves the process (§5.2 step 4). Residual, disclosed: at the network level it can reach the whole live API, because the token host is the live API host; in that process the client type is the only barrier (DEC-821 item 4) |
| **A careless user** | Shares the Robinhood agentic account with another agent, or trades in it by hand | External activity stops openings until acknowledged (CN-7); disclosed at connect |
| **A malicious insider** | Reads a credential from the vault | No human and no identity other than the account's executor may read a credential; that executor's identity reads exactly one (infrastructure §5.2); break-glass grants no read-out (§5.5); every read is in the vault's audit log |
| **Bad tick or outage during a revocation race** | Opening sent after revocation began | CN-6: the state change and the executor's opening refusal are one executor step; anything in flight is `Unknown` and resolved by query |
| **Rate-limit exhaustion** | Heavy reads starve an exit | Reserved exit budget (§6.5); trading §9.7 keeps orders below broker limits |

## 11. What exists and what is planned

| Part | State |
|---|---|
| `BrokerConnector` trait, `ConnectorError`, the account-wide scope type (`mandate-executor`) | **Exists** |
| Alpaca paper connector: paper host only, endpoint allowlist, `SecretString` credentials, account number redaction (`mandate-alpaca`) | **Exists** (E7-2, E7-3, E7-8) |
| Writer-epoch fencing for one writer per account stream (`mandate-journal`) | **Exists** |
| `ConnectionEstablished`, `ConnectionRevoked` payload schemas (journal §9.2) | **Exists** (registered, E7-10); version 2 with `account_ref` is specified in journal §9.8 (E7-17) and not yet registered |
| Connection manager service, connection record, fingerprint, states | Planned: E7-11 |
| Permission checks (§8.1) and refusal events | Planned: E7-12 |
| Alpaca OAuth, paper only, with the token-exchange process and its dedicated client (DEC-821) | Planned: E7-1 (M8) |
| Health probe, mapped onto `closing_only` (§9.1) | Planned: E7-13 |
| Reconnect reusing the connection | Planned: E7-14 |
| Robinhood contract confirmation | Planned: E7-15 (no code) |
| MCP client with allowlist and pinning | Planned: E7-16 |
| Robinhood connector | Planned: E7-6 (M8), blocked on E7-15 |
| Connection state, check, refusal, and rotation event schemas | **Specified** (journal §9.8, DEC-800); registration planned with E7-11 to E7-14 |
| Vault, leases | Planned (infrastructure §5; DEC-434 item 14, accepted by the founder on 2026-10-03) |
| API routes | Planned: E10-13 (#560) |
| Kraken connector | Planned: E16-1 |

## 12. Decisions

[DEC-441](../project/decisions/DEC-441.md) records this spec's choices.

**Accepted (agent, DEC-79 and DEC-176):** each tightens a rule or resolves a gap by the reading that
adds no risk: precedence (item 1); references only (item 2); Alpaca keys paper only (item 3); a
live credential whose fund-movement permission cannot be shown absent is refused, tightening
infrastructure §5.3 (item 4); Alpaca scopes `trading` and `data` only (item 5); one connection per
account, reconnect reuses it (item 6); the connection state machine, with `degraded` and `suspended` mapped onto
`closing_only` (item 7); MCP allowlist, contract pinning, and no metadata to models (item 8); data minimization
(item 9); no Robinhood order path without client order id idempotency (item 10); no live Robinhood
call by any agent, fixtures from the published contract only (item 11); V-031 restated (item 12);
backlog rows (item 13); health defaults (item 14); trading §7.3's connection row and the `cause` on
`AccountRestrictionChanged` (item 23); a token that reaches both environments is
refused until U-A4 is answered (item 21; superseded for `env=paper` Alpaca grants only by
[DEC-821](../project/decisions/DEC-821.md) item 5).

[DEC-690](../project/decisions/DEC-690.md) records what Alpaca's documentation answers (§5.5).
Accepted (agent): the API never calls a broker; the code exchange runs in the token-exchange
process and the checks in the executor (item 1, agent-accepted under DEC-79 as a reversible
engineering decision, conforming to DEC-821 item 2); U-A1 and U-A5 answered (items 2 and 8); `env`
always set (item 3); U-A2 and U-A3 open, with live Alpaca OAuth refused under CN-2 and no refresh
assumed (items 4 and 5); PKCE never relied on (item 9). **Accepted by the founder, see
[DEC-821](../project/decisions/DEC-821.md), paper only:** an `env=paper` grant is treated as
paper-only (item 6), and the one token-endpoint call (item 7). A live Alpaca OAuth connection needs
a new decision.

**Proposed for the founder** (vendor terms, live accounts, legal text; DEC-79):

| Item | Decision | Recommendation |
|---|---|---|
| 15 | Robinhood platform terms: may one platform act for many customers (OD-12) | Get Robinhood's written answer before E7-6 builds; no Robinhood connection in any environment until then |
| 16 | How Robinhood fixtures are obtained | Synthesized from the published tool contract; any recording from a real account is the founder's own, on the founder's own account, after counsel sign-off (DEC-98) |
| 17 | Robinhood equity protection without OCO or bracket | Accept one resting GTC stop-limit for the whole position, as crypto does (DEC-36), only if confirmed (U-R4); otherwise no protected equity mandates on Robinhood |
| 18 | Alpaca OAuth app registration and its terms | **Resolved by [DEC-821](../project/decisions/DEC-821.md) item 6:** the founder registers the app, for paper only, after reading Alpaca's app terms, with the redirect URI of §5.2 |
| 19 | Verifying the account holder is the workspace user | No name matching in v1; owner attestation with step-up; counsel to confirm |
| 20 | Robinhood customer-responsibility and data-scope disclosures | Counsel drafts the text (compliance question 32) before any Robinhood connection |
| 22 | If every Alpaca token reaches both environments, so item 21 blocks Alpaca OAuth entirely | Accept such a token only with the executor bound to its own environment's host, no live host outside production, egress limited to that host, and the token's breadth journaled and disclosed to the owner. **For paper, adopted by [DEC-821](../project/decisions/DEC-821.md)** with the token-endpoint exception (§5.2); open for live |

**The founder's one live order** ([DEC-529](../project/decisions/DEC-529.md), founder, Accepted
2026-10-08) reads these items for that order only, on the founder's own Robinhood agentic account;
each stays in force for every customer connection:

- **Item 10:** half met (`ref_id` yes, query by it no); §6.2's list-and-match with no re-send
  resolves it for DEC-529's order (DEC-529 item 4). Customers wait for U-R1 and U-R2.
- **Item 15:** does not block the founder's own order (DEC-529 item 5); it blocks every customer
  connection in any environment until Robinhood answers in writing.
- **Item 16:** fixtures are synthesized from the published contract only; nothing is recorded from
  a real account (DEC-529 item 6).
- **Item 17:** one resting GTC stop-limit for the whole position after the entry fills completely
  (DEC-529 item 7; trading §5.4).
- **Item 20:** does not apply to the founder's own account (DEC-529 item 8).
- **CN-3 and ES-23:** the runner's `live` cargo feature adds Robinhood's published MCP host to a
  build the founder makes locally from `main`; the default build has no live host (DEC-529 item 3).
  The connection the founder records through the CLI is spent once an order it placed fills, wholly or partly, or its answer is lost and the order is `Unknown`, until the founder reconciles it (DEC-529 item 1). "Unspent" is checked only
  when the version is confirmed and deployed and when the runner starts. A spent connection never
  carries a second opening, but the agent deployed on it keeps `cli_confirm` for owner exits, pause,
  stop, acknowledgments and grants until its account is flat (rule 13).

## 13. Open questions

1. U-A2, U-A3, U-A4, and PKCE (Alpaca; U-A1 and U-A5 answered, §5.5) and U-R1 to U-R12
   (Robinhood), above. The Alpaca ones need Alpaca's written answer (DEC-690 item 10).
2. Answered in §3: a reconnect is a second `ConnectionEstablished` for the same `connection_id`,
   valid only after its `ConnectionRevoked`; journal §9.8 rule 66 (DEC-800).
3. Whether cross-deployment duplicate detection (CN-5) is worth a fingerprint registry in the
   global control plane. A keyed hash of an account id is derived from personal data, so the control
   plane's ban on personal data applies to it (§3.1); it would also add a dependency the trade path
   must not have.
4. Kraken's key-permission query shape, at E16.
5. The OAuth redirect URI for a hybrid or on-prem deployment, which would need its own
   registration with Alpaca (§5.2).
6. A teardown reason of its own for a failed token exchange. Until the journal spec names one,
   §5.2 uses `start_failed`, because no executor was started. Adding one is a journal spec change,
   together with the event that journals the token's possible breadth (§5.2 step 5, DEC-821
   item 4).
