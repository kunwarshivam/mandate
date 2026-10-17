# Model gateway (E15-6 to E15-10)

- **Spec:** inference spec §2 (INF-1 to INF-16), §3 (the call contract), §4 (the registry), §7.3
  (caps), §8.2 and §8.3 (egress and the prompt guard); journal spec §9.19 (the agent stream's
  `ModelInvocationRecorded` and the meter stream, rules 146 to 158); DEC-432, DEC-998, DEC-997
  (OpenRouter's routing lock and the $50 a month workspace cap, Proposed).
- **Code:** `mandate-gateway`, `crates/mandate-gateway/` (layer 1, safety-critical, pure):
  `src/call.rs` (`Gateway::call`, the order of refusals, attempts, settlement, the exact cache),
  `src/registry.rs` (pinned content, the content hash, endpoints and status outside it, the alias
  and probe checks), `src/meter.rs` (reservations, settlements, rule 158, the fold, `Caps`),
  `src/policy.rs` (`EndpointPolicy`, local-only by default, `EgressAllowlist`), `src/guard.rs` (the
  prompt guard), `src/schema.rs` (the output schema language), `src/openrouter.rs` (the routing
  lock), `src/fake.rs` (the recorded-response transport), `src/money.rs` (`Usd6`, prices),
  `src/provider.rs` (the clock, transport, adapter, secret-source and id seams), `src/record.rs`
  (the journal payload). The HTTPS transport is `mandate-gateway-http`,
  `crates/mandate-gateway-http/` (layer 7, safety-critical, impure; DEC-2200): `src/lib.rs`
  (`HttpsTransport`, `url_for`, `budget`, `retry_after_ms`, `SystemClock`).
- **Tests:** `crates/mandate-gateway/tests/call.rs` (every outcome of §3.3, the request body's
  routing lock and absent capabilities, late responses, retries, the cache, withdrawal),
  `tests/registry.rs` (the hash over every pinned member and not routing or status, aliases,
  deadlines, the probe), `tests/meter.rs` (payloads, rule 158, the fold and the admissions against
  `fixtures/refcases/journal.json`'s `model_gateway` vectors; the demo and per-agent caps),
  `tests/policy_and_guard.rs` (local-only, child ⊆ parent, egress, planted values in every input
  source, and fixtures that must not be refused), `tests/money.rs`, `tests/schema_and_kinds.rs`,
  `tests/deadline_and_egress.rs` (an output validated past its deadline, a rogue adapter's host,
  unreadable costs at the maximum), and `tests/properties.rs` (caps, deadline, identity, and every
  record against journal spec §9.19's rules over random sequences, each oracle shown to catch a
  seeded bug), with fixtures in `tests/common/mod.rs`. The transport's tests run against a local
  TLS server whose authority they generate: `crates/mandate-gateway-http/tests/https.rs` (the
  request on the wire, the key header, statuses and `Retry-After`, no redirect, the body cap, the
  deadline, untrusted servers, cut connections, refused hosts and paths, a caller inside tokio),
  `tests/rules.rs` (the host, path, budget and `Retry-After` rules, the system clock), and
  `tests/gateway.rs` (a pinned reply `ok` and a substituted model `identity_mismatch`, end to end
  over HTTPS), with the server in `tests/common/server.rs`.
- **Not yet:** the workspace's per-risk-day model-spend quota (inference spec §7.3; only the
  monthly cap and the per-agent caps are enforced), and a journal-backed `MeterStore`. The HTTPS
  transport exists but nothing wires it to a live endpoint until the meter is journaled and DEC-997
  is Accepted (DEC-998 item 8, DEC-2200 item 2). The guard's unprefixed-secret rule lets a slash end a run, so filing paths pass, and
  an AWS secret key containing `/` can pass with it; prefixed and key-id forms are caught. The
  email rule counts only ASCII before the `@`, and the fold does no Unicode normalization, so a
  listed name in decomposed form, or `İ`, does not match its composed or folded spelling.
- **Run:** `cargo nextest run -p mandate-gateway -p mandate-gateway-http`.
