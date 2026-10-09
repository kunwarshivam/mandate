//! The MCP client over the transport: the tool allowlist, the pinned contract hash, and the
//! refusal of a server that lists a fund-movement tool (connections spec §6.2 rules 2 and 3, CN-2,
//! CN-9, DEC-441 item 8, DEC-839).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use serde_json::{Map, Value, json};

use crate::budget::CallClass;
use crate::error::{McpError, ServerText};
use crate::transport::{McpTransport, PROTOCOL_VERSION};

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
    /// The pin drift is measured against: the stored one, or at a first connection the hash then
    /// listed, which the connection stores (connections spec §8.1).
    pinned: ContractHash,
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
        let initialize = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "mandate-mcp", "version": env!("CARGO_PKG_VERSION")},
        });
        transport
            .request(CallClass::Ordinary, "initialize", &initialize)
            .await?;
        transport
            .notify(CallClass::Ordinary, "notifications/initialized", &json!({}))
            .await?;
        let listing = list_tools(&transport).await?;
        let (pinned, halted) = match pinned {
            Some(pinned) => {
                let halted = listing.fault(&pinned).is_some();
                (pinned, halted)
            }
            None => match listing.fault(&listing.hash) {
                Some(refusal) => return Err(refusal),
                None => (listing.hash.clone(), false),
            },
        };
        Ok(Self {
            transport,
            pinned,
            observed: listing.hash,
            halted,
        })
    }

    /// The hash of the contract the server listed at connect or at the last check.
    pub fn contract(&self) -> Result<&ContractHash, McpError> {
        Ok(&self.observed)
    }

    /// Whether openings are halted: a drift, a missing allowlisted tool, or a fund-movement tool
    /// was seen. Only a new connection with a new pin clears it.
    pub fn openings_halted(&self) -> Result<bool, McpError> {
        Ok(self.halted)
    }

    /// A health check: lists the tools again. A drift from the pin is [`McpError::ContractDrift`],
    /// and it, a fund-movement tool, and a missing tool each halt openings. The halt is sticky: a
    /// later check that matches the pin again returns `Ok` and leaves openings halted. A list
    /// that cannot be read exactly is [`McpError::Malformed`] and halts openings too; a failed
    /// exchange halts nothing (DEC-843).
    pub async fn check_contract(&mut self) -> Result<(), McpError> {
        let listing = match list_tools(&self.transport).await {
            Ok(listing) => listing,
            Err(McpError::Malformed) => {
                self.halted = true;
                return Err(McpError::Malformed);
            }
            Err(failed) => return Err(failed),
        };
        let fault = listing.fault(&self.pinned);
        self.observed = listing.hash;
        match fault {
            Some(fault) => {
                self.halted = true;
                Err(fault)
            }
            None => Ok(()),
        }
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
        if !ALLOWLIST.contains(&tool) {
            return Err(McpError::ToolNotAllowed);
        }
        if self.halted && class == CallClass::Ordinary && tool == OPENING_TOOL {
            return Err(McpError::ContractDrift);
        }
        let params = json!({"name": tool, "arguments": arguments});
        self.transport.request(class, "tools/call", &params).await
    }
}

/// The one tool whose `Ordinary` call a halt holds (DEC-839 item 4).
const OPENING_TOOL: &str = "place_equity_order";

/// The tokens that make a name a fund-movement tool (DEC-839 item 3).
const FUND_TOKENS: [&str; 15] = [
    "transfer",
    "transfers",
    "withdraw",
    "withdrawal",
    "withdrawals",
    "wire",
    "ach",
    "send",
    "payout",
    "disburse",
    "deposit",
    "deposits",
    "fund",
    "funds",
    "funding",
];

/// What one read of the whole tool list found.
struct Listing {
    hash: ContractHash,
    fund_tool: bool,
    missing_tool: bool,
}

impl Listing {
    /// The first cause that halts, in DEC-839's order: a fund-movement tool, then a missing
    /// allowlisted tool, then a hash that differs from `pinned`.
    fn fault(&self, pinned: &ContractHash) -> Option<McpError> {
        if self.fund_tool {
            Some(McpError::FundMovementTool)
        } else if self.missing_tool {
            Some(McpError::ContractMissingTool)
        } else if &self.hash != pinned {
            Some(McpError::ContractDrift)
        } else {
            None
        }
    }
}

/// `tools/list` followed through every `nextCursor`, so no tool hides on a later page (DEC-839
/// item 2). The ordinary budget bounds a server that pages forever.
async fn list_tools(transport: &McpTransport) -> Result<Listing, McpError> {
    let mut tools = Vec::new();
    let mut params = json!({});
    loop {
        let answer = transport
            .request(CallClass::Ordinary, "tools/list", &params)
            .await?;
        let mut page: Map<String, Value> =
            serde_json::from_str(answer.as_json()).map_err(|_| McpError::Malformed)?;
        match page.remove("tools") {
            Some(Value::Array(listed)) => tools.extend(listed),
            _ => return Err(McpError::Malformed),
        }
        match page.remove("nextCursor") {
            None => return read_contract(&tools),
            Some(Value::String(cursor)) => params = json!({ "cursor": cursor }),
            Some(_) => return Err(McpError::Malformed),
        }
    }
}

/// The hash of DEC-839 item 1's canonical form, and whether a fund-movement tool is listed or an
/// allowlisted tool is not. With a tool missing, the form holds the allowlisted tools listed, so
/// the hash differs from any pin (DEC-843).
fn read_contract(tools: &[Value]) -> Result<Listing, McpError> {
    let mut names = BTreeSet::new();
    let mut allowlisted = BTreeMap::new();
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or(McpError::Malformed)?;
        if !names.insert(name) {
            return Err(McpError::Malformed);
        }
        if ALLOWLIST.contains(&name) {
            let input = tool
                .get("inputSchema")
                .filter(|schema| !schema.is_null())
                .ok_or(McpError::Malformed)?;
            let output = tool.get("outputSchema").unwrap_or(&Value::Null);
            allowlisted.insert(name, (input, output));
        }
    }
    let mut form = String::from("[");
    for (index, (name, (input, output))) in allowlisted.into_iter().enumerate() {
        if index != 0 {
            form.push(',');
        }
        form.push_str("{\"inputSchema\":");
        canonical(input, &mut form)?;
        form.push_str(",\"name\":");
        form.push_str(&Value::from(name).to_string());
        form.push_str(",\"outputSchema\":");
        canonical(output, &mut form)?;
        form.push('}');
    }
    form.push(']');
    Ok(Listing {
        hash: ContractHash(*Digest::of(form.as_bytes()).as_bytes()),
        fund_tool: names.iter().any(|name| moves_funds(name)),
        missing_tool: ALLOWLIST.iter().any(|name| !names.contains(name)),
    })
}

/// `value` written compact with object keys sorted by byte value at every depth; a number that
/// is not an integer is [`McpError::Malformed`] (DEC-839 item 1).
fn canonical(value: &Value, form: &mut String) -> Result<(), McpError> {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            form.push_str(&number.to_string());
        }
        Value::Number(_) => return Err(McpError::Malformed),
        Value::Array(items) => {
            form.push('[');
            for (index, item) in items.iter().enumerate() {
                if index != 0 {
                    form.push(',');
                }
                canonical(item, form)?;
            }
            form.push(']');
        }
        Value::Object(members) => {
            let sorted: BTreeMap<&str, &Value> = members
                .iter()
                .map(|(key, member)| (key.as_str(), member))
                .collect();
            form.push('{');
            for (index, (key, member)) in sorted.into_iter().enumerate() {
                if index != 0 {
                    form.push(',');
                }
                form.push_str(&Value::from(key).to_string());
                form.push(':');
                canonical(member, form)?;
            }
            form.push('}');
        }
        Value::Null | Value::Bool(_) | Value::String(_) => form.push_str(&value.to_string()),
    }
    Ok(())
}

/// Whether `name` is a fund-movement tool under DEC-839 item 3: any non-ASCII character, or a
/// whole word among [`FUND_TOKENS`]. Replaced by the shared `mandate_domain::fund_movement` check
/// once lane L2 lands it (E7-12; backlog, "From E7-16's M2 implementation").
fn moves_funds(name: &str) -> bool {
    !name.is_ascii()
        || words(name)
            .iter()
            .any(|word| FUND_TOKENS.contains(&word.as_str()))
}

/// `name` split at every non-alphanumeric character, at a lower-case letter or a digit before a
/// capital, and before the last capital of a run that a lower-case letter follows, then
/// lower-cased: `getACHStatus` is `get|ach|status`, `v2Transfer` is `v2|transfer`.
fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous: Option<u8> = None;
    let mut bytes = name.bytes().peekable();
    while let Some(byte) = bytes.next() {
        let starts_word = byte.is_ascii_uppercase()
            && previous.is_some_and(|before| {
                before.is_ascii_lowercase()
                    || before.is_ascii_digit()
                    || (before.is_ascii_uppercase()
                        && bytes.peek().is_some_and(u8::is_ascii_lowercase))
            });
        if !byte.is_ascii_alphanumeric() || starts_word {
            words.extend(Some(std::mem::take(&mut word)).filter(|done| !done.is_empty()));
        }
        if byte.is_ascii_alphanumeric() {
            word.push(char::from(byte.to_ascii_lowercase()));
        }
        previous = Some(byte);
    }
    words.extend(Some(word).filter(|done| !done.is_empty()));
    words
}
