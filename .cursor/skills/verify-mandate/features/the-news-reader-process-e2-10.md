# The news reader process: the news stream's single writer, storing each Alpaca news item (E2-10)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §9.37 (the news stream, rules 283 to
  286, and what the writer owes); DEC-2277 (the paper data key), DEC-2278 (the reader), DEC-2280
  (tickers from the registered instrument snapshots, no raw response kept), DEC-2281 (the followed
  agents).
- **Code:** `mandate-news-reader` (`crates/mandate-news-reader`): `src/key.rs` (the key read once
  from `platform/news-data-key`, refused `no_key`, `vault_unavailable`, `not_paper` or
  `news_key_malformed`).
- **Tests:** `crates/mandate-news-reader/tests/key.rs` (the vault's answer decides the key or why
  there is none; no byte of the key in any refusal, error or `Debug` output).
- **Run:** `cargo nextest run -p mandate-news-reader`.
