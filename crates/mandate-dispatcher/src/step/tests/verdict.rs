//! DEC-706 (journal spec v0.40 §9.15, rule 134): the step journals every `NoticeAttempted` at
//! schema version 2, its `verdict` the variant of the adapter's `Outcome` and never a reading of
//! its reason, and a stored version-1 `failed` attempt ends its channel. The oracles read the
//! committed rows, never the step's state.

use super::{
    A1, AUDIENCE, Checked, NTF, Shared, T, alerts, draft, payloads, provider, run, run_over, text,
    world, writer,
};
use crate::DispatchError;
use crate::step::{Journal, NoticeWriter};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_notify::{Outcome, PushChannel, Reason};
use mandate_time::UtcNanos;
use std::error::Error;

/// One committed `NoticeAttempted`: `(schema_version, channel, status, reason, verdict)`, with
/// `reason` empty when `null` and `verdict` as stored, `None` when the member is absent.
type Recorded = (u64, String, String, String, Option<Value>);

/// Each committed `NoticeAttempted`, by channel.
fn attempts(journal: &Shared) -> Result<Vec<Recorded>, Box<dyn Error>> {
    let ntf = StreamId::parse(NTF).ok_or(NTF)?;
    let mut got = Vec::new();
    for row in journal.committed(&ntf) {
        if row.event_type != "NoticeAttempted" {
            continue;
        }
        let payload = parse(&row.body)?.get("payload").cloned().ok_or("payload")?;
        let fields = ["channel", "status", "reason"].map(|m| text(&payload, m));
        let [channel, status, reason] = fields;
        let verdict = payload.get("verdict").cloned();
        got.push((row.schema_version, channel, status, reason, verdict));
    }
    got.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(got)
}

fn verdict(v: &str) -> Option<Value> {
    Some(Value::Str(v.to_owned()))
}

/// The adapter's `retryable { reason }`.
fn retry(reason: Reason) -> Outcome {
    Outcome::Retryable { reason }
}

/// The adapter's `permanent { reason }`.
fn stop(reason: Reason) -> Outcome {
    Outcome::Permanent { reason }
}

/// Notifications spec §5.2 and DEC-706 item 4: `permanent { reason }` is `permanent`, and
/// `retryable { reason }`, a timeout among them, is `retryable`. `provider_error` and `too_large`
/// arrive both ways (DEC-728 item 1, DEC-729 items 4, 5, 7 and 8) and each record follows the
/// variant; the delivered attempt beside it carries `null`. Every record is version 2.
#[test]
#[ignore = "pending E8-10"]
fn every_attempt_is_journaled_at_version_2_with_the_verdict_of_its_outcome_variant() -> Checked {
    let cases = [
        (retry(Reason::Timeout), "timeout", "retryable"),
        (retry(Reason::RateLimited), "rate_limited", "retryable"),
        (retry(Reason::ProviderError), "provider_error", "retryable"),
        (stop(Reason::ProviderError), "provider_error", "permanent"),
        (retry(Reason::TooLarge), "too_large", "retryable"),
        (stop(Reason::TooLarge), "too_large", "permanent"),
        (
            stop(Reason::AddressRejected),
            "address_rejected",
            "permanent",
        ),
        (stop(Reason::AuthFailed), "auth_failed", "permanent"),
        (
            stop(Reason::RecipientNotPermitted),
            "recipient_not_permitted",
            "permanent",
        ),
        (stop(Reason::AddressMissing), "address_missing", "permanent"),
    ];
    for (answer, reason, want) in cases {
        let journal = world()?;
        alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
        let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
        sends.script = vec![(PushChannel::Email, answer.clone())];
        run(&journal, &writer, &[A1], &mut sends)??;
        let failed = ("email", "failed", reason, verdict(want));
        let delivered = ("web_push", "delivered", "", Some(Value::Null));
        let expected: Vec<Recorded> = [failed, delivered]
            .into_iter()
            .map(|(c, s, r, v)| (2, c.to_owned(), s.to_owned(), r.to_owned(), v))
            .collect();
        assert_eq!(attempts(&journal)?, expected, "{answer:?}");
    }
    Ok(())
}

/// Rule 134 holds the record to the verdict §5.2 to §5.4 fix for its reason, and the writer never
/// mends an adapter's answer from its reason: an outcome whose variant contradicts that verdict is
/// refused by the journal, and the refused append stops the step before its next send (DEC-704
/// item 3, DEC-706's rationale), with nothing recorded for it.
#[test]
#[ignore = "pending E8-10"]
fn an_outcome_whose_variant_contradicts_its_reasons_fixed_verdict_stops_the_step() -> Checked {
    let contradicting = [
        retry(Reason::AddressRejected),
        retry(Reason::AuthFailed),
        retry(Reason::RecipientNotPermitted),
        retry(Reason::AddressMissing),
        stop(Reason::Timeout),
        stop(Reason::RateLimited),
    ];
    for answer in contradicting {
        let journal = world()?;
        alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
        let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
        sends.script = vec![
            (PushChannel::Email, answer.clone()),
            (PushChannel::WebPush, answer.clone()),
        ];
        let stepped = run(&journal, &writer, &[A1], &mut sends)?;
        let refused = Err(DispatchError::NotCommitted { outcome: "Invalid" });
        assert_eq!(stepped, refused, "{answer:?}");
        assert_eq!(sends.seen.len(), 1, "{answer:?}: no send after the refusal");
        assert!(
            payloads(&journal, "NoticeAttempted").is_empty(),
            "{answer:?}"
        );
    }
    Ok(())
}

/// The shared journal, with one more row at the end of the notice stream that `append` would no
/// longer admit: a stored record written before v0.40.
struct Holding {
    journal: Shared,
    stored: StoredEvent,
}

impl Journal for Holding {
    fn committed(&self, stream: &StreamId) -> Vec<StoredEvent> {
        let mut rows = self.journal.committed(stream);
        if stream.as_str() == NTF {
            rows.push(self.stored.clone());
        }
        rows
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

/// A version-1 `failed` attempt of `notice` on email, stored after `last` as the journal stores
/// a row: the draft with `seq`, `prev_hash`, and `recorded_at`, hashed.
fn stored_version_1(notice: &str, last: &StoredEvent) -> Result<StoredEvent, Box<dyn Error>> {
    let payload = format!(
        r#"{{"notice":"{notice}","recipient":"founder","channel":"email","attempt":1,
        "status":"failed","reason":"timeout","provider_message_id":null,"coalesced_into":null}}"#
    );
    let event_id = "01J8Z3M4000000000000000V1A";
    let drafted = draft(NTF, event_id, "NoticeAttempted", "null", &payload);
    let Value::Object(mut body) = parse(&drafted)? else {
        return Err("a draft is an object".into());
    };
    let seq = last.seq.saturating_add(1);
    for (name, value) in [
        ("seq", seq.to_string()),
        ("prev_hash", format!("\"{}\"", last.hash.to_hex())),
        ("recorded_at", format!("\"{T}\"")),
    ] {
        body.insert(Key::new(name)?, parse(value.as_bytes())?);
    }
    let body = to_canonical(&Value::Object(body));
    Ok(StoredEvent {
        stream_id: NTF.to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: "NoticeAttempted".to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash: last.hash,
        hash: Digest::of(&body),
        body,
    })
}

/// DEC-706 item 6 and journal spec §9.15's lifecycle: a stored version-1 `failed` attempt reads
/// as `permanent`, so its channel is finished for that notice and nothing is sent again on a
/// guess, even for `timeout`, which version 2 records as `retryable`. Without that row the same
/// step sends on that channel, so the row is what holds it.
#[test]
fn a_stored_version_1_failed_attempt_ends_its_channel() -> Checked {
    let journal = world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let (writer, mut sends) = (writer(&journal)?, provider(&journal)?);
    let web_push = [("founder", PushChannel::WebPush)];
    run_over(&mut journal.clone(), &writer, &[A1], &web_push, &mut sends)??;
    let notice = payloads(&journal, "NoticeIssued")
        .first()
        .map(|p| text(p, "notice"))
        .ok_or("one notice")?;
    let ntf = StreamId::parse(NTF).ok_or(NTF)?;
    let last = journal
        .committed(&ntf)
        .pop()
        .ok_or("the notice stream's head")?;
    let mut holding = Holding {
        journal: journal.clone(),
        stored: stored_version_1(&notice, &last)?,
    };
    let sent = sends.fx.sent()?.len();
    run_over(&mut holding, &writer, &[A1], &AUDIENCE, &mut sends)??;
    assert_eq!(
        sends.fx.sent()?.len(),
        sent,
        "the email channel is finished"
    );
    run_over(&mut journal.clone(), &writer, &[A1], &AUDIENCE, &mut sends)??;
    let channels: Vec<PushChannel> = sends.fx.sent()?.iter().map(|s| s.address.channel).collect();
    assert_eq!(
        channels.get(sent..),
        Some(&[PushChannel::Email][..]),
        "without it, email is due"
    );
    Ok(())
}
