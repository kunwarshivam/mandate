//! Each agent's daily research cap, read by the service from the deployed mandate (DEC-2272): the
//! deployed version's `behavior.research.cost_cap_usd_per_day`, zero with no research envelope or
//! no deployment, each document that does not read refused with its code, and a version change read
//! again while an unchanged one is not.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_gateway::money::Usd6;
use mandate_gatewayd::cap::{CapError, DailyCaps, Deployment, agents, deployment, research_cap};
use mandate_journal::{ArtifactRef, Environment, StoredEvent};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const AGENT: &str = "agent-a";
const MANDATE: &str = include_str!("../../mandate-daemon/tests/fixtures/mandate.json");

fn usd(text: &str) -> Usd6 {
    Usd6::parse(text).unwrap()
}

fn monthly() -> Usd6 {
    usd("50")
}

/// The fixture mandate with `research` in place of its `null` research envelope and `environment`
/// as its environment, as canonical bytes.
fn document(research: &str, environment: &str) -> Vec<u8> {
    let text = MANDATE
        .trim_end()
        .replace(r#""research":null"#, &format!(r#""research":{research}"#))
        .replace(
            r#""environment":"paper""#,
            &format!(r#""environment":"{environment}""#),
        );
    mandate_canon::to_canonical(&mandate_canon::parse(text.as_bytes()).unwrap())
}

fn with_cap(cap: &str) -> Vec<u8> {
    document(&format!(r#"{{"cost_cap_usd_per_day":"{cap}"}}"#), "paper")
}

/// A control-stream row as the workspace services commit it.
fn row(seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"actor":{{"build":null,"id":"user-owner","kind":"user","version":"1"}},"artifact_refs":[],"causation_id":null,"clock_source":"system","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","event_time":"2026-10-16T13:00:00.000000000Z","event_type":"{event_type}","payload":{payload},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws1"}}"#
    );
    StoredEvent {
        stream_id: "ctl:ws1".into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: environment.into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// A control stream and the artifact store its versions name.
#[derive(Default)]
struct World {
    control: Vec<StoredEvent>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl World {
    fn next(&self) -> u64 {
        u64::try_from(self.control.len()).unwrap() + 1
    }

    /// Stores `bytes` and returns the version naming them.
    fn stored(&mut self, bytes: Vec<u8>) -> String {
        let reference = ArtifactRef::of(&bytes);
        self.store.insert(reference.digest(), bytes);
        reference.to_string()
    }

    fn deployed_in(&mut self, agent: &str, version: &str, environment: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
        );
        let seq = self.next();
        self.control
            .push(row(seq, "AgentDeployed", environment, &payload));
    }

    fn deployed(&mut self, version: &str) {
        self.deployed_in(AGENT, version, "paper");
    }

    fn stopped(&mut self, agent: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","connection_id":"conn_alpaca_paper_01","loss_added":"0","reason":"owner_stop","retired_on":"2026-10-16"}}"#
        );
        let seq = self.next();
        self.control
            .push(row(seq, "AgentStopped", "paper", &payload));
    }

    fn cap(&self) -> Result<Usd6, CapError> {
        let now = deployment(&self.control, AGENT, Environment::Paper)?;
        research_cap(&now, Environment::Paper, &self.store, monthly())
    }
}

/// DEC-2272 item 1: the cap is the deployed version's `cost_cap_usd_per_day`, which the internal
/// research profile, setting no ceiling of its own, leaves as it is.
#[test]
fn the_cap_is_the_deployed_version_s_own() {
    let mut world = World::default();
    let version = world.stored(with_cap("5.25"));
    world.deployed(&version);
    assert_eq!(world.cap(), Ok(usd("5.25")));
    assert_eq!(
        deployment(&world.control, AGENT, Environment::Paper),
        Ok(Deployment::Deployed(version))
    );
}

/// DEC-2272 item 3: no research envelope, no deployment, or a stop since gives a cap of zero.
#[test]
fn no_research_envelope_no_deployment_or_a_stop_gives_zero() {
    let mut world = World::default();
    assert_eq!(world.cap(), Ok(Usd6::ZERO), "never deployed");
    let none = world.stored(document("null", "paper"));
    world.deployed(&none);
    assert_eq!(world.cap(), Ok(Usd6::ZERO), "behavior.research is null");
    let capped = world.stored(with_cap("5"));
    world.deployed(&capped);
    assert_eq!(world.cap(), Ok(usd("5")));
    world.stopped(AGENT);
    assert_eq!(world.cap(), Ok(Usd6::ZERO), "stopped since");
}

/// DEC-2272 item 1: the agent's latest record in the service's environment names the version;
/// another agent's records and another environment's are not the agent's deployment.
#[test]
fn only_the_agent_s_latest_record_in_this_environment_counts() {
    let mut world = World::default();
    let mine = world.stored(with_cap("5"));
    let theirs = world.stored(with_cap("7"));
    world.deployed(&mine);
    world.deployed_in("agent-b", &theirs, "paper");
    world.deployed_in(AGENT, &theirs, "live");
    world.stopped("agent-b");
    assert_eq!(world.cap(), Ok(usd("5")));
}

/// Stores a case's document, or not, and gives the version an `AgentDeployed` names.
type Stores = Box<dyn Fn(&mut World) -> String>;

/// DEC-2272 item 4: each document that does not give a cap the gateway can hold is refused with
/// its code, never read as some other cap.
#[test]
fn each_document_that_does_not_read_is_refused_with_its_code() {
    let cases: [(&str, Stores, CapError); 8] = [
        (
            "the artifact is missing",
            Box::new(|_| ArtifactRef::of(b"never stored").to_string()),
            CapError::DocumentUnavailable,
        ),
        (
            "the artifact does not hash to its reference",
            Box::new(|world| {
                let reference = ArtifactRef::of(&with_cap("5"));
                world.store.insert(reference.digest(), with_cap("500"));
                reference.to_string()
            }),
            CapError::DocumentUnavailable,
        ),
        (
            "the artifact is not JSON",
            Box::new(|world| world.stored(b"not json".to_vec())),
            CapError::DocumentUnavailable,
        ),
        (
            "the document is another environment's",
            Box::new(|world| world.stored(document(r#"{"cost_cap_usd_per_day":"5"}"#, "live"))),
            CapError::EnvironmentMismatch,
        ),
        (
            "the research envelope names no cap",
            Box::new(|world| world.stored(document("{}", "paper"))),
            CapError::DocumentUnavailable,
        ),
        (
            "the cap is not an amount",
            Box::new(|world| world.stored(with_cap("five"))),
            CapError::DocumentUnavailable,
        ),
        (
            "the cap is above the workspace's monthly cap",
            Box::new(|world| world.stored(with_cap("50.000001"))),
            CapError::DocumentUnavailable,
        ),
        (
            "the cap is finer than Usd6 holds",
            Box::new(|world| world.stored(with_cap("1.0000001"))),
            CapError::DocumentUnavailable,
        ),
    ];
    for (case, version, error) in cases {
        let mut world = World::default();
        let version = version(&mut world);
        world.deployed(&version);
        assert_eq!(world.cap(), Err(error), "{case}");
    }
    let mut unreadable = World::default();
    unreadable
        .control
        .push(row(1, "AgentDeployed", "paper", r#"{"mandate_version":7}"#));
    assert_eq!(unreadable.cap(), Err(CapError::ControlUnreadable));
    let codes: Vec<_> = [
        CapError::ControlUnreadable,
        CapError::DocumentUnavailable,
        CapError::EnvironmentMismatch,
    ]
    .map(CapError::code)
    .into();
    assert_eq!(
        codes,
        [
            "control_unreadable",
            "mandate_document_unavailable",
            "mandate_environment_mismatch"
        ]
    );
}

/// DEC-2272 items 1 and 4, the coordinator's check: bytes stored under the journaled version's
/// digest that are not that version's document are refused, never read as the cap they hold, so a
/// store that swapped a document cannot raise the cap the core validated.
#[test]
fn a_document_whose_hash_differs_from_the_journaled_version_is_refused() {
    let mut world = World::default();
    let journaled = ArtifactRef::of(&with_cap("5"));
    world.store.insert(journaled.digest(), with_cap("49"));
    world.deployed(&journaled.to_string());
    let mut caps = DailyCaps::new(Environment::Paper, monthly());
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Err(CapError::DocumentUnavailable)
    );
    world.store.insert(journaled.digest(), with_cap("5"));
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("5")),
        "the journaled version's own document reads"
    );
}

/// DEC-2272 item 5: an unchanged deployment's cap is held without reading the document again, a
/// changed one is read again, and a read that fails leaves no cap, not the one held before.
#[test]
fn a_version_change_is_read_again_and_a_failed_read_leaves_no_cap() {
    let mut world = World::default();
    let mut caps = DailyCaps::new(Environment::Paper, monthly());
    let first = world.stored(with_cap("5"));
    world.deployed(&first);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("5"))
    );

    let held = world.store.clone();
    world.store.clear();
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("5")),
        "an unchanged deployment is not read again"
    );
    world.store = held;

    let lower = world.stored(with_cap("3"));
    world.deployed(&lower);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("3"))
    );

    let missing = ArtifactRef::of(&with_cap("9")).to_string();
    world.deployed(&missing);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Err(CapError::DocumentUnavailable),
        "never the cap held for the version before"
    );
    world.stored(with_cap("9"));
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("9")),
        "read again once it reads"
    );

    world.stopped(AGENT);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(Usd6::ZERO)
    );
}

/// DEC-2272 items 1 and 4, over any cap: one at or under the workspace's monthly cap is read as
/// itself, one above it is refused.
#[test]
fn any_cap_is_read_as_itself_or_refused_above_the_monthly_cap() {
    let mut runner = TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(&(0_u64..=100_000_000), |micros| {
            let cap = Usd6::from_micros(micros);
            let mut world = World::default();
            let version = world.stored(with_cap(&cap.to_string()));
            world.deployed(&version);
            let expected = if micros <= 50_000_000 {
                Ok(cap)
            } else {
                Err(CapError::DocumentUnavailable)
            };
            prop_assert_eq!(world.cap(), expected);
            Ok(())
        })
        .unwrap();
}

/// DEC-2272 item 4: the agents the start reads caps for are those an `AgentDeployed` in this
/// environment names, stopped since or not; another environment's deployments and the stream's
/// other records name none.
#[test]
fn the_agents_are_those_deployed_in_this_environment() {
    let mut world = World::default();
    let version = world.stored(with_cap("5"));
    world.deployed(&version);
    world.deployed_in("agent-b", &version, "paper");
    world.stopped("agent-b");
    world.deployed_in("agent-live", &version, "live");
    let seq = world.next();
    world.control.push(row(
        seq,
        "MandateVersionCreated",
        "paper",
        r#"{"mandate_version":"v"}"#,
    ));
    assert_eq!(
        agents(&world.control, Environment::Paper),
        Ok(BTreeSet::from([AGENT.to_owned(), "agent-b".to_owned()]))
    );
    world.control.push(row(
        world.next(),
        "AgentDeployed",
        "paper",
        r#"{"mandate_version":"v"}"#,
    ));
    assert_eq!(
        agents(&world.control, Environment::Paper),
        Err(CapError::ControlUnreadable)
    );
}

/// DEC-2272 item 3, the coordinator's bound: an agent never deployed, or stopped since, is read as
/// a cap of zero and held no longer, so the caps held count deployed agents alone.
#[test]
fn a_never_deployed_or_stopped_agent_is_not_held() {
    let mut world = World::default();
    let mut caps = DailyCaps::new(Environment::Paper, monthly());
    for n in 0..20 {
        assert_eq!(
            caps.current(&world.control, &format!("ghost_{n}"), &world.store),
            Ok(Usd6::ZERO)
        );
    }
    assert_eq!(caps.held(), 0, "never deployed");
    let version = world.stored(with_cap("5"));
    world.deployed(&version);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(usd("5"))
    );
    assert_eq!(caps.held(), 1);
    world.stopped(AGENT);
    assert_eq!(
        caps.current(&world.control, AGENT, &world.store),
        Ok(Usd6::ZERO)
    );
    assert_eq!(caps.held(), 0, "stopped since");
}
