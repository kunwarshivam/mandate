//! The wire form of one call between the runtime and the model-gateway service (DEC-2264 item 5):
//! a [`CallRequest`] one way, and its [`CallRecord`] or [`RequestError`] back, each as canonical
//! JSON. Decoding is the exact inverse of encoding: anything else reads as a [`WireError`].

use crate::call::{CallRequest, RequestError};
use crate::record::CallRecord;

/// The wire form's version, a member of every frame.
pub const WIRE_VERSION: u64 = 1;

/// Why a frame does not decode: the first member that is missing, unlisted, of the wrong kind, out
/// of range, or not in canonical form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("the frame is not canonical JSON")]
    NotCanonical,
    #[error("the frame's member `{0}` does not read")]
    Member(&'static str),
}

/// What the gateway answers one request with.
pub type Reply = Result<CallRecord, RequestError>;

/// `request`'s frame body.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn request_bytes(request: &CallRequest) -> Vec<u8> {
    let _ = request;
    todo!()
}

/// The request a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`request_bytes`] would not write.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn request_from(bytes: &[u8]) -> Result<CallRequest, WireError> {
    let _ = bytes;
    todo!()
}

/// `reply`'s frame body.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn reply_bytes(reply: &Reply) -> Vec<u8> {
    let _ = reply;
    todo!()
}

/// The reply a frame body holds.
///
/// # Errors
/// [`WireError`] for any body [`reply_bytes`] would not write.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn reply_from(bytes: &[u8]) -> Result<Reply, WireError> {
    let _ = bytes;
    todo!()
}
