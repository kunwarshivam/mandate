//! Kubernetes auth's templated policies, the provisioner's policy and its role requests
//! (DEC-2060), each written out whole by hand and compared exactly, and the one contract they
//! share with every writer: a policy with its login's ServiceAccount put in reaches exactly the
//! paths [`kv_path`] stores at, and no two connections share a path, a ServiceAccount or a role.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_connections::record::ConnectionId;
use serde_json::{Value, json};

use crate::SourceError;
use crate::kubernetes::{
    KubernetesAuth, kubernetes_mount_requests, kubernetes_policy, kubernetes_policy_name,
    mount_accessor, provisioner_policy, role_request, service_account_name,
};
use crate::policy::{Identity, Mounts, SecretPath, connection_segment, kv_path};

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

/// The workspace's namespace, where both kinds' ServiceAccounts are.
const NAMESPACE: &str = "ws-ws1";

const ACCESSOR: &str = "auth_kubernetes_1a2b";

const TEMPLATE: &str =
    "{{identity.entity.aliases.auth_kubernetes_1a2b.metadata.service_account_name}}";

/// The longest id the grammar allows, 64 characters, so 128 hex digits.
fn longest() -> String {
    "z".repeat(64)
}

/// Ids that a raw, an unprefixed or a case-folding layout would make collide: an id that is
/// another's hex, one that is another's segment, pairs differing only in case, and the longest.
fn tricky() -> Vec<String> {
    ["c1", "6331", "x6331", "Ab", "aB", "x", "78", "A", "a"]
        .map(str::to_owned)
        .into_iter()
        .chain([longest()])
        .collect()
}

/// Whether `text` is one or more pairs of lowercase hex digits, the oracle's own test.
fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text.len().is_multiple_of(2)
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The id whose executor's ServiceAccount `name` would be, decoded the oracle's own way, if any.
fn unhex(name: &str) -> Option<ConnectionId> {
    if !is_hex(name) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..name.len())
        .step_by(2)
        .map(|at| {
            name.get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect();
    ConnectionId::new(&String::from_utf8(bytes?).ok()?).ok()
}

#[test]
fn a_connections_service_account_is_its_id_in_hex() {
    for (identity, name) in [
        (Identity::Executor(c("c1")), "6331".to_owned()),
        (Identity::Executor(c("Ab_-9")), "41625f2d39".to_owned()),
        (Identity::Executor(c("Ab")), "4162".to_owned()),
        (Identity::Executor(c("aB")), "6142".to_owned()),
        (Identity::Executor(c(&longest())), "7a".repeat(64)),
        (Identity::TokenExchange(c("c1")), "x6331".to_owned()),
        (Identity::TokenExchange(c("Ab")), "x4162".to_owned()),
        (
            Identity::TokenExchange(c(&longest())),
            format!("x{}", "7a".repeat(64)),
        ),
    ] {
        let named = service_account_name(&identity);
        assert_eq!(named, Ok(name), "{identity:?}");
    }
    assert_eq!(
        service_account_name(&Identity::Runtime),
        Err(SourceError::Config)
    );
}

/// A connection's code and token are stored under `x` and its hex, written out by hand.
#[test]
fn a_connections_entries_are_stored_under_x_and_its_hex() {
    for (id, segment) in [
        ("c1".to_owned(), "x6331".to_owned()),
        ("6331".to_owned(), "x36333331".to_owned()),
        ("x6331".to_owned(), "x7836333331".to_owned()),
        ("Ab".to_owned(), "x4162".to_owned()),
        ("aB".to_owned(), "x6142".to_owned()),
        (longest(), format!("x{}", "7a".repeat(64))),
    ] {
        let stored = connection_segment(&c(&id));
        assert_eq!(stored, Ok(segment.clone()), "{id}");
        assert_eq!(
            kv_path(&SecretPath::Code(c(&id))),
            Ok(format!("connections/{segment}/code")),
            "{id}"
        );
        assert_eq!(
            kv_path(&SecretPath::Token(c(&id))),
            Ok(format!("connections/{segment}/token")),
            "{id}"
        );
    }
}

/// An executor's name has an even number of hex digits and an exchange's is `x` and the same
/// digits, an odd number of characters, so neither kind's name is ever the other's, for any id.
#[test]
fn no_executors_name_is_ever_an_exchanges() {
    for id in tricky() {
        let executor = service_account_name(&Identity::Executor(c(&id)));
        let exchange = service_account_name(&Identity::TokenExchange(c(&id)));
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

/// No two of the tricky ids share a stored path, a ServiceAccount or a role, across both kinds,
/// even with case folded, as OpenBao folds role and policy names.
#[test]
fn no_two_connections_share_a_path_a_service_account_or_a_role() {
    let ids = tricky();
    let (mut paths, mut names, mut roles) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for id in &ids {
        for identity in [Identity::Executor(c(id)), Identity::TokenExchange(c(id))] {
            let name = service_account_name(&identity);
            assert!(name.is_ok(), "{name:?}");
            names.insert(name.unwrap_or_default().to_lowercase());
            let role = role_request(&identity, &Mounts::default(), NAMESPACE);
            assert!(role.is_ok(), "{role:?}");
            roles.insert(
                role.map(|(path, _)| path)
                    .unwrap_or_default()
                    .to_lowercase(),
            );
        }
        for path in [SecretPath::Code(c(id)), SecretPath::Token(c(id))] {
            let stored = kv_path(&path);
            assert!(stored.is_ok(), "{stored:?}");
            paths.insert(stored.unwrap_or_default().to_lowercase());
        }
    }
    assert_eq!(paths.len(), 2 * ids.len(), "{paths:?}");
    assert_eq!(names.len(), 2 * ids.len(), "{names:?}");
    assert_eq!(roles.len(), 2 * ids.len(), "{roles:?}");
}

/// The cross-contract: each kind's policy, with the ServiceAccount its role binds put where the
/// login's name goes, is exactly the policy written from [`kv_path`] for that connection, so the
/// path a writer stores at and the path a policy grants are one, for every tricky id.
#[test]
#[ignore = "pending E21-32"]
fn each_kinds_policy_reaches_exactly_the_paths_its_connection_is_stored_at() {
    let block = |prefix: &str, path: &SecretPath, capabilities: &str| {
        let stored = kv_path(path);
        assert!(stored.is_ok(), "{stored:?}");
        format!(
            "path \"mandate/{prefix}/{}\" {{\n  capabilities = [{capabilities}]\n}}\n",
            stored.unwrap_or_default()
        )
    };
    for id in tricky() {
        let connection = c(&id);
        let code = SecretPath::Code(connection.clone());
        let token = SecretPath::Token(connection.clone());
        for (identity, wanted) in [
            (
                Identity::Executor(connection.clone()),
                block("data", &token, "\"read\""),
            ),
            (
                Identity::TokenExchange(connection.clone()),
                [
                    block("data", &SecretPath::ClientSecret, "\"read\""),
                    block("data", &code, "\"read\", \"update\""),
                    block("metadata", &code, "\"delete\""),
                    block("data", &token, "\"create\", \"update\""),
                ]
                .concat(),
            ),
        ] {
            let policy = kubernetes_policy(&identity, &Mounts::default(), ACCESSOR);
            assert!(policy.is_ok(), "{policy:?}");
            let role = role_request(&identity, &Mounts::default(), NAMESPACE);
            assert!(role.is_ok(), "{role:?}");
            let bound = role
                .map(|(_, body)| body["bound_service_account_names"][0].clone())
                .unwrap_or_default();
            let bound = bound.as_str().unwrap_or_default();
            assert!(!bound.is_empty(), "{identity:?} binds no ServiceAccount");
            assert_eq!(
                policy.unwrap_or_default().replace(TEMPLATE, bound),
                wanted,
                "{identity:?}"
            );
        }
    }
}

/// No process in the workspace chart's list, `processes.json`, is any connection's ServiceAccount:
/// each name that decodes to an id is checked against the code's name for that id, and the
/// decoding is first shown to find a collision where one exists.
#[test]
fn no_platform_process_name_is_a_service_account_name() {
    let kinds: [fn(ConnectionId) -> Identity; 2] = [Identity::Executor, Identity::TokenExchange];
    let collides = |name: &str| {
        let candidates = [unhex(name), name.strip_prefix('x').and_then(unhex)];
        kinds.iter().zip(candidates).any(|(kind, id)| {
            id.is_some_and(|id| service_account_name(&kind(id)).as_deref() == Ok(name))
        })
    };
    let named = service_account_name(&Identity::Executor(c("c1")));
    assert!(named.is_ok(), "{named:?}");
    for seeded in ["6331", "x6331"] {
        assert!(collides(seeded), "the oracle misses {seeded}");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/cell/charts/workspace/processes.json");
    let list: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let names: Vec<&str> = list["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|process| process["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"executor") && names.contains(&"token-exchange"),
        "{names:?}"
    );
    for name in names {
        assert!(!collides(name), "{name} is a connection's ServiceAccount");
    }
}

#[test]
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
fn the_executors_policy_reads_its_own_token_alone() {
    assert_eq!(
        kubernetes_policy(&Identity::Executor(c("any")), &Mounts::default(), ACCESSOR),
        Ok(format!(
            "path \"mandate/data/connections/x{TEMPLATE}/token\" {{\n  capabilities = [\"read\"]\n}}\n"
        ))
    );
}

/// The exchange's code block reads and updates, the update being its take (DEC-2061), and never
/// creates: `create` would let it write a code where none is.
#[test]
#[ignore = "pending E21-32"]
fn the_exchanges_policy_writes_its_token_and_reads_takes_and_deletes_its_code() {
    assert_eq!(
        kubernetes_policy(
            &Identity::TokenExchange(c("any")),
            &Mounts::default(),
            ACCESSOR
        ),
        Ok(format!(
            "path \"mandate/data/platform/alpaca-oauth-client-secret\" {{\n  capabilities = [\"read\"]\n}}\n\
             path \"mandate/data/connections/{TEMPLATE}/code\" {{\n  capabilities = [\"read\", \"update\"]\n}}\n\
             path \"mandate/metadata/connections/{TEMPLATE}/code\" {{\n  capabilities = [\"delete\"]\n}}\n\
             path \"mandate/data/connections/{TEMPLATE}/token\" {{\n  capabilities = [\"create\", \"update\"]\n}}\n"
        ))
    );
}

/// An accessor that could end the template or the policy, or is empty, writes no policy.
#[test]
fn an_accessor_that_is_not_a_plain_segment_is_refused() {
    let fine = kubernetes_policy(&Identity::Executor(c("c1")), &Mounts::default(), ACCESSOR);
    assert!(fine.is_ok(), "{fine:?}");
    for accessor in [
        "", "auth\"x", "auth}}x", "auth{{x", "auth\nx", "auth.x", "auth/x", "auth x",
    ] {
        for identity in [
            Identity::Executor(c("c1")),
            Identity::TokenExchange(c("c1")),
        ] {
            assert_eq!(
                kubernetes_policy(&identity, &Mounts::default(), accessor),
                Err(SourceError::Config),
                "{accessor:?}"
            );
        }
    }
}

#[test]
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
            kubernetes_policy(&identity, &Mounts::default(), ACCESSOR),
            Err(SourceError::Config),
            "{identity:?}"
        );
    }
}

/// The provisioner's policy, written out whole: one block per mount, every parameter required
/// and every one but the ServiceAccount's name pinned.
#[test]
fn the_provisioners_policy_requires_every_parameter_and_pins_all_but_the_name() {
    let block = |mount: &str, namespace: &str, policy: &str, ttl: &str, max: &str| {
        format!(
            "path \"auth/{mount}/role/+\" {{\n  \
             capabilities = [\"create\", \"update\", \"read\"]\n  \
             required_parameters = [\"alias_name_source\", \"audience\", \"bound_service_account_names\", \"bound_service_account_namespaces\", \"token_max_ttl\", \"token_no_default_policy\", \"token_policies\", \"token_ttl\"]\n  \
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
fn an_executors_role_is_named_by_and_binds_exactly_its_service_account() {
    assert_eq!(
        role_request(&Identity::Executor(c("c1")), &Mounts::default(), NAMESPACE),
        Ok((
            "auth/kubernetes-executor/role/6331".to_owned(),
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
fn an_exchanges_role_is_named_by_and_binds_exactly_its_service_account() {
    assert_eq!(
        role_request(
            &Identity::TokenExchange(c("c2")),
            &Mounts::default(),
            NAMESPACE
        ),
        Ok((
            "auth/kubernetes-exchange/role/x6332".to_owned(),
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

/// Every role request passes the provisioner's policy: the parameters it sends are exactly the
/// required ones, each allowed with the value the policy pins. The oracle reads the policy's
/// text, not the code that wrote it.
#[test]
fn every_role_request_is_one_the_provisioners_policy_allows() {
    let written = provisioner_policy(&Mounts::default(), NAMESPACE);
    assert!(written.is_ok(), "{written:?}");
    let policy = written.unwrap_or_default();
    for identity in [
        Identity::Executor(c("c1")),
        Identity::TokenExchange(c("Zz_9")),
        Identity::Executor(c(&longest())),
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
        let sent: BTreeSet<String> = fields.keys().cloned().collect();
        let required: BTreeSet<String> = block
            .lines()
            .find_map(|line| line.trim_start().strip_prefix("required_parameters = ["))
            .and_then(|list| list.strip_suffix(']'))
            .unwrap_or_default()
            .split(", ")
            .map(|name| name.trim_matches('"').to_owned())
            .collect();
        assert_eq!(required, sent, "{path}");
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
    }
}

/// A CA certificate's shape, enough for the checks here; the server test passes a real one.
const PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";

/// Both mounts are enabled as `kubernetes` and configured with the API server's address, the CA
/// only where one is given, executor's first.
#[test]
fn both_kubernetes_mounts_are_enabled_and_pointed_at_the_api_server() {
    let in_cluster = KubernetesAuth {
        host: "https://10.43.0.1:443".to_owned(),
        ca_pem: None,
    };
    assert_eq!(
        kubernetes_mount_requests(&Mounts::default(), &in_cluster),
        Ok(vec![
            (
                "sys/auth/kubernetes-executor".to_owned(),
                json!({"type": "kubernetes"})
            ),
            (
                "auth/kubernetes-executor/config".to_owned(),
                json!({"kubernetes_host": "https://10.43.0.1:443"})
            ),
            (
                "sys/auth/kubernetes-exchange".to_owned(),
                json!({"type": "kubernetes"})
            ),
            (
                "auth/kubernetes-exchange/config".to_owned(),
                json!({"kubernetes_host": "https://10.43.0.1:443"})
            ),
        ])
    );
    let outside = KubernetesAuth {
        host: "https://127.0.0.1:6443".to_owned(),
        ca_pem: Some(PEM.to_owned()),
    };
    let requests = kubernetes_mount_requests(&Mounts::default(), &outside);
    assert!(requests.is_ok(), "{requests:?}");
    let configs: Vec<Value> = requests
        .unwrap_or_default()
        .into_iter()
        .filter(|(path, _)| path.ends_with("/config"))
        .map(|(_, body)| body)
        .collect();
    assert_eq!(
        configs,
        vec![json!({"kubernetes_host": "https://127.0.0.1:6443", "kubernetes_ca_cert": PEM}); 2]
    );
}

#[test]
fn a_plain_address_a_bad_ca_or_a_bad_mount_sets_nothing_up() {
    let fine = KubernetesAuth {
        host: "https://10.43.0.1:443".to_owned(),
        ca_pem: None,
    };
    let fine_requests = kubernetes_mount_requests(&Mounts::default(), &fine);
    assert!(fine_requests.is_ok(), "{fine_requests:?}");
    for host in ["http://10.43.0.1:443", "", "https://", "10.43.0.1:443"] {
        let auth = KubernetesAuth {
            host: host.to_owned(),
            ca_pem: None,
        };
        assert_eq!(
            kubernetes_mount_requests(&Mounts::default(), &auth),
            Err(SourceError::Config),
            "{host:?}"
        );
    }
    for ca in [
        "",
        "not a certificate",
        "-----BEGIN CERTIFICATE-----\nMIIB\n",
    ] {
        let auth = KubernetesAuth {
            host: fine.host.clone(),
            ca_pem: Some(ca.to_owned()),
        };
        assert_eq!(
            kubernetes_mount_requests(&Mounts::default(), &auth),
            Err(SourceError::Config),
            "{ca:?}"
        );
    }
    for broken in [
        Mounts {
            kubernetes_executor: "a/b".to_owned(),
            ..Mounts::default()
        },
        Mounts {
            kubernetes_exchange: String::new(),
            ..Mounts::default()
        },
    ] {
        assert_eq!(
            kubernetes_mount_requests(&broken, &fine),
            Err(SourceError::Config),
            "{broken:?}"
        );
    }
}

/// The accessor is read from `sys/auth`'s `data`, where each mount is keyed with its trailing
/// slash; one missing or able to end a template is refused.
#[test]
fn a_mounts_accessor_is_read_from_the_vaults_list_and_must_be_plain() {
    let listed = json!({
        "data": {
            "kubernetes-executor/": {"type": "kubernetes", "accessor": "auth_kubernetes_1f88c709"},
            "kubernetes-exchange/": {"type": "kubernetes", "accessor": "auth_kubernetes_2a7b"},
            "evil/": {"type": "kubernetes", "accessor": "auth}}x"}
        }
    });
    assert_eq!(
        mount_accessor(Some(&listed), "kubernetes-executor"),
        Ok("auth_kubernetes_1f88c709".to_owned())
    );
    assert_eq!(
        mount_accessor(Some(&listed), "kubernetes-exchange"),
        Ok("auth_kubernetes_2a7b".to_owned())
    );
    for mount in ["evil", "approle", "kubernetes-executor/"] {
        assert_eq!(
            mount_accessor(Some(&listed), mount),
            Err(SourceError::Malformed),
            "{mount}"
        );
    }
    assert_eq!(
        mount_accessor(None, "kubernetes-executor"),
        Err(SourceError::Malformed)
    );
}
