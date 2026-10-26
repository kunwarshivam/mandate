//! E8-10 slice D4 (spec §5.1 step 4, §5.3, §7 "A provider is down"; DEC-706, DEC-707, DEC-710
//! item 5, DEC-725 item 6): the step retries a `retryable` send on §5.3's schedule, ends it on
//! `accepted` or `permanent`, and journals `abandoned` with `retry_window_ended` at the first step
//! at or after its window's end. Every oracle reads the committed notice stream and the provider's
//! captures, and computes each due instant from §5.3's table typed here, never through the step,
//! `due`, or `mandate_notify::next_attempt`.

use super::{A1, AUDIENCE, BUILD, Checked, Counter, NTF, Shared, alerts, text, world, writer};
use crate::step::{Config, NoticeWriter, step};
use crate::{DispatchError, Due, Next, due};
use mandate_canon::{Value, parse};
use mandate_journal::{Environment, StreamId};
use mandate_notify::{
    AddressHandle, IdempotencyKey, NoticeId, Notification, NotifyError, Origin, Outcome, Provider,
    PushChannel, Reason, Recipient,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;

/// §5.3's gaps after attempt 1, 2, and 3, then after every later attempt.
const GAPS: [i64; 3] = [15, 60, 300];
const LATER_GAP: i64 = 900;
/// §5.3's windows from attempt 1: `safety` 24 hours, `info` 6 hours.
const SAFETY_WINDOW: i64 = 86_400;
const INFO_WINDOW: i64 = 21_600;
const CHANNELS: [PushChannel; 2] = [PushChannel::Email, PushChannel::WebPush];

/// The gap §5.3 puts after attempt `n`.
fn gap(n: u32) -> i64 {
    usize::try_from(n.saturating_sub(1))
        .ok()
        .and_then(|i| GAPS.get(i))
        .copied()
        .unwrap_or(LATER_GAP)
}

/// `secs` seconds after the instant every alert here is committed at.
fn at(secs: i64) -> Result<UtcNanos, Box<dyn Error>> {
    let start = UtcNanos::parse(super::T)?;
    let later = start
        .secs()
        .checked_add(secs)
        .ok_or("an instant in range")?;
    Ok(UtcNanos::from_parts(later, 0)?)
}

/// A provider answering each channel from its own queue, and `retryable { timeout }` once the
/// queue is empty, capturing every send's channel, notice, and key.
#[derive(Default)]
struct Queued {
    answers: BTreeMap<PushChannel, VecDeque<Outcome>>,
    sent: Vec<(PushChannel, String, String)>,
}

impl Provider for Queued {
    fn send(
        &mut self,
        _origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let (notice, key) = (notification.notice.hex()?, key.hex()?.to_owned());
        self.sent.push((address.channel, notice, key));
        let queued = self
            .answers
            .get_mut(&address.channel)
            .and_then(VecDeque::pop_front);
        Ok(queued.unwrap_or(Outcome::Retryable {
            reason: Reason::Timeout,
        }))
    }
}

/// One step over `A1` to [`AUDIENCE`] at `secs`, with the step's own answer inside.
fn step_at(
    journal: &Shared,
    writer: &NoticeWriter,
    sends: &mut Queued,
    random: &mut Counter,
    secs: i64,
) -> Result<Result<(), DispatchError>, Box<dyn Error>> {
    let subjects = [StreamId::parse(A1).ok_or(A1)?];
    let origin = Origin::parse("https://app.example.invalid")?;
    let config = Config {
        writer,
        subjects: &subjects,
        audience: &AUDIENCE,
        origin: &origin,
        environment: Environment::Paper,
        build: BUILD,
    };
    Ok(step(
        &config,
        &mut journal.clone(),
        random,
        sends,
        at(secs)?,
    ))
}

/// One committed `NoticeAttempted` as the oracle reads it: channel, attempt, status, reason
/// (empty when `null`), verdict (empty when `null`), and its `event_time` in seconds from [`at`]'s
/// start.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    channel: String,
    attempt: u32,
    status: String,
    reason: String,
    verdict: String,
    secs: i64,
}

/// Every committed `NoticeAttempted`, in `seq` order.
fn records(journal: &Shared) -> Result<Vec<Record>, Box<dyn Error>> {
    let ntf = StreamId::parse(NTF).ok_or(NTF)?;
    let start = at(0)?.secs();
    let mut got = Vec::new();
    for row in journal.rows(&ntf) {
        if row.event_type != "NoticeAttempted" {
            continue;
        }
        let body = parse(&row.body)?;
        let payload = body.get("payload").ok_or("payload")?;
        let event_time = body.get("event_time").and_then(Value::as_str);
        let event_time = UtcNanos::parse(event_time.ok_or("event_time")?)?;
        let attempt = payload.get("attempt").and_then(Value::as_int);
        got.push(Record {
            channel: text(payload, "channel"),
            attempt: u32::try_from(attempt.ok_or("attempt")?)?,
            status: text(payload, "status"),
            reason: text(payload, "reason"),
            verdict: text(payload, "verdict"),
            secs: event_time.secs().checked_sub(start).ok_or("secs")?,
        });
    }
    Ok(got)
}

/// The one notice's id, from its `NoticeIssued`.
fn notice(journal: &Shared) -> Result<NoticeId, Box<dyn Error>> {
    let issued = super::payloads(journal, "NoticeIssued");
    let first = issued.first().ok_or("one notice")?;
    Ok(NoticeId::parse(&text(first, "notice"))?)
}

/// The key of the founder's send of `notice` on `channel`, as DEC-710 item 5 defines it.
fn key_of(notice: &NoticeId, channel: PushChannel) -> Result<String, Box<dyn Error>> {
    let address = AddressHandle {
        recipient: Recipient::parse("founder")?,
        channel,
    };
    Ok(IdempotencyKey::of(notice, &address)?.hex()?.to_owned())
}

fn record(
    channel: &str,
    attempt: u32,
    status: &str,
    reason: &str,
    verdict: &str,
    secs: i64,
) -> Record {
    Record {
        channel: channel.to_owned(),
        attempt,
        status: status.to_owned(),
        reason: reason.to_owned(),
        verdict: verdict.to_owned(),
        secs,
    }
}

/// Spec §5.1 and §5.3, DEC-706 item 4, DEC-710 item 5: a `retryable` send is made again at 15 s
/// and then 60 s after its previous attempt, not a second before, as the next attempt number,
/// under the same key, and journaled with the verdict of the answer's own variant; `accepted` and
/// `permanent` end it, and no later step sends on it again.
#[test]
fn a_retry_is_the_next_attempt_under_one_key_with_its_own_variants_verdict() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let writer = writer(&journal)?;
    let mut sends = Queued::default();
    let email = [
        Outcome::Retryable {
            reason: Reason::RateLimited,
        },
        Outcome::Retryable {
            reason: Reason::ProviderError,
        },
        Outcome::Accepted {
            provider_message_id: "m-2".to_owned(),
        },
    ];
    let web_push = [
        Outcome::Retryable {
            reason: Reason::Timeout,
        },
        Outcome::Permanent {
            reason: Reason::TooLarge,
        },
    ];
    sends.answers.insert(PushChannel::Email, email.into());
    sends.answers.insert(PushChannel::WebPush, web_push.into());
    let mut random = Counter(0);
    for secs in [0, 14, 15, 74, 75, 10_000] {
        step_at(&journal, &writer, &mut sends, &mut random, secs)??;
    }
    let want = vec![
        record("email", 1, "failed", "rate_limited", "retryable", 0),
        record("web_push", 1, "failed", "timeout", "retryable", 0),
        record("email", 2, "failed", "provider_error", "retryable", 15),
        record("web_push", 2, "failed", "too_large", "permanent", 15),
        record("email", 3, "delivered", "", "", 75),
    ];
    assert_eq!(records(&journal)?, want);
    let notice = notice(&journal)?;
    let keys: Vec<(PushChannel, String)> = sends
        .sent
        .iter()
        .map(|(c, n, k)| (*c, format!("{n}:{k}")))
        .collect();
    let email_key = format!("{}:{}", notice.hex()?, key_of(&notice, PushChannel::Email)?);
    let push_key = format!(
        "{}:{}",
        notice.hex()?,
        key_of(&notice, PushChannel::WebPush)?
    );
    let expected = [
        (PushChannel::Email, email_key.clone()),
        (PushChannel::WebPush, push_key.clone()),
        (PushChannel::Email, email_key.clone()),
        (PushChannel::WebPush, push_key),
        (PushChannel::Email, email_key),
    ];
    assert_eq!(keys, expected, "one key per send, on every attempt");
    Ok(())
}

/// Spec §5.3, DEC-725 item 6.2, DEC-707: a send that fails on every attempt is tried on
/// schedule while an attempt fits its window, waits through the step just before the window's
/// end, and is journaled `abandoned` with `retry_window_ended`, as the attempt after its last, at
/// the first step at or after that end (`safety` 24 hours, `info` 6 hours from attempt 1), with no
/// send, and once. The delivered channel beside it is untouched.
#[test]
fn a_failing_send_is_abandoned_once_at_the_first_step_at_or_after_its_window_end() -> Checked {
    for (kind, window) in [("risk_limit", SAFETY_WINDOW), ("daily_brief", INFO_WINDOW)] {
        let journal = world()?;
        alerts(&journal, A1, 1, &[(1, kind)])?;
        let writer = writer(&journal)?;
        let mut sends = Queued::default();
        let accepted = Outcome::Accepted {
            provider_message_id: "m-1".to_owned(),
        };
        sends
            .answers
            .insert(PushChannel::WebPush, VecDeque::from([accepted]));
        let mut random = Counter(0);
        let mut want = Vec::new();
        let (mut secs, mut n) = (0_i64, 1_u32);
        while secs <= window {
            step_at(&journal, &writer, &mut sends, &mut random, secs)??;
            want.push(record("email", n, "failed", "timeout", "retryable", secs));
            if n == 1 {
                want.push(record("web_push", 1, "delivered", "", "", 0));
            }
            secs = secs.checked_add(gap(n)).ok_or("secs")?;
            n = n.checked_add(1).ok_or("n")?;
        }
        let ends = [
            window.saturating_sub(1),
            window,
            window.saturating_add(1),
            400_000,
        ];
        for secs in ends {
            step_at(&journal, &writer, &mut sends, &mut random, secs)??;
        }
        want.push(record(
            "email",
            n,
            "abandoned",
            "retry_window_ended",
            "",
            window,
        ));
        assert_eq!(records(&journal)?, want, "{kind}");
        let attempts = u32::try_from(sends.sent.len())?;
        assert_eq!(
            attempts, n,
            "{kind}: one send per made attempt, none for the abandon"
        );
    }
    Ok(())
}

/// DEC-707: a dispatcher that steps long after a retry fell due makes one attempt per send in
/// that step, not one for each instant it missed, and the schedule runs on from that attempt.
#[test]
fn a_step_makes_at_most_one_record_per_send_after_a_lapse() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let writer = writer(&journal)?;
    let mut sends = Queued::default();
    let accepted = Outcome::Accepted {
        provider_message_id: "m-1".to_owned(),
    };
    sends
        .answers
        .insert(PushChannel::WebPush, VecDeque::from([accepted]));
    let mut random = Counter(0);
    for secs in [0, 7_200, 7_200, 7_259, 7_260] {
        step_at(&journal, &writer, &mut sends, &mut random, secs)??;
    }
    let want = vec![
        record("email", 1, "failed", "timeout", "retryable", 0),
        record("web_push", 1, "delivered", "", "", 0),
        record("email", 2, "failed", "timeout", "retryable", 7_200),
        record("email", 3, "failed", "timeout", "retryable", 7_260),
    ];
    assert_eq!(records(&journal)?, want);
    Ok(())
}

/// What the oracle expects for one channel at one instant, computed from the committed records of
/// that channel alone and §5.3's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Finished,
    Attempt(u32),
    Abandon(u32),
    Wait(i64),
}

/// The oracle: `channel`'s committed records at `now`, and the class's `window`.
fn expect(records: &[Record], channel: &str, window: i64, now: i64) -> Result<Expect, String> {
    let mine: Vec<&Record> = records.iter().filter(|r| r.channel == channel).collect();
    let (Some(first), Some(last)) = (mine.first(), mine.last()) else {
        return Ok(Expect::Attempt(1));
    };
    let retryable = last.status == "failed" && last.verdict == "retryable";
    if !retryable {
        return Ok(Expect::Finished);
    }
    let following = last.attempt.checked_add(1).ok_or("attempt")?;
    let next_at = last.secs.checked_add(gap(last.attempt)).ok_or("next")?;
    let end = first.secs.checked_add(window).ok_or("end")?;
    Ok(match (next_at <= end, now >= next_at, now >= end) {
        (true, true, _) => Expect::Attempt(following),
        (true, false, _) => Expect::Wait(next_at),
        (false, _, true) => Expect::Abandon(following),
        (false, _, false) => Expect::Wait(end),
    })
}

/// The answers the property draws from, retryable ones more often, every one a pair rule 134
/// admits.
fn answer(which: usize) -> Outcome {
    let retry = |reason| Outcome::Retryable { reason };
    let stop = |reason| Outcome::Permanent { reason };
    match which {
        0 => Outcome::Accepted {
            provider_message_id: "m-1".to_owned(),
        },
        1 => stop(Reason::ProviderError),
        2 => stop(Reason::TooLarge),
        3 => stop(Reason::AddressRejected),
        4 | 5 => retry(Reason::RateLimited),
        6 | 7 => retry(Reason::ProviderError),
        8 => retry(Reason::TooLarge),
        _ => retry(Reason::Timeout),
    }
}

/// How the property moves the clock before a step: by some seconds, or to exactly the instant
/// the oracle says something is next due, so every boundary is stepped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tick {
    By(i64),
    Exact,
}

/// One scenario of the property: the notice's class, each channel's answers, and the ticks, each
/// with whether the dispatcher restarts (a new writer epoch, no state carried) before it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Scenario {
    info: bool,
    email: Vec<usize>,
    web_push: Vec<usize>,
    ticks: Vec<(Tick, bool)>,
}

fn scenarios() -> impl Strategy<Value = Scenario> {
    let tick = prop_oneof![
        (0_i64..=20).prop_map(Tick::By),
        (0_i64..=1_000).prop_map(Tick::By),
        (0_i64..=7_200).prop_map(Tick::By),
        (3_600_i64..=30_000).prop_map(Tick::By),
        Just(Tick::Exact),
        Just(Tick::Exact),
    ];
    (
        any::<bool>(),
        prop::collection::vec(0_usize..12, 0..10),
        prop::collection::vec(0_usize..12, 0..10),
        prop::collection::vec((tick, prop::bool::weighted(0.2)), 1..30),
    )
        .prop_map(|(info, email, web_push, ticks)| Scenario {
            info,
            email,
            web_push,
            ticks,
        })
}

/// The answer the oracle's own copy of a channel's queue says comes next, as `(status, reason,
/// verdict)` its record must carry.
fn recorded_as(queue: &mut VecDeque<usize>) -> Result<(String, String, String), String> {
    let outcome = queue.pop_front().map_or(answer(usize::MAX), answer);
    let key = |r: Reason| r.key().map(str::to_owned).map_err(|e| e.to_string());
    Ok(match outcome {
        Outcome::Accepted { .. } => ("delivered".to_owned(), String::new(), String::new()),
        Outcome::Retryable { reason } => {
            ("failed".to_owned(), key(reason)?, "retryable".to_owned())
        }
        Outcome::Permanent { reason } => {
            ("failed".to_owned(), key(reason)?, "permanent".to_owned())
        }
    })
}

/// Runs one scenario and checks every step against the oracle; `Err` names the first breach.
fn run_scenario(scenario: &Scenario) -> Result<(), String> {
    let fail = |e: Box<dyn Error>| e.to_string();
    let (kind, window) = if scenario.info {
        ("daily_brief", INFO_WINDOW)
    } else {
        ("risk_limit", SAFETY_WINDOW)
    };
    let journal = world().map_err(fail)?;
    alerts(&journal, A1, 1, &[(1, kind)]).map_err(fail)?;
    let mut sends = Queued::default();
    let mut oracle: BTreeMap<PushChannel, VecDeque<usize>> = BTreeMap::new();
    for (channel, answers) in [
        (PushChannel::Email, &scenario.email),
        (PushChannel::WebPush, &scenario.web_push),
    ] {
        sends
            .answers
            .insert(channel, answers.iter().map(|&a| answer(a)).collect());
        oracle.insert(channel, answers.iter().copied().collect());
    }
    let mut random = Counter(0);
    let mut writer = writer(&journal).map_err(|e| e.to_string())?;
    let mut now = 0_i64;
    let mut first = true;
    let ticks = std::iter::once((Tick::By(0), false)).chain(scenario.ticks.iter().copied());
    for (tick, restart) in ticks {
        let before = records(&journal).map_err(fail)?;
        let mut expected = BTreeMap::new();
        for channel in CHANNELS {
            let name = channel.key().map_err(|e| e.to_string())?;
            expected.insert(channel, expect(&before, name, window, now)?);
        }
        if let Tick::Exact = tick {
            let soonest = expected
                .values()
                .filter_map(|e| match e {
                    Expect::Wait(at) => Some(*at),
                    _ => None,
                })
                .min();
            now = soonest.unwrap_or(now).max(now);
            for channel in CHANNELS {
                let name = channel.key().map_err(|e| e.to_string())?;
                expected.insert(channel, expect(&before, name, window, now)?);
            }
        } else if let Tick::By(by) = tick {
            now = now.checked_add(by).ok_or("now")?;
            for channel in CHANNELS {
                let name = channel.key().map_err(|e| e.to_string())?;
                expected.insert(channel, expect(&before, name, window, now)?);
            }
        }
        if restart {
            writer = super::writer(&journal).map_err(|e| e.to_string())?;
        }
        if !first {
            check_due(&journal, &expected, now)?;
        }
        let sent_before = sends.sent.len();
        step_at(&journal, &writer, &mut sends, &mut random, now)
            .map_err(fail)?
            .map_err(|e| format!("the step at {now} failed: {e}"))?;
        let after = records(&journal).map_err(fail)?;
        let new: Vec<&Record> = after
            .get(before.len()..)
            .unwrap_or_default()
            .iter()
            .collect();
        let made = sends.sent.get(sent_before..).unwrap_or_default();
        let notice = notice(&journal).map_err(fail)?;
        let notice_hex = notice.hex().map_err(|e| e.to_string())?;
        for channel in CHANNELS {
            let name = channel.key().map_err(|e| e.to_string())?;
            let mine: Vec<&&Record> = new.iter().filter(|r| r.channel == name).collect();
            let sent: Vec<_> = made.iter().filter(|(c, _, _)| *c == channel).collect();
            let at = format!("{name} at {now}");
            let want_key = key_of(&notice, channel).map_err(fail)?;
            let foreign = sent
                .iter()
                .find(|(_, n, k)| *k != want_key || *n != notice_hex);
            if let Some((_, n, k)) = foreign {
                return Err(format!("{at}: a send under another key or notice: {n} {k}"));
            }
            match expected.get(&channel).copied().unwrap_or(Expect::Finished) {
                Expect::Attempt(n) => {
                    let queue = oracle.get_mut(&channel).ok_or("queue")?;
                    let (status, reason, verdict) = recorded_as(queue)?;
                    let want = record(name, n, &status, &reason, &verdict, now);
                    if mine.as_slice() != [&&want] || sent.len() != 1 {
                        return Err(format!(
                            "{at}: want one attempt {want:?}, got {mine:?} with {} sends",
                            sent.len()
                        ));
                    }
                }
                Expect::Abandon(n) => {
                    let want = record(name, n, "abandoned", "retry_window_ended", "", now);
                    if mine.as_slice() != [&&want] || !sent.is_empty() {
                        return Err(format!(
                            "{at}: want {want:?} and no send, got {mine:?} with {} sends",
                            sent.len()
                        ));
                    }
                }
                Expect::Wait(_) | Expect::Finished => {
                    if !mine.is_empty() || !sent.is_empty() {
                        return Err(format!(
                            "{at}: nothing is due, got {mine:?} with {} sends",
                            sent.len()
                        ));
                    }
                }
            }
        }
        first = false;
    }
    whole_run(&records(&journal).map_err(fail)?, window)
}

/// What `due` must answer at `now`, from the oracle's own reading: each unfinished channel of the
/// one notice in channel order, as an attempt under its key, an abandon, or a wait.
fn check_due(
    journal: &Shared,
    expected: &BTreeMap<PushChannel, Expect>,
    now: i64,
) -> Result<(), String> {
    let fail = |e: Box<dyn Error>| e.to_string();
    let ntf = StreamId::parse(NTF).ok_or(NTF)?;
    let rows = journal.rows(&ntf);
    let notice = notice(journal).map_err(fail)?;
    let founder = Recipient::parse("founder").map_err(|e| e.to_string())?;
    let mut want = Vec::new();
    for (&channel, expect) in expected {
        let key = |n| -> Result<Next, Box<dyn Error>> {
            let address = AddressHandle {
                recipient: Recipient::parse("founder")?,
                channel,
            };
            Ok(Next::Attempt {
                attempt: n,
                key: IdempotencyKey::of(&notice, &address)?,
            })
        };
        let next = match *expect {
            Expect::Finished => continue,
            Expect::Attempt(n) => key(n).map_err(fail)?,
            Expect::Abandon(n) => Next::Abandon { attempt: n },
            Expect::Wait(secs) => Next::Wait {
                until: at(secs).map_err(fail)?,
            },
        };
        want.push(Due {
            notice,
            recipient: founder.clone(),
            channel,
            next,
        });
    }
    let got = due(&rows, &AUDIENCE, at(now).map_err(fail)?).map_err(|e| format!("{e:?}"))?;
    if got != want {
        return Err(format!("due at {now}: got {got:?}, want {want:?}"));
    }
    Ok(())
}

/// The whole run's properties, read from the journal alone: per channel, attempts numbered 1 to k
/// with no gap, each made no earlier than §5.3 allows after the previous one; nothing after a
/// terminal record; at most one `abandoned`, last, with `retry_window_ended`, and never before
/// the window's end.
fn whole_run(records: &[Record], window: i64) -> Result<(), String> {
    for channel in ["email", "web_push"] {
        let mine: Vec<&Record> = records.iter().filter(|r| r.channel == channel).collect();
        let first = mine.first().map_or(0, |r| r.secs);
        let mut previous: Option<&Record> = None;
        for (i, r) in mine.iter().enumerate() {
            let n = u32::try_from(i)
                .map_err(|e| e.to_string())?
                .saturating_add(1);
            if r.attempt != n {
                return Err(format!(
                    "{channel}: attempt {} where {n} is next",
                    r.attempt
                ));
            }
            if let Some(p) = previous {
                if p.status != "failed" || p.verdict != "retryable" {
                    return Err(format!("{channel}: a record after terminal {p:?}"));
                }
                let due_at = p.secs.saturating_add(gap(p.attempt));
                if r.status != "abandoned" && r.secs < due_at {
                    return Err(format!(
                        "{channel}: attempt {n} at {} before {due_at}",
                        r.secs
                    ));
                }
            }
            let early = r.secs < first.saturating_add(window);
            if r.status == "abandoned" && (r.reason != "retry_window_ended" || early) {
                return Err(format!(
                    "{channel}: abandoned early or for another reason: {r:?}"
                ));
            }
            previous = Some(r);
        }
    }
    Ok(())
}

/// The retry property (spec §5.1 step 4, §5.3, §7; DEC-707, DEC-710 item 5, DEC-725 item 6), over
/// random answers on two channels, random clock ticks (some landing exactly on the instant the
/// oracle says is next), and restarts that take a new writer epoch and carry no state: before each
/// step `due` answers what the oracle derives from the committed records and §5.3's typed table,
/// and each step journals for each channel exactly the attempt, under that send's one key, or the
/// abandon the oracle expects, and nothing else. A proptest `TestRunner` in a plain function, as a
/// macro-generated pending test is refused by `cargo xtask markers`.
#[test]
fn retries_follow_the_schedule_through_random_answers_ticks_and_restarts() {
    let checked = TestRunner::default().run(&scenarios(), |scenario| {
        run_scenario(&scenario).map_err(TestCaseError::fail)
    });
    assert_eq!(checked, Ok(()));
}
