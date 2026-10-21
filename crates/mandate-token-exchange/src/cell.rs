//! The token URL of a dev or CI cell's fixture provider (DEC-2629 item 3, DEC-2625). Compiled
//! only with debug assertions: a release build has no code that reads one, so it can send the
//! token request only to the URL `mandate_connections::hosts::LiveTokenRequest` pins.

use mandate_release::Tier;

/// The hosts a cell token URL may name: the loopback literals and `localhost`. No Alpaca host is
/// among them, so the URL can never reach Alpaca.
const LOOPBACK: [&str; 3] = ["127.0.0.1", "[::1]", "localhost"];

/// Why a cell token URL is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// `MANDATE_DEPLOYMENT_TIER` is not `dev` or `ci`.
    Tier,
    /// The URL is not `http` or `https` on a loopback host with a path and nothing else: no user
    /// information, query, or fragment, and never an Alpaca host.
    Url,
}

impl Refused {
    /// The code the process writes.
    pub fn code(self) -> &'static str {
        match self {
            Self::Tier => "token_exchange_cell_tier",
            Self::Url => "token_exchange_cell_url",
        }
    }
}

/// A token URL the cell's tier admits, kept exactly as given. Only [`parse`] makes one, so no
/// other text can become the adapter's target:
///
/// ```compile_fail,E0423
/// let forged = mandate_token_exchange::cell::CellTokenUrl("https://evil.example/t".to_owned());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellTokenUrl(String);

impl CellTokenUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Admits `text` as the cell's token URL when `tier` (`MANDATE_DEPLOYMENT_TIER`, read as
/// `mandate_release::Tier::from_setting` reads it) is `dev` or `ci`.
///
/// # Errors
/// [`Refused::Tier`] first, then [`Refused::Url`].
pub fn parse(tier: Option<&str>, text: &str) -> Result<CellTokenUrl, Refused> {
    if !matches!(Tier::from_setting(tier), Tier::Dev | Tier::Ci) {
        return Err(Refused::Tier);
    }
    if admitted(text) {
        Ok(CellTokenUrl(text.to_owned()))
    } else {
        Err(Refused::Url)
    }
}

/// `http://` or `https://`, a [`LOOPBACK`] host with an optional port from 1 to 65535, and a
/// non-empty path of unreserved characters and `/`: nothing else, so no user information, query,
/// or fragment.
fn admitted(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("http://")
        .or_else(|| text.strip_prefix("https://"))
    else {
        return false;
    };
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    let (host, port) = match authority.split_once(']') {
        Some((inside, after)) => (
            authority.get(..=inside.len()).unwrap_or_default(),
            match after.strip_prefix(':') {
                Some(port) => Some(port),
                None if after.is_empty() => None,
                None => return false,
            },
        ),
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port_ok = port.is_none_or(|port| {
        !port.starts_with('0')
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|n| n != 0)
    });
    let path_ok = !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/'));
    LOOPBACK.contains(&host) && port_ok && path_ok
}
