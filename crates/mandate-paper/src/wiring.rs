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

use mandate_alpaca::{LatestQuote, Pause};
use mandate_canon::Value;
use mandate_domain::AssetClass;
use mandate_journal::StoredEvent;
use mandate_num::{Fraction, Price, Usd};
use mandate_robinhood::Quote;
use mandate_shell::control::{ControlRecord, Origin};
use mandate_shell::{AskedCycle, Cause, Mark, ShellError, Stage};
use mandate_time::{ExchangeCalendar, Session, UtcNanos};

use crate::PaperError;
use crate::ask::BrokerAsk;
use crate::connection::member;
use crate::evidence::{Evidence, EvidenceRefusal, evidence};
use crate::grant::{Asked, GrantRefusal, Journaled, grant};
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

/// The wait's fold over the answers and the runtime's records of them (DEC-914 item 2;
/// [DEC-920](../../../docs/project/decisions/DEC-920.md) items 1 and 4; #1308's review note: the
/// journal read's records reach [`grant`] verbatim): `control_rows` are the journal's own read
/// (`ORDER BY seq`) of the workspace control stream and `agent_rows` its read of the agent stream
/// the run appends to — where the runtime journals its `ApprovalResponded` and
/// `ApprovalRevalidated` — each row mapped one-to-one to its [`Journaled`]: its event id, its
/// envelope's `causation_id`, and its record — its `seq`, its `event_type` and its body's
/// `payload` member, the same mapping the run's own control-stream read performs. The rows are
/// handed to `grant` **verbatim, in journal order**: never re-sorted by another key, never
/// filtered to the answers alone, no row's members re-read from anywhere but the row's own body.
/// The release is `grant`'s own (DEC-912 item 1): the runtime's admitted, re-validated grant of
/// the ask's first deciding answer, never a raw answer, because the release the cycle takes is
/// the one this fold returns and no other.
///
/// A row whose body cannot be read is never a guess (`AGENTS.md` rule 3), and the refusal is
/// scoped by the row's own `event_type` column (DEC-920 item 4): a row of a type the fold reads —
/// `ApprovalResponseSubmitted` on the control stream, `ApprovalResponded` and
/// `ApprovalRevalidated` on the agent stream — could be this ask's, so it refuses as a difference
/// with nothing sent; a row of any other type decides nothing in the fold whatever its body
/// holds, so it is handed in its place with a `null` payload and no `causation_id`, and one
/// unreadable unrelated row never refuses every ask behind it.
///
/// # Errors
/// Any [`GrantRefusal`] `grant` returns, and a row of a folded type whose body cannot be read,
/// which is a difference with nothing sent: every one of them is a stop with nothing sent.
pub fn answers(
    control_rows: &[StoredEvent],
    agent_rows: &[StoredEvent],
    ask: &Asked,
    now: UtcNanos,
) -> Result<(), GrantRefusal> {
    let control = journaled(control_rows, &["ApprovalResponseSubmitted"])?;
    let agent = journaled(agent_rows, &["ApprovalResponded", "ApprovalRevalidated"])?;
    grant(&control, &agent, ask, now)
}

/// One wake's read of the two streams the grant wait folds (DEC-914 item 2;
/// [DEC-920](../../../docs/project/decisions/DEC-920.md) item 1): the journal's own
/// `ORDER BY seq` read of the agent stream the run appends to, where the runtime's admission
/// records land, and of the workspace control stream, where the answers land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeRead {
    /// The agent stream's rows.
    pub agent: Vec<StoredEvent>,
    /// The workspace control stream's rows.
    pub control: Vec<StoredEvent>,
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
/// answers and the runtime's records of them are then re-read **per wake**: `reads` returns the
/// journal's own `ORDER BY seq` read of both streams ([`WakeRead`]), taken again on every wake
/// (#1311's review note: an answer committed, or admitted, after the run began waiting is folded
/// when the journal hands it), and each read's rows reach [`answers`] — and behind it [`grant`]
/// — verbatim, in journal order (DEC-914 item 2), so the wait releases only once the runtime has
/// admitted and re-validated the grant (DEC-920 item 1) and a raw answer alone holds it. Each
/// wake pauses `interval` — the watch's own registered poll interval, DEC-853 item 6's "the
/// least of `exit_step_s`; `unknown_absent_window_s` divided by `unknown_absent_lookups`
/// (whole seconds, rounded down, at least 1); `bracket_partial_fill_timeout_s`; and
/// `max_unprotected_s`", the same `poll_interval` of the registered executor members the run's
/// `Watch` holds — so the wait is bounded by the request's own deadline: an ask whose answers
/// never come denies at it and nothing is sent (rule 3), and an answer already committed before
/// the first wake decides it without waiting.
///
/// `agent_rows` are the agent stream's own rows as the journal read them when the wait began,
/// which bind the ask; `pause` is the run's clock, read per fold and advanced per wake.
///
/// # Errors
/// Every [`PaperError`] is a stop after which nothing is sent: any [`GrantRefusal`] of the ask
/// or its answers (through [`PaperError::Grant`]), a row source the journal could not answer
/// (through `reads`'s own error), and the wait's own stop when the clock the pause keeps
/// cannot run.
pub fn wait_for_the_grant<Reads>(
    agent_rows: &[StoredEvent],
    reads: Reads,
    pause: &(impl Pause + ?Sized),
    interval: Duration,
) -> Result<(), PaperError>
where
    Reads: FnMut() -> Result<WakeRead, PaperError>,
{
    let ask = asked(agent_rows, pause.now()).map_err(PaperError::Grant)?;
    let mut reads = reads;
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
        let read = reads()?;
        let now = pause.now();
        match answers(&read.control, &read.agent, &ask, now) {
            Ok(()) => return Ok(()),
            Err(GrantRefusal::Unanswered) => {}
            Err(refusal) => return Err(PaperError::Grant(refusal)),
        }
        runtime.block_on(pause.pause(interval.min(until(now, ask.deadline))));
    }
}

/// The time from `now` to `deadline`, the wait's last pause (DEC-949 item 2): the wake after it is
/// at the deadline itself, so the runtime is ticked there and closes the ask. `now` is before the
/// deadline whenever the wait pauses — at or after it the fold has already denied — so a
/// `deadline` already reached reads as no time left.
fn until(now: UtcNanos, deadline: UtcNanos) -> Duration {
    let nanos = |at: UtcNanos| {
        i128::from(at.secs())
            .checked_mul(1_000_000_000)
            .and_then(|whole| whole.checked_add(i128::from(at.nanos())))
    };
    nanos(deadline)
        .zip(nanos(now))
        .and_then(|(deadline, now)| deadline.checked_sub(now))
        .and_then(|left| u64::try_from(left).ok())
        .map_or(Duration::ZERO, Duration::from_nanos)
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

/// The live opening's quote cross-check over the preflight's own two quotes (DEC-933 item 2;
/// DEC-529 item 12): `robinhood` is the quote `mandate-robinhood`'s preflight read, the broker's
/// own, and `iex` the latest IEX quote Alpaca's data client read, the independent one. Both must
/// be of one instrument, or there is nothing to compare and the opening refuses as a missing
/// quote; then the two asks go to [`cross_check_the_opening`] as a US equity, within the
/// registered collar's own `x` for the instrument's 20-day median dollar volume.
///
/// # Errors
/// [`PaperError::CrossCheck`]: `quote_absent` for quotes of two instruments, and every refusal of
/// [`cross_check_the_opening`].
pub fn robinhood_cross_check(
    collar: &CollarMembers,
    robinhood: &Quote,
    iex: &LatestQuote,
    median_dollar_volume_20d: Option<Usd>,
) -> Result<(), PaperError> {
    if robinhood.symbol != iex.instrument {
        return Err(PaperError::CrossCheck(QuoteRefusal::Absent));
    }
    cross_check_the_opening(
        collar,
        AssetClass::UsEquity,
        median_dollar_volume_20d,
        Some(robinhood.ask),
        Some(iex.ask),
    )
}

/// The asked opening's cross-check at the check itself
/// ([DEC-945](../../../docs/project/decisions/DEC-945.md) item 3; DEC-529 item 12; DEC-940 item
/// 4): the broker's ask is asked of `broker` now, for the instrument `iex` is of, and held to
/// `max_age` ([`BrokerAsk::ask`]); then it and `iex`'s ask go to [`cross_check_the_opening`]
/// within the registered collar's own `x` for `asset_class` and the 20-day median dollar volume.
/// It runs after the grant and before the one held opening is sent, and refuses with nothing
/// sent.
///
/// # Errors
/// [`PaperError::CrossCheck`]: `quote_absent` or `quote_stale` from the broker's read, and every
/// refusal of [`cross_check_the_opening`].
pub fn broker_cross_check<P: Pause + ?Sized>(
    collar: &CollarMembers,
    asset_class: AssetClass,
    median_dollar_volume_20d: Option<Usd>,
    broker: &BrokerAsk,
    iex: &LatestQuote,
    pause: &P,
    max_age: Duration,
) -> Result<(), PaperError> {
    let ask = broker
        .ask(&iex.instrument, pause, max_age)
        .map_err(PaperError::CrossCheck)?;
    cross_check_the_opening(
        collar,
        asset_class,
        median_dollar_volume_20d,
        Some(ask),
        Some(iex.ask),
    )
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

/// The asked opening's way out of the run (G1b's wiring row; DEC-912 items 1 and 2; DEC-914
/// items 1 to 3 and 7; DEC-920 item 1; DEC-940 items 1 and 4): the production caller of
/// [`wait_for_the_grant`]. The ask is bound off the agent stream as the cycle left it; each wake
/// before the deadline first reads `fresh`, the caller's fresh IEX quote handed to the binding
/// gate and returned as its risk mark (DEC-943 item 2), and a read that fails stops the wait
/// with nothing sent; then the wake is the shell's own ([`AskedCycle::wake`]) — the mark
/// journaled, the control stream tailed whole and handed to the runtime, which journals its
/// `ApprovalResponded` and `ApprovalRevalidated` against that mark, then both streams read back
/// at the clock `pause` keeps — and the wait folds each read through [`answers`], so
/// it releases only on the runtime's admitted, re-validated grant of the first deciding answer.
/// Then `cross_check` — the caller's [`cross_check_the_opening`] over the collar's own `x` —
/// runs before anything leaves, and only then the one held opening is sent
/// ([`AskedCycle::submit`]). A timeout, a deny, a difference, a refused cross-check or a stopped
/// wake each end the run with nothing sent (`AGENTS.md` rule 3); exits and protective orders are
/// never held by the shell, so nothing here can hold one (rule 13). A wake at or past the ask's
/// deadline — the wait's last pause ends at it, and an ask bound already past it takes one — reads
/// no quote and hands the runtime no answer: it ticks the runtime's clock alone
/// ([`AskedCycle::expire`]), so the runtime journals its own `ApprovalTimedOut` (`on_timeout:
/// skip`, caused by the ask), and the fold denies the timeout (DEC-949, closing DEC-940 item 7).
///
/// # Errors
/// Every [`PaperError`] is a stop after which nothing is sent.
pub fn granted_opening<P, F, C>(
    cycle: &mut dyn AskedCycle,
    pause: &P,
    interval: Duration,
    mut fresh: F,
    cross_check: C,
) -> Result<(), PaperError>
where
    P: Pause + ?Sized,
    F: FnMut() -> Result<Mark, PaperError>,
    C: FnOnce() -> Result<(), PaperError>,
{
    let agent_rows = cycle.agent_rows().map_err(PaperError::Shell)?;
    let deadline = match asked(&agent_rows, pause.now()) {
        Ok(ask) => ask.deadline,
        Err(GrantRefusal::TimedOut) => {
            cycle.expire(pause.now()).map_err(PaperError::Shell)?;
            return Err(PaperError::Grant(GrantRefusal::TimedOut));
        }
        Err(refusal) => return Err(PaperError::Grant(refusal)),
    };
    let reads = || {
        let now = pause.now();
        if now >= deadline {
            return cycle
                .expire(now)
                .map(|wake| WakeRead {
                    agent: wake.agent,
                    control: wake.control,
                })
                .map_err(PaperError::Shell);
        }
        let mark = fresh()?;
        cycle
            .wake(now, &mark)
            .map(|wake| WakeRead {
                agent: wake.agent,
                control: wake.control,
            })
            .map_err(PaperError::Shell)
    };
    wait_for_the_grant(&agent_rows, reads, pause, interval)?;
    cross_check()?;
    cycle.submit().map_err(PaperError::Shell)
}

/// The asked run's watch end (LT-10; DEC-914 items 4 and 5; DEC-940 item 5): [`watch_may_end`]
/// — the cap over the bound, then [`watched`] and [`lt10`] — answered in the shell watch's own
/// terms, which the shell asks at every wake of the asked run. A settled LT-10 ends the run
/// (`true`); an unsettled one reads on (`false`); the cap fails closed with the shell's own
/// `cancel_unconfirmed`, exactly as the shell's watch does; a bound no regular session holds
/// stops with the shell's refusal; and any other answer fails closed as the cap does, never a
/// guess at settled (`AGENTS.md` rule 3).
///
/// # Errors
/// The shell's stop for the cap and for a bound no session holds.
pub fn lt10_end(
    read: Option<&str>,
    uncovered: bool,
    alerts: &[&'static str],
    now: UtcNanos,
    bound: UtcNanos,
) -> Result<bool, ShellError> {
    match watch_may_end(read, uncovered, alerts, now, bound) {
        Ok(()) => Ok(true),
        Err(PaperError::Lt10(Lt10Refusal::Unsettled)) => Ok(false),
        Err(PaperError::Shell(stop)) => Err(stop),
        Err(_) => Err(ShellError::CancelUnconfirmed),
    }
}

/// One row's record, mapped one-to-one as the run's own control-stream read maps it (DEC-914
/// item 2; `run()`'s `control_record`): its `seq`, its body's `event_time`, its `event_type` and
/// its body's `payload` member, and no other member re-read from anywhere but the row's own body — a row whose
/// envelope cannot be parsed or payload cannot be read holds members the run cannot read, so it
/// refuses as a difference and nothing is sent (`AGENTS.md` rule 3).
fn record_of(row: &StoredEvent) -> Result<ControlRecord, GrantRefusal> {
    let body = mandate_canon::parse(&row.body).map_err(|_| GrantRefusal::Mismatched)?;
    let Some(payload) = body.get("payload") else {
        return Err(GrantRefusal::Mismatched);
    };
    let origin = Origin::of(&row.stream_id, &row.environment).ok_or(GrantRefusal::Mismatched)?;
    Ok(ControlRecord {
        seq: row.seq,
        event_time: ControlRecord::event_time_of(&body),
        event_type: row.event_type.clone(),
        payload: payload.clone(),
        origin,
    })
}

/// `rows` mapped one-to-one, in their own order, to the [`Journaled`] the grant fold reads
/// ([DEC-920](../../../docs/project/decisions/DEC-920.md) items 1 and 4): each row's event id,
/// its envelope's `causation_id` when the body carries one as text, and its record
/// ([`record_of`]). A row of a `folded` type whose body cannot be read refuses as a difference
/// (`AGENTS.md` rule 3); a row of any other type is handed in its place, its body unread, with a
/// `null` payload and no `causation_id`, because the fold reads nothing of it.
fn journaled(rows: &[StoredEvent], folded: &[&str]) -> Result<Vec<Journaled>, GrantRefusal> {
    rows.iter()
        .map(|row| match journaled_row(row) {
            Ok(journaled) => Ok(journaled),
            Err(refusal) if folded.contains(&row.event_type.as_str()) => Err(refusal),
            Err(refusal) => Ok(Journaled {
                event_id: row.event_id.clone(),
                causation_id: None,
                record: ControlRecord {
                    seq: row.seq,
                    event_time: None,
                    event_type: row.event_type.clone(),
                    payload: Value::Null,
                    origin: Origin::of(&row.stream_id, &row.environment).ok_or(refusal)?,
                },
            }),
        })
        .collect()
}

/// One row's [`Journaled`]: its event id, its envelope's `causation_id` as text or [`None`], and
/// its record ([`record_of`]'s refusals).
fn journaled_row(row: &StoredEvent) -> Result<Journaled, GrantRefusal> {
    let body = mandate_canon::parse(&row.body).map_err(|_| GrantRefusal::Mismatched)?;
    let record = record_of(row)?;
    Ok(Journaled {
        event_id: row.event_id.clone(),
        causation_id: body
            .get("causation_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        record,
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

#[cfg(test)]
mod tests {
    use mandate_canon::Digest;
    use mandate_domain::Environment;
    use mandate_journal::StoredEvent;
    use mandate_shell::control::Origin;

    use super::{GrantRefusal, journaled, record_of};

    fn row(event_type: &str, environment: &str, body: &[u8]) -> StoredEvent {
        StoredEvent {
            stream_id: "ctl:ws_live".to_owned(),
            seq: 2,
            event_id: format!("{:0>26}", 2),
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: environment.to_owned(),
            recorded_at: "2026-10-10T15:05:00Z".to_owned(),
            prev_hash: Digest::of(&[]),
            hash: Digest::of(&[]),
            body: body.to_vec(),
        }
    }

    /// DEC-957 item 4: the grant fold's records carry their row's stream and environment, as the
    /// run's own control read does, and a row naming neither environment refuses as a difference,
    /// whether the fold reads it or hands it in unread.
    #[test]
    fn the_grant_folds_records_carry_their_rows_origin() {
        let live = Origin {
            stream_id: "ctl:ws_live".to_owned(),
            environment: Environment::Live,
        };
        let read = record_of(&row(
            "ApprovalResponseSubmitted",
            "live",
            br#"{"payload":{}}"#,
        ));
        assert_eq!(read.map(|record| record.origin), Ok(live.clone()));
        let unread = journaled(&[row("Other", "live", b"not canonical")], &[]);
        let origins: Result<Vec<Origin>, _> =
            unread.map(|rows| rows.into_iter().map(|row| row.record.origin).collect());
        assert_eq!(origins, Ok(vec![live]));
        let refused = record_of(&row(
            "ApprovalResponseSubmitted",
            "demo",
            br#"{"payload":{}}"#,
        ));
        assert_eq!(refused.err(), Some(GrantRefusal::Mismatched));
        let unread = journaled(&[row("Other", "demo", b"not canonical")], &[]);
        assert_eq!(unread.err(), Some(GrantRefusal::Mismatched));
    }
}
