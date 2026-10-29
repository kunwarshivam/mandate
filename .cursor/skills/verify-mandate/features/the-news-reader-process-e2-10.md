# The news reader process: the news stream's single writer, storing each Alpaca news item (E2-10)

- **Spec:** [journal spec](../../../../docs/specs/journal.md) §9.37 (the news stream, rules 283 to
  286, and what the writer owes); DEC-2277 (the paper data key), DEC-2278 (the reader), DEC-2280
  (tickers from the registered instrument snapshots, no raw response kept), DEC-2281 (the followed
  agents).
- **Code:** `mandate-news-reader` (`crates/mandate-news-reader`): `src/key.rs` (the key read once
  from `platform/news-data-key`, refused `no_key`, `vault_unavailable`, `not_paper` or
  `news_key_malformed`; `vault_key` logs in by AppRole as the news reader with the role and secret
  ids mounted beside the vault's CA; `read_then_end` reads once and then ends the session, whatever
  the read answered, refusing `vault_unavailable` for a session not ended), `src/follows.rs` (the followed agents, each checked at start:
  `no_follows`, `follow_malformed`), `src/tickers.rs` (each asset id's ticker from the latest
  `instrument_snapshot` registration on the control stream), `src/watch.rs` (the followed agents'
  active theses, through `mandate-research`'s `replay_theses`, and held instruments, through
  `mandate-gate-context`'s `Journaled::held`), `src/poll.rs` (a round's tickers in chunks the fetcher takes, a page
  cap per chunk, a pause after an idle or failed round), `src/records.rs` (`StreamOpened` and
  `NewsItemStored` as drafts with deterministic ids, timed `local`), `src/reader.rs`
  (`NewsReader`: one writer epoch, the stream folded at start, one page a step), `src/items.rs`
  (each new item's typed record stored and journaled, a batch in doubt retried as the same bytes,
  a refused append stopping the reader), and the `mandate-news-reader` binary (`run` over Postgres
  and the HTTPS transport, or `--fixture` over an in-memory journal with no key; `probe` for `exec`
  probes; the not-ready reason once on standard error as a code; SIGTERM drains). The parsed news item keeps the vendor's
  tickers in `mandate-sources`' `Draft`.
- **Tests:** `crates/mandate-news-reader/tests/key.rs` (the vault's answer decides the key or why
  there is none; no byte of the key in any refusal, error or `Debug` output;
  one read, then one end of the session), `tests/vault.rs` (the
  login is the news reader's own mounted AppRole credentials, and without them or the CA no vault
  is reached), `tests/process.rs` (a start it cannot make refused with its code; no key or no
  follows: its code, live, not ready, and a drain on SIGTERM), `tests/follows.rs` (follows, tickers, the followed folds), `tests/poll.rs`
  (chunks, pages, cap, cooldown), `tests/start.rs` (not ready and why, paper only, the epoch and
  the fold at start), `tests/rounds.rs` (what is asked as theses and holdings change, counted
  gaps, chunking, the page cap and the pause end to end), `tests/reader.rs` (items stored and
  journaled once, known times, skips, pages and pace, a retried batch and a refused append),
  `tests/properties.rs` (each version stored once and known times never going back, against the
  test's own ledger); `mandate-sources`' `tests/ingest.rs` (the vendor's tickers kept
  as parsed).
- **Run:** `cargo nextest run -p mandate-news-reader -p mandate-sources -p mandate-store`.
