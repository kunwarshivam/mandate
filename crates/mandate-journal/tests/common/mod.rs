//! Draft builders shared by the journal tests.
#![allow(
    dead_code,
    reason = "each test crate that includes this module uses a different subset"
)]

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_time::UtcNanos;

pub const STREAM: &str = "acct:ws_1:ACCT1";
pub const T: &str = "2026-09-21T14:00:00.000000000Z";

pub fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

pub fn opened_draft(env: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"{env}","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca",
        "account_ref":"ACCT1"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(0),
        "3".repeat(64)
    )
    .into_bytes()
}

pub fn mark_draft(n: u64, price: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{STREAM}",
        "event_type":"MarkUpdated","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"instrument_id":"inst","price":"{price}","source":"quote",
        "feed":"iex"}},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(n),
        "3".repeat(64)
    )
    .into_bytes()
}

/// `draft` with `path` (dotted) set to `value` (JSON text), or removed when `value` is `None`.
pub fn edit(draft: &[u8], path: &str, value: Option<&str>) -> Vec<u8> {
    fn go(v: &mut Value, keys: &[&str], value: Option<&Value>) {
        let Value::Object(members) = v else {
            panic!("not an object")
        };
        match keys {
            [last] => match value {
                Some(new) => {
                    members.insert(Key::new(last).unwrap(), new.clone());
                }
                None => {
                    members.remove(*last).expect("field to remove");
                }
            },
            [first, rest @ ..] => go(members.get_mut(*first).unwrap(), rest, value),
            [] => unreachable!(),
        }
    }
    let mut v = parse(draft).unwrap();
    let new = value.map(|text| parse(text.as_bytes()).unwrap());
    go(&mut v, &path.split('.').collect::<Vec<_>>(), new.as_ref());
    to_canonical(&v)
}

pub fn stream() -> StreamId {
    StreamId::parse(STREAM).unwrap()
}

pub fn now() -> UtcNanos {
    UtcNanos::parse(T).unwrap()
}

/// A journal whose stream holds StreamOpened and `marks` MarkUpdated events, with writer epoch 1.
pub fn journal_with(marks: u64) -> MemoryJournal {
    let mut j = MemoryJournal::new();
    let epoch = j.take_ownership(&stream());
    assert!(matches!(
        j.append(&stream(), 0, epoch, now(), &[&opened_draft("paper")]),
        AppendOutcome::Committed(_)
    ));
    for n in 1..=marks {
        let outcome = j.append(&stream(), n, epoch, now(), &[&mark_draft(n, "1")]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }
    j
}
