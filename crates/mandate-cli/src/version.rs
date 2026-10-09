//! `mandate version create` and `mandate version confirm` (first paper trade brief, D2a; DEC-530).
//! Each stores the mandate document and its mandate spec §10 record in the artifact store, then
//! commits exactly one control-stream event naming them (journal spec §9.2, DEC-155 item 5). Both
//! are paper only, and a confirmation takes a `cli_confirm` code bound to the version shown. What
//! `version confirm` prints before the code is typed comes with the commands, D2c.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, Environment, StoredEvent,
    get_artifact,
};
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::validate::validate;
use mandate_spec::{Mandate, MandateVersion, Pointer, ValidationContext, Violation};

use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Ids, Now, Owner, Repeat, Shape,
    Submitted, code_of, commit_choice, control_stream, decide, object, seconds, text,
};

pub(crate) const CREATED: &str = "MandateVersionCreated";
pub(crate) const CONFIRMED: &str = "MandateConfirmed";

/// The document's members that are not envelope paths (mandate spec §4.1 V-020).
const SYSTEM_FIELDS: [&str; 2] = ["mandate_schema_version", "source_text_ref"];

pub(crate) fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

fn store_error(e: &ArtifactError) -> ControlError {
    ControlError::Journal(format!("the artifact store: {e}"))
}

fn reference(digest: Digest) -> String {
    format!("sha256:{}", digest.to_hex())
}

pub(crate) fn paper_only(owner: &Owner) -> Result<(), ControlError> {
    if owner.environment == Environment::Paper {
        Ok(())
    } else {
        Err(refused("paper_only"))
    }
}

/// One control-stream event as a command reads it.
pub(crate) struct Row {
    pub(crate) seq: u64,
    event_id: String,
    pub(crate) event_type: String,
    payload: Value,
}

impl Row {
    pub(crate) fn member(&self, name: &str) -> Option<&str> {
        self.payload.get(name).and_then(Value::as_str)
    }

    pub(crate) fn submitted(&self) -> Submitted {
        Submitted {
            event_id: self.event_id.clone(),
            seq: self.seq,
        }
    }
}

pub(crate) fn rows(journal: &dyn ControlJournal, owner: &Owner) -> Result<Vec<Row>, ControlError> {
    let stream = control_stream(owner)?;
    journal.rows(&stream)?.into_iter().map(row).collect()
}

fn row(stored: StoredEvent) -> Result<Row, ControlError> {
    let envelope =
        parse(&stored.body).map_err(|e| ControlError::Journal(format!("a stored event: {e:?}")))?;
    let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);
    Ok(Row {
        seq: stored.seq,
        event_id: stored.event_id,
        event_type: stored.event_type,
        payload,
    })
}

/// The latest event of `event_type` naming `version`.
pub(crate) fn latest<'a>(rows: &'a [Row], event_type: &str, version: &str) -> Option<&'a Row> {
    rows.iter()
        .rev()
        .find(|r| r.event_type == event_type && r.member("mandate_version") == Some(version))
}

/// The document's envelope paths: its top-level members but the system fields, ascending.
pub(crate) fn envelope_paths(document: &Value) -> Vec<String> {
    let members = document.as_object().into_iter().flat_map(|o| o.keys());
    members
        .map(|k| k.as_str())
        .filter(|k| !SYSTEM_FIELDS.contains(k))
        .map(|k| format!("/{k}"))
        .collect()
}

/// What one command commits: its event type, the payload members it chose, and whether it is
/// stepped up.
pub(crate) type Event = (&'static str, Vec<(&'static str, Value)>, bool);

/// Stores `record` and commits `event` naming `version` and the record, both in its
/// `artifact_refs`, and binding `config_refs`, unless a re-run finds it committed (DEC-290).
pub(crate) fn commit(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    (event_type, mut key, stepped_up): Event,
    (version, record, config_refs): (&str, &Value, Vec<(&'static str, Value)>),
    now: Now,
) -> Result<Submitted, ControlError> {
    let bytes = to_canonical(record);
    let record_ref = reference(Digest::of(&bytes));
    key.push(("mandate_version", text(version)));
    key.push(("record_ref", text(&record_ref)));
    let confirmed = Confirmation::Fixed(stepped_up);
    let decided = match decide(
        journal,
        owner,
        event_type,
        key,
        confirmed,
        Repeat::FindsEarlier,
    )? {
        Decision::Committed(earlier) => return Ok(earlier),
        Decision::Fresh(decided) => decided,
    };
    store.put_artifact(&bytes).map_err(|e| store_error(&e))?;
    let mut artifact_refs = vec![version.to_owned(), record_ref];
    artifact_refs.sort();
    let shape = Shape {
        schema_version: 1,
        artifact_refs,
        config_refs,
    };
    commit_choice(journal, owner, decided, shape, now)
}

/// Stores `document`, a mandate, in canonical form under its version (mandate spec §9.1), and its
/// record, then commits one `MandateVersionCreated`, every envelope path `user_entered` (DEC-530).
///
/// # Errors
/// [`ControlError::Refused`] with a DEC-530 code, having written nothing; [`ControlError::Journal`].
pub fn create(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    document: &[u8],
    now: Now,
) -> Result<Submitted, ControlError> {
    paper_only(owner)?;
    let value = parse(document).map_err(|_| refused("mandate_invalid"))?;
    let mandate = Mandate::parse(&value).map_err(|_| refused("mandate_invalid"))?;
    if value.get("environment").and_then(Value::as_str) != Some("paper") {
        return Err(refused("mandate_not_paper"));
    }
    let canonical = mandate
        .canonical_bytes()
        .map_err(|_| refused("mandate_invalid"))?;
    let version = reference(Digest::of(&canonical));
    if let Some(earlier) = latest(&rows(journal, owner)?, CREATED, &version) {
        return Ok(earlier.submitted());
    }
    let provenance = envelope_paths(&value)
        .iter()
        .map(|path| object(vec![("path", text(path)), ("source", text("user_entered"))]))
        .collect::<Result<Vec<_>, _>>()?;
    let record = object(vec![
        ("kind", text("mandate_version_record")),
        ("mandate_version", text(&version)),
        ("user", text(&owner.user)),
    ])?;
    store
        .put_artifact(&canonical)
        .map_err(|e| store_error(&e))?;
    let event = (
        CREATED,
        vec![("provenance", Value::Array(provenance))],
        false,
    );
    commit(
        journal,
        store,
        owner,
        event,
        (&version, &record, Vec::new()),
        now,
    )
}

/// The stored document `version` names, parsed, and the mandate it is.
pub(crate) fn stored(
    store: &dyn ArtifactSource,
    version: &str,
) -> Result<(Value, Mandate), ControlError> {
    let reference = ArtifactRef::parse(version).ok_or_else(|| refused("version_unknown"))?;
    let bytes = get_artifact(store, &reference).map_err(|e| match e {
        ArtifactError::Missing => refused("document_missing"),
        ArtifactError::Corrupt => refused("document_corrupt"),
        other => store_error(&other),
    })?;
    let value = parse(&bytes).map_err(|_| refused("document_corrupt"))?;
    let mandate = Mandate::parse(&value).map_err(|_| refused("document_corrupt"))?;
    Ok((value, mandate))
}

/// Every control-stream record as the fact the spec's fold reads (DEC-505 item 1), or
/// `control_stream_invalid` for one that cannot be mapped.
fn facts(rows: &[Row], store: &dyn ArtifactSource) -> Result<Vec<JournaledFact>, ControlError> {
    let documents = |d: &Digest| {
        let bytes = get_artifact(store, &ArtifactRef::from_digest(*d)).ok()?;
        parse(&bytes).ok()
    };
    let mut facts = Vec::new();
    for row in rows {
        let fact = JournaledFact::from_record(&row.event_type, &row.payload, &documents, None)
            .map_err(|_| refused("control_stream_invalid"))?;
        facts.extend(fact);
    }
    Ok(facts)
}

/// Every V-rule but V-002 on `mandate` for `agent` (an id no deployment names, for a
/// confirmation), as if `confirming` were confirmed when it is given, in the context the control
/// stream folds to (DEC-530 items 5 and 9); the warnings, by code.
pub(crate) fn check_rules(
    rows: &[Row],
    store: &dyn ArtifactSource,
    (mandate, agent): (&Mandate, &str),
    confirming: Option<(Digest, &[String])>,
    now: Now,
) -> Result<Vec<&'static str>, ControlError> {
    let mut facts = facts(rows, store)?;
    if let Some((digest, paths)) = confirming {
        facts.push(JournaledFact::MandateConfirmed {
            version: MandateVersion::named(digest),
            confirmed_paths: paths.iter().map(|p| Pointer::new(p)).collect(),
        });
    }
    let membership = Membership {
        workspace_users: 1,
        approver_users: 1,
    };
    let args = ContextArgs {
        agent: AgentId::new(agent),
        connection_id: mandate.connection_id.clone(),
        validation_date: now.at.date(),
        membership: Some(membership),
        independent_approval_required: false,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let invalid = |_| refused("version_invalid");
    let mut context = ValidationContext::from_journal(mandate, args, &facts).map_err(invalid)?;
    let connection = mandate.connection_id.as_str();
    let known = rows.iter().any(|r| {
        matches!(
            r.event_type.as_str(),
            "ConnectionEstablished" | "ConnectionRevoked"
        ) && r.member("connection_id") == Some(connection)
    });
    if !known {
        context.connection_environment = None;
    }
    let report = validate(mandate, &context).map_err(invalid)?;
    if report.violations.iter().any(|v| *v != Violation::V002) {
        return Err(refused("version_invalid"));
    }
    Ok(report.warnings.iter().map(|w| w.code()).collect())
}

/// Each pinned instrument is named by the latest registered instrument snapshot whose object
/// names its asset id, with its symbol (DEC-505 item 1, DEC-523 item 5).
pub(crate) fn check_instruments(
    rows: &[Row],
    store: &dyn ArtifactSource,
    document: &Value,
) -> Result<(), ControlError> {
    let mut snapshots: BTreeMap<String, String> = BTreeMap::new();
    let registrations = rows.iter().filter(|r| {
        r.event_type == "ConfigSnapshotRegistered"
            && r.member("kind") == Some("instrument_snapshot")
    });
    for row in registrations {
        let object = row
            .member("content_hash")
            .and_then(ArtifactRef::parse)
            .and_then(|r| get_artifact(store, &r).ok())
            .and_then(|bytes| parse(&bytes).ok())
            .ok_or_else(|| refused("control_stream_invalid"))?;
        let member = |name| object.get(name).and_then(Value::as_str).map(str::to_owned);
        if let (Some(id), Some(symbol)) = (member("instrument_id"), member("symbol")) {
            snapshots.insert(id, symbol);
        }
    }
    let pinned = document
        .get("universe")
        .and_then(|u| u.get("pinned_instruments"))
        .and_then(Value::as_array)
        .unwrap_or_default();
    for instrument in pinned {
        let member = |name| instrument.get(name).and_then(Value::as_str);
        let registered = member("asset_id").and_then(|id| snapshots.get(id));
        if registered.is_none() || registered.map(String::as_str) != member("symbol") {
            return Err(refused("instrument_unregistered"));
        }
    }
    Ok(())
}

/// Confirms `version` when `code` is the one shown for it (DEC-530 item 4): stores the record with
/// fresh `cli_confirm` evidence, then commits one `MandateConfirmed` naming every envelope path.
///
/// # Errors
/// As [`create`].
pub fn confirm(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    ids: &mut dyn Ids,
    owner: &Owner,
    version: &str,
    code: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    paper_only(owner)?;
    let rows = rows(journal, owner)?;
    if latest(&rows, CREATED, version).is_none() {
        return Err(refused("version_unknown"));
    }
    let (document, mandate) = stored(store, version)?;
    let gesture = object(vec![
        ("gesture", text("mandate_confirm")),
        ("mandate_version", text(version)),
    ])?;
    if code != code_of(&gesture) {
        return Err(refused("code_mismatch"));
    }
    let paths = envelope_paths(&document);
    let digest = Digest::of(&to_canonical(&document));
    let warnings = check_rules(&rows, store, (&mandate, ""), Some((digest, &paths)), now)?;
    check_instruments(&rows, store, &document)?;
    let last = rows.iter().rev().find(|r| r.event_type == CONFIRMED);
    if let Some(earlier) = last.filter(|r| r.member("mandate_version") == Some(version)) {
        return Ok(earlier.submitted());
    }
    let step_up = object(vec![
        ("assertion_id", text(&ids.assertion_id())),
        ("authenticated_at", seconds(now.secs)?),
        ("method", text("cli_confirm")),
    ])?;
    let record = object(vec![
        ("kind", text("mandate_confirmation_record")),
        ("mandate_version", text(version)),
        ("step_up", step_up),
        ("user", text(&owner.user)),
        (
            "warnings",
            Value::Array(warnings.into_iter().map(text).collect()),
        ),
    ])?;
    let paths = Value::Array(paths.iter().map(|p| text(p)).collect());
    let event = (CONFIRMED, vec![("confirmed_paths", paths)], true);
    commit(
        journal,
        store,
        owner,
        event,
        (version, &record, Vec::new()),
        now,
    )
}
