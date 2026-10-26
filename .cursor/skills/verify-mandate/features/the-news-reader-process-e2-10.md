# The news reader process: the news stream's single writer, storing each Alpaca news item (E2-10)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §9.37 (the news stream, rules 283 to
  286, and what the writer owes); DEC-2277 (the paper data key), DEC-2278 (the reader), DEC-2280
  (tickers from the registered instrument snapshots, no raw response kept).
- **Code:** `mandate-news-reader` (`crates/mandate-news-reader`): `src/key.rs` (the key read once,
  refused `no_key`, `vault_unavailable`, `not_paper` or `news_key_malformed`), `src/tickers.rs`
  (each asset id's ticker from the latest `instrument_snapshot` registration on the control
  stream), `src/watch.rs` (the followed agents' active theses, through `mandate-research`'s
  `replay_theses`, and held instruments, through `mandate-gate-context`'s `Journaled::held`),
  `src/records.rs` (`StreamOpened` and `NewsItemStored` as drafts with deterministic ids, timed
  `local`), `src/reader.rs` (`NewsReader`: one writer epoch, the stream folded at start, one page a
  step through `mandate-sources`' fetcher, each new item's typed record stored and journaled, a
  batch in doubt retried as the same bytes, a refused append stopping the reader), and the
  `mandate-news-reader` binary (`run`, and `probe` for `exec` probes; SIGTERM drains). The parsed
  news item keeps the vendor's tickers in `mandate-sources`' `Draft`.
- **Tests:** `crates/mandate-news-reader/tests/reader.rs`: no key or a refused one writes and
  fetches nothing and stays not ready; each new item stored and journaled, with no licensed text;
  known times; one record an item across fetches, restarts and failovers; the polled tickers
  following theses and holdings; unregistered instruments counted; skipped and refused items
  counted; paging and pace; a retried batch and a refused append. `tests/key.rs`: the vault's
  answer and no byte of the key in any output. `tests/properties.rs`: each version stored once and
  known times never going back, against the test's own ledger.
- **Run:** `cargo nextest run -p mandate-news-reader -p mandate-sources -p mandate-store`.
