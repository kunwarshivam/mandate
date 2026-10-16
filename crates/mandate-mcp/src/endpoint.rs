//! The one URL a transport ever dials (connections spec §6.2 rule 1).

use std::net::IpAddr;

use reqwest::Url;

use crate::error::McpError;

/// Which build is running: this crate's own test build and the `rh-sim` rehearsal build may dial
/// plain `http`, and only on a loopback address, where the test server or the simulator listens;
/// the rehearsal build dials nothing but loopback, so it can never reach a broker (DEC-922 items
/// 1 and 2). Every other build, the `live` one included, is [`Build::Production`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Build {
    Test,
    Rehearsal,
    Production,
}

impl Build {
    pub(crate) const CURRENT: Self = if cfg!(test) {
        Self::Test
    } else if cfg!(feature = "rh-sim") {
        Self::Rehearsal
    } else {
        Self::Production
    };
}

/// An MCP endpoint on the pinned host: `https`, the host exactly as pinned, and no credentials,
/// query, or fragment in the URL. The pin is the host only: the port and the path are the
/// connector's to choose, and the TLS certificate is verified for that host whatever the port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedEndpoint {
    url: Url,
}

impl PinnedEndpoint {
    /// `pinned_host` is the host the connector compiles in, lower case as a URL parser writes
    /// it; `endpoint` is the full URL to dial, which must be on exactly that host.
    pub fn new(pinned_host: &str, endpoint: &str) -> Result<Self, McpError> {
        Self::for_build(pinned_host, endpoint, Build::CURRENT)
    }

    pub(crate) fn for_build(
        pinned_host: &str,
        endpoint: &str,
        build: Build,
    ) -> Result<Self, McpError> {
        let url = Url::parse(endpoint).map_err(|_| McpError::EndpointShape)?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(McpError::EndpointShape);
        }
        if !scheme_allowed(&url, build) {
            return Err(McpError::NotHttps);
        }
        if !host_allowed(&url, build) {
            return Err(McpError::OffLoopback);
        }
        if url.host_str() != Some(pinned_host) {
            return Err(McpError::HostNotPinned);
        }
        Ok(Self { url })
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
}

/// `https`, or plain `http` to a loopback address in this crate's test build or the rehearsal
/// build.
pub(crate) fn scheme_allowed(url: &Url, build: Build) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => build != Build::Production && is_loopback(url),
        _ => false,
    }
}

/// Any host, except in the rehearsal build, which dials only a loopback address (DEC-922 item 2).
pub(crate) fn host_allowed(url: &Url, build: Build) -> bool {
    build != Build::Rehearsal || is_loopback(url)
}

/// An IP literal on loopback; a name such as `localhost` is not, since resolving it is a lookup.
fn is_loopback(url: &Url) -> bool {
    url.host_str()
        .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}
