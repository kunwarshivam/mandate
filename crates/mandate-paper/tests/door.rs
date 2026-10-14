//! The live door in the run (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1a wiring row): the run-level
//! frames the runner calls the merged connection seams through — the connection chosen from
//! the connection record, the account stream the lapse reads, and DEC-529 item 3's
//! flat-and-unspent check — with the run's own derivations (the same records the control-stream
//! read held, the run's own workspace and account ref) and the exact refusal each stop reports
//! (DEC-918, the row's readings). Every pending test fails on its stub until the row's
//! implementation lands (DEC-77); the fixtures are recorded records and journaled rows that
//! carry no credential and reach no host (ES-19, LT-1, `AGENTS.md` rule 8). Like
//! `tests/connection.rs`'s merged pins (the settled #1297 reading), CI builds this suite in the
//! default build alone, so the pin that asserts the default build's `connector_not_built` arm
//! fails under a locally built `--features live` by design, and no CI job runs it there.

#[path = "../../mandate-shell/tests/common/mod.rs"]
mod common;

use common::{json, model, spy_mandate};
use mandate_canon::Digest;
use mandate_domain::Environment;
use mandate_journal::{StoredEvent, StreamType};
use mandate_paper::{
    Broker, Connection, Held, LapseRefusal, PaperError, account_stream, chosen, unspent,
};
use mandate_shell::control::ControlRecord;
use mandate_shell::{Cause, ShellError, Stage};
use mandate_spec::Mandate;

/// The account stream the run uses and its records bind: an opaque id (TI-8).
const ACCOUNT: &str = "acct_live";

/// The workspace the run reads the control stream of (journal spec §2's `ctl:{workspace}`).
const WORKSPACE: &str = "ws_live";

/// The run's account stream the recorded rows land on (journal spec §2).
const ACCOUNT_STREAM: &str = "acct:ws_live:acct_live";

/// The live connection the confirmed mandate names, as the door chose it: the state the
/// unspent frame's fixtures pin, over the account stream the record binds.
fn live_connection() -> Connection {
    Connection {
        broker: Broker::Robinhood,
        environment: Environment::Live,
        account_ref: ACCOUNT.to_owned(),
    }
}

/// The live mandate the run confirmed: the spy fixture moved to `live`, on a connection of its
/// own (the ids are opaque and name no broker or host; the ticker is the fixture's, not a
/// product choice).
fn live_mandate() -> Mandate {
    let text = spy_mandate(&model())
        .replace(r#""paper""#, r#""live""#)
        .replace("conn_alpaca_paper_01", "conn_rh_live_01");
    Mandate::parse(&json(&text)).expect("the live fixture mandate parses")
}

/// The paper mandate the run confirmed (the fixture unedited, on its own connection).
fn paper_mandate() -> Mandate {
    Mandate::parse(&json(&spy_mandate(&model()))).expect("the paper fixture mandate parses")
}

/// One control-stream record at `seq`.
fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        seq,
        event_type: event_type.to_owned(),
        payload: json(payload),
    }
}

/// The CLI's committed establishment of `connection` (journal spec §9.8, version 2): the broker
/// and environment the run matches, and the account stream it binds.
fn established(
    seq: u64,
    connection: &str,
    broker: &str,
    environment: &str,
    account: &str,
) -> ControlRecord {
    let payload = format!(
        r#"{{"account_ref":"{account}","broker":"{broker}","connection_id":"{connection}","environment":"{environment}","margin_attestation":"cash_account","scopes":["get_accounts","place_equity_order","review_equity_order"],"step_up":{{"assertion_id":"cli-assertion-1","authenticated_at":"2026-10-10T15:00:00Z","method":"cli_confirm"}},"user":"user_founder"}}"#
    );
    record(seq, "ConnectionEstablished", &payload)
}

/// The revocation of `connection` the control stream holds (journal spec §9.2).
fn revoked(seq: u64, connection: &str) -> ControlRecord {
    let payload = format!(r#"{{"connection_id":"{connection}"}}"#);
    record(seq, "ConnectionRevoked", &payload)
}

/// One row of the stream `stream_id` (journal spec §2's forms), at `seq`: an envelope shape the
/// journal could have sealed, where the door's spend read takes the event type and the stream
/// alone.
fn row_on(stream_id: &str, seq: u64, event_type: &str) -> StoredEvent {
    StoredEvent {
        stream_id: stream_id.to_owned(),
        seq,
        event_id: format!("{:0>26}", seq),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "live".to_owned(),
        recorded_at: "2026-10-13T15:05:00Z".to_owned(),
        prev_hash: Digest::of(&[]),
        hash: Digest::of(&[]),
        body: b"{}".to_vec(),
    }
}

/// The connection the run uses, with the door's refusal printed beside it: the exact message the
/// run's report holds (ES-09), so a frame that answers with any other refusal than the one this
/// stop owes fails its pin.
fn through_the_door(records: &[ControlRecord], mandate: &Mandate) -> Result<Connection, String> {
    chosen(records, mandate, ACCOUNT).map_err(|stopped| stopped.to_string())
}

/// The task file's row G1a — "The connector chosen from the connection record and the
/// environment from the confirmed mandate" — V-001's paper half and LT-14, through the frame the
/// run calls (DEC-918 item 1): a paper mandate whose connection no record names keeps today's
/// run exactly — the default build's Alpaca connector, the mandate's own paper environment, over
/// the account stream the run was given — and an agreeing Alpaca-paper record changes nothing
/// the run does.
#[test]
fn a_paper_run_through_the_door_keeps_todays_connection() {
    let expected = Connection {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        account_ref: ACCOUNT.to_owned(),
    };
    let with_no_record = through_the_door(&[], &paper_mandate());
    assert_eq!(
        with_no_record,
        Ok(expected.clone()),
        "V-001's paper default keeps today's run, the run's own account stream carried"
    );
    let mandate = paper_mandate();
    let agreeing = vec![established(
        1,
        mandate.connection_id.as_str(),
        "alpaca",
        "paper",
        ACCOUNT,
    )];
    let recorded = through_the_door(&agreeing, &mandate);
    assert_eq!(
        recorded,
        Ok(expected),
        "an agreeing paper record changes nothing the run does"
    );
}

/// DEC-910 item 2 — "These checks refuse before anything runs, ahead of the shell's own
/// `DeploymentRefusal::Live`" — and ES-09, through the frame the run calls: the door names its
/// own refusal where the shell's fold would answer a V-rule or `live`, and the run's report
/// carries each door refusal verbatim — a revoked record, a record binding another account
/// stream, and LT-1's runtime build test (a live record choosing the Robinhood connector this
/// build does not hold, the arm a locally built `--features live` turns by design).
#[test]
fn the_door_names_its_own_refusal_where_the_shell_would_answer_live() {
    let mandate = paper_mandate();
    let id = mandate.connection_id.as_str();
    let revoked_paper = vec![
        established(1, id, "alpaca", "paper", ACCOUNT),
        revoked(2, id),
    ];
    assert_eq!(
        through_the_door(&revoked_paper, &mandate),
        Err("the connection the mandate names was revoked".to_owned()),
        "a revoked paper record refuses through the door, named exactly"
    );
    let foreign_account = vec![established(1, id, "alpaca", "paper", "acct_other")];
    assert_eq!(
        through_the_door(&foreign_account, &mandate),
        Err("the record binds another account stream than the run's".to_owned()),
        "a record binding another account stream refuses before anything runs"
    );
    let live = live_mandate();
    let live_id = live.connection_id.as_str();
    let not_built = vec![established(1, live_id, "robinhood", "live", ACCOUNT)];
    assert_eq!(
        through_the_door(&not_built, &live),
        Err("the record chooses a connector this build does not hold".to_owned()),
        "LT-1's runtime seam: the default build holds no Robinhood connector, so the run's
         report names the build, never `live`"
    );
}

/// DEC-918 item 3 and DEC-910 item 3 — "The runner reads the account stream through the same
/// journal it reads the control stream from" — journal spec §2, through the frame the run
/// calls: the stream whose rows the lapse reads derives from the run's own workspace and
/// account ref — `acct:{workspace_id}:{account_ref}` — so it parses as a §2 stream id of the
/// account type, and another run's names name another run's stream, never a constant where the
/// run's own belong.
#[test]
fn the_lapse_reads_the_account_stream_derived_from_the_runs_own_names() {
    let bound = account_stream(WORKSPACE, ACCOUNT);
    let stream = bound.expect("the run's own names name a §2 stream");
    assert_eq!(stream.as_str(), ACCOUNT_STREAM);
    assert_eq!(
        stream.stream_type(),
        StreamType::Account,
        "{ACCOUNT_STREAM} is a §2 account stream"
    );
    let other_run = account_stream("ws_other", "acct_other")
        .expect("another run's names name its own §2 stream");
    assert_eq!(
        other_run.as_str(),
        "acct:ws_other:acct_other",
        "the names derive, so no run reads another run's account stream"
    );
}

/// `AGENTS.md` rule 3 and DEC-918 item 3: a form journal spec §2 cannot hold — a workspace
/// whose segments the journal's stream ids refuse — is no stream a run could read, so the
/// naming fails closed with the absent stream the connection binds, at the stage the run
/// reconciles the account's own truth, never a fallback onto another stream the id could be
/// smashed into.
#[test]
fn a_stream_the_journal_cannot_form_refuses_the_run() {
    let refused = account_stream("ws:live", ACCOUNT);
    assert!(
        matches!(
            refused,
            Err(PaperError::Shell(ShellError::Refused {
                stage: Stage::Reconcile,
                cause: Cause::Absent {
                    what: "the account stream the connection binds"
                }
            }))
        ),
        "{refused:?}: no §2 form, no read, never another stream"
    );
}

/// DEC-529 item 3 — "the live build refuses to start unless the account is flat ... and the
/// connection record is unspent: the first `place_equity_order` that fills spends it" — DEC-910
/// item 3, through the frame the run calls: the rows are the journal's own read of the account
/// stream the record binds, so a fill there spends the record and a fill on any other stream of
/// the same journal spends nothing — the run's report carrying the door's refusal verbatim.
#[test]
fn a_fill_on_the_runs_own_account_stream_spends_the_record_through_the_door() {
    let live = live_connection();
    let flat = Held {
        positions: 0,
        open_orders: 0,
    };
    let spent = vec![row_on(ACCOUNT_STREAM, 1, "FillApplied")];
    let refused = unspent(&live, &spent, &flat).map_err(|stopped| stopped.to_string());
    assert_eq!(
        refused,
        Err("the connection record is spent".to_owned()),
        "a fill on the account stream the record binds spends it"
    );
    let elsewhere = vec![row_on("acct:ws_live:acct_other", 1, "FillApplied")];
    let clean = unspent(&live, &elsewhere, &flat).map_err(|stopped| stopped.to_string());
    assert_eq!(
        clean,
        Ok(()),
        "a fill on a stream the record does not bind spends nothing"
    );
}

/// DEC-470 item 1 (DEC-910 item 3): the flat half of the lapse refuses ahead of the spend,
/// whatever the record's stream holds, and the counts are carried as data through the frame the
/// run calls — a snapshot's positions and working orders reach the refusal exactly as the
/// connector answered them, re-derived by nothing (a frame that re-counted the rows beside the
/// executor's own fold would drift, and its counts would fail here).
#[test]
fn the_flat_half_refuses_ahead_of_the_spend_with_the_snapshots_own_counts() {
    let live = live_connection();
    let held = Held {
        positions: 2,
        open_orders: 0,
    };
    let spent = vec![row_on(ACCOUNT_STREAM, 1, "FillApplied")];
    let refused = unspent(&live, &spent, &held);
    assert!(
        matches!(
            refused,
            Err(PaperError::Lapse(LapseRefusal::NotFlat {
                positions: 2,
                open_orders: 0
            }))
        ),
        "{refused:?}: the flat half refuses first, the snapshot's own counts carried"
    );
}

/// LT-14 and DEC-910 item 3 — "the lapse checks DEC-529 item 3's pair for a live run alone; a
/// paper run makes no check" — through the frame the run calls: a paper connection carries no
/// flat refusal and no spent refusal, whatever the counts its snapshot answered and whatever
/// fills the stream it reads holds, so the door changes no Alpaca paper outcome: today's paper
/// path starts exactly as it did.
#[test]
fn a_paper_connection_makes_no_lapse_check_through_the_door() {
    let paper = Connection {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        account_ref: ACCOUNT.to_owned(),
    };
    let held = Held {
        positions: 2,
        open_orders: 3,
    };
    let spent = vec![row_on(ACCOUNT_STREAM, 1, "FillApplied")];
    let clean = unspent(&paper, &[], &held).map_err(|stopped| stopped.to_string());
    let through_a_fill = unspent(&paper, &spent, &held).map_err(|stopped| stopped.to_string());
    assert_eq!(
        (clean, through_a_fill),
        (Ok(()), Ok(())),
        "a paper run makes no check, whatever it holds"
    );
}
