//! Writer drafts: the envelope without the journal-assigned fields (journal spec §3), validated
//! and normalized before anything is compared, sequenced, or hashed.

use mandate_canon::{Object, Value};

use crate::{Environment, Invalid, InvalidReason, StreamId};

/// A validated, normalized draft and its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    fields: Object,
    canonical: Vec<u8>,
    stream_id: StreamId,
    environment: Environment,
}

impl Draft {
    /// Parses and validates one draft: the envelope (§3), canonical values (§4), the catalogue's
    /// stream types and required `config_refs` (§9), and the payload schema. Decimals are
    /// normalized, so equivalent drafts have identical canonical bytes.
    pub fn parse(_bytes: &[u8]) -> Result<Self, Invalid> {
        Err(Invalid::new(InvalidReason::Schema, ""))
    }

    pub fn event_id(&self) -> &str {
        self.text("event_id")
    }

    pub fn event_type(&self) -> &str {
        self.text("event_type")
    }

    pub fn schema_version(&self) -> u64 {
        self.fields
            .get("schema_version")
            .and_then(Value::as_int)
            .unwrap_or_default()
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub fn environment(&self) -> Environment {
        self.environment
    }

    /// The canonical bytes of the normalized draft; idempotency compares these (spec §5.1).
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    fn text(&self, name: &str) -> &str {
        self.fields
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
}
