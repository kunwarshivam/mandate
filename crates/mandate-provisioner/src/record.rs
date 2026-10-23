//! The provisioner's three records (journal spec v0.77 §9.33), as canonical drafts for the control
//! stream: a system actor, the step's cause as `causation_id`, and no credential, role name or
//! reason's detail.

use mandate_canon::{Digest, Int, Key, Value, encode_ulid, to_canonical};
use mandate_deploy_manager::Identity;
use mandate_time::UtcNanos;
use mandate_vault::provisioner::RoleRefusal;

use crate::plan::Step;

/// The provisioner's actor id (DEC-2350; coordinator, 2026-10-22).
pub const ACTOR_ID: &str = "role_provisioner";

/// What the vault did for a step: the roles made, refused, or the exchange's removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Made,
    Refused(RoleRefusal),
    Removed,
}

impl Outcome {
    /// The record the outcome is journaled as.
    #[must_use]
    pub fn event_type(self) -> &'static str {
        match self {
            Self::Made => "ConnectionRolesMade",
            Self::Refused(_) => "ConnectionRolesRefused",
            Self::Removed => "ConnectionRolesRemoved",
        }
    }
}

/// The record's `event_id`: a ULID derived from the stream, the record, the connection and the
/// step's cause, so a retried draft, or a second provisioner's of the same step, is the same event
/// and the journal commits it once (journal spec §5.1).
#[must_use]
pub fn event_id(identity: &Identity, step: &Step, outcome: Outcome) -> String {
    let stream = identity.stream();
    let (connection, cause) = parts(step);
    let digest = Digest::of_parts(&[
        stream.as_bytes(),
        outcome.event_type().as_bytes(),
        connection.as_bytes(),
        cause.as_bytes(),
    ]);
    let mut high = [0_u8; 16];
    for (to, from) in high.iter_mut().zip(digest.as_bytes()) {
        *to = *from;
    }
    encode_ulid(u128::from_be_bytes(high))
}

/// The canonical draft of `step`'s `outcome` for `identity`'s control stream in `environment`, at
/// `now`: its connection, and for a refusal its reason, for a removal the exchange's role and the
/// occasion; caused by the request answered, or by the closer or revocation removed for.
#[must_use]
pub fn draft(
    identity: &Identity,
    step: &Step,
    outcome: Outcome,
    environment: &str,
    now: UtcNanos,
) -> Vec<u8> {
    let (connection, cause) = parts(step);
    let mut payload = vec![("connection_id", text(connection))];
    match (outcome, step) {
        (Outcome::Refused(refusal), _) => payload.push(("reason", text(refusal.reason()))),
        (Outcome::Removed, Step::Remove { occasion, .. }) => {
            payload.push(("role", text("token_exchange")));
            payload.push(("occasion", text(occasion.code())));
        }
        (Outcome::Made | Outcome::Removed, _) => {}
    }
    let actor = object(vec![
        ("kind", text("system")),
        ("id", text(&identity.actor_id)),
        ("version", text(&identity.version)),
        (
            "build",
            text(&format!("sha256:{}", identity.build.to_hex())),
        ),
    ]);
    to_canonical(&object(vec![
        ("envelope_version", int(1)),
        ("environment", text(environment)),
        ("event_id", text(&event_id(identity, step, outcome))),
        ("stream_id", text(&identity.stream())),
        ("event_type", text(outcome.event_type())),
        ("schema_version", int(1)),
        ("event_time", text(&now.to_string())),
        ("clock_source", text("local")),
        ("causation_id", text(cause)),
        ("correlation_id", Value::Null),
        ("actor", actor),
        ("config_refs", object(Vec::new())),
        ("payload", object(payload)),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
    ]))
}

/// A step's connection and cause.
fn parts(step: &Step) -> (&str, &str) {
    match step {
        Step::Make {
            connection,
            request,
        } => (connection, request),
        Step::Remove {
            connection, cause, ..
        } => (connection, cause),
    }
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// An integer member; one the canonical form cannot hold becomes `null`, which the journal
/// refuses, so the record is never committed with another number.
fn int(value: u64) -> Value {
    Int::new(value).map_or(Value::Null, Value::Int)
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .filter_map(|(key, value)| Some((Key::new(key).ok()?, value)))
            .collect(),
    )
}
