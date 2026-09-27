//! Alpaca's trading wire types and the parse into exact values.
//!
//! Every quantity, price, and amount arrives as **raw text** and is parsed by `mandate-num`
//! (ADR-0001 ES-23). A field the broker sends in exponent form, with more places than the
//! instrument's increment, or as a JSON number — which has already been through a float — is a
//! typed [`WireError`] with a stable code, never a rounded value.
//!
//! The wire types name every field this crate records. A field the broker adds later is not in a
//! wire type, so it is never recorded and cannot slip past the redaction pass
//! (task brief interpretation 24). A field the wire type names but this stream does not yet
//! interpret answers [`WireError::NotInterpreted`] with its owning story (DEC-85).

use mandate_executor::{
    BrokerAccount, BrokerFill, BrokerOrder, BrokerPosition, BrokerReject, StatusMapping,
    SubmitOrder,
};

use crate::error::WireError;

/// Alpaca's order object, as `/v2/orders` and `/v2/orders:by_client_order_id` return it.
///
/// There is no `account_number` or account `id` on this type because the order endpoints do not
/// return one; the account endpoint does, and [`WireAccount`] is where that is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireOrder {
    pub id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub asset_class: String,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub qty: Option<String>,
    pub filled_qty: String,
    pub limit_price: Option<String>,
    pub stop_price: Option<String>,
    pub filled_avg_price: Option<String>,
    pub status: String,
    pub extended_hours: bool,
    pub created_at: String,
    pub replaced_by: Option<String>,
    pub legs: Vec<WireOrder>,
}

/// Alpaca's position object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePosition {
    pub symbol: String,
    pub asset_class: String,
    pub qty: String,
    pub avg_entry_price: String,
}

/// Alpaca's account object.
///
/// `account_number` and `id` are named here **so that the redaction pass can replace them**:
/// journal spec §6.4 keeps both in the personal-data vault and holds them by reference, so
/// neither ever reaches a draft, a log, or the artifact store (interpretation 24).
///
/// `Debug` is written by hand and prints neither of them, so a log line or a panic message that
/// formats the account cannot leak personal data either (`AGENTS.md` rule 7, journal §6.4).
#[derive(Clone, PartialEq, Eq)]
pub struct WireAccount {
    pub id: String,
    pub account_number: String,
    pub status: String,
    pub crypto_status: String,
    pub currency: String,
    pub trading_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
    pub multiplier: String,
    pub equity: String,
    pub cash: String,
    pub buying_power: String,
    pub non_marginable_buying_power: String,
    pub accrued_fees: String,
}

impl core::fmt::Debug for WireAccount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WireAccount")
            .field("status", &self.status)
            .field("crypto_status", &self.crypto_status)
            .field("currency", &self.currency)
            .field("trading_blocked", &self.trading_blocked)
            .field("account_blocked", &self.account_blocked)
            .field("trade_suspended_by_user", &self.trade_suspended_by_user)
            .field("multiplier", &self.multiplier)
            .field("equity", &self.equity)
            .field("cash", &self.cash)
            .field("buying_power", &self.buying_power)
            .field(
                "non_marginable_buying_power",
                &self.non_marginable_buying_power,
            )
            .field("accrued_fees", &self.accrued_fees)
            .finish_non_exhaustive()
    }
}

/// One row of `/v2/account/activities`, restricted to the `FILL` activities this stream ingests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireActivity {
    pub id: String,
    pub activity_type: String,
    pub symbol: String,
    pub side: String,
    pub qty: String,
    pub price: String,
    pub transaction_time: String,
    pub order_id: String,
    pub client_order_id: Option<String>,
}

/// Alpaca's error body for a rejected request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRejectBody {
    pub code: Option<i64>,
    pub message: String,
}

/// Parses one order object from a response body.
pub fn order(body: &[u8]) -> Result<BrokerOrder, WireError> {
    let _ = body;
    Err(WireError::Unimplemented { story: "E7-2" })
}

/// Parses a list of open orders.
pub fn open_orders(body: &[u8]) -> Result<Vec<BrokerOrder>, WireError> {
    let _ = body;
    Err(WireError::Unimplemented { story: "E7-3" })
}

/// Parses the position list.
pub fn positions(body: &[u8]) -> Result<Vec<BrokerPosition>, WireError> {
    let _ = body;
    Err(WireError::Unimplemented { story: "E7-3" })
}

/// Parses the account object, dropping `id` and `account_number` before anything else: the type
/// this returns has nowhere to put either (journal spec §6.4).
pub fn account(body: &[u8]) -> Result<BrokerAccount, WireError> {
    let _ = body;
    Err(WireError::Unimplemented { story: "E7-3" })
}

/// Parses the `FILL` activities since the cursor.
pub fn activities(body: &[u8]) -> Result<Vec<BrokerFill>, WireError> {
    let _ = body;
    Err(WireError::Unimplemented { story: "E7-3" })
}

/// Parses a rejection body into the signals trading-domain spec §7.3's restriction table reads.
pub fn reject(status: u16, body: &[u8]) -> Result<BrokerReject, WireError> {
    let _ = (status, body);
    Err(WireError::Unimplemented { story: "E7-3" })
}

/// Maps one raw Alpaca status through trading-domain spec §5.7's table.
///
/// The table is **total**: every value it names maps, and any other value is
/// [`WireError::UnknownStatus`], which the executor turns into a pause and an alert — the spec's
/// own last row — rather than a silent no-op.
pub fn status(raw: &str) -> Result<StatusMapping, WireError> {
    let _ = raw;
    Err(WireError::Unimplemented { story: "E7-2" })
}

/// The canonical JSON body of one submission, with **our** `client_order_id` on the wire.
///
/// The body is canonical so that a recorded `requests.txt` line pins exactly what was sent, and
/// so that two runs of the same submission produce byte-identical bytes (ES-21).
pub fn submission_body(order: &SubmitOrder) -> Result<String, WireError> {
    let _ = order;
    Err(WireError::Unimplemented { story: "E7-2" })
}

/// Reads one decimal field as canonical text, refusing a JSON number, an exponent, and more
/// places than the field allows (ES-23).
pub fn decimal_text(raw: &serde_json::Value, field: &'static str) -> Result<String, WireError> {
    let _ = (raw, field);
    Err(WireError::Unimplemented { story: "E7-2" })
}
