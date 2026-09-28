# Alpaca market-data latest-quote fixtures: provenance

The latest-quote reads of E7-8 ([DEC-168](../../../../../docs/project/04-decision-log.md#decisions)),
one scenario per directory in the shape `../alpaca-trading/` uses: `requests.txt` (one
`GET <path and query>` per request), `statuses.txt`, and `response-<n>.json`, byte for byte. The
data host sends every price and size as a JSON number; the crate reads the number's own text,
never an `f64` (ADR-0001 ES-23).

`fixtures::the_data_provenance_table_names_every_scenario_once` checks this table against the
directory list.

| Scenario | Provenance | Why it is hand-built |
|---|---|---|
| `quote_crypto` | hand-built | no paper credentials were available to record it; Alpaca's documented `v1beta3` latest-quotes shape, keyed by the pair |
| `quote_crypto_absent` | hand-built | the same shape for a pair with no quote: the pair is missing from `quotes` |
| `quote_equity` | hand-built | no paper credentials were available to record it; Alpaca's documented `v2` latest-quote shape on the `iex` feed |
| `quote_equity_absent` | hand-built | a symbol the data host does not know, answered `404` |
