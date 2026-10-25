//! Credential files (DEC-2022): each identity's two files owner-only, an earlier file replaced and
//! tightened, a directory or token open to others refused, against names written out by hand.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use mandate_connections::record::ConnectionId;
use proptest::prelude::*;
use secrecy::{ExposeSecret, SecretString};

use crate::SourceError;
use crate::credentials::{check_credential_dir, read_token, read_token_file, write_credentials};
use crate::openbao::RoleCredentials;
use crate::policy::Identity;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A fresh path under the temporary directory that does not exist yet.
fn fresh(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "vault-credentials-{}-{n}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    let _ = fs::remove_file(&path);
    path
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

fn set_mode(path: &Path, bits: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).unwrap();
}

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn credential(identity: Identity, role_id: &str, secret_id: &str) -> RoleCredentials {
    RoleCredentials {
        identity,
        role_id: role_id.to_owned(),
        secret_id: SecretString::from(secret_id.to_owned()),
    }
}

/// The oracle's role name, written out by hand rather than read from [`crate::policy::role_name`].
fn oracle_name(identity: &Identity) -> String {
    match identity {
        Identity::Api => "api".to_owned(),
        Identity::ConnectionManager => "connection-manager".to_owned(),
        Identity::TokenExchange(c) => format!("exchange-{}", c.as_str()),
        Identity::Executor(c) => format!("executor-{}", c.as_str()),
        Identity::Runtime => "runtime".to_owned(),
        Identity::SecretImport => "secret-import".to_owned(),
        Identity::ModelGateway => "model-gateway".to_owned(),
        Identity::EmailSender => "email-sender".to_owned(),
        Identity::Provisioner => "provisioner".to_owned(),
    }
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn each_identitys_two_files_are_written_owner_only() {
    let dir = fresh("out");
    let written = write_credentials(
        &dir,
        &[
            credential(Identity::Api, "role-a", "secret-a"),
            credential(Identity::Executor(c("c1")), "role-e", "secret-e"),
            credential(Identity::Runtime, "role-r", "secret-r"),
        ],
    )
    .unwrap();
    let want = [
        "api.role-id",
        "api.secret-id",
        "executor-c1.role-id",
        "executor-c1.secret-id",
        "runtime.role-id",
        "runtime.secret-id",
    ];
    assert_eq!(written, want);
    assert_eq!(listing(&dir), want);
    assert_eq!(mode(&dir), 0o700);
    for (name, value) in [
        ("api.role-id", "role-a"),
        ("api.secret-id", "secret-a"),
        ("executor-c1.role-id", "role-e"),
        ("executor-c1.secret-id", "secret-e"),
        ("runtime.role-id", "role-r"),
        ("runtime.secret-id", "secret-r"),
    ] {
        let file = dir.join(name);
        assert_eq!(fs::read_to_string(&file).unwrap(), value, "{name}");
        assert_eq!(mode(&file), 0o600, "{name}");
    }
}

#[test]
fn an_earlier_file_is_replaced_and_tightened() {
    let dir = fresh("again");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    let earlier = dir.join("api.secret-id");
    fs::write(&earlier, "old-secret").unwrap();
    set_mode(&earlier, 0o644);
    write_credentials(&dir, &[credential(Identity::Api, "role-a", "new-secret")]).unwrap();
    assert_eq!(fs::read_to_string(&earlier).unwrap(), "new-secret");
    assert_eq!(mode(&earlier), 0o600);
}

#[test]
fn a_directory_open_to_others_or_linked_is_refused_and_nothing_is_written() {
    let one = [credential(Identity::Api, "role-a", "secret-a")];
    for bits in [0o750, 0o705, 0o770, 0o755, 0o500, 0o300] {
        let dir = fresh("open");
        fs::create_dir(&dir).unwrap();
        set_mode(&dir, bits);
        assert_eq!(
            write_credentials(&dir, &one),
            Err(SourceError::CredentialFile),
            "{bits:o}"
        );
        set_mode(&dir, 0o700);
        assert!(listing(&dir).is_empty(), "{bits:o}");
    }
    let real = fresh("real");
    fs::create_dir(&real).unwrap();
    set_mode(&real, 0o700);
    let link = fresh("link");
    symlink(&real, &link).unwrap();
    assert_eq!(
        write_credentials(&link, &one),
        Err(SourceError::CredentialFile)
    );
    assert!(listing(&real).is_empty());
    let file = fresh("file");
    fs::write(&file, "").unwrap();
    assert_eq!(
        write_credentials(&file, &one),
        Err(SourceError::CredentialFile)
    );
}

#[test]
fn an_owner_only_token_is_read_without_its_newline() {
    for (text, want) in [("tok-1\n", "tok-1"), ("tok-2", "tok-2")] {
        let file = fresh("token");
        fs::write(&file, text).unwrap();
        set_mode(&file, 0o600);
        assert_eq!(read_token_file(&file).unwrap().expose_secret(), want);
        set_mode(&file, 0o400);
        assert_eq!(read_token_file(&file).unwrap().expose_secret(), want);
    }
}

#[test]
fn a_token_open_to_others_missing_empty_or_linked_is_refused() {
    let refused = |path: &Path| {
        let read = read_token_file(path).map(|t| t.expose_secret().to_owned());
        assert_eq!(read, Err(SourceError::CredentialFile), "{}", path.display());
    };
    for bits in [0o640, 0o604, 0o644, 0o660, 0o606] {
        let file = fresh("open-token");
        fs::write(&file, "tok").unwrap();
        set_mode(&file, bits);
        refused(&file);
    }
    for text in ["", "\n", "tok\n\n", "tok\nmore", "tok\r\n", "tok\r"] {
        let file = fresh("bad-token");
        fs::write(&file, text).unwrap();
        set_mode(&file, 0o600);
        refused(&file);
    }
    refused(&fresh("missing"));
    let target = fresh("target");
    fs::write(&target, "tok").unwrap();
    set_mode(&target, 0o600);
    let link = fresh("token-link");
    symlink(&target, &link).unwrap();
    refused(&link);
    let dir = fresh("token-dir");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    refused(&dir);
}

/// A symbolic link planted at an output name is refused, not followed: the link stays, its target
/// is neither read nor written, and nothing else is written (DEC-2022 item 1).
#[test]
fn a_link_planted_at_an_output_name_is_refused_and_its_target_untouched() {
    let dir = fresh("planted");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    let target = fresh("target-of-link");
    fs::write(&target, "someone-elses-file").unwrap();
    set_mode(&target, 0o644);
    let planted = dir.join("api.secret-id");
    symlink(&target, &planted).unwrap();
    let written = write_credentials(&dir, &[credential(Identity::Api, "role-a", "secret-a")]);
    assert_eq!(written, Err(SourceError::CredentialFile));
    assert_eq!(fs::read_to_string(&target).unwrap(), "someone-elses-file");
    assert_eq!(mode(&target), 0o644);
    assert!(
        fs::symlink_metadata(&planted)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(listing(&dir), ["api.secret-id"]);
}

/// A hard link planted at an output name, which the pre-check sees as a plain file, is replaced
/// rather than written through: the file it shares stays as it was (DEC-2022 item 1).
#[test]
fn a_hard_link_planted_at_an_output_name_is_replaced_not_written_through() {
    let dir = fresh("hard-linked");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    let shared = fresh("shared-by-hard-link");
    fs::write(&shared, "someone-elses-file").unwrap();
    set_mode(&shared, 0o644);
    let planted = dir.join("api.secret-id");
    fs::hard_link(&shared, &planted).unwrap();
    let written = write_credentials(&dir, &[credential(Identity::Api, "role-a", "secret-a")]);
    assert_eq!(
        written,
        Ok(vec!["api.role-id".to_owned(), "api.secret-id".to_owned()])
    );
    assert_eq!(fs::read_to_string(&shared).unwrap(), "someone-elses-file");
    assert_eq!(mode(&shared), 0o644);
    assert_eq!(fs::read_to_string(&planted).unwrap(), "secret-a");
    assert_eq!(mode(&planted), 0o600);
    assert_eq!(fs::symlink_metadata(&planted).unwrap().nlink(), 1);
    assert_eq!(listing(&dir), ["api.role-id", "api.secret-id"]);
}

/// A temporary file a crashed write left at an output's name is removed by the next write; one
/// that is not a plain file, and files only like it, are left as they are (DEC-2022 item 1).
#[test]
fn a_crashed_writes_temporary_file_is_removed_by_the_next_write() {
    let dir = fresh("crashed");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    fs::write(dir.join(".api.secret-id.4242.0.tmp"), "secret-left").unwrap();
    fs::create_dir(dir.join(".api.secret-id.held.tmp")).unwrap();
    fs::write(dir.join(".api.secret-id.keep"), "kept").unwrap();
    fs::write(dir.join("notes.tmp"), "kept").unwrap();
    let written = write_credentials(&dir, &[credential(Identity::Api, "role-a", "secret-a")]);
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(
        listing(&dir),
        [
            ".api.secret-id.held.tmp",
            ".api.secret-id.keep",
            "api.role-id",
            "api.secret-id",
            "notes.tmp"
        ]
    );
}

/// A temporary name something else holds is passed over for the next; when every attempt's name
/// is held, the write is refused (DEC-2022 item 1).
#[test]
fn a_held_temporary_name_is_passed_over_until_none_is_left() {
    let held = |dir: &Path, attempts: u32| {
        fs::create_dir(dir).unwrap();
        set_mode(dir, 0o700);
        for attempt in 0..attempts {
            let name = format!(".api.secret-id.{}.{attempt}.tmp", std::process::id());
            fs::create_dir(dir.join(name)).unwrap();
        }
    };
    let one = [credential(Identity::Api, "role-a", "secret-a")];
    let dir = fresh("held-one");
    held(&dir, 1);
    assert!(write_credentials(&dir, &one).is_ok());
    assert_eq!(
        fs::read_to_string(dir.join("api.secret-id")).unwrap(),
        "secret-a"
    );
    let dir = fresh("held-all");
    held(&dir, 8);
    assert_eq!(
        write_credentials(&dir, &one),
        Err(SourceError::CredentialFile)
    );
    assert!(!dir.join("api.secret-id").exists());
}

/// A `dir` whose parent others may write without the sticky bit is refused, since they could
/// rename it away; a sticky parent, as the temporary directory is, or an owner-only one is not
/// (DEC-2022 item 1).
#[test]
fn a_parent_others_may_write_without_the_sticky_bit_is_refused() {
    let one = [credential(Identity::Api, "role-a", "secret-a")];
    for (bits, refused) in [
        (0o777, true),
        (0o775, true),
        (0o757, true),
        (0o1777, false),
        (0o755, false),
        (0o700, false),
    ] {
        let parent = fresh("parent");
        fs::create_dir(&parent).unwrap();
        set_mode(&parent, bits);
        let dir = parent.join("out");
        let written = write_credentials(&dir, &one);
        assert_eq!(written.is_err(), refused, "a parent of mode {bits:o}");
        assert_eq!(dir.exists(), !refused, "a parent of mode {bits:o}");
        set_mode(&parent, 0o700);
    }
}

/// The names of the files `write_credentials` writes for a provisioning of `c1`, written out by
/// hand: each process type's and `c1`'s executor's, never its token exchange's, whose secret id is
/// a grant.
fn oracle_names_for_c1() -> Vec<String> {
    [
        "api",
        "connection-manager",
        "runtime",
        "secret-import",
        "model-gateway",
        "email-sender",
        "executor-c1",
    ]
    .iter()
    .flat_map(|role| [format!("{role}.role-id"), format!("{role}.secret-id")])
    .collect()
}

/// An absent `dir` passes and is not made; an owner-only one passes whether it is empty or holds
/// earlier plain files at the output names (DEC-2022 items 1 and 2).
#[test]
fn an_absent_or_owner_only_dir_passes_before_any_vault_and_nothing_is_made() {
    let connections = [c("c1")];
    let dir = fresh("checked-absent");
    assert_eq!(check_credential_dir(&dir, &connections), Ok(()));
    assert!(!dir.exists());
    let dir = fresh("checked-empty");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    assert_eq!(check_credential_dir(&dir, &connections), Ok(()));
    assert!(listing(&dir).is_empty());
    for name in oracle_names_for_c1() {
        fs::write(dir.join(name), "earlier").unwrap();
    }
    assert_eq!(check_credential_dir(&dir, &connections), Ok(()));
}

/// Every `dir` or parent `write_credentials` refuses is refused before any vault is reached, and
/// nothing is made or changed (DEC-2022 items 1 and 2).
#[test]
fn a_dir_or_parent_the_write_refuses_is_refused_before_any_vault() {
    let connections = [c("c1")];
    for bits in [0o755, 0o750, 0o705, 0o500, 0o300] {
        let dir = fresh("checked-open");
        fs::create_dir(&dir).unwrap();
        set_mode(&dir, bits);
        let checked = check_credential_dir(&dir, &connections);
        set_mode(&dir, 0o700);
        assert_eq!(
            checked,
            Err(SourceError::CredentialFile),
            "a dir of mode {bits:o}"
        );
        assert!(listing(&dir).is_empty(), "a dir of mode {bits:o}");
    }
    let real = fresh("checked-real");
    fs::create_dir(&real).unwrap();
    set_mode(&real, 0o700);
    let linked = fresh("checked-linked");
    symlink(&real, &linked).unwrap();
    assert_eq!(
        check_credential_dir(&linked, &connections),
        Err(SourceError::CredentialFile)
    );
    let file = fresh("checked-file");
    fs::write(&file, "not a directory").unwrap();
    assert_eq!(
        check_credential_dir(&file, &connections),
        Err(SourceError::CredentialFile)
    );
    let parent = fresh("checked-parent");
    fs::create_dir(&parent).unwrap();
    set_mode(&parent, 0o777);
    let checked = check_credential_dir(&parent.join("out"), &connections);
    set_mode(&parent, 0o700);
    assert_eq!(checked, Err(SourceError::CredentialFile));
    assert!(!parent.join("out").exists());
}

/// A link or a directory planted at any name the write would replace is refused before any vault;
/// one at a token exchange's name, which is never written, is not (DEC-2022 items 1 and 2).
#[test]
fn anything_but_a_plain_file_at_a_credential_name_is_refused_before_any_vault() {
    let connections = [c("c1")];
    let target = fresh("checked-target");
    fs::write(&target, "someone-elses-file").unwrap();
    for name in oracle_names_for_c1() {
        for plant in ["link", "directory"] {
            let dir = fresh("checked-planted");
            fs::create_dir(&dir).unwrap();
            set_mode(&dir, 0o700);
            if plant == "link" {
                symlink(&target, dir.join(&name)).unwrap();
            } else {
                fs::create_dir(dir.join(&name)).unwrap();
            }
            assert_eq!(
                check_credential_dir(&dir, &connections),
                Err(SourceError::CredentialFile),
                "a {plant} at {name}"
            );
        }
    }
    let dir = fresh("checked-exchange");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    symlink(&target, dir.join("exchange-c1.role-id")).unwrap();
    fs::create_dir(dir.join("exchange-c1.secret-id")).unwrap();
    assert_eq!(check_credential_dir(&dir, &connections), Ok(()));
    assert_eq!(fs::read_to_string(&target).unwrap(), "someone-elses-file");
}

/// A refusal changes no file: every output name is checked before any is written, so an earlier
/// file stays as it was when a later name cannot be written (DEC-2022 item 1).
#[test]
fn a_refusal_leaves_every_earlier_file_as_it_was() {
    let dir = fresh("partial");
    fs::create_dir(&dir).unwrap();
    set_mode(&dir, 0o700);
    let earlier = dir.join("api.secret-id");
    fs::write(&earlier, "old-secret").unwrap();
    set_mode(&earlier, 0o600);
    fs::create_dir(dir.join("runtime.role-id")).unwrap();
    let written = write_credentials(
        &dir,
        &[
            credential(Identity::Api, "role-a", "new-secret"),
            credential(Identity::Runtime, "role-r", "secret-r"),
        ],
    );
    assert_eq!(written, Err(SourceError::CredentialFile));
    assert_eq!(fs::read_to_string(&earlier).unwrap(), "old-secret");
    assert_eq!(listing(&dir), ["api.secret-id", "runtime.role-id"]);
}

/// Neither the refusal nor a credential's `Debug` shows the secret it holds.
#[test]
fn no_debug_shows_a_credential_or_its_refusal() {
    let held = credential(Identity::Api, "role-a", "canary-77d0-secret-id");
    for output in [
        format!("{held:?}"),
        format!("{:?}", SourceError::CredentialFile),
    ] {
        assert!(!output.contains("canary-77d0"), "{output}");
    }
    assert!(!SourceError::CredentialFile.to_string().contains('/'));
}

fn identity() -> impl Strategy<Value = Identity> {
    let connection = "c[a-z0-9]{0,5}".prop_map(|id| c(&id));
    prop_oneof![
        Just(Identity::Api),
        Just(Identity::ConnectionManager),
        Just(Identity::Runtime),
        Just(Identity::SecretImport),
        Just(Identity::ModelGateway),
        Just(Identity::EmailSender),
        Just(Identity::Provisioner),
        connection.clone().prop_map(Identity::Executor),
        connection.prop_map(Identity::TokenExchange),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// For any set of identities, the files written are exactly two per identity under the
    /// oracle's names, each holding its value and owner-only.
    #[test]
    fn the_files_are_exactly_two_per_identity_under_the_oracles_names(
        identities in proptest::collection::vec(identity(), 0..6),
        value in "[a-z0-9-]{1,24}",
    ) {
        let mut unique: Vec<Identity> = Vec::new();
        for i in identities {
            if !unique.contains(&i) {
                unique.push(i);
            }
        }
        let credentials: Vec<RoleCredentials> = unique
            .iter()
            .map(|i| credential(i.clone(), &format!("r-{value}"), &format!("s-{value}")))
            .collect();
        let dir = fresh("prop");
        let written = write_credentials(&dir, &credentials);
        let mut want: Vec<String> = unique
            .iter()
            .flat_map(|i| [format!("{}.role-id", oracle_name(i)), format!("{}.secret-id", oracle_name(i))])
            .collect();
        want.sort();
        prop_assert_eq!(written.clone(), Ok(want.clone()), "{:?}", written);
        prop_assert_eq!(listing(&dir), want.clone());
        for name in &want {
            let file = dir.join(name);
            let expected = if name.ends_with(".role-id") { format!("r-{value}") } else { format!("s-{value}") };
            prop_assert_eq!(fs::read_to_string(&file).unwrap(), expected);
            prop_assert_eq!(mode(&file), 0o600);
        }
    }
}

fn token_read(input: &[u8]) -> Result<String, SourceError> {
    read_token(&mut &input[..]).map(|token| token.expose_secret().to_owned())
}

/// A piped token is read by the file's rules: the token alone, or with one trailing newline.
#[test]
fn a_piped_token_is_read_with_at_most_one_trailing_newline() {
    assert_eq!(token_read(b"s.abc"), Ok("s.abc".to_owned()));
    assert_eq!(token_read(b"s.abc\n"), Ok("s.abc".to_owned()));
}

/// An empty pipe, a second line, or a carriage return anywhere is refused.
#[test]
fn a_piped_token_that_is_empty_or_not_one_line_is_refused() {
    for input in [
        &b""[..],
        b"\n",
        b"s.abc\n\n",
        b"s.abc\nmore",
        b"s.abc\r\n",
        b"s.a\rbc",
        b"\ns.abc",
    ] {
        assert_eq!(
            token_read(input),
            Err(SourceError::CredentialFile),
            "{:?}",
            String::from_utf8_lossy(input)
        );
    }
}

/// A pipe holding more than 4096 bytes is refused, newline or not; 4096 is read.
#[test]
fn a_piped_token_longer_than_its_bound_is_refused() {
    let longest = "a".repeat(4096);
    assert_eq!(token_read(longest.as_bytes()), Ok(longest.clone()));
    let with_newline = format!("{}\n", "a".repeat(4095));
    assert_eq!(token_read(with_newline.as_bytes()), Ok("a".repeat(4095)));
    for input in ["a".repeat(4097), format!("{longest}\n")] {
        assert_eq!(
            token_read(input.as_bytes()).map(|token| token.len()),
            Err(SourceError::CredentialFile)
        );
    }
}

/// A pipe that is not UTF-8 is refused.
#[test]
fn a_piped_token_that_is_not_utf8_is_refused() {
    assert_eq!(token_read(b"s.\xffabc"), Err(SourceError::CredentialFile));
}
