//! Properties of the executor process over random runs (E21-30, DEC-2003): restarts, crashes at
//! any append, lost journal answers, lost broker answers, kill switches and extra passes, each
//! against an oracle that reads only what the broker saw and what the journal holds.

mod support;

use mandate_process::State;
use proptest::prelude::*;
use support::{
    ACCOUNT_STREAM, AGENT, Error, Fault, Journal, NOON, OTHER_AGENT, Paper, at, deploy, deployed,
    kill, start,
};

#[derive(Debug, Clone)]
enum Op {
    Pass,
    Restart,
    /// The process dies after this many more appends, and a new one starts.
    CrashAfter(usize),
    JournalFault(bool),
    LoseBrokerAnswer,
    Kill(bool),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => Just(Op::Pass),
        1 => Just(Op::Restart),
        1 => (0usize..4).prop_map(Op::CrashAfter),
        1 => any::<bool>().prop_map(Op::JournalFault),
        1 => Just(Op::LoseBrokerAnswer),
        2 => any::<bool>().prop_map(Op::Kill),
    ]
}

/// Runs `ops` on an account with two agents, then lets the journal recover and restarts until a
/// process stays up. Answers the journal, the broker, and the agents whose streams hold a switch.
fn run(ops: &[Op]) -> Result<(Journal, Paper, Vec<&'static str>), Error> {
    let journal = deployed()?;
    deploy(&journal, "c3", OTHER_AGENT, "conn1")?;
    let broker = Paper::new(&journal);
    let mut killed = Vec::new();
    let mut process = start(&journal, &broker)?;
    let now = at(NOON)?;
    for op in ops {
        match op {
            Op::Pass => {
                process.step(now);
            }
            Op::Restart => process = start(&journal, &broker)?,
            Op::CrashAfter(n) => {
                journal.crash_after(Some(*n));
                for _ in 0..3 {
                    process.step(now);
                }
                journal.crash_after(None);
                process = start(&journal, &broker)?;
            }
            Op::JournalFault(lost) => journal.fail_next(&[if *lost {
                Fault::LostAnswer
            } else {
                Fault::Unavailable
            }]),
            Op::LoseBrokerAnswer => broker.lose_next_answers(1),
            Op::Kill(other) => {
                let (agent, id) = if *other {
                    (OTHER_AGENT, "01JK1MM00000000000000000AB")
                } else {
                    (AGENT, "01JK1MM00000000000000000AA")
                };
                if !killed.contains(&agent) {
                    kill(&journal, agent, id)?;
                    killed.push(agent);
                }
            }
        }
    }
    journal.clear_faults();
    broker.lose_next_answers(0);
    for _ in 0..4 {
        process = start(&journal, &broker)?;
        for _ in 0..4 {
            process.step(now);
        }
        if !matches!(process.lifecycle().state(), State::Exited(_)) {
            break;
        }
    }
    Ok((journal, broker, killed))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Whatever restarts, crashes and lost answers a run meets, once it settles each agent whose
    /// stream holds a switch has had it applied exactly once, no other agent was switched, and the
    /// broker never saw an account-wide request (`AGENTS.md` rule 13).
    #[test]
    fn every_switch_is_applied_exactly_once(ops in proptest::collection::vec(op(), 0..24)) {
        let (journal, broker, killed) = run(&ops).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut switched = journal.switched();
        switched.sort();
        let mut want: Vec<String> = killed.iter().map(|a| (*a).to_owned()).collect();
        want.sort();
        prop_assert_eq!(switched, want);
        prop_assert!(broker.seen().iter().all(|seen| seen.journaled_first));
        prop_assert!(broker.seen().iter().all(|seen| !matches!(
            seen.request,
            mandate_executor::BrokerRequest::CancelAll(_)
                | mandate_executor::BrokerRequest::ClosePosition(..)
        )));
    }

    /// A wake hint only adds a pass, and a pass with nothing new journals nothing and sends
    /// nothing: once a run has settled, more passes leave the account stream and the broker as
    /// they were (DEC-17, ES-20). The process takes no hint as input at all, so dropping every
    /// hint leaves only the bounded passes, which read the journal whether or not one came.
    #[test]
    fn a_pass_with_nothing_new_changes_nothing(ops in proptest::collection::vec(op(), 0..16), extra in 1usize..4) {
        let (journal, broker, _) = run(&ops).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let mut process = start(&journal, &broker).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let now = at(NOON).map_err(|e| TestCaseError::fail(e.to_string()))?;
        process.step(now);
        prop_assert!(process.ready(), "the settled process is up and reconciled");
        let rows = journal.rows_of(ACCOUNT_STREAM).len();
        let seen = broker.seen().len();
        prop_assert!(rows > 0 && seen > 0, "the process journaled and asked the broker something");
        for _ in 0..extra {
            process.step(now);
        }
        prop_assert_eq!(journal.rows_of(ACCOUNT_STREAM).len(), rows);
        prop_assert_eq!(broker.seen().len(), seen);
    }
}
