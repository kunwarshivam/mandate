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
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

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
/// `mandate-api-server` is the workspace API server a hosted cell runs as its chart's `api`
/// process, so its image must be signed and published like the others (DEC-2026).
/// `mandate-deployd`, `mandate-daemon` and `mandate-executord` are the deployment manager, the agent
/// runtime and the account executor a hosted cell runs, released the same way; the runtime's binary
/// is built by `mandate-runtime-models`, which links its model composer (DEC-2383). The runtime's image
/// holds no recorded research bundle, which only the local cell's interim image carries (DEC-2246).
/// `mandate-provisionerd` is the role provisioner a hosted cell runs, released the same way
/// (DEC-2350), and `mandate-token-exchange` the OAuth connect's token exchange (DEC-821, DEC-1175),
/// whose release build refuses a token URL of its own (DEC-2633 item 2). `mandate-scheduler`,
/// `mandate-dispatcher`, `mandate-datad` and `mandate-gatewayd` are the scheduler, the alert
/// dispatcher, the data process and the model gateway a hosted cell runs, released the same way so
/// every process the chart builds has a signed digest to promote (DEC-2079); the gateway's image
/// holds no recorded research bundle, which only the local cell's interim image carries.
/// `mandate-news-reader` is the news reader staging runs once its key and followed agents are set
/// (DEC-2084 item 8).
pub(crate) const BINARIES: [(&str, &str); 14] = [
    ("mandate-cli", "mandate"),
    ("mandate-agentd", "mandate-agentd"),
    ("mandate-rh-sim", "mandate-rh-sim"),
    ("mandate-api-server", "mandate-api-server"),
    ("mandate-deployd", "mandate-deployd"),
    ("mandate-runtime-models", "mandate-daemon"),
    ("mandate-executord", "mandate-executord"),
    ("mandate-provisionerd", "mandate-provisionerd"),
    ("mandate-token-exchange", "mandate-token-exchange"),
    ("mandate-scheduler", "mandate-scheduler"),
    ("mandate-dispatcher", "mandate-dispatcher"),
    ("mandate-datad", "mandate-datad"),
    ("mandate-gatewayd", "mandate-gatewayd"),
    ("mandate-news-reader", "mandate-news-reader"),
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
/// half kept as `<out>/test-key/cosign.pub`. Only the release key signs a release for a hosted
/// cell, in CI's `release-signing` job (DEC-2024), and it is never here.
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
        ensure!(
            !image::carries_cell_research(&layout),
            "the {name} release image holds /{}, the local cell's recorded research, which only \
             its interim runtime image carries (DEC-2246)",
            image::CELL_RESEARCH
        );
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

/// The tag of one arch's release-signed image of commit `revision` (DEC-2026 item 3).
pub(crate) fn signed_arch_tag(revision: &str, arch: Arch) -> String {
    format!("signed-{revision}-{}", arch.oci())
}

/// The tag of commit `revision`'s release-signed multi-arch index, the one a hosted cell pulls
/// (DEC-2026 item 3).
pub(crate) fn signed_index_tag(revision: &str) -> String {
    format!("signed-{revision}")
}

/// A signed-in session with GHCR: the 0600 file holding the Basic header the registry trades for
/// push tokens, in a 0700 directory deleted when the session ends, and cosign logged in through its
/// stdin. The token is read from `GH_TOKEN` and reaches nothing else.
struct Registry {
    secrets: SecretDir,
    basic: PathBuf,
    /// The same credential as a registry config file in Docker's format, the one `helm push`
    /// reads (`--registry-config`).
    helm: PathBuf,
}

impl Registry {
    fn sign_in(tools: &Path) -> Result<Self> {
        let token = env::var("GH_TOKEN").ok().context("GH_TOKEN is not set")?;
        let actor = env::var("GITHUB_ACTOR").context("GITHUB_ACTOR is not set")?;
        let secrets = SecretDir::create()?;
        let basic = secrets.0.join("basic");
        let auth = image::base64(format!("{actor}:{token}").as_bytes());
        write_secret(&basic, &format!("Authorization: Basic {auth}\n"))?;
        let helm = secrets.0.join("helm-registry.json");
        write_secret(
            &helm,
            &format!("{{\"auths\":{{\"{REGISTRY_HOST}\":{{\"auth\":\"{auth}\"}}}}}}\n"),
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
        Ok(Self {
            secrets,
            basic,
            helm,
        })
    }

    /// A header file carrying a push token for `repository`.
    fn bearer(&self, repository: &str) -> Result<PathBuf> {
        let token = image_token(&self.basic, repository)?;
        let bearer = self.secrets.0.join("bearer");
        write_secret(&bearer, &format!("Authorization: Bearer {token}\n"))?;
        Ok(bearer)
    }

    fn sign_out(self) -> Result<()> {
        fs::remove_dir_all(&self.secrets.0).context("deleting the auth directory")
    }
}

/// The 0700 directory a registry session's credentials live in: under the runner's own temporary
/// directory (`RUNNER_TEMP`, else the system's), never under an output directory a job uploads,
/// and deleted when dropped, so a failure on any path out leaves no credential behind.
struct SecretDir(PathBuf);

impl SecretDir {
    fn create() -> Result<Self> {
        let base = env::var_os("RUNNER_TEMP").map_or_else(env::temp_dir, PathBuf::from);
        let dir = base.join(format!("mandate-registry-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).context("creating the auth directory")?;
        let secrets = Self(dir);
        fs::set_permissions(&secrets.0, fs::Permissions::from_mode(0o700))?;
        Ok(secrets)
    }
}

impl Drop for SecretDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `cargo xtask release publish <dist> <out>` (E21-35, DEC-1060): only from a push to `main`,
/// where CI's per-arch `publish` jobs run it (ES-12), each on a runner of its own arch. Builds the
/// host arch's images of `dist` without a binary signature (a push signs nothing with the release
/// key, so no published binary starts outside dev and CI; DEC-2024), pushes each to GHCR
/// as `main-<commit>-<arch>`, checks the registry reports the image's own digest, signs each
/// image's digest keyless with cosign (the run's GitHub OIDC identity) and attaches its SBOM as a
/// CycloneDX attestation, and writes `<out>/PUBLISHED`.
pub(crate) fn publish(root: &Path, dist: &Path, out: &Path) -> Result<()> {
    let arch = Arch::host()?;
    images(root, dist, out, false)?;
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let registry = Registry::sign_in(&tools)?;
    let sums = parse_sums(&fs::read_to_string(dist.join("SHA256SUMS"))?)?;
    let mut published = String::new();
    for name in sums.keys() {
        let bytes = fs::read(dist.join("bin").join(name))?;
        let layout = image::layout(name, arch.oci(), &bytes, None, &revision);
        let tag = arch_tag(&revision, arch);
        published.push_str(&push_image(
            &tools,
            &registry,
            dist,
            out,
            (name, &layout),
            &tag,
            arch,
        )?);
    }
    registry.sign_out()?;
    fs::write(out.join("PUBLISHED"), &published).context("writing PUBLISHED")?;
    eprint!("    published {revision}:\n{published}");
    Ok(())
}

/// Pushes `layout`, the image of the binary `name`, to GHCR as `tag`, checks the registry reports
/// its own digest, signs that digest keyless with cosign (the run's GitHub OIDC identity), attaches
/// `dist`'s SBOM of the binary as a CycloneDX attestation, and returns its `PUBLISHED` line.
fn push_image(
    tools: &Path,
    registry: &Registry,
    dist: &Path,
    out: &Path,
    (name, layout): (&str, &image::Layout),
    tag: &str,
    arch: Arch,
) -> Result<String> {
    let repository = format!("{REPOSITORY}/{name}");
    let bearer = registry.bearer(&repository)?;
    let dir = out.join("images").join(name);
    let digest = image::push(layout, &dir, REGISTRY, &repository, tag, Some(&bearer))?;
    let reference = format!("{REGISTRY_HOST}/{repository}@{digest}");
    sign_keyless(&tools.join("cosign"), &reference, SIGN_RETRY_WAIT)?;
    attest_sbom(
        &tools.join("cosign"),
        &dist.join("sbom").join(format!("{name}.cdx.json")),
        &reference,
        SIGN_RETRY_WAIT,
    )?;
    Ok(format!("{reference}  {name}  {}\n", arch.oci()))
}

/// `cargo xtask release publish-index <out>` (DEC-1060): only from a push to `main`, after every
/// arch's `publish` job. For each release binary it reads each arch's `main-<commit>-<arch>` image
/// back from GHCR by the registry's own digest and size, pushes the OCI index over them as
/// `main-<commit>`, checks the registry reports the index's digest, signs the index digest
/// keyless, and writes `<out>/PUBLISHED`: the one reference per binary a cell pulls on any arch.
pub(crate) fn publish_index(root: &Path, out: &Path) -> Result<()> {
    push_indexes(root, out, arch_tag, index_tag)
}

/// `cargo xtask release publish-signed-index <out>` (DEC-2026 item 3): only on a dispatch from
/// `main`, after every arch's `release-signing-publish` job. As [`publish_index`], over each arch's
/// [`signed_arch_tag`] image, pushed as [`signed_index_tag`].
pub(crate) fn publish_signed_index(root: &Path, out: &Path) -> Result<()> {
    push_indexes(root, out, signed_arch_tag, signed_index_tag)
}

/// For each release binary, reads each arch's image tagged `arch_tag` back from GHCR by the
/// registry's own digest and size, pushes the OCI index over them as `index_tag`, checks the
/// registry reports the index's digest, signs it keyless, and writes `<out>/PUBLISHED`.
fn push_indexes(
    root: &Path,
    out: &Path,
    arch_tag: fn(&str, Arch) -> String,
    index_tag: fn(&str) -> String,
) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let registry = Registry::sign_in(&tools)?;
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
        sign_keyless(&tools.join("cosign"), &reference, SIGN_RETRY_WAIT)?;
        published.push_str(&format!("{reference}  {name}\n"));
    }
    registry.sign_out()?;
    fs::write(out.join("PUBLISHED"), &published).context("writing PUBLISHED")?;
    eprint!("    published the index of {revision}:\n{published}");
    Ok(())
}

/// `cargo xtask release publish-signed <dist> <signatures> <out>` (E21-35, DEC-2026): only on a
/// dispatch from `main`, where CI's per-arch `release-signing-publish` job runs it after
/// `release-signing-check`, holding no release secret. [`signed_images`] checks every signature
/// against the committed public half before anything is built, and only then does it sign in,
/// push each image as [`signed_arch_tag`], sign its digest keyless, attest its SBOM and write
/// `<out>/PUBLISHED`.
pub(crate) fn publish_signed(
    root: &Path,
    dist: &Path,
    signatures: &Path,
    out: &Path,
) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let arch = Arch::host()?;
    let images = signed_images(
        &tools,
        &root.join(RELEASE_PUBLIC_KEY),
        dist,
        signatures,
        out,
        arch,
        &revision,
    )?;
    let registry = Registry::sign_in(&tools)?;
    let mut published = String::new();
    for (name, layout) in &images {
        let tag = signed_arch_tag(&revision, arch);
        published.push_str(&push_image(
            &tools,
            &registry,
            dist,
            out,
            (name, layout),
            &tag,
            arch,
        )?);
    }
    registry.sign_out()?;
    fs::write(out.join("PUBLISHED"), &published).context("writing PUBLISHED")?;
    eprint!("    published the release-signed images of {revision}:\n{published}");
    Ok(())
}

/// The release-signed image of every binary `dist/SHA256SUMS` lists, for `arch`, built from commit
/// `revision`, each with its `<name>.sig` layer and written as a layout to `<out>/images/<name>`,
/// where the push reads it. Every binary must match its line, and every binary and `SHA256SUMS`
/// itself must carry a signature in `signatures` that verifies against `key`, before any image is
/// built or written, so a release the key did not sign is never pushed (DEC-2026).
pub(crate) fn signed_images(
    tools: &Path,
    key: &Path,
    dist: &Path,
    signatures: &Path,
    out: &Path,
    arch: Arch,
    revision: &str,
) -> Result<Vec<(String, image::Layout)>> {
    let sums_file = dist.join("SHA256SUMS");
    let sums = parse_sums(
        &fs::read_to_string(&sums_file)
            .with_context(|| format!("reading {}", sums_file.display()))?,
    )?;
    for (name, hex) in &sums {
        let binary = dist.join("bin").join(name);
        ensure!(
            sha256(&binary)? == *hex,
            "{name} is not the binary SHA256SUMS lists"
        );
        verify_blob(tools, key, &binary, &signatures.join(format!("{name}.sig")))?;
    }
    verify_blob(tools, key, &sums_file, &signatures.join("SHA256SUMS.sig"))?;
    sums.keys()
        .map(|name| {
            let binary =
                fs::read(dist.join("bin").join(name)).with_context(|| format!("reading {name}"))?;
            let signature = fs::read(signatures.join(format!("{name}.sig")))
                .with_context(|| format!("reading {name}.sig"))?;
            let layout = image::layout(name, arch.oci(), &binary, Some(&signature), revision);
            image::write(&layout, &out.join("images").join(name))?;
            Ok((name.clone(), layout))
        })
        .collect()
}

/// The charts a hosted cluster installs from `deploy/cell/charts/` (E21-58, DEC-2015 item 3).
/// `archive-store` is the local cell's test stand-in and is never published (DEC-1050 item 5).
pub(crate) const HOSTED_CHARTS: [&str; 4] =
    ["backup-plugin-tls", "journal-db", "vault", "workspace"];

/// A published chart's version: its `Chart.yaml` version with the commit's first twelve hex
/// digits as build metadata, so a chart's tag names the commit it was packaged from.
fn chart_version(chart_yaml: &str, revision: &str) -> Result<String> {
    let base = chart_yaml
        .lines()
        .find_map(|line| line.strip_prefix("version: "))
        .map(str::trim)
        .filter(|version| !version.is_empty() && !version.contains('+'))
        .context("Chart.yaml names no version, or one that already carries build metadata")?;
    let short = revision
        .get(..12)
        .filter(|short| short.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("the commit is not a hex object name")?;
    Ok(format!("{base}+g{short}"))
}

/// The manifest digest `helm push` reports on its `Digest:` line.
fn pushed_digest(output: &str) -> Result<String> {
    let digest = output
        .lines()
        .find_map(|line| line.strip_prefix("Digest: "))
        .map(str::trim)
        .context("helm push printed no digest")?;
    let hex = digest
        .strip_prefix("sha256:")
        .context("helm push printed a digest that is not SHA-256")?;
    ensure!(
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "helm push printed a malformed digest: {digest}"
    );
    Ok(digest.to_owned())
}

/// `cargo xtask release publish-charts <out>` (E21-58, DEC-2015 item 3): only from a push to
/// `main`, where `release.yml`'s `publish-index` job runs it (ES-12). Packages each of
/// [`HOSTED_CHARTS`] with `helm package`, which reads through the charts' symlinks that Flux's git
/// sources would drop, versioned by [`chart_version`]; pushes each to GHCR under
/// `<repository>/charts`, signs its digest keyless with cosign (the run's GitHub OIDC identity,
/// the same as the images'), and writes `<out>/CHARTS`, one `<reference>@<digest>  <chart>` line
/// each, for the overlays' promotion (DEC-2016).
pub(crate) fn publish_charts(root: &Path, out: &Path) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let revision = git(root, &["rev-parse", "HEAD"])?;
    let packages = out.join("charts");
    fs::create_dir_all(&packages).context("creating the charts directory")?;
    let registry = Registry::sign_in(&tools)?;
    let mut charts = String::new();
    for chart in HOSTED_CHARTS {
        let dir = root.join("deploy/cell/charts").join(chart);
        let manifest = fs::read_to_string(dir.join("Chart.yaml"))
            .with_context(|| format!("reading {chart}'s Chart.yaml"))?;
        let name = manifest
            .lines()
            .find_map(|line| line.strip_prefix("name: "))
            .map(str::trim)
            .with_context(|| format!("{chart}'s Chart.yaml names no chart"))?;
        let version = chart_version(&manifest, &revision)?;
        run(Command::new("helm")
            .arg("package")
            .arg(&dir)
            .args(["--version", &version, "--destination"])
            .arg(&packages))?;
        let package = packages.join(format!("{name}-{version}.tgz"));
        let pushed = Command::new("helm")
            .arg("push")
            .arg(&package)
            .arg(format!("oci://{REGISTRY_HOST}/{REPOSITORY}/charts"))
            .arg("--registry-config")
            .arg(&registry.helm)
            .output()
            .context("starting `helm push`")?;
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&pushed.stdout),
            String::from_utf8_lossy(&pushed.stderr)
        );
        eprint!("{printed}");
        ensure!(pushed.status.success(), "helm push of {name} failed");
        let digest = pushed_digest(&printed)?;
        let reference = format!("{REGISTRY_HOST}/{REPOSITORY}/charts/{name}@{digest}");
        sign_keyless(&tools.join("cosign"), &reference, SIGN_RETRY_WAIT)?;
        charts.push_str(&format!("{reference}  {name}  {version}\n"));
    }
    registry.sign_out()?;
    fs::write(out.join("CHARTS"), &charts).context("writing CHARTS")?;
    eprint!("    published the charts of {revision}:\n{charts}");
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

/// Signs `file` with the ephemeral, passwordless cosign key `key` into `signature`, offline: no
/// transparency log, the signature file cosign's `--output-signature` writes.
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

/// Where the release key's public half is committed (#1689), the key `RELEASE_KEYS` compiles in.
const RELEASE_PUBLIC_KEY: &str = "crates/mandate-release/release-keys/cosign.pub";

/// `cargo xtask release check-signed <dist> <signatures>` (DEC-2024, as the founder decided in
/// DEC-2605 rows 8 and 9): run by CI's `release-signing-check` job, after the `release-signing` job
/// signed `dist` with the release key, and holding no secret. Each binary `SHA256SUMS` lists,
/// checked against its line, and `SHA256SUMS` itself must carry a signature in `<signatures>` that
/// verifies against the committed public half; then [`DRILLED`], unpacked from its image with its
/// signature beside it, must start at tier `production`, so a key that is not the one the binaries
/// compile in fails the run rather than leave a release nothing would start.
pub(crate) fn check_signed(root: &Path, dist: &Path, signatures: &Path) -> Result<()> {
    let paths = Paths::from_env(root)?;
    let tools = tools(&paths.cargo_home)?;
    let public = root.join(RELEASE_PUBLIC_KEY);
    let sums_file = dist.join("SHA256SUMS");
    let sums = parse_sums(
        &fs::read_to_string(&sums_file)
            .with_context(|| format!("reading {}", sums_file.display()))?,
    )?;
    for (name, hex) in &sums {
        let binary = dist.join("bin").join(name);
        ensure!(
            sha256(&binary)? == *hex,
            "{name} is not the binary SHA256SUMS lists"
        );
        verify_blob(
            &tools,
            &public,
            &binary,
            &signatures.join(format!("{name}.sig")),
        )?;
    }
    verify_blob(
        &tools,
        &public,
        &sums_file,
        &signatures.join("SHA256SUMS.sig"),
    )?;
    drill_release_signed(dist, signatures, &dist.join("drill"))
}

/// Checks `signature` over `file` against the cosign public key `key`, offline, as `sign` wrote it.
fn verify_blob(tools: &Path, key: &Path, file: &Path, signature: &Path) -> Result<()> {
    run(Command::new(tools.join("cosign"))
        .args(["verify-blob", "--insecure-ignore-tlog=true", "--key"])
        .arg(key)
        .arg("--signature")
        .arg(signature)
        .arg(file))
}

/// Runs [`DRILLED`] as its release-signed image would run, at tier `production`, and requires it
/// to start.
fn drill_release_signed(dist: &Path, signatures: &Path, drill: &Path) -> Result<()> {
    let binary = dist.join("bin").join(DRILLED);
    let bytes = fs::read(&binary).with_context(|| format!("reading {}", binary.display()))?;
    let signature = fs::read(signatures.join(format!("{DRILLED}.sig")))
        .context("reading the release signature")?;
    let _ = fs::remove_dir_all(drill);
    let layout = image::layout(
        DRILLED,
        Arch::host()?.oci(),
        &bytes,
        Some(&signature),
        "release",
    );
    image::write(&layout, &drill.join("layout"))?;
    image::unpack(&layout, &drill.join("layout"), &drill.join("rootfs"))?;
    let output = Command::new(drill.join("rootfs").join(DRILLED))
        .env_clear()
        .env("MANDATE_DEPLOYMENT_TIER", "production")
        .output()
        .context("running the release-signed image")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    ensure!(
        output.status.code() == Some(2) && stderr.trim_end() == ADMITTED,
        "the release-signed {DRILLED} did not start at tier production ({stderr:?}): the key's \
         public half is not in RELEASE_KEYS (docs/runbooks/release-signing-key.md)"
    );
    eprintln!("    sign: the release-signed {DRILLED} starts at tier production");
    Ok(())
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

/// How many times [`logged`] runs a cosign command before the release fails.
const SIGN_ATTEMPTS: u32 = 3;
/// The pause between two of [`logged`]'s attempts.
const SIGN_RETRY_WAIT: Duration = Duration::from_secs(10);

/// Signs `reference` keyless with `cosign` (the run's OIDC identity), through [`logged`].
fn sign_keyless(cosign: &Path, reference: &str, wait: Duration) -> Result<()> {
    logged(
        cosign,
        &["sign".as_ref(), "--yes".as_ref(), reference.as_ref()],
        wait,
    )
}

/// Attests `reference` with the CycloneDX SBOM `predicate`, keyless, through [`logged`].
fn attest_sbom(cosign: &Path, predicate: &Path, reference: &str, wait: Duration) -> Result<()> {
    logged(
        cosign,
        &[
            "attest".as_ref(),
            "--yes".as_ref(),
            "--type".as_ref(),
            "cyclonedx".as_ref(),
            "--predicate".as_ref(),
            predicate.as_os_str(),
            reference.as_ref(),
        ],
        wait,
    )
}

/// Runs `cosign` with `args`, a keyless sign or attest that writes a transparency-log entry, trying
/// [`SIGN_ATTEMPTS`] times with `wait` between them. The log can take long enough to answer that
/// cosign sends its upload again and the log refuses the copy (`409 createLogEntryConflict`),
/// failing the command though the first upload landed; each attempt makes fresh ephemeral keys, so
/// a later one is a new entry, and a digest signed or attested twice only carries two signatures or
/// attestations. Every attempt failing fails it.
fn logged(cosign: &Path, args: &[&OsStr], wait: Duration) -> Result<()> {
    let mut attempt = 1;
    loop {
        match run(Command::new(cosign).args(args)) {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= SIGN_ATTEMPTS => {
                return Err(error.context(format!("attempt {attempt} of {SIGN_ATTEMPTS}")));
            }
            Err(error) => {
                eprintln!("    cosign, attempt {attempt} of {SIGN_ATTEMPTS}: {error:#}");
                thread::sleep(wait);
                attempt += 1;
            }
        }
    }
}

/// Runs `command` with its stdout sent to stderr, so the build's stdout holds only its
/// `digest=` line for `$GITHUB_OUTPUT`. It prints the program and its arguments only, never the
/// variables set on it, which `Command`'s `Debug` would print with their values.
fn run(command: &mut Command) -> Result<()> {
    let shown = shown(command);
    eprintln!("    $ {shown}");
    let status = command
        .stdout(io::stderr())
        .status()
        .with_context(|| format!("starting {shown}"))?;
    if !status.success() {
        bail!("{shown} failed with {status}");
    }
    Ok(())
}

/// The program and arguments of `command`, each quoted, without its environment.
fn shown(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|word| format!("{word:?}"))
        .collect::<Vec<_>>()
        .join(" ")
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
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::repo_root;

    /// A stand-in cosign that fails its first `fails` runs and then succeeds, counting every run in
    /// `count` beside it.
    fn flaky_cosign(name: &str, fails: u32) -> Result<(PathBuf, PathBuf)> {
        let dir = env::temp_dir().join(format!("mandate-release-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let count = dir.join("count");
        fs::write(&count, "")?;
        let cosign = dir.join("cosign");
        fs::write(
            &cosign,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{count}'\n[ \"$(wc -l < '{count}')\" -gt {fails} ]\n",
                count = count.display()
            ),
        )?;
        fs::set_permissions(&cosign, fs::Permissions::from_mode(0o755))?;
        Ok((cosign, count))
    }

    #[test]
    fn a_sign_the_log_refuses_once_is_tried_again_and_signs() -> Result<()> {
        let (cosign, count) = flaky_cosign("sign-once", 1)?;
        sign_keyless(&cosign, "ghcr.io/x/y@sha256:aa", Duration::ZERO)?;
        assert_eq!(
            fs::read_to_string(&count)?,
            "sign --yes ghcr.io/x/y@sha256:aa\nsign --yes ghcr.io/x/y@sha256:aa\n",
            "the second attempt signs the same digest, and nothing runs after it succeeds"
        );
        Ok(())
    }

    #[test]
    fn a_sign_that_fails_every_attempt_fails_the_release_after_three() -> Result<()> {
        let (cosign, count) = flaky_cosign("sign-never", u32::MAX)?;
        let error = sign_keyless(&cosign, "ghcr.io/x/y@sha256:bb", Duration::ZERO)
            .err()
            .context("a cosign that always fails must fail the sign")?;
        assert_eq!(
            fs::read_to_string(&count)?.lines().count(),
            3,
            "exactly SIGN_ATTEMPTS runs"
        );
        assert!(format!("{error:#}").contains("attempt 3 of 3"), "{error:#}");
        Ok(())
    }

    #[test]
    fn an_attest_the_log_refuses_once_is_tried_again_and_attests() -> Result<()> {
        let (cosign, count) = flaky_cosign("attest-once", 1)?;
        attest_sbom(
            &cosign,
            Path::new("dist/sbom/x.cdx.json"),
            "ghcr.io/x/y@sha256:dd",
            Duration::ZERO,
        )?;
        assert_eq!(
            fs::read_to_string(&count)?,
            "attest --yes --type cyclonedx --predicate dist/sbom/x.cdx.json ghcr.io/x/y@sha256:dd\n".repeat(2),
            "the second attempt attests the same digest with the same SBOM"
        );
        Ok(())
    }

    #[test]
    fn a_sign_that_succeeds_first_runs_once() -> Result<()> {
        let (cosign, count) = flaky_cosign("sign-first", 0)?;
        sign_keyless(&cosign, "ghcr.io/x/y@sha256:cc", Duration::ZERO)?;
        assert_eq!(fs::read_to_string(&count)?.lines().count(), 1);
        Ok(())
    }

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

    /// What a release ships, exactly: the CLI, the agent daemon, the simulator the local cell's
    /// drills run, the workspace API server, whose image a hosted cell runs as `images.api`, and
    /// the three agent processes a hosted cell runs, the deployment manager, the agent runtime and
    /// the account executor, the role provisioner (DEC-2350), and the scheduler, the dispatcher,
    /// the data process and the model gateway (DEC-2079), each package building the binary of its
    /// name but the CLI's `mandate` (DEC-971, DEC-2026).
    #[test]
    fn the_release_ships_exactly_these_binaries() {
        assert_eq!(
            BINARIES.as_slice(),
            [
                ("mandate-cli", "mandate"),
                ("mandate-agentd", "mandate-agentd"),
                ("mandate-rh-sim", "mandate-rh-sim"),
                ("mandate-api-server", "mandate-api-server"),
                ("mandate-deployd", "mandate-deployd"),
                ("mandate-runtime-models", "mandate-daemon"),
                ("mandate-executord", "mandate-executord"),
                ("mandate-provisionerd", "mandate-provisionerd"),
                ("mandate-token-exchange", "mandate-token-exchange"),
                ("mandate-scheduler", "mandate-scheduler"),
                ("mandate-dispatcher", "mandate-dispatcher"),
                ("mandate-datad", "mandate-datad"),
                ("mandate-gatewayd", "mandate-gatewayd"),
                ("mandate-news-reader", "mandate-news-reader"),
            ]
            .as_slice()
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

    #[test]
    fn a_chart_version_carries_the_commit_as_build_metadata() -> Result<()> {
        let manifest = "apiVersion: v2\nname: mandate-workspace\nversion: 0.1.0\n";
        let revision = "545f4852774c1de0b2a4a1e3f0e9f6c2b8d7a0e1";
        assert_eq!(chart_version(manifest, revision)?, "0.1.0+g545f4852774c");
        assert!(chart_version("name: x\n", revision).is_err());
        assert!(chart_version("version: 0.1.0+old\n", revision).is_err());
        assert!(chart_version(manifest, "not-a-commit").is_err());
        assert!(chart_version(manifest, "545f").is_err());
        Ok(())
    }

    #[test]
    fn only_a_sha256_digest_from_helm_push_is_taken() -> Result<()> {
        let digest = format!("sha256:{}", hex('a'));
        let printed = format!(
            "Pushed: ghcr.io/kunwarshivam/mandate/charts/mandate-workspace:0.1.0_g545f4852774c\n\
             Digest: {digest}\n"
        );
        assert_eq!(pushed_digest(&printed)?, digest);
        assert!(pushed_digest("Pushed: x\n").is_err());
        assert!(pushed_digest(&format!("Digest: sha512:{}\n", hex('a'))).is_err());
        assert!(pushed_digest("Digest: sha256:abc\n").is_err());
        assert!(pushed_digest(&format!("Digest: sha256:{}\n", hex('A'))).is_err());
        Ok(())
    }

    /// The chart that is never published, beside [`HOSTED_CHARTS`].
    const LOCAL_ONLY_CHARTS: [&str; 1] = ["archive-store"];

    /// Every chart under `deploy/cell/charts/` is either published for the hosted clusters or
    /// named as the local cell's alone, so a new chart cannot be left out of the hosted clusters
    /// unnoticed.
    #[test]
    fn every_cell_chart_is_published_or_named_local_only() -> Result<()> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../deploy/cell/charts");
        let mut found = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().join("Chart.yaml").is_file() {
                found.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        found.sort();
        let mut named: Vec<String> = HOSTED_CHARTS
            .iter()
            .chain(LOCAL_ONLY_CHARTS.iter())
            .map(|chart| (*chart).to_owned())
            .collect();
        named.sort();
        assert_eq!(found, named);
        Ok(())
    }

    /// The jobs that publish, one a runner of each arch and the index over them.
    const PUBLISH_JOBS: [&str; 3] = ["publish-amd64", "publish-arm64", "publish-index"];

    /// The jobs that publish a hosted cell's release-signed images, on a dispatch (DEC-2026).
    const SIGNED_PUBLISH_JOBS: [&str; 2] =
        ["release-signing-publish", "release-signing-publish-index"];

    /// The founder-signed path's jobs, on a dispatch from `main` that is not post-merge (DEC-2024,
    /// DEC-2026, DEC-2651).
    const SIGNING_JOBS: [&str; 5] = [
        "release-signing-build",
        "release-signing",
        "release-signing-check",
        "release-signing-publish",
        "release-signing-publish-index",
    ];

    /// The condition of every job on the push's publish path: a push to `main`, or `merge.yml`'s
    /// post-merge dispatch on `main` in its place (DEC-2651).
    const PUSH_PATH_IF: &str = "${{ (github.event_name == 'push' || (github.event_name == \
         'workflow_dispatch' && inputs.post_merge == true)) && github.ref == 'refs/heads/main' }}";

    /// The condition of every job on the founder-signed path: a dispatch on `main` that is not
    /// post-merge. It holds no negation, which leak-guard L12 refuses on the job with an
    /// `environment:` (DEC-2651).
    const SIGNING_PATH_IF: &str = "${{ github.event_name == 'workflow_dispatch' && \
         inputs.post_merge == false && github.ref == 'refs/heads/main' }}";

    /// A value of the GitHub Actions expression language, as far as release.yml's job conditions
    /// and concurrency group use it.
    #[derive(Clone, Debug, PartialEq)]
    enum Value {
        Null,
        Bool(bool),
        Str(String),
    }

    impl Value {
        fn truthy(&self) -> bool {
            match self {
                Value::Null => false,
                Value::Bool(value) => *value,
                Value::Str(text) => !text.is_empty(),
            }
        }

        /// GitHub's coercion for comparing values of different types; `None` is `NaN`, which
        /// equals nothing.
        fn number(&self) -> Option<i64> {
            match self {
                Value::Null | Value::Bool(false) => Some(0),
                Value::Bool(true) => Some(1),
                Value::Str(text) if text.trim().is_empty() => Some(0),
                Value::Str(text) => text.trim().parse().ok(),
            }
        }

        fn text(&self) -> String {
            match self {
                Value::Null => String::new(),
                Value::Bool(value) => value.to_string(),
                Value::Str(text) => text.clone(),
            }
        }

        fn equals(&self, other: &Value) -> bool {
            match (self, other) {
                (Value::Str(a), Value::Str(b)) => a.eq_ignore_ascii_case(b),
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Null, Value::Null) => true,
                _ => matches!((self.number(), other.number()), (Some(a), Some(b)) if a == b),
            }
        }
    }

    /// The event a run of release.yml answers: its name, its ref, and the `post_merge` input,
    /// which only a dispatch has.
    #[derive(Clone, Copy, Debug)]
    struct Run {
        event: &'static str,
        git_ref: &'static str,
        post_merge: Option<bool>,
    }

    /// An evaluator of GitHub's expression language for the operators, literals, contexts and the
    /// one function release.yml's job conditions and concurrency group use, with GitHub's
    /// precedence (`!`, then `==` and `!=`, then `&&`, then `||`) and its value-returning `&&` and
    /// `||`. Anything else, a context or a function it does not model included, is an error, so a
    /// condition it cannot judge fails the test instead of passing it: the independent oracle for
    /// `the_post_merge_dispatch_publishes_and_never_signs`.
    struct Expression<'a> {
        tokens: Vec<String>,
        at: usize,
        run: &'a Run,
    }

    impl Expression<'_> {
        fn evaluate(source: &str, run: &Run) -> Result<Value> {
            let inner = source
                .trim()
                .strip_prefix("${{")
                .and_then(|rest| rest.strip_suffix("}}"))
                .unwrap_or(source);
            let mut expression = Expression {
                tokens: Self::tokens(inner)?,
                at: 0,
                run,
            };
            let value = expression.or()?;
            ensure!(
                expression.at == expression.tokens.len(),
                "trailing tokens in {source:?}"
            );
            Ok(value)
        }

        fn tokens(text: &str) -> Result<Vec<String>> {
            let chars: Vec<char> = text.chars().collect();
            let mut out = Vec::new();
            let mut index = 0;
            while let Some(&c) = chars.get(index) {
                let pair: String = chars.iter().skip(index).take(2).collect();
                if c.is_whitespace() {
                    index += 1;
                } else if ["&&", "||", "==", "!="].contains(&pair.as_str()) {
                    out.push(pair);
                    index += 2;
                } else if "()!,".contains(c) {
                    out.push(c.to_string());
                    index += 1;
                } else if c == '\'' {
                    let end = chars
                        .iter()
                        .skip(index + 1)
                        .position(|&q| q == '\'')
                        .with_context(|| format!("an unterminated string in {text:?}"))?;
                    out.push(chars[index..index + end + 2].iter().collect());
                    index += end + 2;
                } else if c.is_ascii_alphanumeric() || c == '_' {
                    let len = chars
                        .iter()
                        .skip(index)
                        .take_while(|&&w| w.is_ascii_alphanumeric() || "_.-".contains(w))
                        .count();
                    out.push(chars[index..index + len].iter().collect());
                    index += len;
                } else {
                    bail!("an unmodelled character {c:?} in {text:?}");
                }
            }
            Ok(out)
        }

        fn peek(&self) -> Option<&str> {
            self.tokens.get(self.at).map(String::as_str)
        }

        fn next(&mut self) -> Result<String> {
            let token = self
                .tokens
                .get(self.at)
                .context("the expression ends early")?;
            self.at += 1;
            Ok(token.clone())
        }

        fn expect(&mut self, wanted: &str) -> Result<()> {
            let token = self.next()?;
            ensure!(token == wanted, "expected {wanted:?}, found {token:?}");
            Ok(())
        }

        fn or(&mut self) -> Result<Value> {
            let mut left = self.and()?;
            while self.peek() == Some("||") {
                self.at += 1;
                let right = self.and()?;
                left = if left.truthy() { left } else { right };
            }
            Ok(left)
        }

        fn and(&mut self) -> Result<Value> {
            let mut left = self.comparison()?;
            while self.peek() == Some("&&") {
                self.at += 1;
                let right = self.comparison()?;
                left = if left.truthy() { right } else { left };
            }
            Ok(left)
        }

        fn comparison(&mut self) -> Result<Value> {
            let left = self.unary()?;
            match self.peek() {
                Some("==") => {
                    self.at += 1;
                    Ok(Value::Bool(left.equals(&self.unary()?)))
                }
                Some("!=") => {
                    self.at += 1;
                    Ok(Value::Bool(!left.equals(&self.unary()?)))
                }
                _ => Ok(left),
            }
        }

        fn unary(&mut self) -> Result<Value> {
            if self.peek() == Some("!") {
                self.at += 1;
                return Ok(Value::Bool(!self.unary()?.truthy()));
            }
            self.primary()
        }

        fn primary(&mut self) -> Result<Value> {
            let token = self.next()?;
            if token == "(" {
                let value = self.or()?;
                self.expect(")")?;
                return Ok(value);
            }
            if let Some(text) = token
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
            {
                return Ok(Value::Str(text.to_owned()));
            }
            if token == "format" && self.peek() == Some("(") {
                self.at += 1;
                let mut text = self.or()?.text();
                let mut index = 0;
                while self.peek() == Some(",") {
                    self.at += 1;
                    text = text.replace(&format!("{{{index}}}"), &self.or()?.text());
                    index += 1;
                }
                self.expect(")")?;
                return Ok(Value::Str(text));
            }
            match token.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                "null" => Ok(Value::Null),
                "github.event_name" => Ok(Value::Str(self.run.event.to_owned())),
                "github.ref" => Ok(Value::Str(self.run.git_ref.to_owned())),
                "inputs.post_merge" => Ok(self.run.post_merge.map_or(Value::Null, Value::Bool)),
                other => bail!("the oracle does not model {other:?}"),
            }
        }
    }

    /// DEC-2651: `merge.yml`'s post-merge dispatch on `main` runs the push's publish path, in the
    /// pushes' concurrency group, and never the founder-signed path; a dispatch without the input
    /// still runs only the signing path, in a group of its own; a push runs only the publish path;
    /// and nothing runs either path off `main`. Each job's condition and the concurrency group are
    /// read from release.yml and judged by [`Expression`], and the conditions the change replaced
    /// are judged too, to show the oracle catches the bug it fixed.
    #[test]
    fn the_post_merge_dispatch_publishes_and_never_signs() -> Result<()> {
        let push_main = Run {
            event: "push",
            git_ref: "refs/heads/main",
            post_merge: None,
        };
        let post_merge = Run {
            event: "workflow_dispatch",
            git_ref: "refs/heads/main",
            post_merge: Some(true),
        };
        let dispatch = Run {
            event: "workflow_dispatch",
            git_ref: "refs/heads/main",
            post_merge: Some(false),
        };
        let push_branch = Run {
            git_ref: "refs/heads/dev",
            ..push_main
        };
        let post_merge_branch = Run {
            git_ref: "refs/heads/dev",
            ..post_merge
        };
        let dispatch_branch = Run {
            git_ref: "refs/heads/dev",
            ..dispatch
        };
        let runs = [
            (push_main, true, false, "release-refs/heads/main"),
            (post_merge, true, false, "release-refs/heads/main"),
            (dispatch, false, true, "release-dispatch-refs/heads/main"),
            (push_branch, false, false, "release-refs/heads/dev"),
            (post_merge_branch, false, false, "release-refs/heads/dev"),
            (
                dispatch_branch,
                false,
                false,
                "release-dispatch-refs/heads/dev",
            ),
        ];
        let jobs = release_jobs()?;
        let condition = |name: &str| -> Result<String> {
            let job = jobs
                .get(name)
                .with_context(|| format!("release.yml has no {name}"))?;
            job.lines()
                .find_map(|line| line.strip_prefix("    if: "))
                .map(str::to_owned)
                .with_context(|| format!("{name} has no condition"))
        };
        for (name, body) in &jobs {
            let named = body.contains("inputs.") || body.contains("github.event_name ==");
            let pathed =
                PUBLISH_JOBS.contains(&name.as_str()) || SIGNING_JOBS.contains(&name.as_str());
            assert_eq!(
                named, pathed,
                "{name}: only the publish and signing jobs choose by event or input"
            );
        }
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml"),
        )?;
        let group = text
            .split_once("\nconcurrency:\n  group: release-")
            .and_then(|(_, rest)| rest.lines().next())
            .context("release.yml has no concurrency group")?;
        let cancel = text
            .split_once("\n  cancel-in-progress: ")
            .and_then(|(_, rest)| rest.lines().next())
            .context("release.yml has no cancel-in-progress")?;
        for (run, publishes, signs, wanted_group) in runs {
            for name in PUBLISH_JOBS {
                assert_eq!(
                    Expression::evaluate(&condition(name)?, &run)?.truthy(),
                    publishes,
                    "{name} on {run:?}"
                );
            }
            for name in SIGNING_JOBS {
                assert_eq!(
                    Expression::evaluate(&condition(name)?, &run)?.truthy(),
                    signs,
                    "{name} on {run:?}"
                );
            }
            assert_eq!(
                format!("release-{}", Expression::evaluate(group, &run)?.text()),
                wanted_group,
                "the concurrency group on {run:?}"
            );
            assert!(
                !Expression::evaluate(cancel, &run)?.truthy(),
                "a release on {run:?} cancels nothing"
            );
        }
        let replaced_signing = "${{ github.event_name == 'workflow_dispatch' && github.ref == \
             'refs/heads/main' }}";
        let replaced_publish =
            "${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}";
        let replaced_group = "${{ github.event_name == 'pull_request' && github.ref || \
             github.event_name == 'workflow_dispatch' && format('dispatch-{0}', github.ref) || \
             github.ref }}";
        assert!(Expression::evaluate(replaced_signing, &post_merge)?.truthy());
        assert!(!Expression::evaluate(replaced_publish, &post_merge)?.truthy());
        assert_eq!(
            Expression::evaluate(replaced_group, &post_merge)?.text(),
            "dispatch-refs/heads/main"
        );
        let truthy_input = "${{ github.event_name == 'workflow_dispatch' && inputs.post_merge }}";
        assert!(Expression::evaluate(truthy_input, &post_merge)?.truthy());
        assert!(!Expression::evaluate(truthy_input, &dispatch)?.truthy());
        assert!(Expression::evaluate("${{ hashFiles('x') }}", &post_merge).is_err());
        Ok(())
    }

    /// Only the publish jobs may write packages or mint an OIDC token for keyless signing, and
    /// they run only on a push to `main`: never from a pull request or a branch (ES-12, E21-35).
    /// The release-signed publish jobs are the one other pair, pinned to a dispatch from `main` by
    /// `the_signed_images_are_published_only_from_a_checked_dispatch`.
    /// Each arch publishes from a runner of its own arch, after its builds compared, and the index
    /// only after every arch has published (DEC-1060).
    #[test]
    fn only_the_main_publish_jobs_can_write_packages_or_sign() -> Result<()> {
        let jobs = release_jobs()?;
        for (name, body) in &jobs {
            let writes = body.contains("packages: write") || body.contains("id-token: write");
            let publishes = PUBLISH_JOBS.contains(&name.as_str())
                || SIGNED_PUBLISH_JOBS.contains(&name.as_str());
            assert_eq!(writes, publishes, "{name}");
        }
        for name in PUBLISH_JOBS {
            let job = jobs
                .get(name)
                .with_context(|| format!("release.yml has no {name}"))?;
            assert!(
                job.contains(&format!("    if: {PUSH_PATH_IF}\n")),
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

    /// The three jobs that sign a release run only on a manual dispatch from `main`, with nothing
    /// but `contents: read`. Only `release-signing` reads a secret or names an environment, the
    /// `release-signing` one the founder approves, and it checks out nothing and runs no cargo and
    /// no xtask: no build script, dependency or repository code runs beside the key. It signs only
    /// the build that matched build-a, with cosign pinned to the same release and SHA-256 as
    /// [`Arch::tools`], in a clean environment (DEC-2024; DEC-2605 rows 8 and 9).
    #[test]
    fn only_the_dispatched_signing_job_holds_the_release_key() -> Result<()> {
        let jobs = release_jobs()?;
        for (name, body) in &jobs {
            let holds = body.contains("secrets.") || body.contains("environment:");
            assert_eq!(holds, name == "release-signing", "{name}");
        }
        let workflows = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
        for entry in fs::read_dir(&workflows)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if name == "release.yml" {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            assert!(
                !text.contains("RELEASE_SIGNING") && !text.contains("release-signing"),
                "{name} names the release key or its environment"
            );
            if name == "release-check.yml" {
                assert!(
                    !text.contains("secrets.") && !text.contains("environment:"),
                    "release-check.yml, which a pull request runs, holds no secret"
                );
            }
        }
        for name in [
            "release-signing-build",
            "release-signing",
            "release-signing-check",
        ] {
            let job = jobs
                .get(name)
                .with_context(|| format!("release.yml has no {name}"))?;
            for line in [
                format!("    if: {SIGNING_PATH_IF}\n"),
                "    permissions:\n      contents: read\n    defaults:\n".to_owned(),
            ] {
                assert!(job.contains(&line), "{name} lacks {line:?}");
            }
        }
        let build = jobs
            .get("release-signing-build")
            .context("no release-signing-build")?;
        assert!(build.contains("    needs: [build-a-amd64, build-a-arm64, compare]\n"));
        assert!(build.contains("!= needs[format('build-a-{0}', matrix.arch)].outputs.digest"));
        let job = jobs.get("release-signing").context("no release-signing")?;
        for absent in ["cargo", "xtask", "rustup"] {
            assert!(!job.contains(absent), "release-signing runs {absent}");
        }
        for line in [
            "    needs: [build-a-amd64, build-a-arm64, release-signing-build]\n",
            "    environment: release-signing\n",
            "          sparse-checkout: .github/scripts\n",
            "          DIGEST: ${{ needs[format('build-a-{0}', matrix.arch)].outputs.digest }}\n",
            "          RELEASE_SIGNING_KEY: ${{ secrets.RELEASE_SIGNING_KEY }}\n",
            "          COSIGN_PASSWORD: ${{ secrets.RELEASE_SIGNING_PASSWORD }}\n",
            "        run: .github/scripts/release-sign.sh\n",
        ] {
            assert!(job.contains(line), "release-signing lacks {line:?}");
        }
        assert_eq!(job.matches("secrets.").count(), 2, "{job}");
        for arch in Arch::ALL {
            let cosign = arch
                .tools()
                .into_iter()
                .find(|tool| tool.name == "cosign")
                .context("no pinned cosign")?;
            assert!(
                job.contains(&format!("cosign: {}\n", cosign.url)),
                "{arch:?}"
            );
            assert!(
                job.contains(&format!("cosign_sha256: {}\n", cosign.sha256)),
                "{arch:?}"
            );
        }
        let script = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/scripts/release-sign.sh"),
        )?;
        assert!(
            script.starts_with("#!/bin/bash --posix\n")
                && script.contains(
                    "\nset +x +v\nunset -f compgen\ncase \"$(compgen -A function)\" in\n"
                ),
            "release-sign.sh must start in POSIX mode, turn tracing off and refuse functions first"
        );
        let ordered = [
            "grep -Evq '^[0-9a-f]{64}  [A-Za-z0-9._-]+$' \"$sums\"",
            "echo \"SHA256SUMS lists $name twice; nothing is signed\" >&2",
            "echo \"dist/bin holds $name, which SHA256SUMS does not list; nothing is signed\" >&2",
            "if [ \"${#present[@]}\" -ne \"${#names[@]}\" ]; then",
            "trap 'rm -rf \"$key_dir\" \"$verified\"' EXIT",
            "cp -- \"dist/bin/$name\" \"$verified/bin/$name\"",
            "(cd \"$verified/bin\" && sha256sum --check --strict ../SHA256SUMS)",
            "(cd \"$verified\" && sha256sum --check --strict DIGEST)",
            "sha256sum --check --strict \"$tools/cosign.sha256\"",
            "base64 --decode \"$key_dir/cosign.key.base64\" > \"$key_dir/cosign.key\"",
            "unset RELEASE_SIGNING_KEY",
            "env -i \"$(dirname \"$0\")/release-sign-cosign.sh\" \"$key_dir/home\" \"$tools\"",
            "<<<\"$COSIGN_PASSWORD\"",
        ];
        let mut from = 0;
        for line in ordered {
            let at = script
                .get(from..)
                .and_then(|rest| rest.find(line))
                .with_context(|| {
                    format!("release-sign.sh lacks {line:?}, or holds it out of order")
                })?;
            from = from.saturating_add(at).saturating_add(line.len());
        }
        let cosign = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/scripts/release-sign-cosign.sh"),
        )?;
        let mut from = 0;
        for line in [
            "\nset +x +v\nset -euo pipefail\n",
            "export HOME=\"$1\" PATH=\"$2:/usr/bin:/bin\"",
            "IFS= read -r COSIGN_PASSWORD",
            "export COSIGN_PASSWORD",
            "exec env -u SHLVL -u PWD -u OLDPWD -u _ cosign sign-blob",
        ] {
            let at = cosign
                .get(from..)
                .and_then(|rest| rest.find(line))
                .with_context(|| {
                    format!("release-sign-cosign.sh lacks {line:?}, or out of order")
                })?;
            from = from.saturating_add(at).saturating_add(line.len());
        }
        for absent in ["set -x", "cargo", "xtask", "COSIGN_PASSWORD="] {
            assert!(
                !cosign.contains(absent),
                "release-sign-cosign.sh holds {absent:?}"
            );
        }
        for absent in [
            "set -x",
            "cargo",
            "xtask",
            "dist/bin/*",
            "compgen -e",
            "COSIGN_PASSWORD=",
        ] {
            assert!(!script.contains(absent), "release-sign.sh holds {absent:?}");
        }
        assert!(
            script.contains(
                "for file in \"${names[@]/#/$verified/bin/}\" \"$verified/SHA256SUMS\"; do"
            ),
            "release-sign.sh must sign exactly the verified copies of the names SHA256SUMS lists"
        );
        let check = jobs
            .get("release-signing-check")
            .context("no release-signing-check")?;
        assert!(check.contains("    needs: [release-signing]\n"));
        assert!(check.contains("cargo xtask release check-signed dist signed/signatures"));
        Ok(())
    }

    /// A hosted cell's images are published only on a dispatch from `main`, after
    /// `release-signing-check` proved the run's signatures, by jobs that hold the run's own token
    /// and no release secret, and that verify against the committed key again before any push
    /// (DEC-2026 items 1 and 2). The push-only jobs never publish them.
    #[test]
    fn the_signed_images_are_published_only_from_a_checked_dispatch() -> Result<()> {
        let jobs = release_jobs()?;
        let dispatch = format!("    if: {SIGNING_PATH_IF}\n");
        let token = "    permissions:\n      contents: read\n      packages: write\n      id-token: write\n    defaults:\n";
        for (name, needs, run) in [
            (
                "release-signing-publish",
                "    needs: [release-signing-check]\n",
                "        run: cargo xtask release publish-signed dist signed/signatures signed-published\n",
            ),
            (
                "release-signing-publish-index",
                "    needs: [release-signing-publish]\n",
                "        run: cargo xtask release publish-signed-index signed-published\n",
            ),
        ] {
            let job = jobs
                .get(name)
                .with_context(|| format!("release.yml has no {name}"))?;
            for line in [
                dispatch.as_str(),
                token,
                needs,
                run,
                "          GH_TOKEN: ${{ github.token }}\n",
            ] {
                assert!(job.contains(line), "{name} lacks {line:?}");
            }
        }
        let publish = jobs
            .get("release-signing-publish")
            .context("no release-signing-publish")?;
        for line in [
            "          name: release-to-sign-${{ github.sha }}-${{ matrix.arch }}\n          path: dist\n",
            "          name: release-signatures-${{ github.sha }}-${{ matrix.arch }}\n          path: signed\n",
        ] {
            assert!(
                publish.contains(line),
                "release-signing-publish lacks {line:?}"
            );
        }
        let build = jobs
            .get("release-signing-build")
            .context("no release-signing-build")?;
        assert!(
            build.contains("            dist/sbom/\n"),
            "the SBOMs are not handed on"
        );
        for (name, job) in &jobs {
            let signed = job.contains("cargo xtask release publish-signed");
            assert_eq!(
                signed,
                SIGNED_PUBLISH_JOBS.contains(&name.as_str()),
                "{name}"
            );
            if signed {
                assert!(job.contains(dispatch.as_str()), "{name}");
            }
        }
        Ok(())
    }

    /// A release-signed image and index are tagged apart from a push's `main-*` ones, so nothing
    /// a push publishes can stand in for them (DEC-2026 items 3 and 4).
    #[test]
    fn the_signed_tags_are_apart_from_mains() {
        assert_eq!(signed_arch_tag("abc", Arch::Amd64), "signed-abc-amd64");
        assert_eq!(signed_arch_tag("abc", Arch::Arm64), "signed-abc-arm64");
        assert_eq!(signed_index_tag("abc"), "signed-abc");
        for arch in Arch::ALL {
            assert_ne!(signed_arch_tag("abc", arch), arch_tag("abc", arch));
        }
        assert_ne!(signed_index_tag("abc"), index_tag("abc"));
    }

    /// cosign runs with exactly `COSIGN_PASSWORD`, `HOME` and `PATH` in its environment, whatever
    /// else the job held (a name that is no identifier, a variable bash keeps read-only, a trace
    /// switch), with the password nowhere in its command line and neither secret in the signer's
    /// stderr (DEC-2024 item 4, `AGENTS.md` rule 7). A stand-in cosign, served to the signer by a
    /// `file://` URL and pinned by its SHA-256 as the real one is, records the names its process
    /// received and its arguments.
    #[test]
    fn cosign_sees_only_the_password_home_and_path() -> Result<()> {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/scripts/release-sign.sh");
        let dir = env::temp_dir().join(format!("mandate-xtask-signer-env-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("dist/bin"))?;
        fs::write(dir.join("dist/bin/a"), "a")?;
        fs::write(dir.join("dist/bin/b"), "b")?;
        write_sums(&dir, SIGNER_SUMS)?;
        let stand_in = dir.join("stand-in-cosign");
        fs::write(
            &stand_in,
            concat!(
                "#!/bin/sh\n",
                "tr '\\000' '\\n' < /proc/$$/environ | cut -d= -f1 | LC_ALL=C sort > names\n",
                "tr '\\000' ' ' < /proc/$$/cmdline > arguments\n",
            ),
        )?;
        let password = "the-password-is-not-an-argument";
        let key = image::base64(b"stand-in key");
        let output = Command::new(&script)
            .current_dir(&dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("DIGEST", sha256(&dir.join("dist/SHA256SUMS"))?)
            .env("COSIGN_URL", format!("file://{}", stand_in.display()))
            .env("COSIGN_SHA256", sha256(&stand_in)?)
            .env("RELEASE_SIGNING_KEY", &key)
            .env("COSIGN_PASSWORD", password)
            .env("RUNNER_TEMP", dir.join("runner"))
            .env("LD_PRELOAD", "planted.so")
            .env("SIGSTORE_REKOR_URL", "https://planted.invalid")
            .env("HTTPS_PROXY", "http://planted.invalid")
            .env("a b", "planted")
            .env("EUID", "0")
            .env("SHELLOPTS", "xtrace")
            .output()?;
        assert!(
            output.status.success(),
            "the signer stopped: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(dir.join("names"))?,
            "COSIGN_PASSWORD\nHOME\nPATH\n"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for secret in [password, key.as_str(), "stand-in key"] {
            assert!(
                !stderr.contains(secret),
                "stderr shows {secret:?}: {stderr}"
            );
        }
        let arguments = fs::read_to_string(dir.join("arguments"))?;
        assert!(arguments.contains("sign-blob"), "{arguments}");
        assert!(!arguments.contains(password), "{arguments}");
        assert!(
            !dir.join("runner/release-key").exists(),
            "the key was left behind"
        );
        Ok(())
    }

    /// The signer runs in POSIX mode, where bash refuses to start when an exported function shadows
    /// a special builtin such as `set`, and it refuses any other exported function before it reads
    /// anything, so a planted `BASH_FUNC_echo%%` or `BASH_FUNC_set%%` can neither silence its
    /// `set +x +v` nor stand in for a check, and a planted `compgen` that lists nothing cannot hide
    /// another, even beside a planted `unset` that would keep it (DEC-2024 item 4). Every run is
    /// handed what a signing needs, a stand-in cosign, a key and a password, and the control, with
    /// nothing planted, signs all three files; each planted run stops with its own refusal on
    /// stderr, never runs cosign and never reaches `RUNNER_TEMP` (DEC-2049 item 1).
    #[test]
    fn the_signer_refuses_exported_functions() -> Result<()> {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/scripts/release-sign.sh");
        let root = env::temp_dir().join(format!(
            "mandate-xtask-signer-functions-{}",
            std::process::id()
        ));
        let refused = "the environment defines shell functions; nothing is signed";
        let echo = ("BASH_FUNC_echo%%", "() { builtin echo planted; }");
        let compgen = ("BASH_FUNC_compgen%%", "() { :; }");
        let cases: [(&[(&str, &str)], &str); 5] = [
            (&[], ""),
            (&[echo], refused),
            (
                &[("BASH_FUNC_set%%", "() { builtin echo planted; }")],
                "`set': is a special builtin",
            ),
            (&[compgen, echo], refused),
            (
                &[("BASH_FUNC_unset%%", "() { :; }"), compgen, echo],
                "`unset': is a special builtin",
            ),
        ];
        for (planted, refusal) in cases {
            let name = planted
                .iter()
                .map(|(n, _)| n.replace('%', ""))
                .collect::<Vec<_>>()
                .join("+");
            let name = if name.is_empty() {
                "control".to_owned()
            } else {
                name
            };
            let dir = root.join(&name);
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("dist/bin"))?;
            fs::write(dir.join("dist/bin/a"), "a")?;
            fs::write(dir.join("dist/bin/b"), "b")?;
            write_sums(&dir, SIGNER_SUMS)?;
            let stand_in = dir.join("stand-in-cosign");
            fs::write(&stand_in, "#!/bin/sh\necho signed >> cosign-runs\n")?;
            let output = Command::new(&script)
                .current_dir(&dir)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("DIGEST", sha256(&dir.join("dist/SHA256SUMS"))?)
                .env("COSIGN_URL", format!("file://{}", stand_in.display()))
                .env("COSIGN_SHA256", sha256(&stand_in)?)
                .env("RELEASE_SIGNING_KEY", image::base64(b"stand-in key"))
                .env("COSIGN_PASSWORD", "stand-in password")
                .env("RUNNER_TEMP", dir.join("runner"))
                .envs(planted.iter().copied())
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let runs = fs::read_to_string(dir.join("cosign-runs")).unwrap_or_default();
            if planted.is_empty() {
                assert!(
                    output.status.success(),
                    "control: the signer stopped: {stderr}"
                );
                assert_eq!(runs, "signed\n".repeat(3), "control: cosign runs");
                continue;
            }
            assert!(!output.status.success(), "{name}: the signer went on");
            assert!(stderr.contains(refusal), "{name}: stderr is {stderr:?}");
            assert!(
                !stdout.contains("planted") && !stderr.contains("planted"),
                "{name}: a planted function ran"
            );
            assert!(runs.is_empty(), "{name}: cosign ran");
            assert!(
                !stdout.contains("OK"),
                "{name}: the signer checked the copies"
            );
            assert!(
                !dir.join("runner").exists(),
                "{name}: the signer reached RUNNER_TEMP"
            );
        }
        Ok(())
    }

    /// The `SHA256SUMS` of the exact `dist` every signer case starts from: `a` and `b`, holding
    /// the text `a` and `b`.
    const SIGNER_SUMS: &str = concat!(
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb  a\n",
        "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d  b\n",
    );

    /// One case of [`the_signer_signs_only_what_sha256sums_lists`]: its name, what it changes in
    /// the exact `dist` under the directory it is handed, and what the signer prints as it stops.
    type SignerCase = (&'static str, fn(&Path) -> io::Result<()>, &'static str);

    fn write_sums(dir: &Path, text: &str) -> io::Result<()> {
        fs::write(dir.join("dist/SHA256SUMS"), text)
    }

    /// `release-sign.sh` signs only a `dist` that is a real directory holding a regular
    /// `SHA256SUMS` and a real `dist/bin` of regular files, exactly the names it lists, once
    /// each. Every other shape stops it, with its own message, before the key is decoded or
    /// cosign is fetched, so the key job is no signing oracle; the control, an exact `dist`,
    /// passes every check and stops only at the wrong digest it is handed. Each stop leaves
    /// nothing under `RUNNER_TEMP`: the verified copies it signs from are removed on exit.
    #[test]
    fn the_signer_signs_only_what_sha256sums_lists() -> Result<()> {
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/scripts/release-sign.sh");
        let root =
            env::temp_dir().join(format!("mandate-xtask-release-sign-{}", std::process::id()));
        let not_regular = "not a regular file";
        let malformed = "is not '<sha256>  <name>'";
        let cases: Vec<SignerCase> = vec![
            ("exact", |_| Ok(()), "b: OK\nSHA256SUMS: FAILED"),
            (
                "exact with a spaced RUNNER_TEMP",
                |_| Ok(()),
                "b: OK\nSHA256SUMS: FAILED",
            ),
            (
                "tampered",
                |d| fs::write(d.join("dist/bin/a"), "x"),
                "a: FAILED",
            ),
            (
                "extra",
                |d| fs::write(d.join("dist/bin/c"), "c"),
                "dist/bin holds c, which",
            ),
            (
                "dotfile",
                |d| fs::write(d.join("dist/bin/.h"), "h"),
                "dist/bin holds .h, which",
            ),
            (
                "missing",
                |d| fs::remove_file(d.join("dist/bin/b")),
                "SHA256SUMS lists a binary dist/bin does not hold",
            ),
            (
                "duplicate",
                |d| write_sums(d, &format!("{SIGNER_SUMS}{}", &SIGNER_SUMS[..67])),
                "SHA256SUMS lists a twice",
            ),
            (
                "nested",
                |d| {
                    fs::create_dir(d.join("dist/bin/sub"))?;
                    fs::write(d.join("dist/bin/sub/c"), "c")
                },
                not_regular,
            ),
            (
                "empty-dir",
                |d| fs::create_dir(d.join("dist/bin/sub")),
                not_regular,
            ),
            (
                "fifo",
                |d| {
                    let made = Command::new("mkfifo").arg(d.join("dist/bin/f")).status()?;
                    made.success()
                        .then_some(())
                        .ok_or_else(|| io::Error::other("mkfifo"))
                },
                not_regular,
            ),
            (
                "symlink-to-listed",
                |d| {
                    fs::remove_file(d.join("dist/bin/b"))?;
                    symlink("a", d.join("dist/bin/b"))
                },
                not_regular,
            ),
            (
                "symlink-outside",
                |d| {
                    fs::remove_file(d.join("dist/bin/b"))?;
                    symlink("../SHA256SUMS", d.join("dist/bin/b"))
                },
                not_regular,
            ),
            (
                "symlinked-bin",
                |d| {
                    fs::rename(d.join("dist/bin"), d.join("dist/real"))?;
                    symlink("real", d.join("dist/bin"))
                },
                "dist/bin must be a real directory, not a symlink",
            ),
            (
                "symlinked-sums",
                |d| {
                    fs::rename(d.join("dist/SHA256SUMS"), d.join("dist/sums"))?;
                    symlink("sums", d.join("dist/SHA256SUMS"))
                },
                "dist/SHA256SUMS must be a regular file, not a symlink",
            ),
            (
                "symlinked-dist",
                |d| {
                    fs::rename(d.join("dist"), d.join("real"))?;
                    symlink("real", d.join("dist"))
                },
                "dist must be a real directory, not a symlink",
            ),
            (
                "crlf",
                |d| write_sums(d, &SIGNER_SUMS.replace('\n', "\r\n")),
                malformed,
            ),
            (
                "space",
                |d| write_sums(d, &SIGNER_SUMS.replace("  b", "  b c")),
                malformed,
            ),
            (
                "malformed",
                |d| write_sums(d, &SIGNER_SUMS.replace("  a", "  a x")),
                malformed,
            ),
            ("empty", |d| write_sums(d, ""), malformed),
        ];
        for (name, change, expected) in cases {
            let dir = root.join(name);
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("dist/bin"))?;
            fs::write(dir.join("dist/bin/a"), "a")?;
            fs::write(dir.join("dist/bin/b"), "b")?;
            write_sums(&dir, SIGNER_SUMS)?;
            change(&dir)?;
            let runner = dir.join(if name.contains("spaced") {
                "run ner"
            } else {
                "runner"
            });
            let output = Command::new(&script)
                .current_dir(&dir)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("DIGEST", "0".repeat(64))
                .env("RUNNER_TEMP", &runner)
                .output()?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!output.status.success(), "{name}: the signer went on");
            assert!(
                stderr.contains(expected) || stdout.contains(expected),
                "{name}: expected {expected:?}, got {stdout:?} {stderr:?}"
            );
            for left in ["release-key", "release-tools", "release-verified"] {
                assert!(
                    !runner.join(left).exists(),
                    "{name}: {left} is left or was reached"
                );
            }
            assert!(
                !dir.join("signatures").exists(),
                "{name}: the signer reached signing"
            );
        }
        Ok(())
    }

    fn runner(arch: Arch) -> &'static str {
        match arch {
            Arch::Amd64 => "ubuntu-24.04",
            Arch::Arm64 => "ubuntu-24.04-arm",
        }
    }

    /// Pull requests cancel their own stale runs; pushes to `main` share one group that cancels
    /// nothing, so a burst of merges releases once, at its tip; a manual dispatch, which can wait
    /// on the founder's approval, has its own group so it never holds the pushes' releases
    /// (DEC-2025).
    #[test]
    fn main_releases_once_per_burst_and_a_dispatch_never_holds_it() -> Result<()> {
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/release.yml"),
        )?;
        let block = concat!(
            "\nconcurrency:\n",
            "  group: release-${{ github.event_name == 'pull_request' && github.ref || ",
            "github.event_name == 'workflow_dispatch' && !inputs.post_merge && ",
            "format('dispatch-{0}', github.ref) || ",
            "github.ref }}\n",
            "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n",
        );
        assert!(
            text.contains(block),
            "release.yml's concurrency is not DEC-2025's"
        );
        assert_eq!(
            text.matches("concurrency:").count(),
            1,
            "a job sets its own concurrency"
        );
        assert!(!text.contains("github.sha }}\n  cancel-in-progress"));
        Ok(())
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

    /// One case of [`the_signed_images_refuse_a_release_the_key_did_not_sign`]: its name, what it
    /// changes in the release [`signed_release`] wrote under the directory it is handed, and what
    /// the refusal must name.
    type SignedCase = (
        &'static str,
        fn(&Path) -> io::Result<()>,
        &'static [&'static str],
    );

    /// Writes under `dir` a release `signed_images` accepts: `dist` holding `a` and `b` with their
    /// `SHA256SUMS`, a signature for each and for `SHA256SUMS` in `signatures`, the public half in
    /// `release.pub`, and in `tools` a stand-in cosign that records each call and verifies a
    /// signature only when it starts `good` and the key holds `release key`. Returns the tools
    /// directory and the key.
    fn signed_release(dir: &Path) -> Result<(PathBuf, PathBuf)> {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir.join("dist/bin"))?;
        fs::create_dir_all(dir.join("signatures"))?;
        fs::create_dir_all(dir.join("tools"))?;
        fs::write(dir.join("dist/bin/a"), "a")?;
        fs::write(dir.join("dist/bin/b"), "b")?;
        write_sums(dir, SIGNER_SUMS)?;
        fs::write(dir.join("signatures/a.sig"), "good signature over a")?;
        fs::write(dir.join("signatures/b.sig"), "good signature over b")?;
        fs::write(
            dir.join("signatures/SHA256SUMS.sig"),
            "good signature over the sums",
        )?;
        let key = dir.join("release.pub");
        fs::write(&key, "release key\n")?;
        let cosign = dir.join("tools/cosign");
        fs::write(
            &cosign,
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$*\" >> \"$(dirname \"$0\")/calls\"\n",
                "[ \"$1\" = verify-blob ] && grep -qx 'release key' \"$4\" && grep -q '^good' \"$6\"\n",
            ),
        )?;
        fs::set_permissions(&cosign, fs::Permissions::from_mode(0o755))?;
        Ok((dir.join("tools"), key))
    }

    /// Each release-signed image holds exactly its binary and that binary's release signature,
    /// byte for byte, as a runtime unpacks it, and every binary and `SHA256SUMS` was verified
    /// against the committed key first (DEC-2026).
    #[test]
    fn the_signed_images_carry_each_binary_and_its_release_signature() -> Result<()> {
        let dir = env::temp_dir().join(format!("mandate-xtask-signed-{}", std::process::id()));
        let (tools, key) = signed_release(&dir)?;
        let arch = Arch::host()?;
        let images = signed_images(
            &tools,
            &key,
            &dir.join("dist"),
            &dir.join("signatures"),
            &dir.join("out"),
            arch,
            "release",
        )?;
        let names: Vec<&str> = images.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        for (name, layout) in &images {
            let unpacked = dir.join("unpacked").join(name);
            let written = dir.join("out/images").join(name);
            assert!(written.join("index.json").is_file(), "{name}: not written");
            image::unpack(layout, &written, &unpacked.join("rootfs"))?;
            let mut files: Vec<String> = fs::read_dir(unpacked.join("rootfs"))?
                .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
                .collect::<io::Result<_>>()?;
            files.sort();
            assert_eq!(files, [name.clone(), format!("{name}.sig")], "{name}");
            assert_eq!(
                fs::read(unpacked.join("rootfs").join(name))?,
                fs::read(dir.join("dist/bin").join(name))?,
                "{name}: the binary"
            );
            assert_eq!(
                fs::read(unpacked.join("rootfs").join(format!("{name}.sig")))?,
                fs::read(dir.join("signatures").join(format!("{name}.sig")))?,
                "{name}: the signature"
            );
        }
        let calls = fs::read_to_string(tools.join("calls"))?;
        let mut verified: Vec<&str> = calls
            .lines()
            .map(|call| {
                assert!(
                    call.starts_with("verify-blob ")
                        && call.contains(&format!("--key {} ", key.display())),
                    "{call}"
                );
                call.rsplit('/').next().unwrap_or_default()
            })
            .collect();
        verified.sort_unstable();
        assert_eq!(verified, ["SHA256SUMS", "a", "b"]);
        Ok(())
    }

    /// A release the key did not sign builds no image: a signature by another key, a missing one,
    /// one over `SHA256SUMS` that does not verify, or a binary `SHA256SUMS` does not list each
    /// stops `signed_images` with a refusal naming it, and nothing is written under `out` or beside
    /// the tools, so every check runs before any image is built and `publish-signed` never signs in
    /// to the registry or pushes (DEC-2026).
    #[test]
    fn the_signed_images_refuse_a_release_the_key_did_not_sign() -> Result<()> {
        let root = env::temp_dir().join(format!(
            "mandate-xtask-signed-refusals-{}",
            std::process::id()
        ));
        let cases: Vec<SignedCase> = vec![
            (
                "a's signature is not the key's",
                |d| fs::write(d.join("signatures/a.sig"), "bad"),
                &["verify-blob", "a.sig"],
            ),
            (
                "b has no signature",
                |d| fs::remove_file(d.join("signatures/b.sig")),
                &["verify-blob", "b.sig"],
            ),
            (
                "the sums' signature is not the key's",
                |d| fs::write(d.join("signatures/SHA256SUMS.sig"), "bad"),
                &["verify-blob", "SHA256SUMS.sig"],
            ),
            (
                "b is not the binary SHA256SUMS lists",
                |d| fs::write(d.join("dist/bin/b"), "x"),
                &["b is not the binary SHA256SUMS lists"],
            ),
            (
                "another key",
                |d| fs::write(d.join("release.pub"), "another key\n"),
                &["verify-blob", "/release.pub", "/signatures/a.sig"],
            ),
        ];
        for (name, change, refusal) in cases {
            let dir = root.join(name.replace(' ', "-"));
            let (tools, key) = signed_release(&dir)?;
            change(&dir)?;
            let result = signed_images(
                &tools,
                &key,
                &dir.join("dist"),
                &dir.join("signatures"),
                &dir.join("out"),
                Arch::host()?,
                "release",
            );
            let Err(err) = result else {
                panic!("{name}: images were built");
            };
            let err = format!("{err:#}");
            for part in refusal {
                assert!(err.contains(part), "{name}: refused with {err:?}");
            }
            assert!(!dir.join("out").exists(), "{name}: an image was written");
            let mut tools: Vec<String> = fs::read_dir(&tools)?
                .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
                .collect::<io::Result<_>>()?;
            tools.sort();
            assert_eq!(tools, ["calls", "cosign"], "{name}: the tools directory");
        }
        Ok(())
    }
}
