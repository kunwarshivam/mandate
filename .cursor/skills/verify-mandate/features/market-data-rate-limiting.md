# Market-data rate limiting

- **Spec:** `docs/project/tasks/marketdata-rate-limit.md` (claim #115); ADR-0001 ES-05, ES-19.
- **Code:** `crates/mandate-marketdata/src/rate.rs` (header budget, in-flight count, token-bucket
  floor, 429 window), `Client::get_with_retry` in `crates/mandate-marketdata/src/client.rs`, and
  `rate_headers` in `crates/mandate-marketdata/src/http.rs`.
- **Tests:** `crates/mandate-marketdata/tests/rate_limit.rs` (scripted responses and fake
  fixed-window and token-bucket hosts on an injected clock).
- **Run:** `cargo nextest run -p mandate-marketdata --test rate_limit --test client`.
