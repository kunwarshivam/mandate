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
     token, which OpenTofu mints; its sensitive outputs give the access key id and the secret
     (IAC's `deploy/tofu/README.md` names them).
   - `CLOUDFLARED_TUNNEL_TOKEN`: the tunnel's token, the sensitive OpenTofu output `tunnel_token` of
     `deploy/tofu/environments/staging`.

   ```sh
   export JOURNAL_ARCHIVE_ACCESS_KEY_ID="$(tofu -chdir=deploy/tofu/environments/staging output -raw <archive key id output>)"
   export JOURNAL_ARCHIVE_SECRET_KEY="$(tofu -chdir=deploy/tofu/environments/staging output -raw <archive secret output>)"
   export CLOUDFLARED_TUNNEL_TOKEN="$(tofu -chdir=deploy/tofu/environments/staging output -raw tunnel_token)"
   ```

## Bootstrap

```sh
cargo xtask gitops bootstrap staging
```

1. The command first checks everything:
   - flux is 2.9.6;
   - every Flux image is pinned by digest;
   - the three secrets are set and not empty;
   - `deploy/gitops/clusters/staging/sync.yaml` exists.
2. It prints the context it will use. Type that context's name to go on; anything else stops it
   with nothing applied.
3. It then:
   - installs Flux's source, kustomize and helm controllers and waits for them;
   - writes `ws-ws1/journal-archive-key` and `ws-ws1/cloudflared-tunnel`;
   - applies `sync.yaml`.

   It prints no secret.
4. Flux takes over. Within a few minutes it installs, in order:
   1. the CloudNativePG operator;
   2. the Barman Cloud plugin's certificates and the plugin;
   3. the admission policy controller;
   4. the journal's two instances;
   5. the workspace with its tunnel.

   Watch it:

   ```sh
   flux get all -A
   kubectl -n ws-ws1 get cluster,pods
   ```

## Rotating a secret

Set the new values in your shell, then run `cargo xtask gitops secrets staging`. It rewrites both
Secrets and nothing else, after the same confirmation. A pod reads a Secret only at start, so
restart the tunnel with `kubectl -n ws-ws1 rollout restart deployment/cloudflared`. The journal
picks up the new archive key on its next archive attempt.

## Upgrading Flux

In one pull request, change the pinned version and digests in:

- `xtask/src/gitops.rs` (the version and each controller's digest)
- `docs/dependencies.md` (the `flux` and Flux controller image rows)
- `.cursor/install.sh` (the download and its SHA-256)

Merge it, then run `bootstrap` again. Re-applying is idempotent.

## What this does not do

- It does not promote images. Staging follows `main` through the promotion pull requests, and
  production through pull requests that copy staging (DEC-2016).
- It does not make a workspace process start in `staging` without a release signature. Starting
  them there before any key is lane RS's change, after the founder's DEC-2605 rows 8 and 9.
