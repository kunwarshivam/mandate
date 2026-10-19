//! The executor's start on Alpaca's paper host (DEC-2306 items 1 to 3; DEC-2022): the flags that
//! name the run and its vault, the CA and AppRole credentials read from the mounted Secret before
//! anything is dialled, the lease on the checked connection and the broker over its token, and
//! what each lease tick means for the run. The vault here is scripted and the mount a temporary
//! directory, so nothing dials a host (DEC-2306 item 5).

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mandate_connections::record::ConnectionId;
use mandate_connections::vault::AccessToken;
use mandate_executord::Broker;
use mandate_executord::credential::{
    RunLease, VaultMount, lease_verdict, paper_broker, paper_settings, vault_config, vault_login,
};
use mandate_executord::lease::Held;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::{Lease, lease_times};
use mandate_vault::openbao::OpenBaoConfig;
use mandate_vault::policy::{Identity, Mounts, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice, SecretString};

type Error = Box<dyn std::error::Error>;

const TOKEN: &str = "executorStartSentinelToken-0123456789";
const CA: &[u8] = b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";

/// DEC-2624's token entry for `token`, written out here rather than by its encoder.
fn entry(token: &str) -> Vec<u8> {
    format!(
        r#"{{"access_token":"{token}","format":1,"scope":["data","trading"],"token_type":"bearer"}}"#
    )
    .into_bytes()
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-10-19T15:00:00Z")?)
}

/// `hours` after [`now`].
fn later(hours: i64) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::from_parts(now()?.secs() + hours * 3_600, 0)?)
}

/// What the scripted vault was asked.
#[derive(Debug, Default)]
struct Calls {
    leased: Vec<String>,
    renewals: usize,
    revoked: Vec<Vec<u8>>,
}

/// A vault whose one lease answers `secret`, or refuses.
struct Script {
    identity: Identity,
    secret: Result<Vec<u8>, SourceError>,
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
    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError> {
        self.calls
            .borrow_mut()
            .leased
            .push(connection.as_str().to_owned());
        let secret = self.secret.clone()?;
        let (renew_at, expires_at) = lease_times(now, None).ok_or(SourceError::Malformed)?;
        Ok(Lease {
            connection: connection.clone(),
            secret: SecretSlice::from(secret),
            expires_at,
            renew_at,
        })
    }
    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        self.calls.borrow_mut().renewals += 1;
        Err(SourceError::Unavailable)
    }
    async fn revoke(&self, lease: Lease) -> Result<(), SourceError> {
        let secret = lease.secret.expose_secret().to_vec();
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

fn script(secret: Result<Vec<u8>, SourceError>) -> Result<(Script, Rc<RefCell<Calls>>), Error> {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let vault = Script {
        identity: Identity::Executor(ConnectionId::new("conn1")?),
        secret,
        calls: Rc::clone(&calls),
    };
    Ok((vault, calls))
}

/// A fresh, empty directory standing for the mounted Secret.
fn mount_dir() -> Result<PathBuf, Error> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mandate-executord-start-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _absent = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Writes `name` in `dir` with `mode`, as the kubelet would.
fn put(dir: &Path, name: &str, bytes: &[u8], mode: u32) -> Result<(), Error> {
    let path = dir.join(name);
    fs::write(&path, bytes)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// The flags of a run: `pairs` of flag and value.
fn flags(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(f, v)| ((*f).to_owned(), (*v).to_owned()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(f, _)| f == name)
            .map(|(_, v)| v.clone())
    }
}

const ADDRESS: &str = "https://vault.ws-ws1.svc:8200";

/// DEC-2306 item 1: no broker flag and `fixture` need no vault; `alpaca-paper` needs both the
/// vault's address and its mount, and never runs over the in-memory journal.
#[test]
#[ignore = "pending E21-30"]
fn the_flags_name_the_paper_run_and_its_vault() -> Result<(), Error> {
    assert_eq!(paper_settings(&flags(&[]), false)?, None);
    assert_eq!(
        paper_settings(&flags(&[("--broker", "fixture")]), true)?,
        None
    );
    let paper = [
        ("--broker", "alpaca-paper"),
        ("--vault-address", ADDRESS),
        ("--vault-dir", "/run/mandate/vault"),
    ];
    assert_eq!(
        paper_settings(&flags(&paper), false)?,
        Some(VaultMount {
            address: ADDRESS.to_owned(),
            dir: PathBuf::from("/run/mandate/vault"),
        })
    );
    let refused = |pairs: &[(&str, &str)], fixture: bool| paper_settings(&flags(pairs), fixture);
    assert_eq!(refused(&paper, true), Err("broker_conflict"));
    assert_eq!(
        refused(&[("--broker", "alpaca-live")], false),
        Err("broker_unknown")
    );
    assert_eq!(
        refused(&[paper[0], paper[2]], false),
        Err("vault_address_missing")
    );
    assert_eq!(
        refused(&[paper[0], paper[1]], false),
        Err("vault_dir_missing")
    );
    Ok(())
}

/// The vault's CA is the mount's `ca.crt`, read whole; a missing CA, or one group or others may
/// read, is refused.
#[test]
#[ignore = "pending E21-30"]
fn the_vault_ca_is_read_from_the_mount() -> Result<(), Error> {
    let dir = mount_dir()?;
    let mount = VaultMount {
        address: ADDRESS.to_owned(),
        dir: dir.clone(),
    };
    assert_eq!(vault_config(&mount).err(), Some("vault_ca_unreadable"));
    put(&dir, "ca.crt", CA, 0o644)?;
    assert_eq!(vault_config(&mount).err(), Some("vault_ca_unreadable"));
    put(&dir, "ca.crt", CA, 0o400)?;
    let config = vault_config(&mount)?;
    assert_eq!(config.address, ADDRESS);
    assert_eq!(config.ca_pem, CA);
    assert_eq!(config.mounts, Mounts::default());
    Ok(())
}

/// The login reads `connection`'s mounted AppRole credentials before it dials: none, or only
/// another connection's, is refused without a dial, and a vault it cannot reach is
/// `vault_login_failed`.
#[test]
#[ignore = "pending E21-30"]
fn the_login_reads_its_mounted_credentials_before_dialling() -> Result<(), Error> {
    let dir = mount_dir()?;
    let config = OpenBaoConfig {
        address: "http://vault.invalid:8200".to_owned(),
        ca_pem: CA.to_vec(),
        mounts: Mounts::default(),
    };
    let conn1 = ConnectionId::new("conn1")?;
    let at = now()?;
    let login = |dir: &Path| vault_login(&config, dir, &conn1, at).err();
    assert_eq!(login(&dir), Some("vault_credentials_unreadable"));
    put(&dir, "executor-conn2.role-id", b"role-2\n", 0o400)?;
    put(&dir, "executor-conn2.secret-id", b"secret-2\n", 0o400)?;
    assert_eq!(login(&dir), Some("vault_credentials_unreadable"));
    put(&dir, "executor-conn1.role-id", b"role-1\n", 0o400)?;
    put(&dir, "executor-conn1.secret-id", b"secret-1\n", 0o400)?;
    assert_eq!(login(&dir), Some("vault_login_failed"));
    Ok(())
}

/// DEC-2306 items 2 and 3: the lease is taken on the checked connection, and the broker runs
/// over its token, with nothing given back while the run holds it.
#[test]
#[ignore = "pending E21-30"]
fn the_broker_runs_over_the_leased_token() -> Result<(), Error> {
    let (vault, calls) = script(Ok(entry(TOKEN)))?;
    let (broker, _leased) = paper_broker(vault, &ConnectionId::new("conn1")?, now()?)?;
    broker.profile()?;
    assert_eq!(calls.borrow().leased, ["conn1"]);
    assert!(calls.borrow().revoked.is_empty());
    Ok(())
}

/// A refused lease refuses the start; a lease whose secret is not a token entry, a key pair
/// among them, is given back to the vault, then refuses the start.
#[test]
#[ignore = "pending E21-30"]
fn a_refused_or_undecodable_lease_refuses_the_start() -> Result<(), Error> {
    let conn1 = ConnectionId::new("conn1")?;
    let (refused, calls) = script(Err(SourceError::Denied))?;
    assert_eq!(
        paper_broker(refused, &conn1, now()?).err(),
        Some("credential_unavailable")
    );
    assert!(calls.borrow().revoked.is_empty());
    let key_pair = br#"{"key_id":"PK1","secret":"s"}"#.to_vec();
    let (undecodable, calls) = script(Ok(key_pair.clone()))?;
    assert_eq!(
        paper_broker(undecodable, &conn1, now()?).err(),
        Some("credential_malformed")
    );
    assert_eq!(calls.borrow().revoked, [key_pair]);
    Ok(())
}

/// DEC-2306 item 3: only an unchanged credential keeps the run going; a rotated one, a lost one,
/// and a tick that failed each stop it by their own code.
#[test]
#[ignore = "pending E21-30"]
fn only_an_unchanged_credential_keeps_the_run_going() {
    assert_eq!(lease_verdict(Ok(Held::Unchanged)), Ok(()));
    let rotated = Held::Rotated(AccessToken(SecretString::from(TOKEN)));
    assert_eq!(lease_verdict(Ok(rotated)), Err("credential_rotated"));
    assert_eq!(lease_verdict(Ok(Held::Lost)), Err("credential_lost"));
    assert_eq!(
        lease_verdict(Err("runtime_unavailable")),
        Err("runtime_unavailable")
    );
}

/// The run goes on while its lease holds, through a vault outage; the lease's expiry stops it,
/// and every later pass gives the same code without asking the vault again.
#[test]
#[ignore = "pending E21-30"]
fn the_run_lease_stops_the_run_once_and_for_good() -> Result<(), Error> {
    let (vault, calls) = script(Ok(entry(TOKEN)))?;
    let (_broker, mut lease) = paper_broker(vault, &ConnectionId::new("conn1")?, now()?)?;
    assert_eq!(lease.pass(now()?), None);
    assert_eq!(calls.borrow().renewals, 0);
    assert_eq!(lease.pass(later(2)?), None, "the outage keeps the lease");
    assert!(calls.borrow().renewals > 0);
    assert_eq!(lease.pass(later(25)?), Some("credential_lost"));
    let asked = calls.borrow().renewals;
    assert_eq!(lease.pass(later(26)?), Some("credential_lost"));
    assert_eq!(lease.pass(later(27)?), Some("credential_lost"));
    assert_eq!(
        calls.borrow().renewals,
        asked,
        "a stopped run asks nothing more"
    );
    Ok(())
}

/// A stopped run gives its lease back.
#[test]
#[ignore = "pending E21-30"]
fn a_stopped_run_gives_its_lease_back() -> Result<(), Error> {
    let (vault, calls) = script(Ok(entry(TOKEN)))?;
    let (_broker, lease): (_, RunLease<Script>) =
        paper_broker(vault, &ConnectionId::new("conn1")?, now()?)?;
    lease.give_back()?;
    assert_eq!(calls.borrow().revoked, [entry(TOKEN)]);
    Ok(())
}
