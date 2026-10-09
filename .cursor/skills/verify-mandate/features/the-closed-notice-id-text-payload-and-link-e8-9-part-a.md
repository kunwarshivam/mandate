# The closed notice: id, text, payload, and link (E8-9, part a)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §2 (NT-1, NT-4, NT-12),
  §3.1, §3.2, §4.2, §4.3; DEC-438 items 1, 2, 10; DEC-710.
- **Code:** `mandate-notify` (layer 1, pure, safety-critical): `crates/mandate-notify/src/payload.rs`
  (`SecureRandom`, `NoticeId::mint`, `Notification`, `payload`, `Origin`, `link`) and
  `crates/mandate-notify/src/kind.rs` (`NoticeKind`, `Class`, `TextKey`).
- **Tests:** `crates/mandate-notify/tests/notice.rs` (minting, NT-4's property),
  `crates/mandate-notify/tests/catalogue.rs` (every §3.2 kind to its class and text key, §4.2's
  texts, NT-12's wording), and `crates/mandate-notify/tests/payload.rs` (the payload, the link,
  the origin); and the `compile_fail` doctests in `crates/mandate-notify/src/lib.rs`
  (no constructor of a notice id from text), live.
- **Run:** `cargo nextest run -p mandate-notify`; `cargo test -p mandate-notify --doc`;
  `cargo xtask ci pending`.
