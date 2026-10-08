//! The one URL a transport ever dials (connections spec §6.2 rule 1).

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
/// query, or fragment in the URL.
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
        let _ = (pinned_host, endpoint, build);
        Err(McpError::Unimplemented { story: "E7-16" })
    }
}
