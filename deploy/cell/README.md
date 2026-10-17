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
| `charts/archive-store/` | The local cell's stand-in for the S1 endpoint the journal archives to: the pinned test store, in its own namespace, reachable only by the journal's archive clients (DEC-1050 item 5). Never in cell #1 |
| `compose/compose.json` | The same processes for one host (infrastructure §3.2, hybrid), generated: `cargo xtask cell generate --write` |
| `values/` | One values file per cluster: `k3d.yaml` (this cell), `k3s.yaml` (cell #1), `managed.yaml` (a managed cluster). Only values differ |
| `images/mandate.Dockerfile` | The interim `mandate` image the cell builds from the change; lane R2 (E21-35) replaces it |
| `../tests/test_cell.py` | The chart's and the Compose file's invariants, each shown failing on a seeded bug |
| `../tests/test_journal_db.py` | The journal-db chart's invariants and those of the two charts beside it, each shown failing on a seeded bug |

## What the chart guarantees

Each is a test in `deploy/tests/test_cell.py`, run with helm by `cargo xtask ci cell`:

- one pod per process type, and a process renders only when its image is named;
- the runtime, the executor and the data service share a node by required pod affinity on the
  workspace (§3.2), since the market-data channel is host-local;
- every container has CPU and memory limits, and the executor's and the scheduler's requests
  equal them (§3.3);
- a namespace-wide default-deny policy, then exactly each process type's §3.1 egress: DNS, the
  journal's Postgres, the vault and the model gateway by pod label, and each external target's
  CIDRs from values, empty by default; DEC-821's token host only for the token-exchange process;
- every pod starts behind an `egress-gate` init container: nothing of the process runs until the
  pod's own egress is shown filtered, since k3s's policy controller programs a new pod only seconds
  after it starts, and the gate fails closed after a minute (DEC-993 item 8);
- no inbound connection to the data plane; only the model gateway accepts its callers;
- non-root, a read-only root filesystem, no capability, no privilege escalation, the runtime's
  seccomp profile, and no service-account token, in every container;
- secrets only as mounted files: no Secret in the chart, no secret in any values file, no
  environment variable from a Secret, and no password in a DSN (DEC-822 item 4);
- migrations as a pre-install and pre-upgrade hook Job under the migration-owner role, never at a
  process's start (OPS-13);
- `environment` is `paper` or `backtest` only, and every image is pinned by digest or is the
  cell's own `mandate.local/` build;
- the cell's one ingress is a `cloudflared` pod dialing out to Cloudflare's tunnel edge, off in the
  local cell: no Service, NodePort, LoadBalancer or Ingress, and the tunnel token a mounted file;
- no k3s-only, provider-only or cloud-only API: the k3s and managed renderings are identical, and
  k3d's differs only by the tunnel.

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
cargo xtask cell up               # build the image, start k3d, install the operator, its backup plugin, the test store, journal-db, workspace
cargo xtask cell drill            # boot, journal, egress, compose, restart, kill-switch, standby-down, archive
cargo xtask cell drill --seeded   # each drill with its bug planted; fails if one still holds
cargo xtask cell down
cargo xtask ci cell               # what CI runs: chart checks, up, drills, seeded drills
cargo xtask ci cell-journal       # and beside it: up, then standby-down and archive, plain and seeded
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
credential. It runs `cell up`, opens the `demo` workspace and verifies its control stream in the
cell, and prints each part as `ready` or `skipped` with the lane it waits on. Then it runs `web/`
at http://127.0.0.1:4317 in the foreground (Ctrl-C stops it; the cell keeps running). With
`--no-web` it stops after the summary; CI runs it that way at the end of the `cell` job, which also
upgrades every release in place.

| Part | Ready when |
|---|---|
| The cell: k3d, CloudNativePG, the migration Job | Always |
| The demo workspace, its control stream verified | Always |
| The workspace API server, through a port-forward | Lane A1 (E21-31) marks `api` built in `processes.json` |
| A paper agent on rh-sim and recorded Alpaca fixtures | Lane P1 (#1451) marks `runtime` and `executor` built |
| The founding membership | Lane ID1's `mandate membership` gesture is in the CLI |
| `web/` against the cell's API | The API server is ready; until then `web/` runs on its own fixtures |

A part whose lane is not on `main` is named as skipped, never faked. `web/` needs Node.js at the
release in `web/.nvmrc` and npm; the first run does `npm ci`.

### The drills

| Drill | What holds | Its seeded bug |
|---|---|---|
| `boot` | The migration hook succeeded, and every object of the release, each network policy included, is in the cell | The default-deny policy deleted |
| `journal` | The CLI opens a control stream in the cell's journal, and a Job exports it and verifies it by journal spec §11 | One byte of the segment changed before it is verified |
| `compose` | The Compose file on the host's docker: Postgres initializes the roles, the migration applies, the CLI opens a control stream, and its export verifies | The first event's hash changed before it is verified |
| `restart` | A control stream written before the journal's Postgres primary pod is deleted still exports and verifies by journal spec §11 once CloudNativePG has the cluster Ready again | A stream id the restart never held is verified, which must read as lost (`export_stream_empty`): it proves the drill sees a lost stream, not a loss the restart itself caused |
| `kill-switch` | Skipped, by name, until P1's runtime and executor images and O4's `mandate agent kill` are on `main` (OPS-4) | None until it runs |
| `egress` | From a pod of every process type, each live trading host (`agent.robinhood.com`, `api.robinhood.com`, `api.alpaca.markets`) fails at the network layer, and the journal is reachable exactly from the rendered processes whose egress names Postgres (OPS-5) | Every network policy deleted |
| `standby-down` | With the synchronous standby fenced, an append neither commits nor becomes visible on the primary, and its Job ends on its deadline; once the standby is back and streams synchronously, a new append commits and every stream written verifies by journal spec §11 (OPS-2, E21-41) | The cluster's durability set to `preferred`, so the primary commits alone: `committed with the standby fenced` |
| `archive` | A closed WAL segment is reported archived by Postgres, a base backup completes, and Barman's own tools, reading the store with the archive's key, list the backup and fetch the segment back (E21-41) | The archive pointed at another bucket of the same store, which Postgres cannot tell: `not in the archive` |

A seeded run counts as caught only when its report names the planted bug's own symptom
(`rehash_mismatch`, a live host reached, the deleted policy missing, `export_stream_empty`, a commit
with the standby fenced, a segment not in the archive), never any failure. `standby-down` and
`archive` run in CI's `cell-journal` job, beside `cell`, so each stays within ten minutes
(DEC-464). A skipped drill is reported as skipped, never as held. In the
seeded egress run, with the workspace's policies deleted, the probes open TCP connections to the
live trading hosts from the runner, to prove the drill can see a leak; they send no byte, so no
request reaches a broker (AGENTS.md rule 8).

The kill switch run with the exporter, API and deployment manager down (OPS-4), kills at each step of
submission, two executors for one account (OPS-3), the upgrade drill (OPS-7), and E21-41's
point-in-time restore (OPS-8) and failover with intents in flight (DEC-434 item 15) join as their
processes and slices land (DEC-1050 item 8).

Residuals of the local cell, disclosed: its two journal instances share its one node, so the
standby is no second failure domain (the Cluster says so); and its archive's traffic to the test
store is plain HTTP inside the cluster.

## The Compose file

For one host: `docker compose -f deploy/cell/compose/compose.json up` with `MANDATE_WORKSPACE`
and `MANDATE_IMAGE_MANDATE` (pinned by digest) set, and under `deploy/cell/compose/secrets/`
(never committed) `postgres-superuser` and one `journal-role-<role>.pgpass` per role holding
`*:*:mandate:<role>:<password>`, owned by uid 999 with mode 0600, which libpq and sqlx require.
Every Compose service runs as that one non-root user, the library Postgres image's, so its
first-start script and each process can read the same files.
`docker compose run control workspace open …` runs the CLI.

`docker compose run` of `control` or `journal-verify` needs a writable `/work`, given with
`-v <dir>:/work`, as `cargo xtask cell drill compose` does.

Residuals on one host, disclosed: every service shares the one internal network, so Compose does
not isolate processes from each other or refuse inbound connections between them as the chart's
policies do; Compose cannot filter egress by destination, so a process with no
external target sits on the internal network alone, which has no route out, and one with an
external target also joins `egress`, which the host firewall must narrow; and the journal's
connections on the internal bridge network are not TLS.
