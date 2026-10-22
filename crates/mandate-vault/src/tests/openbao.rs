//! A sealed vault or a dev server is refused before any login (DEC-977 items 5 and 6).

use crate::SourceError;
use reqwest::StatusCode;

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use serde_json::{Value, json};

use crate::openbao::{
    OpenBaoConfig, OpenBaoSource, cas_refusal, kubernetes_login_path, kubernetes_login_refusal,
    kubernetes_login_verdict, kubernetes_namespace_verdict, lost_or_denied_verdict, provision,
    provisioned_identities, refusal, role_settings, seal_verdict, secret_id_identities, taken_read,
};
use crate::policy::{Identity, Mounts};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use secrecy::{ExposeSecret, SecretString};

const UNSEALED_RAFT: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":3,"n":5,"storage_type":"raft"}"#;
const UNSEALED_DEV: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":false,"t":1,"n":1,"storage_type":"inmem"}"#;
const SEALED: &[u8] =
    br#"{"type":"shamir","initialized":true,"sealed":true,"t":3,"n":5,"storage_type":"raft"}"#;

#[test]
fn a_production_server_passes_and_a_dev_server_needs_the_test_allowance() {
    assert_eq!(seal_verdict(UNSEALED_RAFT, false), Ok(()));
    assert_eq!(seal_verdict(UNSEALED_RAFT, true), Ok(()));
    assert_eq!(
        seal_verdict(UNSEALED_DEV, false),
        Err(SourceError::DevServerRefused)
    );
    assert_eq!(seal_verdict(UNSEALED_DEV, true), Ok(()));
}

#[test]
fn a_sealed_or_unreadable_status_is_refused() {
    for allow_dev in [false, true] {
        assert_eq!(seal_verdict(SEALED, allow_dev), Err(SourceError::Sealed));
        assert_eq!(
            seal_verdict(
                br#"{"initialized":false,"sealed":false,"storage_type":"raft"}"#,
                allow_dev
            ),
            Err(SourceError::Sealed),
            "an uninitialized vault waits for its operator as a sealed one does"
        );
        for bad in [
            &b""[..],
            b"[]",
            br#"{"sealed":"no","storage_type":"raft"}"#,
            br#"{"storage_type":"raft"}"#,
            br#"{"sealed":false}"#,
            br#"{"sealed":false,"storage_type":7}"#,
        ] {
            assert_eq!(
                seal_verdict(bad, allow_dev),
                Err(SourceError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}

#[test]
fn only_the_vaults_own_refusals_are_refusals_and_the_rest_are_outages() {
    for code in [200_u16, 204, 299] {
        assert_eq!(
            refusal(StatusCode::from_u16(code).unwrap()),
            Ok(()),
            "{code}"
        );
    }
    for (code, want) in [
        (400_u16, SourceError::Denied),
        (401, SourceError::Denied),
        (403, SourceError::Denied),
        (404, SourceError::Missing),
        (503, SourceError::Sealed),
        (100, SourceError::Unavailable),
        (301, SourceError::Unavailable),
        (307, SourceError::Unavailable),
        (405, SourceError::Unavailable),
        (408, SourceError::Unavailable),
        (412, SourceError::Unavailable),
        (429, SourceError::Unavailable),
        (500, SourceError::Unavailable),
        (502, SourceError::Unavailable),
        (504, SourceError::Unavailable),
    ] {
        let got = refusal(StatusCode::from_u16(code).unwrap());
        assert_eq!(got, Err(want.clone()), "{code}");
        assert_eq!(
            got.is_err_and(|e| e.is_outage()),
            !matches!(want, SourceError::Denied | SourceError::Missing),
            "{code}"
        );
    }
}

#[test]
fn each_role_lives_as_long_as_its_process_type_may() {
    let c = ConnectionId::new("c1").unwrap();
    assert_eq!(
        role_settings(&Identity::TokenExchange(c.clone())),
        json!({"token_ttl": "600s", "token_max_ttl": "600s", "secret_id_num_uses": 1, "secret_id_ttl": "600s"})
    );
    assert_eq!(
        role_settings(&Identity::Executor(c)),
        json!({"token_ttl": "86400s", "token_max_ttl": "2592000s", "secret_id_num_uses": 0, "secret_id_ttl": "0s"})
    );
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
    ] {
        assert_eq!(
            role_settings(&identity),
            json!({"token_ttl": "86400s", "token_max_ttl": "86400s", "secret_id_num_uses": 0, "secret_id_ttl": "0s"}),
            "{identity:?}"
        );
    }
}

/// `provision` makes one role for each process type and, per connection, its executor's and its
/// token exchange's: the list written out by hand.
#[test]
fn provisioning_covers_every_process_type_and_each_connections_two_roles() {
    let c = |id: &str| ConnectionId::new(id).unwrap();
    assert_eq!(
        provisioned_identities(&[c("c1"), c("c2")]),
        vec![
            Identity::Api,
            Identity::ConnectionManager,
            Identity::Runtime,
            Identity::SecretImport,
            Identity::ModelGateway,
            Identity::EmailSender,
            Identity::Executor(c("c1")),
            Identity::TokenExchange(c("c1")),
            Identity::Executor(c("c2")),
            Identity::TokenExchange(c("c2")),
        ]
    );
    assert_eq!(provisioned_identities(&[]).len(), 6);
}

/// A secret id is issued, and its files written, for each process type and each connection's
/// executor, in that order, and never for a token exchange, whose secret ids are grants.
#[test]
fn a_secret_id_is_issued_to_every_identity_but_the_token_exchanges() {
    let c = |id: &str| ConnectionId::new(id).unwrap();
    assert_eq!(
        secret_id_identities(&[c("c1"), c("c2")]),
        vec![
            Identity::Api,
            Identity::ConnectionManager,
            Identity::Runtime,
            Identity::SecretImport,
            Identity::ModelGateway,
            Identity::EmailSender,
            Identity::Executor(c("c1")),
            Identity::Executor(c("c2")),
        ]
    );
    assert_eq!(secret_id_identities(&[]).len(), 6);
}

/// Provisioning a vault nobody answers for is an outage, never an empty success that would leave
/// every process without credentials unnoticed.
#[tokio::test(flavor = "current_thread")]
async fn provisioning_an_unreachable_vault_is_an_outage() {
    let config = OpenBaoConfig {
        address: "https://127.0.0.1:9".to_owned(),
        ca_pem: include_bytes!("fixtures/isrg-root-x1.pem").to_vec(),
        mounts: Mounts::default(),
    };
    let token = SecretString::from("operator".to_owned());
    let provisioned = provision(&config, &token, &[ConnectionId::new("c1").unwrap()], None).await;
    assert_eq!(
        provisioned.map(|credentials| credentials.len()),
        Err(SourceError::Unavailable)
    );
}

fn unreachable() -> OpenBaoConfig {
    OpenBaoConfig {
        address: "https://127.0.0.1:9".to_owned(),
        ca_pem: include_bytes!("fixtures/isrg-root-x1.pem").to_vec(),
        mounts: Mounts::default(),
    }
}

/// The workspace's namespace.
const NAMESPACE: &str = "ws-ws1";

const JWT: &str = "canary-3e91-projected-service-account-token-do-not-print";

fn at_start() -> UtcNanos {
    UtcNanos::from_parts(1_800_000_000, 0).unwrap()
}

/// Only a connection's executor or token exchange logs in with Kubernetes auth; any other
/// identity is refused before the vault is reached.
#[tokio::test(flavor = "current_thread")]
async fn only_a_connections_processes_log_in_with_kubernetes_auth() {
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ] {
        let login = OpenBaoSource::login_kubernetes(
            &unreachable(),
            identity.clone(),
            &SecretString::from(JWT.to_owned()),
            NAMESPACE,
            at_start(),
        )
        .await;
        assert_eq!(login.map(|_| ()), Err(SourceError::Config), "{identity:?}");
    }
}

/// An executor's and an exchange's login reach the vault: one nobody answers for is an outage,
/// and neither the outage nor its `Debug` shows the token.
#[tokio::test(flavor = "current_thread")]
async fn a_kubernetes_login_to_an_unreachable_vault_is_an_outage_that_shows_no_token() {
    for identity in [
        Identity::Executor(ConnectionId::new("c1").unwrap()),
        Identity::TokenExchange(ConnectionId::new("c1").unwrap()),
    ] {
        let login = OpenBaoSource::login_kubernetes(
            &unreachable(),
            identity.clone(),
            &SecretString::from(JWT.to_owned()),
            NAMESPACE,
            at_start(),
        )
        .await;
        let shown = format!("{login:?}");
        assert!(!shown.contains(JWT), "{shown}");
        assert_eq!(
            login.map(|_| ()),
            Err(SourceError::Unavailable),
            "{identity:?}"
        );
    }
}

#[test]
fn each_kind_logs_in_on_its_own_mount_to_the_role_named_by_its_service_account() {
    let c1 = ConnectionId::new("c1").unwrap();
    assert_eq!(
        kubernetes_login_path(&Identity::Executor(c1.clone()), &Mounts::default()),
        Ok((
            "auth/kubernetes-executor/login".to_owned(),
            "6331".to_owned()
        ))
    );
    assert_eq!(
        kubernetes_login_path(&Identity::TokenExchange(c1), &Mounts::default()),
        Ok((
            "auth/kubernetes-exchange/login".to_owned(),
            "x6331".to_owned()
        ))
    );
    assert_eq!(
        kubernetes_login_path(
            &Identity::Executor(ConnectionId::new("Ab").unwrap()),
            &Mounts::default()
        ),
        Ok((
            "auth/kubernetes-executor/login".to_owned(),
            "4162".to_owned()
        ))
    );
    assert_eq!(
        kubernetes_login_path(&Identity::ModelGateway, &Mounts::default()),
        Err(SourceError::Config)
    );
}

/// A login is the identity's own only when the vault reached the role named by its own
/// ServiceAccount, for that account; another role, another account, or an answer missing either
/// is refused.
#[test]
fn a_kubernetes_login_is_accepted_only_for_its_own_role_and_account() {
    let executor = Identity::Executor(ConnectionId::new("c1").unwrap());
    let exchange = Identity::TokenExchange(ConnectionId::new("c1").unwrap());
    let auth = |role: &str, account: &str| json!({"metadata": {"role": role, "service_account_name": account}});
    assert_eq!(
        kubernetes_login_verdict(&executor, Some(&auth("6331", "6331"))),
        Ok(())
    );
    assert_eq!(
        kubernetes_login_verdict(&exchange, Some(&auth("x6331", "x6331"))),
        Ok(())
    );
    for (identity, answer) in [
        (&executor, auth("c1", "6331")),
        (&executor, auth("6332", "6331")),
        (&executor, auth("6331", "6332")),
        (&executor, auth("6331", "x6331")),
        (&executor, auth("x6331", "x6331")),
        (&exchange, auth("x6331", "6331")),
        (&exchange, auth("6331", "x6331")),
        (&exchange, auth("X6331", "x6331")),
    ] {
        assert_eq!(
            kubernetes_login_verdict(identity, Some(&answer)),
            Err(SourceError::Denied),
            "{answer}"
        );
    }
    for answer in [
        json!({"metadata": {"role": "6331"}}),
        json!({"metadata": {"service_account_name": "6331"}}),
        json!({"metadata": {"role": 7, "service_account_name": "6331"}}),
        json!({}),
    ] {
        assert_eq!(
            kubernetes_login_verdict(&executor, Some(&answer)),
            Err(SourceError::Malformed),
            "{answer}"
        );
    }
    assert_eq!(
        kubernetes_login_verdict(&executor, None),
        Err(SourceError::Malformed)
    );
}

/// A Kubernetes login's refusal is `refusal`'s, except that the vault saying the login's own role
/// does not exist is `RoleMissing`, which a process may wait out; a missing role named otherwise,
/// another 400, or a body that is not the vault's error list is still `Denied`.
#[test]
fn a_missing_role_is_its_own_refusal_and_every_other_answer_keeps_its_meaning() {
    let missing = br#"{"errors":["invalid role name \"6331\""]}"#;
    assert_eq!(
        kubernetes_login_refusal(400, missing, "6331"),
        Err(SourceError::RoleMissing)
    );
    assert_eq!(kubernetes_login_refusal(200, missing, "6331"), Ok(()));
    for (status, body, want) in [
        (400_u16, &missing[..], SourceError::RoleMissing),
        (403, &missing[..], SourceError::Denied),
        (500, &missing[..], SourceError::Unavailable),
        (503, &missing[..], SourceError::Sealed),
        (404, &missing[..], SourceError::Missing),
        (
            400,
            br#"{"errors":["invalid role name \"6332\""]}"#,
            SourceError::Denied,
        ),
        (
            400,
            br#"{"errors":["invalid role name \"6331\"","more"]}"#,
            SourceError::Denied,
        ),
        (
            400,
            br#"{"errors":["permission denied"]}"#,
            SourceError::Denied,
        ),
        (
            400,
            br#"{"errors":"invalid role name \"6331\""}"#,
            SourceError::Denied,
        ),
        (400, b"invalid role name \"6331\"", SourceError::Denied),
        (400, b"", SourceError::Denied),
    ] {
        assert_eq!(
            kubernetes_login_refusal(status, body, "6331"),
            Err(want),
            "{status} {}",
            String::from_utf8_lossy(body)
        );
    }
    let shown = format!("{:?}", kubernetes_login_refusal(400, missing, "6331"));
    assert!(!shown.contains("6331"), "{shown}");
}

/// The ServiceAccount the vault checked must be in the workspace's namespace, exactly; an answer
/// that does not say is malformed.
#[test]
fn a_kubernetes_login_is_accepted_only_in_the_workspaces_namespace() {
    let auth = |namespace: Value| json!({"metadata": {"service_account_namespace": namespace}});
    assert_eq!(
        kubernetes_namespace_verdict(Some(&auth(json!("ws-ws1"))), NAMESPACE),
        Ok(())
    );
    for other in ["ws-ws2", "WS-WS1", "ws-ws1 ", "", "kube-system"] {
        assert_eq!(
            kubernetes_namespace_verdict(Some(&auth(json!(other))), NAMESPACE),
            Err(SourceError::Denied),
            "{other:?}"
        );
    }
    for answer in [
        json!({"metadata": {}}),
        json!({"metadata": {"service_account_namespace": 7}}),
        json!({}),
    ] {
        assert_eq!(
            kubernetes_namespace_verdict(Some(&answer), NAMESPACE),
            Err(SourceError::Malformed),
            "{answer}"
        );
    }
    assert_eq!(
        kubernetes_namespace_verdict(None, NAMESPACE),
        Err(SourceError::Malformed)
    );
}

/// A check-and-set overwrite that finds the version moved on lost to another taker, which is no
/// code; every other answer keeps `refusal`'s meaning.
#[test]
fn an_overwrite_that_finds_the_version_moved_on_lost_its_race() {
    let moved = br#"{"errors":["check-and-set parameter did not match the current version"]}"#;
    assert_eq!(cas_refusal(400, moved), Err(SourceError::Missing));
    assert_eq!(cas_refusal(200, moved), Ok(()));
    for (status, body, want) in [
        (403_u16, &moved[..], SourceError::Denied),
        (500, &moved[..], SourceError::Unavailable),
        (
            400,
            br#"{"errors":["permission denied"]}"#,
            SourceError::Denied,
        ),
        (
            400,
            br#"{"errors":["check-and-set parameter did not match the current version","more"]}"#,
            SourceError::Denied,
        ),
        (
            400,
            b"check-and-set parameter did not match the current version",
            SourceError::Denied,
        ),
        (400, b"", SourceError::Denied),
    ] {
        assert_eq!(
            cas_refusal(status, body),
            Err(want),
            "{status} {}",
            String::from_utf8_lossy(body)
        );
    }
}

/// A code the take read, as text, its version beside it.
fn read_code(
    read: Result<Option<Value>, SourceError>,
) -> Result<Option<(u64, String)>, SourceError> {
    taken_read(read).map(|found| {
        found.map(|(version, code)| {
            (
                version,
                String::from_utf8(code.expose_secret().to_vec()).unwrap(),
            )
        })
    })
}

/// The code the take read is its version and its bytes; no code and a tombstone are no code; and
/// a read missing either half, or with a value that is not base64, is malformed. Nothing of the
/// code is ever in what the read's answer shows.
#[test]
fn a_read_before_a_take_is_the_codes_version_and_bytes_or_no_code() {
    let canary = "code-canary-9f2e";
    let encoded = STANDARD.encode(canary);
    let found = json!({"data": {"data": {"value": encoded}, "metadata": {"version": 3}}});
    assert_eq!(
        read_code(Ok(Some(found.clone()))),
        Ok(Some((3, canary.to_owned())))
    );
    let shown = format!("{:?}", taken_read(Ok(Some(found))));
    assert!(
        !shown.contains(canary) && !shown.contains(&encoded),
        "{shown}"
    );
    assert_eq!(read_code(Err(SourceError::Missing)), Ok(None));
    let tombstone = json!({"data": {"data": {"taken": true}, "metadata": {"version": 4}}});
    assert_eq!(read_code(Ok(Some(tombstone))), Ok(None));
    for malformed in [
        json!({"data": {"data": {"value": encoded}, "metadata": {}}}),
        json!({"data": {"data": {"value": encoded}, "metadata": {"version": "3"}}}),
        json!({"data": {"data": {}, "metadata": {"version": 3}}}),
        json!({"data": {"data": {"value": "not base64!"}, "metadata": {"version": 3}}}),
        json!({"data": {"data": {"taken": false, "value": 7}, "metadata": {"version": 3}}}),
    ] {
        let read = taken_read(Ok(Some(malformed.clone())));
        assert!(!format!("{read:?}").contains(canary));
        assert_eq!(
            read.map(|r| r.map(|(v, _)| v)),
            Err(SourceError::Malformed),
            "{malformed}"
        );
    }
    assert_eq!(read_code(Ok(None)), Err(SourceError::Malformed));
    assert_eq!(
        read_code(Err(SourceError::Unavailable)),
        Err(SourceError::Unavailable)
    );
    assert_eq!(
        read_code(Err(SourceError::Denied)),
        Err(SourceError::Denied)
    );
}

/// After a refused overwrite, the code gone is a lost race, no code; a code still there, or a
/// tombstone, is not this taker's to take; a failed read again is that failure.
#[test]
fn a_refused_overwrite_read_again_is_a_lost_race_only_when_the_code_is_gone() {
    let verdict = |reread| lost_or_denied_verdict(reread).map(|code| code.is_some());
    assert_eq!(verdict(Err(SourceError::Missing)), Ok(false));
    for there in [
        json!({"data": {"data": {"value": "eA=="}, "metadata": {"version": 1}}}),
        json!({"data": {"data": {"taken": true}, "metadata": {"version": 2}}}),
    ] {
        assert_eq!(
            verdict(Ok(Some(there.clone()))),
            Err(SourceError::Denied),
            "{there}"
        );
    }
    assert_eq!(
        verdict(Err(SourceError::Unavailable)),
        Err(SourceError::Unavailable)
    );
    assert_eq!(verdict(Err(SourceError::Sealed)), Err(SourceError::Sealed));
}
