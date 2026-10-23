//! The process's wiring around the provisioner (E21-33, DEC-2350): how a pass moves the
//! lifecycle, when the vault session is renewed, and what the vault directory must hold.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(debug_assertions)]
use mandate_connections::record::ConnectionId;
use mandate_process::{Input, Kind, Lifecycle, State};
use mandate_provisioner::driver::Halt;
#[cfg(debug_assertions)]
use mandate_provisionerd::fixture::FixtureVault;
use mandate_provisionerd::{
    EXPIRY_MARGIN_SECS, RELOGIN_RETRY_SECS, RELOGIN_SECS, Renewal, StartRefused, VAULT_CA,
    fixture_allowed, input, vault_config, vault_login,
};
use mandate_time::UtcNanos;
use mandate_vault::lease::LEASE_SECS;
#[cfg(debug_assertions)]
use mandate_vault::provisioner::RoleProvisioner;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-provisionerd-{name}-{}",
        std::process::id()
    ));
    let _fresh = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A file as the vault directory's mount leaves it: readable by its owner alone.
fn mounted(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn a_pass_that_could_not_finish_is_a_heartbeat_and_a_refused_record_stops_the_process() {
    assert_eq!(input(&Ok(())), Input::Heartbeat);
    for halt in [Halt::Journal, Halt::NotOpened] {
        assert_eq!(input(&Err(halt)), Input::Heartbeat);
    }
    assert_eq!(input(&Err(Halt::SessionExpired)), Input::Failed);
    let refused = Halt::Refused {
        event_type: "ConnectionRolesMade",
    };
    assert_eq!(input(&Err(refused.clone())), Input::Failed);
    let at = UtcNanos::EPOCH;
    let mut lifecycle = Lifecycle::new(Kind::Service, Duration::from_secs(30), at);
    lifecycle.observe(at, Input::StartupPassed);
    lifecycle.observe(at, input(&Err(Halt::Journal)));
    assert_eq!(lifecycle.state(), State::Ready);
    lifecycle.observe(at, input(&Err(refused)));
    assert!(matches!(lifecycle.state(), State::Exited(_)));
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// A session is renewed after an hour, inside its day-long token; a refused login is tried again
/// a minute later, not once a pass.
#[test]
fn the_session_is_renewed_hourly_and_a_refused_login_a_minute_later() {
    assert_eq!(RELOGIN_SECS, 3_600);
    assert_eq!(RELOGIN_RETRY_SECS, 60);
    let mut renewal = Renewal::new(at(1_000));
    assert!(!renewal.due(at(4_599)));
    assert!(renewal.due(at(4_600)));
    renewal.refused(at(4_600));
    assert!(!renewal.due(at(4_659)));
    assert!(renewal.due(at(4_660)));
    assert_eq!(Renewal::new(at(4_660)), Renewal::new(at(4_660)));
}

/// A session is used until five minutes before its token's day ends, and never after, however
/// many logins were refused in between: the process then makes no role with it.
#[test]
fn a_session_near_its_tokens_end_is_never_used() {
    assert_eq!(EXPIRY_MARGIN_SECS, 300);
    let mut renewal = Renewal::new(at(1_000));
    let end = 1_000 + LEASE_SECS - EXPIRY_MARGIN_SECS;
    for refused in [4_600, 50_000, end - 1] {
        renewal.refused(at(refused));
        assert!(renewal.usable(at(refused)));
    }
    assert!(renewal.usable(at(end - 1)));
    assert!(!renewal.usable(at(end)));
    assert!(!renewal.usable(at(end + LEASE_SECS)));
}

/// A release build holds no fixture: asking for it is refused, and a run without it starts in
/// either build.
#[test]
fn a_release_build_refuses_the_fixture() {
    assert_eq!(
        fixture_allowed(true, false),
        Err(StartRefused::FixtureNotBuilt)
    );
    assert_eq!(StartRefused::FixtureNotBuilt.code(), "fixture_not_built");
    assert_eq!(fixture_allowed(true, true), Ok(true));
    assert_eq!(fixture_allowed(false, false), Ok(false));
    assert_eq!(fixture_allowed(false, true), Ok(false));
}

/// The vault directory must hold the CA, and the provisioner's own AppRole files; without them
/// the start is refused before the vault is dialled, with a code and no content.
#[tokio::test]
async fn a_vault_directory_without_the_ca_or_the_credentials_refuses_the_start() {
    let empty = dir("empty");
    assert_eq!(
        vault_config("https://127.0.0.1:1", &empty).err(),
        Some("vault_ca_unreadable")
    );
    let with_ca = dir("ca");
    mounted(&with_ca, VAULT_CA, "not a certificate");
    let config = vault_config("https://127.0.0.1:1", &with_ca).unwrap();
    assert_eq!(config.address, "https://127.0.0.1:1");
    assert_eq!(config.ca_pem, b"not a certificate");
    assert_eq!(
        vault_login(&config, &with_ca, UtcNanos::EPOCH).await.err(),
        Some("vault_credentials_unreadable")
    );
    mounted(&with_ca, "provisioner.role-id", "role\n");
    mounted(&with_ca, "provisioner.secret-id", "secret\n");
    assert_eq!(
        vault_login(&config, &with_ca, UtcNanos::EPOCH).await.err(),
        Some("vault_login_failed")
    );
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn the_fixture_vault_makes_and_removes_every_role() {
    let connection = ConnectionId::new("c1").unwrap();
    assert_eq!(FixtureVault.make_roles(&connection, "ws").await, Ok(()));
    assert_eq!(
        FixtureVault.remove_exchange_role(&connection, "ws").await,
        Ok(())
    );
}
