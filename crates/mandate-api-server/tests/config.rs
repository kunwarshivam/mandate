//! The start checks (§3.11 items 1 and 2, DEC-978 items 1 and 2).

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use mandate_api_server::config::{
    MIN_SESSION_KEY_BYTES, SESSION_KEY_FILE, StartRefusal, check_bind, check_database_url,
    check_environment, read_session_key,
};

fn at(ip: IpAddr) -> SocketAddr {
    SocketAddr::new(ip, 8080)
}

const POD: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 42, 0, 7));

#[test]
fn a_loopback_bind_is_accepted_with_or_without_a_pod_address() {
    for ip in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ] {
        assert_eq!(check_bind(at(ip), None), Ok(at(ip)));
        assert_eq!(check_bind(at(ip), Some(POD)), Ok(at(ip)));
    }
}

#[test]
fn the_pods_own_address_is_accepted() {
    assert_eq!(check_bind(at(POD), Some(POD)), Ok(at(POD)));
}

#[test]
fn an_unspecified_bind_is_refused_even_when_named_as_the_pod_address() {
    for ip in [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    ] {
        assert_eq!(check_bind(at(ip), None), Err(StartRefusal::BindNotLocal));
        assert_eq!(
            check_bind(at(ip), Some(ip)),
            Err(StartRefusal::BindNotLocal)
        );
    }
}

#[test]
fn any_other_address_is_refused() {
    let other = IpAddr::V4(Ipv4Addr::new(10, 42, 0, 8));
    assert_eq!(check_bind(at(other), None), Err(StartRefusal::BindNotLocal));
    assert_eq!(
        check_bind(at(other), Some(POD)),
        Err(StartRefusal::BindNotLocal)
    );
    assert_eq!(check_bind(at(POD), None), Err(StartRefusal::BindNotLocal));
    let public = IpAddr::V6("2001:db8::1".parse().expect("an address"));
    assert_eq!(
        check_bind(at(public), Some(POD)),
        Err(StartRefusal::BindNotLocal)
    );
}

#[test]
fn the_deployments_own_variables_pass_the_guard() {
    let vars = [
        ("PATH", "/usr/bin"),
        ("HOME", "/var/lib/owlhead"),
        (
            "CREDENTIALS_DIRECTORY",
            "/run/credentials/owlhead-api.service",
        ),
        (
            "MANDATE_API_DATABASE_URL",
            "postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql",
        ),
        ("MANDATE_API_APP_ORIGIN", "https://app.owlhead.ai"),
        ("MANDATE_API_SUBJECT_LINKS", "/etc/owlhead/subject-links"),
        ("MANDATE_VAULT_DIR", "/var/lib/owlhead/vault"),
        ("ALPACA_OAUTH_CLIENT_ID", "public-client-id"),
    ];
    assert_eq!(check_environment(vars), Ok(()));
    assert_eq!(check_environment(Vec::<(String, String)>::new()), Ok(()));
}

#[test]
fn each_secret_shaped_name_refuses_the_start_whatever_its_case() {
    for name in [
        "MANDATE_ALPACA_PAPER_KEY_ID",
        "MANDATE_ALPACA_PAPER_SECRET",
        "db_password",
        "GITHUB_TOKEN",
        "Session_Key",
        "x_Secret_y",
        "tokenizer",
    ] {
        let vars = [("PATH", "/usr/bin"), (name, "x"), ("OTHER_TOKEN", "y")];
        assert_eq!(
            check_environment(vars),
            Err(StartRefusal::SecretInEnvironment {
                name: name.to_owned()
            }),
            "{name}"
        );
    }
}

#[test]
fn a_refusal_names_the_variable_and_never_its_value() {
    let canary = "canary-7f3a9c-never-printed";
    let refusal = check_environment([("ALPACA_SECRET", canary)])
        .expect_err("a secret-shaped name is refused");
    let shown = format!("{refusal} {refusal:?}");
    assert!(shown.contains("ALPACA_SECRET"), "{shown}");
    assert!(!shown.contains(canary), "{shown}");
}

#[test]
fn a_database_url_with_a_password_is_refused() {
    for url in [
        "postgresql://owlhead_api:hunter2@localhost/owlhead",
        "postgres://owlhead_api:@localhost/owlhead",
        "postgresql://owlhead_api@localhost/owlhead?password=hunter2",
        "postgresql://owlhead_api@localhost/owlhead?host=/run/pg&PASSWORD=hunter2",
        "postgresql://owlhead_api@localhost/owlhead?sslmode=require&password=",
    ] {
        assert_eq!(
            check_database_url(url),
            Err(StartRefusal::PasswordInDatabaseUrl),
            "{url}"
        );
    }
}

#[test]
fn a_database_url_without_a_password_is_accepted() {
    for url in [
        "postgresql://owlhead_api@localhost/owlhead?host=/var/run/postgresql",
        "postgres://owlhead_api@db.cell.local:5432/owlhead?sslmode=verify-full",
        "postgresql://localhost/owlhead",
        "postgresql://owlhead_api@localhost/owlhead?passfile=/run/credentials/pgpass",
    ] {
        assert_eq!(check_database_url(url), Ok(()), "{url}");
    }
}

static DIRS: AtomicU32 = AtomicU32::new(0);

fn credentials(contents: Option<&[u8]>, mode: u32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).expect("a directory");
    if let Some(contents) = contents {
        let file = dir.join(SESSION_KEY_FILE);
        fs::write(&file, contents).expect("the key file");
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).expect("its mode");
    }
    dir
}

#[test]
fn an_owner_only_key_of_32_bytes_is_read() {
    let key = [0x5a_u8; MIN_SESSION_KEY_BYTES];
    let read = read_session_key(&credentials(Some(&key), 0o600)).expect("the key");
    assert_eq!(read.as_bytes(), key.as_slice());
    let longer = [0x11_u8; 64];
    let read = read_session_key(&credentials(Some(&longer), 0o400)).expect("the key");
    assert_eq!(read.as_bytes(), longer.as_slice());
}

#[test]
fn a_key_prints_nothing_of_itself() {
    let key = [0x5a_u8; MIN_SESSION_KEY_BYTES];
    let read = read_session_key(&credentials(Some(&key), 0o600)).expect("the key");
    assert_eq!(format!("{read:?}"), "SessionKey(..)");
}

#[test]
fn a_missing_key_is_refused() {
    assert_eq!(
        read_session_key(&credentials(None, 0o600)),
        Err(StartRefusal::SessionKeyMissing)
    );
}

#[test]
fn a_key_others_can_read_is_refused_before_its_length_is_judged() {
    for mode in [0o640, 0o604, 0o644, 0o660] {
        assert_eq!(
            read_session_key(&credentials(Some(&[1; 64]), mode)),
            Err(StartRefusal::SessionKeyExposed),
            "{mode:o}"
        );
        assert_eq!(
            read_session_key(&credentials(Some(&[1; 4]), mode)),
            Err(StartRefusal::SessionKeyExposed),
            "{mode:o}"
        );
    }
}

#[test]
fn a_short_key_is_refused() {
    assert_eq!(
        read_session_key(&credentials(Some(&[1; MIN_SESSION_KEY_BYTES - 1]), 0o600)),
        Err(StartRefusal::SessionKeyTooShort)
    );
    assert_eq!(
        read_session_key(&credentials(Some(b""), 0o600)),
        Err(StartRefusal::SessionKeyTooShort)
    );
}
