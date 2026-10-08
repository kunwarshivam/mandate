# Task: the first live trade (DEC-529): one founder-run order on the founder's own Robinhood account

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this brief is a **path brief** over the stories below, and each slice it lists is one
story's tests PR or implementation PR under DEC-77. Docs only: it writes no code.

The founder decided on 2026-10-08 that paper trading stays on Alpaca paper
([DEC-502](../decisions/DEC-502.md), [DEC-509](../decisions/DEC-509.md)), and that before
**2026-11-02** they want one real-money order on their own Robinhood agentic account, through the
same production path. [DEC-529](../decisions/DEC-529.md) (Proposed, founder-reserved) sets what
that order is and which rules it narrows. **Until the founder accepts DEC-529, only the slices
marked "now" below are built, and nothing live is compiled or run.**

This path builds on the [first paper trade](first-paper-trade.md): its deployment input from the
control stream, the model host, the CLI's registration and confirmation commands, and the
production cycle. It needs that path's manual run (E2) done first.

## Story

- **Stories on the path:** E7-15 (the contract confirmation, recorded in
  [robinhood-contract.md](robinhood-contract.md)), E7-16 (the MCP client with allowlist and
  pinning), E7-6 (the Robinhood connector, narrowed to equities and to the founder's own
  account), E7-4 (protection: one GTC stop-limit for a Robinhood equity position), and the
  simulated broker with Robinhood's rules (DEC-124). New rows this brief proposes for the
  backlog: **E7-23** (the founder's OAuth login on their own machine), **E7-24** (the live
  binary: the environment gate, the notional cap and the typed confirmation), and **E7-25** (the
  simulated Robinhood server). The coordinator adds them with the claim.
- **Acceptance criteria (DEC-529 item 1):** one limit buy in the regular session on the
  founder's own Robinhood agentic account, long only, on the confirmed mandate's one instrument,
  under the hard notional cap, placed only by the founder running the production binary on their
  own machine after typing a confirmation of the displayed order; the order reaches Robinhood
  only through `ProductionCycle::run` and the Robinhood connector; after it fills, one GTC
  stop-limit covers the whole position; the journal verifies afterwards; and a restart sends no
  second order.
- **PRD / HLD / spec anchors:** PRD FR-2.2, FR-2.4, FR-2.6, FR-3.5, FR-4.3; HLD §6.A (connect),
  §6.B (decision cycle); connections spec §2 (CN-1 to CN-12), §4, §6, §8, §9; trading-domain
  spec §4.2, §4.3, §5.1 to §5.4, §7.1 to §7.3, §9, §11; mandate spec §6.1, V-001; journal spec
  §6.3, §6.4.
- **Decisions that apply:** DEC-36, DEC-77, DEC-79, DEC-124, DEC-155, DEC-176, DEC-441,
  DEC-470, DEC-475, DEC-502, DEC-509, DEC-529, and every decision the paper path applies.

## Invariants first

Every slice keeps all of these. Each is a test (named in the slice that adds it), and each
oracle is shown to fail on a seeded bug before it is trusted.

| # | Invariant |
|---|---|
| LT-1 | **No agent reaches Robinhood.** No test, CI job or agent run opens a connection to any Robinhood host. The default build contains no Robinhood host; only the `live` feature of the one live binary adds the published MCP host to the connector's allowlist. Every test runs against the simulated server on loopback |
| LT-2 | **One door.** An order reaches Robinhood only through `ProductionCycle::run` → executor → the Robinhood connector, called by the live binary after the founder's typed confirmation. No other shipping path holds an MCP transport with the live host |
| LT-3 | **The cap binds twice.** The live binary refuses a mandate whose `max_order_usd` exceeds the `--max-notional-usd` the founder types, and refuses any order whose limit price times quantity plus reserved fees exceeds it, after the gate and before the confirmation prompt |
| LT-4 | **Typed confirmation of exactly what is sent.** The prompt shows instrument, side, type, whole-share quantity, limit price, time in force, session, notional, and the account's last four digits; the confirmation code is bound to the canonical hash of those fields; the request sent is the one whose hash was confirmed. Any difference is a refusal with nothing sent |
| LT-5 | **Journal before acting.** `OrderSubmitted`, carrying the deterministic `ref_id`, commits before the `place_equity_order` call; `review_equity_order` runs before it and any pre-trade alert refuses |
| LT-6 | **No re-send.** After a lost answer the order is `Unknown`; the connector never calls `place_equity_order` again for that intent; recovery is list-and-match (DEC-529 item 4); zero or several matches stay `Unknown` and block the instrument |
| LT-7 | **Only the agentic account.** Every request names the founder-typed account; any data about another account is dropped inside the connector before redaction, hashing or storage (CN-8); a request naming another account is `NotSent` |
| LT-8 | **Allowlist and pinned contract.** The connector calls only the nine tools in [robinhood-contract.md](robinhood-contract.md); a tool list whose allowlisted part hashes differently, or that gains a fund-movement tool, halts openings (CN-2, CN-9) |
| LT-9 | **Credentials stay on the founder's machine** (CN-1, rule 7). The OAuth token exists only in the connector process's memory, is never written to disk, a log, the journal, an artifact or an error, and `Debug` never prints it; a canary-token test scans every output |
| LT-10 | **Protected or flat.** When the run exits, the entry is terminal and any filled quantity is covered by one resting GTC stop-limit for the whole position, or the unprotected interval is journaled and its alert key is in the run's report |
| LT-11 | **No rule relaxed.** Regular-session limit openings outside the auction windows, whole shares, the eligibility floor, the collar, both participation caps, 1× buying power, the day-trading regime, and DEC-470 item 1's flat account all apply |
| LT-12 | **Numbers never through a float.** Every price and quantity from Robinhood is parsed from its decimal string by `mandate-num`; a JSON number is `Unreadable` (ES-23) |
| LT-13 | **Replay never calls Robinhood.** Replay of the run's journal folds to the same state with the connector absent |

## Scope

- **Reference cases that must move from pending to passing:** none named by this path. Every
  passing case stays passing.
- **Invariants touched:** LT-1 to LT-13; CN-1, CN-2, CN-3, CN-4, CN-6, CN-7, CN-8, CN-9; the
  paper path's FT-1 to FT-12 stay true for `mandate-paper`.
- **Crates in scope:** three new crates, `mandate-mcp` (the MCP client, E7-16),
  `mandate-robinhood` (the connector, E7-6) and `mandate-live` (the live binary, E7-24), plus a
  simulated server `mandate-rh-sim` (E7-25, a `tool`-layer crate with a binary for the founder's
  rehearsal); `mandate-executor` (protection by connector capability); `mandate-cli` (a live
  connection record and `live` confirmation for this deployment only).
- **Crates out of scope:** `mandate-paper` (stays paper only), `mandate-alpaca` (used for market
  data, not changed), `mandate-risk`, `mandate-builder`, `mandate-runtime`, the journal crates,
  `web/`, the vault, the connection manager (E7-11).
- **New dependencies:** none external by default. `mandate-mcp` uses what `mandate-alpaca`
  already uses: `reqwest` (no redirects, HTTPS only, rustls with ring), `serde_json` with
  `raw_value`, `tokio`, `secrecy`, `thiserror`; their `docs/dependencies.md` rows gain the new
  crate in "Used by" (a founder-owned file). JSON-RPC 2.0 and the streamable-HTTP framing,
  including server-sent events, are small enough to write by hand. The OAuth slice needs two
  things the workspace lacks as direct dependencies: a cryptographic random source for the PKCE
  verifier and `state` (recommend `getrandom` 0.2, already built through `ring` under `rustls`,
  which needs a new row) and a loopback listener (`tokio`'s `net` feature, a
  feature change, no new crate). Base64url is written by hand against RFC 7636 appendix B's
  vector; URLs use `reqwest::Url`. Any other dependency is a stop condition.
- **Safety-critical:** yes, every code slice (connectors, OAuth, executor, the live gate). Each
  is a DEC-77 tests PR then implementation PR.
- **Size budget:** under 400 non-generated changed lines in safety-critical crates per PR.

## Where each piece lives

| Piece | Crate | Layer | Why there |
|---|---|---|---|
| MCP client: streamable HTTP, JSON-RPC, `initialize`, `tools/list`, `tools/call`, allowlist, contract hash | **new** `mandate-mcp` (impure, safety-critical) | 6 | Below the connector; knows nothing of brokers or orders (connections spec §6.2) |
| OAuth login (PKCE, loopback redirect) | `mandate-mcp`, module `auth` | 6 | The MCP authorization flow belongs to the transport; the token never leaves the process |
| Robinhood connector: `BrokerConnector`, order mapping, `ref_id`, list-and-match, CN-8 filter, state mapping | **new** `mandate-robinhood` (impure, safety-critical) | 7 | Beside `mandate-alpaca`; the executor never learns which broker it talks to |
| Protection by capability: one GTC stop-limit when the connector has no OCO | `mandate-executor` | 6 | Reuses the crypto path's single stop-limit (DEC-36), chosen by the connector's capability table rather than the asset class |
| Live connection record and live confirmation | `mandate-cli` | 7 | The founder's CLI plays the connection manager and deployment manager for this one deployment |
| The live binary: environment gate, cap, typed confirmation, run loop until LT-10 | **new** `mandate-live` (impure, safety-critical) | 9 | Mirrors `mandate-paper`; the only crate with the `live` feature |
| Simulated Robinhood: the published contract over loopback MCP, with Robinhood's rules | **new** `mandate-rh-sim` | tool | DEC-124's paper stage for this path, and E7-16's fixture server |

**Founder review.** The four new crates add `xtask/layers.toml` entries, `CODEOWNERS` lines and
workspace `Cargo.toml` members, and the OAuth slice adds a `docs/dependencies.md` row. Each is a
founder-owned file, named under "Decisions needed" in its PR.

## Robinhood specifics

| Rule | Spec | What the contract gives | Gap, and the slice that closes it |
|---|---|---|---|
| **Idempotency** | Trading §5.3; DEC-441 item 10 | `ref_id`, deduplicated upstream; no query by it | DEC-529 item 4. `ref_id` is a UUID built from the first 16 bytes of SHA-256 (via `mandate-canon`) over a domain-separated intent idempotency key, with RFC 9562's version 8 and variant bits set, so a restart recomputes it from the journal; C1, C3 |
| **Order type** | Rule 12; trading §5.1 | `limit`, `gfd`, `regular_hours` | Whole shares only (decimals are for market orders); C1 |
| **Pre-trade check** | Trading §9 | `review_equity_order` returns buying-power, PDT and halt alerts | Every alert refuses before the place; C1 |
| **Protection** | Trading §5.4; DEC-441 item 17 | `stop_limit` with `gtc`; no OCO placement | One GTC stop-limit for the whole position after a complete fill; P1. A partial fill at the run's bound: the remainder cancelled, then the stop for the filled quantity |
| **Account scope** | CN-8 | Placement rejects non-agentic accounts; reads are not restricted | Filter in the connector; C2 |
| **Account type and regime** | Trading §7.2 names Alpaca only | Not in the contract (U-R7) | Spec change (DEC-529); for this order the founder confirms a cash account or margin disabled; G1 refuses an account `review_equity_order` flags for margin or PDT |
| **Quote** | Trading §4.2: `sip` for live equities | `get_equity_quotes`; `review_equity_order` returns the quote | DEC-529 item 12: Robinhood's quote, cross-checked with Alpaca's IEX quote within the collar; G1 |
| **Bars** | Trading §3.2, §9.6 | `get_equity_historicals` exists but is not allowlisted | Alpaca's daily and IEX minute bars, as on the paper path |
| **Fees** | Trading §6.2 | Not in the contract | `PAPER_ONLY`'s overstated equity figures (paper brief F-1) reserve more than any regulatory fee |
| **Settlement** | Trading §2.2, §8.4: T+1 | Not in the contract | The fold books it; a first buy needs nothing more |
| **Rate limits** | Connections §6.5 | Not published (U-R9) | The conservative token bucket with a reserved exit budget; M1 |
| **Token** | CN-6; U-R5 | Not published | Login at each run; an authorization failure is `closing_only`; the resting stop needs no session; O1 |

## Lifecycle walk

| State | Entered | Blocks | Ends | Who | Session close, midnight, restart, version change |
|---|---|---|---|---|---|
| DEC-529 Proposed | Now | Every live slice (G, L) and any live build | The founder accepts | Founder | — |
| Logged out | Run start | Every call | OAuth login in the founder's browser | Founder | Restart logs out (no token on disk) |
| Logged in | Token in memory | Nothing | Expiry, revocation, process exit | Robinhood, founder | Expiry mid-run: `closing_only`, no opening; the resting stop is unaffected |
| Contract drift | Hash differs at login | Every opening | A released connector version | Agents (code), founder (approval) | Checked at every login |
| Rehearsal | The live binary against `mandate-rh-sim` | Nothing real | Its report | Founder | Its journal is a separate deployment, never the live one |
| Confirmed and deployed (live) | `MandateConfirmed`, `AgentDeployed` with `cli_confirm` under DEC-529 | — | `AgentStopped` or a new version | Founder | As on the paper path |
| Displayed, not confirmed | The binary shows the order | The place call | The founder's typed code, or any other input (refused) | Founder | Restart starts over; nothing was journaled as submitted |
| Intent journaled, not sent | `IntentProposed` | A second cycle | `OrderSubmitted`, or the intent's age | Executor | Restart re-hands nothing past its age |
| Submitted, answer lost | `OrderSubmitted` without an answer | Every order in the instrument | List-and-match finds exactly one; else the founder reconciles | Executor, founder | Next run refused (DEC-470 item 1) |
| Working entry (`gfd`) | Accepted | Another opening | Filled, or cancelled at the run's bound | Executor | `gfd` expires at the close; the binary does not exit with it working |
| Filled, stop not yet placed | Complete fill | Adds | Stop-limit accepted | Executor | Unprotected interval journaled and alerted beyond `max_unprotected_s` |
| Protected | Stop-limit accepted (`gtc`) | Adds | Stop fills, or the founder exits | Robinhood, founder | Rests across sessions with no process running; Robinhood's GTC expiry is unknown, so the founder checks it in the app (open question) |
| Done | Run report | Every further run (account not flat) | — | — | `journal export` and `journal verify` clean |

## Adversary review

| Actor | Attempt | Blocked or disclosed by |
|---|---|---|
| Careless founder | Types a cap larger than intended | The prompt shows the notional and the cap; the mandate's `max_order_usd` must also fit (LT-3) |
| Careless founder | Confirms without reading | The code is bound to the displayed fields' hash; the displayed order is the only one sent (LT-4) |
| Careless founder | Runs on a non-agentic or shared account | Robinhood rejects non-agentic placement; DEC-470 item 1 refuses any existing position or order |
| Careless founder | Runs twice | Refused: the account is no longer flat; no re-send (LT-6) |
| Agent | Calls Robinhood tools in its own session | Rule 8, DEC-441 item 11, DEC-529 item 1; no agent has the founder's login; LT-1's build test |
| Bad model | Outputs `Long` on a thin or expensive instrument | The gate's floors, the collar, the cap |
| Malicious server or poisoned metadata | Adds a transfer tool, changes a schema, injects text | Allowlist, contract hash, metadata reaches no model or notification (LT-8, CN-9) |
| Bad tick | Robinhood's quote jumps | Cross-check with Alpaca's IEX quote within the collar refuses (DEC-529 item 12) |
| Lost answer | The place call times out | `Unknown`, list-and-match, never a re-send (LT-6) |
| Gap through the stop | Price gaps below the stop-limit's limit | Disclosed: a stop-limit may not fill (DEC-529 item 7); the loss is bounded by the cap only if the position is worth zero |
| Credential leak | Token in a log, error or core dump | `secrecy`, no disk write, canary scan (LT-9); core dumps disabled by the binary at start |
| Other accounts' data | `get_accounts` returns every account | Dropped in the connector (LT-7) |

## Cross-spec trace

| Reference | Scope | Units | Outcome | Matches? |
|---|---|---|---|---|
| DEC-441 item 10 ↔ connections §6.6 U-R1, U-R2 ↔ the contract | Robinhood orders | UUID | Idempotent submit and query by id | **No**: no query by `ref_id`; DEC-529 item 4 (founder) |
| Trading §5.4 (OCO or bracket legs for equities) ↔ DEC-441 item 17 ↔ the contract | Robinhood equities | Whole shares | One GTC stop-limit | **No** until the spec change; P1 waits on it |
| Trading §4.2 (`sip` for live equities) ↔ the paper path's `iex` | Live equity quote | USD | Collar and risk mark | **No**: DEC-529 item 12 and a spec change |
| Trading §7.2 (account type, regime) ↔ Robinhood | Account | — | Per-broker facts | **No**: Alpaca only; spec change |
| Mandate §6.1 `cli_confirm` paper only ↔ DEC-155 item 4 | Live confirmation | — | Allowed or refused | **No**: DEC-529 item 3 narrows it for this deployment |
| ES-23 (no `live` feature, vault credentials, signed build) ↔ the live binary | Build | — | — | **No**: DEC-529 item 3 |
| Connections §6.2 rule 5 ↔ ES-23 numbers | Prices, quantities | Decimal strings | Parsed by `mandate-num` | Yes |
| CN-8 ↔ `get_accounts` | Reads | — | Other accounts dropped | Yes, by C2 |
| Journal §6.4 (`pii_refs`) ↔ the account number | Account id | — | By reference | Yes; the founder's CLI holds it outside the journal, mode 0600 |
| DEC-124 (simulated broker as Robinhood's paper stage) ↔ the rehearsal | Founder's run | — | Required before live | Yes, R0 |

## Slices, in order

T is a tests PR (stubs and pending tests that fail on the stubs) and I its implementation PR,
under DEC-77. D is a docs PR; SP a protected spec PR the founder approves. **Now** means it can
start today, in parallel with the paper path, because it touches no crate the paper path is
changing and needs no founder answer.

| # | Story | Kind | Crate | What | After | Now? | Estimate |
|---|---|---|---|---|---|---|---|
| L0 | E7-15 | D | docs | This brief, DEC-529, the contract confirmation | — | this PR | — |
| SP1 | E7-15 | SP | `docs/specs/` | The connections, trading and mandate spec changes DEC-529 names | DEC-529 accepted | no | 150–250 |
| S1 | E7-25 | T, I | **new** `mandate-rh-sim` | Pure rules core: the published equity contract (types, whole shares for limits, `gfd`/`gtc`, regular hours, `agentic_allowed`, `ref_id` dedup, the ten states, cancel refused when terminal, `review` alerts), deterministic, scripted fills and faults | — | **now** | T 250–350, I 250–350 |
| S2 | E7-25 | T, I | `mandate-rh-sim` | Loopback MCP server over S1 (streamable HTTP, `tools/list` with the published schemas, injection-text and extra-tool variants, a lost-answer fault) and a binary for the rehearsal | S1, M1 | **now** (after M1) | T 200–300, I 200–300 |
| M1 | E7-16 | T, I | **new** `mandate-mcp` | Transport: pinned host, HTTPS only (loopback HTTP only in test builds), no redirects, JSON-RPC ids, `Mcp-Session-Id`, JSON and SSE answers, timeouts, the token bucket with a reserved exit budget | — | **now** | T 300–400, I 300–400 |
| M2 | E7-16 | T, I | `mandate-mcp` | Allowlist, canonical contract hash over the allowlisted tools' names and schemas, drift halts, a fund-movement tool refuses, metadata never surfaces (CN-2, CN-9) | M1 | **now** | T 200–300, I 150–250 |
| O1 | E7-23 | T, I | `mandate-mcp` `auth` | OAuth 2.1 authorization code with PKCE (S256) and single-use `state` on a loopback redirect; discovery from the server's published metadata; token in `SecretString` only; canary test (LT-9); `getrandom` row | M1 | **now** | T 250–350, I 250–350 |
| C1 | E7-6 | T, I | **new** `mandate-robinhood` | `Submit` → `review_equity_order` then `place_equity_order` with the derived `ref_id`; `Cancel`; the state mapping; decimal strings | M2, S1 | **now** | T 300–400, I 250–350 |
| C2 | E7-6 | T, I | `mandate-robinhood` | Reads (`get_accounts`, `get_portfolio`, positions, orders, quotes, tradability) filtered to the agentic account (CN-8); the account fingerprint; the capability table (no OCO, no query by client id) | C1 | **now** | T 250–350, I 200–300 |
| C3 | E7-6 | T, I | `mandate-robinhood` | Recovery: list-and-match with pagination and the skew margin; zero or several stay `Unknown`; never a re-send (LT-6) | C1, DEC-529 item 4 accepted | tests now, I after | T 200–300, I 150–250 |
| P1 | E7-4 | T, I | `mandate-executor` | Protection chosen by connector capability: no OCO → one GTC stop-limit for the whole position after a complete fill, reusing DEC-36's path; the equity stop-limit offset from the mandate | SP1 | no | T 200–300, I 200–300 |
| K1 | E7-24 | T, I | `mandate-cli` | `connection establish` for one `live` Robinhood connection (`ConnectionEstablished` with the allowlist as `scopes`; the account number stored outside the journal by reference) and `live` accepted by `version confirm` and `agent deploy` for that connection only, with `cli_confirm` | SP1, the paper path's D2 | no | T 250–350, I 200–300 |
| G1 | E7-24 | T, I | **new** `mandate-live` | The binary: E19-11 input (environment `live`), the `live` feature holding the one host, OAuth login, preflight reads, the quote cross-check, the cap (LT-3), the typed confirmation bound to the order's hash (LT-4), `ProductionCycle::run`; the build test that the default build holds no Robinhood host (LT-1) | C2, O1, K1, the paper path's E1a | no | T 300–400, I 300–400 |
| G2 | E7-24 | T, I | `mandate-live` | After submission: bounded reads until the entry is terminal, the stop-limit placed (LT-10), cancel at the bound, the report; then `journal export` | G1, C3, P1, the paper path's E1b | no | T 250–350, I 200–300 |
| R0 | — | run, then D | — | The founder's rehearsal against `mandate-rh-sim` (DEC-124), then the live run (below); a docs PR records the evidence | G2, the paper path's E2 | no | — |

**Totals.** 26 PRs (one docs, one spec, twelve tests and twelve implementation PRs) and about
5,500 to 8,000 changed lines. Seven pairs (S1, S2, M1, M2, O1, C1, C2) and C3's tests can start
now.

**Critical path.** DEC-529 accepted → SP1 → P1 and K1 → G1 → G2 → R0, with G1 and G2 also
waiting on the paper path's E1a, E1b and E2. The parallel chain M1 → M2 → C1 → C2 → C3 runs
beside the paper path now, with S1, S2 and O1 beside it.

**Estimate.** At the recent pace of two to three review rounds per safety-critical PR, and with
reviews and merges one at a time: the "now" chain is about 14 PRs over **about 8 to 11 working
days** with two to three builders, which ends near the paper path's E2 (about a week from now).
What follows is SP1, P1, K1, G1, G2 and R0, which are serial: **about 6 to 9 more working days**.
That puts the live run at about **2026-10-30 to 2026-11-06**. **2026-11-02 holds only if** the
founder accepts DEC-529 and approves SP1 within about three days, the paper path's E2 lands by
about 2026-10-16, and no slice needs a fourth review round. The most likely slip is the founder's
spec approval and the paper run; the second is OAuth discovery, if Robinhood's server needs
something the published MCP authorization flow does not describe (a stop condition).

### The founder's run (R0)

Only the founder runs it, on their own machine, with their own login; no agent runs any step or
sees its output during the run (`AGENTS.md` rule 8; DEC-529 items 1 and 10).

1. Build `mandate-live` with `--features live` from a commit on `main`, and `mandate-rh-sim`.
2. **Rehearsal (DEC-124).** Start `mandate-rh-sim`; register, create, confirm and deploy a
   rehearsal mandate against a separate journal; run `mandate-live` against the simulated server
   through the whole flow, including the typed confirmation, the fill, the stop-limit, and a
   restart that sends nothing. Then `journal export` and `journal verify`: clean.
3. **Live preparation.** The Robinhood agentic account is flat, holds only what the founder is
   willing to risk, and is a cash account or has margin disabled. Download the instrument's
   bars and `mandate inspect` them; register the configuration with a dated ETP classification;
   `connection establish` the live connection, typing the account number; create, confirm and
   deploy the live mandate with `max_order_usd` within the cap.
4. Between 09:30 and 15:50 New York on a trading day, `mandate-live` without `--place-one-order`:
   it logs in through the browser and reports the order it would place, or the exact refusal. A
   `Flat` model or a gate denial ends here; neither is worked around.
5. `mandate-live --place-one-order --max-notional-usd <cap>`: the binary displays the order;
   the founder types the confirmation; one `place_equity_order`; the run stays until LT-10
   holds.
6. `mandate journal export` for the agent, account and control streams, then `mandate journal
   verify <file> --store <root>` on each: clean.
7. The same command again: refused before any place call because the account holds the position
   (DEC-470 item 1).
8. The founder checks the position and the resting stop-limit in the Robinhood app, and records
   the run's evidence (journal verify output, the report, no account number) in R0's docs PR.

## Decisions

### Founder-reserved (DEC-79), in DEC-529

| # | What | Conservative course meanwhile |
|---|---|---|
| F-1 | DEC-529 itself: one founder-run live order, and the narrowings of rule 8, ES-23, DEC-155 item 4 and V-001 for it | Nothing live is compiled or run; only the "now" slices are built |
| F-2 | The cap (recommended 100 USD) and the instrument: a limit order buys whole shares, so the cap must cover one share of the instrument (DEC-529 item 2) | Neither proposed by agents |
| F-3 | DEC-441 item 10's narrowed reading: `ref_id` without a query by it, list-and-match, never a re-send (DEC-529 item 4) | C3's implementation waits |
| F-4 | DEC-441 items 15, 16, 17 and 20 as DEC-529 reads them | No protected Robinhood equity mandate; no customer connection |
| F-5 | Counsel before a founder-own-account live order (DEC-529 item 9) | No live run |
| F-6 | The live quote source (DEC-529 item 12), or Alpaca's paid SIP feed | No live run |
| F-7 | Founder-owned files: four new crates' `xtask/layers.toml`, `CODEOWNERS` and `Cargo.toml` entries; `docs/dependencies.md` rows for the new crates and `getrandom` | Each PR names it under "Decisions needed" |
| F-8 | SP1, a protected spec change | P1, K1, G1 wait |

### Engineering readings (to be recorded by the slices that need them)

- The `ref_id` derivation (UUID version 8 from SHA-256 of the intent's idempotency key) is a
  reversible engineering choice; C1 records it in a decision under DEC-176.
- The MCP client's framing and timeouts, and the token bucket's figures, are M1's.

## Commands

```bash
cargo nextest run -p mandate-mcp -p mandate-robinhood -p mandate-rh-sim
cargo nextest run -p mandate-executor -p mandate-cli -p mandate-live
cargo nextest run -p <crate> --run-ignored only   # every pending test fails on its stub
cargo check -p mandate-live --features live       # compile only; never run in CI
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci mutants
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo xtask check
```

## Stop conditions

Stop and write a decision rather than continuing if:

- any test, CI job or agent run would open a connection to a Robinhood host, or need a real
  account, a recording, or the founder's credentials;
- the connector would need to re-send an order, match on anything weaker than DEC-529 item 4,
  or adopt an order it cannot attribute (CN-7);
- the OAuth flow needs a client secret, a registered redirect other than loopback, or a token
  stored on disk;
- protection would need an OCO, a bracket, or an unprotected position after the run;
- the order would have to be a market order, a fractional quantity, a dollar amount, or outside
  the regular session;
- the cap or the confirmation could be bypassed by a flag, an environment variable or a
  configuration file;
- a slice needs trading, sizing, pricing or gate logic in `mandate-live`, `mandate-robinhood` or
  `mandate-mcp`;
- a new dependency beyond `getrandom` seems necessary.

## Definition of done

- [ ] DEC-529 accepted by the founder, and SP1 merged.
- [ ] The rehearsal against `mandate-rh-sim` ran clean, restart included.
- [ ] One limit order placed on the founder's own Robinhood agentic account by `mandate-live`
      through `ProductionCycle::run`, inside the regular session, under the cap, after the
      founder's typed confirmation.
- [ ] The filled position is covered by one resting GTC stop-limit (LT-10).
- [ ] `mandate journal verify`, with the artifact store, is clean on every exported stream, and a
      second run sent nothing.
- [ ] No agent called Robinhood or saw the credential; no token, account number or order
      detail appears in the repository or any log that leaves the founder's machine.
- [ ] Every code slice: tests first, zero missed mutants, green CI, and an independent review on
      a different model.
