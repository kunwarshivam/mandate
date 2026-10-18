//! The connect check of a paper Alpaca connection (DEC-2007 item 4; connections spec §8.1 checks
//! 1, 2, 3 and 7; journal spec §9.8): the connect sequence's executor part, which `mandate
//! connection record --checked` then binds (DEC-1171). As `mandate-paper`'s Robinhood check does,
//! it reads the account through the broker, runs [`mandate_connections::checks::run`] over what it
//! read, and appends `StreamOpened` (a new stream only) and `ConnectionChecked` (occasion
//! `connect`) on `acct:{workspace}:{account_ref}` in one batch. A failed check is journaled too.
//!
//! It takes the account stream's writer epoch, so it fences an executor already running there,
//! which the restart then replays; it runs before an executor is started for the account. The
//! account's number is never read: the record holds the opaque `account_pii_ref` it is given, and
//! names it in the envelope's `pii_refs` (rule 62).

use mandate_canon::{Object, Value, is_ulid};
use mandate_connections::checks::{
    AccountRead, CheckInput, CheckReport, Granted, Occasion, Outcome, run,
};
use mandate_connections::record::{
    AccountPiiRef, AuthKind, Broker as Venue, ConnectionId, Environment as Reach,
};
use mandate_executor::{AccountScope, BrokerOutcome, BrokerRequest, IdGen, Seq, WriterEpoch};
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes_with_pii, object, text};
use mandate_time::UtcNanos;

use crate::{Broker, VERSION};

/// What a connect check wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The `ConnectionChecked` record's event id, which `connection record --checked` names.
    pub event_id: String,
    /// Whether every check passed.
    pub passed: bool,
}

/// Runs the connect check of `connection` for the account in `scope` at `now`, reading the account
/// through `broker` and naming it by `pii_ref`.
///
/// # Errors
/// A stable code: `connection_id_invalid`, `pii_ref_invalid` for a reference that is not `pii_` and
/// a ULID (journal spec §6.4), `stream_unavailable` when the account
/// stream cannot be taken or read, `check_refused` when the checks cannot run, and
/// `append_refused` when the journal does not commit the batch.
pub fn connect_check<S: Store, B: Broker>(
    store: &mut S,
    broker: &mut B,
    ids: &dyn IdGen,
    scope: &AccountScope,
    connection: &str,
    pii_ref: &str,
    now: UtcNanos,
) -> Result<Checked, &'static str> {
    let _ = (store, broker, ids, scope, connection, pii_ref, now);
    Err("unimplemented")
}
