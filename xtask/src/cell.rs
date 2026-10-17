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
use std::io::Write;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::{output, sh};

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
/// CloudNativePG's Barman Cloud plugin, which archives the journal's WAL and takes its base backups
/// (E21-41): its chart from the GitHub release, checked by SHA-256, and its two images pinned by
/// digest (D-5). Its certificates come from [`PLUGIN_TLS_CHART`], not cert-manager (DEC-1050).
const PLUGIN_CHART_URL: &str = "https://github.com/cloudnative-pg/charts/releases/download/plugin-barman-cloud-v0.8.1/plugin-barman-cloud-0.8.1.tgz";
const PLUGIN_CHART_SHA256: &str =
    "9ad0b9e56da2777a1a0269feaf7215e18297e9c8b5730e04acb27b5d462e9456";
const PLUGIN_IMAGE_TAG: &str =
    "v0.15.1@sha256:c75acad19a36e8176cfe2885761ff1b836c6ef977d7257e298c99a28b5c302ef";
const PLUGIN_SIDECAR_TAG: &str =
    "v0.15.1@sha256:2287a793a804db751edd0d07d44cd549e357677e98a800eb12e2cf7c0d53f9b0";
pub const PLUGIN_TLS_CHART: &str = "deploy/cell/charts/backup-plugin-tls";
/// The local cell's stand-in for the S1 endpoint, in its own namespace (DEC-1050 item 5).
pub const ARCHIVE_STORE_CHART: &str = "deploy/cell/charts/archive-store";
const ARCHIVE_STORE_NAMESPACE: &str = "archive-store";
pub const WORKSPACE_CHART: &str = "deploy/cell/charts/workspace";
pub const JOURNAL_CHART: &str = "deploy/cell/charts/journal-db";
const WORKSPACE_VALUES: &str = "deploy/cell/values/k3d.yaml";
const JOURNAL_VALUES: &str = "deploy/cell/values/k3d-journal-db.yaml";
pub const PROCESSES: &str = "deploy/cell/charts/workspace/processes.json";
pub const COMPOSE_FILE: &str = "deploy/cell/compose/compose.json";
const IMAGE_DOCKERFILE: &str = "deploy/cell/images/mandate.Dockerfile";
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
    /// With the synchronous standby fenced, an append waits: nothing commits on the primary alone,
    /// and once the standby is back the cluster takes appends again and every stream it holds
    /// verifies (OPS-2, E21-41).
    StandbyDown,
    /// A closed WAL segment and a base backup reach the archive: Postgres reports the segment
    /// archived, and Barman's own tools, reading the store with the archive's key, list the backup
    /// and fetch the segment back (E21-41).
    Archive,
}

pub const DRILLS: [Drill; 6] = [
    Drill::Boot,
    Drill::Journal,
    Drill::Egress,
    Drill::Compose,
    Drill::Restart,
    Drill::KillSwitch,
];

/// The journal's own drills (E21-41), which `cargo xtask ci cell-journal` runs in a job of their
/// own so each cell job stays within its ten minutes (DEC-464).
pub const JOURNAL_DRILLS: [Drill; 2] = [Drill::StandbyDown, Drill::Archive];

impl Drill {
    pub fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Journal => "journal",
            Self::Egress => "egress",
            Self::Compose => "compose",
            Self::Restart => "restart",
            Self::KillSwitch => "kill-switch",
            Self::StandbyDown => "standby-down",
            Self::Archive => "archive",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        DRILLS
            .into_iter()
            .chain(JOURNAL_DRILLS)
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
            Self::StandbyDown => {
                "the cluster's durability set to `preferred`, so the primary commits alone"
            }
            Self::Archive => "the archive pointed at another bucket of the same store",
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
            Self::StandbyDown => "committed with the standby fenced",
            Self::Archive => "not in the archive",
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

/// `cargo xtask ci cell-journal`: the cell, then the journal's drills, plain and seeded (E21-41).
/// The chart checks are `ci cell`'s. Skipped as `ci cell` is.
pub fn ci_journal() -> Result<()> {
    if let Some(reason) = tools_missing() {
        if env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    cell-journal: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    up()?;
    drill(&JOURNAL_DRILLS, false)?;
    drill(&JOURNAL_DRILLS, true)
}

/// The Kubernetes API's address for a rendering with no cluster to read it from: k3s's
/// `kubernetes` Service, the first address of its default service range.
const KUBE_API_RENDERED: &str = r#"kubeApi={"cidrs":["10.43.0.1/32"],"ports":[443]}"#;

/// helm lint and a strict schema check of every rendering of every chart, then the charts'
/// invariant tests, which render them themselves (deploy/tests/).
pub fn chart_checks() -> Result<()> {
    for (chart, values, namespace) in [
        (WORKSPACE_CHART, "deploy/cell/values/k3d.yaml", NAMESPACE),
        (WORKSPACE_CHART, "deploy/cell/values/k3s.yaml", NAMESPACE),
        (
            WORKSPACE_CHART,
            "deploy/cell/values/managed.yaml",
            NAMESPACE,
        ),
        (
            JOURNAL_CHART,
            "deploy/cell/values/k3d-journal-db.yaml",
            NAMESPACE,
        ),
        (ARCHIVE_STORE_CHART, "", ARCHIVE_STORE_NAMESPACE),
        (PLUGIN_TLS_CHART, "", CNPG_NAMESPACE),
    ] {
        let mut args = vec![chart];
        if !values.is_empty() {
            args.extend(["-f", values]);
        }
        if chart == JOURNAL_CHART {
            args.extend(["--set-json", KUBE_API_RENDERED]);
        }
        let mut lint = vec!["lint", "--strict"];
        lint.extend(&args);
        sh("helm", &lint)?;
        let mut template = vec!["template", "cell", "-n", namespace];
        template.extend(&args);
        let rendered = output("helm", &template)?;
        kubeconform(&rendered)?;
        let named = if values.is_empty() { chart } else { values };
        no_live_host_allowed(named, &parse_documents(&rendered)?, &live_addresses()?)?;
    }
    let status = Command::new("uv")
        .args([
            "run",
            "--locked",
            "--directory",
            "python",
            "pytest",
            "-q",
            "../deploy/tests",
        ])
        .env("MANDATE_CELL_HELM_REQUIRED", "1")
        .status()
        .context("starting pytest")?;
    if !status.success() {
        bail!("the chart's invariant tests failed with {status}");
    }
    Ok(())
}

/// Every rendered object against the Kubernetes 1.35 schemas; CloudNativePG's `Cluster` and
/// `ScheduledBackup` and the Barman Cloud plugin's `ObjectStore` have no schema there and are
/// skipped by kind, never by a missing-schema wildcard.
fn kubeconform(rendered: &str) -> Result<()> {
    let mut child = Command::new("kubeconform")
        .args([
            "-strict",
            "-summary",
            "-kubernetes-version",
            "1.35.0",
            "-skip",
            "Cluster,ObjectStore,ScheduledBackup",
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

/// The cell's `mandate` image: the pulled one, or this checkout's build tagged by the binary's own
/// SHA-256, so an uncommitted edit gets a new image rather than the last one imported.
fn image_tag() -> Result<String> {
    if let ImageSource::Pull(image) = host_image_source()? {
        return Ok(image);
    }
    let digest = sha256(&static_binary())?;
    let short = digest
        .get(..12)
        .context("a SHA-256 shorter than 12 digits")?;
    Ok(format!("mandate.local/mandate:{short}"))
}

fn static_binary() -> String {
    format!("{STATIC_TARGET}/{STATIC_TRIPLE}/debug/mandate")
}

/// `cargo xtask cell up`: build the `mandate` image from this checkout (never with the `live`
/// feature), bring up the k3d cluster if it is not up, install the CloudNativePG operator, the
/// journal's Postgres and the workspace chart, and wait until every pod is ready and the
/// migration Job has finished.
pub fn up() -> Result<()> {
    let source = host_image_source()?;
    if source == ImageSource::Build {
        build_binary()?;
    }
    let image = image_tag()?;
    if source == ImageSource::Build {
        build_image(&image)?;
    }
    if !cluster_exists()? {
        create_cluster()?;
    }
    if source == ImageSource::Build {
        sh("k3d", &["image", "import", "--cluster", CLUSTER, &image])?;
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
    let chart = fetched_chart("cloudnative-pg.tgz", CNPG_CHART_URL, CNPG_CHART_SHA256)?;
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
    backup_plugin()?;
    archive_store()?;
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
    let domain = kubectl(&[
        "get",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.metadata.annotations.mandate\\.dev/failure-domain}",
    ])?;
    eprintln!("    journal: failure domains: {}", domain.trim());
    let image_value = format!("images.mandate={image}");
    helm(&[
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
        "--wait",
        "--wait-for-jobs",
        "--timeout",
        "5m",
    ])?;
    eprintln!("    cell: up; `cargo xtask cell drill` runs the drills");
    Ok(())
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

/// `mandate-cli` linked statically, so the image needs no base (DEC-993 item 9), with no feature
/// at all: never `live`. The dev profile, for the CI job's ten minutes (DEC-464): it keeps
/// overflow checks, and the cell's journals are small.
fn build_binary() -> Result<()> {
    let args = [
        "build",
        "--locked",
        "--package",
        "mandate-cli",
        "--target",
        STATIC_TRIPLE,
        "--target-dir",
        STATIC_TARGET,
    ];
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
        bail!("the static build of mandate-cli failed with {status}");
    }
    Ok(())
}

fn build_image(image: &str) -> Result<()> {
    let context = state()?.join("image");
    if context.exists() {
        fs::remove_dir_all(&context).context("clearing the image context")?;
    }
    fs::create_dir_all(&context)?;
    let binary = static_binary();
    fs::copy(&binary, context.join("mandate"))
        .with_context(|| format!("copying {binary} into the image context"))?;
    let context = context.display().to_string();
    sh(
        "docker",
        &[
            "build",
            "--quiet",
            "-f",
            IMAGE_DOCKERFILE,
            "-t",
            image,
            &context,
        ],
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

/// A chart from its GitHub release, kept under `target/cell` as `name` and checked by SHA-256
/// every time it is used.
fn fetched_chart(name: &str, url: &str, want: &str) -> Result<String> {
    let path = state()?.join(name);
    let file = path.display().to_string();
    if !path.exists() || sha256(&file)? != want {
        sh(
            "curl",
            &[
                "-sSfL",
                "--retry",
                "5",
                "--retry-all-errors",
                "-o",
                &file,
                url,
            ],
        )?;
    }
    let found = sha256(&file)?;
    if found != want {
        bail!("the chart {name}'s SHA-256 is {found}, not {want}");
    }
    Ok(file)
}

/// The Barman Cloud plugin beside the operator, with the certificates the TLS chart makes in place
/// of cert-manager's; when an upgrade remade them, the plugin restarts, since it reads them only
/// at start.
fn backup_plugin() -> Result<()> {
    let before = plugin_certificate_end()?;
    helm(&[
        "upgrade",
        "--install",
        "backup-plugin-tls",
        PLUGIN_TLS_CHART,
        "--namespace",
        CNPG_NAMESPACE,
    ])?;
    let chart = fetched_chart(
        "plugin-barman-cloud.tgz",
        PLUGIN_CHART_URL,
        PLUGIN_CHART_SHA256,
    )?;
    let image = format!("image.tag={PLUGIN_IMAGE_TAG}");
    let sidecar = format!("sidecarImage.tag={PLUGIN_SIDECAR_TAG}");
    helm(&[
        "upgrade",
        "--install",
        "barman-cloud",
        &chart,
        "--namespace",
        CNPG_NAMESPACE,
        "--set",
        &image,
        "--set",
        &sidecar,
        "--set",
        "certificate.createClientCertificate=false",
        "--set",
        "certificate.createServerCertificate=false",
        "--set",
        "certificate.createIssuer=false",
        "--wait",
        "--timeout",
        "5m",
    ])?;
    if !before.is_empty() && before != plugin_certificate_end()? {
        kubectl(&[
            "rollout",
            "restart",
            "deployment/barman-cloud",
            "--namespace",
            CNPG_NAMESPACE,
        ])?;
        kubectl(&[
            "rollout",
            "status",
            "deployment/barman-cloud",
            "--namespace",
            CNPG_NAMESPACE,
            "--timeout=300s",
        ])?;
    }
    Ok(())
}

/// When the plugin's server certificate ends, as the TLS chart records it; empty before the first
/// install.
fn plugin_certificate_end() -> Result<String> {
    Ok(kubectl(&[
        "get",
        "secret",
        "barman-cloud-server-tls",
        "--namespace",
        CNPG_NAMESPACE,
        "--ignore-not-found",
        "-o",
        "jsonpath={.metadata.annotations.mandate\\.dev/not-after}",
    ])?
    .trim()
    .to_string())
}

/// The test store the journal archives to in the local cell, and the workspace's namespace, where
/// it puts the archive's key.
fn archive_store() -> Result<()> {
    kubectl(&[
        "create",
        "namespace",
        NAMESPACE,
        "--dry-run=client",
        "-o",
        "json",
    ])
    .and_then(|ns| {
        let ns: Value = serde_json::from_str(&ns).context("reading the namespace")?;
        apply(&[ns])
    })?;
    let client = format!("clientNamespace={NAMESPACE}");
    helm(&[
        "upgrade",
        "--install",
        "archive-store",
        ARCHIVE_STORE_CHART,
        "--namespace",
        ARCHIVE_STORE_NAMESPACE,
        "--create-namespace",
        "--set",
        &client,
        "--wait",
        "--timeout",
        "5m",
    ])
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
            Drill::StandbyDown => standby_down(seeded),
            Drill::Archive => archive(seeded),
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

/// The name of the journal's pod in `role` (`primary` or `replica`).
fn journal_pod(role: &str) -> Result<String> {
    let selector = format!("cnpg.io/cluster=journal,cnpg.io/instanceRole={role}");
    let name = kubectl(&[
        "get",
        "pod",
        "--namespace",
        NAMESPACE,
        "--selector",
        &selector,
        "-o",
        "jsonpath={.items[0].metadata.name}",
    ])?;
    let name = name.trim();
    if name.is_empty() {
        bail!("the journal has no {role} pod");
    }
    Ok(name.to_string())
}

/// One query on the primary as its local superuser, through the pod's own socket: the drills
/// read what Postgres itself holds, not what a process reports.
fn on_primary(sql: &str) -> Result<String> {
    let primary = journal_pod("primary")?;
    Ok(kubectl(&[
        "exec",
        "--namespace",
        NAMESPACE,
        &primary,
        "-c",
        "postgres",
        "--",
        "psql",
        "-X",
        "-d",
        "mandate",
        "-tAc",
        sql,
    ])?
    .trim()
    .to_string())
}

/// Polls `check` until it is true or the drill's polls run out.
fn until(mut check: impl FnMut() -> Result<bool>) -> Result<bool> {
    for _ in 0..POLLS {
        if check()? {
            return Ok(true);
        }
        sleep(Duration::from_secs(2));
    }
    Ok(false)
}

/// How many standbys stream to the primary and count for its synchronous commits.
fn synchronous_standbys() -> Result<String> {
    on_primary(
        "SELECT count(*) FROM pg_stat_replication WHERE sync_state IN ('sync', 'quorum') AND state = 'streaming'",
    )
}

/// The journal's events stored for `stream`, as the primary sees them.
fn stored_events(stream: &str) -> Result<String> {
    on_primary(&format!(
        "SELECT count(*) FROM events WHERE stream_id = '{stream}'"
    ))
}

/// The control CLI's `workspace open` for `workspace` as a run Job, ended after `deadline` seconds.
fn open_job(id: &str, workspace: &str, deadline: u32) -> Result<Vec<Value>> {
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
    let mut open = render(
        &[format!("run.id=\"{id}\""), format!("run.args={args}")],
        Some("templates/run.yaml"),
    )?;
    for job in open.iter_mut().filter(|o| o["kind"] == "Job") {
        job["spec"]["activeDeadlineSeconds"] = json!(deadline);
    }
    Ok(open)
}

/// The verify Job over `streams`, and whether it passed, with its logs.
fn verify_streams(id: &str, streams: &[String]) -> Result<(bool, String)> {
    let verify = render(
        &[
            format!("verify.id=\"{id}\""),
            format!("verify.streams={}", json!(streams)),
        ],
        Some("templates/run.yaml"),
    )?;
    run_job(verify)
}

/// How long the drill's append waits on the fenced standby before the drill ends it: long enough
/// that a primary committing alone would have answered many times over.
const FENCED_APPEND_SECS: u32 = 40;

/// OPS-2 with the synchronous standby down: the standby is fenced (CloudNativePG stops its
/// Postgres), an append is tried, and it must not commit: the CLI never reports it, and the
/// primary shows none of its events. The standby is unfenced, the cluster must take a new append,
/// and every stream written verifies. Seeded, the cluster's durability is `preferred` first, so the
/// primary drops the standby and commits alone, which the drill must report.
fn standby_down(seeded: bool) -> Result<Verdict> {
    if seeded {
        set_durability("preferred")?;
    }
    let fenced = fenced_append(seeded);
    let unfenced = kubectl(&[
        "annotate",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "cnpg.io/fencedInstances-",
    ]);
    if seeded {
        set_durability("required")?;
    }
    unfenced?;
    let (verdict, fenced) = fenced?;
    if verdict != Verdict::Held {
        return Ok(verdict);
    }
    kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        "cluster.postgresql.cnpg.io/journal",
        "--timeout=300s",
    ])?;
    if !until(|| Ok(synchronous_standbys()? == "1"))? {
        return Ok(Verdict::Broken(
            "the standby never streamed synchronously again after it was unfenced".into(),
        ));
    }
    let id = run_id()?;
    let workspace = format!("standby{id}");
    let (opened, logs) = run_job(open_job(&format!("{id}b"), &workspace, 120)?)?;
    if !opened {
        return Ok(Verdict::Broken(format!(
            "with the standby back, `mandate workspace open` failed:\n{logs}"
        )));
    }
    let mut streams = vec![format!("ctl:{workspace}")];
    if stored_events(&fenced)? != "0" {
        streams.push(fenced);
    }
    let (verified, logs) = verify_streams(&id, &streams)?;
    if verified {
        Ok(Verdict::Held)
    } else {
        Ok(Verdict::Broken(format!(
            "with the standby back, the streams did not verify:\n{logs}"
        )))
    }
}

/// Fences the standby, waits until the primary has no synchronous standby, and tries one append;
/// the verdict, and the stream the append was for, which the drill checks once the standby is back.
fn fenced_append(seeded: bool) -> Result<(Verdict, String)> {
    let standby = journal_pod("replica")?;
    let fence = format!("cnpg.io/fencedInstances=[\"{standby}\"]");
    kubectl(&[
        "annotate",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "--overwrite",
        &fence,
    ])?;
    let id = run_id()?;
    let stream = format!("ctl:fenced{id}");
    if !until(|| Ok(synchronous_standbys()? == "0"))? {
        let broken =
            format!("{standby} was fenced, but the primary still streams to a synchronous standby");
        return Ok((Verdict::Broken(broken), stream));
    }
    let workspace = format!("fenced{id}");
    let deadline = if seeded { 120 } else { FENCED_APPEND_SECS };
    let (opened, logs) = run_job(open_job(&id, &workspace, deadline)?)?;
    let stored = stored_events(&stream)?;
    let verdict = if opened {
        Verdict::Broken(format!(
            "an append committed with the standby fenced: `mandate workspace open` succeeded, and the primary holds {stored} of its events"
        ))
    } else if stored != "0" {
        Verdict::Broken(format!(
            "an append committed with the standby fenced: the primary holds {stored} events of {stream}, though the CLI never had its commit\n{logs}"
        ))
    } else if job_end(&format!("control-{id}"))? != "DeadlineExceeded" {
        Verdict::Broken(format!(
            "the append with the standby fenced failed rather than waited:\n{logs}"
        ))
    } else {
        Verdict::Held
    };
    Ok((verdict, stream))
}

/// Why a Job that failed ended, as its `Failed` condition says (`DeadlineExceeded` for one its
/// deadline stopped).
fn job_end(name: &str) -> Result<String> {
    Ok(kubectl(&[
        "get",
        "job",
        name,
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.status.conditions[?(@.type==\"Failed\")].reason}",
    ])?
    .trim()
    .to_string())
}

/// Sets the journal cluster's `dataDurability`, as only a seeded drill does.
fn set_durability(durability: &str) -> Result<()> {
    let patch = json!({"spec": {"postgresql": {"synchronous": {"dataDurability": durability}}}});
    kubectl(&[
        "patch",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "--type",
        "merge",
        "-p",
        &patch.to_string(),
    ])
    .map(|_| ())
}

/// The archive's destination and endpoint, as the installed ObjectStore holds them.
fn archive_destination() -> Result<(String, String)> {
    let both = kubectl(&[
        "get",
        "objectstore.barmancloud.cnpg.io/journal-archive",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.spec.configuration.destinationPath} {.spec.configuration.endpointURL}",
    ])?;
    let (path, url) = both
        .trim()
        .split_once(' ')
        .context("the archive's ObjectStore names no destination or endpoint")?;
    Ok((path.to_string(), url.to_string()))
}

/// Points the archive at `path`, as only a seeded drill does, and back.
fn set_destination(path: &str) -> Result<()> {
    let patch = json!({"spec": {"configuration": {"destinationPath": path}}});
    kubectl(&[
        "patch",
        "objectstore.barmancloud.cnpg.io/journal-archive",
        "--namespace",
        NAMESPACE,
        "--type",
        "merge",
        "-p",
        &patch.to_string(),
    ])
    .map(|_| ())
}

/// Writes a stream, closes the WAL segment that holds it, takes a base backup, and reads both back
/// from the store with Barman's own tools. Seeded, the archive goes to another bucket of the same
/// store first, which Postgres cannot tell, so only the read from the store can catch it.
fn archive(seeded: bool) -> Result<Verdict> {
    let (path, url) = archive_destination()?;
    if seeded {
        set_destination(&format!("{path}-elsewhere"))?;
    }
    let verdict = archived(&path, &url);
    if seeded {
        set_destination(&path)?;
    }
    verdict
}

fn archived(path: &str, url: &str) -> Result<Verdict> {
    let id = run_id()?;
    let workspace = format!("archive{id}");
    let (opened, logs) = run_job(open_job(&id, &workspace, 120)?)?;
    if !opened {
        return Ok(Verdict::Broken(format!(
            "`mandate workspace open` failed before the archive check:\n{logs}"
        )));
    }
    let segment = on_primary("SELECT pg_walfile_name(pg_current_wal_lsn())")?;
    on_primary("SELECT pg_switch_wal()")?;
    let reported = until(|| {
        let last = on_primary("SELECT coalesce(last_archived_wal, '') FROM pg_stat_archiver")?;
        Ok(last.len() == segment.len() && last >= segment)
    })?;
    if !reported {
        return Ok(Verdict::Broken(format!(
            "Postgres never reported {segment} archived: {}",
            on_primary("SELECT failed_count, last_failed_wal FROM pg_stat_archiver")?
        )));
    }
    let backup = format!("journal-drill-{id}");
    apply(&[json!({
        "apiVersion": "postgresql.cnpg.io/v1",
        "kind": "Backup",
        "metadata": {"name": backup},
        "spec": {
            "cluster": {"name": "journal"},
            "method": "plugin",
            "pluginConfiguration": {"name": "barman-cloud.cloudnative-pg.io"}
        }
    })])?;
    let mut phase = String::new();
    let finished = until(|| {
        phase = kubectl(&[
            "get",
            "backup",
            &backup,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.phase}/{.status.backupId}",
        ])?
        .trim()
        .to_string();
        Ok(phase.starts_with("completed/") || phase.starts_with("failed/"))
    })?;
    let backup_id = phase
        .strip_prefix("completed/")
        .unwrap_or_default()
        .to_string();
    if !finished || backup_id.is_empty() {
        return Ok(Verdict::Broken(format!(
            "the base backup {backup} did not complete (phase `{phase}`)"
        )));
    }
    let (read, logs) = read_archive(&id, path, url, &segment)?;
    if !read {
        return Ok(Verdict::Broken(format!(
            "{segment} or the base backup {backup_id} is not in the archive at {path}:\n{logs}"
        )));
    }
    if !logs.contains(&backup_id) {
        return Ok(Verdict::Broken(format!(
            "the base backup {backup_id} is not in the archive at {path}:\n{logs}"
        )));
    }
    Ok(Verdict::Held)
}

/// A pod with the plugin's own sidecar image and the archive's key that lists the base backups and
/// fetches `segment` back from `path`, as a restore would, under a network policy of its own that
/// reaches the store and nothing else.
fn read_archive(id: &str, path: &str, url: &str, segment: &str) -> Result<(bool, String)> {
    let image = format!("ghcr.io/cloudnative-pg/plugin-barman-cloud-sidecar:{PLUGIN_SIDECAR_TAG}");
    let name = format!("archive-check-{id}");
    let key = |env: &str, key: &str| json!({"name": env, "valueFrom": {"secretKeyRef": {"name": "journal-archive-key", "key": key}}});
    let tool = |name: &str, args: Vec<&str>| {
        json!({
            "name": name,
            "image": image,
            "command": args,
            "env": [
                key("AWS_ACCESS_KEY_ID", "ACCESS_KEY_ID"),
                key("AWS_SECRET_ACCESS_KEY", "ACCESS_SECRET_KEY"),
                {"name": "AWS_DEFAULT_REGION", "value": "us-east-1"},
                {"name": "HOME", "value": "/tmp"}
            ],
            "securityContext": {
                "allowPrivilegeEscalation": false, "readOnlyRootFilesystem": true,
                "capabilities": {"drop": ["ALL"]}
            },
            "resources": {
                "limits": {"cpu": "200m", "memory": "256Mi"},
                "requests": {"cpu": "200m", "memory": "256Mi"}
            },
            "volumeMounts": [{"name": "tmp", "mountPath": "/tmp"}]
        })
    };
    let fetched = format!("/tmp/{segment}");
    let policy = json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "NetworkPolicy",
        "metadata": {"name": "archive-check"},
        "spec": {
            "podSelector": {"matchLabels": {"mandate.dev/archive-check": "true"}},
            "policyTypes": ["Ingress", "Egress"],
            "ingress": [],
            "egress": [
                {"to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": "kube-system"}},
                         "podSelector": {"matchLabels": {"k8s-app": "kube-dns"}}}],
                 "ports": [{"protocol": "UDP", "port": 53}, {"protocol": "TCP", "port": 53}]},
                {"to": [{"namespaceSelector": {"matchLabels": {"kubernetes.io/metadata.name": ARCHIVE_STORE_NAMESPACE}},
                         "podSelector": {"matchLabels": {"app.kubernetes.io/name": "mandate-archive-store"}}}],
                 "ports": [{"protocol": "TCP", "port": 9000}]}
            ]
        }
    });
    let pod = json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "labels": {"mandate.dev/archive-check": "true", "mandate.dev/archive-client": "true"}
        },
        "spec": {
            "restartPolicy": "Never",
            "automountServiceAccountToken": false,
            "securityContext": {
                "runAsNonRoot": true, "runAsUser": 26, "runAsGroup": 26,
                "seccompProfile": {"type": "RuntimeDefault"}
            },
            "initContainers": [tool(
                "list",
                vec!["barman-cloud-backup-list", "--endpoint-url", url, path, "journal"]
            )],
            "containers": [tool(
                "fetch",
                vec!["barman-cloud-wal-restore", "--endpoint-url", url, path, "journal", segment, &fetched]
            )],
            "volumes": [{"name": "tmp", "emptyDir": {}}]
        }
    });
    apply(&[policy, pod])?;
    let mut phase = String::new();
    until(|| {
        phase = kubectl(&[
            "get",
            "pod",
            &name,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.phase}",
        ])?
        .trim()
        .to_string();
        Ok(phase == "Succeeded" || phase == "Failed")
    })?;
    let mut logs = String::new();
    for container in ["list", "fetch"] {
        if let Ok(out) = kubectl(&["logs", &name, "--namespace", NAMESPACE, "-c", container]) {
            for line in out.lines() {
                logs.push_str(&format!("[{container}] {line}\n"));
            }
        }
    }
    let _ = kubectl(&[
        "delete",
        "pod",
        &name,
        "--namespace",
        NAMESPACE,
        "--wait=false",
    ]);
    let _ = kubectl(&[
        "delete",
        "networkpolicy",
        "archive-check",
        "--namespace",
        NAMESPACE,
    ]);
    Ok((phase == "Succeeded", logs))
}

const COMPOSE_DIR: &str = "deploy/cell/compose";
/// Where the compose drill keeps the secrets and the exported segment, owned by the Compose user;
/// `secrets/` is ignored by git.
const COMPOSE_SECRETS: &str = "deploy/cell/compose/secrets";

/// `docker compose` with the Compose file, the drill's workspace label and this change's image.
fn compose_cmd(args: &[&str]) -> Result<String> {
    let image = image_tag()?;
    let mut full = vec!["compose", "-f", "compose.json"];
    full.extend_from_slice(args);
    let out = Command::new("docker")
        .current_dir(COMPOSE_DIR)
        .args(&full)
        .env("MANDATE_WORKSPACE", WORKSPACE)
        .env("MANDATE_IMAGE_MANDATE", &image)
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
    script.push_str("chown -R 999:999 /s\nchmod 0600 /s/postgres-superuser /s/*.pgpass\n");
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

/// The workspace `cargo xtask cell demo` opens, whose control stream it verifies.
const DEMO_WORKSPACE: &str = "demo";
/// Where `web/` serves on the developer's machine (its `npm run dev`).
const WEB_URL: &str = "http://127.0.0.1:4317";
/// The process-list name lane A1 gives the workspace API server; `api_is_a_planned_name` checks
/// that, once listed, it is a service.
const API_PROCESS: &str = "api";

/// One piece of the full local cell (E21-37's demo): ready, or skipped because the lane that
/// builds it is not on `main` yet. A skipped piece is named, never faked.
#[derive(Debug, PartialEq, Eq)]
pub struct DemoPart {
    pub name: &'static str,
    pub skipped: Option<String>,
}

/// Which demo parts this checkout can bring up: a process is there once the process list marks
/// it built (its lane's image exists), and the founding membership once the CLI has its gesture.
pub fn demo_parts(processes: &[Value], cli_help: &str) -> Vec<DemoPart> {
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
            name: "the workspace API server, through a port-forward",
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
    let parts = demo_parts(&processes()?, &help);
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
        );
        let skipped: Vec<&str> = parts
            .iter()
            .filter(|p| p.skipped.is_some())
            .map(|p| p.name)
            .collect();
        assert_eq!(
            skipped.len(),
            4,
            "API, agent, membership and web/ wait on their lanes: {skipped:?}"
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
        );
        assert!(parts.iter().all(|p| p.skipped.is_none()), "{parts:?}");
    }

    #[test]
    fn api_is_a_planned_name() -> Result<()> {
        let all = processes_in(&crate::repo_root()?)?;
        let listed = all.iter().find(|p| p["name"] == API_PROCESS);
        assert!(
            listed.is_none_or(|p| p["kind"] == "service"),
            "the API server, once listed, is a service named `{API_PROCESS}`"
        );
        let help = "Commands:\n  workspace  Open\n      membership is named in a wrapped line\n";
        let membership = demo_parts(&[], help)
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
            let agent = demo_parts(&list, "")
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
    fn the_generated_files_are_what_the_process_list_gives() -> Result<()> {
        generate_in(&crate::repo_root()?, false)
    }
}
