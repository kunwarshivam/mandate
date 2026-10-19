//! The account's credential under the vault's lease (DEC-2306 item 3; DEC-977 item 7): renewed
//! when due, rotated when the vault hands back a new secret, kept through a vault outage until it
//! expires, lost when the vault refuses it or the new secret does not read, and given back on stop.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use mandate_connections::record::ConnectionId;
use mandate_executord::lease::{Held, Leased};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::{Lease, LeaseKeeper, lease_times};
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice};

type Error = Box<dyn std::error::Error>;

/// What the scripted vault was asked.
#[derive(Debug, Default)]
struct Calls {
    renewals: usize,
    revoked: Vec<String>,
}

/// A vault whose renewals answer from a script, one answer each: a secret or a refusal.
struct Script {
    identity: Identity,
    answers: RefCell<VecDeque<Result<&'static str, SourceError>>>,
    calls: Rc<RefCell<Calls>>,
}

impl SecretSource for Script {
    fn identity(&self) -> &Identity {
        &self.identity
    }
    async fn put(&self, _: &SecretPath, _: &SecretSlice<u8>) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn read(&self, _: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        Err(SourceError::Denied)
    }
    async fn delete(&self, _: &SecretPath) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn lease(&self, _: &ConnectionId, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }
    async fn renew(&self, lease: &Lease, now: UtcNanos) -> Result<Lease, SourceError> {
        self.calls.borrow_mut().renewals += 1;
        let answer = self
            .answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(SourceError::Unavailable))?;
        let (renew_at, expires_at) = lease_times(now, None).ok_or(SourceError::Malformed)?;
        Ok(Lease {
            connection: lease.connection.clone(),
            secret: SecretSlice::from(answer.as_bytes().to_vec()),
            expires_at,
            renew_at,
        })
    }
    async fn revoke(&self, lease: Lease) -> Result<(), SourceError> {
        let secret = String::from_utf8_lossy(lease.secret.expose_secret()).into_owned();
        self.calls.borrow_mut().revoked.push(secret);
        Ok(())
    }
    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        Err(SourceError::Denied)
    }
    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
}

/// The instant `hours` after the lease was taken.
fn at(hours: i64) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::from_parts(1_790_000_000 + hours * 3_600, 0)?)
}

/// A secret reads when it starts with `ok`; anything else is malformed.
fn read(secret: &[u8]) -> Result<String, &'static str> {
    String::from_utf8(secret.to_vec())
        .ok()
        .filter(|text| text.starts_with("ok"))
        .ok_or("malformed")
}

type Holder = Leased<Script, fn(&[u8]) -> Result<String, &'static str>>;

/// A lease on `ok-1` taken at hour 0, renewing from hour 1 and expiring at hour 24, over a vault
/// that answers its renewals with `answers`.
fn held(
    answers: Vec<Result<&'static str, SourceError>>,
) -> Result<(Holder, Rc<RefCell<Calls>>), Error> {
    let connection = ConnectionId::new("conn1")?;
    let (renew_at, expires_at) = lease_times(at(0)?, None).ok_or("lease times")?;
    let keeper = LeaseKeeper::new(Lease {
        connection: connection.clone(),
        secret: SecretSlice::from(b"ok-1".to_vec()),
        expires_at,
        renew_at,
    });
    let calls = Rc::new(RefCell::new(Calls::default()));
    let vault = Script {
        identity: Identity::Executor(connection),
        answers: RefCell::new(answers.into()),
        calls: Rc::clone(&calls),
    };
    Ok((Leased::new(vault, keeper, read as fn(&[u8]) -> _), calls))
}

/// Before the lease is due for renewal, nothing is asked and nothing changes.
#[test]
fn a_lease_not_yet_due_is_held_without_asking_the_vault() -> Result<(), Error> {
    let (mut leased, calls) = held(vec![])?;
    assert_eq!(leased.tick(at(0)?)?, Held::Unchanged);
    assert_eq!(
        leased.tick(UtcNanos::from_parts(1_790_003_599, 0)?)?,
        Held::Unchanged
    );
    assert_eq!(calls.borrow().renewals, 0);
    Ok(())
}

/// A renewal that hands back the same secret changes nothing; one that hands back a new secret
/// rotates the credentials to it, once.
#[test]
fn a_renewal_keeps_or_rotates_the_secret() -> Result<(), Error> {
    let (mut leased, calls) = held(vec![Ok("ok-1"), Ok("ok-2")])?;
    assert_eq!(leased.tick(at(1)?)?, Held::Unchanged);
    assert_eq!(leased.tick(at(2)?)?, Held::Rotated("ok-2".to_owned()));
    assert_eq!(
        leased.tick(UtcNanos::from_parts(1_790_007_300, 0)?)?,
        Held::Unchanged
    );
    assert_eq!(calls.borrow().renewals, 2);
    Ok(())
}

/// A vault outage keeps the lease it already holds until that lease expires, renewing at every
/// due pass; at the expiry the credential is lost (E21-9).
#[test]
fn an_outage_keeps_the_lease_until_it_expires() -> Result<(), Error> {
    let (mut leased, calls) = held(vec![
        Err(SourceError::Unavailable),
        Err(SourceError::Sealed),
    ])?;
    for hour in 1..24 {
        assert_eq!(leased.tick(at(hour)?)?, Held::Unchanged, "hour {hour}");
    }
    assert!(calls.borrow().renewals >= 2);
    assert_eq!(leased.tick(at(24)?)?, Held::Lost);
    assert_eq!(leased.tick(at(25)?)?, Held::Lost);
    Ok(())
}

/// A renewal the vault refuses, or a new secret that does not read, loses the credential at
/// once and for good: only a new lease, on restart, brings one back.
#[test]
fn a_refused_renewal_or_an_unreadable_secret_loses_the_credential() -> Result<(), Error> {
    let (mut refused, _) = held(vec![Err(SourceError::Denied), Ok("ok-2")])?;
    assert_eq!(refused.tick(at(1)?)?, Held::Lost);
    assert_eq!(refused.tick(at(2)?)?, Held::Lost);
    let (mut unreadable, _) = held(vec![Ok("bad-2"), Ok("ok-3")])?;
    assert_eq!(unreadable.tick(at(1)?)?, Held::Lost);
    assert_eq!(unreadable.tick(at(2)?)?, Held::Lost);
    Ok(())
}

/// On stop the held lease is given back to the vault; with none held, nothing is asked.
#[test]
fn stopping_gives_the_lease_back() -> Result<(), Error> {
    let (leased, calls) = held(vec![])?;
    leased.revoke()?;
    assert_eq!(calls.borrow().revoked, ["ok-1"]);
    let (mut lost, calls) = held(vec![Err(SourceError::Denied)])?;
    assert_eq!(lost.tick(at(1)?)?, Held::Lost);
    lost.revoke()?;
    assert!(calls.borrow().revoked.is_empty());
    Ok(())
}

/// A renewal whose new secret does not read loses the credential, and the lease the vault
/// renewed is still given back on stop rather than left to expire.
#[test]
fn an_unreadable_rotation_still_gives_the_lease_back() -> Result<(), Error> {
    let (mut leased, calls) = held(vec![Ok("bad-2")])?;
    assert_eq!(leased.tick(at(1)?)?, Held::Lost);
    leased.revoke()?;
    assert_eq!(calls.borrow().revoked, ["bad-2"]);
    Ok(())
}

/// `Held`'s `Debug` names the variant and never the credentials a rotation carries.
#[test]
fn held_never_prints_its_credentials() {
    let rotated: Held<String> = Held::Rotated("ok-sentinel-credential".to_owned());
    let shown = format!("{rotated:?}");
    assert!(!shown.contains("ok-sentinel-credential"), "{shown}");
    assert_eq!(shown, "Rotated(<redacted>)");
    assert_eq!(format!("{:?}", Held::<String>::Lost), "Lost");
    assert_eq!(format!("{:?}", Held::<String>::Unchanged), "Unchanged");
}
