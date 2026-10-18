//! The local and CI cell (E21-37): `cargo xtask cell up` brings up a k3d cluster (k3s in
//! containers, cell #1's own distribution) and installs the workspace chart from images this
//! change built; `cargo xtask cell drill` runs the drills against it; `cargo xtask cell compose`
//! renders the Compose file from the chart's process list (E21-36). No cloud account, no broker
//! credential and no broker host is involved (DEC-967 item 3). Results are local-cell results,
//! never staging's (DEC-967 item 5).
//!
//! kubectl is the one inside the k3s node (`docker exec`), so the cell needs only docker, k3d and
//! helm, each pinned in `.github/workflows/ci.yml` and `.cursor/install.sh` (D-4, DEC-969).
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream, ToSocketAddrs};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::Digest;

use crate::{output, sh};

pub mod scenario;

pub const CLUSTER: &str = "mandate-cell";
const NODE: &str = "k3d-mandate-cell-server-0";
/// The workspace the drills deploy, and its namespace: one release per workspace (§3.2).
const WORKSPACE: &str = "ws1";
const NAMESPACE: &str = "ws-ws1";
/// k3s for the cell, pinned by digest (D-5): the version k3d v5.9.0 defaults to.
const K3S_IMAGE: &str = "rancher/k3s:v1.35.5-k3s1@sha256:2074403abe1bded11ef3dde09d457e13be8e0b64c218b1c4f8269b4565cfbc65";
/// The CloudNativePG operator's chart, from its GitHub release, checked by SHA-256, and the
/// operator image it installs, pinned by digest (D-5).
const CNPG_CHART_URL: &str = "https://github.com/cloudnative-pg/charts/releases/download/cloudnative-pg-v0.29.1/cloudnative-pg-0.29.1.tgz";
const CNPG_CHART_SHA256: &str = "b53d3991fe84bcf38767e7702cae78666265427a127a26fff168ab4207d2b1df";
const CNPG_OPERATOR_TAG: &str =
    "1.30.1@sha256:923c267ec29636db3bee20f993d0ec4973fa22998e1adad37da79e4d32b5bc07";
const CNPG_NAMESPACE: &str = "cnpg-system";
pub const WORKSPACE_CHART: &str = "deploy/cell/charts/workspace";
pub const JOURNAL_CHART: &str = "deploy/cell/charts/journal-db";
const WORKSPACE_VALUES: &str = "deploy/cell/values/k3d.yaml";
const JOURNAL_VALUES: &str = "deploy/cell/values/k3d-journal-db.yaml";
pub const PROCESSES: &str = "deploy/cell/charts/workspace/processes.json";
pub const COMPOSE_FILE: &str = "deploy/cell/compose/compose.json";
const IMAGE_DOCKERFILE: &str = "deploy/cell/images/mandate.Dockerfile";
/// The workspace API server's image, built beside `mandate` from the same static build.
const API_DOCKERFILE: &str = "deploy/cell/images/api.Dockerfile";
/// The local stub token issuer's image (DEC-2100, DEC-1025), a sidecar in the API pod.
const STUB_DOCKERFILE: &str = "deploy/cell/images/stub-issuer.Dockerfile";
/// Lane SI's stub issuer crate (E21-52); the cell runs it only once it is in the workspace.
const STUB_CRATE: &str = "crates/mandate-stub-issuer/Cargo.toml";
/// The stub's entry point: its tests land before its code, with this file a `todo!()` stub
/// (DEC-137), which the cell must not run, since the pod would never start.
const STUB_MAIN: &str = "crates/mandate-stub-issuer/src/main.rs";
/// What marks an entry point still pending (DEC-137).
const PENDING_BODY: &str = "todo!(";
/// The server's key-set tier check (E21-53); DEC-2100 item 9 runs the stub only once it is in.
const KEY_SET_TIER_CHECK: (&str, &str) = (
    "crates/mandate-api-server/src/config.rs",
    "pub fn check_key_set_tier(",
);
/// Where a Linux host keeps its CA bundle, in the order `cell up` looks after `SSL_CERT_FILE`:
/// Debian and Ubuntu, Fedora and RHEL, openSUSE, Alpine. The bundle is copied into the `api`
/// image: on an empty base the server finds no roots, cannot build its key-set client, and refuses
/// to start (`MANDATE_API_JWKS_URL`).
const HOST_CA_BUNDLES: [&str; 4] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];
/// Where the cell keeps what it derives: the kubeconfig, the downloaded operator chart, and the
/// image build context. Under `target/`, so nothing derived is ever committed.
const STATE: &str = "target/cell";
/// Hosts that place live orders (Robinhood's agent and trading APIs, Alpaca's live API). No
/// process of the cell may reach any of them (OPS-5, DEC-821 item 4).
const LIVE_TRADING_HOSTS: [&str; 3] = [
    "agent.robinhood.com",
    "api.robinhood.com",
    "api.alpaca.markets",
];
/// An extra CA bundle the k3s node trusts, for a developer machine behind a TLS-inspecting proxy.
const EXTRA_CA_ENV: &str = "MANDATE_CELL_CA_BUNDLE";
/// Set by CI: a missing docker, k3d or helm fails the cell job instead of skipping it.
pub const CELL_REQUIRED_ENV: &str = "MANDATE_CELL_REQUIRED";
/// Polls of a Job, a pod or the journal cluster, two seconds apart.
const POLLS: u32 = 150;

/// The drills of this slice; each grows as the processes it needs land (E21-37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drill {
    /// The release is installed as rendered: the migration Job succeeded before any process,
    /// and every object the chart renders, each network policy included, exists in the cell.
    Boot,
    /// The control CLI opens a control stream in the cell's journal, and a Job exports it and
    /// verifies it by journal spec §11.
    Journal,
    /// From a pod of every process type, each live trading host fails at the network layer,
    /// while a process with a Postgres route reaches the journal and one without does not (OPS-5).
    Egress,
    /// The Compose file on the host's docker: Postgres and the migration come up, the CLI opens a
    /// control stream, and it exports and verifies by journal spec §11. Needs no cluster.
    Compose,
    /// The journal's Postgres primary is killed; once CloudNativePG brings it back, a stream
    /// written before the kill still exports and verifies by journal spec §11.
    Restart,
    /// The kill switch with the exporter, the API and the deployment manager down (OPS-4, E21-34);
    /// skipped, and named, until its processes and `mandate agent kill` are on `main`.
    KillSwitch,
}

pub const DRILLS: [Drill; 6] = [
    Drill::Boot,
    Drill::Journal,
    Drill::Egress,
    Drill::Compose,
    Drill::Restart,
    Drill::KillSwitch,
];

impl Drill {
    pub fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Journal => "journal",
            Self::Egress => "egress",
            Self::Compose => "compose",
            Self::Restart => "restart",
            Self::KillSwitch => "kill-switch",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        DRILLS
            .into_iter()
            .find(|d| d.name() == name)
            .with_context(|| format!("no drill named `{name}`"))
    }

    /// What the drill plants in seeded mode, which it must then report as broken.
    fn seeded_bug(self) -> &'static str {
        match self {
            Self::Boot => "the default-deny network policy deleted from the cell",
            Self::Journal => {
                "the first event's hash changed in the exported segment before it is verified"
            }
            Self::Egress => {
                "every network policy of the workspace release deleted, and the probes ungated"
            }
            Self::Compose => {
                "the first event's hash changed in the exported segment before it is verified"
            }
            Self::Restart => {
                "the stream lost across the restart: a stream id the restart did not keep"
            }
            Self::KillSwitch => "none until the drill runs",
        }
    }
}

impl Drill {
    /// What a seeded run's report must contain for the catch to count: the planted bug's own
    /// symptom, so a drill that broke for another reason (a pull error, an export that failed)
    /// is not taken for a drill that works.
    fn seeded_symptom(self) -> &'static str {
        match self {
            Self::Boot => "NetworkPolicy/default-deny is not in the cell",
            Self::Journal | Self::Compose => "rehash_mismatch",
            Self::Egress => "reached the live trading host",
            Self::Restart => "export_stream_empty",
            Self::KillSwitch => "",
        }
    }
}

/// A drill's outcome: the property held, or it did not and why. An error is the harness itself
/// failing, which never counts as either.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Held,
    Broken(String),
    /// The drill's processes are not on `main` yet; named, never counted as held.
    Skipped(String),
}

/// Whether docker, k3d and helm are all on the path; when one is not, the reason.
pub fn tools_missing() -> Option<String> {
    for (tool, arg) in [("docker", "info"), ("k3d", "version"), ("helm", "version")] {
        let ok = Command::new(tool)
            .arg(arg)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Some(format!("`{tool} {arg}` did not succeed"));
        }
    }
    None
}

/// `cargo xtask ci cell`: the chart checks, then the cell itself, every drill, every drill again
/// with its bug planted, and the demo without `web/`, which upgrades the releases in place. Skipped without docker, k3d and helm unless CI requires it.
pub fn ci() -> Result<()> {
    if let Some(reason) = tools_missing() {
        if env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    cell: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    chart_checks()?;
    up()?;
    drill(&DRILLS, false)?;
    drill(&DRILLS, true)?;
    demo(false)
}

/// helm lint and a strict schema check of every rendering of both charts, then the chart's
/// invariant tests, which render it themselves (deploy/tests/test_cell.py).
pub fn chart_checks() -> Result<()> {
    for (chart, values) in [
        (WORKSPACE_CHART, "deploy/cell/values/k3d.yaml"),
        (WORKSPACE_CHART, "deploy/cell/values/k3s.yaml"),
        (WORKSPACE_CHART, "deploy/cell/values/managed.yaml"),
        (JOURNAL_CHART, "deploy/cell/values/k3d-journal-db.yaml"),
    ] {
        sh("helm", &["lint", "--strict", chart, "-f", values])?;
        let rendered = output(
            "helm",
            &["template", "cell", chart, "-n", NAMESPACE, "-f", values],
        )?;
        kubeconform(&rendered)?;
        no_live_host_allowed(values, &parse_documents(&rendered)?, &live_addresses()?)?;
    }
    let status = Command::new("uv")
        .args([
            "run",
            "--locked",
            "--directory",
            "python",
            "pytest",
            "-q",
            "../deploy/tests/test_cell.py",
        ])
        .env("MANDATE_CELL_HELM_REQUIRED", "1")
        .status()
        .context("starting pytest")?;
    if !status.success() {
        bail!("the chart's invariant tests failed with {status}");
    }
    Ok(())
}

/// Every rendered object against the Kubernetes 1.35 schemas; CloudNativePG's `Cluster` has no
/// schema there and is skipped by kind, never by a missing-schema wildcard.
fn kubeconform(rendered: &str) -> Result<()> {
    let mut child = Command::new("kubeconform")
        .args([
            "-strict",
            "-summary",
            "-kubernetes-version",
            "1.35.0",
            "-skip",
            "Cluster",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .context("starting kubeconform (is it installed? see AGENTS.md)")?;
    child
        .stdin
        .take()
        .context("kubeconform's stdin")?
        .write_all(rendered.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!("kubeconform refused the rendered chart: {status}");
    }
    Ok(())
}

/// No network policy of a rendering allows a live trading host's address (DEC-821 item 4): the
/// values check the schema cannot make, since addresses are resolved, not written.
fn no_live_host_allowed(
    values: &str,
    rendered: &[Value],
    live: &BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let mut problems = Vec::new();
    for policy in rendered.iter().filter(|o| o["kind"] == "NetworkPolicy") {
        let rules = policy["spec"]["egress"].as_array().into_iter().flatten();
        for cidr in rules
            .flat_map(|r| r["to"].as_array().into_iter().flatten())
            .filter_map(|peer| peer["ipBlock"]["cidr"].as_str())
        {
            for (host, addrs) in live {
                for addr in addrs.iter().filter(|a| cidr_contains(cidr, a)) {
                    problems.push(format!(
                        "{}: {cidr} allows {host} ({addr})",
                        policy["metadata"]["name"]
                    ));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        bail!(
            "{values} lets a process reach a live trading host:\n  {}",
            problems.join("\n  ")
        )
    }
}

/// Whether the IPv4 `cidr` contains `addr`; anything that does not parse counts as containing it.
fn cidr_contains(cidr: &str, addr: &str) -> bool {
    let parsed = cidr.split_once('/').and_then(|(net, len)| {
        Some((
            net.parse::<Ipv4Addr>().ok()?,
            len.parse::<u32>().ok().filter(|l| *l <= 32)?,
            addr.parse::<Ipv4Addr>().ok()?,
        ))
    });
    let Some((net, len, addr)) = parsed else {
        return true;
    };
    let mask = u32::MAX.checked_shl(32 - len).unwrap_or(0);
    u32::from(net) & mask == u32::from(addr) & mask
}

fn state() -> Result<PathBuf> {
    let dir = PathBuf::from(STATE);
    fs::create_dir_all(&dir).with_context(|| format!("creating {STATE}"))?;
    Ok(dir)
}

fn kubeconfig() -> Result<String> {
    Ok(state()?.join("kubeconfig").display().to_string())
}

/// Names a published, signed `mandate` image (lane R2, E21-35) by digest, for a host that cannot
/// build the static Linux binary itself.
const IMAGE_ENV: &str = "MANDATE_CELL_IMAGE";

/// Where the cell's `mandate` image comes from.
#[derive(Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// Built from this checkout: only a linux/x86_64 host can link the static binary.
    Build,
    /// Pulled by digest: the R2 lane's signed image, named in `MANDATE_CELL_IMAGE`.
    Pull(String),
}

/// The image source for a host `os`/`arch` and an optional pinned image. A pinned image must name
/// a digest; any other host than linux/x86_64 needs one, since cargo cannot link the static Linux
/// binary there without a cross toolchain, which no dependency kind covers (DEC-990 item 1).
pub fn image_source(os: &str, arch: &str, pinned: Option<&str>) -> Result<ImageSource, String> {
    match pinned.map(str::trim).filter(|p| !p.is_empty()) {
        Some(image) if pinned_by_digest(image) => Ok(ImageSource::Pull(image.to_string())),
        Some(image) => Err(format!(
            "{IMAGE_ENV}={image} is not `<repository>[:tag]@sha256:<64 lowercase hex>`, the form \
             the workspace chart's schema takes"
        )),
        None if os == "linux" && arch == "x86_64" => Ok(ImageSource::Build),
        None => Err(format!(
            "this host is {os}/{arch}, where the static Linux `mandate` binary cannot be built; \
             set {IMAGE_ENV} to the R2 lane's signed image by digest (E21-35). Until R2 publishes \
             linux/arm64 and amd64 images there is none to pull, and the cell runs on linux/x86_64 \
             only (deploy/cell/README.md, \"On a laptop\")"
        )),
    }
}

/// The workspace chart's image form (`values.schema.json`):
/// `^[a-z0-9./_-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}$`.
fn pinned_by_digest(image: &str) -> bool {
    let Some((name, digest)) = image.split_once("@sha256:") else {
        return false;
    };
    let (repository, tag) = name.split_once(':').unwrap_or((name, ""));
    let digest_ok = digest.len() == 64
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let repository_ok = !repository.is_empty()
        && repository
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'/' | b'_' | b'-'));
    let tag_ok = !name.contains(':')
        || (!tag.is_empty()
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')));
    digest_ok && repository_ok && tag_ok
}

fn host_image_source() -> Result<ImageSource> {
    let pinned = env::var(IMAGE_ENV).ok();
    image_source(env::consts::OS, env::consts::ARCH, pinned.as_deref()).map_err(anyhow::Error::msg)
}

/// The cell's `mandate` image: the pulled one, or this checkout's build tagged by its binaries'
/// SHA-256s, the CLI's and the backtest runner's beside it (DEC-1144), so an uncommitted edit to
/// either gets a new image rather than the last one imported.
fn image_tag() -> Result<String> {
    if let ImageSource::Pull(image) = host_image_source()? {
        return Ok(image);
    }
    local_tag("mandate", &["mandate", RUNNER])
}

/// The backtest runner (DEC-1144), built with the CLI and copied into its image.
const RUNNER: &str = "mandate-backtest-runner";

/// The cell's `api` image, this checkout's build of `mandate-api-server`: none when the cell pulls
/// its `mandate` image, since no published API image exists yet, so the chart leaves the API out.
fn api_image_tag() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag("api", &["mandate-api-server"]).map(Some)
}

/// Whether this checkout can run the stub issuer in the cell: lane SI's crate is in the workspace
/// with its entry point implemented, not still the `todo!()` its tests land with, and the server
/// refuses an `http` key set outside dev and CI (DEC-2100 item 9). Until all hold, the cell runs
/// no stub, mounts no subject links, and every sign-in is refused.
pub fn stub_ready(root: &Path) -> bool {
    let (server, check) = KEY_SET_TIER_CHECK;
    root.join(STUB_CRATE).is_file()
        && fs::read_to_string(root.join(STUB_MAIN)).is_ok_and(|main| !main.contains(PENDING_BODY))
        && fs::read_to_string(root.join(server)).is_ok_and(|code| code.contains(check))
}

/// The cell's stub issuer image: this checkout's dev-profile build, since the stub is compiled
/// only with debug assertions (DEC-2100 item 2); none without an API image or a ready stub.
fn stub_image_tag() -> Result<Option<String>> {
    if api_image_tag()?.is_none() || !stub_ready(Path::new(".")) {
        return Ok(None);
    }
    local_tag("stub-issuer", &["mandate-stub-issuer"]).map(Some)
}

/// The values the API's sign-in pieces take: the stub's image, and the subject-link ConfigMap the
/// demo writes, both empty without a stub, so nothing is mounted and every sign-in is refused.
fn signin_values(stub: Option<&str>) -> [String; 2] {
    [
        format!("images.stub-issuer={}", stub.unwrap_or_default()),
        format!(
            "api.subjectLinks={}",
            if stub.is_some() { LINKS_CONFIGMAP } else { "" }
        ),
    ]
}

/// `mandate.local/<name>:<the first 12 hex digits of the binaries' digest>` ([`combined`]).
fn local_tag(name: &str, binaries: &[&str]) -> Result<String> {
    let digests = binaries
        .iter()
        .map(|binary| sha256(&static_binary(binary)))
        .collect::<Result<Vec<_>>>()?;
    let digest = combined(&digests);
    let short = digest
        .get(..12)
        .context("a SHA-256 shorter than 12 digits")?;
    Ok(format!("mandate.local/{name}:{short}"))
}

/// One binary's SHA-256 as it is, so a one-binary image keeps the tag it always had; several
/// binaries' the SHA-256 of their digests, one a line, in order, so a change to any one of them
/// changes the tag.
fn combined(digests: &[String]) -> String {
    match digests {
        [one] => one.clone(),
        many => {
            let joined = many.join("\n");
            sha2::Sha256::digest(joined.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        }
    }
}

fn static_binary(binary: &str) -> String {
    format!("{STATIC_TARGET}/{STATIC_TRIPLE}/debug/{binary}")
}

/// `cargo xtask cell up`: build the `mandate` and `api` images from this checkout (never with the
/// `live` feature), bring up the k3d cluster if it is not up, install the CloudNativePG operator, the
/// journal's Postgres and the workspace chart, and wait until every pod is ready and the
/// migration Job has finished.
pub fn up() -> Result<()> {
    let source = host_image_source()?;
    if source == ImageSource::Build {
        build_binary()?;
    }
    let image = image_tag()?;
    let api = api_image_tag()?;
    let stub = stub_image_tag()?;
    if source == ImageSource::Build {
        let runner = static_binary(RUNNER);
        build_image(&image, IMAGE_DOCKERFILE, "mandate", &[(&runner, RUNNER)])?;
    }
    if let Some(stub) = &stub {
        build_image(stub, STUB_DOCKERFILE, "mandate-stub-issuer", &[])?;
    }
    if let Some(api) = &api {
        build_image(
            api,
            API_DOCKERFILE,
            "mandate-api-server",
            &[(&host_ca_bundle()?, "ca-certificates.crt")],
        )?;
    }
    if !cluster_exists()? {
        create_cluster()?;
    }
    if source == ImageSource::Build {
        sh("k3d", &["image", "import", "--cluster", CLUSTER, &image])?;
    }
    if let Some(api) = &api {
        sh("k3d", &["image", "import", "--cluster", CLUSTER, api])?;
    }
    if let Some(stub) = &stub {
        sh("k3d", &["image", "import", "--cluster", CLUSTER, stub])?;
    }
    let config = kubeconfig()?;
    sh(
        "k3d",
        &[
            "kubeconfig",
            "write",
            CLUSTER,
            "--output",
            &config,
            "--overwrite",
        ],
    )?;
    let chart = operator_chart()?;
    let operator_tag = format!("image.tag={CNPG_OPERATOR_TAG}");
    helm(&[
        "upgrade",
        "--install",
        "cnpg",
        &chart,
        "--namespace",
        CNPG_NAMESPACE,
        "--create-namespace",
        "--set",
        &operator_tag,
        "--wait",
        "--timeout",
        "5m",
    ])?;
    helm(&[
        "upgrade",
        "--install",
        "journal",
        JOURNAL_CHART,
        "--namespace",
        NAMESPACE,
        "--create-namespace",
        "-f",
        JOURNAL_VALUES,
        "--wait",
        "--timeout",
        "5m",
    ])?;
    kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        "cluster.postgresql.cnpg.io/journal",
        "--timeout=300s",
    ])?;
    if stub.is_some() {
        demo_links(&image)?;
    }
    let image_value = format!("images.mandate={image}");
    let api_value = format!("images.api={}", api.clone().unwrap_or_default());
    let [stub_value, links_value] = signin_values(stub.as_deref());
    let installed = helm(&[
        "upgrade",
        "--install",
        "workspace",
        WORKSPACE_CHART,
        "--namespace",
        NAMESPACE,
        "-f",
        WORKSPACE_VALUES,
        "--set",
        &image_value,
        "--set",
        &api_value,
        "--set",
        &stub_value,
        "--set",
        &links_value,
        "--wait",
        "--wait-for-jobs",
        "--timeout",
        "5m",
    ]);
    if installed.is_err() && api.is_some() {
        api_diagnosis();
    }
    installed?;
    eprintln!("    cell: up; `cargo xtask cell drill` runs the drills");
    Ok(())
}

/// What the API's pod says when the workspace release does not become ready: its pods, its
/// containers' last logs, current and from the run before a restart, and the namespace's recent
/// events, so a server that refused to start
/// names its refusal in the run's own output.
fn api_diagnosis() {
    let selector = format!("mandate.dev/workspace={WORKSPACE},mandate.dev/process={API_PROCESS}");
    let logs = |previous: &'static str| {
        [
            "logs",
            "--namespace",
            NAMESPACE,
            "--selector",
            selector.as_str(),
            "--all-containers",
            "--prefix",
            previous,
            "--tail",
            "40",
        ]
    };
    let pods = [
        "get",
        "pods",
        "--namespace",
        NAMESPACE,
        "--selector",
        selector.as_str(),
        "-o",
        "wide",
    ];
    let current = logs("--previous=false");
    let previous = logs("--previous=true");
    let events = [
        "get",
        "events",
        "--namespace",
        NAMESPACE,
        "--sort-by",
        ".lastTimestamp",
    ];
    let shown: [&[&str]; 4] = [&pods, &current, &previous, &events];
    for args in shown {
        match kubectl(args) {
            Ok(text) => eprintln!("    api diagnosis `kubectl {}`:\n{text}", args.join(" ")),
            Err(e) => eprintln!("    api diagnosis `kubectl {}`: {e:#}", args.join(" ")),
        }
    }
}

/// `cargo xtask cell down`: delete the cluster and everything in it.
pub fn down() -> Result<()> {
    if cluster_exists()? {
        sh("k3d", &["cluster", "delete", CLUSTER])?;
    }
    Ok(())
}

/// Where the static build goes: its own target directory, so its flags never invalidate the
/// workspace's own build.
const STATIC_TARGET: &str = "target/cell-static";
const STATIC_TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// `mandate-cli`, the backtest runner and `mandate-api-server` linked statically, so their images
/// need no base (DEC-993 item 9), with no feature at all: never `live`; with them the stub issuer
/// once [`stub_ready`], which the dev profile's debug assertions are what compile in (DEC-2100 item
/// 2). The dev profile, for the CI job's ten minutes (DEC-464): it keeps overflow checks, and the
/// cell's journals are small.
fn build_binary() -> Result<()> {
    let mut args = vec![
        "build",
        "--locked",
        "--package",
        "mandate-cli",
        "--package",
        RUNNER,
        "--package",
        "mandate-api-server",
    ];
    if stub_ready(Path::new(".")) {
        args.extend(["--package", "mandate-stub-issuer"]);
    }
    args.extend(["--target", STATIC_TRIPLE, "--target-dir", STATIC_TARGET]);
    eprintln!(
        "    $ RUSTFLAGS='-C target-feature=+crt-static' cargo {}",
        args.join(" ")
    );
    let status = Command::new("cargo")
        .args(args)
        .env("RUSTFLAGS", "-C target-feature=+crt-static")
        .status()
        .context("starting cargo")?;
    if !status.success() {
        bail!("the static build of the cell's binaries failed with {status}");
    }
    Ok(())
}

/// The host's CA bundle for the `api` image: `SSL_CERT_FILE` when set, else the first of
/// [`HOST_CA_BUNDLES`] that is a file.
fn host_ca_bundle() -> Result<String> {
    let named = env::var("SSL_CERT_FILE").ok();
    ca_bundle_from(named.as_deref(), |path| Path::new(path).is_file())
}

/// The CA bundle `named` (`SSL_CERT_FILE`) when set, else the first of [`HOST_CA_BUNDLES`] for
/// which `exists` holds; refused, naming `SSL_CERT_FILE`, when none is found.
pub fn ca_bundle_from(named: Option<&str>, exists: impl Fn(&str) -> bool) -> Result<String> {
    if let Some(named) = named.map(str::trim).filter(|n| !n.is_empty()) {
        if exists(named) {
            return Ok(named.to_string());
        }
        bail!("SSL_CERT_FILE={named} is not a file; the api image needs the host's CA bundle");
    }
    HOST_CA_BUNDLES
        .iter()
        .find(|path| exists(path))
        .map(|path| (*path).to_string())
        .with_context(|| {
            format!(
                "no CA bundle at {}; set SSL_CERT_FILE to this host's, which the api image needs",
                HOST_CA_BUNDLES.join(", ")
            )
        })
}

/// The image `image` from `dockerfile`, over a context holding only `binary` and each of `files`,
/// copied from its host path under its context name.
fn build_image(image: &str, dockerfile: &str, binary: &str, files: &[(&str, &str)]) -> Result<()> {
    let context = state()?.join("image");
    if context.exists() {
        fs::remove_dir_all(&context).context("clearing the image context")?;
    }
    fs::create_dir_all(&context)?;
    let built = static_binary(binary);
    fs::copy(&built, context.join(binary))
        .with_context(|| format!("copying {built} into the image context"))?;
    for (source, name) in files {
        fs::copy(source, context.join(name)).with_context(|| {
            format!("copying {source} into the image context; the {image} image needs it")
        })?;
    }
    let context = context.display().to_string();
    sh(
        "docker",
        &["build", "--quiet", "-f", dockerfile, "-t", image, &context],
    )
}

fn cluster_exists() -> Result<bool> {
    let list = output("k3d", &["cluster", "list", "--output", "json"])?;
    let clusters: Value = serde_json::from_str(&list).context("reading `k3d cluster list`")?;
    Ok(clusters
        .as_array()
        .is_some_and(|all| all.iter().any(|c| c["name"] == CLUSTER)))
}

/// One server node, no load balancer, and neither Traefik nor ServiceLB: the cell's ingress is a
/// `cloudflared` pod (DEC-967 item 6), never a k3s add-on. On a cgroup v1 host k3s 1.35's
/// kubelet must be told to start anyway.
fn create_cluster() -> Result<()> {
    let mut args: Vec<String> = [
        "cluster",
        "create",
        CLUSTER,
        "--image",
        K3S_IMAGE,
        "--no-lb",
        "--k3s-arg",
        "--disable=traefik@server:0",
        "--k3s-arg",
        "--disable=servicelb@server:0",
        "--kubeconfig-update-default=false",
        "--wait",
        "--timeout",
        "180s",
    ]
    .map(String::from)
    .to_vec();
    if env::consts::OS == "linux" && !Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        args.extend(["--k3s-arg", "--kubelet-arg=fail-cgroupv1=false@server:0"].map(String::from));
    }
    if let Some(bundle) = env::var_os(EXTRA_CA_ENV) {
        let bundle = PathBuf::from(bundle);
        args.push("--volume".into());
        args.push(format!(
            "{}:/etc/ssl/certs/ca-certificates.crt@server:0",
            bundle.display()
        ));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    sh("k3d", &args)?;
    for _ in 0..POLLS {
        if kubectl(&["get", "--raw", "/readyz"]).is_ok() {
            return Ok(());
        }
        sleep(Duration::from_secs(2));
    }
    bail!("the cell's API server never became ready")
}

/// The operator chart from its GitHub release, kept under `target/cell` and checked by SHA-256
/// every time it is used.
fn operator_chart() -> Result<String> {
    let path = state()?.join("cloudnative-pg.tgz");
    let file = path.display().to_string();
    if !path.exists() || sha256(&file)? != CNPG_CHART_SHA256 {
        sh(
            "curl",
            &[
                "-sSfL",
                "--retry",
                "5",
                "--retry-all-errors",
                "-o",
                &file,
                CNPG_CHART_URL,
            ],
        )?;
    }
    let found = sha256(&file)?;
    if found != CNPG_CHART_SHA256 {
        bail!("the CloudNativePG chart's SHA-256 is {found}, not {CNPG_CHART_SHA256}");
    }
    Ok(file)
}

fn sha256(file: &str) -> Result<String> {
    let out = output("sha256sum", &[file]).or_else(|_| output("shasum", &["-a", "256", file]))?;
    out.split_whitespace()
        .next()
        .map(String::from)
        .context("no digest in sha256sum's output")
}

fn helm(args: &[&str]) -> Result<()> {
    let config = kubeconfig()?;
    let mut full = vec!["--kubeconfig", config.as_str()];
    full.extend_from_slice(args);
    sh("helm", &full)
}

/// kubectl in the k3s node, so the host needs no kubectl of its own.
fn kubectl(args: &[&str]) -> Result<String> {
    let mut full = vec!["exec", NODE, "kubectl"];
    full.extend_from_slice(args);
    output("docker", &full)
}

/// `kubectl apply` of `objects`, as JSON on stdin.
fn apply(objects: &[Value]) -> Result<()> {
    apply_as(objects, &[])
}

/// `kubectl apply` of `objects` with `extra` flags, as JSON on stdin.
fn apply_as(objects: &[Value], extra: &[&str]) -> Result<()> {
    let list = json!({"apiVersion": "v1", "kind": "List", "items": objects});
    let mut args = vec!["exec", "-i", NODE, "kubectl", "apply"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--namespace", NAMESPACE, "-f", "-"]);
    let mut child = Command::new("docker")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("starting kubectl apply")?;
    child
        .stdin
        .take()
        .context("kubectl's stdin")?
        .write_all(list.to_string().as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!("kubectl apply failed with {status}");
    }
    Ok(())
}

/// The chart rendered with the cell's values and `sets`, as the JSON documents it emits.
pub fn render(sets: &[String], show_only: Option<&str>) -> Result<Vec<Value>> {
    let image = format!("images.mandate={}", image_tag()?);
    let api = format!("images.api={}", api_image_tag()?.unwrap_or_default());
    let [stub, links] = signin_values(stub_image_tag()?.as_deref());
    let mut args = vec![
        "template",
        "workspace",
        WORKSPACE_CHART,
        "--namespace",
        NAMESPACE,
        "-f",
        WORKSPACE_VALUES,
        "--set",
        image.as_str(),
        "--set",
        api.as_str(),
        "--set",
        stub.as_str(),
        "--set",
        links.as_str(),
    ];
    for set in sets {
        args.push("--set-json");
        args.push(set);
    }
    if let Some(template) = show_only {
        args.push("--show-only");
        args.push(template);
    }
    parse_documents(&output("helm", &args)?)
}

/// The rendered chart's documents: every line that is not a separator or a `# Source:` comment
/// is one JSON object, as `mandate.emit` writes it.
pub fn parse_documents(rendered: &str) -> Result<Vec<Value>> {
    rendered
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "---" && !l.starts_with('#'))
        .map(|l| {
            serde_json::from_str(l).with_context(|| format!("a rendered line is not JSON: {l}"))
        })
        .collect()
}

/// `cargo xtask cell drill [--seeded] [names]`: each drill once; in seeded mode each with its bug
/// planted, and then a drill that still reports the property held fails the run.
pub fn drill(drills: &[Drill], seeded: bool) -> Result<()> {
    let mut failures = Vec::new();
    for &d in drills {
        if seeded {
            eprintln!("==> drill {} (seeded: {})", d.name(), d.seeded_bug());
        } else {
            eprintln!("==> drill {}", d.name());
        }
        let verdict = match d {
            Drill::Boot => boot(seeded),
            Drill::Journal => journal(seeded),
            Drill::Egress => egress(seeded),
            Drill::Compose => compose_drill(seeded),
            Drill::Restart => restart(seeded),
            Drill::KillSwitch => kill_switch(),
        };
        if seeded && matches!(d, Drill::Boot | Drill::Journal | Drill::Egress) {
            restore()?;
        }
        match (verdict?, seeded) {
            (Verdict::Skipped(why), _) => eprintln!("    skipped: {why}"),
            (Verdict::Held, false) => eprintln!("    held"),
            (Verdict::Broken(why), true) if why.contains(d.seeded_symptom()) => {
                eprintln!("    caught the seeded bug: {why}");
            }
            (Verdict::Broken(why), true) => failures.push(format!(
                "{}: broke with its seeded bug planted, but not by it (no `{}`): {why}",
                d.name(),
                d.seeded_symptom()
            )),
            (Verdict::Broken(why), false) => failures.push(format!("{}: {why}", d.name())),
            (Verdict::Held, true) => failures.push(format!(
                "{}: held with its seeded bug planted ({}), so it checks nothing",
                d.name(),
                d.seeded_bug()
            )),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("drills failed:\n  {}", failures.join("\n  "))
    }
}

/// Puts back every object of the release a seeded drill removed, as helm's own field manager, so a
/// later `helm upgrade` (the demo's, at the end of `ci cell`) can own them again; that upgrade is
/// the check that it does.
fn restore() -> Result<()> {
    apply_as(
        &release_objects()?,
        &["--server-side", "--field-manager=helm", "--force-conflicts"],
    )
}

/// The workspace release's objects as installed, hooks aside: `helm get manifest`.
fn release_objects() -> Result<Vec<Value>> {
    let config = kubeconfig()?;
    let manifest = output(
        "helm",
        &[
            "--kubeconfig",
            &config,
            "get",
            "manifest",
            "workspace",
            "--namespace",
            NAMESPACE,
        ],
    )?;
    parse_documents(&manifest)
}

/// Every object in the release's manifest, and the migration hook's own network policy and files,
/// exist in the cell, and the migration hook succeeded.
fn boot(seeded: bool) -> Result<Verdict> {
    if seeded {
        kubectl(&[
            "delete",
            "networkpolicy",
            "default-deny",
            "--namespace",
            NAMESPACE,
        ])?;
    }
    let migrate = kubectl(&[
        "get",
        "job",
        "migrate",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.status.succeeded}",
    ])?;
    if migrate.trim() != "1" {
        return Ok(Verdict::Broken(format!(
            "the migration Job has not succeeded (succeeded: `{}`)",
            migrate.trim()
        )));
    }
    let mut objects = release_objects()?;
    objects.extend(
        render(&[], Some("templates/migrate.yaml"))?
            .into_iter()
            .filter(|o| o["kind"] != "Job"),
    );
    if objects.is_empty() {
        return Ok(Verdict::Broken("the release rendered nothing".into()));
    }
    for object in &objects {
        let kind = object["kind"].as_str().unwrap_or_default();
        let name = object["metadata"]["name"].as_str().unwrap_or_default();
        let found = kubectl(&[
            "get",
            kind,
            name,
            "--namespace",
            NAMESPACE,
            "--ignore-not-found",
            "-o",
            "name",
        ])?;
        if found.trim().is_empty() {
            return Ok(Verdict::Broken(format!("{kind}/{name} is not in the cell")));
        }
    }
    Ok(Verdict::Held)
}

/// A unique suffix for the Jobs and the control stream a drill run makes.
fn run_id() -> Result<String> {
    let nanos = output("date", &["+%s%N"])?;
    Ok(nanos.trim().chars().rev().take(10).collect::<String>())
}

/// Opens a fresh control stream with the CLI in the cell, then exports and verifies it.
fn journal(seeded: bool) -> Result<Verdict> {
    let id = run_id()?;
    let stream_workspace = format!("drill{id}");
    let args = json!([
        "workspace",
        "open",
        "--workspace",
        stream_workspace,
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "/work/store"
    ]);
    let open = render(
        &[format!("run.id=\"{id}\""), format!("run.args={args}")],
        Some("templates/run.yaml"),
    )?;
    let (opened, logs) = run_job(open)?;
    if !opened {
        return Ok(Verdict::Broken(format!(
            "`mandate workspace open` failed in the cell:\n{logs}"
        )));
    }
    let streams = json!([format!("ctl:{stream_workspace}")]);
    let mut verify = render(
        &[
            format!("verify.id=\"{id}\""),
            format!("verify.streams={streams}"),
        ],
        Some("templates/run.yaml"),
    )?;
    if seeded {
        tamper_segment(&mut verify)?;
    }
    let (verified, logs) = run_job(verify)?;
    if !verified {
        return Ok(Verdict::Broken(format!(
            "the exported control stream did not verify:\n{logs}"
        )));
    }
    Ok(Verdict::Held)
}

/// Adds an init container after the exports that changes the first hex digit of the first event's
/// hash in the first segment, using the chart's own Postgres tools image and the pod's security
/// settings.
fn tamper_segment(jobs: &mut [Value]) -> Result<()> {
    let job = jobs.first_mut().context("the verify Job did not render")?;
    let spec = &mut job["spec"]["template"]["spec"];
    let image = spec["initContainers"][0]["image"].clone();
    let tamper = json!({
        "name": "seeded-tamper",
        "image": image,
        "command": [
            "sed",
            "-E",
            "-i",
            "1s/\"hash\":\"0/\"hash\":\"1/;t;1s/\"hash\":\"[1-9a-f]/\"hash\":\"0/",
            "/work/segment-0.jsonl"
        ],
        "securityContext": spec["initContainers"][0]["securityContext"].clone(),
        "resources": spec["initContainers"][0]["resources"].clone(),
        "volumeMounts": [{"name": "work", "mountPath": "/work"}]
    });
    spec["initContainers"]
        .as_array_mut()
        .context("the verify Job has no init containers")?
        .push(tamper);
    Ok(())
}

/// Applies a rendered Job and waits until it succeeds or fails; returns which, with its logs.
fn run_job(objects: Vec<Value>) -> Result<(bool, String)> {
    let job = objects
        .iter()
        .find(|o| o["kind"] == "Job")
        .context("no Job rendered")?;
    let name = job["metadata"]["name"]
        .as_str()
        .context("the Job has no name")?
        .to_string();
    apply(&objects)?;
    for _ in 0..POLLS {
        let status = kubectl(&[
            "get",
            "job",
            &name,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.succeeded}/{.status.failed}",
        ])?;
        let (succeeded, failed) = status.trim().split_once('/').unwrap_or(("", ""));
        if succeeded == "1" || failed == "1" {
            return Ok((succeeded == "1", job_logs(&name, job)));
        }
        sleep(Duration::from_secs(2));
    }
    bail!("Job {name} neither succeeded nor failed in time")
}

/// Every container's logs of a finished Job. `--all-containers` refuses a pod with a container that
/// never started, as when an init container failed, so it falls back to each container in turn,
/// keeping the failed init container's report, which is where a seeded symptom is.
fn job_logs(name: &str, job: &Value) -> String {
    let job_ref = format!("job/{name}");
    let all = kubectl(&[
        "logs",
        &job_ref,
        "--namespace",
        NAMESPACE,
        "--all-containers",
        "--prefix",
    ]);
    let Err(refused) = all else {
        return all.unwrap_or_default();
    };
    let spec = &job["spec"]["template"]["spec"];
    let containers = ["initContainers", "containers"]
        .iter()
        .filter_map(|k| spec[k].as_array())
        .flatten()
        .filter_map(|c| c["name"].as_str());
    let mut logs = String::new();
    for container in containers {
        if let Ok(out) = kubectl(&["logs", &job_ref, "--namespace", NAMESPACE, "-c", container]) {
            for line in out.lines() {
                logs.push_str(&format!("[{container}] {line}\n"));
            }
        }
    }
    if logs.is_empty() {
        format!("(no logs: {refused:#})")
    } else {
        logs
    }
}

/// The process list, as `processes.json` holds it.
pub fn processes() -> Result<Vec<Value>> {
    processes_in(Path::new("."))
}

fn processes_in(root: &Path) -> Result<Vec<Value>> {
    let text =
        fs::read_to_string(root.join(PROCESSES)).with_context(|| format!("reading {PROCESSES}"))?;
    let list: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {PROCESSES}"))?;
    list["processes"]
        .as_array()
        .cloned()
        .context("processes.json has no `processes` list")
}

/// The live trading hosts' IPv4 addresses, resolved on the host, so a probe needs no DNS answer to
/// try them and a refusal can only come from the network.
fn live_addresses() -> Result<BTreeMap<String, Vec<String>>> {
    let mut all = BTreeMap::new();
    for host in LIVE_TRADING_HOSTS {
        let addrs: Vec<String> = (host, 443)
            .to_socket_addrs()
            .with_context(|| format!("resolving {host}"))?
            .filter(|a| a.is_ipv4())
            .map(|a| a.ip().to_string())
            .collect();
        if addrs.is_empty() {
            bail!("{host} has no IPv4 address");
        }
        all.insert(host.to_string(), addrs);
    }
    Ok(all)
}

/// One probe pod per process type, labelled as the chart labels that process, trying every live
/// trading host and the journal. The pod is the chart's: non-root, read-only, no capability, and,
/// outside seeded mode, behind the chart's own egress gate, so the probe starts when a process
/// would. Seeded, the policies are gone and the gate would never open, so the probe goes without
/// it to show it can see a leak.
fn probe_pod(process: &Value, id: &str, image: &str, script: &str) -> Result<Value> {
    let name = process["name"].as_str().context("a process has no name")?;
    let mut labels = serde_json::Map::new();
    labels.insert("mandate.dev/workspace".into(), json!(WORKSPACE));
    labels.insert("mandate.dev/process".into(), json!(name));
    labels.insert("app.kubernetes.io/component".into(), json!(name));
    if process["colocate"] == true {
        labels.insert("mandate.dev/colocate".into(), json!("true"));
    }
    for target in process["egress"].as_array().into_iter().flatten() {
        let target = target
            .as_str()
            .context("an egress target is not a string")?;
        labels.insert(format!("mandate.dev/egress-{target}"), json!("true"));
    }
    Ok(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": format!("probe-{name}-{id}"), "labels": labels},
        "spec": {
            "restartPolicy": "Never",
            "automountServiceAccountToken": false,
            "securityContext": {
                "runAsNonRoot": true, "runAsUser": 65532, "runAsGroup": 65532,
                "seccompProfile": {"type": "RuntimeDefault"}
            },
            "containers": [{
                "name": "probe",
                "image": image,
                "command": ["bash", "-c", script],
                "securityContext": {
                    "allowPrivilegeEscalation": false, "readOnlyRootFilesystem": true,
                    "capabilities": {"drop": ["ALL"]}
                },
                "resources": {
                    "limits": {"cpu": "100m", "memory": "64Mi"},
                    "requests": {"cpu": "50m", "memory": "32Mi"}
                }
            }]
        }
    }))
}

/// The probe's script: one `reached` or `refused` line per address and one for the journal, tried
/// twice: at once, marked `early:`, since a policy controller may filter a new pod only after it
/// starts, and again ten seconds later. A live host reached on either pass is a leak. The journal
/// is tried by its primary's address, so DNS cannot decide the answer.
fn probe_script(addresses: &BTreeMap<String, Vec<String>>, journal_ip: &str) -> String {
    let mut pass = String::new();
    for (host, addrs) in addresses {
        for addr in addrs {
            pass.push_str(&format!("try \"$1{host}@{addr}\" {addr} 443\n"));
        }
    }
    pass.push_str(&format!("try \"$1journal\" {journal_ip} 5432\n"));
    format!(
        "try() {{ if timeout 5 bash -c \"exec 3<>/dev/tcp/$2/$3\" 2>/dev/null; then echo \"reached $1\"; else echo \"refused $1\"; fi; }}\n\
         pass() {{\n{pass}}}\n\
         pass early:\nsleep 10\npass ''\n"
    )
}

/// OPS-5 at the network layer: no process type reaches a live trading host, and the journal is
/// reachable exactly from the rendered processes whose egress names Postgres.
fn egress(seeded: bool) -> Result<Verdict> {
    if seeded {
        for policy in release_objects()?
            .iter()
            .filter(|o| o["kind"] == "NetworkPolicy")
            .filter_map(|o| o["metadata"]["name"].as_str())
        {
            kubectl(&["delete", "networkpolicy", policy, "--namespace", NAMESPACE])?;
        }
    }
    let addresses = live_addresses()?;
    let journal_ip = kubectl(&[
        "get",
        "pod",
        "--namespace",
        NAMESPACE,
        "--selector",
        "cnpg.io/cluster=journal,cnpg.io/instanceRole=primary",
        "-o",
        "jsonpath={.items[0].status.podIP}",
    ])?;
    let journal_ip = journal_ip.trim();
    if journal_ip.parse::<Ipv4Addr>().is_err() {
        bail!("the journal's primary has no pod address (`{journal_ip}`)");
    }
    let script = probe_script(&addresses, journal_ip);
    let rendered = render(&[], None)?;
    let image = rendered
        .iter()
        .find(|o| o["kind"] == "Job" && o["metadata"]["name"] == "migrate")
        .and_then(|j| j["spec"]["template"]["spec"]["containers"][0]["image"].as_str())
        .context("the migration Job did not render, so there is no probe image")?
        .to_string();
    let policies: Vec<String> = rendered
        .iter()
        .filter(|o| o["kind"] == "NetworkPolicy")
        .filter_map(|o| o["metadata"]["name"].as_str().map(String::from))
        .collect();
    let gate = rendered
        .iter()
        .find(|o| o["kind"] == "Job" && o["metadata"]["name"] == "migrate")
        .map(|j| j["spec"]["template"]["spec"]["initContainers"][0].clone())
        .filter(|c| c["name"] == "egress-gate")
        .context("the migration Job renders no egress gate")?;
    let id = run_id()?;
    let all = processes()?;
    let pods = all
        .iter()
        .map(|p| {
            let mut pod = probe_pod(p, &id, &image, &script)?;
            if !seeded {
                pod["spec"]["initContainers"] = json!([gate.clone()]);
            }
            Ok(pod)
        })
        .collect::<Result<Vec<_>>>()?;
    apply(&pods)?;
    let mut problems = Vec::new();
    for (process, pod) in all.iter().zip(&pods) {
        let name = process["name"].as_str().unwrap_or_default();
        let pod_name = pod["metadata"]["name"].as_str().unwrap_or_default();
        let logs = pod_logs(pod_name)?;
        for line in logs.lines().filter(|l| l.starts_with("reached ")) {
            let target = line.trim_start_matches("reached ");
            if target.trim_start_matches("early:") != "journal" {
                problems.push(format!("{name} reached the live trading host {target}"));
            }
        }
        let names_postgres = process["egress"]
            .as_array()
            .is_some_and(|e| e.iter().any(|t| t == "postgres"));
        let has_policy = policies.iter().any(|p| *p == format!("egress-{name}"));
        let expect_journal = names_postgres && has_policy;
        let reached_journal = logs.lines().any(|l| l == "reached journal");
        if !logs.lines().any(|l| l.ends_with(" journal")) {
            problems.push(format!("{name}'s probe did not finish:\n{logs}"));
        } else if reached_journal != expect_journal {
            problems.push(format!(
                "{name} {} the journal, and its rendered egress {} Postgres",
                if reached_journal {
                    "reached"
                } else {
                    "did not reach"
                },
                if expect_journal {
                    "names"
                } else {
                    "does not name"
                }
            ));
        }
    }
    let _ = kubectl(&[
        "delete",
        "pod",
        "--namespace",
        NAMESPACE,
        "--selector",
        "mandate.dev/workspace=ws1",
        "--field-selector",
        "status.phase!=Running",
        "--wait=false",
    ]);
    if problems.is_empty() {
        Ok(Verdict::Held)
    } else {
        if !seeded {
            diagnose_policies();
        }
        Ok(Verdict::Broken(problems.join("\n")))
    }
}

/// What the node enforces, printed when the egress drill fails unseeded: the namespace's policies,
/// the network-policy controller's log lines, and its packet-filter chains.
fn diagnose_policies() {
    let shown = [
        vec![
            "exec",
            NODE,
            "kubectl",
            "get",
            "networkpolicy",
            "--all-namespaces",
        ],
        vec![
            "exec",
            NODE,
            "sh",
            "-c",
            "iptables-save 2>&1 | grep -ciE 'KUBE-NWPLCY|KUBE-POD-FW'; iptables-save 2>&1 | grep -iE 'KUBE-NWPLCY|KUBE-POD-FW|KUBE-ROUTER' | head -40; nft list ruleset 2>&1 | grep -ciE 'nwplcy|pod-fw'",
        ],
        vec!["logs", "--tail", "4000", NODE],
    ];
    for args in shown {
        match Command::new("docker").args(&args).output() {
            Ok(out) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                let lines: Vec<&str> = if args.first() == Some(&"logs") {
                    text.lines()
                        .filter(|l| {
                            let l = l.to_lowercase();
                            l.contains("network polic")
                                || l.contains("netpol")
                                || l.contains("kube-router")
                        })
                        .take(40)
                        .collect()
                } else {
                    text.lines().collect()
                };
                eprintln!(
                    "    diagnose `docker {}`:\n{}",
                    args.join(" "),
                    lines.join("\n")
                );
            }
            Err(e) => eprintln!("    diagnose: {e}"),
        }
    }
}

/// Waits for a probe pod to finish, then returns its logs.
fn pod_logs(name: &str) -> Result<String> {
    for _ in 0..POLLS {
        let phase = kubectl(&[
            "get",
            "pod",
            name,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.phase}",
        ])?;
        match phase.trim() {
            "Succeeded" | "Failed" => {
                return kubectl(&["logs", name, "--namespace", NAMESPACE]);
            }
            _ => sleep(Duration::from_secs(2)),
        }
    }
    bail!("probe pod {name} did not finish in time")
}

/// The library PostgreSQL 18 image the Compose file runs the journal on, pinned by digest (D-5);
/// in a cluster the journal runs under CloudNativePG instead.
const COMPOSE_POSTGRES_IMAGE: &str =
    "postgres:18.6-trixie@sha256:74935e72241653ca55e0414067e6d8763aceb8a810eb51b452253ec3dcfc4336";
/// The one non-root user every Compose service runs as: the library Postgres image's `postgres`
/// user, so its first-start script and each process read the same 0600 pgpass files, which libpq
/// and sqlx refuse when any group can read them. In a cluster each process runs as 65532 and
/// copies its file instead (DEC-993 item 7).
const COMPOSE_USER: &str = "999:999";
pub const ROLES_SQL: &str = "deploy/cell/charts/journal-db/files/roles.sql";
pub const COMPOSE_INITDB: &str = "deploy/cell/compose/initdb.sh";
const GENERATED: &str = "Generated by `cargo xtask cell generate --write` from deploy/cell/charts/workspace/processes.json; edit that file, never this one";

/// Every process type's database role but the migration role, in list order, each once.
fn login_roles(all: &[Value]) -> Vec<String> {
    let mut roles: Vec<String> = Vec::new();
    for role in all.iter().filter_map(|p| p["db_role"].as_str()) {
        if role != "mandate_migrator" && !roles.iter().any(|r| r == role) {
            roles.push(role.to_string());
        }
    }
    roles
}

/// The journal's roles (journal spec §6.1, infrastructure §3.5), run once at bootstrap as the
/// superuser in the journal database: CloudNativePG runs it from the journal-db chart, and the
/// Compose file's Postgres runs the same file. `mandate_journal_owner` owns the tables and only
/// the migration role may become it; each process type logs in as its own role, a member of
/// `mandate_journal_app`. Passwords are set elsewhere, from mounted files.
pub fn roles_sql(all: &[Value]) -> String {
    let mut sql = format!(
        "-- {GENERATED}.\n\
         DO $$\nBEGIN\n  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_owner') THEN\n    CREATE ROLE mandate_journal_owner NOLOGIN;\n  END IF;\n  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'mandate_journal_app') THEN\n    CREATE ROLE mandate_journal_app NOLOGIN;\n  END IF;\nEND\n$$;\n\
         GRANT mandate_journal_owner TO mandate_migrator WITH INHERIT FALSE;\n\
         REVOKE ALL ON DATABASE mandate FROM PUBLIC;\n\
         REVOKE CONNECT ON DATABASE postgres FROM PUBLIC;\n\
         REVOKE CONNECT ON DATABASE template1 FROM PUBLIC;\n\
         REVOKE ALL ON SCHEMA public FROM PUBLIC;\n\
         CREATE SCHEMA IF NOT EXISTS journal AUTHORIZATION mandate_journal_owner;\n\
         GRANT USAGE ON SCHEMA journal TO mandate_journal_app;\n\
         CREATE SCHEMA IF NOT EXISTS deploy AUTHORIZATION mandate_migrator;\n"
    );
    for role in login_roles(all) {
        sql.push_str(&format!(
            "DO $$\nBEGIN\n  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{role}') THEN\n    CREATE ROLE {role} LOGIN IN ROLE mandate_journal_app;\n  END IF;\nEND\n$$;\n\
             GRANT CONNECT ON DATABASE mandate TO {role};\n\
             ALTER ROLE {role} IN DATABASE mandate SET search_path = journal;\n"
        ));
    }
    sql
}

fn secret_name(role: &str) -> String {
    format!("journal-role-{}", role.replace('_', "-"))
}

/// Kubernetes CPU and memory quantities as Compose writes them: `250m` as `0.25`, `256Mi` as
/// `256M`.
fn compose_cpus(cpu: &str) -> Result<String> {
    let millis: u32 = cpu
        .strip_suffix('m')
        .with_context(|| format!("cpu `{cpu}` is not in millicores"))?
        .parse()
        .with_context(|| format!("cpu `{cpu}` is not a number"))?;
    let whole = millis / 1000;
    let fraction = format!("{:03}", millis % 1000);
    let fraction = fraction.trim_end_matches('0');
    Ok(if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    })
}

fn compose_memory(memory: &str) -> Result<String> {
    let mebi = memory
        .strip_suffix("Mi")
        .with_context(|| format!("memory `{memory}` is not in Mi"))?;
    Ok(format!("{mebi}M"))
}

/// The init script the Compose file's Postgres runs once, as its superuser: the migration role and
/// the journal database, the roles file, then each login role's password from its mounted pgpass
/// file. No password is an environment variable or an argument (DEC-822 item 4).
fn compose_initdb(all: &[Value]) -> String {
    let mut script = format!(
        "#!/usr/bin/env bash\n\
         # {GENERATED}.\n\
         # The Compose file's Postgres runs this once, at its first start, as its superuser.\n\
         set -euo pipefail\n\
         set_password() {{\n  psql -v ON_ERROR_STOP=1 -U postgres -d mandate -v role=\"$1\" -v file=\"/run/secrets/$2\" <<'SQL'\n\\set pw `cut -d: -f5 :'file'`\nALTER ROLE :\"role\" PASSWORD :'pw';\nSQL\n}}\n\
         psql -v ON_ERROR_STOP=1 -U postgres -d postgres <<'SQL'\nCREATE ROLE mandate_migrator LOGIN;\nCREATE DATABASE mandate OWNER mandate_migrator;\nSQL\n\
         psql -v ON_ERROR_STOP=1 -U postgres -d mandate -f /etc/mandate/roles.sql\n\
         set_password mandate_migrator journal-role-mandate-migrator\n",
    );
    for role in login_roles(all) {
        script.push_str(&format!("set_password {role} {}\n", secret_name(&role)));
    }
    script
}

/// The Compose file for one host (E21-36, infrastructure §3.2 hybrid): the same processes as the
/// chart, from the same list, each with the chart's security settings. Compose cannot filter by
/// destination, so a process with no external egress target is on the internal network alone,
/// which has no route out; one with an external target also joins `egress`, and the host's
/// firewall narrows it (deploy/cell/README.md).
pub fn compose_document(root: &Path, all: &[Value]) -> Result<Value> {
    let mut services = serde_json::Map::new();
    let mut secrets = serde_json::Map::new();
    let mut role_secrets = vec![secret_name("mandate_migrator")];
    role_secrets.extend(login_roles(all).iter().map(|r| secret_name(r)));
    for secret in &role_secrets {
        secrets.insert(
            secret.clone(),
            json!({"file": format!("./secrets/{secret}.pgpass")}),
        );
    }
    secrets.insert(
        "postgres-superuser".into(),
        json!({"file": "./secrets/postgres-superuser"}),
    );
    if all
        .iter()
        .any(|p| p["name"] == API_PROCESS && p["built"] == true)
    {
        secrets.insert(
            API_SESSION_KEY.into(),
            json!({"file": format!("./secrets/{API_SESSION_KEY}")}),
        );
    }
    let mut db_secrets = vec![json!("postgres-superuser")];
    db_secrets.extend(role_secrets.iter().map(|s| json!(s)));
    services.insert(
        "postgres".into(),
        json!({
            "image": COMPOSE_POSTGRES_IMAGE,
            "environment": {
                "POSTGRES_USER": "postgres",
                "POSTGRES_DB": "postgres",
                "POSTGRES_PASSWORD_FILE": "/run/secrets/postgres-superuser"
            },
            "secrets": db_secrets,
            "configs": [
                {"source": "journal-initdb", "target": "/docker-entrypoint-initdb.d/10-journal.sh"},
                {"source": "journal-roles", "target": "/etc/mandate/roles.sql"}
            ],
            "volumes": ["journal-data:/var/lib/postgresql"],
            "read_only": true,
            "tmpfs": ["/tmp", "/var/run/postgresql"],
            "security_opt": ["no-new-privileges:true"],
            "cap_drop": ["ALL"],
            "cap_add": ["CHOWN", "DAC_OVERRIDE", "FOWNER", "SETGID", "SETUID"],
            "networks": ["journal"],
            "healthcheck": {
                "test": ["CMD", "pg_isready", "-U", "postgres", "-d", "mandate"],
                "interval": "2s",
                "retries": 30
            },
            "restart": "unless-stopped"
        }),
    );
    let mut needs_egress = false;
    for p in all.iter().filter(|p| p["built"] == true) {
        let name = p["name"].as_str().context("a process has no name")?;
        let kind = p["kind"].as_str().context("a process has no kind")?;
        let image_key = p["image"].as_str().context("a process has no image")?;
        let image = if image_key == "postgres" {
            values_image(root, "postgres")?
        } else {
            format!(
                "${{MANDATE_IMAGE_{}:?set the {image_key} image, pinned by digest}}",
                image_key.to_uppercase().replace('-', "_")
            )
        };
        let limits = &p["resources"]["limits"];
        let reservations = if p["guaranteed"] == true {
            limits
        } else {
            &p["resources"]["requests"]
        };
        let as_str = |v: &Value, key: &str| -> Result<String> {
            v[key]
                .as_str()
                .map(String::from)
                .with_context(|| format!("{name} has no {key} resource"))
        };
        let mut networks = vec![json!("journal")];
        let external = p["egress"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|t| !matches!(t.as_str(), Some("postgres" | "vault" | "model-gateway")));
        if external {
            networks.push(json!("egress"));
            needs_egress = true;
        }
        let mut service = json!({
            "image": image,
            "user": COMPOSE_USER,
            "read_only": true,
            "tmpfs": ["/tmp"],
            "security_opt": ["no-new-privileges:true"],
            "cap_drop": ["ALL"],
            "networks": networks,
            "environment": {"HOME": "/tmp"},
            "labels": {
                "mandate.dev/workspace": "${MANDATE_WORKSPACE:?set the workspace}",
                "mandate.dev/process": name
            },
            "deploy": {"resources": {
                "limits": {"cpus": compose_cpus(&as_str(limits, "cpu")?)?, "memory": compose_memory(&as_str(limits, "memory")?)?},
                "reservations": {"cpus": compose_cpus(&as_str(reservations, "cpu")?)?, "memory": compose_memory(&as_str(reservations, "memory")?)?}
            }}
        });
        if let Some(role) = p["db_role"].as_str() {
            let secret = secret_name(role);
            service["environment"]["MANDATE_JOURNAL_DSN"] = json!(format!(
                "postgres://{role}@postgres:5432/mandate?sslmode=disable"
            ));
            service["environment"]["PGPASSFILE"] = json!(format!("/run/secrets/{secret}"));
            service["secrets"] = json!([secret]);
        }
        match kind {
            "migration" => {
                service["command"] = json!(["bash", "/opt/mandate/migrate.sh"]);
                service["volumes"] = json!([
                    "../../../migrations:/migrations:ro",
                    "../charts/workspace/files/migrate.sh:/opt/mandate/migrate.sh:ro"
                ]);
                service["depends_on"] = json!({"postgres": {"condition": "service_healthy"}});
                service["restart"] = json!("no");
            }
            "run" | "verify" => {
                service["profiles"] = json!(["run"]);
                service["depends_on"] =
                    json!({"migrate": {"condition": "service_completed_successfully"}});
                service["restart"] = json!("no");
            }
            "service" => {
                service["tmpfs"] = json!(["/tmp", "/work"]);
                service["depends_on"] =
                    json!({"migrate": {"condition": "service_completed_successfully"}});
                service["restart"] = json!("unless-stopped");
                if name == API_PROCESS {
                    compose_api(&mut service)?;
                }
            }
            other => bail!("{name} has an unknown kind `{other}`"),
        }
        services.insert(name.to_string(), service);
    }
    let mut networks = serde_json::Map::new();
    networks.insert("journal".into(), json!({"internal": true}));
    if needs_egress {
        networks.insert("egress".into(), json!({}));
    }
    Ok(json!({
        "name": "mandate-workspace",
        "x-generated": GENERATED,
        "services": services,
        "networks": networks,
        "volumes": {"journal-data": {}},
        "secrets": secrets,
        "configs": {
            "journal-initdb": {"file": "./initdb.sh"},
            "journal-roles": {"file": "../charts/journal-db/files/roles.sql"}
        }
    }))
}

/// The Compose file's secret holding the API server's session signing key, mounted owner-only as
/// `session-signing-key` under its `CREDENTIALS_DIRECTORY` (workspace API spec §3.11 item 2).
const API_SESSION_KEY: &str = "api-session-signing-key";

/// The API server's own settings in the Compose file, as the chart's `api.yaml` gives them: both
/// listeners on loopback, each setting from the host's environment, the session key a secret file.
/// No subject links are mounted, so every sign-in is refused until the host adds them (DEC-1023).
fn compose_api(service: &mut Value) -> Result<()> {
    service["command"] = json!(["--bind", "127.0.0.1:8080", "--probe-bind", "127.0.0.1:8081"]);
    let env = service["environment"]
        .as_object_mut()
        .context("the API's Compose service has no environment")?;
    let dsn = env
        .get("MANDATE_JOURNAL_DSN")
        .cloned()
        .context("the API's Compose service has no journal DSN")?;
    env.insert("MANDATE_API_DATABASE_URL".into(), dsn);
    for name in [
        "MANDATE_API_ORG",
        "MANDATE_API_WORKSPACE",
        "MANDATE_API_ISSUER",
        "MANDATE_API_AUDIENCE",
        "MANDATE_API_JWKS_URL",
        "MANDATE_API_APP_ORIGIN",
        "MANDATE_DEPLOYMENT_TIER",
    ] {
        env.insert(name.into(), json!(format!("${{{name}:?set {name}}}")));
    }
    env.insert("CREDENTIALS_DIRECTORY".into(), json!("/run/secrets"));
    service["secrets"]
        .as_array_mut()
        .context("the API's Compose service has no secrets")?
        .push(json!({"source": API_SESSION_KEY, "target": "session-signing-key"}));
    Ok(())
}

/// An image the workspace chart's values.yaml pins, read from its `images:` block, so the Compose
/// file runs exactly the chart's default.
fn values_image(root: &Path, key: &str) -> Result<String> {
    let path = format!("{WORKSPACE_CHART}/values.yaml");
    let text = fs::read_to_string(root.join(&path)).with_context(|| format!("reading {path}"))?;
    let prefix = format!("  {key}: ");
    text.lines()
        .skip_while(|l| *l != "images:")
        .take_while(|l| l.is_empty() || l.starts_with(' ') || *l == "images:")
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
        .filter(|v| v.contains("@sha256:"))
        .with_context(|| format!("{path} pins no `images.{key}` by digest"))
}

/// `cargo xtask cell generate [--write]`: the Compose file and the roles file from the process
/// list; without `--write`, fails if either committed file differs from what it generates.
pub fn generate(write: bool) -> Result<()> {
    generate_in(Path::new("."), write)
}

fn generate_in(root: &Path, write: bool) -> Result<()> {
    let all = processes_in(root)?;
    let compose = format!(
        "{}\n",
        serde_json::to_string_pretty(&compose_document(root, &all)?)?
    );
    let roles = roles_sql(&all);
    let initdb = compose_initdb(&all);
    let mut stale = Vec::new();
    for (path, text) in [
        (COMPOSE_FILE, compose),
        (ROLES_SQL, roles),
        (COMPOSE_INITDB, initdb),
    ] {
        let committed = fs::read_to_string(root.join(path)).unwrap_or_default();
        if committed == text {
            continue;
        }
        if write {
            fs::write(root.join(path), text).with_context(|| format!("writing {path}"))?;
            if path.ends_with(".sh") {
                fs::set_permissions(root.join(path), fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("making {path} executable"))?;
            }
            eprintln!("    wrote {path}");
        } else {
            stale.push(path);
        }
    }
    if !stale.is_empty() {
        bail!(
            "{} differ from what processes.json generates; run `cargo xtask cell generate --write`",
            stale.join(" and ")
        );
    }
    Ok(())
}

/// Writes a control stream, kills the journal's primary, waits for CloudNativePG to bring the
/// cluster back, and verifies the stream. Seeded, it asks for a stream id the restart did not keep,
/// which must read as lost (`export_stream_empty`).
fn restart(seeded: bool) -> Result<Verdict> {
    let id = run_id()?;
    let workspace = format!("restart{id}");
    let args = json!([
        "workspace",
        "open",
        "--workspace",
        workspace,
        "--environment",
        "paper",
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "/work/store"
    ]);
    let open = render(
        &[format!("run.id=\"{id}\""), format!("run.args={args}")],
        Some("templates/run.yaml"),
    )?;
    let (opened, logs) = run_job(open)?;
    if !opened {
        return Ok(Verdict::Broken(format!(
            "`mandate workspace open` failed before the restart:\n{logs}"
        )));
    }
    kubectl(&[
        "delete",
        "pod",
        "--namespace",
        NAMESPACE,
        "--selector",
        "cnpg.io/cluster=journal,cnpg.io/instanceRole=primary",
        "--wait=true",
    ])?;
    let mut primary_ready = false;
    for _ in 0..POLLS {
        let ready = kubectl(&[
            "get",
            "pod",
            "--namespace",
            NAMESPACE,
            "--selector",
            "cnpg.io/cluster=journal,cnpg.io/instanceRole=primary",
            "-o",
            "jsonpath={.items[*].status.conditions[?(@.type==\"Ready\")].status}",
        ])?;
        if ready.trim() == "True" {
            primary_ready = true;
            break;
        }
        sleep(Duration::from_secs(2));
    }
    if !primary_ready {
        return Ok(Verdict::Broken(
            "the journal's primary pod was not Ready again within the drill's polls".into(),
        ));
    }
    kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        "cluster.postgresql.cnpg.io/journal",
        "--timeout=300s",
    ])?;
    let stream = if seeded {
        format!("ctl:{workspace}lost")
    } else {
        format!("ctl:{workspace}")
    };
    let verify = render(
        &[
            format!("verify.id=\"{id}\""),
            format!("verify.streams={}", json!([stream])),
        ],
        Some("templates/run.yaml"),
    )?;
    let (verified, logs) = run_job(verify)?;
    if verified {
        Ok(Verdict::Held)
    } else {
        Ok(Verdict::Broken(format!(
            "the stream written before the restart did not verify after it:\n{logs}"
        )))
    }
}

/// The kill-switch drill (OPS-4) needs the runtime and executor processes and `mandate agent
/// kill` in the binary; until both are on `main` it is named as skipped.
fn kill_switch() -> Result<Verdict> {
    let built = processes()?
        .iter()
        .filter(|p| p["built"] == true)
        .filter_map(|p| p["name"].as_str().map(String::from))
        .collect::<Vec<_>>();
    let agents = ["runtime", "executor"]
        .iter()
        .all(|n| built.iter().any(|b| b == n));
    Ok(Verdict::Skipped(if agents {
        "the runtime and executor are built, but the drill itself lands with O4 (E21-34)".into()
    } else {
        "the runtime and executor images are not on main yet (P1, #1451), nor `mandate agent \
         kill` (O4, E21-34)"
            .into()
    }))
}

const COMPOSE_DIR: &str = "deploy/cell/compose";
/// Where the compose drill keeps the secrets and the exported segment, owned by the Compose user;
/// `secrets/` is ignored by git.
const COMPOSE_SECRETS: &str = "deploy/cell/compose/secrets";

/// `docker compose` with the Compose file, the drill's workspace label and this change's image.
fn compose_cmd(args: &[&str]) -> Result<String> {
    let image = image_tag()?;
    let api = api_image_tag()?.unwrap_or_else(|| image.clone());
    let mut full = vec!["compose", "-f", "compose.json"];
    full.extend_from_slice(args);
    let out = Command::new("docker")
        .current_dir(COMPOSE_DIR)
        .args(&full)
        .env("MANDATE_WORKSPACE", WORKSPACE)
        .env("MANDATE_IMAGE_MANDATE", &image)
        .envs(compose_api_settings(&api))
        .output()
        .context("starting docker compose")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        bail!("`docker {}` failed: {text}", full.join(" "));
    }
    Ok(text)
}

/// The API server's settings the Compose file interpolates, the local cell's own (k3d.yaml's `api`
/// block). The drill starts only the migration and one-off runs, never the API, so with no API
/// image of its own (a pulled `mandate`) it names `api_image` only for the file to interpolate.
fn compose_api_settings(api_image: &str) -> [(&'static str, String); 8] {
    [
        ("MANDATE_IMAGE_API", api_image.to_string()),
        ("MANDATE_API_ORG", DEMO_ORG.to_string()),
        ("MANDATE_API_WORKSPACE", DEMO_WORKSPACE.to_string()),
        ("MANDATE_API_ISSUER", LOCAL_ISSUER.to_string()),
        ("MANDATE_API_AUDIENCE", "authenticated".to_string()),
        (
            "MANDATE_API_JWKS_URL",
            format!("{LOCAL_ISSUER}{STUB_KEY_SET}"),
        ),
        ("MANDATE_API_APP_ORIGIN", DEMO_APP_ORIGIN.to_string()),
        ("MANDATE_DEPLOYMENT_TIER", "dev".to_string()),
    ]
}

/// Runs `script` as root in the Compose file's Postgres image with `dir` at `/s`: how the drill
/// writes and removes files owned by the Compose user without root on the host.
fn as_root_in(dir: &str, script: &str) -> Result<String> {
    let abs = fs::canonicalize(dir).with_context(|| format!("resolving {dir}"))?;
    let mount = format!("{}:/s", abs.display());
    output(
        "docker",
        &[
            "run",
            "--rm",
            "--user",
            "0",
            "--network",
            "none",
            "-v",
            &mount,
            COMPOSE_POSTGRES_IMAGE,
            "bash",
            "-c",
            script,
        ],
    )
}

/// The secrets the Compose file names, each random, owned by the Compose user, mode 0600.
fn compose_secrets(all: &[Value]) -> Result<()> {
    fs::create_dir_all(COMPOSE_SECRETS)?;
    let mut script = String::from("set -euo pipefail\ncd /s\nrm -rf work\nmkdir work\n");
    script.push_str("head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \\n' > postgres-superuser\n");
    let mut roles = vec!["mandate_migrator".to_string()];
    roles.extend(login_roles(all));
    for role in roles {
        script.push_str(&format!(
            "printf '*:*:mandate:{role}:%s\\n' \"$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \\n')\" > {}.pgpass\n",
            secret_name(&role)
        ));
    }
    script.push_str(&format!("head -c 32 /dev/urandom > {API_SESSION_KEY}\n"));
    script.push_str(&format!(
        "chown -R 999:999 /s\nchmod 0600 /s/postgres-superuser /s/*.pgpass /s/{API_SESSION_KEY}\n"
    ));
    as_root_in(COMPOSE_SECRETS, &script).map(|_| ())
}

/// The compose drill: a fresh stack, the migration, a control stream opened by the CLI, and its
/// export verified; the stack, its volume and the secrets removed whatever the outcome.
fn compose_drill(seeded: bool) -> Result<Verdict> {
    let all = processes()?;
    let _ = compose_cmd(&["down", "--volumes", "--remove-orphans"]);
    compose_secrets(&all)?;
    let verdict = compose_steps(seeded);
    let _ = compose_cmd(&["down", "--volumes", "--remove-orphans"]);
    let _ = as_root_in(COMPOSE_SECRETS, "rm -rf /s/* /s/.[!.]*");
    verdict
}

fn compose_steps(seeded: bool) -> Result<Verdict> {
    compose_cmd(&["up", "--detach", "migrate"])?;
    let mut migrated = String::new();
    for _ in 0..POLLS {
        migrated = compose_cmd(&[
            "ps",
            "--all",
            "migrate",
            "--format",
            "{{.State}} {{.ExitCode}}",
        ])?;
        if migrated.trim().starts_with("exited") {
            break;
        }
        sleep(Duration::from_secs(2));
    }
    if migrated.trim() != "exited 0" {
        let logs = compose_cmd(&["logs", "migrate"]).unwrap_or_default();
        return Ok(Verdict::Broken(format!(
            "the migration service ended `{}`:\n{logs}",
            migrated.trim()
        )));
    }
    let id = run_id()?;
    let stream = format!("ctl:compose{id}");
    let work = format!(
        "{}:/work",
        fs::canonicalize(COMPOSE_SECRETS)?.join("work").display()
    );
    let dsn = |role: &str| format!("postgres://{role}@postgres:5432/mandate?sslmode=disable");
    let workspace = format!("compose{id}");
    let control = dsn("mandate_control");
    let opened = compose_cmd(&[
        "run",
        "--rm",
        "-v",
        &work,
        "control",
        "workspace",
        "open",
        "--workspace",
        &workspace,
        "--journal",
        &control,
        "--store",
        "/work/store",
    ]);
    if let Err(e) = opened {
        return Ok(Verdict::Broken(format!(
            "`mandate workspace open` failed: {e:#}"
        )));
    }
    let verify_dsn = dsn("mandate_verify");
    compose_cmd(&[
        "run",
        "--rm",
        "-v",
        &work,
        "journal-verify",
        "journal",
        "export",
        &stream,
        "/work/segment.jsonl",
        "--journal",
        &verify_dsn,
    ])?;
    if seeded {
        as_root_in(
            COMPOSE_SECRETS,
            "sed -E -i '1s/\"hash\":\"0/\"hash\":\"1/;t;1s/\"hash\":\"[1-9a-f]/\"hash\":\"0/' /s/work/segment.jsonl",
        )?;
    }
    match compose_cmd(&[
        "run",
        "--rm",
        "-v",
        &work,
        "journal-verify",
        "journal",
        "verify",
        "/work/segment.jsonl",
    ]) {
        Ok(_) => Ok(Verdict::Held),
        Err(e) => Ok(Verdict::Broken(format!(
            "the exported control stream did not verify: {e:#}"
        ))),
    }
}

/// The workspace `cargo xtask cell demo` opens, whose control stream it verifies: a ULID, since
/// the API server hosts it by that id (`api.workspace` in the local cell's values).
const DEMO_WORKSPACE: &str = "01KDEM0000000000000000000W";
/// The demo workspace's organization (`api.org` in the local cell's values).
const DEMO_ORG: &str = "01KDEM0000000000000000000R";
/// The local stub issuer's address on the API pod's loopback (DEC-2100, lane SI), exactly as its
/// tokens carry it in `iss`: no trailing slash.
const LOCAL_ISSUER: &str = "http://127.0.0.1:8090";
/// The stub's port on the API pod's loopback, which the demo forwards to the same local port, so
/// the `Host` the stub requires (`127.0.0.1:8090`, any other gets 421) is the one the demo sends.
const STUB_PORT: u16 = 8090;
/// The stub's key-set path under [`LOCAL_ISSUER`].
const STUB_KEY_SET: &str = "/jwks.json";
/// The ConfigMap holding the demo's subject links, which the chart mounts at the fixed path.
const LINKS_CONFIGMAP: &str = "subject-links";
/// The demo owner's subject at the stub issuer: a name for a demo principal, never a person's.
const DEMO_SUBJECT: &str = "demo-owner";
/// The user principal the demo owner signs in as.
const DEMO_OWNER: &str = "01KDEM0000000000000000000P";
/// The app origin the local cell's API takes (`api.appOrigin` in its values).
const DEMO_APP_ORIGIN: &str = "https://localhost:4317";
/// Where `web/` serves on the developer's machine (its `npm run dev`).
const WEB_URL: &str = "http://127.0.0.1:4317";
/// The process-list name lane A1 gives the workspace API server; `api_is_a_planned_name` checks
/// that, once listed, it is a service.
const API_PROCESS: &str = "api";

/// Which model gateway the demo's research loop calls through (DEC-1120 item 5): the recorded fake,
/// since no cell holds a vaulted OpenRouter key (E21-32) or a hosted transport yet. The selection
/// in `mandate-research-run` picks OpenRouter only when both exist and says so in its own line.
const RESEARCH_GATEWAY: &str = "research's model gateway: the recorded fake, with no network to \
     any model provider (OpenRouter only with a vaulted key and a hosted transport, DEC-1120)";

/// One piece of the full local cell (E21-37's demo): ready, or skipped because the lane that
/// builds it is not on `main` yet. A skipped piece is named, never faked.
#[derive(Debug, PartialEq, Eq)]
pub struct DemoPart {
    pub name: &'static str,
    pub skipped: Option<String>,
}

/// Which demo parts this checkout can bring up: a process is there once the process list marks
/// it built (its lane's image exists), and the founding membership once the CLI has its gesture.
pub fn demo_parts(processes: &[Value], cli_help: &str, stub_ready: bool) -> Vec<DemoPart> {
    let built = |name: &str| {
        processes
            .iter()
            .any(|p| p["name"] == name && p["built"] == true)
    };
    let lane = |ready: bool, why: &str| (!ready).then(|| why.to_string());
    vec![
        DemoPart {
            name: "the cell: k3d, CloudNativePG, the migration Job",
            skipped: None,
        },
        DemoPart {
            name: "a demo workspace whose control stream verifies (journal spec §11)",
            skipped: None,
        },
        DemoPart {
            name: API_PART,
            skipped: lane(
                built(API_PROCESS),
                "the API server is not on main yet (lane A1, E21-31)",
            ),
        },
        DemoPart {
            name: "a paper agent on rh-sim and recorded Alpaca fixtures, no credential",
            skipped: lane(
                built("runtime") && built("executor"),
                "the runtime and executor images are not on main yet (lane P1, #1451)",
            ),
        },
        DemoPart {
            name: RESEARCH_GATEWAY,
            skipped: lane(
                built("runtime"),
                "the research loop runs in the runtime image, not on main yet (lane P1, #1451)",
            ),
        },
        DemoPart {
            name: "the founding membership",
            skipped: lane(
                cli_help
                    .lines()
                    .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
                    .any(|l| l.split_whitespace().next() == Some("membership")),
                "the founding membership gesture is not on main yet (lane ID1)",
            ),
        },
        DemoPart {
            name: SIGNIN_PART,
            skipped: lane(
                built(API_PROCESS) && stub_ready,
                "the local stub issuer (lane SI, E21-52) and the server's key-set tier check \
                 (E21-53) are not both on main (DEC-2100 item 9)",
            ),
        },
        DemoPart {
            name: "web/ against the cell's API",
            skipped: lane(
                built(API_PROCESS),
                "web/ runs on its own fixtures until the API server lands (lane A1)",
            ),
        },
    ]
}

/// `cargo xtask cell demo [--no-web]`: the full local cell on a developer machine, with paper and
/// demo data only. It brings up the cell, opens the demo workspace and verifies its journal,
/// prints what is ready and what is skipped and why, then runs `web/` in the foreground at
/// [`WEB_URL`] unless `--no-web`.
pub fn demo(web: bool) -> Result<()> {
    if let Some(reason) = tools_missing() {
        bail!("the demo needs docker, k3d and helm: {reason} (deploy/cell/README.md)");
    }
    up()?;
    demo_workspace()?;
    let image = image_tag()?;
    let help = output(
        "docker",
        &["run", "--rm", "--network", "none", &image, "--help"],
    )
    .context("reading the CLI's commands from the image it built")?;
    let mut parts = demo_parts(&processes()?, &help, stub_ready(Path::new(".")));
    if api_image_tag()?.is_none() {
        skip_without_api_image(&mut parts);
    }
    let api_ready = parts
        .iter()
        .any(|p| p.name == API_PART && p.skipped.is_none());
    eprintln!("==> the local cell (paper and demo data only; local-cell results, never staging's)");
    for part in &parts {
        match &part.skipped {
            None => eprintln!("    ready    {}", part.name),
            Some(why) => eprintln!("    skipped  {}: {why}", part.name),
        }
    }
    eprintln!(
        "    kubectl: docker exec {NODE} kubectl --namespace {NAMESPACE} get pods; \
         stop: cargo xtask cell down"
    );
    let signin_ready = api_ready
        && parts
            .iter()
            .any(|p| p.name == SIGNIN_PART && p.skipped.is_none());
    if api_ready {
        api_port_forward(signin_ready)?;
    }
    if signin_ready {
        demo_sign_in()?;
    }
    if !web {
        return Ok(());
    }
    if output("npm", &["--version"]).is_err() {
        eprintln!("    web/: npm is not installed (web/.nvmrc names the Node.js release); skipped");
        return Ok(());
    }
    if !Path::new("web/node_modules").exists() {
        sh_in("web", "npm", &["ci"])?;
    }
    let wanted = fs::read_to_string("web/.nvmrc").unwrap_or_default();
    let node = output("node", &["--version"]).unwrap_or_default();
    if node.trim().trim_start_matches('v') != wanted.trim() {
        eprintln!(
            "    web/: Node.js is {}, web/.nvmrc names {}; it may still run",
            node.trim(),
            wanted.trim()
        );
    }
    eprintln!("==> web/ at {WEB_URL} (Ctrl-C stops it; the cell keeps running)");
    let status = Command::new("npm")
        .current_dir("web")
        .args(["run", "dev"])
        .status()
        .context("starting `npm run dev`")?;
    if status.success() || status.code().is_none() {
        Ok(())
    } else {
        bail!("`npm run dev` in web/ failed with {status}")
    }
}

/// Names every part that needs this checkout's API image as skipped, sign-in among them, for a
/// cell that pulls its `mandate` image and so has no API image.
pub fn skip_without_api_image(parts: &mut [DemoPart]) {
    for part in parts.iter_mut().filter(|p| p.skipped.is_none()) {
        if part.name.contains("API") || part.name == SIGNIN_PART {
            part.skipped = Some(format!(
                "{IMAGE_ENV} names a pulled `mandate` image, and no API image is published yet"
            ));
        }
    }
}

/// The local port the demo forwards to the API server's listener on its pod's loopback.
const API_LOCAL_PORT: u16 = 8787;

/// The demo part that is the API server; `api_port_forward` runs only once it is ready.
const API_PART: &str = "the workspace API server, through a port-forward";

/// The demo part that is sign-in; `demo_sign_in` runs only once it is ready.
const SIGNIN_PART: &str =
    "sign-in as the demo owner, through the local stub issuer (dev cell only, DEC-2100)";

/// Forwards [`API_LOCAL_PORT`] on this machine to the API server's listener, with the host's own
/// `kubectl` (the cell's nodes publish no port), and checks that `/v1` answers: with no session,
/// the one 401 (API-1). The forward keeps running after the demo returns, until its pod goes or it
/// is stopped; a later demo whose port still answers that way reuses it rather than starting one.
fn api_port_forward(stub: bool) -> Result<()> {
    let config = kubeconfig()?;
    let forward = port_forward_args(&config, stub);
    let shown = forward.join(" ");
    let path = format!("/v1/workspaces/{DEMO_WORKSPACE}/agents");
    if http_status(API_LOCAL_PORT, &path).is_ok_and(|status| status == 401) {
        if !stub || stub_answers() {
            eprintln!("    api: http://127.0.0.1:{API_LOCAL_PORT}/v1 answers (an earlier forward)");
            return Ok(());
        }
        bail!(
            "an earlier forward holds 127.0.0.1:{API_LOCAL_PORT} without the stub issuer's port \
             {STUB_PORT}; stop it with `pkill -f 'port-forward deployment/api'` and run the demo again"
        );
    }
    if output("kubectl", &["version", "--client"]).is_err() {
        eprintln!("    api: no kubectl on this machine; forward it with `kubectl {shown}`");
        return Ok(());
    }
    let log_path = state()?.join("api-port-forward.log");
    let log =
        fs::File::create(&log_path).with_context(|| format!("creating {}", log_path.display()))?;
    let mut child = Command::new("kubectl")
        .args(&forward)
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .context("starting `kubectl port-forward` to the API server")?;
    let said = || fs::read_to_string(&log_path).unwrap_or_default();
    for _ in 0..30 {
        if let Some(status) = child.try_wait()? {
            bail!("`kubectl {shown}` exited with {status}:\n{}", said());
        }
        if let Ok(status) = http_status(API_LOCAL_PORT, &path) {
            if status == 401 && (!stub || stub_answers()) {
                eprintln!(
                    "    api: http://127.0.0.1:{API_LOCAL_PORT}/v1 answers (GET {path} is 401 \
                     with no session, API-1)"
                );
                return Ok(());
            }
            if status != 401 {
                let _ = child.kill();
                bail!("the API server answered GET {path} with {status}, not the one 401");
            }
        }
        sleep(Duration::from_secs(1));
    }
    let _ = child.kill();
    bail!(
        "the API server did not answer on 127.0.0.1:{API_LOCAL_PORT} within 30 s through \
         `kubectl {shown}`, which said:\n{}",
        said()
    )
}

/// `kubectl` arguments forwarding [`API_LOCAL_PORT`] on this machine's loopback to the API
/// listener's port 8080, and with `stub` [`STUB_PORT`] to the stub issuer's: the address is its
/// own flag, since a port spec is `LOCAL:REMOTE` only.
pub fn port_forward_args(kubeconfig: &str, stub: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--kubeconfig",
        kubeconfig,
        "--namespace",
        NAMESPACE,
        "port-forward",
        "--address",
        "127.0.0.1",
        "deployment/api",
        &format!("{API_LOCAL_PORT}:8080"),
    ]
    .map(String::from)
    .to_vec();
    if stub {
        args.push(format!("{STUB_PORT}:{STUB_PORT}"));
    }
    args
}

/// Whether the stub issuer answers through the forward: its key set, with the `Host` it requires.
fn stub_answers() -> bool {
    http(STUB_PORT, "GET", STUB_KEY_SET, &[], "").is_ok_and(|(status, _)| status == 200)
}

/// Signs in as the demo owner the way `web/` does: a token for [`DEMO_SUBJECT`] from the stub
/// issuer, then `POST /v1/sessions` from the app origin, which must answer 204 with the session
/// cookie (workspace API spec §3.11 item 5).
fn demo_sign_in() -> Result<()> {
    let body = json!({"subject": DEMO_SUBJECT}).to_string();
    let (status, reply) = http(
        STUB_PORT,
        "POST",
        "/token",
        &[("Content-Type", "application/json")],
        &body,
    )
    .context("asking the stub issuer for a token")?;
    if status != 200 {
        bail!("the stub issuer answered POST /token with {status}");
    }
    let token: Value = serde_json::from_str(reply_body(&reply))
        .context("the stub issuer's token reply is not JSON")?;
    let token = token["access_token"]
        .as_str()
        .context("the stub issuer's reply carries no access_token")?;
    let body = json!({"access_token": token}).to_string();
    let (status, reply) = http(
        API_LOCAL_PORT,
        "POST",
        "/v1/sessions",
        &[
            ("Content-Type", "application/json"),
            ("Origin", DEMO_APP_ORIGIN),
            ("X-Mandate-Request", "1"),
        ],
        &body,
    )
    .context("opening a session")?;
    if !signed_in(status, &reply) {
        bail!("POST /v1/sessions answered {status}, not 204 with the session cookie");
    }
    eprintln!("    sign-in: the demo owner opened a session through the local stub issuer");
    Ok(())
}

/// Whether an answer to `POST /v1/sessions` opened a session: 204 and the `__Host-` cookie.
pub fn signed_in(status: u16, reply: &str) -> bool {
    status == 204
        && reply
            .split("\r\n\r\n")
            .next()
            .unwrap_or_default()
            .lines()
            .any(|l| {
                l.to_ascii_lowercase()
                    .starts_with("set-cookie: __host-mandate_session=")
            })
}

/// The body of an HTTP/1.1 reply: what follows its header block.
fn reply_body(reply: &str) -> &str {
    reply.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

/// One HTTP/1.1 request to 127.0.0.1:`port`, with `Host: 127.0.0.1:port` (the stub issuer takes
/// no other), and its status code and whole reply.
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
         Content-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    let status = status_of(&reply).context("the reply is not HTTP/1.1")?;
    Ok((status, reply))
}

/// Writes the demo owner's subject link with the CLI's own `mandate identity link`, in the image
/// this checkout built and with no network, then applies it as the [`LINKS_CONFIGMAP`] ConfigMap
/// the chart mounts at the server's fixed path (DEC-1023). The subject reaches the CLI on standard
/// input only, as it requires.
fn demo_links(image: &str) -> Result<()> {
    let dir = state()?.join("links");
    if dir.exists() {
        fs::remove_dir_all(&dir).context("clearing the demo's links directory")?;
    }
    fs::create_dir_all(&dir)?;
    let owner = fs::metadata(&dir)?;
    let user = format!("{}:{}", owner.uid(), owner.gid());
    let mount = format!("{}:/out", fs::canonicalize(&dir)?.display());
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--network",
            "none",
            "--user",
            &user,
            "-v",
            &mount,
            image,
            "identity",
            "link",
            "--issuer",
            LOCAL_ISSUER,
            "--member",
            DEMO_OWNER,
            "--links",
            "/out/subject-links",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("starting `mandate identity link` in the cell's image")?;
    child
        .stdin
        .take()
        .context("the CLI's standard input")?
        .write_all(DEMO_SUBJECT.as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        bail!(
            "`mandate identity link` for the demo owner failed with {status}; it runs as this \
             user ({user}) writing into {}, which rootless docker maps to another owner",
            dir.display()
        );
    }
    let links = fs::read_to_string(dir.join("subject-links"))
        .context("reading the demo's subject-link file")?;
    apply(&[links_configmap(&links)])
}

/// The ConfigMap holding the subject-link file `links` under the key the chart mounts.
pub fn links_configmap(links: &str) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {"name": LINKS_CONFIGMAP, "labels": {"mandate.dev/workspace": WORKSPACE}},
        "data": {"subject-links": links}
    })
}

/// The status code of an HTTP/1.1 `GET path` to 127.0.0.1:`port`.
fn http_status(port: u16, path: &str) -> Result<u16> {
    http(port, "GET", path, &[], "").map(|(status, _)| status)
}

/// The status code on an HTTP/1.1 reply's first line.
pub fn status_of(reply: &str) -> Option<u16> {
    reply
        .lines()
        .next()?
        .strip_prefix("HTTP/1.1 ")?
        .get(..3)?
        .parse()
        .ok()
}

/// Opens the demo workspace's control stream once (a second run finds it open and goes on), then
/// exports and verifies it in the cell.
fn demo_workspace() -> Result<()> {
    let id = run_id()?;
    let args = json!([
        "workspace",
        "open",
        "--workspace",
        DEMO_WORKSPACE,
        "--environment",
        "paper",
        "--journal",
        "$(MANDATE_JOURNAL_DSN)",
        "--store",
        "/work/store"
    ]);
    let open = render(
        &[format!("run.id=\"{id}\""), format!("run.args={args}")],
        Some("templates/run.yaml"),
    )?;
    let (opened, logs) = run_job(open)?;
    if !opened && !logs.contains("workspace_already_open") {
        bail!("`mandate workspace open --workspace {DEMO_WORKSPACE}` failed in the cell:\n{logs}");
    }
    let streams = json!([format!("ctl:{DEMO_WORKSPACE}")]);
    let verify = render(
        &[
            format!("verify.id=\"{id}\""),
            format!("verify.streams={streams}"),
        ],
        Some("templates/run.yaml"),
    )?;
    let (verified, logs) = run_job(verify)?;
    if !verified {
        bail!("the demo workspace's control stream did not verify:\n{logs}");
    }
    Ok(())
}

fn sh_in(dir: &str, program: &str, args: &[&str]) -> Result<()> {
    eprintln!("    $ (cd {dir} && {program} {})", args.join(" "));
    let status = Command::new(program)
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("starting `{program}`"))?;
    if !status.success() {
        bail!(
            "`{program} {}` in {dir} failed with {status}",
            args.join(" ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One binary's tag is its own digest, as before; two binaries' changes when either changes,
    /// and with their order, so the runner alone changing gives the `mandate` image a new tag.
    #[test]
    fn an_image_of_two_binaries_is_tagged_by_both() {
        let [a, b, c] = ["a", "b", "c"].map(|d| d.repeat(64));
        assert_eq!(combined(std::slice::from_ref(&a)), a);
        let both = combined(&[a.clone(), b.clone()]);
        assert_eq!(both.len(), 64);
        assert!(both.bytes().all(|x| x.is_ascii_hexdigit()));
        assert_ne!(both, a);
        assert_ne!(both, combined(&[a.clone(), c]));
        assert_ne!(both, combined(&[b, a]));
    }

    fn process(name: &str, built: bool) -> Value {
        json!({"name": name, "built": built})
    }

    #[test]
    fn a_demo_part_is_ready_only_once_its_lane_is_on_main() {
        let today = [
            process("control", true),
            process("runtime", false),
            process("executor", false),
        ];
        let parts = demo_parts(
            &today,
            "Commands:\n  workspace  Open the workspace control stream\n",
            false,
        );
        let skipped: Vec<&str> = parts
            .iter()
            .filter(|p| p.skipped.is_some())
            .map(|p| p.name)
            .collect();
        assert_eq!(
            skipped.len(),
            6,
            "API, agent, research, membership, sign-in and web/ wait on their lanes: {skipped:?}"
        );
        assert!(
            skipped.contains(&RESEARCH_GATEWAY),
            "the research gateway's line waits on the runtime image"
        );
        assert!(
            parts.iter().take(2).all(|p| p.skipped.is_none()),
            "the cell and the journal are always there"
        );

        let later = [
            process("api", true),
            process("runtime", true),
            process("executor", true),
        ];
        let parts = demo_parts(
            &later,
            "Commands:\n  membership  Grant the founding membership\n",
            true,
        );
        assert!(parts.iter().all(|p| p.skipped.is_none()), "{parts:?}");
        let no_stub = demo_parts(
            &later,
            "Commands:\n  membership  Grant the founding membership\n",
            false,
        );
        let waiting: Vec<&str> = no_stub
            .iter()
            .filter(|p| p.skipped.is_some())
            .map(|p| p.name)
            .collect();
        assert_eq!(waiting, [SIGNIN_PART], "only sign-in waits on the stub");
    }

    #[test]
    fn api_is_a_built_service() -> Result<()> {
        let all = processes_in(&crate::repo_root()?)?;
        let listed = all.iter().find(|p| p["name"] == API_PROCESS);
        assert!(
            listed.is_some_and(|p| p["kind"] == "service" && p["built"] == true),
            "the API server is a built service named `{API_PROCESS}`"
        );
        let help = "Commands:\n  workspace  Open\n      membership is named in a wrapped line\n";
        let membership = demo_parts(&[], help, false)
            .into_iter()
            .find(|p| p.name == "the founding membership");
        assert!(
            membership.is_some_and(|m| m.skipped.is_some()),
            "a wrapped description line is not a command"
        );
        Ok(())
    }

    #[test]
    fn a_host_that_cannot_build_pulls_a_pinned_image_or_stops() {
        let digest = format!("ghcr.io/x/mandate@sha256:{}", "a".repeat(64));
        assert_eq!(
            image_source("linux", "x86_64", None),
            Ok(ImageSource::Build)
        );
        assert_eq!(
            image_source("macos", "aarch64", Some(&digest)),
            Ok(ImageSource::Pull(digest.clone()))
        );
        assert_eq!(
            image_source("linux", "x86_64", Some(&digest)),
            Ok(ImageSource::Pull(digest.clone())),
            "a pinned image wins on any host"
        );
        let stop = image_source("macos", "aarch64", None)
            .err()
            .unwrap_or_default();
        assert!(
            stop.contains("macos/aarch64") && stop.contains("E21-35"),
            "{stop}"
        );
        assert!(image_source("linux", "aarch64", None).is_err());
        for bad in [
            "ghcr.io/x/mandate:latest",
            "ghcr.io/x/mandate@sha256:abc",
            &format!("ghcr.io/x/mandate@sha256:{}", "A".repeat(64)),
            &format!("@sha256:{}", "a".repeat(64)),
            &format!("localhost:5000/x@sha256:{}", "a".repeat(64)),
            &format!("ghcr.io/X/mandate@sha256:{}", "a".repeat(64)),
        ] {
            assert!(
                image_source("macos", "aarch64", Some(bad)).is_err(),
                "{bad}"
            );
        }
        assert_eq!(
            image_source("linux", "x86_64", Some("  ")),
            Ok(ImageSource::Build)
        );
        let tagged = format!("ghcr.io/x/mandate:v1@sha256:{}", "b".repeat(64));
        assert_eq!(
            image_source("macos", "aarch64", Some(&tagged)),
            Ok(ImageSource::Pull(tagged.clone()))
        );
    }

    #[test]
    fn an_agent_needs_both_its_runtime_and_its_executor() {
        for (runtime, executor) in [(true, false), (false, true)] {
            let list = [process("runtime", runtime), process("executor", executor)];
            let agent = demo_parts(&list, "", false)
                .into_iter()
                .find(|p| p.name.starts_with("a paper agent"));
            assert!(agent.is_some_and(|a| a.skipped.is_some()));
        }
    }

    #[test]
    fn a_cidr_contains_exactly_its_addresses() {
        assert!(cidr_contains("13.35.78.0/24", "13.35.78.121"));
        assert!(!cidr_contains("13.35.79.0/24", "13.35.78.121"));
        assert!(cidr_contains("13.35.78.121/32", "13.35.78.121"));
        assert!(!cidr_contains("13.35.78.120/32", "13.35.78.121"));
        assert!(cidr_contains("0.0.0.0/0", "35.221.23.121"));
        assert!(cidr_contains("not-a-cidr", "35.221.23.121"));
        assert!(cidr_contains("10.0.0.0/33", "35.221.23.121"));
    }

    #[test]
    fn a_policy_allowing_a_live_address_is_refused() -> Result<()> {
        let live = BTreeMap::from([("api.example".to_string(), vec!["192.0.2.7".to_string()])]);
        let policy = |cidr: &str| {
            json!({"kind": "NetworkPolicy", "metadata": {"name": "egress-executor"},
                   "spec": {"egress": [{"to": [{"ipBlock": {"cidr": cidr}}]}]}})
        };
        assert!(no_live_host_allowed("v", &[policy("192.0.2.0/24")], &live).is_err());
        no_live_host_allowed("v", &[policy("198.51.100.0/24")], &live)?;
        Ok(())
    }

    #[test]
    fn compose_quantities_read_as_kubernetes_ones() -> Result<()> {
        assert_eq!(compose_cpus("250m")?, "0.25");
        assert_eq!(compose_cpus("1000m")?, "1");
        assert_eq!(compose_cpus("1500m")?, "1.5");
        assert_eq!(compose_memory("256Mi")?, "256M");
        assert!(compose_cpus("1").is_err());
        Ok(())
    }

    #[test]
    fn the_demo_is_the_workspace_the_local_api_hosts() -> Result<()> {
        let values = fs::read_to_string(crate::repo_root()?.join(WORKSPACE_VALUES))?;
        let api: Vec<&str> = values
            .lines()
            .skip_while(|l| *l != "api:")
            .skip(1)
            .take_while(|l| l.starts_with("  "))
            .map(str::trim)
            .collect();
        for wanted in [
            format!("org: {DEMO_ORG}"),
            format!("workspace: {DEMO_WORKSPACE}"),
            format!("issuer: {LOCAL_ISSUER}"),
            format!("jwksUrl: {LOCAL_ISSUER}{STUB_KEY_SET}"),
            "tier: dev".to_string(),
        ] {
            assert!(
                api.contains(&wanted.as_str()),
                "k3d.yaml's api block lacks `{wanted}`"
            );
        }
        for ulid in [DEMO_ORG, DEMO_WORKSPACE] {
            assert!(
                ulid.len() == 26
                    && ulid.starts_with(|c: char| ('0'..='7').contains(&c))
                    && ulid
                        .bytes()
                        .all(|b| b.is_ascii_digit()
                            || (b.is_ascii_uppercase() && !b"ILOU".contains(&b))),
                "{ulid} is not a ULID, which the API server requires"
            );
        }
        Ok(())
    }

    #[test]
    fn every_compose_placeholder_is_one_the_drill_sets() -> Result<()> {
        let root = crate::repo_root()?;
        let text = serde_json::to_string(&compose_document(&root, &processes_in(&root)?)?)?;
        let set: Vec<&str> = compose_api_settings("x")
            .iter()
            .map(|(name, _)| *name)
            .chain(["MANDATE_WORKSPACE", "MANDATE_IMAGE_MANDATE"])
            .collect();
        let named: Vec<&str> = text
            .split("${")
            .skip(1)
            .filter_map(|rest| rest.split(":?").next())
            .collect();
        assert!(!named.is_empty());
        for name in &named {
            assert!(
                set.contains(name),
                "the Compose file needs {name}, which no drill sets"
            );
        }
        Ok(())
    }

    #[test]
    fn the_composed_api_listens_on_loopback_with_its_key_as_a_file() -> Result<()> {
        let root = crate::repo_root()?;
        let doc = compose_document(&root, &processes_in(&root)?)?;
        let api = &doc["services"][API_PROCESS];
        assert_eq!(
            api["command"],
            json!(["--bind", "127.0.0.1:8080", "--probe-bind", "127.0.0.1:8081"])
        );
        assert_eq!(api["environment"]["CREDENTIALS_DIRECTORY"], "/run/secrets");
        assert_eq!(
            api["environment"]["MANDATE_API_DATABASE_URL"],
            api["environment"]["MANDATE_JOURNAL_DSN"]
        );
        assert!(
            api["secrets"].as_array().is_some_and(|s| s
                .contains(&json!({"source": API_SESSION_KEY, "target": "session-signing-key"}))),
            "the session key is the file the server reads"
        );
        assert!(doc["secrets"][API_SESSION_KEY]["file"].is_string());
        Ok(())
    }

    #[test]
    fn the_api_image_takes_the_named_bundle_else_the_first_on_the_host() -> Result<()> {
        let fedora = |path: &str| path == "/etc/pki/tls/certs/ca-bundle.crt";
        assert_eq!(
            ca_bundle_from(None, fedora)?,
            "/etc/pki/tls/certs/ca-bundle.crt"
        );
        let both = |path: &str| path.starts_with("/etc/");
        assert_eq!(ca_bundle_from(None, both)?, HOST_CA_BUNDLES[0]);
        assert_eq!(
            ca_bundle_from(Some("/opt/ca.pem"), |_| true)?,
            "/opt/ca.pem"
        );
        assert_eq!(
            ca_bundle_from(Some("  "), fedora)?,
            "/etc/pki/tls/certs/ca-bundle.crt"
        );
        let missing = ca_bundle_from(Some("/opt/ca.pem"), both).map_err(|e| e.to_string());
        assert!(missing.is_err_and(|e| e.contains("SSL_CERT_FILE=/opt/ca.pem")));
        let none = ca_bundle_from(None, |_| false).map_err(|e| e.to_string());
        assert!(none.is_err_and(|e| e.contains("set SSL_CERT_FILE")));
        Ok(())
    }

    #[test]
    fn the_api_forward_binds_loopback_and_names_local_then_remote_port() {
        let with_stub = port_forward_args("k", true);
        assert_eq!(
            with_stub.last().map(String::as_str),
            Some("8090:8090"),
            "the stub's port is forwarded to the same local port, so its Host matches"
        );
        assert_eq!(with_stub.len(), port_forward_args("k", false).len() + 1);
        let args = port_forward_args("k", false);
        let at = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        assert_eq!(at("--address").as_deref(), Some("127.0.0.1"));
        let spec = args.last().cloned().unwrap_or_default();
        let ports: Vec<&str> = spec.split(':').collect();
        assert_eq!(
            ports,
            [API_LOCAL_PORT.to_string().as_str(), "8080"],
            "{spec}"
        );
        assert!(ports.iter().all(|p| p.parse::<u16>().is_ok()));
    }

    #[test]
    fn the_stub_runs_only_with_its_crate_and_the_servers_tier_check() -> Result<()> {
        let root = std::env::temp_dir().join(format!("xtask-stub-ready-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (server, check) = KEY_SET_TIER_CHECK;
        let write = |path: &str, text: &str| -> Result<()> {
            let path = root.join(path);
            fs::create_dir_all(path.parent().context("a parent")?)?;
            fs::write(path, text)?;
            Ok(())
        };
        assert!(!stub_ready(&root), "neither");
        write(STUB_CRATE, "[package]")?;
        write(STUB_MAIN, "fn main() { serve() }")?;
        assert!(!stub_ready(&root), "the crate without the server's check");
        write(server, "fn other() {}")?;
        assert!(!stub_ready(&root), "a server without the check");
        write(server, &format!("{check}url: &str, tier: Tier)"))?;
        assert!(stub_ready(&root), "both");
        write(STUB_MAIN, "fn main() {\n    todo!()\n}")?;
        assert!(
            !stub_ready(&root),
            "the crate's tests landed, its entry point still pending"
        );
        fs::remove_file(root.join(STUB_MAIN))?;
        assert!(!stub_ready(&root), "a crate with no entry point");
        write(STUB_MAIN, "fn main() { serve() }")?;
        fs::remove_file(root.join(STUB_CRATE))?;
        assert!(!stub_ready(&root), "the check without the crate");
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn without_a_stub_the_api_mounts_no_links_and_names_no_stub() {
        assert_eq!(
            signin_values(None),
            ["images.stub-issuer=", "api.subjectLinks="]
        );
        assert_eq!(
            signin_values(Some("mandate.local/stub-issuer:0123456789ab")),
            [
                "images.stub-issuer=mandate.local/stub-issuer:0123456789ab".to_string(),
                format!("api.subjectLinks={LINKS_CONFIGMAP}")
            ]
        );
    }

    #[test]
    fn the_demo_links_are_the_configmap_the_chart_mounts() {
        let links = "sha256:ab 01KDEM0000000000000000000P\n";
        let map = links_configmap(links);
        assert_eq!(map["kind"], "ConfigMap");
        assert_eq!(map["metadata"]["name"], LINKS_CONFIGMAP);
        assert_eq!(map["data"]["subject-links"], links);
        assert_eq!(
            map.as_object().map(|o| o.contains_key("binaryData")),
            Some(false)
        );
    }

    #[test]
    fn a_pulled_image_skips_sign_in_with_the_api() {
        let later = [
            process("api", true),
            process("runtime", true),
            process("executor", true),
        ];
        let mut parts = demo_parts(&later, "", true);
        skip_without_api_image(&mut parts);
        for name in [API_PART, SIGNIN_PART] {
            assert!(
                parts.iter().any(|p| p.name == name && p.skipped.is_some()),
                "{name} is skipped without an API image"
            );
        }
        assert!(
            parts
                .iter()
                .any(|p| p.name.starts_with("a paper agent") && p.skipped.is_none()),
            "parts that need no API image stay ready"
        );
    }

    #[test]
    fn a_session_opens_only_with_204_and_the_host_cookie() {
        let reply = |status: &str, cookie: &str| {
            format!("HTTP/1.1 {status}\r\n{cookie}content-length: 0\r\n\r\n")
        };
        let good = "set-cookie: __Host-mandate_session=v1.a.b; Path=/; Secure; HttpOnly\r\n";
        assert!(signed_in(204, &reply("204 No Content", good)));
        assert!(!signed_in(401, &reply("401 Unauthorized", good)));
        assert!(!signed_in(204, &reply("204 No Content", "")));
        assert!(!signed_in(
            204,
            &reply("204 No Content", "set-cookie: other=1\r\n")
        ));
        assert!(
            !signed_in(
                204,
                "HTTP/1.1 204 No Content\r\n\r\nset-cookie: __Host-mandate_session=x\r\n"
            ),
            "a cookie in the body is not a header"
        );
        assert_eq!(
            reply_body("HTTP/1.1 200 OK\r\na: b\r\n\r\n{\"x\":1}"),
            "{\"x\":1}"
        );
    }

    #[test]
    fn the_demo_owner_is_a_ulid_and_the_stub_key_set_is_its_published_path() {
        assert_eq!(STUB_KEY_SET, "/jwks.json");
        assert!(!LOCAL_ISSUER.ends_with('/'));
        assert!(
            DEMO_OWNER.len() == 26
                && DEMO_OWNER.bytes().all(
                    |b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b))
                )
        );
    }

    #[test]
    fn a_reply_status_is_read_from_its_first_line() {
        assert_eq!(status_of("HTTP/1.1 401 Unauthorized\r\n\r\n"), Some(401));
        assert_eq!(
            status_of("HTTP/1.1 200 OK\r\nx: HTTP/1.1 401\r\n"),
            Some(200)
        );
        assert_eq!(status_of("HTTP/1.0 401 Unauthorized\r\n"), None);
        assert_eq!(status_of(""), None);
        assert_eq!(status_of("HTTP/1.1 4x1 Odd\r\n"), None);
    }

    #[test]
    fn the_generated_files_are_what_the_process_list_gives() -> Result<()> {
        generate_in(&crate::repo_root()?, false)
    }
}
