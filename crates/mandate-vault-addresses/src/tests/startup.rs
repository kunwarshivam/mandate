//! Opening refuses every arrangement but DEC-2120 item 1's, and holds its directory open
//! (item 2).

use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};

use super::layout::{ALICE, Layout, WORKSPACE, address, entry, member, set_mode, workspace};
use crate::startup::{
    ADDRESS_KEY_CREDENTIAL, ADDRESSES_DIR, FORBIDDEN_KEY_VARIABLES, PENDING_KEY_CREDENTIAL,
    PREVIOUS_ADDRESS_KEY_CREDENTIAL, TOKEN_KEY_CREDENTIAL, open_reader, open_writer,
};
use crate::{AddressError, UnixNanos};

/// Both handles' answers on one layout, writer first.
fn refusals(layout: &Layout) -> [Result<(), AddressError>; 2] {
    [
        open_writer(&layout.inputs()).map(drop),
        open_reader(&layout.inputs()).map(drop),
    ]
}

/// One way to break a correct layout.
type Breakage = fn(&Layout);

#[test]
#[ignore = "pending E8-14"]
fn a_correct_layout_opens_both_handles_and_a_previous_key_is_optional() {
    let layout = Layout::new();
    assert_eq!(refusals(&layout), [Ok(()), Ok(())]);
    layout.install_key(PREVIOUS_ADDRESS_KEY_CREDENTIAL, [4; 32]);
    assert_eq!(refusals(&layout), [Ok(()), Ok(())]);
}

#[test]
#[ignore = "pending E8-14"]
fn a_key_in_the_environment_is_refused_whatever_its_value() {
    for name in FORBIDDEN_KEY_VARIABLES {
        let mut layout = Layout::new();
        layout.environment.push((name.to_owned(), String::new()));
        let refused = Err(AddressError::KeyInEnvironment);
        assert_eq!(refusals(&layout), [refused.clone(), refused], "{name}");
    }
}

#[test]
#[ignore = "pending E8-14"]
fn a_wrong_key_credential_is_refused() {
    let cases: [(&str, Breakage); 6] = [
        ("missing", |l| {
            fs::remove_file(l.credentials.join(ADDRESS_KEY_CREDENTIAL)).unwrap()
        }),
        ("31 bytes", |l| {
            let key = l.credentials.join(ADDRESS_KEY_CREDENTIAL);
            set_mode(&key, 0o600);
            fs::write(&key, [5; 31]).unwrap();
        }),
        ("group readable", |l| {
            set_mode(&l.credentials.join(ADDRESS_KEY_CREDENTIAL), 0o440)
        }),
        ("a symlink", |l| {
            let key = l.credentials.join(ADDRESS_KEY_CREDENTIAL);
            let real = l.root.join("real-key");
            fs::rename(&key, &real).unwrap();
            symlink(&real, &key).unwrap();
        }),
        ("a directory", |l| {
            let key = l.credentials.join(ADDRESS_KEY_CREDENTIAL);
            fs::remove_file(&key).unwrap();
            fs::create_dir(&key).unwrap();
        }),
        ("a previous key of 33 bytes", |l| {
            fs::write(l.credentials.join(PREVIOUS_ADDRESS_KEY_CREDENTIAL), [4; 33]).unwrap();
            set_mode(&l.credentials.join(PREVIOUS_ADDRESS_KEY_CREDENTIAL), 0o400);
        }),
    ];
    for (why, break_it) in cases {
        let layout = Layout::new();
        break_it(&layout);
        let refused = Err(AddressError::KeyCredential);
        assert_eq!(refusals(&layout), [refused.clone(), refused], "{why}");
    }
}

#[test]
#[ignore = "pending E8-14"]
fn another_vault_key_among_the_credentials_is_refused() {
    let layout = Layout::new();
    fs::write(layout.credentials.join(TOKEN_KEY_CREDENTIAL), [9; 32]).unwrap();
    let refused = Err(AddressError::TokenKeyPresent);
    assert_eq!(
        refusals(&layout),
        [refused.clone(), refused],
        "the token key reaches neither"
    );
    let layout = Layout::new();
    fs::write(layout.credentials.join(PENDING_KEY_CREDENTIAL), [7; 32]).unwrap();
    assert_eq!(
        refusals(&layout),
        [Ok(()), Err(AddressError::PendingKeyPresent)],
        "the API holds the pending key; the dispatcher never does"
    );
}

#[test]
#[ignore = "pending E8-14"]
fn a_vault_root_not_owned_by_root_at_0755_is_refused() {
    let refused = || Err(AddressError::Layout { dir: "vault" });
    for mode in [0o775, 0o750, 0o1755] {
        let layout = Layout::new();
        set_mode(&layout.vault, mode);
        assert_eq!(refusals(&layout), [refused(), refused()], "{mode:o}");
    }
    let mut layout = Layout::new();
    layout.ids.root_uid = layout.ids.root_uid.wrapping_add(1);
    assert_eq!(refusals(&layout), [refused(), refused()], "another owner");
    let layout = Layout::new();
    let real = layout.root.join("real-vault");
    fs::rename(&layout.vault, &real).unwrap();
    symlink(&real, &layout.vault).unwrap();
    assert_eq!(refusals(&layout), [refused(), refused()], "a symlink");
}

#[test]
#[ignore = "pending E8-14"]
fn wrong_directory_modes_are_refused() {
    let refused = || Err(AddressError::Layout { dir: "addresses" });
    for mode in [0o710, 0o2750, 0o2770, 0o2711, 0o2700, 0o6710, 0o3710] {
        let layout = Layout::new();
        set_mode(&layout.vault.join(ADDRESSES_DIR), mode);
        assert_eq!(refusals(&layout), [refused(), refused()], "{mode:o}");
    }
    let mut layout = Layout::new();
    layout.ids.notify_gid = layout.ids.notify_gid.wrapping_add(1);
    assert_eq!(refusals(&layout), [refused(), refused()], "another group");
    let mut layout = Layout::new();
    layout.ids.api_uid = layout.ids.api_uid.wrapping_add(1);
    assert_eq!(refusals(&layout), [refused(), refused()], "another owner");
    let layout = Layout::new();
    let addresses = layout.vault.join(ADDRESSES_DIR);
    let real = layout.root.join("real-addresses");
    fs::rename(&addresses, &real).unwrap();
    symlink(&real, &addresses).unwrap();
    assert_eq!(refusals(&layout), [refused(), refused()], "a symlink");
}

#[test]
#[ignore = "pending E8-14"]
fn workspace_directories_are_made_2710_and_checked_on_every_operation() {
    let layout = Layout::new();
    let writer = layout.writer();
    let ws = workspace(WORKSPACE);
    let it = entry(ALICE, "https://push.example/a", 1);
    writer
        .put_if_absent(&ws, &address(1), &it, UnixNanos(1))
        .unwrap();
    let dir = layout.workspace_dir(WORKSPACE);
    assert_eq!(
        fs::metadata(&dir).unwrap().mode(),
        0o042_710,
        "DEC-2120 item 1"
    );
    set_mode(&dir, 0o2750);
    let refused = Err(AddressError::Layout { dir: "workspace" });
    assert_eq!(
        layout
            .reader()
            .entry(&ws, &member(ALICE), &address(1))
            .map(drop),
        refused.clone()
    );
    assert_eq!(
        writer
            .put_again(&ws, &address(1), &it, UnixNanos(2))
            .map(drop),
        refused
    );
}

#[test]
#[ignore = "pending E8-14"]
fn every_open_is_relative_to_the_directory_opened_at_start() {
    let layout = Layout::new();
    let writer = layout.writer();
    let reader = layout.reader();
    let ws = workspace(WORKSPACE);
    let addresses = layout.vault.join(ADDRESSES_DIR);
    let held = layout.root.join("held-addresses");
    fs::rename(&addresses, &held).unwrap();
    fs::create_dir(&addresses).unwrap();
    set_mode(&addresses, 0o2710);
    writer
        .put_if_absent(
            &ws,
            &address(1),
            &entry(ALICE, "https://push.example/a", 1),
            UnixNanos(1),
        )
        .unwrap();
    assert!(
        held.join(WORKSPACE)
            .join(super::layout::address_text(1))
            .exists()
    );
    assert!(
        !addresses.join(WORKSPACE).exists(),
        "a path swapped in after the open is never used"
    );
    assert!(matches!(
        reader.entry(&ws, &member(ALICE), &address(1)),
        Ok(Some(_))
    ));
}
