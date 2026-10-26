//! A version-3 `AgentDeployed` (journal spec v0.84, DEC-2131 item 4) is followed as its version 2:
//! the executor hands its agent's openings and exits and applies its kill switch, and a revocation
//! hides neither its exit nor its switch. Before the change the follow fold matched version 2 alone,
//! so a version-3 deployment named no connection and the agent was never followed.

mod support;

use support::{
    AGENT, CONNECTION, Error, Journal, NOON, Paper, at, connect, deploy_stepped_up, kill, propose,
    revoke, start, start_allowing, tick,
};

const COPY: &str = "01JKILL00000000000000000AA";
const OPEN: &str = "01JINTENT0000000000000000A";
const EXIT: &str = "01JINTENT0000000000000000C";

fn deployed_stepped_up() -> Result<Journal, Error> {
    let journal = Journal::default();
    connect(&journal, "c1", CONNECTION, support::ACCOUNT)?;
    deploy_stepped_up(&journal, "c2", AGENT, CONNECTION)?;
    tick(&journal, "t1", NOON)?;
    Ok(journal)
}

#[test]
fn a_version_3_deployments_opening_and_exit_are_handed() -> Result<(), Error> {
    let journal = deployed_stepped_up()?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [OPEN, EXIT], "both, as for version 2");
    Ok(())
}

#[test]
fn a_version_3_deployments_kill_switch_is_applied() -> Result<(), Error> {
    let journal = deployed_stepped_up()?;
    kill(&journal, AGENT, COPY)?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.switched(), [AGENT]);
    Ok(())
}

#[test]
fn a_revoked_version_3_deployment_still_has_its_exit_handed() -> Result<(), Error> {
    let journal = deployed_stepped_up()?;
    revoke(&journal, "c3", CONNECTION, false)?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT], "the exit only");
    Ok(())
}
