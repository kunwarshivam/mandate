//! The runner's live entry (E7-26, the [first live trade
//! brief](../../../docs/project/tasks/first-live-trade.md)'s G1a, LT-1 and LT-9 rows,
//! [DEC-937](../../../docs/project/decisions/DEC-937.md)): the acknowledgement a live run needs,
//! the core dumps it turns off before any credential, and the one Robinhood endpoint each build
//! may dial. No test here logs in, holds a credential or reaches a host (ES-19, LT-1,
//! `AGENTS.md` rule 8): the endpoint is judged as text, and the login itself runs only against
//! the simulator in a rehearsal build. CI builds this suite in the default build alone; the
//! pins that read the build branch on `cfg!(feature = "live")` so a local `live` build reads
//! its own answer.

use std::cell::Cell;
use std::path::PathBuf;

use mandate_alpaca::AlpacaPaperHttp;
use mandate_domain::Environment;
use mandate_paper::{
    ACCOUNT_NUMBER_VAR, Args, Broker, Build, Connection, Dial, EndpointRefusal, PaperError, Ports,
    Production, REHEARSAL_ENDPOINT_VAR, REHEARSAL_HOST, RobinhoodEndpoint, SystemClock,
    acknowledge, dial, disable_core_dumps, endpoint, parse, pinned_host, run_clock,
};
use mandate_shell::control::ControlRecord;
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::process::{DumpableBehavior, dumpable_behavior};
use rustix::process::{Resource, Rlimit, getrlimit};

const IDS: &str = "--workspace w --agent a --account-ref r --store s --bars b";

fn parsed(text: &str) -> Result<Args, PaperError> {
    parse(text.split_whitespace().map(str::to_owned))
}

/// Ports that count the hardenings and connections `acknowledge` asks for, and whose hardening
/// fails when `fails` says so.
struct Counting {
    hardens: Cell<usize>,
    connects: Cell<usize>,
    fails: bool,
}

impl Counting {
    fn new(fails: bool) -> Self {
        Self {
            hardens: Cell::new(0),
            connects: Cell::new(0),
            fails,
        }
    }
}

impl Ports for Counting {
    type Session = AlpacaPaperHttp;
    type Pause = SystemClock;

    fn control_stream(&mut self, _workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        Err(PaperError::Control)
    }

    fn connect(
        &mut self,
        _connection: &Connection,
        _dial: Option<&Dial>,
    ) -> Result<AlpacaPaperHttp, PaperError> {
        self.connects.set(self.connects.get() + 1);
        Err(PaperError::Credentials)
    }

    fn harden(&mut self) -> Result<(), PaperError> {
        self.hardens.set(self.hardens.get() + 1);
        if self.fails {
            Err(PaperError::Harden)
        } else {
            Ok(())
        }
    }

    fn pause(&self) -> SystemClock {
        SystemClock
    }
}

fn connection(broker: Broker, environment: Environment) -> Connection {
    Connection {
        broker,
        environment,
        account_ref: "r".to_owned(),
    }
}

fn args(acknowledged: Environment) -> Args {
    Args {
        workspace: "w".to_owned(),
        agent: "a".to_owned(),
        account_ref: "r".to_owned(),
        journal: None,
        store: PathBuf::from("s"),
        bars: PathBuf::from("b"),
        place_one_order: false,
        reconcile_only: false,
        acknowledged,
    }
}

/// DEC-937 item 3: `--confirm-live` is the live acknowledgement and `--confirm-paper` the paper
/// one, each parsed to the environment it names; exactly one is required, so a run giving both
/// or neither is a usage error before anything runs.
#[test]
fn each_acknowledgement_names_its_environment_and_exactly_one_is_required() {
    let live = parsed(&format!("{IDS} --confirm-live")).map(|args| args.acknowledged);
    assert_eq!(live.ok(), Some(Environment::Live));
    let paper = parsed(&format!("{IDS} --confirm-paper")).map(|args| args.acknowledged);
    assert_eq!(paper.ok(), Some(Environment::Paper));
    for text in [
        IDS.to_owned(),
        format!("{IDS} --confirm-paper --confirm-live"),
        format!("{IDS} --confirm-live --confirm-paper"),
    ] {
        let refused = parsed(&text);
        assert!(
            matches!(refused, Err(PaperError::Usage(_))),
            "{text}: {refused:?}"
        );
    }
}

/// DEC-937 item 3 and LT-9: a live connection without `--confirm-live` refuses before any
/// credential is read and leaves the process as it was; with it, the process's core dumps are
/// turned off, once, and still nothing is connected; a paper connection keeps today's run
/// under `--confirm-paper` and refuses under `--confirm-live`, hardening nothing either way.
#[test]
fn a_live_connection_needs_its_acknowledgement_and_hardens_before_any_credential() {
    let live = connection(Broker::Robinhood, Environment::Live);
    let paper = connection(Broker::Alpaca, Environment::Paper);
    let cases = [
        (&live, Environment::Paper, Err("LiveUnconfirmed"), 0),
        (&live, Environment::Live, Ok(()), 1),
        (&paper, Environment::Paper, Ok(()), 0),
        (&paper, Environment::Live, Err("PaperUnconfirmed"), 0),
    ];
    for (chosen, acknowledged, expected, hardens) in cases {
        let mut ports = Counting::new(false);
        let outcome = acknowledge(chosen, &args(acknowledged), &mut ports);
        let named = outcome.map_err(|refused| match refused {
            PaperError::LiveUnconfirmed => "LiveUnconfirmed",
            PaperError::PaperUnconfirmed => "PaperUnconfirmed",
            _ => "another refusal",
        });
        let case = format!("{:?} acknowledged {acknowledged:?}", chosen.environment);
        assert_eq!(named, expected, "{case}");
        assert_eq!(ports.hardens.get(), hardens, "{case}: hardenings");
        assert_eq!(ports.connects.get(), 0, "{case}: no credential is read");
    }
}

/// LT-9: a live run whose core dumps cannot be turned off stops there, with that refusal, and
/// reads no credential.
#[test]
fn a_failed_hardening_stops_the_live_run() {
    let mut ports = Counting::new(true);
    let live = connection(Broker::Robinhood, Environment::Live);
    let outcome = acknowledge(&live, &args(Environment::Live), &mut ports);
    assert!(matches!(outcome, Err(PaperError::Harden)), "{outcome:?}");
    assert_eq!((ports.hardens.get(), ports.connects.get()), (1, 0));
}

/// Each acknowledgement refusal names the flag the run needs and no value (ES-09, rule 7).
#[test]
fn the_acknowledgement_refusals_name_the_flag_they_need() {
    assert_eq!(
        PaperError::LiveUnconfirmed.to_string(),
        "the confirmed mandate is live; a live run needs --confirm-live"
    );
    assert_eq!(
        PaperError::PaperUnconfirmed.to_string(),
        "the confirmed mandate is paper; a paper run needs --confirm-paper"
    );
    assert_eq!(
        PaperError::Harden.to_string(),
        "core dumps could not be turned off"
    );
}

/// LT-9 (the brief's credential-leak row: "the runner disables core dumps at start"): after
/// `disable_core_dumps` the process's core-file limit is zero, soft and hard, so no core file
/// is written, and the process is not dumpable, so neither a pipe handler nor a same-user
/// `ptrace` reads its memory. The oracle reads both back from the kernel.
#[test]
fn hardening_turns_the_core_dumps_off() {
    disable_core_dumps().expect("the kernel takes both settings");
    assert_no_core_dumps();
}

/// LT-9 through the binary's own ports: `Production::harden` is the same switch, read back from
/// the kernel. Each test runs in its own process under nextest, so neither reads the other's.
#[test]
fn the_binarys_ports_harden_the_process() {
    let mut ports = Production {
        journal: None,
        clock: run_clock(&[]).expect("no clock variable"),
    };
    ports.harden().expect("the kernel takes both settings");
    assert_no_core_dumps();
}

/// The process's core-file limit is zero, soft and hard, on every unix target, and on Linux it
/// is not dumpable either (DEC-954): macOS has no dumpable flag, and CI runs this there too.
fn assert_no_core_dumps() {
    let limit = getrlimit(Resource::Core);
    let expected = Rlimit {
        current: Some(0),
        maximum: Some(0),
    };
    assert_eq!(
        (limit.current, limit.maximum),
        (expected.current, expected.maximum)
    );
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert_eq!(
        dumpable_behavior().expect("the kernel answers"),
        DumpableBehavior::NotDumpable
    );
}

/// LT-1 (DEC-937 item 2): the default build holds no Robinhood host, so it has no endpoint to
/// dial whatever the environment says; a `live` build dials exactly Robinhood's published
/// endpoint, and refuses a rehearsal endpoint set beside it rather than ever reading it.
#[test]
fn only_a_live_build_holds_the_robinhood_endpoint() {
    for rehearsal in [None, Some("http://127.0.0.1:8080/mcp")] {
        assert_eq!(
            endpoint(Build::Default, rehearsal),
            Err(EndpointRefusal::NotBuilt),
            "{rehearsal:?}"
        );
    }
    let pinned = endpoint(Build::Live, None);
    if cfg!(all(feature = "live", not(feature = "rh-sim"))) {
        let expected = RobinhoodEndpoint {
            host: "agent.robinhood.com".to_owned(),
            url: "https://agent.robinhood.com/mcp/trading".to_owned(),
        };
        assert_eq!(pinned, Ok(expected));
    } else {
        assert_eq!(pinned, Err(EndpointRefusal::NotBuilt));
    }
    assert_eq!(
        endpoint(Build::Live, Some("http://127.0.0.1:8080/mcp")),
        Err(EndpointRefusal::RehearsalInLive)
    );
}

/// DEC-922 items 1 and 2: the rehearsal build dials only the simulator's endpoint, which must
/// be plain `http` on an IPv4 loopback address with a port and no credentials, query or
/// fragment; anything else — Robinhood's own host above all — is refused before anything is
/// sent, and the build with no endpoint given refuses too.
#[test]
fn the_rehearsal_dials_only_a_loopback_endpoint() {
    assert_eq!(
        endpoint(Build::Rehearsal, None),
        Err(EndpointRefusal::RehearsalUnset)
    );
    for (text, host) in [
        ("http://127.0.0.1:8080/mcp", "127.0.0.1"),
        ("http://127.0.0.1:65535/mcp/trading", "127.0.0.1"),
        ("http://127.1.2.3:9/", "127.1.2.3"),
    ] {
        let expected = RobinhoodEndpoint {
            host: host.to_owned(),
            url: text.to_owned(),
        };
        assert_eq!(
            endpoint(Build::Rehearsal, Some(text)),
            Ok(expected),
            "{text}"
        );
    }
    for text in [
        "https://agent.robinhood.com/mcp/trading",
        "http://agent.robinhood.com/mcp/trading",
        "https://127.0.0.1:8080/mcp",
        "http://localhost:8080/mcp",
        "http://10.0.0.1:8080/mcp",
        "http://128.0.0.1:8080/mcp",
        "http://[::1]:8080/mcp",
        "http://127.0.0.1/mcp",
        "http://127.0.0.1:/mcp",
        "http://127.0.0.1:0/mcp",
        "http://127.0.0.1:65536/mcp",
        "http://127.0.0.1:80a/mcp",
        "http://127.0.0.1:8080",
        "http://user@127.0.0.1:8080/mcp",
        "http://127.0.0.1:8080/mcp?x=1",
        "http://127.0.0.1:8080/mcp#x",
        "http://127.0.0.1:8080@agent.robinhood.com/mcp",
        "HTTP://127.0.0.1:8080/mcp",
        "",
    ] {
        assert_eq!(
            endpoint(Build::Rehearsal, Some(text)),
            Err(EndpointRefusal::OffLoopback),
            "{text}"
        );
    }
}

/// The variable the rehearsal reads is named in one place, and each endpoint refusal names
/// itself and its code (ES-09).
#[test]
fn every_endpoint_refusal_names_itself_and_its_code() {
    assert_eq!(REHEARSAL_ENDPOINT_VAR, "MANDATE_RH_SIM_ENDPOINT");
    let cases = [
        (
            EndpointRefusal::NotBuilt,
            "this build holds no Robinhood host",
            "robinhood_not_built",
        ),
        (
            EndpointRefusal::RehearsalInLive,
            "MANDATE_RH_SIM_ENDPOINT is set in a build that is not the rehearsal",
            "rehearsal_endpoint_in_live",
        ),
        (
            EndpointRefusal::RehearsalUnset,
            "the rehearsal build needs MANDATE_RH_SIM_ENDPOINT",
            "rehearsal_endpoint_unset",
        ),
        (
            EndpointRefusal::OffLoopback,
            "the rehearsal endpoint is not plain http on a loopback address",
            "rehearsal_off_loopback",
        ),
        (
            EndpointRefusal::AccountUnset,
            "MANDATE_RH_ACCOUNT_NUMBER is not set",
            "robinhood_account_unset",
        ),
    ];
    for (refusal, message, code) in cases {
        assert_eq!(
            (refusal.to_string(), refusal.code()),
            (message.to_owned(), code)
        );
        let wrapped = PaperError::Endpoint(refusal);
        assert_eq!(
            wrapped.to_string(),
            message,
            "the run's report carries it verbatim"
        );
    }
}

/// LT-1's build test: the binary CI and every release build is the default one, and it carries
/// no Robinhood host at all, so nothing it runs can dial one; a `live` build carries the one
/// pinned host.
#[test]
fn the_default_binary_holds_no_robinhood_host() {
    let binary = std::fs::read(env!("CARGO_BIN_EXE_mandate-paper")).expect("the binary is built");
    let holds = binary
        .windows(b"robinhood.com".len())
        .any(|window| window == b"robinhood.com");
    assert_eq!(holds, cfg!(all(feature = "live", not(feature = "rh-sim"))));
}

/// DEC-947 item 3 (DEC-922 items 1 and 2, DEC-924 item 3): each build pins one host at most.
/// The default build pins none; the rehearsal build pins the simulator's loopback host, the
/// address the simulator listens on; and only a `live` build without `rh-sim` pins Robinhood's
/// published host, so the rehearsal build holds no production constant at all.
#[test]
fn each_build_pins_its_own_host_and_the_rehearsal_pins_loopback() {
    assert_eq!(REHEARSAL_HOST, "127.0.0.1");
    assert_eq!(pinned_host(Build::Default), None);
    assert_eq!(pinned_host(Build::Rehearsal), Some(REHEARSAL_HOST));
    let live = pinned_host(Build::Live);
    if cfg!(all(feature = "live", not(feature = "rh-sim"))) {
        assert_eq!(live, Some("agent.robinhood.com"));
    } else {
        assert_eq!(live, None);
    }
}

fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// DEC-947 item 5: a Robinhood run dials the build's endpoint and scopes every request to the
/// account number the founder typed, both read from the process environment the run was handed;
/// an endpoint refusal is the dial's, a missing or empty number refuses before anything is sent,
/// and the dial never prints the number.
#[test]
fn a_dial_is_the_builds_endpoint_and_the_founders_account_number() {
    const SIM: &str = "http://127.0.0.1:8080/mcp";
    let given = vars(&[
        ("PATH", "/bin"),
        (REHEARSAL_ENDPOINT_VAR, SIM),
        (ACCOUNT_NUMBER_VAR, "RHSIM00000001"),
    ]);
    let dialed = dial(Build::Rehearsal, &given).expect("the rehearsal dials the simulator");
    let expected = RobinhoodEndpoint {
        host: "127.0.0.1".to_owned(),
        url: SIM.to_owned(),
    };
    assert_eq!(dialed.endpoint, expected);
    assert_eq!(dialed.account_number(), "RHSIM00000001");
    assert_eq!(
        format!("{dialed:?}"),
        "Dial { .. }",
        "the number is never printed"
    );
    let number = vars(&[(ACCOUNT_NUMBER_VAR, "RHSIM00000001")]);
    assert_eq!(
        dial(Build::Rehearsal, &number).err(),
        Some(EndpointRefusal::RehearsalUnset)
    );
    assert_eq!(
        dial(Build::Default, &given).err(),
        Some(EndpointRefusal::NotBuilt)
    );
    assert_eq!(
        dial(Build::Live, &given).err(),
        Some(EndpointRefusal::RehearsalInLive)
    );
    for account in [vec![], vec![(ACCOUNT_NUMBER_VAR, "")]] {
        let mut pairs = vec![(REHEARSAL_ENDPOINT_VAR, SIM)];
        pairs.extend(account);
        assert_eq!(
            dial(Build::Rehearsal, &vars(&pairs)).err(),
            Some(EndpointRefusal::AccountUnset),
            "{pairs:?}"
        );
    }
}
