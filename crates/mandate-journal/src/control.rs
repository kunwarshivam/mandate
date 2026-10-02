//! The payload schemas journal spec §9.2 closes (DEC-261, DEC-302, E7-10): the control stream's
//! records that `ValidationContext::from_journal` reads, and `OwnerCommandRefused` on the agent and
//! account streams, and `AccountSnapshotRecorded` on the account stream, with consistency rules 17
//! to 24, subject rules 25, 26 and 28, and copy rule 27. Every rule only refuses a draft; none
//! changes what a writer may do.
//!
//! `AccountSnapshotRecorded` registered once stream K's fee-step writer conformed (#456), in the
//! change that journals that writer's form in `fees`, so the fee step's snapshot is never refused
//! for a member while it pauses every agent and alerts the owner (`AGENTS.md` rules 3 and 13,
//! DEC-261 item 7, DEC-402).

use mandate_canon::Value;
use mandate_num::Usd;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamId, StreamType};

/// The event types §9.2 closes on the control stream.
const CONTROL: [&str; 9] = [
    "StreamOpened",
    "ConnectionEstablished",
    "ConnectionRevoked",
    "DisclosureAccepted",
    "ConfigSnapshotRegistered",
    "MandateVersionCreated",
    "MandateConfirmed",
    "AgentDeployed",
    "AgentStopped",
];

/// The event type §9.2 closes on both the agent and the account stream.
const REFUSAL: &str = "OwnerCommandRefused";

/// The account stream's snapshot §9.2 closes, with rule 24.
const SNAPSHOT: &str = "AccountSnapshotRecorded";

/// Whether §9.2 governs `event_type` on `stream`; every other event keeps its own registration.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    match stream.stream_type() {
        StreamType::Control => CONTROL.contains(&event_type),
        StreamType::Agent => event_type == REFUSAL,
        StreamType::Account => event_type == REFUSAL || event_type == SNAPSHOT,
        StreamType::Scheduler => false,
    }
}

/// The payload normalized against its §9.2 schema, then consistency rules 17 to 24 in number order
/// (reported only on a well-typed payload). `config_refs` is the envelope's, which rule 22 reads.
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    config_refs: Option<&Value>,
) -> Result<Value, Invalid> {
    let schema = schema(event_type, schema_version)
        .ok_or_else(|| Invalid::new(InvalidReason::UnknownSchema, "payload"))?;
    let payload = crate::schema::normalize(schema, payload, "payload")?;
    let p = Payload(&payload);
    match event_type {
        "MandateVersionCreated" => {
            let paths: Vec<&str> = p
                .list("provenance")
                .iter()
                .map(|entry| {
                    entry
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                })
                .collect();
            ascending(&paths, "payload.provenance")?;
        }
        "MandateConfirmed" => ascending(&p.texts("confirmed_paths"), "payload.confirmed_paths")?,
        "ConnectionEstablished" => ascending(&p.texts("scopes"), "payload.scopes")?,
        "ConfigSnapshotRegistered" => {
            ascending(&p.texts("params"), "payload.params")?;
            let model = p.text("kind") == MODEL_KIND;
            for member in ["model_id", "model_version", "admits_instruments"] {
                ensure(
                    p.is_null(member) != model,
                    InvalidReason::Schema,
                    &format!("payload.{member}"),
                )?;
            }
            ensure(
                model || p.list("params").is_empty(),
                InvalidReason::Schema,
                "payload.params",
            )?;
        }
        "AgentDeployed" => {
            let bound = config_refs
                .and_then(|refs| refs.get("mandate_version"))
                .and_then(Value::as_str);
            ensure(
                bound.is_none_or(|bound| bound == p.text("mandate_version")),
                InvalidReason::Schema,
                "payload.mandate_version",
            )?;
        }
        "AgentStopped" => ensure(
            !p.text("loss_added").starts_with('-'),
            InvalidReason::Schema,
            "payload.loss_added",
        )?,
        SNAPSHOT => {
            let compared = !p.is_null("model_cash");
            for member in ["cash_band", "cash_in_band"] {
                ensure(
                    p.is_null(member) != compared,
                    InvalidReason::Schema,
                    &format!("payload.{member}"),
                )?;
            }
            if compared {
                cash_in_band(p)?;
            }
        }
        _ => {}
    }
    Ok(payload)
}

/// Subject rules 25, 26 and 28 (`stream_mismatch`), then copy rule 27, on a payload that passed
/// [`payload`]: reported after `artifact_refs` and `pii_refs` (§9.1's order, which §9.2 keeps).
pub(crate) fn subject_and_copy(
    event_type: &str,
    stream: &StreamId,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let p = Payload(payload);
    if event_type == "StreamOpened" {
        let opened = format!("ctl:{}", p.text("workspace_id"));
        return ensure(
            opened == stream.as_str(),
            InvalidReason::StreamMismatch,
            "stream_id",
        );
    }
    if event_type != REFUSAL {
        return Ok(());
    }
    let allowed: &[&str] = match stream.stream_type() {
        StreamType::Account => &["acknowledge"],
        StreamType::Agent => &["resume", "stop"],
        StreamType::Control | StreamType::Scheduler => &[],
    };
    ensure(
        allowed.contains(&p.text("command")),
        InvalidReason::StreamMismatch,
        "payload.command",
    )?;
    ensure(
        p.text("reason") != NOT_INDEPENDENT || stream.stream_type() == StreamType::Account,
        InvalidReason::StreamMismatch,
        "payload.reason",
    )?;
    ensure(
        causation_id.is_some_and(|c| *c != Value::Null),
        InvalidReason::Schema,
        "causation_id",
    )
}

/// The refusal only the executor writes, of an acknowledgment that lifts a fired tripwire under
/// `independent_approval_required` (rule 28; mandate spec §5.8, §6.7; DEC-351 items 5 and 6).
const NOT_INDEPENDENT: &str = "not_independent";

/// `ConfigSnapshotRegistered`'s kind for a signal model.
const MODEL_KIND: &str = "model_version";

/// A normalized payload, read member by member; a `null` or absent member reads as empty.
#[derive(Clone, Copy)]
struct Payload<'a>(&'a Value);

impl<'a> Payload<'a> {
    fn text(self, member: &str) -> &'a str {
        self.0
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn is_null(self, member: &str) -> bool {
        matches!(self.0.get(member), None | Some(Value::Null))
    }

    fn list(self, member: &str) -> &'a [Value] {
        self.0
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn texts(self, member: &str) -> Vec<&'a str> {
        self.list(member).iter().filter_map(Value::as_str).collect()
    }
}

fn ensure(holds: bool, reason: InvalidReason, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(reason, path))
    }
}

/// Strictly ascending by bytes, so a repeat is refused as well as a swap (`non_canonical`).
fn ascending(items: &[&str], path: &str) -> Result<(), Invalid> {
    ensure(
        items
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.as_bytes() < b.as_bytes())),
        InvalidReason::NonCanonical,
        path,
    )
}

/// Rule 24 on a compared snapshot: `cash_band` ≥ 0, and `cash_in_band` is `true` exactly when
/// |`cash` − `model_cash`| ≤ `cash_band`. The amounts are compared as exact `Usd`; one too large to
/// compare exactly is refused as `schema` at that member, never compared approximately (DEC-402).
fn cash_in_band(p: Payload<'_>) -> Result<(), Invalid> {
    let amount = |member: &str| {
        Usd::parse(p.text(member))
            .map_err(|_| Invalid::new(InvalidReason::Schema, format!("payload.{member}")))
    };
    let band = amount("cash_band")?;
    ensure(
        !band.is_negative(),
        InvalidReason::Schema,
        "payload.cash_band",
    )?;
    let drift = amount("cash")?
        .checked_sub(amount("model_cash")?)
        .map_err(|_| Invalid::new(InvalidReason::Schema, "payload.model_cash"))?;
    let drift = if drift.is_negative() {
        drift.negated()
    } else {
        drift
    };
    ensure(
        p.0.get("cash_in_band") == Some(&Value::Bool(drift <= band)),
        InvalidReason::Schema,
        "payload.cash_in_band",
    )
}

fn schema(event_type: &str, schema_version: u64) -> Option<&'static Ty> {
    if schema_version != 1 {
        return None;
    }
    Some(match event_type {
        "StreamOpened" => &STREAM_OPENED,
        "ConnectionEstablished" => &CONNECTION_ESTABLISHED,
        "ConnectionRevoked" => &CONNECTION_REVOKED,
        "DisclosureAccepted" => &DISCLOSURE_ACCEPTED,
        "ConfigSnapshotRegistered" => &CONFIG_SNAPSHOT_REGISTERED,
        "MandateVersionCreated" => &MANDATE_VERSION_CREATED,
        "MandateConfirmed" => &MANDATE_CONFIRMED,
        "AgentDeployed" => &AGENT_DEPLOYED,
        "AgentStopped" => &AGENT_STOPPED,
        "OwnerCommandRefused" => &OWNER_COMMAND_REFUSED,
        SNAPSHOT => &ACCOUNT_SNAPSHOT_RECORDED,
        _ => return None,
    })
}

static ACCOUNT_SNAPSHOT_RECORDED: Ty = Ty::Record(&[
    ("status", Ty::Str),
    ("crypto_status", Ty::Str),
    ("trading_blocked", Ty::Bool),
    ("account_blocked", Ty::Bool),
    ("trade_suspended_by_user", Ty::Bool),
    ("multiplier", Ty::Int),
    ("equity", Ty::Decimal),
    ("cash", Ty::Decimal),
    ("buying_power", Ty::Decimal),
    ("non_marginable_buying_power", Ty::Decimal),
    ("accrued_fees", Ty::Decimal),
    ("model_cash", Ty::Nullable(&Ty::Decimal)),
    ("cash_band", Ty::Nullable(&Ty::Decimal)),
    ("cash_in_band", Ty::Nullable(&Ty::Bool)),
    ("risk_clock", Ty::RiskClock),
]);

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["control"])),
    ("workspace_id", Ty::Ident),
]);

static CONNECTION_ESTABLISHED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("broker", Ty::Str),
    ("environment", Ty::OneOf(&["paper", "live"])),
    ("scopes", Ty::List(&Ty::Str)),
]);

static CONNECTION_REVOKED: Ty = Ty::Record(&[("connection_id", Ty::Ident)]);

static DISCLOSURE_ACCEPTED: Ty = Ty::Record(&[
    ("document", Ty::Ident),
    ("version", Ty::DigestRef),
    ("user", Ty::Str),
    (
        "step_up",
        Ty::Record(&[
            ("assertion_id", Ty::Str),
            ("authenticated_at", Ty::Timestamp),
            ("method", Ty::Str),
        ]),
    ),
]);

static CONFIG_SNAPSHOT_REGISTERED: Ty = Ty::Record(&[
    (
        "kind",
        Ty::OneOf(&[
            "fee_config",
            "trading_calendar",
            "settlement_calendar",
            "instrument_snapshot",
            "rule_set",
            "mandate_version",
            "model_version",
        ]),
    ),
    ("content_hash", Ty::DigestRef),
    ("model_id", Ty::Nullable(&Ty::Str)),
    ("model_version", Ty::Nullable(&Ty::Str)),
    ("params", Ty::List(&Ty::Str)),
    ("admits_instruments", Ty::Nullable(&Ty::Bool)),
]);

static MANDATE_VERSION_CREATED: Ty = Ty::Record(&[
    ("mandate_version", Ty::DigestRef),
    (
        "provenance",
        Ty::List(&Ty::Record(&[
            ("path", Ty::Pointer),
            (
                "source",
                Ty::OneOf(&[
                    "user_stated",
                    "user_entered",
                    "template_structure",
                    "platform_proposed",
                    "platform_default",
                ]),
            ),
        ])),
    ),
    ("record_ref", Ty::DigestRef),
]);

static MANDATE_CONFIRMED: Ty = Ty::Record(&[
    ("mandate_version", Ty::DigestRef),
    ("confirmed_paths", Ty::List(&Ty::Pointer)),
    ("record_ref", Ty::DigestRef),
]);

static AGENT_DEPLOYED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("mandate_version", Ty::DigestRef),
    ("record_ref", Ty::DigestRef),
]);

static AGENT_STOPPED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("connection_id", Ty::Ident),
    (
        "reason",
        Ty::OneOf(&[
            "goal_complete",
            "profit_stop_reached",
            "end_date",
            "owner_stop",
        ]),
    ),
    ("retired_on", Ty::Date),
    ("loss_added", Ty::Decimal),
]);

static OWNER_COMMAND_REFUSED: Ty = Ty::Record(&[
    ("command", Ty::OneOf(&["resume", "stop", "acknowledge"])),
    (
        "reason",
        Ty::OneOf(&[
            "step_up_missing",
            "step_up_stale",
            "step_up_reused",
            "step_up_method",
            NOT_INDEPENDENT,
        ]),
    ),
    ("effective_at", Ty::Timestamp),
]);

/// The vectors' `control_stream` section, read inside this crate so the mutation gate, which runs
/// only the mutated crate's own tests, judges §9.2's rules here as well as in `mandate-refcases`.
/// A draft is checked by `Draft::parse` alone.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use mandate_canon::{Int, Key, Object, Value, parse, to_canonical};

    use crate::{Draft, StreamId};

    use super::subject_and_copy;

    fn section() -> Result<Value, String> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let fixture = parse(&bytes).map_err(|e| format!("{e:?}"))?;
        fixture
            .get("control_stream")
            .cloned()
            .ok_or_else(|| "no control_stream".to_owned())
    }

    fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
        value
            .get(name)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn text<'a>(value: &'a Value, name: &str) -> &'a str {
        value.get(name).and_then(Value::as_str).unwrap_or_default()
    }

    /// A case's base as a writer's draft: a chain event's body without the journal's fields, or
    /// one of the section's named drafts.
    fn base(section: &Value, case: &Value) -> Result<Object, String> {
        let body = match case.get("base_seq").and_then(Value::as_int) {
            Some(seq) => list(section, "chain")
                .iter()
                .find(|e| e.get("seq").and_then(Value::as_int) == Some(seq))
                .and_then(|e| e.get("body"))
                .ok_or_else(|| format!("no chain seq {seq}"))?,
            None => section
                .get("drafts")
                .and_then(|d| d.get(text(case, "base_draft")))
                .ok_or_else(|| format!("no draft {}", text(case, "base_draft")))?,
        };
        let mut body = body.as_object().cloned().ok_or("a body is an object")?;
        for field in ["seq", "prev_hash", "recorded_at"] {
            body.remove(field);
        }
        Ok(body)
    }

    /// Applies a `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
    fn apply(draft: &mut Object, change: &Value) -> Result<(), String> {
        let path = text(change, "path");
        let mut names: Vec<&str> = path.split('.').collect();
        let last = names.pop().ok_or("empty path")?;
        let mut node = draft;
        for name in names {
            node = match node.get_mut(name) {
                Some(Value::Object(inner)) => inner,
                _ => return Err(format!("`{path}` has no object `{name}`")),
            };
        }
        if change.get("delete") == Some(&Value::Bool(true)) {
            node.remove(last)
                .map(|_| ())
                .ok_or(format!("`{path}` is absent"))
        } else {
            let key = Key::new(last).map_err(|_| format!("bad key `{last}`"))?;
            let value = change.get("value").cloned().ok_or("no value")?;
            node.insert(key, value);
            Ok(())
        }
    }

    /// The case's draft: its base with its changes applied.
    fn draft(section: &Value, case: &Value) -> Result<Vec<u8>, String> {
        let mut body = base(section, case)?;
        for change in list(case, "changes") {
            apply(&mut body, change)?;
        }
        Ok(to_canonical(&Value::Object(body)))
    }

    #[test]
    fn every_chain_event_and_valid_draft_parses() -> Result<(), String> {
        let section = section()?;
        let chain = list(&section, "chain");
        assert_eq!(chain.len(), 10);
        for entry in chain {
            let case = Value::Object(
                [(
                    Key::new("base_seq").map_err(|_| "key")?,
                    entry.get("seq").cloned().ok_or("seq")?,
                )]
                .into_iter()
                .collect(),
            );
            let bytes = draft(&section, &case)?;
            let parsed = Draft::parse(&bytes).map(|d| d.event_type().to_owned());
            assert_eq!(parsed, Ok(text(entry, "event_type").to_owned()));
        }
        let mut parsed = 0;
        for case in list(&section, "valid_drafts") {
            let bytes = draft(&section, case)?;
            assert_eq!(
                Draft::parse(&bytes).map(|_| ()),
                Ok(()),
                "{}",
                text(case, "name")
            );
            parsed += 1;
        }
        assert!(
            parsed >= 8,
            "{parsed} valid drafts; a vectors change may add more, never fewer"
        );
        for name in [
            "refused_stop",
            "refused_acknowledgment",
            "snapshot_reconciled",
            "snapshot_fees",
        ] {
            let case = Value::Object(
                [(
                    Key::new("base_draft").map_err(|_| "key")?,
                    Value::Str(name.to_owned()),
                )]
                .into_iter()
                .collect(),
            );
            let bytes = draft(&section, &case)?;
            assert_eq!(Draft::parse(&bytes).map(|_| ()), Ok(()), "{name}");
        }
        Ok(())
    }

    #[test]
    fn every_invalid_draft_is_refused_with_its_reason_at_its_path() -> Result<(), String> {
        let section = section()?;
        let mut checked = 0;
        for case in list(&section, "invalid_drafts") {
            let bytes = draft(&section, case)?;
            let expect = case.get("expect").ok_or("no expect")?;
            let refused = Draft::parse(&bytes)
                .err()
                .map(|e| (e.reason.code().to_owned(), e.path));
            let wanted = (
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            );
            assert_eq!(refused, Some(wanted), "{}", text(case, "name"));
            checked += 1;
        }
        assert!(
            checked >= 75,
            "{checked} invalid drafts; a vectors change may add more, never fewer"
        );
        Ok(())
    }

    /// The members §9.2 lists without `?`, at every depth, as paths below `payload`: written out
    /// from the spec's tables rather than read from this module's schemas, so a schema loosened to
    /// nullable cannot also drop its member from this list (#445 round 1, B2; round 2, C1). A list
    /// is named by its first and second elements, so a check that stops after the first is seen
    /// (#445 round 3, D1).
    const REQUIRED: [(&str, &[&str]); 11] = [
        ("StreamOpened", &["stream_type", "workspace_id"]),
        (
            "ConnectionEstablished",
            &[
                "connection_id",
                "broker",
                "environment",
                "scopes",
                "scopes[0]",
                "scopes[1]",
            ],
        ),
        ("ConnectionRevoked", &["connection_id"]),
        (
            "DisclosureAccepted",
            &[
                "document",
                "version",
                "user",
                "step_up",
                "step_up.assertion_id",
                "step_up.authenticated_at",
                "step_up.method",
            ],
        ),
        (
            "ConfigSnapshotRegistered",
            &["kind", "content_hash", "params", "params[0]", "params[1]"],
        ),
        (
            "MandateVersionCreated",
            &[
                "mandate_version",
                "provenance",
                "provenance[0].path",
                "provenance[0].source",
                "provenance[1].path",
                "provenance[1].source",
                "record_ref",
            ],
        ),
        (
            "MandateConfirmed",
            &[
                "mandate_version",
                "confirmed_paths",
                "confirmed_paths[0]",
                "confirmed_paths[1]",
                "record_ref",
            ],
        ),
        (
            "AgentDeployed",
            &["agent_id", "mandate_version", "record_ref"],
        ),
        (
            "AgentStopped",
            &[
                "agent_id",
                "connection_id",
                "reason",
                "retired_on",
                "loss_added",
            ],
        ),
        (
            "OwnerCommandRefused",
            &["command", "reason", "effective_at"],
        ),
        (
            "AccountSnapshotRecorded",
            &[
                "status",
                "crypto_status",
                "trading_blocked",
                "account_blocked",
                "trade_suspended_by_user",
                "multiplier",
                "equity",
                "cash",
                "buying_power",
                "non_marginable_buying_power",
                "accrued_fees",
                "risk_clock",
            ],
        ),
    ];

    /// The first base of `event_type`: a chain event, the agent-stream refusal draft, or the
    /// compared snapshot draft.
    fn base_of(section: &Value, event_type: &str) -> Result<Object, String> {
        let drafted = match event_type {
            "OwnerCommandRefused" => Some("refused_stop"),
            "AccountSnapshotRecorded" => Some("snapshot_reconciled"),
            _ => None,
        };
        let case = if let Some(name) = drafted {
            named("base_draft", Value::Str(name.to_owned()))?
        } else {
            let seq = list(section, "chain")
                .iter()
                .find(|e| e.get("event_type").and_then(Value::as_str) == Some(event_type))
                .and_then(|e| e.get("seq").cloned())
                .ok_or_else(|| format!("no chain event of type {event_type}"))?;
            named("base_seq", seq)?
        };
        base(section, &case)
    }

    fn named(key: &str, value: Value) -> Result<Value, String> {
        let key = Key::new(key).map_err(|_| "key")?;
        Ok(Value::Object([(key, value)].into_iter().collect()))
    }

    fn refusal(body: Object) -> Option<(String, String)> {
        Draft::parse(&to_canonical(&Value::Object(body)))
            .err()
            .map(|e| (e.reason.code().to_owned(), e.path))
    }

    fn with_payload(mut body: Object, member: &str, value: Value) -> Result<Object, String> {
        let key = Key::new(member).map_err(|_| "key")?;
        match body.get_mut("payload") {
            Some(Value::Object(payload)) => {
                payload.insert(key, value);
                Ok(body)
            }
            _ => Err("no payload".to_owned()),
        }
    }

    /// The value at `path` below `at`, a `.`-separated path whose steps may end in one `[i]`.
    fn value_at<'a>(at: &'a mut Value, path: &str) -> Result<&'a mut Value, String> {
        let mut here = at;
        for step in path.split('.') {
            let (name, index) = match step.split_once('[') {
                Some((name, rest)) => {
                    let i = rest.trim_end_matches(']').parse::<usize>();
                    (name, Some(i.map_err(|_| format!("index in {path}"))?))
                }
                None => (step, None),
            };
            here = match here {
                Value::Object(members) => members
                    .get_mut(name)
                    .ok_or_else(|| format!("no {name} in {path}"))?,
                _ => return Err(format!("{name} in {path} is not in a record")),
            };
            if let Some(i) = index {
                here = match here {
                    Value::Array(items) => items
                        .get_mut(i)
                        .ok_or_else(|| format!("no element {i} in {path}"))?,
                    _ => return Err(format!("{name} in {path} is not a list")),
                };
            }
        }
        Ok(here)
    }

    /// Every member §9.2 requires, at every depth, is refused as `schema` at that member when it
    /// is `null`: an absent value is never a valid one (§4.2), even on a reference a writer must
    /// also drop from `artifact_refs` (#445 round 1, B2), and even inside the step-up evidence,
    /// a provenance entry, or a list (#445 round 2, C1).
    #[test]
    fn a_required_member_is_never_null() -> Result<(), String> {
        let section = section()?;
        let mut checked = 0;
        for (event_type, members) in REQUIRED {
            let control = refusal(base_of(&section, event_type)?);
            assert_eq!(control, None, "the {event_type} base appends");
            for member in members {
                let mut body = Value::Object(base_of(&section, event_type)?);
                *value_at(&mut body, &format!("payload.{member}"))? = Value::Null;
                let Value::Object(body) = body else {
                    return Err("the base is a record".to_owned());
                };
                assert_eq!(
                    refusal(body),
                    Some(("schema".to_owned(), format!("payload.{member}"))),
                    "{event_type}.{member} = null"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 56);
        let mut body = Value::Object(base_of(&section, "MandateVersionCreated")?);
        let Value::Object(entry) = value_at(&mut body, "payload.provenance[1]")? else {
            return Err("a provenance entry is a record".to_owned());
        };
        let span = Key::new("quoted_span").map_err(|_| "key")?;
        entry.insert(span, Value::Str("the owner's words".to_owned()));
        let Value::Object(body) = body else {
            return Err("the base is a record".to_owned());
        };
        assert_eq!(
            refusal(body),
            Some((
                "schema".to_owned(),
                "payload.provenance[1].quoted_span".to_owned()
            )),
            "a second provenance entry is closed too (#445 round 3, D1)"
        );
        Ok(())
    }

    /// `OwnerCommandRefused.effective_at` is a §4.7 timestamp: other text is `non_canonical`, as
    /// its sibling `step_up.authenticated_at` is (#445 round 1, B1).
    #[test]
    fn a_refusal_time_is_a_timestamp() -> Result<(), String> {
        let section = section()?;
        let body = with_payload(
            base_of(&section, "OwnerCommandRefused")?,
            "effective_at",
            Value::Str("2026-09-21 20:30:00".to_owned()),
        )?;
        assert_eq!(
            refusal(body),
            Some((
                "non_canonical".to_owned(),
                "payload.effective_at".to_owned()
            ))
        );
        Ok(())
    }

    /// A payload that breaks two rules is refused with the lower-numbered one (§9.1's report
    /// order, which §9.2 keeps): rule 20 before 21 on one snapshot registration, and subject rule 26
    /// before copy rule 27 on one refusal (#445 round 1, B3). Within rule 21, the first offending
    /// member in the spec's order: `model_id`, `model_version`, `admits_instruments`, then
    /// `params` (#445 round 2, C2; round 3, D2).
    #[test]
    fn the_lower_numbered_rule_is_reported_first() -> Result<(), String> {
        let section = section()?;
        let fee = list(&section, "chain")
            .iter()
            .find(|e| {
                e.get("body")
                    .and_then(|b| b.get("payload"))
                    .and_then(|p| p.get("kind"))
                    .and_then(Value::as_str)
                    == Some("fee_config")
            })
            .and_then(|e| e.get("seq").cloned())
            .ok_or("a fee_config registration")?;
        let fee = base(&section, &named("base_seq", fee)?)?;
        let unsorted = Value::Array(vec![
            Value::Str("z_entry".to_owned()),
            Value::Str("lookback_bars".to_owned()),
        ]);
        let named_model = with_payload(fee.clone(), "model_id", Value::Str("pairs".to_owned()))?;
        let both = with_payload(
            named_model.clone(),
            "model_version",
            Value::Str("1".to_owned()),
        )?;
        assert_eq!(
            refusal(both),
            Some(("schema".to_owned(), "payload.model_id".to_owned())),
            "rule 21 reports model_id before model_version (#445 round 2, C2)"
        );
        let sorted = Value::Array(vec![Value::Str("lookback_bars".to_owned())]);
        let with_params = with_payload(named_model, "params", sorted)?;
        assert_eq!(
            refusal(with_params),
            Some(("schema".to_owned(), "payload.model_id".to_owned())),
            "rule 21 reports a model member before params (#445 round 2, C2)"
        );
        let versioned = with_payload(fee.clone(), "model_version", Value::Str("1".to_owned()))?;
        let admitting = with_payload(versioned, "admits_instruments", Value::Bool(false))?;
        assert_eq!(
            refusal(admitting),
            Some(("schema".to_owned(), "payload.model_version".to_owned())),
            "rule 21 reports model_version before admits_instruments (#445 round 3, D2)"
        );
        let body = with_payload(fee, "params", unsorted)?;
        assert_eq!(
            refusal(body),
            Some(("non_canonical".to_owned(), "payload.params".to_owned())),
            "rule 20 (order) before rule 21 (params only on a model)"
        );
        let mut refused = with_payload(
            base_of(&section, "OwnerCommandRefused")?,
            "command",
            Value::Str("acknowledge".to_owned()),
        )?;
        let cause = Key::new("causation_id").map_err(|_| "key")?;
        refused.insert(cause, Value::Null);
        assert_eq!(
            refusal(refused),
            Some(("stream_mismatch".to_owned(), "payload.command".to_owned())),
            "rule 26 (stream) before rule 27 (cause)"
        );
        Ok(())
    }

    /// Rule 28: `not_independent` is the executor's refusal of an acknowledgment, so it is accepted on
    /// the account stream and refused at `payload.reason` on the agent stream, where rule 26 has
    /// already let a resume or Stop through; a step-up reason stays accepted on both. The report
    /// order is pinned both ways: a command on the wrong stream is rule 26's (`payload.command`)
    /// even with `not_independent`, and an uncaused `not_independent` on the agent stream is rule
    /// 28's (`payload.reason`), not rule 27's (`causation_id`).
    #[test]
    fn not_independent_is_an_account_stream_refusal_only() -> Result<(), String> {
        let payload = |command: &str, reason: &str| -> Result<Value, String> {
            Ok(Value::Object(
                [
                    ("command", command),
                    ("reason", reason),
                    ("effective_at", "2026-09-22T13:00:00.000000000Z"),
                ]
                .into_iter()
                .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
                .collect::<Result<_, String>>()?,
            ))
        };
        let cause = Value::Str("01J8ZNB00000000000000000C6".to_owned());
        let at_reason = Some(("stream_mismatch", "payload.reason"));
        let at_command = Some(("stream_mismatch", "payload.command"));
        for (stream, command, reason, caused, verdict) in [
            ("acct:ws_1:a1", "acknowledge", "not_independent", true, None),
            (
                "agent:ws_1:agent_a",
                "stop",
                "not_independent",
                true,
                at_reason,
            ),
            (
                "agent:ws_1:agent_a",
                "resume",
                "not_independent",
                true,
                at_reason,
            ),
            ("agent:ws_1:agent_a", "stop", "step_up_stale", true, None),
            ("acct:ws_1:a1", "acknowledge", "step_up_reused", true, None),
            ("acct:ws_1:a1", "stop", "not_independent", true, at_command),
            (
                "agent:ws_1:agent_a",
                "acknowledge",
                "not_independent",
                true,
                at_command,
            ),
            (
                "agent:ws_1:agent_a",
                "stop",
                "not_independent",
                false,
                at_reason,
            ),
        ] {
            let stream = StreamId::parse(stream).ok_or("a stream id")?;
            let got = subject_and_copy(
                "OwnerCommandRefused",
                &stream,
                &payload(command, reason)?,
                caused.then_some(&cause),
            )
            .err()
            .map(|e| (e.reason.code(), e.path));
            assert_eq!(
                got,
                verdict.map(|(code, path)| (code, path.to_owned())),
                "{stream} {command} {reason} caused={caused}"
            );
        }
        Ok(())
    }

    /// Rule 26 names the account and agent streams only; on any other a refused command matches
    /// nothing, so it is refused (DEC-303 item 12). No other stream routes the type today.
    #[test]
    fn a_refusal_off_its_two_streams_is_refused() -> Result<(), String> {
        let payload = Value::Object(
            [
                ("command", "stop"),
                ("reason", "step_up_stale"),
                ("effective_at", "2026-09-21T20:30:00.000000000Z"),
            ]
            .into_iter()
            .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
            .collect::<Result<_, String>>()?,
        );
        let cause = Value::Str("01J8ZNB00000000000000000C6".to_owned());
        for (stream, verdict) in [
            ("agent:ws_1:agent_a", None),
            ("acct:ws_1:a1", Some("payload.command")),
            ("ctl:ws_1", Some("payload.command")),
            ("clock:ws_1", Some("payload.command")),
        ] {
            let stream = StreamId::parse(stream).ok_or("a stream id")?;
            let got = subject_and_copy("OwnerCommandRefused", &stream, &payload, Some(&cause))
                .err()
                .map(|e| (e.reason.code(), e.path));
            assert_eq!(
                got,
                verdict.map(|path| ("stream_mismatch", path.to_owned())),
                "{stream}"
            );
        }
        Ok(())
    }

    /// A snapshot at any `schema_version` but 1 is refused as `unknown_schema`, as every other §9.2
    /// type is: the snapshot reaches its schema through `schema`'s version gate, never around it
    /// (#467 round 1, m3).
    #[test]
    fn a_snapshot_at_another_schema_version_is_an_unknown_schema() -> Result<(), String> {
        let section = section()?;
        let mut body = base_of(&section, "AccountSnapshotRecorded")?;
        let version = Key::new("schema_version").map_err(|_| "key")?;
        let two = Int::new(2).ok_or("an integer")?;
        body.insert(version, Value::Int(two));
        assert_eq!(
            refusal(body),
            Some(("unknown_schema".to_owned(), "payload".to_owned()))
        );
        Ok(())
    }

    /// `/` is one empty reference token: RFC 6901 grammar admits it, and §9.2's "one or more
    /// `/`-prefixed reference tokens" does too, so it is accepted; only the empty pointer is not
    /// (DEC-303 item 13).
    #[test]
    fn a_pointer_of_one_empty_token_is_accepted() -> Result<(), String> {
        let section = section()?;
        let entry = |path: &str| -> Result<Value, String> {
            Ok(Value::Array(vec![Value::Object(
                [("path", path), ("source", "user_entered")]
                    .into_iter()
                    .map(|(k, v)| Ok((Key::new(k).map_err(|_| "key")?, Value::Str(v.to_owned()))))
                    .collect::<Result<_, String>>()?,
            )]))
        };
        let created = |path: &str| -> Result<Option<(String, String)>, String> {
            let body = base_of(&section, "MandateVersionCreated")?;
            Ok(refusal(with_payload(body, "provenance", entry(path)?)?))
        };
        assert_eq!(created("/")?, None);
        assert_eq!(
            created("")?,
            Some((
                "non_canonical".to_owned(),
                "payload.provenance[0].path".to_owned()
            ))
        );
        Ok(())
    }
}
