//! The connector over the tool seam.

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{BrokerConnector, BrokerOutcome, BrokerRequest, ConnectorError};
use mandate_mcp::{CallClass, McpError};
use serde_json::Value;

/// One MCP `tools/call` (DEC-860 item 2), answered with the JSON-RPC `result` as sent. An `Err`
/// says nothing about whether the call reached the server.
pub trait Tools {
    fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> impl Future<Output = Result<String, McpError>>;
}

/// The connector for one agentic account, whose number the founder typed (CN-8). It has no
/// `Debug`: the account number is personal data (journal spec §6.4).
pub struct RobinhoodConnector<T> {
    #[expect(
        dead_code,
        reason = "read by E7-6's implementation, not by its tests PR's stubs"
    )]
    tools: T,
    #[expect(
        dead_code,
        reason = "read by E7-6's implementation, not by its tests PR's stubs"
    )]
    account_number: String,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// Every request names `account_number`, and only it.
    pub fn new(tools: T, account_number: String) -> Self {
        Self {
            tools,
            account_number,
        }
    }
}

#[expect(
    clippy::todo,
    reason = "`call`'s errors are the executor's `ConnectorError`, which has no `Unimplemented`, \
              so its stub is todo!(), the other form DEC-137 names"
)]
impl<T: Tools> BrokerConnector for RobinhoodConnector<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        crate::profile::robinhood()
    }

    /// `Submit` and `Cancel` (DEC-860); anything the profile does not offer is `NotSent`.
    async fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        todo!()
    }
}
