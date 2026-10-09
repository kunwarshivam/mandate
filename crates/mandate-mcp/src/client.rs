//! The MCP client over the transport: the tool allowlist, the pinned contract hash, and the
//! refusal of a server that lists a fund-movement tool (connections spec §6.2 rules 2 and 3, CN-2,
//! CN-9, DEC-441 item 8, DEC-839).

use serde_json::Value;

use crate::budget::CallClass;
use crate::error::{McpError, ServerText};
use crate::transport::McpTransport;

/// The nine tools the connector may call (connections spec §6.2 rule 2).
pub const ALLOWLIST: [&str; 9] = [
    "get_accounts",
    "get_portfolio",
    "get_equity_positions",
    "get_equity_quotes",
    "get_equity_tradability",
    "get_equity_orders",
    "review_equity_order",
    "place_equity_order",
    "cancel_equity_order",
];

/// SHA-256 of the allowlisted tools' names and input and output schemas, in the canonical form
/// DEC-839 defines. It is derived from server text but is a digest, so it may be printed.
#[derive(Clone, PartialEq, Eq)]
pub struct ContractHash([u8; 32]);

impl ContractHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The digest is not server text, so it prints as `ContractHash(` and 64 lower-case hex digits.
impl std::fmt::Debug for ContractHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContractHash(")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        f.write_str(")")
    }
}

/// One MCP session whose tool contract was checked at connect.
#[derive(Debug)]
pub struct McpClient {
    transport: McpTransport,
    observed: ContractHash,
    halted: bool,
}

impl McpClient {
    /// `initialize`, `notifications/initialized`, then `tools/list` to the last page, all as
    /// [`CallClass::Ordinary`]. With no `pinned` hash (the first connection), a fund-movement tool
    /// is [`McpError::FundMovementTool`] and an allowlisted tool missing is
    /// [`McpError::ContractMissingTool`]: no client is returned. With a `pinned` hash (every later
    /// session start) no such cause returns no client, since exits need the connection: a hash that
    /// differs, a fund-movement tool, or a missing tool each give a client with
    /// [`McpClient::openings_halted`] true (DEC-839, `AGENTS.md` rule 13).
    pub async fn connect(
        transport: McpTransport,
        pinned: Option<ContractHash>,
    ) -> Result<Self, McpError> {
        let _ = (transport, pinned);
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// The hash of the contract the server listed at connect or at the last check.
    pub fn contract(&self) -> Result<&ContractHash, McpError> {
        let _ = &self.observed;
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// Whether openings are halted: a drift, a missing allowlisted tool, or a fund-movement tool
    /// was seen. Only a new connection with a new pin clears it.
    pub fn openings_halted(&self) -> Result<bool, McpError> {
        let _ = self.halted;
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// A health check: lists the tools again. A drift from the pin is [`McpError::ContractDrift`],
    /// and it, a fund-movement tool, and a missing tool each halt openings. The halt is sticky: a
    /// later check that matches the pin again returns `Ok` and leaves openings halted.
    pub async fn check_contract(&mut self) -> Result<(), McpError> {
        let _ = &self.transport;
        Err(McpError::Unimplemented { story: "E7-16" })
    }

    /// `tools/call` for `tool`, which must be in [`ALLOWLIST`] exactly, else
    /// [`McpError::ToolNotAllowed`] before anything is sent or any budget is drawn. While openings
    /// are halted, an [`CallClass::Ordinary`] `place_equity_order` is [`McpError::ContractDrift`]
    /// and nothing is sent; reads, cancels, and every risk-reducing call go on (`AGENTS.md` rule 13).
    pub async fn call_tool(
        &self,
        class: CallClass,
        tool: &str,
        arguments: &Value,
    ) -> Result<ServerText, McpError> {
        let _ = (class, tool, arguments);
        Err(McpError::Unimplemented { story: "E7-16" })
    }
}
