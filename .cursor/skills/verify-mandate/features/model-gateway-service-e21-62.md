# Model-gateway service (E21-62)

- **Spec:** inference spec v0.6 §1.2, §3.1, §3.6 and §8.2; DEC-2257 (the staging plan: the
  service is the only live caller of a model provider), DEC-2265 (one framed canonical-JSON
  exchange per call), DEC-2020 items 3 and 5 (the provider's key, read once as the model gateway).
- **Code:** `mandate-gatewayd`, `crates/mandate-gatewayd/` (layer 9, safety-critical, a workspace
  service): `src/key.rs` (`HeldKey`, the key read once from the vault and served for its upstream
  alone), `src/start.rs` (`meters_ready`, the live start's gate on both meter streams),
  `src/serve.rs` (`Server`, the serve loop: a thread per connection under socket timeouts and a
  cap, one thread holding the gateway, a log of codes alone), `src/cap.rs` (`DailyCaps`, each
  agent's daily research cap read from the deployed mandate and read again on a version change,
  DEC-2272), `src/service.rs` (`Service`, each research call held to its agent's cap read before
  anything is reserved, and the start's check that every deployed agent's cap reads),
  `src/config.rs` (`Config::from_env`, every start-up value required, none with a default),
  `src/control.rs` (`StoreControl`, the control stream through the journal store),
  `src/recorded.rs` (`Replies` and `settings`, the `--recorded` mode, DEC-2326 item 3),
  `src/run.rs` (`command`, the binary's arguments, a live start refused `live_not_built` in every
  form; `recorded_service`, the recorded service over the journaled meter streams, started only
  when both read and every deployed cap reads; the ids, the host clock, the log and the
  heartbeat);
  `RequestError::ALL`, `RequestError::code` and `Meter::set_research_daily` in `mandate-gateway`.
- **Tests:** `crates/mandate-gatewayd/tests/key.rs` (read once, only as the model gateway, each
  vault failure's code, a value a header cannot carry, and a canary that the key shows in no
  `Debug`, error or code), `tests/start.rs` (both meters must read back, and the one that does not
  is named), `tests/serve.rs` (a call crosses it whole, a stalled peer is cut off and frees its slot,
  a connection past the cap is closed unread, a frame past its bound or malformed is never
  answered, stopping ends the loop, a panicking answer stopping the listener, and the DEC-2265
  item 9 log canary), `tests/cap.rs` (the
  deployed version's own cap, zero with no envelope, deployment or after a stop, each document
  that does not read refused with its code, a document whose hash differs from the journaled
  version refused, a version change read again and a failed read leaving no cap, and any cap read
  as itself or refused above the monthly cap, and the agents deployed in this environment),
  `tests/service.rs` (a research call held to the cap of the version deployed now, a cap that does
  not read refusing `meter_unavailable` with nothing reserved, only this workspace's research calls
  read for, the start refusing unless every deployed agent's cap reads, and what is held bounded by
  the deployed agents), `tests/config.rs` (each value unset, empty or malformed stopping the start
  with its code), `tests/control.rs` (the workspace's own control stream), `tests/recorded.rs` (each call
  primed with its turn's reply, and the recording's settings under DEC-2316's rate caps),
  `tests/run.rs` (the arguments and each refusal's code, the recording read strictly, a call
  reserved and settled on both journaled meter streams and answered with the recording, a foreign
  workspace and a budget below the call's maximum refused with nothing sent, the start refused
  while a meter stream or a deployed cap does not read, the ids, the log and the heartbeat);
  `crates/mandate-gateway/tests/wire.rs` (every refusal's code crosses the wire as itself),
  `crates/mandate-gateway/tests/meter.rs` (a replaced daily cap binds the next reservation, and an
  unread one refuses `Unavailable` reserving nothing).
- **Not yet:** the live start (the registry file, the vault key and HTTPS, refused
  `live_not_built` until the founder names the live model and budgets, DEC-2265 item 10), the §4.3
  identity probe before registration, and the runtime's worker over the service.
- **Run:** `cargo nextest run -p mandate-gatewayd`.
