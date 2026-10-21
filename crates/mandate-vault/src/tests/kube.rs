//! Kubernetes Secrets of AppRole credentials (DEC-2630): each identity's name written out by hand,
//! the manifest compared whole, and every refusal leaving no manifest.

use mandate_connections::record::ConnectionId;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::SourceError;
use crate::kube::{SecretManifest, kube_secrets, secret_name};
use crate::openbao::RoleCredentials;
use crate::policy::Identity;

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn credential(identity: Identity, role_id: &str, secret_id: &str) -> RoleCredentials {
    RoleCredentials {
        identity,
        role_id: role_id.to_owned(),
        secret_id: SecretString::from(secret_id.to_owned()),
    }
}

/// The oracle's executor name: the prefix, then each byte of the id as two lowercase hex digits.
fn executor_name(id: &str) -> String {
    let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
    format!("vault-approle-executor-{hex}")
}

/// The oracle's Kubernetes object name rule (DNS-1123 subdomain), written out by hand.
fn is_object_name(name: &str) -> bool {
    let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.';
    let alnum = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    name.len() <= 253
        && name.bytes().all(allowed)
        && alnum(name.as_bytes().first())
        && alnum(name.as_bytes().last())
}

fn parsed(manifest: &SecretManifest) -> Value {
    serde_json::from_str(manifest.manifest.expose_secret()).unwrap()
}

#[test]
fn each_platform_identity_has_its_secret_name_written_out() {
    let names: Vec<_> = [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ]
    .iter()
    .map(secret_name)
    .collect();
    assert_eq!(
        names,
        vec![
            Ok("vault-approle-api".to_owned()),
            Ok("vault-approle-connection-manager".to_owned()),
            Ok("vault-approle-runtime".to_owned()),
            Ok("vault-approle-secret-import".to_owned()),
            Ok("vault-approle-model-gateway".to_owned()),
            Ok("vault-approle-email-sender".to_owned()),
        ]
    );
}

#[test]
fn an_executors_name_writes_its_connection_in_hex() {
    assert_eq!(
        secret_name(&Identity::Executor(c("c1"))),
        Ok("vault-approle-executor-6331".to_owned())
    );
    assert_eq!(
        secret_name(&Identity::Executor(c("Ab_-9"))),
        Ok("vault-approle-executor-41625f2d39".to_owned())
    );
}

#[test]
fn connections_that_differ_only_in_case_or_punctuation_get_different_names() {
    let names: Vec<_> = ["ab", "Ab", "a_b", "a-b"]
        .iter()
        .map(|id| secret_name(&Identity::Executor(c(id))))
        .collect();
    assert_eq!(
        names,
        vec![
            Ok("vault-approle-executor-6162".to_owned()),
            Ok("vault-approle-executor-4162".to_owned()),
            Ok("vault-approle-executor-615f62".to_owned()),
            Ok("vault-approle-executor-612d62".to_owned()),
        ]
    );
}

#[test]
fn a_token_exchange_has_no_secret() {
    assert_eq!(
        secret_name(&Identity::TokenExchange(c("c1"))),
        Err(SourceError::Config)
    );
}

/// For any connection id the grammar admits, the name is the oracle's, and a valid object name.
#[test]
fn every_executors_name_is_the_oracles_and_a_valid_object_name() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let outcome = runner.run(&"[A-Za-z0-9_-]{1,64}", |id| {
        let name = secret_name(&Identity::Executor(c(&id)));
        prop_assert_eq!(&name, &Ok(executor_name(&id)));
        let valid = name.as_deref().is_ok_and(is_object_name);
        prop_assert!(valid, "{name:?} is not a valid object name");
        Ok::<(), TestCaseError>(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

#[test]
fn an_executors_manifest_is_written_out_whole() {
    let manifests = kube_secrets(
        "ws-ws1",
        &[credential(
            Identity::Executor(c("c1")),
            "role-1",
            "secret-1",
        )],
    );
    let shown: Result<Vec<(String, Value)>, SourceError> = manifests.map(|all| {
        all.iter()
            .map(|manifest| (manifest.name.clone(), parsed(manifest)))
            .collect()
    });
    assert_eq!(
        shown,
        Ok(vec![(
            "vault-approle-executor-6331".to_owned(),
            json!({
                "apiVersion": "v1",
                "kind": "Secret",
                "metadata": {
                    "name": "vault-approle-executor-6331",
                    "namespace": "ws-ws1",
                    "labels": {"app.kubernetes.io/managed-by": "mandate-vault-provision"},
                    "annotations": {"mandate.dev/vault-role": "executor-c1"}
                },
                "type": "Opaque",
                "stringData": {"role-id": "role-1", "secret-id": "secret-1"}
            })
        )])
    );
}

#[test]
fn manifests_keep_the_credentials_order() {
    let names = kube_secrets(
        "ws-ws1",
        &[
            credential(Identity::Runtime, "r", "s"),
            credential(Identity::Executor(c("c2")), "r", "s"),
            credential(Identity::Api, "r", "s"),
        ],
    )
    .map(|all| all.into_iter().map(|m| m.name).collect::<Vec<_>>());
    assert_eq!(
        names,
        Ok(vec![
            "vault-approle-runtime".to_owned(),
            "vault-approle-executor-6332".to_owned(),
            "vault-approle-api".to_owned(),
        ])
    );
}

#[test]
fn a_namespace_that_is_not_a_label_refuses_every_manifest() {
    let one = [credential(Identity::Api, "r", "s")];
    let long = "a".repeat(64);
    for namespace in ["", "WS", "-a", "a-", "a_b", "a.b", long.as_str()] {
        assert_eq!(
            kube_secrets(namespace, &one).map(|all| all.len()),
            Err(SourceError::Config),
            "{namespace:?}"
        );
    }
    let longest = "a".repeat(63);
    for namespace in ["a", "ws-ws1", "0", longest.as_str()] {
        assert_eq!(
            kube_secrets(namespace, &one).map(|all| all.len()),
            Ok(1),
            "{namespace:?}"
        );
    }
}

#[test]
fn a_token_exchange_or_a_repeated_identity_refuses_every_manifest() {
    let exchange = [
        credential(Identity::Api, "r", "s"),
        credential(Identity::TokenExchange(c("c1")), "r", "s"),
    ];
    let repeated = [
        credential(Identity::Executor(c("c1")), "r", "s"),
        credential(Identity::Runtime, "r", "s"),
        credential(Identity::Executor(c("c1")), "r2", "s2"),
    ];
    for credentials in [&exchange[..], &repeated[..]] {
        assert_eq!(
            kube_secrets("ws-ws1", credentials).map(|all| all.len()),
            Err(SourceError::Config)
        );
    }
}

#[test]
fn no_manifest_shows_its_secret_id_when_debugged() {
    let manifests = kube_secrets(
        "ws-ws1",
        &[credential(Identity::Runtime, "role-r", "very-secret-id")],
    );
    let shown = format!("{manifests:?}");
    assert!(
        manifests.is_ok()
            && !shown.contains("very-secret-id")
            && shown.contains("vault-approle-runtime"),
        "{shown}"
    );
}
