//! The live start's gate on the meters (DEC-2257 item 2, DEC-2258, E21-62): the service starts only
//! when the workspace's and the deployment's meter streams both read back now, and names the first
//! one that does not.

use std::rc::Rc;

use mandate_gateway::meter::{MemoryMeter, MeterRecord, MeterStore, StoreError};
use mandate_gateway::provider::SequentialIds;
use mandate_gatewayd::start::{StartError, meters_ready};
use mandate_journal::Environment;
use mandate_store::MemoryStore;
use mandate_store::meter::JournalMeter;
use mandate_time::UtcNanos;

/// A meter stream that cannot be read: an outage, or a writer fenced or never opened.
struct Unreadable;

impl MeterStore for Unreadable {
    fn read(&self) -> Result<Vec<MeterRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    fn append(&mut self, _record: MeterRecord) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
}

fn journaled(workspace: &str) -> JournalMeter<MemoryStore> {
    JournalMeter::open(
        MemoryStore::default(),
        workspace,
        Environment::Paper,
        Rc::new(|| UtcNanos::from_parts(1_792_159_200, 0).unwrap()),
        Box::new(SequentialIds::starting_at(1 << 70)),
    )
}

/// DEC-2257 item 2: both meters reading back starts the service, the journaled one as well as any.
#[test]
fn both_meters_reading_back_starts_the_service() {
    assert_eq!(
        meters_ready(&journaled("ws_1"), &MemoryMeter::new()),
        Ok(())
    );
    assert_eq!(
        meters_ready(&MemoryMeter::new(), &MemoryMeter::new()),
        Ok(())
    );
}

/// DEC-2257 item 2: a meter that cannot be read stops the start, and the code names which one, the
/// workspace's when neither reads.
#[test]
fn a_meter_that_cannot_be_read_stops_the_start_and_is_named() {
    assert_eq!(
        meters_ready(&Unreadable, &MemoryMeter::new()),
        Err(StartError::WorkspaceMeter)
    );
    assert_eq!(
        meters_ready(&MemoryMeter::new(), &Unreadable),
        Err(StartError::DeploymentMeter)
    );
    assert_eq!(
        meters_ready(&Unreadable, &Unreadable),
        Err(StartError::WorkspaceMeter)
    );
    assert_eq!(
        meters_ready(&journaled("no such workspace!"), &MemoryMeter::new()),
        Err(StartError::WorkspaceMeter),
        "a journaled meter whose stream cannot be opened"
    );
    assert_eq!(
        [StartError::WorkspaceMeter, StartError::DeploymentMeter].map(StartError::code),
        [
            "workspace_meter_unavailable",
            "deployment_meter_unavailable"
        ]
    );
}
