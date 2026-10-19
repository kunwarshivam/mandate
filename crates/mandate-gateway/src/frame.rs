//! One exchange per call between the runtime and the model-gateway service (DEC-2264 items 1 to 3):
//! a 4-byte big-endian length, then that many bytes of the wire form, one request each way. The
//! framing is generic over `std::io`, so this crate opens no socket; each process hands in its own
//! connection, with its socket timeouts already set to the call's deadline (item 4).
//!
//! A length past its bound is refused before any of the body is read, and a frame cut short or
//! malformed ends the exchange with a [`FrameError`] and no call: [`serve`] answers only a request
//! that decoded whole, and writes nothing when it cannot write the whole reply.

use std::io::{ErrorKind, Read, Write};

use crate::call::CallRequest;
use crate::wire::{Reply, WireError};

/// The longest request frame body: the cap `HttpsTransport` reads a provider's body under.
pub const MAX_REQUEST_FRAME: u32 = 4 * 1024 * 1024;

/// The longest reply frame body: room for the prompt and a whole provider body, both as hex.
pub const MAX_REPLY_FRAME: u32 = 24 * 1024 * 1024;

/// Why an exchange ended without a whole frame. Its text names only the code, never a byte of the
/// frame (DEC-2264 item 9).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("the frame's length passes its bound")]
    TooLong,
    #[error("the connection closed inside a frame")]
    Truncated,
    #[error("the frame does not decode: {0}")]
    Wire(WireError),
    #[error("the connection timed out")]
    TimedOut,
    #[error("the connection failed: {0:?}")]
    Io(ErrorKind),
}

/// Writes `body` as one frame, or nothing when it is longer than `bound`.
///
/// # Errors
/// [`FrameError::TooLong`] past `bound`, and the connection's own failure.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn write_frame<W: Write>(out: &mut W, body: &[u8], bound: u32) -> Result<(), FrameError> {
    let _ = (out, body, bound);
    todo!()
}

/// Reads one frame's body, refusing a length past `bound` before reading any of it.
///
/// # Errors
/// [`FrameError::TooLong`] past `bound`, [`FrameError::Truncated`] when the connection closes
/// first, and the connection's own failure.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn read_frame<R: Read>(input: &mut R, bound: u32) -> Result<Vec<u8>, FrameError> {
    let _ = (input, bound);
    todo!()
}

/// The runtime's side: writes `request` and reads its reply.
///
/// # Errors
/// Any [`FrameError`]. The caller treats every one as a lost reply and never retries the call
/// under the same `call_id` (DEC-2264 items 3 and 4).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn call<S: Read + Write>(stream: &mut S, request: &CallRequest) -> Result<Reply, FrameError> {
    let _ = (stream, request);
    todo!()
}

/// The service's side: reads one request, hands it to `answer` only when it decoded whole, and
/// writes the reply.
///
/// # Errors
/// Any [`FrameError`]; `answer` has not run unless the error is the reply's own.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn serve<S: Read + Write>(
    stream: &mut S,
    answer: impl FnOnce(CallRequest) -> Reply,
) -> Result<(), FrameError> {
    let _ = (stream, answer);
    todo!()
}
