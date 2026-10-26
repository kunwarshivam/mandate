//! One suite both backends pass (E21-32). Each case takes a fresh [`Harness`]: a vault with
//! connections `c1` and `c2`, and a way to open a session as any identity, to spend a grant, and
//! to cut the vault off and bring it back.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    dead_code,
    reason = "ADR-0001 ES-09 excepts tests; each harness file uses the cases it runs"
)]

use crate::SourceError;
use crate::lease::{LEASE_SECS, LeaseKeeper, RENEW_SECS};
use crate::policy::{Access, Holders, Identity, PlatformSecret, SecretPath, holders};
use crate::source::{GRANT_SECS, Grant, SecretSource};
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use secrecy::{ExposeSecret, SecretSlice};

/// A secret no output may contain.
pub const CANARY: &str = "canary-3a9f61d0-secret-source-do-not-print";

#[allow(
    async_fn_in_trait,
    reason = "test harnesses run on one current-thread runtime"
)]
pub trait Harness {
    type Source: SecretSource + std::fmt::Debug;
    /// A session as `identity` (not a token exchange).
    async fn source(&self, identity: Identity, now: UtcNanos) -> Self::Source;
    /// The token exchange's session through `grant`.
    async fn exchange(&self, grant: &Grant, now: UtcNanos) -> Result<Self::Source, SourceError>;
    /// Cuts every session off from the vault, as an outage would.
    async fn outage(&self);
    /// Ends the outage.
    async fn restore(&self);
    /// A session as `connection`'s token exchange that may take its code (DEC-2061): the local
    /// exchange through its grant, and on OpenBao one holding the Kubernetes-auth exchange's
    /// policy, which alone grants the take.
    async fn taker(&self, connection: &ConnectionId, now: UtcNanos) -> Self::Source;
}

pub fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

pub fn secret(text: &str) -> SecretSlice<u8> {
    SecretSlice::from(text.as_bytes().to_vec())
}

fn text(s: &SecretSlice<u8>) -> String {
    String::from_utf8(s.expose_secret().to_vec()).unwrap()
}

/// Puts a token for `connection` the way the platform does: the manager grants, the exchange
/// writes.
async fn store_token<H: Harness>(h: &H, connection: &str, value: &str, now: UtcNanos) {
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c(connection), now).await.unwrap();
    let exchange = h.exchange(&grant, now).await.unwrap();
    exchange
        .put(&SecretPath::Token(c(connection)), &secret(value))
        .await
        .unwrap();
}

/// The connect flow of connections spec §5.2: the API writes the code, the manager grants, the
/// exchange reads the code and the client secret, writes the token, and deletes the code; the
/// executor leases the token.
pub async fn the_connect_flow_runs_end_to_end<H: Harness>(h: &H) {
    let now = at(1_000_000);
    h.source(Identity::SecretImport, now)
        .await
        .put(&SecretPath::ClientSecret, &secret("client-secret"))
        .await
        .unwrap();
    h.source(Identity::Api, now)
        .await
        .put(&SecretPath::Code(c("c1")), &secret("code-and-verifier"))
        .await
        .unwrap();
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    assert_eq!(grant.connection, c("c1"));
    assert_eq!(grant.expires_at, at(1_000_000 + GRANT_SECS));
    let exchange = h.exchange(&grant, now).await.unwrap();
    assert_eq!(exchange.identity(), &Identity::TokenExchange(c("c1")));
    let code = exchange.read(&SecretPath::Code(c("c1"))).await.unwrap();
    assert_eq!(code.map(|s| text(&s)).as_deref(), Some("code-and-verifier"));
    let client = exchange.read(&SecretPath::ClientSecret).await.unwrap();
    assert_eq!(client.map(|s| text(&s)).as_deref(), Some("client-secret"));
    exchange
        .put(&SecretPath::Token(c("c1")), &secret("token-1"))
        .await
        .unwrap();
    exchange.delete(&SecretPath::Code(c("c1"))).await.unwrap();
    assert!(
        exchange
            .read(&SecretPath::Code(c("c1")))
            .await
            .unwrap()
            .is_none()
    );
    manager.revoke_grant(&grant).await.unwrap();

    let executor = h.source(Identity::Executor(c("c1")), now).await;
    let lease = executor.lease(&c("c1"), now).await.unwrap();
    assert_eq!(text(&lease.secret), "token-1");
    assert_eq!(lease.connection, c("c1"));
    assert!(lease.expires_at <= at(1_000_000 + LEASE_SECS));
    assert!(lease.renew_at <= at(1_000_000 + RENEW_SECS));
    assert!(lease.renew_at > now);
}

/// Every identity can do what the matrix allows and is refused everything else, by the vault.
pub async fn every_identity_is_held_to_its_policy<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    let sessions = vec![
        h.source(Identity::Api, now).await,
        manager,
        h.exchange(&grant, now).await.unwrap(),
        h.source(Identity::Executor(c("c1")), now).await,
        h.source(Identity::Runtime, now).await,
        h.source(Identity::SecretImport, now).await,
    ];
    let paths = [
        SecretPath::Code(c("c1")),
        SecretPath::Token(c("c1")),
        SecretPath::Code(c("c2")),
        SecretPath::Token(c("c2")),
        SecretPath::ClientSecret,
    ];
    let allowed = |identity: &Identity, access: Access, path: &SecretPath| -> bool {
        use Access::{Delete, Read, Write};
        let exchange = Identity::TokenExchange(c("c1"));
        let executor = Identity::Executor(c("c1"));
        match (identity, access, path) {
            (Identity::Api, Write, SecretPath::Code(_)) => true,
            (Identity::Api | Identity::ConnectionManager, Delete, SecretPath::Code(_)) => true,
            (Identity::Api | Identity::ConnectionManager, Delete, SecretPath::Token(_)) => true,
            (i, Read, p) if *i == exchange && *p == SecretPath::Code(c("c1")) => true,
            (i, Delete, p) if *i == exchange && *p == SecretPath::Code(c("c1")) => true,
            (i, Write, p) if *i == exchange && *p == SecretPath::Token(c("c1")) => true,
            (i, Read, SecretPath::ClientSecret) if *i == exchange => true,
            (i, Read, p) if *i == executor && *p == SecretPath::Token(c("c1")) => true,
            (Identity::SecretImport, Write, SecretPath::ClientSecret) => true,
            _ => false,
        }
    };
    for session in &sessions {
        let identity = session.identity().clone();
        for path in &paths {
            let wrote = session.put(path, &secret("written")).await;
            assert_eq!(
                wrote.is_ok(),
                allowed(&identity, Access::Write, path),
                "{identity:?} write {path:?}: {wrote:?}"
            );
            if wrote.is_err() {
                assert_eq!(wrote, Err(SourceError::Denied), "{identity:?} {path:?}");
            }
            let read = session.read(path).await.map(|s| s.is_some());
            assert_eq!(
                read.is_ok(),
                allowed(&identity, Access::Read, path),
                "{identity:?} read {path:?}: {read:?}"
            );
            if read.is_err() {
                assert_eq!(read, Err(SourceError::Denied), "{identity:?} {path:?}");
            }
        }
    }
    for session in &sessions {
        let identity = session.identity().clone();
        for path in &paths {
            let deleted = session.delete(path).await;
            assert_eq!(
                deleted.is_ok(),
                allowed(&identity, Access::Delete, path),
                "{identity:?} delete {path:?}: {deleted:?}"
            );
        }
    }
}

/// A runtime reads nothing, an executor exactly its own connection's token, and only the manager
/// issues grants.
pub async fn leases_and_grants_are_held_to_their_owners<H: Harness>(h: &H) {
    let now = at(1_000_000);
    store_token(h, "c1", "token-1", now).await;
    store_token(h, "c2", "token-2", now).await;
    let runtime = h.source(Identity::Runtime, now).await;
    for connection in ["c1", "c2"] {
        assert_eq!(
            runtime.lease(&c(connection), now).await.map(drop),
            Err(SourceError::Denied)
        );
    }
    let executor = h.source(Identity::Executor(c("c1")), now).await;
    assert_eq!(
        executor.lease(&c("c2"), now).await.map(drop),
        Err(SourceError::Denied)
    );
    assert_eq!(
        text(&executor.lease(&c("c1"), now).await.unwrap().secret),
        "token-1"
    );
    for identity in [
        Identity::Api,
        Identity::Executor(c("c1")),
        Identity::Runtime,
        Identity::SecretImport,
    ] {
        let session = h.source(identity.clone(), now).await;
        assert_eq!(
            session.issue_grant(&c("c1"), now).await.map(drop),
            Err(SourceError::Denied),
            "{identity:?}"
        );
    }
}

/// A grant is single-use, bound to its connection, and dead once revoked. Expiry is each
/// harness file's own case, since only it controls the clock the grant is checked at.
pub async fn a_grant_is_single_use_and_revocable<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    let exchange = h.exchange(&grant, now).await.unwrap();
    assert_eq!(
        exchange.read(&SecretPath::Code(c("c2"))).await.map(drop),
        Err(SourceError::Denied)
    );
    assert_eq!(
        h.exchange(&grant, now).await.map(drop),
        Err(SourceError::GrantInvalid),
        "a used grant"
    );
    let revoked = manager.issue_grant(&c("c1"), now).await.unwrap();
    manager.revoke_grant(&revoked).await.unwrap();
    assert_eq!(
        h.exchange(&revoked, now).await.map(drop),
        Err(SourceError::GrantInvalid),
        "a revoked grant"
    );
}

/// E21-9: a vault outage stops no running executor before its lease expires; the next renewal
/// after the outage picks up a rotated token.
pub async fn an_outage_stops_no_executor_before_its_lease_expires<H: Harness>(h: &H) {
    let start = at(1_000_000);
    store_token(h, "c1", "token-1", start).await;
    let executor = h.source(Identity::Executor(c("c1")), start).await;
    let mut keeper = LeaseKeeper::new(executor.lease(&c("c1"), start).await.unwrap());
    let renew = at(1_000_000 + RENEW_SECS);
    assert!(keeper.renewal_due(renew));
    h.outage().await;
    let lease = keeper.lease().unwrap();
    let outcome = executor.renew(lease, renew).await;
    assert!(
        outcome.as_ref().is_err_and(SourceError::is_outage),
        "{outcome:?}"
    );
    keeper.apply(outcome, renew);
    assert_eq!(keeper.secret(renew).map(text).as_deref(), Some("token-1"));
    h.restore().await;
    store_token(h, "c1", "token-2", renew).await;
    let later = at(1_000_000 + 2 * RENEW_SECS);
    let outcome = executor.renew(keeper.lease().unwrap(), later).await;
    keeper.apply(outcome, later);
    assert_eq!(keeper.secret(later).map(text).as_deref(), Some("token-2"));
}

/// A revoked connection's lease ends at the next renewal, outage or not (infrastructure §5.4).
pub async fn a_revoked_connection_ends_its_lease_at_the_next_renewal<H: Harness>(h: &H) {
    let now = at(1_000_000);
    store_token(h, "c1", "token-1", now).await;
    let executor = h.source(Identity::Executor(c("c1")), now).await;
    let mut keeper = LeaseKeeper::new(executor.lease(&c("c1"), now).await.unwrap());
    h.source(Identity::ConnectionManager, now)
        .await
        .delete(&SecretPath::Token(c("c1")))
        .await
        .unwrap();
    let renew = at(1_000_000 + RENEW_SECS);
    let outcome = executor.renew(keeper.lease().unwrap(), renew).await;
    assert!(
        outcome.as_ref().is_err_and(|e| !e.is_outage()),
        "{outcome:?}"
    );
    keeper.apply(outcome, renew);
    assert!(keeper.secret(renew).is_none());
}

/// The canary goes through every call, and no session, lease, grant, or refusal prints it.
pub async fn no_debug_or_refusal_shows_a_secret<H: Harness>(h: &H) {
    let now = at(1_000_000);
    store_token(h, "c1", CANARY, now).await;
    let executor = h.source(Identity::Executor(c("c1")), now).await;
    let lease = executor.lease(&c("c1"), now).await.unwrap();
    assert_eq!(text(&lease.secret), CANARY);
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    let refusals = [
        executor
            .put(&SecretPath::Token(c("c1")), &secret(CANARY))
            .await
            .unwrap_err(),
        h.source(Identity::Runtime, now)
            .await
            .lease(&c("c1"), now)
            .await
            .unwrap_err(),
    ];
    let outputs = [
        format!("{executor:?}"),
        format!("{lease:?}"),
        format!("{grant:?}"),
        format!("{manager:?}"),
        format!("{refusals:?}"),
        refusals.iter().map(ToString::to_string).collect(),
    ];
    for output in outputs {
        assert!(!output.contains(&CANARY[..16]), "{output}");
        assert!(
            !output.contains(grant.secret_id.expose_secret()),
            "a grant's secret id printed: {output}"
        );
    }
}

/// A revoked lease ends the session's leasing: no lease and no renewal after it.
pub async fn after_revoke_the_session_takes_no_lease<H: Harness>(h: &H) {
    let now = at(1_000_000);
    store_token(h, "c1", "token-1", now).await;
    let executor = h.source(Identity::Executor(c("c1")), now).await;
    let lease = executor.lease(&c("c1"), now).await.unwrap();
    let copy = executor.lease(&c("c1"), now).await.unwrap();
    executor.revoke(lease).await.unwrap();
    assert_eq!(
        executor.lease(&c("c1"), now).await.map(drop),
        Err(SourceError::Denied)
    );
    assert!(
        executor
            .renew(&copy, now)
            .await
            .is_err_and(|e| !e.is_outage()),
        "a renewal after revoke is a refusal, which drops the lease"
    );
}

/// The oracle for the platform's own secrets (DEC-2020 item 2, DEC-2277), written out by hand: the
/// model gateway reads the model provider's key, the email sender the email credential, the news
/// reader the news data key, and only the secret import writes any.
pub fn platform_holders(secret: PlatformSecret) -> Holders {
    let reads = match secret {
        PlatformSecret::ModelProviderKey => Identity::ModelGateway,
        PlatformSecret::EmailCredential => Identity::EmailSender,
        PlatformSecret::NewsDataKey => Identity::NewsReader,
    };
    Holders {
        reads,
        writes: Identity::SecretImport,
    }
}

/// Each platform secret's holders are the oracle's. Each backend's run asserts it first, so a
/// run with no OpenBao server still checks the table.
pub fn the_platform_secrets_have_their_holders() {
    for secret in PlatformSecret::ALL {
        assert_eq!(holders(secret), Ok(platform_holders(secret)), "{secret:?}");
    }
}

/// Every session the suite can open, the token exchange's included, for the platform cases.
async fn every_session<H: Harness>(h: &H, now: UtcNanos) -> Vec<H::Source> {
    let manager = h.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    vec![
        h.source(Identity::Api, now).await,
        manager,
        h.exchange(&grant, now).await.unwrap(),
        h.source(Identity::Executor(c("c1")), now).await,
        h.source(Identity::Runtime, now).await,
        h.source(Identity::SecretImport, now).await,
        h.source(Identity::ModelGateway, now).await,
        h.source(Identity::EmailSender, now).await,
    ]
}

/// A platform secret is write-only for the secret import, read by its one process and refused to
/// every other identity, deleted by none, and a rotation is a write over it that its reader picks
/// up at its next read. An outage is an outage, never a refusal (DEC-2020).
pub async fn a_platform_secret_is_written_by_import_and_read_by_its_one_process<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let import = h.source(Identity::SecretImport, now).await;
    let value = |s: PlatformSecret, round: u8| format!("{s:?}-{round}");
    for s in PlatformSecret::ALL {
        import
            .put(&SecretPath::Platform(s), &secret(&value(s, 1)))
            .await
            .unwrap();
    }
    for session in every_session(h, now).await {
        let identity = session.identity().clone();
        for s in PlatformSecret::ALL {
            let path = SecretPath::Platform(s);
            let read = session.read(&path).await;
            if identity == platform_holders(s).reads {
                assert_eq!(
                    text(&read.unwrap().unwrap()),
                    value(s, 1),
                    "{identity:?} {s:?}"
                );
            } else {
                assert_eq!(
                    read.map(|v| v.is_some()),
                    Err(SourceError::Denied),
                    "{identity:?} {s:?}"
                );
            }
            if identity != Identity::SecretImport {
                let wrote = session.put(&path, &secret("forged")).await;
                assert_eq!(wrote, Err(SourceError::Denied), "{identity:?} writes {s:?}");
            }
            let deleted = session.delete(&path).await;
            assert_eq!(
                deleted,
                Err(SourceError::Denied),
                "{identity:?} deletes {s:?}"
            );
        }
    }
    let gateway = h.source(Identity::ModelGateway, now).await;
    let key = SecretPath::Platform(PlatformSecret::ModelProviderKey);
    import
        .put(&key, &secret(&value(PlatformSecret::ModelProviderKey, 2)))
        .await
        .unwrap();
    assert_eq!(
        text(&gateway.read(&key).await.unwrap().unwrap()),
        value(PlatformSecret::ModelProviderKey, 2)
    );
    h.outage().await;
    let cut = gateway.read(&key).await.map(|v| v.is_some());
    h.restore().await;
    assert!(cut.as_ref().is_err_and(SourceError::is_outage), "{cut:?}");
}

/// No `Debug` output, refusal, or error a platform secret's sessions produce shows it.
pub async fn no_debug_or_refusal_shows_a_platform_secret<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let import = h.source(Identity::SecretImport, now).await;
    let mut outputs = vec![format!("{import:?}")];
    for s in PlatformSecret::ALL {
        let path = SecretPath::Platform(s);
        import.put(&path, &secret(CANARY)).await.unwrap();
        let reader = h.source(platform_holders(s).reads, now).await;
        let held = reader.read(&path).await.unwrap().unwrap();
        assert_eq!(text(&held), CANARY);
        let refusals = [
            import.read(&path).await.map(|v| v.is_some()).unwrap_err(),
            reader.put(&path, &secret(CANARY)).await.unwrap_err(),
            h.source(Identity::Runtime, now)
                .await
                .read(&path)
                .await
                .map(|v| v.is_some())
                .unwrap_err(),
        ];
        outputs.push(format!("{reader:?}"));
        outputs.push(format!("{held:?}"));
        outputs.push(format!("{refusals:?}"));
        outputs.extend(refusals.iter().map(ToString::to_string));
    }
    for output in outputs {
        assert!(!output.contains(&CANARY[..16]), "{output}");
    }
}

/// What `session` took of `connection`'s code, as text.
pub async fn taken<S: SecretSource>(
    session: &S,
    connection: &str,
) -> Result<Option<String>, SourceError> {
    session
        .take_code(&c(connection))
        .await
        .map(|code| code.as_ref().map(text))
}

/// DEC-2061: a code is taken once, and only by its own connection's exchange. No other identity,
/// and no other connection's exchange, takes it; a taker finds nothing where no code is, and
/// nothing once it is taken, by take or by read; a new code is taken anew.
pub async fn a_code_is_taken_once_and_only_by_its_own_exchange<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let api = h.source(Identity::Api, now).await;
    let code = SecretPath::Code(c("c1"));
    let taker = h.taker(&c("c1"), now).await;
    assert_eq!(taken(&taker, "c1").await, Ok(None), "no code yet");
    api.put(&code, &secret("code-1")).await.unwrap();
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Executor(c("c1")),
        Identity::Runtime,
    ] {
        let session = h.source(identity.clone(), now).await;
        assert_eq!(
            taken(&session, "c1").await,
            Err(SourceError::Denied),
            "{identity:?}"
        );
    }
    let other = h.taker(&c("c2"), now).await;
    assert_eq!(taken(&other, "c1").await, Err(SourceError::Denied));
    assert_eq!(taken(&taker, "c2").await, Err(SourceError::Denied));
    assert_eq!(taken(&taker, "c1").await, Ok(Some("code-1".to_owned())));
    assert_eq!(taken(&taker, "c1").await, Ok(None), "taken once");
    assert!(
        matches!(taker.read(&code).await, Ok(None)),
        "nothing left to read"
    );
    api.put(&code, &secret("code-2")).await.unwrap();
    assert_eq!(taken(&taker, "c1").await, Ok(Some("code-2".to_owned())));
}

/// DEC-2061: of two takers racing for one code, exactly one gets it, round after round, and
/// nothing is left behind.
pub async fn of_two_racing_takers_exactly_one_gets_the_code<H: Harness>(h: &H) {
    let now = at(1_000_000);
    let api = h.source(Identity::Api, now).await;
    let first = h.taker(&c("c1"), now).await;
    let second = h.taker(&c("c1"), now).await;
    for round in 0..16 {
        let code = format!("code-{round}");
        api.put(&SecretPath::Code(c("c1")), &secret(&code))
            .await
            .unwrap();
        let (one, two) = tokio::join!(taken(&first, "c1"), taken(&second, "c1"));
        let won: Vec<String> = [one, two]
            .into_iter()
            .filter_map(|took| took.unwrap())
            .collect();
        assert_eq!(won, vec![code], "round {round}");
        assert_eq!(taken(&first, "c1").await, Ok(None), "round {round}");
    }
}
