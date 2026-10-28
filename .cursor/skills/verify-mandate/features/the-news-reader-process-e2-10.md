# The news reader process: the news stream's single writer, storing each Alpaca news item (E2-10)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §9.37 (the news stream, rules 283 to
  286, and what the writer owes); DEC-2277 (the paper data key), DEC-2278 (the reader), DEC-2280
  (tickers from the registered instrument snapshots, no raw response kept), DEC-2281 (the followed
  agents).
- **Code:** `mandate-news-reader` (`crates/mandate-news-reader`): `src/key.rs` (the key read once
  from `platform/news-data-key`, refused `no_key`, `vault_unavailable`, `not_paper` or
  `news_key_malformed`), `src/follows.rs` (the followed agents, each checked at start:
  `no_follows`, `follow_malformed`), `src/tickers.rs` (each asset id's ticker from the latest
  `instrument_snapshot` registration on the control stream), `src/watch.rs` (the followed agents'
  active theses, through `mandate-research`'s `replay_theses`, and held instruments, through
  `mandate-gate-context`'s `Journaled::held`), `src/poll.rs` (a round's tickers in chunks the fetcher takes, a page
  cap per chunk, a pause after an idle or failed round). The parsed news item keeps the vendor's
  tickers in `mandate-sources`' `Draft`.
- **Tests:** `crates/mandate-news-reader/tests/key.rs` (the vault's answer decides the key or why
  there is none; no byte of the key in any refusal, error or `Debug` output), `tests/follows.rs` (follows, tickers, the followed folds), `tests/poll.rs`
  (chunks, pages, cap, cooldown); `mandate-sources`' `tests/ingest.rs` (the vendor's tickers kept
  as parsed).
- **Run:** `cargo nextest run -p mandate-news-reader -p mandate-sources`.
