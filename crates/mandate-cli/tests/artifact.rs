//! `mandate artifact put` and `mandate artifact get`: the reference a put prints, the bytes a get
//! writes, the re-hash on the way out, and the stable codes of journal spec §11 check 6 behind a
//! non-zero exit (backlog E5-4).
//!
//! Oracles: the FIPS 180-2 SHA-256 vectors for the address a put must print, and the store's own
//! on-disk object read directly for what a get must return.

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_cli::artifact::{self, ArtifactCommand, GetArgs, PutArgs};
use mandate_cli::{Cli, Command};
use mandate_journal::ArtifactRef;

/// FIPS 180-2 SHA-256 test vectors.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mandate-cli-artifact-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Overwrites the object for `reference` with `bytes`, as a disk fault or an intruder would.
fn tamper(store: &FsArtifactStore, reference: &ArtifactRef, bytes: &[u8]) {
    let path = store.object_path(reference);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test plays a writer that ignores the store's read-only mode"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    fs::write(&path, bytes).unwrap();
}

fn parse_put(argv: &[&str]) -> Result<PutArgs, clap::Error> {
    let mut all = vec!["mandate", "artifact", "put"];
    all.extend_from_slice(argv);
    match Cli::try_parse_from(all)?.command {
        Command::Artifact(ArtifactCommand::Put(args)) => Ok(args),
        other => unreachable!("{other:?} is not artifact put"),
    }
}

fn parse_get(argv: &[&str]) -> Result<GetArgs, clap::Error> {
    let mut all = vec!["mandate", "artifact", "get"];
    all.extend_from_slice(argv);
    match Cli::try_parse_from(all)?.command {
        Command::Artifact(ArtifactCommand::Get(args)) => Ok(args),
        other => unreachable!("{other:?} is not artifact get"),
    }
}

/// Puts `bytes` written to a file, returning the reference and what the command printed.
fn put(scratch: &Scratch, name: &str, bytes: &[u8], store: &str) -> (ArtifactRef, String) {
    let file = scratch.file(name, bytes);
    let args = parse_put(&[file.as_str(), "--store", store]).unwrap();
    let mut report = Vec::new();
    let reference = artifact::put(&args, &mut report).unwrap();
    (reference, String::from_utf8(report).unwrap())
}

/// Gets `reference`, returning the bytes written out.
fn get(reference: &str, store: &str) -> anyhow::Result<Vec<u8>> {
    let args = parse_get(&[reference, "--store", store]).unwrap();
    let mut out = Vec::new();
    artifact::get(&args, &mut out)?;
    Ok(out)
}

#[test]
fn put_prints_sha256_of_the_stored_bytes() {
    let scratch = Scratch::new("address");
    let store = scratch.path("store");
    for (name, bytes, hex) in [
        ("empty", b"".as_slice(), EMPTY_SHA256),
        ("abc", b"abc".as_slice(), ABC_SHA256),
    ] {
        let (reference, printed) = put(&scratch, name, bytes, &store);
        assert_eq!(reference.to_string(), format!("sha256:{hex}"), "{name}");
        assert_eq!(printed, format!("sha256:{hex}\n"), "{name}");
    }
}

#[test]
fn put_creates_the_store_and_get_reads_the_bytes_back() {
    let scratch = Scratch::new("round-trip");
    let store = scratch.path("store");
    assert!(!PathBuf::from(&store).exists());
    let bytes = b"model response".to_vec();
    let (reference, _) = put(&scratch, "artifact.bin", &bytes, &store);
    assert!(PathBuf::from(&store).is_dir(), "a put opens the store");
    assert_eq!(get(&reference.to_string(), &store).unwrap(), bytes);
}

#[test]
fn get_writes_the_bytes_and_nothing_else() {
    let scratch = Scratch::new("bytes-only");
    let store = scratch.path("store");
    let bytes: Vec<u8> = (0_u8..=255).collect();
    let (reference, _) = put(&scratch, "artifact.bin", &bytes, &store);
    assert_eq!(get(&reference.to_string(), &store).unwrap(), bytes);
}

#[test]
fn putting_bytes_that_are_already_stored_changes_nothing() {
    let scratch = Scratch::new("idempotent");
    let store = scratch.path("store");
    let bytes = b"once".to_vec();
    let (first, printed) = put(&scratch, "a.bin", &bytes, &store);
    let (second, printed_again) = put(&scratch, "b.bin", &bytes, &store);
    assert_eq!(first, second);
    assert_eq!(printed, printed_again);
    let object = FsArtifactStore::open(store.as_str())
        .unwrap()
        .object_path(&first);
    assert_eq!(fs::read(&object).unwrap(), bytes);
}

#[test]
fn get_of_a_reference_that_was_never_stored_is_artifact_missing() {
    let scratch = Scratch::new("missing");
    let store = scratch.path("store");
    put(&scratch, "a.bin", b"stored", &store);
    let absent = ArtifactRef::of(b"never stored").to_string();
    let err = get(&absent, &store).unwrap_err();
    let text = format!("{err:#}");
    assert!(text.starts_with("artifact_missing:"), "{text}");
    assert!(text.contains(&absent), "{text}");
}

#[test]
fn get_of_an_altered_object_is_artifact_mismatch() {
    let scratch = Scratch::new("mismatch");
    let store = scratch.path("store");
    let (reference, _) = put(&scratch, "a.bin", b"model response", &store);
    tamper(
        &FsArtifactStore::open(store.as_str()).unwrap(),
        &reference,
        b"tampered",
    );
    let err = get(&reference.to_string(), &store).unwrap_err();
    let text = format!("{err:#}");
    assert!(
        text.starts_with("artifact_mismatch:"),
        "every read re-hashes; the bytes are never returned unchecked: {text}"
    );
    assert!(text.contains(&reference.to_string()), "{text}");
}

#[test]
fn get_rejects_anything_that_is_not_an_artifact_reference() {
    let scratch = Scratch::new("bad-reference");
    let store = scratch.path("store");
    put(&scratch, "a.bin", b"abc", &store);
    for bad in [
        ABC_SHA256.to_owned(),
        format!("SHA256:{ABC_SHA256}"),
        format!("sha256:{}", ABC_SHA256.to_uppercase()),
        format!("sha256:{}", &ABC_SHA256[1..]),
        format!("sha256:{ABC_SHA256}0"),
        format!("sha512:{ABC_SHA256}"),
        String::new(),
    ] {
        let err = get(&bad, &store).unwrap_err();
        assert!(
            format!("{err:#}").contains("not an artifact reference"),
            "{bad}: {err:#}"
        );
    }
}

#[test]
fn get_refuses_a_store_path_that_is_not_a_directory_rather_than_creating_it() {
    let scratch = Scratch::new("no-store");
    let store = scratch.path("store");
    let err = get(&ArtifactRef::of(b"abc").to_string(), &store).unwrap_err();
    assert!(format!("{err:#}").contains("not a directory"), "{err:#}");
    assert!(
        !PathBuf::from(&store).exists(),
        "a get never creates a store"
    );
}

#[test]
fn put_names_a_file_it_cannot_read() {
    let scratch = Scratch::new("no-file");
    let store = scratch.path("store");
    let absent = scratch.path("absent.bin");
    let args = parse_put(&[absent.as_str(), "--store", store.as_str()]).unwrap();
    let err = artifact::put(&args, &mut Vec::new()).unwrap_err();
    assert!(format!("{err:#}").contains("absent.bin"), "{err:#}");
}

#[test]
fn an_artifact_put_and_got_again_is_the_file_that_went_in() {
    let scratch = Scratch::new("files");
    let store = scratch.path("store");
    let mut stored = Vec::new();
    for n in 0_u8..4 {
        let bytes = vec![n; usize::from(n) * 1000];
        let (reference, _) = put(&scratch, &format!("f{n}.bin"), &bytes, &store);
        stored.push((reference, bytes));
    }
    for (reference, bytes) in stored {
        assert_eq!(get(&reference.to_string(), &store).unwrap(), bytes);
    }
}

#[test]
fn put_and_get_take_one_path_and_a_required_store() {
    assert!(parse_put(&[]).is_err());
    assert!(parse_put(&["a.bin"]).is_err());
    assert!(parse_get(&[]).is_err());
    assert!(parse_get(&["sha256:x"]).is_err());
    let put_args = parse_put(&["a.bin", "--store", "art"]).unwrap();
    assert_eq!(put_args.file, PathBuf::from("a.bin"));
    assert_eq!(put_args.store, PathBuf::from("art"));
    let get_args = parse_get(&["sha256:x", "--store", "art"]).unwrap();
    assert_eq!(get_args.reference, "sha256:x");
    assert_eq!(get_args.store, PathBuf::from("art"));
}
