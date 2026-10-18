//! Control-stream rows as the journal stores them (journal spec §3, §9.12), built record by record
//! for the membership parser's tests (E21-55).

use std::collections::{BTreeMap, BTreeSet};

use mandate_api_server::members::{ControlLookup, RowsRefused, membership_records};
use mandate_canon::{Digest, encode_ulid};
use mandate_identity::{
    LookupFailed, Membership, MembershipEvent, MembershipLookup, MembershipQuery, MembershipRecord,
    MembershipState, PrincipalId, Role, Scope,
};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use serde_json::{Value as Json, json};

use super::{WORKSPACE, at, hosted};

pub const T0: i64 = 1_791_000_000;
pub const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";

pub fn hosted_stream() -> String {
    format!("ctl:{WORKSPACE}")
}

pub fn other_stream() -> String {
    format!("ctl:{OTHER_WORKSPACE}")
}

pub fn scope() -> Scope {
    Scope::Workspace {
        org: hosted().org,
        workspace: hosted().workspace,
    }
}

pub fn roles(list: &[Role]) -> BTreeSet<Role> {
    list.iter().copied().collect()
}

/// Role codes in the ascending byte order rule 96 writes them.
fn codes(roles: &BTreeSet<Role>) -> Vec<&'static str> {
    let mut codes: Vec<&str> = roles.iter().filter_map(|role| role.code()).collect();
    codes.sort_unstable();
    codes
}

fn time(secs: i64) -> String {
    at(secs).to_string()
}

pub fn activation(member: PrincipalId, granted: &BTreeSet<Role>, ends: i64) -> Json {
    json!({
        "member": encode_ulid(member.0), "invitation": null, "reason": "founding",
        "roles": codes(granted), "independent_approval_required": false,
        "cool_off_ends_at": time(ends), "session_ref": null,
    })
}

pub fn change(member: PrincipalId, added: &BTreeMap<Role, i64>, removed: &BTreeSet<Role>) -> Json {
    let mut added: Vec<(&str, i64)> = added
        .iter()
        .filter_map(|(role, ends)| role.code().map(|code| (code, *ends)))
        .collect();
    added.sort_unstable();
    let added: Vec<Json> = added
        .into_iter()
        .map(|(code, ends)| json!({"role": code, "cool_off_ends_at": time(ends)}))
        .collect();
    json!({
        "member": encode_ulid(member.0), "added": added, "removed": codes(removed),
        "independent_approval_required": false, "session_ref": null,
    })
}

pub fn reactivation(member: PrincipalId, kept: &BTreeSet<Role>, ends: i64) -> Json {
    json!({
        "member": encode_ulid(member.0), "roles": codes(kept),
        "independent_approval_required": false, "cool_off_ends_at": time(ends),
        "session_ref": null,
    })
}

pub fn member(member: PrincipalId) -> Json {
    json!({"member": encode_ulid(member.0), "session_ref": null})
}

/// The envelope of a `kind` record on `stream` in `environment` at `secs`, at schema version 1.
pub fn envelope(stream: &str, seq: u64, env: &str, kind: &str, secs: i64, payload: Json) -> Json {
    json!({
        "envelope_version": 1, "environment": env, "event_id": encode_ulid(u128::from(seq)),
        "stream_id": stream, "seq": seq, "event_type": kind, "schema_version": 1,
        "event_time": time(secs), "clock_source": "local", "payload": payload,
    })
}

/// `envelope` stored at `seq`, canonical, and recorded long after any instant a test reads at.
pub fn stored(seq: u64, envelope: &Json) -> StoredEvent {
    let text = |key: &str| envelope[key].as_str().unwrap_or_default().to_owned();
    let draft = serde_json::to_vec(envelope).expect("JSON");
    let body = mandate_canon::to_canonical(&mandate_canon::parse(&draft).expect("canonical"));
    StoredEvent {
        stream_id: text("stream_id"),
        seq,
        event_id: text("event_id"),
        event_type: text("event_type"),
        schema_version: envelope["schema_version"].as_u64().unwrap_or_default(),
        environment: text("environment"),
        recorded_at: time(T0 + 10_000),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// Rows read from the hosted control stream, `seq` counted across every row.
pub struct Rows {
    pub rows: Vec<StoredEvent>,
}

impl Rows {
    /// The hosted stream, opened in `paper`.
    pub fn opened() -> Self {
        let mut rows = Self { rows: Vec::new() };
        rows.hosted(
            "StreamOpened",
            T0 - 1_000,
            json!({"stream_type": "control"}),
        );
        rows
    }

    pub fn next(&self) -> u64 {
        u64::try_from(self.rows.len()).expect("a count") + 1
    }

    pub fn push(&mut self, stream: &str, env: &str, kind: &str, secs: i64, payload: Json) {
        self.edited(kind, secs, |e| {
            e["stream_id"] = json!(stream);
            e["environment"] = json!(env);
            e["payload"] = payload;
        });
    }

    pub fn hosted(&mut self, kind: &str, secs: i64, payload: Json) {
        self.push(&hosted_stream(), "paper", kind, secs, payload);
    }

    /// A hosted `paper` row of `kind` at `secs`, its envelope changed by `edit`.
    pub fn edited(&mut self, kind: &str, secs: i64, edit: impl FnOnce(&mut Json)) {
        let seq = self.next();
        let mut envelope = envelope(&hosted_stream(), seq, "paper", kind, secs, json!({}));
        edit(&mut envelope);
        self.rows.push(stored(seq, &envelope));
    }

    pub fn parsed(&self) -> Result<Vec<MembershipRecord>, RowsRefused> {
        membership_records(hosted().workspace, &self.rows)
    }

    /// The lookup at `now` over the records parsed, which a test of a valid stream needs to parse.
    pub fn lookup(&self, now: i64) -> ControlLookup {
        let records = self
            .parsed()
            .unwrap_or_else(|e| panic!("the rows parse, not {e:?}"));
        ControlLookup::new(hosted(), Ok(records), at(now))
    }

    /// The lookup at `now` over whatever the parser answers.
    pub fn failed(&self, now: i64) -> ControlLookup {
        ControlLookup::new(hosted(), self.parsed().map_err(|_| LookupFailed), at(now))
    }
}

pub fn record(seq: u64, secs: i64, event: MembershipEvent) -> MembershipRecord {
    MembershipRecord::new(Seal::grant(), seq, at(secs), event)
}

pub fn activated(
    seq: u64,
    secs: i64,
    member: PrincipalId,
    granted: &[Role],
    ends: i64,
) -> MembershipRecord {
    record(
        seq,
        secs,
        MembershipEvent::Activated {
            member,
            invitation: None,
            roles: roles(granted),
            independent_approval_required: false,
            cool_off_ends_at: at(ends),
        },
    )
}

/// `member`'s membership of the hosted workspace, its `effective` roles read at `now`.
pub fn membership(
    member: PrincipalId,
    state: MembershipState,
    effective: &[Role],
    now: i64,
) -> Membership {
    let effective = effective.iter().map(|role| (*role, at(now))).collect();
    Membership::new(Seal::grant(), member, scope(), state, effective)
}

pub fn query(lookup: &ControlLookup, member: PrincipalId) -> Result<Vec<Membership>, LookupFailed> {
    lookup.memberships(&MembershipQuery {
        member: Some(member),
        scope: scope(),
    })
}
