//! The catalogue reads (§4.1, §3.11 item 11, [DEC-2136](../../../docs/project/decisions/DEC-2136.md)):
//! `GET /v1/workspaces/{ws}/instruments` and `…/signal-models`, the registered instruments and
//! signal models a version may name, so a client builds a document the create accepts without
//! inventing an id.
//!
//! Each is authorized for a user by the "Draft a mandate version" row, writes nothing, and folds the
//! control stream exactly as the create does ([`control_facts`]): a row that fold cannot read makes
//! either read `journal_unavailable`, never a list the create would then refuse. The instruments are
//! [`snapshots`], the fold the create's instrument check reads, less those no mandate document can
//! pin; the models are [`registry`], the registry V-007 reads, each with a fixed projection of its
//! stored content object. Nothing about how a model performed is served (DEC-99, DEC-126).

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::{Digest, Value, parse};
use mandate_identity::Permission;
use mandate_journal::{ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_spec::context::registry;

use crate::http::App;
use crate::routes::{
    Reader, Refused, Unreadable, authorized, enveloped, object, path_refused, session, string,
    watermark,
};
use crate::versions::{Snapshot, control_facts, payload, snapshots};

/// The asset classes a mandate document's `instrument_ref` may name.
const PINNABLE_CLASSES: [&str; 2] = ["us_equity", "crypto"];

/// The longest symbol an `instrument_ref` may name, in characters.
const SYMBOL_MAX_CHARS: usize = 32;

/// The groups of an asset id: the mandate schema's lowercase `uuid`.
const ASSET_ID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

/// `GET /v1/workspaces/{ws}/instruments`: `read-models/instruments`.
pub async fn instruments(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let read = match catalogue(&app, &headers, &workspace).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let listed = match listed_instruments(&read.0.control, read.1.as_ref()) {
        Some(listed) => listed,
        None => return Refused::JournalUnavailable.response(),
    };
    served(&read.0, vec![("instruments", Value::Array(listed))])
}

/// `GET /v1/workspaces/{ws}/signal-models`: `read-models/signal-models`.
pub async fn signal_models(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let read = match catalogue(&app, &headers, &workspace).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let listed = match listed_models(&read.0.control, read.1.as_ref()) {
        Some(listed) => listed,
        None => return Refused::JournalUnavailable.response(),
    };
    served(&read.0, vec![("models", Value::Array(listed))])
}

/// The session, then the "Draft a mandate version" row for a user, then the store the create
/// checks against: `journal_unavailable` when the deployment names none, as the create answers.
async fn catalogue(
    app: &App,
    headers: &HeaderMap,
    workspace: &str,
) -> Result<(Reader, Arc<dyn crate::objects::ObjectStore>), Refused> {
    let (admitted, now) = session(app, headers, Request::Other)?;
    let reader = authorized(
        app,
        &admitted,
        now,
        workspace,
        Permission::DraftVersion,
        Unreadable::Read,
    )
    .await?;
    let store = app.objects.clone().ok_or(Refused::JournalUnavailable)?;
    Ok((reader, store))
}

/// A 200 with the envelope and `as_of` naming the control stream.
fn served(reader: &Reader, mut members: Vec<(&str, Value)>) -> Response {
    members.push((
        "as_of",
        Value::Array(watermark(&reader.control).into_iter().collect()),
    ));
    enveloped(StatusCode::OK, reader.now, members)
}

/// The pinnable instruments of [`snapshots`], sorted by symbol then id; `None` when the create's
/// fold cannot read the control stream or any snapshot.
fn listed_instruments(control: &[StoredEvent], store: &dyn ArtifactSource) -> Option<Vec<Value>> {
    control_facts(control, store).ok()?;
    let mut pinnable: Vec<Snapshot> = snapshots(control, store)
        .ok()?
        .into_values()
        .filter(pinnable)
        .collect();
    pinnable.sort_by(|a, b| (&a.symbol, &a.instrument_id).cmp(&(&b.symbol, &b.instrument_id)));
    Some(pinnable.iter().map(instrument_value).collect())
}

/// Whether a mandate document's `instrument_ref` can name `snapshot`: an asset id, a symbol of 1 to
/// 32 characters, and a pinnable asset class.
fn pinnable(snapshot: &Snapshot) -> bool {
    let symbol = snapshot.symbol.chars().count();
    asset_id(&snapshot.instrument_id)
        && (1..=SYMBOL_MAX_CHARS).contains(&symbol)
        && snapshot
            .asset_class
            .as_deref()
            .is_some_and(|class| PINNABLE_CLASSES.contains(&class))
}

/// The mandate schema's `uuid`: `8-4-4-4-12` lowercase hexadecimal.
fn asset_id(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == ASSET_ID_GROUPS.len()
        && groups.iter().zip(ASSET_ID_GROUPS).all(|(group, width)| {
            group.len() == width
                && group
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

fn instrument_value(snapshot: &Snapshot) -> Value {
    object(vec![
        ("instrument_id", string(&snapshot.instrument_id)),
        ("symbol", string(&snapshot.symbol)),
        (
            "asset_class",
            snapshot.asset_class.as_deref().map_or(Value::Null, string),
        ),
        ("snapshot", string(&snapshot.snapshot)),
        ("registered_at", string(&snapshot.registered_at)),
    ])
}

/// The registry V-007 reads, folded from the create's own facts, sorted by id, each model with its
/// registration row's `recorded_at` and its content's projection; `None` when the fold cannot
/// read the control stream.
fn listed_models(control: &[StoredEvent], store: &dyn ArtifactSource) -> Option<Vec<Value>> {
    let facts = control_facts(control, store).ok()?;
    let mut listed = Vec::new();
    for (id, model) in registry(&facts) {
        let registration = control.iter().rev().find(|row| {
            row.event_type == "ConfigSnapshotRegistered"
                && text_member(row, "kind").as_deref() == Some("model_version")
                && text_member(row, "model_id").as_deref() == Some(id.as_str())
        })?;
        let params = model.params.iter().map(|p| string(p)).collect();
        listed.push(object(vec![
            ("model_id", string(id.as_str())),
            ("model_version", string(&model.version)),
            ("content_hash", string(&content_ref(model.content_hash))),
            ("params", Value::Array(params)),
            ("admits_instruments", Value::Bool(model.admits_instruments)),
            ("content", content(store, model.content_hash)),
            ("registered_at", string(&registration.recorded_at)),
        ]));
    }
    Some(listed)
}

fn text_member(row: &StoredEvent, name: &str) -> Option<String> {
    payload(row)
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn content_ref(digest: Digest) -> String {
    format!("sha256:{}", digest.to_hex())
}

/// The fixed projection of the object stored under `digest` (DEC-2136 item 4): `authorship` and
/// `methodology` when they are text, and of `params_schema` only its text `constraints` and each
/// parameter's text `name` and `type` and integer `min` and `max`, member by member, each only when
/// present; `null` when the store cannot give or parse the object.
fn content(store: &dyn ArtifactSource, digest: Digest) -> Value {
    let Some(stored) = get_artifact(store, &ArtifactRef::from_digest(digest))
        .ok()
        .and_then(|bytes| parse(&bytes).ok())
        .filter(|value| value.as_object().is_some())
    else {
        return Value::Null;
    };
    let mut members = Vec::new();
    for name in ["authorship", "methodology"] {
        if let Some(text) = stored.get(name).filter(|v| v.as_str().is_some()) {
            members.push((name, text.clone()));
        }
    }
    if let Some(schema) = stored
        .get("params_schema")
        .filter(|v| v.as_object().is_some())
    {
        members.push(("params_schema", params_schema(schema)));
    }
    object(members)
}

/// `params_schema`'s projection: its `constraints` when every one is text, and its `params` when
/// every one is an object with a text `name`, each with only its `name`, text `type`, and integer
/// `min` and `max`.
fn params_schema(schema: &Value) -> Value {
    let mut members = Vec::new();
    let constraints = schema
        .get("constraints")
        .and_then(Value::as_array)
        .filter(|items| items.iter().all(|item| item.as_str().is_some()));
    if let Some(items) = constraints {
        members.push(("constraints", Value::Array(items.to_vec())));
    }
    let params = schema
        .get("params")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().map(param).collect::<Option<Vec<Value>>>());
    if let Some(items) = params {
        members.push(("params", Value::Array(items)));
    }
    object(members)
}

/// One parameter's projection, or `None` for one that is not an object with a text `name`.
fn param(item: &Value) -> Option<Value> {
    let name = item.get("name").filter(|v| v.as_str().is_some())?;
    let mut members = vec![("name", name.clone())];
    if let Some(kind) = item.get("type").filter(|v| v.as_str().is_some()) {
        members.push(("type", kind.clone()));
    }
    for bound in ["min", "max"] {
        if let Some(value) = item.get(bound).filter(|v| v.as_int().is_some()) {
            members.push((bound, value.clone()));
        }
    }
    Some(object(members))
}
