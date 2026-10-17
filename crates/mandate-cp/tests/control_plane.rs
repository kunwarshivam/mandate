//! The global control plane's first slice (E21-43 with E20-1, E20-2, E20-5; DEC-984) through
//! [`ControlPlane`] over the [`MemoryStore`]:
//!
//! - E20-1: the directory holds IDs and role names only; no record has a field for an email, a
//!   person's name, or an identity-provider subject;
//! - E20-2: a single-use, expiring token binds the key generated on site, once; a reused or
//!   expired token, a second enrollment, and a creation over an existing deployment are refused;
//! - E20-5: a deployment silent for a day is shown `unreachable` and nothing about it is revoked;
//! - CP-11: a deployment reads and writes only its own records, and no call is accepted twice.

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_cp::{
    ControlPlane, Deployment, DeploymentMode, Endpoint, FleetRow, Health, Ident, Member,
    MemberState, MemoryStore, Operation, Reachability, Refused, Route, Signed, Store, Workspace,
    role_name, signed_request_bytes,
};
use mandate_identity::Role;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

const CELL: &str = "dep_cell1";
const SITE: &str = "dep_site1";

fn id(text: &str) -> Ident {
    Ident::parse(text).unwrap()
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(1_792_000_000 + secs, 0).unwrap()
}

fn pair(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn deployment(deployment_id: &str, mode: DeploymentMode, endpoint: &str) -> Deployment {
    Deployment {
        deployment_id: id(deployment_id),
        owner_org: id(if mode == DeploymentMode::Hybrid {
            "org_s"
        } else {
            "org_platform"
        }),
        mode,
        endpoint: Endpoint::parse(endpoint).unwrap(),
        public_key: None,
    }
}

/// A plane with the cell and the hybrid site created and their tokens returned, nothing enrolled.
fn plane() -> (ControlPlane<MemoryStore>, String, String) {
    let mut plane = ControlPlane {
        store: MemoryStore::default(),
    };
    let cell = deployment(CELL, DeploymentMode::ManagedCell, "https://cell1.example");
    let site = deployment(
        SITE,
        DeploymentMode::Hybrid,
        "https://site.example:8443/api",
    );
    let cell = plane.create_deployment(cell, [1; 32], at(600)).unwrap();
    let site = plane.create_deployment(site, [2; 32], at(600)).unwrap();
    (plane, cell, site)
}

fn workspace(workspace_id: &str, org: &str, deployment_id: &str) -> Workspace {
    Workspace {
        workspace_id: id(workspace_id),
        org_id: id(org),
        deployment_id: id(deployment_id),
    }
}

/// Both deployments enrolled (the cell with key 7, the site with key 8), and the workspaces
/// registered: `ws_a` and `ws_b` of org A and `ws_c` of org C on the cell, `ws_s` of the site's
/// own org on the site.
fn enrolled() -> ControlPlane<MemoryStore> {
    let (mut plane, cell, site) = plane();
    plane
        .enroll(&id(CELL), &cell, pair(7).public_key().as_ref(), at(0))
        .unwrap();
    plane
        .enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(0))
        .unwrap();
    for w in [
        workspace("ws_a", "org_a", CELL),
        workspace("ws_b", "org_a", CELL),
        workspace("ws_c", "org_c", CELL),
        workspace("ws_s", "org_s", SITE),
    ] {
        plane.register_workspace(w).unwrap();
    }
    plane
}

fn canonical(json: &str) -> Vec<u8> {
    to_canonical(&parse(json.as_bytes()).unwrap())
}

fn signed(
    deployment_id: &str,
    key: &Ed25519KeyPair,
    operation: Operation,
    body: &[u8],
    when: UtcNanos,
) -> Signed {
    let (method, path) = operation.route();
    let bytes = signed_request_bytes(method, path, &id(deployment_id), when, body);
    Signed {
        deployment_id: id(deployment_id),
        method: method.to_owned(),
        path: path.to_owned(),
        at: when,
        body: body.to_vec(),
        signature: key.sign(&bytes).as_ref().to_vec(),
    }
}

fn heartbeat_body(health: &str) -> Vec<u8> {
    canonical(&format!(
        r#"{{"release_digest":"{}","components":[{{"name":"api","version":"0.1.0","health":"ok"}},{{"name":"executor","version":"0.1.0+abc","health":"{health}"}}]}}"#,
        Digest::of(b"release 2026.10.17").to_hex()
    ))
}

fn members_body(members: &[(&str, &str, &str)]) -> Vec<u8> {
    let items: Vec<String> = members
        .iter()
        .map(|(user, workspace, state)| {
            format!(r#"{{"user_id":"{user}","workspace_id":"{workspace}","role":"approver","state":"{state}"}}"#)
        })
        .collect();
    canonical(&format!(r#"{{"members":[{}]}}"#, items.join(",")))
}

fn user_body(user: &str) -> Vec<u8> {
    canonical(&format!(r#"{{"user_id":"{user}"}}"#))
}

#[test]
fn a_token_enrolls_once_before_it_expires() {
    let (mut plane, cell, site) = plane();
    let key = pair(7);
    let enrolled = plane
        .enroll(&id(CELL), &cell, key.public_key().as_ref(), at(0))
        .unwrap();
    assert_eq!(enrolled.deployment_id, id(CELL));
    assert_eq!(enrolled.mode, DeploymentMode::ManagedCell);
    assert_eq!(
        enrolled.key_fingerprint,
        Digest::of(key.public_key().as_ref())
    );
    assert_eq!(
        plane.enroll(&id(CELL), &cell, key.public_key().as_ref(), at(1)),
        Err(Refused::TokenUsed)
    );
    assert_eq!(
        plane.enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(600)),
        Err(Refused::TokenExpired),
        "a token is refused at its expiry"
    );
    assert_eq!(
        plane.enroll(&id(SITE), &cell, pair(8).public_key().as_ref(), at(0)),
        Err(Refused::UnknownToken)
    );
    assert_eq!(
        plane.enroll(&id(SITE), "not hex", pair(8).public_key().as_ref(), at(0)),
        Err(Refused::UnknownToken)
    );
    assert_eq!(
        plane.enroll(&id(SITE), &site, &[0; 31], at(0)),
        Err(Refused::BadKey)
    );
    assert_eq!(
        plane.enroll(&id(SITE), &site, key.public_key().as_ref(), at(0)),
        Err(Refused::BadKey),
        "one key is bound to one deployment"
    );
    assert!(
        plane
            .enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(599))
            .is_ok()
    );
}

/// Review of G1a: a bound key is replaced only through a revocation, and no creation can take
/// over, re-key or un-enroll an existing deployment.
#[test]
fn a_bound_key_is_replaced_only_after_a_revocation() {
    let mut plane = enrolled();
    let over = deployment(CELL, DeploymentMode::Hybrid, "https://attacker.example");
    assert_eq!(
        plane.create_deployment(over, [9; 32], at(600)),
        Err(Refused::Exists)
    );
    assert_eq!(
        plane.issue_token(&id(CELL), [9; 32], at(600)),
        Err(Refused::AlreadyEnrolled)
    );
    let heartbeat = signed(
        CELL,
        &pair(7),
        Operation::Heartbeat,
        &heartbeat_body("ok"),
        at(1),
    );
    assert_eq!(
        plane.heartbeat(&heartbeat, at(1)),
        Ok(()),
        "still the cell's own key"
    );
    plane.revoke(&id(CELL)).unwrap();
    let after = signed(
        CELL,
        &pair(7),
        Operation::Heartbeat,
        &heartbeat_body("ok"),
        at(2),
    );
    assert_eq!(
        plane.heartbeat(&after, at(2)),
        Err(Refused::NotEnrolled),
        "revoked: an outage"
    );
    let token = plane.issue_token(&id(CELL), [9; 32], at(600)).unwrap();
    plane
        .enroll(&id(CELL), &token, pair(17).public_key().as_ref(), at(3))
        .unwrap();
    let rekeyed = signed(
        CELL,
        &pair(17),
        Operation::Heartbeat,
        &heartbeat_body("ok"),
        at(4),
    );
    assert_eq!(plane.heartbeat(&rekeyed, at(4)), Ok(()));
    assert_eq!(
        plane.revoke(&id("dep_nowhere")),
        Err(Refused::UnknownDeployment)
    );
}

#[test]
fn only_the_enrolled_key_within_the_skew_authenticates_a_call() {
    let (mut plane, cell, _) = plane();
    let body = heartbeat_body("ok");
    let call = |when: UtcNanos| signed(CELL, &pair(7), Operation::Heartbeat, &body, when);
    assert_eq!(
        plane.heartbeat(&call(at(0)), at(0)),
        Err(Refused::NotEnrolled)
    );
    plane
        .enroll(&id(CELL), &cell, pair(7).public_key().as_ref(), at(0))
        .unwrap();
    assert_eq!(plane.heartbeat(&call(at(-301)), at(0)), Err(Refused::Stale));
    assert_eq!(plane.heartbeat(&call(at(-300)), at(0)), Ok(()));
    assert_eq!(plane.heartbeat(&call(at(301)), at(0)), Err(Refused::Stale));
    assert_eq!(plane.heartbeat(&call(at(300)), at(0)), Ok(()));
    let forged = signed(CELL, &pair(8), Operation::Heartbeat, &body, at(301));
    assert_eq!(
        plane.heartbeat(&forged, at(301)),
        Err(Refused::BadSignature)
    );
    let mut tampered = call(at(302));
    tampered.body = heartbeat_body("down");
    assert_eq!(
        plane.heartbeat(&tampered, at(302)),
        Err(Refused::BadSignature)
    );
    let unknown = signed(
        "dep_nowhere",
        &pair(7),
        Operation::Heartbeat,
        &body,
        at(303),
    );
    assert_eq!(
        plane.heartbeat(&unknown, at(303)),
        Err(Refused::UnknownDeployment)
    );
}

/// Review of G1a: a call is accepted once, and never an older one after a newer one, so a
/// captured sync cannot roll a deactivation back.
#[test]
fn a_replayed_or_older_call_is_refused() {
    let mut plane = enrolled();
    let deactivated = members_body(&[("user_1", "ws_a", "deactivated")]);
    let active = members_body(&[("user_1", "ws_a", "active")]);
    let old = signed(CELL, &pair(7), Operation::Directory, &active, at(10));
    let new = signed(CELL, &pair(7), Operation::Directory, &deactivated, at(20));
    assert_eq!(plane.sync_directory(&old, at(20)), Ok(()));
    assert_eq!(plane.sync_directory(&new, at(20)), Ok(()));
    assert_eq!(
        plane.sync_directory(&old, at(21)),
        Err(Refused::Stale),
        "an older call after a newer one"
    );
    assert_eq!(
        plane.sync_directory(&new, at(21)),
        Err(Refused::Stale),
        "the same call twice"
    );
    let states: Vec<MemberState> = plane.store.members().into_iter().map(|m| m.state).collect();
    assert_eq!(states, [MemberState::Deactivated]);
}

/// Review of G1a: a call's body is decoded in the core, so what is acted on is what was signed,
/// and a call is accepted only at its own route.
#[test]
fn a_call_is_its_own_operation_and_its_body_its_closed_canonical_shape() {
    let mut plane = enrolled();
    let body = heartbeat_body("ok");
    let mut moved = signed(CELL, &pair(7), Operation::Directory, &body, at(1));
    assert_eq!(
        plane.heartbeat(&moved, at(1)),
        Err(Refused::Malformed),
        "a directory call sent as a heartbeat"
    );
    moved.method = "PUT".to_owned();
    assert_eq!(plane.sync_directory(&moved, at(1)), Err(Refused::Malformed));
    let bad_bodies = [
        b"{}".to_vec(),
        b"{ \"members\":[]}".to_vec(),
        canonical(r#"{"members":[],"extra":1}"#),
        members_body(&[
            ("user_1", "ws_a", "active"),
            ("user_1", "ws_a", "deactivated"),
        ]),
        canonical(
            r#"{"members":[{"user_id":"u","workspace_id":"ws_a","role":"owner","state":"active"}]}"#,
        ),
        canonical(
            r#"{"members":[{"user_id":"u","workspace_id":"ws_a","role":"approver","state":"active","email":"x"}]}"#,
        ),
    ];
    for (i, bad) in bad_bodies.into_iter().enumerate() {
        let call = signed(
            CELL,
            &pair(7),
            Operation::Directory,
            &bad,
            at(2 + i64::try_from(i).unwrap()),
        );
        assert_eq!(
            plane.sync_directory(&call, at(10)),
            Err(Refused::Malformed),
            "{}",
            String::from_utf8_lossy(&bad)
        );
    }
    let twice = canonical(&format!(
        r#"{{"release_digest":"{}","components":[{{"name":"api","version":"1","health":"ok"}},{{"name":"api","version":"2","health":"ok"}}]}}"#,
        "a".repeat(64)
    ));
    let call = signed(CELL, &pair(7), Operation::Heartbeat, &twice, at(20));
    assert_eq!(
        plane.heartbeat(&call, at(20)),
        Err(Refused::Malformed),
        "a component named twice"
    );
}

/// E20-5 and CP-8: a deployment silent for a day is shown `unreachable`; it stays enrolled and
/// its next call authenticates, so nothing was revoked.
#[test]
fn a_deployment_silent_for_a_day_is_unreachable_and_nothing_is_revoked() {
    let mut plane = enrolled();
    let call = signed(
        CELL,
        &pair(7),
        Operation::Heartbeat,
        &heartbeat_body("degraded"),
        at(0),
    );
    plane.heartbeat(&call, at(0)).unwrap();
    let row = |plane: &ControlPlane<MemoryStore>, now: UtcNanos| -> FleetRow {
        plane
            .fleet(now)
            .into_iter()
            .find(|r| r.deployment_id == id(CELL))
            .unwrap()
    };
    let fresh = row(&plane, at(180));
    assert_eq!(fresh.reachability, Reachability::Reachable);
    assert_eq!(
        fresh.health,
        Some(Health::Degraded),
        "the worst component's health"
    );
    assert_eq!(
        fresh.release_digest,
        Some(Digest::of(b"release 2026.10.17"))
    );
    assert_eq!(row(&plane, at(181)).reachability, Reachability::Unreachable);
    let day = at(86_400);
    let silent = row(&plane, day);
    assert_eq!(silent.reachability, Reachability::Unreachable);
    assert!(silent.enrolled, "nothing is revoked");
    let late = signed(
        CELL,
        &pair(7),
        Operation::Heartbeat,
        &heartbeat_body("ok"),
        day,
    );
    assert_eq!(plane.heartbeat(&late, day), Ok(()));
    assert_eq!(row(&plane, day).reachability, Reachability::Reachable);
    let site = plane
        .fleet(day)
        .into_iter()
        .find(|r| r.deployment_id == id(SITE))
        .unwrap();
    assert_eq!(site.reachability, Reachability::NeverSeen);
}

/// CP-11: workspaces are placed by an org admin, a hybrid site hosts only its own organization's,
/// and a deployment's sync names only members of workspaces registered on it.
#[test]
fn a_deployment_writes_only_its_own_directory() {
    let mut plane = enrolled();
    assert_eq!(
        plane.register_workspace(workspace("ws_x", "org_a", SITE)),
        Err(Refused::NotYours),
        "a hybrid site hosts its own org only"
    );
    assert_eq!(
        plane.register_workspace(workspace("ws_a", "org_a", SITE)),
        Err(Refused::NotYours)
    );
    assert_eq!(
        plane.register_workspace(workspace("ws_a", "org_s", SITE)),
        Err(Refused::Exists)
    );
    assert_eq!(
        plane.register_workspace(workspace("ws_y", "org_y", "dep_nowhere")),
        Err(Refused::UnknownDeployment)
    );
    let cell = signed(
        CELL,
        &pair(7),
        Operation::Directory,
        &members_body(&[("user_1", "ws_a", "active")]),
        at(1),
    );
    plane.sync_directory(&cell, at(1)).unwrap();
    let site = signed(
        SITE,
        &pair(8),
        Operation::Directory,
        &members_body(&[("user_2", "ws_s", "active")]),
        at(1),
    );
    plane.sync_directory(&site, at(1)).unwrap();
    let theirs = signed(
        SITE,
        &pair(8),
        Operation::Directory,
        &members_body(&[("user_9", "ws_a", "active")]),
        at(2),
    );
    assert_eq!(plane.sync_directory(&theirs, at(2)), Err(Refused::NotYours));
    let unregistered = signed(
        SITE,
        &pair(8),
        Operation::Directory,
        &members_body(&[("user_9", "ws_new", "active")]),
        at(3),
    );
    assert_eq!(
        plane.sync_directory(&unregistered, at(3)),
        Err(Refused::NotYours)
    );
    let empty = signed(
        CELL,
        &pair(7),
        Operation::Directory,
        &members_body(&[]),
        at(4),
    );
    plane.sync_directory(&empty, at(4)).unwrap();
    let held: Vec<Ident> = plane
        .store
        .members()
        .into_iter()
        .map(|m| m.user_id)
        .collect();
    assert_eq!(
        held,
        [id("user_2")],
        "the cell's sync replaced its own members only"
    );
}

/// Routing, and its scope (review of G1a): a deployment asks only for a user it hosts, and learns
/// only that user's workspaces in the organizations where it hosts them.
#[test]
fn a_hosted_user_is_routed_within_their_organizations_only() {
    let mut plane = enrolled();
    let cell = signed(
        CELL,
        &pair(7),
        Operation::Directory,
        &members_body(&[
            ("user_1", "ws_a", "active"),
            ("user_1", "ws_b", "deactivated"),
            ("user_1", "ws_c", "active"),
            ("user_2", "ws_b", "active"),
        ]),
        at(1),
    );
    plane.sync_directory(&cell, at(1)).unwrap();
    let site = signed(
        SITE,
        &pair(8),
        Operation::Directory,
        &members_body(&[("user_1", "ws_s", "active")]),
        at(1),
    );
    plane.sync_directory(&site, at(1)).unwrap();
    let cell_route = |w: &str| Route {
        workspace_id: id(w),
        endpoint: Endpoint::parse("https://cell1.example").unwrap(),
    };
    let ask = signed(
        CELL,
        &pair(7),
        Operation::Routes,
        &user_body("user_1"),
        at(2),
    );
    assert_eq!(
        plane.routes(&ask, at(2)).unwrap(),
        [cell_route("ws_a"), cell_route("ws_c")]
    );
    let from_site = signed(
        SITE,
        &pair(8),
        Operation::Routes,
        &user_body("user_1"),
        at(2),
    );
    assert_eq!(
        plane.routes(&from_site, at(2)).unwrap(),
        [Route {
            workspace_id: id("ws_s"),
            endpoint: Endpoint::parse("https://site.example:8443/api").unwrap()
        }],
        "the site learns nothing of the cell's organizations"
    );
    let stranger = signed(
        SITE,
        &pair(8),
        Operation::Routes,
        &user_body("user_2"),
        at(3),
    );
    assert_eq!(plane.routes(&stranger, at(3)), Err(Refused::NotYours));
}

/// E20-1: every record the plane holds is written out field by field, with no `..`, so a field
/// added later fails to compile here until it is judged; none holds personal data.
#[test]
fn no_record_has_a_field_for_personal_data() {
    let plane = enrolled();
    for d in plane.store.deployments() {
        let Deployment {
            deployment_id,
            owner_org,
            mode,
            endpoint,
            public_key,
        } = d;
        let _: (Ident, Ident, DeploymentMode, Endpoint, Option<[u8; 32]>) =
            (deployment_id, owner_org, mode, endpoint, public_key);
    }
    for w in plane.store.workspaces() {
        let Workspace {
            workspace_id,
            org_id,
            deployment_id,
        } = w;
        let _: (Ident, Ident, Ident) = (workspace_id, org_id, deployment_id);
    }
    let Member {
        user_id,
        workspace_id,
        role,
        state,
    } = Member {
        user_id: id("user_1"),
        workspace_id: id("ws_a"),
        role: Role::Viewer,
        state: MemberState::Active,
    };
    let _: (Ident, Ident, Role, MemberState) = (user_id, workspace_id, role, state);
    for name in Role::ALL.map(role_name) {
        assert!(
            name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{name} is a role name"
        );
    }
    let sync = members_body(&[("user_1", "ws_a", "active")]);
    let fields: Vec<String> = parse(&sync)
        .unwrap()
        .get("members")
        .and_then(Value::as_array)
        .unwrap()[0]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str().to_owned())
        .collect();
    assert_eq!(fields, ["role", "state", "user_id", "workspace_id"]);
}

#[test]
fn an_id_is_one_to_64_of_the_journals_id_characters() {
    for good in ["a", "dep_cell1", "A-b_9", &"x".repeat(64)] {
        assert_eq!(
            Ident::parse(good).map(|i| i.as_str().to_owned()),
            Some(good.to_owned()),
            "{good}"
        );
    }
    for bad in ["", "a.b", "a b", "a@b", "é", &"x".repeat(65)] {
        assert!(Ident::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn an_endpoint_is_an_https_url_naming_a_host_only() {
    for good in ["https://cell1.example", "https://a-b.example:8443/api/v1"] {
        assert_eq!(
            Endpoint::parse(good).map(|e| e.as_str().to_owned()),
            Some(good.to_owned()),
            "{good}"
        );
    }
    for bad in [
        "http://cell1.example",
        "https://",
        "https://user@cell1.example",
        "https://cell1.example/?q=1",
        "https://cell1.example/#x",
        "https://cell 1.example",
        "https://cell1.example\\x",
    ] {
        assert!(Endpoint::parse(bad).is_none(), "{bad}");
    }
    assert!(Endpoint::parse(&format!("https://{}", "a".repeat(248))).is_some());
    assert!(Endpoint::parse(&format!("https://{}", "a".repeat(249))).is_none());
}

/// Only enrollment binds a key: a key handed to the plane when the deployment is created is
/// dropped, so a deployment never authenticates without spending its token.
#[test]
fn a_key_given_at_creation_is_not_bound() {
    let mut plane = ControlPlane {
        store: MemoryStore::default(),
    };
    let pre_keyed = Deployment {
        public_key: Some(pair(7).public_key().as_ref().try_into().unwrap()),
        ..deployment(CELL, DeploymentMode::ManagedCell, "https://cell1.example")
    };
    plane
        .create_deployment(pre_keyed, [1; 32], at(600))
        .unwrap();
    let call = signed(
        CELL,
        &pair(7),
        Operation::Heartbeat,
        &heartbeat_body("ok"),
        at(0),
    );
    assert_eq!(plane.heartbeat(&call, at(0)), Err(Refused::NotEnrolled));
}

proptest! {
    /// No call signed by anything but the deployment's own enrolled key authenticates.
    #[test]
    fn only_the_enrolled_key_authenticates(seed in 9u8..=255, skew in -300i64..=300) {
        let mut plane = enrolled();
        let body = heartbeat_body("ok");
        let forged = signed(CELL, &pair(seed), Operation::Heartbeat, &body, at(skew));
        prop_assert_eq!(plane.heartbeat(&forged, at(0)), Err(Refused::BadSignature));
        let genuine = signed(CELL, &pair(7), Operation::Heartbeat, &body, at(skew));
        prop_assert_eq!(plane.heartbeat(&genuine, at(0)), Ok(()));
    }
}
