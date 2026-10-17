//! `cargo xtask release`: the reproducible release build (E21-8, DEC-970; infrastructure design
//! §7.1 step 2, ES-14, ES-17).
//!
//! `release build` builds the release's binaries from the checked-out commit with `--locked`, as
//! static musl executables, under `cargo auditable` so each carries its dependency list, with every
//! machine path remapped and `SOURCE_DATE_EPOCH` set to the commit's time. It writes the binaries,
//! their `SHA256SUMS`, and one CycloneDX SBOM a binary, which syft reads from the binary's own
//! auditable data, and prints `digest=<sha256 of SHA256SUMS>` on stdout, so a CI job can hand the
//! digest on as an output. Two builds of one commit, on two runners in two checkout paths, must
//! print the same digest: `release compare` and the `release` workflow check it. No build here
//! enables a feature, so none can build `live` (DEC-529 item 3).
//!
//! The tools are pinned by version and SHA-256 and fetched from their GitHub releases only
//! (`AGENTS.md` Setup); `release tools` installs them under `target/release-tools`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

/// The one target every release binary is built for: static, so the image needs no libc.
pub(crate) const TARGET: &str = "x86_64-unknown-linux-musl";

/// The C compiler the musl target's C code (`ring`) is built with: the runner's own `gcc`, named
/// rather than searched for, so both builds use the same one.
const TARGET_CC: &str = "gcc";

/// What a release ships: each package and the binary it builds. `mandate-rh-sim` is the
/// loopback test double the local cell runs beside the executor (DEC-124), never a broker.
pub(crate) const BINARIES: [(&str, &str); 3] = [
    ("mandate-cli", "mandate"),
    ("mandate-agentd", "mandate-agentd"),
    ("mandate-rh-sim", "mandate-rh-sim"),
];

/// Where `release build` keeps its own target directory, apart from the workspace's, because its
/// flags differ and would rebuild everything there.
const TARGET_DIR: &str = "target/release-build";

/// Where `release tools` installs the pinned tools.
const TOOLS_DIR: &str = "target/release-tools";

/// One pinned tool: its release asset, the asset's SHA-256, and the executable's path inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tool {
    pub(crate) name: &'static str,
    pub(crate) version: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
    pub(crate) member: &'static str,
}

/// The release build's tools (D-7, DEC-969; rows in `docs/dependencies.md`).
pub(crate) const TOOLS: [Tool; 2] = [
    Tool {
        name: "cargo-auditable",
        version: "0.7.7",
        url: "https://github.com/rust-secure-code/cargo-auditable/releases/download/v0.7.7/cargo-auditable-x86_64-unknown-linux-gnu.tar.xz",
        sha256: "4c8d3b08c3c34e9f2333367224dbbfb4aab68067698b43de59fe19aded3416dd",
        member: "cargo-auditable-x86_64-unknown-linux-gnu/cargo-auditable",
    },
    Tool {
        name: "syft",
        version: "1.52.0",
        url: "https://github.com/anchore/syft/releases/download/v1.52.0/syft_1.52.0_linux_amd64.tar.gz",
        sha256: "caeedb81fb0491615f1ebd1761e4145d41ee86dd2cc7bf80669f9f5ad9d6133d",
        member: "syft",
    },
];

/// The paths a build would otherwise write into its binaries (panic locations, `file!()`), each
/// with the fixed prefix that replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Paths {
    pub(crate) root: PathBuf,
    pub(crate) cargo_home: PathBuf,
    pub(crate) rustup_home: PathBuf,
}

impl Paths {
    fn from_env(root: &Path) -> Result<Self> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let or_home = |name: &str, under: &str| -> Result<PathBuf> {
            env::var_os(name)
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|home| home.join(under)))
                .with_context(|| format!("neither {name} nor HOME is set"))
        };
        Ok(Self {
            root: root.to_path_buf(),
            cargo_home: or_home("CARGO_HOME", ".cargo")?,
            rustup_home: or_home("RUSTUP_HOME", ".rustup")?,
        })
    }
}

/// The `RUSTFLAGS` of a release build: a static C runtime, and every machine path remapped to a
/// fixed prefix, so two checkouts in two places build the same bytes.
pub(crate) fn rustflags(paths: &Paths) -> String {
    let _ = paths;
    String::new()
}

/// The `cargo` arguments of a release build: every [`BINARIES`] package, `--locked`, the release
/// profile, the musl [`TARGET`], and no feature flag of any kind.
pub(crate) fn build_args(target_dir: &Path) -> Vec<String> {
    let _ = target_dir;
    Vec::new()
}

/// `SHA256SUMS` in `sha256sum`'s own format, `<hex>  <name>`, one line a binary, sorted by name.
pub(crate) fn sums_text(sums: &BTreeMap<String, String>) -> String {
    let _ = sums;
    String::new()
}

/// Reads a `SHA256SUMS` [`sums_text`] wrote: each line `<64 lowercase hex>  <name>`, each name
/// once.
pub(crate) fn parse_sums(text: &str) -> Result<BTreeMap<String, String>> {
    let _ = text;
    bail!("unimplemented (E21-8)")
}

/// Whether two builds' `SHA256SUMS` name the same binaries, every [`BINARIES`] binary among them,
/// with the same digests.
pub(crate) fn compare_sums(a: &str, b: &str) -> Result<()> {
    let _ = (a, b);
    bail!("unimplemented (E21-8)")
}

/// `cargo xtask release tools`: installs each [`TOOLS`] entry under [`TOOLS_DIR`], checked
/// against its SHA-256, and returns the directory that holds them.
pub(crate) fn tools(root: &Path) -> Result<PathBuf> {
    let _ = root;
    bail!("unimplemented (E21-8)")
}

/// `cargo xtask release build <out>`.
pub(crate) fn build(root: &Path, out: &Path) -> Result<()> {
    let _ = (root, out);
    bail!("unimplemented (E21-8)")
}

/// `cargo xtask release compare <a> <b>`: two output directories of [`build`].
pub(crate) fn compare(a: &Path, b: &Path) -> Result<()> {
    let read = |dir: &Path| {
        fs::read_to_string(dir.join("SHA256SUMS"))
            .with_context(|| format!("reading {}/SHA256SUMS", dir.display()))
    };
    compare_sums(&read(a)?, &read(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths {
            root: PathBuf::from("/work/a/mandate"),
            cargo_home: PathBuf::from("/home/runner/.cargo"),
            rustup_home: PathBuf::from("/home/runner/.rustup"),
        }
    }

    fn hex(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn all(c: char) -> BTreeMap<String, String> {
        BINARIES
            .iter()
            .map(|(_, bin)| ((*bin).to_owned(), hex(c)))
            .collect()
    }

    /// Every machine path a binary could carry is remapped to a fixed prefix, the C runtime is
    /// static, and nothing else is set: no `--cfg` that could switch a feature on.
    #[test]
    fn the_flags_remap_every_machine_path_and_link_statically() {
        let flags = rustflags(&paths());
        let words: Vec<&str> = flags.split(' ').collect();
        assert_eq!(
            words,
            [
                "-C",
                "target-feature=+crt-static",
                "--remap-path-prefix=/work/a/mandate=/mandate",
                "--remap-path-prefix=/home/runner/.cargo=/cargo",
                "--remap-path-prefix=/home/runner/.rustup=/rustup",
            ]
        );
    }

    /// Two checkouts in two places give the same flags but for the paths they remap.
    #[test]
    fn the_flags_differ_only_by_the_checkout_path() {
        let mut other = paths();
        other.root = PathBuf::from("/elsewhere/b/src");
        assert_eq!(
            rustflags(&paths()).replace("/work/a/mandate", "/elsewhere/b/src"),
            rustflags(&other)
        );
    }

    /// The release builds every shipped package, locked, for the musl target, and never names a
    /// feature, so the runner's `live` cannot be built here (DEC-529 item 3).
    #[test]
    fn the_build_is_locked_static_and_featureless() {
        let args = build_args(Path::new("/t"));
        assert_eq!(args.first().map(String::as_str), Some("auditable"));
        assert_eq!(args.get(1).map(String::as_str), Some("build"));
        for flag in ["--locked", "--release"] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
        }
        let after = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|at| args.get(at + 1))
                .cloned()
        };
        assert_eq!(after("--target").as_deref(), Some(TARGET));
        assert_eq!(after("--target-dir").as_deref(), Some("/t"));
        let packages: Vec<&str> = args
            .iter()
            .zip(args.iter().skip(1))
            .filter(|(flag, _)| *flag == "-p")
            .map(|(_, package)| package.as_str())
            .collect();
        let shipped: Vec<&str> = BINARIES.iter().map(|(package, _)| *package).collect();
        assert_eq!(packages, shipped);
        assert!(
            !args.iter().any(|a| a.contains("feature")),
            "a feature flag in {args:?}"
        );
    }

    /// No shipped package is the runner, the one crate that may declare `live`.
    #[test]
    fn no_shipped_package_may_declare_live() -> Result<()> {
        let policy: toml::Table = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("layers.toml"),
        )?
        .parse()?;
        let crates = policy
            .get("crates")
            .and_then(toml::Value::as_table)
            .context("layers.toml has no crates table")?;
        for (package, _) in BINARIES {
            let entry = crates
                .get(package)
                .and_then(toml::Value::as_table)
                .with_context(|| format!("{package} has no layers.toml entry"))?;
            assert_ne!(
                entry.get("live_feature").and_then(toml::Value::as_bool),
                Some(true),
                "{package} may declare live"
            );
        }
        Ok(())
    }

    #[test]
    fn sums_are_sha256sums_format_sorted_by_name() {
        let mut sums = BTreeMap::new();
        sums.insert("zeta".to_owned(), hex('1'));
        sums.insert("alpha".to_owned(), hex('2'));
        assert_eq!(
            sums_text(&sums),
            format!("{}  alpha\n{}  zeta\n", hex('2'), hex('1'))
        );
    }

    #[test]
    fn sums_read_back_as_written() -> Result<()> {
        assert_eq!(parse_sums(&sums_text(&all('a')))?, all('a'));
        Ok(())
    }

    #[test]
    fn a_malformed_sums_line_is_refused() {
        for text in [
            String::new(),
            format!("{}  mandate\n{}  mandate\n", hex('a'), hex('b')),
            format!("{} mandate\n", hex('a')),
            format!("{}  mandate\n", hex('A')),
            format!("{}  mandate\n", "a".repeat(63)),
            format!("{}  \n", hex('a')),
            format!("{}  bin/mandate\n", hex('a')),
        ] {
            assert!(parse_sums(&text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn identical_builds_compare_equal() -> Result<()> {
        let text = sums_text(&all('a'));
        compare_sums(&text, &text)
    }

    #[test]
    fn builds_that_differ_in_one_binary_are_refused() {
        let a = all('a');
        for (_, bin) in BINARIES {
            let mut b = a.clone();
            b.insert(bin.to_owned(), hex('b'));
            assert!(
                compare_sums(&sums_text(&a), &sums_text(&b)).is_err(),
                "{bin} differs"
            );
        }
    }

    #[test]
    fn builds_that_miss_a_shipped_binary_are_refused() {
        for (_, bin) in BINARIES {
            let mut a = all('a');
            a.remove(bin);
            let text = sums_text(&a);
            assert!(compare_sums(&text, &text).is_err(), "{bin} missing");
        }
    }

    #[test]
    fn builds_that_name_different_binaries_are_refused() {
        let a = all('a');
        let mut b = a.clone();
        b.insert("extra".to_owned(), hex('a'));
        assert!(compare_sums(&sums_text(&a), &sums_text(&b)).is_err());
        assert!(compare_sums(&sums_text(&b), &sums_text(&a)).is_err());
    }

    /// Each tool is pinned to a GitHub release asset of its own version, with a SHA-256.
    #[test]
    fn every_tool_is_a_pinned_github_release_asset() {
        for tool in TOOLS {
            assert!(
                tool.url.starts_with("https://github.com/"),
                "{}: {}",
                tool.name,
                tool.url
            );
            assert!(tool.url.contains(tool.version), "{}", tool.name);
            assert_eq!(tool.sha256.len(), 64, "{}", tool.name);
            assert!(
                tool.sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{}",
                tool.name
            );
        }
    }

    /// The registry names each tool at the version pinned here (`AGENTS.md`: keep the two in sync).
    #[test]
    fn every_tool_has_a_registry_row_at_its_version() -> Result<()> {
        let registry = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/dependencies.md"),
        )?;
        for tool in TOOLS {
            let row = registry
                .lines()
                .find(|line| line.starts_with(&format!("| `{}` |", tool.name)))
                .with_context(|| format!("no registry row for {}", tool.name))?;
            assert!(row.contains(tool.version), "{}: {row}", tool.name);
        }
        Ok(())
    }
}
