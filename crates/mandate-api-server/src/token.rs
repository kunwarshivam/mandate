//! Checking the managed issuer's access token (identity spec §6.1, §3.11 item 5): one configured
//! issuer and audience, its published keys by `kid`, the algorithms its configuration allows.

use std::sync::Arc;

use mandate_authn::{IssuerConfig, Jwks, TokenKind, verify};
use mandate_time::UtcNanos;

/// What a presented token amounts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// It verified: its issuer and subject.
    Subject { issuer: String, subject: String },
    /// It did not verify.
    Refused,
    /// The issuer's keys could not be had, so nothing can be said of it (identity spec §6.4
    /// route 1's outage).
    Unreachable,
}

/// The issuer's current key set, as last fetched.
pub trait Keys: Send + Sync {
    /// The keys, or `None` when none was ever fetched.
    fn current(&self) -> Option<Arc<Jwks>>;
}

/// A key set fixed at construction.
#[derive(Debug, Clone)]
pub struct FixedKeys(pub Option<Arc<Jwks>>);

impl Keys for FixedKeys {
    fn current(&self) -> Option<Arc<Jwks>> {
        self.0.clone()
    }
}

/// Checks a token.
pub trait TokenCheck: Send + Sync {
    fn check(&self, token: &str, now: UtcNanos) -> Checked;
}

/// The one configured issuer and its keys.
pub struct Issuer {
    config: IssuerConfig,
    keys: Arc<dyn Keys>,
}

impl Issuer {
    pub fn new(config: IssuerConfig, keys: Arc<dyn Keys>) -> Self {
        Self { config, keys }
    }
}

impl TokenCheck for Issuer {
    fn check(&self, token: &str, now: UtcNanos) -> Checked {
        let Some(jwks) = self.keys.current() else {
            return Checked::Unreachable;
        };
        match verify(token, &self.config, &jwks, TokenKind::AccessToken, now) {
            Ok(verified) => Checked::Subject {
                issuer: verified.issuer().to_owned(),
                subject: verified.subject().to_owned(),
            },
            Err(_) => Checked::Refused,
        }
    }
}
