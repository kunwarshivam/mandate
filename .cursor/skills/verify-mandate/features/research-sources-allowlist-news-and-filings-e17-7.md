# Research sources: the allowlist, news and filings, and their readers (E17-7)

- **Spec:** `docs/specs/data-plane.md` §4 (sources, ingestion, tagging, de-duplication, storage,
  the research query interface), §5.2 (knowledge time), DP-1, DP-2, DP-8, DP-9;
  `docs/specs/agent-harness.md` §6.3 (the `news` and `filings` reads and their caps) and §10.2;
  decision `docs/project/decisions/DEC-1130.md`.
- **Configuration:** `config/sources/allowlist-v<N>.toml`, the vetted-source allowlist (E17-7 holds
  its contents, E2-19), one file a version, never edited once merged. Version 3 is in use: Alpaca
  news is in effect on the founder's approval (DEC-2394), recorded as his with `approved_by` and
  `approved_in`, and counsel has not confirmed its terms (DEC-433 item 20, DEC-2276).
- **Code:** `mandate-sources` (safety-critical, pure): `crates/mandate-sources/src/allowlist.rs`
  (the file, `permit`, the only constructor of a `PermittedUrl`), `crates/mandate-sources/src/ingest.rs`
  (Alpaca news pages and EDGAR filings to drafts, structured fields only),
  `crates/mandate-sources/src/store.rs` (the news and filings stores, append-only, as-of reads),
  `crates/mandate-sources/src/reader.rs` (`SourceReader`, the bounded typed read lane I2 consumes,
  and the E17-5 `observation`), `crates/mandate-sources/src/fetch.rs` (the guarded, paced fetchers
  over a `Transport`), `crates/mandate-sources/src/text.rs` (`UntrustedText`). `mandate-sources-http`:
  `crates/mandate-sources-http/src/lib.rs`, the HTTPS transport, run only in a cell with egress to
  the allowlisted hosts.
- **Tests:** `crates/mandate-sources/tests/allowlist.rs` (the checked-in file, the URL guard and its
  property against a literal-prefix oracle), `crates/mandate-sources/tests/ingest.rs` (recorded
  Alpaca and EDGAR fixtures, knowledge time, versions, copies, story groups, amendments),
  `crates/mandate-sources/tests/store_properties.rs` (DP-1 and DP-2 against an arrival-ledger
  oracle), `crates/mandate-sources/tests/reader.rs` (the harness caps, the allowlist version,
  licensed text withheld, drift observations), `crates/mandate-sources/tests/fetch.rs` (nothing
  unpermitted reaches the transport, pacing, status and size refusals),
  `crates/mandate-sources/tests/injection.rs` with the corpus in
  `crates/mandate-sources/tests/fixtures/injection/` (DP-8: no typed field changes, no tag is added,
  caps hold, a payload in a structured field refuses its item), `crates/mandate-sources/tests/layering.rs`
  (the order path cannot depend on the crate).
- **Run:** `cargo nextest run -p mandate-sources -p mandate-sources-http`; mutants:
  `cargo mutants --in-diff <diff> -p mandate-sources`.
