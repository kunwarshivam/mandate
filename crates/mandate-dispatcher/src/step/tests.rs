//! E8-10 slice D2 (DEC-704) over an in-memory journal and the fixture provider. Every oracle reads
//! the journal's rows or the provider's captures, never the step's state: a send is judged against
//! the notice stream as it stood at that send, a cause against the committed subject rows.

use super::{Config, Journal, NoticeWriter, step};
use crate::DispatchError;
use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Environment, MemoryJournal, StoredEvent, StreamId};
use mandate_notify::{
    AddressHandle, FixtureProvider, IdempotencyKey, Notification, NotifyError, Origin, Outcome,
    Provider, PushChannel, Recipient, SecureRandom,
};
use mandate_time::UtcNanos;
use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;

type Checked = Result<(), Box<dyn Error>>;
type Stepped = Result<Result<(), DispatchError>, Box<dyn Error>>;

const NTF: &str = "ntf:ws_1";
const A1: &str = "acct:ws_1:A1";
const A2: &str = "acct:ws_1:A2";
const T: &str = "2026-10-09T14:00:00.000000000Z";
const BUILD: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const AUDIENCE: [(&str, PushChannel); 2] = [
    ("founder", PushChannel::Email),
    ("founder", PushChannel::WebPush),
];
const MARK: &str = r#"{"instrument_id":"inst","price":"1","source":"quote","feed":"iex",
    "risk_clock":"2026-10-09T14:00:00.000000000Z"}"#;

/// The journal under test, shared with the provider so that a send can read it.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<MemoryJournal>>);

impl Journal for Shared {
    fn committed(&self, stream: &StreamId) -> Vec<StoredEvent> {
        self.0.borrow().rows(stream).to_vec()
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        match (writer.stream(), writer.epoch()) {
            (Ok(stream), Ok(epoch)) => self.0.borrow_mut().append(stream, head, epoch, at, drafts),
            _ => AppendOutcome::Unavailable,
        }
    }
}

/// The fixture provider, recording at each send whether the notice stream then held the notice's
/// `NoticeIssued`, and an attempt of it on that channel.
struct Watching {
    journal: Shared,
    fx: FixtureProvider,
    seen: Vec<(String, String, (bool, bool))>,
}

impl Provider for Watching {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let notice = notification.notice.hex()?;
        let channel = address.channel.key()?.to_owned();
        let held = |event_type, by_channel: bool| {
            payloads(&self.journal, event_type).iter().any(|p| {
                text(p, "notice") == notice && (!by_channel || text(p, "channel") == channel)
            })
        };
        let state = (held("NoticeIssued", false), held("NoticeAttempted", true));
        self.seen.push((notice, channel, state));
        self.fx.send(origin, notification, address, key)
    }
}

/// A deterministic source: each draw is 16 copies of the next byte.
struct Counter(u8);

impl SecureRandom for Counter {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        self.0 = self.0.wrapping_add(1);
        *bytes = [self.0; 16];
        Ok(())
    }
}

/// The event id `n` with its last digit `last`: 0 for a subject, 1 for its alert.
fn id(n: u64, last: u8) -> String {
    format!("01J8Z3M4{n:017}{last}")
}

fn draft(stream: &str, id: &str, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{id}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"{BUILD}"}},
        "config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    )
    .into_bytes()
}

/// A journal whose account streams `A1` and `A2` are open, each by its owner at epoch 1.
fn world() -> Result<Shared, Box<dyn Error>> {
    let journal = Shared::default();
    for (stream, account) in [(A1, "A1"), (A2, "A2")] {
        let (sid, mut j) = (
            StreamId::parse(stream).ok_or(stream)?,
            journal.0.borrow_mut(),
        );
        let owner = j.take_ownership(&sid);
        let opened = format!(
            r#"{{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca","account_ref":"{account}"}}"#
        );
        let id = format!("01J8Z3M40000000000000000{account}");
        let first = draft(stream, &id, "StreamOpened", "null", &opened);
        assert_eq!(
            j.append(&sid, 0, owner, now()?, &[&first]).name(),
            "Committed"
        );
    }
    Ok(journal)
}

/// Appends at `epoch`, per `(n, kind)`, a `MarkUpdated` and its alert as one batch (fenced unless
/// at the owner's epoch 1). Every `kill_switch` alert carries out one owner command, `id(9, 0)`.
fn alerts(journal: &Shared, stream: &str, epoch: u64, alerts: &[(u64, &str)]) -> Checked {
    let sid = StreamId::parse(stream).ok_or(stream)?;
    for &(n, kind) in alerts {
        let killed = kind == "kill_switch";
        let command = if killed {
            format!("\"{}\"", id(9, 0))
        } else {
            "null".to_owned()
        };
        let (subject, cause) = (id(n, 0), format!("\"{}\"", id(n, 0)));
        let alert = format!(r#"{{"subject":{cause},"kind":"{kind}","owner_command":{command}}}"#);
        let mark = draft(stream, &subject, "MarkUpdated", "null", MARK);
        let alert = draft(stream, &id(n, 1), "OwnerAlertSent", &cause, &alert);
        let mut j = journal.0.borrow_mut();
        let head = j.head(&sid).seq;
        let fenced = j.append(&sid, head, epoch, now()?, &[&mark, &alert]).name() == "Fenced";
        assert_eq!(fenced, epoch != 1);
    }
    Ok(())
}

fn now() -> Result<UtcNanos, Box<dyn Error>> {
    Ok(UtcNanos::parse(T)?)
}

/// The payloads of the notice stream's committed `event_type` rows, in `seq` order.
fn payloads(journal: &Shared, event_type: &str) -> Vec<Value> {
    let ntf = StreamId::parse(NTF).map(|ntf| journal.committed(&ntf));
    let of = |r: &StoredEvent| r.event_type == event_type;
    let rows = ntf.into_iter().flatten().filter(of);
    let payload = |r: StoredEvent| parse(&r.body).ok()?.get("payload").cloned();
    rows.filter_map(payload).collect()
}

fn text(payload: &Value, member: &str) -> String {
    String::from(payload.get(member).and_then(Value::as_str).unwrap_or(""))
}

fn provider(journal: &Shared) -> Result<Watching, NotifyError> {
    let mut vault = Vec::new();
    for (recipient, channel) in AUDIENCE {
        let recipient = Recipient::parse(recipient)?;
        vault.push((AddressHandle { recipient, channel }, "address".to_owned()));
    }
    let fx = FixtureProvider::holding(vault)?;
    Ok(Watching {
        journal: journal.clone(),
        fx,
        seen: Vec::new(),
    })
}

/// A writer at the notice stream's next writer epoch, which fences every earlier one.
fn writer(journal: &Shared) -> Result<NoticeWriter, DispatchError> {
    let ntf = StreamId::parse(NTF).ok_or(DispatchError::NotANoticeStream)?;
    let epoch = journal.0.borrow_mut().take_ownership(&ntf);
    NoticeWriter::new(ntf, epoch)
}

/// One step by `writer` over `subjects` through `sends`, with the step's own answer inside.
fn run(j: &Shared, writer: &NoticeWriter, subjects: &[&str], sends: &mut Watching) -> Stepped {
    let subjects: Vec<StreamId> = subjects.iter().filter_map(|s| StreamId::parse(s)).collect();
    let origin = Origin::parse("https://app.example.invalid")?;
    let (audience, environment) = (&AUDIENCE[..], Environment::Paper);
    let config = Config {
        writer,
        subjects: &subjects,
        audience,
        origin: &origin,
        environment,
        build: BUILD,
    };
    let (at, mut random) = (now()?, Counter(0));
    Ok(step(&config, &mut j.clone(), &mut random, sends, at))
}

#[test]
#[ignore = "pending E8-10"]
fn a_writer_holds_only_a_notice_stream() -> Checked {
    let ntf = StreamId::parse(NTF).ok_or("ntf")?;
    let writer = NoticeWriter::new(ntf.clone(), 7)?;
    assert_eq!((writer.stream()?, writer.epoch()?), (&ntf, 7));
    for other in [A1, "agent:ws_1:agent_a", "ctl:ws_1", "clock:ws_1"] {
        let refused = NoticeWriter::new(StreamId::parse(other).ok_or(other)?, 1);
        assert_eq!(refused, Err(DispatchError::NotANoticeStream), "{other}");
    }
    Ok(())
}

#[test]
#[ignore = "pending E8-10"]
fn only_committed_alerts_are_causes_and_each_send_follows_its_notice() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit"), (2, "agent_held")])?;
    alerts(&journal, A1, 9, &[(3, "risk_limit")])?;
    let subject = journal.committed(&StreamId::parse(A1).ok_or(A1)?);
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    run(&journal, &writer, &[A1], &mut sends)??;
    let issued = payloads(&journal, "NoticeIssued");
    let causes: Vec<String> = issued.iter().map(|p| text(p, "cause")).collect();
    assert_eq!(causes, [id(1, 1), id(2, 1)], "the fenced alert is no cause");
    assert_eq!(journal.committed(&StreamId::parse(A1).ok_or(A1)?), subject);
    let streams: Vec<String> = journal.0.borrow().stream_ids().map(str::to_owned).collect();
    assert_eq!(streams, [A1, A2, NTF], "only the notice stream is new");
    assert_eq!(sends.seen.len(), 4, "two notices on two channels");
    for (notice, channel, state) in &sends.seen {
        assert_eq!(
            *state,
            (true, false),
            "issued, not attempted: {notice} on {channel}"
        );
        let attempts = payloads(&journal, "NoticeAttempted");
        let of = |p: &&Value| &text(p, "notice") == notice && &text(p, "channel") == channel;
        let after: Vec<String> = attempts
            .iter()
            .filter(of)
            .map(|p| text(p, "status"))
            .collect();
        assert_eq!(after, ["delivered"], "{notice} on {channel}");
    }
    Ok(())
}

#[test]
#[ignore = "pending E8-10"]
fn one_user_kill_switch_is_one_notice_and_a_cause_is_issued_once() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "kill_switch"), (2, "risk_limit")])?;
    alerts(&journal, A2, 1, &[(3, "kill_switch")])?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    run(&journal, &writer, &[A1, A2], &mut sends)??;
    run(&journal, &writer, &[A1, A2], &mut sends)??;
    let issued = payloads(&journal, "NoticeIssued");
    let kinds: Vec<String> = issued.iter().map(|p| text(p, "kind")).collect();
    assert_eq!(kinds, ["kill_switch", "risk_limit"]);
    assert_eq!(sends.fx.sent()?.len(), 4, "the second step sent nothing");
    Ok(())
}

#[test]
#[ignore = "pending E8-10"]
fn a_fenced_dispatcher_sends_nothing_and_the_live_one_sends() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let (stale, live) = (writer(&journal)?, writer(&journal)?);
    let (mut stale_sends, mut live_sends) = (provider(&journal)?, provider(&journal)?);
    let fenced = run(&journal, &stale, &[A1], &mut stale_sends)?;
    assert_eq!(fenced, Err(DispatchError::Fenced { current_epoch: 2 }));
    assert!(stale_sends.fx.sent()?.is_empty());
    assert!(payloads(&journal, "NoticeIssued").is_empty());
    run(&journal, &live, &[A1], &mut live_sends)??;
    assert_eq!(live_sends.fx.sent()?.len(), 2);
    Ok(())
}
