# Alpaca paper trading fixtures: provenance

Each scenario directory is either **recorded** against the Alpaca paper trading host by
`../record.sh` (credentials only as headers, the account's `account_number` and `id` replaced by
opaque `pii:` references before the bytes were saved), or **hand-built** from the trading-domain
spec in the shape a recording lands in. `fixtures::the_provenance_table_names_every_scenario_once`
checks this table against the directory list, so a scenario cannot be added, dropped, or
reclassified without this file changing with it.

Hand-built scenarios reuse the recorded paper order ids and client order ids where they describe
the same order (for example `md-e144b97773a6f87c1978cc2831`), so a test can follow one order across
a recorded and a hand-built scenario. A reused id is not a recording: the provenance is this
table's, not the id's.

| Scenario | Provenance | Why it is hand-built |
|---|---|---|
| `account_active` | recorded | |
| `account_blocked` | hand-built | needs a blocked account, which no client call can provoke |
| `activities_fills` | recorded | |
| `cancel_all_account_scope` | hand-built | the coordinator's ruling: cancel-all is never exercised against the shared paper account (`AGENTS.md` rule 13's blast radius) |
| `cancel_confirmed` | recorded | |
| `cancel_rejected_already_filled` | hand-built | needs an order that filled before its cancel, so a filled position |
| `close_position_account_scope` | hand-built | the coordinator's ruling: close-position is never exercised against the shared paper account |
| `late_fill_after_terminal` | hand-built | needs a fill after a terminal state, so a filled position |
| `open_orders_page` | recorded | |
| `order_by_client_id_absent` | recorded | |
| `order_by_client_id_found` | recorded | |
| `partial_then_filled` | hand-built | needs a filled position |
| `positions` | recorded | |
| `replace_pending_then_replaced` | hand-built | a broker-initiated replace, which no client can provoke |
| `status_unrecognised` | hand-built | a status outside §5.7's table, which the broker does not send on request |
| `submit_bracket_accepted` | recorded | |
| `submit_crypto_stop_limit` | hand-built | protects a filled crypto position |
| `submit_duplicate_client_order_id` | recorded | |
| `submit_limit_accepted` | recorded | |
| `submit_oco_accepted` | hand-built | protects a filled equity position |
| `submit_rejected` | recorded | |
| `submit_timeout_then_absent` | hand-built | a transport failure has no body; the query halves are the recorded `order_by_client_id_absent` shape |
| `submit_timeout_then_found` | hand-built | a transport failure has no body; the query half is the recorded `order_by_client_id_found` shape |
