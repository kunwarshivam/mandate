//! Reading a mounted Kubernetes Secret (DEC-2630): the kubelet's `..data` links followed inside
//! the mount and nowhere else, the mode judged on the file reached, and each value one line.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use mandate_connections::record::ConnectionId;
use secrecy::ExposeSecret;

use crate::SourceError;
use crate::mounted::{read_mounted, read_mounted_credentials};
use crate::policy::Identity;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A fresh directory under the temporary directory, removed when dropped.
struct Fresh(PathBuf);

impl Fresh {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("vault-mounted-{}-{n}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str, mode: u32) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// The kubelet's layout: each file in a timestamped directory, `..data` a relative link to it,
/// and each name a relative link through `..data`.
fn kubelet(dir: &Path, files: &[(&str, &str, u32)]) {
    let stamped = dir.join("..2026_10_19_15_00_00.000000001");
    fs::create_dir(&stamped).unwrap();
    for (name, text, mode) in files {
        write(&stamped.join(name), text, *mode);
        symlink(format!("..data/{name}"), dir.join(name)).unwrap();
    }
    symlink("..2026_10_19_15_00_00.000000001", dir.join("..data")).unwrap();
}

fn executor() -> Identity {
    Identity::Executor(ConnectionId::new("c1").unwrap())
}

fn read(dir: &Path, name: &str) -> Result<String, SourceError> {
    read_mounted(dir, name).map(|value| value.expose_secret().to_owned())
}

fn credentials(dir: &Path, identity: &Identity) -> Result<(String, String), SourceError> {
    read_mounted_credentials(dir, identity)
        .map(|(role_id, secret_id)| (role_id, secret_id.expose_secret().to_owned()))
}

#[test]
#[ignore = "pending E21-32"]
fn an_executors_credentials_are_read_through_the_kubelets_links() {
    let dir = Fresh::new("kubelet");
    kubelet(
        &dir.0,
        &[
            ("executor-c1.role-id", "role-1\n", 0o400),
            ("executor-c1.secret-id", "secret-1", 0o400),
        ],
    );
    assert_eq!(
        credentials(&dir.0, &executor()),
        Ok(("role-1".to_owned(), "secret-1".to_owned()))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn plain_files_owner_only_are_read_too() {
    let dir = Fresh::new("plain");
    write(&dir.0.join("runtime.role-id"), "role-r", 0o600);
    write(&dir.0.join("runtime.secret-id"), "secret-r\n", 0o400);
    assert_eq!(
        credentials(&dir.0, &Identity::Runtime),
        Ok(("role-r".to_owned(), "secret-r".to_owned()))
    );
}

#[test]
#[ignore = "pending E21-32"]
fn a_ca_is_read_whole_lines_and_all() {
    let dir = Fresh::new("ca");
    let pem = "-----BEGIN CERTIFICATE-----\nAAAA\nBBBB\n-----END CERTIFICATE-----\n";
    kubelet(&dir.0, &[("ca.crt", pem, 0o400)]);
    assert_eq!(read(&dir.0, "ca.crt"), Ok(pem.to_owned()));
}

/// `..data` pointing at an absolute directory outside the mount is refused, though the file
/// there is owner-only and well formed.
#[test]
#[ignore = "pending E21-32"]
fn a_data_link_to_an_absolute_path_outside_the_mount_is_refused() {
    let dir = Fresh::new("absolute");
    let outside = Fresh::new("outside");
    write(&outside.0.join("ca.crt"), "planted", 0o400);
    symlink(&outside.0, dir.0.join("..data")).unwrap();
    symlink("..data/ca.crt", dir.0.join("ca.crt")).unwrap();
    assert_eq!(read(&dir.0, "ca.crt"), Err(SourceError::CredentialFile));
}

/// A name linked by a relative path that climbs out of the mount is refused.
#[test]
#[ignore = "pending E21-32"]
fn a_link_that_climbs_out_of_the_mount_is_refused() {
    let parent = Fresh::new("parent");
    let dir = parent.0.join("mount");
    fs::create_dir(&dir).unwrap();
    write(&parent.0.join("ca.crt"), "planted", 0o400);
    symlink("../ca.crt", dir.join("ca.crt")).unwrap();
    assert_eq!(read(&dir, "ca.crt"), Err(SourceError::CredentialFile));
}

/// The mode is judged on the file the links reach: any group or other bit is refused.
#[test]
#[ignore = "pending E21-32"]
fn a_reached_file_open_to_group_or_others_is_refused() {
    for mode in [0o440, 0o404, 0o644, 0o401, 0o410] {
        let dir = Fresh::new("mode");
        kubelet(&dir.0, &[("ca.crt", "x", mode)]);
        assert_eq!(
            read(&dir.0, "ca.crt"),
            Err(SourceError::CredentialFile),
            "{mode:o}"
        );
    }
}

#[test]
#[ignore = "pending E21-32"]
fn a_missing_file_a_directory_or_a_dangling_link_is_refused() {
    let dir = Fresh::new("missing");
    fs::create_dir(dir.0.join("sub")).unwrap();
    symlink("..data/gone", dir.0.join("dangling")).unwrap();
    for name in ["absent", "sub", "dangling"] {
        assert_eq!(
            read(&dir.0, name),
            Err(SourceError::CredentialFile),
            "{name}"
        );
    }
}

/// A name that is not one plain path component is refused before anything is opened.
#[test]
#[ignore = "pending E21-32"]
fn a_name_that_is_not_one_component_is_refused() {
    let dir = Fresh::new("names");
    let inner = dir.0.join("inner");
    fs::create_dir(&inner).unwrap();
    write(&inner.join("ca.crt"), "x", 0o400);
    write(&dir.0.join("ca.crt"), "x", 0o400);
    for name in ["", ".", "..", "inner/ca.crt", "../ca.crt", "/etc/hostname"] {
        assert_eq!(
            read(&inner, name),
            Err(SourceError::CredentialFile),
            "{name:?}"
        );
    }
}

#[test]
#[ignore = "pending E21-32"]
fn a_credential_that_is_empty_or_not_one_line_is_refused() {
    for text in ["", "\n", "a\n\n", "a\nb", "a\r\n", "a\rb"] {
        let dir = Fresh::new("lines");
        write(&dir.0.join("executor-c1.role-id"), "role-1", 0o400);
        write(&dir.0.join("executor-c1.secret-id"), text, 0o400);
        assert_eq!(
            credentials(&dir.0, &executor()),
            Err(SourceError::CredentialFile),
            "{text:?}"
        );
    }
}

#[test]
#[ignore = "pending E21-32"]
fn a_missing_role_id_refuses_the_credentials() {
    let dir = Fresh::new("half");
    write(&dir.0.join("executor-c1.secret-id"), "secret-1", 0o400);
    assert_eq!(
        credentials(&dir.0, &executor()),
        Err(SourceError::CredentialFile)
    );
}

#[test]
#[ignore = "pending E21-32"]
fn a_token_exchange_has_no_mounted_credentials() {
    let dir = Fresh::new("exchange");
    write(&dir.0.join("exchange-c1.role-id"), "role-1", 0o400);
    write(&dir.0.join("exchange-c1.secret-id"), "secret-1", 0o400);
    assert_eq!(
        credentials(
            &dir.0,
            &Identity::TokenExchange(ConnectionId::new("c1").unwrap())
        ),
        Err(SourceError::Config)
    );
}
