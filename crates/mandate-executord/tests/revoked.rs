//! A revocation never hides an agent's kill switch from the executor (DEC-2343, workspace API
//! §5.6). The executor keeps following an agent deployed on a connection before its revocation, for
//! kill switches, exits and protective re-placement, but hands none of its openings. It reads a
//! connection's or the workspace's kill-switch command itself for an agent whose connection is
//! revoked at or after it, and applies each switch once, whichever of the command and the runtime's
//! copy it reads first.

mod support;

use support::{
    AGENT, CONNECTION, Error, OTHER_ACCOUNT, OTHER_AGENT, OTHER_CONNECTION, Paper, WORKSPACE, at,
    connect, copy_of, deploy, deployed, kill, kill_command, propose, revoke, start, start_allowing,
    stopped,
};

const COMMAND: &str = "01JCMD000000000000000000AA";
const COPY: &str = "01JKILL00000000000000000AA";
const OTHER_COPY: &str = "01JKILL00000000000000000AB";
const OPEN: &str = "01JINTENT0000000000000000A";
const EXIT: &str = "01JINTENT0000000000000000C";
const LATER_OPEN: &str = "01JINTENT0000000000000000D";
const NOON: &str = support::NOON;

/// §5.6's compromise batch, the kill switch at the connection's scope then the revocation, with no
/// runtime copy at all: the executor applies the switch to the agent from the command itself.
#[test]
fn a_compromise_revocation_applies_its_kill_switch_with_no_copy() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    revoke(&journal, "c4", CONNECTION, true)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())],
        "the agent's switch, copied from the command"
    );
    Ok(())
}

/// The runtime's copy, written after the revocation as it must be, is applied once: the executor
/// reads the command first and counts the copy that names it as applied, on this pass and after a
/// restart.
#[test]
fn the_copy_after_a_direct_read_is_not_applied_again() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    revoke(&journal, "c4", CONNECTION, true)?;
    copy_of(&journal, AGENT, COPY, COMMAND, "connection", CONNECTION)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    let mut restarted = start(&journal, &broker)?;
    restarted.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())],
        "applied once, from the command"
    );
    Ok(())
}

/// The other order: a copy applied before the revocation is not read again from the command once
/// the revocation lands.
#[test]
fn a_copy_applied_before_the_revocation_is_not_read_again_from_the_command() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    copy_of(&journal, AGENT, COPY, COMMAND, "connection", CONNECTION)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    revoke(&journal, "c4", CONNECTION, true)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COPY.to_owned())],
        "applied once, from the copy"
    );
    Ok(())
}

/// A workspace kill switch before a revocation of the agent's connection is applied from the
/// command too.
#[test]
fn a_workspace_kill_switch_before_a_revocation_is_applied() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "workspace", WORKSPACE)?;
    revoke(&journal, "c4", CONNECTION, true)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())]
    );
    Ok(())
}

/// The stopped agent's case (DEC-2343 "Not covered", but reached by item 3): a kill switch after
/// `AgentStopped` has no runtime to copy it, and a revocation after it still applies it.
#[test]
fn a_stopped_agents_switch_is_applied_from_the_command_once_revoked() -> Result<(), Error> {
    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    revoke(&journal, "c5", CONNECTION, false)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())]
    );
    Ok(())
}

/// A revoked agent's copy, of a switch issued after the revocation, is still applied: the
/// revocation does not end the following of an agent deployed before it.
#[test]
fn a_revoked_agents_later_copy_is_applied() -> Result<(), Error> {
    let journal = deployed()?;
    revoke(&journal, "c3", CONNECTION, false)?;
    kill(&journal, AGENT, COPY)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// A revoked agent's opening is not handed and its exit is; established again, its next opening is
/// handed (DEC-2343 item 1). An opening proposed while revoked may be handed once established again:
/// the agent stream's rows have no order against the control stream's, and the core abandons an
/// intent too old to submit.
#[test]
fn a_revoked_agents_opening_is_not_handed_and_its_exit_is() -> Result<(), Error> {
    let journal = deployed()?;
    revoke(&journal, "c3", CONNECTION, false)?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT], "the exit only");

    connect(&journal, "c4", CONNECTION, support::ACCOUNT)?;
    propose(&journal, AGENT, LATER_OPEN, "buy", "10", "150", "open")?;
    process.step(at(NOON)?);
    assert!(
        journal.received().contains(&LATER_OPEN.to_owned()),
        "established again, its openings are handed: {:?}",
        journal.received()
    );
    Ok(())
}

/// Every connection an agent was deployed on counts (DEC-2343 item 2): a later deployment naming
/// another account's connection does not drop the agent from this account's executor.
#[test]
fn a_later_deployment_elsewhere_keeps_the_agent_followed_here() -> Result<(), Error> {
    let journal = deployed()?;
    connect(&journal, "c3", OTHER_CONNECTION, OTHER_ACCOUNT)?;
    deploy(&journal, "c4", AGENT, OTHER_CONNECTION)?;
    kill(&journal, AGENT, COPY)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

/// Option (2) of DEC-2343: with no revocation, a connection's kill switch waits for the runtime's
/// copy, which carries the owner's confirmation, and is applied once from it.
#[test]
fn without_a_revocation_the_command_waits_for_its_copy() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(journal.applications().is_empty(), "nothing before the copy");
    copy_of(&journal, AGENT, COPY, COMMAND, "connection", CONNECTION)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COPY.to_owned())]
    );
    Ok(())
}

/// A command for another account's connection, revoked, reaches none of this account's agents.
#[test]
fn another_accounts_revoked_command_reaches_no_agent_here() -> Result<(), Error> {
    let journal = deployed()?;
    connect(&journal, "c3", OTHER_CONNECTION, OTHER_ACCOUNT)?;
    deploy(&journal, "c4", OTHER_AGENT, OTHER_CONNECTION)?;
    kill_command(&journal, COMMAND, "connection", OTHER_CONNECTION)?;
    revoke(&journal, "c6", OTHER_CONNECTION, true)?;
    copy_of(
        &journal,
        OTHER_AGENT,
        OTHER_COPY,
        COMMAND,
        "connection",
        OTHER_CONNECTION,
    )?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert!(journal.applications().is_empty());
    Ok(())
}

/// An agent deployed on the connection after the command is not covered by it: the command reaches
/// the agents it named when it was issued.
#[test]
fn an_agent_deployed_after_the_command_is_not_covered_by_it() -> Result<(), Error> {
    let journal = deployed()?;
    kill_command(&journal, COMMAND, "connection", CONNECTION)?;
    deploy(&journal, "c4", OTHER_AGENT, CONNECTION)?;
    revoke(&journal, "c5", CONNECTION, true)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(
        journal.applications(),
        [(AGENT.to_owned(), COMMAND.to_owned())],
        "the agent deployed before it only"
    );
    Ok(())
}
