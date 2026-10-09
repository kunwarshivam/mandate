#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The transport of the Model Context Protocol client a broker connector uses inside itself
//! (backlog E7-16, connections spec §6.2, DEC-441 item 8). It knows nothing of brokers or orders.
//!
//! - **One pinned host.** A [`PinnedEndpoint`] exists only for an `https` URL on the host the
//!   connector compiles in; plain `http` is accepted only for a loopback address in this crate's
//!   own test build. Redirects are never followed: a `3xx` is [`McpError::Redirected`].
//! - **JSON-RPC 2.0 over streamable HTTP**, with the `Mcp-Session-Id` the server assigns carried
//!   on every later request, and the answer read as one JSON body or as server-sent events.
//! - **Bounded**: connect and request timeouts, a cap on the answer's size, and a [`RateBudget`]
//!   whose reserved bucket only risk-reducing calls draw on, so reads can never starve a cancel
//!   or an exit (`AGENTS.md` rule 13, connections spec §6.5).
//! - **Server text stays opaque.** Results and error details are [`ServerText`], which has no
//!   `Display` and whose `Debug` prints nothing the server sent, so tool metadata, descriptions,
//!   and error messages cannot reach a log, the journal, or a model through an error (CN-9).
//!
//! - **Allowlisted tools, pinned contract** ([`McpClient`]): only the nine tools of [`ALLOWLIST`]
//!   are ever called; the hash of their names and schemas is pinned, a drift halts openings, and
//!   a server that lists a fund-movement tool is refused (CN-2, CN-9, DEC-441 item 8).
//!
//! No credential passes through this slice; the OAuth token is E7-24's.

mod budget;
mod client;
mod endpoint;
mod error;
mod frame;
mod transport;

pub use budget::{BucketConfig, BudgetConfig, CallClass, RateBudget};
pub use client::{ALLOWLIST, ContractHash, McpClient};
pub use endpoint::PinnedEndpoint;
pub use error::{McpError, ServerText};
pub use transport::{McpTransport, Monotonic, PROTOCOL_VERSION, SystemMonotonic, TransportConfig};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies; these tests sit in the \
              crate only because loopback is accepted in its own test build"
)]
mod tests;
