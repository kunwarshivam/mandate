//! The scalar members of every shape, and the one decoder and encoder every body goes through
//! (workspace API spec §3.1). Each scalar is a string in the journal's canonical form (journal spec
//! §4, §9.3); a JSON number in a decimal member is refused as `invalid`, never rounded.

use std::collections::BTreeSet;

use mandate_canon::{DecStr, decode_ulid};
use mandate_domain::AssetId;
use mandate_journal::ArtifactRef;
use mandate_time::UtcNanos;
use serde::de::{DeserializeOwned, Error as _, IgnoredAny, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_path_to_error::Segment;

use crate::problem::Violation;

/// The request body `body` as `T`, or every way it fails, each as one [`Violation::Schema`] with a
/// JSON pointer and a code (DEC-681 item 10):
/// - `malformed` at `""`: not UTF-8 JSON (§3.1), empty, or bytes after the value;
/// - `duplicate_member`: an object naming one member twice, at any depth;
/// - `unknown_member` at the member: a member `T` does not name, so a body carrying `requested_by`
///   is refused and never read (API-6; DEC-681 item 6);
/// - `type`: a member of the wrong JSON type, such as a number where a decimal string belongs;
/// - `non_canonical`: a scalar not in its canonical form;
/// - `missing`: a required member absent;
/// - `enum`: a value outside its closed set;
/// - any rule of [`Validate`], checked after the shape decodes.
///
/// Both `malformed` checks are needed: `from_utf8` refuses a byte that is not UTF-8 anywhere,
/// which serde_json leaves unchecked inside a string it skips, and [`duplicate`]'s `IgnoredAny`
/// pass proves the body one JSON value before [`walk`]'s byte scan, which assumes one.
///
/// # Errors
/// [`Refused::Invalid`], which the server answers as `invalid` (422).
pub fn decode<T: DeserializeOwned + Validate>(body: &[u8]) -> Result<T, Refused> {
    std::str::from_utf8(body).map_err(|_| refuse(String::new(), "malformed"))?;
    if let Some(path) = duplicate(body)? {
        return Err(refuse(path, "duplicate_member"));
    }
    let mut reader = serde_json::Deserializer::from_slice(body);
    let value: T =
        serde_path_to_error::deserialize(&mut reader).map_err(|error| located(&error))?;
    value.validate()?;
    Ok(value)
}

/// The one message every schema violation carries: the code and the pointer say what and where,
/// and nothing from the body is echoed (rule 6).
const MESSAGE: &str = "The body does not match its schema.";

fn refuse(path: String, code: &str) -> Refused {
    Refused::Invalid {
        violations: vec![violation(path, code)],
    }
}

fn violation(path: String, code: &str) -> Violation {
    Violation::Schema {
        path,
        code: code.to_owned(),
        message: MESSAGE.to_owned(),
    }
}

/// The pointer of the first member an object names twice, `None` when there is none, or
/// `malformed` for a body that is not exactly one JSON value.
fn duplicate(body: &[u8]) -> Result<Option<String>, Refused> {
    serde_json::from_slice::<IgnoredAny>(body).map_err(|_| refuse(String::new(), "malformed"))?;
    let walked = walk(body);
    Ok(walked.twice.then(|| pointer(&walked.frames)))
}

/// `parent`'s member `name` as a JSON pointer, `~` and `/` escaped (RFC 6901).
fn member(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}

/// The violation for serde's refusal of a body already known to be one JSON value with no
/// duplicate member: its code read from serde's message, and its pointer from the path serde was
/// deserializing when it refused, each member and variant name escaped as [`member`] does
/// (DEC-681 item 10). A refusal inside an internally tagged object, which serde reads whole before
/// it decodes, is located at that object.
fn located(error: &serde_path_to_error::Error<serde_json::Error>) -> Refused {
    let text = error.inner().to_string();
    let code = CODES
        .iter()
        .find(|(start, _)| text.starts_with(start))
        .map_or("type", |(_, code)| code);
    let path = error
        .path()
        .iter()
        .fold(String::new(), |path, segment| match segment {
            Segment::Seq { index } => format!("{path}/{index}"),
            Segment::Map { key } | Segment::Enum { variant: key } => member(&path, key),
            Segment::Unknown => path,
        });
    refuse(path, code)
}

/// serde's refusals by the start of their message, each with its wire code (DEC-681 item 10).
/// Any other refusal is a wrong JSON type or value.
const CODES: [(&str, &str); 4] = [
    ("unknown field", "unknown_member"),
    ("missing field", "missing"),
    ("unknown variant", "enum"),
    ("not in canonical form", "non_canonical"),
];

/// Where a reader stands in its JSON: an object's last member named and every name it has read,
/// or an array's current item.
enum Frame {
    Object {
        name: Option<String>,
        next_is_name: bool,
        seen: BTreeSet<String>,
    },
    Array(usize),
}

/// How far [`walk`] read: the frames open where it stopped, and whether it stopped at a member
/// its object had already named.
struct Walked {
    frames: Vec<Frame>,
    twice: bool,
}

/// Reads `body` to its end, or to the first member an object names twice, which serde's own
/// reader takes without refusing it.
fn walk(body: &[u8]) -> Walked {
    let mut frames = Vec::new();
    let mut at = 0_usize;
    while let Some(byte) = body.get(at) {
        let mut step = 1_usize;
        match (byte, frames.last_mut()) {
            (b'"', top) => {
                let rest = body.get(at..).unwrap_or_default();
                let mut strings = serde_json::Deserializer::from_slice(rest).into_iter::<String>();
                let Some(Ok(text)) = strings.next() else {
                    break;
                };
                step = strings.byte_offset();
                if let Some(Frame::Object {
                    name,
                    next_is_name: next @ true,
                    seen,
                }) = top
                {
                    let again = !seen.insert(text.clone());
                    *name = Some(text);
                    *next = false;
                    if again {
                        return Walked {
                            frames,
                            twice: true,
                        };
                    }
                }
            }
            (b'{', _) => frames.push(Frame::Object {
                name: None,
                next_is_name: true,
                seen: BTreeSet::new(),
            }),
            (b'[', _) => frames.push(Frame::Array(0)),
            (b'}' | b']', _) => drop(frames.pop()),
            (b',', Some(Frame::Object { next_is_name, .. })) => *next_is_name = true,
            (b',', Some(Frame::Array(index))) => *index = index.saturating_add(1),
            _ => {}
        }
        at = at.saturating_add(step);
    }
    Walked {
        frames,
        twice: false,
    }
}

/// The pointer of the member being read where a [`walk`] stopped: the member named twice.
fn pointer(frames: &[Frame]) -> String {
    frames
        .iter()
        .fold(String::new(), |path, frame| match frame {
            Frame::Object {
                name: Some(name), ..
            } => member(&path, name),
            Frame::Object { name: None, .. } => path,
            Frame::Array(index) => format!("{path}/{index}"),
        })
}

/// What a schema requires that serde cannot express: a pattern on a plain string, a numeric bound,
/// an array's length or uniqueness, or one member conditioned on another (DEC-689 item 1).
pub trait Validate {
    /// # Errors
    /// [`Refused::Invalid`] with one located violation per broken rule.
    fn validate(&self) -> Result<(), Refused>;
}

/// [`Validate`] for shapes: `none` whose serde types enforce every rule, and `checked` whose
/// [`Rules`] hold the rest.
macro_rules! rules {
    (none: $($shape:ty),+) => {
        $(impl $crate::wire::Validate for $shape {
            fn validate(&self) -> Result<(), $crate::wire::Refused> {
                Ok(())
            }
        })+
    };
    (checked: $($shape:ty),+) => {
        $(impl $crate::wire::Validate for $shape {
            fn validate(&self) -> Result<(), $crate::wire::Refused> {
                let mut check = $crate::wire::Check::default();
                $crate::wire::Rules::rules(self, "", &mut check);
                check.done()
            }
        })+
    };
}
pub(crate) use rules;

/// `Serialize` and `Deserialize` for each shape derived with `serde(remote = "Self")`, so that a
/// shape reads only from a JSON object (DEC-881, DEC-882). serde's derived structs and internally
/// tagged enums also read a JSON array through `visit_seq`, their members or tag and members in
/// order, which no schema of the crate allows. `remote = "Self"` turns the derives into the
/// shape's own `serialize` and `deserialize` functions; the impls here write through the derived
/// one unchanged, and read by asking for a map: an array, or any other JSON type, is refused by
/// serde_json as an invalid type at the shape's own path, which [`located`] answers as `type`
/// there, never at an item inside the array. The map's members go to the derived reader through
/// `MapAccessDeserializer`, so a refusal inside the object keeps the member path DEC-880 tracks.
macro_rules! object_only {
    ($($shape:ident),+) => {
        $(impl serde::Serialize for $shape {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $shape::serialize(self, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $shape {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Members;
                impl<'de> serde::de::Visitor<'de> for Members {
                    type Value = $shape;
                    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                        f.write_str("an object")
                    }
                    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<$shape, A::Error> {
                        $shape::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    }
                }
                deserializer.deserialize_map(Members)
            }
        })+
    };
}
pub(crate) use object_only;

rules!(none: Decimal, Ref, Timestamp, Asset, Id, EventId);

/// A shape's rules beyond serde's, each broken one reported at its pointer under `at`.
pub(crate) trait Rules {
    fn rules(&self, at: &str, check: &mut Check);
}

/// The violations a shape's [`Rules`] found.
#[derive(Debug, Default)]
pub(crate) struct Check(Vec<Violation>);

impl Check {
    /// Records `code` at `path` unless `holds`.
    pub(crate) fn rule(&mut self, holds: bool, path: &str, code: &str) {
        if !holds {
            self.0.push(violation(path.to_owned(), code));
        }
    }

    pub(crate) fn done(self) -> Result<(), Refused> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Refused::Invalid { violations: self.0 })
        }
    }
}

/// A JSON pointer (RFC 6901) whose segments hold no control character `U+0000` to `U+001F`; the
/// root `""` is one only where `root` allows it.
pub(crate) fn is_pointer(text: &str, root: bool) -> bool {
    let escapes = text
        .split('~')
        .skip(1)
        .all(|after| after.starts_with(['0', '1']));
    let shaped = (root && text.is_empty()) || text.starts_with('/');
    shaped && escapes && !text.chars().any(|c| c < ' ')
}

/// `^[a-z][a-z0-9_]*$`: a wire code, a policy key, a refusal reason.
pub(crate) fn is_word(text: &str) -> bool {
    let mut chars = text.chars();
    let first = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    first && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A journal stream id (journal spec §2): `acct` and `agent` with two segments, and each of
/// `single` with the workspace id alone.
pub(crate) fn is_stream_id(text: &str, single: &[&str]) -> bool {
    let mut parts = text.split(':');
    let kind = parts.next().unwrap_or_default();
    let segments: Vec<&str> = parts.collect();
    let arity = match kind {
        "acct" | "agent" => 2,
        other if single.contains(&other) => 1,
        _ => 0,
    };
    segments.len() == arity && segments.iter().all(|s| is_segment(s))
}

/// `^[A-Za-z0-9_-]+$`, journal spec §2's identifier segment.
pub(crate) fn is_segment(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// An optional member the schema lets be absent but never `null`: with `#[serde(default)]`, an
/// absent member is `None` and a `null` one is refused as the wrong type.
///
/// # Errors
/// `T`'s, including for `null`.
pub fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// A member the schema pins to `null`, as `()`: any value is refused, `{}` and `[]` among them,
/// which a bare `()` inside a tagged enum would take.
///
/// # Errors
/// The wrong type, for anything but `null`.
pub fn null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    match Option::<IgnoredAny>::deserialize(deserializer)? {
        None => Ok(()),
        Some(IgnoredAny) => Err(D::Error::invalid_type(
            Unexpected::Other("a value"),
            &"null",
        )),
    }
}

/// The JSON bytes of a response member or body.
///
/// # Errors
/// [`EncodeError`] for a value serde cannot write as JSON, which no shape of this crate is.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodeError> {
    serde_json::to_vec(value).map_err(|_| EncodeError)
}

/// Why [`encode`] could not write a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the value has no JSON form")]
pub struct EncodeError;

/// Why [`decode`] refused a body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("the body is invalid")]
    Invalid { violations: Vec<Violation> },
}

/// Why a scalar's text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
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
        DecStr::parse(&text)
            .ok()
            .filter(|canonical| canonical.as_str() == text)
            .map(Self)
            .ok_or(WireError::NotCanonical)
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
        if is_segment(&text) && text.len() <= 64 {
            Ok(Self(text))
        } else {
            Err(WireError::NotCanonical)
        }
    }
}

impl Id {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
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
        match decode_ulid(&text) {
            Ok(_) => Ok(Self(text)),
            Err(_) => Err(WireError::NotCanonical),
        }
    }
}

impl EventId {
    /// The id spelling `value` (journal spec §3).
    pub(crate) fn of(value: u128) -> Self {
        Self(mandate_canon::encode_ulid(value))
    }
}

impl Serialize for EventId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}
