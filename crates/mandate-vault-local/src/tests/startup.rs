//! Startup refuses every arrangement but the one DEC-692 and DEC-822 item 4 name: keys only as
//! systemd credentials, the token key only for the executor, and exact owners and modes.

use std::fs;
use std::os::unix::fs::symlink;

use secrecy::ExposeSecret;

use super::layout::{Layout, PENDING_KEY, TOKEN_KEY, set_mode};
use crate::VaultError;
use crate::startup::{
    FORBIDDEN_KEY_VARIABLES, Ids, PENDING_KEY_CREDENTIAL, Role, TOKEN_KEY_CREDENTIAL, check,
};

fn refused(layout: &Layout, role: Role) -> VaultError {
    match check(&layout.inputs(role)) {
        Ok(_) => panic!("{role:?} started on a layout it must refuse"),
        Err(e) => e,
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_correct_layout_starts_each_role_with_its_keys() {
    let layout = Layout::new();
    let executor = check(&layout.inputs(Role::Executor)).unwrap();
    assert_eq!(executor.pending.expose_secret(), &PENDING_KEY);
    assert_eq!(
        executor.token.as_ref().map(|k| *k.expose_secret()),
        Some(TOKEN_KEY)
    );
    let layout = layout.without_token_key();
    let api = check(&layout.inputs(Role::Api)).unwrap();
    assert_eq!(api.pending.expose_secret(), &PENDING_KEY);
    assert!(api.token.is_none());
}

#[test]
#[ignore = "pending E10-13"]
fn a_key_variable_in_the_environment_refuses_startup() {
    for name in FORBIDDEN_KEY_VARIABLES {
        for value in [
            "",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            for role in [Role::Api, Role::Executor] {
                let mut layout = Layout::new();
                if role == Role::Api {
                    layout = layout.without_token_key();
                }
                layout.environment.push((name.to_owned(), value.to_owned()));
                assert_eq!(
                    refused(&layout, role),
                    VaultError::KeyInEnvironment,
                    "{name}={value:?} {role:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "pending E10-13"]
fn the_api_process_refuses_the_token_key() {
    let layout = Layout::new();
    assert_eq!(
        refused(&layout, Role::Api),
        VaultError::TokenKeyInApiCredentials
    );
}

#[test]
#[ignore = "pending E10-13"]
fn each_key_must_be_present_and_exactly_32_bytes() {
    let layout = Layout::new().without_token_key();
    assert_eq!(refused(&layout, Role::Executor), VaultError::KeyMissing);

    let layout = Layout::new();
    fs::remove_file(layout.credentials.join(PENDING_KEY_CREDENTIAL)).unwrap();
    assert_eq!(refused(&layout, Role::Executor), VaultError::KeyMissing);

    for credential in [PENDING_KEY_CREDENTIAL, TOKEN_KEY_CREDENTIAL] {
        for len in [0, 31, 33, 64] {
            let layout = Layout::new();
            fs::write(layout.credentials.join(credential), vec![1_u8; len]).unwrap();
            assert_eq!(
                refused(&layout, Role::Executor),
                VaultError::KeyMalformed,
                "{credential} of {len} bytes"
            );
        }
        let layout = Layout::new();
        let path = layout.credentials.join(credential);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            refused(&layout, Role::Executor),
            VaultError::KeyMalformed,
            "{credential} as a directory"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn every_directory_must_have_its_exact_mode() {
    let cases = [
        ("", "vault", 0o775),
        ("", "vault", 0o750),
        ("", "vault", 0o1755),
        ("pending", "pending", 0o770),
        ("pending", "pending", 0o2775),
        ("pending", "pending", 0o2777),
        ("pending", "pending", 0o3770),
        ("tokens", "tokens", 0o700),
        ("tokens", "tokens", 0o733),
        ("tokens", "tokens", 0o2730),
        ("tokens", "tokens", 0o770),
    ];
    for (sub, dir, mode) in cases {
        let layout = Layout::new();
        set_mode(&layout.vault.join(sub), mode);
        assert_eq!(
            refused(&layout, Role::Executor),
            VaultError::DirectoryMismatch { dir },
            "{dir} at {mode:o}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn every_directory_must_have_its_owner_and_group() {
    type Skew = fn(&mut Ids);
    let cases: [(Skew, &str); 5] = [
        (|ids| ids.root_uid = ids.root_uid.wrapping_add(1), "vault"),
        (|ids| ids.api_uid = ids.api_uid.wrapping_add(1), "pending"),
        (|ids| ids.exec_gid = ids.exec_gid.wrapping_add(1), "pending"),
        (|ids| ids.exec_uid = ids.exec_uid.wrapping_add(1), "tokens"),
        (|ids| ids.api_gid = ids.api_gid.wrapping_add(1), "tokens"),
    ];
    for (skew, dir) in cases {
        let mut layout = Layout::new();
        skew(&mut layout.ids);
        assert_eq!(
            refused(&layout, Role::Executor),
            VaultError::DirectoryMismatch { dir },
            "{dir}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_symlinked_or_missing_directory_is_refused() {
    let layout = Layout::new();
    let real = layout.root.join("elsewhere");
    fs::rename(layout.vault.join("pending"), &real).unwrap();
    symlink(&real, layout.vault.join("pending")).unwrap();
    assert_eq!(
        refused(&layout, Role::Executor),
        VaultError::DirectoryMismatch { dir: "pending" }
    );

    let layout = Layout::new();
    fs::remove_dir(layout.vault.join("tokens")).unwrap();
    assert_eq!(
        refused(&layout, Role::Executor),
        VaultError::DirectoryMismatch { dir: "tokens" }
    );

    let layout = Layout::new();
    let real = layout.root.join("real-vault");
    fs::rename(&layout.vault, &real).unwrap();
    symlink(&real, &layout.vault).unwrap();
    assert_eq!(
        refused(&layout, Role::Executor),
        VaultError::DirectoryMismatch { dir: "vault" }
    );
}
