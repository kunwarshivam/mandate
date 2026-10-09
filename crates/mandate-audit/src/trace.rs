//! The causal trace (workspace API §4.8.1 "Causal trace", J2, FR-7.2, backlog E12-1, DEC-761,
//! DEC-762, DEC-772): a breadth-first walk from one event back to its causes, through the
//! envelope's `causation_id` and the payload link members of §4.8.1's table only, every lookup
//! scoped to the caller's workspace (AU-1), bounded and visiting each event once (AU-3), with model
//! output only in each node's `quoted` list (AU-4, API-18).

use std::collections::BTreeSet;
use std::ops::ControlFlow;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::demand::{Permitted, ReadRecords};
use mandate_journal::StreamId;

use crate::{AuditError, JournalEvent, JournalRead, MemoryRead, segment, served};

/// The deepest a trace goes: a node is at most 16 hops from the start (DEC-762 item 2).
pub const MAX_DEPTH: u16 = 16;

/// The most events a trace holds, the start included (DEC-762 item 2).
pub const MAX_NODES: usize = 256;

/// The most hops a trace records; the walk stops at this many (DEC-762 item 2).
pub const MAX_HOPS: usize = 1024;

/// What became of one link target, in the order DEC-772 item 2 checks them (DEC-762 items 2 to 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopStatus {
    /// The target is already in the trace; it is not followed again.
    AlreadyShown,
    /// The depth or event bound kept the target out of the trace.
    BeyondBound,
    /// The target is absent, in another workspace, or not on the stream the row names.
    NotRecorded,
    /// The target is in the workspace but of a type the row does not name: shown, not followed.
    UnexpectedType { event_type: String },
    /// The target was added to the trace at the next depth and is followed in its turn.
    Shown,
}

/// One link the walk met: `from` the event that holds it, `link` the member's name as §4.8.1's
/// table writes it (`causation_id`, `payload.intent_id`, `payload.evidence[]`, …), and `to` the
/// target's `event_id` when the walk resolved it in the workspace, `None` otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    pub from: String,
    pub link: String,
    pub to: Option<String>,
    pub status: HopStatus,
}

/// Who a quoted output is attributed to (`QuotedContent.author`, mandate spec §6.4, DEC-773).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    OwnerSelected,
    PlatformAuthored,
}

/// `QuotedContent` of `schemas/workspace-api/common.schema.json`, with no member added: `text` is
/// empty when the member holds an artifact ref, and `artifact` is that ref as `sha256:` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedContent {
    pub author: Author,
    pub text: String,
    pub model_id: String,
    pub model_version: String,
    pub produced_at: String,
    pub event_id: String,
    pub artifact: Option<String>,
}

/// One model-authored member of a node's event: `path` its JSON Pointer in the event body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoted {
    pub path: String,
    pub quoted: QuotedContent,
}

/// One event of the trace: the page item with its `stream_id`, its depth, and its quoted members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceNode {
    pub event: JournalEvent,
    pub depth: u16,
    pub quoted: Vec<Quoted>,
}

/// One stream the walk read, at its head in the walk's snapshot (`AsOf`); the API adds the
/// `sha256:` prefix to `hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watermark {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Digest,
    pub recorded_at: String,
}

/// The response of `GET /journal/events/{event_id}/trace`: nodes in the order the walk added them,
/// hops in the order it met them, and `as_of` in ascending `stream_id` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub start: String,
    pub nodes: Vec<TraceNode>,
    pub hops: Vec<Hop>,
    pub truncated: bool,
    pub as_of: Vec<Watermark>,
}

/// The causal trace over a store, for the witness of a context authorized for `ReadRecords`
/// (DEC-655). A start event outside its workspace is the one [`AuditError::NotFound`]; every link
/// that cannot be shown is a hop, never an error.
pub trait TraceRead {
    fn trace(
        &self,
        tenant: &Permitted<'_, ReadRecords>,
        event_id: &str,
    ) -> Result<Trace, AuditError>;
}

impl TraceRead for MemoryRead<'_> {
    fn trace(
        &self,
        tenant: &Permitted<'_, ReadRecords>,
        event_id: &str,
    ) -> Result<Trace, AuditError> {
        let segment = segment(tenant)?;
        let start = self.event(tenant, event_id)?;
        let mut walk = Walk {
            read: self,
            tenant,
            segment,
            nodes: Vec::new(),
            hops: Vec::new(),
            truncated: false,
            streams: BTreeSet::new(),
        };
        walk.add(start, 0, true);
        walk.run();
        let as_of = walk.as_of();
        Ok(Trace {
            start: event_id.to_owned(),
            nodes: walk.nodes.into_iter().map(|node| node.node).collect(),
            hops: walk.hops,
            truncated: walk.truncated,
            as_of,
        })
    }
}

/// The envelope's link member (§4.8.1).
const CAUSATION: &str = "causation_id";

/// `ModelOutputRecorded`'s evidence list (§4.8.1's table, DEC-772 item 4).
const EVIDENCE: &str = "payload.evidence[]";

/// `IntentReceived`'s intent (§4.8.1's table, DEC-772 item 4).
const INTENT: &str = "payload.intent_id";

/// The types an `IntentProposed`'s `causation_id` names (journal spec §9.1 rule 10).
const PROPOSED_CAUSES: &[&str] = &["DecisionMade", "ApprovalRevalidated", "OwnerExitRequested"];

/// The type the `intent_id` row looks up on the agent stream.
const PROPOSED: &[&str] = &["IntentProposed"];

/// The types the `intent_id` row looks up on the from event's own stream, in the row's order.
const INTENT_RECORDS: [&str; 2] = ["IntentReceived", "GateDecided"];

/// One link member of an event, before its targets are looked up.
enum Link {
    /// A member naming one event by id, looked up among all the workspace's streams; `expected`
    /// the types the row names for it, `None` when any type is followed.
    ById {
        name: &'static str,
        id: String,
        expected: Option<&'static [&'static str]>,
    },
    /// An `intent_id`, with the `agent_id` that names the agent stream of its `IntentProposed`.
    Intent { id: String, agent: Option<String> },
}

/// One target of a link: named by id and not yet resolved, found by a lookup on the stream the row
/// names, or a target the row names with no match there (DEC-772 item 5).
enum Target {
    Id(String),
    Found(JournalEvent),
    Missing,
}

/// A node of the walk, and whether its links are followed (`false` for `unexpected_type`, DEC-772
/// item 8).
struct Walked {
    node: TraceNode,
    followed: bool,
}

/// The state of one walk: every read goes through `read` scoped to `tenant`'s `segment`.
struct Walk<'r, 'j, 't> {
    read: &'r MemoryRead<'j>,
    tenant: &'r Permitted<'t, ReadRecords>,
    segment: String,
    nodes: Vec<Walked>,
    hops: Vec<Hop>,
    truncated: bool,
    streams: BTreeSet<String>,
}

impl Walk<'_, '_, '_> {
    /// Breadth first: each node's links in the order [`links`] gives them, until the nodes run out
    /// or the hop bound stops the walk.
    fn run(&mut self) {
        let mut next: usize = 0;
        while let Some(walked) = self.nodes.get(next) {
            next = next.saturating_add(1);
            if !walked.followed {
                continue;
            }
            let from = walked.node.event.clone();
            let depth = walked.node.depth;
            for link in links(&from) {
                if self.follow(&from, depth, link).is_break() {
                    return;
                }
            }
        }
    }

    fn follow(&mut self, from: &JournalEvent, depth: u16, link: Link) -> ControlFlow<()> {
        match link {
            Link::ById { name, id, expected } => {
                self.hop(&from.event_id, depth, name, Target::Id(id), expected)
            }
            Link::Intent { id, agent } => {
                let read = self.read;
                let rows = read
                    .rows(&self.segment, &from.stream_id)
                    .unwrap_or_default();
                for kind in INTENT_RECORDS {
                    let found: Vec<JournalEvent> = rows
                        .iter()
                        .filter(|row| {
                            row.event_type == kind && intent_of(&row.body).as_ref() == Some(&id)
                        })
                        .map(served)
                        .collect();
                    if found.is_empty() {
                        self.hop(&from.event_id, depth, INTENT, Target::Missing, None)?;
                    }
                    for event in found {
                        self.hop(&from.event_id, depth, INTENT, Target::Found(event), None)?;
                    }
                }
                let proposed = agent
                    .and_then(|agent| self.on_agent_stream(&agent, &id))
                    .map_or(Target::Missing, Target::Found);
                self.hop(&from.event_id, depth, INTENT, proposed, Some(PROPOSED))
            }
        }
    }

    /// The event `id` on `agent:{ws}:{agent}`, built with the path's workspace segment; a stream
    /// that exists in the workspace is read, so it joins `as_of` (DEC-772 item 6).
    fn on_agent_stream(&mut self, agent: &str, id: &str) -> Option<JournalEvent> {
        let stream = StreamId::parse(&format!("agent:{}:{agent}", self.segment))?;
        let read = self.read;
        let rows = read.rows(&self.segment, stream.as_str()).ok()?;
        let last = rows.last()?;
        self.streams.insert(last.stream_id.clone());
        rows.iter().find(|row| row.event_id == id).map(served)
    }

    /// Records one hop, in DEC-772 item 2's order, and adds a shown target as a node; breaks once
    /// the hop bound is reached (DEC-772 item 3).
    fn hop(
        &mut self,
        from: &str,
        depth: u16,
        link: &'static str,
        target: Target,
        expected: Option<&'static [&'static str]>,
    ) -> ControlFlow<()> {
        if self.hops.len() >= MAX_HOPS {
            self.truncated = true;
            return ControlFlow::Break(());
        }
        let named = match &target {
            Target::Id(id) => Some(id.as_str()),
            Target::Found(event) => Some(event.event_id.as_str()),
            Target::Missing => None,
        };
        let (to, status) = if let Some(id) = named.filter(|id| self.shown(id)) {
            (Some(id.to_owned()), HopStatus::AlreadyShown)
        } else if depth >= MAX_DEPTH || self.nodes.len() >= MAX_NODES {
            self.truncated = true;
            (None, HopStatus::BeyondBound)
        } else {
            let event = match target {
                Target::Id(id) => self.read.event(self.tenant, &id).ok(),
                Target::Found(event) => Some(event),
                Target::Missing => None,
            };
            match event {
                None => (None, HopStatus::NotRecorded),
                Some(event) => {
                    let to = Some(event.event_id.clone());
                    let wanted = expected.is_none_or(|types| types.contains(&&*event.event_type));
                    let status = if wanted {
                        HopStatus::Shown
                    } else {
                        HopStatus::UnexpectedType {
                            event_type: event.event_type.clone(),
                        }
                    };
                    self.add(event, depth.saturating_add(1), wanted);
                    (to, status)
                }
            }
        };
        self.hops.push(Hop {
            from: from.to_owned(),
            link: link.to_owned(),
            to,
            status,
        });
        ControlFlow::Continue(())
    }

    fn shown(&self, id: &str) -> bool {
        self.nodes
            .iter()
            .any(|walked| walked.node.event.event_id == id)
    }

    fn add(&mut self, event: JournalEvent, depth: u16, followed: bool) {
        self.streams.insert(event.stream_id.clone());
        let quoted = quoted(&event);
        self.nodes.push(Walked {
            node: TraceNode {
                event,
                depth,
                quoted,
            },
            followed,
        });
    }

    /// The head of every stream the walk read, in ascending `stream_id` bytes.
    fn as_of(&self) -> Vec<Watermark> {
        self.streams
            .iter()
            .filter_map(|stream| {
                let last = self.read.rows(&self.segment, stream).ok()?.last()?;
                Some(Watermark {
                    stream_id: last.stream_id.clone(),
                    seq: last.seq,
                    hash: last.hash,
                    recorded_at: last.recorded_at.clone(),
                })
            })
            .collect()
    }
}

/// The links of `event` in §4.8.1's order: its `causation_id`, then its row's payload members, a
/// list in its own order. Only the rows slice A2 covers are read (`ModelOutputRecorded`'s
/// `evidence`, `IntentReceived`'s `intent_id`); the others are slice A2b.
fn links(event: &JournalEvent) -> Vec<Link> {
    let Ok(body) = parse(&event.body) else {
        return Vec::new();
    };
    let payload = body.get("payload");
    let text = |name: &str| payload.and_then(|p| p.get(name)).and_then(Value::as_str);
    let expected = (event.event_type == "IntentProposed").then_some(PROPOSED_CAUSES);
    let mut links: Vec<Link> = body
        .get(CAUSATION)
        .filter(|v| **v != Value::Null)
        .map(malformed_or_id)
        .map(|id| Link::ById {
            name: CAUSATION,
            id: id.to_owned(),
            expected,
        })
        .into_iter()
        .collect();
    match event.event_type.as_str() {
        "ModelOutputRecorded" => {
            let cited = payload
                .and_then(|p| p.get("evidence"))
                .and_then(Value::as_array);
            links.extend(
                cited
                    .unwrap_or_default()
                    .iter()
                    .map(malformed_or_id)
                    .map(|id| Link::ById {
                        name: EVIDENCE,
                        id: id.to_owned(),
                        expected: None,
                    }),
            );
        }
        "IntentReceived" => links.extend(text("intent_id").map(|id| Link::Intent {
            id: id.to_owned(),
            agent: text("agent_id").map(str::to_owned),
        })),
        _ => {}
    }
    links
}

/// A link member's id; a value that is not a string names no event, so it reads `not_recorded`
/// (DEC-772 item 9).
fn malformed_or_id(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// The `payload.intent_id` of a stored body.
fn intent_of(body: &[u8]) -> Option<String> {
    let body = parse(body).ok()?;
    body.get("payload")?
        .get("intent_id")?
        .as_str()
        .map(str::to_owned)
}

/// The model-authored members of `event` in §4.8.1's order, each a `platform_authored` item
/// (DEC-773, Proposed); an absent or `null` member gives none (DEC-772 item 7).
fn quoted(event: &JournalEvent) -> Vec<Quoted> {
    if event.event_type != "ModelOutputRecorded" {
        return Vec::new();
    }
    let Ok(body) = parse(&event.body) else {
        return Vec::new();
    };
    let payload = body.get("payload");
    let member = |name: &str| {
        payload
            .and_then(|p| p.get(name))
            .filter(|v| **v != Value::Null)
    };
    let item = |name: &str, text: String, artifact: Option<String>| Quoted {
        path: format!("/payload/{name}"),
        quoted: QuotedContent {
            author: Author::PlatformAuthored,
            text,
            model_id: text_of(payload, "model_id"),
            model_version: text_of(payload, "model_version"),
            produced_at: event.recorded_at.clone(),
            event_id: event.event_id.clone(),
            artifact,
        },
    };
    let invalidation = member("invalidation").map(|v| item("invalidation", inline(v), None));
    let thesis = member("thesis_ref").map(|v| item("thesis_ref", String::new(), Some(inline(v))));
    invalidation.into_iter().chain(thesis).collect()
}

fn text_of(payload: Option<&Value>, name: &str) -> String {
    payload
        .and_then(|p| p.get(name))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// A member's recorded value as text: a string as recorded, any other value as its canonical JSON
/// (§4.8.1 "Model output").
fn inline(value: &Value) -> String {
    value.as_str().map_or_else(
        || String::from_utf8_lossy(&to_canonical(value)).into_owned(),
        str::to_owned,
    )
}
