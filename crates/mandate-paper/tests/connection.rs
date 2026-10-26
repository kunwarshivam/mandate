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
use mandate_shell::control::{ControlRecord, Origin};
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

/// What a live record choosing the Robinhood connector gets in the build this suite runs in
/// (LT-1, DEC-910 item 1, DEC-963): a build without `live` (the default one, or `rh-sim` alone)
/// holds no Robinhood connector, so the record refuses `connector_not_built`; a build with
/// `live` (the founder's, or the rehearsal's `live,rh-sim`, DEC-922) serves it as
/// [`live_connection`]. The oracle is the suite's own `cfg`, never the crate's door constant, so
/// a door that opens in a build without `live`, or stays shut in one with it, fails here.
fn served_live_record() -> Result<Connection, ConnectionRefusal> {
    if cfg!(feature = "live") {
        Ok(live_connection())
    } else {
        Err(ConnectionRefusal::NotBuilt)
    }
}

/// The paper mandate the run confirmed (the fixture unedited, on its own connection).
fn paper_mandate() -> Mandate {
    Mandate::parse(&json(&spy_mandate(&model()))).expect("the paper fixture mandate parses")
}

/// One control-stream record at `seq`.
fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        seq,
        event_time: None,
        event_type: event_type.to_owned(),
        payload: json(payload),
        origin: Origin {
            stream_id: "ctl:ws".to_owned(),
            environment: Environment::Paper,
        },
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
    record(
        seq,
        "ConnectionEstablished",
        &established_payload(connection, broker, environment, account),
    )
}

/// The establishment record's full version-2 member set (journal spec §9.8), the text the CLI
/// commits, so a fixture can hold one member of it in the shape the tests corrupt.
fn established_payload(connection: &str, broker: &str, environment: &str, account: &str) -> String {
    format!(
        r#"{{"account_ref":"{account}","broker":"{broker}","connection_id":"{connection}","environment":"{environment}","margin_attestation":"cash_account","scopes":["get_accounts","place_equity_order","review_equity_order"],"step_up":{{"assertion_id":"cli-assertion-1","authenticated_at":"2026-10-10T15:00:00Z","method":"cli_confirm"}},"user":"user_founder"}}"#
    )
}

/// The revocation of `connection` the control stream holds (journal spec §9.2).
fn revoked(seq: u64, connection: &str) -> ControlRecord {
    let payload = format!(r#"{{"connection_id":"{connection}"}}"#);
    record(seq, "ConnectionRevoked", &payload)
}

/// One row of the account stream the record binds, at `seq`: an envelope shape the journal could
/// have sealed, where the lapse check reads the event type alone.
fn row(seq: u64, event_type: &str) -> StoredEvent {
    row_on(&format!("acct:ws_live:{ACCOUNT}"), seq, event_type)
}

/// One row of the stream `stream_id` (journal spec §2's forms), at `seq`: the same envelope
/// shape `row` seals, so the stream a fill lands on is the only thing a fixture varies.
fn row_on(stream_id: &str, seq: u64, event_type: &str) -> StoredEvent {
    StoredEvent {
        stream_id: stream_id.to_owned(),
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
        (
            LapseRefusal::NotSupplied,
            "the lapse was handed no account-stream read and no account snapshot",
            "lapse_evidence_not_supplied",
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
fn a_live_mandate_with_no_record_refuses_before_anything_runs() {
    let refused = connection(&[], &live_mandate(), ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Unrecorded));
}

/// V-001's paper half, LT-14 and DEC-910 item 2: a paper mandate whose connection no record
/// names keeps today's run — the paper adapter's own connector and environment, over the
/// account it was given — so the default build's paper path changes nothing.
#[test]
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
/// connector, so the live record that chooses one refuses there — the `live` feature is the only
/// door, and no run of a build without it can reach Robinhood. A build with `live` holds the
/// connector, so there the same record is served, over the account it binds; the expectation is
/// [`served_live_record`]'s, so the test holds in every build and fails in each on a door that
/// does not match the build (DEC-963, which replaces #1297's reading that it fails under
/// `live` by design).
#[test]
fn a_live_record_refuses_the_connector_this_build_does_not_hold() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "robinhood", "live", ACCOUNT)];
    let chosen = connection(&records, &mandate, ACCOUNT);
    assert_eq!(chosen, served_live_record());
}

/// V-001 (DEC-910 item 2): the environment is the confirmed mandate's, so a record that states
/// another one cannot move the run to it — a paper mandate named by a live record refuses
/// rather than trading live under a deployment confirmed for paper.
#[test]
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
fn a_record_of_another_account_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "robinhood", "live", "acct_other")];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Account));
}

/// DEC-910 item 2: the order the refusals fail closed in is the order the DEC lists them, and
/// the environment disagreement is decided before the broker's and the account stream's, the
/// broker's before the account stream's — so a record that disagrees on all three refuses
/// `connection_environment`, and one that agrees on the environment but names a broker the
/// mandate's environment does not serve and binds another account stream refuses
/// `connection_broker` before the account is judged.
#[test]
fn the_environment_broker_and_account_refusals_keep_the_recorded_order() {
    let paper = paper_mandate();
    let id = paper.connection_id.as_str();
    let records = vec![established(1, id, "robinhood", "live", "acct_other")];
    let refused = connection(&records, &paper, ACCOUNT);
    assert_eq!(
        refused,
        Err(ConnectionRefusal::Environment {
            named: "live".to_owned(),
        })
    );
    let live = live_mandate();
    let id = live.connection_id.as_str();
    let records = vec![established(1, id, "alpaca", "live", "acct_other")];
    let refused = connection(&records, &live, ACCOUNT);
    assert_eq!(
        refused,
        Err(ConnectionRefusal::Broker {
            named: "alpaca".to_owned(),
        })
    );
}

/// Journal spec §9.8 (DEC-910 item 2): a connection whose last record is its revocation serves
/// no run, however its establishment read — the later revocation is decided before any member
/// of the establishment is parsed, and only a re-establishment after it counts.
#[test]
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

/// DEC-910 item 2: the revocation refusal's qualifier is whose connection holds the record —
/// "any mandate whose connection has a revoked record" — so a `ConnectionRevoked` of a different
/// connection never refuses this one, wherever the stream holds it beside this connection's own
/// establishment: the paper run keeps its recorded path and the live record gets only what its
/// build serves it ([`served_live_record`]), never a refusal for a revocation that is not its
/// own.
#[test]
fn a_revocation_of_another_connection_does_not_refuse_this_one() {
    let paper = paper_mandate();
    let id = paper.connection_id.as_str();
    let foreign_before = vec![
        revoked(1, "conn_other_01"),
        established(2, id, "alpaca", "paper", ACCOUNT),
    ];
    let foreign_after = vec![
        established(1, id, "alpaca", "paper", ACCOUNT),
        revoked(2, "conn_other_01"),
    ];
    for records in [foreign_before, foreign_after] {
        let agreed = connection(&records, &paper, ACCOUNT);
        assert_eq!(
            agreed,
            Ok(Connection {
                broker: Broker::Alpaca,
                environment: Environment::Paper,
                account_ref: ACCOUNT.to_owned(),
            })
        );
    }
    let live = live_mandate();
    let id = live.connection_id.as_str();
    let records = vec![
        established(1, id, "robinhood", "live", ACCOUNT),
        revoked(2, "conn_other_01"),
    ];
    let chosen = connection(&records, &live, ACCOUNT);
    assert_eq!(chosen, served_live_record());
}

/// DEC-910 item 2 and journal spec §9.8 rule 66: the revocation's reach ends at a later
/// establishment — the refusal is for "a revoked record and no later establishment", and the
/// re-establishment rule 66 allows (a second `ConnectionEstablished` only after the
/// revocation, with the same broker, environment and account stream) is that later
/// establishment — so the connection is served again: the paper run proceeds on it, and the
/// live record gets only what its build serves it ([`served_live_record`]), never a refusal as
/// revoked.
#[test]
fn a_re_establishment_after_a_revocation_restores_the_connection() {
    let paper = paper_mandate();
    let id = paper.connection_id.as_str();
    let re_established = vec![
        established(1, id, "alpaca", "paper", ACCOUNT),
        revoked(2, id),
        established(3, id, "alpaca", "paper", ACCOUNT),
    ];
    let agreed = connection(&re_established, &paper, ACCOUNT);
    assert_eq!(
        agreed,
        Ok(Connection {
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            account_ref: ACCOUNT.to_owned(),
        })
    );
    let live = live_mandate();
    let id = live.connection_id.as_str();
    let re_established = vec![
        established(1, id, "robinhood", "live", ACCOUNT),
        revoked(2, id),
        established(3, id, "robinhood", "live", ACCOUNT),
    ];
    let chosen = connection(&re_established, &live, ACCOUNT);
    assert_eq!(chosen, served_live_record());
}

/// DEC-910 item 2 (journal spec §9.8: "No rule refuses a `ConnectionRevoked`"): a revocation
/// needs no establishment before it — a lone `ConnectionRevoked` is the connection's revoked
/// record, and "any mandate whose connection has a revoked record and no later establishment
/// refuses", the paper default included, so the run fails closed before anything runs, reading
/// the revocation as `connection_revoked`, never as no record.
#[test]
fn a_lone_revocation_without_any_establishment_refuses() {
    let live = live_mandate();
    let id = live.connection_id.as_str();
    let records = vec![revoked(1, id)];
    let refused = connection(&records, &live, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
    let paper = paper_mandate();
    let id = paper.connection_id.as_str();
    let records = vec![revoked(1, id)];
    let refused = connection(&records, &paper, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
}

/// DEC-910 item 2 and `AGENTS.md` rule 3: a stream holds one record at one `seq` (journal
/// spec §5.1), but a slice the run reads could disagree; an equal-`seq` establishment and
/// revocation of the connection resolve to the revocation — the safe default that refuses,
/// never a tie that imports an establishment's data over a revocation — and the outcome does
/// not depend on the order the records are read in: this order and the one after it hold the
/// same pair read both ways, and the revocation outranks the establishment in each.
#[test]
fn a_same_seq_revocation_outranks_the_establishment_read_before_it() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![
        established(7, id, "robinhood", "live", ACCOUNT),
        revoked(7, id),
    ];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
}

/// DEC-910 item 2 and `AGENTS.md` rule 3, the other half of the tie: the same equal-`seq`
/// pair read the other way round refuses exactly the same — the revocation outranks the
/// establishment wherever the slice holds it, so no input order un-refuses the connection.
#[test]
fn a_same_seq_revocation_outranks_the_establishment_read_after_it() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![
        revoked(7, id),
        established(7, id, "robinhood", "live", ACCOUNT),
    ];
    let refused = connection(&records, &mandate, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
}

/// `AGENTS.md` rule 3 and DEC-910 item 2: a record a member the run reads is missing from, or
/// that holds it in another shape, refuses as unreadable, never a guess at what it meant: a
/// record with a member missing, a version 1 record — journal spec §9.8's version-1 member set
/// holds no `account_ref`, so it is never re-established either (rule 66) — and a corrupt
/// payload whose member the run reads holds not text.
#[test]
fn a_record_that_cannot_be_read_refuses() {
    let mandate = live_mandate();
    let id = mandate.connection_id.as_str();
    let partial = format!(r#"{{"connection_id":"{id}","environment":"live"}}"#);
    let version_one = format!(
        r#"{{"broker":"robinhood","connection_id":"{id}","environment":"live","scopes":["get_accounts","place_equity_order","review_equity_order"]}}"#
    );
    let corrupt = established_payload(id, "robinhood", "live", ACCOUNT).replacen(
        &format!(r#""account_ref":"{ACCOUNT}""#),
        r#""account_ref":7"#,
        1,
    );
    for payload in [partial, version_one, corrupt] {
        let records = vec![record(1, "ConnectionEstablished", &payload)];
        let refused = connection(&records, &mandate, ACCOUNT);
        assert_eq!(refused, Err(ConnectionRefusal::Unreadable), "{payload}");
    }
}

/// DEC-910 item 2: the seven refusals fail closed in the order the DEC states them, and
/// `connection_record_unreadable` sits third in it — decided after the revocation ("the later
/// revocation is decided before any member of the establishment is parsed") and before the
/// environment — so a revoked connection with an unreadable record under it refuses
/// `connection_revoked`, and a record the run cannot read refuses `connection_record_unreadable`
/// whatever environment it also states, never a disagreement the run cannot trust itself to
/// have read.
#[test]
fn unreadable_sits_between_the_revocation_and_the_environment() {
    let live = live_mandate();
    let id = live.connection_id.as_str();
    let unreadable = format!(r#"{{"connection_id":"{id}","environment":"live"}}"#);
    let records = vec![
        record(1, "ConnectionEstablished", &unreadable),
        revoked(2, id),
    ];
    let refused = connection(&records, &live, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Revoked));
    let paper = paper_mandate();
    let id = paper.connection_id.as_str();
    let unreadable = format!(r#"{{"connection_id":"{id}","environment":"live"}}"#);
    let records = vec![record(1, "ConnectionEstablished", &unreadable)];
    let refused = connection(&records, &paper, ACCOUNT);
    assert_eq!(refused, Err(ConnectionRefusal::Unreadable));
}

/// LT-14 and DEC-910 item 2: a paper mandate whose record agrees — Alpaca, paper, the run's own
/// account stream — keeps the paper adapter's path and connector, the account stream the run was
/// given included, so a recorded paper connection changes nothing the run does.
#[test]
fn an_agreeing_paper_record_proceeds_unchanged() {
    let mandate = paper_mandate();
    let id = mandate.connection_id.as_str();
    let records = vec![established(1, id, "alpaca", "paper", ACCOUNT)];
    let agreed = connection(&records, &mandate, ACCOUNT);
    assert_eq!(
        agreed,
        Ok(Connection {
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            account_ref: ACCOUNT.to_owned(),
        })
    );
}

/// DEC-529 items 1 and 3 (DEC-910 item 3): a spent record deploys nothing — the account stream
/// it binds holds the fill of the order that spent it, an executor's `FillApplied` and a
/// reconciliation's late fill alike, so the same build cannot start another live run on it
/// however the account reads now.
#[test]
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

/// DEC-910 item 3: the fill that spends the record is "a `FillApplied` on the account stream the
/// record binds" — the spend is scoped to that stream alone, so the same fill evidence on any
/// other stream of the same journal, another account's stream or an agent stream, spends
/// nothing and the connection proceeds to the builder and the gate.
#[test]
fn a_fill_on_a_stream_the_record_does_not_bind_does_not_spend_it() {
    let flat = Held {
        positions: 0,
        open_orders: 0,
    };
    for stream in ["acct:ws_live:acct_other", "agent:ws_live:agent_01"] {
        for filled in ["FillApplied", "LateFillApplied"] {
            let rows = vec![row_on(stream, 1, filled)];
            let clean = lapse(&live_connection(), &rows, &flat);
            assert_eq!(clean, Ok(()), "{stream} {filled}");
        }
    }
}

/// DEC-910 item 3 (journal spec §2): only an account stream — `acct:{workspace_id}:{account_ref}`
/// — can spend the record, so an agent stream whose own id segment *is* the bound `account_ref`
/// spends nothing either: the stream type and the account segment name the record's stream
/// together, and a fill that matches only one of them never spends.
#[test]
fn a_fill_on_an_agent_stream_sharing_the_bound_segment_does_not_spend_it() {
    let flat = Held {
        positions: 0,
        open_orders: 0,
    };
    for filled in ["FillApplied", "LateFillApplied"] {
        let rows = vec![row_on("agent:ws_live:acct_live", 1, filled)];
        let clean = lapse(&live_connection(), &rows, &flat);
        assert_eq!(clean, Ok(()), "{filled}");
    }
}

/// DEC-470 item 1 (DEC-910 item 3): a live run refuses an account that already holds a
/// position or a working order, on the spot and ahead of the spend: the flat half of the lapse
/// refuses first, whatever the record's stream holds.
#[test]
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

/// LT-14 and DEC-910 item 3: "the lapse checks DEC-529 item 3's pair for a live run alone; a
/// paper run makes no check" — a paper connection carries no flat refusal and no spent
/// refusal, whatever the counts its snapshot answered and whatever fills the stream it reads
/// holds, so moving onto the record-reading runner changes no Alpaca paper outcome: today's
/// paper path starts exactly as it did.
#[test]
fn a_paper_run_makes_no_lapse_check() {
    let paper = Connection {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        account_ref: ACCOUNT.to_owned(),
    };
    let held = Held {
        positions: 2,
        open_orders: 3,
    };
    let spent = vec![row(1, "FillApplied")];
    let clean = lapse(&paper, &[], &held);
    let through_a_fill = lapse(&paper, &spent, &held);
    assert_eq!((clean, through_a_fill), (Ok(()), Ok(())));
}
