//! The start checks (§3.11 items 1 and 2, DEC-978 items 1 and 2).

use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use mandate_api_server::config::{
    COOL_OFF_WAIVER, COOL_OFF_WAIVER_UNTIL, MIN_SESSION_KEY_BYTES, SESSION_KEY_FILE,
    STAGING_WAIVER_CEILING, StartRefusal, authorize_page, check_bind, check_database_url,
    check_environment, check_key_set_tier, cool_off_waiver, read_session_key,
};
use mandate_api_server::methods::CoolOffWaiver;
use mandate_connections::start::AuthorizePage;
use mandate_release::startup::Tier;
use mandate_time::UtcNanos;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

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
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
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

#[test]
fn an_ipv4_mapped_address_is_judged_as_the_ipv4_address_it_maps() {
    let mapped = |text: &str| IpAddr::V6(text.parse().expect("an address"));
    assert_eq!(
        check_bind(at(mapped("::ffff:127.0.0.1")), None),
        Ok(at(mapped("::ffff:127.0.0.1")))
    );
    let all = mapped("::ffff:0.0.0.0");
    assert_eq!(
        check_bind(at(all), Some(all)),
        Err(StartRefusal::BindNotLocal)
    );
    assert_eq!(
        check_bind(at(all), Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED))),
        Err(StartRefusal::BindNotLocal)
    );
    let pod = mapped("::ffff:10.42.0.7");
    assert_eq!(check_bind(at(pod), Some(POD)), Ok(at(pod)));
    assert_eq!(check_bind(at(POD), Some(pod)), Ok(at(POD)));
}

#[test]
fn a_password_hidden_by_encoding_or_by_the_keyword_form_is_refused() {
    for url in [
        "postgresql://owlhead_api@localhost/owlhead?%70assword=hunter2",
        "postgresql://owlhead_api@localhost/owlhead?PassWord%3Dhunter2",
        "postgresql://owlhead_api%3Ahunter2@localhost/owlhead",
        "POSTGRESQL://owlhead_api:hunter2@localhost/owlhead",
        "host=/var/run/postgresql user=owlhead_api password=hunter2",
        "host=/var/run/postgresql password = 'hunter2' dbname=owlhead",
        "dbname=owlhead\tPASSWORD=hunter2",
    ] {
        assert_eq!(
            check_database_url(url),
            Err(StartRefusal::PasswordInDatabaseUrl),
            "{url}"
        );
    }
    assert_eq!(
        check_database_url("host=/var/run/postgresql user=owlhead_api dbname=owlhead"),
        Ok(())
    );
    assert_eq!(
        check_database_url("postgresql://owlhead_api@localhost/owlhead?application_name=a%2"),
        Ok(())
    );
}

#[test]
fn a_key_that_is_not_a_regular_file_is_refused_as_missing_and_never_opened() {
    let dir = credentials(None, 0o600);
    fs::create_dir(dir.join(SESSION_KEY_FILE)).expect("a directory where the key should be");
    assert_eq!(read_session_key(&dir), Err(StartRefusal::SessionKeyMissing));
    let dir = credentials(None, 0o600);
    let made = std::process::Command::new("mkfifo")
        .arg(dir.join(SESSION_KEY_FILE))
        .status()
        .expect("mkfifo runs");
    assert!(made.success());
    fs::set_permissions(
        dir.join(SESSION_KEY_FILE),
        fs::Permissions::from_mode(0o600),
    )
    .expect("its mode");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_session_key(&dir));
    });
    let read = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("a FIFO is refused without being opened, so the read never blocks");
    assert_eq!(read, Err(StartRefusal::SessionKeyMissing));
}

/// The content-addressed store the agent reads take documents from (DEC-1020 item 1): optional,
/// opened at start, and refused by name when it cannot be opened.
#[test]
fn the_artifact_store_is_optional_and_refused_by_name_when_unusable() {
    use mandate_api_server::config::artifact_store;
    use mandate_journal::{ArtifactRef, ArtifactStore, get_artifact};

    assert!(artifact_store(None).expect("no store").is_none());
    assert!(
        artifact_store(Some(std::ffi::OsStr::new("")))
            .expect("an empty setting is no store")
            .is_none()
    );

    let root = credentials(None, 0o700);
    let mut writer = mandate_artifacts_fs::FsArtifactStore::open(&root).expect("a store");
    let reference = writer.put_artifact(b"a document").expect("stored");
    let store = artifact_store(Some(root.as_os_str()))
        .expect("opened")
        .expect("a store");
    assert_eq!(
        get_artifact(store.as_ref(), &reference).expect("read"),
        b"a document"
    );
    assert_eq!(reference, ArtifactRef::of(b"a document"));

    let bare = credentials(None, 0o700);
    assert_eq!(
        artifact_store(Some(bare.as_os_str())).err(),
        Some(StartRefusal::Setting {
            name: "MANDATE_API_ARTIFACTS"
        }),
        "a directory that is not a store is refused"
    );
    assert!(
        !bare.join("sha256").exists(),
        "and nothing is written to it"
    );

    let file = root.join("a-file");
    fs::write(&file, b"not a directory").expect("a file");
    let refused = artifact_store(Some(file.join("store").as_os_str())).expect_err("refused");
    assert_eq!(
        refused,
        StartRefusal::Setting {
            name: "MANDATE_API_ARTIFACTS"
        }
    );
}

/// E21-53 (DEC-2100 item 9): outside dev and CI, a key set fetched over `http` is refused, so a
/// staging or production server never trusts a loopback issuer, whatever its settings name.
#[test]
fn an_http_key_set_starts_only_in_dev_and_ci() {
    let refused = Err(StartRefusal::Setting {
        name: "MANDATE_API_JWKS_URL",
    });
    let http = [
        "http://127.0.0.1:8090/jwks.json",
        "http://[::1]:8090/jwks.json",
        "HTTP://127.0.0.1:8090/jwks.json",
        "http://issuer.example/.well-known/jwks.json",
    ];
    for url in http {
        for tier in [Tier::Dev, Tier::Ci] {
            assert_eq!(check_key_set_tier(url, tier), Ok(()), "{url} in {tier:?}");
        }
        for tier in [Tier::Staging, Tier::Production] {
            assert_eq!(check_key_set_tier(url, tier), refused, "{url} in {tier:?}");
        }
    }
    let https = [
        "https://issuer.example/auth/v1/.well-known/jwks.json",
        "HTTPS://issuer.example/jwks.json",
    ];
    for url in https {
        for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
            assert_eq!(check_key_set_tier(url, tier), Ok(()), "{url} in {tier:?}");
        }
    }
    for url in [
        "",
        "ftp://issuer.example/jwks.json",
        "httpsx://issuer.example/jwks.json",
    ] {
        assert_eq!(
            check_key_set_tier(url, Tier::Production),
            refused,
            "{url:?} is not https, so production refuses it"
        );
    }
}

#[test]
fn no_tier_or_an_unknown_one_is_production_for_the_key_set() {
    for setting in [
        None,
        Some(""),
        Some("Dev"),
        Some("development"),
        Some("prod"),
    ] {
        assert_eq!(
            check_key_set_tier(
                "http://127.0.0.1:8090/jwks.json",
                Tier::from_setting(setting)
            ),
            Err(StartRefusal::Setting {
                name: "MANDATE_API_JWKS_URL"
            }),
            "{setting:?}"
        );
    }
}

const CELL_PAGE: &str = "http://oauth-fixture:8091/alpaca/oauth/authorize";

/// Unset, the page is Alpaca's in every tier (DEC-2633 item 3).
#[test]
fn without_an_authorize_url_the_page_is_alpaca_s() {
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        assert_eq!(
            authorize_page(tier, None),
            Ok(AuthorizePage::alpaca()),
            "{tier:?}"
        );
    }
}

/// A dev or CI cell may name its fixture provider's page, kept as given.
#[test]
#[cfg(debug_assertions)]
fn a_dev_or_ci_cell_names_its_fixture_page() {
    for tier in [Tier::Dev, Tier::Ci] {
        assert_eq!(
            authorize_page(tier, Some(OsStr::new(CELL_PAGE))),
            Ok(AuthorizePage::for_cell(CELL_PAGE).unwrap()),
            "{tier:?}"
        );
    }
}

/// Outside dev and CI any set value refuses the start, Alpaca's own page and an empty one
/// included: the variable being set at all is the refusal (DEC-2633 items 2 and 3).
#[test]
fn outside_dev_and_ci_any_authorize_url_refuses_the_start() {
    for tier in [Tier::Staging, Tier::Production] {
        for value in [CELL_PAGE, "", "https://app.alpaca.markets/oauth/authorize"] {
            assert_eq!(
                authorize_page(tier, Some(OsStr::new(value))),
                Err(StartRefusal::AuthorizeUrl),
                "{tier:?} {value:?}"
            );
        }
    }
}

/// In dev and CI a page the cell check refuses, or one that is not UTF-8, refuses the start.
#[test]
#[cfg(debug_assertions)]
fn a_page_the_cell_check_refuses_refuses_the_start_in_dev() {
    for value in [
        OsStr::new(""),
        OsStr::new("https://app.alpaca.markets/oauth/authorize"),
        OsStr::new("http://evil.example/a"),
        OsStr::from_bytes(b"http://127.0.0.1/\xff"),
    ] {
        assert_eq!(
            authorize_page(Tier::Dev, Some(value)),
            Err(StartRefusal::AuthorizeUrl),
            "{value:?}"
        );
    }
}

/// The refusal names the setting and nothing it read.
#[test]
fn the_authorize_url_refusal_names_the_setting_alone() {
    let text = StartRefusal::AuthorizeUrl.to_string();
    assert!(text.contains("MANDATE_ALPACA_AUTHORIZE_URL"), "{text}");
    assert!(
        !text.contains("oauth-fixture") && !text.contains("http"),
        "{text}"
    );
}

/// A build without debug assertions refuses any set page in every tier, dev and ci included,
/// whatever it names (DEC-2633 item 3, DEC-2338 item 5). `cargo xtask ci api-release` runs this
/// file with `--release`, so a tier-dependent loosening of the release arm fails here even where
/// the release binary meets `release_unsigned` first.
#[test]
#[cfg(not(debug_assertions))]
fn a_release_build_refuses_any_set_page_in_every_tier() {
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        for value in [
            OsStr::new(CELL_PAGE),
            OsStr::new("http://127.0.0.1:8091/alpaca/oauth/authorize"),
            OsStr::new("https://app.alpaca.markets/oauth/authorize"),
            OsStr::new(""),
            OsStr::from_bytes(b"http://127.0.0.1/\xff"),
        ] {
            assert_eq!(
                authorize_page(tier, Some(value)),
                Err(StartRefusal::AuthorizeUrl),
                "{tier:?} {value:?}"
            );
        }
    }
}

/// The waiver's setting, `true`.
fn on() -> Option<&'static OsStr> {
    Some(OsStr::new("true"))
}

/// An expiry before [`STAGING_WAIVER_CEILING`].
const BEFORE_CEILING: &str = "2026-11-01T12:00:00Z";

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).expect("a timestamp")
}

/// DEC-2146 item 2: the waiver is exactly `true`, and a `dev` or `ci` tier starts with it without
/// an expiry, each waiving as its own tier (DEC-2150 item 2 leaves them unchanged). Unset is no
/// waiver, in every tier.
#[test]
fn a_waiver_of_true_starts_in_dev_and_ci_without_an_expiry_and_unset_waives_nothing() {
    assert_eq!(
        cool_off_waiver(Tier::Dev, on(), None),
        Ok(Some((CoolOffWaiver::Dev, None)))
    );
    assert_eq!(
        cool_off_waiver(Tier::Ci, on(), None),
        Ok(Some((CoolOffWaiver::Ci, None)))
    );
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        assert_eq!(
            cool_off_waiver(tier, None, None),
            Ok(None),
            "{tier:?} unset"
        );
    }
}

/// DEC-2146 item 2 and DEC-2150 item 1: production, and a tier unset or unknown (which reads as
/// production), refuse the waiver at start, with or without an expiry, naming the waiver alone, so
/// a misconfigured cell fails closed.
#[test]
fn a_waiver_in_production_refuses_the_start_with_or_without_an_expiry() {
    let refused = Err(StartRefusal::Setting {
        name: COOL_OFF_WAIVER,
    });
    for setting in [Some("production"), None, Some("qa"), Some("CI")] {
        let tier = Tier::from_setting(setting);
        for until in [None, Some(OsStr::new(BEFORE_CEILING))] {
            assert_eq!(
                cool_off_waiver(tier, on(), until),
                refused,
                "tier setting {setting:?}, expiry {until:?}"
            );
        }
    }
}

/// DEC-2150 item 3: the ceiling compiled into the server is 2026-11-05T00:00:00Z, in the form the
/// expiry takes.
#[test]
fn the_staging_ceiling_is_the_fifth_of_november() {
    assert_eq!(STAGING_WAIVER_CEILING, "2026-11-05T00:00:00Z");
    assert_eq!(
        instant(STAGING_WAIVER_CEILING),
        UtcNanos::from_parts(1_793_836_800, 0).expect("an instant")
    );
}

/// DEC-2150 items 1 to 3: a staging cell starts with the waiver and an expiry no later than the
/// ceiling, the ceiling itself included, and waives as `staging` until it. An expiry already past
/// still starts: the verifier then counts no waiver (item 4), which fails closed.
#[test]
#[ignore = "pending E9-4"]
fn a_staging_waiver_starts_with_an_expiry_no_later_than_the_ceiling() {
    for until in [
        STAGING_WAIVER_CEILING,
        BEFORE_CEILING,
        "2026-10-01T00:00:00Z",
    ] {
        assert_eq!(
            cool_off_waiver(Tier::Staging, on(), Some(OsStr::new(until))),
            Ok(Some((CoolOffWaiver::Staging, Some(instant(until))))),
            "{until}"
        );
    }
}

/// DEC-2150 item 2: a staging waiver without an expiry refuses the start, naming the expiry.
#[test]
#[ignore = "pending E9-4"]
fn a_staging_waiver_without_an_expiry_refuses_the_start() {
    assert_eq!(
        cool_off_waiver(Tier::Staging, on(), None),
        Err(StartRefusal::Setting {
            name: COOL_OFF_WAIVER_UNTIL,
        })
    );
}

/// DEC-2150 item 3: a staging expiry later than the ceiling, by a second or by more, refuses the
/// start, so a values edit alone cannot extend the waiver.
#[test]
#[ignore = "pending E9-4"]
fn a_staging_expiry_past_the_ceiling_refuses_the_start() {
    for until in [
        "2026-11-05T00:00:01Z",
        "2026-11-06T00:00:00Z",
        "2027-01-01T00:00:00Z",
    ] {
        assert_eq!(
            cool_off_waiver(Tier::Staging, on(), Some(OsStr::new(until))),
            Err(StartRefusal::Setting {
                name: COOL_OFF_WAIVER_UNTIL,
            }),
            "{until}"
        );
    }
}

/// DEC-2150 item 2: an expiry in any form but `YYYY-MM-DDTHH:MM:SSZ` refuses the start, naming the
/// expiry, in staging and in a test tier alike.
#[test]
#[ignore = "pending E9-4"]
fn an_expiry_in_any_other_form_refuses_the_start() {
    let forms = [
        OsStr::new(""),
        OsStr::new("2026-11-01"),
        OsStr::new("2026-11-01T12:00Z"),
        OsStr::new("2026-11-01T12:00:00.000Z"),
        OsStr::new("2026-11-01T12:00:00+00:00"),
        OsStr::new("2026-11-01 12:00:00Z"),
        OsStr::new("2026-11-01t12:00:00z"),
        OsStr::new(" 2026-11-01T12:00:00Z"),
        OsStr::new("2026-11-31T12:00:00Z"),
        OsStr::from_bytes(b"2026-11-01T12:00:00\xff"),
    ];
    for tier in [Tier::Staging, Tier::Ci, Tier::Dev] {
        for until in forms {
            assert_eq!(
                cool_off_waiver(tier, on(), Some(until)),
                Err(StartRefusal::Setting {
                    name: COOL_OFF_WAIVER_UNTIL,
                }),
                "{until:?} in {tier:?}"
            );
        }
    }
}

/// DEC-2150 item 2: an expiry without the waiver refuses the start in every tier, naming the
/// expiry, so a stray expiry is never silently ignored.
#[test]
#[ignore = "pending E9-4"]
fn an_expiry_without_the_waiver_refuses_the_start() {
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        assert_eq!(
            cool_off_waiver(tier, None, Some(OsStr::new(BEFORE_CEILING))),
            Err(StartRefusal::Setting {
                name: COOL_OFF_WAIVER_UNTIL,
            }),
            "{tier:?}"
        );
    }
}

/// DEC-2150 item 2: a `dev` or `ci` cell honours an expiry it is given, which can only end its
/// waiver, and the staging ceiling does not apply to it.
#[test]
#[ignore = "pending E9-4"]
fn a_test_cell_honours_an_expiry_past_the_ceiling() {
    for (tier, waiver) in [
        (Tier::Dev, CoolOffWaiver::Dev),
        (Tier::Ci, CoolOffWaiver::Ci),
    ] {
        for until in [BEFORE_CEILING, "2027-01-01T00:00:00Z"] {
            assert_eq!(
                cool_off_waiver(tier, on(), Some(OsStr::new(until))),
                Ok(Some((waiver, Some(instant(until))))),
                "{until} in {tier:?}"
            );
        }
    }
}

/// DEC-2146 item 2: any value but exactly `true` refuses the start, in a test tier too, and a
/// value that is not UTF-8 is still a value set.
#[test]
fn a_waiver_of_any_other_value_refuses_the_start_in_every_tier() {
    let refused = Err(StartRefusal::Setting {
        name: COOL_OFF_WAIVER,
    });
    let values = [
        OsStr::new(""),
        OsStr::new("false"),
        OsStr::new("1"),
        OsStr::new("yes"),
        OsStr::new("TRUE"),
        OsStr::new("True"),
        OsStr::new(" true"),
        OsStr::new("true\n"),
        OsStr::from_bytes(b"tr\xffue"),
    ];
    for tier in [Tier::Dev, Tier::Ci, Tier::Staging, Tier::Production] {
        for value in values {
            assert_eq!(
                cool_off_waiver(tier, Some(value), None),
                refused,
                "{value:?} in {tier:?}"
            );
        }
    }
}

/// DEC-2146 item 2: the waiver's name holds no part DEC-978 item 2 refuses, so setting it never
/// stops the server at the secret-name check before the waiver's own check is reached.
#[test]
fn the_waivers_name_passes_the_secret_name_check() {
    assert_eq!(check_environment([(COOL_OFF_WAIVER, "true")]), Ok(()));
    assert_eq!(
        check_environment([(COOL_OFF_WAIVER_UNTIL, BEFORE_CEILING)]),
        Ok(())
    );
}

/// The price read's bars root (DEC-2152 item 5): unset or empty names none, which the read answers
/// `no_store`; any other value is the root, even one that does not exist yet, since the data
/// process may write it after the API starts.
#[test]
fn the_bars_root_is_optional_and_never_refused_at_start() {
    use mandate_api_server::config::{BARS, bars_root};
    assert_eq!(BARS, "MANDATE_BARS");
    assert_eq!(bars_root(None), None);
    assert_eq!(bars_root(Some(OsStr::new(""))), None);
    assert_eq!(
        bars_root(Some(OsStr::new(
            "/var/lib/mandate/artifacts/datasets/alpaca-iex"
        ))),
        Some(PathBuf::from(
            "/var/lib/mandate/artifacts/datasets/alpaca-iex"
        ))
    );
    assert_eq!(
        bars_root(Some(OsStr::new("/nowhere"))),
        Some(PathBuf::from("/nowhere"))
    );
}
