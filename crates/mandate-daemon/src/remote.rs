//! The runtime's side of the model-gateway service (DEC-2397 items 1 and 2, DEC-2265 items 1 to 4):
//! one TCP connection per call, one request frame out and one reply frame back. The connect and
//! every write are bounded by the time left to the request's absolute deadline, so a call already
//! past it is never sent; every read is bounded by the time left to the deadline plus [`GRACE`],
//! so a reply the service finishes at its own deadline is still read, and one trickled a byte at a
//! time still ends. Any exchange that does not bring back the whole reply, for this call, by then is
//! [`RequestError::GatewayUnreachable`], and it is never sent again. The service's name is
//! resolved for each call, so a name that does not resolve yet, or a service that moved, costs that
//! call alone, never the runtime's start (DEC-2397 item 6).

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::num::{NonZeroU16, NonZeroU64};
use std::time::Duration;

use mandate_gateway::frame;
use mandate_gateway::{CallRecord, CallRequest, RequestError};
use mandate_research_run::ModelCalls;
use mandate_time::UtcNanos;

/// How long past the request's deadline the reply may still arrive: the service holds the call to
/// the same deadline (inference spec §3.4), so this covers the frame's crossing and the two
/// clocks' skew, never a longer call (DEC-2397 item 1).
pub const GRACE: Duration = Duration::from_secs(2);

/// The service's `host:port` as written: a name or an address, and a port other than 0. It is
/// only parsed, never resolved, so it names the service whether or not its name resolves yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayAddress {
    host: String,
    port: u16,
}

impl GatewayAddress {
    /// `host:port`, `None` when the host is empty or the port is not a number from 1 to 65535.
    pub fn parse(text: &str) -> Option<Self> {
        let (host, port) = text.rsplit_once(':')?;
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        (!host.is_empty()).then_some(())?;
        Some(Self {
            host: host.to_owned(),
            port: port.parse::<NonZeroU16>().ok()?.get(),
        })
    }

    /// The host as written, without the brackets of an IPv6 address.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The first address the name resolves to now, `None` when it resolves to none.
    fn resolve(&self) -> Option<SocketAddr> {
        (self.host.as_str(), self.port)
            .to_socket_addrs()
            .ok()?
            .next()
    }
}

impl From<SocketAddr> for GatewayAddress {
    fn from(address: SocketAddr) -> Self {
        Self {
            host: address.ip().to_string(),
            port: address.port(),
        }
    }
}

/// The model-gateway service at one address, read against one clock.
#[derive(Debug, Clone)]
pub struct RemoteGateway {
    address: GatewayAddress,
    clock: fn() -> UtcNanos,
}

impl RemoteGateway {
    /// The service at `address`, its calls built and bounded at `clock()`.
    pub fn new(address: impl Into<GatewayAddress>, clock: fn() -> UtcNanos) -> Self {
        Self {
            address: address.into(),
            clock,
        }
    }
}

impl ModelCalls for RemoteGateway {
    fn now(&mut self) -> UtcNanos {
        (self.clock)()
    }

    fn call(&mut self, request: &CallRequest) -> Result<CallRecord, RequestError> {
        let address = self
            .address
            .resolve()
            .ok_or(RequestError::GatewayUnreachable)?;
        let left =
            remaining((self.clock)(), request.deadline).ok_or(RequestError::GatewayUnreachable)?;
        let stream = TcpStream::connect_timeout(&address, left)
            .map_err(|_| RequestError::GatewayUnreachable)?;
        let mut bounded = Bounded {
            stream,
            clock: self.clock,
            deadline: request.deadline,
            grace: GRACE,
        };
        let reply =
            frame::call(&mut bounded, request).map_err(|_| RequestError::GatewayUnreachable)?;
        match reply {
            Ok(record) if record.call_id == request.call_id => Ok(record),
            Ok(_) => Err(RequestError::GatewayUnreachable),
            Err(refusal) => Err(refusal),
        }
    }
}

/// `deadline - now`, `None` when nothing is left.
fn remaining(now: UtcNanos, deadline: UtcNanos) -> Option<Duration> {
    let left = total_nanos(deadline).checked_sub(total_nanos(now))?;
    let left = NonZeroU64::new(u64::try_from(left).ok()?)?;
    Some(Duration::from_nanos(left.get()))
}

fn total_nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(1_000_000_000)
        .saturating_add(i128::from(at.nanos()))
}

/// A connection whose every write may take only the time left to `deadline`, and every read the
/// time left to `deadline` plus `grace`.
struct Bounded {
    stream: TcpStream,
    clock: fn() -> UtcNanos,
    deadline: UtcNanos,
    grace: Duration,
}

impl Bounded {
    fn write_bound(&self) -> io::Result<()> {
        let left = remaining((self.clock)(), self.deadline).ok_or_else(timed_out)?;
        self.stream.set_write_timeout(Some(left))
    }

    fn read_bound(&self) -> io::Result<()> {
        let now = (self.clock)();
        let left = match remaining(now, self.deadline) {
            Some(left) => left.checked_add(self.grace),
            None => remaining_past(now, self.deadline, self.grace),
        }
        .ok_or_else(timed_out)?;
        self.stream.set_read_timeout(Some(left))
    }
}

fn timed_out() -> io::Error {
    io::Error::from(io::ErrorKind::TimedOut)
}

/// What is left of `grace` at `now`, already past `deadline`, `None` once it is spent.
fn remaining_past(now: UtcNanos, deadline: UtcNanos, grace: Duration) -> Option<Duration> {
    let late = total_nanos(now).checked_sub(total_nanos(deadline))?;
    let late = Duration::from_nanos(u64::try_from(late).ok()?);
    grace.checked_sub(late).filter(|left| !left.is_zero())
}

impl Read for Bounded {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_bound()?;
        self.stream.read(buf)
    }
}

impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_bound()?;
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write_bound()?;
        self.stream.flush()
    }
}
