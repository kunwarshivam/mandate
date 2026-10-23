# deploy/cell/: the workspace chart, the Compose file, and the local cell

One workspace deployment, packaged once (E21-36), and a cell on a developer machine or a CI
runner that proves it (E21-37), with no cloud account, no broker credential and no broker host
([DEC-967](../../docs/project/decisions/DEC-967.md) item 3). The choices are recorded in
[DEC-993](../../docs/project/decisions/DEC-993.md); the plan is the
[managed-cells brief](../../docs/project/tasks/managed-cells.md), lanes K and C.

| Path | What it is |
|---|---|
| `charts/workspace/processes.json` | **The process list** (infrastructure §3.1): every process type with its kind, image, database role, egress column, placement and resources. The chart and the Compose file are both rendered from it |
| `charts/workspace/` | The Helm chart `mandate-workspace`: one Deployment or Job per process type whose image is named, the migration hook, and the network policies |
| `charts/journal-db/` | The journal's Postgres under the CloudNativePG operator: a primary and a synchronous standby, its roles (`files/roles.sql`), one credential Secret per process type, and the WAL archive and base backups through the Barman Cloud plugin (E21-41, [DEC-1050](../../docs/project/decisions/DEC-1050.md)). Installed before the workspace chart |
| `charts/backup-plugin-tls/` | The two certificates the Barman Cloud plugin and the operator authenticate each other with, made in the cluster in place of cert-manager (DEC-1050 item 3) |
| `charts/vault/` | The workspace vault: OpenBao, pinned by digest, in server mode on raft storage over TLS 1.3, reached only by the processes whose egress names it, unsealed by hand ([DEC-2019](../../docs/project/decisions/DEC-2019.md)). Installed before the workspace chart |
| `charts/archive-store/` | The local cell's stand-in for the S1 endpoint the journal archives to: the pinned test store, in its own namespace, reachable only by the journal's archive clients (DEC-1050 item 5). Never in cell #1 |
| `compose/compose.json` | The same processes for one host (infrastructure §3.2, hybrid), generated: `cargo xtask cell generate --write` |
| `values/` | One values file per cluster: `k3d.yaml` (this cell), `k3s.yaml` (cell #1), `managed.yaml` (a managed cluster). Only values differ ; cell #1's own settings on top of `k3s.yaml`, and its bring-up, are in [`../hosted/`](../hosted/README.md) |
| `images/mandate.Dockerfile` | The interim `mandate` image the cell builds from the change; lane R2 (E21-35) replaces it |
| `images/deploy-manager.Dockerfile` | The deployment manager's image (`mandate-deployd`, DEC-1172), built beside it the same way |
| `../tests/test_cell.py` | The chart's and the Compose file's invariants, each shown failing on a seeded bug |
| `../tests/test_artifacts.py` | The shared artifact store: each process mounts it as it may, or not at all, each shown failing on a seeded bug |
| `../tests/test_vault.py` | The vault chart's invariants, each shown failing on a seeded bug |
| `../tests/test_journal_db.py` | The journal-db chart's invariants and those of the two charts beside it, each shown failing on a seeded bug |

## What the chart guarantees

Each is a test in `deploy/tests/test_cell.py`, run with helm by `cargo xtask ci cell`:

- one pod per process type, and a process renders only when its image is named; once the
  deployment manager's image is named, the runtime and the executor are not Deployments but its pod
  templates (a ConfigMap mounted read-only), the pods the chart would otherwise build, which only
  the manager starts (DEC-1170, DEC-1172);
- the runtime, the executor and the data service share a node by required pod affinity on the
  workspace (§3.2), since the market-data channel is host-local;
- every container has CPU and memory limits, and the executor's and the scheduler's requests
  equal them (§3.3);
- a namespace-wide default-deny policy, then exactly each process type's §3.1 egress: DNS, the
  journal's Postgres, the vault and the model gateway by pod label, and each external target's
  CIDRs from values, empty by default; DEC-821's token host only for the token-exchange process;
  the API server (`kube-api`, its endpoint addresses and `kubeApiPort`) only for the deployment
  manager, which `cargo xtask cell up` points at the cell's own endpoint;
- every pod starts behind an `egress-gate` init container: nothing of the process runs until the
  pod's own egress is shown filtered, since k3s's policy controller programs a new pod only seconds
  after it starts, and the gate fails closed after a minute (DEC-993 item 8); the deployment
  manager's policy allows the API server every other gate waits on, so its gate waits on the
  cluster resolver's metrics port (9153), which its policy denies;
- no inbound connection to the data plane; only the model gateway accepts its callers;
- non-root, a read-only root filesystem, no capability, no privilege escalation, the runtime's
  seccomp profile, and no service-account token, in every container but the deployment manager's;
- the deployment manager alone holds an API token, its own ServiceAccount's, whose one Role in the
  namespace creates, gets, lists and deletes pods and nothing else: no Secret, no exec, no
  ClusterRole or cluster scope; the pod templates it starts mount no token; and a
  ValidatingAdmissionPolicy, matched to its ServiceAccount alone and failing closed, admits only
  pods shaped like those templates: their images, Secrets, labels and egress labels, no token,
  nothing privileged and nothing of the host;
- secrets only as mounted files: no Secret in the chart, no secret in any values file, no
  environment variable from a Secret, and no password in a DSN (DEC-822 item 4);
- migrations as a pre-install and pre-upgrade hook Job under the migration-owner role, never at a
  process's start (OPS-13);
- `environment` is `paper` or `backtest` only, and every image is pinned by digest or is the
  cell's own `mandate.local/` build;
- `MANDATE_DEPLOYMENT_TIER` on every process comes from the `deploymentTier` value alone:
  `production` unless a values file says otherwise, `ci` in the local cell, `dev` on hosted cell #1
  (DEC-971 item 2, [DEC-2001](../../docs/project/decisions/DEC-2001.md) item 6);
- the cell's one ingress is `cloudflared` dialing out to Cloudflare's tunnel edge, off in the
  local cell: no Service, NodePort, LoadBalancer or Ingress, and the tunnel token a mounted file;
  where the API runs, it runs as a second container of the API's pod and reaches the API's
  loopback listener (DEC-2056);
- no k3s-only, provider-only or cloud-only API: the k3s and managed renderings are identical, and
  k3d's differs only by the tunnel.
- one shared artifact store (mandate documents, backtest reports), the claim `artifacts` at
  `/var/lib/mandate/artifacts`, which survives a pod's restart and an upgrade (it goes with the
  release on uninstall, since a kept claim would make a reinstall refuse it). The control runs
  (`MANDATE_ARTIFACTS`) mount it read-write; the API server (`MANDATE_API_ARTIFACTS`) only reads it,
  read-only, though its pod's volume is writable for the layout step; the runtime and the deployment
  manager (`MANDATE_ARTIFACTS`) read it, read-only at both the volume and the mount; and no other
  process mounts it. Before a process starts, its pod's `artifacts-layout` step makes `sha256/` and
  `tmp/` (the control run's and the API server's) or waits two minutes for them, read-only (a reader's): the API server refuses
  a root without `sha256/`. The modes are fixed in the chart by process type, never values; size,
  class and access mode are values (`artifacts`). The claim, and every mount of it, renders only
  with the API server, its one long-running writer, since local-path binds on first use and `helm
  --wait` waits for that, and a reader alone would wait for a layout no writer makes; so a cell
  without the API server's image gives the control run and the runtime no store. The deployment
  manager's read-only access applies once that process is in `processes.json`. It is
  ReadWriteOnce on local-path, so every pod that mounts it runs on one node; a hosted cell names a
  class that can, or the store moves to E21-40's object store. The Compose file has none yet
  (`deploy/tests/test_artifacts.py`).

And the journal-db chart, in `deploy/tests/test_journal_db.py` (E21-41, DEC-1050):

- two instances or more, `synchronous_commit = on`, and one synchronous standby every commit waits
  for, still waited for with that standby down (`dataDurability: required`), in the template, so no
  value can weaken it;
- the WAL archiver is the Barman Cloud plugin, writing to the chart's ObjectStore with a key no
  database role holds, and a scheduled base backup goes through the same plugin;
- the journal's pods reach only the cluster DNS, the Kubernetes API by `/32`, each other on 5432
  and 8000, and the archive's endpoint, by CIDRs of /24 or narrower or, in the cluster, by pod
  label; a plain-HTTP endpoint only for a Service in the cluster;
- instances on separate nodes are a scheduling rule; on one node the Cluster's
  `mandate.dev/failure-domain` annotation says the standby is no second failure domain, and
  `cell up` prints it;
- the local test store accepts only the journal's archive clients and reaches nothing, and the
  plugin's two certificates are self-signed TLS pairs that record when they end.

## The local cell

Needs docker, plus k3d, helm and kubeconform at the versions in `.github/workflows/ci.yml`
(`bash .cursor/install.sh` installs them on x86_64 Linux; on a Mac, see "On a laptop").

```bash
cargo xtask cell up               # build the image, start k3d, install the operator, its backup plugin, the test store, journal-db, vault, workspace
cargo xtask cell drill            # boot, journal, egress, compose, restart, kill-switch, admission, standby-down, archive, executor
cargo xtask cell drill --seeded   # each drill with its bug planted; fails if one still holds
cargo xtask cell down
cargo xtask ci cell               # what CI runs: chart checks, up, drills
cargo xtask ci cell-seeded        # and beside it: up, then the same drills, seeded (DEC-464)
cargo xtask ci cell-upgrade       # and beside it: up, then demo --no-web, an upgrade in place (DEC-464)
cargo xtask ci cell-journal       # and beside it: up, then standby-down and archive, plain and seeded
cargo xtask ci cell-failover      # and beside it: up, then restart (a failover with the standby), plain and seeded
cargo xtask ci cell-kill-switch   # and beside it: up, then kill-switch and admission, plain and seeded (DEC-2602)
cargo xtask ci cell-executor      # and beside it: up, then executor, plain and seeded (DEC-2615, DEC-2035)
cargo xtask ci cell-vault         # and beside it: up, then vault, plain and seeded (DEC-2616, DEC-2022)
```

`cell up` is idempotent: it reuses a running cluster and upgrades the releases. kubectl is the one
in the k3s node: `docker exec k3d-mandate-cell-server-0 kubectl get pods -n ws-ws1`. Behind a
TLS-inspecting proxy, set `MANDATE_CELL_CA_BUNDLE` to the proxy's CA bundle so the node can pull.
These are local-cell results, never staging's (DEC-967 item 5).

### On a laptop

Only linux/x86_64 can build the static `mandate` binary the cell runs, since no cross toolchain is
a dependency kind (DEC-1040). On any other host, `cell up` and `cell demo` pull lane R2's signed
image instead, named by digest in `MANDATE_CELL_IMAGE` (the cell checks the digest, not yet the
signature) (`<registry>/mandate@sha256:<64 hex>`), and
skip the build. Until R2 (E21-35) publishes linux/arm64 and amd64 images there is nothing to pull,
and both commands stop and say so. A pinned image is used on linux/x86_64 too.

On an Apple-silicon Mac: Docker Desktop with at least 6 GB of memory for its VM, and the darwin-arm64
builds of the tools, each checked against its SHA-256:

| Tool | Download | SHA-256 |
|---|---|---|
| k3d 5.9.0 | `github.com/k3d-io/k3d/releases/download/v5.9.0/k3d-darwin-arm64` | `fe106541d5d0a3f18debcd4d432a16f8c0ce3e6ddc06f8fbb6f696a122313e00` |
| helm 4.3.0 | `get.helm.sh/helm-v4.3.0-darwin-arm64.tar.gz` | `d3870437e1e95b67f8edbde964156c84a26503f560821d40c542441658934fba` |
| kubeconform 0.8.0 | `github.com/yannh/kubeconform/releases/download/v0.8.0/kubeconform-darwin-arm64.tar.gz` | `f84f4dfbebf4a6b0b230385fa065a39ea35e02608c2b50d025dcf64775a69d67` |

The `compose` drill and `cell demo`'s web part run the same way there; `web/` needs Node.js as
below.

### The demo

`cargo xtask cell demo` is the whole local cell on a laptop, with paper and demo data only and no
credential. It runs `cell up`, opens the demo workspace (`01KDEM0000000000000000000W`, the one the
cell's API server hosts) and verifies its control stream in the cell, and prints each part as `ready` or `skipped` with the lane it waits on. Then it runs `web/`
at http://127.0.0.1:4317 in the foreground (Ctrl-C stops it; the cell keeps running). With
`--no-web` it stops after the summary; CI runs it that way at the end of the `cell` job, which also
upgrades every release in place.

| Part | Ready when |
|---|---|
| The cell: k3d, CloudNativePG, the migration Job | Always |
| The demo workspace, its control stream verified | Always |
| The workspace API server, through a port-forward | Lane A1 (E21-31) marks `api` built in `processes.json` |
| The agent runtime, on the deny-all wiring until the post-R0 hook | `runtime` is built in `processes.json` and `cell up` built its image (DEC-1013); with the deployment manager built, the manager starts it, which it does once it reads a deployment's account from `AgentDeployed` version 2 (DEC-1171) |
| Sign-in as the demo owner, through the local stub issuer | Lane SI's stub issuer (E21-52) and the server's key-set tier check (E21-53) are both on `main` |
| A paper agent on rh-sim and recorded Alpaca fixtures | `runtime` and `executor` are built (the executor's interim image runs its fixture paper broker, DEC-2031), the runtime proposes orders once the post-R0 hook wires its ports (DEC-1013), and the executor takes them (#1736) and fills them on the fixture once slice X1b-3 of E21-30 lands; until all three hold the part stays a named gap |
| The founding membership | Lane ID1's `mandate membership` gesture is in the CLI |
| `web/` against the cell's API | The API server is ready; until then `web/` runs on its own fixtures |

The API server listens only on its pod's loopback, so nothing in the cluster reaches it until the
tunnel's route lands. The demo forwards http://127.0.0.1:8787 to it with the machine's own `kubectl`,
or prints the `kubectl port-forward` command when there is none, and checks that `/v1` answers with
the one 401. The forward outlives the demo until its pod goes or it is stopped; a later demo
reuses one that still answers.

Sign-in runs through lane SI's stub issuer (DEC-2100, DEC-1025).
- It runs as a sidecar of the API pod on its loopback (`127.0.0.1:8090`), and the server starts
  only once the stub answers.
- It runs only in a dev or ci cell and only from a local build, so no hosted values file can name
  it.
- `cell up` runs it once lane SI's `crates/mandate-stub-issuer` and the server's E21-53 check are
  both in the checkout. It then links the demo owner (`01KDEM0000000000000000000P`, subject
  `demo-owner`) with `mandate identity link` and mounts the result as the `subject-links`
  ConfigMap.
- `cell demo` also forwards 127.0.0.1:8090 to the stub. It then asks the stub for a token and opens
  a session with it through `POST /v1/sessions`, which must answer 204 with the session cookie.
- Until both pieces are in, no stub runs, no links are mounted, every sign-in is refused, and the
  demo names the sign-in step as skipped.
- Once that session opens, `cell demo` runs `web/` with `npm run dev:cell` at
  https://localhost:4317, the API's app origin, reading the demo workspace through
  http://localhost:8787 as the demo owner (DEC-2406). Under `next dev` only, `web/`'s server asks
  the stub for `demo-owner`'s token, and the browser opens its own session with it. Next's
  `--experimental-https` makes a local certificate with mkcert the first time, which may ask to
  trust its CA. Without sign-in, `web/` runs on its own fixtures at http://127.0.0.1:4317, as before.

The session signing key is made in the pod's memory at each
start. The interim `api` image carries a CA bundle found on the building host (`SSL_CERT_FILE`, else
the usual Debian, Fedora, openSUSE or Alpine path), without which the server refuses to start; when `cell up` cannot install the workspace release, it prints the API pod's
state, logs and the namespace's events. A cell that pulls its `mandate` image (`MANDATE_CELL_IMAGE`) has no API image yet, so it
skips the API's parts.

A part whose lane is not on `main` is named as skipped, never faked. `web/` needs Node.js at the
release in `web/.nvmrc` and npm; the first run does `npm ci`.

### The demo's scenario

`cargo xtask cell demo --scenario demo` (E21-54, DEC-2002) runs the demo's story on the cell in
order, the story's six steps with the owner's exit as step 6, before the kill switch (step 7), with
paper and demo data only and no credential. Each gesture whose pieces are on `main` runs in the cell, in one Job of the `control` process, and passes only when it printed
what its success prints. Each missing link prints as `gap` (DEC-2004): what of it is already on
`main`, citing its PRs, and every lane it still waits on, with that lane's story, its open PR when
there is one, and what the lane still has to land. A gap never fails the run and is never faked.
Each step's heading reads `ready`, `partial` or `skipped` with the lanes it waits on, and the
report closes with every wait grouped by lane. The run ends, in the same pod, by exporting the
workspace's control stream and verifying it by journal spec §11 against the artifact store its
gestures wrote, so every artifact a record names is checked. Each run uses a fresh workspace, a
ULID made from the run id (`01KDEM<run id>000000000D`), founded for the demo owner, the member the
stub issuer's `demo-owner` subject is linked to; before the gestures, the scenario points the API
and the deployment manager at it (`helm upgrade --reuse-values --set api.workspace=…`) and waits
for both to roll out, so every step, through the CLI, the API or `web/`, acts on one workspace
(DEC-2010). Step 5 registers the run's settings as the rule set, runs
`mandate-backtest-runner` on its committed demo inputs and records the payload it stores, in the
same pod, so the closing verify checks the record's report (DEC-2023).

The run's own report is the up-to-date list; on 2026-10-19, after slice 11, it read:

| Step | Today | Runs | Waits on |
|---|---|---|---|
| 1. Sign in, see the workspace | partial | `workspace open` (paper), `workspace found` for the demo owner; the cell's API refusing a read of the run's workspace with no session (the one 401); the demo owner's sign-in through the stub issuer (204 and its cookie); and the read with that session, whose agents list the demo's agent (200) | W2: the same in `web/` |
| 2. Deploy under a confirmed envelope | partial | `config register`, `model register`, `version create`, `version confirm` and `agent deploy` of `demo-spy`, each code shown then typed | W2 (E11-9): the same from `web/`; X (E21-30): the executor's connect check, so `connection record` records the paper connection and the deployment is written at version 2 (DEC-1171), which the manager starts |
| 3. Research, through the gateway's recorded fake | partial | In the same owner's second workspace, `<workspace>r`: `workspace open` (paper) and `workspace found`, `model register` of `quant.ma_crossover` and of `llm.research_agent` from the recorded bundle's entry (`--entry`, its hash recomputed), then `version create`, `version confirm` and `agent deploy` of `demo-research` on `deploy/cell/demo/research-mandate.json`, each code shown then typed, so it is the only agent on its control stream (DEC-2247) | X (E21-30): the executor's connect check, so its deployment is written at version 2 and the manager starts its runtime, which runs research over the bundle its image carries (DEC-2246) |
| 4. Sized, gated, asked, approved, filled, protected | partial | `approvals list` (empty until the runtime asks) | X (E21-30): the connect check, so the runtime starts, and P5 (E21-2, the post-R0 hook): the runtime sizes and asks; L1 (E9-4) and W2: step-up for the approval; X (E21-30): paper fills on the fixture in slice X1b-3; the executor already takes the runtime's orders (#1736) and runs in the cell on its fixture paper broker (DEC-2031) |
| 5. A backtest of the same version | partial | `config register --kind rule_set` (the run's settings), `mandate-backtest-runner run` on its committed demo inputs, `backtest record` of the payload it stores, which the closing verify checks, and the owner's signed-in read of it through the API (completed, with the stored report) | W2: web/'s report screen |
| 6. The owner's exit at a confirmed floor | partial | `agent exit demo-spy` of SPY with a confirmed bid, bid size and floor, its code shown then typed, recorded before the closing export and verify | X (E21-30): the connect check, so the executor started for the deployment hands the exit to its core, priced at the confirmed floor (#1736); X (E21-30) and P5 (E21-2): the paper fill on the fixture in slice X1b-3, out of the position step 4 fills |
| 7. The kill switch | partial | A second agent, `demo-web`, on its own envelope (QQQ's snapshot registered, its version created and confirmed after step 5's record, then deployed, each code shown then typed); `demo-web`'s kill switch pressed in `web/` after the gestures' Job (lane W2's `kill_switch_from_web`), its command read back with the owner's session from the run's control stream, accepted (its `KillSwitchActivated` waits on a runtime) (not run on a machine without npm, `web/node_modules`, Playwright's Chromium or openssl, unless `MANDATE_DEMO_WEB_REQUIRED` is set, which makes it fail; that switch alone still reads an unreachable API as not run, so a job that must run the gesture sets it with `MANDATE_CELL_REQUIRED`); `agent kill --agent demo-spy` through the CLI (agent scope), recorded before the closing export and verify (web/'s kill switch comes after them, so the read stands in for the verify) | X (E21-30): the connect check, so the deployment is written at version 2 and the manager starts the processes that stop on it (DEC-1171, DEC-2031) |

The demo's envelope is `deploy/cell/demo/mandate.json`: paper, SPY on the moving-average crossover,
every opening asked, protection on. `demo-web`'s is `deploy/cell/demo/web-mandate.json`, the same
envelope on QQQ (`web-instrument.json`): V-006 refuses a second agent on the account an instrument
another already pins ([DEC-2036](../../docs/project/decisions/DEC-2036.md)). CI's `cell-demo` job, which no required check depends on, runs
the scenario on every change (`cargo xtask ci cell-demo`). Behind a TLS-inspecting proxy, set
`MANDATE_CELL_CA_BUNDLE` to the proxy's CA bundle first.

For staging, `cargo xtask cell demo --scenario demo --dry-run --image <ref@sha256:...> --workspace
<ULID> [--namespace <ns>]` renders the gestures' Job and its inputs' ConfigMap with `helm template`
alone, from staging's values (`deploy/cell/values/managed.yaml`, then
`deploy/gitops/clusters/staging/images.yaml`), in `ws-ws1` unless `--namespace` names another, and
prints them one JSON object a line. It reads no kubeconfig, reaches no cluster and applies nothing.
The image must be a release pinned by its digest, since staging admits only signed release images,
and the workspace is the fixed one staging's API hosts. Running the demo against staging is a later
slice: it needs staging's sign-in (the stub issuer is refused there) and a fresh agent per run in
that one workspace.

### The drills

| Drill | What holds | Its seeded bug |
|---|---|---|
| `boot` | The migration hook succeeded, and every object of the release, each network policy included, is in the cell | The default-deny policy deleted |
| `journal` | The CLI opens a control stream in the cell's journal, and a Job exports it and verifies it by journal spec §11 | One byte of the segment changed before it is verified |
| `compose` | The Compose file on the host's docker: Postgres initializes the roles, the migration applies, the CLI opens a control stream and commits each scope's kill switch and an agent pause through `docker compose run control agent …` (E21-34), and its export verifies and holds every one | The first event's hash changed before it is verified |
| `restart` | A control stream written before the journal's Postgres primary pod is deleted still exports and verifies by journal spec §11 once CloudNativePG has the cluster Ready again | A stream id the restart never held is verified, which must read as lost (`export_stream_empty`): it proves the drill sees a lost stream, not a loss the restart itself caused |
| `kill-switch` | With every pod of `api`, `deploy-manager`, `exporter`, `model-gateway` and `control-plane` gone (scaled to zero, then back), `mandate agent kill` at agent, connection and workspace scope and `mandate agent pause` commit through one-off `control` Jobs, and the control stream exports, verifies and holds each (OPS-4, E21-34). Each stream owner's `KillSwitchActivated` is reported `skipped` until the runtime and executor run in the cell ([DEC-2000](../../docs/project/decisions/DEC-2000.md) item 6) | The connection-scope kill switch never committed: the check must name `no kill_switch of scope connection` |
| `admission` | The hosted clusters' kill-switch admission policies (`deploy/gitops/base/kill-switch-admission.yaml`), applied as they are with a signature webhook that is down and fails closed: the founder's kill-switch Job is admitted and its pod made; an unlabelled pod, a person's or a service account's `control` pod, a service account's control Job, the control label a service account gives its suspended Job, a service account's change to a person's control Job, a labelled Deployment's pod, a CronJob's control Job (another API group's included), the label added to a pod later, and a changed image or an ephemeral container in the kill switch's pod are each refused; a second workspace's namespace is covered by its label; and a finished kill-switch Job is removed by foreground deletion ([DEC-2045](../../docs/project/decisions/DEC-2045.md), [DEC-2046](../../docs/project/decisions/DEC-2046.md)) | The Job policy firing on update only for a newly control label: the check must name `admitted a service account's change to a person's control Job` |
| `egress` | From a pod of every process type, each live trading host (`agent.robinhood.com`, `api.robinhood.com`, `api.alpaca.markets`) fails at the network layer, and the journal is reachable exactly from the rendered processes whose egress names Postgres (OPS-5) | Every network policy deleted |
| `standby-down` | With the synchronous standby fenced, an append neither commits nor becomes visible on the primary, and its Job ends on its deadline; once the standby is back and streams synchronously, a new append commits and every stream written verifies by journal spec §11 (OPS-2, E21-41) | The cluster's durability set to `preferred`, so the primary commits alone: `committed with the standby fenced` |
| `vault` | `mandate vault provision` sets the cell's vault up (DEC-2022), then from a pod labelled as the executor each AppRole logs in and reads: a runtime nothing, an executor exactly its own connection's token and not `drill-other`'s, the model gateway its key; and no `mandate-*` policy holds `*` or `+`. It runs in `cell-kill-switch`, after the kill switch's drill | The runtime's policy rewritten to read every connection's token through `+`: the check must name `wildcard in mandate-runtime` |
| `archive` | A closed WAL segment is reported archived by Postgres, a base backup completes, and Barman's own tools, reading the store with the archive's key, list the backup and fetch the segment back (E21-41) | The archive pointed at another bucket of the same store, which Postgres cannot tell: `not in the archive` |
| `executor` | An agent deployed on a recorded paper connection, and the deployment manager starting that account's executor until its pod runs and its process answers `livez` (E21-30, [DEC-2035](../../docs/project/decisions/DEC-2035.md)). Not `readyz`: the executor is ready only after a scheduler tick, and the cell builds no scheduler yet, so none is ever journaled there. Three one-off Jobs: the executor's connect check on its in-process fixture broker, which prints the passing `ConnectionChecked`; the workspace opened and founded and the record's code shown; then the record committed and the demo's mandate registered, created, confirmed and deployed, so `AgentDeployed` is version 2 (journal spec §9.24 rule 201). No credential, no broker host | The connect check skipped, so the connection is never recorded and the agent deploys at version 1: `no executor pod` |

A seeded run counts as caught only when its report names the planted bug's own symptom
(`rehash_mismatch`, a live host reached, the deleted policy missing, `export_stream_empty`, the
missing kill switch, a commit with the standby fenced, a segment not in the archive, no executor
pod), never any
failure. `standby-down` and `archive` run in CI's `cell-journal` job and `restart`, which with the
synchronous standby is a failover and the old primary's rejoin, in `cell-failover`, each beside
`cell`, so each stays within ten minutes (DEC-464). A part of a drill that cannot run yet is reported
as skipped, never as held. In the
seeded egress run, with the workspace's policies deleted, the probes open TCP connections to the
live trading hosts from the runner, to prove the drill can see a leak; they send no byte, so no
request reaches a broker (AGENTS.md rule 8).

These join as their processes and slices land: each stream owner's `KillSwitchActivated` in the
kill-switch drill, kills at each step of submission, two executors for one account (OPS-3), the
upgrade drill (OPS-7), and E21-41's point-in-time restore (OPS-8) and failover with intents in
flight (DEC-434 item 15) (DEC-1050 item 8).

Residuals of the local cell, disclosed: its two journal instances share its one node, so the
standby is no second failure domain (the Cluster says so); and its archive's traffic to the test
store is plain HTTP inside the cluster.

## The vault

`charts/vault/` runs the workspace's OpenBao (E21-32, [DEC-2019](../../docs/project/decisions/DEC-2019.md)).
In `deploy/tests/test_vault.py`:

- server mode on raft storage alone, from a command the template fixes: no value adds an argument
  or an environment variable, so the dev server cannot be turned on (DEC-977 item 6);
- one TLS 1.3 listener, from a server certificate signed by a CA made for this vault, whose key is
  never stored; clients mount the CA certificate from the ConfigMap `vault-ca`;
- only the workspace's pods labelled `mandate.dev/egress-vault` reach it, on 8200, and it reaches
  nothing;
- sealed is not ready, and no probe restarts it, since a restart seals it;
- hardened as the workspace's pods are, its image by digest, and no secret in a value or the
  environment;
- the workspace chart's vault egress selects exactly its pods and port, and the processes it admits
  are exactly those whose egress names the vault;
- no seal block and no seal key outside staging, and staging's static seal reads its key from a
  read-only mounted file, never the environment (DEC-2021).

`cargo xtask cell up` installs it, initializes it once with one unseal key, kept with the root
token in `target/cell/vault-init.json` (mode 0600; this cell holds fake secrets only), unseals it,
and refuses a vault on any storage but raft. `cell down` discards both.

A production cell is initialized and unsealed by the founder, by hand (DEC-977 item 5):

```bash
helm upgrade --install vault deploy/cell/charts/vault -n ws-ws1 -f deploy/cell/values/vault-managed.yaml
kubectl exec -it -n ws-ws1 vault-0 -- bao operator init -key-shares=3 -key-threshold=2
kubectl exec -it -n ws-ws1 vault-0 -- bao operator unseal    # twice, two different shares, typed
```

The shares and the root token are kept offline, never in the repository, CI, a file the cluster can
read, or an environment variable. After every restart of `vault-0`, it is unsealed again.

Staging (paper only) unseals itself (DEC-2605 row 10, [DEC-2021](../../docs/project/decisions/DEC-2021.md)):
the founder's local `cargo xtask gitops bootstrap staging` creates the Secret `vault-seal-key` (its
`key` entry, typed or pasted at the founder's terminal and piped to kubectl's stdin; no CI job can
reach staging's API, DEC-2015 item 4) before the chart's first install, and the chart mounts it as
a file. Flux installs the release (DEC-2015). Once, after it reports the vault Running:

```bash
helm upgrade --install vault deploy/cell/charts/vault -n ws-ws1 -f deploy/cell/values/vault-staging.yaml
kubectl exec -it -n ws-ws1 vault-0 -- bao operator init -recovery-shares=3 -recovery-threshold=2
```

The recovery keys and the root token are kept offline as the shares are; they are needed only to
make a root token or rotate the seal, never to unseal. The schema refuses `seal.type: static`
anywhere but `environment: staging`. The chart keeps its TLS pair by Helm's `lookup`, so it is
installed by Helm itself (or Flux's helm-controller), not rendered by `helm template` (DEC-2019,
residuals).

Rotating staging's seal key (DEC-2021 item 5) is a one-off with the founder at the terminal; the
chart refuses a second key today, so it waits on a chart change that renders OpenBao's
`previous_key` for one restart (backlog, E21-32 *Left*). Its steps then:

1. Pick a new `keyId` (`staging-2`). Add the new key to `vault-seal-key` beside the old one, through
   the founder's bootstrap, never in git, a value, or a log.
2. Set `seal.keyId` to the new name and the old one as the previous key in
   `deploy/cell/values/vault-staging.yaml`; Flux restarts `vault-0`, which unseals with the old key
   and re-wraps its root key under the new one.
3. Once `bao status` shows the vault unsealed under the new key id, remove the previous key from
   the values, then the old key from the Secret.

The `vault` drill provisions the cell's vault and proves each identity's reach (DEC-2022). Not yet:
each process's secret id as a mounted file (V3, with the executor's wiring, E21-9), and the
vault's audit device.

## The Compose file

For one host: `docker compose -f deploy/cell/compose/compose.json up` with `MANDATE_WORKSPACE`
and `MANDATE_IMAGE_MANDATE` (pinned by digest) set, and under `deploy/cell/compose/secrets/`
(never committed) `postgres-superuser` and one `journal-role-<role>.pgpass` per role holding
`*:*:mandate:<role>:<password>`, owned by uid 999 with mode 0600, which libpq and sqlx require.
Every Compose service runs as that one non-root user, the library Postgres image's, so its
first-start script and each process can read the same files.
`docker compose run control workspace open …` runs the CLI. The `api` service also needs
`MANDATE_IMAGE_API`, the settings `MANDATE_API_ORG`, `MANDATE_API_WORKSPACE`, `MANDATE_API_ISSUER`,
`MANDATE_API_AUDIENCE`, `MANDATE_API_JWKS_URL`, `MANDATE_API_APP_ORIGIN` and
`MANDATE_DEPLOYMENT_TIER`, and `secrets/api-session-signing-key`: at least 32 random bytes, owned by
uid 999 with mode 0600. It listens on its container's loopback and mounts no subject links, so
every sign-in is refused there until a later change adds both.

The deployment manager is not in the Compose file (`"compose": false` in `processes.json`): it
drives Kubernetes only (DEC-1172), so on one host the runtime and executor are Compose services.

`docker compose run` of `control` or `journal-verify` needs a writable `/work`, given with
`-v <dir>:/work`, as `cargo xtask cell drill compose` does.

Residuals on one host, disclosed: every service shares the one internal network, so Compose does
not isolate processes from each other or refuse inbound connections between them as the chart's
policies do; Compose cannot filter egress by destination, so a process with no
external target sits on the internal network alone, which has no route out, and one with an
external target also joins `egress`, which the host firewall must narrow; and the journal's
connections on the internal bridge network are not TLS.
