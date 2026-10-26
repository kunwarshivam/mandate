# Runbook: self-hosted CI runners on DigitalOcean (DEC-2065)

The founder's steps to bring up, switch to, and roll back the self-hosted GitHub Actions runners
that `deploy/tofu/environments/ci-runners` makes. Agents never run any of this and never see a
value below (AGENTS.md rules 7 and 8). Nothing here goes in the repository, a chat or an issue.

**What it is.** Three DigitalOcean droplets (`s-4vcpu-8gb`, `sfo2`), each running one CI job at a
time. Every job gets a fresh container built from a baked image (the repository's pinned toolchain,
Docker, the GitHub runner) and the container is deleted when the job ends. They cost **144 USD a
month** against your cap of **150 USD** ([DEC-2065](../project/decisions/DEC-2065.md)), separate
from the hosting budget of DEC-2011. Only ci.yml's eleven cell jobs move to them, and only for
`main` and for pull requests from this repository's own branches.

**The one rule.** A fork's pull request brings its own workflow files, which can ask for these
runners by label. What stops it is the setting in step 1, and the runners refuse any job that is
not this repository's own before its first step. So **never approve a fork's workflow run while
`CI_RUNNER` is set or the runners are up.** To run one, first unset `CI_RUNNER` and power the
droplets off (step 7).

**The switch.** Nothing changes until you set the repository variable `CI_RUNNER` (step 5).
Rolling back is deleting it (step 7).

## 1. GitHub: verify that forks are held for approval (first)

The runners exist from the moment you apply, before you set `CI_RUNNER`, so confirm this before
anything else. The runners also read it themselves before every registration and register none
while it is anything else.

1. In **Settings → Actions → General → Fork pull request workflows from outside collaborators**,
   select **Require approval for all outside collaborators** and save.
2. Check it took:
   `gh api repos/kunwarshivam/mandate/actions/permissions/fork-pr-contributor-approval` must print
   `"approval_policy":"all_external_contributors"`. Do not go on until it does.

This setting is the fence against code from forks: until you approve a fork's run, none of it runs
anywhere. ci.yml's own guard only keeps this repository's workflow from sending a fork to the
runners, and the runners' job-started hook refuses an approved fork's job before its first step
(DEC-2065 item 4). Keep the rule above.

## 2. DigitalOcean: a team, a billing alert and a token

1. **A separate team for CI.** In the DigitalOcean console, **Create a team** (for example
   `mandate-ci`) and use it for everything below. Its bill and its billing alert then cover the
   runners alone, apart from the hosting budget; its token cannot touch staging or production; and
   only its members can read the droplets' user data, which holds the App's key.
2. **Billing alert at the cap.** In that team, **Billing → Billing alerts**, set an alert at
   **150 USD** a month. No API sets this. The plan's own check also refuses a fleet over 150 USD.
3. **A personal access token**, in **API → Tokens → Generate New Token**, **Custom Scopes**, an
   expiry of 30 days (you only need it while you apply), with only:

   | Resource | Scopes | Why |
   |---|---|---|
   | Droplet | create, read, update, delete | the three runners |
   | Firewall | create, read, update, delete | the runners' firewall: nothing inbound, only DNS, NTP and ports 80 and 443 out |
   | Tag | create, read, delete | the tag the firewall applies to |
   | Sizes | read | the plan's cost check reads the size's price |
   | Regions | read | the plan checks the size is offered in `sfo2` |
   | Image | read | the droplets' Ubuntu 24.04 image |

   If an apply fails with a 403 that names another scope, add only that one. Keep the token in
   your password manager, or regenerate it when you next apply.

## 3. GitHub: the runner App (least privilege)

The droplets need a credential that can register a new single-use runner before every job. A
GitHub App is the least privilege available: each job's token is minted by the droplet, lives one
hour, is narrowed to this one repository and the one permission, and is revoked as soon as the
runner is registered. A fine-grained personal token would need the same permission, but would sit
on every droplet as a long-lived bearer token tied to your account.

1. **Settings → Developer settings → GitHub Apps → New GitHub App.**
   - Name: `mandate-ci-runners` (any free name). Homepage URL: the repository's URL.
   - **Webhook:** untick **Active**.
   - **Repository permissions:** **Administration: Read and write**, and nothing else (Metadata:
     read is added automatically). GitHub requires Administration to register a runner on a
     repository owned by a personal account; there is no narrower permission for it. That is why
     the key stays on the droplets as root only and never reaches a job (DEC-2065 item 5).
   - **Organization and account permissions:** none.
   - **Where can this GitHub App be installed?** Only on this account.
2. **Create**, then on the App's page note the **Client ID**, and under **Private keys** click
   **Generate a private key**. Your browser downloads a `.pem`; keep it in your password manager
   and delete the download after step 4.
3. **Install App → Only select repositories → `mandate`.** The installation's settings page
   address ends in its **installation id** (`.../settings/installations/<number>`); note the
   number.

## 4. Apply, from your own machine

You need OpenTofu `1.13.1` (`bash .cursor/install.sh` installs it, or its GitHub release) and a
checkout of `main`. In a shell, so no value lands in your history or a file:

```bash
cd deploy/tofu/environments/ci-runners
read -rs DIGITALOCEAN_TOKEN && export DIGITALOCEAN_TOKEN       # step 2.3's token
read -rs TF_VAR_state_passphrase && export TF_VAR_state_passphrase   # 32+ random characters; keep it
read -r TF_VAR_github_app_client_id && export TF_VAR_github_app_client_id
read -r TF_VAR_github_app_installation_id && export TF_VAR_github_app_installation_id
TF_VAR_github_app_private_key="$(cat ~/Downloads/mandate-ci-runners.*.private-key.pem)"
export TF_VAR_github_app_private_key
tofu init
tofu apply
```

The plan should create exactly: one `digitalocean_tag`, one `digitalocean_firewall`, three
`digitalocean_droplet`s, and the budget's `terraform_data`, with `estimate_usd = 144`. The boot
script shows as `(sensitive value)`. Type `yes`.

The state is written to `ci-runners.tfstate` in that directory, encrypted with your passphrase.
Keep the passphrase: without it, later applies cannot read the state. Git ignores the state and
`.terraform/` there, so neither is committed by accident; keep a backup of the state with the
passphrase.

**Wait about 20 to 30 minutes.** Each droplet installs Docker and Sysbox, then builds the job image,
which runs the repository's `.cursor/install.sh`. Then **Settings → Actions → Runners** lists one
idle runner per droplet, named `mandate-ci-runner-<n>-<time>`, with the label `mandate-ci-do`. A
new one appears after every job; finished ones remove themselves. If none appears within 40 minutes, check step 1
again: a host registers no runner while the setting reads anything but
`all_external_contributors`.

## 5. Switch the cell jobs over

**Settings → Secrets and variables → Actions → Variables → New repository variable:**
name `CI_RUNNER`, value `mandate-ci-do`.

From the next run on, the eleven cell jobs of a pull request from this repository, and of `main`,
run there. In a cell job's log, **Set up job** names the runner, and the runner's job-started hook
runs first and passes. Every other job stays on GitHub's runners. If this repository's own cell
jobs all fail with "This runner takes only kunwarshivam/mandate's own jobs", roll back (step 7)
and tell lane RN: the hook refuses whatever it cannot read.

## 6. Day to day

- **Nothing to do.** A droplet rebuilds the job image from `main`'s `.cursor/install.sh` once the
  image is a day old, before its next job, so new tool pins arrive by themselves; a job installs
  any pin it still lacks itself. If a rebuild fails, the droplet keeps the old image. Ubuntu's
  unattended upgrades may restart Docker on a droplet now and then; the job it was running fails
  and can be re-run.
- **A change to the fleet, the runner version or the base image** is a pull request to
  `deploy/tofu/`, then `tofu apply` as in step 4. Changing the boot script replaces the droplets.
- **Rotating the App key, every 90 days:** generate a new key on the App's page, apply with it
  (the droplets are replaced), then delete the old key there. Replacing the droplets bills the
  hours both sets run; the cap leaves 6 USD, about 84 droplet-hours, so a month with a replacement
  can come close to the 150 USD alert.
- **No SSH.** The droplets admit nothing inbound and their root password is locked at boot, so
  ignore the password DigitalOcean emails. To look at one, destroy it and apply again.

## 7. Rollback

- **Back to GitHub's runners:** delete the `CI_RUNNER` variable (or set it empty). The next run
  uses `ubuntu-24.04` everywhere. A job already queued for `mandate-ci-do` waits for a runner:
  cancel that run and re-run it.
- **Before approving a fork's workflow run:** delete `CI_RUNNER` and power the three droplets off
  in the console (**Droplets → each → Power → Turn off**); power them on again after the run.
  A powered-off droplet still bills.
- **Stop paying:** after the rollback, run `tofu destroy` in the same directory with the same
  variables, or delete the three droplets, the firewall and the tag in the console. Then delete the
  App's private key (or uninstall the App) and revoke the DigitalOcean token.
- **If a runner misbehaves:** destroy it in the console; the next `tofu apply` makes a fresh one.

## What lands where

| Where | What |
|---|---|
| OpenTofu state (encrypted with your passphrase) | the droplets' ids, names and addresses; the firewall and tag; the cost estimate; and a SHA-1 digest of the boot script, which the DigitalOcean provider stores instead of its text. No variable is stored in state |
| Each droplet | the App's private key at `/etc/mandate-runner/app.pem`, root only (mode 0600), and the client id, installation id and repository in `/etc/mandate-runner/env`. Jobs run in a separate container that never sees either |
| DigitalOcean | the boot script, key included, as the droplets' user data: readable through each droplet's metadata service, and by anyone with Droplet read on the team, in the console and the API. Hence a team of the runners' own, and the 90-day rotation. The host blocks jobs from the metadata service, the host itself and every private range |
| GitHub | each job's one-hour installation token, revoked once its runner is registered, and the runner's single-use registration |

What this does not guard, said plainly, is DEC-2065 item 6.
