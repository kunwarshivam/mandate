//! The live door in the run (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1a wiring row — "the G1a live door
//! compile-in" of the #1308 review's R0-gap list and the work tracker's M6 row): the run-level
//! frames the runner calls the merged connection seams ([`connection`](crate::connection),
//! [`lapse`](crate::lapse), #1301, DEC-910) through, implemented since this row closed its pins.
//! Each frame is the seam's own call with the run's error variant wrapped around its refusal
//! (DEC-918 item 1), and the call sites inside `run()` — `chosen` ahead of the shell's own
//! fold and every credential read, `account_stream` and `unspent` before anything runs — are
//! [`run`'s`](crate::run) own, judged against DEC-910's sentences and DEC-918's readings. The
//! adjacent rows keep their own contracts: the G1b frames' call sites stay stream L / M7's
//! remainder's, the shell watch's own loop stays the shell row's, and the live arm's connector,
//! preflight and account-stream journal read stay the live arm's under these signatures. Nothing
//! here reaches a host, a credential or a broker (LT-1, `AGENTS.md` rules 7 and 8): every input
//! arrives as a control-stream record, a journaled row, a parsed mandate or the run's own
//! identifiers.

use mandate_domain::Environment;
use mandate_journal::{StoredEvent, StreamId};
use mandate_journal_pg::PgJournal;
use mandate_num::SignedQty;
use mandate_robinhood::AccountSnapshot;
use mandate_shell::control::ControlRecord;
use mandate_shell::{Cause, ShellError, Stage};
use mandate_spec::Mandate;

use crate::PaperError;
use crate::connection::{Connection, Held, LapseRefusal, connection, lapse};

/// The connection the run uses (DEC-918 item 1; DEC-910 item 2; the task file's row G1a:
/// "The connector chosen from the connection record and the environment from the confirmed
/// mandate"): the connector chosen from the connection record `records` hold for the
/// `mandate`'s connection, the environment the mandate itself states, and the account stream
/// the record must bind. `records` are the run's own control-stream read, handed verbatim and
/// in the journal's order, never a re-read behind a second query; `account_ref` is the run's own
/// (`--account-ref`), the stream every record of this connection must bind or refuse
/// `connection_account`. Every refusal of the door reaches the run as
/// [`PaperError::Connection`](crate::PaperError::Connection) — the transparent variant the
/// merged suite pins — so the run's report names the door's own refusal ahead of the shell's
/// own `DeploymentRefusal::Live` and every credential read (FT-3; R0's step 4: "the exact
/// refusal").
///
/// # Errors
/// Any [`PaperError::Connection`] that applies, before anything runs.
pub fn chosen(
    records: &[ControlRecord],
    mandate: &Mandate,
    account_ref: &str,
) -> Result<Connection, PaperError> {
    connection(records, mandate, account_ref).map_err(PaperError::Connection)
}

/// The account stream whose rows the door's lapse reads (DEC-918 item 3; DEC-910 item 3:
/// "The runner reads the account stream through the same journal it reads the control stream
/// from"): journal spec §2's `acct:{workspace_id}:{account_ref}`, derived from the run's own
/// workspace and account ref — never a constant where the run's own names belong, the
/// derivation discipline DEC-914 item 6 gave the export names. The stream the record binds and
/// the lapse reads is this one alone; a form §2 cannot hold (a workspace or ref whose segments
/// the journal's stream ids refuse) is no stream a run could read, so the naming fails closed
/// with `Cause::Absent` at the reconcile stage, where the run's own account truth is reconciled
/// — never a fallback onto another stream the id could be smashed into (`AGENTS.md` rule 3).
/// A run that kept no journal has no account stream either — its control-stream read already
/// refused one — so the naming invents no refusal of its own.
///
/// # Errors
/// [`PaperError::Shell`] with `Cause::Absent` when the journal's §2 forms cannot parse the
/// stream the run's own names name.
pub fn account_stream(workspace: &str, account_ref: &str) -> Result<StreamId, PaperError> {
    StreamId::parse(&format!("acct:{workspace}:{account_ref}")).ok_or(PaperError::Shell(
        ShellError::Refused {
            stage: Stage::Reconcile,
            cause: Cause::Absent {
                what: "the account stream the connection binds",
            },
        },
    ))
}

/// The flat-and-unspent half of the door (DEC-918 item 4; DEC-529 item 3; DEC-910 item 3):
/// DEC-529 item 3's lapse, wrapped for the run — a live run refuses to start unless the account
/// the connection binds is flat and the record is unspent, before anything runs, and a paper run
/// makes no check (LT-14). `rows` are the journal's own read of the account stream
/// [`account_stream`] names — the same journal the control stream came from, ungated by
/// `--place-one-order` (DEC-529 item 3's lapse refuses a spent record before anything runs, R0
/// step 4's dry run included, where the evidence's placing filter names nothing because the run
/// appended to nothing); `held` counts what the connector's snapshot listed (DEC-470 item 1),
/// handed by the live arm's preflight, re-derived by nothing here. Every refusal reaches the
/// run as [`PaperError::Lapse`](crate::PaperError::Lapse).
///
/// # Errors
/// Any [`PaperError::Lapse`] that applies — the account not flat, or a fill on the bound
/// account stream spending the record — before anything runs.
pub fn unspent(
    connection: &Connection,
    rows: &[StoredEvent],
    held: &Held,
) -> Result<(), PaperError> {
    lapse(connection, rows, held).map_err(PaperError::Lapse)
}

/// The lapse over its real inputs (DEC-933 item 3; DEC-529 item 3; DEC-918 item 1): a live
/// connection's `held` is counted from the agentic account's own snapshot (DEC-470 item 1: every
/// position record, a zero quantity included, and every working order), and its `rows` are the
/// journal's own read of `stream`, the account stream [`account_stream`] named, through the run's
/// own journal (`journal`, ungated by `--place-one-order`, DEC-918 item 2); a run that kept no
/// journal has no account stream and reads no rows. A live connection with no snapshot is
/// [`unsupplied`]'s refusal, since a lapse with nothing to count cannot be passed (`AGENTS.md`
/// rule 3). A paper connection makes no check and reads nothing (LT-14).
///
/// # Errors
/// [`PaperError::Lapse`] as [`unspent`] answers, and `NotSupplied` as [`unsupplied`] answers
/// for a live connection with no snapshot; [`PaperError::Shell`] with `Cause::Absent` at the
/// reconcile stage when its account stream cannot be read.
pub fn lapse_read(
    connection: &Connection,
    journal: Option<&str>,
    stream: &StreamId,
    snapshot: Option<&AccountSnapshot>,
) -> Result<(), PaperError> {
    if connection.environment != Environment::Live {
        return Ok(());
    }
    let Some(snapshot) = snapshot else {
        return unsupplied(connection);
    };
    let rows = account_rows(journal, stream)?;
    unspent(connection, &rows, &held(snapshot))
}

/// What the snapshot lists, counted (DEC-470 item 1): every position record of a non-zero
/// quantity, long or short, and every working order, re-derived by nothing else (DEC-918 item 1).
/// A row of quantity exactly zero is a sold-out position the broker still lists, and is flat
/// ([DEC-2390](../../../docs/project/decisions/DEC-2390.md) item 1, the founder's).
fn held(snapshot: &AccountSnapshot) -> Held {
    Held {
        positions: snapshot
            .positions
            .iter()
            .filter(|position| position.qty != SignedQty::ZERO)
            .count(),
        open_orders: snapshot.open_orders.len(),
    }
}

/// `stream`'s rows from the journal at `journal`, in `seq` order, or none for a run that kept no
/// journal. The error names no DSN (`AGENTS.md` rule 7).
fn account_rows(journal: Option<&str>, stream: &StreamId) -> Result<Vec<StoredEvent>, PaperError> {
    let Some(dsn) = journal else {
        return Ok(Vec::new());
    };
    let unread = || reconcile_absent("the account stream's rows");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| unread())?;
    runtime.block_on(async {
        let journal = PgJournal::from_dsn(dsn).map_err(|_| unread())?;
        journal.rows(stream).await.map_err(|_| unread())
    })
}

fn reconcile_absent(what: &'static str) -> PaperError {
    PaperError::Shell(ShellError::Refused {
        stage: Stage::Reconcile,
        cause: Cause::Absent { what },
    })
}

/// The lapse where the run holds no evidence for it (DEC-919; DEC-529 item 3; DEC-910 item 3):
/// the door's call until the live arm hands [`unspent`] the journal's own read of the account
/// stream and the connector snapshot's own counts. A live connection refuses to start with
/// [`LapseRefusal::NotSupplied`](crate::LapseRefusal::NotSupplied): no read and no snapshot, so
/// nothing shows the account flat or the record unspent, where an empty read with zero counts
/// would pass a spent or non-flat account silently (`AGENTS.md` rule 3). A paper connection
/// makes no check, exactly as [`unspent`] makes none on one (LT-14), so today's paper path is
/// unchanged. The only way past the refusal is the live arm's own call of [`unspent`] with the
/// inputs its reads produced.
///
/// # Errors
/// [`PaperError::Lapse`] with [`LapseRefusal::NotSupplied`](crate::LapseRefusal::NotSupplied)
/// for a live connection, before anything runs.
pub fn unsupplied(connection: &Connection) -> Result<(), PaperError> {
    if connection.environment == Environment::Live {
        return Err(PaperError::Lapse(LapseRefusal::NotSupplied));
    }
    Ok(())
}
