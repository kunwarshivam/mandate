# The CLI's mandate version and confirmation (E10-16, D2a)

- **Spec:** `docs/specs/journal.md` §9.2 (`MandateVersionCreated`, `MandateConfirmed`);
  `docs/specs/mandate.md` §2.1, §4.1, §6.1 (`cli_confirm`), §9.1, §10; the first paper trade brief
  (D2a); DEC-505, DEC-523, DEC-530.
- **Code:** `crates/mandate-cli/src/version.rs` (`create`, which stores the canonical document and
  its record and commits `MandateVersionCreated`, every envelope path `user_entered`; `confirm`,
  which takes the code bound to the version, checks every V-rule but V-002 and the registered
  instrument snapshots, and commits `MandateConfirmed`; both paper only, but for K1b's live admission below).
- **Tests:** `crates/mandate-cli/tests/version.rs`: payloads and records written
  out from the vectors' shapes and read back through `Draft::parse`; the stream folded with
  `JournaledFact::from_record` and `ValidationContext::from_journal`, leaving only V-001 and V-002;
  every refusal code, each writing nothing; a store failing at each write committing nothing. The SPY mandate is
  `crates/mandate-cli/tests/fixtures/spy_mandate.json`.
- **Run:** `cargo nextest run -p mandate-cli --test version`.
- **`agent deploy` (D2b, DEC-530 item 9):** `crates/mandate-cli/src/deploy.rs` (`deploy`, which
  takes the stream's latest confirmed version and a code bound to the agent and the version, and
  commits `AgentDeployed` with `config_refs.mandate_version`; one active deployment per agent),
  on `version.rs`'s checks; `crates/mandate-cli/tests/deploy.rs`, over a control stream seeded in
  §9.2's shapes: the exact payload, record and envelope; the fold reading the agent's version in
  force; every refusal code, each writing nothing; a failing store committing nothing.
  `cargo nextest run -p mandate-cli --test deploy`.
- **The commands (D2c, DEC-530 item 1):** `crates/mandate-cli/src/gestures.rs` (`version create`,
  `version confirm` and `agent deploy` over DEC-527's owner flags and P0's target; without
  `--code` a gesture prints `code …` and `warnings …` from the read-only `confirmation` or
  `deployment` and commits nothing), stubbed, and `main`; `crates/mandate-cli/tests/gestures.rs`,
  pending E10-16 but for the flag test: the binary's refusals without a database, each exiting
  non-zero with its code on stderr, nothing on stdout, and no part of the DSN; the read-only shown
  codes; and the binary against Postgres, its refusals (`version_unconfirmed`, `version_unknown`,
  `code_mismatch`) committing nothing.
  `cargo nextest run -p mandate-cli --test gestures`; `cargo xtask ci postgres` for the binary.
- **Live admission (K1b, E7-11; DEC-529 item 3, DEC-925):** `crates/mandate-cli/src/version.rs`
  (`admitted`, which every gesture and both show-code reads call once the document is read: a
  `paper` owner takes only a `paper` document, `mandate_not_paper` otherwise; a `live` owner only a
  `live` document on a connection the control stream last records as the owner's own Robinhood
  `live` establishment with a margin attestation and `cli_confirm`, `paper_only` otherwise, and
  only while no `FillApplied` or `LateFillApplied` is on the account stream it binds,
  `connection_spent` otherwise) and `gestures.rs` (`--environment`, `paper` by default);
  `crates/mandate-cli/tests/live.rs`: the one admitted path committed in `live`, every other live
  combination refused with its code and writing nothing, the flag, and a property with an
  independent oracle that no other live mandate is ever admitted.
  `cargo nextest run -p mandate-cli --test live`.
