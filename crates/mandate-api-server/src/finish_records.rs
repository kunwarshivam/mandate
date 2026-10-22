//! The records the connect's finish appends ([DEC-2342](../../../docs/project/decisions/DEC-2342.md)
//! item 9): `ConnectionEstablished` version 2 or `ConnectionRefused` version 1 on the workspace's
//! control stream, written by the connection manager as a `system` actor, under an event id derived
//! from the request so a retried append writes the same event.

use mandate_connections::manager::{ManagerEffect, RequestMembers};
use mandate_time::UtcNanos;

/// The finish's actor id.
pub const ACTOR_ID: &str = "connection_manager";

/// The operation the finish's event id is derived under.
pub const OPERATION: &str = "connect_finish";

/// The finish's event id for `request`: the ULID of the first 128 bits of the SHA-256 of the
/// canonical `{account_ref, connection_id, operation}`. An `account_ref` names one request only, so
/// each request has one.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn event_id(_request: &RequestMembers) -> Option<String> {
    todo!()
}

/// The draft of `effect` on `workspace`'s control stream at `now`, with `causation`; `None` for an
/// effect the finish never writes (`Requested`) or a value the canonical form cannot hold.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn drafted(
    _effect: &ManagerEffect,
    _causation: Option<&str>,
    _workspace: &str,
    _now: UtcNanos,
) -> Option<Vec<u8>> {
    todo!()
}
