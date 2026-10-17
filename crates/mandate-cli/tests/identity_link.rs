//! Lane ID1 (DEC-992; workspace API spec §3.11 item 7, DEC-978 item 7): `mandate identity link`
//! adds one line, `sha256:<hex> <ULID>`, to the API server's subject-link file, the digest being
//! the SHA-256 of the canonical `{"issuer","subject"}` object. The subject comes from standard
//! input and is never written, printed, or reported. The file is replaced by a rename, written
//! `0644`, and a file the server would refuse (group- or world-writable, not a regular file) or
//! cannot read is never extended. A link already there succeeds and writes nothing; a subject
//! linked to another principal is refused.
//!
//! The property test runs random links against a model of its own and checks the file's text,
//! read back and parsed here, never through the crate's parser, after every step.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};

use clap::Parser;
use clap::error::ErrorKind;
use mandate_canon::Digest;
use mandate_cli::control::ControlError;
use mandate_cli::identity::{IdentityCommand, LinkArgs, Linked, link};
use mandate_cli::{Cli, Command};
use proptest::prelude::*;

const ISSUER: &str = "https://demo.supabase.co/auth/v1";
const FOUNDER: &str = "01J8Z4M0AD0000000000000AD1";
const OTHER: &str = "01J8Z4M0AD0000000000000AD2";
const SUBJECT: &str = "5b6c1e0a-9a1f-4c07-b2a3-0d3c9e4f7a21";

/// A fresh, absent directory for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-link-{}-{name}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root).unwrap();
    root
}

/// The digest written out here: the canonical object's bytes typed by hand, members in order.
fn digest_of(issuer: &str, subject: &str) -> String {
    let canonical = format!(r#"{{"issuer":"{issuer}","subject":"{subject}"}}"#);
    format!("sha256:{}", Digest::of(canonical.as_bytes()).to_hex())
}

fn args(links: &Path, member: &str) -> LinkArgs {
    LinkArgs {
        issuer: ISSUER.to_owned(),
        member: member.to_owned(),
        links: links.to_owned(),
    }
}

fn run(links: &Path, member: &str, subject: &str) -> (Result<Linked, ControlError>, String) {
    let mut out = Vec::new();
    let got = link(&args(links, member), &mut subject.as_bytes(), &mut out);
    (got, String::from_utf8(out).unwrap())
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The command takes the issuer, the member, and the file, and no subject argument.
#[test]
fn link_takes_no_subject_argument() {
    let line = format!("mandate identity link --issuer {ISSUER} --member {FOUNDER} --links f");
    let full: Vec<&str> = line.split_whitespace().collect();
    let parsed = Cli::try_parse_from(&full).map(|cli| cli.command);
    let Ok(Command::Identity(IdentityCommand::Link(parsed))) = parsed else {
        panic!("{parsed:?}")
    };
    assert_eq!(
        (parsed.issuer.as_str(), parsed.member.as_str()),
        (ISSUER, FOUNDER)
    );
    for (skip, missing) in [(3, "--issuer"), (5, "--member"), (7, "--links")] {
        let given: Vec<&str> = [&full[..skip], &full[skip + 2..]].concat();
        let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::MissingRequiredArgument), "{missing}");
    }
    let given: Vec<&str> = full.iter().copied().chain(["--subject", SUBJECT]).collect();
    let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
    assert_eq!(kind, Some(ErrorKind::UnknownArgument));
}

/// The first link creates the file with exactly its line, `0644`; the report names the digest and
/// the member and never the subject.
#[test]
fn the_first_link_creates_the_file() {
    let dir = scratch("first");
    let links = dir.join("links");
    let (got, printed) = run(&links, FOUNDER, &format!("{SUBJECT}\n"));
    let digest = digest_of(ISSUER, SUBJECT);
    assert_eq!(
        got,
        Ok(Linked {
            digest: digest.clone(),
            written: true
        })
    );
    assert_eq!(
        fs::read_to_string(&links).unwrap(),
        format!("{digest} {FOUNDER}\n")
    );
    assert_eq!(mode(&links), 0o644);
    assert!(
        printed.contains(&digest) && printed.contains(FOUNDER),
        "{printed}"
    );
    assert!(!printed.contains(SUBJECT), "{printed}");
    assert!(!dir.join("links.writing").exists());
}

/// A trailing CRLF is the line's end, not the subject's.
#[test]
fn a_trailing_line_end_is_not_part_of_the_subject() {
    for input in [
        SUBJECT.to_owned(),
        format!("{SUBJECT}\n"),
        format!("{SUBJECT}\r\n"),
    ] {
        let dir = scratch("line-end");
        let (got, _) = run(&dir.join("links"), FOUNDER, &input);
        assert_eq!(
            got.map(|l| l.digest),
            Ok(digest_of(ISSUER, SUBJECT)),
            "{input:?}"
        );
    }
}

/// The same link again writes nothing and succeeds; the subject linked to another principal is
/// refused; a second subject adds its line, ascending by digest.
#[test]
fn links_are_idempotent_and_never_re_pointed() {
    let dir = scratch("again");
    let links = dir.join("links");
    run(&links, FOUNDER, SUBJECT).0.unwrap();
    let before = fs::read_to_string(&links).unwrap();
    let (again, printed) = run(&links, FOUNDER, SUBJECT);
    assert_eq!(again.map(|l| l.written), Ok(false));
    assert!(printed.contains("already"), "{printed}");
    assert_eq!(
        run(&links, OTHER, SUBJECT).0,
        Err(refused("subject_linked_elsewhere"))
    );
    assert_eq!(fs::read_to_string(&links).unwrap(), before);
    run(&links, OTHER, "second-subject").0.unwrap();
    let mut want = [
        format!("{} {FOUNDER}", digest_of(ISSUER, SUBJECT)),
        format!("{} {OTHER}", digest_of(ISSUER, "second-subject")),
    ];
    want.sort();
    assert_eq!(
        fs::read_to_string(&links).unwrap(),
        format!("{}\n{}\n", want[0], want[1])
    );
}

/// A bad member is refused before standard input is read; a bad issuer or subject is refused, and
/// nothing is written.
#[test]
fn bad_input_writes_nothing() {
    struct Untouched;
    impl Read for Untouched {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("standard input was read")
        }
    }
    let dir = scratch("bad");
    let links = dir.join("links");
    let mut out = Vec::new();
    for member in ["not-a-ulid", "", "01j8z4m0ad0000000000000ad1"] {
        let got = link(&args(&links, member), &mut Untouched, &mut out);
        assert_eq!(got, Err(refused("member_invalid")), "{member:?}");
    }
    let long = "x".repeat(1025);
    for subject in ["", "\n", "a\u{0}b", "a\tb", long.as_str()] {
        assert_eq!(
            run(&links, FOUNDER, subject).0,
            Err(refused("subject_invalid")),
            "{subject:?}"
        );
    }
    assert!(
        run(&links, FOUNDER, &"x".repeat(1024)).0.is_ok(),
        "1024 bytes is the bound"
    );
    fs::remove_file(&links).unwrap();
    for issuer in ["", "https://a b", "https://a\n"] {
        let mut given = args(&links, FOUNDER);
        given.issuer = issuer.to_owned();
        let got = link(&given, &mut SUBJECT.as_bytes(), &mut out);
        assert_eq!(got, Err(refused("issuer_invalid")), "{issuer:?}");
    }
    assert!(!links.exists());
    assert!(out.is_empty(), "a refusal prints nothing");
}

/// A file the server would refuse, or one this command cannot read as links, is never extended.
#[test]
fn an_unsafe_or_malformed_file_is_never_extended() {
    let dir = scratch("unsafe");
    let links = dir.join("links");
    let line = format!("{} {FOUNDER}\n", digest_of(ISSUER, "kept"));
    for writable in [0o664, 0o646, 0o666] {
        fs::write(&links, &line).unwrap();
        fs::set_permissions(&links, fs::Permissions::from_mode(writable)).unwrap();
        assert_eq!(run(&links, OTHER, SUBJECT).0, Err(refused("links_unsafe")));
        assert_eq!(fs::read_to_string(&links).unwrap(), line);
        fs::remove_file(&links).unwrap();
    }
    let target = dir.join("target");
    fs::write(&target, &line).unwrap();
    symlink(&target, &links).unwrap();
    assert_eq!(run(&links, OTHER, SUBJECT).0, Err(refused("links_unsafe")));
    fs::remove_file(&links).unwrap();
    fs::create_dir(&links).unwrap();
    assert_eq!(run(&links, OTHER, SUBJECT).0, Err(refused("links_unsafe")));
    fs::remove_dir(&links).unwrap();
    let digest = digest_of(ISSUER, "kept");
    let malformed = [
        format!("{digest} {FOUNDER}"),
        format!("{digest}  {FOUNDER}\n"),
        format!("{} {FOUNDER}\n", digest.to_uppercase()),
        format!("{digest} not-a-ulid\n"),
        format!("{digest} {FOUNDER}\n{digest} {OTHER}\n"),
        format!("sha256:abc {FOUNDER}\n"),
        "\n".to_owned(),
    ];
    for text in malformed {
        fs::write(&links, &text).unwrap();
        fs::set_permissions(&links, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            run(&links, OTHER, SUBJECT).0,
            Err(refused("links_malformed")),
            "{text:?}"
        );
        assert_eq!(fs::read_to_string(&links).unwrap(), text);
    }
    assert!(!dir.join("links.writing").exists());
}

/// A link already being written is refused, and the other writer's staging file is left alone.
#[test]
fn a_second_writer_at_once_is_refused() {
    let dir = scratch("busy");
    let links = dir.join("links");
    fs::write(dir.join("links.writing"), "theirs").unwrap();
    assert_eq!(run(&links, FOUNDER, SUBJECT).0, Err(refused("links_busy")));
    assert!(!links.exists());
    assert_eq!(
        fs::read_to_string(dir.join("links.writing")).unwrap(),
        "theirs"
    );
}

/// A file written by an earlier link at a stricter mode is still extended, and left `0644`.
#[test]
fn an_owner_only_file_is_extended() {
    let dir = scratch("strict");
    let links = dir.join("links");
    run(&links, FOUNDER, SUBJECT).0.unwrap();
    fs::set_permissions(&links, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(run(&links, OTHER, "second").0.unwrap().written);
    assert_eq!(mode(&links), 0o644);
}

/// The binary reads the subject from standard input and never shows it.
#[test]
fn the_binary_reads_the_subject_from_standard_input() {
    let dir = scratch("binary");
    let links = dir.join("links");
    let mut child = Process::new(env!("CARGO_BIN_EXE_mandate"))
        .args([
            "identity", "link", "--issuer", ISSUER, "--member", FOUNDER, "--links",
        ])
        .arg(&links)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{SUBJECT}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{stderr}");
    assert!(!stdout.contains(SUBJECT) && !stderr.contains(SUBJECT));
    let text = fs::read_to_string(&links).unwrap();
    assert_eq!(text, format!("{} {FOUNDER}\n", digest_of(ISSUER, SUBJECT)));
    assert!(!text.contains(SUBJECT));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Random links against a model: each answers as the model says, and the file always holds
    /// exactly the model's links, one line each, ascending by digest, `0644`, and never a subject.
    #[test]
    fn the_file_always_holds_the_models_links(ops in prop::collection::vec((0_u8..4, 1_u8..4), 1..12)) {
        let dir = scratch("property");
        let links = dir.join("links");
        let mut model: BTreeMap<String, String> = BTreeMap::new();
        for (subject, member) in ops {
            let subject = format!("subject-{subject}");
            let member = format!("01J8Z4M0AD0000000000000AD{member}");
            let digest = digest_of(ISSUER, &subject);
            let want = match model.get(&digest) {
                Some(linked) if *linked == member => Ok(false),
                Some(_) => Err(refused("subject_linked_elsewhere")),
                None => {
                    model.insert(digest.clone(), member.clone());
                    Ok(true)
                }
            };
            let got = run(&links, &member, &subject).0.map(|l| l.written);
            prop_assert_eq!(got, want);
            let text = fs::read_to_string(&links).unwrap_or_default();
            let lines: Vec<String> = model.iter().map(|(d, m)| format!("{d} {m}\n")).collect();
            prop_assert_eq!(&text, &lines.concat());
            prop_assert!(!text.contains("subject-"));
            if links.exists() {
                prop_assert_eq!(mode(&links), 0o644);
            }
        }
        fs::remove_dir_all(dir).ok();
    }
}
