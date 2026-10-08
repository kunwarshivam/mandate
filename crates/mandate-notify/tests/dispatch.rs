//! The dispatcher's core (notifications spec §3.3, §3.4, §5.1, §5.3, §5.6, §5.8; backlog E8-10):
//! one notice per cause, recipients from the receive column, quiet hours by class, and the retry
//! schedule. Every expectation is written here from the spec, not read from the crate.

mod common;

use std::cell::Cell;

use common::{Recording, answer};
use mandate_notify::{
    Cause, Channel, Class, Context, Dispatcher, Draft, EventId, Input, Member, NoticeId,
    NoticeKind, Quiet, Reason, ReceiveTable, Role, SecureRandom, SendResult, Status, Step, TextKey,
    UserId,
};
use mandate_time::UtcNanos;

const T0: i64 = 1_790_000_000;

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(T0 + offset_s, 0).unwrap_or_else(|e| panic!("clock: {e:?}"))
}

fn user(name: &str) -> UserId {
    UserId(name.to_owned())
}

fn member(name: &str, roles: &[Role]) -> Member {
    Member {
        user: user(name),
        roles: roles.to_vec(),
    }
}

fn event(id: &str) -> EventId {
    EventId(id.to_owned())
}

/// Spec §3.3's interim receive column, typed from the table.
fn receive_table() -> ReceiveTable {
    use Role::{Approver, Operator, OrgOwner, Viewer, WorkspaceAdmin};
    ReceiveTable {
        rows: vec![
            (Class::Action, vec![Approver]),
            (
                Class::Safety,
                vec![Operator, Approver, WorkspaceAdmin, OrgOwner],
            ),
            (
                Class::Info,
                vec![Operator, Approver, WorkspaceAdmin, Viewer],
            ),
        ],
    }
}

/// A process stand-in: a recording random source, two channels for everyone, a fixed quiet-hours
/// verdict, and a count of how often the verdict was asked for.
struct Fixture {
    random: Recording,
    channels: Vec<Channel>,
    quiet: Quiet,
    quiet_asked: Cell<u32>,
}

impl Fixture {
    fn new(quiet: Quiet) -> Self {
        Self {
            random: Recording::seeded(7),
            channels: vec![Channel::Email, Channel::Slack],
            quiet,
            quiet_asked: Cell::new(0),
        }
    }
}

impl Context for Fixture {
    fn random(&mut self) -> &mut dyn SecureRandom {
        &mut self.random
    }
    fn channels(&self, _: &UserId) -> Vec<Channel> {
        self.channels.clone()
    }
    fn quiet(&self, _: &UserId, _: UtcNanos) -> Quiet {
        self.quiet_asked.set(self.quiet_asked.get() + 1);
        self.quiet
    }
}

fn step(d: &mut Dispatcher, input: Input, ctx: &mut Fixture) -> Step {
    answer("step", d.step(input, ctx))
}

fn alert(id: &str, kind: NoticeKind, audience: Vec<Member>, offset_s: i64) -> Input {
    Input::Committed {
        event: event(id),
        stream: "acct:w1:a1".to_owned(),
        at: at(offset_s),
        cause: Cause::OwnerAlertSent {
            kind,
            subject: event(&format!("{id}-subject")),
            owner_command: None,
            audience,
        },
    }
}

fn owner() -> Member {
    member("u-owner", &[Role::Operator])
}

fn issued(step: &Step) -> Vec<(NoticeId, NoticeKind, EventId, Vec<UserId>)> {
    step.drafts
        .iter()
        .filter_map(|d| match d {
            Draft::NoticeIssued {
                notice,
                kind,
                cause,
                recipients,
                ..
            } => Some((*notice, *kind, cause.clone(), recipients.clone())),
            Draft::NoticeAttempted { .. } => None,
        })
        .collect()
}

fn attempted(step: &Step) -> Vec<(UserId, Channel, u32, Status, Option<Reason>)> {
    step.drafts
        .iter()
        .filter_map(|d| match d {
            Draft::NoticeAttempted {
                recipient,
                channel,
                attempt,
                status,
                reason,
                ..
            } => Some((recipient.clone(), *channel, *attempt, *status, *reason)),
            Draft::NoticeIssued { .. } => None,
        })
        .collect()
}

#[test]
#[ignore = "pending E8-10"]
fn a_cause_is_issued_once_with_a_minted_id_before_any_send() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    let input = alert("E1", NoticeKind::RiskLimit, vec![owner()], 0);
    let first = step(&mut d, input.clone(), &mut ctx);
    let notices = issued(&first);
    assert_eq!(notices.len(), 1, "one NoticeIssued per cause");
    let (notice, kind, cause, recipients) = &notices[0];
    assert_eq!((*kind, cause), (NoticeKind::RiskLimit, &event("E1")));
    assert_eq!(recipients, &vec![user("u-owner")]);
    assert_eq!(
        ctx.random.issued.first(),
        Some(&notice_bytes(*notice)),
        "the id is the source's first draw, nothing taken from the cause"
    );
    assert!(
        matches!(first.drafts.first(), Some(Draft::NoticeIssued { .. })),
        "NoticeIssued is the step's first record (NT-8)"
    );
    let sent: Vec<_> = first
        .sends
        .iter()
        .map(|s| (s.channel, s.notification))
        .collect();
    let expected_notification = mandate_notify::Notification {
        notice: *notice,
        text: TextKey::AttentionNeeded,
    };
    assert_eq!(
        sent,
        vec![
            (Channel::Email, expected_notification),
            (Channel::Slack, expected_notification)
        ],
        "a safety notice fans out to every configured channel at once"
    );
    let again = step(&mut d, input, &mut ctx);
    assert!(
        issued(&again).is_empty() && again.sends.is_empty(),
        "a cause already issued is skipped"
    );
}

/// The id's bytes, recovered from the payload's 32 hex digits.
fn notice_bytes(notice: NoticeId) -> [u8; 16] {
    let value = answer(
        "payload",
        mandate_notify::payload(&mandate_notify::Notification {
            notice,
            text: TextKey::AttentionNeeded,
        }),
    );
    let Some(mandate_canon::Value::Str(hex)) = value.get("notice") else {
        panic!("the payload has no notice");
    };
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or_else(|e| panic!("hex: {e:?}"));
    }
    out
}

#[test]
#[ignore = "pending E8-10"]
fn a_users_kill_switch_is_one_notice_however_many_streams_record_it() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    let command = event("CMD1");
    let on = |id: &str, stream: &str| Input::Committed {
        event: event(id),
        stream: stream.to_owned(),
        at: at(0),
        cause: Cause::OwnerAlertSent {
            kind: NoticeKind::KillSwitch,
            subject: event(&format!("{id}-subject")),
            owner_command: Some(command.clone()),
            audience: vec![owner()],
        },
    };
    let mut notices = Vec::new();
    for (id, stream) in [
        ("K1", "ctl:w1"),
        ("K2", "agent:w1:g1"),
        ("K3", "acct:w1:a1"),
    ] {
        notices.extend(issued(&step(&mut d, on(id, stream), &mut ctx)));
    }
    assert_eq!(notices.len(), 1, "one notice for the command (spec §3.4)");
    assert_eq!(
        notices[0].2,
        event("K1"),
        "the first record of the command is the cause"
    );
}

#[test]
#[ignore = "pending E8-10"]
fn recipients_are_the_rows_audience_narrowed_by_the_receive_column() {
    let roles = [
        Role::Operator,
        Role::Approver,
        Role::WorkspaceAdmin,
        Role::OrgOwner,
        Role::Viewer,
        Role::Auditor,
        Role::BillingAdmin,
    ];
    let allowed = receive_table();
    for (kind, class) in [
        (NoticeKind::RiskLimit, Class::Safety),
        (NoticeKind::DailyBrief, Class::Info),
    ] {
        let mut d = Dispatcher::new(receive_table());
        let mut ctx = Fixture::new(Quiet::Open);
        let audience: Vec<Member> = roles
            .iter()
            .map(|r| member(&format!("u-{r:?}"), &[*r]))
            .collect();
        let got = issued(&step(&mut d, alert("E1", kind, audience, 0), &mut ctx));
        let mut expected: Vec<UserId> = roles
            .iter()
            .filter(|r| {
                allowed
                    .rows
                    .iter()
                    .any(|(c, rs)| *c == class && rs.contains(r))
            })
            .map(|r| user(&format!("u-{r:?}")))
            .collect();
        expected.sort();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].3, expected,
            "{kind:?}: the receive column, ascending"
        );
    }
}

#[test]
#[ignore = "pending E8-10"]
fn a_viewer_listed_as_an_approver_still_gets_no_action_notice() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    let input = Input::Committed {
        event: event("A1"),
        stream: "agent:w1:g1".to_owned(),
        at: at(0),
        cause: Cause::ApprovalRequested {
            approvers: vec![
                member("u-app", &[Role::Approver]),
                member("u-view", &[Role::Viewer]),
            ],
            deadline: at(900),
        },
    };
    let got = issued(&step(&mut d, input, &mut ctx));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].3, vec![user("u-app")]);
}

fn approval(deadline_s: i64) -> Input {
    Input::Committed {
        event: event("A1"),
        stream: "agent:w1:g1".to_owned(),
        at: at(0),
        cause: Cause::ApprovalRequested {
            approvers: vec![member("u-app", &[Role::Approver])],
            deadline: at(deadline_s),
        },
    }
}

#[test]
#[ignore = "pending E8-10"]
fn an_action_push_in_quiet_hours_is_suppressed_and_never_deferred() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Quiet { until: at(600) });
    let first = step(&mut d, approval(3_600), &mut ctx);
    assert!(first.sends.is_empty(), "no push inside the window");
    assert!(
        attempted(&first)
            .iter()
            .all(|a| a.3 == Status::SuppressedQuietHours && a.4.is_none()),
        "each channel is journaled suppressed_quiet_hours: {:?}",
        attempted(&first)
    );
    assert_eq!(attempted(&first).len(), 2);
    ctx.quiet = Quiet::Open;
    let later = step(&mut d, Input::Tick { now: at(601) }, &mut ctx);
    assert!(
        later.sends.is_empty(),
        "a suppressed action push is never sent later (spec §5.8)"
    );
}

#[test]
#[ignore = "pending E8-10"]
fn a_safety_notice_ignores_quiet_hours_without_asking_for_the_verdict() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Quiet { until: at(36_000) });
    let first = step(
        &mut d,
        alert("E1", NoticeKind::Reconciliation, vec![owner()], 0),
        &mut ctx,
    );
    assert_eq!(
        first.sends.len(),
        2,
        "sent on both channels inside the window (NT-7)"
    );
    assert_eq!(ctx.quiet_asked.get(), 0, "safety never reads the verdict");
}

#[test]
#[ignore = "pending E8-10"]
fn the_brief_in_quiet_hours_waits_for_the_windows_end() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Quiet { until: at(3_600) });
    let first = step(
        &mut d,
        alert("B1", NoticeKind::DailyBrief, vec![owner()], 0),
        &mut ctx,
    );
    assert!(first.sends.is_empty());
    assert!(
        attempted(&first)
            .iter()
            .all(|a| a.3 == Status::DeferredQuietHours),
        "journaled deferred_quiet_hours"
    );
    assert_eq!(
        first.next_wake,
        Some(at(3_600)),
        "woken at the window's end"
    );
    ctx.quiet = Quiet::Open;
    let early = step(&mut d, Input::Tick { now: at(3_599) }, &mut ctx);
    assert!(early.sends.is_empty(), "not before the window ends");
    let due = step(&mut d, Input::Tick { now: at(3_600) }, &mut ctx);
    assert_eq!(due.sends.len(), 2, "sent once the window ends");
    assert!(
        due.sends
            .iter()
            .all(|s| s.notification.text == TextKey::BriefReady)
    );
}

#[test]
#[ignore = "pending E8-10"]
fn an_info_kind_without_a_text_key_is_issued_and_never_pushed() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    let first = step(
        &mut d,
        alert("D1", NoticeKind::DelegationEnded, vec![owner()], 0),
        &mut ctx,
    );
    assert_eq!(issued(&first).len(), 1, "listed in the pull channels");
    assert!(
        first.sends.is_empty() && attempted(&first).is_empty(),
        "no push (spec §3.1)"
    );
}

/// The send times spec §5.3 lists: at once, then 15 s, 60 s, 5 min, then every 15 min, each after
/// the attempt before it.
const GAPS_S: [i64; 6] = [15, 60, 300, 900, 900, 900];

fn fail_retryably(d: &mut Dispatcher, ctx: &mut Fixture, sent: &Step, now: i64) -> Step {
    let mut out = Step::default();
    for s in &sent.sends {
        let got = step(
            d,
            Input::Outcome {
                notice: s.notification.notice,
                recipient: s.recipient.clone(),
                channel: s.channel,
                attempt: s.attempt,
                result: SendResult::Retryable(Reason::Timeout),
                at: at(now),
            },
            ctx,
        );
        out.drafts.extend(got.drafts);
        out.sends.extend(got.sends);
        out.next_wake = got.next_wake.or(out.next_wake);
    }
    out
}

#[test]
#[ignore = "pending E8-10"]
fn retries_follow_the_schedule_and_number_each_attempt() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    ctx.channels = vec![Channel::Email];
    let mut sent = step(
        &mut d,
        alert("E1", NoticeKind::Protection, vec![owner()], 0),
        &mut ctx,
    );
    let mut now = 0;
    for (n, gap) in GAPS_S.iter().enumerate() {
        let failed = fail_retryably(&mut d, &mut ctx, &sent, now);
        assert!(failed.sends.is_empty(), "nothing is resent before its time");
        assert_eq!(
            failed.next_wake,
            Some(at(now + gap)),
            "attempt {} waits {gap} s",
            n + 2
        );
        let early = step(
            &mut d,
            Input::Tick {
                now: at(now + gap - 1),
            },
            &mut ctx,
        );
        assert!(early.sends.is_empty());
        now += gap;
        sent = step(&mut d, Input::Tick { now: at(now) }, &mut ctx);
        let attempts: Vec<u32> = sent.sends.iter().map(|s| s.attempt).collect();
        assert_eq!(attempts, vec![u32::try_from(n + 2).unwrap_or(0)]);
    }
}

#[test]
#[ignore = "pending E8-10"]
fn safety_retries_end_at_24_hours_and_info_at_6_with_retry_window_ended() {
    for (kind, window_s) in [
        (NoticeKind::Protection, 86_400),
        (NoticeKind::DailyBrief, 21_600),
    ] {
        let mut d = Dispatcher::new(receive_table());
        let mut ctx = Fixture::new(Quiet::Open);
        ctx.channels = vec![Channel::Email];
        let mut sent = step(&mut d, alert("E1", kind, vec![owner()], 0), &mut ctx);
        let mut now = 0;
        let mut last_send = 0;
        let mut abandoned = Vec::new();
        for _ in 0..500 {
            if !sent.sends.is_empty() {
                last_send = now;
            }
            let failed = fail_retryably(&mut d, &mut ctx, &sent, now);
            abandoned.extend(
                attempted(&failed)
                    .into_iter()
                    .filter(|a| a.3 == Status::Abandoned),
            );
            let Some(wake) = failed.next_wake else { break };
            assert!(wake > at(now), "{kind:?}: every retry waits");
            now = wake.secs() - T0;
            sent = step(&mut d, Input::Tick { now: wake }, &mut ctx);
            abandoned.extend(
                attempted(&sent)
                    .into_iter()
                    .filter(|a| a.3 == Status::Abandoned),
            );
            assert!(now <= window_s, "{kind:?}: no attempt after {window_s} s");
        }
        assert!(
            last_send > window_s - 900,
            "{kind:?}: retried into the window's last 15 minutes, last at {last_send} s"
        );
        assert_eq!(abandoned.len(), 1, "{kind:?}: abandoned once");
        assert_eq!(abandoned[0].4, Some(Reason::RetryWindowEnded));
    }
}

fn outcome(sent: &Step, result: SendResult, at_s: i64) -> Input {
    let s = sent
        .sends
        .first()
        .unwrap_or_else(|| panic!("nothing was sent"));
    Input::Outcome {
        notice: s.notification.notice,
        recipient: s.recipient.clone(),
        channel: s.channel,
        attempt: s.attempt,
        result,
        at: at(at_s),
    }
}

#[test]
#[ignore = "pending E8-10"]
fn a_retry_due_exactly_at_the_windows_end_is_sent_and_one_due_past_it_is_not() {
    for (kind, window_s) in [
        (NoticeKind::Protection, 86_400),
        (NoticeKind::DailyBrief, 21_600),
    ] {
        let mut d = Dispatcher::new(receive_table());
        let mut ctx = Fixture::new(Quiet::Open);
        ctx.channels = vec![Channel::Email];
        let first = step(&mut d, alert("E1", kind, vec![owner()], 0), &mut ctx);
        let timeout = SendResult::Retryable(Reason::Timeout);
        let failed = step(
            &mut d,
            outcome(&first, timeout.clone(), window_s - 15),
            &mut ctx,
        );
        assert_eq!(
            failed.next_wake,
            Some(at(window_s)),
            "{kind:?}: due at the end"
        );
        let second = step(&mut d, Input::Tick { now: at(window_s) }, &mut ctx);
        assert_eq!(second.sends.len(), 1, "{kind:?}: sent at the window's end");
        let past = step(&mut d, outcome(&second, timeout, window_s), &mut ctx);
        assert_eq!(past.next_wake, None, "{kind:?}: nothing due past the end");
        assert!(
            attempted(&past)
                .iter()
                .any(|a| a.3 == Status::Abandoned && a.4 == Some(Reason::RetryWindowEnded)),
            "{kind:?}: a retry due past the end is abandoned"
        );
    }
}

#[test]
#[ignore = "pending E8-10"]
fn an_outcome_for_a_finished_or_superseded_attempt_changes_nothing() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    ctx.channels = vec![Channel::Email];
    let first = step(
        &mut d,
        alert("E1", NoticeKind::Protection, vec![owner()], 0),
        &mut ctx,
    );
    let accepted = SendResult::Accepted {
        provider_message_id: "pm-1".to_owned(),
    };
    let delivered = step(&mut d, outcome(&first, accepted.clone(), 1), &mut ctx);
    assert_eq!(attempted(&delivered).len(), 1);
    let again = step(&mut d, outcome(&first, accepted, 2), &mut ctx);
    assert!(
        again.drafts.is_empty(),
        "a duplicate outcome records nothing"
    );
    let late = step(
        &mut d,
        outcome(&first, SendResult::Retryable(Reason::Timeout), 3),
        &mut ctx,
    );
    assert!(
        late.drafts.is_empty() && late.next_wake.is_none(),
        "an outcome after delivery schedules nothing"
    );
}

#[test]
#[ignore = "pending E8-10"]
fn action_retries_stop_as_not_pending_when_the_approval_closes() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    ctx.channels = vec![Channel::Email];
    let sent = step(&mut d, approval(3_600), &mut ctx);
    let failed = fail_retryably(&mut d, &mut ctx, &sent, 0);
    assert!(failed.next_wake.is_some());
    let closed = step(
        &mut d,
        Input::ApprovalClosed {
            approval: event("A1"),
            at: at(5),
        },
        &mut ctx,
    );
    let stops = attempted(&closed);
    assert_eq!(stops.len(), 1);
    assert_eq!(
        (stops[0].3, stops[0].4),
        (Status::Abandoned, Some(Reason::NotPending))
    );
    let later = step(&mut d, Input::Tick { now: at(15) }, &mut ctx);
    assert!(later.sends.is_empty(), "no retry after the approval closed");
}

#[test]
#[ignore = "pending E8-10"]
fn action_retries_stop_at_the_deadline_without_a_closing_event() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Open);
    ctx.channels = vec![Channel::Email];
    let sent = step(&mut d, approval(70), &mut ctx);
    let failed = fail_retryably(&mut d, &mut ctx, &sent, 0);
    let second = step(
        &mut d,
        Input::Tick {
            now: failed.next_wake.unwrap_or(at(15)),
        },
        &mut ctx,
    );
    let failed = fail_retryably(&mut d, &mut ctx, &second, 15);
    let due = step(&mut d, Input::Tick { now: at(75) }, &mut ctx);
    assert!(
        failed.sends.is_empty() && due.sends.is_empty(),
        "nothing is sent at or past the deadline"
    );
    let stops: Vec<_> = attempted(&failed)
        .into_iter()
        .chain(attempted(&due))
        .filter(|a| a.3 == Status::Abandoned)
        .collect();
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].4, Some(Reason::NotPending));
}

#[test]
#[ignore = "pending E8-10"]
fn a_permanent_failure_stops_only_that_channel() {
    for reason in [
        Reason::AddressRejected,
        Reason::RecipientNotPermitted,
        Reason::AuthFailed,
    ] {
        let mut d = Dispatcher::new(receive_table());
        let mut ctx = Fixture::new(Quiet::Open);
        let sent = step(
            &mut d,
            alert("E1", NoticeKind::RiskLimit, vec![owner()], 0),
            &mut ctx,
        );
        let email = sent
            .sends
            .iter()
            .find(|s| s.channel == Channel::Email)
            .unwrap_or_else(|| panic!("no email send"));
        let out = step(
            &mut d,
            Input::Outcome {
                notice: email.notification.notice,
                recipient: email.recipient.clone(),
                channel: Channel::Email,
                attempt: 1,
                result: SendResult::Permanent(reason),
                at: at(1),
            },
            &mut ctx,
        );
        assert_eq!(
            attempted(&out),
            vec![(
                user("u-owner"),
                Channel::Email,
                1,
                Status::Failed,
                Some(reason)
            )]
        );
        let later = step(&mut d, Input::Tick { now: at(3_600) }, &mut ctx);
        assert!(
            later.sends.iter().all(|s| s.channel != Channel::Email),
            "{reason:?}: a permanent failure is never retried"
        );
    }
}

#[test]
#[ignore = "pending E8-10"]
fn a_lost_address_is_told_on_the_members_other_channels() {
    let mut d = Dispatcher::new(receive_table());
    let mut ctx = Fixture::new(Quiet::Quiet { until: at(36_000) });
    ctx.channels = vec![Channel::Email, Channel::Slack, Channel::Telegram];
    let lost = Input::Committed {
        event: event("N9"),
        stream: "ntf:w1".to_owned(),
        at: at(0),
        cause: Cause::AddressLost {
            member: owner(),
            channel: Channel::Email,
        },
    };
    let out = step(&mut d, lost, &mut ctx);
    let notices = issued(&out);
    assert_eq!(notices.len(), 1);
    assert_eq!(
        (notices[0].1, &notices[0].2),
        (NoticeKind::ChannelLost, &event("N9"))
    );
    let channels: Vec<Channel> = out.sends.iter().map(|s| s.channel).collect();
    assert_eq!(
        channels,
        vec![Channel::Slack, Channel::Telegram],
        "every other channel, quiet hours or not"
    );
    assert!(
        out.sends
            .iter()
            .all(|s| s.notification.text == TextKey::AccountChanged)
    );
}
