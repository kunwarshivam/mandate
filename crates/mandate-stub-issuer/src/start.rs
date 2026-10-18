//! The start checks (DEC-2100 items 2, 3 and 5), in the order they run. Each refusal is written to
//! standard error as its code alone, and the process exits 2.

use std::net::SocketAddr;

/// Why the stub refused to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// A build without debug assertions, which holds no issuer (item 2).
    NotBuilt,
    /// A deployment tier other than `dev` or `ci`, or none (item 3).
    Tier,
    /// An address that is not `127.0.0.1` or `[::1]` with a port from 1 to 65535, or a port in
    /// use (items 5 and 10).
    Bind,
    /// The check is not written yet.
    Unimplemented { story: &'static str },
}

impl Refused {
    /// The code the process writes.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotBuilt => "stub_issuer_not_built",
            Self::Tier => "stub_issuer_tier",
            Self::Bind => "stub_issuer_bind",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

/// The address to bind, or the first refusal: `built` is whether this build has debug
/// assertions, `tier` the value of `MANDATE_DEPLOYMENT_TIER`, and `bind` the `--bind` argument.
///
/// # Errors
/// [`Refused::NotBuilt`], then [`Refused::Tier`], then [`Refused::Bind`].
pub fn check(built: bool, tier: Option<&str>, bind: &str) -> Result<SocketAddr, Refused> {
    let _ = (built, tier, bind);
    Err(Refused::Unimplemented { story: "E21-52" })
}
