//! The start-up signature check (E21-35, DEC-971; infrastructure design §7.1 step 3, OPS-15): a
//! process outside dev and CI starts only when its own executable carries a signature that
//! verifies against a release key compiled into it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};

use crate::digest::{BuildDigest, running_executable};

/// The DER prefix of a P-256 `SubjectPublicKeyInfo` (RFC 5480): `id-ecPublicKey` with the
/// `prime256v1` curve, then the 65-byte uncompressed point. cosign writes exactly this.
const SPKI_P256_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];
const POINT_LEN: usize = 65;
const PEM_BEGIN: &str = "-----BEGIN PUBLIC KEY-----";
const PEM_END: &str = "-----END PUBLIC KEY-----";

/// The release keys a process outside dev and CI trusts, as cosign's PEM public keys: the
/// founder's release signing key, whose private half lives only in the protected `release-signing`
/// GitHub environment and signs in CI after the founder approves (DEC-2605 rows 8 and 9, DEC-2024).
pub const RELEASE_KEYS: &[&str] = &[include_str!("../release-keys/cosign.pub")];

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
        match setting {
            Some("dev") => Self::Dev,
            Some("ci") => Self::Ci,
            Some("staging") => Self::Staging,
            _ => Self::Production,
        }
    }

    /// Dev and CI admit an unsigned build and honour a test key; nothing else does.
    fn lenient(self) -> bool {
        matches!(self, Self::Dev | Self::Ci)
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
        let body = pem
            .trim()
            .strip_prefix(PEM_BEGIN)
            .and_then(|rest| rest.strip_suffix(PEM_END))
            .ok_or(Refused::Malformed)?;
        let compact: String = body.split_whitespace().collect();
        let der = STANDARD.decode(compact).map_err(|_| Refused::Malformed)?;
        let point = der
            .strip_prefix(SPKI_P256_PREFIX.as_slice())
            .filter(|point| point.len() == POINT_LEN && point.first() == Some(&0x04))
            .ok_or(Refused::Malformed)?;
        Ok(Self {
            point: point.to_vec(),
        })
    }

    fn verifies(&self, message: &[u8], signature: &[u8]) -> bool {
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, &self.point)
            .verify(message, signature)
            .is_ok()
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
        match self {
            Self::Release => "release",
            Self::Test => "test",
            Self::Unsigned => "unsigned",
        }
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
}

impl Refused {
    /// The generic code the process prints when it refuses (`AGENTS.md` rule 6).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsigned => "release_unsigned",
            Self::NotVerified => "release_signature_not_verified",
            Self::Malformed => "release_signature_malformed",
            Self::Unreadable(_) => "release_unreadable",
        }
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
    let digest = BuildDigest::of_bytes(executable);
    let Some(signature) = signature else {
        return if tier.lenient() {
            Ok(Admitted {
                digest,
                trust: Trust::Unsigned,
            })
        } else {
            Err(Refused::Unsigned)
        };
    };
    let der = STANDARD
        .decode(signature.trim())
        .map_err(|_| Refused::Malformed)?;
    let trust = if release_keys
        .iter()
        .any(|key| key.verifies(executable, &der))
    {
        Trust::Release
    } else if tier.lenient() && test_key.is_some_and(|key| key.verifies(executable, &der)) {
        Trust::Test
    } else {
        return Err(Refused::NotVerified);
    };
    Ok(Admitted { digest, trust })
}

/// The check a process runs before anything else: its tier from `vars`, the bytes at
/// `executable`, its signature from the file [`SIGNATURE_VAR`] names or else `default_signature`,
/// and, in dev and CI only, the test key [`TEST_KEY_VAR`] names, with [`RELEASE_KEYS`].
pub fn check(
    vars: &dyn Fn(&str) -> Option<String>,
    executable: &Path,
    default_signature: &Path,
) -> Result<Admitted, Refused> {
    let tier = Tier::from_setting(vars(TIER_VAR).as_deref());
    let bytes = fs::read(executable).map_err(unreadable)?;
    let signature = match vars(SIGNATURE_VAR) {
        Some(named) => Some(fs::read_to_string(named).map_err(unreadable)?),
        None => match fs::read_to_string(default_signature) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(unreadable(error)),
        },
    };
    let release_keys = RELEASE_KEYS
        .iter()
        .map(|pem| ReleaseKey::from_pem(pem))
        .collect::<Result<Vec<_>, _>>()?;
    let test_key = match vars(TEST_KEY_VAR).filter(|_| tier.lenient()) {
        Some(path) => Some(ReleaseKey::from_pem(
            &fs::read_to_string(path).map_err(unreadable)?,
        )?),
        None => None,
    };
    decide(
        tier,
        &bytes,
        signature.as_deref(),
        &release_keys,
        test_key.as_ref(),
    )
}

/// [`check`] over this process's environment and its running executable, whose signature is
/// `<the path it was started from>.sig` unless [`SIGNATURE_VAR`] names another file.
pub fn check_running() -> Result<Admitted, Refused> {
    let executable =
        running_executable().map_err(|_| Refused::Unreadable(io::ErrorKind::NotFound))?;
    let started_from = std::env::current_exe().map_err(unreadable)?;
    let mut beside = started_from.into_os_string();
    beside.push(".sig");
    check(
        &|name| std::env::var(name).ok(),
        &executable,
        &PathBuf::from(beside),
    )
}

fn unreadable(error: io::Error) -> Refused {
    Refused::Unreadable(error.kind())
}
