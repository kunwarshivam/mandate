//! `cargo xtask release`: the reproducible release build (E21-8, DEC-972; infrastructure design
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
//! (`AGENTS.md` Setup); `release tools` installs them under `$CARGO_HOME/mandate-release-tools`.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

use crate::image;

/// The architectures a release is built for, each natively on a runner of its own (DEC-1060): no
/// cross toolchain, so each arch's build is the same reproducible path as the other's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Arch {
    Amd64,
    Arm64,
}

impl Arch {
    pub(crate) const ALL: [Self; 2] = [Self::Amd64, Self::Arm64];

    /// The arch Rust names `rust_arch` (`std::env::consts::ARCH`), if a release is built for it.
    pub(crate) fn of(rust_arch: &str) -> Option<Self> {
        match rust_arch {
            "x86_64" => Some(Self::Amd64),
            "aarch64" => Some(Self::Arm64),
            _ => None,
        }
    }

    /// The arch of the machine running xtask: a release builds only for its own host.
    pub(crate) fn host() -> Result<Self> {
        Self::of(env::consts::ARCH)
            .with_context(|| format!("no release is built on {}", env::consts::ARCH))
    }

    /// The OCI platform's `architecture`, and the suffix of the arch's tags.
    pub(crate) fn oci(self) -> &'static str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        }
    }

    /// The target every release binary of this arch is built for: static musl, so the image needs
    /// no libc.
    pub(crate) fn target(self) -> &'static str {
        match self {
            Self::Amd64 => "x86_64-unknown-linux-musl",
            Self::Arm64 => "aarch64-unknown-linux-musl",
        }
    }

    /// The release tools for a machine of this arch.
    pub(crate) fn tools(self) -> [Tool; 3] {
        match self {
            Self::Amd64 => AMD64_TOOLS,
            Self::Arm64 => ARM64_TOOLS,
        }
    }
}

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

/// The only variables the build inherits, beside the ones it sets: the home directory, the
/// terminal, and the network's proxy and certificate settings, which reach the registry and
/// nothing the compiler sees. Everything else is cleared, so neither what `cargo run` hands xtask
/// (`CARGO_*`, `RUSTUP_TOOLCHAIN`) nor a runner's own settings (`RUSTC_WRAPPER`, inherited
/// rustflags) can reach the build.
const PASSED_ENV: [&str; 11] = [
    "HOME",
    "TERM",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CARGO_HTTP_CAINFO",
];

/// Where `release tools` installs the pinned tools, under `CARGO_HOME`, never under the checkout:
/// cargo hashes the path of `cargo-auditable`, its `RUSTC_WORKSPACE_WRAPPER`, into every workspace
/// crate's metadata, so a tool inside each checkout made two checkouts build different binaries.
const TOOLS_DIR: &str = "mandate-release-tools";

/// One pinned tool: its release asset, the asset's SHA-256, and the executable's path inside it,
/// or `None` when the asset is the executable itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tool {
    pub(crate) name: &'static str,
    pub(crate) version: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: &'static str,
    pub(crate) member: Option<&'static str>,
}

/// The release build's tools on amd64 (D-7, DEC-990; rows in `docs/dependencies.md`).
const AMD64_TOOLS: [Tool; 3] = [
    Tool {
        name: "cargo-auditable",
        version: "0.7.7",
        url: "https://github.com/rust-secure-code/cargo-auditable/releases/download/v0.7.7/cargo-auditable-x86_64-unknown-linux-gnu.tar.xz",
        sha256: "4c8d3b08c3c34e9f2333367224dbbfb4aab68067698b43de59fe19aded3416dd",
        member: Some("cargo-auditable-x86_64-unknown-linux-gnu/cargo-auditable"),
    },
    Tool {
        name: "syft",
        version: "1.52.0",
        url: "https://github.com/anchore/syft/releases/download/v1.52.0/syft_1.52.0_linux_amd64.tar.gz",
        sha256: "caeedb81fb0491615f1ebd1761e4145d41ee86dd2cc7bf80669f9f5ad9d6133d",
        member: Some("syft"),
    },
    Tool {
        name: "cosign",
        version: "3.1.3",
        url: "https://github.com/sigstore/cosign/releases/download/v3.1.3/cosign-linux-amd64",
        sha256: "4629c757b7618056f8ddd7e2625ae9fdd94c0372a65049520bc7d9df9efc7f71",
        member: None,
    },
];

/// The same tools at the same versions on arm64, each checked against its project's published
/// checksum when pinned (DEC-1060).
const ARM64_TOOLS: [Tool; 3] = [
    Tool {
        name: "cargo-auditable",
        version: "0.7.7",
        url: "https://github.com/rust-secure-code/cargo-auditable/releases/download/v0.7.7/cargo-auditable-aarch64-unknown-linux-gnu.tar.xz",
        sha256: "bcb2d46002a8dd8e94d0d669cb39d38c9651db2ae5d1f3f1f1bab516ee1e212c",
        member: Some("cargo-auditable-aarch64-unknown-linux-gnu/cargo-auditable"),
    },
    Tool {
        name: "syft",
        version: "1.52.0",
        url: "https://github.com/anchore/syft/releases/download/v1.52.0/syft_1.52.0_linux_arm64.tar.gz",
        sha256: "c46d5e4c28e12aa4c5becfaa343ef1c7f89045b6b895f2c21d471c62db09c706",
        member: Some("syft"),
    },
    Tool {
        name: "cosign",
        version: "3.1.3",
        url: "https://github.com/sigstore/cosign/releases/download/v3.1.3/cosign-linux-arm64",
        sha256: "c5d324e091826b0d7a78eb16fef316450b4eb9aaec045611c08ba06f5e73220a",
        member: None,
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
    pub(crate) fn from_env(root: &Path) -> Result<Self> {
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
    let remap = |from: &Path, to: &str| format!("--remap-path-prefix={}={to}", from.display());
    [
        "-C".to_owned(),
        "target-feature=+crt-static".to_owned(),
        remap(&paths.root, "/mandate"),
        remap(&paths.cargo_home, "/cargo"),
        remap(&paths.rustup_home, "/rustup"),
    ]
    .join(" ")
}

/// The `cargo` arguments of a release build: every [`BINARIES`] package, `--locked`, the release
/// profile, `arch`'s musl target, and no feature flag of any kind.
pub(crate) fn build_args(target_dir: &Path, arch: Arch) -> Vec<String> {
    let mut args: Vec<String> = [
        "auditable",
        "build",
        "--locked",
        "--release",
        "--target",
        arch.target(),
    ]
    .map(str::to_owned)
    .to_vec();
    for (package, _) in BINARIES {
        args.extend(["-p".to_owned(), package.to_owned()]);
    }
    args.extend(["--target-dir".to_owned(), target_dir.display().to_string()]);
    args
}

/// `SHA256SUMS` in `sha256sum`'s own format, `<hex>  <name>`, one line a binary, sorted by name.
pub(crate) fn sums_text(sums: &BTreeMap<String, String>) -> String {
    sums.iter()
        .map(|(name, hex)| format!("{hex}  {name}\n"))
        .collect()
}

/// Reads a `SHA256SUMS` [`sums_text`] wrote: each line `<64 lowercase hex>  <name>`, each name
/// once.
pub(crate) fn parse_sums(text: &str) -> Result<BTreeMap<String, String>> {
    let mut sums = BTreeMap::new();
    for line in text.lines() {
        let (hex, name) = line
            .split_once("  ")
            .with_context(|| format!("not a SHA256SUMS line: {line:?}"))?;
        ensure!(
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "not 64 lowercase hex digits: {line:?}"
        );
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
            "not a plain binary name: {line:?}"
        );
        ensure!(
            sums.insert(name.to_owned(), hex.to_owned()).is_none(),
            "{name} is listed twice"
        );
    }
    ensure!(!sums.is_empty(), "SHA256SUMS lists no binary");
    Ok(sums)
}

/// Whether two builds' `SHA256SUMS` name the same binaries, every [`BINARIES`] binary among them,
/// with the same digests.
pub(crate) fn compare_sums(a: &str, b: &str) -> Result<()> {
    let (a, b) = (parse_sums(a)?, parse_sums(b)?);
    for (_, bin) in BINARIES {
        ensure!(a.contains_key(bin), "the first build has no {bin}");
        ensure!(b.contains_key(bin), "the second build has no {bin}");
    }
    let names = |sums: &BTreeMap<String, String>| sums.keys().cloned().collect::<Vec<_>>();
    ensure!(
        names(&a) == names(&b),
        "the builds name different binaries: {:?} and {:?}",
        names(&a),
        names(&b)
    );
    let differ: Vec<&String> = a
        .iter()
        .filter(|(name, hex)| b.get(*name) != Some(*hex))
        .map(|(name, _)| name)
        .collect();
    ensure!(differ.is_empty(), "not reproducible: {differ:?} differ");
    Ok(())
}

/// `cargo xtask release tools`: installs each of the host arch's tools ([`Arch::tools`]) under
/// [`TOOLS_DIR`] in
/// `cargo_home`, checked against its SHA-256, and returns the directory that holds them. An
/// installed tool is reused only while its SHA-256 is still the one recorded when it was installed
/// from the checked archive; otherwise it is fetched and checked again.
pub(crate) fn tools(cargo_home: &Path) -> Result<PathBuf> {
    let bin = cargo_home.join(TOOLS_DIR).join("bin");
    fs::create_dir_all(&bin).with_context(|| format!("creating {}", bin.display()))?;
    for tool in Arch::host()?.tools() {
        let stamp = bin.join(format!(".{}-{}", tool.name, tool.version));
        let installed = bin.join(tool.name);
        if installed.is_file()
            && fs::read_to_string(&stamp)
                .is_ok_and(|recorded| sha256(&installed).is_ok_and(|now| now == recorded))
        {
            continue;
        }
        eprintln!("    release: installing {} {}", tool.name, tool.version);
        let work = cargo_home
            .join(TOOLS_DIR)
            .join(format!("{}-{}", tool.name, tool.version));
        fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
        let archive = work.join("archive");
        run(Command::new("curl")
            .args([
                "-sSfL",
                "--retry",
                "5",
                "--retry-delay",
                "2",
                "--retry-all-errors",
                "-o",
            ])
            .arg(&archive)
            .arg(tool.url))?;
        let actual = sha256(&archive)?;
        ensure!(
            actual == tool.sha256,
            "{} {}: SHA-256 {actual}, pinned {}",
            tool.name,
            tool.version,
            tool.sha256
        );
        let executable = match tool.member {
            Some(member) => {
                run(Command::new("tar")
                    .arg("-xf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(&work)
                    .arg(member))?;
                work.join(member)
            }
            None => archive,
        };
        fs::copy(&executable, &installed).with_context(|| format!("installing {}", tool.name))?;
        fs::set_permissions(&installed, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("making {} executable", tool.name))?;
        fs::write(&stamp, sha256(&installed)?)
            .with_context(|| format!("writing {}", stamp.display()))?;
    }
    Ok(bin)
}

/// The whole environment of a release build: [`PASSED_ENV`] from `inherited`, then the build's own
/// settings, which win. Nothing else reaches the build.
pub(crate) fn build_env(
    paths: &Paths,
    arch: Arch,
    path: OsString,
    epoch: &str,
    inherited: impl Fn(&str) -> Option<OsString>,
) -> BTreeMap<String, OsString> {
    let mut vars: BTreeMap<String, OsString> = PASSED_ENV
        .iter()
        .filter_map(|name| inherited(name).map(|value| ((*name).to_owned(), value)))
        .collect();
    let own: [(String, OsString); 7] = [
        ("PATH".to_owned(), path),
        (
            "CARGO_HOME".to_owned(),
            paths.cargo_home.clone().into_os_string(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            paths.rustup_home.clone().into_os_string(),
        ),
        ("RUSTFLAGS".to_owned(), rustflags(paths).into()),
        ("SOURCE_DATE_EPOCH".to_owned(), epoch.into()),
        ("CARGO_INCREMENTAL".to_owned(), "0".into()),
        (
            format!("CC_{}", arch.target().replace('-', "_")),
            TARGET_CC.into(),
        ),
    ];
    vars.extend(own);
    vars
}

/// Whether `name`, a variable of the environment `release build` was started in, could change
/// what a release compiles: rustflags in any of cargo's spellings, a compiler or a wrapper, or any
/// profile setting (DEC-2100 item 2, DEC-2027). [`build_env`] already keeps every one of them from
/// the build; refusing them makes a run that asked for one fail rather than ship without it.
pub(crate) fn overriding_variable(name: &str) -> bool {
    matches!(
        name,
        "RUSTFLAGS" | "RUSTC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER"
    ) || name.starts_with("CARGO_PROFILE_")
        || (name.starts_with("CARGO_") && name.ends_with("RUSTFLAGS"))
        || matches!(
            name,
            "CARGO_BUILD_RUSTC"
                | "CARGO_BUILD_RUSTC_WRAPPER"
                | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
        )
}

/// The cargo configuration files the build's `cargo` reads, in cargo's order: `.cargo/config.toml`
/// and `.cargo/config` in the checkout and each directory above it, then `cargo_home`'s. Only the
/// ones that exist are listed.
pub(crate) fn config_files(root: &Path, cargo_home: &Path) -> Vec<PathBuf> {
    root.ancestors()
        .map(|dir| dir.join(".cargo"))
        .chain(std::iter::once(cargo_home.to_path_buf()))
        .flat_map(|dir| [dir.join("config.toml"), dir.join("config")])
        .filter(|file| file.is_file())
        .collect()
}

/// The settings in one cargo configuration, `text`, that could change what a release compiles:
/// any `profile`, `env` or `include` (cargo follows an `include` without any `-Z` flag, so a file
/// it names could set any of these; DEC-2027 item 2), rustflags under `build` or any `target`, and
/// a compiler or wrapper under `build`. A `--config` argument would be one too, and
/// [`build_args`] passes none.
pub(crate) fn overriding_settings(text: &str) -> Result<Vec<String>> {
    let config: toml::Table = toml::from_str(text).context("parsing a cargo configuration")?;
    let mut found: Vec<String> = ["profile", "env", "include"]
        .into_iter()
        .filter(|key| config.contains_key(*key))
        .map(str::to_owned)
        .collect();
    if let Some(build) = config.get("build").and_then(toml::Value::as_table) {
        for key in [
            "rustflags",
            "rustc",
            "rustc-wrapper",
            "rustc-workspace-wrapper",
        ] {
            if build.contains_key(key) {
                found.push(format!("build.{key}"));
            }
        }
    }
    if let Some(targets) = config.get("target").and_then(toml::Value::as_table) {
        for (target, table) in targets {
            if table.get("rustflags").is_some() {
                found.push(format!("target.{target}.rustflags"));
            }
        }
    }
    Ok(found)
}

/// Every override that would make `release build` refuse: the variables of `names` that
/// [`overriding_variable`] names, then each setting [`overriding_settings`] finds in `configs`,
/// written `<file>: <key>`.
pub(crate) fn overrides(
    names: impl IntoIterator<Item = String>,
    configs: &[(PathBuf, String)],
) -> Result<Vec<String>> {
    let mut found: Vec<String> = names
        .into_iter()
        .filter(|name| overriding_variable(name))
        .collect();
    found.sort();
    for (file, text) in configs {
        for key in overriding_settings(text).with_context(|| format!("in {}", file.display()))? {
            found.push(format!("{}: {key}", file.display()));
        }
    }
    Ok(found)
}

/// Refuses when `names`, the variables of the environment, or a cargo configuration a build in
/// `root` with `cargo_home` would read holds one of [`overrides`] (DEC-2100 item 2, DEC-2027).
pub(crate) fn refuse_overrides(
    root: &Path,
    cargo_home: &Path,
    names: impl IntoIterator<Item = String>,
) -> Result<()> {
    let configs = config_files(root, cargo_home)
        .into_iter()
        .map(|file| {
            fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))
                .map(|text| (file, text))
        })
        .collect::<Result<Vec<_>>>()?;
    let refused = overrides(names, &configs)?;
    ensure!(
        refused.is_empty(),
        "the release build refuses settings that could change what it compiles, such as turning \
         debug assertions on (DEC-2100 item 2, DEC-2027): {}",
        refused.join(", ")
    );
    Ok(())
}

/// `cargo xtask release build <out>`. It runs [`refuse_overrides`] before it installs a tool or
/// builds anything.
pub(crate) fn build(root: &Path, out: &Path) -> Result<()> {
    let paths = Paths::from_env(root)?;
    refuse_overrides(
        root,
        &paths.cargo_home,
        env::vars_os().filter_map(|(name, _)| name.into_string().ok()),
    )?;
    let tools = tools(&paths.cargo_home)?;
    let epoch = git(root, &["log", "-1", "--format=%ct", "HEAD"])?;
    let commit = git(root, &["rev-parse", "HEAD"])?;
    let arch = Arch::host()?;
    run(Command::new("rustup").args(["target", "add", arch.target()]))?;
    let path = env::join_paths(
        std::iter::once(tools.clone())
            .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .context("joining PATH")?;
    let target_dir = root.join(TARGET_DIR);
    let vars = build_env(&paths, arch, path, &epoch, |name| env::var_os(name));
    for (program, args) in [
        ("rustc", ["-vV"]),
        (TARGET_CC, ["--version"]),
        ("ld", ["--version"]),
    ] {
        run(Command::new(program)
            .current_dir(root)
            .env_clear()
            .envs(&vars)
            .args(args))?;
    }
    let mut cargo = Command::new("cargo");
    cargo
        .env_clear()
        .envs(&vars)
        .current_dir(root)
        .args(build_args(&target_dir, arch));
    run(&mut cargo)?;
    let bin_out = out.join("bin");
    let sbom_out = out.join("sbom");
    for dir in [&bin_out, &sbom_out] {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut sums = BTreeMap::new();
    for (_, bin) in BINARIES {
        let built = target_dir.join(arch.target()).join("release").join(bin);
        let shipped = bin_out.join(bin);
        fs::copy(&built, &shipped).with_context(|| format!("copying {}", built.display()))?;
        sums.insert(bin.to_owned(), sha256(&shipped)?);
        run(Command::new(tools.join("syft"))
            .env("SYFT_CHECK_FOR_APP_UPDATE", "false")
            .args([
                "scan",
                "-q",
                "--source-name",
                bin,
                "--source-version",
                &commit,
                "-o",
            ])
            .arg(format!(
                "cyclonedx-json={}",
                sbom_out.join(format!("{bin}.cdx.json")).display()
            ))
            .arg(format!("file:{}", shipped.display())))?;
    }
    let text = sums_text(&sums);
    let sums_file = out.join("SHA256SUMS");
    fs::write(&sums_file, &text).with_context(|| format!("writing {}", sums_file.display()))?;
    eprint!("    release {commit} (SOURCE_DATE_EPOCH={epoch}):\n{text}");
    println!("digest={}", sha256(&sums_file)?);
    Ok(())
}

/// One drill run: the image, the variables it runs with, and the code it must print.
type DrillCase<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);

/// The binary the drill runs: the one that checks its signature at start today.
const DRILLED: &str = "mandate-agentd";

/// What a drilled binary prints on stderr once the check has let it start: it has no wired host.
const ADMITTED: &str = "deployment_host_not_wired";

/// The registry and the repository path the images are published under (H5: the repository's
/// GHCR, public images, no spend).
const REGISTRY: &str = "https://ghcr.io";
const REGISTRY_HOST: &str = "ghcr.io";
const REPOSITORY: &str = "kunwarshivam/mandate";

/// `cargo xtask release images <dist> <out> [--test-sign]` (E21-35): one image a binary of the
/// release `dist`, each checked against its `SHA256SUMS` line, written as an OCI layout to
/// `<out>/images/<binary>`, with `<out>/IMAGES` listing each image's digest. With `--test-sign`,
/// an ephemeral test key made here signs each binary (`<out>/signatures/<binary>.sig`, also the
/// image's second layer); its private half is deleted once the binaries are signed and its public
/// half kept as `<out>/test-key/cosign.pub`. Only the founder's hardware key signs a release for a
/// hosted cell, and it is never here.
pub(crate) fn images(root: &Path, dist: &Path, out: &Path, test_sign: bool) -> Result<()> {
    let arch = Arch::host()?;
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let sums = parse_sums(
        &fs::read_to_string(dist.join("SHA256SUMS"))
            .with_context(|| format!("reading {}/SHA256SUMS", dist.display()))?,
    )?;
    let key = if test_sign {
        Some(new_key(&tools, &out.join("test-key"))?)
    } else {
        None
    };
    let signatures = out.join("signatures");
    fs::create_dir_all(&signatures)
        .with_context(|| format!("creating {}", signatures.display()))?;
    let mut listing = String::new();
    for (name, hex) in &sums {
        let binary = dist.join("bin").join(name);
        ensure!(
            sha256(&binary)? == *hex,
            "{name} is not the binary SHA256SUMS lists"
        );
        let signature = match &key {
            Some(key) => {
                let file = signatures.join(format!("{name}.sig"));
                sign(&tools, key, &binary, &file)?;
                Some(fs::read(&file).with_context(|| format!("reading {}", file.display()))?)
            }
            None => None,
        };
        let bytes = fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
        let layout = image::layout(name, arch.oci(), &bytes, signature.as_deref(), &revision);
        image::write(&layout, &out.join("images").join(name))?;
        listing.push_str(&format!("sha256:{}  {name}\n", layout.manifest));
    }
    if let Some(key) = &key {
        fs::remove_file(key).with_context(|| format!("deleting {}", key.display()))?;
    }
    fs::write(out.join("IMAGES"), &listing).context("writing IMAGES")?;
    eprint!("    {} images of {revision}:\n{listing}", arch.oci());
    Ok(())
}

/// `cargo xtask release drill <dist> <out>` (E21-35): after `release images <dist> <out>
/// --test-sign`, runs [`DRILLED`] as each of three images would run it, unpacked as a runtime
/// does with the signature beside the binary: the test-signed image, the same binary unsigned,
/// and one signed by a key nothing trusts. Outside dev and CI the unsigned and the test-signed
/// images are refused; the wrongly signed one is refused even in CI with the test key; and in CI
/// the test-signed one (with the test key) and the unsigned one start.
pub(crate) fn drill(root: &Path, dist: &Path, out: &Path) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let binary = dist.join("bin").join(DRILLED);
    let bytes = fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
    let test_key = out.join("test-key").join("cosign.pub");
    let signed = fs::read(out.join("signatures").join(format!("{DRILLED}.sig")))
        .context("reading the test signature; run `release images --test-sign` first")?;
    let drill = out.join("drill");
    let _ = fs::remove_dir_all(&drill);
    let stranger = new_key(&tools, &drill.join("stranger"))?;
    let wrong_file = drill.join("wrong.sig");
    sign(&tools, &stranger, &binary, &wrong_file)?;
    fs::remove_file(&stranger).context("deleting the stranger's key")?;
    let wrong = fs::read(&wrong_file).context("reading the stranger's signature")?;
    let mut runs = BTreeMap::new();
    for (image, signature) in [
        ("signed", Some(signed.as_slice())),
        ("unsigned", None),
        ("wrong", Some(wrong.as_slice())),
    ] {
        let layout = image::layout(DRILLED, Arch::host()?.oci(), &bytes, signature, "drill");
        let dir = drill.join(image);
        image::write(&layout, &dir.join("layout"))?;
        image::unpack(&layout, &dir.join("layout"), &dir.join("rootfs"))?;
        runs.insert(image, dir.join("rootfs").join(DRILLED));
    }
    let key = test_key.display().to_string();
    let cases: [DrillCase<'_>; 7] = [
        (
            "signed",
            &[
                ("MANDATE_DEPLOYMENT_TIER", "ci"),
                ("MANDATE_RELEASE_TEST_KEY", &key),
            ],
            ADMITTED,
        ),
        ("signed", &[], "release_signature_not_verified"),
        (
            "signed",
            &[
                ("MANDATE_DEPLOYMENT_TIER", "staging"),
                ("MANDATE_RELEASE_TEST_KEY", &key),
            ],
            "release_signature_not_verified",
        ),
        ("unsigned", &[], "release_unsigned"),
        (
            "unsigned",
            &[("MANDATE_DEPLOYMENT_TIER", "production")],
            "release_unsigned",
        ),
        ("unsigned", &[("MANDATE_DEPLOYMENT_TIER", "ci")], ADMITTED),
        (
            "wrong",
            &[
                ("MANDATE_DEPLOYMENT_TIER", "ci"),
                ("MANDATE_RELEASE_TEST_KEY", &key),
            ],
            "release_signature_not_verified",
        ),
    ];
    for (image, vars, expected) in cases {
        let program = runs
            .get(image)
            .context("an image the drill did not build")?;
        let output = Command::new(program)
            .env_clear()
            .envs(vars.iter().copied())
            .output()
            .with_context(|| format!("running the {image} image"))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        ensure!(
            output.status.code() == Some(2) && stderr.trim_end() == expected,
            "the {image} image with {vars:?}: expected {expected}, got {:?} and {stderr:?}",
            output.status.code()
        );
        eprintln!("    drill: the {image} image with {vars:?}: {expected}");
    }
    Ok(())
}

/// The tag of one arch's image of commit `revision`.
pub(crate) fn arch_tag(revision: &str, arch: Arch) -> String {
    format!("main-{revision}-{}", arch.oci())
}

/// The tag of commit `revision`'s multi-arch index, the one a cell pulls.
pub(crate) fn index_tag(revision: &str) -> String {
    format!("main-{revision}")
}

/// A signed-in session with GHCR: the 0600 file holding the Basic header the registry trades for
/// push tokens, in a 0700 directory deleted when the session ends, and cosign logged in through its
/// stdin. The token is read from `GH_TOKEN` and reaches nothing else.
struct Registry {
    secrets: PathBuf,
    basic: PathBuf,
}

impl Registry {
    fn sign_in(tools: &Path, out: &Path) -> Result<Self> {
        let token = env::var("GH_TOKEN").context("GH_TOKEN is not set")?;
        let actor = env::var("GITHUB_ACTOR").context("GITHUB_ACTOR is not set")?;
        let secrets = out.join("auth");
        fs::create_dir_all(&secrets).context("creating the auth directory")?;
        fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700))?;
        let basic = secrets.join("basic");
        write_secret(
            &basic,
            &format!(
                "Authorization: Basic {}\n",
                image::base64(format!("{actor}:{token}").as_bytes())
            ),
        )?;
        let mut login = Command::new(tools.join("cosign"))
            .args([
                "login",
                REGISTRY_HOST,
                "--username",
                &actor,
                "--password-stdin",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(io::stderr())
            .spawn()
            .context("starting `cosign login`")?;
        if let Some(mut stdin) = login.stdin.take() {
            io::Write::write_all(&mut stdin, token.as_bytes())
                .context("handing cosign the token")?;
        }
        ensure!(login.wait()?.success(), "cosign login failed");
        Ok(Self { secrets, basic })
    }

    /// A header file carrying a push token for `repository`.
    fn bearer(&self, repository: &str) -> Result<PathBuf> {
        let token = image_token(&self.basic, repository)?;
        let bearer = self.secrets.join("bearer");
        write_secret(&bearer, &format!("Authorization: Bearer {token}\n"))?;
        Ok(bearer)
    }

    fn sign_out(self) -> Result<()> {
        fs::remove_dir_all(&self.secrets).context("deleting the auth directory")
    }
}

/// `cargo xtask release publish <dist> <out>` (E21-35, DEC-1060): only from a push to `main`,
/// where CI's per-arch `publish` jobs run it (ES-12), each on a runner of its own arch. Builds the
/// host arch's images of `dist` without a binary signature (the founder's hardware key is not
/// wired, so no binary is release-signed and none starts outside dev and CI), pushes each to GHCR
/// as `main-<commit>-<arch>`, checks the registry reports the image's own digest, signs each
/// image's digest keyless with cosign (the run's GitHub OIDC identity) and attaches its SBOM as a
/// CycloneDX attestation, and writes `<out>/PUBLISHED`.
pub(crate) fn publish(root: &Path, dist: &Path, out: &Path) -> Result<()> {
    let arch = Arch::host()?;
    images(root, dist, out, false)?;
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let registry = Registry::sign_in(&tools, out)?;
    let sums = parse_sums(&fs::read_to_string(dist.join("SHA256SUMS"))?)?;
    let mut published = String::new();
    for name in sums.keys() {
        let repository = format!("{REPOSITORY}/{name}");
        let bearer = registry.bearer(&repository)?;
        let dir = out.join("images").join(name);
        let bytes = fs::read(dist.join("bin").join(name))?;
        let layout = image::layout(name, arch.oci(), &bytes, None, &revision);
        let digest = image::push(
            &layout,
            &dir,
            REGISTRY,
            &repository,
            &arch_tag(&revision, arch),
            Some(&bearer),
        )?;
        let reference = format!("{REGISTRY_HOST}/{repository}@{digest}");
        run(Command::new(tools.join("cosign")).args(["sign", "--yes", &reference]))?;
        run(Command::new(tools.join("cosign"))
            .args(["attest", "--yes", "--type", "cyclonedx", "--predicate"])
            .arg(dist.join("sbom").join(format!("{name}.cdx.json")))
            .arg(&reference))?;
        published.push_str(&format!("{reference}  {name}  {}\n", arch.oci()));
    }
    registry.sign_out()?;
    fs::write(out.join("PUBLISHED"), &published).context("writing PUBLISHED")?;
    eprint!("    published {revision}:\n{published}");
    Ok(())
}

/// `cargo xtask release publish-index <out>` (DEC-1060): only from a push to `main`, after every
/// arch's `publish` job. For each release binary it reads each arch's `main-<commit>-<arch>` image
/// back from GHCR by the registry's own digest and size, pushes the OCI index over them as
/// `main-<commit>`, checks the registry reports the index's digest, signs the index digest
/// keyless, and writes `<out>/PUBLISHED`: the one reference per binary a cell pulls on any arch.
pub(crate) fn publish_index(root: &Path, out: &Path) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let registry = Registry::sign_in(&tools, out)?;
    let mut published = String::new();
    for (_, name) in BINARIES {
        let repository = format!("{REPOSITORY}/{name}");
        let bearer = registry.bearer(&repository)?;
        let mut platforms = Vec::new();
        for arch in Arch::ALL {
            let (manifest, size) = image::remote_manifest(
                REGISTRY,
                &repository,
                &arch_tag(&revision, arch),
                Some(&bearer),
            )?;
            platforms.push(image::Platform {
                architecture: arch.oci().to_owned(),
                manifest,
                size,
            });
        }
        let (bytes, digest) = image::index(&platforms)?;
        let reported = image::push_index(
            &bytes,
            &digest,
            &out.join("index"),
            REGISTRY,
            &repository,
            &index_tag(&revision),
            Some(&bearer),
        )?;
        let reference = format!("{REGISTRY_HOST}/{repository}@{reported}");
        run(Command::new(tools.join("cosign")).args(["sign", "--yes", &reference]))?;
        published.push_str(&format!("{reference}  {name}\n"));
    }
    registry.sign_out()?;
    fs::write(out.join("PUBLISHED"), &published).context("writing PUBLISHED")?;
    eprint!("    published the index of {revision}:\n{published}");
    Ok(())
}

/// A registry token for pushing to `repository`, exchanged for the Basic header in `basic`.
fn image_token(basic: &Path, repository: &str) -> Result<String> {
    let out = Command::new("curl")
        .args(["-sSf", "-H"])
        .arg(format!("@{}", basic.display()))
        .arg(format!(
            "{REGISTRY}/token?service={REGISTRY_HOST}&scope=repository:{repository}:pull,push"
        ))
        .output()
        .context("starting `curl`")?;
    ensure!(
        out.status.success(),
        "the registry refused a token for {repository}"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&out.stdout).context("the registry's token is not JSON")?;
    body.get("token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .context("the registry's answer has no token")
}

/// Writes `text` to `path`, readable by this user only.
fn write_secret(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))
}

/// Makes an ephemeral cosign key pair, with no password, in `dir`, and returns its private key's
/// path; the caller deletes it once it has signed.
fn new_key(tools: &Path, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    run(Command::new(tools.join("cosign"))
        .current_dir(dir)
        .env("COSIGN_PASSWORD", "")
        .arg("generate-key-pair"))?;
    Ok(dir.join("cosign.key"))
}

/// Signs `file` with the cosign key `key` into `signature`, offline: no transparency log, the
/// signature file cosign's `--output-signature` writes.
fn sign(tools: &Path, key: &Path, file: &Path, signature: &Path) -> Result<()> {
    run(Command::new(tools.join("cosign"))
        .env("COSIGN_PASSWORD", "")
        .args([
            "sign-blob",
            "--yes",
            "--new-bundle-format=false",
            "--use-signing-config=false",
            "--tlog-upload=false",
            "--key",
        ])
        .arg(key)
        .arg("--output-signature")
        .arg(signature)
        .arg(file))
}

/// The SHA-256 of the file at `path`, as `sha256sum` prints it.
fn sha256(path: &Path) -> Result<String> {
    let out = Command::new("sha256sum")
        .arg(path)
        .output()
        .context("starting `sha256sum`")?;
    ensure!(
        out.status.success(),
        "`sha256sum {}` failed",
        path.display()
    );
    let text = String::from_utf8(out.stdout).context("sha256sum printed non-UTF-8")?;
    text.split_whitespace()
        .next()
        .map(str::to_owned)
        .with_context(|| format!("sha256sum printed nothing for {}", path.display()))
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("starting `git`")?;
    ensure!(out.status.success(), "`git {}` failed", args.join(" "));
    Ok(String::from_utf8(out.stdout)
        .context("git printed non-UTF-8")?
        .trim()
        .to_owned())
}

/// Runs `command` with its stdout sent to stderr, so the build's stdout holds only its
/// `digest=` line for `$GITHUB_OUTPUT`.
fn run(command: &mut Command) -> Result<()> {
    eprintln!("    $ {command:?}");
    let status = command
        .stdout(io::stderr())
        .status()
        .with_context(|| format!("starting {command:?}"))?;
    if !status.success() {
        bail!("{command:?} failed with {status}");
    }
    Ok(())
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
    use crate::repo_root;

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
        for arch in Arch::ALL {
            locked_static_and_featureless(arch);
        }
    }

    fn locked_static_and_featureless(arch: Arch) {
        let args = build_args(Path::new("/t"), arch);
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
        assert_eq!(after("--target").as_deref(), Some(arch.target()));
        assert!(arch.target().ends_with("-unknown-linux-musl"), "{arch:?}");
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
        let policy: toml::Table =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("layers.toml"))?
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

    /// The build sees the proxy and home settings it inherits and its own settings, and nothing
    /// else: no wrapper, no inherited rustflags, nothing `cargo run` handed xtask.
    #[test]
    fn the_build_environment_is_closed() {
        let inherited = |name: &str| -> Option<OsString> {
            match name {
                "HOME"
                | "HTTPS_PROXY"
                | "RUSTC_WRAPPER"
                | "RUSTC_WORKSPACE_WRAPPER"
                | "CARGO_ENCODED_RUSTFLAGS"
                | "CARGO_BUILD_RUSTFLAGS"
                | "RUSTUP_TOOLCHAIN"
                | "CARGO_MANIFEST_DIR"
                | "RUSTFLAGS"
                | "CARGO_HOME"
                | "PATH" => Some(OsString::from(format!("inherited-{name}"))),
                _ => None,
            }
        };
        let vars = build_env(
            &paths(),
            Arch::Amd64,
            OsString::from("/tools:/usr/bin"),
            "1792212600",
            inherited,
        );
        let names: Vec<&str> = vars.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "CARGO_HOME",
                "CARGO_INCREMENTAL",
                "CC_x86_64_unknown_linux_musl",
                "HOME",
                "HTTPS_PROXY",
                "PATH",
                "RUSTFLAGS",
                "RUSTUP_HOME",
                "SOURCE_DATE_EPOCH",
            ]
        );
        let value = |name: &str| vars.get(name).and_then(|v| v.to_str()).map(str::to_owned);
        assert_eq!(value("RUSTFLAGS"), Some(rustflags(&paths())));
        assert_eq!(value("CARGO_HOME").as_deref(), Some("/home/runner/.cargo"));
        assert_eq!(value("PATH").as_deref(), Some("/tools:/usr/bin"));
        assert_eq!(value("SOURCE_DATE_EPOCH").as_deref(), Some("1792212600"));
        assert_eq!(value("HOME").as_deref(), Some("inherited-HOME"));
    }

    /// DEC-2027: every spelling of rustflags, a compiler or wrapper, and every profile variable is
    /// refused, and what `cargo run`, rustup and a runner set is not.
    #[test]
    fn variables_that_could_change_the_build_are_refused() {
        for name in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS",
            "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS",
            "CARGO_PROFILE_RELEASE_PACKAGE_MANDATE_CLI_DEBUG_ASSERTIONS",
            "CARGO_PROFILE_RELEASE_OPT_LEVEL",
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        ] {
            assert!(overriding_variable(name), "{name} is refused");
        }
        for name in [
            "CARGO",
            "CARGO_HOME",
            "CARGO_MANIFEST_DIR",
            "CARGO_PKG_NAME",
            "CARGO_TERM_COLOR",
            "CARGO_HTTP_CAINFO",
            "RUSTUP_HOME",
            "RUSTUP_TOOLCHAIN",
            "RUSTDOCFLAGS",
            "HOME",
            "PATH",
            "HTTPS_PROXY",
        ] {
            assert!(!overriding_variable(name), "{name} is not refused");
        }
    }

    /// DEC-2027: a cargo configuration that sets a profile, rustflags, or a compiler or wrapper
    /// is refused, naming each setting and its file; the repository's own configuration, an
    /// alias, and registry or network settings are not.
    #[test]
    fn configurations_that_could_change_the_build_are_refused() -> Result<()> {
        let repo = fs::read_to_string(repo_root()?.join(".cargo/config.toml"))?;
        assert_eq!(overriding_settings(&repo)?, Vec::<String>::new());
        assert_eq!(
            overriding_settings(
                "[alias]\nx = \"run\"\n[http]\ncainfo = \"/ca.pem\"\n[net]\nretry = 3\n\
                 [build]\njobs = 4\n[target.x86_64-unknown-linux-musl]\nlinker = \"cc\"\n"
            )?,
            Vec::<String>::new()
        );
        let seeded = "include = [\"more.toml\"]\n[env]\nX = \"1\"\n\
             [profile.release]\ndebug-assertions = true\n\
             [build]\nrustflags = [\"-C\", \"debug-assertions\"]\nrustc-wrapper = \"w\"\n\
             rustc = \"r\"\nrustc-workspace-wrapper = \"ww\"\n\
             [target.x86_64-unknown-linux-musl]\nrustflags = [\"-Cdebug-assertions\"]\n\
             [target.'cfg(unix)']\nrustflags = []\n";
        assert_eq!(
            overriding_settings(seeded)?,
            [
                "profile",
                "env",
                "include",
                "build.rustflags",
                "build.rustc",
                "build.rustc-wrapper",
                "build.rustc-workspace-wrapper",
                "target.cfg(unix).rustflags",
                "target.x86_64-unknown-linux-musl.rustflags",
            ]
        );
        assert_eq!(
            overrides(
                [
                    "PATH".to_owned(),
                    "RUSTFLAGS".to_owned(),
                    "CARGO_PROFILE_RELEASE_LTO".to_owned()
                ],
                &[
                    (PathBuf::from("/repo/.cargo/config.toml"), repo),
                    (
                        PathBuf::from("/home/runner/.cargo/config.toml"),
                        "[profile.release]\ndebug-assertions = true\n".to_owned()
                    ),
                ],
            )?,
            [
                "CARGO_PROFILE_RELEASE_LTO",
                "RUSTFLAGS",
                "/home/runner/.cargo/config.toml: profile",
            ]
        );
        assert!(overrides(Vec::new(), &[(PathBuf::from("/bad"), "[".to_owned())]).is_err());
        Ok(())
    }

    /// DEC-2027: the configuration files are the checkout's and each parent's, in cargo's order,
    /// then `CARGO_HOME`'s, and only those that exist.
    #[test]
    fn the_configuration_files_are_the_ones_cargo_reads() -> Result<()> {
        let base = env::temp_dir().join(format!("mandate-release-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("work").join("mandate");
        let home = base.join("cargo-home");
        for dir in [root.join(".cargo"), base.join(".cargo"), home.clone()] {
            fs::create_dir_all(&dir)?;
        }
        fs::write(root.join(".cargo/config.toml"), "")?;
        fs::write(base.join(".cargo/config"), "")?;
        fs::write(home.join("config.toml"), "")?;
        fs::write(home.join("config"), "")?;
        let found: Vec<PathBuf> = config_files(&root, &home)
            .into_iter()
            .filter(|file| file.starts_with(&base))
            .collect();
        let _ = fs::remove_dir_all(&base);
        assert_eq!(
            found,
            [
                root.join(".cargo/config.toml"),
                base.join(".cargo/config"),
                home.join("config.toml"),
                home.join("config"),
            ],
            "the checkout's, then each parent's, then CARGO_HOME's, `config.toml` before `config`"
        );
        Ok(())
    }

    /// DEC-2027: `refuse_overrides` reads the files cargo would read and refuses a variable or a
    /// setting in any of them, including a file that only includes another; a clean checkout and
    /// environment pass.
    #[test]
    fn overrides_are_refused_from_the_environment_and_every_configuration() -> Result<()> {
        let base = env::temp_dir().join(format!("mandate-release-refuse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("mandate");
        let home = base.join("cargo-home");
        fs::create_dir_all(root.join(".cargo"))?;
        fs::create_dir_all(&home)?;
        fs::write(root.join(".cargo/config.toml"), "[alias]\nx = \"run\"\n")?;
        let clean = refuse_overrides(&root, &home, ["PATH".to_owned(), "HOME".to_owned()]);
        let variable = refuse_overrides(&root, &home, ["RUSTFLAGS".to_owned()]);
        fs::write(home.join("config.toml"), "include = [\"release.toml\"]\n")?;
        let included = refuse_overrides(&root, &home, Vec::new());
        let _ = fs::remove_dir_all(&base);
        assert!(clean.is_ok(), "{clean:?}");
        for (refused, named) in [(variable, "RUSTFLAGS"), (included, "config.toml: include")] {
            let message = format!("{:#}", refused.err().context("a refusal")?);
            assert!(
                message.contains("DEC-2027") && message.contains(named),
                "{message}"
            );
        }
        Ok(())
    }

    /// DEC-2027 item 4: `release build` refuses before it installs a tool, reads git, or builds:
    /// a checkout whose configuration sets a profile is refused with the refusal, not with any
    /// later step's error.
    #[test]
    fn the_build_refuses_before_anything_runs() -> Result<()> {
        let base = env::temp_dir().join(format!("mandate-release-first-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("mandate");
        fs::create_dir_all(root.join(".cargo"))?;
        fs::write(
            root.join(".cargo/config.toml"),
            "[profile.release]\ndebug-assertions = true\n",
        )?;
        let built = build(&root, &base.join("out"));
        let wrote = base.join("out").exists();
        let _ = fs::remove_dir_all(&base);
        let message = format!("{:#}", built.err().context("a refusal")?);
        assert!(
            message.contains("DEC-2027") && message.contains("config.toml: profile"),
            "{message}"
        );
        assert!(!wrote, "a refused build writes nothing");
        Ok(())
    }

    /// DEC-2027: the release build's own `cargo` arguments pass no `--config` and no `-Z`, so no
    /// setting reaches it from its command line.
    #[test]
    fn the_build_passes_no_configuration_on_its_command_line() {
        let args = build_args(Path::new("/t"), Arch::Amd64);
        assert!(
            args.iter()
                .all(|arg| !arg.starts_with("--config") && !arg.starts_with("-Z")),
            "{args:?}"
        );
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

    /// The jobs of `release.yml`, by name, each with its lines.
    fn release_jobs() -> Result<BTreeMap<String, String>> {
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml"),
        )?;
        let jobs = text
            .split_once("\njobs:\n")
            .map(|(_, jobs)| jobs)
            .context("release.yml has no jobs")?;
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        let mut current = None;
        for line in jobs.lines() {
            if let Some(name) = line
                .strip_prefix("  ")
                .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
                .and_then(|rest| rest.strip_suffix(':'))
            {
                current = Some(name.to_owned());
                out.insert(name.to_owned(), String::new());
            } else if let Some(name) = &current {
                let body = out.entry(name.clone()).or_default();
                body.push_str(line);
                body.push('\n');
            }
        }
        Ok(out)
    }

    /// The jobs that publish, one a runner of each arch and the index over them.
    const PUBLISH_JOBS: [&str; 3] = ["publish-amd64", "publish-arm64", "publish-index"];

    /// Only the publish jobs may write packages or mint an OIDC token for keyless signing, and
    /// they run only on a push to `main`: never from a pull request or a branch (ES-12, E21-35).
    /// Each arch publishes from a runner of its own arch, after its builds compared, and the index
    /// only after every arch has published (DEC-1060).
    #[test]
    fn only_the_main_publish_jobs_can_write_packages_or_sign() -> Result<()> {
        let jobs = release_jobs()?;
        for (name, body) in &jobs {
            let writes = body.contains("packages: write") || body.contains("id-token: write");
            assert_eq!(writes, PUBLISH_JOBS.contains(&name.as_str()), "{name}");
        }
        for name in PUBLISH_JOBS {
            let job = jobs
                .get(name)
                .with_context(|| format!("release.yml has no {name}"))?;
            assert!(
                job.contains(
                    "    if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}\n"
                ),
                "{name}: {job}"
            );
            for permission in ["contents: read", "packages: write", "id-token: write"] {
                assert!(job.contains(permission), "{name}: {permission}");
            }
        }
        for arch in Arch::ALL {
            let name = format!("publish-{}", arch.oci());
            let job = jobs.get(&name).with_context(|| format!("no {name}"))?;
            assert!(
                job.contains(&format!("needs: [build-a-{}, compare]", arch.oci())),
                "{name}: {job}"
            );
            assert!(
                job.contains(&format!("runs-on: {}\n", runner(arch))),
                "{name}: {job}"
            );
        }
        let index = jobs.get("publish-index").context("no publish-index")?;
        assert!(
            index.contains("needs: [publish-amd64, publish-arm64]"),
            "{index}"
        );
        Ok(())
    }

    fn runner(arch: Arch) -> &'static str {
        match arch {
            Arch::Amd64 => "ubuntu-24.04",
            Arch::Arm64 => "ubuntu-24.04-arm",
        }
    }

    /// Every arch is built twice, on two runners of its own arch, and compared; nothing is built
    /// for one arch on another's runner (DEC-1060).
    #[test]
    fn every_arch_is_built_twice_on_its_own_runners_and_compared() -> Result<()> {
        let jobs = release_jobs()?;
        let compare = jobs.get("compare").context("no compare")?;
        for arch in Arch::ALL {
            for copy in ["a", "b"] {
                let name = format!("build-{copy}-{}", arch.oci());
                let job = jobs.get(&name).with_context(|| format!("no {name}"))?;
                assert!(
                    job.contains(&format!("runs-on: {}\n", runner(arch))),
                    "{name}"
                );
                assert!(compare.contains(&name), "compare does not need {name}");
            }
            let a = format!("needs.build-a-{}.outputs.digest", arch.oci());
            let b = format!("needs.build-b-{}.outputs.digest", arch.oci());
            assert!(
                compare.contains(&format!("{a} == '' || {a} != {b}")),
                "compare does not compare {:?}: {compare}",
                arch
            );
        }
        Ok(())
    }

    #[test]
    fn rust_names_map_to_release_arches() {
        assert_eq!(Arch::of("x86_64"), Some(Arch::Amd64));
        assert_eq!(Arch::of("aarch64"), Some(Arch::Arm64));
        for other in ["arm", "x86", "riscv64", "", "amd64", "arm64"] {
            assert_eq!(Arch::of(other), None, "{other}");
        }
        assert_eq!(Arch::Amd64.oci(), "amd64");
        assert_eq!(Arch::Arm64.oci(), "arm64");
        assert_eq!(Arch::Amd64.target(), "x86_64-unknown-linux-musl");
        assert_eq!(Arch::Arm64.target(), "aarch64-unknown-linux-musl");
    }

    /// Each arch's tools are its own builds of the same versions: the asset names the arch, and no
    /// arch shares another's checksum.
    #[test]
    fn each_arch_has_its_own_builds_of_the_same_tools() {
        let names = |arch: Arch| arch.tools().map(|tool| (tool.name, tool.version));
        assert_eq!(names(Arch::Amd64), names(Arch::Arm64));
        for (amd, arm) in Arch::Amd64.tools().iter().zip(Arch::Arm64.tools().iter()) {
            assert!(
                amd.url.contains("x86_64") || amd.url.contains("amd64"),
                "{}",
                amd.url
            );
            assert!(
                arm.url.contains("aarch64") || arm.url.contains("arm64"),
                "{}",
                arm.url
            );
            assert_ne!(amd.sha256, arm.sha256, "{}", amd.name);
        }
    }

    #[test]
    fn tags_name_the_commit_and_the_arch() {
        assert_eq!(arch_tag("abc", Arch::Amd64), "main-abc-amd64");
        assert_eq!(arch_tag("abc", Arch::Arm64), "main-abc-arm64");
        assert_eq!(index_tag("abc"), "main-abc");
    }

    /// Each tool is pinned to a GitHub release asset of its own version, with a SHA-256.
    #[test]
    fn every_tool_is_a_pinned_github_release_asset() {
        for tool in Arch::ALL.iter().flat_map(|arch| arch.tools()) {
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
        for tool in Arch::ALL.iter().flat_map(|arch| arch.tools()) {
            let row = registry
                .lines()
                .find(|line| line.starts_with(&format!("| `{}` |", tool.name)))
                .with_context(|| format!("no registry row for {}", tool.name))?;
            assert!(row.contains(tool.version), "{}: {row}", tool.name);
        }
        Ok(())
    }
}
