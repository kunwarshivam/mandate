//! The API-7 operations' lenient decoder (workspace API spec §5, DEC-682 item 27, DEC-689 item 2,
//! DEC-886): only an operation's hard members are strict, and every other member that is unknown or
//! fails to parse is dropped, never applied, and listed by its JSON pointer for the `202`'s
//! `dropped`.

use std::collections::BTreeMap;

use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Serialize};

use crate::envelope::{Record, StepUpEvidence};
use crate::requests::{
    ApprovalResponseRequest, EndDelegationRequest, HoldRequest, KillSwitchRequest,
    OwnerExitRequest, PauseRequest, Scope, Verdict,
};
use crate::wire::{Decimal, Id, Ref, Refused, Timestamp, decode, parse, refuse};

/// What [`decode_lenient`] kept of a body, and the pointer of each member it dropped: once each, in
/// the order the members appear in the body (DEC-886 item 11). A member left out is never listed
/// (item 12), and a body read as `{}` lists `""`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept<T> {
    pub value: T,
    pub dropped: Vec<String>,
}

/// An API-7 operation's body as the server keeps it. The hard members are strict: a body whose hard
/// member is missing, unparsable, or named twice is refused (DEC-886 item 6).
pub trait Api7: Serialize + sealed::Sealed {
    /// The body's hard members by name: the kill switch's `scope`, an owner exit's `instrument`,
    /// and a Skip's `verdict` and `content_hash` (DEC-682 item 27). The path ids are not in a body.
    const HARD: &'static [&'static str];
    /// Whether a body that is not a JSON object is read as `{}` with `dropped: [""]`: pause and
    /// hold only (DEC-682 item 27, DEC-886 item 5).
    const READS_NON_OBJECT: bool;
}

mod sealed {
    use crate::wire::Refused;

    pub trait Sealed: Sized {
        /// What the shape keeps of `body`, marking each member it keeps or lists.
        fn keep(body: &mut Body<'_>) -> Result<Self, Refused>;
    }

    /// A lenient body's members in body order, each with its value's bytes and whether it is
    /// kept, and the pointers listed inside a kept hard member, by that member's name.
    pub struct Body<'a> {
        pub(super) raw: &'a [u8],
        pub(super) members: Vec<(String, &'a [u8], bool)>,
        pub(super) inside: Vec<(String, String)>,
    }
}

use sealed::Body;

/// An approval response as the server keeps it: an `approved` judged strictly, or a Skip judged
/// leniently (§5.2, DEC-682 item 27).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ApprovalAnswer {
    Approved(ApprovalResponseRequest),
    Skipped(SkipResponse),
}

/// The members a Skip keeps. It never names a delegation: a non-null one is dropped (DEC-886
/// item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkipResponse {
    pub verdict: SkipVerdict,
    pub content_hash: Ref,
    pub record: Option<Record>,
    pub step_up: Option<StepUpEvidence>,
}

/// A Skip's verdict, `skipped`, the one this shape can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipVerdict {
    Skipped,
}

/// `$shape`'s [`Api7`] answers.
macro_rules! api7 {
    ($($shape:ty: [$($hard:literal),*], $reads:literal;)+) => {
        $(impl Api7 for $shape {
            const HARD: &'static [&'static str] = &[$($hard),*];
            const READS_NON_OBJECT: bool = $reads;
        })+
    };
}

api7! {
    PauseRequest: [], true;
    HoldRequest: [], true;
    EndDelegationRequest: [], false;
    KillSwitchRequest: ["scope"], false;
    OwnerExitRequest: ["instrument"], false;
    ApprovalAnswer: ["verdict", "content_hash"], false;
}

/// The body of an API-7 operation as `T` keeps it, with what it dropped (DEC-682 item 27,
/// DEC-886).
///
/// # Errors
/// [`Refused::Invalid`] for a missing, unparsable, or duplicated hard member, for a body that is
/// not a JSON object on an operation other than pause and hold, and for an `approved` that fails
/// [`crate::wire::decode`].
pub fn decode_lenient<T: Api7>(body: &[u8]) -> Result<Kept<T>, Refused> {
    let Some(members) = members(body) else {
        if T::READS_NON_OBJECT {
            let value = T::keep(&mut Body::new(body, Vec::new()))?;
            return Ok(Kept {
                value,
                dropped: vec![String::new()],
            });
        }
        let refused = parse::<BTreeMap<String, IgnoredAny>>(body).err();
        return Err(refused.unwrap_or_else(|| refuse(String::new(), "malformed")));
    };
    let mut body = Body::new(body, members);
    if let Some(name) = T::HARD.iter().find(|name| body.twice(name)) {
        return Err(refuse(pointer(name), "duplicate_member"));
    }
    let value = T::keep(&mut body)?;
    Ok(Kept {
        value,
        dropped: body.dropped(),
    })
}

/// `name` as a member of the root, escaped as RFC 6901 says.
fn pointer(name: &str) -> String {
    format!("/{}", name.replace('~', "~0").replace('/', "~1"))
}

/// `at` past any JSON whitespace.
fn skip_space(body: &[u8], at: usize) -> usize {
    let rest = body.get(at..).unwrap_or_default();
    let space = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
    at.saturating_add(space)
}

/// The one JSON value at `at` in `body`, and where it ends.
fn value_at<T: DeserializeOwned>(body: &[u8], at: usize) -> Option<(T, usize)> {
    let rest = body.get(at..)?;
    let mut values = serde_json::Deserializer::from_slice(rest).into_iter::<T>();
    let value = values.next()?.ok()?;
    Some((value, at.checked_add(values.byte_offset())?))
}

/// An object's members in the order its text names them, each with its value's bytes, or `None`
/// for a body that is not exactly one JSON object. `serde_json::Value` sorts members, and
/// `dropped` lists them in body order (DEC-886 item 11).
fn members(body: &[u8]) -> Option<Vec<(String, &[u8])>> {
    std::str::from_utf8(body).ok()?;
    serde_json::from_slice::<IgnoredAny>(body).ok()?;
    let mut at = skip_space(body, 0);
    if body.get(at) != Some(&b'{') {
        return None;
    }
    at = skip_space(body, at.checked_add(1)?);
    let mut found = Vec::new();
    while body.get(at) == Some(&b'"') {
        let (name, end) = value_at::<String>(body, at)?;
        let start = skip_space(body, skip_space(body, end).checked_add(1)?);
        let (IgnoredAny, end) = value_at::<IgnoredAny>(body, start)?;
        found.push((name, body.get(start..end)?));
        at = skip_space(body, end);
        at = skip_space(
            body,
            at.checked_add(usize::from(body.get(at) == Some(&b',')))?,
        );
    }
    Some(found)
}

/// `raw` as the value of member `name`, through the strict decoder, so a refusal is located under
/// the member's pointer.
fn member<T: DeserializeOwned>(name: &str, raw: &[u8]) -> Result<T, Refused> {
    let key = serde_json::to_vec(name).map_err(|_| refuse(pointer(name), "malformed"))?;
    let wrapped = [b"{".as_slice(), &key, b":", raw, b"}"].concat();
    let mut one = parse::<BTreeMap<String, T>>(&wrapped)?;
    one.remove(name)
        .ok_or_else(|| refuse(pointer(name), "missing"))
}

impl<'a> sealed::Body<'a> {
    fn new(raw: &'a [u8], members: Vec<(String, &'a [u8])>) -> Self {
        let members = members.into_iter().map(|(n, v)| (n, v, false)).collect();
        Self {
            raw,
            members,
            inside: Vec::new(),
        }
    }

    fn twice(&self, name: &str) -> bool {
        self.members.iter().filter(|(n, ..)| n == name).count() > 1
    }

    fn present(&self, name: &str) -> bool {
        self.members.iter().any(|(n, ..)| n == name)
    }

    /// The bytes of `name` named once; a duplicated member is never read.
    fn get(&self, name: &str) -> Option<&'a [u8]> {
        let mut named = self.members.iter().filter(|(n, ..)| n == name);
        let (_, raw, _) = named.next()?;
        named.next().is_none().then_some(*raw)
    }

    fn mark(&mut self, name: &str, kept: bool) {
        for member in self.members.iter_mut().filter(|(n, ..)| n == name) {
            member.2 = kept;
        }
    }

    /// `name` if it is named once and parses, kept; otherwise `None`, and it stays dropped.
    fn take<T: DeserializeOwned>(&mut self, name: &str) -> Option<T> {
        let value = member(name, self.get(name)?).ok()?;
        self.mark(name, true);
        Some(value)
    }

    /// A hard member: absent is `missing`, and an unparsable one is refused where it fails.
    fn hard<T: DeserializeOwned>(&mut self, name: &str) -> Result<T, Refused> {
        let raw = self
            .get(name)
            .ok_or_else(|| refuse(pointer(name), "missing"))?;
        let value = member(name, raw)?;
        self.mark(name, true);
        Ok(value)
    }

    /// Each member not kept, and each pointer listed inside one, once, in body order.
    fn dropped(&self) -> Vec<String> {
        let mut dropped: Vec<String> = Vec::new();
        for (name, _, kept) in &self.members {
            let own = (!kept).then(|| pointer(name));
            let inside = self.inside.iter().filter(|(parent, _)| parent == name);
            for listed in own.into_iter().chain(inside.map(|(_, at)| at.clone())) {
                if !dropped.contains(&listed) {
                    dropped.push(listed);
                }
            }
        }
        dropped
    }
}

/// The kept `record` of a pause, a hold, or ending a delegation.
fn record(body: &mut Body<'_>) -> Option<Record> {
    body.take::<Option<Record>>("record").flatten()
}

impl sealed::Sealed for PauseRequest {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        Ok(Self {
            record: record(body),
        })
    }
}

impl sealed::Sealed for HoldRequest {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        Ok(Self {
            record: record(body),
        })
    }
}

impl sealed::Sealed for EndDelegationRequest {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        Ok(Self {
            record: record(body),
        })
    }
}

/// A kill switch scope's kind (§5.4).
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Agent,
    Connection,
    Workspace,
}

/// The members of `scope` the kill switch reads: its kind, then an agent's or connection's id, or
/// a workspace's `null` one.
#[derive(Deserialize)]
struct ScopeKind {
    kind: Kind,
}

#[derive(Deserialize)]
struct ScopeId {
    id: Id,
}

#[derive(Deserialize)]
struct WorkspaceId {
    #[serde(default, rename = "id", deserialize_with = "crate::wire::null")]
    _id: (),
}

/// The scope's `kind` and `id` members, as an object of only those, in the order sent. An unknown
/// member is never read, so nothing inside it can refuse the stop or move its target (DEC-900).
fn read_in_scope(raw: &[u8]) -> Result<Vec<u8>, Refused> {
    let Some(inside) = members(raw) else {
        member::<BTreeMap<String, IgnoredAny>>("scope", raw)?;
        return Err(refuse(pointer("scope"), "malformed"));
    };
    let mut read = Vec::new();
    for (name, value) in inside.into_iter().filter(|(n, _)| n == "kind" || n == "id") {
        let key = serde_json::to_vec(&name).map_err(|_| refuse(pointer("scope"), "malformed"))?;
        read.push([key.as_slice(), b":", value].concat());
    }
    Ok([b"{".as_slice(), &read.join(b",".as_slice()), b"}"].concat())
}

/// The hard `scope`: an unknown member inside it, whatever it holds, and a workspace scope's
/// non-null `id`, are dropped and listed under it; `kind` and `id` apply in the route's workspace
/// only (DEC-886 items 1 and 2, DEC-900). `kind` or `id` named twice inside it is refused.
fn scope(body: &mut Body<'_>) -> Result<Scope, Refused> {
    let sent = body
        .get("scope")
        .ok_or_else(|| refuse(pointer("scope"), "missing"))?;
    let read = read_in_scope(sent)?;
    let raw = read.as_slice();
    member::<BTreeMap<String, IgnoredAny>>("scope", raw)?;
    let ScopeKind { kind } = member("scope", raw)?;
    let (scope, id_kept) = match kind {
        Kind::Agent => (
            Scope::Agent {
                id: member::<ScopeId>("scope", raw)?.id,
            },
            true,
        ),
        Kind::Connection => (
            Scope::Connection {
                id: member::<ScopeId>("scope", raw)?.id,
            },
            true,
        ),
        Kind::Workspace => {
            let null = member::<WorkspaceId>("scope", raw).is_ok();
            (Scope::Workspace { id: () }, null)
        }
    };
    let inside = members(sent).unwrap_or_default().into_iter();
    let listed = inside.filter(|(name, _)| name != "kind" && (name != "id" || !id_kept));
    body.inside = listed
        .map(|(name, _)| ("scope".to_owned(), format!("/scope{}", pointer(&name))))
        .collect();
    body.mark("scope", true);
    Ok(scope)
}

impl sealed::Sealed for KillSwitchRequest {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        Ok(Self {
            scope: scope(body)?,
            environment_shown: body.take("environment_shown").flatten(),
            owner_exit: body.take("owner_exit").flatten(),
            record: body.take("record").flatten(),
            step_up: body.take("step_up").flatten(),
        })
    }
}

/// An owner exit's bid confirmation, all or nothing (§5.4, DEC-682 item 27).
const BID: [&str; 4] = ["bid", "bid_size", "quoted_at", "floor"];

impl sealed::Sealed for OwnerExitRequest {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        let instrument = body.hard("instrument")?;
        let sent: Vec<&str> = BID.into_iter().filter(|n| body.present(n)).collect();
        let bid: Option<Option<Decimal>> = body.take("bid");
        let bid_size: Option<Option<Decimal>> = body.take("bid_size");
        let quoted_at: Option<Option<Timestamp>> = body.take("quoted_at");
        let floor: Option<Option<Decimal>> = body.take("floor");
        let parsed = [
            bid.is_some(),
            bid_size.is_some(),
            quoted_at.is_some(),
            floor.is_some(),
        ];
        let valued = [
            matches!(bid, Some(Some(_))),
            matches!(bid_size, Some(Some(_))),
            matches!(quoted_at, Some(Some(_))),
            matches!(floor, Some(Some(_))),
        ];
        let failed = BID.iter().zip(parsed).any(|(n, p)| !p && sent.contains(n));
        let whole = valued.iter().all(|v| *v);
        let none = !failed && !valued.iter().any(|v| *v);
        if !whole && !none {
            for name in sent {
                body.mark(name, false);
            }
        }
        Ok(Self {
            instrument,
            bid: part(bid, whole),
            bid_size: part(bid_size, whole),
            quoted_at: part(quoted_at, whole),
            floor: part(floor, whole),
            record: body.take("record").flatten(),
            step_up: body.take("step_up").flatten(),
        })
    }
}

/// A part of the bid confirmation, kept only with the whole confirmation.
fn part<T>(part: Option<Option<T>>, whole: bool) -> Option<T> {
    part.flatten().filter(|_| whole)
}

impl sealed::Sealed for ApprovalAnswer {
    fn keep(body: &mut Body<'_>) -> Result<Self, Refused> {
        if body.hard::<Verdict>("verdict")? == Verdict::Approved {
            let strict = decode::<ApprovalResponseRequest>(body.raw)?;
            for member in &mut body.members {
                member.2 = true;
            }
            return Ok(Self::Approved(strict));
        }
        let content_hash = body.hard("content_hash")?;
        if body.get("delegation") == Some(b"null".as_slice()) {
            body.mark("delegation", true);
        }
        Ok(Self::Skipped(SkipResponse {
            verdict: SkipVerdict::Skipped,
            content_hash,
            record: body.take("record"),
            step_up: body.take("step_up").flatten(),
        }))
    }
}
