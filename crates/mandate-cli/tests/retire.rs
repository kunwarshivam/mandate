//! E21-33 slice 5b (journal spec v0.66 §9.1, §9.21, rule 246; DEC-2039): the owner's Stop of an
//! agent no process ran. With the right code, `agent stop` claims the agent's stream, appending its
//! `StreamOpened` at head 0 under the epoch the head holds, and then writes the `AgentStopped`
//! itself, with reason `owner_stop` and `loss_added` 0. A stream its own claim opened is resumed. A
//! stream a runtime opened, or one a runtime takes first, sends the Stop as an `OwnerCommandIssued`,
//! as before: a Stop is never refused for this (`AGENTS.md` rule 13). The oracle is each case's own
//! record of what it seeded and what the journal holds, never the CLI's fold.

mod common;

use std::collections::BTreeMap;

use common::{
    AGENT, CONTROL, FixedIds, Journal, OTHER_AGENT, OWNER, Refusal, WORKSPACE, agent_stream, at,
    owner, stream,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::agent::{self, Command};
use mandate_cli::control::{ControlError, ControlJournal, Submitted};
use mandate_cli::retire;
use mandate_journal::{AppendOutcome, ArtifactStore};
use proptest::prelude::*;

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const CONNECTION: &str = "conn_alpaca_paper_01";
const SERVICES: &str =
    r#"{"build":null,"id":"control_services","kind":"system","version":"0.1.0"}"#;
const RUNTIME: &str = r#"{"build":"sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","id":"agent_a","kind":"agent","version":"0.1.0"}"#;
const CLAIMANT: &str = r#"{"build":null,"id":"user-owner","kind":"user","version":"1"}"#;
const SEEDED: &str = r#"{"actor":@A,"artifact_refs":[],"causation_id":@C,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":@V,"stream_id":"@S"}"#;
const ESTABLISHED: &str = r#"{"account_ref":"01J0ACC0VNT000000000000000","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{"assertion_id":"cli-assertion-0","authenticated_at":"2026-10-08T12:59:00.000000000Z","method":"cli_confirm"},"user":"user-owner"}"#;
const RECORD: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// A workspace whose streams the case seeds record by record, and its store.
#[derive(Default)]
struct World {
    journal: Journal,
    store: Store,
    ids: FixedIds,
    seeded: u64,
}

impl World {
    fn new() -> Self {
        let mut world = Self::default();
        world
            .store
            .put_artifact(&to_canonical(&parse(MANDATE.as_bytes()).unwrap()))
            .unwrap();
        world
    }

    fn version() -> String {
        format!(
            "sha256:{}",
            Digest::of(&to_canonical(&parse(MANDATE.as_bytes()).unwrap())).to_hex()
        )
    }

    /// Another writer's draft of `event_type` on `stream_id`, as `actor`.
    fn drafted(
        &mut self,
        stream_id: &str,
        actor: &str,
        event_type: &str,
        payload: &str,
        version: u64,
    ) -> Vec<u8> {
        self.seeded += 1;
        let causation = if version == 2 {
            r#""8000000000000000000000000K""#
        } else {
            "null"
        };
        SEEDED
            .replace("@A", actor)
            .replace("@I", &format!("9{:025}", self.seeded))
            .replace("@T", event_type)
            .replace("@P", payload)
            .replace("@V", &version.to_string())
            .replace("@S", stream_id)
            .replace("@C", causation)
            .into_bytes()
    }

    fn seed_as(
        &mut self,
        stream_id: &str,
        actor: &str,
        event_type: &str,
        payload: &str,
        version: u64,
    ) {
        let draft = self.drafted(stream_id, actor, event_type, payload, version);
        let target = stream(stream_id);
        let head = self.journal.head(&target).unwrap().seq;
        let epoch = self.journal.take_ownership(&target).unwrap();
        let outcome = self
            .journal
            .append(&target, head, epoch, at(NOW).at, &[draft.as_slice()]);
        assert!(
            matches!(outcome, Ok(AppendOutcome::Committed(_))),
            "{outcome:?}"
        );
    }

    fn established(&mut self) {
        self.seed_as(CONTROL, SERVICES, "ConnectionEstablished", ESTABLISHED, 2);
    }

    fn deployed(&mut self, agent: &str, v2: bool) {
        let version = Self::version();
        let payload = if v2 {
            format!(
                r#"{{"agent_id":"{agent}","connection_id":"{CONNECTION}","mandate_version":"{version}","record_ref":"{RECORD}"}}"#
            )
        } else {
            format!(
                r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{RECORD}"}}"#
            )
        };
        self.seed_as(
            CONTROL,
            SERVICES,
            "AgentDeployed",
            &payload,
            if v2 { 2 } else { 1 },
        );
    }

    fn started(&mut self, agent: &str, generation: u64) {
        let cause = if generation == 1 {
            "deployed"
        } else {
            "restart"
        };
        let payload = format!(
            r#"{{"cause":"{cause}","generation":{generation},"process":"runtime","subject":"{agent}"}}"#
        );
        self.seed_as(CONTROL, SERVICES, "ProcessStartIssued", &payload, 1);
    }

    /// The agent stream's `StreamOpened` (journal spec §9.1), as `actor` writes it.
    fn opening(&mut self, agent: &str, actor: &str) -> Vec<u8> {
        let payload = format!(
            r#"{{"agent_id":"{agent}","stream_type":"agent","workspace_id":"{WORKSPACE}"}}"#
        );
        self.drafted(&agent_stream(agent), actor, "StreamOpened", &payload, 1)
    }

    /// `agent`'s stream opened by `actor`: its runtime, or an earlier Stop's claim.
    fn opened(&mut self, agent: &str, actor: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","stream_type":"agent","workspace_id":"{WORKSPACE}"}}"#
        );
        self.seed_as(&agent_stream(agent), actor, "StreamOpened", &payload, 1);
    }

    /// The code `agent stop` shows now.
    fn code(&self, agent: &str) -> String {
        agent::code(
            &self.journal,
            &owner(),
            agent,
            &Command::Stop { release: false },
        )
        .unwrap()
        .unwrap()
    }

    fn stop(&mut self, agent: &str, typed: Option<&str>) -> Result<Submitted, ControlError> {
        retire::stop(
            &mut self.journal,
            &self.store,
            &mut self.ids,
            &owner(),
            agent,
            false,
            typed,
            at(NOW),
        )
    }

    fn stop_with_code(&mut self, agent: &str) -> Result<Submitted, ControlError> {
        let code = self.code(agent);
        self.stop(agent, Some(&code))
    }

    fn bodies(&self, stream_id: &str) -> Vec<(String, Value)> {
        self.journal
            .rows(&stream(stream_id))
            .unwrap()
            .iter()
            .map(|row| (row.event_type.clone(), parse(&row.body).unwrap()))
            .collect()
    }

    fn rows(&self) -> Vec<(String, Value)> {
        self.bodies(CONTROL)
    }

    fn of(&self, event_type: &str) -> Vec<Value> {
        self.rows()
            .into_iter()
            .filter(|(t, _)| t == event_type)
            .map(|(_, body)| body)
            .collect()
    }

    /// The actor kinds of `agent`'s stream, record by record.
    fn agent_kinds(&self, agent: &str) -> Vec<String> {
        self.bodies(&agent_stream(agent))
            .iter()
            .map(|(_, body)| text(body, &["actor", "kind"]).unwrap().to_owned())
            .collect()
    }

    fn epoch(&self, agent: &str) -> u64 {
        self.journal
            .head(&stream(&agent_stream(agent)))
            .unwrap()
            .writer_epoch
    }
}

fn member<'a>(body: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(body, |value, key| value.get(key))
}

fn text<'a>(body: &'a Value, path: &[&str]) -> Option<&'a str> {
    member(body, path).and_then(Value::as_str)
}

/// The one `AgentStopped` the stop wrote, read back: `owner_stop`, no loss, the deployment's
/// connection, the America/New_York date of the stop, the owner as its actor, the version as its
/// configuration, and no `OwnerCommandIssued` beside it.
fn assert_retired(world: &World, agent: &str) {
    let stopped = world.of("AgentStopped");
    assert_eq!(stopped.len(), 1, "one AgentStopped");
    let body = &stopped[0];
    assert_eq!(text(body, &["payload", "agent_id"]), Some(agent));
    assert_eq!(text(body, &["payload", "connection_id"]), Some(CONNECTION));
    assert_eq!(text(body, &["payload", "reason"]), Some("owner_stop"));
    assert_eq!(text(body, &["payload", "loss_added"]), Some("0"));
    assert_eq!(text(body, &["payload", "retired_on"]), Some("2026-09-21"));
    assert_eq!(text(body, &["actor", "kind"]), Some("user"));
    assert_eq!(text(body, &["actor", "id"]), Some(OWNER));
    assert_eq!(
        text(body, &["config_refs", "mandate_version"]),
        Some(World::version().as_str())
    );
    assert!(
        !world
            .of("OwnerCommandIssued")
            .iter()
            .any(|c| text(c, &["payload", "command"]) == Some("stop")),
        "no Stop command beside it"
    );
    assert_eq!(
        world.agent_kinds(agent).first().map(String::as_str),
        Some("user"),
        "the agent's stream opens with the owner's claim"
    );
}

/// The Stop's own claim on `agent`'s stream: one `StreamOpened`, as the owner, naming the agent's
/// stream, and nothing after it.
fn assert_claimed(world: &World, agent: &str) {
    let bodies = world.bodies(&agent_stream(agent));
    assert_eq!(bodies.len(), 1, "the claim alone: {bodies:?}");
    let (event_type, body) = &bodies[0];
    assert_eq!(event_type, "StreamOpened");
    assert_eq!(text(body, &["actor", "kind"]), Some("user"));
    assert_eq!(text(body, &["actor", "id"]), Some(OWNER));
    assert_eq!(text(body, &["payload", "agent_id"]), Some(agent));
    assert_eq!(text(body, &["payload", "stream_type"]), Some("agent"));
    assert_eq!(text(body, &["payload", "workspace_id"]), Some(WORKSPACE));
    assert_eq!(
        text(body, &["stream_id"]),
        Some(agent_stream(agent).as_str())
    );
}

/// The Stop went the way it always has: one `OwnerCommandIssued` with `stop`, and no
/// `AgentStopped`.
fn assert_commanded(world: &World, agent: &str) {
    assert!(world.of("AgentStopped").is_empty(), "no AgentStopped");
    let commands = world.of("OwnerCommandIssued");
    assert_eq!(commands.len(), 1, "one command");
    assert_eq!(text(&commands[0], &["payload", "command"]), Some("stop"));
    assert_eq!(text(&commands[0], &["payload", "agent"]), Some(agent));
}

#[test]
fn a_version_2_deployment_never_run_is_claimed_and_retired_with_no_loss() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    world.stop_with_code(AGENT).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
}

#[test]
fn an_unread_version_1_deployment_is_retired_on_its_documents_connection() {
    let mut world = World::new();
    world.deployed(AGENT, false);
    world.stop_with_code(AGENT).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
}

#[test]
fn a_version_1_deployment_whose_document_cannot_be_read_is_stopped_by_command_unclaimed() {
    let mut world = World::new();
    world.deployed(AGENT, false);
    world.store.clear();
    world.stop_with_code(AGENT).unwrap();
    assert_commanded(&world, AGENT);
    assert!(world.agent_kinds(AGENT).is_empty(), "nothing claimed");
}

#[test]
fn a_deployment_rule_201_refuses_is_retired() {
    let mut world = World::new();
    world.deployed(AGENT, true);
    world.stop_with_code(AGENT).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
}

#[test]
fn a_runtime_started_that_has_not_opened_its_stream_is_claimed_and_retired() {
    for generation in [1, 2] {
        let mut world = World::new();
        world.established();
        world.deployed(AGENT, true);
        for g in 1..=generation {
            world.started(AGENT, g);
        }
        world.stop_with_code(AGENT).unwrap();
        assert_claimed(&world, AGENT);
        assert_retired(&world, AGENT);
    }
}

#[test]
fn an_agent_whose_runtime_opened_its_stream_is_stopped_by_command_as_before() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    world.opened(AGENT, RUNTIME);
    world.stop_with_code(AGENT).unwrap();
    assert_commanded(&world, AGENT);
    assert_eq!(world.agent_kinds(AGENT), ["agent"], "its stream untouched");
}

#[test]
fn a_runtime_opening_that_lands_before_the_claim_sends_the_stop_by_command() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let code = world.code(AGENT);
    let opening = world.opening(AGENT, RUNTIME);
    world.journal.land_before_next.push(opening);
    world.stop(AGENT, Some(&code)).unwrap();
    assert_commanded(&world, AGENT);
    assert_eq!(
        world.agent_kinds(AGENT),
        ["agent"],
        "the runtime's opening alone"
    );
}

#[test]
fn a_runtime_that_takes_the_stream_before_the_claim_sends_the_stop_by_command() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let code = world.code(AGENT);
    world.journal.fence_next = 1;
    world.stop(AGENT, Some(&code)).unwrap();
    assert_commanded(&world, AGENT);
    assert!(world.agent_kinds(AGENT).is_empty(), "nothing claimed");
}

#[test]
fn the_claim_takes_no_epoch_so_it_fences_no_runtime() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let held = world
        .journal
        .take_ownership(&stream(&agent_stream(AGENT)))
        .unwrap();
    world.stop_with_code(AGENT).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
    assert_eq!(
        world.epoch(AGENT),
        held,
        "the runtime's epoch still holds the stream"
    );
}

#[test]
fn a_claim_an_earlier_stop_left_unfinished_is_resumed() {
    for claimant in [CLAIMANT, SERVICES] {
        let mut world = World::new();
        world.established();
        world.deployed(AGENT, true);
        world.opened(AGENT, claimant);
        world.stop_with_code(AGENT).unwrap();
        assert_eq!(world.agent_kinds(AGENT).len(), 1, "no second claim");
        let stopped = world.of("AgentStopped");
        assert_eq!(stopped.len(), 1, "one AgentStopped");
        assert_eq!(text(&stopped[0], &["payload", "loss_added"]), Some("0"));
        assert!(world.of("OwnerCommandIssued").is_empty());
    }
}

#[test]
fn a_control_record_landing_before_the_retirement_is_decided_again_and_the_claim_stands() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    world.deployed(OTHER_AGENT, true);
    let code = world.code(AGENT);
    let pause = format!(r#"{{"command":"pause","subject":"{OTHER_AGENT}"}}"#);
    let landed = world.drafted(CONTROL, SERVICES, "OwnerCommandIssued", &pause, 1);
    world.journal.land_before_next.push(landed);
    world.stop(AGENT, Some(&code)).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
}

#[test]
fn only_the_agents_own_stream_counts() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    world.deployed(OTHER_AGENT, true);
    world.started(OTHER_AGENT, 1);
    world.opened(OTHER_AGENT, RUNTIME);
    world.stop_with_code(AGENT).unwrap();
    assert_claimed(&world, AGENT);
    assert_retired(&world, AGENT);
    assert_eq!(world.agent_kinds(OTHER_AGENT), ["agent"]);
}

#[test]
fn a_wrong_or_missing_code_is_refused_and_nothing_is_written() {
    for typed in [None, Some("00000000")] {
        let mut world = World::new();
        world.established();
        world.deployed(AGENT, true);
        let before = world.rows().len();
        assert!(matches!(
            world.stop(AGENT, typed),
            Err(ControlError::Refused {
                reason: "step_up_missing"
            })
        ));
        assert_eq!(world.rows().len(), before, "nothing written");
        assert!(world.agent_kinds(AGENT).is_empty(), "nothing claimed");
    }
}

#[test]
fn a_stop_of_an_agent_already_retired_writes_nothing_more() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let first = world.stop_with_code(AGENT).unwrap();
    let again = world.stop_with_code(AGENT).unwrap();
    assert_eq!(again, first, "the retirement is reported, not repeated");
    assert_eq!(world.of("AgentStopped").len(), 1);
    assert!(world.of("OwnerCommandIssued").is_empty());
    assert_claimed(&world, AGENT);
}

#[test]
fn an_agent_never_deployed_is_stopped_by_command_unclaimed() {
    let mut world = World::new();
    world.established();
    world.stop_with_code(AGENT).unwrap();
    assert_commanded(&world, AGENT);
    assert!(world.agent_kinds(AGENT).is_empty(), "nothing claimed");
}

/// DEC-2039 item 6 (a refused claim is Rejected; `AGENTS.md` rule 13): whatever the journal answers
/// to the owner's claim, a conflict, a refusal of the draft, or an error, the Stop is never
/// refused. It goes as an `OwnerCommandIssued`, and nothing is claimed.
#[test]
fn a_claim_the_journal_refuses_or_fails_still_sends_the_stop_by_command() {
    for refusal in [Refusal::Conflict, Refusal::Invalid, Refusal::Fails] {
        let mut world = World::new();
        world.established();
        world.deployed(AGENT, true);
        world.journal.agent_refusal = Some(refusal);
        let outcome = world.stop_with_code(AGENT);
        assert!(
            outcome.is_ok(),
            "{refusal:?}: the Stop was refused: {outcome:?}"
        );
        assert_commanded(&world, AGENT);
        assert!(
            world.agent_kinds(AGENT).is_empty(),
            "{refusal:?}: nothing claimed"
        );
    }
}

/// The owner's `stop` commands for `agent` on the control stream.
fn stop_commands(world: &World, agent: &str) -> usize {
    world
        .of("OwnerCommandIssued")
        .iter()
        .filter(|c| {
            text(c, &["payload", "command"]) == Some("stop")
                && text(c, &["payload", "agent"]) == Some(agent)
        })
        .count()
}

/// The `AgentStopped` records that name `agent`.
fn stops_of(world: &World, agent: &str) -> usize {
    world
        .of("AgentStopped")
        .iter()
        .filter(|b| text(b, &["payload", "agent_id"]) == Some(agent))
        .count()
}

/// The `AgentStopped` may be committed when its append reports nothing settled: the Stop finds it
/// and returns it, and sends no command beside it, so one Stop has one effect.
#[test]
fn a_retirement_whose_append_may_have_committed_is_found_and_no_command_is_sent() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let code = world.code(AGENT);
    world.journal.lost_for_good = true;
    let outcome = world.stop(AGENT, Some(&code));
    world.journal.recover();
    assert!(outcome.is_ok(), "the Stop was refused: {outcome:?}");
    assert_eq!(stops_of(&world, AGENT), 1, "the AgentStopped committed");
    assert_eq!(stop_commands(&world, AGENT), 0, "no command beside it");
}

/// Every round of the Stop finds the control stream moved, and the last round's `AgentStopped` did
/// commit although its answer reports a moved head: the Stop finds it after the last round and
/// returns it, and sends no command beside it. An earlier Stop's claim stands, so every append
/// the Stop makes is on the control stream.
#[test]
fn a_retirement_committed_in_the_last_round_is_found_and_no_command_is_sent() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    world.opened(AGENT, CLAIMANT);
    let code = world.code(AGENT);
    world.journal.behind_next = 2;
    world.journal.mismatch_after_commit = 1;
    let outcome = world.stop(AGENT, Some(&code));
    assert!(outcome.is_ok(), "the Stop was refused: {outcome:?}");
    assert_eq!(stops_of(&world, AGENT), 1, "the AgentStopped committed");
    assert_eq!(stop_commands(&world, AGENT), 0, "no command beside it");
}

/// The claim is stored and the `AgentStopped`'s append then fails: the Stop goes by command, the
/// stream stays claimed (its first record the owner's, so a runtime started on it refuses it,
/// journal spec §9.1), and a later Stop resumes the claim and completes the retirement.
#[test]
fn a_retirement_that_fails_after_its_claim_goes_by_command_and_a_later_stop_completes_it() {
    let mut world = World::new();
    world.established();
    world.deployed(AGENT, true);
    let code = world.code(AGENT);
    world.journal.control_failures = 1;
    let outcome = world.stop(AGENT, Some(&code));
    assert!(outcome.is_ok(), "the Stop was refused: {outcome:?}");
    assert_eq!(stop_commands(&world, AGENT), 1, "the Stop went by command");
    assert_eq!(stops_of(&world, AGENT), 0);
    assert_claimed(&world, AGENT);
    world.stop_with_code(AGENT).unwrap();
    assert_eq!(stops_of(&world, AGENT), 1, "the later Stop retired it");
    assert_claimed(&world, AGENT);
}

/// One step of the property: what a runtime or the manager does, or the owner's Stop.
#[derive(Debug, Clone)]
enum Op {
    Deploy(usize),
    /// The manager's journaled start, which runs nothing until the runtime opens its stream.
    Start(usize),
    /// A runtime, managed or not, opening its stream; on a claimed stream it refuses and writes
    /// nothing (journal spec §9.1).
    Open(usize),
    /// A runtime's opening that lands between the Stop's read and its claim.
    StopRaced(usize),
    /// A runtime taking the stream's epoch between the Stop's read and its claim.
    StopFenced(usize),
    /// A Stop whose claim the journal answers with a conflict, a refusal, or an error.
    StopRefused(usize, Refusal),
    /// A Stop whose first control-stream append commits but whose answer is lost, and every later
    /// one is unavailable.
    StopLost(usize),
    /// A Stop whose first control-stream append fails, storing nothing.
    StopFailing(usize),
    Stop(usize),
}

const AGENTS: [&str; 2] = [AGENT, OTHER_AGENT];

fn op() -> impl Strategy<Value = Op> {
    let agent = 0..AGENTS.len();
    prop_oneof![
        2 => agent.clone().prop_map(Op::Deploy),
        1 => agent.clone().prop_map(Op::Start),
        2 => agent.clone().prop_map(Op::Open),
        2 => agent.clone().prop_map(Op::StopRaced),
        1 => agent.clone().prop_map(Op::StopFenced),
        1 => (
            agent.clone(),
            prop_oneof![Just(Refusal::Conflict), Just(Refusal::Invalid), Just(Refusal::Fails)]
        )
            .prop_map(|(i, refusal)| Op::StopRefused(i, refusal)),
        1 => agent.clone().prop_map(Op::StopLost),
        1 => agent.clone().prop_map(Op::StopFailing),
        3 => agent.prop_map(Op::Stop),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// DEC-2039 items 2 and 7: over random interleavings of deployments, starts, runtime openings
    /// (journaled first, or landing between a Stop's read and its claim), runtimes taking the
    /// stream and Stops, no `AgentStopped` with `loss_added` 0 is ever written for an agent whose
    /// runtime opened its stream, the Stop never writes to a stream a runtime opened, and every
    /// agent no runtime opened, once deployed and stopped, is retired. The oracle is the
    /// operations' own record of which runtime opened its stream.
    #[test]
    fn no_loss_free_stop_is_written_for_an_agent_that_ran(ops in prop::collection::vec(op(), 1..24)) {
        let mut world = World::new();
        world.established();
        let mut deployed = [false; 2];
        let mut ran = [false; 2];
        let mut claimed = [false; 2];
        let mut generation = [0_u64; 2];
        for op in &ops {
            match op {
                Op::Deploy(i) if !deployed[*i] => {
                    world.deployed(AGENTS[*i], true);
                    deployed[*i] = true;
                }
                Op::Deploy(_) => {}
                Op::Start(i) => {
                    generation[*i] += 1;
                    world.started(AGENTS[*i], generation[*i]);
                }
                Op::Open(i) => {
                    if !ran[*i] && !claimed[*i] {
                        world.opened(AGENTS[*i], RUNTIME);
                        ran[*i] = true;
                    }
                }
                Op::StopRaced(i)
                | Op::StopFenced(i)
                | Op::StopRefused(i, _)
                | Op::StopLost(i)
                | Op::StopFailing(i)
                | Op::Stop(i) => {
                    let before = world.of("AgentStopped").len();
                    let records = world.agent_kinds(AGENTS[*i]).len();
                    let code = world.code(AGENTS[*i]);
                    let raced = matches!(op, Op::StopRaced(_)) && !ran[*i] && !claimed[*i];
                    if raced {
                        let opening = world.opening(AGENTS[*i], RUNTIME);
                        world.journal.land_before_next.push(opening);
                    }
                    if matches!(op, Op::StopFenced(_)) {
                        world.journal.fence_next = 1;
                    }
                    if let Op::StopRefused(_, refusal) = op {
                        world.journal.agent_refusal = Some(*refusal);
                    }
                    let faulty = matches!(op, Op::StopLost(_) | Op::StopFailing(_));
                    world.journal.lost_for_good = matches!(op, Op::StopLost(_));
                    if matches!(op, Op::StopFailing(_)) {
                        world.journal.control_failures = 1;
                    }
                    let commands = world.of("OwnerCommandIssued").len();
                    let effects = stops_of(&world, AGENTS[*i]) + stop_commands(&world, AGENTS[*i]);
                    let retired_before = stops_of(&world, AGENTS[*i]) > 0;
                    let outcome = world.stop(AGENTS[*i], Some(&code));
                    world.journal.recover();
                    world.journal.control_failures = 0;
                    let refused = matches!(op, Op::StopRefused(..))
                        && world.journal.agent_refusal.take().is_none();
                    prop_assert!(
                        outcome.is_ok() || faulty,
                        "{:?}: the Stop was refused: {:?}", op, outcome
                    );
                    let added = stops_of(&world, AGENTS[*i]) + stop_commands(&world, AGENTS[*i]) - effects;
                    prop_assert!(added <= 1, "{:?}: the Stop left {} effects", op, added);
                    prop_assert!(
                        outcome.is_err() || added == 1 || retired_before,
                        "{:?}: a Stop that succeeded left no effect", op
                    );
                    if refused {
                        prop_assert!(
                            world.of("OwnerCommandIssued").len() > commands,
                            "{:?}: a refused claim sent no command", op
                        );
                    }
                    let landed = raced && world.journal.land_before_next.is_empty();
                    world.journal.land_before_next.clear();
                    world.journal.fence_next = 0;
                    if landed {
                        ran[*i] = true;
                        prop_assert_eq!(world.agent_kinds(AGENTS[*i]).len(), 1, "{:?}: only the runtime's opening", op);
                    }
                    let kinds = world.agent_kinds(AGENTS[*i]);
                    if ran[*i] {
                        prop_assert_eq!(
                            kinds.len(), records + usize::from(landed),
                            "{:?}: the Stop wrote to a stream a runtime opened", op
                        );
                    } else if kinds.first().map(String::as_str) == Some("user") {
                        claimed[*i] = true;
                        prop_assert_eq!(kinds.len(), 1, "{:?}: a claim and nothing after it", op);
                    }
                    let stopped = world.of("AgentStopped");
                    let wrote = stopped.len() > before;
                    prop_assert!(
                        !(wrote && ran[*i]),
                        "{:?}: a loss-free AgentStopped for an agent that ran: {:?}", op, outcome
                    );
                    if deployed[*i] && !ran[*i] && matches!(op, Op::Stop(_)) {
                        let retired = stopped
                            .iter()
                            .any(|b| text(b, &["payload", "agent_id"]) == Some(AGENTS[*i]));
                        prop_assert!(retired, "{:?}: a never-run agent was not retired", op);
                    }
                }
            }
        }
    }
}
