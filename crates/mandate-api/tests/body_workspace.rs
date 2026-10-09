//! No request body names a workspace (workspace API spec §3.2, API-2, API-9; PRD FR-1.4; HLD §8;
//! E9-8's tenant-isolation layers, #1130; #560 round 1, minor 6). The route's path and the
//! caller's context name the workspace, never a body member, so a body that names one, whether
//! another workspace or its own, is refused before it is read. The inventories are the test's own:
//! the request schemas are listed from `schemas/workspace-api/commands/` at run time, the request
//! types are read from `requests.rs`'s text, and each base body is that schema's example file.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_api::problem::Violation;
use mandate_api::requests::{
    AcknowledgeRequest, ApprovalResponseRequest, ConfirmRequest, DelegationPreviewRequest,
    EndDelegationRequest, HoldRequest, KillSwitchRequest, LiftHoldRequest, OwnerExitRequest,
    OwnerRequest, PauseRequest, ResumeRequest, RevokeRequest, StopRequest,
};
use mandate_api::wire::{Refused, Validate, decode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// The source of the request shapes, read as text for its `pub struct …Request` names.
const REQUESTS: &str = include_str!("../src/requests.rs");

/// The members a careless or hostile client might use to name a workspace.
const WORKSPACE_MEMBERS: [&str; 3] = ["workspace", "workspace_id", "ws"];

/// The workspace the route serves in these tests, and another one.
const OWN: &str = "ws_1";
const OTHER: &str = "ws_other";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/workspace-api")
}

fn json_file(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The stem of every request schema under `commands/`, such as `kill-switch-request`.
fn schema_stems() -> BTreeSet<String> {
    let dir = schemas().join("commands");
    fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter_map(|name| name.strip_suffix(".schema.json").map(str::to_owned))
        .filter(|stem| stem.ends_with("-request"))
        .collect()
}

/// `KillSwitchRequest` as `kill-switch-request`.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Every `pub struct <Name>Request` in `requests.rs`, kebab-cased.
fn crate_stems() -> BTreeSet<String> {
    REQUESTS
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub struct "))
        .filter_map(|rest| rest.split([' ', '{', '(', '<']).next())
        .filter(|name| name.ends_with("Request"))
        .map(kebab)
        .collect()
}

/// What `decode` makes of `body` as `T`: `Ok` when it decodes, or each violation's
/// `(path, code)`.
type Judge = fn(&[u8]) -> Result<(), Vec<(String, String)>>;

fn judge<T: DeserializeOwned + Validate + Debug>(body: &[u8]) -> Result<(), Vec<(String, String)>> {
    match decode::<T>(body) {
        Ok(_) => Ok(()),
        Err(Refused::Invalid { violations }) => Err(violations
            .into_iter()
            .map(|v| match v {
                Violation::Schema { path, code, .. } => (path, code),
                other => panic!("a body's violation is a schema finding: {other:?}"),
            })
            .collect()),
    }
}

/// Each request schema's decoder, by the schema's stem.
const DECODERS: [(&str, Judge); 14] = [
    ("acknowledge-request", judge::<AcknowledgeRequest>),
    (
        "approval-response-request",
        judge::<ApprovalResponseRequest>,
    ),
    ("confirm-request", judge::<ConfirmRequest>),
    (
        "delegation-preview-request",
        judge::<DelegationPreviewRequest>,
    ),
    ("end-delegation-request", judge::<EndDelegationRequest>),
    ("hold-request", judge::<HoldRequest>),
    ("kill-switch-request", judge::<KillSwitchRequest>),
    ("lift-hold-request", judge::<LiftHoldRequest>),
    ("owner-exit-request", judge::<OwnerExitRequest>),
    ("owner-request", judge::<OwnerRequest>),
    ("pause-request", judge::<PauseRequest>),
    ("resume-request", judge::<ResumeRequest>),
    ("revoke-request", judge::<RevokeRequest>),
    ("stop-request", judge::<StopRequest>),
];

fn example(stem: &str) -> Value {
    json_file(&schemas().join(format!("examples/commands.{stem}.json")))
}

/// The decoders cover every request schema and every request type the crate declares, so a shape
/// added to either without a row here fails this test rather than escaping the next one.
#[test]
fn the_decoders_cover_every_request_schema_and_request_type() {
    let listed: BTreeSet<String> = DECODERS.iter().map(|(s, _)| (*s).to_owned()).collect();
    assert_eq!(
        listed,
        schema_stems(),
        "the request schemas under commands/"
    );
    assert_eq!(listed, crate_stems(), "the request types in requests.rs");
}

/// Every request body with a member naming a workspace, another or the route's own, is refused as
/// `unknown_member` at that member and nothing else. The control: each schema's example, the base
/// every case extends, decodes.
#[test]
fn a_request_body_naming_a_workspace_is_refused_at_that_member() {
    let mut wrong = Vec::new();
    for (stem, judge) in DECODERS {
        let base = example(stem);
        let encoded = serde_json::to_vec(&base).expect("json");
        if let Err(found) = judge(&encoded) {
            wrong.push(format!("{stem}'s example is refused: {found:?}"));
        }
        for member in WORKSPACE_MEMBERS {
            for workspace in [OTHER, OWN] {
                let mut body = base.clone();
                body.as_object_mut()
                    .expect("a request example is an object")
                    .insert(member.to_owned(), json!(workspace));
                let encoded = serde_json::to_vec(&body).expect("json");
                let want = vec![(format!("/{member}"), "unknown_member".to_owned())];
                let found = judge(&encoded);
                if found != Err(want.clone()) {
                    wrong.push(format!("{stem} + {member}={workspace}: {found:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Every member name under any `properties` in `node`.
fn property_names(node: &Value, names: &mut BTreeSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                names.extend(properties.keys().cloned());
            }
            map.values().for_each(|child| property_names(child, names));
        }
        Value::Array(items) => items.iter().for_each(|child| property_names(child, names)),
        _ => {}
    }
}

/// No request schema, nor the record or step-up every request may carry, names a member that could
/// hold a workspace, so the schemas give the decoders nothing to read one from.
#[test]
fn no_request_schema_names_a_workspace_member() {
    let mut names = BTreeSet::new();
    for stem in schema_stems() {
        property_names(
            &json_file(&schemas().join(format!("commands/{stem}.schema.json"))),
            &mut names,
        );
    }
    let defs = &json_file(&schemas().join("envelope.schema.json"))["$defs"];
    for shared in ["Record", "StepUpEvidence"] {
        property_names(&defs[shared], &mut names);
    }
    assert!(
        names.contains("scope"),
        "the walk reached the kill switch's members: {names:?}"
    );
    let naming: Vec<&String> = names
        .iter()
        .filter(|name| name.contains("workspace") || name.as_str() == "ws")
        .collect();
    assert!(naming.is_empty(), "{naming:?}");
}

/// The kill switch's workspace scope is the route's workspace, so its `id` is `null` (DEC-682 item
/// 7): naming another workspace, or the route's own, is refused as `type` at `/scope`, where
/// `decode` locates a refusal inside a tagged object (`contract.rs`'s
/// `a_refusal_inside_a_tagged_object_is_located_at_that_object`); `null` decodes.
#[test]
fn a_kill_switch_workspace_scope_names_no_workspace() {
    let body = |id: Value| {
        serde_json::to_vec(&json!({"scope": {"kind": "workspace", "id": id}})).expect("json")
    };
    let judge = judge::<KillSwitchRequest>;
    assert_eq!(judge(&body(Value::Null)), Ok(()));
    for workspace in [OTHER, OWN] {
        assert_eq!(
            judge(&body(json!(workspace))),
            Err(vec![("/scope".to_owned(), "type".to_owned())]),
            "a workspace scope naming {workspace}",
        );
    }
}
