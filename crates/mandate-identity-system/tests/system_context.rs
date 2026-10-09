//! E9-8: a background process's `SystemContext` (identity spec §4.5, ID-1, ID-8, DEC-642 items 5
//! to 8, DEC-655 item 4, DEC-668). Every expected value is the fixture's or the oracle's own, never
//! read back from the context.

use std::collections::BTreeMap;

use mandate_identity::Permission as P;
use mandate_identity::demand::{Grants, ReadRecords};
use mandate_identity::{OrgId, PrincipalId, PrincipalKind, Refusal, Tenant, WorkspaceId};
use mandate_identity_system::{Registration, SystemActor, SystemContext};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const O1: OrgId = OrgId(0x11);
const W1: WorkspaceId = WorkspaceId(0x21);
const W2: WorkspaceId = WorkspaceId(0x22);
const EXECUTOR: PrincipalId = PrincipalId(0x41);
const RUNTIME: PrincipalId = PrincipalId(0x42);

/// The permissions the property draws registrations from: Read records, the one a data API
/// demands today, and rows a background process could plausibly list.
const POOL: [P; 6] = [
    P::ReadRecords,
    P::ViewAgents,
    P::Pause,
    P::KillSwitchAgent,
    P::DryRun,
    P::Deploy,
];

fn system(
    org: OrgId,
    workspace: WorkspaceId,
    workload: PrincipalId,
    actor: SystemActor,
    listed: &[P],
) -> SystemContext {
    let permissions = listed.iter().copied().collect();
    SystemContext::new(
        org,
        workspace,
        Registration {
            workload,
            actor,
            permissions,
        },
    )
}

/// A stand-in data API over every workspace's rows: it reads only the workspace its context names.
fn rows_of(
    store: &BTreeMap<WorkspaceId, &'static str>,
    context: &impl Tenant,
) -> Option<&'static str> {
    store.get(&context.workspace()).copied()
}

#[test]
#[ignore = "pending E9-8"]
fn a_system_context_reports_what_it_was_built_with() {
    for (workspace, workload, actor, kind) in [
        (W1, EXECUTOR, SystemActor::System, PrincipalKind::Process),
        (W2, RUNTIME, SystemActor::Agent, PrincipalKind::Agent),
    ] {
        let c = system(O1, workspace, workload, actor, &[]);
        let seen = (c.workspace(), c.org(), c.principal(), c.kind(), c.actor());
        assert_eq!(seen, (workspace, O1, workload, kind, actor));
    }
}

#[test]
#[ignore = "pending E9-8"]
fn a_system_context_reaches_only_its_own_workspace() {
    let store = BTreeMap::from([(W1, "w1 rows"), (W2, "w2 rows")]);
    for (workspace, rows) in [(W1, "w1 rows"), (W2, "w2 rows")] {
        let context = system(O1, workspace, EXECUTOR, SystemActor::System, &[]);
        assert_eq!(rows_of(&store, &context), Some(rows), "{workspace:?}");
    }
}

#[test]
#[ignore = "pending E9-8"]
fn a_registration_listing_no_sensitive_permission_cannot_read_records() {
    let listed = [P::ViewAgents, P::Pause, P::KillSwitchAgent];
    let context = system(O1, W1, EXECUTOR, SystemActor::System, &listed);
    let refused = context.require::<ReadRecords>().map(|_| ());
    assert_eq!(refused, Err(Refusal::Forbidden));
}

#[test]
#[ignore = "pending E9-8"]
fn a_listed_permission_yields_a_witness_for_the_same_context() {
    let context = system(O1, W2, RUNTIME, SystemActor::Agent, &[P::ReadRecords]);
    let witness = context.require::<ReadRecords>();
    let seen = witness.map(|w| (w.workspace(), w.org(), w.principal(), w.kind()));
    assert_eq!(seen, Ok((W2, O1, RUNTIME, PrincipalKind::Agent)));
}

/// The oracle: whether `permission` is in the list the registration was drawn as, by a linear scan
/// of that list, not of the context.
fn listed(drawn: &[(P, bool)], permission: P) -> bool {
    drawn.iter().any(|&(p, on)| on && p == permission)
}

#[test]
#[ignore = "pending E9-8"]
fn require_succeeds_exactly_when_the_registration_lists_the_permission() {
    let on = proptest::collection::vec(any::<bool>(), POOL.len());
    let draws = (
        on,
        any::<bool>(),
        any::<u128>(),
        any::<u128>(),
        any::<u128>(),
    );
    let config = Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&draws, |(on, agent, org, workspace, workload)| {
        let drawn: Vec<(P, bool)> = POOL.iter().copied().zip(on).collect();
        let chosen: Vec<P> = drawn
            .iter()
            .filter(|(_, on)| *on)
            .map(|(p, _)| *p)
            .collect();
        let actor = [SystemActor::System, SystemActor::Agent][usize::from(agent)];
        let (org, workspace, workload) =
            (OrgId(org), WorkspaceId(workspace), PrincipalId(workload));
        let context = system(org, workspace, workload, actor, &chosen);
        for permission in POOL {
            prop_assert_eq!(context.grants(permission), listed(&drawn, permission));
        }
        let expected = if listed(&drawn, P::ReadRecords) {
            Ok((workspace, org, workload))
        } else {
            Err(Refusal::Forbidden)
        };
        let witness = context.require::<ReadRecords>();
        prop_assert_eq!(
            witness.map(|w| (w.workspace(), w.org(), w.principal())),
            expected
        );
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
}
