//! The account fingerprint's paths and grants (DEC-2345): the executor creates its connection's
//! fingerprint entry once and asks transit for the hash, and nothing else of transit; the
//! connection manager reads each fingerprint entry, reads and changes the registry, and reads the
//! rotation marker; no other identity reaches any of them.

use mandate_connections::record::{AccountRef, ConnectionId};

use crate::SourceError;
use crate::fingerprint::{AccountFingerprint, FINGERPRINT_KEY};
use crate::kubernetes::kubernetes_policy;
use crate::policy::{Access, Identity, Mounts, SecretPath, allows, kv_path, openbao_policy};

/// A transit HMAC-SHA256 output: `vault:v1:` and 44 characters of base64.
const HASH: &str = "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=";

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

/// The account stream of a connect: the first, or a second request on the same connection.
fn a(n: u8) -> AccountRef {
    AccountRef::new(&format!("01J8Z2ACCT000000000000000{n}")).unwrap()
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

fn paths() -> Vec<SecretPath> {
    vec![
        SecretPath::Fingerprint(c("c1"), a(1)),
        SecretPath::Fingerprint(c("c1"), a(2)),
        SecretPath::Fingerprint(c("c2"), a(1)),
        SecretPath::Claims,
        SecretPath::Rotating,
    ]
}

/// The oracle, written out from DEC-2345: anything else is refused.
fn expected() -> Vec<(Identity, Access, SecretPath)> {
    let mut out = Vec::new();
    for n in [1, 2] {
        out.push((
            Identity::Executor(c("c1")),
            Access::Create,
            SecretPath::Fingerprint(c("c1"), a(n)),
        ));
    }
    for (id, n) in [("c1", 1), ("c1", 2), ("c2", 1)] {
        out.push((
            Identity::ConnectionManager,
            Access::Read,
            SecretPath::Fingerprint(c(id), a(n)),
        ));
    }
    out.push((
        Identity::ConnectionManager,
        Access::Read,
        SecretPath::Claims,
    ));
    out.push((
        Identity::ConnectionManager,
        Access::Write,
        SecretPath::Claims,
    ));
    out.push((
        Identity::ConnectionManager,
        Access::Read,
        SecretPath::Rotating,
    ));
    out
}

/// A fingerprint is transit's HMAC output alone, and prints nothing of it.
#[test]
#[ignore = "pending E21-32"]
fn a_fingerprint_is_transits_hmac_output_and_prints_nothing() {
    let fingerprint = AccountFingerprint::new(HASH).unwrap();
    assert_eq!(fingerprint.as_str(), HASH);
    assert_eq!(format!("{fingerprint:?}"), "AccountFingerprint(..)");
    assert!(
        AccountFingerprint::new("vault:v12:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=").is_ok()
    );
    for malformed in [
        "",
        "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=",
        "vault:v:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=",
        "vault:vx:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJ=",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA==",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRW\"JA=",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWe/A=\n",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWe JA=",
        "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA",
    ] {
        assert_eq!(
            AccountFingerprint::new(malformed),
            Err(SourceError::Malformed),
            "{malformed:?}"
        );
    }
}

/// The fingerprint entry is in the personal-data vault beside the account entry, one per request,
/// under the connection's own segment and the request's account stream; the registry is one entry
/// there; the marker is outside it.
#[test]
#[ignore = "pending E21-32"]
fn the_fingerprint_paths_are_pinned() {
    assert_eq!(
        kv_path(&SecretPath::Fingerprint(c("c1"), a(1))).unwrap(),
        "personal/fingerprints/x6331/01J8Z2ACCT0000000000000001"
    );
    assert_eq!(kv_path(&SecretPath::Claims).unwrap(), "personal/claims");
    assert_eq!(
        kv_path(&SecretPath::Rotating).unwrap(),
        "fingerprints/rotating"
    );
    assert_eq!(FINGERPRINT_KEY, "account-fingerprint");
    assert_eq!(Mounts::default().transit, "transit");
}

/// The table grants exactly the oracle on the fingerprint paths: the executor creates only its
/// own entry, and only the connection manager reads entries, the registry and the marker, and
/// changes the registry.
#[test]
#[ignore = "pending E21-32"]
fn the_table_allows_exactly_the_fingerprint_matrix() {
    let expected = expected();
    for identity in identities() {
        for access in [
            Access::Read,
            Access::Write,
            Access::Delete,
            Access::Take,
            Access::Create,
        ] {
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

/// The `path` blocks of an HCL policy under `prefix`, as (path, capabilities).
fn blocks_under(hcl: &str, prefixes: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in hcl.split("path \"").skip(1) {
        let (path, rest) = block.split_once('"').unwrap();
        let caps = rest.split_once('[').unwrap().1.split_once(']').unwrap().0;
        if prefixes.iter().any(|prefix| path.starts_with(prefix)) {
            out.push((path.to_owned(), caps.replace([' ', '"'], "")));
        }
    }
    out
}

const FINGERPRINT_PREFIXES: [&str; 4] = [
    "mandate/data/personal/fingerprints/",
    "mandate/data/personal/claims",
    "mandate/data/fingerprints/",
    "transit/",
];

/// The connection manager's generated policy reads and changes the one registry entry by
/// check-and-set and reads the marker, each path spelled out; no other identity's generated policy
/// names any of them, or transit. Its read of each request's fingerprint entry is DEC-2349's
/// one-segment prefix grant, `personal/fingerprints/+/+`, which that decision's policy renders.
#[test]
#[ignore = "pending E21-32"]
fn the_managers_policy_reads_each_fingerprint_and_changes_the_registry() {
    let connections = [c("c1"), c("c2")];
    for identity in identities() {
        let hcl = openbao_policy(&identity, &connections, &Mounts::default()).unwrap();
        let got = blocks_under(&hcl, &FINGERPRINT_PREFIXES);
        let want: Vec<(String, String)> = if identity == Identity::ConnectionManager {
            [
                ("mandate/data/personal/claims", "read"),
                ("mandate/data/personal/claims", "create,update"),
                ("mandate/data/fingerprints/rotating", "read"),
            ]
            .iter()
            .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
            .collect()
        } else {
            Vec::new()
        };
        assert_eq!(got, want, "{identity:?}: {hcl}");
    }
}

/// The executor's templated policy creates its own connection's fingerprint entry and updates
/// the one transit HMAC path of the fingerprint key: no other transit path, so it can hash an id
/// but never read, export or verify against the key. The token exchange's names none of them.
#[test]
#[ignore = "pending E21-32"]
fn the_executor_creates_its_fingerprint_and_only_hashes() {
    let accessor = "auth_kubernetes_1a2b";
    let executor =
        kubernetes_policy(&Identity::Executor(c("c1")), &Mounts::default(), accessor).unwrap();
    let own =
        format!("x{{{{identity.entity.aliases.{accessor}.metadata.service_account_name}}}}/+");
    assert_eq!(
        blocks_under(&executor, &FINGERPRINT_PREFIXES),
        [
            (
                format!("mandate/data/personal/fingerprints/{own}"),
                "create".to_owned()
            ),
            (
                format!("transit/hmac/{FINGERPRINT_KEY}"),
                "update".to_owned()
            ),
        ],
        "{executor}"
    );
    assert_eq!(executor.matches("transit/").count(), 1, "{executor}");
    let exchange = kubernetes_policy(
        &Identity::TokenExchange(c("c1")),
        &Mounts::default(),
        accessor,
    )
    .unwrap();
    assert!(
        blocks_under(&exchange, &FINGERPRINT_PREFIXES).is_empty(),
        "{exchange}"
    );
}
