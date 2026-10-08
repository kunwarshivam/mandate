//! The dispatcher's pure core (notifications spec §3.3, §3.4, §5.1, §5.3, §5.6, §5.8; backlog
//! E8-10). It turns committed causes into `NoticeIssued` and `NoticeAttempted` drafts and send
//! requests, and never reads a clock, a journal, an address, or the operating system's randomness:
//! every input carries its own time, and the [`Context`] supplies the rest. The process around it
//! (E8-10's `mandate-dispatcher`) appends the drafts to the notice stream before it performs the
//! sends of the same [`Step`] (NT-8).

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;

use crate::{Class, NoticeId, NoticeKind, Notification, NotifyError, SecureRandom, TextKey};

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

/// One recipient's channel for one notice.
#[derive(Debug, Clone)]
struct Delivery {
    attempt: u32,
    due: Option<UtcNanos>,
    done: bool,
}

/// What the core keeps about an issued notice.
#[derive(Debug, Clone)]
struct Issued {
    class: Class,
    text: TextKey,
    start: UtcNanos,
    deadline: Option<UtcNanos>,
}

type DeliveryKey = (NoticeId, UserId, Channel);

/// The gaps of spec §5.3 after a failed attempt, in seconds: 15 s, 60 s, 5 min, then 15 min.
const RETRY_GAPS_S: [i64; 3] = [15, 60, 300];
const LATER_GAP_S: i64 = 900;
const SAFETY_WINDOW_S: i64 = 86_400;
const INFO_WINDOW_S: i64 = 21_600;

fn later(at: UtcNanos, secs: i64) -> Result<UtcNanos, NotifyError> {
    at.secs()
        .checked_add(secs)
        .and_then(|s| UtcNanos::from_parts(s, at.nanos()).ok())
        .ok_or(NotifyError::Unrepresentable { what: "retry time" })
}

fn retry_gap(failed_attempt: u32) -> i64 {
    usize::try_from(failed_attempt)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| RETRY_GAPS_S.get(i))
        .copied()
        .unwrap_or(LATER_GAP_S)
}

/// The dispatcher's state for one workspace.
#[derive(Debug, Clone)]
pub struct Dispatcher {
    receive: ReceiveTable,
    causes: BTreeSet<EventId>,
    commands: BTreeSet<EventId>,
    notices: BTreeMap<NoticeId, Issued>,
    approvals: BTreeMap<EventId, NoticeId>,
    deliveries: BTreeMap<DeliveryKey, Delivery>,
}

impl Dispatcher {
    pub fn new(receive: ReceiveTable) -> Self {
        Self {
            receive,
            causes: BTreeSet::new(),
            commands: BTreeSet::new(),
            notices: BTreeMap::new(),
            approvals: BTreeMap::new(),
            deliveries: BTreeMap::new(),
        }
    }

    /// Folds one input.
    ///
    /// # Errors
    /// [`NotifyError::EntropyUnavailable`] when no notice id can be minted; nothing is issued then.
    pub fn step(&mut self, input: Input, ctx: &mut dyn Context) -> Result<Step, NotifyError> {
        let mut step = Step::default();
        match input {
            Input::Committed {
                event,
                stream,
                at,
                cause,
            } => self.issue(event, stream, at, cause, ctx, &mut step)?,
            Input::ApprovalResponded { .. } => {}
            Input::ApprovalClosed { approval, .. } => self.close(&approval, &mut step),
            Input::Outcome {
                notice,
                recipient,
                channel,
                attempt,
                result,
                at,
            } => self.outcome((notice, recipient, channel), attempt, result, at, &mut step)?,
            Input::Tick { now } => self.tick(now, ctx, &mut step),
        }
        step.next_wake = self
            .deliveries
            .values()
            .filter(|d| !d.done)
            .filter_map(|d| d.due)
            .min();
        Ok(step)
    }

    fn may_receive(&self, class: Class, member: &Member) -> bool {
        self.receive
            .rows
            .iter()
            .any(|(c, roles)| *c == class && member.roles.iter().any(|r| roles.contains(r)))
    }

    fn issue(
        &mut self,
        event: EventId,
        stream: String,
        at: UtcNanos,
        cause: Cause,
        ctx: &mut dyn Context,
        step: &mut Step,
    ) -> Result<(), NotifyError> {
        if self.causes.contains(&event) {
            return Ok(());
        }
        let (kind, audience, deadline, lost) = match cause {
            Cause::ApprovalRequested {
                approvers,
                deadline,
            } => (
                NoticeKind::ApprovalRequested,
                approvers,
                Some(deadline),
                None,
            ),
            Cause::OwnerAlertSent {
                kind,
                owner_command,
                audience,
                ..
            } => {
                if let Some(command) = owner_command
                    && !self.commands.insert(command)
                {
                    return Ok(());
                }
                (kind, audience, None, None)
            }
            Cause::AddressLost { member, channel } => {
                (NoticeKind::ChannelLost, vec![member], None, Some(channel))
            }
        };
        let class = kind.class()?;
        let text = kind.text_key()?;
        let mut recipients: Vec<UserId> = audience
            .iter()
            .filter(|m| self.may_receive(class, m))
            .map(|m| m.user.clone())
            .collect();
        recipients.sort();
        recipients.dedup();
        let notice = NoticeId::mint(ctx.random())?;
        self.causes.insert(event.clone());
        if kind == NoticeKind::ApprovalRequested {
            self.approvals.insert(event.clone(), notice);
        }
        step.drafts.push(Draft::NoticeIssued {
            notice,
            kind,
            class,
            cause: event,
            cause_stream: stream,
            recipients: recipients.clone(),
        });
        let Some(text) = text else {
            return Ok(());
        };
        self.notices.insert(
            notice,
            Issued {
                class,
                text,
                start: at,
                deadline,
            },
        );
        for recipient in recipients {
            let mut channels = ctx.channels(&recipient);
            channels.sort();
            channels.dedup();
            channels.retain(|c| Some(*c) != lost);
            for channel in channels {
                let key = (notice, recipient.clone(), channel);
                let delivery = Delivery {
                    attempt: 0,
                    due: Some(at),
                    done: false,
                };
                self.deliveries.insert(key.clone(), delivery);
                self.attempt(key, at, ctx, step);
            }
        }
        Ok(())
    }

    /// Sends the next attempt of a due delivery, or records why it does not go now.
    fn attempt(&mut self, key: DeliveryKey, now: UtcNanos, ctx: &dyn Context, step: &mut Step) {
        let Some(issued) = self.notices.get(&key.0).cloned() else {
            return;
        };
        let Some(delivery) = self.deliveries.get_mut(&key) else {
            return;
        };
        let attempt = delivery.attempt.saturating_add(1);
        if issued.deadline.is_some_and(|deadline| now >= deadline) {
            delivery.done = true;
            step.drafts
                .push(abandoned(&key, delivery.attempt.max(1), Reason::NotPending));
            return;
        }
        let quiet = match issued.class {
            Class::Safety => Quiet::Open,
            Class::Action | Class::Info => ctx.quiet(&key.1, now),
        };
        match (issued.class, quiet) {
            (Class::Action, Quiet::Quiet { .. }) => {
                delivery.attempt = attempt;
                delivery.done = true;
                step.drafts
                    .push(recorded(&key, attempt, Status::SuppressedQuietHours, None));
            }
            (_, Quiet::Quiet { until }) => {
                delivery.attempt = attempt;
                delivery.due = Some(until);
                step.drafts
                    .push(recorded(&key, attempt, Status::DeferredQuietHours, None));
            }
            (_, Quiet::Open) => {
                delivery.attempt = attempt;
                delivery.due = None;
                step.sends.push(Send {
                    recipient: key.1.clone(),
                    channel: key.2,
                    attempt,
                    notification: Notification {
                        notice: key.0,
                        text: issued.text,
                    },
                });
            }
        }
    }

    fn outcome(
        &mut self,
        key: DeliveryKey,
        attempt: u32,
        result: SendResult,
        at: UtcNanos,
        step: &mut Step,
    ) -> Result<(), NotifyError> {
        let Some(issued) = self.notices.get(&key.0).cloned() else {
            return Ok(());
        };
        let Some(delivery) = self.deliveries.get_mut(&key) else {
            return Ok(());
        };
        if delivery.done || delivery.attempt != attempt {
            return Ok(());
        }
        match result {
            SendResult::Accepted {
                provider_message_id,
            } => {
                delivery.done = true;
                step.drafts.push(Draft::NoticeAttempted {
                    notice: key.0,
                    recipient: key.1,
                    channel: key.2,
                    attempt,
                    status: Status::Delivered,
                    reason: None,
                    provider_message_id: Some(provider_message_id),
                    coalesced_into: None,
                });
            }
            SendResult::Permanent(reason) => {
                delivery.done = true;
                step.drafts
                    .push(recorded(&key, attempt, Status::Failed, Some(reason)));
            }
            SendResult::Retryable(reason) => {
                step.drafts
                    .push(recorded(&key, attempt, Status::Failed, Some(reason)));
                let due = later(at, retry_gap(attempt))?;
                let stop = match (issued.class, issued.deadline) {
                    (Class::Action, Some(deadline)) => {
                        (due >= deadline).then_some(Reason::NotPending)
                    }
                    (Class::Action, None) => None,
                    (Class::Safety, _) => (due > later(issued.start, SAFETY_WINDOW_S)?)
                        .then_some(Reason::RetryWindowEnded),
                    (Class::Info, _) => (due > later(issued.start, INFO_WINDOW_S)?)
                        .then_some(Reason::RetryWindowEnded),
                };
                match stop {
                    Some(reason) => {
                        delivery.done = true;
                        step.drafts.push(abandoned(&key, attempt, reason));
                    }
                    None => delivery.due = Some(due),
                }
            }
        }
        Ok(())
    }

    fn tick(&mut self, now: UtcNanos, ctx: &dyn Context, step: &mut Step) {
        let due: Vec<DeliveryKey> = self
            .deliveries
            .iter()
            .filter(|(_, d)| !d.done && d.due.is_some_and(|t| t <= now))
            .map(|(k, _)| k.clone())
            .collect();
        for key in due {
            self.attempt(key, now, ctx, step);
        }
    }

    fn close(&mut self, approval: &EventId, step: &mut Step) {
        let Some(notice) = self.approvals.get(approval).copied() else {
            return;
        };
        for (key, delivery) in self
            .deliveries
            .range_mut((notice, UserId(String::new()), Channel::Email)..)
        {
            if key.0 != notice {
                break;
            }
            if !delivery.done {
                delivery.done = true;
                step.drafts
                    .push(abandoned(key, delivery.attempt.max(1), Reason::NotPending));
            }
        }
    }
}

fn recorded(key: &DeliveryKey, attempt: u32, status: Status, reason: Option<Reason>) -> Draft {
    Draft::NoticeAttempted {
        notice: key.0,
        recipient: key.1.clone(),
        channel: key.2,
        attempt,
        status,
        reason,
        provider_message_id: None,
        coalesced_into: None,
    }
}

fn abandoned(key: &DeliveryKey, attempt: u32, reason: Reason) -> Draft {
    recorded(key, attempt, Status::Abandoned, Some(reason))
}
