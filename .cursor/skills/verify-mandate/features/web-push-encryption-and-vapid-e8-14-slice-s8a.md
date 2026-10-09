# Web push encryption and VAPID (E8-14, slice S8a)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §4.2 (the closed payload),
  §4.6 (web push and the relay's 512-byte cap), NT-1 and NT-3; DEC-438 items 1 and 13, DEC-700
  item 3 (the envelope per class), DEC-790. RFC 8291, RFC 8188, RFC 8292.
- **Code:** `mandate-webpush` (layer 0, pure, no workspace dependency):
  `crates/mandate-webpush/src/lib.rs` (`build_request`, `encrypt`, `vapid_authorization`,
  `Subscription`, `PushEndpoint`, `VapidSubject`, the `SecureRandom` and `VapidSigner` traits, and
  `NoticeClass`'s fixed urgency and TTL) and `crates/mandate-webpush/src/payload.rs` (the closed
  `PushPlaintext` and `PushText`), stubbed but for the closed tables.
- **Tests:** `crates/mandate-webpush/src/tests.rs`, with the published vectors and their fixtures in
  `crates/mandate-webpush/tests/vectors/mod.rs` (DEC-794): the RFC 8291 §5 and RFC 8188 §3.1 vectors
  byte-exact, a receiver-side decrypt oracle, an ES256 check of the VAPID token against the
  signer's public key, the size cap, refusals, and properties for the opaque payload and the
  token's expiry, pending E8-14 but for the live closed-tables test.
- **Run:** `cargo nextest run -p mandate-webpush`; `cargo xtask ci pending`.
