//! Journal spec rule 68 at the executor (DEC-2341): the account stream is connecting until it holds
//! its establishment's copy, which the executor writes; while connecting it opens no risk and every
//! risk reduction still runs (`AGENTS.md` rule 13).

mod support;

use mandate_canon::{Key, Value};
use mandate_executor::{AccountRef, AccountScope, BrokerRequest, EventId, WorkspaceId};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::establish::{Establishment, OpeningGate, ToCopy, copy_draft, establishment};
use mandate_executord::fixture::Ids;
use mandate_executord::paper::FixturePaper;
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_store::envelope::{Envelope, Fields, draft_bytes};
use mandate_store::{MemoryStore, Store};
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, Allowing, CONNECTION, CONTROL_STREAM, Error, Journal, NOON,
    OTHER_ACCOUNT, OTHER_CONNECTION, Paper, TwoPercentStop, WORKSPACE, at, deployed, object,
    opened, propose, start_wired, wired,
};

const CHECK: &str = "e1h1o1";
const ESTABLISHED: &str = "c9";

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId(WORKSPACE.to_owned()),
    }
}

/// A `ConnectionChecked` of `occasion` for `connection` on the account stream, every result
/// `result`.
fn check(
    journal: &Journal,
    id: &str,
    connection: &str,
    occasion: &str,
    result: &str,
) -> Result<(), Error> {
    let row = |check: &str| {
        object(vec![
            ("check", text(check)),
            ("reason", Value::Null),
            ("result", text(result)),
        ])
    };
    journal.write(
        ACCOUNT_STREAM,
        id,
        "ConnectionChecked",
        1,
        None,
        object(vec![
            ("account_pii_ref", Value::Null),
            ("connection_id", text(connection)),
            ("occasion", text(occasion)),
            (
                "results",
                Value::Array(vec![row("account")?, row("environment")?, row("scope")?]),
            ),
            ("risk_clock", text("2026-09-21T14:59:00.000000000Z")),
        ])?,
    )
}

/// The control services' version-2 `ConnectionEstablished`, caused by `cause`.
fn establish(
    journal: &Journal,
    id: &str,
    version: u64,
    cause: Option<&str>,
    connection: &str,
    account: &str,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "ConnectionEstablished",
        version,
        cause,
        members(connection, account)?,
    )
}

fn members(connection: &str, account: &str) -> Result<Value, Error> {
    object(vec![
        ("account_ref", text(account)),
        ("broker", text("alpaca")),
        ("connection_id", text(connection)),
        ("environment", text("paper")),
        ("margin_attestation", Value::Null),
        ("scopes", Value::Array(vec![text("data"), text("trading")])),
        ("step_up", Value::Null),
        ("user", text("usr_1")),
    ])
}

fn closing(
    journal: &Journal,
    id: &str,
    event_type: &str,
    connection: &str,
    occasion: &str,
) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        event_type,
        1,
        None,
        object(vec![
            ("connection_id", text(connection)),
            ("occasion", text(occasion)),
        ])?,
    )
}

fn state(journal: &Journal) -> Result<Establishment, Error> {
    Ok(establishment(
        &journal.rows_of(ACCOUNT_STREAM),
        &journal.rows_of(CONTROL_STREAM),
        &scope(),
        Environment::Paper,
    )?)
}

fn to_copy(original: &str) -> Result<Establishment, Error> {
    let Value::Object(members) = members(CONNECTION, ACCOUNT)? else {
        return Err("members".into());
    };
    Ok(Establishment::ToCopy(ToCopy {
        original: EventId(original.to_owned()),
        members,
    }))
}

/// Item 1: no check, or a passed one the control services have not established, is connecting;
/// the establishment its check caused, for its connection and account, is due to be copied.
#[test]
#[ignore = "pending E21-32"]
fn only_the_establishment_of_the_passed_connect_check_is_due() -> Result<(), Error> {
    let journal = Journal::default();
    opened(&journal)?;
    assert_eq!(state(&journal)?, Establishment::Connecting);
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
    assert_eq!(state(&journal)?, Establishment::Connecting);
    establish(&journal, "c1", 2, Some("another"), CONNECTION, ACCOUNT)?;
    establish(&journal, "c2", 2, Some(CHECK), OTHER_CONNECTION, ACCOUNT)?;
    establish(&journal, "c3", 2, Some(CHECK), CONNECTION, OTHER_ACCOUNT)?;
    establish(&journal, "c4", 1, Some(CHECK), CONNECTION, ACCOUNT)?;
    assert_eq!(state(&journal)?, Establishment::Connecting);
    establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
    assert_eq!(state(&journal)?, to_copy(ESTABLISHED)?);
    Ok(())
}

/// Item 1: a failed latest `connect` check is never copied, even with an establishment it caused.
#[test]
#[ignore = "pending E21-32"]
fn a_failed_check_is_never_copied() -> Result<(), Error> {
    let journal = Journal::default();
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "failed")?;
    establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
    assert_eq!(state(&journal)?, Establishment::Connecting);
    Ok(())
}

/// Items 1 and 4: a `connect` refusal or a revocation of the checked connection closes it before
/// its copy, over an establishment too; another connection's, or another occasion's, does not.
#[test]
#[ignore = "pending E21-32"]
fn a_refusal_or_revocation_closes_the_connection_before_its_copy() -> Result<(), Error> {
    for (event_type, connection, occasion, closed) in [
        ("ConnectionRefused", CONNECTION, "connect", true),
        ("ConnectionRevoked", CONNECTION, "connect", true),
        ("ConnectionRefused", OTHER_CONNECTION, "connect", false),
        ("ConnectionRefused", CONNECTION, "reauthorize", false),
    ] {
        let journal = Journal::default();
        opened(&journal)?;
        check(&journal, CHECK, CONNECTION, "connect", "passed")?;
        closing(&journal, "r1", event_type, connection, occasion)?;
        let want = if closed {
            Establishment::Closed
        } else {
            Establishment::Connecting
        };
        assert_eq!(
            state(&journal)?,
            want,
            "{event_type} {connection} {occasion}"
        );
        establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
        let want = if closed {
            Establishment::Closed
        } else {
            to_copy(ESTABLISHED)?
        };
        assert_eq!(
            state(&journal)?,
            want,
            "{event_type} {connection} {occasion}"
        );
    }
    Ok(())
}

/// Item 1: a stream holding its copy is established, and a connection record that does not read
/// refuses rather than guesses.
#[test]
#[ignore = "pending E21-32"]
fn a_copy_establishes_and_an_unreadable_record_refuses() -> Result<(), Error> {
    let journal = Journal::default();
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
    journal.write(
        ACCOUNT_STREAM,
        "e2h2o0",
        "ConnectionEstablished",
        2,
        Some(ESTABLISHED),
        members(CONNECTION, ACCOUNT)?,
    )?;
    assert_eq!(state(&journal)?, Establishment::Established);
    let unreadable = Journal::default();
    opened(&unreadable)?;
    unreadable.write(
        ACCOUNT_STREAM,
        CHECK,
        "ConnectionChecked",
        1,
        None,
        object(vec![("occasion", text("connect"))])?,
    )?;
    assert_eq!(
        establishment(
            &unreadable.rows_of(ACCOUNT_STREAM),
            &[],
            &scope(),
            Environment::Paper
        ),
        Err("connection_record_unreadable")
    );
    Ok(())
}

/// Item 2: the copy is the original's members and `risk_clock` on its whole second, with the
/// original as cause; members that already hold `risk_clock` are refused.
#[test]
#[ignore = "pending E21-32"]
fn the_copy_is_the_originals_members_and_the_risk_clock() -> Result<(), Error> {
    let Establishment::ToCopy(due) = to_copy(ESTABLISHED)? else {
        return Err("due".into());
    };
    let draft = copy_draft(
        &due,
        EventId("e5h5o0".to_owned()),
        at("2026-09-21T15:00:07.9Z")?,
    )?;
    assert_eq!(draft.event_type, "ConnectionEstablished");
    assert_eq!(draft.schema_version, 2);
    assert_eq!(draft.event_id, EventId("e5h5o0".to_owned()));
    assert_eq!(draft.causation_id, Some(EventId(ESTABLISHED.to_owned())));
    let mut want = due.members.clone();
    want.insert(
        Key::new("risk_clock")?,
        text("2026-09-21T15:00:07.000000000Z"),
    );
    assert_eq!(draft.payload, Value::Object(want.clone()));
    let twice = ToCopy {
        original: due.original.clone(),
        members: want,
    };
    assert_eq!(
        copy_draft(&twice, EventId("e".to_owned()), at(NOON)?).map(drop),
        Err("copy_refused")
    );
    Ok(())
}

/// Item 3: a shut gate withholds the binding input from openings alone, so an opening is held, not
/// refused; open, it answers as the wiring's source does, and the held opening is asked again and
/// goes with the next.
#[test]
fn a_shut_gate_withholds_only_openings() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let gate = OpeningGate::default();
    assert!(!gate.is_open());
    let guarded = gate.guard(Box::new(Allowing));
    let mut process = start_wired(&journal, &broker, wired(guarded, Box::new(TwoPercentStop))?)?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000A",
        "buy",
        "10",
        "150",
        "open",
    )?;
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    let submits = |broker: &Paper| {
        broker
            .seen()
            .iter()
            .filter(|s| matches!(s.request, BrokerRequest::Submit(_)))
            .count()
    };
    assert_eq!(submits(&broker), 0);
    gate.open();
    assert!(gate.clone().is_open());
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000B",
        "buy",
        "10",
        "150",
        "open",
    )?;
    process.step(at(NOON)?);
    assert_eq!(submits(&broker), 2, "{:?}", journal.types(ACCOUNT_STREAM));
    Ok(())
}

/// Items 3 and 6, rule 13: under a shut gate the executor opens nothing however many passes it
/// runs, while a risk exit behind the held opening is still received.
#[test]
fn a_shut_gate_holds_every_opening_and_lets_every_exit_go() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let gate = OpeningGate::default();
    let mut process = start_wired(
        &journal,
        &broker,
        wired(gate.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000A",
        "buy",
        "10",
        "150",
        "open",
    )?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000C",
        "sell",
        "10",
        "150",
        "risk_exit",
    )?;
    for _ in 0..20 {
        process.step(at(NOON)?);
    }
    assert!(!gate.is_open());
    assert_eq!(journal.received(), ["01JINTENT0000000000000000C"]);
    Ok(())
}

/// A journal and broker where the agent holds 10 AAPL: an opening taken through an open gate and
/// filled whole, so an exit has something to sell.
fn holding() -> Result<(Journal, Paper), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let open = OpeningGate::default();
    open.open();
    let mut opening = start_wired(
        &journal,
        &broker,
        wired(open.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000A",
        "buy",
        "10",
        "150",
        "open",
    )?;
    opening.step(at(NOON)?);
    for id in broker.submitted() {
        broker.fill(&id, "150")?;
    }
    for _ in 0..3 {
        opening.step(at(NOON)?);
    }
    Ok((journal, broker))
}

/// What a restarted executor, under a gate `open` or shut, journals and asks the broker for the
/// risk reduction `action` over that position.
fn reduced(action: &str, open: bool) -> Result<(Vec<String>, Vec<String>), Error> {
    let (journal, broker) = holding()?;
    let gate = OpeningGate::default();
    if open {
        gate.open();
    }
    let mut process = start_wired(
        &journal,
        &broker,
        wired(gate.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    let exit = |purpose| {
        propose(
            &journal,
            AGENT,
            "01JINTENT0000000000000000C",
            "sell",
            "5",
            "150",
            purpose,
        )
    };
    match action {
        "owner_exit" => {
            support::owner_exit(&journal, AGENT, "01JOWNER0000000000000000AA", Some("149"))?
        }
        "kill_switch" => support::kill(&journal, AGENT, "01JKILL00000000000000000AA")?,
        "protective" => propose(
            &journal,
            AGENT,
            "01JINTENT0000000000000000C",
            "sell",
            "5",
            "140",
            "protective",
        )?,
        purpose => exit(purpose)?,
    }
    for _ in 0..3 {
        process.step(at(NOON)?);
    }
    let requests = broker
        .seen()
        .iter()
        .map(|seen| format!("{:?}", seen.request))
        .collect();
    Ok((journal.types(ACCOUNT_STREAM), requests))
}

/// Rule 13 over a real position: a risk, discretionary, protective or owner exit and a kill switch
/// each journal and ask the broker exactly the same under a shut gate as under an open one, and
/// the agent's risk and discretionary exits do reach the broker under the shut gate.
#[test]
fn every_risk_reduction_goes_alike_under_a_shut_and_an_open_gate() -> Result<(), Error> {
    for action in [
        "risk_exit",
        "discretionary_exit",
        "protective",
        "owner_exit",
        "kill_switch",
    ] {
        let shut = reduced(action, false)?;
        assert_eq!(shut, reduced(action, true)?, "{action}");
        if action.ends_with("_exit") && action != "owner_exit" {
            assert!(
                shut.1
                    .iter()
                    .any(|request| request.contains("md-01JINTENT0000000000000000C")),
                "{action}: {:?}",
                shut.1
            );
        }
    }
    Ok(())
}

/// An increase adds risk as an opening does: held while the gate is shut, not even received, and
/// received and decided as soon as it opens.
#[test]
fn an_increase_is_held_while_shut_and_decided_once_open() -> Result<(), Error> {
    let (journal, broker) = holding()?;
    let gate = OpeningGate::default();
    let mut process = start_wired(
        &journal,
        &broker,
        wired(gate.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    propose(
        &journal,
        AGENT,
        "01JINTENT0000000000000000D",
        "buy",
        "5",
        "150",
        "increase",
    )?;
    for _ in 0..5 {
        process.step(at(NOON)?);
    }
    assert_eq!(journal.received(), ["01JINTENT0000000000000000A"]);
    let decided = |journal: &Journal| {
        journal
            .types(ACCOUNT_STREAM)
            .iter()
            .filter(|t| *t == "GateDecided")
            .count()
    };
    let before = decided(&journal);
    gate.open();
    process.step(at(NOON)?);
    assert_eq!(
        journal.received(),
        ["01JINTENT0000000000000000A", "01JINTENT0000000000000000D"]
    );
    assert_eq!(decided(&journal), before + 1);
    Ok(())
}

/// Item 2: the copy's draft is a record the journal admits. On a journal that judges every record
/// against its registered schemas and rules, an account stream opened by a passed `connect`
/// check commits the copy `copy_draft` writes as the next record.
#[test]
#[ignore = "pending E21-32"]
fn the_journal_admits_the_copy() -> Result<(), Error> {
    const ULID_ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
    let mut store = MemoryStore::default();
    let mut broker = AlpacaBroker::new(FixturePaper::new(at(NOON)?))?;
    let scope = AccountScope {
        account: AccountRef(ULID_ACCOUNT.to_owned()),
        workspace: WorkspaceId(WORKSPACE.to_owned()),
    };
    let checked = connect_check(
        &mut store,
        &mut broker,
        &Ids,
        &scope,
        CONNECTION,
        "pii_01J8Z2PQ000000000000000001",
        at(NOON)?,
    )?;
    assert!(checked.passed);
    let Value::Object(members) = object(vec![
        ("account_ref", text(ULID_ACCOUNT)),
        ("broker", text("alpaca")),
        ("connection_id", text(CONNECTION)),
        ("environment", text("paper")),
        ("margin_attestation", Value::Null),
        ("scopes", Value::Array(vec![text("data"), text("trading")])),
        (
            "step_up",
            object(vec![
                ("assertion_id", text("as_1")),
                ("authenticated_at", text("2026-09-21T14:59:00.000000000Z")),
                ("method", text("passkey")),
            ])?,
        ),
        ("user", text("usr_1")),
    ])?
    else {
        return Err("members".into());
    };
    let due = ToCopy {
        original: EventId("01J8Z2CTL00000000000000001".to_owned()),
        members,
    };
    let stream_id = format!("acct:{WORKSPACE}:{ULID_ACCOUNT}");
    let stream = StreamId::parse(&stream_id).ok_or("stream")?;
    let epoch = store.take_ownership(&stream).ok_or("epoch")?;
    let head = u64::try_from(store.rows(&stream).ok_or("rows")?.len())?;
    let draft = copy_draft(
        &due,
        EventId("1000020000000000000002000000".to_owned()),
        at(NOON)?,
    )?;
    let bytes = draft_bytes(
        &Envelope {
            stream: &stream_id,
            environment: Environment::Paper,
            actor_kind: "system",
            actor_id: "executor",
            build: "mandate-executord/0.0.0",
            version: "0.0.0",
            event_time: at(NOON)?,
        },
        &Fields {
            event_id: &draft.event_id.0,
            event_type: &draft.event_type,
            schema_version: draft.schema_version,
            causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
            correlation_id: None,
            config_refs: &draft.config_refs,
            payload: &draft.payload,
        },
    )
    .ok_or("draft")?;
    let appended = store.append(&stream, head, epoch, at(NOON)?, &[bytes]);
    assert!(
        matches!(appended, AppendOutcome::Committed(_)),
        "{appended:?}"
    );
    Ok(())
}
