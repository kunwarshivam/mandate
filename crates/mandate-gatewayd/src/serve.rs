//! The service's serve loop (DEC-2265 items 1, 2, 4 and 9, E21-62): one thread per connection reads
//! one request frame under the socket timeouts, hands a request that decoded whole to the one thread
//! that holds the gateway, and writes its reply. A peer that stalls is cut off at the timeout, a
//! connection past the cap is closed unread, and the log carries only codes, never a byte of a
//! frame, prompt, response, input or output.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use mandate_gateway::CallRequest;
use mandate_gateway::frame::{
    FrameError, MAX_REPLY_FRAME, MAX_REQUEST_FRAME, read_frame, write_frame,
};
use mandate_gateway::wire::{PutReply, PutRequest, Reply, reply_bytes, request_from};

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

/// One decoded request waiting for the gateway's reply.
struct Job {
    request: CallRequest,
    reply: Sender<Reply>,
}

impl Server {
    pub fn new(listener: TcpListener, limits: Limits, log: Arc<dyn Log>) -> Self {
        Self {
            listener,
            limits,
            log,
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The handle that stops this server.
    ///
    /// # Errors
    /// The listener's own failure to name its address.
    pub fn stopper(&self) -> std::io::Result<Stopper> {
        Ok(Stopper {
            stopping: Arc::clone(&self.stopping),
            address: self.listener.local_addr()?,
        })
    }

    /// Serves until stopped, answering each request that decoded whole with `answer`, one at a
    /// time and on this thread, so the gateway it holds never crosses a thread. If `answer`
    /// panics, the listener is stopped as the panic unwinds, so no connection is accepted that
    /// nothing would answer.
    pub fn run(self, mut answer: impl FnMut(CallRequest) -> Reply) {
        let (jobs, queue) = mpsc::channel::<Job>();
        let Self {
            listener,
            limits,
            log,
            stopping,
        } = self;
        let _stop_on_unwind = listener.local_addr().ok().map(|address| {
            StopOnDrop(Stopper {
                stopping: Arc::clone(&stopping),
                address,
            })
        });
        let acceptor = thread::spawn(move || accept(&listener, limits, &log, &stopping, &jobs));
        answer_all(&queue, &mut answer);
        let _ = acceptor.join();
    }

    /// As [`Server::run`], and answers each put that decoded whole with `store`, on the same
    /// thread (DEC-2279 items 1 and 4).
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn run_storing(
        self,
        answer: impl FnMut(CallRequest) -> Reply,
        store: impl FnMut(&PutRequest) -> PutReply,
    ) {
        let _ = (self, answer, store);
        todo!()
    }
}

impl Stopper {
    /// Stops the server, waking its listener with one connection of its own.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
    }
}

/// Stops its server when dropped: at the end of [`Server::run`], or as a panic in `answer`
/// unwinds it.
struct StopOnDrop(Stopper);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

fn answer_all(queue: &Receiver<Job>, answer: &mut impl FnMut(CallRequest) -> Reply) {
    for job in queue {
        let _ = job.reply.send(answer(job.request));
    }
}

fn accept(
    listener: &TcpListener,
    limits: Limits,
    log: &Arc<dyn Log>,
    stopping: &AtomicBool,
    jobs: &Sender<Job>,
) {
    let open = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(connection) = connection else {
            log.line("connection failed: accept");
            continue;
        };
        let Some(slot) = Slot::take(&open, limits.max_connections) else {
            log.line("connection refused: at_capacity");
            drop(connection);
            continue;
        };
        let log = Arc::clone(log);
        let jobs = jobs.clone();
        thread::spawn(move || {
            let _slot = slot;
            exchange(connection, limits.io_timeout, log.as_ref(), &jobs);
        });
    }
}

/// One of the open connections the cap counts, given back when its thread ends.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(open: &Arc<AtomicUsize>, cap: usize) -> Option<Self> {
        open.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            (count < cap).then(|| count.saturating_add(1))
        })
        .ok()?;
        Some(Self(Arc::clone(open)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn exchange(mut connection: TcpStream, timeout: Duration, log: &dyn Log, jobs: &Sender<Job>) {
    let timeouts = connection
        .set_read_timeout(Some(timeout))
        .and_then(|()| connection.set_write_timeout(Some(timeout)));
    if timeouts.is_err() {
        log.line("connection failed: timeouts");
        return;
    }
    let request = match read_frame(&mut connection, MAX_REQUEST_FRAME)
        .and_then(|body| request_from(&body).map_err(FrameError::Wire))
    {
        Ok(request) => request,
        Err(error) => {
            log.line(&format!("request refused: {}", frame_code(&error)));
            return;
        }
    };
    let (reply, answered) = mpsc::channel();
    if jobs.send(Job { request, reply }).is_err() {
        log.line("request dropped: stopping");
        return;
    }
    let Ok(reply) = answered.recv() else {
        log.line("request dropped: stopping");
        return;
    };
    let answered = match &reply {
        Ok(record) => format!("call answered: {}", record.outcome.code()),
        Err(refusal) => format!("call refused: {}", refusal.code()),
    };
    match write_frame(&mut connection, &reply_bytes(&reply), MAX_REPLY_FRAME) {
        Ok(()) => log.line(&answered),
        Err(error) => log.line(&format!("reply lost: {}", frame_code(&error))),
    }
}

/// A frame error's code, never its text's detail.
fn frame_code(error: &FrameError) -> &'static str {
    match error {
        FrameError::TooLong => "too_long",
        FrameError::Truncated => "truncated",
        FrameError::Wire(_) => "malformed",
        FrameError::TimedOut => "timed_out",
        FrameError::Io(_) => "io",
    }
}
