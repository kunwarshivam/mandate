//! SNTP (RFC 4330): one request to one time server over UDP, judged against the host clock the
//! caller injects. The only network call the scheduler makes; it carries no credential.

use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mandate_time::UtcNanos;

use crate::measure::Sample;
use crate::scheduler::TimeServers;

/// How long one server may take to answer.
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// The NTP port, used when a server's name carries none.
pub const PORT: u16 = 123;
const PACKET: usize = 48;
/// Leap indicator 0, version 4, mode 3 (client).
const REQUEST: u8 = 0b0010_0011;
/// Seconds from 1900-01-01, NTP's era 0, to 1970-01-01.
const UNIX_FROM_NTP: i128 = 2_208_988_800;
const NANOS_PER_SEC: i128 = 1_000_000_000;

/// The configured servers, each asked once per measurement, with the host clock `now`.
pub struct Servers<C> {
    pub names: Vec<String>,
    pub now: C,
}

/// How long a measurement waits for its servers: one [`TIMEOUT`] and a margin for the name lookup
/// and the threads to start. A server still resolving its name or waiting for an answer then is
/// left out, and its thread ends on its own; the loop is never held longer.
pub const DEADLINE: Duration = Duration::from_millis(1_500);

/// One server's answer, or the deadline's word that the measurement is over.
enum Heard {
    Answer(Option<Sample>),
    Deadline,
}

impl<C: Fn() -> UtcNanos + Clone + Send + 'static> TimeServers for Servers<C> {
    /// Asks every server at once, each on a thread of its own, and takes the answers that arrive
    /// before [`DEADLINE`], which a timer thread marks without reading a clock: so a measurement
    /// takes at most the deadline however many servers are silent or slow to resolve.
    fn ask(&mut self) -> Vec<Sample> {
        let (heard, hearing) = mpsc::channel();
        for name in &self.names {
            let (name, now, heard) = (name.clone(), self.now.clone(), heard.clone());
            thread::spawn(move || heard.send(Heard::Answer(query(&name, &now))));
        }
        let deadline = heard.clone();
        thread::spawn(move || {
            thread::sleep(DEADLINE);
            deadline.send(Heard::Deadline)
        });
        drop(heard);
        let mut samples = Vec::new();
        let mut waiting = self.names.len();
        while waiting > 0 {
            match hearing.recv() {
                Ok(Heard::Answer(answer)) => {
                    waiting = waiting.saturating_sub(1);
                    samples.extend(answer);
                }
                Ok(Heard::Deadline) | Err(_) => break,
            }
        }
        samples
    }
}

/// One server's answer, asked at `server` (a host, or `host:port`), or `None` when it did not answer in [`TIMEOUT`], answered something that
/// is not a server reply to this request, or flagged its clock unsynchronized.
pub fn query(server: &str, now: &dyn Fn() -> UtcNanos) -> Option<Sample> {
    let address = if server.contains(':') {
        server.to_socket_addrs()
    } else {
        (server, PORT).to_socket_addrs()
    }
    .ok()?
    .next()?;
    let socket = UdpSocket::bind(if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .ok()?;
    socket.set_read_timeout(Some(TIMEOUT)).ok()?;
    socket.connect(address).ok()?;
    let sent = now();
    let request = request(sent);
    socket.send(&request).ok()?;
    let mut reply = [0u8; PACKET];
    let read = socket.recv(&mut reply).ok()?;
    let received = now();
    (read == PACKET)
        .then(|| judge(server, &request, &reply, sent, received))
        .flatten()
}

/// The request: the client's transmit time, which the server echoes as its originate time.
pub fn request(sent: UtcNanos) -> [u8; PACKET] {
    let mut packet = [0u8; PACKET];
    packet[0] = REQUEST;
    if let Some(stamp) = to_ntp(sent) {
        packet[40..48].copy_from_slice(&stamp);
    }
    packet
}

/// The sample `reply` gives, sent at `sent` and received at `received` by the host clock: the
/// host clock minus the server's, `((t1 - t2) + (t4 - t3)) / 2`, and the round trip
/// `(t4 - t1) - (t3 - t2)`. `None` for a reply that is not a server's (mode 4) answer to this
/// request (its originate time is the request's transmit time), whose leap indicator says it is
/// unsynchronized, or whose times cannot be read.
pub fn judge(
    server: &str,
    request: &[u8; PACKET],
    reply: &[u8; PACKET],
    sent: UtcNanos,
    received: UtcNanos,
) -> Option<Sample> {
    let first = *reply.first()?;
    let unsynchronized = first >> 6 == 3;
    if first & 0b111 != 4 || unsynchronized || reply.get(24..32)? != request.get(40..48)? {
        return None;
    }
    let t1 = nanos(sent);
    let t2 = from_ntp(reply.get(32..40)?)?;
    let t3 = from_ntp(reply.get(40..48)?)?;
    let t4 = nanos(received);
    let offset = t1
        .checked_sub(t2)?
        .checked_add(t4.checked_sub(t3)?)?
        .checked_div(2)?;
    let delay = t4.checked_sub(t1)?.checked_sub(t3.checked_sub(t2)?)?;
    Some(Sample {
        server: server.to_owned(),
        stratum: *reply.get(1)?,
        offset_ns: i64::try_from(offset).ok()?,
        delay_ns: i64::try_from(delay).ok()?,
    })
}

fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(NANOS_PER_SEC)
        .saturating_add(i128::from(at.nanos()))
}

/// An NTP timestamp (seconds since 1900 and a 32-bit fraction) as nanoseconds since 1970.
fn from_ntp(stamp: &[u8]) -> Option<i128> {
    let secs = u32::from_be_bytes(stamp.get(0..4)?.try_into().ok()?);
    let fraction = u32::from_be_bytes(stamp.get(4..8)?.try_into().ok()?);
    if secs == 0 && fraction == 0 {
        return None;
    }
    let whole = i128::from(secs)
        .checked_sub(UNIX_FROM_NTP)?
        .checked_mul(NANOS_PER_SEC)?;
    let part = i128::from(fraction).checked_mul(NANOS_PER_SEC)? >> 32;
    whole.checked_add(part)
}

fn to_ntp(at: UtcNanos) -> Option<[u8; 8]> {
    let secs = u32::try_from(i128::from(at.secs()).checked_add(UNIX_FROM_NTP)?).ok()?;
    let fraction = u32::try_from((i128::from(at.nanos()) << 32) / NANOS_PER_SEC).ok()?;
    let mut stamp = [0u8; 8];
    stamp[0..4].copy_from_slice(&secs.to_be_bytes());
    stamp[4..8].copy_from_slice(&fraction.to_be_bytes());
    Some(stamp)
}
