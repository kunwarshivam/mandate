//! The conformance suite over the local vault, in a temporary layout owned by the test's user,
//! which plays root and both service users.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::conformance::{self, Harness};
use crate::SourceError;
use crate::local::{GrantBook, LocalSource};
use crate::policy::Identity;
use crate::source::{Grant, SecretSource};
use mandate_connections::record::{ConnectionId, Environment};
use mandate_time::UtcNanos;
use mandate_vault_local::startup::{Ids, Inputs, Role};
use mandate_vault_local::store::{Opened, open};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Local {
    root: PathBuf,
    ids: Ids,
    grants: Arc<GrantBook>,
}

impl Local {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("secret-source-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = root.join("vault");
        for dir in [
            vault.clone(),
            vault.join("pending"),
            vault.join("tokens"),
            root.join("api"),
            root.join("exec"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        let mode =
            |p: PathBuf, m: u32| fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
        mode(vault.clone(), 0o755);
        mode(vault.join("pending"), 0o2770);
        mode(vault.join("tokens"), 0o730);
        fs::write(root.join("api/vault-pending-key"), [1; 32]).unwrap();
        fs::write(root.join("exec/vault-pending-key"), [1; 32]).unwrap();
        fs::write(root.join("exec/vault-token-key"), [2; 32]).unwrap();
        let meta = fs::metadata(&vault).unwrap();
        let ids = Ids {
            root_uid: meta.uid(),
            api_uid: meta.uid(),
            api_gid: meta.gid(),
            exec_uid: meta.uid(),
            exec_gid: meta.gid(),
        };
        Self {
            root,
            ids,
            grants: Arc::new(GrantBook::default()),
        }
    }

    fn open(&self, role: Role) -> Opened {
        let credentials = self.root.join(match role {
            Role::Api => "api",
            Role::Executor => "exec",
        });
        let environment = Vec::new();
        let inputs = Inputs {
            role,
            vault_dir: &self.root.join("vault"),
            credentials_dir: &credentials,
            environment: &environment,
            ids: self.ids,
        };
        open(&inputs).unwrap()
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        let tokens = self.root.join("vault/tokens");
        if tokens.is_file() {
            let _ = fs::remove_file(&tokens);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Harness for Local {
    type Source = LocalSource;

    async fn source(&self, identity: Identity, _: UtcNanos) -> LocalSource {
        let vault = match identity {
            Identity::Api | Identity::ConnectionManager => Some(self.open(Role::Api)),
            Identity::Runtime => None,
            _ => Some(self.open(Role::Executor)),
        };
        LocalSource::new(
            identity,
            vault,
            Environment::Paper,
            Arc::clone(&self.grants),
        )
        .unwrap()
    }

    async fn taker(&self, connection: &ConnectionId, now: UtcNanos) -> LocalSource {
        let manager = self.source(Identity::ConnectionManager, now).await;
        let grant = manager.issue_grant(connection, now).await.unwrap();
        self.exchange(&grant, now).await.unwrap()
    }

    async fn exchange(&self, grant: &Grant, now: UtcNanos) -> Result<LocalSource, SourceError> {
        LocalSource::with_grant(
            grant,
            self.open(Role::Executor),
            Environment::Paper,
            Arc::clone(&self.grants),
            now,
        )
    }

    /// `tokens/` replaced by a regular file: every read under it fails to stat, which is an
    /// outage, not a refusal.
    async fn outage(&self) {
        let tokens = self.root.join("vault/tokens");
        fs::rename(&tokens, self.root.join("tokens-aside")).unwrap();
        fs::write(&tokens, b"").unwrap();
    }

    async fn restore(&self) {
        let tokens = self.root.join("vault/tokens");
        fs::remove_file(&tokens).unwrap();
        fs::rename(self.root.join("tokens-aside"), &tokens).unwrap();
    }
}

macro_rules! conformance {
    ($($case:ident),* $(,)?) => {
        $(
            #[tokio::test(flavor = "current_thread")]
            async fn $case() {
                conformance::$case(&Local::new()).await;
            }
        )*
    };
}

conformance!(
    the_connect_flow_runs_end_to_end,
    every_identity_is_held_to_its_policy,
    leases_and_grants_are_held_to_their_owners,
    a_grant_is_single_use_and_revocable,
    an_outage_stops_no_executor_before_its_lease_expires,
    a_revoked_connection_ends_its_lease_at_the_next_renewal,
    no_debug_or_refusal_shows_a_secret,
    after_revoke_the_session_takes_no_lease,
);

#[tokio::test(flavor = "current_thread")]
async fn a_session_must_hold_the_vault_its_identity_runs_as() {
    let h = Local::new();
    let cases = [
        (Identity::Api, Some(Role::Executor)),
        (Identity::Api, None),
        (Identity::ConnectionManager, Some(Role::Executor)),
        (Identity::Executor(conformance::c("c1")), Some(Role::Api)),
        (Identity::Executor(conformance::c("c1")), None),
        (Identity::SecretImport, Some(Role::Api)),
        (Identity::Runtime, Some(Role::Executor)),
        (Identity::Runtime, Some(Role::Api)),
        (
            Identity::TokenExchange(conformance::c("c1")),
            Some(Role::Executor),
        ),
    ];
    for (identity, role) in cases {
        let vault = role.map(|r| h.open(r));
        assert_eq!(
            LocalSource::new(
                identity.clone(),
                vault,
                Environment::Paper,
                Arc::clone(&h.grants)
            )
            .map(drop),
            Err(SourceError::Config),
            "{identity:?} {role:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_expired_or_foreign_grant_is_refused() {
    let h = Local::new();
    let now = conformance::at(1_000_000);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = crate::source::SecretSource::issue_grant(&manager, &conformance::c("c1"), now)
        .await
        .unwrap();
    let late = conformance::at(1_000_000 + crate::source::GRANT_SECS);
    assert_eq!(
        h.exchange(&grant, late).await.map(drop),
        Err(SourceError::GrantInvalid)
    );
    let other_book = Local::new();
    assert_eq!(
        other_book.exchange(&grant, now).await.map(drop),
        Err(SourceError::GrantInvalid),
        "a grant another process's book never issued"
    );
    let forged = Grant {
        connection: conformance::c("c2"),
        role_id: grant.role_id.clone(),
        secret_id: grant.secret_id.clone(),
        accessor: grant.accessor.clone(),
        expires_at: grant.expires_at,
    };
    assert_eq!(
        h.exchange(&forged, now).await.map(drop),
        Err(SourceError::GrantInvalid),
        "a grant moved onto another connection"
    );
    assert!(h.exchange(&grant, now).await.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn a_tampered_token_is_an_integrity_refusal_that_drops_the_lease() {
    let h = Local::new();
    let now = conformance::at(1_000_000);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = crate::source::SecretSource::issue_grant(&manager, &conformance::c("c1"), now)
        .await
        .unwrap();
    let exchange = h.exchange(&grant, now).await.unwrap();
    crate::source::SecretSource::put(
        &exchange,
        &crate::policy::SecretPath::Token(conformance::c("c1")),
        &conformance::secret("token-1"),
    )
    .await
    .unwrap();
    let executor = h
        .source(Identity::Executor(conformance::c("c1")), now)
        .await;
    let lease = crate::source::SecretSource::lease(&executor, &conformance::c("c1"), now)
        .await
        .unwrap();
    for entry in fs::read_dir(h.root.join("vault/tokens")).unwrap() {
        let path = entry.unwrap().path();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
    }
    let renewed = crate::source::SecretSource::renew(&executor, &lease, now).await;
    assert_eq!(renewed.as_ref().map(drop), Err(&SourceError::Integrity));
    let mut keeper = crate::lease::LeaseKeeper::new(lease);
    keeper.apply(renewed, now);
    assert!(keeper.secret(now).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn each_local_grant_has_its_own_random_accessor_and_secret_id() {
    let h = Local::new();
    let now = conformance::at(1_000_000);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..4 {
        let grant = crate::source::SecretSource::issue_grant(&manager, &conformance::c("c1"), now)
            .await
            .unwrap();
        let secret = secrecy::ExposeSecret::expose_secret(&grant.secret_id).to_owned();
        assert_eq!(grant.accessor.len(), 32);
        assert_eq!(secret.len(), 64);
        for text in [&grant.accessor, &secret] {
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
            assert!(seen.insert(text.clone()), "a repeated id");
        }
    }
}

/// DEC-2020: the platform's own secrets, on the local vault.
#[tokio::test(flavor = "current_thread")]
async fn a_platform_secret_is_written_by_import_and_read_by_its_one_process() {
    conformance::the_platform_secrets_have_their_holders();
    conformance::a_platform_secret_is_written_by_import_and_read_by_its_one_process(&Local::new())
        .await;
}

/// DEC-2020: no output shows a platform secret, on the local vault.
#[tokio::test(flavor = "current_thread")]
async fn no_debug_or_refusal_shows_a_platform_secret() {
    conformance::the_platform_secrets_have_their_holders();
    conformance::no_debug_or_refusal_shows_a_platform_secret(&Local::new()).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "pending E21-32"]
async fn a_code_is_taken_once_and_only_by_its_own_exchange() {
    conformance::a_code_is_taken_once_and_only_by_its_own_exchange(&Local::new()).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "pending E21-32"]
async fn of_two_racing_takers_exactly_one_gets_the_code() {
    conformance::of_two_racing_takers_exactly_one_gets_the_code(&Local::new()).await;
}
