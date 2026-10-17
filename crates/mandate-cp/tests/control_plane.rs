//! The global control plane's first slice (E21-43 with E20-1, E20-2, E20-5; DEC-984) through
//! [`ControlPlane`] over the [`MemoryStore`]:
//!
//! - E20-1: the directory holds IDs and role names only; no record has a field for an email, a
//!   person's name, or an identity-provider subject;
//! - E20-2: a single-use, expiring token binds the key generated on site; a reused or expired
//!   token is refused;
//! - E20-5: a deployment silent for a day is shown `unreachable` and nothing about it is revoked;
//! - CP-11: a deployment reads and writes only its own records.

use mandate_canon::Digest;
use mandate_cp::{
    Component, ControlPlane, Deployment, DeploymentMode, Endpoint, FleetRow, Health, Heartbeat,
    Ident, Member, MemberState, MemoryStore, Reachability, Refused, Route, Signed, Store,
    Workspace, signed_request_bytes,
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

fn deployment(deployment_id: &str, endpoint: &str) -> Deployment {
    Deployment {
        deployment_id: id(deployment_id),
        owner_org: id("org_platform"),
        mode: DeploymentMode::ManagedCell,
        endpoint: Endpoint::parse(endpoint).unwrap(),
        public_key: None,
    }
}

/// A plane with the cell and the site created, their tokens returned, nothing enrolled.
fn plane() -> (ControlPlane<MemoryStore>, String, String) {
    let mut plane = ControlPlane {
        store: MemoryStore::default(),
    };
    let cell = plane.create_deployment(deployment(CELL, "https://cell1.example"), [1; 32], at(600));
    let site = plane.create_deployment(
        deployment(SITE, "https://site.example:8443/api"),
        [2; 32],
        at(600),
    );
    (plane, cell, site)
}

/// The plane with both deployments enrolled, the cell with key 7 and the site with key 8.
fn enrolled() -> ControlPlane<MemoryStore> {
    let (mut plane, cell, site) = plane();
    plane
        .enroll(&id(CELL), &cell, pair(7).public_key().as_ref(), at(0))
        .unwrap();
    plane
        .enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(0))
        .unwrap();
    plane
}

fn signed(
    deployment_id: &str,
    key: &Ed25519KeyPair,
    path: &str,
    body: &[u8],
    when: UtcNanos,
) -> Signed {
    let bytes = signed_request_bytes("POST", path, &id(deployment_id), when, body);
    Signed {
        deployment_id: id(deployment_id),
        method: "POST".to_owned(),
        path: path.to_owned(),
        at: when,
        body: body.to_vec(),
        signature: key.sign(&bytes).as_ref().to_vec(),
    }
}

fn heartbeat(health: Health) -> Heartbeat {
    Heartbeat {
        release_digest: Digest::of(b"release 2026.10.17"),
        components: vec![
            Component::new(id("api"), "0.1.0", Health::Ok).unwrap(),
            Component::new(id("executor"), "0.1.0+abc", health).unwrap(),
        ],
    }
}

fn workspace(workspace_id: &str, org: &str, deployment_id: &str) -> Workspace {
    Workspace {
        workspace_id: id(workspace_id),
        org_id: id(org),
        deployment_id: id(deployment_id),
    }
}

fn member(user: &str, workspace_id: &str, state: MemberState) -> Member {
    Member {
        user_id: id(user),
        workspace_id: id(workspace_id),
        role: Role::Approver,
        state,
    }
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
        Err(Refused::TokenUsed),
        "a reused token is refused"
    );
    assert_eq!(
        plane.enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(600)),
        Err(Refused::TokenExpired),
        "a token is refused at its expiry"
    );
    assert_eq!(
        plane.enroll(&id(SITE), &cell, pair(8).public_key().as_ref(), at(0)),
        Err(Refused::UnknownToken),
        "a token binds only its own deployment"
    );
    assert_eq!(
        plane.enroll(&id(SITE), "not hex", pair(8).public_key().as_ref(), at(0)),
        Err(Refused::UnknownToken)
    );
    assert_eq!(
        plane.enroll(&id(SITE), &site, &[0; 31], at(0)),
        Err(Refused::BadKey),
        "only an Ed25519 public key is bound"
    );
    assert!(
        plane
            .enroll(&id(SITE), &site, pair(8).public_key().as_ref(), at(599))
            .is_ok()
    );
}

#[test]
fn only_the_enrolled_key_within_the_skew_authenticates_a_call() {
    let (mut plane, cell, _) = plane();
    let call = signed(CELL, &pair(7), "/v1/heartbeat", b"{}", at(0));
    assert_eq!(plane.authenticate(&call, at(0)), Err(Refused::NotEnrolled));
    plane
        .enroll(&id(CELL), &cell, pair(7).public_key().as_ref(), at(0))
        .unwrap();
    assert_eq!(plane.authenticate(&call, at(300)), Ok(id(CELL)));
    assert_eq!(plane.authenticate(&call, at(-300)), Ok(id(CELL)));
    assert_eq!(plane.authenticate(&call, at(301)), Err(Refused::Stale));
    assert_eq!(plane.authenticate(&call, at(-301)), Err(Refused::Stale));
    let forged = signed(CELL, &pair(8), "/v1/heartbeat", b"{}", at(0));
    assert_eq!(
        plane.authenticate(&forged, at(0)),
        Err(Refused::BadSignature)
    );
    let mut tampered = call.clone();
    tampered.body = b"{\"x\":1}".to_vec();
    assert_eq!(
        plane.authenticate(&tampered, at(0)),
        Err(Refused::BadSignature)
    );
    let mut moved = call.clone();
    moved.path = "/v1/directory".to_owned();
    assert_eq!(
        plane.authenticate(&moved, at(0)),
        Err(Refused::BadSignature)
    );
    let unknown = signed("dep_nowhere", &pair(7), "/v1/heartbeat", b"{}", at(0));
    assert_eq!(
        plane.authenticate(&unknown, at(0)),
        Err(Refused::UnknownDeployment)
    );
}

/// E20-5 and CP-8: a deployment silent for a day is shown `unreachable`; it stays enrolled and
/// its next call authenticates, so nothing was revoked.
#[test]
fn a_deployment_silent_for_a_day_is_unreachable_and_nothing_is_revoked() {
    let mut plane = enrolled();
    let call = signed(CELL, &pair(7), "/v1/heartbeat", b"{}", at(0));
    plane
        .heartbeat(&call, heartbeat(Health::Degraded), at(0))
        .unwrap();
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
    let late = signed(CELL, &pair(7), "/v1/heartbeat", b"{}", day);
    assert_eq!(plane.heartbeat(&late, heartbeat(Health::Ok), day), Ok(()));
    assert_eq!(row(&plane, day).reachability, Reachability::Reachable);
    let site = plane
        .fleet(day)
        .into_iter()
        .find(|r| r.deployment_id == id(SITE))
        .unwrap();
    assert_eq!(site.reachability, Reachability::NeverSeen);
}

/// CP-11: a deployment replaces only its own workspaces and their members; naming another
/// deployment's workspace, or a member of a workspace it did not send, refuses the whole call.
#[test]
fn a_deployment_writes_only_its_own_directory() {
    let mut plane = enrolled();
    let cell = signed(CELL, &pair(7), "/v1/directory", b"{}", at(0));
    let site = signed(SITE, &pair(8), "/v1/directory", b"{}", at(0));
    let cell_rows = vec![
        workspace("ws_a", "org_a", CELL),
        workspace("ws_b", "org_b", CELL),
    ];
    let members = vec![member("user_1", "ws_a", MemberState::Active)];
    plane
        .sync_directory(&cell, cell_rows.clone(), members, at(0))
        .unwrap();
    plane
        .sync_directory(&site, vec![workspace("ws_s", "org_s", SITE)], vec![], at(0))
        .unwrap();
    let claims_cell = vec![workspace("ws_a", "org_a", SITE)];
    assert_eq!(
        plane.sync_directory(&site, claims_cell, vec![], at(0)),
        Err(Refused::NotYours)
    );
    let names_other = vec![workspace("ws_x", "org_s", CELL)];
    assert_eq!(
        plane.sync_directory(&site, names_other, vec![], at(0)),
        Err(Refused::NotYours)
    );
    let stray = vec![member("user_1", "ws_a", MemberState::Active)];
    let own = vec![workspace("ws_s", "org_s", SITE)];
    assert_eq!(
        plane.sync_directory(&site, own, stray, at(0)),
        Err(Refused::NotYours)
    );
    plane
        .sync_directory(&cell, vec![workspace("ws_a", "org_a", CELL)], vec![], at(1))
        .unwrap();
    let mut held: Vec<Ident> = plane
        .store
        .workspaces()
        .into_iter()
        .map(|w| w.workspace_id)
        .collect();
    held.sort();
    assert_eq!(
        held,
        [id("ws_a"), id("ws_s")],
        "the cell's sync replaced its own rows only"
    );
}

#[test]
fn a_user_is_routed_to_each_workspace_of_an_active_membership() {
    let mut plane = enrolled();
    let cell = signed(CELL, &pair(7), "/v1/directory", b"{}", at(0));
    let site = signed(SITE, &pair(8), "/v1/directory", b"{}", at(0));
    plane
        .sync_directory(
            &cell,
            vec![
                workspace("ws_a", "org_a", CELL),
                workspace("ws_b", "org_a", CELL),
            ],
            vec![
                member("user_1", "ws_a", MemberState::Active),
                member("user_1", "ws_b", MemberState::Deactivated),
                member("user_2", "ws_b", MemberState::Active),
            ],
            at(0),
        )
        .unwrap();
    plane
        .sync_directory(
            &site,
            vec![workspace("ws_s", "org_a", SITE)],
            vec![member("user_1", "ws_s", MemberState::Active)],
            at(0),
        )
        .unwrap();
    let ask = signed(CELL, &pair(7), "/v1/routes", b"user_1", at(0));
    let routes = plane.routes(&ask, &id("user_1"), at(0)).unwrap();
    assert_eq!(
        routes,
        [
            Route {
                workspace_id: id("ws_a"),
                endpoint: Endpoint::parse("https://cell1.example").unwrap()
            },
            Route {
                workspace_id: id("ws_s"),
                endpoint: Endpoint::parse("https://site.example:8443/api").unwrap()
            },
        ]
    );
    assert_eq!(
        plane.routes(&ask, &id("user_2"), at(0)),
        Err(Refused::NotYours),
        "the signed body names the user"
    );
    let forged = signed(CELL, &pair(9), "/v1/routes", b"user_1", at(0));
    assert_eq!(
        plane.routes(&forged, &id("user_1"), at(0)),
        Err(Refused::BadSignature)
    );
}

/// E20-1: every record the plane holds or answers, written out with its field names, has no
/// field for an email, a person's name, a phone number, or an identity-provider subject.
#[test]
fn no_record_has_a_field_for_personal_data() {
    let mut plane = enrolled();
    let call = signed(CELL, &pair(7), "/v1/heartbeat", b"{}", at(0));
    plane
        .heartbeat(&call, heartbeat(Health::Ok), at(0))
        .unwrap();
    let records = format!(
        "{:?} {:?} {:?} {:?} {:?} {:?}",
        plane.store.deployments(),
        workspace("ws_a", "org_a", CELL),
        member("user_1", "ws_a", MemberState::Active),
        Route {
            workspace_id: id("ws_a"),
            endpoint: Endpoint::parse("https://cell1.example").unwrap()
        },
        plane.fleet(at(0)),
        heartbeat(Health::Ok),
    );
    for forbidden in [
        "email",
        "mail",
        "full_name",
        "display_name",
        "given",
        "family",
        "phone",
        "subject",
        "sub:",
        "idp",
        "issuer",
    ] {
        assert!(!records.contains(forbidden), "{forbidden} in {records}");
    }
}

#[test]
fn an_endpoint_is_an_https_url_naming_a_host_only() {
    for good in ["https://cell1.example", "https://a-b.example:8443/api/v1"] {
        assert!(Endpoint::parse(good).is_some(), "{good}");
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

proptest! {
    /// No call signed by anything but the deployment's own enrolled key authenticates.
    #[test]
    fn only_the_enrolled_key_authenticates(seed in 9u8..=255, body in proptest::collection::vec(any::<u8>(), 0..64), skew in -300i64..=300) {
        let plane = enrolled();
        let forged = signed(CELL, &pair(seed), "/v1/heartbeat", &body, at(skew));
        prop_assert_eq!(plane.authenticate(&forged, at(0)), Err(Refused::BadSignature));
        let genuine = signed(CELL, &pair(7), "/v1/heartbeat", &body, at(skew));
        prop_assert_eq!(plane.authenticate(&genuine, at(0)), Ok(id(CELL)));
    }
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
    let endpoint = "https://cell1.example:8443/api";
    assert_eq!(Endpoint::parse(endpoint).unwrap().as_str(), endpoint);
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
        ..deployment(CELL, "https://cell1.example")
    };
    plane.create_deployment(pre_keyed, [1; 32], at(600));
    let call = signed(CELL, &pair(7), "/v1/heartbeat", b"{}", at(0));
    assert_eq!(plane.authenticate(&call, at(0)), Err(Refused::NotEnrolled));
}
