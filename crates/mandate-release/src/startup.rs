//! The start-up signature check (E21-35, DEC-971; infrastructure design §7.1 step 3, OPS-15): a
//! process outside dev and CI starts only when its own executable carries a signature that
//! verifies against a release key compiled into it.

use std::path::Path;

use crate::digest::BuildDigest;

/// The release keys a process outside dev and CI trusts, as cosign's PEM public keys: the
/// founder's hardware key (ES-17, H8) once it exists. None is wired yet, so no binary passes the
/// check in staging or production until the founder adds the key's public half here.
pub const RELEASE_KEYS: &[&str] = &[];

/// Names the deployment tier: `dev`, `ci`, `staging` or `production`.
pub const TIER_VAR: &str = "MANDATE_DEPLOYMENT_TIER";
/// Names the file holding the executable's signature; without it, `<executable>.sig`.
pub const SIGNATURE_VAR: &str = "MANDATE_RELEASE_SIGNATURE";
/// Names a test key's PEM file, read only in dev and CI.
pub const TEST_KEY_VAR: &str = "MANDATE_RELEASE_TEST_KEY";

/// Where the process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Dev,
    Ci,
    Staging,
    Production,
}

impl Tier {
    /// The tier a setting names; no setting, or one it does not know, is production.
    pub fn from_setting(setting: Option<&str>) -> Self {
        let _ = setting;
        Self::Dev
    }
}

/// A cosign ECDSA P-256 public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseKey {
    point: Vec<u8>,
}

impl ReleaseKey {
    /// Reads cosign's PEM public key: a P-256 `SubjectPublicKeyInfo`.
    pub fn from_pem(pem: &str) -> Result<Self, Refused> {
        let _ = pem;
        Err(Refused::Unimplemented)
    }

    /// The uncompressed P-256 point, `04 || X || Y`.
    pub fn point(&self) -> &[u8] {
        &self.point
    }
}

/// Which key the executable's signature verified against, or that it carried none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// A compiled-in release key.
    Release,
    /// The test key, in dev or CI.
    Test,
    /// No signature, in dev or CI.
    Unsigned,
}

impl Trust {
    pub fn code(&self) -> &'static str {
        ""
    }
}

/// A process the check lets start: the build digest it journals as `actor.build`, and its trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    pub digest: BuildDigest,
    pub trust: Trust,
}

/// Why the check refuses to let a process start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// No signature, outside dev and CI.
    #[error("release_unsigned")]
    Unsigned,
    /// A signature no trusted key verifies.
    #[error("release_signature_not_verified")]
    NotVerified,
    /// A signature or key that is not what cosign writes.
    #[error("release_signature_malformed")]
    Malformed,
    /// The executable, its signature, or the test key could not be read.
    #[error("release_unreadable")]
    Unreadable(std::io::ErrorKind),
    /// The tests-first stub (DEC-921).
    #[error("unimplemented (E21-35)")]
    Unimplemented,
}

impl Refused {
    /// The generic code the process prints when it refuses (`AGENTS.md` rule 6).
    pub fn code(&self) -> &'static str {
        ""
    }
}

/// The decision itself, over inputs only: the tier, the executable's bytes, its signature file's
/// text if it has one, the release keys, and the test key if one was given.
pub fn decide(
    tier: Tier,
    executable: &[u8],
    signature: Option<&str>,
    release_keys: &[ReleaseKey],
    test_key: Option<&ReleaseKey>,
) -> Result<Admitted, Refused> {
    let _ = (tier, executable, signature, release_keys, test_key);
    Err(Refused::Unimplemented)
}

/// The check a process runs before anything else: its tier from `vars`, the bytes at
/// `executable`, its signature from the file [`SIGNATURE_VAR`] names or else `default_signature`,
/// and, in dev and CI only, the test key [`TEST_KEY_VAR`] names, with [`RELEASE_KEYS`].
pub fn check(
    vars: &dyn Fn(&str) -> Option<String>,
    executable: &Path,
    default_signature: &Path,
) -> Result<Admitted, Refused> {
    let _ = (vars, executable, default_signature);
    Err(Refused::Unimplemented)
}

/// [`check`] over this process's environment and its running executable.
pub fn check_running() -> Result<Admitted, Refused> {
    Err(Refused::Unimplemented)
}
