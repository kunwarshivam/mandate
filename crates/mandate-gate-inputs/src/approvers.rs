//! The mandate's approver references resolved to the user ids admission compares (mandate spec
//! §6.4 check 3, V-024; [DEC-953](../../../docs/project/decisions/DEC-953.md),
//! [DEC-2384](../../../docs/project/decisions/DEC-2384.md)).
//!
//! A mandate names its approvers as `role:approver`, `role:owner`, `role:workspace_admin` or
//! `user:<id>`, while an answer names its responder by opaque user id, so the runtime can admit
//! only a set of user ids. A stream with no §9.12 membership record is DEC-953's one-person
//! workspace: its member is the `user` who recorded the mandate connection's latest
//! `ConnectionEstablished`, the record `mandate approvals approve` reads for DEC-923's check, who
//! holds every workspace role (identity spec §3.2, §4.3). A stream with one, as every workspace
//! `mandate workspace found` founds, is folded by `mandate_identity::Roster` at its latest
//! membership record's instant, and each reference resolves only among the members active then
//! who hold the approver role in effect. A stream the run cannot read either way resolves no one,
//! so every grant is refused `not_an_approver` and nothing is sent (`AGENTS.md` rule 3).
//!
//! The resolution itself is [`mandate_identity::approvers`], over the records' sequence, type,
//! event time and payload, so the runtime daemon resolves the same way; this module reads the
//! control stream's records into it and adds DEC-923's live `cli_confirm` scope.

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_domain::Environment;
pub use mandate_identity::approvers::Members;
use mandate_identity::approvers::{self as resolution, RecordView, latest_connection_record};
use mandate_num::Usd;
use mandate_runtime::{LiveBroker, LiveScope, OpaqueUser};
use mandate_spec::Mandate;
use mandate_spec::document::ApproverRef;

use crate::control::ControlRecord;

/// The members `records` show for the workspace whose mandate names `connection`, as
/// [`mandate_identity::approvers::members`] reads them.
pub fn members(records: &[ControlRecord], connection: &str) -> Members {
    resolution::members(&views(records), connection)
}

/// What the run resolves its approvers from again at each read of the control stream (DEC-2384
/// item 6): the mandate's `autonomy.approval.approvers` and its connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproverSource {
    pub references: Vec<ApproverRef>,
    pub connection: String,
}

impl ApproverSource {
    /// `admitted`, the approvers the run was admitted with, narrowed to those `records`, a fresh
    /// read of the control stream, still resolve to: a member deactivated, removed, or stripped of
    /// a role since never approves, and no one is added (DEC-2384 item 6, `AGENTS.md` rule 3).
    pub fn current(
        &self,
        admitted: &BTreeSet<String>,
        records: &[ControlRecord],
    ) -> BTreeSet<String> {
        let fresh = resolve(&self.references, &members(records, &self.connection));
        admitted.intersection(&fresh).cloned().collect()
    }
}

/// The user ids `references` resolve to among `members`, as
/// [`mandate_identity::approvers::resolve`] reads them: the set admission check 3 compares a
/// responder against. Empty when nothing resolves, so every grant is refused.
pub fn resolve(references: &[ApproverRef], members: &Members) -> BTreeSet<String> {
    let texts: Vec<&str> = references.iter().map(ApproverRef::as_str).collect();
    resolution::resolve(&texts, members)
}

/// The resolution's view of each of `records`.
fn views(records: &[ControlRecord]) -> Vec<RecordView<'_>> {
    records
        .iter()
        .map(|record| RecordView {
            seq: record.seq,
            event_type: &record.event_type,
            event_time: record.event_time,
            payload: &record.payload,
        })
        .collect()
}

/// DEC-923's scope for a live `cli_confirm` grant, read as `mandate approvals approve` reads it
/// (DEC-970 item 1): on a `live` run, the mandate connection's latest record is a `live`
/// `ConnectionEstablished`, its broker names the scope's broker, the confirmed mandate gives the
/// per-order cap and the pinned instruments, and the owner is [`members`]' owner. `None` on a paper
/// run, and whenever any part cannot be read, so the runtime refuses every live `cli_confirm`
/// grant `step_up_method` (`AGENTS.md` rule 3).
pub fn live_cli_confirm(
    records: &[ControlRecord],
    mandate: &Mandate,
    environment: Environment,
) -> Option<LiveScope> {
    if environment != Environment::Live {
        return None;
    }
    let connection = mandate.connection_id.as_str();
    let views = views(records);
    let Members::Owner(owner) = resolution::members(&views, connection) else {
        return None;
    };
    let latest = latest_connection_record(&views, connection)?;
    let text = |name: &str| latest.payload.get(name).and_then(Value::as_str);
    if text("environment") != Some("live") {
        return None;
    }
    let broker = match text("broker") {
        Some("robinhood") => LiveBroker::Robinhood,
        Some(_) | None => LiveBroker::Other,
    };
    let Ok(max_order_usd) = Usd::parse(mandate.risk.max_order_usd.as_str()) else {
        return None;
    };
    Some(LiveScope {
        broker,
        max_order_usd,
        pinned_instruments: mandate
            .universe
            .pinned_instruments
            .iter()
            .map(|instrument| instrument.asset_id.as_str().to_owned())
            .collect(),
        workspace_owner: OpaqueUser(owner),
    })
}
