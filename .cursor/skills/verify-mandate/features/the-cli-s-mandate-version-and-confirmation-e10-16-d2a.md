# The CLI's mandate version and confirmation (E10-16, D2a)

- **Spec:** `docs/specs/journal.md` §9.2 (`MandateVersionCreated`, `MandateConfirmed`);
  `docs/specs/mandate.md` §2.1, §4.1, §6.1 (`cli_confirm`), §9.1, §10; the first paper trade brief
  (D2a); DEC-505, DEC-523, DEC-530.
- **Code:** `crates/mandate-cli/src/version.rs` (`create`, which stores the canonical document and
  its record and commits `MandateVersionCreated`, every envelope path `user_entered`; `confirm`,
  which takes the code bound to the version, checks every V-rule but V-002 and the registered
  instrument snapshots, and commits `MandateConfirmed`; both paper only).
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
