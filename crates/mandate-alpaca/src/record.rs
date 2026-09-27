//! `BrokerExchangeRecorded`: the raw broker exchange, redacted before it is hashed or stored.
//!
//! Trading-domain spec §13 keeps raw broker requests and responses as records and `AGENTS.md`
//! rule 7 keeps credentials out of them — but the raw body of `/v2/account` also carries the
//! broker's `account_number` and account `id`, which journal spec §6.4 keeps in the vault and
//! holds by reference. So the redaction pass runs over the **body** as well as the headers, and
//! it runs **before** the bytes are hashed or stored, so nothing that reaches the journal or the
//! artifact store has ever held either (task brief interpretation 24).
//!
//! The pass is a total function over the wire types rather than a denylist of field names: only a
//! field a wire type names is recorded at all, so a field the broker adds later cannot slip
//! through unredacted.

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
    /// One opaque `pii_refs` entry for each personal-data field replaced, which is the only trace
    /// of the value that reaches the journal (journal spec §6.4).
    pub pii_refs: Vec<String>,
}

/// Redacts a request: the authorisation headers are removed before anything is hashed or stored,
/// so no credential has ever been in the bytes this returns.
pub fn request(request: &HttpRequest) -> Result<RecordedExchange, WireError> {
    let _ = request;
    Err(WireError::Unimplemented { story: "E7-2" })
}

/// Redacts a response: every personal-data field a wire type names — the account's
/// `account_number` and `id` above all — is replaced by an opaque `pii_refs` entry before the
/// bytes are hashed or stored.
pub fn response(endpoint: &str, response: &Response) -> Result<RecordedExchange, WireError> {
    let _ = (endpoint, response);
    Err(WireError::Unimplemented { story: "E7-2" })
}
