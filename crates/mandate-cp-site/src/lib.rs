#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Workspace control services' side of the control-plane link ([control-plane design §3](../../../docs/design/control-plane.md#3-components-and-interfaces),
//! CP-5 (b), CP-9; journal spec §9.17; backlog E20-3, [DEC-983]).
//!
//! [`Site`] implements `mandate-cp-agent`'s [`InstructionSink`]. For each instruction `cp-agent`
//! hands over, on the control stream of every workspace it covers, in order:
//!
//! 1. it reads the §9.17 records already journaled there and judges the instruction against the
//!    highest `sequence` of its kind (CP-9): the digest already in effect is `AlreadyApplied` and
//!    journals nothing; a lower sequence, or the same sequence with another digest, is refused and
//!    journaled `ControlPlaneMessageRefused` (`replayed`);
//! 2. otherwise it appends the instruction's record, through [`ControlStreams`], whose writer is
//!    the stream's single owner (journal spec §2, §5.1);
//! 3. only after the append commits, it hands the instruction to [`Effects`].
//!
//! A refusal is journaled once per digest. Nothing here is on the kill-switch path: owner commands
//! are appended by workspace services through their own writer whatever `cp-agent` does, and
//! `cp-agent` calls in here, never the reverse, so a hung `cp-agent` holds nothing (CP-3).
//!
//! [DEC-983]: ../../../docs/project/decisions/DEC-983.md

use mandate_canon::{Digest, Key, Object, Value, to_canonical};
use mandate_cp_agent::{
    HandOver, Ident, Instruction, InstructionSink, Refusal, RefusalReason, Verified,
};
use mandate_time::UtcNanos;

/// The control streams of the workspaces this deployment hosts, as workspace control services
/// write them.
pub trait ControlStreams {
    /// Every workspace the deployment hosts, with its organization.
    fn hosted(&self) -> Vec<(Ident, Ident)>;

    /// Each committed record on `workspace`'s control stream, as `(event_type, payload)`, in
    /// stream order; `None` when the stream cannot be read.
    fn records(&self, workspace: &Ident) -> Option<Vec<(String, Value)>>;

    /// Appends `draft` to `workspace`'s control stream at its head, under the writer's epoch;
    /// `false` when nothing was committed.
    fn append(&mut self, workspace: &Ident, draft: &[u8]) -> bool;
}

/// What an instruction does once journaled. In this slice no instruction has an effect beyond its
/// record (DEC-983 item 4); later stories implement this.
pub trait Effects {
    fn apply(&mut self, workspace: &Ident, verified: &Verified);
}

/// The envelope members a record needs that the instruction does not carry.
pub struct Writer {
    /// `actor.id` and `actor.version`.
    pub id: String,
    pub version: String,
    /// `actor.build`, the binary digest.
    pub build: Digest,
    pub environment: &'static str,
}

/// Workspace control services as `cp-agent`'s [`InstructionSink`].
pub struct Site<S, E, I, C> {
    pub streams: S,
    pub effects: E,
    pub writer: Writer,
    /// A fresh ULID per record.
    pub ids: I,
    pub clock: C,
}

/// How one instruction stands against one stream's records (CP-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    New,
    InEffect,
    Replayed,
}

/// The §9.17 record an instruction is journaled as, its CP-9 kind key, and its identifying digest.
fn record(verified: &Verified) -> (&'static str, String, Object) {
    let key_id = verified.key_id().as_str();
    let document = verified.document_digest().to_hex();
    let mut p = Object::new();
    let (event_type, kind) = match verified.instruction() {
        Instruction::License(l) => {
            put(&mut p, "license_id", text(l.license_id.as_str()));
            put(&mut p, "sequence", int(l.sequence.get()));
            put(&mut p, "org_id", text(l.org_id.as_str()));
            let ids = l.deployment_ids.iter().map(|d| text(d.as_str())).collect();
            put(&mut p, "deployment_ids", Value::Array(ids));
            put(&mut p, "max_workspaces", int(l.max_workspaces));
            put(&mut p, "max_agents_deployed", int(l.max_agents_deployed));
            let features = l.features.iter().map(|f| text(f.as_str())).collect();
            put(&mut p, "features", Value::Array(features));
            put(&mut p, "not_before", text(&l.not_before.to_string()));
            put(&mut p, "not_after", text(&l.not_after.to_string()));
            put(&mut p, "grace_days", int(l.grace_days));
            put(&mut p, "document_digest", text(&document));
            ("LicenseApplied", "license".to_owned())
        }
        Instruction::ReleaseManifest(m) => {
            put(&mut p, "release_id", text(m.release_id.as_str()));
            put(&mut p, "sequence", int(m.sequence.get()));
            put(&mut p, "manifest_digest", text(&document));
            ("ReleaseOffered", "release".to_owned())
        }
        Instruction::ReleaseWithdrawn(w) => {
            put(&mut p, "release_id", text(w.release_id.as_str()));
            put(&mut p, "reason", text(w.reason.as_str()));
            put(&mut p, "document_digest", text(&document));
            (
                "ReleaseWithdrawnNoticed",
                format!("withdrawn:{}", w.release_id.as_str()),
            )
        }
        Instruction::CatalogEntry(c) => {
            put(&mut p, "kind", text(c.kind.as_str()));
            put(&mut p, "entry_id", text(c.entry_id.as_str()));
            put(&mut p, "content_hash", text(&c.content_hash.to_hex()));
            put(&mut p, "sequence", int(c.sequence.get()));
            (
                "CatalogEntryRegistered",
                format!("catalog:{}", c.kind.as_str()),
            )
        }
        Instruction::DataBundle(d) => {
            put(&mut p, "dataset", text(d.dataset.as_str()));
            put(&mut p, "bundle_digest", text(&d.bundle_digest.to_hex()));
            put(&mut p, "sequence", int(d.sequence.get()));
            (
                "DataBundleImported",
                format!("bundle:{}", d.dataset.as_str()),
            )
        }
    };
    put(&mut p, "key_id", text(key_id));
    (event_type, kind, p)
}

/// The CP-9 kind key and identifying digest of a journaled §9.17 record, if it is one.
fn journaled(event_type: &str, p: &Value) -> Option<(String, String)> {
    let member = |m: &str| {
        p.get(m)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match event_type {
        "LicenseApplied" => Some(("license".to_owned(), member("document_digest"))),
        "ReleaseOffered" => Some(("release".to_owned(), member("manifest_digest"))),
        "ReleaseWithdrawnNoticed" => Some((
            format!("withdrawn:{}", member("release_id")),
            member("document_digest"),
        )),
        "CatalogEntryRegistered" => Some((
            format!("catalog:{}", member("kind")),
            format!("{}:{}", member("entry_id"), member("content_hash")),
        )),
        "DataBundleImported" => Some((
            format!("bundle:{}", member("dataset")),
            member("bundle_digest"),
        )),
        _ => None,
    }
}

/// The identity of a new record, as [`journaled`] reads it back: its digest member, and for a
/// catalog entry its `entry_id` with its content hash, since two entries may share content.
fn identity(p: &Object) -> String {
    let member = |m: &str| p.get(m).and_then(Value::as_str).unwrap_or_default();
    if p.contains_key("entry_id") {
        return format!("{}:{}", member("entry_id"), member("content_hash"));
    }
    [
        "document_digest",
        "manifest_digest",
        "content_hash",
        "bundle_digest",
    ]
    .iter()
    .find_map(|m| p.get(*m).and_then(Value::as_str))
    .unwrap_or_default()
    .to_owned()
}

/// Judges an instruction of `kind`, identified by `digest`, with `sequence`, against `records`:
/// in effect when the latest record of its kind has its digest and sequence; replayed when its sequence is not
/// above the highest of its kind (CP-9); otherwise new.
fn standing(
    records: &[(String, Value)],
    kind: &str,
    digest: &str,
    sequence: Option<u64>,
) -> Standing {
    let mine: Vec<(String, Option<u64>)> = records
        .iter()
        .filter_map(|(event_type, p)| {
            let (k, d) = journaled(event_type, p)?;
            (k == kind).then(|| (d, p.get("sequence").and_then(Value::as_int)))
        })
        .collect();
    if mine
        .last()
        .is_some_and(|(d, s)| d == digest && *s == sequence)
    {
        return Standing::InEffect;
    }
    let highest = mine.iter().filter_map(|(_, s)| *s).max();
    match (sequence, highest) {
        (Some(s), Some(h)) if s <= h => Standing::Replayed,
        _ => Standing::New,
    }
}

impl<S, E, I, C> Site<S, E, I, C>
where
    S: ControlStreams,
    E: Effects,
    I: FnMut() -> String,
    C: FnMut() -> UtcNanos,
{
    fn draft(&mut self, event_type: &str, workspace: &Ident, payload: Object) -> Vec<u8> {
        let mut actor = Object::new();
        put(&mut actor, "kind", text("system"));
        put(&mut actor, "id", text(&self.writer.id));
        put(&mut actor, "version", text(&self.writer.version));
        put(
            &mut actor,
            "build",
            text(&format!("sha256:{}", self.writer.build.to_hex())),
        );
        let mut e = Object::new();
        put(&mut e, "envelope_version", int(1));
        put(&mut e, "environment", text(self.writer.environment));
        put(&mut e, "event_id", text(&(self.ids)()));
        put(
            &mut e,
            "stream_id",
            text(&format!("ctl:{}", workspace.as_str())),
        );
        put(&mut e, "event_type", text(event_type));
        put(&mut e, "schema_version", int(1));
        put(&mut e, "event_time", text(&(self.clock)().to_string()));
        put(&mut e, "clock_source", text("local"));
        put(&mut e, "causation_id", Value::Null);
        put(&mut e, "correlation_id", Value::Null);
        put(&mut e, "actor", Value::Object(actor));
        put(&mut e, "config_refs", Value::Object(Object::new()));
        put(&mut e, "payload", Value::Object(payload));
        put(&mut e, "artifact_refs", Value::Array(Vec::new()));
        put(&mut e, "pii_refs", Value::Array(Vec::new()));
        to_canonical(&Value::Object(e))
    }

    /// Journals `refusal` on `workspace` unless its digest is already refused there.
    fn refuse_on(&mut self, workspace: &Ident, refusal: &Refusal) -> HandOver {
        let Some(records) = self.streams.records(workspace) else {
            return HandOver::Unavailable;
        };
        let digest = refusal.document_digest.to_hex();
        let seen = records.iter().any(|(t, p)| {
            t == "ControlPlaneMessageRefused"
                && p.get("document_digest").and_then(Value::as_str) == Some(digest.as_str())
        });
        if seen {
            return HandOver::AlreadyApplied;
        }
        let mut p = Object::new();
        put(&mut p, "message", text(refusal.message.as_str()));
        put(&mut p, "reason", text(refusal.reason.as_str()));
        put(
            &mut p,
            "sequence",
            refusal.sequence.map_or(Value::Null, int),
        );
        put(&mut p, "document_digest", text(&digest));
        let draft = self.draft("ControlPlaneMessageRefused", workspace, p);
        if self.streams.append(workspace, &draft) {
            HandOver::Journaled
        } else {
            HandOver::Unavailable
        }
    }

    fn refuse_everywhere(&mut self, refusal: &Refusal) -> HandOver {
        let outcomes: Vec<HandOver> = self
            .streams
            .hosted()
            .into_iter()
            .map(|(workspace, _)| self.refuse_on(&workspace, refusal))
            .collect();
        combined(&outcomes, HandOver::Refused(refusal.reason))
    }
}

/// One answer for a fan-out: `Unavailable` if no workspace was covered, or any could not be read
/// or written, so `cp-agent` retries and the workspaces already written answer `AlreadyApplied`;
/// else `AlreadyApplied` when no workspace journaled anything new; else `done`.
fn combined(outcomes: &[HandOver], done: HandOver) -> HandOver {
    if outcomes.is_empty() || outcomes.contains(&HandOver::Unavailable) {
        HandOver::Unavailable
    } else if outcomes.iter().all(|o| *o == HandOver::AlreadyApplied) {
        HandOver::AlreadyApplied
    } else {
        done
    }
}

impl<S, E, I, C> InstructionSink for Site<S, E, I, C>
where
    S: ControlStreams,
    E: Effects,
    I: FnMut() -> String,
    C: FnMut() -> UtcNanos,
{
    fn hand_over(&mut self, verified: Verified) -> HandOver {
        let (event_type, kind, payload) = record(&verified);
        let digest = identity(&payload);
        let sequence = verified.instruction().sequence().map(|s| s.get());
        let covered: Vec<Ident> = match verified.instruction() {
            Instruction::License(l) => self
                .streams
                .hosted()
                .into_iter()
                .filter(|(_, org)| *org == l.org_id)
                .map(|(w, _)| w)
                .collect(),
            _ => self.streams.hosted().into_iter().map(|(w, _)| w).collect(),
        };
        let refusal = |reason| Refusal {
            message: verified.message_kind(),
            reason,
            sequence,
            document_digest: verified.document_digest(),
        };
        if covered.is_empty() {
            return self.refuse_everywhere(&refusal(RefusalReason::NotAddressed));
        }
        let mut outcomes = Vec::new();
        let mut replayed = false;
        for workspace in covered {
            let Some(records) = self.streams.records(&workspace) else {
                outcomes.push(HandOver::Unavailable);
                continue;
            };
            match standing(&records, &kind, &digest, sequence) {
                Standing::InEffect => outcomes.push(HandOver::AlreadyApplied),
                Standing::Replayed => {
                    replayed = true;
                    outcomes.push(self.refuse_on(&workspace, &refusal(RefusalReason::Replayed)));
                }
                Standing::New => {
                    let draft = self.draft(event_type, &workspace, payload.clone());
                    if self.streams.append(&workspace, &draft) {
                        self.effects.apply(&workspace, &verified);
                        outcomes.push(HandOver::Journaled);
                    } else {
                        outcomes.push(HandOver::Unavailable);
                    }
                }
            }
        }
        let done = if replayed {
            HandOver::Refused(RefusalReason::Replayed)
        } else {
            HandOver::Journaled
        };
        combined(&outcomes, done)
    }

    fn refused(&mut self, refusal: Refusal) -> HandOver {
        self.refuse_everywhere(&refusal)
    }
}

fn put(object: &mut Object, key: &str, value: Value) {
    if let Ok(key) = Key::new(key) {
        object.insert(key, value);
    }
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u64) -> Value {
    mandate_canon::Int::new(n).map_or(Value::Null, Value::Int)
}
