# Model-gateway service (E21-62)

- **Spec:** inference spec v0.6 §1.2, §3.1, §3.6 and §8.2; DEC-2257 (the staging plan: the
  service is the only live caller of a model provider), DEC-2265 (one framed canonical-JSON
  exchange per call), DEC-2020 items 3 and 5 (the provider's key, read once as the model gateway).
- **Code:** `mandate-gatewayd`, `crates/mandate-gatewayd/` (layer 9, safety-critical, a workspace
  service): `src/key.rs` (`HeldKey`, the key read once from the vault and served for its upstream
  alone), `src/start.rs` (`meters_ready`, the live start's gate on both meter streams),
  `src/cap.rs` (`DailyCaps`, each agent's daily research cap read from the deployed mandate and
  read again on a version change, DEC-2268).
- **Tests:** `crates/mandate-gatewayd/tests/key.rs` (read once, only as the model gateway, each
  vault failure's code, a value a header cannot carry, and a canary that the key shows in no
  `Debug`, error or code), `tests/start.rs` (both meters must read back, and the one that does not
  is named), `tests/cap.rs` (the deployed version's own cap, zero with no envelope, deployment or
  after a stop, each document that does not read refused with its code, a version change read
  again and a failed read leaving no cap, and any cap read as itself or refused above the
  monthly cap).
- **Not yet:** the serve loop and its log canary, the binary with its lifecycle and probes and the
  cap wired into its calls, the journaled deployment meter, the §4.3 identity probe before
  registration, and the runtime's worker over the service.
- **Run:** `cargo nextest run -p mandate-gatewayd`.
