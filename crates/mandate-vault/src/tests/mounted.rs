//! Reading a mounted Kubernetes Secret (DEC-2630): the kubelet's `..data` links followed inside
//! the mount and nowhere else, the mode judged on the file reached, and each value one line.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mandate_connections::record::ConnectionId;
use rustix::fs::{CWD, FileType, Mode, mknodat};
use secrecy::ExposeSecret;

use crate::SourceError;
use crate::mounted::{
    CA_FILE, read_checked, read_mounted, read_mounted_ca, read_mounted_credentials,
    read_service_account_namespace, read_service_account_token,
};
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
fn a_ca_is_read_whole_lines_and_all() {
    let dir = Fresh::new("ca");
    let pem = "-----BEGIN CERTIFICATE-----\nAAAA\nBBBB\n-----END CERTIFICATE-----\n";
    kubelet(&dir.0, &[("ca.crt", pem, 0o400)]);
    assert_eq!(read(&dir.0, "ca.crt"), Ok(pem.to_owned()));
}

/// `..data` pointing at an absolute directory outside the mount is refused, though the file
/// there is owner-only and well formed.
#[test]
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
fn a_missing_role_id_refuses_the_credentials() {
    let dir = Fresh::new("half");
    write(&dir.0.join("executor-c1.secret-id"), "secret-1", 0o400);
    assert_eq!(
        credentials(&dir.0, &executor()),
        Err(SourceError::CredentialFile)
    );
}

#[test]
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

fn read_swapped(dir: &Path, name: &str, swap: &dyn Fn(&Path)) -> Result<String, SourceError> {
    read_checked(dir, name, swap).map(|value| value.expose_secret().to_owned())
}

fn fifo(path: &Path) {
    mknodat(CWD, path, FileType::Fifo, Mode::from_raw_mode(0o600), 0).unwrap();
}

/// Another owner-only file renamed over the checked one before it is opened is refused, by its
/// inode, though it would pass every other check.
#[test]
fn a_file_swapped_in_after_the_check_is_refused() {
    let dir = Fresh::new("swap-file");
    write(&dir.0.join("ca.crt"), "checked", 0o400);
    write(&dir.0.join("other"), "swapped", 0o400);
    let swap = |reached: &Path| fs::rename(reached.with_file_name("other"), reached).unwrap();
    assert_eq!(
        read_swapped(&dir.0, "ca.crt", &swap),
        Err(SourceError::CredentialFile)
    );
}

/// A link swapped in for the checked file is not followed.
#[test]
fn a_link_swapped_in_after_the_check_is_refused() {
    let dir = Fresh::new("swap-link");
    write(&dir.0.join("ca.crt"), "checked", 0o400);
    write(&dir.0.join("other"), "swapped", 0o400);
    let swap = |reached: &Path| {
        fs::remove_file(reached).unwrap();
        symlink("other", reached).unwrap();
    };
    assert_eq!(
        read_swapped(&dir.0, "ca.crt", &swap),
        Err(SourceError::CredentialFile)
    );
}

/// Runs `read` on another thread and fails the test if it has not answered within ten seconds,
/// so a read that blocks on a pipe fails rather than hangs.
fn within_ten_seconds(
    read: impl FnOnce() -> Result<String, SourceError> + Send + 'static,
) -> Result<String, SourceError> {
    let (answer, answered) = mpsc::channel();
    thread::spawn(move || drop(answer.send(read())));
    answered
        .recv_timeout(Duration::from_secs(10))
        .expect("the read blocked on a pipe")
}

/// A pipe swapped in for the checked file is refused without waiting for a writer.
#[test]
fn a_pipe_swapped_in_after_the_check_is_refused_without_blocking() {
    let dir = Fresh::new("swap-fifo");
    write(&dir.0.join("ca.crt"), "checked", 0o400);
    let path = dir.0.clone();
    let answer = within_ten_seconds(move || {
        let swap = |reached: &Path| {
            fs::remove_file(reached).unwrap();
            fifo(reached);
        };
        read_swapped(&path, "ca.crt", &swap)
    });
    assert_eq!(answer, Err(SourceError::CredentialFile));
}

/// An owner-only pipe at the name itself is refused, not read as empty, without blocking.
#[test]
fn a_pipe_at_the_name_is_refused() {
    let dir = Fresh::new("fifo");
    fifo(&dir.0.join("ca.crt"));
    let path = dir.0.clone();
    let answer = within_ten_seconds(move || read(&path, "ca.crt"));
    assert_eq!(answer, Err(SourceError::CredentialFile));
}

/// A file of 65536 bytes is read; one byte more is refused, so a read is bounded.
#[test]
fn a_file_over_the_bound_is_refused() {
    let dir = Fresh::new("bound");
    let longest = "a".repeat(65536);
    write(&dir.0.join("ca.crt"), &longest, 0o400);
    assert_eq!(read(&dir.0, "ca.crt"), Ok(longest.clone()));
    write(&dir.0.join("big.crt"), &format!("{longest}a"), 0o400);
    assert_eq!(read(&dir.0, "big.crt"), Err(SourceError::CredentialFile));
}

/// A mode widened on the checked file before it is opened is judged on the handle and refused.
#[test]
fn a_mode_widened_after_the_check_is_refused() {
    let dir = Fresh::new("swap-mode");
    write(&dir.0.join("ca.crt"), "checked", 0o400);
    let widen = |reached: &Path| {
        fs::set_permissions(reached, fs::Permissions::from_mode(0o444)).unwrap();
    };
    assert_eq!(
        read_swapped(&dir.0, "ca.crt", &widen),
        Err(SourceError::CredentialFile)
    );
}

#[test]
fn a_file_that_is_not_utf8_is_refused() {
    let dir = Fresh::new("utf8");
    fs::write(dir.0.join("ca.crt"), b"\xff\xfe").unwrap();
    fs::set_permissions(dir.0.join("ca.crt"), fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(read(&dir.0, "ca.crt"), Err(SourceError::CredentialFile));
}

/// A link to the very file checked, swapped in for it, is refused though it reaches the same
/// inode: the open never follows a link at its last step.
#[test]
fn a_link_to_the_same_file_swapped_in_after_the_check_is_refused() {
    let dir = Fresh::new("swap-same");
    write(&dir.0.join("ca.crt"), "checked", 0o400);
    let swap = |reached: &Path| {
        fs::rename(reached, reached.with_file_name("moved")).unwrap();
        symlink("moved", reached).unwrap();
    };
    assert_eq!(
        read_swapped(&dir.0, "ca.crt", &swap),
        Err(SourceError::CredentialFile)
    );
}

fn token(dir: &Path) -> Result<String, SourceError> {
    read_service_account_token(dir).map(|value| value.expose_secret().to_owned())
}

/// A projected token behind the kubelet's links, readable by all as the kubelet writes it, is
/// read, its trailing newline dropped.
#[test]
fn a_projected_token_readable_by_all_is_read() {
    let dir = Fresh::new("sa-token");
    kubelet(&dir.0, &[("token", "header.claims.signature\n", 0o644)]);
    assert_eq!(token(&dir.0), Ok("header.claims.signature".to_owned()));
}

/// A token anyone else may write is refused, whatever its read bits.
#[test]
fn a_projected_token_others_may_write_is_refused() {
    for mode in [0o646, 0o664, 0o666, 0o662] {
        let dir = Fresh::new("sa-mode");
        kubelet(&dir.0, &[("token", "h.c.s", mode)]);
        assert_eq!(token(&dir.0), Err(SourceError::CredentialFile), "{mode:o}");
    }
}

#[test]
fn a_projected_token_that_is_not_one_line_is_refused() {
    for text in ["", "\n", "h.c.s\nmore", "h.c.s\r\n", "h.c.s\n\n"] {
        let dir = Fresh::new("sa-lines");
        write(&dir.0.join("token"), text, 0o644);
        assert_eq!(token(&dir.0), Err(SourceError::CredentialFile), "{text:?}");
    }
}

/// A token of 16384 bytes is read; one byte more is refused.
#[test]
fn a_projected_token_over_its_bound_is_refused() {
    let dir = Fresh::new("sa-bound");
    let longest = "a".repeat(16384);
    write(&dir.0.join("token"), &longest, 0o644);
    assert_eq!(token(&dir.0), Ok(longest.clone()));
    let over = Fresh::new("sa-over");
    write(&over.0.join("token"), &format!("{longest}a"), 0o644);
    assert_eq!(token(&over.0), Err(SourceError::CredentialFile));
}

/// The token is found only inside its directory: a link out, or no token at all, is refused.
#[test]
fn a_projected_token_outside_its_directory_or_missing_is_refused() {
    let parent = Fresh::new("sa-parent");
    let dir = parent.0.join("mount");
    fs::create_dir(&dir).unwrap();
    write(&parent.0.join("token"), "h.c.s", 0o644);
    symlink("../token", dir.join("token")).unwrap();
    assert_eq!(token(&dir), Err(SourceError::CredentialFile));
    let empty = Fresh::new("sa-missing");
    assert_eq!(token(&empty.0), Err(SourceError::CredentialFile));
}

/// The namespace the downward API projects beside the token is read, by the token's rules, and
/// must be a Kubernetes label of at most 64 bytes.
#[test]
fn the_projected_namespace_is_read_by_the_tokens_rules() {
    let dir = Fresh::new("sa-namespace");
    kubelet(
        &dir.0,
        &[("token", "h.c.s", 0o644), ("namespace", "ws-ws1", 0o644)],
    );
    assert_eq!(
        read_service_account_namespace(&dir.0),
        Ok("ws-ws1".to_owned())
    );
    let longest = format!("a{}", "b".repeat(62));
    for (text, want) in [
        ("ws-ws1\n", Ok("ws-ws1".to_owned())),
        (longest.as_str(), Ok(longest.clone())),
        ("", Err(SourceError::CredentialFile)),
        ("WS-WS1", Err(SourceError::CredentialFile)),
        ("ws_ws1", Err(SourceError::CredentialFile)),
        ("ws-ws1\nmore", Err(SourceError::CredentialFile)),
        ("-ws", Err(SourceError::CredentialFile)),
    ] {
        let dir = Fresh::new("sa-namespace-text");
        write(&dir.0.join("namespace"), text, 0o644);
        assert_eq!(read_service_account_namespace(&dir.0), want, "{text:?}");
    }
    let over = Fresh::new("sa-namespace-over");
    write(&over.0.join("namespace"), &"a".repeat(65), 0o644);
    assert_eq!(
        read_service_account_namespace(&over.0),
        Err(SourceError::CredentialFile)
    );
    let writable = Fresh::new("sa-namespace-mode");
    write(&writable.0.join("namespace"), "ws-ws1", 0o666);
    assert_eq!(
        read_service_account_namespace(&writable.0),
        Err(SourceError::CredentialFile)
    );
    let missing = Fresh::new("sa-namespace-missing");
    assert_eq!(
        read_service_account_namespace(&missing.0),
        Err(SourceError::CredentialFile)
    );
}

const PEM: &str = "-----BEGIN CERTIFICATE-----\nAAAA\nBBBB\n-----END CERTIFICATE-----\n";

/// A CA behind the kubelet's links is read whole at every mode a ConfigMap mounts it with, none
/// of which lets a group or others write: 0444, 0440 under an `fsGroup`, and owner-only (DEC-2340).
#[test]
fn a_ca_is_read_at_every_mode_no_other_may_write() {
    assert_eq!(CA_FILE, "ca.crt");
    for mode in [0o444, 0o440, 0o404, 0o400, 0o644, 0o600] {
        let dir = Fresh::new("ca-public");
        kubelet(&dir.0, &[("ca.crt", PEM, mode)]);
        assert_eq!(read_mounted_ca(&dir.0), Ok(PEM.to_owned()), "{mode:o}");
    }
}

/// A CA a group or others may write is refused, whatever its read bits.
#[test]
fn a_ca_others_may_write_is_refused() {
    for mode in [0o446, 0o466, 0o464, 0o646, 0o664, 0o666, 0o642, 0o622] {
        let dir = Fresh::new("ca-writable");
        kubelet(&dir.0, &[("ca.crt", PEM, mode)]);
        assert_eq!(
            read_mounted_ca(&dir.0),
            Err(SourceError::CredentialFile),
            "{mode:o}"
        );
    }
}

/// The CA reader reads `ca.crt` alone, inside its mount, by the mounted reader's other rules: a
/// missing CA, a link out of the mount, a key beside it, and a file over the bound are refused.
#[test]
fn the_ca_reader_reads_ca_crt_alone_inside_its_mount() {
    let keyed = Fresh::new("ca-keyed");
    kubelet(&keyed.0, &[("tls.key", PEM, 0o444)]);
    assert_eq!(read_mounted_ca(&keyed.0), Err(SourceError::CredentialFile));
    let parent = Fresh::new("ca-parent");
    let dir = parent.0.join("mount");
    fs::create_dir(&dir).unwrap();
    write(&parent.0.join("ca.crt"), PEM, 0o444);
    symlink("../ca.crt", dir.join("ca.crt")).unwrap();
    assert_eq!(read_mounted_ca(&dir), Err(SourceError::CredentialFile));
    let over = Fresh::new("ca-over");
    write(&over.0.join("ca.crt"), &"a".repeat(65537), 0o444);
    assert_eq!(read_mounted_ca(&over.0), Err(SourceError::CredentialFile));
}

/// A secret file keeps the owner-only rule at the modes the CA may have: AppRole credentials, or
/// any file the mounted reader reads, at 0440 (what an `fsGroup` makes of a 0400 Secret), 0444 or
/// 0404 are refused, and read at 0400 and 0600, so a Secret mounted under an `fsGroup` must be
/// copied to an owner-only file first (DEC-2340).
#[test]
fn a_secret_at_a_ca_mode_is_still_refused() {
    for mode in [0o400, 0o600] {
        let dir = Fresh::new("secret-owner");
        kubelet(
            &dir.0,
            &[
                ("executor-c1.role-id", "role-1", mode),
                ("executor-c1.secret-id", "secret-1", mode),
            ],
        );
        assert_eq!(
            credentials(&dir.0, &executor()),
            Ok(("role-1".to_owned(), "secret-1".to_owned())),
            "{mode:o}"
        );
    }
    for mode in [0o440, 0o444, 0o404] {
        let dir = Fresh::new("secret-public");
        kubelet(
            &dir.0,
            &[
                ("executor-c1.role-id", "role-1", mode),
                ("executor-c1.secret-id", "secret-1", mode),
                ("ca.crt", PEM, mode),
            ],
        );
        assert_eq!(
            credentials(&dir.0, &executor()),
            Err(SourceError::CredentialFile),
            "{mode:o}"
        );
        assert_eq!(
            read(&dir.0, "ca.crt"),
            Err(SourceError::CredentialFile),
            "{mode:o}"
        );
    }
}

/// The chart file that mounts the vault's CA into the executor and the token exchange, from the
/// repository's root, which names it to `cargo xtask ci select` too.
const CHART_HELPERS: &str = "deploy/cell/charts/workspace/templates/_helpers.tpl";

/// The `defaultMode` of every volume of `chart` that mounts the ConfigMap `vault-ca`: the line
/// after `configMap:` and `name: vault-ca`, a YAML integer, octal when it starts with `0`.
fn vault_ca_modes(chart: &str) -> Vec<u32> {
    let lines: Vec<&str> = chart.lines().map(str::trim).collect();
    lines
        .windows(3)
        .filter(|w| w[0] == "configMap:" && w[1] == "name: vault-ca")
        .map(|w| {
            let mode = w[2].strip_prefix("defaultMode: ").unwrap();
            match mode.strip_prefix("0o").or(mode.strip_prefix('0')) {
                Some(octal) if !octal.is_empty() => u32::from_str_radix(octal, 8).unwrap(),
                _ => mode.parse().unwrap(),
            }
        })
        .collect()
}

/// The CA reader reads the CA at the mode the workspace chart mounts it with, and at that mode
/// with the group read bit an `fsGroup` adds, so a chart mode the reader refuses fails here, not
/// on a cluster (DEC-2340).
#[test]
fn the_ca_is_read_at_the_mode_the_chart_mounts_it_with() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let chart = fs::read_to_string(root.join(CHART_HELPERS)).unwrap();
    let modes = vault_ca_modes(&chart);
    assert!(!modes.is_empty(), "no vault-ca volume in {CHART_HELPERS}");
    for chart_mode in modes {
        for mode in [chart_mode, chart_mode | 0o040] {
            let dir = Fresh::new("ca-chart");
            kubelet(&dir.0, &[("ca.crt", PEM, mode)]);
            assert_eq!(read_mounted_ca(&dir.0), Ok(PEM.to_owned()), "{mode:o}");
        }
    }
}

/// The chart parser reads each YAML integer form and every `vault-ca` volume, and nothing else.
#[test]
fn the_chart_parser_reads_every_vault_ca_mode() {
    let chart = "    - name: vault-ca\n      configMap:\n        name: vault-ca\n        defaultMode: 292\n\
                 configMap:\n    name: vault-ca\n    defaultMode: 0440\n\
                 configMap:\n    name: other\n    defaultMode: 511\n";
    assert_eq!(vault_ca_modes(chart), vec![0o444, 0o440]);
}
