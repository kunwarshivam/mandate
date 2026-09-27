//! `BrokerExchangeRecorded`: the raw broker exchange, redacted before it is hashed or stored.
//!
//! Trading-domain spec §13 asks for the broker's requests and responses as records. What this
//! module keeps is their projection onto the fields some wire type reads (`RECORDED_FIELDS`),
//! which drops most of `/v2/account`'s body, so the record is of what the broker said that this
//! platform acts on, not the whole exchange. `AGENTS.md` rule 7 keeps credentials out of it. Neither [`HttpRequest`] nor [`Response`] has a header
//! field at all, so no credential can be in what this module is handed (rung 1 of the trust
//! ladder, stronger than removing one). The body of `/v2/account` does carry the broker's
//! `account_number` and account `id`, which journal spec §6.4 keeps out of the journal, so the
//! redaction pass runs over the **body**, **before** the bytes are hashed or stored: nothing that
//! reaches the journal or the artifact store has ever held either (task brief interpretation 24).
//!
//! Until the vault story lands, a redacted value is **discarded**, not vaulted, and its
//! `pii_refs` entry is a deterministic positional label rather than a random vault reference,
//! because the record is a pure function (ES-21). DEC-142 records this and names the story that
//! replaces both.
//!
//! The pass is a total function over the wire types rather than a denylist of field names: only a
//! field a wire type names is recorded at all, so a field the broker adds later cannot slip
//! through unredacted.

use mandate_canon::Digest;
use serde_json::{Map, Value};

use crate::error::WireError;
use crate::http::{HttpRequest, Response};

/// Above this many bytes the redacted body is stored as a `sha256:` artifact and the payload
/// carries the reference instead (journal spec §6.3, DEC-107).
pub const INLINE_LIMIT: usize = 16_384;

/// Which way the exchange went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Request,
    Response,
}

/// Where the redacted bytes went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedBody {
    /// Small enough to sit in `payload.raw`.
    Inline(String),
    /// Stored under `sha256:{hex}` of the **redacted** bytes, which is what the payload
    /// references. `bytes` are those redacted bytes, so the shell stores exactly what the digest
    /// names and never has to go back to the unredacted body (journal §6.3, DEC-107).
    Artifact { digest: String, bytes: Vec<u8> },
}

/// One redacted exchange, ready to become a `BrokerExchangeRecorded` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedExchange {
    pub direction: Direction,
    /// The endpoint, with no query values that could carry personal data.
    pub endpoint: String,
    pub status: Option<u16>,
    pub body: RecordedBody,
    /// One opaque `pii_refs` entry for each personal-data field replaced, sorted, which is the
    /// only trace of the value that reaches the journal (journal spec §3, §6.4; DEC-142).
    pub pii_refs: Vec<String>,
}

/// Redacts a request's body. A request carries no headers (see the module documentation), so no
/// credential has ever been in the bytes this returns.
pub fn request(request: &HttpRequest) -> Result<RecordedExchange, WireError> {
    let body = request.body().unwrap_or_default().as_bytes();
    let (body, pii_refs) = redact(body)?;
    Ok(RecordedExchange {
        direction: Direction::Request,
        endpoint: endpoint(request.path_and_query()),
        status: None,
        body,
        pii_refs,
    })
}

/// Redacts a response: every personal-data field a wire type names — the account's
/// `account_number` and `id` above all — is replaced by an opaque `pii_refs` entry before the
/// bytes are hashed or stored.
pub fn response(endpoint: &str, response: &Response) -> Result<RecordedExchange, WireError> {
    let (body, pii_refs) = redact(&response.body)?;
    Ok(RecordedExchange {
        direction: Direction::Response,
        endpoint: self::endpoint(endpoint),
        status: Some(response.status),
        body,
        pii_refs,
    })
}

/// Every field name a wire type in [`crate::wire`] names, with the request-side names of a
/// submission body and the per-order rows of a cancel-all answer. A key outside this list is
/// dropped, so a field the broker adds later is never recorded (interpretation 24).
const RECORDED_FIELDS: [&str; 40] = [
    "account_blocked",
    "accrued_fees",
    "activity_type",
    "asset_class",
    "avg_entry_price",
    "body",
    "buying_power",
    "cash",
    "client_order_id",
    "code",
    "created_at",
    "crypto_status",
    "currency",
    "equity",
    "extended_hours",
    "filled_avg_price",
    "filled_qty",
    "id",
    "legs",
    "limit_price",
    "message",
    "multiplier",
    "non_marginable_buying_power",
    "order_class",
    "order_id",
    "order_type",
    "price",
    "qty",
    "replaced_by",
    "side",
    "status",
    "stop_loss",
    "stop_price",
    "symbol",
    "take_profit",
    "time_in_force",
    "trade_suspended_by_user",
    "trading_blocked",
    "transaction_time",
    "type",
];

/// The endpoint without its query, so no query value reaches the record.
fn endpoint(path_and_query: &str) -> String {
    path_and_query
        .split_once('?')
        .map_or(path_and_query, |(path, _)| path)
        .to_owned()
}

/// Projects a body onto [`RECORDED_FIELDS`], replaces its personal data, and places the result
/// inline or by artifact reference. An empty body (a `204`) records as empty. What is kept is the
/// projection: every field some wire type reads, the ones the executor folds, and no other, so
/// the record is of what the broker said that this platform acts on, not its whole body.
fn redact(body: &[u8]) -> Result<(RecordedBody, Vec<String>), WireError> {
    let mut pii_refs = Vec::new();
    let bytes = if body.is_empty() {
        Vec::new()
    } else {
        let value = serde_json::from_slice(body).map_err(|_| WireError::NotJson)?;
        serde_json::to_vec(&project(&value, &mut pii_refs)).map_err(|_| WireError::NotJson)?
    };
    pii_refs.sort();
    let body = if bytes.len() > INLINE_LIMIT {
        RecordedBody::Artifact {
            digest: format!("sha256:{}", Digest::of(&bytes).to_hex()),
            bytes,
        }
    } else {
        RecordedBody::Inline(String::from_utf8(bytes).map_err(|_| WireError::NotJson)?)
    };
    Ok((body, pii_refs))
}

/// The recorded form of one JSON value.
///
/// `account_number` and `account_id` are personal data wherever they appear, and so is the `id`
/// of the object that carries an `account_number` — the account itself (journal spec §6.4). Each
/// is replaced by `pii:<field>:<n>`, numbered within the exchange, and the same text is pushed to
/// `pii_refs`. An order's own `id` is not personal data and is kept: it is how the record ties
/// what the broker said to the order it said it about (trading-domain spec §13).
fn project(value: &Value, pii_refs: &mut Vec<String>) -> Value {
    match value {
        Value::Object(fields) => {
            let is_account = fields.contains_key("account_number");
            let mut kept = Map::new();
            for (key, field) in fields {
                let personal = match key.as_str() {
                    "account_number" | "account_id" => Some(key.as_str()),
                    "id" if is_account => Some("account_id"),
                    _ => None,
                };
                if let Some(name) = personal {
                    let reference = format!("pii:{name}:{}", pii_refs.len().saturating_add(1));
                    pii_refs.push(reference.clone());
                    kept.insert(key.clone(), Value::String(reference));
                } else if RECORDED_FIELDS.contains(&key.as_str()) {
                    kept.insert(key.clone(), project(field, pii_refs));
                }
            }
            Value::Object(kept)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| project(item, pii_refs)).collect())
        }
        other => other.clone(),
    }
}
