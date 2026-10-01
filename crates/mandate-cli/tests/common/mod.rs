//! Fixtures for the owner-control commands (M7 tests PR 4 of 4): a journal whose agent stream holds
//! approval events as the runtime writes them (DEC-257 items 5 and 9), fixed ids, and readers that
//! decode what the CLI committed from the stored bytes.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::BTreeMap;
use std::num::NonZeroU8;

use mandate_approval::{
    AskablePurpose, AssetClass, BoundAction, ReferenceMark, RequestContent, RiskClock,
    content_object,
};
use mandate_canon::{Digest, Int, Key, Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal, Ids, Now, Owner};
use mandate_journal::{AppendOutcome, Environment, Head, StoredEvent, StreamId};
use mandate_num::{Price, Qty, Signed};
use mandate_time::UtcNanos;

pub const WORKSPACE: &str = "ws1";
pub const AGENT: &str = "agent-a";
pub const OTHER_AGENT: &str = "agent-b";
pub const OWNER: &str = "user-owner";
pub const CONTROL: &str = "ctl:ws1";
/// The risk-clock second the fixture asks at, and the instant it is.
pub const ASKED_AT: i64 = 1_790_000_000;
pub const TIMEOUT_S: i64 = 300;

pub fn owner() -> Owner {
    Owner {
        workspace: WORKSPACE.to_owned(),
        user: OWNER.to_owned(),
        environment: Environment::Paper,
    }
}

pub fn at(secs: i64) -> Now {
    Now {
        at: UtcNanos::from_parts(secs, 0).unwrap(),
        secs,
    }
}

pub fn stream(text: &str) -> StreamId {
    StreamId::parse(text).unwrap()
}

pub fn agent_stream(agent: &str) -> String {
    format!("agent:{WORKSPACE}:{agent}")
}

/// Fixed ids: ULID-shaped event ids starting `2`, and numbered assertion ids. The counters are how
/// many of each a command minted.
#[derive(Debug, Default)]
pub struct FixedIds {
    pub events: u64,
    pub assertions: u64,
}

impl Ids for FixedIds {
    fn event_id(&mut self) -> String {
        self.events += 1;
        format!("2{:025}", self.events)
    }

    fn assertion_id(&mut self) -> String {
        self.assertions += 1;
        format!("cli-assertion-{}", self.assertions)
    }
}

/// An id the runtime would have written on the agent stream: ULID-shaped, starting `0`.
pub fn runtime_id(n: u64) -> String {
    format!("0{n:025}")
}

fn key(k: &str) -> Key {
    Key::new(k).unwrap()
}

pub fn object(pairs: &[(&str, Value)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| (key(k), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

pub fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

pub fn int(n: i64) -> Value {
    Value::Int(Int::new(u64::try_from(n).unwrap()).unwrap())
}

/// One draft of the runtime's on `stream`.
fn draft(
    stream: &str,
    event_id: &str,
    event_type: &str,
    causation: Option<&str>,
    payload: Value,
) -> Vec<u8> {
    let envelope = object(&[
        ("envelope_version", int(1)),
        ("environment", text("paper")),
        ("event_id", text(event_id)),
        ("stream_id", text(stream)),
        ("event_type", text(event_type)),
        ("schema_version", int(1)),
        ("event_time", text("2026-09-21T14:00:00.000000000Z")),
        ("clock_source", text("local")),
        ("causation_id", causation.map_or(Value::Null, text)),
        ("correlation_id", Value::Null),
        (
            "actor",
            object(&[
                ("kind", text("agent")),
                ("id", text("runtime")),
                ("version", text("0.1.0")),
                ("build", text(&format!("sha256:{}", "3".repeat(64)))),
            ]),
        ),
        (
            "config_refs",
            object(&[(
                "mandate_version",
                text(&format!("sha256:{}", "4".repeat(64))),
            )]),
        ),
        ("payload", payload),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
    ]);
    to_canonical(&envelope)
}

/// The content of the fixture's request for `qty` shares of `AAPL` at 155.
pub fn content(qty: &str, deadline: i64) -> RequestContent {
    RequestContent {
        bound: BoundAction {
            instrument: "AAPL".to_owned(),
            asset_class: AssetClass::UsEquity,
            qty: Qty::parse(qty).unwrap(),
            limit: Price::parse("155").unwrap(),
            purpose: AskablePurpose::Open,
            mandate_version: "v1".to_owned(),
            decided_by: "rule:big-order".to_owned(),
            combined_score: Signed::parse("0.5").unwrap(),
            reference_mark: Some(ReferenceMark {
                price: Price::parse("155").unwrap(),
                seq: 2,
            }),
            approvers_required: NonZeroU8::MIN,
            independent_required: false,
        },
        evidence: Vec::new(),
        risk_impact: Vec::new(),
        deadline: RiskClock(deadline),
    }
}

/// `sha256:` and the hex digest of `content`'s canonical bytes: the oracle's own hash.
pub fn hash_of(content: &Value) -> String {
    format!("sha256:{}", Digest::of(&to_canonical(content)).to_hex())
}

/// The code `approve` must accept: the first 8 hex digits of the content hash, computed here.
pub fn code_of(content: &Value) -> String {
    Digest::of(&to_canonical(content)).to_hex()[..8].to_owned()
}

/// An in-memory journal that runs journal spec §5.1's idempotency, fencing and head check in that
/// order, but validates no payload: `mandate-journal` registers no payload schema for an agent- or
/// control-stream event yet, so its own journal refuses every event here as `UnknownSchema`
/// (DEC-257 item 17). What the CLI commits is read back from the stored bytes.
///
/// Three faults are injected on the next appends, each to drive one of the CLI's retry arms:
/// another writer taking the stream (`Fenced`), another write landing between the CLI's read of the
/// head and its append (`HeadMismatch`), and an append that commits but whose answer is lost
/// (`Ambiguous`), after which a retry of the same draft is `AlreadyCommitted`.
#[derive(Debug, Default)]
pub struct Journal {
    streams: BTreeMap<String, (u64, Vec<StoredEvent>)>,
    /// Appends to refuse as `Fenced`, taking a new epoch as another writer would.
    pub fence_next: u32,
    /// Appends before which another event lands at the current epoch, so the append is behind.
    pub behind_next: u32,
    /// Appends to commit and then answer `Ambiguous`.
    pub ambiguous_next: u32,
    /// Every append to the control stream the journal was asked for, with the event ids it carried
    /// (the fixture's own agent-stream appends are not the CLI's).
    pub attempts: Vec<Vec<String>>,
    foreign: u64,
}

impl Journal {
    fn stored(&self, event_id: &str) -> Option<&StoredEvent> {
        self.streams
            .values()
            .flat_map(|(_, rows)| rows)
            .find(|r| r.event_id == event_id)
    }

    fn store(
        &mut self,
        stream: &StreamId,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<Vec<StoredEvent>, ControlError> {
        let head = self.head(stream)?;
        let mut stored = Vec::new();
        let mut prev = head.hash;
        for (i, bytes) in drafts.iter().enumerate() {
            let draft = parse(bytes).map_err(|e| ControlError::Journal(format!("{e:?}")))?;
            let text = |k: &str| {
                draft
                    .get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let row = StoredEvent {
                stream_id: stream.as_str().to_owned(),
                seq: head.seq + 1 + u64::try_from(i).unwrap(),
                event_id: text("event_id"),
                event_type: text("event_type"),
                schema_version: draft
                    .get("schema_version")
                    .and_then(Value::as_int)
                    .unwrap_or(0),
                environment: text("environment"),
                recorded_at: recorded_at.to_string(),
                prev_hash: prev,
                hash: Digest::of(bytes),
                body: bytes.to_vec(),
            };
            prev = row.hash;
            stored.push(row);
        }
        self.streams
            .entry(stream.as_str().to_owned())
            .or_default()
            .1
            .extend(stored.iter().cloned());
        Ok(stored)
    }
}

impl ControlJournal for Journal {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        Ok(self
            .streams
            .get(stream.as_str())
            .map(|(_, rows)| rows.clone())
            .unwrap_or_default())
    }

    fn head(&self, stream: &StreamId) -> Result<Head, ControlError> {
        let state = self.streams.get(stream.as_str());
        let last = state.and_then(|(_, rows)| rows.last());
        Ok(Head {
            seq: last.map_or(0, |r| r.seq),
            hash: last.map_or(Digest::ZERO, |r| r.hash),
            writer_epoch: state.map_or(0, |(epoch, _)| *epoch),
        })
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        let state = self.streams.entry(stream.as_str().to_owned()).or_default();
        state.0 += 1;
        Ok(state.0)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        let ids: Vec<String> = drafts
            .iter()
            .map(|bytes| {
                parse(bytes)
                    .ok()
                    .and_then(|d| d.get("event_id").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_default()
            })
            .collect();
        if stream.as_str() == CONTROL {
            self.attempts.push(ids.clone());
        }
        let already: Vec<Option<StoredEvent>> =
            ids.iter().map(|id| self.stored(id).cloned()).collect();
        if let Some(first) = already.iter().flatten().next() {
            let same = drafts
                .iter()
                .zip(&already)
                .all(|(bytes, row)| row.as_ref().is_some_and(|r| r.body == *bytes));
            return Ok(if same {
                AppendOutcome::AlreadyCommitted(already.into_iter().flatten().collect())
            } else {
                AppendOutcome::IdempotencyConflict {
                    stored_seq: first.seq,
                }
            });
        }
        if self.fence_next > 0 {
            self.fence_next -= 1;
            let state = self.streams.entry(stream.as_str().to_owned()).or_default();
            state.0 += 1;
            return Ok(AppendOutcome::Fenced {
                current_epoch: state.0,
            });
        }
        if self.behind_next > 0 {
            self.behind_next -= 1;
            self.foreign += 1;
            let id = format!("3{:025}", self.foreign);
            let other = draft(
                stream.as_str(),
                &id,
                "OwnerCommandIssued",
                None,
                object(&[("command", text("pause")), ("subject", text(OTHER_AGENT))]),
            );
            self.store(stream, recorded_at, &[&other])?;
        }
        let head = self.head(stream)?;
        if writer_epoch != head.writer_epoch {
            return Ok(AppendOutcome::Fenced {
                current_epoch: head.writer_epoch,
            });
        }
        if expected_head != head.seq {
            return Ok(AppendOutcome::HeadMismatch {
                actual_seq: head.seq,
                actual_hash: head.hash,
            });
        }
        let stored = self.store(stream, recorded_at, drafts)?;
        if self.ambiguous_next > 0 {
            self.ambiguous_next -= 1;
            return Ok(AppendOutcome::Ambiguous);
        }
        Ok(AppendOutcome::Committed(stored))
    }
}

/// A journal and the approvals the fixture asked, by agent.
pub struct Fixture {
    pub journal: Journal,
    epochs: BTreeMap<String, u64>,
    pub asked: Vec<Asked>,
    next: u64,
}

#[derive(Debug, Clone)]
pub struct Asked {
    pub agent: String,
    pub approval: String,
    pub content: Value,
    pub deadline: i64,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            journal: Journal::default(),
            epochs: BTreeMap::new(),
            asked: Vec::new(),
            next: 0,
        }
    }

    fn append(
        &mut self,
        stream_text: &str,
        event_type: &str,
        causation: Option<&str>,
        payload: Value,
    ) -> String {
        self.next += 1;
        let id = runtime_id(self.next);
        let s = stream(stream_text);
        let epoch = match self.epochs.get(stream_text) {
            Some(epoch) => *epoch,
            None => {
                let epoch = self.journal.take_ownership(&s).unwrap();
                self.epochs.insert(stream_text.to_owned(), epoch);
                epoch
            }
        };
        let head = self.journal.head(&s).unwrap().seq;
        let bytes = draft(stream_text, &id, event_type, causation, payload);
        let outcome = self
            .journal
            .append(&s, head, epoch, at(ASKED_AT).at, &[&bytes])
            .unwrap();
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "the fixture's {event_type}: {outcome:?}"
        );
        id
    }

    /// The runtime asks `agent` for `qty` shares at `asked_at`, as DEC-257 item 9 records it.
    pub fn ask(&mut self, agent: &str, qty: &str, asked_at: i64) -> Asked {
        let deadline = asked_at + TIMEOUT_S;
        let content = content_object(&content(qty, deadline)).unwrap();
        let payload = object(&[
            ("instrument", text("AAPL")),
            ("asset_class", text("us_equity")),
            ("side", text("buy")),
            ("qty", text(qty)),
            ("limit", text("155")),
            ("purpose", text("open")),
            ("mandate_version", text("v1")),
            ("decided_by", text("rule:big-order")),
            ("deadline", int(deadline)),
            ("timeout_s", int(TIMEOUT_S)),
            ("on_timeout", text("skip")),
            ("content", content.clone()),
            ("content_hash", text(&hash_of(&content))),
        ]);
        let approval = self.append(&agent_stream(agent), "ApprovalRequested", None, payload);
        self.append(
            &agent_stream(agent),
            "ApprovalDelivered",
            None,
            object(&[
                ("approval", text(&approval)),
                ("channel", text("cli_inbox")),
                ("status", text("delivered")),
                ("message_id", Value::Null),
            ]),
        );
        let asked = Asked {
            agent: agent.to_owned(),
            approval,
            content,
            deadline,
        };
        self.asked.push(asked.clone());
        asked
    }

    /// The runtime's record of an answer, naming the control-stream event it copies.
    pub fn responded(
        &mut self,
        asked: &Asked,
        source: &str,
        verdict: &str,
        result: &str,
        reason: Option<&str>,
    ) {
        self.append(
            &agent_stream(&asked.agent),
            "ApprovalResponded",
            Some(source),
            object(&[
                ("approval", text(&asked.approval)),
                ("verdict", text(verdict)),
                ("responder", text(OWNER)),
                ("result", text(result)),
                ("reason", reason.map_or(Value::Null, text)),
            ]),
        );
    }

    pub fn revalidated(&mut self, asked: &Asked, result: &str, reason: Option<&str>) {
        self.append(
            &agent_stream(&asked.agent),
            "ApprovalRevalidated",
            None,
            object(&[
                ("approval", text(&asked.approval)),
                ("result", text(result)),
                ("reason", reason.map_or(Value::Null, text)),
            ]),
        );
    }

    pub fn ended(&mut self, asked: &Asked, event_type: &str) {
        self.append(
            &agent_stream(&asked.agent),
            event_type,
            None,
            object(&[
                ("approval", text(&asked.approval)),
                ("reason", text("owner_pause")),
            ]),
        );
    }

    /// The runtime's mode change on `agent`'s stream.
    pub fn mode(&mut self, agent: &str, to: &str, reason: &str) {
        self.append(
            &agent_stream(agent),
            "AgentModeChanged",
            None,
            object(&[
                ("from", text("normal")),
                ("to", text(to)),
                ("reason", text(reason)),
                ("lifecycle", text("normal")),
            ]),
        );
    }

    pub fn rows(&self, stream_text: &str) -> Vec<StoredEvent> {
        self.journal.rows(&stream(stream_text)).unwrap()
    }
}

/// One stored event's envelope, re-parsed from its bytes.
pub fn body(row: &StoredEvent) -> Value {
    parse(&row.body).unwrap()
}

pub fn member<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |v, k| v.get(k))
}

pub fn member_text<'a>(value: &'a Value, path: &str) -> Option<&'a str> {
    member(value, path).and_then(Value::as_str)
}
