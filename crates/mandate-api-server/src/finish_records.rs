//! The records the connect's finish appends ([DEC-2342](../../../docs/project/decisions/DEC-2342.md)
//! item 9): `ConnectionEstablished` version 2 or `ConnectionRefused` version 1 on the workspace's
//! control stream, written by the connection manager as a `system` actor, under an event id derived
//! from the request so a retried append writes the same event.

use mandate_canon::{Digest, Int, Value, encode_ulid, to_canonical};
use mandate_connections::manager::{ManagerEffect, RequestMembers};
use mandate_time::UtcNanos;

use crate::routes::BUILD;
use crate::versions::{object, text};

/// The finish's actor id.
pub const ACTOR_ID: &str = "connection_manager";

/// The operation the finish's event id is derived under.
pub const OPERATION: &str = "connect_finish";

/// The finish's event id for `request`: the ULID of the first 128 bits of the SHA-256 of the
/// canonical `{account_ref, connection_id, operation}`. An `account_ref` names one request only, so
/// each request has one.
pub fn event_id(request: &RequestMembers) -> Option<String> {
    let derivation = object(vec![
        ("account_ref", text(request.account_ref.as_str())),
        ("connection_id", text(request.connection_id.as_str())),
        ("operation", text(OPERATION)),
    ])?;
    let digest = Digest::of(&to_canonical(&derivation));
    let (head, _) = digest.as_bytes().split_first_chunk::<16>()?;
    Some(encode_ulid(u128::from_be_bytes(*head)))
}

/// The draft of `effect` on `workspace`'s control stream at `now`, with `causation`; `None` for an
/// effect the finish never writes (`Requested`) or a value the canonical form cannot hold.
pub fn drafted(
    effect: &ManagerEffect,
    causation: Option<&str>,
    workspace: &str,
    now: UtcNanos,
) -> Option<Vec<u8>> {
    let (event_type, schema_version, request, payload) = match effect {
        ManagerEffect::Requested(_) => return None,
        ManagerEffect::Established { request, scopes } => {
            let payload = object(vec![
                ("account_ref", text(request.account_ref.as_str())),
                ("broker", text(request.broker.code())),
                ("connection_id", text(request.connection_id.as_str())),
                ("environment", text(request.environment.as_str())),
                ("margin_attestation", Value::Null),
                (
                    "scopes",
                    Value::Array(scopes.iter().map(|scope| text(scope)).collect()),
                ),
                ("step_up", step_up(request)?),
                ("user", text(&request.user)),
            ])?;
            ("ConnectionEstablished", 2, request, payload)
        }
        ManagerEffect::ConnectRefused { request, refused } => {
            let payload = object(vec![
                ("broker", text(request.broker.code())),
                ("check", refused.check.map_or(Value::Null, text)),
                ("connection_id", text(request.connection_id.as_str())),
                ("environment", text(request.environment.as_str())),
                (
                    "existing_connection_id",
                    refused
                        .existing_connection_id
                        .as_ref()
                        .map_or(Value::Null, |id| text(id.as_str())),
                ),
                ("occasion", text("connect")),
                ("reason", text(refused.reason)),
                ("step_up", step_up(request)?),
                ("user", text(&request.user)),
            ])?;
            ("ConnectionRefused", 1, request, payload)
        }
    };
    let actor = object(vec![
        ("build", text(BUILD)),
        ("id", text(ACTOR_ID)),
        ("kind", text("system")),
        ("version", text(env!("CARGO_PKG_VERSION"))),
    ])?;
    let envelope = object(vec![
        ("actor", actor),
        ("artifact_refs", Value::Array(Vec::new())),
        ("causation_id", causation.map_or(Value::Null, text)),
        ("clock_source", text("local")),
        ("config_refs", object(Vec::new())?),
        ("correlation_id", Value::Null),
        ("environment", text(request.environment.as_str())),
        ("envelope_version", Value::Int(Int::new(1)?)),
        ("event_id", text(&event_id(request)?)),
        ("event_time", text(&now.to_string())),
        ("event_type", text(event_type)),
        ("payload", payload),
        ("pii_refs", Value::Array(Vec::new())),
        ("schema_version", Value::Int(Int::new(schema_version)?)),
        ("stream_id", text(&format!("ctl:{workspace}"))),
    ])?;
    Some(to_canonical(&envelope))
}

/// The request's step-up evidence as journal spec §9.8 writes it.
fn step_up(request: &RequestMembers) -> Option<Value> {
    object(vec![
        ("assertion_id", text(&request.step_up.assertion_id)),
        (
            "authenticated_at",
            text(&request.step_up.authenticated_at.to_string()),
        ),
        ("method", text(&request.step_up.method)),
    ])
}
