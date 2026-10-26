//! The clock stream's event ids (lane DR's X9): the journal refuses an `event_id` another stream
//! already holds (journal spec §3), so two workspaces' schedulers in one journal must never derive
//! the same id, and a retry must derive the same one again (§5.1).

use mandate_canon::{is_ulid, stream_event_id};
use mandate_journal::Environment;
use mandate_scheduler::records::Writer;

fn writer(workspace: &str, epoch: u64) -> Writer {
    Writer {
        workspace: workspace.to_owned(),
        environment: Environment::Paper,
        epoch,
    }
}

#[test]
fn two_workspaces_clock_streams_never_share_an_id() {
    let (one, two) = (writer("ws1", 1), writer("ws2", 1));
    for head in 0..64 {
        for ordinal in 0..4 {
            assert_ne!(one.event_id(head, ordinal), two.event_id(head, ordinal));
        }
    }
}

#[test]
fn a_batch_s_ids_are_its_clock_stream_s_and_derive_again_the_same() {
    let clock = writer("ws1", 3);
    let id = clock.event_id(41, 2);
    assert_eq!(id, stream_event_id("clock:ws1", 3, 41, 2));
    assert_eq!(id, clock.event_id(41, 2));
    assert!(is_ulid(&id), "{id}");
}
