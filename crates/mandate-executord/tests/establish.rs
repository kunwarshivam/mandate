//! Journal spec rule 68 at the executor (DEC-2341): the account stream is connecting until it holds
//! its establishment's copy, which the executor writes; while connecting it opens no risk and every
//! risk reduction still runs (`AGENTS.md` rule 13).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Key, Value};
use mandate_executor::{
    AccountRef, AccountScope, AgentId, BindingGateInput, BindingGateRequest, BindingGateSource,
    BrokerRequest, EventId, IdGen, Purpose, Seq, TimeInForce, WorkspaceId, WriterEpoch,
};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::establish::{
    Connected, Establishment, GateReport, OpeningGate, ToCopy, copy_draft, establishment, pass,
};
use mandate_executord::fixture::Ids;
use mandate_executord::paper::FixturePaper;
use mandate_journal::{AppendOutcome, Environment, StreamId};
use mandate_num::{Price, Qty};
use mandate_process::State;
use mandate_store::envelope::{Envelope, Fields, draft_bytes};
use mandate_store::{MemoryStore, Store};
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, Allowing, CONNECTION, CONTROL_STREAM, Error, Journal, NOON,
    OTHER_ACCOUNT, OTHER_CONNECTION, Paper, TwoPercentStop, WORKSPACE, at, deployed, object,
    opened, propose, start_wired, wired,
};

const CHECK: &str = "e1h1o1";
const ESTABLISHED: &str = "c9";
const OWNER: &str = "01JOWNER0000000000000000AA";
const OWNER_EXIT: &str = "01JINTENT0000000000000000D";

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
            support::owner_exit(&journal, AGENT, OWNER, Some("149"))?;
            support::propose_owner_market_exit(&journal, AGENT, OWNER_EXIT, OWNER)?;
        }
        "kill_switch" => support::kill(&journal, AGENT, "01JKILL00000000000000000AA")?,
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

/// Rule 13 over a real position: a risk, discretionary or owner exit and a kill switch each
/// journal and ask the broker exactly the same under a shut gate as under an open one, and each
/// does act under the shut gate: the exits are received and sent, and the kill switch is applied.
#[test]
fn every_risk_reduction_goes_alike_under_a_shut_and_an_open_gate() -> Result<(), Error> {
    for (action, acted) in [
        ("risk_exit", "md-01JINTENT0000000000000000C"),
        ("discretionary_exit", "md-01JINTENT0000000000000000C"),
        ("owner_exit", "md-01JINTENT0000000000000000D"),
        ("kill_switch", "KillSwitchActivated"),
    ] {
        let shut = reduced(action, false)?;
        assert_eq!(shut, reduced(action, true)?, "{action}");
        let (types, requests) = &shut;
        if action == "kill_switch" {
            assert!(types.iter().any(|t| t == acted), "{action}: {types:?}");
        } else {
            assert!(
                types.iter().filter(|t| *t == "IntentReceived").count() == 2
                    && requests
                        .iter()
                        .any(|request| request.starts_with("Submit") && request.contains(acted)),
                "{action}: {types:?} {requests:?}"
            );
        }
    }
    Ok(())
}

/// A binding-gate source that records which purposes it was asked about, and answers none.
#[derive(Default)]
struct Asked(Rc<RefCell<Vec<Purpose>>>);

impl BindingGateSource for Asked {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        self.0.borrow_mut().push(request.purpose);
        None
    }
}

/// Item 3 at the one place the gate acts: while shut it asks the wiring's source about every
/// purpose that adds no risk, a protective order and a kill switch's flatten among them, and about
/// none that adds risk; once open it asks about those too. The executor's own protective orders
/// never ask the binding gate at all: only a received intent does.
#[test]
fn a_shut_gate_asks_its_source_about_everything_but_risk() -> Result<(), Error> {
    let agent = AgentId(AGENT.to_owned());
    let instrument = InstrumentId::new("AAPL")?;
    let ask = |gate: &OpeningGate, purpose| {
        let asked = Asked::default();
        let request = BindingGateRequest {
            agent: &agent,
            instrument: &instrument,
            side: Side::Sell,
            qty: Qty::parse("5")?,
            limit: Price::parse("150")?,
            purpose,
            tif: TimeInForce::Day,
            protection: None,
        };
        let seen = Rc::clone(&asked.0);
        assert!(gate.guard(Box::new(asked)).input(&request).is_none());
        let purposes = seen.borrow().clone();
        Ok::<_, Error>(purposes)
    };
    let gate = OpeningGate::default();
    for purpose in [
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ] {
        assert_eq!(ask(&gate, purpose)?, [purpose], "{purpose:?}");
    }
    for purpose in [Purpose::Open, Purpose::Increase] {
        assert!(ask(&gate, purpose)?.is_empty(), "{purpose:?}");
    }
    gate.open();
    for purpose in [Purpose::Open, Purpose::Increase] {
        assert_eq!(ask(&gate, purpose)?, [purpose], "{purpose:?}");
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
        original: EventId("01J8Z2CCCC0000000000000001".to_owned()),
        members,
    };
    let stream_id = format!("acct:{WORKSPACE}:{ULID_ACCOUNT}");
    let stream = StreamId::parse(&stream_id).ok_or("stream")?;
    let epoch = store.take_ownership(&stream).ok_or("epoch")?;
    let head = u64::try_from(store.rows(&stream).ok_or("rows")?.len())?;
    let draft = copy_draft(
        &due,
        Ids.event_id(WriterEpoch(epoch), Seq(head), 0),
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

/// Items 3 and 6: an executor left connecting, with no establishment and no refusal, opens no
/// risk however many passes it runs, while a risk exit behind the held opening is still received.
#[test]
fn a_connecting_executor_opens_nothing_and_still_exits() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
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
        assert_eq!(
            pass(&mut process, &gate, &scope(), Environment::Paper, at(NOON)?)?,
            Connected::Connecting
        );
        process.step(at(NOON)?);
    }
    assert!(!gate.is_open());
    assert_eq!(journal.received(), ["01JINTENT0000000000000000C"]);
    assert!(
        !journal
            .types(ACCOUNT_STREAM)
            .contains(&"ConnectionEstablished".to_owned())
    );
    Ok(())
}

/// Items 2 and 5: an executor whose control stream already established its check, the API-key
/// executor that served before DEC-2341 among them, writes the copy on its first pass once it has
/// started, opens, and never writes it again; before it has started it copies nothing.
#[test]
fn the_first_pass_copies_a_due_establishment_once_and_opens() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
    establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
    let broker = Paper::new(&journal);
    let gate = OpeningGate::default();
    let mut process = start_wired(
        &journal,
        &broker,
        wired(gate.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    assert_eq!(
        pass(&mut process, &gate, &scope(), Environment::Paper, at(NOON)?)?,
        Connected::Connecting
    );
    assert!(!gate.is_open());
    process.step(at(NOON)?);
    for _ in 0..3 {
        assert_eq!(
            pass(&mut process, &gate, &scope(), Environment::Paper, at(NOON)?)?,
            Connected::Open
        );
    }
    assert!(gate.is_open());
    let copies: Vec<_> = journal
        .rows_of(ACCOUNT_STREAM)
        .into_iter()
        .filter(|row| row.event_type == "ConnectionEstablished")
        .collect();
    assert_eq!(copies.len(), 1);
    let copied = journal.payloads("ConnectionEstablished");
    assert!(
        copied.iter().any(|p| p.get("risk_clock").is_some()),
        "{copied:?}"
    );
    Ok(())
}

/// Item 1: an executor restarted on an account stream that already holds its copy is established
/// on its first pass, before it has stepped, and opens; it writes no second copy.
#[test]
fn a_restarted_established_executor_opens_on_its_first_pass() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
    establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
    let broker = Paper::new(&journal);
    let first = OpeningGate::default();
    let mut process = start_wired(
        &journal,
        &broker,
        wired(first.guard(Box::new(Allowing)), Box::new(TwoPercentStop))?,
    )?;
    process.step(at(NOON)?);
    pass(
        &mut process,
        &first,
        &scope(),
        Environment::Paper,
        at(NOON)?,
    )?;
    drop(process);
    let restarted = OpeningGate::default();
    let mut process = start_wired(
        &journal,
        &broker,
        wired(
            restarted.guard(Box::new(Allowing)),
            Box::new(TwoPercentStop),
        )?,
    )?;
    assert_eq!(
        pass(
            &mut process,
            &restarted,
            &scope(),
            Environment::Paper,
            at(NOON)?
        )?,
        Connected::Open
    );
    assert!(restarted.is_open());
    let copies = journal
        .rows_of(ACCOUNT_STREAM)
        .into_iter()
        .filter(|row| row.event_type == "ConnectionEstablished")
        .count();
    assert_eq!(copies, 1);
    Ok(())
}

/// Item 4 and rule 13: a connection refused or revoked before its copy is closed, and the gate
/// stays shut; the pass stops nothing, so the process goes on running, and after a refusal it
/// still takes every exit and no opening, until the manager stops it. (A revoked connection's
/// agents are no longer followed at all, by `follow::agents_of`, so none of theirs is received.)
#[test]
fn a_refused_connection_is_closed_and_stays_shut() -> Result<(), Error> {
    for event_type in ["ConnectionRefused", "ConnectionRevoked"] {
        let journal = deployed()?;
        opened(&journal)?;
        check(&journal, CHECK, CONNECTION, "connect", "passed")?;
        closing(&journal, "r1", event_type, CONNECTION, "connect")?;
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
        for _ in 0..5 {
            assert_eq!(
                pass(&mut process, &gate, &scope(), Environment::Paper, at(NOON)?)?,
                Connected::Closed,
                "{event_type}"
            );
            process.step(at(NOON)?);
        }
        assert!(!gate.is_open(), "{event_type}");
        assert!(
            !matches!(process.lifecycle().state(), State::Exited(_)),
            "{event_type}"
        );
        let exits: &[&str] = if event_type == "ConnectionRefused" {
            &["01JINTENT0000000000000000C"]
        } else {
            &[]
        };
        assert_eq!(journal.received(), exits, "{event_type}");
    }
    Ok(())
}

/// Rules 3 and 13: a connection record that does not read, here a refusal naming no connection, is
/// ambiguity, so the pass answers its code, copies nothing, opens nothing and stops nothing; the
/// process goes on taking every exit.
#[test]
fn an_unreadable_record_keeps_the_gate_shut_and_every_exit_going() -> Result<(), Error> {
    let journal = deployed()?;
    opened(&journal)?;
    check(&journal, CHECK, CONNECTION, "connect", "passed")?;
    establish(&journal, ESTABLISHED, 2, Some(CHECK), CONNECTION, ACCOUNT)?;
    journal.write(
        CONTROL_STREAM,
        "r1",
        "ConnectionRefused",
        1,
        None,
        object(vec![("occasion", text("connect"))])?,
    )?;
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
    for _ in 0..5 {
        process.step(at(NOON)?);
        assert_eq!(
            pass(&mut process, &gate, &scope(), Environment::Paper, at(NOON)?),
            Err("connection_record_unreadable")
        );
    }
    assert!(!gate.is_open());
    assert!(
        !journal
            .types(ACCOUNT_STREAM)
            .contains(&"ConnectionEstablished".to_owned())
    );
    assert_eq!(journal.received(), ["01JINTENT0000000000000000C"]);
    Ok(())
}

/// Item 4's log: a closed or unreadable connection's code is printed when it starts and again
/// only after it changed, never once per pass, and nothing is printed while open or connecting.
#[test]
fn the_gate_reports_each_code_once_per_change() {
    let mut report = GateReport::default();
    let printed: Vec<_> = [
        Ok(Connected::Connecting),
        Ok(Connected::Closed),
        Ok(Connected::Closed),
        Err("connection_record_unreadable"),
        Err("connection_record_unreadable"),
        Err("stream_unavailable"),
        Ok(Connected::Connecting),
        Ok(Connected::Closed),
        Ok(Connected::Open),
        Ok(Connected::Open),
    ]
    .into_iter()
    .map(|outcome| report.changed(outcome))
    .collect();
    assert_eq!(
        printed,
        [
            None,
            Some("connection_closed"),
            None,
            Some("connection_record_unreadable"),
            None,
            Some("stream_unavailable"),
            None,
            Some("connection_closed"),
            None,
            None,
        ]
    );
}
