//! E8-10 slice D4 (spec §5.1 step 4, §5.3; journal spec §9.15 Lifecycle; DEC-706, DEC-707,
//! DEC-710 item 5, DEC-725 item 6) over notice streams written row by row here. Every expected
//! instant is typed from §5.3's table (15 s, 60 s, 5 min, then 15 min after the previous attempt;
//! `safety` 24 hours and `info` 6 hours from attempt 1), never computed by
//! `mandate_notify::next_attempt`.

use super::{Due, Next, due};
use crate::DispatchError;
use mandate_canon::Digest;
use mandate_journal::StoredEvent;
use mandate_notify::{AddressHandle, IdempotencyKey, NoticeId, PushChannel, Recipient};
use mandate_time::UtcNanos;
use std::error::Error;

type Checked = Result<(), Box<dyn Error>>;

const N1: &str = "0123456789abcdef0123456789abcdef";
const N2: &str = "fedcba9876543210fedcba9876543210";
const AUDIENCE: [(&str, PushChannel); 2] = [
    ("founder", PushChannel::Email),
    ("founder", PushChannel::WebPush),
];
const DAY: i64 = 86_400;
const SIX_HOURS: i64 = 21_600;

/// `secs` seconds after 2026-10-09T14:00:00Z, the instant every stream here starts from.
fn at(secs: i64) -> Result<UtcNanos, Box<dyn Error>> {
    let start = UtcNanos::parse("2026-10-09T14:00:00.000000000Z")?;
    let later = start
        .secs()
        .checked_add(secs)
        .ok_or("an instant in range")?;
    Ok(UtcNanos::from_parts(later, 0)?)
}

/// A notice stream as the journal would hold it, built one committed row at a time. A row's
/// `recorded_at` is `skew` seconds after its `event_time`, so a reading that timed an attempt by
/// the journal's clock would be caught.
#[derive(Default)]
struct Stream {
    rows: Vec<StoredEvent>,
    skew: i64,
}

impl Stream {
    fn row(&mut self, event_type: &str, version: u64, secs: i64, payload: &str) -> Checked {
        let seq = u64::try_from(self.rows.len())?.saturating_add(1);
        let event_time = at(secs)?.to_string();
        let recorded_at = at(secs.checked_add(self.skew).ok_or("skew")?)?.to_string();
        let event_id = format!("01J8Z3M4{seq:018}");
        let body = format!(
            r#"{{"actor":{{"build":"sha256:{z}","id":"dispatcher","kind":"system","version":"0.0.0"}},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"{event_id}","event_time":"{event_time}","event_type":"{event_type}","payload":{payload},"pii_refs":[],"prev_hash":"{z}","recorded_at":"{recorded_at}","schema_version":{version},"seq":{seq},"stream_id":"ntf:ws_1"}}"#,
            z = "0".repeat(64),
        )
        .into_bytes();
        self.rows.push(StoredEvent {
            stream_id: "ntf:ws_1".to_owned(),
            seq,
            event_id,
            event_type: event_type.to_owned(),
            schema_version: version,
            environment: "paper".to_owned(),
            recorded_at,
            prev_hash: Digest::of(b""),
            hash: Digest::of(&body),
            body,
        });
        Ok(())
    }

    /// A `NoticeIssued` of `notice`, of `kind` and `class`, to `recipients`, at `secs`.
    fn issued(&mut self, notice: &str, kind: &str, class: &str, recipients: &[&str]) -> Checked {
        let listed: Vec<String> = recipients.iter().map(|r| format!("\"{r}\"")).collect();
        let payload = format!(
            r#"{{"notice":"{notice}","kind":"{kind}","class":"{class}","cause":"01J8Z3M4000000000000000011","cause_stream":"acct:ws_1:A1","recipients":[{}]}}"#,
            listed.join(",")
        );
        self.row("NoticeIssued", 1, 0, &payload)
    }

    /// A version-2 `NoticeAttempted` of `send`, attempt `n`, at `secs`: `status`, with `reason`
    /// and `verdict` given as JSON (`null` or a quoted string).
    fn attempted(
        &mut self,
        send: Send<'_>,
        n: u32,
        secs: i64,
        status: &str,
        reason: &str,
    ) -> Checked {
        let (notice, recipient, channel) = send;
        let verdict = match status {
            "failed" if is_permanent(reason) => "\"permanent\"",
            "failed" => "\"retryable\"",
            _ => "null",
        };
        let reason = match reason {
            "" => "null".to_owned(),
            r => format!("\"{}\"", r.trim_start_matches('!')),
        };
        let message = if status == "delivered" {
            "\"m-1\""
        } else {
            "null"
        };
        let payload = format!(
            r#"{{"notice":"{notice}","recipient":"{recipient}","channel":"{channel}","attempt":{n},"status":"{status}","reason":{reason},"provider_message_id":{message},"coalesced_into":null,"verdict":{verdict}}}"#
        );
        self.row("NoticeAttempted", 2, secs, &payload)
    }

    /// Attempt `n` of `send` failed `retryable { timeout }` at `secs`.
    fn retryable(&mut self, send: Send<'_>, n: u32, secs: i64) -> Checked {
        self.attempted(send, n, secs, "failed", "timeout")
    }
}

/// `(notice, recipient, channel)` as a record names them.
type Send<'a> = (&'a str, &'a str, &'a str);

const EMAIL: Send<'static> = (N1, "founder", "email");
const WEB_PUSH: Send<'static> = (N1, "founder", "web_push");

/// A reason written `!reason` is the verdict `permanent`; a bare one is `retryable`.
fn is_permanent(reason: &str) -> bool {
    reason.starts_with('!')
}

/// A `safety` notice `N1` to the founder.
fn safety() -> Result<Stream, Box<dyn Error>> {
    let mut stream = Stream::default();
    stream.issued(N1, "risk_limit", "safety", &["founder"])?;
    Ok(stream)
}

/// The key of every attempt of `notice` to `recipient` on `channel` (DEC-710 item 5).
fn key(
    notice: &str,
    recipient: &str,
    channel: PushChannel,
) -> Result<IdempotencyKey, Box<dyn Error>> {
    let address = AddressHandle {
        recipient: Recipient::parse(recipient)?,
        channel,
    };
    Ok(IdempotencyKey::of(&NoticeId::parse(notice)?, &address)?)
}

/// What `due` answers for `N1`'s founder on `channel` at `secs`, `None` when that send is not
/// listed.
fn next(stream: &Stream, channel: PushChannel, secs: i64) -> Result<Option<Next>, Box<dyn Error>> {
    let listed = due(&stream.rows, &AUDIENCE, at(secs)?)?;
    let n1 = NoticeId::parse(N1)?;
    let founder = Recipient::parse("founder")?;
    let mut found = listed
        .into_iter()
        .filter(|d| d.notice == n1 && d.recipient == founder && d.channel == channel);
    let first = found.next().map(|d| d.next);
    assert!(found.next().is_none(), "one entry per send");
    Ok(first)
}

fn attempt(n: u32, channel: PushChannel) -> Result<Option<Next>, Box<dyn Error>> {
    Ok(Some(Next::Attempt {
        attempt: n,
        key: key(N1, "founder", channel)?,
    }))
}

fn wait(secs: i64) -> Result<Option<Next>, Box<dyn Error>> {
    Ok(Some(Next::Wait { until: at(secs)? }))
}

/// Spec §5.1 step 3 and §5.3 "at once": a notice with no attempt is due at once, as attempt 1 on
/// every configured channel of every recipient it lists, each under its own key.
#[test]
fn a_notice_with_no_attempt_is_due_at_once_as_attempt_1_on_each_channel() -> Checked {
    let stream = safety()?;
    let listed = due(&stream.rows, &AUDIENCE, at(0)?)?;
    let founder = Recipient::parse("founder")?;
    let want: Vec<Due> = [PushChannel::Email, PushChannel::WebPush]
        .into_iter()
        .map(|channel| {
            Ok(Due {
                notice: NoticeId::parse(N1)?,
                recipient: founder.clone(),
                channel,
                next: Next::Attempt {
                    attempt: 1,
                    key: key(N1, "founder", channel)?,
                },
            })
        })
        .collect::<Result<_, Box<dyn Error>>>()?;
    assert_eq!(listed, want);
    Ok(())
}

/// Spec §5.3 and DEC-725 item 6.1: after attempt n the next is due at attempt n's own instant plus
/// 15 s (n = 1), 60 s (n = 2), 300 s (n = 3), then 900 s (n ≥ 4). One instant early it waits for
/// exactly that instant; at it, attempt n + 1 is due under the same key. An attempt made late (attempt
/// 2 here, 5 s after it was due) moves the schedule with it.
#[test]
fn after_each_retryable_failure_the_next_attempt_waits_15_s_60_s_5_min_then_15_min() -> Checked {
    let mut stream = safety()?;
    let email = PushChannel::Email;
    let made: [(u32, i64, i64); 5] = [
        (1, 0, 15),
        (2, 20, 80),
        (3, 80, 380),
        (4, 380, 1_280),
        (5, 1_280, 2_180),
    ];
    for (n, made_at, then_due) in made {
        stream.retryable(EMAIL, n, made_at)?;
        let before = then_due.checked_sub(1).ok_or("an instant")?;
        assert_eq!(
            next(&stream, email, before)?,
            wait(then_due)?,
            "after attempt {n}"
        );
        let following = n.checked_add(1).ok_or("attempt")?;
        assert_eq!(
            next(&stream, email, then_due)?,
            attempt(following, email)?,
            "after {n}"
        );
    }
    Ok(())
}

/// Journal spec §9.15 Lifecycle and DEC-706 item 5: `delivered`, `suppressed_quiet_hours`,
/// `abandoned`, and `failed` with the verdict `permanent` are terminal, so that send is no longer
/// listed at any later instant; the other channel's `retryable` failure leaves it due.
#[test]
fn a_terminal_attempt_ends_its_send_and_leaves_the_other_channel_due() -> Checked {
    let ends = [
        ("delivered", ""),
        ("suppressed_quiet_hours", ""),
        ("abandoned", "retry_window_ended"),
        ("failed", "!address_rejected"),
        ("failed", "!provider_error"),
        ("failed", "!too_large"),
        ("failed", "!recipient_not_permitted"),
        ("failed", "!address_missing"),
    ];
    for (status, reason) in ends {
        let mut stream = safety()?;
        stream.attempted(EMAIL, 1, 0, status, reason)?;
        stream.retryable(WEB_PUSH, 1, 0)?;
        for secs in [0, 15, 3_600, DAY, 3_i64.saturating_mul(DAY)] {
            let case = format!("{status} {reason} at {secs}");
            assert_eq!(next(&stream, PushChannel::Email, secs)?, None, "{case}");
            assert!(
                next(&stream, PushChannel::WebPush, secs)?.is_some(),
                "{case}: web push is still due"
            );
        }
    }
    Ok(())
}

/// DEC-725 item 6.3 and DEC-706 item 4: the verdict decides, not the reason. `provider_error` and
/// `too_large` retry when `retryable` and stop when `permanent`; `timeout` and `rate_limited`
/// always retry.
#[test]
fn the_verdict_decides_whether_a_send_is_retried_not_its_reason() -> Checked {
    let cases = [
        ("timeout", true),
        ("rate_limited", true),
        ("provider_error", true),
        ("too_large", true),
        ("!provider_error", false),
        ("!too_large", false),
    ];
    for (reason, retried) in cases {
        let mut stream = safety()?;
        stream.attempted(EMAIL, 1, 0, "failed", reason)?;
        let want = if retried {
            attempt(2, PushChannel::Email)?
        } else {
            None
        };
        assert_eq!(next(&stream, PushChannel::Email, 15)?, want, "{reason}");
    }
    Ok(())
}

/// DEC-706 item 6: a stored version-1 `failed` attempt, even for `timeout`, reads as `permanent`,
/// so its send is finished; nothing is sent again on a guess.
#[test]
fn a_stored_version_1_failed_attempt_reads_as_permanent() -> Checked {
    let mut stream = safety()?;
    let payload = format!(
        r#"{{"notice":"{N1}","recipient":"founder","channel":"email","attempt":1,"status":"failed","reason":"timeout","provider_message_id":null,"coalesced_into":null}}"#
    );
    stream.row("NoticeAttempted", 1, 0, &payload)?;
    for secs in [15, 3_600, DAY] {
        assert_eq!(next(&stream, PushChannel::Email, secs)?, None, "at {secs}");
    }
    assert_eq!(
        next(&stream, PushChannel::WebPush, 15)?,
        attempt(1, PushChannel::WebPush)?
    );
    Ok(())
}

/// Spec §5.3 and DEC-707: a `safety` send whose next attempt would fall after 24 hours from attempt
/// 1 waits until the window's end and is abandoned, as the attempt after its last, at the first
/// instant at or after that end; never before.
#[test]
fn a_safety_send_is_abandoned_at_the_windows_end_and_never_before() -> Checked {
    let mut stream = safety()?;
    for (n, secs) in [(1, 0), (2, 100), (3, 200), (4, 85_501)] {
        stream.retryable(EMAIL, n, secs)?;
    }
    let email = PushChannel::Email;
    for secs in [85_501, 86_000, DAY.saturating_sub(1)] {
        assert_eq!(next(&stream, email, secs)?, wait(DAY)?, "at {secs}");
    }
    for secs in [DAY, 86_401, 90_000] {
        let abandon = Some(Next::Abandon { attempt: 5 });
        assert_eq!(next(&stream, email, secs)?, abandon, "at {secs}");
    }
    Ok(())
}

/// DEC-725 item 6.2: an attempt due exactly at the window's end is made, under the same key.
#[test]
fn an_attempt_due_exactly_at_the_windows_end_is_made() -> Checked {
    let mut stream = safety()?;
    for (n, secs) in [(1, 0), (2, 100), (3, 200), (4, 85_500)] {
        stream.retryable(EMAIL, n, secs)?;
    }
    let email = PushChannel::Email;
    assert_eq!(next(&stream, email, DAY.saturating_sub(1))?, wait(DAY)?);
    assert_eq!(next(&stream, email, DAY)?, attempt(5, email)?);
    Ok(())
}

/// DEC-707: an attempt due inside the window is made whenever the dispatcher next steps, even
/// after the window's end (it was down); only then, with no retry left inside the window, is the
/// send abandoned, at the next instant asked.
#[test]
fn an_attempt_due_inside_the_window_is_made_late_and_the_send_then_abandoned() -> Checked {
    let mut stream = safety()?;
    for (n, secs) in [(1, 0), (2, 100), (3, 200), (4, 85_000)] {
        stream.retryable(EMAIL, n, secs)?;
    }
    let email = PushChannel::Email;
    assert_eq!(next(&stream, email, 90_000)?, attempt(5, email)?);
    stream.retryable(EMAIL, 5, 90_000)?;
    let abandon = Some(Next::Abandon { attempt: 6 });
    assert_eq!(next(&stream, email, 90_000)?, abandon);
    Ok(())
}

/// Spec §5.3: an `info` send's window is 6 hours from attempt 1. Due exactly at its end is made;
/// due one second after is abandoned at the end, and not before.
#[test]
fn an_info_sends_window_is_6_hours() -> Checked {
    let email = PushChannel::Email;
    for (last, at_end) in [
        (20_700, attempt(5, email)?),
        (20_701, Some(Next::Abandon { attempt: 5 })),
    ] {
        let mut stream = Stream::default();
        stream.issued(N1, "daily_brief", "info", &["founder"])?;
        for (n, secs) in [(1, 0), (2, 100), (3, 200), (4, last)] {
            stream.retryable(EMAIL, n, secs)?;
        }
        let before = SIX_HOURS.saturating_sub(1);
        let early = next(&stream, email, before)?;
        assert!(
            matches!(early, Some(Next::Wait { .. })),
            "{last}: {early:?} before the end"
        );
        assert_eq!(next(&stream, email, SIX_HOURS)?, at_end, "{last}");
    }
    Ok(())
}

/// DEC-707 item 1: until slice D7 adds the `not_pending` stop, an `action` send has no window and
/// is never abandoned: it keeps §5.3's schedule however long its retries run, so no approval
/// notice is dropped early.
#[test]
fn an_action_send_is_never_abandoned_before_slice_d7() -> Checked {
    let mut stream = Stream::default();
    stream.issued(N1, "approval_requested", "action", &["founder"])?;
    for (n, secs) in [(1, 0), (2, 100), (3, 200), (4, 90_000)] {
        stream.retryable(EMAIL, n, secs)?;
    }
    let email = PushChannel::Email;
    assert_eq!(next(&stream, email, 90_899)?, wait(90_900)?);
    assert_eq!(next(&stream, email, 90_900)?, attempt(5, email)?);
    let ten_days = 10_i64.saturating_mul(DAY);
    assert_eq!(next(&stream, email, ten_days)?, attempt(5, email)?);
    Ok(())
}

/// DEC-725 item 6.1 and DEC-707: each attempt is timed by its own record's `event_time`, never
/// by the journal's `recorded_at`; a skew between them moves nothing.
#[test]
fn each_attempt_is_timed_by_its_records_event_time() -> Checked {
    for skew in [-3_600, 0, 7, 3_600] {
        let mut stream = Stream {
            skew,
            ..Stream::default()
        };
        stream.issued(N1, "risk_limit", "safety", &["founder"])?;
        stream.retryable(EMAIL, 1, 1_000)?;
        let email = PushChannel::Email;
        assert_eq!(next(&stream, email, 1_014)?, wait(1_015)?, "skew {skew}");
        assert_eq!(
            next(&stream, email, 1_015)?,
            attempt(2, email)?,
            "skew {skew}"
        );
    }
    Ok(())
}

/// Spec §5.1's key and §7 "other channels unaffected": each `(notice, recipient, channel)` keeps
/// its own schedule and its own key; a channel the audience does not configure and a recipient the
/// notice does not list have no send; a second notice to the same address has another key.
#[test]
fn each_send_keeps_its_own_schedule_and_key() -> Checked {
    let mut stream = Stream::default();
    stream.issued(N1, "risk_limit", "safety", &["u_1", "u_2"])?;
    stream.issued(N2, "kill_switch", "safety", &["u_1"])?;
    stream.retryable((N1, "u_1", "email"), 1, 0)?;
    stream.retryable((N1, "u_1", "email"), 2, 15)?;
    stream.attempted((N1, "u_1", "web_push"), 1, 0, "delivered", "")?;
    stream.retryable((N1, "u_2", "email"), 1, 30)?;
    let audience = [
        ("u_1", PushChannel::Email),
        ("u_1", PushChannel::WebPush),
        ("u_2", PushChannel::Email),
        ("u_3", PushChannel::Email),
    ];
    let listed = due(&stream.rows, &audience, at(45)?)?;
    let got: Vec<(String, Recipient, PushChannel, Next)> = listed
        .into_iter()
        .map(|d| Ok((d.notice.hex()?, d.recipient, d.channel, d.next)))
        .collect::<Result<_, DispatchError>>()?;
    let (u_1, u_2) = (Recipient::parse("u_1")?, Recipient::parse("u_2")?);
    let first = |n: &str, r: &str, channel| -> Result<Next, Box<dyn Error>> {
        Ok(Next::Attempt {
            attempt: 1,
            key: key(n, r, channel)?,
        })
    };
    let want = vec![
        (
            N1.to_owned(),
            u_1.clone(),
            PushChannel::Email,
            Next::Wait { until: at(75)? },
        ),
        (
            N1.to_owned(),
            u_2,
            PushChannel::Email,
            Next::Attempt {
                attempt: 2,
                key: key(N1, "u_2", PushChannel::Email)?,
            },
        ),
        (
            N2.to_owned(),
            u_1.clone(),
            PushChannel::Email,
            first(N2, "u_1", PushChannel::Email)?,
        ),
        (
            N2.to_owned(),
            u_1,
            PushChannel::WebPush,
            first(N2, "u_1", PushChannel::WebPush)?,
        ),
    ];
    assert_eq!(got, want);
    assert_ne!(
        key(N1, "u_1", PushChannel::Email)?,
        key(N2, "u_1", PushChannel::Email)?
    );
    Ok(())
}

/// Spec §3.1: an `info` kind other than the brief goes only to the pull channels, so it has no
/// push send to be due.
#[test]
fn a_kind_that_pushes_nothing_has_no_send() -> Checked {
    let mut stream = Stream::default();
    stream.issued(N1, "delegation_ended", "info", &["founder"])?;
    assert_eq!(due(&stream.rows, &AUDIENCE, at(0)?)?, Vec::new());
    Ok(())
}
