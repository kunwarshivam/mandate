# Robinhood tool contract: what the published contract confirms (E7-15)

E7-15 asks for a recorded answer, with its source, to each of the connections spec's U-R1 to
U-R12 ([connections spec §6.6](../../specs/connections.md#66-what-must-be-confirmed-before-building)),
gathered without any call to a real account. This file records what the **published tool
contract** answers: the tool list and tool descriptions Robinhood's MCP server publishes, read as
text. No tool was called, no account was touched, and no credential was used (`AGENTS.md`
rule 8, [DEC-441](../decisions/DEC-441.md) item 11).

The spec is a protected path, so the confirmed values live here until the spec change named at
the end lands. They are the source of Robinhood's capability profile
([DEC-531](../decisions/DEC-531.md)): each "What the connector does" entry below is a profile row
or the shared code's reading of one, never Robinhood-specific code.
[DEC-529](../decisions/DEC-529.md) uses them for the founder's one live order.

**Source.** Robinhood's MCP server tool list as of 2026-10-08, supplied by the coordinator. Each
row below says which tool or parameter the answer comes from. "Confirmed from contract" means
the tool list or a tool's own description says so; it does not mean Robinhood has answered in
writing, and behavior the description does not state stays open.

## The tools

The list includes: `get_accounts`, `get_portfolio`, `get_equity_positions`, `get_equity_quotes`,
`get_equity_tradability`, `get_equity_orders`, `review_equity_order`, `place_equity_order`,
`cancel_equity_order`, `get_equity_tax_lots`, crypto reads and `place_crypto_order` and
`cancel_crypto_order`, option reads and `place_option_order`, watchlist tools, alerts, scans,
`get_earnings_calendar`, SEC filing tools, `get_financials`, `get_equity_historicals`, and index
tools. The descriptions also mention a `get_advanced_orders` listing that includes OCO groups.

### The equity order tools

| Tool | Parameters and stated behavior |
|---|---|
| `place_equity_order` | Real money. Requires an account with `agentic_allowed = true`; others are rejected. `account_number` (required; must come from the user, never defaulted from `get_accounts`), `symbol`, `side` (`buy`, `sell`), `type` (`market`, `limit`, `stop_market`, `stop_limit`), `quantity` (decimal string; decimals only for `market` in `regular_hours`) **or** `dollar_amount` (decimal string; `market` only), `limit_price` (for `limit`, `stop_limit`), `stop_price` (for stop types), `time_in_force` (`gfd` default, `gtc`), `market_hours` (`regular_hours` default, 09:30 to 16:00 ET; `extended_hours`; `all_day_hours`; outside regular hours `limit` only), `tax_lots` (sell only), `ref_id` (idempotency key, a UUID: "Generate once per logical order and re-send on retry — the upstream deduplicates by ref_id. Omitting falls back to a server-generated key") |
| `review_equity_order` | The same parameters without `ref_id`. Simulates the order without placing it; returns the quote and pre-trade alerts (buying power, pattern day trading, halt). Recommended before every place |
| `get_equity_orders` | `account_number` (required), `order_id`, `state` (`new`, `queued`, `confirmed`, `unconfirmed`, `partially_filled`, `filled`, `cancelled`, `rejected`, `failed`, `voided`), `symbol`, `created_at_gte`, `placed_agent` (`user`, `agentic`, `recurring`, `drip`), `cursor` (pagination). Newest first; returns `orders[]` |
| `cancel_equity_order` | `account_number`, `order_id`. May be rejected if the order is already filled or cancelled |

Prices and quantities are decimal strings throughout, which suits ES-23: the connector parses
them with `mandate-num` and never through a float (connections spec §6.2 rule 5).

## U-R1 to U-R12

| ID | Question (spec §6.6) | Status | Answer, and its source | What the connector does |
|---|---|---|---|---|
| U-R1 | Client order id, and is a retry with it idempotent? | **Confirmed in part from contract; the rest needs Robinhood's written answer** | `place_equity_order` takes `ref_id`, a client UUID, and "the upstream deduplicates by ref_id". Not stated: whether a retry with the same `ref_id` returns the original order, returns an error, or something else; how long the deduplication window lasts; whether `ref_id` is scoped to the account or the session | Always sends a `ref_id`, derived deterministically from the journaled intent's idempotency key (never omitted, so never a server-generated key). Never re-sends on its own after a lost answer (DEC-529 item 4) |
| U-R2 | Query by that id? | **Confirmed absent from contract**; whether order records carry `ref_id` **needs Robinhood's written answer** | `get_equity_orders` filters by `order_id`, `state`, `symbol`, `created_at_gte` and `placed_agent`, not `ref_id`. The contract does not list the fields of `orders[]` | List and match (below); `Unknown` when unsure |
| U-R3 | Limit, stop-limit, GTC, extended-hours for equities; crypto types | Equities **confirmed from contract**; crypto **open** (not on this path) | `type` includes `limit` and `stop_limit`; `time_in_force` `gfd` and `gtc`; `market_hours` `regular_hours`, `extended_hours` (limit only), `all_day_hours`. A limit order's quantity is whole shares, since decimals are allowed only for `market` in `regular_hours` | Openings: `limit`, `gfd`, `regular_hours`, whole shares. Protective stop: `stop_limit`, `gtc`. No `market`, `stop_market`, `dollar_amount`, or non-regular session for openings |
| U-R4 | OCO or bracket, or at least a resting stop-limit | Resting stop-limit **confirmed from contract**; OCO placement **confirmed absent from the tool list**; whether OCO can be placed another way **needs Robinhood's written answer** | `stop_limit` with `gtc`. A `get_advanced_orders` listing includes OCO groups, but no tool places one, and no bracket parameter exists | One GTC stop-limit for the whole position after the entry fills completely (DEC-441 item 17, DEC-529 item 7) |
| U-R5 | Token lifetime, refresh, how revocation shows | **Open; needs Robinhood's written answer** | Nothing in the tool contract | Treated as short-lived: the founder logs in at each run; a failed call for authorization reasons sets `closing_only` (CN-6). The resting GTC stop needs no session |
| U-R6 | Account status fields and restriction rejects | **Open in part** | `review_equity_order` returns pre-trade alerts for buying power, pattern day trading and halts. The fields of `get_accounts` and `get_portfolio` are not in the contract; error codes are not published | `review_equity_order` before every place; an alert refuses an opening or an increase before the place, and a sell or protective order is placed with the alert journaled (`AGENTS.md` rule 13). An unrecognized status or error is `blocked` (trading spec §7.3 row 1) |
| U-R7 | Margin and 1× status | **Open; needs Robinhood's written answer** | The contract shows no margin field and no 1× cap | For the founder's order only: the founder attests, with `cli_confirm` when recording the connection, that the agentic account is a cash account or has margin disabled, and the gate's buying power is the lower of cash and the broker's figure (DEC-529 item 11). For customers: paused and prompted (FR-2.6) |
| U-R8 | Fills, fees, dividends, deposits and withdrawals as an activity feed | **Open in part** | Orders (`get_equity_orders` with states), positions (`get_equity_positions`) and tax lots (`get_equity_tax_lots`) exist. No deposit, withdrawal or dividend feed is listed; fill and execution detail fields of `orders[]` are not published | For one order on a flat, dedicated account: reconciliation compares the order's state and filled quantity and the position. Full reconciliation (trading spec §11) for customers waits for an answer |
| U-R9 | Rate limits | **Open; needs Robinhood's written answer** | Not published in the contract | Spec §6.5's conservative token bucket, with a reserved budget for exits and cancels |
| U-R10 | Platform terms: one platform for many customers, data use, attribution | **Needs Robinhood's written answer** (DEC-441 item 15) | Not in the tool contract. `placed_agent` `agentic` shows Robinhood attributes orders to the agent channel | Does not block the founder's own order (DEC-529 item 5); blocks every customer connection |
| U-R11 | Can the tool list or token move funds out? | Tool list **confirmed from contract: no fund-movement tool listed**; the token's scope beyond the tools **needs Robinhood's written answer** | No transfer, withdrawal or send tool appears. The list does carry write tools outside the allowlist (options, option exercise, watchlists, alerts, scans) | Calls only the allowlist below (CN-9); a tool list that gains a transfer tool is refused (CN-2) and changes the contract hash (spec §6.2 rule 3) |
| U-R12 | Can the session be restricted to the agentic account? | **Confirmed from contract: not restricted for reads** | `place_equity_order` rejects any account without `agentic_allowed`; reads take an `account_number`, and `get_accounts` lists the customer's accounts | CN-8 filtering is the control: every request names the founder-typed agentic account; anything about another account is dropped inside the connector before hashing or storage |

### Unknown from the contract, all rows

Rate limits; error codes; the retry behaviour of `ref_id` (the upstream deduplicates by it, but
whether a retry returns the original order, an error, or something else is not stated); the
output shape of every tool, including the fields of `orders[]`, fill and execution detail, and
whether `ref_id` is echoed; how long a `gtc` order rests before Robinhood expires it; OAuth token
lifetime and scopes; and the account and session status values.

**Response shapes are assumed.** The contract publishes inputs, not outputs, so every parser the
connector has is written against an assumed shape. The rule for that (the brief's LT-6): an
unparsable or unexpected answer to a place call is `Unknown`, never treated as rejected and
never re-sent; an unexpected answer to a read refuses (rule 3). The first real check of the
shapes is the founder's run with login and reads and no place call (the brief's R0 step 4).

## Crash recovery without a query by `ref_id` (DEC-441 item 10)

The executor journals `OrderSubmitted` before the place call (journal before acting). If the
process dies, or the answer is lost, between that commit and a readable answer, the order is
`Unknown` and recovery must decide whether Robinhood holds it.

**What recovery can do with this contract:**

1. **Re-send the same `ref_id`.** The contract says the upstream deduplicates by `ref_id`, so a
   re-send should not create a second order. But it does not say what the re-send returns, so
   an error answer cannot be told apart from "the first never arrived and this one failed", and
   a re-sent order is placed at the original limit even if the intent has since expired or the
   session has ended.
2. **List and match.** `get_equity_orders` with `symbol`, `placed_agent` `agentic` and
   `created_at_gte` the intent's `OrderSubmitted` time less a fixed margin for clock skew, all
   pages. A record that matches side, type, quantity, limit price and time in force is the order,
   if it is the only one. If `orders[]` turns out to echo `ref_id`, the match is exact.

**What it cannot do:**

- Prove an order is absent. A list that is empty may be a lag in Robinhood's records, so zero
  matches is not "never placed".
- Tell two identical orders apart without `ref_id` in the records.
- Know whether a re-send's error means "duplicate" or "rejected".

**Recommended reading** (DEC-176: when unsure, the reading that adds no risk; DEC-529 item 4):
the connector never re-sends on its own. It lists and matches; exactly one match is adopted, and
its state is followed by `order_id` from then on. Zero or several matches leave the order
`Unknown`, which blocks every new order in that instrument (trading spec §5.3 rule 9) until the
founder reconciles it by hand. Exits and protective orders in that instrument may also be held by
the `Unknown` order (`AGENTS.md` rule 13). The next run is refused while the account holds any
order or position (DEC-470 item 1). This is safe only on a dedicated, flat account with at most
one agentic order; customers need Robinhood's written answer on U-R1 and U-R2.

## Order states

| Robinhood `state` | Executor reading |
|---|---|
| `new`, `queued`, `confirmed` | Accepted, working |
| `unconfirmed` | Working, not yet confirmed: blocks the instrument as a working order; never treated as absent |
| `partially_filled` | Partly filled, working |
| `filled` | Filled |
| `cancelled` | Cancelled (with any filled quantity) |
| `rejected`, `failed` | Rejected; re-read once by `order_id` before the intent is closed |
| `voided` | Cancelled by the broker; re-read once by `order_id` |
| Any other value | `Unknown` (connections spec U-R6: unrecognized is `blocked`) |

## The allowlist

The connector calls only these tools, and pins the contract hash of exactly these (spec §6.2
rules 2 and 3): `get_accounts`, `get_portfolio`, `get_equity_positions`, `get_equity_quotes`,
`get_equity_tradability`, `get_equity_orders`, `review_equity_order`, `place_equity_order`,
`cancel_equity_order`. `get_accounts` is read only to check that the founder-typed account is
`agentic_allowed`; every other account in its answer is dropped (CN-8). Crypto, options, option
exercise, watchlists, alerts, scans, filings, historicals and index tools are never called.

## The spec change this needs

A protected-path PR, approved by the founder, that moves this file's answers into the
connections spec: the Robinhood column of §4 (client order id **yes, `ref_id`**; query by it
**no**; limit, stop-limit, GTC **yes**; OCO or bracket **no**; the other rows as above), §6.2's
`GetOrderByClientId` mapped to list-and-match, and §6.6's rows marked with their status and this
source. E7-15 stays open until the rows that need Robinhood's written answer have one.
