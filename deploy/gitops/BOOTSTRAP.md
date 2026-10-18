# Bootstrapping a hosted cluster (the founder's runbook)

E21-58, [DEC-2018](../../docs/project/decisions/DEC-2018.md). This is done once per cluster:
staging first, production when the founder decides. After it, the cluster takes every change
from `main` through Flux, and nobody applies anything to it by hand. Agents never run any step
here (DEC-2015 item 5).

## Before you start

1. **The cluster.** Lane IAC's OpenTofu has made the DOKS cluster, with at least two nodes (the
   journal's two instances must sit on separate nodes), the Spaces bucket, DNS and the Cloudflare
   Tunnel and Access application (DEC-2011).
2. **The packages are public.** GHCR makes a new package private. After the first `main` run of
   `release.yml` that publishes the charts, set these packages public once in the repository's
   package settings. This costs nothing, as DEC-971 item 5 notes for the images:
   - `mandate/charts/mandate-backup-plugin-tls`
   - `mandate/charts/mandate-journal-db`
   - `mandate/charts/mandate-workspace`
   - the image packages

   Flux reads them anonymously.
3. **The GitHub environment.** In the repository's settings, under Environments, create (or open)
   `staging` and:
   - add yourself as **required reviewer**;
   - limit deployment branches to `main`;
   - add these **environment secrets**, never repository secrets:

| Secret | What it is | Where it comes from |
|---|---|---|
| `KUBECONFIG_DATA` | The cluster's kubeconfig | IAC's output (`doctl kubernetes cluster kubeconfig show`, or the OpenTofu output) |
| `JOURNAL_ARCHIVE_ACCESS_KEY_ID` | The Spaces access key the journal archives with | A Spaces key limited to the journal's bucket |
| `JOURNAL_ARCHIVE_SECRET_KEY` | Its secret | Same |
| `CLOUDFLARED_TUNNEL_TOKEN` | The tunnel's token | Cloudflare Zero Trust, the tunnel IAC made |

## Bootstrap

1. In GitHub, open Actions, then **gitops-bootstrap**, then **Run workflow**. Choose branch
   `main`, environment `staging`, and action `bootstrap`.
2. Approve the run when GitHub asks; you are the environment's reviewer.
3. The job installs flux 2.9.6 and kubectl 1.35.9, each checked against its SHA-256, and then:
   - installs Flux's source, kustomize and helm controllers, each pinned by digest, and waits for
     them;
   - writes `ws-ws1/journal-archive-key` and `ws-ws1/cloudflared-tunnel` from the environment's
     secrets;
   - applies `deploy/gitops/clusters/staging/sync.yaml`.

   It prints no secret.
4. Flux takes over. Within a few minutes it installs, in order, the CloudNativePG operator, the
   Barman Cloud plugin's certificates and the plugin, the journal's two instances, and the
   workspace with its tunnel. Watch it with read-only access, from a machine holding the
   kubeconfig:

   ```sh
   flux get all -A
   kubectl -n ws-ws1 get cluster,pods
   ```

## Rotating a secret

Change the environment secret, then run **gitops-bootstrap** with action `secrets`. It rewrites
both Secrets and nothing else. A pod reads a Secret only at start, so roll the ones that use it:
the tunnel with `kubectl -n ws-ws1 rollout restart deployment/cloudflared`,
and the journal through CloudNativePG, which picks the new archive key up on its next archive
attempt.

## Upgrading Flux

Change the pinned version and digests in `xtask/src/gitops.rs`, `docs/dependencies.md`,
`.github/workflows/ci.yml`, `.github/workflows/gitops-bootstrap.yml` and `.cursor/install.sh` in
one pull request. Then run `bootstrap` again; re-applying is idempotent.

## What this does not do

- It does not make the workspace's processes start. The overlays set the tier to `staging` or
  `production`, and a binary starts there only once it carries the release key's signature.
  `RELEASE_KEYS` is empty until the founder decides DEC-971 item 7.
- It does not promote images. Staging follows `main` through the promotion pull requests, and
  production through pull requests that copy staging (DEC-2016).
