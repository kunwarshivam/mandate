# Web push relay (E8-14, slice S8b)

- **Spec:** [notifications spec](../../../../docs/specs/notifications.md) §4.6 (the relay's request,
  its 512-byte cap and urgency check, nothing stored, opaque logs), NT-1, NT-2, NT-3 and NT-9;
  DEC-438 item 13, DEC-700 item 3 (the envelope per class), DEC-792 and DEC-722 (the push-service
  allowlist), DEC-724 (the relay's readings), DEC-726 (the deployment's VAPID header, carried
  opaque, shape-checked last, capped at 1 024 octets, and never logged).
- **Code:** `mandate-push-relay` (layer 3, pure, over `mandate-webpush` alone):
  `crates/mandate-push-relay/src/lib.rs` (`relay`, `RelayRequest`, `RelayId`, `Forward`, the
  `PushService` and `RelayLog` traits, `LogEntry`, the closed `RelayError`, and
  `check_authorization`, the header's shape check, run last).
- **Tests:** `crates/mandate-push-relay/src/tests.rs`: a valid request forwarded once as given with
  the push service's status returned; the cap at 512 and 513 and over every length to 2,048; only
  the three class pairs; DEC-792's endpoint table and the 2,048-octet endpoint cap; the relay id's
  shape; an unreachable push service as a refusal after one attempt; nothing carried from one
  request to the next; rule 6's capture of everything the relay passes on, scanned for NT-1's
  canaries; the closed refusal codes; and the crate's dependencies (NT-9). DEC-726: an
  absent or malformed header refused, the 1 024-octet cap, a valid header forwarded byte for byte,
  the writer's own header passing, a canary header never logged, the header checked last, the three
  caps apart, no header of the relay's own, and the request's one new field with no `Debug`.
- **Run:** `cargo nextest run -p mandate-push-relay`; `cargo xtask ci pending`.
