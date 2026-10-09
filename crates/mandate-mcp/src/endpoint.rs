//! The one URL a transport ever dials (connections spec §6.2 rule 1).

use std::net::IpAddr;

use reqwest::Url;

use crate::error::McpError;

/// Which build is running: only this crate's own test build may dial plain `http`, and only on
/// a loopback address, where its tests' server listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Build {
    Test,
    Production,
}

impl Build {
    pub(crate) const CURRENT: Self = if cfg!(test) {
        Self::Test
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
        if url.host_str() != Some(pinned_host) {
            return Err(McpError::HostNotPinned);
        }
        Ok(Self { url })
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }
}

/// `https`, or plain `http` to a loopback address in this crate's test build.
pub(crate) fn scheme_allowed(url: &Url, build: Build) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => build == Build::Test && is_loopback(url),
        _ => false,
    }
}

/// An IP literal on loopback; a name such as `localhost` is not, since resolving it is a lookup.
fn is_loopback(url: &Url) -> bool {
    url.host_str()
        .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}
