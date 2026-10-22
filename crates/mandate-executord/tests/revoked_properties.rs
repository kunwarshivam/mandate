//! DEC-2343's invariant 1 over random runs: every kill switch the journal holds for an agent the
//! executor follows is applied exactly once, whatever the order of deployments, kill-switch
//! commands, the runtime's copies (written late or never), revocations ordinary or compromised,
//! re-establishments, `AgentStopped`, passes and restarts.
//!
//! The oracle reads only the journaled rows, in `seq` order, and never `follow.rs`: an agent is
//! followed when any of its version-2 deployments named a connection this account established. A
//! followed agent owes a command's switch once when the runtime copied the command onto its stream
//! (the runtime judged it covered), or when the command covers it, deployed before it on its
//! connection (or, for the workspace, on any of the account's connections), and that connection is
//! revoked after the command or the agent has an `AgentStopped`, so no runtime copies it, or when an
//! agent-scope command names a stopped agent; and never otherwise. Seeded followers that drop an agent on revocation, or keep only its latest
//! deployment, are shown rejected by the same comparison.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use support::{
    ACCOUNT, CONTROL_STREAM, Error, Journal, NOON, OTHER_ACCOUNT, Paper, WORKSPACE, agent_stream,
    at, connect, copy_of, deploy, kill_command, revoke, start, stopped, tick,
};

const AGENTS: [&str; 2] = ["agenta", "agentb"];
const CONNECTIONS: [(&str, &str); 2] = [("conn1", ACCOUNT), ("conn2", OTHER_ACCOUNT)];

#[derive(Debug, Clone)]
enum Op {
    Deploy(usize, usize),
    Command(Option<usize>),
    AgentCommand(usize),
    Copy(usize),
    Revoke(usize, bool),
    Establish(usize),
    Stop(usize),
    Pass,
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => (0..AGENTS.len(), 0..CONNECTIONS.len()).prop_map(|(a, c)| Op::Deploy(a, c)),
        2 => proptest::option::of(0..CONNECTIONS.len()).prop_map(Op::Command),
        1 => (0..AGENTS.len()).prop_map(Op::AgentCommand),
        2 => (0usize..8).prop_map(Op::Copy),
        1 => (0..CONNECTIONS.len(), any::<bool>()).prop_map(|(c, k)| Op::Revoke(c, k)),
        1 => (0..CONNECTIONS.len()).prop_map(Op::Establish),
        1 => (0..AGENTS.len()).prop_map(Op::Stop),
        3 => Just(Op::Pass),
        1 => Just(Op::Restart),
    ]
}

/// A kill-switch command as the oracle reads it: its id, `seq`, and connection (`None` for the
/// workspace).
#[derive(Debug, Clone)]
struct Command {
    id: String,
    seq: u64,
    connection: Option<String>,
    /// The agent an agent-scope command names; `None` with no connection is the workspace.
    agent: Option<String>,
}

fn payload(row: &StoredEvent) -> Value {
    parse(&row.body)
        .ok()
        .and_then(|body| body.get("payload").cloned())
        .unwrap_or(Value::Null)
}

fn member(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The control stream as the oracle needs it: each connection's account by its version-2
/// establishments, each agent's deployments with their `seq`, the kill-switch commands, and each
/// connection's revocations' `seq`.
struct Control {
    ours: BTreeSet<String>,
    deployed: BTreeMap<String, Vec<(u64, String)>>,
    commands: Vec<Command>,
    revoked: BTreeMap<String, Vec<u64>>,
    stopped: BTreeSet<String>,
}

fn control(rows: &[StoredEvent]) -> Control {
    let mut out = Control {
        ours: BTreeSet::new(),
        deployed: BTreeMap::new(),
        commands: Vec::new(),
        revoked: BTreeMap::new(),
        stopped: BTreeSet::new(),
    };
    for row in rows {
        let p = payload(row);
        match (row.event_type.as_str(), row.schema_version) {
            ("ConnectionEstablished", 2) if member(&p, "account_ref") == ACCOUNT => {
                out.ours.insert(member(&p, "connection_id"));
            }
            ("AgentDeployed", 2) => out
                .deployed
                .entry(member(&p, "agent_id"))
                .or_default()
                .push((row.seq, member(&p, "connection_id"))),
            ("OwnerCommandIssued", _) if member(&p, "command") == "kill_switch" => {
                out.commands.push(Command {
                    id: row.event_id.clone(),
                    seq: row.seq,
                    connection: (member(&p, "scope") == "connection")
                        .then(|| member(&p, "subject")),
                    agent: (member(&p, "scope") == "agent").then(|| member(&p, "subject")),
                });
            }
            ("AgentStopped", _) => {
                out.stopped.insert(member(&p, "agent_id"));
            }
            ("ConnectionRevoked", _) => out
                .revoked
                .entry(member(&p, "connection_id"))
                .or_default()
                .push(row.seq),
            _ => {}
        }
    }
    out
}

/// The connections of this account that `agent` was deployed on before `seq`, and that `command`
/// names (all of them, for the workspace).
fn covering(control: &Control, agent: &str, command: &Command) -> BTreeSet<String> {
    if command.agent.is_some() {
        return BTreeSet::new();
    }
    control
        .deployed
        .get(agent)
        .into_iter()
        .flatten()
        .filter(|(seq, connection)| *seq < command.seq && control.ours.contains(connection))
        .map(|(_, connection)| connection.clone())
        .filter(|connection| {
            command
                .connection
                .as_ref()
                .is_none_or(|named| named == connection)
        })
        .collect()
}

/// The copies on `agent`'s stream, by the command each names.
fn copies(journal: &Journal, agent: &str) -> BTreeMap<String, String> {
    journal
        .rows_of(&agent_stream(agent))
        .iter()
        .filter(|row| row.event_type == "KillSwitchActivated")
        .filter_map(|row| {
            let causation = parse(&row.body)
                .ok()?
                .get("causation_id")?
                .as_str()?
                .to_owned();
            Some((row.event_id.clone(), causation))
        })
        .collect()
}

/// What the journal owes: each (agent, command) pair the executor must apply once.
fn owed(journal: &Journal) -> BTreeSet<(String, String)> {
    let control = control(&journal.rows_of(CONTROL_STREAM));
    let mut out = BTreeSet::new();
    for agent in AGENTS {
        let followed = control
            .deployed
            .get(agent)
            .into_iter()
            .flatten()
            .any(|(_, connection)| control.ours.contains(connection));
        if !followed {
            continue;
        }
        let copied: BTreeSet<String> = copies(journal, agent).into_values().collect();
        for command in &control.commands {
            let covering = covering(&control, agent, command);
            let revoked_after = covering.iter().any(|connection| {
                control
                    .revoked
                    .get(connection)
                    .is_some_and(|seqs| seqs.iter().any(|seq| *seq > command.seq))
            });
            let named = command.agent.as_deref() == Some(agent);
            let unrun = (named || !covering.is_empty()) && control.stopped.contains(agent);
            if revoked_after || unrun || copied.contains(&command.id) {
                out.insert((agent.to_owned(), command.id.clone()));
            }
        }
    }
    out
}

/// What the account stream records: each agent switch, by agent and the command it carries out
/// (a copy counts as the command it names).
fn applied(journal: &Journal) -> BTreeMap<(String, String), usize> {
    let mut by_copy = BTreeMap::new();
    for agent in AGENTS {
        by_copy.extend(copies(journal, agent));
    }
    let mut out = BTreeMap::new();
    for (agent, source) in journal.applications() {
        let command = by_copy.get(&source).cloned().unwrap_or(source);
        *out.entry((agent, command)).or_insert(0) += 1;
    }
    out
}

/// Every pair owed applied exactly once, and nothing else applied.
fn violations(
    owed: &BTreeSet<(String, String)>,
    applied: &BTreeMap<(String, String), usize>,
) -> Vec<String> {
    let mut out = Vec::new();
    for pair in owed {
        let times = applied.get(pair).copied().unwrap_or(0);
        if times != 1 {
            out.push(format!("{pair:?} owed once, applied {times} times"));
        }
    }
    for (pair, times) in applied {
        if !owed.contains(pair) {
            out.push(format!("{pair:?} applied {times} times, owed none"));
        }
    }
    out
}

/// Runs `ops` on a workspace with this account's connection and another account's, then settles
/// with a restart and two passes. Answers the journal.
fn run(ops: &[Op]) -> Result<Journal, Error> {
    let journal = Journal::default();
    for (index, (connection, account)) in CONNECTIONS.iter().enumerate() {
        connect(&journal, &format!("e{index}"), connection, account)?;
    }
    tick(&journal, "t1", NOON)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    let now = at(NOON)?;
    let mut copied: BTreeSet<(String, String)> = BTreeSet::new();
    for (n, op) in ops.iter().enumerate() {
        let id = format!("o{n:03}");
        match op {
            Op::Deploy(agent, connection) => {
                deploy(&journal, &id, AGENTS[*agent], CONNECTIONS[*connection].0)?;
            }
            Op::AgentCommand(agent) => kill_command(&journal, &id, "agent", AGENTS[*agent])?,
            Op::Command(connection) => match connection {
                Some(index) => kill_command(&journal, &id, "connection", CONNECTIONS[*index].0)?,
                None => kill_command(&journal, &id, "workspace", WORKSPACE)?,
            },
            Op::Copy(pick) => {
                let control = control(&journal.rows_of(CONTROL_STREAM));
                let runtime_covers = |agent: &str, command: &Command| {
                    command.agent.as_deref().is_none_or(|named| named == agent)
                        && control
                            .deployed
                            .get(agent)
                            .into_iter()
                            .flatten()
                            .any(|(seq, c)| {
                                *seq < command.seq
                                    && command.connection.as_ref().is_none_or(|named| named == c)
                            })
                };
                let due: Vec<(&str, &Command)> = control
                    .commands
                    .iter()
                    .flat_map(|command| AGENTS.iter().map(move |agent| (*agent, command)))
                    .filter(|(agent, command)| runtime_covers(agent, command))
                    .filter(|(agent, command)| {
                        !copied.contains(&((*agent).to_owned(), command.id.clone()))
                    })
                    .collect();
                if let Some((agent, command)) = due.get(pick % due.len().max(1)).copied() {
                    let (scope, subject) = match (&command.connection, &command.agent) {
                        (Some(connection), _) => ("connection", connection.as_str()),
                        (None, Some(named)) => ("agent", named.as_str()),
                        (None, None) => ("workspace", WORKSPACE),
                    };
                    copy_of(&journal, agent, &id, &command.id, scope, subject)?;
                    copied.insert((agent.to_owned(), command.id.clone()));
                }
            }
            Op::Revoke(connection, compromised) => {
                revoke(&journal, &id, CONNECTIONS[*connection].0, *compromised)?;
            }
            Op::Establish(connection) => {
                let (connection, account) = CONNECTIONS[*connection];
                connect(&journal, &id, connection, account)?;
            }
            Op::Stop(agent) => stopped(&journal, &id, AGENTS[*agent])?,
            Op::Pass => {
                process.step(now);
            }
            Op::Restart => process = start(&journal, &broker)?,
        }
    }
    process = start(&journal, &broker)?;
    process.step(now);
    process.step(now);
    Ok(journal)
}

/// DEC-2343 invariant 1: every switch owed is applied exactly once, and no other. A plain test
/// driving proptest's runner, so its pending marker sits on a function (`cargo xtask markers`).
#[test]
fn every_switch_owed_is_applied_exactly_once() -> Result<(), Error> {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 96,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&proptest::collection::vec(op(), 0..20), |ops| {
            let journal = run(&ops).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let problems = violations(&owed(&journal), &applied(&journal));
            prop_assert!(problems.is_empty(), "{}", problems.join("\n"));
            Ok(())
        })
        .map_err(|failure| format!("{failure}"))?;
    Ok(())
}

/// What a seeded follower would apply: every owed pair, less the ones its bug drops.
fn seeded(
    journal: &Journal,
    drops: impl Fn(&Control, &str) -> bool,
) -> BTreeMap<(String, String), usize> {
    let control = control(&journal.rows_of(CONTROL_STREAM));
    owed(journal)
        .into_iter()
        .filter(|(agent, _)| !drops(&control, agent))
        .map(|pair| (pair, 1))
        .collect()
}

/// The oracle is not the follower's own reading: a follower that drops an agent once its
/// connection's latest record is a revocation (the defect), and one that keeps only an agent's
/// latest deployment, are each rejected by the comparison the property makes.
#[test]
fn the_comparison_rejects_the_seeded_followers() -> Result<(), Error> {
    let compromise = run(&[Op::Deploy(0, 0), Op::Command(Some(0)), Op::Revoke(0, true)])?;
    let drop_on_revocation = |control: &Control, agent: &str| {
        control
            .deployed
            .get(agent)
            .into_iter()
            .flatten()
            .any(|(_, connection)| {
                let revoked = control.revoked.get(connection).and_then(|seqs| seqs.last());
                revoked.is_some()
            })
    };
    assert!(
        !violations(&owed(&compromise), &seeded(&compromise, drop_on_revocation)).is_empty(),
        "the drop on revocation is caught"
    );

    let redeployed = run(&[
        Op::Deploy(0, 0),
        Op::Deploy(0, 1),
        Op::Command(None),
        Op::Copy(0),
    ])?;
    let latest_only = |control: &Control, agent: &str| {
        control
            .deployed
            .get(agent)
            .and_then(|deployments| deployments.last())
            .is_some_and(|(_, connection)| !control.ours.contains(connection))
    };
    assert!(
        !violations(&owed(&redeployed), &seeded(&redeployed, latest_only)).is_empty(),
        "the latest deployment replacing an earlier one is caught"
    );
    Ok(())
}
