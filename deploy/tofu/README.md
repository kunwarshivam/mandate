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

All of it is two tokens, one passphrase, two console steps and an approval per run
([DEC-2030](../../docs/project/decisions/DEC-2030.md)). Nothing below goes in the repository, a
chat or an issue. Each value goes straight into the GitHub environment as a secret. Do it once per
environment; production can wait until staging is up. **Actions → infra → Run workflow** with
action `preflight` checks all of it and prints a checklist of what is still missing, by name,
never a value.

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
   | Spaces Keys | create, read, delete | each run mints its own Spaces key, makes the state bucket if it is missing, and deletes the key (DEC-2030 item 4; DEC-2605 row 13) |

   No droplet, database, load-balancer, domain, project, billing or account scope. The preflight
   probes the read scopes and Spaces Keys create, the run's own mint, and names each one missing.
   Spaces Keys delete is checked when the run deletes its key, and by the preflight only when an
   earlier run left a key to delete; a missing delete scope fails the run's last step by name. A
   missing write scope shows up as a 403 that names it in the first apply.
2. **No Spaces key and no bucket by hand.** Each run mints `mandate-infra-<env>-<run id>`, makes
   `mandate-tofu-state-<env>` in NYC3 if it is missing, and deletes the key when it ends. The first
   apply adopts the bucket, makes it versioned, and protects it from deletion.
3. **Console: the billing alert at 80% of the cap.** In **Billing → Billing Alerts**, set an alert
   at **$120** a month (DEC-2011 item 2). No API sets or reads this, so the preflight only reminds
   you. The plan itself refuses an estimate over $65 per environment.

### 2. Cloudflare

1. **Console: R2 enabled on the account.** In **R2 Object Storage**, subscribe to R2 once (it asks
   for a payment method even on the free tier). The preflight says whether it is.
2. **An API token, `CF_INFRA_API_TOKEN`.** In **My Profile → API Tokens → Create Token → Custom
   token**, named `mandate-tofu`, with only these permissions:

   | Scope | Permission | Level | Why |
   |---|---|---|---|
   | Account | Cloudflare Tunnel | Edit | the tunnel and its ingress; reading its token |
   | Account | Access: Apps and Policies | Edit | the Access application and its policy |
   | Zone | DNS | Edit | the API host's record |
   | Zone | Zone | Read | finding the zone by name |
   | Account | Workers R2 Storage | Edit | the journal archive's R2 bucket and its lock rule |
   | Account | Account API Tokens | Edit | minting Barman's bucket-scoped R2 token (DEC-2029), and letting the preflight read this token's own permissions |

   Account resources: only the account that holds `owlhead.ai`, so the token sees exactly one
   account, whose id the plan reads itself. Zone resources: only `owlhead.ai`. Set an expiry and,
   if you like, restrict client IPs to none, since the runner's address changes. This is not
   `web-deploy.yml`'s `CLOUDFLARE_API_TOKEN`, which stays as it is. R2 Storage Edit can also lift
   the archive's lock rule: R2's lock is governance-equivalent (DEC-973 item 5). Account API
   Tokens Edit makes the token admin-level: it can mint any token, which you accepted for the
   automation (DEC-2029; DEC-2605 row 12). Keep it in the environment alone.
3. **Nothing else to copy.** The account id is read from the token's one account. Access admits
   the members of your Cloudflare account, so no email is configured. Barman's R2 token is
   minted by OpenTofu (DEC-2029).
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
  | `CF_INFRA_API_TOKEN` | §2.2 |
  | `TOFU_STATE_PASSPHRASE` | at least 32 random characters, for example `openssl rand -base64 48`. It is stretched with PBKDF2-SHA-512 at 600,000 iterations into an AES-GCM key. **Keep a copy in your password manager.** Without it the state cannot be read, and every later plan fails. Use a different one per environment |

- **Environment secret, production only:** `ACCESS_ALLOWED_EMAILS`, for example
  `["you@example.com"]`: who Access lets through to production's API host, as a JSON list of full
  addresses. It is a secret so the public log never shows it. Staging needs none: its Access
  admits the members of your Cloudflare account (DEC-2030 item 2).
- **Environment secret, production only:** `KUBE_API_ALLOWED_CIDRS`, a secret so the public log
  never shows your address, for example
  `["203.0.113.7/32"]`: the addresses production's cluster API admits besides a run's own window,
  as a JSON list, each no wider than /24. Staging needs none: each run opens the control plane to
  its own runner for the run (DEC-2028), and with no address the firewall admits nobody. Set it
  in staging too only if you want `kubectl` from your own machine.

No repository-level secret or variable is used, so no other workflow can read these.

### 4. The first run

Every dispatch takes `sha`: paste the full commit of `main` you read (the repository's commit
page has a copy button). The run refuses unless `main` is still at it, and lists what changed
under `deploy/tofu/`, `infra.yml` and `xtask/` since the last apply, so you approve a commit you
have seen. The log never shows a value: only addresses and actions.

1. **Actions → infra → Run workflow**, branch `main`, environment `staging`, action `preflight`,
   and `sha`. Approve the job. Every line should read `[ok]`; each `[MISSING]` line names the step
   above that is still to do.
2. Dispatch again with action `plan`. Read the plan's list of changes:
   - one VPC, a tag, a firewall and a cluster;
   - the state bucket, imported;
   - a tunnel and its configuration;
   - a DNS record, and an Access application and policy;
   - an R2 bucket and its lock rule, and Barman's token;
   - a `terraform_data` for the budget.

   Nothing is destroyed.
   - If it fails with "DOKS does not offer 1.35.1-do.0; it offers: …", tell lane IAC the list.
     The pin changes in a one-line pull request.
3. Dispatch again with action `apply` on the same commit, and approve it. The job plans again after
   your approval, prints that plan, and applies exactly it, so the apply reflects the commit and
   the cloud as they are when the job runs. Dispatch `apply` only at a commit you have read, and
   read the apply job's printed plan afterwards. A first cluster takes about ten minutes.
4. **Handing over to lane GO (E21-58):** the outputs GO needs are the root outputs
   `archive_access_key_id`, `archive_secret_access_key` and `tunnel_token` (sensitive, never
   printed), and `archive_bucket`, `archive_endpoint`, `archive_destination_path`,
   `archive_jurisdiction` and `cloudflare_account_id`. Once the automated bootstrap lands
   (DEC-2028, lane GO's #1695), `infra.yml`'s apply writes the Secrets `ws-ws1/cloudflared-tunnel`
   and `ws-ws1/journal-archive-key` itself, through kubectl's stdin, and Flux pulls from then on.
   Until then, lane GO's `deploy/gitops/BOOTSTRAP.md` is the by-hand route, run from an address in
   `KUBE_API_ALLOWED_CIDRS`.

## What this does not guard, said plainly

- DOKS manages its own worker firewall, and firewalls add up, so a Service of type
  `LoadBalancer` or `NodePort` would be reachable. Lane GO creates none. The nodes keep public
  addresses for egress.
- The archive's R2 lock is governance-equivalent: the Cloudflare token can lift it (DEC-973 item
  5). It holds against Barman's own token and accidents, which is enough for the WAL archive,
  since journal spec §6.2 does not cover it. No §6.2 object ever goes to R2 or to the Spaces state
  bucket.
- The state holds the tunnel token and Barman's R2 credentials, encrypted with
  `TOFU_STATE_PASSPHRASE`. Whoever holds that passphrase and a Spaces key can read it, and
  `DIGITAL_OCEAN_API_KEY` can mint a Spaces key.
- A run's Spaces key is deleted when the run ends, and again by the workflow's last step, which
  runs however the run ended. Only a runner that dies outright leaves it until the next run of
  that environment deletes it (DEC-2030 item 4). Such a run may also leave its control-plane window
  open until the next run closes it (DEC-2028 item 3).
- The two nodes are in one region, one failure domain (DEC-995 item 6).
- The workflow offers no `destroy`. The buckets have `prevent_destroy`. Tearing an environment
  down is a reviewed change, not a button.
