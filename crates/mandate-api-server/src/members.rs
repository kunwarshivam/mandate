//! Memberships from the workspace's control stream (§3.11 item 8, DEC-978 item 8): the one
//! [`MembershipLookup`] `authorize` reads, built per request from journal spec §9.12's records,
//! uncached (identity spec ID-3). A record that cannot be read, or a fold that cannot answer, is a
//! failed lookup, never an empty membership.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, encode_ulid};
use mandate_identity::{
    ControlEntry, LookupFailed, Membership, MembershipFold, MembershipLookup, MembershipQuery,
    MembershipRecord, MembershipState, OrgId, PrincipalId, Role, Scope, WorkspaceId, control_entry,
};
use mandate_identity_seal::{LookupSeal, Seal};
use mandate_journal::StoredEvent;
use mandate_passkey::credentials::{CredentialRecord, credential_record};
use mandate_time::UtcNanos;

/// Turns the control stream's rows into its membership records, refusing a `Member*` record
/// that does not map (`mandate_identity::control_entry`); [`control_parser`] in the deployment.
pub type ParseMembers =
    dyn Fn(&[StoredEvent]) -> Result<Vec<MembershipRecord>, LookupFailed> + Send + Sync;

/// The record that opens every stream at `seq` 1 and names its environment (journal spec §2).
const STREAM_OPENED: &str = "StreamOpened";

/// The one schema version of journal spec §9.12's membership records.
const MEMBERSHIP_VERSION: u64 = 1;

/// Why the hosted control stream's rows give no membership records (DEC-2110). Each is a failed
/// lookup, never an empty membership (`AGENTS.md` rule 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowsRefused {
    /// The hosted stream's first row is not its `StreamOpened` at `seq` 1, so the stream names no
    /// environment.
    NotOpened,
    /// The row at `seq` is not a canonical envelope, or is a `Member*` record that does not map
    /// (`mandate_identity::control_entry`).
    Unreadable { seq: u64 },
    /// The `Member*` record at `seq` is at a schema version other than 1 (journal spec §9.12).
    UnknownVersion { seq: u64 },
    /// The `Member*` record at `seq` is written in an environment other than the stream's.
    OtherEnvironment { seq: u64 },
}

/// The membership records of `workspace`'s control stream among `rows`, in stream order: each
/// row's envelope is read, a row of another stream is not the workspace's and is passed over, and
/// every `Member*` record of the workspace's stream maps through `control_entry` (DEC-991 item 4,
/// DEC-2110).
///
/// # Errors
/// [`RowsRefused`] for a stream not opened, an unreadable row, or a `Member*` record of an unknown
/// schema version or another environment.
pub fn membership_records(
    workspace: WorkspaceId,
    rows: &[StoredEvent],
) -> Result<Vec<MembershipRecord>, RowsRefused> {
    let mut records = Vec::new();
    for row in hosted_rows(workspace, rows)? {
        let unreadable = RowsRefused::Unreadable { seq: row.seq };
        let entry = control_entry(row.seq, &row.envelope).map_err(|_| unreadable)?;
        let ControlEntry::Membership(record) = entry else {
            continue;
        };
        row.same_environment()?;
        if row.envelope.get("schema_version").and_then(Value::as_int) != Some(MEMBERSHIP_VERSION) {
            return Err(RowsRefused::UnknownVersion { seq: row.seq });
        }
        records.push(record);
    }
    Ok(records)
}

/// One row of the hosted control stream: its `seq`, its envelope, and the environment it and the
/// stream's `StreamOpened` were written in.
struct HostedRow {
    seq: u64,
    envelope: Value,
    written: String,
    opened: String,
}

impl HostedRow {
    /// Refuses a record written in another environment than the stream's.
    fn same_environment(&self) -> Result<(), RowsRefused> {
        if self.written != self.opened {
            return Err(RowsRefused::OtherEnvironment { seq: self.seq });
        }
        Ok(())
    }
}

/// The rows of `workspace`'s control stream among `rows`, in stream order, each envelope read: a
/// row of another stream is passed over, and the stream must open with its `StreamOpened` at
/// `seq` 1, whose environment every later row is compared with.
fn hosted_rows(
    workspace: WorkspaceId,
    rows: &[StoredEvent],
) -> Result<Vec<HostedRow>, RowsRefused> {
    let stream = format!("ctl:{}", encode_ulid(workspace.0));
    let mut environment: Option<String> = None;
    let mut hosted = Vec::new();
    for row in rows {
        let unreadable = RowsRefused::Unreadable { seq: row.seq };
        let envelope = mandate_canon::parse(&row.body).map_err(|_| unreadable.clone())?;
        let text = |key| envelope.get(key).and_then(Value::as_str);
        if text("stream_id") != Some(stream.as_str()) {
            continue;
        }
        control_entry(row.seq, &envelope).map_err(|_| unreadable.clone())?;
        let written = text("environment").ok_or(unreadable)?.to_owned();
        let opened = match &environment {
            Some(opened) => opened.clone(),
            None if row.seq == 1 && text("event_type") == Some(STREAM_OPENED) => {
                environment.insert(written.clone()).clone()
            }
            None => return Err(RowsRefused::NotOpened),
        };
        hosted.push(HostedRow {
            seq: row.seq,
            envelope,
            written,
            opened,
        });
    }
    Ok(hosted)
}

/// The passkey records of `workspace`'s control stream among `rows` (journal spec §9.31, read by
/// `mandate_passkey::credentials::credential_record`), in stream order, for route 2's
/// `CredentialFold::held` (§4.10 route 2 item 2). Rows are read as [`membership_records`] reads
/// them: a row of another stream is passed over, the stream must open with its `StreamOpened`, and
/// a credential record in another environment than the stream's is refused.
///
/// # Errors
/// [`RowsRefused`] for a stream not opened, an unreadable row, a credential record that does not
/// map (at any schema version but 1 too), or one written in another environment.
pub fn credential_records(
    workspace: WorkspaceId,
    rows: &[StoredEvent],
) -> Result<Vec<CredentialRecord>, RowsRefused> {
    let mut records = Vec::new();
    for row in hosted_rows(workspace, rows)? {
        let record = credential_record(row.seq, &row.envelope)
            .map_err(|_| RowsRefused::Unreadable { seq: row.seq })?;
        let Some(record) = record else {
            continue;
        };
        row.same_environment()?;
        records.push(record);
    }
    Ok(records)
}

/// The parser of the hosted `workspace`'s control stream: [`membership_records`], any refusal a
/// failed lookup (DEC-2110 item 7).
pub fn control_parser(
    workspace: WorkspaceId,
) -> impl Fn(&[StoredEvent]) -> Result<Vec<MembershipRecord>, LookupFailed> + Send + Sync {
    move |rows| membership_records(workspace, rows).map_err(|_| LookupFailed)
}

/// The one workspace this deployment hosts, and its organization (§3.11 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hosted {
    pub org: OrgId,
    pub workspace: WorkspaceId,
    /// Whether the deployment's configuration marks it one of the team's internal paper
    /// workspaces, checked against its control stream at start (DEC-1143).
    pub internal_paper: bool,
}

/// The hosted workspace's memberships at one instant.
pub struct ControlLookup {
    hosted: Hosted,
    now: UtcNanos,
    records: Result<Vec<MembershipRecord>, LookupFailed>,
}

impl ControlLookup {
    /// The lookup over `records`, read from the hosted workspace's control stream, at `now`.
    pub fn new(
        hosted: Hosted,
        records: Result<Vec<MembershipRecord>, LookupFailed>,
        now: UtcNanos,
    ) -> Self {
        Self {
            hosted,
            now,
            records,
        }
    }

    /// `member`'s memberships, for the session's roles snapshot; none when the lookup fails.
    pub fn snapshot(&self, member: PrincipalId) -> Vec<Membership> {
        self.memberships(&MembershipQuery {
            member: Some(member),
            scope: self.scope(),
        })
        .unwrap_or_default()
    }

    /// `member`'s own row of the membership index (identity spec §4.5, DEC-2113 item 4): the roles
    /// in effect at the lookup's instant when its membership of the hosted workspace is `active`,
    /// and `None` in any other state or none.
    ///
    /// # Errors
    /// [`LookupFailed`] when the control stream's records cannot be read or folded.
    pub fn own_roles(&self, member: PrincipalId) -> Result<Option<BTreeSet<Role>>, LookupFailed> {
        let fold = self.fold()?;
        Ok(
            (fold.state(member, self.now) == Some(MembershipState::Active))
                .then(|| fold.effective_roles(member, self.now)),
        )
    }

    /// Whether `member`'s membership of the hosted workspace reaches it at the lookup's instant:
    /// `active` or `cooling_off` (identity spec §5.1), as route 2 requires (§4.10 route 2 item 2).
    ///
    /// # Errors
    /// [`LookupFailed`] when the control stream's records cannot be read or folded.
    pub fn reaches(&self, member: PrincipalId) -> Result<bool, LookupFailed> {
        let fold = self.fold()?;
        Ok(matches!(
            fold.state(member, self.now),
            Some(MembershipState::Active | MembershipState::CoolingOff)
        ))
    }

    /// The hosted workspace's scope.
    fn scope(&self) -> Scope {
        Scope::Workspace {
            org: self.hosted.org,
            workspace: self.hosted.workspace,
        }
    }

    /// The control stream's fold, readable at `now`, or a failed lookup.
    fn fold(&self) -> Result<MembershipFold, LookupFailed> {
        let records = self.records.clone()?;
        let fold = MembershipFold::new(Seal::grant(), records);
        if fold.unreadable(self.now) {
            return Err(LookupFailed);
        }
        Ok(fold)
    }
}

impl LookupSeal for ControlLookup {}

impl MembershipLookup for ControlLookup {
    /// One member's membership of the hosted workspace, as the fold reads it at the lookup's
    /// instant: its state and the roles effective then. The query for every member of a scope
    /// (the last-owner count of a role change) is not served here and fails.
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        let member = query.member.ok_or(LookupFailed)?;
        let fold = self.fold()?;
        if query.scope != self.scope() {
            return Ok(Vec::new());
        }
        let Some(state) = fold.state(member, self.now) else {
            return Ok(Vec::new());
        };
        let roles: BTreeMap<Role, UtcNanos> = fold
            .effective_roles(member, self.now)
            .into_iter()
            .map(|role| (role, self.now))
            .collect();
        Ok(vec![Membership::new(
            Seal::grant(),
            member,
            query.scope,
            state,
            roles,
        )])
    }

    /// The hosted workspace, for its own organization; the store's read is the control stream's.
    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        self.records.as_ref().map_err(|_| LookupFailed)?;
        Ok(if org == self.hosted.org {
            BTreeSet::from([self.hosted.workspace])
        } else {
            BTreeSet::new()
        })
    }

    /// The hosted workspace's organization, from the deployment's settings; any other workspace
    /// is not hosted here.
    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok((workspace == self.hosted.workspace).then_some(self.hosted.org))
    }
}
