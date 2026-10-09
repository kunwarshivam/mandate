//! The loopback MCP server over the core (E7-25, S2 tests part 1): loopback only, the session,
//! and the pinned contract (LT-1, LT-8). Oracles: the contract file, `mandate-mcp`'s allowlist and
//! revision, and a hash computed outside Rust.

mod common;

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use common::sim;
use common::wire::Wire;
use mandate_mcp::ALLOWLIST;
use mandate_rh_sim::{CONTRACT, INJECTION, ServerError, SimServer, Variant};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Outcome = Result<(), ServerError>;

/// DEC-839 item 1's hash of [`CONTRACT`], computed when it was written with Python's
/// `json.dumps(sort_keys=True, separators=(",", ":"))` and `hashlib.sha256`.
const PINNED: &str = "584bc25909105beb79882b8c39f595bb4aabfcd6176c6183e2374be47fe2d59d";

/// DEC-839 item 1 computed its own way, as `mandate-mcp`'s M2 tests do: the allowlisted tools'
/// names and schemas, sorted by name, written compact by `serde_json`'s sorted map.
fn hash(tools: &[Value]) -> String {
    let mut nine: Vec<Value> = tools
        .iter()
        .filter(|t| ALLOWLIST.contains(&t["name"].as_str().unwrap()))
        .map(|t| {
            let output = t.get("outputSchema").cloned().unwrap_or(Value::Null);
            json!({"inputSchema": t["inputSchema"], "name": t["name"], "outputSchema": output})
        })
        .collect();
    nine.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let digest = Sha256::digest(Value::Array(nine).to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn names(tools: &[Value]) -> BTreeSet<&str> {
    tools.iter().map(|t| t["name"].as_str().unwrap()).collect()
}

#[test]
#[ignore = "pending E7-25"]
fn the_server_listens_on_loopback_only_at_a_port_the_system_chose() -> Outcome {
    let first = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let second = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let (a, b) = (first.addr()?, second.addr()?);
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    assert_eq!((a.ip(), b.ip()), (loopback, loopback), "127.0.0.1 only");
    assert_ne!(a.port(), 0);
    assert_ne!(a.port(), b.port(), "port 0, so the system chooses");
    let url = format!("http://127.0.0.1:{}/mcp", a.port());
    assert_eq!(first.url()?, url);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_handshake_answers_the_transports_revision_and_a_session_later_requests_need() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let mut wire = Wire::connect(&server.url()?);
    let session = wire.session.clone().unwrap();
    let visible = session.bytes().all(|b| (0x21..=0x7e).contains(&b));
    assert!(!session.is_empty() && visible, "{session:?}");
    let other = Wire::connect(&server.url()?).session.unwrap();
    assert_ne!(other, session, "one session per initialize");
    assert_eq!(wire.request("tools/list", json!({})).status, 200);
    wire.session = None;
    let bare = wire.request("tools/list", json!({}));
    assert_eq!(bare.status, 400, "no session id: {}", bare.body);
    wire.session = Some(format!("{session}x"));
    let stale = wire.request("tools/list", json!({}));
    assert_eq!(stale.status, 404, "a session it never gave: {}", stale.body);
    wire.session = Some(other);
    assert_eq!(wire.request("tools/list", json!({})).status, 200);
    Ok(())
}

#[test]
#[ignore = "pending E7-25"]
fn the_honest_listing_is_the_pinned_contract_and_carries_no_instruction_text() -> Outcome {
    let server = SimServer::start(sim().unwrap(), Variant::Honest)?;
    let tools = Wire::connect(&server.url()?).list();
    let file: Vec<Value> = serde_json::from_str(CONTRACT).unwrap();
    assert_eq!(
        names(&tools),
        BTreeSet::from(ALLOWLIST),
        "the nine, no more"
    );
    assert_eq!(tools.len(), 9, "each tool once");
    assert_eq!(hash(&file), PINNED, "the contract file is the pin");
    assert_eq!(hash(&tools), PINNED, "the listing is the contract file");
    for tool in &tools {
        let pinned = file.iter().find(|f| f["name"] == tool["name"]).unwrap();
        assert_eq!(
            tool["inputSchema"], pinned["inputSchema"],
            "{}",
            tool["name"]
        );
        assert_eq!(
            tool.get("outputSchema"),
            None,
            "the contract publishes none"
        );
        assert!(tool["description"].is_string(), "{}", tool["name"]);
    }
    let text = Value::Array(tools).to_string();
    assert!(
        !text.contains(INJECTION),
        "the honest server injects nothing"
    );
    assert_eq!(
        server.calls()?,
        Vec::<String>::new(),
        "a listing is not a call"
    );
    Ok(())
}
