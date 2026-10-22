//! DEC-2349: the Api's and the connection manager's policies reach a connection made after the
//! vault was provisioned, as a connect from the web makes one, by a one-segment `+` on exactly the
//! paths they may write or delete, and on nothing they may read. No other identity holds a `+`,
//! nothing holds a `*`, and the executor's and the token exchange's templated policies stay exact.

use mandate_connections::record::ConnectionId;

use crate::policy::{Access, Identity, Mounts, SecretPath, allows};

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn openbao_policy(identity: &Identity, connections: &[ConnectionId]) -> String {
    crate::policy::openbao_policy(identity, connections, &Mounts::default()).unwrap()
}

fn kv_path(path: &SecretPath) -> String {
    crate::policy::kv_path(path).unwrap()
}

/// The `path` blocks of an HCL policy, as (path, capabilities).
fn blocks(hcl: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in hcl.split("path \"").skip(1) {
        let (path, rest) = block.split_once('"').unwrap();
        let caps = rest.split_once('[').unwrap().1.split_once(']').unwrap().0;
        out.push((path.to_owned(), caps.replace([' ', '"'], "")));
    }
    out
}

/// Whether `pattern`, a policy path whose `+` stands for exactly one segment, names `path`, as
/// OpenBao's ACL reads it: a `+` matches any one segment, an empty one included.
fn names(pattern: &str, path: &str) -> bool {
    let (pattern, path): (Vec<&str>, Vec<&str>) =
        (pattern.split('/').collect(), path.split('/').collect());
    pattern.len() == path.len() && pattern.iter().zip(&path).all(|(p, s)| *p == "+" || p == s)
}

/// Whether `hcl` grants `access` on `path`, read the way OpenBao reads it: a block whose path names
/// the secret's, with the capabilities the access needs.
fn grants(hcl: &str, access: Access, path: &SecretPath) -> bool {
    let (prefix, caps) = match access {
        Access::Read => ("data", "read"),
        Access::Write => ("data", "create,update"),
        Access::Delete => ("metadata", "delete"),
        Access::Take => ("data", "update"),
    };
    let secret = format!("mandate/{prefix}/{}", kv_path(path));
    blocks(hcl)
        .iter()
        .any(|(pattern, granted)| names(pattern, &secret) && granted == caps)
}

/// Connections no provisioning named, shaped as the ids a connect mints.
fn unnamed() -> Vec<ConnectionId> {
    vec![c("c9"), c("conn_01J8Z2NEW0000000000000A1"), c("z")]
}

fn identities() -> Vec<Identity> {
    vec![
        Identity::Api,
        Identity::ConnectionManager,
        Identity::TokenExchange(c("c1")),
        Identity::Executor(c("c1")),
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ]
}

const API_PREFIX: [(&str, &str); 3] = [
    ("mandate/data/connections/+/code", "create,update"),
    ("mandate/metadata/connections/+/code", "delete"),
    ("mandate/metadata/connections/+/token", "delete"),
];
const MANAGER_PREFIX: [(&str, &str); 2] = [
    ("mandate/metadata/connections/+/code", "delete"),
    ("mandate/metadata/connections/+/token", "delete"),
];

#[test]
#[ignore = "pending E21-33"]
fn the_api_and_the_manager_reach_a_connection_no_provisioning_named() {
    for listed in [vec![], vec![c("c1")], vec![c("c1"), c("c2")]] {
        for identity in [Identity::Api, Identity::ConnectionManager] {
            let hcl = openbao_policy(&identity, &listed);
            for connection in unnamed().into_iter().chain(listed.clone()) {
                for path in [
                    SecretPath::Code(connection.clone()),
                    SecretPath::Token(connection.clone()),
                ] {
                    for access in [Access::Read, Access::Write, Access::Delete, Access::Take] {
                        assert_eq!(
                            grants(&hcl, access, &path),
                            allows(&identity, access, &path),
                            "{identity:?} {access:?} {path:?} with {listed:?}: {hcl}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "pending E21-33"]
fn the_connection_blocks_are_the_prefixes_alone_whatever_is_listed() {
    for listed in [vec![], vec![c("c1")], vec![c("c1"), c("c2"), c("c3")]] {
        for (identity, want) in [
            (Identity::Api, API_PREFIX.to_vec()),
            (Identity::ConnectionManager, MANAGER_PREFIX.to_vec()),
        ] {
            let hcl = openbao_policy(&identity, &listed);
            let mut got: Vec<(String, String)> = blocks(&hcl)
                .into_iter()
                .filter(|(p, _)| p.contains("/connections/"))
                .collect();
            got.sort();
            let mut want: Vec<(String, String)> = want
                .iter()
                .map(|(p, caps)| ((*p).to_owned(), (*caps).to_owned()))
                .collect();
            want.sort();
            assert_eq!(got, want, "{identity:?} with {listed:?}: {hcl}");
        }
    }
}

#[test]
fn no_wildcard_reads_and_none_outside_the_api_and_the_manager() {
    for identity in identities() {
        let hcl = openbao_policy(&identity, &[c("c1"), c("c2")]);
        assert!(!hcl.contains('*'), "no `*`, ever: {identity:?}: {hcl}");
        for (path, caps) in blocks(&hcl).into_iter().filter(|(p, _)| p.contains('+')) {
            let allowed: &[(&str, &str)] = match identity {
                Identity::Api => &API_PREFIX,
                Identity::ConnectionManager => &MANAGER_PREFIX,
                _ => &[],
            };
            assert!(
                allowed.iter().any(|(p, c)| *p == path && *c == caps),
                "{identity:?} holds `+` on {path} [{caps}]: {hcl}"
            );
            assert!(
                !caps.contains("read") && !caps.contains("list"),
                "a `+` never reads or lists: {identity:?} {path}"
            );
        }
    }
}

#[test]
fn the_templated_policies_stay_exact() {
    let any = c("0");
    for identity in [
        Identity::Executor(any.clone()),
        Identity::TokenExchange(any),
    ] {
        let hcl = crate::kubernetes::kubernetes_policy(
            &identity,
            &Mounts::default(),
            "auth_kubernetes_0a1b",
        )
        .unwrap();
        assert!(!hcl.contains('*'), "{identity:?}: {hcl}");
        for (path, caps) in blocks(&hcl).into_iter().filter(|(p, _)| p.contains('+')) {
            assert!(
                caps == "create"
                    && path
                        .split('/')
                        .rev()
                        .nth(1)
                        .is_some_and(|own| own.starts_with("x{{"))
                    && path.ends_with("/+"),
                "a templated `+` only creates a last segment under the login's own name (DEC-2345): {identity:?} {path} [{caps}]"
            );
        }
    }
}

#[test]
fn the_pattern_reader_names_any_one_segment_for_a_plus_and_nothing_else() {
    assert!(names("a/+/b", "a/x1/b"));
    assert!(!names("a/+/b", "a/x1/y/b"));
    assert!(
        names("a/+/b", "a//b"),
        "OpenBao's `+` matches an empty segment too"
    );
    assert!(!names("a/+/b", "a/x1/c"));
    assert!(names("a/x1/b", "a/x1/b"));
    assert!(!names("a/x1/b", "a/x2/b"));
}
