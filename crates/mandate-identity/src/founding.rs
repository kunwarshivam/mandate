//! The founding grant (identity spec §3.2, §4.3, ID-13; journal spec §9.12; DEC-991): the one
//! `MemberActivated` with reason `founding` that makes a new workspace's owner its member, which
//! the system issues once, before any other membership exists.
//!
//! [`found_workspace`] decides inside each attempt of the one membership writer, from the head and
//! records that attempt read and appends at, so two bootstraps racing on one stream never both
//! grant: the second's append finds the head moved, re-reads, and finds the first's grant.

use mandate_time::UtcNanos;

use crate::writer::{
    ControlEntry, ControlStream, ControlView, Halt, Step, WriteError, write_decided,
};
use crate::{MembershipEvent, MembershipRecord, PrincipalId, Role};

/// The owner bundle (identity spec §4.3): workspace admin, operator, and approver, and no other
/// role. The founding grant never names more.
pub const OWNER_BUNDLE: [Role; 3] = [Role::WorkspaceAdmin, Role::Operator, Role::Approver];

/// What [`found_workspace`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Founded {
    /// It committed the founding grant: the record, as committed.
    Granted(MembershipRecord),
    /// The stream's first membership record is already this member's founding grant, so it
    /// committed nothing: that record.
    AlreadyFounded(MembershipRecord),
}

/// Why the founding grant is refused. Each commits nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FoundingRefusal {
    /// The control stream holds no record: `mandate workspace open` has not run.
    #[error("the workspace's control stream is not open")]
    NotOpen,
    /// The control stream holds a record after its `StreamOpened` and no membership: the founding
    /// grant is the workspace's first record after its opening (DEC-991 item 3).
    #[error("the workspace's control stream holds records after its opening")]
    NotNew,
    /// A membership record exists that is not this member's founding grant.
    #[error("the workspace already has a membership")]
    MembersExist,
}

impl FoundingRefusal {
    /// Its stable code.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotOpen => "workspace_not_open",
            Self::NotNew => "workspace_not_new",
            Self::MembersExist => "members_exist",
        }
    }
}

/// Why [`found_workspace`] committed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundingError<E> {
    /// The stream's state refuses a founding grant.
    Refused(FoundingRefusal),
    /// The writer committed nothing, as [`WriteError`] says.
    Write(WriteError<E>),
}

/// The founding grant of `member` at `at`: an activation with no invitation, the owner bundle, no
/// independence required, and no cool-off (journal spec §9.12 rules 99, 100, and 103).
pub fn founding_grant(member: PrincipalId, at: UtcNanos) -> MembershipEvent {
    MembershipEvent::Activated {
        member,
        invitation: None,
        roles: OWNER_BUNDLE.into_iter().collect(),
        independent_approval_required: false,
        cool_off_ends_at: at,
    }
}

/// What `view` says of founding the workspace for `member`: `Ok(None)` when the grant may be
/// committed, `Ok(Some(record))` when the stream's first membership record is already `member`'s
/// founding grant. The view must hold every record from the stream's first, as the founding
/// bootstrap's adapter reads it; a view that starts later can only refuse, never grant twice.
///
/// # Errors
/// [`FoundingRefusal`] as its variants say.
pub fn founding_decision(
    view: &ControlView,
    member: PrincipalId,
) -> Result<Option<MembershipRecord>, FoundingRefusal> {
    let first = view.tail.iter().find_map(|entry| match entry {
        ControlEntry::Membership(record) => Some(record),
        ControlEntry::Other { .. } => None,
    });
    match first {
        Some(record) if is_founding_of(record, member) => Ok(Some(record.clone())),
        Some(_) => Err(FoundingRefusal::MembersExist),
        None if view.head == 0 => Err(FoundingRefusal::NotOpen),
        None if view.head > 1 => Err(FoundingRefusal::NotNew),
        None => Ok(None),
    }
}

/// Whether `record` is `member`'s founding grant as [`founding_grant`] writes it.
fn is_founding_of(record: &MembershipRecord, member: PrincipalId) -> bool {
    *record.event() == founding_grant(member, record.event_time())
}

/// Commits `member`'s founding grant through the one membership writer, deciding inside each
/// attempt from the view it appends at, or finds it committed.
///
/// # Errors
/// [`FoundingError`] as its variants say.
pub fn found_workspace<S: ControlStream>(
    stream: &mut S,
    now: impl FnMut() -> UtcNanos,
    member: PrincipalId,
) -> Result<Founded, FoundingError<S::Error>> {
    let decide = |view: &ControlView, at: UtcNanos| {
        Ok(match founding_decision(view, member)? {
            None => Step::Commit(founding_grant(member, at)),
            Some(granted) => Step::Stop(granted),
        })
    };
    match write_decided(stream, now, decide) {
        Ok(Ok(granted)) => Ok(Founded::Granted(granted)),
        Ok(Err(found)) => Ok(Founded::AlreadyFounded(found)),
        Err(Halt::Declined(refusal)) => Err(FoundingError::Refused(refusal)),
        Err(Halt::Write(error)) => Err(FoundingError::Write(error)),
    }
}
