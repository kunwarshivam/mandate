//! Every hand-written `Debug` prints its type and its non-secret fields, and nothing else.

use std::sync::Arc;

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use secrecy::{SecretSlice, SecretString};

use crate::lease::Lease;
use crate::local::GrantBook;
use crate::openbao::{OpenBaoConfig, RoleCredentials};
use crate::policy::{Identity, Mounts};
use crate::source::Grant;

const SECRET: &str = "debug-canary-91c2";

#[test]
fn each_debug_names_its_type_and_hides_its_secret() {
    let connection = ConnectionId::new("c1").unwrap();
    let at = UtcNanos::from_parts(1_000, 0).unwrap();
    let cases = [
        (
            format!(
                "{:?}",
                Lease {
                    connection: connection.clone(),
                    secret: SecretSlice::from(SECRET.as_bytes().to_vec()),
                    expires_at: at,
                    renew_at: at,
                }
            ),
            "Lease { connection: ConnectionId(\"c1\"), expires_at: ",
        ),
        (
            format!(
                "{:?}",
                Grant {
                    connection: connection.clone(),
                    role_id: "role".to_owned(),
                    secret_id: SecretString::from(SECRET.to_owned()),
                    accessor: "acc".to_owned(),
                    expires_at: at,
                }
            ),
            "Grant { connection: ConnectionId(\"c1\"), expires_at: ",
        ),
        (
            format!(
                "{:?}",
                OpenBaoConfig {
                    address: "https://127.0.0.1:8200".to_owned(),
                    ca_pem: SECRET.as_bytes().to_vec(),
                    mounts: Mounts::default(),
                }
            ),
            "OpenBaoConfig { address: \"https://127.0.0.1:8200\", mounts: ",
        ),
        (
            format!(
                "{:?}",
                RoleCredentials {
                    identity: Identity::Runtime,
                    role_id: "role".to_owned(),
                    secret_id: SecretString::from(SECRET.to_owned()),
                }
            ),
            "RoleCredentials { identity: Runtime, .. }",
        ),
        (
            format!("{:?}", Arc::new(GrantBook::default())),
            "GrantBook(redacted)",
        ),
    ];
    for (printed, start) in cases {
        assert!(printed.starts_with(start), "{printed}");
        assert!(!printed.contains(SECRET), "{printed}");
    }
}
