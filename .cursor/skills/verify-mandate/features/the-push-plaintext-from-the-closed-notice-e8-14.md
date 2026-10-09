# The push plaintext from the closed notice (E8-14)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §3.2 (the catalogue and
  each kind's text key), §4.2 (the payload), §4.3 (the link), §4.6 (the 512-byte cap), §9 (a forged
  push shows only the generic text and a link to the fixed origin), NT-1, NT-4; DEC-790 items 2
  and 7, DEC-713 (the placement).
- **Code:** `mandate-webpush` (layer 2, over `mandate-notify`): `PushPlaintext::of` in
  `crates/mandate-webpush/src/payload.rs`, the only public source of a `PushPlaintext`, from the
  closed `mandate_notify::Notification`, which maps the closed `TextKey` to `PushText` with an
  exhaustive `match`.
- **Tests:** `crates/mandate-webpush/src/tests.rs`, module `bridge`: every NT-1 canary, an address
  and a name planted as a notice id's bits never reach the plaintext, which is exactly
  `{"notice","text"}` with no kind or class; every kind of spec §3.2 that pushes fits one
  128-octet record and a 230-octet body under the relay's 512, with its row's text key, and the
  five pull-only `info` kinds push nothing; the same notification gives the same bytes; and the
  plaintext holds no link, while its id rebuilds the fixed origin's `/n/` link.
- **Run:** `cargo nextest run -p mandate-webpush`.
