//! The live run's grant wait, quote cross-check, watch end and export evidence (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1b): the `ask`
//! opening that waits for the founder's grant (LT-4), the broker quote checked against the
//! independent IEX quote within the collar (DEC-529 item 12), the bounded reads until LT-10,
//! and the streams the run's `journal export` reads. These are the seams' pins, live since
//! #1308 closed them; the wiring's own pins (DEC-914) live beside them in `live_wiring.rs`.
//! Every fixture is a recorded record or a parsed decimal, holds no credential and reaches no
//! host (ES-19, LT-1, `AGENTS.md` rule 8).

use mandate_canon::{Digest, Value};
use mandate_domain::Environment;
use mandate_journal::{StreamId, StreamType};
use mandate_num::{Fraction, Price};
use mandate_paper::{
    Asked, Evidence, EvidenceRefusal, GrantRefusal, Journaled, Lt10Refusal, QuoteRefusal, Watched,
    cross_check, evidence, grant, lt10,
};
use mandate_shell::control::{ControlRecord, Origin};
use mandate_time::UtcNanos;

/// The ask's approval: the `ApprovalRequested`'s event id, an opaque id (TI-8).
const APPROVAL: &str = "01JZWTG5EV7Q4ZF0AG5H7M1BR8";

/// The run's clock reading and the ask's deadline, in seconds (mandate spec §6.4).
const NOW_S: i64 = 1_000;
const DEADLINE_S: i64 = 2_000;

/// The session's end, the cap the watch never reads past (DEC-853 item 5).
const CAP_S: i64 = 3_000;

/// One instant at `secs` seconds past the epoch.
fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("the fixture instant parses")
}

/// The `sha256:…` text of a content hash, as the record and the ask both carry it.
fn hash_of(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

/// The ask the run journaled ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 1):
/// the approvals it reads, the content hash of exactly the order the approver was shown, and
/// its deadline.
fn ask() -> Asked {
    Asked {
        approval: APPROVAL.to_owned(),
        content_hash: hash_of(b"the ask's bound order"),
        deadline: at(DEADLINE_S),
    }
}

/// One journaled row at `seq`: its own event id, its envelope's `causation_id`, and its record.
fn journaled(
    seq: u64,
    event_id: &str,
    causation_id: Option<&str>,
    event_type: &str,
    payload: &str,
) -> Journaled {
    Journaled {
        event_id: event_id.to_owned(),
        causation_id: causation_id.map(str::to_owned),
        record: ControlRecord {
            seq,
            event_time: None,
            event_type: event_type.to_owned(),
            payload: json(payload),
            origin: Origin {
                stream_id: "ctl:ws_live".to_owned(),
                environment: Environment::Live,
            },
        },
    }
}

/// One control-stream record at `seq` with an event id of its own and no cause.
fn record(seq: u64, event_type: &str, payload: &str) -> Journaled {
    journaled(
        seq,
        &format!("01JZWCTL{seq:018}"),
        None,
        event_type,
        payload,
    )
}

/// The event id of the answer at control-stream `seq`.
fn answer_id(seq: u64) -> String {
    format!("01JZWANS{seq:018}")
}

/// One `ApprovalResponseSubmitted` the CLI committed (journal spec §9.7, DEC-533): the members
/// the run reads are the approval it names, its verdict and the content hash it repeats; its
/// event id is [`answer_id`].
fn answer(seq: u64, approval: &str, verdict: &str, content_hash: &str) -> Journaled {
    let payload = format!(
        r#"{{"agent":"agent_01","approval":"{approval}","content_hash":"{content_hash}","responder":"user_founder","role":"approver","verdict":"{verdict}"}}"#
    );
    journaled(
        seq,
        &answer_id(seq),
        None,
        "ApprovalResponseSubmitted",
        &payload,
    )
}

/// The event id of the runtime's record at agent-stream `seq`.
fn agent_id(seq: u64) -> String {
    format!("01JZWAGT{seq:018}")
}

/// The runtime's copy of the answer `answer` at agent-stream `seq` (journal spec §9.7, rule 50:
/// its `causation_id` is the answer): the approval and verdict it copies, and the result
/// admission gave it — `admitted`, `counted`, or `refused` with the first failing check's reason.
fn copy(seq: u64, answer: &str, approval: &str, verdict: &str, result: &str) -> Journaled {
    let reason = match result {
        "refused" => r#""not_an_approver""#,
        _ => "null",
    };
    let payload = format!(
        r#"{{"approval":"{approval}","delegation":null,"effective_at":1000,"quorum":null,"reason":{reason},"responder":"user_founder","result":"{result}","role":"approver","separation_of_duties":null,"step_up":null,"verdict":"{verdict}"}}"#
    );
    journaled(
        seq,
        &agent_id(seq),
        Some(answer),
        "ApprovalResponded",
        &payload,
    )
}

/// The runtime's re-validation of the admitted copy `copy` at agent-stream `seq` (journal spec
/// §9.7: its `causation_id` is the `ApprovalResponded` it re-validates), with its result.
fn revalidation(seq: u64, copy: &str, approval: &str, result: &str) -> Journaled {
    let reason = match result {
        "skip" => r#""drift""#,
        _ => "null",
    };
    let payload = format!(
        r#"{{"approval":"{approval}","band_bp":100,"result":"{result}","reason":{reason}}}"#
    );
    journaled(
        seq,
        &agent_id(seq),
        Some(copy),
        "ApprovalRevalidated",
        &payload,
    )
}

/// The runtime's records of a grant it admitted and re-validated to `act`: the copy of the
/// answer at control-stream `answer_seq` at agent-stream `seq`, and its re-validation behind it.
fn admitted(seq: u64, answer_seq: u64) -> Vec<Journaled> {
    vec![
        copy(
            seq,
            &answer_id(answer_seq),
            APPROVAL,
            "approved",
            "admitted",
        ),
        revalidation(seq + 1, &agent_id(seq), APPROVAL, "act"),
    ]
}

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).expect("the fixture payload parses")
}

/// LT-4 and mandate spec §6.4 (DEC-912 item 1): an `ask` opening submits nothing while its
/// grant has not arrived — the answers the run folded hold none, and the deadline is still
/// ahead — so the submit stays blocked and nothing is sent.
#[test]
fn an_unanswered_ask_holds_the_submit_before_its_deadline() {
    let held = grant(&[], &[], &ask(), at(NOW_S));
    assert_eq!(
        held,
        Err(GrantRefusal::Unanswered),
        "nothing is submitted until the grant"
    );
}

/// LT-4 (DEC-912 item 1, DEC-920 item 1): the grant releases the submit exactly when its content
/// hash equals the hash of the request the run journaled — the one order the approver was shown
/// — the runtime admitted it and re-validated it to `act`, and the run's clock is before the
/// ask's deadline. Once released, the cycle's own one-order door (FT-1) still governs what is
/// submitted, once.
#[test]
fn a_grant_repeating_the_requests_hash_releases_the_submit() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let released = grant(&answers, &admitted(1, 1), &ask, at(NOW_S));
    assert_eq!(released, Ok(()));
}

/// LT-4 (DEC-912 item 1): an answer repeating another content hash — whatever its verdict —
/// is a refusal with nothing sent: the grant never binds the order the run would send, so the
/// run ends refused rather than sending it.
#[test]
fn an_answer_repeating_another_hash_refuses_with_nothing_sent() {
    let ask = ask();
    let differing = hash_of(b"another order's content");
    for verdict in ["approved", "skipped"] {
        let answers = vec![answer(1, APPROVAL, verdict, &differing)];
        let refused = grant(&answers, &admitted(1, 1), &ask, at(NOW_S));
        assert_eq!(
            refused,
            Err(GrantRefusal::Mismatched),
            "the answer's verdict must not release it: {verdict}"
        );
    }
}

/// LT-4, mandate spec §6.4's lifecycle (DEC-912 item 1, DEC-920 item 2): a skip of the ask
/// repeating its hash denies it whenever it is read, so nothing is sent; a deny never adds risk,
/// so it waits on no admission.
#[test]
fn a_skip_of_the_ask_denies_it() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "skipped", &ask.content_hash)];
    let denied = grant(&answers, &[], &ask, at(NOW_S));
    assert_eq!(denied, Err(GrantRefusal::Denied));
}

/// `AGENTS.md` rule 3, mandate spec §6.4 check 2 (DEC-912 item 1): a silent ask denies at its
/// deadline, and a grant read at or after it is late — so neither sends anything, however the
/// answers arrived.
#[test]
fn a_silent_or_late_ask_denies_at_its_deadline() {
    let ask = ask();
    let on_time_answer_too_late = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let silent = grant(&[], &[], &ask, at(DEADLINE_S));
    let late = grant(
        &on_time_answer_too_late,
        &admitted(1, 1),
        &ask,
        at(DEADLINE_S),
    );
    assert_eq!(
        (silent, late),
        (Err(GrantRefusal::TimedOut), Err(GrantRefusal::TimedOut))
    );
}

/// LT-4 (DEC-912 item 1): only the answers of this ask's own approval are this seam's to read —
/// a grant of another approval, however much its hash and verdict agree, never releases this
/// ask's submit, and the run keeps waiting on its own.
#[test]
fn an_answer_of_another_ask_never_releases_this_one() {
    let ask = ask();
    let answers = vec![answer(
        1,
        "approval_other_01",
        "approved",
        &ask.content_hash,
    )];
    let held = grant(&answers, &admitted(1, 1), &ask, at(NOW_S));
    assert_eq!(
        held,
        Err(GrantRefusal::Unanswered),
        "a foreign approval's grant is not this ask's"
    );
}

/// `AGENTS.md` rule 3, LT-4 (DEC-912 item 1): an answer holding a member the run cannot read —
/// a verdict missing or not text, a content hash missing — is never a guess: it refuses as a
/// difference, and nothing is sent.
#[test]
fn an_answer_the_run_cannot_read_refuses_as_a_difference() {
    let ask = ask();
    let no_verdict = format!(
        r#"{{"agent":"agent_01","approval":"{APPROVAL}","content_hash":"{}","responder":"user_founder","role":"approver"}}"#,
        ask.content_hash
    );
    let not_text = format!(
        r#"{{"agent":"agent_01","approval":"{APPROVAL}","content_hash":"{}","responder":"user_founder","role":"approver","verdict":7}}"#,
        ask.content_hash
    );
    let no_hash = format!(
        r#"{{"agent":"agent_01","approval":"{APPROVAL}","responder":"user_founder","role":"approver","verdict":"approved"}}"#
    );
    for payload in [no_verdict, not_text, no_hash] {
        let answers = vec![record(1, "ApprovalResponseSubmitted", &payload)];
        let refused = grant(&answers, &admitted(1, 1), &ask, at(NOW_S));
        assert_eq!(refused, Err(GrantRefusal::Mismatched), "{payload}");
    }
}

/// LT-4, mandate spec §6.4 (DEC-912 item 1): the answers decide the ask **in journal order** —
/// "the submit is released by the answers in journal order: the first answer naming the ask's
/// approval decides it" — so a differing hash answered first refuses the run whatever a later
/// matching grant says ("any difference is a refusal with nothing sent"), and a row that is not
/// an `ApprovalResponseSubmitted` decides nothing: even one carrying the ask's approval, a
/// matching hash and an approving verdict between the two answers is a command row, not an
/// answer, and no release reads it.
#[test]
fn the_first_answer_of_the_ask_decides_whatever_rows_follow() {
    let ask = ask();
    let differing_first = answer(
        1,
        APPROVAL,
        "approved",
        &hash_of(b"another order's content"),
    );
    let not_an_answer = record(
        2,
        "OwnerCommandIssued",
        &format!(
            r#"{{"approval":"{APPROVAL}","content_hash":"{}","verdict":"approved"}}"#,
            ask.content_hash
        ),
    );
    let matching_last = answer(3, APPROVAL, "approved", &ask.content_hash);
    let answers = vec![differing_first, not_an_answer, matching_last];
    let refused = grant(&answers, &admitted(1, 3), &ask, at(NOW_S));
    assert_eq!(
        refused,
        Err(GrantRefusal::Mismatched),
        "the first answer decides, and a command row never asks for the submit"
    );
}

/// DEC-920 item 1, DEC-912 item 1 ("Admission ... stays the runtime's"), mandate spec §6.4:
/// an answer reaches the runtime only as an `ApprovalResponseSubmitted`, and only the runtime's
/// admission says whether it is a grant — so the raw answer, its approval, hash and verdict all
/// agreeing, never releases the submit by itself: before the runtime has copied it the wait
/// holds, and at the deadline it denies, with nothing sent.
#[test]
fn a_raw_answer_the_runtime_has_not_admitted_never_releases_the_submit() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let held = grant(&answers, &[], &ask, at(NOW_S));
    let denied = grant(&answers, &[], &ask, at(DEADLINE_S));
    assert_eq!(
        (held, denied),
        (Err(GrantRefusal::Unanswered), Err(GrantRefusal::TimedOut)),
        "a raw answer is not a grant until the runtime admits it"
    );
}

/// DEC-920 item 1, mandate spec §6.4 check 3 ("The response's actor is a `user` in
/// `autonomy.approval.approvers` ... no agent, system, broker, or platform operator can
/// approve") and its lifecycle ("A refused response and a grant short of the quorum (`counted`)
/// are not terminal"): an answer the runtime refused — a non-approver's, or any actor's that is
/// not a user — and one it only counted toward the quorum never release the submit; the wait
/// holds, and the timeout denies at the deadline.
#[test]
fn a_refused_or_counted_answer_never_releases_the_submit() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    for result in ["refused", "counted"] {
        let judged = vec![copy(1, &answer_id(1), APPROVAL, "approved", result)];
        let held = grant(&answers, &judged, &ask, at(NOW_S));
        let denied = grant(&answers, &judged, &ask, at(DEADLINE_S));
        assert_eq!(
            (held, denied),
            (Err(GrantRefusal::Unanswered), Err(GrantRefusal::TimedOut)),
            "a {result} answer is not a grant"
        );
    }
}

/// DEC-920 item 1, mandate spec §6.4's lifecycle: a refused answer is not terminal, so the fold
/// reads on to the ask's next answer — the founder's admitted grant behind a non-approver's
/// refused one releases the submit, and a differing hash behind it still refuses as a
/// difference (LT-4: any difference is a refusal with nothing sent).
#[test]
fn the_fold_reads_past_a_refused_answer_to_the_next() {
    let ask = ask();
    let refused_then_admitted = vec![
        copy(1, &answer_id(1), APPROVAL, "approved", "refused"),
        copy(2, &answer_id(2), APPROVAL, "approved", "admitted"),
        revalidation(3, &agent_id(2), APPROVAL, "act"),
    ];
    let both_match = vec![
        answer(1, APPROVAL, "approved", &ask.content_hash),
        answer(2, APPROVAL, "approved", &ask.content_hash),
    ];
    assert_eq!(
        grant(&both_match, &refused_then_admitted, &ask, at(NOW_S)),
        Ok(()),
        "the admitted grant behind a refused answer releases the submit"
    );
    let differing_behind = vec![
        answer(1, APPROVAL, "approved", &ask.content_hash),
        answer(
            2,
            APPROVAL,
            "approved",
            &hash_of(b"another order's content"),
        ),
    ];
    assert_eq!(
        grant(&differing_behind, &refused_then_admitted, &ask, at(NOW_S)),
        Err(GrantRefusal::Mismatched),
        "a differing hash behind a refused answer still refuses"
    );
}

/// DEC-920 item 1, journal spec §9.7 rule 50 (`ApprovalResponded`'s `causation_id` is "the
/// `ApprovalResponseSubmitted` it copies"): the runtime's record releases only the answer it
/// copies — an admitted copy of another answer, with its own `act`, never releases this one, so
/// the wait holds — and the copy is found by its cause, wherever another answer's copy sits
/// before it in the agent stream's order.
#[test]
fn only_the_runtimes_copy_of_this_answer_decides_it() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let another_answers = vec![
        copy(1, &answer_id(9), APPROVAL, "approved", "admitted"),
        revalidation(2, &agent_id(1), APPROVAL, "act"),
    ];
    assert_eq!(
        grant(&answers, &another_answers, &ask, at(NOW_S)),
        Err(GrantRefusal::Unanswered),
        "another answer's admission is not this answer's"
    );
    let behind_another = vec![
        copy(1, &answer_id(9), APPROVAL, "approved", "refused"),
        copy(2, &answer_id(1), APPROVAL, "approved", "admitted"),
        revalidation(3, &agent_id(2), APPROVAL, "act"),
    ];
    assert_eq!(
        grant(&answers, &behind_another, &ask, at(NOW_S)),
        Ok(()),
        "this answer's own copy decides, wherever it sits"
    );
}

/// DEC-920 item 1, mandate spec §6.4 (re-validation, checks 8 to 12: "Re-validation only
/// skips"; the lifecycle's terminal `ApprovalRevalidated` with result `act` or `skip`) and the
/// workspace API spec's API-12 ("never reports a grant as acting: only the runtime's
/// `ApprovalResponded` and `ApprovalRevalidated` say what happened"): an admitted grant releases
/// only once its re-validation acts — with none yet the wait holds, a `skip` denies, and a
/// re-validation of another copy decides nothing.
#[test]
fn an_admitted_grant_releases_only_once_its_revalidation_acts() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let admitted_only = vec![copy(1, &answer_id(1), APPROVAL, "approved", "admitted")];
    let skipped = vec![
        copy(1, &answer_id(1), APPROVAL, "approved", "admitted"),
        revalidation(2, &agent_id(1), APPROVAL, "skip"),
    ];
    let another_copies_revalidation = vec![
        copy(1, &answer_id(1), APPROVAL, "approved", "admitted"),
        revalidation(2, &agent_id(7), APPROVAL, "act"),
    ];
    assert_eq!(
        (
            grant(&answers, &admitted_only, &ask, at(NOW_S)),
            grant(&answers, &skipped, &ask, at(NOW_S)),
            grant(&answers, &another_copies_revalidation, &ask, at(NOW_S)),
            grant(&answers, &admitted(1, 1), &ask, at(NOW_S)),
        ),
        (
            Err(GrantRefusal::Unanswered),
            Err(GrantRefusal::Denied),
            Err(GrantRefusal::Unanswered),
            Ok(()),
        ),
        "only the re-validation's act releases the admitted grant"
    );
}

/// DEC-920 item 2, LT-4, mandate spec §6.4's lifecycle ("an admitted skip"): a deny stays a
/// deny — a skip of the ask denies whatever the runtime made of it, admitted or refused, and
/// whatever an admitted grant behind it says, because the first answer decides and a deny never
/// adds risk.
#[test]
fn a_skip_denies_whatever_the_runtime_made_of_it() {
    let ask = ask();
    let skipped_then_granted = vec![
        answer(1, APPROVAL, "skipped", &ask.content_hash),
        answer(2, APPROVAL, "approved", &ask.content_hash),
    ];
    for result in ["admitted", "refused"] {
        let mut judged = vec![copy(1, &answer_id(1), APPROVAL, "skipped", result)];
        judged.extend(admitted(2, 2));
        assert_eq!(
            grant(&skipped_then_granted, &judged, &ask, at(NOW_S)),
            Err(GrantRefusal::Denied),
            "a {result} skip denies, whatever grant follows it"
        );
    }
}

/// `AGENTS.md` rule 3, DEC-920 item 3: a record of the runtime's the wait cannot read is never a
/// guess — a copy of this answer naming another approval or another verdict, a copy whose result
/// is none of `admitted`, `counted` and `refused`, a re-validation naming another approval, and
/// one whose result is neither `act` nor `skip` each refuse as a difference, with nothing sent.
#[test]
fn a_runtime_record_the_run_cannot_read_refuses_as_a_difference() {
    let ask = ask();
    let answers = vec![answer(1, APPROVAL, "approved", &ask.content_hash)];
    let acted = |copy_row: Journaled| {
        let revalidated = revalidation(2, &copy_row.event_id, APPROVAL, "act");
        vec![copy_row, revalidated]
    };
    let unread = [
        (
            "a copy of another approval",
            acted(copy(
                1,
                &answer_id(1),
                "approval_other_01",
                "approved",
                "admitted",
            )),
        ),
        (
            "a copy of another verdict",
            acted(copy(1, &answer_id(1), APPROVAL, "skipped", "admitted")),
        ),
        (
            "a copy with no result the spec names",
            acted(copy(1, &answer_id(1), APPROVAL, "approved", "recorded")),
        ),
        (
            "a re-validation of another approval",
            vec![
                copy(1, &answer_id(1), APPROVAL, "approved", "admitted"),
                revalidation(2, &agent_id(1), "approval_other_01", "act"),
            ],
        ),
        (
            "a re-validation with no result the spec names",
            vec![
                copy(1, &answer_id(1), APPROVAL, "approved", "admitted"),
                revalidation(2, &agent_id(1), APPROVAL, "acted"),
            ],
        ),
    ];
    for (what, judged) in unread {
        assert_eq!(
            grant(&answers, &judged, &ask, at(NOW_S)),
            Err(GrantRefusal::Mismatched),
            "{what} is never a guess"
        );
    }
}

/// DEC-529 item 12 (DEC-912 item 2): the tolerance is the collar's own aggressiveness, the
/// fraction the gate reads — so the same reads that agree at a 2% collar disagree at the 1%
/// collar, a broker ask inside the wider band and outside the narrower one, and no constant
/// can sit where the collar's `x` belongs (trading-domain spec §9.6's two equity tiers).
#[test]
fn the_collars_own_aggressiveness_decides_the_agreement() {
    let wide = Fraction::parse("0.02").expect("the wide collar fixture parses");
    let narrow = Fraction::parse("0.01").expect("the narrow collar fixture parses");
    let between_the_bands = cross_check(
        Some(Price::parse("101.5").expect("the broker ask parses")),
        Some(Price::parse("100").expect("the iex ask parses")),
        wide,
    );
    let same_reads_narrower = cross_check(
        Some(Price::parse("101.5").expect("the broker ask parses")),
        Some(Price::parse("100").expect("the iex ask parses")),
        narrow,
    );
    assert_eq!(
        (between_the_bands, same_reads_narrower),
        (Ok(()), Err(QuoteRefusal::Disagreement))
    );
}

/// DEC-529 item 12 (DEC-912 item 2, [DEC-163](../../../docs/project/04-decision-log.md#decisions)
/// item 2): the collar's ceiling rounds inward, exactly as `Price::collar_bound` rounds it —
/// the buy's ceiling truncates toward zero at the 9 places a price holds — so a price sitting
/// exactly on the inward-rounded ceiling stays inside and one grid step past it refuses: an
/// IEX ask of 10.123456789 at a 1% collar prices an exact ceiling of **10.22469135689**
/// (10.123456789 × 101 ÷ 100, eleven places, which no price holds), and the end the band keeps
/// is **10.224691356** — truncated toward zero, verified against `Price::collar_bound` itself.
#[test]
fn the_collars_ceiling_rounds_inward_at_nine_places() {
    let collar = Fraction::parse("0.01").expect("the collar fixture parses");
    let iex = Price::parse("10.123456789").expect("the iex ask parses");
    let on_the_rounded_ceiling = cross_check(
        Some(Price::parse("10.224691356").expect("the broker ask parses")),
        Some(iex),
        collar,
    );
    let one_grid_step_past = cross_check(
        Some(Price::parse("10.224691357").expect("the broker ask parses")),
        Some(iex),
        collar,
    );
    assert_eq!(
        (on_the_rounded_ceiling, one_grid_step_past),
        (Ok(()), Err(QuoteRefusal::Disagreement))
    );
}

/// DEC-529 item 12 (DEC-912 item 2, [DEC-163](../../../docs/project/04-decision-log.md#decisions)
/// item 2): the collar's floor rounds inward too — a sell's floor rounds up, `Rounding::Ceiling`
/// at the 9 places a price holds, as `Price::collar_bound(Adverse::Down)` rounds it — so the
/// same IEX ask of 10.123456789 at a 1% collar prices an exact floor of **10.02222222111**
/// (10.123456789 × 99 ÷ 100, eleven places, which no price holds), and the end the band keeps
/// is **10.022222222**: a price sitting exactly on the inward-rounded floor stays inside and
/// one grid step below it refuses, pinning the lower end as tightly as the ceiling test above.
#[test]
fn the_collars_floor_rounds_inward_at_nine_places() {
    let collar = Fraction::parse("0.01").expect("the collar fixture parses");
    let iex = Price::parse("10.123456789").expect("the iex ask parses");
    let on_the_rounded_floor = cross_check(
        Some(Price::parse("10.022222222").expect("the broker ask parses")),
        Some(iex),
        collar,
    );
    let one_grid_step_below = cross_check(
        Some(Price::parse("10.022222221").expect("the broker ask parses")),
        Some(iex),
        collar,
    );
    assert_eq!(
        (on_the_rounded_floor, one_grid_step_below),
        (Ok(()), Err(QuoteRefusal::Disagreement))
    );
}

/// DEC-529 item 12 (DEC-912 item 2): a broker quote inside the collar the independent IEX ask
/// prices — the tolerance the decision states, no other number — is the quote the collar and
/// the mark may use, and the opening proceeds.
#[test]
fn a_broker_quote_within_the_collar_proceeds() {
    let collar = Fraction::parse("0.01").expect("the collar fixture parses");
    let agreed = cross_check(
        Some(Price::parse("100.85").expect("the broker ask parses")),
        Some(Price::parse("100").expect("the iex ask parses")),
        collar,
    );
    assert_eq!(agreed, Ok(()));
}

/// DEC-529 item 12 (DEC-912 item 2): a broker quote that disagrees with the independent quote
/// beyond the collar — the bad tick the cross-check exists for — refuses the opening before
/// any submission, in whichever direction it jumps.
#[test]
fn a_broker_quote_beyond_the_collar_refuses_before_any_submission() {
    let collar = Fraction::parse("0.01").expect("the collar fixture parses");
    let iex = Price::parse("100").expect("the iex ask parses");
    for jumped in ["101.5", "98.5"] {
        let refused = cross_check(
            Some(Price::parse(jumped).expect("the broker ask parses")),
            Some(iex),
            collar,
        );
        assert_eq!(
            refused,
            Err(QuoteRefusal::Disagreement),
            "a jump in either direction refuses: {jumped}"
        );
    }
}

/// `AGENTS.md` rule 3, trading-domain spec §4.2 (DEC-912 item 2): a missing broker or
/// independent quote refuses the opening: an opening already requires a fresh IEX quote, and a
/// check that cannot be made is not passed.
#[test]
fn a_missing_quote_refuses_the_opening() {
    let collar = Fraction::parse("0.01").expect("the collar fixture parses");
    let broker = Some(Price::parse("100").expect("the broker ask parses"));
    let iex = Some(Price::parse("100").expect("the iex ask parses"));
    for read in [(broker, None), (None, iex), (None, None)] {
        let refused = cross_check(read.0, read.1, collar);
        assert_eq!(refused, Err(QuoteRefusal::Absent), "{read:?}");
    }
}

/// LT-10's first half (DEC-912 item 3): the run may end when the entry is terminal and any
/// filled quantity is covered — a stop resting over the whole of it, or nothing filled at all.
#[test]
fn a_terminal_covered_or_flat_entry_may_end_the_run() {
    let covered = Watched {
        entry_terminal: true,
        uncovered: false,
        alerted: None,
    };
    let ended = lt10(&covered, at(NOW_S), at(CAP_S));
    assert_eq!(ended, Ok(()));
}

/// LT-10 (DEC-912 item 3): a filled quantity left with no protection resting over it keeps the
/// run reading — inside its bounds, unsettled.
#[test]
fn an_uncovered_quantity_keeps_the_run_reading() {
    let uncovered = Watched {
        entry_terminal: true,
        uncovered: true,
        alerted: None,
    };
    let held = lt10(&uncovered, at(NOW_S), at(CAP_S));
    assert_eq!(held, Err(Lt10Refusal::Unsettled));
}

/// LT-10's second half (DEC-912 item 3): a filled quantity left uncovered may end the run
/// only once the unprotected interval is journaled and its alert key — the executor's
/// `unprotected_interval_limit` — is in the run's report.
#[test]
fn an_uncovered_quantity_ends_only_with_its_alert_key_reported() {
    let alerted = Watched {
        entry_terminal: true,
        uncovered: true,
        alerted: Some("unprotected_interval_limit"),
    };
    let ended = lt10(&alerted, at(NOW_S), at(CAP_S));
    assert_eq!(ended, Ok(()));
}

/// LT-10's second half (DEC-912 item 3): only the unprotected interval's own alert key
/// releases the uncovered run — the executor's other protection alert (`stop_watchdog`, the
/// notifications spec §4.2 `protection` row's other key) is another alarm's key, and a report
/// holding it still shows nothing for **this** interval, so the run keeps reading.
#[test]
fn another_interval_alert_key_holds_the_run_open() {
    let wrong_key = Watched {
        entry_terminal: true,
        uncovered: true,
        alerted: Some("stop_watchdog"),
    };
    let held = lt10(&wrong_key, at(NOW_S), at(CAP_S));
    assert_eq!(
        held,
        Err(Lt10Refusal::Unsettled),
        "the run ends only on the unprotected interval's own key"
    );
}

/// LT-10 (DEC-912 item 3): the entry must be terminal before the run may end, whatever else
/// the reads say, so an entry still working keeps the run reading.
#[test]
fn an_entry_still_working_keeps_the_run_reading() {
    let working = Watched {
        entry_terminal: false,
        uncovered: false,
        alerted: Some("unprotected_interval_limit"),
    };
    let held = lt10(&working, at(NOW_S), at(CAP_S));
    assert_eq!(held, Err(Lt10Refusal::Unsettled));
}

/// DEC-853 item 5's cap ([DEC-912](../../../docs/project/decisions/DEC-912.md) item 3): the
/// reads fail closed past the session's end — a watch whose LT-10 does not hold never reads
/// again and refuses — and a watch whose LT-10 holds ends clean at it, needing no further
/// read.
#[test]
fn the_reads_fail_closed_past_the_runs_cap() {
    let unsettled = Watched {
        entry_terminal: true,
        uncovered: true,
        alerted: None,
    };
    let settled = Watched {
        entry_terminal: true,
        uncovered: true,
        alerted: Some("unprotected_interval_limit"),
    };
    let refused = lt10(&unsettled, at(CAP_S), at(CAP_S));
    let ended = lt10(&settled, at(CAP_S), at(CAP_S));
    assert_eq!(
        (refused, ended),
        (Err(Lt10Refusal::Capped), Ok(())),
        "no read past the cap, and no error where nothing is left to read"
    );
}

/// The founder's run's step 6, journal spec §2 (DEC-912 item 4): a placing run ends holding the
/// three streams its export reads, in its order — the agent stream, the account stream, the
/// control stream — named as the journal names them, so `mandate journal export` takes each
/// name as it is.
#[test]
fn a_placing_run_ends_holding_the_streams_its_export_reads() {
    let held = evidence(
        Some("the workspace journal the run appended to"),
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
        held,
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

/// DEC-912 item 4: the stream names derive from the run's own workspace, agent and account ref —
/// the evidence of another run's own names names another run's streams, so no constant can stand
/// where the arguments belong.
#[test]
fn the_streams_derive_from_the_runs_own_names() {
    let held = evidence(
        Some("the rehearsal journal the run appended to"),
        "ws_rehearsal",
        "agent_rh",
        "acct_rh",
    );
    let derived = [
        "agent:ws_rehearsal:agent_rh".to_owned(),
        "acct:ws_rehearsal:acct_rh".to_owned(),
        "ctl:ws_rehearsal".to_owned(),
    ];
    assert_eq!(held, Ok(Evidence { streams: derived }));
}

/// DEC-912 item 4: a run that kept no journal — the dry run — wrote nothing the export could
/// read, so it names no stream and exports nothing.
#[test]
fn a_run_keeping_no_journal_exports_nothing() {
    let refused = evidence(None, "ws_live", "agent_01", "acct_live");
    assert_eq!(refused, Err(EvidenceRefusal::JournalAbsent));
}

/// ES-09 and the founder's run's step 4: what each refusal prints and answers, and the stub's
/// own report beside them (DEC-137), so no `code()` or message drifts.
#[test]
fn every_live_refusal_names_itself_and_its_code() {
    let grants = [
        (
            GrantRefusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            GrantRefusal::Unanswered,
            "the ask waits unanswered",
            "approval_unanswered",
        ),
        (
            GrantRefusal::Mismatched,
            "the answer repeats another content hash, so nothing is sent",
            "content_mismatch",
        ),
        (
            GrantRefusal::Denied,
            "the ask was skipped, so nothing is sent",
            "approval_skipped",
        ),
        (
            GrantRefusal::TimedOut,
            "the ask reached its deadline with no grant",
            "approval_timed_out",
        ),
    ];
    for (refusal, printed, code) in grants {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
    let quotes = [
        (
            QuoteRefusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            QuoteRefusal::Absent,
            "a quote the cross-check reads is missing",
            "quote_absent",
        ),
        (
            QuoteRefusal::Disagreement,
            "the broker's quote disagrees with the independent quote beyond the collar",
            "quote_disagreement",
        ),
    ];
    for (refusal, printed, code) in quotes {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
    let watched = [
        (
            Lt10Refusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            Lt10Refusal::Unsettled,
            "the run cannot end: the entry or its protection is unsettled",
            "lt10_unsettled",
        ),
        (
            Lt10Refusal::Capped,
            "the watch reached the session's cap with the entry unsettled",
            "watch_cap_reached",
        ),
    ];
    for (refusal, printed, code) in watched {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
    let exports = [
        (
            EvidenceRefusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            EvidenceRefusal::JournalAbsent,
            "the run kept no journal, so no stream exports",
            "export_journal_absent",
        ),
    ];
    for (refusal, printed, code) in exports {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}
