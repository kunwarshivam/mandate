# Bootstrapping a hosted cluster (the founder's runbook)

E21-58, [DEC-2018](../../docs/project/decisions/DEC-2018.md). Do this once per cluster: staging
first, production when you decide. After it, the cluster takes every change from `main` through
Flux, and nobody applies anything to it by hand. Agents never run any step here (DEC-2015 item 5),
and the command refuses to run in CI or without a terminal.

## The usual path: CI

Each run of lane IAC's `infra.yml` for an environment (dispatched from `main`, under its
`infra-<environment>` environment, after your approval) bootstraps the cluster after its apply,
with no step by hand: it opens its own address on the API firewall, fetches a short-lived
kubeconfig, installs or re-asserts Flux, writes the workload secrets from OpenTofu's outputs, and
applies `sync.yaml`, then closes the firewall again (DEC-2018 item 7). The rest of this page is the
break-glass path, from your own machine, for when CI cannot run.

## Before you start

1. **The cluster.** Lane IAC's OpenTofu (E21-56, `deploy/tofu/`) has made the DOKS cluster with two
   nodes (the journal's two instances must sit on separate nodes), the R2 archive bucket with
   bucket lock (DEC-2605 row 4), DNS, and the Cloudflare Tunnel and Access application (DEC-2011).
2. **Your machine.**
   - Its address must be in the control-plane firewall's `KUBE_API_ALLOWED_CIDRS`.
   - Its kubeconfig's current context must point at the cluster (`doctl kubernetes cluster
     kubeconfig save <cluster>`).
   - It needs flux 2.9.6 and kubectl 1.35. On x86_64 Linux, `bash .cursor/install.sh` installs
     flux, checked against its SHA-256. Elsewhere, install flux 2.9.6 yourself and check it against
     the release's `flux_2.9.6_checksums.txt`; `bootstrap` refuses any other version.
3. **The packages are public.** GHCR makes a new package private. After the first `main` run of
   `release.yml` that publishes the charts, set these packages public once in the repository's
   package settings. This costs nothing, as DEC-971 item 5 notes for the images:
   - `mandate/charts/mandate-backup-plugin-tls`
   - `mandate/charts/mandate-journal-db`
   - `mandate/charts/mandate-vault-server`
   - `mandate/charts/mandate-workspace`
   - the image packages

   Flux and the cluster read them anonymously.
4. **The secrets, in your shell only**, never from a file in the repository:
   - `JOURNAL_ARCHIVE_ACCESS_KEY_ID` and `JOURNAL_ARCHIVE_SECRET_KEY`: Barman's bucket-scoped R2
     token, which OpenTofu mints: its sensitive outputs `archive_access_key_id` and
     `archive_secret_access_key`.
   - `CLOUDFLARED_TUNNEL_TOKEN`: the tunnel's token, the sensitive OpenTofu output `tunnel_token` of
     `deploy/tofu/environments/staging`.
   - `JOURNAL_ARCHIVE_ENDPOINT`: the archive's R2 endpoint, the sensitive output `archive_endpoint`.
     It names your Cloudflare account, so it is never in git; the bootstrap writes it into the
     ConfigMap `flux-system/journal-archive-endpoint`, which the journal reads (DEC-2017 item 6).

   ```sh
   export JOURNAL_ARCHIVE_ACCESS_KEY_ID="$(tofu -chdir=deploy/tofu/environments/staging output -raw archive_access_key_id)"
   export JOURNAL_ARCHIVE_SECRET_KEY="$(tofu -chdir=deploy/tofu/environments/staging output -raw archive_secret_access_key)"
   export CLOUDFLARED_TUNNEL_TOKEN="$(tofu -chdir=deploy/tofu/environments/staging output -raw tunnel_token)"
   export JOURNAL_ARCHIVE_ENDPOINT="$(tofu -chdir=deploy/tofu/environments/staging output -raw archive_endpoint)"
   ```

## Bootstrap

```sh
cargo xtask gitops bootstrap staging
```

1. The command first checks everything:
   - flux is 2.9.6;
   - every Flux image is pinned by digest;
   - the three secrets are set and not empty, and the endpoint is an R2 account endpoint;
   - `deploy/gitops/clusters/staging/sync.yaml` exists.
2. It prints the context it will use. Type that context's name to go on; anything else stops it
   with nothing applied.
3. It then:
   - installs Flux's source, kustomize and helm controllers and waits for them;
   - writes `ws-ws1/journal-archive-key`, `ws-ws1/cloudflared-tunnel` and
     `flux-system/journal-archive-endpoint`, stopping with an error if any write fails;
   - applies `sync.yaml`.

   It prints no secret.
4. Flux takes over. Within a few minutes it installs, in order:
   1. the CloudNativePG operator;
   2. the Barman Cloud plugin's certificates and the plugin;
   3. the admission policy controller;
   4. the journal's two instances;
   5. the workspace with its tunnel.
   6. in staging only, the vault, which the same run then initializes (below).

   Watch it:

   ```sh
   flux get all -A
   kubectl -n ws-ws1 get cluster,pods
   ```

## Staging's vault

Staging runs the workspace vault; production does not until you decide how it unseals
([DEC-2051](../../docs/project/decisions/DEC-2051.md), DEC-2021 item 6). You chose to keep its
recovery keys in the cluster ([DEC-2052](../../docs/project/decisions/DEC-2052.md)), so staging's
apply sets it up with no step of yours: after the bootstrap it makes the seal key if there is none,
waits for the vault's pod, and initializes it once, storing the init answer's three recovery keys
in the Secret `vault-recovery/vault-init`, which no workload can read, and revoking the root token
it returned, which it never stores, before the run ends. Its log reads only `vault-init: seal key:
created|exists`, `vault: initialized|already initialized`, `recovery keys: stored` and `root token:
revoked`. Every later apply finds it done and changes nothing.

The recovery keys are needed only to make a new root token, for provisioning (DEC-2022) or to rotate
the seal, never to unseal: the vault unseals itself on every restart. To read them, as cluster
admin:

```sh
kubectl -n vault-recovery get secret vault-init -o jsonpath='{.data.init\.json}' | base64 -d
```

Never paste them into an issue, a CI job or a chat. The break-glass bootstrap from your machine
still asks for a seal key when none exists (`openssl rand -hex 32`, not shown as you type; if you
stop it there with Ctrl-C, run `stty echo`), and keeps one that does.

If you ever provision by hand rather than through the apply (DEC-2630), pipe `mandate vault
provision --operator-token-stdin --secrets-namespace ws-ws1 --kubernetes-host https://<IP>:<port>`,
the `kubernetes` Service's ClusterIP and port in `default`, which it requires there, straight into
`kubectl apply -f -`: its output holds every process's secret id, and redirected to a file it
leaves them there at your umask. It refuses a terminal on either end.

## Staging's founder

Staging's web app signs you in only once your identity-provider subject is linked to the founder's
principal, and lets you act only once that principal is the workspace's founder
([DEC-2114](../../docs/project/decisions/DEC-2114.md)). None of it is a step of yours on the
cluster. The link is the apply's; the mount and the grant are two edits you make yourself, in this
order, because granting the founder's membership is your own act, not an agent's:

1. **The link.** Add the `infra-staging` secret `FOUNDER_OIDC_SUBJECT` (your user id in staging's
   Supabase project) and run the staging apply. After the vault is provisioned, it runs the
   `mandate` binary it built, `identity link`, with your subject on standard input only, and applies
   the one line it writes, `sha256:<digest> <founder ULID>`, as the ConfigMap `subject-links` in
   `ws-ws1`. Without the secret it applies the ConfigMap empty, so the secret alone decides who is
   linked. When the link changed and the API server runs, it restarts it, since the API reads the
   file once at start. Its log reads `founding: linked` or `founding: no founder subject is set:
   nobody is linked`, then `, unchanged` or `; api restarted`. The subject is masked and never
   reaches the cluster, a log, or a file left behind. The founder's ULID is `founding.member` in
   `clusters/staging/workspace.yaml`. Rotating it, or removing the secret, is a rerun. If the
   secret is ever unset by mistake, the next apply unlinks you (its log says `nobody is linked`)
   and, once the API restarts, staging refuses every sign-in: set the secret again and rerun. The
   restart is `kubectl rollout restart`, an annotation on the API's pod template that Flux leaves
   alone, since no HelmRelease here sets `driftDetection`; the next Helm upgrade drops it with one
   extra rollout. If drift detection is ever turned on for the workspace release, it must ignore
   `/spec/template/metadata/annotations/kubectl.kubernetes.io~1restartedAt` on `deployment/api`, or
   Flux reverts the restart.
2. **The mount (your edit, after step 1).** Wait for a staging apply that ran with the secret set
   and logged `founding: linked`, so the ConfigMap holds your link. Then, in
   `clusters/staging/workspace.yaml`, add `subjectLinks: subject-links` under `api`, and merge it.
   The API server then reads the link and signs you in. Not before: the mount is not optional, so
   until an apply has written the ConfigMap, the API's new pod cannot start.
3. **The grant (your edit, after step 2).** Wait until the `mandate` image staging runs, as
   promoted in `clusters/staging/images.yaml`, has `workspace found --open`. Then, in the same
   file, set `founding.enabled: true`, update the comment above `founding` so it no longer says
   founding is off, and merge it. An older image refuses `--open`, and the release fails. Once it
   is on, the workspace chart's `founding` hook runs on every apply: the first opens the
   workspace's control stream if it is empty and writes your founding grant; every later one finds
   it and commits nothing. Two runs racing on an empty stream never open it twice: the one that
   loses fails closed, committing nothing, and the hook's retry finds the stream opened and founds
   it (or finds the grant). Verify's `founding` Job line confirms it.

If the control stream already holds records and no membership, the grant is refused
`workspace_not_new`: the release fails, and verify's `founding` Job line names only that code. The
fix is a fresh workspace ULID in `api.workspace`, through a gitops change and a decision; no journal
row is ever deleted.

## When the journal does not install

The journal's release waits for `flux-system/journal-archive-endpoint` and never falls back to an
address of its own, so a cluster bootstrapped without it installs the operator and plugin but holds
the journal and the workspace. `flux get helmreleases -A` then shows `journal` not ready, naming
the missing ConfigMap. The bootstrap stops with an error when it cannot write it; fix the cause
and run it again, or let the next infra run re-assert it. A healthy journal reports
`ContinuousArchiving` true: `kubectl -n ws-ws1 get cluster journal -o jsonpath='{.status.conditions}'`.

## When R2's addresses change

The journal reaches R2 only at the two /32s DEC-2017 item 1 resolved, so if Cloudflare moves them,
archiving stops (`ContinuousArchiving` goes false) and nothing opens wider. Resolve the endpoint
again (`dig +short <account>.r2.cloudflarestorage.com`) and change, in one reviewed pull request:

- `archive.egress.cidrs` in `deploy/gitops/clusters/staging/journal.yaml` and
  `deploy/gitops/clusters/production/journal.yaml`;
- `R2_EGRESS` in `deploy/tests/test_gitops.py`, the checks' own copy;
- DEC-2017 item 1, by a new decision that records the new addresses.

## Rotating a secret

Set the new values in your shell, then run `cargo xtask gitops secrets staging`. It rewrites both
Secrets and the endpoint's ConfigMap and nothing else, after the same confirmation. A pod reads a Secret only at start, so
restart the tunnel with `kubectl -n ws-ws1 rollout restart deployment/cloudflared`, or
`deployment/api` once the API runs, since the tunnel then runs in the API's pod (DEC-2056). The journal
picks up the new archive key on its next archive attempt.

## Upgrading Flux

In one pull request, change the pinned version and digests in:

- `xtask/src/gitops.rs` (the version and each controller's digest)
- `docs/dependencies.md` (the `flux` and Flux controller image rows)
- `.cursor/install.sh` (the download and its SHA-256)

Merge it, then run `bootstrap` again. Re-applying is idempotent.

## What this does not do

- It never replaces staging's vault seal key once one exists.

- It does not promote images. Staging follows `main` through the promotion pull requests, and
  production through pull requests that copy staging (DEC-2016).
- It does not make a workspace process start in `staging` without a release signature. Starting
  them there before any key is lane RS's change, after the founder's DEC-2605 rows 8 and 9.
