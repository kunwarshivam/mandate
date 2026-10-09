//! K1b (E7-11's first slice; DEC-529 items 3 and 11, DEC-800): `mandate connection record` commits
//! `ConnectionEstablished` version 2 (journal spec §9.8) from the CLI, naming the connect
//! sequence's passing `ConnectionChecked` as its cause, and reaches no broker. Every account
//! stream event here is a fixture written as the executor would write it; Robinhood is fixtures
//! only (connections spec §12, `AGENTS.md` rule 8). Cases are written as words, one case a line.

mod common;

use clap::Parser;
use common::{ACCOUNT, ACCOUNT_STREAM, CONTROL, FixedIds, Journal, OWNER, WORKSPACE};
use common::{at, body, int, member, member_text, object, stream, text};
use mandate_canon::{Value, to_canonical};
use mandate_cli::connection::{CODES, ConnectionCommand, RecordArgs, Recorded, Request};
use mandate_cli::connection::{record, run};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_cli::postgres::JournalArgs;
use mandate_cli::{Cli, Command};
use mandate_journal::{AppendOutcome, Environment};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const NOW: i64 = 1_790_000_000;
const CONN: &str = "conn_rh_live_01";
const OTHER_ACCOUNT: &str = "01J0ACC0VNT000000000000001";
/// The personal-data reference the check names: nothing the command prints may carry it.
const PII: &str = "pii-k1b-sentinel";
/// The checks every connection needs, with Robinhood's MCP contract.
const REQUIRED: [&str; 4] = ["account", "contract", "environment", "scope"];
const CHECK: &str = "10000000000000000000000001";
const DSN: &str = "postgresql://k1b-user-sentinel@k1b-host-sentinel.invalid/k1b-db-sentinel";

fn environment(word: &str) -> Environment {
    match word {
        "paper" => Environment::Paper,
        "backtest" => Environment::Backtest,
        _ => Environment::Live,
    }
}

/// Robinhood's tools, out of order and one repeated: the record lists them once, ascending.
fn request() -> Request {
    Request {
        connection_id: CONN.to_owned(),
        broker: "robinhood".to_owned(),
        scopes: [
            "review_equity_order",
            "get_accounts",
            "place_equity_order",
            "get_accounts",
        ]
        .map(str::to_owned)
        .to_vec(),
        account_ref: ACCOUNT.to_owned(),
        checked: CHECK.to_owned(),
        margin_attestation: Some("cash_account".to_owned()),
    }
}

/// The request with one member changed; scopes are joined by `+`, and `-` is none.
fn with(member: &str, value: &str) -> Request {
    let mut r = request();
    let value = value.to_owned();
    match member {
        "connection" => r.connection_id = value,
        "account" => r.account_ref = value,
        "broker" => r.broker = value,
        "checked" => r.checked = value,
        "scopes" => {
            r.scopes = value
                .split('+')
                .filter(|s| *s != "-")
                .map(str::to_owned)
                .collect()
        }
        "attest" => r.margin_attestation = Some(value).filter(|v| v != "-"),
        _ => {}
    }
    r
}

/// Appends one event to `on` as its writer would, in `env`.
fn seed(j: &mut Journal, on: &str, id: &str, kind: (&str, i64, &str), payload: Value) {
    let s = stream(on);
    let epoch = j.take_ownership(&s).unwrap();
    let head = j.head(&s).unwrap().seq;
    let envelope = object(&[
        ("causation_id", Value::Null),
        ("environment", text(kind.2)),
        ("event_id", text(id)),
        ("event_type", text(kind.0)),
        ("payload", payload),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", int(kind.1)),
        ("stream_id", text(on)),
    ]);
    let outcome = j.append(&s, head, epoch, at(NOW).at, &[&to_canonical(&envelope)]);
    assert!(
        matches!(outcome, Ok(AppendOutcome::Committed(_))),
        "{outcome:?}"
    );
}

/// A `ConnectionChecked` on `on`: `results` names each listed check by its initial, `+` passed
/// and `-` failed, as in `a+c+e+s+`; listed by check, as rule 57 has them.
fn check(j: &mut Journal, on: &str, occasion: &str, results: &str, pii: bool, env: &str) {
    let listed = results.as_bytes().chunks(2).map(|pair| {
        let check = REQUIRED
            .iter()
            .find(|c| c.as_bytes()[0] == pair[0])
            .unwrap();
        let passed = pair[1] == b'+';
        let reason = if passed {
            Value::Null
        } else {
            text("scope_mismatch")
        };
        let result = text(if passed { "passed" } else { "failed" });
        object(&[
            ("check", text(check)),
            ("reason", reason),
            ("result", result),
        ])
    });
    let payload = object(&[
        ("account_pii_ref", if pii { text(PII) } else { Value::Null }),
        ("connection_id", text(CONN)),
        ("occasion", text(occasion)),
        ("results", Value::Array(listed.collect())),
        ("risk_clock", text(&at(NOW).at.to_string())),
    ]);
    seed(j, on, CHECK, ("ConnectionChecked", 1, env), payload);
}

fn passing(occasion: &str) -> Journal {
    let mut j = Journal::default();
    check(&mut j, ACCOUNT_STREAM, occasion, "a+c+e+s+", true, "live");
    j
}

/// Control-stream history in words: `E:<conn>:<A|B|1>` a Robinhood live establishment of this
/// account (`A`), the other (`B`), or version 1 with none (`1`); `R:<conn>` a revocation.
fn history(j: &mut Journal, events: &str) {
    for (n, event) in events.split_whitespace().enumerate() {
        let id = format!("2{n:025}");
        let words: Vec<&str> = event.split(':').collect();
        let mut members = vec![("connection_id", text(words[1]))];
        if words[0] == "R" {
            seed(
                j,
                CONTROL,
                &id,
                ("ConnectionRevoked", 1, "live"),
                object(&members),
            );
            continue;
        }
        members.extend([("broker", text("robinhood")), ("environment", text("live"))]);
        members.push(("scopes", Value::Array(vec![text("get_accounts")])));
        let account = [("A", ACCOUNT), ("B", OTHER_ACCOUNT)]
            .into_iter()
            .find(|a| a.0 == words[2]);
        members.extend(account.map(|a| ("account_ref", text(a.1))));
        let kind = (
            "ConnectionEstablished",
            if account.is_some() { 2 } else { 1 },
            "live",
        );
        seed(j, CONTROL, &id, kind, object(&members));
    }
}

type Outcome = (Result<Recorded, ControlError>, String);

/// `record` as the owner in `env`, and what it printed.
fn attempt(j: &mut Journal, ids: &mut FixedIds, req: &Request, code: Option<&str>) -> Outcome {
    attempt_in("live", j, ids, req, code)
}

fn attempt_in(
    env: &str,
    j: &mut Journal,
    ids: &mut FixedIds,
    req: &Request,
    code: Option<&str>,
) -> Outcome {
    let owner = Owner {
        workspace: WORKSPACE.to_owned(),
        user: OWNER.to_owned(),
        environment: environment(env),
    };
    let mut out = Vec::new();
    let result = record(j, ids, &owner, req, code, at(NOW), &mut out);
    (result, String::from_utf8(out).unwrap())
}

/// The code `record` shows for `req`, having committed and minted nothing.
fn shown(j: &mut Journal, req: &Request) -> String {
    let mut ids = FixedIds::default();
    let rows = j.rows(&stream(CONTROL)).unwrap().len();
    let Recorded::Shown { code } = attempt(j, &mut ids, req, None).0.unwrap() else {
        panic!("not shown");
    };
    assert_eq!(
        j.rows(&stream(CONTROL)).unwrap().len(),
        rows,
        "showing commits nothing"
    );
    assert_eq!(ids.assertions, 0, "showing mints no assertion");
    code
}

/// Shows `req`, then records it with the code shown; a refusal to show is the outcome.
fn confirmed(j: &mut Journal, ids: &mut FixedIds, req: &Request) -> Outcome {
    let (result, out) = attempt(j, ids, req, None);
    let Ok(Recorded::Shown { code }) = result else {
        return (result, out);
    };
    attempt(j, ids, req, Some(&code))
}

fn refused(reason: &str) -> Result<Recorded, ControlError> {
    let reason = CODES
        .iter()
        .find(|c| **c == reason)
        .expect("a code `record` gives");
    Err(ControlError::Refused { reason })
}

/// A refusal leaves no trace: nothing printed, minted, or committed.
fn untouched(j: &Journal, ids: &FixedIds, out: &str, control_rows: usize) -> bool {
    out.is_empty() && ids.assertions == 0 && j.rows(&stream(CONTROL)).unwrap().len() == control_rows
}

#[test]
#[ignore = "pending E7-11"]
fn records_a_checked_live_robinhood_connection_as_version_2() {
    let mut j = passing("connect");
    let alpaca = r#"{"broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","scopes":["data","trading"]}"#;
    let alpaca = mandate_canon::parse(alpaca.as_bytes()).unwrap();
    seed(
        &mut j,
        CONTROL,
        "20000000000000000000000009",
        ("ConnectionEstablished", 1, "paper"),
        alpaca,
    );
    let mut ids = FixedIds::default();
    let (result, printed) = confirmed(&mut j, &mut ids, &request());
    let rows = j.rows(&stream(CONTROL)).unwrap();
    let row = rows.last().unwrap();
    let (event_id, seq) = (row.event_id.clone(), row.seq);
    assert_eq!(
        result.unwrap(),
        Recorded::Committed(Submitted { event_id, seq })
    );
    assert_eq!(
        (row.seq, row.event_type.as_str(), row.schema_version),
        (2, "ConnectionEstablished", 2)
    );
    let event = body(row);
    assert_eq!(member_text(&event, "causation_id"), Some(CHECK), "rule 63");
    assert_eq!(member_text(&event, "environment"), Some("live"));
    assert_eq!(member_text(&event, "actor.id"), Some(OWNER));
    assert_eq!(member(&event, "pii_refs"), Some(&Value::Array(Vec::new())));
    let expected = format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"robinhood","connection_id":"{CONN}","environment":"live","margin_attestation":"cash_account","scopes":["get_accounts","place_equity_order","review_equity_order"],"step_up":{{"assertion_id":"cli-assertion-1","authenticated_at":"{}","method":"cli_confirm"}},"user":"{OWNER}"}}"#,
        at(NOW).at
    );
    let payload = member(&event, "payload").map(to_canonical);
    assert_eq!(payload.map(String::from_utf8), Some(Ok(expected)));
    assert_eq!(ids.assertions, 1);
    assert!(
        printed.contains(&format!("{} at seq 2", row.event_id)),
        "{printed}"
    );
    assert!(!printed.contains(PII), "{printed}");
}

#[test]
#[ignore = "pending E7-11"]
fn a_rerun_with_the_same_code_answers_the_committed_event() {
    let mut j = passing("connect");
    let mut ids = FixedIds::default();
    let code = shown(&mut j, &request());
    let first = attempt(&mut j, &mut ids, &request(), Some(&code))
        .0
        .unwrap();
    let again = attempt(&mut j, &mut ids, &request(), Some(&code))
        .0
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(j.rows(&stream(CONTROL)).unwrap().len(), 1);
    assert_eq!(ids.assertions, 1, "the re-run spends no assertion");
}

#[test]
#[ignore = "pending E7-11"]
fn the_code_binds_the_record_it_shows() {
    let mut j = passing("connect");
    let mut ids = FixedIds::default();
    let (result, printed) = attempt(&mut j, &mut ids, &request(), None);
    let Recorded::Shown { code } = result.unwrap() else {
        panic!("not shown");
    };
    for shown in [
        CONN,
        "robinhood",
        "live",
        ACCOUNT,
        "get_accounts",
        "margin",
        &code,
    ] {
        assert!(printed.contains(shown), "{shown} in {printed}");
    }
    assert!(!printed.contains(PII), "{printed}");
    let more = "get_accounts+place_equity_order+review_equity_order+cancel_equity_order";
    let others = [with("scopes", "get_accounts"), with("scopes", more)];
    for other in others {
        let (result, out) = attempt(&mut j, &mut ids, &other, Some(&code));
        assert_eq!(result, refused("code_mismatch"), "{other:?}");
        assert!(untouched(&j, &ids, &out, 0));
    }
}

#[test]
#[ignore = "pending E7-11"]
fn refusals_on_the_request_touch_nothing() {
    let cases = "connection conn*rh live connection_id_invalid
                 account not-a-ulid live account_ref_invalid
                 account 81J0ACC0VNT000000000000000 live account_ref_invalid
                 broker kraken_derivatives_us live broker_unsupported
                 broker alpaca live environment_refused
                 none - paper environment_refused
                 none - backtest environment_refused
                 scopes - live scopes_missing
                 attest - live attestation_missing
                 attest cash live attestation_invalid
                 broker alpaca paper attestation_not_live";
    for case in cases.lines() {
        let [member, value, env, reason] = case.split_whitespace().collect::<Vec<_>>()[..] else {
            panic!("{case}");
        };
        let mut j = passing("connect");
        let mut ids = FixedIds::default();
        let (result, out) = attempt_in(env, &mut j, &mut ids, &with(member, value), Some("0"));
        assert_eq!(result, refused(reason), "{case}");
        assert!(
            untouched(&j, &ids, &out, 0) && j.attempts.is_empty(),
            "{case}"
        );
    }
}

#[test]
#[ignore = "pending E7-11"]
fn the_cause_is_the_passing_connect_check_on_the_bound_stream() {
    let other_stream = format!("acct:{WORKSPACE}:{OTHER_ACCOUNT}");
    let cases = "other connect a+c+e+s+ pii live check_missing
                 own executor_start a+c+e+s+ pii live check_missing
                 own reconnect a+c+e+s+ pii live check_missing
                 own connect a+c+e+s- pii live check_failed
                 own connect c+e+s+ pii live check_failed
                 own connect a+e+s+ pii live check_failed
                 own connect a+c+e+s+ none live check_failed
                 own connect a+c+e+s+ pii paper check_failed";
    for case in cases.lines() {
        let [on, occasion, results, pii, env, reason] =
            case.split_whitespace().collect::<Vec<_>>()[..]
        else {
            panic!("{case}");
        };
        let mut j = Journal::default();
        let on = if on == "own" {
            ACCOUNT_STREAM
        } else {
            &other_stream
        };
        check(&mut j, on, occasion, results, pii == "pii", env);
        let mut ids = FixedIds::default();
        let (result, out) = attempt(&mut j, &mut ids, &request(), Some("0"));
        assert_eq!(result, refused(reason), "{case}");
        assert!(untouched(&j, &ids, &out, 0), "{case}");
    }
    let mut j = passing("connect");
    let mut ids = FixedIds::default();
    let other = with("connection", "conn_rh_live_02");
    assert_eq!(
        attempt(&mut j, &mut ids, &other, None).0,
        refused("check_other_connection")
    );
    let mut j = Journal::default();
    seed(
        &mut j,
        ACCOUNT_STREAM,
        CHECK,
        ("AccountStateObserved", 1, "live"),
        object(&[]),
    );
    assert_eq!(
        attempt(&mut j, &mut ids, &request(), None).0,
        refused("check_missing")
    );
}

#[test]
#[ignore = "pending E7-11"]
fn one_account_one_connection_on_the_control_stream() {
    let cases = "E:conn_rh_live_01:A connection_exists
                 E:conn_rh_live_01:A R:conn_rh_live_01 E:conn_rh_live_01:A connection_exists
                 E:conn_rh_live_01:1 R:conn_rh_live_01 reconnect_mismatch
                 E:conn_rh_live_01:B R:conn_rh_live_01 reconnect_mismatch
                 E:conn_rh_live_00:A R:conn_rh_live_00 account_ref_bound
                 E:conn_rh_live_00:B R:conn_rh_live_00 account_maybe_connected
                 E:conn_rh_live_01:A R:conn_rh_live_01 reconnects";
    for case in cases.lines() {
        let (events, reason) = case.trim().rsplit_once(' ').unwrap();
        let reconnect = events.starts_with("E:conn_rh_live_01");
        let mut j = passing(if reconnect { "reconnect" } else { "connect" });
        history(&mut j, events);
        let before = j.rows(&stream(CONTROL)).unwrap().len();
        let mut ids = FixedIds::default();
        let (result, out) = confirmed(&mut j, &mut ids, &request());
        if reason != "reconnects" {
            assert_eq!(result, refused(reason), "{case}");
            assert!(untouched(&j, &ids, &out, before), "{case}");
            continue;
        }
        assert!(
            matches!(result, Ok(Recorded::Committed(ref s)) if s.seq == 3),
            "{result:?}"
        );
        let rows = j.rows(&stream(CONTROL)).unwrap();
        let last = body(rows.last().unwrap());
        assert_eq!(member_text(&last, "payload.account_ref"), Some(ACCOUNT));
        assert_eq!(member_text(&last, "causation_id"), Some(CHECK));
    }
}

/// Whatever the check recorded, the command commits exactly when every check it needs is listed
/// and passed and the account was read; the oracle reads the drawn results itself.
#[test]
#[ignore = "pending E7-11"]
fn commits_exactly_when_every_needed_check_passed() {
    let drawn = (prop::collection::vec(0..3u8, 4), any::<bool>());
    TestRunner::deterministic()
        .run(&drawn, |(drawn, pii)| {
            let mut results = String::new();
            for (check, d) in REQUIRED.iter().zip(&drawn).filter(|(_, d)| **d > 0) {
                results.push_str(&check[..1]);
                results.push(if *d == 1 { '+' } else { '-' });
            }
            let passes = drawn.iter().all(|d| *d == 1) && pii;
            let mut j = Journal::default();
            check(&mut j, ACCOUNT_STREAM, "connect", &results, pii, "live");
            let (result, _) = confirmed(&mut j, &mut FixedIds::default(), &request());
            if passes {
                prop_assert!(matches!(result, Ok(Recorded::Committed(_))), "{result:?}");
            } else {
                prop_assert_eq!(result, refused("check_failed"), "{} {}", results, pii);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
#[ignore = "pending E7-11"]
fn run_refuses_a_bad_request_before_opening_the_journal() {
    let store = std::env::temp_dir().join(format!("mandate-cli-k1b-{}", std::process::id()));
    std::fs::remove_dir_all(&store).ok();
    let cases = "demo robinhood environment_invalid
                 paper robinhood environment_refused
                 live alpaca environment_refused
                 live etrade broker_unsupported";
    for case in cases.lines() {
        let [env, broker, reason] = case.split_whitespace().collect::<Vec<_>>()[..] else {
            panic!("{case}");
        };
        let target = JournalArgs {
            journal: DSN.into(),
            store: store.clone(),
        };
        let (workspace, user) = (WORKSPACE.to_owned(), OWNER.to_owned());
        let (environment, connection) = (env.to_owned(), with("broker", broker));
        let args = RecordArgs {
            workspace,
            user,
            environment,
            connection,
            code: None,
            target,
        };
        let mut out = Vec::new();
        let error = format!("{:#}", run(&args, at(NOW), &mut out).unwrap_err());
        assert!(
            error.contains(reason) && !error.contains("sentinel"),
            "{error}"
        );
        assert!(out.is_empty() && !store.exists(), "{case}");
    }
}

#[test]
fn the_command_line_takes_each_scope_and_the_attestation() {
    let line = "mandate connection record --workspace ws1 --user user-owner --environment live \
                --connection conn_rh_live_01 --broker robinhood --scope get_accounts --scope \
                place_equity_order --account-ref 01J0ACC0VNT000000000000000 --checked \
                10000000000000000000000001 --margin-attestation margin_disabled --journal postgresql://h/db \
                --store /tmp/s";
    let cli = Cli::try_parse_from(line.split_whitespace()).unwrap();
    let Command::Connection(ConnectionCommand::Record(parsed)) = cli.command else {
        panic!("not a connection record");
    };
    assert_eq!(
        parsed.connection.scopes,
        ["get_accounts", "place_equity_order"]
    );
    assert_eq!(
        parsed.connection.margin_attestation.as_deref(),
        Some("margin_disabled")
    );
    assert!(parsed.code.is_none());
    assert_eq!(parsed.environment, "live");
}
