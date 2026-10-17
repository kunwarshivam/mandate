//! What the stub answers (DEC-2100 item 7): `GET /jwks.json` and `POST /token`, only for its own
//! `Host`, with no CORS header on any answer.

use std::future::Future;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::issuer::Issuer;

/// The largest `POST /token` body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 4096;

/// Why serving stopped other than by `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeError {
    /// The listener failed.
    Io,
    /// Not written yet.
    Unimplemented { story: &'static str },
}

/// Serves `issuer` on `listener` until `stop` completes.
///
/// # Errors
/// [`ServeError::Io`].
pub async fn serve(
    listener: TcpListener,
    issuer: Arc<Issuer>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let _ = (listener, issuer, stop);
    Err(ServeError::Unimplemented { story: "E21-52" })
}
