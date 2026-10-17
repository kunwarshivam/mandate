//! The subject link (§3.11 item 7, DEC-978 item 7).

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use common::hex;
use mandate_api_server::subject::{LinkRefusal, digest, principal, principal_owned};
use mandate_identity::PrincipalId;

const ISSUER: &str = "https://issuer.test/auth/v1";
const ALICE: &str = "6a1f2c3d-alice";
const ALICE_ID: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const BOB: &str = "9b8e7d6c-bob";
const BOB_ID: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";

/// The digest computed apart from the server: SHA-256 of the canonical object, written by hand.
fn expected(issuer: &str, subject: &str) -> String {
    let canonical = format!(r#"{{"issuer":"{issuer}","subject":"{subject}"}}"#);
    let sum = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
    format!("sha256:{}", hex(sum.as_ref()))
}

static FILES: AtomicU32 = AtomicU32::new(0);

fn links(text: &str, mode: u32) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-links-{}-{}",
        std::process::id(),
        FILES.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("subject-links");
    fs::write(&path, text).expect("the links");
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("its mode");
    path
}

fn id(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn two() -> String {
    format!(
        "{} {ALICE_ID}\n{} {BOB_ID}\n",
        expected(ISSUER, ALICE),
        expected(ISSUER, BOB)
    )
}

fn mine(path: &PathBuf) -> [u32; 2] {
    [fs::metadata(path).expect("metadata").uid(), 0]
}

#[test]
fn the_digest_is_sha256_of_the_canonical_issuer_and_subject() {
    assert_eq!(digest(ISSUER, ALICE), expected(ISSUER, ALICE));
    assert_ne!(digest(ISSUER, ALICE), digest(ISSUER, BOB));
    assert_ne!(
        digest(ISSUER, ALICE),
        digest("https://other.test/auth/v1", ALICE)
    );
    let quoted = r#"a"b\c"#;
    let canonical = r#"{"issuer":"https://issuer.test/auth/v1","subject":"a\"b\\c"}"#;
    let sum = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
    assert_eq!(
        digest(ISSUER, quoted),
        format!("sha256:{}", hex(sum.as_ref()))
    );
}

#[test]
fn a_linked_subject_signs_in_as_its_principal() {
    let path = links(&two(), 0o644);
    assert_eq!(principal(&path, ISSUER, ALICE), Ok(id(ALICE_ID)));
    assert_eq!(principal(&path, ISSUER, BOB), Ok(id(BOB_ID)));
    let path = links(&two(), 0o600);
    assert_eq!(principal(&path, ISSUER, BOB), Ok(id(BOB_ID)));
    let path = links(&two(), 0o640);
    assert_eq!(principal(&path, ISSUER, ALICE), Ok(id(ALICE_ID)));
}

#[test]
fn an_unlinked_subject_or_another_issuers_is_not_linked() {
    let path = links(&two(), 0o644);
    assert_eq!(
        principal(&path, ISSUER, "someone-else"),
        Err(LinkRefusal::NotLinked)
    );
    assert_eq!(
        principal(&path, "https://other.test/auth/v1", ALICE),
        Err(LinkRefusal::NotLinked)
    );
    let path = links("", 0o644);
    assert_eq!(principal(&path, ISSUER, ALICE), Err(LinkRefusal::NotLinked));
}

#[test]
fn a_missing_file_or_a_directory_is_unreadable() {
    let path = links(&two(), 0o644);
    let dir = path.parent().expect("its directory").to_path_buf();
    assert_eq!(
        principal(&dir.join("absent"), ISSUER, ALICE),
        Err(LinkRefusal::Unreadable)
    );
    assert_eq!(principal(&dir, ISSUER, ALICE), Err(LinkRefusal::Unreadable));
}

#[test]
fn a_file_others_can_write_is_refused_whole() {
    for mode in [0o664, 0o646, 0o666, 0o620, 0o602] {
        let path = links(&two(), mode);
        assert_eq!(
            principal(&path, ISSUER, ALICE),
            Err(LinkRefusal::Exposed),
            "{mode:o}"
        );
    }
}

#[test]
fn a_file_owned_by_neither_the_process_nor_root_is_refused() {
    let path = links(&two(), 0o644);
    let owner = fs::metadata(&path).expect("metadata").uid();
    assert_eq!(
        principal_owned(&path, ISSUER, ALICE, [owner, 4_000_000_001]),
        Ok(id(ALICE_ID))
    );
    assert_eq!(
        principal_owned(&path, ISSUER, ALICE, [4_000_000_001, owner]),
        Ok(id(ALICE_ID))
    );
    assert_eq!(
        principal_owned(&path, ISSUER, ALICE, [4_000_000_001, 4_000_000_002]),
        Err(LinkRefusal::Exposed)
    );
}

#[test]
fn a_malformed_line_refuses_the_whole_file() {
    let good = format!("{} {ALICE_ID}", expected(ISSUER, ALICE));
    let bob = expected(ISSUER, BOB);
    let hex_only = bob.trim_start_matches("sha256:").to_owned();
    for bad in [
        format!("{hex_only} {BOB_ID}"),
        format!(
            "{} {BOB_ID}",
            bob.to_uppercase().replace("SHA256:", "sha256:")
        ),
        format!("{bob}  {BOB_ID}"),
        format!("{bob} {BOB_ID} trailing"),
        format!("{bob}\t{BOB_ID}"),
        format!("{bob} {}", BOB_ID.to_lowercase()),
        format!("{bob} 81JB3K6F0C9R2V7N4M8Q1T5W3Z"),
        format!("{bob} {BOB_ID}\r"),
        format!("{} {BOB_ID}", &bob[..bob.len() - 1]),
        "# a comment".to_owned(),
        String::new(),
        format!(" {bob} {BOB_ID}"),
    ] {
        let path = links(&format!("{good}\n{bad}\n"), 0o644);
        assert_eq!(
            principal(&path, ISSUER, ALICE),
            Err(LinkRefusal::Malformed),
            "{bad:?}"
        );
    }
    let path = links(&format!("{good}\n\n"), 0o644);
    assert_eq!(principal(&path, ISSUER, ALICE), Err(LinkRefusal::Malformed));
}

#[test]
fn a_last_line_without_its_newline_is_read() {
    let path = links(&format!("{} {ALICE_ID}", expected(ISSUER, ALICE)), 0o644);
    assert_eq!(principal(&path, ISSUER, ALICE), Ok(id(ALICE_ID)));
}

#[test]
fn a_digest_named_twice_refuses_the_whole_file_even_for_one_principal() {
    let alice = expected(ISSUER, ALICE);
    for second in [ALICE_ID, BOB_ID] {
        let path = links(&format!("{alice} {ALICE_ID}\n{alice} {second}\n"), 0o644);
        assert_eq!(
            principal(&path, ISSUER, ALICE),
            Err(LinkRefusal::Duplicate),
            "{second}"
        );
        assert_eq!(
            principal(&path, ISSUER, BOB),
            Err(LinkRefusal::Duplicate),
            "the refusal holds for every subject"
        );
    }
}

#[test]
fn the_file_is_read_again_at_each_call() {
    let path = links(&two(), 0o644);
    assert_eq!(principal(&path, ISSUER, BOB), Ok(id(BOB_ID)));
    fs::write(&path, format!("{} {ALICE_ID}\n", expected(ISSUER, ALICE))).expect("rewrite");
    assert_eq!(principal(&path, ISSUER, BOB), Err(LinkRefusal::NotLinked));
    assert_eq!(mine(&path)[1], 0);
}

/// DEC-1023: a symlink at the link path is refused before it is opened, even when it points at a
/// file every other check accepts, so the server reads links only from the file at the path itself.
#[test]
fn a_symlink_at_the_link_path_is_refused() {
    let target = links(&two(), 0o644);
    assert_eq!(principal(&target, ISSUER, ALICE), Ok(id(ALICE_ID)));
    let link = target.with_file_name("linked");
    std::os::unix::fs::symlink(&target, &link).expect("a symlink");
    assert_eq!(
        principal(&link, ISSUER, ALICE),
        Err(LinkRefusal::Exposed),
        "a symlink to an acceptable file is still refused"
    );
    let dangling = target.with_file_name("dangling");
    std::os::unix::fs::symlink(target.with_file_name("absent"), &dangling).expect("a symlink");
    assert_eq!(
        principal(&dangling, ISSUER, ALICE),
        Err(LinkRefusal::Unreadable)
    );
}

/// DEC-1023: the server reads one fixed path, no setting.
#[test]
fn the_link_file_is_the_one_fixed_path() {
    assert_eq!(
        mandate_api_server::subject::SUBJECT_LINKS,
        "/etc/mandate/subject-links"
    );
}
