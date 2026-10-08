//! The dispatcher's pure core (notifications spec §3.3, §3.4, §5.1, §5.3, §5.6, §5.8; backlog
//! E8-10). It turns committed causes into `NoticeIssued` and `NoticeAttempted` drafts and send
//! requests, and never reads a clock, a journal, an address, or the operating system's randomness:
//! every input carries its own time, and the [`Context`] supplies the rest. The process around it
//! (E8-10's `mandate-dispatcher`) appends the drafts to the notice stream before it performs the
//! sends of the same [`Step`] (NT-8).

use mandate_time::UtcNanos;

use crate::{Class, NoticeId, NoticeKind, Notification, NotifyError, SecureRandom};

/// A committed journal event's id (a ULID). It names a cause inside the workspace and is never
/// sent: no [`NoticeId`] is made from one (NT-4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub String);

/// An opaque user id, the only way a recipient is journaled (NT-2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UserId(pub String);

/// A push channel, as the mandate's `notifications.channels` names them (spec §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    Email,
    Phone,
    Slack,
    Sms,
    Telegram,
    WebPush,
}

/// The identity spec's roles, as the receive column names them (spec §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Operator,
    Approver,
    WorkspaceAdmin,
    OrgOwner,
    Viewer,
    Auditor,
    BillingAdmin,
}

/// A candidate recipient the cause's row names, with the roles they hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user: UserId,
    pub roles: Vec<Role>,
}

/// The receive column of spec §3.3, read as data: which roles may receive each class. A §3.2 row
/// narrows it and never widens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiveTable {
    pub rows: Vec<(Class, Vec<Role>)>,
}

/// What a cause is (spec §3.4): the approval request, a stream owner's `OwnerAlertSent`, or the
/// dispatcher's own committed `NoticeAttempted` that lost an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    ApprovalRequested {
        approvers: Vec<Member>,
        deadline: UtcNanos,
    },
    OwnerAlertSent {
        kind: NoticeKind,
        subject: EventId,
        /// The owner command a kill switch carries out, which is the one cause however many
        /// streams journal it (spec §3.4).
        owner_command: Option<EventId>,
        audience: Vec<Member>,
    },
    AddressLost {
        member: Member,
        channel: Channel,
    },
}

/// Why an attempt failed or was abandoned (spec §5.2, §5.3, §5.5): a closed set, never provider
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    Timeout,
    RateLimited,
    ProviderError,
    AddressRejected,
    AuthFailed,
    TooLarge,
    RecipientNotPermitted,
    Bounced,
    Complained,
    Unsubscribed,
    NotPending,
    RetryWindowEnded,
}

/// An adapter's answer to one send (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    Accepted { provider_message_id: String },
    Retryable(Reason),
    Permanent(Reason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Delivered,
    Failed,
    SuppressedQuietHours,
    DeferredQuietHours,
    Abandoned,
}

/// What the core reads, each with its own time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Committed {
        event: EventId,
        stream: String,
        at: UtcNanos,
        cause: Cause,
    },
    ApprovalResponded {
        approval: EventId,
        approver: UserId,
        at: UtcNanos,
    },
    /// The approval stopped being pending: timed out, canceled, or a terminal re-validation.
    ApprovalClosed {
        approval: EventId,
        at: UtcNanos,
    },
    Outcome {
        notice: NoticeId,
        recipient: UserId,
        channel: Channel,
        attempt: u32,
        result: SendResult,
        at: UtcNanos,
    },
    Tick {
        now: UtcNanos,
    },
}

/// A record for the notice stream, in the journal spec's shapes (DEC-720).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Draft {
    NoticeIssued {
        notice: NoticeId,
        kind: NoticeKind,
        class: Class,
        cause: EventId,
        cause_stream: String,
        recipients: Vec<UserId>,
    },
    NoticeAttempted {
        notice: NoticeId,
        recipient: UserId,
        channel: Channel,
        attempt: u32,
        status: Status,
        reason: Option<Reason>,
        provider_message_id: Option<String>,
        coalesced_into: Option<NoticeId>,
    },
}

/// One message for the process to hand to a channel's adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Send {
    pub recipient: UserId,
    pub channel: Channel,
    pub attempt: u32,
    pub notification: Notification,
}

/// Everything one input produced. `next_wake` is the earliest time the core has work due, at which
/// the process sends a [`Input::Tick`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Step {
    pub drafts: Vec<Draft>,
    pub sends: Vec<Send>,
    pub next_wake: Option<UtcNanos>,
}

/// The quiet-hours verdict for a recipient at an instant (spec §5.8). The process computes it with
/// `mandate_approval::deliver_now`; this crate only applies the class rule to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quiet {
    Open,
    Quiet { until: UtcNanos },
}

/// What the process supplies. No method has a default, so no caller can skip the quiet-hours
/// verdict for an `action` or `info` send.
pub trait Context {
    fn random(&mut self) -> &mut dyn SecureRandom;
    /// The push channels configured for `user` that still have a reachable address.
    fn channels(&self, user: &UserId) -> Vec<Channel>;
    fn quiet(&self, user: &UserId, at: UtcNanos) -> Quiet;
}

/// The dispatcher's state for one workspace.
#[derive(Debug, Clone)]
pub struct Dispatcher {
    receive: ReceiveTable,
}

impl Dispatcher {
    pub fn new(receive: ReceiveTable) -> Self {
        Self { receive }
    }

    /// Folds one input.
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when no notice id can be minted; nothing is issued then.
    pub fn step(&mut self, input: Input, ctx: &mut dyn Context) -> Result<Step, NotifyError> {
        let _ = (&self.receive, input, ctx);
        Err(NotifyError::Unimplemented { story: "E8-10" })
    }
}
