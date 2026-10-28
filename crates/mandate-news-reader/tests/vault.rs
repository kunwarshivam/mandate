//! The news reader's vault login (E2-10, DEC-2277 item 3, E2-22): it logs in by AppRole as the news
//! reader with the role and secret ids mounted beside the vault's CA, never by Kubernetes auth,
//! which the vault offers only a connection's executor and token exchange. Without the news
//! reader's own mounted credentials it reaches no vault at all, and every failure is
//! `vault_unavailable`. These tests reach no further than the login: the one read and the end of
//! the session after it are `tests/key.rs`'s, through `read_then_end`. The cell's vault drill
//! checks only the news reader's policy (`xtask/src/cell.rs`), not this login.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "the tests set up a scratch directory and a local listener"
)]

use std::fs;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mandate_news_reader::{KeyRefusal, vault_key};
use mandate_time::UtcNanos;

const CA: &str = include_str!("fixtures/vault-ca.pem");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-news-reader-vault-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Mounts `role`'s AppRole ids, owner-only as the vault's mounted-file rule asks, beside the CA.
fn mount(dir: &Path, role: &str) {
    fs::write(dir.join("ca.crt"), CA).unwrap();
    for (suffix, value) in [
        ("role-id", "role-id-test\n"),
        ("secret-id", "secret-id-test\n"),
    ] {
        let file = dir.join(format!("{role}.{suffix}"));
        fs::write(&file, value).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn key(listener: &TcpListener, dir: &Path) -> Result<(), KeyRefusal> {
    let address = format!("https://{}", listener.local_addr().unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(vault_key(&address, dir, UtcNanos::EPOCH))
        .map(|_| ())
}

#[test]
fn it_logs_in_with_the_news_reader_s_own_mounted_approle_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let vault = listener.try_clone().unwrap();
    let (hung_up, logged_in) = mpsc::channel();
    thread::spawn(move || {
        drop(vault.accept().unwrap());
        hung_up.send(()).unwrap();
    });
    let dir = scratch("own");
    mount(&dir, "news-reader");
    assert_eq!(
        key(&listener, &dir),
        Err(KeyRefusal::VaultUnavailable),
        "a vault that hangs up on the login is an outage"
    );
    assert_eq!(
        logged_in.recv_timeout(Duration::from_secs(10)),
        Ok(()),
        "with its own credentials it logs in at the address"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn without_its_own_credentials_or_the_ca_it_reaches_no_vault() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let empty = scratch("empty");
    let other = scratch("other");
    mount(&other, "bars-reader");
    let no_ca = scratch("no-ca");
    mount(&no_ca, "news-reader");
    fs::remove_file(no_ca.join("ca.crt")).unwrap();
    let no_secret = scratch("no-secret");
    mount(&no_secret, "news-reader");
    fs::remove_file(no_secret.join("news-reader.secret-id")).unwrap();
    for dir in [&empty, &other, &no_ca, &no_secret] {
        assert_eq!(
            key(&listener, dir),
            Err(KeyRefusal::VaultUnavailable),
            "{}",
            dir.display()
        );
        fs::remove_dir_all(dir).ok();
    }
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().map(|_| ()).map_err(|e| e.kind()),
        Err(ErrorKind::WouldBlock),
        "no login without the news reader's own credentials and the CA"
    );
}
