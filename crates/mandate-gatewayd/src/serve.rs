//! The service's serve loop (DEC-2265 items 1, 2, 4 and 9, E21-62): one thread per connection reads
//! one request frame under the socket timeouts, hands a request that decoded whole to the one thread
//! that holds the gateway, and writes its reply. A peer that stalls is cut off at the timeout, a
//! connection past the cap is closed unread, and the log carries only codes, never a byte of a
//! frame, prompt, response, input or output.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use mandate_gateway::CallRequest;
use mandate_gateway::wire::Reply;

/// The serve loop's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Each socket read and write gives up after this long, so a stalled peer holds its thread
    /// no longer.
    pub io_timeout: Duration,
    /// The most connections served at once; one past it is closed before any of it is read.
    pub max_connections: usize,
}

/// Where the service's log lines go. A line holds only codes.
pub trait Log: Send + Sync {
    fn line(&self, line: &str);
}

/// A listener and its bounds, served by [`Server::run`] until its [`Stopper`] stops it.
pub struct Server {
    listener: TcpListener,
    limits: Limits,
    log: Arc<dyn Log>,
    stopping: Arc<AtomicBool>,
}

/// Stops a running [`Server`]: no connection is accepted after it, and [`Server::run`] returns once
/// the connections already open have ended.
#[derive(Debug, Clone)]
pub struct Stopper {
    stopping: Arc<AtomicBool>,
    address: SocketAddr,
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl Server {
    pub fn new(listener: TcpListener, limits: Limits, log: Arc<dyn Log>) -> Self {
        let _ = (listener, limits, log);
        todo!()
    }

    /// The handle that stops this server.
    ///
    /// # Errors
    /// The listener's own failure to name its address.
    pub fn stopper(&self) -> std::io::Result<Stopper> {
        let _ = (&self.listener, self.limits, &self.log, &self.stopping);
        todo!()
    }

    /// Serves until stopped, answering each request that decoded whole with `answer`, one at a
    /// time and on this thread, so the gateway it holds never crosses a thread.
    pub fn run(self, answer: impl FnMut(CallRequest) -> Reply) {
        let _ = answer;
        todo!()
    }
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl Stopper {
    /// Stops the server, waking its listener with one connection of its own.
    pub fn stop(&self) {
        let _ = (&self.stopping, self.address);
        todo!()
    }
}
