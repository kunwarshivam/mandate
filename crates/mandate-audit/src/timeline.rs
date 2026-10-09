//! The per-agent timeline's core read (workspace API §4.8.1 "Timeline", J1, FR-7.3, E12-2, AU-5,
//! DEC-764, DEC-777): the agent's stream merged with the account streams of the connections it was
//! deployed on, one cursor per stream. The merge runs over the raw stream heads, filtered or not, and
//! a page is a prefix of one unbounded merge (DEC-777 items 1 to 3). The route, its query parsing,
//! and the resolution of `account_refs` from the agent's `AgentDeployed` records are the caller's.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_identity::demand::{Permitted, ReadRecords};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::{AuditError, JournalEvent, MemoryRead, PageLimit, Watermark, segment, served};

/// The most events one timeline page consumes from one stream (DEC-764 item 4). Reaching it stops
/// the whole page, for every stream (DEC-777 item 3).
pub const MAX_CONSUMED_PER_STREAM: u64 = 10_000;

/// One stream's cursor, the `after=<stream_id>:<seq>` of the route: the last `seq` the timeline
/// consumed from that stream. A timeline stream no cursor names starts at 0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamCursor {
    pub stream_id: String,
    pub seq: u64,
}

/// One timeline request. `account_refs` are the `account_ref`s of the connections the agent's
/// `AgentDeployed` events name, which name the streams `acct:{ws}:{account_ref}` of the tenant's
/// own workspace. `types`, `from` (inclusive), and `to` (exclusive, on `recorded_at`) select what is
/// served; every event they exclude is still consumed (AU-5).
#[derive(Debug, Clone)]
pub struct TimelineQuery<'q> {
    pub agent_id: &'q str,
    pub account_refs: &'q [&'q str],
    pub after: &'q [StreamCursor],
    pub types: Option<&'q [&'q str]>,
    pub from: Option<UtcNanos>,
    pub to: Option<UtcNanos>,
    pub limit: PageLimit,
}

/// One timeline page, `{events, next, more, as_of}`: each event is the page item with its
/// `stream_id`, in merge order; `next` is the cursor of every timeline stream, in ascending
/// `stream_id`, one not reached yet at 0 (DEC-777 item 5); `more` is whether some stream holds an
/// event past its `next` cursor in the page's snapshot (DEC-777 item 4); `as_of` is one watermark per
/// stream read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub events: Vec<JournalEvent>,
    pub next: Vec<StreamCursor>,
    pub more: bool,
    pub as_of: Vec<Watermark>,
}

/// The per-agent timeline over a store, for the witness of a context authorized for `ReadRecords`
/// (DEC-655). An unknown agent, and a cursor naming a stream that is not one of the timeline's, are
/// the one [`AuditError::NotFound`] (DEC-760 item 4).
pub trait TimelineRead {
    fn timeline(
        &self,
        tenant: &Permitted<'_, ReadRecords>,
        query: &TimelineQuery<'_>,
    ) -> Result<Timeline, AuditError>;
}

impl TimelineRead for MemoryRead<'_> {
    fn timeline(
        &self,
        tenant: &Permitted<'_, ReadRecords>,
        query: &TimelineQuery<'_>,
    ) -> Result<Timeline, AuditError> {
        let segment = segment(tenant)?;
        let agent_stream = format!("agent:{segment}:{}", query.agent_id);
        let mut streams = BTreeMap::new();
        for account_ref in query.account_refs {
            let stream_id = format!("acct:{segment}:{account_ref}");
            let rows = self.rows(&segment, &stream_id)?;
            streams.insert(stream_id, rows);
        }
        let own = self.rows(&segment, &agent_stream)?;
        if own.is_empty() {
            return Err(AuditError::NotFound);
        }
        streams.insert(agent_stream.clone(), own);
        let members = Members::of(query.agent_id, &agent_stream, own, &streams);
        let mut lanes: Vec<Lane<'_>> = streams.iter().map(Lane::start).collect();
        for cursor in query.after {
            let lane = lanes
                .iter_mut()
                .find(|lane| lane.stream_id == cursor.stream_id)
                .ok_or(AuditError::NotFound)?;
            lane.resume(cursor.seq);
        }
        let limit = usize::from(query.limit.0);
        let mut events = Vec::new();
        while let Some(lane) = lanes
            .iter_mut()
            .filter(|lane| !lane.rest.is_empty())
            .min_by_key(|lane| lane.head_at())
        {
            let Some(row) = lane.consume() else { break };
            if members.holds(row) && query.selects(row) {
                events.push(served(row));
                if events.len() >= limit {
                    break;
                }
            }
            if lane.consumed >= MAX_CONSUMED_PER_STREAM {
                break;
            }
        }
        Ok(Timeline {
            events,
            more: lanes.iter().any(|lane| !lane.rest.is_empty()),
            next: lanes.iter().map(Lane::cursor).collect(),
            as_of: streams
                .values()
                .filter_map(|rows| rows.last())
                .map(watermark)
                .collect(),
        })
    }
}

impl TimelineQuery<'_> {
    /// Whether the `types`, `from` (inclusive), and `to` (exclusive) filters serve `row`. An event
    /// they exclude is still consumed (AU-5).
    fn selects(&self, row: &StoredEvent) -> bool {
        let at = recorded_at(row);
        self.types
            .is_none_or(|types| types.contains(&row.event_type.as_str()))
            && self.from.is_none_or(|from| at.is_some_and(|at| at >= from))
            && self.to.is_none_or(|to| at.is_some_and(|at| at < to))
    }
}

/// One timeline stream as the merge walks it: the rows after its cursor in the page's snapshot,
/// the cursor (the last `seq` consumed), and how many events this page consumed from it.
struct Lane<'a> {
    stream_id: &'a str,
    rest: &'a [StoredEvent],
    cursor: u64,
    consumed: u64,
}

impl<'a> Lane<'a> {
    /// A stream no cursor names, at 0 (DEC-777 item 5).
    fn start((stream_id, rows): (&'a String, &&'a [StoredEvent])) -> Self {
        Self {
            stream_id,
            rest: rows,
            cursor: 0,
            consumed: 0,
        }
    }

    /// Resumes after `seq`. A stream's `seq` is gapless from 1 (journal spec §3), so the rows after
    /// `seq` start at index `seq`; a cursor past the head leaves nothing and is kept as given.
    fn resume(&mut self, seq: u64) {
        let rows = self.rest;
        let after = usize::try_from(seq).unwrap_or(usize::MAX);
        self.rest = rows.get(after..).unwrap_or_default();
        self.cursor = seq;
    }

    /// The `recorded_at` of the next unconsumed row, the merge key. Lanes are in ascending
    /// `stream_id`, and `min_by_key` keeps the first of equal keys, so ties go to the least
    /// `stream_id` (DEC-764 item 3).
    fn head_at(&self) -> Option<UtcNanos> {
        self.rest.first().and_then(recorded_at)
    }

    fn consume(&mut self) -> Option<&'a StoredEvent> {
        let (row, rest) = self.rest.split_first()?;
        self.rest = rest;
        self.cursor = row.seq;
        self.consumed = self.consumed.saturating_add(1);
        Some(row)
    }

    fn cursor(&self) -> StreamCursor {
        StreamCursor {
            stream_id: self.stream_id.to_owned(),
            seq: self.cursor,
        }
    }
}

/// Which events are the agent's (DEC-764 item 2), with every lookup read from the timeline's own
/// streams in the page's snapshot, whole (DEC-777 item 6): the agent's own event ids, the
/// `intent_id`s whose `IntentReceived` names the agent, and the `client_order_id`s of version-2
/// `OrderSubmitted`s whose `causation_id` names an `OrderRequestRecorded` naming it (DEC-778).
struct Members<'a> {
    agent_id: &'a str,
    agent_stream: &'a str,
    own: BTreeSet<&'a str>,
    intents: BTreeSet<String>,
    orders: BTreeSet<String>,
}

impl<'a> Members<'a> {
    fn of(
        agent_id: &'a str,
        agent_stream: &'a str,
        own: &'a [StoredEvent],
        streams: &BTreeMap<String, &'a [StoredEvent]>,
    ) -> Self {
        let rows = || streams.values().flat_map(|rows| rows.iter());
        let typed = |event_type: &'static str| {
            rows()
                .filter(move |row| row.event_type == event_type)
                .filter_map(|row| Some((row, Body::of(row)?)))
        };
        let intents = typed("IntentReceived")
            .filter(|(_, body)| body.payload("agent_id") == Some(agent_id))
            .filter_map(|(_, body)| body.payload("intent_id").map(str::to_owned))
            .collect();
        let requests: BTreeSet<&str> = typed("OrderRequestRecorded")
            .filter(|(_, body)| body.payload("agent_id") == Some(agent_id))
            .map(|(row, _)| row.event_id.as_str())
            .collect();
        let orders = typed("OrderSubmitted")
            .filter(|(row, _)| row.schema_version == 2)
            .filter(|(_, body)| body.causation().is_some_and(|c| requests.contains(c)))
            .filter_map(|(_, body)| body.payload("client_order_id").map(str::to_owned))
            .collect();
        Self {
            agent_id,
            agent_stream,
            own: own.iter().map(|row| row.event_id.as_str()).collect(),
            intents,
            orders,
        }
    }

    /// Whether `row` is the agent's: on its own stream, or naming it or `*`, one of its intents or
    /// orders, or an event of its stream as `causation_id`. DEC-764 item 2's account-wide clause
    /// (`AccountRestrictionChanged`, a connection- or workspace-scope `KillSwitchActivated`) is
    /// omitted until the journal registers those payloads, so that no account stream can hold
    /// them yet (DEC-779).
    fn holds(&self, row: &StoredEvent) -> bool {
        if row.stream_id == self.agent_stream {
            return true;
        }
        let Some(body) = Body::of(row) else {
            return false;
        };
        let named = body.payload("agent_id").or_else(|| body.payload("agent"));
        named.is_some_and(|agent| agent == self.agent_id || agent == "*")
            || body
                .payload("intent_id")
                .is_some_and(|i| self.intents.contains(i))
            || body
                .payload("client_order_id")
                .is_some_and(|o| self.orders.contains(o))
            || body.causation().is_some_and(|c| self.own.contains(c))
    }
}

/// A row's parsed canonical body, whose text members membership reads.
struct Body(Value);

impl Body {
    fn of(row: &StoredEvent) -> Option<Self> {
        parse(&row.body).ok().map(Self)
    }

    fn payload(&self, name: &str) -> Option<&str> {
        self.0.get("payload")?.get(name)?.as_str()
    }

    fn causation(&self) -> Option<&str> {
        self.0.get("causation_id")?.as_str()
    }
}

fn recorded_at(row: &StoredEvent) -> Option<UtcNanos> {
    UtcNanos::parse(&row.recorded_at).ok()
}

fn watermark(last: &StoredEvent) -> Watermark {
    Watermark {
        stream_id: last.stream_id.clone(),
        seq: last.seq,
        hash: last.hash,
        recorded_at: last.recorded_at.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_canon::Digest;
    use mandate_journal::StoredEvent;

    use super::Members;

    /// An account-stream row whose payload is `payload`, with no other link to any agent.
    fn row(payload: &str) -> StoredEvent {
        StoredEvent {
            stream_id: "acct:ws:ACCT1".to_owned(),
            seq: 2,
            event_id: "E2".to_owned(),
            event_type: "AgentModeApplied".to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: "2026-09-21T15:00:00.000000000Z".to_owned(),
            prev_hash: Digest::ZERO,
            hash: Digest::ZERO,
            body: format!(r#"{{"causation_id":null,"payload":{payload}}}"#).into_bytes(),
        }
    }

    /// DEC-764 item 2's "its `agent_id` (or `agent`)": a payload holding both is judged by its
    /// `agent_id` alone, and `agent` is read only when `agent_id` is absent or `null`. No payload
    /// the journal registers holds both today, so this is pinned on the predicate itself.
    #[test]
    fn an_agent_id_takes_precedence_over_agent() {
        let members = Members {
            agent_id: "AG1",
            agent_stream: "agent:ws:AG1",
            own: BTreeSet::new(),
            intents: BTreeSet::new(),
            orders: BTreeSet::new(),
        };
        let cases = [
            (r#"{"agent_id":"AG1","agent":"AG2"}"#, true),
            (r#"{"agent_id":"AG2","agent":"AG1"}"#, false),
            (r#"{"agent_id":"AG2","agent":"*"}"#, false),
            (r#"{"agent_id":null,"agent":"AG1"}"#, true),
            (r#"{"agent":"AG1"}"#, true),
        ];
        for (payload, mine) in cases {
            assert_eq!(members.holds(&row(payload)), mine, "{payload}");
        }
    }
}
