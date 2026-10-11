//! The wiring the live run calls the G1b seams through (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b wiring;
//! [DEC-914](../../../docs/project/decisions/DEC-914.md)): the grant wait, the quote
//! cross-check, the watch's LT-10 end and the export evidence are merged pure seams
//! ([`grant`](crate::grant), [`cross_check`](crate::quote), [`lt10`](crate::lt10),
//! [`evidence`](crate::evidence), DEC-912), and this module is the runner's half of each — the
//! inputs a run derives from its own journal rows, its clock, its registered configuration and
//! its report, handed to those seams exactly as the specs state them. The call frames are here
//! with them: the wait ([`wait_for_the_grant`], DEC-914 items 1 and 2) holds the ask's submit,
//! re-reading the control stream per wake; the cross-check ([`cross_check_the_opening`],
//! DEC-914 item 3) reads the collar's own `x` before the intent leaves; the watch's per-wake
//! question ([`watch_may_end`], DEC-914 items 4 and 5) is LT-10 over the facts each wake read,
//! in place of the bare terminal read the paper watch keeps
//! ([DEC-858](../../../docs/project/decisions/DEC-858.md) item 4; `mandate-shell`'s
//! `TERMINAL_STATUSES` check), and the evidence ([`reported`], DEC-914 item 6) attaches to the
//! run's report. The live cycle that drives the three live frames against the shell's own
//! watch and submit doors is the founder's `--features live` build's to walk (DEC-914 item 7:
//! the call sites' order is this PR's to write and its review's to judge); nothing here
//! reaches a host, a credential or an order (`AGENTS.md` rule 8, LT-1): every input arrives as
//! a journaled row, a parsed decimal, a clock instant or a member the run already holds.

use std::time::Duration;

use mandate_alpaca::Pause;
use mandate_canon::Value;
use mandate_domain::AssetClass;
use mandate_journal::StoredEvent;
use mandate_num::{Fraction, Price, Usd};
use mandate_shell::control::ControlRecord;
use mandate_shell::{Cause, ShellError, Stage};
use mandate_time::{ExchangeCalendar, Session, UtcNanos};

use crate::PaperError;
use crate::connection::member;
use crate::evidence::{Evidence, EvidenceRefusal, evidence};
use crate::grant::{Asked, GrantRefusal, grant};
use crate::lt10::{Lt10Refusal, Watched, lt10};
use crate::quote::{QuoteRefusal, cross_check};

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
    let row = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ApprovalRequested")
        .ok_or(GrantRefusal::Mismatched)?;
    let record = record_of(row)?;
    let wait = Asked {
        approval: row.event_id.clone(),
        content_hash: content_hash_of(&record)?,
        deadline: deadline_of(&record)?,
    };
    if now >= wait.deadline {
        return Err(GrantRefusal::TimedOut);
    }
    Ok(wait)
}

/// The wait's fold over the answers (DEC-914 item 2; #1308's review note: the journal read's
/// records reach [`grant`] verbatim): `rows` are the journal's own read (`ORDER BY seq`) of the
/// workspace control stream, each mapped one-to-one to its record — its `seq`, its `event_type`
/// and its body's `payload` member, the same mapping the run's own control-stream read performs
/// — and the records are handed to `grant` **verbatim, in journal order**: never re-sorted by
/// another key, never filtered to the answers alone, no row's members re-read from anywhere but
/// the row's own payload. The first `ApprovalResponseSubmitted` naming the ask's approval
/// decides the wait wherever it sits in the read (#1308: an answer committed at any `seq` while
/// the run waited is folded when the journal hands it), and every refusal is `grant`'s own
/// (DEC-912 item 1), because the release the cycle takes is the one this fold returns and no
/// other.
///
/// # Errors
/// Any [`GrantRefusal`] `grant` returns, and a row whose envelope cannot be read, which is a
/// difference with nothing sent (`AGENTS.md` rule 3): every one of them is a stop with nothing
/// sent.
pub fn answers(rows: &[StoredEvent], ask: &Asked, now: UtcNanos) -> Result<(), GrantRefusal> {
    let records: Vec<ControlRecord> = rows.iter().map(record_of).collect::<Result<_, _>>()?;
    grant(&records, ask, now)
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
/// collar and the cross-check's band. The derivation is read over the typed members exactly as
/// `conduct.rs` reads them over the configuration it was handed, because the runner holds no
/// `mandate-risk` dependency row; the register's own members are what arrive, so no constant
/// can wear the tier's place.
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
    Ok(match asset_class {
        AssetClass::Crypto => collar_crypto_x,
        AssetClass::UsEquity => match median_dollar_volume_20d {
            Some(volume) if volume < collar_liquid_threshold_usd => collar_other_x,
            Some(_) | None => collar_liquid_x,
        },
    })
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
    Ok(Watched {
        entry_terminal: entry_terminal(read),
        uncovered,
        alerted: alerts
            .contains(&"unprotected_interval_limit")
            .then_some("unprotected_interval_limit"),
    })
}

/// DEC-858 item 4's exact four, and nothing else (DEC-914 item 4; #1311's review: the codec's
/// thirteen states, at a watch's own cost): `filled`, `canceled`, `expired` and `rejected` end
/// the entry, and every other state the executor's codec names — `intent`, `submitting`,
/// `accepted`, `partially_filled`, `pending_cancel`, `pending_replace`, `unknown`, `replaced`
/// and `abandoned`, `replaced` and `abandoned` terminal to the executor's own reservation
/// included — leaves the run reading, as does a read absent, an unmapped broker status, or one
/// no codec names: a terminal entry is never a guess (`AGENTS.md` rule 3), and an `unknown`
/// read owes LT-6's lookups while an `Unknown` entry is unresolved.
fn entry_terminal(read: Option<&str>) -> bool {
    match read {
        Some("filled" | "canceled" | "expired" | "rejected") => true,
        Some(
            "intent" | "submitting" | "accepted" | "partially_filled" | "pending_cancel"
            | "pending_replace" | "unknown" | "replaced" | "abandoned",
        )
        | None
        | Some(_) => false,
    }
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
    let calendar = ExchangeCalendar::us_equities().map_err(|_| no_session())?;
    let (today, _) = mandate_time::new_york_date_and_hour(bound).map_err(|_| no_session())?;
    calendar
        .sessions(today)
        .map_err(|_| no_session())?
        .into_iter()
        .find(|span| span.session() == Session::Regular && span.contains(bound))
        .map(|span| span.end())
        .ok_or_else(no_session)
}

/// The cap's fail-closed refusal: the bound's own regular session, named as the one thing no cap
/// can be placed without (FT-8, `AGENTS.md` rule 3).
fn no_session() -> Cause {
    Cause::Absent {
        what: "the regular session the watch's bound is in",
    }
}

/// The report's evidence (DEC-914 item 6; DEC-912 item 4; the founder's run's steps 4 to 6;
/// DEC-157 item 6): the exportable streams named **at the run's report**, from the journal the
/// run **appended to** — the arguments' journal held only while placing (DEC-157 item 6,
/// `run()`'s own `args.journal.clone().filter(|_| args.place_one_order)`), so a run without
/// `--place-one-order` wrote nothing a `journal export` could read and names no stream at all,
/// the DSN at hand or not (R0 step 4's dry run exports nothing; [`EvidenceRefusal::JournalAbsent`]
/// at the report seam) — and a placing run names the three streams of the founder's run's step
/// 6, in its order — `agent:{workspace}:{agent}`, then `acct:{workspace}:{account_ref}`, then
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
    match (journal, place_one_order) {
        (Some(journal), true) => evidence(Some(journal), workspace, agent, account_ref),
        (None, false) | (None, true) | (Some(_), false) => Err(EvidenceRefusal::JournalAbsent),
    }
}

/// The run-level call frame of the ask's grant wait (E7-26's G1b wiring row; DEC-912 item 1;
/// DEC-914 items 1, 2 and 7): the wait that holds the submit — this frame releases, it never
/// submits, so the cycle's one-order door (FT-1) and the runtime's admission (mandate spec §6.4's
/// checks) still stand in front. The ask is bound once, verbatim off the agent-stream rows
/// [`asked`] reads — the youngest own ask, journaled by this run after its gate passed, and
/// nothing after it can exist while the run holds its one-order door (DEC-914 item 1). The
/// answers are then re-read **per wake**: `control_rows` is the journal's own `ORDER BY seq`
/// read of the workspace control stream, taken again on every wake (#1311's review note: an
/// answer committed after the run began waiting is folded when the journal hands it), and each
/// read's rows reach [`answers`] — and behind it [`grant`] — verbatim, in journal order, over
/// the control stream's records the run's own control-stream read maps (DEC-914 item 2). Each
/// wake pauses `interval` — the watch's own registered poll interval, DEC-853 item 6's "the
/// least of `exit_step_s`; `unknown_absent_window_s` divided by `unknown_absent_lookups`
/// (whole seconds, rounded down, at least 1); `bracket_partial_fill_timeout_s`; and
/// `max_unprotected_s`", the same `poll_interval` of the registered executor members the run's
/// `Watch` holds — so the wait is bounded by the request's own deadline: an ask whose answers
/// never come denies at it and nothing is sent (rule 3), and an answer already committed before
/// the first wake decides it without waiting.
///
/// `agent_rows` are the agent stream's own rows as the journal read them when the wait began;
/// `pause` is the run's clock, read per fold and advanced per wake.
///
/// # Errors
/// Every [`PaperError`] is a stop after which nothing is sent: any [`GrantRefusal`] of the ask
/// or its answers (through [`PaperError::Grant`]), a row source the journal could not answer
/// (through `control_rows`'s own error), and the wait's own stop when the clock the pause keeps
/// cannot run.
pub fn wait_for_the_grant<Rows>(
    agent_rows: &[StoredEvent],
    control_rows: Rows,
    pause: &(impl Pause + ?Sized),
    interval: Duration,
) -> Result<(), PaperError>
where
    Rows: FnMut() -> Result<Vec<StoredEvent>, PaperError>,
{
    let ask = asked(agent_rows, pause.now()).map_err(PaperError::Grant)?;
    let mut control_rows = control_rows;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|_| {
            PaperError::Shell(ShellError::Refused {
                stage: Stage::Executor,
                cause: Cause::Absent {
                    what: "a runtime for the grant wait",
                },
            })
        })?;
    loop {
        let rows = control_rows()?;
        match answers(&rows, &ask, pause.now()) {
            Ok(()) => return Ok(()),
            Err(GrantRefusal::Unanswered) => {}
            Err(refusal) => return Err(PaperError::Grant(refusal)),
        }
        runtime.block_on(pause.pause(interval));
    }
}

/// The registered gate configuration's own collar members (the very members
/// `mandate-risk`'s `conduct.rs` reads its `collar_x` over), bundled so the run-level frame hands
/// the gate's own `x` whole and no caller can drop or reorder one of them (DEC-914 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollarMembers {
    /// The equity tier's liquid member (§9.6: 1%).
    pub liquid_x: Fraction,
    /// The equity tier's other member (§9.6: 2%).
    pub other_x: Fraction,
    /// The crypto member (§9.6: 2%).
    pub crypto_x: Fraction,
    /// The 20-day median dollar volume the liquid tier's boundary is (`≥`, the spec's own sign).
    pub liquid_threshold_usd: Usd,
}

impl CollarMembers {
    /// The gate's own `x` over the registered members ([`collar_x`], the tier `conduct.rs` reads).
    fn x(
        &self,
        asset_class: AssetClass,
        median_dollar_volume_20d: Option<Usd>,
    ) -> Result<Fraction, QuoteRefusal> {
        collar_x(
            self.liquid_x,
            self.other_x,
            self.crypto_x,
            self.liquid_threshold_usd,
            asset_class,
            median_dollar_volume_20d,
        )
    }
}

/// The run-level call frame of the opening's cross-check (E7-26's G1b wiring row; DEC-529 item
/// 12; trading-domain spec §4.2's cross-checked row; DEC-912 item 2; DEC-914 items 3 and 7): the
/// check that runs **before the intent leaves** — its caller holds the one opening's submission
/// on this answer and sends nothing on its refusal ("or the opening is refused and nothing is
/// sent"). The collar's own `x` is read first, over the registered gate configuration's own
/// members exactly as [`collar_x`] binds them (#1308's review note (c): never a second number),
/// and the two asks — the broker's own quote, the side a buy opening prices against, and the
/// independent IEX quote the opening already requires — are compared within it
/// ([`cross_check`]), exactly the order the pin `the_cross_check_reads_the_collars_own_x_not_a_constant`
/// binds. Exits and protective orders never wait on this check (rule 13): the runner's one
/// opening is the only order it reaches, and this frame is the one call the opening's submit
/// waits for.
///
/// # Errors
/// Any [`QuoteRefusal`] through [`PaperError::CrossCheck`] — a missing quote or a broker
/// disagreement beyond the collar — each a stop before any submission, with nothing sent.
pub fn cross_check_the_opening(
    collar: &CollarMembers,
    asset_class: AssetClass,
    median_dollar_volume_20d: Option<Usd>,
    broker_ask: Option<Price>,
    independent_ask: Option<Price>,
) -> Result<(), PaperError> {
    let x = collar
        .x(asset_class, median_dollar_volume_20d)
        .map_err(PaperError::CrossCheck)?;
    cross_check(broker_ask, independent_ask, x).map_err(PaperError::CrossCheck)
}

/// The run-level call frame of the watch's end (E7-26's G1b wiring row; LT-10; DEC-912 item 3;
/// DEC-914 items 4, 5 and 7): the question the live watch asks at every wake in place of the
/// bare terminal read the paper watch keeps (`mandate-shell`'s `TERMINAL_STATUSES` check,
/// DEC-858 item 4) — may the run end at `now`? The cap is read once over the watch's bound
/// ([`session_cap`], DEC-877 item 1's instant: the end of the regular session the bound is in,
/// the same instant the paper watch fails at), and each wake's facts — the entry's last read,
/// the executor's uncovered answer and the run's report's own alert keys ([`watched`]) — go to
/// [`lt10`], so a watch that is never both terminal and covered (or uncovered with its own
/// interval alert key) keeps reading, fails closed past the cap and never reads again, and
/// ends clean when its LT-10 holds at or past it, needing no further read.
///
/// # Errors
/// [`PaperError::Lt10`] while the run keeps reading (unsettled) and past its cap (fail closed),
/// and [`PaperError::Shell`] when the calendar cannot place the bound's regular session (FT-8).
pub fn watch_may_end(
    read: Option<&str>,
    uncovered: bool,
    alerts: &[&'static str],
    now: UtcNanos,
    bound: UtcNanos,
) -> Result<(), PaperError> {
    let cap = session_cap(bound).map_err(|cause| {
        PaperError::Shell(ShellError::Refused {
            stage: Stage::Executor,
            cause,
        })
    })?;
    let facts = watched(read, uncovered, alerts)?;
    lt10(&facts, now, cap).map_err(PaperError::Lt10)
}

/// One row's record, mapped one-to-one as the run's own control-stream read maps it (DEC-914
/// item 2; `run()`'s `control_record`): its `seq`, its `event_type` and its body's `payload`
/// member, and no other member re-read from anywhere but the row's own body — a row whose
/// envelope cannot be parsed or payload cannot be read holds members the run cannot read, so it
/// refuses as a difference and nothing is sent (`AGENTS.md` rule 3).
fn record_of(row: &StoredEvent) -> Result<ControlRecord, GrantRefusal> {
    let body = mandate_canon::parse(&row.body).map_err(|_| GrantRefusal::Mismatched)?;
    let Some(payload) = body.get("payload") else {
        return Err(GrantRefusal::Mismatched);
    };
    Ok(ControlRecord {
        seq: row.seq,
        event_type: row.event_type.clone(),
        payload: payload.clone(),
    })
}

/// The ask's content hash, off the row's own payload member through the connection record's
/// own text reader ([#1311](https://github.com/kunwarshivam/mandate/issues/1311)'s reserved
/// choice: `member` is `pub(crate)` because this wiring reads through it): the recorded member
/// names the order the approver was shown, and a payload missing it or holding not text is
/// never a guess (rule 3) — a recomputation over the content would bind another order than the
/// one the approver granted, so it remains unread here (LT-4, #1308's review note (b)).
fn content_hash_of(record: &ControlRecord) -> Result<String, GrantRefusal> {
    member(record, "content_hash")
        .map(str::to_owned)
        .ok_or(GrantRefusal::Mismatched)
}

/// The ask's deadline, the payload's own `deadline` member in risk-clock seconds (journal spec
/// §9.6: `integer` seconds; mandate spec §6.4's request deadline), never a text a clock could
/// guess a unit for: a member that is not the integer seconds the request carries refuses as a
/// difference (rule 3).
fn deadline_of(record: &ControlRecord) -> Result<UtcNanos, GrantRefusal> {
    let seconds = record
        .payload
        .get("deadline")
        .and_then(Value::as_int)
        .and_then(|seconds| i64::try_from(seconds).ok())
        .ok_or(GrantRefusal::Mismatched)?;
    UtcNanos::from_parts(seconds, 0).map_err(|_| GrantRefusal::Mismatched)
}
