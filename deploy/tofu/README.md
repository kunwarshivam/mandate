# `deploy/tofu/`: the hosted environments as code (E21-56)

OpenTofu makes the hosted staging and production environments on DigitalOcean and Cloudflare
([DEC-2011](../../docs/project/decisions/DEC-2011.md),
[ADR-0006](../../docs/adr/0006-hosted-infrastructure-as-code.md)). It stops at a reachable, empty
cluster with its buckets, DNS and Access. Lane GO's GitOps installs everything inside the cluster.

| Path | What it makes |
|---|---|
| `modules/network` | The VPC and the node firewall, which admits only the VPC's own range |
| `modules/doks` | The DOKS cluster: one pool of two nodes, the version pinned, auto-upgrade off, the control plane fenced |
| `modules/spaces` | OpenTofu's state bucket (adopted), and nothing else |
| `modules/cloudflare` | The tunnel, the API host's proxied record, Access on it, and the journal archive's R2 bucket with its lock rule |
| `modules/budget` | The monthly estimate, which the plan refuses past $65 per environment |
| `modules/cell` | The DigitalOcean modules wired into one environment |
| `environments/staging`, `environments/production` | A root each: backend, encrypted state, values |

**Who runs what.** Pull requests run only checks that need no credential (`cargo xtask ci tofu`,
[DEC-2013](../../docs/project/decisions/DEC-2013.md)). `plan` and `apply` run only in
`.github/workflows/infra.yml`, from `main`, when the founder dispatches it and approves the job
([DEC-2014](../../docs/project/decisions/DEC-2014.md)). Agents never run either, never see a token,
and never dispatch the workflow (AGENTS.md rules 7 and 8).

## Runbook: what the founder creates

Nothing below goes in the repository, a chat or an issue. Each value goes straight into the
GitHub environment named, as a secret or a variable. Do it once per environment; production can
wait until staging is up.

### 1. DigitalOcean

1. **A personal access token, the secret `DIGITAL_OCEAN_API_KEY`.** In **API → Tokens → Generate
   New Token**,
   choose **Custom Scopes**, an expiry of 90 days, and only these scopes:

   | Resource | Scopes | Why |
   |---|---|---|
   | VPC | create, read, update, delete | `modules/network`'s VPC |
   | Firewall | create, read, update, delete | the node firewall |
   | Tag | create, read, delete | the node tag the firewall applies to |
   | Kubernetes | create, read, update, delete | the cluster and its node pool; read also lists the offered versions |
   | Sizes | read | the budget's node price |
   | Regions | read | the cluster's region |

   No droplet, database, load-balancer, domain, project, billing or account scope. If
   DigitalOcean names a scope differently, take the closest match: a missing scope shows up as a
   403 that names it in the first plan, never as a partial apply.
2. **A Spaces access key, `SPACES_ACCESS_KEY_ID` and `SPACES_SECRET_ACCESS_KEY`.** This is the
   state backend's credential, and it is the founder's: you make it, and you make the state
   buckets in step 3, by hand (DEC-2012 item 5). In **Spaces Object Storage → Access Keys →
   Create Access Key**, choose **Limited Access**, named `mandate-tofu-<env>`, with **Read/Write/
   Delete** on that environment's state bucket alone, so a staging key cannot touch production's
   state. Make the bucket first (step 3), then the key. If the first plan's adoption of the bucket
   fails for want of bucket-level rights, widen the key to Full Access and say so to lane IAC.
3. **The state buckets, by hand, once each.** In **Spaces Object Storage → Create Bucket**: region
   **New York (NYC3)**, CDN off, file listing **Restricted**, named exactly
   `mandate-tofu-state-staging` (and later `mandate-tofu-state-production`). The first apply
   adopts each one, makes it versioned, and protects it from deletion.
4. **The billing alert at 80% of the cap.** In **Billing → Billing Alerts**, set an alert at
   **$120** a month (DEC-2011 item 2). No API sets this, so it is the founder's step. The plan
   itself refuses an estimate over $65 per environment.

### 2. Cloudflare

0. **R2 enabled on the account.** In **R2 Object Storage**, subscribe to R2 once (it asks for a
   payment method even on the free tier). Until it is enabled, the first apply fails at the
   archive bucket.
1. **An API token, `CF_INFRA_API_TOKEN`.** In **My Profile → API Tokens → Create Token → Custom
   token**, named `mandate-tofu`, with only these permissions:

   | Scope | Permission | Level | Why |
   |---|---|---|---|
   | Account | Cloudflare Tunnel | Edit | the tunnel and its ingress; reading its token |
   | Account | Access: Apps and Policies | Edit | the Access application and its policy |
   | Zone | DNS | Edit | the API host's record |
   | Zone | Zone | Read | finding the zone by name |
   | Account | Workers R2 Storage | Edit | the journal archive's R2 bucket and its lock rule |
   | Account | Account API Tokens | Edit | minting Barman's bucket-scoped R2 token (DEC-2029) |

   Account resources: only the account that holds `owlhead.ai`. Zone resources: only
   `owlhead.ai`. Set an expiry and, if you like, restrict client IPs to none, since the runner's
   address changes. This is not `web-deploy.yml`'s `CLOUDFLARE_API_TOKEN`, which stays as it is.
   R2 Storage Edit can also lift the archive's lock rule: R2's lock is governance-equivalent
   (DEC-973 item 5). Account API Tokens Edit makes the token admin-level: it can mint any token,
   which you accepted for the automation (DEC-2029). Keep it in the environment alone. Once the
   preflight lands (DEC-2029 item 4), each run checks the token's permissions first and names
   any that are missing.
2. **The account id, `CF_ACCOUNT_ID`.** From the account's overview page (32 hex characters). It is
   not a secret, so it is a variable.
3. **Barman's R2 token is not yours to make.** OpenTofu mints it, scoped to the archive bucket with
   Object Read and Write only, and hands it on as sensitive outputs (DEC-2029).
4. **Before production only:** `api.owlhead.ai` already has DEC-822's hand-made record. The
   production apply refuses to create a second one, so delete that record (or tell lane IAC to
   adopt it) when production is to take the name. Staging uses `api-staging.owlhead.ai`, which
   is free.

### 3. GitHub environments

In **Settings → Environments**, `infra-staging` already exists (the founder made it on 2026-10-18,
with `DIGITAL_OCEAN_API_KEY`). Check it, and later create `infra-production` the same way. Each
needs:

- **Deployment branches and tags:** selected branches, `main` only.
- **Required reviewers:** the founder. Leave "Prevent self-review" off, since the founder both
  dispatches and approves.
- **Environment secrets:**

  | Name | Value |
  |---|---|
  | `DIGITAL_OCEAN_API_KEY` | §1.1 (already created in `infra-staging`) |
  | `SPACES_ACCESS_KEY_ID` | §1.2, that environment's key id |
  | `SPACES_SECRET_ACCESS_KEY` | §1.2, that environment's secret |
  | `CF_INFRA_API_TOKEN` | §2.1 |
  | `TOFU_STATE_PASSPHRASE` | at least 32 random characters, for example `openssl rand -base64 48`. It is stretched with PBKDF2-SHA-512 at 600,000 iterations into an AES-GCM key. **Keep a copy in your password manager.** Without it the state cannot be read, and every later plan fails. Use a different one per environment |

- **Environment variables:**

  | Name | Example | What |
  |---|---|---|
  | `CF_ACCOUNT_ID` | `0123456789abcdef0123456789abcdef` | §2.2 |
  | `KUBE_API_ALLOWED_CIDRS` | `["203.0.113.7/32"]` | the only addresses the cluster's API admits, as a JSON list, each no wider than /24; your own address for kubectl |
  | `ACCESS_ALLOWED_EMAILS` | `["you@example.com"]` | who Access lets through to the API host, as a JSON list of full addresses |

No repository-level secret or variable is used, so no other workflow can read these.

### 4. The first run

1. **Actions → infra → Run workflow**, branch `main`, environment `staging`, action `plan`.
   Approve the job. Read the plan:
   - one VPC, a tag, a firewall and a cluster;
   - the state bucket, imported;
   - a tunnel and its configuration;
   - a DNS record, and an Access application and policy;
   - an R2 bucket and its lock rule;
   - a `terraform_data` for the budget.

   Nothing is destroyed.
   - If it fails with "DOKS does not offer 1.35.1-do.0; it offers: …", tell lane IAC the list.
     The pin changes in a one-line pull request.
2. Dispatch again with action `apply` on the same commit, and approve it. The job plans again after
   your approval, prints that plan, and applies exactly it, so the apply reflects the commit and
   the cloud as they are when the job runs. Dispatch `apply` only at a commit you have read, and
   read the apply job's printed plan afterwards. A first cluster takes about ten minutes.
3. **Reaching the cluster:** from an address in `KUBE_API_ALLOWED_CIDRS`, run
   `doctl kubernetes cluster kubeconfig save mandate-staging` on your own machine.
4. **Handing over to lane GO (E21-58):** the outputs GO needs that are not secret are the cluster
   name `mandate-<env>`, the archive bucket `mandate-journal-archive-<env>` and its endpoint
   `https://<account id>.r2.cloudflarestorage.com`, and the tunnel name `mandate-<env>`, all
   fixed by the code. The tunnel token is a sensitive output that `infra.yml` never prints.
   - Read it once on your own machine, for example in the Zero Trust dashboard under the tunnel.
   - From an address in `KUBE_API_ALLOWED_CIDRS`, run lane GO's `cargo xtask gitops bootstrap
     staging` (its runbook is `deploy/gitops/BOOTSTRAP.md`, #1651). It writes, through kubectl's
     stdin, the Secrets `ws-ws1/cloudflared-tunnel` (key `token`, the tunnel token) and
     `ws-ws1/journal-archive-key` (keys `ACCESS_KEY_ID` and `ACCESS_SECRET_KEY`, the minted R2
     token's outputs). Nothing goes into git or into `infra-staging`. Once the automated bootstrap
     lands (DEC-2028), `infra.yml`'s apply does this itself and this step goes away.
   - Flux pulls from then on, so neither CI nor `apply` needs the cluster's API again.

## What this does not guard, said plainly

- DOKS manages its own worker firewall, and firewalls add up, so a Service of type
  `LoadBalancer` or `NodePort` would be reachable. Lane GO creates none. The nodes keep public
  addresses for egress.
- The archive's R2 lock is governance-equivalent: the Cloudflare token can lift it (DEC-973 item
  5). It holds against Barman's own token and accidents, which is enough for the WAL archive,
  since journal spec §6.2 does not cover it. No §6.2 object ever goes to R2 or to the Spaces state
  bucket.
- The state holds the tunnel token, encrypted with `TOFU_STATE_PASSPHRASE`. Whoever holds that
  passphrase and the Spaces key can read it.
- The two nodes are in one region, one failure domain (DEC-995 item 6).
- The workflow offers no `destroy`. The buckets have `prevent_destroy`. Tearing an environment
  down is a reviewed change, not a button.
