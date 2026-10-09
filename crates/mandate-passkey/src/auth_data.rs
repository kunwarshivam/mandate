//! Authenticator data (WebAuthn §6.1): the RP ID hash, the flags, the signature counter, the
//! attested credential data when `AT` is set, and an extensions map when `ED` is set. Nothing
//! may follow what the flags announce.

use mandate_canon::Digest;

use crate::cbor::{self, Cbor};
use crate::{Refusal, RelyingParty};

const UP: u8 = 0x01;
const UV: u8 = 0x04;
const BE: u8 = 0x08;
const BS: u8 = 0x10;
const AT: u8 = 0x40;
const ED: u8 = 0x80;

/// The longest credential ID WebAuthn allows.
const MAX_CREDENTIAL_ID: usize = 1023;

pub(crate) struct AuthData {
    rp_id_hash: [u8; 32],
    flags: u8,
    pub(crate) sign_count: u32,
    pub(crate) attested: Option<Attested>,
}

pub(crate) struct Attested {
    pub(crate) credential_id: Vec<u8>,
    pub(crate) public_key: Cbor,
}

impl AuthData {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        const MALFORMED: Refusal = Refusal::AuthenticatorDataMalformed;
        let (rp_id_hash, rest) = bytes.split_first_chunk::<32>().ok_or(MALFORMED)?;
        let (&flags, rest) = rest.split_first().ok_or(MALFORMED)?;
        let (count, mut rest) = rest.split_first_chunk::<4>().ok_or(MALFORMED)?;
        let mut attested = None;
        if flags & AT != 0 {
            let (_aaguid, after) = rest.split_first_chunk::<16>().ok_or(MALFORMED)?;
            let (length, after) = after.split_first_chunk::<2>().ok_or(MALFORMED)?;
            let length = usize::from(u16::from_be_bytes(*length));
            if length == 0 || length > MAX_CREDENTIAL_ID {
                return Err(MALFORMED);
            }
            let (credential_id, after) = after.split_at_checked(length).ok_or(MALFORMED)?;
            let (public_key, after) = cbor::read(after).map_err(|_| MALFORMED)?;
            attested = Some(Attested {
                credential_id: credential_id.to_vec(),
                public_key,
            });
            rest = after;
        }
        if flags & ED != 0 {
            let Ok(Cbor::Map(_)) = cbor::read_all(rest) else {
                return Err(MALFORMED);
            };
        } else if !rest.is_empty() {
            return Err(MALFORMED);
        }
        Ok(Self {
            rp_id_hash: *rp_id_hash,
            flags,
            sign_count: u32::from_be_bytes(*count),
            attested,
        })
    }

    /// The RP ID hash, user presence and verification (both required, §6.1), and backup
    /// flags consistent with each other.
    pub(crate) fn check(&self, rp: &RelyingParty) -> Result<(), Refusal> {
        if self.rp_id_hash != *Digest::of(rp.rp_id.as_bytes()).as_bytes() {
            return Err(Refusal::RpIdMismatch);
        }
        if self.flags & UP == 0 {
            return Err(Refusal::UserNotPresent);
        }
        if self.flags & UV == 0 {
            return Err(Refusal::UserNotVerified);
        }
        if self.backup_state() && !self.backup_eligible() {
            return Err(Refusal::BackupFlags);
        }
        Ok(())
    }

    pub(crate) fn backup_eligible(&self) -> bool {
        self.flags & BE != 0
    }

    pub(crate) fn backup_state(&self) -> bool {
        self.flags & BS != 0
    }
}
