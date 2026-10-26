//! K1b's second half (E7-11; DEC-529 item 3; mandate spec V-001; DEC-925): `version create`,
//! `version confirm` and `agent deploy`, and the two gestures that show a code, admit a `live`
//! owner only for a `live` mandate on a Robinhood `live` connection the owner recorded through the
//! CLI and has not spent. Every other live combination keeps its refusal. The control stream and
//! the account stream are seeded here, record by record, in journal spec §9.2's and §9.8's shapes,
//! so these tests read no other command's output; the property's oracle walks the seeded events
//! itself.

mod common;

use std::collections::BTreeMap;

use clap::Parser;
use common::{ACCOUNT, AGENT, CONTROL, FixedIds, Journal, OWNER, at, owner, stream};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_cli::deploy::deploy;
use mandate_cli::gestures::{AgentCommand, VersionCommand, confirmation, deployment};
use mandate_cli::version::{admitted, confirm, create};
use mandate_cli::{Cli, Command, brake};
use mandate_journal::{AppendOutcome, ArtifactStore, Environment};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

type Store = BTreeMap<Digest, Vec<u8>>;
/// One of the three gestures, by an owner, on the paper or the live document.
type Gesture = fn(&mut World, &Owner, bool) -> Result<Submitted, ControlError>;

const NOW: i64 = 1_790_000_000;
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const SNAPSHOT: &str = r#"{"admits_instruments":null,"content_hash":"@H","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}"#;
const SEEDED: &str = r#"{"actor":{"build":null,"id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"@E","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"@S"}"#;
/// The live mandate's connection, and another the control stream may record.
const CONN: &str = "conn_robinhood_live_01";
const OTHER_CONN: &str = "conn_robinhood_live_02";
const OTHER_ACCOUNT: &str = "01J0ACC0VNT000000000000001";
/// `connection record`'s version 2 payload (journal spec §9.8), the owner's own Robinhood `live`
/// connection; `@C`, `@B`, `@N`, `@A`, `@M`, `@U` and `@R` are what a case varies.
const ESTABLISHED: &str = r#"{"account_ref":"@R","broker":"@B","connection_id":"@C","environment":"@N","margin_attestation":@A,"scopes":["get_accounts","place_equity_order","review_equity_order"],"step_up":{"assertion_id":"cli-assertion-9","authenticated_at":1790000000,"method":"@M"},"user":"@U"}"#;
/// The codes a gesture gives for an environment it does not admit (DEC-530 item 2, DEC-925).
const CODES: [&str; 3] = ["connection_spent", "mandate_not_paper", "paper_only"];

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap()
}

fn canonical(text: &str) -> Vec<u8> {
    to_canonical(&json(text))
}

fn reference(text: &str) -> String {
    format!("sha256:{}", Digest::of(&canonical(text)).to_hex())
}

/// The paper fixture, or the same mandate made `live` on [`CONN`], with the stop-limit offset
/// V-008 asks for on Robinhood's profile (DEC-539 item 2, DEC-1152 item 1).
fn document(live: bool) -> String {
    if !live {
        return MANDATE.to_owned();
    }
    without_offset().replace(NO_OFFSET, r#""crypto_stop_limit_offset":"0.01""#)
}

/// The fixture's protection with no stop-limit offset.
const NO_OFFSET: &str = r#""crypto_stop_limit_offset":null"#;

/// The live mandate as [`document`] writes it, but with no stop-limit offset.
fn without_offset() -> String {
    let from = [
        r#""environment":"paper""#,
        "conn_alpaca_paper_01",
        NO_OFFSET,
    ];
    assert!(from.iter().all(|f| MANDATE.contains(f)));
    MANDATE
        .replace(from[0], r#""environment":"live""#)
        .replace(from[1], CONN)
}

fn confirm_code(version: &str) -> String {
    let gesture = format!(r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#);
    Digest::of(&canonical(&gesture)).to_hex()[..8].to_owned()
}

fn deploy_code(version: &str) -> String {
    let gesture = format!(
        r#"{{"agent_id":"{AGENT}","gesture":"agent_deploy","mandate_version":"{version}"}}"#
    );
    Digest::of(&canonical(&gesture)).to_hex()[..8].to_owned()
}

/// One connection record as a case varies it from the owner's good one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Record {
    connection: &'static str,
    broker: &'static str,
    environment: &'static str,
    attested: bool,
    method: &'static str,
    user: &'static str,
    account: &'static str,
}

const GOOD: Record = Record {
    connection: CONN,
    broker: "robinhood",
    environment: "live",
    attested: true,
    method: "cli_confirm",
    user: OWNER,
    account: ACCOUNT,
};

impl Record {
    fn payload(self) -> String {
        let attestation = if self.attested {
            r#""cash_account""#
        } else {
            "null"
        };
        ESTABLISHED
            .replace("@C", self.connection)
            .replace("@B", self.broker)
            .replace("@N", self.environment)
            .replace("@A", attestation)
            .replace("@M", self.method)
            .replace("@U", self.user)
            .replace("@R", self.account)
    }
}

/// One event the world's streams take, as a writer other than the gestures would append it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Establish(Record),
    Revoke(&'static str),
    /// An event of this type on the account stream of this `account_ref`.
    Account(&'static str, &'static str),
    /// An `OrderStateChanged` for this client order id, to this state, on this account stream.
    Order(&'static str, &'static str, &'static str),
}

/// A workspace whose control stream registers the fixture's model and SPY's snapshot.
#[derive(Default)]
struct World {
    journal: Journal,
    store: Store,
    ids: FixedIds,
    seeded: u64,
}

impl World {
    /// A live workspace (DEC-952): its control stream's registrations are `live`, so the live
    /// owner's gestures append to a stream of their own environment.
    fn new() -> Self {
        Self::opened("live")
    }

    /// A workspace whose control stream's registrations are in `environment`.
    fn opened(environment: &str) -> Self {
        let mut world = Self::default();
        world.seed(CONTROL, environment, "ConfigSnapshotRegistered", MODEL);
        world.store.put_artifact(SPY.as_bytes()).unwrap();
        let snapshot = SNAPSHOT.replace("@H", &reference(SPY));
        world.seed(CONTROL, environment, "ConfigSnapshotRegistered", &snapshot);
        world
    }

    fn with(events: &[Event]) -> Self {
        Self::opened_with("live", events)
    }

    fn opened_with(environment: &str, events: &[Event]) -> Self {
        let mut world = Self::opened(environment);
        for event in events {
            world.apply(*event);
        }
        world
    }

    fn seed(&mut self, on: &str, environment: &str, event_type: &str, payload: &str) {
        self.seeded += 1;
        let target = stream(on);
        let head = self.journal.head(&target).unwrap().seq;
        let draft = SEEDED
            .replace("@E", environment)
            .replace("@I", &format!("9{:025}", self.seeded))
            .replace("@T", event_type)
            .replace("@S", on)
            .replace("@P", payload);
        let epoch = self.journal.take_ownership(&target).unwrap();
        let outcome = self
            .journal
            .append(&target, head, epoch, at(NOW).at, &[draft.as_bytes()]);
        assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Establish(record) => {
                let environment = record.environment;
                let payload = record.payload();
                self.seed(CONTROL, environment, "ConnectionEstablished", &payload);
            }
            Event::Revoke(connection) => {
                let payload = format!(r#"{{"connection_id":"{connection}"}}"#);
                self.seed(CONTROL, "live", "ConnectionRevoked", &payload);
            }
            Event::Account(event_type, account) => {
                let on = format!("acct:ws1:{account}");
                self.seed(&on, "live", event_type, r#"{"agent":"agent-a"}"#);
            }
            Event::Order(order, state, account) => {
                let on = format!("acct:ws1:{account}");
                let payload = format!(r#"{{"client_order_id":"{order}","state":"{state}"}}"#);
                self.seed(&on, "live", "OrderStateChanged", &payload);
            }
        }
    }

    fn create(&mut self, owner: &Owner, live: bool) -> Result<Submitted, ControlError> {
        let bytes = document(live);
        create(
            &mut self.journal,
            &mut self.store,
            owner,
            bytes.as_bytes(),
            at(NOW),
        )
    }

    /// The show-code gesture's answer, then the confirmation's with the code it showed.
    fn confirm(&mut self, owner: &Owner, live: bool) -> Result<Submitted, ControlError> {
        let version = reference(&document(live));
        let shown = confirmation(&self.journal, &self.store, owner, &version, at(NOW));
        let (journal, store, ids) = (&mut self.journal, &mut self.store, &mut self.ids);
        let code = confirm_code(&version);
        let done = confirm(journal, store, ids, owner, &version, &code, at(NOW));
        same(shown.map(|s| s.code), &done, &code)?;
        done
    }

    fn deploy(&mut self, owner: &Owner, live: bool) -> Result<Submitted, ControlError> {
        let version = reference(&document(live));
        let named = (AGENT, version.as_str());
        let shown = deployment(&self.journal, &self.store, owner, named, at(NOW));
        let (journal, store, ids) = (&mut self.journal, &mut self.store, &mut self.ids);
        let code = deploy_code(&version);
        let done = deploy(journal, store, ids, owner, AGENT, &version, &code, at(NOW));
        same(shown.map(|s| s.code), &done, &code)?;
        done
    }

    /// What a gesture could have written: the control stream's appends, the store, the assertions.
    fn state(&self) -> (usize, Store, u64) {
        let appends = self.journal.attempts.len();
        (appends, self.store.clone(), self.ids.assertions)
    }

    fn last_envelope(&self) -> Value {
        let rows = self.journal.rows(&stream(CONTROL)).unwrap();
        parse(&rows.last().unwrap().body).unwrap()
    }
}

/// The show-code gesture refuses exactly as its commit does, or shows the code the commit took.
fn same(
    shown: Result<String, ControlError>,
    done: &Result<Submitted, ControlError>,
    code: &str,
) -> Result<(), ControlError> {
    match (shown, done) {
        (Ok(shown), Ok(_)) => assert_eq!(shown, code, "the code shown is the one committed"),
        (Err(shown), Err(done)) => assert_eq!(&shown, done, "show and commit refuse alike"),
        (shown, done) => panic!("show {shown:?} and commit {done:?} disagree"),
    }
    Ok(())
}

fn in_env(environment: Environment) -> Owner {
    Owner {
        environment,
        ..owner()
    }
}

fn reason(result: Result<Submitted, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

/// The one admitted path: a `live` owner creates, confirms and deploys a `live` mandate on the
/// Robinhood connection they recorded through the CLI. Each event is committed in `live`, by the
/// owner, and the confirmation and deployment records carry `cli_confirm` step-up (DEC-529 item 3).
#[test]
fn a_live_mandate_on_the_owners_recorded_robinhood_connection_is_created_confirmed_and_deployed() {
    let mut world = World::with(&[
        Event::Establish(GOOD),
        Event::Account("OrderSubmitted", ACCOUNT),
        Event::Account("FillApplied", OTHER_ACCOUNT),
        Event::Order("mdt-1", "unknown", ACCOUNT),
        Event::Order("mdt-1", "canceled", ACCOUNT),
        Event::Order("mdt-2", "unknown", OTHER_ACCOUNT),
    ]);
    let live = in_env(Environment::Live);
    let steps: [Gesture; 3] = [World::create, World::confirm, World::deploy];
    let types = ["MandateVersionCreated", "MandateConfirmed", "AgentDeployed"];
    for (step, event_type) in steps.into_iter().zip(types) {
        let before = world.journal.attempts.len();
        let done = step(&mut world, &live, true);
        assert!(done.is_ok(), "{event_type}: {done:?}");
        assert_eq!(world.journal.attempts.len(), before + 1, "{event_type}");
        let envelope = world.last_envelope();
        let field = |name: &str| {
            envelope
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        assert_eq!(field("event_type").as_deref(), Some(event_type));
        assert_eq!(
            field("environment").as_deref(),
            Some("live"),
            "{event_type}"
        );
        let actor = envelope.get("actor").and_then(|a| a.get("id"));
        assert_eq!(actor.and_then(Value::as_str), Some(OWNER));
    }
    assert_eq!(
        world.ids.assertions, 2,
        "the confirmation's and the deployment's step-up"
    );
    let again = world.deploy(&live, true);
    assert!(again.is_ok(), "a re-run finds the deployment: {again:?}");
}

/// R0's R22 (DEC-1152 item 1): on the owner's recorded Robinhood connection, whose profile protects
/// equities with a stop-limit only, a live equity mandate with no stop-limit offset is created
/// but refused `version_invalid` (V-008) at its confirmation, before the code is shown and with
/// nothing appended; the same mandate with the offset is confirmed. The paper fixture, on Alpaca's
/// profile, needs none.
#[test]
fn a_live_robinhood_mandate_without_a_stop_limit_offset_is_refused_at_confirmation() {
    let mut world = World::with(&[Event::Establish(GOOD)]);
    let live = in_env(Environment::Live);
    let bytes = without_offset();
    let created = create(
        &mut world.journal,
        &mut world.store,
        &live,
        bytes.as_bytes(),
        at(NOW),
    );
    assert!(created.is_ok(), "{created:?}");
    let before = world.journal.attempts.len();
    let version = reference(&bytes);
    let shown = confirmation(&world.journal, &world.store, &live, &version, at(NOW));
    assert_eq!(
        shown.map(drop),
        Err(ControlError::Refused {
            reason: "version_invalid"
        })
    );
    assert_eq!(world.journal.attempts.len(), before, "nothing appended");
    assert!(world.create(&live, true).is_ok());
    assert!(
        world.confirm(&live, true).is_ok(),
        "with the offset it is confirmed"
    );
}

/// A paper owner's gestures on the paper fixture, in a paper workspace, are unchanged by a live
/// connection's record.
#[test]
fn a_paper_owner_still_creates_confirms_and_deploys_a_paper_mandate() {
    let mut world = World::opened_with("paper", &[Event::Establish(GOOD)]);
    let paper = owner();
    world.create(&paper, false).unwrap();
    world.confirm(&paper, false).unwrap();
    world.deploy(&paper, false).unwrap();
    assert_eq!(
        world.last_envelope().get("environment"),
        Some(&json(r#""paper""#))
    );
}

/// The stage a refusal is met at: the gesture refused, after the setup gestures that pass.
#[derive(Debug, Clone, Copy)]
enum Stage {
    Create,
    Confirm,
    Deploy,
}

/// Every live combination but the admitted one keeps refusing, with `paper_only` as before (DEC-530
/// item 2), `mandate_not_paper` for a paper owner and a live document, or `connection_spent` once
/// the record is spent (a fill, DEC-910 item 3, or an order left `unknown`, V-001); the show-code
/// gestures refuse alike, and nothing is written. A record revoked or spent after an earlier
/// gesture passed refuses the next one.
#[test]
fn every_other_live_combination_is_refused_with_its_code_and_writes_nothing() {
    let paper_record = Record {
        environment: "paper",
        ..GOOD
    };
    let alpaca = Record {
        broker: "alpaca",
        ..GOOD
    };
    let kraken = Record {
        broker: "kraken_derivatives_us",
        ..GOOD
    };
    let unattested = Record {
        attested: false,
        ..GOOD
    };
    let passkey = Record {
        method: "passkey",
        ..GOOD
    };
    let other_user = Record {
        user: "user-other",
        ..GOOD
    };
    let other_conn = Record {
        connection: OTHER_CONN,
        account: OTHER_ACCOUNT,
        ..GOOD
    };
    let good = Event::Establish(GOOD);
    let revoke = Event::Revoke(CONN);
    let fill = Event::Account("FillApplied", ACCOUNT);
    let late = Event::Account("LateFillApplied", ACCOUNT);
    let lost = Event::Order("mdt-1", "unknown", ACCOUNT);
    let (paper, live) = (Environment::Paper, Environment::Live);
    let backtest = Environment::Backtest;
    let cases: [(&str, &[Event], Stage, Environment, bool); 23] = [
        ("paper_only", &[], Stage::Create, live, true),
        ("connection_spent", &[good, lost], Stage::Create, live, true),
        ("connection_spent", &[good, lost], Stage::Deploy, live, true),
        ("paper_only", &[good], Stage::Create, live, false),
        ("paper_only", &[good], Stage::Create, backtest, true),
        (
            "paper_only",
            &[Event::Establish(paper_record)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(alpaca)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(kraken)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(unattested)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(passkey)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(other_user)],
            Stage::Create,
            live,
            true,
        ),
        (
            "paper_only",
            &[Event::Establish(other_conn)],
            Stage::Create,
            live,
            true,
        ),
        ("paper_only", &[good, revoke], Stage::Create, live, true),
        ("paper_only", &[good, revoke], Stage::Confirm, live, true),
        ("paper_only", &[good, revoke], Stage::Deploy, live, true),
        ("paper_only", &[good], Stage::Confirm, backtest, true),
        ("mandate_not_paper", &[good], Stage::Create, paper, true),
        ("mandate_not_paper", &[good], Stage::Confirm, paper, true),
        ("mandate_not_paper", &[good], Stage::Deploy, paper, true),
        ("connection_spent", &[good, fill], Stage::Create, live, true),
        (
            "connection_spent",
            &[good, late],
            Stage::Confirm,
            live,
            true,
        ),
        ("connection_spent", &[good, fill], Stage::Deploy, live, true),
        ("connection_spent", &[good, late], Stage::Deploy, live, true),
    ];
    let mut seen = Vec::new();
    for (code, events, stage, environment, live_document) in cases {
        let (passing, rest) = events.split_at(1.min(events.len()));
        let mut world = World::with(passing);
        let admitted_owner = in_env(Environment::Live);
        let earlier = match stage {
            Stage::Create => 0,
            Stage::Confirm => 1,
            Stage::Deploy => 2,
        };
        if earlier >= 1 {
            world.create(&admitted_owner, true).unwrap();
        }
        if earlier >= 2 {
            world.confirm(&admitted_owner, true).unwrap();
        }
        for event in rest {
            world.apply(*event);
        }
        let before = world.state();
        let owner = in_env(environment);
        let result = match stage {
            Stage::Create => world.create(&owner, live_document),
            Stage::Confirm => world.confirm(&owner, live_document),
            Stage::Deploy => world.deploy(&owner, live_document),
        };
        assert_eq!(reason(result), code, "{events:?} {stage:?} {environment:?}");
        assert!(world.state() == before, "{code} writes nothing");
        seen.push(code);
    }
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, CODES, "each code met");
}

/// An owner neither in `paper` nor in `live` is refused `paper_only` before anything is read
/// (DEC-530 item 2): for a version the stream never created, not `version_unknown`; for a document
/// that does not parse, not `mandate_invalid`.
#[test]
fn a_backtest_owner_is_refused_before_anything_is_read() {
    let mut world = World::new();
    let owner = in_env(Environment::Backtest);
    let unknown = reference(&document(true));
    let shown = confirmation(&world.journal, &world.store, &owner, &unknown, at(NOW));
    assert_eq!(shown.map(|s| s.code), Err(refused("paper_only")));
    let named = (AGENT, unknown.as_str());
    let shown = deployment(&world.journal, &world.store, &owner, named, at(NOW));
    assert_eq!(shown.map(|s| s.code), Err(refused("paper_only")));
    let (journal, store, ids) = (&mut world.journal, &mut world.store, &mut world.ids);
    let code = confirm_code(&unknown);
    let done = confirm(journal, store, ids, &owner, &unknown, &code, at(NOW));
    assert_eq!(reason(done), "paper_only");
    let code = deploy_code(&unknown);
    let done = deploy(journal, store, ids, &owner, AGENT, &unknown, &code, at(NOW));
    assert_eq!(reason(done), "paper_only");
    let done = create(journal, store, &owner, b"not json", at(NOW));
    assert_eq!(reason(done), "paper_only");
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// The gestures' `--environment`: `paper` by default, `live` when given; any other value is
/// refused `environment_invalid` before any journal is opened.
#[test]
fn the_gestures_take_an_environment_paper_by_default() {
    let flags = "--workspace ws1 --user user-owner --journal postgres://h/j --store s";
    let lines = [
        "version create mandate.json",
        "version confirm sha256:aa",
        "agent deploy agent-a sha256:aa",
    ];
    for line in lines {
        for (extra, expected) in [
            ("", Ok(Environment::Paper)),
            ("--environment paper", Ok(Environment::Paper)),
            ("--environment live", Ok(Environment::Live)),
            ("--environment backtest", Err("environment_invalid")),
            ("--environment Live", Err("environment_invalid")),
        ] {
            let full = format!("mandate {line} {flags} {extra}");
            let words: Vec<&str> = full.split_whitespace().collect();
            let args = match Cli::try_parse_from(&words).unwrap().command {
                Command::Version(VersionCommand::Create(args)) => args.owner,
                Command::Version(VersionCommand::Confirm(args)) => args.owner,
                Command::Agent(brake::AgentCommand::Gesture(AgentCommand::Deploy(args))) => {
                    args.owner
                }
                other => panic!("{line}: {other:?}"),
            };
            let got = args.owner().map(|o| o.environment).map_err(|e| match e {
                ControlError::Refused { reason } => reason,
                other => panic!("{other:?}"),
            });
            assert_eq!(got, expected, "{line} {extra}");
        }
    }
}

/// The oracle's reading of the seeded events, independent of the CLI's: the owner's environment,
/// the document's, and every event, in order.
fn expected(owner: Environment, live_document: bool, events: &[Event]) -> Result<(), &'static str> {
    match (owner, live_document) {
        (Environment::Paper, false) => return Ok(()),
        (Environment::Paper, true) => return Err("mandate_not_paper"),
        (Environment::Live, true) => {}
        _ => return Err("paper_only"),
    }
    let mut bound: Option<Record> = None;
    for event in events {
        match event {
            Event::Establish(record) if record.connection == CONN => bound = Some(*record),
            Event::Revoke(connection) if *connection == CONN => bound = None,
            _ => {}
        }
    }
    let Some(record) = bound.filter(|r| {
        *r == Record {
            account: r.account,
            ..GOOD
        }
    }) else {
        return Err("paper_only");
    };
    let fills = ["FillApplied", "LateFillApplied"];
    let spends =
        |e: &&Event| matches!(e, Event::Account(t, a) if fills.contains(t) && *a == record.account);
    let mut latest: Vec<(&str, &str)> = Vec::new();
    for event in events {
        if let Event::Order(order, state, account) = event
            && *account == record.account
        {
            latest.retain(|(o, _)| o != order);
            latest.push((order, state));
        }
    }
    let lost = latest.iter().any(|(_, state)| *state == "unknown");
    if events.iter().any(|e| spends(&e)) || lost {
        return Err("connection_spent");
    }
    Ok(())
}

fn record() -> impl Strategy<Value = Record> {
    (
        prop::sample::select(vec![CONN, OTHER_CONN]),
        prop::sample::select(vec!["robinhood", "alpaca", "kraken_derivatives_us", "ibkr"]),
        prop::sample::select(vec!["live", "paper"]),
        any::<bool>(),
        prop::sample::select(vec!["cli_confirm", "passkey"]),
        prop::sample::select(vec![OWNER, "user-other"]),
        prop::sample::select(vec![ACCOUNT, OTHER_ACCOUNT]),
    )
        .prop_map(
            |(connection, broker, environment, attested, method, user, account)| Record {
                connection,
                broker,
                environment,
                attested,
                method,
                user,
                account,
            },
        )
}

fn event() -> impl Strategy<Value = Event> {
    prop_oneof![
        3 => record().prop_map(Event::Establish),
        1 => prop::sample::select(vec![CONN, OTHER_CONN]).prop_map(Event::Revoke),
        1 => (
            prop::sample::select(vec!["FillApplied", "LateFillApplied", "OrderSubmitted"]),
            prop::sample::select(vec![ACCOUNT, OTHER_ACCOUNT]),
        )
            .prop_map(|(t, a)| Event::Account(t, a)),
        1 => (
            prop::sample::select(vec!["mdt-1", "mdt-2"]),
            prop::sample::select(vec!["unknown", "accepted", "canceled", "filled"]),
            prop::sample::select(vec![ACCOUNT, OTHER_ACCOUNT]),
        )
            .prop_map(|(o, s, a)| Event::Order(o, s, a)),
    ]
}

/// No `live` mandate is ever admitted but on a Robinhood `live` connection the owner recorded
/// through the CLI, not revoked since and unspent; every other draw refuses as the oracle says.
/// Each owner acts in a workspace of its own environment (DEC-952).
#[test]
fn no_live_mandate_is_admitted_but_on_the_owners_unspent_robinhood_record() {
    let drawn = (
        prop::collection::vec(event(), 0..8),
        prop::sample::select(vec![
            Environment::Paper,
            Environment::Live,
            Environment::Backtest,
        ]),
        any::<bool>(),
    );
    TestRunner::deterministic()
        .run(&drawn, |(events, environment, live_document)| {
            let workspace = match environment {
                Environment::Paper => "paper",
                Environment::Live | Environment::Backtest => "live",
            };
            let world = World::opened_with(workspace, &events);
            let owner = in_env(environment);
            let document = json(&document(live_document));
            let got = admitted(&world.journal, &owner, &document).map_err(|e| match e {
                ControlError::Refused { reason } => reason,
                other => panic!("{other:?}"),
            });
            prop_assert_eq!(got, expected(environment, live_document, &events));
            if got.is_ok() && environment == Environment::Live {
                let last = events.iter().rev().find_map(|e| match e {
                    Event::Establish(r) if r.connection == CONN => Some(Some(r)),
                    Event::Revoke(c) if *c == CONN => Some(None),
                    _ => None,
                });
                let robinhood = last.flatten().map(|r| (r.broker, r.environment));
                prop_assert_eq!(robinhood, Some(("robinhood", "live")));
            }
            Ok(())
        })
        .unwrap();
}
