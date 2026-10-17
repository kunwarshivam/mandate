//! The release images (E21-35, DEC-971; infrastructure design §7.1 step 2): one OCI image a
//! release binary, written here byte for byte, so an image is a pure function of its binary.
//!
//! An image is an empty base and one layer holding only the static binary, owned by root and not
//! writable, run as the non-root user 65532 with no shell, no `Cmd` and no environment. A
//! signature, when the binary has one, is a second layer of its own (`/<binary>.sig`), so the
//! binary's layer is the same whoever signed it. Layers are uncompressed tar, which every OCI
//! runtime reads, because no compressor is pinned. A read-only root filesystem is the pod's
//! setting (E21-36's chart), which these images need nothing to honour: nothing in them writes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde_json::json;
use sha2::{Digest as _, Sha256};

/// The user every image runs as: not root, and no account in the image, so no shell or login.
pub(crate) const USER: &str = "65532:65532";

/// Where the images' source lives, for their `org.opencontainers.image.source` label.
pub(crate) const SOURCE: &str = "https://github.com/kunwarshivam/mandate";

pub(crate) const MANIFEST_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub(crate) const CONFIG_TYPE: &str = "application/vnd.oci.image.config.v1+json";
pub(crate) const LAYER_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
pub(crate) const INDEX_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// One image as an OCI image layout: its blobs by SHA-256 hex, its `index.json`, and the digest
/// of its manifest, the image's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) blobs: BTreeMap<String, Vec<u8>>,
    pub(crate) index: Vec<u8>,
    pub(crate) manifest: String,
    pub(crate) layers: Vec<String>,
}

/// The tar of one file at the root, owned by root with `mode`, every timestamp zero.
pub(crate) fn tar_one(name: &str, mode: u32, contents: &[u8]) -> Vec<u8> {
    assert!(
        name.len() <= NAME_LEN,
        "a tar name of more than {NAME_LEN} bytes would overwrite the mode: {name}"
    );
    let mut header = [0u8; RECORD];
    put(&mut header, 0, name.as_bytes());
    put(&mut header, 100, format!("{mode:07o}\0").as_bytes());
    put(&mut header, 108, b"0000000\0");
    put(&mut header, 116, b"0000000\0");
    put(
        &mut header,
        124,
        format!("{:011o}\0", contents.len()).as_bytes(),
    );
    put(&mut header, 136, b"00000000000\0");
    put(&mut header, 148, b"        ");
    put(&mut header, 156, b"0");
    put(&mut header, 257, b"ustar\0");
    put(&mut header, 263, b"00");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    put(&mut header, 148, format!("{sum:06o}\0 ").as_bytes());
    let mut tar = header.to_vec();
    tar.extend_from_slice(contents);
    tar.resize(tar.len().div_ceil(RECORD) * RECORD + 2 * RECORD, 0);
    tar
}

/// Writes `field` into `header` at `at`; a field past the end is dropped, which no caller does.
fn put(header: &mut [u8; RECORD], at: usize, field: &[u8]) {
    if let Some(slot) = header.get_mut(at..at + field.len()) {
        slot.copy_from_slice(field);
    }
}

/// The longest name a ustar header's name field holds without its prefix field.
const NAME_LEN: usize = 100;

/// A tar record's size.
const RECORD: usize = 512;

/// The binary's mode: read and run by anyone, written by no one.
const BINARY_MODE: u32 = 0o555;
/// The signature's mode: read by anyone, written by no one.
const SIGNATURE_MODE: u32 = 0o444;

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn descriptor(media_type: &str, bytes: &[u8]) -> serde_json::Value {
    json!({
        "mediaType": media_type,
        "digest": format!("sha256:{}", hex(bytes)),
        "size": bytes.len(),
    })
}

/// The image of the binary `name` with `binary`'s bytes, signed by `signature` if given, built
/// from commit `revision`.
pub(crate) fn layout(
    name: &str,
    binary: &[u8],
    signature: Option<&[u8]>,
    revision: &str,
) -> Layout {
    let mut tars = vec![tar_one(name, BINARY_MODE, binary)];
    if let Some(signature) = signature {
        tars.push(tar_one(&format!("{name}.sig"), SIGNATURE_MODE, signature));
    }
    let layers: Vec<String> = tars.iter().map(|tar| hex(tar)).collect();
    let labels = json!({
        "org.opencontainers.image.revision": revision,
        "org.opencontainers.image.source": SOURCE,
    });
    let config = json!({
        "architecture": "amd64",
        "os": "linux",
        "config": {
            "Entrypoint": [format!("/{name}")],
            "Labels": labels,
            "User": USER,
        },
        "rootfs": {
            "type": "layers",
            "diff_ids": layers.iter().map(|h| format!("sha256:{h}")).collect::<Vec<_>>(),
        },
    })
    .to_string()
    .into_bytes();
    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST_TYPE,
        "config": descriptor(CONFIG_TYPE, &config),
        "layers": tars.iter().map(|tar| descriptor(LAYER_TYPE, tar)).collect::<Vec<_>>(),
        "annotations": labels,
    })
    .to_string()
    .into_bytes();
    let index = json!({
        "schemaVersion": 2,
        "mediaType": INDEX_TYPE,
        "manifests": [descriptor(MANIFEST_TYPE, &manifest)],
    })
    .to_string()
    .into_bytes();
    let manifest_hex = hex(&manifest);
    let mut blobs: BTreeMap<String, Vec<u8>> =
        tars.into_iter().map(|tar| (hex(&tar), tar)).collect();
    blobs.insert(hex(&config), config);
    blobs.insert(manifest_hex.clone(), manifest);
    Layout {
        blobs,
        index,
        manifest: manifest_hex,
        layers,
    }
}

/// Writes `layout` as an OCI image layout directory at `dir`.
pub(crate) fn write(layout: &Layout, dir: &Path) -> Result<()> {
    let blobs = dir.join("blobs").join("sha256");
    fs::create_dir_all(&blobs).with_context(|| format!("creating {}", blobs.display()))?;
    for (hex, bytes) in &layout.blobs {
        fs::write(blobs.join(hex), bytes).with_context(|| format!("writing blob {hex}"))?;
    }
    fs::write(dir.join("oci-layout"), br#"{"imageLayoutVersion":"1.0.0"}"#)
        .context("writing oci-layout")?;
    fs::write(dir.join("index.json"), &layout.index).context("writing index.json")?;
    Ok(())
}

/// Pushes the image layout at `dir` to `base` (`https://ghcr.io`, or a local registry's
/// `http://…` in a test) as `repository:tag`, by the OCI distribution API, with the
/// `Authorization` header in the file `auth` if given, so no credential is ever on a command line
/// or in a log. Returns the digest the registry reports for the tag, which must be the
/// manifest's.
pub(crate) fn push(
    layout: &Layout,
    dir: &Path,
    base: &str,
    repository: &str,
    tag: &str,
    auth: Option<&Path>,
) -> Result<String> {
    let blobs = dir.join("blobs").join("sha256");
    let api = format!("{base}/v2/{repository}");
    for hex in layout.blobs.keys().filter(|hex| **hex != layout.manifest) {
        let digest = format!("sha256:{hex}");
        if curl(
            auth,
            &["-I", "-o", "/dev/null"],
            &format!("{api}/blobs/{digest}"),
        )
        .is_ok()
        {
            continue;
        }
        let headers = dir.join("upload.headers");
        curl(
            auth,
            &[
                "-X",
                "POST",
                "-o",
                "/dev/null",
                "-D",
                &headers.display().to_string(),
            ],
            &format!("{api}/blobs/uploads/"),
        )?;
        let location = header(&fs::read_to_string(&headers)?, "location")
            .with_context(|| format!("no upload location for {digest}"))?;
        let location = if location.starts_with('/') {
            format!("{base}{location}")
        } else {
            ensure!(
                location.starts_with(&format!("{base}/")),
                "the registry named an upload location on another host; refusing to send it the \
                 credential"
            );
            location
        };
        let separator = if location.contains('?') { '&' } else { '?' };
        curl(
            auth,
            &[
                "-X",
                "PUT",
                "-o",
                "/dev/null",
                "-H",
                "Content-Type: application/octet-stream",
                "--data-binary",
                &format!("@{}", blobs.join(hex).display()),
            ],
            &format!("{location}{separator}digest={digest}"),
        )?;
    }
    curl(
        auth,
        &[
            "-X",
            "PUT",
            "-o",
            "/dev/null",
            "-H",
            &format!("Content-Type: {MANIFEST_TYPE}"),
            "--data-binary",
            &format!("@{}", blobs.join(&layout.manifest).display()),
        ],
        &format!("{api}/manifests/{tag}"),
    )?;
    let headers = curl(
        auth,
        &["-I", "-H", &format!("Accept: {MANIFEST_TYPE}")],
        &format!("{api}/manifests/{tag}"),
    )?;
    let reported = header(&headers, "docker-content-digest").context("no digest reported")?;
    ensure!(
        reported == format!("sha256:{}", layout.manifest),
        "{repository}:{tag} is {reported}, not the image's sha256:{}",
        layout.manifest
    );
    Ok(reported)
}

/// The value of the HTTP header `name` (any case) in `text`.
fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// Runs `curl -sSf` with `args`, the header file `auth` if given, and `url`; returns its stdout.
fn curl(auth: Option<&Path>, args: &[&str], url: &str) -> Result<String> {
    let mut command = Command::new("curl");
    command.args(["-sSf"]).args(args);
    if let Some(auth) = auth {
        command.arg("-H").arg(format!("@{}", auth.display()));
    }
    let out = command.arg(url).output().context("starting `curl`")?;
    ensure!(
        out.status.success(),
        "curl {} failed: {}",
        url.split('?').next().unwrap_or_default(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    String::from_utf8(out.stdout).context("curl printed non-UTF-8")
}

/// Standard base64 with padding (RFC 4648 §4), for a Basic `Authorization` header.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (at, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if at <= chunk.len() {
                let index = usize::try_from((n >> shift) & 63).unwrap_or(0);
                out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'=')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Extracts every layer of `layout` into `rootfs`, in order, as a runtime would.
pub(crate) fn unpack(layout: &Layout, dir: &Path, rootfs: &Path) -> Result<()> {
    fs::create_dir_all(rootfs).with_context(|| format!("creating {}", rootfs.display()))?;
    for hex in &layout.layers {
        let status = Command::new("tar")
            .arg("-xf")
            .arg(dir.join("blobs").join("sha256").join(hex))
            .arg("-C")
            .arg(rootfs)
            .status()
            .context("starting `tar`")?;
        ensure!(status.success(), "unpacking layer {hex}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use serde_json::Value;

    use super::*;

    const BINARY: &[u8] = b"\x7fELF a static release binary";
    const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

    fn json(layout: &Layout, hex: &str) -> Value {
        let bytes = layout
            .blobs
            .get(hex)
            .unwrap_or_else(|| panic!("no blob {hex}"));
        serde_json::from_slice(bytes).unwrap_or_else(|e| panic!("blob {hex}: {e}"))
    }

    fn hex_of(digest: &Value) -> String {
        digest
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .unwrap_or_else(|| panic!("not a digest: {digest}"))
            .to_owned()
    }

    fn manifest(layout: &Layout) -> Value {
        json(layout, &layout.manifest)
    }

    fn config(layout: &Layout) -> Value {
        json(layout, &hex_of(&manifest(layout)["config"]["digest"]))
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-image-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        dir
    }

    /// `sha256sum` and `tar` are the oracles: the layout is written and read back by them.
    fn sha256sum(path: &Path) -> String {
        let out = Command::new("sha256sum")
            .arg(path)
            .output()
            .unwrap_or_else(|e| panic!("sha256sum: {e}"));
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn tar_listing(path: &Path) -> String {
        let out = Command::new("tar")
            .args(["--numeric-owner", "-tvf"])
            .arg(path)
            .output()
            .unwrap_or_else(|e| panic!("tar: {e}"));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn tar_extract(path: &Path, member: &str) -> Vec<u8> {
        let out = Command::new("tar")
            .args(["-xOf"])
            .arg(path)
            .arg(member)
            .output()
            .unwrap_or_else(|e| panic!("tar: {e}"));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    #[test]
    fn an_image_is_a_pure_function_of_its_inputs() {
        let a = layout("mandate-agentd", BINARY, Some(b"sig"), REVISION);
        let b = layout("mandate-agentd", BINARY, Some(b"sig"), REVISION);
        assert_eq!(a, b);
        assert_eq!(a.manifest.len(), 64);
    }

    /// The binary's layer is the same signed or not, and whoever signed it; the image is not.
    #[test]
    fn the_binary_layer_does_not_depend_on_the_signature() {
        let unsigned = layout("mandate-agentd", BINARY, None, REVISION);
        let one = layout("mandate-agentd", BINARY, Some(b"one"), REVISION);
        let two = layout("mandate-agentd", BINARY, Some(b"two"), REVISION);
        assert_eq!(unsigned.layers.len(), 1);
        assert_eq!(one.layers.len(), 2);
        assert_eq!(unsigned.layers.first(), one.layers.first());
        assert_eq!(one.layers.first(), two.layers.first());
        assert_ne!(one.manifest, two.manifest);
        assert_ne!(unsigned.manifest, one.manifest);
    }

    /// Non-root, the binary as the entrypoint, and nothing else to run: no `Cmd`, no `Env`, no
    /// `Shell`, no working directory, no volume, no exposed port.
    #[test]
    fn the_image_runs_its_binary_as_a_non_root_user_and_nothing_else() {
        let image = layout("mandate-agentd", BINARY, Some(b"sig"), REVISION);
        let config = config(&image);
        assert_eq!(config["os"], "linux");
        assert_eq!(config["architecture"], "amd64");
        let run = config["config"]
            .as_object()
            .unwrap_or_else(|| panic!("config.config: {config}"));
        let keys: Vec<&str> = run.keys().map(String::as_str).collect();
        assert_eq!(keys, ["Entrypoint", "Labels", "User"]);
        assert_eq!(run["User"], USER);
        assert_eq!(run["Entrypoint"], serde_json::json!(["/mandate-agentd"]));
        assert_eq!(run["Labels"]["org.opencontainers.image.revision"], REVISION);
        assert_eq!(run["Labels"]["org.opencontainers.image.source"], SOURCE);
        let diff_ids: Vec<String> = config["rootfs"]["diff_ids"]
            .as_array()
            .unwrap_or_else(|| panic!("diff_ids"))
            .iter()
            .map(hex_of)
            .collect();
        assert_eq!(diff_ids, image.layers);
        assert_eq!(config["rootfs"]["type"], "layers");
    }

    /// Written to disk, every blob is named by its own SHA-256, every descriptor's size is its
    /// blob's, and the index names the manifest.
    #[test]
    fn the_layout_is_content_addressed() {
        let image = layout("mandate-agentd", BINARY, Some(b"sig"), REVISION);
        let dir = scratch("addressed");
        write(&image, &dir).unwrap_or_else(|e| panic!("{e}"));
        let blobs = dir.join("blobs").join("sha256");
        for hex in image.blobs.keys() {
            assert_eq!(&sha256sum(&blobs.join(hex)), hex);
        }
        let index: Value = serde_json::from_slice(
            &fs::read(dir.join("index.json")).unwrap_or_else(|e| panic!("{e}")),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(index["mediaType"], INDEX_TYPE);
        let entry = &index["manifests"][0];
        assert_eq!(entry["mediaType"], MANIFEST_TYPE);
        assert_eq!(hex_of(&entry["digest"]), image.manifest);
        let size = |hex: &str| fs::metadata(blobs.join(hex)).map(|m| m.len()).unwrap_or(0);
        assert_eq!(entry["size"].as_u64(), Some(size(&image.manifest)));
        let manifest = manifest(&image);
        assert_eq!(manifest["mediaType"], MANIFEST_TYPE);
        assert_eq!(manifest["config"]["mediaType"], CONFIG_TYPE);
        let config = hex_of(&manifest["config"]["digest"]);
        assert_eq!(manifest["config"]["size"].as_u64(), Some(size(&config)));
        for (layer, hex) in manifest["layers"]
            .as_array()
            .unwrap_or_else(|| panic!("layers"))
            .iter()
            .zip(&image.layers)
        {
            assert_eq!(layer["mediaType"], LAYER_TYPE);
            assert_eq!(&hex_of(&layer["digest"]), hex);
            assert_eq!(layer["size"].as_u64(), Some(size(hex)));
        }
        assert_eq!(
            fs::read_to_string(dir.join("oci-layout")).unwrap_or_default(),
            r#"{"imageLayoutVersion":"1.0.0"}"#
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// Read back by `tar`: the binary's layer holds exactly the binary, root's and not writable;
    /// the signature's holds exactly the signature, read-only. Nothing else, so no shell.
    #[test]
    fn each_layer_holds_exactly_its_one_file() {
        let image = layout("mandate-agentd", BINARY, Some(b"the signature"), REVISION);
        let dir = scratch("layers");
        write(&image, &dir).unwrap_or_else(|e| panic!("{e}"));
        let blobs = dir.join("blobs").join("sha256");
        let (binary, signature) = match image.layers.as_slice() {
            [binary, signature] => (blobs.join(binary), blobs.join(signature)),
            other => panic!("layers: {other:?}"),
        };
        let listing = tar_listing(&binary);
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(lines.len(), 1, "{listing}");
        assert!(lines[0].starts_with("-r-xr-xr-x 0/0"), "{listing}");
        assert!(lines[0].ends_with(" mandate-agentd"), "{listing}");
        assert_eq!(tar_extract(&binary, "mandate-agentd"), BINARY);
        let listing = tar_listing(&signature);
        let lines: Vec<&str> = listing.lines().collect();
        assert_eq!(lines.len(), 1, "{listing}");
        assert!(lines[0].starts_with("-r--r--r-- 0/0"), "{listing}");
        assert!(lines[0].ends_with(" mandate-agentd.sig"), "{listing}");
        assert_eq!(
            tar_extract(&signature, "mandate-agentd.sig"),
            b"the signature"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// Every padding case: one, two and three trailing bytes, and the empty input.
    #[test]
    fn base64_is_rfc_4648() {
        for (plain, encoded) in [
            ("", ""),
            ("a", "YQ=="),
            ("ab", "YWI="),
            ("abc", "YWJj"),
            ("abcd", "YWJjZA=="),
            ("hello world", "aGVsbG8gd29ybGQ="),
            ("\u{ff}\u{fe}", "w7/Dvg=="),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn a_header_is_found_in_any_case() {
        let text = "HTTP/1.1 202 Accepted\r\nLocation: /v2/x/blobs/uploads/1?s=2\r\nDocker-Content-Digest: sha256:ab\r\n";
        assert_eq!(
            header(text, "location").as_deref(),
            Some("/v2/x/blobs/uploads/1?s=2")
        );
        assert_eq!(
            header(text, "DOCKER-CONTENT-DIGEST").as_deref(),
            Some("sha256:ab")
        );
        assert_eq!(header(text, "etag"), None);
    }

    /// Every shipped binary's name, and its signature's, fits the header's name field.
    #[test]
    fn every_shipped_name_fits_a_tar_header() {
        for (_, name) in crate::release::BINARIES {
            assert!(format!("{name}.sig").len() <= NAME_LEN, "{name}");
        }
    }

    #[test]
    #[should_panic(expected = "would overwrite the mode")]
    fn a_name_too_long_for_the_header_is_refused() {
        tar_one(&"n".repeat(NAME_LEN + 1), 0o444, b"");
    }

    /// A tar is whole 512-byte records and ends in two zero records.
    #[test]
    fn a_tar_is_whole_records_ending_in_two_zero_records() {
        for size in [0usize, 1, 511, 512, 513, 4096] {
            let tar = tar_one("f", 0o444, &vec![b'x'; size]);
            assert_eq!(tar.len() % 512, 0, "{size}");
            assert!(tar.len() >= 512 * 3 + size, "{size}");
            assert!(tar[tar.len() - 1024..].iter().all(|b| *b == 0), "{size}");
        }
    }
}
