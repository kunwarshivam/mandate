- **E21-30 (Should, M6, debt; D1)** The runtime decides what the four account-stream records it
  still refuses mean to it: `ConnectionStateChanged`, `ConnectionCredentialRefreshed`,
  `ConnectionCredentialRotated` and `OwnerAcknowledgmentRequested`. The journal admits each on the
  account stream, no writer emits it there yet, and the runtime's fold refuses it as not
  interpreted (DEC-85), which stops every runtime following that account the moment one is written.
  Whether a suspended connection or a rotated credential should hold the runtime's openings, or
  whether each is inert because the executor's binding gate enforces it, is the runtime owner's
  call. *Accepted when:* each has an arm, interpreted or named inert, before any writer emits it on
  an account stream, and `mandate-runtime/tests/account_copies.rs`'s `REFUSED_UNTIL_INTERPRETED`
  is empty.
