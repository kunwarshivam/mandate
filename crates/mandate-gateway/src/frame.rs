//! One exchange per call between the runtime and the model-gateway service (DEC-2265 items 1 to 3):
//! a 4-byte big-endian length, then that many bytes of the wire form, one request each way. The
//! framing is generic over `std::io`, so this crate opens no socket; each process hands in its own
//! connection, with its socket timeouts already set to the call's deadline (item 4).
//!
//! A length past its bound is refused before any of the body is read, and a frame cut short or
//! malformed ends the exchange with a [`FrameError`] and no call: [`serve`] answers only a request
//! that decoded whole, and writes nothing when it cannot write the whole reply.

use std::io::{ErrorKind, Read, Write};

use crate::call::CallRequest;
use crate::wire::{
    PutReply, PutRequest, Reply, WireError, reply_bytes, reply_from, request_bytes, request_from,
};

/// The longest request frame body: the cap `HttpsTransport` reads a provider's body under.
pub const MAX_REQUEST_FRAME: u32 = 4 * 1024 * 1024;

/// The longest reply frame body: room for the prompt and a whole provider body, both as hex.
pub const MAX_REPLY_FRAME: u32 = 24 * 1024 * 1024;

/// Why an exchange ended without a whole frame. Its text names only the code, never a byte of the
/// frame (DEC-2265 item 9).
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
pub fn write_frame<W: Write>(out: &mut W, body: &[u8], bound: u32) -> Result<(), FrameError> {
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| *length <= bound)
        .ok_or(FrameError::TooLong)?;
    let mut frame = length.to_be_bytes().to_vec();
    frame.extend_from_slice(body);
    out.write_all(&frame).map_err(failed)?;
    out.flush().map_err(failed)
}

/// Reads one frame's body, refusing a length past `bound` before reading any of it.
///
/// # Errors
/// [`FrameError::TooLong`] past `bound`, [`FrameError::Truncated`] when the connection closes
/// first, and the connection's own failure.
pub fn read_frame<R: Read>(input: &mut R, bound: u32) -> Result<Vec<u8>, FrameError> {
    let mut length = [0_u8; 4];
    input.read_exact(&mut length).map_err(failed)?;
    let length = u32::from_be_bytes(length);
    if length > bound {
        return Err(FrameError::TooLong);
    }
    let mut body = Vec::new();
    input
        .take(u64::from(length))
        .read_to_end(&mut body)
        .map_err(failed)?;
    if u64::try_from(body.len()) == Ok(u64::from(length)) {
        Ok(body)
    } else {
        Err(FrameError::Truncated)
    }
}

/// The runtime's side: writes `request` and reads its reply.
///
/// # Errors
/// Any [`FrameError`]. The caller treats every one as a lost reply and never retries the call
/// under the same `call_id` (DEC-2265 items 3 and 4).
pub fn call<S: Read + Write>(stream: &mut S, request: &CallRequest) -> Result<Reply, FrameError> {
    write_frame(stream, &request_bytes(request), MAX_REQUEST_FRAME)?;
    let body = read_frame(stream, MAX_REPLY_FRAME)?;
    reply_from(&body).map_err(FrameError::Wire)
}

/// The runtime's side of a put (DEC-2279 item 1): writes `put` and reads its reply. Its bytes cross
/// as hex, so a put whose frame passes [`MAX_REQUEST_FRAME`], an artifact of about half that, is
/// never sent.
///
/// # Errors
/// Any [`FrameError`]. The caller treats every one as a put that stored nothing (item 3).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn put<S: Read + Write>(stream: &mut S, put: &PutRequest) -> Result<PutReply, FrameError> {
    let _ = (stream, put);
    todo!()
}

/// The service's side: reads one request, hands it to `answer` only when it decoded whole, and
/// writes the reply.
///
/// # Errors
/// Any [`FrameError`]; `answer` has not run unless the error is the reply's own.
pub fn serve<S: Read + Write>(
    stream: &mut S,
    answer: impl FnOnce(CallRequest) -> Reply,
) -> Result<(), FrameError> {
    let body = read_frame(stream, MAX_REQUEST_FRAME)?;
    let request = request_from(&body).map_err(FrameError::Wire)?;
    write_frame(stream, &reply_bytes(&answer(request)), MAX_REPLY_FRAME)
}

/// The connection's failure as its code: a timeout, an end inside a frame, or the error's kind.
fn failed(error: std::io::Error) -> FrameError {
    match error.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => FrameError::TimedOut,
        ErrorKind::UnexpectedEof => FrameError::Truncated,
        kind => FrameError::Io(kind),
    }
}
