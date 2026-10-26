//! The fake orchestrator the manager's tests and drills rely on does what it says: a stop
//! removes only what it names, a lost node keeps its instance live until it vanishes, and refused
//! effects change nothing.

mod common;

use common::block_on;
use mandate_deploy_manager::fake::{Call, Fake};
use mandate_deploy_manager::{Orchestrator, Phase, Slot, Unavailable};

fn phases(fake: &Fake) -> Vec<(Slot, u64, Phase)> {
    fake.instances()
        .into_iter()
        .map(|i| (i.slot, i.generation, i.phase))
        .collect()
}

#[test]
fn readiness_crashes_lost_nodes_and_garbage_collection_move_only_their_slot() {
    let a = Slot::runtime("a");
    let b = Slot::runtime("b");
    let mut fake = Fake::new();
    block_on(fake.start(&a, 1)).unwrap();
    block_on(fake.start(&b, 1)).unwrap();
    fake.ready(&a, true);
    assert_eq!(
        phases(&fake),
        vec![
            (a.clone(), 1, Phase::Running { ready: true }),
            (b.clone(), 1, Phase::Running { ready: false })
        ]
    );
    fake.lose_node(&a);
    fake.lose_node(&b);
    fake.vanish(&a);
    assert_eq!(phases(&fake), vec![(b.clone(), 1, Phase::Unknown)]);
    assert_eq!(fake.live(&b), 1, "a lost node's instance may still run");
    block_on(fake.start(&a, 2)).unwrap();
    fake.vanish(&b);
    fake.vanish(&a);
    assert_eq!(
        phases(&fake),
        vec![(a.clone(), 2, Phase::Running { ready: false })]
    );
    fake.crash(&a);
    assert_eq!(fake.live(&a), 0);
    fake.collect();
    assert!(fake.instances().is_empty());
}

#[test]
fn a_stop_removes_its_running_generation_and_leaves_a_lost_one_live() {
    let a = Slot::executor("acct");
    let mut fake = Fake::new();
    block_on(fake.start(&a, 1)).unwrap();
    block_on(fake.stop(&a, 2)).unwrap();
    assert_eq!(fake.live(&a), 1, "another generation is untouched");
    block_on(fake.stop(&a, 1)).unwrap();
    assert_eq!(fake.live(&a), 0);
    block_on(fake.start(&a, 3)).unwrap();
    fake.lose_node(&a);
    block_on(fake.stop(&a, 3)).unwrap();
    assert_eq!(fake.live(&a), 1);
    assert_eq!(fake.calls().len(), 5);
    assert_eq!(
        fake.calls()[4],
        Call::Stop {
            slot: a,
            generation: 3
        }
    );
}

#[test]
fn refused_and_unavailable_effects_change_nothing() {
    let a = Slot::runtime("a");
    let mut fake = Fake::new();
    block_on(fake.start(&a, 1)).unwrap();
    fake.refuse_effects = true;
    assert_eq!(block_on(fake.stop(&a, 1)), Err(Unavailable));
    assert_eq!(block_on(fake.start(&a, 2)), Err(Unavailable));
    assert!(block_on(fake.observe()).is_ok(), "listing still works");
    fake.refuse_effects = false;
    fake.unavailable = true;
    assert_eq!(block_on(fake.stop(&a, 1)), Err(Unavailable));
    assert_eq!(block_on(fake.observe()), Err(Unavailable));
    assert_eq!(fake.live(&a), 1);
    assert_eq!(fake.calls().len(), 1);
}
