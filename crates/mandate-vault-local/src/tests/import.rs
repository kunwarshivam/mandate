//! `mandate vault import` reads the client secret from stdin only, strips one line ending,
//! refuses anything but printable ASCII, and stores it as the executor (DEC-692 item 6).

use std::io::{self, Read};

use secrecy::ExposeSecret;

use super::fixtures::client_secret;
use super::layout::Layout;
use crate::VaultError;
use crate::import::{API_USER, EXEC_USER, ROOT_USER, import_client_secret, resolve_ids};
use crate::startup::Ids;
use crate::store::{ExecutorVault, MAX_SECRET_LEN};

fn stored(executor: &ExecutorVault) -> Option<Vec<u8>> {
    executor
        .read(&client_secret())
        .unwrap()
        .map(|s| s.expose_secret().to_vec())
}

#[test]
fn the_secret_is_stored_without_its_line_ending() {
    for (input, want) in [
        (&b"s3cr3t-Value_~!"[..], &b"s3cr3t-Value_~!"[..]),
        (b"abc\n", b"abc"),
        (b"abc\r\n", b"abc"),
    ] {
        let layout = Layout::new();
        let (_, executor) = layout.both();
        import_client_secret(&executor, &mut &input[..]).unwrap();
        assert_eq!(stored(&executor).as_deref(), Some(want), "{input:?}");
    }
}

#[test]
fn an_import_replaces_the_earlier_secret() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    import_client_secret(&executor, &mut &b"first"[..]).unwrap();
    import_client_secret(&executor, &mut &b"second\n"[..]).unwrap();
    assert_eq!(stored(&executor).as_deref(), Some(&b"second"[..]));
}

#[test]
fn anything_but_one_printable_line_is_refused_and_nothing_is_stored() {
    let max = vec![b'k'; MAX_SECRET_LEN];
    let mut max_nl = max.clone();
    max_nl.push(b'\n');
    let mut over = max.clone();
    over.push(b'k');
    let cases: [(&[u8], VaultError); 11] = [
        (b"", VaultError::SecretSize),
        (b"\n", VaultError::SecretSize),
        (b"\r\n", VaultError::SecretSize),
        (&over, VaultError::SecretSize),
        (b"two words", VaultError::SecretNotPrintable),
        (b"abc\n\n", VaultError::SecretNotPrintable),
        (b"abc\r", VaultError::SecretNotPrintable),
        (b"a\tb", VaultError::SecretNotPrintable),
        (b"a\x7fb", VaultError::SecretNotPrintable),
        (b"\xc3\xa9", VaultError::SecretNotPrintable),
        (b"a\0", VaultError::SecretNotPrintable),
    ];
    for (input, error) in cases {
        let layout = Layout::new();
        let (_, executor) = layout.both();
        assert_eq!(
            import_client_secret(&executor, &mut &input[..]),
            Err(error.clone()),
            "{} bytes",
            input.len()
        );
        assert!(layout.names("tokens").is_empty());
    }
    for ok in [max, max_nl] {
        let layout = Layout::new();
        let (_, executor) = layout.both();
        import_client_secret(&executor, &mut &ok[..]).unwrap();
        assert_eq!(stored(&executor).map(|v| v.len()), Some(MAX_SECRET_LEN));
    }
}

/// A source that never ends: the import reads a bounded prefix and refuses it.
struct Endless;

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(b'z');
        Ok(buf.len())
    }
}

/// A source that never ends and counts what it gave.
struct Counted(usize);

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(1000);
        buf[..n].fill(b'z');
        self.0 += n;
        Ok(n)
    }
}

#[test]
fn the_import_reads_no_more_than_one_byte_past_the_longest_line() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    let mut source = Counted(0);
    assert_eq!(
        import_client_secret(&executor, &mut source),
        Err(VaultError::SecretSize)
    );
    assert_eq!(
        source.0,
        MAX_SECRET_LEN + 3,
        "the secret, a \\r\\n, and one byte more"
    );
}

/// A source whose read fails.
struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("broken pipe"))
    }
}

#[test]
fn an_endless_or_broken_stdin_is_refused() {
    let layout = Layout::new();
    let (_, executor) = layout.both();
    assert_eq!(
        import_client_secret(&executor, &mut Endless),
        Err(VaultError::SecretSize)
    );
    assert_eq!(
        import_client_secret(&executor, &mut Broken),
        Err(VaultError::Io)
    );
    assert!(layout.names("tokens").is_empty());
}

const PASSWD: &str = "rootless:x:7:7::/:/bin/false
root:x:0:0:root:/root:/bin/bash
owlhead_api2:x:1500:1500::/nonexistent:/usr/sbin/nologin
owlhead_api:x:998:997::/nonexistent:/usr/sbin/nologin
broken line
owlhead_exec:x:999:996::/nonexistent:/usr/sbin/nologin
owlhead_exec:x:1234:1234::/nonexistent:/usr/sbin/nologin
";

const GROUP: &str = "root:x:0:
owlhead_api:x:997:
owlhead_exec:x:996:owlhead_api_not_a_member
";

#[test]
fn the_ids_come_from_the_named_users_and_groups() {
    assert_eq!(
        resolve_ids(PASSWD, GROUP),
        Ok(Ids {
            root_uid: 0,
            api_uid: 998,
            api_gid: 997,
            exec_uid: 999,
            exec_gid: 996,
        })
    );
}

#[test]
fn a_missing_or_unreadable_user_or_group_is_refused() {
    let without = |text: &str, name: &str| -> String {
        text.lines()
            .filter(|l| !l.starts_with(&format!("{name}:")))
            .map(|l| format!("{l}\n"))
            .collect()
    };
    for name in [API_USER, EXEC_USER] {
        assert_eq!(
            resolve_ids(&without(PASSWD, name), GROUP),
            Err(VaultError::UnknownUser {
                name: if name == API_USER {
                    API_USER
                } else {
                    EXEC_USER
                }
            }),
            "passwd without {name}"
        );
        assert_eq!(
            resolve_ids(PASSWD, &without(GROUP, name)),
            Err(VaultError::UnknownUser {
                name: if name == API_USER {
                    API_USER
                } else {
                    EXEC_USER
                }
            }),
            "group without {name}"
        );
    }
    assert_eq!(
        resolve_ids(&without(PASSWD, ROOT_USER), GROUP),
        Err(VaultError::UnknownUser { name: ROOT_USER }),
        "passwd without root"
    );
    let bad_uid = PASSWD.replace("owlhead_api:x:998:", "owlhead_api:x:-1:");
    assert_eq!(
        resolve_ids(&bad_uid, GROUP),
        Err(VaultError::UnknownUser { name: API_USER })
    );
    let bad_gid = GROUP.replace("owlhead_exec:x:996:", "owlhead_exec:x::");
    assert_eq!(
        resolve_ids(PASSWD, &bad_gid),
        Err(VaultError::UnknownUser { name: EXEC_USER })
    );
}
