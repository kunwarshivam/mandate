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
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::Digest;

use crate::admission_drill;
use crate::{output, sh};
use web::web_run;

pub mod exchange;
pub mod executor;
pub mod scenario;
pub mod web;

pub const CLUSTER: &str = "mandate-cell";
pub(crate) const NODE: &str = "k3d-mandate-cell-server-0";
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
/// The workspace vault's chart (E21-32, DEC-2019), in the workspace's namespace, and its one pod.
pub const VAULT_CHART: &str = "deploy/cell/charts/vault";
const VAULT_MANAGED_VALUES: &str = "deploy/cell/values/vault-managed.yaml";
const VAULT_STAGING_VALUES: &str = "deploy/cell/values/vault-staging.yaml";
const VAULT_POD: &str = "vault-0";
/// Where under `target/cell` the local cell keeps its vault's one unseal key and root token, mode
/// 0600. They guard fake secrets only; a hosted cell's are the founder's (DEC-977 item 5).
const VAULT_INIT: &str = "vault-init.json";
pub const WORKSPACE_CHART: &str = "deploy/cell/charts/workspace";
pub const JOURNAL_CHART: &str = "deploy/cell/charts/journal-db";
const WORKSPACE_VALUES: &str = "deploy/cell/values/k3d.yaml";
const JOURNAL_VALUES: &str = "deploy/cell/values/k3d-journal-db.yaml";
pub const PROCESSES: &str = "deploy/cell/charts/workspace/processes.json";
pub const COMPOSE_FILE: &str = "deploy/cell/compose/compose.json";
const IMAGE_DOCKERFILE: &str = "deploy/cell/images/mandate.Dockerfile";
/// The workspace API server's image, built beside `mandate` from the same static build.
const API_DOCKERFILE: &str = "deploy/cell/images/api.Dockerfile";
/// The deployment manager's image (DEC-1172), built beside `mandate` from the same static build.
const DEPLOY_MANAGER_DOCKERFILE: &str = "deploy/cell/images/deploy-manager.Dockerfile";
/// The deployment manager's process name, which is also its image's key in the chart's values.
const DEPLOY_MANAGER_PROCESS: &str = "deploy-manager";
/// The role provisioner's image (DEC-2350), built beside `mandate` from the same static build.
const ROLE_PROVISIONER_DOCKERFILE: &str = "deploy/cell/images/role-provisioner.Dockerfile";
/// The role provisioner's process name, which is also its image's key in the chart's values.
const ROLE_PROVISIONER_PROCESS: &str = "role-provisioner";
/// The role provisioner's own AppRole Secret, which its pod mounts (`mandate.vaultCopy`) and
/// `mandate vault provision --secrets-namespace` issues.
const ROLE_PROVISIONER_SECRET: &str = "vault-approle-provisioner";
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
/// to start (`MANDATE_API_JWKS_URL`). The `token-exchange` image carries it for the same reason:
/// without roots its token client cannot be built (DEC-2063 item 8).
const HOST_CA_BUNDLES: [&str; 4] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];
const RUNTIME_DOCKERFILE: &str = "deploy/cell/images/runtime.Dockerfile";
/// The demo's recorded research bundle (DEC-2210 item 1), and each of its files, which the interim
/// runtime image carries under `research/` in its context and at
/// `/usr/local/share/mandate/research` in the image (DEC-2246). No release image carries it.
const DEMO_RESEARCH: &str = "deploy/cell/demo/research";
const DEMO_RESEARCH_FILES: [&str; 5] = [
    "research.json",
    "bars.json",
    "news.json",
    "filings.json",
    "instruments.json",
];
const EXECUTOR_DOCKERFILE: &str = "deploy/cell/images/executor.Dockerfile";
/// The workspace scheduler's image (E21-29): the single writer of the hosted workspace's clock
/// stream, whose ticks the runtime and the executor read.
const SCHEDULER_DOCKERFILE: &str = "deploy/cell/images/scheduler.Dockerfile";
const SCHEDULER_PROCESS: &str = "scheduler";
const SCHEDULER_PACKAGE: &str = "mandate-scheduler";
/// The workspace data process's image (E21-29 lane SC): the deployed instrument's daily bars from
/// the cell's recorded series, which the runtime reads read-only from the artifact store.
const DATA_DOCKERFILE: &str = "deploy/cell/images/data.Dockerfile";
const DATA_PROCESS: &str = "data";
const DATA_PACKAGE: &str = "mandate-datad";
/// The token-exchange process's image (DEC-1175), which the deployment manager starts once per
/// OAuth connect.
const EXCHANGE_DOCKERFILE: &str = "deploy/cell/images/token-exchange.Dockerfile";
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
/// Set on a host that refuses a process a negative `oom_score_adj`, even as root, as some
/// sandboxed VMs do: kubelet asks one for every pod sandbox, so without this no pod starts there
/// (`failed to update /proc/self/oom_score_adj: Permission denied`). The node's containerd then
/// keeps each container's score at or above its own instead of lowering it.
const RESTRICT_OOM_ENV: &str = "MANDATE_CELL_RESTRICT_OOM";
/// The k3s containerd drop-in [`RESTRICT_OOM_ENV`] mounts, and where k3s imports it from.
const RESTRICT_OOM_DROPIN: &str =
    "[plugins.'io.containerd.cri.v1.runtime']\n  restrict_oom_score_adj = true\n";
const CONTAINERD_DROPINS: &str = "/var/lib/rancher/k3s/agent/etc/containerd/config-v3.toml.d";
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
    /// The kill switch with the API, the deployment manager, the exporter, the model gateway and
    /// the control plane down (OPS-4, E21-34): `mandate agent kill` at each scope and `agent pause`
    /// commit through one-off Jobs under the control role, and the control stream holding them
    /// exports and verifies by journal spec §11. The stream owners' `KillSwitchActivated` is
    /// named as skipped until the runtime and executor run in the cell (DEC-2000 item 6).
    KillSwitch,
    /// The hosted clusters' kill-switch admission policies on the cell's API server, with the
    /// signature webhook down and failing closed (DEC-2045, DEC-2046): the founder's kill-switch
    /// Job is admitted and its pod made, every other way to a pod labelled `mandate.dev/process:
    /// control` is refused, and so is any change to such a pod or to a person's control Job.
    Admission,
    /// With the synchronous standby fenced, an append waits: nothing commits on the primary alone,
    /// and once the standby is back the cluster takes appends again and every stream it holds
    /// verifies (OPS-2, E21-41).
    StandbyDown,
    /// A closed WAL segment and a base backup reach the archive: Postgres reports the segment
    /// archived, and Barman's own tools, reading the store with the archive's key, list the backup
    /// and fetch the segment back (E21-41).
    Archive,
    /// The scheduler ticking a fresh workspace's clock stream once the chart names it the hosted
    /// workspace: two exports of its `clock:` stream a few seconds apart, the second longer
    /// (E21-29); see [`executor::ticks_drill`].
    Ticks,
    /// The scheduler ticking the run's workspace's clock stream (E21-29), then an agent deployed on
    /// a recorded paper connection, and the deployment manager starting that account's executor
    /// until its pod runs and answers `livez` (E21-30, DEC-2035); see [`executor`].
    Executor,
    /// `mandate vault provision` sets the cell's vault up, then each AppRole logs in from a pod
    /// labelled as the executor and reads only what DEC-977 item 2's table gives it: a runtime
    /// nothing, an executor exactly its own connection's token, the model gateway its key; and
    /// no policy of the cell's holds a wildcard but DEC-2349's prefix grants, each in its own policy
    /// (E21-32, DEC-2022 item 3).
    Vault,
    /// The real token-exchange pod the deployment manager starts redeems a code the cell's
    /// `oauth-fixture` issued, after a connect the CLI confirmed and the API started, against the
    /// cell's vault (E10-22, DEC-2063); see [`exchange`].
    Exchange,
    /// The same connect with the owner authorizing only once the manager's exchange pod runs, as
    /// at Alpaca, so the exchange waits for its code until the request's deadline (DEC-2363).
    ExchangeLate,
}

pub const DRILLS: [Drill; 4] = [Drill::Boot, Drill::Journal, Drill::Egress, Drill::Compose];

/// The journal's own drills (E21-41), which `cargo xtask ci cell-journal` runs in a job of their
/// own so each cell job stays within its ten minutes (DEC-464).
pub const JOURNAL_DRILLS: [Drill; 2] = [Drill::StandbyDown, Drill::Archive];

/// The drill that kills the journal's primary, which with a synchronous standby is a failover and
/// the old primary's rejoin; `cargo xtask ci cell-failover` runs it in a job of its own (DEC-464).
pub const FAILOVER_DRILLS: [Drill; 1] = [Drill::Restart];

/// The kill-switch drill (OPS-4, E21-34) and the admission drill, which `cargo xtask ci
/// cell-kill-switch` runs on a cell of its own so `ci cell` stays within its ten minutes (DEC-464,
/// DEC-2602).
pub const KILL_SWITCH_DRILLS: [Drill; 2] = [Drill::KillSwitch, Drill::Admission];

/// The scheduler's ticks drill (E21-29) and the executor drill (E21-30, DEC-2035), which `cargo
/// xtask ci cell-executor` runs on a cell of its own, so `ci cell-kill-switch` stays within its ten minutes (DEC-464, DEC-2615).
pub const EXECUTOR_DRILLS: [Drill; 2] = [Drill::Ticks, Drill::Executor];

/// The vault's drill (E21-32, DEC-2022), which `cargo xtask ci cell-vault` runs on a cell of its
/// own, so each cell job stays within its ten minutes (DEC-464, DEC-2616).
pub const VAULT_DRILLS: [Drill; 1] = [Drill::Vault];

/// The token-exchange drill (E10-22, DEC-2063), which `cargo xtask ci cell-exchange` runs on a cell
/// of its own, so each cell job stays within its ten minutes (DEC-464).
pub const EXCHANGE_DRILLS: [Drill; 2] = [Drill::Exchange, Drill::ExchangeLate];

impl Drill {
    pub fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Journal => "journal",
            Self::Egress => "egress",
            Self::Compose => "compose",
            Self::Restart => "restart",
            Self::KillSwitch => "kill-switch",
            Self::Admission => "admission",
            Self::StandbyDown => "standby-down",
            Self::Archive => "archive",
            Self::Ticks => "ticks",
            Self::Executor => "executor",
            Self::Vault => "vault",
            Self::Exchange => "exchange",
            Self::ExchangeLate => "exchange-late",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        DRILLS
            .into_iter()
            .chain(JOURNAL_DRILLS)
            .chain(FAILOVER_DRILLS)
            .chain(KILL_SWITCH_DRILLS)
            .chain(EXECUTOR_DRILLS)
            .chain(VAULT_DRILLS)
            .chain(EXCHANGE_DRILLS)
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
            Self::KillSwitch => "the connection-scope kill switch never committed",
            Self::Admission => {
                "on update the Job policy fires only on a newly control label, so a service account can change a person's control Job"
            }
            Self::StandbyDown => {
                "the cluster's durability set to `preferred`, so the primary commits alone"
            }
            Self::Archive => "the archive pointed at another bucket of the same store",
            Self::Ticks => {
                "the scheduler scaled to zero replicas once it ticks the run's workspace, so its \
                 clock stream stops"
            }
            Self::Executor => {
                "the connect check skipped, so the connection is never recorded and the agent \
                 deploys at version 1"
            }
            Self::Vault => {
                "the runtime's policy rewritten to read every connection's token through a wildcard"
            }
            Self::Exchange | Self::ExchangeLate => {
                "the connect's step-up never confirmed in the CLI, so the API must refuse its start"
            }
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
            Self::KillSwitch => "no kill_switch of scope connection",
            Self::Admission => "admitted a service account's change to a person's control Job",
            Self::StandbyDown => "committed with the standby fenced",
            Self::Archive => "is not in the archive",
            Self::Ticks => "the scheduler is not ticking",
            Self::Executor => "no executor pod",
            Self::Vault => "wildcard in mandate-runtime",
            Self::Exchange | Self::ExchangeLate => "step_up_missing",
        }
    }
}

/// A drill's outcome: the property held, or it did not and why. An error is the harness itself
/// failing, which never counts as either.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Held,
    Broken(String),
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

/// `cargo xtask ci cell`: the chart checks, then the cell itself and every drill; the drills with
/// their bugs planted are `ci cell-seeded`'s, and the upgrade in place is `ci cell-upgrade`'s.
/// Skipped without docker, k3d and helm unless CI requires it.
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
    drill(&DRILLS, false)
}

/// `cargo xtask ci cell-seeded`: the cell, then every drill of [`DRILLS`] again with its bug planted,
/// which each must catch. It ran in `ci cell` until that job came within reach of its ten minutes
/// (DEC-464). Skipped as `ci cell` is.
pub fn ci_seeded() -> Result<()> {
    if let Some(reason) = tools_missing() {
        if env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    cell-seeded: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    up()?;
    drill(&DRILLS, true)
}

/// `cargo xtask ci cell-upgrade`: the cell, then `cell demo --no-web`, whose own `up` upgrades every
/// release in place, with the demo's workspace, readiness, port-forward and sign-in. It ran at the
/// end of `ci cell` until that job outgrew its ten minutes (DEC-464). Skipped as `ci cell` is.
pub fn ci_upgrade() -> Result<()> {
    if let Some(reason) = tools_missing() {
        if env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    cell-upgrade: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    up()?;
    demo(false)
}

/// `cargo xtask ci cell-journal`, `ci cell-failover`, `ci cell-kill-switch`, `ci cell-executor` and
/// `ci cell-vault`: the cell, then `drills`, plain and seeded (E21-41, DEC-2602, DEC-2615, DEC-2616). The chart checks are `ci cell`'s. Skipped as `ci cell` is.
pub fn ci_drills(job: &str, drills: &[Drill]) -> Result<()> {
    if let Some(reason) = tools_missing() {
        if env::var_os(CELL_REQUIRED_ENV).is_some() {
            bail!("{CELL_REQUIRED_ENV} is set but {reason}");
        }
        eprintln!("    {job}: {reason}; skipping (see deploy/cell/README.md)");
        return Ok(());
    }
    up()?;
    drill(drills, false)?;
    drill(drills, true)
}

/// The Kubernetes API's address for a rendering with no cluster to read it from: k3s's
/// `kubernetes` Service, the first address of its default service range.
const KUBE_API_RENDERED: &str = r#"kubeApi={"cidrs":["10.43.0.1/32"],"ports":[443]}"#;
/// The same address for the vault's staging values, set key by key: a whole `kubeApi` map would
/// replace theirs, `enabled: true` included, and so render no route to the API server at all.
const KUBE_API_RENDERED_KEYS: [&str; 2] =
    [r#"kubeApi.cidrs=["10.43.0.1/32"]"#, "kubeApi.ports=[443]"];

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
        (VAULT_CHART, "", NAMESPACE),
        (VAULT_CHART, VAULT_MANAGED_VALUES, NAMESPACE),
        (VAULT_CHART, VAULT_STAGING_VALUES, NAMESPACE),
    ] {
        let mut args = vec![chart];
        if !values.is_empty() {
            args.extend(["-f", values]);
        }
        if chart == JOURNAL_CHART {
            args.extend(["--set-json", KUBE_API_RENDERED]);
        }
        if values == VAULT_STAGING_VALUES {
            for key in KUBE_API_RENDERED_KEYS {
                args.extend(["--set-json", key]);
            }
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

/// The packages [`build_binary`] always builds for the cell's images, the backtest runner among
/// them.
const BUILT_PACKAGES: [&str; 10] = [
    "mandate-cli",
    RUNNER,
    "mandate-api-server",
    "mandate-daemon",
    "mandate-deployd",
    "mandate-executord",
    "mandate-provisionerd",
    EXCHANGE_PACKAGE,
    SCHEDULER_PACKAGE,
    DATA_PACKAGE,
];

/// The token exchange's package (DEC-1175), a dev-profile build like the rest, so its cell token
/// URL compiles in (DEC-2633).
const EXCHANGE_PACKAGE: &str = "mandate-token-exchange";

/// The stub issuer's package, which [`build_binary`] adds once [`stub_ready`].
const STUB_PACKAGE: &str = "mandate-stub-issuer";

/// Every package whose binary the cell's images can hold: what [`build_binary`] builds. A change
/// that reaches none of them, and no path under `deploy/`, cannot change what a cell job runs, so
/// `cargo xtask ci select` starts the cell jobs only for one that does (DEC-2609).
pub const IMAGE_PACKAGES: [&str; 11] = [
    BUILT_PACKAGES[0],
    BUILT_PACKAGES[1],
    BUILT_PACKAGES[2],
    BUILT_PACKAGES[3],
    BUILT_PACKAGES[4],
    BUILT_PACKAGES[5],
    BUILT_PACKAGES[6],
    BUILT_PACKAGES[7],
    BUILT_PACKAGES[8],
    BUILT_PACKAGES[9],
    STUB_PACKAGE,
];

/// The cell's `api` image, this checkout's build of `mandate-api-server`: none when the cell pulls
/// its `mandate` image, since no published API image exists yet, so the chart leaves the API out.
fn api_image_tag() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag("api", &["mandate-api-server"]).map(Some)
}

/// The cell's `deploy-manager` image, this checkout's build of `mandate-deployd`: none when the
/// cell pulls its `mandate` image, as for the API server, so the chart leaves the manager out and
/// renders the runtime and executor as Deployments of their own.
fn deploy_manager_image_tag() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag(DEPLOY_MANAGER_PROCESS, &["mandate-deployd"]).map(Some)
}

/// The cell's `role-provisioner` image (DEC-2350), this checkout's build of `mandate-provisionerd`:
/// none when the cell pulls its `mandate` image, as for the deployment manager, so the chart leaves
/// the provisioner out and no AppRole of its own is issued.
fn role_provisioner_image_tag() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag(ROLE_PROVISIONER_PROCESS, &["mandate-provisionerd"]).map(Some)
}

/// The workspace chart's settings, each a `--set-json` pair, that `cell up` installs it with and
/// the drills render it with, beside [`image_sets`]: the API server's own endpoints as the
/// deployment manager's one `kube-api` destination (DEC-1172). The endpoints are read from the
/// cell, never written in k3d.yaml: docker picks the k3d network's subnet, so the server node's
/// address is known only once it exists.
fn egress_sets() -> Result<Vec<String>> {
    let (addresses, port) = kube_api_endpoints()?;
    Ok(vec![
        format!("egress.kube-api={}", json!(addresses)),
        format!("egress.kubeApiPort={port}"),
    ])
}

/// The `kubernetes` Service's endpoint addresses, each as a /32, and their port, from its
/// EndpointSlices: where a pod's connection to `kubernetes.default.svc` lands once the Service's
/// address is translated, which is what a network policy filters (6443 on k3s).
fn kube_api_endpoints() -> Result<(Vec<String>, u16)> {
    let found = kubectl(&[
        "get",
        "endpointslices",
        "--namespace",
        "default",
        "--selector",
        "kubernetes.io/service-name=kubernetes",
        "-o",
        "jsonpath={range .items[*].endpoints[*].addresses[*]}{@} {end}|{.items[0].ports[0].port}",
    ])?;
    let (ips, port) = found
        .trim()
        .split_once('|')
        .with_context(|| format!("the API server's endpoints read as `{found}`"))?;
    let addresses = ips
        .split_whitespace()
        .map(|ip| {
            ip.parse::<Ipv4Addr>()
                .map(|ip| format!("{ip}/32"))
                .with_context(|| format!("the API server's endpoint `{ip}` is not IPv4"))
        })
        .collect::<Result<Vec<_>>>()?;
    if addresses.is_empty() {
        bail!("the API server lists no endpoint address");
    }
    let port = port
        .trim()
        .parse::<u16>()
        .with_context(|| format!("the API server's endpoint port `{port}` is not a port"))?;
    Ok((addresses, port))
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
    local_tag_with(name, binaries, &[])
}

/// [`local_tag`] over the binaries and then each of `files`, so a change to a file the image
/// carries changes its tag too.
fn local_tag_with(name: &str, binaries: &[&str], files: &[String]) -> Result<String> {
    let digests = binaries
        .iter()
        .map(|binary| sha256(&static_binary(binary)))
        .chain(files.iter().map(|file| sha256(file)))
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

/// The cell's `runtime` image (DEC-1013): this checkout's `mandate-daemon`. `None` where the
/// `mandate` image is pulled: no signed runtime image is published yet, so the runtime is left out
/// of that cell rather than built from something else.
fn runtime_image() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    let files: Vec<String> = demo_research().into_iter().map(|(from, _)| from).collect();
    local_tag_with("runtime", &["mandate-daemon"], &files).map(Some)
}

/// The demo's research bundle as the runtime image's context files: each host path, and its name
/// under `research/`.
fn demo_research() -> Vec<(String, String)> {
    DEMO_RESEARCH_FILES
        .iter()
        .map(|file| {
            (
                format!("{DEMO_RESEARCH}/{file}"),
                format!("research/{file}"),
            )
        })
        .collect()
}

/// The cell's `executor` image (DEC-2031): this checkout's `mandate-executord`, whose broker is the
/// in-process fixture. `None` where the `mandate` image is pulled, as for the runtime.
fn executor_image() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag("executor", &["mandate-executord"]).map(Some)
}

/// The cell's `scheduler` image (E21-29): this checkout's `mandate-scheduler`. `None` where the
/// `mandate` image is pulled, as for the runtime and executor, whose ticks it writes.
fn scheduler_image() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag(SCHEDULER_PROCESS, &[SCHEDULER_PACKAGE]).map(Some)
}

/// The cell's `data` image (E21-29 lane SC): this checkout's `mandate-datad`. `None` where the
/// `mandate` image is pulled, as for the runtime, which reads what it stores.
fn data_image() -> Result<Option<String>> {
    if let ImageSource::Pull(_) = host_image_source()? {
        return Ok(None);
    }
    local_tag(DATA_PROCESS, &[DATA_PACKAGE]).map(Some)
}

/// The cell's `token-exchange` image, this checkout's build of `mandate-token-exchange`: none
/// without the deployment manager, the one process that starts it, as for the manager itself.
fn exchange_image() -> Result<Option<String>> {
    if deploy_manager_image_tag()?.is_none() {
        return Ok(None);
    }
    local_tag_with(
        "token-exchange",
        &[EXCHANGE_PACKAGE],
        &[EXCHANGE_DOCKERFILE.to_owned()],
    )
    .map(Some)
}

/// The fixture OAuth app the cell's API, token exchange and fixture provider share (DEC-1176 item
/// 6): a fixed fake client id, and the API's callback through the port-forward the browser uses.
const FIXTURE_CLIENT_ID: &str = "cell-fixture-client";
const FIXTURE_REDIRECT_URI: &str = "http://127.0.0.1:8080/v1/oauth/alpaca/callback";

/// The fixture provider's settings (DEC-1176): on only when the cell runs both the stub issuer,
/// whose image the fixture runs, and the token exchange, the one process that reaches it; else
/// none, so the chart renders no fixture.
fn fixture_values(stub: Option<&str>, exchange: Option<&str>) -> Vec<String> {
    if stub.is_none() || exchange.is_none() {
        return Vec::new();
    }
    vec![
        "oauthFixture.enabled=true".to_owned(),
        format!("alpacaOAuth.clientId={FIXTURE_CLIENT_ID}"),
        format!("alpacaOAuth.redirectUri={FIXTURE_REDIRECT_URI}"),
    ]
}

/// The API's vault binding and the connect finish (DEC-2338, DEC-2348 item 3): both on only
/// where the cell runs the API and the role provisioner, since only then does `cell up` provision
/// the vault, issuing `vault-approle-api` and `vault-approle-connection-manager` before the
/// workspace installs; else none, so neither pod names a Secret that does not exist.
fn vault_binding_values(api: Option<&str>, provisioner: Option<&str>) -> Result<Vec<String>> {
    if api.is_none() || provisioner.is_none() {
        return Ok(Vec::new());
    }
    Ok(vec![
        "api.vaultBinding=true".to_owned(),
        "api.connectFinish=true".to_owned(),
    ])
}

/// The `images.*` settings for the images this cell runs, so `up` and every render agree.
fn image_sets() -> Result<Vec<String>> {
    let api = api_image_tag()?;
    let provisioner = role_provisioner_image_tag()?;
    let mut sets = vec![
        format!("images.mandate={}", image_tag()?),
        format!("images.api={}", api.clone().unwrap_or_default()),
        format!(
            "images.{DEPLOY_MANAGER_PROCESS}={}",
            deploy_manager_image_tag()?.unwrap_or_default()
        ),
        format!(
            "images.{ROLE_PROVISIONER_PROCESS}={}",
            provisioner.clone().unwrap_or_default()
        ),
    ];
    sets.extend(vault_binding_values(
        api.as_deref(),
        provisioner.as_deref(),
    )?);
    let stub = stub_image_tag()?;
    let exchange = exchange_image()?;
    sets.extend(signin_values(stub.as_deref()));
    if let Some(exchange) = &exchange {
        sets.push(format!("images.token-exchange={exchange}"));
    }
    sets.extend(fixture_values(stub.as_deref(), exchange.as_deref()));
    if let Some(runtime) = runtime_image()? {
        sets.push(format!("images.runtime={runtime}"));
    }
    if let Some(executor) = executor_image()? {
        sets.push(format!("images.executor={executor}"));
    }
    if let Some(scheduler) = scheduler_image()? {
        sets.push(format!("images.{SCHEDULER_PROCESS}={scheduler}"));
    }
    if let Some(data) = data_image()? {
        sets.push(format!("images.{DATA_PROCESS}={data}"));
    }
    Ok(sets)
}

/// `cargo xtask cell up`: build the `mandate`, `api`, `runtime`, `executor`, `scheduler`, `data`,
/// `deploy-manager` and `role-provisioner` images from this checkout (never with the `live` feature), bring up the k3d
/// cluster if it is not up, install the CloudNativePG operator, the journal's Postgres and the
/// vault, issue the vault's AppRole Secrets once the provisioner runs ([`provision_vault`]), install
/// the workspace chart, and wait until every pod is ready and the migration Job has finished.
pub fn up() -> Result<()> {
    let source = host_image_source()?;
    if source == ImageSource::Build {
        build_binary()?;
    }
    let image = image_tag()?;
    let api = api_image_tag()?;
    let manager = deploy_manager_image_tag()?;
    let provisioner = role_provisioner_image_tag()?;
    let runtime = runtime_image()?;
    let executor = executor_image()?;
    let scheduler = scheduler_image()?;
    let data = data_image()?;
    let stub = stub_image_tag()?;
    let exchange = exchange_image()?;
    if source == ImageSource::Build {
        let runner = static_binary(RUNNER);
        build_image(&image, IMAGE_DOCKERFILE, "mandate", &[(&runner, RUNNER)])?;
    }
    if let Some(runtime) = &runtime {
        let research = demo_research();
        let files: Vec<(&str, &str)> = research
            .iter()
            .map(|(from, name)| (from.as_str(), name.as_str()))
            .collect();
        build_image(runtime, RUNTIME_DOCKERFILE, "mandate-daemon", &files)?;
    }
    if let Some(executor) = &executor {
        build_image(executor, EXECUTOR_DOCKERFILE, "mandate-executord", &[])?;
    }
    if let Some(scheduler) = &scheduler {
        build_image(scheduler, SCHEDULER_DOCKERFILE, SCHEDULER_PACKAGE, &[])?;
    }
    if let Some(data) = &data {
        build_image(data, DATA_DOCKERFILE, DATA_PACKAGE, &[])?;
    }
    if let Some(stub) = &stub {
        build_image(stub, STUB_DOCKERFILE, "mandate-stub-issuer", &[])?;
    }
    if let Some(exchange) = &exchange {
        build_image(
            exchange,
            EXCHANGE_DOCKERFILE,
            EXCHANGE_PACKAGE,
            &[(&host_ca_bundle()?, "ca-certificates.crt")],
        )?;
    }
    if let Some(manager) = &manager {
        build_image(manager, DEPLOY_MANAGER_DOCKERFILE, "mandate-deployd", &[])?;
    }
    if let Some(provisioner) = &provisioner {
        build_image(
            provisioner,
            ROLE_PROVISIONER_DOCKERFILE,
            "mandate-provisionerd",
            &[],
        )?;
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
    let built = (source == ImageSource::Build).then_some(&image);
    for image in [
        built,
        api.as_ref(),
        runtime.as_ref(),
        executor.as_ref(),
        scheduler.as_ref(),
        data.as_ref(),
        stub.as_ref(),
        exchange.as_ref(),
        manager.as_ref(),
        provisioner.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        import_image(image)?;
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
    ])
    .with_context(|| format!("the journal cluster was not Ready: {}", cluster_state()))?;
    let domain = kubectl(&[
        "get",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.metadata.annotations.mandate\\.dev/failure-domain}",
    ])?;
    eprintln!("    journal: failure domains: {}", domain.trim());
    let fresh = vault(provisioner.is_some())?;
    if provisioner.is_some() {
        provision_vault(fresh)?;
    }
    fixture_client_secret(
        || {
            kubectl(&[
                "get",
                "secret",
                FIXTURE_CLIENT_SECRET,
                "--namespace",
                NAMESPACE,
                "--ignore-not-found",
                "-o",
                "name",
            ])
        },
        || {
            let mut bytes = [0u8; FIXTURE_CLIENT_SECRET_BYTES];
            fs::File::open("/dev/urandom")
                .and_then(|mut random| random.read_exact(&mut bytes))
                .context("reading /dev/urandom")?;
            Ok(hex_of(&bytes))
        },
        |secret| apply(&[secret]),
    )?;
    if stub.is_some() {
        demo_links(&image)?;
    }
    let sets = image_sets()?;
    let mut install = vec![
        "upgrade",
        "--install",
        "workspace",
        WORKSPACE_CHART,
        "--namespace",
        NAMESPACE,
        "-f",
        WORKSPACE_VALUES,
    ];
    for set in &sets {
        install.extend(["--set", set.as_str()]);
    }
    let egress = egress_sets()?;
    for set in &egress {
        install.extend(["--set-json", set.as_str()]);
    }
    install.extend(["--wait", "--wait-for-jobs", "--timeout", "5m"]);
    let installed = helm(&install);
    if installed.is_err() && api.is_some() {
        api_diagnosis();
    }
    installed?;
    seed_client_secret()?;
    eprintln!("    cell: up; `cargo xtask cell drill` runs the drills");
    Ok(())
}

/// `cargo xtask cell hosted`: cell #1's own `cluster` step (`deploy/hosted/cell1.py`, E21-51,
/// DEC-2001 item 11) against a two-node k3d cluster of its own, the proof before the droplets are
/// bought: the same secrets, service-account patch, operator, journal-db and workspace chart with
/// cell #1's values, the tunnel off.
pub fn hosted() -> Result<()> {
    sh(
        "python3",
        &["deploy/hosted/cell1.py", "--target", "k3d", "all"],
    )
}

/// What the API's pods say when the workspace release does not become ready: the API's and the
/// connect finish's (DEC-2348 item 3), which runs from the API's image. Their pods, how each
/// container last ended (its reason, `OOMKilled` among them, and exit code), their containers'
/// last logs, current and from the run before a restart, and the namespace's recent events, so a
/// server that refused to start or was killed names it in the run's own output.
pub(super) fn api_diagnosis() {
    let selector = format!(
        "mandate.dev/workspace={WORKSPACE},mandate.dev/process in ({API_PROCESS},{CONNECT_FINISH_PROCESS})"
    );
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
    let ended = [
        "get",
        "pods",
        "--namespace",
        NAMESPACE,
        "--selector",
        selector.as_str(),
        "-o",
        "jsonpath={range .items[*]}{.metadata.name}{range .status.containerStatuses[*]} {.name}: {.lastState.terminated.reason} {.lastState.terminated.exitCode} restarts {.restartCount}{end}{\"\\n\"}{end}",
    ];
    let current = logs("--previous=false");
    let restarted = [
        "get",
        "pods",
        "--namespace",
        NAMESPACE,
        "--selector",
        selector.as_str(),
        "-o",
        "jsonpath={range .items[*]}{.metadata.name}{range .status.initContainerStatuses[*]} {.name} {.restartCount}{end}{range .status.containerStatuses[*]} {.name} {.restartCount}{end}{\"\\n\"}{end}",
    ];
    let events = [
        "get",
        "events",
        "--namespace",
        NAMESPACE,
        "--sort-by",
        ".lastTimestamp",
    ];
    let shown: [&[&str]; 3] = [&pods, &ended, &current];
    for args in shown {
        diagnose(args);
    }
    let listing = kubectl(&restarted).unwrap_or_else(|e| {
        eprintln!("    api diagnosis `kubectl {}`: {e:#}", restarted.join(" "));
        String::new()
    });
    for (pod, container) in restarted_containers(&listing) {
        diagnose(&[
            "logs",
            "--namespace",
            NAMESPACE,
            pod.as_str(),
            "--container",
            container.as_str(),
            "--previous=true",
            "--tail",
            "40",
        ]);
    }
    diagnose(&events);
}

/// Prints what `kubectl args` answers, or why it failed, under the API's diagnosis.
fn diagnose(args: &[&str]) {
    match kubectl(args) {
        Ok(text) => eprintln!("    api diagnosis `kubectl {}`:\n{text}", args.join(" ")),
        Err(e) => eprintln!("    api diagnosis `kubectl {}`: {e:#}", args.join(" ")),
    }
}

/// Each `(pod, container)` that has restarted, from a listing of one pod a line, its name then
/// `<container> <restarts>` pairs, init containers first. Only those have logs from a run before: `kubectl logs
/// --previous` over every container of a selector fails outright on the first one that has none,
/// an init container among them, which hid the API's own last run (main's run 40254976469).
fn restarted_containers(listing: &str) -> Vec<(String, String)> {
    listing
        .lines()
        .flat_map(|line| {
            let mut words = line.split_whitespace();
            let pod = words.next().unwrap_or_default().to_owned();
            let rest: Vec<&str> = words.collect();
            rest.chunks(2)
                .filter(|pair| pair.len() == 2 && pair[1] != "0")
                .map(|pair| (pod.clone(), pair[0].to_owned()))
                .collect::<Vec<_>>()
        })
        .collect()
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

/// `mandate-cli`, the backtest runner, `mandate-api-server` and the runtime's `mandate-daemon`
/// (DEC-1013), the deployment manager's `mandate-deployd` (DEC-1172), the executor's
/// `mandate-executord` (DEC-2031), the scheduler's `mandate-scheduler` (E21-29) and the data
/// process's `mandate-datad` linked statically, so their images need no base (DEC-993 item 9),
/// with no feature at all: never `live`; with them the stub issuer once [`stub_ready`], which the
/// dev profile's debug assertions are what compile in (DEC-2100 item 2). The dev profile, for the
/// CI job's ten minutes (DEC-464): it keeps overflow checks, and the cell's journals are small.
fn build_binary() -> Result<()> {
    let mut args = vec!["build", "--locked"];
    for package in BUILT_PACKAGES {
        args.extend(["--package", package]);
    }
    if stub_ready(Path::new(".")) {
        args.extend(["--package", STUB_PACKAGE]);
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
        let to = context.join(name);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {} in the image context", parent.display()))?;
        }
        fs::copy(source, to).with_context(|| {
            format!("copying {source} into the image context; the {image} image needs it")
        })?;
    }
    let context = context.display().to_string();
    sh(
        "docker",
        &["build", "--quiet", "-f", dockerfile, "-t", image, &context],
    )
}

/// How many times [`import_image`] imports one image before it gives up.
const IMPORT_ATTEMPTS: u32 = 3;

/// Imports `image` into the cell's node and confirms the node's containerd holds it. `k3d image
/// import` exits 0 when the import fails inside the node (its own error-level log lines, a node
/// killed with exit 137 and restarting), so a missing image surfaced only later, as a pod that
/// could not pull it or an API server that never answered. Each attempt waits for the API server
/// first, since a failed import can leave the node restarting.
fn import_image(image: &str) -> Result<()> {
    for attempt in 1..=IMPORT_ATTEMPTS {
        let imported = sh("k3d", &["image", "import", "--cluster", CLUSTER, image]);
        api_server_ready()?;
        if imported.is_ok() && node_holds(image) {
            return Ok(());
        }
        eprintln!("xtask: {image} is not in {NODE} after import attempt {attempt}");
    }
    bail!("{image} never reached {NODE} after {IMPORT_ATTEMPTS} imports")
}

/// Whether the node's containerd lists `image` by its exact reference; a node it cannot ask holds
/// nothing.
fn node_holds(image: &str) -> bool {
    output(
        "docker",
        &["exec", NODE, "ctr", "-n", "k8s.io", "images", "ls", "-q"],
    )
    .is_ok_and(|listed| listed_holds(&listed, image))
}

/// Whether `ctr images ls -q`'s output `listed` names `image` on a line of its own.
fn listed_holds(listed: &str, image: &str) -> bool {
    listed.lines().any(|line| line.trim() == image)
}

/// Waits for the cell's API server to answer `/readyz`.
fn api_server_ready() -> Result<()> {
    for _ in 0..POLLS {
        if kubectl(&["get", "--raw", "/readyz"]).is_ok() {
            return Ok(());
        }
        sleep(Duration::from_secs(2));
    }
    bail!("the cell's API server never became ready")
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
    if env::var_os(RESTRICT_OOM_ENV).is_some() {
        let written = state()?.join("containerd-restrict-oom.toml");
        fs::write(&written, RESTRICT_OOM_DROPIN)
            .with_context(|| format!("writing {}", written.display()))?;
        let dropin = fs::canonicalize(&written)
            .with_context(|| format!("resolving {}", written.display()))?;
        args.push("--volume".into());
        args.push(format!(
            "{}:{CONTAINERD_DROPINS}/restrict-oom.toml@server:0",
            dropin.display()
        ));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    sh("k3d", &args)?;
    api_server_ready()
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

/// When each of the plugin's two certificates ends, as the TLS chart records it; empty before the
/// first install. Either changing means the plugin must restart.
fn plugin_certificate_end() -> Result<String> {
    let mut ends = String::new();
    for secret in ["barman-cloud-server-tls", "barman-cloud-client-tls"] {
        let end = kubectl(&[
            "get",
            "secret",
            secret,
            "--namespace",
            CNPG_NAMESPACE,
            "--ignore-not-found",
            "-o",
            "jsonpath={.metadata.annotations.mandate\\.dev/not-after}",
        ])?;
        ends.push_str(end.trim());
        ends.push('/');
    }
    Ok(if ends == "//" { String::new() } else { ends })
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

/// What a vault's `bao status -format=json` says, judged as the client judges `sys/seal-status`
/// (DEC-977 items 5 and 6): the cell refuses a vault on in-memory storage, a dev server, as the
/// client does outside its tests.
#[derive(Debug, PartialEq, Eq)]
enum VaultState {
    Uninitialized,
    Sealed,
    Unsealed,
}

fn vault_state(status: &str) -> Result<VaultState> {
    let status: Value = serde_json::from_str(status).context("reading the vault's status")?;
    let initialized = status["initialized"]
        .as_bool()
        .context("the vault's status has no `initialized`")?;
    let sealed = status["sealed"]
        .as_bool()
        .context("the vault's status has no `sealed`")?;
    let storage = status["storage_type"].as_str().unwrap_or_default();
    if initialized && storage != "raft" {
        bail!(
            "the vault's storage is `{storage}`, not raft: a dev server is refused (DEC-977 item 6)"
        );
    }
    Ok(match (initialized, sealed) {
        (false, _) => VaultState::Uninitialized,
        (true, true) => VaultState::Sealed,
        (true, false) => VaultState::Unsealed,
    })
}

/// `bao` in the vault's pod, with `stdin` on its standard input, and its standard output, whatever
/// its exit: `bao status` exits 2 for a sealed vault. Its output is never printed, since `operator
/// init`'s holds the unseal key and the root token.
fn bao(args: &[&str], stdin: &[u8]) -> Result<(bool, String)> {
    let mut command = vec!["bao"];
    command.extend_from_slice(args);
    in_vault_pod(&command, stdin)
}

/// `command` in the vault's pod, as [`bao`] runs `bao`.
fn in_vault_pod(command: &[&str], stdin: &[u8]) -> Result<(bool, String)> {
    let mut full = vec![
        "exec",
        "-i",
        NODE,
        "kubectl",
        "exec",
        "-i",
        "--namespace",
        NAMESPACE,
        VAULT_POD,
        "--",
    ];
    full.extend_from_slice(command);
    let mut child = Command::new("docker")
        .args(&full)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting bao in the vault's pod")?;
    child
        .stdin
        .take()
        .context("bao's stdin")?
        .write_all(stdin)?;
    let out = child.wait_with_output()?;
    let text = String::from_utf8(out.stdout).context("bao's output is not UTF-8")?;
    Ok((out.status.success(), text))
}

/// The `helm` arguments that install the cell's vault. With `kube_api`, Kubernetes auth is on as in
/// staging (DEC-2060, DEC-2062): the vault's pod projects its ServiceAccount's token and the
/// cluster's CA, the account may review tokens (`system:auth-delegator`), and its egress is the
/// API server alone, read at install. OpenBao refuses a Kubernetes auth config with no CA to read,
/// so `mandate vault provision --kubernetes-host` needs it.
fn vault_install_args(kube_api: bool) -> Vec<&'static str> {
    let mut args = vec![
        "upgrade",
        "--install",
        "vault",
        VAULT_CHART,
        "--namespace",
        NAMESPACE,
        "--create-namespace",
        "--timeout",
        "5m",
    ];
    if kube_api {
        args.extend(["--set", "kubeApi.enabled=true"]);
    }
    args
}

/// The workspace vault (E21-32, DEC-2019): installed without `--wait`, since it is ready only once
/// unsealed, then initialized once with one unseal key, kept with the root token under
/// `target/cell` (mode 0600, set again on a file that was already there; the cell holds fake
/// secrets only), and unsealed with it, the key
/// passed on stdin and never as an argument. A hosted cell is initialized and unsealed by hand
/// (deploy/cell/README.md, "The vault"). Answers whether this run initialized it, so a vault made
/// afresh is provisioned afresh ([`provision_vault`]). With `kube_api`, the vault runs as staging's
/// does ([`vault_install_args`]), so the Kubernetes auth [`provision_vault`] sets up can be.
fn vault(kube_api: bool) -> Result<bool> {
    helm(&vault_install_args(kube_api))?;
    let mut answer = None;
    for _ in 0..POLLS {
        if let Ok((_, status)) = bao(&["status", "-format=json"], b"")
            && serde_json::from_str::<Value>(&status).is_ok()
        {
            answer = Some(status);
            break;
        }
        sleep(Duration::from_secs(2));
    }
    let mut seal = vault_state(&answer.context("the vault never answered `bao status`")?)?;
    let init = state()?.join(VAULT_INIT);
    let mut fresh = false;
    if seal == VaultState::Uninitialized {
        let (ok, made) = bao(
            &[
                "operator",
                "init",
                "-key-shares=1",
                "-key-threshold=1",
                "-format=json",
            ],
            b"",
        )?;
        if !ok {
            bail!("`bao operator init` failed");
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&init)
            .with_context(|| format!("writing {}", init.display()))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(made.as_bytes())?;
        seal = VaultState::Sealed;
        fresh = true;
    }
    if seal == VaultState::Sealed {
        let kept = fs::read_to_string(&init).with_context(|| {
            format!(
                "the vault is initialized but {} is gone; `cargo xtask cell down` starts afresh",
                init.display()
            )
        })?;
        let kept: Value = serde_json::from_str(&kept).context("reading the vault's init")?;
        let key = kept["unseal_keys_b64"][0]
            .as_str()
            .context("the vault's init holds no unseal key")?;
        let (ok, _) = bao(
            &["write", "-format=json", "sys/unseal", "key=-"],
            key.as_bytes(),
        )?;
        if !ok {
            bail!("unsealing the vault failed");
        }
        let (_, status) = bao(&["status", "-format=json"], b"")?;
        if vault_state(&status)? != VaultState::Unsealed {
            bail!("the vault is still sealed");
        }
    }
    kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        &format!("pod/{VAULT_POD}"),
        "--timeout=120s",
    ])?;
    eprintln!("    vault: unsealed, on raft storage");
    Ok(fresh)
}

/// The Secret the local cell makes for the `oauth-fixture` pod, which the workspace chart only
/// mounts, and its key, which holds the fake Alpaca OAuth client secret as plain text (DEC-1176
/// item 6; the chart makes no Secret, DEC-822 item 4).
const FIXTURE_CLIENT_SECRET: &str = "oauth-fixture-client";
const FIXTURE_CLIENT_SECRET_KEY: &str = "client-secret";

/// The script that writes the fixture's client secret into the vault at the client secret's own
/// path (`SecretPath::ClientSecret`, DEC-2636), in the pod, from stdin alone: the root token is
/// its first line, read by `read` so no more of stdin is taken, and the value, already base64 as
/// the vault stores it, is the rest, `value=-`. The KV v2 mount is enabled first when the vault
/// is not provisioned yet, as provisioning would enable it. Neither ever reaches an argument.
const CLIENT_SECRET_SEED: &str = "read -r BAO_TOKEN && export BAO_TOKEN && \
     { bao secrets list -format=json | grep -q '\"mandate/\"' || \
       bao secrets enable -path=mandate -version=2 kv >/dev/null; } && \
     exec bao kv put -mount=mandate platform/alpaca-oauth-client-secret value=- >/dev/null";

/// [`CLIENT_SECRET_SEED`]'s stdin: the root token, then the value as the Secret's data holds it.
fn client_secret_seed_stdin(root: &str, encoded: &str) -> String {
    format!("{root}\n{encoded}")
}

/// The length of the fixture client's fake secret, in random bytes, written as hex.
const FIXTURE_CLIENT_SECRET_BYTES: usize = 32;

/// Whether the cell keeps the fixture client's Secret it already holds: `found` is kubectl's
/// `get secret -o name` answer, empty when there is none.
fn keep_existing(found: &str) -> bool {
    !found.trim().is_empty()
}

/// Makes [`FIXTURE_CLIENT_SECRET`] once, before the workspace release whose `oauth-fixture` pod
/// mounts it, and keeps one already made, so the pod and the vault's seed agree across every
/// `cell up` (DEC-1176 item 6): `found` reads what the cell holds, `value` draws a fake client's
/// secret, random bytes in hex, which no real provider knows, and `create` applies the Secret,
/// which kubectl reads from stdin, so the value reaches no argument. `Ok(true)` when it was made;
/// any failure fails `cell up`.
fn fixture_client_secret(
    found: impl FnOnce() -> Result<String>,
    value: impl FnOnce() -> Result<String>,
    create: impl FnOnce(Value) -> Result<()>,
) -> Result<bool> {
    if keep_existing(&found()?) {
        eprintln!("    oauth-fixture: its client secret is kept");
        return Ok(false);
    }
    create(fixture_client_secret_object(&value()?))?;
    eprintln!("    oauth-fixture: its client secret is made");
    Ok(true)
}

/// The fixture client's Secret holding `value` at its one key.
fn fixture_client_secret_object(value: &str) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": FIXTURE_CLIENT_SECRET,
            "namespace": NAMESPACE,
            "labels": {"app.kubernetes.io/name": "mandate-cell", "mandate.dev/process": "oauth-fixture"}
        },
        "type": "Opaque",
        "stringData": {FIXTURE_CLIENT_SECRET_KEY: value}
    })
}

/// `bytes` in lowercase hex, two digits each.
fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Seeds a dev or ci cell's vault with the fake client secret the `oauth-fixture` pod checks, read
/// from [`FIXTURE_CLIENT_SECRET`] once the workspace release is installed, so the token exchange
/// redeems the fixture's codes (DEC-1176 item 6, DEC-2636). Only the local cell runs it, with its
/// fake root token; a cell without the Secret is told so and the vault is left as it is. The value
/// is never decoded.
fn seed_client_secret() -> Result<()> {
    let jsonpath = format!("jsonpath={{.data.{FIXTURE_CLIENT_SECRET_KEY}}}");
    let encoded = kubectl(&[
        "get",
        "secret",
        FIXTURE_CLIENT_SECRET,
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
        "-o",
        &jsonpath,
    ])?;
    let encoded = encoded.trim();
    if encoded.is_empty() {
        eprintln!("    vault: no {FIXTURE_CLIENT_SECRET} Secret, so no client secret is seeded");
        return Ok(());
    }
    let init = fs::read_to_string(state()?.join(VAULT_INIT)).context("reading the vault's init")?;
    let init: Value = serde_json::from_str(&init).context("reading the vault's init")?;
    let root = init["root_token"]
        .as_str()
        .context("the vault's init holds no root token")?;
    let (seeded, _) = in_vault_pod(
        &["sh", "-c", CLIENT_SECRET_SEED],
        client_secret_seed_stdin(root, encoded).as_bytes(),
    )?;
    ensure!(seeded, "seeding the vault's client secret failed");
    eprintln!("    vault: the fixture's client secret is seeded");
    Ok(())
}

/// Where [`provision_vault`] copies the host's static `mandate` CLI in the k3s node: under k3s's
/// own data volume, which every node has.
const NODE_CLI: &str = "/var/lib/rancher/k3s/mandate-provision";

/// The script [`provision_vault`] runs in the k3s node, which holds `kubectl` and reaches the
/// vault's pod by a port-forward on its own loopback, the listener the vault's certificate names:
/// it reads the vault's CA, forwards a free port, and pipes `mandate vault provision
/// --secrets-namespace` into `kubectl apply`, as staging's apply does (DEC-2630). The root token is
/// the script's standard input, which only the CLI reads, so it is never an argument, a file or an
/// environment variable; `kubectl`'s own calls read `/dev/null`. The Kubernetes auth host is `$1`.
/// The forward is stopped, and the CA's directory and the copied CLI removed, however it ends. A
/// CLI that fails prints nothing, so `kubectl apply`, given no object, fails the script too.
fn provision_script() -> String {
    format!(
        "set -eu\n\
         pf=\n\
         dir=$(mktemp -d /var/lib/rancher/k3s/provision.XXXXXX)\n\
         trap '[ -z \"$pf\" ] || kill \"$pf\"; rm -rf \"$dir\" {NODE_CLI}' EXIT\n\
         kubectl get configmap vault-ca --namespace {NAMESPACE} -o 'jsonpath={{.data.ca\\.crt}}' \
           </dev/null >\"$dir/ca.crt\"\n\
         kubectl port-forward --namespace {NAMESPACE} pod/{VAULT_POD} :8200 \
           </dev/null >\"$dir/forward\" 2>/dev/null &\n\
         pf=$!\n\
         port=\n\
         for _ in $(seq 100); do\n\
           port=$(sed -n 's/^Forwarding from 127.0.0.1:\\([0-9]*\\) .*/\\1/p' \"$dir/forward\")\n\
           [ -z \"$port\" ] || break\n\
           sleep 0.1\n\
         done\n\
         [ -n \"$port\" ]\n\
         {NODE_CLI} vault provision --address \"https://127.0.0.1:$port\" --ca \"$dir/ca.crt\" \
           --operator-token-stdin --secrets-namespace {NAMESPACE} --kubernetes-host \"$1\" \
           | kubectl apply --namespace {NAMESPACE} -f - >/dev/null\n"
    )
}

/// The cell's `kubernetes` Service as the vault's Kubernetes auth reaches it, `https://` its
/// cluster address and its `https` port, as staging's apply reads it.
fn kubernetes_host() -> Result<String> {
    let found = kubectl(&[
        "get",
        "service",
        "kubernetes",
        "--namespace",
        "default",
        "-o",
        "jsonpath={.spec.clusterIP}:{.spec.ports[?(@.name==\"https\")].port}",
    ])?;
    kubernetes_host_from(&found)
}

/// [`kubernetes_host`] from what `kubectl` printed: an IPv4 address and a port, or an error.
fn kubernetes_host_from(found: &str) -> Result<String> {
    let found = found.trim();
    let (ip, port) = found
        .split_once(':')
        .with_context(|| format!("the kubernetes Service read as `{found}`"))?;
    let ip: Ipv4Addr = ip
        .parse()
        .with_context(|| format!("the kubernetes Service's address `{ip}` is not IPv4"))?;
    let port: u16 = port
        .parse()
        .with_context(|| format!("the kubernetes Service's port `{port}` is not a port"))?;
    Ok(format!("https://{ip}:{port}"))
}

/// Whether `cell up` provisions the vault: always for a vault this run initialized (`fresh`), whose
/// roles and secret ids are new, so a Secret left from the vault before would hold a stale secret
/// id; otherwise only when [`ROLE_PROVISIONER_SECRET`] is missing (`held` false), so a second
/// `cell up` rotates no credential in use.
fn must_provision(fresh: bool, held: bool) -> bool {
    fresh || !held
}

/// Issues the vault's AppRole Secrets in the workspace's namespace (DEC-2350, DEC-2630), the role
/// provisioner's [`ROLE_PROVISIONER_SECRET`] among them, and sets Kubernetes auth up at
/// [`kubernetes_host`], unless [`must_provision`] says the Secrets there are still the vault's.
/// The host's static `mandate` CLI is copied into the k3s node and run there by
/// [`provision_script`], with the cell's fake root token on its standard input.
fn provision_vault(fresh: bool) -> Result<()> {
    let held = kubectl(&[
        "get",
        "secret",
        ROLE_PROVISIONER_SECRET,
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
        "-o",
        "name",
    ])?;
    if !must_provision(fresh, !held.trim().is_empty()) {
        eprintln!("    vault: {ROLE_PROVISIONER_SECRET} is there, so nothing is provisioned");
        return Ok(());
    }
    let init = fs::read_to_string(state()?.join(VAULT_INIT)).context("reading the vault's init")?;
    let init: Value = serde_json::from_str(&init).context("reading the vault's init")?;
    let root = init["root_token"]
        .as_str()
        .context("the vault's init holds no root token")?;
    let host = kubernetes_host()?;
    sh(
        "docker",
        &[
            "cp",
            &static_binary("mandate"),
            &format!("{NODE}:{NODE_CLI}"),
        ],
    )?;
    let mut child = Command::new("docker")
        .args([
            "exec",
            "-i",
            NODE,
            "sh",
            "-c",
            &provision_script(),
            "provision",
            &host,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("starting the vault's provisioning")?;
    child
        .stdin
        .take()
        .context("the provisioning's stdin")?
        .write_all(root.as_bytes())?;
    let status = child.wait()?;
    ensure!(
        status.success(),
        "provisioning the vault failed with {status}"
    );
    let issued = kubectl(&[
        "get",
        "secret",
        ROLE_PROVISIONER_SECRET,
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
        "-o",
        "name",
    ])?;
    ensure!(
        !issued.trim().is_empty(),
        "the vault's provisioning issued no {ROLE_PROVISIONER_SECRET}"
    );
    eprintln!("    vault: provisioned, its AppRole Secrets issued");
    Ok(())
}

/// The connections the vault drill provisions: the cell's demo connection and one other, so an
/// executor's reach to a connection not its own is tried.
const VAULT_DRILL_CONNECTIONS: [&str; 2] = ["demo-paper", "drill-other"];

/// The vault drill's checks, `<role> <path>`, each with whether DEC-977 item 2's table (and
/// DEC-2020's rows) lets that role read that KV path. Written out here, not read from the crate:
/// a connection's entries are under `x` and its id's hex (DEC-2060 item 3), `x64656d6f2d7061706572`
/// for `demo-paper` and `x6472696c6c2d6f74686572` for `drill-other`.
const VAULT_DRILL_READS: [(&str, &str, bool); 13] = [
    ("runtime", "connections/x64656d6f2d7061706572/token", false),
    (
        "runtime",
        "connections/x6472696c6c2d6f74686572/token",
        false,
    ),
    ("runtime", "platform/alpaca-oauth-client-secret", false),
    (
        "executor-demo-paper",
        "connections/x64656d6f2d7061706572/token",
        true,
    ),
    (
        "executor-demo-paper",
        "connections/x6472696c6c2d6f74686572/token",
        false,
    ),
    (
        "executor-demo-paper",
        "platform/alpaca-oauth-client-secret",
        false,
    ),
    (
        "executor-drill-other",
        "connections/x6472696c6c2d6f74686572/token",
        true,
    ),
    (
        "executor-drill-other",
        "connections/x64656d6f2d7061706572/token",
        false,
    ),
    ("model-gateway", "platform/model-provider-key", true),
    (
        "model-gateway",
        "connections/x64656d6f2d7061706572/token",
        false,
    ),
    (
        "model-gateway",
        "platform/alpaca-oauth-client-secret",
        false,
    ),
    ("model-gateway", "platform/email-credential", false),
    (
        "executor-demo-paper",
        "connections/x64656d6f2d7061706572/code",
        false,
    ),
];

/// How many `mandate-*` policies the drill's scan must have read: one per process type (the API,
/// the connection manager, the runtime, the secret import, the model gateway and the email
/// sender) and two per connection (its executor and its token exchange), for
/// [`VAULT_DRILL_CONNECTIONS`]. A scan that reads fewer, as one whose `bao policy list` failed,
/// has proved nothing about wildcards.
const VAULT_DRILL_POLICIES: usize = 10;

/// The only stanzas holding a wildcard that the vault drill admits, each in the one policy it may
/// appear in: DEC-2349's one-segment `+` for a connection the web made after the vault was
/// provisioned, on the API's write of a connect's code and its and the connection manager's deletes,
/// and the connection manager's read of each request's account fingerprint, a pseudonymous hash and
/// no credential (DEC-2345); and the role provisioner's role under each Kubernetes-auth mount, the
/// `+` naming the role, with every parameter but the ServiceAccount's name pinned (DEC-2350 item
/// 1). Each is a whole stanza, its closing brace included, as the scan prints it
/// ([`vault_drill_scan`]), whitespace squeezed, so another path, another capability anywhere in
/// the stanza, a `*`, the same stanza in another policy, or
/// a wildcard outside a stanza is still reported.
const VAULT_DRILL_PREFIX_GRANTS: [(&str, &str); 10] = [
    (
        "mandate-api",
        "path \"mandate/data/connections/+/code\" { capabilities = [\"create\", \"update\"] }",
    ),
    (
        "mandate-api",
        "path \"mandate/metadata/connections/+/code\" { capabilities = [\"delete\"] }",
    ),
    (
        "mandate-api",
        "path \"mandate/metadata/connections/+/token\" { capabilities = [\"delete\"] }",
    ),
    (
        "mandate-connection-manager",
        "path \"mandate/metadata/connections/+/code\" { capabilities = [\"delete\"] }",
    ),
    (
        "mandate-connection-manager",
        "path \"mandate/metadata/connections/+/token\" { capabilities = [\"delete\"] }",
    ),
    (
        "mandate-connection-manager",
        "path \"mandate/data/personal/fingerprints/+/+\" { capabilities = [\"read\"] }",
    ),
    (PROVISIONER_POLICY, PROVISIONER_EXECUTOR_ROLES),
    (PROVISIONER_POLICY, PROVISIONER_EXCHANGE_ROLES),
    (
        KUBERNETES_EXECUTOR_POLICY,
        "path \"mandate/data/personal/accounts/x{{identity.entity.aliases.<accessor>.metadata.service_account_name}}/+\" { capabilities = [\"create\"] }",
    ),
    (
        KUBERNETES_EXECUTOR_POLICY,
        "path \"mandate/data/personal/fingerprints/x{{identity.entity.aliases.<accessor>.metadata.service_account_name}}/+\" { capabilities = [\"create\"] }",
    ),
];

/// The Kubernetes-auth executors' templated policy, which `cell up`'s provisioning writes once the
/// role provisioner runs: its two one-segment `create` grants below the executor's own templated
/// connection segment, its account entries (DEC-2339 item 2) and its fingerprint entries (DEC-2345
/// item 3), are in [`VAULT_DRILL_PREFIX_GRANTS`] with the mount's accessor as `<accessor>`.
const KUBERNETES_EXECUTOR_POLICY: &str = "mandate-kubernetes-executor";

/// What a templated policy's alias reference holds in place of the Kubernetes mount's accessor,
/// which OpenBao names `auth_kubernetes_<hex>` afresh for each vault ([`normalised_accessor`]).
const ACCESSOR_PLACEHOLDER: &str = "<accessor>";

/// The role provisioner's policy (DEC-2350), as `mandate vault provision --secrets-namespace`
/// writes it once `cell up` provisions for the provisioner ([`provision_vault`]).
const PROVISIONER_POLICY: &str = "mandate-provisioner";
/// Its stanza on the executors' Kubernetes-auth roles, as the scan prints it: create and update, a
/// lease's TTL, the executor's maximum, bound to the workspace's namespace.
const PROVISIONER_EXECUTOR_ROLES: &str = "path \"auth/kubernetes-executor/role/+\" { capabilities = [\"create\", \"update\"] required_parameters = [\"alias_name_source\", \"audience\", \"bound_service_account_names\", \"bound_service_account_namespaces\", \"token_max_ttl\", \"token_no_default_policy\", \"token_policies\", \"token_ttl\"] allowed_parameters = { \"alias_name_source\" = [\"serviceaccount_uid\"] \"audience\" = [\"mandate-vault\"] \"bound_service_account_names\" = [] \"bound_service_account_namespaces\" = [\"ws-ws1\"] \"token_max_ttl\" = [\"2592000\"] \"token_no_default_policy\" = [\"false\"] \"token_policies\" = [\"mandate-kubernetes-executor\"] \"token_ttl\" = [\"86400\"] } }";
/// Its stanza on the token exchanges' roles: as the executors', with delete, and a grant's TTL.
const PROVISIONER_EXCHANGE_ROLES: &str = "path \"auth/kubernetes-exchange/role/+\" { capabilities = [\"create\", \"update\", \"delete\"] required_parameters = [\"alias_name_source\", \"audience\", \"bound_service_account_names\", \"bound_service_account_namespaces\", \"token_max_ttl\", \"token_no_default_policy\", \"token_policies\", \"token_ttl\"] allowed_parameters = { \"alias_name_source\" = [\"serviceaccount_uid\"] \"audience\" = [\"mandate-vault\"] \"bound_service_account_names\" = [] \"bound_service_account_namespaces\" = [\"ws-ws1\"] \"token_max_ttl\" = [\"600\"] \"token_no_default_policy\" = [\"false\"] \"token_policies\" = [\"mandate-kubernetes-exchange\"] \"token_ttl\" = [\"600\"] } }";

/// Whether a scan line `wildcard in <policy>: <stanza>` names one of [`VAULT_DRILL_PREFIX_GRANTS`]
/// in its own policy, the stanza's mount accessors made [`ACCESSOR_PLACEHOLDER`] first and nothing
/// else changed. A stanza that already holds the placeholder is never admitted, so only a real
/// accessor can stand for it.
fn admitted_wildcard(line: &str) -> bool {
    line.strip_prefix("wildcard in ")
        .and_then(|rest| rest.split_once(": "))
        .is_some_and(|(policy, stanza)| {
            if stanza.contains(ACCESSOR_PLACEHOLDER) {
                return false;
            }
            let stanza = normalised_accessor(stanza);
            VAULT_DRILL_PREFIX_GRANTS.contains(&(policy, stanza.as_str()))
        })
}

/// `stanza` with each `aliases.auth_kubernetes_<hex>.metadata`, the alias reference a templated
/// policy keys its ServiceAccount by, `<hex>` one or more lowercase hexadecimal digits, made
/// `aliases.<accessor>.metadata`. Nothing else is touched: another accessor's shape, another
/// mount type, or the reference anywhere but between `aliases.` and `.metadata` stays as it is,
/// so it matches no grant.
fn normalised_accessor(stanza: &str) -> String {
    const BEFORE: &str = "aliases.auth_kubernetes_";
    const AFTER: &str = ".metadata";
    let mut out = String::new();
    let mut rest = stanza;
    while let Some(at) = rest.find(BEFORE) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let after_prefix = &tail[BEFORE.len()..];
        let hex = after_prefix
            .bytes()
            .take_while(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            .count();
        if hex > 0 && after_prefix[hex..].starts_with(AFTER) {
            out.push_str("aliases.");
            out.push_str(ACCESSOR_PLACEHOLDER);
            rest = &after_prefix[hex..];
        } else {
            out.push_str(BEFORE);
            rest = after_prefix;
        }
    }
    out.push_str(rest);
    out
}

/// The `mandate-*` policies `cell up`'s own provisioning adds once the role provisioner runs
/// ([`provision_vault`]): the provisioner's, and the two Kubernetes-auth kinds', the executor's and
/// the token exchange's (DEC-2350 item 1, DEC-2062).
const PROVISIONED_POLICIES: usize = 3;

/// How many `mandate-*` policies the vault drill must read: [`VAULT_DRILL_POLICIES`], and
/// [`PROVISIONED_POLICIES`] more where `cell up` provisioned for the role provisioner.
fn vault_drill_policies(provisioned: bool) -> usize {
    VAULT_DRILL_POLICIES + if provisioned { PROVISIONED_POLICIES } else { 0 }
}

/// The container of the vault drill's Job that logs in and reads; only its lines are judged, as
/// `kubectl logs --prefix` writes them (see [`scenario::lines_of`]).
const VAULT_DRILL_PROBE: &str = "probe";

/// What a vault drill's check lines must say: exactly one `<role> <path> allowed` or `denied`
/// line for each of [`VAULT_DRILL_READS`], as the table has it, no `wildcard in` line but those of
/// [`VAULT_DRILL_PREFIX_GRANTS`], and exactly one `checked <n> mandate policies` line with
/// `policies` ([`vault_drill_policies`]). A missing line, a second line for the same read, and a
/// scan of fewer policies are problems too.
fn vault_drill_problems(logs: &str, policies: usize) -> Vec<String> {
    let said = scenario::lines_of(logs, VAULT_DRILL_PROBE);
    let mut problems: Vec<String> = said
        .iter()
        .filter(|l| l.starts_with("wildcard in ") && !admitted_wildcard(l))
        .map(|l| l.to_string())
        .collect();
    for (role, path, allowed) in VAULT_DRILL_READS {
        let want = format!(
            "{role} {path} {}",
            if allowed { "allowed" } else { "denied" }
        );
        let answers: Vec<&str> = said
            .iter()
            .filter(|l| l.starts_with(&format!("{role} {path} ")))
            .copied()
            .collect();
        if answers != [want.as_str()] {
            let got = if answers.is_empty() {
                "no answer".to_owned()
            } else {
                answers.join("` and `")
            };
            problems.push(format!("want `{want}`, got `{got}`"));
        }
    }
    let scanned: Vec<&str> = said
        .iter()
        .filter(|l| l.starts_with("checked "))
        .copied()
        .collect();
    let want = format!("checked {policies} mandate policies");
    if scanned != [want.as_str()] {
        problems.push(format!(
            "want `{want}`, got `{}`",
            if scanned.is_empty() {
                "no scan".to_owned()
            } else {
                scanned.join("` and `")
            }
        ));
    }
    problems
}

/// The drill's script, run by the vault's own image: it puts a fake value at each path tried,
/// as the operator, then logs in as each role from the files `mandate vault provision` wrote and
/// prints one line per read, and one per stanza of a policy of the cell's that holds `*` or `+`,
/// whole and whitespace squeezed ([`vault_drill_scan`]), for [`vault_drill_problems`] to judge against
/// [`VAULT_DRILL_PREFIX_GRANTS`]. Seeded, it
/// first rewrites the runtime's policy to read every connection's token through `+`, and puts the
/// policy it read back when the script exits, printing what the restore says so a failed one is
/// seen. Each read first looks its token up, so a token the vault does not honour is an `error`,
/// never a denial. The scan counts the `mandate-*` policies it reads, and a failed read ends the
/// script. Each read is `bao read` of the KV v2 data path, not `bao kv get`, whose preflight on the
/// mount fails without `permission denied` for a role granted nothing under it, as the runtime is.
fn vault_drill_script(seeded: bool) -> String {
    let mut reads = String::new();
    for (role, path, _) in VAULT_DRILL_READS {
        reads.push_str(&format!("check {role} {path}\n"));
    }
    let mut puts = String::new();
    for path in [
        "connections/x64656d6f2d7061706572/token",
        "connections/x6472696c6c2d6f74686572/token",
        "connections/x64656d6f2d7061706572/code",
        "platform/alpaca-oauth-client-secret",
        "platform/model-provider-key",
        "platform/email-credential",
    ] {
        puts.push_str(&format!(
            "BAO_TOKEN=\"$op\" bao kv put -mount=mandate {path} value=ZHJpbGwtZmFrZQ== >/dev/null\n"
        ));
    }
    let plant = if seeded {
        "orig=$(BAO_TOKEN=\"$op\" bao policy read mandate-runtime)\n\
         trap 'printf \"%s\\n\" \"$orig\" | BAO_TOKEN=\"$op\" bao policy write mandate-runtime -' EXIT\n\
         printf 'path \"mandate/data/connections/+/token\" {\\n  capabilities = [\"read\"]\\n}\\n' \
         | BAO_TOKEN=\"$op\" bao policy write mandate-runtime - >/dev/null\n"
    } else {
        ""
    };
    format!(
        "set -eu\n\
         op=$(cat /work/token)\n\
         {puts}{plant}\
         check() {{\n\
           t=$(bao write -field=token auth/approle/login role_id=@/work/private/creds/$1.role-id secret_id=@/work/private/creds/$1.secret-id)\n\
           if ! BAO_TOKEN=\"$t\" bao token lookup >/dev/null 2>&1; then echo \"$1 $2 error\"; return 0; fi\n\
           if out=$(BAO_TOKEN=\"$t\" bao read -field=data \"mandate/data/$2\" 2>&1 >/dev/null); then echo \"$1 $2 allowed\"\n\
           elif echo \"$out\" | grep -q 'permission denied'; then echo \"$1 $2 denied\"\n\
           else echo \"$1 $2 error\"; fi\n\
         }}\n\
         {reads}{scan}",
        scan = vault_drill_scan(),
    )
}

/// The vault drill's policy scan, run with the operator's token in `$op`: each `mandate-*`
/// policy's text cut into whole stanzas, each ending at a line that is `}` alone, as every
/// policy `mandate vault provision` writes ends its stanzas, so a nested block's brace ends none;
/// whatever follows the last such line, or a policy with none, is one more piece, so text the cut
/// misses is still judged. Each piece, whitespace squeezed, that holds `*` or `+` is one
/// `wildcard in <name>: <stanza>` line; then `checked <n> mandate policies`. A failed read ends
/// the script, under its `set -eu`.
fn vault_drill_scan() -> String {
    "n=0\n\
     for p in $(BAO_TOKEN=\"$op\" bao policy list); do\n\
       case \"$p\" in mandate-*)\n\
         body=$(BAO_TOKEN=\"$op\" bao policy read \"$p\")\n\
         n=$((n + 1))\n\
         printf '%s\\n' \"$body\" | awk -v p=\"$p\" 'function judge(r) { gsub(/[ \\t]+/, \" \", r); sub(/^ /, \"\", r); sub(/ $/, \"\", r); if (r ~ /[*+]/) print \"wildcard in \" p \": \" r } { rec = rec \" \" $0 } /^}$/ { judge(rec); rec = \"\" } END { judge(rec) }';;\n\
       esac\n\
     done\n\
     echo \"checked $n mandate policies\"\n"
        .to_owned()
}

/// The vault drill (E21-32, DEC-2022 item 3). A Job labelled as the executor, so its egress
/// reaches the vault and the vault admits it: the vault's image copies the cell's fake operator
/// token into a file only the pod's user reads and makes `/work/private` owner-only, since an
/// emptyDir is 0777 without the sticky bit and `--out`'s parent must not be; the cell's `mandate`
/// image, the one `images.mandate` names, runs `mandate vault provision`, which writes every role's
/// credentials, and the vault's image then logs in as each and reads (see
/// [`vault_drill_script`]). The operator token is the local cell's fake root token, put in a
/// Secret for the drill and deleted after it.
fn vault_drill(seeded: bool) -> Result<Verdict> {
    let init = fs::read_to_string(state()?.join(VAULT_INIT))
        .context("the vault drill needs the cell's vault-init.json; run `cargo xtask cell up`")?;
    let init: Value = serde_json::from_str(&init).context("reading the vault's init")?;
    let root = init["root_token"]
        .as_str()
        .context("the vault's init holds no root token")?;
    let secret = json!({
        "apiVersion": "v1", "kind": "Secret",
        "metadata": {"name": "vault-drill-operator"},
        "type": "Opaque",
        "stringData": {"token": root}
    });
    let mandate = image_tag()?;
    let bao = kubectl(&[
        "get",
        "statefulset",
        "vault",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.spec.template.spec.containers[0].image}",
    ])?;
    let executor = processes()?
        .into_iter()
        .find(|p| p["name"] == "executor")
        .context("processes.json has no executor")?;
    let id = run_id()?;
    let mut pod = probe_pod(&executor, &id, bao.trim(), &vault_drill_script(seeded))?;
    let hardened = pod["spec"]["containers"][0]["securityContext"].clone();
    let env = json!([
        {"name": "BAO_ADDR", "value": format!("https://vault.{NAMESPACE}.svc:8200")},
        {"name": "BAO_CACERT", "value": "/ca/ca.crt"},
        {"name": "HOME", "value": "/tmp"}
    ]);
    let mounts = json!([
        {"name": "work", "mountPath": "/work"},
        {"name": "ca", "mountPath": "/ca", "readOnly": true},
        {"name": "tmp", "mountPath": "/tmp"}
    ]);
    let small = json!({"limits": {"cpu": "100m", "memory": "64Mi"}, "requests": {"cpu": "50m", "memory": "32Mi"}});
    let mut provision = vec![
        "vault".to_owned(),
        "provision".to_owned(),
        "--address".to_owned(),
        format!("https://vault.{NAMESPACE}.svc:8200"),
        "--ca".to_owned(),
        "/ca/ca.crt".to_owned(),
        "--operator-token-file".to_owned(),
        "/work/token".to_owned(),
        "--out".to_owned(),
        "/work/private/creds".to_owned(),
    ];
    for c in VAULT_DRILL_CONNECTIONS {
        provision.extend(["--connection".to_owned(), c.to_owned()]);
    }
    let spec = &mut pod["spec"];
    spec["securityContext"]["fsGroup"] = json!(65532);
    spec["initContainers"] = json!([
        {
            "name": "operator-token",
            "image": bao.trim(),
            "command": ["sh", "-c", "umask 077 && mkdir /work/private && cp /operator/token /work/token"],
            "securityContext": hardened, "resources": small,
            "volumeMounts": [{"name": "work", "mountPath": "/work"}, {"name": "operator", "mountPath": "/operator", "readOnly": true}]
        },
        {
            "name": "provision",
            "image": mandate,
            "args": provision,
            "securityContext": hardened, "resources": small,
            "volumeMounts": mounts
        }
    ]);
    spec["containers"][0]["env"] = env;
    spec["containers"][0]["command"] = json!(["sh", "-c", vault_drill_script(seeded)]);
    spec["containers"][0]["volumeMounts"] = mounts;
    spec["volumes"] = json!([
        {"name": "work", "emptyDir": {}},
        {"name": "tmp", "emptyDir": {}},
        {"name": "ca", "configMap": {"name": "vault-ca"}},
        {"name": "operator", "secret": {"secretName": "vault-drill-operator", "defaultMode": 0o440}}
    ]);
    let template = json!({"metadata": {"labels": pod["metadata"]["labels"].clone()}, "spec": pod["spec"].clone()});
    let job = json!({
        "apiVersion": "batch/v1", "kind": "Job",
        "metadata": {"name": format!("vault-drill-{id}")},
        "spec": {"backoffLimit": 0, "template": template}
    });
    let ran = run_job(vec![secret, job]);
    let _ = kubectl(&[
        "delete",
        "secret",
        "vault-drill-operator",
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
    ]);
    let (finished, logs) = ran?;
    if !finished {
        return Ok(Verdict::Broken(format!(
            "the vault drill's Job failed:\n{logs}"
        )));
    }
    let problems = vault_drill_problems(
        &logs,
        vault_drill_policies(role_provisioner_image_tag()?.is_some()),
    );
    if problems.is_empty() {
        Ok(Verdict::Held)
    } else {
        Ok(Verdict::Broken(problems.join("\n")))
    }
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
    let images = image_sets()?;
    let json_sets: Vec<String> = egress_sets()?
        .into_iter()
        .chain(sets.iter().cloned())
        .collect();
    render_with(&template_args(
        NAMESPACE,
        &[WORKSPACE_VALUES],
        &images,
        &json_sets,
        show_only,
    ))
}

/// `helm template`'s arguments for the workspace chart in `namespace`: each values file in order,
/// each of `sets` as `--set` and each of `json_sets` as `--set-json`, and `show_only` when given.
/// Pure, so a dry run's render is checked without a cluster.
pub fn template_args(
    namespace: &str,
    values: &[&str],
    sets: &[String],
    json_sets: &[String],
    show_only: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "template",
        "workspace",
        WORKSPACE_CHART,
        "--namespace",
        namespace,
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    for file in values {
        args.extend(["-f".to_string(), file.to_string()]);
    }
    for set in sets {
        args.extend(["--set".to_string(), set.clone()]);
    }
    for set in json_sets {
        args.extend(["--set-json".to_string(), set.clone()]);
    }
    if let Some(template) = show_only {
        args.extend(["--show-only".to_string(), template.to_string()]);
    }
    args
}

/// The chart rendered by `helm` with `args` ([`template_args`]): `helm template` alone, which
/// reads no kubeconfig and reaches no cluster.
pub fn render_with(args: &[String]) -> Result<Vec<Value>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
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
            Drill::KillSwitch => kill_switch(seeded),
            Drill::Admission => admission_drill::drill(seeded),
            Drill::StandbyDown => standby_down(seeded),
            Drill::Archive => archive(seeded),
            Drill::Ticks => executor::ticks_drill(seeded),
            Drill::Executor => executor::drill(seeded),
            Drill::Vault => vault_drill(seeded),
            Drill::Exchange => exchange::drill(seeded, false),
            Drill::ExchangeLate => exchange::drill(seeded, true),
        };
        if seeded && matches!(d, Drill::Boot | Drill::Journal | Drill::Egress) {
            restore()?;
        }
        match (verdict?, seeded) {
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
    run_job_watching(objects, || Ok(()))
}

/// [`run_job`], calling `watch` at each poll while the Job runs, as a drill that must see what the
/// database does during the Job.
fn run_job_watching(
    objects: Vec<Value>,
    mut watch: impl FnMut() -> Result<()>,
) -> Result<(bool, String)> {
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
            "jsonpath={.status.succeeded}/{.status.failed}/{.status.conditions[*].type} {.status.conditions[*].reason}",
        ])?;
        let mut fields = status.trim().splitn(3, '/');
        let succeeded = fields.next().unwrap_or_default();
        let failed = fields.next().unwrap_or_default();
        let conditions = fields.next().unwrap_or_default();
        if succeeded == "1" {
            return Ok((true, job_logs(&name, job, "")));
        }
        let failing = conditions
            .split_whitespace()
            .any(|c| c == "FailureTarget" || c == "DeadlineExceeded");
        if failed == "1" || failing {
            let report = failure_report(&name);
            return Ok((false, job_logs(&name, job, &report)));
        }
        watch()?;
        sleep(Duration::from_secs(2));
    }
    bail!("Job {name} neither succeeded nor failed in time")
}

/// Every container's logs of a finished Job. `--all-containers` refuses a pod with a container that
/// never started, as when an init container failed, so it falls back to each container in turn,
/// keeping the failed init container's report, which is where a seeded symptom is. With no logs at
/// all, as when the deadline deleted the pod, `report` from [`failure_report`] says why instead.
fn job_logs(name: &str, job: &Value, report: &str) -> String {
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
        format!("(no logs: {refused:#})\n{report}")
    } else {
        logs
    }
}

/// Seconds each diagnostic `kubectl get` may take, so a failed Job's report costs seconds, not
/// minutes, of a drill's budget (DEC-464).
const REPORT_TIMEOUT: &str = "--request-timeout=5s";

/// What a failing Job's status, pods, and events say, read before its logs while its pod may still
/// exist: a Job its deadline stopped has its pod deleted, and then `kubectl logs` says nothing.
fn failure_report(name: &str) -> String {
    let selector = format!("job-name={name}");
    let get = |args: &[&str]| -> Result<Value> {
        let mut full = vec!["get"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--namespace", NAMESPACE, REPORT_TIMEOUT, "-o", "json"]);
        Ok(serde_json::from_str(&kubectl(&full)?)?)
    };
    let mut unread = Vec::new();
    let job = get(&["job", name]).unwrap_or_else(|e| {
        unread.push(format!("job: {e:#}"));
        Value::Null
    });
    let pods = get(&["pods", "-l", &selector]).unwrap_or_else(|e| {
        unread.push(format!("pods: {e:#}"));
        Value::Null
    });
    let pod_names: Vec<String> = items(&pods)
        .iter()
        .filter_map(|p| p["metadata"]["name"].as_str())
        .map(String::from)
        .collect();
    let mut events = Vec::new();
    for involved in std::iter::once(name.to_string()).chain(pod_names) {
        let field = format!("involvedObject.name={involved}");
        match get(&["events", "--field-selector", &field]) {
            Ok(list) => events.extend(items(&list).iter().cloned()),
            Err(e) => unread.push(format!("events of {involved}: {e:#}")),
        }
    }
    let mut report = describe_failure(&job, &pods, &events);
    for line in unread {
        report.push_str(&format!("unread {}\n", clipped(&line)));
    }
    report
}

/// A list's `items`, or none.
fn items(list: &Value) -> &[Value] {
    list["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// The longest free text, in characters, one line of a failure report quotes.
const REPORT_TEXT: usize = 160;
/// The most pods, and the most of the latest events, a failure report lists.
const REPORT_PODS: usize = 3;
const REPORT_EVENTS: usize = 8;

/// `text` on one line, cut to [`REPORT_TEXT`] characters.
fn clipped(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= REPORT_TEXT {
        line
    } else {
        format!("{}...", line.chars().take(REPORT_TEXT).collect::<String>())
    }
}

/// `type=status (reason: message)` for each of an object's conditions.
fn conditions_of(status: &Value) -> String {
    let all: Vec<String> = status["conditions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|c| {
            let why = [&c["reason"], &c["message"]]
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(": ");
            let head = format!("{}={}", text_of(&c["type"]), text_of(&c["status"]));
            if why.is_empty() {
                head
            } else {
                format!("{head} ({})", clipped(&why))
            }
        })
        .collect();
    if all.is_empty() {
        "none".into()
    } else {
        all.join("; ")
    }
}

/// A string or number field as text, `?` when absent.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "?".into(),
        other => other.to_string(),
    }
}

/// One container state (`waiting`, `running`, or `terminated`) as its reason, exit code, and message.
fn state_of(state: &Value) -> String {
    let Some((kind, detail)) = state.as_object().and_then(|o| o.iter().next()) else {
        return "none".into();
    };
    let mut parts = vec![kind.clone()];
    if let Some(reason) = detail["reason"].as_str() {
        parts.push(reason.into());
    }
    if let Some(code) = detail["exitCode"].as_i64() {
        parts.push(format!("exit {code}"));
    }
    if let Some(at) = detail["startedAt"].as_str() {
        parts.push(format!("since {at}"));
    }
    if let Some(message) = detail["message"].as_str() {
        parts.push(clipped(message));
    }
    parts.join(", ")
}

/// A failing Job's report from its `kubectl get -o json` object, its pods' list, and the events on
/// either: the Job's counts and conditions, each pod's phase, conditions, and every container's
/// state and last state, then the latest events. It names fields it reads, never a pod's spec, so
/// no env value is shown, and each count and text is bounded so a report stays a few lines.
fn describe_failure(job: &Value, pods: &Value, events: &[Value]) -> String {
    let status = &job["status"];
    let mut out = format!(
        "job: active={} failed={} conditions: {}\n",
        text_of(&status["active"]),
        text_of(&status["failed"]),
        conditions_of(status)
    );
    let pods = items(pods);
    if pods.is_empty() {
        out.push_str("pods: none\n");
    }
    for pod in pods.iter().take(REPORT_PODS) {
        out.push_str(&format!(
            "pod {}: phase={} conditions: {}\n",
            text_of(&pod["metadata"]["name"]),
            text_of(&pod["status"]["phase"]),
            conditions_of(&pod["status"])
        ));
        for key in ["initContainerStatuses", "containerStatuses"] {
            for c in pod["status"][key]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                out.push_str(&format!(
                    "  {} {}: {}; last: {}\n",
                    if key == "initContainerStatuses" {
                        "init"
                    } else {
                        "container"
                    },
                    text_of(&c["name"]),
                    state_of(&c["state"]),
                    state_of(&c["lastState"])
                ));
            }
        }
    }
    if pods.len() > REPORT_PODS {
        out.push_str(&format!("({} more pods)\n", pods.len() - REPORT_PODS));
    }
    let latest = &events[events.len().saturating_sub(REPORT_EVENTS)..];
    if latest.is_empty() {
        out.push_str("events: none\n");
    }
    for e in latest {
        out.push_str(&format!(
            "event {} {} {}/{} x{}: {}\n",
            text_of(&e["type"]),
            text_of(&e["reason"]),
            text_of(&e["involvedObject"]["kind"]),
            text_of(&e["involvedObject"]["name"]),
            e["count"].as_u64().unwrap_or(1),
            clipped(e["message"].as_str().unwrap_or_default())
        ));
    }
    out
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
/// would; the deployment manager's probe waits behind the manager's own gate, since its policy
/// allows the API server every other gate waits on (DEC-1172). Seeded, the policies are gone and
/// the gate would never open, so the probe goes without it to show it can see a leak.
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
    let gate_of = |kind: &str, name: &str| {
        rendered
            .iter()
            .find(|o| o["kind"] == kind && o["metadata"]["name"] == name)
            .map(|w| w["spec"]["template"]["spec"]["initContainers"][0].clone())
            .filter(|c| c["name"] == "egress-gate")
    };
    let gate = gate_of("Job", "migrate").context("the migration Job renders no egress gate")?;
    let manager_gate = gate_of("Deployment", DEPLOY_MANAGER_PROCESS);
    let id = run_id()?;
    let all = processes()?;
    let pods = all
        .iter()
        .map(|p| {
            let mut pod = probe_pod(p, &id, &image, &script)?;
            if !seeded {
                let own = manager_gate
                    .as_ref()
                    .filter(|_| p["name"] == DEPLOY_MANAGER_PROCESS);
                pod["spec"]["initContainers"] = json!([own.unwrap_or(&gate).clone()]);
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
    for p in all.iter().filter(|p| in_compose(p)) {
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
                if let Some(args) = p["args"].as_array() {
                    service["command"] = json!(compose_words(args, "")?);
                }
                if let Some(probe) = p["probe"].as_array() {
                    let mut test = vec![json!("CMD")];
                    test.extend(compose_words(probe, "readyz")?.into_iter().map(Value::from));
                    service["healthcheck"] = json!({"test": test, "interval": "5s", "retries": 6});
                }
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

/// Whether process `p` is in the Compose file: built, and not one whose `compose` is false, which
/// has no Compose adapter (the deployment manager drives Kubernetes only, DEC-1172).
fn in_compose(p: &Value) -> bool {
    p["built"] == true && p["compose"] != false
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

/// A service's `args` or `probe` from processes.json for Compose (DEC-1013): `{workspace}`, and
/// `{hosted_workspace}`, which is the same one workspace there, and the `runtime` values'
/// placeholders become the variables the operator sets, each required, and `{probe}` becomes
/// `probe`.
fn compose_words(words: &[Value], probe: &str) -> Result<Vec<String>> {
    words
        .iter()
        .map(|w| {
            let word = w.as_str().context("a launch word is not a string")?;
            Ok(word
                .replace("{workspace}", "${MANDATE_WORKSPACE:?set the workspace}")
                .replace(
                    "{hosted_workspace}",
                    "${MANDATE_WORKSPACE:?set the workspace}",
                )
                .replace(
                    "{agent}",
                    "${MANDATE_RUNTIME_AGENT:?set the runtime's agent}",
                )
                .replace(
                    "{connection}",
                    "${MANDATE_RUNTIME_CONNECTION:?set the runtime's connection}",
                )
                .replace(
                    "{owner}",
                    "${MANDATE_RUNTIME_OWNER:?set the runtime's owner}",
                )
                .replace("{probe}", probe))
        })
        .collect()
}

/// The runtime image Compose is given when this cell built none (its `mandate` image is pulled):
/// the drill never starts the runtime, but Compose resolves every service's variables first.
const RUNTIME_NOT_BUILT: &str = "mandate.local/runtime:0000000";

/// The variables the Compose file requires (DEC-1013): the workspace, the `mandate` image, the
/// API server's settings ([`compose_api_settings`]), the runtime's image, and the runtime's
/// deployment, read from the local cell's values so Compose and the k3d cell run one demo
/// deployment.
fn compose_env(
    root: &Path,
    image: &str,
    api: &str,
    runtime: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let mut env = vec![
        ("MANDATE_WORKSPACE".to_string(), WORKSPACE.to_string()),
        ("MANDATE_IMAGE_MANDATE".to_string(), image.to_string()),
        (
            "MANDATE_IMAGE_RUNTIME".to_string(),
            runtime.unwrap_or(RUNTIME_NOT_BUILT).to_string(),
        ),
    ];
    env.extend(
        compose_api_settings(api)
            .into_iter()
            .map(|(name, value)| (name.to_string(), value)),
    );
    for key in ["agent", "connection", "owner"] {
        env.push((
            format!("MANDATE_RUNTIME_{}", key.to_uppercase()),
            runtime_value(root, key)?,
        ));
    }
    Ok(env)
}

/// One of the local cell's `runtime:` values (`deploy/cell/values/k3d.yaml`).
fn runtime_value(root: &Path, key: &str) -> Result<String> {
    let text = fs::read_to_string(root.join(WORKSPACE_VALUES))
        .with_context(|| format!("reading {WORKSPACE_VALUES}"))?;
    let prefix = format!("  {key}: ");
    text.lines()
        .skip_while(|l| *l != "runtime:")
        .take_while(|l| l.starts_with(' ') || *l == "runtime:")
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .with_context(|| format!("{WORKSPACE_VALUES} names no `runtime.{key}`"))
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

/// What the journal cluster says of itself, each instance's role and readiness, and the last lines
/// of every container of an instance that is not Ready (the init Job's too), for a drill's report.
/// Only the local cell's paper journal is ever read, so a statement in those lines leaves nothing.
fn cluster_state() -> String {
    let status = kubectl(&[
        "get",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.status.phase} ({.status.phaseReason}); primary {.status.currentPrimary}, target {.status.targetPrimary}",
    ])
    .unwrap_or_default();
    let pods = kubectl(&[
        "get",
        "pod",
        "--namespace",
        NAMESPACE,
        "--selector",
        "cnpg.io/cluster=journal",
        "-o",
        "jsonpath={range .items[*]}{.metadata.name}|{.metadata.labels.cnpg\\.io/instanceRole}|{.status.conditions[?(@.type==\"Ready\")].status}{\"\\n\"}{end}",
    ])
    .unwrap_or_default();
    let mut report = format!("{}; instances:", status.trim());
    for line in pods.lines().filter(|l| !l.is_empty()) {
        report.push_str(&format!(" {line};"));
        let mut fields = line.split('|');
        let name = fields.next().unwrap_or_default();
        if fields.nth(1) != Some("True") {
            let tail = kubectl(&[
                "logs",
                name,
                "--namespace",
                NAMESPACE,
                "--all-containers",
                "--prefix",
                "--tail=15",
            ])
            .unwrap_or_default();
            report.push_str(&format!("\n[{name}]\n{tail}"));
        }
    }
    report
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
    if kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        "cluster.postgresql.cnpg.io/journal",
        "--timeout=300s",
    ])
    .is_err()
    {
        return Ok(Verdict::Broken(format!(
            "the journal cluster was not Ready within 300s of its primary's restart: {}",
            cluster_state()
        )));
    }
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

/// The processes the kill-switch drill takes down before it brakes (OPS-4, E21-34): none of them
/// may be needed for a kill switch or a pause to commit. A name the chart does not render yet has
/// nothing to take down.
pub const KILL_SWITCH_DOWN: [&str; 5] = [
    "api",
    "deploy-manager",
    "exporter",
    "model-gateway",
    "control-plane",
];

/// The agent and the connection the kill-switch drill brakes.
const BRAKE_AGENT: &str = "agent-a";
const BRAKE_CONNECTION: &str = "conn_alpaca_paper_01";

/// The owner commands a brake run must leave on its control stream, as `(command, scope,
/// subject)`: each scope's kill switch and an agent pause.
pub fn brakes_expected(workspace: &str) -> Vec<(String, String, String)> {
    [
        ("kill_switch", "agent", BRAKE_AGENT),
        ("kill_switch", "connection", BRAKE_CONNECTION),
        ("kill_switch", "workspace", workspace),
        ("pause", "agent", BRAKE_AGENT),
    ]
    .into_iter()
    .map(|(c, k, s)| (c.to_owned(), k.to_owned(), s.to_owned()))
    .collect()
}

/// The CLI arguments of each brake [`brakes_expected`] names, for `workspace` and `journal`.
fn brake_args(workspace: &str, journal: &str) -> Vec<Vec<String>> {
    let tail = [
        "--workspace",
        workspace,
        "--user",
        "drill-owner",
        "--journal",
        journal,
        "--store",
        "/work/store",
    ];
    [
        vec!["agent", "kill", "--agent", BRAKE_AGENT],
        vec!["agent", "kill", "--connection", BRAKE_CONNECTION],
        vec!["agent", "kill", "--workspace-wide"],
        vec!["agent", "pause", "--agent", BRAKE_AGENT],
    ]
    .into_iter()
    .map(|head| head.into_iter().chain(tail).map(String::from).collect())
    .collect()
}

/// Which brakes of `expected` the exported segment lines in `logs` do not hold, as one report, or
/// `None` when every one is there. Each line of an export is `{"body":<event>,"hash":…}`, read from
/// its first `{`, so the container prefix [`job_logs`] puts on each log line is skipped; any other
/// line is ignored.
pub fn brakes_missing(logs: &str, expected: &[(String, String, String)]) -> Option<String> {
    let mut seen = Vec::new();
    for line in logs.lines() {
        let Some(start) = line.find('{') else {
            continue;
        };
        let Ok(line) = serde_json::from_str::<Value>(line[start..].trim()) else {
            continue;
        };
        let body = &line["body"];
        if body["event_type"] != "OwnerCommandIssued" {
            continue;
        }
        let field = |k: &str| body["payload"][k].as_str().unwrap_or_default().to_owned();
        seen.push((field("command"), field("scope"), field("subject")));
    }
    let missing: Vec<String> = expected
        .iter()
        .filter(|e| !seen.contains(e))
        .map(|(command, scope, subject)| format!("no {command} of scope {scope} ({subject})"))
        .collect();
    (!missing.is_empty()).then(|| missing.join("; "))
}

/// The kill-switch drill (OPS-4, E21-34): takes down every [`KILL_SWITCH_DOWN`] process the chart
/// runs, opens a control stream, commits each scope's kill switch and an agent pause through
/// one-off control Jobs, then exports the stream, verifies it by journal spec §11, and checks
/// every brake is in it; the processes it took down are scaled back after. Seeded, it never
/// commits the connection-scope kill switch, which the check must name.
fn kill_switch(seeded: bool) -> Result<Verdict> {
    let selector = format!("mandate.dev/process in ({})", KILL_SWITCH_DOWN.join(","));
    let downed = kubectl(&[
        "get",
        "deployment",
        "--namespace",
        NAMESPACE,
        "--selector",
        &selector,
        "-o",
        "jsonpath={range .items[*]}{.metadata.name} {.spec.replicas}{\"\\n\"}{end}",
    ])?;
    let downed: Vec<(String, String)> = downed
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(n, r)| (n.to_owned(), r.to_owned()))
        .collect();
    let scaled_down = downed.iter().try_for_each(|(name, _)| {
        kubectl(&[
            "scale",
            "deployment",
            name,
            "--namespace",
            NAMESPACE,
            "--replicas=0",
        ])
        .map(drop)
    });
    let verdict = scaled_down.and_then(|()| brake_in_cell(&selector, seeded));
    let unrestored: Vec<String> = downed
        .iter()
        .filter_map(|(name, replicas)| {
            let replicas = format!("--replicas={replicas}");
            kubectl(&[
                "scale",
                "deployment",
                name,
                "--namespace",
                NAMESPACE,
                &replicas,
            ])
            .err()
            .map(|e| format!("{name}: {e:#}"))
        })
        .collect();
    if unrestored.is_empty() {
        verdict
    } else {
        bail!(
            "the drill's verdict was {verdict:?}, but these processes were not scaled back: {}",
            unrestored.join("; ")
        )
    }
}

fn brake_in_cell(selector: &str, seeded: bool) -> Result<Verdict> {
    let mut gone = false;
    for _ in 0..POLLS {
        let pods = kubectl(&[
            "get",
            "pod",
            "--namespace",
            NAMESPACE,
            "--selector",
            selector,
            "-o",
            "name",
        ])?;
        if pods.trim().is_empty() {
            gone = true;
            break;
        }
        sleep(Duration::from_secs(2));
    }
    if !gone {
        return Ok(Verdict::Broken(format!(
            "a pod of {} was still in the cell",
            KILL_SWITCH_DOWN.join(", ")
        )));
    }
    let id = run_id()?;
    let workspace = format!("kill{id}");
    let journal = "$(MANDATE_JOURNAL_DSN)";
    let open = json!([
        "workspace",
        "open",
        "--workspace",
        workspace,
        "--journal",
        journal,
        "--store",
        "/work/store"
    ]);
    let mut runs = vec![open];
    for (n, args) in brake_args(&workspace, journal).into_iter().enumerate() {
        if seeded && n == 1 {
            continue;
        }
        runs.push(json!(args));
    }
    for (n, args) in runs.iter().enumerate() {
        let job = render(
            &[format!("run.id=\"{id}{n}\""), format!("run.args={args}")],
            Some("templates/run.yaml"),
        )?;
        let (done, logs) = run_job(job)?;
        if !done {
            return Ok(Verdict::Broken(format!(
                "`mandate {}` failed in the cell:\n{logs}",
                args.as_array()
                    .map(|a| a
                        .iter()
                        .take(3)
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "))
                    .unwrap_or_default()
            )));
        }
    }
    let mut verify = render(
        &[
            format!("verify.id=\"{id}\""),
            format!("verify.streams={}", json!([format!("ctl:{workspace}")])),
        ],
        Some("templates/run.yaml"),
    )?;
    show_segment(&mut verify)?;
    let (verified, logs) = run_job(verify)?;
    if !verified {
        return Ok(Verdict::Broken(format!(
            "the control stream holding the brakes did not verify:\n{logs}"
        )));
    }
    if let Some(missing) = brakes_missing(&logs, &brakes_expected(&workspace)) {
        return Ok(Verdict::Broken(missing));
    }
    let built = processes()?;
    let is_built = |name: &str| {
        built
            .iter()
            .any(|p| p["name"] == name && p["built"] == true)
    };
    let owners = if is_built("runtime") && is_built("executor") {
        "the runtime and executor are built, but nothing in the drill starts them for an agent yet"
    } else {
        "the runtime and executor do not run in the cell yet"
    };
    eprintln!(
        "    stream owners: skipped: {owners}, so each scope's KillSwitchActivated in every stream \
         owner waits on them and on the executor's connection and workspace scopes (E7-4; \
         DEC-2000 item 6)"
    );
    Ok(Verdict::Held)
}

/// Adds an init container after the exports that prints the first segment, so the Job's logs
/// carry the exported events for [`brakes_missing`].
fn show_segment(jobs: &mut [Value]) -> Result<()> {
    let job = jobs.first_mut().context("the verify Job did not render")?;
    let spec = &mut job["spec"]["template"]["spec"];
    let image = spec["initContainers"][0]["image"].clone();
    let show = json!({
        "name": "show-segment",
        "image": image,
        "command": ["cat", "/work/segment-0.jsonl"],
        "securityContext": spec["initContainers"][0]["securityContext"].clone(),
        "resources": spec["initContainers"][0]["resources"].clone(),
        "volumeMounts": [{"name": "work", "mountPath": "/work"}]
    });
    spec["initContainers"]
        .as_array_mut()
        .context("the verify Job has no init containers")?
        .push(show);
    Ok(())
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
        "SELECT count(*) FROM journal.events WHERE stream_id = '{stream}'"
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
const FENCED_APPEND_SECS: u32 = 60;

/// OPS-2 with the synchronous standby down: the standby is fenced (CloudNativePG stops its
/// Postgres), an append is tried, and it must wait on the standby rather than commit: the primary
/// shows a backend waiting for synchronous replication, the CLI never reports the commit, the
/// primary shows none of its events, and the Job ends on its deadline. Whatever happened, the
/// standby is then unfenced, the durability put back, and the cluster waited for until a
/// synchronous standby streams again; then it must take a new append, and every stream written
/// verifies. Seeded, the cluster's durability is `preferred` first, so the primary drops the
/// standby and commits alone, which the drill must report.
fn standby_down(seeded: bool) -> Result<Verdict> {
    if !until(|| Ok(synchronous_standbys()? == "1"))? {
        return Ok(Verdict::Broken(
            "before the drill, no synchronous standby streams to the primary".into(),
        ));
    }
    let fenced = if seeded {
        set_durability("preferred").and_then(|()| fenced_append(seeded))
    } else {
        fenced_append(seeded)
    };
    let restored = restore_standby(seeded);
    let (verdict, fenced) = fenced?;
    let streaming = restored?;
    if verdict != Verdict::Held {
        return Ok(verdict);
    }
    if !streaming {
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

/// Unfences the standby, puts `required` back after a seeded run, and waits for the cluster to be
/// Ready with a synchronous standby streaming, so the next drill starts from a whole cluster;
/// whether one streams again. Every step runs even when an earlier one failed.
fn restore_standby(seeded: bool) -> Result<bool> {
    let unfenced = kubectl(&[
        "annotate",
        "cluster.postgresql.cnpg.io/journal",
        "--namespace",
        NAMESPACE,
        "cnpg.io/fencedInstances-",
    ]);
    let durable = if seeded {
        set_durability("required")
    } else {
        Ok(())
    };
    let ready = kubectl(&[
        "wait",
        "--namespace",
        NAMESPACE,
        "--for=condition=Ready",
        "cluster.postgresql.cnpg.io/journal",
        "--timeout=300s",
    ]);
    let streaming = until(|| Ok(synchronous_standbys()? == "1"));
    unfenced?;
    durable?;
    ready?;
    streaming
}

/// Backends on the primary waiting for a standby to confirm their commit.
fn waiting_on_standby() -> Result<String> {
    on_primary("SELECT count(*) FROM pg_stat_activity WHERE wait_event = 'SyncRep'")
}

/// Fences the standby, waits until the primary has no synchronous standby, and tries one append,
/// watching the primary for the commit that waits on the standby; the verdict, and the stream the
/// append was for, which the drill checks once the standby is back.
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
    let mut waited = false;
    let (opened, logs) = run_job_watching(open_job(&id, &workspace, deadline)?, || {
        if !waited {
            waited = waiting_on_standby()? != "0";
        }
        Ok(())
    })?;
    let stored = stored_events(&stream)?;
    let verdict = if opened {
        Verdict::Broken(format!(
            "an append committed with the standby fenced: `mandate workspace open` succeeded, and the primary holds {stored} of its events"
        ))
    } else if stored != "0" {
        Verdict::Broken(format!(
            "an append committed with the standby fenced: the primary holds {stored} events of {stream}, though the CLI never had its commit\n{logs}"
        ))
    } else {
        let end = job_end(&format!("control-{id}"))?;
        if end != "DeadlineExceeded" {
            let reason = if end.is_empty() {
                "no terminal condition"
            } else {
                end.as_str()
            };
            Verdict::Broken(format!(
                "the append with the standby fenced failed (`{reason}`) rather than waited:\n{logs}"
            ))
        } else if !waited {
            Verdict::Broken(format!(
                "the append's Job ran out its deadline, but no commit ever waited on the standby, so the drill did not see what stopped it:\n{logs}"
            ))
        } else {
            Verdict::Held
        }
    };
    Ok((verdict, stream))
}

/// Why a Job that failed ended, as its `Failed` condition says, or its `FailureTarget` while the
/// controller is still stopping its pod (`DeadlineExceeded` for one its deadline stopped); waited
/// for, since a Job counts its failed pod before it sets either.
fn job_end(name: &str) -> Result<String> {
    let mut reason = String::new();
    until(|| {
        reason = kubectl(&[
            "get",
            "job",
            name,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.conditions[?(@.type==\"FailureTarget\")].reason} {.status.conditions[?(@.type==\"Failed\")].reason}",
        ])?
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .to_string();
        Ok(!reason.is_empty())
    })?;
    Ok(reason)
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
/// store first, which Postgres cannot tell, so only the read from the store can catch it; the
/// destination is put back whatever happened. A seeded run leaves its segment and base backup under
/// that other path, so the archive at the real one has a gap: a later restore takes its own base
/// backup first.
fn archive(seeded: bool) -> Result<Verdict> {
    let (path, url) = archive_destination()?;
    let verdict = if seeded {
        set_destination(&format!("{path}-elsewhere")).and_then(|()| archived(&path, &url))
    } else {
        archived(&path, &url)
    };
    let restored = if seeded {
        set_destination(&path)
    } else {
        Ok(())
    };
    let verdict = verdict?;
    restored?;
    Ok(verdict)
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
    let backup_id = base_backup(&backup);
    let _ = kubectl(&[
        "delete",
        "backup",
        &backup,
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
        "--wait=false",
    ]);
    let backup_id = match backup_id? {
        Ok(backup_id) => backup_id,
        Err(phase) => {
            let sidecars = kubectl(&[
                "get",
                "pod",
                "--namespace",
                NAMESPACE,
                "--selector",
                "cnpg.io/cluster=journal",
                "-o",
                "jsonpath={range .items[*]}{.metadata.name}: {.status.containerStatuses[?(@.name==\"plugin-barman-cloud\")].lastState.terminated.reason} {.status.containerStatuses[?(@.name==\"plugin-barman-cloud\")].restartCount} restarts; {end}",
            ])
            .unwrap_or_default();
            return Ok(Verdict::Broken(format!(
                "the base backup {backup} did not complete (phase `{phase}`); the plugin's sidecars: {}",
                sidecars.trim()
            )));
        }
    };
    let read = read_archive(&id, path, url, &segment);
    let _ = kubectl(&[
        "delete",
        "pod",
        &format!("archive-check-{id}"),
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
        "--wait=false",
    ]);
    let _ = kubectl(&[
        "delete",
        "networkpolicy",
        "archive-check",
        "--namespace",
        NAMESPACE,
        "--ignore-not-found",
    ]);
    let read = read?;
    if !read.listed {
        return Ok(Verdict::Broken(format!(
            "Barman could not list the store at {path}, so the drill read nothing:\n{}",
            read.logs
        )));
    }
    if !read.fetched {
        return Ok(Verdict::Broken(if read.logs.contains("does not exist") {
            format!("{segment} is not in the archive at {path}:\n{}", read.logs)
        } else {
            format!(
                "Barman failed to fetch {segment} from {path} for another reason than its absence:\n{}",
                read.logs
            )
        }));
    }
    if !read.logs.contains(&backup_id) {
        return Ok(Verdict::Broken(format!(
            "the base backup {backup_id} is not in the archive at {path}:\n{}",
            read.logs
        )));
    }
    Ok(Verdict::Held)
}

/// Takes a base backup through the plugin: its Barman id once it completes, or the phase it ended
/// in when it did not.
fn base_backup(backup: &str) -> Result<std::result::Result<String, String>> {
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
    until(|| {
        phase = kubectl(&[
            "get",
            "backup",
            backup,
            "--namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.phase}/{.status.backupId}/{.status.error}",
        ])?
        .trim()
        .to_string();
        Ok(phase.starts_with("completed/") || phase.starts_with("failed/"))
    })?;
    Ok(match phase.strip_prefix("completed/") {
        Some(rest) if !rest.starts_with('/') => {
            Ok(rest.split('/').next().unwrap_or_default().to_string())
        }
        _ => Err(phase),
    })
}

/// What the check pod read from the store: whether Barman listed it, whether it fetched the
/// segment, and both tools' logs.
struct ArchiveRead {
    listed: bool,
    fetched: bool,
    logs: String,
}

/// A pod with the plugin's own sidecar image and the archive's key that lists the base backups and
/// fetches `segment` back from `path`, as a restore would, under a network policy of its own that
/// reaches the store and nothing else. The caller removes the pod and the policy.
fn read_archive(id: &str, path: &str, url: &str, segment: &str) -> Result<ArchiveRead> {
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
    let listed = kubectl(&[
        "get",
        "pod",
        &name,
        "--namespace",
        NAMESPACE,
        "-o",
        "jsonpath={.status.initContainerStatuses[0].state.terminated.exitCode}",
    ])?;
    let mut logs = String::new();
    for container in ["list", "fetch"] {
        if let Ok(out) = kubectl(&["logs", &name, "--namespace", NAMESPACE, "-c", container]) {
            for line in out.lines() {
                logs.push_str(&format!("[{container}] {line}\n"));
            }
        }
    }
    Ok(ArchiveRead {
        listed: listed.trim() == "0",
        fetched: phase == "Succeeded",
        logs,
    })
}

const COMPOSE_DIR: &str = "deploy/cell/compose";
/// Where the compose drill keeps the secrets and the exported segment, owned by the Compose user;
/// `secrets/` is ignored by git.
const COMPOSE_SECRETS: &str = "deploy/cell/compose/secrets";

/// `docker compose` with the Compose file, the drill's workspace label and this change's image.
fn compose_cmd(args: &[&str]) -> Result<String> {
    let image = image_tag()?;
    let api = api_image_tag()?.unwrap_or_else(|| image.clone());
    let env = compose_env(Path::new("."), &image, &api, runtime_image()?.as_deref())?;
    let mut full = vec!["compose", "-f", "compose.json"];
    full.extend_from_slice(args);
    let out = Command::new("docker")
        .current_dir(COMPOSE_DIR)
        .args(&full)
        .envs(env)
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
    for args in brake_args(&workspace, &control) {
        let run = ["run", "--rm", "-v", &work, "control"]
            .into_iter()
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>();
        if let Err(e) = compose_cmd(&run) {
            return Ok(Verdict::Broken(format!(
                "`mandate {}` failed under Compose: {e:#}",
                args.join(" ")
            )));
        }
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
        Ok(_) => {
            let segment = as_root_in(COMPOSE_SECRETS, "cat /s/work/segment.jsonl")?;
            Ok(brakes_missing(&segment, &brakes_expected(&workspace))
                .map_or(Verdict::Held, Verdict::Broken))
        }
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
/// The connect finish's process (DEC-2348 item 3), from the API's image.
const CONNECT_FINISH_PROCESS: &str = "connect-finish";

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
    let started_by_built = |name: &str| {
        processes
            .iter()
            .any(|p| p["name"] == name && p["started_by"].as_str().is_some_and(&built))
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
            name: "the agent runtime, on the deny-all wiring until the post-R0 hook (DEC-1013)",
            skipped: if started_by_built("runtime") {
                Some(
                    "the deployment manager starts it from a version-2 `AgentDeployed` \
                     (DEC-1171); the demo's paper deployment stays version 1 until \
                     `connection record` can run, which waits on the executor's connect check \
                     (E21-30, lane X)"
                        .to_string(),
                )
            } else {
                lane(
                    built("runtime"),
                    "the runtime image is not on main yet (lane P3, E21-30)",
                )
            },
        },
        DemoPart {
            name: "a paper agent on rh-sim and recorded Alpaca fixtures, no credential",
            skipped: if built("runtime") && built("executor") {
                Some(
                    "the executor runs on the fixture paper broker (DEC-2031), but the runtime \
                     proposes no order on the deny-all wiring until the post-R0 hook (DEC-1013), \
                     and the executor fills no paper order before slice X1b-3 (E21-30)"
                        .to_string(),
                )
            } else {
                lane(
                    false,
                    "the runtime and executor images are not on main yet (lane P1, #1451)",
                )
            },
        },
        DemoPart {
            name: RESEARCH_GATEWAY,
            skipped: lane(
                false,
                "the research loop runs in the runtime once the post-R0 hook wires its ports; until \
                 then the cell's runtime runs the deny-all wiring (DEC-1013)",
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
/// [`WEB_URL`], or against the cell's API at [`DEMO_APP_ORIGIN`] once sign-in is ready
/// ([`web_run`]), unless `--no-web`.
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
    let mut listed = processes()?;
    if runtime_image()?.is_none() {
        for p in listed.iter_mut().filter(|p| p["name"] == "runtime") {
            p["built"] = json!(false);
        }
    }
    let mut parts = demo_parts(&listed, &help, stub_ready(Path::new(".")));
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
    let run = web_run(signin_ready, DEMO_WORKSPACE);
    eprintln!(
        "==> web/ at {} (Ctrl-C stops it; the cell keeps running)",
        run.url
    );
    if signin_ready {
        eprintln!(
            "    web/: the demo owner's workspace through the API, signed in by the stub issuer \
             (next dev only, DEC-2406)"
        );
    }
    let status = Command::new("npm")
        .current_dir("web")
        .args(["run", run.script])
        .envs(run.env.iter().map(|(name, value)| (*name, value.as_str())))
        .status()
        .with_context(|| format!("starting `npm run {}`", run.script))?;
    if status.success() || status.code().is_none() {
        Ok(())
    } else {
        bail!("`npm run {}` in web/ failed with {status}", run.script)
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
    let (status, reply) = open_session(&stub_token()?)?;
    if !signed_in(status, &reply) {
        bail!("POST /v1/sessions answered {status}, not 204 with the session cookie");
    }
    eprintln!("    sign-in: the demo owner opened a session through the local stub issuer");
    Ok(())
}

/// A sign-in token for [`DEMO_SUBJECT`] from the stub issuer.
fn stub_token() -> Result<String> {
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
    token["access_token"]
        .as_str()
        .map(str::to_owned)
        .context("the stub issuer's reply carries no access_token")
}

/// `POST /v1/sessions` with `token` from the app origin, and the API's status and whole reply.
fn open_session(token: &str) -> Result<(u16, String)> {
    let body = json!({"access_token": token}).to_string();
    http(
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
    .context("opening a session")
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

/// The `name=value` pair of the `__Host-` session cookie an answer to `POST /v1/sessions` sets,
/// as a `Cookie` header carries it back.
fn session_cookie(reply: &str) -> Option<String> {
    reply.split("\r\n\r\n").next()?.lines().find_map(|l| {
        let (name, value) = l.split_once(':')?;
        let pair = value.trim().split(';').next()?.trim();
        (name.eq_ignore_ascii_case("set-cookie") && pair.starts_with("__Host-mandate_session="))
            .then(|| pair.to_owned())
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
    use std::collections::BTreeSet;

    use super::*;

    /// The API's diagnosis asks for the logs of a run before only of the containers that restarted.
    #[test]
    fn only_restarted_containers_are_asked_for_their_previous_logs() {
        let listing = "api-1 api 5\nconnect-finish-1 credentials 0 finish 0\npending-1\napi-2 api 0 sidecar 2\n";
        assert_eq!(
            restarted_containers(listing),
            vec![
                ("api-1".to_owned(), "api".to_owned()),
                ("api-2".to_owned(), "sidecar".to_owned()),
            ]
        );
        assert!(restarted_containers("").is_empty());
    }

    /// The fixture's client secret is written at the client secret's own path in the `mandate`
    /// mount, from stdin alone: the root token is read as its first line and the value is the
    /// rest, already base64, so neither reaches an argument, and nothing is printed.
    #[test]
    fn the_client_secret_seed_reads_its_token_and_value_from_stdin_alone() {
        assert_eq!(FIXTURE_CLIENT_SECRET, "oauth-fixture-client");
        assert_eq!(FIXTURE_CLIENT_SECRET_KEY, "client-secret");
        assert!(CLIENT_SECRET_SEED.starts_with("read -r BAO_TOKEN && export BAO_TOKEN && "));
        assert!(CLIENT_SECRET_SEED.ends_with(
            "exec bao kv put -mount=mandate platform/alpaca-oauth-client-secret value=- >/dev/null"
        ));
        assert!(
            CLIENT_SECRET_SEED
                .contains("bao secrets enable -path=mandate -version=2 kv >/dev/null")
        );
        assert!(
            !CLIENT_SECRET_SEED.contains('$'),
            "no variable reaches an argument"
        );
        let stdin = client_secret_seed_stdin("fake-root", "ZmFrZS1jbGllbnQtc2VjcmV0");
        assert_eq!(stdin, "fake-root\nZmFrZS1jbGllbnQtc2VjcmV0");
    }

    /// The fixture renders only where the cell runs both the stub, whose image it runs, and the
    /// exchange, the one process that reaches it; then it takes the cell's fake client and the
    /// API's loopback callback, which the chart's schema admits.
    #[test]
    fn the_fixture_is_on_only_with_the_stub_and_the_exchange() {
        assert!(fixture_values(None, None).is_empty());
        assert!(fixture_values(Some("mandate.local/stub-issuer:a"), None).is_empty());
        assert!(fixture_values(None, Some("mandate.local/token-exchange:a")).is_empty());
        assert_eq!(
            fixture_values(
                Some("mandate.local/stub-issuer:a"),
                Some("mandate.local/token-exchange:a")
            ),
            [
                "oauthFixture.enabled=true",
                "alpacaOAuth.clientId=cell-fixture-client",
                "alpacaOAuth.redirectUri=http://127.0.0.1:8080/v1/oauth/alpaca/callback",
            ]
        );
    }

    /// The API's vault binding and the connect finish come on together, and only where the cell
    /// runs both the API and the role provisioner, whose presence is what provisions the vault and
    /// issues the two AppRole Secrets they mount.
    #[test]
    fn the_api_vault_binding_and_connect_finish_are_on_only_with_the_api_and_the_provisioner()
    -> Result<()> {
        let api = Some("mandate.local/api:a");
        let provisioner = Some("mandate.local/role-provisioner:a");
        assert!(vault_binding_values(None, None)?.is_empty());
        assert!(vault_binding_values(api, None)?.is_empty());
        assert!(vault_binding_values(None, provisioner)?.is_empty());
        assert_eq!(
            vault_binding_values(api, provisioner)?,
            ["api.vaultBinding=true", "api.connectFinish=true"]
        );
        Ok(())
    }

    /// A Secret the cell already holds is kept, unread and unchanged; none is made, from a value
    /// drawn once; and a failure to read, draw or create fails the step, so `cell up` stops.
    #[test]
    fn the_fixture_client_secret_is_kept_or_made_once_and_its_failures_stop_cell_up() {
        assert!(keep_existing("secret/oauth-fixture-client\n"));
        assert!(!keep_existing(""));
        assert!(!keep_existing("  \n"));
        let kept = fixture_client_secret(
            || Ok("secret/oauth-fixture-client\n".to_owned()),
            || panic!("a kept Secret draws no value"),
            |_| panic!("a kept Secret is not made again"),
        );
        assert!(matches!(kept, Ok(false)));
        let mut made = Vec::new();
        let created = fixture_client_secret(
            || Ok(String::new()),
            || Ok("ab01".to_owned()),
            |secret| {
                made.push(secret);
                Ok(())
            },
        );
        assert!(matches!(created, Ok(true)));
        assert_eq!(made, [fixture_client_secret_object("ab01")]);
        let unread = fixture_client_secret(
            || bail!("kubectl failed"),
            || Ok("ab01".to_owned()),
            |_| panic!("nothing is made when the cell cannot be read"),
        );
        assert!(unread.is_err());
        let undrawn = fixture_client_secret(
            || Ok(String::new()),
            || bail!("no randomness"),
            |_| panic!("nothing is made without a value"),
        );
        assert!(undrawn.is_err());
        let refused = fixture_client_secret(
            || Ok(String::new()),
            || Ok("ab01".to_owned()),
            |_| bail!("kubectl apply failed"),
        );
        assert!(refused.is_err());
    }

    /// The fixture client's Secret holds its one key as written, in the cell's namespace, and its
    /// value is hex.
    #[test]
    fn the_fixture_client_secret_is_one_key_in_the_cells_namespace() {
        let made = fixture_client_secret_object("ab01");
        assert_eq!(made["metadata"]["name"], "oauth-fixture-client");
        assert_eq!(made["metadata"]["namespace"], NAMESPACE);
        assert_eq!(made["type"], "Opaque");
        assert_eq!(made["stringData"], json!({"client-secret": "ab01"}));
        assert!(made.get("data").is_none());
        assert_eq!(hex_of(&[0, 1, 0xab, 0xff]), "0001abff");
        assert_eq!(FIXTURE_CLIENT_SECRET_BYTES, 32);
    }

    /// The cell makes the fixture's Secret before the workspace release that mounts it, and seeds
    /// the vault from it only once that release is installed.
    #[test]
    fn the_client_secret_is_made_before_and_seeded_after_the_workspace_release() {
        let source = include_str!("cell.rs");
        let up = source
            .split("pub fn up() -> Result<()> {")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_default();
        let made = up.find("    fixture_client_secret(").unwrap_or(usize::MAX);
        let made_call = up
            .get(made..)
            .and_then(|rest| rest.split("\n    if stub.is_some()").next())
            .unwrap_or_default();
        assert!(
            made_call.trim_end().ends_with(")?;"),
            "a failure to make the Secret fails `cell up`: {made_call}"
        );
        let install = up.find("let installed = helm(&install);").unwrap_or(0);
        let installed = up.find("    installed?;").unwrap_or(usize::MAX);
        let seeded = up.find("    seed_client_secret()?;").unwrap_or(0);
        assert!(
            made < install,
            "the Secret is made before the workspace install"
        );
        assert!(
            installed < seeded,
            "the seed runs after the workspace install"
        );
        assert_eq!(up.matches("seed_client_secret()").count(), 1);
        assert_eq!(up.matches("fixture_client_secret(").count(), 1);
    }

    /// The vault drill passes only these answers, written out here rather than built from
    /// [`VAULT_DRILL_READS`]: one line per read, every read as written, no wildcard but DEC-2349's
    /// prefix grants, each in its own policy, and a scan of
    /// every `mandate-*` policy. A second line for a read, a missing or failed scan, an error, or a
    /// line from another container fails it; the seeded symptom is the runtime policy's wildcard.
    #[test]
    fn the_vault_drill_judges_each_read_and_every_wildcard() {
        let expected = [
            "runtime connections/x64656d6f2d7061706572/token denied",
            "runtime connections/x6472696c6c2d6f74686572/token denied",
            "runtime platform/alpaca-oauth-client-secret denied",
            "executor-demo-paper connections/x64656d6f2d7061706572/token allowed",
            "executor-demo-paper connections/x6472696c6c2d6f74686572/token denied",
            "executor-demo-paper platform/alpaca-oauth-client-secret denied",
            "executor-drill-other connections/x6472696c6c2d6f74686572/token allowed",
            "executor-drill-other connections/x64656d6f2d7061706572/token denied",
            "model-gateway platform/model-provider-key allowed",
            "model-gateway connections/x64656d6f2d7061706572/token denied",
            "model-gateway platform/alpaca-oauth-client-secret denied",
            "model-gateway platform/email-credential denied",
            "executor-demo-paper connections/x64656d6f2d7061706572/code denied",
            "checked 10 mandate policies",
        ];
        let probe = |lines: &[&str]| -> String {
            lines
                .iter()
                .map(|l| format!("[pod/vault-drill-1-x/probe] {l}\n"))
                .collect()
        };
        let held = probe(&expected);
        assert_eq!(
            vault_drill_problems(&held, VAULT_DRILL_POLICIES),
            Vec::<String>::new()
        );
        let granted = [
            r#"wildcard in mandate-api: path "mandate/data/connections/+/code" { capabilities = ["create", "update"] }"#,
            r#"wildcard in mandate-api: path "mandate/metadata/connections/+/code" { capabilities = ["delete"] }"#,
            r#"wildcard in mandate-api: path "mandate/metadata/connections/+/token" { capabilities = ["delete"] }"#,
            r#"wildcard in mandate-connection-manager: path "mandate/metadata/connections/+/code" { capabilities = ["delete"] }"#,
            r#"wildcard in mandate-connection-manager: path "mandate/metadata/connections/+/token" { capabilities = ["delete"] }"#,
            r#"wildcard in mandate-connection-manager: path "mandate/data/personal/fingerprints/+/+" { capabilities = ["read"] }"#,
        ];
        let prefixed = format!("{held}{}", probe(&granted));
        assert_eq!(
            vault_drill_problems(&prefixed, VAULT_DRILL_POLICIES),
            Vec::<String>::new(),
            "DEC-2349's prefix grants, each in its own policy, are admitted"
        );
        for wider in [
            r#"wildcard in mandate-api: path "mandate/data/connections/+/token" { capabilities = ["read"] }"#,
            r#"wildcard in mandate-api: path "mandate/data/connections/+/code" { capabilities = ["create", "update", "read"] }"#,
            r#"wildcard in mandate-api: path "mandate/data/connections/*" { capabilities = ["create", "update"] }"#,
            r#"wildcard in mandate-api: path "mandate/data/personal/fingerprints/+/+" { capabilities = ["read"] }"#,
            r#"wildcard in mandate-connection-manager: path "mandate/data/personal/+" { capabilities = ["read"] }"#,
            r#"wildcard in mandate-runtime: path "mandate/metadata/connections/+/token" { capabilities = ["delete"] }"#,
            r#"wildcard in mandate-api: # any + path "mandate/metadata/connections/+/code" { capabilities = ["delete"] }"#,
            "wildcard in mandate-api",
        ] {
            assert_eq!(
                vault_drill_problems(
                    &format!("{prefixed}{}", probe(&[wider])),
                    VAULT_DRILL_POLICIES
                ),
                [wider],
                "a wildcard wider than DEC-2349's, or in another policy, is still caught"
            );
        }
        let unprefixed = held.replace("[pod/vault-drill-1-x/probe] ", "");
        assert_eq!(
            vault_drill_problems(&unprefixed, VAULT_DRILL_POLICIES).len(),
            expected.len(),
            "only the probe's lines, as kubectl prefixes them, are judged"
        );
        let elsewhere = held.replace("/probe] ", "/provision] ");
        assert_eq!(
            vault_drill_problems(&elsewhere, VAULT_DRILL_POLICIES).len(),
            expected.len()
        );
        let leaked = held.replace(
            "runtime connections/x64656d6f2d7061706572/token denied",
            "runtime connections/x64656d6f2d7061706572/token allowed",
        );
        let wildcard = format!("{leaked}{}", probe(&["wildcard in mandate-runtime"]));
        let found = vault_drill_problems(&wildcard, VAULT_DRILL_POLICIES).join("\n");
        assert!(found.contains(Drill::Vault.seeded_symptom()), "{found}");
        assert!(
            found.contains("want `runtime connections/x64656d6f2d7061706572/token denied`"),
            "{found}"
        );
        let twice = format!(
            "{held}{}",
            probe(&["runtime connections/x64656d6f2d7061706572/token allowed"])
        );
        assert_eq!(
            vault_drill_problems(&twice, VAULT_DRILL_POLICIES),
            [
                "want `runtime connections/x64656d6f2d7061706572/token denied`, got \
                 `runtime connections/x64656d6f2d7061706572/token denied` and \
                 `runtime connections/x64656d6f2d7061706572/token allowed`"
            ]
        );
        let missing = probe(
            &expected
                .iter()
                .copied()
                .filter(|l| *l != "model-gateway platform/model-provider-key allowed")
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            vault_drill_problems(&missing, VAULT_DRILL_POLICIES),
            ["want `model-gateway platform/model-provider-key allowed`, got `no answer`"]
        );
        let unscanned = held.replace("checked 10 mandate policies", "checked 0 mandate policies");
        assert_eq!(
            vault_drill_problems(&unscanned, VAULT_DRILL_POLICIES),
            ["want `checked 10 mandate policies`, got `checked 0 mandate policies`"]
        );
        let scanned_twice = format!("{held}{}", probe(&["checked 10 mandate policies"]));
        assert_eq!(
            vault_drill_problems(&scanned_twice, VAULT_DRILL_POLICIES),
            [
                "want `checked 10 mandate policies`, got `checked 10 mandate policies` and \
                 `checked 10 mandate policies`"
            ]
        );
        let no_scan = probe(&expected[..expected.len() - 1]);
        assert_eq!(
            vault_drill_problems(&no_scan, VAULT_DRILL_POLICIES),
            ["want `checked 10 mandate policies`, got `no scan`"]
        );
        let errored = held.replace(
            "executor-demo-paper connections/x64656d6f2d7061706572/token allowed",
            "executor-demo-paper connections/x64656d6f2d7061706572/token error",
        );
        assert_eq!(
            vault_drill_problems(&errored, VAULT_DRILL_POLICIES).len(),
            1
        );
        let plain = vault_drill_script(false);
        let seeded = vault_drill_script(true);
        assert!(!plain.contains("policy write"), "{plain}");
        assert!(seeded.contains("trap ") && seeded.contains("bao policy read mandate-runtime"));
        assert!(
            seeded.contains("bao policy write mandate-runtime -' EXIT"),
            "the restore's output is not discarded: {seeded}"
        );
        assert!(
            seeded.find("trap ") < seeded.find("connections/+/token"),
            "the restore is armed before the wildcard is planted: {seeded}"
        );
        for script in [&plain, &seeded] {
            assert!(
                script.contains("bao read -field=data \"mandate/data/$2\"")
                    && !script.contains("kv get"),
                "reads skip kv get's mount preflight: {script}"
            );
            assert!(
                script.contains("bao token lookup")
                    && script.contains("checked $n mandate policies"),
                "{script}"
            );
        }
    }

    /// Once `cell up` provisions for the role provisioner, the drill must read its three policies
    /// too: thirteen, and ten is a scan that missed some.
    #[test]
    fn the_vault_drill_counts_the_provisioned_policies() {
        assert_eq!(vault_drill_policies(false), 10);
        assert_eq!(vault_drill_policies(true), 13);
        let scan = |n: usize| format!("[pod/vault-drill-1-x/probe] checked {n} mandate policies\n");
        let only_scan = |logs: &str, policies: usize| -> Vec<String> {
            vault_drill_problems(logs, policies)
                .into_iter()
                .filter(|p| p.contains("mandate policies"))
                .collect()
        };
        assert_eq!(
            only_scan(&scan(13), vault_drill_policies(true)),
            Vec::<String>::new()
        );
        assert_eq!(
            only_scan(&scan(10), vault_drill_policies(true)),
            ["want `checked 13 mandate policies`, got `checked 10 mandate policies`"]
        );
    }

    /// The vault drill's scan, run by `sh` against a fake `bao` that serves policies from files,
    /// and judged as the drill judges it: the role provisioner's policy, written out as
    /// `mandate-vault` writes it (its `the_provisioners_policy_requires_every_parameter_and_pins_all_but_the_name`),
    /// prints exactly its two role stanzas, each admitted (DEC-2350 item 1); a wildcard anywhere
    /// else in it, a `*` for its `+`, a parameter changed, or the same stanzas in another policy, is
    /// reported.
    #[test]
    fn the_vault_drill_admits_only_the_provisioners_role_stanzas() -> Result<()> {
        let block = |mount: &str, caps: &str, policy: &str, ttl: &str, max: &str| {
            format!(
                "path \"auth/{mount}/role/+\" {{\n  \
                 capabilities = [{caps}]\n  \
                 required_parameters = [\"alias_name_source\", \"audience\", \"bound_service_account_names\", \"bound_service_account_namespaces\", \"token_max_ttl\", \"token_no_default_policy\", \"token_policies\", \"token_ttl\"]\n  \
                 allowed_parameters = {{\n    \
                 \"alias_name_source\" = [\"serviceaccount_uid\"]\n    \
                 \"audience\" = [\"mandate-vault\"]\n    \
                 \"bound_service_account_names\" = []\n    \
                 \"bound_service_account_namespaces\" = [\"ws-ws1\"]\n    \
                 \"token_max_ttl\" = [\"{max}\"]\n    \
                 \"token_no_default_policy\" = [\"false\"]\n    \
                 \"token_policies\" = [\"{policy}\"]\n    \
                 \"token_ttl\" = [\"{ttl}\"]\n  \
                 }}\n}}\n"
            )
        };
        let provisioner = format!(
            "{}{}",
            block(
                "kubernetes-executor",
                "\"create\", \"update\"",
                "mandate-kubernetes-executor",
                "86400",
                "2592000"
            ),
            block(
                "kubernetes-exchange",
                "\"create\", \"update\", \"delete\"",
                "mandate-kubernetes-exchange",
                "600",
                "600"
            )
        );
        let plain =
            "path \"mandate/data/platform/model-provider-key\" {\n  capabilities = [\"read\"]\n}\n";
        let scan = |policies: &[(&str, &str)]| -> Result<Vec<String>> {
            let dir = std::env::temp_dir().join(format!(
                "xtask-vault-scan-{}-{}",
                std::process::id(),
                policies.len() + policies.iter().map(|(_, b)| b.len()).sum::<usize>()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("bin"))?;
            fs::create_dir_all(dir.join("policies"))?;
            let names: Vec<&str> = policies.iter().map(|(name, _)| *name).collect();
            for (name, body) in policies {
                fs::write(dir.join("policies").join(name), body)?;
            }
            let bao = dir.join("bin/bao");
            fs::write(
                &bao,
                format!(
                    "#!/bin/sh\n\
                     case \"$1 $2\" in\n\
                       'policy list') printf '%s\\n' {names} ;;\n\
                       'policy read') cat \"{dir}/policies/$3\" ;;\n\
                       *) exit 9 ;;\n\
                     esac\n",
                    names = names.join(" "),
                    dir = dir.display(),
                ),
            )?;
            fs::set_permissions(&bao, fs::Permissions::from_mode(0o755))?;
            let out = Command::new("sh")
                .arg("-c")
                .arg(format!("set -eu\nop=fake\n{}", vault_drill_scan()))
                .env(
                    "PATH",
                    format!("{}:{}", dir.join("bin").display(), env::var("PATH")?),
                )
                .output()?;
            fs::remove_dir_all(&dir)?;
            ensure!(out.status.success(), "the scan failed: {out:?}");
            Ok(String::from_utf8(out.stdout)?
                .lines()
                .map(str::to_owned)
                .collect())
        };
        let reported = |lines: &[String]| -> Vec<String> {
            lines
                .iter()
                .filter(|l| l.starts_with("wildcard in ") && !admitted_wildcard(l))
                .cloned()
                .collect()
        };
        let held = scan(&[
            ("default", "path \"*\" {}\n"),
            ("mandate-runtime", plain),
            (PROVISIONER_POLICY, &provisioner),
        ])?;
        assert_eq!(
            held,
            [
                format!("wildcard in {PROVISIONER_POLICY}: {PROVISIONER_EXECUTOR_ROLES}"),
                format!("wildcard in {PROVISIONER_POLICY}: {PROVISIONER_EXCHANGE_ROLES}"),
                "checked 2 mandate policies".to_owned(),
            ],
            "the scan prints the provisioner's two stanzas as the grants name them, and reads no \
             policy but `mandate-*`"
        );
        assert_eq!(reported(&held), Vec::<String>::new());
        let api = "path \"mandate/data/connections/+/code\" {\n  capabilities = [\"create\", \"update\"]\n}\n\
                   path \"mandate/metadata/connections/+/code\" {\n  capabilities = [\"delete\"]\n}\n\
                   path \"mandate/metadata/connections/+/token\" {\n  capabilities = [\"delete\"]\n}\n";
        let manager = "path \"mandate/metadata/connections/+/code\" {\n  capabilities = [\"delete\"]\n}\n\
                       path \"mandate/metadata/connections/+/token\" {\n  capabilities = [\"delete\"]\n}\n\
                       path \"mandate/data/personal/fingerprints/+/+\" {\n  capabilities = [\"read\"]\n}\n";
        let granted = scan(&[
            ("mandate-api", api),
            ("mandate-connection-manager", manager),
        ])?;
        assert_eq!(granted.len(), 7, "six stanzas and the count: {granted:?}");
        assert_eq!(
            reported(&granted),
            Vec::<String>::new(),
            "DEC-2349's grants, as `mandate-vault` writes them, are still admitted whole"
        );
        let elsewhere = format!(
            "{provisioner}path \"mandate/data/connections/+/token\" {{\n  capabilities = [\"read\"]\n}}\n"
        );
        for (name, body, why) in [
            (PROVISIONER_POLICY, elsewhere, "a wildcard elsewhere in it"),
            (
                PROVISIONER_POLICY,
                provisioner.replace("kubernetes-exchange/role/+", "kubernetes-exchange/role/*"),
                "a `*` for its `+`",
            ),
            (
                PROVISIONER_POLICY,
                provisioner.replace("[\"ws-ws1\"]", "[\"ws-+\"]"),
                "a `+` inside a stanza's body",
            ),
            (
                PROVISIONER_POLICY,
                provisioner.replace("[\"600\"]", "[\"86400\"]"),
                "a parameter widened",
            ),
            (
                PROVISIONER_POLICY,
                provisioner.replace(
                    "\"create\", \"update\"]",
                    "\"create\", \"update\", \"delete\"]",
                ),
                "a capability added",
            ),
            (
                "mandate-runtime",
                provisioner.clone(),
                "the same stanzas in another policy",
            ),
            (
                PROVISIONER_POLICY,
                provisioner.replacen(
                    "\"token_ttl\" = [\"86400\"]\n  }\n",
                    "\"token_ttl\" = [\"86400\"]\n  }\n  capabilities = [\"create\", \"update\", \"delete\", \"sudo\"]\n",
                    1,
                ),
                "a key repeated after the nested block, which the vault merges",
            ),
            (
                PROVISIONER_POLICY,
                provisioner.replace("}\n}\n", "} }\n"),
                "a stanza whose closing brace is not alone on its line",
            ),
        ] {
            assert!(
                !reported(&scan(&[(name, &body)])?).is_empty(),
                "{why} is reported"
            );
        }
        Ok(())
    }

    /// The role provisioner's image (DEC-2350): its package is built with the cell's others and is
    /// one the cell jobs follow, its Dockerfile copies exactly that binary, and its key is the
    /// chart's process and image name.
    #[test]
    fn the_cell_builds_the_role_provisioners_image() -> Result<()> {
        assert!(BUILT_PACKAGES.contains(&"mandate-provisionerd"));
        assert!(IMAGE_PACKAGES.contains(&"mandate-provisionerd"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dockerfile = fs::read_to_string(root.join(ROLE_PROVISIONER_DOCKERFILE))?;
        assert!(
            dockerfile.contains("COPY mandate-provisionerd /mandate-provisionerd"),
            "{dockerfile}"
        );
        let process = processes_in(&root)?
            .into_iter()
            .find(|p| p["name"] == ROLE_PROVISIONER_PROCESS)
            .context("processes.json has no role provisioner")?;
        assert_eq!(process["image"], ROLE_PROVISIONER_PROCESS);
        assert_eq!(process["built"], true);
        Ok(())
    }

    /// The scheduler's image (E21-29): its package is built with the cell's others and is one the
    /// cell jobs follow, its Dockerfile copies exactly that binary, its key is the chart's process
    /// and image name, and `cell up` names it, so the clock stream the runtime and executor read
    /// is ticked in the cell.
    #[test]
    fn the_cell_builds_the_schedulers_image() -> Result<()> {
        assert!(BUILT_PACKAGES.contains(&SCHEDULER_PACKAGE));
        assert!(IMAGE_PACKAGES.contains(&SCHEDULER_PACKAGE));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dockerfile = fs::read_to_string(root.join(SCHEDULER_DOCKERFILE))?;
        assert!(
            dockerfile.contains("COPY mandate-scheduler /mandate-scheduler"),
            "{dockerfile}"
        );
        assert!(
            dockerfile.contains("FROM scratch") && dockerfile.contains("USER 65532:65532"),
            "{dockerfile}"
        );
        let process = processes_in(&root)?
            .into_iter()
            .find(|p| p["name"] == SCHEDULER_PROCESS)
            .context("processes.json has no scheduler")?;
        assert_eq!(process["image"], SCHEDULER_PROCESS);
        assert_eq!(process["built"], true);
        assert_eq!(process["probe"][0], "/mandate-scheduler");
        Ok(())
    }

    /// The data process's image (E21-29 lane SC): built with the cell's others and followed by the
    /// cell jobs, its Dockerfile copies exactly that binary on an empty base as non-root, and its
    /// key is the chart's process and image name.
    #[test]
    fn the_cell_builds_the_data_image() -> Result<()> {
        assert!(BUILT_PACKAGES.contains(&DATA_PACKAGE));
        assert!(IMAGE_PACKAGES.contains(&DATA_PACKAGE));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dockerfile = fs::read_to_string(root.join(DATA_DOCKERFILE))?;
        assert!(
            dockerfile.contains("COPY mandate-datad /mandate-datad")
                && dockerfile.contains("FROM scratch")
                && dockerfile.contains("USER 65532:65532"),
            "{dockerfile}"
        );
        let process = processes_in(&root)?
            .into_iter()
            .find(|p| p["name"] == DATA_PROCESS)
            .context("processes.json has no data process")?;
        assert_eq!(process["image"], DATA_PROCESS);
        assert_eq!(process["built"], true);
        assert_eq!(process["db_role"], Value::Null);
        assert_eq!(process["probe"][0], "/mandate-datad");
        Ok(())
    }

    /// DEC-2339 item 2's and DEC-2345 item 3's two `create` grants of the Kubernetes-auth
    /// executors' policy, as the cell's drill printed them on CI, are admitted for any accessor;
    /// another capability, a path outside the executor's own `personal/accounts/x{{…}}/` or
    /// `personal/fingerprints/x{{…}}/`, the stanzas in another policy, a change inside the template
    /// but the accessor, or an accessor of another shape, is reported.
    #[test]
    fn the_vault_drill_admits_the_kubernetes_executors_personal_grants() {
        let line = |accessor: &str, path: &str, capabilities: &str| {
            format!(
                "wildcard in mandate-kubernetes-executor: path \"mandate/data/personal/{path}/x{{{{identity.entity.aliases.{accessor}.metadata.service_account_name}}}}/+\" {{ capabilities = [{capabilities}] }}"
            )
        };
        for accessor in [
            "auth_kubernetes_050c14a4",
            "auth_kubernetes_0123456789abcdef",
        ] {
            for path in ["accounts", "fingerprints"] {
                let held = line(accessor, path, "\"create\"");
                assert!(admitted_wildcard(&held), "{held}");
            }
        }
        let held = line("auth_kubernetes_050c14a4", "accounts", "\"create\"");
        for (bad, why) in [
            (
                line(
                    "auth_kubernetes_050c14a4",
                    "accounts",
                    "\"create\", \"update\"",
                ),
                "another capability",
            ),
            (
                line("auth_kubernetes_050c14a4", "accounts", "\"read\""),
                "a read for its create",
            ),
            (
                line("auth_kubernetes_050c14a4", "tokens", "\"create\""),
                "a path outside its personal entries",
            ),
            (held.replace("/+\" {", "/*\" {"), "a `*` for its `+`"),
            (held.replace("/+\" {", "/+/+\" {"), "a second segment"),
            (
                held.replace("mandate-kubernetes-executor", "mandate-kubernetes-exchange"),
                "the stanza in another policy",
            ),
            (
                held.replace("service_account_name", "service_account_namespace"),
                "a change inside the template but the accessor",
            ),
            (
                held.replace("{{identity.", "{{ identity."),
                "the template spelled another way",
            ),
            (
                held.replace("auth_kubernetes_050c14a4", "auth_kubernetes_050C14A4"),
                "an accessor not in lowercase hex",
            ),
            (
                held.replace("auth_kubernetes_050c14a4", "auth_approle_050c14a4"),
                "another mount type's accessor",
            ),
            (
                held.replace("auth_kubernetes_050c14a4", "auth_kubernetes_"),
                "an accessor with no digits",
            ),
            (
                held.replace("auth_kubernetes_050c14a4", ACCESSOR_PLACEHOLDER),
                "the placeholder itself",
            ),
        ] {
            assert!(!admitted_wildcard(&bad), "{why} is admitted: {bad}");
        }
        assert_eq!(
            normalised_accessor(
                "a aliases.auth_kubernetes_ab.metadata b aliases.auth_kubernetes_cd.metadata"
            ),
            "a aliases.<accessor>.metadata b aliases.<accessor>.metadata"
        );
        assert_eq!(
            normalised_accessor("aliases.auth_kubernetes_ab.other"),
            "aliases.auth_kubernetes_ab.other"
        );
    }

    /// The cell's vault turns Kubernetes auth on exactly when the role provisioner runs, as
    /// staging's values do; what the chart then renders (the projected token and CA, the reviewer
    /// binding, egress to the API server alone) is `deploy/tests/test_vault.py`'s.
    #[test]
    fn the_vault_runs_kubernetes_auth_for_the_provisioner() {
        let off = vault_install_args(false);
        assert!(!off.iter().any(|a| a.starts_with("kubeApi")), "{off:?}");
        let on = vault_install_args(true);
        assert_eq!(on[..off.len()], off[..]);
        assert_eq!(on[off.len()..], ["--set", "kubeApi.enabled=true"]);
    }

    /// A vault this run initialized is provisioned even where the Secret is left from the vault
    /// before; an existing vault only where the Secret is missing.
    #[test]
    fn a_fresh_vault_is_always_provisioned() {
        assert!(must_provision(true, true));
        assert!(must_provision(true, false));
        assert!(must_provision(false, false));
        assert!(!must_provision(false, true));
    }

    /// The vault's Kubernetes auth host is the `kubernetes` Service's IPv4 address and port, and
    /// anything else `kubectl` prints is refused.
    #[test]
    fn the_kubernetes_host_is_an_ipv4_address_and_a_port() -> Result<()> {
        assert_eq!(
            kubernetes_host_from("10.43.0.1:443\n")?,
            "https://10.43.0.1:443"
        );
        for bad in [
            "",
            "10.43.0.1",
            "10.43.0.1:",
            ":443",
            "fd00::1:443",
            "kubernetes:443",
            "10.43.0.1:70000",
        ] {
            assert!(kubernetes_host_from(bad).is_err(), "{bad} was accepted");
        }
        Ok(())
    }

    /// The node's provisioning script takes the root token on standard input alone, which only the
    /// CLI reads; prints the Secrets into `kubectl apply` and nowhere else; stops the forward and
    /// removes the CA and the copied CLI on every exit; and, run by `sh` against a CLI and a
    /// `kubectl` that fail, fails.
    #[test]
    fn the_cell_provisions_the_vault_with_its_token_on_stdin() -> Result<()> {
        let script = provision_script();
        assert!(script.starts_with("set -eu\n"), "{script}");
        assert!(
            script.contains(&format!(
                "{NODE_CLI} vault provision --address \"https://127.0.0.1:$port\""
            )) && script.contains(
                "--operator-token-stdin --secrets-namespace ws-ws1 --kubernetes-host \"$1\""
            ) && script.contains("| kubectl apply --namespace ws-ws1 -f - >/dev/null"),
            "{script}"
        );
        assert!(!script.contains("--operator-token-file") && !script.contains("BAO_TOKEN"));
        assert_eq!(
            script.matches("</dev/null").count(),
            2,
            "kubectl reads no stdin: {script}"
        );
        assert!(
            script.contains(&format!(
                "trap '[ -z \"$pf\" ] || kill \"$pf\"; rm -rf \"$dir\" {NODE_CLI}' EXIT"
            )),
            "{script}"
        );
        assert!(
            script.find("trap ") < script.find("port-forward"),
            "{script}"
        );
        let dir = std::env::temp_dir().join(format!("xtask-provision-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        let kubectl = dir.join("kubectl");
        fs::write(
            &kubectl,
            "#!/bin/sh\n\
             case \"$1\" in\n\
               port-forward) echo 'Forwarding from 127.0.0.1:41234 -> 8200'; exec sleep 5 ;;\n\
               apply) cat >/dev/null; exit 1 ;;\n\
               *) echo ca ;;\n\
             esac\n",
        )?;
        fs::set_permissions(&kubectl, fs::Permissions::from_mode(0o755))?;
        let local = script
            .replace(
                "/var/lib/rancher/k3s/provision.XXXXXX",
                &format!("{}/p.XXXXXX", dir.display()),
            )
            .replace(NODE_CLI, "false");
        let out = Command::new("sh")
            .arg("-c")
            .arg(&local)
            .arg("provision")
            .arg("https://10.43.0.1:443")
            .env("PATH", format!("{}:{}", dir.display(), env::var("PATH")?))
            .stdin(Stdio::null())
            .output()?;
        let left: Vec<_> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        fs::remove_dir_all(&dir)?;
        assert!(
            !out.status.success(),
            "a failed CLI fails the script: {out:?}"
        );
        assert_eq!(left, ["kubectl"], "the CA's directory is removed");
        Ok(())
    }

    /// The cell judges the vault's status as the client judges `sys/seal-status`: uninitialized,
    /// sealed or unsealed on raft, and a dev server's in-memory storage refused whatever its seal.
    #[test]
    fn the_cell_refuses_a_dev_vault_and_reads_its_seal() {
        let status = |init: bool, sealed: bool, storage: &str| {
            json!({"initialized": init, "sealed": sealed, "storage_type": storage}).to_string()
        };
        assert_eq!(
            vault_state(&status(false, true, "raft")).unwrap(),
            VaultState::Uninitialized
        );
        assert_eq!(
            vault_state(&status(true, true, "raft")).unwrap(),
            VaultState::Sealed
        );
        assert_eq!(
            vault_state(&status(true, false, "raft")).unwrap(),
            VaultState::Unsealed
        );
        assert!(vault_state(&status(true, false, "inmem")).is_err());
        assert!(vault_state(&status(true, true, "inmem")).is_err());
        assert!(vault_state(r#"{"sealed": false, "storage_type": "raft"}"#).is_err());
        assert!(vault_state("not json").is_err());
    }

    /// The runtime image's context carries every file of the demo's research bundle and nothing
    /// else under `research/`, and its Dockerfile copies them to the one path no release image may
    /// hold (DEC-2246).
    #[test]
    fn the_runtime_image_carries_the_whole_demo_bundle_at_the_cell_s_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let on_disk: BTreeSet<String> = fs::read_dir(root.join(DEMO_RESEARCH))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let carried: BTreeSet<String> = DEMO_RESEARCH_FILES
            .iter()
            .map(|file| (*file).to_owned())
            .collect();
        assert_eq!(carried, on_disk);
        assert!(
            demo_research().iter().all(
                |(from, name)| from.starts_with(DEMO_RESEARCH) && name.starts_with("research/")
            )
        );
        let dockerfile = fs::read_to_string(root.join(RUNTIME_DOCKERFILE)).unwrap();
        assert!(
            dockerfile.contains(&format!(
                "COPY research/ /{}/\n",
                crate::image::CELL_RESEARCH
            )),
            "{dockerfile}"
        );
    }

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

    fn deadline_job() -> Value {
        json!({
            "spec": {"template": {"spec": {"containers": [{"name": "cli", "env": [{"name": "TOKEN", "value": "s3cret-value"}]}]}}},
            "status": {
                "active": 1,
                "conditions": [{
                    "type": "FailureTarget",
                    "status": "True",
                    "reason": "DeadlineExceeded",
                    "message": "Job was active longer than specified deadline"
                }]
            }
        })
    }

    fn pods_of(pod: Value) -> Value {
        json!({"kind": "List", "items": [pod]})
    }

    /// A Job its deadline stopped while its pod could not be scheduled: the report names the
    /// deadline, the pod stuck `Pending` with why, and the scheduler's event.
    #[test]
    fn the_failure_report_shows_a_deadline_with_a_pending_pod() {
        let pods = pods_of(json!({
            "metadata": {"name": "control-a-x1"},
            "spec": {"containers": [{"name": "cli", "env": [{"name": "TOKEN", "value": "s3cret-value"}]}]},
            "status": {
                "phase": "Pending",
                "conditions": [{"type": "PodScheduled", "status": "False", "reason": "Unschedulable", "message": "0/1 nodes are available: 1 Insufficient cpu."}],
                "containerStatuses": [{"name": "cli", "state": {"waiting": {"reason": "ContainerCreating"}}, "lastState": {}}]
            }
        }));
        let events = [json!({
            "type": "Warning",
            "reason": "FailedScheduling",
            "involvedObject": {"kind": "Pod", "name": "control-a-x1"},
            "count": 4,
            "message": "0/1 nodes are available: 1 Insufficient cpu."
        })];
        let report = describe_failure(&deadline_job(), &pods, &events);
        assert!(
            report.contains("FailureTarget=True (DeadlineExceeded: Job was active"),
            "{report}"
        );
        assert!(
            report.contains("pod control-a-x1: phase=Pending"),
            "{report}"
        );
        assert!(
            report.contains("PodScheduled=False (Unschedulable"),
            "{report}"
        );
        assert!(
            report.contains("container cli: waiting, ContainerCreating; last: none"),
            "{report}"
        );
        assert!(
            report.contains("event Warning FailedScheduling Pod/control-a-x1 x4"),
            "{report}"
        );
        assert!(
            !report.contains("s3cret"),
            "an env value was shown: {report}"
        );
    }

    /// A Job its deadline stopped while its pod still ran: the report shows the container running
    /// since its start, and the restart before it as its last state.
    #[test]
    fn the_failure_report_shows_a_deadline_with_a_running_pod() {
        let pods = pods_of(json!({
            "metadata": {"name": "control-b-y2"},
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "True"}],
                "initContainerStatuses": [{"name": "seed", "state": {"terminated": {"reason": "Completed", "exitCode": 0}}}],
                "containerStatuses": [{
                    "name": "cli",
                    "state": {"running": {"startedAt": "2026-10-18T10:00:00Z"}},
                    "lastState": {"terminated": {"reason": "Error", "exitCode": 1, "message": "connection refused"}}
                }]
            }
        }));
        let report = describe_failure(&deadline_job(), &pods, &[]);
        assert!(
            report.contains("job: active=1 failed=? conditions: FailureTarget=True"),
            "{report}"
        );
        assert!(
            report.contains("pod control-b-y2: phase=Running conditions: Ready=True\n"),
            "{report}"
        );
        assert!(
            report.contains("init seed: terminated, Completed, exit 0; last: none"),
            "{report}"
        );
        assert!(
            report.contains("container cli: running, since 2026-10-18T10:00:00Z; last: terminated, Error, exit 1, connection refused"),
            "{report}"
        );
        assert!(report.contains("events: none"), "{report}");
        assert!(
            !report.contains("s3cret"),
            "an env value was shown: {report}"
        );
    }

    /// However many pods and events, and however long their messages, a report stays bounded.
    #[test]
    fn the_failure_report_is_bounded() {
        let long = "m".repeat(5000);
        let pod = json!({"metadata": {"name": "p"}, "status": {"phase": "Failed", "conditions": [{"type": "Ready", "status": "False", "message": long}]}});
        let pods = json!({"items": vec![pod; 50]});
        let events: Vec<Value> = (0..100)
            .map(|i| json!({"type": "Normal", "reason": format!("R{i}"), "message": long}))
            .collect();
        let report = describe_failure(&Value::Null, &pods, &events);
        assert!(report.len() < 3000, "{} bytes", report.len());
        assert!(report.contains("(47 more pods)"), "{report}");
        assert!(
            report.contains("R99") && !report.contains("R91 "),
            "{report}"
        );
        assert!(
            report.starts_with("job: active=? failed=? conditions: none\n"),
            "{report}"
        );
    }

    fn process(name: &str, built: bool) -> Value {
        json!({"name": name, "built": built})
    }

    fn command_line(command: &str, scope: &str, subject: &str) -> String {
        json!({
            "body": {
                "event_type": "OwnerCommandIssued",
                "payload": {"command": command, "scope": scope, "subject": subject}
            },
            "hash": "00"
        })
        .to_string()
    }

    /// The kill-switch drill's check: every brake must be on the exported stream, other lines and
    /// events are ignored, and a missing connection-scope kill switch reads as the seeded symptom.
    #[test]
    fn the_brake_check_names_each_missing_brake() {
        let expected = brakes_expected("kill1");
        let all: Vec<String> = expected
            .iter()
            .map(|(c, k, s)| command_line(c, k, s))
            .chain([
                "init: exporting".to_owned(),
                "{\"body\":{\"event_type\":\"StreamOpened\"}}".to_owned(),
            ])
            .collect();
        assert_eq!(brakes_missing(&all.join("\n"), &expected), None);
        let prefixed: Vec<String> = all
            .iter()
            .map(|l| format!("[pod/verify-1-abc/show-segment] {l}"))
            .collect();
        assert_eq!(
            brakes_missing(&prefixed.join("\n"), &expected),
            None,
            "job_logs' container prefix is skipped"
        );
        let without_connection: Vec<&String> =
            all.iter().filter(|l| !l.contains("connection")).collect();
        let joined = without_connection
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let missing = brakes_missing(&joined, &expected).unwrap();
        assert!(
            missing.contains(Drill::KillSwitch.seeded_symptom()),
            "{missing}"
        );
        assert_eq!(
            missing,
            "no kill_switch of scope connection (conn_alpaca_paper_01)"
        );
        let wrong_workspace = brakes_missing(&all.join("\n"), &brakes_expected("kill2")).unwrap();
        assert_eq!(wrong_workspace, "no kill_switch of scope workspace (kill2)");
        let pause_as_kill = command_line("kill_switch", "agent", "agent-a");
        let everything_but_pause: Vec<String> = expected
            .iter()
            .filter(|(c, _, _)| c != "pause")
            .map(|(c, k, s)| command_line(c, k, s))
            .chain([pause_as_kill])
            .collect();
        assert_eq!(
            brakes_missing(&everything_but_pause.join("\n"), &expected).as_deref(),
            Some("no pause of scope agent (agent-a)")
        );
    }

    /// Every brake the drill commits names exactly one scope flag, and the processes it takes down
    /// are the ones E21-34 names.
    #[test]
    fn the_brakes_reach_each_scope_once() {
        let args = brake_args("kill1", "$(MANDATE_JOURNAL_DSN)");
        assert_eq!(args.len(), brakes_expected("kill1").len());
        for (run, (command, scope, _)) in args.iter().zip(brakes_expected("kill1")) {
            let verb = if command == "pause" { "pause" } else { "kill" };
            assert_eq!(&run[..2], ["agent", verb]);
            let flag = match scope.as_str() {
                "agent" => "--agent",
                "connection" => "--connection",
                _ => "--workspace-wide",
            };
            let flags = ["--agent", "--connection", "--workspace-wide"];
            let named: Vec<&String> = run.iter().filter(|a| flags.contains(&a.as_str())).collect();
            assert_eq!(named, [flag], "{run:?}");
            assert!(
                run.windows(2).any(|w| w == ["--workspace", "kill1"]),
                "{run:?}"
            );
        }
        assert_eq!(
            KILL_SWITCH_DOWN,
            [
                "api",
                "deploy-manager",
                "exporter",
                "model-gateway",
                "control-plane"
            ]
        );
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
            7,
            "API, runtime, agent, research, membership, sign-in and web/ wait on their lanes: {skipped:?}"
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
        let gaps: Vec<&str> = parts
            .iter()
            .filter(|p| p.skipped.is_some())
            .map(|p| p.name)
            .collect();
        assert_eq!(
            gaps.len(),
            2,
            "every lane on main, the paper agent and research still wait on the post-R0 hook and \
             X1b-3 (DEC-2031): {gaps:?}"
        );
        assert!(gaps.contains(&RESEARCH_GATEWAY), "{gaps:?}");
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
        assert_eq!(
            waiting.len(),
            3,
            "beside those two gaps, only sign-in waits on the stub: {waiting:?}"
        );
        assert!(waiting.contains(&SIGNIN_PART), "{waiting:?}");
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
    fn the_deploy_manager_is_built_but_never_composed() -> Result<()> {
        let all = processes_in(&crate::repo_root()?)?;
        let manager = all.iter().find(|p| p["name"] == DEPLOY_MANAGER_PROCESS);
        assert!(
            manager.is_some_and(|p| p["built"] == true && !in_compose(p)),
            "the deployment manager is built, and has no Compose adapter (DEC-1172)"
        );
        let compose = compose_document(&crate::repo_root()?, &all)?;
        assert!(
            compose["services"].get(DEPLOY_MANAGER_PROCESS).is_none(),
            "the Compose file holds no deployment manager"
        );
        assert!(in_compose(&json!({"built": true})));
        assert!(!in_compose(&json!({"built": false})));
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
    fn the_runtime_is_ready_once_built_and_the_agent_still_waits_on_its_executor() {
        let list = [process("runtime", true), process("executor", false)];
        let parts = demo_parts(&list, "", false);
        let skipped = |prefix: &str| {
            parts
                .iter()
                .find(|p| p.name.starts_with(prefix))
                .map(|p| p.skipped.is_some())
        };
        assert_eq!(skipped("the agent runtime"), Some(false));
        assert_eq!(skipped("a paper agent"), Some(true));
        assert_eq!(
            parts
                .iter()
                .find(|p| p.name == RESEARCH_GATEWAY)
                .map(|p| p.skipped.is_some()),
            Some(true),
            "a deny-all runtime runs no research"
        );
        let unbuilt = demo_parts(&[process("runtime", false)], "", false);
        assert!(
            unbuilt
                .iter()
                .any(|p| p.name.starts_with("the agent runtime") && p.skipped.is_some())
        );
    }

    /// A runtime the built deployment manager starts is not called ready: the manager starts it
    /// only from a version-2 deployment (DEC-1171), and the demo's paper deployment is version 1
    /// until its connection is recorded, so no runtime pod runs in the cell yet.
    #[test]
    fn a_runtime_the_built_manager_starts_is_not_called_ready() {
        let runtime = json!({"name": "runtime", "built": true, "started_by": "deploy-manager"});
        let ready = |manager_built: bool| {
            let list = [runtime.clone(), process("deploy-manager", manager_built)];
            demo_parts(&list, "", false)
                .into_iter()
                .find(|p| p.name.starts_with("the agent runtime"))
                .map(|p| p.skipped.is_none())
        };
        assert_eq!(ready(true), Some(false));
        assert_eq!(
            ready(false),
            Some(true),
            "without the manager, the runtime is a Deployment"
        );
    }

    /// Every variable the generated Compose file requires is one the drill sets, so a service it
    /// never starts still cannot stop it (DEC-1013).
    #[test]
    fn the_compose_drill_sets_every_variable_the_compose_file_requires() -> Result<()> {
        let root = crate::repo_root()?;
        let compose = fs::read_to_string(root.join(COMPOSE_FILE))?;
        let required: std::collections::BTreeSet<&str> = compose
            .split("${")
            .skip(1)
            .filter_map(|rest| rest.split_once(":?").map(|(name, _)| name))
            .collect();
        assert!(required.contains("MANDATE_RUNTIME_AGENT"), "{required:?}");
        for runtime in [Some("mandate.local/runtime:0123456789ab"), None] {
            let env = compose_env(
                &root,
                "mandate.local/mandate:0123456789ab",
                "mandate.local/api:0123456789ab",
                runtime,
            )?;
            let set: std::collections::BTreeSet<&str> = env
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, _)| k.as_str())
                .collect();
            let missing: Vec<&&str> = required.iter().filter(|r| !set.contains(**r)).collect();
            assert!(missing.is_empty(), "the drill leaves {missing:?} unset");
        }
        let agent = compose_env(&root, "i", "a", None)?
            .into_iter()
            .find(|(k, _)| k == "MANDATE_RUNTIME_AGENT")
            .map(|(_, v)| v);
        assert_eq!(
            agent.as_deref(),
            Some("demo-agent"),
            "the k3d cell's deployment"
        );
        Ok(())
    }

    /// Both images built still run no paper agent and no research: the runtime proposes nothing on
    /// the deny-all wiring and the executor fills no paper order before X1b-3, so neither part is
    /// called ready, and the paper fill stays a named gap rather than a faked one (DEC-2031).
    #[test]
    fn both_images_built_still_call_no_paper_agent_or_research_ready() {
        let list = [process("runtime", true), process("executor", true)];
        let parts = demo_parts(&list, "", false);
        let skipped = |name: &str| {
            parts
                .iter()
                .find(|p| p.name.starts_with(name))
                .and_then(|p| p.skipped.clone())
        };
        assert!(
            skipped("a paper agent").is_some_and(|why| why.contains("X1b-3")),
            "{:?}",
            skipped("a paper agent")
        );
        assert!(skipped(RESEARCH_GATEWAY).is_some());
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
    fn an_import_counts_only_when_the_node_lists_the_exact_reference() {
        let listed = "docker.io/rancher/mirrored-pause:3.6\nmandate.local/api:3ab66e87ffdd\n";
        assert!(listed_holds(listed, "mandate.local/api:3ab66e87ffdd"));
        assert!(listed_holds(
            "  mandate.local/api:3ab66e87ffdd  \n",
            "mandate.local/api:3ab66e87ffdd"
        ));
        assert!(!listed_holds(listed, "mandate.local/api:3ab66e87ffd"));
        assert!(!listed_holds(listed, "mandate.local/runtime:3ab66e87ffdd"));
        assert!(!listed_holds(
            "mandate.local/api:3ab66e87ffdd-old\n",
            "mandate.local/api:3ab66e87ffdd"
        ));
        assert!(!listed_holds("", "mandate.local/api:3ab66e87ffdd"));
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
            format!("appOrigin: {DEMO_APP_ORIGIN}"),
        ] {
            assert!(
                api.contains(&wanted.as_str()),
                "k3d.yaml's api block lacks `{wanted}`"
            );
        }
        assert!(
            values.lines().any(|l| l == "deploymentTier: ci"),
            "k3d.yaml must set `deploymentTier: ci`, the one tier the API server and the stub \
             issuer beside it both take, and one the stub starts in (DEC-2100)"
        );
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
        let env = compose_env(&root, "x", "x", None)?;
        let set: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
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
                .any(|p| p.name.starts_with("the agent runtime") && p.skipped.is_none()),
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
