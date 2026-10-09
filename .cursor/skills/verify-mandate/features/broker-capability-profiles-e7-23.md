# Broker capability profiles (E7-23)

- **Code:** `crates/mandate-domain/src/profile.rs` (`CapabilityProfile`, validated once and hashed
  over DEC-630's canonical object through `mandate-canon`), `BrokerConnector::profile` in
  `crates/mandate-executor/src/ports.rs`, and Alpaca's trading spec §5.2 rows in
  `crates/mandate-alpaca/src/profile.rs` (DEC-531, ADR-0004). B1 does not register the profile as
  configuration: the `broker_profile` kind waits for the journal spec (DEC-630 item 8).
- **Tests:** `crates/mandate-domain/tests/profile.rs` (each refusal, the canonical object and its
  SHA-256 written by hand, every member moving the hash, declaration order never moving it) and
  `crates/mandate-alpaca/tests/profile.rs` (Alpaca's object by hand, §5.2's fractional rule, the
  connector declaring without a broker call).
- **Run:** `cargo nextest run -p mandate-domain -p mandate-alpaca`.
