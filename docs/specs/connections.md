# Broker Connections Spec (v0.1, draft)

| | |
|---|---|
| **Status** | Draft v0.1, not yet reviewed |
| **Owner** | Engineering |
| **Decisions** | [DEC-441](../project/decisions/DEC-441.md) (items 1 to 14, 21, and 23 Accepted by the agent; items 15 to 20 and 22 Proposed for the founder); [DEC-690](../project/decisions/DEC-690.md) (Alpaca's documentation read for U-A1 to U-A5; items 1 to 5 and 8 to 10 Accepted, items 6 and 7 Proposed) |
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
| **CN-1** | **Credentials live only in the vault** and in the memory of the one executor process that uses them. No credential, token, refresh token, API secret, or authorization code appears in a journal event, artifact, log, metric, trace, prompt, model context, notification, API response, error message, fixture, or backup outside the vault's own snapshots | Rule 7; FR-2.4; OPS-1; API-11 (#560) | Canary-secret scans of logs, journal, artifacts, API responses, and restored backups; a type test that every connection payload schema has no secret-shaped member; the Alpaca `Debug` test extended to every connector |
| **CN-2** | **No permission that can move funds out.** A connection never holds a scope, key permission, or MCP tool that can withdraw, transfer, or send funds or assets out of the account. A credential that carries one is refused at connect time, before storage, with instructions for a trading-only credential. A live credential whose fund-movement permission cannot be shown absent is refused | HLD §6 A step 2; FR-2.2; infrastructure §5.3 (tightened, DEC-441 item 4) | Per connector: a fixture grant or key with each fund-movement permission is refused before any vault write; an MCP tool list containing a transfer tool is refused; a live key from a venue that cannot report permissions is refused |
| **CN-3** | **Paper and live are distinct connections, and paper never reaches live.** A connection has one environment for life. A paper connection's executor has only paper hosts; a non-production build has no live host at all. A credential that does not work against its own environment is refused, and so is one that also reaches the other environment, until the broker's documentation shows it cannot (U-A4; DEC-441 item 21). The other environment is never probed to find out | Rule 8; V-001; V-031; OPS-5; ES-23; DEC-441 item 21 | Build test that no live host is compiled outside production; a staging egress test; a fixture test that a token documented as reaching both environments is refused for a paper and for a live connection, with no request to the other host |
| **CN-4** | **Agents never reach the broker.** Every account-level action goes through the account's executor and its ledger. The runtime holds no credential and has no route to a broker; a connector is reachable only from the executor | Rule 12; trading §7.1; infrastructure §3.1, §3.5 | Crate layering (`xtask/layers.toml`); a vault policy test that a runtime identity reads no connection; an egress test |
| **CN-5** | **One account, one executor, one connection.** A broker account maps to exactly one `account_ref` and one active connection per environment in a workspace deployment, and its account stream has one writer, enforced by the writer epoch. A second connection to an account already connected is refused; a reconnect reuses the same connection | DEC-26; journal §5.1; OPS-3; DEC-441 item 6 | Two concurrent executors for one account: the older is `Fenced` before it can send; connecting the same account twice, from the same or a second workspace in the deployment, is refused |
| **CN-6** | **Losing a credential stops openings at once and never blocks a risk reduction the broker still accepts.** Expiry, revocation, a failed refresh, or a failed permission check sets the account state `closing_only` (trading §7.3, reason `account_restricted`, cause `connection_unavailable`), which moves every agent on the account to `exits_only`; the executor journals `AccountRestrictionChanged` and `AgentModeApplied` before it acts, in the same step (§9.1). While any credential still works, exits, protective re-placement, and kill-switch orders keep going | Rules 3, 13; trading §1 principle 4 | Fault injection: revoke, expire, and fail refresh at every step of an open, an exit, and a kill switch; `AccountRestrictionChanged` (`closing_only`) is committed before anything else, no opening is sent after it, and every exit the fake broker still accepts is sent |
| **CN-7** | **Activity the platform did not originate is external activity.** An order, fill, or position change on the account that has no `client_order_id` of ours, or that the connector cannot attribute, is ingested as external activity (trading §7.1), never adopted as ours | Trading §7.1, §11; DEC-26 | Reconciliation fixtures per connector with an owner order, an order from another platform, and an unattributable fill |
| **CN-8** | **The allocation boundary is the connected account.** Every request a connector sends names the connection's own account. A connector never reads, stores, or acts on another account of the same customer; data about other accounts that a broker returns anyway is dropped at the connector before it is hashed, stored, or journaled | E7-6; R-25; journal §6.4 | Robinhood fixtures where reads return several accounts: only the agentic account's data reaches the executor or the journal; a request naming another account is `NotSent` |
| **CN-9** | **Broker metadata is data, never instructions.** Tool names, tool descriptions, schemas, error text, and any text field a broker returns never reach a model, a prompt, or a notification, and never change what the connector may call. The connector calls only an allowlist of tools pinned by contract hash | R-05; rule 4; DEC-441 item 8 | A fixture MCP server whose tool descriptions carry injection text and whose tool list adds a tool: the text appears nowhere downstream and the new tool is never called; a changed contract hash halts openings |
| **CN-10** | **Every connection change is journaled before it takes effect,** without secrets: connect, each permission-check result (including refusals), state changes, credential rotation, revocation. Today's schemas carry connect, revoke, and the account state a failure sets; the rest is the journal spec change E7-17, which E7-12 and E7-14 wait on (§3) | Rule 5; FR-2.2 acceptance; journal §9.2 | Fault injection on the append: no state change is acted on without its committed event; refusal events carry no credential |
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
| `account_ref` | Opaque ULID naming the account stream `acct:{workspace_id}:{account_ref}` (journal §2) | Connection record; control stream once E7-17 lands | **No.** `ConnectionEstablished` is closed with four members. Journal spec change needed (E7-17): `account_ref` added to `ConnectionEstablished` as a new `schema_version`, the clause DEC-261 item 10 (Proposed) asks for that binds an account stream to its connection |
| `account_fingerprint` | A keyed hash of (broker, broker account id), §3.1. Detects a second connection to the same account (CN-5) without storing the number in clear | Connection record only | Never journaled, never returned |
| `vault_path` | Derived from workspace id and `connection_id`; never sent to a client | Derived, not stored | Never |
| `state` | §9 | Connection record | Through the account state it sets: `AccountRestrictionChanged` and `AgentModeApplied` (§9.1). A connection-state event of its own waits for E7-17 |
| `contract_hash` | MCP only: hash of the pinned tool contract (§6.2 rule 3) | Connection record | **No**; with the check results, E7-17 |
| `checks` | Latest result of each check in §8, with time | Connection record | **No**; check results and refusals need a journal spec change (E7-17). Until then a failed check acts only through `AccountRestrictionChanged` |
| `terms_version` | Hash of the broker terms text the owner saw at connect, where a broker has platform terms (Robinhood) | Control stream | Through the existing `DisclosureAccepted` (journal §9.2): `document` `robinhood_agentic_terms`, `version` the terms hash, the owner, and step-up. No new event is needed |

**Reconnect (answers §13 question 2).** A reconnect is a second `ConnectionEstablished` for the same
`connection_id`, valid only after that id's `ConnectionRevoked` and only with the same `broker`,
`environment`, and (once journaled) `account_ref`. The journal spec change in E7-17 adds that rule;
until it lands, reconnect is not built (E7-14 depends on E7-17).

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
which broker it talks to. A connector also declares a **capability table** that the trading spec's
order policy (§5.1) is checked against at deployment: an agent whose mandate needs a capability the
connection lacks is not deployable.

| Capability | Alpaca paper (keys, Phase 1) | Alpaca live and paper (OAuth, M8) | Robinhood Agentic (MCP, M8) | Kraken Derivatives US (E16) |
|---|---|---|---|---|
| Authentication | API key and secret, local config outside the repo (ES-19) | OAuth 2 authorization code with the platform's client secret, single-use `state`, `env` always set; PKCE not documented by Alpaca, so sent but never relied on (§5.5) | OAuth through Robinhood login at its MCP server (OD-12) | API key; permissions queried from the venue |
| Paper environment | Yes | Yes | **No.** Paper stage is the simulated broker with Robinhood's rules (DEC-124) | Demo environment |
| Asset classes in v1 | US equities, ETFs, crypto spot (DEC-24) | Same | US equities and crypto spot; options excluded (DEC-24) | Perpetuals (Phase 3) |
| Client order id, idempotent | Yes (`client_order_id`) | Yes | **Unknown (U-R1)** | To confirm |
| Query by client order id | Yes | Yes | **Unknown (U-R2)** | To confirm |
| Limit, stop-limit, GTC | Yes (trading §5.2) | Yes | **Unknown (U-R3)** | To confirm |
| OCO or bracket | Yes | Yes | **Unknown (U-R4)** | Not applicable |
| Account restriction signal | Status flags and rejects (trading §7.3) | Same | **Unknown (U-R6)** | To confirm |
| 1× check (FR-2.6) | `multiplier` | Same: `multiplier` is readable with no extra scope (U-A5, answered, §5.5); changing it needs `account:write`, never requested | **Unknown (U-R7)** | Margin by design; E16 |
| Activities for reconciliation | `/v2/account/activities` | Same | **Unknown (U-R8)** | To confirm |
| Fund-movement permission | None used; the crate has no such endpoint | **Not shown absent (U-A2 open, §5.5)**: live is refused (CN-2); paper records and discloses it | Must be absent from the tool list (CN-2) | Queried per key |
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
(workspace API spec §1.4; DEC-690 item 1): it has no broker host in its egress (infrastructure
§3.1). It takes the owner's input, journals it, and reports. The code exchange and every §8.1
check run in the account executor started for the pending connection, the one process type that
holds a vault lease for one connection and that connection's broker hosts.

1. A workspace admin presents step-up (identity spec #556, ID-4) and starts the connect. The server
   creates a single-use `state` bound to the user, workspace, intended environment, and a PKCE
   verifier, with a short expiry (Proposed default: 10 minutes).
2. The browser goes to Alpaca's authorization page with the platform's client id, the redirect
   URI registered for the deployment, the `state`, the PKCE challenge, **only the scopes in §5.3**,
   and `env` set to the connection's environment, always (DEC-690 item 3: without `env`, Alpaca
   prompts for a live and a paper account together).
3. The callback checks `state` (exists, unexpired, unused, same user). It writes the authorization
   code and the PKCE verifier **straight to the vault**, write-only, under the pending connection's
   path, in the same request; the API process holds them only in a `secrecy` value until the
   vault write commits (CN-1). It then asks for the pending connection's executor to start
   (infrastructure §3.5) and reports `connecting`. It sends nothing to Alpaca.
4. The executor reads the code from its lease and exchanges it at once at Alpaca's token endpoint
   (an authorization code lives 10 minutes, §5.5), authenticating with the platform's client
   secret, which only an executor in `connecting` may read. The token goes to the vault and the
   code is deleted. The executor then runs the §8.1 checks against the granted scopes and the
   account. Any failure deletes the vault entry, journals the refusal without the token (CN-10),
   and stops the executor; the API shows the reason it reads from the journal.
5. On success the executor journals the passing check results (CN-10, E7-17), and the API, as the
   control stream's writer, appends `ConnectionEstablished` (journal §9.2) once it reads them.
   The executor then runs as the account's executor.

**Not buildable yet.** U-A4 is open (§5.5), so DEC-441 item 21 refuses every Alpaca OAuth grant,
paper included. And Alpaca's token endpoint is on its live host (`api.alpaca.markets`), which no
non-production build compiles in (ES-23), so even a paper grant cannot be exchanged outside
production. Both readings that would lift these are the founder's (DEC-690 items 6 and 7).

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
  environment's host to find out. If the answer is that every token reaches both, the refusal
  blocks Alpaca OAuth entirely; the alternative, accepting such a token with the executor bound to
  its own environment's host, no live host outside production, egress limited to that host, and
  the token's breadth journaled and disclosed, is the founder's (DEC-441 item 22).
- **U-A5:** whether an OAuth app may read the 1× setting (trading §15 q3).

### 5.4 Refresh, rotation, and revocation

Alpaca documents no token lifetime, no refresh token, and no revoke call (U-A3, open, §5.5). Until
it does, the executor treats a token as valid until a call fails authorization, never assumes a
refresh exists, and the rules below apply as written.

- If tokens refresh, the executor refreshes ahead of expiry (Proposed: at 80% of lifetime) through
  the vault, never through the API process. A failed refresh is retried with back-off until the
  token expires; at expiry the connection is `suspended` (§9) and CN-6 applies.
- The executor picks up a rotated credential at its next lease renewal without a restart
  (infrastructure §5.4).
- **Revocation by the user at Alpaca** shows as an authorization failure on the next call. Two
  consecutive authorization failures on any request (Proposed) move the connection to `suspended`.
  One is treated as a possible transient: the request's outcome is `Unknown` if it was a submit, and
  is resolved by query once a credential works again (trading §5.7).
- **Revocation through the platform** revokes the token at Alpaca where Alpaca offers a revoke
  call; none is documented (§5.5), so the confirmation tells the owner to remove the app at Alpaca
  too. It deletes the vault entry, and journals `ConnectionRevoked`. It is refused while an agent on
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
| **U-A4** does one token reach both paper and live | [O]: "An single Alpaca OAuth token may authorize access to either: One live account; One paper account; One live account and one paper account". `env`: "If provided, must be one of `live` or `paper`. If not specified, the user will be prompted to authorized both a live and a paper account." And: "If you specify a value for the `env` parameter when redirecting to us, we will ask the user to authorize only a live or a paper account". [O] does **not** say a host refuses a token not authorized for its environment, and the token response names no environment. [F] (staff, 2020) says the opposite of binding: the token "belongs to the user instead of a specific account and can be used for both paper and live accounts (using api.alpaca.markets vs paper-api.alpaca.markets)". [O]'s token endpoint is `POST https://api.alpaca.markets/oauth/token`, the live host, for either environment | **Open.** The documentation describes what the owner authorizes, not what a host enforces, so it does not show a token reaches only its own environment. DEC-441 item 21 keeps refusing every Alpaca OAuth grant. Whether an `env`-scoped grant is enough is the founder's (DEC-690 item 6); so is reaching the live host's token endpoint from a non-production build (item 7) |
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
| `Submit` | The allowlisted order tool for the asset class, with the account id pinned to the agentic account and our client order id if the contract supports one (U-R1) |
| `Cancel` | The allowlisted cancel tool, by broker order id looked up from our record |
| `GetOrderByClientId` | Query by client order id (U-R2); otherwise see §6.6 |
| `ListOpenOrders`, `ListPositions`, `GetAccount`, `ListActivities` | Allowlisted read tools, filtered to the agentic account (CN-8) |
| `CancelAll`, `ClosePosition` | Only if the contract offers an account-scoped equivalent; otherwise the kill switch cancels and sells order by order, which trading §5.5 already does for agent scope |
| `AcknowledgeReplace` | Not applicable unless Robinhood replaces orders itself |

Rules for the MCP client:

1. **Transport.** The streamable HTTP transport to Robinhood's published endpoint only, pinned by
   host; no redirects; TLS verified; no other MCP server is ever configured for a broker connection.
2. **Tool allowlist.** The connector calls only the tools it was built for: order placement and
   cancel for equities and crypto spot, and the reads above. It never calls options, option
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
| Whether one platform may act for many customers | Written answer from Robinhood before E7-6 builds (DEC-441 item 15) | Founder |
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
published documentation or a written answer to the founder.

| ID | Question | If the answer is no or unknown |
|---|---|---|
| U-R1 | Do order tools accept a client order id, and is a retry with the same id idempotent? | **E7-6's order path is not built** (DEC-441 item 10). Journal before acting holds, but crash recovery cannot tell a lost submit from an absent one without it; the founder decides |
| U-R2 | Can an order be queried by that id? | As U-R1 |
| U-R3 | Limit, stop-limit, GTC, and extended-hours flags for equities; limit and stop-limit for crypto | Missing types make the mandate's order policy undeployable on Robinhood |
| U-R4 | OCO or bracket orders, or at least a resting stop-limit | Without OCO, protection needs the founder's decision (DEC-441 item 17). With no resting stop at all, no protected equity mandate can deploy on Robinhood |
| U-R5 | Token lifetime, refresh, and how revocation shows | Treated as short-lived: re-check before every session |
| U-R6 | Account status fields and restriction rejects | Unrecognized status is `blocked` (trading §7.3 row 1; principle 3) |
| U-R7 | Margin and 1× status | Paused and prompted (FR-2.6) |
| U-R8 | Fills, fees, dividends, deposits, and withdrawals as an activity feed | Reconciliation (trading §11) cannot run; not deployable |
| U-R9 | Rate limits | §6.5's conservative default |
| U-R10 | Platform terms: one platform acting for many customers; data use; attribution | DEC-441 item 15 |
| U-R11 | Whether the tool list or the token can move funds out | If yes, refused (CN-2) |
| U-R12 | Whether the session can be restricted to the agentic account | If not, CN-8's filtering is the control, and the owner is told the token can read every account |

## 7. Kraken Derivatives US (later)

Phase 3 (E16). The connector follows HLD §6 A: an API key whose permissions are queried from the
venue at connect, at each executor start, and daily; a key with withdrawal or transfer permission
is refused before storage (E16-1, CN-2). Demo and live are distinct connections (CN-3).
Perpetuals accounting and margin are outside v1 (trading §16). This spec adds nothing else for
Kraken now.

## 8. Permission checks and health checks

### 8.1 At connect, at every executor start, and daily

Each check's result is journaled without the credential (CN-10; infrastructure §5.3). The checks
run in the connection's account executor, never in the API process, which holds no credential and
reaches no broker (workspace API spec §1.4; DEC-690 item 1). At connect, the API learns the results
from the journal and only then appends `ConnectionEstablished` or shows the refusal.

| # | Check | Failure at connect | Failure later |
|---|---|---|---|
| 1 | **Scope:** granted scopes equal the requested set; no fund-movement permission; for MCP, the allowlisted tools are present and no fund-movement tool exists (CN-2) | Refused; vault entry deleted | `suspended`; agents `exits_only`; owner alerted |
| 2 | **Environment:** the credential works against the connection's environment, and the broker's documentation shows it does not reach the other; the other host is never probed (CN-3, U-A4) | Refused | `suspended` |
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

## 9. Lifecycle walk

### 9.1 States

| State | Entered when | Openings | Exits, protection, kill switch | Ends when | Who ends it |
|---|---|---|---|---|---|
| `connecting` | Step-up and connect started | No agent yet | — | Checks pass (`active`) or fail (refused, no record kept beyond the refusal event) | System |
| `active` | All §8.1 checks pass | As the gate allows | Yes | Any transition below | — |
| `degraded` | Network errors, low headroom, or contract drift | **Halted**: account state `closing_only`, agents `exits_only` | Yes, while the broker accepts | Good probes, or for drift a released connector version, **then** the owner's acknowledgment (trading §7.3, cause `connection_unavailable`) | Owner, with step-up |
| `suspended` | Credential invalid: expired, revoked at the broker, refresh failed, or a later permission check failed | **Halted**: account state `closing_only`, agents `exits_only` | Attempted while any call succeeds; otherwise protection rests at the broker | The owner reconnects the same account, then acknowledges (trading §7.3, cause `connection_unavailable`) | Owner, with step-up |
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
unjournaled input to the mode machine. It lifts on the connection's own condition (good probes, a
released connector version for drift, or a reconnect) and then the owner's acknowledgment; an
account refresh is not the condition. Recovery therefore needs the owner even when the cause was
transient. A restriction that lifts with no acknowledgment would be a separate spec-first change
(ES-22).

### 9.2 Walk

| Step | What happens | Invariants |
|---|---|---|
| **Connect** | Step-up; OAuth or key entry; code or key to vault (API); executor started; code exchanged and token to vault, §8.1 checks (executor); `ConnectionEstablished` (API, after the checks' results are journaled); first reconciliation | CN-1, CN-2, CN-3, CN-5, CN-10 |
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
| **Paper deployment pointed at live** | A paper mandate on a live connection, or a token that answers both | V-001; one environment per connection; no live host outside production; a token that reaches both is refused (CN-3, DEC-441 item 21) |
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
| `ConnectionEstablished`, `ConnectionRevoked` payload schemas (journal §9.2) | **Exists** (registered, E7-10); does not yet carry `account_ref`, check results, or connection states (§3, E7-17) |
| Connection manager service, connection record, fingerprint, states | Planned: E7-11 |
| Permission checks (§8.1) and refusal events | Planned: E7-12 |
| Alpaca OAuth | Planned: E7-1 (M8) |
| Health probe, mapped onto `closing_only` (§9.1) | Planned: E7-13 |
| Reconnect reusing the connection | Planned: E7-14 |
| Robinhood contract confirmation | Planned: E7-15 (no code) |
| MCP client with allowlist and pinning | Planned: E7-16 |
| Robinhood connector | Planned: E7-6 (M8), blocked on E7-15 |
| Connection state event schemas | Planned: E7-17 (journal spec change) |
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
refused until U-A4 is answered (item 21).

[DEC-690](../project/decisions/DEC-690.md) records what Alpaca's documentation answers (§5.5).
Accepted (agent, DEC-176): the API never calls a broker, and the code exchange and checks run in the
executor (item 1); U-A1 and U-A5 answered (items 2 and 8); `env` always set (item 3); U-A2 and U-A3
open, with live Alpaca OAuth refused under CN-2 and no refresh assumed (items 4 and 5); PKCE never
relied on (item 9). **Proposed for the founder:** whether an `env`-scoped grant satisfies U-A4
(item 6), and whether a non-production build may reach the live host's token endpoint (item 7).
Until then no Alpaca OAuth grant is accepted in any environment.

**Proposed for the founder** (vendor terms, live accounts, legal text; DEC-79):

| Item | Decision | Recommendation |
|---|---|---|
| 15 | Robinhood platform terms: may one platform act for many customers (OD-12) | Get Robinhood's written answer before E7-6 builds; no Robinhood connection in any environment until then |
| 16 | How Robinhood fixtures are obtained | Synthesized from the published tool contract; any recording from a real account is the founder's own, on the founder's own account, after counsel sign-off (DEC-98) |
| 17 | Robinhood equity protection without OCO or bracket | Accept one resting GTC stop-limit for the whole position, as crypto does (DEC-36), only if confirmed (U-R4); otherwise no protected equity mandates on Robinhood |
| 18 | Alpaca OAuth app registration and its terms | Register at M8 after the founder reads Alpaca's app terms |
| 19 | Verifying the account holder is the workspace user | No name matching in v1; owner attestation with step-up; counsel to confirm |
| 20 | Robinhood customer-responsibility and data-scope disclosures | Counsel drafts the text (compliance question 32) before any Robinhood connection |
| 22 | If every Alpaca token reaches both environments, so item 21 blocks Alpaca OAuth entirely | Accept such a token only with the executor bound to its own environment's host, no live host outside production, egress limited to that host, and the token's breadth journaled and disclosed to the owner |

## 13. Open questions

1. U-A2, U-A3, U-A4, and PKCE (Alpaca; U-A1 and U-A5 answered, §5.5) and U-R1 to U-R12
   (Robinhood), above. The Alpaca ones need Alpaca's written answer (DEC-690 item 10).
2. Answered in §3: a reconnect is a second `ConnectionEstablished` for the same `connection_id`,
   valid only after its `ConnectionRevoked`; the rule is a journal spec change (E7-17).
3. Whether cross-deployment duplicate detection (CN-5) is worth a fingerprint registry in the
   global control plane. A keyed hash of an account id is derived from personal data, so the control
   plane's ban on personal data applies to it (§3.1); it would also add a dependency the trade path
   must not have.
4. Kraken's key-permission query shape, at E16.
