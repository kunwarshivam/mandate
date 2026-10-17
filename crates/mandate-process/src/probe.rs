//! Liveness and readiness answers, and the Unix socket that serves them to `exec` probes
//! (design §3.4, DEC-994 item 5). Every answer is one of a few fixed words: no id, symbol, price,
//! quantity, or mandate content (`AGENTS.md` rule 6). This module reads no clock: the time a
//! probe is judged at comes from the caller.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use mandate_time::UtcNanos;

use crate::lifecycle::{Snapshot, State, nanos};

/// The longest request or reply read, in one read of at most this long, and how many connections
/// are answered at once; a connection over that is closed unanswered. A request or reply is one
/// read, never a loop of reads, so a peer that trickles bytes holds its own connection for at most
/// one silence bound, and never the others (DEC-994 item 5).
const LINE_BOUND: usize = 64;
const SILENCE_BOUND: Duration = Duration::from_secs(1);
const CONNECTION_BOUND: usize = 8;

/// Every word an answer can carry.
const WORDS: [&str; 5] = ["live", "stopped", "stalled", "ready", "not_ready"];
const NOT_FOUND: &str = "not_found";

/// The two probes of design §3.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Live,
    Ready,
}

impl Probe {
    /// The request word: `livez` or `readyz`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Live => "livez",
            Self::Ready => "readyz",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "livez" => Some(Self::Live),
            "readyz" => Some(Self::Ready),
            _ => None,
        }
    }
}

/// A probe's answer: whether it passed, and its one fixed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answer {
    pub ok: bool,
    pub word: &'static str,
}

impl Answer {
    /// The `exec` probe's exit status: 0 when it passed, 1 otherwise.
    pub fn exit_status(self) -> u8 {
        if self.ok { 0 } else { 1 }
    }
}

/// The answer to `probe` for `snapshot`, judged at `now`.
///
/// - live: `live` until the process exits (`stopped`) or `now` passes the snapshot's liveness
///   bound (`stalled`);
/// - ready: `ready` only while the state is `Ready` and the process is live, else `not_ready`.
pub fn answer(probe: Probe, snapshot: Snapshot, now: UtcNanos) -> Answer {
    let stalled = snapshot.live_until.is_some_and(|until| nanos(now) > until);
    let live = match snapshot.state {
        State::Exited(_) => Err("stopped"),
        _ if stalled => Err("stalled"),
        _ => Ok("live"),
    };
    let (ok, word) = match probe {
        Probe::Live => match live {
            Ok(word) => (true, word),
            Err(word) => (false, word),
        },
        Probe::Ready if live.is_ok() && snapshot.state == State::Ready => (true, "ready"),
        Probe::Ready => (false, "not_ready"),
    };
    Answer { ok, word }
}

/// The latest snapshot a process published, shared with the probe socket's thread.
#[derive(Debug, Clone)]
pub struct Status(Arc<Mutex<Snapshot>>);

impl Status {
    pub fn new(snapshot: Snapshot) -> Self {
        Self(Arc::new(Mutex::new(snapshot)))
    }

    pub fn publish(&self, snapshot: Snapshot) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
    }

    pub fn snapshot(&self) -> Snapshot {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Binds a Unix socket at `path` and answers each connection's one request line (`livez` or
/// `readyz`) from `status`, judged at `clock()`, on a thread of its own until the process exits.
/// The reply is one line, `ok <word>` or `fail <word>`; an unknown request is `fail not_found`.
/// A socket left at `path` by an earlier process is replaced; any other file there is refused.
/// The new socket is readable and writable by its owner only.
///
/// # Errors
/// `AlreadyExists` for a file at `path` that is not a socket, or the bind's own.
pub fn serve<C>(path: &Path, status: Status, clock: C) -> io::Result<()>
where
    C: Fn() -> UtcNanos + Send + Sync + 'static,
{
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => fs::remove_file(path)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "probe path is not a socket",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let clock = Arc::new(clock);
    let open = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if open.fetch_add(1, Ordering::SeqCst) >= CONNECTION_BOUND {
                open.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let (status, clock, open) = (status.clone(), Arc::clone(&clock), Arc::clone(&open));
            thread::spawn(move || {
                let _answered = respond(&stream, &status, &*clock);
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(())
}

fn respond<C: Fn() -> UtcNanos>(stream: &UnixStream, status: &Status, clock: &C) -> io::Result<()> {
    stream.set_read_timeout(Some(SILENCE_BOUND))?;
    stream.set_write_timeout(Some(SILENCE_BOUND))?;
    let line = read_once(stream)?;
    let reply = match Probe::parse(line.trim_end()) {
        Some(probe) => answer(probe, status.snapshot(), clock()),
        None => Answer {
            ok: false,
            word: NOT_FOUND,
        },
    };
    let verdict = if reply.ok { "ok" } else { "fail" };
    (&mut &*stream).write_all(format!("{verdict} {}\n", reply.word).as_bytes())
}

/// Asks the socket at `path` for `probe`: what a process's `exec` probe subcommand runs.
///
/// # Errors
/// The connection's own, a silent server, or a reply that is not one of the fixed answers
/// (`InvalidData`). A probe that cannot be asked counts as failed.
pub fn query(path: &Path, probe: Probe) -> io::Result<Answer> {
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(SILENCE_BOUND))?;
    stream.set_write_timeout(Some(SILENCE_BOUND))?;
    (&mut &stream).write_all(format!("{}\n", probe.word()).as_bytes())?;
    let line = read_once(&stream)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "unexpected probe reply");
    let (verdict, word) = line.trim_end().split_once(' ').ok_or_else(invalid)?;
    let word = WORDS
        .into_iter()
        .find(|known| *known == word)
        .ok_or_else(invalid)?;
    match verdict {
        "ok" => Ok(Answer { ok: true, word }),
        "fail" => Ok(Answer { ok: false, word }),
        _ => Err(invalid()),
    }
}

/// One read of at most [`LINE_BOUND`] bytes; anything that is not UTF-8 reads as empty.
fn read_once(stream: &UnixStream) -> io::Result<String> {
    let mut buffer = [0u8; LINE_BOUND];
    let read = (&mut &*stream).read(&mut buffer)?;
    Ok(buffer
        .get(..read)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default()
        .to_owned())
}
