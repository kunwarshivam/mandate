#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The Postgres journal hot store ([journal spec](../../../docs/specs/journal.md) §6.1, backlog
//! E5-3). It runs the append protocol of §5.1 over the same `Draft` and `seal` as
//! `mandate_journal::MemoryJournal`, with the same outcomes and rejection reasons, in one
//! transaction per append that holds the stream's head row locked, so concurrent appenders
//! serialize and can neither fork the chain nor reuse a `seq`.
//!
//! The database enforces the rest itself, through the forward-only migrations in `migrations/`
//! applied by [`migrator`] under [`OWNER_ROLE`]: the body is `bytea` with
//! `CHECK (hash = sha256(body))`, every event links to its predecessor, heads only advance, and the
//! event tables reject UPDATE, DELETE, and TRUNCATE by both privilege ([`APP_ROLE`] has INSERT and
//! SELECT) and trigger. Every read re-checks the stored bytes (§11 checks 1 to 5) before returning
//! them (DEC-109).

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{
    AppendOutcome, Draft, Environment, EventCheck, EventFailure, Head, Invalid, InvalidReason,
    StoredEvent, StreamId, StreamType, TrustedStart, check_batch, seal, verify_events,
};
use mandate_time::UtcNanos;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::postgres::{PgDatabaseError, PgRow, PgSeverity};
use sqlx::{PgPool, Postgres, Row, SqlSafeStr, Transaction};

/// The migration-only role that owns the journal tables (journal spec §6.1). Deployments create it
/// before the first migration; no application connects as it.
pub const OWNER_ROLE: &str = "mandate_journal_owner";

/// The application role: INSERT and SELECT on `events` and `event_ids`, and on `stream_heads` the
/// UPDATE that advances a head or takes ownership. Deployments create it before the first
/// migration, which grants it exactly these privileges.
pub const APP_ROLE: &str = "mandate_journal_app";

/// `migrations/`, in order. A new migration is a new file and a new entry, never an edit.
const MIGRATIONS: [(i64, &str, &str); 1] = [(
    1,
    "journal",
    include_str!("../../../migrations/0001_journal.sql"),
)];

/// The journal schema's migrations, embedded at build time: versioned, forward-only (no down
/// migrations), and applied only by the migration command under [`OWNER_ROLE`], never at startup
/// (ADR-0001 ES-08). Running it again is a no-op; a changed or unknown applied migration is an
/// error.
pub fn migrator() -> Migrator {
    Migrator::with_migrations(
        MIGRATIONS
            .iter()
            .map(|&(version, description, sql)| {
                Migration::new(
                    version,
                    description.into(),
                    MigrationType::Simple,
                    sql.into_sql_str(),
                    false,
                )
            })
            .collect(),
    )
}

/// Stored bytes failed re-verification on read: something changed the database outside the append
/// path. Nothing is repaired in place (journal spec §11); the caller treats it as an integrity
/// incident and stops writing the stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("stored event {stream_id} seq {} fails `{}`", failure.seq, failure.check.code())]
pub struct IntegrityError {
    pub stream_id: String,
    pub failure: EventFailure,
}

impl IntegrityError {
    /// Stable reason code (ADR-0001 ES-09): the failed check's code from journal spec §11.
    pub fn code(&self) -> &'static str {
        self.failure.check.code()
    }
}

/// Why a read or an ownership change did not return a result.
#[derive(Debug, thiserror::Error)]
pub enum PgError {
    /// The database could not be reached or the statement failed; nothing changed.
    #[error("the journal database is unavailable")]
    Unavailable(#[source] sqlx::Error),
    #[error(transparent)]
    Integrity(#[from] IntegrityError),
}

impl PgError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::Integrity(_) => "integrity",
        }
    }
}

impl From<sqlx::Error> for PgError {
    fn from(e: sqlx::Error) -> Self {
        Self::Unavailable(e)
    }
}

/// How many times an append reruns after losing a race it cannot see before it starts: the same
/// `event_id` committed to another stream, or a serialization failure.
const ATTEMPTS: usize = 3;

const COLUMNS: &str = "stream_id, seq, event_id, event_type, schema_version, environment, \
                       recorded_at, prev_hash, hash, body";

/// The journal over a pool whose connections act as [`APP_ROLE`]. Streams exist implicitly with
/// head 0 and writer epoch 0, as in `MemoryJournal`.
#[derive(Debug, Clone)]
pub struct PgJournal {
    pool: PgPool,
}

impl PgJournal {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Increments the stream's writer epoch, fencing out the previous writer, and returns it.
    pub async fn take_ownership(&self, stream: &StreamId) -> Result<u64, PgError> {
        let epoch: i64 = sqlx::query_scalar(
            "INSERT INTO stream_heads (stream_id, seq, hash, writer_epoch) VALUES ($1, 0, $2, 1) \
             ON CONFLICT (stream_id) DO UPDATE SET writer_epoch = stream_heads.writer_epoch + 1 \
             RETURNING writer_epoch",
        )
        .bind(stream.as_str())
        .bind(Digest::ZERO.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await?;
        Ok(unsigned(epoch)?)
    }

    pub async fn head(&self, stream: &StreamId) -> Result<Head, PgError> {
        let row = sqlx::query(
            "SELECT seq, hash, writer_epoch, risk_clock FROM stream_heads WHERE stream_id = $1",
        )
        .bind(stream.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            Some(row) => decode_head(&row)?.head,
            None => Head {
                seq: 0,
                hash: Digest::ZERO,
                writer_epoch: 0,
            },
        })
    }

    /// Appends `drafts` to `stream` in one transaction, in the order of journal spec §5.1, with the
    /// outcomes `MemoryJournal::append` gives for the same state and drafts. A failure before the
    /// commit is `Unavailable` (nothing was written; retry with the same drafts); a lost connection
    /// during the commit is `Ambiguous` (re-query by `event_id` before acting). `Err` means stored
    /// events the append had to read failed re-verification, and nothing was written.
    pub async fn append(
        &self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, IntegrityError> {
        let batch = match validate(stream, drafts) {
            Ok(batch) => batch,
            Err(invalid) => return Ok(invalid),
        };
        let request = Request {
            stream,
            expected_head,
            writer_epoch,
            recorded_at,
            batch: &batch,
        };
        for _ in 0..ATTEMPTS {
            let mut tx = match self.pool.begin().await {
                Ok(tx) => tx,
                Err(_) => return Ok(AppendOutcome::Unavailable),
            };
            match request.run(&mut tx).await {
                Ok(Step::Done(outcome)) => return outcome,
                Ok(Step::Commit(rows)) => return Ok(commit(tx, rows).await),
                Err(e) if lost_a_race(&e) => {}
                Err(_) => return Ok(AppendOutcome::Unavailable),
            }
        }
        Ok(AppendOutcome::Unavailable)
    }

    /// Every event of `stream` in `seq` order, after checking the whole chain from seq 1 (journal
    /// spec §11 checks 1 to 5). Artifact checks need the artifact store: run `verify_events` over
    /// these rows for those.
    pub async fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, PgError> {
        let rows = sqlx::query(sql(format!(
            "SELECT {COLUMNS} FROM events WHERE stream_id = $1 ORDER BY seq"
        )))
        .bind(stream.as_str())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(decode_event)
        .collect::<Result<Vec<_>, _>>()?;
        check(stream.as_str(), &rows, TrustedStart::GENESIS)?;
        Ok(rows)
    }

    /// The event stored under `event_id` in any stream, after checking its bytes, columns, and hash
    /// (journal spec §11 checks 1, 2, and 4).
    pub async fn event(&self, event_id: &str) -> Result<Option<StoredEvent>, PgError> {
        let ids = [event_id.to_owned()];
        let mut found = stored_events(&self.pool, &ids).await?;
        match found.remove(event_id) {
            Some(row) => {
                check_row(&row)?;
                Ok(Some(row))
            }
            None => Ok(None),
        }
    }
}

/// One append, rerun whole when it loses a race.
struct Request<'a> {
    stream: &'a StreamId,
    expected_head: u64,
    writer_epoch: u64,
    recorded_at: UtcNanos,
    batch: &'a [Draft],
}

enum Step {
    /// Decided without writing; the transaction rolls back.
    Done(Result<AppendOutcome, IntegrityError>),
    /// Written; the outcome depends on the commit.
    Commit(Vec<StoredEvent>),
}

impl Request<'_> {
    async fn run(&self, tx: &mut Transaction<'static, Postgres>) -> Result<Step, sqlx::Error> {
        let stream = self.stream.as_str();
        sqlx::query(
            "INSERT INTO stream_heads (stream_id, seq, hash, writer_epoch) VALUES ($1, 0, $2, 0) \
             ON CONFLICT (stream_id) DO NOTHING",
        )
        .bind(stream)
        .bind(Digest::ZERO.as_bytes().as_slice())
        .execute(&mut **tx)
        .await?;
        let locked = sqlx::query(
            "SELECT seq, hash, writer_epoch, risk_clock FROM stream_heads \
             WHERE stream_id = $1 FOR UPDATE",
        )
        .bind(stream)
        .fetch_one(&mut **tx)
        .await?;
        let StreamHead { head, risk_clock } = decode_head(&locked)?;

        let ids: Vec<String> = self.batch.iter().map(|d| d.event_id().to_owned()).collect();
        let found = stored_events(&mut **tx, &ids).await?;
        if !found.is_empty() {
            return Ok(Step::Done(idempotent(self.batch, &found)));
        }

        if self.writer_epoch != head.writer_epoch {
            return Ok(Step::Done(Ok(AppendOutcome::Fenced {
                current_epoch: head.writer_epoch,
            })));
        }
        if self.expected_head != head.seq {
            return Ok(Step::Done(Ok(AppendOutcome::HeadMismatch {
                actual_seq: head.seq,
                actual_hash: head.hash,
            })));
        }

        let mut environment = None;
        if head.seq > 0 {
            let last = sqlx::query(sql(format!(
                "SELECT {COLUMNS} FROM events WHERE stream_id = $1 AND seq = $2"
            )))
            .bind(stream)
            .bind(signed(head.seq)?)
            .fetch_optional(&mut **tx)
            .await?
            .as_ref()
            .map(decode_event)
            .transpose()?;
            let last = match last {
                Some(row) if row.hash == head.hash => row,
                _ => {
                    return Ok(Step::Done(Err(IntegrityError {
                        stream_id: stream.to_owned(),
                        failure: EventFailure {
                            seq: head.seq,
                            check: EventCheck::RehashMismatch,
                        },
                    })));
                }
            };
            if let Err(e) = check_row(&last) {
                return Ok(Step::Done(Err(e)));
            }
            environment = Environment::parse(&last.environment);
        }

        let sealed = match self.seal(head, environment, risk_clock) {
            Ok(sealed) => sealed,
            Err(invalid) => return Ok(Step::Done(Ok(invalid))),
        };
        for row in &sealed.rows {
            insert(tx, row).await?;
        }
        if let Some(last) = sealed.rows.last() {
            sqlx::query(
                "UPDATE stream_heads SET seq = $2, hash = $3, risk_clock = $4 WHERE stream_id = $1",
            )
            .bind(stream)
            .bind(signed(last.seq)?)
            .bind(last.hash.as_bytes().as_slice())
            .bind(sealed.risk_clock.map(|c| c.to_string()))
            .execute(&mut **tx)
            .await?;
        }
        Ok(Step::Commit(sealed.rows))
    }

    /// The stream rules and sealing of `MemoryJournal::append`, from the locked head.
    fn seal(
        &self,
        head: Head,
        mut environment: Option<Environment>,
        mut risk_clock: Option<UtcNanos>,
    ) -> Result<Sealed, AppendOutcome> {
        let mut rows = Vec::with_capacity(self.batch.len());
        let mut prev_hash = head.hash;
        let mut seq = head.seq;
        for (i, draft) in self.batch.iter().enumerate() {
            seq = seq
                .checked_add(1)
                .ok_or_else(|| invalid(i, InvalidReason::NonCanonical, "seq"))?;
            let opening = draft.event_type() == "StreamOpened";
            if seq == 1 && !opening {
                return Err(invalid(i, InvalidReason::NotStreamOpened, "event_type"));
            }
            if seq > 1 && opening {
                return Err(invalid(i, InvalidReason::StreamAlreadyOpened, "event_type"));
            }
            if *environment.get_or_insert(draft.environment()) != draft.environment() {
                return Err(invalid(
                    i,
                    InvalidReason::EnvironmentMismatch,
                    "environment",
                ));
            }
            if let Some(clock) = draft.risk_clock() {
                if risk_clock.is_some_and(|last| clock < last) {
                    return Err(invalid(
                        i,
                        InvalidReason::RiskClockRegressed,
                        "payload.risk_clock",
                    ));
                }
                risk_clock = Some(clock);
            }
            let row = seal(draft, seq, prev_hash, self.recorded_at)
                .map_err(|error| AppendOutcome::Invalid { draft: i, error })?;
            prev_hash = row.hash;
            rows.push(row);
        }
        Ok(Sealed { rows, risk_clock })
    }
}

struct Sealed {
    rows: Vec<StoredEvent>,
    risk_clock: Option<UtcNanos>,
}

struct StreamHead {
    head: Head,
    risk_clock: Option<UtcNanos>,
}

/// The batch checks of `MemoryJournal::append` that need no stored state.
fn validate(stream: &StreamId, drafts: &[&[u8]]) -> Result<Vec<Draft>, AppendOutcome> {
    if drafts.is_empty() {
        return Err(invalid(0, InvalidReason::EmptyBatch, ""));
    }
    let mut batch: Vec<Draft> = Vec::with_capacity(drafts.len());
    for (i, bytes) in drafts.iter().enumerate() {
        let draft =
            Draft::parse(bytes).map_err(|error| AppendOutcome::Invalid { draft: i, error })?;
        if draft.stream_id() != stream {
            return Err(invalid(i, InvalidReason::StreamMismatch, "stream_id"));
        }
        if batch.iter().any(|d| d.event_id() == draft.event_id()) {
            return Err(invalid(i, InvalidReason::DuplicateEventId, "event_id"));
        }
        batch.push(draft);
    }
    if stream.stream_type() == StreamType::Agent && batch.len() > 1 {
        check_batch(&batch).map_err(|(draft, error)| AppendOutcome::Invalid { draft, error })?;
    }
    Ok(batch)
}

fn invalid(draft: usize, reason: InvalidReason, path: &str) -> AppendOutcome {
    AppendOutcome::Invalid {
        draft,
        error: Invalid {
            reason,
            path: path.to_owned(),
        },
    }
}

/// Journal spec §5.1 step 1, as `MemoryJournal::append` decides it, once some draft is stored.
/// Every stored event is checked before it is compared or returned.
fn idempotent(
    batch: &[Draft],
    found: &BTreeMap<String, StoredEvent>,
) -> Result<AppendOutcome, IntegrityError> {
    let stored: Vec<Option<&StoredEvent>> = batch.iter().map(|d| found.get(d.event_id())).collect();
    for row in stored.iter().flatten() {
        check_row(row)?;
    }
    for (draft, row) in batch.iter().zip(&stored) {
        if let Some(row) = row
            && stored_draft(row).as_deref() != Some(draft.canonical_bytes())
        {
            return Ok(AppendOutcome::IdempotencyConflict {
                stored_seq: row.seq,
            });
        }
    }
    if stored.iter().any(Option::is_none) {
        return Ok(AppendOutcome::IdempotencyConflict {
            stored_seq: stored.iter().flatten().next().map_or(0, |r| r.seq),
        });
    }
    Ok(AppendOutcome::AlreadyCommitted(
        stored.into_iter().flatten().cloned().collect(),
    ))
}

/// The canonical draft inside a stored body: the body without the journal-assigned fields.
fn stored_draft(row: &StoredEvent) -> Option<Vec<u8>> {
    let Value::Object(mut body) = parse(&row.body).ok()? else {
        return None;
    };
    for field in ["prev_hash", "recorded_at", "seq"] {
        body.remove(field)?;
    }
    Some(to_canonical(&Value::Object(body)))
}

/// The stored events with these `event_id`s, in any stream, by `event_id`.
async fn stored_events<'e, E>(
    executor: E,
    event_ids: &[String],
) -> Result<BTreeMap<String, StoredEvent>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let rows = sqlx::query(sql(format!(
        "SELECT {} FROM event_ids i JOIN events e \
         ON e.stream_id = i.stream_id AND e.seq = i.seq AND e.event_id = i.event_id \
         WHERE i.event_id = ANY($1)",
        COLUMNS
            .split(", ")
            .map(|c| format!("e.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    )))
    .bind(event_ids)
    .fetch_all(executor)
    .await?;
    rows.iter()
        .map(|row| decode_event(row).map(|e| (e.event_id.clone(), e)))
        .collect()
}

async fn insert(
    tx: &mut Transaction<'static, Postgres>,
    row: &StoredEvent,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO event_ids (event_id, stream_id, seq) VALUES ($1, $2, $3)")
        .bind(&row.event_id)
        .bind(&row.stream_id)
        .bind(signed(row.seq)?)
        .execute(&mut **tx)
        .await?;
    sqlx::query(sql(format!(
        "INSERT INTO events ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
    )))
    .bind(&row.stream_id)
    .bind(signed(row.seq)?)
    .bind(&row.event_id)
    .bind(&row.event_type)
    .bind(signed(row.schema_version)?)
    .bind(&row.environment)
    .bind(&row.recorded_at)
    .bind(row.prev_hash.as_bytes().as_slice())
    .bind(row.hash.as_bytes().as_slice())
    .bind(&row.body)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The outcome once the rows are written: Postgres answers an error to COMMIT only after rolling
/// back, so only a connection lost before the answer leaves the outcome unknown (§5.3).
async fn commit(tx: Transaction<'static, Postgres>, rows: Vec<StoredEvent>) -> AppendOutcome {
    match tx.commit().await {
        Ok(()) => AppendOutcome::Committed(rows),
        Err(sqlx::Error::Database(e))
            if e.try_downcast_ref::<PgDatabaseError>()
                .is_some_and(|e| e.severity() == PgSeverity::Error) =>
        {
            AppendOutcome::Unavailable
        }
        Err(_) => AppendOutcome::Ambiguous,
    }
}

/// A race the append could not see when it started, so a rerun decides it: the same `event_id`
/// committed meanwhile (unique violation), a serialization failure, or a deadlock.
fn lost_a_race(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| matches!(code.as_ref(), "23505" | "40001" | "40P01"))
}

/// §11 checks 1 to 5 over `rows` from `start`. Artifact checks (6 and 7) need the artifact store,
/// so an artifact failure is passed over and the walk resumes after that event.
fn check(stream_id: &str, rows: &[StoredEvent], start: TrustedStart) -> Result<(), IntegrityError> {
    let no_artifacts: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    let (mut rest, mut start) = (rows, start);
    loop {
        let failure = match verify_events(rest, start, &no_artifacts) {
            Ok(_) => return Ok(()),
            Err(failure) => failure,
        };
        match failure.check {
            EventCheck::ArtifactMissing | EventCheck::ArtifactMismatch => {}
            EventCheck::NonCanonical
            | EventCheck::ColumnMismatch
            | EventCheck::SeqGap
            | EventCheck::RehashMismatch
            | EventCheck::PrevHashMismatch => {
                return Err(IntegrityError {
                    stream_id: stream_id.to_owned(),
                    failure,
                });
            }
        }
        let Some(at) = rest.iter().position(|r| r.seq == failure.seq) else {
            return Ok(());
        };
        let (passed, after) = rest.split_at(at.saturating_add(1));
        let Some(last) = passed.last() else {
            return Ok(());
        };
        start = TrustedStart {
            from_seq: failure.seq.saturating_add(1),
            prev_hash: last.hash,
        };
        rest = after;
    }
}

/// §11 checks 1, 2, and 4 on one stored event.
fn check_row(row: &StoredEvent) -> Result<(), IntegrityError> {
    check(
        &row.stream_id,
        std::slice::from_ref(row),
        TrustedStart {
            from_seq: row.seq,
            prev_hash: row.prev_hash,
        },
    )
}

fn decode_event(row: &PgRow) -> Result<StoredEvent, sqlx::Error> {
    Ok(StoredEvent {
        stream_id: row.try_get("stream_id")?,
        seq: unsigned(row.try_get("seq")?)?,
        event_id: row.try_get("event_id")?,
        event_type: row.try_get("event_type")?,
        schema_version: unsigned(row.try_get("schema_version")?)?,
        environment: row.try_get("environment")?,
        recorded_at: row.try_get("recorded_at")?,
        prev_hash: digest(row.try_get("prev_hash")?)?,
        hash: digest(row.try_get("hash")?)?,
        body: row.try_get("body")?,
    })
}

fn decode_head(row: &PgRow) -> Result<StreamHead, sqlx::Error> {
    let risk_clock: Option<String> = row.try_get("risk_clock")?;
    Ok(StreamHead {
        head: Head {
            seq: unsigned(row.try_get("seq")?)?,
            hash: digest(row.try_get("hash")?)?,
            writer_epoch: unsigned(row.try_get("writer_epoch")?)?,
        },
        risk_clock: risk_clock
            .map(|c| UtcNanos::parse(&c).map_err(|e| sqlx::Error::Decode(Box::new(e))))
            .transpose()?,
    })
}

fn digest(bytes: Vec<u8>) -> Result<Digest, sqlx::Error> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|b: Vec<u8>| sqlx::Error::Decode(format!("a {}-byte hash", b.len()).into()))?;
    Ok(Digest::from_bytes(bytes))
}

fn unsigned(value: i64) -> Result<u64, sqlx::Error> {
    u64::try_from(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn signed(value: u64) -> Result<i64, sqlx::Error> {
    i64::try_from(value).map_err(|e| sqlx::Error::Encode(Box::new(e)))
}

/// A statement assembled from this crate's own constants, never from input.
fn sql(text: String) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(text)
}
