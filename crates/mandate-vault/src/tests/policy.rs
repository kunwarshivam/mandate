//! The policy table, checked against a matrix written out by hand, and the OpenBao policies it
//! generates: every path spelled out, no wildcard, a runtime's empty.

use mandate_connections::record::ConnectionId;

use crate::policy::{
    Access, Identity, Mounts, PlatformSecret, SecretPath, allows, policy_name, role_name,
};

/// [`crate::policy::kv_path`], which fails only for an id it cannot place; every id here is one it
/// places.
fn kv_path(path: &SecretPath) -> String {
    crate::policy::kv_path(path).unwrap()
}

/// [`crate::policy::openbao_policy`], which fails only where [`kv_path`] does.
fn openbao_policy(identity: &Identity, connections: &[ConnectionId], mounts: &Mounts) -> String {
    crate::policy::openbao_policy(identity, connections, mounts).unwrap()
}

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn identities() -> Vec<Identity> {
    vec![
        Identity::Api,
        Identity::ConnectionManager,
        Identity::TokenExchange(c("c1")),
        Identity::Executor(c("c1")),
        Identity::Runtime,
        Identity::SecretImport,
    ]
}

fn paths() -> Vec<SecretPath> {
    vec![
        SecretPath::Code(c("c1")),
        SecretPath::Token(c("c1")),
        SecretPath::Code(c("c2")),
        SecretPath::Token(c("c2")),
        SecretPath::ClientSecret,
    ]
}

/// The oracle: every allowed triple, written out from infrastructure §5.2; anything else is
/// refused.
fn expected() -> Vec<(Identity, Access, SecretPath)> {
    use Access::{Delete, Read, Write};
    let mut out = Vec::new();
    for id in ["c1", "c2"] {
        out.push((Identity::Api, Write, SecretPath::Code(c(id))));
        out.push((Identity::Api, Delete, SecretPath::Code(c(id))));
        out.push((Identity::Api, Delete, SecretPath::Token(c(id))));
        out.push((Identity::ConnectionManager, Delete, SecretPath::Code(c(id))));
        out.push((
            Identity::ConnectionManager,
            Delete,
            SecretPath::Token(c(id)),
        ));
    }
    let exchange = Identity::TokenExchange(c("c1"));
    out.push((exchange.clone(), Read, SecretPath::Code(c("c1"))));
    out.push((exchange.clone(), Delete, SecretPath::Code(c("c1"))));
    out.push((exchange.clone(), Write, SecretPath::Token(c("c1"))));
    out.push((exchange, Read, SecretPath::ClientSecret));
    out.push((
        Identity::Executor(c("c1")),
        Read,
        SecretPath::Token(c("c1")),
    ));
    out.push((Identity::SecretImport, Write, SecretPath::ClientSecret));
    out
}

#[test]
fn the_table_allows_exactly_the_matrix() {
    let expected = expected();
    for identity in identities() {
        for access in [Access::Read, Access::Write, Access::Delete] {
            for path in paths() {
                let want = expected
                    .iter()
                    .any(|(i, a, p)| i == &identity && *a == access && p == &path);
                assert_eq!(
                    allows(&identity, access, &path),
                    want,
                    "{identity:?} {access:?} {path:?}"
                );
            }
        }
    }
}

#[test]
fn a_runtime_and_an_executor_read_nothing_else() {
    for path in paths() {
        assert!(!allows(&Identity::Runtime, Access::Read, &path));
        assert_eq!(
            allows(&Identity::Executor(c("c2")), Access::Read, &path),
            path == SecretPath::Token(c("c2")),
            "{path:?}"
        );
    }
}

#[test]
#[ignore = "pending E21-33"]
fn names_and_paths_are_pinned() {
    assert_eq!(role_name(&Identity::Api), "api");
    assert_eq!(
        role_name(&Identity::ConnectionManager),
        "connection-manager"
    );
    assert_eq!(
        role_name(&Identity::TokenExchange(c("c_1"))),
        "exchange-c_1"
    );
    assert_eq!(role_name(&Identity::Executor(c("c-2"))), "executor-c-2");
    assert_eq!(role_name(&Identity::Runtime), "runtime");
    assert_eq!(role_name(&Identity::SecretImport), "secret-import");
    assert_eq!(
        kv_path(&SecretPath::ClientSecret),
        "platform/alpaca-oauth-client-secret"
    );
    let other = Mounts {
        kv: "kv9".to_owned(),
        auth: "ar9".to_owned(),
        ..Mounts::default()
    };
    assert_eq!(
        policy_name(&Identity::Api, &Mounts::default()),
        "mandate-api"
    );
    assert_eq!(
        policy_name(&Identity::Executor(c("c1")), &other),
        "kv9-executor-c1"
    );
    let hcl = openbao_policy(&Identity::ConnectionManager, &[c("c1")], &other);
    assert!(
        hcl.contains("path \"kv9/metadata/connections/+/token\""),
        "{hcl}"
    );
    assert!(
        hcl.contains("path \"auth/ar9/role/exchange-c1/secret-id\""),
        "{hcl}"
    );
    assert!(
        !hcl.contains("mandate/") && !hcl.contains("approle/"),
        "{hcl}"
    );
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

/// Whether DEC-2349's one-segment `+` grants `access` on `path` to the Api or the connection
/// manager in place of a spelled-out block: a write or a delete of a connection's code or token.
fn by_prefix(access: Access, path: &SecretPath) -> bool {
    matches!(access, Access::Write | Access::Delete)
        && matches!(path, SecretPath::Code(_) | SecretPath::Token(_))
}

/// Every policy is the table with no wildcard, but the Api's and the connection manager's
/// one-segment `+` on their connection write and delete paths (DEC-2349), which `policy_prefix`
/// pins: only those paths go unchecked here, every read and every other path is checked as before.
#[test]
#[ignore = "pending E21-33"]
fn each_policy_is_the_table_with_no_wildcard_beyond_dec_2349s() {
    let connections = [c("c1"), c("c2")];
    let platform_paths: Vec<String> = PlatformSecret::ALL
        .iter()
        .map(|s| format!("mandate/data/{}", kv_path(&SecretPath::Platform(*s))))
        .collect();
    for identity in identities() {
        let hcl = openbao_policy(&identity, &connections, &Mounts::default());
        let prefixed = matches!(identity, Identity::Api | Identity::ConnectionManager);
        assert!(
            !hcl.contains('*') && (prefixed || !hcl.contains('+')),
            "{identity:?}: {hcl}"
        );
        let got: Vec<(String, String)> = blocks(&hcl)
            .into_iter()
            .filter(|(p, _)| !p.contains('+'))
            .collect();
        let data_blocks = got
            .iter()
            .filter(|(p, _)| p.starts_with("mandate/") && !platform_paths.contains(p))
            .count();
        let allowed = [Access::Read, Access::Write, Access::Delete]
            .iter()
            .flat_map(|a| paths().into_iter().map(move |p| (*a, p)))
            .filter(|(a, p)| allows(&identity, *a, p))
            .filter(|(a, p)| !(prefixed && by_prefix(*a, p)))
            .count();
        assert_eq!(
            data_blocks, allowed,
            "no block beyond the table: {identity:?}: {hcl}"
        );
        let auth_blocks = got.iter().filter(|(p, _)| p.starts_with("auth/")).count();
        let platform_blocks = got
            .iter()
            .filter(|(p, _)| platform_paths.contains(p))
            .count();
        let want_auth = if identity == Identity::ConnectionManager {
            8
        } else {
            0
        };
        assert_eq!(auth_blocks, want_auth, "{identity:?}: {hcl}");
        assert_eq!(
            got.len(),
            data_blocks + auth_blocks + platform_blocks,
            "{identity:?}: {hcl}"
        );
        for access in [Access::Read, Access::Write, Access::Delete] {
            for path in paths() {
                let (prefix, caps) = match access {
                    Access::Read => ("data", "read"),
                    Access::Write => ("data", "create,update"),
                    Access::Delete => ("metadata", "delete"),
                    Access::Take => ("data", "update"),
                };
                if prefixed && by_prefix(access, &path) && allows(&identity, access, &path) {
                    continue;
                }
                let want = format!("mandate/{prefix}/{}", kv_path(&path));
                let present = got.iter().any(|(p, c)| p == &want && c == caps);
                assert_eq!(
                    present,
                    allows(&identity, access, &path),
                    "{identity:?} {access:?} {path:?}: {hcl}"
                );
            }
        }
    }
    assert!(
        blocks(&openbao_policy(
            &Identity::Runtime,
            &connections,
            &Mounts::default()
        ))
        .is_empty()
    );
}

#[test]
fn the_manager_may_issue_and_revoke_only_exchange_grants() {
    let hcl = openbao_policy(&Identity::ConnectionManager, &[c("c1")], &Mounts::default());
    let got = blocks(&hcl);
    for (path, caps) in [
        ("auth/approle/role/exchange-c1/secret-id", "update"),
        (
            "auth/approle/role/exchange-c1/secret-id-accessor/lookup",
            "update",
        ),
        (
            "auth/approle/role/exchange-c1/secret-id-accessor/destroy",
            "update",
        ),
        ("auth/approle/role/exchange-c1/role-id", "read"),
    ] {
        assert!(
            got.iter().any(|(p, c)| p == path && c == caps),
            "{path}: {hcl}"
        );
    }
    let grant_blocks = got.iter().filter(|(p, _)| p.starts_with("auth/")).count();
    assert_eq!(grant_blocks, 4, "{hcl}");
    for identity in identities() {
        if identity != Identity::ConnectionManager {
            let hcl = openbao_policy(&identity, &[c("c1")], &Mounts::default());
            assert!(!hcl.contains("auth/"), "{identity:?}: {hcl}");
        }
    }
}

/// DEC-2061: only a connection's token exchange may take, and only its own connection's code: no
/// other identity, no other connection's code, and no token or secret.
#[test]
fn only_a_token_exchange_takes_and_only_its_own_connections_code() {
    let exchange = Identity::TokenExchange(c("c1"));
    assert!(allows(&exchange, Access::Take, &SecretPath::Code(c("c1"))));
    for identity in identities() {
        for path in paths() {
            let want = identity == exchange && path == SecretPath::Code(c("c1"));
            assert_eq!(
                allows(&identity, Access::Take, &path),
                want,
                "{identity:?} {path:?}"
            );
        }
    }
    for identity in identities() {
        let hcl = openbao_policy(&identity, &[c("c1"), c("c2")], &Mounts::default());
        assert!(
            !blocks(&hcl)
                .iter()
                .any(|(path, caps)| path.starts_with("mandate/data/") && caps == "update"),
            "an AppRole policy never grants a take on a secret: {identity:?}: {hcl}"
        );
    }
}
