# MCP transport for broker connectors (E7-16)

- **Spec:** `docs/specs/connections.md` §6.2 rules 1 to 4 (M2: `client.rs`, DEC-839), §6.5 (the reserved exit budget), CN-9;
  DEC-441 item 8; slice M1 of the first-live-trade brief (#706).
- **Code:** `mandate-mcp`: `crates/mandate-mcp/src/endpoint.rs` (`PinnedEndpoint`: `https` on the
  pinned host only, plain `http` only to a loopback literal in the crate's own test build),
  `crates/mandate-mcp/src/budget.rs` (`RateBudget`: the ordinary bucket and the reserved one only
  risk-reducing calls draw on), `crates/mandate-mcp/src/transport.rs` (`McpTransport`: one `POST`
  per message, the `Mcp-Session-Id` carried, no redirect followed, bounded timeouts and answer size,
  and the injected `Monotonic` clock), `crates/mandate-mcp/src/frame.rs` (JSON-RPC 2.0 framing, and
  the answer as one JSON body or an event stream), `crates/mandate-mcp/src/error.rs` (`McpError`,
  and `ServerText`, which has no `Display` and whose `Debug` withholds what the server sent),
  `crates/mandate-mcp/src/auth.rs` (E7-24 O1a, DEC-847: OAuth discovery on pinned hosts only).
- **Tests:** in-crate where a loopback server is needed, since loopback is accepted only in the
  crate's own test build: `crates/mandate-mcp/src/tests/endpoint.rs`,
  `crates/mandate-mcp/tests/production.rs` (the production build, which refuses plain `http` even to
  loopback), `crates/mandate-mcp/src/tests/budget.rs` (an oracle that steps one refill period at a
  time), `crates/mandate-mcp/src/tests/server.rs` (the scripted loopback server),
  `crates/mandate-mcp/src/tests/answers.rs`, `crates/mandate-mcp/src/tests/bounds.rs` (sessions,
  redirects, timeouts, the exit budget, and a canary in server text),
  `crates/mandate-mcp/src/tests/errors.rs`, and `crates/mandate-mcp/src/tests/contract.rs` (M2: the
  allowlist, fund-movement refusal, and metadata canary; `drift.rs` adds the pinned hash and drift),
  and `crates/mandate-mcp/src/tests/auth.rs` (O1a: discovery's pins, identifiers and refusals).
- **Run:** `cargo nextest run -p mandate-mcp --run-ignored all`.
