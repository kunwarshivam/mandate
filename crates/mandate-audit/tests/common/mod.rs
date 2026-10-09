//! A journal of several workspaces, built through the append protocol, the record of what was
//! appended that the tests' oracles read instead of the code under test, and the tenant contexts
//! the real `authorize` yields for the reads.
#![allow(
    dead_code,
    reason = "each test crate that includes this module uses a different subset"
)]

use std::collections::BTreeMap;

use mandate_audit::PageLimit;
use mandate_identity::{
    Authorized, MembershipState, OrgId, Permission, Principal, PrincipalId, Role, Scope,
    SessionKind, SessionRef, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_time::UtcNanos;

pub const T: &str = "2026-09-21T14:00:00.000000000Z";

/// The two workspaces the tests read as.
pub const WS_A: WorkspaceId = WorkspaceId(0x0192_0C3A_7F10_4B2E_9D01_0000_0000_00A1);
pub const WS_B: WorkspaceId = WorkspaceId(0x0192_0C3A_7F10_4B2E_9D01_0000_0000_00B2);

/// A workspace's stream segment: its ULID text (journal spec §2).
pub fn text(workspace: WorkspaceId) -> String {
    workspace.to_ulid_text().unwrap()
}

/// The workspace segments every fixture holds: [`WS_A`]'s, [`WS_A`]'s with one more character, a
/// segment no tenant names but that has `WS_A`'s as a prefix, so a scope check that compares
/// prefixes rather than whole segments reads across tenants, and [`WS_B`]'s.
pub fn workspaces() -> [String; 3] {
    [text(WS_A), format!("{}0", text(WS_A)), text(WS_B)]
}

/// An Auditor's context for `workspace`, from the real `authorize` for `ReadRecords` (the matrix
/// grants it WA, AU and SA): the only way a read is handed a workspace (identity spec ID-8).
pub fn tenant(workspace: WorkspaceId) -> TenantContext {
    let user = PrincipalId(0x0192_0C3A_7F10_4B2E_9D01_0000_0000_0C01);
    let scope = Scope::Workspace {
        org: OrgId(0x0192_0C3A_7F10_4B2E_9D01_0000_0000_0001),
        workspace,
    };
    let now = UtcNanos::parse(T).unwrap();
    let auditor = membership(
        user,
        scope,
        MembershipState::Active,
        &[(Role::Auditor, now)],
    );
    let session = session(SessionRef(0x5E), SessionKind::Full, vec![auditor.clone()]);
    let principal = Principal::User { id: user };
    let lookup = StaticLookup(vec![auditor]);
    match authorize(
        &lookup,
        &principal,
        &session,
        scope,
        Permission::ReadRecords,
        now,
    ) {
        Ok(Authorized::Workspace { tenant, .. }) => tenant,
        other => panic!("an Auditor reads its workspace's records: {other:?}"),
    }
}

/// The account streams each workspace holds, beside its control stream.
pub const ACCOUNTS: [&str; 2] = ["ACCT1", "ACCT2"];

pub fn acct(workspace: &str, account: &str) -> String {
    format!("acct:{workspace}:{account}")
}

pub fn ctl(workspace: &str) -> String {
    format!("ctl:{workspace}")
}

pub fn limit(n: u64) -> PageLimit {
    PageLimit::new(Some(n)).unwrap()
}

pub fn event_id(n: u64) -> String {
    format!("01J8Z3M4{n:018}")
}

pub fn actor() -> String {
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
    /// Every workspace segment of [`workspaces`] with its control stream and its [`ACCOUNTS`] streams, each
    /// opened, and `marks` `MarkUpdated` events on every account stream.
    pub fn new(marks: u64) -> Self {
        let mut fixture = Self {
            journal: MemoryJournal::new(),
            appended: BTreeMap::new(),
            next_id: 0,
        };
        for workspace in &workspaces() {
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

    pub fn open(&mut self, stream: &str, payload: &str) {
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
        self.append_draft(stream, id.clone(), &draft(&id, stream, event_type, payload));
    }

    /// Appends one draft the caller wrote whole, whose `event_id` is `id`, to an opened `stream`.
    pub fn append_draft(&mut self, stream: &str, id: String, draft: &[u8]) {
        let record = self.appended.get_mut(stream).unwrap();
        let head = record.event_ids.len() as u64;
        let outcome = self.journal.append(
            &StreamId::parse(stream).unwrap(),
            head,
            record.epoch,
            UtcNanos::parse(T).unwrap(),
            &[draft],
        );
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
        record.event_ids.push(id);
    }

    /// The streams of `workspace` that the fixture opened, sorted by `stream_id` bytes.
    pub fn streams_of(&self, workspace: &str) -> Vec<String> {
        self.appended
            .keys()
            .filter(|s| s.split(':').nth(1) == Some(workspace))
            .cloned()
            .collect()
    }
}
