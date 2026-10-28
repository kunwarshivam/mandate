//! The model gateway's meter streams, journaled (DEC-2258, E21-61, E21-62): a workspace's
//! `meter:{workspace_id}` and the deployment's `dmeter:{deployment_id}` (journal spec §9.19,
//! DEC-2207) through a [`Store`], so a restarted gateway folds every reservation it made, an open
//! one at its maximum, and its caps hold across a crash (DEC-998 items 5 and 8). One writer per
//! stream: the store's writer epoch, taken at open, fences an older writer.

use std::rc::Rc;

use mandate_canon::{Object, Value, parse};
use mandate_gateway::meter::{MeterRecord, MeterStore, StoreError, rule_158};
use mandate_gateway::provider::IdSource;
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_time::UtcNanos;

use crate::Store;
use crate::envelope::{Envelope, Fields, draft_bytes, object, text};

/// A meter store over one journaled meter stream, a workspace's or the deployment's.
pub struct JournalMeter<S: Store> {
    store: S,
    stream: Option<StreamId>,
    opened: Opened,
    environment: Environment,
    epoch: Option<u64>,
    now: Rc<dyn Fn() -> UtcNanos>,
    ids: Box<dyn IdSource>,
}

impl<S: Store> JournalMeter<S> {
    /// The meter stream of `workspace` in `environment` over `store`, its writer epoch taken now,
    /// each record written at `now()`, and the stream's `StreamOpened`, when this writer opens it,
    /// at the next of `ids`. A workspace that names no stream, or a store that grants no epoch,
    /// gives a meter whose every read and append fails `Unavailable` (DEC-2258 item 4).
    pub fn open(
        store: S,
        workspace: &str,
        environment: Environment,
        now: Rc<dyn Fn() -> UtcNanos>,
        ids: Box<dyn IdSource>,
    ) -> Self {
        let opened = Opened {
            prefix: "meter",
            stream_type: "meter",
            member: "workspace_id",
            id: workspace.to_owned(),
        };
        Self::over(store, opened, environment, now, ids)
    }

    /// As [`JournalMeter::open`], over the deployment's meter stream `dmeter:{deployment_id}`,
    /// which holds a copy of every workspace's meter records for the deployment's cap and opens
    /// with `{stream_type: "deployment_meter", deployment_id}` (journal spec §9.19, DEC-2207).
    pub fn open_deployment(
        store: S,
        deployment_id: &str,
        environment: Environment,
        now: Rc<dyn Fn() -> UtcNanos>,
        ids: Box<dyn IdSource>,
    ) -> Self {
        let opened = Opened {
            prefix: "dmeter",
            stream_type: "deployment_meter",
            member: "deployment_id",
            id: deployment_id.to_owned(),
        };
        Self::over(store, opened, environment, now, ids)
    }

    fn over(
        mut store: S,
        opened: Opened,
        environment: Environment,
        now: Rc<dyn Fn() -> UtcNanos>,
        ids: Box<dyn IdSource>,
    ) -> Self {
        let stream = StreamId::parse(&format!("{}:{}", opened.prefix, opened.id));
        let epoch = stream
            .as_ref()
            .and_then(|stream| store.take_ownership(stream));
        Self {
            store,
            stream,
            opened,
            environment,
            epoch,
            now,
            ids,
        }
    }

    /// The stream, this writer's epoch, and the stream's rows, or `Unavailable`.
    fn held(&self) -> Result<(&StreamId, u64, Vec<StoredEvent>), StoreError> {
        let (Some(stream), Some(epoch)) = (self.stream.as_ref(), self.epoch) else {
            return Err(StoreError::Unavailable);
        };
        let rows = self.store.rows(stream).ok_or(StoreError::Unavailable)?;
        Ok((stream, epoch, rows))
    }
}

/// A workspace's meter stream `meter:{workspace_id}` read through a [`Store`] without its writer
/// epoch (DEC-2397 item 4): the runtime that calls models through the model-gateway service reads
/// the service's stream for its own checks, and must never fence the service, the stream's one
/// writer. It takes no epoch, so every append is refused `Unavailable` and writes nothing.
pub struct ReadMeter<S: Store> {
    store: S,
    stream: Option<StreamId>,
}

impl<S: Store> ReadMeter<S> {
    /// The meter stream of `workspace` through `store`. A workspace that names no stream gives a
    /// meter whose every read fails `Unavailable`.
    pub fn open(store: S, workspace: &str) -> Self {
        Self {
            store,
            stream: StreamId::parse(&format!("meter:{workspace}")),
        }
    }
}

impl<S: Store> MeterStore for ReadMeter<S> {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        let stream = self.stream.as_ref().ok_or(StoreError::Unavailable)?;
        records(&self.store.rows(stream).ok_or(StoreError::Unavailable)?)
    }

    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        let _never_written = record;
        Err(StoreError::Unavailable)
    }
}

/// What a meter stream is named for and opens with: its prefix, its `StreamOpened`'s
/// `stream_type`, and the member naming whose stream it is.
struct Opened {
    prefix: &'static str,
    stream_type: &'static str,
    member: &'static str,
    id: String,
}

/// The meter records of `rows`, the stream's `StreamOpened` aside, each read back from its
/// payload; `Unavailable` for any row that does not read back.
fn records(rows: &[StoredEvent]) -> Result<Vec<MeterRecord>, StoreError> {
    rows.iter()
        .filter(|row| row.event_type != OPENED)
        .map(|row| {
            let body = parse(&row.body).map_err(|_| StoreError::Unavailable)?;
            let payload = body.get("payload").ok_or(StoreError::Unavailable)?;
            MeterRecord::from_journal(&row.event_type, &row.event_id, payload)
                .ok_or(StoreError::Unavailable)
        })
        .collect()
}

const OPENED: &str = "StreamOpened";

impl<S: Store> MeterStore for JournalMeter<S> {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        records(&self.held()?.2)
    }

    fn append(&mut self, record: MeterRecord) -> Result<(), StoreError> {
        let (stream, epoch, rows) = self.held()?;
        let stream = stream.clone();
        rule_158(&records(&rows)?, &record)?;
        let at = (self.now)();
        let envelope = Envelope {
            stream: stream.as_str(),
            environment: self.environment,
            actor_kind: "system",
            actor_id: "model-gateway",
            build: concat!("mandate-store/", env!("CARGO_PKG_VERSION")),
            version: env!("CARGO_PKG_VERSION"),
            event_time: at,
        };
        let draft = |event_id: &str, event_type: &str, payload: &Value| {
            draft_bytes(
                &envelope,
                &Fields {
                    event_id,
                    event_type,
                    schema_version: 1,
                    causation_id: None,
                    correlation_id: None,
                    config_refs: &Object::new(),
                    payload,
                },
            )
            .ok_or(StoreError::Unavailable)
        };
        let mut drafts = Vec::new();
        if rows.is_empty() {
            let opened = object(vec![
                ("stream_type", text(self.opened.stream_type)),
                (self.opened.member, text(&self.opened.id)),
            ])
            .ok_or(StoreError::Unavailable)?;
            drafts.push(draft(&self.ids.next_id(), OPENED, &opened)?);
        }
        drafts.push(draft(
            record.event_id(),
            record.event_type(),
            &record.payload(),
        )?);
        let head = rows.last().map_or(0, |row| row.seq);
        match self.store.append(&stream, head, epoch, at, &drafts) {
            AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => Ok(()),
            AppendOutcome::Fenced { .. } => Err(StoreError::Fenced),
            AppendOutcome::Invalid { .. } | AppendOutcome::IdempotencyConflict { .. } => {
                Err(StoreError::Rule158("the journal refused the meter record"))
            }
            AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::Unavailable
            | AppendOutcome::Ambiguous => Err(StoreError::Unavailable),
        }
    }
}
