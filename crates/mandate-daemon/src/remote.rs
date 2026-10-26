//! The runtime's side of the model-gateway service (DEC-2397 items 1 and 2, DEC-2265 items 1 to 4):
//! one TCP connection per call, one request frame out and one reply frame back. The connect and
//! every write are bounded by the time left to the request's absolute deadline, so a call already
//! past it is never sent; every read is bounded by the time left to the deadline plus [`GRACE`],
//! so a reply the service finishes at its own deadline is still read, and one trickled a byte at a
//! time still ends. Any exchange that does not bring back the whole reply, for this call, by then is
//! [`RequestError::GatewayUnreachable`], and it is never sent again.

use std::net::SocketAddr;
use std::time::Duration;

use mandate_gateway::{CallRecord, CallRequest, RequestError};
use mandate_research_run::ModelCalls;
use mandate_time::UtcNanos;

/// How long past the request's deadline the reply may still arrive: the service holds the call to
/// the same deadline (inference spec §3.4), so this covers the frame's crossing and the two
/// clocks' skew, never a longer call (DEC-2397 item 1).
pub const GRACE: Duration = Duration::from_secs(2);

/// The model-gateway service at one address, read against one clock.
#[derive(Debug, Clone, Copy)]
pub struct RemoteGateway {
    address: SocketAddr,
    clock: fn() -> UtcNanos,
}

impl RemoteGateway {
    /// The service at `address`, its calls built and bounded at `clock()`.
    pub fn new(address: SocketAddr, clock: fn() -> UtcNanos) -> Self {
        Self { address, clock }
    }
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl ModelCalls for RemoteGateway {
    fn now(&mut self) -> UtcNanos {
        let _ = (self.address, self.clock);
        todo!()
    }

    fn call(&mut self, request: &CallRequest) -> Result<CallRecord, RequestError> {
        let _ = request;
        todo!()
    }
}
