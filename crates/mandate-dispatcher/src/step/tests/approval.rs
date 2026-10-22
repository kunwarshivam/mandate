//! E8-10 slice D7a (notifications spec §3.2, §3.3, §3.4, §5.1, §5.3, NT-1, NT-8; journal spec
//! §9.15 rules 124 and 125; DEC-2501): a committed `ApprovalRequested` on an agent stream is a
//! cause, as an `OwnerAlertSent` is. Every oracle here reads the committed journal and the
//! provider's captures, and takes what a notice must look like from the spec's tables typed in this
//! file, never from the step, `due`, or `mandate_notify`'s kind table.

use super::{
    A1, AUDIENCE, BUILD, Checked, Counter, NTF, Shared, Stepped, T, Watching, alerts, draft, id,
    now, payloads, provider, text, world, writer,
};
use crate::DispatchError;
use crate::step::{Config, Journal, NoticeWriter, step};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{AppendOutcome, Environment, InvalidReason, StoredEvent, StreamId};
use mandate_notify::canary::scan;
use mandate_notify::{
    AddressHandle, IdempotencyKey, NoticeId, Origin, Outcome, PushChannel, Reason, Recipient, Sent,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config as RunnerConfig, TestCaseError, TestRunner};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

/// The agent stream every approval here is requested on.
pub(super) const AG: &str = "agent:ws_1:agent_a";
/// Spec §4.2's rendered texts, typed here: an approval's, and an alert's.
const APPROVAL_TEXT: &str = "An agent in your workspace needs your approval";
const ALERT_TEXT: &str = "Your workspace has a new alert";
const MANDATE: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
/// The request's trading content, each value written nowhere else in the workspace, so a scan can
/// tell whether any of it left: quantity, limit, order value, score, cap, reference mark, deadline,
/// and the plan's stop and take-profit.
const QTY: &str = "4242.4242";
const LIMIT: &str = "987654.321";
const ORDER: &str = "31415926.5";
const SCORE: &str = "0.8675309";
const CAP: &str = "55555.5";
const MARK_PRICE: &str = "24680.135";
const STOP: &str = "13579.2468";
const TAKE_PROFIT: &str = "97531.8642";
const DEADLINE: &str = "2026-10-09T14:05:00.000000000Z";

/// Opens the agent stream [`AG`] by its owner at epoch 1.
pub(super) fn agent(journal: &Shared) -> Checked {
    let sid = StreamId::parse(AG).ok_or(AG)?;
    let opened = r#"{"stream_type":"agent","workspace_id":"ws_1","agent_id":"agent_a"}"#;
    let opened = draft(
        AG,
        "01J8Z3M40000000000000000AG",
        "StreamOpened",
        "null",
        opened,
    );
    let mut j = journal.0.borrow_mut();
    let owner = j.take_ownership(&sid);
    let appended = j.append(&sid, 0, owner, now()?, &[&opened]);
    assert_eq!(appended.name(), "Committed", "{appended:?}");
    Ok(())
}

/// The event id of approval `n`: its last digit 2, beside a subject's 0 and an alert's 1.
pub(super) fn approval_id(n: u64) -> String {
    id(n, 2)
}

/// Commits, as the runtime does, the `ApprovalRequested` `approval_id(n)` on [`AG`] at `version`
/// (1, or 2 with the order builder's plan, journal spec §9.6, DEC-942), for `instrument` and the
/// rule `decided_by`, with this file's trading content. Returns its `content_hash`.
pub(super) fn approval(
    journal: &Shared,
    n: u64,
    version: u64,
    instrument: &str,
    decided_by: &str,
) -> Result<String, Box<dyn Error>> {
    let execution = if version == 2 {
        format!(
            r#","execution":{{"asset_class":"us_equity","tif":"gtc","protection_required":true,
            "protection":{{"stop":"{STOP}","take_profit":"{TAKE_PROFIT}"}}}}"#
        )
    } else {
        String::new()
    };
    let content = format!(
        r#"{{"action":{{"instrument":"{instrument}","asset_class":"us_equity","side":"buy",
        "qty":"{QTY}","limit":"{LIMIT}","order_usd":"{ORDER}","purpose":"open"}},
        "trigger":{{"mandate_version":"{MANDATE}","decided_by":"{decided_by}"}},
        "evidence":{{"combined_score":{{"label":"combined model score, not a probability of profit",
        "value":"{SCORE}"}},"outputs":[]}},
        "risk_impact":[{{"field":"order_usd","value":"{ORDER}","cap":"{CAP}"}}],
        "reference_mark":{{"price":"{MARK_PRICE}","seq":7}},"deadline":"{DEADLINE}",
        "default":"If you do nothing, this action is skipped","choices":["approve","skip"],
        "approvers":{{"required":1,"independent":false}}{execution}}}"#
    );
    let canonical = to_canonical(&parse(content.as_bytes())?);
    let hash = format!("sha256:{}", Digest::of(&canonical).to_hex());
    let content = String::from_utf8(canonical)?;
    let deadline = UtcNanos::parse(DEADLINE)?.secs();
    let payload = format!(
        r#"{{"instrument":"{instrument}","asset_class":"us_equity","side":"buy","qty":"{QTY}",
        "limit":"{LIMIT}","purpose":"open","mandate_version":"{MANDATE}","decided_by":"{decided_by}",
        "combined_score":"{SCORE}","reference_mark":{{"price":"{MARK_PRICE}","seq":7}},
        "approvers_required":1,"independent_required":false,"deadline":{deadline},
        "timeout_s":300,"on_timeout":"skip","content":{content},"content_hash":"{hash}"}}"#
    );
    let refs: BTreeSet<&str> = [MANDATE, hash.as_str()].into();
    let refs: Vec<String> = refs.iter().map(|r| format!("\"{r}\"")).collect();
    let event = approval_id(n);
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event}","stream_id":"{AG}",
        "event_type":"ApprovalRequested","schema_version":{version},"event_time":"{T}",
        "clock_source":"local","causation_id":null,"correlation_id":null,
        "actor":{{"kind":"agent","id":"agent_a","version":"0.1.0","build":"{BUILD}"}},
        "config_refs":{{"mandate_version":"{MANDATE}"}},"payload":{payload},
        "artifact_refs":[{}],"pii_refs":[]}}"#,
        refs.join(",")
    );
    let sid = StreamId::parse(AG).ok_or(AG)?;
    let mut j = journal.0.borrow_mut();
    let head = j.head(&sid).seq;
    let appended = j.append(&sid, head, 1, now()?, &[body.as_bytes()]);
    assert_eq!(appended.name(), "Committed", "{appended:?}");
    Ok(hash)
}

/// One step at `at` by `writer` over `subjects` to [`AUDIENCE`], with the step's own answer inside.
fn step_at(
    journal: &Shared,
    writer: &NoticeWriter,
    subjects: &[&str],
    sends: &mut Watching,
    at: UtcNanos,
) -> Stepped {
    let subjects: Vec<StreamId> = subjects.iter().filter_map(|s| StreamId::parse(s)).collect();
    let origin = Origin::parse("https://app.example.invalid")?;
    let config = Config {
        writer,
        subjects: &subjects,
        audience: &AUDIENCE,
        origin: &origin,
        environment: Environment::Paper,
        build: BUILD,
    };
    let mut random = Counter(sends.draws);
    let stepped = step(&config, &mut journal.clone(), &mut random, sends, at);
    sends.draws = random.0;
    Ok(stepped)
}

/// `secs` seconds after [`T`].
fn after(secs: i64) -> Result<UtcNanos, Box<dyn Error>> {
    let start = UtcNanos::parse(T)?;
    let later = start
        .secs()
        .checked_add(secs)
        .ok_or("an instant in range")?;
    Ok(UtcNanos::from_parts(later, 0)?)
}

/// The notice stream's committed rows.
fn notice_rows(journal: &Shared) -> Result<Vec<StoredEvent>, Box<dyn Error>> {
    Ok(journal.rows(&StreamId::parse(NTF).ok_or(NTF)?))
}

/// The `NoticeIssued` payloads that name `cause`.
fn issued_for(journal: &Shared, cause: &str) -> Vec<Value> {
    payloads(journal, "NoticeIssued")
        .into_iter()
        .filter(|p| text(p, "cause") == cause)
        .collect()
}

/// The captures of `notice`'s sends.
fn sent_for<'a>(sent: &'a [Sent], notice: &str) -> Vec<&'a Sent> {
    sent.iter()
        .filter(|s| s.message.ends_with(notice))
        .collect()
}

/// One `NoticeIssued` written by hand: its notice id, kind, class, cause, and cause stream.
type ByHand<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);

/// Appends to the notice stream, by hand and at the next writer epoch, `StreamOpened` and one
/// `NoticeIssued` per [`ByHand`], each listing the founder, and returns the journal's answer.
/// Draft ids are `id(70 + i, 3)`.
fn issue_by_hand(journal: &Shared, notices: &[ByHand<'_>]) -> AppendOutcome {
    let Some(ntf) = StreamId::parse(NTF) else {
        return AppendOutcome::Unavailable;
    };
    let opened = r#"{"stream_type":"notice","workspace_id":"ws_1"}"#;
    let mut drafts = vec![draft(NTF, &id(69, 3), "StreamOpened", "null", opened)];
    for (i, &(notice, kind, class, cause, cause_stream)) in (70u64..).zip(notices) {
        let payload = format!(
            r#"{{"notice":"{notice}","kind":"{kind}","class":"{class}","cause":"{cause}",
            "cause_stream":"{cause_stream}","recipients":["founder"]}}"#
        );
        let caused = format!("\"{cause}\"");
        drafts.push(draft(NTF, &id(i, 3), "NoticeIssued", &caused, &payload));
    }
    let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    let Ok(at) = now() else {
        return AppendOutcome::Unavailable;
    };
    let mut j = journal.0.borrow_mut();
    let epoch = j.take_ownership(&ntf);
    j.append(&ntf, 0, epoch, at, &drafts)
}

/// Spec §3.2's class of each kind these tests commit, typed here.
fn class_of(kind: &str) -> &'static str {
    match kind {
        "approval_requested" => "action",
        _ => "safety",
    }
}

/// What the oracle expects for one cause key: the events that share it, its kind, and its stream.
struct Expected {
    events: BTreeSet<String>,
    kind: String,
    stream: String,
}

/// The interleaving oracle (spec §3.4, §5.1, NT-8). From the committed rows of [`A1`] and [`AG`]
/// alone it derives one cause key per `OwnerAlertSent` (its `owner_command`, or its own event id)
/// and per `ApprovalRequested` (its own event id), and requires exactly one `NoticeIssued` per key,
/// none for anything else, each with the key's kind, §3.2's class for it, the cause's stream,
/// `causation_id` equal to its cause, a notice id that is not its cause's event id, and the
/// founder as its one recipient (§3.3's interim); then exactly one capture per configured channel
/// for each notice, rendered with §4.2's text for its class, and no capture for anything else.
fn one_notice_per_cause(journal: &Shared, sent: &[Sent]) -> Result<(), String> {
    let mut want: BTreeMap<String, Expected> = BTreeMap::new();
    {
        let j = journal.0.borrow();
        for stream in [A1, AG] {
            let sid = StreamId::parse(stream).ok_or(stream)?;
            for row in j.rows(&sid) {
                let payload = parse(&row.body)
                    .ok()
                    .and_then(|b| b.get("payload").cloned())
                    .unwrap_or(Value::Null);
                let (key, kind) = match row.event_type.as_str() {
                    "OwnerAlertSent" => {
                        let command = payload.get("owner_command").and_then(Value::as_str);
                        let key = command.map_or_else(|| row.event_id.clone(), str::to_owned);
                        (key, text(&payload, "kind"))
                    }
                    "ApprovalRequested" => (row.event_id.clone(), "approval_requested".to_owned()),
                    _ => continue,
                };
                let entry = want.entry(key).or_insert_with(|| Expected {
                    events: BTreeSet::new(),
                    kind,
                    stream: stream.to_owned(),
                });
                entry.events.insert(row.event_id.clone());
            }
        }
    }
    let rows = notice_rows(journal).map_err(|e| e.to_string())?;
    let mut got: BTreeMap<String, usize> = BTreeMap::new();
    let mut notices: Vec<(String, &str)> = Vec::new();
    for row in rows.iter().filter(|r| r.event_type == "NoticeIssued") {
        let body = parse(&row.body).map_err(|e| format!("{e:?}"))?;
        let p = body.get("payload").cloned().unwrap_or(Value::Null);
        let cause = text(&p, "cause");
        let (key, expected) = want
            .iter()
            .find(|(_, e)| e.events.contains(&cause))
            .ok_or(format!("a notice for {cause}, which is no cause"))?;
        let fields = ["kind", "class", "cause_stream"].map(|m| text(&p, m));
        let typed = [
            expected.kind.clone(),
            class_of(&expected.kind).to_owned(),
            expected.stream.clone(),
        ];
        if fields != typed {
            return Err(format!("{cause}: {fields:?}, not {typed:?}"));
        }
        if text(&body, "causation_id") != cause {
            return Err(format!("{cause}: causation_id is not its cause"));
        }
        let notice = text(&p, "notice");
        if notice.eq_ignore_ascii_case(&cause) || NoticeId::parse(&notice).is_err() {
            return Err(format!("{cause}: {notice} is no minted notice id"));
        }
        let recipients = p.get("recipients").and_then(Value::as_array);
        let listed: Vec<&str> = recipients
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        if listed != ["founder"] {
            return Err(format!("{cause}: recipients {listed:?}"));
        }
        let count = got.entry(key.clone()).or_default();
        *count = count.wrapping_add(1);
        let rendered = if expected.kind == "approval_requested" {
            APPROVAL_TEXT
        } else {
            ALERT_TEXT
        };
        notices.push((notice, rendered));
    }
    let keys: Vec<&String> = want.keys().collect();
    let issued: Vec<&String> = got.keys().collect();
    if issued != keys || got.values().any(|&n| n != 1) {
        return Err(format!("one notice per cause: {got:?} for {keys:?}"));
    }
    for (notice, rendered) in &notices {
        let mine = sent_for(sent, notice);
        let channels: BTreeSet<PushChannel> = mine.iter().map(|s| s.address.channel).collect();
        let configured: BTreeSet<PushChannel> = AUDIENCE.iter().map(|&(_, c)| c).collect();
        if mine.len() != AUDIENCE.len() || channels != configured {
            return Err(format!("{notice}: {} sends on {channels:?}", mine.len()));
        }
        if let Some(s) = mine.iter().find(|s| !s.message.starts_with(rendered)) {
            return Err(format!("{notice}: rendered {:?}", s.message));
        }
    }
    let ours = |s: &&Sent| notices.iter().any(|(n, _)| s.message.ends_with(n.as_str()));
    if sent.iter().filter(ours).count() != sent.len() {
        return Err("a send no notice lists".to_owned());
    }
    Ok(())
}

/// Spec §3.2 and §3.4, journal spec rules 124 and 125, DEC-2501 items 1 and 2: each request, at
/// either version, is one `approval_requested` notice of class `action` naming the request and its
/// agent stream, which `MemoryJournal` admits; a second step issues nothing more, and the alert
/// beside the requests keeps its one notice.
#[test]
fn an_approval_request_is_one_action_notice_naming_it_on_its_agent_stream() -> Checked {
    let journal = world()?;
    agent(&journal)?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    approval(&journal, 20, 1, "aapl", "rule:large_order")?;
    approval(&journal, 21, 2, "msft", "rule:large_order")?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    step_at(&journal, &writer, &[A1, AG], &mut sends, now()?)??;
    step_at(&journal, &writer, &[A1, AG], &mut sends, now()?)??;
    for n in [20, 21] {
        let cause = approval_id(n);
        let issued = issued_for(&journal, &cause);
        let [notice] = issued.as_slice() else {
            return Err(format!("{cause}: one notice over two steps, not {}", issued.len()).into());
        };
        let fields = ["kind", "class", "cause_stream"].map(|m| text(notice, m));
        assert_eq!(fields, ["approval_requested", "action", AG], "{cause}");
        assert_ne!(text(notice, "notice").to_uppercase(), cause, "a minted id");
    }
    let alert = issued_for(&journal, &id(1, 1));
    assert_eq!(alert.len(), 1, "the alert beside them keeps its one notice");
    one_notice_per_cause(&journal, sends.fx.sent()?)?;
    assert_eq!(sends.fx.sent()?.len(), 6, "three notices on two channels");
    Ok(())
}

/// Spec §5.1 steps 2 and 3, NT-8, DEC-710 item 5: the approval notice is on the notice stream
/// before each of its sends, there is one send and one `NoticeAttempted` per audience channel, and
/// each send carries the digest of its own `(notice, recipient, channel)`.
#[test]
fn an_approval_notice_is_issued_before_any_send_with_one_attempt_and_key_per_channel() -> Checked {
    let journal = world()?;
    agent(&journal)?;
    approval(&journal, 20, 2, "aapl", "rule:large_order")?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    step_at(&journal, &writer, &[AG], &mut sends, now()?)??;
    let issued = issued_for(&journal, &approval_id(20));
    let [issued] = issued.as_slice() else {
        return Err(format!("one notice, not {}", issued.len()).into());
    };
    let notice = text(issued, "notice");
    let seen: Vec<_> = sends.seen.iter().filter(|(n, _, _)| *n == notice).collect();
    assert_eq!(seen.len(), 2, "one send per audience channel");
    for (_, channel, state) in &seen {
        assert_eq!(*state, (true, false), "issued, not attempted: {channel}");
    }
    let id = NoticeId::parse(&notice)?;
    let mut keys = BTreeSet::new();
    for sent in sent_for(sends.fx.sent()?, &notice) {
        let address = AddressHandle {
            recipient: Recipient::parse("founder")?,
            channel: sent.address.channel,
        };
        assert_eq!(
            sent.key,
            IdempotencyKey::of(&id, &address)?.hex()?,
            "its own key"
        );
        keys.insert(sent.key.clone());
    }
    assert_eq!(keys.len(), 2, "each address has its own key");
    let attempts = payloads(&journal, "NoticeAttempted");
    let mut records: Vec<[String; 3]> = attempts
        .iter()
        .filter(|p| text(p, "notice") == notice)
        .map(|p| ["channel", "status", "recipient"].map(|m| text(p, m)))
        .collect();
    records.sort();
    let want = [
        ["email", "delivered", "founder"].map(String::from),
        ["phone", "delivered", "founder"].map(String::from),
    ];
    assert_eq!(records, want, "one attempt per channel");
    Ok(())
}

/// NT-1, spec §4.2, DEC-2501 item 5: a request whose instrument and rule are canaries and whose
/// prices, plan, deadline, and hash are unique strings is sent as the fixed text and the link only;
/// no canary or content string reaches a capture or any notice-stream record, and no capture holds
/// the request's event id.
#[test]
fn an_approval_notice_carries_none_of_its_requests_content() -> Checked {
    let journal = world()?;
    agent(&journal)?;
    let hash = approval(&journal, 20, 2, "zqinstrument", "rule:zqrule")?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    step_at(&journal, &writer, &[AG], &mut sends, now()?)??;
    let issued = issued_for(&journal, &approval_id(20));
    let [issued] = issued.as_slice() else {
        return Err(format!("one notice, not {}", issued.len()).into());
    };
    let notice = text(issued, "notice");
    let sent = sends.fx.sent()?;
    assert_eq!(sent_for(sent, &notice).len(), 2, "sent on both channels");
    let link = format!("https://app.example.invalid/n/{notice}");
    for s in sent {
        assert_eq!(
            s.message,
            format!("{APPROVAL_TEXT}\n{link}"),
            "fixed text and link"
        );
    }
    let deadline = UtcNanos::parse(DEADLINE)?.secs().to_string();
    let content = [
        QTY,
        LIMIT,
        ORDER,
        SCORE,
        CAP,
        MARK_PRICE,
        STOP,
        TAKE_PROFIT,
        "2026-10-09T14:05",
        &deadline,
        "us_equity",
        "large_order",
        MANDATE,
        &hash,
    ];
    let mut captured: Vec<Vec<u8>> = sent
        .iter()
        .flat_map(|s| [s.message.clone().into_bytes(), s.key.clone().into_bytes()])
        .collect();
    let event = approval_id(20);
    for bytes in &captured {
        let leaked = String::from_utf8_lossy(bytes);
        assert!(!leaked.contains(&event), "no event id leaves: {leaked}");
    }
    captured.extend(notice_rows(&journal)?.into_iter().map(|r| r.body));
    for bytes in &captured {
        assert_eq!(scan(bytes)?, Vec::<&str>::new(), "no canary");
        let read = String::from_utf8_lossy(bytes);
        for needle in &content {
            assert!(!read.contains(needle), "{needle} in {read}");
        }
    }
    Ok(())
}

/// Spec §5.3, DEC-707 items 1, 2, and 4, DEC-2501 item 4: with every send timing out, steps at 0 s,
/// 25 hours, and 25 hours and 61 s make attempts 1, 2, and 3 of the approval notice on each channel,
/// none abandoned, while the alert beside it makes attempts 1 and 2 and is abandoned at 3.
#[test]
fn an_approval_notice_retries_with_no_window_while_an_alert_is_abandoned_at_its_end() -> Checked {
    let journal = world()?;
    agent(&journal)?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    approval(&journal, 20, 2, "aapl", "rule:large_order")?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    let timeout = Outcome::Retryable {
        reason: Reason::Timeout,
    };
    sends.script = vec![
        (PushChannel::Email, timeout.clone()),
        (PushChannel::Phone, timeout),
    ];
    for secs in [0, 90_000, 90_061] {
        step_at(&journal, &writer, &[A1, AG], &mut sends, after(secs)?)??;
    }
    let notice_of = |cause: &str| {
        issued_for(&journal, cause)
            .first()
            .map(|p| text(p, "notice"))
    };
    let approval = notice_of(&approval_id(20)).ok_or("the approval's notice")?;
    let alert = notice_of(&id(1, 1)).ok_or("the alert's notice")?;
    let attempts = payloads(&journal, "NoticeAttempted");
    let history = |notice: &str, channel: &str| -> Vec<(i64, String, String)> {
        attempts
            .iter()
            .filter(|p| text(p, "notice") == notice && text(p, "channel") == channel)
            .map(|p| {
                let n = p.get("attempt").and_then(Value::as_int).unwrap_or_default();
                let n = i64::try_from(n).unwrap_or_default();
                (n, text(p, "status"), text(p, "reason"))
            })
            .collect()
    };
    let failed = |n: i64| (n, "failed".to_owned(), "timeout".to_owned());
    for channel in ["email", "phone"] {
        assert_eq!(
            history(&approval, channel),
            [failed(1), failed(2), failed(3)],
            "DEC-707 item 1: an action send has no window until D7b, {channel}"
        );
        let abandoned = (3, "abandoned".to_owned(), "retry_window_ended".to_owned());
        assert_eq!(
            history(&alert, channel),
            [failed(1), failed(2), abandoned],
            "spec §5.3: the alert's 24 hours, {channel}"
        );
    }
    Ok(())
}

/// One operation of the interleaving property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// An alert of [`KINDS`]'s kind on [`A1`].
    Alert(usize),
    /// An `ApprovalRequested` on [`AG`], at version 1 or 2.
    Approval(u64),
    /// A step by the current dispatcher.
    Step,
    /// A new dispatcher: a new writer epoch, which fences the old one, and no state carried.
    Restart,
}

/// Alert kinds the property commits; every `kill_switch` alert names one owner command, so all of
/// them are one cause.
const KINDS: [&str; 4] = ["risk_limit", "agent_held", "protection", "kill_switch"];

fn operations() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        (0_usize..KINDS.len()).prop_map(Op::Alert),
        (1_u64..=2).prop_map(Op::Approval),
        Just(Op::Step),
        Just(Op::Restart),
    ];
    prop::collection::vec(op, 1..24)
}

/// Runs `ops` and a last step, checking [`one_notice_per_cause`] after every step.
fn interleave(ops: &[Op]) -> Result<(), String> {
    let fail = |e: Box<dyn Error>| e.to_string();
    let journal = world().map_err(fail)?;
    agent(&journal).map_err(fail)?;
    let mut sends = provider(&journal).map_err(|e| e.to_string())?;
    let mut current = writer(&journal).map_err(|e| e.to_string())?;
    let mut n = 10_u64;
    let last = [Op::Step];
    for op in ops.iter().chain(&last) {
        match *op {
            Op::Alert(kind) => {
                let kind = KINDS.get(kind).ok_or("a kind")?;
                alerts(&journal, A1, 1, &[(n, kind)]).map_err(fail)?;
                n = n.wrapping_add(1);
            }
            Op::Approval(version) => {
                approval(&journal, n, version, "aapl", "rule:large_order").map_err(fail)?;
                n = n.wrapping_add(1);
            }
            Op::Restart => current = writer(&journal).map_err(|e| e.to_string())?,
            Op::Step => {
                let at = now().map_err(fail)?;
                let stepped = step_at(&journal, &current, &[A1, AG], &mut sends, at);
                stepped.map_err(fail)?.map_err(|e| format!("{e:?}"))?;
                let sent = sends.fx.sent().map_err(|e| e.to_string())?;
                one_notice_per_cause(&journal, sent)?;
            }
        }
    }
    Ok(())
}

/// The interleaving property (spec §3.4, §5.1, NT-8; DEC-2501): over random sequences of alerts,
/// approval requests at both versions, steps, and restarts, every step leaves exactly one notice
/// per cause, none duplicated across steps or restarts, approvals as `action` notices on their
/// agent stream and alerts exactly as without them. A proptest `TestRunner` in a plain function,
/// as a macro-generated pending test is refused by `cargo xtask markers`.
#[test]
fn approvals_and_alerts_interleave_into_one_notice_per_cause_across_steps_and_restarts() {
    let config = RunnerConfig {
        failure_persistence: None,
        ..RunnerConfig::default()
    };
    let checked = TestRunner::new(config).run(&operations(), |ops| {
        interleave(&ops).map_err(TestCaseError::fail)
    });
    assert_eq!(checked, Ok(()));
}

/// The interleaving oracle is trusted only once it is shown to refuse seeded bugs: an approval
/// with no notice, one with two, and one issued as a `safety` kind; and to accept the alerts-only
/// journal the step writes today.
#[test]
fn the_interleaving_oracle_catches_a_missing_a_doubled_and_a_wrongly_kinded_approval_notice()
-> Checked {
    let alone = world()?;
    agent(&alone)?;
    alerts(&alone, A1, 1, &[(1, "risk_limit"), (2, "kill_switch")])?;
    let (writer, mut sends) = (writer(&alone)?, provider(&alone)?);
    step_at(&alone, &writer, &[A1, AG], &mut sends, now()?)??;
    one_notice_per_cause(&alone, sends.fx.sent()?)?;
    let cause = approval_id(20);
    let (aa, bb, cc) = ("aa".repeat(16), "bb".repeat(16), "cc".repeat(16));
    let seeded: [(&[ByHand<'_>], &str); 3] = [
        (&[], "one notice per cause"),
        (
            &[
                (&aa, "approval_requested", "action", &cause, AG),
                (&bb, "approval_requested", "action", &cause, AG),
            ],
            "one notice per cause",
        ),
        (
            &[(&cc, "risk_limit", "safety", &cause, AG)],
            "\"risk_limit\", \"safety\"",
        ),
    ];
    for (notices, caught) in seeded {
        let journal = world()?;
        agent(&journal)?;
        approval(&journal, 20, 2, "aapl", "rule:large_order")?;
        if !notices.is_empty() {
            let appended = issue_by_hand(&journal, notices);
            assert_eq!(appended.name(), "Committed", "{appended:?}");
        }
        let verdict = one_notice_per_cause(&journal, &[]);
        let named = verdict.as_ref().err().is_some_and(|e| e.contains(caught));
        assert!(named, "{notices:?}: {verdict:?}");
    }
    Ok(())
}

/// Journal spec §9.15 rule 125: an approval notice's `cause_stream` is an agent stream of the
/// notice stream's workspace, and the journal refuses any other as `stream_mismatch`.
#[test]
fn rule_125_admits_an_approval_notice_only_from_an_agent_stream_of_its_workspace() -> Checked {
    let cause = approval_id(20);
    let notice = "dd".repeat(16);
    let cases = [
        (AG, None),
        (A1, Some(InvalidReason::StreamMismatch)),
        ("ctl:ws_1", Some(InvalidReason::StreamMismatch)),
        ("agent:ws_2:agent_a", Some(InvalidReason::StreamMismatch)),
    ];
    for (stream, refused) in cases {
        let journal = world()?;
        let notices = [(
            notice.as_str(),
            "approval_requested",
            "action",
            cause.as_str(),
            stream,
        )];
        match (issue_by_hand(&journal, &notices), refused) {
            (AppendOutcome::Committed(_), None) => {}
            (AppendOutcome::Invalid { draft: 1, error }, Some(reason)) => {
                assert_eq!(error.reason, reason, "{stream}");
                assert_eq!(error.path, "payload.cause_stream", "{stream}");
            }
            (other, _) => return Err(format!("{stream}: {other:?}").into()),
        }
    }
    Ok(())
}

/// The shared journal, with one forged `ApprovalRequested` row shown on [`A1`]: a copy of a real
/// request's committed row, as no journal would admit there (the catalogue keeps the record to the
/// agent stream), so the step's own stream check is what is tested.
struct Forged {
    journal: Shared,
    row: StoredEvent,
}

impl Journal for Forged {
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        let mut rows = self.journal.committed(stream)?;
        if stream.as_str() == A1 {
            let mut row = self.row.clone();
            row.stream_id = A1.to_owned();
            row.seq = rows.last().map_or(1, |r| r.seq.wrapping_add(1));
            rows.push(row);
        }
        Ok(rows)
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        self.journal.append_notices(writer, head, at, drafts)
    }
}

/// DEC-2501 item 1: only an `ApprovalRequested` on an agent stream is a cause. One shown on an
/// account stream is no cause, and it holds back none of the alerts beside it: were it issued, rule
/// 125 would refuse the step's whole issue batch and every alert in it would wait.
#[test]
fn an_approval_request_off_an_agent_stream_is_no_cause_and_holds_no_alert_back() -> Checked {
    let journal = world()?;
    agent(&journal)?;
    approval(&journal, 20, 2, "aapl", "rule:large_order")?;
    alerts(&journal, A1, 1, &[(1, "risk_limit"), (2, "agent_held")])?;
    let ag = StreamId::parse(AG).ok_or(AG)?;
    let row = journal
        .rows(&ag)
        .into_iter()
        .find(|r| r.event_type == "ApprovalRequested")
        .ok_or("the request")?;
    let mut forged = Forged {
        journal: journal.clone(),
        row,
    };
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    let subjects: Vec<StreamId> = [A1].iter().filter_map(|s| StreamId::parse(s)).collect();
    let origin = Origin::parse("https://app.example.invalid")?;
    let config = Config {
        writer: &writer,
        subjects: &subjects,
        audience: &AUDIENCE,
        origin: &origin,
        environment: Environment::Paper,
        build: BUILD,
    };
    let mut random = Counter(0);
    step(&config, &mut forged, &mut random, &mut sends, now()?)?;
    let issued = payloads(&journal, "NoticeIssued");
    let causes: Vec<[String; 2]> = issued
        .iter()
        .map(|p| ["cause", "kind"].map(|m| text(p, m)))
        .collect();
    let want = [
        [id(1, 1), "risk_limit".to_owned()],
        [id(2, 1), "agent_held".to_owned()],
    ];
    assert_eq!(causes, want, "the alerts, and no approval");
    assert_eq!(sends.fx.sent()?.len(), 4, "each alert on both channels");
    Ok(())
}
