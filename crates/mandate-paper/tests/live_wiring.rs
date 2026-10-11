//! The live run's wiring (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1b wiring,
//! [DEC-914](../../../docs/project/decisions/DEC-914.md)): the runner's half of the merged
//! seams — the `ask` the run waits on, read verbatim off the journal it appended to; the
//! answers' fold handed to `grant` unchanged, in the journal's own order; the collar's own `x`
//! the cross-check reads; the `Watched` the watch's LT-10 question observes; the cap its reads
//! fail at; and the evidence named at the run's report. Every pending test fails at its stub
//! until E7-26's wiring implementation lands (DEC-77); every fixture is a recorded row or a
//! parsed decimal, holds no credential and reaches no host (ES-19, LT-1, `AGENTS.md` rule 8).

use mandate_canon::{Digest, to_canonical};
use mandate_domain::AssetClass;
use mandate_journal::{StoredEvent, StreamId, StreamType};
use mandate_num::{Fraction, Price, Usd};
use mandate_paper::{
    Asked, Evidence, EvidenceRefusal, GrantRefusal, Lt10Refusal, QuoteRefusal, Watched, answers,
    asked, collar_x, cross_check, lt10, reported, session_cap, watched,
};
use mandate_shell::Cause;
use mandate_time::UtcNanos;

/// The ask's approval: the `ApprovalRequested`'s event id, an opaque id (TI-8).
const APPROVAL: &str = "01JZWTG5EV7Q4ZF0AG5H7M1BR8";

/// The run's clock reading and the ask's deadline, in seconds (mandate spec §6.4).
const NOW_S: i64 = 1_000;
const DEADLINE_S: i64 = 2_000;

/// The session's end, the cap the watch never reads past (DEC-853 item 5).
const CAP_S: i64 = 3_000;

/// The request's content object (journal spec §9.6): the bound members the fold rebuilds the
/// request from. Its own canonical hash tells a different story than the payload's recorded
/// `content_hash` member, so a seam that recomputed over the content would bind another order's
/// content than the one the approver was shown.
const ASK_CONTENT: &str =
    r#"{"deadline":2000,"instrument":"7b4a1c2e-5555-4a2b-9c3d-000000000005"}"#;

/// One instant at `secs` seconds past the epoch.
fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("the fixture instant parses")
}

/// One instant at an RFC 3339 timestamp, as the run fixture clocks parse theirs.
fn moment(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).expect("the fixture instant parses")
}

/// The `sha256:…` text of a content hash, as the record and the ask both carry it.
fn hash_of(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

/// The ask the run journaled (DEC-912 item 1): the approval it reads, the content hash of
/// exactly the order the approver was shown, and its deadline.
fn ask() -> Asked {
    Asked {
        approval: APPROVAL.to_owned(),
        content_hash: bound_order_hash(),
        deadline: at(DEADLINE_S),
    }
}

/// The content hash of exactly the order the approver was shown (LT-4).
fn bound_order_hash() -> String {
    hash_of(b"the ask's bound order")
}

/// The agent stream the run appends to (journal spec §2).
const AGENT: &str = "agent:ws_live:agent_01";

/// The workspace control stream the run reads (journal spec §2).
const CONTROL: &str = "ctl:ws_live";

/// One stored row of `stream` at `seq`, its body the canonical envelope the journal seals, with
/// the event's own id, type and payload members (journal spec §3).
fn row(stream: &str, seq: u64, event_id: &str, event_type: &str, payload: &str) -> StoredEvent {
    let body = mandate_canon::parse(
        format!(r#"{{"event_id":"{event_id}","event_type":"{event_type}","payload":{payload}}}"#)
            .as_bytes(),
    )
    .expect("the fixture body parses");
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "live".to_owned(),
        recorded_at: "2026-10-23T14:00:00.000000000Z".to_owned(),
        prev_hash: Digest::of(&[]),
        hash: Digest::of(payload.as_bytes()),
        body: to_canonical(&body),
    }
}

/// One `ApprovalRequested` row the run's cycle journaled (journal spec §9.6, closed at version
/// 1): the bound order's members, the request's `deadline` in risk-clock seconds, its timeout,
/// and the canonical `content_hash` of the content the approver was shown.
fn ask_row(seq: u64, event_id: &str, deadline_s: i64, content_hash: &str) -> StoredEvent {
    let payload = format!(
        r#"{{"approvers_required":1,"asset_class":"us_equity","combined_score":"0.9","content":{ASK_CONTENT},"content_hash":"{content_hash}","deadline":{deadline_s},"decided_by":"quant.ma_crossover","independent_required":false,"instrument":"7b4a1c2e-5555-4a2b-9c3d-000000000005","limit":"42","mandate_version":"sha256:{}","on_timeout":"skip","purpose":"open","qty":"1","reference_mark":null,"side":"buy","timeout_s":300}}"#,
        "1".repeat(64),
    );
    row(AGENT, seq, event_id, "ApprovalRequested", &payload)
}

/// One `ApprovalResponseSubmitted` row the CLI committed on the control stream (journal spec
/// §9.7, DEC-533): the members the wait reads are the approval it names, its verdict and the
/// content hash it repeats.
fn answer_row(seq: u64, approval: &str, verdict: &str, content_hash: &str) -> StoredEvent {
    let payload = format!(
        r#"{{"agent":"agent_01","approval":"{approval}","content_hash":"{content_hash}","responder":"user_founder","role":"approver","verdict":"{verdict}"}}"#
    );
    row(
        CONTROL,
        seq,
        &format!("01JZWANS{:018}", seq),
        "ApprovalResponseSubmitted",
        &payload,
    )
}

/// DEC-914 item 1, DEC-912 item 1, journal spec §9.6: the ask the run waits on comes verbatim
/// off the `ApprovalRequested` its own cycle journaled — the approval is the row's own event id,
/// the content hash is the payload's member, the deadline is the payload's seconds — so the
/// wait binds exactly the order the approver was shown, and rows that are not the ask (its
/// `ApprovalDelivered` before it) decide nothing.
#[test]
#[ignore = "pending E7-26"]
fn the_ask_comes_verbatim_from_the_row_the_run_journaled() {
    let delivered = row(
        AGENT,
        2,
        "01JZWDLV000000000000000000",
        "ApprovalDelivered",
        &format!(
            r#"{{"approval":"{APPROVAL}","channel":"cli_inbox","status":"delivered","message_id":null}}"#
        ),
    );
    let rows = vec![
        delivered,
        ask_row(3, APPROVAL, DEADLINE_S, &bound_order_hash()),
    ];
    let waited_on = asked(&rows, at(NOW_S));
    assert_eq!(
        waited_on,
        Ok(Asked {
            approval: APPROVAL.to_owned(),
            content_hash: bound_order_hash(),
            deadline: at(DEADLINE_S),
        }),
        "the ask's members are the row's own, never another row's or a recomputation"
    );
}

/// LT-4, #1308's review note (b), DEC-914 item 1: the asked content hash is never recomputed
/// over a rebuilt order — the fixture's content object carries its own canonical hash,
/// different from the recorded member — and only the member, the hash of the order the
/// approver was shown, releases; the drifted content's hash — what a seam that recomputed
/// would bind — refuses as a difference with nothing sent.
#[test]
#[ignore = "pending E7-26"]
fn a_drifted_rebuilt_order_never_releases_the_submit() {
    let rows = vec![ask_row(1, APPROVAL, DEADLINE_S, &bound_order_hash())];
    let drifted = hash_of(&to_canonical(
        &mandate_canon::parse(ASK_CONTENT.as_bytes()).expect("the fixture content object parses"),
    ));
    assert_ne!(
        drifted,
        bound_order_hash(),
        "the fixture's two hashes must tell different stories"
    );
    let bound = asked(&rows, at(NOW_S));
    assert_eq!(
        bound,
        Ok(Asked {
            approval: APPROVAL.to_owned(),
            content_hash: bound_order_hash(),
            deadline: at(DEADLINE_S),
        }),
        "the member names the order the approver was shown, not the drifted content"
    );
    let shown_order = vec![answer_row(1, APPROVAL, "approved", &bound_order_hash())];
    let drifted_order = vec![answer_row(1, APPROVAL, "approved", &drifted)];
    assert_eq!(answers(&shown_order, &ask(), at(NOW_S)), Ok(()));
    assert_eq!(
        answers(&drifted_order, &ask(), at(NOW_S)),
        Err(GrantRefusal::Mismatched),
        "a grant must bind the order the run journaled, never the rebuilt one"
    );
}

/// DEC-914 item 1, mandate spec §6.4 check 2, rule 3: the run waits on its **own** ask — the
/// youngest `ApprovalRequested` the journal's read holds — and a youngest ask already at its
/// deadline denies by its own timeout, without falling back on an older, still-live one that
/// belongs to another run; a younger live ask is waited on over an older dead one, and over an
/// older live one that is not this run's.
#[test]
#[ignore = "pending E7-26"]
fn the_run_waits_on_its_own_youngest_ask() {
    let old_hash = hash_of(b"an older run's order");
    let older_live = ask_row(1, "01JZWTG5OLDER00000000000AA", CAP_S, &old_hash);
    let young_dead = ask_row(2, APPROVAL, NOW_S, &bound_order_hash());
    let young_live = ask_row(2, APPROVAL, DEADLINE_S, &bound_order_hash());
    let dead_youngest = vec![older_live.clone(), young_dead];
    assert_eq!(
        asked(&dead_youngest, at(NOW_S)),
        Err(GrantRefusal::TimedOut),
        "the youngest ask's deadline denies the wait; an older live ask is not this run's"
    );
    let live_over_dead = vec![older_live.clone(), young_live.clone()];
    assert_eq!(
        asked(&live_over_dead, at(NOW_S)),
        Ok(Asked {
            approval: APPROVAL.to_owned(),
            content_hash: bound_order_hash(),
            deadline: at(DEADLINE_S),
        }),
        "a dead older ask is never waited on over the run's own live one"
    );
    let older_earlier_deadline = older_live;
    let live_over_live = vec![older_earlier_deadline, young_live];
    assert_eq!(
        asked(&live_over_live, at(NOW_S)).map(|asked| asked.approval),
        Ok(APPROVAL.to_owned()),
        "the youngest live ask is the run's own, whatever an older live one names"
    );
}

/// `AGENTS.md` rule 3, DEC-914 item 1, LT-4: an ask the run cannot read is never a guess — a
/// stream holding no `ApprovalRequested` at all binds nothing, and a member missing or not its
/// own shape (a `content_hash` absent or not text, a `deadline` not the integer seconds the
/// request carries) refuses as a difference, so nothing is sent.
#[test]
#[ignore = "pending E7-26"]
fn an_ask_the_run_cannot_read_refuses_as_a_difference() {
    let no_hash = format!(
        r#"{{"asset_class":"us_equity","deadline":{DEADLINE_S},"instrument":"7b4a1c2e-5555-4a2b-9c3d-000000000005","purpose":"open"}}"#
    );
    let hash_not_text = format!(
        r#"{{"asset_class":"us_equity","content_hash":{{"digest":"none"}},"deadline":{DEADLINE_S},"instrument":"7b4a1c2e-5555-4a2b-9c3d-000000000005","purpose":"open"}}"#
    );
    let deadline_not_seconds = r#"{"asset_class":"us_equity","content_hash":"sha256:6","deadline":"soon","instrument":"7b4a1c2e-5555-4a2b-9c3d-000000000005","purpose":"open"}"#;
    let delivered_only = vec![row(
        AGENT,
        1,
        "01JZWDLV000000000000000000",
        "ApprovalDelivered",
        &format!(
            r#"{{"approval":"{APPROVAL}","channel":"cli_inbox","status":"delivered","message_id":null}}"#
        ),
    )];
    assert_eq!(
        asked(&delivered_only, at(NOW_S)),
        Err(GrantRefusal::Mismatched),
        "a stream holding no ask binds nothing"
    );
    for payload in [no_hash, hash_not_text, deadline_not_seconds.to_owned()] {
        let rows = vec![row(AGENT, 1, APPROVAL, "ApprovalRequested", &payload)];
        assert_eq!(
            asked(&rows, at(NOW_S)),
            Err(GrantRefusal::Mismatched),
            "never a guess at what the ask meant: {payload}"
        );
    }
}

/// DEC-914 item 2, DEC-912 item 1 — "the submit is released by the answers in journal order:
/// the first answer naming the ask's approval decides it" — and #1308's review note (a): the
/// journal read's control rows reach `grant` verbatim, in journal order, never re-sorted by
/// another key, so a differing-hash answer first refuses whatever a matching grant behind it
/// says; an implementation reading the latest answer, or sorting the rows by their hashes,
/// would release where the journal's own order refuses.
#[test]
#[ignore = "pending E7-26"]
fn the_wait_folds_the_control_rows_verbatim_in_journal_order() {
    let rows = vec![
        answer_row(
            1,
            APPROVAL,
            "approved",
            &hash_of(b"another order's content"),
        ),
        answer_row(2, APPROVAL, "approved", &bound_order_hash()),
    ];
    let refused = answers(&rows, &ask(), at(NOW_S));
    assert_eq!(
        refused,
        Err(GrantRefusal::Mismatched),
        "the first answer in the journal's own order decides, whatever follows"
    );
}

/// DEC-914 item 2, mandate spec §6.4 — "An answer reaches the runtime only as an
/// `ApprovalResponseSubmitted` committed to the workspace control stream" — with the row's own
/// payload members folded exactly as the CLI committed them (#1308's review note (a)): a
/// command row carrying the ask's approval, hash and an approving verdict between the answers
/// decides nothing, and the committed answer releases the wait inside the ask's deadline.
#[test]
#[ignore = "pending E7-26"]
fn the_committed_answer_releases_the_wait_and_a_command_row_decides_nothing() {
    let command = row(
        CONTROL,
        1,
        "01JZWCMD000000000000000000",
        "OwnerCommandIssued",
        &format!(
            r#"{{"approval":"{APPROVAL}","content_hash":"{}","verdict":"approved"}}"#,
            bound_order_hash()
        ),
    );
    let rows = vec![
        command,
        answer_row(2, APPROVAL, "approved", &bound_order_hash()),
    ];
    let released = answers(&rows, &ask(), at(NOW_S));
    assert_eq!(
        released,
        Ok(()),
        "the control row's own payload members, folded as committed, release the wait"
    );
}

/// DEC-912 item 1 — "With no answer of this ask the submit stays blocked and the timeout denies
/// at it" — over the journal's own rows: the journal read holding no answer yet holds the wait
/// before the ask's deadline, and the same read at the deadline denies it.
#[test]
#[ignore = "pending E7-26"]
fn the_wait_holds_until_the_ask_denies_at_its_deadline() {
    let held = answers(&[], &ask(), at(NOW_S));
    let denied = answers(&[], &ask(), at(DEADLINE_S));
    assert_eq!(
        (held, denied),
        (Err(GrantRefusal::Unanswered), Err(GrantRefusal::TimedOut))
    );
}

/// Trading-domain spec §9.6's tier — "x = 1% for equities with 20-day median dollar volume
/// ≥ 50 M USD, 2% otherwise, 2% for crypto" — read through the gate's own members
/// (`mandate-risk`'s `conduct.rs`, #1308's review note (c): never a second number), with its
/// own sign and its own ambiguous case: an equity **at** the threshold takes the liquid member,
/// one known and below it the other, one unknown the narrower liquid member ("the ambiguous
/// case admits fewer prices, not more", DEC-163 item 2), and crypto the crypto member, whatever
/// the volume.
#[test]
#[ignore = "pending E7-26"]
fn the_collars_own_tier_decides_the_aggressiveness() {
    let liquid = Fraction::parse("0.01").expect("the liquid collar parses");
    let other = Fraction::parse("0.02").expect("the other collar parses");
    let crypto = Fraction::parse("0.03").expect("the crypto collar parses");
    let threshold = Usd::parse("50000000").expect("the liquid threshold parses");
    let below = Usd::parse("49999999").expect("a volume below the threshold parses");
    let above = Usd::parse("50000000.5").expect("a volume above the threshold parses");
    let equity = AssetClass::UsEquity;
    assert_eq!(
        collar_x(liquid, other, crypto, threshold, equity, Some(above)),
        Ok(liquid),
        "a volume above the threshold takes the liquid member"
    );
    assert_eq!(
        collar_x(liquid, other, crypto, threshold, equity, Some(below)),
        Ok(other),
        "a volume known and below the threshold takes the other member"
    );
    assert_eq!(
        collar_x(liquid, other, crypto, threshold, equity, Some(threshold)),
        Ok(liquid),
        "the spec's own sign: at the threshold is liquid, not other"
    );
    assert_eq!(
        collar_x(liquid, other, crypto, threshold, equity, None),
        Ok(liquid),
        "an unknown volume takes the narrower member"
    );
    assert_eq!(
        collar_x(liquid, other, crypto, threshold, AssetClass::Crypto, None),
        Ok(crypto),
        "crypto takes the crypto member, volume or none"
    );
    assert_eq!(
        collar_x(
            liquid,
            other,
            crypto,
            threshold,
            AssetClass::Crypto,
            Some(below)
        ),
        Ok(crypto)
    );
}

/// DEC-529 item 12 — "the collar and the risk mark use Robinhood's own quote … which must agree
/// with Alpaca's IEX quote within the collar or the run refuses" — through the gate's own `x`
/// (#1308's review note (c)): the same two quotes agree or refuse by the tier the volume picks,
/// so no constant can sit where the collar's `x` belongs — above the threshold the 1% collar
/// refuses the broker ask, below it the 2% collar admits it.
#[test]
#[ignore = "pending E7-26"]
fn the_cross_check_reads_the_collars_own_x_not_a_constant() {
    let liquid = Fraction::parse("0.01").expect("the liquid collar parses");
    let other = Fraction::parse("0.02").expect("the other collar parses");
    let threshold = Usd::parse("50000000").expect("the liquid threshold parses");
    let below = Usd::parse("49999999").expect("a volume below the threshold parses");
    let broker = Some(Price::parse("101.5").expect("the broker ask parses"));
    let iex = Some(Price::parse("100").expect("the iex ask parses"));
    let refused = collar_x(
        liquid,
        other,
        liquid,
        threshold,
        AssetClass::UsEquity,
        Some(threshold),
    )
    .and_then(|x| cross_check(broker, iex, x));
    let admitted = collar_x(
        liquid,
        other,
        liquid,
        threshold,
        AssetClass::UsEquity,
        Some(below),
    )
    .and_then(|x| cross_check(broker, iex, x));
    assert_eq!(
        (refused, admitted),
        (Err(QuoteRefusal::Disagreement), Ok(())),
        "the tier the volume picks decides, never a constant"
    );
}

/// DEC-858 item 4 — "The watch ends when the broker reports the entry `filled`, `canceled`,
/// `expired` or `rejected`" — DEC-914 item 4: the `Watched` the LT-10 question reads calls the
/// entry terminal in exactly those four states and in no other, so a read absent, unknown or
/// still working leaves the run reading (`AGENTS.md` rule 3).
#[test]
#[ignore = "pending E7-26"]
fn the_watch_reads_terminal_only_in_the_four_reported_states() {
    for status in ["filled", "canceled", "expired", "rejected"] {
        let terminal = watched(Some(status), false, &[]);
        assert_eq!(
            terminal,
            Ok(Watched {
                entry_terminal: true,
                uncovered: false,
                alerted: None,
            }),
            "the four terminal states alone end the entry: {status}"
        );
    }
    for read in [
        None,
        Some("accepted"),
        Some("partially_filled"),
        Some("pending_new"),
        Some("halt"),
    ] {
        let working = watched(read, false, &[]);
        assert_eq!(
            working,
            Ok(Watched {
                entry_terminal: false,
                uncovered: false,
                alerted: None,
            }),
            "an absent, unknown or working read leaves the run reading: {read:?}"
        );
    }
}

/// DEC-912 item 3 — "what the seam observes is the run's report holding the interval's alert
/// key (`unprotected_interval_limit`, notifications spec §4.2's `protection` row)" — DEC-914
/// item 4: the report's own keys alone report the interval, and only its own key does — the
/// row's other key, `stop_watchdog`, is another alarm's and shows nothing for this interval —
/// and the executor's uncovered answer passes through as the executor gave it.
#[test]
#[ignore = "pending E7-26"]
fn the_watch_carries_the_intervals_own_alert_key_alone() {
    let interval_alerted = watched(Some("filled"), true, &["unprotected_interval_limit"]);
    assert_eq!(
        interval_alerted,
        Ok(Watched {
            entry_terminal: true,
            uncovered: true,
            alerted: Some("unprotected_interval_limit"),
        }),
        "the report holding the interval's key reports it"
    );
    for alerts in [
        Vec::new(),
        vec!["stop_watchdog"],
        vec!["stop_watchdog", "unprotected_interval_limit"],
    ] {
        let reported = watched(Some("filled"), true, &alerts);
        assert_eq!(
            reported.map(|watched| watched.alerted),
            Ok(alerts
                .contains(&"unprotected_interval_limit")
                .then_some("unprotected_interval_limit")),
            "only the interval's own key reports it: {alerts:?}"
        );
    }
}

/// LT-10 — "When the run exits, the entry is terminal and any filled quantity is covered by the
/// profile's strongest protection, or the unprotected interval is journaled and its alert key is
/// in the run's report" — over the `Watched` the wiring observes (DEC-914 item 4): a terminal
/// covered or flat entry ends the run; an uncovered one ends only with its interval reported;
/// an entry still working holds it; and past the cap — the session's end — an unsettled watch
/// fails closed while a settled one ends clean (DEC-853 item 5, DEC-912 item 3).
#[test]
#[ignore = "pending E7-26"]
fn the_watch_ends_only_on_covered_or_alerted_or_terminal_facts() {
    let end = |read: Option<&str>, uncovered: bool, alerts: &[&'static str]| {
        watched(read, uncovered, alerts).and_then(|facts| lt10(&facts, at(NOW_S), at(CAP_S)))
    };
    let flat = end(Some("filled"), false, &[]);
    let covered = end(Some("canceled"), false, &[]);
    let uncovered = end(Some("filled"), true, &[]);
    let alerted = end(Some("filled"), true, &["unprotected_interval_limit"]);
    let working = end(Some("accepted"), false, &[]);
    assert_eq!(flat, Ok(()));
    assert_eq!(covered, Ok(()));
    assert_eq!(uncovered, Err(Lt10Refusal::Unsettled));
    assert_eq!(alerted, Ok(()));
    assert_eq!(working, Err(Lt10Refusal::Unsettled));
    let capped = |facts: Result<Watched, Lt10Refusal>| {
        facts.and_then(|facts| lt10(&facts, at(CAP_S), at(CAP_S)))
    };
    let unsettled_at_cap = capped(watched(Some("filled"), true, &[]));
    let settled_at_cap = capped(watched(
        Some("filled"),
        true,
        &["unprotected_interval_limit"],
    ));
    assert_eq!(
        (unsettled_at_cap, settled_at_cap),
        (Err(Lt10Refusal::Capped), Ok(())),
        "no read past the cap, and no error where nothing is left to read"
    );
}

/// DEC-877 item 1 — "The watch stops at the first wake at or past that instant" — DEC-914 item
/// 5: the cap is the end of the regular session the bound is in, on the same
/// `mandate_time::ExchangeCalendar` the close-window bound read; bounded at 15:50 New York on
/// the full day Tuesday 2026-09-29, the watch's reads fail at 16:00.
#[test]
#[ignore = "pending E7-26"]
fn the_watch_caps_at_the_sessions_end_on_a_full_day() {
    let capped = session_cap(moment("2026-09-29T19:50:00Z"));
    let end = match capped {
        Ok(capped) => capped,
        Err(other) => panic!("the full day's session end must place the cap: {other:?}"),
    };
    assert_eq!(end, moment("2026-09-29T20:00:00Z"));
}

/// DEC-877 item 1 — "So it is 16:00 New York on a full day, and the early close on a short
/// one" — DEC-914 item 5: on Thursday 2026-12-24, an early close at 13:00 New York, a watch
/// bounded at 12:50 caps at the early close itself, never at 16:00.
#[test]
#[ignore = "pending E7-26"]
fn the_watch_caps_at_the_early_close_on_a_short_day() {
    let capped = session_cap(moment("2026-12-24T17:50:00Z"));
    let early_end = match capped {
        Ok(capped) => capped,
        Err(other) => panic!("the short day's session end must place the cap: {other:?}"),
    };
    assert_eq!(early_end, moment("2026-12-24T18:00:00Z"));
}

/// `AGENTS.md` rule 3, DEC-914 item 5: a bound no regular session holds places no cap, and the
/// watch fails closed rather than reading without one — the close-window bound already refused
/// such a run (FT-8), so this stop only refuses again.
#[test]
#[ignore = "pending E7-26"]
fn a_bound_no_session_holds_fails_the_watch_closed() {
    let capped = session_cap(moment("2026-10-03T19:50:00Z"));
    let refused: Result<&str, String> = match capped {
        Err(Cause::Absent { what }) => Ok(what),
        other => panic!("no session, no cap, no read: {other:?}"),
    };
    assert_eq!(refused, Ok("the regular session the watch's bound is in"));
}

/// The founder's run's step 6 — "`mandate journal export` for the agent, account and control
/// streams" — journal spec §2, DEC-912 item 4, DEC-914 item 6: a placing run's report names its
/// exportable evidence as the journal names it, derived from the run's own workspace, agent and
/// account ref (#1308's review note (e): the typed ids), each a §2 stream id of its type.
#[test]
#[ignore = "pending E7-26"]
fn a_placing_run_reports_the_streams_its_export_reads() {
    let evidence = reported(
        Some("the workspace journal the run appended to"),
        true,
        "ws_live",
        "agent_01",
        "acct_live",
    );
    let expected = [
        "agent:ws_live:agent_01".to_owned(),
        "acct:ws_live:acct_live".to_owned(),
        "ctl:ws_live".to_owned(),
    ];
    assert_eq!(
        evidence,
        Ok(Evidence {
            streams: expected.clone()
        })
    );
    let types = [
        (expected[0].as_str(), StreamType::Agent),
        (expected[1].as_str(), StreamType::Account),
        (expected[2].as_str(), StreamType::Control),
    ];
    for (name, stream_type) in types {
        assert_eq!(
            StreamId::parse(name).map(|stream| stream.stream_type()),
            Some(stream_type),
            "{name} must be a §2 stream id"
        );
    }
}

/// DEC-912 item 4 — "A run that kept no journal … has no exportable evidence and the naming
/// refuses" — DEC-157 item 6 ("A planning run keeps no journal"), the founder's run's step 4 (a
/// run without `--place-one-order` writes nothing), DEC-914 item 6: the evidence reads the
/// journal the run **appended to** — the arguments' journal held only while placing — so a run
/// with no DSN names no stream, and so does a dry run holding a DSN it wrote nothing to.
#[test]
#[ignore = "pending E7-26"]
fn a_run_that_wrote_nothing_names_no_stream() {
    let no_dsn = reported(None, true, "ws_live", "agent_01", "acct_live");
    let wrote_nothing = reported(
        Some("a journal the dry run never appended to"),
        false,
        "ws_live",
        "agent_01",
        "acct_live",
    );
    assert_eq!(
        (no_dsn, wrote_nothing),
        (
            Err(EvidenceRefusal::JournalAbsent),
            Err(EvidenceRefusal::JournalAbsent)
        ),
        "the naming reads the journal the run appended to, not the DSN at hand"
    );
}
