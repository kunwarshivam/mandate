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
  cap per chunk, a pause after an idle or failed round, a round from `POLL_OVERLAP_S` before the
  last known time, DEC-2290), `src/records.rs` (`StreamOpened` and
  `NewsItemStored` as drafts with deterministic ids, timed `local`), `src/reader.rs`
  (`NewsReader`: one writer epoch, the stream folded at start, one page a step; DEC-2289's
  `Clock`, a round held past `RECEIPT_LEAD_S` of the trusted time, counted and alerted
  `receipt_ahead` once an episode, every pause by the trusted time), `src/items.rs`
  (each new item's typed record stored and journaled, a batch in doubt retried as the same bytes,
  a refused append stopping the reader), `src/recorded.rs` (DEC-2085: `RecordedNews`, a recording of the vendor's
  items answered one page per request by the tickers asked, reaching no host; `recorded_key`, the
  paper fixture key; `admitted`, `dev` and `ci` alone), and the `mandate-news-reader` binary
  (`run` over Postgres and the HTTPS transport, `--recorded` over Postgres on the recording, or
  `--fixture` over an in-memory journal with no key; `probe` for `exec`
  probes; the not-ready reason once on standard error as a code; SIGTERM drains). The chart runs
  it as `news-reader` in staging alone (DEC-2084): with `images.news-reader` and `news.follows`,
  as `mandate_news_reader`, with its AppRole copied into its vault directory, the allowlist v3
  ConfigMap `news-reader-allowlist`, the artifact store read-write, and egress to Postgres, the
  vault and `data.alpaca.markets` alone; release ships it and promotion holds it. The parsed news item keeps the vendor's
  tickers in `mandate-sources`' `Draft`.
  The agent runtime offers the stored items as event triggers (DEC-2288):
  `mandate-runtime`'s `RetrievedObservation::stored` (the offer from a row and its record, `data_ref`
  the row's `item_ref` as it is, no text; `news_record_unreadable`, `news_record_mismatch`), and
  `mandate-daemon`'s `CellSource` (the news stream read under news triggers, items known by the risk
  clock) and `merge_offers` (feed and stream once each, in arrival order), whose pass alerts each
  mismatch once and unreadable records once. `fixtures/news/` holds the reader's stored bytes both
  sides read.
- **Tests:** `crates/mandate-news-reader/tests/key.rs` (the vault's answer decides the key or why
  there is none; no byte of the key in any refusal, error or `Debug` output;
  one read, then one end of the session), `tests/vault.rs` (the
  login is the news reader's own mounted AppRole credentials, and without them or the CA no vault
  is reached), `tests/recorded.rs` (the recording's pages, its refusal without the key and of
  a malformed file, and each recorded item about a polled ticker journaled once over many rounds),
  `tests/process.rs` (a start it cannot make refused with its code; a recorded start at `dev` and
  `ci` alone; no key or no
  follows: its code, live, not ready, and a drain on SIGTERM), `tests/follows.rs` (follows, tickers, the followed folds), `tests/poll.rs`
  (chunks, pages, cap, cooldown), `tests/start.rs` (not ready and why, paper only, the epoch and
  the fold at start), `tests/rounds.rs` (what is asked as theses and holdings change, counted
  gaps, chunking, the page cap and the pause end to end), `tests/reader.rs` (items stored and
  journaled once, known times, skips, pages and pace, a retried batch and a refused append),
  `tests/properties.rs` (each version stored once and known times never going back, against the
  test's own ledger); `tests/receipt_lead.rs` (a round past the lead held, counted and alerted once an episode, the
  cooldown by the trusted time, the trusted time's arithmetic, and a fuzz against the journal's own
  rows); `tests/stored_records.rs` (the reader stores exactly `fixtures/news/`'s rows and
  records); `mandate-runtime`'s `tests/stored_news.rs` (the offer, each mismatch and unreadable case,
  no text) and `tests/event_trigger.rs` (a stored item starts one `monitor` run naming its
  `item_ref`); `mandate-daemon`'s `tests/source.rs` (the stream read under news triggers alone, an
  unreadable stream, an item known later, refusals by event id), `tests/news_offers.rs` (the merge
  against its own oracle, fuzzed) and `tests/research.rs` (one run per stored item, and the
  refusals' alerts); `mandate-sources`' `tests/ingest.rs` (the vendor's tickers kept
  as parsed); `deploy/tests/test_cell.py`: the reader's staging launch, role, AppRole, allowlist,
  store and egress, and no render outside staging, outside paper, or with no followed agent.
- **Run:** `cargo nextest run -p mandate-news-reader -p mandate-sources -p mandate-store -p mandate-runtime -p mandate-daemon`; the
  chart's with `uv run --directory python pytest ../deploy/tests` (helm required).
