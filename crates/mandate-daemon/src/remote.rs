//! The runtime's side of the model-gateway service (DEC-2397 items 1 and 2, DEC-2265 items 1 to 4):
//! one TCP connection per call, one request frame out and one reply frame back. The connect and
//! every write are bounded by the time left to the request's absolute deadline, so a call already
//! past it is never sent; every read is bounded by the time left to the deadline plus [`GRACE`],
//! so a reply the service finishes at its own deadline is still read, and one trickled a byte at a
//! time still ends. Any exchange that does not bring back the whole reply, for this call, by then is
//! [`RequestError::GatewayUnreachable`], and it is never sent again. The service's name is
//! resolved for each call, so a name that does not resolve yet, or a service that moved, costs that
//! call alone, never the runtime's start (DEC-2397 item 6).
//!
//! The same service stores the runtime's research artifacts in the research store (DEC-2279):
//! [`RemoteSink`] sends each one's bytes as a put, and counts it stored only when the service
//! answers the reference of those bytes and the runtime then reads the object back whole, itself,
//! through its own read-only mount of that store (item 3). Anything else is a put that stored
//! nothing, and research halts (DEC-2208 items 5 and 6).

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::num::{NonZeroU16, NonZeroU64};
use std::path::PathBuf;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_gateway::frame;
use mandate_gateway::wire::PutRequest;
use mandate_gateway::{CallRecord, CallRequest, RequestError};
use mandate_journal::{ArtifactRef, get_artifact};
use mandate_research_run::ModelCalls;
use mandate_time::UtcNanos;

use crate::runtime::ArtifactSink;

/// How long past the request's deadline the reply may still arrive: the service holds the call to
/// the same deadline (inference spec §3.4), so this covers the frame's crossing and the two
/// clocks' skew, never a longer call (DEC-2397 item 1).
pub const GRACE: Duration = Duration::from_secs(2);

/// How long one put may take, connect to reply, before it counts as storing nothing (DEC-2279
/// item 3): a put is one local write, of an artifact at most half the request frame's bound, since
/// its bytes cross as hex.
pub const PUT_TIMEOUT: Duration = Duration::from_secs(30);

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

/// The model-gateway service at one address, read against one clock, each put bounded by
/// `put_timeout`.
#[derive(Debug, Clone)]
pub struct RemoteGateway {
    address: GatewayAddress,
    clock: fn() -> UtcNanos,
    put_timeout: Duration,
}

impl RemoteGateway {
    /// The service at `address`, its calls built and bounded at `clock()`, each put within
    /// [`PUT_TIMEOUT`].
    pub fn new(address: impl Into<GatewayAddress>, clock: fn() -> UtcNanos) -> Self {
        Self {
            address: address.into(),
            clock,
            put_timeout: PUT_TIMEOUT,
        }
    }

    /// As this service, each put within `timeout` instead.
    #[must_use]
    pub fn with_put_timeout(self, timeout: Duration) -> Self {
        Self {
            put_timeout: timeout,
            ..self
        }
    }
}

impl RemoteGateway {
    /// Sends `bytes` as `workspace`'s put and gives back the reference the service answered, or
    /// `None` when it refused, or the exchange did not bring back the whole reply within its put
    /// timeout, connect included (DEC-2279 items 1 and 3). The answer is the service's claim only:
    /// [`RemoteSink`] checks it.
    pub fn put(&self, workspace: &str, bytes: &[u8]) -> Option<Digest> {
        let address = self.address.resolve()?;
        let deadline = after((self.clock)(), self.put_timeout)?;
        let left = remaining((self.clock)(), deadline)?;
        let stream = TcpStream::connect_timeout(&address, left).ok()?;
        let mut bounded = Bounded {
            stream,
            clock: self.clock,
            deadline,
            grace: Duration::ZERO,
        };
        let put = PutRequest {
            workspace_id: workspace.to_owned(),
            bytes: bytes.to_vec(),
        };
        frame::put(&mut bounded, &put).ok()?.ok()
    }
}

/// The research runtime's artifact sink in a cell, where its own mounts are read-only (DEC-2279
/// item 3): each put goes to the model-gateway service, and counts as stored only when the service
/// answers the reference of the bytes sent and the runtime's own checked read of that reference
/// from the research store at `research` gives them back. Reads are the runtime's own, from that
/// store, which is opened at each read and never at start (item 5): a store not there or not laid
/// out reads nothing, so research halts and the runtime still starts.
pub struct RemoteSink {
    gateway: RemoteGateway,
    workspace: String,
    research: PathBuf,
}

impl RemoteSink {
    /// `workspace`'s puts through `gateway`, read back from the research store at `research`.
    pub fn new(gateway: RemoteGateway, workspace: &str, research: impl Into<PathBuf>) -> Self {
        Self {
            gateway,
            workspace: workspace.to_owned(),
            research: research.into(),
        }
    }

    /// The checked read of `reference` from the research store, opened for this read.
    fn read(&self, reference: &ArtifactRef) -> Option<Vec<u8>> {
        let store = FsArtifactStore::open_existing(&self.research).ok()?;
        get_artifact(&store, reference).ok()
    }
}

impl ArtifactSink for RemoteSink {
    fn put(&mut self, bytes: &[u8]) -> bool {
        let reference = ArtifactRef::of(bytes);
        self.gateway.put(&self.workspace, bytes) == Some(reference.digest())
            && self.read(&reference).is_some_and(|read| read == bytes)
    }

    /// The research store's checked read: bytes that do not hash to `digest` are none.
    fn get(&self, digest: &Digest) -> Option<Vec<u8>> {
        self.read(&ArtifactRef::from_digest(*digest))
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

/// `now + span`, `None` past the clock's range.
fn after(now: UtcNanos, span: Duration) -> Option<UtcNanos> {
    let at = total_nanos(now).checked_add(i128::try_from(span.as_nanos()).ok()?)?;
    UtcNanos::from_parts(
        i64::try_from(at.div_euclid(1_000_000_000)).ok()?,
        u32::try_from(at.rem_euclid(1_000_000_000)).ok()?,
    )
    .ok()
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
