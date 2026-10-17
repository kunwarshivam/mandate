//! The encrypted entries (DEC-692 items 4 and 5): each role reads and writes only what it may,
//! every read checks the file before decrypting, and a file moved onto another name, kind, or
//! connection fails to decrypt.

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};

use mandate_connections::record::Environment;
use secrecy::ExposeSecret;

use super::fixtures::{client_secret, code, conn, key_ref, secret, token};
use super::layout::{Layout, set_mode};
use crate::VaultError;
use crate::entry::Entry;
use crate::startup::{Role, TOKEN_KEY_CREDENTIAL};
use crate::store::{FORMAT_TAG, MAX_SECRET_LEN, NONCE_LEN, TAG_LEN, check_open_file, open};

fn read(executor: &crate::store::ExecutorVault, entry: &Entry) -> Option<Vec<u8>> {
    executor
        .read(entry)
        .unwrap()
        .map(|s| s.expose_secret().to_vec())
}

#[test]
fn each_kind_round_trips_through_its_role() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let c = code("ws/c1", "c1");
    let t = token("ws/c1", "c1");
    api.put_code(&c, &secret("the-code")).unwrap();
    executor.put(&t, &secret("the-token")).unwrap();
    executor
        .put(&client_secret(), &secret("the-client-secret"))
        .unwrap();
    assert_eq!(read(&executor, &c).as_deref(), Some(&b"the-code"[..]));
    assert_eq!(read(&executor, &t).as_deref(), Some(&b"the-token"[..]));
    assert_eq!(
        read(&executor, &client_secret()).as_deref(),
        Some(&b"the-client-secret"[..])
    );
}

#[test]
fn an_entry_file_is_tag_nonce_ciphertext_and_tag_at_its_exact_mode() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let c = code("ws/c1", "c1");
    let t = token("ws/c1", "c1");
    api.put_code(&c, &secret("plain-code-bytes")).unwrap();
    executor.put(&t, &secret("plain-token-bytes")).unwrap();
    for (entry, plain, mode) in [
        (&c, "plain-code-bytes", 0o100_640),
        (&t, "plain-token-bytes", 0o100_600),
    ] {
        let path = layout.file(entry);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &FORMAT_TAG);
        assert_eq!(bytes.len(), 4 + NONCE_LEN + plain.len() + TAG_LEN);
        assert!(
            !bytes.windows(plain.len()).any(|w| w == plain.as_bytes()),
            "{plain} is in the clear"
        );
        let meta = fs::symlink_metadata(&path).unwrap();
        assert_eq!(meta.mode(), mode, "{entry:?}");
        assert_eq!(meta.nlink(), 1);
    }
    assert_eq!(layout.names("pending"), vec![c.file_name()]);
    assert_eq!(layout.names("tokens"), vec![t.file_name()]);
}

#[test]
fn the_nonce_is_fresh_for_every_write() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    let t = token("ws/c1", "c1");
    let mut nonces = Vec::new();
    for _ in 0..8 {
        executor.put(&t, &secret("same-token")).unwrap();
        let bytes = fs::read(layout.file(&t)).unwrap();
        nonces.push(bytes[4..4 + NONCE_LEN].to_vec());
    }
    nonces.sort();
    nonces.dedup();
    assert_eq!(nonces.len(), 8);
    assert_eq!(layout.names("tokens").len(), 1, "no temporary file is left");
}

#[test]
fn a_write_replaces_and_a_delete_removes() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let c = code("ws/c1", "c1");
    let t = token("ws/c1", "c1");
    assert_eq!(read(&executor, &c), None);
    assert_eq!(read(&executor, &t), None);
    api.put_code(&c, &secret("one")).unwrap();
    api.put_code(&c, &secret("two")).unwrap();
    assert_eq!(read(&executor, &c).as_deref(), Some(&b"two"[..]));
    executor.put(&t, &secret("t1")).unwrap();
    api.delete(&t).unwrap();
    assert_eq!(read(&executor, &t), None);
    executor.delete(&c).unwrap();
    assert_eq!(read(&executor, &c), None);
    api.delete(&c).unwrap();
    executor.delete(&t).unwrap();
    executor.put(&client_secret(), &secret("s")).unwrap();
    executor.delete(&client_secret()).unwrap();
    assert_eq!(read(&executor, &client_secret()), None);
    assert!(layout.names("pending").is_empty());
    assert!(layout.names("tokens").is_empty());
}

#[test]
fn each_role_is_refused_what_it_may_not_do() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t = token("ws/c1", "c1");
    let c = code("ws/c1", "c1");
    assert_eq!(
        api.put_code(&t, &secret("x")),
        Err(VaultError::WrongRole { kind: "token" })
    );
    assert_eq!(
        api.put_code(&client_secret(), &secret("x")),
        Err(VaultError::WrongRole {
            kind: "client_secret"
        })
    );
    executor.put(&client_secret(), &secret("s")).unwrap();
    assert_eq!(
        api.delete(&client_secret()),
        Err(VaultError::WrongRole {
            kind: "client_secret"
        })
    );
    assert!(read(&executor, &client_secret()).is_some());
    assert_eq!(
        executor.put(&c, &secret("x")),
        Err(VaultError::WrongRole { kind: "code" })
    );
    assert!(layout.names("pending").is_empty());

    let opened = open(&layout.inputs(Role::Executor)).unwrap();
    assert_eq!(
        opened.into_api().map(drop),
        Err(VaultError::WrongRole { kind: "vault" })
    );
    let layout = layout.without_token_key();
    let opened = open(&layout.inputs(Role::Api)).unwrap();
    assert_eq!(
        opened.into_executor().map(drop),
        Err(VaultError::WrongRole { kind: "vault" })
    );
}

#[test]
fn opening_runs_the_startup_checks() {
    let mut layout = Layout::new();
    layout
        .environment
        .push(("MANDATE_VAULT_KEY".to_owned(), String::new()));
    assert_eq!(
        open(&layout.inputs(Role::Executor)).map(drop),
        Err(VaultError::KeyInEnvironment)
    );
    let layout = Layout::new();
    assert_eq!(
        open(&layout.inputs(Role::Api)).map(drop),
        Err(VaultError::TokenKeyInApiCredentials)
    );
}

#[test]
fn a_secret_is_one_to_the_maximum_bytes() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t = token("ws/c1", "c1");
    let c = code("ws/c1", "c1");
    let max = "m".repeat(MAX_SECRET_LEN);
    executor.put(&t, &secret(&max)).unwrap();
    assert_eq!(read(&executor, &t).map(|v| v.len()), Some(MAX_SECRET_LEN));
    for bad in [String::new(), "m".repeat(MAX_SECRET_LEN + 1)] {
        assert_eq!(executor.put(&t, &secret(&bad)), Err(VaultError::SecretSize));
        assert_eq!(api.put_code(&c, &secret(&bad)), Err(VaultError::SecretSize));
    }
    assert_eq!(read(&executor, &t).map(|v| v.len()), Some(MAX_SECRET_LEN));
    assert!(layout.names("pending").is_empty());
}

#[test]
fn a_file_moved_onto_another_binding_fails_to_decrypt() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t1 = token("ws/c1", "c1");
    executor.put(&t1, &secret("token-one")).unwrap();
    let bytes = fs::read(layout.file(&t1)).unwrap();

    let other_name = token("ws/c2", "c2");
    let same_name_other_connection = token("ws/c1", "c2");
    let same_name_other_environment = Entry::token(key_ref("ws/c1"), conn("c1"), Environment::Live);
    let as_client_secret = Entry::client_secret(key_ref("ws/c1"));
    for target in [
        &other_name,
        &same_name_other_connection,
        &same_name_other_environment,
        &as_client_secret,
    ] {
        executor.put(target, &secret("placeholder")).unwrap();
        fs::write(layout.file(target), &bytes).unwrap();
        assert_eq!(
            executor.read(target).map(|s| s.is_some()),
            Err(VaultError::EntryRejected),
            "{target:?}"
        );
    }

    let c1 = code("ws/c1", "c1");
    api.put_code(&c1, &secret("code-one")).unwrap();
    let code_bytes = fs::read(layout.file(&c1)).unwrap();
    fs::write(layout.file(&t1), &code_bytes).unwrap();
    assert_eq!(
        executor.read(&t1).map(|s| s.is_some()),
        Err(VaultError::EntryRejected),
        "a code under a token's name, under the other key"
    );
}

#[test]
fn a_tampered_or_truncated_file_is_refused() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    let t = token("ws/c1", "c1");
    executor.put(&t, &secret("tamper-me")).unwrap();
    let path = layout.file(&t);
    let good = fs::read(&path).unwrap();
    for i in 4..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 0x01;
        fs::write(&path, &bad).unwrap();
        assert_eq!(
            executor.read(&t).map(|s| s.is_some()),
            Err(VaultError::EntryRejected),
            "byte {i} flipped"
        );
    }
    let mut bad = good.clone();
    bad[0] ^= 0x01;
    fs::write(&path, &bad).unwrap();
    assert_eq!(
        executor.read(&t).map(|s| s.is_some()),
        Err(VaultError::EntryMalformed),
        "the format tag"
    );
    for len in [0, 3, 4, 4 + NONCE_LEN, 4 + NONCE_LEN + TAG_LEN] {
        fs::write(&path, &good[..len]).unwrap();
        assert_eq!(
            executor.read(&t).map(|s| s.is_some()),
            Err(VaultError::EntryMalformed),
            "{len} bytes"
        );
    }
    let mut long = good.clone();
    long.resize(4 + NONCE_LEN + MAX_SECRET_LEN + TAG_LEN + 1, 0);
    fs::write(&path, &long).unwrap();
    assert_eq!(
        executor.read(&t).map(|s| s.is_some()),
        Err(VaultError::EntryMalformed),
        "longer than any entry"
    );
    fs::write(&path, &good).unwrap();
    assert!(executor.read(&t).unwrap().is_some());
}

#[test]
fn another_key_cannot_read_an_entry() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    let t = token("ws/c1", "c1");
    executor.put(&t, &secret("keyed")).unwrap();
    fs::write(layout.credentials.join(TOKEN_KEY_CREDENTIAL), [0x55; 32]).unwrap();
    let other = open(&layout.inputs(Role::Executor))
        .unwrap()
        .into_executor()
        .unwrap();
    assert_eq!(
        other.read(&t).map(|s| s.is_some()),
        Err(VaultError::EntryRejected)
    );
}

#[test]
fn a_file_at_another_mode_is_refused_before_decryption() {
    let cases = [
        ("tokens", 0o640),
        ("tokens", 0o644),
        ("tokens", 0o700),
        ("tokens", 0o601),
        ("tokens", 0o4600),
        ("tokens", 0o400),
        ("pending", 0o600),
        ("pending", 0o660),
        ("pending", 0o644),
        ("pending", 0o2640),
        ("pending", 0o440),
    ];
    for (dir, mode) in cases {
        let layout = Layout::new();
        let (api, executor) = layout.both();
        let entry = if dir == "tokens" {
            let t = token("ws/c1", "c1");
            executor.put(&t, &secret("x")).unwrap();
            t
        } else {
            let c = code("ws/c1", "c1");
            api.put_code(&c, &secret("x")).unwrap();
            c
        };
        set_mode(&layout.file(&entry), mode);
        assert_eq!(
            executor.read(&entry).map(|s| s.is_some()),
            Err(VaultError::FileMismatch { dir }),
            "{dir} at {mode:o}"
        );
    }
}

#[test]
fn a_symlink_a_hard_link_or_a_directory_is_refused() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    let t1 = token("ws/c1", "c1");
    let t2 = token("ws/c2", "c2");
    executor.put(&t1, &secret("one")).unwrap();
    symlink(layout.file(&t1), layout.file(&t2)).unwrap();
    assert_eq!(
        executor.read(&t2).map(|s| s.is_some()),
        Err(VaultError::FileMismatch { dir: "tokens" }),
        "a symlink"
    );
    fs::remove_file(layout.file(&t2)).unwrap();
    fs::hard_link(layout.file(&t1), layout.file(&t2)).unwrap();
    for entry in [&t1, &t2] {
        assert_eq!(
            executor.read(entry).map(|s| s.is_some()),
            Err(VaultError::FileMismatch { dir: "tokens" }),
            "two links"
        );
    }
    fs::remove_file(layout.file(&t2)).unwrap();
    fs::create_dir(layout.file(&t2)).unwrap();
    assert_eq!(
        executor.read(&t2).map(|s| s.is_some()),
        Err(VaultError::FileMismatch { dir: "tokens" }),
        "a directory"
    );
    assert!(executor.read(&t1).unwrap().is_some());
}

#[test]
fn the_open_file_must_be_the_named_inode_with_its_owner() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t = token("ws/c1", "c1");
    let other = token("ws/c2", "c2");
    let c = code("ws/c1", "c1");
    executor.put(&t, &secret("one")).unwrap();
    executor.put(&other, &secret("two")).unwrap();
    api.put_code(&c, &secret("three")).unwrap();
    let named = fs::symlink_metadata(layout.file(&t)).unwrap();
    let opened = fs::File::open(layout.file(&t)).unwrap().metadata().unwrap();
    let swapped = fs::File::open(layout.file(&other))
        .unwrap()
        .metadata()
        .unwrap();
    let ids = layout.ids;
    assert_eq!(check_open_file(t.kind(), &ids, &named, &opened), Ok(()));
    assert_eq!(
        check_open_file(t.kind(), &ids, &named, &swapped),
        Err(VaultError::FileMismatch { dir: "tokens" }),
        "another inode opened than the one named"
    );

    let mut wrong = ids;
    wrong.exec_uid = wrong.exec_uid.wrapping_add(1);
    assert_eq!(
        check_open_file(t.kind(), &wrong, &named, &opened),
        Err(VaultError::FileMismatch { dir: "tokens" }),
        "a token the executor does not own"
    );
    let mut other_group = ids;
    other_group.api_gid = other_group.api_gid.wrapping_add(1);
    other_group.exec_gid = other_group.exec_gid.wrapping_add(1);
    assert_eq!(
        check_open_file(t.kind(), &other_group, &named, &opened),
        Ok(()),
        "a token's group grants nothing at 0600"
    );

    let named = fs::symlink_metadata(layout.file(&c)).unwrap();
    let opened = fs::File::open(layout.file(&c)).unwrap().metadata().unwrap();
    assert_eq!(check_open_file(c.kind(), &ids, &named, &opened), Ok(()));
    for skew in [
        |ids: &mut crate::startup::Ids| ids.api_uid = ids.api_uid.wrapping_add(1),
        |ids: &mut crate::startup::Ids| ids.exec_gid = ids.exec_gid.wrapping_add(1),
    ] {
        let mut wrong = ids;
        skew(&mut wrong);
        assert_eq!(
            check_open_file(c.kind(), &wrong, &named, &opened),
            Err(VaultError::FileMismatch { dir: "pending" })
        );
    }
}

#[test]
fn a_delete_that_fails_for_another_reason_than_absence_is_an_io_error() {
    let layout = Layout::new();
    let (api, executor) = layout.both();
    let t = token("ws/c1", "c1");
    fs::create_dir(layout.file(&t)).unwrap();
    assert_eq!(executor.delete(&t), Err(VaultError::Io));
    assert_eq!(api.delete(&t), Err(VaultError::Io));
}
