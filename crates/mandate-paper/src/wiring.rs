//! The wiring the live run calls the G1b seams through (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b wiring;
//! [DEC-914](../../../docs/project/decisions/DEC-914.md)): the grant wait, the quote
//! cross-check, the watch's LT-10 end and the export evidence are merged pure seams
//! ([`grant`](crate::grant), [`cross_check`](crate::quote), [`lt10`](crate::lt10),
//! [`evidence`](crate::evidence), DEC-912), and this module is the runner's half of each — the
//! inputs a run derives from its own journal rows, its clock, its registered configuration and
//! its report, handed to those seams exactly as the specs state them. The call sites themselves
//! — the wait inside the cycle's `ask`, the cross-check before the intent leaves, the LT-10 end
//! in place of the watch's bare terminal read, the evidence at the run's report — are the
//! wiring implementation's
//! ([DEC-77](../../../docs/project/04-decision-log.md#decisions), #1308's Not done); these
//! seams bind what each call site hands and the answers it must take. No host, no credential
//! and no order is reached from here (`AGENTS.md` rule 8, LT-1): every input arrives as a
//! journaled row, a parsed decimal, a clock instant or a member the run already holds.

use mandate_domain::AssetClass;
use mandate_journal::StoredEvent;
use mandate_num::{Fraction, Usd};
use mandate_shell::Cause;
use mandate_time::UtcNanos;

use crate::evidence::{Evidence, EvidenceRefusal};
use crate::grant::{Asked, GrantRefusal};
use crate::lt10::{Lt10Refusal, Watched};
use crate::quote::QuoteRefusal;

/// The `ask` the run waits on, read verbatim off the `ApprovalRequested` its own cycle journaled
/// (DEC-914 item 1; DEC-912 item 1; mandate spec §6.4; journal spec §9.6): `rows` are the
/// journal's own read (`ORDER BY seq`) of the agent stream the run appends to, and the ask the
/// wait serves is the **youngest** `ApprovalRequested` among them — the run journaled it after
/// the gate passed, and nothing after it can exist while the run holds the cycle's one-order
/// door (FT-1). Its members are the row's own: the approval is the row's event id (the only
/// approval whose answers the wait reads), the content hash is the payload's `content_hash`
/// member — **never recomputed** over the row's content object or over any order the cycle
/// rebuilt, so a drifted or rebuilt order never releases a submit, for only the exact order the
/// approver was shown hashes to the member the answers repeat (LT-4) — and the deadline is the
/// payload's `deadline` seconds. A member the run cannot read is never a guess (`AGENTS.md`
/// rule 3): an ask that cannot be bound, and a stream holding no `ApprovalRequested` at all,
/// refuse as a difference with nothing sent, and the youngest ask whose deadline is already at
/// or past `now` denies by its own timeout, for the timeout is a deny (rule 3) and the wait is
/// bounded by the request's own deadline.
///
/// # Errors
/// Any [`GrantRefusal`] that applies; every one of them is a stop with nothing sent.
pub fn asked(rows: &[StoredEvent], now: UtcNanos) -> Result<Asked, GrantRefusal> {
    let _ = (rows, now);
    Err(GrantRefusal::Unimplemented { story: "E7-26" })
}

/// The wait's fold over the answers (DEC-914 item 2; #1308's review note: the journal read's
/// records reach [`grant`](crate::grant) verbatim): `rows` are the journal's own read
/// (`ORDER BY seq`) of the workspace control stream, each mapped one-to-one to its record — its
/// `seq`, its `event_type` and its body's `payload` member, the same mapping the run's own
/// control-stream read performs — and the records are handed to `grant` **verbatim, in journal
/// order**: never re-sorted by another key, never filtered to the answers alone, no row's
/// members re-read from anywhere but the row's own payload. The first
/// `ApprovalResponseSubmitted` naming the ask's approval decides the wait wherever it sits in
/// the read (#1308: an answer committed at any `seq` while the run waited is folded when the
/// journal hands it), and every refusal is `grant`'s own (DEC-912 item 1), because the release
/// the cycle takes is the one this fold returns and no other.
///
/// # Errors
/// Any [`GrantRefusal`] `grant` returns; every one of them is a stop with nothing sent.
pub fn answers(rows: &[StoredEvent], ask: &Asked, now: UtcNanos) -> Result<(), GrantRefusal> {
    let _ = (rows, ask, now);
    Err(GrantRefusal::Unimplemented { story: "E7-26" })
}

/// The collar's own `x` for the opening's cross-check (DEC-914 item 3; DEC-529 item 12;
/// trading-domain spec §9.6's tier row; #1308's review note: the gate's own `collar_x`,
/// `mandate-risk`'s `conduct.rs`, never a second number): the aggressiveness of the very collar
/// the gate read — the crypto member for crypto, and for an equity the liquid member when the
/// 20-day median dollar volume is **at least** the threshold (the spec's own `≥`), the other
/// member when it is known and below it, and the narrower liquid member when it is unknown, so
/// the ambiguous case admits fewer prices, not more (DEC-163 item 2; DEC-912 item 2). The
/// members are the registered gate configuration's own, arriving typed and validated before
/// the run, and the one derivation that reads them stays the gate's: this seam names what the
/// cross-check reads, it invents no tolerance, so no second number can sit between the gate's
/// collar and the cross-check's band.
///
/// # Errors
/// The stub's own report only (DEC-137): every member is typed and already refused the run when
/// it could not be read, so the implemented seam refuses nothing.
pub fn collar_x(
    collar_liquid_x: Fraction,
    collar_other_x: Fraction,
    collar_crypto_x: Fraction,
    collar_liquid_threshold_usd: Usd,
    asset_class: AssetClass,
    median_dollar_volume_20d: Option<Usd>,
) -> Result<Fraction, QuoteRefusal> {
    let _ = (
        collar_liquid_x,
        collar_other_x,
        collar_crypto_x,
        collar_liquid_threshold_usd,
        asset_class,
        median_dollar_volume_20d,
    );
    Err(QuoteRefusal::Unimplemented { story: "E7-26" })
}

/// The `Watched` the run's LT-10 question reads (DEC-914 item 4; DEC-912 item 3; DEC-858 item 4):
/// `read` is the entry's last broker read as the watch handed it to the executor, and the entry
/// is terminal only in the four states the watch ends on — `filled`, `canceled`, `expired` or
/// `rejected` — so a read that is absent, unknown or still working leaves the run reading
/// (`AGENTS.md` rule 3). `uncovered` is the executor's answer: whether a filled quantity is
/// left with no protection resting over the whole of it. `alerts` are the run's report's own
/// keys, and the interval is reported only when they hold **its own** alert key,
/// `unprotected_interval_limit` (notifications spec §4.2's `protection` row) — DEC-912 item 3:
/// "what the seam observes is the run's report holding the interval's alert key" — never a key
/// synthesized from the rows and never another alarm's (`stop_watchdog` is another alarm's and
/// shows nothing for this interval).
///
/// # Errors
/// The stub's own report only (DEC-137): every input decides a member, so the implemented seam
/// refuses nothing.
pub fn watched(
    read: Option<&str>,
    uncovered: bool,
    alerts: &[&'static str],
) -> Result<Watched, Lt10Refusal> {
    let _ = (read, uncovered, alerts);
    Err(Lt10Refusal::Unimplemented { story: "E7-26" })
}

/// The watch's cap (DEC-914 item 5; DEC-877 item 1; DEC-853 item 5): the end of the regular
/// session the watch's `bound` is in, on the same `mandate_time::ExchangeCalendar` the
/// close-window bound read — 16:00 New York on a full day, the early close on a short one — so
/// the live watch's reads fail at the same instant the paper watch's do and the run can never
/// outlive the day. A bound no regular session holds refuses ([`Cause::Absent`]): the
/// close-window bound already refused such a run (FT-8), so this stop only fails closed again,
/// and the run reads nothing past a cap it cannot place.
///
/// # Errors
/// [`Cause::Absent`] when the calendar cannot place the bound's regular session.
pub fn session_cap(bound: UtcNanos) -> Result<UtcNanos, Cause> {
    let _ = bound;
    Err(Cause::Unimplemented { story: "E7-26" })
}

/// The report's evidence (DEC-914 item 6; DEC-912 item 4; the founder's run's steps 4 to 6;
/// DEC-157 item 6): the exportable streams named **at the run's report**, from the journal the
/// run **appended to** — the arguments' journal held only while placing, so a run without
/// `--place-one-order` wrote nothing a `journal export` could read and names no stream at all,
/// the DSN at hand or not (R0 step 4's dry run exports nothing; [`EvidenceRefusal::JournalAbsent`]
/// at the report seam). A placing run names the three streams of the founder's run's step 6, in
/// its order — `agent:{workspace}:{agent}`, then `acct:{workspace}:{account_ref}`, then
/// `ctl:{workspace}` — derived from the run's own workspace, agent and account ref (the typed
/// ids the cycle's own sources carry, #1308's review note), so they parse as journal spec §2
/// stream ids of their types and no constant can stand where they belong. The runner writes no
/// export itself (journal spec §9.13 journals one before it is served, the CLI's door), and no
/// name or refusal here names the journal's DSN (rule 7).
///
/// # Errors
/// [`EvidenceRefusal::JournalAbsent`] when the run appended to no journal.
pub fn reported(
    journal: Option<&str>,
    place_one_order: bool,
    workspace: &str,
    agent: &str,
    account_ref: &str,
) -> Result<Evidence, EvidenceRefusal> {
    let _ = (journal, place_one_order, workspace, agent, account_ref);
    Err(EvidenceRefusal::Unimplemented { story: "E7-26" })
}
