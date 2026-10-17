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

use anyhow::{Context, Result};

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
    let _ = (name, mode, contents);
    Vec::new()
}

/// The image of the binary `name` with `binary`'s bytes, signed by `signature` if given, built
/// from commit `revision`.
pub(crate) fn layout(
    name: &str,
    binary: &[u8],
    signature: Option<&[u8]>,
    revision: &str,
) -> Layout {
    let _ = (name, binary, signature, revision);
    Layout {
        blobs: BTreeMap::new(),
        index: Vec::new(),
        manifest: String::new(),
        layers: Vec::new(),
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
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn tar_extract(path: &Path, member: &str) -> Vec<u8> {
        let out = Command::new("tar")
            .args(["-xOf"])
            .arg(path)
            .arg(member)
            .output()
            .unwrap_or_else(|e| panic!("tar: {e}"));
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
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
        assert_eq!(tar_extract(&signature, "mandate-agentd.sig"), b"the signature");
        fs::remove_dir_all(&dir).ok();
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
