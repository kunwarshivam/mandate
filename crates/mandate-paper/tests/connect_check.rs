//! The runner's connect check (R0 blocker R1, DEC-946; connections spec §8.1 checks 1, 2, 3 and
//! 7; journal spec §9.8): what it refuses before anything is read, what it journals on
//! `acct:{workspace}:{account_ref}` for every outcome of the reads, and the account read itself
//! against the simulated Robinhood server. Oracles: the journal's own rows parsed back field by
//! field, the expected results written out by hand from §8.1, and the simulator's accounts. No
//! test reaches a broker; the account number never reaches a row, a line or an error.

#[path = "../../mandate-rh-sim/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::process::Command;
use std::rc::Rc;

use common::wire::Wire;
use common::{AGENTIC, NOT_AGENTIC, account, sim};
use mandate_canon::{Value, parse};
use mandate_domain::Environment;
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId, TrustedStart, verify_events};
use mandate_mcp::{ALLOWLIST, CallClass, Listed, McpError};
use mandate_paper::{
    ACCOUNT_NUMBER_VAR, CheckArgs, CheckPorts, Observed, PaperError, SeenContract, connect_check,
    observe, parse_check,
};
use mandate_rh_sim::{Sim, SimServer, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools};
use mandate_time::UtcNanos;
use tokio::runtime::Builder;

const WORKSPACE: &str = "ws_r1";
const ACCOUNT_REF: &str = "01J0R1CHECK0ACC0VNT000000A";
const CONNECTION: &str = "conn_rh_r1";
const STREAM: &str = "acct:ws_r1:01J0R1CHECK0ACC0VNT000000A";
const NUMBER: &str = "5QRSENTINEL77";
const NOW: &str = "2026-10-16T14:00:00.123456789Z";
const RISK_CLOCK: &str = "2026-10-16T14:00:00.000000000Z";
const HASH: &str = "ab";

fn args() -> CheckArgs {
    CheckArgs {
        workspace: WORKSPACE.to_owned(),
        account_ref: ACCOUNT_REF.to_owned(),
        connection: CONNECTION.to_owned(),
        journal: "postgres://unused".to_owned(),
        acknowledged: Environment::Live,
    }
}

fn vars() -> Vec<(String, String)> {
    vec![(ACCOUNT_NUMBER_VAR.to_owned(), NUMBER.to_owned())]
}

fn stream() -> StreamId {
    StreamId::parse(STREAM).unwrap()
}

fn allowlist() -> BTreeSet<String> {
    ALLOWLIST.iter().map(|name| (*name).to_owned()).collect()
}

fn contract(names: BTreeSet<String>, allowlisted_present: bool) -> Option<SeenContract> {
    Some(SeenContract {
        listed: Listed {
            names,
            allowlisted_present,
        },
        hash: HASH.repeat(32),
    })
}

fn passing() -> Observed {
    Observed {
        contract: contract(allowlist(), true),
        agentic_account_read: true,
    }
}

/// The ports over a memory journal, recording which effect ran, in order.
struct Fake {
    observed: Result<Observed, ()>,
    harden_fails: bool,
    outcome: Option<AppendOutcome>,
    journal: MemoryJournal,
    calls: Vec<String>,
}

impl Fake {
    fn new(observed: Observed) -> Self {
        Self {
            observed: Ok(observed),
            harden_fails: false,
            outcome: None,
            journal: MemoryJournal::new(),
            calls: Vec::new(),
        }
    }

    fn rows(&self) -> Vec<Value> {
        self.journal
            .rows(&stream())
            .iter()
            .map(|row| parse(&row.body).unwrap())
            .collect()
    }
}

impl CheckPorts for Fake {
    fn harden(&mut self) -> Result<(), PaperError> {
        self.calls.push("harden".to_owned());
        if self.harden_fails {
            Err(PaperError::Harden)
        } else {
            Ok(())
        }
    }

    fn observe(&mut self, account_number: &str) -> Result<Observed, PaperError> {
        self.calls.push(format!("observe {account_number}"));
        self.observed.clone().map_err(|()| PaperError::Credentials)
    }

    fn now(&self) -> UtcNanos {
        UtcNanos::parse(NOW).unwrap()
    }

    fn own(&mut self, stream: &StreamId) -> Result<(u64, u64), PaperError> {
        self.calls.push(format!("own {}", stream.as_str()));
        let epoch = self.journal.take_ownership(stream);
        Ok((epoch, self.journal.head(stream).seq))
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Result<AppendOutcome, PaperError> {
        self.calls.push(format!("append {}", drafts.len()));
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        let outcome = self.journal.append(stream, head, epoch, at, &drafts);
        Ok(self.outcome.clone().unwrap_or(outcome))
    }
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut at = value;
    for name in path {
        at = at.get(name)?;
    }
    at.as_str()
}

/// `results` as `check result reason` triples, in the record's order.
fn results(row: &Value) -> Vec<(String, String, Option<String>)> {
    row.get("payload")
        .and_then(|p| p.get("results"))
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|r| {
            (
                text(r, &["check"]).unwrap().to_owned(),
                text(r, &["result"]).unwrap().to_owned(),
                text(r, &["reason"]).map(str::to_owned),
            )
        })
        .collect()
}

fn passed(check: &str) -> (String, String, Option<String>) {
    (check.to_owned(), "passed".to_owned(), None)
}

fn failed(check: &str, reason: &str) -> (String, String, Option<String>) {
    (
        check.to_owned(),
        "failed".to_owned(),
        Some(reason.to_owned()),
    )
}

fn no_number_in(fake: &Fake) {
    for row in fake.journal.rows(&stream()) {
        let body = String::from_utf8(row.body.clone()).unwrap();
        assert!(!body.contains(NUMBER), "{body}");
    }
}

#[test]
fn a_passing_check_opens_the_account_stream_and_journals_connection_checked() {
    let mut fake = Fake::new(passing());
    let checked = connect_check(&args(), &vars(), &mut fake).unwrap();
    assert_eq!(
        fake.calls,
        [
            "harden".to_owned(),
            format!("observe {NUMBER}"),
            format!("own {STREAM}"),
            "append 2".to_owned(),
        ]
    );
    let rows = fake.rows();
    assert_eq!(rows.len(), 2);
    let (opened, check) = (&rows[0], &rows[1]);
    assert_eq!(text(opened, &["event_type"]), Some("StreamOpened"));
    assert_eq!(text(opened, &["payload", "stream_type"]), Some("account"));
    assert_eq!(text(opened, &["payload", "workspace_id"]), Some(WORKSPACE));
    assert_eq!(text(opened, &["payload", "broker"]), Some("robinhood"));
    assert_eq!(text(opened, &["payload", "account_ref"]), Some(ACCOUNT_REF));
    assert_eq!(text(check, &["event_type"]), Some("ConnectionChecked"));
    assert_eq!(text(check, &["event_id"]), Some(checked.event_id.as_str()));
    for row in &rows {
        assert_eq!(text(row, &["environment"]), Some("live"));
        assert_eq!(text(row, &["stream_id"]), Some(STREAM));
        assert_eq!(text(row, &["actor", "kind"]), Some("system"));
        assert_eq!(text(row, &["event_time"]), Some(NOW));
    }
    assert_eq!(text(check, &["payload", "connection_id"]), Some(CONNECTION));
    assert_eq!(text(check, &["payload", "occasion"]), Some("connect"));
    assert_eq!(text(check, &["payload", "risk_clock"]), Some(RISK_CLOCK));
    assert_eq!(
        results(check),
        [
            passed("account"),
            passed("contract"),
            passed("environment"),
            passed("scope")
        ]
    );
    let pii = format!("pii_{}", checked.event_id);
    assert_eq!(
        text(check, &["payload", "account_pii_ref"]),
        Some(pii.as_str())
    );
    assert_eq!(
        check.get("pii_refs"),
        Some(&Value::Array(vec![Value::Str(pii)]))
    );
    assert_eq!(opened.get("pii_refs"), Some(&Value::Array(Vec::new())));
    assert_eq!(check.get("causation_id"), Some(&Value::Null));
    no_number_in(&fake);
    let empty = std::collections::BTreeMap::new();
    verify_events(fake.journal.rows(&stream()), TrustedStart::GENESIS, &empty).unwrap();
}

#[test]
fn a_second_check_on_the_same_stream_appends_only_its_check_under_a_new_id() {
    let mut fake = Fake::new(passing());
    let first = connect_check(&args(), &vars(), &mut fake).unwrap();
    let second = connect_check(&args(), &vars(), &mut fake).unwrap();
    assert_ne!(first, second);
    let rows = fake.rows();
    let types: Vec<&str> = rows
        .iter()
        .map(|r| text(r, &["event_type"]).unwrap())
        .collect();
    assert_eq!(
        types,
        ["StreamOpened", "ConnectionChecked", "ConnectionChecked"]
    );
    assert_eq!(fake.calls.last().map(String::as_str), Some("append 1"));
    assert_eq!(
        text(&rows[2], &["event_id"]),
        Some(second.event_id.as_str())
    );
    assert_eq!(
        text(&rows[2], &["payload", "account_pii_ref"]),
        Some(format!("pii_{}", second.event_id).as_str())
    );
}

/// Each failed read is journaled with §8.1's results and refused with the first failed check in
/// §8.1's order (scope, environment, account, contract).
#[test]
fn every_failed_read_is_journaled_and_refused_with_its_first_failed_check() {
    let mut fund = allowlist();
    fund.insert("withdraw_funds".to_owned());
    let mut short = allowlist();
    short.remove("get_equity_orders");
    let cases = [
        (
            "no session",
            Observed {
                contract: None,
                agentic_account_read: false,
            },
            vec![
                failed("account", "account_unreadable"),
                failed("contract", "tools_missing"),
                passed("environment"),
                failed("scope", "scope_mismatch"),
            ],
            ("scope", "scope_mismatch"),
        ),
        (
            "fund movement listed",
            Observed {
                contract: contract(fund, true),
                agentic_account_read: true,
            },
            vec![
                passed("account"),
                passed("contract"),
                passed("environment"),
                failed("scope", "fund_movement"),
            ],
            ("scope", "fund_movement"),
        ),
        (
            "an allowlisted tool missing",
            Observed {
                contract: contract(short, false),
                agentic_account_read: true,
            },
            vec![
                passed("account"),
                failed("contract", "tools_missing"),
                passed("environment"),
                passed("scope"),
            ],
            ("contract", "tools_missing"),
        ),
        (
            "no agentic account",
            Observed {
                contract: contract(allowlist(), true),
                agentic_account_read: false,
            },
            vec![
                failed("account", "account_unreadable"),
                passed("contract"),
                passed("environment"),
                passed("scope"),
            ],
            ("account", "account_unreadable"),
        ),
    ];
    for (name, observed, expected, refusal) in cases {
        let mut fake = Fake::new(observed.clone());
        let error = connect_check(&args(), &vars(), &mut fake).unwrap_err();
        let rows = fake.rows();
        assert_eq!(rows.len(), 2, "{name}");
        let check = &rows[1];
        assert_eq!(results(check), expected, "{name}");
        match error {
            PaperError::ConnectCheck {
                event_id,
                check: failed_check,
                reason,
            } => {
                assert_eq!(
                    Some(event_id.as_str()),
                    text(check, &["event_id"]),
                    "{name}"
                );
                assert_eq!((failed_check, reason), refusal, "{name}");
            }
            other => panic!("{name}: {other:?}"),
        }
        let pii = check.get("payload").and_then(|p| p.get("account_pii_ref"));
        if observed.agentic_account_read {
            assert!(matches!(pii, Some(Value::Str(_))), "{name}");
        } else {
            assert_eq!(pii, Some(&Value::Null), "{name}");
            assert_eq!(
                check.get("pii_refs"),
                Some(&Value::Array(Vec::new())),
                "{name}"
            );
        }
        no_number_in(&fake);
    }
}

#[test]
fn the_refusal_names_the_check_and_never_the_account() {
    let mut fake = Fake::new(Observed {
        contract: None,
        agentic_account_read: false,
    });
    let error = connect_check(&args(), &vars(), &mut fake).unwrap_err();
    let line = error.to_string();
    assert!(line.contains("scope scope_mismatch"), "{line}");
    assert!(!line.contains(NUMBER), "{line}");
}

/// A case refused before the login: its name, the arguments, the environment, and the error's
/// variant.
type Refused = (&'static str, CheckArgs, Vec<(String, String)>, &'static str);

/// Everything refused before the login touches no port but those already passed: no credential
/// is read and nothing is journaled.
#[test]
fn refusals_before_the_login_read_nothing_and_journal_nothing() {
    let paper = CheckArgs {
        acknowledged: Environment::Paper,
        ..args()
    };
    let bad_ref = CheckArgs {
        account_ref: "acct-a".to_owned(),
        ..args()
    };
    let bad_connection = CheckArgs {
        connection: "conn rh".to_owned(),
        ..args()
    };
    let bad_workspace = CheckArgs {
        workspace: "ws r1".to_owned(),
        ..args()
    };
    let host = vec![
        (ACCOUNT_NUMBER_VAR.to_owned(), NUMBER.to_owned()),
        (
            "ALPACA_BASE".to_owned(),
            "https://api.alpaca.markets".to_owned(),
        ),
    ];
    let unset: Vec<(String, String)> = Vec::new();
    let empty = vec![(ACCOUNT_NUMBER_VAR.to_owned(), String::new())];
    let cases: [Refused; 7] = [
        ("paper", paper, vars(), "LiveUnconfirmed"),
        ("account ref", bad_ref, vars(), "Usage"),
        ("connection", bad_connection, vars(), "Usage"),
        ("workspace", bad_workspace, vars(), "Usage"),
        ("host", args(), host, "Shell"),
        ("unset", args(), unset, "Usage"),
        ("empty", args(), empty, "Usage"),
    ];
    for (name, case, environment, kind) in cases {
        let mut fake = Fake::new(passing());
        let error = connect_check(&case, &environment, &mut fake).unwrap_err();
        assert!(format!("{error:?}").starts_with(kind), "{name}: {error:?}");
        assert!(fake.calls.is_empty(), "{name}: {:?}", fake.calls);
        assert!(!error.to_string().contains(NUMBER), "{name}");
    }
}

#[test]
fn a_failed_harden_reads_no_credential() {
    let mut fake = Fake::new(passing());
    fake.harden_fails = true;
    let error = connect_check(&args(), &vars(), &mut fake).unwrap_err();
    assert!(matches!(error, PaperError::Harden), "{error:?}");
    assert_eq!(fake.calls, ["harden"]);
}

#[test]
fn a_failed_login_journals_nothing() {
    let mut fake = Fake::new(passing());
    fake.observed = Err(());
    let error = connect_check(&args(), &vars(), &mut fake).unwrap_err();
    assert!(matches!(error, PaperError::Credentials), "{error:?}");
    assert_eq!(
        fake.calls,
        ["harden".to_owned(), format!("observe {NUMBER}")]
    );
    assert!(fake.rows().is_empty());
}

#[test]
fn an_append_that_did_not_commit_is_a_journal_refusal_and_no_check() {
    for outcome in [
        AppendOutcome::Fenced { current_epoch: 9 },
        AppendOutcome::Unavailable,
        AppendOutcome::Ambiguous,
    ] {
        let mut fake = Fake::new(passing());
        fake.outcome = Some(outcome.clone());
        let error = connect_check(&args(), &vars(), &mut fake).unwrap_err();
        assert!(
            matches!(error, PaperError::Journal),
            "{outcome:?}: {error:?}"
        );
    }
}

#[test]
fn an_append_already_committed_answers_its_check() {
    let mut fake = Fake::new(passing());
    let first = connect_check(&args(), &vars(), &mut fake).unwrap();
    let rows = fake.journal.rows(&stream()).to_vec();
    let mut again = Fake::new(passing());
    again.outcome = Some(AppendOutcome::AlreadyCommitted(rows));
    let second = connect_check(&args(), &vars(), &mut again).unwrap();
    assert_eq!(first, second);
}

#[test]
fn the_arguments_name_ids_and_one_acknowledgement_only() {
    let full = |extra: &[&str]| {
        let mut list = vec![
            "--workspace",
            WORKSPACE,
            "--account-ref",
            ACCOUNT_REF,
            "--connection",
            CONNECTION,
            "--journal",
            "postgres://x",
        ];
        list.extend_from_slice(extra);
        list.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    let parsed = parse_check(full(&["--confirm-live"])).unwrap();
    assert_eq!(
        parsed,
        CheckArgs {
            journal: "postgres://x".to_owned(),
            ..args()
        }
    );
    let paper = parse_check(full(&["--confirm-paper"])).unwrap();
    assert_eq!(paper.acknowledged, Environment::Paper);
    for (name, list) in [
        ("neither", full(&[])),
        ("both", full(&["--confirm-live", "--confirm-paper"])),
        ("unknown", full(&["--confirm-live", "--agent", "a"])),
        ("doubled", full(&["--confirm-live", "--workspace", "w"])),
        ("no value", full(&["--confirm-live", "--journal"])),
        ("place", full(&["--confirm-live", "--place-one-order"])),
    ] {
        let error = parse_check(list).unwrap_err();
        assert!(matches!(error, PaperError::Usage(_)), "{name}: {error:?}");
    }
    for missing in ["--workspace", "--account-ref", "--connection", "--journal"] {
        let mut list = full(&["--confirm-live"]);
        let at = list.iter().position(|a| a == missing).unwrap();
        list.drain(at..at + 2);
        let error = parse_check(list).unwrap_err();
        assert!(
            matches!(&error, PaperError::Usage(m) if m.contains(missing)),
            "{missing}: {error:?}"
        );
    }
}

/// The default build holds no Robinhood host, so the binary's connect check refuses before any
/// login and names neither the account nor the DSN.
#[test]
fn the_default_binary_refuses_the_connect_check_before_any_login() {
    let output = Command::new(env!("CARGO_BIN_EXE_mandate-paper"))
        .args([
            "--connect-check",
            "--workspace",
            WORKSPACE,
            "--account-ref",
            ACCOUNT_REF,
            "--connection",
            CONNECTION,
            "--journal",
            "postgres://paper:hunter2-sentinel@db-sentinel.invalid:1/journal",
            "--confirm-live",
        ])
        .env_clear()
        .env(ACCOUNT_NUMBER_VAR, NUMBER)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stdout, "");
    assert_eq!(stderr, "this build holds no Robinhood host\n");
    for secret in [NUMBER, "hunter2", "db-sentinel"] {
        assert!(!stderr.contains(secret), "{stderr}");
    }
}

/// The seam over the simulator's wire, recording each tool called.
struct SimTools {
    wire: RefCell<Wire>,
    seen: Rc<RefCell<Vec<&'static str>>>,
}

impl Tools for SimTools {
    async fn call_tool(
        &self,
        _class: CallClass,
        tool: &'static str,
        args: &serde_json::Value,
    ) -> Result<String, McpError> {
        self.seen.borrow_mut().push(tool);
        let reply = self.wire.borrow_mut().call_raw(tool, args.clone());
        let message: serde_json::Value =
            serde_json::from_str(&reply.ok_or(McpError::Network)?.body)
                .map_err(|_| McpError::Malformed)?;
        let result = message.get("result").ok_or(McpError::Malformed)?;
        Ok(result.to_string())
    }
}

/// The account read against the simulator: with one agentic account, the recorded agentic one
/// reads, a recorded account that is not agentic or not listed does not; with two agentic
/// accounts (the shared fixture's) none reads (DEC-875 item 1); and without a contract nothing
/// is called.
#[test]
fn the_account_read_against_the_simulator_admits_only_the_recorded_agentic_account() {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let one = Sim::new(vec![
        account(AGENTIC, true, false),
        account(NOT_AGENTIC, false, false),
    ])
    .unwrap();
    let server = SimServer::start(one, Variant::Honest).unwrap();
    let url = server.url().unwrap();
    let names: BTreeSet<String> = Wire::connect(&url)
        .list()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(ALLOWLIST.iter().all(|name| names.contains(*name)));
    let seen = contract(names, true);
    for (number, read) in [(AGENTIC, true), (NOT_AGENTIC, false), ("5QR00009", false)] {
        let tools = SimTools {
            wire: RefCell::new(Wire::connect(&url)),
            seen: Rc::default(),
        };
        let connector = RobinhoodConnector::new(tools, number.to_owned());
        let observed = runtime.block_on(observe(seen.clone(), &connector));
        assert_eq!(
            observed,
            Observed {
                contract: seen.clone(),
                agentic_account_read: read
            },
            "{number}"
        );
    }
    let two = SimServer::start(sim().unwrap(), Variant::Honest).unwrap();
    let tools = SimTools {
        wire: RefCell::new(Wire::connect(&two.url().unwrap())),
        seen: Rc::default(),
    };
    let connector = RobinhoodConnector::new(tools, AGENTIC.to_owned());
    let observed = runtime.block_on(observe(seen.clone(), &connector));
    assert!(!observed.agentic_account_read, "two agentic accounts");
    let seen_calls: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let tools = SimTools {
        wire: RefCell::new(Wire::connect(&url)),
        seen: Rc::clone(&seen_calls),
    };
    let connector = RobinhoodConnector::new(tools, AGENTIC.to_owned());
    let observed = runtime.block_on(observe(None, &connector));
    assert!(!observed.agentic_account_read);
    assert!(seen_calls.borrow().is_empty(), "{:?}", seen_calls.borrow());
}
