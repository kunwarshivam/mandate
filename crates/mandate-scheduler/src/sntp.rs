//! SNTP (RFC 4330): one request to one time server over UDP, judged against the host clock the
//! caller injects. The only network call the scheduler makes; it carries no credential.

use std::net::{ToSocketAddrs, UdpSocket};
use std::time::Duration;

use mandate_time::UtcNanos;

use crate::measure::Sample;
use crate::scheduler::TimeServers;

/// How long one server may take to answer.
pub const TIMEOUT: Duration = Duration::from_secs(1);

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

impl<C: Fn() -> UtcNanos> TimeServers for Servers<C> {
    fn ask(&mut self) -> Vec<Sample> {
        todo!()
    }
}

/// One server's answer, or `None` when it did not answer in [`TIMEOUT`], answered something that
/// is not a server reply to this request, or flagged its clock unsynchronized.
pub fn query(server: &str, now: &dyn Fn() -> UtcNanos) -> Option<Sample> {
    todo!()
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
    todo!()
}

fn nanos(at: UtcNanos) -> i128 {

    todo!()

}

/// An NTP timestamp (seconds since 1900 and a 32-bit fraction) as nanoseconds since 1970.
fn from_ntp(stamp: &[u8]) -> Option<i128> {
    todo!()
}

fn to_ntp(at: UtcNanos) -> Option<[u8; 8]> {
    let secs = u32::try_from(i128::from(at.secs()).checked_add(UNIX_FROM_NTP)?).ok()?;
    let fraction = u32::try_from((i128::from(at.nanos()) << 32) / NANOS_PER_SEC).ok()?;
    let mut stamp = [0u8; 8];
    stamp[0..4].copy_from_slice(&secs.to_be_bytes());
    stamp[4..8].copy_from_slice(&fraction.to_be_bytes());
    Some(stamp)
}
