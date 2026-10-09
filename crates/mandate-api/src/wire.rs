//! The scalar members of every shape, and the one decoder and encoder every body goes through
//! (workspace API spec §3.1). Each scalar is a string in the journal's canonical form (journal spec
//! §4, §9.3); a JSON number in a decimal member is refused as `invalid`, never rounded.

use mandate_canon::DecStr;
use mandate_domain::AssetId;
use mandate_journal::ArtifactRef;
use mandate_time::UtcNanos;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize, Serializer};

use crate::Unimplemented;
use crate::problem::Violation;

/// The request body `body` as `T`, or every way it fails, each as one [`Violation::Schema`] with a
/// JSON pointer and a code (DEC-681 item 10):
/// - `malformed` at `""`: not JSON, empty, or bytes after the value;
/// - `duplicate_member`: an object naming one member twice, at any depth;
/// - `unknown_member` at the member: a member `T` does not name, so a body carrying `requested_by`
///   is refused and never read (API-6; DEC-681 item 6);
/// - `type`: a member of the wrong JSON type, such as a number where a decimal string belongs;
/// - `non_canonical`: a scalar not in its canonical form;
/// - `missing`: a required member absent;
/// - `enum`: a value outside its closed set;
/// - any rule of [`Validate`], checked after the shape decodes.
///
/// # Errors
/// [`Refused::Invalid`], which the server answers as `invalid` (422).
pub fn decode<T: DeserializeOwned + Validate>(body: &[u8]) -> Result<T, Refused> {
    let _ = body;
    Err(Refused::Unimplemented(Unimplemented))
}

/// What a schema requires that serde cannot express: a pattern on a plain string, a numeric bound,
/// an array's length or uniqueness, or one member conditioned on another (DEC-689 item 1).
pub trait Validate {
    /// # Errors
    /// [`Refused::Invalid`] with one located violation per broken rule.
    fn validate(&self) -> Result<(), Refused>;
}

/// [`Validate`] for shapes: `none` whose serde types enforce every rule, `pending` with rules
/// serde cannot express, stubbed until E10-10.
macro_rules! rules {
    (none: $($shape:ty),+) => {
        $(impl $crate::wire::Validate for $shape {
            fn validate(&self) -> Result<(), $crate::wire::Refused> {
                Ok(())
            }
        })+
    };
    (pending: $($shape:ty),+) => {
        $(impl $crate::wire::Validate for $shape {
            fn validate(&self) -> Result<(), $crate::wire::Refused> {
                let _ = self;
                Err($crate::wire::Refused::Unimplemented($crate::Unimplemented))
            }
        })+
    };
}
pub(crate) use rules;
rules!(none: Decimal, Ref, Timestamp, Asset, Id, EventId);

/// The JSON bytes of a response member or body.
///
/// # Errors
/// [`Unimplemented`] until E10-10.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Unimplemented> {
    let _ = value;
    Err(Unimplemented)
}

/// Why [`decode`] refused a body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("the body is invalid")]
    Invalid { violations: Vec<Violation> },
}

/// Why a scalar's text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error(transparent)]
    Unimplemented(Unimplemented),
    #[error("not in canonical form")]
    NotCanonical,
}

/// Money, a quantity, a price, or a fraction: journal spec §4.6's canonical decimal string. Text
/// that only normalizes to it (`"1.50"`, `"+1"`) is refused, not normalized (DEC-681 item 2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Decimal(DecStr);

impl TryFrom<String> for Decimal {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let _ = text;
        Err(WireError::Unimplemented(Unimplemented))
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// A `sha256:` reference: an artifact, a content hash, a version hash, or a digest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Ref(ArtifactRef);

impl TryFrom<String> for Ref {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        ArtifactRef::parse(&text)
            .map(Self)
            .ok_or(WireError::NotCanonical)
    }
}

impl Serialize for Ref {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// An instant, journal spec §4.7's canonical form.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Timestamp(UtcNanos);

impl TryFrom<String> for Timestamp {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        UtcNanos::parse(&text)
            .map(Self)
            .map_err(|_| WireError::NotCanonical)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// An instrument: mandate spec §3's asset id, lowercase `8-4-4-4-12` hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Asset(AssetId);

impl TryFrom<String> for Asset {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        AssetId::parse(&text)
            .map(Self)
            .map_err(|_| WireError::NotCanonical)
    }
}

impl Serialize for Asset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// An opaque id (agent, connection, delegation, preview, assertion, principal, workspace): journal
/// spec §9.3's `id`, `[A-Za-z0-9_-]+`, 1 to 64 characters (DEC-681 item 3). It never embeds a
/// name, an instrument, or a personal datum (§3.1), so it cannot carry prose either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Id(String);

impl TryFrom<String> for Id {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let _ = text;
        Err(WireError::Unimplemented(Unimplemented))
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// A control-stream event id: a ULID in 26 Crockford base-32 digits, uppercase (journal spec §3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct EventId(String);

impl TryFrom<String> for EventId {
    type Error = WireError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let _ = text;
        Err(WireError::Unimplemented(Unimplemented))
    }
}

impl Serialize for EventId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}
