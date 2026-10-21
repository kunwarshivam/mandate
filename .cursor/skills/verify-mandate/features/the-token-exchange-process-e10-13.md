# The token-exchange process (E10-13)

- **Spec:** `docs/specs/connections.md` §5.2 step 4; DEC-821 item 2, DEC-2627, DEC-2628,
  DEC-2629.
- **Code:** `mandate-token-exchange`, the one process with the client secret and the live token
  endpoint; no crate may depend on it (`xtask/layers.toml`). `crates/mandate-token-exchange/src/`:
  the token request's form body, the reader of the token endpoint's answer, and (debug builds
  only) a dev or CI cell's token URL and the HTTP adapter of `TokenEndpoint`.
- **Tests:** `crates/mandate-token-exchange/src/tests/`.
- **Run:** `cargo nextest run -p mandate-token-exchange`.
