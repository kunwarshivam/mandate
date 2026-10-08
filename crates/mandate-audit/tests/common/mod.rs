//! A journal of several workspaces, built through the append protocol, and the record of what was
//! appended that the tests' oracles read instead of the code under test.
#![allow(
    dead_code,
    reason = "each test crate that includes this module uses a different subset"
)]

use std::collections::BTreeMap;

use mandate_audit::{PageLimit, WorkspaceId};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_time::UtcNanos;

pub const T: &str = "2026-09-21T14:00:00.000000000Z";

/// The workspaces every fixture holds. `ws_a` is a prefix of `ws_ab`, so a scope check that
/// compares prefixes rather than whole segments reads across tenants.
pub const WORKSPACES: [&str; 3] = ["ws_a", "ws_ab", "ws_b"];

/// The account streams each workspace holds, beside its control stream.
pub const ACCOUNTS: [&str; 2] = ["ACCT1", "ACCT2"];

pub fn acct(workspace: &str, account: &str) -> String {
    format!("acct:{workspace}:{account}")
}

pub fn ctl(workspace: &str) -> String {
    format!("ctl:{workspace}")
}

pub fn ws(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).unwrap()
}

pub fn limit(n: u64) -> PageLimit {
    PageLimit::new(Some(n)).unwrap()
}

pub fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

fn actor() -> String {
    format!(
        r#"{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:{}"}}"#,
        "3".repeat(64)
    )
}

fn draft(event_id: &str, stream: &str, event_type: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,"actor":{},"config_refs":{{}},
        "payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        actor()
    )
    .into_bytes()
}

/// What a fixture appended to one stream, in append order: the oracle the tests compare against.
#[derive(Debug, Default, Clone)]
pub struct Appended {
    pub event_ids: Vec<String>,
    epoch: u64,
}

pub struct Fixture {
    pub journal: MemoryJournal,
    pub appended: BTreeMap<String, Appended>,
    next_id: u64,
}

impl Fixture {
    /// Every workspace of [`WORKSPACES`] with its control stream and its [`ACCOUNTS`] streams, each
    /// opened, and `marks` `MarkUpdated` events on every account stream.
    pub fn new(marks: u64) -> Self {
        let mut fixture = Self {
            journal: MemoryJournal::new(),
            appended: BTreeMap::new(),
            next_id: 0,
        };
        for workspace in WORKSPACES {
            fixture.open(
                &ctl(workspace),
                &format!(r#"{{"stream_type":"control","workspace_id":"{workspace}"}}"#),
            );
            for account in ACCOUNTS {
                let stream = acct(workspace, account);
                fixture.open(
                    &stream,
                    &format!(
                        r#"{{"stream_type":"account","workspace_id":"{workspace}","broker":"alpaca","account_ref":"{account}"}}"#
                    ),
                );
                fixture.marks(&stream, marks);
            }
        }
        fixture
    }

    fn open(&mut self, stream: &str, payload: &str) {
        let id = StreamId::parse(stream).unwrap();
        let epoch = self.journal.take_ownership(&id);
        self.appended.insert(
            stream.to_owned(),
            Appended {
                event_ids: Vec::new(),
                epoch,
            },
        );
        self.append(stream, "StreamOpened", payload);
    }

    /// Appends `count` `MarkUpdated` events to the account stream `stream`.
    pub fn marks(&mut self, stream: &str, count: u64) {
        for _ in 0..count {
            self.append(
                stream,
                "MarkUpdated",
                &format!(
                    r#"{{"instrument_id":"inst","price":"1","source":"quote","feed":"iex","risk_clock":"{T}"}}"#
                ),
            );
        }
    }

    fn append(&mut self, stream: &str, event_type: &str, payload: &str) {
        self.next_id += 1;
        let id = event_id(self.next_id);
        let record = self.appended.get_mut(stream).unwrap();
        let head = record.event_ids.len() as u64;
        let outcome = self.journal.append(
            &StreamId::parse(stream).unwrap(),
            head,
            record.epoch,
            UtcNanos::parse(T).unwrap(),
            &[&draft(&id, stream, event_type, payload)],
        );
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
        record.event_ids.push(id);
    }
}
