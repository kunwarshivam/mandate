//! The followed agents' streams: their theses and their fills.

use mandate_canon::{Int, Value};

use super::fixtures::{ACCOUNT, AGENT, WS, object, s};
use super::journal::Journal;

/// An admitted thesis of the followed agent on `asset`, expiring at `expires_at`.
pub fn thesis(journal: &Journal, thesis_id: &str, asset: &str, expires_at: &str) {
    thesis_of(journal, AGENT, thesis_id, asset, expires_at);
}

/// An admitted thesis of `agent` on `asset`, expiring at `expires_at`.
pub fn thesis_of(journal: &Journal, agent: &str, thesis_id: &str, asset: &str, expires_at: &str) {
    journal.push(
        &format!("agent:{WS}:{agent}"),
        "ThesisProposed",
        object(&[
            ("thesis_id", s(thesis_id)),
            ("lineage_id", s(thesis_id)),
            ("instrument_id", s(asset)),
            ("revision", Value::Int(Int::new(0).unwrap())),
            ("expires_at", s(expires_at)),
            ("admitted", Value::Bool(true)),
            ("reason", Value::Null),
        ]),
    );
}

pub fn invalidated(journal: &Journal, thesis_id: &str) {
    journal.push(
        &format!("agent:{WS}:{AGENT}"),
        "ThesisInvalidated",
        object(&[("thesis_id", s(thesis_id))]),
    );
}

/// The agent's filled buy of `qty` of `asset` on its account stream.
pub fn bought(journal: &Journal, order: &str, asset: &str, qty: &str) {
    let stream = format!("acct:{WS}:{ACCOUNT}");
    let clock = "2026-10-26T14:30:00.000000000Z";
    journal.push(
        &stream,
        "OrderRequestRecorded",
        object(&[
            ("agent_id", s(AGENT)),
            ("purpose", s("open")),
            ("risk_clock", s(clock)),
        ]),
    );
    journal.push(
        &stream,
        "OrderSubmitted",
        object(&[
            ("client_order_id", s(order)),
            ("instrument_id", s(asset)),
            ("side", s("buy")),
        ]),
    );
    journal.push(
        &stream,
        "FillApplied",
        object(&[
            ("fill_id", s(&format!("f-{order}"))),
            ("client_order_id", s(order)),
            ("instrument_id", s(asset)),
            ("side", s("buy")),
            ("qty_gross", s(qty)),
            ("price", s("580.00")),
            ("risk_clock", s(clock)),
        ]),
    );
}
