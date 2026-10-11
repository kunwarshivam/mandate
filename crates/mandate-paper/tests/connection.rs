//! The run's connection and DEC-529 item 3's lapse (E7-26, the
//! [first live trade brief](../../../docs/project/tasks/first-live-trade.md)'s G1a): the
//! connector chosen from the connection record, the environment from the confirmed mandate, and
//! the flat-and-unspent pair a live run requires. Every pending test fails on its stub until
//! E7-26's implementation lands (DEC-77); the fixtures are recorded records and rows that carry
//! no credential and reach no host (ES-19, LT-1, AGENTS.md rule 8).

#[path = "../../mandate-shell/tests/common/mod.rs"]
mod common;

use common::{json, model, spy_mandate};
use mandate_canon::Digest;
use mandate_domain::Environment;
use mandate_journal::StoredEvent;
use mandate_paper::{Broker, Connection, ConnectionRefusal, Held, LapseRefusal, connection, lapse};
use mandate_shell::control::ControlRecord;
use mandate_spec::Mandate;

/// The account stream the run uses and its records bind: an opaque id (TI-8).
const ACCOUNT: &str = "acct_live";

/// The live connection the confirmed mandate names, as the CLI recorded it (journal spec §9.8,
/// version 2): the environment the run's mandate states, the hold the lapse checks, and proof
/// of a fill under the record that spent it.
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

/// `mandate connection record`'s committed establishment of `connection` (K1b): the broker and
/// environment the run matches, the account stream it binds, and the step-up evidence.
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

/// One row of the account stream the record binds, at `seq`: an envelope shape the journal could
/// have sealed, where the lapse check reads the event type alone.
fn row(seq: u64, event_type: &str) -> StoredEvent {
    StoredEvent {
        stream_id: format!("acct:ws_live:{ACCOUNT}"),
        seq,
        event_id: format!("{:0>26}", seq),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "live".to_owned(),
        recorded_at: "2026-10-10T15:05:00Z".to_owned(),
        prev_hash: Digest::of(&[]),
        hash: Digest::of(&[]),
        body: b"{}".to_vec(),
    }
}

/// ES-09: what each refusal prints and answers, so the founder's run names its exact refusal
/// (R0 step 4) and no `code()` or message drifts. The stub's own report is pinned with the rest
/// (DEC-137).
#[test]
fn every_refusal_names_itself_and_its_code() {
    let refused = [
        (
            ConnectionRefusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            ConnectionRefusal::Unrecorded,
            "the mandate's connection has no record on the control stream",
            "connection_unrecorded",
        ),
        (
            ConnectionRefusal::Revoked,
            "the connection the mandate names was revoked",
            "connection_revoked",
        ),
        (
            ConnectionRefusal::Unreadable,
            "the connection's record cannot be read",
            "connection_record_unreadable",
        ),
        (
            ConnectionRefusal::Environment {
                named: "paper".to_owned(),
            },
            "the record states environment paper, not the mandate's",
            "connection_environment",
        ),
        (
            ConnectionRefusal::Broker {
                named: "alpaca".to_owned(),
            },
            "the record names the alpaca broker, which the mandate's environment does not serve",
            "connection_broker",
        ),
        (
            ConnectionRefusal::Account,
            "the record binds another account stream than the run's",
            "connection_account",
        ),
        (
            ConnectionRefusal::NotBuilt,
            "the record chooses a connector this build does not hold",
            "connector_not_built",
        ),
    ];
    for (refusal, printed, code) in refused {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
    let lapsed = [
        (
            LapseRefusal::Unimplemented { story: "E7-26" },
            "E7-26 has not been implemented yet",
            "unimplemented",
        ),
        (
            LapseRefusal::NotFlat {
                positions: 0,
                open_orders: 0,
            },
            "the account the connection binds is not flat",
            "account_not_flat",
        ),
        (
            LapseRefusal::Spent,
            "the connection record is spent",
            "connection_spent",
        ),
    ];
    for (refusal, printed, code) in lapsed {
        assert_eq!(refusal.to_string(), printed, "{refusal:?}");
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}

/// V-001 (DEC-910 item 2): a live mandate whose connection no record on the control stream
/// establishes refuses before anything runs — no record, so no environment a run could trade
/// live under, and no connector chosen.
#[test]
#[ignore = "pending E7-26"]
fn a_live_mandate_with_no_record_refuses_before_anything_runs() {
    let refused = connection(&[], &live_mandate(), ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Unrecorded));
}

/// V-001's paper half, LT-14 and DEC-910 item 2: a paper mandate whose connection no record
/// names keeps today's run — the paper adapter's own connector and environment, over the
/// account it was given — so the default build's paper path changes nothing.
#[test]
#[ignore = "pending E7-26"]
fn a_paper_mandate_without_a_record_keeps_todays_path() {
    let agreed = connection(&[], &paper_mandate(), ACCOUNT);
    let expected = Connection {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        account_ref: ACCOUNT.to_owned(),
    };
    assert_eq!(agreed, Ok(expected));
}

/// LT-1 and DEC-529 item 3 (DEC-910 item 1): the default build holds no Robinhood host or
/// connector, so the live record that chooses one refuses here — the `live` feature is the only
/// door, and no run of this build can reach Robinhood. CI's only build of this suite is the
/// default one; E7-26's implementation, which declares the feature, gates this test where it
/// adds the `live` arm.
#[test]
#[ignore = "pending E7-26"]
fn a_live_record_refuses_the_connector_this_build_does_not_hold() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "robinhood", "live", ACCOUNT)];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::NotBuilt));
}

/// V-001 (DEC-910 item 2): the environment is the confirmed mandate's, so a record that states
/// another one cannot move the run to it — a paper mandate named by a live record refuses
/// rather than trading live under a deployment confirmed for paper.
#[test]
#[ignore = "pending E7-26"]
fn a_record_of_another_environment_refuses() {
    let mandate = paper_mandate();
    let id = mandate.connection_id.as_str();
    for named in ["live", "backtest"] {
        let records = vec![established(1, id, "robinhood", named, ACCOUNT)];
        let refused = connection(&records, &mandate, ACCOUNT);
        assert_eq!(
            refused,
            Err(ConnectionRefusal::Environment {
                named: named.to_owned()
            }),
            "{named}"
        );
    }
}

/// DEC-529 item 3 (DEC-910 item 2): a live mandate is served by the Robinhood connector alone,
/// so a record naming Alpaca refuses, and so does one naming a broker no connector serves at
/// all.
#[test]
#[ignore = "pending E7-26"]
fn a_record_naming_another_broker_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    for named in ["alpaca", "kraken_derivatives_us"] {
        let records = vec![established(1, id, named, "live", ACCOUNT)];
        let refused = connection(&records, &mandate, ACCOUNT);
        assert_eq!(
            refused,
            Err(ConnectionRefusal::Broker {
                named: named.to_owned()
            }),
            "{named}"
        );
    }
}

/// DEC-910 item 2: the account stream the record binds must be the run's own, so a record
/// binding another account refuses before anything runs, wherever else it agrees.
#[test]
#[ignore = "pending E7-26"]
fn a_record_of_another_account_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "robinhood", "live", "acct_other")];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Account));
}

/// Journal spec §9.8 (DEC-910 item 2): a connection whose last record is its revocation serves
/// no run, however its establishment read — the later revocation is decided before any member
/// of the establishment is parsed, and only a re-establishment after it counts.
#[test]
#[ignore = "pending E7-26"]
fn a_revoked_connection_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![
        established(1, id, "robinhood", "live", ACCOUNT),
        revoked(2, id),
    ];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
}

/// DEC-910 item 2: the paper half refuses a revoked record too — a paper mandate whose
/// connection was recorded and then revoked is not covered by V-001's no-record default, and
/// the shell's own fold (V-001) and this seam agree on the refusal.
#[test]
#[ignore = "pending E7-26"]
fn a_revoked_paper_record_refuses_the_paper_run_too() {
    let mandate = paper_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![
        established(1, id, "alpaca", "paper", ACCOUNT),
        revoked(2, id),
    ];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
}

/// `AGENTS.md` rule 3 and DEC-910 item 2: a record a member the run reads is missing from, or
/// that holds it in another shape, refuses as unreadable — a version 1 record or a corrupt
/// payload, never a guess at what it meant.
#[test]
#[ignore = "pending E7-26"]
fn a_record_that_cannot_be_read_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let partial = format!(r#"{{"connection_id":"{id}","environment":"live"}}"#);
    let records = vec![record(1, "ConnectionEstablished", &partial)];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Unreadable));
}

/// LT-14 and DEC-910 item 2: a paper mandate whose record agrees — Alpaca, paper, the run's own
/// account stream — keeps the paper adapter's path and connector, so a recorded paper
/// connection changes nothing the run does.
#[test]
#[ignore = "pending E7-26"]
fn an_agreeing_paper_record_proceeds_unchanged() {
    let mandate = paper_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "alpaca", "paper", ACCOUNT)];
    let agreed = connection(&records, &mandate, ACCOUNT);
    assert_eq!(
        agreed.map(|c| (c.broker, c.environment)),
        Ok((Broker::Alpaca, Environment::Paper))
    );
}

/// DEC-529 items 1 and 3 (DEC-910 item 3): a spent record deploys nothing — the account stream
/// it binds holds the fill of the order that spent it, an executor's `FillApplied` and a
/// reconciliation's late fill alike, so the same build cannot start another live run on it
/// however the account reads now.
#[test]
#[ignore = "pending E7-26"]
fn a_spent_record_deploys_nothing() {
    let rows = [row(1, "TradingDayStarted"), row(3, "FeesCharged")];
    let flat = Held {
        positions: 0,
        open_orders: 0,
    };
    for filled in ["FillApplied", "LateFillApplied"] {
        let mut rows = rows.to_vec();
        rows.insert(1, row(2, filled));
        let refused = lapse(&live_connection(), &rows, &flat);
        assert_eq!(refused, Err(LapseRefusal::Spent), "{filled}");
    }
}

/// DEC-470 item 1 (DEC-910 item 3): a live run refuses an account that already holds a
/// position or a working order, on the spot and ahead of the spend: the flat half of the lapse
/// refuses first, whatever the record's stream holds.
#[test]
#[ignore = "pending E7-26"]
fn a_non_flat_account_refuses_to_start() {
    for held in [
        Held {
            positions: 1,
            open_orders: 0,
        },
        Held {
            positions: 0,
            open_orders: 1,
        },
    ] {
        let spent = vec![row(2, "FillApplied")];
        let refusal = LapseRefusal::NotFlat {
            positions: held.positions,
            open_orders: held.open_orders,
        };
        let clean = lapse(&live_connection(), &[], &held);
        let refused = lapse(&live_connection(), &spent, &held);
        assert_eq!((clean, refused), (Err(refusal.clone()), Err(refusal)));
    }
}

/// DEC-529 item 3 (DEC-910 item 3): the pair that passes — a flat account and an unspent
/// record — is the one that proceeds to the builder and the gate, where the cycle's own
/// one-order door (FT-1) still sends at most one order; nothing the account stream holds short
/// of a fill holds it back, a working order's submission and its cancellation included.
#[test]
#[ignore = "pending E7-26"]
fn an_unspent_record_on_a_flat_account_proceeds_to_the_builder_and_gate() {
    let rows = [
        row(1, "StreamOpened"),
        row(2, "TradingDayStarted"),
        row(3, "OrderSubmitted"),
        row(4, "OrderStateChanged"),
        row(5, "OrderAbandoned"),
        row(6, "FeesCharged"),
        row(7, "AccountSnapshotRecorded"),
    ];
    let flat = Held {
        positions: 0,
        open_orders: 0,
    };
    let clean = lapse(&live_connection(), &rows, &flat);
    assert_eq!(clean, Ok(()));
}
