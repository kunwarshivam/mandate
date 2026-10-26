//! Versions, confirmation, and deployment as the CLI judges and records them (§3.11 item 11,
//! [DEC-2101](../../../docs/project/decisions/DEC-2101.md); `mandate version create`, `version
//! confirm`, `agent deploy`, DEC-530): the control-stream folds, the V-rule check, the records, the
//! confirmation screen, and the step-up digests. Pure: the routes in [`crate::version_routes`] read
//! the journal and the store and hand their rows here.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Key, Object, Value, parse, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_spec::context::{
    AgentId, ContextArgs, JournaledFact, Membership, unrecorded_stop_limit_asset_classes,
};
use mandate_spec::document::ConnectionId;
use mandate_spec::validate::validate;
use mandate_spec::{Mandate, MandateVersion, Pointer, ValidationContext, Violation};
use mandate_time::Date;

pub const CREATED: &str = "MandateVersionCreated";
pub const CONFIRMED: &str = "MandateConfirmed";
pub const DEPLOYED: &str = "AgentDeployed";
const STOPPED: &str = "AgentStopped";
const ESTABLISHED: &str = "ConnectionEstablished";
const REVOKED: &str = "ConnectionRevoked";

/// The document's members that are not envelope paths (mandate spec §4.1 V-020).
const SYSTEM_FIELDS: [&str; 2] = ["mandate_schema_version", "source_text_ref"];

/// A first version's classification: no earlier version bounds any path (DEC-2101 item 4).
pub const FIRST_VERSION: &str = "risk_increasing";

/// Why a version cannot be recorded, confirmed, or deployed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// These V-rules fail, by code, ascending.
    Rules(Vec<&'static str>),
    /// A pinned instrument no registered snapshot names with its symbol.
    Unregistered,
    /// A control-stream record the spec's fold cannot read, or a store that cannot be read.
    Unreadable,
}

/// `sha256:` and the hex digest of `bytes`.
pub fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

/// The payload of a stored event, or `Null` for one that does not parse.
pub fn payload(row: &StoredEvent) -> Value {
    parse(&row.body)
        .ok()
        .and_then(|event| event.get("payload").cloned())
        .unwrap_or(Value::Null)
}

fn member(row: &StoredEvent, name: &str) -> Option<String> {
    payload(row)
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The document's envelope paths: its top-level members but the system fields, ascending.
pub fn envelope_paths(document: &Value) -> Vec<String> {
    let members = document.as_object().into_iter().flat_map(|o| o.keys());
    members
        .map(|k| k.as_str())
        .filter(|k| !SYSTEM_FIELDS.contains(k))
        .map(|k| format!("/{k}"))
        .collect()
}

/// The latest event of `event_type` naming `version`.
pub fn latest<'a>(
    control: &'a [StoredEvent],
    event_type: &str,
    version: &str,
) -> Option<&'a StoredEvent> {
    control.iter().rev().find(|row| {
        row.event_type == event_type && member(row, "mandate_version").as_deref() == Some(version)
    })
}

/// The control stream's last `MandateConfirmed` and the version it names.
pub fn last_confirmed(control: &[StoredEvent]) -> Option<(&StoredEvent, String)> {
    let row = control
        .iter()
        .rev()
        .find(|row| row.event_type == CONFIRMED)?;
    Some((row, member(row, "mandate_version")?))
}

/// The agent's active deployment: its latest `AgentDeployed`, when no `AgentStopped` for it is
/// later, and the version it names.
pub fn active<'a>(control: &'a [StoredEvent], agent: &str) -> Option<(&'a StoredEvent, String)> {
    let row = last_of(control, agent)?;
    if row.event_type != DEPLOYED {
        return None;
    }
    Some((row, member(row, "mandate_version")?))
}

/// The agent's latest `AgentDeployed` or `AgentStopped`.
fn last_of<'a>(control: &'a [StoredEvent], agent: &str) -> Option<&'a StoredEvent> {
    control.iter().rev().find(|row| {
        matches!(row.event_type.as_str(), DEPLOYED | STOPPED)
            && member(row, "agent_id").as_deref() == Some(agent)
    })
}

/// Whether an `AgentStopped` names `agent`: its id is never deployed again (DEC-1173 item 1).
pub fn stopped(control: &[StoredEvent], agent: &str) -> bool {
    last_of(control, agent).is_some_and(|row| row.event_type == STOPPED)
}

/// The accounts each version-2 `ConnectionEstablished` of `connection` names, with its
/// environment: none for a connection never established at version 2 (DEC-1171 item 3).
fn accounts(control: &[StoredEvent], connection: &str) -> BTreeSet<(String, String)> {
    control
        .iter()
        .filter(|row| row.event_type == ESTABLISHED && row.schema_version == 2)
        .filter(|row| member(row, "connection_id").as_deref() == Some(connection))
        .filter_map(|row| Some((member(row, "account_ref")?, member(row, "environment")?)))
        .collect()
}

/// DEC-2809 item 2: whether an agent other than `agent`, with a version-2 or version-3
/// `AgentDeployed` on a connection naming an account `connection` names, has no `AgentStopped`.
/// The executor's risk fold reads one agent on an account (DEC-2804 item 8).
pub fn another_on_account(control: &[StoredEvent], connection: &str, agent: &str) -> bool {
    let ours = accounts(control, connection);
    if ours.is_empty() {
        return false;
    }
    let stopped: BTreeSet<String> = control
        .iter()
        .filter(|row| row.event_type == STOPPED)
        .filter_map(|row| member(row, "agent_id"))
        .collect();
    control
        .iter()
        .filter(|row| row.event_type == DEPLOYED && matches!(row.schema_version, 2 | 3))
        .filter_map(|row| Some((member(row, "agent_id")?, member(row, "connection_id")?)))
        .any(|(other, named)| {
            other != agent
                && !stopped.contains(&other)
                && !accounts(control, &named).is_disjoint(&ours)
        })
}

/// Journal spec v0.54 §9.24 rule 201 for a deployment on `connection`: a version-2
/// `ConnectionEstablished` of it is on the control stream, and its latest establishment or
/// revocation is an establishment. A deployment is written at version 2 exactly when it holds.
pub fn established_v2(control: &[StoredEvent], connection: &str) -> bool {
    let of = |row: &&StoredEvent| member(row, "connection_id").as_deref() == Some(connection);
    let bound = control
        .iter()
        .filter(of)
        .any(|row| row.event_type == ESTABLISHED && row.schema_version == 2);
    let latest = control
        .iter()
        .rev()
        .filter(of)
        .find(|row| matches!(row.event_type.as_str(), ESTABLISHED | REVOKED));
    bound && latest.is_some_and(|row| row.event_type == ESTABLISHED)
}

/// The stored document `version` names, parsed, with the mandate it is: `None` when the store
/// does not hold it, it does not hash to `version`, or it is not a mandate.
pub fn stored(store: &dyn ArtifactSource, version: &str) -> Option<(Value, Mandate)> {
    let bytes = get_artifact(store, &ArtifactRef::parse(version)?).ok()?;
    let document = parse(&bytes).ok()?;
    let mandate = Mandate::parse(&document).ok()?;
    Some((document, mandate))
}

/// Every record as the fact the spec's fold reads (DEC-505 item 1): the control stream's, then
/// the account stream's of `connection` recorded after its last `ConnectionRevoked`, so a
/// snapshot from before a revocation never restores the equity the revocation cleared (canonical
/// instants order as text).
fn facts(
    (control, account): (&[StoredEvent], &[StoredEvent]),
    store: &dyn ArtifactSource,
    connection: &ConnectionId,
) -> Option<Vec<JournaledFact>> {
    let documents = |d: &Digest| {
        let bytes = get_artifact(store, &ArtifactRef::from_digest(*d)).ok()?;
        parse(&bytes).ok()
    };
    let mut facts = Vec::new();
    for row in control {
        let fact =
            JournaledFact::from_record(&row.event_type, &payload(row), &documents, None).ok()?;
        facts.extend(fact);
    }
    let revoked = control
        .iter()
        .rev()
        .find(|row| {
            row.event_type == "ConnectionRevoked"
                && member(row, "connection_id").as_deref() == Some(connection.as_str())
        })
        .map(|row| row.recorded_at.as_str());
    let after_revocation = account
        .iter()
        .filter(|row| revoked.is_none_or(|at| row.recorded_at.as_str() > at));
    for row in after_revocation {
        let fact = JournaledFact::from_record(
            &row.event_type,
            &payload(row),
            &documents,
            Some(connection),
        )
        .ok()?;
        facts.extend(fact);
    }
    Some(facts)
}

/// What a check folds after the journal's records: the `MandateVersionCreated` payload a version
/// being created will commit, and the paths a confirmation being judged will confirm.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pending<'a> {
    pub created: Option<&'a Value>,
    pub confirmed: Option<&'a [String]>,
}

/// Every V-rule on `mandate` for `agent`, as if `pending` were committed, in the context the control stream and `account`, the account stream of the document's
/// connection, fold to on `date`, as the CLI checks (DEC-530 items 5 and 9): the warnings' codes,
/// ascending. A version not yet deployed (`agent` empty) is checked without V-002, as the CLI
/// checks it; a deployment checks V-002 against the account's latest equity snapshot, 0 without
/// one (DEC-2101 item 6).
///
/// # Errors
/// [`Refusal::Rules`] with every failing V-code, or [`Refusal::Unreadable`].
pub fn check(
    (control, account): (&[StoredEvent], &[StoredEvent]),
    store: &dyn ArtifactSource,
    (mandate, agent): (&Mandate, &str),
    pending: Pending<'_>,
    date: Date,
) -> Result<Vec<&'static str>, Refusal> {
    let mut facts =
        facts((control, account), store, &mandate.connection_id).ok_or(Refusal::Unreadable)?;
    if let Some(created) = pending.created {
        let nothing = |_: &Digest| None;
        let fact = JournaledFact::from_record(CREATED, created, &nothing, None)
            .map_err(|_| Refusal::Unreadable)?;
        facts.extend(fact);
    }
    if let Some(paths) = pending.confirmed {
        let digest = Digest::of(&mandate.canonical_bytes().map_err(|_| Refusal::Unreadable)?);
        facts.push(JournaledFact::MandateConfirmed {
            version: MandateVersion::named(digest),
            confirmed_paths: paths.iter().map(|p| Pointer::new(p)).collect(),
        });
    }
    let args = ContextArgs {
        agent: AgentId::new(agent),
        connection_id: mandate.connection_id.clone(),
        validation_date: date,
        membership: Some(Membership {
            workspace_users: 1,
            approver_users: 1,
        }),
        independent_approval_required: false,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let mut context =
        ValidationContext::from_journal(mandate, args, &facts).map_err(|_| Refusal::Unreadable)?;
    let connection = mandate.connection_id.as_str();
    let known = control.iter().any(|row| {
        matches!(
            row.event_type.as_str(),
            "ConnectionEstablished" | "ConnectionRevoked"
        ) && member(row, "connection_id").as_deref() == Some(connection)
    });
    if !known {
        context.connection_environment = None;
        context.stop_limit_asset_classes = unrecorded_stop_limit_asset_classes(mandate);
    }
    let report = validate(mandate, &context).map_err(|_| Refusal::Unreadable)?;
    let failing: Vec<&'static str> = report
        .violations
        .iter()
        .filter(|v| **v != Violation::V002 || !agent.is_empty())
        .map(|v| v.code())
        .collect();
    if !failing.is_empty() {
        return Err(Refusal::Rules(failing));
    }
    Ok(report.warnings.iter().map(|w| w.code()).collect())
}

/// Each pinned instrument is named by the latest registered instrument snapshot whose object
/// names its asset id, with its symbol (DEC-505 item 1, DEC-523 item 5).
///
/// # Errors
/// [`Refusal::Unregistered`], or [`Refusal::Unreadable`] for a snapshot the store cannot give.
pub fn instruments(
    control: &[StoredEvent],
    store: &dyn ArtifactSource,
    document: &Value,
) -> Result<(), Refusal> {
    let mut snapshots: BTreeMap<String, String> = BTreeMap::new();
    let registrations = control.iter().filter(|row| {
        row.event_type == "ConfigSnapshotRegistered"
            && member(row, "kind").as_deref() == Some("instrument_snapshot")
    });
    for row in registrations {
        let object = member(row, "content_hash")
            .and_then(|hash| ArtifactRef::parse(&hash))
            .and_then(|r| get_artifact(store, &r).ok())
            .and_then(|bytes| parse(&bytes).ok())
            .ok_or(Refusal::Unreadable)?;
        let text = |name| object.get(name).and_then(Value::as_str).map(str::to_owned);
        if let (Some(id), Some(symbol)) = (text("instrument_id"), text("symbol")) {
            snapshots.insert(id, symbol);
        }
    }
    let pinned = document
        .get("universe")
        .and_then(|u| u.get("pinned_instruments"))
        .and_then(Value::as_array)
        .unwrap_or_default();
    for instrument in pinned {
        let text = |name| instrument.get(name).and_then(Value::as_str);
        let registered = text("asset_id").and_then(|id| snapshots.get(id));
        if registered.is_none() || registered.map(String::as_str) != text("symbol") {
            return Err(Refusal::Unregistered);
        }
    }
    Ok(())
}

/// An object of `members`; `None` only for a name that is not a valid key.
pub fn object(members: Vec<(&str, Value)>) -> Option<Value> {
    let mut object = Object::new();
    for (name, value) in members {
        object.insert(Key::new(name).ok()?, value);
    }
    Some(Value::Object(object))
}

pub fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub fn texts(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|v| text(v)).collect())
}

/// What the confirmation screen shows of a new mandate (DEC-2101 item 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub required_paths: Vec<String>,
    pub warnings: Vec<&'static str>,
    /// The SHA-256 of the canonical object of the seven digested members, `sha256:<hex>`.
    pub digest: String,
}

/// The screen for `version`, a new mandate whose document has `required_paths` and whose
/// validation now warns `warnings`.
pub fn screen(version: &str, required_paths: Vec<String>, warnings: Vec<&'static str>) -> Screen {
    let paths: Vec<&str> = required_paths.iter().map(String::as_str).collect();
    let digested = object(vec![
        ("mandate_version", text(version)),
        ("agent_id", Value::Null),
        ("base_version", Value::Null),
        ("classification", text(FIRST_VERSION)),
        ("required_paths", texts(&paths)),
        ("warnings", texts(&warnings)),
        ("step_up_required", Value::Bool(true)),
    ]);
    let digest = digested.map_or_else(String::new, |d| reference(&to_canonical(&d)));
    Screen {
        required_paths,
        warnings,
        digest,
    }
}

/// §5.1's step-up digest for a new mandate: the SHA-256 of `{agent_id, base_version,
/// mandate_version}` with no agent and no base.
pub fn confirm_digest(version: &str) -> Option<Digest> {
    let bound = object(vec![
        ("agent_id", Value::Null),
        ("base_version", Value::Null),
        ("mandate_version", text(version)),
    ])?;
    Some(Digest::of(&to_canonical(&bound)))
}

/// A deployment's step-up digest: the SHA-256 of `{agent_id, mandate_version}` (DEC-2101 item 6).
pub fn deploy_digest(agent: &str, version: &str) -> Option<Digest> {
    let bound = object(vec![
        ("agent_id", text(agent)),
        ("mandate_version", text(version)),
    ])?;
    Some(Digest::of(&to_canonical(&bound)))
}
