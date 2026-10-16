//! The tool seam a broker connector calls through (DEC-860 item 2, DEC-928 item 1), and the MCP
//! client behind it.

use serde_json::Value;

use crate::budget::CallClass;
use crate::client::McpClient;
use crate::error::McpError;

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

/// [`McpClient::call_tool`] behind the seam: the allowlist, the halt and the budget are the
/// client's, and its every refusal is the seam's, unchanged. The result is the server's JSON byte
/// for byte; the connector reads it and decides what, if anything, leaves (CN-9).
impl Tools for McpClient {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        let result = McpClient::call_tool(self, class, tool, arguments).await?;
        Ok(result.as_json().to_owned())
    }
}
