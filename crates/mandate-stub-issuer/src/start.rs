//! The start checks (DEC-2100 items 2, 3 and 5), in the order they run. Each refusal is written to
//! standard error as its code alone, and the process exits 2.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use mandate_release::startup::Tier;

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
    /// Some but not all of the Alpaca provider's three settings, an empty one, or a client secret
    /// file that is not an owner-only regular file with content (DEC-2625 items 1 and 2), or
    /// `--alpaca-only` without them (DEC-1176 item 8).
    Alpaca,
    /// `--alpaca-only` without a `--public-host` that is a lowercase host name and a port, or a
    /// `--public-host` without `--alpaca-only` (DEC-1176 item 8).
    PublicHost,
}

impl Refused {
    /// The code the process writes.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotBuilt => "stub_issuer_not_built",
            Self::Tier => "stub_issuer_tier",
            Self::Bind => "stub_issuer_bind",
            Self::Alpaca => "stub_issuer_alpaca",
            Self::PublicHost => "stub_issuer_public_host",
        }
    }
}

/// The address to bind, or the first refusal: `built` is whether this build has debug
/// assertions, `tier` the value of `MANDATE_DEPLOYMENT_TIER`, and `bind` the `--bind` argument.
///
/// The tier is read as every process reads it, `Tier::from_setting`. The bind must also print back
/// as the text given, so the issuer, `http://<bind>`, is exactly what the operator wrote.
///
/// # Errors
/// [`Refused::NotBuilt`], then [`Refused::Tier`], then [`Refused::Bind`].
pub fn check(built: bool, tier: Option<&str>, bind: &str) -> Result<SocketAddr, Refused> {
    if !built {
        return Err(Refused::NotBuilt);
    }
    if !matches!(Tier::from_setting(tier), Tier::Dev | Tier::Ci) {
        return Err(Refused::Tier);
    }
    bind.parse::<SocketAddr>()
        .ok()
        .filter(|addr| loopback_literal(addr.ip()) && addr.port() != 0 && addr.to_string() == bind)
        .ok_or(Refused::Bind)
}

/// `127.0.0.1` or `::1` exactly: not the rest of `127.0.0.0/8`, and not `::ffff:127.0.0.1`.
fn loopback_literal(ip: IpAddr) -> bool {
    ip == IpAddr::V4(Ipv4Addr::LOCALHOST) || ip == IpAddr::V6(Ipv6Addr::LOCALHOST)
}

/// How the fixture provider starts without the issuer (DEC-1176 item 8): the address it listens
/// on, which may be any address, and the one `Host` it answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub bind: SocketAddr,
    pub host: String,
}

/// The fixture provider's start, or the first refusal: `built` and `tier` as for [`check`],
/// `bind` the `--bind` argument, and `public_host` the `--public-host` argument.
///
/// The bind may name any address but must have a port from 1 to 65535 and print back as written.
/// The public host is a lowercase host name, labels of letters, digits and inner hyphens joined by
/// dots, then a colon and a port from 1 to 65535 with no leading zero, and nothing else.
///
/// # Errors
/// [`Refused::NotBuilt`], then [`Refused::Tier`], then [`Refused::Bind`], then
/// [`Refused::PublicHost`].
pub fn check_fixture(
    built: bool,
    tier: Option<&str>,
    bind: &str,
    public_host: &str,
) -> Result<Fixture, Refused> {
    if !built {
        return Err(Refused::NotBuilt);
    }
    if !matches!(Tier::from_setting(tier), Tier::Dev | Tier::Ci) {
        return Err(Refused::Tier);
    }
    let bind = bind
        .parse::<SocketAddr>()
        .ok()
        .filter(|addr| addr.port() != 0 && addr.to_string() == bind)
        .ok_or(Refused::Bind)?;
    if !host_and_port(public_host) {
        return Err(Refused::PublicHost);
    }
    Ok(Fixture {
        bind,
        host: public_host.to_owned(),
    })
}

/// The longest host name, and the longest label in it (RFC 1035 §2.3.4).
const MAX_NAME: usize = 253;
const MAX_LABEL: usize = 63;

/// Whether `text` is a lowercase host name, a colon, and a port from 1 to 65535 with no leading
/// zero.
fn host_and_port(text: &str) -> bool {
    let Some((name, port)) = text.rsplit_once(':') else {
        return false;
    };
    let port_ok = port.bytes().all(|b| b.is_ascii_digit())
        && !port.starts_with('0')
        && port.parse::<u16>().is_ok_and(|port| port != 0);
    port_ok && name.len() <= MAX_NAME && name.split('.').all(label)
}

/// Whether `text` is one label: 1 to 63 lowercase letters, digits and hyphens, neither first nor
/// last a hyphen.
fn label(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=MAX_LABEL).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
}
