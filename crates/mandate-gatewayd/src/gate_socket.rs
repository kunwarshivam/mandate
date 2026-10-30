//! The gate socket (DEC-2294): `mandate-gatewayd gate <model_id> --probe <socket>` asks the running
//! service, over a Unix socket beside its probe socket, to run the chat gate on one chat entry of
//! its live registry file, and prints the one-line answer.
//!
//! The socket is the probe socket's path with [`SUFFIX`] added, in the same pod-local directory,
//! readable and writable by its owner only, so only `kubectl exec` with the namespace's exec rights
//! reaches it, as it reaches the probe socket. The probe socket itself keeps answering `livez` and
//! `readyz` alone, under its one-second bound (DEC-994 item 5), which a gate of minutes could not
//! keep.
//!
//! Each request is one line, `gate <model_id>`, and each answer one line of codes and a reference,
//! never a byte of a prompt, response, tool result or report:
//! - `refused <code>`: the gate did not start and spent nothing ([`GateRefusal`]);
//! - `passed <reference>`: the report the store holds, which passed;
//! - `stopped <code> <reference>`: the report the store holds, which did not pass;
//! - `unwritten <code>`: the store refused the report, so no reference exists for the file to name
//!   and the gate cannot pass; the code is the stop, or `passed`.
//!
//! One gate runs at a time: a request while one runs is answered `refused gate_running` at once.
//! The gate runs on the thread that holds the gateway ([`crate::serve::Side`]), so the operator's
//! disconnect does not stop it, and every other call waits for it.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use mandate_gateway::Gateway;
use mandate_gateway::meter::MeterStore;
use mandate_gateway::provider::{Clock, IdSource, Transport};
use mandate_journal::ArtifactRef;

use crate::gate::{GateEnd, GateRefusal, GateRun, Reports, built_in, run_gate};
use crate::live::ChatLive;
use crate::serve::Side;

/// What the gate socket's path adds to the probe socket's.
pub const SUFFIX: &str = ".gate";
/// The longest request line read, in one read.
const LINE_BOUND: usize = 256;
/// How long the socket waits for a request line, and for its answer to be taken.
const SILENCE_BOUND: Duration = Duration::from_secs(5);
/// How long a failed accept waits before the next, so a persistent error does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// The gate socket beside the probe socket at `probe`.
pub fn path(probe: &Path) -> PathBuf {
    let mut name = probe.as_os_str().to_owned();
    name.push(SUFFIX);
    PathBuf::from(name)
}

/// Binds the gate socket at `path` and answers each connection's one line through `side`, one gate
/// at a time, on threads of their own until the process exits. A socket left at `path` by an
/// earlier process is replaced; any other file there is refused.
///
/// # Errors
/// `AlreadyExists` for a file at `path` that is not a socket, or the bind's or the listener
/// thread's own.
pub fn serve(path: &Path, side: Side) -> io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "gate path is not a socket",
            ));
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let running = Arc::new(AtomicBool::new(false));
    thread::Builder::new().spawn(move || {
        for accepted in listener.incoming() {
            let Ok(stream) = accepted else {
                thread::sleep(ACCEPT_BACKOFF);
                continue;
            };
            let (side, running) = (side.clone(), Arc::clone(&running));
            let _unanswered_when_no_thread_starts =
                thread::Builder::new().spawn(move || respond(&stream, &side, &running));
        }
    })?;
    Ok(())
}

fn respond(stream: &UnixStream, side: &Side, running: &AtomicBool) {
    let answer = match read_line(stream) {
        Ok(line) if parse(&line).is_some() => {
            if running
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let answer = side
                    .ask(&line)
                    .unwrap_or_else(|| refused(GateRefusal::Stopping));
                running.store(false, Ordering::SeqCst);
                answer
            } else {
                refused(GateRefusal::Running)
            }
        }
        Ok(_) | Err(_) => refused(GateRefusal::Malformed),
    };
    let _a_dropped_operator_stops_nothing = stream
        .set_write_timeout(Some(SILENCE_BOUND))
        .and_then(|()| (&mut &*stream).write_all(format!("{answer}\n").as_bytes()));
}

/// Asks the gate socket at `path` to gate `model_id`, and waits up to `wait` for the answer line.
///
/// # Errors
/// The connection's own, or a socket silent past `wait`.
pub fn ask(path: &Path, model_id: &str, wait: Duration) -> io::Result<String> {
    let stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(SILENCE_BOUND))?;
    stream.set_read_timeout(Some(wait))?;
    (&mut &stream).write_all(format!("gate {model_id}\n").as_bytes())?;
    let mut answer = String::new();
    (&mut &stream).read_to_string(&mut answer)?;
    Ok(answer.trim_end().to_owned())
}

fn read_line(stream: &UnixStream) -> io::Result<String> {
    stream.set_read_timeout(Some(SILENCE_BOUND))?;
    let mut buffer = [0u8; LINE_BOUND];
    let read = (&mut &*stream).read(&mut buffer)?;
    Ok(buffer
        .get(..read)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default()
        .trim_end()
        .to_owned())
}

/// The model id a request line names: exactly `gate` and one id of the registry's characters.
pub fn parse(line: &str) -> Option<&str> {
    let mut words = line.split(' ');
    let (Some("gate"), Some(id), None) = (words.next(), words.next(), words.next()) else {
        return None;
    };
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_');
    (!id.is_empty() && id.chars().all(allowed)).then_some(id)
}

/// The answer line for a gate refused.
pub fn refused(refusal: GateRefusal) -> String {
    format!("refused {}", refusal.code())
}

/// The answer line for a gate that ended.
pub fn ended(end: &GateEnd) -> String {
    match (&end.report, end.stopped) {
        (Some(report), None) => format!("passed {report}"),
        (Some(report), Some(stopped)) => format!("stopped {} {report}", stopped.code()),
        (None, None) => "unwritten passed".to_owned(),
        (None, Some(stopped)) => format!("unwritten {}", stopped.code()),
    }
}

/// What answers gate lines on the thread that holds the gateway: the file's chat entries, the
/// reports this service has written, and the ids its gate calls draw from.
pub struct Desk {
    workspace: String,
    chats: Vec<ChatLive>,
    written: Vec<ArtifactRef>,
    ids: Box<dyn IdSource>,
}

impl Desk {
    pub fn new(workspace: &str, chats: Vec<ChatLive>, ids: Box<dyn IdSource>) -> Self {
        Self {
            workspace: workspace.to_owned(),
            chats,
            written: Vec::new(),
            ids,
        }
    }

    /// The reports this service has written, oldest first.
    pub fn written(&self) -> &[ArtifactRef] {
        &self.written
    }

    /// The answer to `line` through `gateway`, the gate's report written to `reports`.
    pub fn answer<T: Transport, S: MeterStore, C: Clock>(
        &mut self,
        line: &str,
        gateway: &mut Gateway<T, S, C>,
        reports: Option<&mut dyn Reports>,
    ) -> String {
        let Some(model_id) = parse(line) else {
            return refused(GateRefusal::Malformed);
        };
        let Some(chat) = self.chats.iter().find(|c| c.entry.model_id == model_id) else {
            return refused(GateRefusal::UnknownEntry);
        };
        let Some((set, digest)) = built_in() else {
            return refused(GateRefusal::ProbeSetUnreadable);
        };
        let ran = run_gate(
            gateway,
            GateRun {
                workspace: &self.workspace,
                chat,
                reports,
                earlier: &self.written,
                ids: self.ids.as_mut(),
                set: &set,
                digest,
            },
        );
        match ran {
            Ok(end) => {
                self.written.extend(end.report);
                ended(&end)
            }
            Err(refusal) => refused(refusal),
        }
    }
}
