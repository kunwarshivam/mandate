//! The news stream's two records (journal spec §9.37) as draft bytes, with the envelope of §3 and
//! deterministic event ids: a retried batch re-derives the same ids, so the journal answers
//! `AlreadyCommitted` rather than writing it twice.
//!
//! The reader times its records by its receipt of the items, the host clock, so `clock_source` is
//! `local`, not the scheduler's.

use std::collections::BTreeSet;

use mandate_canon::{Object, Value, stream_event_id};
use mandate_journal::{ArtifactRef, Environment};
use mandate_store::envelope::{
    ClockSource, Envelope, Fields, draft_bytes_with_clock, int, object, text,
};
use mandate_time::UtcNanos;

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One item the reader stores: `NewsItemStored`'s members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub source: String,
    pub allowlist_version: u32,
    pub item_id: String,
    /// The vendor's tickers, sorted and unique.
    pub symbols: BTreeSet<String>,
    pub published_at: UtcNanos,
    pub knowledge_at: UtcNanos,
    pub item_ref: ArtifactRef,
    pub licensed: bool,
}

/// One record the reader writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Opened,
    Stored(Stored),
}

/// Where and under which epoch a batch is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    pub workspace: String,
    pub environment: Environment,
    pub epoch: u64,
}

impl Writer {
    pub fn stream(&self) -> String {
        format!("news:{}", self.workspace)
    }

    /// The event id of the `ordinal`th draft of the batch appended at `head`: the news stream's
    /// [`stream_event_id`] at this writer's epoch. A page holds at most fifty items, so an ordinal
    /// past `u32::MAX` never occurs; it would read as `u32::MAX`.
    pub fn event_id(&self, head: u64, ordinal: usize) -> String {
        let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
        stream_event_id(&self.stream(), self.epoch, head, ordinal)
    }

    /// The drafts of `records`, appended as one batch at `head`, each with `event_time` `at`.
    /// `None` only for an integer the canonical form cannot hold, which nothing here writes.
    pub fn drafts(&self, head: u64, at: UtcNanos, records: &[Record]) -> Option<Vec<Vec<u8>>> {
        let stream = self.stream();
        let envelope = Envelope {
            stream: &stream,
            environment: self.environment,
            actor_kind: "system",
            actor_id: "news_reader",
            build: concat!("mandate-news-reader/", env!("CARGO_PKG_VERSION")),
            version: VERSION,
            event_time: at,
        };
        records
            .iter()
            .enumerate()
            .map(|(ordinal, record)| {
                let (event_type, payload) = payload(&self.workspace, record)?;
                draft_bytes_with_clock(
                    &envelope,
                    &Fields {
                        event_id: &self.event_id(head, ordinal),
                        event_type,
                        schema_version: 1,
                        causation_id: None,
                        correlation_id: None,
                        config_refs: &Object::new(),
                        payload: &payload,
                    },
                    ClockSource::Local,
                )
            })
            .collect()
    }
}

fn payload(workspace: &str, record: &Record) -> Option<(&'static str, Value)> {
    Some(match record {
        Record::Opened => (
            "StreamOpened",
            object(vec![
                ("stream_type", text("news")),
                ("workspace_id", text(workspace)),
            ])?,
        ),
        Record::Stored(item) => (
            "NewsItemStored",
            object(vec![
                ("allowlist_version", int(u64::from(item.allowlist_version))?),
                ("item_id", text(&item.item_id)),
                ("item_ref", text(&item.item_ref.to_string())),
                ("knowledge_at", Value::Str(item.knowledge_at.to_string())),
                ("licensed", Value::Bool(item.licensed)),
                ("published_at", Value::Str(item.published_at.to_string())),
                ("source", text(&item.source)),
                (
                    "symbols",
                    Value::Array(item.symbols.iter().map(|s| text(s)).collect()),
                ),
            ])?,
        ),
    })
}
