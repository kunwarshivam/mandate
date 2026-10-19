//! Kubernetes auth's templated policies, the provisioner's policy and its role requests
//! (DEC-2060), each written out whole by hand and compared exactly.

use mandate_connections::record::ConnectionId;
use serde_json::{Value, json};

use crate::SourceError;
use crate::kubernetes::{
    kubernetes_policy, kubernetes_policy_name, provisioner_policy, role_request,
    service_account_name,
};
use crate::policy::{Identity, Mounts};

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

/// The workspace's namespace, where both kinds' ServiceAccounts are.
const NAMESPACE: &str = "ws-ws1";

const TEMPLATE: &str =
    "{{identity.entity.aliases.auth_kubernetes_1a2b.metadata.service_account_name}}";

#[test]
#[ignore = "pending E21-32"]
fn a_connections_service_account_is_its_id_in_hex() {
    assert_eq!(
        service_account_name(&Identity::Executor(c("c1"))),
        Ok("6331".to_owned())
    );
    assert_eq!(
        service_account_name(&Identity::Executor(c("Ab_-9"))),
        Ok("41625f2d39".to_owned())
    );
    assert_eq!(
        service_account_name(&Identity::TokenExchange(c("c1"))),
        Ok("x6331".to_owned())
    );
    assert_eq!(
        service_account_name(&Identity::Runtime),
        Err(SourceError::Config)
    );
}

/// An executor's name has an even number of hex digits and an exchange's is `x` and the same
/// digits, an odd number of characters, so neither kind's name is ever the other's, for any id.
#[test]
#[ignore = "pending E21-32"]
fn no_executors_name_is_ever_an_exchanges() {
    for id in ["c1", "x", "A", "a-b_C", "0", &"z".repeat(64)] {
        let executor = service_account_name(&Identity::Executor(c(id)));
        let exchange = service_account_name(&Identity::TokenExchange(c(id)));
        assert!(
            executor.is_ok() && exchange.is_ok(),
            "{executor:?} {exchange:?}"
        );
        let (executor, exchange) = (executor.unwrap_or_default(), exchange.unwrap_or_default());
        assert_eq!(executor.len() % 2, 0, "{executor}");
        assert_eq!(exchange.len() % 2, 1, "{exchange}");
        assert_eq!(exchange, format!("x{executor}"));
    }
}

/// A ServiceAccount name is lowercase hex alone, so no platform process's name, each of which
/// holds a letter past `f` or a hyphen, can ever be one.
#[test]
#[ignore = "pending E21-32"]
fn no_platform_process_name_is_a_service_account_name() {
    let named = service_account_name(&Identity::Executor(c("A_z-0")));
    assert!(named.is_ok(), "{named:?}");
    let name = named.unwrap_or_default();
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{name}"
    );
    for process in [
        "api",
        "control",
        "deploy-manager",
        "runtime",
        "executor",
        "token-exchange",
        "provisioner",
        "scheduler",
        "model-gateway",
        "data",
    ] {
        assert!(
            !process
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{process}"
        );
    }
}

#[test]
#[ignore = "pending E21-32"]
fn each_kinds_policy_is_named_under_the_kv_mount() {
    let mounts = Mounts::default();
    assert_eq!(
        kubernetes_policy_name(&Identity::Executor(c("c1")), &mounts),
        Ok("mandate-kubernetes-executor".to_owned())
    );
    assert_eq!(
        kubernetes_policy_name(&Identity::TokenExchange(c("c2")), &mounts),
        Ok("mandate-kubernetes-exchange".to_owned())
    );
    assert_eq!(
        kubernetes_policy_name(&Identity::Api, &mounts),
        Err(SourceError::Config)
    );
}

#[test]
#[ignore = "pending E21-32"]
fn the_executors_policy_reads_its_own_token_alone() {
    assert_eq!(
        kubernetes_policy(
            &Identity::Executor(c("any")),
            &Mounts::default(),
            "auth_kubernetes_1a2b"
        ),
        Ok(format!(
            "path \"mandate/data/connections/x{TEMPLATE}/token\" {{\n  capabilities = [\"read\"]\n}}\n"
        ))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn the_exchanges_policy_writes_its_token_and_reads_and_deletes_its_code() {
    assert_eq!(
        kubernetes_policy(
            &Identity::TokenExchange(c("any")),
            &Mounts::default(),
            "auth_kubernetes_1a2b"
        ),
        Ok(format!(
            "path \"mandate/data/platform/alpaca-oauth-client-secret\" {{\n  capabilities = [\"read\"]\n}}\n\
             path \"mandate/data/connections/{TEMPLATE}/code\" {{\n  capabilities = [\"read\"]\n}}\n\
             path \"mandate/metadata/connections/{TEMPLATE}/code\" {{\n  capabilities = [\"delete\"]\n}}\n\
             path \"mandate/data/connections/{TEMPLATE}/token\" {{\n  capabilities = [\"create\", \"update\"]\n}}\n"
        ))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn no_platform_identity_has_a_kubernetes_policy() {
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ] {
        assert_eq!(
            kubernetes_policy(&identity, &Mounts::default(), "auth_kubernetes_1a2b"),
            Err(SourceError::Config),
            "{identity:?}"
        );
    }
}

/// The provisioner's policy, written out whole: one block per mount, every parameter but the
/// ServiceAccount's name pinned.
#[test]
#[ignore = "pending E21-32"]
fn the_provisioners_policy_pins_every_parameter_but_the_name() {
    let block = |mount: &str, namespace: &str, policy: &str, ttl: &str, max: &str| {
        format!(
            "path \"auth/{mount}/role/+\" {{\n  \
             capabilities = [\"create\", \"update\", \"read\"]\n  \
             required_parameters = [\"bound_service_account_names\", \"bound_service_account_namespaces\", \"audience\", \"token_policies\"]\n  \
             allowed_parameters = {{\n    \
             \"alias_name_source\" = [\"serviceaccount_uid\"]\n    \
             \"audience\" = [\"mandate-vault\"]\n    \
             \"bound_service_account_names\" = []\n    \
             \"bound_service_account_namespaces\" = [\"{namespace}\"]\n    \
             \"token_max_ttl\" = [\"{max}\"]\n    \
             \"token_no_default_policy\" = [\"false\"]\n    \
             \"token_policies\" = [\"{policy}\"]\n    \
             \"token_ttl\" = [\"{ttl}\"]\n  \
             }}\n}}\n"
        )
    };
    assert_eq!(
        provisioner_policy(&Mounts::default(), NAMESPACE),
        Ok(format!(
            "{}{}",
            block(
                "kubernetes-executor",
                "ws-ws1",
                "mandate-kubernetes-executor",
                "86400",
                "2592000"
            ),
            block(
                "kubernetes-exchange",
                "ws-ws1",
                "mandate-kubernetes-exchange",
                "600",
                "600"
            )
        ))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn mounts_or_a_namespace_that_are_not_plain_names_are_refused() {
    let bad_mounts: [fn(&mut Mounts); 3] = [
        |m| m.kubernetes_executor = "kubernetes/executor".to_owned(),
        |m| m.kubernetes_exchange = String::new(),
        |m| m.kubernetes_executor = "kubernetes\"\n}".to_owned(),
    ];
    for (at, break_it) in bad_mounts.iter().enumerate() {
        let mut broken = Mounts::default();
        break_it(&mut broken);
        assert_eq!(
            provisioner_policy(&broken, NAMESPACE),
            Err(SourceError::Config),
            "mounts {at}"
        );
        assert_eq!(
            role_request(&Identity::Executor(c("c1")), &broken, NAMESPACE).map(|(path, _)| path),
            Err(SourceError::Config),
            "mounts {at}"
        );
    }
    for namespace in ["WS", "ws_ws1", "-ws", ""] {
        assert_eq!(
            provisioner_policy(&Mounts::default(), namespace),
            Err(SourceError::Config),
            "{namespace:?}"
        );
        assert_eq!(
            role_request(
                &Identity::TokenExchange(c("c1")),
                &Mounts::default(),
                namespace
            )
            .map(|(path, _)| path),
            Err(SourceError::Config),
            "{namespace:?}"
        );
    }
}

#[test]
#[ignore = "pending E21-32"]
fn an_executors_role_binds_exactly_its_service_account_in_its_namespace() {
    assert_eq!(
        role_request(&Identity::Executor(c("c1")), &Mounts::default(), NAMESPACE),
        Ok((
            "auth/kubernetes-executor/role/c1".to_owned(),
            json!({
                "bound_service_account_names": ["6331"],
                "bound_service_account_namespaces": ["ws-ws1"],
                "audience": "mandate-vault",
                "alias_name_source": "serviceaccount_uid",
                "token_policies": ["mandate-kubernetes-executor"],
                "token_ttl": "86400",
                "token_max_ttl": "2592000",
                "token_no_default_policy": "false"
            })
        ))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn an_exchanges_role_binds_exactly_its_service_account_in_its_namespace() {
    assert_eq!(
        role_request(
            &Identity::TokenExchange(c("c2")),
            &Mounts::default(),
            NAMESPACE
        ),
        Ok((
            "auth/kubernetes-exchange/role/c2".to_owned(),
            json!({
                "bound_service_account_names": ["x6332"],
                "bound_service_account_namespaces": ["ws-ws1"],
                "audience": "mandate-vault",
                "alias_name_source": "serviceaccount_uid",
                "token_policies": ["mandate-kubernetes-exchange"],
                "token_ttl": "600",
                "token_max_ttl": "600",
                "token_no_default_policy": "false"
            })
        ))
    );
}

/// Every role request passes the provisioner's policy: each parameter it sends is one the
/// policy allows, with a value the policy pins, and every required one is sent. The oracle reads
/// the policy's text, not the code that wrote it.
#[test]
#[ignore = "pending E21-32"]
fn every_role_request_is_one_the_provisioners_policy_allows() {
    let written = provisioner_policy(&Mounts::default(), NAMESPACE);
    assert!(written.is_ok(), "{written:?}");
    let policy = written.unwrap_or_default();
    for identity in [
        Identity::Executor(c("c1")),
        Identity::TokenExchange(c("Zz_9")),
    ] {
        let request = role_request(&identity, &Mounts::default(), NAMESPACE);
        assert!(request.is_ok(), "{request:?}");
        let (path, body) = request.unwrap_or_default();
        let mount = path.split('/').nth(1).unwrap_or_default();
        let block = policy
            .split("path \"")
            .find(|block| block.starts_with(&format!("auth/{mount}/role/+")))
            .unwrap_or_default();
        assert!(!block.is_empty(), "no block for {path}");
        let fields = body.as_object().cloned().unwrap_or_default();
        for (key, value) in &fields {
            let line = block
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("\"{key}\" = ")))
                .unwrap_or_default();
            assert!(!line.is_empty(), "{key} is not allowed");
            let pinned = !line.trim_end().ends_with("= []");
            if pinned {
                let wanted = match value {
                    Value::Array(items) => items.first().cloned().unwrap_or_default(),
                    other => other.clone(),
                };
                assert!(
                    line.contains(&wanted.to_string()),
                    "{key} = {value} is not the pinned {line}"
                );
            }
        }
        for required in [
            "bound_service_account_names",
            "bound_service_account_namespaces",
            "audience",
            "token_policies",
        ] {
            assert!(fields.contains_key(required), "{required} missing");
        }
    }
}
