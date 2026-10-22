//! The control stream through the journal store (DEC-2272 item 1): the service reads
//! `ctl:{workspace_id}` and only that stream, and a store that cannot read it gives no rows.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_canon::Digest;
use mandate_gatewayd::control::StoreControl;
use mandate_gatewayd::service::Control;
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_store::Store;
use mandate_time::UtcNanos;

/// A store that answers one stream's rows, or none, and keeps which streams were asked for.
#[derive(Clone, Default)]
struct Asked {
    rows: Option<Vec<StoredEvent>>,
    asked: Rc<RefCell<Vec<String>>>,
}

impl Store for Asked {
    fn take_ownership(&mut self, _stream: &StreamId) -> Option<u64> {
        None
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.asked.borrow_mut().push(stream.as_str().to_owned());
        self.rows.clone()
    }

    fn append(
        &mut self,
        _stream: &StreamId,
        _expected_head: u64,
        _epoch: u64,
        _recorded_at: UtcNanos,
        _drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}

fn row() -> StoredEvent {
    StoredEvent {
        stream_id: "ctl:ws_1".into(),
        seq: 1,
        event_id: "01J8Z3MC000000000000000001".into(),
        event_type: "AgentDeployed".into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(b"row"),
        body: b"{}".to_vec(),
    }
}

/// The control reads the workspace's own `ctl:` stream and gives its rows back as the store gave
/// them; a store that cannot read gives none; a workspace that names no stream gives no control.
#[test]
#[ignore = "pending E21-62"]
fn the_control_reads_the_workspace_s_own_stream() {
    let store = Asked {
        rows: Some(vec![row()]),
        ..Asked::default()
    };
    let control = StoreControl::new(store.clone(), "ws_1").unwrap();
    assert_eq!(control.rows(), Some(vec![row()]));
    assert_eq!(*store.asked.borrow(), ["ctl:ws_1"]);

    let down = StoreControl::new(Asked::default(), "ws_1").unwrap();
    assert_eq!(down.rows(), None);

    assert!(StoreControl::new(Asked::default(), "no such workspace!").is_none());
}
