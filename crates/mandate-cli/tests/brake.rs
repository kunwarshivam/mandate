//! E21-34 (OPS-4; DEC-2000): `mandate agent pause` and `mandate agent kill` at agent, connection
//! and workspace scope, needing nothing but the journal. A kill switch is one `OwnerCommandIssued`
//! of its scope with no step-up evidence, never refused; an agent-scoped one names the agent and
//! no wider subject, so the executor's agent scope, which never uses cancel-all or close-position,
//! is the one that applies it (trading spec §5.5; `mandate-executor`'s reference cases check that
//! side). A pause of a connection or a workspace is one agent-scoped pause for each agent deployed
//! there, and an agent whose mandate cannot be read is paused rather than skipped. The control
//! stream is seeded here, record by record; the property's oracle walks the seeded events itself.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use clap::Parser;
use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, OWNER, at, body, member, member_text,
    owner,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cli::agent::Scope;
use mandate_cli::brake::{
    self, AgentCommand, Paused, ReachArgs, killed_line, pause, paused_lines, pauses_settled,
    reached,
};
use mandate_cli::control::{ControlError, Submitted};
use mandate_cli::gestures;
use mandate_cli::{Cli, Command};
use proptest::prelude::*;

const MANDATE: &str = include_str!("fixtures/spy_mandate.json");
const CONN: &str = "conn_alpaca_paper_01";
const OTHER_CONN: &str = "conn_alpaca_paper_02";
const THIRD_AGENT: &str = "agent-c";
const FOURTH_AGENT: &str = "agent-d";
const AGENTS: [&str; 4] = [AGENT, OTHER_AGENT, THIRD_AGENT, FOURTH_AGENT];

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

/// Stores the fixture's mandate on `connection` and returns its version, `sha256:` and the hex.
fn stored_on(fx: &mut Fixture, connection: &str) -> String {
    let document = MANDATE.replace(CONN, connection);
    let bytes = to_canonical(&parse(document.as_bytes()).unwrap());
    let digest = Digest::of(&bytes);
    fx.store.insert(digest, bytes);
    format!("sha256:{}", digest.to_hex())
}

/// A version the store does not hold.
fn missing_version() -> String {
    format!("sha256:{}", Digest::of(b"no such mandate").to_hex())
}

fn deployed(fx: &mut Fixture, agent: &str, version: &str) {
    let payload = common::object(&[
        ("agent_id", common::text(agent)),
        ("mandate_version", common::text(version)),
        (
            "record_ref",
            common::text(&format!("sha256:{}", Digest::of(agent.as_bytes()).to_hex())),
        ),
    ]);
    fx.append_in("paper", CONTROL, "AgentDeployed", None, payload);
}

fn stopped(fx: &mut Fixture, agent: &str) {
    let payload = common::object(&[
        ("agent_id", common::text(agent)),
        ("connection_id", common::text(CONN)),
        ("loss_added", common::text("0")),
        ("reason", common::text("owner_stop")),
        ("retired_on", common::text("2026-09-20")),
    ]);
    fx.append_in("paper", CONTROL, "AgentStopped", None, payload);
}

/// The control stream's events but the owner alerts a kill switch commits beside its command
/// (E8-9k), which `a_kill_switch_commits_its_owner_alert_in_its_batch` checks.
fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL)
        .iter()
        .filter(|row| row.event_type != "OwnerAlertSent")
        .map(body)
        .collect()
}

/// The owner's commands committed after the first `before` control-stream events.
fn commands_after(fx: &Fixture, before: usize) -> Vec<Value> {
    let events = control(fx);
    for event in &events[before..] {
        assert_eq!(
            member_text(event, "event_type"),
            Some("OwnerCommandIssued"),
            "a brake commits only owner commands"
        );
        assert_eq!(member_text(event, "actor.kind"), Some("user"));
        assert_eq!(member_text(event, "actor.id"), Some(OWNER));
    }
    events[before..].to_vec()
}

fn paused_agents(fx: &Fixture, before: usize) -> BTreeSet<String> {
    let mut agents = BTreeSet::new();
    for event in commands_after(fx, before) {
        assert_eq!(member_text(&event, "payload.command"), Some("pause"));
        assert_eq!(member_text(&event, "payload.scope"), Some("agent"));
        let agent = member_text(&event, "payload.agent").unwrap().to_owned();
        assert_eq!(member_text(&event, "payload.subject"), Some(agent.as_str()));
        assert_eq!(member(&event, "payload.step_up"), Some(&Value::Null));
        assert!(agents.insert(agent), "one pause per agent");
    }
    agents
}

fn set(agents: &[&str]) -> BTreeSet<String> {
    agents.iter().map(|a| (*a).to_owned()).collect()
}

/// agent-a and agent-d on `CONN` (agent-d deployed twice, the latest counting), agent-b on
/// `OTHER_CONN`, agent-c on `CONN` but stopped, and an agent whose mandate is not in the store.
fn seeded() -> Fixture {
    let mut fx = Fixture::new();
    let on_conn = stored_on(&mut fx, CONN);
    let on_other = stored_on(&mut fx, OTHER_CONN);
    deployed(&mut fx, AGENT, &on_conn);
    deployed(&mut fx, OTHER_AGENT, &on_other);
    deployed(&mut fx, THIRD_AGENT, &on_conn);
    stopped(&mut fx, THIRD_AGENT);
    deployed(&mut fx, FOURTH_AGENT, &on_other);
    stopped(&mut fx, FOURTH_AGENT);
    deployed(&mut fx, FOURTH_AGENT, &on_conn);
    deployed(&mut fx, "agent-unread", &missing_version());
    let corrupt = br#"{"connection_id":"conn_alpaca_paper_02"}"#.to_vec();
    let digest = Digest::of(&corrupt);
    fx.store.insert(digest, corrupt);
    deployed(
        &mut fx,
        "agent-corrupt",
        &format!("sha256:{}", digest.to_hex()),
    );
    fx
}

fn run_pause(fx: &mut Fixture, scope: &Scope) -> Vec<Paused> {
    let mut ids = FixedIds::default();
    let store = fx.store.clone();
    answer(
        "pause",
        pause(
            &mut fx.journal,
            &store,
            &mut ids,
            &owner(),
            scope,
            at(ASKED_AT),
        ),
    )
}

#[test]
fn the_flags_name_exactly_one_scope() {
    let reach = |agent: Option<&str>, connection: Option<&str>, workspace_wide| ReachArgs {
        agent: agent.map(str::to_owned),
        connection: connection.map(str::to_owned),
        workspace_wide,
    };
    assert_eq!(
        reach(Some(AGENT), None, false).scope(),
        Scope::Agent(AGENT.to_owned())
    );
    assert_eq!(
        reach(None, Some(CONN), false).scope(),
        Scope::Connection(CONN.to_owned())
    );
    assert_eq!(reach(None, None, true).scope(), Scope::Workspace);
}

#[test]
fn an_agent_pause_reaches_that_agent_alone_deployed_or_not() {
    let fx = seeded();
    for agent in [AGENT, THIRD_AGENT, "agent-never-deployed"] {
        assert_eq!(
            answer(
                "reached",
                reached(
                    &fx.journal,
                    &fx.store,
                    &owner(),
                    &Scope::Agent(agent.to_owned())
                )
            ),
            set(&[agent])
        );
    }
}

#[test]
fn a_connection_pause_reaches_its_active_agents_and_any_whose_mandate_cannot_be_read() {
    let fx = seeded();
    let of = |connection: &str| {
        answer(
            "reached",
            reached(
                &fx.journal,
                &fx.store,
                &owner(),
                &Scope::Connection(connection.to_owned()),
            ),
        )
    };
    assert_eq!(
        of(CONN),
        set(&[AGENT, FOURTH_AGENT, "agent-unread", "agent-corrupt"])
    );
    assert_eq!(
        of(OTHER_CONN),
        set(&[OTHER_AGENT, "agent-unread", "agent-corrupt"])
    );
    assert_eq!(of("conn_nobody"), set(&["agent-unread", "agent-corrupt"]));
}

#[test]
fn a_workspace_pause_reaches_every_active_agent() {
    let fx = seeded();
    assert_eq!(
        answer(
            "reached",
            reached(&fx.journal, &fx.store, &owner(), &Scope::Workspace)
        ),
        set(&[
            AGENT,
            OTHER_AGENT,
            FOURTH_AGENT,
            "agent-unread",
            "agent-corrupt"
        ])
    );
}

#[test]
fn a_pause_commits_one_agent_scoped_pause_per_agent_reached() {
    for (scope, expected) in [
        (Scope::Agent(OTHER_AGENT.to_owned()), set(&[OTHER_AGENT])),
        (
            Scope::Connection(CONN.to_owned()),
            set(&[AGENT, FOURTH_AGENT, "agent-unread", "agent-corrupt"]),
        ),
        (
            Scope::Workspace,
            set(&[
                AGENT,
                OTHER_AGENT,
                FOURTH_AGENT,
                "agent-unread",
                "agent-corrupt",
            ]),
        ),
    ] {
        let mut fx = seeded();
        let before = control(&fx).len();
        let paused = run_pause(&mut fx, &scope);
        assert_eq!(paused_agents(&fx, before), expected, "{scope:?}");
        let reported: BTreeSet<String> = paused.iter().map(|p| p.agent.clone()).collect();
        assert_eq!(reported, expected, "{scope:?}: every agent reported");
        assert!(paused.iter().all(|p| p.outcome.is_ok()), "{scope:?}");
    }
}

#[test]
fn a_pause_with_no_agent_in_scope_commits_nothing() {
    let mut fx = Fixture::new();
    let before = control(&fx).len();
    assert!(run_pause(&mut fx, &Scope::Workspace).is_empty());
    assert!(run_pause(&mut fx, &Scope::Connection(CONN.to_owned())).is_empty());
    assert_eq!(control(&fx).len(), before);
    assert_eq!(
        paused_lines(&[]),
        vec!["no deployed agent in scope; nothing committed".to_owned()]
    );
}

/// DEC-2000 item 3: one agent whose pause cannot commit does not hold back the others' brakes.
#[test]
fn a_pause_goes_on_past_an_agent_whose_pause_fails() {
    let mut fx = seeded();
    deployed(&mut fx, "agent:bad", &missing_version());
    let before = control(&fx).len();
    let paused = run_pause(&mut fx, &Scope::Workspace);
    let failed: Vec<&Paused> = paused.iter().filter(|p| p.outcome.is_err()).collect();
    assert_eq!(failed.len(), 1, "{paused:?}");
    assert_eq!(failed[0].agent, "agent:bad");
    assert_eq!(
        paused_agents(&fx, before),
        set(&[
            AGENT,
            OTHER_AGENT,
            FOURTH_AGENT,
            "agent-unread",
            "agent-corrupt"
        ])
    );
    let lines = paused_lines(&paused);
    assert_eq!(lines.len(), paused.len(), "one line per agent");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("pause agent:bad not committed: ")),
        "{lines:?}"
    );
}

#[test]
fn a_kill_switch_is_one_owner_command_of_its_scope_with_no_step_up() {
    for (scope, kind, subject, agent) in [
        (
            Scope::Agent(AGENT.to_owned()),
            "agent",
            AGENT,
            common::text(AGENT),
        ),
        (
            Scope::Connection(CONN.to_owned()),
            "connection",
            CONN,
            Value::Null,
        ),
        (Scope::Workspace, "workspace", "ws1", Value::Null),
    ] {
        let mut fx = seeded();
        let before = control(&fx).len();
        let mut ids = FixedIds::default();
        let submitted = answer(
            "kill",
            brake::kill(&mut fx.journal, &mut ids, &owner(), &scope, at(ASKED_AT)),
        );
        let events = commands_after(&fx, before);
        assert_eq!(events.len(), 1, "{scope:?}: one event");
        let event = &events[0];
        assert_eq!(member_text(event, "payload.command"), Some("kill_switch"));
        assert_eq!(member_text(event, "payload.scope"), Some(kind));
        assert_eq!(member_text(event, "payload.subject"), Some(subject));
        assert_eq!(member(event, "payload.agent"), Some(&agent));
        assert_eq!(member(event, "payload.step_up"), Some(&Value::Null));
        assert_eq!(
            member_text(event, "event_id"),
            Some(submitted.event_id.as_str())
        );
        assert_eq!(ids.assertions, 0, "no step-up evidence is minted");
        assert_eq!(
            killed_line(&owner(), &scope, &submitted),
            format!(
                "kill_switch {kind} {subject} committed as event {} at seq {}",
                submitted.event_id, submitted.seq
            )
        );
    }
}

/// E8-9k (notifications spec §3.4): a kill switch from the CLI is one notice, as from the API. Its
/// `OwnerCommandIssued` commits in one append with the `OwnerAlertSent` that names it as subject,
/// cause, and `owner_command`, and the journal's own checks accept that batch, so the executor
/// alerting no user's switch leaves none silent.
#[test]
fn a_kill_switch_commits_its_owner_alert_in_its_batch() {
    for scope in [
        Scope::Agent(AGENT.to_owned()),
        Scope::Connection(CONN.to_owned()),
        Scope::Workspace,
    ] {
        let mut fx = seeded();
        let before = control(&fx).len();
        let mut ids = FixedIds::default();
        let submitted = answer(
            "kill",
            brake::kill(&mut fx.journal, &mut ids, &owner(), &scope, at(ASKED_AT)),
        );
        let rows = fx.rows(CONTROL);
        let batch = &rows[before..];
        let types: Vec<&str> = batch.iter().map(|r| r.event_type.as_str()).collect();
        assert_eq!(
            types,
            ["OwnerCommandIssued", "OwnerAlertSent"],
            "{scope:?}: the command and its alert"
        );
        assert_eq!(
            fx.journal.attempts.last().map(Vec::len),
            Some(2),
            "{scope:?}: one append holds both"
        );
        let alert = body(&batch[1]);
        let command = Some(submitted.event_id.as_str());
        assert_eq!(member_text(&alert, "causation_id"), command, "{scope:?}");
        assert_eq!(member_text(&alert, "payload.subject"), command, "{scope:?}");
        assert_eq!(
            member_text(&alert, "payload.owner_command"),
            command,
            "{scope:?}"
        );
        assert_eq!(member_text(&alert, "payload.kind"), Some("kill_switch"));
        assert_eq!(member_text(&alert, "actor.id"), Some(OWNER));
        let parsed: Vec<mandate_journal::Draft> = batch
            .iter()
            .map(|row| {
                mandate_journal::Draft::parse(&row.body)
                    .unwrap_or_else(|e| panic!("{scope:?}: the journal refuses {e:?}"))
            })
            .collect();
        assert_eq!(mandate_journal::check_batch(&parsed), Ok(()), "{scope:?}");
    }
}

/// Rule 13: a second kill switch of the same scope is committed again, never folded into the first.
#[test]
fn a_repeated_kill_switch_commits_again() {
    let mut fx = seeded();
    let before = control(&fx).len();
    let mut ids = FixedIds::default();
    let first = answer(
        "kill",
        brake::kill(
            &mut fx.journal,
            &mut ids,
            &owner(),
            &Scope::Workspace,
            at(ASKED_AT),
        ),
    );
    let second = answer(
        "kill",
        brake::kill(
            &mut fx.journal,
            &mut ids,
            &owner(),
            &Scope::Workspace,
            at(ASKED_AT + 1),
        ),
    );
    assert_ne!(first, second);
    assert_eq!(commands_after(&fx, before).len(), 2);
}

#[test]
fn a_pause_prints_one_line_per_agent() {
    let at_seq = |seq: u64| Submitted {
        event_id: format!("ev{seq}"),
        seq,
    };
    let lines = paused_lines(&[
        Paused {
            agent: AGENT.to_owned(),
            outcome: Ok(at_seq(7)),
        },
        Paused {
            agent: OTHER_AGENT.to_owned(),
            outcome: Err(ControlError::Journal("unavailable".to_owned())),
        },
    ]);
    assert_eq!(
        lines,
        vec![
            "paused agent-a as event ev7 at seq 7".to_owned(),
            "pause agent-b not committed: the journal: unavailable".to_owned(),
        ]
    );
}

/// DEC-2000 item 3: `agent pause` exits non-zero exactly when a pause did not commit, naming how
/// many, so a partly committed brake never reads as done.
#[test]
fn a_pause_exits_non_zero_exactly_when_a_pause_did_not_commit() {
    let committed = |agent: &str, seq: u64| Paused {
        agent: agent.to_owned(),
        outcome: Ok(Submitted {
            event_id: format!("ev{seq}"),
            seq,
        }),
    };
    let failed = |agent: &str| Paused {
        agent: agent.to_owned(),
        outcome: Err(ControlError::Journal("unavailable".to_owned())),
    };
    assert!(
        pauses_settled(&[]).is_ok(),
        "no agent in scope is no failure"
    );
    assert!(
        pauses_settled(&[committed(AGENT, 7), committed(OTHER_AGENT, 8)]).is_ok(),
        "every pause committed"
    );
    let one = pauses_settled(&[committed(AGENT, 7), failed(OTHER_AGENT)]);
    assert_eq!(
        one.map_err(|e| e.to_string()),
        Err("1 pauses did not commit".to_owned())
    );
    let two = pauses_settled(&[failed(AGENT), failed(OTHER_AGENT)]);
    assert_eq!(
        two.map_err(|e| e.to_string()),
        Err("2 pauses did not commit".to_owned())
    );
}

/// Rule 3: a pause the owner's flags refuse fails before it opens a journal or a store, and prints
/// nothing, so it never reads as a brake that held.
#[test]
fn a_pause_refused_at_its_owner_opens_nothing_and_prints_nothing() {
    let store = std::env::temp_dir().join(format!("mandate-brake-refused-{}", std::process::id()));
    let store_text = store.to_string_lossy().into_owned();
    let cli = Cli::try_parse_from([
        "mandate",
        "agent",
        "pause",
        "--workspace-wide",
        "--workspace",
        "ws1",
        "--user",
        OWNER,
        "--environment",
        "staging",
        "--journal",
        "postgres://journal@localhost/mandate",
        "--store",
        &store_text,
    ])
    .expect("a workspace-wide pause parses");
    let Command::Agent(AgentCommand::Pause(args)) = cli.command else {
        panic!("parsed as {:?}", cli.command);
    };
    let mut printed = Vec::new();
    let outcome = brake::run_pause(&args, at(ASKED_AT), &mut printed);
    let refusal = outcome
        .expect_err("an unknown environment is refused")
        .to_string();
    assert!(refusal.contains("environment_invalid"), "{refusal}");
    assert!(printed.is_empty(), "{}", String::from_utf8_lossy(&printed));
    assert!(!store.exists(), "no store is created for a refused pause");
}

fn parsed(args: &[&str]) -> Result<Cli, clap::Error> {
    let owner = [
        "--workspace",
        "ws1",
        "--user",
        OWNER,
        "--journal",
        "postgres://journal@localhost/mandate",
        "--store",
        "/work/store",
    ];
    let argv = ["mandate", "agent"]
        .into_iter()
        .chain(args.iter().copied())
        .chain(owner);
    Cli::try_parse_from(argv)
}

#[test]
fn the_binary_offers_pause_and_kill_at_each_scope_and_still_deploy() {
    for verb in ["pause", "kill"] {
        for (flags, scope) in [
            (vec!["--agent", AGENT], Scope::Agent(AGENT.to_owned())),
            (
                vec!["--connection", CONN],
                Scope::Connection(CONN.to_owned()),
            ),
            (vec!["--workspace-wide"], Scope::Workspace),
        ] {
            let args: Vec<&str> = [verb].into_iter().chain(flags.iter().copied()).collect();
            let cli = parsed(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let brake = match (verb, cli.command) {
                ("pause", Command::Agent(AgentCommand::Pause(b)))
                | ("kill", Command::Agent(AgentCommand::Kill(b))) => b,
                (_, other) => panic!("{args:?} parsed as {other:?}"),
            };
            assert_eq!(brake.reach.scope(), scope, "{args:?}");
        }
        for wrong in [
            vec![verb],
            vec![verb, "--agent", AGENT, "--connection", CONN],
            vec![verb, "--agent", AGENT, "--workspace-wide"],
            vec![verb, "--connection", CONN, "--workspace-wide"],
        ] {
            assert!(parsed(&wrong).is_err(), "{wrong:?} names one scope or none");
        }
    }
    let deploy = parsed(&["deploy", AGENT, "sha256:00"]).unwrap();
    assert!(matches!(
        deploy.command,
        Command::Agent(AgentCommand::Gesture(gestures::AgentCommand::Deploy(_)))
    ));
}

#[derive(Debug, Clone)]
enum Seed {
    /// Deploy agent `.0` with the mandate on connection `.1`, or with one the store lacks (`None`).
    Deploy(usize, Option<usize>),
    Stop(usize),
}

const CONNS: [&str; 3] = [CONN, OTHER_CONN, "conn_alpaca_paper_03"];

fn seeds() -> impl Strategy<Value = Vec<Seed>> {
    let seed = prop_oneof![
        3 => (0..AGENTS.len(), prop::option::weighted(0.85, 0..CONNS.len()))
            .prop_map(|(a, c)| Seed::Deploy(a, c)),
        1 => (0..AGENTS.len()).prop_map(Seed::Stop),
    ];
    prop::collection::vec(seed, 0..12)
}

/// The oracle: each agent's connection while deployed, walked from the seeds alone (`None` for a
/// mandate the store lacks); a stop ends it.
fn oracle(seeds: &[Seed]) -> BTreeMap<String, Option<String>> {
    let mut active = BTreeMap::new();
    for seed in seeds {
        match seed {
            Seed::Deploy(a, c) => {
                active.insert(AGENTS[*a].to_owned(), c.map(|c| CONNS[c].to_owned()));
            }
            Seed::Stop(a) => {
                active.remove(AGENTS[*a]);
            }
        }
    }
    active
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// Over random deployments and stops, a pause of each scope commits exactly one agent-scoped
    /// pause for each agent the oracle says it reaches, and nothing else.
    #[test]
    fn a_pause_reaches_exactly_the_agents_the_oracle_names(seeds in seeds(), pick in 0..5usize) {
        let mut fx = Fixture::new();
        let versions: Vec<String> = CONNS.iter().map(|c| stored_on(&mut fx, c)).collect();
        for seed in &seeds {
            match seed {
                Seed::Deploy(a, c) => {
                    let version = c.map_or_else(missing_version, |c| versions[c].clone());
                    deployed(&mut fx, AGENTS[*a], &version);
                }
                Seed::Stop(a) => stopped(&mut fx, AGENTS[*a]),
            }
        }
        let active = oracle(&seeds);
        let (scope, expected): (Scope, BTreeSet<String>) = match pick {
            0 => (Scope::Workspace, active.keys().cloned().collect()),
            1..=3 => {
                let conn = CONNS[pick - 1];
                let reached = active
                    .iter()
                    .filter(|(_, c)| c.as_deref().is_none_or(|c| c == conn))
                    .map(|(a, _)| a.clone())
                    .collect();
                (Scope::Connection(conn.to_owned()), reached)
            }
            _ => (Scope::Agent(AGENT.to_owned()), set(&[AGENT])),
        };
        let before = control(&fx).len();
        let paused = run_pause(&mut fx, &scope);
        prop_assert_eq!(paused_agents(&fx, before), expected.clone());
        let reported: BTreeSet<String> = paused.iter().map(|p| p.agent.clone()).collect();
        prop_assert_eq!(reported, expected);
    }
}

/// Journal spec v0.84, DEC-2131 item 4: an `AgentDeployed` version 3, its version 2's payload with
/// a trailing `step_up`, is the agent's active deployment as its version 2 is: a pause of its
/// connection, of another and of the workspace reaches the same agents.
#[test]
fn a_version_3_deployment_reads_as_its_version_2() {
    let reach = |schema_version: i64| {
        let mut fx = seeded();
        let on_conn = stored_on(&mut fx, CONN);
        let mut members = vec![
            ("agent_id", common::text("agent-stepped")),
            ("mandate_version", common::text(&on_conn)),
            (
                "record_ref",
                common::text(&format!("sha256:{}", "6".repeat(64))),
            ),
            ("connection_id", common::text(CONN)),
        ];
        if schema_version == 3 {
            let step_up = common::object(&[
                ("assertion_id", common::text("01JAA3P0A000000000000000H1")),
                (
                    "authenticated_at",
                    common::text("2026-09-21T13:59:57.000000000Z"),
                ),
                ("method", common::text("passkey")),
            ]);
            members.push(("step_up", step_up));
        }
        let payload = common::object(&members);
        fx.append_versioned(
            "paper",
            CONTROL,
            "AgentDeployed",
            None,
            payload,
            schema_version,
        );
        let stored = fx.rows(CONTROL);
        assert_eq!(
            stored.last().map(|r| r.schema_version),
            u64::try_from(schema_version).ok()
        );
        [
            Scope::Connection(CONN.to_owned()),
            Scope::Connection(OTHER_CONN.to_owned()),
            Scope::Workspace,
        ]
        .map(|scope| answer("reached", reached(&fx.journal, &fx.store, &owner(), &scope)))
    };
    let version_2 = reach(2);
    assert!(version_2[0].contains("agent-stepped"), "{version_2:?}");
    assert!(!version_2[1].contains("agent-stepped"), "{version_2:?}");
    assert_eq!(reach(3), version_2);
}
