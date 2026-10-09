//! Field types for the envelope and payloads, and the payload schemas registered so far. A payload
//! schema is registered by the story that first emits the event; appending an event type without one
//! is `Invalid` (`unknown_schema`).

use mandate_canon::{DecStr, Digest, Object, Value};
use mandate_time::{Date, UtcNanos};

use crate::{Invalid, InvalidReason};

/// The type of one field. Records have exactly the listed fields, each always present (journal
/// spec §4.2).
pub(crate) enum Ty {
    /// A non-empty string (empty values are `null`, spec §4.2).
    Str,
    /// `[A-Za-z0-9_-]+` (spec §2).
    Ident,
    /// Mandate spec §3's asset ID: `8-4-4-4-12` lowercase hexadecimal with hyphens (spec §9.3).
    AssetId,
    Ulid,
    /// A journal decimal, normalized on the way in (spec §4.6).
    Decimal,
    Int,
    /// `true` or `false` (spec §4.5).
    Bool,
    Timestamp,
    /// A timestamp on a whole second: the risk clock (mandate spec §5.2).
    RiskClock,
    Date,
    /// `sha256:` followed by 64 lowercase hex characters.
    DigestRef,
    /// A non-empty RFC 6901 pointer: `/`-prefixed tokens, `~` only in `~0` or `~1` (spec §9.2).
    Pointer,
    OneOf(&'static [&'static str]),
    Nullable(&'static Ty),
    List(&'static Ty),
    Record(&'static [(&'static str, Ty)]),
    /// Any object; its values are kept as written.
    OpenObject,
    /// A member that is always `null` at its record's version (journal spec §9.7).
    Null,
    /// A personal-data vault reference: `pii_` followed by a ULID (journal spec §9.8, §6.4).
    PiiRef,
    /// §2's form of a stream's identifier, any of the five stream types (journal spec §9.13).
    StreamName,
    /// 64 lowercase hex characters: a SHA-256 the journal does not store (journal spec §9.13).
    Digest,
    /// A string its predicate admits, refused as `id` is: `schema` when not a string, otherwise
    /// `non_canonical` (journal spec §9.15's `notice_id`, `opaque`, and `kind`).
    Text(fn(&str) -> bool),
}

pub(crate) fn normalize(ty: &Ty, value: &Value, path: &str) -> Result<Value, Invalid> {
    let schema = || Invalid::new(InvalidReason::Schema, path);
    let non_canonical = || Invalid::new(InvalidReason::NonCanonical, path);
    let text = || value.as_str().ok_or_else(schema);
    let checked = |ok: bool| {
        if ok {
            Ok(value.clone())
        } else {
            Err(non_canonical())
        }
    };
    match ty {
        Ty::Str => checked(!text()?.is_empty()),
        Ty::Ident => checked(is_ident(text()?)),
        Ty::AssetId => checked(is_asset_id(text()?)),
        Ty::Ulid => checked(is_ulid(text()?)),
        Ty::Decimal => DecStr::parse(text()?)
            .map(|d| Value::Str(d.as_str().to_owned()))
            .map_err(|_| non_canonical()),
        Ty::Int => value.as_int().map(|_| value.clone()).ok_or_else(schema),
        Ty::Bool => match value {
            Value::Bool(_) => Ok(value.clone()),
            _ => Err(schema()),
        },
        Ty::Timestamp => checked(UtcNanos::parse(text()?).is_ok()),
        Ty::RiskClock => checked(UtcNanos::parse(text()?).is_ok_and(|t| t.nanos() == 0)),
        Ty::Date => checked(Date::parse(text()?).is_ok()),
        Ty::DigestRef => checked(parse_digest_ref(text()?).is_some()),
        Ty::Pointer => checked(is_pointer(text()?)),
        Ty::OneOf(options) => checked(options.contains(&text()?)),
        Ty::Nullable(inner) => match value {
            Value::Null => Ok(Value::Null),
            _ => normalize(inner, value, path),
        },
        Ty::List(inner) => value
            .as_array()
            .ok_or_else(schema)?
            .iter()
            .enumerate()
            .map(|(i, item)| normalize(inner, item, &format!("{path}[{i}]")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Ty::Record(fields) => {
            normalize_record(fields, value.as_object().ok_or_else(schema)?, path).map(Value::Object)
        }
        Ty::OpenObject => value.as_object().map(|_| value.clone()).ok_or_else(schema),
        Ty::PiiRef => checked(text()?.strip_prefix("pii_").is_some_and(is_ulid)),
        Ty::StreamName => checked(is_stream_name(text()?)),
        Ty::Digest => checked(Digest::from_hex(text()?).is_some()),
        Ty::Text(admits) => checked(admits(text()?)),
        Ty::Null => match value {
            Value::Null => Ok(Value::Null),
            _ => Err(schema()),
        },
    }
}

pub(crate) fn normalize_record(
    fields: &[(&str, Ty)],
    object: &Object,
    path: &str,
) -> Result<Object, Invalid> {
    let join = |name: &str| {
        if path.is_empty() {
            name.to_owned()
        } else {
            format!("{path}.{name}")
        }
    };
    if let Some(extra) = object
        .keys()
        .find(|k| !fields.iter().any(|(name, _)| *name == k.as_str()))
    {
        return Err(Invalid::new(InvalidReason::Schema, join(extra.as_str())));
    }
    let mut out = Object::new();
    for (name, ty) in fields {
        let (key, value) = object
            .get_key_value(*name)
            .ok_or_else(|| Invalid::new(InvalidReason::Schema, join(name)))?;
        out.insert(key.clone(), normalize(ty, value, &join(name))?);
    }
    Ok(out)
}

pub(crate) fn is_pointer(s: &str) -> bool {
    s.strip_prefix('/').is_some_and(|tokens| {
        let mut escaped = false;
        tokens.chars().all(|c| {
            let ok = !escaped || c == '0' || c == '1';
            escaped = c == '~' && !escaped;
            ok
        }) && !escaped
    })
}

/// Mandate spec §3's asset ID, as `mandate-domain`'s `AssetId::parse` reads it: five groups of
/// 8, 4, 4, 4 and 12 lowercase hexadecimal digits joined by hyphens. Uppercase is refused, never
/// folded, so one asset has one spelling.
fn is_asset_id(s: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut groups = s.split('-');
    GROUPS.iter().all(|&width| {
        groups.next().is_some_and(|group| {
            group.len() == width
                && group
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }) && groups.next().is_none()
}

/// `acct:{workspace_id}:{account_ref}`, `agent:{workspace_id}:{agent_id}`, `ctl:{workspace_id}`,
/// `clock:{workspace_id}`, or `ntf:{workspace_id}`, each segment an identifier (journal spec §2).
fn is_stream_name(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    let segments = match parts.first() {
        Some(&("acct" | "agent")) => 3,
        Some(&("ctl" | "clock" | "ntf")) => 2,
        _ => return false,
    };
    parts.len() == segments && parts.iter().skip(1).all(|segment| is_ident(segment))
}

pub(crate) fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 26 characters of Crockford base32 (uppercase), the first at most `7` so the value fits 128 bits.
pub(crate) fn is_ulid(s: &str) -> bool {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    s.len() == 26
        && s.bytes().all(|b| ALPHABET.contains(&b))
        && s.bytes().next().is_some_and(|b| b <= b'7')
}

pub(crate) fn parse_digest_ref(s: &str) -> Option<Digest> {
    s.strip_prefix("sha256:").and_then(Digest::from_hex)
}

/// The payload schema for `(event_type, schema_version)`, if one is registered.
pub(crate) fn payload_schema(event_type: &str, schema_version: u64) -> Option<&'static Ty> {
    match (event_type, schema_version) {
        ("StreamOpened", 1) => Some(&STREAM_OPENED_V1),
        ("IntentReceived", 1) => Some(&INTENT_RECEIVED_V1),
        ("GateDecided", 1) => Some(&GATE_DECIDED_V1),
        ("OrderSubmitted", 1) => Some(&ORDER_SUBMITTED_V1),
        ("FillApplied", 1) => Some(&FILL_APPLIED_V1),
        ("MarkUpdated", 1) => Some(&MARK_UPDATED_V1),
        ("ReconciliationRun", 1) => Some(&RECONCILIATION_RUN_V1),
        _ => None,
    }
}

/// Account streams only for now; other stream types register their subject fields when their
/// writers are built.
static STREAM_OPENED_V1: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["account"])),
    ("workspace_id", Ty::Ident),
    ("broker", Ty::Str),
    ("account_ref", Ty::Ident),
]);

pub(crate) static INTENT_RECEIVED_V1: Ty = Ty::Record(&[
    ("intent_id", Ty::Ulid),
    ("agent_id", Ty::Ident),
    ("instrument_id", Ty::Str),
    ("side", Ty::Str),
    ("type", Ty::Str),
    ("tif", Ty::Str),
    ("qty", Ty::Decimal),
    ("limit_price", Ty::Nullable(&Ty::Decimal)),
    ("purpose", Ty::Str),
]);

/// Check IDs from the trading domain spec §9.1, as listed in journal spec §9.
pub(crate) const GATE_CHECK_IDS: &[&str] = &[
    "account_status",
    "agent_mode",
    "eligibility",
    "concentration",
    "order_size",
    "session",
    "halt",
    "order_constraints",
    "mark_freshness",
    "collar",
    "conduct",
    "buying_power",
    "gross_exposure",
    "day_trade_budget",
];

pub(crate) static GATE_DECIDED_V1: Ty = Ty::Record(&[
    ("intent_id", Ty::Ulid),
    ("verdict", Ty::Str),
    ("reason_code", Ty::Nullable(&Ty::Str)),
    ("data_profile", Ty::Str),
    (
        "quotes_used",
        Ty::List(&Ty::Record(&[
            ("instrument_id", Ty::Str),
            ("bid", Ty::Decimal),
            ("ask", Ty::Decimal),
            ("as_of", Ty::Timestamp),
            ("feed", Ty::Str),
        ])),
    ),
    (
        "marks_used",
        Ty::List(&Ty::Record(&[
            ("instrument_id", Ty::Str),
            ("price", Ty::Decimal),
            ("source", Ty::Str),
            ("kind", Ty::Str),
        ])),
    ),
    (
        "checks",
        Ty::List(&Ty::Record(&[
            ("id", Ty::OneOf(GATE_CHECK_IDS)),
            ("result", Ty::Str),
            ("inputs", Ty::OpenObject),
            ("computed", Ty::OpenObject),
        ])),
    ),
]);

pub(crate) static ORDER_SUBMITTED_V1: Ty = Ty::Record(&[
    ("client_order_id", Ty::Str),
    ("attempt", Ty::Int),
    ("instrument_id", Ty::Str),
    ("side", Ty::Str),
    ("type", Ty::Str),
    ("tif", Ty::Str),
    ("qty", Ty::Decimal),
    ("limit_price", Ty::Nullable(&Ty::Decimal)),
]);

static FILL_APPLIED_V1: Ty = Ty::Record(&[
    ("fill_id", Ty::Str),
    ("client_order_id", Ty::Str),
    ("instrument_id", Ty::Str),
    ("side", Ty::Str),
    ("qty_gross", Ty::Decimal),
    ("price", Ty::Decimal),
    ("trade_date", Ty::Date),
    ("risk_clock", Ty::RiskClock),
    (
        "fees",
        Ty::List(&Ty::Record(&[
            ("kind", Ty::Str),
            ("amount", Ty::Decimal),
            ("asset", Ty::Str),
            ("status", Ty::Str),
        ])),
    ),
]);

static MARK_UPDATED_V1: Ty = Ty::Record(&[
    ("instrument_id", Ty::Str),
    ("price", Ty::Decimal),
    ("source", Ty::Str),
    ("feed", Ty::Str),
    ("risk_clock", Ty::RiskClock),
]);

static RECONCILIATION_RUN_V1: Ty = Ty::Record(&[
    ("result", Ty::OneOf(&["clean", "adopted", "mismatch"])),
    ("checkpoint", Ty::Nullable(&Ty::Str)),
    ("snapshot_head", Ty::Int),
    ("differences", Ty::Int),
    ("risk_clock", Ty::RiskClock),
]);
